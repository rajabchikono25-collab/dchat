// Flood control and rate limiting

use libp2p::PeerId;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Rate limiter using token bucket algorithm
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Maximum messages per window
    max_messages: u32,

    /// Time window duration
    window: Duration,

    /// Current message count in window
    message_count: u32,

    /// Window start time
    window_start: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(max_messages_per_second: u32) -> Self {
        Self {
            max_messages: max_messages_per_second,
            window: Duration::from_secs(1),
            message_count: 0,
            window_start: Instant::now(),
        }
    }

    /// Check if rate limit allows a message
    pub fn check(&mut self) -> bool {
        self.reset_if_needed();

        if self.message_count < self.max_messages {
            self.message_count += 1;
            true
        } else {
            false
        }
    }

    /// Record a message
    pub fn record(&mut self) {
        self.reset_if_needed();
        self.message_count += 1;
    }

    /// Reset if window has elapsed
    pub fn reset_if_needed(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= self.window {
            self.message_count = 0;
            self.window_start = now;
        }
    }

    /// Get current usage
    pub fn current_usage(&mut self) -> (u32, u32) {
        self.reset_if_needed();
        (self.message_count, self.max_messages)
    }

    /// Reset counter
    pub fn reset(&mut self) {
        self.message_count = 0;
        self.window_start = Instant::now();
    }
}

/// Flood control manager
pub struct FloodControl {
    /// Per-peer rate limiters
    peer_limits: HashMap<PeerId, RateLimiter>,

    /// Global rate limiter
    global_rate: RateLimiter,

    /// Per-peer limit
    per_peer_limit: u32,
}

impl FloodControl {
    /// Create a new flood control manager
    pub fn new(per_peer_limit: u32, global_limit: u32) -> Self {
        Self {
            peer_limits: HashMap::new(),
            global_rate: RateLimiter::new(global_limit),
            per_peer_limit,
        }
    }

    /// Check if a message from a peer passes rate limits
    pub fn check_rate_limit(&mut self, peer_id: &PeerId) -> bool {
        // Check global rate limit first
        if !self.global_rate.check() {
            return false;
        }

        // Check per-peer rate limit
        let limiter = self
            .peer_limits
            .entry(*peer_id)
            .or_insert_with(|| RateLimiter::new(self.per_peer_limit));

        limiter.check()
    }

    /// Record a message from a peer
    pub fn record_message(&mut self, peer_id: &PeerId) {
        self.global_rate.record();

        let limiter = self
            .peer_limits
            .entry(*peer_id)
            .or_insert_with(|| RateLimiter::new(self.per_peer_limit));

        limiter.record();
    }

    /// Remove a peer's rate limiter
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peer_limits.remove(peer_id);
    }

    /// Get global rate limit usage
    pub fn global_usage(&mut self) -> (u32, u32) {
        self.global_rate.current_usage()
    }

    /// Get per-peer rate limit usage
    pub fn peer_usage(&mut self, peer_id: &PeerId) -> Option<(u32, u32)> {
        self.peer_limits
            .get_mut(peer_id)
            .map(|limiter| limiter.current_usage())
    }

    /// Reset all rate limiters if needed
    pub fn reset_if_needed(&mut self) {
        self.global_rate.reset_if_needed();
        for limiter in self.peer_limits.values_mut() {
            limiter.reset_if_needed();
        }
    }

    /// Get number of tracked peers
    pub fn peer_count(&self) -> usize {
        self.peer_limits.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_rate_limiter_creation() {
        let limiter = RateLimiter::new(10);
        assert_eq!(limiter.max_messages, 10);
    }

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let mut limiter = RateLimiter::new(5);

        for _ in 0..5 {
            assert!(limiter.check());
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let mut limiter = RateLimiter::new(3);

        // First 3 should succeed
        for _ in 0..3 {
            assert!(limiter.check());
        }

        // 4th should fail
        assert!(!limiter.check());
    }

    #[test]
    fn test_rate_limiter_reset() {
        let mut limiter = RateLimiter::new(2);

        assert!(limiter.check());
        assert!(limiter.check());
        assert!(!limiter.check());

        // Wait for window to elapse
        thread::sleep(Duration::from_millis(1100));

        // Should allow again after reset
        assert!(limiter.check());
    }

    #[test]
    fn test_rate_limiter_current_usage() {
        let mut limiter = RateLimiter::new(10);

        let (used, max) = limiter.current_usage();
        assert_eq!(used, 0);
        assert_eq!(max, 10);

        limiter.check();
        limiter.check();

        let (used, _) = limiter.current_usage();
        assert_eq!(used, 2);
    }

    #[test]
    fn test_flood_control_creation() {
        let fc = FloodControl::new(10, 100);
        assert_eq!(fc.per_peer_limit, 10);
    }

    #[test]
    fn test_flood_control_per_peer_limit() {
        let mut fc = FloodControl::new(3, 1000);
        let peer = PeerId::random();

        // First 3 should succeed
        for _ in 0..3 {
            assert!(fc.check_rate_limit(&peer));
        }

        // 4th should fail
        assert!(!fc.check_rate_limit(&peer));
    }

    #[test]
    fn test_flood_control_global_limit() {
        let mut fc = FloodControl::new(100, 5);

        let peer1 = PeerId::random();
        let peer2 = PeerId::random();

        // Use up global limit with two peers
        assert!(fc.check_rate_limit(&peer1));
        assert!(fc.check_rate_limit(&peer1));
        assert!(fc.check_rate_limit(&peer2));
        assert!(fc.check_rate_limit(&peer2));
        assert!(fc.check_rate_limit(&peer1));

        // Global limit reached
        assert!(!fc.check_rate_limit(&peer1));
        assert!(!fc.check_rate_limit(&peer2));
    }

    #[test]
    fn test_flood_control_remove_peer() {
        let mut fc = FloodControl::new(10, 100);
        let peer = PeerId::random();

        fc.check_rate_limit(&peer);
        assert_eq!(fc.peer_count(), 1);

        fc.remove_peer(&peer);
        assert_eq!(fc.peer_count(), 0);
    }

    #[test]
    fn test_flood_control_peer_usage() {
        let mut fc = FloodControl::new(10, 100);
        let peer = PeerId::random();

        fc.check_rate_limit(&peer);
        fc.check_rate_limit(&peer);

        let usage = fc.peer_usage(&peer);
        assert!(usage.is_some());

        let (used, max) = usage.unwrap();
        assert_eq!(used, 2);
        assert_eq!(max, 10);
    }

    #[test]
    fn test_flood_control_global_usage() {
        let mut fc = FloodControl::new(10, 100);
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();

        fc.check_rate_limit(&peer1);
        fc.check_rate_limit(&peer2);
        fc.check_rate_limit(&peer1);

        let (used, max) = fc.global_usage();
        assert_eq!(used, 3);
        assert_eq!(max, 100);
    }
}
