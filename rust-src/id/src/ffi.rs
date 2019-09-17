use crate::{
    chain::{self, CDIVerificationError},
    types::*,
};
use curve_arithmetic::curve_arithmetic::*;
use pairing::bls12_381::{Bls12, G1};
use pedersen_scheme::key::CommitmentKey as PedersenKey;
use std::{error::Error as StdError, fmt, io::Cursor, slice, str::FromStr};

use byteorder::ReadBytesExt;
use failure::Error;
use ffi_helpers::*;
use libc::size_t;
use num::bigint::{BigUint, ParseBigIntError};
use rand::thread_rng;

/// Concrete attribute kinds
#[derive(Copy, Clone, PartialEq, Eq)]
// represented as big-endian bytes.
pub struct AttributeKind([u8; 31]);

#[derive(Debug)]
pub enum ParseAttributeError {
    IntDecodingFailed(ParseBigIntError),
    ValueTooLarge,
}

impl From<ParseBigIntError> for ParseAttributeError {
    fn from(err: ParseBigIntError) -> Self { ParseAttributeError::IntDecodingFailed(err) }
}

impl StdError for ParseAttributeError {
    fn description(&self) -> &str {
        match self {
            ParseAttributeError::IntDecodingFailed(ref x) => x.description(),
            ParseAttributeError::ValueTooLarge => "Value out of range.",
        }
    }
}

impl fmt::Display for ParseAttributeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParseAttributeError::IntDecodingFailed(ref e) => e.fmt(f),
            ParseAttributeError::ValueTooLarge => "Value out of range.".fmt(f),
        }
    }
}

impl FromStr for AttributeKind {
    type Err = ParseAttributeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let buint = BigUint::from_str(s)?;
        if buint.bits() <= 31 * 8 {
            let bytes = buint.to_bytes_be();
            let mut buf = [0; 31];
            buf[31 - bytes.len()..].copy_from_slice(&bytes);
            Ok(AttributeKind(buf))
        } else {
            Err(ParseAttributeError::ValueTooLarge)
        }
    }
}

impl fmt::Display for AttributeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttributeKind(x) => write!(f, "{}", BigUint::from_bytes_be(x)),
        }
    }
}

impl From<u64> for AttributeKind {
    fn from(x: u64) -> Self {
        let mut buf = [0u8; 31];
        buf[23..].copy_from_slice(&x.to_be_bytes());
        AttributeKind(buf)
    }
}

impl Attribute<<G1 as Curve>::Scalar> for AttributeKind {
    fn to_field_element(&self) -> <G1 as Curve>::Scalar {
        let AttributeKind(x) = self;
        let mut buf = [0u8; 32];
        buf[1..].copy_from_slice(x);
        <G1 as Curve>::bytes_to_scalar(&mut Cursor::new(&buf)).unwrap()
    }

    fn to_bytes(&self) -> Box<[u8]> {
        let AttributeKind(x) = self;
        let bytes = (BigUint::from_bytes_be(x)).to_bytes_be();
        let l = bytes.len();
        let mut buf = vec![l as u8; l + 1];
        buf[1..].copy_from_slice(&bytes);
        buf.into_boxed_slice()
    }

    fn from_bytes(cur: &mut Cursor<&[u8]>) -> Option<Self> {
        let l = cur.read_u8().ok()?;
        if l <= 31 {
            let mut r = [0u8; 31];
            for i in (31 - l)..31 {
                r[i as usize] = cur.read_u8().ok()?;
            }
            Some(AttributeKind(r))
        } else {
            None
        }
    }
}

// #[no_mangle]
// #[allow(clippy::not_unsafe_ptr_arg_deref)]
// pub extern "C" fn verify_cdi_ffi(
// gc_dlogbase_ptr: *const G1,
// gc_cmm_key_ptr: *const PedersenKey<G1>,
// ip_verify_key_ptr: *const ps_sig::PublicKey<Bls12>,
// cdi_ptr: *const u8,
// cdi_len: size_t,
// ) -> i32 {
// if gc_dlogbase_ptr.is_null() {
// return -6;
// }
// if gc_cmm_key_ptr.is_null() {
// return -7;
// }
// if ip_verify_key_ptr.is_null() {
// return -8;
// }
// if ip_ar_pub_key_ptr.is_null() {
// return -9;
// }
// if ip_ar_elgamal_generator_ptr.is_null() {
// return -10;
// }
//
// let cdi_bytes = slice_from_c_bytes!(cdi_ptr, cdi_len as usize);
//
// match CredDeploymentInfo::<Bls12, G1, AttributeKind>::from_bytes(&mut
// Cursor::new(&cdi_bytes)) { None => -11,
// Some(cdi) => {
// match chain::verify_cdi_worker::<Bls12, G1, AttributeKind>(
// from_ptr!(gc_cmm_key_ptr),
// from_ptr!(ip_ar_pub_key_ptr),
// from_ptr!(ip_verify_key_ptr),
// &cdi,
// ) {
// Ok(()) => 1, // verification succeeded
// Err(CDIVerificationError::RegId) => -1,
// Err(CDIVerificationError::IdCredPub) => -2,
// Err(CDIVerificationError::Signature) => -3,
// Err(CDIVerificationError::Dlog) => -4,
// Err(CDIVerificationError::Policy) => -5,
// }
// }
// }
// }
//
// macro_derive_from_bytes!(
// pedersen_key_from_bytes,
// PedersenKey<G1>,
// PedersenKey::from_bytes
// );
// macro_derive_to_bytes!(pedersen_key_to_bytes, PedersenKey<G1>);
// macro_free_ffi!(pedersen_key_free, PedersenKey<G1>);
// macro_generate_commitment_key!(pedersen_key_gen, PedersenKey<G1>,
// PedersenKey::generate);
//
// macro_derive_from_bytes!(
// ps_sig_key_from_bytes,
// ps_sig::PublicKey<Bls12>,
// ps_sig::PublicKey::from_bytes
// );
// macro_derive_to_bytes!(ps_sig_key_to_bytes, ps_sig::PublicKey<Bls12>);
// macro_free_ffi!(ps_sig_key_free, ps_sig::PublicKey<Bls12>);
// macro_generate_commitment_key!(
// ps_sig_key_gen,
// ps_sig::PublicKey<Bls12>,
// ps_sig::PublicKey::arbitrary
// );
//
// pub struct ElgamalGenerator(G1);
//
// impl ElgamalGenerator {
// pub fn to_bytes(&self) -> Box<[u8]> { self.0.curve_to_bytes() }
//
// pub fn from_bytes(cur: &mut Cursor<&[u8]>) -> Result<Self, Error> {
// let r = G1::bytes_to_curve(cur)?;
// Ok(ElgamalGenerator(r))
// }
//
// pub fn generate() -> Self { ElgamalGenerator(G1::generate(&mut thread_rng()))
// } }
//
// macro_derive_from_bytes!(
// elgamal_gen_from_bytes,
// ElgamalGenerator,
// ElgamalGenerator::from_bytes
// );
// macro_derive_to_bytes!(elgamal_gen_to_bytes, ElgamalGenerator);
// macro_free_ffi!(elgamal_gen_free, ElgamalGenerator);
// #[no_mangle]
// #[allow(clippy::not_unsafe_ptr_arg_deref)]
// pub extern "C" fn elgamal_gen_gen() -> *const ElgamalGenerator {
// Box::into_raw(Box::new(ElgamalGenerator::generate()))
// }
//
// macro_derive_from_bytes!(
// elgamal_pub_key_from_bytes,
// elgamal::PublicKey<G1>,
// elgamal::PublicKey::from_bytes
// );
// macro_derive_to_bytes!(elgamal_pub_key_to_bytes, elgamal::PublicKey<G1>);
// macro_free_ffi!(elgamal_pub_key_free, elgamal::PublicKey<G1>);
// #[no_mangle]
// pub extern "C" fn elgamal_pub_key_gen() -> *const elgamal::PublicKey<G1> {
// let sk = elgamal::secret::SecretKey::generate(&mut thread_rng());
// Box::into_raw(Box::new(elgamal::PublicKey::from(&sk)))
// }
//
// macro_derive_from_bytes!(
// elgamal_cipher_from_bytes,
// elgamal::cipher::Cipher<G1>,
// elgamal::cipher::Cipher::from_bytes
// );
// macro_derive_to_bytes!(elgamal_cipher_to_bytes, elgamal::cipher::Cipher<G1>);
// macro_free_ffi!(elgamal_cipher_free, elgamal::cipher::Cipher<G1>);
// #[no_mangle]
// pub extern "C" fn elgamal_cipher_gen() -> *const elgamal::cipher::Cipher<G1>
// { let mut csprng = thread_rng();
// Box::into_raw(Box::new(elgamal::cipher::Cipher::generate(&mut csprng)))
// }
//
// #[cfg(test)]
// mod test {
// use super::*;
// use crate::{account_holder::*, identity_provider::*};
// use dodis_yampolskiy_prf::secret as prf;
// use eddsa_ed25519 as ed25519;
// use elgamal::{public::PublicKey, secret::SecretKey};
// use pairing::bls12_381::Bls12;
// use pedersen_scheme::key as pedersen_key;
// use ps_sig;
//
// type ExampleAttributeList = AttributeList<<Bls12 as Pairing>::ScalarField,
// AttributeKind>; type ExampleCurve = G1;
// #[test]
// fn test_pipeline() {
// let mut csprng = thread_rng();
//
// let secret = G1::generate_scalar(&mut csprng);
// let public = G1::one_point().mul_by_scalar(&secret);
// let ah_info = CredentialHolderInfo::<<Bls12 as Pairing>::G_1, ExampleCurve> {
// id_ah:   "ACCOUNT_HOLDER".to_owned(),
// id_cred: IdCredentials {
// id_cred_sec:    secret,
// id_cred_pub:    public,
// id_cred_pub_ip: public,
// },
// };
//
// let id_secret_key = ps_sig::secret::SecretKey::<Bls12>::generate(10, &mut
// csprng); let id_public_key = ps_sig::public::PublicKey::from(&id_secret_key);
//
// let ar_secret_key = SecretKey::generate(&mut csprng);
// let ar_public_key = PublicKey::from(&ar_secret_key);
// let ar_info = ArInfo {
// ar_identity: 0,
// ar_description: "AR".to_owned(),
// ar_public_key,
// ar_elgamal_generator: PublicKey::generator(),
// };
//
// let ip_info = IpInfo {
// ip_identity: 3,
// ip_description: "ID".to_owned(),
// ip_verify_key: id_public_key,
// ar_info,
// };
//
// let prf_key = prf::SecretKey::generate(&mut csprng);
//
// let variant = 0;
// let expiry_date = 123123123;
// let alist = vec![AttributeKind::from(55), AttributeKind::from(313123333)];
//
// let aci = AccCredentialInfo::<Bls12, ExampleCurve, AttributeKind> {
// acc_holder_info: ah_info,
// prf_key,
// attributes: ExampleAttributeList {
// variant,
// expiry: expiry_date,
// alist,
// _phantom: Default::default(),
// },
// };
//
// let context = make_context_from_ip_info(ip_info.clone());
// let (pio, randomness) = generate_pio(&context, &aci);
//
// let sig_ok = verify_credentials(&pio, context, &id_secret_key);
//
// First test, check that we have a valid signature.
// assert!(sig_ok.is_ok());
//
// let ip_sig = sig_ok.unwrap();
// let global_ctx = GlobalContext::<G1> {
// dlog_base_chain:         ExampleCurve::one_point(),
// on_chain_commitment_key: pedersen_key::CommitmentKey::generate(1, &mut
// csprng), };
//
// let policy = Policy {
// variant,
// expiry: expiry_date,
// policy_vec: vec![(0, AttributeKind::from(55))],
// _phantom: Default::default(),
// };
//
// let wrong_policy = Policy {
// variant,
// expiry: expiry_date,
// policy_vec: vec![(0, AttributeKind::from(5))],
// _phantom: Default::default(),
// };
//
// let kp = ed25519::generate_keypair();
// let acc_data = AccountData {
// sign_key:   kp.secret,
// verify_key: kp.public,
// };
//
// let cdi = generate_cdi(
// &ip_info,
// &global_ctx,
// &aci,
// &pio,
// 0,
// &ip_sig,
// &policy,
// &acc_data,
// &randomness,
// );
//
// let wrong_cdi = generate_cdi(
// &ip_info,
// &global_ctx,
// &aci,
// &pio,
// 0,
// &ip_sig,
// &wrong_policy,
// &acc_data,
// &randomness,
// );
//
// let cdi_bytes = cdi.to_bytes();
// let cdi_bytes_len = cdi_bytes.len() as size_t;
//
// let dlog_base_ptr = Box::into_raw(Box::new(global_ctx.dlog_base_chain));
// let cmm_key_ptr =
// Box::into_raw(Box::new(global_ctx.on_chain_commitment_key));
// let ip_verify_key_ptr = Box::into_raw(Box::new(ip_info.ip_verify_key));
// let elgamal_generator_ptr =
// Box::into_raw(Box::new(ip_info.ar_info.ar_elgamal_generator));
// let ar_public_key_ptr =
// Box::into_raw(Box::new(ip_info.ar_info.ar_public_key));
//
// let cdi_check = verify_cdi_ffi(
// dlog_base_ptr,
// cmm_key_ptr,
// ip_verify_key_ptr,
// elgamal_generator_ptr,
// ar_public_key_ptr,
// cdi_bytes.as_ptr(),
// cdi_bytes_len,
// );
// println!("cdi_check={}", cdi_check);
// assert_eq!(cdi_check, 1);
// let wrong_cdi_bytes = &*wrong_cdi.to_bytes();
// let wrong_cdi_bytes_len = wrong_cdi_bytes.len() as size_t;
// let wrong_cdi_check = verify_cdi_ffi(
// dlog_base_ptr,
// cmm_key_ptr,
// ip_verify_key_ptr,
// elgamal_generator_ptr,
// ar_public_key_ptr,
// wrong_cdi_bytes.as_ptr(),
// wrong_cdi_bytes_len,
// );
// assert_ne!(wrong_cdi_check, 1);
// }
// }
