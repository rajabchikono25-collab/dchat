#!/usr/bin/env pwsh
# MinIO Quick Start Script for dchat Testnet
# This script starts MinIO and verifies it's working correctly

Write-Host "=== dchat MinIO Quick Start ===" -ForegroundColor Cyan
Write-Host ""

# Check if Docker/Podman is installed
Write-Host "Checking for container runtime..." -ForegroundColor Yellow
$containerRuntime = $null
if (Get-Command docker -ErrorAction SilentlyContinue) {
    $containerRuntime = "docker"
    Write-Host "✓ Found Docker" -ForegroundColor Green
} elseif (Get-Command podman -ErrorAction SilentlyContinue) {
    $containerRuntime = "podman"
    Write-Host "✓ Found Podman" -ForegroundColor Green
} else {
    Write-Host "✗ Neither Docker nor Podman found. Please install one of them." -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "Starting MinIO container..." -ForegroundColor Yellow

# Stop and remove existing MinIO container if it exists
& $containerRuntime stop dchat-minio 2>$null
& $containerRuntime rm dchat-minio 2>$null

# Start MinIO
& $containerRuntime run -d `
  --name dchat-minio `
  -p 9000:9000 `
  -p 9001:9001 `
  -e "MINIO_ROOT_USER=dchat_admin" `
  -e "MINIO_ROOT_PASSWORD=dchat_secure_password_change_me" `
  quay.io/minio/aistor/minio:latest server /data --console-address ":9001"

if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ Failed to start MinIO container" -ForegroundColor Red
    exit 1
}

Write-Host "✓ MinIO container started" -ForegroundColor Green
Write-Host ""

# Wait for MinIO to be ready
Write-Host "Waiting for MinIO to be ready..." -ForegroundColor Yellow
$maxRetries = 30
$retries = 0
$ready = $false

while ($retries -lt $maxRetries -and -not $ready) {
    Start-Sleep -Seconds 1
    try {
        $response = Invoke-WebRequest -Uri "http://localhost:9000/minio/health/live" -Method Get -TimeoutSec 2 -ErrorAction SilentlyContinue
        if ($response.StatusCode -eq 200) {
            $ready = $true
        }
    } catch {
        # Ignore errors and retry
    }
    $retries++
    Write-Host "." -NoNewline
}

Write-Host ""

if (-not $ready) {
    Write-Host "✗ MinIO failed to start within 30 seconds" -ForegroundColor Red
    Write-Host "Check logs with: $containerRuntime logs dchat-minio" -ForegroundColor Yellow
    exit 1
}

Write-Host "✓ MinIO is ready!" -ForegroundColor Green
Write-Host ""

# Create buckets using MinIO client
Write-Host "Creating buckets..." -ForegroundColor Yellow

$buckets = @(
    "dchat-testnet",
    "dchat-media",
    "dchat-attachments",
    "dchat-backups",
    "dchat-avatars",
    "dchat-channels"
)

foreach ($bucket in $buckets) {
    Write-Host "  Creating bucket: $bucket" -ForegroundColor Gray
    
    # Create bucket using MinIO client in a temporary container
    & $containerRuntime run --rm `
        --network="host" `
        quay.io/minio/aistor/mc:latest `
        /bin/sh -c "mc alias set dchat http://localhost:9000 dchat_admin dchat_secure_password_change_me && mc mb dchat/$bucket --ignore-existing" 2>$null
}

Write-Host "✓ Buckets created" -ForegroundColor Green
Write-Host ""

# Set public access for media and avatars buckets
Write-Host "Setting public read access for media buckets..." -ForegroundColor Yellow

& $containerRuntime run --rm `
    --network="host" `
    quay.io/minio/aistor/mc:latest `
    /bin/sh -c "mc alias set dchat http://localhost:9000 dchat_admin dchat_secure_password_change_me && mc anonymous set download dchat/dchat-media && mc anonymous set download dchat/dchat-avatars" 2>$null

Write-Host "✓ Public access configured" -ForegroundColor Green
Write-Host ""

# Display summary
Write-Host "=== MinIO is ready! ===" -ForegroundColor Cyan
Write-Host ""
Write-Host "MinIO Console: http://localhost:9001" -ForegroundColor Green
Write-Host "  Username: dchat_admin" -ForegroundColor Gray
Write-Host "  Password: dchat_secure_password_change_me" -ForegroundColor Gray
Write-Host ""
Write-Host "S3 API Endpoint: http://localhost:9000" -ForegroundColor Green
Write-Host ""
Write-Host "Buckets created:" -ForegroundColor Yellow
foreach ($bucket in $buckets) {
    Write-Host "  - $bucket" -ForegroundColor Gray
}
Write-Host ""
Write-Host "Test upload:" -ForegroundColor Yellow
Write-Host "  mc alias set dchat http://localhost:9000 dchat_admin dchat_secure_password_change_me" -ForegroundColor Gray
Write-Host "  mc cp test.txt dchat/dchat-testnet/" -ForegroundColor Gray
Write-Host ""
Write-Host "View logs:" -ForegroundColor Yellow
Write-Host "  $containerRuntime logs -f dchat-minio" -ForegroundColor Gray
Write-Host ""
Write-Host "Stop MinIO:" -ForegroundColor Yellow
Write-Host "  $containerRuntime stop dchat-minio" -ForegroundColor Gray
Write-Host ""
