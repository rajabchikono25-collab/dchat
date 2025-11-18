use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dchat_blockchain::cross_chain::CrossChainBridge;
use dchat_blockchain::chat_chain::{ChatChainClient, ChatChainConfig};
use dchat_blockchain::currency_chain::{CurrencyChainClient, CurrencyChainConfig};
use dchat_core::types::UserId;
use std::hint::black_box;
use std::sync::Arc;
use uuid::Uuid;

fn bench_cross_chain_transfer(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("cross_chain_transfer");
    
    for amount in [100, 1000, 10000, 100000].iter() {
        group.bench_with_input(
            BenchmarkId::new("transfer", amount),
            amount,
            |b, amount| {
                b.to_async(&rt).iter(|| async {
                    let chat_client = Arc::new(
                        ChatChainClient::new_mock(ChatChainConfig::default())
                    );
                    let currency_client = Arc::new(
                        CurrencyChainClient::new_mock(CurrencyChainConfig::default())
                    );
                    
                    let bridge = CrossChainBridge::new(chat_client, currency_client);
                    let from = UserId(Uuid::new_v4());
                    let to = UserId(Uuid::new_v4());
                    
                    black_box(bridge.transfer(&from, &to, *amount).await)
                })
            },
        );
    }
    
    group.finish();
}

fn bench_atomic_swap(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("atomic_swap", |b| {
        b.to_async(&rt).iter(|| async {
            let chat_client = Arc::new(
                ChatChainClient::new_mock(ChatChainConfig::default())
            );
            let currency_client = Arc::new(
                CurrencyChainClient::new_mock(CurrencyChainConfig::default())
            );
            
            let bridge = CrossChainBridge::new(chat_client, currency_client);
            let user_a = UserId(Uuid::new_v4());
            let user_b = UserId(Uuid::new_v4());
            
            black_box(bridge.atomic_swap(&user_a, &user_b, 1000, 2000).await)
        })
    });
}

fn bench_state_synchronization(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("sync_state", |b| {
        b.to_async(&rt).iter(|| async {
            let chat_client = Arc::new(
                ChatChainClient::new_mock(ChatChainConfig::default())
            );
            let currency_client = Arc::new(
                CurrencyChainClient::new_mock(CurrencyChainConfig::default())
            );
            
            let bridge = CrossChainBridge::new(chat_client, currency_client);
            
            black_box(bridge.synchronize_state().await)
        })
    });
}

fn bench_finality_verification(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("finality_verification");
    
    for confirmations in [1, 3, 6, 12].iter() {
        group.bench_with_input(
            BenchmarkId::new("verify", confirmations),
            confirmations,
            |b, confs| {
                b.to_async(&rt).iter(|| async {
                    let chat_client = Arc::new(
                        ChatChainClient::new_mock(ChatChainConfig::default())
                    );
                    let currency_client = Arc::new(
                        CurrencyChainClient::new_mock(CurrencyChainConfig::default())
                    );
                    
                    let bridge = CrossChainBridge::new(chat_client, currency_client);
                    
                    black_box(bridge.verify_finality("tx_hash", *confs).await)
                })
            },
        );
    }
    
    group.finish();
}

fn bench_concurrent_bridge_operations(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_operations");
    
    for op_count in [10, 50, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("transfers", op_count),
            op_count,
            |b, count| {
                b.to_async(&rt).iter(|| async {
                    let chat_client = Arc::new(
                        ChatChainClient::new_mock(ChatChainConfig::default())
                    );
                    let currency_client = Arc::new(
                        CurrencyChainClient::new_mock(CurrencyChainConfig::default())
                    );
                    
                    let bridge = CrossChainBridge::new(chat_client, currency_client);
                    
                    let mut handles = Vec::new();
                    for _ in 0..*count {
                        let bridge = bridge.clone();
                        let handle = tokio::spawn(async move {
                            let from = UserId(Uuid::new_v4());
                            let to = UserId(Uuid::new_v4());
                            bridge.transfer(&from, &to, 100).await
                        });
                        handles.push(handle);
                    }
                    
                    for handle in handles {
                        let _ = handle.await;
                    }
                })
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_cross_chain_transfer,
    bench_atomic_swap,
    bench_state_synchronization,
    bench_finality_verification,
    bench_concurrent_bridge_operations
);
criterion_main!(benches);
