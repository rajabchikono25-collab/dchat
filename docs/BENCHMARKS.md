# dchat Benchmark Suite Documentation

## Overview

This comprehensive benchmark suite tests all critical functionalities of the dchat decentralized chat system. It measures performance across cryptography, networking, storage, blockchain operations, and governance.

## Benchmark Categories

### 1. Cryptography Benchmarks

#### `crypto_performance.rs`
- **Key generation**: Ed25519 keypair generation
- **Signing**: Ed25519 signature creation (100B, 1KB, 10KB)
- **Verification**: Ed25519 signature verification

#### `post_quantum_crypto.rs`
- **Dilithium3**: Key generation, signing, verification
- **Kyber768**: Key generation, encapsulation, decapsulation
- **Falcon**: Key generation, signing
- **Hybrid schemes**: Combined classical + post-quantum operations

**Key Metrics**: Operations/second, throughput (MB/s), latency (μs)

### 2. Network Benchmarks

#### `onion_routing_performance.rs`
- **Sphinx packet creation**: 3, 5, 7-hop circuits
- **Layer encryption**: 512B to 16KB payloads
- **Layer decryption**: Multi-hop packet processing
- **Circuit establishment**: Full circuit setup time
- **Path selection**: Relay selection algorithms (10-500 relays)

#### `network_latency.rs`
- **Peer discovery**: DHT lookups, bootstrap times
- **Message routing**: Direct and multi-hop delivery
- **NAT traversal**: UPnP, TURN, hole punching

#### `relay_performance.rs`
- **Message forwarding**: Relay throughput
- **Proof-of-delivery**: PoD generation and verification
- **Uptime scoring**: Reputation calculations

**Key Metrics**: Latency (ms), throughput (msg/s), packet loss (%)

### 3. Storage Benchmarks

#### `storage_backends.rs`
- **Redis**: In-memory cache read/write (100B-100KB)
- **TiKV**: Distributed KV operations
- **MinIO**: S3-compatible object storage (1KB-1MB)
- **CockroachDB**: Distributed SQL queries (1-1000 rows)

#### `database_queries.rs`
- **SQLite**: Local database operations
- **RocksDB**: Key-value store performance
- **Query optimization**: Index usage, join performance

**Key Metrics**: Throughput (ops/s), latency (ms), bandwidth (MB/s)

### 4. Blockchain Benchmarks

#### `genesis_bootstrap.rs`
- **Genesis block creation**: Chat and Currency chains
- **Genesis synchronization**: Dual-chain coordination
- **Bootstrap initialization**: Chain activation
- **First validator detection**: Stake verification
- **Bridge activation**: Cross-chain bridge setup

#### `cross_chain_bridge.rs`
- **Cross-chain transfers**: 100-100K token transfers
- **Atomic swaps**: Dual-chain atomic transactions
- **State synchronization**: Chain state consistency
- **Finality verification**: 1-12 confirmation checks
- **Concurrent operations**: 10-500 parallel transfers

#### `staking_performance.rs`
- **Validator stake verification**: 10K-500K token checks
- **Relay staking**: 1K token minimum enforcement
- **Storage bond calculation**: 1MB-1GB storage bonds
- **Slashing operations**: 10-100% penalty calculations
- **Concurrent stake checks**: 10-500 parallel verifications

**Key Metrics**: Transactions/second (TPS), block time (s), finality time (s)

### 5. Governance Benchmarks

#### `governance_operations.rs`
- **Proposal creation**: Governance proposal initialization
- **Vote casting**: 10-10K voter participation
- **Vote tallying**: 100-100K vote aggregation
- **Moderation actions**: Abuse report submission, jury voting
- **ZK abuse reporting**: Zero-knowledge encrypted reports
- **Voting power calculation**: Stake-weighted voting (1K-1M tokens)
- **Ethics constraints**: Voting caps, term limits

**Key Metrics**: Operations/second, vote processing time (ms), fairness score

### 6. Application Benchmarks

#### `message_throughput.rs`
- **Message creation**: End-to-end encrypted message generation
- **Message delivery**: Offline queue, sync protocols
- **Channel operations**: Message broadcast, filtering
- **Message ordering**: On-chain sequence verification

#### `concurrent_clients.rs`
- **Multi-user simulation**: 10-10K concurrent users
- **Concurrent messaging**: Parallel send/receive
- **Resource usage**: Memory, CPU, network under load

#### `memory_usage.rs`
- **Memory allocation**: Per-user, per-channel overhead
- **Cache efficiency**: Redis hit rates, eviction policies
- **Memory leaks**: Long-running stress tests

**Key Metrics**: Messages/second, concurrent users, memory (MB), CPU (%)

## Running Benchmarks

### Run All Benchmarks
```powershell
# Windows
.\scripts\run_benchmarks.ps1

# Unix/Linux/macOS
./scripts/run_benchmarks.sh
```

### Run Individual Benchmark
```bash
cargo bench --bench crypto_performance
cargo bench --bench onion_routing_performance
cargo bench --bench storage_backends
```

### Run Specific Benchmark Function
```bash
cargo bench --bench crypto_performance -- bench_signing
cargo bench --bench staking_performance -- bench_validator_stake
```

### Run with Filtering
```bash
# Run all benchmarks with "dilithium" in the name
cargo bench -- dilithium

# Run all storage benchmarks
cargo bench --bench storage_backends
```

## Viewing Results

### HTML Reports
Criterion generates beautiful HTML reports with graphs:
```
target/criterion/report/index.html
```

Open in browser for:
- Performance trend graphs
- Throughput comparisons
- Latency distributions
- Statistical analysis

### Terminal Output
- Real-time progress during benchmark execution
- Statistical summary (mean, median, std dev)
- Performance comparison with previous runs

### Text Reports
Generated reports saved to:
```
target/benchmark_reports/<timestamp>/
├── SUMMARY.txt
├── crypto_performance.txt
├── onion_routing_performance.txt
├── storage_backends.txt
└── ...
```

## Performance Targets

### Cryptography
- Ed25519 signing: **>50,000 ops/s**
- Dilithium3 signing: **>10,000 ops/s**
- Kyber768 encapsulation: **>20,000 ops/s**

### Networking
- Message latency (direct): **<50ms**
- Message latency (5-hop onion): **<500ms**
- Relay throughput: **>10,000 msg/s**

### Storage
- Redis cache: **>100,000 ops/s**
- TiKV distributed: **>10,000 ops/s**
- MinIO object storage: **>100 MB/s**

### Blockchain
- Transaction throughput: **>1,000 TPS**
- Block time: **<5s**
- Cross-chain finality: **<30s**

### Governance
- Vote processing: **>10,000 votes/s**
- Proposal creation: **<100ms**

## Prerequisites

### Required Services
Some benchmarks require external services:

1. **Redis** (port 6379)
   ```bash
   docker run -d -p 6379:6379 redis:latest
   ```

2. **TiKV** (port 2379)
   ```bash
   docker run -d -p 2379:2379 tikv/tikv:latest
   ```

3. **MinIO** (port 9000)
   ```bash
   docker run -d -p 9000:9000 minio/minio:latest server /data
   ```

4. **CockroachDB** (port 26257)
   ```bash
   docker run -d -p 26257:26257 cockroachdb/cockroach:latest start-single-node --insecure
   ```

### Optional: Run All Services
```bash
docker-compose -f docker-compose-benchmarks.yml up -d
```

## Configuration

### Benchmark Duration
Edit `Cargo.toml` to adjust benchmark parameters:
```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports", "async_tokio"] }
```

### Sample Size
Modify benchmark files to change iteration counts:
```rust
group.sample_size(100);  // Default: 100
group.measurement_time(Duration::from_secs(10));  // Default: 5s
```

## Continuous Integration

### GitHub Actions
```yaml
- name: Run Benchmarks
  run: |
    cargo bench --all-features
    
- name: Upload Results
  uses: actions/upload-artifact@v3
  with:
    name: benchmark-results
    path: target/criterion/
```

### Performance Regression Detection
Criterion automatically compares with previous runs and warns on regressions:
- **>5% regression**: Warning
- **>10% regression**: Error

## Troubleshooting

### Benchmark Hangs
- Increase timeout: `CRITERION_TIMEOUT=300 cargo bench`
- Reduce sample size in benchmark file

### Out of Memory
- Run benchmarks individually
- Reduce concurrent operation counts
- Increase system swap space

### Network Errors
- Check service availability: `docker ps`
- Verify ports: `netstat -an | grep LISTEN`
- Review firewall rules

### Compilation Issues
```bash
# Clean and rebuild
cargo clean
cargo build --release --all-features
cargo bench
```

## Best Practices

1. **Consistent Environment**: Run on same hardware for valid comparisons
2. **Isolated System**: Minimize background processes
3. **Multiple Runs**: Average results from 3+ runs
4. **Warm-up**: First run may be slower (JIT, caching)
5. **Production Config**: Use `--release` profile settings
6. **Version Control**: Track benchmark results over time

## Interpreting Results

### Understanding Statistics
- **Mean**: Average performance
- **Median**: Middle value (less affected by outliers)
- **Std Dev**: Performance consistency
- **Outliers**: Investigate if >10% of samples

### Performance Trends
- **Flat**: Stable performance ✓
- **Upward**: Improving performance ✓
- **Downward**: Performance regression ✗
- **Spiky**: Inconsistent, investigate cause

### Comparison Baseline
```bash
# Save baseline
cargo bench -- --save-baseline main

# Compare to baseline
cargo bench -- --baseline main
```

## Contributing

### Adding New Benchmarks
1. Create file in `benches/new_benchmark.rs`
2. Use Criterion framework
3. Add `[[bench]]` entry to `Cargo.toml`
4. Update this documentation
5. Run `cargo bench --bench new_benchmark`

### Benchmark Template
```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_feature(c: &mut Criterion) {
    c.bench_function("feature_name", |b| {
        b.iter(|| {
            // Code to benchmark
        })
    });
}

criterion_group!(benches, bench_feature);
criterion_main!(benches);
```

## Resources

- [Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Flamegraph Profiling](https://github.com/flamegraph-rs/flamegraph)

## License

MIT OR Apache-2.0
