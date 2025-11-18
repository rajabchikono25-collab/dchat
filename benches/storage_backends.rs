use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use dchat_storage::distributed::redis::RedisBackend;
use dchat_storage::distributed::tikv::TiKVBackend;
use dchat_storage::distributed::minio::MinIOBackend;
use dchat_storage::distributed::cockroachdb::CockroachBackend;
use std::hint::black_box;

fn bench_redis_write(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("redis_write");
    
    for size in [100, 1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        
        group.bench_with_input(BenchmarkId::new("write", size), &data, |b, data| {
            b.to_async(&rt).iter(|| async {
                let backend = RedisBackend::new("redis://localhost:6379").await.unwrap();
                black_box(backend.set("benchmark_key", data).await)
            })
        });
    }
    
    group.finish();
}

fn bench_redis_read(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("redis_read");
    
    for size in [100, 1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        
        // Setup
        rt.block_on(async {
            let backend = RedisBackend::new("redis://localhost:6379").await.unwrap();
            backend.set("benchmark_key", &data).await.unwrap();
        });
        
        group.bench_function(BenchmarkId::new("read", size), |b| {
            b.to_async(&rt).iter(|| async {
                let backend = RedisBackend::new("redis://localhost:6379").await.unwrap();
                black_box(backend.get::<Vec<u8>>("benchmark_key").await)
            })
        });
    }
    
    group.finish();
}

fn bench_tikv_write(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("tikv_write");
    
    for size in [100, 1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        
        group.bench_with_input(BenchmarkId::new("write", size), &data, |b, data| {
            b.to_async(&rt).iter(|| async {
                let backend = TiKVBackend::new(vec!["127.0.0.1:2379"]).await.unwrap();
                black_box(backend.put(b"benchmark_key", data).await)
            })
        });
    }
    
    group.finish();
}

fn bench_tikv_read(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("tikv_read");
    
    for size in [100, 1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        
        // Setup
        rt.block_on(async {
            let backend = TiKVBackend::new(vec!["127.0.0.1:2379"]).await.unwrap();
            backend.put(b"benchmark_key", &data).await.unwrap();
        });
        
        group.bench_function(BenchmarkId::new("read", size), |b| {
            b.to_async(&rt).iter(|| async {
                let backend = TiKVBackend::new(vec!["127.0.0.1:2379"]).await.unwrap();
                black_box(backend.get(b"benchmark_key").await)
            })
        });
    }
    
    group.finish();
}

fn bench_minio_write(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("minio_write");
    
    for size in [1024, 10240, 102400, 1024000].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        let data = vec![0u8; *size];
        
        group.bench_with_input(BenchmarkId::new("write", size), &data, |b, data| {
            b.to_async(&rt).iter(|| async {
                let backend = MinIOBackend::new(
                    "http://localhost:9000",
                    "minioadmin",
                    "minioadmin",
                    "benchmark-bucket"
                ).await.unwrap();
                black_box(backend.put_object("benchmark_key", data).await)
            })
        });
    }
    
    group.finish();
}

fn bench_cockroachdb_write(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("cockroachdb_write");
    
    for rows in [1, 10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("write", rows), rows, |b, rows| {
            b.to_async(&rt).iter(|| async {
                let backend = CockroachBackend::new("postgresql://root@localhost:26257/defaultdb")
                    .await.unwrap();
                
                for i in 0..*rows {
                    let _ = backend.execute_query(
                        "INSERT INTO benchmark_table (id, data) VALUES ($1, $2)",
                        &[&i.to_string(), &vec![0u8; 1024]]
                    ).await;
                }
            })
        });
    }
    
    group.finish();
}

fn bench_cockroachdb_read(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("cockroachdb_read");
    
    for rows in [1, 10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("read", rows), rows, |b, rows| {
            b.to_async(&rt).iter(|| async {
                let backend = CockroachBackend::new("postgresql://root@localhost:26257/defaultdb")
                    .await.unwrap();
                
                black_box(backend.query(
                    "SELECT * FROM benchmark_table LIMIT $1",
                    &[rows]
                ).await)
            })
        });
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_redis_write,
    bench_redis_read,
    bench_tikv_write,
    bench_tikv_read,
    bench_minio_write,
    bench_cockroachdb_write,
    bench_cockroachdb_read
);
criterion_main!(benches);
