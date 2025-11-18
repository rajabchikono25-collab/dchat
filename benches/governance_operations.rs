use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dchat_governance::voting::GovernanceVoting;
use dchat_governance::moderation::DecentralizedModeration;
use dchat_governance::abuse_reporting::AbuseReporter;
use dchat_core::types::UserId;
use std::hint::black_box;
use uuid::Uuid;

fn bench_proposal_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("governance_proposals");
    
    group.bench_function("create_proposal", |b| {
        b.iter(|| {
            let voting = GovernanceVoting::new();
            let proposer = UserId(Uuid::new_v4());
            
            black_box(voting.create_proposal(
                proposer,
                "Test Proposal",
                "Description of the proposal"
            ))
        })
    });
    
    group.finish();
}

fn bench_vote_casting(c: &mut Criterion) {
    let mut group = c.benchmark_group("vote_casting");
    
    for voter_count in [10, 100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("cast_votes", voter_count),
            voter_count,
            |b, count| {
                b.iter(|| {
                    let voting = GovernanceVoting::new();
                    let proposal_id = Uuid::new_v4();
                    
                    for _ in 0..*count {
                        let voter = UserId(Uuid::new_v4());
                        let _ = voting.cast_vote(proposal_id, voter, true);
                    }
                })
            },
        );
    }
    
    group.finish();
}

fn bench_vote_tallying(c: &mut Criterion) {
    let mut group = c.benchmark_group("vote_tallying");
    
    for vote_count in [100, 1000, 10000, 100000].iter() {
        group.bench_with_input(
            BenchmarkId::new("tally", vote_count),
            vote_count,
            |b, count| {
                b.iter(|| {
                    let voting = GovernanceVoting::new();
                    let proposal_id = Uuid::new_v4();
                    
                    // Setup votes
                    for _ in 0..*count {
                        let voter = UserId(Uuid::new_v4());
                        let _ = voting.cast_vote(proposal_id, voter, true);
                    }
                    
                    black_box(voting.tally_votes(proposal_id))
                })
            },
        );
    }
    
    group.finish();
}

fn bench_moderation_actions(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("moderation");
    
    group.bench_function("submit_report", |b| {
        b.to_async(&rt).iter(|| async {
            let moderation = DecentralizedModeration::new();
            let reporter = UserId(Uuid::new_v4());
            let target = UserId(Uuid::new_v4());
            
            black_box(moderation.submit_report(reporter, target, "spam").await)
        })
    });
    
    group.bench_function("jury_vote", |b| {
        b.to_async(&rt).iter(|| async {
            let moderation = DecentralizedModeration::new();
            let report_id = Uuid::new_v4();
            let juror = UserId(Uuid::new_v4());
            
            black_box(moderation.cast_jury_vote(report_id, juror, true).await)
        })
    });
    
    group.finish();
}

fn bench_abuse_reporting_zk(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("zk_encrypted_report", |b| {
        b.to_async(&rt).iter(|| async {
            let reporter = AbuseReporter::new();
            let user_id = UserId(Uuid::new_v4());
            let content_hash = vec![0u8; 32];
            
            black_box(reporter.create_zk_report(user_id, &content_hash).await)
        })
    });
}

fn bench_voting_power_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("voting_power");
    
    for stake_amount in [1000, 10000, 100000, 1000000].iter() {
        group.bench_with_input(
            BenchmarkId::new("calculate", stake_amount),
            stake_amount,
            |b, stake| {
                b.iter(|| {
                    let voting = GovernanceVoting::new();
                    let user = UserId(Uuid::new_v4());
                    
                    black_box(voting.calculate_voting_power(user, *stake))
                })
            },
        );
    }
    
    group.finish();
}

fn bench_ethics_constraints(c: &mut Criterion) {
    let mut group = c.benchmark_group("ethics");
    
    group.bench_function("check_voting_cap", |b| {
        b.iter(|| {
            let voting = GovernanceVoting::new();
            let user = UserId(Uuid::new_v4());
            
            black_box(voting.check_voting_power_cap(user, 100_000))
        })
    });
    
    group.bench_function("check_term_limits", |b| {
        b.iter(|| {
            let voting = GovernanceVoting::new();
            let user = UserId(Uuid::new_v4());
            
            black_box(voting.check_term_limits(user))
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_proposal_creation,
    bench_vote_casting,
    bench_vote_tallying,
    bench_moderation_actions,
    bench_abuse_reporting_zk,
    bench_voting_power_calculation,
    bench_ethics_constraints
);
criterion_main!(benches);
