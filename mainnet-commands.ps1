#!/usr/bin/env pwsh
# Mainnet command shortcuts - source this file for quick access

# Quick deploy
function Deploy-Mainnet {
    param([switch]$DryRun, [switch]$ValidatorsOnly, [switch]$RelaysOnly)
    
    $params = @()
    if ($DryRun) { $params += "-DryRun" }
    if ($ValidatorsOnly) { $params += "-ValidatorsOnly" }
    if ($RelaysOnly) { $params += "-RelaysOnly" }
    
    & "$PSScriptRoot/deploy-mainnet.ps1" @params
}

# Quick start
function Start-Validators {
    param([switch]$OneByOne, [string]$OnlyRegion)
    
    $params = @()
    if ($OneByOne) { $params += "-OneByOne" }
    if ($OnlyRegion) { $params += "-OnlyRegion"; $params += $OnlyRegion }
    
    & "$PSScriptRoot/start-mainnet-validators.ps1" @params
}

function Start-Relays {
    param([string]$OnlyRegion)
    
    $params = @()
    if ($OnlyRegion) { $params += "-OnlyRegion"; $params += $OnlyRegion }
    
    & "$PSScriptRoot/start-mainnet-relays.ps1" @params
}

# Monitoring shortcuts
function Watch-Validators {
    & "$PSScriptRoot/monitor-mainnet.ps1" -Component Validators -Continuous
}

function Watch-Relays {
    & "$PSScriptRoot/monitor-mainnet.ps1" -Component Relays -Continuous
}

function Watch-Consensus {
    & "$PSScriptRoot/monitor-mainnet.ps1" -Component Consensus -Continuous
}

function Watch-Storage {
    & "$PSScriptRoot/monitor-mainnet.ps1" -Component Storage -Continuous
}

function Watch-All {
    & "$PSScriptRoot/monitor-mainnet.ps1" -Continuous
}

# SSH shortcuts
function SSH-Ohio {
    wsl ssh -i Foundation-servers/AWS-Ohio/ohio-key.pem azureuser@validator1-ohio.schikuno.top
}

function SSH-Singapore {
    wsl ssh -i Foundation-servers/AWS-Singapore/singapore-key.pem azureuser@validator1-singapore.schikuno.top
}

function SSH-Stockholm {
    wsl ssh -i Foundation-servers/AWS-Stockholm/stockholm-key.pem azureuser@validator1-stockholm.schikuno.top
}

function SSH-SaoPaulo {
    wsl ssh -i Foundation-servers/AWS-SaoPaulo/saopaulo-key.pem azureuser@validator1-saopaulo.schikuno.top
}

function SSH-India {
    wsl ssh -i Foundation-servers/Azure-India/uramami.pem azureuser@74.225.183.196
}

function SSH-SouthAfrica {
    wsl ssh -i Foundation-servers/Azure-SAfrica/anacreon.pem azureuser@4.221.211.71
}

function SSH-UAE {
    wsl ssh -i Foundation-servers/Azure_UAE/Randal_key.pem azureuser@4.161.34.228
}

# Health check shortcuts
function Check-ValidatorHealth {
    param([string]$Region = "ohio")
    
    $url = "http://validator1-$Region.schikuno.top:8080/health"
    Write-Host "Checking validator health at $url..." -ForegroundColor Yellow
    wsl curl -s $url | ConvertFrom-Json | Format-List
}

function Check-RelayHealth {
    param([string]$Region = "ohio", [int]$RelayNumber = 1)
    
    $port = 8080 + $RelayNumber
    $url = "http://validator1-$Region.schikuno.top:$port/health"
    Write-Host "Checking relay $RelayNumber health at $url..." -ForegroundColor Yellow
    wsl curl -s $url | ConvertFrom-Json | Format-List
}

# Service management (runs on server)
function Restart-ValidatorService {
    param([Parameter(Mandatory)][string]$Region)
    
    $sshCommand = Get-SSHCommand $Region
    Write-Host "Restarting validator on $Region..." -ForegroundColor Yellow
    wsl bash -c "$sshCommand 'sudo systemctl restart dchat-validator'"
    Start-Sleep -Seconds 2
    wsl bash -c "$sshCommand 'sudo systemctl status dchat-validator'"
}

function Stop-ValidatorService {
    param([Parameter(Mandatory)][string]$Region)
    
    $sshCommand = Get-SSHCommand $Region
    Write-Host "Stopping validator on $Region..." -ForegroundColor Red
    wsl bash -c "$sshCommand 'sudo systemctl stop dchat-validator'"
}

function Get-ValidatorLogs {
    param(
        [Parameter(Mandatory)][string]$Region,
        [int]$Lines = 100,
        [switch]$Follow
    )
    
    $sshCommand = Get-SSHCommand $Region
    $journalCommand = if ($Follow) {
        "sudo journalctl -u dchat-validator -f"
    } else {
        "sudo journalctl -u dchat-validator -n $Lines"
    }
    
    Write-Host "Fetching validator logs from $Region..." -ForegroundColor Yellow
    wsl bash -c "$sshCommand '$journalCommand'"
}

# Helper function
function Get-SSHCommand {
    param([string]$Region)
    
    switch ($Region) {
        "ohio" { "ssh -i Foundation-servers/AWS-Ohio/ohio-key.pem azureuser@validator1-ohio.schikuno.top" }
        "singapore" { "ssh -i Foundation-servers/AWS-Singapore/singapore-key.pem azureuser@validator1-singapore.schikuno.top" }
        "stockholm" { "ssh -i Foundation-servers/AWS-Stockholm/stockholm-key.pem azureuser@validator1-stockholm.schikuno.top" }
        "saopaulo" { "ssh -i Foundation-servers/AWS-SaoPaulo/saopaulo-key.pem azureuser@validator1-saopaulo.schikuno.top" }
        "india" { "ssh -i Foundation-servers/Azure-India/uramami.pem azureuser@74.225.183.196" }
        "southafrica" { "ssh -i Foundation-servers/Azure-SAfrica/anacreon.pem azureuser@4.221.211.71" }
        "uae" { "ssh -i Foundation-servers/Azure_UAE/Randal_key.pem azureuser@4.161.34.228" }
        default { throw "Unknown region: $Region" }
    }
}

# Emergency stop all
function Stop-AllValidators {
    Write-Host "🚨 EMERGENCY: Stopping all validators..." -ForegroundColor Red
    Write-Host "Press Ctrl+C within 5 seconds to cancel..." -ForegroundColor Yellow
    Start-Sleep -Seconds 5
    
    $regions = @("ohio", "singapore", "stockholm", "saopaulo", "india", "southafrica", "uae")
    foreach ($region in $regions) {
        Write-Host "Stopping $region..." -ForegroundColor Red
        Stop-ValidatorService -Region $region
    }
    
    Write-Host "All validators stopped." -ForegroundColor Red
}

# Build commands
function Build-Mainnet {
    Write-Host "Building dchat for mainnet..." -ForegroundColor Yellow
    wsl bash -c "cd /mnt/c/Users/USER/dchat && cargo build --release --bin dchat"
}

function Build-MainnetAndDeploy {
    Build-Mainnet
    if ($LASTEXITCODE -eq 0) {
        Deploy-Mainnet
    } else {
        Write-Host "Build failed, skipping deployment" -ForegroundColor Red
    }
}

# Show help
function Show-MainnetCommands {
    Write-Host ""
    Write-Host "=== dchat Mainnet Command Shortcuts ===" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Deployment:" -ForegroundColor Yellow
    Write-Host "  Deploy-Mainnet [-DryRun] [-ValidatorsOnly] [-RelaysOnly]"
    Write-Host "  Build-Mainnet"
    Write-Host "  Build-MainnetAndDeploy"
    Write-Host ""
    Write-Host "Starting:" -ForegroundColor Yellow
    Write-Host "  Start-Validators [-OneByOne] [-OnlyRegion <region>]"
    Write-Host "  Start-Relays [-OnlyRegion <region>]"
    Write-Host ""
    Write-Host "Monitoring:" -ForegroundColor Yellow
    Write-Host "  Watch-Validators"
    Write-Host "  Watch-Relays"
    Write-Host "  Watch-Consensus"
    Write-Host "  Watch-Storage"
    Write-Host "  Watch-All"
    Write-Host ""
    Write-Host "Health Checks:" -ForegroundColor Yellow
    Write-Host "  Check-ValidatorHealth [-Region <region>]"
    Write-Host "  Check-RelayHealth [-Region <region>] [-RelayNumber 1|2]"
    Write-Host ""
    Write-Host "SSH Access:" -ForegroundColor Yellow
    Write-Host "  SSH-Ohio | SSH-Singapore | SSH-Stockholm | SSH-SaoPaulo"
    Write-Host "  SSH-India | SSH-SouthAfrica | SSH-UAE"
    Write-Host ""
    Write-Host "Service Management:" -ForegroundColor Yellow
    Write-Host "  Restart-ValidatorService -Region <region>"
    Write-Host "  Stop-ValidatorService -Region <region>"
    Write-Host "  Get-ValidatorLogs -Region <region> [-Lines 100] [-Follow]"
    Write-Host ""
    Write-Host "Emergency:" -ForegroundColor Red
    Write-Host "  Stop-AllValidators  # EMERGENCY ONLY"
    Write-Host ""
    Write-Host "Regions: ohio, singapore, stockholm, saopaulo, india, southafrica, uae" -ForegroundColor Gray
    Write-Host ""
}

# Show help on load
Write-Host "✅ Mainnet command shortcuts loaded!" -ForegroundColor Green
Write-Host "Type 'Show-MainnetCommands' for available commands" -ForegroundColor Cyan
