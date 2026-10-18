# Final review round, reviewer 4: Fable 5.1, 2026-10-18 — accept (no critical findings)

Reviewed the PDF after the Astra/Sol fixes. "No path by which an honest party loses funds or is held hostage under
the stated assumptions; every timing bound checks out arithmetically."

- **M1.** "Removing any of R2c-R3l yields an attack" is unsupported for R2c (= Opus M3) and contradicted by the
  implementation note. FIXED: the claim names only the mutated rules (R2r, R3e, R3d, L_M); R2c and R3l stated as not
  mutated separately; the conclusion says "each of the rules it mutates".
- **M2.** 1.4% is for structural look-alikes. FIXED in the abstract. MAP and full-settings columns: FUTURE.md.
- **M3.** t0 - open budgets three draws of the taker's delay but not the maker's delay before setReady, k_Z or the
  Zcash expiry/re-send. Not fund-level (a missed Ready ends in a refund). Recorded in FUTURE.md.
- Minor, FIXED: theta = 15.9 h; "the taker is anonymous" qualified for a self-funded relayer; directory signature
  contents listed; testnet withdrawal wording.
- Minor, REJECTED after checking: "L_M's upper end conflicts with R3d's strict inequality" -- L_M's window is
  half-open, [t1 + D + F, t2 - B_R).
- Other minor items (Lemma 3(b) wording, L_M and Locked-state refunds, Delta_fund vs the 40-block expiry, the discrete
  budget's -R, the redraw cap): FUTURE.md.
