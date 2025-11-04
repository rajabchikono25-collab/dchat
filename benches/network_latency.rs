use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dchat_crypto::KeyPair;
use std::hint::black_box;

fn bench_connection_establishment(c: &mut Criterion) {
    c.bench_function("keypair_for_connection", |b| {
        b.iter(|| {
            let _initiator_keypair = KeyPair::generate();
            let _responder_keypair = KeyPair::generate();
            black_box(())
        })
    });
}

fn bench_message_routing(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_routing");

    for num_hops in [1, 2, 3].iter() {
        group.bench_with_input(
            BenchmarkId::new("hops", num_hops),
            num_hops,
            |b, &num_hops| {
                b.iter(|| {
                    // Simulate routing overhead
                    let mut result = 0;
                    for _ in 0..num_hops {
                        result += 1;
                    }
                    black_box(result)
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_connection_establishment,
    bench_message_routing
);
criterion_main!(benches);
