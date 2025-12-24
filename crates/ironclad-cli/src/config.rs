//! Project configuration (Ironclad.toml)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Ironclad project configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IroncladConfig {
    /// Project metadata
    pub project: ProjectConfig,

    /// Build configuration
    #[serde(default)]
    pub build: BuildConfig,

    /// Network configurations
    #[serde(default)]
    pub networks: HashMap<String, NetworkConfig>,

    /// Test configuration
    #[serde(default)]
    pub test: TestConfig,

    /// IDL configuration
    #[serde(default)]
    pub idl: IdlConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project name
    pub name: String,

    /// Project version
    #[serde(default = "default_version")]
    pub version: String,

    /// Program ID (set after first deployment)
    pub program_id: Option<String>,

    /// Authors
    #[serde(default)]
    pub authors: Vec<String>,

    /// Description
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildConfig {
    /// Target triple
    #[serde(default = "default_target")]
    pub target: String,

    /// Custom target directory
    pub target_dir: Option<PathBuf>,

    /// Extra rustc flags
    #[serde(default)]
    pub rustflags: Vec<String>,

    /// Enable reproducible builds
    #[serde(default)]
    pub reproducible: bool,

    /// Generate IDL on build
    #[serde(default)]
    pub generate_idl: bool,

    /// Verify schema hash on build
    #[serde(default)]
    pub verify_schema: bool,

    /// Expected schema hash (for CI)
    pub expected_schema_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// RPC URL
    pub url: String,

    /// WebSocket URL (optional)
    pub ws_url: Option<String>,

    /// Default keypair path
    pub keypair: Option<String>,

    /// Confirm transactions
    #[serde(default = "default_true")]
    pub confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestConfig {
    /// Test timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Run tests in parallel
    #[serde(default = "default_true")]
    pub parallel: bool,

    /// Test filter pattern
    pub filter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdlConfig {
    /// Output directory for IDL files
    #[serde(default = "default_idl_dir")]
    pub output_dir: PathBuf,

    /// Generate TypeScript types
    #[serde(default)]
    pub generate_ts: bool,

    /// TypeScript output directory
    #[serde(default = "default_ts_dir")]
    pub ts_output_dir: PathBuf,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_target() -> String {
    "wasm32-unknown-unknown".to_string()
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    300
}

fn default_idl_dir() -> PathBuf {
    PathBuf::from("idl")
}

fn default_ts_dir() -> PathBuf {
    PathBuf::from("ts-client")
}

impl Default for IroncladConfig {
    fn default() -> Self {
        let mut networks = HashMap::new();
        networks.insert(
            "localnet".to_string(),
            NetworkConfig {
                url: "http://localhost:8545".to_string(),
                ws_url: Some("ws://localhost:8546".to_string()),
                keypair: Some("~/.dchat/keypair.json".to_string()),
                confirm: true,
            },
        );
        networks.insert(
            "devnet".to_string(),
            NetworkConfig {
                url: "https://devnet.dchat.network".to_string(),
                ws_url: Some("wss://devnet.dchat.network/ws".to_string()),
                keypair: None,
                confirm: true,
            },
        );
        networks.insert(
            "testnet".to_string(),
            NetworkConfig {
                url: "https://testnet.dchat.network".to_string(),
                ws_url: Some("wss://testnet.dchat.network/ws".to_string()),
                keypair: None,
                confirm: true,
            },
        );
        networks.insert(
            "mainnet".to_string(),
            NetworkConfig {
                url: "https://mainnet.dchat.network".to_string(),
                ws_url: Some("wss://mainnet.dchat.network/ws".to_string()),
                keypair: None,
                confirm: true,
            },
        );

        Self {
            project: ProjectConfig {
                name: "my-program".to_string(),
                version: default_version(),
                program_id: None,
                authors: vec![],
                description: None,
            },
            build: BuildConfig::default(),
            networks,
            test: TestConfig::default(),
            idl: IdlConfig::default(),
        }
    }
}

impl IroncladConfig {
    /// Load configuration from Ironclad.toml
    pub fn load(path: &std::path::Path) -> Result<Self, crate::error::IroncladError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::IroncladError::ConfigError(format!("Failed to read config: {}", e))
        })?;
        toml::from_str(&content).map_err(|e| {
            crate::error::IroncladError::ConfigError(format!("Failed to parse config: {}", e))
        })
    }

    /// Save configuration to Ironclad.toml
    pub fn save(&self, path: &std::path::Path) -> Result<(), crate::error::IroncladError> {
        let content = toml::to_string_pretty(self).map_err(|e| {
            crate::error::IroncladError::ConfigError(format!("Failed to serialize config: {}", e))
        })?;
        std::fs::write(path, content).map_err(|e| {
            crate::error::IroncladError::ConfigError(format!("Failed to write config: {}", e))
        })
    }

    /// Get network configuration
    pub fn get_network(&self, name: &str) -> Option<&NetworkConfig> {
        self.networks.get(name)
    }

    /// Create default config with a specific project name
    pub fn default_with_name(name: &str) -> Self {
        let mut config = Self::default();
        config.project.name = name.to_string();
        config
    }
}
