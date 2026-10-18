# Cloakroom e2e on Zebra regtest

This is an end-to-end run on a **real local Zebra node** (regtest, NU6.3 "Ironwood" rules active from height 1, NU7 not activated). It shows that a note held at a jointly derived Orchard-protocol address can be spent by a v6 transaction. The transaction is built through the PCZT roles. The spend is authorized by an **external RedPallas signature** made with the summed key share `(a+b)+alpha` over the real ZIP 244 shielded sighash. The node accepts the transaction to its mempool and mines it.

The results from the recorded run are in [EVIDENCE.md](EVIDENCE.md).

## What the program does (`src/main.rs`)

1. **Joint key.** It picks random shares `a` and `b`, with `A = [a]G` and `B = [b]G` (where G is the SpendAuth base point). It sets `ak = A + B`, negating both shares when needed so that `ytilde(ak) = 0`. `nk` and `rivk` are hashed from seeds. `FullViewingKey::from_bytes(ak || nk || rivk)` gives the joint address. This is the same derivation as `../keys-spike`. The address is encoded as a regtest Unified Address with only an Orchard receiver (`uregtest1...`).
2. **Funding.** `generatetoaddress 1 <joint UA>` mines a block whose coinbase pays a **shielded Ironwood output** to the joint address. From NU6.3, Zebra sends shielded coinbase to the Ironwood pool, and the coinbase is a v6 transaction. Two more blocks are mined to the default transparent miner address.
3. **Finding the note.** The program reads every block and transaction over RPC (`getblock`, `getrawtransaction`) and parses each one with `zcash_primitives::Transaction::read(.., BranchId::Nu6_3)`. It tries to decrypt every Ironwood action with the joint IVK (`IronwoodDomain`). It also collects every Ironwood `cmx` in chain order into a `ShardTree` and witnesses the note. The local root is checked against the node's `z_gettreestate(...).ironwood.commitments.finalRoot`.
4. **Building the transaction (Creator).** It uses `zcash_primitives::transaction::builder::Builder` with `BuildConfig::Standard { ironwood_anchor: Some(root), .. }`. It adds one Ironwood spend (the joint FVK, the note and its witness) and one Ironwood output to a fresh recipient. The output uses `ovk = None` and pays `value - 10,000` zats, the ZIP 317 fee. `build_for_pczt` is followed by `pczt::roles::creator::Creator::build_from_parts`.
5. **`IoFinalizer`, then `Prover`.** The prover makes the halo2 proof with `ProvingKey::build(OrchardCircuitVersion::PostNu6_3)`. The PCZT is then serialized and parsed again, as it would be when passed between parties.
6. **Signer.**
   - `alpha` for the joint spend is read from the PCZT through `low_level_signer::Signer::sign_ironwood_with`. This step only reads.
   - `pczt::roles::signer::Signer::new(pczt).shielded_sighash()` gives the real ZIP 244 sighash for the whole v6 transaction.
   - The external signature is `SigningKey(a+b).randomize(alpha).sign(sighash)`, applied with `Signer::apply_ironwood_signature(idx, sig)`.
   - **Negative test, local:** signatures made with `b` alone or `a` alone are rejected by the Signer with `IronwoodSign(InvalidExternalSignature)`.
7. **`TransactionExtractor`.** It adds the binding signature, then checks the proof and all signatures locally with the post-NU6.3 verifying key.
8. **Negative test, node:** the program makes a byte-for-byte copy of the final transaction and replaces only the joint spend's 64-byte `spendAuthSig` with a signature from `b` alone. It submits that copy with `sendrawtransaction`, and the node rejects it.
9. **Broadcast and mine.** The real transaction goes out with `sendrawtransaction`, then `getrawmempool`, `generate 1`, `getblock` and `getrawtransaction <txid> 1`.
10. **Recipient check.** The program fetches the mined transaction from the node, and the recipient's IVK decrypts the output (value and memo). The on-chain nullifier equals the joint note's nullifier. The joint OVK cannot recover the output, because the sweep used `ovk = None`.

Every RPC request and its full response is written to `evidence/run.log`.

## Reproduce

Prerequisites: Docker Desktop running, a Rust toolchain (tested with 1.94.0), and `python3` for the evidence excerpt.

```bash
cd cloakroom/e2e-regtest
./run.sh
```

`run.sh` does the following:

```bash
docker pull zfnd/zebra:7.0.0-rc.0                 # ~119 MB compressed
(cd zebra && docker compose up -d)                # regtest node, RPC on 127.0.0.1:18232, ephemeral state
cargo build                                       # dev profile with opt-level=3
./target/debug/e2e-regtest | tee evidence/console.txt
python3 -I scripts/excerpt.py evidence/run.log > evidence/rpc_excerpts.txt
```

To restart from a fresh chain, run `(cd zebra && docker compose down && docker compose up -d)`. State is ephemeral, so a restart starts again at the genesis block. The program expects a chain whose next block is NU6.3 (branch `37a5165b`). It generates new keys every run, so the txids differ between runs.

To stop the node, run `(cd zebra && docker compose down)`.

## Node configuration (`zebra/zebrad.toml`)

- `network = "Regtest"`, with Canopy, NU5, NU6, NU6.1, NU6.2 and NU6.3 all at height 1. NU7 is not set.
- `[mining] miner_address`: the transparent regtest address from the Zebra book, used by `generate`. The funding block uses `generatetoaddress` with the joint UA.
- `[rpc] listen_addr = "0.0.0.0:18232"`, `enable_cookie_auth = false`. Docker publishes the port on `127.0.0.1` only.
- `[mempool] debug_enable_at_height = 0`, `[state] ephemeral = true`.

## Versions

| Component | Version |
|---|---|
| zebrad | `zfnd/zebra:7.0.0-rc.0` (`/Zebra:7.0.0-rc.0/`, image `sha256:d534756d899b...`). Built with orchard 0.16.0, zcash_primitives =0.31.0-pre.0 and zcash_protocol =0.11.0-pre.0. |
| orchard | 0.16.0 |
| pczt | 0.10.0-pre.1 (features: orchard, io-finalizer, prover, signer, tx-extractor, zcp-builder) |
| zcash_primitives | 0.31.0-pre.1 |
| zcash_protocol | 0.11.0-pre.0 (feature: local-consensus) |
| zcash_address | 0.14.0-pre.1 |
| zcash_transparent | 0.11.0-pre.1 (pulled in by other crates) |
| shardtree / incrementalmerkletree | 0.8.0 / 0.9.0 |
| zcash_note_encryption | 0.5.1 |
| rustc | 1.94.0 |

## Notes and caveats

- **Funding uses a shielded coinbase, not a wallet send.** Under ZIP 213, Zebra encrypts the coinbase output with an all-zero OVK, so anyone can see the coinbase amount and recipient. That is fine on regtest, but real funding should come from a normal wallet transaction.
- **One process knows both `a` and `b`.** This run shows that the sum-of-shares key is cryptographically valid on a real chain. It does not show how the shares are exchanged between parties; the Base-side reveal is not part of this run.
- **Misleading node error text.** Zebra rejects the transaction with only the `b` share using the message `Orchard or Ironwood Halo2 proof verification failed`. The proof itself is fine. Zebra checks each Orchard-shaped bundle with `orchard::bundle::BatchValidator::add_bundle`, which checks the halo2 proof, every `spendAuthSig` under `rk`, and the `bindingSig` together, and any failure there is reported under that name (see `zebra-consensus/src/primitives/halo2.rs`). The program's local check of the tampered transaction shows that only the `spendAuthSig` is bad: the txid is unchanged, the proof is valid, the `bindingSig` is valid, and the `spendAuthSig` is invalid.
- **Pre-signing option.** `zcash_primitives` 0.31.0-pre.1 also has a `DeferredPcztBuilder` (ZIP 374). For V6 transactions it lets the anchor and witness be filled in after signing, because the v6 txid and sighash do not commit to the anchor. That could suit a pre-signed refund or sweep path. It was not used here.
