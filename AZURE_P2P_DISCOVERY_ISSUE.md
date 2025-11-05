# Azure Validators P2P Discovery Issue - Root Cause Analysis

**Date**: November 5, 2025, 23:30 UTC

## Current Status

### Validators Running
- ✅ **South Africa (Johannesburg)**: Running, producing blocks (#12+)
  - IP: 4.221.211.71
  - Peer ID: `12D3KooWGsHuCA4461BEEZkFymh1FjQdqsQrocUJZfEmn4naWEQ2`
  
- ✅ **UAE (Dubai)**: Running, producing blocks (#12+)
  - IP: 4.221.34.228
  - Peer ID: `12D3KooWQSK3Zrcd68Q2azEczqX9xENqgGtAmPC6Kqyc8TtL2GJP`
  
- ⚠️ **India (Mumbai)**: Temporarily unreachable (SSH connection timeout)
  - IP: 74.225.183.196
  - Last known Peer ID: `12D3KooWSsDaL6yjZK766K85qSF7uvgh26h7d9iFbaG43LwsvJST`

### Configuration Status
✅ Bootstrap peers have been updated in `/opt/dchat/config.toml` on all validators
✅ Config files are valid TOML and load successfully
✅ Validators start and produce blocks

### P2P Network Status
❌ **Validators are NOT discovering each other**
❌ **No P2P mesh connections established**
❌ **Each validator runs in isolation**

## Root Cause Identified

### The Problem
The code successfully loads `config.toml`, but **does not use the `bootstrap_peers` configuration** when initializing the libp2p network.

### Evidence

**From South Africa logs (23:26:50 UTC):**
```
INFO dchat: Loading config from "config.toml"
INFO dchat: ✓ Configuration loaded successfully
...
INFO dchat_network::swarm: No bootstrap nodes configured - will use mDNS for local peer discovery
    at crates/dchat-network/src/swarm.rs:138
```

**Analysis:**
1. Config loads: ✅ `"✓ Configuration loaded successfully"`
2. Network init happens at: `crates/dchat-network/src/swarm.rs:138`
3. Network code reports: `"No bootstrap nodes configured"`
4. This means: **The bootstrap_peers from config.toml are NOT being passed to the network initialization**

### Code Location
**File**: `crates/dchat-network/src/swarm.rs`  
**Line**: ~138  
**Issue**: The `new()` or `init()` function for the swarm is not receiving/using the bootstrap peers from the loaded config

## Technical Details

### What's Happening
1. `main.rs:1272` - Loads config.toml ✅
2. `main.rs` - Parses network configuration ✅
3. `main.rs:1901` - Initializes validator network
4. `dchat-network/src/swarm.rs:138` - Creates libp2p swarm
5. ❌ **Bootstrap peers are not passed/used** during swarm creation

### Expected vs Actual Behavior

**Expected:**
```rust
// In swarm initialization
if !bootstrap_peers.is_empty() {
    for peer_addr in bootstrap_peers {
        swarm.dial(peer_addr)?;
    }
    info!("Configured {} bootstrap peers", bootstrap_peers.len());
}
```

**Actual:**
```rust
// Current code (line 138)
info!("No bootstrap nodes configured - will use mDNS for local peer discovery");
```

This message appears UNCONDITIONALLY, meaning:
- Either `bootstrap_peers` is empty when it reaches the network code
- OR the network code is not checking for bootstrap peers at all

## Solution Required

### Option 1: Fix Network Initialization (RECOMMENDED)
Modify `crates/dchat-network/src/swarm.rs` to:
1. Accept bootstrap peers as a parameter
2. Dial each bootstrap peer on startup
3. Log the number of bootstrap peers configured

**Files to modify:**
- `crates/dchat-network/src/swarm.rs` - Add bootstrap peer dialing
- `src/main.rs` - Pass config.network.bootstrap_peers to network init

### Option 2: Use Environment Variable (WORKAROUND)
The systemd service has `DCHAT_BOOTSTRAP_PEERS` environment variable, but:
- Peer IDs in env var are also outdated
- This approach is less maintainable than using config.toml

### Option 3: Command-Line Arguments (TEMPORARY)
Add `--bootstrap-peer` flags to the systemd ExecStart command:
```bash
ExecStart=/opt/dchat/dchat validator \
  --key keys/validator.key \
  --chain-rpc http://localhost:26657 \
  --stake 10000 \
  --producer \
  --bootstrap-peer /ip4/74.225.183.196/tcp/9090/p2p/12D3KooWGsHuCA4461BEEZkFymh1FjQdqsQrocUJZfEmn4naWEQ2 \
  --bootstrap-peer /ip4/4.221.211.71/tcp/9090/p2p/12D3KooWQSK3Zrcd68Q2azEczqX9xENqgGtAmPC6Kqyc8TtL2GJP
  # ... etc
```

## Code Changes Required

### Minimal Fix (crates/dchat-network/src/swarm.rs)

**Before (line ~138):**
```rust
info!("No bootstrap nodes configured - will use mDNS for local peer discovery");
```

**After:**
```rust
if bootstrap_peers.is_empty() {
    info!("No bootstrap nodes configured - will use mDNS for local peer discovery");
} else {
    info!("Configuring {} bootstrap peers", bootstrap_peers.len());
    for peer_addr in bootstrap_peers {
        match swarm.dial(peer_addr.clone()) {
            Ok(_) => debug!("Dialing bootstrap peer: {}", peer_addr),
            Err(e) => warn!("Failed to dial bootstrap peer {}: {}", peer_addr, e),
        }
    }
}
```

### Main.rs Integration

Ensure `src/main.rs` passes bootstrap peers when creating the network:

```rust
// Around line 1901 (validator network initialization)
let network = dchat_network::create_validator_network(
    validator_key,
    config.network.listen_addresses,
    config.network.bootstrap_peers,  // ← Make sure this is passed!
    config.network.max_connections,
)?;
```

## Testing After Fix

Once the code is fixed and redeployed:

1. **Restart validators**
2. **Check logs for**:
   ```
   INFO dchat_network::swarm: Configuring 6 bootstrap peers
   DEBUG dchat_network: Dialing bootstrap peer: /ip4/74.225.183.196/tcp/9090/p2p/...
   INFO libp2p_swarm: Connection established: peer=12D3KooW...
   ```

3. **Verify mesh connectivity**:
   - Each validator should connect to 2-6 peers (other Azure + AWS validators)
   - Use `journalctl -u dchat -f | grep -i "connect\|peer"`

## Workaround Until Code Fix

Since the code fix requires recompilation and redeployment, and we currently have build errors (18 compilation errors in main.rs), the validators will continue running in isolation mode.

**Current behavior**:
- Each validator produces its own blocks
- No cross-validator consensus
- No P2P message routing
- Each chain is independent

**Impact**:
- Can't test multi-validator consensus
- Can't test message propagation across nodes
- Can't verify Byzantine fault tolerance
- Network is effectively 3 separate single-node testnets

## Next Steps

1. ✅ **Configurations updated** - Bootstrap peers in config.toml are current
2. ⏳ **Fix compilation errors** - Resolve 18 errors in main.rs (database, governance, crypto modules)
3. ⏳ **Fix network initialization** - Make swarm.rs use bootstrap peers from config
4. ⏳ **Rebuild binary** - `cargo build --release --bin dchat`
5. ⏳ **Redeploy to validators** - Update all 3 Azure validators with fixed binary
6. ⏳ **Verify P2P mesh** - Check logs for successful peer connections

## Summary

**Infrastructure**: ✅ Ready (servers, configs, SSH access)  
**Configuration**: ✅ Ready (bootstrap peers updated)  
**Code**: ❌ **Broken** (network init doesn't use config, plus 18 compilation errors)  
**Deployment**: ⏳ Blocked until code is fixed

The validators are running and stable, but isolated. P2P mesh requires code fixes before it will work.
