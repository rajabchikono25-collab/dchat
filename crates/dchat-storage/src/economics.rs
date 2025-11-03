// crates/dchat-storage/src/economics.rs
//! Storage economics with micropayments and bonding curves.
//!
//! Implements economic incentives for long-term storage through token bonds
//! and micropayment streams.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// Storage bond for long-term data retention
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBond {
    /// Bond ID
    pub id: i64,
    /// User ID who created the bond
    pub user_id: String,
    /// Amount of DCHAT tokens bonded
    pub amount_tokens: f64,
    /// Storage size purchased (bytes)
    pub storage_bytes: i64,
    /// Duration of storage guarantee (days)
    pub duration_days: i64,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Expiration timestamp
    pub expires_at: DateTime<Utc>,
    /// Whether bond has been withdrawn
    pub withdrawn: bool,
}

impl StorageBond {
    /// Calculate storage cost using bonding curve
    /// Formula: cost = base_rate * size_gb * sqrt(duration_days) * demand_multiplier
    pub fn calculate_cost(
        size_bytes: i64,
        duration_days: i64,
        demand_multiplier: f64,
    ) -> f64 {
        const BASE_RATE_PER_GB_DAY: f64 = 0.0001; // 0.0001 DCHAT per GB per day base rate
        
        let size_gb = size_bytes as f64 / 1_073_741_824.0;
        let duration_factor = (duration_days as f64).sqrt();
        
        BASE_RATE_PER_GB_DAY * size_gb * duration_factor * demand_multiplier
    }

    /// Calculate yield (interest) for storage bond
    /// Providers earn yield for storing data
    pub fn calculate_yield(amount_tokens: f64, duration_days: i64, apy: f64) -> f64 {
        let years = duration_days as f64 / 365.0;
        amount_tokens * apy * years
    }
}

/// Micropayment stream for pay-as-you-go storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicropaymentStream {
    /// Stream ID
    pub id: i64,
    /// Sender (user)
    pub sender_id: String,
    /// Receiver (storage provider)
    pub receiver_id: String,
    /// Flow rate (tokens per second)
    pub flow_rate_tokens_per_sec: f64,
    /// Total amount streamed so far
    pub total_streamed: f64,
    /// Stream start time
    pub started_at: DateTime<Utc>,
    /// Last payment timestamp
    pub last_payment_at: DateTime<Utc>,
    /// Whether stream is active
    pub active: bool,
}

impl MicropaymentStream {
    /// Calculate amount owed since last payment
    pub fn calculate_owed(&self) -> f64 {
        let now = Utc::now();
        let duration_secs = (now - self.last_payment_at).num_seconds() as f64;
        
        self.flow_rate_tokens_per_sec * duration_secs
    }

    /// Determine flow rate needed for storage requirements
    pub fn required_flow_rate(storage_bytes: i64, cost_per_gb_month: f64) -> f64 {
        let storage_gb = storage_bytes as f64 / 1_073_741_824.0;
        let cost_per_month = storage_gb * cost_per_gb_month;
        let seconds_per_month = 30.0 * 24.0 * 3600.0;
        
        cost_per_month / seconds_per_month
    }
}

/// Storage economics manager with database-backed persistence
pub struct StorageEconomicsManager {
    db_pool: SqlitePool,
    config: EconomicsConfig,
}

/// Economics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicsConfig {
    /// Enable storage bonds
    pub enable_storage_bonds: bool,
    /// Enable micropayment streams
    pub enable_micropayments: bool,
    /// Base APY for storage bonds (e.g., 0.05 = 5%)
    pub storage_bond_apy: f64,
    /// Network demand multiplier (1.0 = normal, >1.0 = high demand)
    pub demand_multiplier: f64,
    /// Minimum bond amount (DCHAT tokens)
    pub min_bond_amount: f64,
    /// Minimum stream duration (seconds)
    pub min_stream_duration_secs: i64,
}

impl Default for EconomicsConfig {
    fn default() -> Self {
        Self {
            enable_storage_bonds: true,
            enable_micropayments: true,
            storage_bond_apy: 0.05, // 5% APY
            demand_multiplier: 1.0,
            min_bond_amount: 10.0, // 10 DCHAT minimum
            min_stream_duration_secs: 3600, // 1 hour minimum
        }
    }
}

impl StorageEconomicsManager {
    pub fn new(db_pool: SqlitePool, config: EconomicsConfig) -> Self {
        Self { db_pool, config }
    }

    /// Create a storage bond with database persistence
    pub async fn create_bond(
        &self,
        user_id: String,
        storage_bytes: i64,
        duration_days: i64,
    ) -> Result<StorageBond, EconomicsError> {
        if !self.config.enable_storage_bonds {
            return Err(EconomicsError::BondsDisabled);
        }

        // Calculate required bond amount
        let amount_tokens = StorageBond::calculate_cost(
            storage_bytes,
            duration_days,
            self.config.demand_multiplier,
        );

        if amount_tokens < self.config.min_bond_amount {
            return Err(EconomicsError::BondTooSmall);
        }

        let now = Utc::now();
        let expires_at = now + Duration::days(duration_days);

        // Insert into storage_bonds table
        let result = sqlx::query(
            "INSERT INTO storage_bonds 
             (user_id, amount_tokens, storage_bytes, duration_days, apy_rate, created_at, expires_at, withdrawn)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)"
        )
        .bind(&user_id)
        .bind(amount_tokens)
        .bind(storage_bytes)
        .bind(duration_days)
        .bind(self.config.storage_bond_apy)
        .bind(now.to_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&self.db_pool)
        .await?;

        let bond_id = result.last_insert_rowid();

        Ok(StorageBond {
            id: bond_id,
            user_id,
            amount_tokens,
            storage_bytes,
            duration_days,
            created_at: now,
            expires_at,
            withdrawn: false,
        })
    }

    /// Get bond by ID
    pub async fn get_bond(&self, bond_id: i64) -> Result<Option<StorageBond>, EconomicsError> {
        let row = sqlx::query_as::<_, (i64, String, f64, i64, i64, String, String, i64)>(
            "SELECT id, user_id, amount_tokens, storage_bytes, duration_days, created_at, expires_at, withdrawn
             FROM storage_bonds WHERE id = ?1"
        )
        .bind(bond_id)
        .fetch_optional(&self.db_pool)
        .await?;

        if let Some((id, user_id, amount_tokens, storage_bytes, duration_days, created_at_str, expires_at_str, withdrawn_int)) = row {
            Ok(Some(StorageBond {
                id,
                user_id,
                amount_tokens,
                storage_bytes,
                duration_days,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                expires_at: DateTime::parse_from_rfc3339(&expires_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                withdrawn: withdrawn_int != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// List bonds for a user
    pub async fn list_user_bonds(&self, user_id: &str) -> Result<Vec<StorageBond>, EconomicsError> {
        let rows = sqlx::query_as::<_, (i64, String, f64, i64, i64, String, String, i64)>(
            "SELECT id, user_id, amount_tokens, storage_bytes, duration_days, created_at, expires_at, withdrawn
             FROM storage_bonds WHERE user_id = ?1 ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.db_pool)
        .await?;

        let bonds = rows.into_iter().map(|(id, user_id, amount_tokens, storage_bytes, duration_days, created_at_str, expires_at_str, withdrawn_int)| {
            StorageBond {
                id,
                user_id,
                amount_tokens,
                storage_bytes,
                duration_days,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                expires_at: DateTime::parse_from_rfc3339(&expires_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                withdrawn: withdrawn_int != 0,
            }
        }).collect();

        Ok(bonds)
    }

    /// Withdraw expired bond with yield calculation
    pub async fn withdraw_bond(&self, bond_id: i64) -> Result<f64, EconomicsError> {
        // Fetch bond details
        let bond = self.get_bond(bond_id).await?
            .ok_or(EconomicsError::Database("Bond not found".to_string()))?;

        if bond.withdrawn {
            return Err(EconomicsError::AlreadyWithdrawn);
        }

        let now = Utc::now();
        if now < bond.expires_at {
            return Err(EconomicsError::BondNotExpired);
        }

        // Calculate yield
        let yield_tokens = StorageBond::calculate_yield(
            bond.amount_tokens,
            bond.duration_days,
            self.config.storage_bond_apy,
        );
        let total_return = bond.amount_tokens + yield_tokens;

        // Mark as withdrawn and record accrued interest
        sqlx::query(
            "UPDATE storage_bonds 
             SET withdrawn = 1, accrued_interest = ?1 
             WHERE id = ?2"
        )
        .bind(yield_tokens)
        .bind(bond_id)
        .execute(&self.db_pool)
        .await?;

        Ok(total_return)
    }

    /// Accrue interest for all active bonds
    pub async fn accrue_interest(&self) -> Result<u64, EconomicsError> {
        let rows = sqlx::query_as::<_, (i64, f64, i64, String)>(
            "SELECT id, amount_tokens, duration_days, created_at
             FROM storage_bonds 
             WHERE withdrawn = 0 AND datetime(expires_at) > datetime('now')"
        )
        .fetch_all(&self.db_pool)
        .await?;

        let mut total_accrued = 0u64;

        for (bond_id, amount_tokens, _duration_days, created_at_str) in rows {
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            
            let elapsed_days = (Utc::now() - created_at).num_days();
            let partial_yield = StorageBond::calculate_yield(
                amount_tokens,
                elapsed_days,
                self.config.storage_bond_apy,
            );

            sqlx::query(
                "UPDATE storage_bonds SET accrued_interest = ?1 WHERE id = ?2"
            )
            .bind(partial_yield)
            .bind(bond_id)
            .execute(&self.db_pool)
            .await?;

            total_accrued += partial_yield as u64;
        }

        Ok(total_accrued)
    }

    /// Start micropayment stream with database persistence
    pub async fn start_stream(
        &self,
        sender_id: String,
        receiver_id: String,
        storage_bytes: i64,
    ) -> Result<MicropaymentStream, EconomicsError> {
        if !self.config.enable_micropayments {
            return Err(EconomicsError::MicropaymentsDisabled);
        }

        // Calculate required flow rate
        let flow_rate = MicropaymentStream::required_flow_rate(storage_bytes, 0.023);
        let now = Utc::now();

        // Insert into micropayment_streams table
        let result = sqlx::query(
            "INSERT INTO micropayment_streams 
             (sender_id, receiver_id, flow_rate_tokens_per_sec, total_streamed, started_at, last_payment_at, is_active)
             VALUES (?1, ?2, ?3, 0.0, ?4, ?5, 1)"
        )
        .bind(&sender_id)
        .bind(&receiver_id)
        .bind(flow_rate)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.db_pool)
        .await?;

        let stream_id = result.last_insert_rowid();

        Ok(MicropaymentStream {
            id: stream_id,
            sender_id,
            receiver_id,
            flow_rate_tokens_per_sec: flow_rate,
            total_streamed: 0.0,
            started_at: now,
            last_payment_at: now,
            active: true,
        })
    }

    /// Get stream by ID
    pub async fn get_stream(&self, stream_id: i64) -> Result<Option<MicropaymentStream>, EconomicsError> {
        let row = sqlx::query_as::<_, (i64, String, String, f64, f64, String, String, i64)>(
            "SELECT id, sender_id, receiver_id, flow_rate_tokens_per_sec, total_streamed, started_at, last_payment_at, is_active
             FROM micropayment_streams WHERE id = ?1"
        )
        .bind(stream_id)
        .fetch_optional(&self.db_pool)
        .await?;

        if let Some((id, sender_id, receiver_id, flow_rate, total_streamed, started_at_str, last_payment_at_str, is_active_int)) = row {
            Ok(Some(MicropaymentStream {
                id,
                sender_id,
                receiver_id,
                flow_rate_tokens_per_sec: flow_rate,
                total_streamed,
                started_at: DateTime::parse_from_rfc3339(&started_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                last_payment_at: DateTime::parse_from_rfc3339(&last_payment_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                active: is_active_int != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Process payment for stream
    pub async fn process_stream_payment(&self, stream_id: i64) -> Result<f64, EconomicsError> {
        let stream = self.get_stream(stream_id).await?
            .ok_or(EconomicsError::Database("Stream not found".to_string()))?;

        if !stream.active {
            return Err(EconomicsError::StreamInactive);
        }

        let amount_owed = stream.calculate_owed();
        let now = Utc::now();

        // Update total_streamed and last_payment_at
        sqlx::query(
            "UPDATE micropayment_streams 
             SET total_streamed = total_streamed + ?1, last_payment_at = ?2 
             WHERE id = ?3"
        )
        .bind(amount_owed)
        .bind(now.to_rfc3339())
        .bind(stream_id)
        .execute(&self.db_pool)
        .await?;

        Ok(amount_owed)
    }

    /// Stop micropayment stream
    pub async fn stop_stream(&self, stream_id: i64) -> Result<(), EconomicsError> {
        // Process final payment
        self.process_stream_payment(stream_id).await?;

        // Mark as inactive
        sqlx::query(
            "UPDATE micropayment_streams SET is_active = 0 WHERE id = ?1"
        )
        .bind(stream_id)
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    /// Get economics statistics from database
    pub async fn get_statistics(&self) -> Result<EconomicsStatistics, EconomicsError> {
        // Query bond statistics
        let bond_stats = sqlx::query_as::<_, (i64, f64, i64)>(
            "SELECT COUNT(*), COALESCE(SUM(amount_tokens), 0.0), COALESCE(SUM(storage_bytes), 0)
             FROM storage_bonds WHERE withdrawn = 0"
        )
        .fetch_one(&self.db_pool)
        .await?;

        // Query stream statistics
        let stream_stats = sqlx::query_as::<_, (i64, f64)>(
            "SELECT COUNT(*), COALESCE(SUM(total_streamed), 0.0)
             FROM micropayment_streams WHERE is_active = 1"
        )
        .fetch_one(&self.db_pool)
        .await?;

        Ok(EconomicsStatistics {
            total_bonds: bond_stats.0 as usize,
            total_bonded_tokens: bond_stats.1,
            total_storage_bonded_bytes: bond_stats.2,
            total_active_streams: stream_stats.0 as usize,
            total_streamed_tokens: stream_stats.1,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicsStatistics {
    pub total_bonds: usize,
    pub total_bonded_tokens: f64,
    pub total_storage_bonded_bytes: i64,
    pub total_active_streams: usize,
    pub total_streamed_tokens: f64,
}

#[derive(Debug, Clone)]
pub enum EconomicsError {
    Database(String),
    BondsDisabled,
    MicropaymentsDisabled,
    BondTooSmall,
    BondNotExpired,
    AlreadyWithdrawn,
    StreamInactive,
    InsufficientBalance,
}

impl std::fmt::Display for EconomicsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "Database error: {}", e),
            Self::BondsDisabled => write!(f, "Storage bonds are disabled"),
            Self::MicropaymentsDisabled => write!(f, "Micropayments are disabled"),
            Self::BondTooSmall => write!(f, "Bond amount too small"),
            Self::BondNotExpired => write!(f, "Bond has not expired yet"),
            Self::AlreadyWithdrawn => write!(f, "Bond already withdrawn"),
            Self::StreamInactive => write!(f, "Micropayment stream is inactive"),
            Self::InsufficientBalance => write!(f, "Insufficient balance"),
        }
    }
}

impl std::error::Error for EconomicsError {}

impl From<sqlx::Error> for EconomicsError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_bond_cost() {
        // 10GB for 30 days with normal demand
        let cost = StorageBond::calculate_cost(10 * 1_073_741_824, 30, 1.0);
        
        // Base: 0.0001 * 10GB * sqrt(30) * 1.0 = 0.001 * 5.477 ≈ 0.005477 DCHAT
        assert!((cost - 0.005477).abs() < 0.0001);
    }

    #[test]
    fn test_storage_bond_yield() {
        // 100 DCHAT for 365 days at 5% APY
        let yield_tokens = StorageBond::calculate_yield(100.0, 365, 0.05);
        
        // 100 * 0.05 * 1 year = 5.0 DCHAT
        assert!((yield_tokens - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_micropayment_flow_rate() {
        // 10GB at $0.023/GB/month
        let flow_rate = MicropaymentStream::required_flow_rate(10 * 1_073_741_824, 0.023);
        
        // 10GB * $0.023/GB/month / (30*24*3600 seconds)
        // = 0.23 / 2592000 ≈ 0.0000000887 tokens/sec
        assert!(flow_rate > 0.0);
        assert!(flow_rate < 0.0001); // Very small per-second rate
    }

    #[test]
    fn test_micropayment_calculate_owed() {
        let now = Utc::now();
        let stream = MicropaymentStream {
            id: 1,
            sender_id: "sender".to_string(),
            receiver_id: "receiver".to_string(),
            flow_rate_tokens_per_sec: 0.000001,
            total_streamed: 0.0,
            started_at: now - Duration::hours(1),
            last_payment_at: now - Duration::hours(1),
            active: true,
        };

        let owed = stream.calculate_owed();
        
        // 3600 seconds * 0.000001 tokens/sec = 0.0036 tokens
        assert!((owed - 0.0036).abs() < 0.0001);
    }

    #[tokio::test]
    async fn test_economics_manager_creation() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        let config = EconomicsConfig::default();
        let manager = StorageEconomicsManager::new(pool, config);
        
        assert_eq!(manager.config.storage_bond_apy, 0.05);
        assert_eq!(manager.config.demand_multiplier, 1.0);
    }

    #[tokio::test]
    async fn test_bond_creation_validation() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        
        // Create storage_bonds table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS storage_bonds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                amount_tokens REAL NOT NULL,
                storage_bytes INTEGER NOT NULL,
                duration_days INTEGER NOT NULL,
                apy_rate REAL NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                withdrawn INTEGER NOT NULL DEFAULT 0,
                accrued_interest REAL NOT NULL DEFAULT 0
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut config = EconomicsConfig::default();
        config.min_bond_amount = 100.0; // High minimum
        let manager = StorageEconomicsManager::new(pool, config);

        // Small bond should fail
        let result = manager.create_bond(
            "user123".to_string(),
            1_073_741_824, // 1GB
            30,
        ).await;

        assert!(matches!(result, Err(EconomicsError::BondTooSmall)));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_bond_lifecycle() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        
        // Create table
        sqlx::query(
            "CREATE TABLE storage_bonds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                amount_tokens REAL NOT NULL,
                storage_bytes INTEGER NOT NULL,
                duration_days INTEGER NOT NULL,
                apy_rate REAL NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                withdrawn INTEGER NOT NULL DEFAULT 0,
                accrued_interest REAL NOT NULL DEFAULT 0
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        let config = EconomicsConfig::default();
        let manager = StorageEconomicsManager::new(pool, config);

        // Create bond (need large size for cost to exceed min_bond_amount of 10.0)
        // 100TB for 365 days: 0.0001 * 100000 * sqrt(365) ≈ 191 DCHAT
        let bond = manager.create_bond(
            "user456".to_string(),
            100_000 * 1_073_741_824, // 100TB
            365,
        ).await.unwrap();

        assert_eq!(bond.user_id, "user456");
        assert_eq!(bond.storage_bytes, 100_000 * 1_073_741_824);
        assert!(!bond.withdrawn);

        // Retrieve bond
        let retrieved = manager.get_bond(bond.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, bond.id);
        assert_eq!(retrieved.user_id, bond.user_id);

        // List user bonds
        let bonds = manager.list_user_bonds("user456").await.unwrap();
        assert_eq!(bonds.len(), 1);
        assert_eq!(bonds[0].id, bond.id);
    }

    #[tokio::test]
    async fn test_full_stream_lifecycle() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        
        // Create table
        sqlx::query(
            "CREATE TABLE micropayment_streams (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender_id TEXT NOT NULL,
                receiver_id TEXT NOT NULL,
                flow_rate_tokens_per_sec REAL NOT NULL,
                total_streamed REAL NOT NULL DEFAULT 0,
                started_at TEXT NOT NULL,
                last_payment_at TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        let config = EconomicsConfig::default();
        let manager = StorageEconomicsManager::new(pool, config);

        // Start stream
        let stream = manager.start_stream(
            "sender789".to_string(),
            "receiver456".to_string(),
            5 * 1_073_741_824, // 5GB
        ).await.unwrap();

        assert_eq!(stream.sender_id, "sender789");
        assert_eq!(stream.receiver_id, "receiver456");
        assert!(stream.active);

        // Retrieve stream
        let retrieved = manager.get_stream(stream.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, stream.id);
        assert!(retrieved.active);

        // Stop stream
        manager.stop_stream(stream.id).await.unwrap();
        
        let stopped = manager.get_stream(stream.id).await.unwrap().unwrap();
        assert!(!stopped.active);
    }

    #[tokio::test]
    async fn test_economics_statistics() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        
        // Create tables
        sqlx::query(
            "CREATE TABLE storage_bonds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                amount_tokens REAL NOT NULL,
                storage_bytes INTEGER NOT NULL,
                duration_days INTEGER NOT NULL,
                apy_rate REAL NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                withdrawn INTEGER NOT NULL DEFAULT 0,
                accrued_interest REAL NOT NULL DEFAULT 0
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE micropayment_streams (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender_id TEXT NOT NULL,
                receiver_id TEXT NOT NULL,
                flow_rate_tokens_per_sec REAL NOT NULL,
                total_streamed REAL NOT NULL DEFAULT 0,
                started_at TEXT NOT NULL,
                last_payment_at TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1
            )"
        )
        .execute(&pool)
        .await
        .unwrap();

        let config = EconomicsConfig::default();
        let manager = StorageEconomicsManager::new(pool, config);

        // Initially empty
        let stats = manager.get_statistics().await.unwrap();
        assert_eq!(stats.total_bonds, 0);
        assert_eq!(stats.total_active_streams, 0);

        // Create some bonds and streams (100TB each to exceed min_bond_amount)
        manager.create_bond("user1".to_string(), 100_000 * 1_073_741_824, 365).await.unwrap();
        manager.create_bond("user2".to_string(), 100_000 * 1_073_741_824, 180).await.unwrap();
        manager.start_stream("user1".to_string(), "provider1".to_string(), 5_368_709_120).await.unwrap();

        let stats = manager.get_statistics().await.unwrap();
        assert_eq!(stats.total_bonds, 2);
        assert_eq!(stats.total_active_streams, 1);
        assert!(stats.total_bonded_tokens > 0.0);
    }
}
