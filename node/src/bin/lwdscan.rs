//! lwdscan: scan recent compact blocks from a lightwalletd/Zaino server and report the
//! Ironwood balance of one or more Cloakroom wallet directories (testnet).
//!
//!   lwdscan --server https://testnet.zec.rocks:443 --blocks 4000 DIR [DIR...]

use anyhow::{anyhow, Result};
use orchard::{
    keys::{FullViewingKey, PreparedIncomingViewingKey, Scope, SpendingKey},
    note_encryption::{CompactAction, IronwoodDomain},
};
use std::collections::HashSet;
use tonic::transport::{Channel, ClientTlsConfig};
use zcash_client_backend::proto::service::{
    compact_tx_streamer_client::CompactTxStreamerClient, BlockId, BlockRange, ChainSpec, Empty, PoolType,
};
use zcash_note_encryption::try_compact_note_decryption;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

struct W {
    dir: String,
    fvk: FullViewingKey,
    ivks: Vec<PreparedIncomingViewingKey>,
    notes: Vec<(u64, String, u64, [u8; 32])>, // height, txid, value, nf
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let server = arg(&args, "--server").unwrap_or("https://testnet.zec.rocks:443".into());
    let blocks: u64 = arg(&args, "--blocks").unwrap_or("4000".into()).parse()?;
    let dirs: Vec<String> = args.iter().skip(1).filter(|a| !a.starts_with("--") && !args.iter().any(|b| b.starts_with("--") && arg(&args, b).as_ref() == Some(a))).cloned().collect();

    let mut ws = vec![];
    for d in &dirs {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{d}/wallet.json"))?)?;
        let b: [u8; 32] = hex::decode(v["sk"].as_str().unwrap())?.try_into().map_err(|_| anyhow!("sk"))?;
        let sk = Option::<SpendingKey>::from(SpendingKey::from_bytes(b)).ok_or_else(|| anyhow!("sk"))?;
        let fvk = FullViewingKey::from(&sk);
        let ivks = [Scope::External, Scope::Internal].iter().map(|s| PreparedIncomingViewingKey::new(&fvk.to_ivk(*s))).collect();
        ws.push(W { dir: d.clone(), fvk, ivks, notes: vec![] });
    }

    let ch = Channel::from_shared(server.clone())?.tls_config(ClientTlsConfig::new().with_webpki_roots())?.connect().await?;
    let mut c = CompactTxStreamerClient::new(ch);
    let info = c.get_lightd_info(Empty {}).await?.into_inner();
    println!("server {server}: {} {} chain={} block_height={} branch={}", info.vendor, info.version, info.chain_name, info.block_height, info.consensus_branch_id);
    let tip = c.get_latest_block(ChainSpec {}).await?.into_inner().height;
    let start = tip.saturating_sub(blocks);
    println!("scanning compact blocks {start}..={tip} (Ironwood actions)");

    let mut nfs: HashSet<[u8; 32]> = HashSet::new();
    let (mut n_blocks, mut n_actions) = (0u64, 0u64);
    let mut lo = start;
    while lo <= tip {
        let hi = (lo + 999).min(tip);
        let mut stream = c
            .get_block_range(BlockRange {
                start: Some(BlockId { height: lo, hash: vec![] }),
                end: Some(BlockId { height: hi, hash: vec![] }),
                pool_types: vec![PoolType::Orchard as i32, PoolType::Ironwood as i32],
            })
            .await?
            .into_inner();
        while let Some(block) = stream.message().await? {
            n_blocks += 1;
            for tx in &block.vtx {
                let mut txid = tx.txid.clone();
                txid.reverse();
                for a in &tx.ironwood_actions {
                    n_actions += 1;
                    if let Ok(nf) = a.nf() {
                        nfs.insert(nf.to_bytes());
                    }
                    let Ok(ca) = CompactAction::try_from(a) else { continue };
                    let domain = IronwoodDomain::for_compact_action(&ca);
                    for w in &mut ws {
                        for ivk in &w.ivks {
                            if let Some((note, _)) = try_compact_note_decryption(&domain, ivk, &ca) {
                                let nf = note.nullifier(&w.fvk).to_bytes();
                                w.notes.push((block.height, hex::encode(&txid), note.value().inner(), nf));
                            }
                        }
                    }
                }
            }
        }
        lo = hi + 1;
    }
    println!("scanned {n_blocks} blocks, {n_actions} Ironwood actions");
    for w in &ws {
        let mut bal = 0u64;
        for (h, txid, v, nf) in &w.notes {
            let spent = nfs.contains(nf);
            if !spent {
                bal += v;
            }
            println!("  {}: note {v} zats at height {h} tx {txid}{}", w.dir, if spent { " (spent)" } else { "" });
        }
        println!("{}: unspent Ironwood balance {} zats ({} TAZ)", w.dir, bal, bal as f64 / 1e8);
    }
    Ok(())
}
