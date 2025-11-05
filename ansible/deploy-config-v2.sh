#!/bin/bash
# Deploy config template to all validators

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
    
    echo "[$region] Deploying config to $ip..."
    
    # Create config with correct IP
    sed "s/VALIDATOR_IP/$ip/g" config.template.toml > /tmp/config_${region}.toml
    
    # Copy to validator
    scp -i "$key_path" -o StrictHostKeyChecking=no /tmp/config_${region}.toml ${user}@${ip}:/tmp/config.toml
    
    # Move and restart
    ssh -i "$key_path" -o StrictHostKeyChecking=no ${user}@${ip} "
        sudo mv /tmp/config.toml /opt/dchat/config.toml
        sudo chown ${user}:${user} /opt/dchat/config.toml
        sudo systemctl restart dchat
        sleep 3
        sudo systemctl status dchat --no-pager | head -15
    "
    
    # Cleanup
    rm /tmp/config_${region}.toml
    
    echo "[$region] ✅ Done"
    echo ""
done

echo "=== Config deployment complete ==="
