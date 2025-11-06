# Mainnet Storage Setup Guide

## Overview
dchat mainnet uses a distributed storage architecture combining Redis, MinIO, TiKV, and CockroachDB for different data types and performance requirements.

## Storage Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Application Layer                      │
│  (Validators, Relays, Indexers, API Servers)               │
└────────────┬───────────────┬────────────┬───────────────────┘
             │               │            │
    ┌────────▼─────┐  ┌─────▼──────┐  ┌──▼─────────────┐
    │ Redis Cluster│  │MinIO Cluster│  │  TiKV Cluster  │
    │  (Hot Cache) │  │(Object Store)│  │(Key-Value DB) │
    │   7 Nodes    │  │   7 Nodes   │  │   3 PD Nodes  │
    │  Port 6379   │  │  Port 9000  │  │  Port 2379    │
    └──────────────┘  └─────────────┘  └────────────────┘
                              │
                       ┌──────▼──────────────┐
                       │   CockroachDB       │
                       │  (Cloud-Managed)    │
                       │  (Relational Data)  │
                       └─────────────────────┘
```

## Storage Layer Responsibilities

| Storage | Data Type | Use Case | Consistency | Performance |
|---------|-----------|----------|-------------|-------------|
| **Redis** | Hot cache, session data | Message queues, rate limiting | Eventually consistent | < 1ms reads |
| **MinIO** | Large objects | Media files, backups, snapshots | Strong consistency | S3-compatible |
| **TiKV** | Key-value data | State machine, channel data | Strong consistency | < 10ms reads |
| **CockroachDB** | Relational data | User accounts, governance | Serializable | < 50ms queries |

## 1. Redis Cluster Setup

### Architecture
- **Mode**: Redis Cluster (not Sentinel)
- **Nodes**: 7 (one per validator server)
- **Replication**: 3 replicas per shard
- **Shards**: 16384 hash slots distributed across 7 nodes

### Installation (per server)

```bash
# Install Redis
sudo apt-get update
sudo apt-get install -y redis-server redis-tools

# Configure Redis for cluster mode
sudo tee /etc/redis/redis.conf > /dev/null <<EOF
# Network
bind 0.0.0.0
protected-mode no
port 6379

# Cluster
cluster-enabled yes
cluster-config-file nodes-6379.conf
cluster-node-timeout 15000

# Persistence
appendonly yes
appendfilename "appendonly.aof"
appendfsync everysec

# Memory
maxmemory 4gb
maxmemory-policy allkeys-lru

# Performance
tcp-backlog 511
timeout 0
tcp-keepalive 300
EOF

# Start Redis
sudo systemctl enable redis-server
sudo systemctl start redis-server
```

### Cluster Initialization

Run this **once** from any server (e.g., Ohio):

```bash
# Create cluster with 7 nodes
redis-cli --cluster create \
  validator1-ohio.schikuno.top:6379 \
  validator1-singapore.schikuno.top:6379 \
  validator1-stockholm.schikuno.top:6379 \
  validator1-saopaulo.schikuno.top:6379 \
  validator1-india.schikuno.top:6379 \
  validator1-southafrica.schikuno.top:6379 \
  validator1-uae.schikuno.top:6379 \
  --cluster-replicas 0

# Add replicas (after initial cluster is up)
redis-cli --cluster add-node \
  validator1-singapore.schikuno.top:6379 \
  validator1-ohio.schikuno.top:6379 \
  --cluster-slave

redis-cli --cluster add-node \
  validator1-stockholm.schikuno.top:6379 \
  validator1-ohio.schikuno.top:6379 \
  --cluster-slave
```

### Health Check

```bash
# Check cluster status
redis-cli -c -h validator1-ohio.schikuno.top -p 6379 cluster info

# Expected output:
# cluster_state:ok
# cluster_slots_assigned:16384
# cluster_slots_ok:16384
# cluster_known_nodes:7

# Check node health
redis-cli -c -h validator1-ohio.schikuno.top -p 6379 cluster nodes
```

### Systemd Service

Create `/etc/systemd/system/redis-cluster.service`:

```ini
[Unit]
Description=Redis Cluster Node
After=network.target

[Service]
Type=notify
User=redis
Group=redis
ExecStart=/usr/bin/redis-server /etc/redis/redis.conf --supervised systemd
ExecStop=/bin/redis-cli shutdown
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

Enable:
```bash
sudo systemctl daemon-reload
sudo systemctl enable redis-cluster
sudo systemctl start redis-cluster
```

### Application Integration

```rust
// In dchat application
use redis::cluster::ClusterClient;

let nodes = vec![
    "redis://validator1-ohio.schikuno.top:6379",
    "redis://validator1-singapore.schikuno.top:6379",
    "redis://validator1-stockholm.schikuno.top:6379",
    "redis://validator1-saopaulo.schikuno.top:6379",
    "redis://validator1-india.schikuno.top:6379",
    "redis://validator1-southafrica.schikuno.top:6379",
    "redis://validator1-uae.schikuno.top:6379",
];

let client = ClusterClient::new(nodes)?;
let mut connection = client.get_connection()?;

// Use connection for caching
redis::cmd("SET")
    .arg("message:12345")
    .arg(b"encrypted_message_payload")
    .query::<()>(&mut connection)?;
```

## 2. MinIO Distributed Setup

### Architecture
- **Mode**: Distributed MinIO (SNSD - Single Namespace Distributed Deployment)
- **Nodes**: 7 servers
- **Erasure Coding**: EC:4 (survives 3 node failures)
- **Consistency**: Strong (read-after-write)

### Installation (per server)

```bash
# Download MinIO
wget https://dl.min.io/server/minio/release/linux-amd64/minio
chmod +x minio
sudo mv minio /usr/local/bin/

# Create data directory
sudo mkdir -p /mnt/minio-data
sudo chown azureuser:azureuser /mnt/minio-data

# Set environment variables
sudo tee /etc/default/minio > /dev/null <<EOF
MINIO_ROOT_USER=dchat_admin
MINIO_ROOT_PASSWORD=$(openssl rand -base64 32)
MINIO_VOLUMES="http://validator1-{ohio,singapore,stockholm,saopaulo,india,southafrica,uae}.schikuno.top/mnt/minio-data"
MINIO_SERVER_URL="http://validator1-ohio.schikuno.top:9000"
MINIO_OPTS="--console-address :9001"
EOF
```

### Systemd Service

Create `/etc/systemd/system/minio.service`:

```ini
[Unit]
Description=MinIO Distributed Object Storage
After=network.target

[Service]
Type=notify
User=azureuser
Group=azureuser
EnvironmentFile=/etc/default/minio
ExecStart=/usr/local/bin/minio server $MINIO_OPTS $MINIO_VOLUMES
Restart=always
RestartSec=10
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

Enable on all servers:
```bash
sudo systemctl daemon-reload
sudo systemctl enable minio
sudo systemctl start minio
```

### Cluster Initialization

Run from any server:

```bash
# Install MinIO client
wget https://dl.min.io/client/mc/release/linux-amd64/mc
chmod +x mc
sudo mv mc /usr/local/bin/

# Configure alias
mc alias set dchat-minio http://validator1-ohio.schikuno.top:9000 \
  dchat_admin <password_from_env>

# Create buckets
mc mb dchat-minio/messages
mc mb dchat-minio/media
mc mb dchat-minio/backups
mc mb dchat-minio/snapshots

# Set policies
mc anonymous set download dchat-minio/media
mc anonymous set none dchat-minio/messages
mc anonymous set none dchat-minio/backups
```

### Health Check

```bash
# Check cluster status
mc admin info dchat-minio

# Expected output shows all 7 nodes online

# Check bucket list
mc ls dchat-minio
```

### Application Integration

```rust
use aws_sdk_s3::{Client, Config, Credentials, Region};

let config = Config::builder()
    .region(Region::new("us-east-1"))
    .endpoint_url("http://validator1-ohio.schikuno.top:9000")
    .credentials_provider(Credentials::new(
        "dchat_admin",
        env::var("DCHAT_MINIO_SECRET_KEY")?,
        None,
        None,
        "dchat-minio",
    ))
    .build();

let client = Client::from_conf(config);

// Upload media file
client
    .put_object()
    .bucket("media")
    .key("image_12345.jpg")
    .body(file_bytes.into())
    .send()
    .await?;
```

## 3. TiKV Cluster Setup

### Architecture
- **Placement Driver (PD)**: 3 nodes (Ohio, Singapore, Stockholm)
- **TiKV Servers**: 7 nodes (all validator servers)
- **Replication**: 3 replicas per region
- **Consistency**: Linearizable

### Installation - PD Nodes (Ohio, Singapore, Stockholm)

```bash
# Download TiUP (TiKV installer)
curl --proto '=https' --tlsv1.2 -sSf https://tiup-mirrors.pingcap.com/install.sh | sh
source ~/.bashrc

# Install PD
tiup cluster deploy tikv-dchat v7.5.0 topology.yaml --user azureuser -i ~/.ssh/id_rsa
```

**topology.yaml** (create this file):

```yaml
global:
  user: "azureuser"
  ssh_port: 22
  deploy_dir: "/opt/tikv"
  data_dir: "/mnt/tikv-data"

pd_servers:
  - host: validator1-ohio.schikuno.top
    port: 2379
    peer_port: 2380
  - host: validator1-singapore.schikuno.top
    port: 2379
    peer_port: 2380
  - host: validator1-stockholm.schikuno.top
    port: 2379
    peer_port: 2380

tikv_servers:
  - host: validator1-ohio.schikuno.top
    port: 20160
    status_port: 20180
  - host: validator1-singapore.schikuno.top
    port: 20160
    status_port: 20180
  - host: validator1-stockholm.schikuno.top
    port: 20160
    status_port: 20180
  - host: validator1-saopaulo.schikuno.top
    port: 20160
    status_port: 20180
  - host: validator1-india.schikuno.top
    port: 20160
    status_port: 20180
  - host: validator1-southafrica.schikuno.top
    port: 20160
    status_port: 20180
  - host: validator1-uae.schikuno.top
    port: 20160
    status_port: 20180

monitoring_servers:
  - host: validator1-ohio.schikuno.top

grafana_servers:
  - host: validator1-ohio.schikuno.top

alertmanager_servers:
  - host: validator1-ohio.schikuno.top
```

### Cluster Initialization

```bash
# Start cluster
tiup cluster start tikv-dchat

# Check cluster status
tiup cluster display tikv-dchat

# Check PD
curl http://validator1-ohio.schikuno.top:2379/pd/health
```

### Systemd Service (auto-created by TiUP)

Services are created at:
- `/etc/systemd/system/tikv-pd.service` (PD nodes only)
- `/etc/systemd/system/tikv-server.service` (all nodes)

### Health Check

```bash
# Check PD cluster
curl http://validator1-ohio.schikuno.top:2379/pd/api/v1/members

# Check TiKV stores
curl http://validator1-ohio.schikuno.top:2379/pd/api/v1/stores
```

### Application Integration

```rust
use tikv_client::{Config, RawClient};

let pd_endpoints = vec![
    "validator1-ohio.schikuno.top:2379",
    "validator1-singapore.schikuno.top:2379",
    "validator1-stockholm.schikuno.top:2379",
];

let client = RawClient::new(pd_endpoints).await?;

// Put key-value
client.put("channel:123".to_owned(), b"channel_data".to_vec()).await?;

// Get key-value
let value = client.get("channel:123".to_owned()).await?;
```

## 4. CockroachDB Cloud Setup

### Architecture
- **Mode**: Cloud-managed (CockroachDB Dedicated or Serverless)
- **Regions**: Multi-region deployment (Ohio, Singapore, Stockholm)
- **Replication**: 3 replicas
- **Consistency**: Serializable

### Setup (CockroachDB Cloud Console)

1. Create cluster at https://cockroachlabs.cloud/
2. Select "Multi-Region" deployment
3. Choose regions: Ohio, Singapore, Stockholm
4. Configure networking: Allow all validator IPs
5. Generate service account and connection string

### Connection String

```
postgresql://dchat_user:<password>@<cluster-id>.aws-us-east-1.cockroachlabs.cloud:26257/dchat?sslmode=verify-full
```

Store in environment variable:
```bash
export DCHAT_COCKROACH_URL="postgresql://..."
```

### Schema Initialization

```sql
-- Create database
CREATE DATABASE dchat;
USE dchat;

-- User accounts
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username VARCHAR(32) UNIQUE NOT NULL,
    public_key BYTEA NOT NULL,
    created_at TIMESTAMP DEFAULT now(),
    INDEX idx_username (username)
);

-- Channels
CREATE TABLE channels (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(64) NOT NULL,
    owner_id UUID REFERENCES users(id),
    created_at TIMESTAMP DEFAULT now(),
    member_count INT DEFAULT 0,
    INDEX idx_owner (owner_id)
);

-- Governance proposals
CREATE TABLE proposals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title VARCHAR(256) NOT NULL,
    description TEXT,
    proposer_id UUID REFERENCES users(id),
    status VARCHAR(20) DEFAULT 'pending',
    votes_for INT DEFAULT 0,
    votes_against INT DEFAULT 0,
    created_at TIMESTAMP DEFAULT now()
);

-- Moderation logs
CREATE TABLE moderation_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    action VARCHAR(32) NOT NULL,
    target_id UUID NOT NULL,
    moderator_id UUID REFERENCES users(id),
    reason TEXT,
    timestamp TIMESTAMP DEFAULT now()
);
```

### Application Integration

```rust
use sqlx::{Pool, Postgres};

let pool = Pool::<Postgres>::connect(&env::var("DCHAT_COCKROACH_URL")?).await?;

// Query users
let users = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
    .bind("alice")
    .fetch_all(&pool)
    .await?;
```

## Storage Monitoring

### Metrics to Track

**Redis:**
- `redis_connected_clients`: Current connections
- `redis_used_memory_bytes`: Memory usage
- `redis_cluster_state`: Cluster health
- `redis_commands_processed_total`: Throughput

**MinIO:**
- `minio_cluster_nodes_online`: Node availability
- `minio_cluster_capacity_usable_total_bytes`: Available space
- `minio_s3_requests_total`: Request rate
- `minio_bucket_objects_size_bytes`: Bucket sizes

**TiKV:**
- `tikv_engine_size_bytes`: Storage usage
- `tikv_thread_cpu_seconds_total`: CPU usage
- `tikv_grpc_msg_duration_seconds`: Request latency
- `pd_cluster_status`: PD cluster health

**CockroachDB:**
- `sql_conns`: Active connections
- `sql_query_count`: Query throughput
- `replication_quiescent`: Replication lag
- `capacity_available`: Available storage

### Prometheus Configuration

Add to `prometheus.yml`:

```yaml
scrape_configs:
  # Redis
  - job_name: 'redis'
    static_configs:
      - targets:
        - validator1-ohio.schikuno.top:6379
        - validator1-singapore.schikuno.top:6379
        # ... all 7 nodes
  
  # MinIO
  - job_name: 'minio'
    metrics_path: /minio/v2/metrics/cluster
    static_configs:
      - targets:
        - validator1-ohio.schikuno.top:9000
  
  # TiKV
  - job_name: 'tikv'
    static_configs:
      - targets:
        - validator1-ohio.schikuno.top:20180
        # ... all 7 nodes
  
  # TiKV PD
  - job_name: 'pd'
    static_configs:
      - targets:
        - validator1-ohio.schikuno.top:2379
        - validator1-singapore.schikuno.top:2379
        - validator1-stockholm.schikuno.top:2379
```

## Backup & Recovery

### Redis Backups

```bash
# Enable RDB snapshots in redis.conf
save 900 1
save 300 10
save 60 10000

# Manual backup
redis-cli --rdb /backup/redis-$(date +%F).rdb

# Restore
sudo cp /backup/redis-2024-01-15.rdb /var/lib/redis/dump.rdb
sudo systemctl restart redis-server
```

### MinIO Backups

```bash
# Mirror bucket to backup location
mc mirror dchat-minio/messages s3://backup-bucket/messages

# Schedule with cron
0 2 * * * mc mirror dchat-minio/messages s3://backup-bucket/messages
```

### TiKV Backups

```bash
# Use BR (Backup & Restore)
tiup br backup full --pd "validator1-ohio.schikuno.top:2379" \
  --storage "s3://dchat-backups/tikv/$(date +%F)" \
  --s3.region "us-east-1"
```

### CockroachDB Backups

```sql
-- Automatic in CockroachDB Cloud
-- Manual backup:
BACKUP INTO 's3://dchat-backups/cockroach?AWS_ACCESS_KEY_ID=xxx&AWS_SECRET_ACCESS_KEY=yyy';
```

## Troubleshooting

### Redis Cluster Split-Brain

```bash
# Check cluster state on all nodes
for host in ohio singapore stockholm saopaulo india southafrica uae; do
  echo "=== $host ==="
  redis-cli -h validator1-$host.schikuno.top cluster nodes | grep myself
done

# Fix split-brain by resetting and recreating cluster
```

### MinIO Node Offline

```bash
# Check node status
mc admin info dchat-minio

# Restart node
sudo systemctl restart minio

# Check healing status
mc admin heal dchat-minio
```

### TiKV Store Down

```bash
# Check store status
curl http://validator1-ohio.schikuno.top:2379/pd/api/v1/stores | jq '.stores[] | select(.store.state_name != "Up")'

# Restart TiKV
sudo systemctl restart tikv-server
```

## Related Documentation

- **Network Architecture**: `MAINNET_NETWORK_CONFIG.md`
- **Validator Operations**: `MAINNET_VALIDATOR_GUIDE.md`
- **Monitoring Setup**: `MAINNET_MONITORING.md`
- **Deployment Guide**: `MAINNET_LAUNCH_CRITICAL.md`
