//! Sandbox and registry tests for dchat-miniapps
//!
//! Verifies:
//! - App registration and verification
//! - Sandbox resource limits
//! - Bot identity management
//! - Wallet integration

use chrono::Utc;
use dchat_miniapps::bot::{BotAuthToken, BotCommandContext, BotId, BotIdentity};
use dchat_miniapps::manifest::{
    AppCategory, AppManifest, ManifestBuilder, ManifestVersion, ResourceSpec, RuntimeRequirements,
};
use dchat_miniapps::permissions::{Permission, PermissionSet};
use dchat_miniapps::registry::{
    AppId, AppRegistration, Developer, DeveloperId, DeveloperStatus, RegistrationStatus,
    VerifiedApp,
};
use dchat_miniapps::sandbox::{
    ResourceSnapshot, ResourceUsage, SandboxConfig, SandboxMessage, SandboxState,
};
use dchat_miniapps::wallet::{
    ConnectionId, SignRequest, SignRequestType, WalletConnection, WalletContext, WalletVisibility,
};
use ed25519_dalek::Signer;
use ed25519_dalek::SigningKey;
use std::sync::atomic::Ordering;

fn pubkey_n(n: u8) -> [u8; 32] {
    // Create a valid Ed25519 public key for testing
    let mut seed = [0u8; 32];
    seed[0] = n;
    let signing_key = SigningKey::from_bytes(&seed);
    signing_key.verifying_key().to_bytes()
}

fn signing_key_n(n: u8) -> SigningKey {
    let mut seed = [0u8; 32];
    seed[0] = n;
    SigningKey::from_bytes(&seed)
}

fn app_id_n(n: u8) -> AppId {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    AppId::from_bytes(bytes)
}

fn sandbox_id_n(n: u8) -> dchat_miniapps::sandbox::SandboxId {
    dchat_miniapps::sandbox::SandboxId::new()
}

// ===================== Registry Tests =====================

#[test]
fn test_app_id_from_bytes() {
    let bytes = [42u8; 32];
    let id = AppId::from_bytes(bytes);
    assert_eq!(id.0, bytes);
}

#[test]
fn test_app_id_derive() {
    let dev_id = DeveloperId::from_bytes([1u8; 32]);
    let id1 = AppId::derive(&dev_id, "TestApp");
    let id2 = AppId::derive(&dev_id, "TestApp");
    let id3 = AppId::derive(&dev_id, "OtherApp");

    // Same inputs produce same output
    assert_eq!(id1.0, id2.0);
    // Different app name produces different ID
    assert_ne!(id1.0, id3.0);
}

#[test]
fn test_app_id_hex_roundtrip() {
    let id = app_id_n(42);
    let hex = id.to_hex();
    let parsed = AppId::from_hex(&hex).expect("valid hex");
    assert_eq!(id.0, parsed.0);
}

#[test]
fn test_developer_creation() {
    let pubkey = pubkey_n(1);
    let developer = Developer::new("TestDev".to_string(), pubkey);

    assert_eq!(developer.name, "TestDev");
    assert_eq!(developer.public_key, pubkey);
    assert_eq!(developer.status, DeveloperStatus::Pending);
    assert!(!developer.is_verified());
}

#[test]
fn test_developer_id_from_pubkey() {
    let pubkey1 = pubkey_n(1);
    let pubkey2 = pubkey_n(2);

    let dev1 = Developer::new("Dev1".to_string(), pubkey1);
    let dev2 = Developer::new("Dev2".to_string(), pubkey2);
    let dev1_again = Developer::new("Dev1Again".to_string(), pubkey1);

    // Same public key produces same developer ID
    assert_eq!(dev1.id.0, dev1_again.id.0);
    // Different public key produces different ID
    assert_ne!(dev1.id.0, dev2.id.0);
}

#[test]
fn test_developer_can_register_apps() {
    let pubkey = pubkey_n(1);
    let mut developer = Developer::new("TestDev".to_string(), pubkey);

    // Pending developers cannot register apps
    assert!(!developer.can_register_apps());

    // Set to verified
    developer.status = DeveloperStatus::Verified;
    assert!(developer.can_register_apps());

    // Suspended developers cannot register
    developer.status = DeveloperStatus::Suspended;
    assert!(!developer.can_register_apps());

    // Banned developers cannot register
    developer.status = DeveloperStatus::Banned;
    assert!(!developer.can_register_apps());
}

#[test]
fn test_developer_signature_verification() {
    let signing_key = signing_key_n(1);
    let pubkey = signing_key.verifying_key().to_bytes();
    let developer = Developer::new("TestDev".to_string(), pubkey);

    let message = b"test message to sign";
    let signature = signing_key.sign(message);

    // Valid signature should pass
    assert!(developer
        .verify_signature(message, &signature.to_bytes())
        .is_ok());

    // Invalid signature should fail
    let bad_signature = [0u8; 64];
    assert!(developer.verify_signature(message, &bad_signature).is_err());
}

#[test]
fn test_registration_status_variants() {
    let pending = RegistrationStatus::Pending;
    let active = RegistrationStatus::Active;
    let suspended = RegistrationStatus::Suspended;
    let revoked = RegistrationStatus::Revoked;
    let deprecated = RegistrationStatus::Deprecated;

    assert_ne!(pending, active);
    assert_ne!(active, suspended);
    assert_ne!(suspended, revoked);
    assert_ne!(revoked, deprecated);
}

// ===================== Manifest Tests =====================

#[test]
fn test_manifest_version() {
    let version = ManifestVersion::CURRENT;
    assert!(version.is_compatible());

    let old_version = ManifestVersion::new(0, 1);
    assert!(!old_version.is_compatible());
}

#[test]
fn test_manifest_builder() {
    let manifest = ManifestBuilder::new("TestApp", "1.0.0", "A test application")
        .category(AppCategory::Utilities)
        .entry_point("main.html")
        .build()
        .expect("valid manifest");

    assert_eq!(manifest.metadata.name, "TestApp");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.metadata.category, AppCategory::Utilities);
    assert_eq!(manifest.resources.entry_point, "main.html");
}

#[test]
fn test_manifest_json_roundtrip() {
    let manifest = ManifestBuilder::new("TestApp", "1.0.0", "Test desc")
        .build()
        .expect("valid manifest");

    let json = manifest.to_json().expect("serializable");
    let parsed = AppManifest::from_json(&json).expect("parseable");

    assert_eq!(manifest.metadata.name, parsed.metadata.name);
    assert_eq!(manifest.version, parsed.version);
}

#[test]
fn test_app_category_variants() {
    let categories = [
        AppCategory::Games,
        AppCategory::Social,
        AppCategory::Finance,
        AppCategory::Utilities,
        AppCategory::Entertainment,
        AppCategory::Education,
        AppCategory::Productivity,
        AppCategory::Shopping,
        AppCategory::News,
        AppCategory::Health,
        AppCategory::Other,
    ];

    // All categories should be distinct
    for (i, cat1) in categories.iter().enumerate() {
        for (j, cat2) in categories.iter().enumerate() {
            if i != j {
                assert_ne!(std::mem::discriminant(cat1), std::mem::discriminant(cat2));
            }
        }
    }
}

#[test]
fn test_resource_spec_validation() {
    let valid = ResourceSpec {
        entry_point: "index.html".to_string(),
        preload: vec!["style.css".to_string()],
        allowed_domains: vec!["api.example.com".to_string()],
        csp: None,
    };
    assert!(valid.validate().is_ok());

    let invalid_traversal = ResourceSpec {
        entry_point: "../../../etc/passwd".to_string(),
        preload: vec![],
        allowed_domains: vec![],
        csp: None,
    };
    assert!(invalid_traversal.validate().is_err());

    let invalid_domain = ResourceSpec {
        entry_point: "index.html".to_string(),
        preload: vec![],
        allowed_domains: vec!["https://example.com".to_string()],
        csp: None,
    };
    assert!(invalid_domain.validate().is_err());
}

#[test]
fn test_runtime_requirements_default() {
    let requirements = RuntimeRequirements::default();

    assert!(requirements.max_memory_mb > 0);
    assert!(requirements.max_cpu_time_ms > 0);
    assert!(!requirements.requires_keyboard);
    assert!(!requirements.requires_touch);
}

#[test]
fn test_manifest_content_hash_deterministic() {
    let manifest = ManifestBuilder::new("TestApp", "1.0.0", "Test")
        .build()
        .expect("valid manifest");

    let hash1 = manifest.content_hash();
    let hash2 = manifest.content_hash();

    assert_eq!(hash1, hash2);
}

#[test]
fn test_manifest_content_hash_changes() {
    let manifest1 = ManifestBuilder::new("TestApp1", "1.0.0", "Test")
        .build()
        .expect("valid manifest");

    let manifest2 = ManifestBuilder::new("TestApp2", "1.0.0", "Test")
        .build()
        .expect("valid manifest");

    assert_ne!(manifest1.content_hash(), manifest2.content_hash());
}

// ===================== Sandbox Tests =====================

#[test]
fn test_sandbox_config_default() {
    let config = SandboxConfig::default();

    assert!(config.max_memory > 0);
    assert!(config.max_cpu_time_ms > 0);
    assert!(config.allow_network);
    assert!(config.allowed_domains.is_empty());
    assert!(!config.debug_mode);
}

#[test]
fn test_sandbox_config_from_manifest() {
    let mut perms = PermissionSet::default();
    perms.insert(Permission::Network);

    let manifest = ManifestBuilder::new("TestApp", "1.0.0", "Test")
        .permission(Permission::Network)
        .allowed_domain("api.example.com")
        .build()
        .expect("valid manifest");

    let config = SandboxConfig::from_manifest(&manifest);

    assert!(config.allow_network);
    assert!(config
        .allowed_domains
        .contains(&"api.example.com".to_string()));
}

#[test]
fn test_resource_usage_tracking() {
    let usage = ResourceUsage::new();

    assert_eq!(usage.memory_used.load(Ordering::Relaxed), 0);
    assert_eq!(usage.cpu_time_used.load(Ordering::Relaxed), 0);
    assert_eq!(usage.network_requests.load(Ordering::Relaxed), 0);

    usage.record_memory(1000);
    usage.record_cpu_time(50);
    usage.record_network_request();

    assert_eq!(usage.memory_used.load(Ordering::Relaxed), 1000);
    assert_eq!(usage.cpu_time_used.load(Ordering::Relaxed), 50);
    assert_eq!(usage.network_requests.load(Ordering::Relaxed), 1);
}

#[test]
fn test_resource_usage_snapshot() {
    let usage = ResourceUsage::new();
    usage.record_memory(2048);
    usage.record_cpu_time(100);
    usage.record_network_request();
    usage.record_network_request();

    let snapshot = usage.snapshot();

    assert_eq!(snapshot.memory_used, 2048);
    assert_eq!(snapshot.cpu_time_used, 100);
    assert_eq!(snapshot.network_requests, 2);
}

#[test]
fn test_resource_limit_checking() {
    let usage = ResourceUsage::new();
    let config = SandboxConfig {
        max_memory: 1000,
        max_cpu_time_ms: 100,
        ..Default::default()
    };

    // Under limit
    usage.record_memory(500);
    assert!(usage.check_memory(config.max_memory).is_ok());

    // Over limit
    usage.record_memory(600);
    assert!(usage.check_memory(config.max_memory).is_err());
}

#[test]
fn test_sandbox_state_variants() {
    let states = [
        SandboxState::Initializing,
        SandboxState::Ready,
        SandboxState::Running,
        SandboxState::Paused,
        SandboxState::Terminated,
        SandboxState::Crashed,
    ];

    // All states should be distinct
    for (i, s1) in states.iter().enumerate() {
        for (j, s2) in states.iter().enumerate() {
            if i != j {
                assert_ne!(s1, s2);
            }
        }
    }
}

#[test]
fn test_sandbox_message_init() {
    let msg = SandboxMessage::Init {
        app_id: "test-app-123".to_string(),
        config: serde_json::json!({"debug": true}),
    };

    assert!(matches!(msg, SandboxMessage::Init { .. }));

    // Test JSON serialization
    let json = msg.to_json().expect("serializable");
    let parsed = SandboxMessage::from_json(&json).expect("parseable");
    assert!(matches!(parsed, SandboxMessage::Init { .. }));
}

#[test]
fn test_sandbox_message_invoke() {
    let msg = SandboxMessage::InvokeMethod {
        method: "doSomething".to_string(),
        params: serde_json::json!({"arg1": 42}),
        id: "req-001".to_string(),
    };

    if let SandboxMessage::InvokeMethod { method, id, .. } = &msg {
        assert_eq!(method, "doSomething");
        assert_eq!(id, "req-001");
    } else {
        panic!("wrong message type");
    }
}

#[test]
fn test_sandbox_message_type_names() {
    let init = SandboxMessage::Init {
        app_id: "test".to_string(),
        config: serde_json::json!(null),
    };
    assert_eq!(init.type_name(), "init");

    let ready = SandboxMessage::Ready;
    assert_eq!(ready.type_name(), "ready");

    let terminate = SandboxMessage::Terminate {
        reason: "test".to_string(),
    };
    assert_eq!(terminate.type_name(), "terminate");
}

#[test]
fn test_sandbox_message_lifecycle() {
    let lifecycle_messages = vec![
        SandboxMessage::Init {
            app_id: "test".to_string(),
            config: serde_json::json!(null),
        },
        SandboxMessage::Ready,
        SandboxMessage::Terminate {
            reason: "done".to_string(),
        },
        SandboxMessage::Terminated { code: 0 },
    ];

    for msg in lifecycle_messages {
        let json = msg.to_json().expect("serializable");
        let parsed = SandboxMessage::from_json(&json).expect("parseable");
        assert_eq!(msg.type_name(), parsed.type_name());
    }
}

// ===================== Bot Tests =====================

#[test]
fn test_bot_id_creation() {
    let bytes = [1u8; 32];
    let id = BotId::from_bytes(bytes);
    assert_eq!(id.as_bytes(), &bytes);
}

#[test]
fn test_bot_id_from_app_id() {
    let app_id = app_id_n(42);
    let bot_id = BotId::from_app_id(&app_id);
    assert_eq!(bot_id.0, app_id.0);
}

#[test]
fn test_bot_identity_creation() {
    let app_id = app_id_n(1);
    let pubkey = pubkey_n(1);

    let bot = BotIdentity::new(
        app_id,
        "test_bot".to_string(),
        "Test Bot".to_string(),
        pubkey,
    )
    .expect("valid bot identity");

    assert_eq!(bot.app_id.0, app_id.0);
    assert_eq!(bot.username, "test_bot");
    assert_eq!(bot.display_name, "Test Bot");
    assert_eq!(bot.public_key, pubkey);
    assert!(!bot.is_verified);
}

#[test]
fn test_bot_identity_username_validation() {
    let app_id = app_id_n(1);
    let pubkey = pubkey_n(1);

    // Too short
    let result = BotIdentity::new(app_id, "ab".to_string(), "Display".to_string(), pubkey);
    assert!(result.is_err());

    // Invalid characters
    let result = BotIdentity::new(
        app_id,
        "bot with spaces".to_string(),
        "Display".to_string(),
        pubkey,
    );
    assert!(result.is_err());

    // Valid with underscores
    let result = BotIdentity::new(
        app_id,
        "test_bot_123".to_string(),
        "Display".to_string(),
        pubkey,
    );
    assert!(result.is_ok());
}

#[test]
fn test_bot_identity_builder_pattern() {
    let app_id = app_id_n(1);
    let pubkey = pubkey_n(1);

    let bot = BotIdentity::new(
        app_id,
        "test_bot".to_string(),
        "Test Bot".to_string(),
        pubkey,
    )
    .expect("valid bot")
    .with_description("A helpful test bot".to_string())
    .with_avatar("https://example.com/avatar.png".to_string());

    assert_eq!(bot.description, "A helpful test bot");
    assert_eq!(
        bot.avatar_url,
        Some("https://example.com/avatar.png".to_string())
    );
}

#[test]
fn test_bot_auth_token_creation() {
    let app_id = app_id_n(1);
    let signing_key = signing_key_n(1);
    let pubkey = signing_key.verifying_key().to_bytes();
    let bot_id = BotId::from_app_id(&app_id);

    let token = BotAuthToken::create(
        bot_id,
        &signing_key,
        Some("channel123".to_string()),
        None,
        chrono::Duration::hours(1),
    );

    assert_eq!(token.bot_id.0, bot_id.0);
    assert!(!token.is_expired());
    assert_eq!(token.channel_id, Some("channel123".to_string()));
}

#[test]
fn test_bot_auth_token_verification() {
    let signing_key = signing_key_n(1);
    let verifying_key = signing_key.verifying_key();
    let bot_id = BotId::from_bytes([1u8; 32]);

    let token = BotAuthToken::create(bot_id, &signing_key, None, None, chrono::Duration::hours(1));

    // Valid verification
    assert!(token.verify(&verifying_key).is_ok());

    // Wrong key should fail
    let other_key = signing_key_n(2);
    assert!(token.verify(&other_key.verifying_key()).is_err());
}

#[test]
fn test_bot_command_context() {
    let ctx = BotCommandContext {
        command: "help".to_string(),
        args: vec!["topic".to_string()],
        raw_args: "topic".to_string(),
        user_id: "user123".to_string(),
        channel_id: "channel456".to_string(),
        message_id: "msg789".to_string(),
        timestamp: Utc::now(),
        reply_to: None,
    };

    assert_eq!(ctx.command, "help");
    assert_eq!(ctx.args.len(), 1);
    assert_eq!(ctx.args[0], "topic");
}

// ===================== Wallet Tests =====================

#[test]
fn test_connection_id_uniqueness() {
    let id1 = ConnectionId::new();
    let id2 = ConnectionId::new();
    assert_ne!(id1.0, id2.0);
}

#[test]
fn test_wallet_visibility_variants() {
    let hidden = WalletVisibility::Hidden;
    let balance_only = WalletVisibility::BalanceOnly;
    let full = WalletVisibility::Full;
    let invisible = WalletVisibility::Invisible;

    assert_ne!(hidden, balance_only);
    assert_ne!(balance_only, full);
    assert_ne!(full, invisible);

    // Default should be hidden
    assert_eq!(WalletVisibility::default(), WalletVisibility::Hidden);
}

#[test]
fn test_wallet_connection_creation() {
    let app_id = app_id_n(1);
    let sandbox_id = sandbox_id_n(1);
    let pubkey = pubkey_n(2);

    let connection = WalletConnection::new(app_id, sandbox_id, pubkey, "dchat-mainnet".to_string());

    assert_eq!(connection.app_id.0, app_id.0);
    assert_eq!(connection.user_public_key, pubkey);
    assert_eq!(connection.chain_id, "dchat-mainnet");
    assert_eq!(connection.visibility, WalletVisibility::Hidden);
    assert!(connection.session_permissions.is_empty());
}

#[test]
fn test_wallet_connection_with_visibility() {
    let app_id = app_id_n(1);
    let sandbox_id = sandbox_id_n(1);
    let pubkey = pubkey_n(2);

    let connection = WalletConnection::new(app_id, sandbox_id, pubkey, "dchat-testnet".to_string())
        .with_visibility(WalletVisibility::Full);

    assert_eq!(connection.visibility, WalletVisibility::Full);
}

#[test]
fn test_wallet_connection_permissions() {
    let app_id = app_id_n(1);
    let sandbox_id = sandbox_id_n(1);
    let pubkey = pubkey_n(2);

    let mut connection =
        WalletConnection::new(app_id, sandbox_id, pubkey, "dchat-testnet".to_string());

    assert!(!connection.has_permission(&Permission::SendTokens));

    connection.add_permission(Permission::SendTokens);
    assert!(connection.has_permission(&Permission::SendTokens));

    // Adding same permission twice should not duplicate
    connection.add_permission(Permission::SendTokens);
    assert_eq!(connection.session_permissions.len(), 1);
}

#[test]
fn test_wallet_connection_staleness() {
    let app_id = app_id_n(1);
    let sandbox_id = sandbox_id_n(1);
    let pubkey = pubkey_n(2);

    let connection = WalletConnection::new(app_id, sandbox_id, pubkey, "dchat-testnet".to_string());

    // Should not be stale with reasonable timeout
    assert!(!connection.is_stale(chrono::Duration::hours(1)));

    // Should be stale with zero timeout
    assert!(connection.is_stale(chrono::Duration::zero()));
}

#[test]
fn test_wallet_context_from_connection() {
    let app_id = app_id_n(1);
    let sandbox_id = sandbox_id_n(1);
    let pubkey = pubkey_n(2);

    let connection = WalletConnection::new(app_id, sandbox_id, pubkey, "dchat-mainnet".to_string())
        .with_visibility(WalletVisibility::BalanceOnly);

    let context = WalletContext::from_connection(&connection);

    assert_eq!(context.connection_id, connection.id);
    assert_eq!(context.chain_id, "dchat-mainnet");
    assert_eq!(context.visibility, WalletVisibility::BalanceOnly);
    assert!(context.is_connected);
    assert_eq!(context.balance, 0);
}

#[test]
fn test_wallet_context_with_balances() {
    let app_id = app_id_n(1);
    let sandbox_id = sandbox_id_n(1);
    let pubkey = pubkey_n(2);

    let connection = WalletConnection::new(app_id, sandbox_id, pubkey, "dchat-mainnet".to_string());

    let context = WalletContext::from_connection(&connection)
        .with_balance(1_000_000_000)
        .with_token_balance("USDC".to_string(), 500_000);

    assert_eq!(context.balance, 1_000_000_000);
    assert_eq!(context.token_balances.get("USDC"), Some(&500_000));
}

#[test]
fn test_sign_request_message() {
    let conn_id = ConnectionId::new();
    let message = b"Sign this message".to_vec();

    let request = SignRequest::message(
        conn_id,
        message.clone(),
        "Please sign this message".to_string(),
    );

    assert_eq!(request.connection_id, conn_id);
    assert!(matches!(request.request_type, SignRequestType::Message));
    assert_eq!(request.message, Some(message));
    assert!(request.intent.is_none());
    assert!(request.requires_approval);
    assert!(!request.is_expired());
}

#[test]
fn test_sign_request_approval_setting() {
    let conn_id = ConnectionId::new();

    let request =
        SignRequest::message(conn_id, b"test".to_vec(), "Test".to_string()).with_approval(false);

    assert!(!request.requires_approval);
}

#[test]
fn test_sign_request_signing_payload() {
    let conn_id = ConnectionId::new();
    let message = b"Test message".to_vec();

    let request = SignRequest::message(conn_id, message.clone(), "Test".to_string());

    let payload = request.signing_payload().expect("should get payload");
    assert_eq!(payload, message);
}

// ===================== Integration Tests =====================

#[test]
fn test_developer_to_app_flow() {
    // 1. Developer registers
    let signing_key = signing_key_n(1);
    let pubkey = signing_key.verifying_key().to_bytes();
    let mut developer = Developer::new("TestDev".to_string(), pubkey);

    // 2. Developer gets verified
    developer.status = DeveloperStatus::Verified;
    assert!(developer.can_register_apps());

    // 3. Developer creates manifest
    let manifest = ManifestBuilder::new("TestApp", "1.0.0", "A test app")
        .category(AppCategory::Utilities)
        .permission(Permission::ReadProfile)
        .build()
        .expect("valid manifest");

    // 4. App ID is derived from developer and app name
    let app_id = AppId::derive(&developer.id, &manifest.metadata.name);

    // The app ID should be deterministic
    let app_id2 = AppId::derive(&developer.id, &manifest.metadata.name);
    assert_eq!(app_id.0, app_id2.0);
}

#[test]
fn test_sandbox_resource_lifecycle() {
    // 1. Create config from manifest
    let manifest = ManifestBuilder::new("TestApp", "1.0.0", "Test")
        .permission(Permission::Network)
        .allowed_domain("api.test.com")
        .build()
        .expect("valid manifest");

    let config = SandboxConfig::from_manifest(&manifest);
    assert!(config.allow_network);

    // 2. Track resource usage
    let usage = ResourceUsage::new();

    // 3. Record some operations
    usage.record_memory(1024 * 1024);
    usage.record_cpu_time(100);
    usage.record_network_request();

    // 4. Check limits
    assert!(usage.check_memory(config.max_memory).is_ok());
    assert!(usage.check_cpu_time(config.max_cpu_time_ms).is_ok());

    // 5. Snapshot for reporting
    let snapshot = usage.snapshot();
    assert!(snapshot.memory_used > 0);
}

#[test]
fn test_bot_with_wallet_integration() {
    // 1. Create app and bot
    let app_id = app_id_n(1);
    let signing_key = signing_key_n(1);
    let pubkey = signing_key.verifying_key().to_bytes();

    let bot = BotIdentity::new(
        app_id,
        "payment_bot".to_string(),
        "Payment Bot".to_string(),
        pubkey,
    )
    .expect("valid bot");

    // 2. Create auth token
    let bot_id = BotId::from_app_id(&bot.app_id);
    let token = BotAuthToken::create(
        bot_id,
        &signing_key,
        Some("channel123".to_string()),
        None,
        chrono::Duration::hours(1),
    );
    assert!(!token.is_expired());

    // 3. User connects wallet
    let user_pubkey = pubkey_n(2);
    let sandbox_id = sandbox_id_n(1);
    let mut connection = WalletConnection::new(
        bot.app_id,
        sandbox_id,
        user_pubkey,
        "dchat-mainnet".to_string(),
    );

    // 4. Grant permission for transaction
    connection.add_permission(Permission::SendTokens);
    assert!(connection.has_permission(&Permission::SendTokens));

    // 5. Create sign request
    let request = SignRequest::message(
        connection.id,
        b"Transfer 100 DCHAT to merchant".to_vec(),
        "Confirm payment of 100 DCHAT".to_string(),
    );

    assert!(request.requires_approval);
    assert!(!request.is_expired());
}

#[test]
fn test_sandbox_message_flow() {
    // Simulate message exchange between host and sandbox
    let messages = vec![
        // Host -> Sandbox: Initialize
        SandboxMessage::Init {
            app_id: "app123".to_string(),
            config: serde_json::json!({"debug": false}),
        },
        // Sandbox -> Host: Ready
        SandboxMessage::Ready,
        // Host -> Sandbox: Send data
        SandboxMessage::SendData {
            payload: serde_json::json!({"user_id": "user123"}),
        },
        // Sandbox -> Host: Request permission
        SandboxMessage::PermissionRequest {
            permissions: vec!["send_tokens".to_string()],
        },
        // Host -> Sandbox: Grant permission
        SandboxMessage::PermissionResponse {
            granted: vec!["send_tokens".to_string()],
            denied: vec![],
        },
        // Sandbox -> Host: Invoke wallet method
        SandboxMessage::InvokeMethod {
            method: "signTransaction".to_string(),
            params: serde_json::json!({"amount": 100}),
            id: "tx001".to_string(),
        },
        // Host -> Sandbox: Method result
        SandboxMessage::MethodResult {
            id: "tx001".to_string(),
            result: serde_json::json!({"signature": "0xabc..."}),
            error: None,
        },
        // Host -> Sandbox: Terminate
        SandboxMessage::Terminate {
            reason: "user_closed".to_string(),
        },
        // Sandbox -> Host: Terminated
        SandboxMessage::Terminated { code: 0 },
    ];

    // All messages should serialize and deserialize correctly
    for msg in &messages {
        let json = msg.to_json().expect("should serialize");
        let parsed = SandboxMessage::from_json(&json).expect("should parse");
        assert_eq!(msg.type_name(), parsed.type_name());
    }
}

/// Property-based tests
#[cfg(feature = "proptest")]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_app_id_derive_deterministic(
            name in "[a-z]{1,20}"
        ) {
            let dev_id = DeveloperId::from_bytes([42u8; 32]);
            let id1 = AppId::derive(&dev_id, &name);
            let id2 = AppId::derive(&dev_id, &name);
            prop_assert_eq!(id1.0, id2.0);
        }

        #[test]
        fn prop_resource_usage_monotonic(adds: Vec<u64>) {
            let usage = ResourceUsage::new();
            let mut total = 0u64;

            for add in adds.iter().take(10) {
                let add = *add % 10000;
                usage.record_memory(add);
                total = total.saturating_add(add);
                prop_assert!(usage.memory_used.load(Ordering::Relaxed) <= total);
            }
        }

        #[test]
        fn prop_manifest_hash_deterministic(name in "[a-zA-Z]{1,20}") {
            let manifest = ManifestBuilder::new(&name, "1.0.0", "Test")
                .build()
                .unwrap();

            let hash1 = manifest.content_hash();
            let hash2 = manifest.content_hash();
            prop_assert_eq!(hash1, hash2);
        }

        #[test]
        fn prop_sandbox_message_json_roundtrip(payload in "[a-zA-Z0-9]{0,100}") {
            let msg = SandboxMessage::SendData {
                payload: serde_json::json!({"data": payload}),
            };

            let json = msg.to_json().unwrap();
            let parsed = SandboxMessage::from_json(&json).unwrap();

            if let SandboxMessage::SendData { payload: p } = parsed {
                prop_assert!(p.is_object());
            } else {
                prop_assert!(false, "wrong message type");
            }
        }
    }
}
