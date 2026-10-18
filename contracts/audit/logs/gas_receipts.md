# Escrow gas per transaction (receipt gasUsed)

anvil 1.2.3-stable, chain id 84532, solc 0.8.30 via_ir, optimizer 200. Measured 2026-10-14.

| Transaction | gasUsed |
|---|---|
| open (2 on-curve checks, transferFrom, balance-delta check) | 337254 |
| setReady | 32494 |
| commitClaim (ecrecover, sent by relayer) | 69456 |
| claim (scalar check a·G = A, credits payout and relayer) | 555423 |
| withdraw (payout) | 57071 |
| withdraw (relayer) | 52259 |
| open (second swap, warm token balance) | 327654 |
| refund at t1 (scalar check b·G = B, credits maker) | 529429 |
| withdraw (maker) | 52271 |
check: payout received amount - fee
