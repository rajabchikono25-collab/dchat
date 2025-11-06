#!/bin/bash
# Fix systemd service files to use different metrics port

set -e

# Server configuration
declare -A SERVERS=(
    ["india"]="74.225.183.196"
    ["south-africa"]="4.221.211.71"
    ["uae"]="4.161.34.228"
)

declare -A KEYS=(
    ["india"]="~/.ssh/azure-keys/uramami.pem"
    ["south-africa"]="~/.ssh/azure-keys/anacreon.pem"
    ["uae"]="~/.ssh/azure-keys/Randal_key.pem"
)

echo "🔧 Fixing systemd service files (changing metrics port from 9090 to 9091)"
echo "==========================================================================="
echo ""

# Process each server
for server_name in "${!SERVERS[@]}"; do
    SERVER_IP="${SERVERS[$server_name]}"
    KEY_FILE="${KEYS[$server_name]}"
    
    echo "📍 Updating: $server_name ($SERVER_IP)"
    
    # Check if this is the bootstrap node (India)
    if [ "$server_name" = "india" ]; then
        BOOTSTRAP_FLAG=""
    else
        BOOTSTRAP_FLAG="--bootstrap /ip4/74.225.183.196/tcp/9090"
    fi
    
    # Update service file
    ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" << 'ENDSSH'
sudo tee /etc/systemd/system/dchat.service > /dev/null << 'EOF'
[Unit]
Description=dchat Relay Node
After=network.target

[Service]
Type=simple
User=azureuser
WorkingDirectory=/opt/dchat
ExecStart=/opt/dchat/dchat --metrics-addr 127.0.0.1:9091 --health-addr 0.0.0.0:8080 relay --listen 0.0.0.0:9090 BOOTSTRAP_PLACEHOLDER --stake 1000
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

# Replace bootstrap placeholder with actual flag
sudo sed -i "s|BOOTSTRAP_PLACEHOLDER|$BOOTSTRAP_FLAG|" /etc/systemd/system/dchat.service

# Reload systemd and restart service
sudo systemctl daemon-reload
sudo systemctl restart dchat

# Wait a moment
sleep 2

# Check status
if sudo systemctl is-active dchat >/dev/null 2>&1; then
    echo "  ✅ Service running"
else
    echo "  ❌ Service failed to start"
fi
ENDSSH
    
    # Need to pass bootstrap flag to SSH session
    ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "export BOOTSTRAP_FLAG='$BOOTSTRAP_FLAG' && sudo sed -i \"s|BOOTSTRAP_PLACEHOLDER|\$BOOTSTRAP_FLAG|\" /etc/systemd/system/dchat.service"
    
    echo "  ✅ Updated $server_name"
    echo ""
done

echo "✅ All servers updated!"
echo ""
echo "🔍 Checking service status on all servers..."
echo ""

for server_name in "${!SERVERS[@]}"; do
    SERVER_IP="${SERVERS[$server_name]}"
    KEY_FILE="${KEYS[$server_name]}"
    
    echo "📍 $server_name ($SERVER_IP):"
    STATUS=$(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo systemctl is-active dchat 2>/dev/null || echo 'inactive'")
    
    if [ "$STATUS" = "active" ]; then
        echo "  ✅ Active"
        PEER_ID=$(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
            "sudo journalctl -u dchat -n 100 --no-pager 2>/dev/null | grep 'Local peer ID:' | tail -1 | awk '{print \$NF}' || echo 'N/A'")
        echo "  📡 Peer ID: $PEER_ID"
    else
        echo "  ❌ Inactive/Failed"
    fi
    echo ""
done
