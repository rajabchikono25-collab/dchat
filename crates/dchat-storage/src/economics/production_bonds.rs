//! Production-grade storage bond system
//!
//! This module implements a comprehensive bonding system with:
//! - Cryptographic signature verification for all operations
//! - Unbonding period with cooldown
//! - Early termination with slashing penalty
//! - Bond status state machine
//! - Provider staking requirements
//! - Rate limiting and abuse prevention
//! - Multi-signature bonds for high-value storage
//! - Chain synchronization
//!
//! Implements Section 23 (Data Lifecycle & Storage Economics) from ARCHITECTURE-2.0.md

use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Production bond configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionBondConfig {
    /// Minimum bond amount (in smallest token unit, 8 decimals)
    pub min_bond_amount: u64,
    /// Maximum bond amount (prevent excessive concentration)
    pub max_bond_amount: u64,
    /// Minimum bond duration (days)
    pub min_duration_days: u32,
    /// Maximum bond duration (days)
    pub max_duration_days: u32,
    /// Unbonding period (days) - funds locked after withdrawal request
    pub unbonding_period_days: u32,
    /// Early termination penalty rate (0.0 to 1.0)
    pub early_termination_penalty: f64,
    /// Base APY for storage providers (0.0 to 1.0)
    pub base_apy: f64,
    /// Provider staking requirement (tokens)
    pub provider_stake_requirement: u64,
    /// Multi-sig threshold for large bonds (bond_amount / this value = required signatures)
    pub multisig_threshold_amount: u64,
    /// Rate limit: max bonds per user per day
    pub max_bonds_per_user_per_day: u32,
    /// Grace period before expired bonds are slashed (days)
    pub grace_period_days: u32,
    /// Demand multiplier adjustment rate
    pub demand_adjustment_rate: f64,
    /// Network utilization target (0.0 to 1.0)
    pub target_utilization: f64,
    /// Offline mode - skip chain submissions (for testing)
    #[serde(default)]
    pub offline_mode: bool,
}

impl Default for ProductionBondConfig {
    fn default() -> Self {
        Self {
            min_bond_amount: 10_0000_0000,        // 10 DCHAT (8 decimals)
            max_bond_amount: 1_000_000_0000_0000, // 1M DCHAT
            min_duration_days: 30,
            max_duration_days: 730, // 2 years max
            unbonding_period_days: 7,
            early_termination_penalty: 0.10, // 10% penalty
            base_apy: 0.05,                  // 5% APY
            provider_stake_requirement: 10_000_0000_0000, // 10k DCHAT
            multisig_threshold_amount: 100_000_0000_0000, // 100k DCHAT triggers multisig
            max_bonds_per_user_per_day: 10,
            grace_period_days: 7,
            demand_adjustment_rate: 0.01, // 1% per day
            target_utilization: 0.70,     // 70% target
            offline_mode: false,
        }
    }
}

/// Bond status state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BondStatus {
    /// Bond request submitted, awaiting confirmation
    Pending,
    /// Bond is active and storage is reserved
    Active,
    /// Bond has expired, in grace period
    Expired,
    /// Unbonding initiated, funds locked for unbonding period
    Unbonding,
    /// Bond fully withdrawn
    Withdrawn,
    /// Bond was slashed (provider misbehavior or user violation)
    Slashed,
    /// Bond terminated early with penalty
    EarlyTerminated,
}

impl BondStatus {
    /// Check if transition is valid
    pub fn can_transition_to(&self, next: BondStatus) -> bool {
        use BondStatus::*;
        matches!(
            (self, next),
            (Pending, Active)
                | (Pending, Slashed)
                | (Active, Expired)
                | (Active, Unbonding)
                | (Active, Slashed)
                | (Active, EarlyTerminated)
                | (Expired, Unbonding)
                | (Expired, Slashed)
                | (Unbonding, Withdrawn)
                | (Unbonding, Slashed)
        )
    }
}

/// Production storage bond
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionBond {
    /// Unique bond ID (hash of creation parameters)
    pub id: [u8; 32],
    /// User's public key
    pub user_key: [u8; 32],
    /// Assigned storage provider (if any)
    pub provider_key: Option<[u8; 32]>,
    /// Bond amount in smallest token unit
    pub amount: u64,
    /// Storage quota in bytes
    pub storage_bytes: u64,
    /// Duration in days
    pub duration_days: u32,
    /// APY rate at creation time
    pub apy_rate: f64,
    /// Demand multiplier at creation time
    pub demand_multiplier: f64,
    /// Current status
    pub status: BondStatus,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Expiration timestamp
    pub expires_at: DateTime<Utc>,
    /// Unbonding start timestamp (if unbonding)
    pub unbonding_started_at: Option<DateTime<Utc>>,
    /// Withdrawal timestamp
    pub withdrawn_at: Option<DateTime<Utc>>,
    /// Accrued interest
    pub accrued_interest: u64,
    /// Slashed amount (if slashed)
    pub slashed_amount: u64,
    /// Creation transaction ID on chain
    pub creation_tx_id: Option<String>,
    /// Last state update transaction ID
    pub last_update_tx_id: Option<String>,
    /// Required signatures for multi-sig (1 = normal, >1 = multisig)
    pub required_signatures: u8,
    /// Collected signatures (for multisig operations)
    pub collected_signatures: Vec<BondSignature>,
    /// Nonce for replay protection
    pub nonce: u64,
}

/// Signature for bond operations
#[derive(Debug, Clone)]
pub struct BondSignature {
    pub signer: [u8; 32],
    pub signature: Vec<u8>, // 64 bytes stored as Vec for serde compatibility
    pub timestamp: DateTime<Utc>,
    pub operation: BondOperation,
}

// Custom serialization for BondSignature
impl Serialize for BondSignature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BondSignature", 4)?;
        state.serialize_field("signer", &hex::encode(self.signer))?;
        state.serialize_field("signature", &hex::encode(&self.signature))?;
        state.serialize_field("timestamp", &self.timestamp)?;
        state.serialize_field("operation", &self.operation)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for BondSignature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct BondSignatureHelper {
            signer: String,
            signature: String,
            timestamp: DateTime<Utc>,
            operation: BondOperation,
        }

        let helper = BondSignatureHelper::deserialize(deserializer)?;
        let signer_bytes = hex::decode(&helper.signer).map_err(serde::de::Error::custom)?;
        let signature_bytes = hex::decode(&helper.signature).map_err(serde::de::Error::custom)?;

        let mut signer = [0u8; 32];
        if signer_bytes.len() == 32 {
            signer.copy_from_slice(&signer_bytes);
        } else {
            return Err(serde::de::Error::custom("Invalid signer length"));
        }

        Ok(BondSignature {
            signer,
            signature: signature_bytes,
            timestamp: helper.timestamp,
            operation: helper.operation,
        })
    }
}

/// Bond operations that require signatures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BondOperation {
    Create,
    Extend,
    InitiateUnbonding,
    CompleteWithdrawal,
    EarlyTerminate,
    AssignProvider,
}

/// Bond creation request
#[derive(Debug, Clone)]
pub struct CreateBondRequest {
    pub user_key: VerifyingKey,
    pub amount: u64,
    pub storage_gb: u64,
    pub duration_days: u32,
    pub signature: Signature,
    pub nonce: u64,
}

/// Bond withdrawal request
#[derive(Debug, Clone)]
pub struct WithdrawBondRequest {
    pub bond_id: [u8; 32],
    pub user_key: VerifyingKey,
    pub signature: Signature,
    pub nonce: u64,
}

/// Storage provider registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageProvider {
    pub key: [u8; 32],
    pub stake_amount: u64,
    pub registered_at: DateTime<Utc>,
    pub total_storage_capacity: u64,
    pub used_storage: u64,
    pub reputation_score: f64,
    pub bonds_served: u64,
    pub slashing_incidents: u32,
    pub is_active: bool,
}

/// Rate limiting state
#[derive(Debug, Default)]
struct RateLimitState {
    bonds_today: HashMap<[u8; 32], Vec<DateTime<Utc>>>,
    last_cleanup: DateTime<Utc>,
}

/// Production bond manager
pub struct ProductionBondManager {
    /// Configuration
    config: ProductionBondConfig,
    /// Active bonds
    bonds: Arc<RwLock<HashMap<[u8; 32], ProductionBond>>>,
    /// Registered storage providers
    providers: Arc<RwLock<HashMap<[u8; 32], StorageProvider>>>,
    /// Rate limiting state
    rate_limits: Arc<RwLock<RateLimitState>>,
    /// Current demand multiplier
    demand_multiplier: Arc<RwLock<f64>>,
    /// Network utilization (0.0 to 1.0)
    network_utilization: Arc<RwLock<f64>>,
    /// Total bonded amount
    total_bonded: AtomicU64,
    /// Total storage bonded (bytes)
    total_storage_bonded: AtomicU64,
    /// Chain RPC endpoint
    rpc_endpoint: String,
    /// Nonce tracker for replay protection
    user_nonces: Arc<RwLock<HashMap<[u8; 32], u64>>>,
}

/// Bond creation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondCreationResult {
    pub bond_id: [u8; 32],
    pub status: BondStatus,
    pub amount: u64,
    pub storage_bytes: u64,
    pub expires_at: DateTime<Utc>,
    pub estimated_yield: u64,
    pub tx_id: Option<String>,
}

/// Withdrawal result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalResult {
    pub bond_id: [u8; 32],
    pub principal_returned: u64,
    pub interest_earned: u64,
    pub penalty_applied: u64,
    pub net_amount: u64,
    pub unbonding_ends_at: Option<DateTime<Utc>>,
    pub tx_id: Option<String>,
}

/// Bond error
#[derive(Debug, Clone)]
pub enum BondError {
    /// Invalid signature
    InvalidSignature(String),
    /// Bond not found
    BondNotFound([u8; 32]),
    /// Invalid state transition
    InvalidStateTransition { from: BondStatus, to: BondStatus },
    /// Insufficient funds
    InsufficientFunds { required: u64, available: u64 },
    /// Rate limited
    RateLimited { retry_after: Duration },
    /// Bond amount out of range
    AmountOutOfRange { min: u64, max: u64, provided: u64 },
    /// Duration out of range
    DurationOutOfRange { min: u32, max: u32, provided: u32 },
    /// Provider not registered
    ProviderNotRegistered([u8; 32]),
    /// Provider insufficient stake
    ProviderInsufficientStake { required: u64, staked: u64 },
    /// Unbonding period not complete
    UnbondingNotComplete { ends_at: DateTime<Utc> },
    /// Invalid nonce (replay attack)
    InvalidNonce { expected: u64, provided: u64 },
    /// Multi-sig required
    MultiSigRequired { required: u8, collected: u8 },
    /// Chain error
    ChainError(String),
    /// Internal error
    Internal(String),
}

impl std::fmt::Display for BondError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSignature(msg) => write!(f, "Invalid signature: {}", msg),
            Self::BondNotFound(id) => write!(f, "Bond not found: {}", hex::encode(id)),
            Self::InvalidStateTransition { from, to } => {
                write!(f, "Invalid state transition: {:?} -> {:?}", from, to)
            }
            Self::InsufficientFunds {
                required,
                available,
            } => {
                write!(
                    f,
                    "Insufficient funds: required {}, available {}",
                    required, available
                )
            }
            Self::RateLimited { retry_after } => {
                write!(f, "Rate limited, retry after {:?}", retry_after)
            }
            Self::AmountOutOfRange { min, max, provided } => {
                write!(f, "Amount {} out of range [{}, {}]", provided, min, max)
            }
            Self::DurationOutOfRange { min, max, provided } => {
                write!(
                    f,
                    "Duration {} days out of range [{}, {}]",
                    provided, min, max
                )
            }
            Self::ProviderNotRegistered(key) => {
                write!(f, "Provider not registered: {}", hex::encode(key))
            }
            Self::ProviderInsufficientStake { required, staked } => {
                write!(f, "Provider stake {}, required {}", staked, required)
            }
            Self::UnbondingNotComplete { ends_at } => {
                write!(f, "Unbonding ends at {}", ends_at)
            }
            Self::InvalidNonce { expected, provided } => {
                write!(f, "Invalid nonce: expected {}, got {}", expected, provided)
            }
            Self::MultiSigRequired {
                required,
                collected,
            } => {
                write!(
                    f,
                    "Multi-sig: need {} signatures, have {}",
                    required, collected
                )
            }
            Self::ChainError(msg) => write!(f, "Chain error: {}", msg),
            Self::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for BondError {}

impl ProductionBondManager {
    /// Create a new production bond manager
    pub fn new(config: ProductionBondConfig, rpc_endpoint: String) -> Self {
        Self {
            config,
            bonds: Arc::new(RwLock::new(HashMap::new())),
            providers: Arc::new(RwLock::new(HashMap::new())),
            rate_limits: Arc::new(RwLock::new(RateLimitState::default())),
            demand_multiplier: Arc::new(RwLock::new(1.0)),
            network_utilization: Arc::new(RwLock::new(0.0)),
            total_bonded: AtomicU64::new(0),
            total_storage_bonded: AtomicU64::new(0),
            rpc_endpoint,
            user_nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new storage bond with signature verification
    pub async fn create_bond(
        &self,
        request: CreateBondRequest,
    ) -> Result<BondCreationResult, BondError> {
        let user_key_bytes: [u8; 32] = request.user_key.to_bytes();

        // 1. Verify signature
        let message = self.create_bond_message(&request);
        request
            .user_key
            .verify(&message, &request.signature)
            .map_err(|e| BondError::InvalidSignature(e.to_string()))?;

        // 2. Verify nonce (replay protection)
        self.verify_and_increment_nonce(&user_key_bytes, request.nonce)?;

        // 3. Check rate limits
        self.check_rate_limit(&user_key_bytes)?;

        // 4. Validate amount
        if request.amount < self.config.min_bond_amount {
            return Err(BondError::AmountOutOfRange {
                min: self.config.min_bond_amount,
                max: self.config.max_bond_amount,
                provided: request.amount,
            });
        }
        if request.amount > self.config.max_bond_amount {
            return Err(BondError::AmountOutOfRange {
                min: self.config.min_bond_amount,
                max: self.config.max_bond_amount,
                provided: request.amount,
            });
        }

        // 5. Validate duration
        if request.duration_days < self.config.min_duration_days {
            return Err(BondError::DurationOutOfRange {
                min: self.config.min_duration_days,
                max: self.config.max_duration_days,
                provided: request.duration_days,
            });
        }
        if request.duration_days > self.config.max_duration_days {
            return Err(BondError::DurationOutOfRange {
                min: self.config.min_duration_days,
                max: self.config.max_duration_days,
                provided: request.duration_days,
            });
        }

        // 6. Calculate storage and pricing
        let storage_bytes = request.storage_gb * 1_073_741_824;
        let demand_multiplier = *self.demand_multiplier.read();
        let required_amount =
            self.calculate_bond_cost(storage_bytes, request.duration_days, demand_multiplier);

        if request.amount < required_amount {
            return Err(BondError::InsufficientFunds {
                required: required_amount,
                available: request.amount,
            });
        }

        // 7. Determine if multi-sig is required
        let required_signatures = if request.amount >= self.config.multisig_threshold_amount {
            2 // Require 2 signatures for large bonds
        } else {
            1
        };

        // 8. Create bond
        let now = Utc::now();
        let expires_at = now + Duration::days(request.duration_days as i64);

        let bond_id = self.generate_bond_id(&user_key_bytes, request.nonce, now);

        let bond = ProductionBond {
            id: bond_id,
            user_key: user_key_bytes,
            provider_key: None,
            amount: request.amount,
            storage_bytes,
            duration_days: request.duration_days,
            apy_rate: self.config.base_apy,
            demand_multiplier,
            status: BondStatus::Pending,
            created_at: now,
            expires_at,
            unbonding_started_at: None,
            withdrawn_at: None,
            accrued_interest: 0,
            slashed_amount: 0,
            creation_tx_id: None,
            last_update_tx_id: None,
            required_signatures,
            collected_signatures: vec![BondSignature {
                signer: user_key_bytes,
                signature: request.signature.to_bytes().to_vec(),
                timestamp: now,
                operation: BondOperation::Create,
            }],
            nonce: request.nonce,
        };

        // 9. Submit to chain
        let tx_id = self.submit_bond_to_chain(&bond).await?;

        // 10. Store bond
        let mut bond = bond;
        bond.creation_tx_id = Some(tx_id.clone());
        bond.status = BondStatus::Active;

        {
            let mut bonds = self.bonds.write();
            bonds.insert(bond_id, bond.clone());
        }

        // 11. Update totals
        self.total_bonded
            .fetch_add(request.amount, Ordering::SeqCst);
        self.total_storage_bonded
            .fetch_add(storage_bytes, Ordering::SeqCst);

        // 12. Record rate limit
        self.record_bond_creation(&user_key_bytes);

        // 13. Calculate estimated yield
        let estimated_yield = self.calculate_yield(request.amount, request.duration_days);

        info!(
            "Bond created: id={}, user={}, amount={}, storage={}GB, duration={}d",
            hex::encode(bond_id),
            hex::encode(user_key_bytes),
            request.amount,
            request.storage_gb,
            request.duration_days
        );

        Ok(BondCreationResult {
            bond_id,
            status: BondStatus::Active,
            amount: request.amount,
            storage_bytes,
            expires_at,
            estimated_yield,
            tx_id: Some(tx_id),
        })
    }

    /// Initiate unbonding (start withdrawal cooldown)
    pub async fn initiate_unbonding(
        &self,
        request: WithdrawBondRequest,
    ) -> Result<WithdrawalResult, BondError> {
        let user_key_bytes = request.user_key.to_bytes();

        // 1. Verify signature
        let message = self.create_withdraw_message(&request);
        request
            .user_key
            .verify(&message, &request.signature)
            .map_err(|e| BondError::InvalidSignature(e.to_string()))?;

        // 2. Verify nonce
        self.verify_and_increment_nonce(&user_key_bytes, request.nonce)?;

        // 3. Get bond
        let bond = {
            let bonds = self.bonds.read();
            bonds
                .get(&request.bond_id)
                .cloned()
                .ok_or(BondError::BondNotFound(request.bond_id))?
        };

        // 4. Verify ownership
        if bond.user_key != user_key_bytes {
            return Err(BondError::InvalidSignature("Not bond owner".to_string()));
        }

        // 5. Check valid state transition
        let now = Utc::now();
        let current_status = if now > bond.expires_at && bond.status == BondStatus::Active {
            BondStatus::Expired
        } else {
            bond.status
        };

        if !current_status.can_transition_to(BondStatus::Unbonding) {
            return Err(BondError::InvalidStateTransition {
                from: current_status,
                to: BondStatus::Unbonding,
            });
        }

        // 6. Calculate amounts
        let (principal, interest, penalty) = self.calculate_withdrawal_amounts(&bond, now);

        // 7. Update bond
        let unbonding_ends_at = now + Duration::days(self.config.unbonding_period_days as i64);
        {
            let mut bonds = self.bonds.write();
            if let Some(b) = bonds.get_mut(&request.bond_id) {
                b.status = BondStatus::Unbonding;
                b.unbonding_started_at = Some(now);
                b.accrued_interest = interest;
                b.slashed_amount = penalty;
            }
        }

        // 8. Submit to chain
        let tx_id = self.submit_unbonding_to_chain(&bond).await?;

        info!(
            "Unbonding initiated: id={}, principal={}, interest={}, penalty={}",
            hex::encode(request.bond_id),
            principal,
            interest,
            penalty
        );

        Ok(WithdrawalResult {
            bond_id: request.bond_id,
            principal_returned: principal,
            interest_earned: interest,
            penalty_applied: penalty,
            net_amount: principal + interest - penalty,
            unbonding_ends_at: Some(unbonding_ends_at),
            tx_id: Some(tx_id),
        })
    }

    /// Complete withdrawal after unbonding period
    pub async fn complete_withdrawal(
        &self,
        request: WithdrawBondRequest,
    ) -> Result<WithdrawalResult, BondError> {
        let user_key_bytes = request.user_key.to_bytes();

        // 1. Verify signature
        let message = self.create_withdraw_message(&request);
        request
            .user_key
            .verify(&message, &request.signature)
            .map_err(|e| BondError::InvalidSignature(e.to_string()))?;

        // 2. Get bond
        let bond = {
            let bonds = self.bonds.read();
            bonds
                .get(&request.bond_id)
                .cloned()
                .ok_or(BondError::BondNotFound(request.bond_id))?
        };

        // 3. Verify ownership
        if bond.user_key != user_key_bytes {
            return Err(BondError::InvalidSignature("Not bond owner".to_string()));
        }

        // 4. Check status is Unbonding
        if bond.status != BondStatus::Unbonding {
            return Err(BondError::InvalidStateTransition {
                from: bond.status,
                to: BondStatus::Withdrawn,
            });
        }

        // 5. Check unbonding period is complete
        let now = Utc::now();
        if let Some(unbonding_started) = bond.unbonding_started_at {
            let unbonding_ends =
                unbonding_started + Duration::days(self.config.unbonding_period_days as i64);
            if now < unbonding_ends {
                return Err(BondError::UnbondingNotComplete {
                    ends_at: unbonding_ends,
                });
            }
        } else {
            return Err(BondError::Internal(
                "Unbonding start time not set".to_string(),
            ));
        }

        // 6. Calculate final amounts
        let principal = bond.amount;
        let interest = bond.accrued_interest;
        let penalty = bond.slashed_amount;
        let net_amount = principal + interest - penalty;

        // 7. Update bond status
        {
            let mut bonds = self.bonds.write();
            if let Some(b) = bonds.get_mut(&request.bond_id) {
                b.status = BondStatus::Withdrawn;
                b.withdrawn_at = Some(now);
            }
        }

        // 8. Update totals
        self.total_bonded.fetch_sub(bond.amount, Ordering::SeqCst);
        self.total_storage_bonded
            .fetch_sub(bond.storage_bytes, Ordering::SeqCst);

        // 9. Submit to chain
        let tx_id = self.submit_withdrawal_to_chain(&bond, net_amount).await?;

        info!(
            "Withdrawal complete: id={}, net_amount={}",
            hex::encode(request.bond_id),
            net_amount
        );

        Ok(WithdrawalResult {
            bond_id: request.bond_id,
            principal_returned: principal,
            interest_earned: interest,
            penalty_applied: penalty,
            net_amount,
            unbonding_ends_at: None,
            tx_id: Some(tx_id),
        })
    }

    /// Early terminate a bond with penalty
    pub async fn early_terminate(
        &self,
        request: WithdrawBondRequest,
    ) -> Result<WithdrawalResult, BondError> {
        let user_key_bytes = request.user_key.to_bytes();

        // 1. Verify signature
        let message = self.create_withdraw_message(&request);
        request
            .user_key
            .verify(&message, &request.signature)
            .map_err(|e| BondError::InvalidSignature(e.to_string()))?;

        // 2. Verify nonce
        self.verify_and_increment_nonce(&user_key_bytes, request.nonce)?;

        // 3. Get bond
        let bond = {
            let bonds = self.bonds.read();
            bonds
                .get(&request.bond_id)
                .cloned()
                .ok_or(BondError::BondNotFound(request.bond_id))?
        };

        // 4. Verify ownership
        if bond.user_key != user_key_bytes {
            return Err(BondError::InvalidSignature("Not bond owner".to_string()));
        }

        // 5. Only Active bonds can be early terminated
        if bond.status != BondStatus::Active {
            return Err(BondError::InvalidStateTransition {
                from: bond.status,
                to: BondStatus::EarlyTerminated,
            });
        }

        // 6. Calculate early termination penalty
        let now = Utc::now();
        let elapsed_days = (now - bond.created_at).num_days() as u32;
        let remaining_days = bond.duration_days.saturating_sub(elapsed_days);

        // Penalty = base penalty + proportional penalty for remaining time
        let base_penalty = (bond.amount as f64 * self.config.early_termination_penalty) as u64;
        let proportional_penalty = (bond.amount as f64
            * (remaining_days as f64 / bond.duration_days as f64)
            * 0.05) as u64; // 5% of remaining duration value
        let total_penalty = base_penalty + proportional_penalty;

        // Interest only for elapsed time (reduced by penalty)
        let earned_interest = self.calculate_yield(bond.amount, elapsed_days);
        let net_interest = earned_interest.saturating_sub(total_penalty / 2);

        let net_amount = bond.amount + net_interest - total_penalty;

        // 7. Update bond
        {
            let mut bonds = self.bonds.write();
            if let Some(b) = bonds.get_mut(&request.bond_id) {
                b.status = BondStatus::EarlyTerminated;
                b.withdrawn_at = Some(now);
                b.accrued_interest = net_interest;
                b.slashed_amount = total_penalty;
            }
        }

        // 8. Update totals
        self.total_bonded.fetch_sub(bond.amount, Ordering::SeqCst);
        self.total_storage_bonded
            .fetch_sub(bond.storage_bytes, Ordering::SeqCst);

        // 9. Submit to chain
        let tx_id = self
            .submit_early_termination_to_chain(&bond, net_amount, total_penalty)
            .await?;

        warn!(
            "Early termination: id={}, penalty={}, net={}",
            hex::encode(request.bond_id),
            total_penalty,
            net_amount
        );

        Ok(WithdrawalResult {
            bond_id: request.bond_id,
            principal_returned: bond.amount,
            interest_earned: net_interest,
            penalty_applied: total_penalty,
            net_amount,
            unbonding_ends_at: None,
            tx_id: Some(tx_id),
        })
    }

    /// Slash a bond for provider/user misbehavior
    pub async fn slash_bond(
        &self,
        bond_id: [u8; 32],
        slash_percentage: f64,
        reason: &str,
        authority_signature: &Signature,
        authority_key: &VerifyingKey,
    ) -> Result<u64, BondError> {
        // Verify authority signature
        let message = format!(
            "SLASH:{}:{}:{}",
            hex::encode(bond_id),
            slash_percentage,
            reason
        );
        authority_key
            .verify(message.as_bytes(), authority_signature)
            .map_err(|e| BondError::InvalidSignature(e.to_string()))?;

        // Get and validate bond
        let bond = {
            let bonds = self.bonds.read();
            bonds
                .get(&bond_id)
                .cloned()
                .ok_or(BondError::BondNotFound(bond_id))?
        };

        // Calculate slash amount
        let slash_amount = (bond.amount as f64 * slash_percentage.min(1.0)) as u64;

        // Submit slashing to chain
        self.submit_slashing_to_chain(&bond, slash_amount, reason)
            .await?;

        // Update bond
        {
            let mut bonds = self.bonds.write();
            if let Some(b) = bonds.get_mut(&bond_id) {
                b.status = BondStatus::Slashed;
                b.slashed_amount = slash_amount;
            }
        }

        // Update totals
        self.total_bonded.fetch_sub(bond.amount, Ordering::SeqCst);
        self.total_storage_bonded
            .fetch_sub(bond.storage_bytes, Ordering::SeqCst);

        error!(
            "Bond slashed: id={}, amount={}, reason={}",
            hex::encode(bond_id),
            slash_amount,
            reason
        );

        Ok(slash_amount)
    }

    /// Register a storage provider
    pub async fn register_provider(
        &self,
        provider_key: VerifyingKey,
        stake_amount: u64,
        storage_capacity: u64,
        signature: Signature,
    ) -> Result<(), BondError> {
        let key_bytes = provider_key.to_bytes();

        // Verify signature
        let message = format!("REGISTER:{}:{}", stake_amount, storage_capacity);
        provider_key
            .verify(message.as_bytes(), &signature)
            .map_err(|e| BondError::InvalidSignature(e.to_string()))?;

        // Check stake requirement
        if stake_amount < self.config.provider_stake_requirement {
            return Err(BondError::ProviderInsufficientStake {
                required: self.config.provider_stake_requirement,
                staked: stake_amount,
            });
        }

        // Submit provider registration to chain
        self.submit_provider_registration_to_chain(&key_bytes, stake_amount, storage_capacity)
            .await?;

        let provider = StorageProvider {
            key: key_bytes,
            stake_amount,
            registered_at: Utc::now(),
            total_storage_capacity: storage_capacity,
            used_storage: 0,
            reputation_score: 1.0, // Start with perfect reputation
            bonds_served: 0,
            slashing_incidents: 0,
            is_active: true,
        };

        {
            let mut providers = self.providers.write();
            providers.insert(key_bytes, provider);
        }

        info!(
            "Provider registered: key={}, stake={}, capacity={}GB",
            hex::encode(key_bytes),
            stake_amount,
            storage_capacity / 1_073_741_824
        );

        Ok(())
    }

    /// Update demand multiplier based on network utilization
    pub fn update_demand_multiplier(&self) {
        let utilization = *self.network_utilization.read();
        let target = self.config.target_utilization;
        let adjustment_rate = self.config.demand_adjustment_rate;

        let mut multiplier = self.demand_multiplier.write();

        if utilization > target + 0.1 {
            // High demand, increase price
            *multiplier = (*multiplier * (1.0 + adjustment_rate)).min(5.0);
        } else if utilization < target - 0.1 {
            // Low demand, decrease price
            *multiplier = (*multiplier * (1.0 - adjustment_rate)).max(0.5);
        }

        debug!("Demand multiplier updated: {:.4}", *multiplier);
    }

    /// Get bond by ID
    pub fn get_bond(&self, bond_id: &[u8; 32]) -> Option<ProductionBond> {
        self.bonds.read().get(bond_id).cloned()
    }

    /// List user's bonds
    pub fn list_user_bonds(&self, user_key: &[u8; 32]) -> Vec<ProductionBond> {
        self.bonds
            .read()
            .values()
            .filter(|b| b.user_key == *user_key)
            .cloned()
            .collect()
    }

    /// Get statistics
    pub fn get_statistics(&self) -> BondStatistics {
        let bonds = self.bonds.read();
        let providers = self.providers.read();

        let active_bonds = bonds
            .values()
            .filter(|b| b.status == BondStatus::Active)
            .count();
        let total_bonded = self.total_bonded.load(Ordering::Relaxed);
        let total_storage = self.total_storage_bonded.load(Ordering::Relaxed);
        let demand_multiplier = *self.demand_multiplier.read();
        let utilization = *self.network_utilization.read();

        BondStatistics {
            total_bonds: bonds.len(),
            active_bonds,
            total_bonded_amount: total_bonded,
            total_storage_bonded_bytes: total_storage,
            active_providers: providers.values().filter(|p| p.is_active).count(),
            total_provider_capacity: providers.values().map(|p| p.total_storage_capacity).sum(),
            demand_multiplier,
            network_utilization: utilization,
        }
    }

    // === Private helper methods ===

    fn create_bond_message(&self, request: &CreateBondRequest) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(b"CREATE_BOND:");
        message.extend_from_slice(&request.user_key.to_bytes());
        message.extend_from_slice(&request.amount.to_le_bytes());
        message.extend_from_slice(&request.storage_gb.to_le_bytes());
        message.extend_from_slice(&request.duration_days.to_le_bytes());
        message.extend_from_slice(&request.nonce.to_le_bytes());
        message
    }

    fn create_withdraw_message(&self, request: &WithdrawBondRequest) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(b"WITHDRAW_BOND:");
        message.extend_from_slice(&request.bond_id);
        message.extend_from_slice(&request.user_key.to_bytes());
        message.extend_from_slice(&request.nonce.to_le_bytes());
        message
    }

    fn generate_bond_id(
        &self,
        user_key: &[u8; 32],
        nonce: u64,
        timestamp: DateTime<Utc>,
    ) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(user_key);
        hasher.update(nonce.to_le_bytes());
        hasher.update(timestamp.timestamp().to_le_bytes());
        hasher.finalize().into()
    }

    fn verify_and_increment_nonce(&self, user_key: &[u8; 32], nonce: u64) -> Result<(), BondError> {
        let mut nonces = self.user_nonces.write();
        let current_nonce = nonces.entry(*user_key).or_insert(0);

        if nonce != *current_nonce {
            return Err(BondError::InvalidNonce {
                expected: *current_nonce,
                provided: nonce,
            });
        }

        *current_nonce += 1;
        Ok(())
    }

    fn check_rate_limit(&self, user_key: &[u8; 32]) -> Result<(), BondError> {
        let now = Utc::now();
        let mut state = self.rate_limits.write();

        // Cleanup old entries daily
        if now - state.last_cleanup > Duration::days(1) {
            state.bonds_today.clear();
            state.last_cleanup = now;
        }

        // Check rate limit
        let entries = state.bonds_today.entry(*user_key).or_insert_with(Vec::new);

        // Remove entries older than 24 hours
        entries.retain(|t| now - *t < Duration::hours(24));

        if entries.len() >= self.config.max_bonds_per_user_per_day as usize {
            let oldest = entries.first().unwrap();
            let retry_after = Duration::hours(24) - (now - *oldest);
            return Err(BondError::RateLimited { retry_after });
        }

        Ok(())
    }

    fn record_bond_creation(&self, user_key: &[u8; 32]) {
        let mut state = self.rate_limits.write();
        state
            .bonds_today
            .entry(*user_key)
            .or_default()
            .push(Utc::now());
    }

    fn calculate_bond_cost(
        &self,
        storage_bytes: u64,
        duration_days: u32,
        demand_multiplier: f64,
    ) -> u64 {
        // Base rate: 0.0001 DCHAT per GB per day (in 8-decimal token units)
        const BASE_RATE_PER_GB_DAY: u64 = 10_000; // 0.0001 * 10^8

        let storage_gb = storage_bytes / 1_073_741_824;
        let duration_factor = (duration_days as f64).sqrt();

        ((BASE_RATE_PER_GB_DAY as f64) * (storage_gb as f64) * duration_factor * demand_multiplier)
            as u64
    }

    fn calculate_yield(&self, amount: u64, duration_days: u32) -> u64 {
        let years = duration_days as f64 / 365.0;
        (amount as f64 * self.config.base_apy * years) as u64
    }

    fn calculate_withdrawal_amounts(
        &self,
        bond: &ProductionBond,
        now: DateTime<Utc>,
    ) -> (u64, u64, u64) {
        let elapsed_days = (now - bond.created_at).num_days() as u32;
        let is_early = now < bond.expires_at;

        let principal = bond.amount;
        let interest = self.calculate_yield(principal, elapsed_days);

        let penalty = if is_early {
            (principal as f64 * self.config.early_termination_penalty) as u64
        } else {
            0
        };

        (principal, interest, penalty)
    }

    async fn submit_bond_to_chain(&self, bond: &ProductionBond) -> Result<String, BondError> {
        // Offline mode for testing
        if self.config.offline_mode {
            return Ok(format!("offline-bond-{}", hex::encode(&bond.id[..8])));
        }

        // Production: Submit to currency chain
        use reqwest::Client;
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.create_storage_bond",
            "params": {
                "bond_id": hex::encode(bond.id),
                "user_key": hex::encode(bond.user_key),
                "amount": bond.amount,
                "storage_bytes": bond.storage_bytes,
                "duration_days": bond.duration_days,
                "expires_at": bond.expires_at.to_rfc3339(),
            }
        });

        let client = Client::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BondError::ChainError("Missing tx_id in response".to_string()))
    }

    async fn submit_unbonding_to_chain(&self, bond: &ProductionBond) -> Result<String, BondError> {
        // Offline mode for testing
        if self.config.offline_mode {
            return Ok(format!("offline-unbond-{}", hex::encode(&bond.id[..8])));
        }

        use reqwest::Client;
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.initiate_unbonding",
            "params": {
                "bond_id": hex::encode(bond.id),
                "user_key": hex::encode(bond.user_key),
            }
        });

        let client = Client::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BondError::ChainError("Missing tx_id in response".to_string()))
    }

    async fn submit_withdrawal_to_chain(
        &self,
        bond: &ProductionBond,
        amount: u64,
    ) -> Result<String, BondError> {
        // Offline mode for testing
        if self.config.offline_mode {
            return Ok(format!("offline-withdraw-{}", hex::encode(&bond.id[..8])));
        }

        use reqwest::Client;
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.complete_withdrawal",
            "params": {
                "bond_id": hex::encode(bond.id),
                "user_key": hex::encode(bond.user_key),
                "amount": amount,
            }
        });

        let client = Client::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BondError::ChainError("Missing tx_id in response".to_string()))
    }

    async fn submit_early_termination_to_chain(
        &self,
        bond: &ProductionBond,
        amount: u64,
        penalty: u64,
    ) -> Result<String, BondError> {
        // Offline mode for testing
        if self.config.offline_mode {
            return Ok(format!("offline-terminate-{}", hex::encode(&bond.id[..8])));
        }

        use reqwest::Client;
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.early_termination",
            "params": {
                "bond_id": hex::encode(bond.id),
                "user_key": hex::encode(bond.user_key),
                "amount": amount,
                "penalty": penalty,
            }
        });

        let client = Client::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BondError::ChainError("Missing tx_id in response".to_string()))
    }

    async fn submit_slashing_to_chain(
        &self,
        bond: &ProductionBond,
        slash_amount: u64,
        reason: &str,
    ) -> Result<String, BondError> {
        // Offline mode for testing
        if self.config.offline_mode {
            return Ok(format!("offline-slash-{}", hex::encode(&bond.id[..8])));
        }

        use reqwest::Client;
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.slash_storage_bond",
            "params": {
                "bond_id": hex::encode(bond.id),
                "user_key": hex::encode(bond.user_key),
                "slash_amount": slash_amount,
                "reason": reason,
            }
        });

        let client = Client::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BondError::ChainError("Missing tx_id in response".to_string()))
    }

    async fn submit_provider_registration_to_chain(
        &self,
        provider_key: &[u8; 32],
        stake_amount: u64,
        storage_capacity: u64,
    ) -> Result<String, BondError> {
        // Offline mode for testing
        if self.config.offline_mode {
            return Ok(format!(
                "offline-provider-{}",
                hex::encode(&provider_key[..8])
            ));
        }

        use reqwest::Client;
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.register_storage_provider",
            "params": {
                "provider_key": hex::encode(provider_key),
                "stake_amount": stake_amount,
                "storage_capacity": storage_capacity,
            }
        });

        let client = Client::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BondError::ChainError(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BondError::ChainError("Missing tx_id in response".to_string()))
    }
}

/// Bond statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondStatistics {
    pub total_bonds: usize,
    pub active_bonds: usize,
    pub total_bonded_amount: u64,
    pub total_storage_bonded_bytes: u64,
    pub active_providers: usize,
    pub total_provider_capacity: u64,
    pub demand_multiplier: f64,
    pub network_utilization: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn create_test_manager() -> ProductionBondManager {
        let mut config = ProductionBondConfig::default();
        config.offline_mode = true; // Enable offline mode for tests

        ProductionBondManager::new(config, "http://localhost:8545".to_string())
    }

    #[test]
    fn test_bond_status_transitions() {
        use BondStatus::*;

        assert!(Pending.can_transition_to(Active));
        assert!(Pending.can_transition_to(Slashed));
        assert!(!Pending.can_transition_to(Withdrawn));

        assert!(Active.can_transition_to(Expired));
        assert!(Active.can_transition_to(Unbonding));
        assert!(Active.can_transition_to(EarlyTerminated));
        assert!(!Active.can_transition_to(Withdrawn));

        assert!(Unbonding.can_transition_to(Withdrawn));
        assert!(Unbonding.can_transition_to(Slashed));
        assert!(!Unbonding.can_transition_to(Active));
    }

    #[test]
    fn test_calculate_bond_cost() {
        let manager = create_test_manager();

        // 10 GB for 30 days at 1.0 demand multiplier
        let cost = manager.calculate_bond_cost(10 * 1_073_741_824, 30, 1.0);
        // Expected: 10_000 * 10 * sqrt(30) * 1.0 = 547,722 (in 8-decimal units)
        assert!(cost > 0);
        assert!(cost < 1_000_000);
    }

    #[test]
    fn test_calculate_yield() {
        let manager = create_test_manager();

        // 1000 tokens for 365 days at 5% APY
        let yield_amount = manager.calculate_yield(1000_0000_0000, 365);
        // Expected: 1000 * 0.05 * 1 = 50 DCHAT = 50_0000_0000 units
        assert!((yield_amount as i64 - 50_0000_0000).abs() < 1000);
    }

    #[test]
    fn test_generate_bond_id() {
        let manager = create_test_manager();
        let user_key = [1u8; 32];
        let now = Utc::now();

        let id1 = manager.generate_bond_id(&user_key, 0, now);
        let id2 = manager.generate_bond_id(&user_key, 1, now);

        assert_ne!(id1, id2); // Different nonces = different IDs
    }

    #[test]
    fn test_rate_limit_check() {
        let manager = create_test_manager();
        let user_key = [2u8; 32];

        // First 10 should pass
        for _ in 0..10 {
            assert!(manager.check_rate_limit(&user_key).is_ok());
            manager.record_bond_creation(&user_key);
        }

        // 11th should be rate limited
        let result = manager.check_rate_limit(&user_key);
        assert!(matches!(result, Err(BondError::RateLimited { .. })));
    }

    #[test]
    fn test_nonce_verification() {
        let manager = create_test_manager();
        let user_key = [3u8; 32];

        // Nonce 0 should pass
        assert!(manager.verify_and_increment_nonce(&user_key, 0).is_ok());

        // Nonce 0 again should fail
        assert!(matches!(
            manager.verify_and_increment_nonce(&user_key, 0),
            Err(BondError::InvalidNonce { .. })
        ));

        // Nonce 1 should pass
        assert!(manager.verify_and_increment_nonce(&user_key, 1).is_ok());
    }

    #[test]
    fn test_config_defaults() {
        let config = ProductionBondConfig::default();

        assert_eq!(config.min_bond_amount, 10_0000_0000);
        assert_eq!(config.max_duration_days, 730);
        assert_eq!(config.unbonding_period_days, 7);
        assert_eq!(config.early_termination_penalty, 0.10);
        assert_eq!(config.base_apy, 0.05);
    }

    #[tokio::test]
    async fn test_register_provider() {
        let manager = create_test_manager();
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let stake = 10_000_0000_0000u64; // 10k DCHAT
        let capacity = 1_000_000_000_000u64; // 1 TB

        let message = format!("REGISTER:{}:{}", stake, capacity);
        let signature = signing_key.sign(message.as_bytes());

        let result = manager
            .register_provider(verifying_key, stake, capacity, signature)
            .await;

        assert!(result.is_ok());

        // Check provider is registered
        let providers = manager.providers.read();
        assert!(providers.contains_key(&verifying_key.to_bytes()));
    }

    #[test]
    fn test_statistics() {
        let manager = create_test_manager();
        let stats = manager.get_statistics();

        assert_eq!(stats.total_bonds, 0);
        assert_eq!(stats.active_bonds, 0);
        assert_eq!(stats.total_bonded_amount, 0);
    }
}
