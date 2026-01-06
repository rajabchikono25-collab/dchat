// Configuration module
pub mod constants;

pub use constants::*;

/// Configuration management for dchat
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main configuration structure for dchat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub network: NetworkConfig,
    pub storage: StorageConfig,
    pub crypto: CryptoConfig,
    pub governance: GovernanceConfig,
    pub relay: RelayConfig,

    #[serde(default)]
    pub rpc: RpcConfig,

    #[serde(default)]
    pub chain: ChainConfig,

    /// Feature flags for enabling/disabling subsystems
    #[serde(default)]
    pub features: FeaturesConfig,

    /// Payment channels configuration
    #[serde(default)]
    pub payment_channels: PaymentChannelsConfig,

    /// Oracle network configuration
    #[serde(default)]
    pub oracle: OracleConfig,

    /// Onion routing configuration
    #[serde(default)]
    pub onion_routing: OnionRoutingConfig,

    /// Storage provider configuration
    #[serde(default)]
    pub storage_provider: StorageProviderConfig,
}

/// Chain timing configuration for epoch calculations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Genesis timestamp (Unix seconds) for block height calculations.
    /// Default: 1735689600 (Jan 1, 2025 00:00:00 UTC)
    #[serde(default = "default_genesis_timestamp")]
    pub genesis_timestamp: u64,

    /// Block time in seconds.
    /// Default: 6 seconds
    #[serde(default = "default_block_time_secs")]
    pub block_time_secs: u64,
}

fn default_genesis_timestamp() -> u64 {
    1735689600 // Jan 1, 2025 00:00:00 UTC
}

fn default_block_time_secs() -> u64 {
    6
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            genesis_timestamp: default_genesis_timestamp(),
            block_time_secs: default_block_time_secs(),
        }
    }
}

/// RPC configuration for external chain dependencies
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcConfig {
    /// Chat chain JSON-RPC endpoint.
    ///
    /// Environment override: `DCHAT_CHAT_CHAIN_RPC_URL`
    pub chat_chain_rpc_url: Option<String>,

    /// Currency chain JSON-RPC endpoint.
    ///
    /// Environment override: `DCHAT_CURRENCY_CHAIN_RPC_URL`
    pub currency_chain_rpc_url: Option<String>,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addresses: Vec<String>,
    pub bootstrap_peers: Vec<String>,
    pub max_connections: u32,
    pub connection_timeout_ms: u64,
    pub enable_mdns: bool,
    pub enable_upnp: bool,
    /// External/public address to announce (optional, for NAT traversal)
    /// Format: "/ip4/<public_ip>/tcp/<port>"
    #[serde(default)]
    pub external_address: Option<String>,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: PathBuf,
    pub max_message_cache_size: usize,
    pub message_retention_days: u32,
    pub enable_backup: bool,
    pub backup_interval_hours: u32,

    // Database connection pool settings
    pub db_pool_size: u32,
    pub db_connection_timeout_secs: u64,
    pub db_idle_timeout_secs: u64,
    pub db_max_lifetime_secs: u64,
    pub db_enable_wal: bool,
}

/// Cryptography configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoConfig {
    pub key_rotation_interval_hours: u32,
    pub max_messages_per_key: u64,
    pub enable_post_quantum: bool,
    pub noise_protocol_pattern: String,
}

/// Governance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    /// Duration of voting period in hours
    pub voting_period_hours: u32,
    /// Minimum stake required to submit a proposal
    pub minimum_stake_for_proposal: u64,
    /// Quorum threshold in basis points (0-10000, where 10000 = 100%)
    /// Example: 1000 = 10% quorum required
    ///
    /// For backward compatibility, also accepts `quorum_threshold` as f64 (0.0-1.0)
    /// which will be converted to basis points automatically.
    #[serde(
        default = "default_quorum_bps",
        deserialize_with = "deserialize_quorum_bps",
        alias = "quorum_threshold"
    )]
    pub quorum_threshold_bps: u16,
    /// Enable anonymous/encrypted voting
    pub enable_anonymous_voting: bool,
    /// Approval threshold in basis points (0-10000, where 5001 = simple majority)
    #[serde(default = "default_approval_bps")]
    pub approval_threshold_bps: u16,
}

fn default_quorum_bps() -> u16 {
    1000 // 10%
}

fn default_approval_bps() -> u16 {
    5001 // Simple majority (>50%)
}

/// Deserialize quorum threshold - accepts either:
/// - u16 (basis points: 0-10000)
/// - f64 (decimal: 0.0-1.0, converted to bps)
fn deserialize_quorum_bps<'de, D>(deserializer: D) -> std::result::Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct QuorumVisitor;

    impl<'de> Visitor<'de> for QuorumVisitor {
        type Value = u16;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(
                "a number representing quorum threshold (0-10000 bps or 0.0-1.0 decimal)",
            )
        }

        fn visit_i64<E>(self, value: i64) -> std::result::Result<u16, E>
        where
            E: de::Error,
        {
            if value < 0 || value > 10000 {
                return Err(E::custom(format!(
                    "quorum bps {} out of range 0-10000",
                    value
                )));
            }
            Ok(value as u16)
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<u16, E>
        where
            E: de::Error,
        {
            if value > 10000 {
                return Err(E::custom(format!(
                    "quorum bps {} out of range 0-10000",
                    value
                )));
            }
            Ok(value as u16)
        }

        fn visit_f64<E>(self, value: f64) -> std::result::Result<u16, E>
        where
            E: de::Error,
        {
            // f64 values <= 1.0 are treated as decimals (0.5 = 50% = 5000 bps)
            // f64 values > 1.0 and <= 100.0 are treated as percentages (50.0 = 5000 bps)
            // f64 values > 100.0 and <= 10000.0 are treated as bps directly
            let bps = if value <= 1.0 {
                (value * 10000.0).round() as u16
            } else if value <= 100.0 {
                (value * 100.0).round() as u16
            } else if value <= 10000.0 {
                value.round() as u16
            } else {
                return Err(E::custom(format!("quorum value {} out of range", value)));
            };
            Ok(bps)
        }
    }

    deserializer.deserialize_any(QuorumVisitor)
}

/// Relay configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub enable_relay: bool,
    pub max_relay_connections: u32,
    pub relay_reward_threshold: u64,
    pub uptime_reporting_interval_minutes: u32,
    pub stake_amount: u64,
    /// Seed relay nodes for initial network bootstrap
    /// Format: [{ relay_id, multiaddr, stake, continent }]
    #[serde(default)]
    pub seed_relays: Vec<SeedRelay>,
}

/// Configuration for a seed relay node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedRelay {
    /// Unique relay identifier
    pub relay_id: String,
    /// libp2p multiaddr for the relay
    pub multiaddr: String,
    /// Stake amount (in smallest token units)
    pub stake: u64,
    /// Geographic continent for diversity
    #[serde(default = "default_continent")]
    pub continent: String,
    /// Optional operator user ID
    #[serde(default)]
    pub operator_id: Option<String>,
}

fn default_continent() -> String {
    "Europe".to_string()
}

/// Feature flags configuration for enabling/disabling subsystems
/// All features default to disabled for safe rollout
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeaturesConfig {
    /// Enable onion routing for metadata protection (Phase 2)
    pub enable_onion_routing: bool,
    /// Enable off-chain payment channels for micropayments (Phase 2)
    pub enable_payment_channels: bool,
    /// Enable end-to-end encryption with Double Ratchet (Phase 1)
    /// Default: true - always enable E2E for security
    pub enable_e2e_encryption: bool,
    /// Enable watchtower service for fraud detection (Phase 3, validator only)
    pub enable_watchtower: bool,
    /// Enable oracle network for external price feeds (Phase 3, validator only)
    pub enable_oracle: bool,
    /// Enable bot platform with BotFather (Phase 4, relay only)
    pub enable_bots: bool,
    /// Enable mini-app platform (Phase 4)
    pub enable_miniapps: bool,
    /// Enable marketplace for digital goods (Phase 4)
    pub enable_marketplace: bool,
    /// Enable VRF-based committee selection (Phase 5, validator only)
    pub enable_vrf_committees: bool,
    /// Enable two-stage finality for faster confirmation (Phase 5, validator only)
    pub enable_two_stage_finality: bool,
    /// Enable decentralized storage providers (Phase 1)
    pub enable_storage_providers: bool,
    /// Enable admission control for rate limiting (Phase 5)
    pub enable_admission_control: bool,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            enable_onion_routing: false,
            enable_payment_channels: false,
            enable_e2e_encryption: true, // Always enable E2E by default
            enable_watchtower: false,
            enable_oracle: false,
            enable_bots: false,
            enable_miniapps: false,
            enable_marketplace: false,
            enable_vrf_committees: false,
            enable_two_stage_finality: false,
            enable_storage_providers: false,
            enable_admission_control: false,
        }
    }
}

/// Payment channels configuration for off-chain micropayments
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PaymentChannelsConfig {
    /// Minimum channel capacity in motes (default: 10 DCHAT)
    pub min_channel_capacity: u64,
    /// Maximum channel capacity in motes (default: 100,000 DCHAT)
    pub max_channel_capacity: u64,
    /// Dispute window in seconds (default: 48 hours)
    pub dispute_window_secs: u64,
    /// Auto-close threshold as fraction of capacity (0.1 = 10% remaining)
    pub auto_close_threshold: f64,
    /// Maximum pending payments before forcing settlement
    pub max_pending_payments: u32,
}

impl Default for PaymentChannelsConfig {
    fn default() -> Self {
        Self {
            min_channel_capacity: 10_000_000_000,      // 10 DCHAT
            max_channel_capacity: 100_000_000_000_000, // 100,000 DCHAT
            dispute_window_secs: 48 * 60 * 60,         // 48 hours
            auto_close_threshold: 0.1,                 // 10%
            max_pending_payments: 1000,
        }
    }
}

/// Oracle network configuration for external price feeds
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OracleConfig {
    /// Minimum stake required to participate as oracle (in motes)
    pub min_stake: u64,
    /// Price update interval in seconds
    pub update_interval_secs: u64,
    /// Maximum deviation from median before considered outlier (0.5 = 50%)
    pub max_outlier_deviation: f64,
    /// Slash rate for bad predictions (0.1 = 10%)
    pub outlier_slash_rate: f64,
    /// Minimum number of oracle submissions for consensus
    pub min_submissions: u32,
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            min_stake: 1_000_000_000_000, // 1000 DCHAT
            update_interval_secs: 60,
            max_outlier_deviation: 0.5,
            outlier_slash_rate: 0.10,
            min_submissions: 3,
        }
    }
}

/// Onion routing configuration for metadata protection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OnionRoutingConfig {
    /// Routing mode: "direct" (default) or "onion"
    pub routing_mode: String,
    /// Minimum circuit hops (default: 3)
    pub min_circuit_hops: usize,
    /// Maximum circuit hops (default: 5)
    pub max_circuit_hops: usize,
    /// Circuit rotation interval in seconds (default: 10 minutes)
    pub circuit_rotation_secs: u64,
    /// Circuit build timeout in seconds
    pub circuit_build_timeout_secs: u64,
    /// Enable cover traffic to mask real traffic patterns
    pub enable_cover_traffic: bool,
}

impl Default for OnionRoutingConfig {
    fn default() -> Self {
        Self {
            routing_mode: "direct".to_string(),
            min_circuit_hops: 3,
            max_circuit_hops: 5,
            circuit_rotation_secs: 600, // 10 minutes
            circuit_build_timeout_secs: 30,
            enable_cover_traffic: true,
        }
    }
}

/// Storage provider configuration for decentralized storage
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageProviderConfig {
    /// Storage provider type: "local", "s3", "ipfs"
    pub provider: String,
    /// Inline threshold in bytes - messages smaller than this stored in DB
    pub inline_threshold_bytes: usize,
    /// Maximum blob size in bytes
    pub max_blob_size_bytes: usize,
    /// Enable at-rest encryption for stored data
    pub enable_at_rest_encryption: bool,
    /// Default user storage quota in bytes
    pub default_user_quota_bytes: u64,
    /// S3 bucket name (if using S3)
    pub s3_bucket: Option<String>,
    /// S3 region (if using S3)
    pub s3_region: Option<String>,
    /// IPFS gateway URL (if using IPFS)
    pub ipfs_gateway: Option<String>,
}

impl Default for StorageProviderConfig {
    fn default() -> Self {
        Self {
            provider: "local".to_string(),
            inline_threshold_bytes: 65536,    // 64KB
            max_blob_size_bytes: 104_857_600, // 100MB
            enable_at_rest_encryption: true,
            default_user_quota_bytes: 10_737_418_240, // 10GB
            s3_bucket: None,
            s3_region: None,
            ipfs_gateway: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: NetworkConfig {
                listen_addresses: vec![
                    "/ip4/0.0.0.0/tcp/0".to_string(),
                    "/ip4/0.0.0.0/udp/0/quic-v1".to_string(),
                ],
                bootstrap_peers: vec![],
                max_connections: 100,
                connection_timeout_ms: 10000,
                enable_mdns: true,
                enable_upnp: true,
                external_address: None,
            },
            storage: StorageConfig {
                data_dir: PathBuf::from("./dchat_data"),
                max_message_cache_size: 10000,
                message_retention_days: 30,
                enable_backup: true,
                backup_interval_hours: 24,
                db_pool_size: 10,
                db_connection_timeout_secs: 30,
                db_idle_timeout_secs: 600,
                db_max_lifetime_secs: 1800,
                db_enable_wal: true,
            },
            crypto: CryptoConfig {
                key_rotation_interval_hours: 168, // 1 week
                max_messages_per_key: 10000,
                enable_post_quantum: false,
                noise_protocol_pattern: "Noise_XX_25519_ChaChaPoly_BLAKE2s".to_string(),
            },
            governance: GovernanceConfig {
                voting_period_hours: 168, // 1 week
                minimum_stake_for_proposal: 1000,
                quorum_threshold_bps: 1000, // 10%
                enable_anonymous_voting: true,
                approval_threshold_bps: 5001, // Simple majority
            },
            relay: RelayConfig {
                enable_relay: false,
                max_relay_connections: 50,
                relay_reward_threshold: 100,
                uptime_reporting_interval_minutes: 15,
                stake_amount: 1000,
                seed_relays: vec![],
            },

            rpc: RpcConfig::default(),
            chain: ChainConfig::default(),
            features: FeaturesConfig::default(),
            payment_channels: PaymentChannelsConfig::default(),
            oracle: OracleConfig::default(),
            onion_routing: OnionRoutingConfig::default(),
            storage_provider: StorageProviderConfig::default(),
        }
    }
}

impl RpcConfig {
    pub fn resolved_chat_chain_rpc_url(&self) -> Option<String> {
        if let Some(url) = &self.chat_chain_rpc_url {
            if !url.trim().is_empty() {
                return Some(url.clone());
            }
        }

        for key in [
            "DCHAT_CHAT_CHAIN_RPC_URL",
            // Legacy/compat env vars
            "CHAT_CHAIN_RPC",
            "CHAT_CHAIN_RPC_URL",
        ] {
            if let Ok(v) = std::env::var(key) {
                let trimmed = v.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }

        None
    }

    pub fn resolved_currency_chain_rpc_url(&self) -> Option<String> {
        if let Some(url) = &self.currency_chain_rpc_url {
            if !url.trim().is_empty() {
                return Some(url.clone());
            }
        }

        for key in [
            "DCHAT_CURRENCY_CHAIN_RPC_URL",
            // Legacy/compat env vars
            "CURRENCY_CHAIN_RPC",
            "CURRENCY_CHAIN_RPC_URL",
        ] {
            if let Ok(v) = std::env::var(key) {
                let trimmed = v.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }

        None
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("Failed to read config file: {}", e)))?;

        let config: Config = toml::from_str(&contents)
            .map_err(|e| Error::Config(format!("Failed to parse config: {}", e)))?;

        config.validate()?;
        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn to_file(&self, path: &PathBuf) -> Result<()> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(path, contents)
            .map_err(|e| Error::Config(format!("Failed to write config file: {}", e)))?;

        Ok(())
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<()> {
        if self.network.max_connections == 0 {
            return Err(Error::Config(
                "max_connections must be greater than 0".to_string(),
            ));
        }

        if self.network.connection_timeout_ms == 0 {
            return Err(Error::Config(
                "connection_timeout_ms must be greater than 0".to_string(),
            ));
        }

        if self.crypto.key_rotation_interval_hours == 0 {
            return Err(Error::Config(
                "key_rotation_interval_hours must be greater than 0".to_string(),
            ));
        }

        if self.crypto.max_messages_per_key == 0 {
            return Err(Error::Config(
                "max_messages_per_key must be greater than 0".to_string(),
            ));
        }

        if self.governance.quorum_threshold_bps > 10000 {
            return Err(Error::Config(
                "quorum_threshold_bps must be between 0 and 10000".to_string(),
            ));
        }

        if self.governance.approval_threshold_bps > 10000 {
            return Err(Error::Config(
                "approval_threshold_bps must be between 0 and 10000".to_string(),
            ));
        }

        Ok(())
    }
}
