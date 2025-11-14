// Relay Reputation Scorer
//
// Tracks relay node performance metrics and calculates reputation scores
// using a weighted algorithm:
// - Uptime: 30%
// - Latency: 25%
// - Delivery Success: 30%
// - Geographic Diversity: 15%

use ed25519_dalek::VerifyingKey;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime};
use thiserror::Error;


/// Reputation scoring constants
pub const UPTIME_WEIGHT: f64 = 0.30;
pub const LATENCY_WEIGHT: f64 = 0.25;
pub const DELIVERY_SUCCESS_WEIGHT: f64 = 0.30;
pub const GEOGRAPHIC_DIVERSITY_WEIGHT: f64 = 0.15;

pub const MIN_REPUTATION_SCORE: f64 = 0.0;
pub const MAX_REPUTATION_SCORE: f64 = 100.0;
pub const GOOD_REPUTATION_THRESHOLD: f64 = 70.0;
pub const EXCELLENT_REPUTATION_THRESHOLD: f64 = 90.0;

pub const UPTIME_MEASUREMENT_WINDOW_SECS: u64 = 86400; // 24 hours
pub const LATENCY_MEASUREMENT_WINDOW_SECS: u64 = 3600; // 1 hour
pub const MAX_ACCEPTABLE_LATENCY_MS: u64 = 1000;

/// Errors during reputation scoring
#[derive(Debug, Error)]
pub enum ReputationError {
    #[error("Relay not found: {0:?}")]
    RelayNotFound(VerifyingKey),

    #[error("Insufficient data for scoring")]
    InsufficientData,

    #[error("Invalid metric value: {0}")]
    InvalidMetric(String),
}

/// Relay performance metrics
#[derive(Debug, Clone)]
pub struct RelayMetrics {
    /// Relay's public key
    pub relay_key: VerifyingKey,

    /// Total uptime in seconds (last 24 hours)
    pub uptime_secs: u64,

    /// Total measurement window (should be 24 hours)
    pub measurement_window_secs: u64,

    /// Average latency in milliseconds
    pub avg_latency_ms: u64,

    /// Successful message deliveries
    pub successful_deliveries: u64,

    /// Total delivery attempts
    pub total_delivery_attempts: u64,

    /// Geographic region score (0.0-1.0)
    /// 1.0 = diverse/underserved region
    /// 0.0 = oversaturated region
    pub geographic_score: f64,

    /// Last update timestamp
    pub last_updated: SystemTime,
}

impl RelayMetrics {
    /// Create new metrics for a relay
    pub fn new(relay_key: VerifyingKey) -> Self {
        Self {
            relay_key,
            uptime_secs: 0,
            measurement_window_secs: UPTIME_MEASUREMENT_WINDOW_SECS,
            avg_latency_ms: 0,
            successful_deliveries: 0,
            total_delivery_attempts: 0,
            geographic_score: 0.5, // Default: neutral
            last_updated: SystemTime::now(),
        }
    }

    /// Calculate uptime percentage
    pub fn uptime_percentage(&self) -> f64 {
        if self.measurement_window_secs == 0 {
            return 0.0;
        }
        (self.uptime_secs as f64 / self.measurement_window_secs as f64) * 100.0
    }

    /// Calculate delivery success rate
    pub fn delivery_success_rate(&self) -> f64 {
        if self.total_delivery_attempts == 0 {
            return 0.0;
        }
        (self.successful_deliveries as f64 / self.total_delivery_attempts as f64) * 100.0
    }

    /// Update latency measurement
    pub fn update_latency(&mut self, new_latency_ms: u64) {
        // Exponential moving average
        if self.avg_latency_ms == 0 {
            self.avg_latency_ms = new_latency_ms;
        } else {
            self.avg_latency_ms =
                ((self.avg_latency_ms as f64 * 0.7) + (new_latency_ms as f64 * 0.3)) as u64;
        }
        self.last_updated = SystemTime::now();
    }

    /// Record a delivery attempt
    pub fn record_delivery(&mut self, success: bool) {
        self.total_delivery_attempts += 1;
        if success {
            self.successful_deliveries += 1;
        }
        self.last_updated = SystemTime::now();
    }
}

/// Relay reputation score
#[derive(Debug, Clone)]
pub struct RelayReputationScore {
    pub relay_key: VerifyingKey,
    pub total_score: f64,
    pub uptime_score: f64,
    pub latency_score: f64,
    pub delivery_score: f64,
    pub geographic_score: f64,
    pub tier: ReputationTier,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReputationTier {
    Excellent, // 90+
    Good,      // 70-89
    Average,   // 50-69
    Poor,      // 30-49
    VeryPoor,  // <30
}

impl ReputationTier {
    pub fn from_score(score: f64) -> Self {
        if score >= EXCELLENT_REPUTATION_THRESHOLD {
            ReputationTier::Excellent
        } else if score >= GOOD_REPUTATION_THRESHOLD {
            ReputationTier::Good
        } else if score >= 50.0 {
            ReputationTier::Average
        } else if score >= 30.0 {
            ReputationTier::Poor
        } else {
            ReputationTier::VeryPoor
        }
    }
}

/// Relay reputation scorer
pub struct RelayReputationScorer {
    /// Metrics per relay
    relay_metrics: Arc<RwLock<HashMap<VerifyingKey, RelayMetrics>>>,

    /// Cached reputation scores
    reputation_scores: Arc<RwLock<HashMap<VerifyingKey, RelayReputationScore>>>,

    /// Last scoring update
    last_update: Arc<RwLock<Instant>>,
}

impl RelayReputationScorer {
    /// Create a new reputation scorer
    pub fn new() -> Self {
        Self {
            relay_metrics: Arc::new(RwLock::new(HashMap::new())),
            reputation_scores: Arc::new(RwLock::new(HashMap::new())),
            last_update: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// Register a new relay
    pub fn register_relay(&self, relay_key: VerifyingKey) {
        let mut metrics = self.relay_metrics.write().unwrap();
        metrics
            .entry(relay_key)
            .or_insert_with(|| RelayMetrics::new(relay_key));
    }

    /// Update relay metrics
    pub fn update_metrics(&self, relay_key: VerifyingKey, metrics: RelayMetrics) {
        let mut all_metrics = self.relay_metrics.write().unwrap();
        all_metrics.insert(relay_key, metrics);
    }

    /// Record a delivery attempt
    pub fn record_delivery(
        &self,
        relay_key: VerifyingKey,
        success: bool,
    ) -> Result<(), ReputationError> {
        let mut metrics = self.relay_metrics.write().unwrap();
        let relay_metrics = metrics
            .get_mut(&relay_key)
            .ok_or(ReputationError::RelayNotFound(relay_key))?;

        relay_metrics.record_delivery(success);
        Ok(())
    }

    /// Update relay latency
    pub fn update_latency(
        &self,
        relay_key: VerifyingKey,
        latency_ms: u64,
    ) -> Result<(), ReputationError> {
        let mut metrics = self.relay_metrics.write().unwrap();
        let relay_metrics = metrics
            .get_mut(&relay_key)
            .ok_or(ReputationError::RelayNotFound(relay_key))?;

        relay_metrics.update_latency(latency_ms);
        Ok(())
    }

    /// Calculate reputation score for a relay
    pub fn calculate_score(
        &self,
        relay_key: VerifyingKey,
    ) -> Result<RelayReputationScore, ReputationError> {
        let metrics = self.relay_metrics.read().unwrap();
        let relay_metrics = metrics
            .get(&relay_key)
            .ok_or(ReputationError::RelayNotFound(relay_key))?;

        // Calculate component scores (0-100 scale)

        // 1. Uptime score (30% weight)
        let uptime_score = relay_metrics.uptime_percentage();

        // 2. Latency score (25% weight) - inverse relationship
        let latency_score = if relay_metrics.avg_latency_ms == 0 {
            100.0
        } else {
            let normalized = (MAX_ACCEPTABLE_LATENCY_MS as f64
                - relay_metrics.avg_latency_ms as f64)
                / MAX_ACCEPTABLE_LATENCY_MS as f64;
            (normalized.max(0.0) * 100.0).min(100.0)
        };

        // 3. Delivery success score (30% weight)
        let delivery_score = relay_metrics.delivery_success_rate();

        // 4. Geographic diversity score (15% weight) - already 0-1, scale to 0-100
        let geographic_score = relay_metrics.geographic_score * 100.0;

        // Calculate weighted total
        let total_score = (uptime_score * UPTIME_WEIGHT)
            + (latency_score * LATENCY_WEIGHT)
            + (delivery_score * DELIVERY_SUCCESS_WEIGHT)
            + (geographic_score * GEOGRAPHIC_DIVERSITY_WEIGHT);

        let total_score = total_score.clamp(MIN_REPUTATION_SCORE, MAX_REPUTATION_SCORE);

        let tier = ReputationTier::from_score(total_score);

        let score = RelayReputationScore {
            relay_key,
            total_score,
            uptime_score,
            latency_score,
            delivery_score,
            geographic_score,
            tier,
            timestamp: SystemTime::now(),
        };

        // Cache the score
        let mut scores = self.reputation_scores.write().unwrap();
        scores.insert(relay_key, score.clone());

        *self.last_update.write().unwrap() = Instant::now();

        Ok(score)
    }

    /// Get cached reputation score
    pub fn get_score(&self, relay_key: &VerifyingKey) -> Option<RelayReputationScore> {
        self.reputation_scores
            .read()
            .unwrap()
            .get(relay_key)
            .cloned()
    }

    /// Get all reputation scores sorted by total score
    pub fn get_all_scores(&self) -> Vec<RelayReputationScore> {
        let mut scores: Vec<_> = self
            .reputation_scores
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect();

        scores.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap());
        scores
    }

    /// Get top N relays by reputation
    pub fn get_top_relays(&self, n: usize) -> Vec<RelayReputationScore> {
        let all_scores = self.get_all_scores();
        all_scores.into_iter().take(n).collect()
    }

    /// Get relays by tier
    pub fn get_relays_by_tier(&self, tier: ReputationTier) -> Vec<RelayReputationScore> {
        self.reputation_scores
            .read()
            .unwrap()
            .values()
            .filter(|s| s.tier == tier)
            .cloned()
            .collect()
    }

    /// Get statistics
    pub fn get_stats(&self) -> ReputationStats {
        let metrics = self.relay_metrics.read().unwrap();
        let scores = self.reputation_scores.read().unwrap();

        let total_relays = metrics.len();
        let scored_relays = scores.len();

        let avg_score = if scores.is_empty() {
            0.0
        } else {
            scores.values().map(|s| s.total_score).sum::<f64>() / scores.len() as f64
        };

        let tier_distribution = [
            scores
                .values()
                .filter(|s| s.tier == ReputationTier::Excellent)
                .count(),
            scores
                .values()
                .filter(|s| s.tier == ReputationTier::Good)
                .count(),
            scores
                .values()
                .filter(|s| s.tier == ReputationTier::Average)
                .count(),
            scores
                .values()
                .filter(|s| s.tier == ReputationTier::Poor)
                .count(),
            scores
                .values()
                .filter(|s| s.tier == ReputationTier::VeryPoor)
                .count(),
        ];

        ReputationStats {
            total_relays,
            scored_relays,
            avg_score,
            tier_distribution,
        }
    }
}

impl Default for RelayReputationScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ReputationStats {
    pub total_relays: usize,
    pub scored_relays: usize,
    pub avg_score: f64,
    pub tier_distribution: [usize; 5], // [Excellent, Good, Average, Poor, VeryPoor]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_relay_metrics_uptime() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let mut metrics = RelayMetrics::new(verifying_key);
        metrics.uptime_secs = 86400; // 100% uptime
        metrics.measurement_window_secs = 86400;

        assert_eq!(metrics.uptime_percentage(), 100.0);
    }

    #[test]
    fn test_delivery_success_rate() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let mut metrics = RelayMetrics::new(verifying_key);

        // 8 successes out of 10 attempts
        for _ in 0..8 {
            metrics.record_delivery(true);
        }
        for _ in 0..2 {
            metrics.record_delivery(false);
        }

        assert_eq!(metrics.delivery_success_rate(), 80.0);
    }

    #[test]
    fn test_latency_update() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let mut metrics = RelayMetrics::new(verifying_key);

        metrics.update_latency(100);
        assert_eq!(metrics.avg_latency_ms, 100);

        metrics.update_latency(200);
        // Exponential moving average: 100*0.7 + 200*0.3 = 130
        assert_eq!(metrics.avg_latency_ms, 130);
    }

    #[test]
    fn test_reputation_tier() {
        assert_eq!(ReputationTier::from_score(95.0), ReputationTier::Excellent);
        assert_eq!(ReputationTier::from_score(80.0), ReputationTier::Good);
        assert_eq!(ReputationTier::from_score(60.0), ReputationTier::Average);
        assert_eq!(ReputationTier::from_score(40.0), ReputationTier::Poor);
        assert_eq!(ReputationTier::from_score(20.0), ReputationTier::VeryPoor);
    }

    #[test]
    fn test_reputation_scorer() {
        let scorer = RelayReputationScorer::new();
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Register relay
        scorer.register_relay(verifying_key);

        // Create excellent metrics
        let mut metrics = RelayMetrics::new(verifying_key);
        metrics.uptime_secs = 86400; // 100% uptime
        metrics.measurement_window_secs = 86400;
        metrics.avg_latency_ms = 50; // Very low latency
        metrics.successful_deliveries = 100;
        metrics.total_delivery_attempts = 100; // 100% success
        metrics.geographic_score = 1.0; // Excellent diversity

        scorer.update_metrics(verifying_key, metrics);

        // Calculate score
        let score = scorer.calculate_score(verifying_key).unwrap();

        // Should be excellent (near 100)
        assert!(score.total_score >= 90.0);
        assert_eq!(score.tier, ReputationTier::Excellent);
    }

    #[test]
    fn test_poor_reputation() {
        let scorer = RelayReputationScorer::new();
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        scorer.register_relay(verifying_key);

        // Create poor metrics
        let mut metrics = RelayMetrics::new(verifying_key);
        metrics.uptime_secs = 21600; // 25% uptime
        metrics.measurement_window_secs = 86400;
        metrics.avg_latency_ms = 3000; // High latency
        metrics.successful_deliveries = 30;
        metrics.total_delivery_attempts = 100; // 30% success
        metrics.geographic_score = 0.2; // Poor diversity

        scorer.update_metrics(verifying_key, metrics);

        let score = scorer.calculate_score(verifying_key).unwrap();

        // Should be poor (below 50)
        assert!(score.total_score < 50.0);
    }

    #[test]
    fn test_top_relays() {
        let scorer = RelayReputationScorer::new();

        // Create 5 relays with different scores
        for i in 0..5 {
            let signing_key = SigningKey::generate(&mut OsRng);
            let verifying_key = signing_key.verifying_key();

            scorer.register_relay(verifying_key);

            let mut metrics = RelayMetrics::new(verifying_key);
            metrics.uptime_secs = 86400;
            metrics.measurement_window_secs = 86400;
            metrics.avg_latency_ms = 50 + (i * 100); // Varying latency
            metrics.successful_deliveries = 100 - (i * 10);
            metrics.total_delivery_attempts = 100;
            metrics.geographic_score = 1.0 - (i as f64 * 0.15);

            scorer.update_metrics(verifying_key, metrics);
            scorer.calculate_score(verifying_key).unwrap();
        }

        let top_3 = scorer.get_top_relays(3);
        assert_eq!(top_3.len(), 3);

        // Scores should be in descending order
        assert!(top_3[0].total_score >= top_3[1].total_score);
        assert!(top_3[1].total_score >= top_3[2].total_score);
    }

    #[test]
    fn test_reputation_stats() {
        let scorer = RelayReputationScorer::new();

        for _ in 0..10 {
            let signing_key = SigningKey::generate(&mut OsRng);
            let verifying_key = signing_key.verifying_key();

            scorer.register_relay(verifying_key);

            let mut metrics = RelayMetrics::new(verifying_key);
            metrics.uptime_secs = 86400;
            metrics.measurement_window_secs = 86400;
            metrics.avg_latency_ms = 100;
            metrics.successful_deliveries = 90;
            metrics.total_delivery_attempts = 100;
            metrics.geographic_score = 0.8;

            scorer.update_metrics(verifying_key, metrics);
            scorer.calculate_score(verifying_key).unwrap();
        }

        let stats = scorer.get_stats();
        assert_eq!(stats.total_relays, 10);
        assert_eq!(stats.scored_relays, 10);
        assert!(stats.avg_score > 70.0); // Should be good
    }
}
