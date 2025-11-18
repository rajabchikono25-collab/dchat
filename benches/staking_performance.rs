use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dchat_chain::chain::currency_chain::validator_enforcement::ValidatorStakingEnforcer;
use dchat_network::relay::staking::RelayStakingManager;
use dchat_storage::economics::storage_bonds::StorageBondManager;
use dchat_blockchain::currency_chain::{CurrencyChainClient, CurrencyChainConfig};
use dchat_core::types::UserId;
use std::hint::black_box;
use uuid::Uuid;

fn bench_validator_stake_verification(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("validator_staking");
    
    for stake_amount in [10_000, 50_000, 100_000, 500_000].iter() {
        group.bench_with_input(
            BenchmarkId::new("verify_stake", stake_amount),
            stake_amount,
            |b, amount| {
                b.to_async(&rt).iter(|| async {
                    let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
                    let enforcer = ValidatorStakingEnforcer::new(client);
                    let validator_id = UserId(Uuid::new_v4());
                    
                    black_box(enforcer.verify_validator_stake(&validator_id, *amount).await)
                })
            },
        );
    }
    
    group.finish();
}

fn bench_relay_stake_slashing(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("relay_slashing");
    
    for slash_percent in [10, 25, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::new("slash", slash_percent),
            slash_percent,
            |b, percent| {
                b.to_async(&rt).iter(|| async {
                    let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
                    let manager = RelayStakingManager::new(client);
                    let relay_id = UserId(Uuid::new_v4());
                    
                    black_box(manager.slash_relay(&relay_id, *percent).await)
                })
            },
        );
    }
    
    group.finish();
}

fn bench_storage_bond_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_bonds");
    
    for size_mb in [1, 10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("calculate_bond", size_mb),
            size_mb,
            |b, size| {
                b.iter(|| {
                    let manager = StorageBondManager::new();
                    black_box(manager.calculate_bond(*size * 1024 * 1024))
                })
            },
        );
    }
    
    group.finish();
}

fn bench_concurrent_stake_checks(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_stake_checks");
    
    for validator_count in [10, 50, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("verify_multiple", validator_count),
            validator_count,
            |b, count| {
                b.to_async(&rt).iter(|| async {
                    let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
                    let enforcer = ValidatorStakingEnforcer::new(client);
                    
                    let mut handles = Vec::new();
                    for _ in 0..*count {
                        let enforcer = enforcer.clone();
                        let handle = tokio::spawn(async move {
                            let validator_id = UserId(Uuid::new_v4());
                            enforcer.verify_validator_stake(&validator_id, 10_000).await
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
    bench_validator_stake_verification,
    bench_relay_stake_slashing,
    bench_storage_bond_calculation,
    bench_concurrent_stake_checks
);
criterion_main!(benches);
