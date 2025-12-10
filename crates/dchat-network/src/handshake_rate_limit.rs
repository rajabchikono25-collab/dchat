//! Handshake Rate Limiting
//!
//! DoS protection for handshake attempts with per-IP, per-peer, and global limits.
//! 
//! See plan3.md Section 3.5: Rate Limiting

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use libp2p::PeerId;

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct HandshakeRateLimitConfig {
    /// Maximum handshakes per IP within the time window
    pub per_ip_limit: u32,
    /// Maximum handshakes per peer within the time window
    pub per_peer_limit: u32,
    /// Maximum concurrent handshakes globally
    pub max_concurrent: u32,
    /// Time window for rate limiting
    pub window: Duration,
    /// Backoff multiplier for repeated failures
    pub backoff_multiplier: f64,
    /// Maximum backoff duration
    pub max_backoff: Duration,
}

impl Default for HandshakeRateLimitConfig {
    fn default() -> Self {
        Self {
            per_ip_limit: 10,
            per_peer_limit: 5,
            max_concurrent: 100,
            window: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            max_backoff: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Rate limit state for a single entity (IP or peer)
#[derive(Debug, Clone)]
struct RateLimitState {
    /// Number of attempts in current window
    count: u32,
    /// Number of failures (for backoff)
    failure_count: u32,
    /// Last attempt timestamp
    last_attempt: Instant,
    /// Window start time
    window_start: Instant,
}

impl Default for RateLimitState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            count: 0,
            failure_count: 0,
            last_attempt: now,
            window_start: now,
        }
    }
}

impl RateLimitState {
    /// Clean up old entries and reset window if needed
    fn cleanup(&mut self, now: Instant, window: Duration) {
        if now.duration_since(self.window_start) > window {
            self.count = 0;
            self.window_start = now;
        }
    }
    
    /// Calculate backoff duration based on failure count
    fn backoff_duration(&self, config: &HandshakeRateLimitConfig) -> Duration {
        if self.failure_count == 0 {
            return Duration::ZERO;
        }
        
        let base = Duration::from_secs(1);
        let multiplier = config.backoff_multiplier.powi(self.failure_count as i32);
        let backoff = Duration::from_secs_f64(base.as_secs_f64() * multiplier);
        
        std::cmp::min(backoff, config.max_backoff)
    }
}

/// Result of a rate limit check
#[derive(Debug)]
pub enum RateLimitResult {
    /// Request is allowed
    Allowed {
        /// Token to release when handshake completes
        token: RateLimitToken,
    },
    /// Request is rejected
    Rejected {
        /// Reason for rejection
        reason: RateLimitReason,
        /// Suggested retry time in milliseconds
        retry_after_ms: u64,
    },
}

/// Token returned when a handshake is allowed
/// Must be released when the handshake completes
#[derive(Debug)]
pub struct RateLimitToken {
    ip: IpAddr,
    peer_id: Option<PeerId>,
    started_at: Instant,
}

impl RateLimitToken {
    fn new(ip: IpAddr, peer_id: Option<PeerId>) -> Self {
        Self {
            ip,
            peer_id,
            started_at: Instant::now(),
        }
    }
    
    /// Get the duration since the handshake started
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
    
    /// Get the start time of the handshake
    pub fn started_at(&self) -> Instant {
        self.started_at
    }
    
    /// Check if the handshake has exceeded a timeout
    pub fn is_timed_out(&self, timeout: Duration) -> bool {
        self.started_at.elapsed() > timeout
    }
    
    /// Get the IP address associated with this token
    pub fn ip(&self) -> IpAddr {
        self.ip
    }
    
    /// Get the peer ID if known
    pub fn peer_id(&self) -> Option<PeerId> {
        self.peer_id
    }
}

/// Reason for rate limit rejection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitReason {
    /// Too many handshakes from this IP
    IpLimit,
    /// Too many handshakes from this peer
    PeerLimit,
    /// Too many concurrent handshakes globally
    GlobalLimit,
    /// Backoff due to previous failures
    Backoff,
}

/// Handshake rate limiter
pub struct HandshakeRateLimiter {
    config: HandshakeRateLimitConfig,
    ip_limits: HashMap<IpAddr, RateLimitState>,
    peer_limits: HashMap<PeerId, RateLimitState>,
    concurrent: AtomicU32,
}

impl HandshakeRateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: HandshakeRateLimitConfig) -> Self {
        Self {
            config,
            ip_limits: HashMap::new(),
            peer_limits: HashMap::new(),
            concurrent: AtomicU32::new(0),
        }
    }
    
    /// Check if a handshake from the given IP and optional peer is allowed
    pub fn check(&mut self, ip: IpAddr, peer_id: Option<&PeerId>) -> RateLimitResult {
        let now = Instant::now();
        
        // Check global concurrent limit
        let current_concurrent = self.concurrent.load(Ordering::Relaxed);
        if current_concurrent >= self.config.max_concurrent {
            return RateLimitResult::Rejected {
                reason: RateLimitReason::GlobalLimit,
                retry_after_ms: 1000, // Suggest retry in 1 second
            };
        }
        
        // Check IP limit
        let ip_state = self.ip_limits.entry(ip).or_default();
        ip_state.cleanup(now, self.config.window);
        
        // Check for backoff
        let backoff = ip_state.backoff_duration(&self.config);
        if backoff > Duration::ZERO {
            let since_last = now.duration_since(ip_state.last_attempt);
            if since_last < backoff {
                return RateLimitResult::Rejected {
                    reason: RateLimitReason::Backoff,
                    retry_after_ms: (backoff - since_last).as_millis() as u64,
                };
            }
        }
        
        if ip_state.count >= self.config.per_ip_limit {
            let time_until_reset = self.config.window
                .saturating_sub(now.duration_since(ip_state.window_start));
            return RateLimitResult::Rejected {
                reason: RateLimitReason::IpLimit,
                retry_after_ms: time_until_reset.as_millis() as u64,
            };
        }
        
        // Check peer limit if peer ID is known
        if let Some(peer) = peer_id {
            let peer_state = self.peer_limits.entry(*peer).or_default();
            peer_state.cleanup(now, self.config.window);
            
            if peer_state.count >= self.config.per_peer_limit {
                let time_until_reset = self.config.window
                    .saturating_sub(now.duration_since(peer_state.window_start));
                return RateLimitResult::Rejected {
                    reason: RateLimitReason::PeerLimit,
                    retry_after_ms: time_until_reset.as_millis() as u64,
                };
            }
            
            // Increment peer counter
            peer_state.count += 1;
            peer_state.last_attempt = now;
        }
        
        // Increment IP counter
        ip_state.count += 1;
        ip_state.last_attempt = now;
        
        // Increment global concurrent counter
        self.concurrent.fetch_add(1, Ordering::Relaxed);
        
        RateLimitResult::Allowed {
            token: RateLimitToken::new(ip, peer_id.copied()),
        }
    }
    
    /// Release a rate limit token after handshake completion
    pub fn release(&mut self, token: RateLimitToken, success: bool) {
        // Log handshake duration for metrics
        let handshake_duration = token.elapsed();
        tracing::debug!(
            "Handshake completed: ip={}, peer={:?}, success={}, duration={:?}",
            token.ip,
            token.peer_id,
            success,
            handshake_duration
        );
        
        // Decrement global concurrent counter
        self.concurrent.fetch_sub(1, Ordering::Relaxed);
        
        // Update failure count for backoff
        if !success {
            if let Some(state) = self.ip_limits.get_mut(&token.ip) {
                state.failure_count += 1;
            }
            if let Some(peer) = token.peer_id {
                if let Some(state) = self.peer_limits.get_mut(&peer) {
                    state.failure_count += 1;
                }
            }
        } else {
            // Reset failure count on success
            if let Some(state) = self.ip_limits.get_mut(&token.ip) {
                state.failure_count = 0;
            }
            if let Some(peer) = token.peer_id {
                if let Some(state) = self.peer_limits.get_mut(&peer) {
                    state.failure_count = 0;
                }
            }
        }
    }
    
    /// Release a rate limit token and return the handshake duration
    pub fn release_with_duration(&mut self, token: RateLimitToken, success: bool) -> Duration {
        let duration = token.elapsed();
        self.release(token, success);
        duration
    }
    
    /// Clean up old entries from the rate limiter
    pub fn cleanup(&mut self) {
        let now = Instant::now();
        let window = self.config.window;
        
        // Remove entries older than 2x the window
        let expiry = window * 2;
        
        self.ip_limits.retain(|_, state| {
            now.duration_since(state.last_attempt) < expiry
        });
        
        self.peer_limits.retain(|_, state| {
            now.duration_since(state.last_attempt) < expiry
        });
    }
    
    /// Get current statistics
    pub fn stats(&self) -> RateLimiterStats {
        RateLimiterStats {
            tracked_ips: self.ip_limits.len(),
            tracked_peers: self.peer_limits.len(),
            concurrent_handshakes: self.concurrent.load(Ordering::Relaxed) as usize,
        }
    }
}

/// Rate limiter statistics
#[derive(Debug, Clone)]
pub struct RateLimiterStats {
    /// Number of IPs being tracked
    pub tracked_ips: usize,
    /// Number of peers being tracked
    pub tracked_peers: usize,
    /// Current concurrent handshakes
    pub concurrent_handshakes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    
    #[test]
    fn test_rate_limiter_allows_initial_request() {
        let mut limiter = HandshakeRateLimiter::new(HandshakeRateLimitConfig::default());
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        
        match limiter.check(ip, None) {
            RateLimitResult::Allowed { token } => {
                limiter.release(token, true);
            }
            RateLimitResult::Rejected { .. } => panic!("Should allow first request"),
        }
    }
    
    #[test]
    fn test_rate_limiter_rejects_over_limit() {
        let config = HandshakeRateLimitConfig {
            per_ip_limit: 2,
            ..Default::default()
        };
        let mut limiter = HandshakeRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        
        // First two should succeed
        for _ in 0..2 {
            match limiter.check(ip, None) {
                RateLimitResult::Allowed { token } => {
                    limiter.release(token, true);
                }
                RateLimitResult::Rejected { .. } => panic!("Should allow request"),
            }
        }
        
        // Third should fail
        match limiter.check(ip, None) {
            RateLimitResult::Allowed { .. } => panic!("Should reject over limit"),
            RateLimitResult::Rejected { reason, .. } => {
                assert_eq!(reason, RateLimitReason::IpLimit);
            }
        }
    }
    
    #[test]
    fn test_rate_limiter_global_limit() {
        let config = HandshakeRateLimitConfig {
            max_concurrent: 1,
            ..Default::default()
        };
        let mut limiter = HandshakeRateLimiter::new(config);
        
        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));
        
        // First should succeed
        let token = match limiter.check(ip1, None) {
            RateLimitResult::Allowed { token } => token,
            RateLimitResult::Rejected { .. } => panic!("Should allow first"),
        };
        
        // Second should fail (different IP but global limit reached)
        match limiter.check(ip2, None) {
            RateLimitResult::Allowed { .. } => panic!("Should reject due to global limit"),
            RateLimitResult::Rejected { reason, .. } => {
                assert_eq!(reason, RateLimitReason::GlobalLimit);
            }
        }
        
        // Release first, second should now work
        limiter.release(token, true);
        
        match limiter.check(ip2, None) {
            RateLimitResult::Allowed { token } => {
                limiter.release(token, true);
            }
            RateLimitResult::Rejected { .. } => panic!("Should allow after release"),
        }
    }
}
