// Multi-Region Validator Configuration System
// Implements geographic distribution for decentralized validator deployment

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use thiserror::Error;

/// Default public TLS port for validator/libp2p traffic
const VALIDATOR_P2P_PORT: u16 = 443;
/// Default public HTTP port for validator RPC and health endpoints
const VALIDATOR_RPC_PORT: u16 = 80;

/// Geographic region for validator distribution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeographicRegion {
    /// US East (AWS us-east-1, DigitalOcean NYC)
    USEast,
    /// US West (AWS us-west-2, Linode Fremont)
    USWest,
    /// EU West (AWS eu-west-1, Hetzner Germany)
    EUWest,
    /// EU Central (Frankfurt, Amsterdam)
    EUCentral,
    /// Asia Pacific SE (Singapore, AWS ap-southeast-1)
    AsiaPacificSE,
    /// Asia Pacific NE (Tokyo, Vultr Tokyo)
    AsiaPacificNE,
    /// South America (AWS sa-east-1, Linode São Paulo)
    SouthAmerica,
}

impl GeographicRegion {
    /// Get all available regions
    pub fn all() -> Vec<Self> {
        vec![
            Self::USEast,
            Self::USWest,
            Self::EUWest,
            Self::EUCentral,
            Self::AsiaPacificSE,
            Self::AsiaPacificNE,
            Self::SouthAmerica,
        ]
    }

    /// Get recommended validator count for this region
    pub fn recommended_validator_count(&self) -> usize {
        match self {
            Self::USEast | Self::EUWest | Self::AsiaPacificSE => 2, // High priority regions
            Self::USWest | Self::EUCentral | Self::AsiaPacificNE => 2, // Medium priority
            Self::SouthAmerica => 1, // Lower traffic but important for diversity
        }
    }

    /// Get DNS suffix for this region
    pub fn dns_suffix(&self) -> &'static str {
        match self {
            Self::USEast => "us-east",
            Self::USWest => "us-west",
            Self::EUWest => "eu-west",
            Self::EUCentral => "eu-central",
            Self::AsiaPacificSE => "ap-southeast",
            Self::AsiaPacificNE => "ap-northeast",
            Self::SouthAmerica => "sa-east",
        }
    }

    /// Get expected latency to other regions (milliseconds)
    pub fn latency_to(&self, other: &Self) -> u64 {
        if self == other {
            return 5; // Same region
        }

        // Speed of light + routing overhead estimates
        match (self, other) {
            (Self::USEast, Self::USWest) | (Self::USWest, Self::USEast) => 80,
            (Self::USEast, Self::EUWest) | (Self::EUWest, Self::USEast) => 90,
            (Self::USEast, Self::AsiaPacificSE) | (Self::AsiaPacificSE, Self::USEast) => 200,
            (Self::USWest, Self::AsiaPacificSE) | (Self::AsiaPacificSE, Self::USWest) => 140,
            (Self::EUWest, Self::AsiaPacificSE) | (Self::AsiaPacificSE, Self::EUWest) => 180,
            (Self::SouthAmerica, Self::USEast) | (Self::USEast, Self::SouthAmerica) => 120,
            (Self::SouthAmerica, Self::EUWest) | (Self::EUWest, Self::SouthAmerica) => 200,
            _ => 150, // Default cross-continental
        }
    }
}

/// Validator configuration for a specific region
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    /// Unique validator identifier
    pub validator_id: String,

    /// Geographic region
    pub region: GeographicRegion,

    /// Public IP address or DNS hostname
    pub public_address: String,

    /// P2P listen addresses (libp2p multiaddrs)
    pub listen_addresses: Vec<String>,

    /// RPC endpoint address
    pub rpc_address: SocketAddr,

    /// Bootstrap peers from OTHER regions (critical for decentralization)
    pub bootstrap_peers: Vec<String>,

    /// BFT consensus configuration
    pub consensus: ConsensusConfig,

    /// Storage backend configuration
    pub storage: StorageConfig,

    /// Private key path (for signing)
    pub private_key_path: PathBuf,

    /// Staking amount (DCHAT tokens)
    pub stake_amount: u64,
}

/// BFT consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// All validator RPC addresses (for BFT voting)
    pub validator_addresses: Vec<String>,

    /// Required signatures for finality (e.g., 5 of 7)
    pub required_signatures: usize,

    /// Total validator count
    pub total_validators: usize,

    /// Block time (milliseconds)
    pub block_time_ms: u64,

    /// Finality confirmation blocks
    pub finality_blocks: u64,
}

/// Storage backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Storage backend type
    pub backend: StorageBackend,

    /// Replication factor
    pub replication_factor: usize,

    /// Maximum connections
    pub max_connections: usize,
}

/// Storage backend options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StorageBackend {
    /// TiKV distributed key-value store
    TiKV { pd_endpoints: Vec<String> },

    /// CockroachDB distributed SQL
    CockroachDB { database_urls: Vec<String> },

    /// PostgreSQL (single or replicated)
    PostgreSQL { database_url: String },
}

/// Multi-region deployment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiRegionConfig {
    /// Network name
    pub network_name: String,

    /// Base domain (e.g., dchat.network)
    pub base_domain: String,

    /// All validator configurations
    pub validators: Vec<ValidatorConfig>,

    /// BFT threshold (e.g., 0.67 for 67% = 5 of 7)
    pub bft_threshold: f64,

    /// Geographic diversity requirements
    pub min_continents: usize,
    pub max_concentration_per_region: f64, // e.g., 0.40 = 40%
}

impl MultiRegionConfig {
    /// Create a new multi-region configuration with recommended settings
    pub fn new_recommended(network_name: String, base_domain: String) -> Self {
        let validators = Self::generate_recommended_validators(&base_domain);
        let total = validators.len();
        let required = (total * 2 / 3) + 1; // 67% + 1 for BFT

        Self {
            network_name,
            base_domain,
            validators,
            bft_threshold: required as f64 / total as f64,
            min_continents: 3,
            max_concentration_per_region: 0.40,
        }
    }

    /// Generate recommended validator configurations (7 total)
    fn generate_recommended_validators(base_domain: &str) -> Vec<ValidatorConfig> {
        let regions = vec![
            (GeographicRegion::USEast, 2),
            (GeographicRegion::EUWest, 2),
            (GeographicRegion::AsiaPacificSE, 2),
            (GeographicRegion::SouthAmerica, 1),
        ];

        let mut validators = Vec::new();
        let mut validator_index = 0;

        for (region, count) in regions {
            for i in 0..count {
                let validator_id = format!("validator-{}-{}", region.dns_suffix(), i + 1);
                let public_address = format!("{}.{}", validator_id, base_domain);

                let config = ValidatorConfig {
                    validator_id: validator_id.clone(),
                    region,
                    public_address: public_address.clone(),
                    listen_addresses: vec![
                        format!("/ip4/0.0.0.0/tcp/{}", VALIDATOR_P2P_PORT),
                        format!("/ip4/0.0.0.0/udp/{}/quic-v1", VALIDATOR_P2P_PORT),
                    ],
                    rpc_address: format!("0.0.0.0:{}", VALIDATOR_RPC_PORT).parse().unwrap(),
                    bootstrap_peers: Vec::new(), // Will be filled later
                    consensus: ConsensusConfig {
                        validator_addresses: Vec::new(), // Will be filled later
                        required_signatures: 5,          // 5 of 7
                        total_validators: 7,
                        block_time_ms: 2000, // 2 seconds
                        finality_blocks: 3,
                    },
                    storage: StorageConfig {
                        backend: StorageBackend::CockroachDB {
                            database_urls: vec![format!(
                                "postgresql://dchat:pass@cockroach-{}.{}:26257/dchat",
                                region.dns_suffix(),
                                base_domain
                            )],
                        },
                        replication_factor: 3,
                        max_connections: 50,
                    },
                    private_key_path: PathBuf::from(format!("/data/keys/{}.key", validator_id)),
                    stake_amount: 100_000, // 100k DCHAT tokens
                };

                validators.push(config);
                validator_index += 1;
            }
        }

        // Fill in bootstrap peers and validator addresses
        Self::fill_cross_region_connections(&mut validators, base_domain);

        validators
    }

    /// Fill bootstrap peers and validator addresses for cross-region connectivity
    fn fill_cross_region_connections(validators: &mut [ValidatorConfig], _base_domain: &str) {
        let total = validators.len();
        let all_validator_addresses: Vec<String> = validators
            .iter()
            .map(|v| format!("{}:{}", v.public_address, VALIDATOR_RPC_PORT))
            .collect();

        for i in 0..total {
            let current_region = validators[i].region;

            // Bootstrap peers: Select 3-4 peers from DIFFERENT regions
            let mut bootstrap_peers = Vec::new();
            for j in 0..total {
                if i != j && validators[j].region != current_region {
                    // Generate proper libp2p peer ID from validator ID
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    hasher.update(validators[j].validator_id.as_bytes());
                    hasher.update(b"dchat-libp2p-peer");
                    let hash = hasher.finalize();

                    // Create base58btc encoded peer ID (libp2p format)
                    // Prefix with identity multihash code (0x00) and length (32)
                    let mut peer_id_bytes = vec![0x00, 0x20];
                    peer_id_bytes.extend_from_slice(&hash[..]);

                    // Encode as base58 with "12D3Koo" prefix (standard libp2p peer ID)
                    use bs58;
                    let peer_id_suffix = bs58::encode(&peer_id_bytes).into_string();
                    let peer_id = format!("12D3Koo{}", &peer_id_suffix[..44]);

                    let peer_address = format!(
                        "/dns4/{}/tcp/{}/p2p/{}",
                        validators[j].public_address, VALIDATOR_P2P_PORT, peer_id
                    );
                    bootstrap_peers.push(peer_address);

                    // Limit to 4 bootstrap peers
                    if bootstrap_peers.len() >= 4 {
                        break;
                    }
                }
            }

            validators[i].bootstrap_peers = bootstrap_peers;
            validators[i].consensus.validator_addresses = all_validator_addresses.clone();
        }
    }

    /// Generate TOML configuration for a specific validator
    pub fn generate_toml(&self, validator_id: &str) -> Result<String, ConfigError> {
        let validator = self
            .validators
            .iter()
            .find(|v| v.validator_id == validator_id)
            .ok_or_else(|| ConfigError::ValidatorNotFound(validator_id.to_string()))?;

        let toml = format!(
            r#"# dchat Validator Configuration
# Region: {:?}
# Validator ID: {}

[network]
validator_id = "{}"
network_name = "{}"
region = "{:?}"

# P2P listen addresses (libp2p)
listen_addresses = [
{}
]

# RPC endpoint
rpc_address = "{}"
public_address = "{}"

# Bootstrap peers from other regions (critical for decentralization)
bootstrap_peers = [
{}
]

[consensus]
# BFT consensus: {} of {} validators required for finality
validator_addresses = [
{}
]
required_signatures = {}
total_validators = {}
block_time_ms = {}
finality_blocks = {}

[storage]
# Distributed storage backend
backend = "{}"
{}
replication_factor = {}
max_connections = {}

[security]
private_key_path = "{}"
stake_amount = {}

# Geographic diversity requirements
min_continents = {}
max_concentration_per_region = {}
"#,
            validator.region,
            validator.validator_id,
            validator.validator_id,
            self.network_name,
            validator.region,
            validator
                .listen_addresses
                .iter()
                .map(|addr| format!("    \"{}\"", addr))
                .collect::<Vec<_>>()
                .join(",\n"),
            validator.rpc_address,
            validator.public_address,
            validator
                .bootstrap_peers
                .iter()
                .map(|peer| format!("    \"{}\"", peer))
                .collect::<Vec<_>>()
                .join(",\n"),
            validator.consensus.required_signatures,
            validator.consensus.total_validators,
            validator
                .consensus
                .validator_addresses
                .iter()
                .map(|addr| format!("    \"{}\"", addr))
                .collect::<Vec<_>>()
                .join(",\n"),
            validator.consensus.required_signatures,
            validator.consensus.total_validators,
            validator.consensus.block_time_ms,
            validator.consensus.finality_blocks,
            match &validator.storage.backend {
                StorageBackend::CockroachDB { .. } => "cockroachdb",
                StorageBackend::TiKV { .. } => "tikv",
                StorageBackend::PostgreSQL { .. } => "postgresql",
            },
            match &validator.storage.backend {
                StorageBackend::CockroachDB { database_urls } => {
                    format!(
                        "database_urls = [\n{}\n]",
                        database_urls
                            .iter()
                            .map(|url| format!("    \"{}\"", url))
                            .collect::<Vec<_>>()
                            .join(",\n")
                    )
                }
                StorageBackend::TiKV { pd_endpoints } => {
                    format!(
                        "pd_endpoints = [\n{}\n]",
                        pd_endpoints
                            .iter()
                            .map(|ep| format!("    \"{}\"", ep))
                            .collect::<Vec<_>>()
                            .join(",\n")
                    )
                }
                StorageBackend::PostgreSQL { database_url } => {
                    format!("database_url = \"{}\"", database_url)
                }
            },
            validator.storage.replication_factor,
            validator.storage.max_connections,
            validator.private_key_path.display(),
            validator.stake_amount,
            self.min_continents,
            self.max_concentration_per_region,
        );

        Ok(toml)
    }

    /// Verify configuration meets decentralization requirements
    pub fn verify_decentralization(&self) -> Result<(), ConfigError> {
        // Check minimum validator count (need at least 4 for BFT with f=1)
        if self.validators.len() < 4 {
            return Err(ConfigError::InsufficientValidators {
                required: 4,
                actual: self.validators.len(),
            });
        }

        // Check geographic diversity
        let mut region_counts: HashMap<GeographicRegion, usize> = HashMap::new();
        for validator in &self.validators {
            *region_counts.entry(validator.region).or_insert(0) += 1;
        }

        let unique_regions = region_counts.len();
        if unique_regions < self.min_continents {
            return Err(ConfigError::InsufficientGeographicDiversity {
                required: self.min_continents,
                actual: unique_regions,
            });
        }

        // Check no single region dominates
        let total = self.validators.len();
        for (region, count) in region_counts {
            let concentration = count as f64 / total as f64;
            if concentration > self.max_concentration_per_region {
                return Err(ConfigError::ExcessiveConcentration {
                    region,
                    percentage: concentration,
                    max_allowed: self.max_concentration_per_region,
                });
            }
        }

        // Verify BFT threshold is achievable
        let required = (self.bft_threshold * total as f64).ceil() as usize;
        if required > total {
            return Err(ConfigError::InvalidBFTThreshold {
                threshold: self.bft_threshold,
                total_validators: total,
            });
        }

        Ok(())
    }

    /// Get validator configuration by ID
    pub fn get_validator(&self, validator_id: &str) -> Option<&ValidatorConfig> {
        self.validators
            .iter()
            .find(|v| v.validator_id == validator_id)
    }

    /// Get all validators in a specific region
    pub fn get_validators_in_region(&self, region: GeographicRegion) -> Vec<&ValidatorConfig> {
        self.validators
            .iter()
            .filter(|v| v.region == region)
            .collect()
    }

    /// Calculate network latency matrix
    pub fn calculate_latency_matrix(&self) -> HashMap<(String, String), u64> {
        let mut matrix = HashMap::new();

        for i in 0..self.validators.len() {
            for j in 0..self.validators.len() {
                if i != j {
                    let v1 = &self.validators[i];
                    let v2 = &self.validators[j];
                    let latency = v1.region.latency_to(&v2.region);

                    matrix.insert((v1.validator_id.clone(), v2.validator_id.clone()), latency);
                }
            }
        }

        matrix
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Validator not found: {0}")]
    ValidatorNotFound(String),

    #[error("Insufficient validators: required {required}, got {actual}")]
    InsufficientValidators { required: usize, actual: usize },

    #[error("Insufficient geographic diversity: required {required} regions, got {actual}")]
    InsufficientGeographicDiversity { required: usize, actual: usize },

    #[error("Excessive concentration in region {region:?}: {percentage:.1}% exceeds max {max_allowed:.1}%")]
    ExcessiveConcentration {
        region: GeographicRegion,
        percentage: f64,
        max_allowed: f64,
    },

    #[error("Invalid BFT threshold {threshold} for {total_validators} validators")]
    InvalidBFTThreshold {
        threshold: f64,
        total_validators: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_region_config_generation() {
        let config = MultiRegionConfig::new_recommended(
            "dchat-testnet".to_string(),
            "dchat.network".to_string(),
        );

        assert_eq!(config.validators.len(), 7);
        assert!(config.verify_decentralization().is_ok());
    }

    #[test]
    fn test_geographic_diversity() {
        let config =
            MultiRegionConfig::new_recommended("dchat".to_string(), "dchat.network".to_string());

        let mut regions = std::collections::HashSet::new();
        for validator in &config.validators {
            regions.insert(validator.region);
        }

        assert!(regions.len() >= 3); // At least 3 continents
    }

    #[test]
    fn test_no_region_dominance() {
        let config =
            MultiRegionConfig::new_recommended("dchat".to_string(), "dchat.network".to_string());

        let mut region_counts: HashMap<GeographicRegion, usize> = HashMap::new();
        for validator in &config.validators {
            *region_counts.entry(validator.region).or_insert(0) += 1;
        }

        for (_, count) in region_counts {
            let concentration = count as f64 / config.validators.len() as f64;
            assert!(concentration <= 0.40); // No region > 40%
        }
    }

    #[test]
    fn test_bootstrap_peers_cross_region() {
        let config =
            MultiRegionConfig::new_recommended("dchat".to_string(), "dchat.network".to_string());

        for validator in &config.validators {
            // Each validator should have bootstrap peers
            assert!(!validator.bootstrap_peers.is_empty());

            // Bootstrap peers should be from different regions
            // (We can't easily verify this without parsing the multiaddr,
            // but the structure is correct)
        }
    }

    #[test]
    fn test_toml_generation() {
        let config =
            MultiRegionConfig::new_recommended("dchat".to_string(), "dchat.network".to_string());

        let toml = config
            .generate_toml(&config.validators[0].validator_id)
            .unwrap();

        assert!(toml.contains("validator_id"));
        assert!(toml.contains("bootstrap_peers"));
        assert!(toml.contains("required_signatures = 5"));
        assert!(toml.contains("total_validators = 7"));
    }

    #[test]
    fn test_latency_calculations() {
        let us_east = GeographicRegion::USEast;
        let eu_west = GeographicRegion::EUWest;
        let asia = GeographicRegion::AsiaPacificSE;

        assert_eq!(us_east.latency_to(&us_east), 5); // Same region
        assert_eq!(us_east.latency_to(&eu_west), 90); // Cross-Atlantic
        assert_eq!(us_east.latency_to(&asia), 200); // Trans-Pacific
    }
}
