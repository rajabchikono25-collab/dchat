#!/bin/bash
# Get private IPs from all servers

set -e

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

echo "🔍 Getting private IPs from all Azure servers"
echo "=============================================="
echo ""

for server_name in "${!SERVERS[@]}"; do
    SERVER_IP="${SERVERS[$server_name]}"
    KEY_FILE="${KEYS[$server_name]}"
    
    PRIVATE_IP=$(ssh -i "$KEY_FILE" -o StrictHostKeyChecking=no azureuser@"$SERVER_IP" \
        "ip addr show eth0 | grep 'inet ' | awk '{print \$2}' | cut -d/ -f1")
    
    echo "$server_name: Public=$SERVER_IP, Private=$PRIVATE_IP"
done
