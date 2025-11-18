#!/usr/bin/env bash
# Comprehensive Benchmark Suite Runner for dchat (Unix/Linux/macOS)
# Runs all benchmarks and generates detailed performance reports

set -e

echo "=================================================="
echo "  dchat Comprehensive Benchmark Suite"
echo "=================================================="
echo ""

benchmarks=(
    "crypto_performance"
    "post_quantum_crypto"
    "onion_routing_performance"
    "message_throughput"
    "network_latency"
    "relay_performance"
    "storage_backends"
    "database_queries"
    "staking_performance"
    "genesis_bootstrap"
    "cross_chain_bridge"
    "governance_operations"
    "memory_usage"
    "concurrent_clients"
)

timestamp=$(date +"%Y-%m-%d_%H-%M-%S")
report_dir="target/benchmark_reports/$timestamp"

# Create report directory
mkdir -p "$report_dir"

echo "Starting benchmark suite at $(date)"
echo "Reports will be saved to: $report_dir"
echo ""

success_count=0
fail_count=0
total_duration=0

for benchmark in "${benchmarks[@]}"; do
    echo "Running benchmark: $benchmark"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    start_time=$(date +%s)
    
    if cargo bench --bench "$benchmark" 2>&1 | tee "$report_dir/$benchmark.txt"; then
        end_time=$(date +%s)
        duration=$((end_time - start_time))
        total_duration=$((total_duration + duration))
        
        echo "✓ SUCCESS - Completed in ${duration}s"
        ((success_count++))
    else
        echo "✗ FAILED"
        ((fail_count++))
    fi
    
    echo ""
done

# Generate summary
echo "=================================================="
echo "  Benchmark Summary"
echo "=================================================="
echo ""
echo "Total benchmarks: ${#benchmarks[@]}"
echo "Successful: $success_count"
echo "Failed: $fail_count"
echo "Total execution time: ${total_duration}s"
echo ""

# Save summary
cat > "$report_dir/SUMMARY.txt" <<EOF
dchat Benchmark Suite Summary
Generated: $(date)
================================================================

Total Benchmarks: ${#benchmarks[@]}
Successful: $success_count
Failed: $fail_count
Total Execution Time: ${total_duration}s

Detailed Reports Location: $report_dir
EOF

echo "Summary report saved to: $report_dir/SUMMARY.txt"
echo ""

# Open Criterion HTML reports if available
criterion_index="target/criterion/report/index.html"
if [ -f "$criterion_index" ]; then
    echo "Opening Criterion HTML reports..."
    if command -v xdg-open &> /dev/null; then
        xdg-open "$criterion_index"
    elif command -v open &> /dev/null; then
        open "$criterion_index"
    fi
fi

echo "Benchmark suite completed at $(date)"
