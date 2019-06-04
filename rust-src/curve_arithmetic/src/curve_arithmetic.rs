// Authors:
// - bm@concordium.com
//

use pairing::Field;
use std::fmt::{Debug, Display};
use rand::*;

pub enum FieldDecodingError {
    NotFieldElement,
}
pub enum CurveDecodingError {
    NotOnCurve,
}


pub trait Curve:
    Copy + Clone + Sized + Send + Sync + Debug + Display + PartialEq + Eq + 'static {
    type Scalar: Field;
    type Base: Field;
    type Compressed;
    const SCALAR_LENGTH : usize;
    const GROUP_ELEMENT_LENGTH : usize;
    fn zero_point() -> Self;
    fn one_point() -> Self; // generator
    fn is_zero_point(&self) -> bool;
    fn inverse_point(&self) -> Self;
    fn double_point(&self) -> Self;
    fn plus_point(&self, other: &Self) -> Self;
    fn minus_point(&self, other: &Self) -> Self;
    fn mul_by_scalar(&self, scalar: &Self::Scalar) -> Self;
    fn compress(&self) -> Self::Compressed;
    fn decompress(c: &Self::Compressed) -> Result<Self, CurveDecodingError>;
    fn decompress_unchecked(c: &Self::Compressed) -> Result<Self, CurveDecodingError>;
    fn scalar_to_bytes(s: &Self::Scalar)-> Box<[u8]>;
    fn bytes_to_scalar(b: &[u8]) -> Result<Self::Scalar, FieldDecodingError>;
    fn curve_to_bytes(&self)-> Box<[u8]>;
    fn bytes_to_curve(b: &[u8]) -> Result<Self, CurveDecodingError>;
    fn generate<R: Rng> (rng: &mut R) -> Self;
    fn generate_scalar<R: Rng>(rng:&mut R)-> Self::Scalar;
}
