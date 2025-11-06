#!/usr/bin/env pwsh
# Deploy dchat binary to 3 Azure servers and establish P2P connections

$ErrorActionPreference = "Stop"

Write-Host "=== dchat 3-Server Deployment Script ===" -ForegroundColor Cyan
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Gray

# Server configuration
$servers = @(
    @{
        Name = "India (Mumbai)"
        IP = "74.225.183.196"
        Domain = "validator1-india.schikuno.top"
        User = "azureuser"
        KeyPath = "Foundation-servers/Azure-India/uramami.pem"
        P2PPort = 9090
        Role = "bootstrap"  # Primary bootstrap node
    },
    @{
        Name = "South Africa (Johannesburg)"
        IP = "4.221.211.71"
        Domain = "validator1-southafrica.schikuno.top"
        User = "azureuser"
        KeyPath = "Foundation-servers/Azure-SAfrica/anacreon.pem"
        P2PPort = 9090
        Role = "relay"
    },
    @{
        Name = "UAE (Dubai)"
        IP = "4.161.34.228"
        Domain = "validator1-uae.schikuno.top"
        User = "azureuser"
        KeyPath = "Foundation-servers/Azure_UAE/Randal_key.pem"
        P2PPort = 9090
        Role = "relay"
    }
)

# Binary path
$binaryPath = "target/release/dchat"
$wslBinaryPath = "/mnt/c/Users/USER/dchat/target/release/dchat"

# Check if binary exists
Write-Host "`n[1/6] Checking binary..." -ForegroundColor Yellow
if (-not (Test-Path $binaryPath)) {
    Write-Host "❌ Binary not found at $binaryPath" -ForegroundColor Red
    exit 1
}
Write-Host "✅ Binary found ($('{0:N2}' -f ((Get-Item $binaryPath).Length / 1MB)) MB)" -ForegroundColor Green

# Check SSH keys
Write-Host "`n[2/6] Checking SSH keys..." -ForegroundColor Yellow
foreach ($server in $servers) {
    if (-not (Test-Path $server.KeyPath)) {
        Write-Host "❌ SSH key not found: $($server.KeyPath)" -ForegroundColor Red
        Write-Host "   Please ensure SSH keys are in place" -ForegroundColor Yellow
        exit 1
    }
    Write-Host "✅ $($server.Name): Key found" -ForegroundColor Green
}

# Function to run SSH command
function Invoke-SSHCommand {
    param(
        [string]$Server,
        [string]$User,
        [string]$KeyPath,
        [string]$Command
    )
    
    $sshCmd = "ssh -o StrictHostKeyChecking=no -o ConnectTimeout=10 -i `"$KeyPath`" $User@$Server `"$Command`""
    Write-Host "   Running: $Command" -ForegroundColor Gray
    Invoke-Expression $sshCmd
}

# Upload binary to all servers
Write-Host "`n[3/6] Uploading binary to servers..." -ForegroundColor Yellow
foreach ($server in $servers) {
    Write-Host "`n   Uploading to $($server.Name) ($($server.IP))..." -ForegroundColor Cyan
    
    try {
        # Create directory on server
        Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
            -Command "sudo mkdir -p /opt/dchat && sudo chown $($server.User):$($server.User) /opt/dchat"
        
        # Upload binary using WSL (for proper permissions)
        $uploadCmd = "wsl bash -c `"scp -o StrictHostKeyChecking=no -i '$($server.KeyPath)' '$wslBinaryPath' $($server.User)@$($server.IP):/opt/dchat/dchat`""
        Invoke-Expression $uploadCmd
        
        # Make binary executable
        Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
            -Command "chmod +x /opt/dchat/dchat"
        
        Write-Host "   ✅ Upload complete" -ForegroundColor Green
    }
    catch {
        Write-Host "   ❌ Upload failed: $_" -ForegroundColor Red
        exit 1
    }
}

# Generate configuration for each server
Write-Host "`n[4/6] Generating configurations..." -ForegroundColor Yellow

# First, we need to get or generate peer IDs for each server
# For now, we'll create a basic config that allows bootstrapping

$bootstrapIP = $servers[0].IP
$bootstrapDomain = $servers[0].Domain

foreach ($server in $servers) {
    Write-Host "`n   Configuring $($server.Name)..." -ForegroundColor Cyan
    
    # Create a basic config file
    $config = @"
# dchat Configuration for $($server.Name)
# Generated: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')

[network]
listen_addr = "/ip4/0.0.0.0/tcp/$($server.P2PPort)"
public_addr = "/ip4/$($server.IP)/tcp/$($server.P2PPort)"

"@

    # Add bootstrap peers for non-bootstrap nodes
    if ($server.Role -ne "bootstrap") {
        $config += @"

# Bootstrap peers
[[network.bootstrap_peers]]
address = "/ip4/$bootstrapIP/tcp/9090"

"@
    }

    $config += @"

[relay]
enabled = true
max_connections = 100

[storage]
data_dir = "/opt/dchat/data"

[logging]
level = "info"
"@

    # Write config to temp file and upload
    $tempConfig = New-TemporaryFile
    Set-Content -Path $tempConfig.FullName -Value $config
    
    try {
        # Upload config
        $uploadCmd = "wsl bash -c `"scp -o StrictHostKeyChecking=no -i '$($server.KeyPath)' '$($tempConfig.FullName)' $($server.User)@$($server.IP):/opt/dchat/config.toml`""
        Invoke-Expression $uploadCmd
        
        Write-Host "   ✅ Configuration uploaded" -ForegroundColor Green
    }
    catch {
        Write-Host "   ❌ Config upload failed: $_" -ForegroundColor Red
    }
    finally {
        Remove-Item $tempConfig.FullName -Force
    }
}

# Create systemd service on each server
Write-Host "`n[5/6] Setting up systemd services..." -ForegroundColor Yellow

$serviceContent = @'
[Unit]
Description=dchat Relay Node
After=network.target

[Service]
Type=simple
User=azureuser
WorkingDirectory=/opt/dchat
ExecStart=/opt/dchat/dchat --config /opt/dchat/config.toml
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
'@

foreach ($server in $servers) {
    Write-Host "`n   Setting up service on $($server.Name)..." -ForegroundColor Cyan
    
    try {
        # Create temp service file
        $tempService = New-TemporaryFile
        Set-Content -Path $tempService.FullName -Value $serviceContent
        
        # Upload and install service
        $uploadCmd = "wsl bash -c `"scp -o StrictHostKeyChecking=no -i '$($server.KeyPath)' '$($tempService.FullName)' $($server.User)@$($server.IP):/tmp/dchat.service`""
        Invoke-Expression $uploadCmd
        
        Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
            -Command "sudo mv /tmp/dchat.service /etc/systemd/system/dchat.service && sudo systemctl daemon-reload"
        
        Write-Host "   ✅ Service installed" -ForegroundColor Green
        
        Remove-Item $tempService.FullName -Force
    }
    catch {
        Write-Host "   ❌ Service setup failed: $_" -ForegroundColor Red
    }
}

# Start services
Write-Host "`n[6/6] Starting services..." -ForegroundColor Yellow

# Start bootstrap node first
Write-Host "`n   Starting bootstrap node: $($servers[0].Name)..." -ForegroundColor Cyan
try {
    Invoke-SSHCommand -Server $servers[0].IP -User $servers[0].User -KeyPath $servers[0].KeyPath `
        -Command "sudo systemctl enable dchat && sudo systemctl restart dchat"
    Write-Host "   ✅ Bootstrap node started" -ForegroundColor Green
    Write-Host "   Waiting 10 seconds for bootstrap node to initialize..." -ForegroundColor Gray
    Start-Sleep -Seconds 10
}
catch {
    Write-Host "   ❌ Failed to start: $_" -ForegroundColor Red
}

# Start other nodes
for ($i = 1; $i -lt $servers.Count; $i++) {
    $server = $servers[$i]
    Write-Host "`n   Starting $($server.Name)..." -ForegroundColor Cyan
    try {
        Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
            -Command "sudo systemctl enable dchat && sudo systemctl restart dchat"
        Write-Host "   ✅ Node started" -ForegroundColor Green
        Start-Sleep -Seconds 5
    }
    catch {
        Write-Host "   ❌ Failed to start: $_" -ForegroundColor Red
    }
}

# Verification
Write-Host "`n=== Verification ===" -ForegroundColor Cyan
Write-Host "Waiting 15 seconds for nodes to establish connections..." -ForegroundColor Gray
Start-Sleep -Seconds 15

foreach ($server in $servers) {
    Write-Host "`n--- $($server.Name) Status ---" -ForegroundColor Yellow
    
    try {
        # Check service status
        Write-Host "Service Status:" -ForegroundColor Gray
        Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
            -Command "sudo systemctl status dchat --no-pager | head -n 10"
        
        Write-Host "`nRecent Logs:" -ForegroundColor Gray
        Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
            -Command "sudo journalctl -u dchat -n 20 --no-pager"
    }
    catch {
        Write-Host "❌ Failed to get status: $_" -ForegroundColor Red
    }
}

Write-Host "`n=== Deployment Complete ===" -ForegroundColor Green
Write-Host @"

Next Steps:
1. Monitor logs: ssh <user>@<server> 'sudo journalctl -u dchat -f'
2. Check connections: Look for 'peer connected' or 'handshake' messages
3. Verify network: All 3 nodes should discover each other within 1-2 minutes

Server Addresses:
- India:        $($servers[0].IP) ($($servers[0].Domain))
- South Africa: $($servers[1].IP) ($($servers[1].Domain))
- UAE:          $($servers[2].IP) ($($servers[2].Domain))

To check connectivity:
  ./check-3-server-connectivity.ps1
"@ -ForegroundColor Cyan
