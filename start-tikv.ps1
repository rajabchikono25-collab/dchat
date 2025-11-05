#!/usr/bin/env pwsh
# Quick start script for TiKV (Windows/PowerShell)
# Starts 3 PD nodes + 3 TiKV nodes for local development

param(
    [switch]$Stop,
    [switch]$Clean,
    [switch]$Status
)

$ErrorActionPreference = "Stop"

# Colors for output
function Write-ColorOutput($ForegroundColor) {
    $fc = $host.UI.RawUI.ForegroundColor
    $host.UI.RawUI.ForegroundColor = $ForegroundColor
    if ($args) {
        Write-Output $args
    }
    $host.UI.RawUI.ForegroundColor = $fc
}

Write-ColorOutput Green "=================================="
Write-ColorOutput Green "dchat TiKV Quick Start"
Write-ColorOutput Green "=================================="

# Check status
if ($Status) {
    Write-ColorOutput Cyan "`nChecking TiKV cluster status..."
    
    docker ps --filter "name=dchat-pd" --filter "name=dchat-tikv" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
    
    Write-ColorOutput Cyan "`nPD Health Check:"
    try {
        $response = Invoke-WebRequest -Uri "http://localhost:2379/health" -TimeoutSec 2 -UseBasicParsing
        Write-ColorOutput Green "PD1 (port 2379): $($response.Content)"
    } catch {
        Write-ColorOutput Yellow "PD1 (port 2379): Not responding"
    }
    
    Write-ColorOutput Cyan "`nTiKV Status Check:"
    try {
        $response = Invoke-WebRequest -Uri "http://localhost:20180/status" -TimeoutSec 2 -UseBasicParsing
        Write-ColorOutput Green "TiKV1 (port 20180): Healthy"
    } catch {
        Write-ColorOutput Yellow "TiKV1 (port 20180): Not responding"
    }
    
    exit 0
}

# Stop TiKV
if ($Stop) {
    Write-ColorOutput Yellow "`nStopping TiKV cluster..."
    docker-compose -f docker-compose-testnet.yml stop pd1 pd2 pd3 tikv1 tikv2 tikv3
    Write-ColorOutput Green "TiKV cluster stopped successfully"
    exit 0
}

# Clean TiKV data
if ($Clean) {
    Write-ColorOutput Red "`n⚠️  WARNING: This will DELETE all TiKV data!"
    $confirm = Read-Host "Type 'yes' to continue"
    
    if ($confirm -ne "yes") {
        Write-ColorOutput Yellow "Clean cancelled"
        exit 0
    }
    
    Write-ColorOutput Yellow "`nStopping TiKV cluster..."
    docker-compose -f docker-compose-testnet.yml stop pd1 pd2 pd3 tikv1 tikv2 tikv3
    
    Write-ColorOutput Yellow "Removing containers..."
    docker-compose -f docker-compose-testnet.yml rm -f pd1 pd2 pd3 tikv1 tikv2 tikv3
    
    Write-ColorOutput Yellow "Removing volumes..."
    docker volume rm dchat_pd1_data dchat_pd2_data dchat_pd3_data dchat_tikv1_data dchat_tikv2_data dchat_tikv3_data -f 2>$null
    
    Write-ColorOutput Green "TiKV data cleaned successfully"
    exit 0
}

# Start TiKV cluster
Write-ColorOutput Cyan "`nStarting TiKV cluster (3 PD + 3 TiKV nodes)..."

# Check if Docker is running
try {
    docker ps | Out-Null
} catch {
    Write-ColorOutput Red "❌ Error: Docker is not running. Please start Docker Desktop."
    exit 1
}

# Check if docker-compose-testnet.yml exists
if (-not (Test-Path "docker-compose-testnet.yml")) {
    Write-ColorOutput Red "❌ Error: docker-compose-testnet.yml not found"
    exit 1
}

# Start PD cluster
Write-ColorOutput Cyan "`n[1/3] Starting PD (Placement Driver) cluster..."
docker-compose -f docker-compose-testnet.yml up -d pd1 pd2 pd3

Write-ColorOutput Cyan "Waiting for PD cluster to be ready..."
$maxAttempts = 30
$attempt = 0
$pdReady = $false

while ($attempt -lt $maxAttempts -and -not $pdReady) {
    Start-Sleep -Seconds 2
    try {
        $response = Invoke-WebRequest -Uri "http://localhost:2379/health" -TimeoutSec 2 -UseBasicParsing
        if ($response.StatusCode -eq 200) {
            $pdReady = $true
            Write-ColorOutput Green "✓ PD cluster is ready"
        }
    } catch {
        $attempt++
        Write-Host "." -NoNewline
    }
}

if (-not $pdReady) {
    Write-ColorOutput Red "`n❌ Error: PD cluster failed to start within 60 seconds"
    Write-ColorOutput Yellow "Check logs with: docker-compose -f docker-compose-testnet.yml logs pd1"
    exit 1
}

# Start TiKV nodes
Write-ColorOutput Cyan "`n[2/3] Starting TiKV storage nodes..."
docker-compose -f docker-compose-testnet.yml up -d tikv1 tikv2 tikv3

Write-ColorOutput Cyan "Waiting for TiKV nodes to register..."
Start-Sleep -Seconds 10

$tikvReady = $false
$attempt = 0
while ($attempt -lt $maxAttempts -and -not $tikvReady) {
    Start-Sleep -Seconds 2
    try {
        $response = Invoke-WebRequest -Uri "http://localhost:20180/status" -TimeoutSec 2 -UseBasicParsing
        if ($response.StatusCode -eq 200) {
            $tikvReady = $true
            Write-ColorOutput Green "✓ TiKV nodes are ready"
        }
    } catch {
        $attempt++
        Write-Host "." -NoNewline
    }
}

if (-not $tikvReady) {
    Write-ColorOutput Yellow "`n⚠️  Warning: TiKV nodes may still be initializing"
    Write-ColorOutput Yellow "Check logs with: docker-compose -f docker-compose-testnet.yml logs tikv1"
}

# Display connection info
Write-ColorOutput Green "`n=================================="
Write-ColorOutput Green "✓ TiKV Cluster Started Successfully"
Write-ColorOutput Green "=================================="

Write-ColorOutput Cyan "`nPD Endpoints (for client connections):"
Write-Output "  - http://localhost:2379  (PD1)"
Write-Output "  - http://localhost:2381  (PD2)"
Write-Output "  - http://localhost:2383  (PD3)"

Write-ColorOutput Cyan "`nTiKV Nodes:"
Write-Output "  - tikv1:20160  (Status: http://localhost:20180)"
Write-Output "  - tikv2:20160  (Status: http://localhost:20181)"
Write-Output "  - tikv3:20160  (Status: http://localhost:20182)"

Write-ColorOutput Cyan "`nRust Client Configuration (testnet-config.toml):"
Write-ColorOutput White @"
[storage.tikv]
pd_endpoints = [
    "http://localhost:2379",
    "http://localhost:2381",
    "http://localhost:2383"
]
"@

Write-ColorOutput Cyan "`nUseful Commands:"
Write-Output "  Status:    .\start-tikv.ps1 -Status"
Write-Output "  Stop:      .\start-tikv.ps1 -Stop"
Write-Output "  Clean:     .\start-tikv.ps1 -Clean"
Write-Output "  Logs:      docker-compose -f docker-compose-testnet.yml logs -f tikv1"
Write-Output "  PD Health: Invoke-WebRequest http://localhost:2379/health"

Write-ColorOutput Green "`n✓ Ready for development!"
