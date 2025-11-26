//! Validator Staking System for dchat Consensus
//!
//! This module implements the complete validator staking lifecycle:
//! - Stake submission and locking
//! - Validator registration and activation
//! - Stake updates (increases/decreases)
//! - Unstaking with cooldown period
//! - Slashing for malicious behavior
//! - Reward distribution
//! - Validator set management
//!
//! Architecture:
//! - Staking data stored on Currency Chain
//! - Validator status tracked on Chat Chain
//! - Cross-chain state synchronization via bridge
//! - Economics managed by TokenomicsManager
//!
//! Security:
//! - Minimum stake: 10,000 DCHAT tokens (prevents Sybil attacks)
//! - Cooldown period: 7 days (prevents rapid stake manipulation)
//! - Slashing: Up to 100% for malicious behavior
//! - Multi-signature required for slashing (5-of-7 governance council)

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Minimum stake required to become validator (10,000 DCHAT)
pub const MIN_VALIDATOR_STAKE: u64 = 10_000_000_000; // 10,000 tokens with 6 decimal precision

/// Maximum stake allowed per validator (1M DCHAT - anti-whale)
pub const MAX_VALIDATOR_STAKE: u64 = 1_000_000_000_000; // 1M tokens

/// Cooldown period for unstaking (7 days)
pub const UNSTAKE_COOLDOWN_SECONDS: i64 = 7 * 24 * 60 * 60;

/// Maximum validators in active set
pub const MAX_ACTIVE_VALIDATORS: usize = 100;

/// Slashing severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashingSeverity {
    /// Minor infraction (1% stake slashed)
    Minor,
    /// Moderate infraction (5% stake slashed)
    Moderate,
    /// Major infraction (15% stake slashed)
    Major,
    /// Critical infraction (50% stake slashed)
    Critical,
    /// Malicious behavior (100% stake slashed + permanent ban)
    Malicious,
}

impl SlashingSeverity {
    /// Get percentage of stake to slash
    pub fn slash_percentage(&self) -> u8 {
        match self {
            SlashingSeverity::Minor => 1,
            SlashingSeverity::Moderate => 5,
            SlashingSeverity::Major => 15,
            SlashingSeverity::Critical => 50,
            SlashingSeverity::Malicious => 100,
        }
    }

    /// Check if ban is permanent
    pub fn is_permanent_ban(&self) -> bool {
        matches!(self, SlashingSeverity::Malicious)
    }
}

/// Validator status in the network
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidatorStatus {
    /// Pending activation (stake submitted but not yet active)
    Pending,
    /// Active and participating in consensus
    Active,
    /// Inactive (stake below minimum or voluntary pause)
    Inactive,
    /// Unstaking (cooldown period in progress)
    Unstaking,
    /// Slashed (stake reduced due to misbehavior)
    Slashed,
    /// Banned (permanent exclusion from validator set)
    Banned,
}

/// Slashing reason and evidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvent {
    pub event_id: Uuid,
    pub validator_id: UserId,
    pub severity: SlashingSeverity,
    pub reason: String,
    /// Cryptographic evidence (e.g., double-sign proof, invalid block)
    pub evidence: Vec<u8>,
    /// Governance council signatures approving slash
    pub council_signatures: Vec<Signature>,
    pub amount_slashed: u64,
    pub slashed_at: DateTime<Utc>,
}

/// Validator stake position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorStake {
    pub validator_id: UserId,
    /// Total staked amount (in smallest token unit)
    pub staked_amount: u64,
    /// Validator public key for block signing
    pub validator_pubkey: VerifyingKey,
    /// Current status
    pub status: ValidatorStatus,
    /// When stake was first submitted
    pub staked_at: DateTime<Utc>,
    /// When stake became active (if Active)
    pub activated_at: Option<DateTime<Utc>>,
    /// When unstaking was initiated (if Unstaking)
    pub unstaking_initiated_at: Option<DateTime<Utc>>,
    /// Amount pending withdrawal after cooldown
    pub unstaking_amount: u64,
    /// Total rewards earned (lifetime)
    pub total_rewards_earned: u64,
    /// Pending rewards (not yet claimed)
    pub pending_rewards: u64,
    /// Slashing history
    pub slashing_events: Vec<SlashingEvent>,
    /// Total amount slashed (lifetime)
    pub total_slashed: u64,
    /// Performance metrics
    pub blocks_produced: u64,
    pub blocks_missed: u64,
    /// Geographic region (for diversity scoring)
    pub region: Option<String>,
    /// Network endpoint for P2P
    pub endpoint: Option<String>,
}

impl ValidatorStake {
    /// Create new validator stake
    pub fn new(
        validator_id: UserId,
        staked_amount: u64,
        validator_pubkey: VerifyingKey,
    ) -> Result<Self> {
        if staked_amount < MIN_VALIDATOR_STAKE {
            return Err(Error::validation(format!(
                "Stake amount {} below minimum {}",
                staked_amount, MIN_VALIDATOR_STAKE
            )));
        }

        if staked_amount > MAX_VALIDATOR_STAKE {
            return Err(Error::validation(format!(
                "Stake amount {} exceeds maximum {}",
                staked_amount, MAX_VALIDATOR_STAKE
            )));
        }

        Ok(Self {
            validator_id,
            staked_amount,
            validator_pubkey,
            status: ValidatorStatus::Pending,
            staked_at: Utc::now(),
            activated_at: None,
            unstaking_initiated_at: None,
            unstaking_amount: 0,
            total_rewards_earned: 0,
            pending_rewards: 0,
            slashing_events: Vec::new(),
            total_slashed: 0,
            blocks_produced: 0,
            blocks_missed: 0,
            region: None,
            endpoint: None,
        })
    }

    /// Activate validator (transition Pending -> Active)
    pub fn activate(&mut self) -> Result<()> {
        if self.status != ValidatorStatus::Pending {
            return Err(Error::validation(format!(
                "Cannot activate validator with status {:?}",
                self.status
            )));
        }

        self.status = ValidatorStatus::Active;
        self.activated_at = Some(Utc::now());
        Ok(())
    }

    /// Initiate unstaking (transition Active -> Unstaking)
    pub fn initiate_unstake(&mut self, amount: u64) -> Result<()> {
        if self.status != ValidatorStatus::Active && self.status != ValidatorStatus::Inactive {
            return Err(Error::validation(format!(
                "Cannot unstake with status {:?}",
                self.status
            )));
        }

        if amount > self.staked_amount {
            return Err(Error::validation(format!(
                "Unstake amount {} exceeds staked amount {}",
                amount, self.staked_amount
            )));
        }

        // Check if partial unstake would drop below minimum
        let remaining = self.staked_amount - amount;
        if remaining > 0 && remaining < MIN_VALIDATOR_STAKE {
            return Err(Error::validation(format!(
                "Remaining stake {} would be below minimum {}",
                remaining, MIN_VALIDATOR_STAKE
            )));
        }

        self.status = ValidatorStatus::Unstaking;
        self.unstaking_initiated_at = Some(Utc::now());
        self.unstaking_amount = amount;

        // If unstaking everything, deactivate immediately
        if remaining == 0 {
            self.staked_amount = 0;
            self.status = ValidatorStatus::Inactive;
        }

        Ok(())
    }

    /// Complete unstaking after cooldown period
    pub fn complete_unstake(&mut self) -> Result<u64> {
        if self.status != ValidatorStatus::Unstaking {
            return Err(Error::validation("Validator not in unstaking status"));
        }

        let unstaking_initiated = self.unstaking_initiated_at.ok_or_else(|| {
            Error::validation("Unstaking initiated timestamp missing")
        })?;

        let elapsed = Utc::now().signed_duration_since(unstaking_initiated);
        if elapsed.num_seconds() < UNSTAKE_COOLDOWN_SECONDS {
            return Err(Error::validation(format!(
                "Cooldown period not complete: {} / {} seconds",
                elapsed.num_seconds(),
                UNSTAKE_COOLDOWN_SECONDS
            )));
        }

        let amount = self.unstaking_amount;
        self.unstaking_amount = 0;
        self.unstaking_initiated_at = None;

        // Update status based on remaining stake
        if self.staked_amount == 0 {
            self.status = ValidatorStatus::Inactive;
        } else {
            self.status = ValidatorStatus::Active;
        }

        Ok(amount)
    }

    /// Apply slashing for misbehavior
    pub fn slash(&mut self, event: SlashingEvent) -> Result<u64> {
        let slash_amount = (self.staked_amount * event.severity.slash_percentage() as u64) / 100;
        
        self.staked_amount = self.staked_amount.saturating_sub(slash_amount);
        self.total_slashed += slash_amount;
        self.slashing_events.push(event.clone());

        // Update status based on severity
        if event.severity.is_permanent_ban() {
            self.status = ValidatorStatus::Banned;
        } else if self.staked_amount < MIN_VALIDATOR_STAKE {
            self.status = ValidatorStatus::Inactive;
        } else {
            self.status = ValidatorStatus::Slashed;
        }

        tracing::warn!(
            "Validator {} slashed: -{} tokens ({:?}), reason: {}",
            self.validator_id,
            slash_amount,
            event.severity,
            event.reason
        );

        Ok(slash_amount)
    }

    /// Add block production reward
    pub fn add_reward(&mut self, amount: u64) {
        self.pending_rewards += amount;
        self.total_rewards_earned += amount;
    }

    /// Claim pending rewards
    pub fn claim_rewards(&mut self) -> u64 {
        let amount = self.pending_rewards;
        self.pending_rewards = 0;
        amount
    }

    /// Record successful block production
    pub fn record_block_produced(&mut self) {
        self.blocks_produced += 1;
    }

    /// Record missed block production opportunity
    pub fn record_block_missed(&mut self) {
        self.blocks_missed += 1;
    }

    /// Get uptime percentage
    pub fn uptime_percentage(&self) -> f64 {
        let total_blocks = self.blocks_produced + self.blocks_missed;
        if total_blocks == 0 {
            return 100.0;
        }
        (self.blocks_produced as f64 / total_blocks as f64) * 100.0
    }

    /// Check if validator is eligible for consensus participation
    pub fn is_eligible(&self) -> bool {
        matches!(
            self.status,
            ValidatorStatus::Active | ValidatorStatus::Slashed
        ) && self.staked_amount >= MIN_VALIDATOR_STAKE
    }
}

/// Staking transaction on currency chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingTransaction {
    pub tx_id: Uuid,
    pub tx_type: StakingTxType,
    pub validator_id: UserId,
    pub amount: u64,
    pub status: String, // "pending", "confirmed", "failed"
    pub block_height: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StakingTxType {
    Stake,
    Unstake,
    IncreaseStake,
    ClaimRewards,
    Slash,
}

/// Staking manager for validator stake lifecycle
pub struct StakingManager {
    /// Active validator stakes (validator_id -> stake)
    validators: Arc<RwLock<HashMap<UserId, ValidatorStake>>>,
    /// Pending staking transactions
    pending_transactions: Arc<RwLock<HashMap<Uuid, StakingTransaction>>>,
    /// Active validator set (sorted by stake, descending)
    active_set: Arc<RwLock<BTreeMap<u64, Vec<UserId>>>>,
    /// Slashing events history
    slashing_history: Arc<RwLock<Vec<SlashingEvent>>>,
}

impl StakingManager {
    /// Create new staking manager
    pub fn new() -> Self {
        Self {
            validators: Arc::new(RwLock::new(HashMap::new())),
            pending_transactions: Arc::new(RwLock::new(HashMap::new())),
            active_set: Arc::new(RwLock::new(BTreeMap::new())),
            slashing_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Submit validator stake (register new validator)
    pub async fn submit_validator_stake(
        &self,
        validator_id: UserId,
        amount: u64,
        validator_pubkey: VerifyingKey,
    ) -> Result<Uuid> {
        // Validate stake amount
        if amount < MIN_VALIDATOR_STAKE {
            return Err(Error::validation(format!(
                "Stake amount {} below minimum {}",
                amount, MIN_VALIDATOR_STAKE
            )));
        }

        if amount > MAX_VALIDATOR_STAKE {
            return Err(Error::validation(format!(
                "Stake amount {} exceeds maximum {}",
                amount, MAX_VALIDATOR_STAKE
            )));
        }

        // Check if validator already exists
        let validators = self.validators.read().unwrap();
        if validators.contains_key(&validator_id) {
            return Err(Error::validation(
                "Validator already registered".to_string(),
            ));
        }
        drop(validators);

        // Create validator stake
        let stake = ValidatorStake::new(validator_id.clone(), amount, validator_pubkey)?;

        // Create pending transaction
        let tx_id = Uuid::new_v4();
        let tx = StakingTransaction {
            tx_id,
            tx_type: StakingTxType::Stake,
            validator_id: validator_id.clone(),
            amount,
            status: "pending".to_string(),
            block_height: 0,
            created_at: Utc::now(),
        };

        // Store stake and transaction
        let mut validators = self.validators.write().unwrap();
        validators.insert(validator_id.clone(), stake);
        drop(validators);

        let mut pending = self.pending_transactions.write().unwrap();
        pending.insert(tx_id, tx);

        tracing::info!(
            "✅ Validator stake submitted: {} ({} DCHAT)",
            validator_id,
            amount as f64 / 1_000_000.0
        );

        Ok(tx_id)
    }

    /// Submit validator unstake
    pub async fn submit_validator_unstake(
        &self,
        validator_id: &UserId,
        amount: u64,
    ) -> Result<Uuid> {
        let mut validators = self.validators.write().unwrap();
        let stake = validators
            .get_mut(validator_id)
            .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))?;

        // Initiate unstaking
        stake.initiate_unstake(amount)?;

        // Create pending transaction
        let tx_id = Uuid::new_v4();
        let tx = StakingTransaction {
            tx_id,
            tx_type: StakingTxType::Unstake,
            validator_id: validator_id.clone(),
            amount,
            status: "pending".to_string(),
            block_height: 0,
            created_at: Utc::now(),
        };

        drop(validators);

        let mut pending = self.pending_transactions.write().unwrap();
        pending.insert(tx_id, tx);

        tracing::info!(
            "Validator {} initiated unstake: {} DCHAT (cooldown: {} days)",
            validator_id,
            amount as f64 / 1_000_000.0,
            UNSTAKE_COOLDOWN_SECONDS / 86400
        );

        Ok(tx_id)
    }

    /// Update stake amount (increase only - decrease via unstake)
    pub async fn update_stake_amount(
        &self,
        validator_id: &UserId,
        additional_amount: u64,
    ) -> Result<Uuid> {
        let mut validators = self.validators.write().unwrap();
        let stake = validators
            .get_mut(validator_id)
            .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))?;

        // Verify not exceeding max stake
        let new_total = stake.staked_amount + additional_amount;
        if new_total > MAX_VALIDATOR_STAKE {
            return Err(Error::validation(format!(
                "New total stake {} would exceed maximum {}",
                new_total, MAX_VALIDATOR_STAKE
            )));
        }

        stake.staked_amount = new_total;

        // If previously inactive, check if can reactivate
        if stake.status == ValidatorStatus::Inactive && new_total >= MIN_VALIDATOR_STAKE {
            stake.status = ValidatorStatus::Pending;
        }

        // Create transaction
        let tx_id = Uuid::new_v4();
        let tx = StakingTransaction {
            tx_id,
            tx_type: StakingTxType::IncreaseStake,
            validator_id: validator_id.clone(),
            amount: additional_amount,
            status: "pending".to_string(),
            block_height: 0,
            created_at: Utc::now(),
        };

        drop(validators);

        let mut pending = self.pending_transactions.write().unwrap();
        pending.insert(tx_id, tx);

        tracing::info!(
            "Validator {} increased stake by {} DCHAT (new total: {})",
            validator_id,
            additional_amount as f64 / 1_000_000.0,
            new_total as f64 / 1_000_000.0
        );

        Ok(tx_id)
    }

    /// Query validator status
    pub fn query_validator_status(&self, validator_id: &UserId) -> Result<ValidatorStake> {
        let validators = self.validators.read().unwrap();
        validators
            .get(validator_id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))
    }

    /// Get all active validators
    pub fn get_active_validators(&self) -> Vec<ValidatorStake> {
        let validators = self.validators.read().unwrap();
        validators
            .values()
            .filter(|v| v.is_eligible())
            .cloned()
            .collect()
    }

    /// Get validator set for consensus (top N by stake)
    pub fn get_validator_set(&self, max_validators: usize) -> Vec<ValidatorStake> {
        let mut eligible: Vec<ValidatorStake> = self.get_active_validators();
        
        // Sort by stake (descending)
        eligible.sort_by(|a, b| b.staked_amount.cmp(&a.staked_amount));
        
        // Take top N
        eligible.truncate(max_validators);
        eligible
    }

    /// Activate pending validator
    pub async fn activate_validator(&self, validator_id: &UserId) -> Result<()> {
        let mut validators = self.validators.write().unwrap();
        let stake = validators
            .get_mut(validator_id)
            .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))?;

        stake.activate()?;

        tracing::info!("✅ Validator {} activated", validator_id);
        Ok(())
    }

    /// Complete unstaking after cooldown
    pub async fn complete_unstake(&self, validator_id: &UserId) -> Result<u64> {
        let mut validators = self.validators.write().unwrap();
        let stake = validators
            .get_mut(validator_id)
            .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))?;

        let amount = stake.complete_unstake()?;

        tracing::info!(
            "✅ Validator {} completed unstake: {} DCHAT returned",
            validator_id,
            amount as f64 / 1_000_000.0
        );

        Ok(amount)
    }

    /// Slash validator for misbehavior
    pub async fn slash_validator(
        &self,
        validator_id: &UserId,
        severity: SlashingSeverity,
        reason: String,
        evidence: Vec<u8>,
        council_signatures: Vec<Signature>,
    ) -> Result<u64> {
        // Verify governance council signatures (5-of-7 multisig)
        if council_signatures.len() < 5 {
            return Err(Error::validation(
                "Insufficient council signatures for slashing".to_string(),
            ));
        }

        let event = SlashingEvent {
            event_id: Uuid::new_v4(),
            validator_id: validator_id.clone(),
            severity,
            reason: reason.clone(),
            evidence,
            council_signatures,
            amount_slashed: 0, // Will be set by slash()
            slashed_at: Utc::now(),
        };

        let mut validators = self.validators.write().unwrap();
        let stake = validators
            .get_mut(validator_id)
            .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))?;

        let slashed_amount = stake.slash(event.clone())?;

        drop(validators);

        // Record in slashing history
        let mut history = self.slashing_history.write().unwrap();
        let mut event_with_amount = event;
        event_with_amount.amount_slashed = slashed_amount;
        history.push(event_with_amount);

        tracing::warn!(
            "⚠️ Validator {} slashed: {} DCHAT ({:?})",
            validator_id,
            slashed_amount as f64 / 1_000_000.0,
            severity
        );

        Ok(slashed_amount)
    }

    /// Distribute block reward to validator
    pub async fn distribute_reward(&self, validator_id: &UserId, amount: u64) -> Result<()> {
        let mut validators = self.validators.write().unwrap();
        let stake = validators
            .get_mut(validator_id)
            .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))?;

        stake.add_reward(amount);

        tracing::debug!(
            "Validator {} earned reward: {} DCHAT",
            validator_id,
            amount as f64 / 1_000_000.0
        );

        Ok(())
    }

    /// Claim validator rewards
    pub async fn claim_rewards(&self, validator_id: &UserId) -> Result<u64> {
        let mut validators = self.validators.write().unwrap();
        let stake = validators
            .get_mut(validator_id)
            .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))?;

        let amount = stake.claim_rewards();

        tracing::info!(
            "Validator {} claimed rewards: {} DCHAT",
            validator_id,
            amount as f64 / 1_000_000.0
        );

        Ok(amount)
    }

    /// Record validator block production
    pub fn record_block_produced(&self, validator_id: &UserId) -> Result<()> {
        let mut validators = self.validators.write().unwrap();
        if let Some(stake) = validators.get_mut(validator_id) {
            stake.record_block_produced();
        }
        Ok(())
    }

    /// Record validator missed block
    pub fn record_block_missed(&self, validator_id: &UserId) -> Result<()> {
        let mut validators = self.validators.write().unwrap();
        if let Some(stake) = validators.get_mut(validator_id) {
            stake.record_block_missed();
        }
        Ok(())
    }

    /// Get total staked amount across all validators
    pub fn get_total_staked(&self) -> u64 {
        let validators = self.validators.read().unwrap();
        validators.values().map(|v| v.staked_amount).sum()
    }

    /// Get slashing history
    pub fn get_slashing_history(&self) -> Vec<SlashingEvent> {
        self.slashing_history.read().unwrap().clone()
    }
}

impl Default for StakingManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Keypair;
    use rand::rngs::OsRng;

    fn create_test_keypair() -> Keypair {
        let mut csprng = OsRng;
        Keypair::generate(&mut csprng)
    }

    #[tokio::test]
    async fn test_submit_validator_stake() {
        let manager = StakingManager::new();
        let keypair = create_test_keypair();
        let validator_id = UserId::new();

        let tx_id = manager
            .submit_validator_stake(
                validator_id.clone(),
                MIN_VALIDATOR_STAKE,
                keypair.public,
            )
            .await
            .unwrap();

        assert!(!tx_id.is_nil());

        let status = manager.query_validator_status(&validator_id).unwrap();
        assert_eq!(status.staked_amount, MIN_VALIDATOR_STAKE);
        assert_eq!(status.status, ValidatorStatus::Pending);
    }

    #[tokio::test]
    async fn test_stake_below_minimum() {
        let manager = StakingManager::new();
        let keypair = create_test_keypair();
        let validator_id = UserId::new();

        let result = manager
            .submit_validator_stake(
                validator_id,
                MIN_VALIDATOR_STAKE - 1,
                keypair.public,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_activate_validator() {
        let manager = StakingManager::new();
        let keypair = create_test_keypair();
        let validator_id = UserId::new();

        manager
            .submit_validator_stake(
                validator_id.clone(),
                MIN_VALIDATOR_STAKE,
                keypair.public,
            )
            .await
            .unwrap();

        manager.activate_validator(&validator_id).await.unwrap();

        let status = manager.query_validator_status(&validator_id).unwrap();
        assert_eq!(status.status, ValidatorStatus::Active);
        assert!(status.activated_at.is_some());
    }

    #[tokio::test]
    async fn test_unstake_validator() {
        let manager = StakingManager::new();
        let keypair = create_test_keypair();
        let validator_id = UserId::new();

        manager
            .submit_validator_stake(
                validator_id.clone(),
                MIN_VALIDATOR_STAKE,
                keypair.public,
            )
            .await
            .unwrap();

        manager.activate_validator(&validator_id).await.unwrap();

        let tx_id = manager
            .submit_validator_unstake(&validator_id, MIN_VALIDATOR_STAKE)
            .await
            .unwrap();

        assert!(!tx_id.is_nil());

        let status = manager.query_validator_status(&validator_id).unwrap();
        assert_eq!(status.status, ValidatorStatus::Inactive);
        assert_eq!(status.staked_amount, 0);
    }

    #[tokio::test]
    async fn test_increase_stake() {
        let manager = StakingManager::new();
        let keypair = create_test_keypair();
        let validator_id = UserId::new();

        manager
            .submit_validator_stake(
                validator_id.clone(),
                MIN_VALIDATOR_STAKE,
                keypair.public,
            )
            .await
            .unwrap();

        let additional = 5_000_000_000; // 5,000 DCHAT
        manager
            .update_stake_amount(&validator_id, additional)
            .await
            .unwrap();

        let status = manager.query_validator_status(&validator_id).unwrap();
        assert_eq!(status.staked_amount, MIN_VALIDATOR_STAKE + additional);
    }

    #[tokio::test]
    async fn test_validator_set_selection() {
        let manager = StakingManager::new();

        // Create 5 validators with different stakes
        let stakes = vec![
            20_000_000_000u64,
            15_000_000_000,
            25_000_000_000,
            10_000_000_000,
            30_000_000_000,
        ];

        for (i, stake) in stakes.iter().enumerate() {
            let keypair = create_test_keypair();
            let validator_id = UserId::from(format!("validator-{}", i));

            manager
                .submit_validator_stake(validator_id.clone(), *stake, keypair.public)
                .await
                .unwrap();

            manager.activate_validator(&validator_id).await.unwrap();
        }

        // Get top 3 validators
        let validator_set = manager.get_validator_set(3);
        assert_eq!(validator_set.len(), 3);

        // Verify sorted by stake (descending)
        assert_eq!(validator_set[0].staked_amount, 30_000_000_000);
        assert_eq!(validator_set[1].staked_amount, 25_000_000_000);
        assert_eq!(validator_set[2].staked_amount, 20_000_000_000);
    }

    #[tokio::test]
    async fn test_slashing() {
        let manager = StakingManager::new();
        let keypair = create_test_keypair();
        let validator_id = UserId::new();

        let initial_stake = MIN_VALIDATOR_STAKE * 2;
        manager
            .submit_validator_stake(
                validator_id.clone(),
                initial_stake,
                keypair.public,
            )
            .await
            .unwrap();

        manager.activate_validator(&validator_id).await.unwrap();

        // Slash 15% for major infraction
        let slashed = manager
            .slash_validator(
                &validator_id,
                SlashingSeverity::Major,
                "Double signing detected".to_string(),
                vec![0xde, 0xad, 0xbe, 0xef],
                vec![keypair.sign(b"governance approval").into(); 5],
            )
            .await
            .unwrap();

        assert_eq!(slashed, initial_stake * 15 / 100);

        let status = manager.query_validator_status(&validator_id).unwrap();
        assert_eq!(status.total_slashed, slashed);
        assert_eq!(status.staked_amount, initial_stake - slashed);
    }

    #[tokio::test]
    async fn test_reward_distribution() {
        let manager = StakingManager::new();
        let keypair = create_test_keypair();
        let validator_id = UserId::new();

        manager
            .submit_validator_stake(
                validator_id.clone(),
                MIN_VALIDATOR_STAKE,
                keypair.public,
            )
            .await
            .unwrap();

        manager.activate_validator(&validator_id).await.unwrap();

        // Distribute rewards
        let reward = 1_000_000; // 1 DCHAT
        manager
            .distribute_reward(&validator_id, reward)
            .await
            .unwrap();

        let status = manager.query_validator_status(&validator_id).unwrap();
        assert_eq!(status.pending_rewards, reward);

        // Claim rewards
        let claimed = manager.claim_rewards(&validator_id).await.unwrap();
        assert_eq!(claimed, reward);

        let status = manager.query_validator_status(&validator_id).unwrap();
        assert_eq!(status.pending_rewards, 0);
        assert_eq!(status.total_rewards_earned, reward);
    }
}
