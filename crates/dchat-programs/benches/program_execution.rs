//! Program execution benchmarks

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use dchat_programs::{
    account::{Account, AccountId, AccountInfo},
    instruction::{Instruction, InstructionBuilder},
    metering::ComputeMeter,
    vm::ProgramVm,
};

fn benchmark_compute_meter(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_meter");

    for size in [100, 1000, 10000, 100000] {
        group.bench_with_input(BenchmarkId::new("consume", size), &size, |b, &size| {
            b.iter(|| {
                let mut meter = ComputeMeter::new(1_000_000);
                for _ in 0..size {
                    let _ = meter.consume(black_box(10));
                }
            });
        });
    }

    group.finish();
}

fn benchmark_account_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("account");

    // Create test account
    let account = Account::new(
        AccountId::new([1u8; 32]),
        1_000_000,
        vec![0u8; 1024],
        AccountId::system_program(),
    );

    group.bench_function("serialize", |b| {
        b.iter(|| {
            let _ = black_box(&account).serialize();
        });
    });

    let serialized = account.serialize().unwrap();
    group.bench_function("deserialize", |b| {
        b.iter(|| {
            let _ = Account::deserialize(black_box(&serialized));
        });
    });

    group.finish();
}

fn benchmark_instruction_building(c: &mut Criterion) {
    let mut group = c.benchmark_group("instruction");

    group.bench_function("build_transfer", |b| {
        b.iter(|| {
            InstructionBuilder::new(AccountId::system_program())
                .with_data(black_box(vec![2, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]))
                .with_account(AccountId::new([1u8; 32]), true, true)
                .with_account(AccountId::new([2u8; 32]), true, false)
                .build()
        });
    });

    group.finish();
}

fn benchmark_vm_initialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm");

    // Minimal valid WASM module (empty)
    let wasm_module = wat::parse_str(
        r#"
        (module
            (func (export "entrypoint")
                (nop)
            )
        )
        "#,
    )
    .unwrap();

    group.bench_function("create", |b| {
        b.iter(|| {
            let _ = ProgramVm::new(
                black_box(&wasm_module),
                dchat_programs::vm::VmConfig::default(),
            );
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_compute_meter,
    benchmark_account_serialization,
    benchmark_instruction_building,
    benchmark_vm_initialization,
);

criterion_main!(benches);
