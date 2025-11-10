// Multi-Region Validator Infrastructure
//
// This module provides the infrastructure for deploying and managing validators
// across multiple geographic regions for true decentralization and high availability.
//
// Key Features:
// - Geographic distribution across 7+ regions
// - BFT consensus requiring 5 of 7 validator signatures
// - Automatic region diversity enforcement
// - Health monitoring and failover
// - Kubernetes orchestration support

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::time::SystemTime;
use thiserror::Error;
use tracing::{error, info, warn};

/// Errors that can occur in multi-region validator operations
#[derive(Debug, Error)]
pub enum MultiRegionError {
    #[error("Insufficient validator signatures: got {got}, need {required}")]
    InsufficientSignatures { got: usize, required: usize },

    #[error("Region diversity requirement not met: {details}")]
    RegionDiversityViolation { details: String },

    #[error("Validator {validator_id} not found in region {region:?}")]
    ValidatorNotFound {
        validator_id: String,
        region: GeographicRegion,
    },

    #[error("Health check failed for validator {validator_id}: {reason}")]
    HealthCheckFailed {
        validator_id: String,
        reason: String,
    },

    #[error("Byzantine behavior detected from validator {validator_id}")]
    ByzantineBehavior { validator_id: String },

    #[error("Network partition detected: {details}")]
    NetworkPartition { details: String },

    #[error("Invalid signature from validator {validator_id}")]
    InvalidSignature { validator_id: String },
}

pub type Result<T> = std::result::Result<T, MultiRegionError>;

/// Geographic regions for validator distribution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeographicRegion {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Asia,
    Africa,
    Oceania,
    MiddleEast,
}

impl GeographicRegion {
    /// Returns all supported regions
    pub fn all() -> Vec<Self> {
        vec![
            Self::NorthAmerica,
            Self::SouthAmerica,
            Self::Europe,
            Self::Asia,
            Self::Africa,
            Self::Oceania,
            Self::MiddleEast,
        ]
    }

    /// Returns the continent name
    pub fn name(&self) -> &str {
        match self {
            Self::NorthAmerica => "North America",
            Self::SouthAmerica => "South America",
            Self::Europe => "Europe",
            Self::Asia => "Asia",
            Self::Africa => "Africa",
            Self::Oceania => "Oceania",
            Self::MiddleEast => "Middle East",
        }
    }
}

/// Configuration for a single validator node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    /// Unique identifier for this validator
    pub id: String,

    /// Geographic region where this validator is deployed
    pub region: GeographicRegion,

    /// RPC endpoint address
    pub rpc_address: SocketAddr,

    /// P2P network address
    pub p2p_address: SocketAddr,

    /// Public key for signature verification
    pub public_key: Vec<u8>,

    /// Minimum hardware requirements
    pub hardware: HardwareRequirements,

    /// DNS hostname for this validator
    pub hostname: String,
}

/// Hardware requirements for validator nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareRequirements {
    /// CPU cores (minimum)
    pub cpu_cores: u32,

    /// RAM in GB (minimum)
    pub ram_gb: u32,

    /// Storage in GB (minimum, NVMe SSD)
    pub storage_gb: u32,

    /// Network bandwidth in Mbps (minimum)
    pub bandwidth_mbps: u32,
}

impl Default for HardwareRequirements {
    fn default() -> Self {
        Self {
            cpu_cores: 16,
            ram_gb: 32,
            storage_gb: 1000,
            bandwidth_mbps: 1000,
        }
    }
}

/// Multi-region validator coordinator
pub struct MultiRegionCoordinator {
    /// All configured validators by ID
    validators: HashMap<String, ValidatorConfig>,

    /// Validators grouped by region
    validators_by_region: HashMap<GeographicRegion, Vec<String>>,

    /// Health status of each validator
    health_status: HashMap<String, ValidatorHealth>,

    /// BFT consensus configuration
    bft_config: BftConfig,

    /// Local signing key (if this node is a validator)
    signing_key: Option<SigningKey>,
}

/// Byzantine Fault Tolerance configuration
#[derive(Debug, Clone)]
pub struct BftConfig {
    /// Total number of validators
    pub total_validators: usize,

    /// Number of signatures required for finality (typically 2f+1 where f is Byzantine nodes)
    pub required_signatures: usize,

    /// Minimum number of distinct regions required
    pub min_regions: usize,

    /// Maximum percentage of validators from any single region
    pub max_region_percentage: f64,
}

impl BftConfig {
    /// Compute dynamic BFT thresholds from active validator count
    ///
    /// Uses standard BFT formula:
    /// - f = floor((N - 1) / 3)  -- Maximum Byzantine nodes tolerated
    /// - Required signatures = 2f + 1
    pub fn from_validator_count(
        total_validators: usize,
        min_regions: usize,
        max_region_percentage: f64,
    ) -> Self {
        if total_validators == 0 {
            return Self {
                total_validators: 0,
                required_signatures: 0,
                min_regions,
                max_region_percentage,
            };
        }

        // Standard BFT formula: f = floor((N - 1) / 3)
        let f = (total_validators.saturating_sub(1)) / 3;

        // Required signatures for finality: 2f + 1
        let required_signatures = 2 * f + 1;

        Self {
            total_validators,
            required_signatures,
            min_regions,
            max_region_percentage,
        }
    }

    /// Get Byzantine tolerance (f)
    pub fn byzantine_tolerance(&self) -> usize {
        (self.total_validators.saturating_sub(1)) / 3
    }
}

impl Default for BftConfig {
    fn default() -> Self {
        // Default: 7 validators configuration
        Self::from_validator_count(7, 3, 0.40)
    }
}

/// Health status for a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorHealth {
    /// Validator ID
    pub validator_id: String,

    /// Current health status
    pub status: HealthStatus,

    /// Uptime percentage (last 24 hours)
    pub uptime_percentage: f64,

    /// Average response time in milliseconds
    pub avg_response_time_ms: u64,

    /// Last successful health check timestamp
    pub last_check: SystemTime,

    /// Number of consecutive failures
    pub consecutive_failures: u32,

    /// Block height of this validator
    pub block_height: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unreachable,
}

/// A block finalization signature from a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    /// ID of the signing validator
    pub validator_id: String,

    /// Block hash being signed
    pub block_hash: [u8; 32],

    /// Block height
    pub block_height: u64,

    /// Signature bytes
    pub signature: Vec<u8>,

    /// Timestamp of signature
    pub timestamp: SystemTime,
}

impl MultiRegionCoordinator {
    /// Create a new multi-region coordinator
    pub fn new(bft_config: BftConfig, signing_key: Option<SigningKey>) -> Self {
        Self {
            validators: HashMap::new(),
            validators_by_region: HashMap::new(),
            health_status: HashMap::new(),
            bft_config,
            signing_key,
        }
    }

    /// Register a validator node
    pub fn register_validator(&mut self, config: ValidatorConfig) -> Result<()> {
        let validator_id = config.id.clone();
        let region = config.region;

        info!(
            "Registering validator {} in region {:?}",
            validator_id, region
        );

        // Add to validators map
        self.validators.insert(validator_id.clone(), config);

        // Add to region index
        self.validators_by_region
            .entry(region)
            .or_insert_with(Vec::new)
            .push(validator_id.clone());

        // Initialize health status
        self.health_status.insert(
            validator_id.clone(),
            ValidatorHealth {
                validator_id: validator_id.clone(),
                status: HealthStatus::Healthy,
                uptime_percentage: 100.0,
                avg_response_time_ms: 0,
                last_check: SystemTime::now(),
                consecutive_failures: 0,
                block_height: 0,
            },
        );

        Ok(())
    }

    /// Verify that a set of validator signatures meets BFT requirements
    pub fn verify_bft_signatures(
        &self,
        block_hash: &[u8; 32],
        block_height: u64,
        signatures: &[ValidatorSignature],
    ) -> Result<()> {
        // Check if we have enough signatures
        if signatures.len() < self.bft_config.required_signatures {
            return Err(MultiRegionError::InsufficientSignatures {
                got: signatures.len(),
                required: self.bft_config.required_signatures,
            });
        }

        // Verify each signature
        let mut valid_signatures = Vec::new();
        let mut regions_represented = HashSet::new();
        let mut region_counts: HashMap<GeographicRegion, usize> = HashMap::new();

        for sig in signatures {
            // Verify this is a known validator
            let validator = self.validators.get(&sig.validator_id).ok_or_else(|| {
                // Use NorthAmerica as default for unknown validators (error case)
                // In production, this error shouldn't occur (validators are registered)
                MultiRegionError::ValidatorNotFound {
                    validator_id: sig.validator_id.clone(),
                    region: GeographicRegion::NorthAmerica,
                }
            })?;

            // Verify the signature
            let public_key =
                VerifyingKey::from_bytes(validator.public_key.as_slice().try_into().unwrap())
                    .map_err(|_| MultiRegionError::InvalidSignature {
                        validator_id: sig.validator_id.clone(),
                    })?;

            let signature = Signature::from_bytes(sig.signature.as_slice().try_into().unwrap());

            // Verify signature on block hash
            if public_key.verify(block_hash, &signature).is_ok() {
                valid_signatures.push(sig);
                regions_represented.insert(validator.region);
                *region_counts.entry(validator.region).or_insert(0) += 1;
            } else {
                warn!("Invalid signature from validator {}", sig.validator_id);
            }
        }

        // Check if we still have enough valid signatures
        if valid_signatures.len() < self.bft_config.required_signatures {
            return Err(MultiRegionError::InsufficientSignatures {
                got: valid_signatures.len(),
                required: self.bft_config.required_signatures,
            });
        }

        // Verify geographic diversity
        if regions_represented.len() < self.bft_config.min_regions {
            return Err(MultiRegionError::RegionDiversityViolation {
                details: format!(
                    "Only {} regions represented, need {}",
                    regions_represented.len(),
                    self.bft_config.min_regions
                ),
            });
        }

        // Check that no single region has too many signatures
        for (region, count) in region_counts {
            let percentage = count as f64 / valid_signatures.len() as f64;
            if percentage > self.bft_config.max_region_percentage {
                return Err(MultiRegionError::RegionDiversityViolation {
                    details: format!(
                        "Region {:?} has {:.1}% of signatures (max: {:.1}%)",
                        region,
                        percentage * 100.0,
                        self.bft_config.max_region_percentage * 100.0
                    ),
                });
            }
        }

        info!(
            "BFT verification passed: {} valid signatures from {} regions for block {}",
            valid_signatures.len(),
            regions_represented.len(),
            block_height
        );

        Ok(())
    }

    /// Sign a block with this validator's key
    pub fn sign_block(
        &self,
        validator_id: &str,
        block_hash: &[u8; 32],
        block_height: u64,
    ) -> Result<ValidatorSignature> {
        let signing_key = self.signing_key.as_ref().ok_or_else(|| {
            // Get actual region from validator config or use default
            let region = self
                .validators
                .get(validator_id)
                .map(|v| v.region)
                .unwrap_or(GeographicRegion::NorthAmerica);

            MultiRegionError::ValidatorNotFound {
                validator_id: validator_id.to_string(),
                region,
            }
        })?;

        let signature = signing_key.sign(block_hash);

        Ok(ValidatorSignature {
            validator_id: validator_id.to_string(),
            block_hash: *block_hash,
            block_height,
            signature: signature.to_bytes().to_vec(),
            timestamp: SystemTime::now(),
        })
    }

    /// Update health status for a validator
    pub fn update_health(&mut self, validator_id: &str, health: ValidatorHealth) {
        self.health_status.insert(validator_id.to_string(), health);
    }

    /// Get healthy validators (excluding unhealthy/unreachable)
    pub fn get_healthy_validators(&self) -> Vec<&ValidatorConfig> {
        self.validators
            .values()
            .filter(|v| {
                self.health_status
                    .get(&v.id)
                    .map(|h| matches!(h.status, HealthStatus::Healthy | HealthStatus::Degraded))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Detect network partition
    pub fn detect_partition(&self) -> Option<String> {
        let healthy_validators = self.get_healthy_validators();
        let healthy_count = healthy_validators.len();

        // If less than 2f+1 validators are healthy, we may have a partition
        if healthy_count < self.bft_config.required_signatures {
            return Some(format!(
                "Only {} of {} validators are healthy (need {})",
                healthy_count,
                self.bft_config.total_validators,
                self.bft_config.required_signatures
            ));
        }

        // Check if healthy validators are spread across enough regions
        let mut regions: HashSet<GeographicRegion> = HashSet::new();
        for validator in healthy_validators {
            regions.insert(validator.region);
        }

        if regions.len() < self.bft_config.min_regions {
            return Some(format!(
                "Healthy validators only in {} regions (need {})",
                regions.len(),
                self.bft_config.min_regions
            ));
        }

        None
    }

    /// Get validator statistics by region
    pub fn get_region_stats(&self) -> HashMap<GeographicRegion, RegionStats> {
        let mut stats: HashMap<GeographicRegion, RegionStats> = HashMap::new();

        for (region, validator_ids) in &self.validators_by_region {
            let total = validator_ids.len();
            let healthy = validator_ids
                .iter()
                .filter(|id| {
                    self.health_status
                        .get(*id)
                        .map(|h| matches!(h.status, HealthStatus::Healthy))
                        .unwrap_or(false)
                })
                .count();

            stats.insert(
                *region,
                RegionStats {
                    total_validators: total,
                    healthy_validators: healthy,
                    region: *region,
                },
            );
        }

        stats
    }
}

/// Statistics for a geographic region
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionStats {
    pub region: GeographicRegion,
    pub total_validators: usize,
    pub healthy_validators: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_validator(id: &str, region: GeographicRegion) -> ValidatorConfig {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let public_key = signing_key.verifying_key();

        ValidatorConfig {
            id: id.to_string(),
            region,
            rpc_address: "127.0.0.1:80".parse().unwrap(),
            p2p_address: "127.0.0.1:443".parse().unwrap(),
            public_key: public_key.to_bytes().to_vec(),
            hardware: HardwareRequirements::default(),
            hostname: format!("validator-{}.schikuno.top", id),
        }
    }

    #[test]
    fn test_validator_registration() {
        let mut coordinator = MultiRegionCoordinator::new(BftConfig::default(), None);

        let validator = create_test_validator("us-east-1", GeographicRegion::NorthAmerica);
        coordinator.register_validator(validator).unwrap();

        assert_eq!(coordinator.validators.len(), 1);
        assert_eq!(
            coordinator.validators_by_region[&GeographicRegion::NorthAmerica].len(),
            1
        );
    }

    #[test]
    fn test_geographic_diversity() {
        let mut coordinator = MultiRegionCoordinator::new(BftConfig::default(), None);

        // Register validators across multiple regions
        coordinator
            .register_validator(create_test_validator(
                "us-east",
                GeographicRegion::NorthAmerica,
            ))
            .unwrap();
        coordinator
            .register_validator(create_test_validator(
                "us-west",
                GeographicRegion::NorthAmerica,
            ))
            .unwrap();
        coordinator
            .register_validator(create_test_validator("eu-west", GeographicRegion::Europe))
            .unwrap();
        coordinator
            .register_validator(create_test_validator(
                "eu-central",
                GeographicRegion::Europe,
            ))
            .unwrap();
        coordinator
            .register_validator(create_test_validator("asia-se", GeographicRegion::Asia))
            .unwrap();
        coordinator
            .register_validator(create_test_validator("asia-ne", GeographicRegion::Asia))
            .unwrap();
        coordinator
            .register_validator(create_test_validator(
                "sa-east",
                GeographicRegion::SouthAmerica,
            ))
            .unwrap();

        let stats = coordinator.get_region_stats();
        assert_eq!(stats.len(), 4); // 4 distinct regions
        assert_eq!(stats[&GeographicRegion::NorthAmerica].total_validators, 2);
        assert_eq!(stats[&GeographicRegion::Europe].total_validators, 2);
    }

    #[test]
    fn test_partition_detection() {
        let mut coordinator = MultiRegionCoordinator::new(BftConfig::default(), None);

        // Register 7 validators
        for i in 0..7 {
            let region = match i % 4 {
                0 => GeographicRegion::NorthAmerica,
                1 => GeographicRegion::Europe,
                2 => GeographicRegion::Asia,
                _ => GeographicRegion::SouthAmerica,
            };
            coordinator
                .register_validator(create_test_validator(&format!("v{}", i), region))
                .unwrap();
        }

        // All healthy - no partition
        assert!(coordinator.detect_partition().is_none());

        // Mark some validators as unhealthy
        for i in 0..4 {
            let validator_id = format!("v{}", i);
            if let Some(health) = coordinator.health_status.get_mut(&validator_id) {
                health.status = HealthStatus::Unreachable;
            }
        }

        // Should detect partition (only 3 healthy validators, need 5)
        assert!(coordinator.detect_partition().is_some());
    }
}
