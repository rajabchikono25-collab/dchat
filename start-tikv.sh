#!/usr/bin/env bash
# Quick start script for TiKV (Linux/macOS)
# Starts 3 PD nodes + 3 TiKV nodes for local development

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${GREEN}=================================="
echo "dchat TiKV Quick Start"
echo -e "==================================${NC}"

# Parse arguments
ACTION="${1:-start}"

# Check status
if [[ "$ACTION" == "status" ]]; then
    echo -e "\n${CYAN}Checking TiKV cluster status...${NC}"
    
    docker ps --filter "name=dchat-pd" --filter "name=dchat-tikv" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
    
    echo -e "\n${CYAN}PD Health Check:${NC}"
    if curl -sf http://localhost:2379/health > /dev/null 2>&1; then
        echo -e "${GREEN}PD1 (port 2379): $(curl -s http://localhost:2379/health)${NC}"
    else
        echo -e "${YELLOW}PD1 (port 2379): Not responding${NC}"
    fi
    
    echo -e "\n${CYAN}TiKV Status Check:${NC}"
    if curl -sf http://localhost:20180/status > /dev/null 2>&1; then
        echo -e "${GREEN}TiKV1 (port 20180): Healthy${NC}"
    else
        echo -e "${YELLOW}TiKV1 (port 20180): Not responding${NC}"
    fi
    
    exit 0
fi

# Stop TiKV
if [[ "$ACTION" == "stop" ]]; then
    echo -e "\n${YELLOW}Stopping TiKV cluster...${NC}"
    docker-compose -f docker-compose-testnet.yml stop pd1 pd2 pd3 tikv1 tikv2 tikv3
    echo -e "${GREEN}TiKV cluster stopped successfully${NC}"
    exit 0
fi

# Clean TiKV data
if [[ "$ACTION" == "clean" ]]; then
    echo -e "\n${RED}⚠️  WARNING: This will DELETE all TiKV data!${NC}"
    read -p "Type 'yes' to continue: " confirm
    
    if [[ "$confirm" != "yes" ]]; then
        echo -e "${YELLOW}Clean cancelled${NC}"
        exit 0
    fi
    
    echo -e "\n${YELLOW}Stopping TiKV cluster...${NC}"
    docker-compose -f docker-compose-testnet.yml stop pd1 pd2 pd3 tikv1 tikv2 tikv3
    
    echo -e "${YELLOW}Removing containers...${NC}"
    docker-compose -f docker-compose-testnet.yml rm -f pd1 pd2 pd3 tikv1 tikv2 tikv3
    
    echo -e "${YELLOW}Removing volumes...${NC}"
    docker volume rm dchat_pd1_data dchat_pd2_data dchat_pd3_data dchat_tikv1_data dchat_tikv2_data dchat_tikv3_data 2>/dev/null || true
    
    echo -e "${GREEN}TiKV data cleaned successfully${NC}"
    exit 0
fi

# Start TiKV cluster
echo -e "\n${CYAN}Starting TiKV cluster (3 PD + 3 TiKV nodes)...${NC}"

# Check if Docker is running
if ! docker ps > /dev/null 2>&1; then
    echo -e "${RED}❌ Error: Docker is not running. Please start Docker.${NC}"
    exit 1
fi

# Check if docker-compose-testnet.yml exists
if [[ ! -f "docker-compose-testnet.yml" ]]; then
    echo -e "${RED}❌ Error: docker-compose-testnet.yml not found${NC}"
    exit 1
fi

# Start PD cluster
echo -e "\n${CYAN}[1/3] Starting PD (Placement Driver) cluster...${NC}"
docker-compose -f docker-compose-testnet.yml up -d pd1 pd2 pd3

echo -e "${CYAN}Waiting for PD cluster to be ready...${NC}"
MAX_ATTEMPTS=30
ATTEMPT=0
PD_READY=false

while [[ $ATTEMPT -lt $MAX_ATTEMPTS ]] && [[ "$PD_READY" == "false" ]]; do
    sleep 2
    if curl -sf http://localhost:2379/health > /dev/null 2>&1; then
        PD_READY=true
        echo -e "${GREEN}✓ PD cluster is ready${NC}"
    else
        ((ATTEMPT++))
        echo -n "."
    fi
done

if [[ "$PD_READY" == "false" ]]; then
    echo -e "\n${RED}❌ Error: PD cluster failed to start within 60 seconds${NC}"
    echo -e "${YELLOW}Check logs with: docker-compose -f docker-compose-testnet.yml logs pd1${NC}"
    exit 1
fi

# Start TiKV nodes
echo -e "\n${CYAN}[2/3] Starting TiKV storage nodes...${NC}"
docker-compose -f docker-compose-testnet.yml up -d tikv1 tikv2 tikv3

echo -e "${CYAN}Waiting for TiKV nodes to register...${NC}"
sleep 10

TIKV_READY=false
ATTEMPT=0
while [[ $ATTEMPT -lt $MAX_ATTEMPTS ]] && [[ "$TIKV_READY" == "false" ]]; do
    sleep 2
    if curl -sf http://localhost:20180/status > /dev/null 2>&1; then
        TIKV_READY=true
        echo -e "${GREEN}✓ TiKV nodes are ready${NC}"
    else
        ((ATTEMPT++))
        echo -n "."
    fi
done

if [[ "$TIKV_READY" == "false" ]]; then
    echo -e "\n${YELLOW}⚠️  Warning: TiKV nodes may still be initializing${NC}"
    echo -e "${YELLOW}Check logs with: docker-compose -f docker-compose-testnet.yml logs tikv1${NC}"
fi

# Display connection info
cat <<EOF

${GREEN}==================================
✓ TiKV Cluster Started Successfully
==================================${NC}

${CYAN}PD Endpoints (for client connections):${NC}
  - http://localhost:2379  (PD1)
  - http://localhost:2381  (PD2)
  - http://localhost:2383  (PD3)

${CYAN}TiKV Nodes:${NC}
  - tikv1:20160  (Status: http://localhost:20180)
  - tikv2:20160  (Status: http://localhost:20181)
  - tikv3:20160  (Status: http://localhost:20182)

${CYAN}Rust Client Configuration (testnet-config.toml):${NC}
[storage.tikv]
pd_endpoints = [
    "http://localhost:2379",
    "http://localhost:2381",
    "http://localhost:2383"
]

${CYAN}Useful Commands:${NC}
  Status:    ./start-tikv.sh status
  Stop:      ./start-tikv.sh stop
  Clean:     ./start-tikv.sh clean
  Logs:      docker-compose -f docker-compose-testnet.yml logs -f tikv1
  PD Health: curl http://localhost:2379/health

${GREEN}✓ Ready for development!${NC}

EOF
