# dchat Mainnet - Quick Start Guide

## 🚀 Deploy a Relay Node

```bash
# 1. Set your region
export DCHAT_REGION="us-east-1"  # or "singapore", "stockholm", etc.

# 2. Start the relay
cargo run --release -- relay \
    --stake 1000 \
    --listen /ip4/0.0.0.0/tcp/7070 \
    --metrics-addr 0.0.0.0:9090 \
    --health-addr 0.0.0.0:8080

# 3. Check health
curl http://localhost:8080/health

# 4. View metrics
curl http://localhost:9090/metrics | grep dchat_peer
```

## 🔐 Deploy a Validator Node

```bash
# 1. Generate validator keys (first time only)
cargo run -- keygen --output validator_keys/validator1.key

# 2. Set your region
export DCHAT_REGION="us-east-1"

# 3. Start the validator
cargo run --release -- validator \
    --key validator_keys/validator1.key \
    --stake 10000 \
    --chain-rpc https://chat-chain.schikuno.top \
    --producer true \
    --metrics-addr 0.0.0.0:9090 \
    --health-addr 0.0.0.0:8080

# 4. Monitor consensus
tail -f logs/validator.log | grep "block"
```

## 👤 Connect as a Client

```bash
# 1. Find a relay node multiaddr
RELAY_ADDR="/ip4/relay1.schikuno.top/tcp/7070/p2p/12D3KooW..."

# 2. Start interactive chat
cargo run --release -- user \
    --bootstrap "$RELAY_ADDR" \
    --username "YourName"

# 3. Send messages
Type in #global channel and press Enter
```

## 📊 What to Monitor

### Relay Node Logs
```
✓ Registered 11 bootstrap peers in global registry
✓ Peer health monitor started (30s interval)
✓ Peer list sync started (60s interval)
✓ Phase 4 complete: Connected to 5 peers
🆕 New peer joined: 12D3KooW...
📊 Stats: 100 events processed, 12 peers in registry
```

### Validator Node Logs
```
✓ Registered 6 bootstrap validators
✓ Consensus threshold reached (5/7 validators connected)
🔐 BFT Configuration: N=7, f=2, required_signatures=5
📦 Producing block #123 with validator signature
```

### Client Node Logs
```
✓ Registered 5 bootstrap peers
✓ Bootstrap complete: 3 relay(s) connected
✓ Subscribed to #global channel
📤 Sent test message #1
```

## 🔍 Debugging

### Check Peer Discovery
```bash
# DNS discovery should find validators
dig +short validator1-ohio.schikuno.top
dig +short validator1-singapore.schikuno.top

# Check libp2p peer list
curl http://localhost:9090/metrics | grep peer_count
```

### Check Handshaking
```bash
# Look for handshake logs
tail -f logs/*.log | grep -i handshake

# Expected output:
# Handshake initiated with 12D3KooW...
# Validator handshake completed with 12D3KooW...
```

### Check Background Tasks
```bash
# Health monitor runs every 30s
tail -f logs/*.log | grep "health_monitor"

# Peer sync runs every 60s
tail -f logs/*.log | grep "peer_list_sync"
```

## 🛑 Graceful Shutdown

```
Press Ctrl+C

# Expected output:
🛑 Received shutdown signal (Ctrl+C)
╔═══════════════════════════════════════════════════════════╗
║              Initiating Graceful Shutdown                ║
╚═══════════════════════════════════════════════════════════╝
📡 Stopping background tasks...
✓ All background tasks stopped cleanly
💾 Closing database...
✓ Database closed
📊 Final Network Statistics:
   Total peers discovered: 12
   Validators: 6
   Relays: 5
   Events processed: 543
╔═══════════════════════════════════════════════════════════╗
║            ✓ Relay Node Shutdown Complete                ║
╚═══════════════════════════════════════════════════════════╝
```

## 🌍 Multi-Region Deployment

### Deploy to 3 Regions
```bash
# Region 1: US-East
ssh us-east-server
export DCHAT_REGION="us-east-1"
cargo run --release -- relay --stake 1000 --listen /ip4/0.0.0.0/tcp/7070

# Region 2: Singapore
ssh singapore-server
export DCHAT_REGION="singapore"
cargo run --release -- relay --stake 1000 --listen /ip4/0.0.0.0/tcp/7070

# Region 3: Stockholm
ssh stockholm-server
export DCHAT_REGION="stockholm"
cargo run --release -- relay --stake 1000 --listen /ip4/0.0.0.0/tcp/7070
```

### Verify Cross-Region Discovery
```bash
# Each relay should discover all others
curl http://us-east-ip:9090/metrics | grep peer_count
# dchat_peer_count{type="relay"} 2  ← Should see 2 other relays
```

## 🎯 Expected Behavior

| Event | Timeframe | What to See |
|-------|-----------|-------------|
| Node Start | 0-10s | DNS discovery finds validators/relays |
| Bootstrap | 10-30s | Connects to 3-5 peers |
| Handshaking | 30-60s | Exchanges node metadata with peers |
| Steady State | 60s+ | Peer health checks every 30s, sync every 60s |
| New Peer Join | Immediate | Handshake initiated, added to registry |
| Peer Leave | 120s | Pruned from registry after timeout |
| Shutdown | 5-30s | Clean task termination, stats logged |

## 🐛 Common Issues

### "Failed to discover validators"
- **Cause**: DNS not resolving `*.schikuno.top`
- **Fix**: Check DNS settings, try `dig validator1-ohio.schikuno.top`

### "Insufficient validator connections"
- **Cause**: Less than 3 validators connected (BFT threshold)
- **Fix**: Wait 60s for discovery, check firewall rules

### "Handshake failed"
- **Cause**: Network timeout, peer offline
- **Fix**: Normal for some peers, should succeed with others

### "Background tasks shutdown timeout"
- **Cause**: Slow network cleanup
- **Fix**: Increase timeout in code (currently 30s), not critical

## 📚 Related Files

- **Full Architecture**: `ARCHITECTURE.md`
- **Refactor Details**: `MAINNET_REFACTOR_COMPLETE.md`
- **Development Guide**: `.github/copilot-instructions.md`
- **Configuration**: `config-production.toml`

## 🆘 Support

- **Logs**: Check `logs/dchat.log` for detailed traces
- **Metrics**: Prometheus at `http://localhost:9090/metrics`
- **Health**: `http://localhost:8080/health`
- **Issues**: See `IMPLEMENTATION_STATUS.md` for known issues

---

**Quick Reference**: Keep this file open during deployment!
