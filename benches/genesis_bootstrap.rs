use criterion::{criterion_group, criterion_main, Criterion};
use dchat_chain::chain::genesis::{GenesisCoordinator, ChatGenesisBlock, CurrencyGenesisBlock};
use dchat_chain::chain::bootstrap::BootstrapCoordinator;
use dchat_blockchain::chat_chain::{ChatChainClient, ChatChainConfig};
use dchat_blockchain::currency_chain::{CurrencyChainClient, CurrencyChainConfig};
use dchat_crypto::KeyPair;
use std::hint::black_box;
use std::sync::Arc;

fn bench_genesis_block_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("genesis");
    
    group.bench_function("create_chat_genesis", |b| {
        b.iter(|| {
            let keypair = KeyPair::generate();
            black_box(ChatGenesisBlock::new(keypair))
        })
    });
    
    group.bench_function("create_currency_genesis", |b| {
        b.iter(|| {
            let keypair = KeyPair::generate();
            black_box(CurrencyGenesisBlock::new(keypair, 1_000_000_000))
        })
    });
    
    group.finish();
}

fn bench_genesis_synchronization(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("synchronize_genesis_blocks", |b| {
        b.to_async(&rt).iter(|| async {
            let coordinator = GenesisCoordinator::new();
            let chat_keypair = KeyPair::generate();
            let currency_keypair = KeyPair::generate();
            
            black_box(coordinator.synchronize_genesis(
                chat_keypair,
                currency_keypair,
                1_000_000_000
            ).await)
        })
    });
}

fn bench_bootstrap_initialization(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("bootstrap_chains", |b| {
        b.to_async(&rt).iter(|| async {
            let chat_client = Arc::new(
                ChatChainClient::new_mock(ChatChainConfig::default())
            );
            let currency_client = Arc::new(
                CurrencyChainClient::new_mock(CurrencyChainConfig::default())
            );
            
            let coordinator = BootstrapCoordinator::new(
                chat_client.clone(),
                currency_client.clone()
            );
            
            black_box(coordinator.initialize_chains().await)
        })
    });
}

fn bench_first_validator_detection(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("monitor_first_validator", |b| {
        b.to_async(&rt).iter(|| async {
            let chat_client = Arc::new(
                ChatChainClient::new_mock(ChatChainConfig::default())
            );
            let currency_client = Arc::new(
                CurrencyChainClient::new_mock(CurrencyChainConfig::default())
            );
            
            let coordinator = BootstrapCoordinator::new(
                chat_client.clone(),
                currency_client.clone()
            );
            
            // Simulate validator stake check
            black_box(coordinator.check_validator_ready().await)
        })
    });
}

fn bench_bridge_activation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("activate_bridge", |b| {
        b.to_async(&rt).iter(|| async {
            let chat_client = Arc::new(
                ChatChainClient::new_mock(ChatChainConfig::default())
            );
            let currency_client = Arc::new(
                CurrencyChainClient::new_mock(CurrencyChainConfig::default())
            );
            
            let coordinator = BootstrapCoordinator::new(
                chat_client.clone(),
                currency_client.clone()
            );
            
            black_box(coordinator.activate_bridge().await)
        })
    });
}

criterion_group!(
    benches,
    bench_genesis_block_creation,
    bench_genesis_synchronization,
    bench_bootstrap_initialization,
    bench_first_validator_detection,
    bench_bridge_activation
);
criterion_main!(benches);
