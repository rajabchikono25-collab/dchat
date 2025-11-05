
#!/bin/bash
# Deploy dchat validator binary to all validators

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

BINARY_PATH="../target/release/dchat"
COCKROACHDB_CONNECTION="${COCKROACHDB_CONNECTION:-postgresql://dchat:password@absurd-auroch-17923.j77.cockroachlabs.cloud:26257/dchat?sslmode=require}"
MINIO_ROOT_PASSWORD="${MINIO_ROOT_PASSWORD:-YourSecurePassword123!}"

echo -e "${CYAN}=== dchat Validator Binary Deployment ===${NC}\n"

# Check if binary exists
if [[ ! -f "$BINARY_PATH" ]]; then
    echo -e "${RED}Binary not found at $BINARY_PATH${NC}"
    echo -e "${YELLOW}Please build first: cargo build --release --bin dchat-validator${NC}"
    exit 1
fi

echo -e "${GREEN}Binary found: $BINARY_PATH ($(du -h $BINARY_PATH | cut -f1))${NC}\n"

deploy_binary() {
    local region=$1
    local ip=$2
    local user=$3
    local key=$4
    
    echo -e "${YELLOW}[$region] Deploying to $ip...${NC}"
    
    # SSH options
    SSH_OPTS="-i $key -o StrictHostKeyChecking=no -o ConnectTimeout=30"
    
    # Copy binary
    echo -e "${CYAN}[$region] Copying binary...${NC}"
    scp $SSH_OPTS "$BINARY_PATH" ${user}@${ip}:/tmp/dchat
    
    # Install and configure
    echo -e "${CYAN}[$region] Installing and configuring...${NC}"
    ssh $SSH_OPTS ${user}@${ip} "bash -s" << ENDSSH
        set -e
        
        # Create directory and move binary
        sudo mkdir -p /opt/dchat
        sudo mv /tmp/dchat /opt/dchat/dchat
        sudo chmod +x /opt/dchat/dchat
        
        # Create config file
        cat > /tmp/config.toml << 'EOF'
[network]
listen_address = "0.0.0.0:9090"
external_address = "$ip:9090"
bootstrap_peers = []

[storage.cockroachdb]
connection_string = "$COCKROACHDB_CONNECTION"

[storage.redis]
host = "localhost"
port = 6379
db = 0

[storage.minio]
endpoint = "localhost:9000"
access_key = "dchat"
secret_key = "$MINIO_ROOT_PASSWORD"
secure = false

[storage.tikv]
pd_endpoints = []

[metrics]
enabled = true
listen_address = "0.0.0.0:9100"

[logging]
level = "info"
file = "/opt/dchat/logs/validator.log"
EOF
        sudo mv /tmp/config.toml /opt/dchat/config.toml
        
        # Create systemd service
        sudo tee /etc/systemd/system/dchat.service > /dev/null << 'SVCEOF'
[Unit]
Description=dchat Validator Node
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=$user
WorkingDirectory=/opt/dchat
ExecStart=/opt/dchat/dchat validator --key keys/validator.key --chain-rpc http://localhost:26657 --stake 10000 --producer
Environment=DCHAT_KEY_PASSWORD=validator_password
Restart=on-failure
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
SVCEOF
        
        # Create log directory
        sudo mkdir -p /opt/dchat/logs
        sudo chown -R $user:$user /opt/dchat
        
        # Reload systemd
        sudo systemctl daemon-reload
        
        echo "Binary deployed successfully"
ENDSSH
    
    echo -e "${GREEN}[$region] ✅ Deployment complete${NC}\n"
}

# Deploy to all validators
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
    
    deploy_binary "$region" "$ip" "$user" "$key_path" || echo -e "${RED}[$region] Deployment failed${NC}"
done

echo -e "${CYAN}=== Binary Deployment Complete ===${NC}"
echo -e "To start validators: ${YELLOW}./start-all-validators.sh${NC}"
echo -e "To check status: ${YELLOW}./check-all-validators.sh${NC}\n"
