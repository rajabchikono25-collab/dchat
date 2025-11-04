#!/usr/bin/env pwsh
# Integration tests against live infrastructure
# Usage: .\run-integration-tests.ps1 -ConfigFile config.dev.toml

param(
    [Parameter(Mandatory=$true)]
    [string]$ConfigFile
)

$ErrorActionPreference = "Stop"

Write-Host "`n🧪 Running integration tests`n" -ForegroundColor Cyan

# Parse config
$config = Get-Content $ConfigFile | ConvertFrom-StringData -Delimiter '='

# Test 1: STUN connectivity
Write-Host "1️⃣ Testing STUN connectivity..." -ForegroundColor Yellow

$env:RUST_LOG = "info"
$stunResult = cargo test -p dchat-network --test integration_stun -- --nocapture 2>&1 | Out-String

if ($LASTEXITCODE -eq 0) {
    Write-Host "   ✅ STUN tests passed" -ForegroundColor Green
} else {
    Write-Host "   ❌ STUN tests failed" -ForegroundColor Red
    exit 1
}

# Test 2: TURN relay
Write-Host "`n2️⃣ Testing TURN relay..." -ForegroundColor Yellow

$turnResult = cargo test -p dchat-network --test integration_turn -- --nocapture 2>&1 | Out-String

if ($LASTEXITCODE -eq 0) {
    Write-Host "   ✅ TURN tests passed" -ForegroundColor Green
} else {
    Write-Host "   ❌ TURN tests failed" -ForegroundColor Red
    exit 1
}

# Test 3: Bootstrap discovery
Write-Host "`n3️⃣ Testing bootstrap node discovery..." -ForegroundColor Yellow

$bootstrapResult = cargo test -p dchat-network --test integration_bootstrap -- --nocapture 2>&1 | Out-String

if ($LASTEXITCODE -eq 0) {
    Write-Host "   ✅ Bootstrap tests passed" -ForegroundColor Green
} else {
    Write-Host "   ❌ Bootstrap tests failed" -ForegroundColor Red
    exit 1
}

# Test 4: Full circuit
Write-Host "`n4️⃣ Testing full onion routing circuit..." -ForegroundColor Yellow

$circuitResult = cargo test -p dchat-network --test integration_circuit -- --nocapture 2>&1 | Out-String

if ($LASTEXITCODE -eq 0) {
    Write-Host "   ✅ Circuit tests passed" -ForegroundColor Green
} else {
    Write-Host "   ❌ Circuit tests failed" -ForegroundColor Red
    exit 1
}

Write-Host "`n✅ All integration tests passed!`n" -ForegroundColor Green

# Generate report
$report = @"
# Integration Test Report
Generated: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")
Config: $ConfigFile

## Results
- STUN Connectivity: ✅ PASSED
- TURN Relay: ✅ PASSED
- Bootstrap Discovery: ✅ PASSED
- Onion Circuit: ✅ PASSED

## Details
$stunResult

$turnResult

$bootstrapResult

$circuitResult
"@

$report | Out-File -FilePath "integration-test-results.md" -Encoding UTF8
Write-Host "Report saved to: integration-test-results.md`n" -ForegroundColor Cyan
