use criterion::{criterion_group, criterion_main, Criterion};
use dchat_crypto::{KeyPair, SigningKey};
use std::hint::black_box;

fn bench_relay_simulation(c: &mut Criterion) {
    c.bench_function("relay_basic", |b| {
        b.iter(|| {
            let keypair = KeyPair::generate();
            black_box(keypair)
        })
    });
}

fn bench_proof_of_delivery_creation(c: &mut Criterion) {
    c.bench_function("proof_of_delivery", |b| {
        let keypair = KeyPair::generate();
        let signing_key = SigningKey::from_private_key(keypair.private_key());

        b.iter(|| {
            let message_id = "msg_12345";
            let timestamp = "2024-01-01T00:00:00Z";
            let proof_data = format!("{}:{}", message_id, timestamp);
            let signature = signing_key.sign(proof_data.as_bytes());
            black_box(signature)
        })
    });
}

criterion_group!(
    benches,
    bench_relay_simulation,
    bench_proof_of_delivery_creation
);
criterion_main!(benches);
