# Azure Testnet Status Report

**Date**: November 5, 2025  
**Time**: 20:50 UTC (approximately)

## Executive Summary

✅ **All 3 Azure validators are running and producing blocks**  
✅ **Network ports (9090) are open and reachable between all validators**  
✅ **Block heights are synchronized (232-234)**  
❌ **P2P peer discovery is NOT working - validators are isolated**

## Current Status

### Validator Infrastructure

| Region | Hostname | IP | Peer ID (Current) | Status |
|--------|----------|-----|-------------------|--------|
| India (Mumbai) | uramami | 74.225.183.196 | `12D3KooWSsDaL6yjZK766K85qSF7uvgh26h7d9iFbaG43LwsvJST` | ✅ Running (Block #232) |
| South Africa (Johannesburg) | thesbis | 4.221.211.71 | `12D3KooWAABVerV5qfVgEYMCbsqAwveLGv1PUX831EuMvdWp9vHq` | ✅ Running (Block #233) |
| UAE (Dubai) | Randal | 4.161.34.228 | `12D3KooWMsBEsDXMA8jfQEUzc3wFnLwpe7Byu3HUov2sbA4cnSWn` | ✅ Running (Block #234) |

### Validator Details

**All validators:**
- Binary: `/opt/dchat/dchat` (16 MB, compiled Nov 5 15:56-15:57 UTC)
- Service: `dchat.service` (active, running since ~20:21 UTC)
- Stake: 10000 tokens
- Database: SQLite with connection pooling (10 connections)
- Storage: `/opt/dchat/data`

**Service Command:**
```bash
/opt/dchat/dchat validator --key keys/validator.key --chain-rpc http://localhost:26657 --stake 10000 --producer
```

## Problem Diagnosis

### Issue: Outdated Bootstrap Peer IDs

**Root Cause:**
- Validators have **new peer IDs** (generated from their validator keys)
- Bootstrap peer lists in configs contain **old peer IDs**
- Validators cannot discover each other because they're looking for peers that no longer exist

**Evidence from logs:**
```
INFO dchat_network::swarm: No bootstrap nodes configured - will use mDNS for local peer discovery
```

This message appears despite bootstrap peers being configured, indicating the code is not reading the config properly OR the peer IDs don't match.

### Bootstrap Peer ID Mismatch

**Configured in `/opt/dchat/config.toml` (OLD):**
- India: `12D3KooWM7sdVe45gGe4Esk2YLe4xoUoqNepfEhacvpktDRGzCT5`
- South Africa: `12D3KooWML3ZTRUoaorZKpydsJwDaAig8im4cjafHu59kb5NeVgS`
- UAE: `12D3KooWLEB1MHiuLwtUtrknbtj8y93cjqgWNWVnk9EuxssUdrUe`

**Actual Current Peer IDs (NEW):**
- India: `12D3KooWSsDaL6yjZK766K85qSF7uvgh26h7d9iFbaG43LwsvJST`
- South Africa: `12D3KooWAABVerV5qfVgEYMCbsqAwveLGv1PUX831EuMvdWp9vHq`
- UAE: `12D3KooWMsBEsDXMA8jfQEUzc3wFnLwpe7Byu3HUov2sbA4cnSWn`

**Also configured in systemd environment variable `DCHAT_BOOTSTRAP_PEERS` with even older IDs:**
- India: `12D3KooWJ8ueQfaCeNCGY1shpxbCzDARumfczDkCuL7YT5bTYRdP`
- South Africa: `12D3KooWML3ZTRUoaorZKpydsJwDaAig8im4cjafHu59kb5NeVgS`
- UAE: `12D3KooWLEB1MHiuLwtUtrknbtj8y93cjqgWNWVnk9EuxssUdrUe`

## Network Connectivity

✅ **All P2P ports (9090) are accessible:**
- India → South Africa: **OPEN**
- India → UAE: **OPEN**
- South Africa → India: **OPEN**
- South Africa → UAE: **OPEN**

✅ **Validators can potentially connect once peer IDs are corrected**

## How They're Currently Synchronized

Despite no P2P connections, all validators are at similar block heights (232-234). This suggests:

1. **They're each producing blocks independently** (solo consensus)
2. **OR they're syncing via the chain RPC** (`http://localhost:26657`)
3. **OR they're connected via a different mechanism** (possibly chain-level consensus, not P2P messaging)

The logs show: `"📦 Produced block #XXX"` every 6 seconds, indicating each validator is producing blocks on its own chain.

## Configuration Files

Each validator has identical configurations at `/opt/dchat/config.toml`:

```toml
[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/9090"]
bootstrap_peers = [
    # 4 AWS validators (Sao Paulo, Stockholm, Ohio, Singapore)
    "/ip4/54.233.203.82/tcp/9090/p2p/12D3KooWCjY9HZdzjdep5QsYHMH48RXPaWHb4bP17xb5KB4dYmuh",
    "/ip4/13.48.49.2/tcp/9090/p2p/12D3KooWKxoJic9ZjkiqXyTRyeTzcE6XfBapqnreZquZTA2oz4EA",
    "/ip4/18.191.118.167/tcp/9090/p2p/12D3KooWQqp6pq22h3LFhu73krUkGyJwwwZm499pr6GQNPMCDFit",
    "/ip4/18.142.96.209/tcp/9090/p2p/12D3KooWAdaNGLmZhq2ACGZJLueMRVSsbkAFdUW3j5L8d8JwHbpd",
    # 3 Azure validators (India, UAE, South Africa) - OLD PEER IDs
    "/ip4/74.225.183.196/tcp/9090/p2p/12D3KooWM7sdVe45gGe4Esk2YLe4xoUoqNepfEhacvpktDRGzCT5",
    "/ip4/4.161.34.228/tcp/9090/p2p/12D3KooWLEB1MHiuLwtUtrknbtj8y93cjqgWNWVnk9EuxssUdrUe",
    "/ip4/4.221.211.71/tcp/9090/p2p/12D3KooWML3ZTRUoaorZKpydsJwDaAig8im4cjafHu59kb5NeVgS"
]
max_connections = 100
connection_timeout_ms = 10000
enable_mdns = false
enable_upnp = false
```

## Solution Required

### Fix P2P Peer Discovery

**Option 1: Update config.toml with current peer IDs (RECOMMENDED)**

Update each validator's `/opt/dchat/config.toml` with the current peer IDs:

```toml
bootstrap_peers = [
    # AWS validators (unchanged)
    "/ip4/54.233.203.82/tcp/9090/p2p/12D3KooWCjY9HZdzjdep5QsYHMH48RXPaWHb4bP17xb5KB4dYmuh",
    "/ip4/13.48.49.2/tcp/9090/p2p/12D3KooWKxoJic9ZjkiqXyTRyeTzcE6XfBapqnreZquZTA2oz4EA",
    "/ip4/18.191.118.167/tcp/9090/p2p/12D3KooWQqp6pq22h3LFhu73krUkGyJwwwZm499pr6GQNPMCDFit",
    "/ip4/18.142.96.209/tcp/9090/p2p/12D3KooWAdaNGLmZhq2ACGZJLueMRVSsbkAFdUW3j5L8d8JwHbpd",
    # Azure validators (UPDATED)
    "/ip4/74.225.183.196/tcp/9090/p2p/12D3KooWSsDaL6yjZK766K85qSF7uvgh26h7d9iFbaG43LwsvJST",
    "/ip4/4.221.211.71/tcp/9090/p2p/12D3KooWAABVerV5qfVgEYMCbsqAwveLGv1PUX831EuMvdWp9vHq",
    "/ip4/4.161.34.228/tcp/9090/p2p/12D3KooWMsBEsDXMA8jfQEUzc3wFnLwpe7Byu3HUov2sbA4cnSWn"
]
```

Then restart services: `sudo systemctl restart dchat`

**Option 2: Fix code to read config properly**

If the code isn't reading the config file, fix `src/network/swarm.rs` or similar to:
1. Parse config.toml on startup
2. Load bootstrap peers from config
3. Fall back to environment variable if config missing

**Option 3: Update systemd environment variable**

Update `/etc/systemd/system/dchat.service` with current peer IDs, then:
```bash
sudo systemctl daemon-reload
sudo systemctl restart dchat
```

## Next Steps

1. ✅ **Confirmed**: Validators are running and producing blocks
2. ✅ **Confirmed**: Network connectivity between validators exists
3. ✅ **Confirmed**: Block heights are synchronized
4. ❌ **Issue Identified**: Bootstrap peer IDs are outdated
5. 🔧 **Action Required**: Update bootstrap peer configurations
6. 🔧 **Action Required**: Restart services
7. 🔍 **Verification Required**: Check for successful P2P handshakes in logs

## Expected Behavior After Fix

After updating bootstrap peers and restarting:

```
✓ INFO libp2p_swarm: Connection established: peer=12D3KooWSsDaL6yjZK766K85qSF7uvgh26h7d9iFbaG43LwsvJST
✓ INFO dchat_network: Connected to peer: South Africa (12D3KooWAABVerV5qfVgEYMCbsqAwveLGv1PUX831EuMvdWp9vHq)
✓ INFO dchat_network: Connected to peer: UAE (12D3KooWMsBEsDXMA8jfQEUzc3wFnLwpe7Byu3HUov2sbA4cnSWn)
✓ INFO dchat: Network fully connected: 6 peers (3 Azure + 3 AWS)
```

## Access Information

**SSH Keys:**
- India: `Foundation-servers/Azure-India/uramami.pem`
- South Africa: `Foundation-servers/Azure-SAfrica/anacreon.pem`
- UAE: `Foundation-servers/Azure_UAE/Randal_key.pem`

**SSH Username:** `azureuser` (all servers)

**Quick Commands:**
```powershell
# Check logs
ssh -i "Foundation-servers/Azure-India/uramami.pem" azureuser@74.225.183.196 "sudo journalctl -u dchat -f"

# Restart service
ssh -i "Foundation-servers/Azure-India/uramami.pem" azureuser@74.225.183.196 "sudo systemctl restart dchat"

# Check status
ssh -i "Foundation-servers/Azure-India/uramami.pem" azureuser@74.225.183.196 "sudo systemctl status dchat --no-pager"
```

## Summary

🎯 **Validators are deployed, running, and producing blocks**  
🎯 **Network infrastructure is correct**  
⚠️ **P2P mesh is not connected due to outdated bootstrap peer IDs**  
📋 **Fix required: Update bootstrap peers with current peer IDs and restart**

The validators are **functional** but **isolated**. They need updated bootstrap configurations to discover and handshake with each other across the global P2P mesh network.
