//! Common types and operations used throughout the Concordium chain
//! development.
mod helpers;
mod impls;
mod serde_impls;
mod serialize;
pub mod types;
mod version;

pub use self::{helpers::*, impls::*, serialize::*, version::*};

// Reexport for ease of use.
pub use byteorder::{ReadBytesExt, WriteBytesExt};

/// Derive macro to derive [serde::Deserialize] instances.
pub use serde::Deserialize as SerdeDeserialize;
/// Derive macro to derive [serde::Serialize] instances.
pub use serde::Serialize as SerdeSerialize;

/// These are re-exported to help the derive crate.
#[doc(hidden)]
pub use serde::Deserializer as SerdeDeserializer;
#[doc(hidden)]
pub use serde::Serializer as SerdeSerializer;

pub use concordium_base_derive::*;

#[doc(hidden)]
/// This is provided as a workaround so that we can build these libraries in
/// Wasm. FIXME: At some point we should handle this better. The FFI exports are
/// mostly not needed in Wasm, and they are the only ones that need this.
#[cfg(not(target_arch = "wasm32"))]
pub use libc::size_t;
#[cfg(target_arch = "wasm32")]
#[allow(non_camel_case_types)]
pub type size_t = usize;

#[cfg(feature = "encryption")]
/// Module that provides a simple API for symmetric encryption in the output
/// formats used by Concordium.
pub mod encryption;
