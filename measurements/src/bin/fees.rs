//! fees: fetch full transactions by txid (GetTransaction) and report fee and other
//! wallet-fingerprint fields that compact blocks omit.
//!
//!   fees --server https://zec.rocks:443 --branch 37a5165b --in sample.txt --out fees.csv
//!
//! Input: lines "height,txid" (txid in display/hex order). Output CSV (appending, resumable):
//!   height,txid,version,size,expiry_delta,lock_time,iw_actions,iw_vb,iw_spends_en,iw_outputs_en,
//!   orch_actions,orch_vb,sap_spends,sap_outputs,t_in,t_out,fee
//! fee is empty when the tx has transparent inputs (needs prevouts).

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig};
use zcash_client_backend::proto::service::{compact_tx_streamer_client::CompactTxStreamerClient, TxFilter};
use zcash_primitives::transaction::Transaction;
use zcash_protocol::consensus::BranchId;
use zcash_protocol::value::BalanceError;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let server = arg(&args, "--server").unwrap_or("https://zec.rocks:443".into());
    let branch = BranchId::try_from(u32::from_str_radix(&arg(&args, "--branch").unwrap_or("37a5165b".into()), 16)?)
        .map_err(|e| anyhow::anyhow!(e))?;
    let pause_ms: u64 = arg(&args, "--pause-ms").unwrap_or("300".into()).parse()?;
    let inp = arg(&args, "--in").context("--in")?;
    let out = arg(&args, "--out").context("--out")?;

    let mut done: HashSet<String> = HashSet::new();
    let exists = std::path::Path::new(&out).exists();
    if exists {
        for l in std::io::BufReader::new(std::fs::File::open(&out)?).lines().map_while(Result::ok) {
            if let Some(t) = l.split(',').nth(1) {
                done.insert(t.to_string());
            }
        }
    }
    let mut of = std::io::BufWriter::new(std::fs::OpenOptions::new().create(true).append(true).open(&out)?);
    if !exists {
        writeln!(of, "height,txid,version,size,expiry_delta,lock_time,iw_actions,iw_vb,iw_spends_en,iw_outputs_en,orch_actions,orch_vb,sap_spends,sap_outputs,t_in,t_out,fee")?;
    }

    let ch = Channel::from_shared(server.clone())?
        .tls_config(ClientTlsConfig::new().with_webpki_roots())?
        .timeout(Duration::from_secs(60))
        .connect()
        .await?;
    let mut c = CompactTxStreamerClient::new(ch).max_decoding_message_size(64 << 20);

    let lines: Vec<String> = std::io::BufReader::new(std::fs::File::open(&inp)?).lines().map_while(Result::ok).collect();
    let (mut ok, mut fail) = (0, 0);
    for l in lines {
        let mut it = l.split(',');
        let (Some(h), Some(txid)) = (it.next(), it.next()) else { continue };
        if done.contains(txid) {
            continue;
        }
        let mut hash = hex::decode(txid)?;
        hash.reverse(); // protocol order
        let mut res = None;
        for attempt in 0..3 {
            match c.get_transaction(TxFilter { block: None, index: 0, hash: hash.clone() }).await {
                Ok(r) => {
                    res = Some(r.into_inner());
                    break;
                }
                Err(e) => {
                    eprintln!("{txid}: {e} (attempt {attempt})");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
        let Some(raw) = res else {
            fail += 1;
            continue;
        };
        let tx = match Transaction::read(&raw.data[..], branch) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{txid}: parse error {e}");
                fail += 1;
                continue;
            }
        };
        if tx.txid().to_string() != txid {
            eprintln!("{txid}: txid mismatch {}", tx.txid());
        }
        let height: u64 = h.parse()?;
        let expiry = u32::from(tx.expiry_height()) as i64;
        let expiry_delta = if expiry == 0 { String::new() } else { (expiry - height as i64).to_string() };
        let (iwn, iwvb, iws, iwo) = tx.ironwood_bundle().map_or((0, 0i64, 0, 0), |b| {
            (b.actions().len(), i64::from(*b.value_balance()), b.flags().spends_enabled() as u8, b.flags().outputs_enabled() as u8)
        });
        let (on, ovb) = tx.orchard_bundle().map_or((0, 0i64), |b| (b.actions().len(), i64::from(*b.value_balance())));
        let (ss, so) = tx.sapling_bundle().map_or((0, 0), |b| (b.shielded_spends().len(), b.shielded_outputs().len()));
        let (ti, to) = tx.transparent_bundle().map_or((0, 0), |b| (b.vin.len(), b.vout.len()));
        let fee = tx
            .fee_paid(|_| Ok::<_, BalanceError>(None))
            .ok()
            .flatten()
            .map(|z| u64::from(z).to_string())
            .unwrap_or_default();
        writeln!(
            of,
            "{height},{txid},{:?},{},{expiry_delta},{},{iwn},{iwvb},{iws},{iwo},{on},{ovb},{ss},{so},{ti},{to},{fee}",
            tx.version(),
            raw.data.len(),
            tx.lock_time()
        )?;
        of.flush()?;
        ok += 1;
        if ok % 100 == 0 {
            eprintln!("{ok} ok, {fail} failed");
        }
        tokio::time::sleep(Duration::from_millis(pause_ms)).await;
    }
    eprintln!("done: {ok} ok, {fail} failed");
    Ok(())
}
