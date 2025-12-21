//! Integration tests for dchat-miniapps
//!
//! End-to-end tests covering the full mini-app lifecycle.

use dchat_miniapps::bot::{BotAuthToken, BotId, BotIdentity};
use dchat_miniapps::intent::{IntentId, IntentPayload, IntentType};
use dchat_miniapps::manifest::{AppCategory, ManifestBuilder};
use dchat_miniapps::permissions::{Permission, PermissionGrant, RiskLevel};
use dchat_miniapps::receipt::{Attestation, ExecutionResult, Receipt, ReceiptStatus, SignerSet};
use dchat_miniapps::registry::{AppId, Developer, DeveloperStatus};
use dchat_miniapps::sandbox::{SandboxConfig, SandboxMessage};
use dchat_miniapps::wallet::{SignRequest, WalletConnection, WalletVisibility};
use ed25519_dalek::{Signer, SigningKey};

fn signing_key_n(n: u8) -> SigningKey {
    let mut seed = [0u8; 32];
    seed[0] = n;
    SigningKey::from_bytes(&seed)
}

fn pubkey_n(n: u8) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[0] = n;
    let signing_key = SigningKey::from_bytes(&seed);
    signing_key.verifying_key().to_bytes()
}

fn app_id_n(n: u8) -> AppId {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    AppId::from_bytes(bytes)
}

fn tx_hash_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

fn block_hash_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = n;
    bytes
}

// ===================== Full Flow Integration Tests =====================

#[test]
fn test_developer_registration_to_app_launch() {
    // 1. Developer registers
    let signing_key = signing_key_n(1);
    let pubkey = signing_key.verifying_key().to_bytes();
    let mut developer = Developer::new("GameStudio".to_string(), pubkey);

    // Initially pending
    assert!(!developer.can_register_apps());

    // 2. Developer gets verified (simulating admin action)
    developer.status = DeveloperStatus::Verified;
    assert!(developer.can_register_apps());

    // 3. Developer creates app manifest
    let manifest = ManifestBuilder::new("SuperGame", "1.0.0", "An awesome game mini-app")
        .category(AppCategory::Games)
        .permission(Permission::ReadProfile)
        .permission(Permission::ViewBalance)
        .entry_point("game.html")
        .build()
        .expect("valid manifest");

    // 4. App ID derived from developer + app name
    let app_id = AppId::derive(&developer.id, &manifest.metadata.name);

    // 5. Verify determinism
    let app_id2 = AppId::derive(&developer.id, &manifest.metadata.name);
    assert_eq!(app_id.0, app_id2.0);

    // 6. Create sandbox config from manifest
    let sandbox_config = SandboxConfig::from_manifest(&manifest);
    assert!(sandbox_config.max_memory > 0);
}

#[test]
fn test_bot_authentication_flow() {
    // 1. Create bot identity
    let signing_key = signing_key_n(1);
    let pubkey = signing_key.verifying_key().to_bytes();
    let app_id = app_id_n(1);

    let bot = BotIdentity::new(
        app_id,
        "payment_bot".to_string(),
        "Payment Bot".to_string(),
        pubkey,
    )
    .expect("valid bot");

    // 2. Generate auth token
    let bot_id = BotId::from_app_id(&app_id);
    let token = BotAuthToken::create(
        bot_id,
        &signing_key,
        Some("channel_123".to_string()),
        Some("user_456".to_string()),
        chrono::Duration::hours(1),
    );

    // 3. Verify token is valid
    assert!(!token.is_expired());

    // 4. Verify with correct key
    let verifying_key = signing_key.verifying_key();
    assert!(token.verify(&verifying_key).is_ok());

    // 5. Verify rejects wrong key
    let wrong_key = signing_key_n(2);
    assert!(token.verify(&wrong_key.verifying_key()).is_err());
}

#[test]
fn test_wallet_connection_and_signing() {
    // 1. User opens mini-app
    let app_id = app_id_n(1);
    let sandbox_id = dchat_miniapps::sandbox::SandboxId::new();
    let user_pubkey = pubkey_n(100);

    // 2. Connect wallet
    let mut connection =
        WalletConnection::new(app_id, sandbox_id, user_pubkey, "dchat-mainnet".to_string());

    assert_eq!(connection.visibility, WalletVisibility::Hidden);
    assert!(connection.session_permissions.is_empty());

    // 3. Grant permission for token transfer
    connection.add_permission(Permission::SendTokens);
    assert!(connection.has_permission(&Permission::SendTokens));

    // 4. Create sign request
    let sign_request = SignRequest::message(
        connection.id,
        b"Transfer 100 DCHAT to 0xabc...".to_vec(),
        "Confirm payment of 100 DCHAT".to_string(),
    );

    assert!(sign_request.requires_approval);
    assert!(!sign_request.is_expired());

    // 5. Get signing payload
    let payload = sign_request.signing_payload().expect("valid payload");
    assert!(!payload.is_empty());
}

#[test]
fn test_intent_creation_to_receipt() {
    // 1. Create transfer intent
    let sender = pubkey_n(1);
    let recipient = pubkey_n(2);

    let intent_type = IntentType::Transfer {
        recipient,
        mint: None,        // Native token
        amount: 1_000_000, // 1 DCHAT
        memo: Some("Test payment".to_string()),
    };

    let intent = IntentPayload::new(
        sender,
        intent_type,
        10_000, // max fee
        1,      // chain_id
    );

    // 2. Create signer set and add sender
    let mut signers = SignerSet::new(50, 1); // 50% threshold, min 1 attestation
    signers.add_signer(sender, 10);

    // 3. Create intent ID
    let intent_id = IntentId::new();

    // 4. Create execution result
    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec![
            "Validate intent and signatures".to_string(),
            "Verify sender has sufficient balance".to_string(),
            "Execute the transfer".to_string(),
            "Record transaction on chain".to_string(),
        ],
        account_changes: vec![],
        compute_units: 5000,
        fee: 100,
    };

    // 5. Create receipt from execution
    let mut receipt = Receipt::new(intent_id, result, 12345, tx_hash_n(1), block_hash_n(1));

    // 6. Verify receipt structure
    assert_eq!(receipt.status, ReceiptStatus::Pending);
    assert_eq!(receipt.intent_id, intent_id);

    // 7. Confirm and finalize
    receipt.confirm();
    assert_eq!(receipt.status, ReceiptStatus::Confirmed);

    receipt.finalize();
    assert_eq!(receipt.status, ReceiptStatus::Finalized);
}

#[test]
fn test_sandbox_message_lifecycle() {
    // Simulate full sandbox lifecycle via messages
    let messages: Vec<SandboxMessage> = vec![
        // 1. Initialize sandbox
        SandboxMessage::Init {
            app_id: "test-app".to_string(),
            config: serde_json::json!({
                "debug": false,
                "theme": "dark"
            }),
        },
        // 2. Sandbox ready
        SandboxMessage::Ready,
        // 3. User interaction: viewport change
        SandboxMessage::ViewportChange {
            width: 375,
            height: 812,
        },
        // 4. Theme change
        SandboxMessage::ThemeChange {
            theme: "dark".to_string(),
        },
        // 5. App requests permission
        SandboxMessage::PermissionRequest {
            permissions: vec!["send_tokens".to_string(), "view_balance".to_string()],
        },
        // 6. Host grants permissions
        SandboxMessage::PermissionResponse {
            granted: vec!["view_balance".to_string()],
            denied: vec!["send_tokens".to_string()],
        },
        // 7. App invokes method
        SandboxMessage::InvokeMethod {
            method: "getBalance".to_string(),
            params: serde_json::json!({}),
            id: "req-001".to_string(),
        },
        // 8. Host returns result
        SandboxMessage::MethodResult {
            id: "req-001".to_string(),
            result: serde_json::json!({"balance": 1000000}),
            error: None,
        },
        // 9. User clicks back
        SandboxMessage::BackButton,
        // 10. App terminates
        SandboxMessage::Terminate {
            reason: "user_back".to_string(),
        },
        // 11. Sandbox terminated
        SandboxMessage::Terminated { code: 0 },
    ];

    // All messages should serialize/deserialize correctly
    for msg in &messages {
        let json = msg.to_json().expect("should serialize");
        let parsed = SandboxMessage::from_json(&json).expect("should parse");
        assert_eq!(msg.type_name(), parsed.type_name());
    }
}

#[test]
fn test_permission_flow_for_payment_app() {
    // 1. App declares required permissions in manifest
    let manifest = ManifestBuilder::new("PaymentApp", "1.0.0", "Send and receive payments")
        .category(AppCategory::Finance)
        .permission(Permission::ReadProfile)
        .permission(Permission::ViewBalance)
        .permission(Permission::SendTokens)
        .build()
        .expect("valid manifest");

    // 2. Check risk levels
    assert_eq!(Permission::ReadProfile.risk_level(), RiskLevel::Medium);
    assert_eq!(Permission::ViewBalance.risk_level(), RiskLevel::Medium);
    assert_eq!(Permission::SendTokens.risk_level(), RiskLevel::Critical);

    // 3. Create permission grant for user session
    let grant = PermissionGrant::new(
        app_id_n(1),
        pubkey_n(1),
        Permission::SendTokens,
        dchat_miniapps::permissions::PermissionScope::unrestricted(),
        Some(chrono::Duration::hours(1)),
    );

    assert!(grant.is_valid());
    assert_eq!(grant.permission, Permission::SendTokens);
}

#[test]
fn test_receipt_attestation_flow() {
    // Test receipt creation and attestation by multiple signers
    let intent_id = IntentId::new();

    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec!["Transaction executed".to_string()],
        account_changes: vec![],
        compute_units: 5000,
        fee: 100,
    };

    let receipt = Receipt::new(intent_id, result, 12345, tx_hash_n(1), block_hash_n(1));

    let receipt_hash = receipt.hash();

    // Multiple validators attest to the receipt
    let attestations: Vec<_> = (1..=5)
        .map(|i| {
            let key = signing_key_n(i);
            Attestation::create(&key, receipt_hash, 1000 * i as u64)
        })
        .collect();

    // All attestations should verify
    for att in &attestations {
        assert!(att.verify().is_ok());
    }

    // Calculate total stake
    let total_stake: u64 = attestations.iter().map(|a| a.stake).sum();
    assert_eq!(total_stake, 15000); // 1000 + 2000 + 3000 + 4000 + 5000
}

#[test]
fn test_full_mini_app_payment_flow() {
    // Complete payment flow from app launch to receipt

    // 1. Developer and app setup
    let dev_key = signing_key_n(1);
    let dev_pubkey = dev_key.verifying_key().to_bytes();
    let mut developer = Developer::new("PaymentCo".to_string(), dev_pubkey);
    developer.status = DeveloperStatus::Verified;

    let manifest = ManifestBuilder::new("PayApp", "1.0.0", "Payments")
        .category(AppCategory::Finance)
        .permission(Permission::SendTokens)
        .build()
        .expect("valid manifest");

    let app_id = AppId::derive(&developer.id, &manifest.metadata.name);

    // 2. User connects wallet
    let user_pubkey = pubkey_n(100);
    let sandbox_id = dchat_miniapps::sandbox::SandboxId::new();
    let mut connection =
        WalletConnection::new(app_id, sandbox_id, user_pubkey, "dchat-mainnet".to_string());
    connection.add_permission(Permission::SendTokens);

    // 3. Create payment intent
    let intent_type = IntentType::Transfer {
        recipient: pubkey_n(200),
        mint: None, // Native token
        amount: 500_000,
        memo: Some("Purchase".to_string()),
    };

    let intent = IntentPayload::new(
        user_pubkey,
        intent_type,
        5000,
        1, // dchat-mainnet chain_id
    );

    let intent_id = IntentId::new();

    // 4. Create sign request
    let sign_request = SignRequest::message(
        connection.id,
        intent.hash().to_vec(),
        "Confirm payment of 0.5 DCHAT".to_string(),
    );

    assert!(!sign_request.is_expired());

    // 5. Execute and create receipt
    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec!["Payment completed".to_string()],
        account_changes: vec![],
        compute_units: 3000,
        fee: 100,
    };

    let mut receipt = Receipt::new(intent_id, result, 99999, tx_hash_n(42), block_hash_n(42));

    // 6. Attestation and finalization
    let receipt_hash = receipt.hash();
    let validator_key = signing_key_n(1);
    let attestation = Attestation::create(&validator_key, receipt_hash, 10000);

    assert!(attestation.verify().is_ok());

    receipt.confirm();
    receipt.finalize();

    assert_eq!(receipt.status, ReceiptStatus::Finalized);
}
