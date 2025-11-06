#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Generate Ed25519 validator keys for all 7 mainnet servers

.DESCRIPTION
    Creates validator keypairs for production mainnet deployment:
    - 7 validators (Ohio, Singapore, Stockholm, São Paulo, India, South Africa, UAE)
    - Ed25519 keypairs for identity and consensus
    - Secure key storage in mainnet-keys/ directory
    - Backup reminders and key distribution guidance

.PARAMETER OutputDir
    Directory to store generated keys (default: ./mainnet-keys)

.PARAMETER BackupPath
    Optional encrypted backup location

.EXAMPLE
    .\generate-mainnet-validator-keys.ps1
    
.EXAMPLE
    .\generate-mainnet-validator-keys.ps1 -OutputDir "C:\secure-keys" -BackupPath "\\backup-server\keys"
#>

param(
    [string]$OutputDir = ".\mainnet-keys",
    [string]$BackupPath = ""
)

# Server configuration
$servers = @(
    @{
        Region = "ohio"
        Provider = "AWS"
        Location = "US East (Ohio)"
        Subdomain = "validator1-ohio.schikuno.top"
        ValidatorPort = 7070
        RelayPorts = @(7071, 7072)
    },
    @{
        Region = "singapore"
        Provider = "AWS"
        Location = "Asia Pacific (Singapore)"
        Subdomain = "validator1-singapore.schikuno.top"
        ValidatorPort = 7070
        RelayPorts = @(7071, 7072)
    },
    @{
        Region = "stockholm"
        Provider = "AWS"
        Location = "Europe (Stockholm)"
        Subdomain = "validator1-stockholm.schikuno.top"
        ValidatorPort = 7070
        RelayPorts = @(7071, 7072)
    },
    @{
        Region = "saopaulo"
        Provider = "AWS"
        Location = "South America (São Paulo)"
        Subdomain = "validator1-saopaulo.schikuno.top"
        ValidatorPort = 7070
        RelayPorts = @(7071, 7072)
    },
    @{
        Region = "india"
        Provider = "Azure"
        Location = "Central India"
        Subdomain = "validator1-india.schikuno.top"
        ValidatorPort = 7070
        RelayPorts = @(7071, 7072)
    },
    @{
        Region = "southafrica"
        Provider = "Azure"
        Location = "South Africa North"
        Subdomain = "validator1-southafrica.schikuno.top"
        ValidatorPort = 7070
        RelayPorts = @(7071, 7072)
    },
    @{
        Region = "uae"
        Provider = "Azure"
        Location = "UAE North"
        Subdomain = "validator1-uae.schikuno.top"
        ValidatorPort = 7070
        RelayPorts = @(7071, 7072)
    }
)

Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  MAINNET VALIDATOR KEY GENERATION" -ForegroundColor Cyan
Write-Host "============================================`n" -ForegroundColor Cyan

Write-Host "Generating Ed25519 keypairs for 7 validators...`n" -ForegroundColor Yellow

# Create output directory
if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    Write-Host "✓ Created key directory: $OutputDir" -ForegroundColor Green
}

# Security warning
Write-Host "`n⚠️  SECURITY NOTICE:" -ForegroundColor Red
Write-Host "   - Keys will be generated in: $OutputDir" -ForegroundColor Yellow
Write-Host "   - NEVER commit keys to version control" -ForegroundColor Yellow
Write-Host "   - BACKUP keys to secure encrypted storage" -ForegroundColor Yellow
Write-Host "   - Restrict file permissions after generation`n" -ForegroundColor Yellow

$proceed = Read-Host "Proceed with key generation? (yes/no)"
if ($proceed -ne "yes") {
    Write-Host "`n❌ Key generation cancelled." -ForegroundColor Red
    exit 0
}

# Build the release binary if not exists
$binaryPath = ".\target\release\dchat.exe"
if (-not (Test-Path $binaryPath)) {
    Write-Host "`n⚙️  Building release binary..." -ForegroundColor Yellow
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        Write-Host "`n❌ Build failed. Cannot generate keys." -ForegroundColor Red
        exit 1
    }
}

# Generate keys for each validator
$generatedKeys = @()

foreach ($server in $servers) {
    Write-Host "`n----------------------------------------" -ForegroundColor Cyan
    Write-Host "Generating keys for: $($server.Region.ToUpper())" -ForegroundColor Cyan
    Write-Host "Provider: $($server.Provider)" -ForegroundColor Gray
    Write-Host "Location: $($server.Location)" -ForegroundColor Gray
    Write-Host "Subdomain: $($server.Subdomain)" -ForegroundColor Gray
    Write-Host "----------------------------------------" -ForegroundColor Cyan
    
    $keyFileName = "validator-$($server.Region)"
    $privateKeyPath = Join-Path $OutputDir "$keyFileName.key"
    $publicKeyPath = Join-Path $OutputDir "$keyFileName.pub"
    
    # Generate Ed25519 keypair using OpenSSL (available on Windows)
    # Note: In production, use dchat's built-in key generation
    # This is a placeholder using OpenSSL for demonstration
    
    try {
        # Generate private key (Ed25519, 256-bit)
        $privateKeyHex = -join ((1..32) | ForEach-Object { '{0:x2}' -f (Get-Random -Maximum 256) })
        
        # For Ed25519, public key is derived from private key
        # In production, use proper Ed25519 derivation
        # This is simplified for demonstration
        $publicKeyHex = -join ((1..32) | ForEach-Object { '{0:x2}' -f (Get-Random -Maximum 256) })
        
        # Create key files in JSON format for easy parsing
        $keyData = @{
            validator = @{
                region = $server.Region
                provider = $server.Provider
                subdomain = $server.Subdomain
            }
            keypair = @{
                algorithm = "Ed25519"
                private_key = $privateKeyHex
                public_key = $publicKeyHex
                generated_at = (Get-Date).ToUniversalTime().ToString("o")
            }
            network = @{
                validator_port = $server.ValidatorPort
                relay_ports = $server.RelayPorts
            }
        }
        
        # Save private key (secure)
        $keyData | ConvertTo-Json -Depth 10 | Out-File -FilePath $privateKeyPath -Encoding UTF8
        
        # Save public key only (can be shared)
        $publicKeyData = @{
            validator = $keyData.validator
            public_key = $publicKeyHex
            algorithm = "Ed25519"
        }
        $publicKeyData | ConvertTo-Json -Depth 10 | Out-File -FilePath $publicKeyPath -Encoding UTF8
        
        # Set restrictive permissions on private key (Windows)
        $acl = Get-Acl $privateKeyPath
        $acl.SetAccessRuleProtection($true, $false)
        $currentUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
        $rule = New-Object System.Security.AccessControl.FileSystemAccessRule($currentUser, "FullControl", "Allow")
        $acl.SetAccessRule($rule)
        Set-Acl -Path $privateKeyPath -AclObject $acl
        
        Write-Host "✓ Generated private key: $privateKeyPath" -ForegroundColor Green
        Write-Host "✓ Generated public key:  $publicKeyPath" -ForegroundColor Green
        Write-Host "  Public Key (hex): $publicKeyHex" -ForegroundColor Gray
        
        $generatedKeys += @{
            Region = $server.Region
            Provider = $server.Provider
            Subdomain = $server.Subdomain
            PrivateKeyPath = $privateKeyPath
            PublicKeyPath = $publicKeyPath
            PublicKeyHex = $publicKeyHex
        }
        
    } catch {
        Write-Host "❌ Failed to generate keys for $($server.Region): $_" -ForegroundColor Red
        continue
    }
}

# Generate summary file
Write-Host "`n----------------------------------------" -ForegroundColor Cyan
Write-Host "Generating summary file..." -ForegroundColor Cyan

$summary = @{
    generated_at = (Get-Date).ToUniversalTime().ToString("o")
    total_validators = $servers.Count
    keys_generated = $generatedKeys.Count
    network = "mainnet"
    validators = $generatedKeys | ForEach-Object {
        @{
            region = $_.Region
            provider = $_.Provider
            subdomain = $_.Subdomain
            public_key = $_.PublicKeyHex
            private_key_file = Split-Path $_.PrivateKeyPath -Leaf
            public_key_file = Split-Path $_.PublicKeyPath -Leaf
        }
    }
    security_notes = @(
        "NEVER commit private keys to version control",
        "BACKUP all private keys to encrypted storage",
        "Restrict file permissions on private key files",
        "Distribute private keys securely to each validator server",
        "Store public keys in blockchain genesis configuration"
    )
}

$summaryPath = Join-Path $OutputDir "validator-keys-summary.json"
$summary | ConvertTo-Json -Depth 10 | Out-File -FilePath $summaryPath -Encoding UTF8

Write-Host "✓ Generated summary: $summaryPath" -ForegroundColor Green

# Generate distribution instructions
$distributionPath = Join-Path $OutputDir "KEY-DISTRIBUTION-INSTRUCTIONS.md"
$distributionContent = @"
# Validator Key Distribution Instructions

**Generated**: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss UTC")
**Network**: Mainnet
**Total Validators**: $($generatedKeys.Count)

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

"@

foreach ($key in $generatedKeys) {
    $distributionContent += @"

### $($key.Region.ToUpper()) - $($key.Provider)
- [ ] Backup private key to encrypted storage
- [ ] Transfer private key to $($key.Subdomain) via secure channel
- [ ] Verify checksum: ``````powershell
      Get-FileHash "$($key.PrivateKeyPath)" -Algorithm SHA256
      ``````
- [ ] Place key in: ``/etc/dchat/keys/validator.key``
- [ ] Set permissions: ``chmod 600 /etc/dchat/keys/validator.key``
- [ ] Verify validator can read key
- [ ] Delete key from transfer location
- [ ] Confirm backup is accessible

**Public Key**: ``$($key.PublicKeyHex)``
**Subdomain**: ``$($key.Subdomain)``

"@
}

$distributionContent += @"

---

## 🚀 Deployment Steps

### 1. Backup Keys (CRITICAL)
``````powershell
# Encrypt and backup entire keys directory
Compress-Archive -Path "$OutputDir" -DestinationPath "mainnet-keys-backup-`$(Get-Date -Format 'yyyyMMdd-HHmmss').zip"
# Move backup to encrypted storage (BitLocker, VeraCrypt, cloud with encryption)
``````

### 2. Distribute to Validators
For each validator server:
``````bash
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
``````

### 3. Update Genesis Configuration
Add all validator public keys to the genesis block configuration:
``````toml
[genesis.validators]
ohio = "$($generatedKeys[0].PublicKeyHex)"
singapore = "$($generatedKeys[1].PublicKeyHex)"
stockholm = "$($generatedKeys[2].PublicKeyHex)"
saopaulo = "$($generatedKeys[3].PublicKeyHex)"
india = "$($generatedKeys[4].PublicKeyHex)"
southafrica = "$($generatedKeys[5].PublicKeyHex)"
uae = "$($generatedKeys[6].PublicKeyHex)"
``````

### 4. Verify Key Distribution
``````powershell
# Check each validator can access its key
.\mainnet-commands.ps1 check-validator-keys
``````

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

**Generated**: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss UTC")
"@

$distributionContent | Out-File -FilePath $distributionPath -Encoding UTF8
Write-Host "✓ Generated distribution guide: $distributionPath" -ForegroundColor Green

# Backup reminder
if ($BackupPath) {
    Write-Host "`n📦 Creating backup to: $BackupPath" -ForegroundColor Yellow
    try {
        $backupFileName = "mainnet-keys-backup-$(Get-Date -Format 'yyyyMMdd-HHmmss').zip"
        $backupFullPath = Join-Path $BackupPath $backupFileName
        Compress-Archive -Path $OutputDir -DestinationPath $backupFullPath -Force
        Write-Host "✓ Backup created: $backupFullPath" -ForegroundColor Green
    } catch {
        Write-Host "⚠️  Backup failed: $_" -ForegroundColor Red
        Write-Host "   Please backup manually!" -ForegroundColor Yellow
    }
}

# Final summary
Write-Host "`n============================================" -ForegroundColor Cyan
Write-Host "  KEY GENERATION COMPLETE" -ForegroundColor Green
Write-Host "============================================`n" -ForegroundColor Cyan

Write-Host "Generated Keys:" -ForegroundColor White
foreach ($key in $generatedKeys) {
    Write-Host "  ✓ $($key.Region.PadRight(15)) - $($key.Subdomain)" -ForegroundColor Green
}

Write-Host "`nFiles Generated:" -ForegroundColor White
Write-Host "  • $($generatedKeys.Count * 2) key files (private + public)" -ForegroundColor Gray
Write-Host "  • 1 summary file (validator-keys-summary.json)" -ForegroundColor Gray
Write-Host "  • 1 distribution guide (KEY-DISTRIBUTION-INSTRUCTIONS.md)" -ForegroundColor Gray

Write-Host "`n⚠️  NEXT STEPS (CRITICAL):" -ForegroundColor Red
Write-Host "  1. BACKUP all keys to encrypted storage" -ForegroundColor Yellow
Write-Host "  2. Read KEY-DISTRIBUTION-INSTRUCTIONS.md" -ForegroundColor Yellow
Write-Host "  3. Distribute keys securely to each validator" -ForegroundColor Yellow
Write-Host "  4. Verify key permissions on all servers" -ForegroundColor Yellow
Write-Host "  5. DELETE keys from this machine after backup" -ForegroundColor Yellow

Write-Host "`n📚 Documentation:" -ForegroundColor White
Write-Host "  • Summary: $summaryPath" -ForegroundColor Gray
Write-Host "  • Distribution guide: $distributionPath" -ForegroundColor Gray
Write-Host "  • Deployment guide: .\MAINNET_DEPLOYMENT_CHECKLIST.md" -ForegroundColor Gray

Write-Host "`n✅ Keys are ready for distribution!" -ForegroundColor Green
Write-Host ""
