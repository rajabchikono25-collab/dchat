# 🚨 MAINNET LAUNCH - CRITICAL DEPLOYMENT GUIDE

**PRODUCTION DEPLOYMENT - NO ROOM FOR ERROR**

Status: **READY FOR LAUNCH**  
Network: **dchat Foundation Mainnet**  
Date: November 6, 2025

---

## 🌍 INFRASTRUCTURE OVERVIEW

### Validator Nodes (7 Total)

| # | Region | Provider | Public IP | Subdomain | Ports Open |
|---|--------|----------|-----------|-----------|------------|
| 1 | Ohio | AWS | Dynamic | validator1-ohio.schikuno.top | 22, 80, 443, 7070, 9090 |
| 2 | Singapore | AWS | Dynamic | validator1-singapore.schikuno.top | 22, 80, 443, 7070, 9090 |
| 3 | Stockholm | AWS | Dynamic | validator1-stockholm.schikuno.top | 22, 80, 443, 7070, 9090 |
| 4 | São Paulo | AWS | Dynamic | validator1-saopaulo.schikuno.top | 22, 80, 443, 7070, 9090 |
| 5 | Mumbai | Azure | 74.225.183.196 | validator1-india.schikuno.top | 22, 80, 443, 7070, 9090 |
| 6 | Johannesburg | Azure | 4.221.211.71 | validator1-southafrica.schikuno.top | 22, 80, 443, 7070, 9090 |
| 7 | Dubai | Azure | 4.161.34.228 | validator1-uae.schikuno.top | 22, 80, 443, 7070, 9090 |

### Relay Configuration
- **2 relays per validator server** (14 total relays)
- **Ports**: 7071, 7072 (relays), 9091, 9092 (metrics)
- **Health**: 8081, 8082

### Local Storage Clusters (Per Server)
- **Redis**: Port 6379 (cluster mode)
- **MinIO**: Port 9000 (distributed mode, TLS)
- **TiKV**: Port 2379 (PD), 20160 (TiKV)

### Managed Cloud Database
- **CockroachDB**: Hosted globally, accessed via TLS

---

## ✅ PRE-LAUNCH CHECKLIST

### Network Connectivity
- [ ] All 7 validators can resolve each other's subdomains
- [ ] DNS A records pointing to current IPs (Cloudflare)
- [ ] Firewall rules allow ports 22, 80, 443, 7070-7072, 9090-9092
- [ ] TLS certificates installed for HTTPS/WSS
- [ ] NAT traversal configured (UPnP enabled)

### Storage Infrastructure
- [ ] Redis clusters initialized on all 7 servers
- [ ] MinIO distributed mode configured (7-node federation)
- [ ] TiKV PD cluster started (3 regions minimum)
- [ ] CockroachDB connection string tested with TLS

### Validator Setup
- [ ] Validator keys generated and securely stored
- [ ] Genesis file distributed to all validators
- [ ] Bootstrap peer list configured in config-production.toml
- [ ] Initial stake amounts locked in contracts

### Monitoring & Observability
- [ ] Prometheus scraping all endpoints
- [ ] Grafana dashboards configured
- [ ] Health check endpoints returning 200 OK
- [ ] Log aggregation working (journalctl)

---

## 🚀 DEPLOYMENT SEQUENCE

### Phase 1: Storage Layer (15 minutes)

```bash
# On EACH validator server, start storage services:

# 1. Start Redis Cluster
sudo systemctl start redis-cluster
sudo systemctl enable redis-cluster
redis-cli ping  # Should return PONG

# 2. Start TiKV (on 3 primary servers only: Ohio, Singapore, Stockholm)
sudo systemctl start tikv-pd
sudo systemctl start tikv-server
sudo systemctl enable tikv-pd tikv-server

# 3. Start MinIO
sudo systemctl start minio
sudo systemctl enable minio
mc admin info local  # Verify cluster

# 4. Verify CockroachDB connectivity
psql "$COCKROACHDB_URL" -c "SELECT version();"
```

### Phase 2: Validator Network Bootstrap (10 minutes)

```bash
# On validator1-ohio (LEADER - starts first):
sudo systemctl start dchat-validator
sudo journalctl -u dchat-validator -f

# Wait 30 seconds for leader to initialize, then on remaining 6 validators:
# validator1-singapore, validator1-stockholm, validator1-saopaulo, 
# validator1-india, validator1-southafrica, validator1-uae
sudo systemctl start dchat-validator
sudo journalctl -u dchat-validator -f
```

Expected logs:
```
✓ Validator key loaded
✓ Network initialized (peer_id: 12D3Koo...)
📡 Connected to bootstrap peer: validator1-ohio
✓ Consensus engine started
📦 Producing block #1
```

### Phase 3: Relay Network Activation (5 minutes)

```bash
# On EACH validator server, start 2 relay nodes:
sudo systemctl start dchat-relay1
sudo systemctl start dchat-relay2
sudo systemctl enable dchat-relay1 dchat-relay2

# Verify relays are advertising themselves
curl http://localhost:8081/health  # Relay 1
curl http://localhost:8082/health  # Relay 2
```

### Phase 4: Cross-Region Verification (10 minutes)

```bash
# From ANY validator, test connectivity to all others:
./scripts/test-mainnet-connectivity.sh

# Expected output:
# ✓ validator1-ohio: reachable, 6 peers, height 42
# ✓ validator1-singapore: reachable, 6 peers, height 42
# [... all 7 validators ...]
# ✓ All validators synchronized at block 42
```

### Phase 5: Consensus Verification (5 minutes)

```bash
# Monitor block production across all validators
watch -n 1 'curl -s http://validator1-ohio.schikuno.top/status | jq .block_height'

# Should increment every 6 seconds:
# Block 1 -> Block 2 -> Block 3 ...

# Check all validators agree on block hashes:
for v in ohio singapore stockholm saopaulo india southafrica uae; do
  echo "validator1-$v:"
  curl -s "http://validator1-$v.schikuno.top/status" | jq '{height, hash}'
done
```

---

## 🛠️ CONFIGURATION FILES

### Generated During Deployment

1. **`/opt/dchat/config-mainnet.toml`** - Main configuration
2. **`/opt/dchat/genesis.json`** - Genesis state
3. **`/opt/dchat/keys/validator.key`** - Validator signing key (ENCRYPTED)
4. **`/etc/dchat/redis-cluster.conf`** - Redis cluster config
5. **`/etc/dchat/minio.env`** - MinIO environment variables
6. **`/etc/dchat/tikv.toml`** - TiKV PD configuration

### Systemd Services

- `/etc/systemd/system/dchat-validator.service`
- `/etc/systemd/system/dchat-relay1.service`
- `/etc/systemd/system/dchat-relay2.service`
- `/etc/systemd/system/redis-cluster.service`
- `/etc/systemd/system/minio.service`
- `/etc/systemd/system/tikv-pd.service`
- `/etc/systemd/system/tikv-server.service`

---

## 🔍 HEALTH MONITORING

### Real-Time Dashboard

```bash
# Launch monitoring dashboard (from local machine)
./scripts/mainnet-dashboard.sh

# Shows:
# - Block height per validator
# - Peer connection count
# - Consensus round time
# - Transaction throughput
# - Storage cluster health
```

### Critical Metrics to Watch

| Metric | Healthy Range | Action if Outside Range |
|--------|---------------|-------------------------|
| Block Height | Increments every 6s | Check consensus logs |
| Peer Count | 6-14 per node | Check firewall/DNS |
| Block Propagation | < 2 seconds | Check network latency |
| Memory Usage | < 2 GB per process | Restart if > 4 GB |
| Disk I/O | < 50 MB/s | Check TiKV/MinIO |
| Redis Latency | < 5ms | Check cluster sync |

### Endpoints

**Health**: `http://<subdomain>/health`  
**Metrics**: `http://<subdomain>:9090/metrics`  
**Status**: `http://<subdomain>/status`

---

## 🚨 ROLLBACK PROCEDURE

**If launch fails within first 30 minutes:**

```bash
# On ALL servers simultaneously:
sudo systemctl stop dchat-validator dchat-relay1 dchat-relay2

# Clear corrupted state (if needed):
sudo rm -rf /opt/dchat/data/*

# Restore from backup:
sudo cp /opt/dchat/backups/genesis.json.backup /opt/dchat/genesis.json

# Restart from Phase 2
```

**DO NOT rollback after 30 minutes without team approval.**

---

## 🔐 SECURITY CHECKLIST

- [ ] Validator keys stored in `/opt/dchat/keys/` with permissions `0600`
- [ ] TLS certificates valid for all subdomains (Cloudflare managed)
- [ ] SSH access restricted to team IPs only
- [ ] Redis password set and rotated
- [ ] MinIO root credentials secured (not default)
- [ ] CockroachDB using certificate authentication
- [ ] UFW/iptables rules applied to all servers
- [ ] fail2ban monitoring SSH attempts

---

## 📞 EMERGENCY CONTACTS

**DevOps Lead**: Available during launch  
**Network Engineer**: Monitoring connectivity  
**Security Officer**: Watching for attacks

**Emergency Stop Command** (use only if network compromised):
```bash
# Broadcast to all validators
for v in ohio singapore stockholm saopaulo india southafrica uae; do
  ssh azureuser@validator1-$v.schikuno.top "sudo systemctl stop dchat-validator"
done
```

---

## 📊 POST-LAUNCH VERIFICATION (24 hours)

### Hour 1
- [ ] All validators producing blocks
- [ ] 14 relays advertising availability
- [ ] Storage clusters synchronized

### Hour 6
- [ ] No consensus faults
- [ ] Transaction throughput stable
- [ ] Memory usage stable

### Hour 24
- [ ] Governance contracts functional
- [ ] Token transfers working
- [ ] Relay rewards distributing

---

## 🎯 SUCCESS CRITERIA

1. **All 7 validators online and synchronized**
2. **Block production steady at 6-second intervals**
3. **Zero consensus faults or chain halts**
4. **All 14 relays discoverable and routing messages**
5. **Storage clusters showing < 10ms latency**
6. **Health endpoints green across all services**
7. **No security incidents or unauthorized access**

---

## 📄 RELATED DOCUMENTATION

- `MAINNET_NETWORK_CONFIG.md` - Detailed network architecture
- `MAINNET_STORAGE_SETUP.md` - Storage cluster configuration
- `MAINNET_VALIDATOR_GUIDE.md` - Validator operation manual
- `MAINNET_MONITORING.md` - Observability and alerting

---

**Status**: READY FOR PRODUCTION LAUNCH 🚀  
**Next Action**: Execute Phase 1 deployment sequence  
**Last Updated**: November 6, 2025
