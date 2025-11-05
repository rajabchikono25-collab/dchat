#!/bin/bash
# Update systemd service to run as root (temp fix for port 80 binding)

declare -A VALIDATORS=(
    ["ohio"]="18.191.118.167:ubuntu:AWS-Ohio/gecko.pem"
    ["saopaulo"]="54.233.203.82:ubuntu:AWS-Sao-Paulo/pablo.pem"
    ["singapore"]="18.142.96.209:ubuntu:AWS-Singapore/craig.pem"
    ["stockholm"]="13.48.49.2:ubuntu:AWS-Stokholm/relay.pem"
    ["india"]="74.225.183.196:azureuser:Azure-India/uramami.pem"
    ["southafrica"]="4.221.211.71:azureuser:Azure-SAfrica/anacreon.pem"
    ["uae"]="4.161.34.228:azureuser:Azure_UAE/Randal_key.pem"
)

for region in "${!VALIDATORS[@]}"; do
    IFS=':' read -r ip user key <<< "${VALIDATORS[$region]}"
    
    if [[ -f "/root/dchat-deploy/Foundation-servers/$key" ]]; then
        key_path="/root/dchat-deploy/Foundation-servers/$key"
    else
        echo "[$region] Key not found: $key"
        continue
    fi
    
    echo "[$region] Updating service on $ip..."
    
    ssh -i "$key_path" -o StrictHostKeyChecking=no ${user}@${ip} "cat > /tmp/dchat.service << 'SVCEOF'
[Unit]
Description=dchat Validator Node
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=root
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
sudo mv /tmp/dchat.service /etc/systemd/system/dchat.service
sudo systemctl daemon-reload
sudo systemctl restart dchat
sleep 5
sudo systemctl status dchat --no-pager | head -20"
    
    echo "[$region] ✅ Updated"
    echo ""
done

echo "=== Service update complete ==="
