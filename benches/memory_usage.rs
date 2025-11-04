use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dchat_messaging::MessageQueue;
use std::hint::black_box;

fn bench_message_queue_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue_memory");

    for queue_size in [100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("size", queue_size),
            queue_size,
            |b, &queue_size| {
                b.iter(|| {
                    let queue = MessageQueue::new(queue_size, 1_000_000);
                    black_box(queue)
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_message_queue_memory);
criterion_main!(benches);
