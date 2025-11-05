#!/bin/bash
# Generate validator keys for all 7 regional validators

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

BINARY_PATH="../target/release/dchat"

echo -e "${CYAN}=== dchat Validator Key Generation ===${NC}\n"

# Check if binary exists
if [[ ! -f "$BINARY_PATH" ]]; then
    echo -e "${RED}Binary not found at $BINARY_PATH${NC}"
    echo -e "${YELLOW}Please build first: cargo build --release --bin dchat${NC}"
    exit 1
fi

# Create local keys directory
mkdir -p validator_keys

generate_key() {
    local region=$1
    local ip=$2
    local user=$3
    local key=$4
    
    echo -e "${YELLOW}[$region] Generating validator key for $ip...${NC}"
    
    SSH_OPTS="-i $key -o StrictHostKeyChecking=no -o ConnectTimeout=30"
    
    # Generate key remotely
    ssh $SSH_OPTS ${user}@${ip} "bash -s" << ENDSSH
        set -e
        
        # Create keys directory
        sudo mkdir -p /opt/dchat/keys
        
        # Generate validator key
        cd /opt/dchat
        sudo ./dchat keygen --output keys/validator.key
        
        # Set proper permissions
        sudo chmod 600 /opt/dchat/keys/validator.key
        sudo chown $user:$user /opt/dchat/keys/validator.key
        
        # Display public key/peer ID
        echo "Validator key generated successfully"
        cat /opt/dchat/keys/validator.key | grep -A 1 "public_key" || true
ENDSSH
    
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}[$region] ✅ Key generated${NC}"
        
        # Download key for backup
        scp $SSH_OPTS ${user}@${ip}:/opt/dchat/keys/validator.key validator_keys/${region}_validator.key || true
        
        echo ""
    else
        echo -e "${RED}[$region] ❌ Key generation failed${NC}\n"
    fi
}

# Generate keys for all validators
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
    
    generate_key "$region" "$ip" "$user" "$key_path"
done

echo -e "${CYAN}=== Key Generation Complete ===${NC}"
echo -e "Keys backed up to: ${GREEN}./validator_keys/${NC}"
echo -e "\nNext steps:"
echo -e "  1. Review backed up keys in ${YELLOW}./validator_keys/${NC}"
echo -e "  2. Update bootstrap peers in configs with validator peer IDs"
echo -e "  3. Start validators: ${YELLOW}./start-all-validators.sh${NC}\n"
