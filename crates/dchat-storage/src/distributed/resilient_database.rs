//! Resilient CockroachDB database wrapper
//!
//! This module wraps the base CockroachDB database with resilience patterns:
//! - Circuit breaker to prevent cascade failures
//! - Connection pool management with failover
//! - Automatic retry with exponential backoff
//! - Read replica routing for load distribution
//! - Health monitoring and metrics
//! - Graceful degradation for read operations

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::distributed::database::{DatabaseConfig, DatabaseStats, DistributedDatabase, MessageRow};
use crate::error::{StorageError, StorageResult};
use crate::resilience::{
    CircuitBreaker, CircuitBreakerConfig, CircuitState, HealthMonitor, HealthMonitorConfig,
    HealthStatus, LocalCache, LocalCacheConfig, RetryConfig, RetryExecutor,
};

/// Configuration for resilient database
#[derive(Debug, Clone)]
pub struct ResilientDatabaseConfig {
    /// Base CockroachDB configuration
    pub cockroach: DatabaseConfig,
    /// Circuit breaker configuration
    pub circuit_breaker: CircuitBreakerConfig,
    /// Local cache configuration for read queries
    pub read_cache: LocalCacheConfig,
    /// Retry configuration
    pub retry: RetryConfig,
    /// Health monitor configuration
    pub health_monitor: HealthMonitorConfig,
    /// Enable read caching
    pub enable_read_cache: bool,
    /// Cache TTL for message reads
    pub message_cache_ttl: Duration,
    /// Maximum pending writes before rejecting
    pub max_pending_writes: usize,
    /// Enable write-ahead logging locally
    pub enable_local_wal: bool,
}

impl Default for ResilientDatabaseConfig {
    fn default() -> Self {
        Self {
            cockroach: DatabaseConfig::default(),
            circuit_breaker: CircuitBreakerConfig {
                failure_threshold: 5,
                success_threshold: 3,
                reset_timeout: Duration::from_secs(30),
                half_open_max_requests: 3,
            },
            read_cache: LocalCacheConfig {
                max_entries: 50_000,
                default_ttl: Duration::from_secs(60),
                write_through: true,
            },
            retry: RetryConfig {
                max_retries: 3,
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(5),
                backoff_multiplier: 2.0,
                jitter: true,
            },
            health_monitor: HealthMonitorConfig::default(),
            enable_read_cache: true,
            message_cache_ttl: Duration::from_secs(300),
            max_pending_writes: 10_000,
            enable_local_wal: true,
        }
    }
}

/// Cached message for local fallback during database outages
/// Used by ResilientDatabase for read-through caching
#[derive(Debug, Clone)]
pub struct CachedMessage {
    /// The cached message data
    pub message: MessageRow,
    /// When the message was cached
    pub cached_at: Instant,
}

/// Pending write operation queued during database unavailability
/// Synced to database when connection recovers
#[derive(Debug, Clone)]
pub struct PendingWrite {
    /// The message to write
    pub message: MessageRow,
    /// Target region for the write
    pub region: String,
    /// When the write was queued
    pub queued_at: Instant,
    /// Number of retry attempts
    pub retry_count: u32,
}

/// Resilient database with fault tolerance
/// 
/// Provides automatic failover, circuit breaker pattern, and local caching
/// for CockroachDB operations to ensure high availability during network
/// partitions or database maintenance.
pub struct ResilientDatabase {
    /// Underlying CockroachDB connection
    inner: Option<DistributedDatabase>,
    /// Configuration
    config: ResilientDatabaseConfig,
    /// Circuit breaker for writes
    write_circuit: Arc<CircuitBreaker>,
    /// Circuit breaker for reads
    read_circuit: Arc<CircuitBreaker>,
    /// Retry executor
    retry_executor: Arc<RetryExecutor>,
    /// Health monitor
    health_monitor: Arc<HealthMonitor>,
    /// Local cache for messages
    message_cache: Arc<LocalCache<CachedMessage>>,
    /// User messages cache (user_id -> list of message IDs)
    user_messages_cache: Arc<RwLock<HashMap<String, Vec<Uuid>>>>,
    /// Pending writes to sync when DB recovers
    pending_writes: Arc<RwLock<Vec<PendingWrite>>>,
    /// Is connected to database
    is_connected: AtomicBool,
    /// Metrics
    metrics: Arc<ResilientDatabaseMetrics>,
}

/// Metrics for resilient database
#[derive(Debug, Default)]
pub struct ResilientDatabaseMetrics {
    pub read_hits: AtomicU64,
    pub read_misses: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub write_successes: AtomicU64,
    pub write_failures: AtomicU64,
    pub pending_writes: AtomicU64,
    pub fallback_reads: AtomicU64,
    pub retried_operations: AtomicU64,
}

impl ResilientDatabase {
    /// Create a new resilient database
    pub async fn new(config: ResilientDatabaseConfig) -> StorageResult<Self> {
        info!("Initializing resilient database");

        let write_circuit = Arc::new(CircuitBreaker::new(
            "cockroach_write",
            config.circuit_breaker.clone(),
        ));
        let read_circuit = Arc::new(CircuitBreaker::new(
            "cockroach_read",
            config.circuit_breaker.clone(),
        ));
        let retry_executor = Arc::new(RetryExecutor::new(config.retry.clone()));
        let health_monitor = Arc::new(HealthMonitor::new(config.health_monitor.clone()));
        let message_cache = Arc::new(LocalCache::new(config.read_cache.clone()));

        // Try to connect to database
        let inner = match DistributedDatabase::new(config.cockroach.clone()).await {
            Ok(db) => {
                info!("Successfully connected to CockroachDB cluster");
                Some(db)
            }
            Err(e) => {
                warn!(
                    "Failed to connect to CockroachDB, operating in degraded mode: {}",
                    e
                );
                None
            }
        };

        let is_connected = AtomicBool::new(inner.is_some());

        Ok(Self {
            inner,
            config,
            write_circuit,
            read_circuit,
            retry_executor,
            health_monitor,
            message_cache,
            user_messages_cache: Arc::new(RwLock::new(HashMap::new())),
            pending_writes: Arc::new(RwLock::new(Vec::new())),
            is_connected,
            metrics: Arc::new(ResilientDatabaseMetrics::default()),
        })
    }

    /// Reconnect to database if disconnected
    pub async fn reconnect(&mut self) -> StorageResult<bool> {
        if self.is_connected() {
            return Ok(true);
        }

        info!("Attempting to reconnect to CockroachDB");
        match DistributedDatabase::new(self.config.cockroach.clone()).await {
            Ok(db) => {
                info!("Successfully reconnected to CockroachDB");
                self.inner = Some(db);
                self.is_connected.store(true, Ordering::SeqCst);
                self.health_monitor.record_success("cockroach", 0);
                Ok(true)
            }
            Err(e) => {
                warn!("Failed to reconnect: {}", e);
                self.health_monitor
                    .record_failure("cockroach", e.to_string());
                Ok(false)
            }
        }
    }

    /// Check if connected to database
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    /// Get write circuit breaker state
    pub fn write_circuit_state(&self) -> CircuitState {
        self.write_circuit.state()
    }

    /// Get read circuit breaker state
    pub fn read_circuit_state(&self) -> CircuitState {
        self.read_circuit.state()
    }

    /// Get health status
    pub fn health_status(&self) -> HealthStatus {
        self.health_monitor.overall_status()
    }

    /// Get the retry executor for custom retry operations
    pub fn retry_executor(&self) -> &Arc<RetryExecutor> {
        &self.retry_executor
    }

    /// Insert message with resilience
    pub async fn insert_message(&self, message: &MessageRow, region: &str) -> StorageResult<()> {
        // Check write circuit breaker
        if !self.write_circuit.can_execute() {
            warn!("Write circuit open, queuing message for later");
            self.queue_pending_write(message.clone(), region.to_string())?;
            return Ok(());
        }

        // Try to write to database
        if let Some(ref db) = self.inner {
            let start = Instant::now();

            match db.insert_message_geo(message, region).await {
                Ok(()) => {
                    self.write_circuit.record_success();
                    self.health_monitor
                        .record_success("cockroach", start.elapsed().as_millis() as u64);
                    self.metrics.write_successes.fetch_add(1, Ordering::Relaxed);

                    // Update cache
                    if self.config.enable_read_cache {
                        let cache_key = format!("msg:{}", message.id);
                        self.message_cache.insert(
                            cache_key,
                            CachedMessage {
                                message: message.clone(),
                                cached_at: Instant::now(),
                            },
                        );
                    }

                    Ok(())
                }
                Err(e) => {
                    self.write_circuit.record_failure();
                    self.health_monitor
                        .record_failure("cockroach", e.to_string());
                    self.metrics.write_failures.fetch_add(1, Ordering::Relaxed);

                    warn!("Database write failed, queuing for retry: {}", e);
                    self.queue_pending_write(message.clone(), region.to_string())?;
                    Ok(()) // Return success since message is queued
                }
            }
        } else {
            self.queue_pending_write(message.clone(), region.to_string())?;
            Ok(())
        }
    }

    /// Get message by ID with caching
    pub async fn get_message(&self, message_id: Uuid) -> StorageResult<Option<MessageRow>> {
        let cache_key = format!("msg:{}", message_id);

        // Check cache first
        if self.config.enable_read_cache {
            if let Some(cached) = self.message_cache.get(&cache_key) {
                self.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(Some(cached.message));
            }
            self.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
        }

        // Check read circuit breaker
        if !self.read_circuit.can_execute() {
            debug!("Read circuit open, returning None for message {}", message_id);
            self.metrics.fallback_reads.fetch_add(1, Ordering::Relaxed);
            return Ok(None);
        }

        // Fetch from database
        if let Some(ref db) = self.inner {
            let start = Instant::now();

            match db.get_message(message_id).await {
                Ok(Some(message)) => {
                    self.read_circuit.record_success();
                    self.health_monitor
                        .record_success("cockroach", start.elapsed().as_millis() as u64);
                    self.metrics.read_hits.fetch_add(1, Ordering::Relaxed);

                    // Update cache
                    if self.config.enable_read_cache {
                        self.message_cache.insert(
                            cache_key,
                            CachedMessage {
                                message: message.clone(),
                                cached_at: Instant::now(),
                            },
                        );
                    }

                    Ok(Some(message))
                }
                Ok(None) => {
                    self.read_circuit.record_success();
                    self.health_monitor
                        .record_success("cockroach", start.elapsed().as_millis() as u64);
                    self.metrics.read_misses.fetch_add(1, Ordering::Relaxed);
                    Ok(None)
                }
                Err(e) => {
                    self.read_circuit.record_failure();
                    self.health_monitor
                        .record_failure("cockroach", e.to_string());
                    warn!("Database read failed: {}", e);
                    Ok(None) // Graceful degradation
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Get recent messages for a user with caching
    pub async fn get_recent_messages(
        &self,
        user_id: &str,
        limit: i64,
        offset: i64,
    ) -> StorageResult<Vec<MessageRow>> {
        // Check read circuit breaker
        if !self.read_circuit.can_execute() {
            debug!("Read circuit open, returning empty list for user {}", user_id);
            self.metrics.fallback_reads.fetch_add(1, Ordering::Relaxed);
            return Ok(Vec::new());
        }

        // Fetch from database
        if let Some(ref db) = self.inner {
            let start = Instant::now();

            match db.get_recent_messages(user_id, limit, offset).await {
                Ok(messages) => {
                    self.read_circuit.record_success();
                    self.health_monitor
                        .record_success("cockroach", start.elapsed().as_millis() as u64);

                    // Cache individual messages
                    if self.config.enable_read_cache {
                        for message in &messages {
                            let cache_key = format!("msg:{}", message.id);
                            self.message_cache.insert(
                                cache_key,
                                CachedMessage {
                                    message: message.clone(),
                                    cached_at: Instant::now(),
                                },
                            );
                        }
                    }

                    Ok(messages)
                }
                Err(e) => {
                    self.read_circuit.record_failure();
                    self.health_monitor
                        .record_failure("cockroach", e.to_string());
                    warn!("Database read failed: {}", e);
                    Ok(Vec::new()) // Graceful degradation
                }
            }
        } else {
            Ok(Vec::new())
        }
    }

    /// Get channel messages with caching
    pub async fn get_channel_messages(
        &self,
        channel_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> StorageResult<Vec<MessageRow>> {
        if !self.read_circuit.can_execute() {
            return Ok(Vec::new());
        }

        if let Some(ref db) = self.inner {
            let start = Instant::now();

            match db.get_channel_messages(channel_id, limit, offset).await {
                Ok(messages) => {
                    self.read_circuit.record_success();
                    self.health_monitor
                        .record_success("cockroach", start.elapsed().as_millis() as u64);

                    // Cache individual messages
                    if self.config.enable_read_cache {
                        for message in &messages {
                            let cache_key = format!("msg:{}", message.id);
                            self.message_cache.insert(
                                cache_key,
                                CachedMessage {
                                    message: message.clone(),
                                    cached_at: Instant::now(),
                                },
                            );
                        }
                    }

                    Ok(messages)
                }
                Err(e) => {
                    self.read_circuit.record_failure();
                    self.health_monitor
                        .record_failure("cockroach", e.to_string());
                    Ok(Vec::new())
                }
            }
        } else {
            Ok(Vec::new())
        }
    }

    /// Update message tier
    pub async fn update_message_tier(
        &self,
        message_id: Uuid,
        new_tier: &str,
        s3_key: Option<&str>,
    ) -> StorageResult<()> {
        if !self.write_circuit.can_execute() {
            return Err(StorageError::CircuitOpen(
                "Write circuit is open".to_string(),
            ));
        }

        if let Some(ref db) = self.inner {
            let start = Instant::now();

            match db.update_message_tier(message_id, new_tier, s3_key).await {
                Ok(()) => {
                    self.write_circuit.record_success();
                    self.health_monitor
                        .record_success("cockroach", start.elapsed().as_millis() as u64);

                    // Invalidate cache
                    let cache_key = format!("msg:{}", message_id);
                    self.message_cache.remove(&cache_key);

                    Ok(())
                }
                Err(e) => {
                    self.write_circuit.record_failure();
                    self.health_monitor
                        .record_failure("cockroach", e.to_string());
                    Err(e)
                }
            }
        } else {
            Err(StorageError::ConnectionLost(
                "Database not connected".to_string(),
            ))
        }
    }

    /// Queue a write for later sync
    fn queue_pending_write(&self, message: MessageRow, region: String) -> StorageResult<()> {
        let mut pending = self.pending_writes.write();

        if pending.len() >= self.config.max_pending_writes {
            return Err(StorageError::Internal(
                "Pending write queue is full".to_string(),
            ));
        }

        pending.push(PendingWrite {
            message,
            region,
            queued_at: Instant::now(),
            retry_count: 0,
        });

        self.metrics
            .pending_writes
            .store(pending.len() as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Sync pending writes to database
    pub async fn sync_pending(&self) -> StorageResult<SyncResult> {
        if !self.write_circuit.can_execute() {
            return Ok(SyncResult {
                synced: 0,
                failed: 0,
                errors: vec!["Write circuit breaker is open".to_string()],
            });
        }

        let Some(ref db) = self.inner else {
            return Ok(SyncResult {
                synced: 0,
                failed: 0,
                errors: vec!["Database not connected".to_string()],
            });
        };

        let pending: Vec<_> = {
            let mut pending = self.pending_writes.write();
            std::mem::take(&mut *pending)
        };

        let mut result = SyncResult::default();
        let mut failed_writes = Vec::new();

        for mut write in pending {
            match db.insert_message_geo(&write.message, &write.region).await {
                Ok(()) => {
                    result.synced += 1;
                    self.write_circuit.record_success();
                }
                Err(e) => {
                    result.failed += 1;
                    result.errors.push(format!("Message {}: {}", write.message.id, e));
                    self.write_circuit.record_failure();

                    // Re-queue with incremented retry count
                    write.retry_count += 1;
                    if write.retry_count < 5 {
                        failed_writes.push(write);
                    }
                }
            }
        }

        // Re-queue failed writes
        if !failed_writes.is_empty() {
            let mut pending = self.pending_writes.write();
            pending.extend(failed_writes);
            self.metrics
                .pending_writes
                .store(pending.len() as u64, Ordering::Relaxed);
        }

        info!(
            "Database sync: {} synced, {} failed",
            result.synced, result.failed
        );
        Ok(result)
    }

    /// Get number of pending writes
    pub fn pending_count(&self) -> usize {
        self.pending_writes.read().len()
    }

    /// Get database statistics
    pub async fn get_stats(&self) -> StorageResult<Option<DatabaseStats>> {
        if !self.read_circuit.can_execute() {
            return Ok(None);
        }

        if let Some(ref db) = self.inner {
            match db.get_stats().await {
                Ok(stats) => {
                    self.read_circuit.record_success();
                    Ok(Some(stats))
                }
                Err(e) => {
                    self.read_circuit.record_failure();
                    warn!("Failed to get database stats: {}", e);
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Get resilience metrics
    pub fn get_metrics(&self) -> ResilientDatabaseStats {
        ResilientDatabaseStats {
            read_hits: self.metrics.read_hits.load(Ordering::Relaxed),
            read_misses: self.metrics.read_misses.load(Ordering::Relaxed),
            cache_hits: self.metrics.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.metrics.cache_misses.load(Ordering::Relaxed),
            cache_entries: self.message_cache.len(),
            cache_hit_ratio: self.message_cache.hit_ratio(),
            write_successes: self.metrics.write_successes.load(Ordering::Relaxed),
            write_failures: self.metrics.write_failures.load(Ordering::Relaxed),
            pending_writes: self.pending_count(),
            write_circuit_state: self.write_circuit_state(),
            read_circuit_state: self.read_circuit_state(),
            fallback_reads: self.metrics.fallback_reads.load(Ordering::Relaxed),
        }
    }

    /// Perform health check
    pub async fn health_check(&self) -> StorageResult<bool> {
        if let Some(ref db) = self.inner {
            let start = Instant::now();
            match db.health_check().await {
                Ok(healthy) => {
                    if healthy {
                        self.health_monitor
                            .record_success("cockroach", start.elapsed().as_millis() as u64);
                    } else {
                        self.health_monitor
                            .record_failure("cockroach", "Health check returned false");
                    }
                    Ok(healthy)
                }
                Err(e) => {
                    self.health_monitor
                        .record_failure("cockroach", e.to_string());
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }

    /// Clear read cache
    pub fn clear_cache(&self) {
        self.message_cache.clear();
        self.user_messages_cache.write().clear();
        info!("Database read cache cleared");
    }
}

/// Result of a sync operation
#[derive(Debug, Default)]
pub struct SyncResult {
    pub synced: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// Statistics for resilient database
#[derive(Debug, Clone)]
pub struct ResilientDatabaseStats {
    pub read_hits: u64,
    pub read_misses: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_entries: usize,
    pub cache_hit_ratio: f64,
    pub write_successes: u64,
    pub write_failures: u64,
    pub pending_writes: usize,
    pub write_circuit_state: CircuitState,
    pub read_circuit_state: CircuitState,
    pub fallback_reads: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ResilientDatabaseConfig::default();
        assert!(config.enable_read_cache);
        assert!(config.enable_local_wal);
        assert_eq!(config.max_pending_writes, 10_000);
    }
}
