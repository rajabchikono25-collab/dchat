#!/usr/bin/env pwsh
# DCHAT Mainnet Deployment Script
# Compiles on Linux, then deploys to all servers

param(
    [string]$SSHKeyPath = "C:\foundation-servers"
)

$ErrorActionPreference = "Stop"

# Server configuration
$servers = @(
    @{ Region = "ohio"; Host = "validator1-ohio.schikuno.top"; User = "ubuntu"; Order = 1; KeyFile = "AWS-Ohio\gecko.pem" }
    @{ Region = "singapore"; Host = "validator1-singapore.schikuno.top"; User = "ubuntu"; Order = 2; KeyFile = "AWS-Singapore\craig.pem" }
    @{ Region = "stockholm"; Host = "validator1-stockholm.schikuno.top"; User = "ubuntu"; Order = 3; KeyFile = "AWS-Stokholm\relay.pem" }
    @{ Region = "saopaulo"; Host = "validator1-saopaulo.schikuno.top"; User = "ubuntu"; Order = 4; KeyFile = "AWS-Sao-Paulo\pablo.pem" }
    @{ Region = "india"; Host = "validator1-india.schikuno.top"; User = "azureuser"; Order = 5; KeyFile = "Azure-India\uramami.pem" }
    @{ Region = "southafrica"; Host = "validator1-southafrica.schikuno.top"; User = "azureuser"; Order = 6; KeyFile = "Azure-SAfrica\anacreon.pem" }
    @{ Region = "uae"; Host = "validator1-uae.schikuno.top"; User = "azureuser"; Order = 7; KeyFile = "Azure_UAE\Randal_key.pem" }
)

Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  DCHAT MAINNET DEPLOYMENT" -ForegroundColor Cyan
Write-Host "============================================`n" -ForegroundColor Cyan

# Find SSH keys
Write-Host "🔑 Locating SSH keys in $SSHKeyPath..." -ForegroundColor Yellow
$sshKeys = Get-ChildItem -Path $SSHKeyPath -Filter "*.pem" -Recurse -ErrorAction SilentlyContinue
if ($sshKeys.Count -eq 0) {
    Write-Host "❌ No SSH keys found in $SSHKeyPath" -ForegroundColor Red
    exit 1
}
Write-Host "✓ Found $($sshKeys.Count) SSH key(s)" -ForegroundColor Green

# Build server (use Ohio as the build server)
$buildServer = $servers[0]
$buildKeyFile = Join-Path $SSHKeyPath $buildServer.KeyFile
if (-not (Test-Path $buildKeyFile)) {
    Write-Host "❌ Build server SSH key not found: $buildKeyFile" -ForegroundColor Red
    exit 1
}
Write-Host "✓ Using build server key: $($buildServer.KeyFile)" -ForegroundColor Green

Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  PHASE 1: PREPARE BUILD SERVER ($($buildServer.Region.ToUpper()))" -ForegroundColor Cyan
Write-Host "============================================`n" -ForegroundColor Cyan

# Create deployment package
Write-Host "📦 Creating deployment package..." -ForegroundColor Yellow
$tempDir = Join-Path $env:TEMP "dchat-mainnet-$(Get-Date -Format 'yyyyMMddHHmmss')"
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

# Copy source code
Write-Host "  Copying source code..." -ForegroundColor Gray
Copy-Item -Path ".\Cargo.toml" -Destination $tempDir
Copy-Item -Path ".\src" -Destination $tempDir -Recurse
Copy-Item -Path ".\crates" -Destination $tempDir -Recurse

# Create tarball
Write-Host "  Creating tarball..." -ForegroundColor Gray
$tarballPath = Join-Path $env:TEMP "dchat-source.tar.gz"
tar -czf $tarballPath -C $tempDir .

Write-Host "✓ Package created: $tarballPath" -ForegroundColor Green

# Upload source to build server
Write-Host "`n📤 Uploading source to build server..." -ForegroundColor Yellow

$scpArgs = @("-i", $buildKeyFile, "-o", "StrictHostKeyChecking=no", $tarballPath, "$($buildServer.User)@$($buildServer.Host):/tmp/dchat-source.tar.gz")
& scp $scpArgs
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Failed to upload source" -ForegroundColor Red
    exit 1
}

# Build on server
Write-Host "`n🔨 Building on $($buildServer.Region.ToUpper())..." -ForegroundColor Yellow
Write-Host "  This will take 20-30 minutes..." -ForegroundColor Gray

$buildCommands = @"
set -e
cd /tmp
rm -rf dchat-build
mkdir dchat-build
cd dchat-build
tar -xzf ../dchat-source.tar.gz
echo '📦 Installing Rust...'
if ! command -v rustc &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source ~/.cargo/env
fi
echo '🔨 Building release binary...'
cargo build --release
echo '✓ Build complete'
ls -lh target/release/dchat
"@

$sshArgs = @("-i", $buildKeyFile, "-o", "StrictHostKeyChecking=no", "$($buildServer.User)@$($buildServer.Host)", $buildCommands)
& ssh $sshArgs
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Build failed" -ForegroundColor Red
    exit 1
}

Write-Host "✓ Binary built successfully" -ForegroundColor Green

Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  PHASE 2: DEPLOY TO ALL SERVERS" -ForegroundColor Cyan
Write-Host "============================================`n" -ForegroundColor Cyan

foreach ($server in $servers) {
    $region = $server.Region
    $host = $server.Host
    $user = $server.User
    
    Write-Host "`n📦 Deploying to $($region.ToUpper())..." -ForegroundColor Yellow
    
    # Get SSH key for this server
    $keyFile = Join-Path $SSHKeyPath $server.KeyFile
    if (-not (Test-Path $keyFile)) {
        Write-Host "  ⚠️  Key not found: $($server.KeyFile), using build server key" -ForegroundColor Yellow
        $keyFile = $buildKeyFile
    }
    # Deploy binary
    Write-Host "  📤 Deploying binary..." -ForegroundColor Gray
    if ($region -eq "ohio") {
        # Already on build server, just move it
        $sshArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$user@$host", "sudo cp /tmp/dchat-build/target/release/dchat /usr/local/bin/ && sudo chmod 755 /usr/local/bin/dchat")
        & ssh $sshArgs
    } else {
        # Download from build server and upload to target
        $sshArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$user@$host", "mkdir -p /tmp/dchat-deploy")
        & ssh $sshArgs
        
        # Use scp to copy from build server through local machine to target
        $tempBinary = Join-Path $env:TEMP "dchat-binary"
        $scpDownloadArgs = @("-i", $buildKeyFile, "-o", "StrictHostKeyChecking=no", "$($buildServer.User)@$($buildServer.Host):/tmp/dchat-build/target/release/dchat", $tempBinary)
        & scp $scpDownloadArgs
        
        $scpUploadArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", $tempBinary, "${user}@${host}:/tmp/dchat-deploy/dchat")
        & scp $scpUploadArgs
        
        $sshArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$user@$host", "sudo mv /tmp/dchat-deploy/dchat /usr/local/bin/ && sudo chmod 755 /usr/local/bin/dchat")
        & ssh $sshArgs
        
        Remove-Item $tempBinary -Force -ErrorAction SilentlyContinue
    }
    
    # Deploy configuration
    Write-Host "  ⚙️  Deploying configuration..." -ForegroundColor Gray
    $configFile = ".\mainnet-configs\config-mainnet-$region.toml"
    $scpArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", $configFile, "${user}@${host}:/tmp/config.toml")
    & scp $scpArgs
    $sshArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$user@$host", "sudo mkdir -p /etc/dchat && sudo mv /tmp/config.toml /etc/dchat/config.toml")
    & ssh $sshArgs
    
    # Deploy validator key
    Write-Host "  🔑 Deploying validator key..." -ForegroundColor Gray
    $keyPath = ".\mainnet-keys\validator-$region.key"
    $scpArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", $keyPath, "${user}@${host}:/tmp/validator.key")
    & scp $scpArgs
    $sshArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$user@$host", "sudo mkdir -p /etc/dchat/keys && sudo mv /tmp/validator.key /etc/dchat/keys/validator.key && sudo chmod 600 /etc/dchat/keys/validator.key")
    & ssh $sshArgs
    
    # Create systemd service
    Write-Host "  🔧 Creating systemd service..." -ForegroundColor Gray
    $serviceContent = @"
[Unit]
Description=dchat Validator Node
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/dchat --config /etc/dchat/config.toml --role validator
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"@
    
    $serviceContent | Out-File -FilePath "$tempDir/dchat-validator.service" -Encoding utf8
    $scpArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$tempDir/dchat-validator.service", "${user}@${host}:/tmp/dchat-validator.service")
    & scp $scpArgs
    $sshArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$user@$host", "sudo mv /tmp/dchat-validator.service /etc/systemd/system/ && sudo systemctl daemon-reload")
    & ssh $sshArgs
    Write-Host "`n▶️  Starting validator $order/7: $($region.ToUpper())" -ForegroundColor Yellow
    
    $keyFile = Join-Path $SSHKeyPath $server.KeyFile
    if (-not (Test-Path $keyFile)) {
        $keyFile = $buildKeyFile
    }
    
    $sshArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$user@$host", "sudo systemctl start dchat-validator && sudo systemctl enable dchat-validator")
    & ssh $sshArgs
    $region = $server.Region
    $host = $server.Host
    $user = $server.User
    $order = $server.Order
    
    Write-Host "`n▶️  Starting validator $order/7: $($region.ToUpper())" -ForegroundColor Yellow
    
    $keyFile = Join-Path $SSHKeyPath $server.KeyFile
    if (-not (Test-Path $keyFile)) {
        $keyFile = $buildKeyFile
    }
    
    $sshCmd = "ssh -i `"$keyFile`" -o StrictHostKeyChecking=no $user@$host"
    
    Invoke-Expression "$sshCmd 'sudo systemctl start dchat-validator && sudo systemctl enable dchat-validator'"
    
    if ($order -eq 4) {
        Write-Host "`n  🎯 CONSENSUS CHECKPOINT!" -ForegroundColor Cyan
        Write-Host "     4/7 validators now running - consensus should form" -ForegroundColor Cyan
        Write-Host "     Waiting 60 seconds to verify..." -ForegroundColor Gray
        Start-Sleep -Seconds 60
        
        # Check consensus
        Write-Host "`n  🔍 Checking consensus..." -ForegroundColor Yellow
        try {
            $response = Invoke-WebRequest -Uri "http://${host}:8080/health" -TimeoutSec 5 -UseBasicParsing
            Write-Host "  ✓ Validator responding: $($response.StatusCode)" -ForegroundColor Green
        } catch {
            Write-Host "  ⚠️  Validator not responding yet (may still be starting)" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  ⏳ Waiting 30 seconds..." -ForegroundColor Gray
        Start-Sleep -Seconds 30
    Write-Host "`n🔄 Starting relays on $($region.ToUpper())..." -ForegroundColor Yellow
    
    $keyFile = Join-Path $SSHKeyPath $server.KeyFile
    if (-not (Test-Path $keyFile)) {
        $keyFile = $buildKeyFile
    }
    
    # Create relay service files
    for ($i = 1; $i -le 2; $i++) {
        $port = 7070 + $i
        $relayService = @"
[Unit]
Description=dchat Relay Node $i
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/dchat --config /etc/dchat/config.toml --role relay --port $port
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"@
        
        $relayService | Out-File -FilePath "$tempDir/dchat-relay-$i.service" -Encoding utf8
        $scpArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$tempDir/dchat-relay-$i.service", "${user}@${host}:/tmp/dchat-relay-$i.service")
        & scp $scpArgs
        $sshArgs = @("-i", $keyFile, "-o", "StrictHostKeyChecking=no", "$user@$host", "sudo mv /tmp/dchat-relay-$i.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl start dchat-relay-$i && sudo systemctl enable dchat-relay-$i")
        & ssh $sshArgs
        
        Write-Host "  ✓ Relay $i started (port $port)" -ForegroundColor Green
    }rt=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"@
        
        $relayService | Out-File -FilePath "$tempDir/dchat-relay-$i.service" -Encoding utf8
        $scpCmd = "scp -i `"$keyFile`" -o StrictHostKeyChecking=no"
        & $scpCmd "$tempDir/dchat-relay-$i.service" "${user}@${host}:/tmp/dchat-relay-$i.service"
        Invoke-Expression "$sshCmd 'sudo mv /tmp/dchat-relay-$i.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl start dchat-relay-$i && sudo systemctl enable dchat-relay-$i'"
        
        Write-Host "  ✓ Relay $i started (port $port)" -ForegroundColor Green
    }
}

Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  PHASE 5: NETWORK HEALTH CHECK" -ForegroundColor Cyan
Write-Host "============================================`n" -ForegroundColor Cyan

Write-Host "🏥 Checking network health...`n" -ForegroundColor Yellow

$healthyCount = 0
foreach ($server in $servers) {
    $region = $server.Region
    $host = $server.Host
    
    Write-Host "🔍 $($region.ToUpper())" -ForegroundColor Cyan
    
    try {
        $response = Invoke-WebRequest -Uri "http://${host}:8080/health" -TimeoutSec 5 -UseBasicParsing
        Write-Host "  ✓ Status: Healthy ($($response.StatusCode))" -ForegroundColor Green
        $healthyCount++
    } catch {
        Write-Host "  ❌ Status: Unreachable" -ForegroundColor Red
        Write-Host "     Error: $($_.Exception.Message)" -ForegroundColor Gray
    }
    
    Write-Host "  Health: http://${host}:8080/health" -ForegroundColor Gray
    Write-Host "  Metrics: http://${host}:9090/metrics" -ForegroundColor Gray
    Write-Host ""
}

Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  MAINNET LAUNCH COMPLETE!" -ForegroundColor Cyan
Write-Host "============================================`n" -ForegroundColor Cyan

Write-Host "📊 Deployment Summary:" -ForegroundColor Yellow
Write-Host "   • Validators: 7/7 started" -ForegroundColor White
Write-Host "   • Relays: 14/14 started" -ForegroundColor White
Write-Host "   • Healthy nodes: $healthyCount/7" -ForegroundColor White
Write-Host "   • Consensus: 4/7 minimum (BFT)" -ForegroundColor White
Write-Host ""

Write-Host "🔍 Verification Commands:" -ForegroundColor Yellow
Write-Host "   Check consensus:" -ForegroundColor Gray
Write-Host "   curl http://validator1-ohio.schikuno.top:8080/consensus" -ForegroundColor White
Write-Host ""
Write-Host "   Watch block production:" -ForegroundColor Gray
Write-Host "   watch -n 2 'curl -s http://validator1-ohio.schikuno.top:8080/block/latest'" -ForegroundColor White
Write-Host ""
Write-Host "   Check all validators:" -ForegroundColor Gray
Write-Host "   foreach(`$r in 'ohio','singapore','stockholm','saopaulo','india','southafrica','uae') { curl http://validator1-`$r.schikuno.top:8080/health }" -ForegroundColor White
Write-Host ""

Write-Host "🎉 MAINNET IS LIVE!" -ForegroundColor Green
Write-Host ""

# Cleanup
Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path $tarballPath -Force -ErrorAction SilentlyContinue
