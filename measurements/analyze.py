#!/usr/bin/env python3
"""Anonymity-set analysis of Ironwood transactions from compact-block summaries.

Usage: python3 analyze.py NET [DAYS [END_UNIX_TIME]]   (NET = mainnet | testnet)
Reads data/NET.blocks.csv, data/NET.txs.csv and (optional) data/NET.fee_sample.csv.
Writes data/NET.results.json and prints a Markdown report fragment to stdout.
Python stdlib only.
"""
import bisect
import collections
import csv
import json
import math
import os
import sys
import time

NET = sys.argv[1] if len(sys.argv) > 1 else "mainnet"
DAYS = float(sys.argv[2]) if len(sys.argv) > 2 else 30.0
HERE = os.path.dirname(os.path.abspath(__file__))
D = os.path.join(HERE, "data")


def q(xs, p):
    """Linear-interpolated quantile (p in [0,1]) of a list."""
    if not xs:
        return float("nan")
    s = sorted(xs)
    k = (len(s) - 1) * p
    lo, hi = math.floor(k), math.ceil(k)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


def qs(xs):
    return {
        "n": len(xs),
        "mean": (sum(xs) / len(xs)) if xs else float("nan"),
        "min": min(xs) if xs else float("nan"),
        "p1": q(xs, 0.01),
        "p10": q(xs, 0.10),
        "median": q(xs, 0.5),
        "p90": q(xs, 0.90),
        "max": max(xs) if xs else float("nan"),
    }


def fmt(x):
    if isinstance(x, float):
        if math.isnan(x):
            return "n/a"
        return f"{x:.1f}" if abs(x - round(x)) > 1e-9 else f"{int(round(x))}"
    return str(x)


def ascii_bar(vals, labels, width=50):
    m = max(vals) if vals and max(vals) > 0 else 1
    out = []
    for lab, v in zip(labels, vals):
        out.append(f"{lab:>12} | {'#' * int(round(width * v / m)):<{width}} {fmt(v)}")
    return "\n".join(out)


# ---------------------------------------------------------------- load
blocks = {}
with open(os.path.join(D, f"{NET}.blocks.csv")) as f:
    r = csv.reader(f)
    next(r)
    for h, t, n in r:
        blocks[int(h)] = (int(t), int(n))
tip = max(blocks)
first = min(blocks)
t_end = blocks[tip][0]
END_OVERRIDE = int(sys.argv[3]) if len(sys.argv) > 3 else None  # unix time: analyse [END-DAYS, END]
if END_OVERRIDE:
    t_end = END_OVERRIDE
t_start = t_end - DAYS * 86400
in_period = {h for h, (t, _) in blocks.items() if t_start < t <= t_end}
h_lo, h_hi = min(in_period), max(in_period)
n_blocks = len(in_period)
period_secs = blocks[h_hi][0] - blocks[h_lo][0]

# per tx: (height, time, iw, orch, sap_in, sap_out, t_in, t_out)
txs = []
txs_all_period_coinbase = 0
with open(os.path.join(D, f"{NET}.txs.csv")) as f:
    r = csv.reader(f)
    next(r)
    for row in r:
        h = int(row[0])
        if h not in in_period:
            continue
        if row[1] == "0":  # coinbase
            txs_all_period_coinbase += 1
            continue
        iw, orch, si, so, ti, to = map(int, row[2:8])
        txs.append((h, blocks[h][0], iw, orch, si, so, ti, to))

res = {"net": NET, "days": DAYS, "tip": tip, "scanned_from": first,
       "period": {"height_lo": h_lo, "height_hi": h_hi, "blocks": n_blocks,
                  "time_lo": blocks[h_lo][0], "time_hi": blocks[h_hi][0],
                  "time_lo_utc": time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(blocks[h_lo][0])),
                  "time_hi_utc": time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(blocks[h_hi][0])),
                  "mean_block_interval_s": period_secs / (n_blocks - 1)}}


def is_iw(t):
    return t[2] > 0


def fully_shielded_iw_only(t):
    return t[2] > 0 and t[3] == 0 and t[4] == 0 and t[5] == 0 and t[6] == 0 and t[7] == 0


def fingerprint(t):  # structural Cloakroom fingerprint (fee checked separately)
    return fully_shielded_iw_only(t) and t[2] == 2


n_nc = len(txs)
iw_txs = [t for t in txs if is_iw(t)]
fp_txs = [t for t in txs if fingerprint(t)]

# ---------------------------------------------------------------- (f) composition
comp = collections.Counter()
for t in iw_txs:
    tin, tout = t[6] > 0, t[7] > 0
    other = t[3] > 0 or t[4] > 0 or t[5] > 0
    if not tin and not tout and not other:
        k = "fully shielded, Ironwood only"
    elif not tin and not tout and other:
        k = "shielded, Ironwood + Orchard/Sapling (cross-pool)"
    elif tin and not tout:
        k = "transparent in (shielding / t->IW)"
    elif tout and not tin:
        k = "transparent out (deshielding / IW->t)"
    else:
        k = "transparent in and out"
    comp[k] += 1
pool = collections.Counter()
for t in txs:
    key = []
    if t[6] or t[7]:
        key.append("T")
    if t[4] or t[5]:
        key.append("Sapling")
    if t[3]:
        key.append("Orchard")
    if t[2]:
        key.append("Ironwood")
    pool["+".join(key) or "(empty)"] += 1
res["composition"] = {
    "non_coinbase_txs": n_nc, "coinbase_txs": txs_all_period_coinbase,
    "ironwood_txs": len(iw_txs), "fingerprint_structural_txs": len(fp_txs),
    "ironwood_breakdown": dict(comp), "pool_mix_all_txs": dict(pool),
    "turnstile_share_of_ironwood": sum(v for k, v in comp.items() if k.startswith("transparent")) / len(iw_txs),
    "fully_shielded_iw_only_share_of_ironwood": comp["fully shielded, Ironwood only"] / len(iw_txs),
    "fingerprint_share_of_ironwood": len(fp_txs) / len(iw_txs),
    "fingerprint_share_of_all_noncoinbase": len(fp_txs) / n_nc,
    "fingerprint_share_of_fully_shielded_iw_only": len(fp_txs) / comp["fully shielded, Ironwood only"],
}

# ---------------------------------------------------------------- (b) action counts
act = collections.Counter(t[2] for t in iw_txs)
act_fs = collections.Counter(t[2] for t in iw_txs if fully_shielded_iw_only(t))
res["iw_action_counts_all"] = dict(sorted(act.items()))
res["iw_action_counts_fully_shielded"] = dict(sorted(act_fs.items()))

# ---------------------------------------------------------------- fee sample
fee_path = os.path.join(D, f"{NET}.fee_sample.csv")
fee = None
if os.path.exists(fee_path) and not END_OVERRIDE:  # sample was drawn from the default period
    rows = list(csv.DictReader(open(fee_path)))
    fp_rows = [r for r in rows if r["iw_actions"] == "2" and r["orch_actions"] == "0" and r["sap_spends"] == "0"
               and r["sap_outputs"] == "0" and r["t_in"] == "0" and r["t_out"] == "0"]
    oth = [r for r in rows if r not in fp_rows]
    fees = collections.Counter(r["fee"] for r in fp_rows)
    n = len(fp_rows)
    k10 = fees.get("10000", 0)
    p = k10 / n if n else float("nan")
    # Wilson 95% interval
    z = 1.96
    if n:
        den = 1 + z * z / n
        c = (p + z * z / (2 * n)) / den
        hw = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / den
        ci = (c - hw, c + hw)
    else:
        ci = (float("nan"), float("nan"))
    fee = {
        "sample_fingerprint_txs": n,
        "fee_counts": dict(fees.most_common()),
        "p_fee_10000": p, "p_fee_10000_ci95": ci,
        "expiry_delta_counts": dict(collections.Counter(r["expiry_delta"] for r in fp_rows).most_common(12)),
        "lock_time_counts": dict(collections.Counter(r["lock_time"] for r in fp_rows).most_common(5)),
        "version_counts": dict(collections.Counter(r["version"] for r in fp_rows).most_common()),
        "size_counts": dict(collections.Counter(r["size"] for r in fp_rows).most_common(5)),
        "flags_counts": dict(collections.Counter(r["iw_spends_en"] + r["iw_outputs_en"] for r in fp_rows).most_common()),
        "other_sample_txs": len(oth),
        "other_fee_by_actions": {a: dict(collections.Counter(r["fee"] for r in oth if r["iw_actions"] == a).most_common(4))
                                 for a in sorted({r["iw_actions"] for r in oth}, key=int)},
        "sample_height_range": [min(int(r["height"]) for r in rows), max(int(r["height"]) for r in rows)] if rows else None,
    }
    exp40 = sum(1 for r in fp_rows if r["fee"] == "10000" and r["lock_time"] == "0"
                and r["expiry_delta"] and 38 <= int(r["expiry_delta"]) <= 40)
    fee["p_fee10000_lock0_expiry38to40"] = exp40 / n if n else float("nan")
    res["fee_sample"] = fee
P_FEE = fee["p_fee_10000"] if fee else 1.0

# ---------------------------------------------------------------- (a) rates
iw_per_block = collections.Counter(t[0] for t in iw_txs)
fp_per_block = collections.Counter(t[0] for t in fp_txs)
iw_blk = [iw_per_block.get(h, 0) for h in in_period]
fp_blk = [fp_per_block.get(h, 0) for h in in_period]
res["iw_per_block"] = qs(iw_blk)
res["fp_per_block"] = qs(fp_blk)
res["iw_per_block_hist"] = dict(sorted(collections.Counter(min(x, 15) for x in iw_blk).items()))
res["fp_per_block_hist"] = dict(sorted(collections.Counter(min(x, 15) for x in fp_blk).items()))

# hour buckets (UTC, aligned)
hr0 = int(t_start // 3600) + 1  # first full hour
hr1 = int(t_end // 3600)        # exclusive (last full hour ends at hr1*3600)
iw_hr = collections.Counter(int(t[1] // 3600) for t in iw_txs)
fp_hr = collections.Counter(int(t[1] // 3600) for t in fp_txs)
hours = list(range(hr0, hr1))
res["iw_per_hour"] = qs([iw_hr.get(h, 0) for h in hours])
res["fp_per_hour"] = qs([fp_hr.get(h, 0) for h in hours])

# daily trend over the whole scan (not just the period)
day_iw = collections.Counter()
day_fp = collections.Counter()
with open(os.path.join(D, f"{NET}.txs.csv")) as f:
    r = csv.reader(f)
    next(r)
    for row in r:
        if row[1] == "0":
            continue
        h = int(row[0])
        d = time.strftime("%Y-%m-%d", time.gmtime(blocks[h][0]))
        iw, orch, si, so, ti, to = map(int, row[2:8])
        if iw:
            day_iw[d] += 1
            if iw == 2 and not (orch or si or so or ti or to):
                day_fp[d] += 1
res["daily_iw"] = dict(sorted(day_iw.items()))
res["daily_fp"] = dict(sorted(day_fp.items()))

# ---------------------------------------------------------------- full-tx fingerprint weights
# An adversary with full transactions (anyone can fetch them) also sees fee, lock_time and expiry
# height.  Cloakroom builds with zcash_primitives' Builder at target = tip+1 and the default expiry
# delta of 40 blocks, lock_time 0, ZIP 317 fee 10,000 -> expiry - mined_height in [30, 40] if mined
# within 10 blocks.  We estimate, from the random full-tx sample, the probability p_b that a
# structural-fingerprint tx also matches on these fields, per block-load bucket b (number of
# structural-fingerprint txs in the same block), and weight each tx by p_b.
def load_bucket(n):
    return "1" if n == 1 else "2-4" if n <= 4 else "5-15" if n <= 15 else "16+"


def full_match(r):
    return (r["fee"] == "10000" and r["lock_time"] == "0" and r["expiry_delta"] != ""
            and 30 <= int(r["expiry_delta"]) <= 40)


PB = None
if fee:
    tot = collections.Counter()
    hit = collections.Counter()
    for r in fp_rows:
        b = load_bucket(fp_per_block[int(r["height"])])
        tot[b] += 1
        hit[b] += full_match(r)
    allp = sum(hit.values()) / sum(tot.values())
    PB = {b: (hit[b] / tot[b] if tot[b] >= 10 else allp) for b in ("1", "2-4", "5-15", "16+")}
    fee["full_match_overall"] = allp
    fee["full_match_by_load_bucket"] = {b: {"n": tot[b], "match": hit[b], "p_used": PB[b]} for b in PB}
    fee["fee_x_expiry_cluster"] = dict(collections.Counter(
        (r["fee"], "30-40" if r["expiry_delta"] and 30 <= int(r["expiry_delta"]) <= 40 else
         "<=10" if r["expiry_delta"] and int(r["expiry_delta"]) <= 10 else "other") for r in fp_rows).most_common())
    fee["fee_x_expiry_cluster"] = {f"fee={a}, expiry_delta {b}": v for (a, b), v in fee["fee_x_expiry_cluster"].items()}

# ---------------------------------------------------------------- (d)/(e) anonymity sets
WINDOWS = [("1 min", 60), ("5 min", 300), ("15 min", 900), ("60 min", 3600), ("24 h", 86400)]
DELAYS_MIN = [1, 5, 10, 15, 30, 45, 60, 90, 120, 180, 240, 360, 480, 720, 1080, 1440]
starts = sorted(blocks[h][0] for h in in_period)
by_hour_starts = collections.defaultdict(list)
for s_ in starts:
    by_hour_starts[time.gmtime(s_).tm_hour].append(s_)


class Series:
    """Sorted event times with weights; count(a, b) = total weight of events in [a, b)."""

    def __init__(self, pairs):
        pairs = sorted(pairs)
        self.ts = [t for t, _ in pairs]
        self.ws = [w for _, w in pairs]
        self.cum = [0.0]
        for w in self.ws:
            self.cum.append(self.cum[-1] + w)

    def count(self, a, b):
        return self.cum[bisect.bisect_left(self.ts, b)] - self.cum[bisect.bisect_left(self.ts, a)]


def tumbling(S, w):
    return [S.count(k * w, (k + 1) * w) for k in range(int(math.ceil(t_start / w)), int(t_end // w))]


def delay_counts(S, dmin, sts=None):
    d = dmin * 60
    return [S.count(s, s + d + 1e-9) for s in (sts or starts) if s + d <= t_end]


def required(S, k, quant, sts=None):
    # the quantile of counts is monotone non-decreasing in D -> binary search over the grid
    grid = list(range(1, 60)) + list(range(60, 24 * 60 + 1, 5))

    def ok(dmin):
        c = delay_counts(S, dmin, sts)
        return bool(c) and q(c, quant) >= k

    if not ok(grid[-1]):
        return None
    lo, hi = 0, len(grid) - 1
    while lo < hi:
        mid = (lo + hi) // 2
        if ok(grid[mid]):
            hi = mid
        else:
            lo = mid + 1
    return grid[lo]


def run_sets(S, per_block, centers):
    """centers: list of (time, own_weight) for tx-centric windows."""
    out = {"anon": {}, "hour_of_day": {}, "delay": {}, "batch_epochs": {}, "required_delay_min": {}}
    for name, w in WINDOWS:
        tumb = tumbling(S, w)
        lo_ok, hi_ok = t_start + w / 2, t_end - w / 2
        cent = [S.count(t - w / 2, t + w / 2 + 1e-9) - ow + 1 for t, ow in centers if lo_ok <= t <= hi_ok]
        out["anon"][name] = {"window_s": w, "tumbling": qs(tumb), "tx_centric": qs(cent),
                             "tumbling_lt1_share": (sum(1 for x in tumb if x < 1) / len(tumb)) if tumb else float("nan")}
    blk = [per_block.get(h, 0) for h in in_period]
    out["anon"]["same block"] = {"window_s": None, "tumbling": qs(blk),
                                 "tx_centric": qs([per_block[h] - ow + 1 for (t, ow), h in zip(centers, center_heights)]),
                                 "tumbling_lt1_share": sum(1 for x in blk if x < 1) / len(blk)}
    for name, w in [("15 min", 900), ("60 min", 3600)]:
        by = collections.defaultdict(list)
        for k in range(int(math.ceil(t_start / w)), int(t_end // w)):
            by[time.gmtime(k * w).tm_hour].append(S.count(k * w, (k + 1) * w))
        out["hour_of_day"][name] = {h: qs(by[h]) for h in range(24)}
    dow = collections.defaultdict(list)
    for k in range(int(math.ceil(t_start / 3600)), int(t_end // 3600)):
        dow[time.gmtime(k * 3600).tm_wday].append(S.count(k * 3600, (k + 1) * 3600))
    out["day_of_week_60min"] = {d: qs(dow[d]) for d in range(7) if dow[d]}
    for dmin in DELAYS_MIN:
        out["delay"][dmin] = qs(delay_counts(S, dmin))
    for emin in (5, 10, 15, 30, 60, 120, 180, 240, 360, 720, 1440):
        out["batch_epochs"][emin] = qs(tumbling(S, emin * 60))
    for k in (50, 200):
        r_ = {}
        for label, quant in (("median", 0.5), ("p10", 0.10), ("p1", 0.01)):
            r_[label] = required(S, k, quant)
        worst = None
        for hh in range(24):
            x = required(S, k, 0.10, by_hour_starts[hh])
            if x is None:
                worst = (hh, None)
                break
            if worst is None or x > worst[1]:
                worst = (hh, x)
        r_["worst_hour_p10"] = worst
        out["required_delay_min"][k] = r_
    return out


fp_sorted = sorted(fp_txs, key=lambda t: t[1])
center_heights = [t[0] for t in fp_sorted]
S_struct = Series([(t[1], 1.0) for t in fp_sorted])
res["structural"] = run_sets(S_struct, fp_per_block, [(t[1], 1.0) for t in fp_sorted])
if PB:
    wts = [PB[load_bucket(fp_per_block[t[0]])] for t in fp_sorted]
    S_full = Series([(t[1], w) for t, w in zip(fp_sorted, wts)])
    pbw = collections.Counter()
    for t, w in zip(fp_sorted, wts):
        pbw[t[0]] += w
    res["full_estimated"] = run_sets(S_full, pbw, [(t[1], w) for t, w in zip(fp_sorted, wts)])

# conservative: drop every structural-fp tx in a block holding >= 16 of them (bulk/bot bursts an
# adversary might attribute to a single other entity and discount)
nb = [t for t in fp_sorted if fp_per_block[t[0]] < 16]
nb_heights = [t[0] for t in nb]
_saved = center_heights
center_heights = nb_heights
res["structural_no_bursts"] = run_sets(Series([(t[1], 1.0) for t in nb]),
                                       collections.Counter(t[0] for t in nb), [(t[1], 1.0) for t in nb])
center_heights = _saved
res["structural_no_bursts_txs"] = len(nb)

period_h = period_secs / 3600
res["fp_rate_per_hour"] = len(fp_txs) / period_h
res["iw_rate_per_hour"] = len(iw_txs) / period_h
if PB:
    res["full_est_rate_per_hour"] = S_full.cum[-1] / period_h

json.dump(res, open(os.path.join(D, f"{NET}{'_end' + str(END_OVERRIDE) if END_OVERRIDE else ''}.results.json"), "w"), indent=1, default=str)

# ---------------------------------------------------------------- report
P = print
pr = res["period"]
lam = res["fp_rate_per_hour"]
P(f"### {NET}: period {pr['time_lo_utc']} .. {pr['time_hi_utc']} UTC, heights {h_lo}..{h_hi} "
  f"({n_blocks} blocks, mean interval {pr['mean_block_interval_s']:.1f} s)\n")
c = res["composition"]
P(f"- non-coinbase txs: {n_nc}; Ironwood txs: {len(iw_txs)} ({100*len(iw_txs)/n_nc:.1f}%); "
  f"structural fingerprint txs (2 IW actions, nothing else): {len(fp_txs)}")
P(f"- structural-fingerprint rate: {lam:.1f}/hour ({lam*24:.0f}/day); Ironwood rate {res['iw_rate_per_hour']:.1f}/hour"
  + (f"; estimated full-tx-fingerprint rate {res['full_est_rate_per_hour']:.1f}/hour" if PB else "") + "\n")
P("Composition of Ironwood txs:\n\n| class | txs | share |\n|---|---:|---:|")
for k, v in comp.most_common():
    P(f"| {k} | {v} | {100*v/len(iw_txs):.2f}% |")
P(f"\nTurnstile (touches transparent) share of Ironwood txs: {100*c['turnstile_share_of_ironwood']:.2f}%; "
  f"fully shielded Ironwood-only: {100*c['fully_shielded_iw_only_share_of_ironwood']:.2f}%")
P(f"Structural fingerprint share: {100*c['fingerprint_share_of_ironwood']:.2f}% of Ironwood txs, "
  f"{100*c['fingerprint_share_of_all_noncoinbase']:.2f}% of all non-coinbase txs, "
  f"{100*c['fingerprint_share_of_fully_shielded_iw_only']:.2f}% of fully-shielded Ironwood-only txs\n")
P("Pool mix of all non-coinbase txs:\n\n| pools touched | txs | share |\n|---|---:|---:|")
for k, v in pool.most_common():
    P(f"| {k} | {v} | {100*v/n_nc:.2f}% |")
P("\nIronwood action count per tx (counts > 12 grouped):\n\n| actions | all IW txs | share | fully-shielded IW-only | share |\n|---:|---:|---:|---:|---:|")
tot_fs = sum(act_fs.values())
for a in [x for x in sorted(act) if x <= 12]:
    P(f"| {a} | {act[a]} | {100*act[a]/len(iw_txs):.2f}% | {act_fs.get(a,0)} | {100*act_fs.get(a,0)/tot_fs:.2f}% |")
big = sum(v for a, v in act.items() if a > 12)
big_fs = sum(v for a, v in act_fs.items() if a > 12)
P(f"| >12 | {big} | {100*big/len(iw_txs):.2f}% | {big_fs} | {100*big_fs/tot_fs:.2f}% |")
if fee:
    P(f"\nFull-tx sample (GetTransaction, random over the period): {fee['sample_fingerprint_txs']} structural-fingerprint txs, "
      f"heights {fee['sample_height_range']}.\n")
    P(f"- fee = 10,000 zats: {100*fee['p_fee_10000']:.2f}% (Wilson 95% CI {100*fee['p_fee_10000_ci95'][0]:.2f}-{100*fee['p_fee_10000_ci95'][1]:.2f}%); fee counts {fee['fee_counts']}")
    P(f"- fee x expiry-delta cluster: {fee['fee_x_expiry_cluster']}")
    P(f"- expiry delta (expiry_height - mined height), top: {fee['expiry_delta_counts']}")
    P(f"- lock_time: {fee['lock_time_counts']}; version: {fee['version_counts']}; size (bytes): {fee['size_counts']}; IW flags (spends,outputs): {fee['flags_counts']}")
    P(f"- full-tx fingerprint match (fee 10,000, lock_time 0, expiry delta 30-40): {100*fee['full_match_overall']:.2f}% overall; by block-load bucket: "
      + "; ".join(f"{b}: {v['match']}/{v['n']}" for b, v in fee['full_match_by_load_bucket'].items()))
    P(f"- other sampled fully-shielded IW-only txs ({fee['other_sample_txs']}), fee by action count: {fee['other_fee_by_actions']}")
P("\nRates:\n\n| metric | mean | p10 | median | p90 | max |\n|---|---:|---:|---:|---:|---:|")
for lab, key in (("IW txs / block", "iw_per_block"), ("structural-fp txs / block", "fp_per_block"),
                 ("IW txs / hour", "iw_per_hour"), ("structural-fp txs / hour", "fp_per_hour")):
    s = res[key]
    P(f"| {lab} | {s['mean']:.2f} | {fmt(s['p10'])} | {fmt(s['median'])} | {fmt(s['p90'])} | {fmt(s['max'])} |")
P(f"\nStructural-fp txs per block histogram (15 = 15+): {res['fp_per_block_hist']}")
P(f"IW txs per block histogram (15 = 15+): {res['iw_per_block_hist']}")


def report_sets(R, title):
    P(f"\n#### {title}\n")
    P("Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); "
      "*tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.\n")
    P("| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for name in ["same block"] + [w[0] for w in WINDOWS]:
        a = R["anon"][name]
        t_, x_ = a["tumbling"], a["tx_centric"]
        P(f"| {name} | {fmt(t_['p10'])} | {fmt(t_['median'])} | {fmt(t_['p90'])} | {t_['mean']:.1f} | {fmt(t_['min'])} | "
          f"{100*a['tumbling_lt1_share']:.1f}% | {fmt(x_['p10'])} | {fmt(x_['median'])} | {fmt(x_['p90'])} | {fmt(x_['min'])} |")
    hod = R["hour_of_day"]
    P("\nBy UTC hour of day (per 60-min / per 15-min tumbling window):\n\n| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |\n|---:|---:|---:|---:|---:|---:|---:|")
    for hh in range(24):
        a, b = hod["60 min"][hh], hod["15 min"][hh]
        P(f"| {hh:02d} | {fmt(a['p10'])} | {fmt(a['median'])} | {fmt(a['p90'])} | {fmt(b['p10'])} | {fmt(b['median'])} | {fmt(b['p90'])} |")
    P("\nMedian per 60-min window by UTC hour:\n\n```\n" + ascii_bar([hod["60 min"][h]["median"] for h in range(24)], [f"{h:02d}:00" for h in range(24)]) + "\n```")
    names = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
    P("\nBy day of week (60-min windows): " + ", ".join(
        f"{names[d]} median {fmt(v['median'])} / p10 {fmt(v['p10'])}" for d, v in R["day_of_week_60min"].items()))
    P("\nUniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:\n\n| D (min) | min | p1 | p10 | median | p90 |\n|---:|---:|---:|---:|---:|---:|")
    for dmin, s in R["delay"].items():
        P(f"| {dmin} | {fmt(s['min'])} | {fmt(s['p1'])} | {fmt(s['p10'])} | {fmt(s['median'])} | {fmt(s['p90'])} |")
    P("\nFixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:\n\n| epoch (min) | min | p1 | p10 | median |\n|---:|---:|---:|---:|---:|")
    for e, s in R["batch_epochs"].items():
        P(f"| {e} | {fmt(s['min'])} | {fmt(s['p1'])} | {fmt(s['p10'])} | {fmt(s['median'])} |")
    P("\nSmallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):\n\n| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |\n|---:|---:|---:|---:|---|")
    for k, r_ in R["required_delay_min"].items():
        P(f"| {k} | {r_['median']} | {r_['p10']} | {r_['p1']} | {r_['worst_hour_p10']} |")


report_sets(res["structural"], "Structural fingerprint (visible in compact blocks): fully shielded, Ironwood-only, exactly 2 Ironwood actions")
report_sets(res["structural_no_bursts"], f"Conservative: structural fingerprint excluding blocks with >= 16 such txs ({res['structural_no_bursts_txs']} of {len(fp_txs)} txs kept)")
if PB:
    report_sets(res["full_estimated"], "Full-tx fingerprint (estimated): structural + fee 10,000 + lock_time 0 + expiry delta 30-40, each tx weighted by p(bucket)")
P("\nDaily structural-fingerprint txs over the whole scan:\n\n```\n" + ascii_bar(list(res["daily_fp"].values()), list(res["daily_fp"].keys())) + "\n```")
