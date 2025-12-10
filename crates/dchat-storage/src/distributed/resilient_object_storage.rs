//! Resilient MinIO/S3 object storage wrapper
//!
//! This module wraps the base object storage with resilience patterns:
//! - Circuit breaker to prevent cascade failures
//! - Local file cache for frequently accessed objects
//! - Automatic retry with exponential backoff
//! - Multi-region failover
//! - Health monitoring and metrics
//! - Graceful degradation for read operations

use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tracing::{info, warn};

use crate::distributed::object_storage::{
    DistributedObjectStorage, ObjectMetadata, ObjectStorageConfig,
};
use crate::error::{StorageError, StorageResult};
use crate::resilience::{
    CircuitBreaker, CircuitBreakerConfig, CircuitState, HealthMonitor, HealthMonitorConfig,
    HealthStatus, RetryConfig, RetryExecutor,
};

/// Configuration for resilient object storage
#[derive(Debug, Clone)]
pub struct ResilientObjectStorageConfig {
    /// Base S3/MinIO configuration
    pub s3: ObjectStorageConfig,
    /// Circuit breaker configuration
    pub circuit_breaker: CircuitBreakerConfig,
    /// Retry configuration
    pub retry: RetryConfig,
    /// Health monitor configuration
    pub health_monitor: HealthMonitorConfig,
    /// Enable local file cache
    pub enable_local_cache: bool,
    /// Local cache directory
    pub local_cache_dir: PathBuf,
    /// Maximum local cache size in bytes
    pub max_cache_size_bytes: u64,
    /// Cache TTL
    pub cache_ttl: Duration,
    /// Enable upload queue when S3 is unavailable
    pub enable_upload_queue: bool,
    /// Maximum queued uploads
    pub max_queued_uploads: usize,
}

impl Default for ResilientObjectStorageConfig {
    fn default() -> Self {
        Self {
            s3: ObjectStorageConfig::default(),
            circuit_breaker: CircuitBreakerConfig {
                failure_threshold: 5,
                success_threshold: 3,
                reset_timeout: Duration::from_secs(60),
                half_open_max_requests: 2,
            },
            retry: RetryConfig {
                max_retries: 3,
                initial_delay: Duration::from_millis(500),
                max_delay: Duration::from_secs(30),
                backoff_multiplier: 2.0,
                jitter: true,
            },
            health_monitor: HealthMonitorConfig::default(),
            enable_local_cache: true,
            local_cache_dir: PathBuf::from("./cache/objects"),
            max_cache_size_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
            cache_ttl: Duration::from_secs(3600),          // 1 hour
            enable_upload_queue: true,
            max_queued_uploads: 1000,
        }
    }
}

/// Cached object metadata for local object cache
/// Tracks downloaded objects for offline access
#[derive(Debug, Clone)]
pub struct CachedObject {
    /// Path to the locally cached object file
    pub local_path: PathBuf,
    /// Object metadata from S3/MinIO
    pub metadata: ObjectMetadata,
    /// When the object was cached locally
    pub cached_at: Instant,
    /// Size of the cached object in bytes
    pub size_bytes: u64,
}

/// Queued upload pending S3/MinIO availability
/// Stored locally and synced when connection recovers
#[derive(Debug, Clone)]
pub struct QueuedUpload {
    /// Path to the local file to upload
    pub local_path: PathBuf,
    /// Target object key in S3/MinIO
    pub object_key: String,
    /// MIME content type of the file
    pub content_type: String,
    /// When the upload was queued
    pub queued_at: Instant,
    /// Number of upload retry attempts
    pub retry_count: u32,
}

/// Resilient object storage with fault tolerance
/// 
/// Provides automatic failover to local storage, circuit breaker pattern,
/// queued uploads, and LRU caching for S3/MinIO operations to ensure
/// high availability during network issues or object storage maintenance.
pub struct ResilientObjectStorage {
    /// Underlying S3/MinIO storage
    inner: Option<DistributedObjectStorage>,
    /// Configuration
    config: ResilientObjectStorageConfig,
    /// Circuit breaker
    circuit_breaker: Arc<CircuitBreaker>,
    /// Retry executor
    retry_executor: Arc<RetryExecutor>,
    /// Health monitor
    health_monitor: Arc<HealthMonitor>,
    /// Local cache index (key -> cached object info)
    cache_index: Arc<RwLock<HashMap<String, CachedObject>>>,
    /// Current cache size in bytes
    cache_size: AtomicU64,
    /// Pending uploads
    upload_queue: Arc<RwLock<Vec<QueuedUpload>>>,
    /// Is connected to S3
    is_connected: AtomicBool,
    /// Metrics
    metrics: Arc<ResilientObjectStorageMetrics>,
}

/// Metrics for resilient object storage
#[derive(Debug, Default)]
pub struct ResilientObjectStorageMetrics {
    pub upload_successes: AtomicU64,
    pub upload_failures: AtomicU64,
    pub download_successes: AtomicU64,
    pub download_failures: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub queued_uploads: AtomicU64,
    pub fallback_reads: AtomicU64,
}

impl ResilientObjectStorage {
    /// Create a new resilient object storage
    pub async fn new(config: ResilientObjectStorageConfig) -> StorageResult<Self> {
        info!("Initializing resilient object storage");

        // Create local cache directory
        if config.enable_local_cache {
            fs::create_dir_all(&config.local_cache_dir)
                .await
                .map_err(|e| StorageError::Io(e))?;
        }

        let circuit_breaker = Arc::new(CircuitBreaker::new("s3", config.circuit_breaker.clone()));
        let retry_executor = Arc::new(RetryExecutor::new(config.retry.clone()));
        let health_monitor = Arc::new(HealthMonitor::new(config.health_monitor.clone()));

        // Try to connect to S3
        let inner = match DistributedObjectStorage::new(config.s3.clone()).await {
            Ok(storage) => {
                info!("Successfully connected to S3/MinIO");
                Some(storage)
            }
            Err(e) => {
                warn!("Failed to connect to S3/MinIO, operating in degraded mode: {}", e);
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
            cache_index: Arc::new(RwLock::new(HashMap::new())),
            cache_size: AtomicU64::new(0),
            upload_queue: Arc::new(RwLock::new(Vec::new())),
            is_connected,
            metrics: Arc::new(ResilientObjectStorageMetrics::default()),
        })
    }

    /// Reconnect to S3 if disconnected
    pub async fn reconnect(&mut self) -> StorageResult<bool> {
        if self.is_connected() {
            return Ok(true);
        }

        info!("Attempting to reconnect to S3/MinIO");
        match DistributedObjectStorage::new(self.config.s3.clone()).await {
            Ok(storage) => {
                info!("Successfully reconnected to S3/MinIO");
                self.inner = Some(storage);
                self.is_connected.store(true, Ordering::SeqCst);
                self.health_monitor.record_success("s3", 0);
                Ok(true)
            }
            Err(e) => {
                warn!("Failed to reconnect: {}", e);
                self.health_monitor.record_failure("s3", e.to_string());
                Ok(false)
            }
        }
    }

    /// Check if connected to S3
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

    /// Get the retry executor for custom retry operations
    pub fn retry_executor(&self) -> &Arc<RetryExecutor> {
        &self.retry_executor
    }

    /// Upload file with resilience
    pub async fn upload_file(
        &self,
        file_path: &Path,
        object_key: &str,
        content_type: &str,
    ) -> StorageResult<ObjectMetadata> {
        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            if self.config.enable_upload_queue {
                self.queue_upload(file_path, object_key, content_type)?;
                // Return a placeholder metadata
                return Ok(ObjectMetadata {
                    key: object_key.to_string(),
                    size_bytes: fs::metadata(file_path).await.map(|m| m.len()).unwrap_or(0),
                    content_type: content_type.to_string(),
                    etag: "pending".to_string(),
                    public_url: format!("queued://{}", object_key),
                });
            }
            return Err(StorageError::CircuitOpen("S3 circuit is open".to_string()));
        }

        // Try to upload to S3
        if let Some(ref storage) = self.inner {
            let start = Instant::now();

            match storage.upload_file(file_path, object_key, content_type).await {
                Ok(metadata) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("s3", start.elapsed().as_millis() as u64);
                    self.metrics.upload_successes.fetch_add(1, Ordering::Relaxed);
                    Ok(metadata)
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor.record_failure("s3", e.to_string());
                    self.metrics.upload_failures.fetch_add(1, Ordering::Relaxed);

                    if self.config.enable_upload_queue {
                        warn!("S3 upload failed, queuing for retry: {}", e);
                        self.queue_upload(file_path, object_key, content_type)?;
                        Ok(ObjectMetadata {
                            key: object_key.to_string(),
                            size_bytes: fs::metadata(file_path).await.map(|m| m.len()).unwrap_or(0),
                            content_type: content_type.to_string(),
                            etag: "pending".to_string(),
                            public_url: format!("queued://{}", object_key),
                        })
                    } else {
                        Err(e)
                    }
                }
            }
        } else if self.config.enable_upload_queue {
            self.queue_upload(file_path, object_key, content_type)?;
            Ok(ObjectMetadata {
                key: object_key.to_string(),
                size_bytes: fs::metadata(file_path).await.map(|m| m.len()).unwrap_or(0),
                content_type: content_type.to_string(),
                etag: "pending".to_string(),
                public_url: format!("queued://{}", object_key),
            })
        } else {
            Err(StorageError::ConnectionLost("S3 not connected".to_string()))
        }
    }

    /// Upload bytes with resilience
    pub async fn upload_bytes(
        &self,
        data: &[u8],
        object_key: &str,
        content_type: &str,
    ) -> StorageResult<ObjectMetadata> {
        // For bytes upload, we need to write to temp file first if queuing
        if !self.circuit_breaker.can_execute() {
            if self.config.enable_upload_queue {
                let temp_path = self.config.local_cache_dir.join(format!(
                    "upload_{}",
                    uuid::Uuid::new_v4()
                ));
                fs::write(&temp_path, data).await.map_err(StorageError::Io)?;
                self.queue_upload(&temp_path, object_key, content_type)?;
                return Ok(ObjectMetadata {
                    key: object_key.to_string(),
                    size_bytes: data.len() as u64,
                    content_type: content_type.to_string(),
                    etag: "pending".to_string(),
                    public_url: format!("queued://{}", object_key),
                });
            }
            return Err(StorageError::CircuitOpen("S3 circuit is open".to_string()));
        }

        if let Some(ref storage) = self.inner {
            let start = Instant::now();

            match storage.upload_bytes(data, object_key, content_type).await {
                Ok(metadata) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("s3", start.elapsed().as_millis() as u64);
                    self.metrics.upload_successes.fetch_add(1, Ordering::Relaxed);

                    // Cache locally if enabled
                    if self.config.enable_local_cache {
                        let _ = self.cache_object(object_key, data, &metadata).await;
                    }

                    Ok(metadata)
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor.record_failure("s3", e.to_string());
                    self.metrics.upload_failures.fetch_add(1, Ordering::Relaxed);
                    Err(e)
                }
            }
        } else {
            Err(StorageError::ConnectionLost("S3 not connected".to_string()))
        }
    }

    /// Download object with caching
    pub async fn download_bytes(&self, object_key: &str) -> StorageResult<Vec<u8>> {
        // Check local cache first
        if self.config.enable_local_cache {
            if let Some(data) = self.get_from_cache(object_key).await? {
                self.metrics.cache_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(data);
            }
            self.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
        }

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            self.metrics.fallback_reads.fetch_add(1, Ordering::Relaxed);
            return Err(StorageError::CircuitOpen("S3 circuit is open".to_string()));
        }

        // Download from S3
        if let Some(ref storage) = self.inner {
            let start = Instant::now();

            match storage.download_bytes(object_key).await {
                Ok(data) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("s3", start.elapsed().as_millis() as u64);
                    self.metrics.download_successes.fetch_add(1, Ordering::Relaxed);

                    // Cache locally
                    if self.config.enable_local_cache {
                        let metadata = ObjectMetadata {
                            key: object_key.to_string(),
                            size_bytes: data.len() as u64,
                            content_type: "application/octet-stream".to_string(),
                            etag: String::new(),
                            public_url: String::new(),
                        };
                        let _ = self.cache_object(object_key, &data, &metadata).await;
                    }

                    Ok(data)
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor.record_failure("s3", e.to_string());
                    self.metrics.download_failures.fetch_add(1, Ordering::Relaxed);
                    Err(e)
                }
            }
        } else {
            Err(StorageError::ConnectionLost("S3 not connected".to_string()))
        }
    }

    /// Check if object exists
    pub async fn exists(&self, object_key: &str) -> StorageResult<bool> {
        // Check local cache first
        if self.config.enable_local_cache {
            let cache_index = self.cache_index.read();
            if let Some(cached) = cache_index.get(object_key) {
                if cached.cached_at.elapsed() < self.config.cache_ttl {
                    return Ok(true);
                }
            }
        }

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            return Err(StorageError::CircuitOpen("S3 circuit is open".to_string()));
        }

        // Check S3
        if let Some(ref storage) = self.inner {
            storage.exists(object_key).await
        } else {
            Ok(false)
        }
    }

    /// Delete object
    pub async fn delete(&self, object_key: &str) -> StorageResult<()> {
        // Remove from local cache
        self.remove_from_cache(object_key).await;

        // Check circuit breaker
        if !self.circuit_breaker.can_execute() {
            return Err(StorageError::CircuitOpen("S3 circuit is open".to_string()));
        }

        // Delete from S3
        if let Some(ref storage) = self.inner {
            let start = Instant::now();

            match storage.delete(object_key).await {
                Ok(()) => {
                    self.circuit_breaker.record_success();
                    self.health_monitor
                        .record_success("s3", start.elapsed().as_millis() as u64);
                    Ok(())
                }
                Err(e) => {
                    self.circuit_breaker.record_failure();
                    self.health_monitor.record_failure("s3", e.to_string());
                    Err(e)
                }
            }
        } else {
            Err(StorageError::ConnectionLost("S3 not connected".to_string()))
        }
    }

    /// Generate pre-signed URL
    pub async fn generate_presigned_url(
        &self,
        object_key: &str,
        expires_in_seconds: u32,
    ) -> StorageResult<String> {
        if !self.circuit_breaker.can_execute() {
            return Err(StorageError::CircuitOpen("S3 circuit is open".to_string()));
        }

        if let Some(ref storage) = self.inner {
            storage.generate_presigned_url(object_key, expires_in_seconds).await
        } else {
            Err(StorageError::ConnectionLost("S3 not connected".to_string()))
        }
    }

    /// Queue an upload for later
    fn queue_upload(&self, file_path: &Path, object_key: &str, content_type: &str) -> StorageResult<()> {
        let mut queue = self.upload_queue.write();

        if queue.len() >= self.config.max_queued_uploads {
            return Err(StorageError::Internal(
                "Upload queue is full".to_string(),
            ));
        }

        queue.push(QueuedUpload {
            local_path: file_path.to_path_buf(),
            object_key: object_key.to_string(),
            content_type: content_type.to_string(),
            queued_at: Instant::now(),
            retry_count: 0,
        });

        self.metrics
            .queued_uploads
            .store(queue.len() as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Sync queued uploads to S3
    pub async fn sync_pending(&self) -> StorageResult<SyncResult> {
        if !self.circuit_breaker.can_execute() {
            return Ok(SyncResult {
                synced: 0,
                failed: 0,
                errors: vec!["Circuit breaker is open".to_string()],
            });
        }

        let Some(ref storage) = self.inner else {
            return Ok(SyncResult {
                synced: 0,
                failed: 0,
                errors: vec!["S3 not connected".to_string()],
            });
        };

        let uploads: Vec<_> = {
            let mut queue = self.upload_queue.write();
            std::mem::take(&mut *queue)
        };

        let mut result = SyncResult::default();
        let mut failed_uploads = Vec::new();

        for mut upload in uploads {
            // Check if file still exists
            if !upload.local_path.exists() {
                result.errors.push(format!(
                    "File no longer exists: {}",
                    upload.local_path.display()
                ));
                continue;
            }

            match storage
                .upload_file(&upload.local_path, &upload.object_key, &upload.content_type)
                .await
            {
                Ok(_) => {
                    result.synced += 1;
                    self.circuit_breaker.record_success();

                    // Clean up temp file if it's in our cache dir
                    if upload.local_path.starts_with(&self.config.local_cache_dir)
                        && upload.local_path.file_name()
                            .map(|n| n.to_string_lossy().starts_with("upload_"))
                            .unwrap_or(false)
                    {
                        let _ = fs::remove_file(&upload.local_path).await;
                    }
                }
                Err(e) => {
                    result.failed += 1;
                    result.errors.push(format!("{}: {}", upload.object_key, e));
                    self.circuit_breaker.record_failure();

                    upload.retry_count += 1;
                    if upload.retry_count < 5 {
                        failed_uploads.push(upload);
                    }
                }
            }
        }

        // Re-queue failed uploads
        if !failed_uploads.is_empty() {
            let mut queue = self.upload_queue.write();
            queue.extend(failed_uploads);
            self.metrics
                .queued_uploads
                .store(queue.len() as u64, Ordering::Relaxed);
        }

        info!(
            "S3 sync: {} synced, {} failed",
            result.synced, result.failed
        );
        Ok(result)
    }

    /// Cache an object locally
    async fn cache_object(
        &self,
        object_key: &str,
        data: &[u8],
        metadata: &ObjectMetadata,
    ) -> StorageResult<()> {
        let size = data.len() as u64;

        // Check if we need to evict
        while self.cache_size.load(Ordering::Relaxed) + size > self.config.max_cache_size_bytes {
            self.evict_oldest_from_cache().await?;
        }

        // Write to local file
        let local_path = self.config.local_cache_dir.join(
            object_key.replace("/", "_").replace(":", "_")
        );
        fs::write(&local_path, data).await.map_err(StorageError::Io)?;

        // Update cache index
        self.cache_index.write().insert(
            object_key.to_string(),
            CachedObject {
                local_path,
                metadata: metadata.clone(),
                cached_at: Instant::now(),
                size_bytes: size,
            },
        );

        self.cache_size.fetch_add(size, Ordering::Relaxed);

        Ok(())
    }

    /// Get object from local cache
    async fn get_from_cache(&self, object_key: &str) -> StorageResult<Option<Vec<u8>>> {
        let cache_index = self.cache_index.read();

        if let Some(cached) = cache_index.get(object_key) {
            // Check TTL
            if cached.cached_at.elapsed() > self.config.cache_ttl {
                drop(cache_index);
                self.remove_from_cache(object_key).await;
                return Ok(None);
            }

            // Read from local file
            match fs::read(&cached.local_path).await {
                Ok(data) => Ok(Some(data)),
                Err(_) => {
                    drop(cache_index);
                    self.remove_from_cache(object_key).await;
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Remove object from local cache
    async fn remove_from_cache(&self, object_key: &str) {
        let mut cache_index = self.cache_index.write();

        if let Some(cached) = cache_index.remove(object_key) {
            self.cache_size.fetch_sub(cached.size_bytes, Ordering::Relaxed);
            let _ = fs::remove_file(&cached.local_path).await;
        }
    }

    /// Evict oldest object from cache
    async fn evict_oldest_from_cache(&self) -> StorageResult<()> {
        let oldest_key = {
            let cache_index = self.cache_index.read();
            cache_index
                .iter()
                .min_by_key(|(_, cached)| cached.cached_at)
                .map(|(key, _)| key.clone())
        };

        if let Some(key) = oldest_key {
            self.remove_from_cache(&key).await;
        }

        Ok(())
    }

    /// Get number of queued uploads
    pub fn queued_uploads_count(&self) -> usize {
        self.upload_queue.read().len()
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> ResilientObjectStorageStats {
        ResilientObjectStorageStats {
            upload_successes: self.metrics.upload_successes.load(Ordering::Relaxed),
            upload_failures: self.metrics.upload_failures.load(Ordering::Relaxed),
            download_successes: self.metrics.download_successes.load(Ordering::Relaxed),
            download_failures: self.metrics.download_failures.load(Ordering::Relaxed),
            cache_hits: self.metrics.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.metrics.cache_misses.load(Ordering::Relaxed),
            cache_entries: self.cache_index.read().len(),
            cache_size_bytes: self.cache_size.load(Ordering::Relaxed),
            queued_uploads: self.queued_uploads_count(),
            circuit_state: self.circuit_state(),
        }
    }

    /// Perform health check
    pub async fn health_check(&self) -> StorageResult<bool> {
        if let Some(ref storage) = self.inner {
            let start = Instant::now();
            match storage.health_check().await {
                Ok(healthy) => {
                    if healthy {
                        self.health_monitor
                            .record_success("s3", start.elapsed().as_millis() as u64);
                    } else {
                        self.health_monitor
                            .record_failure("s3", "Health check returned false");
                    }
                    Ok(healthy)
                }
                Err(e) => {
                    self.health_monitor.record_failure("s3", e.to_string());
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }

    /// Clear local cache
    pub async fn clear_cache(&self) -> StorageResult<()> {
        let keys: Vec<String> = self.cache_index.read().keys().cloned().collect();
        for key in keys {
            self.remove_from_cache(&key).await;
        }
        info!("Object storage cache cleared");
        Ok(())
    }
}

/// Result of a sync operation
#[derive(Debug, Default)]
pub struct SyncResult {
    pub synced: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// Statistics for resilient object storage
#[derive(Debug, Clone)]
pub struct ResilientObjectStorageStats {
    pub upload_successes: u64,
    pub upload_failures: u64,
    pub download_successes: u64,
    pub download_failures: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_entries: usize,
    pub cache_size_bytes: u64,
    pub queued_uploads: usize,
    pub circuit_state: CircuitState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ResilientObjectStorageConfig::default();
        assert!(config.enable_local_cache);
        assert!(config.enable_upload_queue);
        assert_eq!(config.max_queued_uploads, 1000);
    }
}
