# Formal verification of the Cloakroom v2 atomic swap (TLA+ / TLC)

This directory contains a TLA+ model of the Cloakroom **v2 + t2 "punish window"**
cross-chain atomic swap (shielded ZEC ↔ ERC-20), together with TLC configurations
that check its safety and liveness properties exhaustively, and a suite of
*mutations* that deliberately break one protective rule at a time to show that
TLC catches the resulting attack.

- `Cloakroom.tla` — the model (commented).
- `cfg_*.cfg` — the three main configurations + the "without-assumption" run.
- `mut_*.cfg` — mutation configurations (Task 4).
- `live_*.cfg` — liveness configurations (Task 3).
- `tla2tools.jar` — TLC, v1.7.4 (official `tlaplus/tlaplus` release).

Reproduce (OpenJDK installed via Homebrew):

```sh
J=$(brew --prefix openjdk)/bin/java
$J -XX:+UseParallelGC -cp tla2tools.jar tlc2.TLC -deadlock -config cfg_a_honest.cfg -workers auto Cloakroom.tla
```

`-deadlock` disables TLC's deadlock check: a completed/aborted swap is a genuine
terminal state (`FullyDone`), not a deadlock.

---

## 1. What is modeled

The model is grounded in the real code
(`contracts/src/CloakroomEscrow.sol`, `node/src/{maker,taker}.rs`), which was
updated to the t2 design. The escrow guards modeled are exactly those of
`CloakroomEscrow.sol`:

| op | guard (contract) | signer / knowledge |
|----|------------------|--------------------|
| `setReady` | `Open ∧ now<t0` | maker |
| `commitClaim` | `(Ready ∧ now<t1) ∨ now≥t2`, state∈{Open,Ready} | payout key ⇒ **taker only** |
| `claim(a)` | `Locked ∧ now<lockedAt+W` | anyone who knows `a` |
| `refund(b)` | `(Open ∧ now<t0) ∨ ((Open∨Ready) ∧ t1≤now<t2) ∨ (Locked ∧ now≥lockedAt+W)` | anyone who knows `b` |

Modeled explicitly:

- **Discrete time** `now ∈ 0..MaxTime`.
- **EVM escrow as a block history** (`chain`, a sequence of `[state, appliedAt]`),
  so that **finality** and **reorgs** are first-class: a block is *finalized*
  once `now − appliedAt ≥ F`; a `ReorgEVM` drops any **unfinalized** suffix.
  Honest parties read the **finalized** projection `FinalState`; the adversarial
  scheduler acts on the tip.
- **EVM inclusion delay**: a broadcast tx sits in a mempool and is **included
  within `[bt+1, bt+D]`** (the scheduler picks when, but inclusion by `bt+D` is
  mandatory — `D` is the *max* delay, not an option to drop the tx). At inclusion
  the contract precondition is re-checked; if it fails the tx **reverts**.
- **Secrets leak at broadcast**: `claim` publishes `a` and `refund` publishes `b`
  the moment the tx is *broadcast*, **even if it later reverts or is reorged out**.
  `commitClaim` publishes only a hash (leaks nothing).
- **ZEC joint note**: funded by the taker (`Pending`), reorg-able by the adversary
  until `k_Z` confirmations, then **final** (`PromoteZec` is forced at
  `zfundAt+KZ` — confirmations take bounded time). Whoever knows **both** `a` and
  `b` can sweep the funded note; first sweep wins (`SweptT` / `SweptM`).
- **Adversary**: controls at most one party (`ADV ∈ {none, maker, taker}`), which
  may deviate arbitrarily (never act / act any time / broadcast anything it
  knows), **plus the scheduler** (inclusion delays within `D`, reorgs within `F`
  bounded by `MaxReorg`, and crashes/offline of the honest party).
- **Honest-but-offline**: honest parties are "online" via **urgency** — an online
  honest party is not allowed to let time pass while it has a loss-avoiding step
  to take (modeled by blocking `Tick`). The knobs `AssumeMakerOnline` /
  `AssumeTakerOnline` turn urgency off for a party, modeling it **crashed/offline**
  for that stretch. Each windowed honest action is forced at its *last safe
  moment*, so earlier action is still explored and a too-small margin still lands
  the action too late.

### Abstractions and their justification

| abstraction | justification |
|-------------|---------------|
| Block *finality = age ≥ F* (time-based, not depth-based) | `F` is the finality lag in time steps; honest code already keys off `block_info("finalized")`. Collapses depth and time into one parameter without losing the "unfinalized ⇒ reorg-able" property. |
| `ReorgEVM` drops any unfinalized suffix; total reorgs bounded by `MaxReorg` | A finalized chain suffers only finitely many reorgs; the bound keeps the state space finite and is the reorg *budget* the adversary spends. Timing conditions are stated as functions of this budget. |
| ZEC note = 5-valued (`Unfunded/Pending/Final/SweptT/SweptM`) | The swap only cares about whether the joint note is spendable and by whom; amounts/UTXO structure are irrelevant to the safety questions. |
| `open` is applied immediately (not two-phase) | Its *finality* is still modeled (honest taker funds only after `Open` is finalized); the inclusion delay of `open` itself is not adversarially interesting for the four properties. |
| secrets are a 2-element set `{a,b}` | The ZK/Pallas machinery (`isDiscreteLogOf`, PoK, rogue-key) is out of scope here; we model only the *information* a broadcast leaks. |
| single joint note, single swap instance | The properties are per-swap; ids are maker-scoped and independent. |

---

## 2. Constants and the exact timing conditions

Checked constants (all configs): `D=1, F=1, KZ=1, MaxReorg=1, MarginC=2,
MarginR=1, T0=4, T1=7, T2=13, W=4, MaxTime=22`.

Let `D` = max inclusion delay, `F` = finality lag, `k_Z` = ZEC confirmations,
`R` = reorg budget (`MaxReorg`). The properties hold **iff** the parameters
satisfy (verified sufficient by TLC; each is violated by a mutation below):

1. **Taker claim/commit margin** `MarginC ≥ D + R·F`
   — after `Locked` finalizes, the taker's (retried) `claim` must finalize inside
   the window despite inclusion delay `D` and up to `R` reorgs of her `Claimed`
   block. *(checked: 2 = 1 + 1·1.)*
2. **Reveal window** `W ≥ F + MarginC + 1`
   — `Locked` finalizes `F` after `lockedAt`; she reveals with `MarginC` slack.
   *(checked: 4 > 1 + 2.)*
3. **`t0`** `T0 ≥ F + k_Z + D + 1` and `T0 − F ≥ MarginC + 1`
   — Open finalizes (`F`), taker funds with margin, note confirms (`k_Z`), maker
   `setReady` lands (`D`), all before `t0`. *(checked: 4.)*
4. **`t1`** `T1 ≥ T0 + F + MarginC`
   — `Ready` finalizes after `t0`; taker commits with margin before `t1`.
   *(checked: 7 ≥ 4 + 1 + 2.)*
5. **`t2` (the punish-window spacing — the key condition)**
   **`T2 − T1 ≥ 2D + 2F + MarginR + 1`**
   — the honest maker's deadline refund must (a) wait for a **finalized** block
   past `t1` (`now ≥ t1 + D + F`) so no pre-`t1` commit can still appear, and
   (b) itself **finalize before `t2`** (`now ≤ t2 − D − F − MarginR`), with room
   to re-broadcast if a reorg drops it. *(checked: 13 − 7 = 6 = 2+2+1+1.
   `t2 = 11`, giving gap 4 < 6, is caught — see mutation `refundTightT2`.)*
6. **Horizon** `MaxTime ≥ T2 + 2(D+F) + W` so the latest punish-path claim can
   finalize. *(checked: 22.)*

**This is the headline formal result.** Condition **(5)** is subtle and easy to
get wrong: it couples the refund's *observation* finality and its *inclusion*
finality against the punish deadline. The first `t2` I tried, `t2 = 11`
(`t2 − t1 = 4`), **fails** `HonestMakerSafe` even for an *online* honest maker.

---

## 3. Results

### Safety invariants (Task 2)

- `NoDoubleExit` — a finalized `Claimed` and a finalized `Refunded` never coexist.
- `NoFreeClaim` — `Claimed` ⇒ a final ZEC funding of the joint note existed
  (`zecWasFinal`). *(The t2 punish path intentionally decouples `claim` from a
  preceding `Ready`, so this is "a final funding existed" rather than the stronger
  "…when Ready was set"; the stronger form holds only for the no-t2 design.)*
- `HonestTakerSafe` — at genuine termination (`FullyDone`) an honest taker never
  ends with the maker holding her ZEC and no tokens, with no claim left to make.
- `HonestMakerSafe` — at termination an honest maker never ends with the taker
  holding the tokens while the maker can get neither the tokens nor the ZEC.

Final-accounting invariants are evaluated at `FullyDone` (settled chain + resolved
ZEC + empty mempool); liveness (below) guarantees that terminal is reached. This
split avoids misreading an in-flight honest operation at the artificial horizon as
a loss.

| # | Config | ADV | maker/taker online | result | states (distinct) | depth | time |
|---|--------|-----|--------------------|--------|------:|------:|-----:|
| a | both honest, adversarial scheduling + reorg | none | on / on | **all hold** | 319 | 25 | ~1s |
| b | malicious maker + honest taker | maker | — / on | **all hold** (incl. `HonestTakerSafe`) | 52 423 | 72 | ~2s |
| c | malicious taker + honest maker, **with** maker-online | taker | on / — | **all hold** (incl. `HonestMakerSafe`) | 248 853 | 75 | ~4s |
| c′ | malicious taker + honest maker, **without** maker-online | taker | off / — | `NoFreeClaim` & `HonestMakerSafe` **FAIL** | 43 781† | 20 | ~3s |

†counterexample found mid-search (BFS); the number is states explored so far.

Config **c′** is the *documented* boundary, not a surprise attack: it is exactly
the statement that `HonestMakerSafe` requires the maker to be **online at least
once in `[t1, t2)`**. With the online assumption (config c) it holds; without it,
the malicious taker never funds, waits past `t2`, punish-commits from `Open`, and
claims for free.

### Liveness (Task 3), under weak fairness for online honest parties

- `SwapSettles`: once `Open` is finalized, the swap always settles (`~>` Claimed/Refunded).
- `NoHostage`: once the honest taker's note is funded, she eventually gets the
  tokens or her ZEC is resolved (back to her / recovered / taken in a completed swap).

| Config | property | result |
|--------|----------|--------|
| `live_complete` (both honest) | `SwapSettles` | **holds** |
| `live_hostage_main` (malicious maker, main design) | `NoHostage` | **holds** — the taker uses the **punish path** to force resolution |
| `live_hostage_noT2` (malicious maker, `noT2` mutation) | `NoHostage` | **FAILS** — permanent hostage |

### Mutations (Task 4) — each breaks one protective rule; TLC finds a counterexample

| mutation | rule removed | config | property violated | counterexample (summary) |
|----------|--------------|--------|-------------------|--------------------------|
| `smallMargin` | taker reveal margin (`MarginC→0`) | a (both honest) | `HonestTakerSafe` | Taker reveals `a` at the last step `now=lockedAt+W−1`; inclusion delay `D` makes `claim` land at `lockedAt+W` → **reverts**. `a` is public, maker sweeps the ZEC (`SweptM`), maker refunds after the window → taker has neither leg. |
| `revealBeforeFinal` | "reveal only after Locked is final" | a | `HonestTakerSafe` | Taker reveals `a` while `Locked` is still **unfinalized**; one reorg drops the commit (`→Ready`); `claim` reverts; `a` public; maker sweeps ZEC; taker loses both. |
| `claimFromOpen` (v1) | "claim requires Ready" | c (mal. taker) | `NoFreeClaim` | Malicious taker never funds, commits **from `Open`** (no Ready, no funding), claims → `Claimed` with the joint note never funded → free theft of the maker's tokens. |
| `refundTightT2` | **condition (5)**: `t2` too small (`t2=11`, gap 4<6) — *main guard, online maker* | c (mal. taker) | `HonestMakerSafe` | Honest **online** maker refunds at the earliest finality-gated time `t1+D+F=9`; the refund finalizes only at `11 = t2`; the adversary reorgs the still-unfinalized `Refunded` at `t2`; the taker punish-commits from `Open` at `t2` and claims → free claim. |
| `noFinalityRefund` | refund's finalize-before-`t2` check | c (maker free to act late) | `HonestMakerSafe` | The buggy guard lets the maker refund **late** (near `t2`) with no finality margin; the refund is reorged out and the taker punish-commits at `t2` → same free-claim mechanism. |
| `refundNearT1NoFinal` | wait for a finalized block past `t1` | c (maker free to act late) | `HonestMakerSafe` | As above: a tip-based refund with no finality margin can be reorged, enabling the punish-path free claim. |
| `noT2` | the punish window itself | malicious maker | `NoHostage` (liveness) | Honest taker funds; malicious maker never sets Ready, never refunds, never reveals `b`; with no punish path the taker can neither claim nor recover → **permanent hostage**. |

Note the honest classification of `noT2`: run as a **safety** check
(`mut_noT2_safety`, 68 125 states) `HonestTakerSafe` and `NoDoubleExit` still
**hold** — the hostage is a *liveness* failure (her ZEC is **locked, not stolen**:
`zec = Final`, not `SweptM`), exactly as the spec intends.

An interesting nuance surfaced by the model: under the **online-maker** assumption
the `noFinalityRefund` / `refundNearT1NoFinal` mutations *pass*, because a forced-
early online maker refunds at `t1` and that refund finalizes long before `t2`. The
finality rules only bite when the refund lands **late** — which is why the clean,
assumption-free demonstration of condition (5) is `refundTightT2` (correct guard,
insufficient `t2` spacing).

---

## 4. Does any property FAIL for the real v2 design?

**No — provided the timing conditions of §2 hold and the maker is online once in
`[t1, t2)`.** Under those assumptions all four safety invariants hold in every
adversary configuration, and liveness holds (with the documented hostage carve-out
when a party disappears *and* the design lacks `t2`).

The one actionable finding is condition **(5)**,
`T2 − T1 ≥ 2D + 2F + MarginR + 1`. It is **necessary**, not just sufficient: an
implementation that picks a `t2` only `2D` past `t1` (a natural-looking choice —
"leave the inclusion delay before the punish window") is **defrauded** even with a
fully honest, online maker, via

> refund broadcast → included but **not yet finalized** at `t2` → adversary reorgs
> the `Refunded` block → malicious taker punish-commits from `Open` at `t2` →
> `claim` → tokens taken with **no ZEC ever funded**.

The maker's deadline refund must *finalize* before `t2`, which forces the wider
spacing. The real contract/relayer parameters should be chosen to satisfy §2 with
the deployment's real `D` (inclusion delay), `F` (L2/L1 finality lag) and `k_Z`.

Debugging the model to a passing state also surfaced the honest-policy rules the
real agents already follow and which are *required* for safety (each one, when
removed, produced a concrete counterexample during development):
the taker commits only on a **final** funding; the maker's early (pre-`t0`) refund
is gated to *near `t0`* so it is disjoint from the taker's funding window (else `b`
leaks while she can still fund-and-both-exit); the taker **retries** her claim and
**recovers** her ZEC the moment `b` is public.

---

## 5. Limitations

- **Bounded reorg budget** (`MaxReorg`). Reorgs are capped globally to keep the
  state space finite; condition (1)/(5) are stated as functions of this budget
  `R`. Checked at `R=1`. A deployment tolerating deeper reorgs needs
  correspondingly larger `MarginC` and `t2 − t1`.
- **Small constants.** Verification is for one small parameter point per §2.
  Soundness of the inequalities beyond the checked point is argued, not model-
  checked; a deployment should re-run TLC at its own `D, F, k_Z` (the model is
  parametric).
- **Time = finality.** Finality is abstracted as block age ≥ `F` rather than a
  consensus gadget; adequate for the "unfinalized ⇒ reorg-able within `F`"
  property but not for gadget-specific reorg dynamics.
- **One swap, one note, no value arithmetic.** Fees, amounts, the relayer-fee
  split, and multi-swap interleavings are out of scope; the properties are
  per-swap and the ids are independent.
- **Cryptography is abstracted** to information leakage (`{a,b}`). Pallas
  on-curve / discrete-log / proof-of-knowledge / rogue-key checks are assumed
  correct and are not modeled; a rogue-key or bad-PoK attack would be a *different*
  model.
- **Online = urgency.** "Online honest party" is modeled as acting at the last
  safe moment before a window closes; a real party that is online but merely slow
  within `D`/`F` is covered by the scheduler, but pathological intermittent
  availability finer than the step granularity is not separately modeled.

## Round 2 (2026-10-15): second parameter point D = F = 2, re-sends, late maker

A reviewer found that the budget used for share-revealing transactions, MarginC >= D + R*F,
omits one inclusion delay per reorganization. In this discrete model a block can be reorganized
only while its age is < F steps, so a transaction broadcast at tau survives R reorgs if
tau + D + R(D + F - 1) < deadline. That is D + R*F only when D = 1, which is why the D = F = 1
point could not show the error. The continuous budget (paper, implementation) is
B_R = (R + 1)D + RF.

Model changes:
- `HonestSetReadyGuard` / `MakerSetReadyUrgent`: no Ready after b is public (mutation `readyAfterRefund`).
- `RefundResend`: once b is public, the honest maker re-sends a dropped refund while the escrow can
  still accept it. The budget limits only the first broadcast (mutation `noResend`).
- `MakerOnlineFrom`: the honest maker cannot act before this time (0 = always online). Setting it to
  the latest point L_M allows checks the refund budget directly.

Commands: `./run_d2.sh <config> ...` (one TLC at a time, 2 workers, 2 GB heap; logs in `logs/`).
Results on the final model (`logs/round2_final.txt`):

| Config | D=F | Result | Distinct states |
|---|---|---|---:|
| cfg_a_honest | 1 | all invariants hold | 352 |
| cfg_b_malmaker | 1 | all hold | 52,423 |
| cfg_c_maltaker | 1 | all hold | 327,390 |
| cfg_c_offline (L_M violated) | 1 | NoFreeClaim violated (expected) | — |
| d2_a_honest | 2 | all hold | 2,406 |
| d2_b_malmaker | 2 | all hold | 856,632 |
| d2_c_maltaker | 2 | all hold | 13,467,219 |
| d2_c_lateMaker (maker online only from t2 - D - F - MarginR) | 2 | all hold | 2,124,904 |
| d2_m_oldMarginC (claim budget D + RF, one step short) | 2 | HonestTakerSafe violated | — |
| d2_m_lateMakerShortMargin (refund margin one step short, late maker) | 2 | NoFreeClaim violated | — |
| d2_m_noResend | 2 | NoFreeClaim violated | — |
| d2_m_readyAfterRefund | 2 | HonestMakerSafe violated | — |
| live_complete / live_hostage_main | 1 | SwapSettles / NoHostage hold | — |
| live_hostage_noT2 | 1 | NoHostage violated (stuttering: permanent hostage) | — |
| mut_claimFromOpen, mut_smallMargin, mut_revealBeforeFin, mut_refundTightT2, mut_noFinalityRefund_off, mut_refundNearT1_off | 1 | violated as before | — |

## Round 3 (2026-10-18): R = 2 and liveness at D = F = 2 (logs/round3_tlc.txt)

| Config | Result | Distinct states | Time |
|---|---|---:|---:|
| d2_live_complete (SwapSettles) | holds | 2,406 | 2 s |
| d2_live_hostage (NoHostage, malicious maker) | holds | 856,632 | 48 s |
| r2_a_honest (R=2, D=F=2) | all hold | 9,626 | 1 s |
| r2_b_malmaker | all hold | 3,588,748 | 41 s |
| r2_c_maltaker | all hold | 120,580,544 | 31 min 45 s |
| r2_c_lateMaker (maker online from t2-D-F-MarginR) | all hold | 9,075,177 | 1 min 52 s |
| r2_m_shortC (claim budget 7 < 8) | HonestTakerSafe violated | — | — |
| r2_m_lateShortR (refund margin 4 < 5, late maker) | NoFreeClaim violated | — | — |

R = 2 discrete budgets: MarginC = D + R(D+F-1) = 8; MarginR = R(D+F-1) - F + 1 = 5.
