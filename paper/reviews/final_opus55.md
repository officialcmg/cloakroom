# Final review round, reviewer 1: Opus 5.5 (high), 2026-10-18 — weak accept (conditional on C1)

Rule for this round: fix only critical / fund-level findings, then freeze.

## Critical
- **C1. Taker never checks W; a maker-chosen W (and t2) can hold the taker's ZEC hostage; Theorem 1 does not state the parameter conditions.**
  TRIAGE: valid in the paper, half valid in the code. The code already rejected W < 2F + D + margin + 1, which with the
  enforced margin >= RD + (R-1)F is exactly W > F + B_R, but no rule in Table 2 said so and the text credited the maker.
  The code did NOT bound t2: a quote with t2 ~136 years ahead was accepted (fund-level).
  FIXED: rule R1q (taker checks W > F + B_R with her own D, F, R, and t2 - open <= H, default 48 h); Theorem 1 takes R1q
  as a hypothesis and bounds the hostage time by open + H + B_R + W + F. Scenarios s16 (short W) and s17 (far t2).

## Major (not fund-level; recorded, not fixed under the freeze rule unless decided otherwise)
- M1. Proposition 1's impossibility claim is too general (timed-commitment counterexample on the Zcash side).
- M2. TLC points all have D = F, so they cannot distinguish (R+1)D + RF from RD + (R+1)F.
- M3. "Removing R2c yields an attack" has no R2c mutation; R3l mapping unclear.
- M4. Theorem 3 (liveness) ignores reorgs of setReady and the probabilistic Δ_fund.
- M5. Headline 1.4% is median, per transaction, structural class, sampling guesser; per-swap full-settings is ~8% median.
- M6. Funding is anchored on both sides (open and setReady); Table 8 is one-sided.
- M7. Theorem 4's shape omits the anchor depth, which is public.
- M8. Abstract's testnet sentence goes beyond Sec. 9 (one swap, pre-fix wallet; R <= 1 at those parameters).
- M9. The maker holds a free option until t0 and keeps the premium.
- M10. Self-relaying needs ETH on the relayer key, which can link the taker on the EVM side.

## Minor
m1 refund checks on every Table 1 row; m2 Table 4 caption vs the t2 mutation; m3 count of TLC points;
m4 the extra D in L_M; m5 sweep-time bounds are percentiles; m6 R2r trigger should be inclusion, not broadcast;
m7 setReady delay not in the t0 budget; m8 "up to six" vs five transactions; m9 "audited" vs "no external audit";
m10 RPC-provider and ivk-compromise leaks; m11 "final before t2" vs Lemma 2 wording.
