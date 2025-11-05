#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Test connectivity and health of Azure validators

.DESCRIPTION
    Comprehensive connectivity tests for all Azure servers including:
    - Ping test
    - SSH connectivity
    - HTTP health endpoint
    - Service status
    - Peer connectivity
#>

$ErrorActionPreference = "Continue"

$Green = "`e[32m"
$Red = "`e[31m"
$Yellow = "`e[33m"
$Cyan = "`e[36m"
$Reset = "`e[0m"

$AzureServers = @(
    @{
        Name = "India (Mumbai)"
        IP = "74.225.183.196"
        User = "azureuser"
        Key = "Foundation-servers/Azure-India/uramami.pem"
        Domain = "validator1-india.schikuno.top"
    },
    @{
        Name = "South Africa (Johannesburg)"
        IP = "4.221.211.71"
        User = "azureuser"
        Key = "Foundation-servers/Azure-SAfrica/anacreon.pem"
        Domain = "validator1-southafrica.schikuno.top"
    },
    @{
        Name = "UAE (Dubai)"
        IP = "4.161.34.228"
        User = "azureuser"
        Key = "Foundation-servers/Azure_UAE/Randal_key.pem"
        Domain = "validator1-uae.schikuno.top"
    }
)

function Test-SingleServer {
    param([hashtable]$Server)
    
    Write-Host "`n${Cyan}═══════════════════════════════════════════════════════════${Reset}"
    Write-Host "${Cyan}Testing: $($Server.Name)${Reset}"
    Write-Host "${Cyan}IP: $($Server.IP) | Domain: $($Server.Domain)${Reset}"
    Write-Host "${Cyan}═══════════════════════════════════════════════════════════${Reset}`n"
    
    $results = @{
        Ping = $false
        SSH = $false
        Health = $false
        Service = $false
        Peers = "N/A"
        BlockHeight = "N/A"
    }
    
    # Test 1: Ping
    Write-Host "[1/6] Ping test... " -NoNewline
    try {
        $pingResult = Test-Connection -ComputerName $Server.IP -Count 2 -Quiet -TimeoutSeconds 3
        if ($pingResult) {
            Write-Host "${Green}✓ OK${Reset}" -ForegroundColor Green
            $results.Ping = $true
        } else {
            Write-Host "${Red}✗ FAILED${Reset}" -ForegroundColor Red
        }
    } catch {
        Write-Host "${Red}✗ ERROR${Reset}" -ForegroundColor Red
    }
    
    # Test 2: SSH connectivity
    Write-Host "[2/6] SSH connectivity... " -NoNewline
    try {
        $keyPath = Resolve-Path $Server.Key
        $sshTest = wsl ssh -i "'$keyPath'" -o StrictHostKeyChecking=no -o ConnectTimeout=10 "$($Server.User)@$($Server.IP)" "echo 'SSH OK'" 2>&1
        if ($LASTEXITCODE -eq 0) {
            Write-Host "${Green}✓ OK${Reset}" -ForegroundColor Green
            $results.SSH = $true
        } else {
            Write-Host "${Red}✗ FAILED${Reset}" -ForegroundColor Red
        }
    } catch {
        Write-Host "${Red}✗ ERROR${Reset}" -ForegroundColor Red
    }
    
    # Test 3: HTTP Health endpoint
    Write-Host "[3/6] Health endpoint (http://$($Server.IP)/health)... " -NoNewline
    try {
        $health = Invoke-RestMethod -Uri "http://$($Server.IP)/health" -TimeoutSec 10 -ErrorAction Stop
        if ($health.status -eq "healthy") {
            Write-Host "${Green}✓ HEALTHY${Reset}" -ForegroundColor Green
            $results.Health = $true
        } else {
            Write-Host "${Yellow}⚠ Status: $($health.status)${Reset}" -ForegroundColor Yellow
        }
    } catch {
        Write-Host "${Red}✗ NOT RESPONDING${Reset}" -ForegroundColor Red
    }
    
    # Test 4: Service status (via SSH)
    Write-Host "[4/6] dchat service status... " -NoNewline
    if ($results.SSH) {
        try {
            $keyPath = Resolve-Path $Server.Key
            $serviceStatus = wsl ssh -i "'$keyPath'" -o StrictHostKeyChecking=no "$($Server.User)@$($Server.IP)" "sudo systemctl is-active dchat" 2>&1
            if ($serviceStatus -match "active") {
                Write-Host "${Green}✓ ACTIVE${Reset}" -ForegroundColor Green
                $results.Service = $true
            } else {
                Write-Host "${Red}✗ $serviceStatus${Reset}" -ForegroundColor Red
            }
        } catch {
            Write-Host "${Red}✗ ERROR${Reset}" -ForegroundColor Red
        }
    } else {
        Write-Host "${Yellow}⚠ SKIPPED (SSH failed)${Reset}" -ForegroundColor Yellow
    }
    
    # Test 5: Node status
    Write-Host "[5/6] Node status... " -NoNewline
    try {
        $status = Invoke-RestMethod -Uri "http://$($Server.IP)/status" -TimeoutSec 10 -ErrorAction Stop
        $results.BlockHeight = $status.block_height
        $results.Peers = $status.peer_count
        Write-Host "${Green}✓ Block: $($status.block_height), Peers: $($status.peer_count)${Reset}" -ForegroundColor Green
    } catch {
        Write-Host "${Yellow}⚠ NOT AVAILABLE${Reset}" -ForegroundColor Yellow
    }
    
    # Test 6: Recent logs
    Write-Host "[6/6] Recent activity... " -NoNewline
    if ($results.SSH) {
        try {
            $keyPath = Resolve-Path $Server.Key
            $logs = wsl ssh -i "'$keyPath'" -o StrictHostKeyChecking=no "$($Server.User)@$($Server.IP)" "sudo journalctl -u dchat -n 5 --no-pager" 2>&1
            Write-Host "${Green}✓ OK${Reset}" -ForegroundColor Green
            Write-Host "`n${Yellow}Recent logs:${Reset}"
            Write-Host $logs
        } catch {
            Write-Host "${Red}✗ ERROR${Reset}" -ForegroundColor Red
        }
    } else {
        Write-Host "${Yellow}⚠ SKIPPED${Reset}" -ForegroundColor Yellow
    }
    
    # Summary for this server
    $passedTests = ($results.Ping, $results.SSH, $results.Health, $results.Service | Where-Object { $_ }).Count
    Write-Host "`n${Cyan}Summary:${Reset} $passedTests/4 core tests passed"
    
    return $results
}

# Main execution
Write-Host "${Cyan}"
Write-Host "╔════════════════════════════════════════════════════════════╗"
Write-Host "║                                                            ║"
Write-Host "║         Azure Validators Connectivity Test                ║"
Write-Host "║                                                            ║"
Write-Host "╚════════════════════════════════════════════════════════════╝"
Write-Host "${Reset}`n"

$allResults = @()

foreach ($server in $AzureServers) {
    $result = Test-SingleServer -Server $server
    $allResults += @{
        Server = $server
        Results = $result
    }
}

# Final summary
Write-Host "`n${Cyan}════════════════════════════════════════════════════════════${Reset}"
Write-Host "${Cyan}Overall Summary${Reset}"
Write-Host "${Cyan}════════════════════════════════════════════════════════════${Reset}`n"

$healthyCount = ($allResults | Where-Object { $_.Results.Health }).Count
$totalCount = $allResults.Count

Write-Host "| Server              | Ping | SSH | Health | Service | Peers      |"
Write-Host "|---------------------|------|-----|--------|---------|------------|"

foreach ($item in $allResults) {
    $r = $item.Results
    $name = $item.Server.Name.PadRight(19)
    $ping = if ($r.Ping) { "✓" } else { "✗" }
    $ssh = if ($r.SSH) { "✓" } else { "✗" }
    $health = if ($r.Health) { "✓" } else { "✗" }
    $service = if ($r.Service) { "✓" } else { "✗" }
    $peers = if ($r.Peers -ne "N/A") { $r.Peers.ToString().PadLeft(10) } else { "N/A".PadLeft(10) }
    
    Write-Host "| $name | $ping    | $ssh   | $health      | $service       | $peers |"
}

Write-Host ""

if ($healthyCount -eq $totalCount) {
    Write-Host "${Green}✓ All Azure validators are operational! ($healthyCount/$totalCount)${Reset}" -ForegroundColor Green
} elseif ($healthyCount -gt 0) {
    Write-Host "${Yellow}⚠ Partial availability: $healthyCount/$totalCount validators healthy${Reset}" -ForegroundColor Yellow
} else {
    Write-Host "${Red}✗ No validators are healthy - manual intervention required${Reset}" -ForegroundColor Red
}

Write-Host "`n${Cyan}Next steps:${Reset}"
Write-Host "  • Deploy/update: ./deploy-azure-testnet.ps1"
Write-Host "  • Monitor logs: ./monitor-azure-logs.ps1"
Write-Host "  • Check full network: ./deploy-azure-testnet.ps1 -StatusOnly"
