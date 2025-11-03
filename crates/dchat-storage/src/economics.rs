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

/// Storage economics manager
pub struct StorageEconomicsManager {
    #[allow(dead_code)]
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

    /// Create a storage bond
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

        // Mock implementation - would store in database
        Ok(StorageBond {
            id: 1,
            user_id,
            amount_tokens,
            storage_bytes,
            duration_days,
            created_at: now,
            expires_at,
            withdrawn: false,
        })
    }

    /// Withdraw expired bond
    pub async fn withdraw_bond(&self, _bond_id: i64) -> Result<f64, EconomicsError> {
        // Mock implementation - would fetch from database
        let sample_bond_amount = 100.0;
        let sample_duration_days = 365;
        
        let yield_tokens = StorageBond::calculate_yield(
            sample_bond_amount,
            sample_duration_days,
            self.config.storage_bond_apy,
        );
        let total_return = sample_bond_amount + yield_tokens;

        Ok(total_return)
    }

    /// Start micropayment stream
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

        // Mock implementation - would store in database
        Ok(MicropaymentStream {
            id: 1,
            sender_id,
            receiver_id,
            flow_rate_tokens_per_sec: flow_rate,
            total_streamed: 0.0,
            started_at: now,
            last_payment_at: now,
            active: true,
        })
    }

    /// Process payment for stream
    pub async fn process_stream_payment(&self, _stream_id: i64) -> Result<f64, EconomicsError> {
        // Mock implementation - would fetch from database and process payment
        let sample_flow_rate = 0.000001; // tokens per second
        let elapsed_secs = 3600.0; // 1 hour
        let amount_owed = sample_flow_rate * elapsed_secs;
        
        Ok(amount_owed)
    }

    /// Stop micropayment stream
    pub async fn stop_stream(&self, stream_id: i64) -> Result<(), EconomicsError> {
        // Process final payment
        self.process_stream_payment(stream_id).await?;

        // Mock implementation - would mark as inactive in database
        Ok(())
    }

    /// Get economics statistics
    pub async fn get_statistics(&self) -> Result<EconomicsStatistics, EconomicsError> {
        // Mock implementation - would query database for actual statistics
        Ok(EconomicsStatistics {
            total_bonds: 0,
            total_bonded_tokens: 0.0,
            total_storage_bonded_bytes: 0,
            total_active_streams: 0,
            total_streamed_tokens: 0.0,
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
}
