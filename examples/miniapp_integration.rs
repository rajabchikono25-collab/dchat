//! Mini-App Integration Example
//!
//! This example demonstrates how to integrate dchat mini-apps into your application.
//! It covers the full lifecycle: developer registration, app creation, launching,
//! permission handling, and wallet integration.
//!
//! Run with: cargo run --example miniapp_integration

use chrono::Utc;
use dchat_miniapps::{
    bot::{BotAuthToken, BotId, BotIdentity},
    intent::{IntentId, IntentPayload, IntentType},
    manifest::{AppCategory, ManifestBuilder},
    permissions::{Permission, PermissionGrant, PermissionManager, PermissionSet, RiskLevel},
    receipt::{Attestation, ExecutionResult, Receipt, ReceiptStatus, SignerSet},
    registry::{AppId, Developer, DeveloperStatus, MiniAppRegistry},
    sandbox::{SandboxConfig, SandboxContext, SandboxId, SandboxMessage, SandboxState},
    wallet::{SignRequest, WalletConnection, WalletVisibility},
};
use ed25519_dalek::{Signer, SigningKey};
use std::collections::HashMap;
use uuid::Uuid;

fn main() {
    println!("═══════════════════════════════════════════════════════════════════");
    println!("              DCHAT MINI-APP INTEGRATION EXAMPLE");
    println!("═══════════════════════════════════════════════════════════════════\n");

    // Run all examples
    example_1_developer_registration();
    example_2_create_app_manifest();
    example_3_launch_sandbox();
    example_4_handle_permissions();
    example_5_wallet_integration();
    example_6_intent_flow();
    example_7_bot_integration();

    println!("\n═══════════════════════════════════════════════════════════════════");
    println!("                    ALL EXAMPLES COMPLETED!");
    println!("═══════════════════════════════════════════════════════════════════");
}

/// Example 1: Developer Registration
///
/// Before publishing mini-apps, developers must register their identity
/// on the chat chain. This example shows how to create a developer profile.
fn example_1_developer_registration() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ Example 1: Developer Registration                               │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    // Generate developer keypair (in production, load from secure storage)
    let signing_key = generate_signing_key(1);
    let public_key = signing_key.verifying_key().to_bytes();

    // Create developer profile
    let mut developer = Developer::new("GameStudio".to_string(), public_key);
    developer.website = Some("https://gamestudio.example.com".to_string());
    developer.email = Some("dev@gamestudio.example.com".to_string());

    println!("📝 Developer Created:");
    println!("   Name:       {}", developer.name);
    println!("   ID:         {}", developer.id);
    println!("   Status:     {:?}", developer.status);
    println!("   Can Publish: {}", developer.can_register_apps());

    // Simulate verification (in production, this happens via governance)
    developer.status = DeveloperStatus::Verified;
    developer.verified_at = Some(Utc::now());

    println!("\n✅ After Verification:");
    println!("   Status:     {:?}", developer.status);
    println!("   Can Publish: {}", developer.can_register_apps());
    println!();
}

/// Example 2: Create an App Manifest
///
/// Every mini-app needs a manifest that defines its metadata, permissions,
/// and runtime requirements.
fn example_2_create_app_manifest() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ Example 2: Create App Manifest                                  │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    // Use the ManifestBuilder for easy manifest creation
    let manifest = ManifestBuilder::new(
        "SuperGame",
        "1.0.0",
        "An awesome multiplayer game for dchat users",
    )
    .category(AppCategory::Games)
    .tag("multiplayer")
    .tag("arcade")
    // Request permissions the app needs
    .permission(Permission::ReadProfile)
    .permission(Permission::ViewBalance)
    .permission(Permission::SendTokens)
    // Set entry point and resources
    .entry_point("game.html")
    // Allow external API calls to game server
    .allowed_domain("api.supergame.example.com")
    // Build the manifest
    .build()
    .expect("Failed to build manifest");

    println!("📦 Manifest Created:");
    println!("   Name:        {}", manifest.metadata.name);
    println!("   Version:     {}", manifest.version);
    println!("   Category:    {:?}", manifest.metadata.category);
    println!("   Entry Point: {}", manifest.resources.entry_point);
    println!("   Max Memory:  {} MB", manifest.runtime.max_memory_mb);

    println!("\n🔐 Requested Permissions:");
    for perm in manifest.permissions.iter() {
        let risk = perm.risk_level();
        let icon = match risk {
            RiskLevel::Low => "🟢",
            RiskLevel::Medium => "🟡",
            RiskLevel::High => "🔴",
            RiskLevel::Critical => "⛔",
        };
        println!("   {} {:?} ({:?})", icon, perm, risk);
    }

    // Validate the manifest
    match manifest.validate() {
        Ok(()) => println!("\n✅ Manifest is valid!"),
        Err(e) => println!("\n❌ Manifest validation failed: {:?}", e),
    }

    // Serialize to JSON for storage/transmission
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    println!("\n📄 Manifest JSON (first 500 chars):");
    println!("{}", &json[..json.len().min(500)]);
    println!();
}

/// Example 3: Launch a Sandbox
///
/// Mini-apps run in isolated sandboxes. This example shows how to create
/// and configure a sandbox for app execution.
fn example_3_launch_sandbox() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ Example 3: Launch Sandbox                                       │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    // Create a manifest (simplified)
    let manifest = ManifestBuilder::new("TestApp", "1.0.0", "A test app")
        .category(AppCategory::Utilities)
        .permission(Permission::ReadProfile)
        .entry_point("index.html")
        .build()
        .expect("Valid manifest");

    // Create sandbox configuration from manifest
    let sandbox_config = SandboxConfig::from_manifest(&manifest);

    println!("🔧 Sandbox Configuration:");
    println!(
        "   Max Memory:       {} MB",
        sandbox_config.max_memory / 1024 / 1024
    );
    println!("   CPU Timeout:      {} ms", sandbox_config.max_cpu_time_ms);
    println!(
        "   Max Storage:      {} MB",
        sandbox_config.max_storage / 1024 / 1024
    );
    println!("   Network Allowed:  {}", sandbox_config.allow_network);
    println!(
        "   Max Net Requests: {}/min",
        sandbox_config.max_network_requests
    );

    // Create sandbox context (session-specific data)
    let sandbox_id = SandboxId::new();
    let app_id = AppId::from_bytes([1u8; 32]);
    let user_id = [2u8; 32];
    let session_id = Uuid::new_v4();

    println!("\n🆔 Sandbox Context:");
    println!("   Sandbox ID:  {}", sandbox_id);
    println!("   App ID:      {}", app_id);
    println!("   User ID:     0x{}...", hex::encode(&user_id[..8]));
    println!("   Session ID:  {}", session_id);

    // Create initialization message
    let init_message = SandboxMessage::Init {
        app_id: app_id.to_string(),
        config: serde_json::json!({
            "theme": "dark",
            "viewport": { "width": 480, "height": 800 },
            "locale": "en-US",
        }),
    };

    println!("\n📨 Init Message:");
    println!("   {}", init_message.to_json().unwrap_or_default());

    // Simulate sandbox lifecycle
    println!("\n🔄 Sandbox Lifecycle:");
    println!("   1. Initializing → sandbox receives Init message");
    println!("   2. Ready → sandbox sends Ready message");
    println!("   3. Running → user interacts with app");
    println!("   4. Terminated → app closes gracefully");
    println!();
}

/// Example 4: Handle Permissions
///
/// Mini-apps must request permissions from users. This example shows
/// the permission flow including user consent.
fn example_4_handle_permissions() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ Example 4: Handle Permissions                                   │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    // App requests permissions
    let requested_permissions = vec![
        Permission::ReadProfile,
        Permission::ViewBalance,
        Permission::SendTokens,
        Permission::AccessContacts,
    ];

    println!("🔐 App Requests Permissions:");
    for perm in &requested_permissions {
        let risk = perm.risk_level();
        let desc = perm.description();
        println!("   • {:?}", perm);
        println!("     Risk: {:?}", risk);
        println!("     Description: {}", desc);
    }

    // Simulate user consent (in production, this shows a UI dialog)
    println!("\n👤 User Decision:");
    let user_grants: Vec<(Permission, bool)> = vec![
        (Permission::ReadProfile, true),    // Approved
        (Permission::ViewBalance, true),    // Approved
        (Permission::SendTokens, false),    // Denied - too risky
        (Permission::AccessContacts, true), // Approved
    ];

    let mut granted = Vec::new();
    let mut denied = Vec::new();

    for (perm, approved) in user_grants {
        if approved {
            println!("   ✅ {:?} - Approved", perm);
            granted.push(perm);
        } else {
            println!("   ❌ {:?} - Denied", perm);
            denied.push(perm);
        }
    }

    // Create permission response message
    let response = SandboxMessage::PermissionResponse {
        granted: granted.iter().map(|p| format!("{:?}", p)).collect(),
        denied: denied.iter().map(|p| format!("{:?}", p)).collect(),
    };

    println!("\n📨 Permission Response:");
    println!("   {}", response.to_json().unwrap_or_default());
    println!();
}

/// Example 5: Wallet Integration
///
/// Mini-apps can integrate with the user's wallet for payments and signing.
/// The wallet is "invisible" by default for seamless UX.
fn example_5_wallet_integration() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ Example 5: Wallet Integration                                   │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let app_id = AppId::from_bytes([1u8; 32]);
    let sandbox_id = SandboxId::new();
    let user_pubkey = [100u8; 32];

    // Create wallet connection
    let mut connection =
        WalletConnection::new(app_id, sandbox_id, user_pubkey, "dchat-mainnet".to_string());

    println!("💳 Wallet Connection:");
    println!("   Connection ID: {}", connection.id);
    println!("   Visibility:    {:?}", connection.visibility);
    println!("   Chain:         {}", connection.chain_id);
    println!("   Permissions:   {:?}", connection.session_permissions);

    // Grant permission for token transfers
    connection.add_permission(Permission::SendTokens);
    println!("\n🔓 After granting SendTokens:");
    println!(
        "   Has SendTokens: {}",
        connection.has_permission(&Permission::SendTokens)
    );

    // Create a sign request (for a payment)
    let sign_request = SignRequest::message(
        connection.id,
        b"Transfer 100 DCHAT to 0xabc123...".to_vec(),
        "Confirm payment of 100 DCHAT for in-game purchase".to_string(),
    );

    println!("\n✍️ Sign Request:");
    println!("   Request ID:       {}", sign_request.id);
    println!("   Display Message:  {}", sign_request.display_message);
    println!("   Requires Approval: {}", sign_request.requires_approval);
    println!("   Expires:          {:?}", sign_request.expires_at);

    // In production, this would show a confirmation dialog
    println!("\n💡 In a real app:");
    println!("   1. User sees confirmation dialog");
    println!("   2. User approves or rejects");
    println!("   3. If approved, wallet signs the message");
    println!("   4. Signature returned to mini-app");
    println!();
}

/// Example 6: Intent Flow (Cross-Chain Transactions)
///
/// Intents allow mini-apps to request blockchain transactions.
/// The flow is: Intent → Execute → Receipt
fn example_6_intent_flow() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ Example 6: Intent Flow (Cross-Chain Transactions)               │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let sender = [1u8; 32];
    let recipient = [2u8; 32];

    // Step 1: Create a transfer intent
    // DCHAT uses 8 decimals: 1 DCHAT = 100,000,000 motes
    let intent_type = IntentType::Transfer {
        recipient,
        mint: None,          // Native token
        amount: 100_000_000, // 1 DCHAT = 100,000,000 motes (8 decimals)
        memo: Some("Payment for premium subscription".to_string()),
    };

    let intent_payload = IntentPayload::new(
        sender,
        intent_type,
        10_000, // max fee in motes (0.0001 DCHAT)
        1,      // chain_id
    );

    println!("📤 Step 1: Create Intent");
    println!("   Sender:    0x{}...", hex::encode(&sender[..8]));
    println!("   Recipient: 0x{}...", hex::encode(&recipient[..8]));
    println!("   Amount:    1 DCHAT (100,000,000 motes)");
    println!("   Max Fee:   0.0001 DCHAT (10,000 motes)");

    // Step 2: Intent is submitted and executed
    let intent_id = IntentId::new();
    println!("\n⚡ Step 2: Execute Intent");
    println!("   Intent ID: {}", intent_id);

    // Step 3: Create execution result
    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec![
            "Validated intent signatures".to_string(),
            "Checked sender balance: 100 DCHAT".to_string(),
            "Executed transfer: 1 DCHAT".to_string(),
            "Updated recipient balance".to_string(),
            "Transaction finalized".to_string(),
        ],
        account_changes: vec![],
        compute_units: 5000,
        fee: 100,
    };

    println!("   Compute Units: {}", result.compute_units);
    println!("   Fee:           {} microDCHAT", result.fee);

    // Step 4: Create receipt
    let tx_hash = [0xab; 32];
    let block_hash = [0xcd; 32];
    let receipt = Receipt::new(intent_id, result, 12345, tx_hash, block_hash);

    println!("\n📋 Step 3: Receipt Created");
    println!("   Receipt ID:   {}", receipt.id);
    println!("   Block Height: {}", receipt.block_height);
    println!("   TX Hash:      0x{}...", hex::encode(&tx_hash[..8]));
    println!("   Status:       {:?}", receipt.status);

    println!("\n📜 Execution Logs:");
    for (i, log) in receipt.result.logs.iter().enumerate() {
        println!("   {}. {}", i + 1, log);
    }
    println!();
}

/// Example 7: Bot Integration
///
/// Mini-apps can integrate with bots for automated workflows
/// like payment notifications or scheduled tasks.
fn example_7_bot_integration() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ Example 7: Bot Integration                                      │");
    println!("└─────────────────────────────────────────────────────────────────┘\n");

    let signing_key = generate_signing_key(1);
    let pubkey = signing_key.verifying_key().to_bytes();
    let app_id = AppId::from_bytes([1u8; 32]);

    // Create bot identity
    let bot = BotIdentity::new(
        app_id,
        "payment_bot".to_string(),
        "Payment Notification Bot".to_string(),
        pubkey,
    )
    .expect("Valid bot identity");

    println!("🤖 Bot Identity:");
    println!("   Username:    @{}", bot.username);
    println!("   Display:     {}", bot.display_name);
    println!("   App ID:      {}", bot.app_id);

    // Generate auth token for bot API calls
    let bot_id = BotId::from_app_id(&app_id);
    let token = BotAuthToken::create(
        bot_id,
        &signing_key,
        Some("channel_payments".to_string()),
        Some("user_alice".to_string()),
        chrono::Duration::hours(1),
    );

    println!("\n🔑 Auth Token:");
    println!("   Bot ID:     {}", token.bot_id);
    println!("   Channel:    {:?}", token.channel_scope);
    println!("   User:       {:?}", token.user_scope);
    println!("   Expired:    {}", token.is_expired());

    // Verify token with bot's public key
    let verifying_key = signing_key.verifying_key();
    match token.verify(&verifying_key) {
        Ok(()) => println!("   Signature:  ✅ Valid"),
        Err(e) => println!("   Signature:  ❌ Invalid: {:?}", e),
    }

    println!("\n💡 Bot Use Cases:");
    println!("   • Send payment confirmations");
    println!("   • Process scheduled payments");
    println!("   • Handle inline payment buttons");
    println!("   • Automate refunds and disputes");
    println!();
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Generate a deterministic signing key for examples
fn generate_signing_key(seed: u8) -> SigningKey {
    let mut seed_bytes = [0u8; 32];
    seed_bytes[0] = seed;
    SigningKey::from_bytes(&seed_bytes)
}
