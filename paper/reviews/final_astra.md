# Final review round, reviewer 2: GPT-6 Astra (high), via Codex CLI, 2026-10-18 — reject

- **C1.** Theorem 1's bound t2 + B_R + W + F needs the taker online at t2; (H) only covers her from the commit.
  TRIAGE: valid as a false theorem statement (no fund loss: refunds are disabled from t2, so she commits on return).
  FIXED: bound measured from tau_T, the first time from t2 on that T is online; stated that her ZEC is not lost meanwhile.
- **M1.** The abstract's "at most about 6% per swap" applies a union bound to medians. TRIAGE: valid. FIXED: removed;
  the abstract now says the per-swap probability is higher and is not bounded.
- **M2.** Posteriors are not independent: the funding is anchored by open and setReady. TRIAGE: valid (= Opus M6).
  FIXED: independence and union-bound sentences replaced; joint posterior stated as not measured. Measurement in FUTURE.md.
- **M3.** Theorem 3 fails if setReady is reorganized just before t0. TRIAGE: valid (= Opus M4). FIXED: Theorem 3 now
  ends "with the claim or the refund", and with the claim if setReady is included before t0 and not removed.
- **N1.** "final before t2" overstates Lemma 2. TRIAGE: valid (= Opus m11). FIXED.
