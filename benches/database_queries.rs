use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dchat_core::types::{MessageContent, UserId};
use dchat_messaging::MessageBuilder;
use std::hint::black_box;

fn bench_message_retrieval(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_retrieval");

    for db_size in [100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("creation", db_size),
            db_size,
            |b, &db_size| {
                b.iter(|| {
                    let user_id = UserId::default();
                    let messages: Vec<_> = (0..db_size)
                        .map(|i| {
                            MessageBuilder::new()
                                .direct(user_id.clone(), user_id.clone())
                                .content(MessageContent::Text(format!("Message {}", i)))
                                .encrypted_payload(vec![1, 2, 3])
                                .build()
                                .unwrap()
                        })
                        .collect();
                    black_box(messages)
                })
            },
        );
    }

    group.finish();
}

fn bench_database_initialization(c: &mut Criterion) {
    c.bench_function("db_init", |b| {
        b.iter(|| {
            let temp_dir = tempfile::tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            black_box(db_path)
        })
    });
}

criterion_group!(
    benches,
    bench_message_retrieval,
    bench_database_initialization
);
criterion_main!(benches);
