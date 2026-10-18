//! Maker: quotes RFQs that arrive as encrypted memos, escrows tokens, sweeps ZEC on claim.
//!
//! v2 rules: Ready only after the ZEC note has k_Z confirmations; refunds after a deadline
//! only once a *finalized* EVM block past that deadline shows no claim can still happen
//! (so a refund can never revert and leak b); payments are withdrawn separately.

use anyhow::{anyhow, bail, Result};
use group::GroupEncoding;
use orchard::keys::{FullViewingKey, Scope};
use pasta_curves::pallas;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, thread, time::Duration};

use crate::{
    common::{load, maybe_vanish, rfq_key, save, Ctx},
    evm::{Evm, State},
    keys,
    msg::{self, Envelope, Quote, Rfq},
    wallet::{self, SignWith},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Stage {
    Quoted,
    Opened,
    Ready,
    /// refund(b) broadcast; b is public. Kept until the refund is FINAL (re-sent if dropped).
    RefundSent,
    /// Sweep of the joint note broadcast; kept until it has k_Z confirmations (re-sent if it expires).
    Sweeping,
    Swept,
    Refunded,
    Aborted(String),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MakerSwap {
    pub rfq_id: String,
    pub stage: Stage,
    pub lots: u8,
    pub price: u64,
    pub relayer_fee: u64,
    pub premium: u64,
    pub payout: String,
    pub a_pub: String,
    pub commit_s_t: String,
    pub b: String,   // maker share, un-normalized (erased at the end)
    pub s_m: String, // erased at the end
    pub e_m: String, // ephemeral DH secret (erased at the end)
    pub e_t_pub: String,
    pub quote_hash: String,
    pub t0_secs: u32,
    pub t1_secs: u32,
    pub t2_secs: u32,
    pub reveal_secs: u32,
    pub negated: bool,
    pub joint_fvk: Option<String>,
    pub escrow_id: Option<String>,
    pub t0: Option<u64>,
    pub t1: Option<u64>,
    pub t2: Option<u64>,
    #[serde(default)]
    pub last_refund_at: Option<u64>,
    #[serde(default)]
    pub sweep_height: Option<u64>,
    pub log: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Seen {
    msgs: HashSet<String>,
}

pub struct Opts {
    pub vanish_at: Option<String>,
    pub evil: Option<String>,
    pub exit_after: Option<usize>,
}

fn h(b: &[u8]) -> String {
    hex::encode(b)
}
fn unhex<const N: usize>(s: &str) -> Result<[u8; N]> {
    hex::decode(s.trim_start_matches("0x"))?.try_into().map_err(|_| anyhow!("hex length"))
}

fn terminal(s: &Stage) -> bool {
    matches!(s, Stage::Swept | Stage::Refunded | Stage::Aborted(_))
}

pub fn run(cx: &mut Ctx, opts: Opts) -> Result<()> {
    let key = cx.cfg.evm_key.clone().ok_or_else(|| anyhow!("maker needs evm_key"))?;
    let maker_evm = Evm::address_of(&key)?;
    let (epoch, rfq_sk) = rfq_key(&cx.dir)?;
    // Timing conditions from the TLA+ model (formal/README.md): refuse unsafe parameters.
    cx.cfg.check_margin()?;
    let (dd, ff, mm) = (cx.cfg.inclusion_secs, cx.cfg.finality_secs, cx.cfg.margin_secs);
    let t2gap = cx.cfg.t2_secs.unwrap_or(7200) as u64;
    let w = cx.cfg.reveal_secs.unwrap_or(600) as u64;
    // The deadline refund goes out once a finalized block is past t1 + delta_small (about
    // t1 + delta_small + F) and must itself be final before t2 (now + D + F + margin < t2).
    let first = cx.cfg.delta_small.max(dd);
    if t2gap < first + dd + 2 * ff + mm + 1 {
        bail!("unsafe parameters: t2 - t1 = {t2gap}s must be >= max(delta, D) + D + 2F + margin + 1 = {}s", first + dd + 2 * ff + mm + 1);
    }
    // The taker reveals once the lock is final (lockedAt + F) and the claim must be final in W.
    if w < 2 * ff + dd + mm + 1 {
        bail!("unsafe parameters: reveal window {w}s must be >= 2F + D + margin + 1 = {}s", 2 * ff + dd + mm + 1);
    }
    cx.log(&format!("maker up. shielded UA {} ; EVM {} ; RFQ key epoch {epoch} {}", cx.me.ua(), maker_evm, h(&msg::dh_public(&rfq_sk))));
    let seen_path = cx.dir.join("seen.json");
    let mut seen: Seen = if seen_path.exists() { load(&seen_path)? } else { Seen::default() };

    for s in all_swaps(cx)? {
        if let Some(f) = &s.joint_fvk {
            let fvk = FullViewingKey::from_bytes(&unhex::<96>(f)?).ok_or_else(|| anyhow!("fvk"))?;
            cx.w.watch(&s.rfq_id, &fvk, &[Scope::External]);
        }
    }

    loop {
        if let Err(e) = cx.w.sync() {
            cx.log(&format!("transient sync error, retrying: {e:#}"));
            thread::sleep(Duration::from_secs(5));
            continue;
        }
        let incoming: Vec<_> = cx.w.received.iter().filter(|r| r.label == "wallet").cloned().collect();
        for r in incoming {
            let tag = format!("{}:{}", r.txid, r.pos);
            if seen.msgs.contains(&tag) {
                continue;
            }
            let handled = match Envelope::decode(&r.memo) {
                Ok(Some(Envelope::Rfq { epoch: ep, eph, ct })) => {
                    let res = (|| {
                        if ep != epoch {
                            bail!("RFQ for epoch {ep}, current epoch is {epoch}");
                        }
                        let me_pub = msg::dh_public(&rfq_sk);
                        let rfq = Rfq::from_bytes(&msg::open(b"rfq", &rfq_sk, &eph, &eph, &me_pub, &ct)?)?;
                        on_rfq(cx, &opts, &maker_evm, rfq, eph, r.height)
                    })();
                    if let Err(e) = res {
                        cx.log(&format!("RFQ rejected: {e:#}"));
                    }
                    true
                }
                Ok(Some(Envelope::Accept { rfq_id, ct })) => match on_accept(cx, &opts, &key, rfq_id, &ct, r.note.value().inner(), r.height) {
                    Err(e) => {
                        cx.log(&format!("ACCEPT processing failed, will retry: {e:#}"));
                        false
                    }
                    Ok(()) => true,
                },
                _ => true,
            };
            if handled {
                seen.msgs.insert(tag);
                save(&seen_path, &seen)?;
            }
        }
        for s in all_swaps(cx)? {
            if matches!(s.stage, Stage::Opened | Stage::Ready | Stage::RefundSent | Stage::Sweeping) {
                if let Err(e) = advance(cx, &opts, &key, s) {
                    cx.log(&format!("advance error: {e:#}"));
                }
            }
        }
        if let Some(n) = opts.exit_after {
            let done = all_swaps(cx)?.iter().filter(|s| terminal(&s.stage)).count();
            if done >= n {
                cx.log(&format!("{done} swap(s) terminal; exiting"));
                for s in all_swaps(cx)? {
                    println!("RESULT {}", crate::common::redacted_json(&s)?);
                }
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn all_swaps(cx: &Ctx) -> Result<Vec<MakerSwap>> {
    let mut v = vec![];
    for e in fs::read_dir(cx.dir.join("swaps"))? {
        let p = e?.path();
        if p.extension().map(|x| x == "json").unwrap_or(false) {
            v.push(load(&p)?);
        }
    }
    Ok(v)
}

fn put(cx: &Ctx, s: &mut MakerSwap, line: String) -> Result<()> {
    cx.log(&format!("{} {}", &s.rfq_id[..8], line));
    s.log.push(line);
    if terminal(&s.stage) {
        // R4: erase per-swap secrets once nothing more can be done with them.
        s.b.clear();
        s.s_m.clear();
        s.e_m.clear();
        s.joint_fvk = None;
    }
    save(&cx.swap_path(&s.rfq_id), s)
}

fn on_rfq(cx: &mut Ctx, opts: &Opts, maker_evm: &str, rfq: Rfq, e_t_pub: [u8; 32], seen_at: u64) -> Result<()> {
    let rfq_hex = h(&rfq.rfq_id);
    if cx.swap_path(&rfq_hex).exists() {
        return Ok(());
    }
    let max_lots = cx.cfg.max_lots.unwrap_or(5);
    if rfq.lots == 0 || rfq.lots > max_lots {
        bail!("lots {} outside 1..={max_lots}", rfq.lots);
    }
    let a_pub = keys::point_from_bytes(&rfq.a_pub)?;
    if !keys::pok_verify(&a_pub, &rfq.pok_a, &rfq.rfq_id, b"taker") {
        bail!("taker proof of knowledge invalid");
    }
    let reply = wallet::addr_from_raw(&rfq.reply_addr)?;
    let gg = keys::g();
    let (b, b_pub, pok_b) = if opts.evil.as_deref() == Some("rogue-key") {
        let b2 = keys::random_scalar();
        (b2, gg * b2 - a_pub, keys::pok_prove(&b2, &rfq.rfq_id, b"maker"))
    } else {
        let b = keys::random_scalar();
        (b, gg * b, keys::pok_prove(&b, &rfq.rfq_id, b"maker"))
    };
    let mut s_m = [0u8; 32];
    rand::Rng::fill_bytes(&mut { crate::OsRng }, &mut s_m);
    let e_m = keys::random_scalar();
    let e_m_pub = msg::dh_public(&e_m);
    let price = rfq.lots as u64 * cx.cfg.price_per_lot.ok_or_else(|| anyhow!("price_per_lot"))?;
    let mut q = Quote {
        price,
        relayer_fee: cx.cfg.relayer_fee.unwrap_or(0),
        premium: cx.cfg.premium_zats.unwrap_or(0),
        b_pub: b_pub.to_bytes(),
        pok_b,
        s_m,
        escrow: unhex::<20>(&cx.evm.escrow)?,
        maker_evm: unhex::<20>(maker_evm)?,
        t0_secs: cx.cfg.t0_secs.unwrap_or(120),
        t1_secs: cx.cfg.t1_secs.unwrap_or(600),
        t2_secs: cx.cfg.t2_secs.unwrap_or(7200),
        reveal_secs: cx.cfg.reveal_secs.unwrap_or(600),
        sig: vec![],
    };
    // Test-only malicious quotes (harness s16, s17): windows an honest maker refuses at startup.
    match opts.evil.as_deref() {
        Some("short-reveal") => q.reveal_secs = (cx.cfg.finality_secs + cx.cfg.inclusion_secs) as u32,
        Some("far-horizon") => q.t2_secs = u32::MAX,
        _ => {}
    }
    // Sign with the maker's EVM key: the taker checks it against the address it trusts before
    // paying the premium, and keeps it as transferable evidence of the quote.
    q.sig = Evm::personal_sign(cx.cfg.evm_key.as_deref().ok_or_else(|| anyhow!("maker needs evm_key"))?, &q.sig_digest(&rfq.rfq_id))?;
    let body = q.to_bytes();
    let ct = msg::seal(b"quote", &e_m, &e_t_pub, &e_t_pub, &e_m_pub, &body)?;
    let env = Envelope::Quote { rfq_id: rfq.rfq_id, eph: e_m_pub, ct };
    let quote_hash = keys::h32(b"quote", &[&rfq.rfq_id, &body]);
    let mut s = MakerSwap {
        rfq_id: rfq_hex,
        stage: Stage::Quoted,
        lots: rfq.lots,
        price,
        relayer_fee: q.relayer_fee,
        premium: q.premium,
        payout: format!("0x{}", h(&rfq.payout)),
        a_pub: h(&rfq.a_pub),
        commit_s_t: h(&rfq.commit_s_t),
        b: h(&keys::scalar_to_bytes(&b)),
        s_m: h(&s_m),
        e_m: h(&keys::scalar_to_bytes(&e_m)),
        e_t_pub: h(&e_t_pub),
        quote_hash: h(&quote_hash),
        t0_secs: q.t0_secs,
        t1_secs: q.t1_secs,
        t2_secs: q.t2_secs,
        reveal_secs: q.reveal_secs,
        negated: false,
        joint_fvk: None,
        escrow_id: None,
        t0: None,
        t1: None,
        t2: None,
        last_refund_at: None,
        sweep_height: None,
        log: vec![],
    };
    cx.cover_wait("QUOTE", seen_at, None);
    let (change, ovk, ask) = (cx.me.change(), cx.me.ovk.clone(), cx.me.ask.clone());
    let txid = cx.w.pay("wallet", &ask, change, &ovk, vec![(reply, 0, env.encode())])?;
    put(cx, &mut s, format!("RFQ for {} lot(s) decrypted and verified; encrypted QUOTE ({} memo bytes, price {price}, premium {} zats) sent in tx {txid}{}",
        rfq.lots, env.wire_len(), q.premium, opts.evil.as_ref().map(|e| format!(" [EVIL MODE: {e}]")).unwrap_or_default()))?;
    maybe_vanish(cx, "quoted", &opts.vanish_at);
    Ok(())
}

fn on_accept(cx: &mut Ctx, opts: &Opts, key: &str, rfq_id: [u8; 16], ct: &[u8], paid: u64, seen_at: u64) -> Result<()> {
    let rfq_hex = h(&rfq_id);
    let p = cx.swap_path(&rfq_hex);
    if !p.exists() {
        bail!("ACCEPT for unknown RFQ");
    }
    let mut s: MakerSwap = load(&p)?;
    if s.stage != Stage::Quoted {
        return Ok(());
    }
    let e_m = keys::scalar_from_bytes(&unhex::<32>(&s.e_m)?)?;
    let e_t_pub = unhex::<32>(&s.e_t_pub)?;
    let acc = msg::Accept::from_bytes(&msg::open(b"accept", &e_m, &e_t_pub, &e_t_pub, &msg::dh_public(&e_m), ct)?)?;
    if h(&acc.quote_hash) != s.quote_hash {
        bail!("ACCEPT references a different quote");
    }
    if h(&keys::h32(b"commit-sT", &[&acc.s_t])) != s.commit_s_t {
        s.stage = Stage::Aborted("s_T does not open its commitment".into());
        return put(cx, &mut s, "ABORT: bad seed reveal".into());
    }
    if paid < s.premium {
        s.stage = Stage::Aborted(format!("premium {paid} < {}", s.premium));
        let line = format!("ABORT: ACCEPT carried {paid} zats, premium is {}", s.premium);
        return put(cx, &mut s, line);
    }
    let a_pub = keys::point_from_bytes(&unhex::<32>(&s.a_pub)?)?;
    let b = keys::scalar_from_bytes(&unhex::<32>(&s.b)?)?;
    let joint = keys::joint(&rfq_id, a_pub, keys::g() * b, &acc.s_t, &unhex::<32>(&s.s_m)?)?;
    s.negated = joint.negated;
    s.joint_fvk = Some(h(&joint.fvk.to_bytes()));
    cx.w.watch(&rfq_hex, &joint.fvk, &[Scope::External]);

    let salt = keys::escrow_salt(&rfq_id, &acc.s_t, &unhex::<32>(&s.s_m)?);
    let id = Evm::swap_id(&Evm::address_of(key)?, &salt)?;
    if cx.evm.swap(&id)?.state != State::None {
        let v = cx.evm.swap(&id)?;
        s.escrow_id = Some(id.clone());
        s.t0 = Some(v.t0);
        s.t1 = Some(v.t1);
        s.t2 = Some(v.t2);
        s.stage = Stage::Opened;
        return put(cx, &mut s, format!("escrow {id} already open on chain (retry); t0={} t1={} t2={}", v.t0, v.t1, v.t2));
    }
    let (ax, ay) = keys::affine_evm(&joint.a_pub);
    let (bx, by) = keys::affine_evm(&joint.b_pub);
    // `open` is public, so without a delay it pins the Accept (the Zcash tx just before it) from
    // behind. Draw a cover delay first (round-2 review M1). The deadlines are set after the delay.
    cx.cover_wait("OPEN", seen_at, None);
    let now = cx.evm.now()?;
    let t0 = now + s.t0_secs as u64;
    let t1 = t0 + s.t1_secs as u64;
    let t2 = t1 + s.t2_secs as u64;
    cx.evm.send(key, &cx.evm.token.clone(), "approve(address,uint256)", &[cx.evm.escrow.clone(), s.price.to_string()])?;
    let tx = cx.evm.send(
        key,
        &cx.evm.escrow.clone(),
        "open(bytes32,address,address,uint256,uint256,uint256,uint256,uint256,uint256,uint64,uint64,uint64,uint64)",
        &[salt, s.payout.clone(), cx.evm.token.clone(), s.price.to_string(), s.relayer_fee.to_string(), ax, ay, bx, by, t0.to_string(), t1.to_string(), t2.to_string(), s.reveal_secs.to_string()],
    )?;
    s.escrow_id = Some(id.clone());
    s.t0 = Some(t0);
    s.t1 = Some(t1);
    s.t2 = Some(t2);
    s.stage = Stage::Opened;
    let line = format!("ACCEPT decrypted (premium {paid} zats received, seed commitment opens, negated: {}); escrow OPEN id {id} t0={t0} t1={t1} t2={t2} W={} base tx {tx}", joint.negated, s.reveal_secs);
    put(cx, &mut s, line)?;
    maybe_vanish(cx, "opened", &opts.vanish_at);
    Ok(())
}

/// Broadcast refund(b). b is public from here on, so the swap never returns to Ready; the stage
/// stays RefundSent (secrets kept) until the refund is FINAL, and a dropped refund is re-sent.
fn refund(cx: &mut Ctx, key: &str, s: &mut MakerSwap, id: &str, b: &pallas::Scalar, why: &str) -> Result<()> {
    let now = cx.evm.now()?;
    if s.stage == Stage::RefundSent && s.last_refund_at.map(|t| now < t + cx.cfg.inclusion_secs).unwrap_or(false) {
        return Ok(()); // the previous refund may still be pending
    }
    let again = s.stage == Stage::RefundSent;
    let tx = cx.evm.send(key, &cx.evm.escrow.clone(), "refund(bytes32,uint256)", &[id.to_string(), keys::scalar_evm(b)])?;
    s.stage = Stage::RefundSent;
    s.last_refund_at = Some(now);
    put(cx, s, format!("{why}; {} publishes b, tx {tx}; waiting for it to be final", if again { "refund was dropped; RE-SENT REFUND" } else { "REFUND" }))
}

/// Sweep the joint note with a + b to the maker's own wallet.
fn sweep(cx: &mut Ctx, s: &mut MakerSwap, n: wallet::Received, a: pallas::Scalar, b: pallas::Scalar, why: &str) -> Result<()> {
    let tip = cx.w.height;
    cx.cover_wait("SWEEP", tip, None);
    let v = n.note.value().inner();
    let fee = wallet::zip317_fee(1, 1);
    let to = cx.me.fvk.address_at(0u32, Scope::External);
    let txid = cx.w.send(vec![(n, SignWith::Joint(a + b))], vec![(to, v - fee, wallet::empty_memo(), None)])?;
    s.stage = Stage::Sweeping;
    s.sweep_height = Some(cx.w.height);
    put(cx, s, format!("{why}; swept {} zats with (a+b) to own wallet, ovk=None, tx {txid}; waiting for confirmations", v - fee))
}

fn advance(cx: &mut Ctx, opts: &Opts, key: &str, mut s: MakerSwap) -> Result<()> {
    let id = s.escrow_id.clone().ok_or_else(|| anyhow!("no escrow id"))?;
    let (t0, t1) = (s.t0.unwrap(), s.t1.unwrap());
    let d = cx.cfg.delta_small;
    let view = cx.evm.swap(&id)?;
    let now = cx.evm.now()?;
    let b = keys::normalize(keys::scalar_from_bytes(&unhex::<32>(&s.b)?)?, s.negated);

    // Sweep in flight: done once it has k_Z confirmations; re-sent if it expired unmined.
    if s.stage == Stage::Sweeping {
        cx.w.sync()?;
        let k_z = cx.cfg.k_z.unwrap_or(2);
        let tip = cx.w.height;
        match cx.w.spend_of(&s.rfq_id) {
            Some((tx, h)) if tip + 1 - h >= k_z => {
                s.stage = Stage::Swept;
                return put(cx, &mut s, format!("sweep {tx} has {} confirmations (k_Z = {k_z}); done", tip + 1 - h));
            }
            Some(_) => return Ok(()),
            None if tip > s.sweep_height.unwrap_or(tip) + 40 => {
                let (a_hex, _) = cx.evm.revealed("Claimed", &id)?.ok_or_else(|| anyhow!("Claimed without event"))?;
                let a = keys::scalar_from_evm(&a_hex)?;
                let n = cx.w.unspent(&s.rfq_id).into_iter().next().ok_or_else(|| anyhow!("joint note missing"))?;
                return sweep(cx, &mut s, n, a, b, "previous sweep expired unmined; RE-SENT");
            }
            None => return Ok(()),
        }
    }

    if view.state == State::Claimed {
        let (a_hex, bn) = cx.evm.revealed("Claimed", &id)?.ok_or_else(|| anyhow!("Claimed without event"))?;
        let a = keys::scalar_from_evm(&a_hex)?;
        let a_pub = keys::point_from_bytes(&unhex::<32>(&s.a_pub)?)?;
        let a_pub = if s.negated { -a_pub } else { a_pub };
        if keys::g() * a != a_pub {
            bail!("published a does not match A");
        }
        cx.w.sync()?;
        let Some(n) = cx.w.unspent(&s.rfq_id).into_iter().next() else {
            if view.t2 <= now && !s.log.iter().any(|l| l.contains("READY")) {
                // Only possible through the punish path while the maker was absent for all of [t1, t2).
                s.stage = Stage::Aborted("claimed via punish path without funding: L_M was violated".into());
                return put(cx, &mut s, format!("CLAIMED at base block {bn} via the punish path, but no joint note exists: the maker was offline for all of [t1, t2) (assumption L_M violated) and lost the escrowed tokens"));
            }
            bail!("claimed, but no unspent joint note to sweep yet");
        };
        // The sweep has no deadline, so it waits for cover to decorrelate it from the claim.
        return sweep(cx, &mut s, n, a, b, &format!("CLAIMED at base block {bn}: read a"));
    }
    if view.state == State::Refunded {
        // Our refund (or an earlier one) is on chain; finish once it is final.
        let (fin_n, _) = cx.evm.block_info("finalized")?;
        if cx.evm.swap_at(&id, Some(fin_n))?.state == State::Refunded {
            let maker = Evm::address_of(key)?;
            let wtx = cx.evm.send(key, &cx.evm.escrow.clone(), "withdraw(address,address)", &[cx.evm.token.clone(), maker])?;
            s.stage = Stage::Refunded;
            return put(cx, &mut s, format!("refund is final (block <= {fin_n}); tokens withdrawn, tx {wtx}"));
        }
        return Ok(());
    }

    // Deadline refunds are sent only when a FINALIZED block past the deadline shows that no
    // commit (or reveal) can still land, so refund(b) cannot revert and leak b.
    let (fin_n, fin_t) = cx.evm.block_info("finalized")?;
    match view.state {
        State::Open | State::Ready if s.stage == Stage::RefundSent && now >= t1 && now < view.t2 => {
            // b is already public and a reorg dropped the refund: re-send while the escrow can still
            // accept it. The budget limits only the first broadcast (Lemma 2; TLA+ mutation noResend).
            return refund(cx, key, &mut s, &id, &b, "deadline refund not on chain");
        }
        State::Open if s.stage == Stage::RefundSent && now < t0 => {
            return refund(cx, key, &mut s, &id, &b, "early refund not on chain");
        }
        State::Open | State::Ready if fin_t >= t1 + d => {
            // R3d: the refund must also land before t2, when the punish window opens.
            if !cx.cfg.final_before(now, view.t2) {
                if !s.log.last().map(|l| l.starts_with("L_M")).unwrap_or(false) {
                    put(cx, &mut s, format!("L_M violated: back online at {now}, too close to t2 {} to refund safely; the taker may now claim (the trade completes if she funded)", view.t2))?;
                }
                return Ok(());
            }
            let fin = cx.evm.swap_at(&id, Some(fin_n))?;
            if matches!(fin.state, State::Open | State::Ready) {
                return refund(cx, key, &mut s, &id, &b, &format!("finalized block {fin_n} (t={fin_t}) is past t1 with state {:?}", fin.state));
            }
        }
        State::Locked => {
            let deadline = view.locked_at + view.reveal_window;
            if fin_t >= deadline + d {
                let fin = cx.evm.swap_at(&id, Some(fin_n))?;
                if fin.state == State::Locked {
                    return refund(cx, key, &mut s, &id, &b, &format!("taker locked but did not reveal by {deadline} (finalized block {fin_n})"));
                }
            }
            return Ok(());
        }
        _ => {}
    }

    if view.state == State::Open {
        let need = s.lots as u64 * cx.cfg.lot_zats + wallet::zip317_fee(1, 1);
        let k_z = cx.cfg.k_z.unwrap_or(2);
        cx.w.sync()?;
        let tip = cx.w.height;
        let note = cx.w.unspent(&s.rfq_id).into_iter().next();
        let near_t0 = now + 2 * d >= t0 && now + d < t0;
        match &note {
            Some(n) if s.stage == Stage::Opened && n.note.value().inner() >= need && tip + 1 - n.height >= k_z && now + d < t0 => {
                if opts.evil.as_deref() == Some("never-ready") {
                    return Ok(());
                }
                // setReady is public and would otherwise follow the funding by exactly k_Z blocks;
                // delay it too, but never past t0 - 2*delta.
                let fund_h = n.height;
                cx.cover_wait("READY", fund_h, Some(t0.saturating_sub(2 * d)));
                if cx.evm.now()? + d >= t0 {
                    return put(cx, &mut s, "cover delay reached t0 before Ready; not setting Ready (the swap will refund after t1)".into());
                }
                let tx = cx.evm.send(key, &cx.evm.escrow.clone(), "setReady(bytes32)", &[id.clone()])?;
                s.stage = Stage::Ready;
                put(cx, &mut s, format!("joint note {} zats with {} confirmations (k_Z = {k_z}); READY base tx {tx}", n.note.value().inner(), tip + 1 - n.height))?;
                maybe_vanish(cx, "ready", &opts.vanish_at);
            }
            Some(n) if n.note.value().inner() < need && near_t0 => {
                // An early refund that lands late merely reverts: claims need Ready, so no loss.
                return refund(cx, key, &mut s, &id, &b, &format!("joint note underfunded ({} < {need}) before t0", n.note.value().inner()));
            }
            None if near_t0 => return refund(cx, key, &mut s, &id, &b, "no funding before t0"),
            _ => {}
        }
    }
    Ok(())
}
