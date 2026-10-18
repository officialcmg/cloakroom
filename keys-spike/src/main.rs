//! Cloakroom key spike.
//!
//! Proves, against the real `orchard` 0.16 crate (the code Zebra uses for consensus
//! verification), that:
//!   1. Two parties can each hold a share of an Orchard-protocol spend authorizing key,
//!      ask = a + b, and derive a valid Ironwood (NU6.3) shared address from
//!      FVK = (ak = A + B, nk, rivk).
//!   2. ZEC sent to that address is visible to both parties via the shared IVK.
//!   3. Whoever learns both shares can spend the note through the PCZT flow by
//!      supplying an *external* RedPallas signature made with (a + b) + alpha, and the
//!      resulting bundle passes proof, spend-auth and binding-signature verification
//!      under the post-NU6.3 circuit.
//!   4. One share alone cannot produce a valid signature.
//!   5. A sweep built with ovk = None is unrecoverable by the counterparty, while one
//!      built with the shared ovk is recoverable (the privacy pitfall we must avoid).
//!   6. Schnorr proofs of knowledge reject a rogue-key share.
//! It also emits test vectors for the Solidity Pallas verifier.

use blake2b_simd::Params;
use ff::{Field, FromUniformBytes, PrimeField};
use group::{Curve, Group, GroupEncoding};
use incrementalmerkletree::{Hashable, Marking, Retention};
use orchard::{
    builder::{Builder, BundleType},
    bundle::{Authorized, BundleVersion, Flags, TxVersion},
    circuit::{OrchardCircuitVersion, ProvingKey, VerifyingKey},
    keys::{FullViewingKey, PreparedIncomingViewingKey, Scope, SpendingKey},
    note::ExtractedNoteCommitment,
    note_encryption::IronwoodDomain,
    primitives::redpallas::{self, SpendAuth},
    tree::{MerkleHashOrchard, MerklePath},
    value::NoteValue,
    Bundle,
};
use pasta_curves::{arithmetic::CurveAffine, pallas};
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use shardtree::{store::memory::MemoryShardStore, ShardTree};
use std::time::Instant;
use zcash_note_encryption::{try_note_decryption, try_output_recovery_with_ovk};

#[allow(non_upper_case_globals)]
const OsRng: UnwrapErr<SysRng> = UnwrapErr(SysRng);

// ---------------------------------------------------------------- helpers

fn h64(domain: &[u8], parts: &[&[u8]]) -> [u8; 64] {
    let mut st = Params::new().hash_length(64).personal(b"Cloakroom_v0_H64").to_state();
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

fn hash_to_scalar(domain: &[u8], parts: &[&[u8]]) -> pallas::Scalar {
    pallas::Scalar::from_uniform_bytes(&h64(domain, parts))
}

fn hash_to_base(domain: &[u8], parts: &[&[u8]]) -> pallas::Base {
    pallas::Base::from_uniform_bytes(&h64(domain, parts))
}

/// G^Orchard, the RedPallas SpendAuth basepoint, recovered from the crate itself
/// (the verification key of the signing key with scalar 1).
fn spend_auth_g() -> pallas::Point {
    let one = redpallas::SigningKey::<SpendAuth>::try_from(pallas::Scalar::ONE.to_repr()).unwrap();
    let vk: [u8; 32] = (&redpallas::VerificationKey::from(&one)).into();
    pallas::Point::from_bytes(&vk).unwrap()
}

fn ytilde(p: &pallas::Point) -> u8 {
    p.to_bytes()[31] >> 7
}

fn be_hex_base(x: &pallas::Base) -> String {
    let mut b = x.to_repr();
    b.reverse();
    format!("0x{}", hex::encode(b))
}
fn be_hex_scalar(x: &pallas::Scalar) -> String {
    let mut b = x.to_repr();
    b.reverse();
    format!("0x{}", hex::encode(b))
}
fn affine(p: &pallas::Point) -> (String, String) {
    let a = p.to_affine();
    let c = a.coordinates().unwrap();
    (be_hex_base(c.x()), be_hex_base(c.y()))
}

/// Schnorr proof of knowledge of x for X = [x]G, bound to a context (swap id + role).
struct Pok {
    r: pallas::Point,
    s: pallas::Scalar,
}
fn pok_prove(g: &pallas::Point, x: &pallas::Scalar, ctx: &[u8]) -> Pok {
    let k = pallas::Scalar::random(&mut OsRng);
    let r = g * k;
    let xp = g * x;
    let c = hash_to_scalar(b"pok", &[ctx, &xp.to_bytes(), &r.to_bytes()]);
    Pok { r, s: k + c * x }
}
fn pok_verify(g: &pallas::Point, xp: &pallas::Point, pok: &Pok, ctx: &[u8]) -> bool {
    let c = hash_to_scalar(b"pok", &[ctx, &xp.to_bytes(), &pok.r.to_bytes()]);
    g * pok.s == pok.r + xp * c
}

fn single_leaf_witness(cmx: &ExtractedNoteCommitment) -> (MerkleHashOrchard, MerklePath) {
    let leaf = MerkleHashOrchard::from_cmx(cmx);
    let mut tree: ShardTree<MemoryShardStore<MerkleHashOrchard, u32>, 32, 16> =
        ShardTree::new(MemoryShardStore::empty(), 100);
    tree.append(leaf, Retention::Checkpoint { id: 0, marking: Marking::Marked }).unwrap();
    let root = tree.root_at_checkpoint_id(&0).unwrap().unwrap();
    let pos = tree.max_leaf_position(None).unwrap().unwrap();
    let path = tree.witness_at_checkpoint_id(pos, &0).unwrap().unwrap();
    (root, path.into())
}

fn verify_bundle(bundle: &Bundle<Authorized, i64>, vk: &VerifyingKey) -> [u8; 32] {
    bundle.verify_proof(vk).expect("halo2 proof verifies");
    let sighash: [u8; 32] = bundle.commitment(TxVersion::V6).unwrap().into();
    for action in bundle.actions() {
        action.rk().verify(&sighash, action.authorization()).expect("spendAuthSig verifies");
    }
    bundle
        .binding_validating_key()
        .verify(&sighash, bundle.authorization().binding_signature())
        .expect("bindingSig verifies");
    sighash
}

/// Sweep the joint note to `to` via the PCZT flow, signing externally with `ask_joint`.
/// Returns the authorized bundle, or the signer error if the key is wrong.
fn sweep(
    pk: &ProvingKey,
    joint_fvk: &FullViewingKey,
    note: orchard::Note,
    to: orchard::Address,
    ovk: Option<orchard::keys::OutgoingViewingKey>,
    ask_joint: &pallas::Scalar,
) -> Result<Bundle<Authorized, i64>, String> {
    let cmx: ExtractedNoteCommitment = note.commitment().into();
    let (root, path) = single_leaf_witness(&cmx);
    let fee = 10_000u64; // ZIP 317: 2 logical actions x 5,000 zats
    let v = note.value().inner();
    let version = BundleVersion::ironwood_v3();
    let mut b = Builder::new(BundleType::DEFAULT, version, version.default_flags(), root.into())
        .map_err(|e| format!("{e:?}"))?;
    b.add_spend(joint_fvk.clone(), note, path).map_err(|e| format!("{e:?}"))?;
    b.add_output(ovk, to, NoteValue::from_raw(v - fee), [0u8; 512]).map_err(|e| format!("{e:?}"))?;
    let (mut pczt, _meta) = b.build_for_pczt(OsRng).map_err(|e| format!("{e:?}"))?;

    // Sighash over the bundle's effects (in a full tx this is the ZIP 244 tx sighash).
    let sighash: [u8; 32] = pczt
        .extract_effects::<i64>()
        .unwrap()
        .unwrap()
        .commitment(TxVersion::V6)
        .unwrap()
        .into();

    pczt.finalize_io(sighash, OsRng).map_err(|e| format!("{e:?}"))?; // signs dummy spends
    let t = Instant::now();
    pczt.create_proof(pk, OsRng).map_err(|e| format!("{e:?}"))?;
    println!("      halo2 proof created in {:.2?}", t.elapsed());

    let sk = redpallas::SigningKey::<SpendAuth>::try_from(ask_joint.to_repr()).unwrap();
    for action in pczt.actions_mut() {
        if action.spend().fvk().as_ref() == Some(joint_fvk) {
            let alpha = action.spend().alpha().expect("constructor sets alpha");
            let sig = sk.randomize(&alpha).sign(OsRng, &sighash); // rsk = ask + alpha
            action.apply_signature(sighash, sig).map_err(|e| format!("{e:?}"))?;
        }
    }
    let unbound = pczt.extract::<i64>().map_err(|e| format!("{e:?}"))?.unwrap();
    unbound.apply_binding_signature(sighash, OsRng).ok_or_else(|| "binding failed".into())
}

// ---------------------------------------------------------------- main

fn main() {
    let g = spend_auth_g();
    let (gx, gy) = affine(&g);
    println!("== Cloakroom key spike (orchard 0.16.0, post-NU6.3 / Ironwood) ==");
    println!("G^Orchard (SpendAuth basepoint)\n  x = {gx}\n  y = {gy}");

    let swap_id = b"cloakroom-demo-swap-0001";

    // ---------- 1. key generation with proofs of knowledge
    let mut a = pallas::Scalar::random(&mut OsRng);
    let mut b = pallas::Scalar::random(&mut OsRng);
    let pok_a = pok_prove(&g, &a, &[&swap_id[..], b"alice"].concat());
    let pok_b = pok_prove(&g, &b, &[&swap_id[..], b"bob"].concat());
    let (mut big_a, mut big_b) = (g * a, g * b);
    assert!(pok_verify(&g, &big_a, &pok_a, &[&swap_id[..], b"alice"].concat()));
    assert!(pok_verify(&g, &big_b, &pok_b, &[&swap_id[..], b"bob"].concat()));
    // A proof made for the "alice" role must not verify as "bob" (no replay).
    assert!(!pok_verify(&g, &big_a, &pok_a, &[&swap_id[..], b"bob"].concat()));
    println!("\n[1] shares generated; Schnorr PoKs verify; cross-role replay rejected");

    // Orchard requires ytilde(ak) = 0. Both parties deterministically negate if needed.
    let mut negated = false;
    if ytilde(&(big_a + big_b)) == 1 {
        a = -a;
        b = -b;
        big_a = -big_a;
        big_b = -big_b;
        negated = true;
    }
    let ak = big_a + big_b;
    assert_eq!(ytilde(&ak), 0);
    println!("    ak = A + B, ytilde(ak) = 0 (shares negated: {negated})");

    // Statistics on the sign rule over many trials.
    let mut neg = 0;
    for _ in 0..2000 {
        let p = g * pallas::Scalar::random(&mut OsRng) + g * pallas::Scalar::random(&mut OsRng);
        neg += ytilde(&p) as u32;
    }
    println!("    sign rule: {neg}/2000 random joint keys needed negation (~50% expected)");

    // ---------- 2. jointly derived nk, rivk -> FVK -> shared Ironwood address
    let (seed_a, seed_b) = ([7u8; 32], [9u8; 32]); // commit-revealed nonces in the real protocol
    let nk = hash_to_base(b"nk", &[swap_id, &seed_a, &seed_b]);
    let rivk = hash_to_scalar(b"rivk", &[swap_id, &seed_a, &seed_b]);
    let mut fvk_bytes = [0u8; 96];
    fvk_bytes[..32].copy_from_slice(&ak.to_bytes());
    fvk_bytes[32..64].copy_from_slice(&nk.to_repr());
    fvk_bytes[64..].copy_from_slice(&rivk.to_repr());
    let joint_fvk = FullViewingKey::from_bytes(&fvk_bytes).expect("valid Orchard FVK");
    let joint_addr = joint_fvk.address_at(0u32, Scope::External);
    println!("\n[2] joint FVK accepted by orchard::FullViewingKey::from_bytes");
    println!("    joint raw address = {}", hex::encode(joint_addr.to_raw_address_bytes()));

    // ---------- circuit keys (post-NU6.3)
    let t = Instant::now();
    let pk = ProvingKey::build(OrchardCircuitVersion::PostNu6_3);
    let vk = VerifyingKey::build(OrchardCircuitVersion::PostNu6_3);
    println!("\n    post-NU6.3 proving/verifying keys built in {:.2?}", t.elapsed());

    // ---------- 3. Alice funds the joint address (output-only Ironwood bundle)
    let lot = 1_000_010_000u64; // 10 ZEC lot + 10,000 zat sweep fee
    let mut memo = [0u8; 512];
    memo[..24].copy_from_slice(swap_id);
    let funding: Bundle<Authorized, i64> = {
        let anchor = MerkleHashOrchard::empty_root(32.into()).into();
        let mut bld =
            Builder::new(BundleType::DEFAULT, BundleVersion::ironwood_v3(), Flags::SPENDS_DISABLED, anchor)
                .unwrap();
        bld.add_output(None, joint_addr, NoteValue::from_raw(lot), memo).unwrap();
        let (unauth, _) = bld.build::<i64>(OsRng).unwrap().unwrap();
        let sighash = unauth.commitment(TxVersion::V6).unwrap().into();
        unauth.create_proof(&pk, OsRng).unwrap().apply_signatures(OsRng, sighash, &[]).unwrap()
    };
    verify_bundle(&funding, &vk);
    println!("\n[3] Alice's funding bundle (Ironwood v3) verifies: proof + sigs + binding");

    // ---------- 4. Bob (and Alice) detect the note with the joint IVK
    let ivk = PreparedIncomingViewingKey::new(&joint_fvk.to_ivk(Scope::External));
    let (note, _addr, got_memo) = funding
        .actions()
        .iter()
        .find_map(|act| try_note_decryption(&IronwoodDomain::for_action(act), &ivk, act))
        .expect("joint IVK decrypts the funding output");
    assert_eq!(note.value().inner(), lot);
    assert_eq!(&got_memo[..24], swap_id);
    println!("[4] joint IVK decrypts note: value = {} zats, memo = {:?}",
        note.value().inner(), std::str::from_utf8(&got_memo[..24]).unwrap());

    // ---------- 5. one share alone cannot authorize the spend
    let bob_sk = SpendingKey::from_bytes([42u8; 32]).unwrap();
    let bob_addr = FullViewingKey::from(&bob_sk).address_at(0u32, Scope::External);
    match sweep(&pk, &joint_fvk, note.clone(), bob_addr, None, &b) {
        Err(e) => println!("\n[5] sweep signed with b alone REJECTED: {e}"),
        Ok(_) => panic!("single share must not work"),
    }

    // ---------- 6. Alice claims on Base (reveals a); Bob sweeps with a + b, ovk = None
    let swept = sweep(&pk, &joint_fvk, note.clone(), bob_addr, None, &(a + b)).expect("sweep");
    verify_bundle(&swept, &vk);
    println!("[6] Bob's sweep with (a+b)+alpha via PCZT apply_signature VERIFIES under post-NU6.3 circuit");

    // ---------- 7. OVK privacy check
    let joint_ovk = joint_fvk.to_ovk(Scope::External);
    let recovered_none = swept.actions().iter().any(|act| {
        try_output_recovery_with_ovk(&IronwoodDomain::for_action(act), &joint_ovk, act, act.cv_net(), &act.encrypted_note().out_ciphertext).is_some()
    });
    let leaky = sweep(&pk, &joint_fvk, note.clone(), bob_addr, Some(joint_ovk.clone()), &(a + b)).unwrap();
    let recovered_leaky = leaky.actions().iter().any(|act| {
        try_output_recovery_with_ovk(&IronwoodDomain::for_action(act), &joint_ovk, act, act.cv_net(), &act.encrypted_note().out_ciphertext).is_some()
    });
    println!("\n[7] counterparty recovers sweep destination with joint ovk?  ovk=None: {recovered_none}   ovk=joint: {recovered_leaky}");
    assert!(!recovered_none && recovered_leaky);

    // ---------- 8. refund path: b is revealed, Alice sweeps back to herself
    let alice_sk = SpendingKey::from_bytes([24u8; 32]).unwrap();
    let alice_addr = FullViewingKey::from(&alice_sk).address_at(0u32, Scope::External);
    let refunded = sweep(&pk, &joint_fvk, note, alice_addr, None, &(a + b)).unwrap();
    verify_bundle(&refunded, &vk);
    println!("[8] refund path: Alice's recovery sweep verifies");

    // ---------- 9. rogue-key attack is caught by PoK
    let b_prime = pallas::Scalar::random(&mut OsRng);
    let rogue_b = g * b_prime - big_a; // Bob would solely control ak = [b']G
    let fake = pok_prove(&g, &b_prime, &[&swap_id[..], b"bob"].concat());
    println!("\n[9] rogue key B = [b']G - A: PoK verifies? {}",
        pok_verify(&g, &rogue_b, &fake, &[&swap_id[..], b"bob"].concat()));

    // ---------- 10. Rerandomization: rk published on chain differs from ak
    let rk_bytes: Vec<[u8; 32]> = swept.actions().iter().map(|act| act.rk().into()).collect();
    let ak_bytes = ak.to_bytes();
    println!("[10] ak appears verbatim in any on-chain rk? {}", rk_bytes.iter().any(|r| *r == ak_bytes));

    // ---------- Solidity vectors
    let (ax, ay) = affine(&big_a);
    let (bx, by) = affine(&big_b);
    println!("\n== Solidity test vectors ==");
    println!("a  = {}\nAx = {ax}\nAy = {ay}", be_hex_scalar(&a));
    println!("b  = {}\nBx = {bx}\nBy = {by}", be_hex_scalar(&b));
    let _ = pallas::Point::generator();
}
