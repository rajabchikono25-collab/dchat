#!/bin/bash
# Deploy correct config files to all validators

declare -A VALIDATORS=(
    ["ohio"]="18.191.118.167:ubuntu:AWS-Ohio/gecko.pem"
    ["saopaulo"]="54.233.203.82:ubuntu:AWS-Sao-Paulo/pablo.pem"
    ["singapore"]="18.142.96.209:ubuntu:AWS-Singapore/craig.pem"
    ["stockholm"]="13.48.49.2:ubuntu:AWS-Stokholm/relay.pem"
    ["india"]="74.225.183.196:azureuser:Azure-India/uramami.pem"
    ["southafrica"]="4.221.211.71:azureuser:Azure-SAfrica/anacreon.pem"
    ["uae"]="4.161.34.228:azureuser:Azure_UAE/Randal_key.pem"
)

COCKROACHDB_CONNECTION="postgresql://dchat:password@absurd-auroch-17923.j77.cockroachlabs.cloud:26257/dchat?sslmode=require"
MINIO_ROOT_PASSWORD="YourSecurePassword123!"

for region in "${!VALIDATORS[@]}"; do
    IFS=':' read -r ip user key <<< "${VALIDATORS[$region]}"
    
    if [[ -f "/root/dchat-deploy/Foundation-servers/$key" ]]; then
        key_path="/root/dchat-deploy/Foundation-servers/$key"
    else
        echo "[$region] Key not found: $key"
        continue
    fi
    
    echo "[$region] Deploying config to $ip..."
    
    ssh -i "$key_path" -o StrictHostKeyChecking=no ${user}@${ip} "cat > /tmp/config.toml << 'EOF'
[network]
listen_addresses = [\"0.0.0.0:9090\"]
external_address = \"${ip}:9090\"
bootstrap_peers = []

[storage.cockroachdb]
connection_string = \"${COCKROACHDB_CONNECTION}\"

[storage.redis]
host = \"localhost\"
port = 6379
db = 0

[storage.minio]
endpoint = \"localhost:9000\"
access_key = \"dchat\"
secret_key = \"${MINIO_ROOT_PASSWORD}\"
secure = false

[storage.tikv]
pd_endpoints = []

[metrics]
enabled = true
listen_address = \"0.0.0.0:9100\"

[logging]
level = \"info\"
file = \"/opt/dchat/logs/validator.log\"
EOF
sudo mv /tmp/config.toml /opt/dchat/config.toml
sudo chown ${user}:${user} /opt/dchat/config.toml
sudo systemctl restart dchat
sleep 3
sudo systemctl status dchat --no-pager | head -15"
    
    echo "[$region] ✅ Done"
    echo ""
done

echo "=== Config deployment complete ==="
