#!/bin/bash
# dchat Validator Bootstrap Script
# Installs Docker, Docker Compose, and deploys storage services

set -e

# Variables from Terraform
HOSTNAME="${hostname}"
REGION="${region}"
MINIO_ROOT_PASSWORD="${minio_root_password}"
REDIS_PASSWORD="${redis_password}"
COCKROACHDB_CONNECTION="${cockroachdb_connection}"

# Set hostname
echo "Setting hostname to $HOSTNAME..."
hostnamectl set-hostname "$HOSTNAME"
echo "127.0.0.1 $HOSTNAME" >> /etc/hosts

# Update system
echo "Updating system packages..."
apt-get update
apt-get upgrade -y

# Install dependencies
echo "Installing dependencies..."
apt-get install -y \
    apt-transport-https \
    ca-certificates \
    curl \
    gnupg \
    lsb-release \
    software-properties-common \
    git \
    vim \
    htop \
    net-tools \
    jq

# Install Docker
echo "Installing Docker..."
curl -fsSL https://get.docker.com -o get-docker.sh
sh get-docker.sh
systemctl enable docker
systemctl start docker

# Install Docker Compose
echo "Installing Docker Compose..."
DOCKER_COMPOSE_VERSION="2.23.0"
curl -L "https://github.com/docker/compose/releases/download/v$${DOCKER_COMPOSE_VERSION}/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
chmod +x /usr/local/bin/docker-compose
ln -sf /usr/local/bin/docker-compose /usr/bin/docker-compose

# Create dchat user
echo "Creating dchat user..."
if ! id -u dchat > /dev/null 2>&1; then
    useradd -m -s /bin/bash dchat
    usermod -aG docker dchat
fi

# Create data directories
echo "Creating data directories..."
mkdir -p /opt/redis/data
mkdir -p /opt/minio/data
mkdir -p /opt/tikv/data
mkdir -p /opt/tikv/pd
mkdir -p /opt/dchat/data
mkdir -p /home/dchat/dchat

chown -R dchat:dchat /opt/redis
chown -R dchat:dchat /opt/minio
chown -R dchat:dchat /opt/tikv
chown -R dchat:dchat /opt/dchat
chown -R dchat:dchat /home/dchat

# Deploy Redis
echo "Deploying Redis..."
docker run -d \
    --name redis \
    --restart unless-stopped \
    -p 6379:6379 \
    -v /opt/redis/data:/data \
    redis:7-alpine \
    redis-server \
    --appendonly yes \
    --maxmemory 2gb \
    --maxmemory-policy allkeys-lru \
    $([ -n "$REDIS_PASSWORD" ] && echo "--requirepass $REDIS_PASSWORD" || echo "")

# Deploy MinIO
echo "Deploying MinIO..."
docker run -d \
    --name minio \
    --restart unless-stopped \
    -p 9000:9000 \
    -p 9001:9001 \
    -e "MINIO_ROOT_USER=dchat_admin" \
    -e "MINIO_ROOT_PASSWORD=$MINIO_ROOT_PASSWORD" \
    -v /opt/minio/data:/data \
    quay.io/minio/aistor/minio:latest \
    server /data --console-address ":9001"

# Wait for MinIO to start
echo "Waiting for MinIO to start..."
sleep 10

# Create MinIO buckets
echo "Creating MinIO buckets..."
docker run --rm \
    --network host \
    -e MINIO_ROOT_USER=dchat_admin \
    -e MINIO_ROOT_PASSWORD=$MINIO_ROOT_PASSWORD \
    quay.io/minio/mc:latest \
    /bin/sh -c "
        mc alias set dchat http://localhost:9000 dchat_admin $MINIO_ROOT_PASSWORD
        mc mb --ignore-existing dchat/dchat-testnet
        mc mb --ignore-existing dchat/dchat-media
        mc mb --ignore-existing dchat/dchat-attachments
        mc mb --ignore-existing dchat/dchat-backups
        mc mb --ignore-existing dchat/dchat-avatars
        mc mb --ignore-existing dchat/dchat-channels
        echo 'MinIO buckets created successfully'
    "

# Create dchat configuration
echo "Creating dchat configuration..."
cat > /home/dchat/dchat/config.toml <<EOF
# dchat Production Configuration - $REGION

[network]
listen_addresses = [
    "/ip4/0.0.0.0/tcp/7070",
    "/ip4/0.0.0.0/udp/7070/quic-v1"
]

# Bootstrap peers (update with actual peer IDs after genesis)
bootstrap_peers = []

max_connections = 200
connection_timeout_ms = 15000
enable_mdns = false
enable_upnp = true

[storage]
data_dir = "/opt/dchat/data"
database_url = "$COCKROACHDB_CONNECTION"
max_message_cache_size = 50000
message_retention_days = 90
enable_backup = true
backup_interval_hours = 6

db_pool_size = 20
db_connection_timeout_secs = 30
db_idle_timeout_secs = 600
db_max_lifetime_secs = 1800
db_enable_wal = true

[storage.minio]
endpoint = "http://localhost:9000"
access_key = "dchat_admin"
secret_key = "$MINIO_ROOT_PASSWORD"
region = "us-east-1"
use_ssl = false

[storage.buckets]
default = "dchat-testnet"
media = "dchat-media"
attachments = "dchat-attachments"
backups = "dchat-backups"
avatars = "dchat-avatars"
channels = "dchat-channels"

[storage.redis]
url = "redis://localhost:6379$([ -n "$REDIS_PASSWORD" ] && echo "?password=$REDIS_PASSWORD" || echo "")"
pool_size = 20
connection_timeout_secs = 5
command_timeout_secs = 3
max_retries = 3

[crypto]
key_rotation_interval_hours = 168
max_messages_per_key = 10000
enable_post_quantum = false
noise_protocol_pattern = "Noise_XX_25519_ChaChaPoly_BLAKE2s"

[governance]
voting_period_hours = 168
minimum_stake_for_proposal = 1000
quorum_threshold = 0.1
enable_anonymous_voting = true

[relay]
enable_relay = true
max_relay_connections = 100
relay_reward_threshold = 100
uptime_reporting_interval_minutes = 10
stake_amount = 5000

[monitoring]
metrics_enabled = true
metrics_port = 9090
tracing_enabled = true
tracing_endpoint = "http://localhost:4317"
health_check_enabled = true
health_check_port = 8080
EOF

chown dchat:dchat /home/dchat/dchat/config.toml

# Create systemd service for dchat validator (placeholder - update with actual binary)
cat > /etc/systemd/system/dchat-validator.service <<EOF
[Unit]
Description=dchat Validator Node
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=dchat
WorkingDirectory=/home/dchat/dchat
# ExecStart=/home/dchat/dchat/dchat-validator --config /home/dchat/dchat/config.toml
# Uncomment above when binary is available
ExecStart=/bin/sleep infinity
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Enable but don't start the service yet (no binary deployed)
systemctl daemon-reload
# systemctl enable dchat-validator.service

# Configure firewall (ufw)
echo "Configuring firewall..."
apt-get install -y ufw
ufw --force enable
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp comment 'SSH'
ufw allow 7070/tcp comment 'dchat P2P TCP'
ufw allow 7070/udp comment 'dchat P2P UDP/QUIC'
ufw allow 8080/tcp comment 'Health Check'
ufw allow 9090/tcp comment 'Metrics'
ufw allow 26656/tcp comment 'Tendermint'
ufw allow 9001/tcp comment 'MinIO Console'

# Create status script
cat > /home/dchat/status.sh <<'EOFSTATUS'
#!/bin/bash
echo "=== dchat Validator Status ==="
echo ""
echo "Hostname: $(hostname)"
echo "Region: ${region}"
echo ""
echo "=== Docker Containers ==="
docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
echo ""
echo "=== Storage Services ==="
echo -n "Redis: "
if docker exec redis redis-cli ping 2>/dev/null | grep -q PONG; then
    echo "✅ Running"
else
    echo "❌ Not responding"
fi
echo -n "MinIO: "
if curl -s http://localhost:9000/minio/health/live > /dev/null 2>&1; then
    echo "✅ Running"
else
    echo "❌ Not responding"
fi
echo ""
echo "=== System Resources ==="
echo "CPU Load: $(uptime | awk -F'load average:' '{print $2}')"
echo "Memory: $(free -h | grep Mem | awk '{print $3 "/" $2}')"
echo "Disk: $(df -h / | tail -1 | awk '{print $3 "/" $2 " (" $5 " used)"}')"
echo ""
echo "=== Network ==="
ip -4 addr show | grep -oP '(?<=inet\s)\d+(\.\d+){3}' | grep -v 127.0.0.1
EOFSTATUS

chmod +x /home/dchat/status.sh
chown dchat:dchat /home/dchat/status.sh

# Success message
echo ""
echo "================================================"
echo "dchat Validator Bootstrap Complete!"
echo "================================================"
echo "Hostname: $HOSTNAME"
echo "Region: $REGION"
echo ""
echo "Services Deployed:"
echo "  - Redis (port 6379)"
echo "  - MinIO (ports 9000, 9001)"
echo ""
echo "Run './status.sh' to check system status"
echo "================================================"
