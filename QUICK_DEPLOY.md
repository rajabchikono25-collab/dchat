# Quick Deployment Guide

## Prerequisites

- AWS CLI configured
- Rust toolchain
- PowerShell 7+ (Windows) or Bash (Linux)

## 1. Deploy Infrastructure (5 minutes)

```powershell
# Development environment
.\scripts\deploy-infrastructure.ps1 -Environment dev

# Production environment
.\scripts\deploy-infrastructure.ps1 -Environment prod
```

This creates:
- 3 bootstrap nodes (us-east-1, us-west-2, eu-west-1)
- 2 TURN servers (us-east-1, eu-west-1)
- Security groups and networking
- config.dev.toml or config.prod.toml

## 2. Run Integration Tests (2 minutes)

```powershell
# Against deployed infrastructure
.\scripts\run-integration-tests.ps1 -ConfigFile config.dev.toml
```

## 3. Monitor Infrastructure

```bash
# SSH into bootstrap node
ssh ubuntu@<bootstrap-ip>
sudo journalctl -u dchat-bootstrap -f

# SSH into TURN server
ssh ubuntu@<turn-ip>
sudo journalctl -u coturn -f
```

## Cost Estimate

**Dev environment**: ~$75/month
- 3x t3.small (bootstrap): ~$45/month
- 2x t3.small (TURN): ~$30/month

**Prod environment**: ~$150/month (with redundancy)

## Teardown

```powershell
# Delete all resources
aws ec2 terminate-instances --instance-ids $(aws ec2 describe-instances --filters "Name=tag:Environment,Values=dev" --query 'Reservations[].Instances[].InstanceId' --output text)
```

## Manual Testing

```bash
# Test STUN
cargo test --test integration_stun -- --nocapture

# Test with deployed TURN (requires credentials)
export TURN_SERVER=<turn-ip>:3478
export TURN_USERNAME=dchat
export TURN_SECRET=<secret>
cargo test --test integration_turn -- --ignored --nocapture
```
