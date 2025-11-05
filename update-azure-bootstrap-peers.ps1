#!/usr/bin/env pwsh
# Update Azure Validator Bootstrap Peers with Current Peer IDs

param(
    [switch]$WhatIf
)

$ErrorActionPreference = "Stop"

# Current peer IDs from running validators
$config = @"
[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/9090"]
bootstrap_peers = [
    "/ip4/54.233.203.82/tcp/9090/p2p/12D3KooWCjY9HZdzjdep5QsYHMH48RXPaWHb4bP17xb5KB4dYmuh",
    "/ip4/13.48.49.2/tcp/9090/p2p/12D3KooWKxoJic9ZjkiqXyTRyeTzcE6XfBapqnreZquZTA2oz4EA",
    "/ip4/18.191.118.167/tcp/9090/p2p/12D3KooWQqp6pq22h3LFhu73krUkGyJwwwZm499pr6GQNPMCDFit",
    "/ip4/18.142.96.209/tcp/9090/p2p/12D3KooWAdaNGLmZhq2ACGZJLueMRVSsbkAFdUW3j5L8d8JwHbpd",
    "/ip4/74.225.183.196/tcp/9090/p2p/12D3KooWSsDaL6yjZK766K85qSF7uvgh26h7d9iFbaG43LwsvJST",
    "/ip4/4.221.211.71/tcp/9090/p2p/12D3KooWAABVerV5qfVgEYMCbsqAwveLGv1PUX831EuMvdWp9vHq",
    "/ip4/4.161.34.228/tcp/9090/p2p/12D3KooWMsBEsDXMA8jfQEUzc3wFnLwpe7Byu3HUov2sbA4cnSWn"
]
max_connections = 100
connection_timeout_ms = 10000
enable_mdns = false
enable_upnp = false

[storage]
data_dir = "/opt/dchat/data"
max_message_cache_size = 10000
message_retention_days = 30
enable_backup = true
backup_interval_hours = 24
db_pool_size = 10
db_connection_timeout_secs = 30
db_idle_timeout_secs = 600
db_max_lifetime_secs = 1800
db_enable_wal = true

[crypto]
key_rotation_interval_hours = 168
max_messages_per_key = 10000
enable_post_quantum = false
noise_protocol_pattern = "Noise_XX_25519_ChaChaPoly_BLAKE2s"

[governance]
voting_period_hours = 168
minimum_stake_for_proposal = 10000
quorum_threshold = 0.51
enable_anonymous_voting = true

[relay]
enable_relay = false
max_relay_connections = 50
relay_reward_threshold = 1000
uptime_reporting_interval_minutes = 60
stake_amount = 10000
"@

$validators = @(
    @{
        Name = "India"
        IP = "74.225.183.196"
        Key = "Foundation-servers/Azure-India/uramami.pem"
        PeerID = "12D3KooWSsDaL6yjZK766K85qSF7uvgh26h7d9iFbaG43LwsvJST"
    },
    @{
        Name = "South Africa"
        IP = "4.221.211.71"
        Key = "Foundation-servers/Azure-SAfrica/anacreon.pem"
        PeerID = "12D3KooWAABVerV5qfVgEYMCbsqAwveLGv1PUX831EuMvdWp9vHq"
    },
    @{
        Name = "UAE"
        IP = "4.161.34.228"
        Key = "Foundation-servers/Azure_UAE/Randal_key.pem"
        PeerID = "12D3KooWMsBEsDXMA8jfQEUzc3wFnLwpe7Byu3HUov2sbA4cnSWn"
    }
)

Write-Host "`n=== Updating Azure Validator Bootstrap Peers ===`n" -ForegroundColor Cyan

foreach ($validator in $validators) {
    Write-Host "[$($validator.Name)] $($validator.IP)" -ForegroundColor Yellow
    
    # Test connectivity
    $canConnect = Test-NetConnection -ComputerName $validator.IP -Port 22 -InformationLevel Quiet -WarningAction SilentlyContinue
    
    if (-not $canConnect) {
        Write-Host "  ✗ Cannot connect to SSH port 22" -ForegroundColor Red
        continue
    }
    
    if ($WhatIf) {
        Write-Host "  [WHAT-IF] Would update config and restart service" -ForegroundColor Cyan
        continue
    }
    
    # Create temp file with config
    $tempFile = [System.IO.Path]::GetTempFileName()
    Set-Content -Path $tempFile -Value $config -NoNewline
    
    try {
        # Upload config
        Write-Host "  → Uploading config..." -NoNewline
        $scpResult = scp -i $validator.Key -o StrictHostKeyChecking=no -o ConnectTimeout=10 $tempFile "azureuser@$($validator.IP):/tmp/config.toml" 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host " ✗ Failed" -ForegroundColor Red
            Write-Host "    Error: $scpResult" -ForegroundColor Red
            continue
        }
        Write-Host " ✓" -ForegroundColor Green
        
        # Move to correct location and restart
        Write-Host "  → Installing config and restarting..." -NoNewline
        $sshResult = ssh -i $validator.Key -o StrictHostKeyChecking=no -o ConnectTimeout=10 "azureuser@$($validator.IP)" @"
sudo mv /tmp/config.toml /opt/dchat/config.toml && \
sudo systemctl restart dchat && \
echo OK
"@ 2>&1
        
        if ($sshResult -match "OK") {
            Write-Host " ✓" -ForegroundColor Green
            Write-Host "  ✓ $($validator.Name) updated successfully" -ForegroundColor Green
        } else {
            Write-Host " ✗ Failed" -ForegroundColor Red
            Write-Host "    Output: $sshResult" -ForegroundColor Red
        }
    }
    finally {
        Remove-Item -Path $tempFile -ErrorAction SilentlyContinue
    }
    
    Write-Host ""
}

Write-Host "`n=== Waiting 15 seconds for services to stabilize... ===`n" -ForegroundColor Cyan
Start-Sleep -Seconds 15

Write-Host "=== Checking Service Status ===`n" -ForegroundColor Cyan

foreach ($validator in $validators) {
    $canConnect = Test-NetConnection -ComputerName $validator.IP -Port 22 -InformationLevel Quiet -WarningAction SilentlyContinue
    if (-not $canConnect) { continue }
    
    Write-Host "[$($validator.Name)]" -ForegroundColor Yellow
    $status = ssh -i $validator.Key -o StrictHostKeyChecking=no -o ConnectTimeout=10 "azureuser@$($validator.IP)" "sudo systemctl is-active dchat 2>&1 && sudo journalctl -u dchat -n 5 --no-pager 2>&1 | tail -5"
    
    if ($status -match "active") {
        Write-Host "  Status: " -NoNewline
        Write-Host "RUNNING ✓" -ForegroundColor Green
        Write-Host "$status" -ForegroundColor Gray
    } else {
        Write-Host "  Status: " -NoNewline
        Write-Host "FAILED ✗" -ForegroundColor Red
        Write-Host "$status" -ForegroundColor Gray
    }
    Write-Host ""
}

Write-Host "`n=== Update Complete ===`n" -ForegroundColor Green
Write-Host "Next: Monitor logs for P2P connections with:" -ForegroundColor Cyan
Write-Host "  .\monitor-azure-logs.ps1" -ForegroundColor White
Write-Host "`nOr check individual validator:" -ForegroundColor Cyan
Write-Host "  ssh -i Foundation-servers/Azure-SAfrica/anacreon.pem azureuser@4.221.211.71 'sudo journalctl -u dchat -f'" -ForegroundColor White
