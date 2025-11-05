#!/bin/bash
# Generate simple JSON keys for validators (temporary until encryption support added)

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
    
    echo "[$region] Generating JSON key for $ip..."
    
    # Generate random 32-byte key as JSON
    ssh -i "$key_path" -o StrictHostKeyChecking=no ${user}@${ip} '
sudo python3 -c "
import json
import secrets

# Generate 32 random bytes for Ed25519 private key
private_key_bytes = secrets.token_bytes(32)

# Format as array string like [1, 2, 3, ...]  
bytes_array = str(list(private_key_bytes))

key_data = {
    \"private_key\": bytes_array,
    \"key_type\": \"Ed25519\"
}

with open(\"/opt/dchat/keys/validator.key\", \"w\") as f:
    json.dump(key_data, f, indent=2)

print(\"✅ Generated validator key\")
"
sudo chown root:root /opt/dchat/keys/validator.key
sudo chmod 600 /opt/dchat/keys/validator.key
sudo systemctl restart dchat
sleep 5
sudo systemctl status dchat --no-pager | head -15
'
    
    echo "[$region] ✅ Done"
    echo ""
done

echo "=== Key generation complete ==="
