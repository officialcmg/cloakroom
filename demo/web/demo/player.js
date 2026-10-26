/* The demo page's engine. In "live" mode the app frames talk to the sandbox through /demo/api/<side>/.
   In "replay" mode (while the sandbox starts, or if it is down) they are answered from recording.json,
   a real swap recorded from the same two services, on a clock that can run faster than real time. */
(function () {
  var D = window.DEMO = {
    mode: null, rec: null, t: 0, speed: 10, playing: false, subs: [],
    load: function () {
      return fetch("/demo/recording.json").then(function (r) { return r.json(); }).then(function (rec) {
        D.rec = rec;
        var fs = rec.frames;
        D.postT = fs.filter(function (f) { return f.taker.status.running; })[0].t - 0.4;
        D.end = fs[fs.length - 1].t;
        D.marks = [];
        var last = null;
        fs.forEach(function (f) { var s = D.stageOf(f.taker.swaps); if (s && s !== last) { D.marks.push({ t: f.t, stage: s }); last = s; } });
        D.t = D.postT;
      });
    },
    stageOf: function (swaps) {
      var sw = swaps && swaps.swaps && swaps.swaps[0]; if (!sw) return null;
      var st = sw.stage; return typeof st === "string" ? st : Object.keys(st)[0];
    },
    frame: function () {
      var fs = D.rec.frames, lo = 0, hi = fs.length - 1;
      while (lo < hi) { var mid = (lo + hi + 1) >> 1; if (fs[mid].t <= D.t) lo = mid; else hi = mid - 1; }
      return fs[lo];
    },
    elapsed: function () { return Math.max(0, D.t - D.postT); },
    total: function () { return D.end - D.postT; },
    // Called by the shim in each replay frame.
    replayApi: function (side, path, method) {
      if (method === "POST") return { status: 409, body: { error: "This is a recording. You can start a swap when the live sandbox is up." } };
      var key = path.split("?")[0].replace("/api/", "");
      return { status: 200, body: D.frame()[side][key] };
    },
    play: function () { if (D.t >= D.end) D.t = D.postT; D.playing = true; D.emit(); },
    pause: function () { D.playing = false; D.emit(); },
    seek: function (t) { D.t = Math.min(D.end, Math.max(D.postT, t)); D.emit(); },
    on: function (fn) { D.subs.push(fn); return function () { D.subs = D.subs.filter(function (x) { return x !== fn; }); }; },
    emit: function () { D.subs.forEach(function (fn) { fn(); }); },
  };
  var prev = null, lastEmit = 0;
  function loop(ts) {
    if (prev != null && D.playing && D.rec) {
      D.t += (ts - prev) / 1000 * D.speed;
      if (D.t >= D.end) { D.t = D.end; D.playing = false; D.emit(); }
      if (ts - lastEmit > 200) { lastEmit = ts; D.emit(); }
    }
    prev = ts; requestAnimationFrame(loop);
  }
  requestAnimationFrame(loop);
  D.mmss = function (s) { s = Math.max(0, Math.round(s)); return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0"); };

  // For recording the submission video: a visible cursor and captions (only with ?video=1).
  D.overlay = function () {
    var cur = document.createElement("div"); cur.className = "vid-cursor"; document.body.appendChild(cur);
    var cap = document.createElement("div"); cap.className = "vid-caption"; document.body.appendChild(cap);
    var x = -40, y = -40; cur.style.transform = "translate(" + x + "px," + y + "px)";
    return {
      move: function (tx, ty, ms) {
        return new Promise(function (res) {
          var x0 = x, y0 = y, t0 = performance.now();
          (function step(now) {
            var k = Math.min(1, (now - t0) / ms), e = k < .5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2;
            x = x0 + (tx - x0) * e; y = y0 + (ty - y0) * e; cur.style.transform = "translate(" + x + "px," + y + "px)";
            if (k < 1) requestAnimationFrame(step); else res();
          })(t0);
        });
      },
      click: function () { cur.classList.add("down"); setTimeout(function () { cur.classList.remove("down"); }, 180); },
      caption: function (text) { cap.textContent = text || ""; cap.classList.toggle("on", !!text); },
    };
  };
})();
