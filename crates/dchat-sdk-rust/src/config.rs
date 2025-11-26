use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Display name for the user
    pub name: String,
    /// Storage configuration
    pub storage: StorageConfig,
    /// Network configuration
    pub network: NetworkConfig,
    /// Enable encryption (default: true)
    pub encryption_enabled: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            name: "Anonymous".to_string(),
            storage: StorageConfig::default(),
            network: NetworkConfig::default(),
            encryption_enabled: true,
        }
    }
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Path to storage directory
    pub data_dir: PathBuf,
    /// Maximum storage size in MB (0 = unlimited)
    pub max_size_mb: u64,
    /// Enable local caching
    pub cache_enabled: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./dchat_data"),
            max_size_mb: 1000, // 1GB default
            cache_enabled: true,
        }
    }
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Bootstrap peer addresses
    pub bootstrap_peers: Vec<String>,
    /// Listen port (0 = random)
    pub listen_port: u16,
    /// Maximum connections
    pub max_connections: usize,
    /// Connection timeout in seconds
    pub connection_timeout_secs: u64,
    /// Blockchain RPC URL for transaction submission
    pub blockchain_rpc_url: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bootstrap_peers: vec![],
            listen_port: 0, // Random port
            max_connections: 50,
            connection_timeout_secs: 30,
            blockchain_rpc_url: std::env::var("DCHAT_BLOCKCHAIN_RPC_URL")
                .unwrap_or_else(|_| "http://localhost:8545".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ClientConfig::default();
        assert_eq!(config.name, "Anonymous");
        assert!(config.encryption_enabled);
    }

    #[test]
    fn test_storage_config() {
        let config = StorageConfig::default();
        assert_eq!(config.max_size_mb, 1000);
        assert!(config.cache_enabled);
    }

    #[test]
    fn test_network_config() {
        let config = NetworkConfig::default();
        assert_eq!(config.max_connections, 50);
        assert_eq!(config.connection_timeout_secs, 30);
    }
}
