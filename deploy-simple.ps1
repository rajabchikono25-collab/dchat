#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Simple build and deploy script for 3 Azure servers

.DESCRIPTION
    Builds dchat in WSL and uploads to the 3 Azure servers
#>

$ErrorActionPreference = "Stop"

# Server configuration
$Servers = @(
    @{
        Name = "India"
        IP = "74.225.183.196"
        User = "azureuser"
        KeyFile = "Foundation-servers/Azure-India/uramami.pem"
    },
    @{
        Name = "South Africa"
        IP = "4.221.211.71"
        User = "azureuser"
        KeyFile = "Foundation-servers/Azure-SAfrica/anacreon.pem"
    },
    @{
        Name = "UAE"
        IP = "4.161.34.228"
        User = "azureuser"
        KeyFile = "Foundation-servers/Azure_UAE/Randal_key.pem"
    }
)

Write-Host "`n==> Building dchat binary in WSL..." -ForegroundColor Cyan

# Build command
$buildCmd = @"
source ~/.cargo/env && \
cd /mnt/c/Users/USER/dchat && \
echo 'Starting build...' && \
cargo build --release --bin dchat && \
echo 'Build complete!' && \
ls -lh target/release/dchat
"@

Write-Host "Running: cargo build --release --bin dchat"
$buildStart = Get-Date

try {
    wsl bash -c $buildCmd
    if ($LASTEXITCODE -ne 0) {
        throw "Build failed with exit code $LASTEXITCODE"
    }
} catch {
    Write-Host "ERROR: Build failed - $_" -ForegroundColor Red
    exit 1
}

$buildDuration = (Get-Date) - $buildStart
Write-Host "✓ Build completed in $([math]::Round($buildDuration.TotalSeconds, 1)) seconds" -ForegroundColor Green

# Verify binary exists
if (-not (Test-Path "target\release\dchat")) {
    Write-Host "ERROR: Binary not found at target/release/dchat" -ForegroundColor Red
    exit 1
}

$binarySize = (Get-Item "target\release\dchat").Length
$binarySizeMB = [math]::Round($binarySize / 1MB, 2)
Write-Host "✓ Binary size: ${binarySizeMB} MB" -ForegroundColor Green

# Deploy to each server
Write-Host "`n==> Deploying to servers..." -ForegroundColor Cyan

foreach ($server in $Servers) {
    Write-Host "`n--- Deploying to $($server.Name) ($($server.IP)) ---" -ForegroundColor Yellow
    
    # Check if key file exists
    if (-not (Test-Path $server.KeyFile)) {
        Write-Host "ERROR: Key file not found: $($server.KeyFile)" -ForegroundColor Red
        continue
    }
    
    $keyPath = Resolve-Path $server.KeyFile
    $localBinary = Resolve-Path "target\release\dchat"
    
    # Test connectivity
    Write-Host "Testing connectivity..."
    $testCmd = "ssh -i '$keyPath' -o StrictHostKeyChecking=no -o ConnectTimeout=5 $($server.User)@$($server.IP) 'echo Connected'"
    try {
        $testResult = wsl bash -c $testCmd 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host "ERROR: Cannot connect to $($server.Name)" -ForegroundColor Red
            continue
        }
        Write-Host "✓ Connected" -ForegroundColor Green
    } catch {
        Write-Host "ERROR: Connection failed - $_" -ForegroundColor Red
        continue
    }
    
    # Copy binary
    Write-Host "Uploading binary..."
    $scpCmd = "scp -i '$keyPath' -o StrictHostKeyChecking=no '$localBinary' $($server.User)@$($server.IP):/tmp/dchat"
    try {
        wsl bash -c $scpCmd 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "SCP failed"
        }
        Write-Host "✓ Binary uploaded" -ForegroundColor Green
    } catch {
        Write-Host "ERROR: Upload failed - $_" -ForegroundColor Red
        continue
    }
    
    # Install and restart
    Write-Host "Installing and restarting service..."
    $deployCmd = @"
sudo mv /tmp/dchat /opt/dchat/dchat && \
sudo chmod +x /opt/dchat/dchat && \
sudo chown root:root /opt/dchat/dchat && \
sudo systemctl restart dchat && \
echo 'Service restarted'
"@
    
    $sshDeployCmd = "ssh -i '$keyPath' -o StrictHostKeyChecking=no $($server.User)@$($server.IP) '$deployCmd'"
    try {
        $deployResult = wsl bash -c $sshDeployCmd 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "Deploy command failed"
        }
        Write-Host "✓ Service restarted" -ForegroundColor Green
        Write-Host "✓ Deployment to $($server.Name) complete!" -ForegroundColor Green
    } catch {
        Write-Host "ERROR: Deployment failed - $_" -ForegroundColor Red
        Write-Host $deployResult
    }
}

Write-Host "`n==> Checking server status..." -ForegroundColor Cyan
Start-Sleep -Seconds 5

foreach ($server in $Servers) {
    Write-Host "`n$($server.Name):" -NoNewline
    
    # Check if service is running
    $keyPath = Resolve-Path $server.KeyFile
    $statusCmd = "ssh -i '$keyPath' -o StrictHostKeyChecking=no $($server.User)@$($server.IP) 'sudo systemctl is-active dchat'"
    
    try {
        $status = wsl bash -c $statusCmd 2>&1
        if ($status -match "active") {
            Write-Host " ✓ Running" -ForegroundColor Green
        } else {
            Write-Host " ✗ Not running ($status)" -ForegroundColor Red
        }
    } catch {
        Write-Host " ? Status unknown" -ForegroundColor Yellow
    }
}

Write-Host "`n==> Deployment complete!" -ForegroundColor Green
Write-Host "Next steps:"
Write-Host "  1. Check logs: wsl ssh -i <keyfile> azureuser@<ip> 'sudo journalctl -u dchat -f'"
Write-Host "  2. Verify connectivity between servers"
Write-Host ""
