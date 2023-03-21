#![doc = include_str!("../README.md")]
pub mod base;
pub mod cis2_types;
pub mod constants;
pub mod hashes;
mod internal;
pub mod smart_contracts;
pub mod transactions;
pub mod updates;

// Since types from these crates are exposed in the public API of this crate
// we re-export them so that they don't have to be added as separate
// dependencies by users.
pub mod aggregate_sig;
pub mod bulletproofs;
pub use concordium_contracts_common as contracts_common;
pub mod common;
pub mod curve_arithmetic;
pub mod ecvrf;
pub mod eddsa_ed25519;
pub mod elgamal;
pub mod encrypted_transfers;
pub mod id;
pub mod random_oracle;

pub mod pedersen_commitment;
pub mod ps_sig;

pub mod dodis_yampolskiy_prf;

#[cfg(feature = "ffi")]
mod ffi_helpers;

// This is here so that we can use the _derive crate inside this crate as well.
// It allows the generated code to refer to concordium_base::
#[doc(hidden)]
extern crate self as concordium_base;
