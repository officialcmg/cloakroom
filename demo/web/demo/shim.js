/* Runs inside each frame holding the unchanged app (app/ui/app.js). The app calls /api/...; here those
   calls go to this side's proxy on the demo server, or, in replay mode, to the recording. */
(function () {
  var side = new URLSearchParams(location.search).get("side");
  var D = window.parent.DEMO;
  window.CLOAKROOM_WHERE = "Running on the Cloakroom demo server. ";
  try { localStorage.setItem("cr-theme", window.parent.document.documentElement.getAttribute("data-theme") || "light"); } catch (e) {}
  var realFetch = window.fetch.bind(window);
  window.fetch = function (url, opts) {
    if (typeof url !== "string" || url.indexOf("/api/") !== 0) return realFetch(url, opts);
    var method = (opts && opts.method) || "GET";
    if (D.mode === "replay") {
      var r = D.replayApi(side, url, method);
      return Promise.resolve({ ok: r.status < 400, status: r.status, statusText: "", json: function () { return Promise.resolve(r.body); } });
    }
    return realFetch("/demo/api/" + side + url.slice(4), opts).then(function (res) {
      if (method === "POST" && res.ok) D.startedHere(); else if (method === "POST") D.refresh();
      return res;
    });
  };
  if (D.mode === "replay") {
    var si = window.setInterval;
    window.setInterval = function (fn, ms) { return si(fn, ms >= 1000 ? Math.max(150, ms / D.speed) : ms); };
  }
})();
