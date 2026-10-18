//! Taker: sells shielded ZEC for tokens through one maker, never touching a transparent address.
//!
//! v2 rules: fund only after the escrow is on the FINALIZED head and every parameter matches;
//! lock with a payout-signed commitClaim (a late commit reverts and leaks only a hash);
//! reveal a only after the commit is finalized and well before the reveal deadline;
//! withdraw separately (pull payments).

use anyhow::{anyhow, bail, Result};
use group::GroupEncoding;
use orchard::keys::{FullViewingKey, Scope};
use pasta_curves::pallas;
use serde::{Deserialize, Serialize};
use std::{fs, thread, time::Duration};

use crate::{
    common::{load, maybe_vanish, save, Ctx},
    evm::{Evm, State},
    keys,
    msg::{self, Accept, Envelope, Quote, Rfq},
    wallet::{self, SignWith},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Stage {
    RfqSent,
    Accepted,
    Funded,
    Committed,
    Revealed,
    Claimed,
    /// Recovery sweep broadcast; kept (with a) until it has k_Z confirmations.
    Recovering,
    Recovered,
    Aborted(String),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TakerSwap {
    pub rfq_id: String,
    pub stage: Stage,
    pub lots: u8,
    pub a: String,   // taker share, un-normalized (erased at the end)
    pub s_t: String, // erased at the end
    pub e_t: String, // ephemeral DH secret (erased at the end)
    pub e_t_pub: String,
    pub payout_key: String, // kept: it controls the payout address
    pub payout: String,
    pub reply_index: u32,
    pub price: Option<u64>,
    pub relayer_fee: Option<u64>,
    pub premium: Option<u64>,
    pub b_pub: Option<String>,
    pub e_m_pub: Option<String>,
    pub t1_secs: Option<u32>,
    pub t2_secs: Option<u32>,
    pub reveal_secs: Option<u32>,
    pub negated: bool,
    pub joint_fvk: Option<String>,
    pub escrow_id: Option<String>,
    pub fund_txid: Option<String>,
    pub commit_tx: Option<String>,
    pub claim_tx: Option<String>,
    pub sweep_txid: Option<String>,
    #[serde(default)]
    pub sweep_height: Option<u64>,
    #[serde(default)]
    pub last_claim_at: Option<u64>,
    #[serde(default)]
    pub escrow_salt: Option<String>,
    pub started: u64,
    pub log: Vec<String>,
}

pub struct Opts {
    pub lots: u8,
    pub vanish_at: Option<String>,
    pub evil: Option<String>,
}

fn h(b: &[u8]) -> String {
    hex::encode(b)
}
fn unhex<const N: usize>(s: &str) -> Result<[u8; N]> {
    hex::decode(s.trim_start_matches("0x"))?.try_into().map_err(|_| anyhow!("hex length"))
}
fn terminal(s: &Stage) -> bool {
    matches!(s, Stage::Claimed | Stage::Recovered | Stage::Aborted(_))
}

fn put(cx: &Ctx, s: &mut TakerSwap, line: String) -> Result<()> {
    cx.log(&format!("{} {}", &s.rfq_id[..8], line));
    s.log.push(line);
    if terminal(&s.stage) {
        s.a.clear();
        s.s_t.clear();
        s.e_t.clear();
        s.joint_fvk = None;
    }
    save(&cx.swap_path(&s.rfq_id), s)
}

fn current(cx: &Ctx) -> Result<Option<TakerSwap>> {
    for e in fs::read_dir(cx.dir.join("swaps"))? {
        let p = e?.path();
        if p.extension().map(|x| x == "json").unwrap_or(false) {
            let s: TakerSwap = load(&p)?;
            if !terminal(&s.stage) {
                return Ok(Some(s));
            }
        }
    }
    Ok(None)
}

pub fn run(cx: &mut Ctx, opts: Opts) -> Result<()> {
    cx.cfg.check_margin()?;
    verify_directory(cx)?;
    let mut s = match current(cx)? {
        Some(s) => {
            cx.log(&format!("resuming swap {} at stage {:?}", &s.rfq_id[..8], s.stage));
            if let Some(f) = &s.joint_fvk {
                let fvk = FullViewingKey::from_bytes(&unhex::<96>(f)?).ok_or_else(|| anyhow!("fvk"))?;
                cx.w.watch(&s.rfq_id, &fvk, &[Scope::External]);
            }
            s
        }
        None => send_rfq(cx, &opts)?,
    };
    loop {
        let step = (|| -> Result<bool> {
            cx.w.sync()?;
            match s.stage.clone() {
                Stage::RfqSent => on_quote(cx, &opts, &mut s)?,
                Stage::Accepted => on_open(cx, &opts, &mut s)?,
                Stage::Funded => on_funded(cx, &opts, &mut s)?,
                Stage::Committed => on_committed(cx, &opts, &mut s)?,
                Stage::Revealed => on_revealed(cx, &mut s)?,
                Stage::Recovering => on_recovering(cx, &mut s)?,
                _ => return Ok(true),
            }
            Ok(false)
        })();
        match step {
            Ok(true) => {
                println!("RESULT {}", serde_json::to_string(&s)?);
                return Ok(());
            }
            Ok(false) => {}
            Err(e) => {
                cx.log(&format!("transient error, retrying: {e:#}"));
                thread::sleep(Duration::from_secs(5));
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn send_rfq(cx: &mut Ctx, opts: &Opts) -> Result<TakerSwap> {
    let mut rfq_id = [0u8; 16];
    rand::Rng::fill_bytes(&mut { crate::OsRng }, &mut rfq_id);
    let mut s_t = [0u8; 32];
    rand::Rng::fill_bytes(&mut { crate::OsRng }, &mut s_t);
    // Fresh per RFQ (R4): share, seed, DH key, payout key, reply address.
    let a = keys::random_scalar();
    let pok_a = keys::pok_prove(&a, &rfq_id, b"taker");
    let e_t = keys::random_scalar();
    let e_t_pub = msg::dh_public(&e_t);
    let mut pk = [0u8; 32];
    rand::Rng::fill_bytes(&mut { crate::OsRng }, &mut pk);
    let payout_key = format!("0x{}", h(&pk));
    let payout = Evm::address_of(&payout_key)?;
    let reply_index = u32::from_le_bytes(rfq_id[..4].try_into().unwrap()) & 0x7fff_ffff;
    let reply = cx.me.fvk.address_at(reply_index, Scope::External);
    let rfq = Rfq {
        rfq_id,
        lots: opts.lots,
        a_pub: (keys::g() * a).to_bytes(),
        pok_a,
        commit_s_t: keys::h32(b"commit-sT", &[&s_t]),
        payout: unhex::<20>(&payout)?,
        reply_addr: reply.to_raw_address_bytes(),
    };
    let maker_pub = unhex::<32>(cx.cfg.maker_rfq_pub.as_deref().ok_or_else(|| anyhow!("maker_rfq_pub"))?)?;
    let ct = msg::seal(b"rfq", &e_t, &maker_pub, &e_t_pub, &maker_pub, &rfq.to_bytes())?;
    let env = Envelope::Rfq { epoch: cx.cfg.maker_rfq_epoch.unwrap_or(1), eph: e_t_pub, ct };
    let maker = wallet::decode_ua(cx.cfg.maker_ua.as_deref().ok_or_else(|| anyhow!("maker_ua"))?)?;
    let (change, ovk, ask) = (cx.me.change(), cx.me.ovk.clone(), cx.me.ask.clone());
    let txid = cx.w.pay("wallet", &ask, change, &ovk, vec![(maker, 0, env.encode())])?;
    let mut s = TakerSwap {
        rfq_id: h(&rfq_id),
        stage: Stage::RfqSent,
        lots: opts.lots,
        a: h(&keys::scalar_to_bytes(&a)),
        s_t: h(&s_t),
        e_t: h(&keys::scalar_to_bytes(&e_t)),
        e_t_pub: h(&e_t_pub),
        payout_key,
        payout,
        reply_index,
        price: None,
        relayer_fee: None,
        premium: None,
        b_pub: None,
        e_m_pub: None,
        t1_secs: None,
        t2_secs: None,
        reveal_secs: None,
        negated: false,
        joint_fvk: None,
        escrow_id: None,
        fund_txid: None,
        commit_tx: None,
        claim_tx: None,
        sweep_txid: None,
        started: cx.evm.now()?,
        sweep_height: None,
        last_claim_at: None,
        escrow_salt: None,
        log: vec![],
    };
    put(cx, &mut s, format!("encrypted RFQ, {} lot(s) ({} memo bytes), sent to maker in shielded tx {txid}", opts.lots, env.wire_len()))?;
    maybe_vanish(cx, "rfq", &opts.vanish_at);
    Ok(s)
}

/// The maker's directory entry (unified address, RFQ epoch and key) must be signed by the maker's
/// EVM key, so an impostor cannot advertise a known maker's EVM address with its own UA and
/// collect premiums (round-2 review M5).
fn verify_directory(cx: &Ctx) -> Result<()> {
    let ua = cx.cfg.maker_ua.clone().ok_or_else(|| anyhow!("maker_ua"))?;
    let evm = cx.cfg.maker_evm.clone().ok_or_else(|| anyhow!("maker_evm"))?;
    let pk = unhex::<32>(cx.cfg.maker_rfq_pub.as_deref().ok_or_else(|| anyhow!("maker_rfq_pub"))?)?;
    let epoch = cx.cfg.maker_rfq_epoch.ok_or_else(|| anyhow!("maker_rfq_epoch"))?;
    let sig = hex::decode(cx.cfg.maker_dir_sig.as_deref().ok_or_else(|| anyhow!("maker_dir_sig: the maker directory entry must be signed"))?.trim_start_matches("0x"))?;
    let digest = keys::directory_digest(&ua, epoch, &pk, &evm);
    if !Evm::personal_verify(&evm, &digest, &sig) {
        bail!("maker directory entry is not signed by {evm}");
    }
    Ok(())
}

fn on_quote(cx: &mut Ctx, opts: &Opts, s: &mut TakerSwap) -> Result<()> {
    let rfq_id = unhex::<16>(&s.rfq_id)?;
    let e_t = keys::scalar_from_bytes(&unhex::<32>(&s.e_t)?)?;
    let e_t_pub = unhex::<32>(&s.e_t_pub)?;
    let found = cx.w.received.iter().filter(|r| r.label == "wallet").find_map(|r| match Envelope::decode(&r.memo) {
        Ok(Some(Envelope::Quote { rfq_id: id, eph, ct })) if id == rfq_id => {
            msg::open(b"quote", &e_t, &eph, &e_t_pub, &eph, &ct).ok().and_then(|b| Quote::from_bytes(&b).ok().map(|q| (q, b, eph, r.height)))
        }
        _ => None,
    });
    let Some((q, body, e_m_pub, quote_height)) = found else {
        if cx.evm.now()? > s.started + cx.cfg.quote_timeout_secs.unwrap_or(300) {
            s.stage = Stage::Aborted("no quote".into());
            put(cx, s, "ABORT: no quote before timeout".into())?;
        }
        return Ok(());
    };
    let expect_escrow = unhex::<20>(&cx.evm.escrow)?;
    let expect_maker = unhex::<20>(cx.cfg.maker_evm.as_deref().ok_or_else(|| anyhow!("maker_evm"))?)?;
    let b_pub = keys::point_from_bytes(&q.b_pub)?;
    let reject = if !keys::pok_verify(&b_pub, &q.pok_b, &rfq_id, b"maker") {
        Some("maker proof of knowledge INVALID (possible rogue-key attack)".to_string())
    } else if q.escrow != expect_escrow {
        Some("quote names an unknown escrow contract".into())
    } else if q.maker_evm != expect_maker {
        Some("quote names an unexpected maker address".into())
    } else if !Evm::personal_verify(cx.cfg.maker_evm.as_deref().unwrap_or(""), &q.sig_digest(&rfq_id), &q.sig) {
        Some("quote is not signed by the expected maker EVM address".into())
    } else if q.price > cx.cfg.max_price.unwrap_or(u64::MAX) {
        Some(format!("price {} above limit", q.price))
    } else if q.premium > cx.cfg.max_premium.unwrap_or(u64::MAX) {
        Some(format!("premium {} above limit", q.premium))
    } else if q.relayer_fee >= q.price {
        Some("relayer fee not below price".into())
    } else if (q.t1_secs as u64) < 2 * cx.cfg.delta_claim {
        Some("claim window too short".into())
    } else if q.t0_secs as u64 + q.t1_secs as u64 + q.t2_secs as u64 > cx.cfg.max_horizon_secs.unwrap_or(48 * 3600) {
        Some(format!("deadline horizon t2 - open = {}s above the taker's limit of {}s",
            q.t0_secs as u64 + q.t1_secs as u64 + q.t2_secs as u64, cx.cfg.max_horizon_secs.unwrap_or(48 * 3600)))
    } else if (q.reveal_secs as u64) < 2 * cx.cfg.finality_secs + cx.cfg.inclusion_secs + cx.cfg.margin_secs + 1 {
        Some(format!("reveal window {}s too short for finality (needs 2F + D + margin + 1)", q.reveal_secs))
    } else if (q.t0_secs as u64) < cx.cfg.finality_secs + cx.cfg.inclusion_secs + 3 * cx.cfg.delta_claim + 3 * cx.cfg.cover_max_secs.unwrap_or(0) {
        // The funding waits for open's finality and then for a cover delay; if t0 cannot absorb the
        // delay's tail (3 draws), the wait would be truncated at a public deadline (review M4).
        Some(format!("funding window t0 - open = {}s too short for finality plus the cover delay", q.t0_secs))
    } else {
        None
    };
    if let Some(why) = reject {
        s.stage = Stage::Aborted(why.clone());
        return put(cx, s, format!("QUOTE REJECTED: {why}; nothing was funded"));
    }
    let a = keys::scalar_from_bytes(&unhex::<32>(&s.a)?)?;
    let s_t = unhex::<32>(&s.s_t)?;
    let joint = keys::joint(&rfq_id, keys::g() * a, b_pub, &s_t, &q.s_m)?;
    s.escrow_salt = Some(keys::escrow_salt(&rfq_id, &s_t, &q.s_m));
    s.negated = joint.negated;
    s.joint_fvk = Some(h(&joint.fvk.to_bytes()));
    s.b_pub = Some(h(&q.b_pub));
    s.e_m_pub = Some(h(&e_m_pub));
    s.price = Some(q.price);
    s.relayer_fee = Some(q.relayer_fee);
    s.premium = Some(q.premium);
    s.t1_secs = Some(q.t1_secs);
    s.t2_secs = Some(q.t2_secs);
    s.reveal_secs = Some(q.reveal_secs);
    cx.w.watch(&s.rfq_id, &joint.fvk, &[Scope::External]);

    let quote_hash = keys::h32(b"quote", &[&rfq_id, &body]);
    let ct = msg::seal(b"accept", &e_t, &e_m_pub, &e_t_pub, &e_m_pub, &Accept { s_t, quote_hash }.to_bytes())?;
    let env = Envelope::Accept { rfq_id, ct };
    let maker = wallet::decode_ua(cx.cfg.maker_ua.as_deref().unwrap())?;
    cx.cover_wait("ACCEPT", quote_height, None);
    let (change, ovk, ask) = (cx.me.change(), cx.me.ovk.clone(), cx.me.ask.clone());
    let txid = cx.w.pay("wallet", &ask, change, &ovk, vec![(maker, q.premium, env.encode())])?;
    s.stage = Stage::Accepted;
    put(cx, s, format!("QUOTE decrypted (price {}, maker PoK valid); encrypted ACCEPT with {} zats premium sent in tx {txid}; joint address {}",
        q.price, q.premium, wallet::encode_ua(&joint.fvk.address_at(0u32, Scope::External))))?;
    maybe_vanish(cx, "accepted", &opts.vanish_at);
    Ok(())
}

fn on_open(cx: &mut Ctx, opts: &Opts, s: &mut TakerSwap) -> Result<()> {
    let maker = cx.cfg.maker_evm.clone().unwrap().to_lowercase();
    let salt = s.escrow_salt.clone().ok_or_else(|| anyhow!("no escrow salt"))?;
    let id = Evm::swap_id(&maker, &salt)?;
    let view = cx.evm.swap(&id)?;
    if view.state == State::None {
        if cx.evm.now()? > s.started + cx.cfg.quote_timeout_secs.unwrap_or(300) {
            s.stage = Stage::Aborted("maker never opened escrow".into());
            put(cx, s, "ABORT: no escrow opened; nothing funded".into())?;
        }
        return Ok(());
    }
    let a = keys::scalar_from_bytes(&unhex::<32>(&s.a)?)?;
    let b_pub = keys::point_from_bytes(&unhex::<32>(s.b_pub.as_ref().unwrap())?)?;
    let (mut a_pub, mut b_pub) = (keys::g() * a, b_pub);
    if s.negated {
        a_pub = -a_pub;
        b_pub = -b_pub;
    }
    let (ax, ay) = keys::affine_evm(&a_pub);
    let (bx, by) = keys::affine_evm(&b_pub);
    let same = |x: &str, y: &str| match (keys::u256_be_bytes(x), keys::u256_be_bytes(y)) {
        (Ok(p), Ok(q)) => p == q,
        _ => false,
    };
    // R1: every stored parameter must match what was quoted and requested.
    let checks = [
        ("maker", view.maker == maker),
        ("payout", view.payout == s.payout.to_lowercase()),
        ("token", view.token == cx.evm.token),
        ("amount", Some(view.amount) == s.price),
        ("relayer fee", Some(view.relayer_fee) == s.relayer_fee),
        ("A", same(&view.ax, &ax) && same(&view.ay, &ay)),
        ("B", same(&view.bx, &bx) && same(&view.by, &by)),
        ("claim window", view.t1 >= view.t0 + s.t1_secs.unwrap() as u64),
        ("punish window", view.t2 >= view.t1 + s.t2_secs.unwrap() as u64),
        ("reveal window", view.reveal_window >= s.reveal_secs.unwrap() as u64),
        ("state", view.state == State::Open),
    ];
    if let Some((what, _)) = checks.iter().find(|(_, ok)| !ok) {
        s.stage = Stage::Aborted(format!("escrow parameter mismatch: {what}"));
        return put(cx, s, format!("ABORT: escrow {what} does not match the quote; nothing funded"));
    }
    // R1: the open must be on the FINALIZED head (an L1 reorg can still remove a merely safe block).
    let opened_at = cx.evm.opened_block(&id)?.unwrap_or(u64::MAX);
    let (fin_n, _) = cx.evm.block_info("finalized")?;
    if opened_at > fin_n {
        return Ok(());
    }
    // R1b: fund only if the maker can still confirm the note and set Ready before t0.
    let now = cx.evm.now()?;
    if now + 2 * cx.cfg.delta_claim >= view.t0 {
        s.stage = Stage::Aborted("too close to t0 to fund safely".into());
        return put(cx, s, format!("ABORT (R1b): now {now} is within {}s of t0 {}; not funding", 2 * cx.cfg.delta_claim, view.t0));
    }
    s.escrow_id = Some(id.clone());
    if opts.evil.as_deref() == Some("free-claim") {
        // Attack: never fund; try to lock the escrow once t0 has passed.
        s.stage = Stage::Funded;
        return put(cx, s, "[EVIL MODE: free-claim] escrow verified; NOT funding; will try to claim after t0".into());
    }
    let fvk = FullViewingKey::from_bytes(&unhex::<96>(s.joint_fvk.as_ref().unwrap())?).unwrap();
    let joint_addr = fvk.address_at(0u32, Scope::External);
    let mut amount = s.lots as u64 * cx.cfg.lot_zats + wallet::zip317_fee(1, 1);
    if opts.evil.as_deref() == Some("underfund") {
        amount /= 2;
    }
    let memo = Envelope::Fund { rfq_id: unhex::<16>(&s.rfq_id)? }.encode();
    // Decorrelate funding from the public open(), but stay inside R1b's funding deadline.
    let tip = cx.w.height;
    cx.cover_wait("FUND", tip, Some(view.t0.saturating_sub(3 * cx.cfg.delta_claim)));
    let now = cx.evm.now()?;
    if now + 2 * cx.cfg.delta_claim >= view.t0 {
        s.stage = Stage::Aborted("cover delay reached the funding deadline".into());
        return put(cx, s, format!("ABORT (R1b after cover wait): now {now} too close to t0 {}; not funding", view.t0));
    }
    let (change, ovk, ask) = (cx.me.change(), cx.me.ovk.clone(), cx.me.ask.clone());
    let txid = cx.w.pay("wallet", &ask, change, &ovk, vec![(joint_addr, amount, memo)])?;
    s.fund_txid = Some(txid.clone());
    s.stage = Stage::Funded;
    put(cx, s, format!("escrow verified ({} checks) and finalized; FUNDED joint address with {amount} zats in fully shielded tx {txid}", checks.len()))?;
    maybe_vanish(cx, "funded", &opts.vanish_at);
    Ok(())
}

fn commit(cx: &mut Ctx, s: &mut TakerSwap, id: &str) -> Result<String> {
    let a = keys::normalize(keys::scalar_from_bytes(&unhex::<32>(&s.a)?)?, s.negated);
    let relayer_key = cx.cfg.relayer_key.clone().ok_or_else(|| anyhow!("relayer_key"))?;
    let relayer = Evm::address_of(&relayer_key)?;
    let c = cx.evm.commitment_of(id, &keys::scalar_evm(&a))?;
    let (v, r, sg) = cx.evm.sign_commit(&s.payout_key, id, &c, &relayer)?;
    cx.evm.send(&relayer_key, &cx.evm.escrow.clone(), "commitClaim(bytes32,bytes32,address,uint8,bytes32,bytes32)", &[id.to_string(), c, relayer, v, r, sg])
}

fn recover(cx: &mut Ctx, s: &mut TakerSwap, id: &str) -> Result<()> {
    let (b_hex, bn) = cx.evm.revealed("Refunded", id)?.ok_or_else(|| anyhow!("refund event"))?;
    let b = keys::scalar_from_evm(&b_hex)?;
    let b_pub = keys::point_from_bytes(&unhex::<32>(s.b_pub.as_ref().unwrap())?)?;
    let b_pub = if s.negated { -b_pub } else { b_pub };
    if keys::g() * b != b_pub {
        bail!("published b does not match B");
    }
    let a = keys::normalize(keys::scalar_from_bytes(&unhex::<32>(&s.a)?)?, s.negated);
    let Some(n) = cx.w.unspent(&s.rfq_id).into_iter().next() else {
        s.stage = Stage::Recovered;
        return put(cx, s, format!("maker REFUNDED at base block {bn}; no joint note was funded, nothing to recover"));
    };
    let tip = cx.w.height;
    cx.cover_wait("RECOVERY SWEEP", tip, None);
    recovery_sweep(cx, s, n, a, b, &format!("maker REFUNDED at base block {bn}; read b"))
}

fn recovery_sweep(cx: &mut Ctx, s: &mut TakerSwap, n: wallet::Received, a: pallas::Scalar, b: pallas::Scalar, why: &str) -> Result<()> {
    let v = n.note.value().inner();
    let fee = wallet::zip317_fee(1, 1);
    let to = cx.me.fvk.address_at(0u32, Scope::External);
    let txid = cx.w.send(vec![(n, SignWith::Joint(a + b))], vec![(to, v - fee, wallet::empty_memo(), None)])?;
    s.sweep_txid = Some(txid.clone());
    s.sweep_height = Some(cx.w.height);
    s.stage = Stage::Recovering;
    put(cx, s, format!("{why}; swept {} zats back with (a+b), tx {txid}; waiting for confirmations", v - fee))
}

/// Keep a (and the joint key) until the recovery sweep has k_Z confirmations; re-send it if it
/// expired unmined (40 blocks). Erasing earlier could strand the note.
fn on_recovering(cx: &mut Ctx, s: &mut TakerSwap) -> Result<()> {
    let k_z = cx.cfg.k_z.unwrap_or(3);
    let tip = cx.w.height;
    match cx.w.spend_of(&s.rfq_id) {
        Some((tx, h)) if tip + 1 - h >= k_z => {
            s.stage = Stage::Recovered;
            put(cx, s, format!("recovery sweep {tx} has {} confirmations (k_Z = {k_z}); done", tip + 1 - h))
        }
        Some(_) => Ok(()),
        None if tip > s.sweep_height.unwrap_or(tip) + 40 => {
            let id = s.escrow_id.clone().unwrap();
            let (b_hex, _) = cx.evm.revealed("Refunded", &id)?.ok_or_else(|| anyhow!("refund event"))?;
            let b = keys::scalar_from_evm(&b_hex)?;
            let a = keys::normalize(keys::scalar_from_bytes(&unhex::<32>(&s.a)?)?, s.negated);
            let n = cx.w.unspent(&s.rfq_id).into_iter().next().ok_or_else(|| anyhow!("joint note missing"))?;
            recovery_sweep(cx, s, n, a, b, "previous recovery sweep expired unmined; RE-SENT")
        }
        None => Ok(()),
    }
}

fn on_funded(cx: &mut Ctx, opts: &Opts, s: &mut TakerSwap) -> Result<()> {
    let id = s.escrow_id.clone().unwrap();
    let view = cx.evm.swap(&id)?;
    let now = cx.evm.now()?;
    match view.state {
        State::Refunded => recover(cx, s, &id),
        State::Ready if now + cx.cfg.delta_small < view.t1 => {
            let tx = commit(cx, s, &id)?;
            s.commit_tx = Some(tx.clone());
            s.stage = Stage::Committed;
            put(cx, s, format!("escrow READY; LOCKED with payout-signed commitClaim (publishes only H(id, a)) via relayer, base tx {tx}"))?;
            maybe_vanish(cx, "committed", &opts.vanish_at);
            Ok(())
        }
        State::Open | State::Ready if now >= view.t2 => {
            // Punish path: the maker neither set Ready in time nor refunded in [t1, t2).
            let tx = commit(cx, s, &id)?;
            s.commit_tx = Some(tx.clone());
            s.stage = Stage::Committed;
            put(cx, s, format!("past t2 with escrow {:?}: LOCKED via the punish path (payout-signed commitClaim), base tx {tx}{}", view.state,
                if opts.evil.as_deref() == Some("free-claim") { " [EVIL MODE: free-claim, never funded]" } else { "" }))
        }
        State::Open if opts.evil.as_deref() == Some("free-claim") && now >= view.t0 && !s.log.iter().any(|l| l.contains("REJECTED")) => {
            // The attack, before t2: lock without Ready. The escrow rejects it.
            match commit(cx, s, &id) {
                Ok(tx) => bail!("free claim lock was ACCEPTED before t2 (tx {tx}): invariant broken"),
                Err(e) => put(cx, s, format!("[EVIL MODE: free-claim] commitClaim without Ready before t2 REJECTED by escrow: {}; waiting for t2", e.to_string().lines().next().unwrap_or(""))),
            }
        }
        State::Ready | State::Open if opts.evil.as_deref() == Some("late-commit") && now >= view.t1 && !s.log.iter().any(|l| l.contains("REVERTED")) => {
            // Ignore the deadline on purpose: the late commit must revert and reveal nothing.
            match commit(cx, s, &id) {
                Ok(tx) => bail!("late commit ACCEPTED (tx {tx}): v2 invariant broken"),
                Err(e) => {
                    let line = format!("[EVIL MODE: late-commit] commitClaim after t1 REVERTED, publishing only a hash: {}", e.to_string().lines().next().unwrap_or(""));
                    cx.log(&line);
                    s.log.push(line);
                    save(&cx.swap_path(&s.rfq_id), s)?;
                    Ok(()) // fall back to waiting for the maker's refund
                }
            }
        }
        State::Ready | State::Open if now + cx.cfg.delta_small >= view.t1 => {
            if !s.log.iter().any(|l| l.starts_with("R2")) {
                put(cx, s, format!("R2: claim window closed (now {now}, t1 {}); not committing; waiting for the maker's refund or for t2 {}", view.t1, view.t2))?;
            }
            Ok(())
        }
        State::Claimed | State::Locked => {
            s.stage = Stage::Committed;
            put(cx, s, "escrow already locked/claimed; resuming".into())
        }
        _ => Ok(()),
    }
}

/// After revealing a, stay until Claimed is FINAL. If a reorg drops the claim while a is
/// already public, re-send it before the deadline (TLA+ condition MarginC >= D + R*F).
fn on_revealed(cx: &mut Ctx, s: &mut TakerSwap) -> Result<()> {
    let id = s.escrow_id.clone().unwrap();
    let relayer_key = cx.cfg.relayer_key.clone().ok_or_else(|| anyhow!("relayer_key"))?;
    let (fin_n, _) = cx.evm.block_info("finalized")?;
    if cx.evm.swap_at(&id, Some(fin_n))?.state == State::Claimed {
        let wtx = cx.evm.send(&relayer_key, &cx.evm.escrow.clone(), "withdraw(address,address)", &[cx.evm.token.clone(), s.payout.clone()])?;
        // the relayer collects its own fee the same way
        cx.evm.send(&relayer_key, &cx.evm.escrow.clone(), "withdraw(address,address)", &[cx.evm.token.clone(), Evm::address_of(&relayer_key)?])?;
        s.stage = Stage::Claimed;
        return put(cx, s, format!("claim is final (block <= {fin_n}); payout withdrawn by relayer (tx {wtx})"));
    }
    let view = cx.evm.swap(&id)?;
    let now = cx.evm.now()?;
    // Still Locked at the head after the claim had D to land: it was dropped (or reorged out).
    let pending = s.last_claim_at.map(|t| now < t + cx.cfg.inclusion_secs).unwrap_or(false);
    if view.state == State::Locked && !pending && now < view.locked_at + view.reveal_window {
        let a = keys::normalize(keys::scalar_from_bytes(&unhex::<32>(&s.a)?)?, s.negated);
        let tx = cx.evm.send(&relayer_key, &cx.evm.escrow.clone(), "claim(bytes32,uint256)", &[id.clone(), keys::scalar_evm(&a)])?;
        s.last_claim_at = Some(now);
        put(cx, s, format!("claim not on chain; RE-SENT claim(a), tx {tx}"))?;
    }
    Ok(())
}

fn on_committed(cx: &mut Ctx, opts: &Opts, s: &mut TakerSwap) -> Result<()> {
    let id = s.escrow_id.clone().unwrap();
    let view = cx.evm.swap(&id)?;
    let now = cx.evm.now()?;
    let relayer_key = cx.cfg.relayer_key.clone().ok_or_else(|| anyhow!("relayer_key"))?;
    match view.state {
        State::Locked => {
            if opts.evil.as_deref() == Some("no-reveal") {
                return Ok(());
            }
            // Reveal only once the lock is final, and only with margin before the deadline.
            let locked_at_block = cx.evm.locked_block(&id)?.unwrap_or(u64::MAX);
            let (fin_n, _) = cx.evm.block_info("finalized")?;
            if locked_at_block > fin_n {
                return Ok(());
            }
            let deadline = view.locked_at + view.reveal_window;
            // R2r: the reveal must be FINAL before the deadline, or a reorg could push it past.
            if !cx.cfg.final_before(now, deadline) {
                if !s.log.last().map(|l| l.starts_with("R2")).unwrap_or(false) {
                    put(cx, s, format!("R2: reveal deadline {deadline} too close; NOT revealing a; waiting for maker refund"))?;
                }
                return Ok(());
            }
            let a = keys::normalize(keys::scalar_from_bytes(&unhex::<32>(&s.a)?)?, s.negated);
            let tx = cx.evm.send(&relayer_key, &cx.evm.escrow.clone(), "claim(bytes32,uint256)", &[id.clone(), keys::scalar_evm(&a)])?;
            s.claim_tx = Some(tx.clone());
            s.last_claim_at = Some(now);
            s.stage = Stage::Revealed;
            put(cx, s, format!("lock finalized; REVEALED a via relayer (tx {tx}); waiting for the claim to be final"))
        }
        State::Claimed => {
            s.stage = Stage::Claimed;
            put(cx, s, "already claimed".into())
        }
        State::Refunded => recover(cx, s, &id),
        State::Ready => {
            // our commit did not stick (e.g. reorged out); go back and commit again
            s.stage = Stage::Funded;
            put(cx, s, "commit not on chain; retrying".into())
        }
        _ => Ok(()),
    }
}
