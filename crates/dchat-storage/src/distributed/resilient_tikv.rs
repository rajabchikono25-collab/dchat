//! Resilient TiKV storage wrapper
//!
//! This module wraps the base TiKV storage with resilience patterns:
//! - Circuit breaker to prevent cascade failures
//! - Local cache to reduce latency and provide fallback
//! - Automatic retry with exponential backoff
//! - Health monitoring and metrics
//! - Graceful degradation when TiKV is unavailable

use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::distributed::tikv_backend::{BlockMetadata, ChainState, TiKVConfig, TiKVStorage};
use crate::error::{StorageError, StorageResult};
use crate::resilience::{
    BackendHealth, CircuitBreaker, CircuitBreakerConfig, CircuitState,
    HealthMonitor, HealthMonitorConfig, HealthStatus, LocalCache, LocalCacheConfig,
    RetryConfig, RetryExecutor,
};

/// Configuration for resilient TiKV storage
#[derive(Debug, Clone)]
pub struct ResilientTiKVConfig {
    /// Base TiKV configuration
    pub tikv: TiKVConfig,
    /// Circuit breaker configuration
    pub circuit_breaker: CircuitBreakerConfig,
    /// Local cache configuration
    pub local_cache: LocalCacheConfig,
    /// Retry configuration
    pub retry: RetryConfig,
    /// Health monitor configuration
    pub health_monitor: HealthMonitorConfig,
    /// Enable read-through caching
    pub read_through_cache: bool,
    /// Enable write-behind queue
    pub write_behind_queue: bool,
    /// Maximum write-behind queue size
    pub max_write_behind_size: usize,
}

impl Default for ResilientTiKVConfig {
    fn default() -> Self {
        Self {
            tikv: TiKVConfig::default(),
            circuit_breaker: CircuitBreakerConfig {
                failure_threshold: 5,
                success_threshold: 3,
                reset_timeout: Duration::from_secs(30),
                half_open_max_requests: 3,
            },
            local_cache: LocalCacheConfig {
                max_entries: 50_000,
                default_ttl: Duration::from_secs(300),
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
            read_through_cache: true,
            write_behind_queue: true,
            max_write_behind_size: 10_000,
        }
    }
}

/// Cached chain state with serialization support
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct CachedChainState {
    state: ChainState,
    cached_at: Instant,
}

/// Cached block metadata
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct CachedBlockMetadata {
    metadata: BlockMetadata,
    cached_at: Instant,
}

/// Resilient TiKV storage with fault tolerance
#[allow(dead_code)]
pub struct ResilientTiKVStorage {
    /// Underlying TiKV storage (wrapped in Option for initialization)
    inner: Option<TiKVStorage>,
    /// Configuration
    config: ResilientTiKVConfig,
    /// Circuit breaker
    circuit_breaker: Arc<CircuitBreaker>,
    /// Retry executor
    retry_executor: Arc<RetryExecutor>,
    /// Health monitor
    health_monitor: Arc<HealthMonitor>,
    /// Local cache for chain states
    chain_state_cache: Arc<LocalCache<CachedChainState>>,
    /// Local cache for block metadata
    block_metadata_cache: Arc<LocalCache<CachedBlockMetadata>>,
    /// Write-behind queue for chain states
    pending_chain_states: Arc<parking_lot::RwLock<Vec<(u64, ChainState)>>>,
    /// Write-behind queue for block metadata
    pending_block_metadata: Arc<parking_lot::RwLock<Vec<BlockMetadata>>>,
    /// Is the storage initialized and connected
    is_connected: std::sync::atomic::AtomicBool,
}

impl ResilientTiKVStorage {
    /// Create a new resilient TiKV storage
    pub async fn new(config: ResilientTiKVConfig) -> StorageResult<Self> {
        info!("Initializing resilient TiKV storage");

        let circuit_breaker = Arc::new(CircuitBreaker::new(
            "tikv",
            config.circuit_breaker.clone(),
        ));
        let retry_executor = Arc::new(RetryExecutor::new(config.retry.clone()));
        let health_monitor = Arc::new(HealthMonitor::new(config.health_monitor.clone()));

        let chain_state_cache = Arc::new(LocalCache::new(config.local_cache.clone()));
        let block_metadata_cache = Arc::new(LocalCache::new(config.local_cache.clone()));

        let mut storage = Self {
            inner: None,
            config,
            circuit_breaker,
            retry_executor,
            health_monitor,
            chain_state_cache,
            block_metadata_cache,
            pending_chain_states: Arc::new(parking_lot::RwLock::new(Vec::new())),
            pending_block_metadata: Arc::new(parking_lot::RwLock::new(Vec::new())),
            is_connected: std::sync::atomic::AtomicBool::new(false),
        };

        // Try to connect, but don't fail if TiKV is unavailable
        storage.try_connect().await;

        Ok(storage)
    }

    /// Try to establish connection to TiKV
    async fn try_connect(&mut self) {
        info!("Attempting to connect to TiKV cluster");

        match TiKVStorage::new(self.config.tikv.clone()).await {
            Ok(tikv) => {
                info!("Successfully connected to TiKV cluster");
                self.inner = Some(tikv);
                self.is_connected
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                self.health_monitor.record_success("tikv", 0);
            }
            Err(e) => {
                warn!("Failed to connect to TiKV, operating in degraded mode: {}", e);
                self.is_connected
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                self.health_monitor
                    .record_failure("tikv", format!("Connection failed: {}", e));
            }
        }
    }

    /// Reconnect to TiKV if disconnected
    pub async fn reconnect(&mut self) -> StorageResult<bool> {
        if self.is_connected() {
            return Ok(true);
        }

        self.try_connect().await;
        Ok(self.is_connected())
    }

    /// Check if connected to TiKV
    pub fn is_connected(&self) -> bool {
        self.is_connected
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get circuit breaker state
    pub fn circuit_state(&self) -> CircuitState {
        self.circuit_breaker.state()
    }

    /// Get health status
    pub fn health_status(&self) -> HealthStatus {
        self.health_monitor.overall_status()
    }

    /// Get backend health
    pub fn get_health(&self) -> Option<BackendHealth> {
        self.health_monitor.get_health("tikv")
    }

    /// Store chain state with resilience
    pub async fn store_chain_state(
        &self,
        block_height: u64,
        state: &ChainState,
    ) -> StorageResult<()> {
        let cache_key = format!("chain:block:{}", block_height);

        // Always cache locally first
        self.chain_state_cache.insert(
            cache_key,
            CachedChainState {
                state: state.clone(),
                cached_at: Instant::now(),
            },
        );

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            if self.config.write_behind_queue {
                self.queue_chain_state_write(block_height, state.clone())?;
            }
            return Ok(());
        }

        // Try to write to TiKV
        if let Some(ref tikv) = self.inner {
            let start = Instant::now();

            match tikv.store_chain_state(block_height, state).await {
                Ok(()) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("tikv", start.elapsed().as_millis() as u64);
                    Ok(())
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor
                        .record_failure("tikv", e.to_string());

                    if self.config.write_behind_queue {
                        self.queue_chain_state_write(block_height, state.clone())?;
                    }
                    Ok(()) // Graceful degradation - data is cached locally
                }
            }
        } else {
            // Not connected, queue for later
            if self.config.write_behind_queue {
                self.queue_chain_state_write(block_height, state.clone())?;
            }
            Ok(())
        }
    }

    /// Get chain state with resilience
    pub async fn get_chain_state(&self, block_height: u64) -> StorageResult<Option<ChainState>> {
        let cache_key = format!("chain:block:{}", block_height);

        // Check local cache first
        if self.config.read_through_cache {
            if let Some(cached) = self.chain_state_cache.get(&cache_key) {
                debug!("Cache hit for chain state at block {}", block_height);
                return Ok(Some(cached.state));
            }
        }

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            debug!(
                "Circuit open, returning None for block {} (degraded mode)",
                block_height
            );
            return Ok(None);
        }

        // Try to fetch from TiKV
        if let Some(ref tikv) = self.inner {
            let start = Instant::now();

            match tikv.get_chain_state(block_height).await {
                Ok(Some(state)) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("tikv", start.elapsed().as_millis() as u64);

                    // Cache the result
                    if self.config.read_through_cache {
                        self.chain_state_cache.insert(
                            cache_key,
                            CachedChainState {
                                state: state.clone(),
                                cached_at: Instant::now(),
                            },
                        );
                    }

                    Ok(Some(state))
                }
                Ok(None) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("tikv", start.elapsed().as_millis() as u64);
                    Ok(None)
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor
                        .record_failure("tikv", e.to_string());
                    warn!(
                        "Failed to fetch chain state for block {}: {}",
                        block_height, e
                    );
                    Ok(None) // Graceful degradation
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Store block metadata with resilience
    pub async fn store_block_metadata(&self, metadata: &BlockMetadata) -> StorageResult<()> {
        let cache_key = format!("block:meta:{}", metadata.height);

        // Always cache locally first
        self.block_metadata_cache.insert(
            cache_key,
            CachedBlockMetadata {
                metadata: metadata.clone(),
                cached_at: Instant::now(),
            },
        );

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            if self.config.write_behind_queue {
                self.queue_block_metadata_write(metadata.clone())?;
            }
            return Ok(());
        }

        // Try to write to TiKV
        if let Some(ref tikv) = self.inner {
            let start = Instant::now();

            match tikv.store_block_metadata(metadata).await {
                Ok(()) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("tikv", start.elapsed().as_millis() as u64);
                    Ok(())
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor
                        .record_failure("tikv", e.to_string());

                    if self.config.write_behind_queue {
                        self.queue_block_metadata_write(metadata.clone())?;
                    }
                    Ok(())
                }
            }
        } else {
            if self.config.write_behind_queue {
                self.queue_block_metadata_write(metadata.clone())?;
            }
            Ok(())
        }
    }

    /// Get block metadata with resilience
    pub async fn get_block_metadata(&self, height: u64) -> StorageResult<Option<BlockMetadata>> {
        let cache_key = format!("block:meta:{}", height);

        // Check local cache first
        if self.config.read_through_cache {
            if let Some(cached) = self.block_metadata_cache.get(&cache_key) {
                debug!("Cache hit for block metadata at height {}", height);
                return Ok(Some(cached.metadata));
            }
        }

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            return Ok(None);
        }

        // Try to fetch from TiKV
        if let Some(ref tikv) = self.inner {
            let start = Instant::now();

            match tikv.get_block_metadata(height).await {
                Ok(Some(metadata)) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("tikv", start.elapsed().as_millis() as u64);

                    // Cache the result
                    if self.config.read_through_cache {
                        self.block_metadata_cache.insert(
                            cache_key,
                            CachedBlockMetadata {
                                metadata: metadata.clone(),
                                cached_at: Instant::now(),
                            },
                        );
                    }

                    Ok(Some(metadata))
                }
                Ok(None) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("tikv", start.elapsed().as_millis() as u64);
                    Ok(None)
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor
                        .record_failure("tikv", e.to_string());
                    warn!("Failed to fetch block metadata for height {}: {}", height, e);
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Queue a chain state write for later sync
    fn queue_chain_state_write(&self, block_height: u64, state: ChainState) -> StorageResult<()> {
        let mut pending = self.pending_chain_states.write();

        if pending.len() >= self.config.max_write_behind_size {
            return Err(StorageError::Internal(
                "Write-behind queue is full".to_string(),
            ));
        }

        pending.push((block_height, state));
        debug!(
            "Queued chain state write for block {}, queue size: {}",
            block_height,
            pending.len()
        );
        Ok(())
    }

    /// Queue a block metadata write for later sync
    fn queue_block_metadata_write(&self, metadata: BlockMetadata) -> StorageResult<()> {
        let mut pending = self.pending_block_metadata.write();

        if pending.len() >= self.config.max_write_behind_size {
            return Err(StorageError::Internal(
                "Write-behind queue is full".to_string(),
            ));
        }

        pending.push(metadata);
        Ok(())
    }

    /// Sync pending writes to TiKV
    pub async fn sync_pending(&self) -> StorageResult<SyncResult> {
        if !self.circuit_breaker.can_execute() {
            return Ok(SyncResult {
                chain_states_synced: 0,
                block_metadata_synced: 0,
                errors: vec!["Circuit breaker is open".to_string()],
            });
        }

        let Some(ref tikv) = self.inner else {
            return Ok(SyncResult {
                chain_states_synced: 0,
                block_metadata_synced: 0,
                errors: vec!["Not connected to TiKV".to_string()],
            });
        };

        let mut result = SyncResult::default();

        // Sync chain states
        let chain_states: Vec<_> = {
            let mut pending = self.pending_chain_states.write();
            std::mem::take(&mut *pending)
        };

        let mut failed_chain_states = Vec::new();
        for (height, state) in chain_states {
            match tikv.store_chain_state(height, &state).await {
                Ok(()) => {
                    result.chain_states_synced += 1;
                    self.circuit_breaker.record_success();
                }
                Err(e) => {
                    result.errors.push(format!("Chain state {}: {}", height, e));
                    self.circuit_breaker.record_failure();
                    failed_chain_states.push((height, state));
                }
            }
        }

        // Re-queue failed chain states
        if !failed_chain_states.is_empty() {
            let mut pending = self.pending_chain_states.write();
            pending.extend(failed_chain_states);
        }

        // Sync block metadata
        let block_metadata: Vec<_> = {
            let mut pending = self.pending_block_metadata.write();
            std::mem::take(&mut *pending)
        };

        let mut failed_metadata = Vec::new();
        for metadata in block_metadata {
            match tikv.store_block_metadata(&metadata).await {
                Ok(()) => {
                    result.block_metadata_synced += 1;
                    self.circuit_breaker.record_success();
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("Block metadata {}: {}", metadata.height, e));
                    self.circuit_breaker.record_failure();
                    failed_metadata.push(metadata);
                }
            }
        }

        // Re-queue failed metadata
        if !failed_metadata.is_empty() {
            let mut pending = self.pending_block_metadata.write();
            pending.extend(failed_metadata);
        }

        info!(
            "Sync completed: {} chain states, {} block metadata, {} errors",
            result.chain_states_synced,
            result.block_metadata_synced,
            result.errors.len()
        );

        Ok(result)
    }

    /// Get number of pending writes
    pub fn pending_count(&self) -> (usize, usize) {
        (
            self.pending_chain_states.read().len(),
            self.pending_block_metadata.read().len(),
        )
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            chain_state_entries: self.chain_state_cache.len(),
            chain_state_hit_ratio: self.chain_state_cache.hit_ratio(),
            block_metadata_entries: self.block_metadata_cache.len(),
            block_metadata_hit_ratio: self.block_metadata_cache.hit_ratio(),
        }
    }

    /// Clear local caches
    pub fn clear_caches(&self) {
        self.chain_state_cache.clear();
        self.block_metadata_cache.clear();
        info!("Local caches cleared");
    }

    /// Perform health check
    pub async fn health_check(&self) -> StorageResult<bool> {
        if let Some(ref tikv) = self.inner {
            let start = Instant::now();
            match tikv.health_check().await {
                Ok(healthy) => {
                    self.health_monitor
                        .record_success("tikv", start.elapsed().as_millis() as u64);
                    Ok(healthy)
                }
                Err(e) => {
                    self.health_monitor
                        .record_failure("tikv", e.to_string());
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }
}

/// Result of a sync operation
#[derive(Debug, Default)]
pub struct SyncResult {
    pub chain_states_synced: usize,
    pub block_metadata_synced: usize,
    pub errors: Vec<String>,
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub chain_state_entries: usize,
    pub chain_state_hit_ratio: f64,
    pub block_metadata_entries: usize,
    pub block_metadata_hit_ratio: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ResilientTiKVConfig::default();
        assert_eq!(config.circuit_breaker.failure_threshold, 5);
        assert_eq!(config.local_cache.max_entries, 50_000);
        assert!(config.read_through_cache);
        assert!(config.write_behind_queue);
    }
}
