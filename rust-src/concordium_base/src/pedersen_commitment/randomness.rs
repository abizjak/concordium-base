// -*- mode: rust; -*-

//! Randomness
//! The randomness used in commitment

use crate::{
    common::*,
    curve_arithmetic::{curve_group::Group, *},
};

use ff::Field;

use rand::*;
use std::ops::Deref;

use std::rc::Rc;

/// Randomness used in the commitment.
/// Secret by default.
#[repr(transparent)]
#[derive(Debug, PartialEq, Eq, Serialize, SerdeBase16Serialize)]
pub struct Randomness<C: Group> {
    pub randomness: Rc<Secret<C::Scalar>>,
}

impl<C: Group> Clone for Randomness<C> {
    fn clone(&self) -> Self {
        Self {
            randomness: self.randomness.clone(),
        }
    }
}

/// This trait allows automatic conversion of `&Randomness<C>` to `&C::Scalar`.
impl<C: Group> Deref for Randomness<C> {
    type Target = C::Scalar;

    fn deref(&self) -> &C::Scalar { &self.randomness }
}

impl<C: Group> AsRef<C::Scalar> for Randomness<C> {
    fn as_ref(&self) -> &C::Scalar { &self.randomness }
}

impl<C: Group> Randomness<C> {
    pub fn new(x: C::Scalar) -> Self {
        Randomness {
            randomness: Rc::new(Secret::new(x)),
        }
    }

    pub fn as_value(&self) -> Value<C> {
        Value {
            value: self.randomness.clone(),
        }
    }

    pub fn from_value(x: &Value<C>) -> Self {
        Self {
            randomness: x.value.clone(),
        }
    }

    pub fn from_u64(x: u64) -> Self { Self::new(C::scalar_from_u64(x)) }

    /// Zero randomness.
    #[inline]
    pub fn zero() -> Self { Randomness::new(C::Scalar::zero()) }

    /// Generate a scalar as randomness.
    pub fn generate<T>(csprng: &mut T) -> Randomness<C>
    where
        T: Rng, {
        Randomness::new(C::generate_scalar(csprng))
    }

    /// Generate a non-zero scalar as randomness.
    pub fn generate_non_zero<T>(csprng: &mut T) -> Randomness<C>
    where
        T: Rng, {
        Randomness::new(C::generate_non_zero_scalar(csprng))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pairing::bls12_381::{G1Affine, G2Affine};
    macro_rules! macro_test_randomness_to_byte_conversion {
        ($function_name:ident, $curve_type:path) => {
            #[test]
            pub fn $function_name() {
                let mut csprng = thread_rng();
                for _i in 1..20 {
                    let val = Randomness::<$curve_type>::generate(&mut csprng);
                    let res_val2 = serialize_deserialize(&val);
                    assert!(res_val2.is_ok());
                    let val2 = res_val2.unwrap();
                    assert_eq!(val2, val);
                }
            }
        };
    }

    macro_test_randomness_to_byte_conversion!(
        randomness_to_byte_conversion_bls12_381_g1_affine,
        G1Affine
    );

    macro_test_randomness_to_byte_conversion!(
        randomness_to_byte_conversion_bls12_381_g2_affine,
        G2Affine
    );
}
