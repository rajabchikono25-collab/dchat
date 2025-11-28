//! Resilience patterns for distributed storage
//!
//! This module provides fault tolerance and reliability features:
//! - Circuit breaker for preventing cascade failures
//! - Local cache for reducing network latency
//! - Automatic retry with exponential backoff
//! - Graceful degradation when backends are unavailable
//! - Health monitoring and metrics

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::error::{StorageError, StorageResult};

// ============================================================================
// Circuit Breaker
// ============================================================================

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally
    Closed,
    /// Circuit is open, requests fail fast without calling backend
    Open,
    /// Circuit is half-open, allowing limited test requests
    HalfOpen,
}

/// Circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Number of successes in half-open to close circuit
    pub success_threshold: u32,
    /// Time to wait before transitioning from open to half-open
    pub reset_timeout: Duration,
    /// Maximum number of requests allowed in half-open state
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            reset_timeout: Duration::from_secs(30),
            half_open_max_requests: 3,
        }
    }
}

/// Circuit breaker for protecting against cascade failures
pub struct CircuitBreaker {
    name: String,
    config: CircuitBreakerConfig,
    state: RwLock<CircuitState>,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    half_open_requests: AtomicU64,
    last_failure_time: RwLock<Option<Instant>>,
    metrics: Arc<CircuitBreakerMetrics>,
}

/// Metrics for circuit breaker
#[derive(Debug, Default)]
pub struct CircuitBreakerMetrics {
    pub total_requests: AtomicU64,
    pub successful_requests: AtomicU64,
    pub failed_requests: AtomicU64,
    pub rejected_requests: AtomicU64,
    pub state_transitions: AtomicU64,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            name: name.into(),
            config,
            state: RwLock::new(CircuitState::Closed),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            half_open_requests: AtomicU64::new(0),
            last_failure_time: RwLock::new(None),
            metrics: Arc::new(CircuitBreakerMetrics::default()),
        }
    }

    /// Check if the circuit allows the request
    pub fn can_execute(&self) -> bool {
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);

        let state = *self.state.read();

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if we should transition to half-open
                if self.should_attempt_reset() {
                    self.transition_to_half_open();
                    true
                } else {
                    self.metrics.rejected_requests.fetch_add(1, Ordering::Relaxed);
                    false
                }
            }
            CircuitState::HalfOpen => {
                // Allow limited requests in half-open state
                let current = self.half_open_requests.fetch_add(1, Ordering::Relaxed);
                if current < self.config.half_open_max_requests as u64 {
                    true
                } else {
                    self.metrics.rejected_requests.fetch_add(1, Ordering::Relaxed);
                    false
                }
            }
        }
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        self.metrics.successful_requests.fetch_add(1, Ordering::Relaxed);

        let state = *self.state.read();

        match state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::Relaxed);
            }
            CircuitState::HalfOpen => {
                let success_count = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                if success_count >= self.config.success_threshold as u64 {
                    self.transition_to_closed();
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but handle gracefully
                warn!("Success recorded while circuit is open");
            }
        }
    }

    /// Record a failed operation
    pub fn record_failure(&self) {
        self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
        *self.last_failure_time.write() = Some(Instant::now());

        let state = *self.state.read();

        match state {
            CircuitState::Closed => {
                let failure_count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                if failure_count >= self.config.failure_threshold as u64 {
                    self.transition_to_open();
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open immediately opens the circuit
                self.transition_to_open();
            }
            CircuitState::Open => {
                // Already open, nothing to do
            }
        }
    }

    /// Execute a function with circuit breaker protection
    pub async fn execute<F, T, E>(&self, f: F) -> Result<T, StorageError>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        if !self.can_execute() {
            return Err(StorageError::Internal(format!(
                "Circuit breaker '{}' is open, request rejected",
                self.name
            )));
        }

        match f.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(StorageError::Internal(format!(
                    "Operation failed through circuit breaker '{}': {}",
                    self.name, e
                )))
            }
        }
    }

    /// Get current state
    pub fn state(&self) -> CircuitState {
        *self.state.read()
    }

    /// Get metrics
    pub fn metrics(&self) -> &CircuitBreakerMetrics {
        &self.metrics
    }

    /// Check if enough time has passed to attempt reset
    fn should_attempt_reset(&self) -> bool {
        if let Some(last_failure) = *self.last_failure_time.read() {
            last_failure.elapsed() >= self.config.reset_timeout
        } else {
            true
        }
    }

    fn transition_to_open(&self) {
        let mut state = self.state.write();
        if *state != CircuitState::Open {
            info!(
                "Circuit breaker '{}' transitioning to OPEN state",
                self.name
            );
            *state = CircuitState::Open;
            self.metrics.state_transitions.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn transition_to_half_open(&self) {
        let mut state = self.state.write();
        if *state == CircuitState::Open {
            info!(
                "Circuit breaker '{}' transitioning to HALF-OPEN state",
                self.name
            );
            *state = CircuitState::HalfOpen;
            self.success_count.store(0, Ordering::Relaxed);
            self.half_open_requests.store(0, Ordering::Relaxed);
            self.metrics.state_transitions.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn transition_to_closed(&self) {
        let mut state = self.state.write();
        if *state != CircuitState::Closed {
            info!(
                "Circuit breaker '{}' transitioning to CLOSED state",
                self.name
            );
            *state = CircuitState::Closed;
            self.failure_count.store(0, Ordering::Relaxed);
            self.success_count.store(0, Ordering::Relaxed);
            self.metrics.state_transitions.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ============================================================================
// Local Write-Through Cache
// ============================================================================

/// Configuration for local cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCacheConfig {
    /// Maximum number of entries
    pub max_entries: usize,
    /// Default TTL for entries
    pub default_ttl: Duration,
    /// Enable write-through to backend
    pub write_through: bool,
}

impl Default for LocalCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            default_ttl: Duration::from_secs(300), // 5 minutes
            write_through: true,
        }
    }
}

/// A cached entry with metadata
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    created_at: Instant,
    ttl: Duration,
    access_count: u64,
}

impl<T> CacheEntry<T> {
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

/// In-memory LRU cache for reducing network latency
pub struct LocalCache<T: Clone> {
    config: LocalCacheConfig,
    entries: RwLock<HashMap<String, CacheEntry<T>>>,
    metrics: LocalCacheMetrics,
}

/// Metrics for local cache
#[derive(Debug, Default)]
pub struct LocalCacheMetrics {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
    pub inserts: AtomicU64,
}

impl<T: Clone> LocalCache<T> {
    /// Create a new local cache
    pub fn new(config: LocalCacheConfig) -> Self {
        Self {
            config,
            entries: RwLock::new(HashMap::new()),
            metrics: LocalCacheMetrics::default(),
        }
    }

    /// Get an entry from cache
    pub fn get(&self, key: &str) -> Option<T> {
        let entries = self.entries.read();

        if let Some(entry) = entries.get(key) {
            if !entry.is_expired() {
                self.metrics.hits.fetch_add(1, Ordering::Relaxed);
                return Some(entry.value.clone());
            }
        }

        self.metrics.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Insert an entry into cache
    pub fn insert(&self, key: String, value: T) {
        self.insert_with_ttl(key, value, self.config.default_ttl);
    }

    /// Insert an entry with custom TTL
    pub fn insert_with_ttl(&self, key: String, value: T, ttl: Duration) {
        let mut entries = self.entries.write();

        // Evict if at capacity
        if entries.len() >= self.config.max_entries {
            self.evict_lru(&mut entries);
        }

        entries.insert(
            key,
            CacheEntry {
                value,
                created_at: Instant::now(),
                ttl,
                access_count: 0,
            },
        );

        self.metrics.inserts.fetch_add(1, Ordering::Relaxed);
    }

    /// Remove an entry from cache
    pub fn remove(&self, key: &str) -> Option<T> {
        self.entries.write().remove(key).map(|e| e.value)
    }

    /// Clear all entries
    pub fn clear(&self) {
        self.entries.write().clear();
    }

    /// Get cache size
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    /// Get cache hit ratio
    pub fn hit_ratio(&self) -> f64 {
        let hits = self.metrics.hits.load(Ordering::Relaxed);
        let misses = self.metrics.misses.load(Ordering::Relaxed);
        let total = hits + misses;

        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Clean up expired entries
    pub fn cleanup_expired(&self) {
        let mut entries = self.entries.write();
        let initial_len = entries.len();

        entries.retain(|_, entry| !entry.is_expired());

        let evicted = initial_len - entries.len();
        if evicted > 0 {
            self.metrics
                .evictions
                .fetch_add(evicted as u64, Ordering::Relaxed);
            debug!("Cleaned up {} expired cache entries", evicted);
        }
    }

    /// Evict least recently used entry
    fn evict_lru(&self, entries: &mut HashMap<String, CacheEntry<T>>) {
        // Find the oldest entry (simple LRU approximation)
        if let Some(oldest_key) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.created_at)
            .map(|(k, _)| k.clone())
        {
            entries.remove(&oldest_key);
            self.metrics.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ============================================================================
// Retry with Exponential Backoff
// ============================================================================

/// Retry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Add jitter to prevent thundering herd
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

/// Retry executor with exponential backoff
pub struct RetryExecutor {
    config: RetryConfig,
    metrics: RetryMetrics,
}

/// Retry metrics
#[derive(Debug, Default)]
pub struct RetryMetrics {
    pub total_attempts: AtomicU64,
    pub successful_first_try: AtomicU64,
    pub successful_after_retry: AtomicU64,
    pub exhausted_retries: AtomicU64,
}

impl RetryExecutor {
    /// Create a new retry executor
    pub fn new(config: RetryConfig) -> Self {
        Self {
            config,
            metrics: RetryMetrics::default(),
        }
    }

    /// Execute a function with retry logic
    pub async fn execute<F, Fut, T, E>(&self, mut f: F) -> Result<T, StorageError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        let mut attempt = 0;
        let mut delay = self.config.initial_delay;

        loop {
            self.metrics.total_attempts.fetch_add(1, Ordering::Relaxed);
            attempt += 1;

            match f().await {
                Ok(result) => {
                    if attempt == 1 {
                        self.metrics
                            .successful_first_try
                            .fetch_add(1, Ordering::Relaxed);
                    } else {
                        self.metrics
                            .successful_after_retry
                            .fetch_add(1, Ordering::Relaxed);
                        debug!("Operation succeeded after {} attempts", attempt);
                    }
                    return Ok(result);
                }
                Err(e) => {
                    if attempt >= self.config.max_retries {
                        self.metrics.exhausted_retries.fetch_add(1, Ordering::Relaxed);
                        return Err(StorageError::Internal(format!(
                            "Operation failed after {} attempts: {}",
                            attempt, e
                        )));
                    }

                    warn!(
                        "Attempt {} failed, retrying in {:?}: {}",
                        attempt, delay, e
                    );

                    // Apply jitter if enabled
                    let actual_delay = if self.config.jitter {
                        let jitter_factor = 0.5 + rand::random::<f64>() * 0.5;
                        Duration::from_secs_f64(delay.as_secs_f64() * jitter_factor)
                    } else {
                        delay
                    };

                    tokio::time::sleep(actual_delay).await;

                    // Calculate next delay with exponential backoff
                    delay = Duration::from_secs_f64(
                        (delay.as_secs_f64() * self.config.backoff_multiplier)
                            .min(self.config.max_delay.as_secs_f64()),
                    );
                }
            }
        }
    }

    /// Get retry metrics
    pub fn metrics(&self) -> &RetryMetrics {
        &self.metrics
    }
}

// ============================================================================
// Fallback Storage Manager
// ============================================================================

/// Fallback storage for graceful degradation
pub struct FallbackManager<T: Clone> {
    /// Local cache for when remote is unavailable
    local_cache: LocalCache<T>,
    /// Circuit breaker for remote storage
    circuit_breaker: CircuitBreaker,
    /// Retry executor
    retry_executor: RetryExecutor,
    /// Pending writes to sync when remote recovers
    pending_writes: RwLock<Vec<(String, T)>>,
    /// Maximum pending writes before rejecting
    max_pending_writes: usize,
}

impl<T: Clone + Send + Sync + 'static> FallbackManager<T> {
    /// Create a new fallback manager
    pub fn new(
        name: impl Into<String>,
        cache_config: LocalCacheConfig,
        circuit_config: CircuitBreakerConfig,
        retry_config: RetryConfig,
    ) -> Self {
        Self {
            local_cache: LocalCache::new(cache_config),
            circuit_breaker: CircuitBreaker::new(name, circuit_config),
            retry_executor: RetryExecutor::new(retry_config),
            pending_writes: RwLock::new(Vec::new()),
            max_pending_writes: 1000,
        }
    }

    /// Get a value, falling back to local cache if remote fails
    pub async fn get<F, Fut, E>(&self, key: &str, remote_fetch: F) -> StorageResult<Option<T>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Option<T>, E>>,
        E: std::fmt::Display,
    {
        // Try local cache first
        if let Some(value) = self.local_cache.get(key) {
            debug!("Cache hit for key: {}", key);
            return Ok(Some(value));
        }

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            debug!(
                "Circuit open, returning None for key: {} (degraded mode)",
                key
            );
            return Ok(None);
        }

        // Try remote
        match remote_fetch().await {
            Ok(Some(value)) => {
                self.circuit_breaker.record_success();
                // Cache locally
                self.local_cache.insert(key.to_string(), value.clone());
                Ok(Some(value))
            }
            Ok(None) => {
                self.circuit_breaker.record_success();
                Ok(None)
            }
            Err(e) => {
                self.circuit_breaker.record_failure();
                warn!("Remote fetch failed for key '{}': {}", key, e);
                Ok(None) // Graceful degradation
            }
        }
    }

    /// Put a value, queuing for sync if remote is unavailable
    pub async fn put<F, Fut, E>(&self, key: String, value: T, remote_put: F) -> StorageResult<()>
    where
        F: FnOnce(T) -> Fut,
        Fut: std::future::Future<Output = Result<(), E>>,
        E: std::fmt::Display,
    {
        // Always update local cache
        self.local_cache.insert(key.clone(), value.clone());

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            // Queue for later sync
            self.queue_pending_write(key, value)?;
            return Ok(());
        }

        // Try remote
        match remote_put(value.clone()).await {
            Ok(()) => {
                self.circuit_breaker.record_success();
                Ok(())
            }
            Err(e) => {
                self.circuit_breaker.record_failure();
                warn!("Remote put failed, queuing for sync: {}", e);
                self.queue_pending_write(key, value)?;
                Ok(()) // Return success since data is safely cached locally
            }
        }
    }

    /// Queue a write for later synchronization
    fn queue_pending_write(&self, key: String, value: T) -> StorageResult<()> {
        let mut pending = self.pending_writes.write();

        if pending.len() >= self.max_pending_writes {
            return Err(StorageError::Internal(
                "Pending write queue is full, cannot accept more writes".to_string(),
            ));
        }

        pending.push((key, value));
        Ok(())
    }

    /// Sync pending writes to remote
    pub async fn sync_pending<F, Fut, E>(&self, mut remote_put: F) -> StorageResult<usize>
    where
        F: FnMut(String, T) -> Fut,
        Fut: std::future::Future<Output = Result<(), E>>,
        E: std::fmt::Display,
    {
        if !self.circuit_breaker.can_execute() {
            return Ok(0);
        }

        let pending: Vec<_> = {
            let mut pending = self.pending_writes.write();
            std::mem::take(&mut *pending)
        };

        let mut synced = 0;
        let mut failed = Vec::new();

        for (key, value) in pending {
            match remote_put(key.clone(), value.clone()).await {
                Ok(()) => {
                    synced += 1;
                    self.circuit_breaker.record_success();
                }
                Err(e) => {
                    warn!("Failed to sync pending write for '{}': {}", key, e);
                    self.circuit_breaker.record_failure();
                    failed.push((key, value));
                }
            }
        }

        // Re-queue failed writes
        if !failed.is_empty() {
            let mut pending = self.pending_writes.write();
            pending.extend(failed);
        }

        info!("Synced {} pending writes", synced);
        Ok(synced)
    }

    /// Get number of pending writes
    pub fn pending_count(&self) -> usize {
        self.pending_writes.read().len()
    }

    /// Get circuit breaker state
    pub fn circuit_state(&self) -> CircuitState {
        self.circuit_breaker.state()
    }

    /// Get cache hit ratio
    pub fn cache_hit_ratio(&self) -> f64 {
        self.local_cache.hit_ratio()
    }
}

// ============================================================================
// Health Monitor
// ============================================================================

/// Health status for a backend
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Backend health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendHealth {
    pub name: String,
    pub status: HealthStatus,
    pub latency_ms: Option<u64>,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub error_message: Option<String>,
    pub consecutive_failures: u32,
}

/// Health monitor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMonitorConfig {
    /// Interval between health checks
    pub check_interval: Duration,
    /// Timeout for health check requests
    pub check_timeout: Duration,
    /// Number of consecutive failures to mark unhealthy
    pub unhealthy_threshold: u32,
    /// Latency threshold for degraded status (ms)
    pub degraded_latency_ms: u64,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            check_timeout: Duration::from_secs(5),
            unhealthy_threshold: 3,
            degraded_latency_ms: 500,
        }
    }
}

/// Health monitor for storage backends
pub struct HealthMonitor {
    config: HealthMonitorConfig,
    backends: RwLock<HashMap<String, BackendHealth>>,
}

impl HealthMonitor {
    /// Create a new health monitor
    pub fn new(config: HealthMonitorConfig) -> Self {
        Self {
            config,
            backends: RwLock::new(HashMap::new()),
        }
    }

    /// Record a successful health check
    pub fn record_success(&self, name: &str, latency_ms: u64) {
        let mut backends = self.backends.write();

        let status = if latency_ms > self.config.degraded_latency_ms {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        };

        backends.insert(
            name.to_string(),
            BackendHealth {
                name: name.to_string(),
                status,
                latency_ms: Some(latency_ms),
                last_check: chrono::Utc::now(),
                error_message: None,
                consecutive_failures: 0,
            },
        );
    }

    /// Record a failed health check
    pub fn record_failure(&self, name: &str, error: impl Into<String>) {
        let mut backends = self.backends.write();

        let prev_failures = backends
            .get(name)
            .map(|h| h.consecutive_failures)
            .unwrap_or(0);

        let new_failures = prev_failures + 1;
        let status = if new_failures >= self.config.unhealthy_threshold {
            HealthStatus::Unhealthy
        } else {
            HealthStatus::Degraded
        };

        backends.insert(
            name.to_string(),
            BackendHealth {
                name: name.to_string(),
                status,
                latency_ms: None,
                last_check: chrono::Utc::now(),
                error_message: Some(error.into()),
                consecutive_failures: new_failures,
            },
        );
    }

    /// Get health status for all backends
    pub fn get_all_health(&self) -> Vec<BackendHealth> {
        self.backends.read().values().cloned().collect()
    }

    /// Get health status for a specific backend
    pub fn get_health(&self, name: &str) -> Option<BackendHealth> {
        self.backends.read().get(name).cloned()
    }

    /// Check if all backends are healthy
    pub fn all_healthy(&self) -> bool {
        self.backends
            .read()
            .values()
            .all(|h| h.status == HealthStatus::Healthy)
    }

    /// Check if any backend is unhealthy
    pub fn any_unhealthy(&self) -> bool {
        self.backends
            .read()
            .values()
            .any(|h| h.status == HealthStatus::Unhealthy)
    }

    /// Get overall system health
    pub fn overall_status(&self) -> HealthStatus {
        let backends = self.backends.read();

        if backends.is_empty() {
            return HealthStatus::Unknown;
        }

        if backends
            .values()
            .any(|h| h.status == HealthStatus::Unhealthy)
        {
            HealthStatus::Unhealthy
        } else if backends
            .values()
            .any(|h| h.status == HealthStatus::Degraded)
        {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_transitions() {
        let cb = CircuitBreaker::new(
            "test",
            CircuitBreakerConfig {
                failure_threshold: 3,
                success_threshold: 2,
                reset_timeout: Duration::from_millis(100),
                half_open_max_requests: 2,
            },
        );

        // Start closed
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.can_execute());

        // Record failures to open
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_local_cache() {
        let cache: LocalCache<String> = LocalCache::new(LocalCacheConfig {
            max_entries: 100,
            default_ttl: Duration::from_secs(60),
            write_through: true,
        });

        // Insert and get
        cache.insert("key1".to_string(), "value1".to_string());
        assert_eq!(cache.get("key1"), Some("value1".to_string()));

        // Miss
        assert_eq!(cache.get("nonexistent"), None);

        // Hit ratio
        let ratio = cache.hit_ratio();
        assert!(ratio > 0.0);
    }

    #[test]
    fn test_health_monitor() {
        let monitor = HealthMonitor::new(HealthMonitorConfig::default());

        // Record healthy
        monitor.record_success("tikv", 50);
        assert!(monitor.all_healthy());

        // Record degraded latency
        monitor.record_success("redis", 600);
        let redis_health = monitor.get_health("redis").unwrap();
        assert_eq!(redis_health.status, HealthStatus::Degraded);

        // Overall status
        assert_eq!(monitor.overall_status(), HealthStatus::Degraded);
    }
}
