# Review round 2, reviewer 1: Opus 5.5 (high), 2026-10-18 — weak reject (major revision)

## Critical
- **C1. No-hostage fails if T crashes between commit and reveal.**
  (H) covers only share-publishing transactions. commitClaim publishes only c. If T is offline through W, a malicious M never refunds and T's ZEC is stuck.
  Replay: commit signatures have no validity interval, so a reverted late commit can be replayed at t2+.
  TRIAGE: valid. TLC missed it because AssumeTakerOnline=TRUE means an always-online taker.
  Fix: extend (H) to cover commitClaim through claim finality; restate Theorem 1; explain why TLC missed it.
  Consider binding the commit signature to its path (contract change; deferred unless needed).

## Major
- **M1. Accept and funding are anchored BACKWARD by `open` / `setReady`.**
  TRIAGE: valid, and worse than the reviewer says: setReady comes exactly k_Z blocks after the funding.
  Fix: the maker applies the cover delay before `open` and before `setReady`, with the deadline cap.
- **M2. Table 8 vs Table 7 inconsistent** (uniform 2^H p10 = 45 vs 60-min window p10 = 16).
  TRIAGE: investigate the sim's candidate filter and window; "exact posterior by Monte Carlo" is wrong wording.
- **M3. Anchor depth not in the shape; the funding may need >2 actions; per-swap linking probability not reported.**
  TRIAGE: anchor depth was not measured, so it goes in as a limitation. Enforce single-note funding. Report the per-swap union bound.
- **M4. Cover delay vs the public t0 deadline (truncation).**
  TRIAGE: valid. The taker rejects quotes whose t0 does not cover the p99 delay; describe truncation.
- **M5. Premium harvesting / impostor maker** (unsigned Quote, unsigned directory).
  TRIAGE: valid. The maker signs the directory entry and the Quote with its EVM key.
- **M6. Forward secrecy overclaim.**
  TRIAGE: valid. The funding and message outputs use the sender's ovk. Switch to ovk = ⊥ and restate precisely what ivk and ovk reveal.
- **M7. Claims exceed evidence:**
  - "any rule ... attack" — no mutations for R1, R1b, R2c's t1 budget or R4;
  - the abstract misattributes the budget fix;
  - R=2 never model-checked; liveness only at D=F=1;
  - the testnet run is pre-fix (and uses R=1);
  - the curve-overflow claim.
  TRIAGE: valid. Temper the claims; run TLC with R=2 if tractable and liveness at D=F=2.

## Minor (all to fix in text)
- m1 Δ_fund percentile; ε_Z
- m2 justify R; batcher stall
- m3 option horizon from the Quote; σ of the rate; 3.96 h
- m4 units, unbounded C
- m5 Figure 1 "final before t2"
- m6 discrete ≥
- m7 counts (five vs six transactions; "eleven"; three vs four gaps)
- m8 AthanorLabs relayer
- m9 the 51% cross-reference

Nits: Pointcheval cite unused; AEAD wording; abstract vs π; Monte Carlo wording; define "share of D".
