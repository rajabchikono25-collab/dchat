#!/bin/bash
# Test SSH connectivity to all validators using existing .pem keys

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
GRAY='\033[0;37m'
NC='\033[0m' # No Color

echo -e "\n${CYAN}=== Testing SSH Connectivity to Validators ===${NC}"
echo -e "${WHITE}Using existing .pem keys from Foundation-servers/${NC}\n"

# Validator array
declare -a REGIONS=("Ohio" "São Paulo" "Singapore" "Stockholm" "India" "South Africa" "UAE")
declare -a HOSTS=(
    "validator1-ohio.schikuno.top"
    "validator1-saopaulo.schikuno.top"
    "validator1-singapore.schikuno.top"
    "validator1-stockholm.schikuno.top"
    "validator1-india.schikuno.top"
    "validator1-southafrica.schikuno.top"
    "validator1-uae.schikuno.top"
)
declare -a IPS=(
    "18.191.118.167"
    "54.233.203.82"
    "18.142.96.209"
    "13.48.49.2"
    "74.225.183.196"
    "4.221.211.71"
    "4.161.34.228"
)
declare -a KEYS=(
    "../Foundation-servers/AWS-Ohio/gecko.pem"
    "../Foundation-servers/AWS-Sao-Paulo/pablo.pem"
    "../Foundation-servers/AWS-Singapore/craig.pem"
    "../Foundation-servers/AWS-Stokholm/relay.pem"
    "../Foundation-servers/Azure-India/uramami.pem"
    "../Foundation-servers/Azure-SAfrica/anacreon.pem"
    "../Foundation-servers/Azure_UAE/Randal_key.pem"
)
declare -a USERS=("ubuntu" "ubuntu" "ubuntu" "ubuntu" "azureuser" "azureuser" "azureuser")

successful=0
failed=0

for i in "${!HOSTS[@]}"; do
    region="${REGIONS[$i]}"
    host="${HOSTS[$i]}"
    ip="${IPS[$i]}"
    key="${KEYS[$i]}"
    user="${USERS[$i]}"
    
    echo -e "${YELLOW}[$region]${NC} ${WHITE}Testing $host...${NC}"
    
    # Check if key file exists
    if [ ! -f "$key" ]; then
        echo -e "  ${RED}❌ Key file not found: $key${NC}"
        ((failed++))
        echo ""
        continue
    fi
    
    # Check and fix key permissions (must be 400 or 600)
    chmod 400 "$key" 2>/dev/null
    echo -e "  ${GRAY}ℹ️  Key: $key | User: $user${NC}"
    
    # Try SSH connection
    if ssh -i "$key" -o StrictHostKeyChecking=no -o ConnectTimeout=10 -o BatchMode=yes ${user}@$ip 'echo Connected' &>/dev/null; then
        echo -e "  ${GREEN}✅ SSH connection successful${NC}"
        ((successful++))
    else
        echo -e "  ${RED}❌ SSH connection failed${NC}"
        ((failed++))
    fi
    
    echo ""
done

# Summary
echo -e "\n${CYAN}=== Summary ===${NC}"
if [ $successful -eq 7 ]; then
    echo -e "${GREEN}Successful: $successful/7${NC}"
else
    echo -e "${YELLOW}Successful: $successful/7${NC}"
fi

if [ $failed -eq 0 ]; then
    echo -e "${GREEN}Failed: $failed/7${NC}"
else
    echo -e "${RED}Failed: $failed/7${NC}"
fi

if [ $successful -eq 7 ]; then
    echo -e "\n${GREEN}✅ All validators are accessible!${NC}"
    echo -e "${CYAN}You can now run: ansible-playbook -i inventory.ini playbook.yml${NC}"
else
    echo -e "\n${YELLOW}⚠️  Some validators are not accessible.${NC}"
    echo -e "${GRAY}Please check:${NC}"
    echo -e "${GRAY}  - Key file permissions (should be 400)${NC}"
    echo -e "${GRAY}  - SSH port (22) is open in security groups${NC}"
    echo -e "${GRAY}  - Correct username (ubuntu/ec2-user/admin)${NC}"
    echo -e "${GRAY}  - IP addresses are correct${NC}"
fi

echo -e "\n${WHITE}Done!${NC}\n"
