//! Cloakroom end-to-end on a local Zebra regtest node (NU6.3 / Ironwood rules).
//!
//! 1. Two parties hold shares a, b of the spend authorizing key; joint FVK = (A+B, nk, rivk).
//! 2. The joint Ironwood address is funded by a real shielded coinbase (`generatetoaddress`).
//! 3. The note is found by trial-decrypting the chain with the joint IVK; its Merkle witness is
//!    rebuilt from every Ironwood cmx on the chain.
//! 4. A v6 transaction is built with zcash_primitives' Builder and driven through the PCZT
//!    roles (Creator -> IoFinalizer -> Prover -> Signer -> TransactionExtractor). The spend is
//!    authorized by an EXTERNAL RedPallas signature over the real ZIP 244 shielded sighash
//!    (taken from the Signer role) made with rsk = (a + b) + alpha.
//! 5. Negative tests: the Signer rejects a b-only signature, and the node rejects a copy of the
//!    final transaction whose spendAuthSig was replaced by a b-only signature.
//! 6. The good transaction is broadcast, mined, and the recipient decrypts the output.
//!
//! Every RPC call and response is appended to `evidence/run.log`.

use anyhow::{anyhow, bail, Context, Result};
use blake2b_simd::Params;
use ff::{Field, FromUniformBytes, PrimeField};
use group::GroupEncoding;
use incrementalmerkletree::{Position, Retention};
use orchard::{
    circuit::{OrchardCircuitVersion, ProvingKey, VerifyingKey},
    keys::{FullViewingKey, PreparedIncomingViewingKey, Scope, SpendingKey},
    note_encryption::IronwoodDomain,
    primitives::redpallas::{self, SpendAuth},
    tree::MerkleHashOrchard,
};
use pasta_curves::pallas;
use pczt::{
    roles::{
        creator::Creator, io_finalizer::IoFinalizer, low_level_signer, prover::Prover,
        signer::Signer, tx_extractor::TransactionExtractor,
    },
    Pczt,
};
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use serde_json::{json, Value};
use shardtree::{store::memory::MemoryShardStore, ShardTree};
use std::{convert::Infallible, fs, io::Write, time::Instant};
use zcash_address::unified::{self, Encoding};
use zcash_note_encryption::{try_note_decryption, try_output_recovery_with_ovk};
use zcash_primitives::transaction::{
    builder::{BuildConfig, Builder, BundlePadding},
    fees::zip317,
    Transaction,
};
use zcash_protocol::{
    consensus::{BlockHeight, BranchId, NetworkType},
    local_consensus::LocalNetwork,
    memo::MemoBytes,
    value::Zatoshis,
};

#[allow(non_upper_case_globals)]
const OsRng: UnwrapErr<SysRng> = UnwrapErr(SysRng);

const RPC_URL: &str = "http://127.0.0.1:18232";
const FEE: u64 = 10_000; // ZIP 317: max(2 grace actions, 1 logical action) x 5,000 zats

// ---------------------------------------------------------------- evidence log + RPC

struct Ctx {
    http: reqwest::blocking::Client,
    log: fs::File,
    id: u64,
}

impl Ctx {
    fn note(&mut self, s: &str) {
        println!("{s}");
        let _ = writeln!(self.log, "{s}");
    }

    /// JSON-RPC call; returns Ok(result) or Err(full error object as text). Always logged.
    fn rpc_raw(&mut self, method: &str, params: Value) -> Result<std::result::Result<Value, Value>> {
        self.id += 1;
        let req = json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params});
        let resp: Value = self
            .http
            .post(RPC_URL)
            .json(&req)
            .send()
            .with_context(|| format!("RPC {method} transport"))?
            .json()
            .with_context(|| format!("RPC {method} decode"))?;
        let shown_params = abbreviate(&params);
        let _ = writeln!(self.log, "\n$ rpc {method} {shown_params}");
        let _ = writeln!(self.log, "{}", serde_json::to_string_pretty(&resp).unwrap());
        if let Some(err) = resp.get("error").filter(|e| !e.is_null()) {
            return Ok(Err(err.clone()));
        }
        Ok(Ok(resp["result"].clone()))
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        self.rpc_raw(method, params)?
            .map_err(|e| anyhow!("RPC {method} error: {e}"))
    }
}

/// Keep the log readable: long hex params (raw transactions) are shortened in the echo line
/// (the full hex is written to evidence/*.hex).
fn abbreviate(v: &Value) -> String {
    match v {
        Value::Array(a) => format!(
            "[{}]",
            a.iter()
                .map(|x| match x {
                    Value::String(s) if s.len() > 120 => {
                        format!("\"{}...({} hex chars)\"", &s[..64], s.len())
                    }
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------- key helpers (as keys-spike)

fn h64(domain: &[u8], parts: &[&[u8]]) -> [u8; 64] {
    let mut st = Params::new().hash_length(64).personal(b"Cloakroom_v0_H64").to_state();
    st.update(&(domain.len() as u64).to_le_bytes());
    st.update(domain);
    for p in parts {
        st.update(&(p.len() as u64).to_le_bytes());
        st.update(p);
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(st.finalize().as_bytes());
    out
}
fn hash_to_scalar(domain: &[u8], parts: &[&[u8]]) -> pallas::Scalar {
    pallas::Scalar::from_uniform_bytes(&h64(domain, parts))
}
fn hash_to_base(domain: &[u8], parts: &[&[u8]]) -> pallas::Base {
    pallas::Base::from_uniform_bytes(&h64(domain, parts))
}
/// G^Orchard (SpendAuth basepoint) = verification key of the signing key with scalar 1.
fn spend_auth_g() -> pallas::Point {
    let one = redpallas::SigningKey::<SpendAuth>::try_from(pallas::Scalar::ONE.to_repr()).unwrap();
    let vk: [u8; 32] = (&redpallas::VerificationKey::from(&one)).into();
    pallas::Point::from_bytes(&vk).unwrap()
}
fn ytilde(p: &pallas::Point) -> u8 {
    p.to_bytes()[31] >> 7
}
fn sign_with(share: &pallas::Scalar, alpha: &pallas::Scalar, sighash: &[u8; 32]) -> redpallas::Signature<SpendAuth> {
    redpallas::SigningKey::<SpendAuth>::try_from(share.to_repr())
        .expect("nonzero scalar")
        .randomize(alpha)
        .sign(OsRng, sighash)
}
fn sig_bytes(sig: &redpallas::Signature<SpendAuth>) -> [u8; 64] {
    sig.into()
}

fn regtest_params() -> LocalNetwork {
    let one = Some(BlockHeight::from_u32(1));
    LocalNetwork {
        overwinter: one,
        sapling: one,
        blossom: one,
        heartwood: one,
        canopy: one,
        nu5: one,
        nu6: one,
        nu6_1: one,
        nu6_2: one,
        nu6_3: one,
        nu7: None,
    }
}

fn encode_ua(addr: &orchard::Address) -> String {
    let ua = unified::Address::try_from_items(
        zcash_protocol::address::Revision::R0,
        vec![unified::Uitem::Data(unified::Receiver::Orchard(addr.to_raw_address_bytes()))],
    )
    .expect("valid UA");
    ua.encode(&NetworkType::Regtest)
}

fn fetch_tx(cx: &mut Ctx, txid: &str) -> Result<(Vec<u8>, Transaction)> {
    let hex_tx = cx.rpc("getrawtransaction", json!([txid, 0]))?;
    let bytes = hex::decode(hex_tx.as_str().ok_or_else(|| anyhow!("hex"))?)?;
    let tx = Transaction::read(&bytes[..], BranchId::Nu6_3).context("parse tx")?;
    Ok((bytes, tx))
}

#[derive(Debug)]
#[allow(dead_code)]
struct LlErr(String);
impl From<low_level_signer::OrchardParseError> for LlErr {
    fn from(e: low_level_signer::OrchardParseError) -> Self {
        LlErr(format!("{e:?}"))
    }
}

// ---------------------------------------------------------------- main

fn main() -> Result<()> {
    fs::create_dir_all("evidence")?;
    let mut cx = Ctx {
        http: reqwest::blocking::Client::new(),
        log: fs::File::create("evidence/run.log")?,
        id: 0,
    };
    let params = regtest_params();

    cx.note("== Cloakroom e2e on Zebra regtest (NU6.3 / Ironwood, v6 tx, PCZT, external RedPallas sig) ==");
    let info = cx.rpc("getinfo", json!([]))?;
    cx.note(&format!("node: {} {}", info["build"], info["subversion"]));
    let bci = cx.rpc("getblockchaininfo", json!([]))?;
    cx.note(&format!("chain tip {} ; next block consensus branch {}", bci["blocks"], bci["consensus"]["nextblock"]));
    if bci["consensus"]["nextblock"] != "37a5165b" {
        bail!("next block is not NU6.3 (branch 37a5165b); check zebrad.toml activation heights");
    }

    // ---------- 1. joint key
    let g = spend_auth_g();
    let mut a = pallas::Scalar::random(&mut { OsRng });
    let mut b = pallas::Scalar::random(&mut { OsRng });
    if ytilde(&(g * a + g * b)) == 1 {
        a = -a;
        b = -b;
    }
    let ak = g * a + g * b;
    assert_eq!(ytilde(&ak), 0);
    let mut seed_a = [0u8; 32];
    let mut seed_b = [0u8; 32];
    rand::Rng::fill_bytes(&mut { OsRng }, &mut seed_a);
    rand::Rng::fill_bytes(&mut { OsRng }, &mut seed_b);
    let swap_id = b"cloakroom-e2e-regtest-0001";
    let nk = hash_to_base(b"nk", &[swap_id, &seed_a, &seed_b]);
    let rivk = hash_to_scalar(b"rivk", &[swap_id, &seed_a, &seed_b]);
    let mut fvk_bytes = [0u8; 96];
    fvk_bytes[..32].copy_from_slice(&ak.to_bytes());
    fvk_bytes[32..64].copy_from_slice(&nk.to_repr());
    fvk_bytes[64..].copy_from_slice(&rivk.to_repr());
    let joint_fvk = FullViewingKey::from_bytes(&fvk_bytes).ok_or_else(|| anyhow!("FVK rejected"))?;
    let joint_addr = joint_fvk.address_at(0u32, Scope::External);
    let joint_ua = encode_ua(&joint_addr);
    cx.note(&format!("\n[1] joint FVK accepted (ak = A + B, ytilde(ak) = 0)"));
    cx.note(&format!("    A  = {}", hex::encode((g * a).to_bytes())));
    cx.note(&format!("    B  = {}", hex::encode((g * b).to_bytes())));
    cx.note(&format!("    ak = {}", hex::encode(ak.to_bytes())));
    cx.note(&format!("    joint UA (Orchard-protocol receiver only, regtest) = {joint_ua}"));
    if let Ok(v) = cx.rpc("z_validateaddress", json!([joint_ua])) {
        cx.note(&format!("    node z_validateaddress: {v}"));
    }

    // ---------- 2. fund via shielded coinbase
    let funding_hash = cx.rpc("generatetoaddress", json!([1, joint_ua]))?[0]
        .as_str()
        .ok_or_else(|| anyhow!("no block hash"))?
        .to_string();
    let fblock = cx.rpc("getblock", json!([funding_hash, 1]))?;
    let funding_height = fblock["height"].as_u64().unwrap();
    let funding_txid = fblock["tx"][0].as_str().unwrap().to_string();
    cx.note(&format!("\n[2] funding block (coinbase -> joint UA): height {funding_height}, hash {funding_hash}"));
    cx.note(&format!("    funding (coinbase) txid {funding_txid}"));
    let fverbose = cx.rpc("getrawtransaction", json!([funding_txid, 1]))?;
    cx.note(&format!(
        "    coinbase tx version {}, ironwood actions {}, valueBalanceIronwood {}",
        fverbose["version"],
        fverbose["ironwood"]["actions"].as_array().map(|a| a.len()).unwrap_or(0),
        fverbose["ironwood"]["valueBalance"]
    ));
    // a couple of ordinary blocks on top (default transparent miner address)
    cx.rpc("generate", json!([2]))?;

    // ---------- 3. scan chain: all Ironwood cmx in order + trial decryption with joint IVK
    let tip = cx.rpc("getblockcount", json!([]))?.as_u64().unwrap();
    let ivk = PreparedIncomingViewingKey::new(&joint_fvk.to_ivk(Scope::External));
    let mut leaves: Vec<MerkleHashOrchard> = vec![];
    let mut found: Option<(orchard::Note, u64, String)> = None;
    for h in 1..=tip {
        let blk = cx.rpc("getblock", json!([h.to_string(), 1]))?;
        for txid in blk["tx"].as_array().unwrap().clone() {
            let txid = txid.as_str().unwrap().to_string();
            let (_, tx) = fetch_tx(&mut cx, &txid)?;
            if let Some(bundle) = tx.ironwood_bundle() {
                for act in bundle.actions() {
                    let pos = leaves.len() as u64;
                    leaves.push(MerkleHashOrchard::from_cmx(act.cmx()));
                    if let Some((note, _, _)) =
                        try_note_decryption(&IronwoodDomain::for_action(act), &ivk, act)
                    {
                        found = Some((note, pos, txid.clone()));
                    }
                }
            }
        }
    }
    let (note, note_pos, note_txid) = found.ok_or_else(|| anyhow!("joint IVK found no note"))?;
    cx.note(&format!(
        "\n[3] joint IVK decrypted note in tx {note_txid}: value = {} zats, version {:?}, tree position {note_pos} of {} Ironwood leaves",
        note.value().inner(),
        note.version(),
        leaves.len()
    ));

    let mut tree: ShardTree<MemoryShardStore<MerkleHashOrchard, u32>, 32, 16> =
        ShardTree::new(MemoryShardStore::empty(), 100);
    for (i, leaf) in leaves.iter().enumerate() {
        let ret = if i as u64 == note_pos { Retention::Marked } else { Retention::Ephemeral };
        tree.append(*leaf, ret).map_err(|e| anyhow!("{e:?}"))?;
    }
    tree.checkpoint(0).map_err(|e| anyhow!("{e:?}"))?;
    let root = tree.root_at_checkpoint_id(&0).map_err(|e| anyhow!("{e:?}"))?.unwrap();
    let path = tree
        .witness_at_checkpoint_id(Position::from(note_pos), &0)
        .map_err(|e| anyhow!("{e:?}"))?
        .unwrap();
    let anchor: orchard::Anchor = root.into();
    cx.note(&format!("    local Ironwood root at tip {tip}: {}", hex::encode(anchor.to_bytes())));
    if let Ok(ts) = cx.rpc("z_gettreestate", json!([tip.to_string()])) {
        cx.note(&format!("    node z_gettreestate({tip}).ironwood = {}", ts["ironwood"]));
    }

    // ---------- 4. circuit keys
    let t = Instant::now();
    let pk = ProvingKey::build(OrchardCircuitVersion::PostNu6_3);
    let vk = VerifyingKey::build(OrchardCircuitVersion::PostNu6_3);
    cx.note(&format!("\n[4] post-NU6.3 circuit keys built in {:.2?}", t.elapsed()));

    // ---------- 5. Creator (via zcash_primitives Builder -> build_for_pczt)
    let recipient_sk: SpendingKey = loop {
        let mut s = [0u8; 32];
        rand::Rng::fill_bytes(&mut { OsRng }, &mut s);
        if let Some(sk) = Option::<SpendingKey>::from(SpendingKey::from_bytes(s)) {
            break sk;
        }
    };
    let recipient_fvk = FullViewingKey::from(&recipient_sk);
    let recipient_addr = recipient_fvk.address_at(0u32, Scope::External);
    cx.note(&format!("\n[5] recipient (fresh key, not joint) UA = {}", encode_ua(&recipient_addr)));

    let target_height = BlockHeight::from_u32((tip + 1) as u32);
    let mut builder = Builder::new(
        params,
        target_height,
        BuildConfig::Standard {
            sapling_anchor: None,
            orchard_anchor: None,
            ironwood_anchor: Some(anchor),
            orchard_padding: BundlePadding::DEFAULT,
            ironwood_padding: BundlePadding::DEFAULT,
        },
    );
    let v = note.value().inner();
    let send = v - FEE;
    builder
        .add_ironwood_spend::<Infallible>(joint_fvk.clone(), note.clone(), path.into())
        .map_err(|e| anyhow!("add_ironwood_spend: {e:?}"))?;
    let mut memo = [0u8; 512];
    memo[..26].copy_from_slice(swap_id);
    builder
        .add_ironwood_output::<Infallible>(None, recipient_addr, Zatoshis::from_u64(send).unwrap(), MemoBytes::from_bytes(&memo).unwrap())
        .map_err(|e| anyhow!("add_ironwood_output: {e:?}"))?;
    let res = builder
        .build_for_pczt(OsRng, &zip317::FeeRule::standard())
        .map_err(|e| anyhow!("build_for_pczt: {e:?}"))?;
    let spend_idx = res.ironwood_meta.spend_action_index(0).ok_or_else(|| anyhow!("no spend idx"))?;
    cx.note(&format!(
        "    builder: tx version {:?}, branch {:?}, expiry {}, ironwood spend at action index {spend_idx}, output {} zats, fee {FEE}",
        res.pczt_parts.version, res.pczt_parts.consensus_branch_id, u32::from(res.pczt_parts.expiry_height), send
    ));
    let pczt = Creator::build_from_parts(res.pczt_parts).ok_or_else(|| anyhow!("Creator: tx version unsupported"))?;
    cx.note("    Creator: PCZT created");

    // ---------- 6. IoFinalizer, Prover
    let pczt = IoFinalizer::new(pczt).finalize_io(OsRng).map_err(|e| anyhow!("IoFinalizer: {e:?}"))?;
    cx.note("    IoFinalizer: done (dummy spends signed, bsk derived)");
    let t = Instant::now();
    let pczt = Prover::new(pczt)
        .create_ironwood_proof(OsRng, &pk)
        .map_err(|e| anyhow!("Prover: {e:?}"))?
        .finish();
    cx.note(&format!("    Prover: Ironwood halo2 proof created in {:.2?}", t.elapsed()));

    // Transport round trip, as between parties.
    let wire = pczt.serialize().map_err(|e| anyhow!("Pczt::serialize: {e:?}"))?;
    let pczt = Pczt::parse(&wire).map_err(|e| anyhow!("Pczt::parse: {e:?}"))?;
    cx.note(&format!("    PCZT serialized ({} bytes) and re-parsed", wire.len()));

    // ---------- 7. Signer: real ZIP 244 shielded sighash, external signature
    // The external signer needs (sighash, alpha). alpha is read from the PCZT spend.
    let mut alpha: Option<pallas::Scalar> = None;
    let pczt = low_level_signer::Signer::new(pczt)
        .sign_ironwood_with(|_p, bundle, _m| -> Result<(), LlErr> {
            alpha = *bundle.actions()[spend_idx].spend().alpha();
            Ok(())
        })
        .map_err(|e| anyhow!("low-level signer: {e:?}"))?
        .finish();
    let alpha = alpha.ok_or_else(|| anyhow!("alpha missing in PCZT"))?;

    let mut signer = Signer::new(pczt).map_err(|e| anyhow!("Signer::new: {e:?}"))?;
    let sighash = signer.shielded_sighash();
    cx.note(&format!("\n[7] Signer: ZIP 244 shielded sighash = {}", hex::encode(sighash)));

    // Negative A: b alone
    let bad_sig = sign_with(&b, &alpha, &sighash);
    match signer.apply_ironwood_signature(spend_idx, bad_sig.clone()) {
        Err(e) => cx.note(&format!("    NEGATIVE: apply_ironwood_signature with b-only signature REJECTED: {e:?}")),
        Ok(()) => bail!("b-only signature was accepted by the Signer!"),
    }
    // Negative A': a alone
    match signer.apply_ironwood_signature(spend_idx, sign_with(&a, &alpha, &sighash)) {
        Err(e) => cx.note(&format!("    NEGATIVE: apply_ironwood_signature with a-only signature REJECTED: {e:?}")),
        Ok(()) => bail!("a-only signature was accepted by the Signer!"),
    }
    // Positive: (a + b) + alpha
    let good_sig = sign_with(&(a + b), &alpha, &sighash);
    signer
        .apply_ironwood_signature(spend_idx, good_sig.clone())
        .map_err(|e| anyhow!("good signature rejected: {e:?}"))?;
    cx.note("    apply_ironwood_signature with rsk = (a+b)+alpha ACCEPTED");
    let pczt = signer.finish();

    // ---------- 8. TransactionExtractor (binding sig + full local verification)
    let tx = TransactionExtractor::new(pczt)
        .with_orchard(&vk)
        .extract(OsRng)
        .map_err(|e| anyhow!("TransactionExtractor: {e:?}"))?;
    let mut tx_bytes = vec![];
    tx.write(&mut tx_bytes)?;
    let txid = tx.txid().to_string();
    fs::write("evidence/sweep_tx.hex", hex::encode(&tx_bytes))?;
    cx.note(&format!(
        "\n[8] TransactionExtractor: v6 tx extracted and verified locally; txid {txid}, {} bytes, header version bytes {}",
        tx_bytes.len(),
        hex::encode(&tx_bytes[..4])
    ));

    // ---------- 9. Negative B: same tx with spendAuthSig replaced by b-only signature
    let good = sig_bytes(&good_sig);
    let bad = sig_bytes(&sign_with(&b, &alpha, &sighash));
    let occurrences: Vec<usize> = tx_bytes.windows(64).enumerate().filter(|(_, w)| *w == good).map(|(i, _)| i).collect();
    if occurrences.len() != 1 {
        bail!("expected exactly one copy of spendAuthSig in tx bytes, found {}", occurrences.len());
    }
    let mut tampered = tx_bytes.clone();
    tampered[occurrences[0]..occurrences[0] + 64].copy_from_slice(&bad);
    fs::write("evidence/tampered_b_only_tx.hex", hex::encode(&tampered))?;
    let differing = tx_bytes.iter().zip(&tampered).filter(|(x, y)| x != y).count();
    cx.note(&format!(
        "\n[9] NEGATIVE (node): copy of the final tx with ONLY the joint spend's spendAuthSig replaced by a b-only signature (offset {}, {differing} bytes differ, same length {})",
        occurrences[0],
        tampered.len()
    ));
    // Local diagnosis of the tampered tx: proof and binding sig still valid, spendAuthSig not.
    {
        let ttx = Transaction::read(&tampered[..], BranchId::Nu6_3)?;
        let tb = ttx.ironwood_bundle().unwrap();
        let proof_ok = tb.verify_proof(&vk).is_ok();
        let binding_ok = tb.binding_validating_key().verify(&sighash, tb.authorization().binding_signature()).is_ok();
        let act = &tb.actions()[spend_idx];
        let spend_sig_ok = act.rk().verify(&sighash, act.authorization()).is_ok();
        cx.note(&format!(
            "    local check of tampered tx: txid unchanged = {}, halo2 proof valid = {proof_ok}, bindingSig valid = {binding_ok}, spendAuthSig(rk) valid = {spend_sig_ok}",
            ttx.txid().to_string() == txid
        ));
    }
    match cx.rpc_raw("sendrawtransaction", json!([hex::encode(&tampered)]))? {
        Err(e) => cx.note(&format!("    node REJECTED b-only tx: {e}")),
        Ok(r) => bail!("node ACCEPTED the b-only transaction: {r}"),
    }
    cx.note("    (Zebra verifies each Orchard-shaped bundle with orchard::bundle::BatchValidator::add_bundle, which batches the halo2 proof,");
    cx.note("     every action's spendAuthSig under rk, and the bindingSig; any failure surfaces as TransactionError::Halo2VerificationFailed.)");

    // ---------- 10. broadcast the real one, mine it
    let sent = cx.rpc("sendrawtransaction", json!([hex::encode(&tx_bytes)]))?;
    cx.note(&format!("\n[10] sendrawtransaction accepted: {sent}"));
    let mempool = cx.rpc("getrawmempool", json!([]))?;
    cx.note(&format!("     getrawmempool = {mempool}"));
    let mined_hash = cx.rpc("generate", json!([1]))?[0].as_str().unwrap().to_string();
    let mblock = cx.rpc("getblock", json!([mined_hash, 1]))?;
    let mined_height = mblock["height"].as_u64().unwrap();
    let included = mblock["tx"].as_array().unwrap().iter().any(|t| t.as_str() == Some(txid.as_str()));
    cx.note(&format!("     mined block height {mined_height}, hash {mined_hash}, tx list {}", mblock["tx"]));
    if !included {
        bail!("sweep tx not in mined block");
    }
    let verbose = cx.rpc("getrawtransaction", json!([txid, 1]))?;
    cx.note(&format!(
        "     getrawtransaction(verbose): version {}, height {}, confirmations {}, blockhash {}, ironwood actions {}",
        verbose["version"], verbose["height"], verbose["confirmations"], verbose["blockhash"],
        verbose["ironwood"]["actions"].as_array().map(|a| a.len()).unwrap_or(0)
    ));
    let mempool_after = cx.rpc("getrawmempool", json!([]))?;
    cx.note(&format!("     getrawmempool after mining = {mempool_after}"));

    // ---------- 11. recipient decrypts from the chain copy; joint OVK cannot recover
    let (_, chain_tx) = fetch_tx(&mut cx, &txid)?;
    let r_ivk = PreparedIncomingViewingKey::new(&recipient_fvk.to_ivk(Scope::External));
    let bundle = chain_tx.ironwood_bundle().ok_or_else(|| anyhow!("no ironwood bundle"))?;
    let got = bundle
        .actions()
        .iter()
        .find_map(|act| try_note_decryption(&IronwoodDomain::for_action(act), &r_ivk, act))
        .ok_or_else(|| anyhow!("recipient could not decrypt"))?;
    cx.note(&format!(
        "\n[11] recipient IVK decrypts mined output: value = {} zats, memo prefix = {:?}",
        got.0.value().inner(),
        String::from_utf8_lossy(&got.2[..26])
    ));
    let expected_nf = note.nullifier(&joint_fvk).to_bytes();
    let onchain_nf = bundle.actions()[spend_idx].nullifier().to_bytes();
    cx.note(&format!(
        "     on-chain nullifier of action {spend_idx} = {} ; equals joint note's nullifier: {}",
        hex::encode(onchain_nf),
        onchain_nf == expected_nf
    ));
    let joint_ovk = joint_fvk.to_ovk(Scope::External);
    let leaked = bundle.actions().iter().any(|act| {
        try_output_recovery_with_ovk(&IronwoodDomain::for_action(act), &joint_ovk, act, act.cv_net(), &act.encrypted_note().out_ciphertext).is_some()
    });
    cx.note(&format!("     joint OVK can recover the sweep output (ovk = None used)? {leaked}"));
    let joint_sees = bundle.actions().iter().any(|act| try_note_decryption(&IronwoodDomain::for_action(act), &ivk, act).is_some());
    cx.note(&format!("     joint IVK can decrypt the sweep output? {joint_sees}"));

    cx.note("\nSUCCESS: joint-key Ironwood note spent by a v6 tx built via PCZT, externally signed with (a+b)+alpha, accepted and mined.");
    Ok(())
}
