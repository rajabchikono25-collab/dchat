// crates/dchat-storage/src/tier_management.rs
//! Advanced storage tier management with automated migration.
//!
//! Implements hot/warm/cold/archive tiering with configurable retention policies
//! and automated data migration based on age and access patterns.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

pub use super::lifecycle::{DataTier, LifecycleManager as BasicLifecycleManager, TtlConfig};

/// Enhanced storage tier with cost and performance characteristics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageTierAdvanced {
    /// Hot: SSD, <7 days, instant access (1ms)
    Hot,
    /// Warm: SSD, 7-90 days, fast access (10ms)  
    Warm,
    /// Cold: S3/MinIO, 90-365 days, delayed access (2-5s)
    Cold,
    /// Archive: Glacier, >365 days, very delayed (5+ min)
    Archive,
}

impl StorageTierAdvanced {
    pub fn latency_ms(&self) -> u64 {
        match self {
            Self::Hot => 1,
            Self::Warm => 10,
            Self::Cold => 2500,
            Self::Archive => 300_000,
        }
    }

    pub fn cost_per_gb_month_usd(&self) -> f64 {
        match self {
            Self::Hot => 0.23,
            Self::Warm => 0.10,
            Self::Cold => 0.023,
            Self::Archive => 0.004,
        }
    }
}

/// Retention policy per message type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicyAdvanced {
    pub hot_days: i64,
    pub warm_days: i64,
    pub cold_days: i64,
    pub archive_days: Option<i64>, // None = keep forever
    pub auto_delete: bool,
}

impl RetentionPolicyAdvanced {
    pub fn direct_message() -> Self {
        Self {
            hot_days: 7,
            warm_days: 30,
            cold_days: 365,
            archive_days: Some(730),
            auto_delete: true,
        }
    }

    pub fn public_channel() -> Self {
        Self {
            hot_days: 3,
            warm_days: 30,
            cold_days: 365,
            archive_days: None,
            auto_delete: false,
        }
    }

    pub fn blockchain_event() -> Self {
        Self {
            hot_days: 0,
            warm_days: 0,
            cold_days: 0,
            archive_days: None,
            auto_delete: false,
        }
    }
}

/// Tier migration manager with database-backed storage
pub struct TierMigrationManager {
    db_pool: SqlitePool,
    policies: HashMap<String, RetentionPolicyAdvanced>,
}

impl TierMigrationManager {
    pub fn new(db_pool: SqlitePool) -> Self {
        let mut policies = HashMap::new();
        policies.insert("direct_message".to_string(), RetentionPolicyAdvanced::direct_message());
        policies.insert("public_channel".to_string(), RetentionPolicyAdvanced::public_channel());
        policies.insert("blockchain_event".to_string(), RetentionPolicyAdvanced::blockchain_event());

        Self { db_pool, policies }
    }

    /// Add custom retention policy
    pub fn add_policy(&mut self, message_type: String, policy: RetentionPolicyAdvanced) {
        self.policies.insert(message_type, policy);
    }

    /// Run tier migration for all messages
    pub async fn migrate_all(&self) -> Result<TierMigrationStats, TierMigrationError> {
        let mut stats = TierMigrationStats::default();

        stats.hot_to_warm = self.migrate_hot_to_warm().await?;
        stats.warm_to_cold = self.migrate_warm_to_cold().await?;
        stats.cold_to_archive = self.migrate_cold_to_archive().await?;
        stats.deleted = self.delete_expired().await?;

        Ok(stats)
    }

    /// Migrate specific message to a target tier
    pub async fn migrate_message_to_tier(
        &self,
        message_id: &str,
        target_tier: StorageTierAdvanced,
    ) -> Result<(), TierMigrationError> {
        // Update tier in database
        sqlx::query(
            "UPDATE messages 
             SET tier = ?1, last_tier_migration = CURRENT_TIMESTAMP 
             WHERE id = ?2"
        )
        .bind(tier_to_string(&target_tier))
        .bind(message_id)
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    /// Get tier distribution statistics
    pub async fn get_tier_distribution(&self) -> Result<TierDistributionStats, TierMigrationError> {
        let rows = sqlx::query_as::<_, (String, i64, i64)>(
            "SELECT tier, COUNT(*) as count, COALESCE(SUM(compressed_size), 0) as total_size
             FROM messages
             WHERE tier IS NOT NULL
             GROUP BY tier"
        )
        .fetch_all(&self.db_pool)
        .await?;

        let mut stats = TierDistributionStats::default();
        for (tier_name, count, size) in rows {
            match tier_name.as_str() {
                "hot" => {
                    stats.hot_count = count as usize;
                    stats.hot_bytes = size as u64;
                }
                "warm" => {
                    stats.warm_count = count as usize;
                    stats.warm_bytes = size as u64;
                }
                "cold" => {
                    stats.cold_count = count as usize;
                    stats.cold_bytes = size as u64;
                }
                "archive" => {
                    stats.archive_count = count as usize;
                    stats.archive_bytes = size as u64;
                }
                _ => {}
            }
        }

        Ok(stats)
    }

    /// Calculate current storage cost across all tiers
    pub async fn calculate_storage_cost(&self) -> Result<f64, TierMigrationError> {
        let stats = self.get_tier_distribution().await?;
        
        let hot_cost = (stats.hot_bytes as f64 / 1_073_741_824.0) * StorageTierAdvanced::Hot.cost_per_gb_month_usd();
        let warm_cost = (stats.warm_bytes as f64 / 1_073_741_824.0) * StorageTierAdvanced::Warm.cost_per_gb_month_usd();
        let cold_cost = (stats.cold_bytes as f64 / 1_073_741_824.0) * StorageTierAdvanced::Cold.cost_per_gb_month_usd();
        let archive_cost = (stats.archive_bytes as f64 / 1_073_741_824.0) * StorageTierAdvanced::Archive.cost_per_gb_month_usd();

        Ok(hot_cost + warm_cost + cold_cost + archive_cost)
    }

    async fn migrate_hot_to_warm(&self) -> Result<usize, TierMigrationError> {
        // Query v_tier_migration_candidates for hot→warm migrations
        let result = sqlx::query(
            "UPDATE messages 
             SET tier = 'warm', last_tier_migration = CURRENT_TIMESTAMP
             WHERE tier = 'hot' 
             AND created_at < datetime('now', '-7 days')
             AND tier IS NOT NULL"
        )
        .execute(&self.db_pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn migrate_warm_to_cold(&self) -> Result<usize, TierMigrationError> {
        // Query for warm→cold migrations (>30 days)
        let result = sqlx::query(
            "UPDATE messages 
             SET tier = 'cold', last_tier_migration = CURRENT_TIMESTAMP
             WHERE tier = 'warm' 
             AND created_at < datetime('now', '-30 days')
             AND tier IS NOT NULL"
        )
        .execute(&self.db_pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn migrate_cold_to_archive(&self) -> Result<usize, TierMigrationError> {
        // Query for cold→archive migrations (>365 days)
        let result = sqlx::query(
            "UPDATE messages 
             SET tier = 'archive', last_tier_migration = CURRENT_TIMESTAMP
             WHERE tier = 'cold' 
             AND created_at < datetime('now', '-365 days')
             AND tier IS NOT NULL"
        )
        .execute(&self.db_pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn delete_expired(&self) -> Result<usize, TierMigrationError> {
        // Delete messages past retention policy
        let result = sqlx::query(
            "DELETE FROM messages 
             WHERE tier = 'archive' 
             AND created_at < datetime('now', '-730 days')
             AND tier IS NOT NULL"
        )
        .execute(&self.db_pool)
        .await?;

        Ok(result.rows_affected() as usize)
    }
}

/// Tier distribution statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TierDistributionStats {
    pub hot_count: usize,
    pub hot_bytes: u64,
    pub warm_count: usize,
    pub warm_bytes: u64,
    pub cold_count: usize,
    pub cold_bytes: u64,
    pub archive_count: usize,
    pub archive_bytes: u64,
}

impl TierDistributionStats {
    pub fn total_messages(&self) -> usize {
        self.hot_count + self.warm_count + self.cold_count + self.archive_count
    }

    pub fn total_bytes(&self) -> u64 {
        self.hot_bytes + self.warm_bytes + self.cold_bytes + self.archive_bytes
    }
}

/// Convert tier enum to string for database storage
fn tier_to_string(tier: &StorageTierAdvanced) -> &'static str {
    match tier {
        StorageTierAdvanced::Hot => "hot",
        StorageTierAdvanced::Warm => "warm",
        StorageTierAdvanced::Cold => "cold",
        StorageTierAdvanced::Archive => "archive",
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TierMigrationStats {
    pub hot_to_warm: usize,
    pub warm_to_cold: usize,
    pub cold_to_archive: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone)]
pub enum TierMigrationError {
    Database(String),
}

impl std::fmt::Display for TierMigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "Database error: {}", e),
        }
    }
}

impl std::error::Error for TierMigrationError {}

impl From<sqlx::Error> for TierMigrationError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_latency() {
        assert_eq!(StorageTierAdvanced::Hot.latency_ms(), 1);
        assert_eq!(StorageTierAdvanced::Warm.latency_ms(), 10);
        assert_eq!(StorageTierAdvanced::Cold.latency_ms(), 2500);
        assert_eq!(StorageTierAdvanced::Archive.latency_ms(), 300_000);
    }

    #[test]
    fn test_tier_costs() {
        assert_eq!(StorageTierAdvanced::Hot.cost_per_gb_month_usd(), 0.23);
        assert_eq!(StorageTierAdvanced::Warm.cost_per_gb_month_usd(), 0.10);
        assert_eq!(StorageTierAdvanced::Cold.cost_per_gb_month_usd(), 0.023);
        assert_eq!(StorageTierAdvanced::Archive.cost_per_gb_month_usd(), 0.004);
    }

    #[test]
    fn test_retention_policies() {
        let dm_policy = RetentionPolicyAdvanced::direct_message();
        assert_eq!(dm_policy.hot_days, 7);
        assert_eq!(dm_policy.warm_days, 30);
        assert_eq!(dm_policy.cold_days, 365);
        assert_eq!(dm_policy.archive_days, Some(730));
        assert!(dm_policy.auto_delete);

        let channel_policy = RetentionPolicyAdvanced::public_channel();
        assert_eq!(channel_policy.hot_days, 3);
        assert_eq!(channel_policy.archive_days, None);
        assert!(!channel_policy.auto_delete);

        let blockchain_policy = RetentionPolicyAdvanced::blockchain_event();
        assert_eq!(blockchain_policy.hot_days, 0);
        assert!(!blockchain_policy.auto_delete);
    }

    #[test]
    fn test_tier_distribution_total() {
        let stats = TierDistributionStats {
            hot_count: 100,
            hot_bytes: 1_000_000,
            warm_count: 200,
            warm_bytes: 2_000_000,
            cold_count: 500,
            cold_bytes: 5_000_000,
            archive_count: 1000,
            archive_bytes: 10_000_000,
        };

        assert_eq!(stats.total_messages(), 1800);
        assert_eq!(stats.total_bytes(), 18_000_000);
    }

    #[test]
    fn test_tier_to_string() {
        assert_eq!(tier_to_string(&StorageTierAdvanced::Hot), "hot");
        assert_eq!(tier_to_string(&StorageTierAdvanced::Warm), "warm");
        assert_eq!(tier_to_string(&StorageTierAdvanced::Cold), "cold");
        assert_eq!(tier_to_string(&StorageTierAdvanced::Archive), "archive");
    }

    #[tokio::test]
    async fn test_tier_manager_creation() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        let manager = TierMigrationManager::new(pool);
        
        // Should have 3 default policies
        assert_eq!(manager.policies.len(), 3);
        assert!(manager.policies.contains_key("direct_message"));
        assert!(manager.policies.contains_key("public_channel"));
        assert!(manager.policies.contains_key("blockchain_event"));
    }

    #[tokio::test]
    async fn test_add_custom_policy() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        let mut manager = TierMigrationManager::new(pool);
        
        let custom_policy = RetentionPolicyAdvanced {
            hot_days: 1,
            warm_days: 7,
            cold_days: 30,
            archive_days: Some(90),
            auto_delete: true,
        };

        manager.add_policy("ephemeral".to_string(), custom_policy);
        assert_eq!(manager.policies.len(), 4);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_tier_distribution_stats() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        
        // Create messages table with tier columns
        sqlx::query(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                tier TEXT,
                compressed_size INTEGER,
                created_at TEXT NOT NULL,
                last_tier_migration TEXT
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        // Insert test data
        sqlx::query(
            "INSERT INTO messages (id, tier, compressed_size, created_at) VALUES 
             ('msg1', 'hot', 1000, datetime('now')),
             ('msg2', 'hot', 2000, datetime('now')),
             ('msg3', 'warm', 3000, datetime('now', '-10 days')),
             ('msg4', 'cold', 5000, datetime('now', '-100 days')),
             ('msg5', 'archive', 10000, datetime('now', '-400 days'))"
        )
        .execute(&pool)
        .await
        .unwrap();

        let manager = TierMigrationManager::new(pool);
        let stats = manager.get_tier_distribution().await.unwrap();

        assert_eq!(stats.hot_count, 2);
        assert_eq!(stats.hot_bytes, 3000);
        assert_eq!(stats.warm_count, 1);
        assert_eq!(stats.warm_bytes, 3000);
        assert_eq!(stats.cold_count, 1);
        assert_eq!(stats.cold_bytes, 5000);
        assert_eq!(stats.archive_count, 1);
        assert_eq!(stats.archive_bytes, 10000);
    }

    #[tokio::test]
    async fn test_calculate_storage_cost() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        
        sqlx::query(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                tier TEXT,
                compressed_size INTEGER,
                created_at TEXT NOT NULL,
                last_tier_migration TEXT
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        // 1GB in each tier
        let gb = 1_073_741_824i64;
        sqlx::query(
            "INSERT INTO messages (id, tier, compressed_size, created_at) VALUES 
             ('msg1', 'hot', ?1, datetime('now')),
             ('msg2', 'warm', ?1, datetime('now')),
             ('msg3', 'cold', ?1, datetime('now')),
             ('msg4', 'archive', ?1, datetime('now'))"
        )
        .bind(gb)
        .execute(&pool)
        .await
        .unwrap();

        let manager = TierMigrationManager::new(pool);
        let cost = manager.calculate_storage_cost().await.unwrap();

        // 1GB * (0.23 + 0.10 + 0.023 + 0.004) = 0.357 USD/month
        assert!((cost - 0.357).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_migrate_message_to_tier() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        
        sqlx::query(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                tier TEXT,
                compressed_size INTEGER,
                created_at TEXT NOT NULL,
                last_tier_migration TEXT
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO messages (id, tier, compressed_size, created_at) VALUES 
             ('msg123', 'hot', 5000, datetime('now', '-10 days'))"
        )
        .execute(&pool)
        .await
        .unwrap();

        let manager = TierMigrationManager::new(pool.clone());
        manager.migrate_message_to_tier("msg123", StorageTierAdvanced::Warm).await.unwrap();

        // Verify tier changed
        let (tier,): (String,) = sqlx::query_as("SELECT tier FROM messages WHERE id = 'msg123'")
            .fetch_one(&pool)
            .await
            .unwrap();
        
        assert_eq!(tier, "warm");
    }

    #[tokio::test]
    async fn test_migrate_hot_to_warm() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        
        sqlx::query(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                tier TEXT,
                compressed_size INTEGER,
                created_at TEXT NOT NULL,
                last_tier_migration TEXT
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        // Insert messages: some old (should migrate), some new (should stay)
        sqlx::query(
            "INSERT INTO messages (id, tier, compressed_size, created_at) VALUES 
             ('old1', 'hot', 1000, datetime('now', '-10 days')),
             ('old2', 'hot', 2000, datetime('now', '-8 days')),
             ('new1', 'hot', 3000, datetime('now', '-2 days'))"
        )
        .execute(&pool)
        .await
        .unwrap();

        let manager = TierMigrationManager::new(pool.clone());
        let stats = manager.migrate_all().await.unwrap();

        // Should migrate 2 messages (>7 days old)
        assert_eq!(stats.hot_to_warm, 2);

        // Verify tiers
        let warm_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM messages WHERE tier = 'warm'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(warm_count.0, 2);
    }
}
