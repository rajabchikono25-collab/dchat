use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;

fn bench_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_clients");

    for num_clients in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::new("clients", num_clients),
            num_clients,
            |b, &num_clients| {
                b.iter(|| {
                    // Simulate client load
                    let mut total = 0;
                    for i in 0..num_clients {
                        total += i;
                    }
                    black_box(total)
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_simulation);
criterion_main!(benches);
