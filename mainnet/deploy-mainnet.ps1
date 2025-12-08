# dchat Mainnet Deployment Script
# Deploys 3 Validators, 3 Relays, 1 User Client
# Date: 2025-12-08

param(
    [switch]$BuildOnly,
    [switch]$DeployOnly,
    [switch]$ValidatorsOnly,
    [switch]$RelaysOnly,
    [switch]$UserOnly,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

# ============================================================================
# SERVER CONFIGURATION
# ============================================================================

$Servers = @{
    # Validators (Azure)
    "validator-india" = @{
        IP = "74.225.183.196"
        User = "azureuser"
        Key = "uramami.pem"
        DNS = "validator.india.schikuno.top"
        Role = "validator"
        Config = "config-validator-india.toml"
        Identity = "validator1-india.json"
        Password = "indiamain123"
    }
    "validator-southafrica" = @{
        IP = "4.221.211.71"
        User = "azureuser"
        Key = "anacreon.pem"
        DNS = "validator.southafrica.schikuno.top"
        Role = "validator"
        Config = "config-validator-southafrica.toml"
        Identity = "validator2-southafrica.json"
        Password = "SAmain123"
    }
    "validator-uae" = @{
        IP = "4.161.34.228"
        User = "azureuser"
        Key = "Randal_key.pem"
        DNS = "validator.uae.schikuno.top"
        Role = "validator"
        Config = "config-validator-uae.toml"
        Identity = "validator3-uae.json"
        Password = "UAEmain123"
    }
    # Relays (AWS)
    "relay-ohio" = @{
        IP = "13.58.182.122"
        User = "ubuntu"
        Key = "gecko.pem"
        DNS = "relay.ohio.schikuno.top"
        Role = "relay"
        Config = "config-relay-ohio.toml"
        Identity = "relay1-ohio.json"
        Password = "ohiorelay123"
    }
    "relay-stockholm" = @{
        IP = "16.16.212.80"
        User = "ubuntu"
        Key = "restock.pem"
        DNS = "relay.stockholm.schikuno.top"
        Role = "relay"
        Config = "config-relay-stockholm.toml"
        Identity = "relay2-stockholm.json"
        Password = "stockholmrelay123"
    }
    "relay-saopaulo" = @{
        IP = "18.230.144.17"
        User = "ubuntu"
        Key = "pablo.pem"
        DNS = "relay.saopaulo.schikuno.top"
        Role = "relay"
        Config = "config-relay-saopaulo.toml"
        Identity = "relay3-saopaulo.json"
        Password = "saopaulorelay123"
    }
    # User Client (AWS)
    "user-singapore" = @{
        IP = "13.251.102.178"
        User = "ubuntu"
        Key = "craig.pem"
        DNS = "user.singapore.schikuno.top"
        Role = "user"
        Config = "config-user-singapore.toml"
        Identity = "user-singapore.json"
        Password = "singaporeuser123"
    }
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$KeyDir = Join-Path $ScriptDir "..\foundation servers"
$MainnetDir = $ScriptDir
$BinaryPath = Join-Path $ScriptDir "..\target\release\dchat"

# ============================================================================
# FUNCTIONS
# ============================================================================

function Write-Header {
    param([string]$Message)
    Write-Host ""
    Write-Host "=" * 70 -ForegroundColor Cyan
    Write-Host "  $Message" -ForegroundColor Cyan
    Write-Host "=" * 70 -ForegroundColor Cyan
    Write-Host ""
}

function Write-Step {
    param([string]$Message)
    Write-Host "[*] $Message" -ForegroundColor Yellow
}

function Write-Success {
    param([string]$Message)
    Write-Host "[✓] $Message" -ForegroundColor Green
}

function Write-Error {
    param([string]$Message)
    Write-Host "[✗] $Message" -ForegroundColor Red
}

function Test-SSHConnection {
    param(
        [string]$Server,
        [hashtable]$Config
    )
    
    $keyPath = Join-Path $KeyDir $Config.Key
    try {
        $result = ssh -i $keyPath -o ConnectTimeout=10 -o StrictHostKeyChecking=no "$($Config.User)@$($Config.IP)" "echo 'connected'" 2>&1
        return $result -eq "connected"
    } catch {
        return $false
    }
}

function Deploy-ToServer {
    param(
        [string]$ServerName,
        [hashtable]$Config
    )
    
    Write-Step "Deploying to $ServerName ($($Config.DNS))..."
    
    $keyPath = Join-Path $KeyDir $Config.Key
    $configPath = Join-Path $MainnetDir $Config.Config
    $identityPath = Join-Path $MainnetDir $Config.Identity
    $sshTarget = "$($Config.User)@$($Config.IP)"
    
    # Create directories on remote
    Write-Step "Creating directories..."
    ssh -i $keyPath -o StrictHostKeyChecking=no $sshTarget "sudo mkdir -p /opt/dchat/{bin,config,keys,data,logs} && sudo chown -R $($Config.User):$($Config.User) /opt/dchat"
    
    # Copy binary
    Write-Step "Copying dchat binary..."
    scp -i $keyPath -o StrictHostKeyChecking=no "$BinaryPath" "${sshTarget}:/opt/dchat/bin/dchat"
    ssh -i $keyPath -o StrictHostKeyChecking=no $sshTarget "chmod +x /opt/dchat/bin/dchat"
    
    # Copy config
    Write-Step "Copying configuration..."
    scp -i $keyPath -o StrictHostKeyChecking=no $configPath "${sshTarget}:/opt/dchat/config/config.toml"
    
    # Copy identity
    Write-Step "Copying identity key..."
    scp -i $keyPath -o StrictHostKeyChecking=no $identityPath "${sshTarget}:/opt/dchat/keys/identity.json"
    
    Write-Success "Deployed to $ServerName"
}

function Install-SystemdService {
    param(
        [string]$ServerName,
        [hashtable]$Config
    )
    
    Write-Step "Installing systemd service on $ServerName..."
    
    $keyPath = Join-Path $KeyDir $Config.Key
    $sshTarget = "$($Config.User)@$($Config.IP)"
    
    $serviceContent = @"
[Unit]
Description=dchat $($Config.Role) node - $ServerName
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=$($Config.User)
Group=$($Config.User)
WorkingDirectory=/opt/dchat
Environment=DCHAT_KEY_PASSWORD=$($Config.Password)
Environment=RUST_LOG=info
Environment=RUST_BACKTRACE=1
"@

    if ($Config.Role -eq "validator") {
        $serviceContent += @"

ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml validator --key /opt/dchat/keys/identity.json --chain-rpc http://127.0.0.1:26657 --producer --stake 10000
"@
    } elseif ($Config.Role -eq "relay") {
        $serviceContent += @"

ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml relay --listen 0.0.0.0:7070 --stake 1000
"@
    } else {
        $serviceContent += @"

ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml user --non-interactive --username dchat-user
"@
    }

    $serviceContent += @"

Restart=always
RestartSec=10
LimitNOFILE=65535

StandardOutput=journal
StandardError=journal
SyslogIdentifier=dchat

[Install]
WantedBy=multi-user.target
"@
    
    # Write service file
    $tempFile = New-TemporaryFile
    $serviceContent | Out-File -FilePath $tempFile.FullName -Encoding utf8 -NoNewline
    
    scp -i $keyPath -o StrictHostKeyChecking=no $tempFile.FullName "${sshTarget}:/tmp/dchat.service"
    ssh -i $keyPath -o StrictHostKeyChecking=no $sshTarget "sudo mv /tmp/dchat.service /etc/systemd/system/dchat.service && sudo systemctl daemon-reload && sudo systemctl enable dchat"
    
    Remove-Item $tempFile.FullName -Force
    
    Write-Success "Systemd service installed on $ServerName"
}

function Start-DchatService {
    param(
        [string]$ServerName,
        [hashtable]$Config
    )
    
    Write-Step "Starting dchat on $ServerName..."
    
    $keyPath = Join-Path $KeyDir $Config.Key
    $sshTarget = "$($Config.User)@$($Config.IP)"
    
    ssh -i $keyPath -o StrictHostKeyChecking=no $sshTarget "sudo systemctl start dchat"
    Start-Sleep -Seconds 2
    
    $status = ssh -i $keyPath -o StrictHostKeyChecking=no $sshTarget "sudo systemctl is-active dchat"
    if ($status -eq "active") {
        Write-Success "dchat started on $ServerName"
        return $true
    } else {
        Write-Error "dchat failed to start on $ServerName"
        ssh -i $keyPath -o StrictHostKeyChecking=no $sshTarget "sudo journalctl -u dchat -n 20 --no-pager"
        return $false
    }
}

function Get-ServiceStatus {
    param(
        [string]$ServerName,
        [hashtable]$Config
    )
    
    $keyPath = Join-Path $KeyDir $Config.Key
    $sshTarget = "$($Config.User)@$($Config.IP)"
    
    $status = ssh -i $keyPath -o StrictHostKeyChecking=no $sshTarget "sudo systemctl is-active dchat 2>/dev/null || echo 'not-running'"
    return $status.Trim()
}

function Open-Firewall {
    param(
        [string]$ServerName,
        [hashtable]$Config
    )
    
    Write-Step "Opening firewall ports on $ServerName..."
    
    $keyPath = Join-Path $KeyDir $Config.Key
    $sshTarget = "$($Config.User)@$($Config.IP)"
    
    # Open ports for dchat
    $commands = @"
sudo ufw allow 7070/tcp comment 'dchat p2p'
sudo ufw allow 8080/tcp comment 'dchat health'
sudo ufw allow 9090/tcp comment 'dchat metrics'
sudo ufw allow 26657/tcp comment 'tendermint rpc'
sudo ufw --force enable
sudo ufw status
"@
    
    ssh -i $keyPath -o StrictHostKeyChecking=no $sshTarget $commands
    
    Write-Success "Firewall configured on $ServerName"
}

# ============================================================================
# MAIN DEPLOYMENT
# ============================================================================

Write-Header "dchat Mainnet Deployment"
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host "Network: dchat-foundation-mainnet"
Write-Host ""

# Build binary for Linux
if (-not $SkipBuild -and -not $DeployOnly) {
    Write-Header "Building dchat for Linux (x86_64)"
    
    Push-Location (Join-Path $ScriptDir "..")
    
    # Cross-compile for Linux
    Write-Step "Cross-compiling for linux-gnu target..."
    $env:CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER = "x86_64-linux-gnu-gcc"
    cargo build --release --target x86_64-unknown-linux-gnu --bin dchat
    
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Build failed!"
        Pop-Location
        exit 1
    }
    
    # Update binary path
    $script:BinaryPath = Join-Path $ScriptDir "..\target\x86_64-unknown-linux-gnu\release\dchat"
    
    Pop-Location
    Write-Success "Build completed"
}

if ($BuildOnly) {
    Write-Success "Build only mode - exiting"
    exit 0
}

# Test connectivity
Write-Header "Testing SSH Connectivity"

foreach ($server in $Servers.GetEnumerator()) {
    Write-Step "Testing $($server.Key)..."
    if (Test-SSHConnection -Server $server.Key -Config $server.Value) {
        Write-Success "$($server.Key) - Connected"
    } else {
        Write-Error "$($server.Key) - Failed to connect!"
    }
}

# Deploy to servers
Write-Header "Deploying to Servers"

$deployList = @()
if ($ValidatorsOnly) {
    $deployList = $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "validator" }
} elseif ($RelaysOnly) {
    $deployList = $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "relay" }
} elseif ($UserOnly) {
    $deployList = $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "user" }
} else {
    $deployList = $Servers.GetEnumerator()
}

foreach ($server in $deployList) {
    Deploy-ToServer -ServerName $server.Key -Config $server.Value
    Open-Firewall -ServerName $server.Key -Config $server.Value
    Install-SystemdService -ServerName $server.Key -Config $server.Value
}

# Start services in order: Validators first, then relays, then user
Write-Header "Starting Services"

# 1. Start validators (with delay between each)
Write-Step "Starting validators..."
$validators = $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "validator" } | Sort-Object Key

foreach ($validator in $validators) {
    if (-not $RelaysOnly -and -not $UserOnly) {
        Start-DchatService -ServerName $validator.Key -Config $validator.Value
        Write-Host "Waiting 10 seconds for validator to initialize..."
        Start-Sleep -Seconds 10
    }
}

# 2. Start relays
Write-Step "Starting relays..."
$relays = $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "relay" }

foreach ($relay in $relays) {
    if (-not $ValidatorsOnly -and -not $UserOnly) {
        Start-DchatService -ServerName $relay.Key -Config $relay.Value
        Start-Sleep -Seconds 5
    }
}

# 3. Start user client
Write-Step "Starting user client..."
$users = $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "user" }

foreach ($user in $users) {
    if (-not $ValidatorsOnly -and -not $RelaysOnly) {
        Start-DchatService -ServerName $user.Key -Config $user.Value
    }
}

# Final status check
Write-Header "Deployment Status"

foreach ($server in $Servers.GetEnumerator()) {
    $status = Get-ServiceStatus -ServerName $server.Key -Config $server.Value
    $statusColor = if ($status -eq "active") { "Green" } else { "Red" }
    Write-Host "$($server.Key.PadRight(25)) : $($server.Value.Role.PadRight(10)) : " -NoNewline
    Write-Host $status -ForegroundColor $statusColor
}

Write-Header "Deployment Complete!"
Write-Host ""
Write-Host "Next steps:"
Write-Host "  1. Check logs: ssh -i <key> <user>@<ip> 'sudo journalctl -u dchat -f'"
Write-Host "  2. Monitor health: curl http://<dns>:8080/health"
Write-Host "  3. View metrics: curl http://<dns>:9090/metrics"
Write-Host ""
