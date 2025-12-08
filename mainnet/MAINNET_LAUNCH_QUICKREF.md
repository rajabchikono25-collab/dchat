# dchat Mainnet Launch Quick Reference
**Date**: December 8, 2025  
**Network**: dchat-foundation-mainnet

## 🌍 Infrastructure Overview

| Role | Node Name | IP | DNS | User | SSH Key |
|------|-----------|-----|-----|------|---------|
| **Validator** | India | 74.225.183.196 | validator.india.schikuno.top | azureuser | uramami.pem |
| **Validator** | South Africa | 4.221.211.71 | validator.southafrica.schikuno.top | azureuser | anacreon.pem |
| **Validator** | UAE | 4.161.34.228 | validator.uae.schikuno.top | azureuser | Randal_key.pem |
| **Relay** | Ohio | 13.58.182.122 | relay.ohio.schikuno.top | ubuntu | gecko.pem |
| **Relay** | Stockholm | 16.16.212.80 | relay.stockholm.schikuno.top | ubuntu | restock.pem |
| **Relay** | São Paulo | 18.230.144.17 | relay.saopaulo.schikuno.top | ubuntu | pablo.pem |
| **User** | Singapore | 13.251.102.178 | user.singapore.schikuno.top | ubuntu | craig.pem |

## 🔑 Generated Keys

| Node | Identity File | Password |
|------|---------------|----------|
| validator-india | validator1-india.json | indiamain123 |
| validator-southafrica | validator2-southafrica.json | SAmain123 |
| validator-uae | validator3-uae.json | UAEmain123 |
| relay-ohio | relay1-ohio.json | ohiorelay123 |
| relay-stockholm | relay2-stockholm.json | stockholmrelay123 |
| relay-saopaulo | relay3-saopaulo.json | saopaulorelay123 |
| user-singapore | user-singapore.json | singaporeuser123 |

## 🚀 Manual Deployment Steps

### Step 1: SSH to Server

```powershell
# Validators (Azure)
ssh -i "foundation servers\uramami.pem" azureuser@74.225.183.196  # India
ssh -i "foundation servers\anacreon.pem" azureuser@4.221.211.71   # South Africa
ssh -i "foundation servers\Randal_key.pem" azureuser@4.161.34.228 # UAE

# Relays (AWS)
ssh -i "foundation servers\gecko.pem" ubuntu@13.58.182.122   # Ohio
ssh -i "foundation servers\restock.pem" ubuntu@16.16.212.80  # Stockholm
ssh -i "foundation servers\pablo.pem" ubuntu@18.230.144.17   # São Paulo

# User (AWS)
ssh -i "foundation servers\craig.pem" ubuntu@13.251.102.178  # Singapore
```

### Step 2: On Each Server - Install Dependencies

```bash
# Update system
sudo apt update && sudo apt upgrade -y

# Install build dependencies
sudo apt install -y build-essential pkg-config libssl-dev curl git

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env

# Create dchat directories
sudo mkdir -p /opt/dchat/{bin,config,keys,data,logs}
sudo chown -R $USER:$USER /opt/dchat
```

### Step 3: Transfer Files (from local machine)

```powershell
# For each server, transfer binary, config, and identity
# Example for India validator:
$key = "foundation servers\uramami.pem"
$server = "azureuser@74.225.183.196"

# Copy Windows-built binary won't work - need to build on server
# Instead copy source and build there:
scp -i $key -r . ${server}:/tmp/dchat-src/

# Or copy just the config and identity:
scp -i $key mainnet\config-validator-india.toml ${server}:/opt/dchat/config/config.toml
scp -i $key mainnet\validator1-india.json ${server}:/opt/dchat/keys/identity.json
```

### Step 4: Build on Server

```bash
cd /tmp/dchat-src
cargo build --release --bin dchat

# Copy binary
cp target/release/dchat /opt/dchat/bin/
chmod +x /opt/dchat/bin/dchat

# Set permissions
chmod 600 /opt/dchat/keys/identity.json
```

### Step 5: Configure Firewall

```bash
sudo ufw allow 7070/tcp comment 'dchat p2p'
sudo ufw allow 8080/tcp comment 'dchat health'  
sudo ufw allow 9090/tcp comment 'dchat metrics'
sudo ufw allow 26657/tcp comment 'tendermint rpc'
sudo ufw --force enable
```

### Step 6: Create Systemd Service

**For Validators:**
```bash
cat << 'EOF' | sudo tee /etc/systemd/system/dchat.service
[Unit]
Description=dchat validator node
After=network.target

[Service]
Type=simple
User=azureuser
WorkingDirectory=/opt/dchat
Environment=RUST_LOG=info
ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml validator --key /opt/dchat/keys/identity.json --chain-rpc http://127.0.0.1:26657 --producer --stake 10000
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable dchat
```

**For Relays:**
```bash
cat << 'EOF' | sudo tee /etc/systemd/system/dchat.service
[Unit]
Description=dchat relay node
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/opt/dchat
Environment=RUST_LOG=info
ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml relay --listen 0.0.0.0:7070 --stake 1000
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable dchat
```

**For User Client:**
```bash
cat << 'EOF' | sudo tee /etc/systemd/system/dchat.service
[Unit]
Description=dchat user client
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/opt/dchat
Environment=RUST_LOG=info
ExecStart=/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml user --non-interactive --username dchat-user
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable dchat
```

## 🎯 Launch Sequence

### Phase 1: Start Validators (in order, 30s apart)
```bash
# 1. India (first)
sudo systemctl start dchat
sudo journalctl -u dchat -f
# Wait 30 seconds

# 2. South Africa
sudo systemctl start dchat

# 3. UAE
sudo systemctl start dchat
```

### Phase 2: Start Relays (after validators are up)
```bash
# All 3 relays can start simultaneously
sudo systemctl start dchat
```

### Phase 3: Start User Client
```bash
sudo systemctl start dchat
```

## 📊 Monitoring Commands

```bash
# Check service status
sudo systemctl status dchat

# View logs (live)
sudo journalctl -u dchat -f

# View recent logs
sudo journalctl -u dchat -n 100

# Health check
curl http://localhost:8080/health

# Metrics
curl http://localhost:9090/metrics

# Stop service
sudo systemctl stop dchat

# Restart service
sudo systemctl restart dchat
```

## 🔧 Troubleshooting

### Service won't start
```bash
# Check detailed logs
sudo journalctl -u dchat -n 200 --no-pager

# Check if port is in use
sudo netstat -tulpn | grep 7070

# Test config manually
/opt/dchat/bin/dchat -c /opt/dchat/config/config.toml --help
```

### Peers not connecting
```bash
# Check firewall
sudo ufw status

# Test connectivity
nc -zv validator.india.schikuno.top 7070

# Check DNS resolution
dig validator.india.schikuno.top
```

### High memory usage
```bash
# Check memory
free -h

# Restart service
sudo systemctl restart dchat
```

## 📁 Directory Structure

```
/opt/dchat/
├── bin/
│   └── dchat           # Binary
├── config/
│   └── config.toml     # Configuration
├── keys/
│   └── identity.json   # Node identity (encrypted)
├── data/               # Runtime data
└── logs/               # Log files (optional)
```

## 🛡️ Security Checklist

- [ ] SSH keys secured (chmod 600)
- [ ] Identity files secured (chmod 600)
- [ ] Firewall enabled (ufw)
- [ ] Only necessary ports open
- [ ] SSH root login disabled
- [ ] Regular system updates scheduled
