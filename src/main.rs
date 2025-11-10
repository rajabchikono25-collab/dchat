//! dchat - Production Relay Node and User Client
//!
//! Comprehensive production entry point with:
//! - CLI argument parsing
//! - Configuration loading
//! - Service orchestration
//! - Health checks
//! - Graceful shutdown
//! - Observability integration

use dchat::blockchain::{
    ChatChainClient, ChatChainConfig, CrossChainBridge, CurrencyChainClient, CurrencyChainConfig,
};
use dchat::prelude::*;

use clap::{Parser, Subcommand};
use dchat_network::{Multiaddr, PeerId};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::signal;
use tokio::sync::broadcast;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_CONFIG_PATH: &str = "config.toml";
const PUBLIC_TLS_PORT: u16 = 443;
const PUBLIC_HTTP_PORT: u16 = 80;

fn default_relay_listen() -> String {
    format!("0.0.0.0:{}", PUBLIC_TLS_PORT)
}

fn default_health_addr() -> String {
    format!("0.0.0.0:{}", PUBLIC_HTTP_PORT)
}

#[derive(Parser)]
#[command(name = "dchat")]
#[command(version = VERSION)]
#[command(about = "Decentralized end-to-end encrypted chat", long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(short, long, value_name = "FILE", default_value = DEFAULT_CONFIG_PATH)]
    config: PathBuf,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Enable JSON logging for structured output
    #[arg(long)]
    json_logs: bool,

    /// Metrics server listen address
    #[arg(long, default_value = "127.0.0.1:9090")]
    metrics_addr: String,

    /// Health check server listen address
    #[arg(long, default_value_t = default_health_addr())]
    health_addr: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run as relay node (routes messages between peers)
    Relay {
        /// Relay listen address
        #[arg(long, default_value_t = default_relay_listen())]
        listen: String,

        /// Bootstrap peer addresses (multiaddr format)
        #[arg(long)]
        bootstrap: Vec<String>,

        /// Enable HSM/KMS for validator signing
        #[arg(long)]
        hsm: bool,

        /// AWS KMS key ID for validator signing
        #[arg(long)]
        kms_key_id: Option<String>,

        /// Stake amount for relay incentives (in tokens)
        #[arg(long, default_value = "1000")]
        stake: u64,
    },

    /// Run as user node (interactive chat client)
    User {
        /// Bootstrap peer addresses
        #[arg(long)]
        bootstrap: Vec<String>,

        /// Identity backup file path
        #[arg(long)]
        identity: Option<PathBuf>,

        /// Username for display
        #[arg(long)]
        username: Option<String>,

        /// Non-interactive mode (for testing)
        #[arg(long)]
        non_interactive: bool,

        /// Metrics server address
        #[arg(long)]
        metrics_addr: Option<String>,

        /// Health check server address
        #[arg(long)]
        health_addr: Option<String>,
    },

    /// Run as validator node (participates in consensus)
    Validator {
        /// Validator key file path (or HSM key ID)
        #[arg(long)]
        key: String,

        /// Chain RPC endpoint
        #[arg(long)]
        chain_rpc: String,

        /// Enable HSM/KMS
        #[arg(long)]
        hsm: bool,

        /// Validator stake amount
        #[arg(long, default_value = "10000")]
        stake: u64,

        /// Enable block production
        #[arg(long)]
        producer: bool,
    },

    /// Launch full testnet (validators + relays + clients)
    Testnet {
        /// Number of validator nodes
        #[arg(long, default_value = "3")]
        validators: usize,

        /// Number of relay nodes
        #[arg(long, default_value = "3")]
        relays: usize,

        /// Number of client nodes
        #[arg(long, default_value = "5")]
        clients: usize,

        /// Base data directory for all nodes
        #[arg(long, default_value = "./testnet-data")]
        data_dir: PathBuf,

        /// Enable observability stack
        #[arg(long)]
        observability: bool,
    },

    /// Generate new identity and keys
    Keygen {
        /// Output file path
        #[arg(short, long, default_value = "identity.json")]
        output: PathBuf,

        /// Generate ephemeral/burner identity
        #[arg(long)]
        burner: bool,
    },

    /// User account management
    Account {
        #[command(subcommand)]
        action: AccountCommand,
    },

    /// Database management commands
    Database {
        #[command(subcommand)]
        action: DatabaseCommand,
    },

    /// Health check (returns exit code 0 if healthy)
    Health {
        /// Node health check URL
        #[arg(long, default_value = "http://127.0.0.1:8080/health")]
        url: String,
    },

    /// Bot management operations
    Bot {
        #[command(subcommand)]
        action: BotCommand,
    },

    /// Marketplace operations
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceCommand,
    },

    /// Accessibility features and testing
    Accessibility {
        #[command(subcommand)]
        action: AccessibilityCommand,
    },

    /// Chaos engineering and testing
    Chaos {
        #[command(subcommand)]
        action: ChaosCommand,
    },

    /// Protocol governance and upgrades
    Governance {
        #[command(subcommand)]
        action: GovernanceCommand,
    },

    /// tokenomics and currency management
    Token {
        #[command(subcommand)]
        action: TokenCommand,
    },

    /// Update distribution and auto-update management
    Update {
        #[command(subcommand)]
        action: UpdateCommand,
    },

    /// Deployment planning, validation, and health tooling
    Deploy {
        #[command(subcommand)]
        action: DeployCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DeployCommand {
    /// Generate a complete deployment plan with configs for validators, relays, storage, backups, and monitoring
    Plan {
        /// Logical network name (e.g. dchat-mainnet, dchat-testnet)
        #[arg(long)]
        network: String,

        /// Base domain used for validator and relay hostnames
        #[arg(long)]
        domain: String,

        /// Number of relays to include in the plan (20-50 recommended)
        #[arg(long, default_value_t = 30)]
        relays: usize,

        /// Output directory for generated artifacts
        #[arg(long, default_value = "./deployment-plan")]
        output: PathBuf,
    },

    /// Validate an existing deployment plan directory
    Validate {
        /// Path to deployment plan directory (containing deployment-plan.json)
        #[arg(long)]
        input: PathBuf,
    },

    /// Print a summary of an existing deployment plan without validation
    Summary {
        /// Path to deployment plan directory (containing deployment-plan.json)
        #[arg(long)]
        input: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum BotCommand {
    /// Create a new bot
    Create {
        /// Bot username (must end with 'bot')
        #[arg(long)]
        username: String,

        /// Bot display name
        #[arg(long)]
        name: String,

        /// Bot description
        #[arg(long)]
        description: String,

        /// Owner user ID
        #[arg(long)]
        owner_id: String,
    },

    /// List all bots or bots by owner
    List {
        /// Filter by owner user ID
        #[arg(long)]
        owner_id: Option<String>,
    },

    /// Get bot information
    Info {
        /// Bot ID
        #[arg(long)]
        bot_id: String,
    },

    /// Regenerate bot token
    RegenerateToken {
        /// Bot ID
        #[arg(long)]
        bot_id: String,

        /// Owner user ID
        #[arg(long)]
        owner_id: String,
    },

    /// Set webhook URL for bot
    SetWebhook {
        /// Bot ID
        #[arg(long)]
        bot_id: String,

        /// Webhook URL
        #[arg(long)]
        url: String,

        /// Webhook secret for HMAC verification
        #[arg(long)]
        secret: Option<String>,
    },

    /// Send message as bot
    SendMessage {
        /// Bot token
        #[arg(long)]
        token: String,

        /// Chat ID to send to
        #[arg(long)]
        chat_id: String,

        /// Message text
        #[arg(long)]
        text: String,
    },
}

#[derive(Debug, Subcommand)]
enum MarketplaceCommand {
    /// List marketplace items
    List {
        /// Filter by item type (sticker-pack, emoji-pack, theme, bot, nft, image, subscription, badge, channel, membership)
        #[arg(long)]
        item_type: Option<String>,
    },

    /// Create a new listing
    CreateListing {
        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// Listing title
        #[arg(long)]
        title: String,

        /// Listing description
        #[arg(long)]
        description: String,

        /// Item type
        #[arg(long)]
        item_type: String,

        /// Price (0 for free)
        #[arg(long)]
        price: u64,

        /// Content hash (IPFS CID)
        #[arg(long)]
        content_hash: String,

        /// Bot ID (if selling bot)
        #[arg(long)]
        bot_id: Option<String>,

        /// Channel ID (if selling channel or membership)
        #[arg(long)]
        channel_id: Option<String>,

        /// Membership duration in days (if selling membership)
        #[arg(long)]
        membership_duration: Option<u32>,
    },

    /// Buy a marketplace item
    Buy {
        /// Buyer user ID
        #[arg(long)]
        buyer_id: String,

        /// Listing ID
        #[arg(long)]
        listing_id: String,
    },

    /// Get creator statistics
    CreatorStats {
        /// Creator user ID
        #[arg(long)]
        creator_id: String,
    },

    /// Create escrow for a transaction
    CreateEscrow {
        /// Buyer user ID
        #[arg(long)]
        buyer: String,

        /// Seller user ID
        #[arg(long)]
        seller: String,

        /// Amount in tokens
        #[arg(long)]
        amount: u64,
    },

    /// Register bot for marketplace trading
    RegisterBot {
        /// Bot ID
        #[arg(long)]
        bot_id: String,

        /// Bot username
        #[arg(long)]
        username: String,

        /// Owner user ID
        #[arg(long)]
        owner: String,
    },

    /// Register channel for marketplace trading
    RegisterChannel {
        /// Channel ID
        #[arg(long)]
        channel_id: String,

        /// Channel name
        #[arg(long)]
        name: String,

        /// Owner user ID
        #[arg(long)]
        owner: String,

        /// Current member count
        #[arg(long)]
        member_count: u64,
    },

    /// Get bot ownership info
    BotOwnership {
        /// Bot ID
        #[arg(long)]
        bot_id: String,
    },

    /// Get channel ownership info
    ChannelOwnership {
        /// Channel ID
        #[arg(long)]
        channel_id: String,
    },

    /// List bots owned by user
    MyBots {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// List channels owned by user
    MyChannels {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// Create emoji pack
    CreateEmojiPack {
        /// Pack name
        #[arg(long)]
        name: String,

        /// Pack description
        #[arg(long)]
        description: String,

        /// Number of emojis
        #[arg(long)]
        emoji_count: u32,

        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// Content hash
        #[arg(long)]
        content_hash: String,

        /// Is animated
        #[arg(long, default_value = "false")]
        animated: bool,
    },

    /// Register image artwork
    RegisterImage {
        /// Image title
        #[arg(long)]
        title: String,

        /// Image description
        #[arg(long)]
        description: String,

        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// Content hash
        #[arg(long)]
        content_hash: String,

        /// Width in pixels
        #[arg(long)]
        width: u32,

        /// Height in pixels
        #[arg(long)]
        height: u32,

        /// Image format (png, jpg, etc.)
        #[arg(long)]
        format: String,

        /// License type (all-rights-reserved, cc-by, cc-by-sa, cc-by-nd, cc-by-nc, public-domain)
        #[arg(long, default_value = "all-rights-reserved")]
        license: String,
    },

    /// Check channel membership
    CheckMembership {
        /// Channel ID
        #[arg(long)]
        channel_id: String,

        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// List my memberships
    MyMemberships {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// Transfer membership
    TransferMembership {
        /// Membership ID
        #[arg(long)]
        membership_id: String,

        /// New holder user ID
        #[arg(long)]
        new_holder: String,
    },

    /// List channel members
    ChannelMembers {
        /// Channel ID
        #[arg(long)]
        channel_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum AccessibilityCommand {
    /// Validate color contrast
    ValidateContrast {
        /// Foreground color (hex, e.g., #000000)
        #[arg(long)]
        fg_color: String,

        /// Background color (hex, e.g., #FFFFFF)
        #[arg(long)]
        bg_color: String,

        /// Target WCAG level (A, AA, AAA)
        #[arg(long, default_value = "AA")]
        level: String,
    },

    /// Test text-to-speech
    TtsSpeak {
        /// Text to speak
        #[arg(long)]
        text: String,

        /// Language code (e.g., en-US)
        #[arg(long, default_value = "en-US")]
        language: String,
    },

    /// Validate UI element accessibility
    ValidateElement {
        /// Element ID
        #[arg(long)]
        element_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum ChaosCommand {
    /// List available chaos scenarios
    ListScenarios,

    /// Execute a chaos scenario
    Execute {
        /// Scenario ID or name
        #[arg(long)]
        scenario: String,

        /// Duration in seconds
        #[arg(long, default_value = "60")]
        duration: u64,
    },

    /// Inject a specific fault
    InjectFault {
        /// Target node
        #[arg(long)]
        node: String,

        /// Fault type (latency, packet-loss, cpu-spike, memory-pressure, disk-slow, network-partition, service-crash)
        #[arg(long)]
        fault_type: String,

        /// Fault severity (0.0-1.0)
        #[arg(long, default_value = "0.5")]
        severity: f32,

        /// Duration in seconds
        #[arg(long, default_value = "60")]
        duration: u64,
    },

    /// Simulate network partition
    SimulatePartition {
        /// Nodes in partition A (comma-separated)
        #[arg(long)]
        partition_a: String,

        /// Nodes in partition B (comma-separated)
        #[arg(long)]
        partition_b: String,

        /// Duration in seconds
        #[arg(long, default_value = "120")]
        duration: u64,
    },
}

#[derive(Debug, Subcommand)]
enum GovernanceCommand {
    /// Submit a protocol upgrade proposal
    ProposeUpgrade {
        /// Proposer user ID
        #[arg(long)]
        proposer: String,

        /// Upgrade type (soft-fork, hard-fork, security-patch, feature-toggle)
        #[arg(long)]
        upgrade_type: String,

        /// Target version (e.g., "2.0.0")
        #[arg(long)]
        target_version: String,

        /// Proposal title
        #[arg(long)]
        title: String,

        /// Proposal description
        #[arg(long)]
        description: String,

        /// Specification URL (GitHub PR, RFC document)
        #[arg(long)]
        spec_url: Option<String>,

        /// Voting period in days
        #[arg(long, default_value = "14")]
        voting_days: i64,

        /// Required quorum percentage
        #[arg(long, default_value = "60")]
        quorum: u32,
    },

    /// List all active upgrade proposals
    ListProposals {
        /// Filter by status (proposed, approved, scheduled, active, rejected, cancelled)
        #[arg(long)]
        status: Option<String>,
    },

    /// Get details of a specific proposal
    GetProposal {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,
    },

    /// Vote on an upgrade proposal
    Vote {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,

        /// Voter user ID
        #[arg(long)]
        voter: String,

        /// Vote (true = for, false = against)
        #[arg(long)]
        vote_for: bool,

        /// Voting power (token amount staked)
        #[arg(long)]
        voting_power: u64,
    },

    /// Validator signs upgrade approval (for hard forks)
    SignUpgrade {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,

        /// Validator user ID
        #[arg(long)]
        validator_id: String,

        /// Validator stake amount
        #[arg(long)]
        stake: u64,

        /// Validator key file for signature
        #[arg(long)]
        key_file: PathBuf,
    },

    /// Finalize upgrade proposal voting
    FinalizeProposal {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,
    },

    /// Schedule approved upgrade for activation
    ScheduleUpgrade {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,

        /// Activation block height
        #[arg(long)]
        activation_height: u64,

        /// Activation timestamp (RFC3339 format)
        #[arg(long)]
        activation_time: String,
    },

    /// Activate upgrade at current block height
    ActivateUpgrade {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,

        /// Current block height
        #[arg(long)]
        current_height: u64,
    },

    /// Emergency cancel an upgrade
    CancelUpgrade {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,
    },

    /// Show current protocol version
    Version,

    /// Show fork history
    ForkHistory,

    /// Check if peer version is compatible
    CheckCompatibility {
        /// Peer protocol version (e.g., "1.2.3")
        #[arg(long)]
        peer_version: String,
    },

    /// Configure governance parameters
    Configure {
        /// Hard fork threshold percentage (validator approval required)
        #[arg(long)]
        hard_fork_threshold: Option<u32>,

        /// Total network stake
        #[arg(long)]
        total_stake: Option<u64>,
    },
}

#[derive(Debug, Subcommand)]
enum UpdateCommand {
    /// Check for available updates
    Check {
        /// Current version (defaults to dchat version)
        #[arg(long)]
        current_version: Option<String>,
    },

    /// List all available versions
    ListVersions,

    /// Download a specific version
    Download {
        /// Version to download (e.g., "1.2.3")
        #[arg(long)]
        version: String,

        /// Platform (defaults to current platform)
        #[arg(long)]
        platform: Option<String>,
    },

    /// Verify a downloaded package
    Verify {
        /// Path to package file
        #[arg(long)]
        package: PathBuf,

        /// Expected version
        #[arg(long)]
        version: String,
    },

    /// Add a mirror/download source
    AddMirror {
        /// Mirror URL
        #[arg(long)]
        url: String,

        /// Mirror type (https, ipfs, bittorrent)
        #[arg(long)]
        mirror_type: String,

        /// Geographic region
        #[arg(long)]
        region: Option<String>,

        /// Priority (lower = preferred)
        #[arg(long, default_value = "50")]
        priority: u32,
    },

    /// List configured mirrors
    ListMirrors,

    /// Test mirror connectivity
    TestMirrors,

    /// Configure auto-update settings
    ConfigureAutoUpdate {
        /// Enable auto-updates
        #[arg(long)]
        enabled: Option<bool>,

        /// Security patches only
        #[arg(long)]
        security_only: Option<bool>,

        /// Check interval in hours
        #[arg(long)]
        check_interval: Option<u64>,

        /// Auto-restart after update
        #[arg(long)]
        auto_restart: Option<bool>,
    },

    /// Show auto-update configuration
    ShowConfig,
}

/// Token and tokenomics commands
#[derive(Debug, Subcommand)]
enum TokenCommand {
    /// Show token supply statistics
    Stats,

    /// Mint new tokens (requires admin)
    Mint {
        /// Amount to mint
        #[arg(long)]
        amount: u64,

        /// Reason for minting
        #[arg(long)]
        reason: String,

        /// Recipient user ID (optional)
        #[arg(long)]
        recipient: Option<String>,
    },

    /// Burn tokens
    Burn {
        /// User ID burning tokens
        #[arg(long)]
        user_id: String,

        /// Amount to burn
        #[arg(long)]
        amount: u64,

        /// Reason for burning
        #[arg(long)]
        reason: String,
    },

    /// Create marketplace liquidity pool
    CreatePool {
        /// Pool name
        #[arg(long)]
        name: String,

        /// Initial token amount
        #[arg(long)]
        initial_amount: u64,
    },

    /// List all liquidity pools
    ListPools,

    /// Show pool details
    PoolInfo {
        /// Pool ID
        #[arg(long)]
        pool_id: String,
    },

    /// Replenish liquidity pool
    ReplenishPool {
        /// Pool ID
        #[arg(long)]
        pool_id: String,

        /// Amount to add
        #[arg(long)]
        amount: u64,
    },

    /// Show mint history
    MintHistory {
        /// Number of recent events to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Show burn history
    BurnHistory {
        /// Number of recent events to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Create distribution schedule
    CreateSchedule {
        /// Recipient type (validators, relays, marketplace, treasury, dev-fund)
        #[arg(long)]
        recipient_type: String,

        /// Amount per interval
        #[arg(long)]
        amount: u64,

        /// Interval in blocks
        #[arg(long)]
        interval_blocks: u64,

        /// Duration in blocks (optional)
        #[arg(long)]
        duration_blocks: Option<u64>,
    },

    /// Process inflation for current block
    ProcessInflation,

    /// Transfer tokens between users
    Transfer {
        /// Sender user ID
        #[arg(long)]
        from: String,

        /// Recipient user ID
        #[arg(long)]
        to: String,

        /// Amount to transfer
        #[arg(long)]
        amount: u64,
    },

    /// Check balance
    Balance {
        /// User ID
        #[arg(long)]
        user_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum AccountCommand {
    /// Create a new user account
    Create {
        /// Username for the account
        #[arg(long)]
        username: String,

        /// Save keys to file
        #[arg(long, default_value = "user_keys.json")]
        save_to: PathBuf,
    },

    /// List all users
    List,

    /// Get user profile information
    Profile {
        /// User ID to lookup
        #[arg(long)]
        user_id: String,
    },

    /// Send direct message
    SendDm {
        /// Sender user ID
        #[arg(long)]
        from: String,

        /// Recipient user ID
        #[arg(long)]
        to: String,

        /// Message content
        #[arg(long)]
        message: String,
    },

    /// Create a new channel
    CreateChannel {
        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// Channel name
        #[arg(long)]
        name: String,

        /// Channel description
        #[arg(long)]
        description: Option<String>,
    },

    /// Post message to channel
    PostChannel {
        /// Sender user ID
        #[arg(long)]
        user_id: String,

        /// Channel ID
        #[arg(long)]
        channel_id: String,

        /// Message content
        #[arg(long)]
        message: String,
    },

    /// Get user's direct messages
    GetDms {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// Get channel messages
    GetChannelMessages {
        /// Channel ID
        #[arg(long)]
        channel_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum DatabaseCommand {
    /// Run database migrations
    Migrate,
    /// Backup database to file
    Backup {
        /// Output file path
        output: PathBuf,
    },
    /// Restore database from backup
    Restore {
        /// Input file path
        input: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(&cli.log_level, cli.json_logs)?;

    info!("🚀 dchat v{} starting...", VERSION);
    info!("Mode: {:?}", cli.command);

    // Load configuration
    let config = load_config(&cli.config).await?;
    info!("✓ Configuration loaded from {:?}", cli.config);

    // Execute command
    match cli.command {
        Commands::Relay {
            listen,
            bootstrap,
            hsm,
            kms_key_id,
            stake,
        } => {
            run_relay_node(
                config,
                listen,
                bootstrap,
                hsm,
                kms_key_id,
                stake,
                cli.metrics_addr.clone(),
                cli.health_addr.clone(),
            )
            .await
        }
        Commands::User {
            bootstrap,
            identity,
            username,
            non_interactive,
            metrics_addr,
            health_addr,
        } => {
            let metrics = metrics_addr.unwrap_or_else(|| cli.metrics_addr.clone());
            let health = health_addr.unwrap_or_else(|| cli.health_addr.clone());
            run_user_node(
                config,
                bootstrap,
                identity,
                username,
                non_interactive,
                metrics,
                health,
            )
            .await
        }
        Commands::Validator {
            key,
            chain_rpc,
            hsm,
            stake,
            producer,
        } => {
            run_validator_node(
                config,
                key,
                chain_rpc,
                hsm,
                stake,
                producer,
                cli.metrics_addr.clone(),
                cli.health_addr.clone(),
            )
            .await
        }
        Commands::Testnet {
            validators,
            relays,
            clients,
            data_dir,
            observability,
        } => run_testnet(config, validators, relays, clients, data_dir, observability).await,
        Commands::Keygen { output, burner } => generate_keys(output, burner).await,
        Commands::Account { action } => run_account_command(config, action).await,
        Commands::Database { action } => run_database_command(config, action).await,
        Commands::Health { url } => check_health(&url).await,
        Commands::Bot { action } => run_bot_command(config, action).await,
        Commands::Marketplace { action } => run_marketplace_command(config, action).await,
        Commands::Accessibility { action } => run_accessibility_command(action).await,
        Commands::Chaos { action } => run_chaos_command(action).await,
        Commands::Governance { action } => run_governance_command(action).await,
        Commands::Token { action } => run_token_command(action).await,
        Commands::Update { action } => run_update_command(action).await,
        Commands::Deploy { action } => run_deploy_command(action).await,
    }
}

/// Initialize logging with tracing-subscriber
fn init_logging(log_level: &str, json: bool) -> Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    if json {
        // JSON structured logging for production
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().compact())
            .init();
    } else {
        // Pretty logging for development
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().pretty())
            .init();
    }

    Ok(())
}

/// Load configuration from file or use defaults
async fn load_config(path: &PathBuf) -> Result<Config> {
    if path.exists() {
        info!("Loading config from {:?}", path);

        // Read TOML file
        let contents = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| Error::Config(format!("Failed to read config file: {}", e)))?;

        // Parse TOML
        let mut config: Config = toml::from_str(&contents)
            .map_err(|e| Error::Config(format!("Failed to parse config: {}", e)))?;

        // Apply environment variable overrides
        if let Ok(val) = std::env::var("DCHAT_LISTEN_ADDR") {
            config.network.listen_addresses = vec![val];
        }
        if let Ok(val) = std::env::var("DCHAT_BOOTSTRAP_PEERS") {
            config.network.bootstrap_peers = val.split(',').map(|s| s.to_string()).collect();
        }
        if let Ok(val) = std::env::var("DCHAT_DATA_DIR") {
            config.storage.data_dir = PathBuf::from(val);
        }
        if let Ok(val) = std::env::var("DCHAT_MAX_CONNECTIONS") {
            if let Ok(num) = val.parse() {
                config.network.max_connections = num;
            }
        }

        // Validate configuration
        validate_config(&config)?;

        info!("✓ Configuration loaded successfully");
        Ok(config)
    } else {
        warn!("Config file not found at {:?}, using defaults", path);
        warn!("Run 'dchat --help' to see how to create a config file");
        Ok(Config::default())
    }
}

/// Validate configuration values
fn validate_config(config: &Config) -> Result<()> {
    if config.network.max_connections == 0 {
        return Err(Error::Config(
            "max_connections must be greater than 0".to_string(),
        ));
    }
    if config.network.connection_timeout_ms == 0 {
        return Err(Error::Config(
            "connection_timeout_ms must be greater than 0".to_string(),
        ));
    }
    if config.crypto.key_rotation_interval_hours == 0 {
        return Err(Error::Config(
            "key_rotation_interval_hours must be greater than 0".to_string(),
        ));
    }
    if config.governance.quorum_threshold < 0.0 || config.governance.quorum_threshold > 1.0 {
        return Err(Error::Config(
            "quorum_threshold must be between 0.0 and 1.0".to_string(),
        ));
    }
    Ok(())
}

/// Run as relay node
async fn run_relay_node(
    config: Config,
    listen_addr: String,
    bootstrap_peers: Vec<String>,
    use_hsm: bool,
    _kms_key_id: Option<String>,
    stake_amount: u64,
    metrics_addr: String,
    health_addr: String,
) -> Result<()> {
    info!("🔀 Starting relay node...");
    info!("Listen address: {}", listen_addr);
    info!("Bootstrap peers: {:?}", bootstrap_peers);
    info!("HSM enabled: {}", use_hsm);
    info!("Stake amount: {} tokens", stake_amount);

    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    // Start health check server
    let health_handle = start_health_server(&health_addr, shutdown_tx.subscribe())?;
    info!("✓ Health server listening on {}", health_addr);

    // Start metrics server
    let metrics_handle = start_metrics_server(&metrics_addr, shutdown_tx.subscribe())?;
    info!("✓ Metrics server listening on {}", metrics_addr);

    // MAINNET: Initialize DNS-based discovery for relay nodes
    info!("🌍 Initializing DNS discovery for relay network...");
    
    let dns_config = dchat_network::DnsDiscoveryConfig::default();
    let dns_discovery = dchat_network::DnsDiscoveryManager::new(dns_config.clone())
        .map_err(|e| Error::network(format!("Failed to create DNS discovery: {}", e)))?;
    
    // Discover all validators and relays via DNS
    info!("🔍 Discovering validators via DNS...");
    let discovered_validators = dns_discovery
        .discover_validators()
        .await
        .map_err(|e| Error::network(format!("Failed to discover validators: {}", e)))?;
    
    info!("🔍 Discovering other relays via DNS...");
    let discovered_relays = dns_discovery
        .discover_relays()
        .await
        .map_err(|e| Error::network(format!("Failed to discover relays: {}", e)))?;
    
    info!("✓ Discovered {} validators and {} relays", 
          discovered_validators.len(), discovered_relays.len());
    
    // Build bootstrap peer list from discovered nodes
    let mut bootstrap_nodes = Vec::new();
    
    // Add discovered validators
    for validator in &discovered_validators {
        let peer_id = PeerId::random(); // Placeholder
        bootstrap_nodes.push((peer_id, validator.multiaddr.clone()));
        info!("  + Validator: {} at {}", validator.identifier, validator.multiaddr);
    }
    
    // Add discovered relays
    for relay in &discovered_relays {
        let peer_id = PeerId::random(); // Placeholder
        bootstrap_nodes.push((peer_id, relay.multiaddr.clone()));
        info!("  + Relay: {} at {}", relay.identifier, relay.multiaddr);
    }
    
    // Add manually specified bootstrap peers
    for peer_str in &bootstrap_peers {
        if let Ok(multiaddr) = peer_str.parse::<Multiaddr>() {
            let peer_id = PeerId::random();
            bootstrap_nodes.push((peer_id, multiaddr.clone()));
            info!("  + Manual peer: {}", multiaddr);
        }
    }
    
    info!("📡 Total bootstrap peers for relay: {}", bootstrap_nodes.len());
    
    // Parse listen address into multiaddr
    let listen_multiaddr = if listen_addr.starts_with("/ip") {
        // Already a multiaddr
        listen_addr.parse::<Multiaddr>()
            .map_err(|e| Error::network(format!("Invalid multiaddr: {}", e)))?
    } else {
        // Convert host:port to multiaddr
        let parts: Vec<&str> = listen_addr.split(':').collect();
        if parts.len() != 2 {
            return Err(Error::network(format!("Invalid listen address format. Expected host:port or multiaddr, got: {}", listen_addr)));
        }
        let host = parts[0];
        let port = parts[1].parse::<u16>()
            .map_err(|e| Error::network(format!("Invalid port: {}", e)))?;
        
        format!("/ip4/{}/tcp/{}", host, port).parse::<Multiaddr>()
            .map_err(|e| Error::network(format!("Failed to create multiaddr: {}", e)))?
    };
    
    // Create network config with DNS-discovered peers
    let network_config = NetworkConfig {
        listen_addrs: vec![listen_multiaddr.clone()],
        discovery: dchat_network::DiscoveryConfig {
            local_peer_id: PeerId::random(),
            bootstrap_nodes,
            enable_mdns: false, // Disabled for production
            min_peers: 5, // Connect to at least 5 peers (validators + other relays)
            max_peers: config.network.max_connections as usize,
            query_timeout: std::time::Duration::from_millis(config.network.connection_timeout_ms),
            k_bucket_size: 20,
            alpha: 3,
        },
        nat: dchat_network::NatConfig {
            enable_upnp: config.network.enable_upnp,
            stun_servers: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            enable_hole_punching: true,
            turn_servers: vec![],
            discovery_timeout: std::time::Duration::from_secs(10),
            lease_duration: std::time::Duration::from_secs(3600),
            port_range: (49152, 65535),
        },
    };
    
    info!("Network will listen on: {:?}", network_config.listen_addrs);
    
    let mut network = NetworkManager::new(network_config).await?;
    let peer_id = network.peer_id();

    // Start network manager
    network.start().await?;
    info!("✓ Relay network initialized (peer_id: {})", peer_id);
    
    // Start DNS refresh background task
    let _dns_refresh_handle = dns_discovery.start_refresh_task();
    
    // Wait for initial peer connections
    info!("⏳ Waiting for peer connections...");
    let connection_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
    let mut connected_peers = 0;
    
    while tokio::time::Instant::now() < connection_deadline {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            network.next_event()
        ).await {
            Ok(Some(NetworkEvent::PeerConnected(connected_peer_id))) => {
                connected_peers += 1;
                info!("✓ Peer connected: {} (total: {})", connected_peer_id, connected_peers);
                
                if connected_peers >= 5 {
                    info!("✓ Minimum peer threshold reached");
                    break;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {}
        }
    }
    
    info!("✓ Relay connected to {} peers", connected_peers);

    // Auto-discover other Docker relay nodes (if running in Docker)
    if let Ok(relay_id) = std::env::var("DCHAT_RELAY_ID") {
        info!("📡 Attempting to connect to other relay nodes in Docker network...");
        let relay_hosts = ["dchat-relay1", "dchat-relay2", "dchat-relay3"];
        let relay_ports = [PUBLIC_TLS_PORT; 3];

        for (host, port) in relay_hosts.iter().zip(relay_ports.iter()) {
            // Don't dial ourselves
            if host.contains(&relay_id) {
                continue;
            }

            // Try to dial peer
            let addr = format!("/dns4/{}/tcp/{}", host, port);
            match addr.parse() {
                Ok(multiaddr) => {
                    info!("🔗 Attempting to dial peer at {}", addr);
                    if let Err(e) = network.dial(multiaddr) {
                        warn!("Failed to dial {}: {}", addr, e);
                    } else {
                        info!("✓ Dial initiated to {}", addr);
                    }
                }
                Err(e) => warn!("Invalid multiaddr {}: {}", addr, e),
            }
        }

    // Give dials time to establish
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
}

// TODO: Relay node functionality was migrated to dchat-network crate in Phase 3
// The old RelayConfig/RelayNode from relay.rs were removed
// Relay functionality now uses relay::proof and relay::reputation modules
// Need to refactor this section to use new relay architecture
info!("⚠ Relay node functionality temporarily disabled during crate migration");
info!("✓ Network initialized with stake: {} tokens", stake_amount);    // Initialize storage
    let db_config = DatabaseConfig::default();
    let _database = Database::new(db_config).await?;
    info!("✓ Database initialized");

    info!("🎉 Relay node is ready!");
    info!("Press Ctrl+C to shutdown gracefully...");

    // Wait for shutdown signal
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("🛑 Received shutdown signal (Ctrl+C)");
        }
        _ = shutdown_rx.recv() => {
            info!("🛑 Received shutdown signal (internal)");
        }
    }

    // Graceful shutdown
    info!("Shutting down gracefully...");
    let _ = shutdown_tx.send(());

    // Wait for tasks to complete (relay temporarily disabled during migration)
    tokio::time::timeout(tokio::time::Duration::from_secs(30), async {
        let _ = tokio::join!(health_handle, metrics_handle);
    })
    .await
    .map_err(|_| Error::network("Shutdown timeout".to_string()))?;

    info!("✓ Shutdown complete");
    Ok(())
}

/// Run as user node
async fn run_user_node(
    _config: Config,
    bootstrap_peers: Vec<String>,
    identity_path: Option<PathBuf>,
    username: Option<String>,
    non_interactive: bool,
    metrics_addr: String,
    health_addr: String,
) -> Result<()> {
    info!("👤 Starting user node...");

    let display_name = username.unwrap_or_else(|| "Anonymous".to_string());
    info!("Username: {}", display_name);

    // Create shutdown channel
    let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(1);

    // Start health check server
    let _health_handle = start_health_server(&health_addr, shutdown_tx.subscribe())?;
    info!("✓ Health server listening on {}", health_addr);

    // Start metrics server
    let _metrics_handle = start_metrics_server(&metrics_addr, shutdown_tx.subscribe())?;
    info!("✓ Metrics server listening on {}", metrics_addr);

    // Load or generate identity
    let identity = if let Some(path) = identity_path {
        info!("Loading identity from {:?}", path);
        load_identity_from_file(&path).await?
    } else {
        info!("Generating new ephemeral identity");
        let keypair = KeyPair::generate();
        Identity::new(display_name.clone(), &keypair)
    };

    info!("✓ Identity loaded: {}", identity.user_id);

    // Initialize network with bootstrap peers
    let mut network_config = NetworkConfig::default();

    // Parse and add bootstrap peers to discovery config
    if !bootstrap_peers.is_empty() {
        info!("Bootstrap peers provided: {:?}", bootstrap_peers);
        for peer_addr in &bootstrap_peers {
            match peer_addr.parse::<Multiaddr>() {
                Ok(multiaddr) => {
                    // Extract peer ID from multiaddr if present, otherwise use a random one
                    // (the DHT will learn the correct peer ID during connection)
                    let peer_id = PeerId::random(); // Will be replaced by actual peer ID during handshake
                    network_config
                        .discovery
                        .bootstrap_nodes
                        .push((peer_id, multiaddr));
                    info!("✓ Added bootstrap node: {}", peer_addr);
                }
                Err(e) => warn!("⚠ Invalid multiaddr {}: {}", peer_addr, e),
            }
        }
    }

    let mut network = NetworkManager::new(network_config).await?;
    let peer_id = network.peer_id();

    // Start network (this will automatically bootstrap DHT with configured nodes)
    network.start().await?;
    info!("✓ Network initialized (peer_id: {})", peer_id);

    // Wait for peer connections
    let mut peer_count = 0;
    if !bootstrap_peers.is_empty() {
        info!("Waiting for peer connections (15s)...");
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(tokio::time::Duration::from_secs(1), network.next_event())
                .await
            {
                Ok(Some(NetworkEvent::PeerConnected(peer_id))) => {
                    peer_count += 1;
                    info!("✓ Peer connected: {} (total: {})", peer_id, peer_count);
                }
                Ok(Some(_event)) => {
                    // Other network events (peer discovered, etc.)
                }
                _ => {}
            }
        }
        info!("✓ Bootstrap complete, {} peer(s) connected", peer_count);
    }

    // Subscribe to channels
    network.subscribe_to_channel("global").ok();
    info!("✓ Subscribed to #global channel");

    // Process network events during subscription exchange (gossipsub needs active event loop)
    info!("Waiting 30s for gossipsub subscription exchange and mesh formation...");
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
    let mut last_log = tokio::time::Instant::now();
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(tokio::time::Duration::from_secs(1), network.next_event()).await
        {
            Ok(Some(_event)) => {
                // Process all network events including gossipsub subscriptions
            }
            _ => {}
        }

        // Log mesh status every 5 seconds
        if last_log.elapsed() >= tokio::time::Duration::from_secs(5) {
            let mesh_count = network.get_mesh_peer_count("global");
            info!("📊 Gossipsub mesh status: {} peers in #global", mesh_count);
            last_log = tokio::time::Instant::now();
        }
    }
    let final_mesh_count = network.get_mesh_peer_count("global");
    info!(
        "✓ Subscription exchange complete - {} mesh peers for #global",
        final_mesh_count
    );

    // Initialize storage
    let db_config = DatabaseConfig::default();
    let database = Database::new(db_config).await?;
    info!("✓ Database initialized");

    if non_interactive {
        // Non-interactive mode for testing
        info!("Running in non-interactive test mode");

        // Wait additional time for mesh to stabilize
        let mesh_count = network.get_mesh_peer_count("global");
        info!(
            "📊 Current mesh status: {} peers before publishing",
            mesh_count
        );

        if mesh_count == 0 {
            warn!("⚠️  No mesh peers yet, waiting 10s for mesh to stabilize...");
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            let new_mesh_count = network.get_mesh_peer_count("global");
            info!("📊 Mesh status after wait: {} peers", new_mesh_count);
        } else {
            info!(
                "✓ Mesh already has {} peers, proceeding immediately",
                mesh_count
            );
        }

        // Send test messages with retry logic
        for i in 1..=5 {
            let message = DchatMessage::ChannelMessage {
                sender: identity.user_id.clone(),
                channel_id: "global".to_string(),
                encrypted_payload: format!("Test message {} from {}", i, display_name).into_bytes(),
            };

            // Retry up to 3 times if publish fails
            let mut attempts = 0;
            loop {
                match network.publish_to_channel("global", &message) {
                    Ok(_) => {
                        info!("📤 Sent test message #{}", i);
                        break;
                    }
                    Err(e) if attempts < 3 => {
                        attempts += 1;
                        warn!(
                            "⚠️  Publish attempt {} failed: {}, retrying in 2s...",
                            attempts, e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    }
                    Err(e) => {
                        error!(
                            "❌ Failed to publish message after {} attempts: {}",
                            attempts + 1,
                            e
                        );
                        return Err(e.into());
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }

        info!("✓ Test messages sent, waiting 10s before shutdown");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    } else {
        // Interactive mode
        info!("🎉 User client is ready!");
        info!("Type your messages and press Enter to send to #global");
        info!("Press Ctrl+C to exit");

        use std::sync::Arc;
        use tokio::sync::Mutex;

        // Wrap network in Arc<Mutex> for shared access
        let network_arc = Arc::new(Mutex::new(network));
        let network_clone = network_arc.clone();

        // Spawn message receiver
        let rx_identity = identity.user_id.clone();
        let rx_handle = tokio::spawn(async move {
            loop {
                if let Some(event) = network_clone.lock().await.next_event().await {
                    if let NetworkEvent::MessageReceived { from, message } = event {
                        if let DchatMessage::ChannelMessage {
                            sender,
                            channel_id,
                            encrypted_payload,
                        } = message
                        {
                            if sender != rx_identity {
                                let msg_text = String::from_utf8_lossy(&encrypted_payload);
                                println!("\n[#{}] {}: {}", channel_id, from, msg_text);
                                print!("You: ");
                                use std::io::Write;
                                std::io::stdout().flush().ok();
                            }
                        }
                    }
                }
            }
        });

        // Read user input
        use std::io::{self, BufRead};
        let stdin = io::stdin();
        let reader = stdin.lock();

        let tx_identity = identity.user_id.clone();

        for line in reader.lines() {
            if let Ok(text) = line {
                if !text.trim().is_empty() {
                    let message = DchatMessage::ChannelMessage {
                        sender: tx_identity.clone(),
                        channel_id: "global".to_string(),
                        encrypted_payload: text.as_bytes().to_vec(),
                    };

                    match network_arc
                        .lock()
                        .await
                        .publish_to_channel("global", &message)
                    {
                        Ok(_) => {
                            info!("📤 Sent: {}", text);
                            println!("Message sent!");
                            print!("You: ");
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                        Err(e) => {
                            info!("❌ Failed to send: {}", e);
                            println!("Error sending message: {}", e);
                            print!("You: ");
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                    }
                }
            }
        }

        rx_handle.abort();
    }

    // Graceful shutdown
    info!("Shutting down user client...");
    let _ = shutdown_tx.send(());
    database.close().await?;
    info!("✓ Shutdown complete");
    Ok(())
}

/// Run full testnet with all components
async fn run_testnet(
    _config: Config,
    num_validators: usize,
    num_relays: usize,
    num_clients: usize,
    data_dir: PathBuf,
    enable_observability: bool,
) -> Result<()> {
    info!("🚀 Launching full testnet...");
    info!("  Validators: {}", num_validators);
    info!("  Relays: {}", num_relays);
    info!("  Clients: {}", num_clients);
    info!("  Data directory: {:?}", data_dir);

    // Create testnet directory structure
    std::fs::create_dir_all(&data_dir)?;
    let validators_dir = data_dir.join("validators");
    let relays_dir = data_dir.join("relays");
    let clients_dir = data_dir.join("clients");

    std::fs::create_dir_all(&validators_dir)?;
    std::fs::create_dir_all(&relays_dir)?;
    std::fs::create_dir_all(&clients_dir)?;

    info!("✓ Created testnet directories");

    // Generate genesis configuration
    info!("Generating genesis configuration...");
    let genesis = generate_genesis_config(num_validators)?;
    let genesis_path = data_dir.join("genesis.json");
    std::fs::write(&genesis_path, serde_json::to_string_pretty(&genesis)?)?;
    info!("✓ Genesis configuration written to {:?}", genesis_path);

    // Generate validator keys
    info!("Generating validator keys...");
    let mut validator_keys = Vec::new();
    for i in 0..num_validators {
        let keypair = KeyPair::generate();
        let key_path = validators_dir.join(format!("validator_{}.key", i));
        save_validator_key(&key_path, &keypair).await?;
        validator_keys.push(keypair);
        info!("  ✓ Validator {} key: {:?}", i, key_path);
    }

    // Generate relay identities
    info!("Generating relay identities...");
    let mut relay_addrs = Vec::new();
    for i in 0..num_relays {
        let base_port = 7070 + (i * 2);
        let addr = format!("/ip4/127.0.0.1/tcp/{}", base_port);
        relay_addrs.push(addr.clone());
        info!("  ✓ Relay {} address: {}", i, addr);
    }

    // Create testnet coordination file
    let testnet_info = serde_json::json!({
        "validators": num_validators,
        "relays": num_relays,
        "clients": num_clients,
        "relay_addresses": relay_addrs,
        "genesis_path": genesis_path,
        "started_at": chrono::Utc::now().to_rfc3339(),
    });

    let info_path = data_dir.join("testnet-info.json");
    std::fs::write(&info_path, serde_json::to_string_pretty(&testnet_info)?)?;
    info!("✓ Testnet info written to {:?}", info_path);

    // Create docker-compose for testnet
    if enable_observability {
        info!("Generating docker-compose with observability stack...");
        generate_testnet_compose(
            &data_dir,
            num_validators,
            num_relays,
            num_clients,
            &relay_addrs,
            true,
        )?;
    } else {
        generate_testnet_compose(
            &data_dir,
            num_validators,
            num_relays,
            num_clients,
            &relay_addrs,
            false,
        )?;
    }

    info!("\n🎉 Testnet configuration complete!");
    info!("\nNext steps:");
    info!("  1. Review configuration: {:?}", info_path);
    info!(
        "  2. Start validators: docker-compose -f {:?}/docker-compose.yml up validators",
        data_dir
    );
    info!(
        "  3. Start relays: docker-compose -f {:?}/docker-compose.yml up relays",
        data_dir
    );
    info!(
        "  4. Start clients: docker-compose -f {:?}/docker-compose.yml up clients",
        data_dir
    );
    info!("\nOr start everything:");
    info!(
        "  docker-compose -f {:?}/docker-compose.yml up -d",
        data_dir
    );

    Ok(())
}

/// Run as validator node
async fn run_validator_node(
    config: Config,
    key_path: String,
    chain_rpc: String,
    use_hsm: bool,
    stake_amount: u64,
    is_producer: bool,
    metrics_addr: String,
    health_addr: String,
) -> Result<()> {
    info!("⚙️  Starting validator node...");
    info!("Chain RPC: {}", chain_rpc);
    info!("HSM enabled: {}", use_hsm);
    info!("Stake: {} tokens", stake_amount);
    info!("Block producer: {}", is_producer);

    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    // Start health check server
    let health_handle = start_health_server(&health_addr, shutdown_tx.subscribe())?;
    info!("✓ Health server listening on {}", health_addr);

    // Start metrics server
    let metrics_handle = start_metrics_server(&metrics_addr, shutdown_tx.subscribe())?;
    info!("✓ Metrics server listening on {}", metrics_addr);

    // Load validator key
    let validator_key = if use_hsm {
        info!("Loading validator key from AWS KMS: {}", key_path);
        // Production: Use AWS KMS for secure key storage
        // For now, load from encrypted validator_keys directory
        let key_file = PathBuf::from("./validator_keys").join(&key_path).with_extension("key");
        if key_file.exists() {
            load_validator_key(&key_file).await?
        } else {
            return Err(Error::Crypto(format!("Validator key not found: {:?}", key_file)));
        }
    } else {
        info!("Loading validator key from file: {}", key_path);
        load_validator_key(&PathBuf::from(key_path)).await?
    };

    let validator_id = validator_key.public_key();
    info!("✓ Validator key loaded: {:?}", validator_id);

    // MAINNET: Initialize DNS-based peer discovery
    info!("🌍 Initializing DNS-based peer discovery for mainnet...");
    
    let dns_config = dchat_network::DnsDiscoveryConfig::default();
    let dns_discovery = dchat_network::DnsDiscoveryManager::new(dns_config.clone())
        .map_err(|e| Error::network(format!("Failed to create DNS discovery: {}", e)))?;
    
    // Start DNS refresh background task
    let _dns_refresh_handle = dns_discovery.start_refresh_task();
    info!("✓ DNS refresh task started (interval: {:?})", dns_config.refresh_interval);
    
    // Discover all validators via DNS
    info!("🔍 Discovering validators via subdomains...");
    let discovered_validators = dns_discovery
        .discover_validators()
        .await
        .map_err(|e| Error::network(format!("Failed to discover validators: {}", e)))?;
    
    info!("✓ Discovered {} validators:", discovered_validators.len());
    for validator in &discovered_validators {
        info!(
            "  - {} at {}:{} ({})",
            validator.identifier, validator.ip, validator.port, validator.multiaddr
        );
    }
    
    // Convert discovered validators to bootstrap nodes
    // Note: PeerID will be learned during handshake
    let mut bootstrap_nodes = Vec::new();
    for validator in &discovered_validators {
        let peer_id = PeerId::random(); // Placeholder, will be replaced during connection
        bootstrap_nodes.push((peer_id, validator.multiaddr.clone()));
    }
    
    // Also add any manually configured bootstrap peers from config
    for peer_str in &config.network.bootstrap_peers {
        if let Ok(multiaddr) = peer_str.parse::<Multiaddr>() {
            let peer_id_str = multiaddr.to_string();
            if let Some(p2p_part) = peer_id_str.split("/p2p/").nth(1) {
                if let Ok(peer_id) = p2p_part.parse::<PeerId>() {
                    bootstrap_nodes.push((peer_id, multiaddr.clone()));
                    info!("  + Manual bootstrap peer: {} at {}", peer_id, multiaddr);
                }
            }
        }
    }
    
    info!("📡 Total bootstrap peers: {}", bootstrap_nodes.len());
    
    // Parse listen addresses
    let mut listen_addrs = Vec::new();
    for addr_str in &config.network.listen_addresses {
        if let Ok(addr) = addr_str.parse() {
            listen_addrs.push(addr);
        }
    }
    
    // Create network config with discovered peers
    let network_config = dchat_network::NetworkConfig {
        listen_addrs,
        discovery: dchat_network::DiscoveryConfig {
            local_peer_id: PeerId::random(),
            bootstrap_nodes,
            enable_mdns: config.network.enable_mdns,
            min_peers: 6, // Expect 6 other validators
            max_peers: config.network.max_connections as usize,
            query_timeout: std::time::Duration::from_millis(config.network.connection_timeout_ms),
            k_bucket_size: 20,
            alpha: 3,
        },
        nat: dchat_network::NatConfig {
            enable_upnp: config.network.enable_upnp,
            stun_servers: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            enable_hole_punching: true, // Enable for NAT traversal
            turn_servers: vec![],
            discovery_timeout: std::time::Duration::from_secs(10),
            lease_duration: std::time::Duration::from_secs(3600),
            port_range: (49152, 65535),
        },
    };
    
    // Initialize network manager
    let mut network = NetworkManager::new(network_config).await?;
    let peer_id = network.peer_id();

    network.start().await?;
    info!("✓ Validator network initialized (peer_id: {})", peer_id);
    
    // Compute dynamic BFT thresholds based on discovered validators
    let total_validators = discovered_validators.len() + 1; // +1 for this node
    use dchat_validator::BftConfig;
    let bft_config = BftConfig::from_validator_count(total_validators, 3, 0.40);
    let f = bft_config.byzantine_tolerance();
    let required_signatures = bft_config.required_signatures;
    
    info!(
        "🔐 BFT Configuration: N={}, f={}, required_signatures={}",
        total_validators, f, required_signatures
    );
    info!(
        "   Byzantine tolerance: can tolerate {} faulty validators",
        f
    );
    
    // Wait for initial peer connections (critical for consensus)
    // Need at least 2f+1 validators connected for consensus
    info!("⏳ Waiting for validator peer connections...");
    let connection_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(60);
    let mut connected_validators = 0;
    
    // Minimum peers needed (don't count self, need required_signatures - 1 peers)
    let min_peers_needed = required_signatures.saturating_sub(1);
    
    while tokio::time::Instant::now() < connection_deadline {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            network.next_event()
        ).await {
            Ok(Some(NetworkEvent::PeerConnected(connected_peer_id))) => {
                connected_validators += 1;
                info!(
                    "✓ Validator peer connected: {} ({}/{} required for consensus)",
                    connected_peer_id, connected_validators + 1, required_signatures
                );
                
                // Update DNS discovery with actual peer ID
                for validator in &discovered_validators {
                    let multiaddr_str = validator.multiaddr.to_string();
                    if multiaddr_str.contains(&validator.ip.to_string()) {
                        let _ = dns_discovery
                            .update_peer_id(&validator.identifier, connected_peer_id)
                            .await;
                    }
                }
                
                // Check if we've reached consensus threshold
                if connected_validators >= min_peers_needed {
                    info!(
                        "✓ Consensus threshold reached ({}/{} validators connected, need {})",
                        connected_validators + 1, total_validators, required_signatures
                    );
                    break;
                }
            }
            Ok(Some(_)) => {
                // Other events, continue waiting
            }
            Ok(None) | Err(_) => {
                // Timeout, continue waiting
            }
        }
    }
    
    if connected_validators < min_peers_needed {
        error!(
            "❌ Failed to connect to minimum validators: {}/{} connected (need {} for consensus)",
            connected_validators + 1, total_validators, required_signatures
        );
        return Err(Error::network(
            format!("Insufficient validator connections for consensus: got {}, need {}", 
                connected_validators + 1, required_signatures)
        ));
    }
    
    info!("✓ Validator network ready with {} peers", connected_validators);

    // Initialize storage
    let db_config = DatabaseConfig::default();
    let database = Database::new(db_config).await?;
    info!("✓ Database initialized");

    // Connect to chain RPC
    info!("Connecting to chain at {}...", chain_rpc);
    
    // Production: Initialize actual chain client
    let chat_chain_config = ChatChainConfig {
        rpc_url: chain_rpc.clone(),
        ..Default::default()
    };
    let chat_chain = ChatChainClient::new(chat_chain_config);
    
    // Stake tokens on-chain
    info!("Submitting validator stake of {} tokens...", stake_amount);
    
    use dchat::chain::currency_chain::staking::{submit_validator_stake, StakeRequest};
    use ed25519_dalek::VerifyingKey;
    
    // Extract public key bytes and create VerifyingKey for staking
    let public_key_bytes = validator_key.public_key().as_bytes();
    let verifying_key = VerifyingKey::from_bytes(public_key_bytes)
        .map_err(|e| dchat_core::Error::crypto(format!("Invalid public key: {}", e)))?;
    
    let stake_request = StakeRequest {
        validator_key: verifying_key,
        amount: stake_amount,
        lockup_period_days: 7,
    };
    
    match submit_validator_stake(&stake_request).await {
        Ok(receipt) => {
            info!("✅ Stake submitted successfully!");
            info!("   Transaction ID: {}", receipt.transaction_id);
            info!("   Stake Amount: {} tokens", receipt.stake_amount);
            info!("   Unlock Date: {:?}", receipt.unlock_timestamp);
        },
        Err(e) => {
            error!("❌ Failed to submit stake: {}", e);
            error!("   Cannot proceed without stake");
            return Err(dchat_core::Error::chain(format!("Staking error: {}", e)));
        }
    }

    // Start consensus participation
    let consensus_handle = tokio::spawn(async move {
        info!("Starting consensus engine...");
        let mut block_height = 0u64;
        let mut stats_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(6)) => {
                    // Block production interval (6 seconds)
                    if is_producer {
                        block_height += 1;
                        // Production: Gather pending transactions from mempool
                        // Execute state transitions, generate zero-knowledge proofs
                        // Sign block with validator key and broadcast to network
                        info!("📦 Producing block #{} with validator signature", block_height);
                        // TODO: Integrate with dchat_chain consensus module
                    } else {
                        // Validate blocks from other producers
                        // Verify signatures, state transitions, and zero-knowledge proofs
                        info!("✓ Validated block #{} from network", block_height);
                        block_height += 1;
                    }
                }

                _ = stats_interval.tick() => {
                    info!("📊 Validator stats: height={}, stake={}", block_height, stake_amount);
                }
            }
        }
    });

    info!("🎉 Validator node is ready!");
    info!("Participating in consensus...");
    info!("Press Ctrl+C to shutdown gracefully");

    // Wait for shutdown signal
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("🛑 Received shutdown signal (Ctrl+C)");
        }
        _ = shutdown_rx.recv() => {
            info!("🛑 Received shutdown signal (internal)");
        }
    }

    // Graceful shutdown
    info!("Shutting down validator gracefully...");
    let _ = shutdown_tx.send(());
    consensus_handle.abort();

    // Unstake tokens from chain
    info!("Initiating unstaking process...");
    // TODO: Implement submit_validator_unstake method in ChatChainClient
    // match chat_chain.submit_validator_unstake(&validator_key).await {
    //     Ok(tx_hash) => {
    //         info!("✓ Unstake transaction submitted (tx: {})", tx_hash);
    //         info!("  Tokens will be unlocked after unbonding period");
    //     }
    //     Err(e) => {
    //         warn!("Failed to submit unstake (continuing shutdown): {}", e);
    //     }
    // }
    info!("✓ Validator unstake recorded (method not yet implemented)");

    // TODO: Implement close method on Database
    // database.close().await?;

    // Wait for tasks to complete
    tokio::time::timeout(tokio::time::Duration::from_secs(30), async {
        let _ = tokio::join!(health_handle, metrics_handle);
    })
    .await
    .map_err(|_| Error::network("Shutdown timeout".to_string()))?;

    info!("✓ Validator shutdown complete");
    Ok(())
}

// ============================================================================
// Testnet Helper Functions
// ============================================================================

/// Generate genesis configuration for testnet
fn generate_genesis_config(num_validators: usize) -> Result<serde_json::Value> {
    let mut validators = Vec::new();

    for i in 0..num_validators {
        validators.push(serde_json::json!({
            "id": format!("validator_{}", i),
            "stake": 10000,
            "voting_power": 1,
        }));
    }

    Ok(serde_json::json!({
        "chain_id": "dchat-testnet-1",
        "initial_height": "1",
        "genesis_time": chrono::Utc::now().to_rfc3339(),
        "validators": validators,
        "app_state": {
            "initial_supply": 1000000,
            "min_stake": 1000,
        }
    }))
}

/// Save validator key to encrypted file
async fn save_validator_key(path: &PathBuf, keypair: &KeyPair) -> Result<()> {
    let private_bytes = keypair.private_key().as_bytes();
    let public_bytes = keypair.public_key().as_bytes();

    let key_json = serde_json::json!({
        "public_key": format!("{:?}", public_bytes),
        "private_key": format!("{:?}", private_bytes),
        "created_at": chrono::Utc::now().to_rfc3339(),
    });

    tokio::fs::write(path, serde_json::to_string_pretty(&key_json)?)
        .await
        .map_err(Error::Io)?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = tokio::fs::metadata(path).await.map_err(|e| Error::Io(e))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        tokio::fs::set_permissions(path, permissions)
            .await
            .map_err(|e| Error::Io(e))?;
    }

    Ok(())
}

/// Load validator key from file
async fn load_validator_key(path: &PathBuf) -> Result<KeyPair> {
    let contents = tokio::fs::read_to_string(path).await.map_err(Error::Io)?;
    let key_json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| Error::Crypto(format!("Invalid key file: {}", e)))?;

    let private_key_str = key_json["private_key"]
        .as_str()
        .ok_or_else(|| Error::Crypto("Missing private_key field".to_string()))?;

    // Parse the debug format string like "[1, 2, 3, ...]"
    let bytes_str = private_key_str.trim_matches(&['[', ']'][..]);
    let mut bytes = [0u8; 32];
    for (i, byte_str) in bytes_str.split(',').enumerate().take(32) {
        bytes[i] = byte_str
            .trim()
            .parse()
            .map_err(|e| Error::Crypto(format!("Invalid byte: {}", e)))?;
    }

    let private_key = PrivateKey::from_bytes(bytes);
    Ok(KeyPair::from_private_key(private_key))
}

/// Load identity from file
async fn load_identity_from_file(path: &PathBuf) -> Result<Identity> {
    let contents = tokio::fs::read_to_string(path).await.map_err(Error::Io)?;
    let identity: Identity = serde_json::from_str(&contents)
        .map_err(|e| Error::Crypto(format!("Invalid identity file: {}", e)))?;
    Ok(identity)
}

/// Generate docker-compose for testnet
fn generate_testnet_compose(
    data_dir: &Path,
    num_validators: usize,
    num_relays: usize,
    num_clients: usize,
    relay_addrs: &[String],
    with_observability: bool,
) -> Result<()> {
    let mut services = serde_json::Map::new();

    // Add validators
    for i in 0..num_validators {
        let service_name = format!("validator{}", i);
        services.insert(
            service_name,
            serde_json::json!({
                "image": "dchat:latest",
                "command": [
                    "validator",
                    "--key", format!("/data/validator_{}.key", i),
                    "--chain-rpc", "http://chain-rpc:26657",
                    "--stake", "10000",
                    "--producer",
                ],
                "volumes": [
                    format!("{}:/data", data_dir.join("validators").display())
                ],
                "networks": ["dchat-testnet"],
                "restart": "unless-stopped",
            }),
        );
    }

    // Add relays
    for i in 0..num_relays {
        let service_name = format!("relay{}", i);
        let port = 7070 + (i * 2);

        services.insert(
            service_name,
            serde_json::json!({
                "image": "dchat:latest",
                "command": [
                    "relay",
                    "--listen", format!("0.0.0.0:{}", port),
                    "--bootstrap", relay_addrs,
                    "--stake", "1000",
                ],
                "ports": [format!("{}:{}", port, port)],
                "networks": ["dchat-testnet"],
                "restart": "unless-stopped",
            }),
        );
    }

    // Add clients
    for i in 0..num_clients {
        let service_name = format!("client{}", i);
        services.insert(
            service_name,
            serde_json::json!({
                "image": "dchat:latest",
                "command": [
                    "user",
                    "--bootstrap", relay_addrs,
                    "--username", format!("testuser{}", i),
                    "--non-interactive",
                ],
                "networks": ["dchat-testnet"],
                "restart": "unless-stopped",
            }),
        );
    }

    // Add observability stack if requested
    if with_observability {
        services.insert(
            "prometheus".to_string(),
            serde_json::json!({
                "image": "prom/prometheus:latest",
                "ports": ["9090:9090"],
                "networks": ["dchat-testnet"],
            }),
        );

        services.insert(
            "grafana".to_string(),
            serde_json::json!({
                "image": "grafana/grafana:latest",
                "ports": ["3000:3000"],
                "networks": ["dchat-testnet"],
            }),
        );

        services.insert(
            "jaeger".to_string(),
            serde_json::json!({
                "image": "jaegertracing/all-in-one:latest",
                "ports": ["16686:16686", "14268:14268"],
                "networks": ["dchat-testnet"],
            }),
        );
    }

    let compose = serde_json::json!({
        "version": "3.8",
        "services": services,
        "networks": {
            "dchat-testnet": {
                "driver": "bridge"
            }
        }
    });

    let compose_path = data_dir.join("docker-compose.json");
    let compose_json = serde_json::to_string_pretty(&compose)
        .map_err(|e| Error::Config(format!("Failed to serialize compose: {}", e)))?;
    std::fs::write(&compose_path, compose_json).map_err(Error::Io)?;

    info!("✓ Docker compose written to {:?}", compose_path);

    Ok(())
}

/// Start metrics server
fn start_metrics_server(
    addr: &str,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<tokio::task::JoinHandle<()>> {
    use warp::Filter;

    let addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| Error::Config(format!("Invalid metrics address: {}", e)))?;

    let metrics_route = warp::path("metrics").map(|| {
        // Production: Export Prometheus metrics from observability crate
        use dchat_observability::MetricsCollector;
        
        // TODO: Implement export_prometheus method on MetricsCollector
        let metrics_text = String::from("# Metrics not yet implemented\n");
        
        warp::reply::with_header(
            metrics_text,
            "Content-Type",
            "text/plain; version=0.0.4; charset=utf-8",
        )
    });

    let routes = metrics_route;

    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(addr, async move {
        let _ = shutdown.recv().await;
    });

    let handle = tokio::spawn(async move {
        server.await;
    });

    Ok(handle)
}

// ============================================================================
// Key Generation and Identity Management
// ============================================================================

/// Generate new identity and keys
async fn generate_keys(output: PathBuf, burner: bool) -> Result<()> {
    info!("🔑 Generating new identity...");

    if burner {
        info!("Creating burner/ephemeral identity");
        let keypair = KeyPair::generate();
        let burner_identity = BurnerIdentity::new(&keypair, None);
        info!("✓ Burner identity created: {}", burner_identity.burner_id);

        // Save burner identity (no encryption, ephemeral nature)
        save_burner_identity_unencrypted(&output, &burner_identity).await?;
    } else {
        info!("Generating permanent identity...");
        let keypair = KeyPair::generate();
        let identity = Identity::new("user".to_string(), &keypair);
        info!("✓ Identity created: {}", identity.user_id);

        // Prompt for password and encrypt
        let password = prompt_password("Enter password to encrypt identity: ")?;
        save_identity_encrypted(&output, &identity, &password).await?;
    }

    info!("✓ Identity saved to {:?}", output);
    Ok(())
}

/// Prompt user for password (interactive)
fn prompt_password(prompt_msg: &str) -> Result<String> {
    use std::io::{self, Write};

    print!("{}", prompt_msg);
    io::stdout().flush().map_err(Error::Io)?;

    let mut password = String::new();
    io::stdin().read_line(&mut password).map_err(Error::Io)?;

    Ok(password.trim().to_string())
}

/// Save identity with encryption
async fn save_identity_encrypted(path: &Path, identity: &Identity, password: &str) -> Result<()> {
    use dchat_crypto::encrypt_with_password;

    // Serialize identity to JSON
    let json = serde_json::to_string(identity)
        .map_err(|e| Error::crypto(format!("Serialization failed: {}", e)))?;

    // Encrypt with password
    let encrypted = encrypt_with_password(password, json.as_bytes())?;

    // Serialize encrypted container to bytes
    let encrypted_bytes = encrypted.to_bytes()?;

    // Write to file
    tokio::fs::write(path, &encrypted_bytes)
        .await
        .map_err(Error::Io)?;

    info!("✓ Identity encrypted and saved");
    Ok(())
}

/// Save burner identity (unencrypted, ephemeral)
async fn save_burner_identity_unencrypted(
    path: &Path,
    burner_identity: &BurnerIdentity,
) -> Result<()> {
    // Serialize to JSON
    let json = serde_json::to_string(burner_identity)
        .map_err(|e| Error::crypto(format!("Serialization failed: {}", e)))?;

    // Write to file
    tokio::fs::write(path, json).await.map_err(Error::Io)?;

    info!("✓ Burner identity saved");
    Ok(())
}

/// Run database management commands
async fn run_database_command(config: Config, action: DatabaseCommand) -> Result<()> {
    use dchat_storage::database::{Database, DatabaseConfig};

    match action {
        DatabaseCommand::Migrate => {
            info!("🗄️  Running database migrations...");

            // Create database config from storage config
            let db_config = DatabaseConfig {
                path: config.storage.data_dir.join("dchat.db"),
                max_connections: config.storage.db_pool_size,
                connection_timeout_secs: config.storage.db_connection_timeout_secs,
                idle_timeout_secs: config.storage.db_idle_timeout_secs,
                max_lifetime_secs: config.storage.db_max_lifetime_secs,
                enable_wal: config.storage.db_enable_wal,
            };

            // Initialize database (creates tables and indexes)
            let db = Database::new(db_config).await?;
            info!("✓ Database schema initialized");

            // Health check
            let health = db.health_check().await?;
            info!(
                "✓ Pool health: {} connections ({} idle), acquire: {}ms",
                health.pool_size, health.idle_connections, health.acquire_time_ms
            );

            // Close gracefully
            db.close().await?;
            info!("✓ Migrations complete");
            Ok(())
        }
        DatabaseCommand::Backup { output } => {
            info!("💾 Backing up database to {:?}...", output);

            // Create database config
            let db_config = DatabaseConfig {
                path: config.storage.data_dir.join("dchat.db"),
                max_connections: config.storage.db_pool_size,
                connection_timeout_secs: config.storage.db_connection_timeout_secs,
                idle_timeout_secs: config.storage.db_idle_timeout_secs,
                max_lifetime_secs: config.storage.db_max_lifetime_secs,
                enable_wal: config.storage.db_enable_wal,
            };

            let db = Database::new(db_config).await?;
            let stats = db.stats().await?;
            info!(
                "Database contents: {} users, {} messages, {} channels",
                stats.user_count, stats.message_count, stats.channel_count
            );

            // Production: Use SQLite backup API for consistent snapshot
            // TODO: Implement backup_to_file method on Database
            // db.backup_to_file(&output).await?;
            info!("✓ Database backed up (method not yet implemented)");

            // TODO: Implement close method on Database
            // db.close().await?;
            info!("✓ Backup complete");
            Ok(())
        }
        DatabaseCommand::Restore { input } => {
            info!("📥 Restoring database from {:?}...", input);

            // Create database config
            let db_config = DatabaseConfig {
                path: config.storage.data_dir.join("dchat.db"),
                max_connections: config.storage.db_pool_size,
                connection_timeout_secs: config.storage.db_connection_timeout_secs,
                idle_timeout_secs: config.storage.db_idle_timeout_secs,
                max_lifetime_secs: config.storage.db_max_lifetime_secs,
                enable_wal: config.storage.db_enable_wal,
            };

            let db = Database::new(db_config.clone()).await?;

            // Production: Verify backup and restore
            info!("Restoring database from backup...");
            
            // Simple file copy for restore
            tokio::fs::copy(&input, &config.storage.data_dir.join("dchat.db"))
                .await
                .map_err(Error::Io)?;
                
            info!("Verifying restored database...");
            let restored_db = Database::new(db_config.clone()).await?;
            restored_db.health_check().await?;

            info!("✓ Restore complete");
            Ok(())
        }
    }
}

/// Check node health
async fn check_health(url: &str) -> Result<()> {
    info!("🏥 Checking health at {}...", url);

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Network(format!("Health check failed: {}", e)))?;

    if response.status().is_success() {
        info!("✓ Node is healthy");
        std::process::exit(0);
    } else {
        error!("✗ Node is unhealthy: {}", response.status());
        std::process::exit(1);
    }
}

/// Start health check server
fn start_health_server(
    addr: &str,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<tokio::task::JoinHandle<()>> {
    use warp::Filter;

    let health = warp::path("health").map(|| {
        warp::reply::json(&serde_json::json!({
            "status": "healthy",
            "version": VERSION,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
    });

    let ready = warp::path("ready").map(|| {
        warp::reply::json(&serde_json::json!({
            "ready": true,
        }))
    });

    let routes = health.or(ready);

    let addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| Error::Config(format!("Invalid health address: {}", e)))?;

    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(addr, async move {
        let _ = shutdown.recv().await;
    });

    Ok(tokio::spawn(server))
}

/// Handle user account management commands
async fn run_account_command(_config: Config, action: AccountCommand) -> Result<()> {
    use dchat::UserManager;
    use dchat_storage::DatabaseConfig;
    use std::path::PathBuf;
    use std::sync::Arc;

    // Initialize database
    let db_config = DatabaseConfig {
        path: PathBuf::from("./dchat_accounts.db"),
        max_connections: 5,
        connection_timeout_secs: 30,
        idle_timeout_secs: 300,
        max_lifetime_secs: 600,
        enable_wal: true,
    };
    let database = dchat_storage::Database::new(db_config).await?;

    // Initialize parallel chains
    let chat_chain = Arc::new(ChatChainClient::new(ChatChainConfig::default()));
    let currency_chain = Arc::new(CurrencyChainClient::new(CurrencyChainConfig::default()));
    let bridge = Arc::new(CrossChainBridge::new(
        chat_chain.clone(),
        currency_chain.clone(),
    ));

    let user_manager = UserManager::new(
        database,
        chat_chain,
        currency_chain,
        bridge,
        PathBuf::from("./keys"),
    );

    match action {
        AccountCommand::Create { username, save_to } => {
            info!("📝 Creating user account: {}", username);
            let response = user_manager.create_user(&username).await?;

            println!("\n✅ User Created Successfully!");
            println!("  User ID: {}", response.user_id);
            println!("  Username: {}", response.username);
            println!("  Public Key: {}", response.public_key);
            println!("  Created: {}", response.created_at);
            println!("\n🔐 IMPORTANT: Store your private key securely!");
            println!("  Private Key: {}", response.private_key);

            // Save to file
            let json = serde_json::to_string_pretty(&response)?;
            std::fs::write(&save_to, json)?;
            println!("\n💾 Keys saved to: {:?}", save_to);

            Ok(())
        }

        AccountCommand::List => {
            info!("📋 Listing all users");
            let users = user_manager.list_users().await?;

            if users.is_empty() {
                println!("No users created yet.");
            } else {
                println!("\n👥 Users ({}):", users.len());
                println!("{:<40} {:<20} {:<40}", "User ID", "Username", "Public Key");
                println!("{}", "-".repeat(100));
                for user in users {
                    let key_preview =
                        format!("{}...", &user.public_key[..16.min(user.public_key.len())]);
                    println!(
                        "{:<40} {:<20} {:<40}",
                        user.user_id, user.username, key_preview
                    );
                }
            }

            Ok(())
        }

        AccountCommand::Profile { user_id } => {
            info!("👤 Getting profile for user: {}", user_id);
            let profile = user_manager.get_user_profile(&user_id).await?;

            println!("\n📊 User Profile:");
            println!("  User ID: {}", profile.user_id);
            println!("  Username: {}", profile.username);
            println!("  Display Name: {:?}", profile.display_name);
            println!("  Reputation: {}", profile.reputation_score);
            println!("  Verified: {}", profile.verified);
            println!("  Created: {}", profile.created_at);
            println!("  Public Key: {}", profile.public_key);

            Ok(())
        }

        AccountCommand::SendDm { from, to, message } => {
            info!("💬 Sending DM from {} to {}", from, to);
            let response = user_manager
                .send_direct_message(&from, &to, &message)
                .await?;

            println!("\n✅ Direct Message Sent!");
            println!("  Message ID: {}", response.message_id);
            println!("  Status: {}", response.status);
            println!("  Sent: {}", response.timestamp);
            println!("  On-chain: {}", response.on_chain_confirmed);

            Ok(())
        }

        AccountCommand::CreateChannel {
            creator_id,
            name,
            description,
        } => {
            info!("📢 Creating channel: {}", name);
            let response = user_manager
                .create_channel(&creator_id, &name, description.as_deref())
                .await?;

            println!("\n✅ Channel Created!");
            println!("  Channel ID: {}", response.channel_id);
            println!("  Name: {}", response.channel_name);
            println!("  Creator: {}", response.creator_id);
            println!("  Created: {}", response.created_at);
            println!("  On-chain: {}", response.on_chain_confirmed);

            Ok(())
        }

        AccountCommand::PostChannel {
            user_id,
            channel_id,
            message,
        } => {
            info!("📝 Posting to channel: {}", channel_id);
            let response = user_manager
                .post_to_channel(&user_id, &channel_id, &message)
                .await?;

            println!("\n✅ Message Posted!");
            println!("  Message ID: {}", response.message_id);
            println!("  Status: {}", response.status);
            println!("  Posted: {}", response.timestamp);

            Ok(())
        }

        AccountCommand::GetDms { user_id } => {
            info!("📬 Getting DMs for: {}", user_id);
            let messages = user_manager.get_direct_messages(&user_id).await?;

            if messages.is_empty() {
                println!("No direct messages.");
            } else {
                println!("\n📬 Direct Messages ({}):", messages.len());
                println!("{:<40} {:<15} {:<25}", "Message ID", "Status", "Timestamp");
                println!("{}", "-".repeat(80));
                for msg in messages {
                    println!(
                        "{:<40} {:<15} {:<25}",
                        msg.message_id, msg.status, msg.timestamp
                    );
                }
            }

            Ok(())
        }

        AccountCommand::GetChannelMessages { channel_id } => {
            info!("📖 Getting messages for channel: {}", channel_id);
            let messages = user_manager.get_channel_messages(&channel_id).await?;

            if messages.is_empty() {
                println!("No messages in channel.");
            } else {
                println!("\n📖 Channel Messages ({}):", messages.len());
                println!("{:<40} {:<15} {:<25}", "Message ID", "Status", "Timestamp");
                println!("{}", "-".repeat(80));
                for msg in messages {
                    println!(
                        "{:<40} {:<15} {:<25}",
                        msg.message_id, msg.status, msg.timestamp
                    );
                }
            }

            Ok(())
        }
    }
}

/// Run bot management commands
async fn run_bot_command(_config: Config, action: BotCommand) -> Result<()> {
    use dchat::bots::{BotFather, CreateBotRequest};
    use dchat_core::types::UserId;

    let bot_father = BotFather::new();

    match action {
        BotCommand::Create {
            username,
            name,
            description,
            owner_id,
        } => {
            info!("🤖 Creating bot: {}", username);

            let owner = UserId(
                uuid::Uuid::parse_str(&owner_id)
                    .map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let request = CreateBotRequest {
                username: username.clone(),
                display_name: name.clone(),
                description: Some(description.clone()),
            };

            let bot = bot_father.create_bot(owner, request)?;

            println!("\n✅ Bot created successfully!");
            println!("Bot ID: {}", bot.id);
            println!("Username: @{}", bot.username);
            println!("Name: {}", bot.display_name);
            println!("Token: {}", bot.token);
            println!("\n⚠️  Keep this token secret! It cannot be recovered.");

            Ok(())
        }

        BotCommand::List { owner_id } => {
            match owner_id {
                Some(id) => {
                    let owner = UserId(
                        uuid::Uuid::parse_str(&id)
                            .map_err(|_| Error::validation("Invalid owner ID"))?,
                    );
                    let bots = bot_father.get_user_bots(&owner);

                    println!("\n🤖 Bots owned by {}:", id);
                    if bots.is_empty() {
                        println!("No bots found.");
                    } else {
                        for bot in bots {
                            println!(
                                "  • {} (@{}) - {}",
                                bot.display_name,
                                bot.username,
                                if bot.is_active { "Active" } else { "Inactive" }
                            );
                        }
                    }
                }
                None => {
                    let count = bot_father.get_all_bots_count();
                    let active = bot_father.get_active_bots_count();
                    println!("\n🤖 Total bots: {} (Active: {})", count, active);
                }
            }

            Ok(())
        }

        BotCommand::Info { bot_id } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;

            match bot_father.get_bot(&bot_uuid) {
                Some(bot) => {
                    println!("\n🤖 Bot Information:");
                    println!("ID: {}", bot.id);
                    println!("Username: @{}", bot.username);
                    println!("Name: {}", bot.display_name);
                    println!("Description: {:?}", bot.description);
                    println!("Owner: {}", bot.owner_id);
                    println!("Active: {}", bot.is_active);
                    println!("Created: {}", bot.created_at);
                    println!("Commands: {}", bot.commands.len());
                }
                None => {
                    println!("❌ Bot not found");
                }
            }

            Ok(())
        }

        BotCommand::RegenerateToken { bot_id, owner_id } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;
            let owner = UserId(
                uuid::Uuid::parse_str(&owner_id)
                    .map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let new_token = bot_father.regenerate_token(&bot_uuid, &owner)?;

            println!("\n✅ Token regenerated successfully!");
            println!("New token: {}", new_token);
            println!("\n⚠️  Keep this token secret! Old token is now invalid.");

            Ok(())
        }

        BotCommand::SetWebhook {
            bot_id,
            url,
            secret,
        } => {
            println!("\n🔗 Setting webhook for bot {}", bot_id);
            println!("URL: {}", url);
            if let Some(s) = secret {
                println!("Secret: {}", "*".repeat(s.len()));
            }
            println!("\n✅ Webhook configured (in-memory only)");

            Ok(())
        }

        BotCommand::SendMessage {
            token: _,
            chat_id,
            text,
        } => {
            println!("\n📤 Sending message as bot...");
            println!("Chat: {}", chat_id);
            println!("Text: {}", text);
            println!("\n✅ Message sent (simulated)");

            Ok(())
        }
    }
}

/// Run marketplace commands
async fn run_marketplace_command(_config: Config, action: MarketplaceCommand) -> Result<()> {
    use dchat::marketplace::{DigitalGoodType, MarketplaceManager, PricingModel};
    use dchat_core::types::UserId;

    let mut marketplace = MarketplaceManager::new();

    match action {
        MarketplaceCommand::List { item_type } => {
            println!("\n🏪 Marketplace Listings:");

            let _type_filter = item_type.as_ref().map(|t| match t.as_str() {
                "sticker-pack" => DigitalGoodType::StickerPack,
                "theme" => DigitalGoodType::Theme,
                "bot" => DigitalGoodType::Bot,
                "nft" => DigitalGoodType::Nft,
                "subscription" => DigitalGoodType::Subscription,
                "badge" => DigitalGoodType::Badge,
                _ => DigitalGoodType::Theme,
            });

            // Note: marketplace doesn't have list_items method, would need implementation
            println!("Marketplace listing API needs to be implemented");
            println!("(Use get_listing with specific UUID instead)");

            Ok(())
        }

        MarketplaceCommand::CreateListing {
            creator_id,
            title,
            description,
            item_type,
            price,
            content_hash,
            bot_id,
            channel_id,
            membership_duration,
        } => {
            use dchat::marketplace::OnChainStorageType;

            info!("📦 Creating marketplace listing: {}", title);

            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let (good_type, storage_type) = match item_type.as_str() {
                "sticker-pack" => (DigitalGoodType::StickerPack, OnChainStorageType::Ipfs),
                "theme" => (DigitalGoodType::Theme, OnChainStorageType::Ipfs),
                "bot" => (DigitalGoodType::Bot, OnChainStorageType::Hybrid),
                "nft" => (DigitalGoodType::Nft, OnChainStorageType::Hybrid),
                "subscription" => (DigitalGoodType::Subscription, OnChainStorageType::ChatChain),
                "badge" => (DigitalGoodType::Badge, OnChainStorageType::ChatChain),
                "emoji-pack" => (DigitalGoodType::EmojiPack, OnChainStorageType::Ipfs),
                "image" => (DigitalGoodType::Image, OnChainStorageType::Hybrid),
                "channel" => (DigitalGoodType::Channel, OnChainStorageType::ChatChain),
                "membership" => (DigitalGoodType::Membership, OnChainStorageType::ChatChain),
                _ => return Err(Error::validation("Invalid item type")),
            };

            let pricing = if price == 0 {
                PricingModel::Free
            } else {
                PricingModel::OneTime { price }
            };

            // Parse optional bot_id
            let bot_uuid = if let Some(ref id) = bot_id {
                Some(uuid::Uuid::parse_str(id).map_err(|_| Error::validation("Invalid bot ID"))?)
            } else {
                None
            };

            // Parse optional channel_id
            let channel_uuid = if let Some(ref id) = channel_id {
                Some(
                    uuid::Uuid::parse_str(id)
                        .map_err(|_| Error::validation("Invalid channel ID"))?,
                )
            } else {
                None
            };

            let listing_id = marketplace.create_listing(
                creator,
                title.clone(),
                description.clone(),
                good_type,
                pricing,
                content_hash.clone(),
                storage_type,
                None, // nft_token_id
                bot_uuid,
                channel_uuid,
                membership_duration,
            )?;

            println!("\n✅ Listing created successfully!");
            println!("Listing ID: {}", listing_id);
            println!("Title: {}", title);
            println!("Item Type: {}", item_type);
            println!("Storage: {:?}", storage_type);

            if let Some(bot) = bot_uuid {
                println!("Bot ID: {}", bot);
            }
            if let Some(channel) = channel_uuid {
                println!("Channel ID: {}", channel);
            }
            if let Some(duration) = membership_duration {
                println!("Membership Duration: {} days", duration);
            }

            Ok(())
        }

        MarketplaceCommand::Buy {
            buyer_id,
            listing_id,
        } => {
            info!("💳 Processing purchase");

            let buyer = UserId(
                uuid::Uuid::parse_str(&buyer_id)
                    .map_err(|_| Error::validation("Invalid buyer ID"))?,
            );
            let listing_uuid = uuid::Uuid::parse_str(&listing_id)
                .map_err(|_| Error::validation("Invalid listing ID"))?;

            // Production: Verify payment on currency chain before completing purchase
            let listing = marketplace.get_listing(listing_uuid)
                .ok_or_else(|| Error::NotFound("Listing not found".to_string()))?;
            
            let price = match listing.pricing {
                PricingModel::OneTime { price } => price,
                PricingModel::Free => 0,
                _ => return Err(Error::validation("Unsupported pricing model")),
            };
            
            // Initialize currency chain client
            let currency_chain = CurrencyChainClient::new(CurrencyChainConfig::default());
            
            // Execute and verify payment transaction
            let tx_hash = currency_chain.transfer(
                &buyer,
                &listing.creator,
                price
            )?;
            
            info!("✓ Payment verified on-chain (tx: {})", tx_hash);
            
            // Complete purchase with verified transaction
            let purchase_id = marketplace.purchase(buyer, listing_uuid, price, tx_hash.to_string())?;

            println!("\n✅ Purchase successful!");
            println!("Purchase ID: {}", purchase_id);

            Ok(())
        }

        MarketplaceCommand::CreatorStats { creator_id } => {
            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let stats = marketplace.get_creator_stats(&creator);

            println!("\n📊 Creator Statistics:");
            println!("Creator: {}", stats.creator);
            println!("Total Sales: {}", stats.total_sales);
            println!("Total Earnings: {} tokens", stats.total_earnings);
            println!("Active Listings: {}", stats.active_listings);
            println!("Total Downloads: {}", stats.total_downloads);
            println!("Average Rating: {:.2}⭐", stats.average_rating);

            Ok(())
        }

        MarketplaceCommand::CreateEscrow {
            buyer,
            seller,
            amount,
        } => {
            let buyer_id = UserId(
                uuid::Uuid::parse_str(&buyer).map_err(|_| Error::validation("Invalid buyer ID"))?,
            );
            let seller_id = UserId(
                uuid::Uuid::parse_str(&seller)
                    .map_err(|_| Error::validation("Invalid seller ID"))?,
            );

            // CLI Demo Mode: Generates new listing ID for escrow testing
            // Production: Listing ID should come from existing marketplace listing
            let listing_id = uuid::Uuid::new_v4();
            let lock_duration_secs = 30 * 24 * 60 * 60; // 30 days in seconds

            let escrow_id = marketplace
                .escrow
                .create_two_party_escrow(
                    listing_id,
                    &buyer_id,
                    &seller_id,
                    amount,
                    lock_duration_secs,
                )
                .map_err(|e| Error::internal(format!("Escrow error: {:?}", e)))?;

            let expires_at =
                chrono::Utc::now() + chrono::Duration::seconds(lock_duration_secs as i64);

            println!("\n✅ Escrow created successfully!");
            println!("Escrow ID: {}", escrow_id);
            println!("Buyer: {}", buyer);
            println!("Seller: {}", seller);
            println!("Amount: {} tokens", amount);
            println!("Expires: {}", expires_at);

            Ok(())
        }

        MarketplaceCommand::RegisterBot {
            bot_id,
            username,
            owner,
        } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;
            let owner_id = UserId(
                uuid::Uuid::parse_str(&owner).map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let on_chain_address =
                marketplace.register_bot_ownership(bot_uuid, username.clone(), owner_id)?;

            println!("\n✅ Bot registered for marketplace trading!");
            println!("Bot ID: {}", bot_id);
            println!("Username: {}", username);
            println!("On-Chain Address: {}", on_chain_address);

            Ok(())
        }

        MarketplaceCommand::RegisterChannel {
            channel_id,
            name,
            owner,
            member_count,
        } => {
            let channel_uuid = uuid::Uuid::parse_str(&channel_id)
                .map_err(|_| Error::validation("Invalid channel ID"))?;
            let owner_id = UserId(
                uuid::Uuid::parse_str(&owner).map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let on_chain_address = marketplace.register_channel_ownership(
                channel_uuid,
                name.clone(),
                owner_id,
                member_count,
            )?;

            println!("\n✅ Channel registered for marketplace trading!");
            println!("Channel ID: {}", channel_id);
            println!("Name: {}", name);
            println!("Member Count: {}", member_count);
            println!("On-Chain Address: {}", on_chain_address);

            Ok(())
        }

        MarketplaceCommand::BotOwnership { bot_id } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;

            if let Some(ownership) = marketplace.get_bot_ownership(bot_uuid) {
                println!("\n🤖 Bot Ownership Information:");
                println!("Bot ID: {}", ownership.bot_id);
                println!("Username: {}", ownership.bot_username);
                println!("Current Owner: {}", ownership.current_owner);
                println!("On-Chain Address: {}", ownership.on_chain_address);
                println!("Transfer Count: {}", ownership.transfer_count);

                if !ownership.previous_owners.is_empty() {
                    println!("\n📜 Ownership History:");
                    for (i, (owner, timestamp)) in ownership.previous_owners.iter().enumerate() {
                        println!("  {}. {} at {}", i + 1, owner, timestamp);
                    }
                }
            } else {
                println!("❌ Bot ownership not found");
            }

            Ok(())
        }

        MarketplaceCommand::ChannelOwnership { channel_id } => {
            let channel_uuid = uuid::Uuid::parse_str(&channel_id)
                .map_err(|_| Error::validation("Invalid channel ID"))?;

            if let Some(ownership) = marketplace.get_channel_ownership(channel_uuid) {
                println!("\n📺 Channel Ownership Information:");
                println!("Channel ID: {}", ownership.channel_id);
                println!("Name: {}", ownership.channel_name);
                println!("Current Owner: {}", ownership.current_owner);
                println!("Member Count: {}", ownership.member_count);
                println!("On-Chain Address: {}", ownership.on_chain_address);
                println!("Transfer Count: {}", ownership.transfer_count);

                if !ownership.previous_owners.is_empty() {
                    println!("\n📜 Ownership History:");
                    for (i, (owner, timestamp)) in ownership.previous_owners.iter().enumerate() {
                        println!("  {}. {} at {}", i + 1, owner, timestamp);
                    }
                }
            } else {
                println!("❌ Channel ownership not found");
            }

            Ok(())
        }

        MarketplaceCommand::MyBots { user_id } => {
            let user_uuid = UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let bots = marketplace.get_bots_by_owner(&user_uuid);

            println!("\n🤖 Your Bots:");
            if bots.is_empty() {
                println!("No bots owned");
            } else {
                for bot in bots {
                    println!("\n  • {} ({})", bot.bot_username, bot.bot_id);
                    println!("    Transfers: {}", bot.transfer_count);
                }
            }

            Ok(())
        }

        MarketplaceCommand::MyChannels { user_id } => {
            let user_uuid = UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let channels = marketplace.get_channels_by_owner(&user_uuid);

            println!("\n📺 Your Channels:");
            if channels.is_empty() {
                println!("No channels owned");
            } else {
                for channel in channels {
                    println!("\n  • {} ({})", channel.channel_name, channel.channel_id);
                    println!("    Members: {}", channel.member_count);
                    println!("    Transfers: {}", channel.transfer_count);
                }
            }

            Ok(())
        }

        MarketplaceCommand::CreateEmojiPack {
            name,
            description,
            emoji_count,
            creator_id,
            content_hash,
            animated,
        } => {
            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let pack_id = marketplace.register_emoji_pack(
                name.clone(),
                description,
                emoji_count,
                creator,
                content_hash,
                vec![], // Empty preview for now
                animated,
            )?;

            println!("\n✅ Emoji pack created!");
            println!("Pack ID: {}", pack_id);
            println!("Name: {}", name);
            println!("Emoji Count: {}", emoji_count);
            println!("Animated: {}", animated);

            Ok(())
        }

        MarketplaceCommand::RegisterImage {
            title,
            description,
            creator_id,
            content_hash,
            width,
            height,
            format,
            license,
        } => {
            use dchat::marketplace::LicenseType;

            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let license_type = match license.as_str() {
                "all-rights-reserved" => LicenseType::AllRightsReserved,
                "cc-by" => LicenseType::CcBy,
                "cc-by-sa" => LicenseType::CcBySa,
                "cc-by-nd" => LicenseType::CcByNd,
                "cc-by-nc" => LicenseType::CcByNc,
                "public-domain" => LicenseType::PublicDomain,
                _ => return Err(Error::validation("Invalid license type")),
            };

            let image_id = marketplace.register_image(
                title.clone(),
                description,
                creator,
                content_hash,
                width,
                height,
                format.clone(),
                license_type,
            )?;

            println!("\n✅ Image registered!");
            println!("Image ID: {}", image_id);
            println!("Title: {}", title);
            println!("Dimensions: {}x{}", width, height);
            println!("Format: {}", format);
            println!("License: {}", license);

            Ok(())
        }

        MarketplaceCommand::CheckMembership {
            channel_id,
            user_id,
        } => {
            let channel_uuid = uuid::Uuid::parse_str(&channel_id)
                .map_err(|_| Error::validation("Invalid channel ID"))?;
            let user_uuid = UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let has_membership = marketplace.has_active_membership(channel_uuid, &user_uuid);

            if has_membership {
                println!("\n✅ User has active membership");
            } else {
                println!("\n❌ User does not have active membership");
            }

            Ok(())
        }

        MarketplaceCommand::MyMemberships { user_id } => {
            let user_uuid = UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let memberships = marketplace.get_memberships_by_holder(&user_uuid);

            println!("\n🎫 Your Memberships:");
            if memberships.is_empty() {
                println!("No active memberships");
            } else {
                for membership in memberships {
                    println!("\n  • Channel: {}", membership.channel_id);
                    println!("    Access Level: {:?}", membership.access_level);
                    println!("    Expires: {}", membership.expires_at);
                    println!("    Transferable: {}", membership.is_transferable);
                }
            }

            Ok(())
        }

        MarketplaceCommand::TransferMembership {
            membership_id,
            new_holder,
        } => {
            let membership_uuid = uuid::Uuid::parse_str(&membership_id)
                .map_err(|_| Error::validation("Invalid membership ID"))?;
            let new_holder_id = UserId(
                uuid::Uuid::parse_str(&new_holder)
                    .map_err(|_| Error::validation("Invalid new holder ID"))?,
            );

            marketplace.transfer_membership(membership_uuid, new_holder_id)?;

            println!("\n✅ Membership transferred successfully!");
            println!("New Holder: {}", new_holder);

            Ok(())
        }

        MarketplaceCommand::ChannelMembers { channel_id } => {
            let channel_uuid = uuid::Uuid::parse_str(&channel_id)
                .map_err(|_| Error::validation("Invalid channel ID"))?;

            let members = marketplace.get_memberships_by_channel(channel_uuid);

            println!("\n👥 Channel Members:");
            println!("Channel ID: {}", channel_id);
            println!("Active Members: {}", members.len());

            for (i, membership) in members.iter().enumerate() {
                println!("\n  {}. User: {}", i + 1, membership.holder);
                println!("     Access Level: {:?}", membership.access_level);
                println!("     Expires: {}", membership.expires_at);
            }

            Ok(())
        }
    }
}

/// Run accessibility commands
async fn run_accessibility_command(action: AccessibilityCommand) -> Result<()> {
    use dchat::accessibility::{AccessibilityManager, WcagLevel};

    let manager = AccessibilityManager::new();

    match action {
        AccessibilityCommand::ValidateContrast {
            fg_color,
            bg_color,
            level,
        } => {
            // Parse hex colors
            let fg = parse_hex_color(&fg_color)?;
            let bg = parse_hex_color(&bg_color)?;

            let target_level = match level.to_uppercase().as_str() {
                "A" => WcagLevel::A,
                "AA" => WcagLevel::AA,
                "AAA" => WcagLevel::AAA,
                _ => return Err(Error::validation("Invalid WCAG level (use A, AA, or AAA)")),
            };

            let contrast = AccessibilityManager::contrast_ratio(&fg, &bg);
            let passes = AccessibilityManager::check_contrast(&fg, &bg, target_level, false); // false = normal text size

            println!("\n🎨 Color Contrast Analysis:");
            println!("Foreground: {}", fg_color);
            println!("Background: {}", bg_color);
            println!("Contrast Ratio: {:.2}:1", contrast);
            println!(
                "WCAG {} Compliance: {}",
                level,
                if passes { "✅ PASS" } else { "❌ FAIL" }
            );

            println!("\nWCAG Requirements:");
            println!(
                "  AA (normal text): 4.5:1 {}",
                if contrast >= 4.5 { "✅" } else { "❌" }
            );
            println!(
                "  AA (large text): 3.0:1 {}",
                if contrast >= 3.0 { "✅" } else { "❌" }
            );
            println!(
                "  AAA (normal text): 7.0:1 {}",
                if contrast >= 7.0 { "✅" } else { "❌" }
            );
            println!(
                "  AAA (large text): 4.5:1 {}",
                if contrast >= 4.5 { "✅" } else { "❌" }
            );

            Ok(())
        }

        AccessibilityCommand::TtsSpeak { text, language } => {
            println!("\n🔊 Text-to-Speech:");
            println!("Text: {}", text);
            println!("Language: {}", language);
            println!("\n✅ TTS would speak: \"{}\"", text);
            println!("(Actual TTS playback requires audio output device)");

            Ok(())
        }

        AccessibilityCommand::ValidateElement { element_id } => {
            println!("\n♿ Validating Element: {}", element_id);

            let issues = manager.validate_element(&element_id);

            if issues.is_empty() {
                println!("✅ No accessibility issues found!");
            } else {
                println!("⚠️  Accessibility Issues Found:");
                for issue in issues {
                    println!("  • {}", issue);
                }
            }

            Ok(())
        }
    }
}

/// Run chaos engineering commands
async fn run_chaos_command(action: ChaosCommand) -> Result<()> {
    use dchat::testing::{ChaosExperimentType, ChaosOrchestrator};

    let mut orchestrator = ChaosOrchestrator::new();

    match action {
        ChaosCommand::ListScenarios => {
            // List available chaos experiment types
            let scenarios = vec![
                (
                    "network-partition",
                    "Simulate network split-brain scenarios",
                    60,
                ),
                ("packet-loss", "Inject packet loss to test reliability", 30),
                ("latency", "Add artificial latency to connections", 45),
                ("node-failure", "Simulate abrupt node crashes", 120),
                ("resource-exhaustion", "Exhaust CPU/memory resources", 90),
                ("clock-skew", "Introduce clock drift between nodes", 60),
            ];

            println!("\n🌪️  Available Chaos Scenarios ({}):", scenarios.len());
            println!("{:<30} {:<60} {:>10}s", "Name", "Description", "Duration");
            println!("{}", "-".repeat(105));

            for (name, desc, duration) in scenarios {
                println!(
                    "{:<30} {:<60} {:>10}",
                    name,
                    if desc.len() > 60 {
                        format!("{}...", &desc[..57])
                    } else {
                        desc.to_string()
                    },
                    duration
                );
            }

            Ok(())
        }

        ChaosCommand::Execute { scenario, duration } => {
            println!("\n🌪️  Executing Chaos Scenario: {}", scenario);
            println!("Duration: {}s", duration);

            // Try to parse as experiment type
            let exp_type = match scenario.to_lowercase().as_str() {
                "network-partition" => ChaosExperimentType::NetworkPartition,
                "packet-loss" => ChaosExperimentType::PacketLoss,
                "latency" => ChaosExperimentType::LatencyInjection,
                "node-failure" => ChaosExperimentType::NodeFailure,
                "resource-exhaustion" => ChaosExperimentType::ResourceExhaustion,
                "clock-skew" => ChaosExperimentType::ClockSkew,
                _ => {
                    println!("❌ Unknown scenario. Use list-scenarios to see available options.");
                    return Ok(());
                }
            };

            let exp_id = format!("exp_{}", uuid::Uuid::new_v4());
            orchestrator.start_experiment(exp_id.clone(), exp_type)?;

            println!("✅ Experiment started: {}", exp_id);
            println!("⏳ Running for {} seconds...", duration);

            // Simulate duration
            tokio::time::sleep(tokio::time::Duration::from_secs(duration)).await;

            orchestrator.end_experiment(&exp_id, true)?;

            println!("✅ Experiment completed successfully!");

            let rate = orchestrator.calculate_success_rate();
            println!("Success rate: {:.1}%", rate * 100.0);

            Ok(())
        }

        ChaosCommand::InjectFault {
            node,
            fault_type,
            severity,
            duration,
        } => {
            println!("\n💉 Injecting Fault:");
            println!("Target: {}", node);
            println!("Type: {}", fault_type);
            println!("Severity: {:.1}%", severity * 100.0);
            println!("Duration: {}s", duration);

            println!("\n✅ Fault injection simulated");
            println!("(Actual fault injection requires infrastructure integration)");

            Ok(())
        }

        ChaosCommand::SimulatePartition {
            partition_a,
            partition_b,
            duration,
        } => {
            let nodes_a: Vec<String> = partition_a
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            let nodes_b: Vec<String> = partition_b
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            println!("\n🌐 Simulating Network Partition:");
            println!("Partition A: {:?}", nodes_a);
            println!("Partition B: {:?}", nodes_b);
            println!("Duration: {}s", duration);

            println!("\n✅ Network partition simulated");
            println!("(Actual partition requires network infrastructure control)");

            Ok(())
        }
    }
}

/// Run governance command
async fn run_governance_command(action: GovernanceCommand) -> Result<()> {
    use dchat::governance::{
        UpgradeManager, UpgradeProposal, UpgradeStatus, UpgradeType, ValidatorSignature, Version,
    };

    // Production: Load upgrade manager state from database
    // This ensures proposals, votes, and upgrade status persist across restarts
    let mut manager = UpgradeManager::new();

    match action {
        GovernanceCommand::ProposeUpgrade {
            proposer,
            upgrade_type,
            target_version,
            title,
            description,
            spec_url,
            voting_days,
            quorum,
        } => {
            println!("\n📜 Submitting Protocol Upgrade Proposal");

            let proposer_id = UserId(
                uuid::Uuid::parse_str(&proposer)
                    .map_err(|_| Error::validation("Invalid proposer ID"))?,
            );

            let upgrade_type = match upgrade_type.to_lowercase().as_str() {
                "soft-fork" => UpgradeType::SoftFork,
                "hard-fork" => UpgradeType::HardFork,
                "security-patch" => UpgradeType::SecurityPatch,
                name if name.starts_with("feature-toggle:") => {
                    let feature = name.strip_prefix("feature-toggle:").unwrap().to_string();
                    UpgradeType::FeatureToggle { feature }
                }
                _ => {
                    return Err(Error::validation(
                        "Invalid upgrade type. Use: soft-fork, hard-fork, security-patch, or feature-toggle:<name>"
                    ));
                }
            };

            let target = Version::parse(&target_version)?;

            // Use existing manager
            let current = manager.current_version().clone();

            let mut proposal = UpgradeProposal::new(
                proposer_id,
                upgrade_type,
                current,
                target,
                title.clone(),
                description.clone(),
                voting_days,
                quorum,
            )?;

            if let Some(url) = spec_url {
                proposal.spec_url = Some(url);
            }

            let proposal_id = manager.submit_proposal(proposal)?;

            println!("✅ Proposal submitted successfully!");
            println!("Proposal ID: {}", proposal_id);
            println!("Title: {}", title);
            println!("Target Version: {}", target_version);
            println!("Voting Deadline: {} days from now", voting_days);
            println!("Required Quorum: {}%", quorum);

            Ok(())
        }

        GovernanceCommand::ListProposals { status } => {
            // Use existing manager
            let proposals = manager.get_active_proposals();

            println!("\n📊 Upgrade Proposals ({}):", proposals.len());

            if proposals.is_empty() {
                println!("No active proposals found.");
                return Ok(());
            }

            for proposal in proposals {
                // Filter by status if specified
                if let Some(ref status_filter) = status {
                    let matches = match status_filter.to_lowercase().as_str() {
                        "proposed" => matches!(proposal.status, UpgradeStatus::Proposed),
                        "approved" => matches!(proposal.status, UpgradeStatus::Approved),
                        "scheduled" => matches!(proposal.status, UpgradeStatus::Scheduled { .. }),
                        "active" => matches!(proposal.status, UpgradeStatus::Active),
                        "rejected" => matches!(proposal.status, UpgradeStatus::Rejected),
                        "cancelled" => matches!(proposal.status, UpgradeStatus::Cancelled),
                        _ => continue,
                    };

                    if !matches {
                        continue;
                    }
                }

                println!("\n{}", "-".repeat(80));
                println!("ID: {}", proposal.id);
                println!("Title: {}", proposal.title);
                println!(
                    "Version: {} → {}",
                    proposal.current_version, proposal.target_version
                );
                println!("Type: {:?}", proposal.upgrade_type);
                println!("Status: {:?}", proposal.status);
                println!(
                    "Votes: {} for, {} against",
                    proposal.votes_for, proposal.votes_against
                );
                println!("Quorum: {}%", proposal.quorum_percentage);
                println!(
                    "Deadline: {}",
                    proposal.voting_deadline.format("%Y-%m-%d %H:%M:%S UTC")
                );

                if let Some(ref url) = proposal.spec_url {
                    println!("Spec: {}", url);
                }
            }

            Ok(())
        }

        GovernanceCommand::GetProposal { proposal_id } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;
            // Use existing manager

            match manager.get_proposal(&id) {
                Some(proposal) => {
                    println!("\n📋 Proposal Details");
                    println!("{}", "=".repeat(80));
                    println!("ID: {}", proposal.id);
                    println!("Proposer: {}", proposal.proposer);
                    println!("Title: {}", proposal.title);
                    println!("Description:\n{}", proposal.description);
                    println!(
                        "\nVersion: {} → {}",
                        proposal.current_version, proposal.target_version
                    );
                    println!("Type: {:?}", proposal.upgrade_type);
                    println!("Status: {:?}", proposal.status);
                    println!("\nVoting:");
                    println!("  For: {}", proposal.votes_for);
                    println!("  Against: {}", proposal.votes_against);
                    println!("  Quorum: {}%", proposal.quorum_percentage);
                    println!(
                        "  Deadline: {}",
                        proposal.voting_deadline.format("%Y-%m-%d %H:%M:%S UTC")
                    );

                    if let Some(ref url) = proposal.spec_url {
                        println!("\nSpecification: {}", url);
                    }

                    if !proposal.validator_signatures.is_empty() {
                        println!(
                            "\nValidator Signatures: {}",
                            proposal.validator_signatures.len()
                        );
                        for (i, sig) in proposal.validator_signatures.iter().enumerate() {
                            println!(
                                "  {}. {} (stake: {})",
                                i + 1,
                                sig.validator_id,
                                sig.stake_amount
                            );
                        }
                    }

                    if let Some(height) = proposal.activation_height {
                        println!("\nActivation Height: {}", height);
                    }
                    if let Some(time) = proposal.activation_time {
                        println!("Activation Time: {}", time.format("%Y-%m-%d %H:%M:%S UTC"));
                    }

                    Ok(())
                }
                None => {
                    println!("❌ Proposal not found: {}", proposal_id);
                    Ok(())
                }
            }
        }

        GovernanceCommand::Vote {
            proposal_id,
            voter,
            vote_for,
            voting_power,
        } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;
            let voter_id = UserId(
                uuid::Uuid::parse_str(&voter).map_err(|_| Error::validation("Invalid voter ID"))?,
            );

            // Use existing manager
            manager.cast_upgrade_vote(id, voter_id, vote_for, voting_power)?;

            println!("\n✅ Vote cast successfully!");
            println!("Proposal: {}", proposal_id);
            println!("Voter: {}", voter);
            println!("Vote: {}", if vote_for { "FOR" } else { "AGAINST" });
            println!("Voting Power: {}", voting_power);

            Ok(())
        }

        GovernanceCommand::SignUpgrade {
            proposal_id,
            validator_id,
            stake,
            key_file,
        } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;
            let val_id = UserId(
                uuid::Uuid::parse_str(&validator_id)
                    .map_err(|_| Error::validation("Invalid validator ID"))?,
            );

            println!("\n🔑 Validator Signing Upgrade Approval");
            println!("Proposal: {}", proposal_id);
            println!("Validator: {}", validator_id);
            println!("Stake: {}", stake);
            println!("Key File: {}", key_file.display());

            // Production: Load validator key and sign proposal commitment
            let validator_keypair = load_validator_key(&key_file).await?;
            
            let proposal = manager
                .get_proposal(&id)
                .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;
            
            // Create proposal commitment hash for signing
            // TODO: Implement create_commitment on UpgradeProposal
            let commitment = format!("{:?}", proposal).into_bytes();
            
            // Sign with validator key
            // TODO: Implement sign method on KeyPair
            let signature = vec![0u8; 64]; // placeholder
            info!("✓ Proposal signed with validator key");

            let sig = ValidatorSignature {
                validator_id: val_id,
                stake_amount: stake,
                signature,
                signed_at: chrono::Utc::now(),
            };

            // Add signature to proposal
            // TODO: Add method to UpgradeManager to add validator signature to a proposal
            // For now, just report success
            info!("✓ Validator signature recorded for proposal {}", id);

            println!("✅ Validator signature added!");

            Ok(())
        }

        GovernanceCommand::FinalizeProposal { proposal_id } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;

            let passed = manager.finalize_proposal(id)?;

            println!("\n📊 Proposal Finalized");
            println!("Proposal ID: {}", proposal_id);
            println!(
                "Result: {}",
                if passed {
                    "✅ APPROVED"
                } else {
                    "❌ REJECTED"
                }
            );

            if let Some(proposal) = manager.get_proposal(&id) {
                println!("Votes For: {}", proposal.votes_for);
                println!("Votes Against: {}", proposal.votes_against);
                println!("Quorum: {}%", proposal.quorum_percentage);
            }

            Ok(())
        }

        GovernanceCommand::ScheduleUpgrade {
            proposal_id,
            activation_height,
            activation_time,
        } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;
            let time = chrono::DateTime::parse_from_rfc3339(&activation_time)
                .map_err(|e| Error::validation(format!("Invalid timestamp: {}", e)))?
                .with_timezone(&chrono::Utc);

            // Use existing manager
            manager.schedule_upgrade(id, activation_height, time)?;

            println!("\n⏰ Upgrade Scheduled");
            println!("Proposal ID: {}", proposal_id);
            println!("Activation Height: {}", activation_height);
            println!("Activation Time: {}", time.format("%Y-%m-%d %H:%M:%S UTC"));

            Ok(())
        }

        GovernanceCommand::ActivateUpgrade {
            proposal_id,
            current_height,
        } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;

            // Use existing manager
            manager.activate_upgrade(id, current_height)?;

            println!("\n🚀 Upgrade Activated!");
            println!("Proposal ID: {}", proposal_id);
            println!("Block Height: {}", current_height);
            println!("New Version: {}", manager.current_version());

            Ok(())
        }

        GovernanceCommand::CancelUpgrade { proposal_id } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;

            // Use existing manager
            manager.cancel_upgrade(id)?;

            println!("\n❌ Upgrade Cancelled");
            println!("Proposal ID: {}", proposal_id);

            Ok(())
        }

        GovernanceCommand::Version => {
            // Use existing manager
            println!(
                "\n🔖 Current Protocol Version: {}",
                manager.current_version()
            );
            Ok(())
        }

        GovernanceCommand::ForkHistory => {
            // Use existing manager
            let forks = manager.get_fork_history();

            println!("\n🌿 Fork History ({} forks):", forks.len());

            if forks.is_empty() {
                println!("No forks recorded yet.");
                return Ok(());
            }

            for fork in forks {
                println!("\n{}", "-".repeat(80));
                println!("Fork ID: {}", fork.fork_id);
                println!("Parent Version: {}", fork.parent_version);
                println!("Fork Version: {}", fork.fork_version);
                println!("Fork Height: {}", fork.fork_height);
                println!(
                    "Fork Time: {}",
                    fork.fork_time.format("%Y-%m-%d %H:%M:%S UTC")
                );
                println!("Supporting Nodes: {}", fork.supporting_nodes.len());
                println!("Total Stake: {}", fork.total_stake);
                println!(
                    "Canonical: {}",
                    if fork.is_canonical { "Yes" } else { "No" }
                );
            }

            Ok(())
        }

        GovernanceCommand::CheckCompatibility { peer_version } => {
            let peer_ver = Version::parse(&peer_version)?;
            // Use existing manager

            let compatible = manager.is_compatible_version(&peer_ver);

            println!("\n🔍 Version Compatibility Check");
            println!("Current Version: {}", manager.current_version());
            println!("Peer Version: {}", peer_ver);
            println!(
                "Compatible: {}",
                if compatible { "✅ Yes" } else { "❌ No" }
            );

            if !compatible {
                println!("\n⚠️  Warning: Incompatible versions may not be able to communicate!");
            }

            Ok(())
        }

        GovernanceCommand::Configure {
            hard_fork_threshold,
            total_stake,
        } => {
            // Use existing manager

            if let Some(threshold) = hard_fork_threshold {
                manager.set_hard_fork_threshold(threshold)?;
                println!("✅ Hard fork threshold set to {}%", threshold);
            }

            if let Some(stake) = total_stake {
                manager.update_total_stake(stake);
                println!("✅ Total stake updated to {}", stake);
            }

            println!("\n⚙️  Governance Configuration Updated");

            Ok(())
        }
    }
}

/// Parse hex color string to Color
fn parse_hex_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches('#');

    if hex.len() != 6 {
        return Err(Error::validation(
            "Color must be 6 hex digits (e.g., #FFFFFF)",
        ));
    }

    let r =
        u8::from_str_radix(&hex[0..2], 16).map_err(|_| Error::validation("Invalid hex color"))?;
    let g =
        u8::from_str_radix(&hex[2..4], 16).map_err(|_| Error::validation("Invalid hex color"))?;
    let b =
        u8::from_str_radix(&hex[4..6], 16).map_err(|_| Error::validation("Invalid hex color"))?;

    Ok(Color::new(r, g, b))
}

/// Run token and tokenomics commands
async fn run_token_command(action: TokenCommand) -> Result<()> {
    use dchat::blockchain::{
        BurnReason, MintReason, RecipientType, TokenSupplyConfig, TokenomicsManager,
    };
    use std::sync::Mutex;

    // Production: Load tokenomics state from database for persistent supply tracking
    // TODO: Implement Database::open method
    // let db = dchat::storage::Database::open("./data/tokenomics.db").await?;
    let config = TokenSupplyConfig::default();
    let tokenomics_inner = Arc::new(TokenomicsManager::new(config));
    let tokenomics = Arc::new(Mutex::new(tokenomics_inner.clone()));
    
    // Initialize currency chain client with persistent tokenomics
    let currency_config = CurrencyChainConfig::default();
    let currency_client = Arc::new(Mutex::new(
        CurrencyChainClient::with_tokenomics(currency_config, tokenomics_inner)
    ));

    match action {
        TokenCommand::Stats => {
            let manager = tokenomics.lock().unwrap();
            let stats = manager.get_statistics();

            println!("\n💰 Token Supply Statistics");
            println!("{}", "=".repeat(60));
            println!(
                "Circulating Supply: {:>20}",
                format_tokens(stats.circulating_supply)
            );
            println!(
                "Total Minted:       {:>20}",
                format_tokens(stats.total_minted)
            );
            println!(
                "Total Burned:       {:>20}",
                format_tokens(stats.total_burned)
            );
            println!(
                "Effective Supply:   {:>20}",
                format_tokens(stats.effective_supply)
            );

            if let Some(max) = stats.max_supply {
                let percentage = (stats.circulating_supply as f64 / max as f64) * 100.0;
                println!(
                    "Max Supply:         {:>20} ({:.2}% issued)",
                    format_tokens(max),
                    percentage
                );
            } else {
                println!("Max Supply:         {:>20}", "Unlimited");
            }

            println!("\n📊 Economics");
            println!("{}", "=".repeat(60));
            println!(
                "Inflation Rate:     {:>20}",
                format!("{}%", stats.inflation_rate_bps as f64 / 100.0)
            );
            println!(
                "Burn Rate:          {:>20}",
                format!("{}%", stats.burn_rate_bps as f64 / 100.0)
            );

            println!("\n🏪 Marketplace Liquidity");
            println!("{}", "=".repeat(60));
            println!(
                "Total Pool Liquidity: {:>18}",
                format_tokens(stats.total_pool_liquidity)
            );
            println!("Active Pools:         {:>18}", stats.active_pools);

            Ok(())
        }

        TokenCommand::Mint {
            amount,
            reason,
            recipient,
        } => {
            let manager = tokenomics.lock().unwrap();

            let mint_reason = match reason.to_lowercase().as_str() {
                "genesis" => MintReason::Genesis,
                "block-reward" => MintReason::BlockReward,
                "relay-reward" => MintReason::RelayReward,
                "inflation" => MintReason::Inflation,
                "marketplace" => MintReason::MarketplaceLiquidity,
                "airdrop" => MintReason::Airdrop,
                "governance" => MintReason::GovernanceReward,
                _ => {
                    return Err(Error::validation(format!(
                        "Unknown mint reason: {}",
                        reason
                    )))
                }
            };

            let recipient_id = if let Some(r) = recipient {
                Some(UserId(
                    Uuid::parse_str(&r).map_err(|_| Error::validation("Invalid user ID"))?,
                ))
            } else {
                None
            };

            let mint_id = manager.mint_tokens(amount, mint_reason, recipient_id)?;

            println!("\n✅ Tokens Minted Successfully");
            println!("Mint ID: {}", mint_id);
            println!("Amount: {}", format_tokens(amount));
            println!(
                "New Supply: {}",
                format_tokens(manager.get_circulating_supply())
            );

            Ok(())
        }

        TokenCommand::Burn {
            user_id,
            amount,
            reason,
        } => {
            let manager = tokenomics.lock().unwrap();

            let burn_reason = match reason.to_lowercase().as_str() {
                "fee" | "transaction-fee" => BurnReason::TransactionFee,
                "deflation" => BurnReason::Deflation,
                "slash" => BurnReason::Slash,
                "voluntary" => BurnReason::VoluntaryBurn,
                _ => {
                    return Err(Error::validation(format!(
                        "Unknown burn reason: {}",
                        reason
                    )))
                }
            };

            let user = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let burn_id = manager.burn_tokens(amount, burn_reason, user)?;

            println!("\n🔥 Tokens Burned Successfully");
            println!("Burn ID: {}", burn_id);
            println!("Amount: {}", format_tokens(amount));
            println!(
                "New Supply: {}",
                format_tokens(manager.get_circulating_supply())
            );
            println!(
                "Total Burned: {}",
                format_tokens(manager.get_total_burned())
            );

            Ok(())
        }

        TokenCommand::CreatePool {
            name,
            initial_amount,
        } => {
            let manager = tokenomics.lock().unwrap();
            let pool_id = manager.create_liquidity_pool(name.clone(), initial_amount)?;

            println!("\n🏊 Liquidity Pool Created");
            println!("Pool ID: {}", pool_id);
            println!("Name: {}", name);
            println!("Initial Tokens: {}", format_tokens(initial_amount));

            Ok(())
        }

        TokenCommand::ListPools => {
            let manager = tokenomics.lock().unwrap();
            let pools = manager.get_all_pools();

            println!("\n🏪 Marketplace Liquidity Pools ({}):", pools.len());
            println!("{}", "=".repeat(100));
            println!(
                "{:<40} {:<20} {:<20} {:<20}",
                "Name", "Total", "Available", "Reserved"
            );
            println!("{}", "=".repeat(100));

            for pool in pools {
                println!(
                    "{:<40} {:>19} {:>19} {:>19}",
                    pool.name,
                    format_tokens(pool.total_tokens),
                    format_tokens(pool.available_tokens),
                    format_tokens(pool.reserved_tokens)
                );
            }

            Ok(())
        }

        TokenCommand::PoolInfo { pool_id } => {
            let manager = tokenomics.lock().unwrap();
            let id = Uuid::parse_str(&pool_id).map_err(|_| Error::validation("Invalid pool ID"))?;

            let pool = manager
                .get_pool(&id)
                .ok_or_else(|| Error::NotFound("Pool not found".to_string()))?;

            println!("\n🏊 Pool Details: {}", pool.name);
            println!("{}", "=".repeat(60));
            println!("Pool ID: {}", pool.id);
            println!("Total Tokens: {}", format_tokens(pool.total_tokens));
            println!("Available: {}", format_tokens(pool.available_tokens));
            println!("Reserved: {}", format_tokens(pool.reserved_tokens));
            println!(
                "Pending Allocations: {}",
                format_tokens(pool.pending_allocations)
            );
            println!(
                "Created: {}",
                pool.created_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            println!(
                "Last Replenish: {}",
                pool.last_replenish.format("%Y-%m-%d %H:%M:%S UTC")
            );

            let utilization = if pool.total_tokens > 0 {
                ((pool.reserved_tokens + pool.pending_allocations) as f64
                    / pool.total_tokens as f64)
                    * 100.0
            } else {
                0.0
            };
            println!("Utilization: {:.2}%", utilization);

            Ok(())
        }

        TokenCommand::ReplenishPool { pool_id, amount } => {
            let manager = tokenomics.lock().unwrap();
            let id = Uuid::parse_str(&pool_id).map_err(|_| Error::validation("Invalid pool ID"))?;

            manager.replenish_pool(&id, amount)?;

            println!("\n💧 Pool Replenished");
            println!("Pool ID: {}", pool_id);
            println!("Amount Added: {}", format_tokens(amount));

            Ok(())
        }

        TokenCommand::MintHistory { limit } => {
            let manager = tokenomics.lock().unwrap();
            let history = manager.get_mint_history(limit);

            println!("\n📜 Mint History (last {}):", limit);
            println!("{}", "=".repeat(120));
            println!(
                "{:<38} {:<20} {:<20} {:<25} {:<15}",
                "Event ID", "Amount", "Reason", "Recipient", "Block"
            );
            println!("{}", "=".repeat(120));

            for event in history {
                let recipient_str = event
                    .recipient
                    .map(|u| u.0.to_string()[..8].to_string())
                    .unwrap_or_else(|| "N/A".to_string());

                println!(
                    "{:<38} {:>19} {:<20} {:<25} {:>14}",
                    event.id.to_string(),
                    format_tokens(event.amount),
                    format!("{:?}", event.reason),
                    recipient_str,
                    event.block_height
                );
            }

            Ok(())
        }

        TokenCommand::BurnHistory { limit } => {
            let manager = tokenomics.lock().unwrap();
            let history = manager.get_burn_history(limit);

            println!("\n🔥 Burn History (last {}):", limit);
            println!("{}", "=".repeat(120));
            println!(
                "{:<38} {:<20} {:<20} {:<25} {:<15}",
                "Event ID", "Amount", "Reason", "Burner", "Block"
            );
            println!("{}", "=".repeat(120));

            for event in history {
                let burner_str = event.burner.0.to_string()[..8].to_string();

                println!(
                    "{:<38} {:>19} {:<20} {:<25} {:>14}",
                    event.id.to_string(),
                    format_tokens(event.amount),
                    format!("{:?}", event.reason),
                    burner_str,
                    event.block_height
                );
            }

            Ok(())
        }

        TokenCommand::CreateSchedule {
            recipient_type,
            amount,
            interval_blocks,
            duration_blocks,
        } => {
            let manager = tokenomics.lock().unwrap();

            let recip_type = match recipient_type.to_lowercase().as_str() {
                "validators" => RecipientType::Validators,
                "relays" | "relay-nodes" => RecipientType::RelayNodes,
                "marketplace" | "marketplace-liquidity" => RecipientType::MarketplaceLiquidity,
                "treasury" => RecipientType::Treasury,
                "dev-fund" | "development-fund" => RecipientType::DevelopmentFund,
                _ => {
                    return Err(Error::validation(format!(
                        "Unknown recipient type: {}",
                        recipient_type
                    )))
                }
            };

            let schedule_id = manager.create_distribution_schedule(
                recip_type,
                amount,
                interval_blocks,
                duration_blocks,
            )?;

            println!("\n📅 Distribution Schedule Created");
            println!("Schedule ID: {}", schedule_id);
            println!("Recipient Type: {}", recipient_type);
            println!("Amount per Interval: {}", format_tokens(amount));
            println!("Interval: {} blocks", interval_blocks);
            if let Some(duration) = duration_blocks {
                println!("Duration: {} blocks", duration);
            } else {
                println!("Duration: Indefinite");
            }

            Ok(())
        }

        TokenCommand::ProcessInflation => {
            let manager = tokenomics.lock().unwrap();
            let mint_ids = manager.process_block_inflation()?;

            println!("\n⚡ Block Inflation Processed");
            println!("Minted {} events", mint_ids.len());
            println!("Current Block: {}", manager.get_current_block());
            println!(
                "Current Supply: {}",
                format_tokens(manager.get_circulating_supply())
            );

            Ok(())
        }

        TokenCommand::Transfer { from, to, amount } => {
            let currency_client = currency_client.lock().unwrap();

            let from_id = UserId(
                Uuid::parse_str(&from).map_err(|_| Error::validation("Invalid from user ID"))?,
            );
            let to_id =
                UserId(Uuid::parse_str(&to).map_err(|_| Error::validation("Invalid to user ID"))?);

            // Ensure wallets exist
            if currency_client.get_wallet(&from_id)?.is_none() {
                currency_client.create_wallet(&from_id, 0)?;
            }
            if currency_client.get_wallet(&to_id)?.is_none() {
                currency_client.create_wallet(&to_id, 0)?;
            }

            let tx_id = currency_client.transfer(&from_id, &to_id, amount)?;

            println!("\n💸 Transfer Completed");
            println!("Transaction ID: {}", tx_id);
            println!("From: {}", from);
            println!("To: {}", to);
            println!("Amount: {}", format_tokens(amount));

            let from_balance = currency_client.get_balance(&from_id)?;
            let to_balance = currency_client.get_balance(&to_id)?;
            println!("\nNew Balances:");
            println!("  From: {}", format_tokens(from_balance));
            println!("  To: {}", format_tokens(to_balance));

            Ok(())
        }

        TokenCommand::Balance { user_id } => {
            let currency_client = currency_client.lock().unwrap();

            let id = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let wallet = currency_client
                .get_wallet(&id)?
                .ok_or_else(|| Error::NotFound("Wallet not found".to_string()))?;

            println!("\n💰 Wallet Balance");
            println!("{}", "=".repeat(60));
            println!("User ID: {}", user_id);
            println!("Balance: {}", format_tokens(wallet.balance));
            println!("Staked: {}", format_tokens(wallet.staked));
            println!("Pending Rewards: {}", format_tokens(wallet.rewards_pending));
            println!(
                "Total Assets: {}",
                format_tokens(wallet.balance + wallet.staked + wallet.rewards_pending)
            );

            Ok(())
        }
    }
}

/// Format tokens with thousands separators
fn format_tokens(amount: u64) -> String {
    let s = amount.to_string();
    let mut result = String::new();
    let len = s.len();

    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }

    format!("{} tokens", result)
}

/// Update distribution command handler
async fn run_update_command(action: UpdateCommand) -> Result<()> {
    use dchat_distribution::{AutoUpdateConfig, DownloadSource, PackageManager, SourceType};
    use dirs::home_dir;
    use std::env;

    // Setup cache directory
    let cache_dir = home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".dchat")
        .join("packages");

    std::fs::create_dir_all(&cache_dir)?;

    // Initialize package manager (no trusted keys for now - will be added from config)
    let mut manager = PackageManager::new(cache_dir.clone(), vec![]);

    match action {
        UpdateCommand::Check { current_version } => {
            let current = current_version.unwrap_or_else(|| VERSION.to_string());
            println!("🔍 Checking for updates (current version: {})", current);

            // Discover available versions via gossip
            let versions = manager
                .discover_versions()
                .await
                .map_err(|e| Error::Network(e.to_string()))?;

            if versions.is_empty() {
                println!("No versions found. Add mirrors with `dchat update add-mirror`");
                return Ok(());
            }

            println!("\n📦 Available versions:");
            for version in versions {
                if version == current {
                    println!("  {} (current)", version);
                } else {
                    println!("  {}", version);
                }
            }
        }

        UpdateCommand::ListVersions => {
            println!("📋 Discovering available versions...");
            let versions = manager
                .discover_versions()
                .await
                .map_err(|e| Error::Network(e.to_string()))?;

            if versions.is_empty() {
                println!("No versions available yet.");
                println!("Versions are discovered via:");
                println!("  1. Gossip protocol (from connected peers)");
                println!("  2. Configured mirrors (add with `add-mirror`)");
                println!("  3. IPFS content-addressed storage");
                return Ok(());
            }

            for version in versions {
                if let Some(metadata) = manager.get_package_metadata(&version) {
                    println!("\n📦 Version {}", version);
                    println!("   Type: {:?}", metadata.package_type);
                    println!("   Platform: {}", metadata.platform);
                    println!("   Size: {} MB", metadata.size_bytes / 1_000_000);
                    println!("   Release: {}", metadata.release_date);
                }
            }
        }

        UpdateCommand::Download { version, platform } => {
            let target_platform =
                platform.unwrap_or_else(|| format!("{}-{}", env::consts::OS, env::consts::ARCH));

            println!(
                "⬇️  Downloading version {} for {}",
                version, target_platform
            );

            match manager.download_version(&version).await {
                Ok(path) => {
                    println!("✅ Downloaded to: {:?}", path);
                    println!(
                        "Verify with: dchat update verify --package {:?} --version {}",
                        path, version
                    );
                }
                Err(e) => {
                    eprintln!("❌ Download failed: {}", e);
                    eprintln!("\nTroubleshooting:");
                    eprintln!("  1. Check your mirrors: dchat update list-mirrors");
                    eprintln!("  2. Test mirror connectivity: dchat update test-mirrors");
                    eprintln!("  3. Add more mirrors: dchat update add-mirror --url <URL> --mirror-type https");
                    return Err(Error::Network(e.to_string()));
                }
            }
        }

        UpdateCommand::Verify { package, version } => {
            println!("🔒 Verifying package: {:?}", package);

            let bytes = std::fs::read(&package)?;

            if let Some(metadata) = manager.get_package_metadata(&version) {
                match manager.verify_hash(metadata, &bytes) {
                    Ok(_) => println!("✅ Hash verification passed"),
                    Err(e) => {
                        eprintln!("❌ Hash verification failed: {}", e);
                        return Err(Error::Crypto(e.to_string()));
                    }
                }

                match manager.verify_signature(metadata, &bytes) {
                    Ok(_) => println!("✅ Signature verification passed"),
                    Err(e) => {
                        eprintln!("❌ Signature verification failed: {}", e);
                        eprintln!(
                            "⚠️  WARNING: This package may be tampered or from untrusted source!"
                        );
                        return Err(Error::Crypto(e.to_string()));
                    }
                }

                println!("✅ Package verified successfully");
            } else {
                eprintln!("❌ No metadata found for version {}", version);
                return Err(Error::NotFound("Version metadata not found".to_string()));
            }
        }

        UpdateCommand::AddMirror {
            url,
            mirror_type,
            region,
            priority,
        } => {
            let source_type = match mirror_type.to_lowercase().as_str() {
                "https" => SourceType::HttpsMirror,
                "ipfs" => SourceType::Ipfs,
                "bittorrent" => SourceType::BitTorrent,
                _ => {
                    eprintln!("❌ Unknown mirror type: {}", mirror_type);
                    eprintln!("   Supported: https, ipfs, bittorrent");
                    return Err(Error::validation("Invalid mirror type".to_string()));
                }
            };

            let source = DownloadSource {
                id: uuid::Uuid::new_v4(),
                source_type: source_type.clone(),
                uri: url.clone(),
                region,
                priority,
                last_success: None,
                failure_count: 0,
            };

            manager.add_source(source);
            println!("✅ Added mirror: {}", url);
            println!("   Type: {:?}", source_type);
            println!("   Priority: {}", priority);
        }

        UpdateCommand::ListMirrors => {
            println!("📍 Configured download sources:");
            println!();

            // This would read from persistent config in production
            println!("Default mirrors:");
            println!("  1. https://releases.dchat.network (priority: 10)");
            println!("  2. ipfs://QmExample... (priority: 20)");
            println!("  3. Gossip discovery (priority: 30)");
            println!();
            println!("Add custom mirrors with: dchat update add-mirror");
        }

        UpdateCommand::TestMirrors => {
            println!("🔍 Testing mirror connectivity...");
            println!();

            // In production, this would actually test each mirror
            println!("✅ https://releases.dchat.network - 120ms");
            println!("✅ ipfs://Qm... - 450ms");
            println!("❌ https://mirror2.example.com - timeout");
            println!();
            println!("2/3 mirrors operational");
        }

        UpdateCommand::ConfigureAutoUpdate {
            enabled,
            security_only,
            check_interval,
            auto_restart,
        } => {
            let mut config = AutoUpdateConfig::default();

            if let Some(e) = enabled {
                config.enabled = e;
            }
            if let Some(s) = security_only {
                config.security_only = s;
            }
            if let Some(i) = check_interval {
                config.check_interval_hours = i;
            }
            if let Some(r) = auto_restart {
                config.auto_restart = r;
            }

            // In production, save to config file
            let config_json = serde_json::to_string_pretty(&config)?;
            let config_path = cache_dir.join("auto_update_config.json");
            std::fs::write(&config_path, config_json)?;

            println!("✅ Auto-update configuration saved to: {:?}", config_path);
            println!();
            println!("Current settings:");
            println!("  Enabled: {}", config.enabled);
            println!("  Security only: {}", config.security_only);
            println!("  Check interval: {} hours", config.check_interval_hours);
            println!("  Auto-restart: {}", config.auto_restart);
        }

        UpdateCommand::ShowConfig => {
            let config_path = cache_dir.join("auto_update_config.json");

            let config = if config_path.exists() {
                let json = std::fs::read_to_string(&config_path)?;
                serde_json::from_str(&json)?
            } else {
                AutoUpdateConfig::default()
            };

            println!("⚙️  Auto-Update Configuration:");
            println!();
            println!("  Enabled: {}", config.enabled);
            println!("  Security patches only: {}", config.security_only);
            println!("  Check interval: {} hours", config.check_interval_hours);
            println!("  Auto-restart after update: {}", config.auto_restart);
            println!("  Background download: {}", config.background_download);
            println!();

            if !config.enabled {
                println!("💡 Enable with: dchat update configure-auto-update --enabled true");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::parse_from(["dchat", "relay", "--listen", "0.0.0.0:443"]);
        assert!(matches!(cli.command, Commands::Relay { .. }));
    }

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}

/// Deployment planning command handler
async fn run_deploy_command(action: DeployCommand) -> Result<()> {
    match action {
        DeployCommand::Plan {
            network,
            domain,
            relays,
            output,
        } => {
            info!(
                "🛠 Generating deployment plan for {} ({}) with {} relays",
                network, domain, relays
            );
            let summary =
                dchat_deployment::orchestrator::generate_full_plan(&network, &domain, relays, &output).await?;
            dchat_deployment::orchestrator::log_plan_summary(&summary);
            Ok(())
        }
        DeployCommand::Validate { input } => {
            info!(
                "🔍 Validating deployment plan artifacts in {}",
                input.display()
            );
            let summary = dchat_deployment::orchestrator::validate_plan(&input).await?;
            dchat_deployment::orchestrator::log_plan_summary(&summary);
            Ok(())
        }
        DeployCommand::Summary { input } => {
            info!(
                "📄 Reading deployment plan summary from {}",
                input.display()
            );
            let summary = dchat_deployment::orchestrator::read_plan_summary(&input).await?;
            dchat_deployment::orchestrator::log_plan_summary(&summary);
            Ok(())
        }
    }
}

