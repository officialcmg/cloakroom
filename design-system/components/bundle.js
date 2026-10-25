/* @ds-bundle: {"format":4,"namespace":"Cloakroom","components":[{"name":"Button"},{"name":"StatusChip"},{"name":"AmountRow"},{"name":"SwapCard"},{"name":"Timeline"},{"name":"Notice"},{"name":"Hash"},{"name":"AmountInput"},{"name":"QuoteForm"},{"name":"InventoryCard"},{"name":"SwapRow"},{"name":"LivenessAlarm"},{"name":"Logo"},{"name":"Icon"}]} */
(function () {
  var R = window.React, h = R.createElement;
  function cx() { return Array.prototype.filter.call(arguments, Boolean).join(" "); }
  function omit(p, keys) { var o = {}; for (var k in p) if (keys.indexOf(k) < 0) o[k] = p[k]; return o; }

  function Button(p) {
    var v = p.variant || "primary";
    return h("button", Object.assign({ type: "button" }, omit(p, ["variant", "className", "children"]), { className: cx("cr-btn", "cr-btn-" + v, p.className) }), p.children);
  }

  var CHIP = { final: "Final", locked: "Locked", waiting: "Waiting", refund: "Refunding", danger: "Action needed" };
  function StatusChip(p) {
    var t = p.tone || "waiting";
    return h("span", { className: "cr-chip cr-chip-" + t }, p.children || CHIP[t]);
  }

  function AmountRow(p) {
    return h("div", { className: cx("cr-amount", p.size === "small" && "cr-amount-small") },
      h("span", { className: "cr-label" }, p.label),
      h("span", { className: "cr-amount-v" }, p.value, h("small", null, p.unit)));
  }

  function SwapCard(p) {
    return h("section", { className: "cr-swap", "aria-label": p.title || "Swap" },
      p.status ? h("div", { className: "cr-swap-head" }, h("span", { className: "cr-label" }, p.title || "Swap"), p.status) : null,
      h(AmountRow, { label: (p.sell && p.sell.label) || "Sell", value: p.sell.value, unit: p.sell.unit }),
      h("div", { className: "cr-swap-rule" }),
      h(AmountRow, { label: (p.receive && p.receive.label) || "Get", value: p.receive.value, unit: p.receive.unit }),
      p.meta ? h("p", { className: "cr-caption" }, p.meta) : null,
      p.action ? h(Button, { variant: "primary", onClick: p.onAction, disabled: p.disabled, className: "cr-btn-block" }, p.action) : null);
  }

  function Timeline(p) {
    return h("ol", { className: "cr-tl" }, (p.steps || []).map(function (s, i) {
      var st = s.state || "todo";
      return h("li", { key: i, className: "cr-tl-step cr-tl-" + st, "aria-current": st === "now" ? "step" : undefined },
        h("span", { className: "cr-tl-dot", "aria-hidden": "true" }),
        h("span", { className: "cr-tl-text" }, s.label, s.detail ? h("span", { className: "cr-tl-detail" }, s.detail) : null),
        s.time ? h("time", { className: "cr-tl-time" }, s.time) : null);
    }));
  }

  function Notice(p) {
    var t = p.tone || "info";
    return h("div", { className: "cr-notice cr-notice-" + t, role: t === "danger" ? "alert" : "note" },
      p.title ? h("p", { className: "cr-notice-title" }, p.title) : null,
      h("div", { className: "cr-notice-body" }, p.children));
  }

  function Hash(p) {
    var v = String(p.value || ""), head = p.head == null ? 6 : p.head, tail = p.tail == null ? 4 : p.tail;
    var short = v.length > head + tail + 1 ? v.slice(0, head) + "…" + v.slice(v.length - tail) : v;
    var inner = [p.label ? h("span", { key: "l", className: "cr-hash-label" }, p.label) : null, h("span", { key: "v" }, short)];
    return p.href
      ? h("a", { className: "cr-hash", href: p.href, title: v, target: "_blank", rel: "noreferrer" }, inner)
      : h("span", { className: "cr-hash", title: v }, inner);
  }

  var ICONS = {
    shield: [["path", { d: "M12 3l7 3v5c0 4.5-3 8.2-7 10-4-1.8-7-5.5-7-10V6z" }]],
    lock: [["rect", { x: 5, y: 11, width: 14, height: 10, rx: 2 }], ["path", { d: "M8 11V8a4 4 0 0 1 8 0v3" }]],
    clock: [["circle", { cx: 12, cy: 12, r: 9 }], ["path", { d: "M12 7v5l3 2" }]],
    check: [["path", { d: "M5 12.5l4.5 4.5L19 7.5" }]],
    "return": [["path", { d: "M9 14L4 9l5-5" }], ["path", { d: "M4 9h10a6 6 0 0 1 0 12h-3" }]],
    alert: [["path", { d: "M12 3L2 20h20z" }], ["path", { d: "M12 10v4" }], ["path", { d: "M12 17h.01" }]],
    copy: [["rect", { x: 8, y: 8, width: 12, height: 12, rx: 2 }], ["path", { d: "M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3" }]],
    external: [["path", { d: "M14 4h6v6" }], ["path", { d: "M20 4l-9 9" }], ["path", { d: "M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" }]],
    arrow: [["path", { d: "M5 12h14" }], ["path", { d: "M13 6l6 6-6 6" }]]
  };
  function Icon(p) {
    var shapes = ICONS[p.name] || [], size = p.size || 20;
    var props = { className: cx("cr-icon", p.className), width: size, height: size, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round" };
    if (p.title) { props.role = "img"; props["aria-label"] = p.title; } else { props["aria-hidden"] = "true"; }
    return h("svg", props, shapes.map(function (s, i) { return h(s[0], Object.assign({ key: i }, s[1])); }));
  }

  function Logo(p) {
    var size = p.size || 28;
    return h("span", { className: "cr-logo", style: { fontSize: Math.round(size * 0.82) + "px" } },
      h("svg", { className: "cr-logo-mark", width: size * 1.75, height: size, viewBox: "0 0 56 32", "aria-hidden": "true" },
        h("rect", { x: 2, y: 4, width: 52, height: 24, rx: 12 }),
        h("circle", { cx: 16, cy: 16, r: 4 })),
      p.markOnly ? null : h("span", { className: "cr-logo-word" }, "cloakroom"));
  }

  function AmountInput(p) {
    var id = p.id || "cr-amount-input";
    return h("div", { className: cx("cr-field", p.error && "cr-field-error") },
      h("label", { className: "cr-label", htmlFor: id }, p.label),
      h("div", { className: "cr-field-box" },
        h("input", { id: id, className: "cr-field-input", inputMode: "decimal", autoComplete: "off", value: p.value, placeholder: p.placeholder || "0.00", onChange: p.onChange, "aria-invalid": p.error ? "true" : undefined, "aria-describedby": (p.error || p.hint) ? id + "-msg" : undefined }),
        h("span", { className: "cr-field-unit" }, p.unit)),
      (p.error || p.hint) ? h("p", { id: id + "-msg", className: p.error ? "cr-field-msg cr-field-msg-error" : "cr-field-msg" }, p.error || p.hint) : null);
  }

  function QuoteForm(p) {
    var choices = p.privacyOptions || [{ value: "fast", label: "Fast", detail: "~1.5 h" }, { value: "private", label: "Private", detail: "~12 h" }];
    return h("form", { className: "cr-swap cr-quote", onSubmit: function (e) { e.preventDefault(); if (p.onSubmit) p.onSubmit(); } },
      h(AmountInput, { id: p.id || "cr-sell", label: "Sell", value: p.sell, unit: p.sellUnit || "ZEC", onChange: p.onSellChange, hint: p.balance ? "Shielded balance " + p.balance : null, error: p.error }),
      p.receive ? h(AmountRow, { label: "You get about", value: p.receive, unit: p.receiveUnit || "USDC", size: "small" }) : null,
      p.maker ? h("p", { className: "cr-quote-maker" }, h(Icon, { name: "check", size: 16 }), h("span", null, p.maker)) : null,
      h("fieldset", { className: "cr-choice" },
        h("legend", { className: "cr-label" }, "Privacy"),
        h("div", { className: "cr-choice-row" }, choices.map(function (c) {
          var on = p.privacy === c.value;
          return h("label", { key: c.value, className: cx("cr-choice-opt", on && "cr-choice-on") },
            h("input", { type: "radio", name: (p.id || "cr") + "-privacy", value: c.value, checked: on, onChange: function () { if (p.onPrivacyChange) p.onPrivacyChange(c.value); } }),
            h("span", { className: "cr-choice-label" }, c.label), h("span", { className: "cr-choice-detail" }, c.detail));
        })),
        p.privacyNote ? h("p", { className: "cr-caption" }, p.privacyNote) : null),
      h(Button, { variant: "primary", type: "submit", className: "cr-btn-block", disabled: p.disabled }, p.action || "Get quote"));
  }

  function InventoryCard(p) {
    return h("section", { className: "cr-card cr-inventory", "aria-label": p.title || "Inventory" },
      h("div", { className: "cr-card-head" }, h("span", { className: "cr-label" }, p.title || "Inventory"), p.status || null),
      (p.items || []).map(function (it, i) {
        return h("div", { key: i, className: "cr-inventory-row" },
          h(AmountRow, { label: it.label, value: it.value, unit: it.unit, size: "small" }),
          it.note ? h("p", { className: "cr-caption" }, it.note) : null);
      }));
  }

  function SwapRow(p) {
    return h("div", { className: "cr-swaprow", role: "row" },
      h("span", { className: "cr-swaprow-id", role: "cell" }, h(Hash, { value: p.id, head: 8, tail: 0 })),
      h("span", { className: "cr-swaprow-pair", role: "cell" }, p.sell, h(Icon, { name: "arrow", size: 14 }), p.receive),
      h("span", { className: "cr-swaprow-step", role: "cell" }, p.step),
      h("span", { className: "cr-swaprow-deadline", role: "cell" }, p.deadline),
      h("span", { className: "cr-swaprow-status", role: "cell" }, h(StatusChip, { tone: p.tone || "waiting" }, p.statusLabel)));
  }

  var ALARM = { upcoming: "Be online for this swap", open: "Stay online now", done: "Window passed safely" };
  function LivenessAlarm(p) {
    var st = p.state || "upcoming";
    return h("div", { className: "cr-alarm cr-alarm-" + st, role: st === "open" ? "alert" : "status" },
      h(Icon, { name: st === "done" ? "check" : "alert", size: 22 }),
      h("div", { className: "cr-alarm-text" },
        h("p", { className: "cr-alarm-title" }, (p.title || ALARM[st]) + (p.window ? " · " + p.window : "")),
        h("p", { className: "cr-alarm-body" }, p.children),
        p.countdown ? h("p", { className: "cr-alarm-count" }, p.countdown) : null),
      p.swapId ? h(Hash, { value: p.swapId, head: 8, tail: 0, label: "Swap" }) : null);
  }

  window.Cloakroom = Object.assign(window.Cloakroom || {}, { Button: Button, StatusChip: StatusChip, AmountRow: AmountRow, SwapCard: SwapCard, Timeline: Timeline, Notice: Notice, Hash: Hash, AmountInput: AmountInput, QuoteForm: QuoteForm, InventoryCard: InventoryCard, SwapRow: SwapRow, LivenessAlarm: LivenessAlarm, Logo: Logo, Icon: Icon });
})();
