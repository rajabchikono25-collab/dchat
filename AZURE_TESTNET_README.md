# Azure Testnet Deployment Guide

This guide covers deploying dchat to the three Azure validators for pre-mainnet testing.

## 🌍 Azure Testnet Infrastructure

### Validator Nodes

| Region | Location | IP Address | Domain |
|--------|----------|------------|--------|
| **India** | Mumbai | 74.225.183.196 | validator1-india.schikuno.top |
| **South Africa** | Johannesburg | 4.221.211.71 | validator1-southafrica.schikuno.top |
| **UAE** | Dubai | 4.161.34.228 | validator1-uae.schikuno.top |

### Network Details

- **Provider:** Microsoft Azure
- **SSH User:** `azureuser`
- **Geographic Coverage:** 3 continents (Asia, Africa, Middle East)
- **Purpose:** Pre-mainnet testing and validation

## 🚀 Quick Start

### 1. Deploy to All Azure Servers

```powershell
# Full deployment (build + deploy)
.\deploy-azure-testnet.ps1

# Skip build (use existing binary)
.\deploy-azure-testnet.ps1 -SkipBuild

# Skip backup (faster, but no rollback)
.\deploy-azure-testnet.ps1 -SkipBackup

# Verbose output
.\deploy-azure-testnet.ps1 -Verbose
```

### 2. Check Network Status

```powershell
# Quick status check (no deployment)
.\deploy-azure-testnet.ps1 -StatusOnly
```

### 3. Monitor Logs

```powershell
# Real-time log streaming from all 3 servers
.\monitor-azure-logs.ps1
```

### 4. Test Connectivity

```powershell
# Comprehensive connectivity test
.\test-azure-connectivity.ps1
```

## 📋 Deployment Process

The deployment script performs these steps automatically:

1. **Prerequisites Check**
   - Verifies WSL is installed
   - Checks SSH keys exist
   - Validates key permissions

2. **Build Binary** (optional, can skip with `-SkipBuild`)
   - Builds dchat in WSL: `cargo build --release --bin dchat`
   - Takes ~3-5 minutes
   - Produces optimized Linux binary (~15MB)

3. **Deploy to Each Server**
   - Tests SSH connectivity
   - Backs up existing binary (optional, can skip with `-SkipBackup`)
   - Copies new binary via SCP
   - Installs to `/opt/dchat/dchat`
   - Restarts systemd service
   - Verifies health endpoint

4. **Network Status Check**
   - Pings all servers
   - Queries health endpoints
   - Checks block height and peer count
   - Displays summary

## 🔑 SSH Access

### Manual Connection

**India:**
```bash
ssh -i Foundation-servers/Azure-India/uramami.pem azureuser@74.225.183.196
```

**South Africa:**
```bash
ssh -i Foundation-servers/Azure-SAfrica/anacreon.pem azureuser@4.221.211.71
```

**UAE:**
```bash
ssh -i Foundation-servers/Azure_UAE/Randal_key.pem azureuser@4.161.34.228
```

### Key File Locations

All SSH keys are located in the `Foundation-servers/` directory:

```
Foundation-servers/
├── Azure-India/
│   └── uramami.pem
├── Azure-SAfrica/
│   └── anacreon.pem
└── Azure_UAE/
    └── Randal_key.pem
```

## 🔍 Monitoring & Debugging

### Check Service Status

```bash
# On any Azure server
sudo systemctl status dchat
sudo journalctl -u dchat -f
```

### Health Endpoints

- **India:** http://74.225.183.196/health
- **South Africa:** http://4.221.211.71/health
- **UAE:** http://4.161.34.228/health

### Status Endpoints (if available)

- **India:** http://74.225.183.196/status
- **South Africa:** http://4.221.211.71/status
- **UAE:** http://4.161.34.228/status

### View Recent Logs

```bash
# Last 50 lines
sudo journalctl -u dchat -n 50

# Follow in real-time
sudo journalctl -u dchat -f

# Show logs from last hour
sudo journalctl -u dchat --since "1 hour ago"
```

### Check Block Production

```bash
# Look for block production messages
sudo journalctl -u dchat | grep "Produced block"

# Check validator stats
sudo journalctl -u dchat | grep "Validator stats"
```

## 🔧 Manual Operations

### Restart Service

```bash
sudo systemctl restart dchat
```

### Stop Service

```bash
sudo systemctl stop dchat
```

### View Configuration

```bash
cat /opt/dchat/config.toml
```

### Check Binary Version

```bash
/opt/dchat/dchat --version
```

### View Validator Key

```bash
sudo cat /opt/dchat/keys/validator.key
```

## 📊 Expected Output

### Successful Deployment

```
==> Deploying to India (Mumbai)
IP: 74.225.183.196
Domain: validator1-india.schikuno.top
ℹ Testing SSH connectivity...
✓ Connected to India
ℹ Copying binary to server...
✓ Binary copied to server
ℹ Installing binary and restarting service...
✓ Service restarted on India
ℹ Checking health endpoint...
✓ Health check passed: healthy
✓ Deployment to India complete!
```

### Network Status

```
Azure Testnet Status:
================================================================================
✓ India (Mumbai)
   IP: 74.225.183.196
   Reachable: Yes
   Healthy: Yes
   Block Height: 42
   Peers: 6
   Version: 0.1.0

✓ South Africa (Johannesburg)
   IP: 4.221.211.71
   Reachable: Yes
   Healthy: Yes
   Block Height: 42
   Peers: 6
   Version: 0.1.0

✓ UAE (Dubai)
   IP: 4.161.34.228
   Reachable: Yes
   Healthy: Yes
   Block Height: 42
   Peers: 6
   Version: 0.1.0

================================================================================
Summary: 3/3 validators healthy
✓ All Azure validators are operational! 🎉
```

## 🐛 Troubleshooting

### Issue: SSH Connection Fails

**Solution:**
```powershell
# Check if key file exists
Test-Path Foundation-servers/Azure-India/uramami.pem

# Fix key permissions (Windows)
icacls Foundation-servers\Azure-India\uramami.pem /inheritance:r
icacls Foundation-servers\Azure-India\uramami.pem /grant:r "$($env:USERNAME):(R)"
```

### Issue: Binary Won't Start

**Solution:**
```bash
# Check binary is executable
ls -l /opt/dchat/dchat
sudo chmod +x /opt/dchat/dchat

# Check logs for errors
sudo journalctl -u dchat -n 100

# Try starting manually
sudo /opt/dchat/dchat validator --key keys/validator.key
```

### Issue: Health Endpoint Not Responding

**Possible Causes:**
1. Service not started yet (wait 10-30 seconds)
2. Port 80 blocked by firewall
3. Service crashed (check logs)

**Solutions:**
```bash
# Check if service is running
sudo systemctl status dchat

# Check if port 80 is open
sudo netstat -tlnp | grep :80

# Restart service
sudo systemctl restart dchat
```

### Issue: No Peers Connecting

**Solution:**
```bash
# Check if port 9090 is open
sudo ufw status
sudo ufw allow 9090/tcp
sudo ufw allow 9090/udp

# Verify bootstrap peers in config
cat /opt/dchat/config.toml | grep bootstrap_peers

# Check network connectivity
ping 54.233.203.82  # São Paulo AWS
ping 13.48.49.2     # Stockholm AWS
```

## 🎯 Pre-Mainnet Testing Checklist

Before going to full mainnet, verify:

- [ ] All 3 Azure validators are running
- [ ] Health endpoints returning "healthy"
- [ ] Block production is active
- [ ] Validators have 6-7 peers connected
- [ ] No errors in logs
- [ ] Services restart automatically after reboot
- [ ] Cross-region connectivity working (AWS ↔ Azure)
- [ ] Block heights synchronized across all validators
- [ ] Governance proposals can be created and voted on
- [ ] Token transfers work between accounts
- [ ] Message delivery working end-to-end

## 🔄 Rollback Procedure

If deployment fails, you can rollback:

```bash
# On affected server
sudo systemctl stop dchat

# List available backups
ls -lh /opt/dchat/dchat.backup.*

# Restore previous version
sudo cp /opt/dchat/dchat.backup.20251105_120000 /opt/dchat/dchat
sudo chmod +x /opt/dchat/dchat

# Restart service
sudo systemctl start dchat
```

## 📈 Performance Metrics

Monitor these metrics during testing:

| Metric | Expected Value | Action if Outside Range |
|--------|----------------|-------------------------|
| Block Time | 5-10 seconds | Check consensus logs |
| Peer Count | 6-7 | Check firewall rules |
| Memory Usage | 50-200 MB | Restart if > 500 MB |
| CPU Usage | < 10% | Check for stuck processes |
| Disk I/O | < 10 MB/s | Check database health |

## 🚦 Next Steps

After successful Azure testnet deployment:

1. **Monitor for 24-48 hours** to ensure stability
2. **Run load tests** to verify throughput
3. **Test failure scenarios** (restart nodes, network partitions)
4. **Deploy to AWS validators** (Ohio, Singapore, Stockholm, São Paulo)
5. **Configure full 7-validator network** with cross-cloud connectivity
6. **Prepare mainnet launch** with final configurations

## 📞 Support

If you encounter issues not covered in this guide:

1. Check logs: `sudo journalctl -u dchat -n 100`
2. Verify configuration: `cat /opt/dchat/config.toml`
3. Test connectivity: `.\test-azure-connectivity.ps1`
4. Manual intervention: SSH to affected server
5. Contact DevOps team if issue persists

---

**Status:** Ready for Azure Testnet Deployment  
**Last Updated:** November 5, 2025  
**Maintainer:** dchat Foundation
