#!/usr/bin/env pwsh
# Test script for fork signature verification implementation

Write-Host "Testing Fork Signature Verification Implementation" -ForegroundColor Green
Write-Host "=================================================" -ForegroundColor Green
Write-Host ""

Write-Host "Step 1: Building dchat-chain crate..." -ForegroundColor Cyan
$buildResult = cargo build -p dchat-chain 2>&1 | Out-String
if ($LASTEXITCODE -eq 0) {
    Write-Host "✓ Build successful" -ForegroundColor Green
} else {
    Write-Host "✗ Build failed:" -ForegroundColor Red
    Write-Host $buildResult
    exit 1
}

Write-Host ""
Write-Host "Step 2: Running dispute resolution tests..." -ForegroundColor Cyan
$testResult = cargo test -p dchat-chain --lib dispute_resolution 2>&1 | Out-String
Write-Host $testResult

if ($testResult -match "test result: ok") {
    Write-Host ""
    Write-Host "✓ All tests passed!" -ForegroundColor Green
    
    # Extract test summary
    if ($testResult -match "(\d+) passed") {
        Write-Host "  Total tests passed: $($Matches[1])" -ForegroundColor Green
    }
} else {
    Write-Host ""
    Write-Host "✗ Some tests failed" -ForegroundColor Red
}

Write-Host ""
Write-Host "Step 3: Verifying Ed25519 signature verification implementation..." -ForegroundColor Cyan
Write-Host "Checking for key features:" -ForegroundColor Yellow
Write-Host "  - Ed25519 imports: " -NoNewline
if (Select-String -Path "crates\dchat-chain\src\dispute_resolution.rs" -Pattern "use ed25519_dalek" -Quiet) {
    Write-Host "✓" -ForegroundColor Green
} else {
    Write-Host "✗" -ForegroundColor Red
}

Write-Host "  - VerifyingKey usage: " -NoNewline
if (Select-String -Path "crates\dchat-chain\src\dispute_resolution.rs" -Pattern "VerifyingKey::from_bytes" -Quiet) {
    Write-Host "✓" -ForegroundColor Green
} else {
    Write-Host "✗" -ForegroundColor Red
}

Write-Host "  - Signature verification: " -NoNewline
if (Select-String -Path "crates\dchat-chain\src\dispute_resolution.rs" -Pattern "verifying_key\.verify" -Quiet) {
    Write-Host "✓" -ForegroundColor Green
} else {
    Write-Host "✗" -ForegroundColor Red
}

Write-Host "  - Public key field in ForkEvidence: " -NoNewline
if (Select-String -Path "crates\dchat-chain\src\dispute_resolution.rs" -Pattern "accused_public_key" -Quiet) {
    Write-Host "✓" -ForegroundColor Green
} else {
    Write-Host "✗" -ForegroundColor Red
}

Write-Host ""
Write-Host "Implementation verification complete!" -ForegroundColor Green
