# Using Existing SSH Keys for Deployment

Your existing SSH keys have been discovered and configured for automated deployment.

## 📁 Discovered SSH Keys

| Region | Server | Key File | Status |
|--------|--------|----------|--------|
| Ohio | validator1-ohio.schikuno.top | `Foundation-servers/AWS-Ohio/gecko.pem` | ✅ |
| São Paulo | validator1-saopaulo.schikuno.top | `Foundation-servers/AWS-Sao-Paulo/pablo.pem` | ✅ |
| Singapore | validator1-singapore.schikuno.top | `Foundation-servers/AWS-Singapore/craig.pem` | ✅ |
| Stockholm | validator1-stockholm.schikuno.top | `Foundation-servers/AWS-Stokholm/relay.pem` | ✅ |
| India | validator1-india.schikuno.top | `Foundation-servers/Azure-India/uramami.pem` | ✅ |
| South Africa | validator1-southafrica.schikuno.top | `Foundation-servers/Azure-SAfrica/anacreon.pem` | ✅ |
| UAE | validator1-uae.schikuno.top | `Foundation-servers/Azure_UAE/Randal_key.pem` | ✅ |

## 🔐 Key Permissions (Important!)

Before using these keys, ensure they have the correct permissions:

### Windows (PowerShell)
```powershell
# Navigate to each key directory and set permissions
cd Foundation-servers\AWS-Ohio
icacls gecko.pem /inheritance:r
icacls gecko.pem /grant:r "$($env:USERNAME):(R)"

# Repeat for all keys, or use this script:
Get-ChildItem -Path Foundation-servers -Recurse -Filter *.pem | ForEach-Object {
    icacls $_.FullName /inheritance:r
    icacls $_.FullName /grant:r "$($env:USERNAME):(R)"
    Write-Host "Fixed permissions for: $($_.FullName)" -ForegroundColor Green
}
```

### Linux/macOS (Bash)
```bash
# Set correct permissions (400 = read-only for owner)
chmod 400 Foundation-servers/AWS-Ohio/gecko.pem
chmod 400 Foundation-servers/AWS-Sao-Paulo/pablo.pem
chmod 400 Foundation-servers/AWS-Singapore/craig.pem
chmod 400 Foundation-servers/AWS-Stokholm/relay.pem
chmod 400 Foundation-servers/Azure-India/uramami.pem
chmod 400 Foundation-servers/Azure-SAfrica/anacreon.pem
chmod 400 Foundation-servers/Azure_UAE/Randal_key.pem

# Or fix all at once:
find Foundation-servers -name "*.pem" -exec chmod 400 {} \;
```

## 🧪 Test Connectivity

Before running the full deployment, test SSH connectivity:

### Windows
```powershell
cd ansible
.\test-connection.ps1
```

### Linux/macOS
```bash
cd ansible
chmod +x test-connection.sh
./test-connection.sh
```

This will:
- Check if all key files exist
- Verify SSH connectivity to each validator
- Confirm the correct username (ubuntu)
- Report any connection issues

## 🚀 Deploy with Ansible

Once connectivity is confirmed:

```bash
cd ansible

# Test with Ansible ping
ansible validators -i inventory.ini -m ping

# Deploy storage services to all validators
ansible-playbook -i inventory.ini playbook.yml

# Deploy to specific region
ansible-playbook -i inventory.ini playbook.yml --limit validator1-ohio.schikuno.top

# Deploy to AWS validators only
ansible-playbook -i inventory.ini playbook.yml --limit aws_validators
```

## 🔧 Manual SSH Connection

To manually connect to any validator:

### Ohio
```bash
ssh -i Foundation-servers/AWS-Ohio/gecko.pem ubuntu@3.134.77.79
# or
ssh -i Foundation-servers/AWS-Ohio/gecko.pem ubuntu@validator1-ohio.schikuno.top
```

### São Paulo
```bash
ssh -i Foundation-servers/AWS-Sao-Paulo/pablo.pem ubuntu@54.207.201.126
```

### Singapore
```bash
ssh -i Foundation-servers/AWS-Singapore/craig.pem ubuntu@13.212.237.87
```

### Stockholm
```bash
ssh -i Foundation-servers/AWS-Stokholm/relay.pem ubuntu@13.50.244.122
```

### India
```bash
ssh -i Foundation-servers/Azure-India/uramami.pem ubuntu@74.225.183.196
```

### South Africa
```bash
ssh -i Foundation-servers/Azure-SAfrica/anacreon.pem ubuntu@4.221.211.71
```

### UAE
```bash
ssh -i Foundation-servers/Azure_UAE/Randal_key.pem ubuntu@4.161.34.228
```

## 📝 Updated Configuration

The Ansible inventory has been updated to use these keys:

```ini
[validators]
validator1-ohio.schikuno.top ansible_host=3.134.77.79 region=ohio ansible_ssh_private_key_file=../Foundation-servers/AWS-Ohio/gecko.pem
validator1-saopaulo.schikuno.top ansible_host=54.207.201.126 region=saopaulo ansible_ssh_private_key_file=../Foundation-servers/AWS-Sao-Paulo/pablo.pem
# ... etc
```

## ⚠️ Troubleshooting

### "Permission denied (publickey)"
```bash
# Verify key permissions
ls -la Foundation-servers/AWS-Ohio/gecko.pem
# Should show: -r-------- (400)

# Fix permissions
chmod 400 Foundation-servers/AWS-Ohio/gecko.pem
```

### "Connection refused"
```bash
# Check if SSH port is open
nc -zv 3.134.77.79 22

# Check security group rules
aws ec2 describe-security-groups --region us-east-2
```

### Wrong username
Different cloud providers use different default users:
- **AWS Ubuntu**: `ubuntu`
- **AWS Amazon Linux**: `ec2-user`
- **Azure Ubuntu**: `azureuser` or `ubuntu`

If `ubuntu` doesn't work, try:
```bash
ssh -i Foundation-servers/AWS-Ohio/gecko.pem ec2-user@3.134.77.79
# or
ssh -i Foundation-servers/Azure-India/uramami.pem azureuser@74.225.183.196
```

### Test single server
```bash
# Verbose SSH for debugging
ssh -v -i Foundation-servers/AWS-Ohio/gecko.pem ubuntu@3.134.77.79

# Test with Ansible
ansible validator1-ohio.schikuno.top -i ansible/inventory.ini -m ping -vvv
```

## 🎯 Next Steps

1. **Fix key permissions** (see above)
2. **Test connectivity**: `cd ansible && .\test-connection.ps1`
3. **Set environment variables**:
   ```powershell
   $env:MINIO_ROOT_PASSWORD = "YourSecurePassword123!"
   $env:REDIS_PASSWORD = "YourRedisPassword123"
   ```
4. **Deploy**: `ansible-playbook -i inventory.ini playbook.yml`
5. **Verify**: SSH into each validator and run `./status.sh`

## 🔒 Security Best Practices

1. **Never commit .pem files to git**:
   ```bash
   # Already in .gitignore, but verify:
   echo "*.pem" >> .gitignore
   ```

2. **Backup keys securely**:
   ```bash
   # Encrypt and backup
   tar -czf keys-backup.tar.gz Foundation-servers/*/*.pem
   gpg -c keys-backup.tar.gz
   rm keys-backup.tar.gz
   ```

3. **Rotate keys regularly**:
   ```bash
   # Generate new key pair
   ssh-keygen -t ed25519 -f new-validator-key
   
   # Add to authorized_keys on servers
   ssh-copy-id -i new-validator-key ubuntu@validator1-ohio.schikuno.top
   ```

4. **Use SSH config for convenience**:
   ```bash
   # Add to ~/.ssh/config
   Host validator-ohio
       HostName validator1-ohio.schikuno.top
       User ubuntu
       IdentityFile ~/dchat/Foundation-servers/AWS-Ohio/gecko.pem
       StrictHostKeyChecking no
   
   # Then connect with: ssh validator-ohio
   ```

---

**Status**: SSH keys discovered and configured ✅  
**Ready for**: Ansible deployment  
**Next**: Test connectivity and deploy storage services
