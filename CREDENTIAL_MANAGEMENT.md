# Credential Management Guide

## Overview

dchat implements secure credential management following AWS security best practices. **All placeholder credentials are rejected at startup in production mode.**

## Credential Types

### 1. AWS KMS (Validator Keys)

**Purpose**: Hardware Security Module (HSM) backed validator key storage

**Configuration**:
```bash
# Use IAM roles (preferred) - no credentials needed
# Credentials automatically discovered via AWS SDK credential chain:
# 1. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)
# 2. Web Identity Token (EKS/ECS)
# 3. EC2 instance profile (recommended for production)
# 4. ~/.aws/credentials file

# Specify KMS key ID
export DCHAT_KMS_KEY_ID="arn:aws:kms:us-east-1:123456789012:key/12345678-1234-1234-1234-123456789012"

# Specify AWS region
export AWS_REGION="us-east-1"
```

**Production Deployment**:
- Use EC2 instance profiles or EKS IAM roles for service accounts (IRSA)
- Never hardcode AWS credentials in code or configuration files
- Grant minimal IAM permissions: `kms:Sign`, `kms:GetPublicKey`, `kms:DescribeKey`

**Module**: `dchat-crypto::kms`

**Files**:
- `crates/dchat-crypto/src/kms.rs` - AWS KMS integration
- `src/main.rs:3208` - Validator key loading (TODO: update to use KMS module)

### 2. S3 Backup Credentials

**Purpose**: Encrypted backups to S3 (CockroachDB, TiKV)

**Configuration**:
```bash
# Specify backup bucket
export DCHAT_BACKUP_S3_BUCKET="dchat-backups-production"

# Use IAM roles (preferred) - no credentials needed
# AWS SDK automatically discovers credentials
```

**Production Deployment**:
- Use IAM instance profiles or service roles
- Grant S3 permissions: `s3:PutObject`, `s3:GetObject`, `s3:ListBucket`
- Enable S3 bucket versioning and lifecycle policies
- **DO NOT** embed credentials in S3 URLs (e.g., `s3://bucket?AWS_ACCESS_KEY_ID=xxx`)

**Module**: `dchat-deployment::backup_system`

**Files**:
- `crates/dchat-deployment/src/backup_system.rs` - Backup configuration
- Line 370: `BackendBackupConfig::new_production()` - Now uses environment variables
- Line 430: `validate_for_production()` - Rejects placeholder credentials

### 3. Slack Webhook

**Purpose**: Critical alert notifications (consensus failures, network partitions)

**Configuration**:
```bash
# Set real Slack webhook URL
export DCHAT_SLACK_WEBHOOK_URL="https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXX"
```

**Production Validation**:
- Startup fails if URL contains placeholders (`/XXX/`, `/YYY/`, `/ZZZ`)
- Warns if not configured (alerts go to logs only)

**Module**: `dchat-deployment::health_monitor`

**Files**:
- `crates/dchat-deployment/src/health_monitor.rs:489` - Alert channel configuration
- `crates/dchat-deployment/src/health_monitor.rs:309` - Validation logic

### 4. PagerDuty Integration Key

**Purpose**: Critical alert escalation to on-call engineers

**Configuration**:
```bash
# Set real PagerDuty integration key
export DCHAT_PAGERDUTY_KEY="R01ABCDEF1234567890ABCDEF123456"
```

**Production Validation**:
- Startup fails if key equals `"pagerduty_integration_key"` or contains `"placeholder"`
- Warns if not configured

**Module**: `dchat-deployment::health_monitor`

**Files**:
- `crates/dchat-deployment/src/health_monitor.rs:503` - PagerDuty configuration
- `crates/dchat-deployment/src/health_monitor.rs:319` - Validation logic

## Production Startup Validation

### Validation Flow

1. **Load configuration** (`src/main.rs:1819`)
2. **Run mainnet validation** (`src/main.rs:3182` for validators, similar for relays)
3. **Check credentials**:
   - Reject Slack webhook placeholders (`XXX/YYY/ZZZ`)
   - Reject PagerDuty key placeholder (`pagerduty_integration_key`)
   - Validate backup config (S3 URLs must not contain embedded credentials)
4. **Startup proceeds** only if validation passes

### Validation Function

**Location**: `src/main.rs:156`

```rust
async fn validate_mainnet_environment(config: &Config, node_type: NodeType) -> Result<()>
```

**Behavior**:
- Returns `Err(Error::Config(...))` if placeholders detected
- Logs warnings for missing (but not invalid) credentials
- Always runs in production mode (`is_mainnet = true`)

## AWS Secrets Manager Integration

### Current Status

**NOT YET IMPLEMENTED** - Planned for Sprint 4 (Medium priority)

### Planned Implementation

```rust
// Future: Fetch secrets from AWS Secrets Manager
use aws_sdk_secretsmanager as secretsmanager;

async fn load_secret(secret_name: &str) -> Result<String> {
    let config = aws_config::load_from_env().await;
    let client = secretsmanager::Client::new(&config);
    
    let resp = client
        .get_secret_value()
        .secret_id(secret_name)
        .send()
        .await?;
    
    Ok(resp.secret_string().unwrap().to_string())
}

// Usage:
let slack_webhook = load_secret("dchat/prod/slack-webhook").await?;
let pagerduty_key = load_secret("dchat/prod/pagerduty-key").await?;
```

**Benefits**:
- Centralized secret rotation
- Audit logging of secret access
- Fine-grained IAM permissions
- Automatic secret versioning

**Cost**: ~$0.40/secret/month + $0.05/10,000 API calls

## Security Best Practices

### 1. Principle of Least Privilege

Grant only required IAM permissions:

**Validator Node IAM Role**:
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "kms:Sign",
        "kms:GetPublicKey",
        "kms:DescribeKey"
      ],
      "Resource": "arn:aws:kms:*:*:key/*",
      "Condition": {
        "StringEquals": {
          "kms:KeyUsage": "SIGN_VERIFY"
        }
      }
    },
    {
      "Effect": "Allow",
      "Action": [
        "s3:PutObject",
        "s3:GetObject"
      ],
      "Resource": "arn:aws:s3:::dchat-backups-production/*"
    },
    {
      "Effect": "Allow",
      "Action": "s3:ListBucket",
      "Resource": "arn:aws:s3:::dchat-backups-production"
    }
  ]
}
```

### 2. Credential Rotation

**AWS KMS Keys**:
- Automatically rotated by AWS (yearly)
- Enable automatic key rotation via AWS KMS console

**Slack/PagerDuty**:
- Rotate webhooks/keys every 90 days
- Use AWS Secrets Manager for automated rotation

### 3. Audit Logging

**AWS CloudTrail**:
- All KMS operations logged (Sign, GetPublicKey)
- S3 access logged (PutObject, GetObject)
- Review logs monthly for unauthorized access

**Application Logs**:
- Credential validation results logged at startup
- Failed alert deliveries logged (rate-limited)

### 4. Environment Separation

**Testnet** (`.env.testnet`):
```bash
DCHAT_BACKUP_S3_BUCKET="dchat-backups-testnet"
# Slack/PagerDuty optional for testnet
```

**Production** (`.env.production`):
```bash
DCHAT_BACKUP_S3_BUCKET="dchat-backups-production"
DCHAT_SLACK_WEBHOOK_URL="https://hooks.slack.com/services/..."
DCHAT_PAGERDUTY_KEY="R01..."
AWS_REGION="us-east-1"
```

## Troubleshooting

### Error: "contains placeholder credentials"

**Cause**: Environment variable set to default/example value

**Fix**:
```bash
# Check current value
echo $DCHAT_SLACK_WEBHOOK_URL

# If placeholder detected, set real value
export DCHAT_SLACK_WEBHOOK_URL="https://hooks.slack.com/services/REAL/VALUES/HERE"

# Or unset to skip validation
unset DCHAT_SLACK_WEBHOOK_URL
```

### Error: "AWS KMS not available"

**Cause**: IAM permissions missing or region misconfigured

**Fix**:
```bash
# Verify IAM role has KMS permissions
aws sts get-caller-identity

# Check KMS key exists and is accessible
aws kms describe-key --key-id $DCHAT_KMS_KEY_ID

# Set correct region
export AWS_REGION="us-east-1"
```

### Error: "S3 backup failed: Access Denied"

**Cause**: Instance profile lacks S3 permissions

**Fix**:
1. Attach IAM policy with S3 permissions to instance role
2. Verify bucket exists: `aws s3 ls s3://$DCHAT_BACKUP_S3_BUCKET`
3. Check bucket policy allows access from instance role

## Roadmap

### Sprint 1 (Completed ✅)
- ✅ AWS KMS module implementation
- ✅ S3 backup credential validation
- ✅ Startup validation for placeholders
- ✅ Environment variable configuration

### Sprint 2 (Planned)
- MPC threshold signing integration
- On-chain staking submission
- Enhanced credential rotation automation

### Sprint 4 (Planned)
- AWS Secrets Manager integration
- Automated credential rotation
- Secret versioning support
- Cross-account backup encryption

## References

- **AWS KMS Best Practices**: https://docs.aws.amazon.com/kms/latest/developerguide/best-practices.html
- **AWS Security Token Service**: https://docs.aws.amazon.com/STS/latest/APIReference/welcome.html
- **AWS Secrets Manager**: https://docs.aws.amazon.com/secretsmanager/latest/userguide/intro.html
- **IAM Roles for EC2**: https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/iam-roles-for-amazon-ec2.html
- **EKS IAM Roles for Service Accounts**: https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html

## Related Files

- `ARCHITECTURE-2.0.md` - Implementation status analysis
- `crates/dchat-crypto/src/kms.rs` - AWS KMS integration
- `crates/dchat-deployment/src/backup_system.rs` - Backup configuration
- `crates/dchat-deployment/src/health_monitor.rs` - Alert channel validation
- `src/main.rs:156` - Production environment validation
- `src/main.rs:3208` - Validator key loading (TODO: migrate to KMS)

## Support

For credential management issues:
1. Check logs: `journalctl -u dchat-validator -f`
2. Verify IAM permissions: `aws iam get-role --role-name dchat-validator`
3. Review CloudTrail logs for access denials
4. Contact security team for credential rotation

---

**Last Updated**: 2025-01-XX (Sprint 1 completion)
**Security Review**: Required before mainnet launch (Sprint 6)
