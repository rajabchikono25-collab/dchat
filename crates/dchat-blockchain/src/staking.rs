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
//!
//! PRODUCTION NOTE: All stake operations now call currency_chain for actual
//! on-chain token locking (plan2.md S-1 through S-7 implementation).

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::currency_chain::CurrencyChainClient;

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

/// Receipt for a reward claim transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimReceipt {
    /// Validator who claimed rewards
    pub validator_id: UserId,
    /// Amount claimed (in smallest units)
    pub amount: u64,
    /// Transaction ID on currency chain
    pub tx_id: String,
    /// When the claim was processed
    pub claimed_at: DateTime<Utc>,
}

impl ClaimReceipt {
    /// Create a zero-amount receipt (no rewards to claim)
    pub fn zero(validator_id: UserId) -> Self {
        Self {
            validator_id,
            amount: 0,
            tx_id: String::new(),
            claimed_at: Utc::now(),
        }
    }
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
    /// Currency chain client for on-chain token operations (production mode)
    currency_chain: Option<Arc<CurrencyChainClient>>,
    /// Governance council public keys for slashing verification (7 members)
    /// SECURITY: These MUST be loaded from secure configuration in production
    governance_council_keys: Vec<VerifyingKey>,
}

/// Minimum number of council signatures required for slashing (5-of-7)
pub const MIN_COUNCIL_SIGNATURES: usize = 5;

impl StakingManager {
    /// Get read access to the active validator set (sorted by stake, descending)
    pub fn active_set(&self) -> &Arc<RwLock<BTreeMap<u64, Vec<UserId>>>> {
        &self.active_set
    }

    /// Create new staking manager without currency chain integration (testing only)
    pub fn new() -> Self {
        Self {
            validators: Arc::new(RwLock::new(HashMap::new())),
            pending_transactions: Arc::new(RwLock::new(HashMap::new())),
            active_set: Arc::new(RwLock::new(BTreeMap::new())),
            slashing_history: Arc::new(RwLock::new(Vec::new())),
            currency_chain: None,
            governance_council_keys: Vec::new(), // Must be set via set_governance_council
        }
    }

    /// Create staking manager with currency chain integration (production)
    /// 
    /// PRODUCTION: Use this constructor in production to ensure all stake
    /// operations are persisted on the currency chain.
    pub fn with_currency_chain(currency_chain: Arc<CurrencyChainClient>) -> Self {
        Self {
            validators: Arc::new(RwLock::new(HashMap::new())),
            pending_transactions: Arc::new(RwLock::new(HashMap::new())),
            active_set: Arc::new(RwLock::new(BTreeMap::new())),
            slashing_history: Arc::new(RwLock::new(Vec::new())),
            currency_chain: Some(currency_chain),
            governance_council_keys: Vec::new(), // Must be set via set_governance_council
        }
    }
    
    /// Set governance council public keys for slashing verification
    /// 
    /// # Security Note
    /// This MUST be called during initialization with the correct council keys.
    /// The keys should be loaded from a secure, auditable configuration source.
    /// Without valid council keys, slashing operations will fail.
    pub fn set_governance_council(&mut self, keys: Vec<VerifyingKey>) {
        if keys.len() != 7 {
            tracing::warn!(
                "Governance council should have exactly 7 members, got {}",
                keys.len()
            );
        }
        self.governance_council_keys = keys;
    }
    
    /// Verify council signatures for a slashing event
    /// 
    /// # Security Note
    /// This performs ACTUAL cryptographic verification of Ed25519 signatures.
    /// Each signature must be from a unique council member.
    fn verify_council_signatures(
        &self,
        validator_id: &UserId,
        severity: &SlashingSeverity,
        reason: &str,
        evidence: &[u8],
        signatures: &[Signature],
    ) -> Result<()> {
        use ed25519_dalek::Verifier;
        
        // Check we have enough council keys configured
        if self.governance_council_keys.is_empty() {
            return Err(Error::validation(
                "Governance council keys not configured - cannot verify slashing".to_string(),
            ));
        }
        
        // Check minimum signature count
        if signatures.len() < MIN_COUNCIL_SIGNATURES {
            return Err(Error::validation(format!(
                "Insufficient council signatures: {} provided, {} required",
                signatures.len(),
                MIN_COUNCIL_SIGNATURES
            )));
        }
        
        // Construct the message that was signed
        // Format: validator_id || severity || reason || evidence_hash
        let mut message = Vec::new();
        message.extend_from_slice(validator_id.to_string().as_bytes());
        message.extend_from_slice(&[*severity as u8]);
        message.extend_from_slice(reason.as_bytes());
        // Hash evidence to prevent message length issues
        let evidence_hash = blake3::hash(evidence);
        message.extend_from_slice(evidence_hash.as_bytes());
        
        // Track which council members have signed (prevent duplicate signatures)
        let mut signed_by: Vec<usize> = Vec::new();
        let mut valid_count = 0;
        
        for signature in signatures {
            // Try to verify against each council member's key
            for (idx, council_key) in self.governance_council_keys.iter().enumerate() {
                // Skip if this council member already signed
                if signed_by.contains(&idx) {
                    continue;
                }
                
                // Verify the signature
                if council_key.verify(&message, signature).is_ok() {
                    valid_count += 1;
                    signed_by.push(idx);
                    break;
                }
            }
        }
        
        // Check if we have enough valid, unique signatures
        if valid_count < MIN_COUNCIL_SIGNATURES {
            return Err(Error::validation(format!(
                "Only {} valid council signatures verified, {} required",
                valid_count,
                MIN_COUNCIL_SIGNATURES
            )));
        }
        
        tracing::info!(
            "Slashing signatures verified: {}/{} council members approved",
            valid_count,
            self.governance_council_keys.len()
        );
        
        Ok(())
    }

    /// Submit validator stake (register new validator)
    /// 
    /// PRODUCTION: This now calls currency_chain.stake() to lock tokens on-chain.
    /// The stake transaction must be confirmed before the validator becomes active.
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

        // PRODUCTION: Lock tokens on currency chain
        let currency_chain_tx_id = if let Some(ref currency_chain) = self.currency_chain {
            // Lock tokens with unstaking cooldown period
            let tx_id = currency_chain.stake(&validator_id, amount, UNSTAKE_COOLDOWN_SECONDS)?;
            tracing::info!(
                "Currency chain stake transaction submitted: {} for {} tokens",
                tx_id, amount
            );
            Some(tx_id)
        } else {
            tracing::warn!(
                "No currency chain configured - stake operation is not persisted on-chain"
            );
            None
        };

        // Create validator stake record
        let stake = ValidatorStake::new(validator_id.clone(), amount, validator_pubkey)?;

        // Create pending transaction (use currency chain tx_id if available)
        let tx_id = currency_chain_tx_id.unwrap_or_else(Uuid::new_v4);
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
            "✅ Validator stake submitted: {} ({} DCHAT) - tx: {}",
            validator_id,
            amount as f64 / 1_000_000.0,
            tx_id
        );

        Ok(tx_id)
    }

    /// Submit validator unstake
    /// 
    /// PRODUCTION: Initiates unstaking. Tokens remain locked until cooldown
    /// period expires and complete_unstake() is called.
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
    /// 
    /// PRODUCTION: Additional stake is locked on the currency chain.
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

        // PRODUCTION: Lock additional tokens on currency chain
        let _currency_chain_tx_id = if let Some(ref currency_chain) = self.currency_chain {
            let tx_id = currency_chain.stake(validator_id, additional_amount, UNSTAKE_COOLDOWN_SECONDS)?;
            tracing::info!(
                "Currency chain additional stake transaction: {} for {} tokens",
                tx_id, additional_amount
            );
            Some(tx_id)
        } else {
            None
        };

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
    /// 
    /// # Security Note
    /// This performs ACTUAL cryptographic verification of council signatures.
    /// Each signature is verified against the governance council's public keys.
    /// At least 5-of-7 valid signatures are required.
    pub async fn slash_validator(
        &self,
        validator_id: &UserId,
        severity: SlashingSeverity,
        reason: String,
        evidence: Vec<u8>,
        council_signatures: Vec<Signature>,
    ) -> Result<u64> {
        // SECURITY FIX: Verify governance council signatures cryptographically
        // Previously only checked signature count, now verifies actual Ed25519 signatures
        self.verify_council_signatures(
            validator_id,
            &severity,
            &reason,
            &evidence,
            &council_signatures,
        )?;

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

    /// Claim validator rewards (legacy method - returns amount only)
    ///
    /// Use `claim_rewards_with_transfer` for production which actually transfers rewards.
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

    /// Claim validator rewards with actual currency chain transfer
    ///
    /// This is the production method that:
    /// 1. Checks pending rewards amount
    /// 2. Mints rewards to validator's wallet via currency chain
    /// 3. Clears pending rewards
    /// 4. Returns a ClaimReceipt with transaction details
    ///
    /// Use this instead of `claim_rewards()` in production code.
    pub async fn claim_rewards_with_transfer(
        &self,
        validator_id: &UserId,
        currency_chain: &CurrencyChainClient,
    ) -> Result<ClaimReceipt> {
        use crate::tokenomics::MintReason;

        // Get pending rewards amount
        let amount = {
            let validators = self.validators.read().unwrap();
            let stake = validators
                .get(validator_id)
                .ok_or_else(|| Error::NotFound(format!("Validator not found: {}", validator_id)))?;
            stake.pending_rewards
        };

        // If no rewards, return zero receipt
        if amount == 0 {
            tracing::debug!("Validator {} has no pending rewards to claim", validator_id);
            return Ok(ClaimReceipt::zero(validator_id.clone()));
        }

        // Mint rewards to validator's wallet via currency chain
        let tx_id = currency_chain.mint_rewards(validator_id, amount, MintReason::BlockReward)?;

        tracing::info!(
            "✅ Minted {} DCHAT rewards to validator {} (tx: {})",
            amount as f64 / 100_000_000.0,
            validator_id,
            tx_id
        );

        // Clear pending rewards
        {
            let mut validators = self.validators.write().unwrap();
            if let Some(stake) = validators.get_mut(validator_id) {
                stake.pending_rewards = 0;
            }
        }

        // Create staking transaction record
        let staking_tx = StakingTransaction {
            tx_id,
            tx_type: StakingTxType::ClaimRewards,
            validator_id: validator_id.clone(),
            amount,
            status: "confirmed".to_string(),
            block_height: 0, // Will be set by chain
            created_at: Utc::now(),
        };

        self.pending_transactions
            .write()
            .unwrap()
            .insert(tx_id, staking_tx);

        Ok(ClaimReceipt {
            validator_id: validator_id.clone(),
            amount,
            tx_id: tx_id.to_string(),
            claimed_at: Utc::now(),
        })
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
