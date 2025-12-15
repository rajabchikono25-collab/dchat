//! Provider Selection and Replication
//!
//! Implements provider selection based on:
//! - Region/geographic proximity
//! - Reputation score
//! - Available capacity
//! - Pricing
//!
//! Also handles replication configuration (e.g., 2 providers in different regions)

use super::capabilities::ProviderCapability;
use super::registry::{ProviderRegistry, RegisteredProvider};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::debug;

use crate::error::StorageResult;

/// Configuration for replication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    /// Minimum number of replicas for each blob
    pub min_replicas: usize,
    /// Target number of replicas (will try to achieve this)
    pub target_replicas: usize,
    /// Maximum number of replicas
    pub max_replicas: usize,
    /// Whether to require different regions for replicas
    pub require_region_diversity: bool,
    /// Minimum number of different regions (if region diversity required)
    pub min_regions: usize,
    /// Weight for reputation in scoring (0.0 to 1.0)
    pub reputation_weight: f64,
    /// Weight for price in scoring (0.0 to 1.0)
    pub price_weight: f64,
    /// Weight for capacity in scoring (0.0 to 1.0)
    pub capacity_weight: f64,
    /// Weight for geographic proximity in scoring (0.0 to 1.0)
    pub proximity_weight: f64,
    /// Preferred regions (in order of preference)
    pub preferred_regions: Vec<String>,
    /// Excluded provider IDs
    pub excluded_providers: Vec<[u8; 32]>,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            min_replicas: 2,
            target_replicas: 2,
            max_replicas: 5,
            require_region_diversity: true,
            min_regions: 2,
            reputation_weight: 0.4,
            price_weight: 0.2,
            capacity_weight: 0.2,
            proximity_weight: 0.2,
            preferred_regions: vec![],
            excluded_providers: vec![],
        }
    }
}

impl ReplicationConfig {
    /// Create config for high availability (3 regions)
    pub fn high_availability() -> Self {
        Self {
            min_replicas: 3,
            target_replicas: 3,
            max_replicas: 5,
            require_region_diversity: true,
            min_regions: 3,
            reputation_weight: 0.5,
            price_weight: 0.1,
            capacity_weight: 0.2,
            proximity_weight: 0.2,
            preferred_regions: vec![],
            excluded_providers: vec![],
        }
    }

    /// Create config for cost-optimized storage
    pub fn cost_optimized() -> Self {
        Self {
            min_replicas: 1,
            target_replicas: 2,
            max_replicas: 2,
            require_region_diversity: false,
            min_regions: 1,
            reputation_weight: 0.2,
            price_weight: 0.6,
            capacity_weight: 0.1,
            proximity_weight: 0.1,
            preferred_regions: vec![],
            excluded_providers: vec![],
        }
    }

    /// Create config for low-latency access
    pub fn low_latency(preferred_region: &str) -> Self {
        Self {
            min_replicas: 2,
            target_replicas: 2,
            max_replicas: 3,
            require_region_diversity: false,
            min_regions: 1,
            reputation_weight: 0.3,
            price_weight: 0.1,
            capacity_weight: 0.1,
            proximity_weight: 0.5,
            preferred_regions: vec![preferred_region.to_string()],
            excluded_providers: vec![],
        }
    }
}

/// Criteria for selecting providers
#[derive(Debug, Clone)]
pub struct SelectionCriteria {
    /// Required storage size in bytes
    pub required_size: u64,
    /// Required capabilities
    pub required_capabilities: Vec<ProviderCapability>,
    /// User's region (for proximity calculation)
    pub user_region: Option<String>,
    /// Maximum price per GB per month (in token units)
    pub max_price_per_gb: Option<u64>,
    /// Minimum reputation score
    pub min_reputation: Option<f64>,
    /// Replication configuration
    pub replication: ReplicationConfig,
}

impl Default for SelectionCriteria {
    fn default() -> Self {
        Self {
            required_size: 0,
            required_capabilities: vec![ProviderCapability::ObjectS3],
            user_region: None,
            max_price_per_gb: None,
            min_reputation: Some(0.5),
            replication: ReplicationConfig::default(),
        }
    }
}

/// Result of provider selection
#[derive(Debug, Clone)]
pub struct ProviderSelection {
    /// Selected providers for primary storage
    pub primary_providers: Vec<SelectedProvider>,
    /// Backup providers (if needed for future replication)
    pub backup_providers: Vec<SelectedProvider>,
    /// Total estimated cost per month
    pub estimated_monthly_cost: u64,
    /// Geographic regions covered
    pub regions_covered: Vec<String>,
    /// Whether selection meets minimum requirements
    pub meets_requirements: bool,
    /// Reason if requirements not met
    pub failure_reason: Option<String>,
}

/// A selected provider with scoring details
#[derive(Debug, Clone)]
pub struct SelectedProvider {
    /// The provider
    pub provider: RegisteredProvider,
    /// Overall score (0.0 to 1.0)
    pub score: f64,
    /// Individual score components
    pub score_breakdown: ScoreBreakdown,
    /// Estimated cost for the requested storage
    pub estimated_cost: u64,
    /// Priority order (1 = highest)
    pub priority: u8,
}

/// Breakdown of scoring components
#[derive(Debug, Clone)]
pub struct ScoreBreakdown {
    pub reputation_score: f64,
    pub price_score: f64,
    pub capacity_score: f64,
    pub proximity_score: f64,
}

/// Provider selector
pub struct ProviderSelector {
    registry: std::sync::Arc<ProviderRegistry>,
}

impl ProviderSelector {
    /// Create a new provider selector
    pub fn new(registry: std::sync::Arc<ProviderRegistry>) -> Self {
        Self { registry }
    }

    /// Select providers based on criteria
    pub async fn select_providers(
        &self,
        criteria: &SelectionCriteria,
    ) -> StorageResult<ProviderSelection> {
        debug!("Selecting providers for {} bytes", criteria.required_size);

        // Get all active providers
        let mut providers = self.registry.list_active_providers().await?;

        // Filter by requirements
        providers = self.filter_providers(providers, criteria);

        if providers.is_empty() {
            return Ok(ProviderSelection {
                primary_providers: vec![],
                backup_providers: vec![],
                estimated_monthly_cost: 0,
                regions_covered: vec![],
                meets_requirements: false,
                failure_reason: Some("No providers meet the requirements".to_string()),
            });
        }

        // Score and rank providers
        let mut scored_providers = self.score_providers(providers, criteria);
        scored_providers.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Select primary providers (respecting replication config)
        let primary_providers =
            self.select_with_diversity(&scored_providers, &criteria.replication)?;

        // Select backup providers
        let primary_ids: HashSet<_> = primary_providers.iter().map(|p| p.provider.id).collect();
        let backup_providers: Vec<_> = scored_providers
            .into_iter()
            .filter(|p| !primary_ids.contains(&p.provider.id))
            .take(3) // Keep up to 3 backups
            .collect();

        // Calculate stats
        let regions_covered: Vec<_> = primary_providers
            .iter()
            .map(|p| p.provider.primary_region.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let estimated_monthly_cost: u64 = primary_providers.iter().map(|p| p.estimated_cost).sum();

        let meets_requirements = primary_providers.len() >= criteria.replication.min_replicas
            && (!criteria.replication.require_region_diversity
                || regions_covered.len() >= criteria.replication.min_regions);

        let failure_reason = if !meets_requirements {
            if primary_providers.len() < criteria.replication.min_replicas {
                Some(format!(
                    "Only {} providers available, need {}",
                    primary_providers.len(),
                    criteria.replication.min_replicas
                ))
            } else {
                Some(format!(
                    "Only {} regions covered, need {}",
                    regions_covered.len(),
                    criteria.replication.min_regions
                ))
            }
        } else {
            None
        };

        Ok(ProviderSelection {
            primary_providers,
            backup_providers,
            estimated_monthly_cost,
            regions_covered,
            meets_requirements,
            failure_reason,
        })
    }

    /// Filter providers by criteria
    fn filter_providers(
        &self,
        providers: Vec<RegisteredProvider>,
        criteria: &SelectionCriteria,
    ) -> Vec<RegisteredProvider> {
        providers
            .into_iter()
            .filter(|p| {
                // Check excluded providers
                if criteria.replication.excluded_providers.contains(&p.id) {
                    return false;
                }

                // Check capabilities
                for cap in &criteria.required_capabilities {
                    if !p.has_capability(*cap) {
                        return false;
                    }
                }

                // Check capacity
                if p.available_storage < criteria.required_size {
                    return false;
                }

                // Check reputation
                if let Some(min_rep) = criteria.min_reputation {
                    if p.reputation < min_rep {
                        return false;
                    }
                }

                // Check price
                if let Some(max_price) = criteria.max_price_per_gb {
                    if p.s3_price_per_gb_month() > max_price {
                        return false;
                    }
                }

                true
            })
            .collect()
    }

    /// Score providers based on criteria
    fn score_providers(
        &self,
        providers: Vec<RegisteredProvider>,
        criteria: &SelectionCriteria,
    ) -> Vec<SelectedProvider> {
        // Calculate normalization factors
        let max_price = providers
            .iter()
            .map(|p| p.s3_price_per_gb_month())
            .max()
            .unwrap_or(1);
        let max_capacity = providers
            .iter()
            .map(|p| p.available_storage)
            .max()
            .unwrap_or(1);

        providers
            .into_iter()
            .map(|p| {
                let breakdown =
                    self.calculate_score_breakdown(&p, criteria, max_price, max_capacity);
                let score = self.calculate_weighted_score(&breakdown, &criteria.replication);
                let estimated_cost = self.calculate_cost(&p, criteria.required_size);

                SelectedProvider {
                    provider: p,
                    score,
                    score_breakdown: breakdown,
                    estimated_cost,
                    priority: 0, // Will be set during selection
                }
            })
            .collect()
    }

    /// Calculate score breakdown for a provider
    fn calculate_score_breakdown(
        &self,
        provider: &RegisteredProvider,
        criteria: &SelectionCriteria,
        max_price: u64,
        max_capacity: u64,
    ) -> ScoreBreakdown {
        // Reputation score (already 0.0 to 1.0)
        let reputation_score = provider.reputation;

        // Price score (lower is better, inverted)
        let price_score = if max_price > 0 {
            1.0 - (provider.s3_price_per_gb_month() as f64 / max_price as f64)
        } else {
            1.0
        };

        // Capacity score (higher is better)
        let capacity_score = if max_capacity > 0 {
            provider.available_storage as f64 / max_capacity as f64
        } else {
            1.0
        };

        // Proximity score
        let proximity_score = self.calculate_proximity_score(provider, criteria);

        ScoreBreakdown {
            reputation_score,
            price_score,
            capacity_score,
            proximity_score,
        }
    }

    /// Calculate proximity score based on region
    fn calculate_proximity_score(
        &self,
        provider: &RegisteredProvider,
        criteria: &SelectionCriteria,
    ) -> f64 {
        // Check preferred regions
        if let Some(pos) = criteria
            .replication
            .preferred_regions
            .iter()
            .position(|r| *r == provider.primary_region)
        {
            // Higher score for earlier in the preference list
            return 1.0 - (pos as f64 * 0.1).min(0.5);
        }

        // Check user region
        if let Some(user_region) = &criteria.user_region {
            if provider.serves_region(user_region) {
                return 0.9;
            }
            // Check if same continent (simplified)
            if same_continent(user_region, &provider.primary_region) {
                return 0.6;
            }
        }

        0.3 // Default score for distant providers
    }

    /// Calculate weighted score
    fn calculate_weighted_score(
        &self,
        breakdown: &ScoreBreakdown,
        config: &ReplicationConfig,
    ) -> f64 {
        breakdown.reputation_score * config.reputation_weight
            + breakdown.price_score * config.price_weight
            + breakdown.capacity_score * config.capacity_weight
            + breakdown.proximity_score * config.proximity_weight
    }

    /// Calculate estimated cost
    fn calculate_cost(&self, provider: &RegisteredProvider, size: u64) -> u64 {
        let gb = (size as f64 / 1_073_741_824.0).ceil() as u64;
        provider.s3_price_per_gb_month() * gb
    }

    /// Select providers with region diversity
    fn select_with_diversity(
        &self,
        scored_providers: &[SelectedProvider],
        config: &ReplicationConfig,
    ) -> StorageResult<Vec<SelectedProvider>> {
        let mut selected: Vec<SelectedProvider> = Vec::new();
        let mut regions_used: HashSet<String> = HashSet::new();
        let mut priority = 1u8;

        for sp in scored_providers {
            // Check if we've reached target
            if selected.len() >= config.target_replicas {
                break;
            }

            // Check region diversity requirement
            if config.require_region_diversity && regions_used.contains(&sp.provider.primary_region)
            {
                // Skip if we need more region diversity
                if regions_used.len() < config.min_regions
                    && scored_providers
                        .iter()
                        .any(|p| !regions_used.contains(&p.provider.primary_region))
                {
                    continue;
                }
            }

            let mut provider = sp.clone();
            provider.priority = priority;
            priority += 1;

            regions_used.insert(sp.provider.primary_region.clone());
            selected.push(provider);
        }

        // If we didn't get enough providers, relax diversity requirements
        if selected.len() < config.min_replicas {
            for sp in scored_providers {
                if selected.len() >= config.target_replicas {
                    break;
                }
                if selected.iter().any(|s| s.provider.id == sp.provider.id) {
                    continue;
                }

                let mut provider = sp.clone();
                provider.priority = priority;
                priority += 1;
                selected.push(provider);
            }
        }

        Ok(selected)
    }
}

/// Check if two regions are on the same continent (simplified)
fn same_continent(region1: &str, region2: &str) -> bool {
    let continent1 = get_continent(region1);
    let continent2 = get_continent(region2);
    continent1 == continent2
}

/// Get continent for a region (simplified mapping)
fn get_continent(region: &str) -> &str {
    if region.starts_with("us-") || region.starts_with("ca-") {
        "north-america"
    } else if region.starts_with("eu-") || region.starts_with("uk-") {
        "europe"
    } else if region.starts_with("ap-") || region.starts_with("cn-") || region.starts_with("in-") {
        "asia"
    } else if region.starts_with("sa-") {
        "south-america"
    } else if region.starts_with("af-") {
        "africa"
    } else if region.starts_with("me-") {
        "middle-east"
    } else if region.starts_with("au-") {
        "oceania"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replication_config_defaults() {
        let config = ReplicationConfig::default();
        assert_eq!(config.min_replicas, 2);
        assert_eq!(config.target_replicas, 2);
        assert!(config.require_region_diversity);
    }

    #[test]
    fn test_high_availability_config() {
        let config = ReplicationConfig::high_availability();
        assert_eq!(config.min_replicas, 3);
        assert_eq!(config.min_regions, 3);
    }

    #[test]
    fn test_cost_optimized_config() {
        let config = ReplicationConfig::cost_optimized();
        assert_eq!(config.min_replicas, 1);
        assert!(!config.require_region_diversity);
        assert_eq!(config.price_weight, 0.6);
    }

    #[test]
    fn test_same_continent() {
        assert!(same_continent("us-east-1", "us-west-2"));
        assert!(same_continent("eu-west-1", "eu-central-1"));
        assert!(!same_continent("us-east-1", "eu-west-1"));
        assert!(!same_continent("ap-northeast-1", "us-west-2"));
    }

    #[test]
    fn test_get_continent() {
        assert_eq!(get_continent("us-east-1"), "north-america");
        assert_eq!(get_continent("eu-west-1"), "europe");
        assert_eq!(get_continent("ap-southeast-1"), "asia");
        assert_eq!(get_continent("sa-east-1"), "south-america");
    }
}
