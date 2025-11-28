// Distributed Redis cache implementation
//
// This module provides a high-performance caching layer using Redis Cluster
// for hot data (recent messages, active users, session data).

use redis::cluster::{ClusterClient, ClusterConnection};
use redis::{Commands, RedisError};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::error::{StorageError, StorageResult};

/// Configuration for distributed cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Redis cluster node URLs
    pub cluster_urls: Vec<String>,
    /// Default TTL for cached items
    pub default_ttl_seconds: u64,
    /// Maximum memory per node
    pub max_memory_mb: u64,
    /// Connection timeout
    pub connection_timeout_seconds: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cluster_urls: vec!["redis://localhost:6379".to_string()],
            default_ttl_seconds: 300, // 5 minutes
            max_memory_mb: 8192,      // 8GB
            connection_timeout_seconds: 5,
        }
    }
}

/// Distributed cache client with Redis Cluster
pub struct DistributedCache {
    /// Redis cluster client
    client: ClusterClient,
    /// Configuration
    config: CacheConfig,
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub memory_used_bytes: u64,
}

impl DistributedCache {
    /// Create new distributed cache connection
    pub fn new(config: CacheConfig) -> StorageResult<Self> {
        info!(
            "Connecting to Redis cluster with {} nodes",
            config.cluster_urls.len()
        );

        let client = ClusterClient::new(config.cluster_urls.clone()).map_err(|e| {
            error!("Failed to create Redis cluster client: {}", e);
            StorageError::Cache(format!("Redis cluster connection failed: {}", e))
        })?;

        info!("Successfully created Redis cluster client");
        Ok(Self { client, config })
    }

    /// Get connection to cluster
    fn get_connection(&self) -> Result<ClusterConnection, RedisError> {
        self.client.get_connection()
    }

    /// Cache a message with default TTL
    pub fn cache_message<T: Serialize>(&self, key: &str, message: &T) -> StorageResult<()> {
        self.cache_message_with_ttl(
            key,
            message,
            Duration::from_secs(self.config.default_ttl_seconds),
        )
    }

    /// Cache a message with custom TTL
    pub fn cache_message_with_ttl<T: Serialize>(
        &self,
        key: &str,
        message: &T,
        ttl: Duration,
    ) -> StorageResult<()> {
        let mut conn = self
            .get_connection()
            .map_err(|e| StorageError::Cache(format!("Failed to get Redis connection: {}", e)))?;

        let serialized = serde_json::to_string(message)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let _: () = redis::cmd("SET")
            .arg(key)
            .arg(serialized)
            .arg("EX")
            .arg(ttl.as_secs() as usize)
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to cache message: {}", e);
                StorageError::Cache(format!("Cache SET failed: {}", e))
            })?;

        debug!("Cached message with key: {}", key);
        Ok(())
    }

    /// Get cached message
    pub fn get_cached_message<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> StorageResult<Option<T>> {
        let mut conn = self
            .get_connection()
            .map_err(|e| StorageError::Cache(format!("Failed to get Redis connection: {}", e)))?;

        let cached: Option<String> = conn.get(key).map_err(|e| {
            error!("Failed to get cached message: {}", e);
            StorageError::Cache(format!("Cache GET failed: {}", e))
        })?;

        match cached {
            Some(json) => {
                debug!("Cache hit for key: {}", key);
                let message = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(message))
            }
            None => {
                debug!("Cache miss for key: {}", key);
                Ok(None)
            }
        }
    }

    /// Invalidate cached message
    pub fn invalidate(&self, key: &str) -> StorageResult<()> {
        let mut conn = self
            .get_connection()
            .map_err(|e| StorageError::Cache(format!("Failed to get Redis connection: {}", e)))?;

        let _: () = conn.del(key).map_err(|e| {
            error!("Failed to invalidate cache: {}", e);
            StorageError::Cache(format!("Cache DEL failed: {}", e))
        })?;

        debug!("Invalidated cache key: {}", key);
        Ok(())
    }

    /// Invalidate multiple keys matching a pattern
    pub fn invalidate_pattern(&self, pattern: &str) -> StorageResult<u64> {
        let mut conn = self
            .get_connection()
            .map_err(|e| StorageError::Cache(format!("Failed to get Redis connection: {}", e)))?;

        // Get all keys matching pattern
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(pattern)
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to scan keys: {}", e);
                StorageError::Cache(format!("Cache KEYS failed: {}", e))
            })?;

        if keys.is_empty() {
            return Ok(0);
        }

        // Delete all matching keys
        let deleted: u64 = conn.del(&keys).map_err(|e| {
            error!("Failed to delete keys: {}", e);
            StorageError::Cache(format!("Cache bulk DEL failed: {}", e))
        })?;

        debug!("Invalidated {} keys matching pattern: {}", deleted, pattern);
        Ok(deleted)
    }

    /// Increment a counter (for rate limiting, statistics)
    pub fn increment_counter(
        &self,
        key: &str,
        delta: i64,
        ttl: Option<Duration>,
    ) -> StorageResult<i64> {
        let mut conn = self
            .get_connection()
            .map_err(|e| StorageError::Cache(format!("Failed to get Redis connection: {}", e)))?;

        let new_value: i64 = conn.incr(key, delta).map_err(|e| {
            error!("Failed to increment counter: {}", e);
            StorageError::Cache(format!("Cache INCR failed: {}", e))
        })?;

        // Set TTL if provided and key is new
        if let Some(ttl) = ttl {
            if new_value == delta {
                let _: () = conn.expire(key, ttl.as_secs() as i64).map_err(|e| {
                    warn!("Failed to set TTL on counter: {}", e);
                    StorageError::Cache(format!("Cache EXPIRE failed: {}", e))
                })?;
            }
        }

        Ok(new_value)
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> StorageResult<CacheStats> {
        let mut conn = self
            .get_connection()
            .map_err(|e| StorageError::Cache(format!("Failed to get Redis connection: {}", e)))?;

        let info: String = redis::cmd("INFO")
            .arg("stats")
            .query(&mut conn)
            .map_err(|e| {
                error!("Failed to get cache stats: {}", e);
                StorageError::Cache(format!("Cache INFO failed: {}", e))
            })?;

        // Parse INFO response
        let mut stats = CacheStats::default();
        for line in info.lines() {
            if line.starts_with("keyspace_hits:") {
                if let Some(value) = line.split(':').nth(1) {
                    stats.hits = value.parse().unwrap_or(0);
                }
            } else if line.starts_with("keyspace_misses:") {
                if let Some(value) = line.split(':').nth(1) {
                    stats.misses = value.parse().unwrap_or(0);
                }
            } else if line.starts_with("evicted_keys:") {
                if let Some(value) = line.split(':').nth(1) {
                    stats.evictions = value.parse().unwrap_or(0);
                }
            }
        }

        // Get memory usage
        let memory_info: String =
            redis::cmd("INFO")
                .arg("memory")
                .query(&mut conn)
                .map_err(|e| {
                    warn!("Failed to get memory info: {}", e);
                    StorageError::Cache(format!("Cache INFO memory failed: {}", e))
                })?;

        for line in memory_info.lines() {
            if line.starts_with("used_memory:") {
                if let Some(value) = line.split(':').nth(1) {
                    stats.memory_used_bytes = value.parse().unwrap_or(0);
                }
            }
        }

        Ok(stats)
    }

    /// Calculate cache hit ratio
    pub fn hit_ratio(&self) -> StorageResult<f64> {
        let stats = self.get_stats()?;
        let total = stats.hits + stats.misses;

        if total == 0 {
            Ok(0.0)
        } else {
            Ok(stats.hits as f64 / total as f64)
        }
    }

    /// Health check - test cache connectivity
    pub fn health_check(&self) -> StorageResult<bool> {
        let mut conn = self.get_connection().map_err(|e| {
            error!("Cache health check failed: {}", e);
            return StorageError::Cache(format!("Health check failed: {}", e));
        })?;

        let pong: String = redis::cmd("PING").query(&mut conn).map_err(|e| {
            error!("Cache PING failed: {}", e);
            StorageError::Cache(format!("PING failed: {}", e))
        })?;

        Ok(pong == "PONG")
    }

    /// Flush all cache (use with caution!)
    pub fn flush_all(&self) -> StorageResult<()> {
        warn!("Flushing all cache data - this is destructive!");

        let mut conn = self
            .get_connection()
            .map_err(|e| StorageError::Cache(format!("Failed to get Redis connection: {}", e)))?;

        let _: () = redis::cmd("FLUSHALL").query(&mut conn).map_err(|e| {
            error!("Failed to flush cache: {}", e);
            StorageError::Cache(format!("Cache FLUSHALL failed: {}", e))
        })?;

        info!("Successfully flushed all cache data");
        Ok(())
    }
}

/// Helper functions for cache key generation
pub mod keys {
    use uuid::Uuid;

    /// Generate cache key for a message
    pub fn message_key(message_id: Uuid) -> String {
        format!("msg:{}", message_id)
    }

    /// Generate cache key for user's recent messages
    pub fn user_messages_key(user_id: &str, page: u32) -> String {
        format!("user:{}:messages:page:{}", user_id, page)
    }

    /// Generate cache key for channel messages
    pub fn channel_messages_key(channel_id: Uuid, page: u32) -> String {
        format!("channel:{}:messages:page:{}", channel_id, page)
    }

    /// Generate cache key for user profile
    pub fn user_profile_key(user_id: &str) -> String {
        format!("user:{}:profile", user_id)
    }

    /// Generate cache key for channel metadata
    pub fn channel_metadata_key(channel_id: Uuid) -> String {
        format!("channel:{}:metadata", channel_id)
    }

    /// Generate cache key for rate limit counter
    pub fn rate_limit_key(user_id: &str, action: &str) -> String {
        format!("ratelimit:{}:{}", user_id, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_cache_key_generation() {
        let message_id = Uuid::new_v4();
        let key = keys::message_key(message_id);
        assert!(key.starts_with("msg:"));

        let user_key = keys::user_messages_key("user123", 1);
        assert_eq!(user_key, "user:user123:messages:page:1");

        let rate_limit_key = keys::rate_limit_key("user456", "send_message");
        assert_eq!(rate_limit_key, "ratelimit:user456:send_message");
    }

    #[test]
    fn test_default_config() {
        let config = CacheConfig::default();
        assert_eq!(config.default_ttl_seconds, 300);
        assert_eq!(config.max_memory_mb, 8192);
    }
}
