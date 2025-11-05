#!/bin/bash
# Check status of all dchat validators

set -e

# Color codes
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

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

echo -e "${CYAN}=== dchat Foundation Validators Status Check ===${NC}\n"

check_validator() {
    local region=$1
    local ip=$2
    local user=$3
    local key=$4
    
    echo -e "${YELLOW}[$region] Checking $ip...${NC}"
    
    # SSH options
    SSH_OPTS="-i $key -o StrictHostKeyChecking=no -o ConnectTimeout=10"
    
    # Check status
    if ssh $SSH_OPTS ${user}@${ip} '/opt/dchat/status.sh' 2>/dev/null; then
        echo -e "${GREEN}[$region] ✅ Status check complete${NC}\n"
    else
        echo -e "${RED}[$region] ❌ Failed to check status${NC}\n"
    fi
}

# Check all validators
for region in "${!VALIDATORS[@]}"; do
    IFS=':' read -r ip user key <<< "${VALIDATORS[$region]}"
    
    # Adjust key path
    if [[ -f "$key" ]]; then
        key_path="$key"
    elif [[ -f "../Foundation-servers/$key" ]]; then
        key_path="../Foundation-servers/$key"
    elif [[ -f "/root/dchat-deploy/Foundation-servers/$key" ]]; then
        key_path="/root/dchat-deploy/Foundation-servers/$key"
    else
        echo -e "${RED}[$region] Key file not found: $key${NC}"
        continue
    fi
    
    check_validator "$region" "$ip" "$user" "$key_path"
done

echo -e "${CYAN}=== Status Check Complete ===${NC}"
