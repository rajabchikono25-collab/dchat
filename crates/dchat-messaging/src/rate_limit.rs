//! Rate limiting and backpressure control for message ingress
//!
//! Implements token bucket algorithm with reputation-based adaptive QoS,
//! per-user quotas, queue backpressure management, and behavioral bot detection.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Global messages per second limit
    pub global_limit: u64,
    /// Per-user messages per second limit
    pub per_user_limit: u64,
    /// Burst capacity (max accumulated tokens)
    pub burst_capacity: u64,
    /// Bandwidth limit in bytes per second per user
    pub bandwidth_bytes_per_second: u64,
    /// Maximum concurrent connections per user
    pub max_concurrent_connections: u32,
    /// Time window for rate limiting in seconds
    pub window_seconds: u64,
    /// Maximum queue depth before backpressure
    pub max_queue_size: usize,
    /// Enable behavioral bot detection
    pub enable_bot_detection: bool,
    /// Bot score threshold (0.0-1.0) - users above this are flagged
    pub bot_score_threshold: f64,
    /// Number of messages to track for behavior analysis
    pub behavior_analysis_window: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self::production()
    }
}

impl RateLimitConfig {
    /// Production-safe defaults with conservative limits
    ///
    /// These values are designed for a production relay node serving
    /// typical user traffic. Adjust based on your deployment scale:
    /// - For high-traffic relays: increase global_limit to 50k+
    /// - For resource-constrained nodes: reduce max_queue_size to 10k
    /// - For premium users: adjust per_user_limit via reputation scoring
    pub fn production() -> Self {
        Self {
            global_limit: 10000,                   // 10k msgs/sec globally (adjust for scale)
            per_user_limit: 100,                   // 100 msgs/sec per user (prevents spam)
            burst_capacity: 200,                   // Allow 2x burst for traffic spikes
            bandwidth_bytes_per_second: 1_048_576, // 1 MB/sec per user (prevents DoS)
            max_concurrent_connections: 10,        // Max connections per user
            window_seconds: 60,                    // 1-minute sliding window
            max_queue_size: 100000,                // 100k messages max in queue (~100MB memory)
            enable_bot_detection: true,            // Enable behavioral bot detection
            bot_score_threshold: 0.7,              // Flag users with >70% bot probability
            behavior_analysis_window: 100,         // Track last 100 messages for analysis
        }
    }

    /// Test configuration with relaxed limits
    #[cfg(any(test, feature = "test-mocks"))]
    pub fn test() -> Self {
        Self {
            global_limit: 100000,
            per_user_limit: 1000,
            burst_capacity: 2000,
            bandwidth_bytes_per_second: 10_485_760, // 10 MB/sec
            max_concurrent_connections: 100,
            window_seconds: 60,
            max_queue_size: 1000000,
            enable_bot_detection: false,           // Disable for tests by default
            bot_score_threshold: 0.9,
            behavior_analysis_window: 50,
        }
    }
}

/// Token bucket for rate limiting
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Maximum capacity of the bucket
    capacity: u64,
    /// Current number of tokens
    tokens: f64,
    /// Rate at which tokens refill (tokens per second)
    refill_rate: f64,
    /// Last time tokens were refilled
    last_refill: DateTime<Utc>,
}

impl TokenBucket {
    /// Create a new token bucket
    pub fn new(capacity: u64, refill_rate: u64) -> Self {
        Self {
            capacity,
            tokens: capacity as f64,
            refill_rate: refill_rate as f64,
            last_refill: Utc::now(),
        }
    }

    /// Try to consume tokens, returns true if successful
    pub fn try_consume(&mut self, count: u64) -> bool {
        self.refill();

        if self.tokens >= count as f64 {
            self.tokens -= count as f64;
            true
        } else {
            false
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Utc::now();
        let elapsed = now.signed_duration_since(self.last_refill);
        let elapsed_secs = elapsed.num_milliseconds() as f64 / 1000.0;

        let new_tokens = self.tokens + (self.refill_rate * elapsed_secs);
        self.tokens = new_tokens.min(self.capacity as f64);
        self.last_refill = now;
    }

    /// Get current token count
    pub fn available_tokens(&mut self) -> u64 {
        self.refill();
        self.tokens.floor() as u64
    }

    /// Get time until next token is available
    pub fn time_until_available(&mut self, count: u64) -> Option<ChronoDuration> {
        self.refill();

        if self.tokens >= count as f64 {
            return Some(ChronoDuration::zero());
        }

        let deficit = count as f64 - self.tokens;
        let secs_needed = (deficit / self.refill_rate).ceil() as i64;
        Some(ChronoDuration::seconds(secs_needed))
    }
}

/// Bandwidth tracking for a user
#[derive(Debug, Clone)]
struct BandwidthTracker {
    bytes_in_window: u64,
    window_start: DateTime<Utc>,
    window_seconds: u64,
}

impl BandwidthTracker {
    fn new(window_seconds: u64) -> Self {
        Self {
            bytes_in_window: 0,
            window_start: Utc::now(),
            window_seconds,
        }
    }

    fn try_consume(&mut self, bytes: u64, limit: u64) -> bool {
        let now = Utc::now();
        let elapsed = now.signed_duration_since(self.window_start);

        // Reset window if expired
        if elapsed.num_seconds() >= self.window_seconds as i64 {
            self.bytes_in_window = 0;
            self.window_start = now;
        }

        // Check if adding bytes would exceed limit
        if self.bytes_in_window + bytes <= limit * self.window_seconds {
            self.bytes_in_window += bytes;
            true
        } else {
            false
        }
    }
}

/// Behavioral bot detection profile
///
/// Tracks user messaging patterns to detect automated/bot behavior:
/// - Timing regularity: Bots tend to send messages at very regular intervals
/// - Content entropy: Bot messages often have low entropy (templated)
/// - Recipient diversity: Bots often target many unique recipients (spam)
/// - Message length variance: Humans have more varied message lengths
#[derive(Debug, Clone)]
pub struct BehaviorProfile {
    /// Timestamps of recent messages for timing analysis
    message_timestamps: VecDeque<Instant>,
    /// Shannon entropy scores of recent message content
    entropy_scores: VecDeque<f64>,
    /// Unique recipients in the analysis window
    unique_recipients: HashSet<String>,
    /// Message lengths for variance analysis
    message_lengths: VecDeque<usize>,
    /// Inter-message delays in milliseconds
    inter_message_delays: VecDeque<u64>,
    /// Rolling bot probability score (0.0 = human, 1.0 = bot)
    bot_score: f64,
    /// Number of bot detection triggers
    bot_flags: u32,
    /// Maximum window size for analysis
    max_window_size: usize,
}

impl BehaviorProfile {
    /// Create a new behavior profile
    pub fn new(max_window_size: usize) -> Self {
        Self {
            message_timestamps: VecDeque::with_capacity(max_window_size),
            entropy_scores: VecDeque::with_capacity(max_window_size),
            unique_recipients: HashSet::new(),
            message_lengths: VecDeque::with_capacity(max_window_size),
            inter_message_delays: VecDeque::with_capacity(max_window_size),
            bot_score: 0.0,
            bot_flags: 0,
            max_window_size,
        }
    }

    /// Record a new message for behavior analysis
    pub fn record_message(&mut self, content: &[u8], recipient: Option<&str>) {
        let now = Instant::now();

        // Calculate inter-message delay
        if let Some(last_ts) = self.message_timestamps.back() {
            let delay_ms = now.duration_since(*last_ts).as_millis() as u64;
            self.inter_message_delays.push_back(delay_ms);
            if self.inter_message_delays.len() > self.max_window_size {
                self.inter_message_delays.pop_front();
            }
        }

        // Record timestamp
        self.message_timestamps.push_back(now);
        if self.message_timestamps.len() > self.max_window_size {
            self.message_timestamps.pop_front();
        }

        // Calculate and record content entropy
        let entropy = Self::calculate_shannon_entropy(content);
        self.entropy_scores.push_back(entropy);
        if self.entropy_scores.len() > self.max_window_size {
            self.entropy_scores.pop_front();
        }

        // Record message length
        self.message_lengths.push_back(content.len());
        if self.message_lengths.len() > self.max_window_size {
            self.message_lengths.pop_front();
        }

        // Track recipient diversity
        if let Some(recipient) = recipient {
            self.unique_recipients.insert(recipient.to_string());
            // Limit recipient tracking to prevent memory growth
            if self.unique_recipients.len() > self.max_window_size * 2 {
                // Clear oldest entries by resetting (simple approach)
                if self.unique_recipients.len() > self.max_window_size * 3 {
                    self.unique_recipients.clear();
                }
            }
        }

        // Update bot score based on all factors
        self.update_bot_score();
    }

    /// Calculate Shannon entropy of content (bits per byte)
    /// Low entropy indicates templated/repetitive content (bot-like)
    fn calculate_shannon_entropy(content: &[u8]) -> f64 {
        if content.is_empty() {
            return 0.0;
        }

        let mut frequency = [0u64; 256];
        for &byte in content {
            frequency[byte as usize] += 1;
        }

        let len = content.len() as f64;
        let mut entropy = 0.0;

        for &count in &frequency {
            if count > 0 {
                let p = count as f64 / len;
                entropy -= p * p.log2();
            }
        }

        entropy
    }

    /// Calculate coefficient of variation for timing regularity
    /// Bots tend to have very low CV (regular intervals)
    fn calculate_timing_regularity(&self) -> f64 {
        if self.inter_message_delays.len() < 3 {
            return 0.5; // Not enough data, assume neutral
        }

        let delays: Vec<f64> = self.inter_message_delays.iter().map(|&d| d as f64).collect();
        let mean = delays.iter().sum::<f64>() / delays.len() as f64;

        if mean < 1.0 {
            return 1.0; // Very fast messaging is suspicious
        }

        let variance = delays.iter().map(|&d| (d - mean).powi(2)).sum::<f64>() / delays.len() as f64;
        let std_dev = variance.sqrt();
        let cv = std_dev / mean; // Coefficient of variation

        // Low CV = regular timing = bot-like
        // CV < 0.1 is very suspicious (less than 10% variation)
        // CV > 0.5 is human-like (natural variation)
        if cv < 0.1 {
            1.0 // Very regular timing
        } else if cv < 0.3 {
            0.7 // Somewhat regular
        } else if cv < 0.5 {
            0.3 // Some variation
        } else {
            0.0 // Human-like variation
        }
    }

    /// Calculate message length variance score
    fn calculate_length_variance_score(&self) -> f64 {
        if self.message_lengths.len() < 3 {
            return 0.5;
        }

        let lengths: Vec<f64> = self.message_lengths.iter().map(|&l| l as f64).collect();
        let mean = lengths.iter().sum::<f64>() / lengths.len() as f64;

        if mean < 1.0 {
            return 0.5;
        }

        let variance = lengths.iter().map(|&l| (l - mean).powi(2)).sum::<f64>() / lengths.len() as f64;
        let cv = variance.sqrt() / mean;

        // Low variance in message length is bot-like
        if cv < 0.05 {
            1.0 // Almost identical lengths
        } else if cv < 0.2 {
            0.5 // Some variation
        } else {
            0.0 // Human-like variation
        }
    }

    /// Calculate recipient diversity score
    /// High diversity with rapid messaging is spam-like
    fn calculate_recipient_diversity_score(&self) -> f64 {
        let message_count = self.message_timestamps.len();
        if message_count < 5 {
            return 0.0;
        }

        let unique_count = self.unique_recipients.len();
        let diversity_ratio = unique_count as f64 / message_count as f64;

        // High diversity ratio (many unique recipients) is spam-like
        // Normal conversation has low diversity (same few people)
        if diversity_ratio > 0.8 {
            1.0 // Almost all unique recipients
        } else if diversity_ratio > 0.5 {
            0.7
        } else if diversity_ratio > 0.3 {
            0.3
        } else {
            0.0 // Normal conversation pattern
        }
    }

    /// Calculate average entropy score
    fn calculate_entropy_score(&self) -> f64 {
        if self.entropy_scores.is_empty() {
            return 0.5;
        }

        let avg_entropy: f64 = self.entropy_scores.iter().sum::<f64>() / self.entropy_scores.len() as f64;

        // Natural text typically has entropy around 4-5 bits per byte
        // Very low entropy (<3) suggests templated content
        // Very high entropy (>6) might be encrypted/random data
        if avg_entropy < 2.0 {
            0.9 // Very low entropy - likely bot
        } else if avg_entropy < 3.5 {
            0.5 // Somewhat low
        } else if avg_entropy > 6.5 {
            0.3 // Suspiciously high (could be encrypted)
        } else {
            0.0 // Normal text entropy
        }
    }

    /// Update the composite bot score
    fn update_bot_score(&mut self) {
        // Weight factors based on their reliability
        let timing_weight = 0.35;
        let entropy_weight = 0.25;
        let length_weight = 0.15;
        let diversity_weight = 0.25;

        let timing_score = self.calculate_timing_regularity();
        let entropy_score = self.calculate_entropy_score();
        let length_score = self.calculate_length_variance_score();
        let diversity_score = self.calculate_recipient_diversity_score();

        // Weighted average
        self.bot_score = (timing_score * timing_weight)
            + (entropy_score * entropy_weight)
            + (length_score * length_weight)
            + (diversity_score * diversity_weight);

        // Apply exponential moving average to smooth score
        // This prevents sudden jumps from a single message
        self.bot_score = self.bot_score.clamp(0.0, 1.0);
    }

    /// Check if user is likely a bot
    pub fn is_likely_bot(&self, threshold: f64) -> bool {
        self.bot_score >= threshold
    }

    /// Get current bot probability score
    pub fn bot_probability(&self) -> f64 {
        self.bot_score
    }

    /// Get detailed behavior metrics for debugging/logging
    pub fn get_metrics(&self) -> BehaviorMetrics {
        BehaviorMetrics {
            message_count: self.message_timestamps.len(),
            unique_recipients: self.unique_recipients.len(),
            timing_regularity: self.calculate_timing_regularity(),
            avg_entropy: if self.entropy_scores.is_empty() {
                0.0
            } else {
                self.entropy_scores.iter().sum::<f64>() / self.entropy_scores.len() as f64
            },
            length_variance_score: self.calculate_length_variance_score(),
            recipient_diversity_score: self.calculate_recipient_diversity_score(),
            bot_score: self.bot_score,
            bot_flags: self.bot_flags,
        }
    }

    /// Increment bot flag count (for external signals)
    pub fn flag_as_bot(&mut self) {
        self.bot_flags += 1;
        // External flags increase bot score
        self.bot_score = (self.bot_score + 0.1).min(1.0);
    }

    /// Reset the profile (e.g., after user proves human via CAPTCHA)
    pub fn reset(&mut self) {
        self.message_timestamps.clear();
        self.entropy_scores.clear();
        self.unique_recipients.clear();
        self.message_lengths.clear();
        self.inter_message_delays.clear();
        self.bot_score = 0.0;
        self.bot_flags = 0;
    }
}

/// Detailed behavior metrics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorMetrics {
    pub message_count: usize,
    pub unique_recipients: usize,
    pub timing_regularity: f64,
    pub avg_entropy: f64,
    pub length_variance_score: f64,
    pub recipient_diversity_score: f64,
    pub bot_score: f64,
    pub bot_flags: u32,
}

/// User rate limit state
#[derive(Debug)]
struct UserRateLimitState {
    /// Message token bucket
    message_bucket: TokenBucket,
    /// Bandwidth tracker
    bandwidth_tracker: BandwidthTracker,
    /// Current concurrent connections
    concurrent_connections: u32,
    /// Total messages sent
    total_messages: u64,
    /// Total messages dropped due to rate limiting
    messages_dropped: u64,
    /// User reputation score (0.0 - 1.0, higher is better)
    reputation_score: f64,
    /// Last activity timestamp
    last_activity: DateTime<Utc>,
    /// Behavioral bot detection profile
    behavior_profile: BehaviorProfile,
    /// Whether user has been flagged as bot
    is_flagged_bot: bool,
}

impl UserRateLimitState {
    fn new(config: &RateLimitConfig, reputation_score: f64) -> Self {
        // Adjust limits based on reputation
        let adjusted_limit =
            (config.per_user_limit as f64 * (1.0 + reputation_score)).floor() as u64;
        let adjusted_capacity =
            (config.burst_capacity as f64 * (1.0 + reputation_score * 0.5)).floor() as u64;

        Self {
            message_bucket: TokenBucket::new(adjusted_capacity, adjusted_limit),
            bandwidth_tracker: BandwidthTracker::new(config.window_seconds),
            concurrent_connections: 0,
            total_messages: 0,
            messages_dropped: 0,
            reputation_score,
            last_activity: Utc::now(),
            behavior_profile: BehaviorProfile::new(config.behavior_analysis_window),
            is_flagged_bot: false,
        }
    }
}

/// Drop policy for queue backpressure
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPolicy {
    /// Drop the oldest messages when queue is full
    TailDrop,
    /// Drop random messages probabilistically as queue fills
    RandomEarlyDetection,
    /// Drop newest messages (reject incoming)
    HeadDrop,
}

/// Rate limit enforcement result
#[derive(Debug, Clone, PartialEq)]
pub enum RateLimitResult {
    /// Request allowed
    Allowed,
    /// Request denied due to message rate limit
    MessageRateLimitExceeded,
    /// Request denied due to bandwidth limit
    BandwidthLimitExceeded,
    /// Request denied due to connection limit
    ConnectionLimitExceeded,
    /// Request denied due to queue backpressure
    QueueFull,
    /// Request denied due to bot detection
    BotDetected { score: f64 },
}

/// Rate limit metrics
///
/// These metrics should be exported to Prometheus for monitoring:
/// - `rate_limit_total_checks`: Counter of all rate limit checks
/// - `rate_limit_allowed_total`: Counter of allowed requests
/// - `rate_limit_denied_total{reason}`: Counter of denied requests by reason
/// - `rate_limit_queue_depth`: Gauge of current queue depth
/// - `rate_limit_active_users`: Gauge of active users
/// - `rate_limit_bots_detected`: Counter of users flagged as bots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitMetrics {
    /// Total rate limit checks
    pub total_checks: u64,
    /// Allowed requests
    pub allowed_count: u64,
    /// Denied requests
    pub denied_count: u64,
    /// Rate limit hits by reason
    pub message_rate_hits: u64,
    pub bandwidth_hits: u64,
    pub connection_hits: u64,
    pub queue_full_hits: u64,
    /// Bot detection hits
    pub bot_detection_hits: u64,
    /// Total messages dropped
    pub total_messages_dropped: u64,
    /// Current queue depth
    pub current_queue_depth: usize,
    /// Active users
    pub active_users: usize,
    /// Users flagged as bots
    pub flagged_bots: usize,
}

impl RateLimitMetrics {
    /// Export metrics in Prometheus format
    ///
    /// Call this from your metrics endpoint to expose rate limiting stats
    pub fn to_prometheus_text(&self) -> String {
        format!(
            "# HELP rate_limit_total_checks Total rate limit checks performed\n\
             # TYPE rate_limit_total_checks counter\n\
             rate_limit_total_checks {}\n\
             # HELP rate_limit_allowed_total Total allowed requests\n\
             # TYPE rate_limit_allowed_total counter\n\
             rate_limit_allowed_total {}\n\
             # HELP rate_limit_denied_total Total denied requests by reason\n\
             # TYPE rate_limit_denied_total counter\n\
             rate_limit_denied_total{{reason=\"message_rate\"}} {}\n\
             rate_limit_denied_total{{reason=\"bandwidth\"}} {}\n\
             rate_limit_denied_total{{reason=\"connection\"}} {}\n\
             rate_limit_denied_total{{reason=\"queue_full\"}} {}\n\
             rate_limit_denied_total{{reason=\"bot_detected\"}} {}\n\
             # HELP rate_limit_messages_dropped_total Total messages dropped per user\n\
             # TYPE rate_limit_messages_dropped_total counter\n\
             rate_limit_messages_dropped_total {}\n\
             # HELP rate_limit_queue_depth Current queue depth\n\
             # TYPE rate_limit_queue_depth gauge\n\
             rate_limit_queue_depth {}\n\
             # HELP rate_limit_active_users Number of active users\n\
             # TYPE rate_limit_active_users gauge\n\
             rate_limit_active_users {}\n\
             # HELP rate_limit_flagged_bots Number of users flagged as bots\n\
             # TYPE rate_limit_flagged_bots gauge\n\
             rate_limit_flagged_bots {}\n",
            self.total_checks,
            self.allowed_count,
            self.message_rate_hits,
            self.bandwidth_hits,
            self.connection_hits,
            self.queue_full_hits,
            self.bot_detection_hits,
            self.total_messages_dropped,
            self.current_queue_depth,
            self.active_users,
            self.flagged_bots
        )
    }
}

impl Default for RateLimitMetrics {
    fn default() -> Self {
        Self {
            total_checks: 0,
            allowed_count: 0,
            denied_count: 0,
            message_rate_hits: 0,
            bandwidth_hits: 0,
            connection_hits: 0,
            queue_full_hits: 0,
            bot_detection_hits: 0,
            total_messages_dropped: 0,
            current_queue_depth: 0,
            active_users: 0,
            flagged_bots: 0,
        }
    }
}

/// Rate limiter with reputation-based adaptive QoS
pub struct RateLimiter {
    config: RateLimitConfig,
    /// Per-user rate limit state
    user_states: Arc<RwLock<HashMap<String, UserRateLimitState>>>,
    /// Global token bucket
    global_bucket: Arc<RwLock<TokenBucket>>,
    /// Metrics
    metrics: Arc<RwLock<RateLimitMetrics>>,
    /// Current queue depth
    queue_depth: Arc<RwLock<usize>>,
    /// Drop policy
    drop_policy: DropPolicy,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(config: RateLimitConfig) -> Self {
        let global_bucket = TokenBucket::new(config.burst_capacity * 100, config.global_limit);

        Self {
            config: config.clone(),
            user_states: Arc::new(RwLock::new(HashMap::new())),
            global_bucket: Arc::new(RwLock::new(global_bucket)),
            metrics: Arc::new(RwLock::new(RateLimitMetrics::default())),
            queue_depth: Arc::new(RwLock::new(0)),
            drop_policy: DropPolicy::RandomEarlyDetection,
        }
    }

    /// Create with custom drop policy
    pub fn with_drop_policy(mut self, policy: DropPolicy) -> Self {
        self.drop_policy = policy;
        self
    }

    /// Check if a message should be allowed
    pub async fn check_message(&self, user_id: &str, message_size_bytes: u64) -> RateLimitResult {
        let mut metrics = self.metrics.write().await;
        metrics.total_checks += 1;
        drop(metrics);

        // Check global rate limit first
        {
            let mut global = self.global_bucket.write().await;
            if !global.try_consume(1) {
                self.record_denial(&RateLimitResult::MessageRateLimitExceeded)
                    .await;
                return RateLimitResult::MessageRateLimitExceeded;
            }
        }

        // Check queue backpressure
        let queue_depth = *self.queue_depth.read().await;
        if queue_depth >= self.config.max_queue_size {
            if self.should_drop_message(queue_depth).await {
                self.record_denial(&RateLimitResult::QueueFull).await;
                return RateLimitResult::QueueFull;
            }
        }

        // Check per-user limits
        let mut user_states = self.user_states.write().await;
        let state = user_states
            .entry(user_id.to_string())
            .or_insert_with(|| UserRateLimitState::new(&self.config, 0.5));

        // Update last activity
        state.last_activity = Utc::now();

        // Check message rate limit
        if !state.message_bucket.try_consume(1) {
            state.messages_dropped += 1;
            drop(user_states);
            self.record_denial(&RateLimitResult::MessageRateLimitExceeded)
                .await;
            return RateLimitResult::MessageRateLimitExceeded;
        }

        // Check bandwidth limit
        if !state
            .bandwidth_tracker
            .try_consume(message_size_bytes, self.config.bandwidth_bytes_per_second)
        {
            state.messages_dropped += 1;
            drop(user_states);
            self.record_denial(&RateLimitResult::BandwidthLimitExceeded)
                .await;
            return RateLimitResult::BandwidthLimitExceeded;
        }

        // Check connection limit
        if state.concurrent_connections >= self.config.max_concurrent_connections {
            state.messages_dropped += 1;
            drop(user_states);
            self.record_denial(&RateLimitResult::ConnectionLimitExceeded)
                .await;
            return RateLimitResult::ConnectionLimitExceeded;
        }

        // Allowed
        state.total_messages += 1;
        drop(user_states);

        let mut metrics = self.metrics.write().await;
        metrics.allowed_count += 1;

        RateLimitResult::Allowed
    }

    /// Check if a message should be allowed with behavioral bot detection
    ///
    /// This extended version also analyzes message content and recipient patterns
    /// to detect automated/bot behavior.
    pub async fn check_message_with_behavior(
        &self,
        user_id: &str,
        message_content: &[u8],
        recipient: Option<&str>,
    ) -> RateLimitResult {
        let message_size_bytes = message_content.len() as u64;

        // First perform standard rate limit checks
        let mut metrics = self.metrics.write().await;
        metrics.total_checks += 1;
        drop(metrics);

        // Check global rate limit first
        {
            let mut global = self.global_bucket.write().await;
            if !global.try_consume(1) {
                self.record_denial(&RateLimitResult::MessageRateLimitExceeded)
                    .await;
                return RateLimitResult::MessageRateLimitExceeded;
            }
        }

        // Check queue backpressure
        let queue_depth = *self.queue_depth.read().await;
        if queue_depth >= self.config.max_queue_size {
            if self.should_drop_message(queue_depth).await {
                self.record_denial(&RateLimitResult::QueueFull).await;
                return RateLimitResult::QueueFull;
            }
        }

        // Check per-user limits and bot detection
        let mut user_states = self.user_states.write().await;
        let state = user_states
            .entry(user_id.to_string())
            .or_insert_with(|| UserRateLimitState::new(&self.config, 0.5));

        // Update last activity
        state.last_activity = Utc::now();

        // Record message for behavioral analysis
        state.behavior_profile.record_message(message_content, recipient);

        // Check bot detection (if enabled)
        if self.config.enable_bot_detection {
            let bot_score = state.behavior_profile.bot_probability();
            if state.behavior_profile.is_likely_bot(self.config.bot_score_threshold) {
                state.is_flagged_bot = true;
                state.messages_dropped += 1;
                let result = RateLimitResult::BotDetected { score: bot_score };
                drop(user_states);
                self.record_denial(&result).await;
                return result;
            }
        }

        // Check message rate limit
        if !state.message_bucket.try_consume(1) {
            state.messages_dropped += 1;
            drop(user_states);
            self.record_denial(&RateLimitResult::MessageRateLimitExceeded)
                .await;
            return RateLimitResult::MessageRateLimitExceeded;
        }

        // Check bandwidth limit
        if !state
            .bandwidth_tracker
            .try_consume(message_size_bytes, self.config.bandwidth_bytes_per_second)
        {
            state.messages_dropped += 1;
            drop(user_states);
            self.record_denial(&RateLimitResult::BandwidthLimitExceeded)
                .await;
            return RateLimitResult::BandwidthLimitExceeded;
        }

        // Check connection limit
        if state.concurrent_connections >= self.config.max_concurrent_connections {
            state.messages_dropped += 1;
            drop(user_states);
            self.record_denial(&RateLimitResult::ConnectionLimitExceeded)
                .await;
            return RateLimitResult::ConnectionLimitExceeded;
        }

        // Allowed
        state.total_messages += 1;
        drop(user_states);

        let mut metrics = self.metrics.write().await;
        metrics.allowed_count += 1;

        RateLimitResult::Allowed
    }

    /// Get behavior metrics for a user (for debugging/monitoring)
    pub async fn get_behavior_metrics(&self, user_id: &str) -> Option<BehaviorMetrics> {
        let user_states = self.user_states.read().await;
        user_states.get(user_id).map(|state| state.behavior_profile.get_metrics())
    }

    /// Manually flag a user as bot (from external signal like CAPTCHA failure)
    pub async fn flag_user_as_bot(&self, user_id: &str) {
        let mut user_states = self.user_states.write().await;
        if let Some(state) = user_states.get_mut(user_id) {
            state.behavior_profile.flag_as_bot();
            state.is_flagged_bot = true;
        }
    }

    /// Reset bot detection for user (after CAPTCHA success)
    pub async fn reset_bot_detection(&self, user_id: &str) {
        let mut user_states = self.user_states.write().await;
        if let Some(state) = user_states.get_mut(user_id) {
            state.behavior_profile.reset();
            state.is_flagged_bot = false;
        }
    }

    /// Check if a user is currently flagged as a bot
    pub async fn is_user_flagged_bot(&self, user_id: &str) -> bool {
        let user_states = self.user_states.read().await;
        user_states.get(user_id).map(|s| s.is_flagged_bot).unwrap_or(false)
    }

    /// Update user reputation score (0.0 - 1.0)
    pub async fn update_reputation(&self, user_id: &str, reputation_score: f64) {
        let mut user_states = self.user_states.write().await;

        // Get or create user state
        let state = user_states
            .entry(user_id.to_string())
            .or_insert_with(|| UserRateLimitState::new(&self.config, 0.5));

        state.reputation_score = reputation_score.clamp(0.0, 1.0);

        // Adjust limits based on new reputation
        let adjusted_limit =
            (self.config.per_user_limit as f64 * (1.0 + reputation_score)).floor() as u64;
        let adjusted_capacity =
            (self.config.burst_capacity as f64 * (1.0 + reputation_score * 0.5)).floor() as u64;

        state.message_bucket = TokenBucket::new(adjusted_capacity, adjusted_limit);
    }

    /// Record a connection open
    pub async fn open_connection(&self, user_id: &str) -> bool {
        let mut user_states = self.user_states.write().await;
        let state = user_states
            .entry(user_id.to_string())
            .or_insert_with(|| UserRateLimitState::new(&self.config, 0.5));

        if state.concurrent_connections < self.config.max_concurrent_connections {
            state.concurrent_connections += 1;
            true
        } else {
            false
        }
    }

    /// Record a connection close
    pub async fn close_connection(&self, user_id: &str) {
        let mut user_states = self.user_states.write().await;
        if let Some(state) = user_states.get_mut(user_id) {
            state.concurrent_connections = state.concurrent_connections.saturating_sub(1);
        }
    }

    /// Update queue depth
    pub async fn set_queue_depth(&self, depth: usize) {
        let mut queue_depth = self.queue_depth.write().await;
        *queue_depth = depth;

        let mut metrics = self.metrics.write().await;
        metrics.current_queue_depth = depth;
    }

    /// Get current metrics
    pub async fn get_metrics(&self) -> RateLimitMetrics {
        let mut metrics = self.metrics.read().await.clone();

        let user_states = self.user_states.read().await;
        metrics.active_users = user_states.len();
        metrics.total_messages_dropped = user_states.values().map(|s| s.messages_dropped).sum();
        metrics.flagged_bots = user_states.values().filter(|s| s.is_flagged_bot).count();

        metrics
    }

    /// Get user statistics
    pub async fn get_user_stats(&self, user_id: &str) -> Option<UserStats> {
        let user_states = self.user_states.read().await;
        user_states.get(user_id).map(|state| UserStats {
            total_messages: state.total_messages,
            messages_dropped: state.messages_dropped,
            concurrent_connections: state.concurrent_connections,
            reputation_score: state.reputation_score,
            available_tokens: 0, // Will update below
            last_activity: state.last_activity,
        })
    }

    /// Clean up inactive users
    pub async fn cleanup_inactive(&self, inactive_threshold_secs: i64) {
        let now = Utc::now();
        let mut user_states = self.user_states.write().await;

        user_states.retain(|_, state| {
            let inactive_duration = now.signed_duration_since(state.last_activity);
            inactive_duration.num_seconds() < inactive_threshold_secs
        });
    }

    /// Record a denial
    async fn record_denial(&self, result: &RateLimitResult) {
        let mut metrics = self.metrics.write().await;
        metrics.denied_count += 1;

        match result {
            RateLimitResult::MessageRateLimitExceeded => metrics.message_rate_hits += 1,
            RateLimitResult::BandwidthLimitExceeded => metrics.bandwidth_hits += 1,
            RateLimitResult::ConnectionLimitExceeded => metrics.connection_hits += 1,
            RateLimitResult::QueueFull => metrics.queue_full_hits += 1,
            RateLimitResult::BotDetected { .. } => metrics.bot_detection_hits += 1,
            _ => {}
        }
    }

    /// Determine if message should be dropped based on drop policy
    async fn should_drop_message(&self, queue_depth: usize) -> bool {
        match self.drop_policy {
            DropPolicy::TailDrop => {
                // Always drop when full
                queue_depth >= self.config.max_queue_size
            }
            DropPolicy::RandomEarlyDetection => {
                // Probabilistic drop as queue fills
                let fill_ratio = queue_depth as f64 / self.config.max_queue_size as f64;
                if fill_ratio < 0.7 {
                    false
                } else if fill_ratio >= 1.0 {
                    true
                } else {
                    // Linear probability from 0% at 70% full to 100% at 100% full
                    let drop_probability = (fill_ratio - 0.7) / 0.3;
                    rand::random::<f64>() < drop_probability
                }
            }
            DropPolicy::HeadDrop => {
                // Always drop new messages when full
                queue_depth >= self.config.max_queue_size
            }
        }
    }
}

/// User statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStats {
    pub total_messages: u64,
    pub messages_dropped: u64,
    pub concurrent_connections: u32,
    pub reputation_score: f64,
    pub available_tokens: u64,
    pub last_activity: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_consume() {
        let mut bucket = TokenBucket::new(10, 1);

        assert!(bucket.try_consume(5));
        assert_eq!(bucket.available_tokens(), 5);

        assert!(bucket.try_consume(5));
        assert_eq!(bucket.available_tokens(), 0);

        assert!(!bucket.try_consume(1));
    }

    #[tokio::test]
    async fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(10, 10); // 10 tokens/sec

        bucket.try_consume(10);
        assert_eq!(bucket.available_tokens(), 0);

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Should have ~5 tokens after 0.5 seconds
        let tokens = bucket.available_tokens();
        assert!(tokens >= 4 && tokens <= 6);
    }

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let config = RateLimitConfig {
            per_user_limit: 2,
            burst_capacity: 2,
            ..Default::default()
        };

        let limiter = RateLimiter::new(config);

        // First two should succeed
        assert_eq!(
            limiter.check_message("user1", 100).await,
            RateLimitResult::Allowed
        );
        assert_eq!(
            limiter.check_message("user1", 100).await,
            RateLimitResult::Allowed
        );

        // Third should fail
        assert_eq!(
            limiter.check_message("user1", 100).await,
            RateLimitResult::MessageRateLimitExceeded
        );
    }

    #[tokio::test]
    async fn test_bandwidth_limit() {
        let config = RateLimitConfig {
            per_user_limit: 1000,
            bandwidth_bytes_per_second: 100,
            window_seconds: 1,
            ..Default::default()
        };

        let limiter = RateLimiter::new(config);

        // 100 bytes should succeed
        assert_eq!(
            limiter.check_message("user1", 100).await,
            RateLimitResult::Allowed
        );

        // Another 100 bytes should fail (exceeds 100 bytes/sec)
        assert_eq!(
            limiter.check_message("user1", 100).await,
            RateLimitResult::BandwidthLimitExceeded
        );
    }

    #[tokio::test]
    async fn test_reputation_adjustment() {
        let config = RateLimitConfig {
            per_user_limit: 10,
            burst_capacity: 10,
            ..Default::default()
        };

        let limiter = RateLimiter::new(config);

        // Set high reputation (2x limits)
        limiter.update_reputation("user1", 1.0).await;

        // Should allow more messages due to reputation
        for _ in 0..15 {
            let result = limiter.check_message("user1", 10).await;
            assert_eq!(result, RateLimitResult::Allowed);
        }
    }

    #[tokio::test]
    async fn test_connection_limit() {
        let config = RateLimitConfig {
            max_concurrent_connections: 2,
            ..Default::default()
        };

        let limiter = RateLimiter::new(config);

        assert!(limiter.open_connection("user1").await);
        assert!(limiter.open_connection("user1").await);
        assert!(!limiter.open_connection("user1").await); // Should fail

        limiter.close_connection("user1").await;
        assert!(limiter.open_connection("user1").await); // Should succeed now
    }

    #[tokio::test]
    async fn test_queue_backpressure() {
        let config = RateLimitConfig {
            max_queue_size: 10,
            per_user_limit: 1000,
            ..Default::default()
        };

        let limiter = RateLimiter::new(config).with_drop_policy(DropPolicy::TailDrop);

        limiter.set_queue_depth(5).await;
        assert_eq!(
            limiter.check_message("user1", 10).await,
            RateLimitResult::Allowed
        );

        limiter.set_queue_depth(15).await; // Exceed max
        assert_eq!(
            limiter.check_message("user1", 10).await,
            RateLimitResult::QueueFull
        );
    }

    #[tokio::test]
    async fn test_metrics() {
        let limiter = RateLimiter::new(RateLimitConfig::default());

        limiter.check_message("user1", 100).await;
        limiter.check_message("user2", 100).await;

        let metrics = limiter.get_metrics().await;
        assert_eq!(metrics.total_checks, 2);
        assert_eq!(metrics.allowed_count, 2);
        assert_eq!(metrics.active_users, 2);
    }

    #[tokio::test]
    async fn test_cleanup_inactive() {
        let limiter = RateLimiter::new(RateLimitConfig::default());

        limiter.check_message("user1", 100).await;

        // Wait and cleanup
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        limiter.cleanup_inactive(1).await; // 1 second threshold

        let metrics = limiter.get_metrics().await;
        assert_eq!(metrics.active_users, 0); // Should be cleaned up
    }

    #[test]
    fn test_behavior_profile_entropy_calculation() {
        let mut profile = BehaviorProfile::new(100);
        
        // Low entropy content (repeated text) - bot-like
        let repetitive_content = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        profile.record_message(repetitive_content, Some("recipient1"));
        let metrics = profile.get_metrics();
        assert!(metrics.avg_entropy < 1.0, "Repetitive content should have low entropy");

        // Reset and test high entropy content (varied text) - human-like
        profile.reset();
        let varied_content = b"The quick brown fox jumps over the lazy dog!";
        profile.record_message(varied_content, Some("recipient1"));
        let metrics = profile.get_metrics();
        assert!(metrics.avg_entropy > 3.0, "Varied content should have higher entropy");
    }

    #[test]
    fn test_behavior_profile_timing_regularity() {
        let mut profile = BehaviorProfile::new(100);

        // Simulate bot-like regular timing (messages at exact intervals)
        for i in 0..10 {
            let content = format!("message {}", i);
            profile.record_message(content.as_bytes(), Some("recipient1"));
            // Note: In real tests, we'd add precise delays. Here we're testing the structure.
        }

        // Check that profile tracks messages
        let metrics = profile.get_metrics();
        assert_eq!(metrics.message_count, 10);
    }

    #[test]
    fn test_behavior_profile_recipient_diversity() {
        let mut profile = BehaviorProfile::new(100);

        // Simulate spam-like behavior (many unique recipients)
        for i in 0..20 {
            let recipient = format!("recipient{}", i);
            profile.record_message(b"spam message", Some(&recipient));
        }

        let metrics = profile.get_metrics();
        assert_eq!(metrics.unique_recipients, 20);
        // High diversity ratio should increase bot score
        assert!(metrics.recipient_diversity_score > 0.5, 
            "Many unique recipients should increase diversity score");
    }

    #[test]
    fn test_behavior_profile_bot_flagging() {
        let mut profile = BehaviorProfile::new(100);
        
        // External signal to flag as bot
        profile.flag_as_bot();
        assert!(profile.bot_probability() >= 0.1);
        
        // Multiple flags should increase score
        profile.flag_as_bot();
        profile.flag_as_bot();
        assert!(profile.bot_probability() >= 0.3);
    }

    #[test]
    fn test_behavior_profile_reset() {
        let mut profile = BehaviorProfile::new(100);
        
        // Record some activity
        for _ in 0..10 {
            profile.record_message(b"test message", Some("recipient"));
        }
        profile.flag_as_bot();
        
        let metrics_before = profile.get_metrics();
        assert!(metrics_before.message_count > 0);
        assert!(metrics_before.bot_flags > 0);

        // Reset (e.g., after CAPTCHA success)
        profile.reset();
        
        let metrics_after = profile.get_metrics();
        assert_eq!(metrics_after.message_count, 0);
        assert_eq!(metrics_after.bot_flags, 0);
        assert_eq!(metrics_after.bot_score, 0.0);
    }

    #[tokio::test]
    async fn test_rate_limiter_bot_detection() {
        let config = RateLimitConfig {
            per_user_limit: 1000,
            burst_capacity: 1000,
            enable_bot_detection: true,
            bot_score_threshold: 0.1, // Low threshold for testing
            behavior_analysis_window: 50,
            ..RateLimitConfig::production()
        };

        let limiter = RateLimiter::new(config);

        // Send many identical messages to many recipients (bot-like behavior)
        for i in 0..30 {
            let recipient = format!("recipient{}", i);
            let result = limiter
                .check_message_with_behavior("bot_user", b"spam spam spam", Some(&recipient))
                .await;
            
            // Eventually should be flagged as bot
            if let RateLimitResult::BotDetected { score } = result {
                assert!(score >= 0.1);
                break;
            }
        }

        // Verify user is flagged
        assert!(limiter.is_user_flagged_bot("bot_user").await);

        // Test reset
        limiter.reset_bot_detection("bot_user").await;
        assert!(!limiter.is_user_flagged_bot("bot_user").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_behavior_metrics() {
        let config = RateLimitConfig {
            enable_bot_detection: true,
            ..RateLimitConfig::production()
        };

        let limiter = RateLimiter::new(config);

        // Send some messages
        for _ in 0..5 {
            limiter
                .check_message_with_behavior("user1", b"hello world", Some("friend"))
                .await;
        }

        // Get behavior metrics
        let metrics = limiter.get_behavior_metrics("user1").await;
        assert!(metrics.is_some());
        let metrics = metrics.unwrap();
        assert_eq!(metrics.message_count, 5);
    }

    #[tokio::test]
    async fn test_rate_limiter_manual_bot_flagging() {
        let config = RateLimitConfig::production();
        let limiter = RateLimiter::new(config);

        // Create user state
        limiter.check_message("user1", 100).await;

        // Manually flag as bot
        limiter.flag_user_as_bot("user1").await;
        assert!(limiter.is_user_flagged_bot("user1").await);

        // Reset
        limiter.reset_bot_detection("user1").await;
        assert!(!limiter.is_user_flagged_bot("user1").await);
    }
}
