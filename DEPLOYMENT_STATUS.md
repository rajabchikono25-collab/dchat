# dchat Foundation Validator Deployment Guide

## Deployment Status

### Infrastructure: ✅ COMPLETE
All 7 regional validators operational with:
- Docker Engine 28.5.1
- Redis 7-alpine (6379, 2GB memory, AOF persistence)
- MinIO latest (9000-9001, 6 buckets)
- UFW firewall (ports 22, 9090-9091, 9100, 6379, 9000-9001)
- Disk usage: 10-19%

### Binary Build: ⏳ IN PROGRESS
Currently compiling: `cargo build --release --bin dchat`
- Binary will be located at: `target/release/dchat`
- Estimated completion: 5-10 minutes

### Next Steps: 📋 PENDING
1. Deploy binary to all validators
2. Generate validator keys
3. Configure bootstrap peers
4. Start validator services
5. Verify P2P connectivity

---

## Validator Network

| Region | Hostname | IP Address | User | SSH Key | Status |
|--------|----------|------------|------|---------|--------|
| Ohio | validator1-ohio.schikuno.top | 18.191.118.167 | ubuntu | AWS-Ohio/gecko.pem | ✅ Ready |
| São Paulo | validator1-saopaulo.schikuno.top | 54.233.203.82 | ubuntu | AWS-Sao-Paulo/pablo.pem | ✅ Ready |
| Singapore | validator1-singapore.schikuno.top | 18.142.96.209 | ubuntu | AWS-Singapore/craig.pem | ✅ Ready |
| Stockholm | validator1-stockholm.schikuno.top | 13.48.49.2 | ubuntu | AWS-Stokholm/relay.pem | ✅ Ready |
| India | validator1-india.schikuno.top | 74.225.183.196 | azureuser | Azure-India/uramami.pem | ✅ Ready |
| South Africa | validator1-southafrica.schikuno.top | 4.221.211.71 | azureuser | Azure-SAfrica/anacreon.pem | ✅ Ready |
| UAE | validator1-uae.schikuno.top | 4.161.34.228 | azureuser | Azure_UAE/Randal_key.pem | ✅ Ready |

---

## Deployment Scripts (ansible/)

### Infrastructure Management
- **`deploy-direct.sh`**: Deploy Docker, Redis, MinIO to all validators ✅ DONE
- **`check-all-validators.sh`**: Check status of all services ✅ DONE

### Binary Deployment
- **`deploy-binary.sh`**: Deploy dchat binary and configs to all validators ⏳ READY
- **`generate-validator-keys.sh`**: Generate validator keys on each node ⏳ READY

### Service Management
- **`start-all-validators.sh`**: Start dchat validator services ⏳ READY
- **`monitor-all-validators.sh`**: Monitor logs and status ⏳ READY

---

## Deployment Workflow

### Step 1: Deploy Binary (NEXT)
```bash
cd ansible
./deploy-binary.sh
```
**Actions:**
- Copy `target/release/dchat` to `/opt/dchat/dchat` on all validators
- Create systemd service: `dchat-validator.service`
- Generate config: `/opt/dchat/config.toml`
- Set proper permissions

**Expected Time:** 2-3 minutes

### Step 2: Generate Validator Keys
```bash
./generate-validator-keys.sh
```
**Actions:**
- Run `dchat keygen` on each validator
- Save keys to `/opt/dchat/keys/validator.key`
- Backup keys to local `./validator_keys/`
- Extract peer IDs for bootstrap config

**Expected Time:** 1-2 minutes

### Step 3: Update Bootstrap Peers
Manually update `/opt/dchat/config.toml` on each validator:
```toml
[network]
bootstrap_peers = [
    "/ip4/18.191.118.167/tcp/9090/p2p/<ohio-peer-id>",
    "/ip4/54.233.203.82/tcp/9090/p2p/<saopaulo-peer-id>",
    "/ip4/18.142.96.209/tcp/9090/p2p/<singapore-peer-id>",
    "/ip4/13.48.49.2/tcp/9090/p2p/<stockholm-peer-id>",
    "/ip4/74.225.183.196/tcp/9090/p2p/<india-peer-id>",
    "/ip4/4.221.211.71/tcp/9090/p2p/<southafrica-peer-id>",
    "/ip4/4.161.34.228/tcp/9090/p2p/<uae-peer-id>"
]
```

**Expected Time:** 5 minutes (manual editing)

### Step 4: Start Validators
```bash
./start-all-validators.sh
```
**Actions:**
- Enable systemd service on all validators
- Start dchat validator processes
- Verify startup logs

**Expected Time:** 1 minute

### Step 5: Monitor & Verify
```bash
# Check all validators
./monitor-all-validators.sh

# Follow logs for specific region
./monitor-all-validators.sh -f ohio
```
**Verify:**
- ✅ All services running
- ✅ P2P connections established (6 peers each)
- ✅ Consensus participating
- ✅ Metrics endpoint responding (port 9100)

---

## Configuration Details

### Validator Service
**Systemd Unit:** `/etc/systemd/system/dchat-validator.service`
```ini
[Service]
ExecStart=/opt/dchat/dchat validator \
  --config /opt/dchat/config.toml \
  --key /opt/dchat/keys/validator.key \
  --chain-rpc http://localhost:26657 \
  --stake 10000 \
  --producer
```

### Validator Config
**File:** `/opt/dchat/config.toml`
```toml
[network]
listen_address = "0.0.0.0:9090"
external_address = "<validator-ip>:9090"
bootstrap_peers = [] # Update after key generation

[storage.cockroachdb]
connection_string = "${COCKROACHDB_CONNECTION}"

[storage.redis]
host = "localhost"
port = 6379

[storage.minio]
endpoint = "localhost:9000"
access_key = "dchat"
secret_key = "YourSecurePassword123!"

[metrics]
enabled = true
listen_address = "0.0.0.0:9100"

[logging]
level = "info"
file = "/opt/dchat/logs/validator.log"
```

---

## Service Ports

| Port | Service | Protocol | Description |
|------|---------|----------|-------------|
| 22 | SSH | TCP | Remote management |
| 6379 | Redis | TCP | In-memory cache |
| 9000 | MinIO | TCP | Object storage API |
| 9001 | MinIO | TCP | Console UI |
| 9090 | P2P | TCP | Validator P2P network |
| 9091 | Health | TCP | Health check endpoint |
| 9100 | Metrics | TCP | Prometheus metrics |

---

## Verification Checklist

### Infrastructure ✅
- [x] DNS resolving correctly (7/7)
- [x] SSH access working (7/7)
- [x] Docker running (7/7)
- [x] Redis responding PONG (7/7)
- [x] MinIO health OK (7/7)
- [x] Firewall configured (7/7)
- [x] Disk space sufficient (7/7)

### Binary Deployment ⏳
- [ ] Binary built successfully
- [ ] Binary deployed to all validators
- [ ] Config files generated
- [ ] Systemd services created
- [ ] Permissions set correctly

### Validator Network ⏳
- [ ] Validator keys generated (7/7)
- [ ] Peer IDs extracted
- [ ] Bootstrap peers configured
- [ ] Services started (7/7)
- [ ] P2P connections established
- [ ] Consensus active
- [ ] Metrics endpoints responding

---

## Troubleshooting

### Check Service Status
```bash
ssh <user>@<ip> "sudo systemctl status dchat-validator"
```

### View Logs
```bash
ssh <user>@<ip> "sudo journalctl -u dchat-validator -f"
```

### Check P2P Connections
```bash
curl http://<ip>:9100/metrics | grep libp2p_peers
```

### Restart Service
```bash
ssh <user>@<ip> "sudo systemctl restart dchat-validator"
```

### Check Redis
```bash
ssh <user>@<ip> "sudo docker exec dchat-redis redis-cli ping"
```

### Check MinIO
```bash
curl http://<ip>:9000/minio/health/live
```

---

## Current Build Progress

**Command:** `cargo build --release --bin dchat`
**Status:** Compiling dependencies (~291/777 crates)
**Started:** [Current time]
**Estimated Completion:** 5-10 minutes

**Binary Output:** `target/release/dchat`
**Expected Size:** ~50-100 MB (release mode with optimizations)

---

## Security Notes

### SSH Keys
- All .pem files have 400 permissions (owner read-only)
- Keys stored in `Foundation-servers/` directory
- AWS uses `ubuntu` user, Azure uses `azureuser`

### MinIO Credentials
- **Access Key:** dchat
- **Secret Key:** YourSecurePassword123!
- **Recommendation:** Change password after deployment

### CockroachDB Connection
- Currently using default connection string
- **Recommendation:** Update with production cluster endpoint

### Validator Keys
- Generated locally on each validator
- Backed up to `./validator_keys/` directory
- **CRITICAL:** Secure backup storage required

---

## Monitoring

### Health Check
```bash
curl http://<ip>:9091/health
```
**Expected:** `{"status": "healthy"}`

### Metrics
```bash
curl http://<ip>:9100/metrics
```
**Key Metrics:**
- `dchat_validator_height` - Current block height
- `dchat_validator_peers` - Connected peer count
- `dchat_consensus_round` - Consensus round
- `dchat_messages_processed` - Total messages processed

---

## Next Steps After Deployment

1. **Monitor Initial Consensus**
   - Verify all 7 validators participating
   - Check block production (every 6 seconds)
   - Monitor peer connections (should be 6 peers each)

2. **Test Message Routing**
   - Deploy test user clients
   - Send messages between regions
   - Verify delivery latency

3. **Configure Monitoring**
   - Set up Prometheus scraping
   - Configure Grafana dashboards
   - Set up alerting rules

4. **Production Hardening**
   - Rotate MinIO password
   - Set up automated backups
   - Configure log rotation
   - Enable fail2ban
   - Set up DDoS protection

5. **Documentation**
   - Document peer IDs
   - Create runbooks
   - Set up on-call rotation

---

**Generated:** $(date)
**Status:** Binary compilation in progress
**ETA to Production:** 30-45 minutes
