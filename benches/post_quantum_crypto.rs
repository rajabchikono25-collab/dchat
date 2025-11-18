use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use dchat_crypto::post_quantum::dilithium::Dilithium3Signer;
use dchat_crypto::post_quantum::kyber::Kyber768;
use dchat_crypto::post_quantum::falcon::FalconSigner;
use dchat_crypto::post_quantum::hybrid::HybridScheme;
use std::hint::black_box;

fn bench_dilithium3_keygen(c: &mut Criterion) {
    c.bench_function("dilithium3_keygen", |b| {
        b.iter(|| black_box(Dilithium3Signer::generate()))
    });
}

fn bench_dilithium3_signing(c: &mut Criterion) {
    let signer = Dilithium3Signer::generate();
    let mut group = c.benchmark_group("dilithium3_sign");
    
    for size in [100, 1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        
        group.bench_with_input(BenchmarkId::new("sign", size), &data, |b, data| {
            b.iter(|| black_box(signer.sign(data)))
        });
    }
    
    group.finish();
}

fn bench_dilithium3_verification(c: &mut Criterion) {
    let signer = Dilithium3Signer::generate();
    let mut group = c.benchmark_group("dilithium3_verify");
    
    for size in [100, 1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        let signature = signer.sign(&data).unwrap();
        
        group.bench_with_input(BenchmarkId::new("verify", size), &data, |b, data| {
            b.iter(|| black_box(signer.verify(data, &signature)))
        });
    }
    
    group.finish();
}

fn bench_kyber768_keygen(c: &mut Criterion) {
    c.bench_function("kyber768_keygen", |b| {
        b.iter(|| black_box(Kyber768::generate_keypair()))
    });
}

fn bench_kyber768_encapsulation(c: &mut Criterion) {
    let (public_key, _) = Kyber768::generate_keypair();
    
    c.bench_function("kyber768_encapsulate", |b| {
        b.iter(|| black_box(Kyber768::encapsulate(&public_key)))
    });
}

fn bench_kyber768_decapsulation(c: &mut Criterion) {
    let (public_key, secret_key) = Kyber768::generate_keypair();
    let (ciphertext, _) = Kyber768::encapsulate(&public_key).unwrap();
    
    c.bench_function("kyber768_decapsulate", |b| {
        b.iter(|| black_box(Kyber768::decapsulate(&ciphertext, &secret_key)))
    });
}

fn bench_falcon_keygen(c: &mut Criterion) {
    c.bench_function("falcon_keygen", |b| {
        b.iter(|| black_box(FalconSigner::generate()))
    });
}

fn bench_falcon_signing(c: &mut Criterion) {
    let signer = FalconSigner::generate();
    let mut group = c.benchmark_group("falcon_sign");
    
    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        
        group.bench_with_input(BenchmarkId::new("sign", size), &data, |b, data| {
            b.iter(|| black_box(signer.sign(data)))
        });
    }
    
    group.finish();
}

fn bench_hybrid_scheme_sign(c: &mut Criterion) {
    let hybrid = HybridScheme::new();
    let mut group = c.benchmark_group("hybrid_sign");
    
    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        
        group.bench_with_input(BenchmarkId::new("sign", size), &data, |b, data| {
            b.iter(|| black_box(hybrid.sign(data)))
        });
    }
    
    group.finish();
}

fn bench_hybrid_scheme_verify(c: &mut Criterion) {
    let hybrid = HybridScheme::new();
    let mut group = c.benchmark_group("hybrid_verify");
    
    for size in [100, 1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        let signature = hybrid.sign(&data).unwrap();
        
        group.bench_with_input(BenchmarkId::new("verify", size), &data, |b, data| {
            b.iter(|| black_box(hybrid.verify(data, &signature)))
        });
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_dilithium3_keygen,
    bench_dilithium3_signing,
    bench_dilithium3_verification,
    bench_kyber768_keygen,
    bench_kyber768_encapsulation,
    bench_kyber768_decapsulation,
    bench_falcon_keygen,
    bench_falcon_signing,
    bench_hybrid_scheme_sign,
    bench_hybrid_scheme_verify
);
criterion_main!(benches);
