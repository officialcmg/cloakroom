# Slither triage: CloakroomEscrow v3 (final, with the L-01 and I-01 fixes)

- Tool: slither-analyzer 0.11.6 (PyPI, in `contracts/.venv`), 102 detectors, all enabled.
- Compiler: solc 0.8.30 run through forge 1.2.3 (via_ir, optimizer 200, evm cancun).
- Target: `src/CloakroomEscrow.sol`, `src/Pallas.sol`, `src/MockUSDC.sol` (`lib/` filtered out).
- Command: `slither . --filter-paths "lib/" --json audit/logs/slither.json`. Run it on a copy of `src/` + `foundry.toml`, because Slither calls `forge clean`, which would wipe `out/` and `cache/` while other runs are in flight.
- Raw output: `audit/logs/slither.txt`, `audit/logs/slither.json`.
- Result (rerun on the final source, 2026-10-14): 16 findings from 5 detectors. Slither ranks one as High impact (`reentrancy-balance`, #8); after triage it is not exploitable. The rest are Low or Informational. Each one is triaged below. Compared with the v2 run, `reentrancy-balance` is new (it is caused by the L-01 balance-delta check), and `missing-inheritance` on `MockUSDC` no longer fires.

| # | Detector (impact / confidence) | Location | Verdict | Reason |
|---|---|---|---|---|
| 1 | reentrancy-events (Low / Medium) | `open()`: `Opened` emitted after `token.transferFrom` | **True positive, informational impact** | All storage effects of `open()` are written before the external call. Halmos proves this (`check_open_CEI`; mutant M18 is killed). A malicious token can re-enter during `transferFrom`. For example, it can `refund(id, b)` the swap that is still being opened (`test_reentrancy_open_refundDuringPull`). In that case `Refunded(id)` is logged **before** `Opened(id)`. Accounting stays exact. Only swaps of that same token are affected, and the token's own deployer could do the same without re-entrancy. Off-chain indexers must not assume that `Opened` always comes first in the log for a given id. |
| 2 | reentrancy-events (Low / Medium) | `withdraw()`: `Withdrawn` emitted after `token.transfer` | **True positive, informational impact** | The credit is zeroed before the transfer (`check_withdraw_CEI_and_noDoublePay`; mutant M13 is killed). A nested `withdraw` sees credit 0 and pays nothing (`test_reentrancy_withdraw_noDoublePay`). The only effect is log order: a nested `Withdrawn(…, 0)` can appear before the outer one. |
| 3 | timestamp (Low / Medium) | `open`: `t0 <= block.timestamp` | False positive (by design) | Deadlines are the protocol. Windows are hours long. Since the Merge, proposers can shift `block.timestamp` only to slot boundaries (12 s), which is negligible. |
| 4 | timestamp | `setReady`: `block.timestamp >= s.t0` | False positive (by design) | Same as 3. |
| 5 | timestamp | `commitClaim`: `(Ready && now < t1) \|\| now >= t2` | False positive (by design) | Same as 3. Halmos proves that the commit and refund windows are disjoint at every instant (`check_windowsDisjoint`), so moving a timestamp across a boundary can only choose which of two legitimate exits is open. It can never open both. |
| 6 | timestamp | `claim`: `now >= lockedAt + revealWindow` | False positive (by design) | Same as 5: claim and Locked-refund are disjoint. The usual HTLC caveat applies and is covered in REPORT.md (I-05): a reveal sent close to the deadline can be delayed past it. |
| 7 | timestamp | `refund`: the three windows | False positive (by design) | Same as 5. |
| 8 | reentrancy-balance (High / Medium) | `open()`: `before` read before `transferFrom`, compared after | **Pattern present; not exploitable** (REPORT.md I-03) | Every nested escrow operation during the pull is balance-exact on its own: a nested `open` checks its own delta, and a nested `withdraw` removes exactly the credit it pays. So if the outer delta equals `amount`, the outer arrival equals `amount`. Put differently, re-entering the escrow during the pull can only change the delta in a way that is matched by an equal change in credits. Only a token that misreports its own transfers can pass while under-delivering, and such a token could lie in `balanceOf` anyway. The check targets fee-charging tokens. The taker defends against hostile tokens by verifying `token` (REPORT.md I-04). |
| 9–14 | naming-convention (Info / High) | `Pallas.dbl` / `maddG` params `X1, Y1, Z1` | False positive (style) | The names follow the EFD formula notation (dbl-2009-l, madd-2007-bl). That makes the formulas easier to audit against the reference. |
| 15–16 | too-many-digits (Info / Medium) | `Pallas.P`, `Pallas.Q` literals | False positive | Curve constants. They are checked against the Rust vectors by the existing `test_vectorsMatchRust` and against the published Pallas parameters. |

**Detectors that fired nothing, and why that is correct:**

- `unchecked-transfer`: both `transfer` and `transferFrom` return values are checked.
- `arbitrary-send-erc20`: `transferFrom` always pulls from `msg.sender`.
- `reentrancy-no-eth` / `reentrancy-benign`: no state is written after an external call.
- `uninitialized-*`, `incorrect-equality`: none apply.

Slither does not model token semantics, so it cannot see the fee-on-transfer insolvency (finding L-01 in REPORT.md).
