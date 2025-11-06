# Mainnet Network Architecture

## Overview
dchat mainnet consists of 7 validators distributed globally across AWS and Azure regions, with 14 relay nodes (2 per validator server) and distributed storage clusters.

## Network Topology

```
┌─────────────────────────────────────────────────────────────────────┐
│                         dchat Mainnet Network                       │
│                                                                     │
│  ┌────────────────┐     ┌────────────────┐     ┌────────────────┐ │
│  │   Ohio (AWS)   │────▶│Singapore (AWS) │◀───▶│Stockholm (AWS) │ │
│  │  Validator 1   │     │  Validator 2   │     │  Validator 3   │ │
│  │   2 Relays     │     │   2 Relays     │     │   2 Relays     │ │
│  └────────────────┘     └────────────────┘     └────────────────┘ │
│         │                       │                       │          │
│         │                       │                       │          │
│         └───────────────────────┼───────────────────────┘          │
│                                 │                                  │
│  ┌────────────────┐     ┌────────────────┐     ┌────────────────┐ │
│  │São Paulo (AWS) │────▶│  India (Azure) │◀───▶│S.Africa (Azure)│ │
│  │  Validator 4   │     │  Validator 5   │     │  Validator 6   │ │
│  │   2 Relays     │     │   2 Relays     │     │   2 Relays     │ │
│  └────────────────┘     └────────────────┘     └────────────────┘ │
│                                 │                                  │
│                         ┌────────────────┐                         │
│                         │   UAE (Azure)  │                         │
│                         │  Validator 7   │                         │
│                         │   2 Relays     │                         │
│                         └────────────────┘                         │
└─────────────────────────────────────────────────────────────────────┘
```

## Infrastructure Details

### Validators

| ID | Region | Provider | Public IP | Subdomain | P2P Port | Health Port | Metrics Port |
|----|--------|----------|-----------|-----------|----------|-------------|--------------|
| 1 | Ohio | AWS | Dynamic | validator1-ohio.schikuno.top | 7070 | 8080 | 9090 |
| 2 | Singapore | AWS | Dynamic | validator1-singapore.schikuno.top | 7070 | 8080 | 9090 |
| 3 | Stockholm | AWS | Dynamic | validator1-stockholm.schikuno.top | 7070 | 8080 | 9090 |
| 4 | São Paulo | AWS | Dynamic | validator1-saopaulo.schikuno.top | 7070 | 8080 | 9090 |
| 5 | India | Azure | 74.225.183.196 | validator1-india.schikuno.top | 7070 | 8080 | 9090 |
| 6 | South Africa | Azure | 4.221.211.71 | validator1-southafrica.schikuno.top | 7070 | 8080 | 9090 |
| 7 | UAE | Azure | 4.161.34.228 | validator1-uae.schikuno.top | 7070 | 8080 | 9090 |

### Relays (2 per validator server)

Each validator server hosts 2 relay nodes:
- **Relay 1**: Port 7071 (P2P), 8081 (Health), 9091 (Metrics)
- **Relay 2**: Port 7072 (P2P), 8082 (Health), 9092 (Metrics)

**Total Relays**: 14 (7 servers × 2 relays)

### DNS-Based Peer Discovery

All nodes use DNS-based peer discovery to handle dynamic IP addresses:

**DNS Configuration:**
- Provider: Cloudflare DNS
- Update frequency: 60 seconds
- Cache TTL: 5 minutes
- Fallback DNS: Google DNS (8.8.8.8)

**Discovery Process:**
1. Node resolves validator subdomains via `trust-dns-resolver`
2. Caches resolved IPs with expiration tracking
3. Background task refreshes DNS every 60 seconds
4. Builds multiaddrs from resolved IPs and known ports
5. Connects to discovered peers via libp2p

**Implementation:** `crates/dchat-network/src/dns_discovery.rs`

## Network Protocols

### Transport Layers

| Protocol | Port(s) | Usage | Public Access |
|----------|---------|-------|---------------|
| TCP | 7070-7072 | P2P communication | Limited (peer-to-peer) |
| WebSocket | 443 | P2P over HTTPS | Yes |
| HTTP | 80, 8080-8082 | Health checks | Yes |
| HTTPS | 443 | API endpoints | Yes |
| SSH | 22 | Server management | Admin only |

### P2P Stack (libp2p)

**Components:**
- **Transport**: TCP, WebSocket (for NAT traversal)
- **Security**: Noise Protocol (XX handshake pattern)
- **Multiplexing**: yamux
- **Discovery**: Kademlia DHT + DNS-based bootstrap
- **NAT Traversal**: UPnP, STUN, DCUtR (Direct Connection Upgrade through Relay)
- **Relay Protocol**: Circuit relay v2

**Connection Flow:**
```
1. Validator starts → DNS discovery resolves all validator subdomains
2. Builds bootstrap peer list from resolved IPs
3. Establishes TCP connections to bootstrap peers
4. Performs Noise handshake for encrypted channels
5. Discovers additional peers via Kademlia DHT
6. Falls back to relay nodes if direct connection fails (DCUtR)
7. Maintains connections with periodic keepalives
```

**Consensus Requirements:**
- **Minimum validators for BFT**: 4/7 (57%)
- **Minimum relays for health**: 5 peer connections
- **Block time**: 6 seconds
- **Finality**: 2 block confirmations (~12 seconds)

## Firewall & Network Security

### Public Ports (0.0.0.0)
- **22**: SSH (admin access only, key-based auth)
- **80**: HTTP health endpoints
- **443**: HTTPS/WSS (P2P + API)
- **8080-8082**: Health check endpoints
- **9090-9092**: Prometheus metrics (recommend restricting to monitoring IPs)

### Private Ports (localhost or cluster-only)
- **6379**: Redis cluster
- **9000**: MinIO S3
- **2379**: TiKV PD
- **20160**: TiKV server

### Recommended Firewall Rules

**Validators:**
```bash
# Allow SSH (admin only)
ufw allow from <admin_ip> to any port 22

# Allow HTTP/HTTPS
ufw allow 80/tcp
ufw allow 443/tcp

# Allow P2P (restricted to known peers)
ufw allow 7070/tcp

# Allow health checks (public)
ufw allow 8080/tcp

# Allow metrics (monitoring only)
ufw allow from <monitoring_ip> to any port 9090
```

**Relays:**
```bash
# Similar to validators, but open relay ports
ufw allow 7071/tcp
ufw allow 7072/tcp
ufw allow 8081/tcp
ufw allow 8082/tcp
ufw allow 9091/tcp
ufw allow 9092/tcp
```

**Storage (cluster-internal only):**
```bash
# Redis
ufw allow from <validator_ips> to any port 6379

# MinIO
ufw allow from <validator_ips> to any port 9000

# TiKV
ufw allow from <validator_ips> to any port 2379
ufw allow from <validator_ips> to any port 20160
```

## Network Monitoring

### Health Endpoints

**Validator Health:**
```bash
curl http://validator1-ohio.schikuno.top:8080/health
```

**Expected Response:**
```json
{
  "status": "healthy",
  "peer_count": 6,
  "block_height": 12345,
  "is_synced": true,
  "chain_id": "dchat-mainnet-1",
  "validator_address": "dchat1..."
}
```

**Relay Health:**
```bash
curl http://validator1-ohio.schikuno.top:8081/health
```

**Expected Response:**
```json
{
  "status": "healthy",
  "peer_count": 12,
  "relay_stats": {
    "messages_relayed": 54321,
    "active_circuits": 8
  }
}
```

### Prometheus Metrics

**Key Metrics:**
- `dchat_chain_height`: Current blockchain height
- `dchat_peer_count`: Number of connected peers
- `dchat_block_time_seconds`: Time to produce block
- `dchat_consensus_rounds`: Consensus round duration
- `dchat_relay_messages_total`: Total messages relayed
- `dchat_network_bandwidth_bytes`: Network bandwidth usage

**Scrape Endpoints:**
```
http://validator1-ohio.schikuno.top:9090/metrics
http://validator1-ohio.schikuno.top:9091/metrics (relay1)
http://validator1-ohio.schikuno.top:9092/metrics (relay2)
```

### Alerting Thresholds

**Critical:**
- Validators online < 4/7
- Relays online < 7/14
- Block height diff > 10 blocks
- Peer count < 3

**Warning:**
- Validators online < 6/7
- Relays online < 10/14
- Block time > 10 seconds
- Peer count < 5

## Network Performance

### Expected Latencies

| Path | Expected RTT | Max Acceptable |
|------|--------------|----------------|
| Ohio ↔ Singapore | 220ms | 400ms |
| Ohio ↔ Stockholm | 90ms | 200ms |
| Ohio ↔ São Paulo | 140ms | 300ms |
| Ohio ↔ India | 260ms | 500ms |
| Ohio ↔ South Africa | 290ms | 600ms |
| Ohio ↔ UAE | 240ms | 500ms |
| Singapore ↔ India | 60ms | 150ms |

### Bandwidth Requirements

**Per Validator:**
- P2P traffic: ~5-10 Mbps sustained, 50 Mbps burst
- Relay traffic: ~10-20 Mbps per relay
- Storage sync: ~2-5 Mbps
- Metrics/monitoring: ~0.5 Mbps

**Recommended:** 100 Mbps+ symmetric connection per server

### Block Propagation

**Target:**
- Block production: 6 seconds
- Block propagation: < 1 second to 4/7 validators
- Full network propagation: < 3 seconds

**Optimization:**
- Compact block relay (only send transaction IDs)
- Parallel block propagation to all peers
- Priority P2P lanes for block announcements

## Troubleshooting

### Validator Not Connecting

1. Check DNS resolution:
```bash
nslookup validator1-ohio.schikuno.top
```

2. Check port accessibility:
```bash
nc -zv validator1-ohio.schikuno.top 7070
```

3. Check firewall rules:
```bash
sudo ufw status
```

4. Check validator logs:
```bash
sudo journalctl -u dchat-validator -n 100
```

### Relay Peer Count Low

1. Check if validator is reachable
2. Verify relay service is running:
```bash
sudo systemctl status dchat-relay1 dchat-relay2
```

3. Check for NAT issues (should use relay circuit):
```bash
# Look for "circuit established" in logs
sudo journalctl -u dchat-relay1 | grep circuit
```

### DNS Not Resolving

1. Check Cloudflare DNS records
2. Verify DNS resolver config in `/etc/resolv.conf`
3. Test with Google DNS:
```bash
nslookup validator1-ohio.schikuno.top 8.8.8.8
```

4. Check DNS cache TTL hasn't expired (5 min max)

## Network Upgrades

### Adding New Validators

1. Provision new server
2. Point subdomain to new IP
3. Deploy validator with mainnet config
4. Coordinate with existing validators for governance vote
5. Activate after 2/3 majority approval

### Changing Network Topology

1. Submit governance proposal
2. Coordinate maintenance window
3. Update DNS records
4. Rolling restart of validators (maintain 4/7 quorum)
5. Verify consensus continues

### Emergency Network Halt

If critical vulnerability detected:

1. **Immediate:** Coordinate with all validator operators
2. **Stop validators:** `sudo systemctl stop dchat-validator`
3. **Announce on Discord/Telegram:** "@everyone Network halted for emergency patch"
4. **Deploy patch:** Follow security update procedure
5. **Restart validators:** Coordinate simultaneous restart to maintain genesis state
6. **Verify consensus:** Ensure all validators on same block height

## Related Documentation

- **Validator Operations**: `MAINNET_VALIDATOR_GUIDE.md`
- **Storage Configuration**: `MAINNET_STORAGE_SETUP.md`
- **Monitoring Setup**: `MAINNET_MONITORING.md`
- **Deployment Guide**: `MAINNET_LAUNCH_CRITICAL.md`
- **DNS Discovery Implementation**: `crates/dchat-network/src/dns_discovery.rs`
