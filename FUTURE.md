# Future considerations

Things we decided, for the Oct 28 ZECATHON deadline, to leave out of the frozen paper and code.
None of them can lose funds today. Each one says what to do later and why it was left.

## 1. Bring the paper's protocol and the implementation back into line on R2c

- **Now:** the protocol (paper, Table 2, rule R2c) says the taker commits only once Ready is *final*.
  The implementation (`node/src/taker.rs`, `on_funded`) commits as soon as Ready is *visible* at the chain head.
  The paper states this difference in Section 9 and why it is safe: a commit publishes only `c`, a commit on a
  Ready that is later removed reverts, and `a` is revealed only once the lock is final (R2r).
- **Why it was left:** changing the code means re-running the suite and the testnet swaps; it adds no safety and
  makes every swap about F (25 min on Base Sepolia) slower.
- **Later, pick one and update the paper so the protocol, the model and the code say the same thing:**
  - make the taker wait for a final Ready before committing; or
  - change R2c in the paper to "commit once Ready is visible", add a TLA+ variant that checks the relaxed rule,
    and keep the safety argument above as a lemma.
- Found while watching final testnet swap B, which committed 6 s after Ready.

## 2. Non-fund findings from the final review round (Opus 5.5)

Recorded in `paper/reviews/final_opus55.md`. The cheap wording ones (M1, M2/m3, M5, m8, m9) and C1 are done.
Still open:
- **M2:** model-check points with D != F (e.g. D=1, F=2 and D=2, F=1) so TLC can tell (R+1)D + RF from RD + (R+1)F.
- **M3:** give concrete counterexamples for removing R2c and R3l, or drop them from the "each rule is needed" claim.
- **M4:** Theorem 3 (liveness) ignores a reorganized `setReady` and treats the funding delay as a hard bound.
- **M5/M6:** report per-swap linking probability with the best-guess observer, for the full-settings class, and
  simulate the two-sided anchor on the funding (between `open` and `setReady`).
- **M7:** put the anchor depth into the transaction shape, or state Theorem 4 under an explicit assumption, and measure it.
- **M9:** the maker holds a free option until t0 and keeps the premium; price it or make the premium partly refundable.
- **M10:** self-relaying needs ETH on the relayer key, which can link the taker on the EVM side; say how to fund it.
- **Minor m1, m2, m4-m7, m10, m11:** see the review file.

## 3. Testnet notes

- The quote parameters for testnet need `t0 - open` well above `F + Δ_fund`: testnet once took 17 blocks to mine a
  funding. The maker config now uses 80 min (`t0_secs = 4800`).
- 0.0101 TAZ is stranded in the joint note of final swap A (`a` was erased by the bug since fixed). It is
  testnet money and cannot be recovered.
- Cover delays were off in the testnet runs; a run with the real cover profile would show the privacy behaviour end to end.
