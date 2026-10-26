/* /demo: the taker and maker apps side by side on one shared sandbox, with a plain account of what each
   step made public. Visitors use the taker app itself to start a swap; the maker is view only. */
(function () {
  var D = window.DEMO, C = window.Cloakroom, R = window.React, h = R.createElement, useState = R.useState, useEffect = R.useEffect;
  var ORDER = ["RfqSent", "Accepted", "Funded", "Committed", "Revealed", "Claimed"];
  var LABEL = { RfqSent: "Request sent", Accepted: "Quote accepted", Funded: "Both sides locked", Committed: "Claim committed", Revealed: "Share revealed", Claimed: "USDC claimed" };
  var EVENTS = {
    RfqSent: { title: "Request sent", text: "The taker's request goes to the maker encrypted, inside a shielded Zcash transaction.", zec: "One shielded transaction. Nothing readable.", base: "Nothing." },
    Accepted: { title: "Quote accepted", text: "The maker's signed quote comes back the same way. The taker pays a small ZEC premium, which pays the maker for holding the price.", zec: "Shielded transactions only.", base: "Nothing." },
    Funded: { title: "Both sides locked", text: "The maker locks 300 USDC in the escrow. The taker sends 1 ZEC to an address that only both secret shares together can spend.", zec: "One shielded transaction, not linked to the escrow.", base: "The amount, the escrow and a fresh payout address." },
    Committed: { title: "Claim committed", text: "The maker confirms the ZEC and marks the escrow ready. The taker commits to a hash of its share, which locks the escrow so the maker cannot refund once the share is public.", zec: "Nothing new.", base: "A hash." },
    Revealed: { title: "Share revealed", text: "The taker reveals its share to claim the USDC. That share is exactly what the maker needs to spend the ZEC, so both legs settle or neither does.", zec: "Nothing new.", base: "The taker's share. Useless without the maker's own share." },
    Claimed: { title: "USDC claimed", text: "The claim is final: 299 USDC is at the taker's payout address, after the 1 USDC relayer fee. The maker now moves the ZEC to its wallet.", zec: "The maker's sweep: one more shielded transaction.", base: "The payout." },
    Recovering: { title: "Maker refunded", text: "The swap did not complete in time, so the maker took the refund. That revealed the maker's share, and the taker is moving its ZEC back.", zec: "The taker's sweep: one shielded transaction.", base: "The refund and the maker's share." },
    Recovered: { title: "ZEC back with the taker", text: "Neither side lost anything but the premium and fees.", zec: "Nothing new.", base: "Nothing new." },
  };

  // ---------- theme follows the system ----------
  var dark = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");

  // ---------- live sandbox state ----------
  var L = D.live = { sandbox: null, swaps: null, offset: 0, mine: 0 };
  function getJSON(u) { return fetch(u, { cache: "no-store" }).then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); }); }
  D.refresh = function () {
    return Promise.all([getJSON("/demo/api/sandbox"), D.mode === "live" ? getJSON("/demo/api/taker/swaps").catch(function () { return L.swaps; }) : null])
      .then(function (r) { L.sandbox = r[0]; L.swaps = r[1]; L.offset = r[0].now - Date.now() / 1000; L.ok = true; D.emit(); })
      .catch(function () { L.ok = false; D.emit(); });
  };
  D.startedHere = function () { L.mine = Date.now() / 1000; D.refresh(); };
  function serverNow() { return Date.now() / 1000 + L.offset; }

  function stageName(st) { return typeof st === "string" ? st : Object.keys(st || {})[0] || ""; }

  // Stages reached by the swap the page is showing, oldest first.
  function reachedStages() {
    if (D.mode === "replay") return D.marks.filter(function (m) { return m.t <= D.t; }).map(function (m) { return m.stage; });
    var sb = L.sandbox, sw = L.swaps && L.swaps.swaps && L.swaps.swaps[0];
    if (!sb || !sw || (sb.state !== "running" && sb.state !== "cooldown")) return [];
    if (sb.state === "cooldown" && sb.last && sb.last.rfq_id !== sw.rfq_id) return [];
    var n = stageName(sw.stage);
    if (n === "Aborted") return ["Aborted:" + sw.stage.Aborted];
    if (n === "Recovering" || n === "Recovered") return ["RfqSent", "Accepted", "Recovering"].concat(n === "Recovered" ? ["Recovered"] : []);
    var i = ORDER.indexOf(n);
    return ORDER.slice(0, i + 1);
  }

  function Event(p) {
    if (p.stage.indexOf("Aborted:") === 0) {
      return h("div", { className: "ev" }, h("b", null, "Swap stopped"), h("p", null, p.stage.slice(8)));
    }
    var e = EVENTS[p.stage];
    return h("div", { className: "ev" + (p.ghost ? " ghost" : "") },
      h("b", null, (p.ghost ? "Next: " : "") + e.title),
      p.ghost ? null : h(R.Fragment, null,
        h("p", null, e.text),
        h("div", { className: "pub" }, h("span", { className: "tag zec" }, "Zcash"), h("span", null, e.zec), h("span", { className: "tag base" }, "Base"), h("span", null, e.base))));
  }

  function Feed() {
    var reached = reachedStages();
    var running = D.mode === "replay" ? D.t > D.postT && D.t < D.end : L.sandbox && L.sandbox.state === "running";
    var next = ORDER.filter(function (s) { return reached.indexOf(s) < 0; })[0];
    return h("aside", { className: "feed", "aria-live": "polite", "aria-label": "What just happened" },
      h("h3", null, "What just happened"),
      reached.length ? null : h("p", { className: "feed-empty" }, "Each step of the swap appears here, with what it made public on each chain."),
      next && running && reached[0] && reached[0].indexOf("Aborted") < 0 && reached.indexOf("Recovering") < 0 ? h(Event, { key: "next", stage: next, ghost: true }) : null,
      reached.slice().reverse().map(function (s) { return h(Event, { key: s, stage: s }); }));
  }

  function Pane(p) {
    return h("section", { className: "pane pane-" + p.side, "aria-label": p.title },
      h("div", { className: "pane-head" }, h("span", { className: "dot" + (p.live ? " live" : "") }), p.title, p.note ? h("span", { className: "muted" }, p.note) : null),
      h("div", { className: "pane-body" }, h("iframe", { src: "/demo/frame.html?side=" + p.side, title: p.title + " app", id: "frame-" + p.side })));
  }

  function Scrubber() {
    var span = D.total(), at = D.elapsed() / span;
    function seek(e) { var r = e.currentTarget.getBoundingClientRect(); D.seek(D.postT + (e.clientX - r.left) / r.width * span); }
    function key(e) { if (e.key === "ArrowRight") D.seek(D.t + 10); if (e.key === "ArrowLeft") D.seek(D.t - 10); }
    return h("div", { className: "scrub" },
      h("div", { className: "scrub-track", onClick: seek, onKeyDown: key, tabIndex: 0, role: "slider", "aria-label": "Swap time", "aria-valuemin": 0, "aria-valuemax": Math.round(span), "aria-valuenow": Math.round(D.elapsed()), "aria-valuetext": D.mmss(D.elapsed()) },
        h("div", { className: "scrub-fill", style: { width: (at * 100) + "%" } })),
      h("div", { className: "scrub-marks" }, D.marks.map(function (m, i) {
        var x = Math.max(0.04, Math.min(0.96, (m.t - D.postT) / span));
        return h("span", { key: m.stage, className: "scrub-mark" + (i % 2 ? " low" : "") + (D.t >= m.t ? " hit" : ""), style: { left: (x * 100) + "%" } }, LABEL[m.stage] || m.stage);
      })));
  }

  function Control() {
    if (D.mode === "replay") {
      var liveReady = L.ok && L.sandbox && L.sandbox.state !== "starting";
      return h(R.Fragment, null,
        h("div", { className: "control" },
          h("div", { className: "control-text" },
            h("p", null, h("b", null, liveReady ? "The live sandbox is ready." : L.ok ? "The live sandbox is starting (about two minutes)." : "The live sandbox is offline.")),
            h("p", { className: "sub" }, liveReady ? "You are watching a recording. Go live to start a swap yourself." : "Meanwhile, this is a recording of a real swap between the same two apps.")),
          h("div", { className: "control-actions" },
            h("span", { className: "mono" }, D.mmss(D.elapsed()) + " / " + D.mmss(D.total())),
            h("div", { className: "seg", role: "group", "aria-label": "Playback speed" }, [1, 10, 30].map(function (s) {
              return h("button", { key: s, type: "button", "aria-pressed": D.speed === s, onClick: function () { D.speed = s; D.emit(); } }, s + "×");
            })),
            h(C.Button, { id: "play", variant: liveReady ? "secondary" : "primary", onClick: function () { D.playing ? D.pause() : D.play(); } }, D.playing ? "Pause" : D.t >= D.end ? "Replay" : "Play"),
            liveReady ? h(C.Button, { id: "golive", onClick: function () { location.reload(); } }, "Go live") : null)),
        h(Scrubber));
    }
    var sb = L.sandbox || {}, now = serverNow(), text, sub;
    if (sb.state === "running") {
      text = "Swap running · " + D.mmss(now - sb.started_at);
      sub = (L.mine && L.mine >= sb.started_at - 5 ? "You started this swap." : "Another visitor started this swap; you are watching it live.") + " It takes about four minutes, most of it waiting for each chain to be final.";
    } else if (sb.state === "cooldown") {
      var ok = sb.last && sb.last.stage === "Claimed";
      text = ok ? "Settled in " + D.mmss(sb.last.secs) + "." : "Swap ended" + (sb.last ? ": " + sb.last.stage : "") + ".";
      sub = "The next swap can start in " + D.mmss(sb.cooldown_until - now) + ".";
    } else {
      text = "The sandbox is free.";
      sub = "Press Start private swap in the taker app to sell 1 test ZEC. Everyone on this page watches the same swap.";
    }
    return h("div", { className: "control" },
      h("div", { className: "control-text", "aria-live": "polite" }, h("p", null, h("b", null, text)), h("p", { className: "sub" }, sub)),
      sb.state === "idle" ? h("div", { className: "control-hint" }, h(C.Icon, { name: "arrow", size: 16 }), "Start it in the taker app") : null);
  }

  function App() {
    var s = useState(0), bump = s[1];
    useEffect(function () { return D.on(function () { bump(function (n) { return n + 1; }); }); }, []);
    var live = D.mode === "live", running = live ? L.sandbox && L.sandbox.state === "running" : D.playing;
    return h("div", { className: "shell" },
      h("header", { className: "bar" },
        h("a", { href: "/", className: "bar-logo", "aria-label": "Cloakroom, about" }, h(C.Logo, { size: 22 })), h("div", { className: "bar-sep" }),
        h(C.StatusChip, { tone: live ? "final" : "waiting" }, live ? "Live" : "Recording"),
        h("div", { className: "bar-title" }, h("b", null, "Shared sandbox"), h("span", null, "Real Zebra and anvil chains on the demo server · test coins only")),
        h("div", { className: "bar-right" },
          h("span", { className: "mono bar-note" }, "finality: about 1 min here, about 25 min on Base"),
          h("a", { href: "/", className: "bar-link" }, "About Cloakroom"))),
      h(Control),
      h("main", { className: "stage" },
        h(Pane, { side: "taker", title: "Taker", note: live ? "you can use it" : "recording", live: running }),
        h(Pane, { side: "maker", title: "Maker", note: "view only", live: running }),
        h(Feed)));
  }

  function render() {
    if (new URLSearchParams(location.search).get("video") === "1") D.ov = D.overlay();
    ReactDOM.createRoot(document.getElementById("root")).render(h(App));
    setInterval(function () { D.refresh(); }, 1500);
    setInterval(function () { if (D.mode === "live") D.emit(); }, 1000);
  }

  // Live if the sandbox answers and has finished starting; otherwise play the recording.
  var forceReplay = new URLSearchParams(location.search).get("replay") === "1";
  Promise.all([D.load(), forceReplay ? null : getJSON("/demo/api/sandbox").catch(function () { return null; })])
    .then(function (r) {
      var sb = r[1];
      D.mode = sb && sb.state !== "starting" ? "live" : "replay";
      if (sb) { L.sandbox = sb; L.ok = true; L.offset = sb.now - Date.now() / 1000; }
      if (D.mode === "replay") D.speed = 10;
      return D.mode === "live" ? D.refresh() : null;
    }, function () { D.mode = "replay"; })
    .then(render);
})();
