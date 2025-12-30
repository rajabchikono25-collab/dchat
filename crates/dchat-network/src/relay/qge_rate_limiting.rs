//! QGE Rate Limiting
//!
//! This module implements sophisticated rate limiting for Quorum-Gated
//! Encryption requests. It protects relays from abuse while ensuring
//! legitimate users maintain good service.
//!
//! # Rate Limit Types
//!
//! - **Token Bucket**: Classic algorithm for burst + sustained rate
//! - **Sliding Window**: Precise counting over rolling time window
//! - **Reputation-Based**: Dynamic limits based on user reputation
//! - **Adaptive**: Adjusts limits based on system load
//!
//! # Limit Categories
//!
//! - **Token Requests**: Epoch token issuance (most resource-intensive)
//! - **Message Relay**: Message routing through network
//! - **Revocation Checks**: Verification of revocation status
//! - **Key Distribution**: Group key distribution requests
//!
//! # Fairness Properties
//!
//! - New users get baseline allocation
//! - Good behavior increases limits over time
//! - Abuse temporarily reduces limits
//! - Emergency override for critical messages

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Default token bucket capacity
pub const DEFAULT_BUCKET_CAPACITY: u64 = 100;

/// Default token refill rate (tokens per second)
pub const DEFAULT_REFILL_RATE: f64 = 10.0;

/// Default sliding window duration (seconds)
pub const DEFAULT_WINDOW_SECS: u64 = 60;

/// Minimum requests per window
pub const MIN_REQUESTS_PER_WINDOW: u64 = 5;

/// Maximum requests per window (for high-reputation users)
pub const MAX_REQUESTS_PER_WINDOW: u64 = 1000;

/// High priority request bypass threshold
pub const EMERGENCY_BYPASS_THRESHOLD: u64 = 3;

/// Rate limit entry expiry (no activity)
pub const ENTRY_EXPIRY_SECS: u64 = 3600; // 1 hour

/// Request category for rate limiting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RequestCategory {
    /// Epoch token request
    TokenRequest,
    /// Message relay
    MessageRelay,
    /// Revocation check
    RevocationCheck,
    /// Key distribution
    KeyDistribution,
    /// Handshake
    Handshake,
    /// Recovery request
    Recovery,
    /// Admin action
    AdminAction,
}

impl RequestCategory {
    /// Get default limit for category
    pub fn default_limit(&self) -> u64 {
        match self {
            RequestCategory::TokenRequest => 10,    // Expensive
            RequestCategory::MessageRelay => 200,   // High volume
            RequestCategory::RevocationCheck => 50, // Moderate
            RequestCategory::KeyDistribution => 20, // Moderate
            RequestCategory::Handshake => 5,        // Low
            RequestCategory::Recovery => 3,         // Very low
            RequestCategory::AdminAction => 10,     // Moderate
        }
    }

    /// Get cost multiplier (for mixed limiting)
    pub fn cost(&self) -> u64 {
        match self {
            RequestCategory::TokenRequest => 10,
            RequestCategory::MessageRelay => 1,
            RequestCategory::RevocationCheck => 2,
            RequestCategory::KeyDistribution => 5,
            RequestCategory::Handshake => 20,
            RequestCategory::Recovery => 50,
            RequestCategory::AdminAction => 5,
        }
    }
}

/// Rate limit decision
#[derive(Debug, Clone)]
pub enum RateLimitDecision {
    /// Request allowed
    Allowed { remaining: u64, reset_at: u64 },
    /// Request throttled (temporary)
    Throttled { retry_after_ms: u64, reason: String },
    /// Request blocked (abuse detected)
    Blocked { until: u64, reason: String },
    /// Emergency bypass granted
    EmergencyBypass { remaining_bypasses: u64 },
}

impl RateLimitDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(
            self,
            RateLimitDecision::Allowed { .. } | RateLimitDecision::EmergencyBypass { .. }
        )
    }
}

/// Token bucket state
#[derive(Debug, Clone)]
struct TokenBucket {
    /// Current tokens
    tokens: f64,
    /// Maximum capacity
    capacity: f64,
    /// Refill rate (tokens per second)
    refill_rate: f64,
    /// Last refill time
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: u64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity as f64,
            capacity: capacity as f64,
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

    fn try_consume(&mut self, cost: f64) -> bool {
        self.refill();
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    fn tokens_available(&self) -> u64 {
        self.tokens as u64
    }

    fn time_until_available(&self, cost: f64) -> Duration {
        if self.tokens >= cost {
            Duration::ZERO
        } else if self.refill_rate <= 0.0 {
            // If no refill, wait indefinitely (use max reasonable duration)
            Duration::from_secs(3600) // 1 hour
        } else {
            let needed = cost - self.tokens;
            Duration::from_secs_f64(needed / self.refill_rate)
        }
    }
}

/// Sliding window counter
#[derive(Debug, Clone)]
struct SlidingWindow {
    /// Window duration
    window_secs: u64,
    /// Request timestamps
    requests: Vec<u64>,
    /// Maximum requests in window
    max_requests: u64,
}

impl SlidingWindow {
    fn new(window_secs: u64, max_requests: u64) -> Self {
        Self {
            window_secs,
            requests: Vec::new(),
            max_requests,
        }
    }

    fn cleanup(&mut self) {
        let cutoff = current_timestamp().saturating_sub(self.window_secs);
        self.requests.retain(|&ts| ts > cutoff);
    }

    fn try_request(&mut self) -> bool {
        self.cleanup();
        if self.requests.len() < self.max_requests as usize {
            self.requests.push(current_timestamp());
            true
        } else {
            false
        }
    }

    fn remaining(&self) -> u64 {
        let cutoff = current_timestamp().saturating_sub(self.window_secs);
        let active = self.requests.iter().filter(|&&ts| ts > cutoff).count() as u64;
        self.max_requests.saturating_sub(active)
    }

    fn reset_at(&self) -> u64 {
        self.requests
            .first()
            .map(|&first| first + self.window_secs)
            .unwrap_or_else(current_timestamp)
    }
}

/// Per-user rate limit state
struct UserRateLimitState {
    /// User/device ID
    user_id: [u8; 32],
    /// Token buckets per category
    buckets: HashMap<RequestCategory, TokenBucket>,
    /// Sliding windows per category
    windows: HashMap<RequestCategory, SlidingWindow>,
    /// Emergency bypass tokens remaining
    emergency_bypasses: u64,
    /// Reputation score (0-100)
    reputation: u64,
    /// Block until (if blocked)
    blocked_until: Option<u64>,
    /// Block reason
    block_reason: Option<String>,
    /// Abuse score (accumulated)
    abuse_score: u64,
    /// Last activity
    last_activity: u64,
}

impl UserRateLimitState {
    fn new(user_id: [u8; 32]) -> Self {
        Self {
            user_id,
            buckets: HashMap::new(),
            windows: HashMap::new(),
            emergency_bypasses: EMERGENCY_BYPASS_THRESHOLD,
            reputation: 50, // Start at neutral
            blocked_until: None,
            block_reason: None,
            abuse_score: 0,
            last_activity: current_timestamp(),
        }
    }

    fn get_or_create_bucket(
        &mut self,
        category: RequestCategory,
        config: &RateLimitConfig,
    ) -> &mut TokenBucket {
        self.buckets.entry(category).or_insert_with(|| {
            let base_limit = category.default_limit();
            let reputation_mult = 1.0 + (self.reputation as f64 - 50.0) / 100.0;
            let capacity = ((base_limit as f64 * reputation_mult) as u64)
                .max(MIN_REQUESTS_PER_WINDOW)
                .min(MAX_REQUESTS_PER_WINDOW);
            TokenBucket::new(capacity, config.refill_rate)
        })
    }

    fn get_or_create_window(
        &mut self,
        category: RequestCategory,
        config: &RateLimitConfig,
    ) -> &mut SlidingWindow {
        self.windows.entry(category).or_insert_with(|| {
            let base_limit = category.default_limit();
            let reputation_mult = 1.0 + (self.reputation as f64 - 50.0) / 100.0;
            let max_requests = ((base_limit as f64 * reputation_mult) as u64)
                .max(MIN_REQUESTS_PER_WINDOW)
                .min(MAX_REQUESTS_PER_WINDOW);
            SlidingWindow::new(config.window_secs, max_requests)
        })
    }

    fn is_blocked(&self) -> bool {
        self.blocked_until
            .map(|until| current_timestamp() < until)
            .unwrap_or(false)
    }

    fn update_reputation(&mut self, delta: i64) {
        self.reputation = (self.reputation as i64 + delta).clamp(0, 100) as u64;
    }

    fn record_abuse(&mut self, severity: u64) {
        self.abuse_score += severity;

        // Block if abuse score is too high
        if self.abuse_score >= 100 {
            let block_duration = (self.abuse_score / 100) * 3600; // 1 hour per 100 abuse points
            self.blocked_until = Some(current_timestamp() + block_duration);
            self.block_reason = Some("Abuse threshold exceeded".to_string());
        }
    }
}

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Token bucket refill rate
    pub refill_rate: f64,
    /// Sliding window duration
    pub window_secs: u64,
    /// Enable adaptive limiting
    pub adaptive: bool,
    /// Current system load (0.0 - 1.0)
    pub system_load: f64,
    /// Enable reputation-based limits
    pub reputation_based: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            refill_rate: DEFAULT_REFILL_RATE,
            window_secs: DEFAULT_WINDOW_SECS,
            adaptive: true,
            system_load: 0.0,
            reputation_based: true,
        }
    }
}

/// QGE Rate Limiter
pub struct QgeRateLimiter {
    /// Configuration
    config: Arc<RwLock<RateLimitConfig>>,
    /// Per-user state
    users: Arc<RwLock<HashMap<[u8; 32], UserRateLimitState>>>,
    /// Global rate bucket (for DDoS protection)
    global_bucket: Arc<RwLock<TokenBucket>>,
    /// Statistics
    stats: Arc<RwLock<RateLimitStats>>,
}

impl QgeRateLimiter {
    /// Create a new rate limiter
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(RateLimitConfig::default())),
            users: Arc::new(RwLock::new(HashMap::new())),
            global_bucket: Arc::new(RwLock::new(TokenBucket::new(10000, 1000.0))),
            stats: Arc::new(RwLock::new(RateLimitStats::default())),
        }
    }

    /// Create with custom config
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            users: Arc::new(RwLock::new(HashMap::new())),
            global_bucket: Arc::new(RwLock::new(TokenBucket::new(10000, 1000.0))),
            stats: Arc::new(RwLock::new(RateLimitStats::default())),
        }
    }

    /// Check rate limit for a request
    pub async fn check(
        &self,
        user_id: [u8; 32],
        category: RequestCategory,
        is_emergency: bool,
    ) -> RateLimitDecision {
        let config = self.config.read().await;

        // Check global limit first
        {
            let mut global = self.global_bucket.write().await;
            if !global.try_consume(1.0) {
                let mut stats = self.stats.write().await;
                stats.global_throttles += 1;
                return RateLimitDecision::Throttled {
                    retry_after_ms: global.time_until_available(1.0).as_millis() as u64,
                    reason: "Global rate limit exceeded".to_string(),
                };
            }
        }

        let mut users = self.users.write().await;
        let state = users
            .entry(user_id)
            .or_insert_with(|| UserRateLimitState::new(user_id));

        state.last_activity = current_timestamp();

        // Check if blocked
        if state.is_blocked() {
            let mut stats = self.stats.write().await;
            stats.blocked_requests += 1;
            return RateLimitDecision::Blocked {
                until: state.blocked_until.unwrap(),
                reason: state.block_reason.clone().unwrap_or_default(),
            };
        }

        // Emergency bypass
        if is_emergency && state.emergency_bypasses > 0 {
            state.emergency_bypasses -= 1;
            let mut stats = self.stats.write().await;
            stats.emergency_bypasses += 1;
            return RateLimitDecision::EmergencyBypass {
                remaining_bypasses: state.emergency_bypasses,
            };
        }

        // Adaptive limiting based on system load
        let cost = if config.adaptive && config.system_load > 0.8 {
            category.cost() as f64 * (1.0 + config.system_load)
        } else {
            category.cost() as f64
        };

        // Check token bucket
        let bucket_result = {
            let bucket = state.get_or_create_bucket(category, &config);
            if bucket.try_consume(cost) {
                Ok(bucket.tokens_available())
            } else {
                Err(bucket.time_until_available(cost).as_millis() as u64)
            }
        };

        if let Err(retry_after_ms) = bucket_result {
            let mut stats = self.stats.write().await;
            stats.throttled_requests += 1;
            stats
                .throttles_by_category
                .entry(category)
                .and_modify(|c| *c += 1)
                .or_insert(1);

            // Record slight abuse for hitting limit
            state.record_abuse(1);

            return RateLimitDecision::Throttled {
                retry_after_ms,
                reason: format!("Rate limit exceeded for {:?}", category),
            };
        }

        let remaining = bucket_result.unwrap();

        // Check sliding window
        let window_result = {
            let window = state.get_or_create_window(category, &config);
            if window.try_request() {
                Ok(window.reset_at())
            } else {
                Err(())
            }
        };

        if window_result.is_err() {
            let mut stats = self.stats.write().await;
            stats.throttled_requests += 1;

            return RateLimitDecision::Throttled {
                retry_after_ms: 1000, // 1 second
                reason: format!("Window limit exceeded for {:?}", category),
            };
        }

        let reset_at = window_result.unwrap();

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.allowed_requests += 1;
            stats
                .requests_by_category
                .entry(category)
                .and_modify(|c| *c += 1)
                .or_insert(1);
        }

        RateLimitDecision::Allowed {
            remaining,
            reset_at,
        }
    }

    /// Update user reputation
    pub async fn update_reputation(&self, user_id: [u8; 32], delta: i64) {
        let mut users = self.users.write().await;
        if let Some(state) = users.get_mut(&user_id) {
            state.update_reputation(delta);
        }
    }

    /// Report abuse
    pub async fn report_abuse(&self, user_id: [u8; 32], severity: u64) {
        let mut users = self.users.write().await;
        if let Some(state) = users.get_mut(&user_id) {
            state.record_abuse(severity);
        }
    }

    /// Unblock a user
    pub async fn unblock(&self, user_id: [u8; 32]) {
        let mut users = self.users.write().await;
        if let Some(state) = users.get_mut(&user_id) {
            state.blocked_until = None;
            state.block_reason = None;
            state.abuse_score = 0;
        }
    }

    /// Update system load
    pub async fn update_system_load(&self, load: f64) {
        let mut config = self.config.write().await;
        config.system_load = load.clamp(0.0, 1.0);
    }

    /// Get user stats
    pub async fn get_user_stats(&self, user_id: [u8; 32]) -> Option<UserRateLimitInfo> {
        let users = self.users.read().await;
        users.get(&user_id).map(|state| UserRateLimitInfo {
            user_id,
            reputation: state.reputation,
            emergency_bypasses: state.emergency_bypasses,
            is_blocked: state.is_blocked(),
            blocked_until: state.blocked_until,
            abuse_score: state.abuse_score,
        })
    }

    /// Get global stats
    pub async fn get_stats(&self) -> RateLimitStats {
        self.stats.read().await.clone()
    }

    /// Cleanup expired entries
    pub async fn cleanup(&self) {
        let cutoff = current_timestamp().saturating_sub(ENTRY_EXPIRY_SECS);
        let mut users = self.users.write().await;
        users.retain(|_, state| state.last_activity > cutoff);
    }

    /// Start cleanup task
    pub fn start_cleanup_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await;
                self.cleanup().await;
            }
        })
    }
}

impl Default for QgeRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// User rate limit info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRateLimitInfo {
    pub user_id: [u8; 32],
    pub reputation: u64,
    pub emergency_bypasses: u64,
    pub is_blocked: bool,
    pub blocked_until: Option<u64>,
    pub abuse_score: u64,
}

/// Rate limit statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimitStats {
    pub allowed_requests: u64,
    pub throttled_requests: u64,
    pub blocked_requests: u64,
    pub emergency_bypasses: u64,
    pub global_throttles: u64,
    pub requests_by_category: HashMap<RequestCategory, u64>,
    pub throttles_by_category: HashMap<RequestCategory, u64>,
}

/// IP-based rate limiter (for unauthenticated requests)
pub struct IpRateLimiter {
    /// Per-IP state
    ips: Arc<RwLock<HashMap<std::net::IpAddr, IpRateLimitState>>>,
    /// Config
    config: IpRateLimitConfig,
}

/// IP rate limit config
#[derive(Debug, Clone)]
pub struct IpRateLimitConfig {
    /// Max requests per minute
    pub requests_per_minute: u64,
    /// Max connections per IP
    pub max_connections: u64,
    /// Block duration for abuse
    pub block_duration_secs: u64,
}

impl Default for IpRateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            max_connections: 10,
            block_duration_secs: 3600,
        }
    }
}

struct IpRateLimitState {
    window: SlidingWindow,
    connections: u64,
    blocked_until: Option<u64>,
    last_seen: u64,
}

impl IpRateLimiter {
    pub fn new(config: IpRateLimitConfig) -> Self {
        Self {
            ips: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn check(&self, ip: std::net::IpAddr) -> RateLimitDecision {
        let mut ips = self.ips.write().await;
        let state = ips.entry(ip).or_insert_with(|| IpRateLimitState {
            window: SlidingWindow::new(60, self.config.requests_per_minute),
            connections: 0,
            blocked_until: None,
            last_seen: current_timestamp(),
        });

        state.last_seen = current_timestamp();

        // Check block
        if let Some(until) = state.blocked_until {
            if current_timestamp() < until {
                return RateLimitDecision::Blocked {
                    until,
                    reason: "IP blocked for abuse".to_string(),
                };
            }
            state.blocked_until = None;
        }

        // Check rate
        if !state.window.try_request() {
            return RateLimitDecision::Throttled {
                retry_after_ms: 1000,
                reason: "IP rate limit exceeded".to_string(),
            };
        }

        RateLimitDecision::Allowed {
            remaining: state.window.remaining(),
            reset_at: state.window.reset_at(),
        }
    }

    pub async fn track_connection(&self, ip: std::net::IpAddr) -> bool {
        let mut ips = self.ips.write().await;
        let state = ips.entry(ip).or_insert_with(|| IpRateLimitState {
            window: SlidingWindow::new(60, self.config.requests_per_minute),
            connections: 0,
            blocked_until: None,
            last_seen: current_timestamp(),
        });

        if state.connections >= self.config.max_connections {
            false
        } else {
            state.connections += 1;
            true
        }
    }

    pub async fn release_connection(&self, ip: std::net::IpAddr) {
        let mut ips = self.ips.write().await;
        if let Some(state) = ips.get_mut(&ip) {
            state.connections = state.connections.saturating_sub(1);
        }
    }

    pub async fn block(&self, ip: std::net::IpAddr) {
        let mut ips = self.ips.write().await;
        if let Some(state) = ips.get_mut(&ip) {
            state.blocked_until = Some(current_timestamp() + self.config.block_duration_secs);
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_creation() {
        let limiter = QgeRateLimiter::new();
        let stats = limiter.get_stats().await;
        assert_eq!(stats.allowed_requests, 0);
    }

    #[tokio::test]
    async fn test_allow_request() {
        let limiter = QgeRateLimiter::new();
        let user = [1u8; 32];

        let decision = limiter
            .check(user, RequestCategory::MessageRelay, false)
            .await;

        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn test_throttle_after_limit() {
        let config = RateLimitConfig {
            refill_rate: 0.0, // No refill
            ..Default::default()
        };
        let limiter = QgeRateLimiter::with_config(config);
        let user = [1u8; 32];

        // Exhaust the limit
        for _ in 0..1000 {
            let decision = limiter
                .check(user, RequestCategory::TokenRequest, false)
                .await;
            if !decision.is_allowed() {
                // Got throttled as expected
                return;
            }
        }

        // Should have been throttled by now
        let decision = limiter
            .check(user, RequestCategory::TokenRequest, false)
            .await;
        assert!(!decision.is_allowed());
    }

    #[tokio::test]
    async fn test_emergency_bypass() {
        let config = RateLimitConfig {
            refill_rate: 0.0, // No refill
            ..Default::default()
        };
        let limiter = QgeRateLimiter::with_config(config);
        let user = [1u8; 32];

        // Use emergency bypass
        let decision = limiter
            .check(user, RequestCategory::TokenRequest, true)
            .await;

        match decision {
            RateLimitDecision::EmergencyBypass { remaining_bypasses } => {
                assert_eq!(remaining_bypasses, EMERGENCY_BYPASS_THRESHOLD - 1);
            }
            _ => panic!("Expected emergency bypass"),
        }
    }

    #[tokio::test]
    async fn test_reputation_update() {
        let limiter = QgeRateLimiter::new();
        let user = [1u8; 32];

        // Make a request to create user state
        limiter
            .check(user, RequestCategory::MessageRelay, false)
            .await;

        // Update reputation
        limiter.update_reputation(user, 20).await;

        let info = limiter.get_user_stats(user).await.unwrap();
        assert_eq!(info.reputation, 70); // 50 + 20
    }

    #[tokio::test]
    async fn test_abuse_blocking() {
        let limiter = QgeRateLimiter::new();
        let user = [1u8; 32];

        // Make a request to create user state
        limiter
            .check(user, RequestCategory::MessageRelay, false)
            .await;

        // Report severe abuse
        limiter.report_abuse(user, 200).await;

        let info = limiter.get_user_stats(user).await.unwrap();
        assert!(info.is_blocked);
    }

    #[tokio::test]
    async fn test_ip_rate_limiter() {
        let limiter = IpRateLimiter::new(IpRateLimitConfig {
            requests_per_minute: 5,
            ..Default::default()
        });

        let ip: std::net::IpAddr = "192.168.1.1".parse().unwrap();

        // First requests should pass
        for _ in 0..5 {
            let decision = limiter.check(ip).await;
            assert!(decision.is_allowed());
        }

        // Next should be throttled
        let decision = limiter.check(ip).await;
        assert!(!decision.is_allowed());
    }
}
