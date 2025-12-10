//! Handshake-specific Prometheus metrics
//!
//! Provides detailed observability for the handshake protocol including:
//! - Connection attempts and completions
//! - Duration histograms
//! - Failure reasons
//! - Rate limiting statistics

use once_cell::sync::Lazy;
use prometheus::{
    Counter, CounterVec, Gauge, Histogram, HistogramOpts, HistogramVec, Opts, Registry,
};
use std::time::Instant;

/// Global handshake metrics registry
pub static HANDSHAKE_METRICS: Lazy<HandshakeMetrics> = Lazy::new(|| {
    HandshakeMetrics::new().expect("Failed to create handshake metrics")
});

/// Handshake outcomes for labeling
#[derive(Debug, Clone, Copy)]
pub enum HandshakeOutcome {
    Success,
    Timeout,
    Rejected,
    ProtocolError,
    CryptoError,
    RateLimited,
}

impl HandshakeOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            HandshakeOutcome::Success => "success",
            HandshakeOutcome::Timeout => "timeout",
            HandshakeOutcome::Rejected => "rejected",
            HandshakeOutcome::ProtocolError => "protocol_error",
            HandshakeOutcome::CryptoError => "crypto_error",
            HandshakeOutcome::RateLimited => "rate_limited",
        }
    }
}

/// Handshake role (initiator vs responder)
#[derive(Debug, Clone, Copy)]
pub enum HandshakeRole {
    Initiator,
    Responder,
}

impl HandshakeRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            HandshakeRole::Initiator => "initiator",
            HandshakeRole::Responder => "responder",
        }
    }
}

/// Failure reasons for detailed tracking
#[derive(Debug, Clone, Copy)]
pub enum FailureReason {
    Timeout,
    InvalidSignature,
    InvalidNonce,
    ExpiredChallenge,
    MismatchedPeerId,
    UnsupportedVersion,
    RateLimited,
    ConnectionDropped,
    MalformedMessage,
    InternalError,
}

impl FailureReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureReason::Timeout => "timeout",
            FailureReason::InvalidSignature => "invalid_signature",
            FailureReason::InvalidNonce => "invalid_nonce",
            FailureReason::ExpiredChallenge => "expired_challenge",
            FailureReason::MismatchedPeerId => "mismatched_peer_id",
            FailureReason::UnsupportedVersion => "unsupported_version",
            FailureReason::RateLimited => "rate_limited",
            FailureReason::ConnectionDropped => "connection_dropped",
            FailureReason::MalformedMessage => "malformed_message",
            FailureReason::InternalError => "internal_error",
        }
    }
}

/// Comprehensive handshake metrics
pub struct HandshakeMetrics {
    /// Total handshakes initiated (we are the initiator)
    pub initiated_total: Counter,
    
    /// Total handshakes received (we are the responder)
    pub received_total: Counter,
    
    /// Completed handshakes by outcome and role
    pub completed: CounterVec,
    
    /// Handshake duration in seconds
    pub duration_seconds: HistogramVec,
    
    /// Failures by reason
    pub failures: CounterVec,
    
    /// Currently active handshakes
    pub active: Gauge,
    
    /// Handshakes rejected by rate limiting
    pub rate_limited_total: Counter,
    
    /// Identity verification successes
    pub identity_verified_total: Counter,
    
    /// Identity verification failures
    pub identity_failed_total: Counter,
    
    /// Challenge-response round-trips
    pub challenge_responses_total: Counter,
    
    /// Handshake message sizes (bytes)
    pub message_size_bytes: Histogram,
    
    /// Handshakes by peer region (for geo-distributed monitoring)
    pub by_region: CounterVec,
    
    /// Noise protocol phase durations
    pub noise_phase_duration_seconds: HistogramVec,
}

impl HandshakeMetrics {
    /// Create new handshake metrics
    pub fn new() -> Result<Self, prometheus::Error> {
        let initiated_total = Counter::with_opts(Opts::new(
            "dchat_handshake_initiated_total",
            "Total handshakes initiated by this node",
        ))?;

        let received_total = Counter::with_opts(Opts::new(
            "dchat_handshake_received_total",
            "Total handshakes received by this node",
        ))?;

        let completed = CounterVec::new(
            Opts::new(
                "dchat_handshake_completed_total",
                "Completed handshakes by outcome and role",
            ),
            &["outcome", "role"],
        )?;

        let duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "dchat_handshake_duration_seconds",
                "Handshake duration in seconds",
            )
            .buckets(vec![0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["role", "outcome"],
        )?;

        let failures = CounterVec::new(
            Opts::new(
                "dchat_handshake_failures_total",
                "Handshake failures by reason",
            ),
            &["reason"],
        )?;

        let active = Gauge::with_opts(Opts::new(
            "dchat_handshake_active",
            "Currently active handshakes in progress",
        ))?;

        let rate_limited_total = Counter::with_opts(Opts::new(
            "dchat_handshake_rate_limited_total",
            "Handshakes rejected due to rate limiting",
        ))?;

        let identity_verified_total = Counter::with_opts(Opts::new(
            "dchat_handshake_identity_verified_total",
            "Successful identity verifications during handshake",
        ))?;

        let identity_failed_total = Counter::with_opts(Opts::new(
            "dchat_handshake_identity_failed_total",
            "Failed identity verifications during handshake",
        ))?;

        let challenge_responses_total = Counter::with_opts(Opts::new(
            "dchat_handshake_challenge_responses_total",
            "Total challenge-response exchanges",
        ))?;

        let message_size_bytes = Histogram::with_opts(
            HistogramOpts::new(
                "dchat_handshake_message_size_bytes",
                "Size of handshake messages in bytes",
            )
            .buckets(vec![64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0, 4096.0, 8192.0]),
        )?;

        let by_region = CounterVec::new(
            Opts::new(
                "dchat_handshake_by_region_total",
                "Handshakes by peer geographic region",
            ),
            &["region"],
        )?;

        let noise_phase_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "dchat_handshake_noise_phase_duration_seconds",
                "Duration of individual Noise protocol phases",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5]),
            &["phase"],
        )?;

        Ok(Self {
            initiated_total,
            received_total,
            completed,
            duration_seconds,
            failures,
            active,
            rate_limited_total,
            identity_verified_total,
            identity_failed_total,
            challenge_responses_total,
            message_size_bytes,
            by_region,
            noise_phase_duration_seconds,
        })
    }

    /// Register all metrics with a Prometheus registry
    pub fn register(&self, registry: &Registry) -> Result<(), prometheus::Error> {
        registry.register(Box::new(self.initiated_total.clone()))?;
        registry.register(Box::new(self.received_total.clone()))?;
        registry.register(Box::new(self.completed.clone()))?;
        registry.register(Box::new(self.duration_seconds.clone()))?;
        registry.register(Box::new(self.failures.clone()))?;
        registry.register(Box::new(self.active.clone()))?;
        registry.register(Box::new(self.rate_limited_total.clone()))?;
        registry.register(Box::new(self.identity_verified_total.clone()))?;
        registry.register(Box::new(self.identity_failed_total.clone()))?;
        registry.register(Box::new(self.challenge_responses_total.clone()))?;
        registry.register(Box::new(self.message_size_bytes.clone()))?;
        registry.register(Box::new(self.by_region.clone()))?;
        registry.register(Box::new(self.noise_phase_duration_seconds.clone()))?;
        Ok(())
    }

    /// Record a handshake initiation
    pub fn record_initiated(&self) {
        self.initiated_total.inc();
        self.active.inc();
    }

    /// Record a handshake received
    pub fn record_received(&self) {
        self.received_total.inc();
        self.active.inc();
    }

    /// Record handshake completion
    pub fn record_completed(
        &self,
        role: HandshakeRole,
        outcome: HandshakeOutcome,
        duration_seconds: f64,
    ) {
        self.completed
            .with_label_values(&[outcome.as_str(), role.as_str()])
            .inc();
        self.duration_seconds
            .with_label_values(&[role.as_str(), outcome.as_str()])
            .observe(duration_seconds);
        self.active.dec();
    }

    /// Record a failure with reason
    pub fn record_failure(&self, reason: FailureReason) {
        self.failures.with_label_values(&[reason.as_str()]).inc();
    }

    /// Record rate limiting
    pub fn record_rate_limited(&self) {
        self.rate_limited_total.inc();
        self.record_failure(FailureReason::RateLimited);
    }

    /// Record successful identity verification
    pub fn record_identity_verified(&self) {
        self.identity_verified_total.inc();
    }

    /// Record failed identity verification
    pub fn record_identity_failed(&self) {
        self.identity_failed_total.inc();
    }

    /// Record a challenge-response exchange
    pub fn record_challenge_response(&self) {
        self.challenge_responses_total.inc();
    }

    /// Record handshake message size
    pub fn record_message_size(&self, size_bytes: usize) {
        self.message_size_bytes.observe(size_bytes as f64);
    }

    /// Record handshake by region
    pub fn record_by_region(&self, region: &str) {
        self.by_region.with_label_values(&[region]).inc();
    }

    /// Record Noise protocol phase duration
    pub fn record_noise_phase(&self, phase: &str, duration_seconds: f64) {
        self.noise_phase_duration_seconds
            .with_label_values(&[phase])
            .observe(duration_seconds);
    }
}

/// Timer for measuring handshake duration
pub struct HandshakeTimer {
    start: Instant,
    role: HandshakeRole,
    metrics: &'static HandshakeMetrics,
    completed: bool,
}

impl HandshakeTimer {
    /// Start a new handshake timer
    pub fn start_initiator() -> Self {
        HANDSHAKE_METRICS.record_initiated();
        Self {
            start: Instant::now(),
            role: HandshakeRole::Initiator,
            metrics: &HANDSHAKE_METRICS,
            completed: false,
        }
    }

    /// Start a new handshake timer as responder
    pub fn start_responder() -> Self {
        HANDSHAKE_METRICS.record_received();
        Self {
            start: Instant::now(),
            role: HandshakeRole::Responder,
            metrics: &HANDSHAKE_METRICS,
            completed: false,
        }
    }

    /// Complete the handshake with success
    pub fn complete_success(mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        self.metrics
            .record_completed(self.role, HandshakeOutcome::Success, duration);
        self.completed = true;
    }

    /// Complete the handshake with failure
    pub fn complete_failure(mut self, outcome: HandshakeOutcome, reason: FailureReason) {
        let duration = self.start.elapsed().as_secs_f64();
        self.metrics.record_completed(self.role, outcome, duration);
        self.metrics.record_failure(reason);
        self.completed = true;
    }

    /// Get elapsed time
    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
}

impl Drop for HandshakeTimer {
    fn drop(&mut self) {
        // If timer was dropped without completion, record as dropped connection
        if !self.completed {
            self.metrics.active.dec();
            self.metrics.record_failure(FailureReason::ConnectionDropped);
        }
    }
}

/// Noise protocol phase timer
pub struct NoisePhaseTimer {
    start: Instant,
    phase: String,
}

impl NoisePhaseTimer {
    /// Start timing a Noise phase
    pub fn start(phase: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            phase: phase.into(),
        }
    }

    /// Complete the phase and record metrics
    pub fn complete(self) {
        let duration = self.start.elapsed().as_secs_f64();
        HANDSHAKE_METRICS.record_noise_phase(&self.phase, duration);
    }
}

/// Summary of handshake metrics for health checks
#[derive(Debug, Clone)]
pub struct HandshakeHealthSummary {
    pub active_handshakes: f64,
    pub total_initiated: f64,
    pub total_received: f64,
    pub success_rate: f64,
    pub avg_duration_ms: f64,
    pub rate_limited_count: f64,
}

impl HandshakeHealthSummary {
    /// Calculate health summary from current metrics
    pub fn from_metrics(metrics: &HandshakeMetrics) -> Self {
        let total_initiated = metrics.initiated_total.get();
        let total_received = metrics.received_total.get();
        let total_attempts = total_initiated + total_received;
        
        // Calculate success rate (approximate)
        let success_initiated = metrics
            .completed
            .with_label_values(&["success", "initiator"])
            .get();
        let success_received = metrics
            .completed
            .with_label_values(&["success", "responder"])
            .get();
        let total_success = success_initiated + success_received;
        
        let success_rate = if total_attempts > 0.0 {
            total_success / total_attempts
        } else {
            1.0 // No attempts = 100% (no failures)
        };

        Self {
            active_handshakes: metrics.active.get(),
            total_initiated,
            total_received,
            success_rate,
            avg_duration_ms: 0.0, // Would need histogram queries
            rate_limited_count: metrics.rate_limited_total.get(),
        }
    }

    /// Check if handshake subsystem is healthy
    pub fn is_healthy(&self) -> bool {
        // Healthy if success rate > 80% and not too many active handshakes
        self.success_rate > 0.8 && self.active_handshakes < 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_metrics_creation() {
        let metrics = HandshakeMetrics::new().unwrap();
        assert_eq!(metrics.active.get(), 0.0);
    }

    #[test]
    fn test_record_initiated() {
        let metrics = HandshakeMetrics::new().unwrap();
        metrics.record_initiated();
        assert_eq!(metrics.initiated_total.get(), 1.0);
        assert_eq!(metrics.active.get(), 1.0);
    }

    #[test]
    fn test_record_completed() {
        let metrics = HandshakeMetrics::new().unwrap();
        metrics.record_initiated();
        metrics.record_completed(HandshakeRole::Initiator, HandshakeOutcome::Success, 0.5);
        assert_eq!(metrics.active.get(), 0.0);
    }

    #[test]
    fn test_handshake_timer() {
        let timer = HandshakeTimer::start_initiator();
        assert!(timer.elapsed_secs() >= 0.0);
        timer.complete_success();
    }

    #[test]
    fn test_noise_phase_timer() {
        let timer = NoisePhaseTimer::start("xx_init");
        std::thread::sleep(std::time::Duration::from_millis(1));
        timer.complete();
    }

    #[test]
    fn test_health_summary() {
        let metrics = HandshakeMetrics::new().unwrap();
        
        // Simulate some handshakes
        metrics.record_initiated();
        metrics.record_completed(HandshakeRole::Initiator, HandshakeOutcome::Success, 0.1);
        
        let summary = HandshakeHealthSummary::from_metrics(&metrics);
        assert_eq!(summary.total_initiated, 1.0);
        assert!(summary.is_healthy());
    }
}
