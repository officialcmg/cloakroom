# Round 2 review: Opus 5.5 (high), 2026-10-14

Verdict: **weak reject.** The full report is summarized below, with my triage after each finding
(checked against the code and the model).

## Critical
- **C1. The relayer can steal (it is not liveness-only).** VALID FOR THE PAPER.
  `claim` is permissionless (contract line 199, no sender check), and the implementation's taker
  holds the relayer key itself. But the paper describes an external relayer that sees `a`.
  Fix:
  - say that `claim` is permissionless;
  - give the taker a fallback submission path;
  - add the hand-off delay to R2r's budget;
  - state when the relayer is liveness-only;
  - say the implementation self-relays.
- **C2. The margins omit the D of each re-send after a reorg.** VALID.
  In the TLA+ model a reorg is possible only while a block's age is < F. With D = F = 1, the
  reorg must happen in the same step as inclusion, which hides the extra re-inclusion cost.
  In continuous time the cost per reorg is D + F. Correct budget: a share-revealing broadcast at τ
  needs τ + (R+1)D + RF < deadline. In final_before form that is margin μ ≥ RD + (R−1)F.
  The implementation is safe for R = 1: the testnet uses μ = 60 s ≥ D = 30 s, the harness
  μ = 5 s ≥ D = 2 s. The paper's general conditions are wrong; recheck with TLC at D = F = 2.

## Major
- **M1. σ is unused; τ vs ts.** PARTLY VALID. The wallet reads time from the chain head
  (`evm.now()`), so its rules compare block time with block time. Fix: state that the rules use the
  head's timestamp, fold node lag into D, and remove σ or use it only for Zcash.
- **M2. The censorship-resistant profile is inconsistent with E2.** VALID. Drop the profile, or
  model a sequencer outage. Plan: drop it and keep forced inclusion as a limitation.
- **M3. Crashes vs "online once".** VALID. Model a party that has broadcast a share-revealing
  transaction as online until that transaction is final (or crash time counts toward D). L_M then
  becomes an interval of length ≥ (R+1)(D+F) + D.
- **M4. Count-gated cover concentrates the adversary's guess.** VALID. Snapping the broadcast to
  the k-th look-alike leaks timing. Fix the policy (resample instead of snapping) and measure the
  adversary's posterior on mainnet data.
- **M5. The indistinguishability assumption is refuted by fee/expiry fingerprints; anonymity sets
  do not compose.** VALID. Define "shape" over all public fields, analyse a swap's linked
  transactions and intersection across a maker's swaps, and soften the abstract.
- **M6. The public runs were v1; no reorg tests; Halmos is bounded.** VALID. The v3 testnet run is
  in progress. Add reorg-injection scenarios (anvil_reorg) and qualify the claims.
- **M7. Necessity is overstated.** VALID. Scope it to the rule R3d and to a model in which the
  escrow cannot verify Zcash state.

## Minor and nits
m1 ZIP 2005 wording; m2 the premium horizon, and a maker can keep premiums and never open;
m3 the Table 7 estimator; m4 Lemma 1 extraction; m5 time bounds; m6 what the identifier is;
m7 boundary vectors near p and q; m8 the relayer sees the taker's IP; m9 Orchard fix in NU6.2 vs
NU6.3 (verify); m10 parameter units. Nits: σ is overloaded; "nk" is overloaded; "thirteen" vs
twelve scenarios; "no successful commit"; R3e is undefined; attribution of the findings; negated
A/B; state k_Z in blocks.
