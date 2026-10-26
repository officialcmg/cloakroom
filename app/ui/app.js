/* Cloakroom local app: one screen for the taker, one for the maker, built from window.Cloakroom. */
(function () {
  var C = window.Cloakroom, R = window.React, h = R.createElement;
  var useState = R.useState, useEffect = R.useEffect;

  // ---------- data ----------
  function api(path, body) {
    var opts = { headers: { "X-Cloakroom": "1" } };
    if (body !== undefined) { opts.method = "POST"; opts.headers["Content-Type"] = "application/json"; opts.body = JSON.stringify(body); }
    return fetch(path, opts).then(function (r) {
      return r.json().then(function (j) { if (!r.ok) throw new Error(j.error || r.statusText); return j; });
    });
  }
  function usePoll(path, ms) {
    var s = useState(null), data = s[0], set = s[1];
    useEffect(function () {
      var alive = true;
      function tick() { api(path).then(function (j) { if (alive) set(j); }).catch(function () {}); }
      tick(); var t = setInterval(tick, ms);
      return function () { alive = false; clearInterval(t); };
    }, [path, ms]);
    return data;
  }

  // ---------- formatting ----------
  function zec(zats) { if (zats == null) return "—"; var v = zats / 1e8; return v.toFixed(v > 0 && v < 0.1 ? 4 : 2); }
  function usdc(units) { if (units == null) return "—"; return (units / 1e6).toFixed(2); }
  function eth(wei) { if (wei == null) return "—"; return (Number(wei) / 1e18).toFixed(4); }
  function clock(t) { if (!t) return ""; var d = new Date(t * 1000); return d.toTimeString().slice(0, 5); }
  function stageName(st) { return typeof st === "string" ? st : Object.keys(st || {})[0] || ""; }
  function abortReason(st) { return typeof st === "object" && st ? st.Aborted : null; }
  // Plain-language "what is happening now" for each stage; the raw lines stay in the technical log.
  var TAKER_NOW = {
    RfqSent: "Waiting for the maker's quote.",
    Accepted: "Waiting for the maker to lock the USDC on Base, then your ZEC goes to the joint address.",
    Funded: "Your ZEC is in the joint address. Waiting for the maker to confirm it and mark the escrow ready.",
    Committed: "Your claim is locked in. Waiting for it to be final before you reveal your share.",
    Revealed: "Claim sent. Waiting for it to confirm.",
    Recovering: "The maker took a refund, which gave you their share. Moving the ZEC back to your wallet.",
  };
  var MAKER_NOW = {
    Quoted: "Quote sent. Waiting for the taker's premium.",
    Opened: "USDC locked. Waiting for the taker's ZEC to confirm.",
    Ready: "Escrow ready. Waiting for the taker to claim.",
    Sweeping: "The taker claimed. Moving the ZEC to your wallet.",
    RefundSent: "Refund sent. Waiting for it to confirm.",
    Refunded: "USDC refunded to you.",
    Swept: "ZEC received. Swap settled.",
  };
  function lastLine(s) { var l = (s.log || [])[s.log ? s.log.length - 1 : 0] || ""; return l.length > 160 ? l.slice(0, 157) + "…" : l; }
  function explorer(status, kind, v) {
    if (!v || status.network !== "test") return undefined;
    return "https://sepolia.basescan.org/" + kind + "/" + v;
  }

  // ---------- theme ----------
  function initialTheme() {
    try { var t = localStorage.getItem("cr-theme"); if (t === "light" || t === "dark") return t; } catch (e) {}
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  function ThemeToggle() {
    var s = useState(initialTheme()), theme = s[0], set = s[1];
    useEffect(function () { document.documentElement.setAttribute("data-theme", theme); try { localStorage.setItem("cr-theme", theme); } catch (e) {} }, [theme]);
    return h("button", { type: "button", className: "app-theme", onClick: function () { set(theme === "dark" ? "light" : "dark"); } }, theme === "dark" ? "Light" : "Dark");
  }

  function Header(p) {
    var st = p.status;
    return h("header", { className: "app-head" },
      h(C.Logo, { size: 26 }),
      h("div", { className: "app-head-right" },
        st && st.balance && st.balance.zats != null ? h("span", { className: "app-pill" }, zec(st.balance.zats) + " ZEC shielded") : h("span", { className: "app-pill" }, "Syncing wallet…"),
        st ? h(C.StatusChip, { tone: st.running ? "final" : "waiting" }, st.role === "maker" ? (st.running ? "Quoting" : "Paused") : (st.running ? "Swap running" : "Idle")) : null,
        h(ThemeToggle)));
  }

  function LogDetails() {
    var log = usePoll("/api/log", 4000);
    if (!log || !log.lines || !log.lines.length) return null;
    return h("details", { className: "app-details" }, h("summary", null, "Technical log"), h("pre", null, log.lines.join("\n")));
  }

  // ---------- taker ----------
  var TAKER_ORDER = ["RfqSent", "Accepted", "Funded", "Committed", "Revealed", "Claimed"];
  function takerChip(s) {
    var n = stageName(s.stage);
    if (n === "Claimed") return h(C.StatusChip, { tone: "final" });
    if (n === "Recovered") return h(C.StatusChip, { tone: "final" }, "Recovered");
    if (n === "Recovering") return h(C.StatusChip, { tone: "refund" });
    if (n === "Aborted") return h(C.StatusChip, { tone: "danger" }, "Stopped");
    if (n === "Funded" || n === "Committed" || n === "Revealed") return h(C.StatusChip, { tone: "locked" });
    return h(C.StatusChip, { tone: "waiting" });
  }
  function takerSteps(s, status) {
    var n = stageName(s.stage), idx = TAKER_ORDER.indexOf(n), price = s.price != null ? usdc(s.price) + " USDC" : "the USDC";
    var lot = status.params.lot_zats * (s.lots || 1);
    var labels = [
      "Request sent, encrypted",
      "Quote checked and accepted, premium paid",
      "Maker locks " + price + ", you fund " + zec(lot) + " ZEC",
      "Maker confirms your ZEC, you lock the claim",
      "You reveal your share to claim",
      "USDC claimed",
    ];
    var detail = TAKER_NOW[n];
    if (n === "Recovering" || n === "Recovered") {
      return [
        { label: labels[0], state: "done" }, { label: labels[1], state: "done" },
        { label: "Maker refunded", state: "done" },
        { label: "Sweeping your ZEC back", state: n === "Recovered" ? "done" : "now", detail: n === "Recovering" ? detail : undefined },
        { label: "ZEC recovered", state: n === "Recovered" ? "done" : "todo" },
      ];
    }
    var reason = abortReason(s.stage);
    var at = reason ? Math.max(0, (s.fund_txid ? 3 : s.premium != null ? 2 : 1)) : idx + 1;
    return labels.map(function (l, i) {
      if (reason) return { label: l, state: i < at ? "done" : i === at ? "failed" : "todo", detail: i === at ? reason : undefined };
      if (n === "Claimed") return { label: l, state: "done" };
      return { label: l, state: i < at ? "done" : i === at ? "now" : "todo", detail: i === at ? (detail || "Working…") : undefined };
    });
  }
  function terminal(s) { var n = stageName(s.stage); return n === "Claimed" || n === "Recovered" || n === "Aborted"; }

  function TakerSwapView(p) {
    var s = p.swap, status = p.status, lot = status.params.lot_zats * (s.lots || 1);
    var receive = s.price != null ? usdc(s.price - (s.relayer_fee || 0)) : "…";
    return h("section", { className: "app-section" },
      h(C.SwapCard, {
        title: "Swap " + s.rfq_id.slice(0, 8), status: takerChip(s),
        sell: { label: "You sell", value: zec(lot), unit: "ZEC" },
        receive: { label: "You get", value: receive, unit: "USDC" },
        meta: s.price != null ? "Quote " + usdc(s.price) + " · premium " + zec(s.premium) + " ZEC · relayer " + usdc(s.relayer_fee) : "Waiting for the maker's quote",
      }),
      h(C.Timeline, { steps: takerSteps(s, status) }),
      !terminal(s) ? h("p", { className: "app-foot" }, "You can close this window. The swap keeps running in the Cloakroom service, and your ZEC is safe at every step.") : null,
      h("div", { className: "app-links" },
        s.escrow_id ? h(C.Hash, { label: "Escrow", value: s.escrow_id, href: explorer(status, "address", status.params.escrow) }) : null,
        s.fund_txid ? h(C.Hash, { label: "Funding", value: s.fund_txid, head: 8 }) : null,
        s.claim_tx ? h(C.Hash, { label: "Claim", value: s.claim_tx, href: explorer(status, "tx", s.claim_tx) }) : null),
      h(C.Notice, { tone: "info", title: "Visible on Base" }, "The amount, the escrow and your fresh payout address. Not your Zcash wallet, and not which Zcash transaction paid."));
  }

  function TakerHome(p) {
    var status = p.status, swaps = p.swaps, lotZats = status.params.lot_zats;
    var s1 = useState(zec(lotZats)), amount = s1[0], setAmount = s1[1];
    var s2 = useState(null), err = s2[0], setErr = s2[1];
    var s3 = useState(false), busy = s3[0], setBusy = s3[1];
    var active = swaps.filter(function (s) { return !terminal(s); })[0];
    var past = swaps.filter(terminal);
    var lots = Math.round(parseFloat(amount) * 1e8 / lotZats);
    var bal = status.balance && status.balance.zats;
    var problem = !(lots >= 1) ? "Enter at least " + zec(lotZats) + " ZEC." :
      Math.abs(lots * lotZats - parseFloat(amount) * 1e8) > 1 ? "Swaps come in lots of " + zec(lotZats) + " ZEC." :
      lots > 255 ? "At most 255 lots per swap." :
      bal != null && lots * lotZats > bal ? "More than your shielded balance of " + zec(bal) + " ZEC." : null;
    var maker = status.params.maker_evm ? "Maker " + status.params.maker_evm.slice(0, 6) + "…" + status.params.maker_evm.slice(-4) + " · signed directory" : null;
    var privateOn = status.params.cover_max_secs > 0;
    function submit() {
      if (problem) { setErr(problem); return; }
      setBusy(true); setErr(null);
      api("/api/swaps", { lots: lots }).catch(function (e) { setErr(e.message); }).then(function () { setBusy(false); });
    }
    return h(R.Fragment, null,
      active ? h(TakerSwapView, { swap: active, status: status }) :
      status.running ? h(C.Notice, { tone: "info", title: "Sending your request" }, "The encrypted request is on its way to the maker. The quote usually arrives within a few blocks.") :
      h("section", { className: "app-section" },
        h(C.QuoteForm, {
          id: "sell", sell: amount, onSellChange: function (e) { setAmount(e.target.value); setErr(null); },
          balance: bal != null ? zec(bal) + " ZEC" : null, maker: maker,
          privacy: privateOn ? "private" : "fast", onPrivacyChange: function () {},
          privacyOptions: privateOn ? [{ value: "private", label: "Private", detail: "cover delays on" }] : [{ value: "fast", label: "Fast", detail: "cover delays off" }],
          privacyNote: privateOn ? "Random delays hide which Zcash transactions belong to this swap." : "This setup has cover delays off, so the timing of your Zcash transactions is not hidden. Turn them on in the config for real use.",
          action: busy ? "Starting…" : "Start private swap", onSubmit: submit, disabled: busy, error: err,
        }),
        h("p", { className: "app-foot" }, "Swaps come in lots of " + zec(lotZats) + " ZEC. The maker's quote is accepted automatically if it is within your limits" + (status.params.max_price ? " (at most " + usdc(status.params.max_price) + " USDC per lot)" : "") + ".")),
      past.length ? h("section", { className: "app-section" }, h("h2", null, "Earlier swaps"),
        h("div", { role: "table", "aria-label": "Earlier swaps", className: "cr-card app-table" }, past.map(function (s) {
          return h(C.SwapRow, { key: s.rfq_id, id: s.rfq_id, sell: zec(lotZats * (s.lots || 1)) + " ZEC", receive: s.price != null ? usdc(s.price - (s.relayer_fee || 0)) + " USDC" : "—", step: abortReason(s.stage) || (stageName(s.stage) === "Recovered" ? "ZEC back in your wallet." : "USDC received."), deadline: clock(s.updated), tone: stageName(s.stage) === "Aborted" ? "danger" : "final", statusLabel: stageName(s.stage) === "Aborted" ? "Stopped" : stageName(s.stage) === "Recovered" ? "Recovered" : undefined });
        }))) : null);
  }

  // ---------- maker ----------
  function makerTone(n) {
    return { Quoted: "waiting", Opened: "locked", Ready: "locked", Sweeping: "locked", RefundSent: "refund", Refunded: "refund", Swept: "final", Aborted: "danger" }[n] || "waiting";
  }
  function makerLabel(n) {
    return { Quoted: "Quoted", Opened: "Locked", Ready: "Ready", Sweeping: "Sweeping", RefundSent: "Refunding", Refunded: "Refunded", Swept: "Settled", Aborted: "Stopped" }[n];
  }
  function MakerHome(p) {
    var status = p.status, swaps = p.swaps, prm = status.params, now = status.now;
    var open = swaps.filter(function (s) { var n = stageName(s.stage); return n === "Opened" || n === "Ready"; });
    var locked = open.reduce(function (a, s) { return a + (s.price || 0); }, 0);
    var alarms = open.filter(function (s) { return s.t1 && s.t2; }).map(function (s) {
      var from = s.t1 + prm.inclusion_secs + prm.finality_secs, to = s.t2 - prm.budget_secs;
      var state = now < from ? "upcoming" : "open";
      var mins = Math.max(0, Math.round(((state === "upcoming" ? from : to) - now) / 60));
      return h(C.LivenessAlarm, { key: s.rfq_id, state: state, window: clock(from) + " to " + clock(to), swapId: s.rfq_id,
        countdown: state === "upcoming" ? "Opens in " + mins + " min" : mins + " min left" },
        state === "upcoming" ? "Keep Cloakroom running then, so it can refund if the taker never claims. If you are offline for the whole window, the taker can claim the USDC." : "Do not close Cloakroom or let the computer sleep.");
    });
    var sorted = swaps.slice().sort(function (a, b) { return (a.t2 || 9e12) - (b.t2 || 9e12); });
    return h(R.Fragment, null,
      alarms,
      h("div", { className: "app-grid" },
        h("div", { className: "app-section" },
          h(C.InventoryCard, { title: "Inventory", status: h(C.StatusChip, { tone: status.running ? "final" : "waiting" }, status.running ? "Quoting" : "Paused"), items: [
            { label: "USDC on Base", value: usdc(status.evm && status.evm.token_units), unit: "USDC", note: locked ? usdc(locked) + " locked in " + open.length + " open swap" + (open.length > 1 ? "s" : "") : "Nothing locked" },
            { label: "Shielded ZEC", value: zec(status.balance && status.balance.zats), unit: "ZEC" },
            { label: "Gas", value: eth(status.evm && status.evm.eth_wei), unit: "ETH" },
          ] }),
          h("div", { className: "app-row-actions" },
            status.running ? h(C.Button, { variant: "secondary", onClick: function () { api("/api/stop", {}); } }, "Stop quoting") : h(C.Button, { onClick: function () { api("/api/maker/start", {}); } }, "Start quoting")),
          h("p", { className: "app-foot" }, "Quoting " + usdc(prm.price_per_lot) + " USDC per " + zec(prm.lot_zats) + " ZEC lot, premium " + zec(prm.premium_zats) + " ZEC, relayer fee " + usdc(prm.relayer_fee) + " USDC.")),
        h("div", { className: "app-section" }, h("h2", null, "Swaps"),
          h("div", { role: "table", "aria-label": "Swaps", className: "cr-card app-table" },
            sorted.length ? sorted.map(function (s) {
              var n = stageName(s.stage);
              var deadline = (n === "Opened" || n === "Ready") && s.t2 ? "refund window ends " + clock(s.t2 - prm.budget_secs) : clock(s.updated);
              return h(C.SwapRow, { key: s.rfq_id, id: s.rfq_id, sell: zec(prm.lot_zats * (s.lots || 1)) + " ZEC", receive: usdc(s.price) + " USDC", step: abortReason(s.stage) || MAKER_NOW[n] || lastLine(s), deadline: deadline, tone: makerTone(n), statusLabel: makerLabel(n) });
            }) : h("p", { className: "app-empty" }, status.running ? "Waiting for requests." : "Start quoting to receive requests.")))));
  }

  // ---------- app ----------
  function App() {
    var status = usePoll("/api/status", 3000), swapsResp = usePoll("/api/swaps", 3000);
    var swaps = (swapsResp && swapsResp.swaps) || [];
    if (!status) return h("main", { className: "app" }, h(C.Logo, { size: 26 }), h("p", { className: "app-foot" }, "Connecting to the Cloakroom service…"));
    var maker = status.role === "maker";
    return h("main", { className: maker ? "app app-wide" : "app" },
      h(Header, { status: status }),
      status.last_exit && !status.running ? h(C.Notice, { tone: "info", title: "Stopped" }, status.last_exit) : null,
      maker ? h(MakerHome, { status: status, swaps: swaps }) : h(TakerHome, { status: status, swaps: swaps }),
      h(LogDetails),
      h("p", { className: "app-foot" }, (window.CLOAKROOM_WHERE || "Running locally on this computer. ") + (status.network === "test" ? "Zcash testnet and Base Sepolia." : "Local test chains (Zebra regtest and anvil).")));
  }

  ReactDOM.createRoot(document.getElementById("root")).render(h(App));
})();
