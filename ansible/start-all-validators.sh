#!/bin/bash
# Start dchat validator services on all nodes

set -e

# Color codes
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

# Validators configuration
declare -A VALIDATORS=(
    ["ohio"]="18.191.118.167:ubuntu:AWS-Ohio/gecko.pem"
    ["saopaulo"]="54.233.203.82:ubuntu:AWS-Sao-Paulo/pablo.pem"
    ["singapore"]="18.142.96.209:ubuntu:AWS-Singapore/craig.pem"
    ["stockholm"]="13.48.49.2:ubuntu:AWS-Stokholm/relay.pem"
    ["india"]="74.225.183.196:azureuser:Azure-India/uramami.pem"
    ["southafrica"]="4.221.211.71:azureuser:Azure-SAfrica/anacreon.pem"
    ["uae"]="4.161.34.228:azureuser:Azure_UAE/Randal_key.pem"
)

echo -e "${CYAN}=== Starting dchat Validators ===${NC}\n"

start_validator() {
    local region=$1
    local ip=$2
    local user=$3
    local key=$4
    
    echo -e "${YELLOW}[$region] Starting validator on $ip...${NC}"
    
    SSH_OPTS="-i $key -o StrictHostKeyChecking=no -o ConnectTimeout=30"
    
    ssh $SSH_OPTS ${user}@${ip} "bash -s" << 'ENDSSH'
        # Enable and start service
        sudo systemctl enable dchat
        sudo systemctl start dchat
        
        # Wait a moment
        sleep 2
        
        # Check status
        if systemctl is-active --quiet dchat; then
            echo "✅ Validator service is running"
            sudo systemctl status dchat --no-pager -l | head -n 15
        else
            echo "❌ Validator service failed to start"
            sudo journalctl -u dchat -n 50 --no-pager
            exit 1
        fi
ENDSSH
    
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}[$region] ✅ Started successfully${NC}\n"
    else
        echo -e "${RED}[$region] ❌ Failed to start${NC}\n"
    fi
}

# Start all validators
for region in "${!VALIDATORS[@]}"; do
    IFS=':' read -r ip user key <<< "${VALIDATORS[$region]}"
    
    # Find key file
    if [[ -f "$key" ]]; then
        key_path="$key"
    elif [[ -f "../Foundation-servers/$key" ]]; then
        key_path="../Foundation-servers/$key"
    elif [[ -f "/root/dchat-deploy/Foundation-servers/$key" ]]; then
        key_path="/root/dchat-deploy/Foundation-servers/$key"
    else
        echo -e "${RED}[$region] Key not found: $key${NC}"
        continue
    fi
    
    start_validator "$region" "$ip" "$user" "$key_path"
done

echo -e "${CYAN}=== Validator Startup Complete ===${NC}"
echo -e "Monitor logs: ${YELLOW}./monitor-all-validators.sh${NC}\n"
