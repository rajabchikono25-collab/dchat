use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use dchat_network::network::onion::sphinx::{SphinxPacket, LayeredEncryption, CircuitManager};
use dchat_network::network::onion::path::PathSelector;
use dchat_crypto::KeyPair;
use std::hint::black_box;
use std::sync::Arc;

fn bench_sphinx_packet_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("sphinx_packet");
    
    for hop_count in [3, 5, 7].iter() {
        let mut relay_keys = Vec::new();
        for _ in 0..*hop_count {
            relay_keys.push(KeyPair::generate());
        }
        
        group.bench_with_input(
            BenchmarkId::new("create_packet", hop_count),
            hop_count,
            |b, _| {
                let message = vec![0u8; 1024];
                b.iter(|| {
                    black_box(SphinxPacket::create(
                        &message,
                        &relay_keys.iter().map(|k| k.public_key()).collect::<Vec<_>>(),
                    ))
                })
            },
        );
    }
    
    group.finish();
}

fn bench_onion_layer_encryption(c: &mut Criterion) {
    let mut group = c.benchmark_group("onion_encryption");
    
    for size in [512, 1024, 4096, 16384].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        
        let data = vec![0u8; *size];
        let key = KeyPair::generate();
        
        group.bench_with_input(
            BenchmarkId::new("encrypt_layer", size),
            size,
            |b, _| {
                b.iter(|| {
                    black_box(LayeredEncryption::encrypt_layer(&data, key.public_key()))
                })
            },
        );
    }
    
    group.finish();
}

fn bench_onion_layer_decryption(c: &mut Criterion) {
    let mut group = c.benchmark_group("onion_decryption");
    
    for size in [512, 1024, 4096, 16384].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        
        let data = vec![0u8; *size];
        let key = KeyPair::generate();
        let encrypted = LayeredEncryption::encrypt_layer(&data, key.public_key()).unwrap();
        
        group.bench_with_input(
            BenchmarkId::new("decrypt_layer", size),
            size,
            |b, _| {
                b.iter(|| {
                    black_box(LayeredEncryption::decrypt_layer(&encrypted, key.private_key()))
                })
            },
        );
    }
    
    group.finish();
}

fn bench_circuit_establishment(c: &mut Criterion) {
    c.bench_function("circuit_establishment", |b| {
        b.iter(|| {
            let manager = CircuitManager::new();
            let mut relay_keys = Vec::new();
            for _ in 0..5 {
                relay_keys.push(KeyPair::generate());
            }
            black_box(manager.establish_circuit(&relay_keys))
        })
    });
}

fn bench_path_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_selection");
    
    for relay_count in [10, 50, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("select_path", relay_count),
            relay_count,
            |b, count| {
                let selector = PathSelector::new();
                b.iter(|| {
                    black_box(selector.select_path(*count, 5))
                })
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_sphinx_packet_creation,
    bench_onion_layer_encryption,
    bench_onion_layer_decryption,
    bench_circuit_establishment,
    bench_path_selection
);
criterion_main!(benches);
