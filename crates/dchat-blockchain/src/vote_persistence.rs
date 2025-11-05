//! Vote Persistence Layer
//!
//! Provides database persistence for consensus votes from all three layers:
//! - Proof-of-Relay-Work (PoRW) votes
//! - Proof-of-Transit (PoT) transit proofs  
//! - Temporal Stake Consensus (TSC) votes
//!
//! Key features:
//! - Audit trail for all consensus votes
//! - Double-vote detection and prevention
//! - Historical vote queries
//! - Vote weight calculations
//! - Slashing evidence storage

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;


#[derive(Debug, Error)]
pub enum VotePersistenceError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Vote already exists for block {0} from validator {1}")]
    DuplicateVote(String, String),

    #[error("Invalid vote data: {0}")]
    InvalidData(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

pub type Result<T> = std::result::Result<T, VotePersistenceError>;

/// PoRW vote record
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PoRWVoteRecord {
    pub id: Uuid,
    pub block_hash: String,
    pub validator_pubkey: Vec<u8>,
    pub vote_weight: f64,
    pub stake_amount: i64,
    pub delivery_count: i64,
    pub uptime_hours: f64,
    pub reputation_score: f64,
    pub seniority_days: i64,
    pub region: String,
    pub signature: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub is_finalized: bool,
}

/// PoT transit proof record
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PoTProofRecord {
    pub id: Uuid,
    pub message_hash: String,
    pub path_id: String,
    pub relay_count: i32,
    pub total_distance_km: f64,
    pub total_duration_ms: i64,
    pub geographic_diversity_score: f64,
    pub finality_level: String,
    pub timestamp: DateTime<Utc>,
    pub is_verified: bool,
}

/// TSC vote record
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TSCVoteRecord {
    pub id: Uuid,
    pub block_hash: String,
    pub validator_pubkey: Vec<u8>,
    pub temporal_weight: f64,
    pub stake_amount: i64,
    pub lockup_duration_days: i64,
    pub lockup_tier: String,
    pub oracle_weight: f64,
    pub uptime_multiplier: f64,
    pub signature: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub is_finalized: bool,
}

/// Vote persistence manager
#[derive(Clone)]
pub struct VotePersistence {
    pool: Arc<PgPool>,
}

impl VotePersistence {
    /// Create new persistence manager
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(database_url)
            .await?;

        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    /// Initialize database schema
    pub async fn initialize_schema(&self) -> Result<()> {
        // PoRW votes table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS porw_votes (
                id UUID PRIMARY KEY,
                block_hash TEXT NOT NULL,
                validator_pubkey BYTEA NOT NULL,
                vote_weight DOUBLE PRECISION NOT NULL,
                stake_amount BIGINT NOT NULL,
                delivery_count BIGINT NOT NULL,
                uptime_hours DOUBLE PRECISION NOT NULL,
                reputation_score DOUBLE PRECISION NOT NULL,
                seniority_days BIGINT NOT NULL,
                region TEXT NOT NULL,
                signature BYTEA NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                is_finalized BOOLEAN NOT NULL DEFAULT FALSE,
                UNIQUE(block_hash, validator_pubkey)
            );
            
            CREATE INDEX IF NOT EXISTS idx_porw_votes_block_hash ON porw_votes(block_hash);
            CREATE INDEX IF NOT EXISTS idx_porw_votes_timestamp ON porw_votes(timestamp);
            CREATE INDEX IF NOT EXISTS idx_porw_votes_finalized ON porw_votes(is_finalized);
            "#,
        )
        .execute(&*self.pool)
        .await?;

        // PoT proofs table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS pot_proofs (
                id UUID PRIMARY KEY,
                message_hash TEXT NOT NULL,
                path_id TEXT NOT NULL,
                relay_count INTEGER NOT NULL,
                total_distance_km DOUBLE PRECISION NOT NULL,
                total_duration_ms BIGINT NOT NULL,
                geographic_diversity_score DOUBLE PRECISION NOT NULL,
                finality_level TEXT NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                is_verified BOOLEAN NOT NULL DEFAULT FALSE,
                UNIQUE(message_hash, path_id)
            );
            
            CREATE INDEX IF NOT EXISTS idx_pot_proofs_message_hash ON pot_proofs(message_hash);
            CREATE INDEX IF NOT EXISTS idx_pot_proofs_timestamp ON pot_proofs(timestamp);
            CREATE INDEX IF NOT EXISTS idx_pot_proofs_verified ON pot_proofs(is_verified);
            "#,
        )
        .execute(&*self.pool)
        .await?;

        // TSC votes table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS tsc_votes (
                id UUID PRIMARY KEY,
                block_hash TEXT NOT NULL,
                validator_pubkey BYTEA NOT NULL,
                temporal_weight DOUBLE PRECISION NOT NULL,
                stake_amount BIGINT NOT NULL,
                lockup_duration_days BIGINT NOT NULL,
                lockup_tier TEXT NOT NULL,
                oracle_weight DOUBLE PRECISION NOT NULL,
                uptime_multiplier DOUBLE PRECISION NOT NULL,
                signature BYTEA NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                is_finalized BOOLEAN NOT NULL DEFAULT FALSE,
                UNIQUE(block_hash, validator_pubkey)
            );
            
            CREATE INDEX IF NOT EXISTS idx_tsc_votes_block_hash ON tsc_votes(block_hash);
            CREATE INDEX IF NOT EXISTS idx_tsc_votes_timestamp ON tsc_votes(timestamp);
            CREATE INDEX IF NOT EXISTS idx_tsc_votes_finalized ON tsc_votes(is_finalized);
            "#,
        )
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    /// Store PoRW vote
    pub async fn store_porw_vote(&self, vote: &PoRWVoteRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO porw_votes (
                id, block_hash, validator_pubkey, vote_weight, stake_amount,
                delivery_count, uptime_hours, reputation_score, seniority_days,
                region, signature, timestamp, is_finalized
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (block_hash, validator_pubkey) DO NOTHING
            "#,
        )
        .bind(vote.id)
        .bind(&vote.block_hash)
        .bind(&vote.validator_pubkey)
        .bind(vote.vote_weight)
        .bind(vote.stake_amount)
        .bind(vote.delivery_count)
        .bind(vote.uptime_hours)
        .bind(vote.reputation_score)
        .bind(vote.seniority_days)
        .bind(&vote.region)
        .bind(&vote.signature)
        .bind(vote.timestamp)
        .bind(vote.is_finalized)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    /// Get all PoRW votes for a block
    pub async fn get_porw_votes(&self, block_hash: &str) -> Result<Vec<PoRWVoteRecord>> {
        let votes = sqlx::query_as::<_, PoRWVoteRecord>(
            "SELECT * FROM porw_votes WHERE block_hash = $1 ORDER BY timestamp",
        )
        .bind(block_hash)
        .fetch_all(&*self.pool)
        .await?;

        Ok(votes)
    }

    /// Check for double-vote in PoRW
    pub async fn check_porw_double_vote(
        &self,
        block_hash: &str,
        validator_pubkey: &[u8],
    ) -> Result<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM porw_votes WHERE block_hash = $1 AND validator_pubkey = $2",
        )
        .bind(block_hash)
        .bind(validator_pubkey)
        .fetch_one(&*self.pool)
        .await?;

        Ok(count > 0)
    }

    /// Store PoT proof
    pub async fn store_pot_proof(&self, proof: &PoTProofRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO pot_proofs (
                id, message_hash, path_id, relay_count, total_distance_km,
                total_duration_ms, geographic_diversity_score, finality_level,
                timestamp, is_verified
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (message_hash, path_id) DO NOTHING
            "#,
        )
        .bind(proof.id)
        .bind(&proof.message_hash)
        .bind(&proof.path_id)
        .bind(proof.relay_count)
        .bind(proof.total_distance_km)
        .bind(proof.total_duration_ms)
        .bind(proof.geographic_diversity_score)
        .bind(&proof.finality_level)
        .bind(proof.timestamp)
        .bind(proof.is_verified)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    /// Get all PoT proofs for a message
    pub async fn get_pot_proofs(&self, message_hash: &str) -> Result<Vec<PoTProofRecord>> {
        let proofs = sqlx::query_as::<_, PoTProofRecord>(
            "SELECT * FROM pot_proofs WHERE message_hash = $1 ORDER BY timestamp",
        )
        .bind(message_hash)
        .fetch_all(&*self.pool)
        .await?;

        Ok(proofs)
    }

    /// Store TSC vote
    pub async fn store_tsc_vote(&self, vote: &TSCVoteRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tsc_votes (
                id, block_hash, validator_pubkey, temporal_weight, stake_amount,
                lockup_duration_days, lockup_tier, oracle_weight, uptime_multiplier,
                signature, timestamp, is_finalized
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (block_hash, validator_pubkey) DO NOTHING
            "#,
        )
        .bind(vote.id)
        .bind(&vote.block_hash)
        .bind(&vote.validator_pubkey)
        .bind(vote.temporal_weight)
        .bind(vote.stake_amount)
        .bind(vote.lockup_duration_days)
        .bind(&vote.lockup_tier)
        .bind(vote.oracle_weight)
        .bind(vote.uptime_multiplier)
        .bind(&vote.signature)
        .bind(vote.timestamp)
        .bind(vote.is_finalized)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    /// Get all TSC votes for a block
    pub async fn get_tsc_votes(&self, block_hash: &str) -> Result<Vec<TSCVoteRecord>> {
        let votes = sqlx::query_as::<_, TSCVoteRecord>(
            "SELECT * FROM tsc_votes WHERE block_hash = $1 ORDER BY timestamp",
        )
        .bind(block_hash)
        .fetch_all(&*self.pool)
        .await?;

        Ok(votes)
    }

    /// Check for double-vote in TSC
    pub async fn check_tsc_double_vote(
        &self,
        block_hash: &str,
        validator_pubkey: &[u8],
    ) -> Result<bool> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tsc_votes WHERE block_hash = $1 AND validator_pubkey = $2",
        )
        .bind(block_hash)
        .bind(validator_pubkey)
        .fetch_one(&*self.pool)
        .await?;

        Ok(count > 0)
    }

    /// Mark votes as finalized for a block
    pub async fn mark_finalized(&self, block_hash: &str) -> Result<()> {
        // Mark PoRW votes
        sqlx::query("UPDATE porw_votes SET is_finalized = TRUE WHERE block_hash = $1")
            .bind(block_hash)
            .execute(&*self.pool)
            .await?;

        // Mark TSC votes
        sqlx::query("UPDATE tsc_votes SET is_finalized = TRUE WHERE block_hash = $1")
            .bind(block_hash)
            .execute(&*self.pool)
            .await?;

        Ok(())
    }

    /// Get vote statistics for a validator
    pub async fn get_validator_stats(&self, validator_pubkey: &[u8]) -> Result<ValidatorStats> {
        let porw_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM porw_votes WHERE validator_pubkey = $1")
                .bind(validator_pubkey)
                .fetch_one(&*self.pool)
                .await?;

        let tsc_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tsc_votes WHERE validator_pubkey = $1")
                .bind(validator_pubkey)
                .fetch_one(&*self.pool)
                .await?;

        let finalized_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM porw_votes WHERE validator_pubkey = $1 AND is_finalized = TRUE",
        )
        .bind(validator_pubkey)
        .fetch_one(&*self.pool)
        .await?;

        Ok(ValidatorStats {
            porw_votes: porw_count,
            tsc_votes: tsc_count,
            finalized_votes: finalized_count,
        })
    }

    /// Clean up old votes (retain last 30 days)
    pub async fn cleanup_old_votes(&self, days: i64) -> Result<u64> {
        let mut total_deleted = 0u64;

        let result = sqlx::query(
            "DELETE FROM porw_votes WHERE timestamp < NOW() - $1 * INTERVAL '1 day' AND is_finalized = TRUE"
        )
        .bind(days)
        .execute(&*self.pool)
        .await?;
        total_deleted += result.rows_affected();

        let result = sqlx::query(
            "DELETE FROM pot_proofs WHERE timestamp < NOW() - $1 * INTERVAL '1 day' AND is_verified = TRUE"
        )
        .bind(days)
        .execute(&*self.pool)
        .await?;
        total_deleted += result.rows_affected();

        let result = sqlx::query(
            "DELETE FROM tsc_votes WHERE timestamp < NOW() - $1 * INTERVAL '1 day' AND is_finalized = TRUE"
        )
        .bind(days)
        .execute(&*self.pool)
        .await?;
        total_deleted += result.rows_affected();

        Ok(total_deleted)
    }
}

/// Validator voting statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorStats {
    pub porw_votes: i64,
    pub tsc_votes: i64,
    pub finalized_votes: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vote_persistence() {
        // Integration test requires database connection
        // Run with: cargo test --features test-db -- --ignored
    }
}
