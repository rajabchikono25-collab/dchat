#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Generate mainnet configuration files for all 7 validator servers

.DESCRIPTION
    Creates production TOML configuration files with:
    - DNS discovery for all validators
    - Validator and relay node configurations
    - Storage cluster endpoints
    - Monitoring and health check settings

.PARAMETER OutputDir
    Directory to store generated configs (default: ./mainnet-configs)

.EXAMPLE
    .\generate-mainnet-configs.ps1
#>

param(
    [string]$OutputDir = ".\mainnet-configs"
)

# Load validator keys
$keySummaryPath = ".\mainnet-keys\validator-keys-summary.json"
if (-not (Test-Path $keySummaryPath)) {
    Write-Host "❌ Validator keys not found. Run generate-mainnet-validator-keys.ps1 first." -ForegroundColor Red
    exit 1
}

$keyData = Get-Content $keySummaryPath | ConvertFrom-Json

Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  MAINNET CONFIG GENERATION" -ForegroundColor Cyan
Write-Host "============================================`n" -ForegroundColor Cyan

# Create output directory
if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    Write-Host "✓ Created config directory: $OutputDir" -ForegroundColor Green
}

# Server configurations
$servers = @(
    @{
        Region = "ohio"
        Provider = "AWS"
        Subdomain = "validator1-ohio.schikuno.top"
        PublicKey = ($keyData.validators | Where-Object { $_.region -eq "ohio" }).public_key
    },
    @{
        Region = "singapore"
        Provider = "AWS"
        Subdomain = "validator1-singapore.schikuno.top"
        PublicKey = ($keyData.validators | Where-Object { $_.region -eq "singapore" }).public_key
    },
    @{
        Region = "stockholm"
        Provider = "AWS"
        Subdomain = "validator1-stockholm.schikuno.top"
        PublicKey = ($keyData.validators | Where-Object { $_.region -eq "stockholm" }).public_key
    },
    @{
        Region = "saopaulo"
        Provider = "AWS"
        Subdomain = "validator1-saopaulo.schikuno.top"
        PublicKey = ($keyData.validators | Where-Object { $_.region -eq "saopaulo" }).public_key
    },
    @{
        Region = "india"
        Provider = "Azure"
        Subdomain = "validator1-india.schikuno.top"
        PublicKey = ($keyData.validators | Where-Object { $_.region -eq "india" }).public_key
    },
    @{
        Region = "southafrica"
        Provider = "Azure"
        Subdomain = "validator1-southafrica.schikuno.top"
        PublicKey = ($keyData.validators | Where-Object { $_.region -eq "southafrica" }).public_key
    },
    @{
        Region = "uae"
        Provider = "Azure"
        Subdomain = "validator1-uae.schikuno.top"
        PublicKey = ($keyData.validators | Where-Object { $_.region -eq "uae" }).public_key
    }
)

# All validator subdomains for DNS discovery
$allValidatorSubdomains = $servers | ForEach-Object { $_.Subdomain }

foreach ($server in $servers) {
    Write-Host "Generating config for: $($server.Region.ToUpper())" -ForegroundColor Cyan
    
    $configContent = @"
# Mainnet Production Configuration
# Server: $($server.Region.ToUpper())
# Provider: $($server.Provider)
# Subdomain: $($server.Subdomain)
# Generated: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss UTC")

[node]
role = "validator"
region = "$($server.Region)"
public_key = "$($server.PublicKey)"

[network]
# DNS-based peer discovery
dns_discovery_enabled = true
base_domain = "schikuno.top"

# Validator subdomains for discovery
validator_subdomains = [
$(($allValidatorSubdomains | ForEach-Object { "    `"$_`"" }) -join ",`n")
]

# DNS configuration
dns_servers = ["1.1.1.1", "8.8.8.8"]  # Cloudflare + Google
dns_cache_ttl_seconds = 300  # 5 minutes
dns_refresh_interval_seconds = 60  # 1 minute

[validator]
# Validator configuration
listen_address = "0.0.0.0:7070"
external_address = "$($server.Subdomain):7070"
key_file = "/etc/dchat/keys/validator.key"

# Consensus settings
min_validators = 4  # Minimum 4 of 7 for BFT
block_time_seconds = 6
max_block_size = 1048576  # 1MB

[relay]
# Relay 1
[[relay.instances]]
listen_address = "0.0.0.0:7071"
external_address = "$($server.Subdomain):7071"

# Relay 2
[[relay.instances]]
listen_address = "0.0.0.0:7072"
external_address = "$($server.Subdomain):7072"

[transport]
# P2P transport configuration
tcp_enabled = true
websocket_enabled = true
websocket_port = 443
http_port = 80

# NAT traversal
upnp_enabled = true
stun_servers = ["stun:stun.l.google.com:19302"]

[storage.redis]
mode = "cluster"
endpoints = [
$(($allValidatorSubdomains | ForEach-Object { "    `"$_:6379`"" }) -join ",`n")
]
password = ""  # Set via environment variable: REDIS_PASSWORD

[storage.minio]
mode = "distributed"
endpoints = [
$(($allValidatorSubdomains | ForEach-Object { "    `"$_:9000`"" }) -join ",`n")
]
access_key = ""  # Set via environment variable: MINIO_ACCESS_KEY
secret_key = ""  # Set via environment variable: MINIO_SECRET_KEY
bucket = "dchat-mainnet"

[storage.tikv]
# TiKV cluster (3 primary regions)
pd_endpoints = [
    "validator1-ohio.schikuno.top:2379",
    "validator1-singapore.schikuno.top:2379",
    "validator1-stockholm.schikuno.top:2379"
]

[storage.cockroachdb]
# CockroachDB cloud connection
connection_string = ""  # Set via environment variable: COCKROACH_CONNECTION_STRING
ssl_mode = "require"

[monitoring]
# Prometheus metrics
prometheus_enabled = true
prometheus_port = 9090

# Health checks
health_check_enabled = true
health_check_port = 8080
health_check_path = "/health"

[logging]
level = "info"
format = "json"
output = "/var/log/dchat/validator-$($server.Region).log"

[security]
# TLS/SSL configuration
tls_enabled = true
tls_cert_path = "/etc/dchat/certs/cert.pem"
tls_key_path = "/etc/dchat/certs/key.pem"

# Rate limiting
max_connections_per_ip = 100
rate_limit_requests_per_second = 1000

[genesis]
# Genesis block configuration
chain_id = "dchat-mainnet-1"
genesis_time = "2025-11-06T12:00:00Z"

# All validator public keys
[genesis.validators]
ohio = "$($servers[0].PublicKey)"
singapore = "$($servers[1].PublicKey)"
stockholm = "$($servers[2].PublicKey)"
saopaulo = "$($servers[3].PublicKey)"
india = "$($servers[4].PublicKey)"
southafrica = "$($servers[5].PublicKey)"
uae = "$($servers[6].PublicKey)"
"@
    
    $configPath = Join-Path $OutputDir "config-mainnet-$($server.Region).toml"
    $configContent | Out-File -FilePath $configPath -Encoding UTF8
    
    Write-Host "  ✓ Generated: $configPath" -ForegroundColor Green
}

# Generate deployment summary
$summaryContent = @"
# Mainnet Configuration Summary

**Generated**: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss UTC")
**Total Validators**: $($servers.Count)
**Network**: dchat-mainnet-1

## Validator Configurations

| Region | Provider | Subdomain | Public Key |
|--------|----------|-----------|------------|
$(($servers | ForEach-Object { "| $($_.Region) | $($_.Provider) | $($_.Subdomain) | ``$($_.PublicKey)`` |" }) -join "`n")

## DNS Discovery Configuration

- **Base Domain**: schikuno.top
- **DNS Servers**: Cloudflare (1.1.1.1), Google (8.8.8.8)
- **Cache TTL**: 5 minutes
- **Refresh Interval**: 60 seconds

## Validator Subdomains

$(($allValidatorSubdomains | ForEach-Object { "- $($_)" }) -join "`n")

## Network Ports

### Validators
- **TCP**: 7070
- **WebSocket**: 443
- **HTTP**: 80

### Relays (per validator)
- **Relay 1**: 7071
- **Relay 2**: 7072

### Monitoring
- **Prometheus**: 9090
- **Health Check**: 8080

### Storage Clusters
- **Redis**: 6379
- **MinIO**: 9000
- **TiKV PD**: 2379
- **TiKV Server**: 20160

## Consensus Configuration

- **Minimum Validators**: 4 of 7 (BFT)
- **Block Time**: 6 seconds
- **Max Block Size**: 1 MB

## Storage Configuration

### Redis Cluster
- Mode: Cluster
- Nodes: 7 (one per validator)
- Port: 6379

### MinIO Distributed
- Mode: Distributed
- Nodes: 7 (one per validator)
- Port: 9000

### TiKV Cluster
- PD Nodes: 3 (Ohio, Singapore, Stockholm)
- Port: 2379 (PD), 20160 (Server)

### CockroachDB
- Type: Cloud-managed
- Connection: TLS required

## Deployment Order

1. **Ohio** (AWS US East) - First validator
2. **Singapore** (AWS Asia Pacific) - Second validator
3. **Stockholm** (AWS Europe) - Third validator
4. **São Paulo** (AWS South America) - Fourth validator (consensus reached)
5. **India** (Azure Central India) - Fifth validator
6. **South Africa** (Azure South Africa North) - Sixth validator
7. **UAE** (Azure UAE North) - Seventh validator

## Environment Variables Required

Each server needs these environment variables set:

\`\`\`bash
# Redis password
export REDIS_PASSWORD="<secure-password>"

# MinIO credentials
export MINIO_ACCESS_KEY="<access-key>"
export MINIO_SECRET_KEY="<secret-key>"

# CockroachDB connection
export COCKROACH_CONNECTION_STRING="<connection-string>"
\`\`\`

## Configuration Files Generated

$(($servers | ForEach-Object { "- config-mainnet-$($_.Region).toml" }) -join "`n")

## Next Steps

1. Review all configuration files
2. Set environment variables on each server
3. Copy config files to servers: \`/etc/dchat/config.toml\`
4. Copy validator keys to servers: \`/etc/dchat/keys/validator.key\`
5. Start validators one-by-one using deployment script
6. Monitor consensus formation

---

**Generated**: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss UTC")
"@

$summaryPath = Join-Path $OutputDir "MAINNET-CONFIG-SUMMARY.md"
$summaryContent | Out-File -FilePath $summaryPath -Encoding UTF8

Write-Host "`n✓ Generated summary: $summaryPath" -ForegroundColor Green

Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  CONFIG GENERATION COMPLETE" -ForegroundColor Green
Write-Host "============================================`n" -ForegroundColor Cyan

Write-Host "Generated Configurations:" -ForegroundColor White
foreach ($server in $servers) {
    Write-Host "  ✓ $($server.Region.PadRight(15)) - config-mainnet-$($server.Region).toml" -ForegroundColor Green
}

Write-Host "`n📚 Documentation:" -ForegroundColor White
Write-Host "  • Summary: $summaryPath" -ForegroundColor Gray
Write-Host "  • Deployment: .\MAINNET_DEPLOYMENT_CHECKLIST.md" -ForegroundColor Gray

Write-Host "`n🚀 Ready for deployment!" -ForegroundColor Green
Write-Host ""
