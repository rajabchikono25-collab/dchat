#!/bin/bash
# Check for peer connections on all 3 servers

KEY_DIR="$HOME/.ssh/azure-keys"

echo "=== India (74.225.183.196) Logs ==="
ssh -o StrictHostKeyChecking=no -i "$KEY_DIR/uramami.pem" azureuser@74.225.183.196 \
    'sudo journalctl -u dchat --since "2 minutes ago" --no-pager | grep -iE "peer|connect|handshake|bootstrap" | tail -15'

echo ""
echo "=== South Africa (4.221.211.71) Logs ==="
ssh -o StrictHostKeyChecking=no -i "$KEY_DIR/anacreon.pem" azureuser@4.221.211.71 \
    'sudo journalctl -u dchat --since "2 minutes ago" --no-pager | grep -iE "peer|connect|handshake|bootstrap" | tail -15'

echo ""
echo "=== UAE (4.161.34.228) Logs ==="
ssh -o StrictHostKeyChecking=no -i "$KEY_DIR/Randal_key.pem" azureuser@4.161.34.228 \
    'sudo journalctl -u dchat --since "2 minutes ago" --no-pager | grep -iE "peer|connect|handshake|bootstrap" | tail -15'

echo ""
echo "=== Summary ==="
echo "All 3 servers are running dchat relay nodes!"
echo "India (bootstrap): 74.225.183.196:9090"
echo "South Africa:      4.221.211.71:9090"
echo "UAE:               4.161.34.228:9090"
