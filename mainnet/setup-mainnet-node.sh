#!/bin/bash
# Setup script for dchat mainnet nodes
# Installs Redis and creates systemd service

set -e

NODE_TYPE=$1  # validator, relay, or user
USER_NAME=$2  # azureuser or ubuntu

echo "=== Setting up dchat $NODE_TYPE node ==="

# Install Redis
echo "Installing Redis..."
sudo apt-get update
sudo apt-get install -y redis-server

# Configure Redis
sudo systemctl enable redis-server
sudo systemctl start redis-server

# Set permissions on dchat directories
sudo chown -R $USER_NAME:$USER_NAME /opt/dchat
chmod 600 /opt/dchat/keys/identity.json

# Create systemd service based on node type
echo "Creating systemd service..."

if [ "$NODE_TYPE" = "validator" ]; then
    cat << EOF | sudo tee /etc/systemd/system/dchat.service
[Unit]
Description=dchat validator node
After=network.target redis-server.service

[Service]
Type=simple
User=$USER_NAME
WorkingDirectory=/opt/dchat
Environment=RUST_LOG=info
Environment=DCHAT_MINIO_SECRET_KEY=dchat_secret_mainnet_2024
ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml validator --key /opt/dchat/keys/identity.json --producer --stake 10000
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

elif [ "$NODE_TYPE" = "relay" ]; then
    cat << EOF | sudo tee /etc/systemd/system/dchat.service
[Unit]
Description=dchat relay node
After=network.target redis-server.service

[Service]
Type=simple
User=$USER_NAME
WorkingDirectory=/opt/dchat
Environment=RUST_LOG=info
ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml relay --key /opt/dchat/keys/identity.json --listen 0.0.0.0:7070 --stake 1000
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

elif [ "$NODE_TYPE" = "user" ]; then
    cat << EOF | sudo tee /etc/systemd/system/dchat.service
[Unit]
Description=dchat user client
After=network.target redis-server.service

[Service]
Type=simple
User=$USER_NAME
WorkingDirectory=/opt/dchat
Environment=RUST_LOG=info
ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml user --key /opt/dchat/keys/identity.json --non-interactive --username dchat-user
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF
fi

# Reload systemd and enable service
sudo systemctl daemon-reload
sudo systemctl enable dchat

# Configure firewall
echo "Configuring firewall..."
sudo ufw allow 7070/tcp comment 'dchat p2p'
sudo ufw allow 8080/tcp comment 'dchat health'
sudo ufw allow 9090/tcp comment 'dchat metrics'
sudo ufw allow 6379/tcp comment 'redis'
sudo ufw --force enable 2>/dev/null || true

echo "=== Setup complete for $NODE_TYPE ==="
echo "To start: sudo systemctl start dchat"
echo "To check logs: sudo journalctl -u dchat -f"
