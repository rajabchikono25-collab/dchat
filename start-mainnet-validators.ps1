#!/usr/bin/env pwsh
# Start all mainnet validators

param(
    [switch]$OneByOne,
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

Write-Host "🚀 Starting Mainnet Validators" -ForegroundColor Cyan
Write-Host "=============================" -ForegroundColor Cyan
Write-Host ""

$started = @()
$failed = @()

foreach ($server in $servers) {
    Write-Host "Starting validator on $($server.Region)..." -ForegroundColor Yellow
    
    $sshTarget = "azureuser@$($server.Host)"
    if ($server.IP) {
        $sshTarget = "azureuser@$($server.IP)"
    }
    
    try {
        if (-not $DryRun) {
            wsl bash -c "ssh -i '$($server.SSHKey)' -o StrictHostKeyChecking=no $sshTarget 'sudo systemctl start dchat-validator'"
            
            # Wait for startup
            Start-Sleep -Seconds 2
            
            # Check status
            $status = wsl bash -c "ssh -i '$($server.SSHKey)' -o StrictHostKeyChecking=no $sshTarget 'sudo systemctl is-active dchat-validator'"
            
            if ($status -match "active") {
                Write-Host "✓ Validator on $($server.Region) is running" -ForegroundColor Green
                $started += $server.Region
            } else {
                throw "Validator failed to start (status: $status)"
            }
        } else {
            Write-Host "[DRY RUN] Would start validator on $($server.Region)" -ForegroundColor Yellow
            $started += $server.Region
        }
        
        if ($OneByOne -and ($server.Region -ne $servers[-1].Region)) {
            Write-Host ""
            Write-Host "Waiting 30 seconds before starting next validator..." -ForegroundColor Yellow
            Start-Sleep -Seconds 30
        }
        
    } catch {
        Write-Host "❌ Failed to start validator on $($server.Region): $_" -ForegroundColor Red
        $failed += $server.Region
    }
}

Write-Host ""
Write-Host "Summary:" -ForegroundColor Cyan
Write-Host "✓ Started: $($started -join ', ')" -ForegroundColor Green

if ($failed.Count -gt 0) {
    Write-Host "❌ Failed: $($failed -join ', ')" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "Waiting for consensus..." -ForegroundColor Yellow
Start-Sleep -Seconds 10

Write-Host "Checking validator status..." -ForegroundColor Yellow
foreach ($server in $servers) {
    $sshTarget = "azureuser@$($server.Host)"
    if ($server.IP) {
        $sshTarget = "azureuser@$($server.IP)"
    }
    
    try {
        $healthCheck = wsl bash -c "curl -s http://$($server.Host):8080/health | jq -r '.status'"
        Write-Host "$($server.Region): $healthCheck" -ForegroundColor $(if ($healthCheck -eq "healthy") { "Green" } else { "Yellow" })
    } catch {
        Write-Host "$($server.Region): Unable to check health" -ForegroundColor Red
    }
}

Write-Host ""
Write-Host "🎉 All validators started!" -ForegroundColor Green
Write-Host ""
Write-Host "Monitor logs: ./monitor-mainnet.ps1 -Component Validators" -ForegroundColor Yellow
