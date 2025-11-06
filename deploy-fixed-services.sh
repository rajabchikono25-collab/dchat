#!/bin/bash
# Deploy corrected systemd service files with metrics on port 9091

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

declare -A SERVICE_FILES=(
    ["india"]="/mnt/c/Users/USER/dchat/dchat-india.service"
    ["south-africa"]="/mnt/c/Users/USER/dchat/dchat-south-africa.service"
    ["uae"]="/mnt/c/Users/USER/dchat/dchat-uae.service"
)

echo "🔧 Deploying corrected systemd service files"
echo "============================================="
echo ""

# Process each server
for server_name in "${!SERVERS[@]}"; do
    SERVER_IP="${SERVERS[$server_name]}"
    KEY_FILE="${KEYS[$server_name]}"
    SERVICE_FILE="${SERVICE_FILES[$server_name]}"
    
    echo "📍 Processing: $server_name ($SERVER_IP)"
    
    # Stop service
    echo "  [1/4] Stopping service..."
    ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo systemctl stop dchat" 2>/dev/null || echo "  (service was not running)"
    
    # Upload service file
    echo "  [2/4] Uploading service file..."
    scp -i "$KEY_FILE" -o StrictHostKeyChecking=no "$SERVICE_FILE" azureuser@"$SERVER_IP":/tmp/dchat.service
    
    # Install service file
    echo "  [3/4] Installing service file..."
    ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo mv /tmp/dchat.service /etc/systemd/system/dchat.service && sudo chmod 644 /etc/systemd/system/dchat.service"
    
    # Reload and start
    echo "  [4/4] Starting service..."
    ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo systemctl daemon-reload && sudo systemctl start dchat"
    
    # Wait for startup
    sleep 3
    
    # Check status
    STATUS=$(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo systemctl is-active dchat 2>/dev/null || echo 'inactive'")
    
    if [ "$STATUS" = "active" ]; then
        echo "  ✅ Service is running"
        
        # Get peer ID
        PEER_ID=$(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
            "sudo journalctl -u dchat -n 100 --no-pager 2>/dev/null | grep 'Local peer ID:' | tail -1 | awk '{print \$NF}' || echo 'N/A'")
        echo "  📡 Peer ID: $PEER_ID"
    else
        echo "  ❌ Service failed to start!"
        echo "  📋 Last 10 log lines:"
        ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
            "sudo journalctl -u dchat -n 10 --no-pager" | sed 's/^/      /'
    fi
    
    echo ""
done

echo "✅ Deployment complete!"
echo ""
echo "🔍 Waiting 10 seconds for P2P connections..."
sleep 10

echo ""
echo "📊 Final status check:"
echo "====================="
echo ""

for server_name in "${!SERVERS[@]}"; do
    SERVER_IP="${SERVERS[$server_name]}"
    KEY_FILE="${KEYS[$server_name]}"
    
    echo "📍 $server_name ($SERVER_IP):"
    
    # Get listening addresses
    echo "  Listening addresses:"
    ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo journalctl -u dchat -n 50 --no-pager 2>/dev/null | grep 'Listening on:' | tail -3" | \
        sed 's/^/    /'
    
    # Check for peer connections
    PEER_COUNT=$(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "sudo journalctl -u dchat -n 100 --no-pager 2>/dev/null | grep 'Connection established with peer' | wc -l")
    
    echo "  Connected peers: $PEER_COUNT"
    echo ""
done

echo "📋 Next step: Run ./check-peer-connections.sh to verify P2P mesh"
