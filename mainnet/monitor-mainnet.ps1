# dchat Mainnet Monitor Script
# Monitor all mainnet nodes

param(
    [switch]$Continuous,
    [int]$Interval = 30
)

$Servers = @{
    "validator-india" = @{ IP = "74.225.183.196"; DNS = "validator.india.schikuno.top"; Role = "validator" }
    "validator-southafrica" = @{ IP = "4.221.211.71"; DNS = "validator.southafrica.schikuno.top"; Role = "validator" }
    "validator-uae" = @{ IP = "4.161.34.228"; DNS = "validator.uae.schikuno.top"; Role = "validator" }
    "relay-ohio" = @{ IP = "13.58.182.122"; DNS = "relay.ohio.schikuno.top"; Role = "relay" }
    "relay-stockholm" = @{ IP = "16.16.212.80"; DNS = "relay.stockholm.schikuno.top"; Role = "relay" }
    "relay-saopaulo" = @{ IP = "18.230.144.17"; DNS = "relay.saopaulo.schikuno.top"; Role = "relay" }
    "user-singapore" = @{ IP = "13.251.102.178"; DNS = "user.singapore.schikuno.top"; Role = "user" }
}

function Get-NodeHealth {
    param([string]$DNS)
    try {
        $response = Invoke-RestMethod -Uri "http://${DNS}:8080/health" -TimeoutSec 5 -ErrorAction Stop
        return @{ Status = "healthy"; Data = $response }
    } catch {
        return @{ Status = "unhealthy"; Error = $_.Exception.Message }
    }
}

function Show-Dashboard {
    Clear-Host
    Write-Host "=" * 80 -ForegroundColor Cyan
    Write-Host "  dchat MAINNET MONITOR" -ForegroundColor Cyan
    Write-Host "  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Gray
    Write-Host "=" * 80 -ForegroundColor Cyan
    Write-Host ""
    
    Write-Host "VALIDATORS" -ForegroundColor Yellow
    Write-Host "-" * 80
    Write-Host "Node                     IP               Health    Peers    Block" -ForegroundColor Gray
    
    foreach ($server in $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "validator" }) {
        $health = Get-NodeHealth -DNS $server.Value.DNS
        $statusColor = if ($health.Status -eq "healthy") { "Green" } else { "Red" }
        $peers = if ($health.Data.peers) { $health.Data.peers } else { "N/A" }
        $block = if ($health.Data.block_height) { $health.Data.block_height } else { "N/A" }
        
        Write-Host "$($server.Key.PadRight(24)) $($server.Value.IP.PadRight(16)) " -NoNewline
        Write-Host "$($health.Status.PadRight(9))" -ForegroundColor $statusColor -NoNewline
        Write-Host " $($peers.ToString().PadRight(8)) $block"
    }
    
    Write-Host ""
    Write-Host "RELAYS" -ForegroundColor Yellow
    Write-Host "-" * 80
    
    foreach ($server in $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "relay" }) {
        $health = Get-NodeHealth -DNS $server.Value.DNS
        $statusColor = if ($health.Status -eq "healthy") { "Green" } else { "Red" }
        $peers = if ($health.Data.peers) { $health.Data.peers } else { "N/A" }
        
        Write-Host "$($server.Key.PadRight(24)) $($server.Value.IP.PadRight(16)) " -NoNewline
        Write-Host "$($health.Status.PadRight(9))" -ForegroundColor $statusColor -NoNewline
        Write-Host " $peers"
    }
    
    Write-Host ""
    Write-Host "USER CLIENTS" -ForegroundColor Yellow
    Write-Host "-" * 80
    
    foreach ($server in $Servers.GetEnumerator() | Where-Object { $_.Value.Role -eq "user" }) {
        $health = Get-NodeHealth -DNS $server.Value.DNS
        $statusColor = if ($health.Status -eq "healthy") { "Green" } else { "Red" }
        
        Write-Host "$($server.Key.PadRight(24)) $($server.Value.IP.PadRight(16)) " -NoNewline
        Write-Host "$($health.Status)" -ForegroundColor $statusColor
    }
    
    Write-Host ""
    Write-Host "=" * 80 -ForegroundColor Cyan
    if ($Continuous) {
        Write-Host "Refreshing every $Interval seconds... Press Ctrl+C to stop" -ForegroundColor Gray
    }
}

if ($Continuous) {
    while ($true) {
        Show-Dashboard
        Start-Sleep -Seconds $Interval
    }
} else {
    Show-Dashboard
}
