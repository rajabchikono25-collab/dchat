//! Mini-app benchmarks

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use dchat_miniapps::{
    intent::{IntentPayload, IntentRateLimiter, IntentType},
    manifest::{AppManifest, ManifestBuilder, ManifestVersion},
    permissions::{Permission, PermissionRequest, PermissionSet},
    receipt::{Attestation, ExecutionResult, Receipt, ReceiptStatus},
    registry::{AppId, AppRegistration, DeveloperId},
    sandbox::SandboxConfig,
};

fn benchmark_intent_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("intent");

    group.bench_function("create_transfer", |b| {
        b.iter(|| {
            IntentPayload::new(
                black_box(AppId([1u8; 32])),
                black_box(IntentType::Transfer {
                    to: "recipient_address".to_string(),
                    amount: 1000000,
                }),
            )
        });
    });

    group.bench_function("hash", |b| {
        let intent = IntentPayload::new(
            AppId([1u8; 32]),
            IntentType::Transfer {
                to: "recipient_address".to_string(),
                amount: 1000000,
            },
        );
        b.iter(|| black_box(&intent).hash());
    });

    group.finish();
}

fn benchmark_rate_limiter(c: &mut Criterion) {
    let mut group = c.benchmark_group("rate_limiter");

    group.bench_function("check_allowed", |b| {
        let limiter = IntentRateLimiter::new(60, std::time::Duration::from_secs(60));
        let user_id = "user123";

        b.iter(|| {
            // Create fresh limiter each iteration to avoid hitting limit
            let limiter = IntentRateLimiter::new(1000, std::time::Duration::from_secs(60));
            limiter.check(black_box(user_id))
        });
    });

    group.finish();
}

fn benchmark_permission_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("permissions");

    group.bench_function("add_permissions", |b| {
        b.iter(|| {
            let mut set = PermissionSet::new();
            set.add(black_box(Permission::ReadProfile));
            set.add(black_box(Permission::ViewBalance));
            set.add(black_box(Permission::SendTokens));
            set.add(black_box(Permission::ReadMessages));
            set
        });
    });

    group.bench_function("has_permission", |b| {
        let mut set = PermissionSet::new();
        set.add(Permission::ReadProfile);
        set.add(Permission::ViewBalance);
        set.add(Permission::SendTokens);

        b.iter(|| set.has(black_box(&Permission::ViewBalance)));
    });

    group.bench_function("max_risk_level", |b| {
        let mut set = PermissionSet::new();
        set.add(Permission::ReadProfile);
        set.add(Permission::ViewBalance);
        set.add(Permission::SendTokens);
        set.add(Permission::WriteStorage);

        b.iter(|| black_box(&set).max_risk_level());
    });

    group.finish();
}

fn benchmark_manifest_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("manifest");

    group.bench_function("create_and_validate", |b| {
        b.iter(|| {
            ManifestBuilder::new("test_app", "1.0.0")
                .display_name("Test App")
                .description("A test application")
                .entry_point("https://app.example.com")
                .add_permission(Permission::ReadProfile)
                .add_permission(Permission::ViewBalance)
                .build()
        });
    });

    group.finish();
}

fn benchmark_receipt_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("receipt");

    group.bench_function("create", |b| {
        b.iter(|| {
            Receipt::new(
                black_box("intent_123".to_string()),
                black_box("dchat-1".to_string()),
                black_box(ExecutionResult::success(vec![1, 2, 3, 4])),
            )
        });
    });

    group.bench_function("hash", |b| {
        let receipt = Receipt::new(
            "intent_123".to_string(),
            "dchat-1".to_string(),
            ExecutionResult::success(vec![1, 2, 3, 4]),
        );

        b.iter(|| black_box(&receipt).hash());
    });

    group.finish();
}

fn benchmark_sandbox_config(c: &mut Criterion) {
    let mut group = c.benchmark_group("sandbox");

    group.bench_function("create_default", |b| {
        b.iter(|| SandboxConfig::default());
    });

    group.bench_function("create_custom", |b| {
        b.iter(|| {
            SandboxConfig::new(
                black_box(64 * 1024 * 1024),
                black_box(30),
                black_box(10 * 1024 * 1024),
            )
        });
    });

    group.finish();
}

fn benchmark_app_id_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry");

    group.bench_function("create_app_id", |b| {
        let developer_id = DeveloperId::from_public_key(&[1u8; 32]);
        b.iter(|| AppId::derive(black_box(&developer_id), black_box("test_app")));
    });

    group.bench_function("create_developer_id", |b| {
        b.iter(|| DeveloperId::from_public_key(black_box(&[1u8; 32])));
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_intent_creation,
    benchmark_rate_limiter,
    benchmark_permission_set,
    benchmark_manifest_validation,
    benchmark_receipt_creation,
    benchmark_sandbox_config,
    benchmark_app_id_creation,
);

criterion_main!(benches);
