#!/usr/bin/env pwsh
# Start all mainnet relays

param(
    [switch]$DryRun,
    [string]$OnlyRegion
)

$ErrorActionPreference = "Stop"

$servers = @(
    @{Region="ohio"; Host="validator1-ohio.schikuno.top"; IP=$null; SSHKey="Foundation-servers/AWS-Ohio/ohio-key.pem"},
    @{Region="singapore"; Host="validator1-singapore.schikuno.top"; IP=$null; SSHKey="Foundation-servers/AWS-Singapore/singapore-key.pem"},
    @{Region="stockholm"; Host="validator1-stockholm.schikuno.top"; IP=$null; SSHKey="Foundation-servers/AWS-Stockholm/stockholm-key.pem"},
    @{Region="saopaulo"; Host="validator1-saopaulo.schikuno.top"; IP=$null; SSHKey="Foundation-servers/AWS-SaoPaulo/saopaulo-key.pem"},
    @{Region="india"; Host="validator1-india.schikuno.top"; IP="74.225.183.196"; SSHKey="Foundation-servers/Azure-India/uramami.pem"},
    @{Region="southafrica"; Host="validator1-southafrica.schikuno.top"; IP="4.221.211.71"; SSHKey="Foundation-servers/Azure-SAfrica/anacreon.pem"},
    @{Region="uae"; Host="validator1-uae.schikuno.top"; IP="4.161.34.228"; SSHKey="Foundation-servers/Azure_UAE/Randal_key.pem"}
)

if ($OnlyRegion) {
    $servers = $servers | Where-Object { $_.Region -eq $OnlyRegion }
    if ($servers.Count -eq 0) {
        Write-Error "Region '$OnlyRegion' not found"
        exit 1
    }
}

Write-Host "🚀 Starting Mainnet Relays" -ForegroundColor Cyan
Write-Host "=========================" -ForegroundColor Cyan
Write-Host ""

$started = 0
$failed = 0

foreach ($server in $servers) {
    Write-Host "Starting relays on $($server.Region)..." -ForegroundColor Yellow
    
    $sshTarget = "azureuser@$($server.Host)"
    if ($server.IP) {
        $sshTarget = "azureuser@$($server.IP)"
    }
    
    try {
        if (-not $DryRun) {
            # Start relay 1
            wsl bash -c "ssh -i '$($server.SSHKey)' -o StrictHostKeyChecking=no $sshTarget 'sudo systemctl start dchat-relay1'"
            Start-Sleep -Seconds 1
            
            # Start relay 2
            wsl bash -c "ssh -i '$($server.SSHKey)' -o StrictHostKeyChecking=no $sshTarget 'sudo systemctl start dchat-relay2'"
            Start-Sleep -Seconds 1
            
            # Check status
            $status1 = wsl bash -c "ssh -i '$($server.SSHKey)' -o StrictHostKeyChecking=no $sshTarget 'sudo systemctl is-active dchat-relay1'"
            $status2 = wsl bash -c "ssh -i '$($server.SSHKey)' -o StrictHostKeyChecking=no $sshTarget 'sudo systemctl is-active dchat-relay2'"
            
            if ($status1 -match "active" -and $status2 -match "active") {
                Write-Host "✓ Both relays on $($server.Region) are running" -ForegroundColor Green
                $started += 2
            } elseif ($status1 -match "active") {
                Write-Host "⚠ Only relay 1 on $($server.Region) is running" -ForegroundColor Yellow
                $started += 1
                $failed += 1
            } elseif ($status2 -match "active") {
                Write-Host "⚠ Only relay 2 on $($server.Region) is running" -ForegroundColor Yellow
                $started += 1
                $failed += 1
            } else {
                Write-Host "❌ No relays on $($server.Region) are running" -ForegroundColor Red
                $failed += 2
            }
        } else {
            Write-Host "[DRY RUN] Would start 2 relays on $($server.Region)" -ForegroundColor Yellow
            $started += 2
        }
        
    } catch {
        Write-Host "❌ Failed to start relays on $($server.Region): $_" -ForegroundColor Red
        $failed += 2
    }
}

Write-Host ""
Write-Host "Summary:" -ForegroundColor Cyan
Write-Host "✓ Started: $started relays" -ForegroundColor Green

if ($failed -gt 0) {
    Write-Host "❌ Failed: $failed relays" -ForegroundColor Red
}

Write-Host ""
Write-Host "Checking relay connectivity..." -ForegroundColor Yellow
Start-Sleep -Seconds 5

foreach ($server in $servers) {
    $sshTarget = "azureuser@$($server.Host)"
    if ($server.IP) {
        $sshTarget = "azureuser@$($server.IP)"
    }
    
    try {
        $health1 = wsl bash -c "curl -s http://$($server.Host):8081/health 2>/dev/null | jq -r '.peer_count' 2>/dev/null"
        $health2 = wsl bash -c "curl -s http://$($server.Host):8082/health 2>/dev/null | jq -r '.peer_count' 2>/dev/null"
        
        Write-Host "$($server.Region) relay1: $health1 peers, relay2: $health2 peers" -ForegroundColor White
    } catch {
        Write-Host "$($server.Region): Unable to check relay health" -ForegroundColor Red
    }
}

Write-Host ""
if ($failed -eq 0) {
    Write-Host "🎉 All relays started successfully!" -ForegroundColor Green
} else {
    Write-Host "⚠ Some relays failed to start" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Monitor logs: ./monitor-mainnet.ps1 -Component Relays" -ForegroundColor Yellow
