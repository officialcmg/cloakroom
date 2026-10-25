//! cloakroom: trustless swaps between shielded ZEC (Ironwood) and an EVM token.
//!
//! Usage:
//!   cloakroom init    --dir D [--network test]   create a shielded wallet key, print its UA
//!   cloakroom rfqpub  --dir D            maker: print RFQ epoch and encryption public key
//!   cloakroom balance --dir D            print shielded balance (JSON)
//!   cloakroom pay     --dir D --to UA --zats N   fully shielded send
//!   cloakroom maker   --dir D [--vanish-at STAGE] [--evil MODE] [--exit-after N]
//!   cloakroom taker   --dir D --lots N [--vanish-at STAGE] [--evil MODE]
//!   cloakroom serve   --dir D [--port 7800] [--ui app/ui] [--ds design-system]   local app on 127.0.0.1
//!
//! Maker and taker communicate only through encrypted memos on Zcash and the escrow
//! contract on the EVM chain. `--vanish-at` makes the process exit at a stage so a test
//! harness can simulate a party disappearing; rerunning the same command resumes.

mod common;
mod evm;
mod keys;
mod maker;
mod msg;
mod serve;
mod taker;
mod wallet;

use anyhow::{anyhow, bail, Result};
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use std::path::PathBuf;

#[allow(non_upper_case_globals)]
pub const OsRng: UnwrapErr<SysRng> = UnwrapErr(SysRng);

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).cloned().unwrap_or_default();
    let dir = PathBuf::from(arg(&args, "--dir").ok_or_else(|| anyhow!("--dir required"))?);
    std::fs::create_dir_all(&dir)?;
    match cmd.as_str() {
        "init" => {
            if let Some(n) = arg(&args, "--network") {
                wallet::set_network(&n)?;
            }
            let me = common::Me::init(&dir)?;
            println!("{}", me.ua());
        }
        "rfqpub" => {
            // maker: print "<epoch> <hex public key> <EVM signature>" for the directory entry
            let (epoch, sk) = common::rfq_key(&dir)?;
            let cfg: common::Config = common::load(&dir.join("config.json"))?;
            if let Some(n) = &cfg.network {
                wallet::set_network(n)?;
            }
            let ua = common::Me::init(&dir)?.ua();
            let key = cfg.evm_key.ok_or_else(|| anyhow::anyhow!("maker needs evm_key"))?;
            let pk = msg::dh_public(&sk);
            let digest = keys::directory_digest(&ua, epoch, &pk, &evm::Evm::address_of(&key)?);
            println!("{epoch} {} 0x{}", hex::encode(pk), hex::encode(evm::Evm::personal_sign(&key, &digest)?));
        }
        "balance" => {
            let mut cx = common::Ctx::open(&dir, "wallet")?;
            cx.w.sync()?;
            println!(
                "{}",
                serde_json::json!({"ua": cx.me.ua(), "height": cx.w.height, "zats": cx.w.balance("wallet")})
            );
        }
        "pay" => {
            // cloakroom pay --dir D --to UA --zats N   (fully shielded send from the wallet)
            let mut cx = common::Ctx::open(&dir, "wallet")?;
            let to = wallet::decode_ua(&arg(&args, "--to").ok_or_else(|| anyhow!("--to"))?)?;
            let zats: u64 = arg(&args, "--zats").ok_or_else(|| anyhow!("--zats"))?.parse()?;
            let (change, ovk, ask) = (cx.me.change(), cx.me.ovk.clone(), cx.me.ask.clone());
            let txid = cx.w.pay("wallet", &ask, change, &ovk, vec![(to, zats, wallet::empty_memo())])?;
            println!("{txid}");
        }
        "maker" => {
            let mut cx = common::Ctx::open(&dir, "maker")?;
            maker::run(
                &mut cx,
                maker::Opts {
                    vanish_at: arg(&args, "--vanish-at"),
                    evil: arg(&args, "--evil"),
                    exit_after: arg(&args, "--exit-after").map(|s| s.parse()).transpose()?,
                },
            )?;
        }
        "taker" => {
            let mut cx = common::Ctx::open(&dir, "taker")?;
            taker::run(
                &mut cx,
                taker::Opts {
                    lots: arg(&args, "--lots").unwrap_or("1".into()).parse()?,
                    vanish_at: arg(&args, "--vanish-at"),
                    evil: arg(&args, "--evil"),
                },
            )?;
        }
        "serve" => {
            serve::run(
                &dir,
                serve::Opts {
                    port: arg(&args, "--port").unwrap_or("7800".into()).parse()?,
                    ui: PathBuf::from(arg(&args, "--ui").unwrap_or("app/ui".into())),
                    ds: PathBuf::from(arg(&args, "--ds").unwrap_or("design-system".into())),
                },
            )?;
        }
        _ => bail!("unknown command {cmd:?}; see source header for usage"),
    }
    Ok(())
}
