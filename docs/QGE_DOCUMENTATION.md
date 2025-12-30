# Quorum-Gated Encryption (QGE) - Technical Documentation

> **Version**: 1.0.0  
> **Last Updated**: December 2025  
> **Status**: Production Ready

## Table of Contents

1. [Overview](#1-overview)
2. [Architecture](#2-architecture)
3. [Components](#3-components)
4. [Token Lifecycle](#4-token-lifecycle)
5. [Revocation System](#5-revocation-system)
6. [Relay Incentives](#6-relay-incentives)
7. [Security Considerations](#7-security-considerations)
8. [API Reference](#8-api-reference)
9. [CLI Commands](#9-cli-commands)
10. [Integration Guide](#10-integration-guide)

---

## 1. Overview

Quorum-Gated Encryption (QGE) is dchat's cryptographic access control system that ensures only authorized users can read messages in channels and direct conversations. Unlike traditional server-side access control, QGE uses threshold cryptography to create a decentralized, trustless system where no single party can grant or revoke access.

### Key Properties

| Property            | Description                               |
| ------------------- | ----------------------------------------- |
| **Decentralized**   | No single authority controls access       |
| **Threshold-based** | Requires k-of-n relay signatures          |
| **Epoch-based**     | Tokens valid for fixed time periods       |
| **Forward Secure**  | Revoked users cannot read future messages |
| **Auditable**       | All operations logged with hash chain     |

### Design Goals

1. **Security**: Cryptographic enforcement of access control
2. **Privacy**: Minimize metadata leakage
3. **Performance**: Efficient for large channels (10,000+ members)
4. **Resilience**: Tolerate relay failures without service disruption

---

## 2. Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           QGE SYSTEM                                    │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌──────────────┐     ┌─────────────────────┐     ┌─────────────────┐  │
│  │   CLIENT     │     │   RELAY COMMITTEE   │     │   CHAIN STATE   │  │
│  │              │◄───►│                     │◄───►│                 │  │
│  │ • Request    │     │ • FROST Signing     │     │ • Revocations   │  │
│  │   Tokens     │     │ • Token Issuance    │     │ • Memberships   │  │
│  │ • Encrypt    │     │ • Revocation Check  │     │ • Stakes        │  │
│  │ • Decrypt    │     │ • Committee Rotate  │     │ • Rewards       │  │
│  └──────────────┘     └─────────────────────┘     └─────────────────┘  │
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────────┤
│  │                     SUPPORTING SYSTEMS                              │
│  ├─────────────────────────────────────────────────────────────────────│
│  │  Rate Limiter │ Audit Logger │ Incentives │ Cleanup │ Group Keys  │ │
│  └─────────────────────────────────────────────────────────────────────┘
└─────────────────────────────────────────────────────────────────────────┘
```

### Component Relationships

```
EpochToken ──────────► EpochTokenIssuer ──────────► FrostSigningCommittee
    │                        │                              │
    ▼                        ▼                              ▼
RevocationChecker ◄──── TokenRequest ─────────► CommitteeRotation
    │                                                       │
    ▼                                                       ▼
AdminRevocation ────► RevocationPropagation ◄─────── RelayIncentives
    │                        │                              │
    ▼                        ▼                              ▼
AuditLogger ◄────────── RateLimiter ──────────► ForwardSecrecyCleanup
```

---

## 3. Components

### 3.1 Epoch Token System

**Location**: `crates/dchat-network/src/relay/epoch_token.rs`

Epoch tokens are time-limited credentials that authorize message encryption/decryption.

```rust
pub struct EpochToken {
    pub epoch_id: u64,
    pub user_id: [u8; 32],
    pub channel_id: Option<[u8; 32]>,
    pub conversation_type: ConversationType,
    pub signature: [u8; 64],  // FROST aggregate signature
}
```

**Constants**:

- `EPOCH_DURATION_SECS`: 600 (10 minutes)
- `EPOCH_GRACE_PERIOD_SECS`: 60 (1 minute overlap)
- `MAX_CACHED_EPOCHS`: 3

### 3.2 Committee Rotation

**Location**: `crates/dchat-network/src/relay/committee_rotation.rs`

Manages the relay committee that signs epoch tokens.

```rust
pub struct CommitteeRotationManager {
    config: RotationConfig,
    current_committee: Vec<CommitteeMember>,
    pending_committee: Option<Vec<CommitteeMember>>,
    rotation_history: VecDeque<RotationEvent>,
}
```

**Rotation Phases**:

1. **Announcement**: New committee published
2. **Preparation**: Key shares distributed
3. **Transition**: Both committees active
4. **Completion**: Old committee retired

### 3.3 Revocation System

**Location**: `crates/dchat-network/src/relay/revocation.rs`

Handles access revocation and propagation.

```rust
pub enum RevocationType {
    UserBan,           // Global ban
    ChannelBan,        // Channel-specific
    TemporaryMute,     // Time-limited
    ShadowBan,         // Silent restriction
    DeviceRevocation,  // Device-specific
}
```

### 3.4 Admin Revocation

**Location**: `crates/dchat-network/src/relay/admin_revocation.rs`

Channel admin capabilities for member management.

```rust
pub enum AdminRole {
    Member,     // Regular member
    Moderator,  // Can mute, timeout
    Admin,      // Can ban, unban
    Owner,      // Full control
}

pub enum RevocationAction {
    Timeout { duration_secs: u64 },
    Mute { duration_secs: u64 },
    SoftRevoke,  // Requires key rotation
    Ban,         // Permanent
    ShadowBan,   // Silent, receives but not seen
}
```

### 3.5 Rate Limiting

**Location**: `crates/dchat-network/src/relay/qge_rate_limiting.rs`

Prevents abuse of token requests and other operations.

```rust
pub struct QgeRateLimiter {
    buckets: HashMap<UserId, TokenBucket>,
    windows: HashMap<UserId, SlidingWindow>,
    config: RateLimitConfig,
}

pub enum RateLimitDecision {
    Allowed,
    Throttled { wait_ms: u64 },
    Blocked { reason: String, duration_secs: u64 },
    EmergencyBypass,
}
```

**Request Categories**:

- `TokenRequest` - New epoch token requests
- `MessageRelay` - Message forwarding
- `RevocationCheck` - Access verification
- `KeyDistribution` - Group key operations
- `CommitteeVote` - Committee operations

### 3.6 Audit Logging

**Location**: `crates/dchat-network/src/relay/qge_audit_logging.rs`

Tamper-evident logging for security events.

```rust
pub struct AuditEntry {
    pub id: u64,
    pub timestamp: u64,
    pub severity: Severity,
    pub category: EventCategory,
    pub event: EventType,
    pub actor: Option<ActorInfo>,
    pub target: Option<TargetInfo>,
    pub prev_hash: [u8; 32],  // Hash chain
    pub entry_hash: [u8; 32],
}
```

**Severity Levels**: Debug, Info, Warning, Error, Critical, Alert

### 3.7 Relay Incentives

**Location**: `crates/dchat-network/src/relay/relay_incentives.rs`

Economic incentives for relay participation.

```rust
pub struct RelayStake {
    pub relay_id: [u8; 32],
    pub operator_id: [u8; 32],
    pub stake_amount: u64,
    pub reputation: f64,
    pub region: GeoRegion,
}

pub struct EpochReward {
    pub base_reward: u64,
    pub token_bonus: u64,
    pub message_bonus: u64,
    pub uptime_multiplier: f64,
    pub geo_bonus: u64,
}
```

**Constants**:

- `MIN_RELAY_STAKE`: 10,000 DCHAT
- `MAX_EFFECTIVE_STAKE`: 100,000 DCHAT
- `BASE_EPOCH_REWARD`: 1,000,000,000 motes
- `MAX_UPTIME_BONUS`: 2.0x

### 3.8 Forward Secrecy Cleanup

**Location**: `crates/dchat-crypto/src/forward_secrecy_cleanup.rs`

Automatic cleanup of cryptographic state.

```rust
pub struct ForwardSecrecyCleanup {
    epoch_keys: HashMap<ConversationId, TrackedKey>,
    chain_keys: HashMap<ConversationId, TrackedKey>,
    skipped_keys: HashMap<(ConversationId, u64), TrackedKey>,
    config: CleanupConfig,
}
```

**Cleanup Policies**:

- Epoch keys: Retained for 12 epochs (2 hours)
- Chain keys: Cleaned after conversation ends
- Skipped keys: 7 days maximum retention
- SUKs: Cleaned after unlock verification

### 3.9 Group Key Distribution

**Location**: `crates/dchat-crypto/src/group_key.rs`

Efficient key distribution for large groups using Sender Keys.

```rust
pub struct GroupKeyDistribution {
    sender_keys: HashMap<(ChannelId, UserId), SenderChainKey>,
    tree_distribution: Option<TreeKeyDistribution>,
}
```

**Features**:

- O(1) encryption per message
- Tree-based distribution (fanout 32)
- Key rotation on member changes
- Maximum group size: 10,000 members

---

## 4. Token Lifecycle

### 4.1 Token Request Flow

```
User Device                     Relay Committee                  Chain
    │                                │                             │
    │──── 1. TokenRequest ──────────►│                             │
    │                                │──── 2. Check Revocations ──►│
    │                                │◄─── 3. Revocation Status ───│
    │                                │                             │
    │                                │──── 4. FROST Round 1 ──────►│
    │                                │                             │
    │                                │──── 5. FROST Round 2 ──────►│
    │                                │                             │
    │◄─── 6. EpochToken ────────────│                             │
    │                                │                             │
```

### 4.2 Token Validation

1. **Epoch Check**: Token epoch must be current or within grace period
2. **Signature Check**: FROST aggregate signature must verify
3. **Revocation Check**: User/device not in revocation list
4. **Rate Limit Check**: Request within allowed limits

### 4.3 Token Refresh

Tokens are automatically refreshed before expiration:

```rust
pub async fn maybe_refresh_token(&self, token: &EpochToken) -> Option<EpochToken> {
    let current_epoch = current_epoch_id();
    let remaining = token.epoch_id.saturating_sub(current_epoch);

    if remaining <= 1 {
        // Refresh needed
        self.request_token(token.conversation_type, token.channel_id).await
    } else {
        None
    }
}
```

---

## 5. Revocation System

### 5.1 Revocation Types

| Type       | Scope   | Duration  | Key Rotation |
| ---------- | ------- | --------- | ------------ |
| Timeout    | Channel | Fixed     | No           |
| Mute       | Channel | Fixed     | No           |
| SoftRevoke | Channel | Permanent | Yes          |
| Ban        | Channel | Permanent | Yes          |
| ShadowBan  | Channel | Permanent | No           |
| GlobalBan  | Network | Permanent | Yes          |

### 5.2 Revocation Propagation

Revocations are propagated via gossip protocol:

```rust
pub struct RevocationGossipMessage {
    pub revocation_id: RevocationId,
    pub revocation: RevocationEntry,
    pub signature: [u8; 64],
    pub priority: PropagationPriority,
}
```

**Propagation Priorities**:

- `Critical`: Immediate broadcast (security threats)
- `High`: Within 1 epoch
- `Normal`: Standard gossip
- `Low`: Batch with others

### 5.3 Appeal Process

```rust
pub enum AppealStatus {
    Pending,
    UnderReview { reviewer: [u8; 32], since: u64 },
    Approved { by: [u8; 32], at: u64 },
    Rejected { by: [u8; 32], reason: String },
    Escalated,
}
```

---

## 6. Relay Incentives

### 6.1 Reward Calculation

```
Total Reward = Base + Token Bonus + Message Bonus + Geographic Bonus
             × Uptime Multiplier × Stake Weight
```

**Components**:

- **Base Reward**: Fixed per epoch
- **Token Bonus**: Per token issued
- **Message Bonus**: Per message relayed
- **Geographic Bonus**: For underserved regions
- **Uptime Multiplier**: 1.0 - 2.0x based on history

### 6.2 Slashing Conditions

| Condition         | Penalty    | Recovery  |
| ----------------- | ---------- | --------- |
| Double Sign       | 50% stake  | 30 days   |
| Extended Downtime | 1% per 24h | Immediate |
| Invalid Token     | 10% stake  | 7 days    |
| False Proof       | 100% stake | Permanent |

### 6.3 Geographic Diversity

```rust
pub enum GeoRegion {
    NorthAmerica,    // 1.0x bonus
    Europe,          // 1.0x bonus
    Asia,            // 1.1x bonus
    SouthAmerica,    // 1.2x bonus
    Africa,          // 1.3x bonus
    Oceania,         // 1.1x bonus
    Other,           // 1.0x bonus
}
```

---

## 7. Security Considerations

### 7.1 Threat Model

| Threat          | Mitigation                    |
| --------------- | ----------------------------- |
| Malicious Relay | Threshold signatures (k-of-n) |
| Token Replay    | Epoch-based expiration        |
| Key Compromise  | Forward secrecy, key rotation |
| DoS Attacks     | Rate limiting, staking        |
| Collusion       | Committee rotation, diversity |

### 7.2 Cryptographic Primitives

- **Key Exchange**: X25519
- **Signatures**: Ed25519, FROST
- **Encryption**: ChaCha20-Poly1305
- **Hashing**: BLAKE3
- **KDF**: HKDF-SHA256

### 7.3 Rate Limiting Strategy

```
Level 1: Token Bucket (burst capacity)
Level 2: Sliding Window (sustained rate)
Level 3: Reputation Modifier (0.1x - 2.0x)
Level 4: Abuse Detection (automatic blocking)
```

---

## 8. API Reference

### 8.1 EpochTokenManager

```rust
impl EpochTokenManager {
    /// Request a new epoch token
    pub async fn request_token(
        &self,
        conversation_type: ConversationType,
        channel_id: Option<[u8; 32]>,
    ) -> Result<EpochToken>;

    /// Validate an epoch token
    pub fn validate_token(&self, token: &EpochToken) -> Result<()>;

    /// Check if token needs refresh
    pub fn needs_refresh(&self, token: &EpochToken) -> bool;
}
```

### 8.2 RevocationChecker

```rust
impl RevocationChecker {
    /// Check if user is revoked from channel
    pub async fn check(
        &self,
        user_id: &[u8; 32],
        channel_id: &[u8; 32],
    ) -> RevocationCheckResult;

    /// Add a new revocation
    pub async fn add_revocation(&self, entry: RevocationEntry);

    /// Remove expired revocations
    pub async fn cleanup_expired(&self);
}
```

### 8.3 QgeRateLimiter

```rust
impl QgeRateLimiter {
    /// Check rate limit for a request
    pub async fn check_rate_limit(
        &self,
        user_id: &[u8; 32],
        category: RequestCategory,
        reputation: f64,
    ) -> RateLimitDecision;

    /// Get user's current rate limit status
    pub async fn get_user_info(&self, user_id: &[u8; 32]) -> Option<UserRateLimitInfo>;
}
```

### 8.4 QgeAuditLogger

```rust
impl QgeAuditLogger {
    /// Log a security event
    pub async fn log(
        &self,
        severity: Severity,
        event: EventType,
        actor: Option<ActorInfo>,
        target: Option<TargetInfo>,
        context: HashMap<String, String>,
    );

    /// Query audit entries
    pub async fn query(&self, filter: &AuditQuery) -> Vec<AuditEntry>;

    /// Verify hash chain integrity
    pub async fn verify_chain(&self) -> bool;
}
```

---

## 9. CLI Commands

### 9.1 Token Management

```bash
# Show token status
dchat qge token-status
dchat qge token-status --conversation-type channel
dchat qge token-status --epoch 12345 --include-expired

# Request new token
dchat qge request-token --target <channel-id> --conversation-type channel
dchat qge request-token --target <user-id> --force
```

### 9.2 Committee Information

```bash
# Show committee status
dchat qge committee
dchat qge committee --detailed
dchat qge committee --history --epochs 20
```

### 9.3 Relay Status

```bash
# Show relay status
dchat qge relay-status
dchat qge relay-status --stake --metrics --rewards
dchat qge relay-status --relay-id <hex-id>
```

### 9.4 Revocation Management

```bash
# List revocations
dchat qge revocation list
dchat qge revocation list --channel-id <hex-id> --pending

# Check revocation status
dchat qge revocation check --user-id <hex> --channel-id <hex>

# Create revocation (admin)
dchat qge revocation create \
  --user-id <hex> \
  --channel-id <hex> \
  --action ban \
  --reason "Terms violation"

# Lift revocation (admin)
dchat qge revocation lift --revocation-id <hex>

# Submit appeal
dchat qge revocation appeal --revocation-id <hex> --message "Appeal text"
```

### 9.5 Audit Logs

```bash
# View audit logs
dchat qge audit-log
dchat qge audit-log --category security --min-severity critical
dchat qge audit-log --from "2025-01-01" --to "2025-01-31" --format json
```

### 9.6 Maintenance

```bash
# Check rate limit status
dchat qge rate-limit-status
dchat qge rate-limit-status --detailed

# Show statistics
dchat qge stats --network --detailed

# Cleanup old state
dchat qge cleanup --dry-run
dchat qge cleanup --force --max-age 86400
```

---

## 10. Integration Guide

### 10.1 Client Integration

```rust
use dchat_network::relay::{
    EpochTokenManager, RevocationChecker, current_epoch_id,
};

// Initialize components
let token_manager = EpochTokenManager::new(config);
let revocation_checker = RevocationChecker::new();

// Request token for channel
let token = token_manager
    .request_token(ConversationType::Channel, Some(channel_id))
    .await?;

// Before sending message, check access
let check = revocation_checker.check(&my_user_id, &channel_id).await;
if !check.allowed {
    return Err("Access denied: {:?}", check.reason);
}

// Encrypt message with epoch key
let epoch_key = derive_epoch_key(&token);
let ciphertext = encrypt_message(&message, &epoch_key)?;
```

### 10.2 Relay Integration

```rust
use dchat_network::relay::{
    EpochTokenIssuer, FrostSigningCommittee, CommitteeRotationManager,
    RelayIncentivesManager, QgeAuditLogger,
};

// Initialize relay components
let issuer = EpochTokenIssuer::new(frost_key_share);
let rotation_manager = CommitteeRotationManager::new(rotation_config);
let incentives = RelayIncentivesManager::new();
let logger = QgeAuditLogger::with_file("audit.log")?;

// Handle token request
async fn handle_token_request(req: TokenRequest) -> Result<TokenShare> {
    // Rate limit check
    let decision = rate_limiter.check(&req.user_id, RequestCategory::TokenRequest, 1.0).await;
    if !matches!(decision, RateLimitDecision::Allowed) {
        return Err("Rate limited");
    }

    // Revocation check
    if !revocation_checker.check(&req.user_id, &req.channel_id).await.allowed {
        return Err("User revoked");
    }

    // Sign token share
    let share = issuer.sign_token_share(&req)?;

    // Log issuance
    logger.log_token_issued(req.user_id, req.epoch_id, share.hash()).await;

    // Record for incentives
    incentives.record_token_issued(my_relay_id, req.epoch_id).await;

    Ok(share)
}
```

### 10.3 Testing

```rust
#[tokio::test]
async fn test_token_flow() {
    let manager = EpochTokenManager::new_for_testing();

    // Request token
    let token = manager
        .request_token(ConversationType::Direct, None)
        .await
        .unwrap();

    assert!(token.is_valid());
    assert_eq!(token.epoch_id, current_epoch_id());
}

#[tokio::test]
async fn test_revocation() {
    let checker = RevocationChecker::new();
    let user = [1u8; 32];
    let channel = [2u8; 32];

    // Initially allowed
    assert!(checker.check(&user, &channel).await.allowed);

    // Add revocation
    checker.add_revocation(RevocationEntry::ban(user, channel)).await;

    // Now denied
    assert!(!checker.check(&user, &channel).await.allowed);
}
```

---

## Appendix A: Constants Reference

| Constant                  | Value         | Description           |
| ------------------------- | ------------- | --------------------- |
| `EPOCH_DURATION_SECS`     | 600           | Epoch length (10 min) |
| `EPOCH_GRACE_PERIOD_SECS` | 60            | Token overlap period  |
| `MAX_CACHED_EPOCHS`       | 3             | Cached epoch keys     |
| `MIN_RELAY_STAKE`         | 10,000 DCHAT  | Minimum stake         |
| `MAX_EFFECTIVE_STAKE`     | 100,000 DCHAT | Capped stake          |
| `MAX_GROUP_SIZE`          | 10,000        | Max channel members   |
| `DEFAULT_BUCKET_CAPACITY` | 100           | Rate limit bucket     |
| `LOG_RETENTION_SECS`      | 7,776,000     | 90-day log retention  |

## Appendix B: Error Codes

| Code     | Name                 | Description             |
| -------- | -------------------- | ----------------------- |
| `QGE001` | TokenExpired         | Epoch token has expired |
| `QGE002` | InvalidSignature     | FROST signature invalid |
| `QGE003` | UserRevoked          | User access revoked     |
| `QGE004` | RateLimited          | Request rate exceeded   |
| `QGE005` | CommitteeUnavailable | Cannot reach quorum     |
| `QGE006` | InsufficientStake    | Relay stake too low     |
| `QGE007` | InvalidEpoch         | Epoch ID invalid        |
| `QGE008` | ChainMismatch        | Hash chain broken       |

---

_For more information, see the [Architecture 2.0 document](./ARCHITECTURE-2.0.md) and [API Specification](./API_SPECIFICATION.md)._
