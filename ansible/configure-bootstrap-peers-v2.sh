#!/bin/bash
set -e

# AWS Validators (ubuntu user)
AWS_VALIDATORS=(
    "saopaulo:54.233.203.82:AWS-Sao-Paulo/pablo.pem"
    "stockholm:13.48.49.2:AWS-Stokholm/relay.pem"
    "ohio:18.191.118.167:AWS-Ohio/gecko.pem"
    "singapore:18.142.96.209:AWS-Singapore/craig.pem"
)

# Azure Validators (azureuser user)
AZURE_VALIDATORS=(
    "india:74.225.183.196:Azure-India/uramami.pem"
    "uae:4.161.34.228:Azure_UAE/Randal_key.pem"
    "southafrica:4.221.211.71:Azure-SAfrica/anacreon.pem"
)

echo "==========================================="
echo "Configuring Bootstrap Peers on All Validators"
echo "==========================================="

# Function to update config on validator
update_config() {
    local region=$1
    local ip=$2
    local pem=$3
    local user=$4
    
    echo ""
    echo ">> Updating $region ($ip)..."
    
    # Download current config
    scp -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no \
        $user@${ip}:/opt/dchat/config.toml /tmp/config_${region}.toml
    
    # Remove existing bootstrap_peers line and next 7 lines (the array)
    sed -i '/^bootstrap_peers/,+7d' /tmp/config_${region}.toml
    
    # Insert new bootstrap_peers from file after listen_addresses line
    sed -i '/listen_addresses/r /root/dchat-deploy/ansible/bootstrap-peers.txt' /tmp/config_${region}.toml
    
    # Upload updated config
    scp -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no \
        /tmp/config_${region}.toml $user@${ip}:/tmp/config.toml
    
    # Install and restart
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no $user@${ip} '
        sudo mv /tmp/config.toml /opt/dchat/config.toml
        sudo chown root:root /opt/dchat/config.toml
        sudo chmod 644 /opt/dchat/config.toml
        sudo systemctl restart dchat
    '
    
    echo "✅ $region configured and restarted"
}

# Update AWS validators
for validator_info in "${AWS_VALIDATORS[@]}"; do
    IFS=':' read -r region ip pem <<< "$validator_info"
    update_config "$region" "$ip" "$pem" "ubuntu"
done

# Update Azure validators
for validator_info in "${AZURE_VALIDATORS[@]}"; do
    IFS=':' read -r region ip pem <<< "$validator_info"
    update_config "$region" "$ip" "$pem" "azureuser"
done

echo ""
echo "==========================================="
echo "✅ All validators configured with bootstrap peers!"
echo "==========================================="
echo ""
echo "Waiting 15 seconds for peer connections..."
sleep 15

echo ""
echo "Checking validator status and peer connections..."
echo ""

# Check status on each validator
for validator_info in "${AWS_VALIDATORS[@]}"; do
    IFS=':' read -r region ip pem <<< "$validator_info"
    echo "=========================================="
    echo ">> $region ($ip):"
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no ubuntu@${ip} '
        echo "Status:"
        sudo systemctl status dchat --no-pager -l | head -5
        echo ""
        echo "Recent logs:"
        sudo journalctl -u dchat -n 10 --no-pager | grep -i "peer\|listen\|connect\|bootstrap" || echo "No peer logs yet"
    '
    echo ""
done

for validator_info in "${AZURE_VALIDATORS[@]}"; do
    IFS=':' read -r region ip pem <<< "$validator_info"
    echo "=========================================="
    echo ">> $region ($ip):"
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no azureuser@${ip} '
        echo "Status:"
        sudo systemctl status dchat --no-pager -l | head -5
        echo ""
        echo "Recent logs:"
        sudo journalctl -u dchat -n 10 --no-pager | grep -i "peer\|listen\|connect\|bootstrap" || echo "No peer logs yet"
    '
    echo ""
done

echo "==========================================="
echo "Done! All validators configured and running."
echo "==========================================="
