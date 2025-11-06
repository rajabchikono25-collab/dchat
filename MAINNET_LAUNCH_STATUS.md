# 🚀 MAINNET LAUNCH PREPARATION COMPLETE

**Date**: November 6, 2025
**Network**: dchat-mainnet-1
**Status**: ✅ **READY FOR DEPLOYMENT**

---

## ✅ COMPLETED TASKS

### 1. ✅ Validator Keys Generated
**Location**: `./mainnet-keys/`

Generated 7 Ed25519 keypairs:
- ✅ **Ohio** (AWS) - `e26a3592fbffe18419e083e5728935cd17d4b68b67f36dd1bc16d6c60baccf2a`
- ✅ **Singapore** (AWS) - `14b66a5114968f83f8496340f13dbc64a1f1ad68b8a17f911610cb422e25ec30`
- ✅ **Stockholm** (AWS) - `8438736db20a2a800780680c3cb7823bd217d1a53873cab2751dd54c20e8185`
- ✅ **São Paulo** (AWS) - `0fc82f04eaec83cca7dea8a732e1aa711e5587e5a5be1bb09acc428fe44e8411`
- ✅ **India** (Azure) - `1e3e684e97aa00bc5d22084d2ca75cfe911ee6fb90e1277589d40b8ab8799579`
- ✅ **South Africa** (Azure) - `b709ae1e971d1d9b77a092c09f9718065a23c72bc83c7077c49686d17570c62c`
- ✅ **UAE** (Azure) - `b8b88bdca8c84479c0bb1f872c8a62890d87bed2b21b76c0b160c9812dcc1494`

**Files Generated**:
- 14 key files (7 private + 7 public)
- 1 summary file
- 1 distribution guide

### 2. ✅ Configuration Files Generated
**Location**: `./mainnet-configs/`

Generated 7 production TOML configs:
- ✅ config-mainnet-ohio.toml
- ✅ config-mainnet-singapore.toml
- ✅ config-mainnet-stockholm.toml
- ✅ config-mainnet-saopaulo.toml
- ✅ config-mainnet-india.toml
- ✅ config-mainnet-southafrica.toml
- ✅ config-mainnet-uae.toml

**Configuration Includes**:
- DNS-based peer discovery (all 7 validator subdomains)
- Validator settings (TCP 7070, consensus, BFT 4/7)
- Relay configurations (2 per server: 7071, 7072)
- Storage clusters (Redis, MinIO, TiKV, CockroachDB)
- Monitoring (Prometheus 9090, Health 8080)
- Security (TLS, rate limiting)
- Genesis block with all validator public keys

---

## 📋 DEPLOYMENT CHECKLIST

### ⏳ Pre-Deployment Tasks

#### A. Key Distribution (CRITICAL - DO THIS FIRST)
```bash
# Read the distribution guide
cat ./mainnet-keys/KEY-DISTRIBUTION-INSTRUCTIONS.md

# For each validator:
# 1. Backup private key to encrypted storage
# 2. Transfer key securely via SSH (never email/Slack)
# 3. Place key at: /etc/dchat/keys/validator.key
# 4. Set permissions: chmod 600 /etc/dchat/keys/validator.key
# 5. Verify validator can read key
```

#### B. Environment Variables (REQUIRED)
Set these on each server:
```bash
export REDIS_PASSWORD="<generate-strong-password>"
export MINIO_ACCESS_KEY="<generate-access-key>"
export MINIO_SECRET_KEY="<generate-secret-key>"
export COCKROACH_CONNECTION_STRING="<your-cockroachdb-connection>"
```

#### C. Server Preparation
- [ ] Verify SSH access to all 7 servers
- [ ] Confirm Cloudflare DNS records point to current IPs
- [ ] Test DNS resolution: `nslookup validator1-ohio.schikuno.top`
- [ ] Verify firewall rules (ports 22, 80, 443, 7070-7072 open)
- [ ] Create dchat user: `sudo useradd -m -s /bin/bash dchat`
- [ ] Create directories:
  ```bash
  sudo mkdir -p /etc/dchat/{keys,certs}
  sudo mkdir -p /var/log/dchat
  sudo chown -R dchat:dchat /etc/dchat /var/log/dchat
  ```

---

## 🚀 DEPLOYMENT SEQUENCE

### Phase 1: Deploy Binary & Configuration (All Servers)

For each server, run:
```bash
# Copy binary
scp ./target/release/dchat user@validator1-REGION.schikuno.top:/tmp/
ssh user@validator1-REGION.schikuno.top
sudo mv /tmp/dchat /usr/local/bin/
sudo chown root:root /usr/local/bin/dchat
sudo chmod 755 /usr/local/bin/dchat

# Copy config
scp ./mainnet-configs/config-mainnet-REGION.toml user@validator1-REGION.schikuno.top:/tmp/
ssh user@validator1-REGION.schikuno.top
sudo mv /tmp/config-mainnet-REGION.toml /etc/dchat/config.toml
sudo chown dchat:dchat /etc/dchat/config.toml
sudo chmod 644 /etc/dchat/config.toml

# Copy validator key (ALREADY DONE in key distribution step)

# Verify installation
dchat --version
sudo -u dchat dchat validate-config /etc/dchat/config.toml
```

### Phase 2: Start Validators One-by-One

**IMPORTANT**: Start validators sequentially to form consensus properly.

#### Start Order (Wait for Each to Sync Before Next):

**1. Ohio (First Validator)**
```bash
ssh user@validator1-ohio.schikuno.top
sudo systemctl start dchat-validator
sudo journalctl -u dchat-validator -f
# Wait for: "Validator started, waiting for peers..."
```

**2. Singapore (Second Validator)**
```bash
ssh user@validator1-singapore.schikuno.top
sudo systemctl start dchat-validator
sudo journalctl -u dchat-validator -f
# Wait for: "Connected to 1 peer(s)"
```

**3. Stockholm (Third Validator)**
```bash
ssh user@validator1-stockholm.schikuno.top
sudo systemctl start dchat-validator
sudo journalctl -u dchat-validator -f
# Wait for: "Connected to 2 peer(s)"
```

**4. São Paulo (Fourth Validator - CONSENSUS REACHED)**
```bash
ssh user@validator1-saopaulo.schikuno.top
sudo systemctl start dchat-validator
sudo journalctl -u dchat-validator -f
# Watch for: "✓ CONSENSUS REACHED (4/7 validators)"
# Watch for: "Block #1 produced"
```

**5. India (Fifth Validator)**
```bash
ssh user@validator1-india.schikuno.top
sudo systemctl start dchat-validator
# Should sync quickly with existing chain
```

**6. South Africa (Sixth Validator)**
```bash
ssh user@validator1-southafrica.schikuno.top
sudo systemctl start dchat-validator
```

**7. UAE (Seventh Validator)**
```bash
ssh user@validator1-uae.schikuno.top
sudo systemctl start dchat-validator
# Network now has all 7 validators active!
```

### Phase 3: Start Relays (All Servers)

After all validators are running:
```bash
# On each server, start both relays
sudo systemctl start dchat-relay-1
sudo systemctl start dchat-relay-2

# Verify
sudo systemctl status dchat-relay-1
sudo systemctl status dchat-relay-2
```

### Phase 4: Verify Network Health

```bash
# Check consensus
curl http://validator1-ohio.schikuno.top:8080/health

# Check peer connections (should show 6+ connections)
curl http://validator1-ohio.schikuno.top:8080/peers

# Check block height (should be increasing every 6 seconds)
curl http://validator1-ohio.schikuno.top:8080/block/latest

# Check Prometheus metrics
curl http://validator1-ohio.schikuno.top:9090/metrics
```

---

## 📊 EXPECTED NETWORK STATE

### After Full Deployment

**Validators**:
- 7 validators running
- Minimum 4/7 for consensus (BFT)
- Block production every 6 seconds
- Each validator connected to 6+ peers

**Relays**:
- 14 relays running (2 per validator)
- Load balancing message relay
- Each relay connected to all validators

**Storage**:
- Redis cluster: 7 nodes
- MinIO distributed: 7 nodes
- TiKV: 3 PD nodes, 7 server nodes
- CockroachDB: Cloud-managed

**Monitoring**:
- 7 Prometheus endpoints (port 9090)
- 7 Health check endpoints (port 8080)
- Network dashboard available

---

## 🔍 MONITORING COMMANDS

### Check Validator Status
```bash
# Ohio
curl http://validator1-ohio.schikuno.top:8080/health
# Expected: {"status":"healthy","role":"validator","peers":6,"consensus":"active"}

# All validators
for region in ohio singapore stockholm saopaulo india southafrica uae; do
  echo "=== $region ===="
  curl -s http://validator1-$region.schikuno.top:8080/health | jq .
done
```

### Check Consensus
```bash
# View consensus status
curl http://validator1-ohio.schikuno.top:8080/consensus
# Expected: {"active_validators":7,"minimum_required":4,"status":"ACTIVE"}
```

### Check Block Production
```bash
# Watch block production (should increment every 6 seconds)
watch -n 2 'curl -s http://validator1-ohio.schikuno.top:8080/block/latest | jq .height'
```

### Check Peer Connections
```bash
# View connected peers
curl http://validator1-ohio.schikuno.top:8080/peers | jq .
# Expected: Array with 6+ peer connections
```

### View Logs
```bash
# Validator logs
ssh user@validator1-ohio.schikuno.top
sudo journalctl -u dchat-validator -f

# Relay logs
sudo journalctl -u dchat-relay-1 -f
sudo journalctl -u dchat-relay-2 -f
```

---

## ⚠️ TROUBLESHOOTING

### If Consensus Not Forming

**Symptoms**: Block height stuck at 0, "waiting for peers"

**Checks**:
1. Verify DNS resolution: `nslookup validator1-ohio.schikuno.top`
2. Test connectivity: `telnet validator1-ohio.schikuno.top 7070`
3. Check firewall: `sudo ufw status` (ports 7070 must be open)
4. Verify validator keys match config public keys
5. Check logs for connection errors

**Solution**:
```bash
# Restart validators in order
sudo systemctl restart dchat-validator
```

### If Validator Can't Read Key

**Symptoms**: "Failed to load validator key" error

**Solution**:
```bash
# Fix permissions
sudo chown dchat:dchat /etc/dchat/keys/validator.key
sudo chmod 600 /etc/dchat/keys/validator.key

# Verify
sudo -u dchat cat /etc/dchat/keys/validator.key
```

### If DNS Discovery Fails

**Symptoms**: "DNS resolution failed" in logs

**Solution**:
```bash
# Verify DNS servers are accessible
dig @1.1.1.1 validator1-ohio.schikuno.top
dig @8.8.8.8 validator1-ohio.schikuno.top

# Check Cloudflare DNS records
# Ensure A records point to current IPs
```

### If Storage Connection Fails

**Symptoms**: "Redis connection refused" or similar

**Solution**:
```bash
# Verify environment variables are set
echo $REDIS_PASSWORD
echo $MINIO_ACCESS_KEY
echo $COCKROACH_CONNECTION_STRING

# Test connections
redis-cli -h validator1-ohio.schikuno.top -p 6379 ping
curl http://validator1-ohio.schikuno.top:9000/minio/health/live
```

---

## 📚 DOCUMENTATION REFERENCE

### Generated Files
- **Validator Keys**: `./mainnet-keys/`
  - Key files: `validator-REGION.key`, `validator-REGION.pub`
  - Summary: `validator-keys-summary.json`
  - Distribution guide: `KEY-DISTRIBUTION-INSTRUCTIONS.md`

- **Configurations**: `./mainnet-configs/`
  - Config files: `config-mainnet-REGION.toml`
  - Summary: `MAINNET-CONFIG-SUMMARY.md`

### Documentation
- `MAINNET_LAUNCH_CRITICAL.md` - Critical deployment procedures
- `MAINNET_NETWORK_CONFIG.md` - Network architecture details
- `MAINNET_STORAGE_SETUP.md` - Storage cluster configuration
- `MAINNET_DEPLOYMENT_CHECKLIST.md` - Deployment checklist
- `MAINNET_QUICK_REF.md` - Quick reference guide
- `COMPILATION_VERIFICATION_COMPLETE.md` - Build verification

---

## 🎯 SUCCESS CRITERIA

Mainnet launch is successful when:

- [x] ✅ All 7 validator keys generated
- [x] ✅ All 7 configuration files generated
- [ ] ⏳ All 7 validators running
- [ ] ⏳ Consensus active (4/7 minimum)
- [ ] ⏳ Blocks producing every 6 seconds
- [ ] ⏳ All 14 relays running
- [ ] ⏳ Storage clusters operational
- [ ] ⏳ Monitoring dashboards active
- [ ] ⏳ Health checks passing

---

## 🚀 NEXT IMMEDIATE ACTIONS

### 1. Distribute Validator Keys (CRITICAL)
```powershell
# Read distribution instructions
cat ./mainnet-keys/KEY-DISTRIBUTION-INSTRUCTIONS.md

# Backup keys to encrypted storage
Compress-Archive -Path ./mainnet-keys -DestinationPath "mainnet-keys-backup-$(Get-Date -Format 'yyyyMMdd-HHmmss').zip"

# Securely transfer keys to each validator
# Follow the distribution checklist in KEY-DISTRIBUTION-INSTRUCTIONS.md
```

### 2. Set Environment Variables on All Servers
```bash
# On each server, add to /etc/environment or ~/.bashrc
export REDIS_PASSWORD="<generate-strong-password>"
export MINIO_ACCESS_KEY="<generate-access-key>"
export MINIO_SECRET_KEY="<generate-secret-key>"
export COCKROACH_CONNECTION_STRING="<your-connection-string>"
```

### 3. Deploy Binary to All Servers
```bash
# Use deployment script or manual scp
for region in ohio singapore stockholm saopaulo india southafrica uae; do
  scp ./target/release/dchat user@validator1-$region.schikuno.top:/tmp/
  ssh user@validator1-$region.schikuno.top "sudo mv /tmp/dchat /usr/local/bin/ && sudo chmod 755 /usr/local/bin/dchat"
done
```

### 4. Deploy Configurations
```bash
# Copy configs to each server
for region in ohio singapore stockholm saopaulo india southafrica uae; do
  scp ./mainnet-configs/config-mainnet-$region.toml user@validator1-$region.schikuno.top:/tmp/
  ssh user@validator1-$region.schikuno.top "sudo mv /tmp/config-mainnet-$region.toml /etc/dchat/config.toml"
done
```

### 5. Start Validators Sequentially
```bash
# Follow the startup sequence in Phase 2 above
# Start Ohio first, then Singapore, Stockholm, São Paulo (consensus), India, South Africa, UAE
```

---

## ✅ PREPARATION COMPLETE

**Status**: 🟢 **READY FOR DEPLOYMENT**

All preparation work is complete:
- ✅ Validator keys generated (7 Ed25519 keypairs)
- ✅ Configuration files generated (7 TOML files)
- ✅ Distribution guides created
- ✅ Monitoring dashboards configured
- ✅ Security settings applied

**Next Step**: Follow the deployment sequence above to launch the mainnet!

---

**Generated**: November 6, 2025
**Network**: dchat-mainnet-1
**Validators**: 7 (4 AWS + 3 Azure)
**Status**: READY FOR LAUNCH 🚀
