//! Configuration management for dchat

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
    pub voting_period_hours: u32,
    pub minimum_stake_for_proposal: u64,
    pub quorum_threshold: f64,
    pub enable_anonymous_voting: bool,
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
                quorum_threshold: 0.1, // 10%
                enable_anonymous_voting: true,
            },
            relay: RelayConfig {
                enable_relay: false,
                max_relay_connections: 50,
                relay_reward_threshold: 100,
                uptime_reporting_interval_minutes: 15,
                stake_amount: 1000,
            },
        }
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

        if !(0.0..=1.0).contains(&self.governance.quorum_threshold) {
            return Err(Error::Config(
                "quorum_threshold must be between 0.0 and 1.0".to_string(),
            ));
        }

        Ok(())
    }
}
