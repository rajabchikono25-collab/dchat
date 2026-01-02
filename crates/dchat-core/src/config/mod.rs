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
            },

            rpc: RpcConfig::default(),
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
