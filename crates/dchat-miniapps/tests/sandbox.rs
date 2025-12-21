//! Sandbox and registry tests for dchat-miniapps
//!
//! Verifies:
//! - App registration and verification
//! - Sandbox resource limits
//! - Bot identity management
//! - Wallet integration

use dchat_miniapps::bot::{BotAuthToken, BotCommandContext, BotIdentity, BotPermission};
use dchat_miniapps::manifest::{AppManifest, AppType, ResourceLimits};
use dchat_miniapps::registry::{AppId, AppRegistration, Developer, VerifiedApp};
use dchat_miniapps::sandbox::{ResourceUsage, SandboxConfig, SandboxMessage, SandboxState};
use dchat_miniapps::wallet::{SignRequest, TransactionVisibility, WalletConnection};

fn pubkey_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

fn app_id_n(n: u8) -> AppId {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    AppId(bytes)
}

// ===================== Registry Tests =====================

#[test]
fn test_app_id_generation() {
    let id1 = AppId::generate();
    let id2 = AppId::generate();

    assert_ne!(id1.0, id2.0);
}

#[test]
fn test_developer_creation() {
    let pubkey = pubkey_n(1);
    let developer = Developer::new(pubkey, "TestDev".to_string());

    assert_eq!(developer.pubkey, pubkey);
    assert_eq!(developer.name, "TestDev");
    assert!(!developer.verified);
}

#[test]
fn test_developer_verification() {
    let pubkey = pubkey_n(1);
    let mut developer = Developer::new(pubkey, "TestDev".to_string());

    assert!(!developer.verified);

    developer.verify();

    assert!(developer.verified);
}

#[test]
fn test_app_registration_creation() {
    let app_id = app_id_n(1);
    let developer = Developer::new(pubkey_n(1), "TestDev".to_string());

    let registration = AppRegistration::new(
        app_id,
        "TestApp".to_string(),
        developer,
        "1.0.0".to_string(),
    );

    assert_eq!(registration.app_id, app_id);
    assert_eq!(registration.name, "TestApp");
    assert_eq!(registration.version, "1.0.0");
    assert!(!registration.is_verified());
}

#[test]
fn test_app_registration_signature() {
    let app_id = app_id_n(1);
    let developer = Developer::new(pubkey_n(1), "TestDev".to_string());

    let mut registration = AppRegistration::new(
        app_id,
        "TestApp".to_string(),
        developer,
        "1.0.0".to_string(),
    );

    // Sign the registration
    registration.sign(vec![1, 2, 3, 4]);

    assert!(registration.signature.is_some());
}

#[test]
fn test_verified_app_creation() {
    let app_id = app_id_n(1);
    let developer = Developer::new(pubkey_n(1), "TestDev".to_string());

    let registration = AppRegistration::new(
        app_id,
        "TestApp".to_string(),
        developer,
        "1.0.0".to_string(),
    );

    let verified = VerifiedApp::from_registration(registration, 12345);

    assert!(verified.is_some());
    let verified = verified.unwrap();
    assert_eq!(verified.verified_at, 12345);
}

// ===================== Manifest Tests =====================

#[test]
fn test_manifest_creation() {
    let manifest = AppManifest {
        app_id: app_id_n(1),
        name: "TestApp".to_string(),
        version: "1.0.0".to_string(),
        app_type: AppType::MiniApp,
        entry_point: "main.wasm".to_string(),
        permissions: vec![],
        resource_limits: ResourceLimits::default(),
        developer_pubkey: pubkey_n(1),
        signature: None,
    };

    assert_eq!(manifest.name, "TestApp");
    assert!(matches!(manifest.app_type, AppType::MiniApp));
}

#[test]
fn test_app_type_variants() {
    let mini_app = AppType::MiniApp;
    let bot = AppType::Bot;
    let widget = AppType::Widget;

    assert_ne!(
        std::mem::discriminant(&mini_app),
        std::mem::discriminant(&bot)
    );
    assert_ne!(
        std::mem::discriminant(&bot),
        std::mem::discriminant(&widget)
    );
}

#[test]
fn test_resource_limits_default() {
    let limits = ResourceLimits::default();

    assert!(limits.max_memory_bytes > 0);
    assert!(limits.max_cpu_ms > 0);
    assert!(limits.max_storage_bytes > 0);
    assert!(limits.max_network_requests_per_minute > 0);
}

#[test]
fn test_manifest_hash() {
    let manifest = AppManifest {
        app_id: app_id_n(1),
        name: "TestApp".to_string(),
        version: "1.0.0".to_string(),
        app_type: AppType::MiniApp,
        entry_point: "main.wasm".to_string(),
        permissions: vec![],
        resource_limits: ResourceLimits::default(),
        developer_pubkey: pubkey_n(1),
        signature: None,
    };

    let hash1 = manifest.compute_hash();
    let hash2 = manifest.compute_hash();

    assert_eq!(hash1, hash2);
}

// ===================== Sandbox Tests =====================

#[test]
fn test_sandbox_config_creation() {
    let config = SandboxConfig::default();

    assert!(config.memory_limit_bytes > 0);
    assert!(config.cpu_time_limit_ms > 0);
    assert!(config.allow_network);
}

#[test]
fn test_sandbox_config_restrictive() {
    let config = SandboxConfig::restrictive();

    // Restrictive config should have lower limits
    let default = SandboxConfig::default();
    assert!(config.memory_limit_bytes <= default.memory_limit_bytes);
    assert!(!config.allow_network);
}

#[test]
fn test_resource_usage_tracking() {
    let mut usage = ResourceUsage::new();

    assert_eq!(usage.memory_bytes, 0);
    assert_eq!(usage.cpu_time_ms, 0);

    usage.add_memory(1000);
    usage.add_cpu_time(50);

    assert_eq!(usage.memory_bytes, 1000);
    assert_eq!(usage.cpu_time_ms, 50);
}

#[test]
fn test_resource_usage_exceeds_limit() {
    let mut usage = ResourceUsage::new();
    let config = SandboxConfig {
        memory_limit_bytes: 1000,
        cpu_time_limit_ms: 100,
        ..Default::default()
    };

    usage.add_memory(500);
    assert!(!usage.exceeds_limits(&config));

    usage.add_memory(600); // Now at 1100, exceeds 1000
    assert!(usage.exceeds_limits(&config));
}

#[test]
fn test_sandbox_state_initial() {
    let state = SandboxState::new(app_id_n(1));

    assert_eq!(state.app_id, app_id_n(1));
    assert!(state.is_running());
}

#[test]
fn test_sandbox_message_types() {
    let start = SandboxMessage::Start {
        config: SandboxConfig::default(),
    };
    let stop = SandboxMessage::Stop;
    let invoke = SandboxMessage::Invoke {
        method: "test".to_string(),
        args: vec![1, 2, 3],
    };

    assert!(matches!(start, SandboxMessage::Start { .. }));
    assert!(matches!(stop, SandboxMessage::Stop));
    assert!(matches!(invoke, SandboxMessage::Invoke { .. }));
}

// ===================== Bot Tests =====================

#[test]
fn test_bot_identity_creation() {
    let bot = BotIdentity::new(app_id_n(1), "TestBot".to_string(), pubkey_n(1));

    assert_eq!(bot.app_id, app_id_n(1));
    assert_eq!(bot.name, "TestBot");
    assert_eq!(bot.owner_pubkey, pubkey_n(1));
}

#[test]
fn test_bot_auth_token_generation() {
    let bot = BotIdentity::new(app_id_n(1), "TestBot".to_string(), pubkey_n(1));

    let token = BotAuthToken::generate(&bot, 3600); // 1 hour validity

    assert!(!token.is_expired());
    assert_eq!(token.bot_id, bot.id);
}

#[test]
fn test_bot_auth_token_expiration() {
    let bot = BotIdentity::new(app_id_n(1), "TestBot".to_string(), pubkey_n(1));

    let token = BotAuthToken::generate(&bot, 0); // Immediate expiry

    // Sleep a tiny bit to ensure expiry
    std::thread::sleep(std::time::Duration::from_millis(10));
    assert!(token.is_expired());
}

#[test]
fn test_bot_permission_types() {
    let read = BotPermission::ReadMessages;
    let send = BotPermission::SendMessages;
    let manage = BotPermission::ManageChannel;

    assert_ne!(read, send);
    assert_ne!(send, manage);
}

#[test]
fn test_bot_command_context() {
    let ctx = BotCommandContext {
        bot_id: app_id_n(1),
        channel_id: Some([1u8; 32]),
        user_id: pubkey_n(2),
        command: "/help".to_string(),
        args: vec!["topic".to_string()],
        timestamp: 12345,
    };

    assert_eq!(ctx.command, "/help");
    assert_eq!(ctx.args.len(), 1);
    assert!(ctx.channel_id.is_some());
}

// ===================== Wallet Tests =====================

#[test]
fn test_wallet_connection_creation() {
    let connection = WalletConnection::new(pubkey_n(1), app_id_n(10));

    assert_eq!(connection.user_pubkey, pubkey_n(1));
    assert_eq!(connection.app_id, app_id_n(10));
    assert!(connection.is_connected());
}

#[test]
fn test_wallet_connection_disconnect() {
    let mut connection = WalletConnection::new(pubkey_n(1), app_id_n(10));

    assert!(connection.is_connected());

    connection.disconnect();

    assert!(!connection.is_connected());
}

#[test]
fn test_sign_request_creation() {
    let request = SignRequest::new(
        pubkey_n(1),
        app_id_n(10),
        vec![1, 2, 3, 4],
        "Sign this message".to_string(),
    );

    assert_eq!(request.user_pubkey, pubkey_n(1));
    assert_eq!(request.app_id, app_id_n(10));
    assert_eq!(request.message, vec![1, 2, 3, 4]);
}

#[test]
fn test_sign_request_visibility() {
    let visible = SignRequest::new_with_visibility(
        pubkey_n(1),
        app_id_n(10),
        vec![1, 2, 3],
        "Test".to_string(),
        TransactionVisibility::Visible,
    );

    let invisible = SignRequest::new_with_visibility(
        pubkey_n(1),
        app_id_n(10),
        vec![1, 2, 3],
        "Test".to_string(),
        TransactionVisibility::Invisible,
    );

    assert!(matches!(visible.visibility, TransactionVisibility::Visible));
    assert!(matches!(
        invisible.visibility,
        TransactionVisibility::Invisible
    ));
}

#[test]
fn test_transaction_visibility_default() {
    let request = SignRequest::new(pubkey_n(1), app_id_n(10), vec![1, 2, 3], "Test".to_string());

    // Default should be visible (user should see what they're signing)
    assert!(matches!(request.visibility, TransactionVisibility::Visible));
}

// ===================== Integration Tests =====================

#[test]
fn test_full_app_registration_flow() {
    // 1. Developer creates account
    let developer = Developer::new(pubkey_n(1), "TestDev".to_string());

    // 2. Developer creates app registration
    let app_id = AppId::generate();
    let mut registration = AppRegistration::new(
        app_id,
        "TestApp".to_string(),
        developer,
        "1.0.0".to_string(),
    );

    // 3. Developer signs registration
    registration.sign(vec![1, 2, 3, 4, 5]);

    // 4. App gets verified (by registry)
    let verified = VerifiedApp::from_registration(registration, 12345);

    assert!(verified.is_some());
}

#[test]
fn test_bot_with_wallet_integration() {
    // 1. Create bot
    let bot = BotIdentity::new(app_id_n(1), "PaymentBot".to_string(), pubkey_n(1));

    // 2. User connects wallet
    let connection = WalletConnection::new(pubkey_n(2), bot.app_id);

    // 3. Bot creates sign request
    let request = SignRequest::new(
        connection.user_pubkey,
        bot.app_id,
        vec![1, 2, 3, 4], // Transaction data
        "Confirm payment of 100 DCHAT".to_string(),
    );

    // 4. Verify request is properly formed
    assert!(request.message.len() > 0);
    assert!(matches!(request.visibility, TransactionVisibility::Visible));
}

/// Property-based tests
#[cfg(feature = "proptest")]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_app_id_unique(seed in any::<u64>()) {
            let id1 = AppId::generate();
            let id2 = AppId::generate();
            prop_assert_ne!(id1.0, id2.0);
        }

        #[test]
        fn prop_resource_usage_monotonic(adds: Vec<u64>) {
            let mut usage = ResourceUsage::new();
            let mut total = 0u64;

            for add in adds.iter().take(10) {
                let add = *add % 10000; // Cap additions
                usage.add_memory(add);
                total = total.saturating_add(add);
                prop_assert!(usage.memory_bytes <= total);
            }
        }

        #[test]
        fn prop_manifest_hash_deterministic(name in "[a-z]{1,20}") {
            let manifest = AppManifest {
                app_id: app_id_n(1),
                name: name.clone(),
                version: "1.0.0".to_string(),
                app_type: AppType::MiniApp,
                entry_point: "main.wasm".to_string(),
                permissions: vec![],
                resource_limits: ResourceLimits::default(),
                developer_pubkey: pubkey_n(1),
                signature: None,
            };

            let hash1 = manifest.compute_hash();
            let hash2 = manifest.compute_hash();
            prop_assert_eq!(hash1, hash2);
        }
    }
}
