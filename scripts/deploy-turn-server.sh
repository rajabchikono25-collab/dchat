#!/bin/bash
# Deploy coturn TURN server
# Usage: ./deploy-turn-server.sh <region> <external-ip>

set -e

REGION=${1:-us-east-1}
EXTERNAL_IP=${2:-auto}
TURN_USER="dchat"
TURN_SECRET=$(openssl rand -hex 32)

echo "🚀 Deploying TURN server in $REGION"

# Install coturn
sudo apt-get update
sudo apt-get install -y coturn

# Auto-detect external IP if needed
if [ "$EXTERNAL_IP" = "auto" ]; then
    EXTERNAL_IP=$(curl -s ifconfig.me)
fi

echo "External IP: $EXTERNAL_IP"

# Configure coturn
sudo tee /etc/turnserver.conf > /dev/null <<EOF
listening-port=3478
tls-listening-port=5349
external-ip=$EXTERNAL_IP
realm=dchat.io
server-name=turn.$REGION.dchat.io

# Authentication
lt-cred-mech
use-auth-secret
static-auth-secret=$TURN_SECRET

# Security
fingerprint
no-multicast-peers
no-loopback-peers

# Logging
log-file=/var/log/turnserver.log
verbose

# Performance
max-bps=1000000
bps-capacity=0

# Relay
relay-ip=$EXTERNAL_IP
EOF

# Enable and start
sudo systemctl enable coturn
sudo systemctl start coturn

# Save credentials
cat > /tmp/turn-credentials.txt <<EOF
TURN Server: turn:$EXTERNAL_IP:3478
Username: $TURN_USER
Secret: $TURN_SECRET
EOF

echo "✅ TURN server deployed at $EXTERNAL_IP:3478"
echo "Credentials saved to /tmp/turn-credentials.txt"
