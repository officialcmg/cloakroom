//! A minimal Ironwood light wallet over Zebra JSON-RPC.
//!
//! It scans every block, keeps the full Ironwood note commitment tree (fine for regtest
//! and small test chains), trial-decrypts with any number of viewing keys (the wallet's
//! own and per-swap joint keys), and builds v6 transactions through the PCZT roles.

use anyhow::{anyhow, bail, Context, Result};
use incrementalmerkletree::{Position, Retention};
use orchard::{
    circuit::{OrchardCircuitVersion, ProvingKey, VerifyingKey},
    keys::{FullViewingKey, OutgoingViewingKey, PreparedIncomingViewingKey, Scope, SpendAuthorizingKey},
    note_encryption::IronwoodDomain,
    tree::MerkleHashOrchard,
    Address, Note,
};
use pasta_curves::pallas;
use pczt::roles::{
    creator::Creator, io_finalizer::IoFinalizer, low_level_signer, prover::Prover, signer::Signer,
    tx_extractor::TransactionExtractor,
};
use serde_json::{json, Value};
use shardtree::{store::memory::MemoryShardStore, ShardTree};
use std::{collections::HashSet, convert::Infallible, sync::OnceLock};
use zcash_address::unified::{self, Container, Encoding};
use zcash_note_encryption::try_note_decryption;
use zcash_primitives::transaction::{
    builder::{BuildConfig, Builder, BundlePadding},
    fees::zip317,
    Transaction,
};
use incrementalmerkletree::frontier::Frontier;
use tonic::transport::{Channel, ClientTlsConfig};
use zcash_client_backend::proto::service::{
    compact_tx_streamer_client::CompactTxStreamerClient, BlockId, BlockRange, ChainSpec, PoolType, RawTransaction, TxFilter,
};
use zcash_note_encryption::try_compact_note_decryption;
use zcash_protocol::{
    consensus::{BlockHeight, BranchId, NetworkType, NetworkUpgrade, Parameters, TEST_NETWORK},
    local_consensus::LocalNetwork,
    memo::MemoBytes,
    value::Zatoshis,
};

use crate::{keys, OsRng};

pub const MARGINAL_FEE: u64 = 5_000;
pub const GRACE_ACTIONS: u64 = 2;

pub fn zip317_fee(spends: usize, outputs: usize) -> u64 {
    MARGINAL_FEE * GRACE_ACTIONS.max(spends.max(outputs) as u64)
}

// ---------------------------------------------------------------- RPC

pub struct Rpc {
    http: reqwest::blocking::Client,
    url: String,
}

impl Rpc {
    pub fn new(url: &str) -> Self {
        Rpc { http: reqwest::blocking::Client::new(), url: url.to_string() }
    }
    pub fn call_raw(&self, method: &str, params: Value) -> Result<std::result::Result<Value, Value>> {
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let resp: Value = self
            .http
            .post(&self.url)
            .json(&req)
            .send()
            .with_context(|| format!("zebra rpc {method}"))?
            .json()?;
        if let Some(e) = resp.get("error").filter(|e| !e.is_null()) {
            return Ok(Err(e.clone()));
        }
        Ok(Ok(resp["result"].clone()))
    }
    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.call_raw(method, params)?.map_err(|e| anyhow!("zebra rpc {method}: {e}"))
    }
}

// ---------------------------------------------------------------- network + addresses

pub fn regtest_params(nu7: Option<u32>) -> LocalNetwork {
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
        nu7: nu7.map(BlockHeight::from_u32),
    }
}

/// Network used for address encoding: regtest unless set to testnet/mainnet at startup.
pub static NET: OnceLock<NetworkType> = OnceLock::new();

pub fn set_network(name: &str) -> Result<()> {
    let n = match name {
        "regtest" => NetworkType::Regtest,
        "test" | "testnet" => NetworkType::Test,
        "main" | "mainnet" => NetworkType::Main,
        _ => bail!("unknown network {name}"),
    };
    let _ = NET.set(n);
    Ok(())
}

pub fn encode_ua(addr: &Address) -> String {
    unified::Address::try_from_items(
        zcash_protocol::address::Revision::R0,
        vec![unified::Uitem::Data(unified::Receiver::Orchard(addr.to_raw_address_bytes()))],
    )
    .expect("valid UA")
    .encode(NET.get().unwrap_or(&NetworkType::Regtest))
}

pub fn decode_ua(s: &str) -> Result<Address> {
    let (_net, _rev, ua) = unified::Address::decode(s).map_err(|e| anyhow!("bad UA: {e:?}"))?;
    for item in ua.items() {
        if let unified::Receiver::Orchard(raw) = item {
            return Option::from(Address::from_raw_address_bytes(&raw)).ok_or_else(|| anyhow!("bad Orchard receiver"));
        }
    }
    bail!("UA has no Orchard-protocol receiver")
}

pub fn addr_from_raw(raw: &[u8; 43]) -> Result<Address> {
    Option::from(Address::from_raw_address_bytes(raw)).ok_or_else(|| anyhow!("bad raw address"))
}

/// Consensus parameters: a local regtest chain or the public testnet.
#[derive(Clone, Copy, Debug)]
pub enum Net {
    Local(LocalNetwork),
    Test,
}

impl Parameters for Net {
    fn network_type(&self) -> NetworkType {
        match self {
            Net::Local(p) => p.network_type(),
            Net::Test => NetworkType::Test,
        }
    }
    fn activation_height(&self, nu: NetworkUpgrade) -> Option<BlockHeight> {
        match self {
            Net::Local(p) => p.activation_height(nu),
            Net::Test => TEST_NETWORK.activation_height(nu),
        }
    }
}

/// lightwalletd / Zaino gRPC client (compact blocks), used on public networks.
pub struct Lwd {
    rt: tokio::runtime::Runtime,
    client: CompactTxStreamerClient<Channel>,
    pub birthday: u64,
}

impl Lwd {
    pub fn connect(url: &str, birthday: u64) -> Result<Lwd> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let rt = tokio::runtime::Runtime::new()?;
        let client = rt.block_on(async {
            // Also trust the roots in SSL_CERT_FILE, if set, for networks whose egress re-signs TLS.
            let mut tls = ClientTlsConfig::new().with_webpki_roots();
            if let Ok(path) = std::env::var("SSL_CERT_FILE") {
                if let Ok(pem) = std::fs::read(&path) {
                    tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(pem));
                }
            }
            let ch = Channel::from_shared(url.to_string())?
                .tls_config(tls)?
                .connect()
                .await?;
            Ok::<_, anyhow::Error>(CompactTxStreamerClient::new(ch))
        })?;
        Ok(Lwd { rt, client, birthday })
    }
    fn tip(&mut self) -> Result<u64> {
        let c = &mut self.client;
        Ok(self.rt.block_on(c.get_latest_block(ChainSpec {}))?.into_inner().height)
    }
    fn tree_before(&mut self, height: u64) -> Result<Frontier<MerkleHashOrchard, 32>> {
        let c = &mut self.client;
        let ts = self.rt.block_on(c.get_tree_state(BlockId { height, hash: vec![] }))?.into_inner();
        Ok(ts.ironwood_tree()?.to_frontier())
    }
    fn blocks(&mut self, lo: u64, hi: u64) -> Result<Vec<zcash_client_backend::proto::compact_formats::CompactBlock>> {
        let c = &mut self.client;
        self.rt.block_on(async {
            let mut st = c
                .get_block_range(BlockRange {
                    start: Some(BlockId { height: lo, hash: vec![] }),
                    end: Some(BlockId { height: hi, hash: vec![] }),
                    pool_types: vec![PoolType::Ironwood as i32],
                })
                .await?
                .into_inner();
            let mut v = vec![];
            while let Some(b) = st.message().await? {
                v.push(b);
            }
            Ok(v)
        })
    }
    fn raw_tx(&mut self, txid_internal: &[u8]) -> Result<Vec<u8>> {
        let c = &mut self.client;
        Ok(self.rt.block_on(c.get_transaction(TxFilter { block: None, index: 0, hash: txid_internal.to_vec() }))?.into_inner().data)
    }
    fn send(&mut self, data: Vec<u8>) -> Result<()> {
        let c = &mut self.client;
        let r = self.rt.block_on(c.send_transaction(RawTransaction { data, height: 0 }))?.into_inner();
        if r.error_code != 0 {
            bail!("lightwalletd rejected tx: {} {}", r.error_code, r.error_message);
        }
        Ok(())
    }
}

pub enum Backend {
    Zebra(Rpc),
    Lwd(Lwd),
}

// ---------------------------------------------------------------- circuit keys

static PK: OnceLock<ProvingKey> = OnceLock::new();
static VK: OnceLock<VerifyingKey> = OnceLock::new();
fn pk() -> &'static ProvingKey {
    PK.get_or_init(|| ProvingKey::build(OrchardCircuitVersion::PostNu6_3))
}
fn vk() -> &'static VerifyingKey {
    VK.get_or_init(|| VerifyingKey::build(OrchardCircuitVersion::PostNu6_3))
}

// ---------------------------------------------------------------- scanning

struct KeyEntry {
    label: String,
    fvk: FullViewingKey,
    ivks: Vec<(Scope, PreparedIncomingViewingKey)>,
}

#[derive(Clone, Debug)]
pub struct Received {
    pub label: String,
    pub note: Note,
    pub pos: u64,
    pub height: u64,
    pub txid: String,
    pub memo: [u8; 512],
    pub nf: [u8; 32],
    pub spent_in: Option<String>,
}

pub enum SignWith {
    Key(SpendAuthorizingKey),
    /// Joint key: ask = a + b (already normalized).
    Joint(pallas::Scalar),
}

pub struct Wallet {
    pub backend: Backend,
    pub params: Net,
    /// Ironwood tree frontier just before the first scanned block (light-wallet mode).
    frontier: Frontier<MerkleHashOrchard, 32>,
    /// Global position of leaves[0].
    leaf_offset: u64,
    keys: Vec<KeyEntry>,
    leaves: Vec<MerkleHashOrchard>,
    pub received: Vec<Received>,
    /// nullifier -> (spending txid, block height)
    spent: std::collections::HashMap<[u8; 32], (String, u64)>,
    pub height: u64,
    /// Per block: number of "look-alike" transactions, i.e. fully shielded, Ironwood-only,
    /// exactly 2 actions -- the same shape as every Cloakroom transaction.
    pub cover: std::collections::BTreeMap<u64, u32>,
}

/// Cover-traffic policy for a broadcast: wait U[min_secs, max_secs] AND until at least `k`
/// look-alike transactions have been mined since the trigger, but never past `deadline`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CoverPolicy {
    pub min_secs: u64,
    pub max_secs: u64,
    pub k: u64,
}

impl Wallet {
    pub fn new(backend: Backend, params: Net) -> Self {
        Wallet { backend, params, frontier: Frontier::empty(), leaf_offset: 0, keys: vec![], leaves: vec![], received: vec![], spent: Default::default(), height: 0, cover: Default::default() }
    }

    /// Look-alike transactions mined in blocks (after, tip].
    pub fn lookalikes_since(&self, after: u64) -> u64 {
        self.cover.range(after + 1..).map(|(_, n)| *n as u64).sum()
    }

    /// Cover delay before a broadcast that follows a public trigger ("redraw" policy). Wait a fresh
    /// U[min_secs, max_secs]; if fewer than `k` look-alikes have been mined since the trigger when
    /// the wait ends, draw a new U and wait again. The broadcast therefore always happens at the
    /// end of a random wait, never right after the k-th look-alike: snapping to the k-th arrival
    /// lets a policy-aware observer guess the transaction with median probability about 1/3
    /// (measurements/cover_posterior.py). Never waits past `deadline_unix`.
    pub fn wait_for_cover(&mut self, policy: CoverPolicy, trigger_height: u64, deadline_unix: Option<u64>, now_unix: &dyn Fn() -> u64) -> String {
        if policy.max_secs == 0 && policy.k == 0 {
            return "no cover delay configured".into();
        }
        let span = policy.max_secs.saturating_sub(policy.min_secs);
        let draw = || policy.min_secs + if span == 0 { 0 } else { (rand::Rng::next_u64(&mut { crate::OsRng })) % (span + 1) };
        let start = now_unix();
        let (mut wait_from, mut delay, mut draws) = (start, draw(), 1u32);
        loop {
            let _ = self.sync();
            let now = now_unix();
            let seen = self.lookalikes_since(trigger_height);
            if let Some(d) = deadline_unix {
                if now >= d {
                    return format!("cover cut short by the protocol deadline after {}s ({draws} draw(s)): {seen} of {} look-alikes", now - start, policy.k);
                }
            }
            if now >= wait_from + delay {
                if seen >= policy.k {
                    return format!("cover: waited {}s over {draws} random draw(s), {seen} look-alike txs mined since height {trigger_height}", now - start);
                }
                // not enough cover yet: redraw instead of waiting for the k-th arrival
                wait_from = now;
                delay = draw().max(1);
                draws += 1;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }

    /// Track a viewing key. Adding a key resets the scan so its history is found.
    pub fn watch(&mut self, label: &str, fvk: &FullViewingKey, scopes: &[Scope]) {
        if self.keys.iter().any(|k| k.label == label) {
            return;
        }
        let ivks = scopes.iter().map(|s| (*s, PreparedIncomingViewingKey::new(&fvk.to_ivk(*s)))).collect();
        self.keys.push(KeyEntry { label: label.to_string(), fvk: fvk.clone(), ivks });
        self.leaves.clear();
        self.received.clear();
        self.spent.clear();
        self.height = 0;
        self.frontier = Frontier::empty();
        self.leaf_offset = 0;
        self.cover.clear();
    }

    pub fn tip(&mut self) -> Result<u64> {
        match &mut self.backend {
            Backend::Zebra(rpc) => Ok(rpc.call("getblockcount", json!([]))?.as_u64().unwrap_or(0)),
            Backend::Lwd(l) => l.tip(),
        }
    }

    fn broadcast(&mut self, bytes: Vec<u8>) -> Result<()> {
        match &mut self.backend {
            Backend::Zebra(rpc) => rpc.call("sendrawtransaction", json!([hex::encode(&bytes)])).map(|_| ()),
            Backend::Lwd(l) => l.send(bytes),
        }
    }

    pub fn sync(&mut self) -> Result<()> {
        if matches!(self.backend, Backend::Lwd(_)) {
            return self.sync_lwd();
        }
        let tip = self.tip()?;
        let Backend::Zebra(rpc) = &self.backend else { unreachable!() };
        while self.height < tip {
            let h = self.height + 1;
            let blk = rpc.call("getblock", json!([h.to_string(), 1]))?;
            let branch = BranchId::for_height(&self.params, BlockHeight::from_u32(h as u32));
            for txid in blk["tx"].as_array().cloned().unwrap_or_default() {
                let txid = txid.as_str().unwrap_or_default().to_string();
                let raw = rpc.call("getrawtransaction", json!([txid, 0]))?;
                let bytes = hex::decode(raw.as_str().unwrap_or_default())?;
                let tx = Transaction::read(&bytes[..], branch).with_context(|| format!("parse tx {txid} at {h}"))?;
                let lookalike = tx.transparent_bundle().is_none()
                    && tx.sapling_bundle().is_none()
                    && tx.orchard_bundle().is_none()
                    && tx.ironwood_bundle().map(|b| b.actions().len() == 2).unwrap_or(false);
                if lookalike {
                    *self.cover.entry(h).or_default() += 1;
                }
                if let Some(bundle) = tx.ironwood_bundle() {
                    for act in bundle.actions() {
                        self.spent.insert(act.nullifier().to_bytes(), (txid.clone(), h));
                        let pos = self.leaves.len() as u64;
                        self.leaves.push(MerkleHashOrchard::from_cmx(act.cmx()));
                        for k in &self.keys {
                            for (_scope, ivk) in &k.ivks {
                                if let Some((note, _addr, memo)) =
                                    try_note_decryption(&IronwoodDomain::for_action(act), ivk, act)
                                {
                                    let nf = note.nullifier(&k.fvk).to_bytes();
                                    self.received.push(Received {
                                        label: k.label.clone(),
                                        note,
                                        pos,
                                        height: h,
                                        txid: txid.clone(),
                                        memo,
                                        nf,
                                        spent_in: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            self.height = h;
        }
        for r in &mut self.received {
            r.spent_in = self.spent.get(&r.nf).map(|(t, _)| t.clone());
        }
        Ok(())
    }

    /// Light-wallet sync: start from the server's Ironwood tree state just before the
    /// birthday, scan compact blocks, and fetch full transactions only for our own notes
    /// (compact blocks carry no memo).
    fn sync_lwd(&mut self) -> Result<()> {
        let Backend::Lwd(l) = &mut self.backend else { unreachable!() };
        if self.height == 0 {
            let b = l.birthday;
            self.frontier = l.tree_before(b - 1)?;
            self.leaf_offset = self.frontier.value().map(|f| u64::from(f.position()) + 1).unwrap_or(0);
            self.height = b - 1;
        }
        let tip = l.tip()?;
        while self.height < tip {
            let lo = self.height + 1;
            let hi = (lo + 999).min(tip);
            let blocks = l.blocks(lo, hi)?;
            for blk in &blocks {
                let branch = BranchId::for_height(&self.params, BlockHeight::from_u32(blk.height as u32));
                for ctx in &blk.vtx {
                    if ctx.vin.is_empty() && ctx.vout.is_empty() && ctx.spends.is_empty() && ctx.outputs.is_empty()
                        && ctx.actions.is_empty() && ctx.ironwood_actions.len() == 2
                    {
                        *self.cover.entry(blk.height).or_default() += 1;
                    }
                    let mut disp = ctx.txid.clone();
                    disp.reverse();
                    let txid = hex::encode(&disp);
                    let mut full: Option<Transaction> = None;
                    for ca in &ctx.ironwood_actions {
                        let nf = ca.nf().map_err(|e| anyhow!("compact nf: {e:?}"))?.to_bytes();
                        self.spent.insert(nf, (txid.clone(), blk.height));
                        let cmx = ca.cmx().map_err(|e| anyhow!("compact cmx: {e:?}"))?;
                        let pos = self.leaf_offset + self.leaves.len() as u64;
                        self.leaves.push(MerkleHashOrchard::from_cmx(&cmx));
                        let Ok(compact) = orchard::note_encryption::CompactAction::try_from(ca) else { continue };
                        let cdomain = IronwoodDomain::for_compact_action(&compact);
                        for k in &self.keys {
                            for (_scope, ivk) in &k.ivks {
                                if try_compact_note_decryption(&cdomain, ivk, &compact).is_none() {
                                    continue;
                                }
                                // ours: fetch the full transaction to recover the memo
                                if full.is_none() {
                                    let raw = l.raw_tx(&ctx.txid)?;
                                    full = Some(Transaction::read(&raw[..], branch).with_context(|| format!("parse {txid}"))?);
                                }
                                let tx = full.as_ref().unwrap();
                                let bundle = tx.ironwood_bundle().ok_or_else(|| anyhow!("no ironwood bundle in {txid}"))?;
                                let act = bundle.actions().iter().find(|a| a.cmx().to_bytes() == cmx.to_bytes()).ok_or_else(|| anyhow!("action not found"))?;
                                if let Some((note, _addr, memo)) = try_note_decryption(&IronwoodDomain::for_action(act), ivk, act) {
                                    let nf = note.nullifier(&k.fvk).to_bytes();
                                    self.received.push(Received { label: k.label.clone(), note, pos, height: blk.height, txid: txid.clone(), memo, nf, spent_in: None });
                                }
                            }
                        }
                    }
                }
            }
            self.height = hi;
        }
        for r in &mut self.received {
            r.spent_in = self.spent.get(&r.nf).map(|(t, _)| t.clone());
        }
        Ok(())
    }

    /// (txid, height) of the transaction that spent the first note received under `label`.
    pub fn spend_of(&self, label: &str) -> Option<(String, u64)> {
        self.received.iter().find(|r| r.label == label).and_then(|r| self.spent.get(&r.nf).cloned())
    }

    pub fn unspent(&self, label: &str) -> Vec<Received> {
        self.received.iter().filter(|r| r.label == label && r.spent_in.is_none()).cloned().collect()
    }

    pub fn balance(&self, label: &str) -> u64 {
        self.unspent(label).iter().map(|r| r.note.value().inner()).sum()
    }

    fn fvk_of(&self, label: &str) -> Result<FullViewingKey> {
        self.keys.iter().find(|k| k.label == label).map(|k| k.fvk.clone()).ok_or_else(|| anyhow!("unknown key {label}"))
    }

    /// Build, prove, sign and broadcast a v6 transaction. Returns the txid.
    pub fn send(
        &mut self,
        spends: Vec<(Received, SignWith)>,
        outputs: Vec<(Address, u64, [u8; 512], Option<OutgoingViewingKey>)>,
    ) -> Result<String> {
        self.sync()?;
        let tip = self.height;
        // Tree with witnesses for the spent notes, anchored at the current tip.
        let mut tree: ShardTree<MemoryShardStore<MerkleHashOrchard, u32>, 32, 16> =
            ShardTree::new(MemoryShardStore::empty(), 100);
        let marked: HashSet<u64> = spends.iter().map(|(r, _)| r.pos).collect();
        if self.frontier.value().is_some() {
            tree.insert_frontier(self.frontier.clone(), Retention::Ephemeral).map_err(|e| anyhow!("insert frontier: {e:?}"))?;
        }
        for (i, leaf) in self.leaves.iter().enumerate() {
            let pos = self.leaf_offset + i as u64;
            let ret = if marked.contains(&pos) { Retention::Marked } else { Retention::Ephemeral };
            tree.append(*leaf, ret).map_err(|e| anyhow!("tree append: {e:?}"))?;
        }
        tree.checkpoint(0).map_err(|e| anyhow!("{e:?}"))?;
        let root = tree.root_at_checkpoint_id(&0).map_err(|e| anyhow!("{e:?}"))?.ok_or_else(|| anyhow!("empty tree"))?;
        let anchor: orchard::Anchor = root.into();

        let target = BlockHeight::from_u32((tip + 1) as u32);
        let mut b = Builder::new(
            self.params,
            target,
            BuildConfig::Standard {
                sapling_anchor: None,
                orchard_anchor: None,
                ironwood_anchor: Some(anchor),
                orchard_padding: BundlePadding::DEFAULT,
                ironwood_padding: BundlePadding::DEFAULT,
            },
        );
        for (r, _) in &spends {
            let path = tree
                .witness_at_checkpoint_id(Position::from(r.pos), &0)
                .map_err(|e| anyhow!("{e:?}"))?
                .ok_or_else(|| anyhow!("no witness for position {}", r.pos))?;
            let fvk = self.fvk_of(&r.label)?;
            b.add_ironwood_spend::<Infallible>(fvk, r.note.clone(), path.into())
                .map_err(|e| anyhow!("add_ironwood_spend: {e:?}"))?;
        }
        for (to, value, memo, ovk) in &outputs {
            b.add_ironwood_output::<Infallible>(ovk.clone(), *to, Zatoshis::from_u64(*value).unwrap(), MemoBytes::from_bytes(memo).unwrap())
                .map_err(|e| anyhow!("add_ironwood_output: {e:?}"))?;
        }
        let res = b
            .build_for_pczt(OsRng, &zip317::FeeRule::standard())
            .map_err(|e| anyhow!("build_for_pczt: {e:?}"))?;
        let spend_idx: Vec<usize> = (0..spends.len())
            .map(|i| res.ironwood_meta.spend_action_index(i).ok_or_else(|| anyhow!("spend index")))
            .collect::<Result<_>>()?;

        let pczt = Creator::build_from_parts(res.pczt_parts).ok_or_else(|| anyhow!("Creator failed"))?;
        let pczt = IoFinalizer::new(pczt).finalize_io(OsRng).map_err(|e| anyhow!("IoFinalizer: {e:?}"))?;
        let pczt = Prover::new(pczt).create_ironwood_proof(OsRng, pk()).map_err(|e| anyhow!("Prover: {e:?}"))?.finish();

        // Read the spend randomizers (alpha) for external signing.
        let mut alphas: Vec<Option<pallas::Scalar>> = vec![None; spend_idx.len()];
        let pczt = low_level_signer::Signer::new(pczt)
            .sign_ironwood_with(|_p, bundle, _m| -> std::result::Result<(), LlErr> {
                for (j, idx) in spend_idx.iter().enumerate() {
                    alphas[j] = *bundle.actions()[*idx].spend().alpha();
                }
                Ok(())
            })
            .map_err(|e| anyhow!("low-level signer: {e:?}"))?
            .finish();

        let mut signer = Signer::new(pczt).map_err(|e| anyhow!("Signer: {e:?}"))?;
        let sighash = signer.shielded_sighash();
        for (j, (_, how)) in spends.iter().enumerate() {
            match how {
                SignWith::Key(ask) => signer.sign_ironwood(OsRng, spend_idx[j], ask).map_err(|e| anyhow!("sign: {e:?}"))?,
                SignWith::Joint(ask) => {
                    let alpha = alphas[j].ok_or_else(|| anyhow!("alpha missing"))?;
                    signer
                        .apply_ironwood_signature(spend_idx[j], keys::sign_spend(ask, &alpha, &sighash))
                        .map_err(|e| anyhow!("joint signature rejected: {e:?}"))?
                }
            }
        }
        let pczt = signer.finish();
        let tx = TransactionExtractor::new(pczt).with_orchard(vk()).extract(OsRng).map_err(|e| anyhow!("extract: {e:?}"))?;
        let mut bytes = vec![];
        tx.write(&mut bytes)?;
        let txid = tx.txid().to_string();
        self.broadcast(bytes)?;
        Ok(txid)
    }

    /// Pay `outputs` from this wallet's own notes (label), returning change to `change`.
    pub fn pay(
        &mut self,
        label: &str,
        ask: &SpendAuthorizingKey,
        change: Address,
        ovk: &OutgoingViewingKey,
        outputs: Vec<(Address, u64, [u8; 512])>,
    ) -> Result<String> {
        self.sync()?;
        let mut notes = self.unspent(label);
        notes.sort_by_key(|r| std::cmp::Reverse(r.note.value().inner()));
        let want: u64 = outputs.iter().map(|o| o.1).sum();
        let mut chosen = vec![];
        let mut total = 0u64;
        for n in notes {
            if total >= want + zip317_fee(chosen.len() + 1, outputs.len() + 1) {
                break;
            }
            total += n.note.value().inner();
            chosen.push(n);
        }
        let fee = zip317_fee(chosen.len(), outputs.len() + 1);
        if total < want + fee {
            bail!("insufficient funds: have {total}, need {}", want + fee);
        }
        // Every Cloakroom transaction must have exactly 2 actions (the measured anonymity class):
        // Orchard pads to max(spends, outputs), so with 2 outputs at most 2 input notes are allowed.
        if chosen.len() > 2 {
            bail!("needs {} input notes; consolidate the wallet first so a swap transaction keeps the 2-action shape", chosen.len());
        }
        // ovk = None on every output: a later compromise of this wallet's viewing keys must not reveal
        // the destination or value of swap messages and funding (round-2 review M6).
        let _ = ovk;
        let mut outs: Vec<_> = outputs.into_iter().map(|(a, v, m)| (a, v, m, None)).collect();
        outs.push((change, total - want - fee, empty_memo(), None));
        let spends = chosen.into_iter().map(|r| (r, SignWith::Key(ask.clone()))).collect();
        self.send(spends, outs)
    }
}

pub fn empty_memo() -> [u8; 512] {
    let mut m = [0u8; 512];
    m[0] = 0xF6;
    m
}

#[derive(Debug)]
#[allow(dead_code)]
struct LlErr(String);
impl From<low_level_signer::OrchardParseError> for LlErr {
    fn from(e: low_level_signer::OrchardParseError) -> Self {
        LlErr(format!("{e:?}"))
    }
}
