// Distributed database implementation using CockroachDB
//
// This module provides a geo-distributed PostgreSQL-compatible database layer
// for dchat storage. CockroachDB handles multi-region replication, automatic
// failover, and horizontal scalability.

use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;
use std::time::Duration;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::error::{StorageError, StorageResult};

/// Configuration for distributed database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Multiple database URLs for failover (CockroachDB nodes)
    pub database_urls: Vec<String>,
    /// Maximum connections in pool
    pub max_connections: u32,
    /// Connection timeout
    pub acquire_timeout_seconds: u64,
    /// Replication factor (default: 3)
    pub replication_factor: u32,
    /// Consistency level: "strong" or "eventual"
    pub consistency_level: String,
    /// Query timeout
    pub query_timeout_seconds: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            database_urls: vec![
                "postgresql://dchat:password@localhost:26257/dchat".to_string(),
            ],
            max_connections: 50,
            acquire_timeout_seconds: 10,
            replication_factor: 3,
            consistency_level: "strong".to_string(),
            query_timeout_seconds: 30,
        }
    }
}

/// Distributed database client with multi-region support
pub struct DistributedDatabase {
    /// Connection pool to CockroachDB cluster
    pool: PgPool,
    /// Configuration
    config: DatabaseConfig,
}

/// Message row for database storage
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MessageRow {
    pub id: Uuid,
    pub sender_id: String,
    pub recipient_id: Option<String>,
    pub channel_id: Option<String>,
    pub content: String,
    pub tier: String,
    pub message_type: String,
    pub created_at: DateTime<Utc>,
    pub region: Option<String>,
    pub s3_key: Option<String>,
    pub content_hash: Option<Vec<u8>>,
}

/// Statistics about database operations
#[derive(Debug, Clone, Default)]
pub struct DatabaseStats {
    pub total_messages: i64,
    pub messages_by_tier: std::collections::HashMap<String, i64>,
    pub total_size_bytes: i64,
    pub avg_message_size: i64,
}

impl DistributedDatabase {
    /// Create new distributed database connection
    pub async fn new(config: DatabaseConfig) -> StorageResult<Self> {
        info!("Connecting to distributed database with {} nodes", config.database_urls.len());
        
        // Try each database URL until one succeeds
        let mut last_error = None;
        for (i, url) in config.database_urls.iter().enumerate() {
            debug!("Attempting connection to node {}: {}", i + 1, mask_password(url));
            
            match PgPoolOptions::new()
                .max_connections(config.max_connections)
                .acquire_timeout(Duration::from_secs(config.acquire_timeout_seconds))
                .connect(url)
                .await
            {
                Ok(pool) => {
                    info!("Successfully connected to database node {}", i + 1);
                    return Ok(Self { pool, config });
                }
                Err(e) => {
                    warn!("Failed to connect to node {}: {}", i + 1, e);
                    last_error = Some(e);
                }
            }
        }
        
        error!("Failed to connect to any database node");
        Err(StorageError::Database(
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "No database URLs provided".to_string())
        ))
    }
    
    /// Insert message with geographic awareness
    pub async fn insert_message_geo(
        &self,
        message: &MessageRow,
        region: &str,
    ) -> StorageResult<()> {
        let query_timeout = Duration::from_secs(self.config.query_timeout_seconds);
        
        let result = tokio::time::timeout(
            query_timeout,
            sqlx::query(
                "INSERT INTO messages (id, sender_id, recipient_id, channel_id, content, tier, type, region, created_at, s3_key, content_hash) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
            )
            .bind(&message.id)
            .bind(&message.sender_id)
            .bind(&message.recipient_id)
            .bind(&message.channel_id)
            .bind(&message.content)
            .bind(&message.tier)
            .bind(&message.message_type)
            .bind(region)
            .bind(message.created_at)
            .bind(&message.s3_key)
            .bind(&message.content_hash)
            .execute(&self.pool)
        ).await;
        
        match result {
            Ok(Ok(_)) => {
                debug!("Inserted message {} to region {}", message.id, region);
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Database error inserting message: {}", e);
                Err(StorageError::Database(e.to_string()))
            }
            Err(_) => {
                error!("Query timeout inserting message");
                Err(StorageError::Timeout)
            }
        }
    }
    
    /// Get message by ID
    pub async fn get_message(&self, message_id: Uuid) -> StorageResult<Option<MessageRow>> {
        let query_timeout = Duration::from_secs(self.config.query_timeout_seconds);
        
        let result = tokio::time::timeout(
            query_timeout,
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, sender_id, recipient_id, channel_id, content, tier, type as message_type, created_at, region, s3_key, content_hash \
                 FROM messages WHERE id = $1"
            )
            .bind(message_id)
            .fetch_optional(&self.pool)
        ).await;
        
        match result {
            Ok(Ok(msg)) => Ok(msg),
            Ok(Err(e)) => {
                error!("Database error fetching message: {}", e);
                Err(StorageError::Database(e.to_string()))
            }
            Err(_) => {
                error!("Query timeout fetching message");
                Err(StorageError::Timeout)
            }
        }
    }
    
    /// Get recent messages for a user with pagination
    pub async fn get_recent_messages(
        &self,
        user_id: &str,
        limit: i64,
        offset: i64,
    ) -> StorageResult<Vec<MessageRow>> {
        let query_timeout = Duration::from_secs(self.config.query_timeout_seconds);
        
        let result = tokio::time::timeout(
            query_timeout,
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, sender_id, recipient_id, channel_id, content, tier, type as message_type, created_at, region, s3_key, content_hash \
                 FROM messages \
                 WHERE (sender_id = $1 OR recipient_id = $1) \
                   AND created_at > NOW() - INTERVAL '30 days' \
                 ORDER BY created_at DESC \
                 LIMIT $2 OFFSET $3"
            )
            .bind(user_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
        ).await;
        
        match result {
            Ok(Ok(messages)) => Ok(messages),
            Ok(Err(e)) => {
                error!("Database error fetching recent messages: {}", e);
                Err(StorageError::Database(e.to_string()))
            }
            Err(_) => {
                error!("Query timeout fetching recent messages");
                Err(StorageError::Timeout)
            }
        }
    }
    
    /// Get messages for a channel with pagination
    pub async fn get_channel_messages(
        &self,
        channel_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> StorageResult<Vec<MessageRow>> {
        let query_timeout = Duration::from_secs(self.config.query_timeout_seconds);
        
        let result = tokio::time::timeout(
            query_timeout,
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, sender_id, recipient_id, channel_id, content, tier, type as message_type, created_at, region, s3_key, content_hash \
                 FROM messages \
                 WHERE channel_id = $1 \
                 ORDER BY created_at DESC \
                 LIMIT $2 OFFSET $3"
            )
            .bind(channel_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
        ).await;
        
        match result {
            Ok(Ok(messages)) => Ok(messages),
            Ok(Err(e)) => {
                error!("Database error fetching channel messages: {}", e);
                Err(StorageError::Database(e.to_string()))
            }
            Err(_) => {
                error!("Query timeout fetching channel messages");
                Err(StorageError::Timeout)
            }
        }
    }
    
    /// Update message tier (for lifecycle management)
    pub async fn update_message_tier(
        &self,
        message_id: Uuid,
        new_tier: &str,
        s3_key: Option<&str>,
    ) -> StorageResult<()> {
        let query_timeout = Duration::from_secs(self.config.query_timeout_seconds);
        
        let result = tokio::time::timeout(
            query_timeout,
            sqlx::query(
                "UPDATE messages SET tier = $1, s3_key = $2, last_tier_migration = NOW() WHERE id = $3"
            )
            .bind(new_tier)
            .bind(s3_key)
            .bind(message_id)
            .execute(&self.pool)
        ).await;
        
        match result {
            Ok(Ok(_)) => {
                debug!("Updated message {} to tier {}", message_id, new_tier);
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Database error updating message tier: {}", e);
                Err(StorageError::Database(e.to_string()))
            }
            Err(_) => {
                error!("Query timeout updating message tier");
                Err(StorageError::Timeout)
            }
        }
    }
    
    /// Get database statistics
    pub async fn get_stats(&self) -> StorageResult<DatabaseStats> {
        let query_timeout = Duration::from_secs(self.config.query_timeout_seconds);
        
        // Get total message count
        let total_result = tokio::time::timeout(
            query_timeout,
            sqlx::query("SELECT COUNT(*) as count FROM messages")
                .fetch_one(&self.pool)
        ).await;
        
        let total_messages = match total_result {
            Ok(Ok(row)) => row.try_get::<i64, _>("count").unwrap_or(0),
            _ => 0,
        };
        
        // Get messages by tier
        let tier_result = tokio::time::timeout(
            query_timeout,
            sqlx::query("SELECT tier, COUNT(*) as count FROM messages GROUP BY tier")
                .fetch_all(&self.pool)
        ).await;
        
        let mut messages_by_tier = std::collections::HashMap::new();
        if let Ok(Ok(rows)) = tier_result {
            for row in rows {
                if let (Ok(tier), Ok(count)) = (
                    row.try_get::<String, _>("tier"),
                    row.try_get::<i64, _>("count")
                ) {
                    messages_by_tier.insert(tier, count);
                }
            }
        }
        
        Ok(DatabaseStats {
            total_messages,
            messages_by_tier,
            total_size_bytes: 0, // TODO: calculate from content sizes
            avg_message_size: 0,
        })
    }
    
    /// Health check - test database connectivity
    pub async fn health_check(&self) -> StorageResult<bool> {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            sqlx::query("SELECT 1")
                .fetch_one(&self.pool)
        ).await;
        
        match result {
            Ok(Ok(_)) => Ok(true),
            Ok(Err(e)) => {
                error!("Database health check failed: {}", e);
                Ok(false)
            }
            Err(_) => {
                error!("Database health check timeout");
                Ok(false)
            }
        }
    }
    
    /// Get connection pool reference (for direct queries)
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
    
    /// Close database connection pool
    pub async fn close(self) {
        info!("Closing database connection pool");
        self.pool.close().await;
    }
}

/// Mask password in database URL for logging
fn mask_password(url: &str) -> String {
    if let Some(at_pos) = url.rfind('@') {
        if let Some(colon_pos) = url[..at_pos].rfind(':') {
            let mut masked = url.to_string();
            masked.replace_range(colon_pos + 1..at_pos, "****");
            return masked;
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mask_password() {
        let url = "postgresql://user:password@localhost:26257/dchat";
        let masked = mask_password(url);
        assert!(!masked.contains("password"));
        assert!(masked.contains("****"));
    }
    
    #[test]
    fn test_default_config() {
        let config = DatabaseConfig::default();
        assert_eq!(config.max_connections, 50);
        assert_eq!(config.replication_factor, 3);
        assert_eq!(config.consistency_level, "strong");
    }
}
