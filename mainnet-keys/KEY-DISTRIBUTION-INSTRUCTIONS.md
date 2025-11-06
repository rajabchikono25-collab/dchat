# Validator Key Distribution Instructions

**Generated**: 2025-11-06 14:51:53 UTC
**Network**: Mainnet
**Total Validators**: 7

---

## 🔐 Security Requirements

### CRITICAL - READ BEFORE PROCEEDING

1. **NEVER commit private keys to git**
2. **BACKUP all keys to encrypted storage immediately**
3. **Use secure channels for key distribution (never email/Slack)**
4. **Verify checksums after transferring keys**
5. **Delete keys from local machine after secure backup**

---

## 📋 Distribution Checklist

### OHIO - AWS
- [ ] Backup private key to encrypted storage
- [ ] Transfer private key to validator1-ohio.schikuno.top via secure channel
- [ ] Verify checksum: ```powershell
      Get-FileHash ".\mainnet-keys\validator-ohio.key" -Algorithm SHA256
      ```
- [ ] Place key in: `/etc/dchat/keys/validator.key`
- [ ] Set permissions: `chmod 600 /etc/dchat/keys/validator.key`
- [ ] Verify validator can read key
- [ ] Delete key from transfer location
- [ ] Confirm backup is accessible

**Public Key**: `e26a3592fbffe18419e083e5728935cd17d4b68b67f36dd1bc16d6c60baccf2a`
**Subdomain**: `validator1-ohio.schikuno.top`

### SINGAPORE - AWS
- [ ] Backup private key to encrypted storage
- [ ] Transfer private key to validator1-singapore.schikuno.top via secure channel
- [ ] Verify checksum: ```powershell
      Get-FileHash ".\mainnet-keys\validator-singapore.key" -Algorithm SHA256
      ```
- [ ] Place key in: `/etc/dchat/keys/validator.key`
- [ ] Set permissions: `chmod 600 /etc/dchat/keys/validator.key`
- [ ] Verify validator can read key
- [ ] Delete key from transfer location
- [ ] Confirm backup is accessible

**Public Key**: `14b66a5114968f83f8496340f13dbc64a1f1ad68b8a17f911610cb422e25ec30`
**Subdomain**: `validator1-singapore.schikuno.top`

### STOCKHOLM - AWS
- [ ] Backup private key to encrypted storage
- [ ] Transfer private key to validator1-stockholm.schikuno.top via secure channel
- [ ] Verify checksum: ```powershell
      Get-FileHash ".\mainnet-keys\validator-stockholm.key" -Algorithm SHA256
      ```
- [ ] Place key in: `/etc/dchat/keys/validator.key`
- [ ] Set permissions: `chmod 600 /etc/dchat/keys/validator.key`
- [ ] Verify validator can read key
- [ ] Delete key from transfer location
- [ ] Confirm backup is accessible

**Public Key**: `8438736db20a2a800780680c3cb7823bd217d1a53873cab275d1dd54c20e8185`
**Subdomain**: `validator1-stockholm.schikuno.top`

### SAOPAULO - AWS
- [ ] Backup private key to encrypted storage
- [ ] Transfer private key to validator1-saopaulo.schikuno.top via secure channel
- [ ] Verify checksum: ```powershell
      Get-FileHash ".\mainnet-keys\validator-saopaulo.key" -Algorithm SHA256
      ```
- [ ] Place key in: `/etc/dchat/keys/validator.key`
- [ ] Set permissions: `chmod 600 /etc/dchat/keys/validator.key`
- [ ] Verify validator can read key
- [ ] Delete key from transfer location
- [ ] Confirm backup is accessible

**Public Key**: `0fc82f04eaec83cca7dea8a732e1aa711e5587e5a5be1bb09acc428fe44e8411`
**Subdomain**: `validator1-saopaulo.schikuno.top`

### INDIA - Azure
- [ ] Backup private key to encrypted storage
- [ ] Transfer private key to validator1-india.schikuno.top via secure channel
- [ ] Verify checksum: ```powershell
      Get-FileHash ".\mainnet-keys\validator-india.key" -Algorithm SHA256
      ```
- [ ] Place key in: `/etc/dchat/keys/validator.key`
- [ ] Set permissions: `chmod 600 /etc/dchat/keys/validator.key`
- [ ] Verify validator can read key
- [ ] Delete key from transfer location
- [ ] Confirm backup is accessible

**Public Key**: `1e3e684e97aa00bc5d22084d2ca75cfe911ee6fb90e1277589d40b8ab8799579`
**Subdomain**: `validator1-india.schikuno.top`

### SOUTHAFRICA - Azure
- [ ] Backup private key to encrypted storage
- [ ] Transfer private key to validator1-southafrica.schikuno.top via secure channel
- [ ] Verify checksum: ```powershell
      Get-FileHash ".\mainnet-keys\validator-southafrica.key" -Algorithm SHA256
      ```
- [ ] Place key in: `/etc/dchat/keys/validator.key`
- [ ] Set permissions: `chmod 600 /etc/dchat/keys/validator.key`
- [ ] Verify validator can read key
- [ ] Delete key from transfer location
- [ ] Confirm backup is accessible

**Public Key**: `b709ae1e971d1d9b77a092c09f9718065a23c72bc83c7077c49686d17570c62c`
**Subdomain**: `validator1-southafrica.schikuno.top`

### UAE - Azure
- [ ] Backup private key to encrypted storage
- [ ] Transfer private key to validator1-uae.schikuno.top via secure channel
- [ ] Verify checksum: ```powershell
      Get-FileHash ".\mainnet-keys\validator-uae.key" -Algorithm SHA256
      ```
- [ ] Place key in: `/etc/dchat/keys/validator.key`
- [ ] Set permissions: `chmod 600 /etc/dchat/keys/validator.key`
- [ ] Verify validator can read key
- [ ] Delete key from transfer location
- [ ] Confirm backup is accessible

**Public Key**: `b8b88bdca8c84479c0bb1f872c8a62890d87bed2b21b76c0b160c9812dcc1494`
**Subdomain**: `validator1-uae.schikuno.top`

---

## 🚀 Deployment Steps

### 1. Backup Keys (CRITICAL)
```powershell
# Encrypt and backup entire keys directory
Compress-Archive -Path ".\mainnet-keys" -DestinationPath "mainnet-keys-backup-$(Get-Date -Format 'yyyyMMdd-HHmmss').zip"
# Move backup to encrypted storage (BitLocker, VeraCrypt, cloud with encryption)
```

### 2. Distribute to Validators
For each validator server:
```bash
# On validator server
sudo mkdir -p /etc/dchat/keys
sudo chown dchat:dchat /etc/dchat/keys
sudo chmod 700 /etc/dchat/keys

# Transfer key securely (use scp with key authentication)
scp validator-REGION.key user@validator1-REGION.schikuno.top:/tmp/
ssh user@validator1-REGION.schikuno.top
sudo mv /tmp/validator-REGION.key /etc/dchat/keys/validator.key
sudo chown dchat:dchat /etc/dchat/keys/validator.key
sudo chmod 600 /etc/dchat/keys/validator.key

# Verify
sudo -u dchat cat /etc/dchat/keys/validator.key
```

### 3. Update Genesis Configuration
Add all validator public keys to the genesis block configuration:
```toml
[genesis.validators]
ohio = "e26a3592fbffe18419e083e5728935cd17d4b68b67f36dd1bc16d6c60baccf2a"
singapore = "14b66a5114968f83f8496340f13dbc64a1f1ad68b8a17f911610cb422e25ec30"
stockholm = "8438736db20a2a800780680c3cb7823bd217d1a53873cab275d1dd54c20e8185"
saopaulo = "0fc82f04eaec83cca7dea8a732e1aa711e5587e5a5be1bb09acc428fe44e8411"
india = "1e3e684e97aa00bc5d22084d2ca75cfe911ee6fb90e1277589d40b8ab8799579"
southafrica = "b709ae1e971d1d9b77a092c09f9718065a23c72bc83c7077c49686d17570c62c"
uae = "b8b88bdca8c84479c0bb1f872c8a62890d87bed2b21b76c0b160c9812dcc1494"
```

### 4. Verify Key Distribution
```powershell
# Check each validator can access its key
.\mainnet-commands.ps1 check-validator-keys
```

---

## 🔒 Security Verification

After distribution:
- [ ] All private keys backed up to encrypted storage
- [ ] All private keys transferred securely
- [ ] All private keys have correct permissions (600)
- [ ] All validators can read their keys
- [ ] Original keys deleted from local machine
- [ ] Backup tested and accessible
- [ ] No keys in version control
- [ ] No keys in unencrypted locations

---

## 📞 Emergency Procedures

### If Key Compromised
1. **Immediately** revoke compromised validator
2. Generate new keypair for that validator
3. Update genesis configuration
4. Restart network with new validator set
5. Investigate breach source

### If Key Lost
1. Use backup to restore
2. If no backup exists, generate new key
3. Update validator registration on-chain
4. May require governance vote for validator replacement

---

**Generated**: 2025-11-06 14:51:53 UTC
