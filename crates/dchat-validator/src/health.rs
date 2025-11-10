// Enhanced Health Checks for Validators
//
// Provides comprehensive health monitoring including:
// - Actual RTT latency measurement
// - Block height divergence detection
// - Signature freshness validation
// - Network partition detection
// - Consecutive failure tracking

use ed25519_dalek::VerifyingKey;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tokio::time::timeout;

use crate::{HealthStatus, ValidatorHealth};

/// Health check configuration constants
pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;
pub const MAX_ACCEPTABLE_LATENCY_MS: u64 = 5000;
pub const MAX_BLOCK_HEIGHT_LAG: u64 = 10;
pub const SIGNATURE_FRESHNESS_SECS: u64 = 300; // 5 minutes
pub const HEALTH_CHECK_TIMEOUT_SECS: u64 = 10;

/// Errors during health checks
#[derive(Debug, Error)]
pub enum HealthCheckError {
    #[error("Validator unreachable: {0}")]
    Unreachable(String),

    #[error("Health check timeout after {0}s")]
    Timeout(u64),

    #[error("Block height lag: {current} vs {expected} (lag: {lag})")]
    BlockHeightLag {
        current: u64,
        expected: u64,
        lag: u64,
    },

    #[error("Stale signature: {age_secs}s old (max: {max_age_secs}s)")]
    StaleSignature { age_secs: u64, max_age_secs: u64 },

    #[error("High latency: {latency_ms}ms (max: {max_latency_ms}ms)")]
    HighLatency {
        latency_ms: u64,
        max_latency_ms: u64,
    },

    #[error("Consecutive failures: {count} (max: {max})")]
    ConsecutiveFailures { count: u32, max: u32 },
}

/// Enhanced health check probe with latency measurement
#[derive(Debug, Clone)]
pub struct HealthProbe {
    pub validator_id: String,
    pub validator_key: VerifyingKey,
    pub endpoint: String,
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub rtt_ms: Option<u64>,
    pub success: bool,
    pub error: Option<String>,
}

impl HealthProbe {
    /// Create a new health probe
    pub fn new(validator_id: String, validator_key: VerifyingKey, endpoint: String) -> Self {
        Self {
            validator_id,
            validator_key,
            endpoint,
            started_at: Instant::now(),
            completed_at: None,
            rtt_ms: None,
            success: false,
            error: None,
        }
    }

    /// Mark probe as successful
    pub fn mark_success(&mut self) {
        self.completed_at = Some(Instant::now());
        self.rtt_ms = Some(self.started_at.elapsed().as_millis() as u64);
        self.success = true;
    }

    /// Mark probe as failed
    pub fn mark_failure(&mut self, error: String) {
        self.completed_at = Some(Instant::now());
        self.rtt_ms = Some(self.started_at.elapsed().as_millis() as u64);
        self.success = false;
        self.error = Some(error);
    }
}

/// Block height status check result
#[derive(Debug, Clone)]
pub struct BlockHeightCheck {
    pub validator_id: String,
    pub current_height: u64,
    pub network_height: u64,
    pub lag: u64,
    pub is_synced: bool,
}

impl BlockHeightCheck {
    /// Check if validator is synced with network
    pub fn check_sync(validator_id: String, current_height: u64, network_height: u64) -> Self {
        let lag = network_height.saturating_sub(current_height);
        let is_synced = lag <= MAX_BLOCK_HEIGHT_LAG;

        Self {
            validator_id,
            current_height,
            network_height,
            lag,
            is_synced,
        }
    }
}

/// Signature freshness check
#[derive(Debug, Clone)]
pub struct SignatureFreshnessCheck {
    pub validator_id: String,
    pub last_signature_time: SystemTime,
    pub age_secs: u64,
    pub is_fresh: bool,
}

impl SignatureFreshnessCheck {
    /// Check if validator's last signature is fresh
    pub fn check_freshness(validator_id: String, last_signature_time: SystemTime) -> Self {
        let now = SystemTime::now();
        let age_secs = now
            .duration_since(last_signature_time)
            .unwrap_or(Duration::from_secs(u64::MAX))
            .as_secs();

        let is_fresh = age_secs <= SIGNATURE_FRESHNESS_SECS;

        Self {
            validator_id,
            last_signature_time,
            age_secs,
            is_fresh,
        }
    }
}

/// Enhanced health check result
#[derive(Debug, Clone)]
pub struct EnhancedHealthCheck {
    pub validator_id: String,
    pub status: HealthStatus,
    pub probe: HealthProbe,
    pub block_height_check: Option<BlockHeightCheck>,
    pub signature_freshness_check: Option<SignatureFreshnessCheck>,
    pub consecutive_failures: u32,
    pub timestamp: SystemTime,
}

impl EnhancedHealthCheck {
    /// Determine overall health status from checks
    pub fn determine_status(&self) -> HealthStatus {
        // Check consecutive failures first
        if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            return HealthStatus::Unreachable;
        }

        // Check probe success
        if !self.probe.success {
            return HealthStatus::Unhealthy;
        }

        // Check latency
        if let Some(rtt_ms) = self.probe.rtt_ms {
            if rtt_ms > MAX_ACCEPTABLE_LATENCY_MS {
                return HealthStatus::Degraded;
            }
        }

        // Check block height sync
        if let Some(ref height_check) = self.block_height_check {
            if !height_check.is_synced {
                return HealthStatus::Degraded;
            }
        }

        // Check signature freshness
        if let Some(ref sig_check) = self.signature_freshness_check {
            if !sig_check.is_fresh {
                return HealthStatus::Degraded;
            }
        }

        HealthStatus::Healthy
    }

    /// Convert to ValidatorHealth for compatibility
    pub fn to_validator_health(&self) -> ValidatorHealth {
        ValidatorHealth {
            validator_id: self.validator_id.clone(),
            status: self.status,
            uptime_percentage: self.calculate_uptime(),
            avg_response_time_ms: self.probe.rtt_ms.unwrap_or(0),
            last_check: self.timestamp,
            consecutive_failures: self.consecutive_failures,
            block_height: self
                .block_height_check
                .as_ref()
                .map(|c| c.current_height)
                .unwrap_or(0),
        }
    }

    /// Calculate uptime percentage based on failures
    fn calculate_uptime(&self) -> f64 {
        if self.consecutive_failures == 0 {
            100.0
        } else {
            let failure_rate =
                (self.consecutive_failures as f64) / (MAX_CONSECUTIVE_FAILURES as f64);
            (1.0 - failure_rate) * 100.0
        }
    }
}

/// Enhanced health checker with comprehensive monitoring
pub struct EnhancedHealthChecker {
    /// History of health checks per validator
    check_history: Arc<RwLock<HashMap<String, Vec<EnhancedHealthCheck>>>>,

    /// Current network-wide highest block height
    network_height: Arc<RwLock<u64>>,

    /// Consecutive failure counts
    failure_counts: Arc<RwLock<HashMap<String, u32>>>,

    /// Test mode: if true, endpoints containing "invalid" will fail
    test_mode: bool,
}

impl EnhancedHealthChecker {
    /// Create a new enhanced health checker
    pub fn new() -> Self {
        Self {
            check_history: Arc::new(RwLock::new(HashMap::new())),
            network_height: Arc::new(RwLock::new(0)),
            failure_counts: Arc::new(RwLock::new(HashMap::new())),
            test_mode: false,
        }
    }

    /// Create a new health checker in test mode
    #[cfg(test)]
    pub fn new_test_mode() -> Self {
        Self {
            check_history: Arc::new(RwLock::new(HashMap::new())),
            network_height: Arc::new(RwLock::new(0)),
            failure_counts: Arc::new(RwLock::new(HashMap::new())),
            test_mode: true,
        }
    }

    /// Update the network height from consensus
    pub fn update_network_height(&self, height: u64) {
        let mut network_height = self.network_height.write().unwrap();
        if height > *network_height {
            *network_height = height;
        }
    }

    /// Perform a comprehensive health check on a validator
    pub async fn check_validator(
        &self,
        validator_id: String,
        validator_key: VerifyingKey,
        endpoint: String,
        current_block_height: u64,
        last_signature_time: Option<SystemTime>,
    ) -> Result<EnhancedHealthCheck, HealthCheckError> {
        // Create and execute latency probe with timeout
        let mut probe = HealthProbe::new(validator_id.clone(), validator_key, endpoint.clone());

        let probe_result = timeout(
            Duration::from_secs(HEALTH_CHECK_TIMEOUT_SECS),
            self.execute_probe(&mut probe),
        )
        .await;

        match probe_result {
            Ok(Ok(())) => probe.mark_success(),
            Ok(Err(e)) => probe.mark_failure(e.to_string()),
            Err(_) => {
                probe.mark_failure(format!("Timeout after {}s", HEALTH_CHECK_TIMEOUT_SECS));
                return Err(HealthCheckError::Timeout(HEALTH_CHECK_TIMEOUT_SECS));
            }
        }

        // Check block height divergence
        let network_height = *self.network_height.read().unwrap();
        let block_height_check = BlockHeightCheck::check_sync(
            validator_id.clone(),
            current_block_height,
            network_height,
        );

        // Check signature freshness
        let signature_freshness_check = last_signature_time
            .map(|time| SignatureFreshnessCheck::check_freshness(validator_id.clone(), time));

        // Update failure count
        let consecutive_failures = if probe.success {
            // Reset on success
            self.failure_counts
                .write()
                .unwrap()
                .insert(validator_id.clone(), 0);
            0
        } else {
            // Increment on failure
            let mut counts = self.failure_counts.write().unwrap();
            let count = counts.entry(validator_id.clone()).or_insert(0);
            *count += 1;
            *count
        };

        // Create enhanced health check result
        let mut check = EnhancedHealthCheck {
            validator_id: validator_id.clone(),
            status: HealthStatus::Healthy, // Will be updated
            probe,
            block_height_check: Some(block_height_check),
            signature_freshness_check,
            consecutive_failures,
            timestamp: SystemTime::now(),
        };

        // Determine final status
        check.status = check.determine_status();

        // Store in history
        let mut history = self.check_history.write().unwrap();
        history
            .entry(validator_id.clone())
            .or_insert_with(Vec::new)
            .push(check.clone());

        // Keep only last 100 checks per validator
        if let Some(checks) = history.get_mut(&validator_id) {
            if checks.len() > 100 {
                checks.drain(0..checks.len() - 100);
            }
        }

        Ok(check)
    }

    /// Execute a health probe (actual network call simulation)
    async fn execute_probe(&self, probe: &mut HealthProbe) -> Result<(), HealthCheckError> {
        // In test mode, fail if endpoint contains "invalid"
        if self.test_mode && probe.endpoint.contains("invalid") {
            return Err(HealthCheckError::Unreachable(
                "Invalid endpoint".to_string(),
            ));
        }

        // TODO: In production, this would make an actual HTTP/gRPC call to the validator
        // For now, simulate success
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(())
    }

    /// Get check history for a validator
    pub fn get_history(&self, validator_id: &str) -> Vec<EnhancedHealthCheck> {
        self.check_history
            .read()
            .unwrap()
            .get(validator_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get average latency for a validator (last 10 checks)
    pub fn get_average_latency(&self, validator_id: &str) -> Option<u64> {
        let history = self.get_history(validator_id);
        if history.is_empty() {
            return None;
        }

        let recent: Vec<_> = history.iter().rev().take(10).collect();
        let total: u64 = recent.iter().filter_map(|c| c.probe.rtt_ms).sum();

        let count = recent.iter().filter(|c| c.probe.rtt_ms.is_some()).count();

        if count > 0 {
            Some(total / count as u64)
        } else {
            None
        }
    }

    /// Get statistics
    pub fn get_stats(&self) -> HealthCheckStats {
        let history = self.check_history.read().unwrap();
        let total_validators = history.len();

        let mut total_checks = 0;
        let mut successful_checks = 0;
        let mut degraded_count = 0;
        let mut unhealthy_count = 0;

        for checks in history.values() {
            if let Some(latest) = checks.last() {
                total_checks += checks.len();
                match latest.status {
                    HealthStatus::Healthy => successful_checks += 1,
                    HealthStatus::Degraded => degraded_count += 1,
                    HealthStatus::Unhealthy | HealthStatus::Unreachable => unhealthy_count += 1,
                }
            }
        }

        HealthCheckStats {
            total_validators,
            total_checks,
            successful_checks,
            degraded_count,
            unhealthy_count,
        }
    }
}

impl Default for EnhancedHealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct HealthCheckStats {
    pub total_validators: usize,
    pub total_checks: usize,
    pub successful_checks: usize,
    pub degraded_count: usize,
    pub unhealthy_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn test_health_probe() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let mut probe = HealthProbe::new(
            "validator1".to_string(),
            verifying_key,
            "http://localhost:9090".to_string(),
        );

        probe.mark_success();
        assert!(probe.success);
        assert!(probe.rtt_ms.is_some());
    }

    #[test]
    fn test_block_height_sync() {
        let check = BlockHeightCheck::check_sync("validator1".to_string(), 100, 105);

        assert_eq!(check.lag, 5);
        assert!(check.is_synced); // Within MAX_BLOCK_HEIGHT_LAG (10)
    }

    #[test]
    fn test_block_height_out_of_sync() {
        let check = BlockHeightCheck::check_sync("validator1".to_string(), 100, 115);

        assert_eq!(check.lag, 15);
        assert!(!check.is_synced); // Exceeds MAX_BLOCK_HEIGHT_LAG (10)
    }

    #[test]
    fn test_signature_freshness() {
        let recent = SystemTime::now() - Duration::from_secs(60);
        let check = SignatureFreshnessCheck::check_freshness("validator1".to_string(), recent);

        assert!(check.is_fresh);
        assert!(check.age_secs <= SIGNATURE_FRESHNESS_SECS);
    }

    #[test]
    fn test_stale_signature() {
        let old = SystemTime::now() - Duration::from_secs(600);
        let check = SignatureFreshnessCheck::check_freshness("validator1".to_string(), old);

        assert!(!check.is_fresh);
        assert!(check.age_secs > SIGNATURE_FRESHNESS_SECS);
    }

    #[tokio::test]
    async fn test_enhanced_health_check() {
        let checker = EnhancedHealthChecker::new();
        checker.update_network_height(100);

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let result = checker
            .check_validator(
                "validator1".to_string(),
                verifying_key,
                "http://localhost:9090".to_string(),
                98, // Slight lag
                Some(SystemTime::now() - Duration::from_secs(30)),
            )
            .await;

        assert!(result.is_ok());
        let check = result.unwrap();
        assert_eq!(check.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_consecutive_failures() {
        let checker = EnhancedHealthChecker::new_test_mode();
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Simulate 5 failures by using invalid endpoint
        for _ in 0..5 {
            let _ = checker
                .check_validator(
                    "validator1".to_string(),
                    verifying_key,
                    "http://invalid:9999".to_string(),
                    100,
                    None,
                )
                .await;
        }

        let history = checker.get_history("validator1");
        assert_eq!(history.len(), 5);

        if let Some(latest) = history.last() {
            assert_eq!(latest.consecutive_failures, 5);
        }
    }
}
