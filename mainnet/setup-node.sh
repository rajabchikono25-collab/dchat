#!/bin/bash
# dchat Mainnet Manual Deployment Script
# Run this on each server after copying files

set -e

ROLE=$1
NODE_NAME=$2

if [ -z "$ROLE" ] || [ -z "$NODE_NAME" ]; then
    echo "Usage: $0 <validator|relay|user> <node-name>"
    echo "Example: $0 validator validator-india"
    exit 1
fi

echo "========================================"
echo "  dchat Mainnet Deployment"
echo "  Role: $ROLE"
echo "  Node: $NODE_NAME"
echo "========================================"

# Create directories
echo "[*] Creating directories..."
sudo mkdir -p /opt/dchat/{bin,config,keys,data,logs}
sudo chown -R $USER:$USER /opt/dchat

# Install Rust if not present
if ! command -v cargo &> /dev/null; then
    echo "[*] Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi

# Build dchat from source (assumes source is in /tmp/dchat-src)
if [ -d "/tmp/dchat-src" ]; then
    echo "[*] Building dchat from source..."
    cd /tmp/dchat-src
    cargo build --release --bin dchat
    cp target/release/dchat /opt/dchat/bin/
else
    echo "[!] Source not found at /tmp/dchat-src"
    echo "[!] Please copy source code or pre-built binary to /opt/dchat/bin/dchat"
fi

chmod +x /opt/dchat/bin/dchat

# Copy config if present
if [ -f "/tmp/config.toml" ]; then
    cp /tmp/config.toml /opt/dchat/config/
fi

# Copy identity if present
if [ -f "/tmp/identity.json" ]; then
    cp /tmp/identity.json /opt/dchat/keys/
    chmod 600 /opt/dchat/keys/identity.json
fi

# Open firewall ports
echo "[*] Configuring firewall..."
sudo ufw allow 7070/tcp comment 'dchat p2p'
sudo ufw allow 8080/tcp comment 'dchat health'
sudo ufw allow 9090/tcp comment 'dchat metrics'
sudo ufw allow 26657/tcp comment 'tendermint rpc'
sudo ufw --force enable || true

# Create systemd service
echo "[*] Creating systemd service..."

if [ "$ROLE" == "validator" ]; then
    EXEC_START="/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml validator --key /opt/dchat/keys/identity.json --chain-rpc http://127.0.0.1:26657 --producer --stake 10000"
elif [ "$ROLE" == "relay" ]; then
    EXEC_START="/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml relay --listen 0.0.0.0:7070 --stake 1000"
else
    EXEC_START="/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml user --non-interactive --username dchat-user"
fi

cat << EOF | sudo tee /etc/systemd/system/dchat.service
[Unit]
Description=dchat $ROLE node - $NODE_NAME
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=$USER
Group=$USER
WorkingDirectory=/opt/dchat
Environment=RUST_LOG=info
Environment=RUST_BACKTRACE=1
ExecStart=$EXEC_START
Restart=always
RestartSec=10
LimitNOFILE=65535

StandardOutput=journal
StandardError=journal
SyslogIdentifier=dchat

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable dchat

echo "[✓] Setup complete!"
echo ""
echo "Commands:"
echo "  Start:   sudo systemctl start dchat"
echo "  Stop:    sudo systemctl stop dchat"
echo "  Status:  sudo systemctl status dchat"
echo "  Logs:    sudo journalctl -u dchat -f"
