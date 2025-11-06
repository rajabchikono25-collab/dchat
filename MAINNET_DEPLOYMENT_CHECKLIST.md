# Mainnet Deployment Checklist

Last Updated: 2024-01-15

## ✅ COMPLETED

### Core Implementation
- [x] DNS-based peer discovery system (`crates/dchat-network/src/dns_discovery.rs`)
  - Subdomain resolution with caching (5-min TTL)
  - Background refresh task (60-second interval)
  - Cloudflare + Google DNS fallback
  - Validator and relay discovery separation
- [x] Mainnet configuration generator (`crates/dchat-deployment/src/mainnet_config.rs`)
  - All 7 validator configs
  - Storage cluster endpoints
  - TOML output generation
- [x] Validator startup integration (`src/main.rs::run_validator_node`)
  - DNS discovery integration
  - 4/7 BFT consensus waiting
  - Dynamic peer connection
  - PeerID updating after connection
- [x] Relay startup integration (`src/main.rs::run_relay_node`)
  - Discovers validators + other relays
  - Builds comprehensive bootstrap list
  - Waits for 5 peer connections
- [x] Dependency management
  - Added trust-dns-resolver 0.23 to dchat-network
  - Verified chrono in dchat-deployment

### Deployment Automation
- [x] Main deployment script (`deploy-mainnet.ps1`)
  - Build automation via WSL
  - Config generation
  - SSH deployment to all 7 servers
  - Systemd service installation (validator + 2 relays per server)
  - Error handling and rollback
- [x] Validator startup script (`start-mainnet-validators.ps1`)
  - Parallel or sequential startup
  - Health check verification
  - Consensus monitoring
- [x] Relay startup script (`start-mainnet-relays.ps1`)
  - Starts both relays per server
  - Connectivity verification
  - Peer count monitoring
- [x] Monitoring dashboard (`monitor-mainnet.ps1`)
  - Validator health (status, peers, height, sync)
  - Relay health (peer counts)
  - Consensus metrics (block height, block time)
  - Storage cluster health (Redis, MinIO, TiKV, CockroachDB)
  - Continuous monitoring mode
  - Component filtering

### Documentation
- [x] Critical deployment guide (`MAINNET_LAUNCH_CRITICAL.md`)
  - Infrastructure table
  - Pre-launch checklist
  - 5-phase deployment sequence
  - Health monitoring thresholds
  - Rollback procedures
  - Emergency stop commands
- [x] Network architecture documentation (`MAINNET_NETWORK_CONFIG.md`)
  - Complete network topology diagram
  - Infrastructure details (validators, relays)
  - DNS discovery process
  - P2P protocol stack (libp2p)
  - Firewall rules and security
  - Health endpoints and metrics
  - Performance benchmarks
  - Troubleshooting guides
- [x] Storage setup guide (`MAINNET_STORAGE_SETUP.md`)
  - Redis cluster setup (7 nodes)
  - MinIO distributed setup (7 nodes, erasure coding)
  - TiKV cluster setup (3 PD nodes, 7 TiKV servers)
  - CockroachDB cloud configuration
  - Systemd services for all storage components
  - Health checks and monitoring
  - Backup and recovery procedures
  - Application integration code samples
- [x] This TODO checklist (`MAINNET_DEPLOYMENT_CHECKLIST.md`)

## 🔄 IN PROGRESS

None currently - ready for implementation testing

## ❌ NOT STARTED

### Storage Runtime Integration
- [ ] Redis cluster client in database layer
  - Implement cluster discovery
  - Connection pooling
  - Automatic failover
  - Location: `crates/dchat-storage/src/redis.rs`
- [ ] MinIO client implementation
  - S3-compatible API integration
  - Multi-node upload/download
  - Erasure coding awareness
  - Location: `crates/dchat-storage/src/minio.rs`
- [ ] TiKV client integration
  - PD endpoint connection
  - Key-value operations
  - Transaction support
  - Location: `crates/dchat-storage/src/tikv.rs`
- [ ] CockroachDB integration
  - SQLx connection pool
  - Schema migrations
  - Transaction handling
  - Location: `crates/dchat-storage/src/cockroach.rs`
- [ ] Database layer router
  - Route data to appropriate storage
  - Implement storage interface traits
  - Handle storage failures gracefully
  - Location: `crates/dchat-storage/src/lib.rs`

### Transport Layer Enhancement
- [ ] WebSocket transport configuration
  - Configure libp2p websocket explicitly
  - Enable wss:// (WebSocket Secure) for port 443
  - TLS certificate loading
  - Location: `src/main.rs` (validator/relay startup)
- [ ] TLS certificate management
  - Let's Encrypt integration
  - Auto-renewal system
  - Certificate distribution to all nodes
  - Location: `crates/dchat-network/src/tls.rs`

### Key Management
- [ ] Validator key generation script
  - Ed25519 keypair generation
  - Secure storage in `/opt/dchat/keys/`
  - Key backup procedures
  - File: `scripts/generate-validator-keys.sh`
- [ ] Key distribution to all servers
  - Secure SCP transfer
  - Proper permissions (600)
  - Verification checksum
- [ ] Key backup system
  - Encrypted backup to MinIO
  - Multi-signature recovery
  - Guardian system integration

### Storage Cluster Deployment
- [ ] Redis cluster initialization script
  - Automated cluster creation
  - Replica configuration
  - Health verification
  - File: `scripts/init-redis-cluster.sh`
- [ ] MinIO cluster initialization
  - Distributed setup automation
  - Bucket creation
  - Policy configuration
  - File: `scripts/init-minio-cluster.sh`
- [ ] TiKV cluster deployment
  - TiUP automation
  - Topology verification
  - PD leader election check
  - File: `scripts/deploy-tikv-cluster.sh`
- [ ] CockroachDB schema deployment
  - Database creation
  - Schema migration
  - User grants
  - File: `scripts/init-cockroach-schema.sql`

### Additional Documentation
- [ ] Validator operations manual (`MAINNET_VALIDATOR_GUIDE.md`)
  - Key management procedures
  - Starting/stopping validators
  - Upgrading validator software
  - Slashing prevention
  - Emergency procedures
- [ ] Monitoring setup guide (`MAINNET_MONITORING.md`)
  - Prometheus configuration
  - Grafana dashboard setup
  - Alert rules configuration
  - Log aggregation (Loki)
  - Incident response playbook
- [ ] Disaster recovery playbook
  - Chain replay procedures
  - Consensus recovery
  - Data restoration from backups
  - Network partition handling

### Testing & Validation
- [ ] Integration tests for DNS discovery
  - Test subdomain resolution
  - Test cache expiration
  - Test fallback DNS
  - File: `tests/integration/dns_discovery.rs`
- [ ] Consensus simulation tests
  - Test 4/7 validator consensus
  - Test validator failure scenarios
  - Test network partition recovery
  - File: `tests/integration/consensus.rs`
- [ ] Storage cluster tests
  - Test Redis cluster failover
  - Test MinIO node failure
  - Test TiKV replica recovery
  - File: `tests/integration/storage.rs`
- [ ] End-to-end mainnet simulation
  - Deploy to testnet infrastructure
  - Run for 24 hours minimum
  - Stress test message throughput
  - Verify all metrics within thresholds

### Post-Deployment
- [ ] Prometheus + Grafana deployment
  - Deploy to monitoring server (Ohio)
  - Configure scrape targets
  - Import dashboards
  - Set up alert rules
- [ ] Log aggregation setup
  - Deploy Loki
  - Configure log shipping from all nodes
  - Create log queries for common issues
- [ ] Backup automation
  - Schedule hourly Redis snapshots
  - Schedule daily MinIO mirrors
  - Schedule daily TiKV full backups
  - Verify backup restoration procedures
- [ ] Performance baseline
  - Record initial metrics
  - Establish performance baselines
  - Set up regression detection
- [ ] Security audit
  - Review firewall rules on all servers
  - Audit SSH key access
  - Check TLS certificate validity
  - Review storage access policies

## 📋 DEPLOYMENT SEQUENCE

When ready to deploy:

1. **Pre-Deployment** (Day -1)
   - [ ] Build and test all binaries in WSL
   - [ ] Generate validator keys on secure machine
   - [ ] Deploy storage clusters (Redis, MinIO, TiKV)
   - [ ] Verify Cloudflare DNS records
   - [ ] Test SSH access to all servers

2. **Phase 1: Binary Deployment** (Day 0, Hour 0-1)
   - [ ] Run `./deploy-mainnet.ps1` to deploy to all servers
   - [ ] Verify binary installation on all nodes
   - [ ] Copy validator keys to each server
   - [ ] Verify config files on all nodes

3. **Phase 2: Storage Initialization** (Day 0, Hour 1-2)
   - [ ] Initialize Redis cluster
   - [ ] Initialize MinIO distributed cluster
   - [ ] Start TiKV cluster
   - [ ] Deploy CockroachDB schema
   - [ ] Verify all storage health checks pass

4. **Phase 3: Validator Startup** (Day 0, Hour 2-3)
   - [ ] Run `./start-mainnet-validators.ps1 -OneByOne`
   - [ ] Wait 30 seconds between each validator
   - [ ] Monitor consensus formation
   - [ ] Verify 4/7 validators achieve consensus
   - [ ] Check first block production

5. **Phase 4: Relay Startup** (Day 0, Hour 3-4)
   - [ ] Run `./start-mainnet-relays.ps1`
   - [ ] Verify all 14 relays start successfully
   - [ ] Check peer connections
   - [ ] Verify message routing

6. **Phase 5: Monitoring & Validation** (Day 0, Hour 4+)
   - [ ] Run `./monitor-mainnet.ps1 -Continuous`
   - [ ] Monitor for 1 hour - all metrics healthy
   - [ ] Monitor for 6 hours - check consensus stability
   - [ ] Monitor for 24 hours - verify no issues
   - [ ] Declare mainnet launch successful

## 🚨 CRITICAL ISSUES

None identified - all critical components implemented.

## 📞 DEPLOYMENT CONTACTS

- **Network Lead**: [Add Discord/Telegram]
- **DevOps Lead**: [Add Discord/Telegram]
- **Security Lead**: [Add Discord/Telegram]
- **Emergency Contact**: [Add phone number]

## 🔗 RELATED DOCUMENTATION

- `MAINNET_LAUNCH_CRITICAL.md` - Critical deployment procedures
- `MAINNET_NETWORK_CONFIG.md` - Network architecture and configuration
- `MAINNET_STORAGE_SETUP.md` - Storage cluster setup guide
- `ARCHITECTURE.md` - Complete system architecture (34 components)
- `crates/dchat-network/src/dns_discovery.rs` - DNS discovery implementation
- `crates/dchat-deployment/src/mainnet_config.rs` - Config generation

---

**Status**: ✅ READY FOR DEPLOYMENT (pending storage runtime integration and key generation)

**Next Action**: Choose deployment approach:
1. Deploy validators first, add storage integration later
2. Complete storage integration in development, then deploy all together

**Recommendation**: Deploy validators + relays now (working P2P networking), integrate storage incrementally in production.
