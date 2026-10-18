#!/usr/bin/env python3
"""What a policy-aware adversary learns from a cover-delayed broadcast (round-2 review, M4).

For random trigger times T0 in the 30-day mainnet window, the adversary knows T0 (a public EVM
event), the policy and its parameters, and every block's look-alike count. It computes the
posterior probability that each observed look-alike transaction is ours (see metrics()).

Policies, with U ~ Uniform[0, X] and a count threshold k:
  uniform   broadcast at T0 + U (no count gate)
  snap      broadcast at max(T0 + U, time of the k-th look-alike), i.e. as soon as both hold
            (the implementation before round 2)
  redraw    broadcast at the end of the first wait whose end has >= k look-alikes since T0,
            drawing a fresh U for each wait
A broadcast at S is mined in the first block with timestamp > S; within that block the posterior
is split evenly over its look-alikes (ours included). Posteriors are estimated by Monte Carlo.

Usage: python3 -I cover_posterior.py [X_minutes k triggers draws]
"""
import bisect, csv, math, os, random, sys

HERE = os.path.dirname(os.path.abspath(__file__))
LO, HI = 3483191, 3517592  # the 30-day window of RESULTS.md

times = {}
for r in csv.DictReader(open(os.path.join(HERE, "data", "mainnet.blocks.csv"))):
    h = int(r["height"])
    if LO <= h <= HI:
        times[h] = int(r["time"])
count = {h: 0 for h in times}
for r in csv.DictReader(open(os.path.join(HERE, "data", "mainnet.txs.csv"))):
    h = int(r["height"])
    if h in count and int(r["iw"]) == 2 and all(int(r[c]) == 0 for c in ("orch", "sap_in", "sap_out", "t_in", "t_out")):
        count[h] += 1
H = sorted(times)
T = [times[h] for h in H]
C = [count[h] for h in H]
CUM = [0]
for c in C:
    CUM.append(CUM[-1] + c)


def first_block_after(t):
    return bisect.bisect_right(T, t)  # index of first block with time > t


def seen_between(t0, t):  # look-alikes in blocks with time in (t0, t]
    return CUM[bisect.bisect_right(T, t)] - CUM[bisect.bisect_right(T, t0)]


def kth_time(t0, k):  # time of the block holding the k-th look-alike after t0
    i0 = bisect.bisect_right(T, t0)
    j = bisect.bisect_left(CUM, CUM[i0] + k, lo=i0)
    return T[j - 1] if j - 1 < len(T) else None


def broadcast(policy, t0, X, k, rng):
    if policy == "uniform":
        return t0 + rng.uniform(0, X)
    if policy == "snap":
        tk = kth_time(t0, k)
        return None if tk is None else max(t0 + rng.uniform(0, X), tk)
    s = t0  # redraw
    for _ in range(1000):
        s += rng.uniform(0, X)
        if seen_between(t0, s) >= k:
            return s
    return None


def metrics(policy, t0, X, k, draws, rng):
    """Bayesian posterior over the OBSERVED look-alikes. f(b) = P(our broadcast lands in block b)
    under the policy (Monte Carlo over the policy's randomness, given the background arrivals).
    If ours is in block b*, the observer sees O_b = C_b + [b = b*] look-alikes per block; with a
    locally smooth background rate the posterior of each observed look-alike in block b is
    f(b) / Z, where Z = sum_b f(b) O_b. Averaged over b* ~ f we report:
      success: the probability that the observer's best guess (a look-alike in the most likely
               block) is ours,
      eff:     2^H of the posterior,
      p_true:  the posterior mass on our transaction."""
    mass = {}
    for _ in range(draws):
        s = broadcast(policy, t0, X, k, rng)
        if s is None:
            return None
        i = first_block_after(s)
        if i >= len(T):
            return None
        mass[i] = mass.get(i, 0) + 1
    f = {i: m / draws for i, m in mass.items()}
    base = sum(f[i] * C[i] for i in f)
    fmax = max(f.values())
    succ = eff = ptrue = 0.0
    for bs, fb in f.items():
        z = base + fb
        h = 0.0
        for i, fi in f.items():
            o = C[i] + (1 if i == bs else 0)
            if o:
                q = fi / z
                h -= o * q * math.log2(q)
        eff += fb * 2 ** h
        ptrue += fb * fb / z
        if fb == fmax:  # observer guesses uniformly among look-alikes in its most likely block(s)
            tied = sum(C[i] + (1 if i == bs else 0) for i in f if f[i] == fmax)
            succ += fb / tied
    return succ, eff, ptrue


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(q * len(xs)))]


def main():
    X = float(sys.argv[1]) * 60 if len(sys.argv) > 1 else 175 * 60
    k = int(sys.argv[2]) if len(sys.argv) > 2 else 50
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 400
    draws = int(sys.argv[4]) if len(sys.argv) > 4 else 2000
    rng = random.Random(20261015)
    span = (T[0], T[-1] - 3 * 86400)
    triggers = [rng.uniform(*span) for _ in range(n)]
    print(f"X = {X/60:.0f} min, k = {k}, {n} triggers, {draws} draws each; blocks {H[0]}..{H[-1]}")
    print("| policy | p_true median | p_true p90 | eff. set p10 | eff. set median | mean delay (min) |")
    print("|---|---:|---:|---:|---:|---:|")
    for pol in ("uniform", "snap", "redraw"):
        g, e, pt, d = [], [], [], []
        for t0 in triggers:
            m = metrics(pol, t0, X, k, draws, rng)
            if m:
                g.append(m[0]); e.append(m[1]); pt.append(m[2])
                d.append(sum((broadcast(pol, t0, X, k, rng) or t0) - t0 for _ in range(50)) / 50 / 60)
        print(f"| {pol} | {pct(pt,.5):.3f} | {pct(pt,.9):.3f} | {pct(e,.1):.1f} | {pct(e,.5):.1f} | {sum(d)/len(d):.0f} |")


if __name__ == "__main__":
    main()
