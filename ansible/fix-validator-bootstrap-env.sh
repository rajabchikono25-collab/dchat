#!/bin/bash

# Fix validator bootstrap peers using environment variable
# The validator code ignores config.toml bootstrap_peers but respects DCHAT_BOOTSTRAP_PEERS env var

set -e

# Construct bootstrap peers comma-separated list with CURRENT peer IDs
# These MUST match the actual running validators after key generation
BOOTSTRAP_PEERS="/ip4/54.233.203.82/tcp/9090/p2p/12D3KooWSJ8ef2CeYNLPhtKTL4rVjtjP28nz4rCASXsHk125iRZF,/ip4/13.48.49.2/tcp/9090/p2p/12D3KooWFarm5rCcmatPPVDU1SThRt7tf1WtQ8N581Sckaq19Qdu,/ip4/18.191.118.167/tcp/9090/p2p/12D3KooWRUZXh16GCfgUXMGvosGXVQD39jCRACsH2v3BPVPrYSPt,/ip4/18.142.96.209/tcp/9090/p2p/12D3KooWAdaNGLmZhq2ACGZJLueMRVSsbkAFdUW3j5L8d8JwHbpd,/ip4/74.225.183.196/tcp/9090/p2p/12D3KooWJ8ueQfaCeNCGY1shpxbCzDARumfczDkCuL7YT5bTYRdP,/ip4/4.161.34.228/tcp/9090/p2p/12D3KooWLEB1MHiuLwtUtrknbtj8y93cjqgWNWVnk9EuxssUdrUe,/ip4/4.221.211.71/tcp/9090/p2p/12D3KooWML3ZTRUoaorZKpydsJwDaAig8im4cjafHu59kb5NeVgS"

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

# Copy keys to WSL
echo "Copying SSH keys..."
cp /mnt/c/Users/USER/dchat/Foundation-servers/AWS-Ohio/gecko.pem /tmp/ohio.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/AWS-Stokholm/relay.pem /tmp/stockholm.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/AWS-Sao-Paulo/pablo.pem /tmp/saopaulo.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/AWS-Singapore/craig.pem /tmp/singapore.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/Azure-India/uramami.pem /tmp/india.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/Azure_UAE/Randal_key.pem /tmp/uae.pem
cp /mnt/c/Users/USER/dchat/Foundation-servers/Azure-SAfrica/anacreon.pem /tmp/southafrica.pem
chmod 600 /tmp/*.pem
echo "✓ Keys ready"
echo ""

for i in "${!SERVERS[@]}"; do
    SERVER="${SERVERS[$i]}"
    KEY="${KEYS[$i]}"
    NAME=$(echo "$SERVER" | cut -d':' -f2)
    HOST=$(echo "$SERVER" | cut -d'@' -f2 | cut -d':' -f1)
    USER=$(echo "$SERVER" | cut -d'@' -f1)
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🔧 Fixing $NAME ($HOST)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # Update systemd service to add Environment variable
    ssh -o StrictHostKeyChecking=no -i "$KEY" "$USER@$HOST" << EOF
        sudo bash -c 'cat > /etc/systemd/system/dchat.service << SERVICE
[Unit]
Description=dchat Validator Node
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/dchat
Environment=DCHAT_BOOTSTRAP_PEERS=${BOOTSTRAP_PEERS}
ExecStart=/opt/dchat/dchat validator --key keys/validator.key --chain-rpc http://localhost:26657 --stake 10000 --producer
Restart=on-failure
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
SERVICE
'
        sudo systemctl daemon-reload
        sudo systemctl restart dchat
        sleep 3
        if sudo systemctl is-active --quiet dchat; then
            echo "  ✓ Service restarted with bootstrap peers env var"
        else
            echo "  ✗ Service failed"
            sudo journalctl -u dchat -n 5 --no-pager
        fi
EOF
    
    echo ""
done

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All validators updated with bootstrap peers"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Wait 30 seconds for peer discovery, then check:"
echo "  wsl bash /mnt/c/Users/USER/dchat/ansible/check-peer-connections.sh"
