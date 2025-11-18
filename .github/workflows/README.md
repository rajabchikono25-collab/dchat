# GitHub Workflows Documentation

This directory contains all GitHub Actions workflows for the dchat project, optimized for mainnet blockchain deployment and operations.

## 📋 Workflow Overview

### Core CI/CD Workflows

#### 1. **ci.yml** - Continuous Integration
**Triggers:** Push to main/develop, PRs, daily schedule, manual
- ✅ Multi-OS testing (Linux, Windows, macOS)
- ✅ Rust versions (stable, beta)
- ✅ Code formatting and linting (rustfmt, clippy)
- ✅ Code coverage with codecov
- ✅ Performance benchmarking
- ✅ **Blockchain integration tests**
- ✅ **Validator node tests**
- ✅ **P2P network tests**
- ✅ Release build artifact generation

**When to use:** Automatic on every push/PR

---

#### 2. **security.yml** - Security Auditing
**Triggers:** Push, PRs, weekly schedule, manual
- 🔒 Cargo audit for vulnerabilities
- 🔒 Cargo deny for license/advisory checks
- 🔒 Clippy security lints
- 🔒 Dependency review on PRs
- 🔒 Cryptographic test validation

**When to use:** Automatic security scanning

---

#### 3. **check-production-safety.yml** - Production Safety
**Triggers:** Push to main/develop, PRs to main
- 🛡️ Verify no mock code in release builds
- 🛡️ Check feature gates (test-mocks not in default)
- 🛡️ Scan for unsafe production markers
- 🛡️ Verify secrets not logged
- 🛡️ Dependency security audit

**When to use:** Automatic before merging to main

---

### Deployment Workflows

#### 4. **deploy-staging.yml** - Staging Deployment
**Triggers:** Push to main, manual
- 🚀 Build and push Docker images
- 🚀 Deploy to Kubernetes staging cluster
- 🚀 Run smoke tests
- 🚀 Load testing with k6
- 🚀 Trivy security scanning

**When to use:** Automatic on main branch, or manual for testing

---

#### 5. **deploy-production.yml** - Production Deployment
**Triggers:** Manual only
- 🎯 Blue/green deployment strategy
- 🎯 Rolling/canary options
- 🎯 Pre-deployment security checks
- 🎯 Database backups
- 🎯 Smoke tests and monitoring
- 🎯 Automatic rollback on failure
- 🎯 Multi-region support

**Inputs:**
- `version`: Release tag (e.g., v1.0.0)
- `deployment_strategy`: blue-green/rolling/canary
- `confirm`: Type "DEPLOY-TO-PRODUCTION" (all caps)
- `skip_monitoring`: Skip monitoring phase (NOT recommended)

**When to use:** Manual production deployments only

---

### Mainnet-Specific Workflows

#### 6. **deploy-mainnet.yml** - Mainnet Deployment ⭐
**Triggers:** Manual only (CRITICAL)
- 🔥 **Full mainnet deployment with validators and relays**
- 🔥 Requires "DEPLOY-TO-MAINNET" confirmation
- 🔥 Pre-deployment security audit
- 🔥 Build multi-arch binaries (amd64, arm64)
- 🔥 Deploy validators (consensus nodes)
- 🔥 Deploy relay nodes (P2P network)
- 🔥 Comprehensive smoke tests
- 🔥 30-minute monitoring phase
- 🔥 Automatic rollback on failure
- 🔥 Slack/Discord notifications

**Inputs:**
- `version`: Release tag (must exist, e.g., v1.0.0)
- `deployment_type`: full-deployment/validators-only/relays-only/rollback
- `confirm_mainnet`: Type "DEPLOY-TO-MAINNET" (all caps)
- `skip_smoke_tests`: Skip tests (NOT recommended)

**When to use:** Mainnet network deployments only (HANDLE WITH CARE)

**Security Features:**
- Version validation (must be tagged release)
- Mock code detection
- Hardcoded secret scanning
- Multi-platform Docker image scanning
- Health checks before traffic switching

---

#### 7. **release.yml** - Release Management ⭐
**Triggers:** Git tags (v*.*.*), manual
- 📦 Validates semver tags
- 📦 Security audit for releases
- 📦 Multi-platform binary builds (Linux, Windows, macOS)
- 📦 Docker image builds (amd64, arm64)
- 📦 GitHub release creation with notes
- 📦 Publishes to crates.io
- 📦 Generates checksums
- 📦 Slack/Discord notifications

**Tag Formats:**
- `v1.0.0` - Stable release
- `v1.0.0-rc.1` - Release candidate
- `v1.0.0-beta.1` - Beta release

**When to use:** Automatic on version tags

**Artifacts Generated:**
- Binary archives (.tar.gz, .zip)
- SHA256 checksums
- Docker images (ghcr.io)
- GitHub release page

---

#### 8. **network-upgrade.yml** - Network Upgrades ⭐⭐
**Triggers:** Manual only (CRITICAL)
- 🔄 **Coordinated network upgrades**
- 🔄 Hard fork / soft fork support
- 🔄 Validator notification (24h coordination)
- 🔄 Rolling upgrade execution
- 🔄 Post-upgrade validation
- 🔄 2-hour monitoring period
- 🔄 Automatic rollback on failure

**Inputs:**
- `upgrade_version`: Target version (e.g., v2.0.0)
- `upgrade_block`: Block height for upgrade (optional)
- `network`: mainnet/testnet/staging
- `upgrade_type`: hard-fork/soft-fork/runtime-upgrade/config-update
- `confirm_upgrade`: Type "EXECUTE-NETWORK-UPGRADE"
- `skip_coordination`: Skip validator coordination (NOT recommended)

**When to use:** Blockchain network upgrades

**Upgrade Process:**
1. Validation and security audit
2. Notify all validators (email + Slack)
3. 24-hour coordination window (mainnet)
4. Build upgrade artifacts
5. Rolling upgrade execution
6. Post-upgrade validation
7. 2-hour monitoring

---

#### 9. **validator-onboarding.yml** - Validator Onboarding
**Triggers:** Manual only
- 👥 Onboard new validators to the network
- 👥 Infrastructure provisioning
- 👥 Key generation and management
- 👥 On-chain registration
- 👥 Monitoring setup
- 👥 Health checks and verification

**Inputs:**
- `validator_name`: Validator identifier
- `validator_address`: Public address (64 hex chars)
- `stake_amount`: Initial stake (min: 100,000 mainnet, 1,000 testnet)
- `network`: mainnet/testnet
- `region`: Deployment region
- `validator_type`: full-validator/light-validator
- `confirm_onboarding`: Type "ONBOARD-VALIDATOR"

**When to use:** Adding new validators to the network

**Features:**
- Automated infrastructure setup
- Secure key generation
- Grafana dashboard creation
- Email welcome notifications
- Tracking issues

---

#### 10. **mainnet-monitoring.yml** - Continuous Monitoring ⭐
**Triggers:** Every 15 minutes, manual
- 📊 **Automated health checks**
- 📊 Consensus monitoring
- 📊 Network connectivity checks
- 📊 Error rate tracking
- 📊 Performance metrics
- 📊 Security monitoring (double-signing detection)
- 📊 Automatic alerting

**Monitoring Areas:**
- Validator RPC health
- Relay node health
- Block production
- Validator participation
- Missed blocks
- P2P connectivity
- Error rates
- Transaction throughput
- Security events (slashing, double-signing)

**When to use:** Automatic continuous monitoring

**Alerts:**
- Critical alerts → Slack critical channel
- Warnings → Slack general channel
- Daily summary reports

---

## 🔐 Required Secrets

### General
- `GITHUB_TOKEN` - Provided by GitHub Actions

### Container Registry
- `DOCKERHUB_USERNAME` - Docker Hub username (optional)
- `DOCKERHUB_TOKEN` - Docker Hub token (optional)

### Cloud Infrastructure
- `AWS_ACCESS_KEY_ID` - AWS credentials
- `AWS_SECRET_ACCESS_KEY` - AWS secret key
- `MAINNET_KUBECONFIG` - Mainnet Kubernetes config (base64)
- `TESTNET_KUBECONFIG` - Testnet Kubernetes config (base64)
- `STAGING_KUBECONFIG` - Staging Kubernetes config (base64)

### Blockchain
- `MAINNET_GENESIS_HASH` - Mainnet genesis block hash
- `MAINNET_VALIDATOR_KEY` - Validator signing key
- `VALIDATOR_EMAIL_LIST` - Email list for validator notifications
- `VALIDATOR_CONTACT_EMAIL` - Validator contact email

### Notifications
- `SLACK_WEBHOOK` - General Slack webhook
- `SLACK_WEBHOOK_CRITICAL` - Critical alerts Slack webhook
- `SLACK_WEBHOOK_VALIDATORS` - Validator-specific Slack webhook
- `DISCORD_WEBHOOK` - Discord webhook

### Email
- `MAIL_SERVER` - SMTP server
- `MAIL_PORT` - SMTP port
- `MAIL_USERNAME` - SMTP username
- `MAIL_PASSWORD` - SMTP password

### Publishing
- `CARGO_REGISTRY_TOKEN` - crates.io API token

---

## 🚀 Deployment Checklist

### Pre-Mainnet Launch
- [ ] All CI tests passing
- [ ] Security audit completed
- [ ] Production safety checks passing
- [ ] Staging deployment tested
- [ ] Load testing completed
- [ ] Documentation updated
- [ ] Changelog updated
- [ ] Release tag created
- [ ] Validators notified
- [ ] Team approvals obtained

### Mainnet Launch Steps
1. Create release tag (e.g., `v1.0.0`)
2. Run `release.yml` workflow
3. Verify release artifacts
4. Run `deploy-mainnet.yml` with "full-deployment"
5. Monitor for 30+ minutes
6. Announce to community

### Network Upgrade Steps
1. Create upgrade version tag
2. Update CHANGELOG.md
3. Run `network-upgrade.yml` workflow
4. Validators coordinate upgrade
5. Execute at specified block height
6. Monitor for 2 hours
7. Announce completion

---

## 📈 Monitoring & Alerts

### Health Check Frequency
- Continuous monitoring: Every 15 minutes
- Manual checks: On-demand via workflow_dispatch

### Alert Thresholds
- Error rate: > 1%
- Missed blocks: > 10
- Min peers: < 5
- Consensus time: > 5 seconds
- Memory usage: Tracked
- CPU usage: Tracked

### Alert Channels
- **Critical:** Slack critical channel (immediate)
- **Warning:** Slack general channel
- **Info:** Daily summary reports

---

## 🛠️ Workflow Maintenance

### Adding New Workflows
1. Create workflow file in `.github/workflows/`
2. Add documentation to this README
3. Configure required secrets
4. Test in staging environment
5. Update team documentation

### Modifying Existing Workflows
1. Create feature branch
2. Modify workflow file
3. Test changes in staging
4. Create PR with description
5. Get team review
6. Merge to main

### Best Practices
- ✅ Always test workflows in staging first
- ✅ Use proper confirmation strings for critical operations
- ✅ Include rollback mechanisms
- ✅ Add comprehensive monitoring
- ✅ Document all inputs and outputs
- ✅ Use semantic versioning
- ✅ Keep secrets secure
- ✅ Review audit logs regularly

---

## 📚 Additional Resources

- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [dchat Documentation](https://docs.dchat.network)
- [Validator Guide](https://docs.dchat.network/validators)
- [Deployment Guide](../../../PRODUCTION_DEPLOYMENT_GUIDE.md)
- [Security Model](../../../SECURITY_MODEL.md)

---

## 🤝 Support

For workflow issues or questions:
- **Slack:** #devops
- **Email:** devops@dchat.network
- **Issues:** Create GitHub issue with `workflow` label

---

**Last Updated:** 2025-11-17
**Maintainer:** DevOps Team
