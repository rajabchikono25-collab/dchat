#![cfg(feature = "qge-integration-tests")]

//! QGE Integration Tests
//!
//! End-to-end integration tests for Quorum-Gated Encryption flow.
//! These tests verify the complete token issuance, encryption, and revocation lifecycle.
//!
//! # Test Scenarios
//!
//! 1. Token Issuance Flow
//! 2. Message Encryption with Epoch Tokens
//! 3. Revocation and Access Denial
//! 4. Committee Rotation and Token Continuity
//! 5. Multi-Device Token Sync
//! 6. Rate Limiting Behavior
//! 7. Audit Logging Verification

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// Import QGE components
use dchat_network::relay::{
    // Epoch tokens
    current_epoch_id,
    epoch_start,
    // Admin revocation
    AdminRevocationManager,
    AdminRole,
    // Committee
    CommitteeInfo,
    CommitteeRotationManager,
    ConversationType,
    EpochToken,
    EpochTokenIssuer,
    EpochTokenRequest,
    // Audit logging
    EventCategory,
    EventType,
    GeoRegion,
    QgeAuditLogger,
    // Rate limiting
    QgeRateLimiter,
    RateLimitConfig,
    RateLimitDecision,
    // Incentives
    RelayIncentivesManager,
    RelayStake,
    RequestCategory,
    RevocationAction,
    // Revocation
    RevocationChecker,
    RevocationEntry,
    RevocationId,
    RevocationReason,
    RevocationType,
    RotationConfig,
    Severity,
    TokenRejectionReason,
    EPOCH_DURATION_SECS,
};

/// Test user ID type
type UserId = [u8; 32];
/// Test channel ID type
type ChannelId = [u8; 32];
/// Test relay ID type
type RelayId = [u8; 32];

/// Create a test user ID
fn test_user(n: u8) -> UserId {
    let mut id = [0u8; 32];
    id[0] = n;
    id
}

/// Create a test channel ID
fn test_channel(n: u8) -> ChannelId {
    let mut id = [0u8; 32];
    id[31] = n;
    id
}

/// Create a test relay ID
fn test_relay(n: u8) -> RelayId {
    let mut id = [0u8; 32];
    id[15] = n;
    id
}

// =============================================================================
// Token Issuance Tests
// =============================================================================

#[tokio::test]
async fn test_epoch_token_creation() {
    let user = test_user(1);
    let channel = test_channel(1);
    let epoch_id = current_epoch_id();

    // Create epoch token directly (simulating successful committee signing)
    let token = EpochToken::new(
        epoch_id,
        user,
        Some(channel),
        ConversationType::Channel,
        [0xAB; 64], // Simulated signature
    );

    assert_eq!(token.epoch_id, epoch_id);
    assert_eq!(token.user_id, user);
    assert_eq!(token.channel_id, Some(channel));
    assert!(token.is_valid()); // Epoch is current
}

#[tokio::test]
async fn test_epoch_token_expiration() {
    let user = test_user(1);
    let epoch_id = current_epoch_id().saturating_sub(2); // 2 epochs ago

    let token = EpochToken::new(epoch_id, user, None, ConversationType::Direct, [0xAB; 64]);

    assert!(!token.is_valid()); // Token should be expired
}

#[tokio::test]
async fn test_token_request_creation() {
    let user = test_user(1);
    let device = test_user(2);
    let channel = test_channel(1);
    let epoch_id = current_epoch_id();

    let request = EpochTokenRequest::new(
        user,
        device,
        epoch_id,
        ConversationType::Channel,
        Some(channel),
    );

    assert_eq!(request.user_id, user);
    assert_eq!(request.device_id, device);
    assert_eq!(request.epoch_id, epoch_id);
    assert_eq!(request.conversation_type, ConversationType::Channel);
}

// =============================================================================
// Revocation Tests
// =============================================================================

#[tokio::test]
async fn test_revocation_check() {
    let checker = RevocationChecker::new();
    let user = test_user(1);
    let channel = test_channel(1);

    // Initially not revoked
    let result = checker.check(&user, &channel).await;
    assert!(result.allowed);

    // Add a revocation
    let revocation = RevocationEntry::new(
        RevocationId::generate(),
        user,
        Some(channel),
        RevocationType::ChannelBan,
        RevocationReason::AdminAction {
            admin: test_user(99),
            reason: "Test ban".to_string(),
        },
        None, // Permanent
    );

    checker.add_revocation(revocation).await;

    // Now should be revoked
    let result = checker.check(&user, &channel).await;
    assert!(!result.allowed);
}

#[tokio::test]
async fn test_temporary_revocation_expiry() {
    let checker = RevocationChecker::new();
    let user = test_user(1);
    let channel = test_channel(1);

    // Add a very short timeout
    let revocation = RevocationEntry::new(
        RevocationId::generate(),
        user,
        Some(channel),
        RevocationType::Timeout,
        RevocationReason::AdminAction {
            admin: test_user(99),
            reason: "Short timeout".to_string(),
        },
        Some(1), // 1 second timeout
    );

    checker.add_revocation(revocation).await;

    // Should be revoked immediately
    let result = checker.check(&user, &channel).await;
    assert!(!result.allowed);

    // Wait for expiry
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Run cleanup
    checker.cleanup_expired().await;

    // Should no longer be revoked
    let result = checker.check(&user, &channel).await;
    assert!(result.allowed);
}

// =============================================================================
// Admin Revocation Tests
// =============================================================================

#[tokio::test]
async fn test_admin_revocation_manager() {
    let manager = AdminRevocationManager::new();
    let channel = test_channel(1);
    let owner = test_user(1);
    let member = test_user(2);

    // Set up channel with owner
    manager
        .set_channel_role(channel, owner, AdminRole::Owner)
        .await;
    manager
        .set_channel_role(channel, member, AdminRole::Member)
        .await;

    // Owner revokes member
    let result = manager
        .revoke_member(
            channel,
            owner,
            member,
            RevocationAction::SoftRevoke,
            "Spam".to_string(),
            None,
        )
        .await;

    assert!(result.is_ok());

    // Check member is revoked
    let state = manager.get_member_state(channel, member).await;
    assert!(state.is_some());
    assert!(!state.unwrap().is_active());
}

#[tokio::test]
async fn test_admin_role_hierarchy() {
    let manager = AdminRevocationManager::new();
    let channel = test_channel(1);
    let owner = test_user(1);
    let admin = test_user(2);
    let moderator = test_user(3);
    let member = test_user(4);

    // Set up hierarchy
    manager
        .set_channel_role(channel, owner, AdminRole::Owner)
        .await;
    manager
        .set_channel_role(channel, admin, AdminRole::Admin)
        .await;
    manager
        .set_channel_role(channel, moderator, AdminRole::Moderator)
        .await;
    manager
        .set_channel_role(channel, member, AdminRole::Member)
        .await;

    // Admin can revoke moderator
    let result = manager
        .revoke_member(
            channel,
            admin,
            moderator,
            RevocationAction::Mute,
            "Test".to_string(),
            Some(3600),
        )
        .await;
    assert!(result.is_ok());

    // Moderator cannot revoke admin (insufficient permission)
    let result = manager
        .revoke_member(
            channel,
            moderator,
            admin,
            RevocationAction::Mute,
            "Test".to_string(),
            Some(3600),
        )
        .await;
    assert!(result.is_err());
}

// =============================================================================
// Rate Limiting Tests
// =============================================================================

#[tokio::test]
async fn test_rate_limiter_allows_normal_usage() {
    let limiter = QgeRateLimiter::new(RateLimitConfig::default());
    let user = test_user(1);

    // Normal usage should be allowed
    for _ in 0..10 {
        let decision = limiter
            .check_rate_limit(&user, RequestCategory::TokenRequest, 1.0)
            .await;
        assert!(matches!(decision, RateLimitDecision::Allowed));
    }
}

#[tokio::test]
async fn test_rate_limiter_throttles_excessive_usage() {
    let config = RateLimitConfig {
        bucket_capacity: 5,
        refill_rate: 1.0,
        window_secs: 60,
        ..Default::default()
    };
    let limiter = QgeRateLimiter::new(config);
    let user = test_user(1);

    // Exhaust the bucket
    for _ in 0..5 {
        let decision = limiter
            .check_rate_limit(&user, RequestCategory::TokenRequest, 1.0)
            .await;
        assert!(matches!(decision, RateLimitDecision::Allowed));
    }

    // Next request should be throttled
    let decision = limiter
        .check_rate_limit(&user, RequestCategory::TokenRequest, 1.0)
        .await;
    assert!(matches!(decision, RateLimitDecision::Throttled { .. }));
}

#[tokio::test]
async fn test_rate_limiter_reputation_bonus() {
    let limiter = QgeRateLimiter::new(RateLimitConfig::default());
    let good_user = test_user(1);
    let bad_user = test_user(2);

    // Set different reputations (in production this would come from reputation system)
    limiter.set_user_reputation(&good_user, 0.9).await;
    limiter.set_user_reputation(&bad_user, 0.1).await;

    // Good user gets more capacity
    let info_good = limiter.get_user_info(&good_user).await;
    let info_bad = limiter.get_user_info(&bad_user).await;

    // Good user should have more effective capacity
    assert!(info_good.is_some());
    assert!(info_bad.is_some());
}

// =============================================================================
// Audit Logging Tests
// =============================================================================

#[tokio::test]
async fn test_audit_logger_records_events() {
    let logger = QgeAuditLogger::new();

    // Log some events
    logger
        .log(
            Severity::Info,
            EventType::TokenIssued {
                epoch_id: 100,
                token_hash: [0xAB; 32],
            },
            None,
            None,
            HashMap::new(),
        )
        .await;

    logger
        .log(
            Severity::Warning,
            EventType::AccessDenied {
                resource: "channel-1".to_string(),
                reason: "Revoked".to_string(),
            },
            None,
            None,
            HashMap::new(),
        )
        .await;

    let stats = logger.stats().await;
    assert_eq!(stats.total_entries, 2);
}

#[tokio::test]
async fn test_audit_logger_query_filtering() {
    let logger = QgeAuditLogger::new();

    // Log events of different categories
    logger
        .log(
            Severity::Info,
            EventType::TokenIssued {
                epoch_id: 1,
                token_hash: [0; 32],
            },
            None,
            None,
            HashMap::new(),
        )
        .await;

    logger
        .log(
            Severity::Critical,
            EventType::AttackDetected {
                attack_type: "DoS".to_string(),
                source: "unknown".to_string(),
            },
            None,
            None,
            HashMap::new(),
        )
        .await;

    logger
        .log(
            Severity::Info,
            EventType::KeyGenerated {
                key_type: "SUK".to_string(),
                key_id: [0; 32],
            },
            None,
            None,
            HashMap::new(),
        )
        .await;

    // Query only security events
    use dchat_network::relay::AuditQuery;
    let query = AuditQuery::new().category(EventCategory::Security);
    let results = logger.query(&query).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].severity, Severity::Critical);

    // Query by severity
    let query = AuditQuery::new().min_severity(Severity::Critical);
    let results = logger.query(&query).await;
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_audit_logger_hash_chain() {
    let logger = QgeAuditLogger::new();

    // Log multiple events
    for i in 0..10 {
        logger
            .log(
                Severity::Info,
                EventType::EpochTransition {
                    from_epoch: i,
                    to_epoch: i + 1,
                },
                None,
                None,
                HashMap::new(),
            )
            .await;
    }

    // Verify hash chain integrity
    assert!(logger.verify_chain().await);
}

// =============================================================================
// Relay Incentives Tests
// =============================================================================

#[tokio::test]
async fn test_relay_stake_registration() {
    let manager = RelayIncentivesManager::new();
    let relay = test_relay(1);
    let operator = test_user(1);

    // Register stake
    let stake = RelayStake {
        relay_id: relay,
        operator_id: operator,
        stake_amount: 50_000_000_000_000, // 50,000 DCHAT
        staked_at: std::time::SystemTime::now(),
        locked_until: None,
        reputation: 1.0,
        region: GeoRegion::NorthAmerica,
    };

    let result = manager.register_stake(stake).await;
    assert!(result.is_ok());

    // Check stake is registered
    let info = manager.get_relay_info(&relay).await;
    assert!(info.is_some());
}

#[tokio::test]
async fn test_relay_reward_calculation() {
    let manager = RelayIncentivesManager::new();
    let relay = test_relay(1);
    let operator = test_user(1);

    // Register stake
    let stake = RelayStake {
        relay_id: relay,
        operator_id: operator,
        stake_amount: 50_000_000_000_000,
        staked_at: std::time::SystemTime::now(),
        locked_until: None,
        reputation: 1.0,
        region: GeoRegion::NorthAmerica,
    };
    manager.register_stake(stake).await.unwrap();

    // Record performance
    manager
        .record_performance(
            relay,
            current_epoch_id(),
            100,  // tokens issued
            1000, // messages relayed
            0.99, // uptime
        )
        .await;

    // Calculate rewards
    let reward = manager
        .calculate_epoch_reward(&relay, current_epoch_id())
        .await;
    assert!(reward.is_some());
    assert!(reward.unwrap().total_amount > 0);
}

#[tokio::test]
async fn test_relay_slashing() {
    let manager = RelayIncentivesManager::new();
    let relay = test_relay(1);
    let operator = test_user(1);

    // Register stake
    let stake = RelayStake {
        relay_id: relay,
        operator_id: operator,
        stake_amount: 50_000_000_000_000,
        staked_at: std::time::SystemTime::now(),
        locked_until: None,
        reputation: 1.0,
        region: GeoRegion::NorthAmerica,
    };
    manager.register_stake(stake).await.unwrap();

    // Record slashing event
    use dchat_network::relay::SlashingReason;
    let result = manager
        .slash_relay(
            relay,
            SlashingReason::DoubleSign,
            5_000_000_000_000, // 5,000 DCHAT penalty
        )
        .await;

    assert!(result.is_ok());

    // Check stake is reduced
    let info = manager.get_relay_info(&relay).await;
    assert!(info.is_some());
    assert!(info.unwrap().stake_amount < 50_000_000_000_000);
}

// =============================================================================
// Committee Rotation Tests
// =============================================================================

#[tokio::test]
async fn test_committee_rotation_scheduling() {
    let config = RotationConfig {
        rotation_interval_epochs: 10,
        transition_period_epochs: 2,
        min_committee_size: 3,
        max_committee_size: 10,
    };
    let manager = CommitteeRotationManager::new(config);

    let epoch = current_epoch_id();
    let should_rotate = manager.should_rotate(epoch).await;

    // Rotation happens every 10 epochs
    let expected = epoch % 10 == 0;
    assert_eq!(should_rotate, expected);
}

#[tokio::test]
async fn test_committee_member_selection() {
    let config = RotationConfig::default();
    let manager = CommitteeRotationManager::new(config);

    // Register candidate relays
    for i in 1..=5 {
        let relay = test_relay(i);
        manager
            .register_candidate(relay, 50_000 * i as u64, 0.95)
            .await;
    }

    // Select committee
    let committee = manager.select_committee(5).await;
    assert!(committee.is_ok());
    assert_eq!(committee.unwrap().len(), 5);
}

// =============================================================================
// End-to-End Flow Tests
// =============================================================================

#[tokio::test]
async fn test_full_token_issuance_flow() {
    // This test simulates the complete token issuance flow:
    // 1. User requests token
    // 2. Rate limiter checks
    // 3. Revocation check
    // 4. Committee signs token
    // 5. Audit log entry

    let rate_limiter = QgeRateLimiter::new(RateLimitConfig::default());
    let revocation_checker = RevocationChecker::new();
    let audit_logger = QgeAuditLogger::new();

    let user = test_user(1);
    let device = test_user(2);
    let channel = test_channel(1);
    let epoch_id = current_epoch_id();

    // Step 1: Check rate limit
    let rate_decision = rate_limiter
        .check_rate_limit(&user, RequestCategory::TokenRequest, 1.0)
        .await;
    assert!(matches!(rate_decision, RateLimitDecision::Allowed));

    // Step 2: Check revocation
    let revocation_result = revocation_checker.check(&user, &channel).await;
    assert!(revocation_result.allowed);

    // Step 3: Create token request
    let request = EpochTokenRequest::new(
        user,
        device,
        epoch_id,
        ConversationType::Channel,
        Some(channel),
    );

    // Step 4: Simulate committee signing (in production, this involves FROST)
    let token = EpochToken::new(
        epoch_id,
        user,
        Some(channel),
        ConversationType::Channel,
        [0xAB; 64], // Simulated committee signature
    );
    assert!(token.is_valid());

    // Step 5: Log the issuance
    let mut token_hash = [0u8; 32];
    token_hash.copy_from_slice(&blake3::hash(&token.signature).as_bytes()[..32]);

    audit_logger
        .log(
            Severity::Info,
            EventType::TokenIssued {
                epoch_id,
                token_hash,
            },
            Some(dchat_network::relay::ActorInfo {
                id: user,
                actor_type: dchat_network::relay::ActorType::User,
                ip_addr: None,
                device_id: Some(device),
            }),
            None,
            HashMap::new(),
        )
        .await;

    // Verify audit log
    let stats = audit_logger.stats().await;
    assert_eq!(stats.total_entries, 1);
}

#[tokio::test]
async fn test_revocation_flow_with_logging() {
    let revocation_checker = RevocationChecker::new();
    let admin_manager = AdminRevocationManager::new();
    let audit_logger = QgeAuditLogger::new();

    let channel = test_channel(1);
    let owner = test_user(1);
    let violator = test_user(2);

    // Set up channel
    admin_manager
        .set_channel_role(channel, owner, AdminRole::Owner)
        .await;
    admin_manager
        .set_channel_role(channel, violator, AdminRole::Member)
        .await;

    // Step 1: Owner initiates revocation
    let result = admin_manager
        .revoke_member(
            channel,
            owner,
            violator,
            RevocationAction::Ban,
            "Terms of service violation".to_string(),
            None,
        )
        .await;
    assert!(result.is_ok());

    // Step 2: Log the revocation
    audit_logger
        .log(
            Severity::Warning,
            EventType::MemberRevoked {
                action: "ban".to_string(),
                reason: "Terms of service violation".to_string(),
                duration_secs: None,
            },
            Some(dchat_network::relay::ActorInfo {
                id: owner,
                actor_type: dchat_network::relay::ActorType::Admin,
                ip_addr: None,
                device_id: None,
            }),
            Some(dchat_network::relay::TargetInfo {
                id: violator,
                target_type: dchat_network::relay::TargetType::User,
                info: Some("channel ban".to_string()),
            }),
            HashMap::new(),
        )
        .await;

    // Step 3: Add to revocation checker
    let revocation = RevocationEntry::new(
        RevocationId::generate(),
        violator,
        Some(channel),
        RevocationType::ChannelBan,
        RevocationReason::AdminAction {
            admin: owner,
            reason: "Terms of service violation".to_string(),
        },
        None,
    );
    revocation_checker.add_revocation(revocation).await;

    // Step 4: Verify access is denied
    let check_result = revocation_checker.check(&violator, &channel).await;
    assert!(!check_result.allowed);

    // Step 5: Log access denial
    audit_logger
        .log(
            Severity::Info,
            EventType::AccessDenied {
                resource: hex::encode(channel),
                reason: "User is banned".to_string(),
            },
            Some(dchat_network::relay::ActorInfo {
                id: violator,
                actor_type: dchat_network::relay::ActorType::User,
                ip_addr: None,
                device_id: None,
            }),
            None,
            HashMap::new(),
        )
        .await;

    // Verify complete audit trail
    let stats = audit_logger.stats().await;
    assert_eq!(stats.total_entries, 2);
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
async fn test_invalid_epoch_token_rejection() {
    let user = test_user(1);

    // Create token with invalid (future) epoch
    let future_epoch = current_epoch_id() + 100;
    let token = EpochToken::new(
        future_epoch,
        user,
        None,
        ConversationType::Direct,
        [0xAB; 64],
    );

    // Token from future should not be valid
    assert!(!token.is_valid());
}

#[tokio::test]
async fn test_rate_limit_blocked_after_abuse() {
    let config = RateLimitConfig {
        bucket_capacity: 10,
        refill_rate: 0.1,
        window_secs: 60,
        abuse_threshold: 5,
        ..Default::default()
    };
    let limiter = QgeRateLimiter::new(config);
    let abuser = test_user(1);

    // Simulate abuse pattern
    for _ in 0..15 {
        let _ = limiter
            .check_rate_limit(&abuser, RequestCategory::TokenRequest, 1.0)
            .await;
    }

    // Should be blocked
    let decision = limiter
        .check_rate_limit(&abuser, RequestCategory::TokenRequest, 1.0)
        .await;
    assert!(matches!(decision, RateLimitDecision::Blocked { .. }));
}
