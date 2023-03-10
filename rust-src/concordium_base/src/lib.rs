//! A library that defines common types and functionality that is
//! needed by various Rust projects. The scope of this library is
//! meant to be limited to core chain definitions. At present this is an
//! internal library with an unstable API. Where necessary the functionality
//! should be re-exported, for example like it is in the Rust-SDK.
//!
//! This library should always be possible to compile for android, iOS, Wasm,
//! and x86 code. Some parts may be feature gated to work around platform
//! specific limitations though.
//!
//! This library also exports other core crypto dependencies so that consumers
//! may simplify their dependencies. Users are intended to get the re-exported
//! dependencies through the library, instead of separately.
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
pub mod ecvrf;
pub mod eddsa_ed25519;
pub mod encrypted_transfers;
pub mod id;
pub mod random_oracle;
pub mod curve_arithmetic;
pub mod elgamal;

pub mod pedersen_commitment;
pub mod ps_sig;

pub mod dodis_yampolskiy_prf;

#[cfg(feature = "ffi")]
mod ffi_helpers;
