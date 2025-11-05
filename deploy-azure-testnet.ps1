#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Deploy dchat to Azure testnet servers before mainnet launch

.DESCRIPTION
    This script deploys the latest dchat build to three Azure servers:
    - India (Mumbai) - 74.225.183.196
    - South Africa (Johannesburg) - 4.221.211.71
    - UAE (Dubai) - 4.161.34.228
    
    It builds the binary in WSL, deploys to all three servers, and verifies health.

.PARAMETER SkipBuild
    Skip the build step and use existing binary

.PARAMETER SkipBackup
    Skip backing up existing deployment before updating
#>

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$SkipBackup
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# ANSI color codes for better output
$Red = "`e[31m"
$Green = "`e[32m"
$Yellow = "`e[33m"
$Blue = "`e[34m"
$Magenta = "`e[35m"
$Cyan = "`e[36m"
$Reset = "`e[0m"

# Azure server configuration
$AzureServers = @(
    @{
        Name = "India"
        Region = "Mumbai"
        IP = "74.225.183.196"
        User = "azureuser"
        KeyFile = "Foundation-servers/Azure-India/uramami.pem"
        Domain = "validator1-india.schikuno.top"
    },
    @{
        Name = "South Africa"
        Region = "Johannesburg"
        IP = "4.221.211.71"
        User = "azureuser"
        KeyFile = "Foundation-servers/Azure-SAfrica/anacreon.pem"
        Domain = "validator1-southafrica.schikuno.top"
    },
    @{
        Name = "UAE"
        Region = "Dubai"
        IP = "4.161.34.228"
        User = "azureuser"
        KeyFile = "Foundation-servers/Azure_UAE/Randal_key.pem"
        Domain = "validator1-uae.schikuno.top"
    }
)

function Write-Step {
    param([string]$Message)
    Write-Host "`n${Cyan}==>${Reset} ${Message}" -ForegroundColor Cyan
}

function Write-Success {
    param([string]$Message)
    Write-Host "${Green}✓${Reset} ${Message}" -ForegroundColor Green
}

function Write-Error-Message {
    param([string]$Message)
    Write-Host "${Red}✗${Reset} ${Message}" -ForegroundColor Red
}

function Write-Warning-Message {
    param([string]$Message)
    Write-Host "${Yellow}⚠${Reset} ${Message}" -ForegroundColor Yellow
}

function Write-Info {
    param([string]$Message)
    Write-Host "${Blue}ℹ${Reset} ${Message}" -ForegroundColor Blue
}

# Check prerequisites
function Test-Prerequisites {
    Write-Step "Checking prerequisites..."
    
    # Check if WSL is available
    try {
        $wslCheck = wsl --status 2>&1
        Write-Success "WSL is available"
    } catch {
        Write-Error-Message "WSL is not available. Please install WSL2 first."
        exit 1
    }
    
    # Check if SSH keys exist
    $missingKeys = @()
    foreach ($server in $AzureServers) {
        if (-not (Test-Path $server.KeyFile)) {
            $missingKeys += $server.KeyFile
        }
    }
    
    if ($missingKeys.Count -gt 0) {
        Write-Error-Message "Missing SSH key files:"
        $missingKeys | ForEach-Object { Write-Host "  - $_" }
        exit 1
    }
    Write-Success "All SSH keys found"
    
    # Check if keys have correct permissions (on Windows, this is informational)
    Write-Info "Verifying SSH key permissions..."
    foreach ($server in $AzureServers) {
        $keyPath = Resolve-Path $server.KeyFile
        # On Windows, we just need to ensure the file is readable
        if (Test-Path $keyPath) {
            Write-Success "Key for $($server.Name): $keyPath"
        }
    }
}

# Build binary in WSL
function Build-Binary {
    if ($SkipBuild) {
        Write-Warning-Message "Skipping build (using existing binary)"
        return
    }
    
    Write-Step "Building dchat binary in WSL..."
    
    # Get Windows path and convert to WSL path
    $currentPath = (Get-Location).Path
    $wslPath = wsl wslpath -a "'$currentPath'"
    
    Write-Info "Building in: $wslPath"
    
    # Build command
    $buildCmd = @"
cd $wslPath && \
cargo build --release --bin dchat && \
ls -lh target/release/dchat && \
echo "Binary size: `$(du -h target/release/dchat | cut -f1)"
"@
    
    Write-Info "Running: cargo build --release --bin dchat"
    $buildStart = Get-Date
    
    if ($VerbosePreference -eq 'Continue') {
        wsl bash -c $buildCmd
    } else {
        $buildOutput = wsl bash -c $buildCmd 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Error-Message "Build failed!"
            Write-Host $buildOutput
            exit 1
        }
    }
    
    $buildDuration = (Get-Date) - $buildStart
    Write-Success "Build completed in $([math]::Round($buildDuration.TotalSeconds, 1)) seconds"
    
    # Verify binary exists
    if (-not (Test-Path "target\release\dchat")) {
        Write-Error-Message "Binary not found at target/release/dchat"
        exit 1
    }
    
    # Show binary info
    $binarySize = (Get-Item "target\release\dchat").Length
    $binarySizeMB = [math]::Round($binarySize / 1MB, 2)
    Write-Success "Binary size: ${binarySizeMB} MB"
}

# Test SSH connectivity to a server
function Test-ServerConnectivity {
    param(
        [hashtable]$Server
    )
    
    $sshCmd = "echo 'Connection successful'"
    $keyPath = Resolve-Path $Server.KeyFile
    
    try {
        $result = wsl ssh -i "'$keyPath'" -o StrictHostKeyChecking=no -o ConnectTimeout=10 "$($Server.User)@$($Server.IP)" "$sshCmd" 2>&1
        if ($LASTEXITCODE -eq 0) {
            return $true
        }
    } catch {
        return $false
    }
    return $false
}

# Backup existing deployment on server
function Backup-Deployment {
    param(
        [hashtable]$Server
    )
    
    if ($SkipBackup) {
        Write-Info "Skipping backup for $($Server.Name)"
        return
    }
    
    Write-Info "Backing up existing deployment on $($Server.Name)..."
    
    $keyPath = Resolve-Path $Server.KeyFile
    $timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
    
    $backupCmd = @"
if [ -f /opt/dchat/dchat ]; then
    sudo cp /opt/dchat/dchat /opt/dchat/dchat.backup.$timestamp
    echo 'Backed up to dchat.backup.$timestamp'
else
    echo 'No existing binary to backup'
fi
"@
    
    wsl ssh -i "'$keyPath'" -o StrictHostKeyChecking=no "$($Server.User)@$($Server.IP)" "$backupCmd" 2>&1 | Out-Null
}

# Deploy to a single server
function Deploy-ToServer {
    param(
        [hashtable]$Server
    )
    
    Write-Step "Deploying to $($Server.Name) ($($Server.Region))"
    Write-Info "IP: $($Server.IP)"
    Write-Info "Domain: $($Server.Domain)"
    
    # Test connectivity
    Write-Info "Testing SSH connectivity..."
    if (-not (Test-ServerConnectivity -Server $Server)) {
        Write-Error-Message "Cannot connect to $($Server.Name)"
        return $false
    }
    Write-Success "Connected to $($Server.Name)"
    
    # Backup existing deployment
    Backup-Deployment -Server $Server
    
    # Get paths
    $keyPath = Resolve-Path $Server.KeyFile
    $localBinary = Resolve-Path "target\release\dchat"
    
    Write-Info "Copying binary to server..."
    $scpCmd = "scp -i '$keyPath' -o StrictHostKeyChecking=no '$localBinary' $($Server.User)@$($Server.IP):/tmp/dchat"
    wsl bash -c $scpCmd 2>&1 | Out-Null
    
    if ($LASTEXITCODE -ne 0) {
        Write-Error-Message "Failed to copy binary to $($Server.Name)"
        return $false
    }
    Write-Success "Binary copied to server"
    
    # Deploy and restart service
    Write-Info "Installing binary and restarting service..."
    $deployCmd = @"
sudo mv /tmp/dchat /opt/dchat/dchat && \
sudo chmod +x /opt/dchat/dchat && \
sudo chown root:root /opt/dchat/dchat && \
sudo systemctl restart dchat && \
sleep 3 && \
sudo systemctl status dchat --no-pager
"@
    
    $sshDeployCmd = "ssh -i '$keyPath' -o StrictHostKeyChecking=no $($Server.User)@$($Server.IP) '$deployCmd'"
    $deployResult = wsl bash -c $sshDeployCmd 2>&1
    
    if ($LASTEXITCODE -ne 0) {
        Write-Error-Message "Failed to deploy on $($Server.Name)"
        Write-Host $deployResult
        return $false
    }
    
    Write-Success "Service restarted on $($Server.Name)"
    
    # Verify health
    Write-Info "Checking health endpoint..."
    Start-Sleep -Seconds 5
    
    try {
        $health = Invoke-RestMethod -Uri "http://$($Server.IP)/health" -TimeoutSec 10 -ErrorAction Stop
        if ($health.status -eq "healthy") {
            Write-Success "Health check passed: $($health.status)"
        } else {
            Write-Warning-Message "Unexpected health status: $($health | ConvertTo-Json -Compress)"
        }
    } catch {
        Write-Warning-Message "Health endpoint not responding yet (this may be normal during startup)"
    }
    
    Write-Success "Deployment to $($Server.Name) complete!"
    return $true
}

# Check status of all servers
function Get-NetworkStatus {
    Write-Step "Checking network status..."
    
    $statusResults = @()
    
    foreach ($server in $AzureServers) {
        Write-Info "Querying $($server.Name)..."
        
        $status = @{
            Name = $server.Name
            Region = $server.Region
            IP = $server.IP
            Reachable = $false
            Healthy = $false
            BlockHeight = "N/A"
            Peers = "N/A"
            Version = "N/A"
        }
        
        # Test basic connectivity
        if (Test-Connection -ComputerName $server.IP -Count 1 -Quiet -TimeoutSeconds 2) {
            $status.Reachable = $true
            
            # Try health endpoint
            try {
                $health = Invoke-RestMethod -Uri "http://$($server.IP)/health" -TimeoutSec 5 -ErrorAction Stop
                $status.Healthy = ($health.status -eq "healthy")
                
                # Try status endpoint
                try {
                    $statusInfo = Invoke-RestMethod -Uri "http://$($server.IP)/status" -TimeoutSec 5 -ErrorAction Stop
                    $status.BlockHeight = $statusInfo.block_height
                    $status.Peers = $statusInfo.peer_count
                    $status.Version = $statusInfo.version
                } catch {
                    # Status endpoint might not exist yet
                }
            } catch {
                # Health endpoint not responding
            }
        }
        
        $statusResults += $status
    }
    
    # Display results
    Write-Host "`n${Cyan}Azure Testnet Status:${Reset}" -ForegroundColor Cyan
    Write-Host ("=" * 80)
    
    foreach ($status in $statusResults) {
        $healthIcon = if ($status.Healthy) { "${Green}✓${Reset}" } else { "${Red}✗${Reset}" }
        Write-Host "$healthIcon $($status.Name) ($($status.Region))"
        Write-Host "   IP: $($status.IP)"
        Write-Host "   Reachable: $(if ($status.Reachable) { 'Yes' } else { 'No' })"
        Write-Host "   Healthy: $(if ($status.Healthy) { 'Yes' } else { 'No' })"
        if ($status.BlockHeight -ne "N/A") {
            Write-Host "   Block Height: $($status.BlockHeight)"
            Write-Host "   Peers: $($status.Peers)"
            Write-Host "   Version: $($status.Version)"
        }
        Write-Host ""
    }
    
    # Summary
    $healthyCount = ($statusResults | Where-Object { $_.Healthy }).Count
    $totalCount = $statusResults.Count
    
    Write-Host ("=" * 80)
    Write-Host "${Cyan}Summary:${Reset} $healthyCount/$totalCount validators healthy" -ForegroundColor Cyan
    
    if ($healthyCount -eq $totalCount) {
        Write-Success "All Azure validators are operational! 🎉"
    } elseif ($healthyCount -gt 0) {
        Write-Warning-Message "Some validators need attention"
    } else {
        Write-Error-Message "No validators are healthy - manual intervention required"
    }
}

# Main deployment flow
function Start-Deployment {
    Write-Host "${Magenta}"
    Write-Host "╔════════════════════════════════════════════════════════════╗"
    Write-Host "║                                                            ║"
    Write-Host "║         dchat Azure Testnet Deployment                    ║"
    Write-Host "║                                                            ║"
    Write-Host "║         Deploying to 3 Azure servers                      ║"
    Write-Host "║         India • South Africa • UAE                        ║"
    Write-Host "║                                                            ║"
    Write-Host "╚════════════════════════════════════════════════════════════╝"
    Write-Host "${Reset}`n"
    
    # Prerequisites
    Test-Prerequisites
    
    # Build
    Build-Binary
    
    # Deploy to each server
    $deployResults = @{}
    $successCount = 0
    
    foreach ($server in $AzureServers) {
        $success = Deploy-ToServer -Server $server
        $deployResults[$server.Name] = $success
        if ($success) {
            $successCount++
        }
        Write-Host ""
    }
    
    # Summary
    Write-Host "${Cyan}═══════════════════════════════════════════════════════════${Reset}"
    Write-Step "Deployment Summary"
    
    foreach ($server in $AzureServers) {
        $success = $deployResults[$server.Name]
        $icon = if ($success) { "${Green}✓${Reset}" } else { "${Red}✗${Reset}" }
        $statusText = if ($success) { "SUCCESS" } else { "FAILED" }
        Write-Host "$icon $($server.Name): $statusText"
    }
    
    Write-Host ""
    
    if ($successCount -eq $AzureServers.Count) {
        Write-Success "All deployments successful! ($successCount/$($AzureServers.Count))"
    } elseif ($successCount -gt 0) {
        Write-Warning-Message "Partial deployment: $successCount/$($AzureServers.Count) succeeded"
    } else {
        Write-Error-Message "All deployments failed"
        exit 1
    }
    
    # Wait a bit for services to stabilize
    Write-Info "Waiting 10 seconds for services to stabilize..."
    Start-Sleep -Seconds 10
    
    # Check network status
    Get-NetworkStatus
    
    Write-Host "`n${Green}Deployment complete!${Reset}" -ForegroundColor Green
    Write-Host "${Cyan}Next steps:${Reset}"
    Write-Host "  1. Monitor logs: ./monitor-azure-logs.ps1"
    Write-Host "  2. Check status: ./deploy-azure-testnet.ps1 -StatusOnly"
    Write-Host "  3. Test connectivity: ./test-azure-connectivity.ps1"
    Write-Host ""
}

# If -StatusOnly parameter, just check status
if ($args -contains "-StatusOnly") {
    Get-NetworkStatus
    exit 0
}

# Run main deployment
Start-Deployment
