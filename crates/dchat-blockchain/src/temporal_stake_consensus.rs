//! Temporal Stake Consensus (TSC)
//!
//! TSC is the third consensus layer that rewards long-term commitment through exponential
//! temporal compounding. Validators gain increasing influence over time, incentivizing
//! network stability and discouraging short-term speculation.
//!
//! Key features:
//! - Exponential temporal compounding: weight = stake × e^(t/T)
//! - Predictive validation using oracle consensus
//! - Lockup tier system (Fluid → Multi-Year)
//! - Time-weighted voting power with diminishing returns
//! - Integration with PoRW and PoT for triple-consensus finality

use crate::block_hierarchy::Hash;
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tracing;

/// Lockup tier determines stake multiplier and withdrawal restrictions
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LockupTier {
    /// No lockup, instant withdrawal, 1.0x multiplier
    Fluid,

    /// 30-day lockup, 1.5x multiplier
    Monthly,

    /// 90-day lockup, 2.25x multiplier
    Quarterly,

    /// 365-day lockup, 4.0x multiplier
    Annual,

    /// 1095-day (3-year) lockup, 8.0x multiplier
    MultiYear,
}

impl LockupTier {
    /// Stake multiplier for this tier
    pub fn multiplier(&self) -> f64 {
        match self {
            LockupTier::Fluid => 1.0,
            LockupTier::Monthly => 1.5,
            LockupTier::Quarterly => 2.25,
            LockupTier::Annual => 4.0,
            LockupTier::MultiYear => 8.0,
        }
    }

    /// Lockup duration
    pub fn duration(&self) -> Duration {
        match self {
            LockupTier::Fluid => Duration::from_secs(0),
            LockupTier::Monthly => Duration::from_secs(30 * 86400),
            LockupTier::Quarterly => Duration::from_secs(90 * 86400),
            LockupTier::Annual => Duration::from_secs(365 * 86400),
            LockupTier::MultiYear => Duration::from_secs(1095 * 86400),
        }
    }

    /// Early withdrawal penalty (% of stake)
    pub fn early_withdrawal_penalty(&self) -> f64 {
        match self {
            LockupTier::Fluid => 0.0,
            LockupTier::Monthly => 0.05,   // 5%
            LockupTier::Quarterly => 0.10, // 10%
            LockupTier::Annual => 0.25,    // 25%
            LockupTier::MultiYear => 0.50, // 50%
        }
    }
}

/// Validator stake with temporal information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalStake {
    /// Validator public key
    pub validator_id: VerifyingKey,

    /// Staked amount (base)
    pub stake_amount: u64,

    /// Lockup tier
    pub lockup_tier: LockupTier,

    /// Timestamp when stake was deposited
    pub deposit_time: SystemTime,

    /// Timestamp when stake can be withdrawn
    pub withdrawal_available_time: SystemTime,

    /// Historical uptime percentage (0.0-1.0)
    pub uptime_percentage: f64,

    /// Number of blocks validated
    pub blocks_validated: u64,

    /// Last activity timestamp
    pub last_activity: SystemTime,
}

impl TemporalStake {
    /// Calculate temporal weight using exponential compounding
    /// Formula: weight = stake × tier_multiplier × e^(t/T)
    /// where t = time staked, T = compounding constant (1 year)
    pub fn calculate_temporal_weight(&self) -> f64 {
        const COMPOUNDING_CONSTANT_DAYS: f64 = 365.0;
        const MAX_MULTIPLIER: f64 = 10.0; // Cap at 10x to prevent excessive concentration

        let time_staked = SystemTime::now()
            .duration_since(self.deposit_time)
            .unwrap_or(Duration::from_secs(0));

        let days_staked = time_staked.as_secs() as f64 / 86400.0;

        // Exponential temporal factor with cap
        let temporal_factor = (days_staked / COMPOUNDING_CONSTANT_DAYS).exp();
        let capped_temporal_factor = temporal_factor.min(MAX_MULTIPLIER);

        // Combine with lockup tier multiplier
        let tier_multiplier = self.lockup_tier.multiplier();

        // Uptime adjustment (penalize poor uptime)
        let uptime_multiplier = self.uptime_percentage.max(0.5); // Min 50% credit

        let base_weight = self.stake_amount as f64;
        base_weight * tier_multiplier * capped_temporal_factor * uptime_multiplier
    }

    /// Check if stake can be withdrawn
    pub fn can_withdraw(&self) -> bool {
        SystemTime::now() >= self.withdrawal_available_time
    }

    /// Calculate penalty for early withdrawal
    pub fn calculate_early_withdrawal_penalty(&self) -> u64 {
        if self.can_withdraw() {
            return 0;
        }

        let penalty_rate = self.lockup_tier.early_withdrawal_penalty();
        (self.stake_amount as f64 * penalty_rate) as u64
    }
}

/// Predictive validation oracle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictiveOracle {
    /// Oracle validator ID
    pub oracle_id: VerifyingKey,

    /// Prediction for next block hash
    pub predicted_hash: Hash,

    /// Confidence score (0.0-1.0)
    pub confidence: f64,

    /// Historical accuracy (0.0-1.0)
    pub historical_accuracy: f64,

    /// Timestamp of prediction
    pub prediction_time: SystemTime,
}

impl PredictiveOracle {
    /// Weight for oracle vote based on accuracy and confidence
    pub fn calculate_vote_weight(&self) -> f64 {
        self.confidence * self.historical_accuracy
    }
}

/// TSC vote for block finality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TSCVote {
    /// Validator ID
    pub validator_id: VerifyingKey,

    /// Block hash being voted on
    pub block_hash: Hash,

    /// Temporal weight of vote
    pub vote_weight: f64,

    /// Predictive oracle (optional)
    pub oracle: Option<PredictiveOracle>,

    /// Timestamp of vote
    pub timestamp: SystemTime,
}

/// Aggregated TSC votes for a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TSCBlockVotes {
    /// Block hash
    pub block_hash: Hash,

    /// Individual votes
    pub votes: Vec<TSCVote>,

    /// Total temporal weight
    pub total_weight: f64,

    /// Oracle consensus weight
    pub oracle_weight: f64,

    /// Whether TSC finality is reached
    pub finalized: bool,
}

impl TSCBlockVotes {
    pub fn new(block_hash: Hash) -> Self {
        Self {
            block_hash,
            votes: Vec::new(),
            total_weight: 0.0,
            oracle_weight: 0.0,
            finalized: false,
        }
    }
}

/// Temporal Stake Consensus engine
pub struct TemporalStakeConsensus {
    /// Validator stakes with temporal information
    validator_stakes: Arc<RwLock<HashMap<VerifyingKey, TemporalStake>>>,

    /// Active TSC votes for blocks
    active_votes: Arc<RwLock<HashMap<Hash, TSCBlockVotes>>>,

    /// Predictive oracles for consensus optimization
    oracles: Arc<RwLock<HashMap<VerifyingKey, PredictiveOracle>>>,

    /// Total network stake (for calculating percentages)
    total_network_stake: Arc<RwLock<u64>>,

    /// TSC finality threshold (default 51% weighted stake)
    finality_threshold: f64,

    /// Oracle consensus weight requirement (default 60%)
    oracle_threshold: f64,
}

impl TemporalStakeConsensus {
    /// Get the predictive oracles
    pub fn oracles(&self) -> &Arc<RwLock<HashMap<VerifyingKey, PredictiveOracle>>> {
        &self.oracles
    }

    /// Get the oracle consensus weight requirement
    pub fn oracle_threshold(&self) -> f64 {
        self.oracle_threshold
    }

    pub fn new() -> Self {
        Self {
            validator_stakes: Arc::new(RwLock::new(HashMap::new())),
            active_votes: Arc::new(RwLock::new(HashMap::new())),
            oracles: Arc::new(RwLock::new(HashMap::new())),
            total_network_stake: Arc::new(RwLock::new(0)),
            finality_threshold: 0.51, // 51% weighted stake
            oracle_threshold: 0.60,   // 60% oracle agreement
        }
    }

    /// Register validator stake
    pub fn register_stake(
        &self,
        validator_id: VerifyingKey,
        stake_amount: u64,
        lockup_tier: LockupTier,
    ) -> Result<(), TSCError> {
        if stake_amount < 1000 {
            return Err(TSCError::InsufficientStake);
        }

        let now = SystemTime::now();
        let withdrawal_time = now + lockup_tier.duration();

        let stake = TemporalStake {
            validator_id,
            stake_amount,
            lockup_tier,
            deposit_time: now,
            withdrawal_available_time: withdrawal_time,
            uptime_percentage: 1.0,
            blocks_validated: 0,
            last_activity: now,
        };

        let temporal_weight = stake.calculate_temporal_weight();

        let mut stakes = self.validator_stakes.write().unwrap();
        stakes.insert(validator_id, stake);

        let mut total = self.total_network_stake.write().unwrap();
        *total += stake_amount;

        tracing::info!(
            "Registered validator stake: {} tokens, {:?} tier, {:.2}x temporal weight",
            stake_amount,
            lockup_tier,
            temporal_weight / stake_amount as f64
        );

        Ok(())
    }

    /// Submit TSC vote for block
    pub fn submit_vote(
        &self,
        validator_id: VerifyingKey,
        block_hash: Hash,
        oracle: Option<PredictiveOracle>,
    ) -> Result<(), TSCError> {
        // Get validator stake
        let stakes = self.validator_stakes.read().unwrap();
        let stake = stakes
            .get(&validator_id)
            .ok_or(TSCError::ValidatorNotRegistered)?;

        // Calculate temporal weight
        let vote_weight = stake.calculate_temporal_weight();

        // Calculate oracle weight if present
        let oracle_weight = oracle
            .as_ref()
            .map(|o| o.calculate_vote_weight())
            .unwrap_or(0.0);

        drop(stakes);

        // Create vote
        let vote = TSCVote {
            validator_id,
            block_hash,
            vote_weight,
            oracle,
            timestamp: SystemTime::now(),
        };

        // Add to active votes
        let mut votes = self.active_votes.write().unwrap();
        let block_votes = votes
            .entry(block_hash)
            .or_insert_with(|| TSCBlockVotes::new(block_hash));

        // Check for double voting
        if block_votes
            .votes
            .iter()
            .any(|v| v.validator_id == validator_id)
        {
            return Err(TSCError::DoubleVote);
        }

        block_votes.votes.push(vote);
        block_votes.total_weight += vote_weight;
        block_votes.oracle_weight += oracle_weight;

        // Check finality
        let total_stake = *self.total_network_stake.read().unwrap();
        let weight_percentage = block_votes.total_weight / total_stake as f64;

        if weight_percentage >= self.finality_threshold {
            block_votes.finalized = true;

            tracing::info!(
                "Block {} reached TSC finality: {:.2}% temporal weight, {:.2}% oracle consensus",
                hex::encode(block_hash.as_bytes()),
                weight_percentage * 100.0,
                block_votes.oracle_weight * 100.0
            );
        }

        Ok(())
    }

    /// Check if block has reached TSC finality
    pub fn check_finality(&self, block_hash: &Hash) -> bool {
        let votes = self.active_votes.read().unwrap();

        if let Some(block_votes) = votes.get(block_hash) {
            block_votes.finalized
        } else {
            false
        }
    }

    /// Get TSC votes for a block
    pub fn get_votes(&self, block_hash: &Hash) -> Option<TSCBlockVotes> {
        let votes = self.active_votes.read().unwrap();
        votes.get(block_hash).cloned()
    }

    /// Update validator uptime (called periodically)
    pub fn update_validator_uptime(
        &self,
        validator_id: &VerifyingKey,
        uptime_percentage: f64,
    ) -> Result<(), TSCError> {
        let mut stakes = self.validator_stakes.write().unwrap();
        let stake = stakes
            .get_mut(validator_id)
            .ok_or(TSCError::ValidatorNotRegistered)?;

        stake.uptime_percentage = uptime_percentage.clamp(0.0, 1.0);
        stake.last_activity = SystemTime::now();

        Ok(())
    }

    /// Withdraw stake (with penalty if early)
    pub fn withdraw_stake(&self, validator_id: &VerifyingKey) -> Result<u64, TSCError> {
        let mut stakes = self.validator_stakes.write().unwrap();
        let stake = stakes
            .get(validator_id)
            .ok_or(TSCError::ValidatorNotRegistered)?
            .clone();

        let penalty = stake.calculate_early_withdrawal_penalty();
        let withdrawal_amount = stake.stake_amount - penalty;

        stakes.remove(validator_id);

        let mut total = self.total_network_stake.write().unwrap();
        *total = total.saturating_sub(stake.stake_amount);

        if penalty > 0 {
            tracing::warn!(
                "Early withdrawal penalty applied: {} tokens ({:.1}%)",
                penalty,
                penalty as f64 / stake.stake_amount as f64 * 100.0
            );
        }

        Ok(withdrawal_amount)
    }

    /// Get total temporal weight in the network
    pub fn total_temporal_weight(&self) -> f64 {
        let stakes = self.validator_stakes.read().unwrap();
        stakes.values().map(|s| s.calculate_temporal_weight()).sum()
    }
}

impl Default for TemporalStakeConsensus {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Error)]
pub enum TSCError {
    #[error("Insufficient stake amount (minimum 1000 tokens)")]
    InsufficientStake,

    #[error("Validator not registered")]
    ValidatorNotRegistered,

    #[error("Double vote detected")]
    DoubleVote,

    #[error("Stake still locked (early withdrawal penalty applies)")]
    StakeLocked,

    #[error("Oracle prediction failed")]
    OraclePredictionFailed,

    #[error("Insufficient oracle consensus")]
    InsufficientOracleConsensus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lockup_tier_multipliers() {
        assert_eq!(LockupTier::Fluid.multiplier(), 1.0);
        assert_eq!(LockupTier::Monthly.multiplier(), 1.5);
        assert_eq!(LockupTier::Quarterly.multiplier(), 2.25);
        assert_eq!(LockupTier::Annual.multiplier(), 4.0);
        assert_eq!(LockupTier::MultiYear.multiplier(), 8.0);
    }

    #[test]
    fn test_temporal_weight_calculation() {
        let validator_id = VerifyingKey::from_bytes(&[1u8; 32]).unwrap();

        let mut stake = TemporalStake {
            validator_id,
            stake_amount: 10000,
            lockup_tier: LockupTier::Annual,
            deposit_time: SystemTime::now() - Duration::from_secs(365 * 86400), // 1 year ago
            withdrawal_available_time: SystemTime::now(),
            uptime_percentage: 1.0,
            blocks_validated: 1000,
            last_activity: SystemTime::now(),
        };

        let weight = stake.calculate_temporal_weight();

        // After 1 year with Annual tier (4x multiplier) and e^1 temporal factor
        // Expected: 10000 * 4.0 * e^1 ≈ 108,731
        assert!(weight > 100000.0 && weight < 120000.0);

        // Test uptime penalty
        stake.uptime_percentage = 0.5;
        let penalized_weight = stake.calculate_temporal_weight();
        assert!(penalized_weight < weight * 0.6); // Should be roughly halved
    }

    #[test]
    fn test_early_withdrawal_penalty() {
        let validator_id = VerifyingKey::from_bytes(&[1u8; 32]).unwrap();
        let now = SystemTime::now();

        let stake = TemporalStake {
            validator_id,
            stake_amount: 10000,
            lockup_tier: LockupTier::Annual,
            deposit_time: now,
            withdrawal_available_time: now + Duration::from_secs(365 * 86400),
            uptime_percentage: 1.0,
            blocks_validated: 0,
            last_activity: now,
        };

        // Should not be able to withdraw yet
        assert!(!stake.can_withdraw());

        // Penalty should be 25% for Annual tier
        let penalty = stake.calculate_early_withdrawal_penalty();
        assert_eq!(penalty, 2500); // 25% of 10000
    }
}
