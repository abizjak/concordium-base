use curve_arithmetic::{curve_arithmetic::Curve, serialization::*};
use failure::Error;
use pairing::Field;
use rand::*;
use sha2::{Digest, Sha256};
use std::io::Cursor;

#[derive(Clone, Debug, Eq, PartialEq, Copy)]
pub struct ComEqDiffGrpsProof<C1: Curve, C2: Curve<Scalar = C1::Scalar>> {
    challenge:        C1::Scalar,
    randomised_point: (C1, C2),
    witness:          (C1::Scalar, C1::Scalar, C1::Scalar),
}

impl<C1, C2> ComEqDiffGrpsProof<C1, C2>
where
    C1: Curve,
    C2: Curve<Scalar = C1::Scalar>,
{
    pub fn to_bytes(&self) -> Vec<u8> {
        let bytes_len = C1::SCALAR_LENGTH
            + C1::GROUP_ELEMENT_LENGTH
            + C2::GROUP_ELEMENT_LENGTH
            + 3 * C1::SCALAR_LENGTH;
        let mut bytes = Vec::with_capacity(bytes_len);
        write_curve_scalar::<C1>(&self.challenge, &mut bytes);
        write_curve_element::<C1>(&self.randomised_point.0, &mut bytes);
        write_curve_element::<C2>(&self.randomised_point.1, &mut bytes);
        write_curve_scalar::<C1>(&self.witness.0, &mut bytes);
        write_curve_scalar::<C1>(&self.witness.1, &mut bytes);
        write_curve_scalar::<C1>(&self.witness.2, &mut bytes);
        bytes
    }

    pub fn from_bytes(bytes: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        let mut scalar_buffer = vec![0; C1::SCALAR_LENGTH];
        let mut group_buffer_c1 = vec![0; C1::GROUP_ELEMENT_LENGTH];
        let mut group_buffer_c2 = vec![0; C2::GROUP_ELEMENT_LENGTH];
        let challenge = read_curve_scalar::<C1>(bytes, &mut scalar_buffer)?;
        let r1 = read_curve::<C1>(bytes, &mut group_buffer_c1)?;
        let r2 = read_curve::<C2>(bytes, &mut group_buffer_c2)?;
        let w1 = read_curve_scalar::<C1>(bytes, &mut scalar_buffer)?;
        let w2 = read_curve_scalar::<C1>(bytes, &mut scalar_buffer)?;
        let w3 = read_curve_scalar::<C1>(bytes, &mut scalar_buffer)?;
        let randomised_point = (r1, r2);
        let witness = (w1, w2, w3);
        Ok(ComEqDiffGrpsProof {
            challenge,
            randomised_point,
            witness,
        })
    }
}

pub fn prove_com_eq_diff_grps<C1: Curve, C2: Curve<Scalar = C1::Scalar>, R: Rng>(
    csprng: &mut R,
    public: &(C1, C2),
    secret: &(C1::Scalar, C1::Scalar, C1::Scalar),
    coeff: &((C1, C1), (C2, C2)),
) -> ComEqDiffGrpsProof<C1, C2> {
    let (public_1, public_2) = public;

    let ((g_1, h_1), (g_2, h_2)) = coeff;
    let mut hasher = Sha256::new();
    hasher.input(&*public_1.curve_to_bytes());
    hasher.input(&*public_2.curve_to_bytes());
    let mut hash = [0u8; 32];
    let mut suc = false;
    let mut w_1 = secret.0.clone();
    let mut w_2 = secret.1.clone();
    let mut w_3 = secret.2.clone();
    let mut challenge = C1::Scalar::zero();
    let mut randomised_point = (C1::zero_point(), C2::zero_point());
    while !suc {
        let mut hasher2 = hasher.clone();
        let (r_1, r_2, r_3) = (
            C1::generate_scalar(csprng),
            C1::generate_scalar(csprng),
            C1::generate_scalar(csprng),
        );
        let rp_1 = g_1.mul_by_scalar(&r_1).plus_point(&h_1.mul_by_scalar(&r_2));
        let rp_2 = g_2.mul_by_scalar(&r_1).plus_point(&h_2.mul_by_scalar(&r_3));
        hasher2.input(&*rp_1.curve_to_bytes());
        hasher2.input(&*rp_2.curve_to_bytes());
        hash.copy_from_slice(hasher2.result().as_slice());
        match C1::bytes_to_scalar(&hash) {
            Err(_) => {}
            Ok(x) => {
                if x == C1::Scalar::zero() {
                    println!("x = 0");
                } else {
                    challenge = x;
                    randomised_point = (rp_1, rp_2);
                    w_1.mul_assign(&challenge);
                    w_1.negate();
                    w_1.add_assign(&r_1);
                    w_2.mul_assign(&challenge);
                    w_2.negate();
                    w_2.add_assign(&r_2);
                    w_3.mul_assign(&challenge);
                    w_3.negate();
                    w_3.add_assign(&r_3);
                    suc = true;
                }
            }
        }
    }

    ComEqDiffGrpsProof {
        challenge,
        randomised_point,
        witness: (w_1, w_2, w_3),
    }
}

pub fn verify_com_eq_diff_grps<C1: Curve, C2: Curve<Scalar = C1::Scalar>>(
    coeff: &((C1, C1), (C2, C2)),
    public: &(C1, C2),
    proof: &ComEqDiffGrpsProof<C1, C2>,
) -> bool {
    let mut hasher = Sha256::new();
    let (public_1, public_2) = public;
    let ((g_1, h_1), (g_2, h_2)) = coeff;
    let (w_1, w_2, w_3) = proof.witness;
    hasher.input(&*public_1.curve_to_bytes());
    hasher.input(&*public_2.curve_to_bytes());
    let (rp_1, rp_2) = proof.randomised_point;
    hasher.input(&*rp_1.curve_to_bytes());
    hasher.input(&*rp_2.curve_to_bytes());
    let mut hash = [0u8; 32];
    hash.copy_from_slice(hasher.result().as_slice());
    match C1::bytes_to_scalar(&hash) {
        Err(_) => false,
        Ok(c) => {
            if c != proof.challenge {
                false
            } else {
                rp_1 == public_1
                    .mul_by_scalar(&c)
                    .plus_point(&g_1.mul_by_scalar(&w_1))
                    .plus_point(&h_1.mul_by_scalar(&w_2))
                    && rp_2
                        == public_2
                            .mul_by_scalar(&c)
                            .plus_point(&g_2.mul_by_scalar(&w_1))
                            .plus_point(&h_2.mul_by_scalar(&w_3))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pairing::bls12_381::{G1Affine, G2Affine};

    #[test]
    pub fn test_com_eq_diff_grps() {
        let mut csprng = thread_rng();
        for i in 0..100 {
            let (s_1, s_2, s_3) = (
                G1Affine::generate_scalar(&mut csprng),
                G1Affine::generate_scalar(&mut csprng),
                G1Affine::generate_scalar(&mut csprng),
            );
            let ((g_1, h_1), (g_2, h_2)) = (
                (
                    G1Affine::generate(&mut csprng),
                    G1Affine::generate(&mut csprng),
                ),
                (
                    G2Affine::generate(&mut csprng),
                    G2Affine::generate(&mut csprng),
                ),
            );
            let public = (
                g_1.mul_by_scalar(&s_1).plus_point(&h_1.mul_by_scalar(&s_2)),
                g_2.mul_by_scalar(&s_1).plus_point(&h_2.mul_by_scalar(&s_3)),
            );
            let secret = (s_1, s_2, s_3);
            let coeff = ((g_1, h_1), (g_2, h_2));
            let proof = prove_com_eq_diff_grps::<G1Affine, G2Affine, ThreadRng>(
                &mut csprng,
                &public,
                &secret,
                &coeff,
            );
            assert!(verify_com_eq_diff_grps(&coeff, &public, &proof));
        }
    }

    #[test]
    pub fn test_com_eq_diff_grps_proof_serialization() {
        let mut csprng = thread_rng();
        for i in 0..100 {
            let challenge = G1Affine::generate_scalar(&mut csprng);
            let randomised_point = (
                G1Affine::generate(&mut csprng),
                G2Affine::generate(&mut csprng),
            );
            let witness = (
                G1Affine::generate_scalar(&mut csprng),
                G1Affine::generate_scalar(&mut csprng),
                G1Affine::generate_scalar(&mut csprng),
            );
            let ap = ComEqDiffGrpsProof {
                challenge,
                randomised_point,
                witness,
            };
            let bytes = ap.to_bytes();
            let app = ComEqDiffGrpsProof::from_bytes(&mut Cursor::new(&bytes));
            assert!(app.is_ok());
            assert_eq!(ap, app.unwrap());
        }
    }

}
