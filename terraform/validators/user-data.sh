#!/bin/bash
# Validator Node Initialization Script
#
# This script runs on first boot to set up a dchat validator node.
# It installs Docker, configures the validator, and starts the service.

set -e

# Variables from Terraform
ENVIRONMENT="${environment}"
REGION="${region}"

# Logging
exec > >(tee -a /var/log/validator-init.log)
exec 2>&1

echo "=========================================="
echo "dchat Validator Node Initialization"
echo "Environment: $ENVIRONMENT"
echo "Region: $REGION"
echo "Started: $(date)"
echo "=========================================="

# Update system
echo "[1/10] Updating system packages..."
apt-get update -y
apt-get upgrade -y

# Install Docker
echo "[2/10] Installing Docker..."
curl -fsSL https://get.docker.com -o get-docker.sh
sh get-docker.sh
systemctl enable docker
systemctl start docker

# Install Docker Compose
echo "[3/10] Installing Docker Compose..."
curl -L "https://github.com/docker/compose/releases/latest/download/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
chmod +x /usr/local/bin/docker-compose

# Install monitoring tools
echo "[4/10] Installing monitoring tools..."
apt-get install -y prometheus-node-exporter
systemctl enable prometheus-node-exporter
systemctl start prometheus-node-exporter

# Create validator data directory
echo "[5/10] Setting up data directories..."
mkdir -p /data/validator/{keys,config,chain,logs}
chmod 700 /data/validator/keys

# Mount EBS volume (if not already mounted)
echo "[6/10] Configuring storage..."
DEVICE="/dev/nvme1n1"
if [ -b "$DEVICE" ]; then
    if ! blkid "$DEVICE"; then
        echo "Formatting EBS volume..."
        mkfs.ext4 -F "$DEVICE"
    fi
    
    if ! mountpoint -q /data; then
        echo "Mounting EBS volume..."
        mount "$DEVICE" /data
        echo "$DEVICE /data ext4 defaults,nofail 0 2" >> /etc/fstab
    fi
fi

# Generate validator keys
echo "[7/10] Generating validator keys..."
docker run --rm \
    -v /data/validator/keys:/keys \
    dchat/validator:latest \
    keygen --output /keys/validator.key

# Create validator configuration
echo "[8/10] Creating validator configuration..."
cat > /data/validator/config/validator.toml <<EOF
[network]
listen_addresses = [
    "/ip4/0.0.0.0/tcp/7070",
    "/ip4/0.0.0.0/udp/7070/quic-v1"
]

# Will be populated by discovery service
bootstrap_peers = []

[consensus]
required_signatures = 5
min_regions = 3
max_region_percentage = 0.40

[rpc]
bind_address = "0.0.0.0:9545"

[storage]
backend = "rocksdb"
data_dir = "/data/validator/chain"

[logging]
level = "info"
format = "json"

[region]
name = "$REGION"
EOF

# Create systemd service
echo "[9/10] Creating systemd service..."
cat > /etc/systemd/system/dchat-validator.service <<EOF
[Unit]
Description=dchat Validator Node
After=docker.service
Requires=docker.service

[Service]
Type=simple
Restart=always
RestartSec=10
User=root
ExecStartPre=-/usr/bin/docker stop dchat-validator
ExecStartPre=-/usr/bin/docker rm dchat-validator
ExecStart=/usr/bin/docker run --name dchat-validator \\
    --network host \\
    -v /data/validator:/data \\
    -e RUST_LOG=info \\
    dchat/validator:latest \\
    start --config /data/config/validator.toml

ExecStop=/usr/bin/docker stop dchat-validator

[Install]
WantedBy=multi-user.target
EOF

# Start validator service
echo "[10/10] Starting validator service..."
systemctl daemon-reload
systemctl enable dchat-validator
systemctl start dchat-validator

# Configure log rotation
cat > /etc/logrotate.d/dchat-validator <<EOF
/data/validator/logs/*.log {
    daily
    rotate 14
    compress
    delaycompress
    notifempty
    create 0640 root root
    sharedscripts
    postrotate
        systemctl reload dchat-validator > /dev/null 2>&1 || true
    endscript
}
EOF

# Install CloudWatch agent (for metrics)
if [ "$ENVIRONMENT" = "production" ]; then
    echo "Installing CloudWatch agent..."
    wget https://s3.amazonaws.com/amazoncloudwatch-agent/ubuntu/amd64/latest/amazon-cloudwatch-agent.deb
    dpkg -i -E ./amazon-cloudwatch-agent.deb
    
    # Configure CloudWatch agent
    cat > /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.json <<EOF
{
  "metrics": {
    "namespace": "dchat/Validators",
    "metrics_collected": {
      "cpu": {
        "measurement": [
          {
            "name": "cpu_usage_idle",
            "rename": "CPU_IDLE",
            "unit": "Percent"
          }
        ],
        "totalcpu": false
      },
      "disk": {
        "measurement": [
          {
            "name": "used_percent",
            "rename": "DISK_USED",
            "unit": "Percent"
          }
        ],
        "resources": [
          "/data"
        ]
      },
      "mem": {
        "measurement": [
          {
            "name": "mem_used_percent",
            "rename": "MEM_USED",
            "unit": "Percent"
          }
        ]
      }
    }
  }
}
EOF
    
    systemctl enable amazon-cloudwatch-agent
    systemctl start amazon-cloudwatch-agent
fi

echo "=========================================="
echo "Validator initialization complete!"
echo "Finished: $(date)"
echo "=========================================="
echo ""
echo "Validator status:"
systemctl status dchat-validator --no-pager
echo ""
echo "View logs: journalctl -u dchat-validator -f"
