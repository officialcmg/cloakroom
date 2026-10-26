/* Light or dark for the whole site: the visitor's choice (kept under the same key the app uses), else
   the system setting. Applies to this page and to any app frames inside it. */
(function () {
  var KEY = "cr-theme", subs = [];
  function stored() { try { var t = localStorage.getItem(KEY); return t === "light" || t === "dark" ? t : null; } catch (e) { return null; } }
  function system() { return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"; }
  var theme = stored() || system();
  function apply() {
    document.documentElement.setAttribute("data-theme", theme);
    Array.prototype.forEach.call(document.querySelectorAll("iframe"), function (f) {
      try { f.contentDocument.documentElement.setAttribute("data-theme", theme); } catch (e) {}
    });
    Array.prototype.forEach.call(document.querySelectorAll("[data-theme-toggle]"), label);
    subs.forEach(function (fn) { fn(theme); });
  }
  function label(btn) {
    btn.textContent = theme === "dark" ? "Light" : "Dark";
    btn.setAttribute("aria-label", "Switch to " + (theme === "dark" ? "light" : "dark") + " mode");
  }
  window.CRTheme = {
    get: function () { return theme; },
    set: function (t) { theme = t; try { localStorage.setItem(KEY, t); } catch (e) {} apply(); },
    toggle: function () { window.CRTheme.set(theme === "dark" ? "light" : "dark"); },
    on: function (fn) { subs.push(fn); },
    label: label,
  };
  document.documentElement.setAttribute("data-theme", theme);
  document.addEventListener("click", function (e) {
    var b = e.target.closest && e.target.closest("[data-theme-toggle]");
    if (b) window.CRTheme.toggle();
  });
  document.addEventListener("DOMContentLoaded", apply);
})();
