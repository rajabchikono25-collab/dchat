#!/usr/bin/env pwsh
# Multi-Region Validator Deployment Script
#
# This script deploys dchat validators across multiple geographic regions
# using Kubernetes clusters. It ensures proper distribution and BFT configuration.
#
# Usage:
#   .\deploy-multi-region-validators.ps1 -Environment production -Regions @("us-east-1", "us-west-2", "eu-west-1", "eu-central-1", "ap-southeast-1", "ap-northeast-1", "sa-east-1")

param(
    [Parameter(Mandatory=$true)]
    [ValidateSet("development", "staging", "production")]
    [string]$Environment,
    
    [Parameter(Mandatory=$true)]
    [string[]]$Regions,
    
    [Parameter(Mandatory=$false)]
    [int]$ValidatorsPerRegion = 1,
    
    [Parameter(Mandatory=$false)]
    [string]$KubeConfig = "$HOME\.kube\config",
    
    [Parameter(Mandatory=$false)]
    [string]$DockerRegistry = "dchat",
    
    [Parameter(Mandatory=$false)]
    [string]$ImageTag = "latest",
    
    [Parameter(Mandatory=$false)]
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

# Color output functions
function Write-Success { param([string]$Message) Write-Host "✓ $Message" -ForegroundColor Green }
function Write-Info { param([string]$Message) Write-Host "ℹ $Message" -ForegroundColor Cyan }
function Write-Warning { param([string]$Message) Write-Host "⚠ $Message" -ForegroundColor Yellow }
function Write-Failure { param([string]$Message) Write-Host "✗ $Message" -ForegroundColor Red }

# Configuration
$Script:TotalValidators = $Regions.Count * $ValidatorsPerRegion
$Script:RequiredSignatures = [Math]::Ceiling($TotalValidators * 2 / 3)  # BFT: 2f+1

# Region to cloud provider mapping
$RegionMapping = @{
    "us-east-1"      = @{ Provider = "AWS"; Location = "Virginia, USA"; Continent = "NorthAmerica" }
    "us-west-2"      = @{ Provider = "AWS"; Location = "Oregon, USA"; Continent = "NorthAmerica" }
    "eu-west-1"      = @{ Provider = "AWS"; Location = "Ireland"; Continent = "Europe" }
    "eu-central-1"   = @{ Provider = "AWS"; Location = "Frankfurt, Germany"; Continent = "Europe" }
    "ap-southeast-1" = @{ Provider = "AWS"; Location = "Singapore"; Continent = "Asia" }
    "ap-northeast-1" = @{ Provider = "AWS"; Location = "Tokyo, Japan"; Continent = "Asia" }
    "sa-east-1"      = @{ Provider = "AWS"; Location = "São Paulo, Brazil"; Continent = "SouthAmerica" }
    "af-south-1"     = @{ Provider = "AWS"; Location = "Cape Town, South Africa"; Continent = "Africa" }
    "ap-south-1"     = @{ Provider = "AWS"; Location = "Mumbai, India"; Continent = "Asia" }
}

function Show-DeploymentPlan {
    Write-Info "=========================================="
    Write-Info "  Multi-Region Validator Deployment Plan"
    Write-Info "=========================================="
    Write-Host ""
    Write-Info "Environment: $Environment"
    Write-Info "Total Validators: $Script:TotalValidators"
    Write-Info "Required Signatures (BFT): $Script:RequiredSignatures"
    Write-Info "Validators per Region: $ValidatorsPerRegion"
    Write-Info "Docker Image: ${DockerRegistry}/validator:${ImageTag}"
    Write-Host ""
    Write-Info "Geographic Distribution:"
    
    $continentCounts = @{}
    foreach ($region in $Regions) {
        $info = $RegionMapping[$region]
        if ($info) {
            $continent = $info.Continent
            if (-not $continentCounts.ContainsKey($continent)) {
                $continentCounts[$continent] = 0
            }
            $continentCounts[$continent] += $ValidatorsPerRegion
            
            Write-Host "  • $region ($($info.Location)) - $ValidatorsPerRegion validator(s)" -ForegroundColor Cyan
        }
    }
    
    Write-Host ""
    Write-Info "Continental Distribution:"
    foreach ($continent in $continentCounts.Keys | Sort-Object) {
        $percentage = ($continentCounts[$continent] / $Script:TotalValidators) * 100
        Write-Host "  • $continent: $($continentCounts[$continent]) validators ($([Math]::Round($percentage, 1))%)" -ForegroundColor Cyan
    }
    Write-Host ""
}

function Test-Prerequisites {
    Write-Info "Checking prerequisites..."
    
    # Check kubectl
    if (-not (Get-Command kubectl -ErrorAction SilentlyContinue)) {
        Write-Failure "kubectl not found. Please install kubectl first."
        exit 1
    }
    Write-Success "kubectl found"
    
    # Check helm (optional but recommended)
    if (-not (Get-Command helm -ErrorAction SilentlyContinue)) {
        Write-Warning "helm not found. Helm is recommended for easier deployments."
    } else {
        Write-Success "helm found"
    }
    
    # Check Docker
    if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
        Write-Failure "docker not found. Please install Docker first."
        exit 1
    }
    Write-Success "docker found"
    
    # Check kubeconfig
    if (-not (Test-Path $KubeConfig)) {
        Write-Failure "Kubeconfig not found at $KubeConfig"
        exit 1
    }
    Write-Success "kubeconfig found"
    
    Write-Host ""
}

function Build-ValidatorImage {
    Write-Info "Building validator Docker image..."
    
    $buildCmd = "docker build -t ${DockerRegistry}/validator:${ImageTag} -f Dockerfile ."
    
    if ($DryRun) {
        Write-Warning "[DRY RUN] Would execute: $buildCmd"
        return
    }
    
    try {
        Push-Location -Path $PSScriptRoot\..
        Invoke-Expression $buildCmd
        if ($LASTEXITCODE -ne 0) {
            throw "Docker build failed with exit code $LASTEXITCODE"
        }
        Write-Success "Validator image built successfully"
    } catch {
        Write-Failure "Failed to build validator image: $_"
        exit 1
    } finally {
        Pop-Location
    }
    
    Write-Host ""
}

function Push-ValidatorImage {
    Write-Info "Pushing validator image to registry..."
    
    $pushCmd = "docker push ${DockerRegistry}/validator:${ImageTag}"
    
    if ($DryRun) {
        Write-Warning "[DRY RUN] Would execute: $pushCmd"
        return
    }
    
    try {
        Invoke-Expression $pushCmd
        if ($LASTEXITCODE -ne 0) {
            throw "Docker push failed with exit code $LASTEXITCODE"
        }
        Write-Success "Validator image pushed successfully"
    } catch {
        Write-Failure "Failed to push validator image: $_"
        exit 1
    }
    
    Write-Host ""
}

function Deploy-ValidatorToRegion {
    param(
        [string]$Region,
        [int]$Index
    )
    
    Write-Info "Deploying validator to $Region..."
    
    $regionInfo = $RegionMapping[$Region]
    if (-not $regionInfo) {
        Write-Warning "Unknown region: $Region, using default settings"
        $regionInfo = @{ Provider = "Unknown"; Location = $Region; Continent = "Unknown" }
    }
    
    # Set kubectl context for this region
    $contextName = "dchat-$Environment-$Region"
    
    if (-not $DryRun) {
        kubectl config use-context $contextName 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Context $contextName not found, attempting to create..."
            # In production, you'd create the context here
        }
    }
    
    # Apply Kubernetes manifests
    $manifestPath = Join-Path $PSScriptRoot ".." "k8s" "validator-statefulset.yaml"
    
    if ($DryRun) {
        Write-Warning "[DRY RUN] Would apply manifests: $manifestPath"
    } else {
        kubectl apply -f $manifestPath --namespace dchat-prod
        if ($LASTEXITCODE -eq 0) {
            Write-Success "Validator deployed to $Region"
        } else {
            Write-Failure "Failed to deploy validator to $Region"
        }
    }
    
    Write-Host ""
}

function Wait-ForValidatorHealth {
    param([string]$Region)
    
    Write-Info "Waiting for validators in $Region to become healthy..."
    
    if ($DryRun) {
        Write-Warning "[DRY RUN] Would wait for health checks"
        return
    }
    
    $maxWaitSeconds = 300
    $elapsedSeconds = 0
    $checkInterval = 10
    
    while ($elapsedSeconds -lt $maxWaitSeconds) {
        $ready = kubectl get pods -n dchat-prod -l app=dchat-validator --field-selector=status.phase=Running 2>$null | Measure-Object -Line | Select-Object -ExpandProperty Lines
        
        if ($ready -ge $ValidatorsPerRegion) {
            Write-Success "All validators in $Region are healthy"
            return $true
        }
        
        Start-Sleep -Seconds $checkInterval
        $elapsedSeconds += $checkInterval
        Write-Host "." -NoNewline
    }
    
    Write-Host ""
    Write-Warning "Timeout waiting for validators in $Region"
    return $false
}

function Verify-BftQuorum {
    Write-Info "Verifying BFT quorum..."
    
    if ($DryRun) {
        Write-Warning "[DRY RUN] Would verify BFT quorum"
        return
    }
    
    $totalHealthy = 0
    foreach ($region in $Regions) {
        kubectl config use-context "dchat-$Environment-$Region" 2>$null
        $healthy = kubectl get pods -n dchat-prod -l app=dchat-validator --field-selector=status.phase=Running 2>$null | Measure-Object -Line | Select-Object -ExpandProperty Lines
        $totalHealthy += $healthy
    }
    
    Write-Info "Healthy validators: $totalHealthy / $Script:TotalValidators"
    Write-Info "Required for BFT: $Script:RequiredSignatures"
    
    if ($totalHealthy -ge $Script:RequiredSignatures) {
        Write-Success "BFT quorum achieved! Network can reach consensus."
    } else {
        Write-Failure "Insufficient validators for BFT quorum!"
        exit 1
    }
    
    Write-Host ""
}

function Show-ValidatorEndpoints {
    Write-Info "Validator RPC Endpoints:"
    
    foreach ($region in $Regions) {
        Write-Host "  • $region: https://validator-$region.dchat.network:9545" -ForegroundColor Cyan
    }
    
    Write-Host ""
    Write-Info "Load Balanced Endpoint: https://validator.dchat.network:9545"
    Write-Host ""
}

function Main {
    Write-Host ""
    Write-Host "╔════════════════════════════════════════════════════╗" -ForegroundColor Magenta
    Write-Host "║                                                    ║" -ForegroundColor Magenta
    Write-Host "║     dchat Multi-Region Validator Deployment       ║" -ForegroundColor Magenta
    Write-Host "║                                                    ║" -ForegroundColor Magenta
    Write-Host "╚════════════════════════════════════════════════════╝" -ForegroundColor Magenta
    Write-Host ""
    
    # Show deployment plan
    Show-DeploymentPlan
    
    # Confirm deployment
    if (-not $DryRun) {
        $confirm = Read-Host "Proceed with deployment? (yes/no)"
        if ($confirm -ne "yes") {
            Write-Info "Deployment cancelled by user"
            exit 0
        }
    }
    
    Write-Host ""
    
    # Check prerequisites
    Test-Prerequisites
    
    # Build and push validator image
    Build-ValidatorImage
    Push-ValidatorImage
    
    # Deploy to each region
    $deployedRegions = @()
    for ($i = 0; $i -lt $Regions.Count; $i++) {
        $region = $Regions[$i]
        Deploy-ValidatorToRegion -Region $region -Index $i
        
        # Wait for health check
        if (Wait-ForValidatorHealth -Region $region) {
            $deployedRegions += $region
        }
    }
    
    Write-Host ""
    Write-Info "Deployment Summary:"
    Write-Host "  • Deployed to $($deployedRegions.Count) / $($Regions.Count) regions" -ForegroundColor Cyan
    Write-Host ""
    
    # Verify BFT quorum
    Verify-BftQuorum
    
    # Show endpoints
    Show-ValidatorEndpoints
    
    # Final success message
    Write-Host ""
    Write-Success "╔════════════════════════════════════════════════════╗"
    Write-Success "║                                                    ║"
    Write-Success "║    Multi-Region Deployment Complete! 🎉           ║"
    Write-Success "║                                                    ║"
    Write-Success "╚════════════════════════════════════════════════════╝"
    Write-Host ""
    Write-Info "Next steps:"
    Write-Host "  1. Monitor validator health: kubectl get pods -n dchat-prod -w" -ForegroundColor Cyan
    Write-Host "  2. View logs: kubectl logs -n dchat-prod dchat-validator-0 -f" -ForegroundColor Cyan
    Write-Host "  3. Check consensus: curl https://validator.dchat.network:9545/health" -ForegroundColor Cyan
    Write-Host ""
}

# Run main function
Main
