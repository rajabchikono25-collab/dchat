#!/bin/bash
set -e

# All 7 validators with bootstrap peer configuration
BOOTSTRAP_PEERS='bootstrap_peers = [
    "/ip4/54.233.203.82/tcp/9090/p2p/12D3KooWDwVLXK867iiDnB58iMMcyCqEK45WN3Y52xRPB1Cyegkj",
    "/ip4/13.48.49.2/tcp/9090/p2p/12D3KooW9sebMoe4meGTdXEiid7PqjcNq1dPhm32crHuWsZm97SM",
    "/ip4/18.191.118.167/tcp/9090/p2p/12D3KooWR9vuCXZWgMTivYtpWAZqb2wcB74nYMrRqkFAHFkD7yoi",
    "/ip4/18.142.96.209/tcp/9090/p2p/12D3KooWQ4qz5M7cvT8ri557QARdwHyFSep6EdWSnG7rRxCGXiz3",
    "/ip4/74.225.183.196/tcp/9090/p2p/12D3KooWNkrN3MA2y5syEwJa4nYdVW4rZi3vUN8WMPmFWV48sPZY",
    "/ip4/4.161.34.228/tcp/9090/p2p/12D3KooWNwDFq94Rkf5koHUsbpimFApiFfGCPSMrzWoGN3xjWr85",
    "/ip4/4.221.211.71/tcp/9090/p2p/12D3KooWMekFi9cuABuWAcStf8Ke82ZGMSnz3Qh5tv4XULHot759"
]'

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
    
    # Add bootstrap_peers to [network] section
    # Remove existing bootstrap_peers line
    sed -i '/^bootstrap_peers/d' /tmp/config_${region}.toml
    
    # Insert new bootstrap_peers after listen_addresses line
    sed -i '/listen_addresses/a\'"$BOOTSTRAP_PEERS" /tmp/config_${region}.toml
    
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
echo "Waiting 10 seconds for peer connections..."
sleep 10

echo ""
echo "Checking peer connections on each validator..."
echo ""

# Check connections on each validator
for validator_info in "${AWS_VALIDATORS[@]}"; do
    IFS=':' read -r region ip pem <<< "$validator_info"
    echo ">> $region peer connections:"
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no ubuntu@${ip} \
        'sudo journalctl -u dchat -n 5 --no-pager | grep -i "peer\|connect" || echo "No peer connection logs yet"'
    echo ""
done

for validator_info in "${AZURE_VALIDATORS[@]}"; do
    IFS=':' read -r region ip pem <<< "$validator_info"
    echo ">> $region peer connections:"
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no azureuser@${ip} \
        'sudo journalctl -u dchat -n 5 --no-pager | grep -i "peer\|connect" || echo "No peer connection logs yet"'
    echo ""
done

echo "Done! All validators should now be connected."
