# Mainnet Quick Reference

## 🚀 Quick Start Commands

### Deploy Everything
```powershell
# Full deployment to all 7 servers
./deploy-mainnet.ps1

# Dry run first (recommended)
./deploy-mainnet.ps1 -DryRun

# Deploy only validators
./deploy-mainnet.ps1 -ValidatorsOnly

# Deploy only relays
./deploy-mainnet.ps1 -RelaysOnly
```

### Start Network
```powershell
# Start all validators (one by one with 30s delay)
./start-mainnet-validators.ps1 -OneByOne

# Start all relays
./start-mainnet-relays.ps1

# Start only specific region
./start-mainnet-validators.ps1 -OnlyRegion ohio
```

### Monitor Network
```powershell
# Monitor everything continuously (refresh every 10s)
./monitor-mainnet.ps1 -Continuous

# Monitor only validators
./monitor-mainnet.ps1 -Component Validators

# Monitor only relays
./monitor-mainnet.ps1 -Component Relays

# Monitor consensus metrics
./monitor-mainnet.ps1 -Component Consensus

# Monitor storage
./monitor-mainnet.ps1 -Component Storage
```

## 🌐 Server Access

### SSH Commands
```powershell
# Ohio (AWS)
wsl ssh -i Foundation-servers/AWS-Ohio/ohio-key.pem azureuser@validator1-ohio.schikuno.top

# Singapore (AWS)
wsl ssh -i Foundation-servers/AWS-Singapore/singapore-key.pem azureuser@validator1-singapore.schikuno.top

# Stockholm (AWS)
wsl ssh -i Foundation-servers/AWS-Stockholm/stockholm-key.pem azureuser@validator1-stockholm.schikuno.top

# São Paulo (AWS)
wsl ssh -i Foundation-servers/AWS-SaoPaulo/saopaulo-key.pem azureuser@validator1-saopaulo.schikuno.top

# India (Azure)
wsl ssh -i Foundation-servers/Azure-India/uramami.pem azureuser@74.225.183.196

# South Africa (Azure)
wsl ssh -i Foundation-servers/Azure-SAfrica/anacreon.pem azureuser@4.221.211.71

# UAE (Azure)
wsl ssh -i Foundation-servers/Azure_UAE/Randal_key.pem azureuser@4.161.34.228
```

## 📊 Health Check URLs

### Validators
```
http://validator1-ohio.schikuno.top:8080/health
http://validator1-singapore.schikuno.top:8080/health
http://validator1-stockholm.schikuno.top:8080/health
http://validator1-saopaulo.schikuno.top:8080/health
http://validator1-india.schikuno.top:8080/health
http://validator1-southafrica.schikuno.top:8080/health
http://validator1-uae.schikuno.top:8080/health
```

### Relays
```
# Relay 1 (port 8081), Relay 2 (port 8082)
http://validator1-ohio.schikuno.top:8081/health
http://validator1-ohio.schikuno.top:8082/health
```

### Metrics (Prometheus)
```
http://validator1-ohio.schikuno.top:9090/metrics  # Validator
http://validator1-ohio.schikuno.top:9091/metrics  # Relay 1
http://validator1-ohio.schikuno.top:9092/metrics  # Relay 2
```

## 🔧 Service Management (On Server)

### Validator
```bash
# Status
sudo systemctl status dchat-validator

# Start
sudo systemctl start dchat-validator

# Stop
sudo systemctl stop dchat-validator

# Restart
sudo systemctl restart dchat-validator

# Logs (last 100 lines)
sudo journalctl -u dchat-validator -n 100

# Follow logs
sudo journalctl -u dchat-validator -f
```

### Relays
```bash
# Relay 1
sudo systemctl status dchat-relay1
sudo systemctl start dchat-relay1
sudo systemctl stop dchat-relay1
sudo journalctl -u dchat-relay1 -f

# Relay 2
sudo systemctl status dchat-relay2
sudo systemctl start dchat-relay2
sudo systemctl stop dchat-relay2
sudo journalctl -u dchat-relay2 -f
```

### All Services
```bash
# Start all
sudo systemctl start dchat-validator dchat-relay1 dchat-relay2

# Stop all
sudo systemctl stop dchat-validator dchat-relay1 dchat-relay2

# Status all
systemctl status 'dchat-*'
```

## 💾 Storage Management

### Redis
```bash
# Check cluster status
redis-cli -c -h validator1-ohio.schikuno.top -p 6379 cluster info

# Check nodes
redis-cli -c -h validator1-ohio.schikuno.top -p 6379 cluster nodes

# Ping
redis-cli -h validator1-ohio.schikuno.top -p 6379 PING
```

### MinIO
```bash
# Check cluster info
mc admin info dchat-minio

# List buckets
mc ls dchat-minio

# Check specific bucket
mc ls dchat-minio/messages
```

### TiKV
```bash
# Check PD health
curl http://validator1-ohio.schikuno.top:2379/pd/health

# Check TiKV stores
curl http://validator1-ohio.schikuno.top:2379/pd/api/v1/stores
```

## 🚨 Emergency Procedures

### Stop All Validators
```powershell
# From Windows
$servers = @("ohio", "singapore", "stockholm", "saopaulo", "india", "southafrica", "uae")
foreach ($server in $servers) {
    Write-Host "Stopping $server..."
    wsl ssh validator1-$server.schikuno.top "sudo systemctl stop dchat-validator"
}
```

### Emergency Validator Stop (On Server)
```bash
# Graceful stop
sudo systemctl stop dchat-validator

# Force kill if not responding
sudo pkill -9 dchat

# Check if stopped
ps aux | grep dchat
```

### Restart All Validators (Coordinated)
```bash
# Stop all first (coordinate with team)
# Wait for all to stop
# Start all simultaneously

# On each server:
sudo systemctl start dchat-validator
```

## 📈 Key Metrics to Watch

### Healthy Thresholds
- **Validators online**: ≥ 4/7 (minimum for BFT consensus)
- **Relays online**: ≥ 10/14 (good network health)
- **Peer count per validator**: ≥ 3
- **Peer count per relay**: ≥ 5
- **Block height difference**: ≤ 2 blocks
- **Block time**: ≤ 8 seconds (target 6s)

### Warning Thresholds
- **Validators online**: 4-5/7
- **Relays online**: 7-9/14
- **Block height difference**: 3-10 blocks
- **Block time**: 8-12 seconds

### Critical Thresholds (IMMEDIATE ACTION)
- **Validators online**: < 4/7
- **Relays online**: < 7/14
- **Block height difference**: > 10 blocks
- **Block time**: > 15 seconds
- **No blocks produced**: > 1 minute

## 🔍 Troubleshooting Quick Checks

### Validator Not Syncing
```bash
# Check peers
curl http://localhost:8080/health | jq '.peer_count'

# Check logs for connection errors
sudo journalctl -u dchat-validator -n 200 | grep -i "error\|failed\|timeout"

# Check DNS resolution
nslookup validator1-ohio.schikuno.top
```

### Relay Not Connecting
```bash
# Check if validator is reachable
nc -zv validator1-ohio.schikuno.top 7070

# Check firewall
sudo ufw status

# Restart relay
sudo systemctl restart dchat-relay1
```

### High Latency
```bash
# Check network latency to other validators
for region in ohio singapore stockholm saopaulo india southafrica uae; do
  echo "$region: $(ping -c 3 validator1-$region.schikuno.top | grep avg | awk -F'/' '{print $5}')ms"
done
```

## 📝 Log Locations

```
/var/log/syslog                              # System logs
/opt/dchat/data/logs/validator.log           # Validator logs
/opt/dchat/data/logs/relay1.log              # Relay 1 logs
/opt/dchat/data/logs/relay2.log              # Relay 2 logs
```

Or use journalctl:
```bash
sudo journalctl -u dchat-validator -n 100
sudo journalctl -u dchat-relay1 -f
sudo journalctl --since "1 hour ago" | grep dchat
```

## 🔐 Security Quick Checks

```bash
# Check open ports
sudo ss -tulpn | grep dchat

# Check firewall status
sudo ufw status numbered

# Check for unauthorized access attempts
sudo grep "Failed password" /var/log/auth.log | tail -20

# Check validator key permissions
ls -la /opt/dchat/keys/
# Should be: -rw------- (600) azureuser:azureuser
```

## 📞 Escalation

1. **Network issues**: Check `MAINNET_NETWORK_CONFIG.md`
2. **Storage issues**: Check `MAINNET_STORAGE_SETUP.md`
3. **Deployment issues**: Check `MAINNET_LAUNCH_CRITICAL.md`
4. **Architecture questions**: Check `ARCHITECTURE.md`

## 🎯 Most Common Issues

| Issue | Quick Fix | Reference |
|-------|-----------|-----------|
| Validator not connecting to peers | Check DNS resolution, restart validator | MAINNET_NETWORK_CONFIG.md |
| Consensus stuck | Verify ≥4/7 validators online, check block height sync | MAINNET_LAUNCH_CRITICAL.md |
| Relay low peer count | Check validator connectivity, restart relay | MAINNET_NETWORK_CONFIG.md |
| Storage cluster down | Check Redis/MinIO/TiKV status, restart services | MAINNET_STORAGE_SETUP.md |
| High block time | Check network latency between validators | MAINNET_NETWORK_CONFIG.md |
| Validator can't sync | Check if more than 3 blocks behind, may need snapshot | MAINNET_LAUNCH_CRITICAL.md |

---

**Keep this file handy during mainnet operations!**
