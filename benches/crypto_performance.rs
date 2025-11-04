use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use dchat_crypto::{KeyPair, SigningKey};
use std::hint::black_box;

fn bench_key_generation(c: &mut Criterion) {
    c.bench_function("keypair_generation", |b| {
        b.iter(|| black_box(KeyPair::generate()))
    });
}

fn bench_signing(c: &mut Criterion) {
    let keypair = KeyPair::generate();
    let signing_key = SigningKey::from_private_key(keypair.private_key());
    let mut group = c.benchmark_group("signing");

    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        let data = vec![0u8; *size];

        group.bench_with_input(BenchmarkId::new("sign", size), &data, |b, data| {
            b.iter(|| black_box(signing_key.sign(data)))
        });
    }

    group.finish();
}

fn bench_verification(c: &mut Criterion) {
    let keypair = KeyPair::generate();
    let signing_key = SigningKey::from_private_key(keypair.private_key());
    let verifying_key = signing_key.verifying_key();
    let mut group = c.benchmark_group("verification");

    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        let data = vec![0u8; *size];
        let signature = signing_key.sign(&data);

        group.bench_with_input(BenchmarkId::new("verify", size), &data, |b, data| {
            b.iter(|| black_box(verifying_key.verify(data, &signature).is_ok()))
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_key_generation,
    bench_signing,
    bench_verification
);
criterion_main!(benches);
