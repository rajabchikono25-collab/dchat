#!/bin/bash
# Direct SSH deployment script for dchat validators
# This script deploys Docker, Redis, MinIO directly via SSH without Ansible

set -e

# Color codes
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Validators configuration
declare -A VALIDATORS=(
    ["ohio"]="18.191.118.167:ubuntu:AWS-Ohio/gecko.pem"
    ["saopaulo"]="54.233.203.82:ubuntu:AWS-Sao-Paulo/pablo.pem"
    ["singapore"]="18.142.96.209:ubuntu:AWS-Singapore/craig.pem"
    ["stockholm"]="13.48.49.2:ubuntu:AWS-Stokholm/relay.pem"
    ["india"]="74.225.183.196:azureuser:Azure-India/uramami.pem"
    ["southafrica"]="4.221.211.71:azureuser:Azure-SAfrica/anacreon.pem"
    ["uae"]="4.161.34.228:azureuser:Azure_UAE/Randal_key.pem"
)

# MinIO and Redis passwords (set these as environment variables)
MINIO_ROOT_PASSWORD="${MINIO_ROOT_PASSWORD:-ChangeMeMinIO123!}"
REDIS_PASSWORD="${REDIS_PASSWORD:-ChangeMeRedis123!}"
COCKROACHDB_CONNECTION="${COCKROACHDB_CONNECTION:-postgresql://dchat:password@absurd-auroch-17923.j77.cockroachlabs.cloud:26257/dchat?sslmode=require}"

echo -e "${CYAN}=== dchat Foundation Validators Deployment ===${NC}\n"

deploy_to_validator() {
    local region=$1
    local ip=$2
    local user=$3
    local key=$4
    
    echo -e "${YELLOW}[$region] Deploying to $ip...${NC}"
    
    # SSH options
    SSH_OPTS="-i $key -o StrictHostKeyChecking=no -o ConnectTimeout=30"
    
    # Test connection
    if ! ssh $SSH_OPTS ${user}@${ip} "echo 'Connected'" &>/dev/null; then
        echo -e "${RED}[$region] Failed to connect${NC}"
        return 1
    fi
    
    echo -e "${CYAN}[$region] Installing Docker...${NC}"
    ssh $SSH_OPTS ${user}@${ip} 'bash -s' << 'ENDSSH'
        set -e
        
        # Update system
        sudo apt-get update -qq
        
        # Install Docker if not present
        if ! command -v docker &> /dev/null; then
            curl -fsSL https://get.docker.com -o get-docker.sh
            sudo sh get-docker.sh
            sudo usermod -aG docker $USER
            rm get-docker.sh
        fi
        
        # Install Docker Compose
        if ! command -v docker-compose &> /dev/null; then
            sudo curl -L "https://github.com/docker/compose/releases/download/v2.23.0/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
            sudo chmod +x /usr/local/bin/docker-compose
        fi
        
        # Create directories
        sudo mkdir -p /opt/dchat/{data,config,logs}
        sudo chown -R $USER:$USER /opt/dchat
ENDSSH
    
    echo -e "${CYAN}[$region] Deploying Redis...${NC}"
    ssh $SSH_OPTS ${user}@${ip} "bash -s" << ENDSSH
        set -e
        
        # Stop existing containers if any
        sudo docker stop dchat-redis 2>/dev/null || true
        sudo docker rm dchat-redis 2>/dev/null || true
        
        # Deploy Redis
        sudo docker run -d --name dchat-redis --restart unless-stopped \
            -p 6379:6379 \
            -v /opt/dchat/data/redis:/data \
            redis:7-alpine \
            redis-server --appendonly yes --maxmemory 2gb --maxmemory-policy allkeys-lru
ENDSSH
    
    echo -e "${CYAN}[$region] Deploying MinIO...${NC}"
    ssh $SSH_OPTS ${user}@${ip} "bash -s" << ENDSSH
        set -e
        
        # Stop existing containers if any
        sudo docker stop dchat-minio 2>/dev/null || true
        sudo docker rm dchat-minio 2>/dev/null || true
        
        # Deploy MinIO
        sudo docker run -d --name dchat-minio --restart unless-stopped \
            -p 9000:9000 -p 9001:9001 \
            -v /opt/dchat/data/minio:/data \
            -e "MINIO_ROOT_USER=dchat" \
            -e "MINIO_ROOT_PASSWORD=${MINIO_ROOT_PASSWORD}" \
            quay.io/minio/minio:latest server /data --console-address ":9001"
        
        # Wait for MinIO to start
        sleep 5
        
        # Create buckets
        sudo docker run --rm --network host --entrypoint /bin/sh quay.io/minio/mc:latest -c "
            mc alias set dchat http://localhost:9000 dchat ${MINIO_ROOT_PASSWORD}
            mc mb dchat/messages --ignore-existing
            mc mb dchat/attachments --ignore-existing
            mc mb dchat/avatars --ignore-existing
            mc mb dchat/backups --ignore-existing
            mc mb dchat/logs --ignore-existing
            mc mb dchat/metrics --ignore-existing
        "
ENDSSH
    
    echo -e "${CYAN}[$region] Configuring firewall...${NC}"
    ssh $SSH_OPTS ${user}@${ip} 'bash -s' << 'ENDSSH'
        set -e
        
        # Install and configure UFW
        sudo apt-get install -y ufw -qq
        
        # Allow SSH
        sudo ufw allow 22/tcp
        
        # Allow P2P, Health, Metrics
        sudo ufw allow 9090/tcp
        sudo ufw allow 9091/tcp
        sudo ufw allow 9100/tcp
        
        # Allow Redis and MinIO (internal use only - should restrict in production)
        sudo ufw allow 6379/tcp
        sudo ufw allow 9000:9001/tcp
        
        # Enable firewall
        sudo ufw --force enable
ENDSSH
    
    echo -e "${CYAN}[$region] Creating status script...${NC}"
    ssh $SSH_OPTS ${user}@${ip} "cat > /opt/dchat/status.sh" << 'ENDSSH'
#!/bin/bash
echo "=== dchat Validator Status ==="
echo ""
echo "Docker Containers:"
sudo docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
echo ""
echo "Redis Status:"
sudo docker exec dchat-redis redis-cli ping 2>/dev/null || echo "Redis not responding"
echo ""
echo "MinIO Status:"
curl -s http://localhost:9000/minio/health/live && echo "MinIO: OK" || echo "MinIO: DOWN"
echo ""
echo "Disk Usage:"
df -h /opt/dchat/data
ENDSSH
    
    ssh $SSH_OPTS ${user}@${ip} "chmod +x /opt/dchat/status.sh"
    
    echo -e "${GREEN}[$region] ✅ Deployment complete${NC}\n"
}

# Deploy to all validators
for region in "${!VALIDATORS[@]}"; do
    IFS=':' read -r ip user key <<< "${VALIDATORS[$region]}"
    
    # Adjust key path based on where script is run from
    if [[ -f "$key" ]]; then
        key_path="$key"
    elif [[ -f "../Foundation-servers/$key" ]]; then
        key_path="../Foundation-servers/$key"
    elif [[ -f "/root/dchat-deploy/Foundation-servers/$key" ]]; then
        key_path="/root/dchat-deploy/Foundation-servers/$key"
    else
        echo -e "${RED}[$region] Key file not found: $key${NC}"
        continue
    fi
    
    deploy_to_validator "$region" "$ip" "$user" "$key_path" || echo -e "${RED}[$region] Deployment failed${NC}"
done

echo -e "${CYAN}=== Deployment Summary ===${NC}"
echo -e "Check status on each validator with: ${YELLOW}ssh <user>@<ip> '/opt/dchat/status.sh'${NC}"
echo -e "Or use: ${YELLOW}./check-all-validators.sh${NC}\n"
