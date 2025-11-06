#!/bin/bash
# Check connectivity between 3 Azure servers

set -e

echo "=== dchat 3-Server Connectivity Check ==="
echo "Date: $(date)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# Server configuration
KEY_DIR="$HOME/.ssh/azure-keys"
SERVERS=(
    "India:74.225.183.196:azureuser:$KEY_DIR/uramami.pem"
    "SouthAfrica:4.221.211.71:azureuser:$KEY_DIR/anacreon.pem"
    "UAE:4.161.34.228:azureuser:$KEY_DIR/Randal_key.pem"
)

P2P_PORT=9090

# Check each server
running_count=0

for server_info in "${SERVERS[@]}"; do
    IFS=':' read -r name ip user key_path <<< "$server_info"
    
    echo -e "\n${YELLOW}=== $name ($ip) ===${NC}"
    
    # Check if service is running
    echo -e "${CYAN}[1] Checking service status...${NC}"
    service_status=$(ssh -o StrictHostKeyChecking=no -o ConnectTimeout=10 -i "$key_path" "$user@$ip" \
        "sudo systemctl is-active dchat 2>&1" || echo "failed")
    
    if [[ "$service_status" == "active" ]]; then
        echo -e "${GREEN}✅ Service is running${NC}"
        ((running_count++))
    else
        echo -e "${RED}❌ Service is not running: $service_status${NC}"
    fi
    
    # Check process
    echo -e "${CYAN}[2] Checking dchat process...${NC}"
    process_check=$(ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "ps aux | grep '[d]chat'" 2>&1 || echo "")
    
    if [[ -n "$process_check" ]]; then
        echo -e "${GREEN}✅ Process found${NC}"
        echo "$process_check" | sed 's/^/   /'
    else
        echo -e "${RED}❌ Process not found${NC}"
    fi
    
    # Check port listening
    echo -e "${CYAN}[3] Checking port $P2P_PORT...${NC}"
    port_check=$(ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "sudo netstat -tlnp 2>/dev/null | grep $P2P_PORT || sudo ss -tlnp 2>/dev/null | grep $P2P_PORT || echo ''" || echo "")
    
    if [[ -n "$port_check" ]]; then
        echo -e "${GREEN}✅ Port $P2P_PORT is listening${NC}"
        echo "$port_check" | sed 's/^/   /'
    else
        echo -e "${YELLOW}⚠️  Port $P2P_PORT not detected (may still be starting)${NC}"
    fi
    
    # Check recent logs for connections
    echo -e "${CYAN}[4] Checking connection logs...${NC}"
    logs=$(ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "sudo journalctl -u dchat -n 50 --no-pager 2>&1" || echo "")
    
    if [[ -n "$logs" ]]; then
        peer_lines=$(echo "$logs" | grep -i "peer\|connect\|handshake" || echo "")
        
        if [[ -n "$peer_lines" ]]; then
            echo -e "${GREEN}✅ Found connection activity:${NC}"
            echo "$peer_lines" | sed 's/^/   /'
        else
            echo -e "${YELLOW}⚠️  No peer connection logs found yet${NC}"
        fi
        
        # Check for errors
        error_lines=$(echo "$logs" | grep -i "error\|failed\|panic" | head -n 5 || echo "")
        if [[ -n "$error_lines" ]]; then
            echo -e "${YELLOW}⚠️  Found errors:${NC}"
            echo "$error_lines" | sed 's/^/   /'
        fi
    fi
    
    # Show recent log output
    echo -e "${CYAN}[5] Recent log output:${NC}"
    recent_logs=$(ssh -o StrictHostKeyChecking=no -i "$key_path" "$user@$ip" \
        "sudo journalctl -u dchat -n 10 --no-pager 2>&1" || echo "")
    
    if [[ -n "$recent_logs" ]]; then
        echo "$recent_logs" | sed 's/^/   /'
    fi
done

# Summary
echo -e "\n${CYAN}=== Summary ===${NC}"
echo -e "Servers Running: ${GREEN}$running_count/3${NC}"

# Network connectivity test
echo -e "\n${CYAN}=== Network Connectivity Test ===${NC}"

for source_server in "${SERVERS[@]}"; do
    IFS=':' read -r source_name source_ip source_user source_key <<< "$source_server"
    
    echo -e "\n${YELLOW}From $source_name:${NC}"
    
    for target_server in "${SERVERS[@]}"; do
        IFS=':' read -r target_name target_ip target_user target_key <<< "$target_server"
        
        if [[ "$source_ip" != "$target_ip" ]]; then
            echo -e "  Testing connection to $target_name ($target_ip)..."
            
            # Test port connectivity
            test_result=$(ssh -o StrictHostKeyChecking=no -o ConnectTimeout=10 -i "$source_key" "$source_user@$source_ip" \
                "timeout 5 bash -c 'cat < /dev/null > /dev/tcp/$target_ip/$P2P_PORT' 2>&1 && echo 'success' || echo 'failed'" 2>&1 || echo "failed")
            
            if [[ "$test_result" == *"success"* ]]; then
                echo -e "    ${GREEN}✅ Port $P2P_PORT accessible${NC}"
            else
                echo -e "    ${RED}❌ Port $P2P_PORT not accessible${NC}"
            fi
        fi
    done
done

# Recommendations
echo -e "\n${CYAN}=== Recommendations ===${NC}"

if [ $running_count -lt 3 ]; then
    echo -e "${YELLOW}• Start all services: sudo systemctl start dchat${NC}"
fi

echo -e "${CYAN}• Monitor live logs: ssh <user>@<server> 'sudo journalctl -u dchat -f'${NC}"
echo -e "${CYAN}• Check P2P connections: Look for 'peer connected' or 'handshake completed' messages${NC}"
echo -e "${CYAN}• Verify firewall: Ensure port $P2P_PORT is open in Azure NSG${NC}"

echo -e "\n${GREEN}Done!${NC}"
