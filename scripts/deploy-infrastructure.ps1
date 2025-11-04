#!/usr/bin/env pwsh
# Deploy infrastructure on AWS
# Usage: .\deploy-infrastructure.ps1 -Environment dev|prod

param(
    [Parameter(Mandatory=$true)]
    [ValidateSet('dev','prod')]
    [string]$Environment
)

$ErrorActionPreference = "Stop"

Write-Host "`n🚀 Deploying dchat infrastructure - $Environment`n" -ForegroundColor Cyan

# Configuration
$regions = @(
    @{Name="us-east-1"; BootstrapName="bootstrap-us-east"},
    @{Name="us-west-2"; BootstrapName="bootstrap-us-west"},
    @{Name="eu-west-1"; BootstrapName="bootstrap-eu-west"}
)

$turnRegions = @("us-east-1", "eu-west-1")

# 1. Create security groups
Write-Host "📋 Creating security groups..." -ForegroundColor Yellow

aws ec2 create-security-group `
    --group-name "dchat-bootstrap-$Environment" `
    --description "dchat bootstrap nodes"

aws ec2 authorize-security-group-ingress `
    --group-name "dchat-bootstrap-$Environment" `
    --protocol tcp --port 30303 --cidr 0.0.0.0/0

aws ec2 create-security-group `
    --group-name "dchat-turn-$Environment" `
    --description "dchat TURN servers"

aws ec2 authorize-security-group-ingress `
    --group-name "dchat-turn-$Environment" `
    --protocol tcp --port 3478 --cidr 0.0.0.0/0

aws ec2 authorize-security-group-ingress `
    --group-name "dchat-turn-$Environment" `
    --protocol udp --port 3478 --cidr 0.0.0.0/0

# 2. Launch bootstrap nodes
Write-Host "`n🌐 Launching bootstrap nodes..." -ForegroundColor Yellow

foreach ($region in $regions) {
    Write-Host "  Region: $($region.Name)" -ForegroundColor Gray
    
    $instanceId = aws ec2 run-instances `
        --region $region.Name `
        --image-id ami-0c55b159cbfafe1f0 `
        --instance-type t3.small `
        --security-groups "dchat-bootstrap-$Environment" `
        --user-data file://scripts/deploy-bootstrap-node.sh `
        --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=$($region.BootstrapName)},{Key=Environment,Value=$Environment}]" `
        --query 'Instances[0].InstanceId' --output text
    
    Write-Host "    Instance: $instanceId" -ForegroundColor Green
}

# 3. Launch TURN servers
Write-Host "`n🔄 Launching TURN servers..." -ForegroundColor Yellow

foreach ($region in $turnRegions) {
    Write-Host "  Region: $region" -ForegroundColor Gray
    
    $instanceId = aws ec2 run-instances `
        --region $region `
        --image-id ami-0c55b159cbfafe1f0 `
        --instance-type t3.small `
        --security-groups "dchat-turn-$Environment" `
        --user-data file://scripts/deploy-turn-server.sh `
        --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=turn-$region},{Key=Environment,Value=$Environment}]" `
        --query 'Instances[0].InstanceId' --output text
    
    Write-Host "    Instance: $instanceId" -ForegroundColor Green
}

# 4. Wait for instances to be running
Write-Host "`n⏳ Waiting for instances to start..." -ForegroundColor Yellow
Start-Sleep -Seconds 60

# 5. Collect endpoints
Write-Host "`n📝 Collecting endpoints..." -ForegroundColor Yellow

$bootstrapNodes = @()
foreach ($region in $regions) {
    $ip = aws ec2 describe-instances `
        --region $region.Name `
        --filters "Name=tag:Name,Values=$($region.BootstrapName)" "Name=instance-state-name,Values=running" `
        --query 'Reservations[0].Instances[0].PublicIpAddress' --output text
    
    $bootstrapNodes += "/ip4/$ip/tcp/30303"
    Write-Host "  Bootstrap: $ip" -ForegroundColor Green
}

$turnServers = @()
foreach ($region in $turnRegions) {
    $ip = aws ec2 describe-instances `
        --region $region `
        --filters "Name=tag:Name,Values=turn-$region" "Name=instance-state-name,Values=running" `
        --query 'Reservations[0].Instances[0].PublicIpAddress' --output text
    
    $turnServers += "turn:$ip:3478"
    Write-Host "  TURN: $ip" -ForegroundColor Green
}

# 6. Generate config
$config = @"
# dchat Infrastructure - $Environment
# Generated: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")

[network]
bootstrap_nodes = [
$($bootstrapNodes | ForEach-Object { "    `"$_`"" } | Join-String -Separator ",`n")
]

turn_servers = [
$($turnServers | ForEach-Object { "    `"$_`"" } | Join-String -Separator ",`n")
]

stun_servers = [
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302"
]
"@

$config | Out-File -FilePath "config.$Environment.toml" -Encoding UTF8

Write-Host "`n✅ Infrastructure deployed!" -ForegroundColor Green
Write-Host "Configuration saved to: config.$Environment.toml`n" -ForegroundColor Cyan

# 7. Display summary
Write-Host "📊 Deployment Summary:" -ForegroundColor Cyan
Write-Host "  Bootstrap Nodes: $($bootstrapNodes.Count)" -ForegroundColor White
Write-Host "  TURN Servers: $($turnServers.Count)" -ForegroundColor White
Write-Host "  Estimated Cost: `$$(($bootstrapNodes.Count + $turnServers.Count) * 0.0104 * 24 * 30)/month" -ForegroundColor Yellow
