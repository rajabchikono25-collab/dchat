#!/bin/bash
set -e

VALIDATORS=(
    "saopaulo:54.233.203.82:AWS-Sao-Paulo/pablo.pem:ubuntu"
    "stockholm:13.48.49.2:AWS-Stokholm/relay.pem:ubuntu"
    "ohio:18.191.118.167:AWS-Ohio/gecko.pem:ubuntu"
    "india:74.225.183.196:Azure-India/uramami.pem:azureuser"
    "uae:4.161.34.228:Azure_UAE/Randal_key.pem:azureuser"
    "southafrica:4.221.211.71:Azure-SAfrica/anacreon.pem:azureuser"
)

echo "==========================================="
echo "Configuring Bootstrap Peers - All Validators"
echo "==========================================="

for validator_info in "${VALIDATORS[@]}"; do
    IFS=':' read -r region ip pem user <<< "$validator_info"
    
    echo ""
    echo ">> $region ($ip)..."
    
    # Download config
    scp -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no -o ConnectTimeout=10 \
        $user@${ip}:/opt/dchat/config.toml /tmp/config_${region}.toml || {
        echo "❌ Failed to download config from $region, skipping..."
        continue
    }
    
    # Update with Python script
    python3 /root/dchat-deploy/ansible/update-bootstrap-peers.py /tmp/config_${region}.toml
    
    # Upload and restart
    scp -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no \
        /tmp/config_${region}.toml $user@${ip}:/tmp/config.toml
    
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no $user@${ip} '
        sudo mv /tmp/config.toml /opt/dchat/config.toml
        sudo systemctl restart dchat
    '
    
    echo "✅ $region updated"
done

echo ""
echo "==========================================="
echo "Waiting 10 seconds for validators to restart..."
sleep 10

echo ""
echo "Checking status..."
for validator_info in "${VALIDATORS[@]}"; do
    IFS=':' read -r region ip pem user <<< "$validator_info"
    echo ""
    echo ">> $region:"
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no -o ConnectTimeout=5 $user@${ip} \
        'sudo systemctl status dchat --no-pager | head -3' || echo "❌ Failed to check $region"
done

echo ""
echo "==========================================="
echo "✅ Bootstrap peers configured on all validators!"
echo "==========================================="
