// BFT Threshold Calculation and Dynamic Validator Set Management
//
// This module provides dynamic threshold calculation for Byzantine Fault Tolerance,
// replacing hardcoded values with runtime-computed thresholds based on active validator set.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dynamic BFT thresholds computed from active validator set
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BftThresholds {
    /// Total number of active validators
    pub total_validators: usize,

    /// Maximum number of Byzantine (faulty) validators tolerated
    /// Computed as f = floor((N - 1) / 3)
    pub byzantine_tolerance: usize,

    /// Minimum signatures required for finality
    /// Computed as 2f + 1
    pub required_signatures: usize,

    /// Minimum number of distinct geographic regions required
    pub min_regions: usize,

    /// Maximum percentage of validators from any single region
    pub max_region_percentage: f64,
}

impl BftThresholds {
    /// Compute BFT thresholds from active validator count
    ///
    /// Uses standard BFT formula:
    /// - f = floor((N - 1) / 3)  -- Maximum Byzantine nodes tolerated
    /// - Required signatures = 2f + 1
    ///
    /// # Examples
    ///
    /// ```
    /// use dchat::validator::thresholds::BftThresholds;
    ///
    /// // With 7 validators: f=2, required=5
    /// let thresholds = BftThresholds::from_validator_count(7, 3, 0.40);
    /// assert_eq!(thresholds.byzantine_tolerance, 2);
    /// assert_eq!(thresholds.required_signatures, 5);
    ///
    /// // With 4 validators: f=1, required=3
    /// let thresholds = BftThresholds::from_validator_count(4, 3, 0.40);
    /// assert_eq!(thresholds.byzantine_tolerance, 1);
    /// assert_eq!(thresholds.required_signatures, 3);
    /// ```
    pub fn from_validator_count(
        total_validators: usize,
        min_regions: usize,
        max_region_percentage: f64,
    ) -> Self {
        if total_validators == 0 {
            return Self {
                total_validators: 0,
                byzantine_tolerance: 0,
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
            byzantine_tolerance: f,
            required_signatures,
            min_regions,
            max_region_percentage,
        }
    }

    /// Check if the current connected validator count meets consensus requirements
    pub fn can_achieve_consensus(&self, connected_validators: usize) -> bool {
        connected_validators >= self.required_signatures
    }

    /// Get consensus health as percentage (connected / required)
    pub fn consensus_health_percentage(&self, connected_validators: usize) -> f64 {
        if self.required_signatures == 0 {
            return 0.0;
        }
        (connected_validators as f64 / self.required_signatures as f64) * 100.0
    }

    /// Check if we're in danger zone (close to losing consensus)
    /// Returns true if we're within 1 validator of losing consensus
    pub fn is_near_consensus_loss(&self, connected_validators: usize) -> bool {
        connected_validators <= self.required_signatures
    }
}

/// Region diversity metrics and validation
#[derive(Debug, Clone)]
pub struct RegionDiversityMetrics {
    /// Validators per region
    pub region_distribution: HashMap<String, usize>,

    /// Total validators counted
    pub total_validators: usize,

    /// Number of distinct regions represented
    pub regions_represented: usize,
}

impl RegionDiversityMetrics {
    /// Create new metrics from validator region data
    pub fn new() -> Self {
        Self {
            region_distribution: HashMap::new(),
            total_validators: 0,
            regions_represented: 0,
        }
    }

    /// Add a validator from a specific region
    pub fn add_validator(&mut self, region: String) {
        *self.region_distribution.entry(region).or_insert(0) += 1;
        self.total_validators += 1;
        self.regions_represented = self.region_distribution.len();
    }

    /// Get percentage of validators in a specific region
    pub fn region_percentage(&self, region: &str) -> f64 {
        if self.total_validators == 0 {
            return 0.0;
        }

        let count = self.region_distribution.get(region).copied().unwrap_or(0);
        count as f64 / self.total_validators as f64
    }

    /// Get the region with the highest concentration
    pub fn max_region_concentration(&self) -> Option<(String, f64)> {
        self.region_distribution
            .iter()
            .map(|(region, &count)| {
                let percentage = count as f64 / self.total_validators as f64;
                (region.clone(), percentage)
            })
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
    }

    /// Check if diversity requirements are met
    pub fn check_diversity(
        &self,
        min_regions: usize,
        max_region_percentage: f64,
    ) -> Result<(), DiversityViolation> {
        // Check minimum regions
        if self.regions_represented < min_regions {
            return Err(DiversityViolation::InsufficientRegions {
                required: min_regions,
                actual: self.regions_represented,
            });
        }

        // Check maximum region concentration
        if let Some((region, percentage)) = self.max_region_concentration() {
            if percentage > max_region_percentage {
                return Err(DiversityViolation::RegionOverConcentration {
                    region,
                    percentage,
                    max_allowed: max_region_percentage,
                });
            }
        }

        Ok(())
    }

    /// Get warning level based on approaching thresholds
    pub fn get_warning_level(
        &self,
        min_regions: usize,
        max_region_percentage: f64,
        warning_threshold: f64,
    ) -> DiversityWarningLevel {
        // Critical: diversity requirement violated
        if self
            .check_diversity(min_regions, max_region_percentage)
            .is_err()
        {
            return DiversityWarningLevel::Critical;
        }

        // Warning: approaching max concentration
        if let Some((_, percentage)) = self.max_region_concentration() {
            if percentage >= warning_threshold {
                return DiversityWarningLevel::Warning;
            }
        }

        // Check if close to minimum regions
        if self.regions_represented == min_regions {
            return DiversityWarningLevel::Warning;
        }

        DiversityWarningLevel::Healthy
    }
}

impl Default for RegionDiversityMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiversityViolation {
    InsufficientRegions {
        required: usize,
        actual: usize,
    },
    RegionOverConcentration {
        region: String,
        percentage: f64,
        max_allowed: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiversityWarningLevel {
    Healthy,
    Warning,
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bft_thresholds_7_validators() {
        let thresholds = BftThresholds::from_validator_count(7, 3, 0.40);
        assert_eq!(thresholds.total_validators, 7);
        assert_eq!(thresholds.byzantine_tolerance, 2);
        assert_eq!(thresholds.required_signatures, 5);
    }

    #[test]
    fn test_bft_thresholds_4_validators() {
        let thresholds = BftThresholds::from_validator_count(4, 3, 0.40);
        assert_eq!(thresholds.byzantine_tolerance, 1);
        assert_eq!(thresholds.required_signatures, 3);
    }

    #[test]
    fn test_bft_thresholds_10_validators() {
        let thresholds = BftThresholds::from_validator_count(10, 3, 0.40);
        assert_eq!(thresholds.byzantine_tolerance, 3);
        assert_eq!(thresholds.required_signatures, 7);
    }

    #[test]
    fn test_consensus_health() {
        let thresholds = BftThresholds::from_validator_count(7, 3, 0.40);

        assert!(thresholds.can_achieve_consensus(5));
        assert!(thresholds.can_achieve_consensus(6));
        assert!(!thresholds.can_achieve_consensus(4));

        assert_eq!(thresholds.consensus_health_percentage(5), 100.0);
        assert_eq!(thresholds.consensus_health_percentage(7), 140.0);
    }

    #[test]
    fn test_region_diversity() {
        let mut metrics = RegionDiversityMetrics::new();

        metrics.add_validator("us-east".to_string());
        metrics.add_validator("us-east".to_string());
        metrics.add_validator("eu-west".to_string());
        metrics.add_validator("asia-se".to_string());

        assert_eq!(metrics.total_validators, 4);
        assert_eq!(metrics.regions_represented, 3);
        assert_eq!(metrics.region_percentage("us-east"), 0.5);

        // Should violate 40% rule
        assert!(metrics.check_diversity(3, 0.40).is_err());

        // Add more validators to balance
        metrics.add_validator("eu-west".to_string());
        metrics.add_validator("asia-se".to_string());

        // Now should pass (2/6 = 33%)
        assert!(metrics.check_diversity(3, 0.40).is_ok());
    }

    #[test]
    fn test_diversity_warning_levels() {
        let mut metrics = RegionDiversityMetrics::new();

        // Start balanced: 4 regions with 2 validators each = 25% each
        metrics.add_validator("region1".to_string());
        metrics.add_validator("region1".to_string());
        metrics.add_validator("region2".to_string());
        metrics.add_validator("region2".to_string());
        metrics.add_validator("region3".to_string());
        metrics.add_validator("region3".to_string());
        metrics.add_validator("region4".to_string());
        metrics.add_validator("region4".to_string());

        assert_eq!(
            metrics.get_warning_level(3, 0.40, 0.35),
            DiversityWarningLevel::Healthy // 4 regions > 3 minimum, 25% < 35%
        );

        // Add 3 more to region1 (5/11 = 45.4% > 40% critical threshold)
        metrics.add_validator("region1".to_string());
        metrics.add_validator("region1".to_string());
        metrics.add_validator("region1".to_string());

        assert_eq!(
            metrics.get_warning_level(3, 0.40, 0.35),
            DiversityWarningLevel::Critical // 45.4% > 40% critical
        );
    }
}
