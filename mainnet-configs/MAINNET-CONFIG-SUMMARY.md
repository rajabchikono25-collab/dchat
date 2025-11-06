# Mainnet Configuration Summary

**Generated**: 2025-11-06 14:54:35 UTC
**Total Validators**: 7
**Network**: dchat-mainnet-1

## Validator Configurations

| Region | Provider | Subdomain | Public Key |
|--------|----------|-----------|------------|
| ohio | AWS | validator1-ohio.schikuno.top | `e26a3592fbffe18419e083e5728935cd17d4b68b67f36dd1bc16d6c60baccf2a` |
| singapore | AWS | validator1-singapore.schikuno.top | `14b66a5114968f83f8496340f13dbc64a1f1ad68b8a17f911610cb422e25ec30` |
| stockholm | AWS | validator1-stockholm.schikuno.top | `8438736db20a2a800780680c3cb7823bd217d1a53873cab275d1dd54c20e8185` |
| saopaulo | AWS | validator1-saopaulo.schikuno.top | `0fc82f04eaec83cca7dea8a732e1aa711e5587e5a5be1bb09acc428fe44e8411` |
| india | Azure | validator1-india.schikuno.top | `1e3e684e97aa00bc5d22084d2ca75cfe911ee6fb90e1277589d40b8ab8799579` |
| southafrica | Azure | validator1-southafrica.schikuno.top | `b709ae1e971d1d9b77a092c09f9718065a23c72bc83c7077c49686d17570c62c` |
| uae | Azure | validator1-uae.schikuno.top | `b8b88bdca8c84479c0bb1f872c8a62890d87bed2b21b76c0b160c9812dcc1494` |

## DNS Discovery Configuration

- **Base Domain**: schikuno.top
- **DNS Servers**: Cloudflare (1.1.1.1), Google (8.8.8.8)
- **Cache TTL**: 5 minutes
- **Refresh Interval**: 60 seconds

## Validator Subdomains

- validator1-ohio.schikuno.top
- validator1-singapore.schikuno.top
- validator1-stockholm.schikuno.top
- validator1-saopaulo.schikuno.top
- validator1-india.schikuno.top
- validator1-southafrica.schikuno.top
- validator1-uae.schikuno.top

## Network Ports

### Validators
- **TCP**: 7070
- **WebSocket**: 443
- **HTTP**: 80

### Relays (per validator)
- **Relay 1**: 7071
- **Relay 2**: 7072

### Monitoring
- **Prometheus**: 9090
- **Health Check**: 8080

### Storage Clusters
- **Redis**: 6379
- **MinIO**: 9000
- **TiKV PD**: 2379
- **TiKV Server**: 20160

## Consensus Configuration

- **Minimum Validators**: 4 of 7 (BFT)
- **Block Time**: 6 seconds
- **Max Block Size**: 1 MB

## Storage Configuration

### Redis Cluster
- Mode: Cluster
- Nodes: 7 (one per validator)
- Port: 6379

### MinIO Distributed
- Mode: Distributed
- Nodes: 7 (one per validator)
- Port: 9000

### TiKV Cluster
- PD Nodes: 3 (Ohio, Singapore, Stockholm)
- Port: 2379 (PD), 20160 (Server)

### CockroachDB
- Type: Cloud-managed
- Connection: TLS required

## Deployment Order

1. **Ohio** (AWS US East) - First validator
2. **Singapore** (AWS Asia Pacific) - Second validator
3. **Stockholm** (AWS Europe) - Third validator
4. **São Paulo** (AWS South America) - Fourth validator (consensus reached)
5. **India** (Azure Central India) - Fifth validator
6. **South Africa** (Azure South Africa North) - Sixth validator
7. **UAE** (Azure UAE North) - Seventh validator

## Environment Variables Required

Each server needs these environment variables set:

\\\ash
# Redis password
export REDIS_PASSWORD="<secure-password>"

# MinIO credentials
export MINIO_ACCESS_KEY="<access-key>"
export MINIO_SECRET_KEY="<secret-key>"

# CockroachDB connection
export COCKROACH_CONNECTION_STRING="<connection-string>"
\\\

## Configuration Files Generated

- config-mainnet-ohio.toml
- config-mainnet-singapore.toml
- config-mainnet-stockholm.toml
- config-mainnet-saopaulo.toml
- config-mainnet-india.toml
- config-mainnet-southafrica.toml
- config-mainnet-uae.toml

## Next Steps

1. Review all configuration files
2. Set environment variables on each server
3. Copy config files to servers: \/etc/dchat/config.toml\
4. Copy validator keys to servers: \/etc/dchat/keys/validator.key\
5. Start validators one-by-one using deployment script
6. Monitor consensus formation

---

**Generated**: 2025-11-06 14:54:36 UTC
