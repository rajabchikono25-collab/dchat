#!/usr/bin/env pwsh
# Mainnet deployment automation script
# Deploys validator, relays, and storage clusters to all 7 servers

param(
    [switch]$SkipBuild,
    [switch]$ValidatorsOnly,
    [switch]$RelaysOnly,
    [switch]$StorageOnly,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

# Server configuration
$servers = @(
    @{Region="ohio"; Host="validator1-ohio.schikuno.top"; IP=$null; Provider="AWS"; SSHKey="Foundation-servers/AWS-Ohio/ohio-key.pem"},
    @{Region="singapore"; Host="validator1-singapore.schikuno.top"; IP=$null; Provider="AWS"; SSHKey="Foundation-servers/AWS-Singapore/singapore-key.pem"},
    @{Region="stockholm"; Host="validator1-stockholm.schikuno.top"; IP=$null; Provider="AWS"; SSHKey="Foundation-servers/AWS-Stockholm/stockholm-key.pem"},
    @{Region="saopaulo"; Host="validator1-saopaulo.schikuno.top"; IP=$null; Provider="AWS"; SSHKey="Foundation-servers/AWS-SaoPaulo/saopaulo-key.pem"},
    @{Region="india"; Host="validator1-india.schikuno.top"; IP="74.225.183.196"; Provider="Azure"; SSHKey="Foundation-servers/Azure-India/uramami.pem"},
    @{Region="southafrica"; Host="validator1-southafrica.schikuno.top"; IP="4.221.211.71"; Provider="Azure"; SSHKey="Foundation-servers/Azure-SAfrica/anacreon.pem"},
    @{Region="uae"; Host="validator1-uae.schikuno.top"; IP="4.161.34.228"; Provider="Azure"; SSHKey="Foundation-servers/Azure_UAE/Randal_key.pem"}
)

Write-Host "🚀 dchat Mainnet Deployment" -ForegroundColor Cyan
Write-Host "=========================" -ForegroundColor Cyan
Write-Host ""

# Step 1: Build binaries
if (-not $SkipBuild) {
    Write-Host "📦 Building dchat binaries in WSL..." -ForegroundColor Yellow
    wsl bash -c "cd /mnt/c/Users/USER/dchat && cargo build --release --bin dchat"
    
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Build failed!"
        exit 1
    }
    
    Write-Host "✓ Build complete" -ForegroundColor Green
} else {
    Write-Host "⏭ Skipping build (using existing binary)" -ForegroundColor Yellow
}

# Step 2: Generate mainnet configurations
Write-Host ""
Write-Host "📝 Generating mainnet configurations..." -ForegroundColor Yellow

$configDir = "mainnet-configs"
if (-not (Test-Path $configDir)) {
    New-Item -ItemType Directory -Path $configDir | Out-Null
}

wsl bash -c "cd /mnt/c/Users/USER/dchat && cargo run --bin dchat -- deploy plan --network mainnet --domain schikuno.top --output $configDir"

Write-Host "✓ Configurations generated" -ForegroundColor Green

# Step 3: Deploy to each server
$deployed = @()
$failed = @()

foreach ($server in $servers) {
    Write-Host ""
    Write-Host "================================================" -ForegroundColor Cyan
    Write-Host "Deploying to $($server.Region) ($($server.Host))" -ForegroundColor Cyan
    Write-Host "================================================" -ForegroundColor Cyan
    
    $sshTarget = "azureuser@$($server.Host)"
    if ($server.IP) {
        $sshTarget = "azureuser@$($server.IP)"
    }
    
    $sshKey = $server.SSHKey
    
    try {
        # Test SSH connectivity
        Write-Host "Testing SSH connection..." -ForegroundColor Yellow
        wsl bash -c "ssh -i '$sshKey' -o ConnectTimeout=10 -o StrictHostKeyChecking=no $sshTarget 'echo Connected'"
        
        if ($LASTEXITCODE -ne 0) {
            throw "SSH connection failed"
        }
        
        Write-Host "✓ SSH connection successful" -ForegroundColor Green
        
        if (-not $DryRun) {
            # Copy binary
            Write-Host "Copying dchat binary..." -ForegroundColor Yellow
            wsl bash -c "scp -i '$sshKey' -o StrictHostKeyChecking=no /mnt/c/Users/USER/dchat/target/release/dchat $sshTarget:/tmp/dchat"
            
            # Copy configuration
            Write-Host "Copying configuration..." -ForegroundColor Yellow
            $configFile = "$configDir/config-mainnet-$($server.Region).toml"
            wsl bash -c "scp -i '$sshKey' -o StrictHostKeyChecking=no /mnt/c/Users/USER/dchat/$configFile $sshTarget:/tmp/config-mainnet.toml"
            
            # Install binary and config
            Write-Host "Installing on server..." -ForegroundColor Yellow
            $installScript = @"
sudo mkdir -p /opt/dchat/bin /opt/dchat/config /opt/dchat/keys /opt/dchat/data
sudo mv /tmp/dchat /opt/dchat/bin/dchat
sudo chmod +x /opt/dchat/bin/dchat
sudo mv /tmp/config-mainnet.toml /opt/dchat/config/config-mainnet.toml
sudo chown -R azureuser:azureuser /opt/dchat
"@
            
            wsl bash -c "ssh -i '$sshKey' -o StrictHostKeyChecking=no $sshTarget '$installScript'"
            
            # Deploy validator
            if (-not $RelaysOnly -and -not $StorageOnly) {
                Write-Host "Deploying validator service..." -ForegroundColor Yellow
                $validatorService = @"
[Unit]
Description=dchat Validator Node
After=network.target

[Service]
Type=simple
User=azureuser
WorkingDirectory=/opt/dchat
ExecStart=/opt/dchat/bin/dchat validator --key /opt/dchat/keys/validator.key --chain-rpc https://validator1-ohio.schikuno.top --stake 10000 --producer --config /opt/dchat/config/config-mainnet.toml
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"@
                
                $validatorService | wsl bash -c "cat | ssh -i '$sshKey' -o StrictHostKeyChecking=no $sshTarget 'sudo tee /etc/systemd/system/dchat-validator.service > /dev/null'"
                
                wsl bash -c "ssh -i '$sshKey' -o StrictHostKeyChecking=no $sshTarget 'sudo systemctl daemon-reload && sudo systemctl enable dchat-validator'"
                Write-Host "✓ Validator service installed" -ForegroundColor Green
            }
            
            # Deploy relays
            if (-not $ValidatorsOnly -and -not $StorageOnly) {
                Write-Host "Deploying relay services..." -ForegroundColor Yellow
                
                # Relay 1
                $relay1Service = @"
[Unit]
Description=dchat Relay Node 1
After=network.target

[Service]
Type=simple
User=azureuser
WorkingDirectory=/opt/dchat
ExecStart=/opt/dchat/bin/dchat relay --listen 0.0.0.0:7071 --stake 1000 --config /opt/dchat/config/config-mainnet.toml --health-addr 0.0.0.0:8081 --metrics-addr 0.0.0.0:9091
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"@
                
                $relay1Service | wsl bash -c "cat | ssh -i '$sshKey' -o StrictHostKeyChecking=no $sshTarget 'sudo tee /etc/systemd/system/dchat-relay1.service > /dev/null'"
                
                # Relay 2
                $relay2Service = @"
[Unit]
Description=dchat Relay Node 2
After=network.target

[Service]
Type=simple
User=azureuser
WorkingDirectory=/opt/dchat
ExecStart=/opt/dchat/bin/dchat relay --listen 0.0.0.0:7072 --stake 1000 --config /opt/dchat/config/config-mainnet.toml --health-addr 0.0.0.0:8082 --metrics-addr 0.0.0.0:9092
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"@
                
                $relay2Service | wsl bash -c "cat | ssh -i '$sshKey' -o StrictHostKeyChecking=no $sshTarget 'sudo tee /etc/systemd/system/dchat-relay2.service > /dev/null'"
                
                wsl bash -c "ssh -i '$sshKey' -o StrictHostKeyChecking=no $sshTarget 'sudo systemctl daemon-reload && sudo systemctl enable dchat-relay1 dchat-relay2'"
                Write-Host "✓ Relay services installed" -ForegroundColor Green
            }
            
            Write-Host "✓ Deployment to $($server.Region) complete!" -ForegroundColor Green
            $deployed += $server.Region
        } else {
            Write-Host "[DRY RUN] Would deploy to $($server.Region)" -ForegroundColor Yellow
            $deployed += $server.Region
        }
        
    } catch {
        Write-Host "❌ Deployment to $($server.Region) failed: $_" -ForegroundColor Red
        $failed += $server.Region
    }
}

# Summary
Write-Host ""
Write-Host "================================================" -ForegroundColor Cyan
Write-Host "Deployment Summary" -ForegroundColor Cyan
Write-Host "================================================" -ForegroundColor Cyan
Write-Host "✓ Deployed: $($deployed -join ', ')" -ForegroundColor Green

if ($failed.Count -gt 0) {
    Write-Host "❌ Failed: $($failed -join ', ')" -ForegroundColor Red
    exit 1
} else {
    Write-Host ""
    Write-Host "🎉 All servers deployed successfully!" -ForegroundColor Green
    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor Yellow
    Write-Host "1. Generate validator keys on each server" -ForegroundColor White
    Write-Host "2. Start storage clusters (Redis, MinIO, TiKV)" -ForegroundColor White
    Write-Host "3. Start validators: ./start-mainnet-validators.ps1" -ForegroundColor White
    Write-Host "4. Start relays: ./start-mainnet-relays.ps1" -ForegroundColor White
    Write-Host "5. Monitor: ./monitor-mainnet.ps1" -ForegroundColor White
}
