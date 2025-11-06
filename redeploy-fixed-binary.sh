#!/bin/bash
# Redeploy fixed dchat binary that respects --listen flag

set -e  # Exit on any error

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

# Binary location in WSL
BINARY="/mnt/c/Users/USER/dchat/target/release/dchat"
REMOTE_PATH="/opt/dchat/dchat"

echo "🚀 Starting redeployment of fixed dchat binary"
echo "================================================"
echo ""

# Verify binary exists
if [ ! -f "$BINARY" ]; then
    echo "❌ Error: Binary not found at $BINARY"
    exit 1
fi

BINARY_SIZE=$(du -h "$BINARY" | cut -f1)
echo "✅ Binary found: $BINARY ($BINARY_SIZE)"
echo ""

# Process each server
for server_name in "${!SERVERS[@]}"; do
    SERVER_IP="${SERVERS[$server_name]}"
    KEY_FILE="${KEYS[$server_name]}"
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "📍 Processing: $server_name ($SERVER_IP)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    # Step 1: Stop the service
    echo "  [1/4] Stopping dchat service..."
    if ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo systemctl stop dchat" 2>/dev/null; then
        echo "  ✅ Service stopped"
    else
        echo "  ⚠️  Service was not running or failed to stop (continuing anyway)"
    fi
    
    # Step 2: Upload new binary
    echo "  [2/4] Uploading new binary..."
    if scp -i "$KEY_FILE" -o StrictHostKeyChecking=no "$BINARY" azureuser@"$SERVER_IP":/tmp/dchat; then
        echo "  ✅ Binary uploaded to /tmp/dchat"
    else
        echo "  ❌ Failed to upload binary to $server_name"
        continue
    fi
    
    # Step 3: Move binary to final location
    echo "  [3/4] Installing binary..."
    if ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo mv /tmp/dchat $REMOTE_PATH && sudo chmod +x $REMOTE_PATH"; then
        echo "  ✅ Binary installed at $REMOTE_PATH"
    else
        echo "  ❌ Failed to install binary on $server_name"
        continue
    fi
    
    # Step 4: Start the service
    echo "  [4/4] Starting dchat service..."
    if ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo systemctl start dchat"; then
        echo "  ✅ Service started"
    else
        echo "  ❌ Failed to start service on $server_name"
        continue
    fi
    
    # Wait a moment for service to initialize
    sleep 2
    
    # Check service status
    echo "  [CHECK] Verifying service status..."
    if ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo systemctl is-active dchat" | grep -q "active"; then
        echo "  ✅ Service is running"
        
        # Get peer ID from logs
        echo "  [INFO] Extracting peer ID..."
        PEER_ID=$(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
            "sudo journalctl -u dchat -n 100 --no-pager 2>/dev/null | grep 'Local peer ID:' | tail -1 | awk '{print \$NF}'")
        
        if [ -n "$PEER_ID" ]; then
            echo "  📡 Peer ID: $PEER_ID"
        fi
    else
        echo "  ❌ Service is not running!"
    fi
    
    echo ""
done

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Redeployment complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "📋 Next steps:"
echo "  1. Check logs: ./check-peer-connections.sh"
echo "  2. Verify listening on port 9090 (not random ports)"
echo ""
