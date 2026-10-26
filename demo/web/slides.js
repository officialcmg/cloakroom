/* Slide navigation: arrows, dots, keys, swipe, and #2-style links. Narrow screens get a plain column. */
(function () {
  var track = document.getElementById("track"), slides = track.children, n = slides.length;
  var dots = document.getElementById("dots"), prev = document.getElementById("prev"), next = document.getElementById("next");
  var at = 0;
  document.documentElement.setAttribute("data-theme", window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  document.getElementById("of").textContent = n;
  for (var i = 0; i < n; i++) {
    var b = document.createElement("button");
    b.type = "button"; b.setAttribute("aria-label", "Slide " + (i + 1));
    b.addEventListener("click", go.bind(null, i));
    dots.appendChild(b);
  }
  function go(i) {
    at = Math.max(0, Math.min(n - 1, i));
    track.style.transform = "translateX(" + (-100 * at) + "vw)";
    for (var j = 0; j < n; j++) {
      dots.children[j].setAttribute("aria-current", j === at ? "true" : "false");
      slides[j].setAttribute("aria-hidden", j === at ? "false" : "true");
      slides[j].inert = j !== at && !column();
    }
    prev.disabled = at === 0; next.disabled = at === n - 1;
    document.getElementById("at").textContent = at + 1;
    if (location.hash !== "#" + (at + 1)) history.replaceState(null, "", at ? "#" + (at + 1) : location.pathname);
  }
  function column() { return window.matchMedia("(max-width: 860px), (max-height: 620px)").matches; }
  prev.addEventListener("click", function () { go(at - 1); });
  next.addEventListener("click", function () { go(at + 1); });
  document.querySelectorAll("[data-go]").forEach(function (el) {
    el.addEventListener("click", function () {
      var i = +el.getAttribute("data-go");
      if (column()) slides[i].scrollIntoView({ behavior: "smooth" }); else go(i);
    });
  });
  document.addEventListener("keydown", function (e) {
    if (column() || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === "ArrowRight" || e.key === "PageDown" || (e.key === " " && !/^(BUTTON|A)$/.test(e.target.tagName))) { e.preventDefault(); go(at + 1); }
    if (e.key === "ArrowLeft" || e.key === "PageUp") { e.preventDefault(); go(at - 1); }
    if (e.key === "Home") go(0);
    if (e.key === "End") go(n - 1);
  });
  var x0 = null;
  track.addEventListener("touchstart", function (e) { x0 = e.touches[0].clientX; }, { passive: true });
  track.addEventListener("touchend", function (e) {
    if (x0 == null || column()) return;
    var dx = e.changedTouches[0].clientX - x0; x0 = null;
    if (Math.abs(dx) > 50) go(at + (dx < 0 ? 1 : -1));
  });
  window.addEventListener("resize", function () { go(at); });
  var h = parseInt((location.hash || "").slice(1), 10);
  go(h >= 1 && h <= n ? h - 1 : 0);
})();
