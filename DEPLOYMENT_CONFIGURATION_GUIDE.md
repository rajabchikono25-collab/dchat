# dchat Production Deployment Configuration Guide

**Last Updated**: November 4, 2025  
**Status**: Ready for Deployment with Configuration

## Executive Summary

✅ **All critical mock code has been replaced with production implementations**  
⚠️ **Configuration placeholders require environment-specific values before deployment**  
📋 **This guide documents all configuration points that need real values**

---

## 🔴 CRITICAL - Must Configure Before Production

### 1. Bootstrap Relay Nodes

**File**: `crates/dchat-network/src/discovery/bootstrap.rs`  
**Current**: DNS names `bootstrap-1/2/3.dchat.network` (must resolve to real IPs)  
**Required Action**:

```bash
# Option 1: Set environment variable with real relay addresses
export DCHAT_BOOTSTRAP_NODES="/ip4/203.0.113.10/tcp/9000,/ip4/203.0.113.11/tcp/9000,/ip4/203.0.113.12/tcp/9000"

# Option 2: Configure DNS A records
bootstrap-1.dchat.network → <Your Relay 1 IP>
bootstrap-2.dchat.network → <Your Relay 2 IP>
bootstrap-3.dchat.network → <Your Relay 3 IP>
```

**Impact**: Without configured bootstrap nodes, users cannot discover the network  
**Priority**: 🔴 CRITICAL

---

### 2. Bot API Base URL

**File**: `crates/dchat-bots/src/api/http_client.rs`  
**Current**: `https://api.dchat.network`  
**Required Action**:

```rust
// Option 1: Set via environment variable
std::env::set_var("DCHAT_BOT_API_URL", "https://your-api.dchat.network");

// Option 2: Override in bot client code
let client = BotHttpClient::new(token)
    .with_base_url("https://your-api.dchat.network".to_string());
```

**Impact**: Bots cannot communicate with API  
**Priority**: 🔴 HIGH (if using bot features)

---

### 3. Chain RPC Endpoints

**File**: `src/main.rs` (validator command)  
**Current**: Accepts `--chain-rpc` parameter  
**Required Action**:

```bash
# Set default in config.toml
[chain]
chat_chain_rpc = "http://chat-validator-1.dchat.network:26657"
currency_chain_rpc = "http://currency-validator-1.dchat.network:26657"

# Or pass via CLI
dchat validator --chain-rpc http://your-chain.dchat.network:26657
```

**Impact**: Validators cannot participate in consensus  
**Priority**: 🔴 CRITICAL (for validators)

---

## 🟡 MEDIUM - Platform-Specific Features

### 4. Android Biometric Authentication

**File**: `crates/dchat-identity/src/biometric.rs`  
**Current**: Returns error ("JNI bindings required")  
**Required Action**:

**Option 1: JNI Implementation**
```rust
// Create Android module with JNI bindings
// See biometric.rs documentation for full guide
```

**Option 2: Flutter/React Native Plugin**
```dart
// Use existing biometric plugin
import 'package:local_auth/local_auth.dart';
```

**Option 3: Disable Feature**
```rust
// Document that Android biometric is unavailable
// Fall back to password/PIN authentication
```

**Impact**: Keyless UX unavailable on Android  
**Priority**: 🟡 MEDIUM (iOS works, Android needs implementation)

---

### 5. External API Integrations (Bot Examples)

**File**: `crates/dchat-bots/examples/complete_integration.rs`  
**Current**: Uses `mock_spotify_token`  
**Required Action**:

```rust
// Replace with OAuth flow
let auth_code = get_spotify_auth_code().await?;
let access_token = exchange_auth_code_for_token(auth_code).await?;
music_client.set_spotify_token(access_token);
```

**Documentation**: See `crates/dchat-bots/examples/README.md`  
**Impact**: Music bot features non-functional  
**Priority**: 🟡 LOW (example code only)

---

## 🟢 OPTIONAL - Enhancement Features

### 6. Post-Quantum Cryptography Keys

**File**: `crates/dchat-blockchain/src/proof_of_transit.rs`  
**Current**: Dilithium keys are zeros (placeholder)  
**Required Action**:

```rust
// Add pqcrypto-dilithium dependency
use pqcrypto_dilithium::dilithium3;

let (pk, sk) = dilithium3::keypair();
dilithium_keys: vec![pk.as_bytes().to_vec(), pk.as_bytes().to_vec()],
```

**Impact**: No post-quantum protection (classical crypto still secure)  
**Priority**: 🟢 LOW (roadmap: 2030 for full PQ migration)

---

### 7. Geographic Region Detection

**File**: `crates/dchat-validator/src/multi_region.rs`  
**Current**: Defaults to `GeographicRegion::NorthAmerica`  
**Required Action**:

```rust
// Use IP geolocation service
let region = detect_region_from_ip().await
    .unwrap_or(GeographicRegion::NorthAmerica);
```

**Impact**: Sub-optimal multi-region routing  
**Priority**: 🟢 LOW (single region works fine initially)

---

## 📋 Configuration Checklist

Use this checklist before production deployment:

### Infrastructure
- [ ] Deploy 3+ relay nodes with static public IPs
- [ ] Configure DNS or set `DCHAT_BOOTSTRAP_NODES` environment variable
- [ ] Set up load balancer for Bot API (if using bots)
- [ ] Configure TLS certificates for all public endpoints
- [ ] Deploy validator nodes with `--chain-rpc` configured

### Secrets Management
- [ ] Generate production validator keys (`dchat keygen`)
- [ ] Store private keys in secure HSM/KMS (or encrypted files)
- [ ] Set `DCHAT_BOT_API_URL` for bot deployments
- [ ] Configure OAuth credentials for external APIs (if using)
- [ ] Set up webhook secrets for bot callbacks

### Monitoring
- [ ] Configure Prometheus metrics endpoint (`--metrics-addr`)
- [ ] Set up health check monitoring (`--health-addr`)
- [ ] Deploy observability stack (optional: `dchat testnet --observability`)
- [ ] Configure alerting for relay/validator downtime

### Testing
- [ ] Test bootstrap node discovery from external network
- [ ] Verify relay node connectivity with `dchat user --bootstrap <addrs>`
- [ ] Test bot API with real bot token
- [ ] Run `dchat health --url http://your-node:8080/health`
- [ ] Load test with multiple concurrent users

---

## 🚀 Quick Start Deployment

### 1. Deploy Bootstrap Relays

```bash
# On each relay server (3 minimum)
export DCHAT_RELAY_ID=1  # 2, 3, etc.

dchat relay \
  --listen 0.0.0.0:9000 \
  --stake 1000 \
  --metrics-addr 0.0.0.0:9090 \
  --health-addr 0.0.0.0:8080
```

### 2. Configure DNS or Environment

```bash
# Option A: DNS
bootstrap-1.dchat.network → 203.0.113.10
bootstrap-2.dchat.network → 203.0.113.11
bootstrap-3.dchat.network → 203.0.113.12

# Option B: Environment variable for users
export DCHAT_BOOTSTRAP_NODES="/ip4/203.0.113.10/tcp/9000,/ip4/203.0.113.11/tcp/9000"
```

### 3. Start User Node

```bash
dchat user \
  --bootstrap /ip4/203.0.113.10/tcp/9000 \
  --bootstrap /ip4/203.0.113.11/tcp/9000 \
  --username "alice"
```

### 4. Verify Deployment

```bash
# Check relay health
curl http://203.0.113.10:8080/health

# Check metrics
curl http://203.0.113.10:9090/metrics

# Test user connectivity
dchat user --bootstrap /ip4/203.0.113.10/tcp/9000 --non-interactive
```

---

## 📊 Deployment Readiness Status

| Component | Status | Blockers |
|-----------|--------|----------|
| Core Cryptography | ✅ Ready | None |
| Network Stack | ✅ Ready | Bootstrap config needed |
| Relay Nodes | ✅ Ready | Infrastructure deployment |
| User Clients | ✅ Ready | Bootstrap config needed |
| Validator Nodes | ✅ Ready | Chain RPC config needed |
| Bot System | ✅ Ready | API endpoint config |
| iOS Biometric | ✅ Ready | None |
| Android Biometric | ⚠️ Limited | JNI implementation |
| Post-Quantum | 🔄 Optional | Dilithium library integration |

**Overall**: ✅ **PRODUCTION READY** with configuration

---

## 🔒 Security Notes

### What's Production-Ready
- ✅ End-to-end encryption (ChaCha20Poly1305)
- ✅ Key rotation and forward secrecy
- ✅ Merkle proof verification
- ✅ Signature validation
- ✅ Rate limiting and DoS protection
- ✅ NAT traversal (STUN/TURN/UPnP)

### What Needs External Setup
- ⚠️ Bootstrap node DNS/IPs (infrastructure)
- ⚠️ TLS certificates (infrastructure)
- ⚠️ Validator keys (secrets management)
- ⚠️ OAuth credentials (external APIs)

### What's Optional
- 🔄 Post-quantum cryptography (roadmap 2030)
- 🔄 Android biometric (needs JNI)
- 🔄 Hardware QRNG (optional entropy source)

---

## 📚 Additional Documentation

- **Architecture**: `ARCHITECTURE.md`
- **Mock Code Analysis**: `MOCK_CODE_REMEDIATION_SUMMARY.md`
- **Production Roadmap**: `PRODUCTION_IMPROVEMENTS_ROADMAP.md`
- **Deployment Package**: `DEPLOYMENT_READY_SUMMARY.md`
- **Bot Examples**: `crates/dchat-bots/examples/README.md`

---

## 🆘 Support

### Deployment Issues
1. Check health endpoints: `http://node-ip:8080/health`
2. Review logs: Look for ERROR/WARN in output
3. Test connectivity: `dchat user --bootstrap <addr> --non-interactive`

### Configuration Issues
1. Verify DNS resolution: `dig bootstrap-1.dchat.network`
2. Check environment variables: `echo $DCHAT_BOOTSTRAP_NODES`
3. Validate config.toml syntax: `toml check config.toml`

### Common Errors

**Error**: "Failed to connect to bootstrap nodes"
- **Fix**: Set `DCHAT_BOOTSTRAP_NODES` with real IPs or configure DNS

**Error**: "No peer connections after 30s"
- **Fix**: Check firewall rules, ensure TCP/9000 is open

**Error**: "Android biometric not available"
- **Fix**: Expected behavior, see Section 4 for alternatives

---

**Last Review**: 2025-11-04  
**Next Review**: Before production launch  
**Maintainer**: dchat Core Team
