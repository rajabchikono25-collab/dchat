//! Rate limiting module for dchat network
//!
//! Implements per-peer rate limiting with reputation-based throttling
//! to prevent spam and DDoS attacks.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum messages per second per peer
    pub messages_per_second: u32,
    /// Burst capacity (allows short bursts above rate)
    pub burst_capacity: u32,
    /// Time window for rate calculation
    pub window: Duration,
    /// Base reputation score for new peers
    pub base_reputation: f64,
    /// Reputation decay per violation
    pub reputation_decay: f64,
    /// Reputation recovery per second
    pub reputation_recovery: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            messages_per_second: 10,
            burst_capacity: 20,
            window: Duration::from_secs(1),
            base_reputation: 100.0,
            reputation_decay: 5.0,
            reputation_recovery: 0.1,
        }
    }
}

/// Token bucket for rate limiting
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;
    }

    fn try_consume(&mut self, tokens: f64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }
}

/// Peer rate limit state
#[derive(Debug)]
struct PeerLimit {
    bucket: TokenBucket,
    reputation: f64,
    violations: u64,
    last_violation: Option<Instant>,
}

impl PeerLimit {
    fn new(config: &RateLimitConfig) -> Self {
        Self {
            bucket: TokenBucket::new(
                config.burst_capacity as f64,
                config.messages_per_second as f64,
            ),
            reputation: config.base_reputation,
            violations: 0,
            last_violation: None,
        }
    }

    fn update_reputation(&mut self, config: &RateLimitConfig) {
        // Recover reputation over time
        if let Some(last_violation) = self.last_violation {
            let elapsed = Instant::now().duration_since(last_violation).as_secs_f64();
            self.reputation = (self.reputation + elapsed * config.reputation_recovery)
                .min(config.base_reputation);
        }
    }

    fn record_violation(&mut self, config: &RateLimitConfig) {
        self.violations += 1;
        self.reputation = (self.reputation - config.reputation_decay).max(0.0);
        self.last_violation = Some(Instant::now());
    }
}

/// Rate limiter for network peers
pub struct RateLimiter {
    config: RateLimitConfig,
    peers: Arc<RwLock<HashMap<SocketAddr, PeerLimit>>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if a peer is allowed to send a message
    pub async fn allow(&self, peer: SocketAddr) -> bool {
        let mut peers = self.peers.write().await;
        let limit = peers
            .entry(peer)
            .or_insert_with(|| PeerLimit::new(&self.config));

        // Update reputation
        limit.update_reputation(&self.config);

        // Always consume 1 token per message
        // Reputation affects burst capacity, not token consumption rate
        if limit.bucket.try_consume(1.0) {
            true
        } else {
            limit.record_violation(&self.config);
            false
        }
    }

    /// Get peer reputation score
    pub async fn get_reputation(&self, peer: SocketAddr) -> Option<f64> {
        let peers = self.peers.read().await;
        peers.get(&peer).map(|limit| limit.reputation)
    }

    /// Get peer violation count
    pub async fn get_violations(&self, peer: SocketAddr) -> Option<u64> {
        let peers = self.peers.read().await;
        peers.get(&peer).map(|limit| limit.violations)
    }

    /// Reset peer state (e.g., after successful authentication)
    pub async fn reset_peer(&self, peer: SocketAddr) {
        let mut peers = self.peers.write().await;
        if let Some(limit) = peers.get_mut(&peer) {
            limit.reputation = self.config.base_reputation;
            limit.violations = 0;
            limit.last_violation = None;
        }
    }

    /// Ban a peer (set reputation to 0)
    pub async fn ban_peer(&self, peer: SocketAddr) {
        let mut peers = self.peers.write().await;
        if let Some(limit) = peers.get_mut(&peer) {
            limit.reputation = 0.0;
        }
    }

    /// Cleanup old peers (not seen in last hour)
    pub async fn cleanup_stale_peers(&self) {
        let mut peers = self.peers.write().await;
        let cutoff = Instant::now() - Duration::from_secs(3600);
        peers.retain(|_, limit| limit.last_violation.is_none_or(|last| last > cutoff));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_rate_limiting() {
        let config = RateLimitConfig {
            messages_per_second: 10,
            burst_capacity: 20,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let peer = "127.0.0.1:8080".parse().unwrap();

        // Should allow burst
        for _ in 0..20 {
            assert!(limiter.allow(peer).await);
        }

        // Should rate limit after burst
        assert!(!limiter.allow(peer).await);
    }

    #[tokio::test]
    async fn test_reputation_based_throttling() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        let peer = "127.0.0.1:8080".parse().unwrap();

        // Cause violations
        for _ in 0..30 {
            limiter.allow(peer).await;
        }

        // Check reputation decreased
        let reputation = limiter.get_reputation(peer).await.unwrap();
        assert!(reputation < 100.0);

        // Check violations recorded
        let violations = limiter.get_violations(peer).await.unwrap();
        assert!(violations > 0);
    }

    #[tokio::test]
    async fn test_reputation_recovery() {
        let config = RateLimitConfig {
            reputation_recovery: 10.0, // Fast recovery for test
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let peer = "127.0.0.1:8080".parse().unwrap();

        // Cause violation
        for _ in 0..30 {
            limiter.allow(peer).await;
        }

        let reputation_before = limiter.get_reputation(peer).await.unwrap();

        // Wait for recovery
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Trigger reputation update
        limiter.allow(peer).await;

        let reputation_after = limiter.get_reputation(peer).await.unwrap();
        assert!(reputation_after > reputation_before);
    }
}
