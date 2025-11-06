#!/bin/bash
# Configure and start the 3 Azure servers (binaries already uploaded)

set -e

echo "=== dchat 3-Server Configuration & Startup ==="
echo "Date: $(date)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

KEY_DIR="$HOME/.ssh/azure-keys"
SERVERS=(
    "India:74.225.183.196:azureuser:$KEY_DIR/uramami.pem"
    "SouthAfrica:4.221.211.71:azureuser:$KEY_DIR/anacreon.pem"
    "UAE:4.161.34.228:azureuser:$KEY_DIR/Randal_key.pem"
)

P2P_PORT=9090
bootstrap_ip="74.225.183.196"

# Fix permissions on all servers
echo -e "\n${YELLOW}[1/3] Fixing permissions...${NC}"
for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path <<< "$server_info"
    echo -e "${CYAN}   Fixing $name...${NC}"
    ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "sudo chown -R $user:$user /opt/dchat"
    echo -e "${GREEN}   ✅ Done${NC}"
done

# Generate and upload configurations
echo -e "\n${YELLOW}[2/3] Uploading configurations...${NC}"

server_idx=0
for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path <<< "$server_info"
    
    echo -e "${CYAN}   Configuring $name...${NC}"
    
    CONFIG_FILE="/tmp/dchat-config-$name.toml"
    
    cat > "$CONFIG_FILE" << EOF
# dchat Configuration for $name
# Generated: $(date)

[network]
listen_addr = "/ip4/0.0.0.0/tcp/$P2P_PORT"
public_addr = "/ip4/$ip/tcp/$P2P_PORT"

EOF

    # Add bootstrap peers for non-bootstrap nodes
    if [ $server_idx -ne 0 ]; then
        cat >> "$CONFIG_FILE" << EOF
# Bootstrap peers
[[network.bootstrap_peers]]
address = "/ip4/$bootstrap_ip/tcp/$P2P_PORT"

EOF
    fi

    cat >> "$CONFIG_FILE" << EOF
[relay]
enabled = true
max_connections = 100

[storage]
data_dir = "/opt/dchat/data"

[logging]
level = "info"
EOF

    # Upload config
    scp -o StrictHostKeyChecking=no -i "$key_path" "$CONFIG_FILE" "$user@$ip:/opt/dchat/config.toml"
    rm "$CONFIG_FILE"
    
    echo -e "${GREEN}   ✅ Config uploaded${NC}"
    
    ((server_idx++))
done

# Start services
echo -e "\n${YELLOW}[3/3] Starting services...${NC}"

# Start bootstrap node first
echo -e "${CYAN}   Starting bootstrap node: India...${NC}"
ssh -o StrictHostKeyChecking=no -i "${KEY_DIR}/uramami.pem" "azureuser@${bootstrap_ip}" \
    "sudo systemctl restart dchat && sudo systemctl enable dchat"
echo -e "${GREEN}   ✅ Bootstrap node started${NC}"
sleep 10

# Start other nodes
for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path <<< "$server_info"
    
    if [ "$ip" != "$bootstrap_ip" ]; then
        echo -e "${CYAN}   Starting $name...${NC}"
        ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
            "sudo systemctl restart dchat && sudo systemctl enable dchat"
        echo -e "${GREEN}   ✅ Node started${NC}"
        sleep 5
    fi
done

echo -e "\n${GREEN}=== All services started! ===${NC}"
echo -e "${CYAN}Waiting 15 seconds for connections...${NC}"
sleep 15

# Check status
echo -e "\n${YELLOW}=== Checking Status ===${NC}"
for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path <<< "$server_info"
    
    echo -e "\n${CYAN}--- $name ---${NC}"
    ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "echo 'Service Status:' && sudo systemctl status dchat --no-pager | head -5 && echo '' && echo 'Recent Logs:' && sudo journalctl -u dchat -n 15 --no-pager | tail -10" || true
done

echo -e "\n${GREEN}=== Done! ===${NC}"
echo -e "${CYAN}Run connectivity check: wsl bash /mnt/c/Users/USER/dchat/check-3-server-connectivity.sh${NC}"
