//! Configuration, persisted state, and logging shared by maker and taker.

use anyhow::{anyhow, Context, Result};
use orchard::keys::{FullViewingKey, OutgoingViewingKey, Scope, SpendAuthorizingKey, SpendingKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

use crate::{evm::Evm, wallet::{self, Wallet}, OsRng};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub zebra_rpc: String,
    pub evm_rpc: String,
    pub escrow: String,
    pub token: String,
    #[serde(default)]
    pub nu7_height: Option<u32>,
    /// "regtest" (default) or "test" for address encoding.
    #[serde(default)]
    pub network: Option<String>,
    /// lightwalletd/Zaino gRPC URL; when set, the wallet syncs compact blocks from `birthday`.
    #[serde(default)]
    pub lightwalletd: Option<String>,
    #[serde(default)]
    pub birthday: Option<u64>,
    /// First EVM block to search for escrow events (the escrow's deployment block).
    #[serde(default)]
    pub evm_from_block: Option<u64>,
    /// Cover-traffic policy applied before each Zcash broadcast that follows a public trigger.
    #[serde(default)]
    pub cover_min_secs: Option<u64>,
    #[serde(default)]
    pub cover_max_secs: Option<u64>,
    #[serde(default)]
    pub cover_k: Option<u64>,
    /// Zatoshis per lot.
    pub lot_zats: u64,
    /// Seconds of margin for R2/R3 boundary checks.
    #[serde(default = "d_small")]
    pub delta_small: u64,
    /// Taker stops attempting claims at t1 - delta_claim.
    #[serde(default = "d_claim")]
    pub delta_claim: u64,
    /// D: bound on EVM inclusion delay for an honest transaction (seconds).
    #[serde(default = "d_incl")]
    pub inclusion_secs: u64,
    /// F: EVM finality lag (seconds) -- a block older than this is final.
    #[serde(default = "d_fin")]
    pub finality_secs: u64,
    /// Extra safety margin on every share-revealing deadline (seconds).
    #[serde(default = "d_margin")]
    pub margin_secs: u64,
    /// R: number of EVM reorganizations a share-revealing transaction must survive.
    #[serde(default = "d_reorgs")]
    pub reorg_budget: u64,
    // ---- maker
    #[serde(default)]
    pub evm_key: Option<String>,
    #[serde(default)]
    pub price_per_lot: Option<u64>,
    #[serde(default)]
    pub relayer_fee: Option<u64>,
    #[serde(default)]
    pub t0_secs: Option<u32>,
    #[serde(default)]
    pub t1_secs: Option<u32>,
    /// Punish window: t2 = t1 + t2_secs. The maker must come online once in [t1, t2).
    #[serde(default)]
    pub t2_secs: Option<u32>,
    #[serde(default)]
    pub k_z: Option<u64>,
    #[serde(default)]
    pub max_lots: Option<u8>,
    /// Non-refundable premium (zatoshis) the taker pays with Accept.
    #[serde(default)]
    pub premium_zats: Option<u64>,
    /// Reveal window after commitClaim, seconds.
    #[serde(default)]
    pub reveal_secs: Option<u32>,
    // ---- taker
    #[serde(default)]
    pub maker_ua: Option<String>,
    #[serde(default)]
    pub maker_evm: Option<String>,
    #[serde(default)]
    pub relayer_key: Option<String>,
    /// Maker's published RFQ encryption key (hex) and its epoch.
    #[serde(default)]
    pub maker_rfq_pub: Option<String>,
    #[serde(default)]
    pub maker_rfq_epoch: Option<u32>,
    /// Maker's EVM signature over keys::directory_digest(maker_ua, epoch, rfq key, maker_evm).
    #[serde(default)]
    pub maker_dir_sig: Option<String>,
    /// Largest t2 - open the taker accepts (seconds; default 48 h). If the maker never sets Ready
    /// or never refunds, the taker's ZEC waits for the punish path at t2, so the maker must not
    /// choose t2 freely (final-round review C1).
    #[serde(default)]
    pub max_horizon_secs: Option<u64>,
    /// Largest premium the taker accepts.
    #[serde(default)]
    pub max_premium: Option<u64>,
    #[serde(default)]
    pub max_price: Option<u64>,
    #[serde(default)]
    pub quote_timeout_secs: Option<u64>,
}
fn d_small() -> u64 { 5 }
fn d_claim() -> u64 { 30 }
fn d_incl() -> u64 { 5 }
fn d_fin() -> u64 { 70 }
fn d_margin() -> u64 { 5 }
fn d_reorgs() -> u64 { 1 }

impl Config {
    /// A share-revealing transaction broadcast now must be FINAL before `deadline`
    /// (TLA+ finding: an included-but-unfinalized refund can be reorganized past t2).
    /// Times are block timestamps of the chain head; head lag is part of D.
    pub fn final_before(&self, now: u64, deadline: u64) -> bool {
        now + self.inclusion_secs + self.finality_secs + self.margin_secs < deadline
    }

    /// Each reorganization of a share-revealing transaction costs up to D + F before the re-sent
    /// copy is included, so surviving R reorgs needs now + (R+1)D + RF < deadline. With the
    /// final_before rule that is: margin >= R*D + (R-1)*F.
    pub fn check_margin(&self) -> Result<()> {
        let (d, f, r) = (self.inclusion_secs, self.finality_secs, self.reorg_budget);
        let need = r * d + r.saturating_sub(1) * f;
        if r == 0 || self.margin_secs < need {
            return Err(anyhow!("unsafe parameters: margin {}s must be >= R*D + (R-1)*F = {need}s for reorg budget R = {r} (R >= 1)", self.margin_secs));
        }
        Ok(())
    }
}

pub fn load<T: DeserializeOwned>(p: &Path) -> Result<T> {
    serde_json::from_str(&fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?)
        .with_context(|| format!("parse {}", p.display()))
}

pub fn save<T: Serialize>(p: &Path, v: &T) -> Result<()> {
    let tmp = p.with_extension("tmp");
    fs::write(&tmp, serde_json::to_string_pretty(v)?)?;
    fs::rename(tmp, p)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct WalletFile {
    sk: String,
}

/// The party's own shielded wallet key and its derived keys.
pub struct Me {
    pub sk: SpendingKey,
    pub fvk: FullViewingKey,
    pub ask: SpendAuthorizingKey,
    pub ovk: OutgoingViewingKey,
}

impl Me {
    pub fn init(dir: &Path) -> Result<Me> {
        let p = dir.join("wallet.json");
        if !p.exists() {
            let sk = loop {
                let mut s = [0u8; 32];
                rand::Rng::fill_bytes(&mut { OsRng }, &mut s);
                if let Some(sk) = Option::<SpendingKey>::from(SpendingKey::from_bytes(s)) {
                    break sk;
                }
            };
            save(&p, &WalletFile { sk: hex::encode(sk.to_bytes()) })?;
        }
        let w: WalletFile = load(&p)?;
        let b: [u8; 32] = hex::decode(w.sk)?.try_into().map_err(|_| anyhow!("sk len"))?;
        let sk = Option::<SpendingKey>::from(SpendingKey::from_bytes(b)).ok_or_else(|| anyhow!("bad sk"))?;
        let fvk = FullViewingKey::from(&sk);
        let ovk = fvk.to_ovk(Scope::External);
        Ok(Me { ask: SpendAuthorizingKey::from(&sk), fvk, ovk, sk })
    }
    pub fn ua(&self) -> String {
        wallet::encode_ua(&self.fvk.address_at(0u32, Scope::External))
    }
    pub fn change(&self) -> orchard::Address {
        self.fvk.address_at(0u32, Scope::Internal)
    }
}

pub struct Ctx {
    pub dir: PathBuf,
    pub cfg: Config,
    pub me: Me,
    pub w: Wallet,
    pub evm: Evm,
    pub role: &'static str,
}

impl Ctx {
    pub fn open(dir: &Path, role: &'static str) -> Result<Ctx> {
        let cfg: Config = load(&dir.join("config.json"))?;
        if let Some(n) = &cfg.network {
            wallet::set_network(n)?;
        }
        let me = Me::init(dir)?;
        let mut w = match &cfg.lightwalletd {
            Some(url) => Wallet::new(
                wallet::Backend::Lwd(wallet::Lwd::connect(url, cfg.birthday.ok_or_else(|| anyhow!("birthday required with lightwalletd"))?)?),
                wallet::Net::Test,
            ),
            None => Wallet::new(wallet::Backend::Zebra(wallet::Rpc::new(&cfg.zebra_rpc)), wallet::Net::Local(wallet::regtest_params(cfg.nu7_height))),
        };
        w.watch("wallet", &me.fvk, &[Scope::External, Scope::Internal]);
        let evm = Evm { rpc: cfg.evm_rpc.clone(), escrow: cfg.escrow.to_lowercase(), token: cfg.token.to_lowercase(), from_block: cfg.evm_from_block.unwrap_or(0) };
        fs::create_dir_all(dir.join("swaps"))?;
        Ok(Ctx { dir: dir.to_path_buf(), cfg, me, w, evm, role })
    }

    pub fn log(&self, msg: &str) {
        let base = self.evm.now().map(|t| t.to_string()).unwrap_or_else(|_| "?".into());
        println!("[{}] zec#{} base_t={} | {}", self.role, self.w.height, base, msg);
    }

    pub fn cover(&self) -> wallet::CoverPolicy {
        wallet::CoverPolicy {
            min_secs: self.cfg.cover_min_secs.unwrap_or(0),
            max_secs: self.cfg.cover_max_secs.unwrap_or(0),
            k: self.cfg.cover_k.unwrap_or(0),
        }
    }

    /// Wait for cover before a broadcast; logs the outcome.
    pub fn cover_wait(&mut self, what: &str, trigger_height: u64, deadline: Option<u64>) {
        let policy = self.cover();
        let evm = self.evm.clone();
        // Without a protocol deadline, cap the wait so a quiet chain cannot stall the swap.
        let deadline = deadline.or_else(|| evm.now().ok().map(|n| n + (2 * policy.max_secs).max(60)));
        let msg = self.w.wait_for_cover(policy, trigger_height, deadline, &|| evm.now().unwrap_or(0));
        if policy.k > 0 || policy.max_secs > 0 {
            self.log(&format!("{what}: {msg}"));
        }
    }

    pub fn swap_path(&self, rfq_hex: &str) -> PathBuf {
        self.dir.join("swaps").join(format!("{rfq_hex}.json"))
    }
}

#[derive(Serialize, Deserialize)]
pub struct RfqKey {
    pub epoch: u32,
    pub sk: String,
}

/// The maker's RFQ epoch key (created on first use). Rotating it means writing a new epoch
/// and deleting the old secret, which gives forward secrecy for RFQs across epochs.
pub fn rfq_key(dir: &Path) -> Result<(u32, pasta_curves::pallas::Scalar)> {
    let p = dir.join("rfqkey.json");
    if !p.exists() {
        let sk = crate::keys::random_scalar();
        save(&p, &RfqKey { epoch: 1, sk: hex::encode(crate::keys::scalar_to_bytes(&sk)) })?;
    }
    let k: RfqKey = load(&p)?;
    let b: [u8; 32] = hex::decode(&k.sk)?.try_into().map_err(|_| anyhow!("rfq key len"))?;
    Ok((k.epoch, crate::keys::scalar_from_bytes(&b)?))
}

/// Process exits here when the test harness asked this party to "vanish" at a stage.
pub fn maybe_vanish(cx: &Ctx, stage: &str, vanish_at: &Option<String>) {
    if vanish_at.as_deref() == Some(stage) {
        cx.log(&format!("VANISH at stage {stage} (process exits; state is persisted)"));
        std::process::exit(0);
    }
}

/// Fields of a swap record that must never reach a log: keys, shares, seeds and the joint viewing key.
const SECRET_FIELDS: &[&str] = &["payout_key", "a", "b", "s_t", "s_m", "e_t", "e_m", "joint_fvk"];

/// A swap record as JSON with its secrets replaced, for the RESULT line printed on exit.
pub fn redacted_json<T: Serialize>(s: &T) -> Result<String> {
    let mut v = serde_json::to_value(s)?;
    if let Some(m) = v.as_object_mut() {
        for k in SECRET_FIELDS {
            if let Some(x) = m.get_mut(*k) {
                if !(x.is_null() || x.as_str() == Some("")) {
                    *x = serde_json::Value::String("<redacted>".into());
                }
            }
        }
    }
    Ok(serde_json::to_string(&v)?)
}
