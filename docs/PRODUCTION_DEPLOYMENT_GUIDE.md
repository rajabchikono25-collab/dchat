# Production Deployment Guide

## Critical Pre-Launch Checklist

### 1. Staking Verification Configuration

**REQUIRED:** Wire `ChainStakingVerifier` in all production deployments.

```rust
use dchat_messaging::{ChainStakingVerifier, ChannelAccessManager};
use std::sync::Arc;

// ✅ CORRECT: Production configuration
let staking_verifier = Arc::new(ChainStakingVerifier::from_env()?);
let mut channel_manager = ChannelAccessManager::with_staking_verifier(staking_verifier);

// ❌ WRONG: Never use mock in production
// #[cfg(feature = "test-mocks")]
// let mock = Arc::new(MockStakingVerifier::new());  // DO NOT DO THIS
```

**Environment Variables:**
```bash
# Required: Currency chain RPC endpoint
export CURRENCY_CHAIN_RPC="https://currency-chain-rpc.dchat.network:8545"

# Verify connection before launch
curl $CURRENCY_CHAIN_RPC -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

### 2. Keystore Passphrase Management

**REQUIRED:** Secure the relay keystore passphrase.

#### Option A: Kubernetes Secrets (Recommended)
```yaml
apiVersion: v1
kind: Secret
metadata:
  name: dchat-relay-secrets
type: Opaque
stringData:
  keystore-passphrase: <generate-strong-passphrase>
---
apiVersion: v1
kind: Pod
metadata:
  name: dchat-relay
spec:
  containers:
  - name: relay
    image: dchat/relay:latest
    env:
    - name: DCHAT_RELAY_KEYSTORE_PASSPHRASE
      valueFrom:
        secretKeyRef:
          name: dchat-relay-secrets
          key: keystore-passphrase
```

#### Option B: HashiCorp Vault
```bash
# Store passphrase
vault kv put secret/dchat/relay keystore-passphrase="<strong-passphrase>"

# Retrieve in startup script
export DCHAT_RELAY_KEYSTORE_PASSPHRASE=$(vault kv get -field=keystore-passphrase secret/dchat/relay)
```

#### Option C: AWS Secrets Manager
```bash
# Store secret
aws secretsmanager create-secret \
  --name dchat/relay/keystore-passphrase \
  --secret-string "<strong-passphrase>"

# Retrieve in startup script
export DCHAT_RELAY_KEYSTORE_PASSPHRASE=$(aws secretsmanager get-secret-value \
  --secret-id dchat/relay/keystore-passphrase --query SecretString --output text)
```

**Passphrase Requirements:**
- Minimum 32 characters
- Mix of upper/lower case, numbers, symbols
- Generated via cryptographically secure RNG
- Rotated every 90 days
- Never committed to version control
- Never logged or displayed

**Generate Strong Passphrase:**
```bash
# Linux/macOS
openssl rand -base64 32

# Or using Python
python3 -c "import secrets; print(secrets.token_urlsafe(32))"
```

### 3. Rate Limiting Configuration

Configure rate limits based on your deployment scale:

```toml
# config-production.toml
[rate_limit]
global_limit = 10000              # Adjust for relay capacity
per_user_limit = 100              # Balance UX vs abuse prevention
burst_capacity = 200              # Allow 2x burst
bandwidth_bytes_per_second = 1048576  # 1 MB/sec per user
max_concurrent_connections = 10   # Per user
window_seconds = 60
max_queue_size = 100000           # ~100MB memory

# For high-traffic relays (10k+ concurrent users)
# global_limit = 50000
# max_queue_size = 500000

# For resource-constrained nodes
# global_limit = 5000
# max_queue_size = 10000
```

### 4. Metrics and Monitoring

**Prometheus Endpoint Setup:**
```rust
use axum::{Router, routing::get};

async fn metrics_handler(
    State(rate_limiter): State<Arc<RateLimiter>>,
) -> String {
    let metrics = rate_limiter.get_metrics().await;
    metrics.to_prometheus_text()
}

let app = Router::new()
    .route("/metrics", get(metrics_handler))
    .with_state(rate_limiter);
```

**Critical Alerts:**
```yaml
# prometheus-alerts.yml
groups:
- name: dchat_relay
  interval: 30s
  rules:
  - alert: HighRateLimitDenials
    expr: rate(rate_limit_denied_total[5m]) > 100
    for: 5m
    labels:
      severity: warning
    annotations:
      summary: "High rate of denied requests"
      
  - alert: QueueDepthHigh
    expr: rate_limit_queue_depth > 80000
    for: 2m
    labels:
      severity: critical
    annotations:
      summary: "Message queue nearing capacity"
      
  - alert: StakingVerifierErrors
    expr: rate(staking_verification_errors_total[5m]) > 10
    labels:
      severity: critical
    annotations:
      summary: "Currency chain connection issues"
```

### 5. Security Hardening

#### A. Disable Debug Assertions
```toml
[profile.release]
debug-assertions = false
overflow-checks = true
lto = "fat"
codegen-units = 1
panic = "abort"
```

#### B. Verify Build Configuration
```bash
# Check that mocks are not compiled
cargo build --release
objdump -t target/release/dchat-relay | grep -i mock

# Should return nothing. If MockStakingVerifier appears, BUILD FAILED.
```

#### C. Runtime Checks
```bash
# Test production binary before deploy
./target/release/dchat-relay --verify-config

# Should output:
# ✅ StakingVerifier: ChainStakingVerifier
# ✅ No test mocks compiled
# ✅ All security features enabled
```

### 6. Pre-Flight Checks Script

```bash
#!/bin/bash
# pre-flight-check.sh

set -e

echo "🔍 Running pre-flight checks..."

# 1. Check environment variables
if [ -z "$CURRENCY_CHAIN_RPC" ]; then
  echo "❌ CURRENCY_CHAIN_RPC not set"
  exit 1
fi

if [ -z "$DCHAT_RELAY_KEYSTORE_PASSPHRASE" ]; then
  echo "❌ DCHAT_RELAY_KEYSTORE_PASSPHRASE not set"
  exit 1
fi

# 2. Verify currency chain connectivity
echo "Testing currency chain RPC..."
if ! curl -sf "$CURRENCY_CHAIN_RPC" -X POST -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' > /dev/null; then
  echo "❌ Cannot connect to currency chain"
  exit 1
fi

# 3. Check binary build
if objdump -t ./dchat-relay | grep -i "MockStakingVerifier"; then
  echo "❌ CRITICAL: Mock code found in binary!"
  exit 1
fi

# 4. Verify metrics endpoint
if [ -f "./dchat-relay" ]; then
  ./dchat-relay --check-metrics-config || exit 1
fi

echo "✅ All pre-flight checks passed"
echo "Ready for production deployment"
```

### 7. Rollback Plan

Maintain rollback capability:
```bash
# Tag production releases
git tag -a v1.0.0-prod -m "Production release 1.0.0"
git push origin v1.0.0-prod

# Keep previous version binary
mv dchat-relay dchat-relay-v1.0.0.backup

# Rollback if needed
systemctl stop dchat-relay
cp dchat-relay-v0.9.9.backup dchat-relay
systemctl start dchat-relay
```

### 8. Post-Deployment Verification

```bash
# Check logs for warnings
journalctl -u dchat-relay -n 100 | grep -i "mock\|test\|warning"

# Verify staking verification is working
curl http://localhost:9090/metrics | grep rate_limit

# Monitor for first 24 hours
watch -n 10 'curl -s http://localhost:9090/metrics | grep denied'
```

## Emergency Contacts

- **Mainnet Issues:** ops@dchat.network
- **Security Issues:** security@dchat.network
- **On-Call:** +1-XXX-XXX-XXXX

## Deployment Schedule

1. **T-24h:** Deploy to staging, run load tests
2. **T-12h:** Security audit, verify all secrets configured
3. **T-6h:** Deploy to production, monitor closely
4. **T+0h:** Mainnet launch
5. **T+24h:** Post-launch review

---

**Remember:** Never deploy with test mocks enabled. Always verify ChainStakingVerifier is wired.
