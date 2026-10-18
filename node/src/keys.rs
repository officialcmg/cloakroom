//! Pallas key shares, Schnorr proofs of knowledge, and joint Ironwood key derivation.

use anyhow::{anyhow, bail, Result};
use blake2b_simd::Params;
use ff::{Field, FromUniformBytes, PrimeField};
use group::{Curve, GroupEncoding};
use orchard::{
    keys::FullViewingKey,
    primitives::redpallas::{self, SpendAuth},
};
use pasta_curves::{arithmetic::CurveAffine, pallas};

use crate::OsRng;

pub fn h64(domain: &[u8], parts: &[&[u8]]) -> [u8; 64] {
    let mut st = Params::new().hash_length(64).personal(b"Cloakroom_v1_H64").to_state();
    st.update(&(domain.len() as u64).to_le_bytes());
    st.update(domain);
    for p in parts {
        st.update(&(p.len() as u64).to_le_bytes());
        st.update(p);
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(st.finalize().as_bytes());
    out
}

pub fn h32(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&h64(domain, parts)[..32]);
    out
}

fn hash_to_scalar(domain: &[u8], parts: &[&[u8]]) -> pallas::Scalar {
    pallas::Scalar::from_uniform_bytes(&h64(domain, parts))
}

fn hash_to_base(domain: &[u8], parts: &[&[u8]]) -> pallas::Base {
    pallas::Base::from_uniform_bytes(&h64(domain, parts))
}

/// G^Orchard, the RedPallas SpendAuth basepoint (verification key of scalar 1).
pub fn g() -> pallas::Point {
    let one = redpallas::SigningKey::<SpendAuth>::try_from(pallas::Scalar::ONE.to_repr()).unwrap();
    let vk: [u8; 32] = (&redpallas::VerificationKey::from(&one)).into();
    pallas::Point::from_bytes(&vk).unwrap()
}

pub fn ytilde(p: &pallas::Point) -> u8 {
    p.to_bytes()[31] >> 7
}

pub fn random_scalar() -> pallas::Scalar {
    loop {
        let s = pallas::Scalar::random(&mut { OsRng });
        if !bool::from(s.is_zero()) {
            return s;
        }
    }
}

pub fn point_from_bytes(b: &[u8; 32]) -> Result<pallas::Point> {
    Option::from(pallas::Point::from_bytes(b)).ok_or_else(|| anyhow!("invalid Pallas point encoding"))
}

pub fn scalar_to_bytes(s: &pallas::Scalar) -> [u8; 32] {
    s.to_repr()
}

pub fn scalar_from_bytes(b: &[u8; 32]) -> Result<pallas::Scalar> {
    Option::from(pallas::Scalar::from_repr(*b)).ok_or_else(|| anyhow!("non-canonical scalar"))
}

fn be_hex(mut le: [u8; 32]) -> String {
    le.reverse();
    format!("0x{}", hex::encode(le))
}

/// Big-endian hex of a scalar, as the EVM escrow expects for claim(a) / refund(b).
pub fn scalar_evm(s: &pallas::Scalar) -> String {
    be_hex(s.to_repr())
}

/// Parse a uint256 (decimal or 0x-hex, big-endian) into a Pallas scalar.
pub fn scalar_from_evm(v: &str) -> Result<pallas::Scalar> {
    let be = u256_be_bytes(v)?;
    let mut le = be;
    le.reverse();
    scalar_from_bytes(&le)
}

/// Affine (x, y) of a point as big-endian hex, for the EVM escrow.
pub fn affine_evm(p: &pallas::Point) -> (String, String) {
    let a = p.to_affine();
    let c = a.coordinates().unwrap();
    (be_hex(c.x().to_repr()), be_hex(c.y().to_repr()))
}

/// uint256 string (decimal or hex) -> 32 big-endian bytes.
pub fn u256_be_bytes(v: &str) -> Result<[u8; 32]> {
    let v = v.trim();
    let mut out = [0u8; 32];
    if let Some(h) = v.strip_prefix("0x") {
        let h = format!("{:0>64}", h);
        out.copy_from_slice(&hex::decode(h)?);
    } else {
        // decimal -> big-endian via repeated multiply-add on a byte array
        for ch in v.chars() {
            let d = ch.to_digit(10).ok_or_else(|| anyhow!("bad decimal {v}"))? as u32;
            let mut carry = d;
            for byte in out.iter_mut().rev() {
                let x = (*byte as u32) * 10 + carry;
                *byte = (x & 0xff) as u8;
                carry = x >> 8;
            }
            if carry != 0 {
                bail!("uint256 overflow");
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- proofs of knowledge

/// Schnorr proof of knowledge of x for X = [x]G, bound to (swap id, role).
pub fn pok_prove(x: &pallas::Scalar, sid: &[u8; 16], role: &[u8]) -> [u8; 64] {
    let gg = g();
    let k = random_scalar();
    let r = gg * k;
    let xp = gg * x;
    let c = hash_to_scalar(b"pok", &[sid, role, &xp.to_bytes(), &r.to_bytes()]);
    let s = k + c * x;
    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&r.to_bytes());
    out[32..].copy_from_slice(&s.to_repr());
    out
}

pub fn pok_verify(xp: &pallas::Point, pok: &[u8; 64], sid: &[u8; 16], role: &[u8]) -> bool {
    let gg = g();
    let r_bytes: [u8; 32] = pok[..32].try_into().unwrap();
    let s_bytes: [u8; 32] = pok[32..].try_into().unwrap();
    let (Ok(r), Ok(s)) = (point_from_bytes(&r_bytes), scalar_from_bytes(&s_bytes)) else {
        return false;
    };
    let c = hash_to_scalar(b"pok", &[sid, role, &xp.to_bytes(), &r.to_bytes()]);
    gg * s == r + xp * c
}

// ---------------------------------------------------------------- joint key

pub struct Joint {
    /// True if both shares had to be negated so that ytilde(A + B) = 0.
    pub negated: bool,
    /// Normalized public shares, as stored in the escrow.
    pub a_pub: pallas::Point,
    pub b_pub: pallas::Point,
    pub fvk: FullViewingKey,
}

/// Derive the joint full viewing key from the (un-normalized) public shares and both seeds.
///
/// nk and rivk come from both parties' seeds. This is the ZIP 2005 `use_qsk = false`
/// branch: Ironwood accepts it, but notes at the joint address are not quantum-recoverable.
/// Escrow salt, so the public swap id keccak(maker, salt) cannot be computed from the RFQ id that
/// travels in the memo envelopes: a later viewing-key compromise then cannot link a Zcash wallet
/// to its escrow (s_T and s_M are only ever sent encrypted under erased ephemeral keys).
/// Digest a maker signs with its EVM key to publish (unified address, RFQ epoch, RFQ key).
pub fn directory_digest(ua: &str, epoch: u32, rfq_pub: &[u8; 32], evm: &str) -> [u8; 32] {
    h32(b"maker-directory", &[ua.as_bytes(), &epoch.to_le_bytes(), rfq_pub, evm.to_lowercase().as_bytes()])
}

pub fn escrow_salt(sid: &[u8; 16], s_t: &[u8; 32], s_m: &[u8; 32]) -> String {
    format!("0x{}", hex::encode(h32(b"escrow-salt", &[sid, s_t, s_m])))
}

pub fn joint(sid: &[u8; 16], a_pub: pallas::Point, b_pub: pallas::Point, s_t: &[u8; 32], s_m: &[u8; 32]) -> Result<Joint> {
    let mut a_pub = a_pub;
    let mut b_pub = b_pub;
    let negated = ytilde(&(a_pub + b_pub)) == 1;
    if negated {
        a_pub = -a_pub;
        b_pub = -b_pub;
    }
    let ak = a_pub + b_pub;
    debug_assert_eq!(ytilde(&ak), 0);
    let nk = hash_to_base(b"nk", &[sid, s_t, s_m]);
    let rivk = hash_to_scalar(b"rivk", &[sid, s_t, s_m]);
    let mut fvk_bytes = [0u8; 96];
    fvk_bytes[..32].copy_from_slice(&ak.to_bytes());
    fvk_bytes[32..64].copy_from_slice(&nk.to_repr());
    fvk_bytes[64..].copy_from_slice(&rivk.to_repr());
    let fvk = FullViewingKey::from_bytes(&fvk_bytes)
        .ok_or_else(|| anyhow!("joint FVK invalid (ivk = 0 or bottom); renegotiate seeds"))?;
    Ok(Joint { negated, a_pub, b_pub, fvk })
}

/// Apply the normalization to one's own share.
pub fn normalize(share: pallas::Scalar, negated: bool) -> pallas::Scalar {
    if negated { -share } else { share }
}

pub fn sign_spend(ask: &pallas::Scalar, alpha: &pallas::Scalar, sighash: &[u8; 32]) -> redpallas::Signature<SpendAuth> {
    redpallas::SigningKey::<SpendAuth>::try_from(ask.to_repr())
        .expect("nonzero scalar")
        .randomize(alpha)
        .sign(OsRng, sighash)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both parties' view of one swap, in both ytilde branches, for the claim and refund paths.
    #[test]
    fn normalization_claim_and_refund_paths() {
        let gg = g();
        let (mut saw_neg, mut saw_pos) = (false, false);
        for i in 0..400u32 {
            let sid = [i as u8; 16];
            let (a, b) = (random_scalar(), random_scalar());
            let (s_t, s_m) = ([1u8; 32], [2u8; 32]);
            // each side derives the joint key from public data only
            let t_view = joint(&sid, gg * a, gg * b, &s_t, &s_m).unwrap();
            let m_view = joint(&sid, gg * a, gg * b, &s_t, &s_m).unwrap();
            assert_eq!(t_view.fvk.to_bytes(), m_view.fvk.to_bytes());
            assert_eq!(t_view.negated, m_view.negated);
            let (a_n, b_n) = (normalize(a, t_view.negated), normalize(b, m_view.negated));
            // escrow doors: [a_n]G == stored A, [b_n]G == stored B
            assert_eq!(gg * a_n, t_view.a_pub);
            assert_eq!(gg * b_n, m_view.b_pub);
            // either path yields the joint spend key whose public key is ak in the FVK
            let ak_bytes: [u8; 32] = t_view.fvk.to_bytes()[..32].try_into().unwrap();
            assert_eq!((gg * (a_n + b_n)).to_bytes(), ak_bytes);
            // EVM encoding round trip of the revealed scalar
            assert_eq!(scalar_from_evm(&scalar_evm(&a_n)).unwrap(), a_n);
            assert_eq!(scalar_from_evm(&scalar_evm(&b_n)).unwrap(), b_n);
            if t_view.negated { saw_neg = true } else { saw_pos = true }
        }
        assert!(saw_neg && saw_pos, "both branches must be exercised");
    }

    #[test]
    fn pok_rejects_rogue_and_replay() {
        let gg = g();
        let sid = [9u8; 16];
        let a = random_scalar();
        let pa = pok_prove(&a, &sid, b"taker");
        assert!(pok_verify(&(gg * a), &pa, &sid, b"taker"));
        assert!(!pok_verify(&(gg * a), &pa, &sid, b"maker"));
        assert!(!pok_verify(&(gg * a), &pa, &[8u8; 16], b"taker"));
        let b2 = random_scalar();
        let rogue = gg * b2 - gg * a;
        assert!(!pok_verify(&rogue, &pok_prove(&b2, &sid, b"maker"), &sid, b"maker"));
    }
}
