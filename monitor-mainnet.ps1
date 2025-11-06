#!/usr/bin/env pwsh
# Monitor mainnet health and metrics

param(
    [ValidateSet("All", "Validators", "Relays", "Storage", "Consensus")]
    [string]$Component = "All",
    
    [switch]$Continuous,
    [int]$RefreshInterval = 10
)

$ErrorActionPreference = "Stop"

$servers = @(
    @{Region="ohio"; Host="validator1-ohio.schikuno.top"; IP=$null},
    @{Region="singapore"; Host="validator1-singapore.schikuno.top"; IP=$null},
    @{Region="stockholm"; Host="validator1-stockholm.schikuno.top"; IP=$null},
    @{Region="saopaulo"; Host="validator1-saopaulo.schikuno.top"; IP=$null},
    @{Region="india"; Host="validator1-india.schikuno.top"; IP="74.225.183.196"},
    @{Region="southafrica"; Host="validator1-southafrica.schikuno.top"; IP="4.221.211.71"},
    @{Region="uae"; Host="validator1-uae.schikuno.top"; IP="4.161.34.228"}
)

function Show-ValidatorHealth {
    Write-Host ""
    Write-Host "=== VALIDATOR HEALTH ===" -ForegroundColor Cyan
    Write-Host ""
    
    $totalHealthy = 0
    $totalUnhealthy = 0
    
    foreach ($server in $servers) {
        $host_url = if ($server.IP) { $server.IP } else { $server.Host }
        
        try {
            $response = wsl bash -c "curl -s --max-time 5 http://${host_url}:8080/health 2>/dev/null"
            $health = $response | ConvertFrom-Json
            
            $status = $health.status
            $peers = $health.peer_count
            $height = $health.block_height
            $synced = $health.is_synced
            
            $color = if ($status -eq "healthy") { "Green" } else { "Yellow" }
            if ($status -eq "healthy") { $totalHealthy++ } else { $totalUnhealthy++ }
            
            Write-Host ("[{0,-15}] Status: {1,-10} Peers: {2,2} Height: {3,8} Synced: {4}" -f `
                $server.Region, $status, $peers, $height, $synced) -ForegroundColor $color
                
        } catch {
            Write-Host ("[{0,-15}] UNREACHABLE" -f $server.Region) -ForegroundColor Red
            $totalUnhealthy++
        }
    }
    
    Write-Host ""
    Write-Host ("Total: {0} healthy, {1} unhealthy" -f $totalHealthy, $totalUnhealthy) -ForegroundColor $(if ($totalHealthy -ge 4) { "Green" } else { "Red" })
    
    if ($totalHealthy -lt 4) {
        Write-Host "⚠ WARNING: Less than 4/7 validators healthy - consensus at risk!" -ForegroundColor Red
    }
}

function Show-RelayHealth {
    Write-Host ""
    Write-Host "=== RELAY HEALTH ===" -ForegroundColor Cyan
    Write-Host ""
    
    $totalHealthy = 0
    $totalUnhealthy = 0
    
    foreach ($server in $servers) {
        $host_url = if ($server.IP) { $server.IP } else { $server.Host }
        
        # Check relay 1
        try {
            $response1 = wsl bash -c "curl -s --max-time 5 http://${host_url}:8081/health 2>/dev/null"
            $health1 = $response1 | ConvertFrom-Json
            $peers1 = $health1.peer_count
            $status1 = if ($peers1 -ge 5) { "healthy" } else { "degraded" }
            $color1 = if ($status1 -eq "healthy") { "Green" } else { "Yellow" }
            
            Write-Host ("[{0,-15} R1] Peers: {1,2} - {2}" -f $server.Region, $peers1, $status1) -ForegroundColor $color1
            if ($status1 -eq "healthy") { $totalHealthy++ } else { $totalUnhealthy++ }
        } catch {
            Write-Host ("[{0,-15} R1] UNREACHABLE" -f $server.Region) -ForegroundColor Red
            $totalUnhealthy++
        }
        
        # Check relay 2
        try {
            $response2 = wsl bash -c "curl -s --max-time 5 http://${host_url}:8082/health 2>/dev/null"
            $health2 = $response2 | ConvertFrom-Json
            $peers2 = $health2.peer_count
            $status2 = if ($peers2 -ge 5) { "healthy" } else { "degraded" }
            $color2 = if ($status2 -eq "healthy") { "Green" } else { "Yellow" }
            
            Write-Host ("[{0,-15} R2] Peers: {1,2} - {2}" -f $server.Region, $peers2, $status2) -ForegroundColor $color2
            if ($status2 -eq "healthy") { $totalHealthy++ } else { $totalUnhealthy++ }
        } catch {
            Write-Host ("[{0,-15} R2] UNREACHABLE" -f $server.Region) -ForegroundColor Red
            $totalUnhealthy++
        }
    }
    
    Write-Host ""
    Write-Host ("Total: {0}/14 relays healthy" -f $totalHealthy) -ForegroundColor $(if ($totalHealthy -ge 10) { "Green" } elseif ($totalHealthy -ge 7) { "Yellow" } else { "Red" })
}

function Show-ConsensusMetrics {
    Write-Host ""
    Write-Host "=== CONSENSUS METRICS ===" -ForegroundColor Cyan
    Write-Host ""
    
    $heights = @()
    $blockTimes = @()
    
    foreach ($server in $servers) {
        $host_url = if ($server.IP) { $server.IP } else { $server.Host }
        
        try {
            $response = wsl bash -c "curl -s --max-time 5 http://${host_url}:9090/metrics 2>/dev/null | grep 'dchat_chain_height'"
            if ($response -match 'dchat_chain_height\s+(\d+)') {
                $height = [int]$matches[1]
                $heights += $height
            }
            
            $blockTimeResponse = wsl bash -c "curl -s --max-time 5 http://${host_url}:9090/metrics 2>/dev/null | grep 'dchat_block_time_seconds'"
            if ($blockTimeResponse -match 'dchat_block_time_seconds\s+([\d\.]+)') {
                $blockTime = [double]$matches[1]
                $blockTimes += $blockTime
            }
        } catch {
            # Skip unreachable validators
        }
    }
    
    if ($heights.Count -gt 0) {
        $minHeight = ($heights | Measure-Object -Minimum).Minimum
        $maxHeight = ($heights | Measure-Object -Maximum).Maximum
        $heightDiff = $maxHeight - $minHeight
        
        Write-Host ("Block Height Range: {0} - {1} (diff: {2})" -f $minHeight, $maxHeight, $heightDiff) -ForegroundColor $(if ($heightDiff -le 2) { "Green" } elseif ($heightDiff -le 10) { "Yellow" } else { "Red" })
        
        if ($heightDiff -gt 10) {
            Write-Host "⚠ WARNING: Validators out of sync!" -ForegroundColor Red
        }
    }
    
    if ($blockTimes.Count -gt 0) {
        $avgBlockTime = ($blockTimes | Measure-Object -Average).Average
        Write-Host ("Average Block Time: {0:F2}s (target: 6.0s)" -f $avgBlockTime) -ForegroundColor $(if ($avgBlockTime -le 7.0) { "Green" } else { "Yellow" })
    }
}

function Show-StorageHealth {
    Write-Host ""
    Write-Host "=== STORAGE CLUSTER HEALTH ===" -ForegroundColor Cyan
    Write-Host ""
    
    # Redis cluster
    Write-Host "Redis Cluster:" -ForegroundColor Yellow
    $redisHealthy = 0
    foreach ($server in $servers) {
        $host_url = if ($server.IP) { $server.IP } else { $server.Host }
        try {
            $result = wsl bash -c "redis-cli -h $host_url -p 6379 --no-auth-warning PING 2>/dev/null"
            if ($result -match "PONG") {
                Write-Host "  ✓ $($server.Region): Connected" -ForegroundColor Green
                $redisHealthy++
            } else {
                Write-Host "  ✗ $($server.Region): No response" -ForegroundColor Red
            }
        } catch {
            Write-Host "  ✗ $($server.Region): Unreachable" -ForegroundColor Red
        }
    }
    Write-Host "  Status: $redisHealthy/7 nodes reachable" -ForegroundColor $(if ($redisHealthy -ge 5) { "Green" } else { "Red" })
    
    # MinIO cluster
    Write-Host ""
    Write-Host "MinIO Cluster:" -ForegroundColor Yellow
    $minioHealthy = 0
    foreach ($server in $servers) {
        $host_url = if ($server.IP) { $server.IP } else { $server.Host }
        try {
            $result = wsl bash -c "curl -s --max-time 5 http://${host_url}:9000/minio/health/live 2>/dev/null"
            if ($result) {
                Write-Host "  ✓ $($server.Region): Online" -ForegroundColor Green
                $minioHealthy++
            } else {
                Write-Host "  ✗ $($server.Region): Offline" -ForegroundColor Red
            }
        } catch {
            Write-Host "  ✗ $($server.Region): Unreachable" -ForegroundColor Red
        }
    }
    Write-Host "  Status: $minioHealthy/7 nodes online" -ForegroundColor $(if ($minioHealthy -ge 5) { "Green" } else { "Red" })
    
    # TiKV PD (only 3 nodes)
    Write-Host ""
    Write-Host "TiKV Placement Driver:" -ForegroundColor Yellow
    $tikvRegions = @("ohio", "singapore", "stockholm")
    $tikvHealthy = 0
    foreach ($region in $tikvRegions) {
        $server = $servers | Where-Object { $_.Region -eq $region }
        $host_url = if ($server.IP) { $server.IP } else { $server.Host }
        try {
            $result = wsl bash -c "curl -s --max-time 5 http://${host_url}:2379/pd/health 2>/dev/null"
            $health = $result | ConvertFrom-Json
            if ($health.health -eq $true) {
                Write-Host "  ✓ $region: Healthy" -ForegroundColor Green
                $tikvHealthy++
            } else {
                Write-Host "  ✗ $region: Unhealthy" -ForegroundColor Red
            }
        } catch {
            Write-Host "  ✗ $region: Unreachable" -ForegroundColor Red
        }
    }
    Write-Host "  Status: $tikvHealthy/3 PD nodes healthy" -ForegroundColor $(if ($tikvHealthy -ge 2) { "Green" } else { "Red" })
    
    # CockroachDB (cloud-managed)
    Write-Host ""
    Write-Host "CockroachDB: Cloud-managed (check provider dashboard)" -ForegroundColor Yellow
}

do {
    Clear-Host
    Write-Host "╔════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
    Write-Host "║         dchat Mainnet Health Monitor                  ║" -ForegroundColor Cyan
    Write-Host "╚════════════════════════════════════════════════════════╝" -ForegroundColor Cyan
    Write-Host "Last updated: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Gray
    
    if ($Component -in @("All", "Validators")) {
        Show-ValidatorHealth
    }
    
    if ($Component -in @("All", "Relays")) {
        Show-RelayHealth
    }
    
    if ($Component -in @("All", "Consensus")) {
        Show-ConsensusMetrics
    }
    
    if ($Component -in @("All", "Storage")) {
        Show-StorageHealth
    }
    
    if ($Continuous) {
        Write-Host ""
        Write-Host "Press Ctrl+C to stop monitoring. Refreshing in $RefreshInterval seconds..." -ForegroundColor Gray
        Start-Sleep -Seconds $RefreshInterval
    }
} while ($Continuous)
