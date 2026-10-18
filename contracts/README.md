# Cloakroom contracts

- `src/CloakroomEscrow.sol`: the escrow. `open`, `setReady`, `commitClaim` + `claim` (reveals the taker's share `a`),
  the refund paths (reveal the maker's share `b`), pull-based withdrawals.
- `src/Pallas.sol`: Pallas curve arithmetic used to check a revealed share against its public point.
- `src/MockUSDC.sol`: test token for the local harness (testnet runs use Circle's test USDC).
- `test/`: unit, audit-finding, invariant, Halmos and Pallas (real and small-curve) tests.
- `audit/`: internal audit report, Slither output, Halmos and mutation-testing scripts, gas measurement.

```sh
./audit/gen_harnesses.sh
forge test --skip halmos --skip invariant   # 52 tests
```
