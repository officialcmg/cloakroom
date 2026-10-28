# Cloakroom

Non-custodial cross-chain swaps for shielded Zcash. A taker swaps ZEC for an EVM token (USDC on Base) with a market maker,
with no custodian, no bridge and no script on the Zcash side. The ZEC never leaves the shielded pool.

**Paper:** [`Cloakroom_Yellow_Paper.pdf`](Cloakroom_Yellow_Paper.pdf) (source in `paper/`). It gives the
protocol, the timing model, the proofs, the model checking, the privacy measurements and the evaluation.

## How it works, in one paragraph

The taker funds a fresh shielded address whose spending key is the sum `a + b` of two secret shares on
the Pallas curve: the taker holds `a`, the maker `b`. The maker locks the tokens in an EVM escrow. The
taker's claim on the escrow reveals `a`, which lets the maker spend the ZEC; the maker's refund reveals
`b`, which lets the taker sweep her ZEC back. The escrow checks each revealed share on chain with a
Pallas scalar multiplication. A commit-then-reveal claim, a Ready gate and a punish window make every
exit safe under stated bounds on inclusion delay, finality and reorganizations. Requests, quotes and
acceptance travel as encrypted memos inside shielded Zcash transactions.

## What is in the repository

| Folder | Contents |
|---|---|
| `contracts/` | `CloakroomEscrow.sol`, the `Pallas.sol` curve library, tests (unit, audit findings, invariants, Halmos, real and small-curve Pallas), and `audit/` (internal audit report, Slither, mutation testing, gas). |
| `node/` | The Rust maker/taker client: key setup, encrypted messages, both state machines, and a light wallet that builds v6 Ironwood transactions. `txaudit` and `lwdscan` are inspection tools. |
| `harness/` | `run.py` runs 18 two-party scenarios on Zebra regtest and anvil; per-scenario evidence in `evidence/`, summary in `RESULTS.md`. |
| `formal/` | The TLA+ model, TLC configurations (honest, malicious maker or taker, liveness, mutations) and TLC logs. |
| `measurements/` | 30 days of Zcash mainnet anonymity-set measurements and the cover-delay analysis behind the privacy section. |
| `testnet/` | `EVIDENCE.md`: swaps on Zcash testnet and Base Sepolia, with every transaction hash. Keys and configs are not in git. |
| `app/` | The local app: `cloakroom serve` serves a web UI on 127.0.0.1 for a maker or a taker; `app/dev.py` runs both against local test chains. |
| `demo/` | The hosted demo: slides at `/` and a shared live sandbox at `/demo/` (`host.py`, `Dockerfile`; see `demo/README.md`). |
| `design-system/` | Tokens, brand book, React components, fonts, logo and icons the app is built from. |
| `paper/` | LaTeX source, compiled PDF, and every review with its triage (`reviews/`). |
| `archive/` | Earlier prototypes, paper drafts and old run logs, kept for the record. |

## Results

- **Contracts:** 52/52 Foundry tests, Halmos bounded proofs, mutation testing and stateful fuzzing (`contracts/audit/REPORT.md`).
- **Model:** TLC checks safety and liveness at three small parameter points, and each mutated rule or shortened budget yields an attack (`formal/`).
- **Two-party scenarios:** 18/18 pass, 121/121 checks, including injected reorganizations, an impostor maker, malicious quotes and a funding mined after the maker's refund. Every Zcash transaction is checked to be fully shielded.
- **Testnet:** the final escrow on Base Sepolia (`0xCCE4aFc81A6ccC9Db05B2a7D157942435b944283`) has settled three swaps, the last with the final code (`testnet/EVIDENCE.md`).

## Run it

Requirements: Rust (stable), Foundry 1.2.x, Python 3, Docker (for Zebra 7.0.0-rc.0), and TeX Live with `txfonts` for the paper.

```sh
# contracts
cd contracts && ./audit/gen_harnesses.sh && forge test --skip halmos --skip invariant

# node
cd node && cargo build

# the local app: a maker and a taker on local chains (http://127.0.0.1:7801 and :7802)
python3 -I app/dev.py

# two-party scenarios (starts Zebra regtest in Docker and anvil; about 50 minutes for all)
python3 -I harness/run.py            # or a subset: python3 -I harness/run.py s1 s15

# paper
cd paper && pdflatex cloakroom.tex && pdflatex cloakroom.tex
```

## Status and limits

This is research code for the ZECATHON hackathon, not a product. The contracts have had an internal audit
only. The light wallet has no Tor support, and the final testnet runs had cover delays switched off.
`FUTURE.md` lists the known open items, and the paper's Limitations section states the assumptions.

## License

MIT, see [`LICENSE`](LICENSE).
