#!/bin/bash
# Deploy dchat bootstrap node
# Usage: ./deploy-bootstrap-node.sh <region> <node-name>

set -e

REGION=${1:-us-east-1}
NODE_NAME=${2:-bootstrap-1}
PORT=30303

echo "🚀 Deploying bootstrap node: $NODE_NAME in $REGION"

# Build release binary
cargo build --release --bin dchat-relay

# Generate node identity
NODE_ID=$(openssl rand -hex 32)
echo "Node ID: $NODE_ID"

# Create systemd service
cat > /tmp/dchat-bootstrap.service <<EOF
[Unit]
Description=dchat Bootstrap Node
After=network.target

[Service]
Type=simple
User=dchat
WorkingDirectory=/opt/dchat
ExecStart=/opt/dchat/dchat-relay --mode bootstrap --port $PORT --node-id $NODE_ID
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Deploy
sudo mkdir -p /opt/dchat
sudo cp target/release/dchat-relay /opt/dchat/
sudo cp /tmp/dchat-bootstrap.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable dchat-bootstrap
sudo systemctl start dchat-bootstrap

echo "✅ Bootstrap node deployed"
echo "Peer ID: 12D3Koo$(echo -n $NODE_ID | sha256sum | base58 | head -c 44)"
