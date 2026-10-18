//! scan: fetch compact blocks from a lightwalletd/Zaino server and write per-tx summaries.
//!
//!   scan --server https://zec.rocks:443 --start H --end H --out data/mainnet
//!   scan --server https://zec.rocks:443 --info          (server info + tip only)
//!
//! Writes (appending, resumable) <out>.blocks.csv and <out>.txs.csv:
//!   blocks: height,time,n_tx
//!   txs:    height,index,iw,orch,sap_in,sap_out,t_in,t_out,fee,txid
//! `txid` (display order hex) is only written for txs with Ironwood actions, to keep the file small.
//! Requests ranges of at most 1000 blocks and pauses between them (politeness).

use anyhow::{Context, Result};
use std::io::{BufRead, Write};
use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig};
use zcash_client_backend::proto::service::{
    compact_tx_streamer_client::CompactTxStreamerClient, BlockId, BlockRange, ChainSpec, Empty, PoolType,
};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn last_height(path: &str) -> Option<u64> {
    let f = std::fs::File::open(path).ok()?;
    let mut last = None;
    for l in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        if let Some(h) = l.split(',').next().and_then(|s| s.parse::<u64>().ok()) {
            last = Some(h);
        }
    }
    last
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let server = arg(&args, "--server").unwrap_or("https://zec.rocks:443".into());
    let chunk: u64 = arg(&args, "--chunk").unwrap_or("1000".into()).parse()?;
    let pause_ms: u64 = arg(&args, "--pause-ms").unwrap_or("400".into()).parse()?;

    let ch = Channel::from_shared(server.clone())?
        .tls_config(ClientTlsConfig::new().with_webpki_roots())?
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(20))
        .connect()
        .await
        .with_context(|| format!("connect {server}"))?;
    let mut c = CompactTxStreamerClient::new(ch).max_decoding_message_size(64 << 20);
    let info = c.get_lightd_info(Empty {}).await?.into_inner();
    let tip = c.get_latest_block(ChainSpec {}).await?.into_inner().height;
    eprintln!(
        "server {server}: vendor={} version={} chain={} block_height={} branch={} sapling_act={} tip={tip}",
        info.vendor, info.version, info.chain_name, info.block_height, info.consensus_branch_id, info.sapling_activation_height
    );
    if args.iter().any(|a| a == "--info") {
        println!("{tip}");
        return Ok(());
    }
    let out = arg(&args, "--out").context("--out")?;
    let end: u64 = arg(&args, "--end").map(|s| s.parse()).transpose()?.unwrap_or(tip).min(tip);
    let mut start: u64 = arg(&args, "--start").context("--start")?.parse()?;
    let bpath = format!("{out}.blocks.csv");
    let tpath = format!("{out}.txs.csv");
    if let Some(h) = last_height(&bpath) {
        if h + 1 > start {
            eprintln!("resuming after height {h}");
            start = h + 1;
        }
    }
    let new = !std::path::Path::new(&bpath).exists();
    let mut bf = std::io::BufWriter::new(std::fs::OpenOptions::new().create(true).append(true).open(&bpath)?);
    let mut tf = std::io::BufWriter::new(std::fs::OpenOptions::new().create(true).append(true).open(&tpath)?);
    if new {
        writeln!(bf, "height,time,n_tx")?;
        writeln!(tf, "height,index,iw,orch,sap_in,sap_out,t_in,t_out,fee,txid")?;
    }

    let mut lo = start;
    while lo <= end {
        let hi = (lo + chunk - 1).min(end);
        let mut tries = 0;
        loop {
            let res: Result<Vec<_>> = async {
                let mut stream = c
                    .get_block_range(BlockRange {
                        start: Some(BlockId { height: lo, hash: vec![] }),
                        end: Some(BlockId { height: hi, hash: vec![] }),
                        pool_types: vec![
                            PoolType::Transparent as i32,
                            PoolType::Sapling as i32,
                            PoolType::Orchard as i32,
                            PoolType::Ironwood as i32,
                        ],
                    })
                    .await?
                    .into_inner();
                let mut v = vec![];
                while let Some(b) = stream.message().await? {
                    v.push(b);
                }
                Ok(v)
            }
            .await;
            match res {
                Ok(blocks) => {
                    let mut expect = lo;
                    for b in &blocks {
                        if b.height != expect {
                            anyhow::bail!("gap: expected {expect} got {}", b.height);
                        }
                        expect += 1;
                        writeln!(bf, "{},{},{}", b.height, b.time, b.vtx.len())?;
                        for tx in &b.vtx {
                            let txid = if tx.ironwood_actions.is_empty() {
                                String::new()
                            } else {
                                let mut t = tx.txid.clone();
                                t.reverse();
                                hex::encode(t)
                            };
                            writeln!(
                                tf,
                                "{},{},{},{},{},{},{},{},{},{}",
                                b.height,
                                tx.index,
                                tx.ironwood_actions.len(),
                                tx.actions.len(),
                                tx.spends.len(),
                                tx.outputs.len(),
                                tx.vin.len(),
                                tx.vout.len(),
                                tx.fee,
                                txid
                            )?;
                        }
                    }
                    if expect != hi + 1 {
                        anyhow::bail!("short range {lo}..={hi}: got up to {}", expect - 1);
                    }
                    bf.flush()?;
                    tf.flush()?;
                    eprintln!("{lo}..={hi} ok ({} blocks)", blocks.len());
                    break;
                }
                Err(e) => {
                    tries += 1;
                    eprintln!("{lo}..={hi} error (try {tries}): {e:#}");
                    if tries >= 5 {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_secs(5 * tries)).await;
                }
            }
        }
        lo = hi + 1;
        tokio::time::sleep(Duration::from_millis(pause_ms)).await;
    }
    Ok(())
}
