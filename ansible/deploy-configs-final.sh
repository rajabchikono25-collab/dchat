#!/bin/bash
set -e

# Update template with bootstrap peers
echo "Updating template config with bootstrap peers..."
python3 /root/dchat-deploy/ansible/update-bootstrap-peers.py /root/dchat-deploy/ansible/config.template.toml

# Deploy to all validators
declare -A VALIDATORS=(
    [saopaulo]="ubuntu@54.233.203.82:AWS-Sao-Paulo/pablo.pem"
    [stockholm]="ubuntu@13.48.49.2:AWS-Stokholm/relay.pem"
    [ohio]="ubuntu@18.191.118.167:AWS-Ohio/gecko.pem"
    [india]="azureuser@74.225.183.196:Azure-India/uramami.pem"
    [uae]="azureuser@4.161.34.228:Azure_UAE/Randal_key.pem"
    [southafrica]="azureuser@4.221.211.71:Azure-SAfrica/anacreon.pem"
)

echo ""
echo "Deploying updated config to all validators..."
echo ""

for region in "${!VALIDATORS[@]}"; do
    IFS=':' read -r userhost pem <<< "${VALIDATORS[$region]}"
    echo ">> $region ($userhost)..."
    
    scp -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no -o ConnectTimeout=10 \
        /root/dchat-deploy/ansible/config.template.toml ${userhost}:/tmp/config.toml || {
        echo "❌ Failed to deploy to $region"
        continue
    }
    
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no $userhost \
        'sudo mv /tmp/config.toml /opt/dchat/config.toml && sudo systemctl restart dchat'
    
    echo "✅ $region updated"
    echo ""
done

echo "Waiting 10 seconds..."
sleep 10

echo ""
echo "Checking all validators..."
for region in "${!VALIDATORS[@]}"; do
    IFS=':' read -r userhost pem <<< "${VALIDATORS[$region]}"
    echo ">> $region:"
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no -o ConnectTimeout=5 $userhost \
        'sudo systemctl status dchat --no-pager | head -3' || echo "❌ Failed"
    echo ""
done

echo "✅ All validators updated!"
