#!/bin/bash
# DNS Verification Script for dchat Foundation Servers
# Verifies all 7 regional validator DNS records are resolving correctly

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
GRAY='\033[0;37m'
NC='\033[0m' # No Color

echo -e "\n${CYAN}=== dchat DNS Verification ===${NC}"
echo -e "${WHITE}Domain: schikuno.top${NC}"
echo -e "${WHITE}Date: $(date '+%Y-%m-%d %H:%M:%S')${NC}\n"

# Validator array
declare -a REGIONS=(
    "AWS Ohio"
    "AWS São Paulo"
    "AWS Singapore"
    "AWS Stockholm"
    "Azure India"
    "Azure South Africa"
    "Azure UAE"
)

declare -a HOSTS=(
    "validator1-ohio.schikuno.top"
    "validator1-saopaulo.schikuno.top"
    "validator1-singapore.schikuno.top"
    "validator1-stockholm.schikuno.top"
    "validator1-india.schikuno.top"
    "validator1-southafrica.schikuno.top"
    "validator1-uae.schikuno.top"
)

PORT=7070

dns_ok=0
ping_ok=0
port_ok=0
total=7

for i in "${!HOSTS[@]}"; do
    region="${REGIONS[$i]}"
    host="${HOSTS[$i]}"
    
    echo -e "${YELLOW}[$region]${NC} ${WHITE}Checking $host...${NC}"
    
    # DNS resolution
    ip=$(dig +short "$host" | grep -E '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' | head -n1)
    
    if [ -n "$ip" ]; then
        echo -e "  ${GREEN}✅ DNS resolves to: ${WHITE}$ip${NC}"
        ((dns_ok++))
        
        # Ping test
        if ping -c 1 -W 2 "$host" &> /dev/null; then
            echo -e "  ${GREEN}✅ Server is reachable (ICMP)${NC}"
            ((ping_ok++))
        else
            echo -e "  ${YELLOW}⚠️  Server not responding to ping (may be firewalled)${NC}"
        fi
        
        # Port test
        if timeout 3 bash -c "echo > /dev/tcp/$host/$PORT" 2>/dev/null; then
            echo -e "  ${GREEN}✅ Port $PORT is open${NC}"
            ((port_ok++))
        else
            echo -e "  ${YELLOW}⚠️  Port $PORT is not open (service not deployed yet)${NC}"
        fi
    else
        echo -e "  ${RED}❌ DNS resolution failed${NC}"
    fi
    
    echo ""
done

# Summary
echo -e "\n${CYAN}=== Summary ===${NC}"

if [ $dns_ok -eq $total ]; then
    echo -e "${GREEN}DNS Resolution: $dns_ok/$total validators${NC}"
else
    echo -e "${YELLOW}DNS Resolution: $dns_ok/$total validators${NC}"
fi

if [ $ping_ok -eq $total ]; then
    echo -e "${GREEN}Ping Response: $ping_ok/$total validators${NC}"
elif [ $ping_ok -eq 0 ]; then
    echo -e "${YELLOW}Ping Response: $ping_ok/$total validators (expected if not deployed)${NC}"
else
    echo -e "${YELLOW}Ping Response: $ping_ok/$total validators${NC}"
fi

if [ $port_ok -eq $total ]; then
    echo -e "${GREEN}Port $PORT Open: $port_ok/$total validators${NC}"
elif [ $port_ok -eq 0 ]; then
    echo -e "${YELLOW}Port $PORT Open: $port_ok/$total validators (expected if not deployed)${NC}"
else
    echo -e "${YELLOW}Port $PORT Open: $port_ok/$total validators${NC}"
fi

# Next steps
echo -e "\n${CYAN}=== Next Steps ===${NC}"
if [ $dns_ok -eq $total ]; then
    echo -e "${GREEN}✅ All DNS records are configured correctly!${NC}"
    if [ $port_ok -eq 0 ]; then
        echo -e "${YELLOW}⏳ Services not yet deployed. Next: Deploy validators to servers.${NC}"
        echo -e "${GRAY}   See: Foundation-servers/DEPLOYMENT_CHECKLIST.md${NC}"
    elif [ $port_ok -lt $total ]; then
        echo -e "${YELLOW}⚠️  Some services are deployed, others are not.${NC}"
        echo -e "${GRAY}   Check deployment status on each server.${NC}"
    else
        echo -e "${GREEN}✅ All services are deployed and reachable!${NC}"
    fi
else
    echo -e "${YELLOW}⚠️  Some DNS records are not resolving. Check your DNS configuration.${NC}"
    echo -e "${GRAY}   Records not resolving:${NC}"
    for i in "${!HOSTS[@]}"; do
        host="${HOSTS[$i]}"
        ip=$(dig +short "$host" | head -n1)
        if [ -z "$ip" ]; then
            echo -e "${RED}   - $host${NC}"
        fi
    done
fi

echo -e "\n${GRAY}For detailed DNS propagation check, visit:${NC}"
echo -e "${CYAN}  https://www.whatsmydns.net/${NC}"
echo -e "${CYAN}  https://dnschecker.org/${NC}"

echo -e "\n${WHITE}Done!${NC}\n"
