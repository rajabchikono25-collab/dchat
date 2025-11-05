#!/bin/bash
# Monitor dchat validator logs across all nodes

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

monitor_validator() {
    local region=$1
    local ip=$2
    local user=$3
    local key=$4
    
    echo -e "${CYAN}[$region] validator1-$region.schikuno.top ($ip)${NC}"
    echo -e "${YELLOW}───────────────────────────────────────────────────────${NC}"
    
    SSH_OPTS="-i $key -o StrictHostKeyChecking=no -o ConnectTimeout=30"
    
    ssh $SSH_OPTS ${user}@${ip} "bash -s" << 'ENDSSH' || echo -e "${RED}Failed to connect${NC}"
        # Service status
        echo "Service Status:"
        systemctl is-active dchat-validator && echo "✅ RUNNING" || echo "❌ STOPPED"
        
        # Recent logs
        echo -e "\nRecent Logs (last 30 lines):"
        sudo journalctl -u dchat-validator -n 30 --no-pager | tail -30
        
        # Metrics check
        echo -e "\nMetrics Endpoint:"
        curl -s http://localhost:9100/metrics | head -5 || echo "❌ Metrics not available"
        
        # Resource usage
        echo -e "\nResource Usage:"
        ps aux | grep dchat-validator | grep -v grep | awk '{printf "CPU: %s%% | Memory: %s%% | PID: %s\n", $3, $4, $2}'
        
ENDSSH
    
    echo -e "${YELLOW}───────────────────────────────────────────────────────${NC}\n"
}

# Show monitoring options
if [ "$1" == "-f" ] || [ "$1" == "--follow" ]; then
    # Follow logs from specific validator
    region=${2:-ohio}
    
    IFS=':' read -r ip user key <<< "${VALIDATORS[$region]}"
    
    if [[ -z "$ip" ]]; then
        echo -e "${RED}Unknown region: $region${NC}"
        echo "Available regions: ${!VALIDATORS[@]}"
        exit 1
    fi
    
    # Find key file
    if [[ -f "$key" ]]; then
        key_path="$key"
    elif [[ -f "../Foundation-servers/$key" ]]; then
        key_path="../Foundation-servers/$key"
    elif [[ -f "/root/dchat-deploy/Foundation-servers/$key" ]]; then
        key_path="/root/dchat-deploy/Foundation-servers/$key"
    fi
    
    echo -e "${CYAN}Following logs for $region validator...${NC}"
    ssh -i $key_path -o StrictHostKeyChecking=no ${user}@${ip} "sudo journalctl -u dchat-validator -f"
else
    # Check all validators
    echo -e "${CYAN}=== dchat Validator Status (All Regions) ===${NC}\n"
    
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
            continue
        fi
        
        monitor_validator "$region" "$ip" "$user" "$key_path"
    done
    
    echo -e "${CYAN}=== End of Status Report ===${NC}"
    echo -e "Follow specific validator: ${YELLOW}$0 -f <region>${NC}"
    echo -e "Available regions: ${GREEN}${!VALIDATORS[@]}${NC}\n"
fi
