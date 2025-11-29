//! Path Selection for Onion Routing
//!
//! Selects relay nodes for circuits with geographic diversity and reputation filtering.

use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashMap;
use thiserror::Error;

/// Minimum reputation score for relay selection
pub const MIN_REPUTATION_SCORE: f64 = 0.6;

/// Minimum uptime percentage for relay selection (70%)
pub const MIN_UPTIME_PERCENT: f64 = 70.0;

/// Maximum load factor (connections/capacity) for relay selection
pub const MAX_LOAD_FACTOR: f64 = 0.85;

/// Errors that can occur during path selection
#[derive(Debug, Error)]
pub enum PathSelectionError {
    #[error("Insufficient relays: need {required}, found {available}")]
    InsufficientRelays { required: usize, available: usize },

    #[error("No suitable relays in region: {0}")]
    NoSuitableRelaysInRegion(String),

    #[error("Geographic diversity requirement not met: {0}")]
    GeographicDiversityFailed(String),

    #[error("All candidate relays are overloaded")]
    AllRelaysOverloaded,

    #[error("Reputation threshold not met for any relay")]
    NoReputationMeetingThreshold,
}

/// Information about a relay node
#[derive(Debug, Clone)]
pub struct RelayInfo {
    /// Unique peer ID
    pub peer_id: String,
    /// Geographic region (e.g., "us-west", "eu-central")
    pub region: String,
    /// Reputation score (0.0 to 1.0)
    pub reputation: f64,
    /// Uptime percentage (0.0 to 100.0)
    pub uptime_percent: f64,
    /// Current load factor (connections/capacity)
    pub load_factor: f64,
    /// Average latency in milliseconds
    pub latency_ms: f64,
    /// Whether this relay is currently available
    pub available: bool,
}

impl RelayInfo {
    /// Check if this relay meets minimum quality requirements
    pub fn meets_requirements(&self) -> bool {
        self.available
            && self.reputation >= MIN_REPUTATION_SCORE
            && self.uptime_percent >= MIN_UPTIME_PERCENT
            && self.load_factor <= MAX_LOAD_FACTOR
    }

    /// Calculate a selection score for weighted random selection
    pub fn selection_score(&self) -> f64 {
        if !self.meets_requirements() {
            return 0.0;
        }

        // Score based on reputation, uptime, and inverse of load/latency
        let reputation_weight = 0.4;
        let uptime_weight = 0.3;
        let load_weight = 0.2;
        let latency_weight = 0.1;

        let reputation_score = self.reputation;
        let uptime_score = self.uptime_percent / 100.0;
        let load_score = 1.0 - self.load_factor;
        let latency_score = 1.0 / (1.0 + self.latency_ms / 1000.0);

        reputation_weight * reputation_score
            + uptime_weight * uptime_score
            + load_weight * load_score
            + latency_weight * latency_score
    }
}

/// Selects paths for onion routing circuits
pub struct PathSelector {
    /// Available relay nodes indexed by peer ID
    relays: HashMap<String, RelayInfo>,
    /// Relays grouped by region for diversity
    relays_by_region: HashMap<String, Vec<String>>,
}

impl PathSelector {
    /// Create a new path selector
    pub fn new() -> Self {
        Self {
            relays: HashMap::new(),
            relays_by_region: HashMap::new(),
        }
    }

    /// Add or update a relay
    pub fn add_relay(&mut self, relay: RelayInfo) {
        let peer_id = relay.peer_id.clone();
        let region = relay.region.clone();

        // Update relay info
        self.relays.insert(peer_id.clone(), relay);

        // Update region index
        self.relays_by_region
            .entry(region)
            .or_insert_with(Vec::new)
            .push(peer_id);
    }

    /// Remove a relay
    pub fn remove_relay(&mut self, peer_id: &str) {
        if let Some(relay) = self.relays.remove(peer_id) {
            // Remove from region index
            if let Some(region_relays) = self.relays_by_region.get_mut(&relay.region) {
                region_relays.retain(|id| id != peer_id);
            }
        }
    }

    /// Get all available regions
    pub fn available_regions(&self) -> Vec<String> {
        self.relays_by_region
            .iter()
            .filter(|(_, relays)| {
                relays.iter().any(|peer_id| {
                    self.relays
                        .get(peer_id)
                        .map(|r| r.meets_requirements())
                        .unwrap_or(false)
                })
            })
            .map(|(region, _)| region.clone())
            .collect()
    }

    /// Select a path with geographic diversity
    pub fn select_path(&self, num_hops: usize) -> Result<Vec<RelayInfo>, PathSelectionError> {
        if num_hops == 0 {
            return Ok(Vec::new());
        }

        // Get suitable relays
        let suitable_relays: Vec<&RelayInfo> = self
            .relays
            .values()
            .filter(|r| r.meets_requirements())
            .collect();

        if suitable_relays.len() < num_hops {
            return Err(PathSelectionError::InsufficientRelays {
                required: num_hops,
                available: suitable_relays.len(),
            });
        }

        // Get available regions
        let available_regions = self.available_regions();
        if available_regions.len() < num_hops {
            return Err(PathSelectionError::GeographicDiversityFailed(format!(
                "Need {} different regions, only {} available",
                num_hops,
                available_regions.len()
            )));
        }

        // Select regions (ensuring diversity)
        let selected_regions = self.select_diverse_regions(num_hops)?;

        // Select one relay from each region
        let mut selected_relays = Vec::new();
        let mut rng = rand::thread_rng();

        for region in selected_regions {
            let relay = self.select_best_relay_in_region(&region, &mut rng)?;
            selected_relays.push(relay);
        }

        Ok(selected_relays)
    }

    /// Select diverse regions for a circuit
    fn select_diverse_regions(
        &self,
        num_regions: usize,
    ) -> Result<Vec<String>, PathSelectionError> {
        let available_regions = self.available_regions();

        if available_regions.len() < num_regions {
            return Err(PathSelectionError::GeographicDiversityFailed(format!(
                "Need {} regions, only {} available",
                num_regions,
                available_regions.len()
            )));
        }

        // Randomly select regions ensuring no duplicates
        let mut rng = rand::thread_rng();
        let selected = available_regions
            .choose_multiple(&mut rng, num_regions)
            .cloned()
            .collect();

        Ok(selected)
    }

    /// Select the best relay in a region using weighted random selection
    fn select_best_relay_in_region(
        &self,
        region: &str,
        rng: &mut impl Rng,
    ) -> Result<RelayInfo, PathSelectionError> {
        let region_relays = self
            .relays_by_region
            .get(region)
            .ok_or_else(|| PathSelectionError::NoSuitableRelaysInRegion(region.to_string()))?;

        // Filter suitable relays and calculate selection scores
        let candidates: Vec<(RelayInfo, f64)> = region_relays
            .iter()
            .filter_map(|peer_id| self.relays.get(peer_id))
            .filter(|r| r.meets_requirements())
            .map(|r| (r.clone(), r.selection_score()))
            .filter(|(_, score)| *score > 0.0)
            .collect();

        if candidates.is_empty() {
            return Err(PathSelectionError::NoSuitableRelaysInRegion(
                region.to_string(),
            ));
        }

        // Weighted random selection
        let total_score: f64 = candidates.iter().map(|(_, score)| score).sum();
        let mut selection_point = rng.gen::<f64>() * total_score;

        for (relay, score) in &candidates {
            selection_point -= score;
            if selection_point <= 0.0 {
                return Ok(relay.clone());
            }
        }

        // Fallback to last candidate (shouldn't reach here)
        Ok(candidates.last().unwrap().0.clone())
    }

    /// Get statistics about available relays
    pub fn get_stats(&self) -> PathSelectionStats {
        let total_relays = self.relays.len();
        let suitable_relays = self
            .relays
            .values()
            .filter(|r| r.meets_requirements())
            .count();
        let available_regions = self.available_regions().len();

        let avg_reputation = if total_relays > 0 {
            self.relays.values().map(|r| r.reputation).sum::<f64>() / total_relays as f64
        } else {
            0.0
        };

        let avg_load = if total_relays > 0 {
            self.relays.values().map(|r| r.load_factor).sum::<f64>() / total_relays as f64
        } else {
            0.0
        };

        PathSelectionStats {
            total_relays,
            suitable_relays,
            available_regions,
            avg_reputation,
            avg_load,
        }
    }
}

impl Default for PathSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// Path selection statistics
#[derive(Debug, Clone)]
pub struct PathSelectionStats {
    pub total_relays: usize,
    pub suitable_relays: usize,
    pub available_regions: usize,
    pub avg_reputation: f64,
    pub avg_load: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn create_test_relay(peer_id: &str, region: &str, reputation: f64, load: f64) -> RelayInfo {
        RelayInfo {
            peer_id: peer_id.to_string(),
            region: region.to_string(),
            reputation,
            uptime_percent: 95.0,
            load_factor: load,
            latency_ms: 50.0,
            available: true,
        }
    }

    #[test]
    fn test_relay_meets_requirements() {
        let good_relay = create_test_relay("peer1", "us-west", 0.8, 0.5);
        assert!(good_relay.meets_requirements());

        let low_reputation = create_test_relay("peer2", "us-west", 0.4, 0.5);
        assert!(!low_reputation.meets_requirements());

        let overloaded = create_test_relay("peer3", "us-west", 0.8, 0.95);
        assert!(!overloaded.meets_requirements());
    }

    #[test]
    fn test_relay_selection_score() {
        let relay = create_test_relay("peer1", "us-west", 0.9, 0.3);
        let score = relay.selection_score();
        assert!(score > 0.0);
        assert!(score <= 1.0);

        let bad_relay = create_test_relay("peer2", "us-west", 0.3, 0.95);
        assert_eq!(bad_relay.selection_score(), 0.0);
    }

    #[test]
    fn test_path_selector_add_remove() {
        let mut selector = PathSelector::new();
        let relay = create_test_relay("peer1", "us-west", 0.8, 0.5);

        selector.add_relay(relay.clone());
        assert_eq!(selector.relays.len(), 1);
        assert_eq!(selector.relays_by_region.get("us-west").unwrap().len(), 1);

        selector.remove_relay("peer1");
        assert_eq!(selector.relays.len(), 0);
    }

    #[test]
    fn test_available_regions() {
        let mut selector = PathSelector::new();

        selector.add_relay(create_test_relay("peer1", "us-west", 0.8, 0.5));
        selector.add_relay(create_test_relay("peer2", "eu-central", 0.9, 0.4));
        selector.add_relay(create_test_relay("peer3", "ap-south", 0.7, 0.6));

        let regions = selector.available_regions();
        assert_eq!(regions.len(), 3);
        assert!(regions.contains(&"us-west".to_string()));
        assert!(regions.contains(&"eu-central".to_string()));
        assert!(regions.contains(&"ap-south".to_string()));
    }

    #[test]
    fn test_select_path_insufficient_relays() {
        let mut selector = PathSelector::new();

        selector.add_relay(create_test_relay("peer1", "us-west", 0.8, 0.5));
        selector.add_relay(create_test_relay("peer2", "eu-central", 0.9, 0.4));

        let result = selector.select_path(3);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PathSelectionError::InsufficientRelays { .. }
        ));
    }

    #[test]
    fn test_select_path_geographic_diversity() {
        let mut selector = PathSelector::new();

        selector.add_relay(create_test_relay("peer1", "us-west", 0.8, 0.5));
        selector.add_relay(create_test_relay("peer2", "eu-central", 0.9, 0.4));
        selector.add_relay(create_test_relay("peer3", "ap-south", 0.7, 0.6));
        selector.add_relay(create_test_relay("peer4", "af-south", 0.85, 0.3));

        let path = selector.select_path(3).unwrap();
        assert_eq!(path.len(), 3);

        // Ensure all regions are different
        let regions: HashSet<String> = path.iter().map(|r| r.region.clone()).collect();
        assert_eq!(regions.len(), 3);
    }

    #[test]
    fn test_select_path_filters_unsuitable_relays() {
        let mut selector = PathSelector::new();

        selector.add_relay(create_test_relay("peer1", "us-west", 0.8, 0.5));
        selector.add_relay(create_test_relay("peer2", "eu-central", 0.3, 0.4)); // Low reputation
        selector.add_relay(create_test_relay("peer3", "ap-south", 0.9, 0.95)); // Overloaded

        let result = selector.select_path(3);
        assert!(result.is_err()); // Only 1 suitable relay
    }

    #[test]
    fn test_select_diverse_regions() {
        let mut selector = PathSelector::new();

        selector.add_relay(create_test_relay("peer1", "us-west", 0.8, 0.5));
        selector.add_relay(create_test_relay("peer2", "eu-central", 0.9, 0.4));
        selector.add_relay(create_test_relay("peer3", "ap-south", 0.7, 0.6));
        selector.add_relay(create_test_relay("peer4", "af-south", 0.85, 0.3));

        let regions = selector.select_diverse_regions(3).unwrap();
        assert_eq!(regions.len(), 3);

        // Ensure no duplicates
        let unique_regions: HashSet<_> = regions.iter().collect();
        assert_eq!(unique_regions.len(), 3);
    }

    #[test]
    fn test_get_stats() {
        let mut selector = PathSelector::new();

        selector.add_relay(create_test_relay("peer1", "us-west", 0.8, 0.5));
        selector.add_relay(create_test_relay("peer2", "eu-central", 0.9, 0.4));
        selector.add_relay(create_test_relay("peer3", "ap-south", 0.3, 0.95)); // Unsuitable

        let stats = selector.get_stats();
        assert_eq!(stats.total_relays, 3);
        assert_eq!(stats.suitable_relays, 2);
        assert_eq!(stats.available_regions, 2);
        assert!(stats.avg_reputation > 0.0);
    }

    #[test]
    fn test_weighted_selection_favors_high_score() {
        let mut selector = PathSelector::new();

        // Add multiple relays in same region with different scores
        selector.add_relay(create_test_relay("low", "us-west", 0.6, 0.8));
        selector.add_relay(create_test_relay("high", "us-west", 0.95, 0.2));

        // Select multiple times and verify high-score relay is chosen more often
        let mut high_count = 0;
        let iterations = 1000; // Increased for statistical significance

        for _ in 0..iterations {
            let mut rng = rand::thread_rng();
            let relay = selector
                .select_best_relay_in_region("us-west", &mut rng)
                .unwrap();
            if relay.peer_id == "high" {
                high_count += 1;
            }
        }

        // High score relay should be chosen significantly more often
        // With weighted random selection, expect at least 50% selection rate
        // Using 50% threshold to avoid flaky tests while still validating bias
        assert!(
            high_count > 500,
            "High score relay selected {} out of {} times (expected >500)",
            high_count,
            iterations
        );
    }

    #[test]
    fn test_path_selection_with_exact_hops() {
        let mut selector = PathSelector::new();

        selector.add_relay(create_test_relay("peer1", "us-west", 0.8, 0.5));
        selector.add_relay(create_test_relay("peer2", "eu-central", 0.9, 0.4));
        selector.add_relay(create_test_relay("peer3", "ap-south", 0.7, 0.6));
        selector.add_relay(create_test_relay("peer4", "af-south", 0.85, 0.3));
        selector.add_relay(create_test_relay("peer5", "sa-east", 0.8, 0.5));

        // Test different hop counts
        for hops in 3..=5 {
            let path = selector.select_path(hops).unwrap();
            assert_eq!(path.len(), hops);

            // Verify geographic diversity
            let regions: HashSet<String> = path.iter().map(|r| r.region.clone()).collect();
            assert_eq!(regions.len(), hops);
        }
    }

    #[test]
    fn test_empty_selector() {
        let selector = PathSelector::new();
        let result = selector.select_path(3);
        assert!(result.is_err());

        let stats = selector.get_stats();
        assert_eq!(stats.total_relays, 0);
        assert_eq!(stats.suitable_relays, 0);
    }
}
