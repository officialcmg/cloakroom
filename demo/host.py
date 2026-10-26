"""Cloakroom hosted demo.

Runs everything `app/dev.py` runs (Zebra regtest, anvil with the escrow and a mock USDC, a funded
maker and taker, one `cloakroom serve` for each on localhost) and puts one public web server in front:

    /            the slides
    /demo/       both apps side by side, live; a recording plays while the sandbox starts
    /demo/api/   a narrow proxy to the two local services (see ROUTES)

The two services stay on 127.0.0.1. Visitors can read status, swaps and log lines for either side and
start a one-lot taker swap; nothing else. One swap runs at a time, followed by a short cooldown, and an
address can start a few swaps an hour. The keys are generated at start-up and hold test coins only.
"""
import gzip
import http.server
import json
import mimetypes
import os
import signal
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(ROOT, "harness"))
import run as H  # noqa: E402  (the harness: chains, parties, funding)

WEB = os.path.join(HERE, "web")
APP = os.path.join(ROOT, "app", "ui")
DS = os.path.join(ROOT, "design-system")
PORT = int(os.environ.get("PORT", "8080"))
SIDES = {"maker": 7801, "taker": 7802}
READS = {"status", "swaps", "log"}
COOLDOWN = 20                 # seconds between swaps
SWAP_LIMIT = 20 * 60          # a swap still running after this means something is stuck: restart
PER_IP_HOUR = 6               # swaps one address may start per hour
FRESH_AFTER = 6 * 3600        # rebuild the chains from scratch after this long, when idle
REFILL_BELOW = 2 * H.LOT      # top the taker up with a coinbase below 2 ZEC

H.EVID = os.path.join(H.WORK, "demo-evidence")


def log(msg):
    print(f"[demo {time.strftime('%H:%M:%S')}] {msg}", flush=True)


class Sandbox:
    def __init__(self):
        self.lock = threading.Lock()
        self.state = "starting"   # starting | idle | running | cooldown
        self.started_at = None
        self.cooldown_until = None
        self.last = None          # {"rfq_id", "stage", "secs"} of the last swap
        self.ips = {}
        self.taker_ua = None
        self.booted_at = time.time()

    def view(self):
        return {"state": self.state, "started_at": self.started_at, "cooldown_until": self.cooldown_until,
                "last": self.last, "now": time.time()}


S = Sandbox()


# ---------------------------------------------------------------- the two local services

def party(side, path, body=None, timeout=15):
    port = SIDES[side]
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", method="POST" if body is not None else "GET",
                                 data=json.dumps(body).encode() if body is not None else None,
                                 headers={"X-Cloakroom": "1", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


cache, cache_lock = {}, threading.Lock()


def cached_read(side, what):
    """Every visitor's page polls both sides; answer from a one-second cache so the services see one reader."""
    key = (side, what)
    with cache_lock:
        hit = cache.get(key)
        if hit and time.time() - hit[0] < 1.0:
            return hit[1], hit[2]
    code, body = party(side, f"/api/{what}")
    with cache_lock:
        cache[key] = (time.time(), code, body)
    return code, body


def restart(why):
    """End the whole process group (chains, services, swaps); the platform starts a fresh container."""
    log(f"restarting: {why}")
    os.killpg(0, signal.SIGKILL)


def stage_name(st):
    return st if isinstance(st, str) else next(iter(st or {}), "")


def boot():
    H.start_chains(nu7=False)
    maker, taker = H.parties("demo")
    logs = os.path.join(H.WORK, "demo")
    for side, p in (("maker", maker), ("taker", taker)):
        subprocess.Popen([H.BIN, "serve", "--dir", p.dir, "--port", str(SIDES[side]), "--ui", APP, "--ds", DS],
                         stdout=open(os.path.join(logs, f"serve-{side}.log"), "a"), stderr=subprocess.STDOUT)
    H.wait_for(lambda: all(_up(s) for s in SIDES), 60, "both services")
    party("maker", "/api/maker/start", {})
    S.taker_ua = taker.ua
    with S.lock:
        S.state = "idle"
    log("sandbox ready")


def _up(side):
    try:
        return party(side, "/api/status", timeout=3)[0] == 200
    except Exception:
        return False


def watch():
    """Notice when a swap ends, keep the maker quoting and the taker funded, restart when stuck."""
    last_refill = 0
    while True:
        time.sleep(2)
        if S.state == "starting":
            continue
        try:
            taker = json.loads(party("taker", "/api/status")[1])
            maker = json.loads(party("maker", "/api/status")[1])
        except Exception as e:
            log(f"status unavailable: {e}")
            continue
        now = time.time()
        with S.lock:
            if S.state == "running" and not taker["running"] and now - S.started_at > 3:
                swaps = json.loads(party("taker", "/api/swaps")[1])["swaps"]
                newest = swaps[0] if swaps else {}
                S.last = {"rfq_id": newest.get("rfq_id"), "stage": stage_name(newest.get("stage")), "secs": round(now - S.started_at)}
                S.state, S.cooldown_until = "cooldown", now + COOLDOWN
                log(f"swap ended: {S.last}")
            elif S.state == "running" and now - S.started_at > SWAP_LIMIT:
                restart("swap stuck past the limit")
            elif S.state == "cooldown" and now >= S.cooldown_until:
                S.state = "idle"
            idle = S.state == "idle"
        if not maker["running"]:
            log("maker not quoting; starting it")
            party("maker", "/api/maker/start", {})
        zats = (taker.get("balance") or {}).get("zats")
        if idle and zats is not None and zats < REFILL_BELOW and now - last_refill > 60:
            with H.mine_lock:
                H.zrpc("generatetoaddress", [1, S.taker_ua])
            last_refill = now
            log(f"taker refilled (had {zats} zats)")
        if idle and now - S.booted_at > FRESH_AFTER:
            restart("rebuilding the chains from scratch")


def start_swap(ip, body):
    if not isinstance(body, dict) or body.get("lots") != 1:
        return 400, {"error": "The demo sells 1 ZEC per swap."}
    now = time.time()
    with S.lock:
        if S.state == "running":
            return 409, {"error": "A swap is already running. You are watching it live."}
        if S.state == "cooldown":
            return 409, {"error": f"The next swap can start in {max(1, round(S.cooldown_until - now))} s."}
        if S.state != "idle":
            return 503, {"error": "The sandbox is starting. Try again in a minute."}
        recent = [t for t in S.ips.get(ip, []) if now - t < 3600]
        if len(recent) >= PER_IP_HOUR:
            return 429, {"error": "You have started six swaps in the last hour. Watch, or come back later."}
        code, out = party("taker", "/api/swaps", {"lots": 1})
        if code == 200:
            S.state, S.started_at, S.ips[ip] = "running", now, recent + [now]
            log("swap started")
        return code, json.loads(out or b"{}")


# ---------------------------------------------------------------- the public server

STATIC = [("/demo/app/", APP), ("/ds/", DS), ("/demo/", os.path.join(WEB, "demo")), ("/", WEB)]
SECURITY = {
    "Content-Security-Policy": "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; "
                               "frame-ancestors 'self'; base-uri 'none'; form-action 'none'",
    "X-Content-Type-Options": "nosniff",
    "Referrer-Policy": "no-referrer",
    "X-Frame-Options": "SAMEORIGIN",
}
mimetypes.add_type("font/woff2", ".woff2")
mimetypes.add_type("text/javascript", ".js")
gz_cache = {}


def resolve(path):
    """Map a URL path to a file under one of the static roots, or None."""
    for prefix, base in STATIC:
        if path.startswith(prefix):
            rel = path[len(prefix):] or "index.html"
            if rel.endswith("/"):
                rel += "index.html"
            parts = rel.split("/")
            if any(p in ("", ".", "..") or p.startswith(".") for p in parts):
                return None
            full = os.path.realpath(os.path.join(base, *parts))
            if not full.startswith(os.path.realpath(base) + os.sep) or not os.path.isfile(full):
                return None
            return full
    return None


class Handler(http.server.BaseHTTPRequestHandler):
    timeout = 20
    server_version = "cloakroom-demo"
    sys_version = ""

    def log_message(self, fmt, *args):
        pass

    def send(self, code, body, ctype="application/json", cache="no-store", extra=None):
        if isinstance(body, (dict, list)):
            body = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", cache)
        for k, v in {**SECURITY, **(extra or {})}.items():
            self.send_header(k, v)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def client_ip(self):
        fwd = self.headers.get("X-Forwarded-For")
        return fwd.split(",")[0].strip() if fwd else self.client_address[0]

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/healthz":
            return self.send(200, {"ok": True, "state": S.state})
        if path == "/demo":
            self.send_response(301)
            self.send_header("Location", "/demo/")
            self.send_header("Content-Length", "0")
            return self.end_headers()
        if path == "/demo/api/sandbox":
            return self.send(200, S.view())
        if path.startswith("/demo/api/"):
            parts = path.split("/")[3:]
            if len(parts) != 2 or parts[0] not in SIDES or parts[1] not in READS:
                return self.send(404, {"error": "not found"})
            if S.state == "starting":
                return self.send(503, {"error": "starting"})
            try:
                code, body = cached_read(*parts)
            except Exception:
                return self.send(502, {"error": "unavailable"})
            return self.send(code, body)
        full = resolve(path)
        if not full:
            return self.send(404, b"not found", "text/plain")
        ctype = mimetypes.guess_type(full)[0] or "application/octet-stream"
        if ctype.startswith("text/") or ctype in ("application/json", "image/svg+xml"):
            ctype += "; charset=utf-8"
        data = open(full, "rb").read()
        extra = {}
        if len(data) > 8192 and "gzip" in (self.headers.get("Accept-Encoding") or "") and not ctype.startswith(("font/", "image/png")):
            key = (full, os.path.getmtime(full))
            if key not in gz_cache:
                gz_cache[key] = gzip.compress(data, 6)
            data, extra = gz_cache[key], {"Content-Encoding": "gzip", "Vary": "Accept-Encoding"}
        self.send(200, data, ctype, "public, max-age=300", extra)

    def do_POST(self):
        path = self.path.split("?")[0]
        if path != "/demo/api/taker/swaps":
            return self.send(404, {"error": "not found"})
        n = int(self.headers.get("Content-Length") or 0)
        if n > 1024:
            return self.send(413, {"error": "too large"})
        try:
            body = json.loads(self.rfile.read(n) or b"{}")
        except ValueError:
            return self.send(400, {"error": "bad json"})
        try:
            code, out = start_swap(self.client_ip(), body)
        except Exception as e:
            log(f"start failed: {e}")
            code, out = 502, {"error": "The sandbox could not start the swap. Try again in a minute."}
        self.send(code, out)


class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 64


def main():
    server = Server(("0.0.0.0", PORT), Handler)
    log(f"listening on :{PORT}")
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        boot()
    except Exception as e:
        restart(f"boot failed: {e}")
    watch()


if __name__ == "__main__":
    main()
