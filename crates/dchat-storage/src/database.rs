//! Database operations

use crate::schema::Schema;
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePool, Row};
use std::path::PathBuf;

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Path to database file
    pub path: PathBuf,

    /// Maximum number of connections in the pool
    pub max_connections: u32,

    /// Connection acquisition timeout in seconds
    pub connection_timeout_secs: u64,

    /// Idle connection timeout in seconds
    pub idle_timeout_secs: u64,

    /// Maximum connection lifetime in seconds
    pub max_lifetime_secs: u64,

    /// Enable WAL mode for better concurrency
    pub enable_wal: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("dchat.db"),
            max_connections: 10,
            connection_timeout_secs: 30,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1800,
            enable_wal: true,
        }
    }
}

/// Database handle
#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
    config: DatabaseConfig,
}

impl Database {
    /// Create a new database connection with connection pooling
    pub async fn new(config: DatabaseConfig) -> Result<Self> {
        use sqlx::sqlite::SqlitePoolOptions;
        use std::time::Duration;

        // Create database URL
        let db_url = format!("sqlite:{}?mode=rwc", config.path.display());

        tracing::info!(
            "Initializing database pool: max_connections={}, timeout={}s",
            config.max_connections,
            config.connection_timeout_secs
        );

        // Create connection pool with configuration
        let pool = SqlitePoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(Duration::from_secs(config.connection_timeout_secs))
            .idle_timeout(Some(Duration::from_secs(config.idle_timeout_secs)))
            .max_lifetime(Some(Duration::from_secs(config.max_lifetime_secs)))
            .connect(&db_url)
            .await
            .map_err(|e| Error::storage(format!("Failed to connect to database: {}", e)))?;

        tracing::info!("Database connection pool established");

        // Initialize schema
        let mut db = Self { pool, config };
        db.initialize_schema().await?;

        Ok(db)
    }

    /// Initialize database schema
    async fn initialize_schema(&mut self) -> Result<()> {
        // Enable WAL mode if configured
        if self.config.enable_wal {
            sqlx::query("PRAGMA journal_mode = WAL")
                .execute(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to enable WAL: {}", e)))?;
        }

        // Create tables
        for sql in Schema::create_tables() {
            sqlx::query(sql)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to create table: {}", e)))?;
        }

        // Create indexes
        for sql in Schema::create_indexes() {
            sqlx::query(sql)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to create index: {}", e)))?;
        }

        tracing::info!("Database schema initialized");
        Ok(())
    }

    /// Insert a user
    pub async fn insert_user(&self, id: &str, username: &str, public_key: &[u8]) -> Result<()> {
        let created_at = chrono::Utc::now().timestamp();

        sqlx::query("INSERT INTO users (id, username, public_key, created_at) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(username)
            .bind(public_key)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to insert user: {}", e)))?;

        Ok(())
    }

    /// Get a user by ID
    pub async fn get_user(&self, id: &str) -> Result<Option<UserRow>> {
        let row = sqlx::query("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to get user: {}", e)))?;

        if let Some(row) = row {
            Ok(Some(UserRow {
                id: row.get("id"),
                username: row.get("username"),
                public_key: row.get("public_key"),
                created_at: row.get("created_at"),
            }))
        } else {
            Ok(None)
        }
    }

    /// Insert a message
    pub async fn insert_message(&self, message: &MessageRow) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO messages 
            (id, sender_id, recipient_id, channel_id, content_type, content, 
             encrypted_payload, timestamp, sequence_num, status, expires_at, size, content_hash)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&message.id)
        .bind(&message.sender_id)
        .bind(&message.recipient_id)
        .bind(&message.channel_id)
        .bind(&message.content_type)
        .bind(&message.content)
        .bind(&message.encrypted_payload)
        .bind(message.timestamp)
        .bind(message.sequence_num)
        .bind(&message.status)
        .bind(message.expires_at)
        .bind(message.size as i64)
        .bind(&message.content_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to insert message: {}", e)))?;

        Ok(())
    }

    /// Get messages for a user
    pub async fn get_messages_for_user(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query(
            "SELECT * FROM messages WHERE recipient_id = ? OR sender_id = ? ORDER BY timestamp DESC LIMIT ?"
        )
        .bind(user_id)
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to get messages: {}", e)))?;

        let messages = rows
            .iter()
            .map(|row| MessageRow {
                id: row.get("id"),
                sender_id: row.get("sender_id"),
                recipient_id: row.get("recipient_id"),
                channel_id: row.get("channel_id"),
                content_type: row.get("content_type"),
                content: row.get("content"),
                encrypted_payload: row.get("encrypted_payload"),
                timestamp: row.get("timestamp"),
                sequence_num: row.get("sequence_num"),
                status: row.get("status"),
                expires_at: row.get("expires_at"),
                size: row.get::<i64, _>("size") as usize,
                content_hash: row.get("content_hash"),
            })
            .collect();

        Ok(messages)
    }

    /// Get all messages (for filtering by channel, etc.)
    pub async fn get_all_messages(&self, limit: i64) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query("SELECT * FROM messages ORDER BY timestamp DESC LIMIT ?")
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to get all messages: {}", e)))?;

        let messages = rows
            .iter()
            .map(|row| MessageRow {
                id: row.get("id"),
                sender_id: row.get("sender_id"),
                recipient_id: row.get("recipient_id"),
                channel_id: row.get("channel_id"),
                content_type: row.get("content_type"),
                content: row.get("content"),
                encrypted_payload: row.get("encrypted_payload"),
                timestamp: row.get("timestamp"),
                sequence_num: row.get("sequence_num"),
                status: row.get("status"),
                expires_at: row.get("expires_at"),
                size: row.get::<i64, _>("size") as usize,
                content_hash: row.get("content_hash"),
            })
            .collect();

        Ok(messages)
    }

    /// Delete expired messages
    pub async fn delete_expired_messages(&self) -> Result<u64> {
        let now = chrono::Utc::now().timestamp();

        let result =
            sqlx::query("DELETE FROM messages WHERE expires_at IS NOT NULL AND expires_at < ?")
                .bind(now)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to delete expired messages: {}", e)))?;

        Ok(result.rows_affected())
    }

    /// Get database statistics
    pub async fn stats(&self) -> Result<DatabaseStats> {
        let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to get stats: {}", e)))?;

        let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to get stats: {}", e)))?;

        let channel_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to get stats: {}", e)))?;

        Ok(DatabaseStats {
            user_count: user_count as usize,
            message_count: message_count as usize,
            channel_count: channel_count as usize,
        })
    }

    /// Perform health check on the database connection pool
    pub async fn health_check(&self) -> Result<PoolHealth> {
        // Check if we can acquire a connection
        let start = std::time::Instant::now();
        let _conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| Error::storage(format!("Health check failed: {}", e)))?;
        let acquire_time = start.elapsed();

        // Simple query to verify database is responsive
        let _: i64 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Health check query failed: {}", e)))?;

        let total_time = start.elapsed();

        Ok(PoolHealth {
            is_healthy: true,
            pool_size: self.pool.size(),
            idle_connections: self.pool.num_idle(),
            acquire_time_ms: acquire_time.as_millis() as u64,
            query_time_ms: total_time.as_millis() as u64,
        })
    }

    /// Validate all connections in the pool
    pub async fn validate_connections(&self) -> Result<ConnectionValidation> {
        let mut valid_count = 0;
        let mut invalid_count = 0;
        let max_connections = self.config.max_connections as usize;

        // Try to validate by acquiring and testing each connection
        for _ in 0..self.pool.size() {
            match self.pool.acquire().await {
                Ok(mut conn) => match sqlx::query("SELECT 1").execute(&mut *conn).await {
                    Ok(_) => valid_count += 1,
                    Err(_) => invalid_count += 1,
                },
                Err(_) => invalid_count += 1,
            }
        }

        Ok(ConnectionValidation {
            valid_connections: valid_count,
            invalid_connections: invalid_count,
            max_connections,
            validation_passed: invalid_count == 0,
        })
    }

    /// Get connection pool metrics
    pub fn pool_metrics(&self) -> PoolMetrics {
        PoolMetrics {
            size: self.pool.size(),
            idle: self.pool.num_idle(),
            max_connections: self.config.max_connections,
        }
    }

    /// Close the database connection pool gracefully
    pub async fn close(self) -> Result<()> {
        tracing::info!("Closing database connection pool");
        self.pool.close().await;
        Ok(())
    }
}

/// User row
#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub public_key: Vec<u8>,
    pub created_at: i64,
}

/// Message row
#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: String,
    pub sender_id: String,
    pub recipient_id: Option<String>,
    pub channel_id: Option<String>,
    pub content_type: String,
    pub content: String,
    pub encrypted_payload: Vec<u8>,
    pub timestamp: i64,
    pub sequence_num: Option<i64>,
    pub status: String,
    pub expires_at: Option<i64>,
    pub size: usize,
    pub content_hash: Option<String>,
}

/// Database statistics
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub user_count: usize,
    pub message_count: usize,
    pub channel_count: usize,
}

/// Connection pool health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolHealth {
    pub is_healthy: bool,
    pub pool_size: u32,
    pub idle_connections: usize,
    pub acquire_time_ms: u64,
    pub query_time_ms: u64,
}

/// Connection validation results
#[derive(Debug, Clone)]
pub struct ConnectionValidation {
    pub valid_connections: usize,
    pub invalid_connections: usize,
    pub max_connections: usize,
    pub validation_passed: bool,
}

/// Connection pool metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMetrics {
    pub size: u32,
    pub idle: usize,
    pub max_connections: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_database_creation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let config = DatabaseConfig {
            path: db_path,
            max_connections: 5,
            enable_wal: true,
            connection_timeout_secs: 30,
            idle_timeout_secs: 300,
            max_lifetime_secs: 1800,
        };

        let db = Database::new(config).await;
        assert!(db.is_ok());
    }
}
