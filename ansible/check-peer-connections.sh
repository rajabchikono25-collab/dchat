#!/bin/bash

echo "=========================================="
echo "Checking P2P Peer Connections"
echo "=========================================="
echo ""

VALIDATORS=(
    "Ohio:18.191.118.167:AWS-Ohio/gecko.pem:ubuntu"
    "Stockholm:13.48.49.2:AWS-Stokholm/relay.pem:ubuntu"
    "São Paulo:54.233.203.82:AWS-Sao-Paulo/pablo.pem:ubuntu"
    "India:74.225.183.196:Azure-India/uramami.pem:azureuser"
)

for validator_info in "${VALIDATORS[@]}"; do
    IFS=':' read -r name ip pem user <<< "$validator_info"
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🌍 $name ($ip)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no -o ConnectTimeout=10 $user@$ip '
        echo "📡 Searching for peer connection logs..."
        sudo journalctl -u dchat --no-pager | grep -E "peer|connect|Handshake|bootstrap|Local peer" | tail -30
        
        echo ""
        echo "📊 Checking metrics endpoint for peer count..."
        curl -s http://localhost:9090/metrics 2>/dev/null | grep -i peer || echo "Metrics not available"
        
        echo ""
        echo "🔍 Network listening status..."
        sudo ss -tlnp | grep dchat || echo "No listening sockets found"
    ' 2>/dev/null || echo "❌ Failed to connect"
    
    echo ""
    echo ""
done

echo "=========================================="
echo "Summary"
echo "=========================================="
echo ""
echo "Note: Look for 'Connected to peer' or similar messages"
echo "If no peer connections found, validators are isolated."
echo ""
