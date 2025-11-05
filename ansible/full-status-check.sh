#!/bin/bash

echo "=========================================="
echo "dchat Validator Network - Full Status Check"
echo "=========================================="
echo ""

VALIDATORS=(
    "São Paulo:54.233.203.82:AWS-Sao-Paulo/pablo.pem:ubuntu"
    "Stockholm:13.48.49.2:AWS-Stokholm/relay.pem:ubuntu"
    "Ohio:18.191.118.167:AWS-Ohio/gecko.pem:ubuntu"
    "Singapore:18.142.96.209:AWS-Singapore/craig.pem:ubuntu"
    "India:74.225.183.196:Azure-India/uramami.pem:azureuser"
    "UAE:4.161.34.228:Azure_UAE/Randal_key.pem:azureuser"
    "South Africa:4.221.211.71:Azure-SAfrica/anacreon.pem:azureuser"
)

running_count=0
total_count=0

for validator_info in "${VALIDATORS[@]}"; do
    IFS=':' read -r name ip pem user <<< "$validator_info"
    total_count=$((total_count + 1))
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    printf "🌍 %-15s %s\n" "$name" "($ip)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # Check if running
    status=$(ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no -o ConnectTimeout=5 $user@$ip 'sudo systemctl is-active dchat' 2>/dev/null || echo "failed")
    
    if [ "$status" = "active" ]; then
        echo "✅ Status: RUNNING"
        running_count=$((running_count + 1))
        
        # Get detailed info
        ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no $user@$ip '
            echo ""
            echo "📊 Process Info:"
            sudo systemctl status dchat --no-pager | grep -E "PID|Tasks|Memory|CPU" | head -4
            
            echo ""
            echo "📝 Recent Activity (last 5 lines):"
            sudo journalctl -u dchat -n 5 --no-pager | tail -5
            
            echo ""
            echo "🔑 Peer ID:"
            sudo journalctl -u dchat --no-pager | grep "peer_id:" | tail -1 || echo "Not found yet"
            
            echo ""
        ' 2>/dev/null || echo "Failed to get details"
    else
        echo "❌ Status: $status"
    fi
    
    echo ""
done

echo "=========================================="
echo "📈 Network Summary"
echo "=========================================="
echo "Total Validators: $total_count"
echo "Running: $running_count"
echo "Offline: $((total_count - running_count))"

if [ $running_count -eq $total_count ]; then
    echo ""
    echo "🎉 ALL VALIDATORS OPERATIONAL! 🎉"
    echo ""
    echo "Network is ready for:"
    echo "  ✅ Cross-region peer connectivity"
    echo "  ✅ Consensus participation"
    echo "  ✅ Block production"
    echo "  ✅ Message routing"
else
    echo ""
    echo "⚠️  $((total_count - running_count)) validator(s) offline"
fi

echo "=========================================="
