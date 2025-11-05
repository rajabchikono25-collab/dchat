#!/usr/bin/env pwsh
# Quick start script for Redis (Windows/PowerShell)
# Starts Redis server for local development

param(
    [switch]$Stop,
    [switch]$Clean,
    [switch]$Status,
    [switch]$Cli
)

$ErrorActionPreference = "Stop"

# Colors for output
function Write-ColorOutput($ForegroundColor) {
    $fc = $host.UI.RawUI.ForegroundColor
    $host.UI.RawUI.ForegroundColor = $ForegroundColor
    if ($args) {
        Write-Output $args
    }
    $host.UI.RawUI.ForegroundColor = $fc
}

Write-ColorOutput Green "=================================="
Write-ColorOutput Green "dchat Redis Quick Start"
Write-ColorOutput Green "=================================="

# Check status
if ($Status) {
    Write-ColorOutput Cyan "`nChecking Redis status..."
    
    docker ps --filter "name=dchat-redis" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
    
    Write-ColorOutput Cyan "`nRedis Health Check:"
    try {
        $response = docker exec dchat-redis redis-cli PING 2>&1
        if ($response -eq "PONG") {
            Write-ColorOutput Green "Redis is responding: $response"
        } else {
            Write-ColorOutput Yellow "Redis returned unexpected response: $response"
        }
    } catch {
        Write-ColorOutput Yellow "Redis is not responding or container not running"
    }
    
    Write-ColorOutput Cyan "`nRedis Info:"
    try {
        docker exec dchat-redis redis-cli INFO server | Select-String "redis_version|uptime_in_seconds|connected_clients"
    } catch {
        Write-ColorOutput Yellow "Could not retrieve Redis info"
    }
    
    exit 0
}

# Open Redis CLI
if ($Cli) {
    Write-ColorOutput Cyan "`nOpening Redis CLI..."
    docker exec -it dchat-redis redis-cli
    exit 0
}

# Stop Redis
if ($Stop) {
    Write-ColorOutput Yellow "`nStopping Redis..."
    docker-compose -f docker-compose-testnet.yml stop redis
    Write-ColorOutput Green "Redis stopped successfully"
    exit 0
}

# Clean Redis data
if ($Clean) {
    Write-ColorOutput Red "`n⚠️  WARNING: This will DELETE all Redis data!"
    $confirm = Read-Host "Type 'yes' to continue"
    
    if ($confirm -ne "yes") {
        Write-ColorOutput Yellow "Clean cancelled"
        exit 0
    }
    
    Write-ColorOutput Yellow "`nStopping Redis..."
    docker-compose -f docker-compose-testnet.yml stop redis
    
    Write-ColorOutput Yellow "Removing container..."
    docker-compose -f docker-compose-testnet.yml rm -f redis
    
    Write-ColorOutput Yellow "Removing volume..."
    docker volume rm dchat_redis_data -f 2>$null
    
    Write-ColorOutput Green "Redis data cleaned successfully"
    exit 0
}

# Start Redis
Write-ColorOutput Cyan "`nStarting Redis server..."

# Check if Docker is running
try {
    docker ps | Out-Null
} catch {
    Write-ColorOutput Red "❌ Error: Docker is not running. Please start Docker Desktop."
    exit 1
}

# Check if docker-compose-testnet.yml exists
if (-not (Test-Path "docker-compose-testnet.yml")) {
    Write-ColorOutput Red "❌ Error: docker-compose-testnet.yml not found"
    exit 1
}

# Start Redis
docker-compose -f docker-compose-testnet.yml up -d redis

Write-ColorOutput Cyan "Waiting for Redis to be ready..."
$maxAttempts = 10
$attempt = 0
$redisReady = $false

while ($attempt -lt $maxAttempts -and -not $redisReady) {
    Start-Sleep -Seconds 1
    try {
        $response = docker exec dchat-redis redis-cli PING 2>&1
        if ($response -eq "PONG") {
            $redisReady = $true
            Write-ColorOutput Green "✓ Redis is ready"
        }
    } catch {
        $attempt++
        Write-Host "." -NoNewline
    }
}

if (-not $redisReady) {
    Write-ColorOutput Red "`n❌ Error: Redis failed to start within 10 seconds"
    Write-ColorOutput Yellow "Check logs with: docker-compose -f docker-compose-testnet.yml logs redis"
    exit 1
}

# Display connection info
Write-ColorOutput Green "`n=================================="
Write-ColorOutput Green "✓ Redis Started Successfully"
Write-ColorOutput Green "=================================="

Write-ColorOutput Cyan "`nConnection Information:"
Write-Output "  URL:      redis://localhost:6379"
Write-Output "  Host:     localhost"
Write-Output "  Port:     6379"
Write-Output "  Password: (none - development only)"

Write-ColorOutput Cyan "`nConfiguration (testnet-config.toml):"
Write-ColorOutput White @"
[storage.redis]
url = "redis://localhost:6379"
pool_size = 20
connection_timeout_secs = 5
"@

Write-ColorOutput Cyan "`nUse Cases:"
Write-Output "  • Channel metadata cache"
Write-Output "  • User online status (presence)"
Write-Output "  • Rate limiting counters"
Write-Output "  • Session tokens"
Write-Output "  • Message delivery queues"
Write-Output "  • Relay discovery cache"

Write-ColorOutput Cyan "`nUseful Commands:"
Write-Output "  Status:    .\start-redis.ps1 -Status"
Write-Output "  CLI:       .\start-redis.ps1 -Cli"
Write-Output "  Stop:      .\start-redis.ps1 -Stop"
Write-Output "  Clean:     .\start-redis.ps1 -Clean"
Write-Output "  Logs:      docker-compose -f docker-compose-testnet.yml logs -f redis"
Write-Output "  Monitor:   docker exec dchat-redis redis-cli MONITOR"

Write-ColorOutput Cyan "`nQuick Test:"
Write-ColorOutput White @"
# Set a key
docker exec dchat-redis redis-cli SET test_key "Hello dchat"

# Get the key
docker exec dchat-redis redis-cli GET test_key

# Check connected clients
docker exec dchat-redis redis-cli CLIENT LIST
"@

Write-ColorOutput Green "`n✓ Ready for development!"
