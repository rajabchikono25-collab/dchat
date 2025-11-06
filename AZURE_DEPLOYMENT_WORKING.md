# Azure 3-Server Deployment Working File

**Date:** November 6, 2025  
**Objective:** Build dchat in WSL, upload binary to 3 Azure servers, establish P2P handshake

---

## Pre-Deployment Checklist

- [ ] Build binary in WSL
- [ ] Identify 3 Azure server IPs/hostnames
- [ ] Upload binary to Server 1
- [ ] Upload binary to Server 2
- [ ] Upload binary to Server 3
- [ ] Configure P2P networking on each server
- [ ] Start services
- [ ] Verify handshake between servers

---

## Server Information

### Server 1 - India (Mumbai)
- **IP/Hostname:** 74.225.183.196
- **Domain:** validator1-india.schikuno.top
- **SSH User:** azureuser
- **SSH Key:** Foundation-servers/Azure-India/uramami.pem
- **Binary Path:** /opt/dchat/dchat
- **Config Path:** /opt/dchat/config.toml
- **Status:** ❌ Not deployed

### Server 2 - South Africa (Johannesburg)
- **IP/Hostname:** 4.221.211.71
- **Domain:** validator1-southafrica.schikuno.top
- **SSH User:** azureuser
- **SSH Key:** Foundation-servers/Azure-SAfrica/anacreon.pem
- **Binary Path:** /opt/dchat/dchat
- **Config Path:** /opt/dchat/config.toml
- **Status:** ❌ Not deployed

### Server 3 - UAE (Dubai)
- **IP/Hostname:** 4.161.34.228
- **Domain:** validator1-uae.schikuno.top
- **SSH User:** azureuser
- **SSH Key:** Foundation-servers/Azure_UAE/Randal_key.pem
- **Binary Path:** /opt/dchat/dchat
- **Config Path:** /opt/dchat/config.toml
- **Status:** ❌ Not deployed

---

## Build Process

### WSL Build Commands
```bash
# Navigate to project directory
cd /mnt/c/Users/USER/dchat

# Build release binary
cargo build --release

# Verify binary
ls -lh target/release/dchat
```

**Build Output:** TBD

---

## Upload Process

### Server 1 Upload
```bash
scp target/release/dchat user@server1:/opt/dchat/
```
**Status:** ❌ Pending

### Server 2 Upload
```bash
scp target/release/dchat user@server2:/opt/dchat/
```
**Status:** ❌ Pending

### Server 3 Upload
```bash
scp target/release/dchat user@server3:/opt/dchat/
```
**Status:** ❌ Pending

---

## P2P Configuration

### Bootstrap Nodes
Server 1 will be the primary bootstrap node:
- **Server 1 Peer ID:** TBD
- **Server 1 Multiaddr:** TBD

### Configuration Files
Each server needs:
1. Unique identity (peer ID)
2. Bootstrap peers list pointing to other servers
3. Listening address (0.0.0.0:9090)
4. Proper NAT configuration

---

## Service Start Commands

### Server 1 (Bootstrap Node)
```bash
ssh user@server1
cd /opt/dchat
./dchat --role relay --config config.toml
```

### Server 2
```bash
ssh user@server2
cd /opt/dchat
./dchat --role relay --config config.toml
```

### Server 3
```bash
ssh user@server3
cd /opt/dchat
./dchat --role relay --config config.toml
```

---

## Verification Steps

### Check Connectivity
```bash
# On each server, check peer connections
ssh user@server1 'journalctl -u dchat -n 50 | grep -i "peer\|connect\|handshake"'
ssh user@server2 'journalctl -u dchat -n 50 | grep -i "peer\|connect\|handshake"'
ssh user@server3 'journalctl -u dchat -n 50 | grep -i "peer\|connect\|handshake"'
```

### Expected Log Messages
- ✅ "Peer connected"
- ✅ "Handshake completed"
- ✅ "DHT bootstrapped"
- ✅ "Joined network"

---

## Troubleshooting Notes

### Common Issues
1. **Port 9090 blocked:** Ensure Azure NSG allows inbound/outbound on 9090
2. **Bootstrap peers incorrect:** Verify multiaddrs are correct format
3. **Keys not generated:** Run generate-validator-keys.ps1 first
4. **NAT issues:** Check UPnP/TURN configuration

### Debugging Commands
```bash
# Check if dchat is running
ps aux | grep dchat

# Check port listening
netstat -tlnp | grep 9090

# Check firewall
sudo ufw status

# View full logs
journalctl -u dchat -f
```

---

## Deployment Log

### Build Phase
- **Start Time:** $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")
- **Duration:** In progress
- **Status:** 🔄 Using deploy-azure-testnet.ps1 script

### Upload Phase
- **Start Time:** TBD
- **Duration:** TBD
- **Status:** ❌ Not started

### Configuration Phase
- **Start Time:** TBD
- **Duration:** TBD
- **Status:** ❌ Not started

### Service Start Phase
- **Start Time:** TBD
- **Duration:** TBD
- **Status:** ❌ Not started

### Verification Phase
- **Start Time:** TBD
- **Duration:** TBD
- **Status:** ❌ Not started

---

## Final Status

**Overall Deployment Status:** ❌ Not started  
**Servers Connected:** 0/3  
**Handshakes Successful:** 0/3  
**Network Health:** Unknown

---

## Notes

- Use WSL Ubuntu for consistent build environment
- All servers should run the same binary version
- Ensure time synchronization across servers
- Monitor network latency between servers
- Keep this file updated with actual IPs/credentials once discovered
