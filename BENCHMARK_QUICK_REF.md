# Quick Benchmark Commands

## Run All Benchmarks
```powershell
# Windows
.\scripts\run_benchmarks.ps1

# Unix/Linux/macOS
./scripts/run_benchmarks.sh
```

## Individual Benchmarks

### Cryptography
```bash
cargo bench --bench crypto_performance
cargo bench --bench post_quantum_crypto
```

### Networking
```bash
cargo bench --bench onion_routing_performance
cargo bench --bench network_latency
cargo bench --bench relay_performance
```

### Storage
```bash
cargo bench --bench storage_backends
cargo bench --bench database_queries
```

### Blockchain
```bash
cargo bench --bench genesis_bootstrap
cargo bench --bench staking_performance
cargo bench --bench cross_chain_bridge
```

### Governance
```bash
cargo bench --bench governance_operations
```

### Application
```bash
cargo bench --bench message_throughput
cargo bench --bench concurrent_clients
cargo bench --bench memory_usage
```

## Start Required Services
```bash
docker-compose -f docker-compose-benchmarks.yml up -d
```

## View Results
```bash
# Open HTML reports
open target/criterion/report/index.html  # macOS
xdg-open target/criterion/report/index.html  # Linux
start target/criterion/report/index.html  # Windows
```

## Performance Targets
- **Crypto signing**: >50,000 ops/s
- **Message latency**: <50ms
- **Storage throughput**: >100,000 ops/s
- **Blockchain TPS**: >1,000 TPS

See [BENCHMARKS.md](docs/BENCHMARKS.md) for full documentation.
