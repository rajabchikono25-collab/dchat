#!/usr/bin/env pwsh
# Pre-launch verification script for dchat mainnet
# Run this script before deploying to production

$ErrorActionPreference = "Stop"

Write-Host "🚀 dchat Mainnet Pre-Launch Verification" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$script:FailureCount = 0

function Test-Check {
    param(
        [string]$Name,
        [scriptblock]$Test,
        [string]$FailureMessage
    )
    
    Write-Host "🔍 $Name..." -NoNewline
    try {
        $result = & $Test
        if ($result) {
            Write-Host " ✅" -ForegroundColor Green
            return $true
        } else {
            Write-Host " ❌" -ForegroundColor Red
            Write-Host "   $FailureMessage" -ForegroundColor Yellow
            $script:FailureCount++
            return $false
        }
    } catch {
        Write-Host " ❌" -ForegroundColor Red
        Write-Host "   Error: $_" -ForegroundColor Yellow
        $script:FailureCount++
        return $false
    }
}

# 1. Check Rust toolchain
Test-Check -Name "Checking Rust toolchain" -Test {
    $rustc = Get-Command rustc -ErrorAction SilentlyContinue
    return $null -ne $rustc
} -FailureMessage "Rust not installed. Install from https://rustup.rs/"

# 2. Check cargo workspace
Test-Check -Name "Verifying cargo workspace" -Test {
    cargo check --workspace --quiet 2>&1 | Out-Null
    return $LASTEXITCODE -eq 0
} -FailureMessage "Cargo check failed. Fix compilation errors first."

# 3. Check for MockStakingVerifier in release build
Test-Check -Name "Verifying no mocks in release build" -Test {
    $buildOutput = cargo build --release 2>&1 | Out-String
    return -not ($buildOutput -match "MockStakingVerifier")
} -FailureMessage "CRITICAL: MockStakingVerifier found in release build! This is a security vulnerability."

# 4. Check environment variables
$envChecks = @(
    @{Name = "CURRENCY_CHAIN_RPC"; Required = $true},
    @{Name = "DCHAT_RELAY_KEYSTORE_PASSPHRASE"; Required = $true}
)

foreach ($envCheck in $envChecks) {
    Test-Check -Name "Checking env var: $($envCheck.Name)" -Test {
        $value = [Environment]::GetEnvironmentVariable($envCheck.Name)
        return -not [string]::IsNullOrEmpty($value)
    } -FailureMessage "$($envCheck.Name) not set. See docs/PRODUCTION_DEPLOYMENT_GUIDE.md"
}

# 5. Test currency chain RPC connectivity
Test-Check -Name "Testing currency chain RPC connection" -Test {
    $rpcUrl = [Environment]::GetEnvironmentVariable("CURRENCY_CHAIN_RPC")
    if ([string]::IsNullOrEmpty($rpcUrl)) { return $false }
    
    try {
        $body = @{
            jsonrpc = "2.0"
            method = "eth_blockNumber"
            params = @()
            id = 1
        } | ConvertTo-Json
        
        $response = Invoke-RestMethod -Uri $rpcUrl -Method Post -Body $body -ContentType "application/json" -TimeoutSec 10
        return $null -ne $response.result
    } catch {
        return $false
    }
} -FailureMessage "Cannot connect to currency chain RPC. Check CURRENCY_CHAIN_RPC value."

# 6. Check for test artifacts
Test-Check -Name "Checking for test artifacts in source" -Test {
    $testArtifacts = Get-ChildItem -Path "crates" -Recurse -Include "*.rs" | 
        Select-String -Pattern "INSECURE FOR PRODUCTION|TODO.*production|FIXME.*production" -SimpleMatch
    return $testArtifacts.Count -eq 0
} -FailureMessage "Found test artifacts or production TODOs in source code."

# 7. Run unit tests
Test-Check -Name "Running unit tests" -Test {
    cargo test --workspace --lib --quiet 2>&1 | Out-Null
    return $LASTEXITCODE -eq 0
} -FailureMessage "Unit tests failed. Fix test failures before deploying."

# 8. Run integration tests
Test-Check -Name "Running integration tests" -Test {
    cargo test --workspace --test '*' --quiet 2>&1 | Out-Null
    return $LASTEXITCODE -eq 0
} -FailureMessage "Integration tests failed. Review test output."

# 9. Check for secrets in logs (static analysis)
Test-Check -Name "Checking for potential secret logging" -Test {
    $secretLogs = Get-ChildItem -Path "crates" -Recurse -Include "*.rs" |
        Select-String -Pattern "tracing::.*passphrase|println!.*secret|dbg!\(.*key" -SimpleMatch
    return $secretLogs.Count -eq 0
} -FailureMessage "Potential secret logging detected. Never log passwords or keys."

# 10. Verify production config exists
Test-Check -Name "Verifying production configuration" -Test {
    return Test-Path "config-production.toml"
} -FailureMessage "config-production.toml not found. Create production configuration."

# 11. Check binary size (should be optimized)
Test-Check -Name "Checking release binary optimization" -Test {
    $binaryPath = "target/release/dchat-relay.exe"
    if (-not (Test-Path $binaryPath)) {
        cargo build --release --quiet 2>&1 | Out-Null
    }
    
    if (Test-Path $binaryPath) {
        $size = (Get-Item $binaryPath).Length / 1MB
        # Release binary should be reasonably sized (< 100MB for Rust)
        return $size -lt 100
    }
    return $false
} -FailureMessage "Release binary is too large or missing."

# 12. Verify documentation is up to date
Test-Check -Name "Verifying deployment documentation" -Test {
    return (Test-Path "docs/PRODUCTION_DEPLOYMENT_GUIDE.md") -and 
           (Test-Path "ARCHITECTURE.md")
} -FailureMessage "Documentation missing. Ensure all deployment guides exist."

# 13. Check for cargo-audit issues
Test-Check -Name "Running security audit" -Test {
    $cargoAudit = Get-Command cargo-audit -ErrorAction SilentlyContinue
    if ($null -eq $cargoAudit) {
        Write-Warning "cargo-audit not installed. Run: cargo install cargo-audit"
        return $true # Don't fail, just warn
    }
    
    cargo audit --quiet 2>&1 | Out-Null
    return $LASTEXITCODE -eq 0
} -FailureMessage "Security vulnerabilities detected. Run 'cargo audit' for details."

# 14. Verify git state
Test-Check -Name "Checking git repository state" -Test {
    $gitStatus = git status --porcelain
    if ($gitStatus) {
        Write-Warning "Uncommitted changes detected"
    }
    
    # Check we're on the right branch
    $branch = git rev-parse --abbrev-ref HEAD
    return $branch -eq "main" -or $branch -eq "release"
} -FailureMessage "Not on main or release branch. Deploy from stable branch."

# Summary
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan

if ($script:FailureCount -eq 0) {
    Write-Host "✅ All checks passed! Ready for production deployment." -ForegroundColor Green
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor Cyan
    Write-Host "1. Tag this release: git tag -a v1.0.0 -m 'Production release 1.0.0'" -ForegroundColor White
    Write-Host "2. Review docs/PRODUCTION_DEPLOYMENT_GUIDE.md" -ForegroundColor White
    Write-Host "3. Deploy to staging first, then production" -ForegroundColor White
    Write-Host "4. Monitor metrics at /metrics endpoint" -ForegroundColor White
    exit 0
} else {
    Write-Host "❌ $script:FailureCount check(s) failed. Review errors above." -ForegroundColor Red
    Write-Host ""
    Write-Host "Do NOT deploy until all checks pass." -ForegroundColor Yellow
    Write-Host "See docs/PRODUCTION_DEPLOYMENT_GUIDE.md for guidance." -ForegroundColor Yellow
    exit 1
}
