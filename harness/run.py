#!/usr/bin/env python3
"""Cloakroom scenario harness.

Runs the full two-party protocol on a local Zebra regtest node (Ironwood rules) and a
local anvil chain standing in for Base, with maker and taker as separate OS processes
that communicate only through encrypted memos and the escrow contract.

  python3 -I harness/run.py            # all scenarios, NU6.3 then NU7
  python3 -I harness/run.py s1 s4      # selected scenarios (NU6.3 chain)

Evidence for every scenario is written to harness/evidence/<scenario>/.
"""
import json
import os
import shutil
import subprocess
import sys
import threading
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
BIN = os.environ.get("CLOAKROOM_BIN") or os.path.join(ROOT, "node", "target", "debug", "cloakroom")
CONTRACTS = os.path.join(ROOT, "contracts")
EVID = os.path.join(HERE, "evidence")
WORK = os.path.join(HERE, "work")
ZEBRA = "http://127.0.0.1:18232"
ANVIL = "http://127.0.0.1:8545"
# Anvil's well-known development keys (public test keys, never used on a real network).
K_DEPLOY = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
K_MAKER = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
K_RELAYER = "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"

LOT = 100_000_000          # 1 ZEC
FEE = 10_000               # ZIP 317, 2 logical actions
PRICE = 300_000_000        # 300 mUSDC per lot
RELAYER_FEE = 1_000_000    # 1 mUSDC
PREMIUM = 50_000           # zatoshis paid to the maker with Accept
COVER_K = 4                # look-alike txs required before each delayed broadcast (scenario s12)
T0, T1, T2, W = 150, 60, 240, 150   # seconds: fund+ready, commit window, punish delay (t2 - t1), reveal window (anvil finality lags 64 blocks)
D_SMALL, D_CLAIM = 3, 10

state = {"escrow": None, "token": None, "nu7": None}
anvil_proc = None
zebra_proc = None  # set when ZEBRAD names a zebrad binary to run directly instead of the Docker image
stop_miner = threading.Event()
mine_lock = threading.Lock()


def log(msg):
    print(f"[harness {time.strftime('%H:%M:%S')}] {msg}", flush=True)


def zrpc(method, params=None):
    req = urllib.request.Request(ZEBRA, data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params or []}).encode(),
                                 headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        resp = json.load(r)
    if resp.get("error"):
        raise RuntimeError(f"{method}: {resp['error']}")
    return resp["result"]


def sh(*args, cwd=None, check=True):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if check and p.returncode != 0:
        raise RuntimeError(f"{' '.join(args[:3])}...: {p.stderr.strip() or p.stdout.strip()}")
    return p.stdout.strip()


def cast(*args):
    return sh("cast", *args)


def first(s):
    return s.split()[0] if s else s


def evm_now():
    return int(first(cast("block", "latest", "--field", "timestamp", "--rpc-url", ANVIL)))


def warp_to(ts):
    d = ts - evm_now()
    if d > 0:
        cast("rpc", "--rpc-url", ANVIL, "evm_increaseTime", str(d))
        cast("rpc", "--rpc-url", ANVIL, "evm_mine")
        log(f"warped Base clock forward {d}s to {evm_now()}")


def token_balance(addr):
    return int(first(cast("call", "--rpc-url", ANVIL, state["token"], "balanceOf(address)(uint256)", addr)))


def addr_of(key):
    return cast("wallet", "address", "--private-key", key).lower()


def receipt_block(tx):
    """Block number of tx on the canonical chain, or None (raw RPC: `cast receipt` would block)."""
    r = json.loads(cast("rpc", "--rpc-url", ANVIL, "eth_getTransactionReceipt", tx) or "null")
    return int(r["blockNumber"], 16) if r else None


def reorg_out(tx):
    """Reorganize away the (not yet final) block holding tx; anvil does not re-include it."""
    bn = receipt_block(tx)
    head = int(first(cast("block-number", "--rpc-url", ANVIL)))
    depth = head - bn + 1
    assert depth < 64, f"block {bn} is already final (depth {depth})"
    cast("rpc", "--rpc-url", ANVIL, "anvil_reorg", str(depth), "[]")
    log(f"REORG: replaced the last {depth} blocks (from {bn}); tx {tx[:12]}.. dropped")
    return {"tx": tx, "block": bn, "depth": depth, "gone_after_reorg": receipt_block(tx) is None}


def wait_for(fn, timeout, what):
    t = time.time()
    while time.time() - t < timeout:
        v = fn()
        if v:
            return v
        time.sleep(0.3)
    raise RuntimeError(f"timed out waiting for {what}")


# ---------------------------------------------------------------- chains

def miner():
    """Mine a block whenever the mempool is non-empty, and every 3 s regardless."""
    last = 0.0
    while not stop_miner.is_set():
        try:
            with mine_lock:
                if zrpc("getrawmempool") or time.time() - last > 3:
                    zrpc("generate", [1])
                    last = time.time()
        except Exception:
            pass
        time.sleep(0.4)


def start_chains(nu7):
    global anvil_proc, zebra_proc
    conf = "zebrad-nu7.toml" if nu7 else "zebrad-nu63.toml"
    if os.environ.get("ZEBRAD"):
        zebra_proc = subprocess.Popen([os.environ["ZEBRAD"], "-c", os.environ.get("CLOAKROOM_ZEBRA_CONF") or os.path.join(HERE, "zebra", conf), "start"],
                                      stdout=open(os.path.join(WORK, "zebrad.log"), "w"), stderr=subprocess.STDOUT)
    else:
        env = dict(os.environ, ZEBRA_CONF=conf)
        subprocess.run(["docker", "compose", "down"], cwd=os.path.join(HERE, "zebra"), env=env, capture_output=True)
        subprocess.run(["docker", "compose", "up", "-d"], cwd=os.path.join(HERE, "zebra"), env=env, check=True, capture_output=True)
    for _ in range(90):
        try:
            zrpc("getblockchaininfo")
            break
        except Exception:
            time.sleep(1)
    zrpc("generate", [3])
    info = zrpc("getblockchaininfo")
    log(f"zebra up: {zrpc('getinfo')['subversion']} tip {info['blocks']} next-block branch {info['consensus']['nextblock']}")
    state["nu7"] = 2 if nu7 else None
    state["branch"] = info["consensus"]["nextblock"]

    if anvil_proc:
        anvil_proc.terminate()
        anvil_proc.wait()
    anvil_proc = subprocess.Popen(["anvil", "--block-time", "1", "--chain-id", "84532", "--port", "8545", "--silent"])
    for _ in range(30):
        try:
            evm_now()
            break
        except Exception:
            time.sleep(0.5)
    def deploy(name):
        out = sh("forge", "create", f"src/{name}.sol:{name}", "--rpc-url", ANVIL, "--private-key", K_DEPLOY, "--broadcast", "--json", cwd=CONTRACTS)
        return json.loads(out)["deployedTo"].lower()
    state["token"] = deploy("MockUSDC")
    state["escrow"] = deploy("CloakroomEscrow")
    log(f"anvil up (chain id 84532); MockUSDC {state['token']}, CloakroomEscrow {state['escrow']}")
    stop_miner.clear()
    threading.Thread(target=miner, daemon=True).start()


def stop_chains():
    stop_miner.set()
    time.sleep(0.5)
    if anvil_proc:
        anvil_proc.terminate()
    if zebra_proc:
        zebra_proc.terminate()
    else:
        subprocess.run(["docker", "compose", "down"], cwd=os.path.join(HERE, "zebra"), capture_output=True)


# ---------------------------------------------------------------- parties

class Party:
    def __init__(self, scen, role, cfg):
        self.dir = os.path.join(WORK, scen, role)
        shutil.rmtree(self.dir, ignore_errors=True)
        os.makedirs(self.dir)
        self.role = role
        self.scen = scen
        self.logpath = os.path.join(EVID, scen, f"{role}.log")
        open(self.logpath, "w").close()
        with open(os.path.join(self.dir, "config.json"), "w") as f:
            json.dump(cfg, f, indent=2)
        self.ua = sh(BIN, "init", "--dir", self.dir)
        self.proc = None

    def fund_coinbase(self):
        with mine_lock:
            zrpc("generatetoaddress", [1, self.ua])

    def balance(self):
        return json.loads(sh(BIN, "balance", "--dir", self.dir))["zats"]

    def start(self, *args):
        f = open(self.logpath, "a")
        f.write(f"\n===== start {self.role} {' '.join(args)} =====\n")
        f.flush()
        self.proc = subprocess.Popen([BIN, self.role, "--dir", self.dir, *args], stdout=f, stderr=subprocess.STDOUT)
        return self.proc

    def wait(self, timeout):
        try:
            return self.proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            raise RuntimeError(f"{self.role} timed out after {timeout}s (see {self.logpath})")

    def kill(self):
        if self.proc and self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait()

    def wait_log(self, needle, timeout):
        t = time.time()
        while time.time() - t < timeout:
            if needle in open(self.logpath).read():
                return
            if self.proc and self.proc.poll() is not None and needle not in open(self.logpath).read():
                raise RuntimeError(f"{self.role} exited before logging {needle!r}")
            time.sleep(0.5)
        raise RuntimeError(f"timeout waiting for {self.role} to log {needle!r}")

    def swap(self):
        d = os.path.join(self.dir, "swaps")
        files = [f for f in os.listdir(d) if f.endswith(".json")] if os.path.isdir(d) else []
        return json.load(open(os.path.join(d, files[0]))) if files else None


def base_cfg():
    return {"zebra_rpc": ZEBRA, "evm_rpc": ANVIL, "escrow": state["escrow"], "token": state["token"],
            "nu7_height": state["nu7"], "lot_zats": LOT, "delta_small": D_SMALL, "delta_claim": D_CLAIM,
            "inclusion_secs": 2, "finality_secs": 65, "margin_secs": 5}


def finalize():
    """Anvil's finalized head lags 64 blocks; mine 65 at once so everything so far is final."""
    cast("rpc", "--rpc-url", ANVIL, "anvil_mine", "0x41")


def parties(scen):
    os.makedirs(os.path.join(EVID, scen), exist_ok=True)
    maker = Party(scen, "maker", {**base_cfg(), "evm_key": K_MAKER, "price_per_lot": PRICE, "relayer_fee": RELAYER_FEE,
                                   "premium_zats": PREMIUM, "t0_secs": T0, "t1_secs": T1, "t2_secs": T2, "reveal_secs": W, "k_z": 2, "max_lots": 5})
    epoch, rfq_pub, dir_sig = sh(BIN, "rfqpub", "--dir", maker.dir).split()
    taker = Party(scen, "taker", {**base_cfg(), "maker_ua": maker.ua, "maker_evm": addr_of(K_MAKER), "relayer_key": K_RELAYER,
                                   "maker_rfq_pub": rfq_pub, "maker_rfq_epoch": int(epoch), "maker_dir_sig": dir_sig,
                                   "max_price": 2 * PRICE, "max_premium": 2 * PREMIUM, "quote_timeout_secs": 400})
    maker.fund_coinbase()
    taker.fund_coinbase()
    cast("send", "--rpc-url", ANVIL, "--private-key", K_DEPLOY, state["token"], "mint(address,uint256)", addr_of(K_MAKER), str(1000 * PRICE))
    time.sleep(4)
    for p in (maker, taker):
        if p.balance() <= 0:
            raise RuntimeError(f"{p.role} wallet not funded")
    return maker, taker


def escrow_state(swap_id):
    out = cast("call", "--rpc-url", ANVIL, state["escrow"],
               "swaps(bytes32)(address,address,address,address,uint64,uint64,uint64,uint64,uint64,uint8,uint256,uint256,uint256,uint256,uint256,uint256,bytes32)", swap_id)
    return ["None", "Open", "Ready", "Locked", "Claimed", "Refunded"][int(first(out.splitlines()[9]))]


def escrow_field(swap_id, idx):
    out = cast("call", "--rpc-url", ANVIL, state["escrow"],
               "swaps(bytes32)(address,address,address,address,uint64,uint64,uint64,uint64,uint64,uint8,uint256,uint256,uint256,uint256,uint256,uint256,bytes32)", swap_id)
    return int(first(out.splitlines()[idx]))


def claimed_event_count(swap_id):
    topic0 = cast("keccak", "Claimed(bytes32,uint256)")
    out = cast("logs", "--rpc-url", ANVIL, "--from-block", "0", "--address", state["escrow"], "--json", topic0)
    return sum(1 for l in json.loads(out or "[]") if l["topics"][1].lower() == swap_id.lower())


def privacy_audit(c, maker, taker):
    """Every Zcash tx either party broadcast must be fully shielded: no transparent inputs or
    outputs, no Sapling or Orchard-pool activity, and an Ironwood value balance equal to the fee."""
    import re
    txids = set()
    for p in (maker, taker):
        sw = p.swap() or {}
        for line in sw.get("log", []):
            txids.update(re.findall(r"\btx ([0-9a-f]{64})\b", line))
    bad = []
    for txid in sorted(txids):
        v = zrpc("getrawtransaction", [txid, 1])
        iw = v.get("ironwood") or {}
        shielded_only = not v.get("vin") and not v.get("vout") and not (v.get("orchard") or {}).get("actions") \
            and not v.get("vShieldedSpend") and not v.get("vShieldedOutput")
        vb = iw.get("valueBalanceZat", iw.get("valueBalance"))
        if not shielded_only or not iw.get("actions"):
            bad.append(txid)
        c.facts.setdefault("zcash_txs", []).append({"txid": txid, "height": v.get("height"), "version": v.get("version"),
            "ironwood_actions": len(iw.get("actions", [])), "ironwood_value_balance": vb,
            "transparent_in": len(v.get("vin") or []), "transparent_out": len(v.get("vout") or [])})
    c.ok(f"privacy audit: all {len(txids)} Zcash txs by maker/taker are fully shielded Ironwood v6 (no transparent in/out)", txids and not bad, bad)


class Checks:
    def __init__(self, scen, title):
        self.scen, self.title, self.items, self.facts = scen, title, [], {}

    def ok(self, desc, cond, detail=""):
        self.items.append({"check": desc, "pass": bool(cond), "detail": str(detail)})
        log(f"  {'PASS' if cond else 'FAIL'}  {desc} {detail}")

    def save(self, maker, taker):
        # every scenario with a swap gets the privacy audit and the R4 erasure check
        has_swap = bool(maker.swap() or taker.swap())
        if has_swap and not any(i["check"].startswith("privacy audit") for i in self.items):
            privacy_audit(self, maker, taker)
        if has_swap and not any(i["check"].startswith("R4:") for i in self.items):
            secrets_erased(self, maker, taker)
        rec = {"scenario": self.scen, "title": self.title, "chain_branch": state.get("branch"), "checks": self.items, "facts": self.facts,
               "maker_swap": maker.swap(), "taker_swap": taker.swap()}
        with open(os.path.join(EVID, self.scen, "result.json"), "w") as f:
            json.dump(rec, f, indent=2)
        return all(i["pass"] for i in self.items)


def settle():
    """Wait until every broadcast transaction is mined, plus one more block."""
    for _ in range(60):
        if not zrpc("getrawmempool"):
            break
        time.sleep(0.5)
    h = zrpc("getblockcount")
    while zrpc("getblockcount") <= h:
        time.sleep(0.3)


def snapshot(maker, taker):
    settle()
    return {"maker_zec": maker.balance(), "taker_zec": taker.balance(), "maker_tok": token_balance(addr_of(K_MAKER)),
            "relayer_tok": token_balance(addr_of(K_RELAYER))}


MAKER_FINAL = {"Swept", "Refunded", "Aborted"}
TAKER_FINAL = {"Claimed", "Recovered", "Aborted"}  # Recovering / RefundSent / Sweeping are not terminal


def secrets_erased(c, maker, taker):
    """R4 for every party that reached a terminal stage. A party that vanished mid-swap must keep
    its secrets (it needs them when it returns), so it is reported and not checked."""
    m, t = dict(maker.swap() or {}), dict(taker.swap() or {})
    for d in (m, t):  # an aborted stage is stored as {"Aborted": reason}
        if isinstance(d.get("stage"), dict):
            d["stage"] = next(iter(d["stage"]))
    checked, skipped, bad = [], [], []
    if m.get("stage") in MAKER_FINAL:
        checked.append("maker:" + m["stage"])
        if m.get("b") or m.get("s_m") or m.get("e_m") or m.get("joint_fvk"):
            bad.append("maker")
    elif m:
        skipped.append("maker:" + str(m.get("stage")))
    if t.get("stage") in TAKER_FINAL:
        checked.append("taker:" + t["stage"])
        if t.get("a") or t.get("s_t") or t.get("e_t") or t.get("joint_fvk"):
            bad.append("taker")
    elif t:
        skipped.append("taker:" + str(t.get("stage")))
    c.facts["r4_checked"], c.facts["r4_not_terminal"] = checked, skipped
    c.ok("R4: per-swap secrets erased by every party in a terminal stage (b, s_M, e_M / a, s_T, e_T; joint FVK)",
         checked and not bad, f"checked={checked} not_terminal={skipped} bad={bad}")


# ---------------------------------------------------------------- scenarios (protocol v2)

def s1_happy(scen="s1_happy", title="Happy path: encrypted RFQ/quote/accept (+premium) -> open -> fund -> ready -> commit -> reveal -> withdraw -> sweep"):
    maker, taker = parties(scen)
    c = Checks(scen, title)
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1")
    taker.wait(400)
    maker.wait(200)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("taker reached Claimed", t["stage"] == "Claimed", t["stage"])
    c.ok("maker reached Swept", m["stage"] == "Swept", m["stage"])
    c.ok("escrow state Claimed", escrow_state(m["escrow_id"]) == "Claimed")
    c.ok("taker payout withdrew price - relayer fee", token_balance(t["payout"]) == PRICE - RELAYER_FEE, token_balance(t["payout"]))
    c.ok("relayer withdrew its fee", after["relayer_tok"] - before["relayer_tok"] == RELAYER_FEE)
    c.ok("maker paid exactly the price", before["maker_tok"] - after["maker_tok"] == PRICE)
    c.ok("maker ZEC: +lot + premium - quote fee (exact)", after["maker_zec"] - before["maker_zec"] == LOT + PREMIUM - FEE, after["maker_zec"] - before["maker_zec"])
    c.ok("taker ZEC: -(lot + sweep fee) - premium - 3 tx fees (exact)", before["taker_zec"] - after["taker_zec"] == LOT + FEE + PREMIUM + 3 * FEE, before["taker_zec"] - after["taker_zec"])
    privacy_audit(c, maker, taker)
    secrets_erased(c, maker, taker)
    return c.save(maker, taker)


def s2_taker_vanishes_before_funding():
    scen = "s2_taker_vanishes_before_funding"
    maker, taker = parties(scen)
    c = Checks(scen, "Taker accepts (pays premium), then disappears before funding: maker refunds before t0")
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1", "--vanish-at", "accepted")
    taker.wait(300)
    maker.wait_log("escrow OPEN", 200)
    m = maker.swap()
    warp_to(m["t0"] - 2 * D_SMALL - 1)
    maker.wait(120)
    m = maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("taker vanished at Accepted", taker.swap()["stage"] == "Accepted")
    c.ok("maker reached Refunded", m["stage"] == "Refunded", m["stage"])
    c.ok("escrow state Refunded", escrow_state(m["escrow_id"]) == "Refunded")
    c.ok("maker tokens fully restored (withdrawn)", after["maker_tok"] == before["maker_tok"])
    c.ok("maker kept the premium: ZEC +premium - quote fee (exact)", after["maker_zec"] - before["maker_zec"] == PREMIUM - FEE)
    return c.save(maker, taker)


def s3_taker_vanishes_after_funding():
    scen = "s3_taker_vanishes_after_funding"
    maker, taker = parties(scen)
    c = Checks(scen, "Taker funds then disappears; returns after t1: R2 refuses to commit, maker refunds once t1 is final, taker recovers ZEC")
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1", "--vanish-at", "funded")
    taker.wait(400)
    maker.wait_log("READY", 200)
    m = maker.swap()
    warp_to(m["t1"] + 1)
    taker.start("--lots", "1")       # resume after the commit deadline
    taker.wait_log("R2", 120)
    finalize()
    taker.wait(200)
    maker.wait(120)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("taker refused to commit after t1 (R2)", any(l.startswith("R2") for l in t["log"]))
    c.ok("maker refunded only from a finalized block past t1", m["stage"] == "Refunded" and any("finalized block" in l and "past t1" in l for l in m["log"]), m["stage"])
    c.ok("taker recovered the joint note", t["stage"] == "Recovered", t["stage"])
    c.ok("maker tokens fully restored", after["maker_tok"] == before["maker_tok"])
    c.ok("taker ZEC: premium + 4 tx fees lost (exact)", before["taker_zec"] - after["taker_zec"] == PREMIUM + 4 * FEE, before["taker_zec"] - after["taker_zec"])
    c.ok("no Claimed event: a never published", claimed_event_count(m["escrow_id"]) == 0)
    privacy_audit(c, maker, taker)
    secrets_erased(c, maker, taker)
    return c.save(maker, taker)


def s4_maker_vanishes_after_open():
    scen = "s4_maker_vanishes_after_open"
    maker, taker = parties(scen)
    c = Checks(scen, "Maker opens then disappears (never sets Ready) after the taker funds: no hostage. From t2 the taker completes via the punish path; the returning maker sweeps the ZEC")
    before = snapshot(maker, taker)
    maker.start("--vanish-at", "opened")
    taker.start("--lots", "1")
    maker.wait(300)
    taker.wait_log("FUNDED", 200)
    m = maker.swap()
    warp_to(m["t2"] + 1)
    taker.wait_log("punish path", 120)
    finalize()
    taker.wait(200)
    maker.start("--exit-after", "1")  # returns after the trade completed
    maker.wait(200)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("taker locked via the punish path after t2", any("punish path" in l for l in t["log"]))
    c.ok("taker reached Claimed (no hostage)", t["stage"] == "Claimed", t["stage"])
    c.ok("returning maker swept the ZEC", m["stage"] == "Swept", m["stage"])
    c.ok("taker payout withdrew price - relayer fee", token_balance(t["payout"]) == PRICE - RELAYER_FEE)
    c.ok("maker ZEC: +lot + premium - quote fee (exact)", after["maker_zec"] - before["maker_zec"] == LOT + PREMIUM - FEE)
    return c.save(maker, taker)


def s5_rogue_key():
    scen = "s5_rogue_key"
    maker, taker = parties(scen)
    c = Checks(scen, "Malicious maker sends a rogue key B = [b']G - A: taker rejects the quote and never funds")
    before = snapshot(maker, taker)
    maker.start("--evil", "rogue-key")
    taker.start("--lots", "1")
    taker.wait(300)
    maker.kill()
    t = taker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after)
    c.ok("taker aborted on invalid proof of knowledge", isinstance(t["stage"], dict) and "proof of knowledge" in t["stage"].get("Aborted", ""), t["stage"])
    c.ok("taker never funded and paid no premium", t.get("fund_txid") is None and before["taker_zec"] - after["taker_zec"] == FEE)
    return c.save(maker, taker)


def s6_underfund():
    scen = "s6_underfund"
    maker, taker = parties(scen)
    c = Checks(scen, "Taker underfunds the joint address: maker refuses Ready and refunds before t0; taker recovers")
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1", "--evil", "underfund")
    taker.wait_log("FUNDED", 300)
    time.sleep(6)
    m = maker.swap()
    warp_to(m["t0"] - 2 * D_SMALL - 1)
    maker.wait(120)
    taker.wait(200)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("maker did not set Ready", not any("READY" in l for l in m["log"]))
    c.ok("maker refunded because underfunded", m["stage"] == "Refunded" and any("underfunded" in l for l in m["log"]))
    c.ok("taker recovered", t["stage"] == "Recovered", t["stage"])
    c.ok("maker tokens fully restored", after["maker_tok"] == before["maker_tok"])
    c.ok("taker ZEC: premium + 4 tx fees lost (exact)", before["taker_zec"] - after["taker_zec"] == PREMIUM + 4 * FEE, before["taker_zec"] - after["taker_zec"])
    return c.save(maker, taker)


def s7_happy_nu7():
    return s1_happy("s7_happy_nu7", "Happy path again, on a chain with NU7 active (25 s block rules)")


def s8_free_claim_attempt():
    scen = "s8_free_claim_attempt"
    maker, taker = parties(scen)
    c = Checks(scen, "Review finding: a taker who never funds tries to claim while the maker is offline. Rejected before t2 (claims need Ready); the maker comes back once in [t1, t2) (assumption L_M) and refunds, closing the punish path")
    before = snapshot(maker, taker)
    maker.start("--vanish-at", "opened")
    taker.start("--lots", "1", "--evil", "free-claim")
    maker.wait(300)
    taker.wait_log("NOT funding", 200)
    m = maker.swap()
    warp_to(m["t0"] + 1)
    taker.wait_log("REJECTED", 120)
    warp_to(m["t1"] + 1)
    finalize()
    maker.start("--exit-after", "1")
    maker.wait(200)
    taker.wait(200)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("unfunded lock before t2 rejected by the escrow", any("REJECTED" in l for l in t["log"]))
    c.ok("maker refunded within [t1, t2)", m["stage"] == "Refunded" and any("past t1" in l for l in m["log"]), m["stage"])
    c.ok("no Claimed event", claimed_event_count(m["escrow_id"]) == 0)
    c.ok("maker tokens fully restored", after["maker_tok"] == before["maker_tok"])
    return c.save(maker, taker)


def s9_late_commit():
    scen = "s9_late_commit"
    maker, taker = parties(scen)
    c = Checks(scen, "Review finding: a claim that lands after the deadline. The late commitClaim reverts publishing only H(id, a); maker refunds; taker recovers; a is never revealed")
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1", "--evil", "late-commit", "--vanish-at", "funded")
    taker.wait(400)
    maker.wait_log("READY", 200)
    m = maker.swap()
    warp_to(m["t1"] + 1)
    taker.start("--lots", "1", "--evil", "late-commit")
    taker.wait_log("REVERTED", 120)
    finalize()
    maker.wait(200)
    taker.wait(200)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("late commit reverted", any("REVERTED" in l for l in t["log"]))
    c.ok("no Claimed event: a never published", claimed_event_count(m["escrow_id"]) == 0)
    c.ok("maker refunded (did not sweep: it never learned a)", m["stage"] == "Refunded", m["stage"])
    c.ok("taker recovered", t["stage"] == "Recovered", t["stage"])
    c.ok("taker ZEC: premium + 4 tx fees lost (exact)", before["taker_zec"] - after["taker_zec"] == PREMIUM + 4 * FEE, before["taker_zec"] - after["taker_zec"])
    return c.save(maker, taker)


def s10_lock_without_reveal():
    scen = "s10_lock_without_reveal"
    maker, taker = parties(scen)
    c = Checks(scen, "Taker locks (commitClaim) then disappears before revealing: maker refunds once the reveal deadline is final; taker returns and recovers")
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1", "--vanish-at", "committed")
    taker.wait(400)
    eid = taker.swap()["escrow_id"]
    locked_at, window = escrow_field(eid, 8), escrow_field(eid, 7)
    warp_to(locked_at + window + 1)
    finalize()
    maker.wait(200)
    taker.start("--lots", "1")
    taker.wait(200)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("escrow was Locked before the refund", any("LOCKED" in l for l in t["log"]))
    c.ok("maker refunded after the reveal deadline", m["stage"] == "Refunded" and any("did not reveal" in l for l in m["log"]), m["stage"])
    c.ok("taker recovered", t["stage"] == "Recovered", t["stage"])
    c.ok("no Claimed event", claimed_event_count(m["escrow_id"]) == 0)
    c.ok("maker tokens fully restored", after["maker_tok"] == before["maker_tok"])
    return c.save(maker, taker)


def s11_maker_liveness_violated():
    scen = "s11_maker_liveness_violated"
    maker, taker = parties(scen)
    c = Checks(scen, "Assumption boundary (Proposition 3): the maker is offline for ALL of [t1, t2) and the taker never funds. The punish path then lets her claim for free. This is the expected, documented consequence of violating L_M")
    before = snapshot(maker, taker)
    maker.start("--vanish-at", "opened")
    taker.start("--lots", "1", "--evil", "free-claim")
    maker.wait(300)
    taker.wait_log("NOT funding", 200)
    m = maker.swap()
    warp_to(m["t2"] + 1)
    taker.wait_log("punish path", 120)
    finalize()
    taker.wait(200)
    maker.start("--exit-after", "1")
    maker.wait(200)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("taker never funded the joint address", t.get("fund_txid") is None)
    c.ok("taker claimed via the punish path", t["stage"] == "Claimed", t["stage"])
    c.ok("maker lost the escrowed tokens, as Proposition 3 predicts", before["maker_tok"] - after["maker_tok"] == PRICE)
    c.ok("maker detected and reported the L_M violation", any("L_M" in l for l in m["log"]), m["stage"])
    return c.save(maker, taker)


def lookalikes_between(lo, hi):
    """Fully shielded, Ironwood-only, 2-action txs in blocks lo < h < hi (independent count)."""
    n = 0
    for h in range(lo + 1, hi):
        for txid in zrpc("getblock", [str(h), 1])["tx"]:
            v = zrpc("getrawtransaction", [txid, 1])
            iw = v.get("ironwood") or {}
            if not v.get("vin") and not v.get("vout") and not (v.get("orchard") or {}).get("actions") \
                    and not v.get("vShieldedSpend") and not v.get("vShieldedOutput") and len(iw.get("actions", [])) == 2:
                n += 1
    return n


def tx_height(txid):
    return zrpc("getrawtransaction", [txid, 1])["height"]


def s12_cover_traffic():
    import re
    scen = "s12_cover_traffic"
    maker, taker = parties(scen)
    for p in (maker, taker):
        cfg = json.load(open(os.path.join(p.dir, "config.json")))
        cfg.update(cover_min_secs=5, cover_max_secs=15, cover_k=COVER_K)
        json.dump(cfg, open(os.path.join(p.dir, "config.json"), "w"), indent=2)
    noise = Party(scen, "noise", base_cfg())
    noise.fund_coinbase()
    time.sleep(4)
    stop = threading.Event()
    def chatter():
        while not stop.is_set():
            subprocess.run([BIN, "pay", "--dir", noise.dir, "--to", noise.ua, "--zats", "1000"], capture_output=True)
            time.sleep(2)
    th = threading.Thread(target=chatter, daemon=True)
    th.start()
    c = Checks(scen, f"Cover traffic: with look-alike background traffic, every delayed Zcash broadcast waits until at least k={COVER_K} look-alikes have been mined since its trigger, and the swap still completes")
    try:
        maker.start("--exit-after", "1")
        taker.start("--lots", "1")
        taker.wait(600)
        maker.wait(300)
    finally:
        stop.set()
        th.join(timeout=30)
    t, m = taker.swap(), maker.swap()
    c.ok("swap completed under cover delays", t["stage"] == "Claimed" and m["stage"] == "Swept", (t["stage"], m["stage"]))
    logs = open(taker.logpath).read() + open(maker.logpath).read()
    # redraw policy: the broadcast ends a random wait (re-drawn while fewer than k look-alikes)
    waits = re.findall(r"(QUOTE|ACCEPT|FUND|SWEEP): cover: waited (\d+)s over (\d+) random draw\(s\), (\d+) look-alike", logs)
    c.facts["cover_waits"] = waits
    c.ok("all four delayable broadcasts waited for cover", sorted({w[0] for w in waits}) == ["ACCEPT", "FUND", "QUOTE", "SWEEP"], waits)
    c.ok(f"each wait saw >= {COVER_K} look-alikes (wallet count)", all(int(w[3]) >= COVER_K for w in waits))
    # independent on-chain check for the message round trip: RFQ -> QUOTE -> ACCEPT
    txs = {}
    for line in t["log"] + m["log"]:
        for kind in ("RFQ", "QUOTE", "ACCEPT"):
            if line.startswith(("encrypted RFQ" if kind == "RFQ" else "RFQ for" if kind == "QUOTE" else "QUOTE decrypted")):
                mt = re.search(r"\btx ([0-9a-f]{64})\b", line)
                if mt:
                    txs[kind] = mt.group(1)
    if len(txs) == 3:
        h = {k: tx_height(v) for k, v in txs.items()}
        between_rq = lookalikes_between(h["RFQ"], h["QUOTE"])
        between_qa = lookalikes_between(h["QUOTE"], h["ACCEPT"])
        c.facts["independent_counts"] = {"rfq_to_quote": between_rq, "quote_to_accept": between_qa, "heights": h}
        c.ok(f"independent chain count: >= {COVER_K} look-alikes mined between RFQ and QUOTE, and between QUOTE and ACCEPT", between_rq >= COVER_K and between_qa >= COVER_K, (between_rq, between_qa))
    else:
        c.ok("found RFQ/QUOTE/ACCEPT txids for the independent check", False, txs)
    privacy_audit(c, maker, taker)
    return c.save(maker, taker)



def s13_reorg_claim():
    scen = "s13_reorg_claim"
    maker, taker = parties(scen)
    c = Checks(scen, "Reorg injection: the block with the taker's claim(a) is reorganized away before it is final; a is already public; the taker re-sends the claim and completes, the maker sweeps")
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1")
    tx = wait_for(lambda: (lambda t: t and t.get("claim_tx") and receipt_block(t["claim_tx"]) is not None and t["claim_tx"])(taker.swap()), 400, "claim included")
    r = reorg_out(tx)
    c.facts["reorg"] = r
    c.ok("claim block reorganized away before finality", r["gone_after_reorg"] and r["depth"] < 64, r)
    taker.wait(400)
    maker.wait(300)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("taker re-sent the claim after the reorg", any("RE-SENT claim" in l for l in t["log"]))
    c.ok("taker reached Claimed (claim final)", t["stage"] == "Claimed", t["stage"])
    c.ok("maker reached Swept", m["stage"] == "Swept", m["stage"])
    c.ok("exactly one Claimed event on the canonical chain", claimed_event_count(m["escrow_id"]) == 1)
    c.ok("taker payout withdrew price - relayer fee", token_balance(t["payout"]) == PRICE - RELAYER_FEE, token_balance(t["payout"]))
    c.ok("maker paid exactly the price", before["maker_tok"] - after["maker_tok"] == PRICE)
    return c.save(maker, taker)


def s14_reorg_refund():
    scen = "s14_reorg_refund"
    maker, taker = parties(scen)
    c = Checks(scen, "Reorg injection: the block with the maker's deadline refund(b) is reorganized away before it is final; the maker stays in RefundSent, re-sends, and finishes only when the refund is final; the taker recovers")
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1", "--vanish-at", "funded")
    taker.wait(400)
    maker.wait_log("READY", 200)
    m = maker.swap()
    warp_to(m["t1"] + 1)
    taker.start("--lots", "1")
    taker.wait_log("R2", 120)
    finalize()
    import re as _re
    tx = wait_for(lambda: next((x for l in (maker.swap() or {}).get("log", []) if "REFUND publishes b" in l
                                for x in _re.findall(r"tx (0x[0-9a-f]{64})", l) if receipt_block(x) is not None), None), 200, "refund included")
    c.ok("maker held the swap in RefundSent (not terminal) after broadcasting", maker.swap()["stage"] == "RefundSent", maker.swap()["stage"])
    r = reorg_out(tx)
    c.facts["reorg"] = r
    c.ok("refund block reorganized away before finality", r["gone_after_reorg"] and r["depth"] < 64, r)
    maker.wait_log("RE-SENT REFUND", 120)
    finalize()
    maker.wait(200)
    taker.wait(300)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("maker re-sent the refund and finished only once it was final", m["stage"] == "Refunded" and any("refund is final" in l for l in m["log"]), m["stage"])
    c.ok("escrow state Refunded", escrow_state(m["escrow_id"]) == "Refunded")
    c.ok("taker recovered the joint note", t["stage"] == "Recovered", t["stage"])
    c.ok("maker tokens fully restored", after["maker_tok"] == before["maker_tok"])
    c.ok("no Claimed event: a never published", claimed_event_count(m["escrow_id"]) == 0)
    return c.save(maker, taker)


def s15_impostor_directory():
    scen = "s15_impostor_directory"
    maker, taker = parties(scen)
    c = Checks(scen, "Impostor: a directory entry naming the real maker's EVM address but signed by another key; the taker refuses before sending an RFQ or paying any premium")
    cfg = json.load(open(os.path.join(taker.dir, "config.json")))
    # re-sign the same (UA, epoch, key, maker address) with the relayer's key instead of the maker's
    other = os.path.join(WORK, scen, "impostor")
    shutil.rmtree(other, ignore_errors=True); os.makedirs(other)
    shutil.copy(os.path.join(maker.dir, "wallet.json"), other)
    shutil.copy(os.path.join(maker.dir, "rfqkey.json"), other)
    icfg = json.load(open(os.path.join(maker.dir, "config.json"))); icfg["evm_key"] = K_RELAYER
    json.dump(icfg, open(os.path.join(other, "config.json"), "w"))
    cfg["maker_dir_sig"] = sh(BIN, "rfqpub", "--dir", other).split()[2]
    json.dump(cfg, open(os.path.join(taker.dir, "config.json"), "w"), indent=2)
    before = snapshot(maker, taker)
    taker.start("--lots", "1")
    code = taker.wait(60)
    after = snapshot(maker, taker)
    out = open(taker.logpath).read()
    c.ok("taker refused to start", code != 0 and "not signed by" in out, code)
    c.ok("no RFQ sent and no premium paid", after["taker_zec"] == before["taker_zec"] and taker.swap() is None)
    return c.save(maker, taker)

def quote_rejected(scen, evil, title, why):
    maker, taker = parties(scen)
    c = Checks(scen, title)
    before = snapshot(maker, taker)
    maker.start("--evil", evil)
    taker.start("--lots", "1")
    taker.wait(300)
    maker.kill()
    t = taker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after)
    c.ok(f"taker rejected the quote ({why})", isinstance(t["stage"], dict) and why in t["stage"].get("Aborted", ""), t["stage"])
    c.ok("taker never funded and paid no premium", t.get("fund_txid") is None and before["taker_zec"] - after["taker_zec"] == FEE)
    return c.save(maker, taker)


def s16_short_reveal_window():
    return quote_rejected("s16_short_reveal_window", "short-reveal",
        "Malicious maker quotes a reveal window W <= F + B_R (a lock could never be revealed safely): taker rejects the quote and never funds",
        "reveal window")


def s17_far_horizon():
    return quote_rejected("s17_far_horizon", "far-horizon",
        "Malicious maker quotes a punish deadline t2 about 136 years away (hostage by delay): taker rejects the quote and never funds",
        "deadline horizon")

def s18_late_funding():
    scen = "s18_late_funding"
    maker, taker = parties(scen)
    c = Checks(scen, "Taker's funding stays unmined until after the maker's no-funding refund (b public): the taker keeps a and sweeps the late note with a+b")
    before = snapshot(maker, taker)
    maker.start("--exit-after", "1")
    taker.start("--lots", "1")
    maker.wait_log("escrow OPEN", 200)
    mine_lock.acquire()                  # hold Zcash blocks: the funding will sit in the mempool
    try:
        # first mine what is already pending (the Accept and its change) and confirm it, so only
        # the funding is held back
        while zrpc("getrawmempool"):
            zrpc("generate", [1])
        zrpc("generate", [2])
        taker.wait_log("FUNDED", 200)
        held = len(zrpc("getrawmempool"))
        m = maker.swap()
        warp_to(m["t0"] - 2 * D_SMALL - 1)
        maker.wait_log("no funding before t0", 120)
        finalize()
        maker.wait(120)
        taker.wait_log("keeping a", 120)
        mid = taker.swap()
    finally:
        mine_lock.release()
    taker.wait(300)
    t, m = taker.swap(), maker.swap()
    after = snapshot(maker, taker)
    c.facts.update(before=before, after=after, escrow_id=m["escrow_id"])
    c.ok("funding was unmined when the maker refunded for lack of funding", held > 0 and any("no funding before t0" in l for l in m["log"]), held)
    c.ok("maker reached Refunded", m["stage"] == "Refunded", m["stage"])
    c.ok("taker kept a while its funding was not visible", mid["stage"] == "Funded" and mid["a"] != "", mid["stage"])
    c.ok("taker swept the late joint note back with (a+b)", t["stage"] == "Recovered" and any("swept" in l for l in t["log"]), t["stage"])
    c.ok("maker tokens fully restored", after["maker_tok"] == before["maker_tok"])
    c.ok("taker ZEC: premium + 4 tx fees lost (exact)", before["taker_zec"] - after["taker_zec"] == PREMIUM + 4 * FEE, before["taker_zec"] - after["taker_zec"])
    return c.save(maker, taker)


NU63 = {"s1": s1_happy, "s2": s2_taker_vanishes_before_funding, "s3": s3_taker_vanishes_after_funding,
        "s4": s4_maker_vanishes_after_open, "s5": s5_rogue_key, "s6": s6_underfund,
        "s8": s8_free_claim_attempt, "s9": s9_late_commit, "s10": s10_lock_without_reveal,
        "s11": s11_maker_liveness_violated, "s12": s12_cover_traffic,
        "s13": s13_reorg_claim, "s14": s14_reorg_refund, "s15": s15_impostor_directory,
        "s16": s16_short_reveal_window, "s17": s17_far_horizon,
        "s18": s18_late_funding}


def write_report():
    rows, detail = [], []
    for name in sorted(os.listdir(EVID)):
        f = os.path.join(EVID, name, "result.json")
        if not os.path.exists(f):
            continue
        r = json.load(open(f))
        ok = all(c["pass"] for c in r["checks"])
        m, t = r.get("maker_swap") or {}, r.get("taker_swap") or {}
        rows.append(f"| `{name}` | {r['title']} | `{r['chain_branch']}` | {m.get('negated')} | {m.get('stage')} | {t.get('stage')} | {'PASS' if ok else 'FAIL'} |")
        detail.append(f"\n### {name}\n\n{r['title']}\n")
        detail += [f"- {'PASS' if c['pass'] else 'FAIL'}: {c['check']} {('(' + c['detail'] + ')') if c['detail'] and c['detail'] != '[]' else ''}" for c in r["checks"]]
        for who, sw in (("taker", t), ("maker", m)):
            if sw:
                detail.append(f"\n{who} log:\n")
                detail += [f"    {l}" for l in sw.get("log", [])]
        for tx in r["facts"].get("zcash_txs", []):
            detail.append(f"- Zcash tx `{tx['txid']}` height {tx['height']}, v{tx['version']}, {tx['ironwood_actions']} Ironwood actions, value balance {tx['ironwood_value_balance']}, transparent in/out {tx['transparent_in']}/{tx['transparent_out']}")
    head = ["# Cloakroom local verification results", "",
            f"Generated by `harness/run.py` on {time.strftime('%Y-%m-%d %H:%M:%S %Z')}. Zebra 7.0.0-rc.0 regtest (branch `37a5165b` = NU6.3, `77190ad9` = NU7) and anvil (chain id 84532) with CloakroomEscrow + MockUSDC. Maker and taker are separate processes that communicate only through shielded memos and the escrow.", "",
            "| Scenario | What it tests | Zcash branch | Shares negated | Maker end | Taker end | Result |", "|---|---|---|---|---|---|---|"]
    open(os.path.join(HERE, "RESULTS.md"), "w").write("\n".join(head + rows + ["", "## Details"] + detail) + "\n")


def main():
    sel = sys.argv[1:] or list(NU63) + ["s7"]
    os.makedirs(EVID, exist_ok=True)
    results = {}
    try:
        nu63 = [s for s in sel if s in NU63]
        if nu63:
            start_chains(nu7=False)
            for s in nu63:
                log(f"=== {s} ===")
                t = time.time()
                try:
                    results[s] = (NU63[s](), round(time.time() - t))
                except Exception as e:
                    log(f"  ERROR {e}")
                    results[s] = (False, round(time.time() - t))
        if "s7" in sel:
            start_chains(nu7=True)
            log("=== s7 ===")
            t = time.time()
            try:
                results["s7"] = (s7_happy_nu7(), round(time.time() - t))
            except Exception as e:
                log(f"  ERROR {e}")
                results["s7"] = (False, round(time.time() - t))
    finally:
        stop_chains()
    log("SUMMARY: " + ", ".join(f"{k} {'PASS' if v[0] else 'FAIL'} ({v[1]}s)" for k, v in results.items()))
    with open(os.path.join(EVID, "summary.json"), "w") as f:
        json.dump({k: {"pass": v[0], "seconds": v[1]} for k, v in results.items()}, f, indent=2)
    write_report()
    sys.exit(0 if all(v[0] for v in results.values()) else 1)


if __name__ == "__main__":
    main()
