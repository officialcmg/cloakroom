# Cloakroom: security model and proofs (draft for the paper)

This is the working draft of the formal section. It is checked against the TLA+ model
(`formal/`) and the contract verification (`contracts/audit/`). It describes the final design:
claims need Ready; claims are committed then revealed; payments are pulled; and there is a
punish window t2.

## 1. Model

**Parties and corruption.** There is a taker T and a maker M. A PPT adversary 𝒜 statically
corrupts at most one of them, controls message and transaction scheduling within the bounds below,
and observes everything public. An honest party may also crash and recover. Its state is persistent.

**Time.** Real time is τ. EVM block timestamps satisfy |ts − τ| ≤ σ.

**EVM ledger (Base).**
- **E1, bounded inclusion.** A transaction an honest party broadcasts at τ is included in a block with
  timestamp ≤ τ + D, unless its preconditions fail at inclusion, in which case it reverts.
- **E2, finality.** At time τ, every block with timestamp ≤ τ − F is final.
- **E3, calldata is public.** The adversary learns a transaction's calldata when it is broadcast. This
  holds even if the transaction later reverts or is reorganized away.
- Block timestamps are monotone.

**Zcash ledger.**
- **Z1, persistence.** Except with probability ε_Z, a transaction with k_Z confirmations is never
  reverted.
- **Z2, liveness.** An honest party's valid transaction is mined within Δ_Z.

**Cryptographic assumptions.**
- DL on Pallas.
- RedPallas spend authorization is unforgeable under rerandomized keys. This is the assumption
  Orchard/Ironwood consensus already rests on, together with Halo 2 soundness: no one can spend a
  note without a valid signature under rk = ak + [α]𝒢.
- The Schnorr proofs of knowledge, Fiat–Shamir in the ROM, are knowledge-sound with rewinding
  extraction. Each proof is bound to (sid, role).
- keccak256 and BLAKE2b are modeled as random oracles. This covers the commitment c = H(id, a) (hiding
  and binding) and the KDF.
- ECDSA (secp256k1) is EUF-CMA. This is the payout-key authorization.
- ChaCha20-Poly1305 is IND-CCA. Gap-DH holds on Pallas for the message KDF.

**Token assumption.** Token transfers into and out of the escrow eventually succeed. An issuer
blocking a credited owner forever is outside the protocol's control, and is listed as an issuer
risk.

## 2. Protocol rules (normative)

Windows: open < t0 < t1 < t2. The reveal window is W, measured from lockedAt.

| Rule | Party | Statement |
|---|---|---|
| R1 | T | Fund only after open() is final (E2) and every stored parameter equals the quote. |
| R1b | T | Fund only if τ + Δ_Z·k_Z + D + σ < t0, so M can confirm the note and set Ready in time. |
| R2c | T | Commit while Ready and τ < t1 (a late commit reverts and reveals only c). Commit after t2 if the state is still Open or Ready (punish path). |
| R2r | T | Reveal a only after Locked is final and τ + D + F + margin < lockedAt + W (the reveal must be **final**, not just included, before the deadline). |
| R3e | M | Early refund only if Open (not Ready) and τ + D + σ < t0. |
| R3d | M | Deadline refund only if a final block with ts ≥ t1 shows Open or Ready, and τ + D + F + margin < t2 (the refund must be **final** before t2; found by TLC, mutation mut_refundTightT2). |
| R3l | M | Locked refund only if a final block with ts ≥ lockedAt + W shows Locked. |
| R4 | both | Shares, seeds and DH keys are fresh per RFQ and erased at termination. |
| L_M | M | **Maker liveness.** Online at least once during [t1 + F, t2 − D − σ]. |

Parameter conditions. These were model-checked by TLC (formal/README.md). The model is parametric
in D, F, k_Z and a reorg budget R; at the checked point D = F = k_Z = R = 1, all safety properties
hold:
- MarginC ≥ D + R·F. This is the taker's commit and reveal margin.
- W ≥ F + MarginC + 1. This is the reveal window.
- t0 ≥ F + k_Z + D + 1, and t0 − F ≥ MarginC + 1.
- t1 ≥ t0 + F + MarginC.
- **t2 − t1 ≥ 2D + 2F + MarginR + 1. This one is necessary, not just sufficient.** The maker cannot
  refund before a final block past t1 exists (that takes D + F), and the refund must itself be final
  before t2 (another D + F). With a smaller gap, TLC finds an attack on an honest, online maker:
  1. The maker's refund is included but not yet final at t2.
  2. The refund is reorganized away.
  3. A taker who never funded commits through the punish path and takes the tokens.

## 3. Lemmas

**Lemma 1 (key isolation).** Suppose the corrupted party's public share X_c is accepted with a valid
PoK bound to (sid, role), and the honest share x_h is never revealed. Then the adversary produces a
valid spend authorization for any note at the joint address with only negligible probability.

*Proof.* Rewind the PoK to extract x_c, so ak = [x_h + x_c]𝒢 with x_c known to the reduction. Take any
valid signature under rk = ak + [α]𝒢 for a sighash of the adversary's choice. It is a valid
signature under the key [x_h]𝒢 rerandomized by the known offset x_c + α, because RedPallas
verification depends only on the public key and rk is a rerandomization of it. That gives a forgery
under the honest key [x_h]𝒢, contradicting unforgeability. The sign normalization replaces (x_h, x_c)
by (−x_h, −x_c) together, which preserves the argument. Without the PoK, X_c = [x']𝒢 − X_h would make
ak = [x']𝒢 known to the adversary; this is the rogue-key attack, and it is exactly what the PoK
excludes. ∎

**Lemma 2 (honest revelation succeeds).**
(a) If T reveals under R2r, claim(a) is included and succeeds.
(b) If M refunds under R3d or R3l, refund(b) is included and succeeds.
(c) A reverted honest early refund (R3e) can leak b only while the state is Open and before t0.

*Proof.*
(a) By E2, Locked is final, so no other transition can precede T's claim. By E1 the claim lands before
lockedAt + W. The commitment matches, and claim() makes no external calls, so it cannot revert on
token behaviour.
(b) R3d: the final block with ts ≥ t1 shows no Locked state. Timestamps are monotone and every block
with ts < t1 is final, so no commitClaim with ts < t1 exists. In [t1, t2) commitClaim is disabled.
setReady needs M. By E1 and E2, the refund is included and final before t2 (R3d requires τ + D + F + margin < t2); a refund that is merely included could be reorganized past t2, which is exactly the counterexample TLC finds when this margin is dropped. Hence its precondition holds at inclusion.
R3l: the final block past lockedAt + W shows Locked, and claim is disabled from then on, so the refund
succeeds.
(c) By definition of R3e. ∎

## 4. Theorems

**Theorem 1 (honest-taker safety, no hostage).** Let T be honest, with M possibly corrupted. Except
with probability negl + ε_Z, T ends in exactly one of three outcomes:
- (i) T never funded;
- (ii) T is credited u − f_r;
- (iii) T knows a + b while the joint note is unspent, or T has swept it.

T reaches (ii) or (iii) by time max(t2, lockedAt + W) + 2F + D + σ + Δ_Z with no action by M, so T's
ZEC is never held hostage. No liveness assumption on M is needed.

*Proof sketch.*
- By Lemma 1, only a party knowing a + b spends the joint note. M learns a only from T's claim, which
  succeeds by Lemma 2(a), giving (ii).
- If M refunds, b is public and T sweeps, giving (iii).
- If neither happens by t2, then R2c lets T commit. After t2, commitClaim from Open or Ready cannot be
  blocked, because refunds from Open or Ready are disabled at or after t2. The commit leads to (ii) by
  Lemma 2(a).
- If T committed before t1 and the state is Locked, T reveals within W.
- A reorganized commit or reveal is retried until it is final (E2). ∎

**Theorem 2 (honest-maker safety).** Let M be honest and satisfy L_M, with T possibly corrupted.
Except with probability negl + ε_Z, M ends in one of two outcomes:
- (i) M is credited u (refunded);
- (ii) T is credited, and M controls a final joint note of value ≥ v + f.

*Proof sketch.*
- T is credited only via claim, which needs Locked. Locked arises in only two ways:
  - (Ready ∧ τ < t1). M sets Ready only after Z1 confirmation of a note of value ≥ v + f.
  - (Open or Ready ∧ τ ≥ t2). Under L_M, M's R3d refund is final before t2 and succeeds by Lemma 2(b), so
    the state is Refunded and the second case cannot occur. If the state is Locked before t1, R3l
    covers a missing reveal.
- When claim(a) succeeds, M knows a + b. T does not know b: honest M reveals b only under R3d or R3l,
  which succeed, or R3e. By Lemma 2(c), an R3e leak happens only in Open before t0. After it, M never
  sets Ready, and R3d refunds before t2, so T is never credited. By Lemma 1, no one else spends the
  note, and M sweeps it, giving (ii). ∎

**Proposition 3 (necessity of L_M).** Without L_M, there is an attack on the maker:
1. A taker who never funds waits until t2.
2. She commits from Open and reveals.
3. If M is offline throughout [t1, t2), M loses u.

Any design in this model that gives the taker freedom from hostage, with a scriptless Zcash leg,
needs the maker to act at least once before such a deadline. The same holds for Gugger's punish
window. (TLA+ mutation M-noLM.)

**Theorem 4 (liveness).** If both parties are honest and online, and the parameter conditions hold,
the swap completes by t0 + (t1 − t0) + F + W + D + σ + Δ_Z after open: T is credited, and M sweeps.

**Theorem 5 (unlinkability for a passive observer).** Assume the shielded-transaction
indistinguishability of Ironwood: two fully shielded transactions with the same number of actions are
indistinguishable to an observer who does not hold the keys. Then the public EVM data of a swap is
computationally independent of the swap's Zcash transactions, except through block times. That data
is (A, B, c, one of a or b, payout, relayer, u, f_r, times).

*Proof sketch.* Each Zcash spend reveals rk = ak + [α]𝒢 with fresh uniform α, so rk is uniform and
independent of ak. Note commitments are hiding, and nullifiers are PRF outputs under nk, which is
never public. Hence the Zcash transactions can be simulated without (a, b), and the EVM view is a
function of values independent of them. The observer's best strategy is a timing correlation within
the anonymity set of same-shape transactions, which we measure (Section 8). ∎

**Proposition 6 (forward secrecy of RFQ content).** Consider an adversary who later obtains a
party's Zcash incoming viewing key and its long-term secrets, after the swap ended and the R4 erasure
ran, and after the maker's RFQ epoch key was rotated. That adversary learns nothing about the
contents of RFQ, Quote and Accept beyond their lengths, under Gap-DH and AEAD security. Before the
epoch rotation, RFQ contents are exposed to holders of the epoch key.

## 5. Economics: optionality and griefing

**Free options.**
- T holds an option to complete the trade from Ready until her commit (at most t1 − t_Ready), plus,
  after committing, until her reveal (at most W).
- M holds an option to back out before Ready (at most t0 − t_fund).
- Approximate value: 0.4·σ_ZEC·√T_opt·notional (Han, Lin and Yu, AFT 2019). At σ_ZEC ≈ 100%/year,
  a 2-hour option is worth about 0.6% of notional.

**Mitigation.** The non-refundable ZEC premium paid with Accept prices T's option and M's gas. M's
residual option is limited to the time before Ready and is reputational: an early refund is public on
the EVM chain.

**Griefing cost to M per abandoned RFQ.** The quote transaction fee, the open and refund gas, and
capital locked until t1 (or t0 for an unfunded swap). The premium covers this when set above
gas + rate·u·(t1 − open).
