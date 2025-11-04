use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dchat_core::types::{MessageContent, UserId};
use dchat_messaging::{MessageBuilder, MessageQueue};
use std::hint::black_box;

fn bench_message_creation(c: &mut Criterion) {
    c.bench_function("message_creation", |b| {
        let user_id = UserId::default();

        b.iter(|| {
            let msg = MessageBuilder::new()
                .direct(user_id.clone(), user_id.clone())
                .content(MessageContent::Text("test".to_string()))
                .encrypted_payload(vec![1, 2, 3, 4])
                .build()
                .unwrap();
            black_box(msg)
        })
    });
}

fn bench_message_queue_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_queue");

    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("push", size), size, |b, &size| {
            b.iter(|| {
                let mut queue = MessageQueue::new(10000, 10_000_000);
                let user_id = UserId::default();

                for _ in 0..size {
                    let msg = MessageBuilder::new()
                        .direct(user_id.clone(), user_id.clone())
                        .content(MessageContent::Text("Test".to_string()))
                        .encrypted_payload(vec![1, 2, 3])
                        .build()
                        .unwrap();
                    let _ = queue.push(msg);
                }
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_message_creation,
    bench_message_queue_operations
);
criterion_main!(benches);
