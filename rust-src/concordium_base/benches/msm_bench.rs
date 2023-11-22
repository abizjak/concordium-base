#![allow(non_snake_case)]

#[macro_use]
extern crate criterion;

use concordium_base::curve_arithmetic::*;
use criterion::Criterion;
use curve25519_dalek_ng::{ristretto::RistrettoPoint, traits::VartimePrecomputedMultiscalarMul};
use pairing::bls12_381::G1;
use rand::*;
use std::time::Duration;

const N: usize = 512;

pub fn ccd_msm_benchmarks<SomeCurve: Curve> (c: &mut Criterion) {
    let mut group = c.benchmark_group("Multi-Scalar Multiplication");
    let rng = &mut thread_rng();
    
    
    let mut G = Vec::with_capacity(N);
    let mut V: Vec<<SomeCurve as Curve>::Scalar> = Vec::with_capacity(N);

    for _ in 0..N {
        let g = SomeCurve::generate(rng);
        let v: <SomeCurve as Curve>::Scalar = SomeCurve::generate_scalar(rng);
        G.push(g);
        V.push(v);
    }
    group.bench_function("MSM in Concordium over BLS/Ristretto curve", move |b| {
        b.iter(|| {
            // Create msm algoritm instane with a precomputed point table.
            // For the ristretto curve it will use the VartimeRistrettoPrecomputation and our generic implementation for the BLS curve
            let msm = SomeCurve::new_multiexp(&G);
            msm.multiexp(&V);
        })
    });
}

pub fn dalek_msm_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("Multi-Scalar Multiplication");
    let mut rng = &mut thread_rng();
    
    use curve25519_dalek_ng::scalar::Scalar;
    use curve25519_dalek_ng::traits::VartimeMultiscalarMul;
    let G: Vec<RistrettoPoint> = (0..N).map(|_| RistrettoPoint::random(&mut rng)).collect();
    let V: Vec<_> = (0..N).map(|_| Scalar::random(&mut rng)).collect();

    group.bench_function("MSM in Dalek over Ristretto curve", move |b| {
        b.iter(|| {
            RistrettoPoint::vartime_multiscalar_mul(&V, &G);
        })
    });
}

pub fn dalek_msm_benchmarks_precompute(c: &mut Criterion) {
    let mut group = c.benchmark_group("Multi-Scalar Multiplication");
    let mut rng = &mut thread_rng();
    
    use curve25519_dalek_ng::scalar::Scalar;
    use curve25519_dalek_ng::ristretto::VartimeRistrettoPrecomputation;
    let G: Vec<RistrettoPoint> = (0..N).map(|_| RistrettoPoint::random(&mut rng)).collect();
    let V: Vec<_> = (0..N).map(|_| Scalar::random(&mut rng)).collect();

    group.bench_function("MSM in Dalek over Ristretto curve", move |b| {
        b.iter(|| {
            let msm = <VartimeRistrettoPrecomputation as VartimePrecomputedMultiscalarMul>::new(&G);
            msm.vartime_multiscalar_mul(&V);
        })
    });
}

criterion_group!(
    name = benchmarks;
    config = Criterion::default().measurement_time(Duration::from_millis(10000)).sample_size(100);
    targets = ccd_msm_benchmarks::<G1>, ccd_msm_benchmarks::<RistrettoPoint>, dalek_msm_benchmarks, dalek_msm_benchmarks_precompute);
criterion_main!(benchmarks);

