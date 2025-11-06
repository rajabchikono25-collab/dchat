#!/bin/bash
# Deploy dchat binary to 3 Azure servers via WSL

set -e

echo "=== dchat 3-Server Deployment Script (WSL) ==="
echo "Date: $(date)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Server configuration
KEY_DIR="$HOME/.ssh/azure-keys"
SERVERS=(
    "India:74.225.183.196:azureuser:$KEY_DIR/uramami.pem:validator1-india.schikuno.top"
    "SouthAfrica:4.221.211.71:azureuser:$KEY_DIR/anacreon.pem:validator1-southafrica.schikuno.top"
    "UAE:4.161.34.228:azureuser:$KEY_DIR/Randal_key.pem:validator1-uae.schikuno.top"
)

# Binary path
BINARY_PATH="/mnt/c/Users/USER/dchat/target/release/dchat"
P2P_PORT=9090

# Check binary exists
echo -e "\n${YELLOW}[1/6] Checking binary...${NC}"
if [ ! -f "$BINARY_PATH" ]; then
    echo -e "${RED}❌ Binary not found at $BINARY_PATH${NC}"
    exit 1
fi
BINARY_SIZE=$(du -h "$BINARY_PATH" | cut -f1)
echo -e "${GREEN}✅ Binary found ($BINARY_SIZE)${NC}"

# Check SSH keys
echo -e "\n${YELLOW}[2/6] Checking SSH keys...${NC}"
for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path domain <<< "$server_info"
    if [ ! -f "$key_path" ]; then
        echo -e "${RED}❌ SSH key not found: $key_path${NC}"
        exit 1
    fi
    echo -e "${GREEN}✅ $name: Key found${NC}"
done

# Upload binary to all servers
echo -e "\n${YELLOW}[3/6] Uploading binary to servers...${NC}"
for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path domain <<< "$server_info"
    
    echo -e "\n${CYAN}   Uploading to $name ($ip)...${NC}"
    
    # Create directory
    ssh -o StrictHostKeyChecking=no -o ConnectTimeout=10 -i "$key_path" "$user@$ip" \
        "sudo mkdir -p /opt/dchat && sudo chown $user:$user /opt/dchat"
    
    # Upload binary
    scp -o StrictHostKeyChecking=no -i "$key_path" "$BINARY_PATH" "$user@$ip:/opt/dchat/dchat"
    
    # Make executable
    ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "chmod +x /opt/dchat/dchat"
    
    echo -e "${GREEN}   ✅ Upload complete${NC}"
done

# Get bootstrap node info (first server)
IFS=':' read -r bootstrap_name bootstrap_ip bootstrap_user bootstrap_key bootstrap_domain <<< "${SERVERS[0]}"

# Generate and upload configurations
echo -e "\n${YELLOW}[4/6] Generating configurations...${NC}"

server_idx=0
for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path domain <<< "$server_info"
    
    echo -e "\n${CYAN}   Configuring $name...${NC}"
    
    # Create config file
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
    
    echo -e "${GREEN}   ✅ Configuration uploaded${NC}"
    
    ((server_idx++))
done

# Create and upload systemd service
echo -e "\n${YELLOW}[5/6] Setting up systemd services...${NC}"

SERVICE_FILE="/tmp/dchat.service"
cat > "$SERVICE_FILE" << 'EOF'
[Unit]
Description=dchat Relay Node
After=network.target

[Service]
Type=simple
User=azureuser
WorkingDirectory=/opt/dchat
ExecStart=/opt/dchat/dchat --config /opt/dchat/config.toml
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path domain <<< "$server_info"
    
    echo -e "\n${CYAN}   Setting up service on $name...${NC}"
    
    # Upload and install service
    scp -o StrictHostKeyChecking=no -i "$key_path" "$SERVICE_FILE" "$user@$ip:/tmp/dchat.service"
    ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "sudo mv /tmp/dchat.service /etc/systemd/system/dchat.service && sudo systemctl daemon-reload"
    
    echo -e "${GREEN}   ✅ Service installed${NC}"
done

rm "$SERVICE_FILE"

# Start services
echo -e "\n${YELLOW}[6/6] Starting services...${NC}"

# Start bootstrap node first
echo -e "\n${CYAN}   Starting bootstrap node: $bootstrap_name...${NC}"
ssh -o StrictHostKeyChecking=no -i "$bootstrap_key" "$bootstrap_user@$bootstrap_ip" \
    "sudo systemctl enable dchat && sudo systemctl restart dchat"
echo -e "${GREEN}   ✅ Bootstrap node started${NC}"
echo -e "${CYAN}   Waiting 10 seconds for bootstrap node to initialize...${NC}"
sleep 10

# Start other nodes
server_idx=0
for server_info in "${SERVERS[@]}"; do
    if [ $server_idx -ne 0 ]; then
        IFS=':' read -r name ip user key_path domain <<< "$server_info"
        
        echo -e "\n${CYAN}   Starting $name...${NC}"
        ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
            "sudo systemctl enable dchat && sudo systemctl restart dchat"
        echo -e "${GREEN}   ✅ Node started${NC}"
        sleep 5
    fi
    ((server_idx++))
done

# Verification
echo -e "\n${CYAN}=== Verification ===${NC}"
echo -e "${CYAN}Waiting 15 seconds for nodes to establish connections...${NC}"
sleep 15

for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path domain <<< "$server_info"
    
    echo -e "\n${YELLOW}--- $name Status ---${NC}"
    
    echo -e "${CYAN}Service Status:${NC}"
    ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "sudo systemctl status dchat --no-pager | head -n 10" || true
    
    echo -e "\n${CYAN}Recent Logs:${NC}"
    ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "sudo journalctl -u dchat -n 20 --no-pager" || true
done

echo -e "\n${GREEN}=== Deployment Complete ===${NC}"
echo -e "
${CYAN}Next Steps:${NC}
1. Monitor logs: ssh <user>@<server> 'sudo journalctl -u dchat -f'
2. Check connections: Look for 'peer connected' or 'handshake' messages
3. Verify network: All 3 nodes should discover each other within 1-2 minutes

${CYAN}Server Addresses:${NC}
- India:        $bootstrap_ip ($bootstrap_domain)
- South Africa: 4.221.211.71 (validator1-southafrica.schikuno.top)
- UAE:          4.161.34.228 (validator1-uae.schikuno.top)

${CYAN}To check connectivity:${NC}
  wsl bash check-3-server-connectivity.sh
"
