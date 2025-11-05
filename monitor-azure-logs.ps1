#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Monitor logs from all Azure validators in real-time

.DESCRIPTION
    Connects to all three Azure servers and tails their dchat service logs
    in parallel, showing a live stream of activity.
#>

$ErrorActionPreference = "Stop"

# ANSI colors
$Red = "`e[31m"
$Green = "`e[32m"
$Yellow = "`e[33m"
$Blue = "`e[34m"
$Magenta = "`e[35m"
$Cyan = "`e[36m"
$Reset = "`e[0m"

$AzureServers = @(
    @{ Name = "India"; IP = "74.225.183.196"; User = "azureuser"; Key = "Foundation-servers/Azure-India/uramami.pem"; Color = $Green },
    @{ Name = "South Africa"; IP = "4.221.211.71"; User = "azureuser"; Key = "Foundation-servers/Azure-SAfrica/anacreon.pem"; Color = $Yellow },
    @{ Name = "UAE"; IP = "4.161.34.228"; User = "azureuser"; Key = "Foundation-servers/Azure_UAE/Randal_key.pem"; Color = $Blue }
)

Write-Host "${Magenta}=== Azure Validators Log Monitor ===${Reset}`n" -ForegroundColor Magenta

foreach ($server in $AzureServers) {
    Write-Host "$($server.Color)[$($server.Name)]${Reset} Starting log stream from $($server.IP)..."
}

Write-Host "`n${Cyan}Press Ctrl+C to stop monitoring${Reset}`n"
Start-Sleep -Seconds 2

# Create a script block for each server
$jobs = @()

foreach ($server in $AzureServers) {
    $job = Start-Job -ScriptBlock {
        param($ServerName, $IP, $User, $KeyPath, $ColorCode)
        
        $keyPath = Resolve-Path $KeyPath
        $prefix = "[$ServerName]"
        
        # Tail logs via SSH
        wsl ssh -i "'$keyPath'" -o StrictHostKeyChecking=no "$User@$IP" "sudo journalctl -u dchat -f -n 20" 2>&1 | ForEach-Object {
            Write-Host "${ColorCode}${prefix}${Reset} $_"
        }
    } -ArgumentList $server.Name, $server.IP, $server.User, $server.Key, $server.Color
    
    $jobs += $job
}

# Wait for Ctrl+C
try {
    while ($true) {
        Start-Sleep -Seconds 1
        
        # Display any output from jobs
        foreach ($job in $jobs) {
            Receive-Job -Job $job
        }
    }
} finally {
    Write-Host "`n${Yellow}Stopping log monitoring...${Reset}"
    $jobs | Stop-Job
    $jobs | Remove-Job
}
