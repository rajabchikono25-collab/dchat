#!/usr/bin/env pwsh
# Comprehensive Benchmark Suite Runner for dchat
# Runs all benchmarks and generates detailed performance reports

Write-Host "==================================================" -ForegroundColor Cyan
Write-Host "  dchat Comprehensive Benchmark Suite" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Cyan
Write-Host ""

$ErrorActionPreference = "Continue"
$benchmarks = @(
    "crypto_performance",
    "post_quantum_crypto",
    "onion_routing_performance",
    "message_throughput",
    "network_latency",
    "relay_performance",
    "storage_backends",
    "database_queries",
    "staking_performance",
    "genesis_bootstrap",
    "cross_chain_bridge",
    "governance_operations",
    "memory_usage",
    "concurrent_clients"
)

$results = @()
$timestamp = Get-Date -Format "yyyy-MM-dd_HH-mm-ss"
$reportDir = "target/benchmark_reports/$timestamp"

# Create report directory
New-Item -ItemType Directory -Force -Path $reportDir | Out-Null

Write-Host "Starting benchmark suite at $(Get-Date)" -ForegroundColor Green
Write-Host "Reports will be saved to: $reportDir" -ForegroundColor Yellow
Write-Host ""

foreach ($benchmark in $benchmarks) {
    Write-Host "Running benchmark: $benchmark" -ForegroundColor Cyan
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
    
    $startTime = Get-Date
    
    try {
        $output = cargo bench --bench $benchmark 2>&1 | Tee-Object -Variable benchOutput
        $endTime = Get-Date
        $duration = ($endTime - $startTime).TotalSeconds
        
        $status = if ($LASTEXITCODE -eq 0) { "✓ SUCCESS" } else { "✗ FAILED" }
        $statusColor = if ($LASTEXITCODE -eq 0) { "Green" } else { "Red" }
        
        Write-Host "$status - Completed in $([math]::Round($duration, 2))s" -ForegroundColor $statusColor
        
        $results += [PSCustomObject]@{
            Benchmark = $benchmark
            Status = $status
            Duration = $duration
            ExitCode = $LASTEXITCODE
        }
        
        # Save individual benchmark output
        $output | Out-File "$reportDir/$benchmark.txt"
        
    } catch {
        Write-Host "✗ ERROR: $($_.Exception.Message)" -ForegroundColor Red
        
        $results += [PSCustomObject]@{
            Benchmark = $benchmark
            Status = "✗ ERROR"
            Duration = 0
            ExitCode = -1
        }
    }
    
    Write-Host ""
}

# Generate summary report
Write-Host "==================================================" -ForegroundColor Cyan
Write-Host "  Benchmark Summary" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Cyan
Write-Host ""

$results | Format-Table -AutoSize

$successCount = ($results | Where-Object { $_.Status -eq "✓ SUCCESS" }).Count
$failCount = ($results | Where-Object { $_.Status -ne "✓ SUCCESS" }).Count
$totalDuration = ($results | Measure-Object -Property Duration -Sum).Sum

Write-Host "Total benchmarks: $($results.Count)" -ForegroundColor White
Write-Host "Successful: $successCount" -ForegroundColor Green
Write-Host "Failed: $failCount" -ForegroundColor $(if ($failCount -gt 0) { "Red" } else { "Green" })
Write-Host "Total execution time: $([math]::Round($totalDuration, 2))s" -ForegroundColor Yellow
Write-Host ""

# Save summary
$summaryReport = @"
dchat Benchmark Suite Summary
Generated: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")
================================================================

Total Benchmarks: $($results.Count)
Successful: $successCount
Failed: $failCount
Total Execution Time: $([math]::Round($totalDuration, 2))s

Individual Results:
================================================================

$($results | Format-Table -AutoSize | Out-String)

Detailed Reports Location: $reportDir
"@

$summaryReport | Out-File "$reportDir/SUMMARY.txt"

Write-Host "Summary report saved to: $reportDir/SUMMARY.txt" -ForegroundColor Green
Write-Host ""

# Open Criterion HTML reports if available
$criterionIndex = "target/criterion/report/index.html"
if (Test-Path $criterionIndex) {
    Write-Host "Opening Criterion HTML reports..." -ForegroundColor Cyan
    Start-Process $criterionIndex
}

Write-Host "Benchmark suite completed at $(Get-Date)" -ForegroundColor Green
