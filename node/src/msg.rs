//! Cloakroom RFQ messages, carried in 512-byte shielded memos (ZIP 302 type 0xFF).
//!
//! Payloads are end-to-end encrypted on top of Zcash note encryption, so that someone who
//! later obtains a party's incoming viewing key (an auditor, or a breach) cannot read past
//! RFQs. The RFQ is encrypted to the maker's current epoch key; Quote and Accept are
//! encrypted under an ephemeral Diffie-Hellman between the two parties' per-swap keys, which
//! are erased when the swap ends. Keys: BLAKE2b-256 over (label, shared point, both public
//! keys); cipher: ChaCha20-Poly1305 with a zero nonce (every key is used once).

use anyhow::{anyhow, bail, Result};
use blake2b_simd::Params;
use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Key, Nonce};
use group::{Group, GroupEncoding};
use pasta_curves::pallas;

pub const MAGIC: &[u8; 4] = b"CLKR";
pub const VERSION: u8 = 2;
const T_RFQ: u8 = 1;
const T_QUOTE: u8 = 2;
const T_ACCEPT: u8 = 3;
const T_FUND: u8 = 4;
const HDR: usize = 7;

#[derive(Clone, Debug)]
pub struct Rfq {
    pub rfq_id: [u8; 16],
    pub lots: u8,
    pub a_pub: [u8; 32],
    pub pok_a: [u8; 64],
    pub commit_s_t: [u8; 32],
    pub payout: [u8; 20],
    pub reply_addr: [u8; 43],
}

#[derive(Clone, Debug)]
pub struct Quote {
    pub price: u64,       // token base units for the whole RFQ
    pub relayer_fee: u64, // paid from `price` to the relayer that submits the commit
    pub premium: u64,     // zatoshis the taker pays the maker with Accept (non-refundable)
    pub b_pub: [u8; 32],
    pub pok_b: [u8; 64],
    pub s_m: [u8; 32],
    pub escrow: [u8; 20],
    pub maker_evm: [u8; 20],
    pub t0_secs: u32,     // t0 = open time + t0_secs
    pub t1_secs: u32,     // t1 = t0 + t1_secs
    pub t2_secs: u32,     // t2 = t1 + t2_secs (punish window opens)
    pub reveal_secs: u32, // reveal window after commitClaim
    /// Maker's EIP-191 signature (65 bytes) over sig_digest(): transferable evidence of the quote,
    /// and proof that it comes from the EVM address the taker expects (round-2 review M5).
    pub sig: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Accept {
    pub s_t: [u8; 32],
    pub quote_hash: [u8; 32],
}

/// What travels in a memo. Only routing data is in the clear.
#[derive(Clone, Debug)]
pub enum Envelope {
    Rfq { epoch: u32, eph: [u8; 32], ct: Vec<u8> },
    Quote { rfq_id: [u8; 16], eph: [u8; 32], ct: Vec<u8> },
    Accept { rfq_id: [u8; 16], ct: Vec<u8> },
    Fund { rfq_id: [u8; 16] },
}

fn header(t: u8) -> Vec<u8> {
    let mut v = vec![0xFF];
    v.extend_from_slice(MAGIC);
    v.push(VERSION);
    v.push(t);
    v
}

fn to_memo(v: Vec<u8>) -> [u8; 512] {
    assert!(v.len() <= 512, "memo too long");
    let mut m = [0u8; 512];
    m[..v.len()].copy_from_slice(&v);
    m
}

impl Envelope {
    pub fn encode(&self) -> [u8; 512] {
        let mut v;
        match self {
            Envelope::Rfq { epoch, eph, ct } => {
                v = header(T_RFQ);
                v.extend_from_slice(&epoch.to_le_bytes());
                v.extend_from_slice(eph);
                v.extend_from_slice(&(ct.len() as u16).to_le_bytes());
                v.extend_from_slice(ct);
            }
            Envelope::Quote { rfq_id, eph, ct } => {
                v = header(T_QUOTE);
                v.extend_from_slice(rfq_id);
                v.extend_from_slice(eph);
                v.extend_from_slice(&(ct.len() as u16).to_le_bytes());
                v.extend_from_slice(ct);
            }
            Envelope::Accept { rfq_id, ct } => {
                v = header(T_ACCEPT);
                v.extend_from_slice(rfq_id);
                v.extend_from_slice(&(ct.len() as u16).to_le_bytes());
                v.extend_from_slice(ct);
            }
            Envelope::Fund { rfq_id } => {
                v = header(T_FUND);
                v.extend_from_slice(rfq_id);
            }
        }
        to_memo(v)
    }

    /// Ok(None) for memos that are not Cloakroom messages.
    pub fn decode(m: &[u8; 512]) -> Result<Option<Envelope>> {
        if m[0] != 0xFF || &m[1..5] != MAGIC {
            return Ok(None);
        }
        if m[5] != VERSION {
            bail!("unsupported Cloakroom message version {}", m[5]);
        }
        let b = &m[HDR..];
        let take_ct = |off: usize| -> Result<Vec<u8>> {
            let n = u16::from_le_bytes([b[off], b[off + 1]]) as usize;
            if off + 2 + n > b.len() {
                bail!("truncated message");
            }
            Ok(b[off + 2..off + 2 + n].to_vec())
        };
        Ok(Some(match m[6] {
            T_RFQ => Envelope::Rfq { epoch: u32::from_le_bytes(b[..4].try_into()?), eph: b[4..36].try_into()?, ct: take_ct(36)? },
            T_QUOTE => Envelope::Quote { rfq_id: b[..16].try_into()?, eph: b[16..48].try_into()?, ct: take_ct(48)? },
            T_ACCEPT => Envelope::Accept { rfq_id: b[..16].try_into()?, ct: take_ct(16)? },
            T_FUND => Envelope::Fund { rfq_id: b[..16].try_into()? },
            t => bail!("unknown Cloakroom message type {t}"),
        }))
    }

    pub fn wire_len(&self) -> usize {
        let e = self.encode();
        512 - e.iter().rev().take_while(|b| **b == 0).count()
    }
}

// ---------------------------------------------------------------- payload codecs

impl Rfq {
    pub fn to_bytes(&self) -> Vec<u8> {
        [&self.rfq_id[..], &[self.lots], &self.a_pub, &self.pok_a, &self.commit_s_t, &self.payout, &self.reply_addr].concat()
    }
    pub fn from_bytes(b: &[u8]) -> Result<Rfq> {
        if b.len() != 16 + 1 + 32 + 64 + 32 + 20 + 43 {
            bail!("bad RFQ length");
        }
        Ok(Rfq {
            rfq_id: b[0..16].try_into()?,
            lots: b[16],
            a_pub: b[17..49].try_into()?,
            pok_a: b[49..113].try_into()?,
            commit_s_t: b[113..145].try_into()?,
            payout: b[145..165].try_into()?,
            reply_addr: b[165..208].try_into()?,
        })
    }
}

impl Quote {
    pub fn to_bytes(&self) -> Vec<u8> {
        [self.unsigned_bytes(), self.sig.clone()].concat()
    }
    pub fn sig_digest(&self, rfq_id: &[u8; 16]) -> [u8; 32] {
        crate::keys::h32(b"quote-sig", &[rfq_id, &self.unsigned_bytes()])
    }
    pub fn unsigned_bytes(&self) -> Vec<u8> {
        [
            &self.price.to_le_bytes()[..],
            &self.relayer_fee.to_le_bytes(),
            &self.premium.to_le_bytes(),
            &self.b_pub,
            &self.pok_b,
            &self.s_m,
            &self.escrow,
            &self.maker_evm,
            &self.t0_secs.to_le_bytes(),
            &self.t1_secs.to_le_bytes(),
            &self.t2_secs.to_le_bytes(),
            &self.reveal_secs.to_le_bytes(),
        ]
        .concat()
    }
    pub fn from_bytes(b: &[u8]) -> Result<Quote> {
        if b.len() != 8 * 3 + 32 + 64 + 32 + 20 + 20 + 16 + 65 {
            bail!("bad Quote length");
        }
        let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        Ok(Quote {
            price: u64_at(0),
            relayer_fee: u64_at(8),
            premium: u64_at(16),
            b_pub: b[24..56].try_into()?,
            pok_b: b[56..120].try_into()?,
            s_m: b[120..152].try_into()?,
            escrow: b[152..172].try_into()?,
            maker_evm: b[172..192].try_into()?,
            t0_secs: u32_at(192),
            t1_secs: u32_at(196),
            t2_secs: u32_at(200),
            reveal_secs: u32_at(204),
            sig: b[208..273].to_vec(),
        })
    }
}

impl Accept {
    pub fn to_bytes(&self) -> Vec<u8> {
        [&self.s_t[..], &self.quote_hash].concat()
    }
    pub fn from_bytes(b: &[u8]) -> Result<Accept> {
        if b.len() != 64 {
            bail!("bad Accept length");
        }
        Ok(Accept { s_t: b[..32].try_into()?, quote_hash: b[32..].try_into()? })
    }
}

// ---------------------------------------------------------------- end-to-end encryption

/// A Diffie-Hellman key on Pallas (generator = the standard Pallas generator).
pub fn dh_public(sk: &pallas::Scalar) -> [u8; 32] {
    (pallas::Point::generator() * sk).to_bytes()
}

fn kdf(label: &[u8], shared: &pallas::Point, a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut st = Params::new().hash_length(32).personal(b"CloakroomAEADv2_").to_state();
    for part in [label, &shared.to_bytes()[..], a, b] {
        st.update(&(part.len() as u64).to_le_bytes());
        st.update(part);
    }
    st.finalize().as_bytes().try_into().unwrap()
}

/// shared = [my_sk] their_pub; key bound to both public keys (ordered: initiator, responder).
fn session_key(label: &[u8], my_sk: &pallas::Scalar, their_pub: &[u8; 32], initiator_pub: &[u8; 32], responder_pub: &[u8; 32]) -> Result<[u8; 32]> {
    let p: pallas::Point = Option::from(pallas::Point::from_bytes(their_pub)).ok_or_else(|| anyhow!("bad DH public key"))?;
    if bool::from(p.is_identity()) {
        bail!("identity DH key");
    }
    Ok(kdf(label, &(p * my_sk), initiator_pub, responder_pub))
}

pub fn seal(label: &[u8], my_sk: &pallas::Scalar, their_pub: &[u8; 32], init_pub: &[u8; 32], resp_pub: &[u8; 32], pt: &[u8]) -> Result<Vec<u8>> {
    let k = session_key(label, my_sk, their_pub, init_pub, resp_pub)?;
    ChaCha20Poly1305::new(Key::from_slice(&k)).encrypt(Nonce::from_slice(&[0u8; 12]), pt).map_err(|_| anyhow!("encrypt"))
}

pub fn open(label: &[u8], my_sk: &pallas::Scalar, their_pub: &[u8; 32], init_pub: &[u8; 32], resp_pub: &[u8; 32], ct: &[u8]) -> Result<Vec<u8>> {
    let k = session_key(label, my_sk, their_pub, init_pub, resp_pub)?;
    ChaCha20Poly1305::new(Key::from_slice(&k)).decrypt(Nonce::from_slice(&[0u8; 12]), ct).map_err(|_| anyhow!("decryption failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::random_scalar;

    #[test]
    fn rfq_quote_accept_roundtrip_and_sizes() {
        let (maker_epoch, e_t, e_m) = (random_scalar(), random_scalar(), random_scalar());
        let (pm, pt, pq) = (dh_public(&maker_epoch), dh_public(&e_t), dh_public(&e_m));
        let rfq = Rfq { rfq_id: [1; 16], lots: 2, a_pub: [3; 32], pok_a: [4; 64], commit_s_t: [5; 32], payout: [6; 20], reply_addr: [7; 43] };
        // RFQ: taker -> maker epoch key
        let ct = seal(b"rfq", &e_t, &pm, &pt, &pm, &rfq.to_bytes()).unwrap();
        let env = Envelope::Rfq { epoch: 7, eph: pt, ct };
        let wire = env.encode();
        let Envelope::Rfq { eph, ct, .. } = Envelope::decode(&wire).unwrap().unwrap() else { panic!() };
        let got = Rfq::from_bytes(&open(b"rfq", &maker_epoch, &eph, &eph, &pm, &ct).unwrap()).unwrap();
        assert_eq!(got.to_bytes(), rfq.to_bytes());
        // the wrong key cannot read it
        assert!(open(b"rfq", &random_scalar(), &eph, &eph, &pm, &ct).is_err());
        // Quote: maker ephemeral -> taker ephemeral; Accept on the same pair, different label
        let q = Quote { price: 9, relayer_fee: 1, premium: 50_000, b_pub: [3; 32], pok_b: [4; 64], s_m: [5; 32], escrow: [6; 20], maker_evm: [8; 20], t0_secs: 60, t1_secs: 600, t2_secs: 7200, reveal_secs: 3600, sig: vec![7; 65] };
        let qct = seal(b"quote", &e_m, &pt, &pt, &pq, &q.to_bytes()).unwrap();
        let qenv = Envelope::Quote { rfq_id: [1; 16], eph: pq, ct: qct.clone() };
        assert_eq!(Quote::from_bytes(&open(b"quote", &e_t, &pq, &pt, &pq, &qct).unwrap()).unwrap().to_bytes(), q.to_bytes());
        let a = Accept { s_t: [9; 32], quote_hash: [8; 32] };
        let act = seal(b"accept", &e_t, &pq, &pt, &pq, &a.to_bytes()).unwrap();
        assert_eq!(open(b"accept", &e_m, &pt, &pt, &pq, &act).unwrap(), a.to_bytes());
        assert!(open(b"quote", &e_m, &pt, &pt, &pq, &act).is_err(), "labels separate keys");
        let aenv = Envelope::Accept { rfq_id: [1; 16], ct: act };
        println!("wire sizes: rfq {} quote {} accept {}", env.wire_len(), qenv.wire_len(), aenv.wire_len());
        assert!(env.wire_len() <= 512 && qenv.wire_len() <= 512 && aenv.wire_len() <= 512);
    }
}
