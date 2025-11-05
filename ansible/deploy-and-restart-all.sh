#!/bin/bash

# Deploy updated configs with correct peer IDs to all validators and restart services

set -e

SERVERS=(
    "ubuntu@18.191.118.167:ohio"
    "ubuntu@13.48.49.2:stockholm"
    "ubuntu@54.233.203.82:saopaulo"
    "ubuntu@18.142.96.209:singapore"
    "azureuser@74.225.183.196:india"
    "azureuser@4.161.34.228:uae"
    "azureuser@4.221.211.71:southafrica"
)

KEYS=(
    "/tmp/ohio.pem"
    "/tmp/stockholm.pem"
    "/tmp/saopaulo.pem"
    "/tmp/singapore.pem"
    "/tmp/india.pem"
    "/tmp/uae.pem"
    "/tmp/southafrica.pem"
)

# Copy Windows keys to WSL with proper permissions
echo "Copying SSH keys to WSL..."
cp /mnt/c/Users/USER/dchat/Foundation-servers/AWS-Ohio/gecko.pem /tmp/ohio.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/AWS-Stokholm/relay.pem /tmp/stockholm.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/AWS-Sao-Paulo/pablo.pem /tmp/saopaulo.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/AWS-Singapore/craig.pem /tmp/singapore.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/Azure-India/uramami.pem /tmp/india.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/Azure_UAE/Randal_key.pem /tmp/uae.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/Azure-SAfrica/anacreon.pem /tmp/southafrica.pem

chmod 600 /tmp/*.pem
echo "✓ Keys copied with correct permissions"
echo ""

CONFIG_TEMPLATE="/mnt/c/Users/USER/dchat/ansible/config.template.toml"

for i in "${!SERVERS[@]}"; do
    SERVER="${SERVERS[$i]}"
    KEY="${KEYS[$i]}"
    NAME=$(echo "$SERVER" | cut -d':' -f2)
    HOST=$(echo "$SERVER" | cut -d'@' -f2 | cut -d':' -f1)
    USER=$(echo "$SERVER" | cut -d'@' -f1)
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "📦 Deploying to $NAME ($HOST)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # Deploy updated config
    echo "  • Uploading config..."
    scp -o StrictHostKeyChecking=no -i "$KEY" "$CONFIG_TEMPLATE" "$USER@$HOST:/tmp/config.toml"
    
    # Backup old config, replace, and restart
    ssh -o StrictHostKeyChecking=no -i "$KEY" "$USER@$HOST" << 'REMOTE'
        sudo cp /opt/dchat/config.toml /opt/dchat/config.toml.backup
        sudo mv /tmp/config.toml /opt/dchat/config.toml
        sudo chown root:root /opt/dchat/config.toml
        sudo chmod 644 /opt/dchat/config.toml
        echo "  • Restarting dchat service..."
        sudo systemctl restart dchat
        sleep 2
        if sudo systemctl is-active --quiet dchat; then
            echo "  ✓ Service restarted successfully"
        else
            echo "  ✗ Service failed to start"
            sudo journalctl -u dchat -n 10 --no-pager
        fi
REMOTE
    
    echo ""
done

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All validators updated and restarted"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Wait 30 seconds for peer discovery, then check connections:"
echo "  wsl bash /mnt/c/Users/USER/dchat/ansible/check-peer-connections.sh"
