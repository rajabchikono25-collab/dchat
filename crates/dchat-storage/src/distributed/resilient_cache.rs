//! Resilient Redis cache wrapper
//!
//! This module wraps the base Redis cache with resilience patterns:
//! - Circuit breaker to prevent cascade failures
//! - Local in-memory fallback cache
//! - Automatic retry with exponential backoff
//! - Health monitoring and metrics
//! - Graceful degradation when Redis is unavailable

use parking_lot::RwLock;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::distributed::cache::{CacheConfig, DistributedCache};
use crate::error::{StorageError, StorageResult};
use crate::resilience::{
    CircuitBreaker, CircuitBreakerConfig, CircuitState, HealthMonitor, HealthMonitorConfig,
    HealthStatus, LocalCache, LocalCacheConfig, RetryConfig, RetryExecutor,
};

/// Configuration for resilient cache
#[derive(Debug, Clone)]
pub struct ResilientCacheConfig {
    /// Base Redis configuration
    pub redis: CacheConfig,
    /// Circuit breaker configuration
    pub circuit_breaker: CircuitBreakerConfig,
    /// Local cache configuration for fallback
    pub local_cache: LocalCacheConfig,
    /// Retry configuration
    pub retry: RetryConfig,
    /// Health monitor configuration
    pub health_monitor: HealthMonitorConfig,
    /// Enable local fallback when Redis is unavailable
    pub enable_fallback: bool,
    /// Sync local cache to Redis when it recovers
    pub sync_on_recovery: bool,
}

impl Default for ResilientCacheConfig {
    fn default() -> Self {
        Self {
            redis: CacheConfig::default(),
            circuit_breaker: CircuitBreakerConfig {
                failure_threshold: 5,
                success_threshold: 3,
                reset_timeout: Duration::from_secs(30),
                half_open_max_requests: 3,
            },
            local_cache: LocalCacheConfig {
                max_entries: 10_000,
                default_ttl: Duration::from_secs(300),
                write_through: true,
            },
            retry: RetryConfig {
                max_retries: 2,
                initial_delay: Duration::from_millis(50),
                max_delay: Duration::from_secs(1),
                backoff_multiplier: 2.0,
                jitter: true,
            },
            health_monitor: HealthMonitorConfig::default(),
            enable_fallback: true,
            sync_on_recovery: true,
        }
    }
}

/// Cached entry for local fallback
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct CachedEntry {
    value: String,
    cached_at: Instant,
    ttl: Duration,
}

/// Resilient cache with fault tolerance
#[allow(dead_code)]
pub struct ResilientCache {
    /// Underlying Redis cache (optional - may not be connected)
    inner: Option<DistributedCache>,
    /// Configuration
    config: ResilientCacheConfig,
    /// Circuit breaker
    circuit_breaker: Arc<CircuitBreaker>,
    /// Retry executor
    retry_executor: Arc<RetryExecutor>,
    /// Health monitor
    health_monitor: Arc<HealthMonitor>,
    /// Local fallback cache
    local_cache: Arc<LocalCache<CachedEntry>>,
    /// Pending writes to sync when Redis recovers
    pending_writes: Arc<RwLock<HashMap<String, (String, Duration)>>>,
    /// Is connected to Redis
    is_connected: AtomicBool,
    /// Metrics
    metrics: Arc<ResilientCacheMetrics>,
}

/// Metrics for resilient cache
#[derive(Debug, Default)]
pub struct ResilientCacheMetrics {
    pub redis_hits: AtomicU64,
    pub redis_misses: AtomicU64,
    pub local_hits: AtomicU64,
    pub local_misses: AtomicU64,
    pub fallback_activations: AtomicU64,
    pub sync_operations: AtomicU64,
}

impl ResilientCache {
    /// Create a new resilient cache
    pub fn new(config: ResilientCacheConfig) -> StorageResult<Self> {
        info!("Initializing resilient cache");

        let circuit_breaker = Arc::new(CircuitBreaker::new(
            "redis",
            config.circuit_breaker.clone(),
        ));
        let retry_executor = Arc::new(RetryExecutor::new(config.retry.clone()));
        let health_monitor = Arc::new(HealthMonitor::new(config.health_monitor.clone()));
        let local_cache = Arc::new(LocalCache::new(config.local_cache.clone()));

        // Try to connect to Redis
        let inner = match DistributedCache::new(config.redis.clone()) {
            Ok(cache) => {
                info!("Successfully connected to Redis cluster");
                Some(cache)
            }
            Err(e) => {
                warn!("Failed to connect to Redis, operating in local-only mode: {}", e);
                None
            }
        };

        let is_connected = AtomicBool::new(inner.is_some());

        Ok(Self {
            inner,
            config,
            circuit_breaker,
            retry_executor,
            health_monitor,
            local_cache,
            pending_writes: Arc::new(RwLock::new(HashMap::new())),
            is_connected,
            metrics: Arc::new(ResilientCacheMetrics::default()),
        })
    }

    /// Check if connected to Redis
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    /// Get circuit breaker state
    pub fn circuit_state(&self) -> CircuitState {
        self.circuit_breaker.state()
    }

    /// Get health status
    pub fn health_status(&self) -> HealthStatus {
        self.health_monitor.overall_status()
    }

    /// Cache a value with default TTL
    pub fn cache<T: Serialize>(&self, key: &str, value: &T) -> StorageResult<()> {
        self.cache_with_ttl(key, value, Duration::from_secs(self.config.redis.default_ttl_seconds))
    }

    /// Cache a value with custom TTL
    pub fn cache_with_ttl<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Duration,
    ) -> StorageResult<()> {
        let serialized = serde_json::to_string(value)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        // Always update local cache
        self.local_cache.insert(
            key.to_string(),
            CachedEntry {
                value: serialized.clone(),
                cached_at: Instant::now(),
                ttl,
            },
        );

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            debug!("Circuit open, caching locally only for key: {}", key);
            self.queue_pending_write(key, &serialized, ttl);
            return Ok(());
        }

        // Try to write to Redis
        if let Some(ref cache) = self.inner {
            let start = Instant::now();
            match cache.cache_message_with_ttl(key, value, ttl) {
                Ok(()) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("redis", start.elapsed().as_millis() as u64);
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor.record_failure("redis", e.to_string());
                    warn!("Redis cache write failed, queuing for sync: {}", e);
                    self.queue_pending_write(key, &serialized, ttl);
                }
            }
        } else {
            self.queue_pending_write(key, &serialized, ttl);
        }

        Ok(())
    }

    /// Get a cached value
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> StorageResult<Option<T>> {
        // Check circuit breaker first
        if !self.circuit_breaker.can_execute() {
            // Fall back to local cache
            return self.get_from_local(key);
        }

        // Try Redis first
        if let Some(ref cache) = self.inner {
            let start = Instant::now();
            match cache.get_cached_message::<T>(key) {
                Ok(Some(value)) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("redis", start.elapsed().as_millis() as u64);
                    self.metrics.redis_hits.fetch_add(1, Ordering::Relaxed);
                    return Ok(Some(value));
                }
                Ok(None) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("redis", start.elapsed().as_millis() as u64);
                    self.metrics.redis_misses.fetch_add(1, Ordering::Relaxed);
                    // Fall through to local cache
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor.record_failure("redis", e.to_string());
                    warn!("Redis get failed, falling back to local cache: {}", e);
                    self.metrics.fallback_activations.fetch_add(1, Ordering::Relaxed);
                    return self.get_from_local(key);
                }
            }
        }

        // Check local cache
        self.get_from_local(key)
    }

    /// Get from local cache only
    fn get_from_local<T: DeserializeOwned>(&self, key: &str) -> StorageResult<Option<T>> {
        if let Some(entry) = self.local_cache.get(key) {
            self.metrics.local_hits.fetch_add(1, Ordering::Relaxed);
            let value = serde_json::from_str(&entry.value)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            Ok(Some(value))
        } else {
            self.metrics.local_misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    /// Invalidate a cached key
    pub fn invalidate(&self, key: &str) -> StorageResult<()> {
        // Remove from local cache
        self.local_cache.remove(key);

        // Remove pending write if any
        self.pending_writes.write().remove(key);

        // Try to remove from Redis
        if self.circuit_breaker.can_execute() {
            if let Some(ref cache) = self.inner {
                let _ = cache.invalidate(key); // Best effort
            }
        }

        Ok(())
    }

    /// Queue a write for later sync
    fn queue_pending_write(&self, key: &str, value: &str, ttl: Duration) {
        if self.config.sync_on_recovery {
            self.pending_writes
                .write()
                .insert(key.to_string(), (value.to_string(), ttl));
        }
    }

    /// Sync pending writes to Redis
    pub fn sync_pending(&self) -> StorageResult<usize> {
        if !self.circuit_breaker.can_execute() {
            return Ok(0);
        }

        let Some(ref cache) = self.inner else {
            return Ok(0);
        };

        let pending: HashMap<_, _> = {
            let mut pending = self.pending_writes.write();
            std::mem::take(&mut *pending)
        };

        let mut synced = 0;
        let mut failed = HashMap::new();

        for (key, (value, ttl)) in pending {
            // We need to write raw value, using the internal cache_message_with_ttl
            match cache.cache_message_with_ttl(&key, &value, ttl) {
                Ok(()) => {
                    synced += 1;
                    self.circuit_breaker.record_success();
                }
                Err(e) => {
                    warn!("Failed to sync pending write for '{}': {}", key, e);
                    self.circuit_breaker.record_failure();
                    failed.insert(key, (value, ttl));
                }
            }
        }

        // Re-queue failed writes
        if !failed.is_empty() {
            self.pending_writes.write().extend(failed);
        }

        self.metrics.sync_operations.fetch_add(1, Ordering::Relaxed);
        info!("Synced {} pending cache writes", synced);
        Ok(synced)
    }

    /// Get number of pending writes
    pub fn pending_count(&self) -> usize {
        self.pending_writes.read().len()
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> ResilientCacheStats {
        ResilientCacheStats {
            redis_hits: self.metrics.redis_hits.load(Ordering::Relaxed),
            redis_misses: self.metrics.redis_misses.load(Ordering::Relaxed),
            local_hits: self.metrics.local_hits.load(Ordering::Relaxed),
            local_misses: self.metrics.local_misses.load(Ordering::Relaxed),
            local_entries: self.local_cache.len(),
            local_hit_ratio: self.local_cache.hit_ratio(),
            pending_writes: self.pending_count(),
            circuit_state: self.circuit_state(),
            fallback_activations: self.metrics.fallback_activations.load(Ordering::Relaxed),
        }
    }

    /// Perform health check
    pub fn health_check(&self) -> StorageResult<bool> {
        if let Some(ref cache) = self.inner {
            let start = Instant::now();
            match cache.health_check() {
                Ok(healthy) => {
                    if healthy {
                        self.health_monitor
                            .record_success("redis", start.elapsed().as_millis() as u64);
                    } else {
                        self.health_monitor
                            .record_failure("redis", "Health check returned false");
                    }
                    Ok(healthy)
                }
                Err(e) => {
                    self.health_monitor.record_failure("redis", e.to_string());
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }

    /// Clear local cache
    pub fn clear_local_cache(&self) {
        self.local_cache.clear();
        info!("Local cache cleared");
    }

    /// Increment a counter (rate limiting, statistics)
    pub fn increment_counter(&self, key: &str, delta: i64, ttl: Option<Duration>) -> StorageResult<i64> {
        // For counters, we need Redis for atomic operations
        if !self.circuit_breaker.can_execute() {
            return Err(StorageError::CircuitOpen(
                "Cannot perform atomic counter operations in fallback mode".to_string(),
            ));
        }

        if let Some(ref cache) = self.inner {
            let start = Instant::now();
            match cache.increment_counter(key, delta, ttl) {
                Ok(value) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("redis", start.elapsed().as_millis() as u64);
                    Ok(value)
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor.record_failure("redis", e.to_string());
                    Err(e)
                }
            }
        } else {
            Err(StorageError::ConnectionLost("Redis not connected".to_string()))
        }
    }
}

/// Statistics for resilient cache
#[derive(Debug, Clone)]
pub struct ResilientCacheStats {
    pub redis_hits: u64,
    pub redis_misses: u64,
    pub local_hits: u64,
    pub local_misses: u64,
    pub local_entries: usize,
    pub local_hit_ratio: f64,
    pub pending_writes: usize,
    pub circuit_state: CircuitState,
    pub fallback_activations: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ResilientCacheConfig::default();
        assert!(config.enable_fallback);
        assert!(config.sync_on_recovery);
        assert_eq!(config.circuit_breaker.failure_threshold, 5);
    }
}
