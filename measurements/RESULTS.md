# Cloakroom anonymity-set measurement (Zcash mainnet, Ironwood pool)

Measured 2026-10-14. Every number below comes from the data files in `data/` and the scripts in this
directory; nothing is extrapolated except where a row is marked *estimated*, and the estimation method
is given.

## TL;DR

Primary period: the last 30 days of mainnet, 2026-09-14 14:20 to 2026-10-14 14:20 UTC, heights
3,483,191 to 3,517,592 (34,402 blocks, mean block interval 75.3 s). Server: `https://zec.rocks:443`
(ECC LightWalletD v0.5.3, chain `main`, consensus branch `37a5165b` = NU6.3/Ironwood).

* **Structural fingerprint** (what compact blocks show: fully shielded, Ironwood only, exactly 2
  Ironwood actions, no transparent/Sapling/Orchard parts). 56,361 txs in the period: 78.3/h on
  average, but the **median hour has only 33** (p10 16), because traffic comes in heavy bursts.
  These txs are 33.1% of Ironwood txs and 17.8% of all non-coinbase txs.
* **Fee.** The server leaves `CompactTx.fee` empty (0) for every tx. I fetched 1,500 randomly chosen
  structural-fingerprint txs in full: **92.8% pay exactly 10,000 zats** (95% CI 91.4 to 94.0%). Of
  the rest, 106 pay 40,000 and 2 pay 100,000.
* **Full-tx fingerprint** (anyone can fetch full txs: fee 10,000, lock_time 0, expiry delta 30 to 40,
  which is what Cloakroom's builder produces). **55.7%** of structural-fingerprint txs match, giving
  about 43.8/h. There is a second big cluster with expiry delta ≤10 (about 43% of the sample), and it
  dominates the bursts.
* **Turnstile share.** 51.0% of Ironwood txs touch the transparent pool: 25.4% deshielding, 13.5%
  shielding, 12.0% both. 43.4% are fully shielded Ironwood-only, and 5.6% cross pools with
  Orchard/Sapling but stay shielded.

Anonymity set, last 30 days. The first three data columns count structural-fingerprint txs in
UTC-aligned (tumbling) windows. The last column is the estimated full-tx-fingerprint count:

| window | p10 | median | p90 | P(window empty) | full-tx est. p10 / median |
|---|---:|---:|---:|---:|---:|
| same block (75 s) | 0 | 0 | 4 | 58.6% | 0 / 0 |
| 1 min | 0 | 0 | 3 | 68.9% | 0 / 0 |
| 5 min | 0 | 3 | 14 | 14.3% | 0 / 1.9 |
| 15 min | 3 | 8 | 39 | 0.7% | 2.1 / 5.5 |
| 60 min | 16 | 33 | 158 | 0.0% | 11.0 / 22.3 |
| 24 h | 492 | 847 | 4,223 | 0.0% | 342 / 535 |

**Recommendation.** Use a uniform random broadcast delay, measured from the observable trigger. Here
"p10" means the anonymity set is reached in 90% of cases, and "p1" in 99%.

| target | compact-block adversary (structural) | full-tx adversary (estimated) | quiet regime (Jul 28 to Aug 27, structural) |
|---|---|---|---|
| k = 50 | U[0, 175 min] (p10), U[0, 245 min] (p1) | U[0, 240 min] (p10), U[0, 320 min] (p1) | U[0, 660 min] (p10), U[0, 1015 min] (p1) |
| k = 200 | U[0, 615 min] (p10), U[0, 745 min] (p1) | U[0, 875 min] (p10), U[0, 1000 min] (p1) | not reached within 24 h, even at the median |

So with current traffic, reaching **k=50 needs about 4 to 5 hours** of jitter, and **k=200 needs about
15 to 17 hours** against a realistic (full-tx) adversary. Traffic was 10× lower one month after
activation, and a fixed delay tuned to today's rate would fall short by about the same factor. The
recommended design is a **count-gated release**. Details, caveats, and why these figures apply to each
of the 5 txs of a swap are in [§5](#5-recommendation-jitter--batching-for-k50-and-k200).

---

## 1. Data and method

### 1.1 Collection

* `src/bin/scan.rs` is a new Rust crate, `cloakroom-measurements`, using `zcash_client_backend
  =0.25.0-pre.1` with tonic + webpki TLS and the rustls ring provider. It calls `GetBlockRange` in
  ranges of ≤1000 blocks with all four pool types requested (Transparent, Sapling, Orchard, Ironwood),
  pausing 400 ms between ranges. For each tx it records height, index, Ironwood action count, Orchard
  action count, Sapling spends/outputs, transparent vin/vout counts, `CompactTx.fee`, and the txid
  (Ironwood txs only). It checks that ranges are complete and contiguous. No errors or retries
  happened.
* `src/bin/fees.rs` calls `GetTransaction` on a list of txids, parses each tx with `zcash_primitives
  =0.31.0-pre.1` under branch `37a5165b` (mainnet) or `77190ad9` (testnet NU7), and records version,
  size, expiry height minus mined height, lock_time, per-pool action counts, value balances, bundle
  flags, and the fee. The fee is computed exactly as Σ value balances − Σ transparent outputs, which
  applies only when there are no transparent inputs. All 1,800 mainnet and 300 testnet fetches parsed,
  and every recomputed txid matched.
* Servers checked: `zec.rocks`, `na.zec.rocks`, `eu.zec.rocks` and `ap.zec.rocks` (port 443) all work.
  All four report ECC LightWalletD v0.5.3, chain main, tip 3,517,592 at collection time.
  `mainnet.lightwalletd.com` does not resolve (DNS) on port 443 or 9067. `testnet.zec.rocks:443` works
  (LightWalletD v0.5.4, chain test, branch `77190ad9`). All data was taken from `zec.rocks:443` and
  `testnet.zec.rocks:443`.
* Mainnet coverage: every block from Ironwood activation 3,428,143 (2026-07-28 14:07 UTC) to tip
  3,517,592 (2026-10-14 14:20 UTC). That is 89,450 blocks and 693,991 tx rows. The primary analysis
  period is the last 30 days. The first 30 days after activation are analysed as a "quiet regime"
  comparison.
* Testnet coverage: heights 4,487,625 to 4,501,625. The analysis uses the last 3 days, heights
  4,488,993 to 4,501,625 (12,633 blocks, mean interval **20.5 s**, not the nominal 25 s).

### 1.2 Definitions

* Coinbase txs (index 0) are excluded everywhere.
* **Ironwood tx**: ≥1 Ironwood action.
* **Structural fingerprint** (Cloakroom RFQ / QUOTE / ACCEPT / FUND / SWEEP all have this shape):
  exactly 2 Ironwood actions, 0 Orchard actions, 0 Sapling spends/outputs, 0 transparent inputs and
  outputs.
* **Full-tx fingerprint**: structural, plus fee = 10,000, lock_time = 0, and expiry_height − mined
  height in [30, 40]. Cloakroom's `wallet.rs` builds with `Builder::new(params, tip+1, ...)` using the
  default `DEFAULT_TX_EXPIRY_DELTA = 40` and ZIP 317 fee `zip317_fee(1,1)` = 10,000. A tx mined within
  10 blocks of construction therefore lands in [30, 40]. All 1,500 sampled txs were v6, 9,166 bytes,
  lock_time 0, with both Ironwood flags set. Version, size and flags therefore add no distinguishing
  power within the structural class.
* **Full-tx weighting.** The full-tx count is estimated by weighting each structural tx by
  p(match | block-load bucket), measured from the random sample. Buckets are defined by the number of
  structural txs in the same block: 1 → 119/155 = 76.8%, 2 to 4 → 243/375 = 64.8%, 5 to 15 → 246/468
  = 52.6%, 16+ → 228/502 = 45.4%. The sample is uniform over structural txs in the period, so it is
  unbiased per bucket. The window-level numbers are still model estimates, not exact counts.
* **Anonymity set** = number of fingerprint-matching txs in the window. Only block timestamps are
  available, so all timing has block granularity. "Tumbling" uses UTC-aligned windows, empty windows
  included. "Tx-centric" uses a window centred on each fingerprint tx, counting the tx itself. This is
  inflated by bursts, because most txs sit inside bursts.
* **Random delay.** A party delays broadcast by U[0, D] after an observable trigger at time t. The
  candidate set is the fingerprint txs mined in [t, t+D]. Trigger times t are every block time in the
  period, so they are uniform over chain time. Under a uniform delay the posterior over candidates is
  close to uniform, so this count also approximates the effective set size. **Batching** uses
  UTC-aligned epochs of length E with release at the end of the epoch, so the set is the fingerprint
  txs in that epoch.

## 2. Mainnet, last 30 days: composition

| | txs | share |
|---|---:|---:|
| non-coinbase txs | 317,351 | 100% |
| Ironwood txs | 170,271 | 53.7% |
| structural fingerprint | 56,361 | 17.8% of all, 33.1% of Ironwood, 76.3% of fully-shielded Ironwood-only |

**(f) Turnstile vs. fully shielded** (Ironwood txs):

| class | txs | share |
|---|---:|---:|
| fully shielded, Ironwood only | 73,874 | 43.39% |
| transparent out only (deshielding IW→t) | 43,238 | 25.39% |
| transparent in only (shielding t→IW) | 23,059 | 13.54% |
| transparent in and out | 20,500 | 12.04% |
| shielded, Ironwood + Orchard/Sapling (pool migration) | 9,600 | 5.64% |
| **touches transparent (turnstile)** | **86,797** | **50.98%** |

Pool mix of all non-coinbase txs: transparent-only 44.1%, T+Ironwood 27.3%, Ironwood-only 23.3%,
Sapling+Ironwood 2.1%, T+Sapling 1.7%, Orchard+Ironwood 0.9%. Every other combination is below 0.3%.

**(b) Ironwood action count per tx:**

| actions | all IW txs | share | fully-shielded IW-only | share |
|---:|---:|---:|---:|---:|
| 1 | 21,379 | 12.56% | 0 | 0.00% |
| **2** | **116,273** | **68.29%** | **56,361** | **76.29%** |
| 3 | 9,976 | 5.86% | 5,846 | 7.91% |
| 4 | 12,151 | 7.14% | 4,387 | 5.94% |
| 5 | 5,145 | 3.02% | 3,528 | 4.78% |
| 6 to 12 | see appendix A | | | |

No fully-shielded Ironwood-only tx has 1 action, because standard padding brings bundles up to ≥2.
The 2-action class is by far the largest, so Cloakroom's shape is the right one to hide in.

**Fee sample** (1,500 random structural txs over heights 3,483,191 to 3,517,480):

| fee × expiry-delta cluster | n | share |
|---|---:|---:|
| fee 10,000, expiry delta 30 to 40 (**Cloakroom-like**) | 836 | 55.7% |
| fee 10,000, expiry delta ≤10 | 547 | 36.5% |
| fee 40,000, expiry delta ≤10 | 102 | 6.8% |
| fee 10,000, other expiry | 9 | 0.6% |
| fee 40,000, expiry 30 to 40 | 4 | 0.3% |
| fee 100,000, expiry 30 to 40 | 2 | 0.1% |

The most common expiry deltas were 40 (423), 5 (296), 39 (229), 4 (221), 38 (106), 3 (76), 37 (44),
2 (39). The sample also includes 300 fully-shielded Ironwood-only txs with other action counts. Those
pay 5,000 × actions (ZIP 317), with a minority paying 4× that. Expiry delta is therefore a strong
wallet fingerprint. The ≤10 cluster is concentrated in bursty blocks: it makes up 268/502 = 53% of the
sampled txs in blocks with 16+ structural txs, against 35/155 = 23% in single-tx blocks.

**(a) Rates:**

| metric | mean | p10 | median | p90 | max |
|---|---:|---:|---:|---:|---:|
| Ironwood txs / block | 4.95 | 0 | 2 | 12 | 218 |
| structural txs / block | 1.64 | 0 | 0 | 4 | 217 |
| Ironwood txs / hour | 236.7 | 80 | 136 | 500 | 3,592 |
| structural txs / hour | 78.4 | 16 | 33 | 158 | 3,510 |

Traffic is very bursty. 33.2% of structural txs sit in 438 blocks holding ≥16 such txs, and 12.4% sit
in 42 blocks holding ≥100. Daily structural counts ranged from 95 to 346/day in August, then 9,002 on
2026-09-03 and 9,949 on 2026-09-21; see the ASCII chart in appendix A. Blocks without a structural tx:
58.6%.

## 3. Mainnet: anonymity sets, last 30 days

**(d)** The three groups are: **S** = structural fingerprint; **S−bursts** = structural excluding
blocks with ≥16 such txs (37,634 of 56,361 txs kept), a conservative view against an adversary that
discounts bulk bursts; **F** = full-tx fingerprint (estimated).

| window | S p10 | S median | S p90 | S−bursts p10 | S−bursts median | F p10 | F median | F p90 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 4 | 0 | 0 | 0 | 0 | 2.6 |
| 1 min | 0 | 0 | 3 | 0 | 0 | 0 | 0 | 1.9 |
| 5 min | 0 | 3 | 14 | 0 | 3 | 0 | 1.9 | 7.7 |
| 15 min | 3 | 8 | 39 | 3 | 8 | 2.1 | 5.5 | 22.2 |
| 60 min | 16 | 33 | 158 | 16 | 33 | 11.0 | 22.3 | 86.3 |
| 24 h | 492 | 847 | 4,223 | 492 | 826 | 342 | 535 | 2,290 |

Tx-centric (S), median / p10: same block 8 / 1, 5 min 17 / 3, 15 min 46 / 7, 60 min 177 / 28,
24 h 3,703 / 700. These medians are large because a typical tx sits inside a burst. S−bursts
tx-centric median for 60 min: 88.

**By UTC hour of day** (S, 60-min windows): medians range from 25.5 (22:00) to 49 (15:00), and p10
from 11.9 (03:00) to 22 (14:00). Activity is fairly flat, with a mild peak from 13:00 to 18:00 UTC.
The full 24-row tables for S, S−bursts and F, including 15-min windows, are in appendix A. By weekday
(60-min medians): Mon 26, Tue 34.5, Wed 34, Thu 40, Fri 33, Sat 36, Sun 33.

**(e) Uniform random delay U[0, D]**, set = fingerprint txs in [t, t+D]:

| D | S p1 | S p10 | S median | F p1 | F p10 | F median |
|---:|---:|---:|---:|---:|---:|---:|
| 15 min | 1 | 3 | 8 | 0.8 | 2.1 | 5.7 |
| 30 min | 3 | 7 | 17 | 2.3 | 4.9 | 11.2 |
| 1 h | 9 | 15 | 33 | 6.4 | 10.9 | 21.9 |
| 2 h | 21 | 34 | 65 | 15.3 | 23.7 | 43.6 |
| 3 h | 35 | 53 | 98 | 25.2 | 37.0 | 65.3 |
| 4 h | 49 | 72 | 131 | 35.5 | 50.3 | 86.7 |
| 6 h | 82 | 113 | 199 | 58.4 | 78.8 | 131.8 |
| 8 h | 117 | 156 | 269 | 83.2 | 107.4 | 178.3 |
| 12 h | 193 | 239 | 425 | 136.6 | 165.2 | 279.5 |
| 18 h | 312 | 363 | 640 | 219.2 | 251.2 | 417.2 |
| 24 h | 413 | 486 | 862 | 289.7 | 338.1 | 556.2 |

**Required D** is the smallest uniform window that reaches the target; the binary search uses a
1-minute grid below 1 h and a 5-minute grid above. Values are in minutes:

| k | S median | S p10 | S p1 | S worst hour p10 | F median | F p10 | F p1 | F worst hour p10 |
|---:|---:|---:|---:|---|---:|---:|---:|---|
| 50 | 95 | 175 | 245 | 220 (01 UTC) | 140 | 240 | 320 | 290 (00 UTC) |
| 200 | 365 | 615 | 745 | 665 (21 UTC) | 535 | 875 | 1000 | 920 (15 UTC) |

**Batching** (UTC epochs of length E), as p10 / min. For S: 1 h 16 / 3; 3 h 52.8 / 36; 4 h 71 / 45;
6 h 109 / 85; 12 h 244 / 183; 24 h 492 / 430. For F: 4 h 49.8 / 32; 6 h 76 / 61; 12 h 168 / 130;
24 h 342 / 301.

### 3.1 Quiet regime: first 30 days after activation

The window is 2026-07-28 14:22 to 2026-08-27 14:18 UTC, heights 3,428,153 to 3,462,537. There were
5,224 structural txs, only **7.3/h** (174/day). The structural share was 10.5% of Ironwood txs, the
turnstile share was 48.7% of Ironwood txs, and no blocks had ≥16 structural txs. The fee sample does
not cover this period, so only S is reported.

| window | S p10 | S median | S p90 |
|---|---:|---:|---:|
| 15 min | 0 | 1 | 4 |
| 60 min | 2 | 6 | 13 |
| 24 h | 104 | 154 | 240 |

Required D: k=50 → 470 (median), 660 (p10), 1,015 (p1) minutes; the worst hour (22 UTC) needs 765 at
p10. k=200 is **not reached within 24 h** even at the median, because the median 24 h count is 154.

## 4. Testnet comparison (last 3 days, NU7)

The window is 2026-10-11 14:29 to 2026-10-14 14:29 UTC, heights 4,488,993 to 4,501,625, 12,633 blocks
at a mean interval of 20.5 s. Server: `testnet.zec.rocks:443`.

* 9,299 non-coinbase txs; 2,860 Ironwood txs (30.8%); 1,593 structural txs (**22.1/h**). Structural
  txs are 55.7% of Ironwood txs, 17.1% of all txs, and 79.1% of fully-shielded Ironwood-only txs.
* Turnstile: **28.85%** of Ironwood txs (deshielding 28.0%, shielding 0.8%). Fully shielded
  Ironwood-only: 70.4%.
* Fee sample: 300/300 pay 10,000 (95% CI 98.7 to 100%). Full-tx match: 92.0%, and the expiry ≤10
  cluster is absent. The testnet population is therefore much more homogeneous. It probably includes
  Cloakroom's own test traffic; I cannot separate that out.
* Anonymity sets (S, tumbling, p10 / median): 15 min 0 / 2; 60 min 2 / 10; 24 h 310 / 552. P(empty
  60-min window) = 4.2%. Activity follows strong daily cycles: the 60-min median is 0 from 02:00 to
  04:00 UTC and peaks at 33 at 18:00.
* Required D: k=50 → 290 (median), 515 (p10), 845 (p1) minutes. k=200 → 775 at the median, and not
  reached at p10 within 24 h. F gives nearly the same figures: 320 / 535 / 870 for k=50.

Testnet has 3.5× fewer structural-fingerprint txs per hour than mainnet's last 30 days (22.1 vs
78.3). Its 3.7× faster blocks give a finer timing resolution but do not add cover.

## 5. Recommendation: jitter / batching for k=50 and k=200

1. **Same-block or immediate broadcast gives no cover.** With no delay, the median structural set in
   the tx's own 75 s block is 0 other txs (58.6% of blocks have none). The p10 for 5-minute windows is
   0. Any Cloakroom tx that is broadcast promptly after an observable trigger can be tied to it.
   Triggers include the Base-side escrow commit/claim and the previous Zcash step.
2. **Uniform jitter at current (Sep to Oct) rates.** The p10 target means 90% of txs achieve k:
   * k ≥ 50: **U[0, 3 h]** against a compact-block adversary (S p10 = 53 at 3 h). **U[0, 4 h]**
     against a full-tx adversary (F p10 = 50.3). **U[0, 5.5 h]** for 99% of txs (F p1 needs 320 min).
   * k ≥ 200: **U[0, 10.5 h]** for S (p10) and **U[0, 15 h]** for F (p10 needs 875 min). For F at
     99%, about 17 h (1,000 min).
3. **Batching** gives the same order of magnitude. With 4 h UTC epochs, S p10 = 71 and F p10 = 50.
   With 12 h epochs, S p10 = 244 and F p10 = 168, so k=200 for F needs epochs between 12 h and 24 h
   (24 h gives F p10 = 342; intermediate lengths were not measured).
   Batching has one advantage: all Cloakroom parties that release at the same epoch boundary hide in
   one another as well. Its drawback is that the release instant itself becomes a timing marker unless
   many other wallets use it too.
4. **Rates vary by 10×, so gate on count, not just time.** In the quiet regime (7.3/h), k=50 needed
   11 to 17 h of jitter, and k=200 was not reachable within a day. A fixed delay calibrated to today
   would silently give about a tenth of the intended k if traffic drops back.
   * Recommended: before broadcasting, watch compact blocks. Release only after at least k
     structural-fingerprint txs have been mined since the trigger, then add an extra U[0, Δ]. Cap the
     total wait by the protocol timelocks (t0/t1/reveal).
   * Caveat: an adversary can satisfy the counter cheaply by flooding fingerprint-shaped txs. Each
     costs 10,000 zats, so k=200 costs about 0.02 ZEC. Combine the count gate with a minimum time
     floor, for example the S/F p10 figures above.
5. **Build at broadcast time.** The expiry height is visible in full transactions, and Cloakroom uses
   the 40-block default.
   * A tx built at trigger time and broadcast after a delay leaks the delay through a reduced expiry
     delta, and it becomes invalid after 40 blocks (about 50 min).
   * So the delay must come *before* tx construction, or the builder must choose its target height at
     broadcast time.
   * Keep fee 10,000, lock_time 0 and expiry delta 40. These match the largest cluster (55.7% of
     structural txs), whereas expiry ≤10 or fee 40,000 would put Cloakroom in a smaller cluster.
6. **Per-step application.** Each swap emits 5 such txs on Zcash: RFQ, QUOTE, ACCEPT, FUND and SWEEP.
   * The three memo txs are interactive and close together. A burst of 3 to 5 fingerprint txs within
     minutes is itself a pattern, because the background is only about 3 per 5 min at the median.
   * The figures above are per-tx sets. Apply independent jitter per step, or batch the memo
     exchange, so that the steps are not linked by timing.
   * Linking a whole swap needs all steps to be matched, so the joint set is not simply the per-tx k.
     Modelling the joint set is outside this measurement.

## 6. Limitations

* **Compact blocks omit data.** There are no amounts or memos; amounts are not visible at all because
  the txs are shielded. There are no value balances, expiry or lock_time, and no fee: `CompactTx.fee`
  was 0 for every tx from both servers (LightWalletD v0.5.3 and v0.5.4). Fee, expiry and lock_time
  come from a **random sample** (1,500 mainnet structural txs, 300 other Ironwood-only txs, 300
  testnet), not from every tx. F is therefore an estimate using block-load-bucket weights. If the
  share of each wallet cluster changes within a bucket over time, the F numbers shift.
* **Single data source.** All blocks come from one server, `zec.rocks`, and are not cross-checked
  against a full node. Range continuity was verified, and all 4 regional zec.rocks endpoints reported
  the same tip.
* **Timing granularity is the block timestamp**, which is miner-set: median interval 53 s, p90 172 s,
  max 930 s. Mempool arrival times, which a network observer could see, are not measured. Such an
  observer gets finer timing and smaller sets for short windows.
* **No adversarial clustering beyond fee and expiry.** Further heuristics would shrink the set further:
  note-age patterns, mempool and IP observation, burst attribution, and cross-chain timing on the EVM
  leg. The S−bursts row shows one such cut. The numbers are therefore upper bounds on what a
  sophisticated adversary faces.
* **Uniform-posterior assumption** in the delay analysis. Background arrivals are bursty, not
  Poisson, so effective (entropy-based) set sizes are somewhat smaller than the counts. This was not
  computed.
* **Testnet** includes developer and test traffic (probably including Cloakroom's own), and its rates
  are not representative of mainnet usage.

## 7. Files and reproduction

| path | content |
|---|---|
| `Cargo.toml`, `src/bin/scan.rs`, `src/bin/fees.rs` | Rust crate: compact-block scanner and full-tx fee/expiry sampler |
| `analyze.py` | stdlib-only analysis: `python3 analyze.py mainnet 30`, `python3 analyze.py mainnet 30 1787840400` (quiet regime), `python3 analyze.py testnet 3` |
| `data/mainnet.blocks.csv` (1.9 MB), `data/mainnet.txs.csv` (37 MB) | raw per-block / per-tx summaries, heights 3,428,143 to 3,517,592 |
| `data/testnet.blocks.csv`, `data/testnet.txs.csv` | testnet heights 4,487,625 to 4,501,625 |
| `data/fee_sample_input.txt`, `data/mainnet.fee_sample.csv` | mainnet sample (seed 20261014) and full-tx results |
| `data/testnet_fee_sample_input.txt`, `data/testnet.fee_sample.csv` | testnet sample and results |
| `data/*.results.json` | all computed statistics (machine readable) |
| `data/mainnet.report.md`, `data/mainnet_early.report.md`, `data/testnet.report.md` | full generated reports (also appended below) |
| `data/*.log` | scan and fetch logs |

```
cargo build --release
./target/release/scan --server https://zec.rocks:443 --start 3428143 --end 3517592 --out data/mainnet
./target/release/fees --server https://zec.rocks:443 --branch 37a5165b --in data/fee_sample_input.txt --out data/mainnet.fee_sample.csv
./target/release/scan --server https://testnet.zec.rocks:443 --start 4487625 --end 4501625 --out data/testnet
./target/release/fees --server https://testnet.zec.rocks:443 --branch 77190ad9 --in data/testnet_fee_sample_input.txt --out data/testnet.fee_sample.csv
python3 analyze.py mainnet 30 > data/mainnet.report.md
```

The data directory totals 40 MB. matplotlib is not installed, so all charts are ASCII.

---

# Appendix A: generated report, mainnet, last 30 days

## mainnet: period 2026-09-14 14:20:40 .. 2026-10-14 14:20:09 UTC, heights 3483191..3517592 (34402 blocks, mean interval 75.3 s)

- non-coinbase txs: 317351; Ironwood txs: 170271 (53.7%); structural fingerprint txs (2 IW actions, nothing else): 56361
- structural-fingerprint rate: 78.3/hour (1879/day); Ironwood rate 236.5/hour; estimated full-tx-fingerprint rate 43.8/hour

Composition of Ironwood txs:

| class | txs | share |
|---|---:|---:|
| fully shielded, Ironwood only | 73874 | 43.39% |
| transparent out (deshielding / IW->t) | 43238 | 25.39% |
| transparent in (shielding / t->IW) | 23059 | 13.54% |
| transparent in and out | 20500 | 12.04% |
| shielded, Ironwood + Orchard/Sapling (cross-pool) | 9600 | 5.64% |

Turnstile (touches transparent) share of Ironwood txs: 50.98%; fully shielded Ironwood-only: 43.39%
Structural fingerprint share: 33.10% of Ironwood txs, 17.76% of all non-coinbase txs, 76.29% of fully-shielded Ironwood-only txs

Pool mix of all non-coinbase txs:

| pools touched | txs | share |
|---|---:|---:|
| T | 139880 | 44.08% |
| T+Ironwood | 86577 | 27.28% |
| Ironwood | 73874 | 23.28% |
| Sapling+Ironwood | 6655 | 2.10% |
| T+Sapling | 5536 | 1.74% |
| Orchard+Ironwood | 2939 | 0.93% |
| T+Orchard | 734 | 0.23% |
| Orchard | 471 | 0.15% |
| Sapling | 448 | 0.14% |
| T+Orchard+Ironwood | 182 | 0.06% |
| T+Sapling+Ironwood | 36 | 0.01% |
| Sapling+Orchard | 7 | 0.00% |
| Sapling+Orchard+Ironwood | 6 | 0.00% |
| T+Sapling+Orchard | 4 | 0.00% |
| T+Sapling+Orchard+Ironwood | 2 | 0.00% |

Ironwood action count per tx (counts > 12 grouped):

| actions | all IW txs | share | fully-shielded IW-only | share |
|---:|---:|---:|---:|---:|
| 1 | 21379 | 12.56% | 0 | 0.00% |
| 2 | 116273 | 68.29% | 56361 | 76.29% |
| 3 | 9976 | 5.86% | 5846 | 7.91% |
| 4 | 12151 | 7.14% | 4387 | 5.94% |
| 5 | 5145 | 3.02% | 3528 | 4.78% |
| 6 | 1348 | 0.79% | 816 | 1.10% |
| 7 | 728 | 0.43% | 442 | 0.60% |
| 8 | 398 | 0.23% | 236 | 0.32% |
| 9 | 436 | 0.26% | 331 | 0.45% |
| 10 | 387 | 0.23% | 291 | 0.39% |
| 11 | 320 | 0.19% | 249 | 0.34% |
| 12 | 189 | 0.11% | 111 | 0.15% |
| >12 | 1541 | 0.91% | 1276 | 1.73% |

Full-tx sample (GetTransaction, random over the period): 1500 structural-fingerprint txs, heights [3483191, 3517480].

- fee = 10,000 zats: 92.80% (Wilson 95% CI 91.38-94.00%); fee counts {'10000': 1392, '40000': 106, '100000': 2}
- fee x expiry-delta cluster: {'fee=10000, expiry_delta 30-40': 836, 'fee=10000, expiry_delta <=10': 547, 'fee=40000, expiry_delta <=10': 102, 'fee=10000, expiry_delta other': 9, 'fee=40000, expiry_delta 30-40': 4, 'fee=100000, expiry_delta 30-40': 2}
- expiry delta (expiry_height - mined height), top: {'40': 423, '5': 296, '39': 229, '4': 221, '38': 106, '3': 76, '37': 44, '2': 39, '36': 16, '35': 13, '1': 10, '0': 7}
- lock_time: {'0': 1500}; version: {'V6': 1500}; size (bytes): {'9166': 1500}; IW flags (spends,outputs): {'11': 1500}
- full-tx fingerprint match (fee 10,000, lock_time 0, expiry delta 30-40): 55.73% overall; by block-load bucket: 1: 119/155; 2-4: 243/375; 5-15: 246/468; 16+: 228/502
- other sampled fully-shielded IW-only txs (300), fee by action count: {'3': {'15000': 89, '60000': 13}, '4': {'20000': 55, '80000': 5}, '5': {'25000': 51, '100000': 16}, '6': {'30000': 14, '120000': 1}, '7': {'35000': 10}, '8': {'40000': 6}, '9': {'45000': 6}, '10': {'50000': 7}, '11': {'55000': 5}, '12': {'60000': 2}, '14': {'70000': 1}, '15': {'75000': 2}, '16': {'80000': 2}, '18': {'90000': 2}, '20': {'100000': 1}, '21': {'105000': 1}, '22': {'110000': 1}, '23': {'115000': 1}, '24': {'120000': 1}, '26': {'130000': 1}, '28': {'140000': 1}, '38': {'190000': 1}, '51': {'255000': 1}, '70': {'350000': 2}, '101': {'505000': 2}}

Rates:

| metric | mean | p10 | median | p90 | max |
|---|---:|---:|---:|---:|---:|
| IW txs / block | 4.95 | 0 | 2 | 12 | 218 |
| structural-fp txs / block | 1.64 | 0 | 0 | 4 | 217 |
| IW txs / hour | 236.67 | 80 | 136 | 499.6 | 3592 |
| structural-fp txs / hour | 78.36 | 16 | 33 | 157.6 | 3510 |

Structural-fp txs per block histogram (15 = 15+): {0: 20168, 1: 6213, 2: 2900, 3: 1553, 4: 898, 5: 566, 6: 430, 7: 283, 8: 209, 9: 196, 10: 145, 11: 102, 12: 111, 13: 76, 14: 59, 15: 493}
IW txs per block histogram (15 = 15+): {0: 11992, 1: 3947, 2: 3346, 3: 2666, 4: 2180, 5: 1680, 6: 1345, 7: 1119, 8: 850, 9: 697, 10: 579, 11: 451, 12: 373, 13: 317, 14: 288, 15: 2572}

#### Structural fingerprint (visible in compact blocks): fully shielded, Ironwood-only, exactly 2 Ironwood actions

Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); *tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.

| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 4 | 1.6 | 0 | 58.6% | 1 | 8 | 138 | 1 |
| 1 min | 0 | 0 | 3 | 1.3 | 0 | 68.9% | 2 | 8 | 145 | 1 |
| 5 min | 0 | 3 | 14 | 6.5 | 0 | 14.3% | 3 | 17 | 313 | 1 |
| 15 min | 3 | 8 | 39.2 | 19.6 | 0 | 0.7% | 7 | 46 | 638 | 1 |
| 60 min | 16 | 33 | 157.6 | 78.4 | 3 | 0.0% | 28 | 177 | 2200 | 3 |
| 24 h | 492.2 | 847 | 4222.8 | 1752.4 | 430 | 0.0% | 700 | 3703 | 9848 | 396 |

By UTC hour of day (per 60-min / per 15-min tumbling window):

| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |
|---:|---:|---:|---:|---:|---:|---:|
| 00 | 13.8 | 26.5 | 71.8 | 2 | 7 | 25.1 |
| 01 | 14.9 | 28.5 | 139.3 | 2 | 7 | 31.4 |
| 02 | 12.9 | 33 | 136.1 | 2 | 8 | 30.1 |
| 03 | 11.9 | 26.5 | 141.3 | 2 | 6.5 | 31.3 |
| 04 | 12.8 | 26 | 129.1 | 2 | 7 | 35.1 |
| 05 | 18.5 | 29.5 | 131.4 | 3 | 8 | 35.6 |
| 06 | 12.9 | 32 | 104.7 | 2 | 7 | 31.1 |
| 07 | 18.9 | 33 | 139.5 | 2.9 | 9 | 36 |
| 08 | 21 | 36 | 113.1 | 3.9 | 9 | 29.1 |
| 09 | 13.9 | 28.5 | 123.6 | 3 | 7 | 30.1 |
| 10 | 18 | 36.5 | 108 | 3 | 8 | 26.5 |
| 11 | 12.9 | 31.5 | 163 | 2 | 7 | 43.2 |
| 12 | 15 | 37 | 138.5 | 3 | 8 | 42.1 |
| 13 | 21.9 | 39 | 240.5 | 4 | 9 | 60.8 |
| 14 | 22 | 35 | 237.4 | 3 | 9 | 58.2 |
| 15 | 20.6 | 49 | 309.7 | 4 | 12 | 82.2 |
| 16 | 20.9 | 39.5 | 209.2 | 4 | 9.5 | 56.1 |
| 17 | 17.8 | 43 | 181.3 | 3.9 | 10 | 52.3 |
| 18 | 15.6 | 45 | 160.6 | 3 | 10 | 48.2 |
| 19 | 14.9 | 37.5 | 136.2 | 3 | 9 | 33.1 |
| 20 | 18.9 | 33 | 164.4 | 3 | 8 | 43.6 |
| 21 | 17 | 29 | 127.9 | 2.9 | 8 | 30.1 |
| 22 | 15 | 25.5 | 92.4 | 3 | 6 | 25.1 |
| 23 | 15.9 | 31 | 94.5 | 2 | 7 | 28.1 |

Median per 60-min window by UTC hour:

```
       00:00 | ###########################                        26.5
       01:00 | #############################                      28.5
       02:00 | ##################################                 33
       03:00 | ###########################                        26.5
       04:00 | ###########################                        26
       05:00 | ##############################                     29.5
       06:00 | #################################                  32
       07:00 | ##################################                 33
       08:00 | #####################################              36
       09:00 | #############################                      28.5
       10:00 | #####################################              36.5
       11:00 | ################################                   31.5
       12:00 | ######################################             37
       13:00 | ########################################           39
       14:00 | ####################################               35
       15:00 | ################################################## 49
       16:00 | ########################################           39.5
       17:00 | ############################################       43
       18:00 | ##############################################     45
       19:00 | ######################################             37.5
       20:00 | ##################################                 33
       21:00 | ##############################                     29
       22:00 | ##########################                         25.5
       23:00 | ################################                   31
```

By day of week (60-min windows): Mon median 26 / p10 14.4, Tue median 34.5 / p10 17, Wed median 34 / p10 15.9, Thu median 40 / p10 17, Fri median 33 / p10 15.5, Sat median 36 / p10 13, Sun median 33 / p10 15.5

Uniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:

| D (min) | min | p1 | p10 | median | p90 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 4 |
| 5 | 0 | 0 | 0 | 3 | 14 |
| 10 | 0 | 0 | 1.9 | 6 | 27 |
| 15 | 0 | 1 | 3 | 8 | 41 |
| 30 | 0 | 3 | 7 | 17 | 81 |
| 45 | 1 | 6 | 11 | 25 | 122 |
| 60 | 3 | 9 | 15 | 33 | 157 |
| 90 | 5 | 15 | 24 | 49 | 241 |
| 120 | 12 | 21 | 34 | 65 | 330 |
| 180 | 25 | 35 | 53 | 98 | 493 |
| 240 | 41 | 49 | 72 | 131 | 667.2 |
| 360 | 72 | 82 | 113 | 199 | 1119 |
| 480 | 106 | 117 | 156 | 269 | 1532.7 |
| 720 | 169 | 193 | 239 | 425 | 2409 |
| 1080 | 286 | 312 | 363 | 640 | 3337 |
| 1440 | 396 | 413 | 486 | 862 | 4127 |

Fixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:

| epoch (min) | min | p1 | p10 | median |
|---:|---:|---:|---:|---:|
| 5 | 0 | 0 | 0 | 3 |
| 10 | 0 | 0 | 1 | 5 |
| 15 | 0 | 1 | 3 | 8 |
| 30 | 1 | 3 | 7 | 16 |
| 60 | 3 | 9 | 16 | 33 |
| 120 | 19 | 21.6 | 33 | 66 |
| 180 | 36 | 37.4 | 52.8 | 98 |
| 240 | 45 | 49.3 | 71 | 134 |
| 360 | 85 | 86.7 | 109.4 | 200 |
| 720 | 183 | 191.1 | 244 | 430 |
| 1440 | 430 | 435.9 | 492.2 | 847 |

Smallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):

| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |
|---:|---:|---:|---:|---|
| 50 | 95 | 175 | 245 | (1, 220) |
| 200 | 365 | 615 | 745 | (21, 665) |

#### Conservative: structural fingerprint excluding blocks with >= 16 such txs (37634 of 56361 txs kept)

Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); *tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.

| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 3 | 1.1 | 0 | 59.9% | 1 | 4 | 12 | 1 |
| 1 min | 0 | 0 | 3 | 0.9 | 0 | 69.7% | 1 | 4 | 12 | 1 |
| 5 min | 0 | 3 | 11 | 4.4 | 0 | 15.5% | 2 | 8 | 24 | 1 |
| 15 min | 3 | 8 | 32 | 13.1 | 0 | 0.8% | 6 | 22 | 56 | 1 |
| 60 min | 16 | 33 | 130.2 | 52.3 | 3 | 0.0% | 24 | 88 | 186 | 3 |
| 24 h | 492.2 | 826 | 3134.2 | 1275.2 | 430 | 0.0% | 642 | 1569 | 3542 | 396 |

By UTC hour of day (per 60-min / per 15-min tumbling window):

| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |
|---:|---:|---:|---:|---:|---:|---:|
| 00 | 13.8 | 26.5 | 70.2 | 2 | 7 | 19.3 |
| 01 | 14.9 | 28.5 | 124 | 2 | 7 | 30.1 |
| 02 | 12.9 | 33 | 110.7 | 2 | 8 | 28.1 |
| 03 | 11.9 | 26.5 | 105 | 2 | 6.5 | 30.1 |
| 04 | 12.8 | 25 | 129.1 | 2 | 7 | 33.1 |
| 05 | 18.5 | 29.5 | 124.8 | 3 | 8 | 30.2 |
| 06 | 12.9 | 32 | 103.4 | 2 | 7 | 27.1 |
| 07 | 18.9 | 32.5 | 115.2 | 2 | 9 | 29.2 |
| 08 | 20.6 | 36 | 95 | 3.9 | 9 | 28 |
| 09 | 13.9 | 28.5 | 102.5 | 3 | 7 | 27 |
| 10 | 18 | 36.5 | 106.3 | 3 | 8 | 24.1 |
| 11 | 12.9 | 31.5 | 142.8 | 2 | 7 | 40.1 |
| 12 | 15 | 37 | 128.1 | 3 | 8 | 33.1 |
| 13 | 21.9 | 39 | 153 | 3.9 | 9 | 39.3 |
| 14 | 22 | 35 | 174.2 | 3 | 9 | 44.4 |
| 15 | 20.6 | 49 | 147.2 | 4 | 11 | 40.2 |
| 16 | 20.9 | 39.5 | 141.4 | 4 | 9 | 37.1 |
| 17 | 17.8 | 43 | 147 | 3 | 9.5 | 37.2 |
| 18 | 15.6 | 45 | 133.7 | 3 | 10 | 38.3 |
| 19 | 14.9 | 37.5 | 130.1 | 3 | 9 | 32.1 |
| 20 | 18.9 | 33 | 110.3 | 3 | 8 | 29.1 |
| 21 | 17 | 29 | 127.9 | 2.9 | 8 | 28.2 |
| 22 | 15 | 25.5 | 92.4 | 3 | 6 | 23.1 |
| 23 | 15.9 | 31 | 79.2 | 2 | 7 | 19.2 |

Median per 60-min window by UTC hour:

```
       00:00 | ###########################                        26.5
       01:00 | #############################                      28.5
       02:00 | ##################################                 33
       03:00 | ###########################                        26.5
       04:00 | ##########################                         25
       05:00 | ##############################                     29.5
       06:00 | #################################                  32
       07:00 | #################################                  32.5
       08:00 | #####################################              36
       09:00 | #############################                      28.5
       10:00 | #####################################              36.5
       11:00 | ################################                   31.5
       12:00 | ######################################             37
       13:00 | ########################################           39
       14:00 | ####################################               35
       15:00 | ################################################## 49
       16:00 | ########################################           39.5
       17:00 | ############################################       43
       18:00 | ##############################################     45
       19:00 | ######################################             37.5
       20:00 | ##################################                 33
       21:00 | ##############################                     29
       22:00 | ##########################                         25.5
       23:00 | ################################                   31
```

By day of week (60-min windows): Mon median 26 / p10 14.4, Tue median 34.5 / p10 17, Wed median 33.5 / p10 15.9, Thu median 40 / p10 17, Fri median 33 / p10 15.5, Sat median 36 / p10 13, Sun median 33 / p10 15.5

Uniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:

| D (min) | min | p1 | p10 | median | p90 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 4 |
| 5 | 0 | 0 | 0 | 3 | 12 |
| 10 | 0 | 0 | 1 | 5 | 23 |
| 15 | 0 | 1 | 3 | 8 | 34 |
| 30 | 0 | 3 | 7 | 17 | 66 |
| 45 | 1 | 6 | 11 | 24 | 99 |
| 60 | 3 | 9 | 15 | 32 | 131 |
| 90 | 5 | 15 | 24 | 48 | 197 |
| 120 | 12 | 21 | 34 | 65 | 267 |
| 180 | 25 | 35 | 53 | 96 | 397 |
| 240 | 41 | 49 | 72 | 128 | 515 |
| 360 | 72 | 82 | 113 | 195 | 763 |
| 480 | 106 | 117 | 156 | 261.5 | 1011 |
| 720 | 169 | 193 | 239 | 416 | 1521 |
| 1080 | 286 | 312 | 363 | 634 | 2322.5 |
| 1440 | 396 | 413 | 486 | 851 | 3204 |

Fixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:

| epoch (min) | min | p1 | p10 | median |
|---:|---:|---:|---:|---:|
| 5 | 0 | 0 | 0 | 3 |
| 10 | 0 | 0 | 1 | 5 |
| 15 | 0 | 1 | 3 | 8 |
| 30 | 1 | 3 | 7 | 16 |
| 60 | 3 | 9 | 16 | 33 |
| 120 | 19 | 21.6 | 33 | 65 |
| 180 | 36 | 37.4 | 52.8 | 96 |
| 240 | 45 | 49.3 | 71 | 126 |
| 360 | 85 | 86.7 | 109.4 | 197 |
| 720 | 183 | 191.1 | 244 | 413 |
| 1440 | 430 | 435.9 | 492.2 | 826 |

Smallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):

| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |
|---:|---:|---:|---:|---|
| 50 | 95 | 175 | 245 | (1, 220) |
| 200 | 370 | 615 | 745 | (21, 665) |

#### Full-tx fingerprint (estimated): structural + fee 10,000 + lock_time 0 + expiry delta 30-40, each tx weighted by p(bucket)

Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); *tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.

| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 2.6 | 0.9 | 0 | 76.7% | 1 | 4.7 | 63.2 | 1 |
| 1 min | 0 | 0 | 1.9 | 0.7 | 0 | 81.6% | 1.6 | 4.7 | 66.4 | 1 |
| 5 min | 0 | 1.9 | 7.7 | 3.6 | 0 | 32.3% | 2.3 | 9.4 | 142.7 | 1 |
| 15 min | 2.1 | 5.5 | 22.2 | 10.9 | 0 | 3.7% | 5.2 | 25.7 | 290.9 | 1 |
| 60 min | 11.0 | 22.3 | 86.3 | 43.8 | 2.3 | 0.0% | 19.4 | 96.5 | 1009.3 | 2.5 |
| 24 h | 341.8 | 535.4 | 2290.4 | 995.2 | 301.0 | 0.0% | 468.6 | 2036.9 | 4807.9 | 279.9 |

By UTC hour of day (per 60-min / per 15-min tumbling window):

| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |
|---:|---:|---:|---:|---:|---:|---:|
| 00 | 9.0 | 18.2 | 44.3 | 1.5 | 4.5 | 15.7 |
| 01 | 10.4 | 19.3 | 76.8 | 1.5 | 4.9 | 18.4 |
| 02 | 9.2 | 22.6 | 78.1 | 1.5 | 5.3 | 17.2 |
| 03 | 8.9 | 17.4 | 76.0 | 1.3 | 4.4 | 17.5 |
| 04 | 8.5 | 18.2 | 74.9 | 1.5 | 4.9 | 19.9 |
| 05 | 12.5 | 20.4 | 75.9 | 2.1 | 5.1 | 20.6 |
| 06 | 9.6 | 20.8 | 62.4 | 1.5 | 5.0 | 18.0 |
| 07 | 13.4 | 21.4 | 79.8 | 2.2 | 6.0 | 19.7 |
| 08 | 13.6 | 24.0 | 64.2 | 2.7 | 6.0 | 17.4 |
| 09 | 9.6 | 19.6 | 70.7 | 2.1 | 4.7 | 16.3 |
| 10 | 12.9 | 24.0 | 65.4 | 2.1 | 5.5 | 15.6 |
| 11 | 9.2 | 20.7 | 86.9 | 1.5 | 5.0 | 24.5 |
| 12 | 10.8 | 23.7 | 79.0 | 2.3 | 5.4 | 24.1 |
| 13 | 15.5 | 25.5 | 129.0 | 2.7 | 6.0 | 31.5 |
| 14 | 15.0 | 23.3 | 126.1 | 2.3 | 6.1 | 30.5 |
| 15 | 14.3 | 30.6 | 154.8 | 2.8 | 7.5 | 40.8 |
| 16 | 14.4 | 26.4 | 109.7 | 2.8 | 6.4 | 29.0 |
| 17 | 11.9 | 28.6 | 98.2 | 2.7 | 6.6 | 28.6 |
| 18 | 11.1 | 29.3 | 88.5 | 2.1 | 6.5 | 26.3 |
| 19 | 10.6 | 25.1 | 74.5 | 2.1 | 6.1 | 18.9 |
| 20 | 13.1 | 21.8 | 88.5 | 2.1 | 5.4 | 22.5 |
| 21 | 12.2 | 19.5 | 72.9 | 2.0 | 5.3 | 18.2 |
| 22 | 10.8 | 17.3 | 54.3 | 2.1 | 4.3 | 15.1 |
| 23 | 11.1 | 20.6 | 54.0 | 1.5 | 5.0 | 15.6 |

Median per 60-min window by UTC hour:

```
       00:00 | ##############################                     18.2
       01:00 | ################################                   19.3
       02:00 | #####################################              22.6
       03:00 | ############################                       17.4
       04:00 | ##############################                     18.2
       05:00 | #################################                  20.4
       06:00 | ##################################                 20.8
       07:00 | ###################################                21.4
       08:00 | #######################################            24.0
       09:00 | ################################                   19.6
       10:00 | #######################################            24.0
       11:00 | ##################################                 20.7
       12:00 | #######################################            23.7
       13:00 | ##########################################         25.5
       14:00 | ######################################             23.3
       15:00 | ################################################## 30.6
       16:00 | ###########################################        26.4
       17:00 | ###############################################    28.6
       18:00 | ################################################   29.3
       19:00 | #########################################          25.1
       20:00 | ####################################               21.8
       21:00 | ################################                   19.5
       22:00 | ############################                       17.3
       23:00 | ##################################                 20.6
```

By day of week (60-min windows): Mon median 18.0 / p10 10.3, Tue median 22.5 / p10 12.6, Wed median 22.6 / p10 11.1, Thu median 26.0 / p10 12.2, Fri median 22.0 / p10 10.8, Sat median 24.0 / p10 9.4, Sun median 22.2 / p10 11.0

Uniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:

| D (min) | min | p1 | p10 | median | p90 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 2.6 |
| 5 | 0 | 0 | 0 | 2.1 | 7.9 |
| 10 | 0 | 0 | 1.2 | 3.9 | 15.6 |
| 15 | 0 | 0.8 | 2.1 | 5.7 | 22.9 |
| 30 | 0 | 2.3 | 4.9 | 11.2 | 45.2 |
| 45 | 0.8 | 4.4 | 7.7 | 16.6 | 66.9 |
| 60 | 2.3 | 6.4 | 10.9 | 21.9 | 87.3 |
| 90 | 3.8 | 10.6 | 17.2 | 32.4 | 133.1 |
| 120 | 9.0 | 15.3 | 23.7 | 43.6 | 181.7 |
| 180 | 18.3 | 25.2 | 37.0 | 65.3 | 269.7 |
| 240 | 30.3 | 35.5 | 50.3 | 86.7 | 363.6 |
| 360 | 51.8 | 58.4 | 78.8 | 131.8 | 593.8 |
| 480 | 76.4 | 83.2 | 107.4 | 178.3 | 813.5 |
| 720 | 120.6 | 136.6 | 165.2 | 279.5 | 1271.3 |
| 1080 | 201.8 | 219.2 | 251.2 | 417.2 | 1794.8 |
| 1440 | 279.7 | 289.7 | 338.1 | 556.2 | 2243.4 |

Fixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:

| epoch (min) | min | p1 | p10 | median |
|---:|---:|---:|---:|---:|
| 5 | 0 | 0 | 0 | 1.9 |
| 10 | 0 | 0 | 0.8 | 3.6 |
| 15 | 0 | 0.8 | 2.1 | 5.5 |
| 30 | 0.8 | 2.3 | 4.9 | 11.1 |
| 60 | 2.3 | 6.4 | 11.0 | 22.3 |
| 120 | 14.1 | 15.8 | 23.4 | 44.2 |
| 180 | 25.7 | 27.0 | 36.3 | 64.9 |
| 240 | 32.2 | 35.4 | 49.8 | 85.8 |
| 360 | 60.9 | 61.9 | 76.1 | 133.4 |
| 720 | 129.8 | 135.7 | 168.3 | 275.5 |
| 1440 | 301.0 | 304.7 | 341.8 | 535.4 |

Smallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):

| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |
|---:|---:|---:|---:|---|
| 50 | 140 | 240 | 320 | (0, 290) |
| 200 | 535 | 875 | 1000 | (15, 920) |

Daily structural-fingerprint txs over the whole scan:

```
  2026-07-28 | #                                                  104
  2026-07-29 | #                                                  142
  2026-07-30 | #                                                  231
  2026-07-31 | #                                                  185
  2026-08-01 | #                                                  136
  2026-08-02 | #                                                  123
  2026-08-03 | #                                                  165
  2026-08-04 | #                                                  140
  2026-08-05 | #                                                  216
  2026-08-06 | #                                                  183
  2026-08-07 | #                                                  211
  2026-08-08 | #                                                  154
  2026-08-09 | #                                                  146
  2026-08-10 | #                                                  138
  2026-08-11 | #                                                  129
  2026-08-12 | #                                                  146
  2026-08-13 | #                                                  104
  2026-08-14 |                                                    95
  2026-08-15 | #                                                  101
  2026-08-16 | #                                                  104
  2026-08-17 | ##                                                 346
  2026-08-18 | #                                                  238
  2026-08-19 | #                                                  211
  2026-08-20 | #                                                  141
  2026-08-21 | #                                                  188
  2026-08-22 | #                                                  237
  2026-08-23 | #                                                  167
  2026-08-24 | #                                                  263
  2026-08-25 | #                                                  250
  2026-08-26 | #                                                  150
  2026-08-27 | #                                                  143
  2026-08-28 | #                                                  144
  2026-08-29 | #                                                  210
  2026-08-30 | #                                                  181
  2026-08-31 | #                                                  182
  2026-09-01 | #                                                  290
  2026-09-02 | #                                                  245
  2026-09-03 | #############################################      9002
  2026-09-04 | ###############                                    3041
  2026-09-05 | ########                                           1667
  2026-09-06 | ##########                                         1980
  2026-09-07 | ########                                           1534
  2026-09-08 | ################                                   3210
  2026-09-09 | ####                                               863
  2026-09-10 | ####                                               790
  2026-09-11 | ##                                                 446
  2026-09-12 | ##                                                 318
  2026-09-13 | ##                                                 385
  2026-09-14 | ##                                                 393
  2026-09-15 | ###############                                    3000
  2026-09-16 | ###                                                680
  2026-09-17 | ########                                           1583
  2026-09-18 | #########                                          1810
  2026-09-19 | #########################                          4906
  2026-09-20 | ####################                               4052
  2026-09-21 | ################################################## 9949
  2026-09-22 | ##############################                     6008
  2026-09-23 | ################                                   3265
  2026-09-24 | #######                                            1327
  2026-09-25 | ####                                               772
  2026-09-26 | #####                                              1067
  2026-09-27 | #####                                              903
  2026-09-28 | ###                                                657
  2026-09-29 | ####                                               711
  2026-09-30 | ####                                               702
  2026-10-01 | ###                                                508
  2026-10-02 | ###                                                502
  2026-10-03 | ##                                                 430
  2026-10-04 | ##                                                 451
  2026-10-05 | ##                                                 453
  2026-10-06 | ###                                                585
  2026-10-07 | ####                                               766
  2026-10-08 | ######                                             1161
  2026-10-09 | #######                                            1490
  2026-10-10 | ###                                                689
  2026-10-11 | ####                                               847
  2026-10-12 | ###                                                650
  2026-10-13 | #####                                              897
  2026-10-14 | ###########################                        5354
```

---

# Appendix B: generated report, mainnet quiet regime (first 30 days after activation)

## mainnet: period 2026-07-28 14:22:09 .. 2026-08-27 14:18:18 UTC, heights 3428153..3462537 (34385 blocks, mean interval 75.4 s)

- non-coinbase txs: 134396; Ironwood txs: 49631 (36.9%); structural fingerprint txs (2 IW actions, nothing else): 5224
- structural-fingerprint rate: 7.3/hour (174/day); Ironwood rate 68.9/hour

Composition of Ironwood txs:

| class | txs | share |
|---|---:|---:|
| shielded, Ironwood + Orchard/Sapling (cross-pool) | 18036 | 36.34% |
| transparent out (deshielding / IW->t) | 11575 | 23.32% |
| fully shielded, Ironwood only | 7413 | 14.94% |
| transparent in (shielding / t->IW) | 7398 | 14.91% |
| transparent in and out | 5209 | 10.50% |

Turnstile (touches transparent) share of Ironwood txs: 48.72%; fully shielded Ironwood-only: 14.94%
Structural fingerprint share: 10.53% of Ironwood txs, 3.89% of all non-coinbase txs, 70.47% of fully-shielded Ironwood-only txs

Pool mix of all non-coinbase txs:

| pools touched | txs | share |
|---|---:|---:|
| T | 67165 | 49.98% |
| T+Ironwood | 23623 | 17.58% |
| Orchard+Ironwood | 17734 | 13.20% |
| Orchard | 9141 | 6.80% |
| Ironwood | 7413 | 5.52% |
| T+Sapling | 6132 | 4.56% |
| T+Orchard | 2117 | 1.58% |
| T+Orchard+Ironwood | 541 | 0.40% |
| Sapling+Ironwood | 283 | 0.21% |
| Sapling | 185 | 0.14% |
| Sapling+Orchard+Ironwood | 19 | 0.01% |
| T+Sapling+Orchard | 15 | 0.01% |
| T+Sapling+Ironwood | 13 | 0.01% |
| Sapling+Orchard | 10 | 0.01% |
| T+Sapling+Orchard+Ironwood | 5 | 0.00% |

Ironwood action count per tx (counts > 12 grouped):

| actions | all IW txs | share | fully-shielded IW-only | share |
|---:|---:|---:|---:|---:|
| 1 | 9943 | 20.03% | 0 | 0.00% |
| 2 | 32033 | 64.54% | 5224 | 70.47% |
| 3 | 1863 | 3.75% | 543 | 7.32% |
| 4 | 3850 | 7.76% | 684 | 9.23% |
| 5 | 1067 | 2.15% | 484 | 6.53% |
| 6 | 294 | 0.59% | 117 | 1.58% |
| 7 | 113 | 0.23% | 43 | 0.58% |
| 8 | 63 | 0.13% | 19 | 0.26% |
| 9 | 40 | 0.08% | 14 | 0.19% |
| 10 | 43 | 0.09% | 24 | 0.32% |
| 11 | 20 | 0.04% | 9 | 0.12% |
| 12 | 7 | 0.01% | 1 | 0.01% |
| >12 | 295 | 0.59% | 251 | 3.39% |

Rates:

| metric | mean | p10 | median | p90 | max |
|---|---:|---:|---:|---:|---:|
| IW txs / block | 1.44 | 0 | 1 | 4 | 45 |
| structural-fp txs / block | 0.15 | 0 | 0 | 1 | 7 |
| IW txs / hour | 68.67 | 41 | 64 | 102.2 | 221 |
| structural-fp txs / hour | 7.25 | 2 | 6 | 13 | 40 |

Structural-fp txs per block histogram (15 = 15+): {0: 30102, 1: 3577, 2: 549, 3: 106, 4: 32, 5: 12, 6: 6, 7: 1}
IW txs per block histogram (15 = 15+): {0: 14747, 1: 7962, 2: 4662, 3: 2745, 4: 1693, 5: 1003, 6: 592, 7: 376, 8: 227, 9: 131, 10: 74, 11: 53, 12: 34, 13: 26, 14: 17, 15: 43}

#### Structural fingerprint (visible in compact blocks): fully shielded, Ironwood-only, exactly 2 Ironwood actions

Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); *tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.

| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 1 | 0.2 | 0 | 87.5% | 1 | 1 | 3 | 1 |
| 1 min | 0 | 0 | 0 | 0.1 | 0 | 90.2% | 1 | 1 | 3 | 1 |
| 5 min | 0 | 0 | 2 | 0.6 | 0 | 59.9% | 1 | 2 | 4 | 1 |
| 15 min | 0 | 1 | 4 | 1.8 | 0 | 25.3% | 1 | 3 | 7 | 1 |
| 60 min | 2 | 6 | 13 | 7.3 | 0 | 1.8% | 4 | 9 | 22 | 1 |
| 24 h | 104 | 154 | 240.4 | 173.8 | 95 | 0.0% | 125 | 188 | 275 | 59 |

By UTC hour of day (per 60-min / per 15-min tumbling window):

| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |
|---:|---:|---:|---:|---:|---:|---:|
| 00 | 2 | 5 | 9 | 0 | 1 | 3 |
| 01 | 2 | 5 | 8.1 | 0 | 1 | 3 |
| 02 | 1 | 3.5 | 9.3 | 0 | 1 | 3 |
| 03 | 2 | 5 | 11.1 | 0 | 1 | 4 |
| 04 | 2 | 5.5 | 11 | 0 | 1 | 3 |
| 05 | 1 | 5 | 12.3 | 0 | 1 | 3 |
| 06 | 2 | 5 | 7.2 | 0 | 1 | 3 |
| 07 | 2.9 | 8 | 15.2 | 0 | 2 | 5 |
| 08 | 3 | 7.5 | 12.1 | 0 | 2 | 4 |
| 09 | 2.9 | 7.5 | 12.2 | 0 | 2 | 4 |
| 10 | 1.9 | 5 | 9.1 | 0 | 1 | 3 |
| 11 | 2 | 6.5 | 11 | 0 | 1 | 4 |
| 12 | 2.9 | 6 | 13.2 | 0 | 2 | 4 |
| 13 | 3.8 | 8 | 13.4 | 0 | 2 | 4.1 |
| 14 | 2.8 | 7 | 15.6 | 0 | 2 | 5 |
| 15 | 3 | 6.5 | 14.5 | 0 | 1 | 5 |
| 16 | 3.9 | 7.5 | 15.1 | 0 | 2 | 4 |
| 17 | 2.9 | 8 | 16.2 | 0 | 2 | 5 |
| 18 | 4 | 7 | 14.3 | 0 | 2 | 4 |
| 19 | 3 | 7.5 | 16.1 | 0 | 2 | 5 |
| 20 | 2.9 | 7 | 14.1 | 0 | 1 | 4 |
| 21 | 2 | 6 | 14.2 | 0 | 1 | 4 |
| 22 | 2 | 6.5 | 13.5 | 0 | 2 | 4 |
| 23 | 2 | 5 | 11.1 | 0 | 1 | 4 |

Median per 60-min window by UTC hour:

```
       00:00 | ###############################                    5
       01:00 | ###############################                    5
       02:00 | ######################                             3.5
       03:00 | ###############################                    5
       04:00 | ##################################                 5.5
       05:00 | ###############################                    5
       06:00 | ###############################                    5
       07:00 | ################################################## 8
       08:00 | ###############################################    7.5
       09:00 | ###############################################    7.5
       10:00 | ###############################                    5
       11:00 | #########################################          6.5
       12:00 | ######################################             6
       13:00 | ################################################## 8
       14:00 | ############################################       7
       15:00 | #########################################          6.5
       16:00 | ###############################################    7.5
       17:00 | ################################################## 8
       18:00 | ############################################       7
       19:00 | ###############################################    7.5
       20:00 | ############################################       7
       21:00 | ######################################             6
       22:00 | #########################################          6.5
       23:00 | ###############################                    5
```

By day of week (60-min windows): Mon median 7.5 / p10 2, Tue median 7 / p10 2.4, Wed median 6 / p10 2.9, Thu median 5.5 / p10 2, Fri median 6 / p10 2, Sat median 6 / p10 2, Sun median 5 / p10 2

Uniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:

| D (min) | min | p1 | p10 | median | p90 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 1 |
| 5 | 0 | 0 | 0 | 0 | 2 |
| 10 | 0 | 0 | 0 | 1 | 3 |
| 15 | 0 | 0 | 0 | 1 | 4 |
| 30 | 0 | 0 | 1 | 3 | 7 |
| 45 | 0 | 0 | 1 | 5 | 10 |
| 60 | 0 | 0 | 2 | 6 | 13 |
| 90 | 0 | 1 | 4 | 9 | 19 |
| 120 | 0 | 2 | 6 | 12 | 25 |
| 180 | 0 | 4 | 11 | 19 | 36 |
| 240 | 1 | 5 | 15 | 25 | 47 |
| 360 | 6 | 9.1 | 25 | 38 | 70 |
| 480 | 11 | 15 | 34 | 51 | 91 |
| 720 | 25 | 31 | 55 | 76 | 131 |
| 1080 | 48 | 52 | 83 | 119 | 186 |
| 1440 | 59 | 78 | 112 | 159 | 244 |

Fixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:

| epoch (min) | min | p1 | p10 | median |
|---:|---:|---:|---:|---:|
| 5 | 0 | 0 | 0 | 0 |
| 10 | 0 | 0 | 0 | 1 |
| 15 | 0 | 0 | 0 | 1 |
| 30 | 0 | 0 | 1 | 3 |
| 60 | 0 | 0 | 2 | 6 |
| 120 | 0 | 2 | 6 | 12 |
| 180 | 0 | 5.4 | 11 | 19 |
| 240 | 2 | 5.3 | 15 | 25 |
| 360 | 11 | 12.2 | 25 | 36 |
| 720 | 32 | 34.3 | 57.4 | 72 |
| 1440 | 95 | 96.7 | 104 | 154 |

Smallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):

| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |
|---:|---:|---:|---:|---|
| 50 | 470 | 660 | 1015 | (22, 765) |
| 200 | None | None | None | (0, None) |

#### Conservative: structural fingerprint excluding blocks with >= 16 such txs (5224 of 5224 txs kept)

Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); *tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.

| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 1 | 0.2 | 0 | 87.5% | 1 | 1 | 3 | 1 |
| 1 min | 0 | 0 | 0 | 0.1 | 0 | 90.2% | 1 | 1 | 3 | 1 |
| 5 min | 0 | 0 | 2 | 0.6 | 0 | 59.9% | 1 | 2 | 4 | 1 |
| 15 min | 0 | 1 | 4 | 1.8 | 0 | 25.3% | 1 | 3 | 7 | 1 |
| 60 min | 2 | 6 | 13 | 7.3 | 0 | 1.8% | 4 | 9 | 22 | 1 |
| 24 h | 104 | 154 | 240.4 | 173.8 | 95 | 0.0% | 125 | 188 | 275 | 59 |

By UTC hour of day (per 60-min / per 15-min tumbling window):

| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |
|---:|---:|---:|---:|---:|---:|---:|
| 00 | 2 | 5 | 9 | 0 | 1 | 3 |
| 01 | 2 | 5 | 8.1 | 0 | 1 | 3 |
| 02 | 1 | 3.5 | 9.3 | 0 | 1 | 3 |
| 03 | 2 | 5 | 11.1 | 0 | 1 | 4 |
| 04 | 2 | 5.5 | 11 | 0 | 1 | 3 |
| 05 | 1 | 5 | 12.3 | 0 | 1 | 3 |
| 06 | 2 | 5 | 7.2 | 0 | 1 | 3 |
| 07 | 2.9 | 8 | 15.2 | 0 | 2 | 5 |
| 08 | 3 | 7.5 | 12.1 | 0 | 2 | 4 |
| 09 | 2.9 | 7.5 | 12.2 | 0 | 2 | 4 |
| 10 | 1.9 | 5 | 9.1 | 0 | 1 | 3 |
| 11 | 2 | 6.5 | 11 | 0 | 1 | 4 |
| 12 | 2.9 | 6 | 13.2 | 0 | 2 | 4 |
| 13 | 3.8 | 8 | 13.4 | 0 | 2 | 4.1 |
| 14 | 2.8 | 7 | 15.6 | 0 | 2 | 5 |
| 15 | 3 | 6.5 | 14.5 | 0 | 1 | 5 |
| 16 | 3.9 | 7.5 | 15.1 | 0 | 2 | 4 |
| 17 | 2.9 | 8 | 16.2 | 0 | 2 | 5 |
| 18 | 4 | 7 | 14.3 | 0 | 2 | 4 |
| 19 | 3 | 7.5 | 16.1 | 0 | 2 | 5 |
| 20 | 2.9 | 7 | 14.1 | 0 | 1 | 4 |
| 21 | 2 | 6 | 14.2 | 0 | 1 | 4 |
| 22 | 2 | 6.5 | 13.5 | 0 | 2 | 4 |
| 23 | 2 | 5 | 11.1 | 0 | 1 | 4 |

Median per 60-min window by UTC hour:

```
       00:00 | ###############################                    5
       01:00 | ###############################                    5
       02:00 | ######################                             3.5
       03:00 | ###############################                    5
       04:00 | ##################################                 5.5
       05:00 | ###############################                    5
       06:00 | ###############################                    5
       07:00 | ################################################## 8
       08:00 | ###############################################    7.5
       09:00 | ###############################################    7.5
       10:00 | ###############################                    5
       11:00 | #########################################          6.5
       12:00 | ######################################             6
       13:00 | ################################################## 8
       14:00 | ############################################       7
       15:00 | #########################################          6.5
       16:00 | ###############################################    7.5
       17:00 | ################################################## 8
       18:00 | ############################################       7
       19:00 | ###############################################    7.5
       20:00 | ############################################       7
       21:00 | ######################################             6
       22:00 | #########################################          6.5
       23:00 | ###############################                    5
```

By day of week (60-min windows): Mon median 7.5 / p10 2, Tue median 7 / p10 2.4, Wed median 6 / p10 2.9, Thu median 5.5 / p10 2, Fri median 6 / p10 2, Sat median 6 / p10 2, Sun median 5 / p10 2

Uniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:

| D (min) | min | p1 | p10 | median | p90 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 1 |
| 5 | 0 | 0 | 0 | 0 | 2 |
| 10 | 0 | 0 | 0 | 1 | 3 |
| 15 | 0 | 0 | 0 | 1 | 4 |
| 30 | 0 | 0 | 1 | 3 | 7 |
| 45 | 0 | 0 | 1 | 5 | 10 |
| 60 | 0 | 0 | 2 | 6 | 13 |
| 90 | 0 | 1 | 4 | 9 | 19 |
| 120 | 0 | 2 | 6 | 12 | 25 |
| 180 | 0 | 4 | 11 | 19 | 36 |
| 240 | 1 | 5 | 15 | 25 | 47 |
| 360 | 6 | 9.1 | 25 | 38 | 70 |
| 480 | 11 | 15 | 34 | 51 | 91 |
| 720 | 25 | 31 | 55 | 76 | 131 |
| 1080 | 48 | 52 | 83 | 119 | 186 |
| 1440 | 59 | 78 | 112 | 159 | 244 |

Fixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:

| epoch (min) | min | p1 | p10 | median |
|---:|---:|---:|---:|---:|
| 5 | 0 | 0 | 0 | 0 |
| 10 | 0 | 0 | 0 | 1 |
| 15 | 0 | 0 | 0 | 1 |
| 30 | 0 | 0 | 1 | 3 |
| 60 | 0 | 0 | 2 | 6 |
| 120 | 0 | 2 | 6 | 12 |
| 180 | 0 | 5.4 | 11 | 19 |
| 240 | 2 | 5.3 | 15 | 25 |
| 360 | 11 | 12.2 | 25 | 36 |
| 720 | 32 | 34.3 | 57.4 | 72 |
| 1440 | 95 | 96.7 | 104 | 154 |

Smallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):

| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |
|---:|---:|---:|---:|---|
| 50 | 470 | 660 | 1015 | (22, 765) |
| 200 | None | None | None | (0, None) |

Daily structural-fingerprint txs over the whole scan:

```
  2026-07-28 | #                                                  104
  2026-07-29 | #                                                  142
  2026-07-30 | #                                                  231
  2026-07-31 | #                                                  185
  2026-08-01 | #                                                  136
  2026-08-02 | #                                                  123
  2026-08-03 | #                                                  165
  2026-08-04 | #                                                  140
  2026-08-05 | #                                                  216
  2026-08-06 | #                                                  183
  2026-08-07 | #                                                  211
  2026-08-08 | #                                                  154
  2026-08-09 | #                                                  146
  2026-08-10 | #                                                  138
  2026-08-11 | #                                                  129
  2026-08-12 | #                                                  146
  2026-08-13 | #                                                  104
  2026-08-14 |                                                    95
  2026-08-15 | #                                                  101
  2026-08-16 | #                                                  104
  2026-08-17 | ##                                                 346
  2026-08-18 | #                                                  238
  2026-08-19 | #                                                  211
  2026-08-20 | #                                                  141
  2026-08-21 | #                                                  188
  2026-08-22 | #                                                  237
  2026-08-23 | #                                                  167
  2026-08-24 | #                                                  263
  2026-08-25 | #                                                  250
  2026-08-26 | #                                                  150
  2026-08-27 | #                                                  143
  2026-08-28 | #                                                  144
  2026-08-29 | #                                                  210
  2026-08-30 | #                                                  181
  2026-08-31 | #                                                  182
  2026-09-01 | #                                                  290
  2026-09-02 | #                                                  245
  2026-09-03 | #############################################      9002
  2026-09-04 | ###############                                    3041
  2026-09-05 | ########                                           1667
  2026-09-06 | ##########                                         1980
  2026-09-07 | ########                                           1534
  2026-09-08 | ################                                   3210
  2026-09-09 | ####                                               863
  2026-09-10 | ####                                               790
  2026-09-11 | ##                                                 446
  2026-09-12 | ##                                                 318
  2026-09-13 | ##                                                 385
  2026-09-14 | ##                                                 393
  2026-09-15 | ###############                                    3000
  2026-09-16 | ###                                                680
  2026-09-17 | ########                                           1583
  2026-09-18 | #########                                          1810
  2026-09-19 | #########################                          4906
  2026-09-20 | ####################                               4052
  2026-09-21 | ################################################## 9949
  2026-09-22 | ##############################                     6008
  2026-09-23 | ################                                   3265
  2026-09-24 | #######                                            1327
  2026-09-25 | ####                                               772
  2026-09-26 | #####                                              1067
  2026-09-27 | #####                                              903
  2026-09-28 | ###                                                657
  2026-09-29 | ####                                               711
  2026-09-30 | ####                                               702
  2026-10-01 | ###                                                508
  2026-10-02 | ###                                                502
  2026-10-03 | ##                                                 430
  2026-10-04 | ##                                                 451
  2026-10-05 | ##                                                 453
  2026-10-06 | ###                                                585
  2026-10-07 | ####                                               766
  2026-10-08 | ######                                             1161
  2026-10-09 | #######                                            1490
  2026-10-10 | ###                                                689
  2026-10-11 | ####                                               847
  2026-10-12 | ###                                                650
  2026-10-13 | #####                                              897
  2026-10-14 | ###########################                        5354
```

---

# Appendix C: generated report, testnet, last 3 days

## testnet: period 2026-10-11 14:29:37 .. 2026-10-14 14:29:24 UTC, heights 4488993..4501625 (12633 blocks, mean interval 20.5 s)

- non-coinbase txs: 9299; Ironwood txs: 2860 (30.8%); structural fingerprint txs (2 IW actions, nothing else): 1593
- structural-fingerprint rate: 22.1/hour (531/day); Ironwood rate 39.7/hour; estimated full-tx-fingerprint rate 20.4/hour

Composition of Ironwood txs:

| class | txs | share |
|---|---:|---:|
| fully shielded, Ironwood only | 2014 | 70.42% |
| transparent out (deshielding / IW->t) | 801 | 28.01% |
| transparent in (shielding / t->IW) | 24 | 0.84% |
| shielded, Ironwood + Orchard/Sapling (cross-pool) | 21 | 0.73% |

Turnstile (touches transparent) share of Ironwood txs: 28.85%; fully shielded Ironwood-only: 70.42%
Structural fingerprint share: 55.70% of Ironwood txs, 17.13% of all non-coinbase txs, 79.10% of fully-shielded Ironwood-only txs

Pool mix of all non-coinbase txs:

| pools touched | txs | share |
|---|---:|---:|
| T | 6438 | 69.23% |
| Ironwood | 2014 | 21.66% |
| T+Ironwood | 824 | 8.86% |
| Sapling+Ironwood | 19 | 0.20% |
| Orchard+Ironwood | 2 | 0.02% |
| Sapling | 1 | 0.01% |
| T+Sapling+Ironwood | 1 | 0.01% |

Ironwood action count per tx (counts > 12 grouped):

| actions | all IW txs | share | fully-shielded IW-only | share |
|---:|---:|---:|---:|---:|
| 1 | 2 | 0.07% | 1 | 0.05% |
| 2 | 2340 | 81.82% | 1593 | 79.10% |
| 3 | 108 | 3.78% | 71 | 3.53% |
| 4 | 52 | 1.82% | 45 | 2.23% |
| 5 | 39 | 1.36% | 29 | 1.44% |
| 6 | 19 | 0.66% | 16 | 0.79% |
| 7 | 12 | 0.42% | 10 | 0.50% |
| 8 | 22 | 0.77% | 19 | 0.94% |
| 9 | 16 | 0.56% | 11 | 0.55% |
| 10 | 10 | 0.35% | 9 | 0.45% |
| 11 | 5 | 0.17% | 4 | 0.20% |
| 12 | 12 | 0.42% | 11 | 0.55% |
| >12 | 223 | 7.80% | 195 | 9.68% |

Full-tx sample (GetTransaction, random over the period): 300 structural-fingerprint txs, heights [4488995, 4501555].

- fee = 10,000 zats: 100.00% (Wilson 95% CI 98.74-100.00%); fee counts {'10000': 300}
- fee x expiry-delta cluster: {'fee=10000, expiry_delta 30-40': 276, 'fee=10000, expiry_delta other': 24}
- expiry delta (expiry_height - mined height), top: {'40': 63, '37': 52, '39': 48, '38': 39, '36': 29, '35': 16, '34': 11, '': 8, '32': 6, '33': 5, '27': 5, '31': 4}
- lock_time: {'0': 300}; version: {'V6': 300}; size (bytes): {'9166': 300}; IW flags (spends,outputs): {'11': 300}
- full-tx fingerprint match (fee 10,000, lock_time 0, expiry delta 30-40): 92.00% overall; by block-load bucket: 1: 81/88; 2-4: 79/86; 5-15: 107/114; 16+: 9/12
- other sampled fully-shielded IW-only txs (0), fee by action count: {}

Rates:

| metric | mean | p10 | median | p90 | max |
|---|---:|---:|---:|---:|---:|
| IW txs / block | 0.23 | 0 | 0 | 0 | 21 |
| structural-fp txs / block | 0.13 | 0 | 0 | 0 | 20 |
| IW txs / hour | 39.86 | 7 | 26 | 66 | 342 |
| structural-fp txs / hour | 22.31 | 2 | 10 | 38 | 278 |

Structural-fp txs per block histogram (15 = 15+): {0: 11898, 1: 467, 2: 103, 3: 53, 4: 30, 5: 16, 6: 19, 7: 16, 8: 5, 9: 10, 10: 5, 11: 1, 12: 2, 13: 3, 14: 1, 15: 4}
IW txs per block histogram (15 = 15+): {0: 11403, 1: 634, 2: 257, 3: 139, 4: 66, 5: 44, 6: 21, 7: 21, 8: 10, 9: 8, 10: 9, 11: 5, 12: 4, 13: 2, 14: 2, 15: 8}

#### Structural fingerprint (visible in compact blocks): fully shielded, Ironwood-only, exactly 2 Ironwood actions

Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); *tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.

| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 0 | 0.1 | 0 | 94.2% | 1 | 3 | 10 | 1 |
| 1 min | 0 | 0 | 1 | 0.4 | 0 | 83.9% | 1 | 3 | 10 | 1 |
| 5 min | 0 | 1 | 4 | 1.8 | 0 | 48.8% | 1 | 7 | 26 | 1 |
| 15 min | 0 | 2 | 10 | 5.6 | 0 | 26.5% | 3 | 16 | 73 | 1 |
| 60 min | 2 | 10 | 38 | 22.3 | 0 | 4.2% | 11 | 41 | 275 | 1 |
| 24 h | 309.6 | 552 | 794.4 | 552.0 | 249 | 0.0% | 238 | 905 | 1011 | 147 |

By UTC hour of day (per 60-min / per 15-min tumbling window):

| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |
|---:|---:|---:|---:|---:|---:|---:|
| 00 | 3.8 | 7 | 10.2 | 0 | 1 | 4.9 |
| 01 | 2 | 6 | 6.8 | 0 | 1 | 2.9 |
| 02 | 1.2 | 6 | 14.8 | 0 | 1 | 5.8 |
| 03 | 2.4 | 12 | 20.8 | 0 | 3 | 5.9 |
| 04 | 1.2 | 6 | 25.2 | 0 | 1 | 8.7 |
| 05 | 6 | 22 | 26.8 | 0 | 3 | 8 |
| 06 | 7.8 | 27 | 31 | 0 | 2.5 | 11.6 |
| 07 | 3 | 7 | 23 | 0 | 2 | 7 |
| 08 | 3.4 | 13 | 29 | 0 | 2 | 10.8 |
| 09 | 17 | 29 | 49.8 | 1.1 | 6 | 20.2 |
| 10 | 6 | 10 | 50 | 0.1 | 3 | 16.3 |
| 11 | 4.8 | 12 | 24.8 | 1 | 2.5 | 8.6 |
| 12 | 3.2 | 8 | 20 | 0 | 2 | 5.9 |
| 13 | 6.2 | 7 | 11 | 0 | 2 | 3.9 |
| 14 | 6.5 | 8.5 | 10.5 | 1 | 2 | 4 |
| 15 | 10.8 | 38 | 210.8 | 1 | 9.5 | 67.4 |
| 16 | 10.6 | 25 | 227.4 | 0.3 | 6.5 | 69.6 |
| 17 | 9.8 | 17 | 80.2 | 1.1 | 3.5 | 14.5 |
| 18 | 14.6 | 33 | 39.4 | 0.1 | 5 | 14.9 |
| 19 | 2.4 | 4 | 8 | 0 | 1 | 2.9 |
| 20 | 4.2 | 5 | 36.2 | 0 | 1.5 | 10.8 |
| 21 | 3.6 | 6 | 19.6 | 0 | 1 | 6.9 |
| 22 | 2.8 | 10 | 13.2 | 0 | 1 | 5.9 |
| 23 | 4.2 | 9 | 17 | 0 | 2 | 5.9 |

Median per 60-min window by UTC hour:

```
       00:00 | #########                                          7
       01:00 | ########                                           6
       02:00 | ########                                           6
       03:00 | ################                                   12
       04:00 | ########                                           6
       05:00 | #############################                      22
       06:00 | ####################################               27
       07:00 | #########                                          7
       08:00 | #################                                  13
       09:00 | ######################################             29
       10:00 | #############                                      10
       11:00 | ################                                   12
       12:00 | ###########                                        8
       13:00 | #########                                          7
       14:00 | ###########                                        8.5
       15:00 | ################################################## 38
       16:00 | #################################                  25
       17:00 | ######################                             17
       18:00 | ###########################################        33
       19:00 | #####                                              4
       20:00 | #######                                            5
       21:00 | ########                                           6
       22:00 | #############                                      10
       23:00 | ############                                       9
```

By day of week (60-min windows): Mon median 6 / p10 2, Tue median 10.5 / p10 0.3, Wed median 27.5 / p10 8.5, Sun median 10 / p10 4.8

Uniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:

| D (min) | min | p1 | p10 | median | p90 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 1 |
| 5 | 0 | 0 | 0 | 0 | 3 |
| 10 | 0 | 0 | 0 | 1 | 6 |
| 15 | 0 | 0 | 0 | 1 | 9 |
| 30 | 0 | 0 | 0 | 3 | 19 |
| 45 | 0 | 0 | 0 | 5 | 28 |
| 60 | 0 | 0 | 0 | 7 | 37 |
| 90 | 0 | 0 | 0 | 11 | 49 |
| 120 | 0 | 0 | 0 | 16 | 62 |
| 180 | 0 | 0 | 4 | 26 | 94 |
| 240 | 0 | 1 | 5 | 39 | 141 |
| 360 | 6 | 6 | 22 | 59 | 210 |
| 480 | 8 | 10 | 45 | 77 | 447.2 |
| 720 | 22 | 24 | 65 | 152 | 743 |
| 1080 | 80 | 85 | 103 | 473 | 855 |
| 1440 | 143 | 147 | 170 | 834 | 1012 |

Fixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:

| epoch (min) | min | p1 | p10 | median |
|---:|---:|---:|---:|---:|
| 5 | 0 | 0 | 0 | 1 |
| 10 | 0 | 0 | 0 | 1 |
| 15 | 0 | 0 | 0 | 2 |
| 30 | 0 | 0 | 0 | 5 |
| 60 | 0 | 0 | 2 | 10 |
| 120 | 0 | 0.7 | 5.2 | 22 |
| 180 | 2 | 2.4 | 9.4 | 31 |
| 240 | 4 | 5.1 | 11.6 | 56 |
| 360 | 6 | 9.3 | 39 | 63 |
| 720 | 65 | 66.1 | 76.2 | 156 |
| 1440 | 249 | 255.1 | 309.6 | 552 |

Smallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):

| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |
|---:|---:|---:|---:|---|
| 50 | 290 | 515 | 845 | (19, 860) |
| 200 | 775 | None | None | (6, None) |

#### Conservative: structural fingerprint excluding blocks with >= 16 such txs (1541 of 1593 txs kept)

Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); *tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.

| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 0 | 0.1 | 0 | 94.2% | 1 | 3 | 9 | 1 |
| 1 min | 0 | 0 | 1 | 0.4 | 0 | 83.9% | 1 | 3 | 10 | 1 |
| 5 min | 0 | 1 | 4 | 1.8 | 0 | 48.9% | 1 | 6 | 26 | 1 |
| 15 min | 0 | 2 | 10 | 5.4 | 0 | 26.5% | 3 | 15 | 72.3 | 1 |
| 60 min | 2 | 10 | 35 | 21.6 | 0 | 4.2% | 10 | 40 | 259 | 1 |
| 24 h | 288.4 | 526 | 763.6 | 526.0 | 229 | 0.0% | 218 | 873 | 980 | 147 |

By UTC hour of day (per 60-min / per 15-min tumbling window):

| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |
|---:|---:|---:|---:|---:|---:|---:|
| 00 | 3.8 | 7 | 10.2 | 0 | 1 | 4.9 |
| 01 | 2 | 6 | 6.8 | 0 | 1 | 2.9 |
| 02 | 1.2 | 6 | 14.8 | 0 | 1 | 5.8 |
| 03 | 2.4 | 12 | 20.8 | 0 | 3 | 5.9 |
| 04 | 1.2 | 6 | 25.2 | 0 | 1 | 8.7 |
| 05 | 6 | 22 | 26.8 | 0 | 3 | 8 |
| 06 | 7.8 | 27 | 31 | 0 | 2.5 | 11.6 |
| 07 | 3 | 7 | 23 | 0 | 2 | 7 |
| 08 | 3.4 | 13 | 29 | 0 | 2 | 10.8 |
| 09 | 17 | 29 | 33.8 | 1 | 5 | 12.7 |
| 10 | 6 | 10 | 50 | 0.1 | 3 | 16.3 |
| 11 | 4.8 | 12 | 24.8 | 1 | 2.5 | 8.6 |
| 12 | 3.2 | 8 | 20 | 0 | 2 | 5.9 |
| 13 | 6.2 | 7 | 11 | 0 | 2 | 3.9 |
| 14 | 6.5 | 8.5 | 10.5 | 1 | 2 | 4 |
| 15 | 10.8 | 38 | 198 | 1 | 9.5 | 67.4 |
| 16 | 10.6 | 25 | 214.6 | 0.3 | 6.5 | 65.8 |
| 17 | 9.8 | 17 | 80.2 | 1.1 | 3.5 | 14.5 |
| 18 | 14.6 | 33 | 39.4 | 0.1 | 5 | 14.9 |
| 19 | 2.4 | 4 | 8 | 0 | 1 | 2.9 |
| 20 | 4.2 | 5 | 36.2 | 0 | 1.5 | 10.8 |
| 21 | 3.6 | 6 | 19.6 | 0 | 1 | 6.9 |
| 22 | 2.8 | 10 | 13.2 | 0 | 1 | 5.9 |
| 23 | 4.2 | 9 | 17 | 0 | 2 | 5.9 |

Median per 60-min window by UTC hour:

```
       00:00 | #########                                          7
       01:00 | ########                                           6
       02:00 | ########                                           6
       03:00 | ################                                   12
       04:00 | ########                                           6
       05:00 | #############################                      22
       06:00 | ####################################               27
       07:00 | #########                                          7
       08:00 | #################                                  13
       09:00 | ######################################             29
       10:00 | #############                                      10
       11:00 | ################                                   12
       12:00 | ###########                                        8
       13:00 | #########                                          7
       14:00 | ###########                                        8.5
       15:00 | ################################################## 38
       16:00 | #################################                  25
       17:00 | ######################                             17
       18:00 | ###########################################        33
       19:00 | #####                                              4
       20:00 | #######                                            5
       21:00 | ########                                           6
       22:00 | #############                                      10
       23:00 | ############                                       9
```

By day of week (60-min windows): Mon median 6 / p10 2, Tue median 10.5 / p10 0.3, Wed median 27.5 / p10 8.5, Sun median 10 / p10 4.8

Uniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:

| D (min) | min | p1 | p10 | median | p90 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 1 |
| 5 | 0 | 0 | 0 | 0 | 3 |
| 10 | 0 | 0 | 0 | 1 | 6 |
| 15 | 0 | 0 | 0 | 1 | 9 |
| 30 | 0 | 0 | 0 | 3 | 18 |
| 45 | 0 | 0 | 0 | 5 | 28 |
| 60 | 0 | 0 | 0 | 7 | 35 |
| 90 | 0 | 0 | 0 | 11 | 47 |
| 120 | 0 | 0 | 0 | 15 | 62 |
| 180 | 0 | 0 | 4 | 26 | 94 |
| 240 | 0 | 1 | 5 | 39 | 141 |
| 360 | 6 | 6 | 22 | 58 | 210 |
| 480 | 8 | 10 | 45 | 71 | 430 |
| 720 | 22 | 24 | 65 | 136 | 711 |
| 1080 | 80 | 85 | 103 | 471.5 | 823 |
| 1440 | 143 | 147 | 156 | 802 | 980 |

Fixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:

| epoch (min) | min | p1 | p10 | median |
|---:|---:|---:|---:|---:|
| 5 | 0 | 0 | 0 | 1 |
| 10 | 0 | 0 | 0 | 1 |
| 15 | 0 | 0 | 0 | 2 |
| 30 | 0 | 0 | 0 | 5 |
| 60 | 0 | 0 | 2 | 10 |
| 120 | 0 | 0.7 | 5.2 | 22 |
| 180 | 2 | 2.4 | 9.4 | 31 |
| 240 | 4 | 5.1 | 11.6 | 54 |
| 360 | 6 | 9.3 | 39 | 63 |
| 720 | 65 | 66.1 | 76.2 | 136 |
| 1440 | 229 | 234.9 | 288.4 | 526 |

Smallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):

| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |
|---:|---:|---:|---:|---|
| 50 | 300 | 515 | 845 | (19, 860) |
| 200 | 780 | None | None | (4, None) |

#### Full-tx fingerprint (estimated): structural + fee 10,000 + lock_time 0 + expiry delta 30-40, each tx weighted by p(bucket)

Anonymity set per window. *Tumbling* = UTC-aligned windows (all windows, empty ones included); *tx-centric* = window of that length centred on each structural-fingerprint tx, counting itself.

| window | tumbling p10 | median | p90 | mean | min | P(<1) | tx-centric p10 | median | p90 | min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| same block | 0 | 0 | 0 | 0.1 | 0 | 97.9% | 1 | 2.8 | 9.4 | 1 |
| 1 min | 0 | 0 | 0.9 | 0.3 | 0 | 94.1% | 1 | 2.8 | 9.4 | 1 |
| 5 min | 0 | 0.9 | 3.7 | 1.7 | 0 | 72.5% | 1 | 6.6 | 24.2 | 1 |
| 15 min | 0 | 1.8 | 9.2 | 5.1 | 0 | 44.3% | 2.8 | 15.0 | 68.1 | 1 |
| 60 min | 1.8 | 9.2 | 34.9 | 20.5 | 0 | 8.5% | 10.2 | 38.2 | 254.1 | 1 |
| 24 h | 283.2 | 508.4 | 733.6 | 508.4 | 226.9 | 0.0% | 216.4 | 836.0 | 933.4 | 135.8 |

By UTC hour of day (per 60-min / per 15-min tumbling window):

| UTC hour | 60m p10 | 60m median | 60m p90 | 15m p10 | 15m median | 15m p90 |
|---:|---:|---:|---:|---:|---:|---:|
| 00 | 3.5 | 6.4 | 9.4 | 0 | 0.9 | 4.5 |
| 01 | 1.8 | 5.5 | 6.3 | 0 | 0.9 | 2.7 |
| 02 | 1.1 | 5.5 | 13.6 | 0 | 0.9 | 5.3 |
| 03 | 2.2 | 11.0 | 19.1 | 0 | 2.8 | 5.4 |
| 04 | 1.1 | 5.5 | 23.2 | 0 | 0.9 | 8.0 |
| 05 | 5.6 | 20.4 | 24.7 | 0 | 2.8 | 7.4 |
| 06 | 7.2 | 25.1 | 28.5 | 0 | 2.3 | 10.7 |
| 07 | 2.8 | 6.4 | 21.2 | 0 | 1.8 | 6.4 |
| 08 | 3.1 | 12.0 | 26.7 | 0 | 1.8 | 9.9 |
| 09 | 15.7 | 26.8 | 43.4 | 1.0 | 5.5 | 15.5 |
| 10 | 5.5 | 9.2 | 46.3 | 0.1 | 2.8 | 15.1 |
| 11 | 4.4 | 11.0 | 22.9 | 0.9 | 2.3 | 7.9 |
| 12 | 2.9 | 7.4 | 18.4 | 0 | 1.8 | 5.4 |
| 13 | 5.7 | 6.4 | 10.1 | 0 | 1.8 | 3.6 |
| 14 | 6.0 | 7.8 | 9.7 | 0.9 | 1.8 | 3.7 |
| 15 | 9.9 | 34.9 | 194.5 | 0.9 | 8.7 | 62.9 |
| 16 | 9.7 | 23.0 | 210.0 | 0.3 | 6.0 | 62.4 |
| 17 | 9.0 | 15.6 | 74.6 | 1.0 | 3.2 | 13.3 |
| 18 | 13.5 | 30.6 | 36.7 | 0.1 | 4.6 | 13.9 |
| 19 | 2.2 | 3.7 | 7.4 | 0 | 0.9 | 2.7 |
| 20 | 3.9 | 4.6 | 33.6 | 0 | 1.4 | 9.9 |
| 21 | 3.3 | 5.5 | 18.0 | 0 | 0.9 | 6.3 |
| 22 | 2.6 | 9.2 | 12.1 | 0 | 0.9 | 5.4 |
| 23 | 3.9 | 8.3 | 15.6 | 0 | 1.8 | 5.4 |

Median per 60-min window by UTC hour:

```
       00:00 | #########                                          6.4
       01:00 | ########                                           5.5
       02:00 | ########                                           5.5
       03:00 | ################                                   11.0
       04:00 | ########                                           5.5
       05:00 | #############################                      20.4
       06:00 | ####################################               25.1
       07:00 | #########                                          6.4
       08:00 | #################                                  12.0
       09:00 | ######################################             26.8
       10:00 | #############                                      9.2
       11:00 | ################                                   11.0
       12:00 | ###########                                        7.4
       13:00 | #########                                          6.4
       14:00 | ###########                                        7.8
       15:00 | ################################################## 34.9
       16:00 | #################################                  23.0
       17:00 | ######################                             15.6
       18:00 | ############################################       30.6
       19:00 | #####                                              3.7
       20:00 | #######                                            4.6
       21:00 | ########                                           5.5
       22:00 | #############                                      9.2
       23:00 | ############                                       8.3
```

By day of week (60-min windows): Mon median 5.5 / p10 1.8, Tue median 9.7 / p10 0.3, Wed median 25.3 / p10 7.8, Sun median 9.2 / p10 4.4

Uniform random delay U[0,D] after an observable trigger; candidates = fingerprint txs mined in [t, t+D], t = every block time:

| D (min) | min | p1 | p10 | median | p90 |
|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 0.9 |
| 5 | 0 | 0 | 0 | 0 | 2.8 |
| 10 | 0 | 0 | 0 | 0.9 | 5.5 |
| 15 | 0 | 0 | 0 | 0.9 | 8.3 |
| 30 | 0 | 0 | 0 | 2.8 | 16.8 |
| 45 | 0 | 0 | 0 | 4.6 | 25.7 |
| 60 | 0 | 0 | 0 | 6.4 | 34.0 |
| 90 | 0 | 0 | 0 | 10.1 | 45.3 |
| 120 | 0 | 0 | 0 | 14.7 | 57.0 |
| 180 | 0 | 0 | 3.7 | 23.9 | 86.4 |
| 240 | 0 | 0.9 | 4.6 | 35.9 | 130.4 |
| 360 | 5.5 | 5.5 | 20.2 | 54.3 | 193.8 |
| 480 | 7.4 | 9.2 | 41.4 | 71.3 | 411.1 |
| 720 | 20.2 | 22.1 | 59.8 | 137.2 | 686.5 |
| 1080 | 73.6 | 78.2 | 95.2 | 436.4 | 789.9 |
| 1440 | 132.0 | 135.7 | 153.8 | 770.6 | 934.3 |

Fixed batching epochs (UTC-aligned; release at epoch end); set = fingerprint txs in the epoch:

| epoch (min) | min | p1 | p10 | median |
|---:|---:|---:|---:|---:|
| 5 | 0 | 0 | 0 | 0.9 |
| 10 | 0 | 0 | 0 | 0.9 |
| 15 | 0 | 0 | 0 | 1.8 |
| 30 | 0 | 0 | 0 | 4.6 |
| 60 | 0 | 0 | 1.8 | 9.2 |
| 120 | 0 | 0.6 | 4.8 | 20.3 |
| 180 | 1.8 | 2.2 | 8.6 | 28.5 |
| 240 | 3.7 | 4.7 | 10.7 | 51.5 |
| 360 | 5.5 | 8.6 | 35.9 | 58.2 |
| 720 | 59.8 | 60.9 | 70.3 | 140.9 |
| 1440 | 226.9 | 232.6 | 283.2 | 508.4 |

Smallest uniform-delay window D (minutes) such that the given quantile of the candidate count reaches k (None = not reached within 24 h):

| k | median | p10 | p1 | p10 in worst UTC hour-of-day (hour, D) |
|---:|---:|---:|---:|---|
| 50 | 320 | 535 | 870 | (19, 880) |
| 200 | 780 | None | None | (4, None) |

Daily structural-fingerprint txs over the whole scan:

```
  2026-10-11 | ################                                   270
  2026-10-12 | ###############                                    249
  2026-10-13 | ################################################## 855
  2026-10-14 | #####################                              359
```
