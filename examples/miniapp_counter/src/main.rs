//! Counter Mini-App Example for dchat
//!
//! This example demonstrates how to create a simple mini-app that:
//! 1. Registers as a developer
//! 2. Creates an app manifest
//! 3. Registers the app on-chain
//! 4. Uses the wallet integration to sign transactions
//! 5. Creates intents and handles receipts
//!
//! Run with: `cargo run -p miniapp-counter-example`

use std::sync::Arc;

use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use dchat_miniapps::{
    // Manifest types
    manifest::{AppCategory, ManifestBuilder},
    permissions::PermissionManager,
    // Registry types
    AppId,
    AppManifest,
    AppRegistration,
    BotBridge,
    BotCallbackQuery,
    BotCommandContext,
    BotCommandHandler,
    BotCommandResult,
    // Bot types
    BotIdentity,
    Developer,
    DeveloperRegistry,
    // Intent types
    IntentBuilder,
    MiniAppRegistry,
    MiniAppResult,
    // Permission types
    Permission,
    PermissionSet,
    RegistrationStatus,
    // Sandbox types
    SandboxConfig,
    SandboxRuntime,
    VerifiedApp,
    // Wallet types
    WalletIntegration,
};

/// Counter app state
#[derive(Debug, Clone, Default)]
pub struct CounterState {
    pub value: i64,
    pub increments: u64,
    pub decrements: u64,
}

impl CounterState {
    pub fn increment(&mut self) {
        self.value += 1;
        self.increments += 1;
    }

    pub fn decrement(&mut self) {
        self.value -= 1;
        self.decrements += 1;
    }

    pub fn reset(&mut self) {
        self.value = 0;
    }
}

/// Counter action for intent payloads
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CounterAction {
    Increment,
    Decrement,
    Reset,
    SetValue(i64),
}

/// Main example function
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("🚀 Starting Counter Mini-App Example");
    info!("=====================================\n");

    // Step 1: Create developer identity
    info!("📝 Step 1: Creating developer identity...");
    let (developer, signing_key) = create_developer()?;
    info!("   Developer ID: {}", developer.id);
    info!("   Developer Name: {}", developer.name);

    // Step 2: Register developer
    info!("\n📋 Step 2: Registering developer...");
    let dev_registry = Arc::new(DeveloperRegistry::new());
    dev_registry.register(developer.clone())?;
    dev_registry.verify(&developer.id)?;
    info!("   ✅ Developer registered and verified!");

    // Step 3: Create app manifest
    info!("\n📦 Step 3: Creating app manifest...");
    let manifest = create_counter_manifest()?;
    info!("   App: {}", manifest.metadata.name);
    info!("   Version: {}", manifest.version);
    info!("   Category: {:?}", manifest.metadata.category);
    info!("   Permissions: {} required", manifest.permissions.len());

    // Step 4: Register the mini-app
    info!("\n🔐 Step 4: Registering mini-app...");
    let app_registry = MiniAppRegistry::new(dev_registry.clone());
    let registration = create_app_registration(&developer, &manifest, &signing_key)?;
    let app_id = app_registry.register(registration)?;
    info!("   ✅ App registered with ID: {}", app_id);

    // Step 5: Get verified app info
    info!("\n✅ Step 5: Verifying app registration...");
    let verified_app = app_registry.get_verified(&app_id)?;
    info!("   Verified at: {}", verified_app.verified_at);
    info!(
        "   Bundle hash: {}",
        hex::encode(verified_app.registration.bundle_hash)
    );

    // Step 6: Simulate wallet integration
    info!("\n💰 Step 6: Simulating wallet integration...");
    simulate_wallet_flow(&verified_app).await?;

    // Step 7: Simulate sandbox execution
    info!("\n🧪 Step 7: Simulating sandbox execution...");
    simulate_sandbox_execution(&verified_app).await?;

    // Step 8: Simulate bot commands
    info!("\n🤖 Step 8: Simulating bot commands...");
    simulate_bot_commands().await?;

    // Step 9: Run counter state machine
    info!("\n🔢 Step 9: Running counter state machine...");
    run_counter_demo()?;

    info!("\n=====================================");
    info!("🎉 Counter Mini-App Example Complete!");

    Ok(())
}

/// Create a new developer with Ed25519 keypair
fn create_developer() -> Result<(Developer, SigningKey), Box<dyn std::error::Error>> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let public_key = signing_key.verifying_key();

    let mut developer = Developer::new("Counter App Developer".to_string(), public_key.to_bytes());
    developer.website = Some("https://counter.dchat.network".to_string());
    developer.email = Some("dev@counter.dchat.network".to_string());

    Ok((developer, signing_key))
}

/// Create the counter app manifest
fn create_counter_manifest() -> Result<AppManifest, Box<dyn std::error::Error>> {
    // Using the builder pattern
    let manifest = ManifestBuilder::new(
        "Counter Mini-App",
        "1.0.0",
        "A simple counter app that demonstrates dchat mini-app capabilities",
    )
    .icon("data:image/svg+xml,<svg>...</svg>")
    .category(AppCategory::Utilities)
    .tag("counter")
    .tag("demo")
    .tag("example")
    .entry_point("index.html")
    .allowed_domain("api.counter.dchat.network")
    .permission(Permission::ReadProfile)
    .permission(Permission::SendMessages)
    .theme_color("#4CAF50")
    .with_bot("counter_bot", "Counter Bot")
    .build()?;

    Ok(manifest)
}

/// Create app registration with signature
fn create_app_registration(
    developer: &Developer,
    manifest: &AppManifest,
    signing_key: &SigningKey,
) -> Result<AppRegistration, Box<dyn std::error::Error>> {
    let app_id = AppId::derive(&developer.id, &manifest.metadata.name);

    // Create bundle hash (in production, this would hash the actual app bundle)
    let bundle_hash: [u8; 32] = blake3::hash(b"counter-app-bundle-v1.0.0").into();

    // Create registration without signature first
    let mut registration = AppRegistration {
        app_id,
        developer_id: developer.id,
        name: manifest.metadata.name.clone(),
        version: semver::Version::parse(&manifest.version)?,
        manifest: manifest.clone(),
        bundle_hash,
        status: RegistrationStatus::Active,
        registered_at: Utc::now(),
        updated_at: Utc::now(),
        signature: [0u8; 64], // Will be filled below
        downloads: 0,
        rating: 0,
        rating_count: 0,
        permissions: manifest.permissions.clone(),
        expires_at: None,
    };

    // Sign the registration
    let payload = registration.signature_payload();
    let signature = signing_key.sign(&payload);
    registration.signature = signature.to_bytes();

    Ok(registration)
}

/// Simulate wallet integration flow
async fn simulate_wallet_flow(
    verified_app: &VerifiedApp,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create wallet integration with permission manager
    let permission_manager = Arc::new(PermissionManager::new());
    let _wallet = WalletIntegration::new(permission_manager);

    info!("   Creating wallet context for app...");

    // Simulate a sign request (counter increment transaction)
    let action = CounterAction::Increment;
    let payload = serde_json::to_vec(&action)?;

    info!("   Action: {:?}", action);
    info!("   Payload size: {} bytes", payload.len());

    // Create an intent for the action using the builder
    let sender = [0u8; 32]; // Mock sender
    let intent = IntentBuilder::new(verified_app.registration.app_id, sender, 1)
        .transfer([1u8; 32], 100) // Mock transfer as intent example
        .max_fee(5000)
        .build()?;

    info!("   Intent ID: {}", intent.id);
    info!("   Intent status: {:?}", intent.status);

    // Simulate intent approval and execution
    info!("   ✅ Intent would be signed by user's wallet");

    Ok(())
}

/// Simulate sandbox execution
async fn simulate_sandbox_execution(
    verified_app: &VerifiedApp,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create sandbox config from manifest
    let config = SandboxConfig::from_manifest(&verified_app.registration.manifest);

    info!("   Sandbox config:");
    info!("     Max memory: {} MB", config.max_memory / (1024 * 1024));
    info!("     Max CPU time: {} ms", config.max_cpu_time_ms);
    info!("     Allowed domains: {:?}", config.allowed_domains);

    // Create sandbox runtime
    let runtime = SandboxRuntime::new(10);

    // Create a sandbox instance
    let user_id = [0u8; 32];
    let sandbox = runtime.create_sandbox(verified_app.registration.app_id, user_id, config)?;

    info!("   ✅ Sandbox created with ID: {}", sandbox.id);
    info!(
        "   Sandbox would execute: {}",
        verified_app.registration.manifest.resources.entry_point
    );

    // Start the sandbox
    sandbox.start()?;
    info!("   ✅ Sandbox is now running");

    // Clean up
    runtime.terminate_sandbox(&sandbox.id, "example complete")?;

    Ok(())
}

/// Simulate bot commands
async fn simulate_bot_commands() -> Result<(), Box<dyn std::error::Error>> {
    // Create bot identity
    let signing_key = SigningKey::generate(&mut OsRng);
    let public_key = signing_key.verifying_key();

    // Create a bot identity
    let app_id = AppId([42u8; 32]); // Example app ID
    let bot_identity = BotIdentity::new(
        app_id,
        "counter_bot".to_string(),
        "Counter Bot".to_string(),
        public_key.to_bytes(),
    )?;

    let bot_id = bot_identity.id;

    info!("   Bot ID: {}", bot_id);
    info!("   Bot username: @{}", bot_identity.username);

    // Create a bot bridge with permission manager
    let permission_manager = Arc::new(PermissionManager::new());
    let bridge = BotBridge::new(permission_manager);

    // Create a simple command handler
    struct CounterHandler;
    impl BotCommandHandler for CounterHandler {
        fn handle_command(&self, ctx: &BotCommandContext) -> MiniAppResult<BotCommandResult> {
            match ctx.command.as_str() {
                "count" => Ok(BotCommandResult::text("Current count: 0")),
                "increment" => Ok(BotCommandResult::text("Counter incremented! New value: 1")),
                "decrement" => Ok(BotCommandResult::text("Counter decremented! New value: -1")),
                "reset" => Ok(BotCommandResult::text("Counter reset to 0")),
                _ => Ok(BotCommandResult::text(format!(
                    "Unknown command: /{}",
                    ctx.command
                ))),
            }
        }

        fn handle_callback(
            &self,
            query: &dchat_miniapps::BotCallbackQuery,
        ) -> MiniAppResult<BotCommandResult> {
            Ok(BotCommandResult::text(format!(
                "Callback received: {}",
                query.data
            )))
        }
    }

    // Register the bot
    bridge.register(bot_identity, Arc::new(CounterHandler), PermissionSet::new())?;

    // Simulate bot commands
    let commands = vec![
        ("/count", "Show current counter value"),
        ("/increment", "Increment the counter by 1"),
        ("/decrement", "Decrement the counter by 1"),
        ("/reset", "Reset the counter to 0"),
    ];

    info!("   Available commands:");
    for (cmd, desc) in &commands {
        info!("     {} - {}", cmd, desc);
    }

    // Test a command
    if let Some(ctx) = BotCommandContext::parse("/count", "user123", "channel456", "msg789") {
        let result = bridge.handle_command(&bot_id, ctx)?;
        if let Some(text) = result.text {
            info!("   Bot response: {}", text);
        }
    }

    info!("   ✅ Bot commands configured successfully");

    Ok(())
}

/// Run counter state machine demo
fn run_counter_demo() -> Result<(), Box<dyn std::error::Error>> {
    let mut counter = CounterState::default();

    info!("   Initial state: {}", counter.value);

    // Perform some operations
    let operations = vec![
        CounterAction::Increment,
        CounterAction::Increment,
        CounterAction::Increment,
        CounterAction::Decrement,
        CounterAction::SetValue(100),
        CounterAction::Increment,
        CounterAction::Reset,
    ];

    for op in operations {
        match op {
            CounterAction::Increment => {
                counter.increment();
                info!("   Increment -> {}", counter.value);
            }
            CounterAction::Decrement => {
                counter.decrement();
                info!("   Decrement -> {}", counter.value);
            }
            CounterAction::Reset => {
                counter.reset();
                info!("   Reset -> {}", counter.value);
            }
            CounterAction::SetValue(v) => {
                counter.value = v;
                info!("   SetValue({}) -> {}", v, counter.value);
            }
        }
    }

    info!("   Final state:");
    info!("     Value: {}", counter.value);
    info!("     Total increments: {}", counter.increments);
    info!("     Total decrements: {}", counter.decrements);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_increment() {
        let mut counter = CounterState::default();
        counter.increment();
        assert_eq!(counter.value, 1);
        assert_eq!(counter.increments, 1);
    }

    #[test]
    fn test_counter_decrement() {
        let mut counter = CounterState::default();
        counter.decrement();
        assert_eq!(counter.value, -1);
        assert_eq!(counter.decrements, 1);
    }

    #[test]
    fn test_counter_reset() {
        let mut counter = CounterState {
            value: 100,
            increments: 50,
            decrements: 10,
        };
        counter.reset();
        assert_eq!(counter.value, 0);
        // Reset preserves increment/decrement counts
        assert_eq!(counter.increments, 50);
    }

    #[test]
    fn test_developer_creation() {
        let (developer, _) = create_developer().unwrap();
        assert_eq!(developer.name, "Counter App Developer");
        assert!(developer.website.is_some());
    }

    #[test]
    fn test_manifest_creation() {
        let manifest = create_counter_manifest().unwrap();
        assert_eq!(manifest.metadata.name, "Counter Mini-App");
        assert_eq!(manifest.version, "1.0.0");
        assert!(manifest.permissions.contains(&Permission::ReadProfile));
    }

    #[test]
    fn test_app_registration() {
        let (developer, signing_key) = create_developer().unwrap();
        let manifest = create_counter_manifest().unwrap();
        let registration = create_app_registration(&developer, &manifest, &signing_key).unwrap();

        assert_eq!(registration.name, "Counter Mini-App");
        assert_eq!(registration.status, RegistrationStatus::Active);
    }

    #[test]
    fn test_counter_action_serialization() {
        let action = CounterAction::SetValue(42);
        let json = serde_json::to_string(&action).unwrap();
        let parsed: CounterAction = serde_json::from_str(&json).unwrap();

        match parsed {
            CounterAction::SetValue(v) => assert_eq!(v, 42),
            _ => panic!("Wrong action type"),
        }
    }
}
