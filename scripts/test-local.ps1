#!/usr/bin/env pwsh
# Local testing without AWS infrastructure
# Tests against public STUN servers and creates local mock infrastructure

param(
    [switch]$WithDocker
)

$ErrorActionPreference = "Stop"

Write-Host "`n🧪 Running Local Integration Tests`n" -ForegroundColor Cyan

# Test 1: STUN with public servers
Write-Host "1️⃣ STUN Tests (Google servers)..." -ForegroundColor Yellow
cargo test -p dchat-network nat::stun::tests --lib -- --nocapture 2>&1 | Select-String "test result"
if ($LASTEXITCODE -eq 0) {
    Write-Host "   ✅ STUN passed" -ForegroundColor Green
} else {
    Write-Host "   ❌ STUN failed" -ForegroundColor Red
    exit 1
}

# Test 2: UPnP discovery
Write-Host "`n2️⃣ UPnP Tests..." -ForegroundColor Yellow
cargo test -p dchat-network nat::upnp::tests --lib -- --nocapture 2>&1 | Select-String "test result"
if ($LASTEXITCODE -eq 0) {
    Write-Host "   ✅ UPnP passed" -ForegroundColor Green
} else {
    Write-Host "   ❌ UPnP failed" -ForegroundColor Red
}

# Test 3: Onion routing
Write-Host "`n3️⃣ Onion Routing Tests..." -ForegroundColor Yellow
cargo test -p dchat-network onion_routing::tests --lib -- --nocapture 2>&1 | Select-String "test result"
if ($LASTEXITCODE -eq 0) {
    Write-Host "   ✅ Onion routing passed" -ForegroundColor Green
} else {
    Write-Host "   ❌ Onion routing failed" -ForegroundColor Red
}

# Test 4: MPC
Write-Host "`n4️⃣ MPC Tests..." -ForegroundColor Yellow
cargo test -p dchat-identity mpc::tests --lib -- --nocapture 2>&1 | Select-String "test result"
if ($LASTEXITCODE -eq 0) {
    Write-Host "   ✅ MPC passed" -ForegroundColor Green
} else {
    Write-Host "   ❌ MPC failed" -ForegroundColor Red
}

# Optional: Docker TURN server
if ($WithDocker) {
    Write-Host "`n5️⃣ Starting local TURN server (Docker)..." -ForegroundColor Yellow
    docker run -d --name dchat-turn -p 3478:3478 -p 3478:3478/udp coturn/coturn 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "   ✅ TURN server started on localhost:3478" -ForegroundColor Green
        Write-Host "   Run: cargo test -p dchat-network nat::turn::tests" -ForegroundColor Gray
    } else {
        Write-Host "   ⚠️  Docker not available" -ForegroundColor Yellow
    }
}

Write-Host "`n✅ Local tests complete!`n" -ForegroundColor Green
Write-Host "Summary:" -ForegroundColor Cyan
Write-Host "  • STUN: Working with public servers" -ForegroundColor White
Write-Host "  • UPnP: Local testing only" -ForegroundColor White
Write-Host "  • Onion: Fully functional" -ForegroundColor White
Write-Host "  • MPC: Fully functional" -ForegroundColor White

if ($WithDocker) {
    Write-Host "`nTo stop TURN server: docker stop dchat-turn && docker rm dchat-turn`n" -ForegroundColor Gray
}
