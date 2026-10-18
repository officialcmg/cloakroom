//! txaudit: fetch transactions from a lightwalletd server and report what they reveal
//! (version, transparent inputs/outputs, Sapling/Orchard/Ironwood activity).
//!
//!   txaudit --server https://testnet.zec.rocks:443 TXID [TXID...]

use anyhow::Result;
use tonic::transport::{Channel, ClientTlsConfig};
use zcash_client_backend::proto::service::{compact_tx_streamer_client::CompactTxStreamerClient, TxFilter};
use zcash_primitives::transaction::Transaction;
use zcash_protocol::consensus::{BlockHeight, BranchId, TEST_NETWORK};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let server = args.iter().position(|a| a == "--server").map(|i| args[i + 1].clone()).unwrap_or("https://testnet.zec.rocks:443".into());
    let txids: Vec<&String> = args.iter().filter(|a| a.len() == 64).collect();
    let ch = Channel::from_shared(server)?.tls_config(ClientTlsConfig::new().with_webpki_roots())?.connect().await?;
    let mut c = CompactTxStreamerClient::new(ch);
    println!("txid | height | version | transparent in/out | sapling spends/outputs | orchard actions | ironwood actions | ironwood value balance (zats)");
    for t in txids {
        let mut h = hex::decode(t)?;
        h.reverse();
        let raw = c.get_transaction(TxFilter { block: None, index: 0, hash: h }).await?.into_inner();
        let branch = BranchId::for_height(&TEST_NETWORK, BlockHeight::from_u32(raw.height as u32));
        let tx = Transaction::read(&raw.data[..], branch)?;
        let (tin, tout) = tx.transparent_bundle().map(|b| (b.vin.len(), b.vout.len())).unwrap_or((0, 0));
        let (ss, so) = tx.sapling_bundle().map(|b| (b.shielded_spends().len(), b.shielded_outputs().len())).unwrap_or((0, 0));
        let oa = tx.orchard_bundle().map(|b| b.actions().len()).unwrap_or(0);
        let (ia, ivb) = tx.ironwood_bundle().map(|b| (b.actions().len(), i64::from(*b.value_balance()))).unwrap_or((0, 0));
        println!("{t} | {} | v{} | {tin}/{tout} | {ss}/{so} | {oa} | {ia} | {ivb}", raw.height, u32::from(tx.version().header()) & 0x7fffffff);
    }
    Ok(())
}
