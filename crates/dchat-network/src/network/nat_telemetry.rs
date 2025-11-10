// src/network/nat_telemetry.rs - NAT traversal telemetry and metrics collection
//
// Tracks success rates and method usage for:
// - UPnP (Universal Plug and Play) - automatic port mapping
// - Hole Punching - simultaneous connection attempts
// - TURN (Traversal Using Relays around NAT) - relay fallback
//
// Integrates with Prometheus metrics for production observability

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum NatTelemetryError {
    #[error("Metrics recording failed: {0}")]
    MetricsError(String),
    #[error("Invalid method: {0}")]
    InvalidMethod(String),
}

/// NAT traversal method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NatMethod {
    /// UPnP automatic port mapping
    Upnp,
    /// Direct connection (no NAT or already mapped)
    Direct,
    /// Hole punching via simultaneous connection attempts
    HolePunch,
    /// TURN relay fallback
    Turn,
}

impl NatMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            NatMethod::Upnp => "upnp",
            NatMethod::Direct => "direct",
            NatMethod::HolePunch => "hole_punch",
            NatMethod::Turn => "turn",
        }
    }
}

/// Result of a NAT traversal attempt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatResult {
    Success,
    Failure,
    Timeout,
}

impl NatResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            NatResult::Success => "success",
            NatResult::Failure => "failure",
            NatResult::Timeout => "timeout",
        }
    }
}

/// Statistics for a single NAT traversal method
#[derive(Debug, Clone)]
pub struct NatMethodStats {
    pub total_attempts: u64,
    pub successful_attempts: u64,
    pub failed_attempts: u64,
    pub timeout_attempts: u64,
    pub avg_duration_ms: f64,
    pub success_rate: f64,
}

impl NatMethodStats {
    fn new() -> Self {
        Self {
            total_attempts: 0,
            successful_attempts: 0,
            failed_attempts: 0,
            timeout_attempts: 0,
            avg_duration_ms: 0.0,
            success_rate: 0.0,
        }
    }

    fn record_attempt(&mut self, result: NatResult, duration_ms: f64) {
        self.total_attempts += 1;

        match result {
            NatResult::Success => self.successful_attempts += 1,
            NatResult::Failure => self.failed_attempts += 1,
            NatResult::Timeout => self.timeout_attempts += 1,
        }

        // Update average duration using exponential moving average (EMA)
        // Weight: 70% old, 30% new
        if self.total_attempts == 1 {
            self.avg_duration_ms = duration_ms;
        } else {
            self.avg_duration_ms = 0.7 * self.avg_duration_ms + 0.3 * duration_ms;
        }

        // Calculate success rate
        self.success_rate = self.successful_attempts as f64 / self.total_attempts as f64;
    }
}

/// NAT traversal telemetry collector
pub struct NatTelemetry {
    /// Per-method statistics
    method_stats: Arc<RwLock<HashMap<NatMethod, NatMethodStats>>>,
    
    /// Global counters
    total_connections: Arc<RwLock<u64>>,
    
    /// Method preference order (most successful first)
    method_preference: Arc<RwLock<Vec<NatMethod>>>,
}

impl NatTelemetry {
    /// Create a new NAT telemetry collector
    pub fn new() -> Self {
        let mut stats = HashMap::new();
        stats.insert(NatMethod::Upnp, NatMethodStats::new());
        stats.insert(NatMethod::Direct, NatMethodStats::new());
        stats.insert(NatMethod::HolePunch, NatMethodStats::new());
        stats.insert(NatMethod::Turn, NatMethodStats::new());

        // Default preference order: Direct > UPnP > Hole Punch > TURN
        let default_preference = vec![
            NatMethod::Direct,
            NatMethod::Upnp,
            NatMethod::HolePunch,
            NatMethod::Turn,
        ];

        Self {
            method_stats: Arc::new(RwLock::new(stats)),
            total_connections: Arc::new(RwLock::new(0)),
            method_preference: Arc::new(RwLock::new(default_preference)),
        }
    }

    /// Record a NAT traversal attempt
    pub async fn record_attempt(
        &self,
        method: NatMethod,
        result: NatResult,
        duration: Duration,
    ) -> Result<(), NatTelemetryError> {
        let duration_ms = duration.as_secs_f64() * 1000.0;

        // Update method-specific stats
        {
            let mut stats = self.method_stats.write().await;
            if let Some(method_stats) = stats.get_mut(&method) {
                method_stats.record_attempt(result, duration_ms);
            }
        }

        // Increment total connections on success
        if result == NatResult::Success {
            let mut total = self.total_connections.write().await;
            *total += 1;
        }

        // TODO: Update Prometheus metrics when observability is migrated
        // if let Some(prometheus) = crate::observability::get_prometheus() {
        //     prometheus.record_nat_attempt(method.as_str(), result.as_str());
        //     
        //     // Update success rate gauge
        //     let stats = self.method_stats.read().await;
        //     if let Some(method_stats) = stats.get(&method) {
        //         prometheus.set_nat_success_rate(method.as_str(), method_stats.success_rate);
        //     }
        // }

        // Periodically recompute method preference based on success rates
        self.recompute_preference().await;

        Ok(())
    }

    /// Get statistics for a specific NAT method
    pub async fn get_method_stats(&self, method: NatMethod) -> Option<NatMethodStats> {
        let stats = self.method_stats.read().await;
        stats.get(&method).cloned()
    }

    /// Get statistics for all methods
    pub async fn get_all_stats(&self) -> HashMap<NatMethod, NatMethodStats> {
        let stats = self.method_stats.read().await;
        stats.clone()
    }

    /// Get total successful connections
    pub async fn get_total_connections(&self) -> u64 {
        *self.total_connections.read().await
    }

    /// Get current method preference order (most preferred first)
    pub async fn get_method_preference(&self) -> Vec<NatMethod> {
        self.method_preference.read().await.clone()
    }

    /// Recompute method preference based on success rates
    /// Methods with higher success rates are preferred
    async fn recompute_preference(&self) {
        let stats = self.method_stats.read().await;
        
        // Sort methods by success rate (descending)
        let mut methods: Vec<(NatMethod, f64)> = stats
            .iter()
            .map(|(method, stats)| (*method, stats.success_rate))
            .collect();
        
        methods.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        let mut preference = self.method_preference.write().await;
        *preference = methods.into_iter().map(|(method, _)| method).collect();
    }

    /// Get a summary report of all NAT traversal attempts
    pub async fn get_summary_report(&self) -> String {
        let stats = self.method_stats.read().await;
        let total = *self.total_connections.read().await;
        let preference = self.method_preference.read().await;

        let mut report = String::new();
        report.push_str(&format!("=== NAT Traversal Telemetry Report ===\n"));
        report.push_str(&format!("Total successful connections: {}\n\n", total));

        report.push_str("Method preference order:\n");
        for (i, method) in preference.iter().enumerate() {
            report.push_str(&format!("  {}. {:?}\n", i + 1, method));
        }
        report.push_str("\n");

        for method in &[
            NatMethod::Direct,
            NatMethod::Upnp,
            NatMethod::HolePunch,
            NatMethod::Turn,
        ] {
            if let Some(method_stats) = stats.get(method) {
                report.push_str(&format!("{:?} Statistics:\n", method));
                report.push_str(&format!("  Total attempts: {}\n", method_stats.total_attempts));
                report.push_str(&format!(
                    "  Successful: {} ({:.2}%)\n",
                    method_stats.successful_attempts,
                    method_stats.success_rate * 100.0
                ));
                report.push_str(&format!("  Failed: {}\n", method_stats.failed_attempts));
                report.push_str(&format!("  Timeouts: {}\n", method_stats.timeout_attempts));
                report.push_str(&format!(
                    "  Avg duration: {:.2}ms\n\n",
                    method_stats.avg_duration_ms
                ));
            }
        }

        report
    }
}

impl Default for NatTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global NAT telemetry singleton
static NAT_TELEMETRY: once_cell::sync::OnceCell<Arc<NatTelemetry>> = once_cell::sync::OnceCell::new();

/// Initialize the global NAT telemetry collector
pub fn initialize_nat_telemetry() -> Result<(), NatTelemetryError> {
    let telemetry = NatTelemetry::new();
    NAT_TELEMETRY
        .set(Arc::new(telemetry))
        .map_err(|_| {
            NatTelemetryError::MetricsError("NAT telemetry already initialized".to_string())
        })?;
    Ok(())
}

/// Get the global NAT telemetry collector
pub fn get_nat_telemetry() -> Option<Arc<NatTelemetry>> {
    NAT_TELEMETRY.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_nat_telemetry_creation() {
        let telemetry = NatTelemetry::new();
        let total = telemetry.get_total_connections().await;
        assert_eq!(total, 0, "Initial connections should be zero");
    }

    #[tokio::test]
    async fn test_record_successful_attempt() {
        let telemetry = NatTelemetry::new();

        telemetry
            .record_attempt(NatMethod::Upnp, NatResult::Success, Duration::from_millis(100))
            .await
            .unwrap();

        let stats = telemetry.get_method_stats(NatMethod::Upnp).await.unwrap();
        assert_eq!(stats.total_attempts, 1);
        assert_eq!(stats.successful_attempts, 1);
        assert_eq!(stats.success_rate, 1.0);
        assert!(stats.avg_duration_ms > 0.0);

        let total = telemetry.get_total_connections().await;
        assert_eq!(total, 1);
    }

    #[tokio::test]
    async fn test_record_failed_attempt() {
        let telemetry = NatTelemetry::new();

        telemetry
            .record_attempt(NatMethod::HolePunch, NatResult::Failure, Duration::from_millis(50))
            .await
            .unwrap();

        let stats = telemetry
            .get_method_stats(NatMethod::HolePunch)
            .await
            .unwrap();
        assert_eq!(stats.total_attempts, 1);
        assert_eq!(stats.failed_attempts, 1);
        assert_eq!(stats.success_rate, 0.0);

        let total = telemetry.get_total_connections().await;
        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn test_record_timeout() {
        let telemetry = NatTelemetry::new();

        telemetry
            .record_attempt(NatMethod::Turn, NatResult::Timeout, Duration::from_secs(5))
            .await
            .unwrap();

        let stats = telemetry.get_method_stats(NatMethod::Turn).await.unwrap();
        assert_eq!(stats.total_attempts, 1);
        assert_eq!(stats.timeout_attempts, 1);
        assert_eq!(stats.success_rate, 0.0);
    }

    #[tokio::test]
    async fn test_success_rate_calculation() {
        let telemetry = NatTelemetry::new();

        // Record 3 successes and 1 failure
        telemetry
            .record_attempt(NatMethod::Direct, NatResult::Success, Duration::from_millis(10))
            .await
            .unwrap();
        telemetry
            .record_attempt(NatMethod::Direct, NatResult::Success, Duration::from_millis(15))
            .await
            .unwrap();
        telemetry
            .record_attempt(NatMethod::Direct, NatResult::Success, Duration::from_millis(12))
            .await
            .unwrap();
        telemetry
            .record_attempt(NatMethod::Direct, NatResult::Failure, Duration::from_millis(20))
            .await
            .unwrap();

        let stats = telemetry.get_method_stats(NatMethod::Direct).await.unwrap();
        assert_eq!(stats.total_attempts, 4);
        assert_eq!(stats.successful_attempts, 3);
        assert_eq!(stats.failed_attempts, 1);
        assert_eq!(stats.success_rate, 0.75);
    }

    #[tokio::test]
    async fn test_average_duration_ema() {
        let telemetry = NatTelemetry::new();

        // First attempt: avg should be exactly 100ms
        telemetry
            .record_attempt(NatMethod::Upnp, NatResult::Success, Duration::from_millis(100))
            .await
            .unwrap();

        let stats = telemetry.get_method_stats(NatMethod::Upnp).await.unwrap();
        assert_eq!(stats.avg_duration_ms, 100.0);

        // Second attempt: EMA should apply (70% old + 30% new)
        telemetry
            .record_attempt(NatMethod::Upnp, NatResult::Success, Duration::from_millis(200))
            .await
            .unwrap();

        let stats = telemetry.get_method_stats(NatMethod::Upnp).await.unwrap();
        let expected = 0.7 * 100.0 + 0.3 * 200.0; // = 130.0
        assert!((stats.avg_duration_ms - expected).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_method_preference_ordering() {
        let telemetry = NatTelemetry::new();

        // Record different success rates for each method
        // Direct: 100% (1/1)
        telemetry
            .record_attempt(NatMethod::Direct, NatResult::Success, Duration::from_millis(10))
            .await
            .unwrap();

        // UPnP: 50% (1/2)
        telemetry
            .record_attempt(NatMethod::Upnp, NatResult::Success, Duration::from_millis(50))
            .await
            .unwrap();
        telemetry
            .record_attempt(NatMethod::Upnp, NatResult::Failure, Duration::from_millis(100))
            .await
            .unwrap();

        // Hole Punch: 0% (0/1)
        telemetry
            .record_attempt(
                NatMethod::HolePunch,
                NatResult::Failure,
                Duration::from_millis(200),
            )
            .await
            .unwrap();

        let preference = telemetry.get_method_preference().await;

        // Direct should be first (100% success)
        assert_eq!(preference[0], NatMethod::Direct);

        // UPnP should be second (50% success)
        assert_eq!(preference[1], NatMethod::Upnp);
    }

    #[tokio::test]
    async fn test_get_all_stats() {
        let telemetry = NatTelemetry::new();

        telemetry
            .record_attempt(NatMethod::Direct, NatResult::Success, Duration::from_millis(10))
            .await
            .unwrap();
        telemetry
            .record_attempt(NatMethod::Upnp, NatResult::Failure, Duration::from_millis(50))
            .await
            .unwrap();

        let all_stats = telemetry.get_all_stats().await;
        assert_eq!(all_stats.len(), 4);

        let direct_stats = all_stats.get(&NatMethod::Direct).unwrap();
        assert_eq!(direct_stats.total_attempts, 1);

        let upnp_stats = all_stats.get(&NatMethod::Upnp).unwrap();
        assert_eq!(upnp_stats.total_attempts, 1);
    }

    #[tokio::test]
    async fn test_summary_report() {
        let telemetry = NatTelemetry::new();

        telemetry
            .record_attempt(NatMethod::Direct, NatResult::Success, Duration::from_millis(10))
            .await
            .unwrap();
        telemetry
            .record_attempt(NatMethod::Upnp, NatResult::Success, Duration::from_millis(50))
            .await
            .unwrap();
        telemetry
            .record_attempt(NatMethod::Upnp, NatResult::Failure, Duration::from_millis(100))
            .await
            .unwrap();

        let report = telemetry.get_summary_report().await;

        assert!(report.contains("NAT Traversal Telemetry Report"));
        assert!(report.contains("Total successful connections: 2"));
        assert!(report.contains("Direct Statistics"));
        assert!(report.contains("Upnp Statistics"));
    }

    #[tokio::test]
    async fn test_method_enum_conversions() {
        assert_eq!(NatMethod::Upnp.as_str(), "upnp");
        assert_eq!(NatMethod::Direct.as_str(), "direct");
        assert_eq!(NatMethod::HolePunch.as_str(), "hole_punch");
        assert_eq!(NatMethod::Turn.as_str(), "turn");
    }

    #[tokio::test]
    async fn test_result_enum_conversions() {
        assert_eq!(NatResult::Success.as_str(), "success");
        assert_eq!(NatResult::Failure.as_str(), "failure");
        assert_eq!(NatResult::Timeout.as_str(), "timeout");
    }

    #[test]
    fn test_global_singleton() {
        // Test initialization
        let result = initialize_nat_telemetry();
        // May succeed or fail depending on test execution order
        if result.is_ok() {
            let telemetry = get_nat_telemetry();
            assert!(telemetry.is_some());
        }
    }
}
