#!/usr/bin/env pwsh
# Check connectivity and handshake status between 3 Azure servers

$ErrorActionPreference = "Stop"

Write-Host "=== dchat 3-Server Connectivity Check ===" -ForegroundColor Cyan
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Gray

# Server configuration
$servers = @(
    @{
        Name = "India (Mumbai)"
        IP = "74.225.183.196"
        User = "azureuser"
        KeyPath = "Foundation-servers/Azure-India/uramami.pem"
    },
    @{
        Name = "South Africa (Johannesburg)"
        IP = "4.221.211.71"
        User = "azureuser"
        KeyPath = "Foundation-servers/Azure-SAfrica/anacreon.pem"
    },
    @{
        Name = "UAE (Dubai)"
        IP = "4.161.34.228"
        User = "azureuser"
        KeyPath = "Foundation-servers/Azure_UAE/Randal_key.pem"
    }
)

# Function to run SSH command
function Invoke-SSHCommand {
    param(
        [string]$Server,
        [string]$User,
        [string]$KeyPath,
        [string]$Command,
        [switch]$Silent
    )
    
    $sshCmd = "ssh -o StrictHostKeyChecking=no -o ConnectTimeout=10 -i `"$KeyPath`" $User@$Server `"$Command`" 2>&1"
    
    if (-not $Silent) {
        Write-Host "   Running: $Command" -ForegroundColor Gray
    }
    
    try {
        $result = Invoke-Expression $sshCmd
        return $result
    }
    catch {
        return $null
    }
}

# Check each server
$results = @()

foreach ($server in $servers) {
    Write-Host "`n=== $($server.Name) ===" -ForegroundColor Yellow
    
    $serverStatus = @{
        Name = $server.Name
        IP = $server.IP
        ServiceRunning = $false
        PeerCount = 0
        Connections = @()
        Logs = @()
        Errors = @()
    }
    
    # Check if service is running
    Write-Host "[1] Checking service status..." -ForegroundColor Cyan
    $serviceStatus = Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
        -Command "sudo systemctl is-active dchat" -Silent
    
    if ($serviceStatus -match "active") {
        Write-Host "✅ Service is running" -ForegroundColor Green
        $serverStatus.ServiceRunning = $true
    }
    else {
        Write-Host "❌ Service is not running: $serviceStatus" -ForegroundColor Red
        $serverStatus.Errors += "Service not active"
    }
    
    # Check process
    Write-Host "[2] Checking dchat process..." -ForegroundColor Cyan
    $processCheck = Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
        -Command "ps aux | grep '[d]chat'" -Silent
    
    if ($processCheck) {
        Write-Host "✅ Process found" -ForegroundColor Green
        Write-Host "   $processCheck" -ForegroundColor Gray
    }
    else {
        Write-Host "❌ Process not found" -ForegroundColor Red
        $serverStatus.Errors += "Process not running"
    }
    
    # Check port listening
    Write-Host "[3] Checking port 9090..." -ForegroundColor Cyan
    $portCheck = Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
        -Command "sudo netstat -tlnp | grep 9090 || sudo ss -tlnp | grep 9090" -Silent
    
    if ($portCheck) {
        Write-Host "✅ Port 9090 is listening" -ForegroundColor Green
        Write-Host "   $portCheck" -ForegroundColor Gray
    }
    else {
        Write-Host "⚠️  Port 9090 not detected (may still be starting)" -ForegroundColor Yellow
        $serverStatus.Errors += "Port 9090 not listening"
    }
    
    # Check recent logs for connections
    Write-Host "[4] Checking connection logs..." -ForegroundColor Cyan
    $logs = Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
        -Command "sudo journalctl -u dchat -n 50 --no-pager" -Silent
    
    if ($logs) {
        # Look for peer connections
        $peerLines = $logs -split "`n" | Where-Object { $_ -match "peer|connect|handshake" }
        
        if ($peerLines) {
            Write-Host "✅ Found connection activity:" -ForegroundColor Green
            $peerLines | ForEach-Object {
                Write-Host "   $_" -ForegroundColor Gray
                $serverStatus.Logs += $_
            }
        }
        else {
            Write-Host "⚠️  No peer connection logs found yet" -ForegroundColor Yellow
        }
        
        # Check for errors
        $errorLines = $logs -split "`n" | Where-Object { $_ -match "error|failed|panic" }
        if ($errorLines) {
            Write-Host "⚠️  Found errors:" -ForegroundColor Yellow
            $errorLines | Select-Object -First 5 | ForEach-Object {
                Write-Host "   $_" -ForegroundColor Red
                $serverStatus.Errors += $_
            }
        }
    }
    
    # Check last 10 lines of logs
    Write-Host "[5] Recent log output:" -ForegroundColor Cyan
    $recentLogs = Invoke-SSHCommand -Server $server.IP -User $server.User -KeyPath $server.KeyPath `
        -Command "sudo journalctl -u dchat -n 10 --no-pager" -Silent
    
    if ($recentLogs) {
        $recentLogs -split "`n" | ForEach-Object {
            Write-Host "   $_" -ForegroundColor Gray
        }
    }
    
    $results += $serverStatus
}

# Summary
Write-Host "`n=== Summary ===" -ForegroundColor Cyan

$runningCount = ($results | Where-Object { $_.ServiceRunning }).Count
Write-Host "Servers Running: $runningCount/3" -ForegroundColor $(if ($runningCount -eq 3) { "Green" } else { "Yellow" })

# Check if all servers can reach each other
Write-Host "`n=== Network Connectivity Test ===" -ForegroundColor Cyan

foreach ($sourceServer in $servers) {
    Write-Host "`nFrom $($sourceServer.Name):" -ForegroundColor Yellow
    
    foreach ($targetServer in $servers) {
        if ($sourceServer.IP -ne $targetServer.IP) {
            Write-Host "  Testing connection to $($targetServer.Name) ($($targetServer.IP))..." -ForegroundColor Gray
            
            # Test port connectivity
            $testCmd = "timeout 5 bash -c 'cat < /dev/null > /dev/tcp/$($targetServer.IP)/9090' 2>&1 && echo 'success' || echo 'failed'"
            $testResult = Invoke-SSHCommand -Server $sourceServer.IP -User $sourceServer.User `
                -KeyPath $sourceServer.KeyPath -Command $testCmd -Silent
            
            if ($testResult -match "success") {
                Write-Host "    ✅ Port 9090 accessible" -ForegroundColor Green
            }
            else {
                Write-Host "    ❌ Port 9090 not accessible" -ForegroundColor Red
            }
        }
    }
}

Write-Host "`n=== Recommendations ===" -ForegroundColor Cyan

if ($runningCount -lt 3) {
    Write-Host "• Start all services: sudo systemctl start dchat" -ForegroundColor Yellow
}

$errorCount = ($results | Where-Object { $_.Errors.Count -gt 0 }).Count
if ($errorCount -gt 0) {
    Write-Host "• Check logs for errors: sudo journalctl -u dchat -f" -ForegroundColor Yellow
}

Write-Host "• Monitor live logs: ssh <user>@<server> 'sudo journalctl -u dchat -f'" -ForegroundColor Cyan
Write-Host "• Check P2P connections: Look for 'peer connected' or 'handshake completed' messages" -ForegroundColor Cyan
Write-Host "• Verify firewall: Ensure port 9090 is open in Azure NSG" -ForegroundColor Cyan

Write-Host "`nDone!" -ForegroundColor Green
