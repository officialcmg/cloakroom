# Cloakroom on public networks: Zcash testnet ↔ Base Sepolia (2026-10-14)

Two real swaps ran between separate maker and taker processes. They communicated only through encrypted Zcash memos and the escrow contract on Base Sepolia.

- **Zcash testnet:**
  - NU7 rules (branch `77190ad9`)
  - accessed through the public `testnet.zec.rocks:443` server (ECC lightwalletd v0.5.4)
  - wallets sync compact blocks from their birthday height
- **Base Sepolia:**
  - escrow `0x2df30788C417D09307aaA02A62e401B5a0e939a1`, source verified on [Blockscout](https://base-sepolia.blockscout.com/address/0x2df30788c417d09307aaa02a62e401b5a0e939a1)
  - Circle test USDC `0x036CbD53842c5426634e7929541eC2318f3dCF7e`
  - a second, identical escrow `0x1EDd2B30…4e7C` was deployed by mistake and is unused
- **Parameters:**
  - lot = 0.01 TAZ, price = 1 USDC per lot, relayer fee = 0.1 USDC
  - t0 = open + 15 min, t1 = t0 + 15 min
  - the maker waits for 3 Zcash confirmations before Ready

Links:
- Base: `https://sepolia.basescan.org/tx/<hash>` or `https://base-sepolia.blockscout.com/tx/<hash>`
- Zcash: `https://testnet.zcashexplorer.app/transactions/<txid>`. This explorer blocks automated requests, so its links were not checked; the shielded details below come from our own `txaudit` tool.

## Swap 1: happy path (RFQ `6bd8cbc8`, escrow id `0x600ea5dc…2ed0`)

| Step | Chain | Transaction |
|---|---|---|
| RFQ memo (taker → maker) | Zcash | `e051ce38822707bbcab03352781a71c59e81877bb8daf7a5f1c4d1c5ef0d400b` (block 4,501,217) |
| QUOTE memo (maker → taker) | Zcash | `3b52ca30a3116968d08ac821e5373d9eb23e33dd09c2222f75a2985b9421fe21` (4,501,219) |
| ACCEPT memo (taker → maker) | Zcash | `ee35788f59599dad48860b37b7d0fae55fbdb6fbdd541dfe36b228d7b63e1e59` (4,501,225) |
| `open` (1 USDC escrowed) | Base | `0xc0eced68f45e04f504ee09d526815af8f1ec293ee04a20867d6cd6a6de7611cb` (gas 297,756) |
| Fund joint address (0.0101 TAZ) | Zcash | `1a9ca2861ae50227347fd9c00a745b3b710a1afd4d8d5b8a3a7ca15210a8b160` (4,501,234) |
| `setReady` (after 4 confirmations) | Base | `0x33159658e5c2843a9a5ad1e186b57c82d9d4b4ecb45c9d9a631d3fb591b80e6a` (gas 32,440) |
| `claim(a)` via relayer → fresh payout | Base | `0xb6487a16d041f9263d6aeacd56d5f36a7d700834ce7cbe57bc38b2dd20aa6041` (gas 564,054) |
| Maker sweeps with (a+b), ovk = None | Zcash | `b8b53aaee23d83edc9f1ca13436ac08881776ff88a101112435361b69ceed454` (4,501,240) |

**Outcome, checked on chain:**
- Escrow state: Claimed.
- Fresh payout `0x6727…95df` holds 0.9 USDC, the relayer holds 0.1 USDC, and the maker went from 20 to 19 USDC.
- Maker ZEC 0.03 → 0.0399 TAZ, and taker ZEC 0.0949 → 0.0845 TAZ. Both are exact to the zatoshi after fees.

## Swap 2: refund path, taker vanishes after funding (RFQ `a86d925c`, escrow id `0x77f3b9e5…aea2`)

| Step | Chain | Transaction |
|---|---|---|
| RFQ / QUOTE / ACCEPT memos | Zcash | `5a915a21…87d8`, `754d956a…0cf6`, `64d6fb54…9d2c` |
| `open` | Base | `0x1a0c774eb6f1e0d9084c20fee68fc589af1b71f5f1eb8077dc7bf98cb58b2235` |
| Fund joint address, then the taker process **exits** | Zcash | `a65a51610af40df2e40a7575b3d2a1f4d5cb06b2593533f6fb33588319bf6ed8` (4,501,274) |
| `setReady` | Base | `0x2a8c9b2cae89706f1fa171ae9a61be333db60617b44e7e74819ba3d1a28419cc` |
| No claim by t1 → `refund(b)` publishes b | Base | `0x3ff79f4350526bbf85f6f63a3301313a1f04ad7d2512d014c15a2ff96528e09d` (gas 505,221, block 48,106,568) |
| The taker returns, reads b, and sweeps the note back with (a+b) | Zcash | `a7bcdd1e8d3b5fde1a23eb92bee136cd35a94a128ce7e761a5da1ef28c4824ed` (4,501,358) |

**Outcome, checked on chain:**
- Escrow state: Refunded.
- The maker still holds 19 USDC; his tokens came back.
- Taker ZEC 0.0845 → 0.0841 TAZ: exactly four 0.0001 fees lost and nothing else.
- During the 30-minute wait the maker hit two Base RPC timeouts. It retried and carried on.

## Privacy audit of every public Zcash transaction (`node/target/debug/txaudit`)

All 11 transactions are listed below: the funding transfer, swap 1 and swap 2.
- Every one is v6.
- None has transparent inputs, transparent outputs, Sapling activity or Orchard-pool activity.
- Each has 2 Ironwood actions, and its Ironwood value balance equals exactly the 10,000-zatoshi fee.

```
fb86ca7c…f997  4501214  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   (taker → maker 0.03 TAZ)
e051ce38…400b  4501217  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 1 RFQ
3b52ca30…fe21  4501219  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 1 QUOTE
ee35788f…1e59  4501225  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 1 ACCEPT
1a9ca286…b160  4501234  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 1 FUND
b8b53aae…d454  4501240  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 1 SWEEP (joint key)
5a915a21…87d8  4501243  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 2 RFQ
754d956a…0cf6  4501250  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 2 QUOTE
64d6fb54…9d2c  4501258  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 2 ACCEPT
a65a5161…6ed8  4501274  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 2 FUND
a7bcdd1e…24ed  4501358  v6  t 0/0  sapling 0/0  orchard 0  ironwood 2  vb 10000   swap 2 RECOVERY (joint key)
```

## Bugs found only on public infrastructure (all fixed)

1. **Stale nonce.** Base's public RPC served a stale pending nonce, so `open` collided with the preceding `approve` ("nonce too low").
   - Fix: EVM sends now retry.
   - The maker also no longer marks a message handled when processing fails.
   - `open` is now idempotent: it checks the escrow before sending.
2. **Log-range limit.** `eth_getLogs` is capped at 200 blocks on the public RPC. Fix: event lookups query in 200-block chunks.
3. **Crash on transient errors.** A network error made the process exit. Fix: maker and taker log transient errors and retry. This worked in swap 2.
4. **Stale balance read.** A balance read straight after the claim returned 0, a stale RPC read. Fixed by checking on chain later: the payout held 0.9 USDC.

After these fixes the local regression suite (`harness/run.py`) still passes all 7 scenarios.

---

# Final binary on the final escrow (2026-10-18)

Run from a cloud container with the code at branch `final-round`: signed directory and quotes, rule R1q,
and the cover-delay and erasure changes of the final round. Escrow `0xCCE4aFc81A6ccC9Db05B2a7D157942435b944283`
(runtime bytecode identical to our build), Circle test USDC, Zcash testnet via `testnet.zec.rocks:443`.
Parameters: lot = 0.01 TAZ, price = 1 USDC, relayer fee = 0.1 USDC, premium = 20,000 zats,
D = 30 s, F = 1,500 s, R = 1, t1 - t0 = t2 - t1 = W = 3,300 s, k_Z = 3. Logs stay on the machine
(they contain the payout key); every hash below was checked on chain.

## Final swap A: refund, and a fund-level bug (RFQ `ed492258`, escrow id `0x3a0d57cb…78b7`)

t0 = open + 40 min. Base Sepolia: `open` block 48,287,386; maker refund `0xb6190439…002e` block 48,288,554;
maker withdrawal block 48,288,999.

- RFQ, signed Quote and Accept went through; the quote signature verified (the round-2 signing bug is fixed).
- The taker funded at Zcash height 4,515,917 (tx `9cc85ed4…566f`), but testnet mined it only at **4,515,934**,
  17 blocks later. At t0 the maker had seen no note and refunded, publishing b (rule R3e, correct).
- **Bug:** the taker's wallet did not show the note either. It concluded that nothing was funded and erased a.
  The note was then mined with b public and a gone: **1,010,000 testnet zats are unspendable**.
- **Fix:** the taker keeps a while a funding it broadcast is not visible, until the note appears (it then sweeps
  with a+b) or the funding is past its 40-block expiry plus k_Z (rule R4). Harness scenario s18 holds the funding
  in the mempool across the maker's refund; it fails on the old code with the same log line and passes 8/8 on the fix.

## Final swap B: settled (RFQ `e9f6faf4`, escrow id `0x3f2777de…ba91`)

t0 = open + 80 min (doubled after swap A). Fixed binary.

| Step | Chain | Tx | Block / height |
|---|---|---|---|
| RFQ | Zcash | `76ba4650…82a5` | 4,515,972 |
| Quote (signed) | Zcash | `dd58f47d…fc7f` | 4,515,975 |
| Accept + premium | Zcash | `6ec049da…3c64` | 4,515,979 |
| `open` | Base | `0x67f6bebe…39d7` | 48,289,158 |
| funding, 1,010,000 zats | Zcash | `97e4cce4…17c7` | 4,516,038 |
| `setReady` (3 confirmations) | Base | `0x6eebee12…558b` | 48,289,942 |
| `commitClaim` (publishes only H(id, a)) | Base | `0x84cc5b34…49eb` | 48,289,944 |
| `claim` (reveals a) | Base | `0x9cf0e630…8052` | 48,290,629 |
| maker sweep with a+b | Zcash | `aa53b4d2…baf1` | 4,516,112 |
| payout withdrawal (claim final) | Base | `0x09cf252a…dc41` | 48,291,111 |

Maker ended `Swept`, taker `Claimed`; both erased their per-swap secrets.

## Privacy audit of the final-binary swaps

All 9 Zcash transactions of swaps A and B (`txaudit`): v6, 0/0 transparent inputs/outputs, no Sapling or
Orchard activity, 2 Ironwood actions each.

## Also found in this run (fixed)

- The wallet's lightwalletd client trusted only the webpki roots, so it could not connect where egress re-signs
  TLS. It (and `txaudit`, `lwdscan`) now also trust the roots in `SSL_CERT_FILE` when set.
- The implementation commits as soon as Ready is visible, while the paper's R2c said "once Ready is final".
  Not a fund issue (a commit reveals only c, and a is revealed only once the lock is final, which implies a
  final Ready); recorded for the paper.
