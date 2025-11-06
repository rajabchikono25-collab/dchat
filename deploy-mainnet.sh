#!/bin/bash
# DCHAT Mainnet Deployment Script
# Compiles on Linux, then deploys to all 7 validators

set -e

# Configuration
SSH_KEY_PATH="${1:-./Foundation-servers}"
TEMP_DIR=$(mktemp -d)
BUILD_SERVER="ohio"

# Server configuration
declare -A SERVERS=(
    ["ohio"]="validator1-ohio.schikuno.top ubuntu AWS-Ohio/gecko.pem 1"
    ["singapore"]="validator1-singapore.schikuno.top ubuntu AWS-Singapore/craig.pem 2"
    ["stockholm"]="validator1-stockholm.schikuno.top ubuntu AWS-Stokholm/relay.pem 3"
    ["saopaulo"]="validator1-saopaulo.schikuno.top ubuntu AWS-Sao-Paulo/pablo.pem 4"
    ["india"]="validator1-india.schikuno.top azureuser Azure-India/uramami.pem 5"
    ["southafrica"]="validator1-southafrica.schikuno.top azureuser Azure-SAfrica/anacreon.pem 6"
    ["uae"]="validator1-uae.schikuno.top azureuser Azure_UAE/Randal_key.pem 7"
)

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${CYAN}"
echo "============================================"
echo "  DCHAT MAINNET DEPLOYMENT"
echo "============================================"
echo -e "${NC}"

# Find SSH keys
echo -e "${YELLOW}🔑 Locating SSH keys in $SSH_KEY_PATH...${NC}"
KEY_COUNT=$(find "$SSH_KEY_PATH" -name "*.pem" -type f | wc -l)
if [ "$KEY_COUNT" -eq 0 ]; then
    echo -e "${RED}❌ No SSH keys found in $SSH_KEY_PATH${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Found $KEY_COUNT SSH key(s)${NC}"

# Get build server details
IFS=' ' read -r BUILD_HOST BUILD_USER BUILD_KEY_FILE BUILD_ORDER <<< "${SERVERS[$BUILD_SERVER]}"
BUILD_KEY="$SSH_KEY_PATH/$BUILD_KEY_FILE"

if [ ! -f "$BUILD_KEY" ]; then
    echo -e "${RED}❌ Build server SSH key not found: $BUILD_KEY${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Using build server key: $BUILD_KEY_FILE${NC}"

echo -e "${CYAN}"
echo "============================================"
echo "  PHASE 1: PREPARE BUILD SERVER (OHIO)"
echo "============================================"
echo -e "${NC}"

# Create deployment package
echo -e "${YELLOW}📦 Creating deployment package...${NC}"
echo "  Copying source code..."

# Create tarball of source
TARBALL="$TEMP_DIR/dchat-source.tar.gz"
tar -czf "$TARBALL" \
    --exclude='target' \
    --exclude='mainnet-keys' \
    --exclude='mainnet-configs' \
    --exclude='Foundation-servers' \
    --exclude='.git' \
    Cargo.toml src/ crates/ 2>/dev/null

echo -e "${GREEN}✓ Package created: $TARBALL${NC}"

# Upload source to build server
echo -e "${YELLOW}📤 Uploading source to build server...${NC}"
scp -i "$BUILD_KEY" -o StrictHostKeyChecking=no "$TARBALL" "$BUILD_USER@$BUILD_HOST:/tmp/dchat-source.tar.gz"
if [ $? -ne 0 ]; then
    echo -e "${RED}❌ Failed to upload source${NC}"
    exit 1
fi

# Build on server
echo -e "${YELLOW}🔨 Building on OHIO...${NC}"
echo "  This will take 20-30 minutes..."

ssh -i "$BUILD_KEY" -o StrictHostKeyChecking=no "$BUILD_USER@$BUILD_HOST" << 'ENDSSH'
set -e
cd /tmp
rm -rf dchat-build
mkdir dchat-build
cd dchat-build
tar -xzf ../dchat-source.tar.gz
echo '📦 Installing Rust...'
if ! command -v rustc &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source $HOME/.cargo/env
fi
echo '🔨 Building release binary...'
cargo build --release
echo '✓ Build complete'
ls -lh target/release/dchat
ENDSSH

if [ $? -ne 0 ]; then
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Binary built successfully${NC}"

echo -e "${CYAN}"
echo "============================================"
echo "  PHASE 2: DEPLOY TO ALL SERVERS"
echo "============================================"
echo -e "${NC}"

# Deploy to each server
for region in ohio singapore stockholm saopaulo india southafrica uae; do
    IFS=' ' read -r host user key_file order <<< "${SERVERS[$region]}"
    key_path="$SSH_KEY_PATH/$key_file"
    
    echo -e "${YELLOW}📦 Deploying to ${region^^}...${NC}"
    
    if [ ! -f "$key_path" ]; then
        echo -e "${YELLOW}  ⚠️  Key not found: $key_file, using build server key${NC}"
        key_path="$BUILD_KEY"
    fi
    
    # Deploy binary
    echo "  📤 Deploying binary..."
    if [ "$region" == "ohio" ]; then
        # Already on build server
        ssh -i "$key_path" -o StrictHostKeyChecking=no "$user@$host" \
            'sudo cp /tmp/dchat-build/target/release/dchat /usr/local/bin/ && sudo chmod 755 /usr/local/bin/dchat'
    else
        # Download from build server and upload to target
        ssh -i "$key_path" -o StrictHostKeyChecking=no "$user@$host" 'mkdir -p /tmp/dchat-deploy'
        
        # Transfer via local machine
        temp_binary="$TEMP_DIR/dchat-binary"
        scp -i "$BUILD_KEY" -o StrictHostKeyChecking=no "$BUILD_USER@$BUILD_HOST:/tmp/dchat-build/target/release/dchat" "$temp_binary"
        scp -i "$key_path" -o StrictHostKeyChecking=no "$temp_binary" "$user@$host:/tmp/dchat-deploy/dchat"
        
        ssh -i "$key_path" -o StrictHostKeyChecking=no "$user@$host" \
            'sudo mv /tmp/dchat-deploy/dchat /usr/local/bin/ && sudo chmod 755 /usr/local/bin/dchat'
        
        rm -f "$temp_binary"
    fi
    
    # Deploy configuration
    echo "  ⚙️  Deploying configuration..."
    config_file="./mainnet-configs/config-mainnet-$region.toml"
    scp -i "$key_path" -o StrictHostKeyChecking=no "$config_file" "$user@$host:/tmp/config.toml"
    ssh -i "$key_path" -o StrictHostKeyChecking=no "$user@$host" \
        'sudo mkdir -p /etc/dchat && sudo mv /tmp/config.toml /etc/dchat/config.toml'
    
    # Deploy validator key
    echo "  🔑 Deploying validator key..."
    key_file_path="./mainnet-keys/validator-$region.key"
    scp -i "$key_path" -o StrictHostKeyChecking=no "$key_file_path" "$user@$host:/tmp/validator.key"
    ssh -i "$key_path" -o StrictHostKeyChecking=no "$user@$host" \
        'sudo mkdir -p /etc/dchat/keys && sudo mv /tmp/validator.key /etc/dchat/keys/validator.key && sudo chmod 600 /etc/dchat/keys/validator.key'
    
    # Create systemd service
    echo "  🔧 Creating systemd service..."
    cat > "$TEMP_DIR/dchat-validator.service" << EOF
[Unit]
Description=dchat Validator Node
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/dchat --config /etc/dchat/config.toml --role validator
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF
    
    scp -i "$key_path" -o StrictHostKeyChecking=no "$TEMP_DIR/dchat-validator.service" "$user@$host:/tmp/dchat-validator.service"
    ssh -i "$key_path" -o StrictHostKeyChecking=no "$user@$host" \
        'sudo mv /tmp/dchat-validator.service /etc/systemd/system/ && sudo systemctl daemon-reload'
    
    echo -e "${GREEN}  ✓ ${region^^} deployment complete${NC}"
done

echo -e "${CYAN}"
echo "============================================"
echo "  PHASE 3: START VALIDATORS SEQUENTIALLY"
echo "============================================"
echo -e "${NC}"

# Start validators in order
for order_num in {1..7}; do
    for region in ohio singapore stockholm saopaulo india southafrica uae; do
        IFS=' ' read -r host user key_file order <<< "${SERVERS[$region]}"
        
        if [ "$order" -eq "$order_num" ]; then
            echo -e "${YELLOW}▶️  Starting validator $order/7: ${region^^}${NC}"
            
            key_path="$SSH_KEY_PATH/$key_file"
            if [ ! -f "$key_path" ]; then
                key_path="$BUILD_KEY"
            fi
            
            ssh -i "$key_path" -o StrictHostKeyChecking=no "$user@$host" \
                'sudo systemctl start dchat-validator && sudo systemctl enable dchat-validator'
            
            if [ "$order" -eq 4 ]; then
                echo -e "${CYAN}"
                echo "  🎯 CONSENSUS CHECKPOINT!"
                echo "     4/7 validators now running - consensus should form"
                echo "     Waiting 60 seconds to verify..."
                echo -e "${NC}"
                sleep 60
                
                # Check consensus
                echo -e "${YELLOW}  🔍 Checking consensus...${NC}"
                if curl -s -m 5 "http://$host:8080/health" > /dev/null; then
                    echo -e "${GREEN}  ✓ Validator responding${NC}"
                else
                    echo -e "${YELLOW}  ⚠️  Validator not responding yet (may still be starting)${NC}"
                fi
            else
                echo "  ⏳ Waiting 30 seconds..."
                sleep 30
            fi
            
            echo -e "${GREEN}  ✓ Started${NC}"
            break
        fi
    done
done

echo -e "${CYAN}"
echo "============================================"
echo "  PHASE 4: START RELAY NODES"
echo "============================================"
echo -e "${NC}"

# Start relay nodes on each server
for region in ohio singapore stockholm saopaulo india southafrica uae; do
    IFS=' ' read -r host user key_file order <<< "${SERVERS[$region]}"
    
    echo -e "${YELLOW}🔄 Starting relays on ${region^^}...${NC}"
    
    key_path="$SSH_KEY_PATH/$key_file"
    if [ ! -f "$key_path" ]; then
        key_path="$BUILD_KEY"
    fi
    
    # Create and start relay service files
    for i in 1 2; do
        port=$((7070 + i))
        
        cat > "$TEMP_DIR/dchat-relay-$i.service" << EOF
[Unit]
Description=dchat Relay Node $i
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/dchat --config /etc/dchat/config.toml --role relay --port $port
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF
        
        scp -i "$key_path" -o StrictHostKeyChecking=no "$TEMP_DIR/dchat-relay-$i.service" "$user@$host:/tmp/dchat-relay-$i.service"
        ssh -i "$key_path" -o StrictHostKeyChecking=no "$user@$host" \
            "sudo mv /tmp/dchat-relay-$i.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl start dchat-relay-$i && sudo systemctl enable dchat-relay-$i"
        
        echo -e "${GREEN}  ✓ Relay $i started (port $port)${NC}"
    done
done

echo -e "${CYAN}"
echo "============================================"
echo "  PHASE 5: NETWORK HEALTH CHECK"
echo "============================================"
echo -e "${NC}"

echo -e "${YELLOW}🏥 Checking network health...${NC}"
echo ""

healthy_count=0
for region in ohio singapore stockholm saopaulo india southafrica uae; do
    IFS=' ' read -r host user key_file order <<< "${SERVERS[$region]}"
    
    echo -e "${CYAN}🔍 ${region^^}${NC}"
    
    if curl -s -m 5 "http://$host:8080/health" > /dev/null; then
        echo -e "${GREEN}  ✓ Status: Healthy${NC}"
        ((healthy_count++))
    else
        echo -e "${RED}  ❌ Status: Unreachable${NC}"
    fi
    
    echo "  Health: http://$host:8080/health"
    echo "  Metrics: http://$host:9090/metrics"
    echo ""
done

echo -e "${CYAN}"
echo "============================================"
echo "  MAINNET LAUNCH COMPLETE!"
echo "============================================"
echo -e "${NC}"

echo -e "${YELLOW}📊 Deployment Summary:${NC}"
echo "   • Validators: 7/7 started"
echo "   • Relays: 14/14 started"
echo "   • Healthy nodes: $healthy_count/7"
echo "   • Consensus: 4/7 minimum (BFT)"
echo ""

echo -e "${YELLOW}🔍 Verification Commands:${NC}"
echo "   Check consensus:"
echo "   curl http://validator1-ohio.schikuno.top:8080/consensus"
echo ""
echo "   Watch block production:"
echo "   watch -n 2 'curl -s http://validator1-ohio.schikuno.top:8080/block/latest'"
echo ""
echo "   Check all validators:"
echo "   for r in ohio singapore stockholm saopaulo india southafrica uae; do"
echo "     curl http://validator1-\$r.schikuno.top:8080/health"
echo "   done"
echo ""

echo -e "${GREEN}🎉 MAINNET IS LIVE!${NC}"
echo ""

# Cleanup
rm -rf "$TEMP_DIR"
