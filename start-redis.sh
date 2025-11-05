#!/usr/bin/env bash
# Quick start script for Redis (Linux/macOS)
# Starts Redis server for local development

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${GREEN}=================================="
echo "dchat Redis Quick Start"
echo -e "==================================${NC}"

# Parse arguments
ACTION="${1:-start}"

# Check status
if [[ "$ACTION" == "status" ]]; then
    echo -e "\n${CYAN}Checking Redis status...${NC}"
    
    docker ps --filter "name=dchat-redis" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
    
    echo -e "\n${CYAN}Redis Health Check:${NC}"
    if response=$(docker exec dchat-redis redis-cli PING 2>&1) && [[ "$response" == "PONG" ]]; then
        echo -e "${GREEN}Redis is responding: $response${NC}"
    else
        echo -e "${YELLOW}Redis is not responding or container not running${NC}"
    fi
    
    echo -e "\n${CYAN}Redis Info:${NC}"
    docker exec dchat-redis redis-cli INFO server 2>/dev/null | grep -E "redis_version|uptime_in_seconds|connected_clients" || echo -e "${YELLOW}Could not retrieve Redis info${NC}"
    
    exit 0
fi

# Open Redis CLI
if [[ "$ACTION" == "cli" ]]; then
    echo -e "\n${CYAN}Opening Redis CLI...${NC}"
    docker exec -it dchat-redis redis-cli
    exit 0
fi

# Stop Redis
if [[ "$ACTION" == "stop" ]]; then
    echo -e "\n${YELLOW}Stopping Redis...${NC}"
    docker-compose -f docker-compose-testnet.yml stop redis
    echo -e "${GREEN}Redis stopped successfully${NC}"
    exit 0
fi

# Clean Redis data
if [[ "$ACTION" == "clean" ]]; then
    echo -e "\n${RED}⚠️  WARNING: This will DELETE all Redis data!${NC}"
    read -p "Type 'yes' to continue: " confirm
    
    if [[ "$confirm" != "yes" ]]; then
        echo -e "${YELLOW}Clean cancelled${NC}"
        exit 0
    fi
    
    echo -e "\n${YELLOW}Stopping Redis...${NC}"
    docker-compose -f docker-compose-testnet.yml stop redis
    
    echo -e "${YELLOW}Removing container...${NC}"
    docker-compose -f docker-compose-testnet.yml rm -f redis
    
    echo -e "${YELLOW}Removing volume...${NC}"
    docker volume rm dchat_redis_data 2>/dev/null || true
    
    echo -e "${GREEN}Redis data cleaned successfully${NC}"
    exit 0
fi

# Start Redis
echo -e "\n${CYAN}Starting Redis server...${NC}"

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

# Start Redis
docker-compose -f docker-compose-testnet.yml up -d redis

echo -e "${CYAN}Waiting for Redis to be ready...${NC}"
MAX_ATTEMPTS=10
ATTEMPT=0
REDIS_READY=false

while [[ $ATTEMPT -lt $MAX_ATTEMPTS ]] && [[ "$REDIS_READY" == "false" ]]; do
    sleep 1
    if response=$(docker exec dchat-redis redis-cli PING 2>&1) && [[ "$response" == "PONG" ]]; then
        REDIS_READY=true
        echo -e "${GREEN}✓ Redis is ready${NC}"
    else
        ((ATTEMPT++))
        echo -n "."
    fi
done

if [[ "$REDIS_READY" == "false" ]]; then
    echo -e "\n${RED}❌ Error: Redis failed to start within 10 seconds${NC}"
    echo -e "${YELLOW}Check logs with: docker-compose -f docker-compose-testnet.yml logs redis${NC}"
    exit 1
fi

# Display connection info
cat <<EOF

${GREEN}==================================
✓ Redis Started Successfully
==================================${NC}

${CYAN}Connection Information:${NC}
  URL:      redis://localhost:6379
  Host:     localhost
  Port:     6379
  Password: (none - development only)

${CYAN}Configuration (testnet-config.toml):${NC}
[storage.redis]
url = "redis://localhost:6379"
pool_size = 20
connection_timeout_secs = 5

${CYAN}Use Cases:${NC}
  • Channel metadata cache
  • User online status (presence)
  • Rate limiting counters
  • Session tokens
  • Message delivery queues
  • Relay discovery cache

${CYAN}Useful Commands:${NC}
  Status:    ./start-redis.sh status
  CLI:       ./start-redis.sh cli
  Stop:      ./start-redis.sh stop
  Clean:     ./start-redis.sh clean
  Logs:      docker-compose -f docker-compose-testnet.yml logs -f redis
  Monitor:   docker exec dchat-redis redis-cli MONITOR

${CYAN}Quick Test:${NC}
# Set a key
docker exec dchat-redis redis-cli SET test_key "Hello dchat"

# Get the key
docker exec dchat-redis redis-cli GET test_key

# Check connected clients
docker exec dchat-redis redis-cli CLIENT LIST

${GREEN}✓ Ready for development!${NC}

EOF
