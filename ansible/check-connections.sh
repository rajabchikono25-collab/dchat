#!/bin/bash

echo "Checking if validators can see bootstrap peers..."
echo ""

VALIDATORS=(
    "Ohio:18.191.118.167:AWS-Ohio/gecko.pem:ubuntu"
    "Stockholm:13.48.49.2:AWS-Stokholm/relay.pem:ubuntu"
    "India:74.225.183.196:Azure-India/uramami.pem:azureuser"
)

for validator_info in "${VALIDATORS[@]}"; do
    IFS=':' read -r name ip pem user <<< "$validator_info"
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🌍 $name ($ip)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no $user@$ip '
        echo "Status:"
        sudo systemctl is-active dchat
        echo ""
        echo "Environment variables:"
        sudo systemctl show dchat | grep Environment
        echo ""
        echo "Recent logs (looking for bootstrap/peer messages):"
        sudo journalctl -u dchat -n 30 --no-pager | grep -i bootstrap || echo "No bootstrap messages"
        sudo journalctl -u dchat -n 30 --no-pager | grep -i "peer_id:" || echo "No peer_id messages"
        sudo journalctl -u dchat -n 5 --no-pager | tail -5
    '
    echo ""
done
