//! Data lifecycle management and TTL

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// TTL configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtlConfig {
    /// Default TTL for messages
    pub default_message_ttl: Option<Duration>,

    /// TTL for ephemeral messages
    pub ephemeral_ttl: Duration,

    /// TTL for archived messages
    pub archive_ttl: Duration,

    /// Cleanup interval
    pub cleanup_interval: Duration,
}

impl Default for TtlConfig {
    fn default() -> Self {
        Self {
            default_message_ttl: None,                         // No default expiration
            ephemeral_ttl: Duration::from_secs(24 * 3600),     // 24 hours
            archive_ttl: Duration::from_secs(365 * 24 * 3600), // 1 year
            cleanup_interval: Duration::from_secs(3600),       // 1 hour
        }
    }
}

/// Data tier for storage management
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataTier {
    /// Hot storage - frequently accessed
    Hot,

    /// Warm storage - occasionally accessed
    Warm,

    /// Cold storage - archived, rarely accessed
    Cold,
}

/// Lifecycle manager for data
pub struct LifecycleManager {
    config: TtlConfig,

    /// Track access patterns
    access_counts: HashMap<String, usize>,
    last_accessed: HashMap<String, SystemTime>,

    /// Data tier assignments
    tiers: HashMap<String, DataTier>,

    /// Expiration times
    expirations: HashMap<String, SystemTime>,
}

impl LifecycleManager {
    pub fn new(config: TtlConfig) -> Self {
        Self {
            config,
            access_counts: HashMap::new(),
            last_accessed: HashMap::new(),
            tiers: HashMap::new(),
            expirations: HashMap::new(),
        }
    }

    /// Get the TTL configuration
    pub fn config(&self) -> &TtlConfig {
        &self.config
    }

    /// Register data access
    pub fn record_access(&mut self, key: String) {
        *self.access_counts.entry(key.clone()).or_insert(0) += 1;
        self.last_accessed.insert(key, SystemTime::now());
    }

    /// Set expiration for a key
    pub fn set_expiration(&mut self, key: String, expires_at: SystemTime) {
        self.expirations.insert(key, expires_at);
    }

    /// Set expiration with TTL
    pub fn set_ttl(&mut self, key: String, ttl: Duration) {
        let expires_at = SystemTime::now() + ttl;
        self.set_expiration(key, expires_at);
    }

    /// Check if key has expired
    pub fn is_expired(&self, key: &str) -> bool {
        if let Some(expires_at) = self.expirations.get(key) {
            SystemTime::now() > *expires_at
        } else {
            false
        }
    }

    /// Get all expired keys
    pub fn expired_keys(&self) -> Vec<String> {
        let now = SystemTime::now();
        self.expirations
            .iter()
            .filter(|(_, &expires_at)| now > expires_at)
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Update data tier based on access patterns
    pub fn update_tier(&mut self, key: &str) {
        let access_count = self.access_counts.get(key).copied().unwrap_or(0);
        let last_access = self.last_accessed.get(key).copied();

        let tier = if access_count > 100 {
            DataTier::Hot
        } else if let Some(last_access) = last_access {
            let age = SystemTime::now()
                .duration_since(last_access)
                .unwrap_or(Duration::from_secs(0));

            if age < Duration::from_secs(7 * 24 * 3600) {
                DataTier::Warm
            } else {
                DataTier::Cold
            }
        } else {
            DataTier::Cold
        };

        self.tiers.insert(key.to_string(), tier);
    }

    /// Get data tier
    pub fn get_tier(&self, key: &str) -> DataTier {
        self.tiers.get(key).copied().unwrap_or(DataTier::Cold)
    }

    /// Get keys in a specific tier
    pub fn keys_in_tier(&self, tier: DataTier) -> Vec<String> {
        self.tiers
            .iter()
            .filter(|(_, &t)| t == tier)
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Remove expired entries
    pub fn cleanup(&mut self) -> usize {
        let expired = self.expired_keys();
        let count = expired.len();

        for key in expired {
            self.expirations.remove(&key);
            self.access_counts.remove(&key);
            self.last_accessed.remove(&key);
            self.tiers.remove(&key);
        }

        count
    }

    /// Get statistics
    pub fn stats(&self) -> LifecycleStats {
        let hot_count = self.keys_in_tier(DataTier::Hot).len();
        let warm_count = self.keys_in_tier(DataTier::Warm).len();
        let cold_count = self.keys_in_tier(DataTier::Cold).len();
        let expired_count = self.expired_keys().len();

        LifecycleStats {
            hot_count,
            warm_count,
            cold_count,
            expired_count,
            total_tracked: self.expirations.len(),
        }
    }
}

impl Default for LifecycleManager {
    fn default() -> Self {
        Self::new(TtlConfig::default())
    }
}

#[derive(Debug, Clone)]
pub struct LifecycleStats {
    pub hot_count: usize,
    pub warm_count: usize,
    pub cold_count: usize,
    pub expired_count: usize,
    pub total_tracked: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expiration() {
        let mut manager = LifecycleManager::default();

        let key = "test_key".to_string();
        let past = SystemTime::now() - Duration::from_secs(100);

        manager.set_expiration(key.clone(), past);
        assert!(manager.is_expired(&key));

        let expired = manager.expired_keys();
        assert_eq!(expired.len(), 1);
    }

    #[test]
    fn test_tier_management() {
        let mut manager = LifecycleManager::default();

        let key = "test_key";

        // Initial tier should be cold
        assert_eq!(manager.get_tier(key), DataTier::Cold);

        // After many accesses, should be hot
        for _ in 0..150 {
            manager.record_access(key.to_string());
        }
        manager.update_tier(key);
        assert_eq!(manager.get_tier(key), DataTier::Hot);
    }

    #[test]
    fn test_cleanup() {
        let mut manager = LifecycleManager::default();

        let past = SystemTime::now() - Duration::from_secs(100);

        manager.set_expiration("key1".to_string(), past);
        manager.set_expiration("key2".to_string(), past);
        manager.set_expiration(
            "key3".to_string(),
            SystemTime::now() + Duration::from_secs(100),
        );

        let cleaned = manager.cleanup();
        assert_eq!(cleaned, 2);
        assert_eq!(manager.expired_keys().len(), 0);
    }
}
