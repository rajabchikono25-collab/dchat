use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use dchat_crypto::post_quantum::{
    falcon, kyber, HybridKem, HybridSigner, verify_hybrid_signature,
};
use std::hint::black_box;

fn bench_falcon_keygen(c: &mut Criterion) {
    c.bench_function("falcon512_keygen", |b| {
        b.iter(|| black_box(falcon::keypair()))
    });
}

fn bench_falcon_signing(c: &mut Criterion) {
    let (_, secret_key) = falcon::keypair();
    let mut group = c.benchmark_group("falcon512_sign");

    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];

        group.bench_with_input(BenchmarkId::new("sign", size), &data, |b, data| {
            b.iter(|| black_box(falcon::detached_sign(data, &secret_key)))
        });
    }

    group.finish();
}

fn bench_falcon_verification(c: &mut Criterion) {
    let (public_key, secret_key) = falcon::keypair();
    let mut group = c.benchmark_group("falcon512_verify");

    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        let signature = falcon::detached_sign(&data, &secret_key);

        group.bench_with_input(BenchmarkId::new("verify", size), &data, |b, data| {
            b.iter(|| {
                black_box(falcon::verify_detached_signature(&signature, data, &public_key))
            })
        });
    }

    group.finish();
}

fn bench_kyber768_keygen(c: &mut Criterion) {
    c.bench_function("kyber768_keygen", |b| {
        b.iter(|| black_box(kyber::keypair()))
    });
}

fn bench_kyber768_encapsulation(c: &mut Criterion) {
    let (public_key, _) = kyber::keypair();

    c.bench_function("kyber768_encapsulate", |b| {
        b.iter(|| black_box(kyber::encapsulate(&public_key)))
    });
}

fn bench_kyber768_decapsulation(c: &mut Criterion) {
    let (public_key, secret_key) = kyber::keypair();
    let (_, ciphertext) = kyber::encapsulate(&public_key);

    c.bench_function("kyber768_decapsulate", |b| {
        b.iter(|| black_box(kyber::decapsulate(&ciphertext, &secret_key)))
    });
}

fn bench_hybrid_kem_keygen(c: &mut Criterion) {
    c.bench_function("hybrid_kem_keygen", |b| {
        b.iter(|| black_box(HybridKem::keypair()))
    });
}

fn bench_hybrid_kem_encapsulation(c: &mut Criterion) {
    let (public_key, _) = HybridKem::keypair().unwrap();

    c.bench_function("hybrid_kem_encapsulate", |b| {
        b.iter(|| black_box(HybridKem::encapsulate(&public_key)))
    });
}

fn bench_hybrid_kem_decapsulation(c: &mut Criterion) {
    let (public_key, secret_key) = HybridKem::keypair().unwrap();
    let (_, ciphertext) = HybridKem::encapsulate(&public_key).unwrap();

    c.bench_function("hybrid_kem_decapsulate", |b| {
        b.iter(|| black_box(HybridKem::decapsulate(&ciphertext, &secret_key)))
    });
}

fn bench_hybrid_signer_keygen(c: &mut Criterion) {
    c.bench_function("hybrid_signer_keygen", |b| {
        b.iter(|| black_box(HybridSigner::new()))
    });
}

fn bench_hybrid_signer_sign(c: &mut Criterion) {
    let signer = HybridSigner::new();
    let mut group = c.benchmark_group("hybrid_sign");

    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];

        group.bench_with_input(BenchmarkId::new("sign", size), &data, |b, data| {
            b.iter(|| black_box(signer.sign(data)))
        });
    }

    group.finish();
}

fn bench_hybrid_signer_verify(c: &mut Criterion) {
    let signer = HybridSigner::new();
    let (classical_public, pq_public) = signer.public_keys();
    let mut group = c.benchmark_group("hybrid_verify");

    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        let signature = signer.sign(&data);

        group.bench_with_input(BenchmarkId::new("verify", size), &data, |b, data| {
            b.iter(|| {
                black_box(verify_hybrid_signature(
                    &signature,
                    data,
                    &classical_public,
                    &pq_public,
                ))
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_falcon_keygen,
    bench_falcon_signing,
    bench_falcon_verification,
    bench_kyber768_keygen,
    bench_kyber768_encapsulation,
    bench_kyber768_decapsulation,
    bench_hybrid_kem_keygen,
    bench_hybrid_kem_encapsulation,
    bench_hybrid_kem_decapsulation,
    bench_hybrid_signer_keygen,
    bench_hybrid_signer_sign,
    bench_hybrid_signer_verify
);
criterion_main!(benches);
