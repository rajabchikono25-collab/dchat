//! Reputation-based rate limiting and Quality of Service
//!
//! Implements Section 15 (Rate Limiting & QoS) from ARCHITECTURE.md
//! - Peer reputation scoring based on behavior
//! - Token bucket algorithm with reputation-based refill
//! - Adaptive traffic control and backpressure
//! - Spam detection and anomaly identification
//! - Priority queues for different message types

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Reputation score for a peer (0-100)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ReputationScore(f64);

impl ReputationScore {
    pub fn new(score: f64) -> Self {
        Self(score.clamp(0.0, 100.0))
    }

    pub fn value(&self) -> f64 {
        self.0
    }

    /// Excellent reputation (90-100)
    pub fn is_excellent(&self) -> bool {
        self.0 >= 90.0
    }

    /// Good reputation (70-89)
    pub fn is_good(&self) -> bool {
        self.0 >= 70.0 && self.0 < 90.0
    }

    /// Average reputation (40-69)
    pub fn is_average(&self) -> bool {
        self.0 >= 40.0 && self.0 < 70.0
    }

    /// Poor reputation (20-39)
    pub fn is_poor(&self) -> bool {
        self.0 >= 20.0 && self.0 < 40.0
    }

    /// Bad reputation (0-19)
    pub fn is_bad(&self) -> bool {
        self.0 < 20.0
    }
}

/// Factors contributing to reputation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationFactors {
    /// Message delivery success rate (0-100)
    pub delivery_rate: f64,
    /// Uptime percentage (0-100)
    pub uptime: f64,
    /// Message quality score (0-100)
    pub message_quality: f64,
    /// Response time score (0-100)
    pub response_time: f64,
    /// Protocol compliance score (0-100)
    pub protocol_compliance: f64,
}

impl ReputationFactors {
    /// Calculate overall reputation from factors
    pub fn calculate_score(&self) -> ReputationScore {
        let weights = [0.25, 0.20, 0.20, 0.15, 0.20]; // Sum = 1.0
        let scores = [
            self.delivery_rate,
            self.uptime,
            self.message_quality,
            self.response_time,
            self.protocol_compliance,
        ];

        let weighted_sum: f64 = weights.iter().zip(scores.iter()).map(|(w, s)| w * s).sum();

        ReputationScore::new(weighted_sum)
    }
}

/// Token bucket for rate limiting
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Maximum tokens the bucket can hold
    capacity: usize,
    /// Current number of tokens
    tokens: f64,
    /// Tokens added per second
    refill_rate: f64,
    /// Last refill timestamp
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(capacity: usize, refill_rate: f64) -> Self {
        Self {
            capacity,
            tokens: capacity as f64,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();

        let new_tokens = elapsed * self.refill_rate;
        self.tokens = (self.tokens + new_tokens).min(self.capacity as f64);
        self.last_refill = now;
    }

    /// Try to consume tokens
    pub fn try_consume(&mut self, amount: usize) -> bool {
        self.refill();

        if self.tokens >= amount as f64 {
            self.tokens -= amount as f64;
            true
        } else {
            false
        }
    }

    /// Get current token count
    pub fn available_tokens(&mut self) -> usize {
        self.refill();
        self.tokens as usize
    }

    /// Adjust refill rate based on reputation
    pub fn adjust_refill_rate(&mut self, reputation: ReputationScore) {
        // Higher reputation = faster refill (up to 2x base rate)
        let multiplier = 1.0 + (reputation.value() / 100.0);
        self.refill_rate *= multiplier;
    }
}

/// Message priority for QoS
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MessagePriority {
    /// Critical system messages
    Critical = 4,
    /// High priority (e.g., direct messages)
    High = 3,
    /// Normal priority (e.g., channel messages)
    Normal = 2,
    /// Low priority (e.g., bulk operations)
    Low = 1,
    /// Background tasks
    Background = 0,
}

/// Rate limiter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Base token bucket capacity
    pub base_capacity: usize,
    /// Base refill rate (tokens/second)
    pub base_refill_rate: f64,
    /// Enable reputation-based adjustments
    pub reputation_based: bool,
    /// Spam detection threshold (messages/second)
    pub spam_threshold: f64,
    /// Anomaly detection window (seconds)
    pub anomaly_window_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            base_capacity: 100,
            base_refill_rate: 10.0, // 10 tokens/second
            reputation_based: true,
            spam_threshold: 50.0,
            anomaly_window_secs: 60,
        }
    }
}

/// Peer rate limiter
pub struct PeerRateLimiter {
    peer_id: String,
    bucket: TokenBucket,
    reputation: ReputationScore,
    reputation_factors: ReputationFactors,
    message_history: Vec<(Instant, MessagePriority)>,
    spam_detected: bool,
}

impl PeerRateLimiter {
    pub fn new(peer_id: String, config: &RateLimitConfig) -> Self {
        let bucket = TokenBucket::new(config.base_capacity, config.base_refill_rate);

        Self {
            peer_id,
            bucket,
            reputation: ReputationScore::new(50.0), // Start at average
            reputation_factors: ReputationFactors {
                delivery_rate: 50.0,
                uptime: 50.0,
                message_quality: 50.0,
                response_time: 50.0,
                protocol_compliance: 50.0,
            },
            message_history: Vec::new(),
            spam_detected: false,
        }
    }

    /// Get the peer ID this rate limiter is tracking
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Attempt to send a message
    pub fn try_send(&mut self, priority: MessagePriority) -> Result<()> {
        // Check spam detection
        if self.spam_detected {
            return Err(Error::rate_limit("Peer is flagged for spam"));
        }

        // Calculate token cost based on priority
        let cost = match priority {
            MessagePriority::Critical => 1, // Critical always costs 1
            MessagePriority::High => 2,
            MessagePriority::Normal => 3,
            MessagePriority::Low => 5,
            MessagePriority::Background => 10,
        };

        // Try to consume tokens
        if !self.bucket.try_consume(cost) {
            return Err(Error::rate_limit("Rate limit exceeded"));
        }

        // Record message
        self.message_history.push((Instant::now(), priority));

        Ok(())
    }

    /// Update reputation based on behavior
    pub fn update_reputation(&mut self, factors: ReputationFactors) {
        self.reputation_factors = factors.clone();
        let new_score = factors.calculate_score();

        // Smooth transition (exponential moving average)
        let alpha = 0.3; // Weight for new score
        let smoothed = self.reputation.value() * (1.0 - alpha) + new_score.value() * alpha;

        self.reputation = ReputationScore::new(smoothed);

        // Adjust bucket refill rate based on reputation
        self.bucket.adjust_refill_rate(self.reputation);
    }

    /// Detect spam patterns
    pub fn detect_spam(&mut self, config: &RateLimitConfig) -> bool {
        let window = Duration::from_secs(config.anomaly_window_secs);
        let now = Instant::now();

        // Remove old entries
        self.message_history
            .retain(|(timestamp, _)| now.duration_since(*timestamp) < window);

        // Calculate message rate
        let message_count = self.message_history.len();
        let rate = message_count as f64 / config.anomaly_window_secs as f64;

        // Flag if rate exceeds threshold
        self.spam_detected = rate > config.spam_threshold;

        self.spam_detected
    }

    /// Get current reputation
    pub fn reputation(&self) -> ReputationScore {
        self.reputation
    }

    /// Check if peer is throttled
    pub fn is_throttled(&self) -> bool {
        self.spam_detected || self.reputation.is_bad()
    }

    /// Reset spam flag (after cooldown)
    pub fn reset_spam_flag(&mut self) {
        self.spam_detected = false;
    }
}

/// Global rate limit manager
pub struct RateLimitManager {
    config: RateLimitConfig,
    peer_limiters: HashMap<String, PeerRateLimiter>,
}

impl RateLimitManager {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            peer_limiters: HashMap::new(),
        }
    }

    /// Get or create rate limiter for peer
    pub fn get_limiter(&mut self, peer_id: &str) -> &mut PeerRateLimiter {
        self.peer_limiters
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerRateLimiter::new(peer_id.to_string(), &self.config))
    }

    /// Try to send message from peer
    pub fn try_send(&mut self, peer_id: &str, priority: MessagePriority) -> Result<()> {
        let limiter = self.get_limiter(peer_id);
        limiter.try_send(priority)
    }

    /// Update peer reputation
    pub fn update_reputation(&mut self, peer_id: &str, factors: ReputationFactors) {
        if let Some(limiter) = self.peer_limiters.get_mut(peer_id) {
            limiter.update_reputation(factors);
        }
    }

    /// Run spam detection for all peers
    pub fn detect_spam_all(&mut self) {
        for limiter in self.peer_limiters.values_mut() {
            limiter.detect_spam(&self.config);
        }
    }

    /// Get reputation for peer
    pub fn get_reputation(&self, peer_id: &str) -> Option<ReputationScore> {
        self.peer_limiters.get(peer_id).map(|l| l.reputation())
    }

    /// Remove inactive peers
    pub fn cleanup_inactive(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.peer_limiters.retain(|_, limiter| {
            limiter
                .message_history
                .last()
                .map(|(timestamp, _)| now.duration_since(*timestamp) < max_age)
                .unwrap_or(false)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reputation_score() {
        let score = ReputationScore::new(85.0);
        assert!(score.is_good());
        assert!(!score.is_excellent());

        let excellent = ReputationScore::new(95.0);
        assert!(excellent.is_excellent());
    }

    #[test]
    fn test_reputation_calculation() {
        let factors = ReputationFactors {
            delivery_rate: 90.0,
            uptime: 80.0,
            message_quality: 85.0,
            response_time: 75.0,
            protocol_compliance: 95.0,
        };

        let score = factors.calculate_score();
        assert!(score.value() > 80.0);
        assert!(score.value() < 90.0);
    }

    #[test]
    fn test_token_bucket() {
        let mut bucket = TokenBucket::new(10, 1.0);

        // Should succeed
        assert!(bucket.try_consume(5));
        assert_eq!(bucket.available_tokens(), 5);

        // Should fail
        assert!(!bucket.try_consume(10));
    }

    #[test]
    fn test_rate_limiting() {
        let config = RateLimitConfig {
            base_capacity: 100,
            base_refill_rate: 0.0, // No refill for predictable test
            ..Default::default()
        };
        let mut limiter = PeerRateLimiter::new("peer1".to_string(), &config);

        // Should succeed initially (cost 3, have 100 tokens)
        assert!(limiter.try_send(MessagePriority::Normal).is_ok());

        // Exhaust tokens: 100 tokens / 3 per message = 33 messages
        // After first message we have 97, need 32 more to exhaust
        for _ in 0..32 {
            let _ = limiter.try_send(MessagePriority::Normal);
        }

        // Now we've sent 33 messages * 3 tokens = 99 tokens used, 1 left
        // Next Normal message costs 3, should fail
        assert!(limiter.try_send(MessagePriority::Normal).is_err());

        // But Critical (cost 1) should succeed
        assert!(limiter.try_send(MessagePriority::Critical).is_ok());
    }

    #[test]
    fn test_priority_costs() {
        let config = RateLimitConfig {
            base_capacity: 10,
            base_refill_rate: 0.0, // No refill for test
            ..Default::default()
        };
        let mut limiter = PeerRateLimiter::new("peer1".to_string(), &config);

        // Critical costs 1
        assert!(limiter.try_send(MessagePriority::Critical).is_ok());
        // High costs 2
        assert!(limiter.try_send(MessagePriority::High).is_ok());
        // Normal costs 3
        assert!(limiter.try_send(MessagePriority::Normal).is_ok());

        // Should have 10 - 1 - 2 - 3 = 4 tokens left
        assert_eq!(limiter.bucket.available_tokens(), 4);
    }

    #[test]
    fn test_spam_detection() {
        let config = RateLimitConfig {
            spam_threshold: 5.0, // 5 messages/second
            anomaly_window_secs: 1,
            ..Default::default()
        };
        let mut limiter = PeerRateLimiter::new("peer1".to_string(), &config);

        // Send 10 messages (exceeds 5/sec threshold)
        for _ in 0..10 {
            let _ = limiter.try_send(MessagePriority::Critical);
        }

        assert!(limiter.detect_spam(&config));
        assert!(limiter.is_throttled());
    }

    #[test]
    fn test_reputation_based_adjustment() {
        let config = RateLimitConfig::default();
        let mut limiter = PeerRateLimiter::new("peer1".to_string(), &config);

        // Initial reputation is 50.0 (average)
        assert!(limiter.reputation().is_average());

        let initial_rate = limiter.bucket.refill_rate;

        // Improve reputation with excellent scores
        let good_factors = ReputationFactors {
            delivery_rate: 95.0,
            uptime: 98.0,
            message_quality: 92.0,
            response_time: 90.0,
            protocol_compliance: 96.0,
        };

        limiter.update_reputation(good_factors);

        // Refill rate should increase with better reputation
        assert!(limiter.bucket.refill_rate > initial_rate);

        // Reputation improves but is smoothed with EMA
        // Initial 50.0, new ~94.5, with alpha=0.3: 50*0.7 + 94.5*0.3 ≈ 63.35
        assert!(limiter.reputation().value() > 50.0);
        assert!(limiter.reputation().is_average() || limiter.reputation().is_good());
    }
}
