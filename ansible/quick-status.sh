#!/bin/bash

echo "=== dchat Validator Status Summary ==="
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

for validator_info in "${VALIDATORS[@]}"; do
    IFS=':' read -r name ip pem user <<< "$validator_info"
    printf "%-15s %-16s " "$name" "$ip"
    
    status=$(ssh -i /root/dchat-deploy/Foundation-servers/$pem -o StrictHostKeyChecking=no -o ConnectTimeout=5 $user@$ip 'sudo systemctl is-active dchat' 2>/dev/null || echo "failed")
    
    if [ "$status" = "active" ]; then
        echo "✅ RUNNING"
    else
        echo "❌ $status"
    fi
done

echo ""
echo "Checking peer connections..."
echo ""

# Check one validator for peer logs
echo ">> Ohio validator logs:"
ssh -i /root/dchat-deploy/Foundation-servers/AWS-Ohio/gecko.pem -o StrictHostKeyChecking=no ubuntu@18.191.118.167 'sudo journalctl -u dchat -n 15 --no-pager'
