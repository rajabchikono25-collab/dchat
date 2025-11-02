# Gossipsub Mesh Zero Peers Fix

## Problem Identified
The testnet showed all containers as "healthy" but the gossipsub mesh had 0 peers. Users could send messages with 0 peers, which shouldn't be possible.

### Root Cause
1. **Missing Bootstrap Flags**: All user and relay nodes had `DCHAT_BOOTSTRAP_PEERS` environment variables set, but the actual CLI commands were missing `--bootstrap` flags
2. **Incorrect Format**: Initial fix attempted used `hostname:port` format, but libp2p requires full **multiaddr format**: `/dns4/hostname/tcp/port`
3. **Code Dependency**: The `run_user_node()` and relay code only processes bootstrap peers from the CLI `--bootstrap` flags, NOT from environment variables

## Changes Applied

### 1. User Nodes (user1, user2, user3)
**Before:**
```bash
command: user --username user1 --non-interactive --health-addr 0.0.0.0:8080 --metrics-addr 0.0.0.0:9110
```

**After:**
```bash
command: user --username user1 --non-interactive --bootstrap /dns4/relay1/tcp/7080 --bootstrap /dns4/relay2/tcp/7080 --bootstrap /dns4/relay3/tcp/7080 --health-addr 0.0.0.0:8080 --metrics-addr 0.0.0.0:9110
```

- **user1**: Bootstraps to relay1, relay2, relay3
- **user2**: Bootstraps to relay4, relay5, relay1
- **user3**: Bootstraps to relay6, relay7, relay2

### 2. Relay Nodes (relay1-7)
**Before:**
```bash
command: relay --listen 0.0.0.0:7080 --stake 1000
```

**After (example relay1):**
```bash
command: relay --listen 0.0.0.0:7080 --bootstrap /dns4/validator1/tcp/7070 --bootstrap /dns4/validator2/tcp/7070 --bootstrap /dns4/validator3/tcp/7070 --stake 1000
```

All 7 relays now bootstrap to multiple validators:
- **relay1**: validator1, validator2, validator3
- **relay2**: validator1, validator4, relay1
- **relay3**: validator2, validator3, relay1
- **relay4**: validator2, validator4, relay2
- **relay5**: validator3, validator1, relay3
- **relay6**: validator4, validator2, relay4
- **relay7**: validator1, validator3, relay5

### 3. Validator Nodes
No changes needed - validators discover each other through the consensus layer (Tendermint), not libp2p gossipsub.

## Technical Details

### Multiaddr Format
libp2p requires addresses in **multiaddr format**, not simple `hostname:port`:
- ✅ Correct: `/dns4/relay1/tcp/7080`
- ❌ Incorrect: `relay1:7080`

The parsing happens in `src/main.rs` at line 1416:
```rust
match peer_addr.parse::<Multiaddr>() {
    Ok(multiaddr) => {
        let peer_id = PeerId::random();
        network_config.discovery.bootstrap_nodes.push((peer_id, multiaddr));
        info!("✓ Added bootstrap node: {}", peer_addr);
    }
    Err(e) => warn!("⚠ Invalid multiaddr {}: {}", peer_addr, e),
}
```

### Bootstrap Process
From `src/main.rs` lines 1410-1460:
1. Parse CLI `--bootstrap` flags into `Vec<String>`
2. Convert each to `Multiaddr` format
3. Add to `network_config.discovery.bootstrap_nodes`
4. Call `network.start()` to initiate connections
5. Wait 15 seconds for peer connections
6. Log each successful connection: "✓ Peer connected: {peer_id} (total: N)"

## Deployment Instructions

### Step 1: Rebuild Docker Image
The binary needs to be rebuilt with health server support:
```bash
docker compose -f docker-compose-testnet.yml build
```

### Step 2: Upload Updated Files to Server
```powershell
scp -i C:\Users\USER\Downloads\anacreon.pem docker-compose-testnet.yml azureuser@4.221.211.71:~/dchat/
```

### Step 3: Restart Containers
SSH to server and restart:
```bash
ssh -i C:\Users\USER\Downloads\anacreon.pem azureuser@4.221.211.71
cd dchat
docker compose -f docker-compose-testnet.yml down
docker compose -f docker-compose-testnet.yml up -d
```

### Step 4: Verify Mesh Formation
Check logs for successful peer connections:
```bash
# User node logs
docker logs dchat-user1 2>&1 | grep -E "Bootstrap|Peer connected|mesh"

# Relay node logs
docker logs dchat-relay1 2>&1 | grep -E "Bootstrap|Peer connected|mesh"

# Expected output:
# Bootstrap peers provided: ["/dns4/relay1/tcp/7080", ...]
# ✓ Added bootstrap node: /dns4/relay1/tcp/7080
# ✓ Peer connected: 12D3KooW... (total: 1)
# ✓ Bootstrap complete, 3 peer(s) connected
```

### Step 5: Test Message Sending
After mesh formation, verify messages propagate:
```bash
# Attach to user1
docker attach dchat-user1

# Send test message (should now work with peers)
/global Hello from user1 with proper mesh!
```

## Expected Behavior After Fix

### User Nodes
- Should connect to 3 relay nodes each
- Mesh peer count should be 3-6 (depending on gossipsub mesh size)
- Can send messages only after mesh formation
- Logs should show: "✓ Bootstrap complete, N peer(s) connected"

### Relay Nodes
- Should connect to 2-3 validators + 1-2 other relays
- Act as intermediaries in the gossipsub mesh
- Forward messages between users and validators
- Should show higher peer counts (5-10 peers)

### Network Topology
```
validator1 ←→ validator2 ←→ validator3 ←→ validator4
    ↑             ↑             ↑             ↑
    |             |             |             |
relay1 ←→ relay2 ←→ relay3 ←→ relay4 ←→ relay5 ←→ relay6 ←→ relay7
    ↑       ↑       ↑       ↑       ↑       ↑       ↑
    |       |       |       |       |       |       |
user1      user2          user3
```

## Files Modified
1. `docker-compose-testnet.yml` - Added `--bootstrap` flags to all user and relay commands
2. `src/main.rs` - Already had bootstrap parsing logic (no changes needed)

## Prevention
To avoid this issue in future:
1. Always add `--bootstrap` CLI flags when deploying nodes
2. Use proper multiaddr format: `/dns4/hostname/tcp/port`
3. Don't rely on `DCHAT_BOOTSTRAP_PEERS` environment variable (it's not read by code)
4. Verify peer connections in logs before declaring deployment successful

## Related Issues Fixed
- Health server support added to user nodes (previous fix)
- Validator key permissions handled in deployment scripts
- Docker mount preparation prevents permission issues
