//! Fee Distribution and Reward Accounting
//!
//! This module implements deterministic fee distribution and reward accounting for dchat.
//!
//! # Fee Flow Architecture
//!
//! All fees collected by the protocol are distributed according to the following split
//! (derived from PRODUCTION_IMPROVEMENTS_ROADMAP.md Section 16.1):
//!
//! - **Validators**: 70% - Block producers and consensus participants
//! - **Relays**: 20% - Message relay operators
//! - **Treasury**: 10% - Protocol development and operations
//!
//! # Burn Policy
//!
//! The 1% burn rate (from TokenSupplyConfig.burn_rate_bps) is applied to:
//! - Standard token transfers between users
//! - NOT applied to: fee distributions, reward payouts, internal accounting transfers
//!
//! This ensures that reward recipients receive the intended net amount.
//!
//! # Consensus Verification
//!
//! All fee distributions are recorded in `BlockFeeAccounting` which is committed
//! to the state root, making fee routing consensus-verifiable.

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

// =============================================================================
// FEE DISTRIBUTION CONSTANTS
// Derived from: PRODUCTION_IMPROVEMENTS_ROADMAP.md Section 16.1
//
// After burn, protocol fees are split as follows (must sum to 10000 bps = 100%):
//   Validator: 68% | Relay: 20% | Treasury: 10% | Insurance: 2%
// =============================================================================

/// Validator share of protocol fees (68%)
/// Validators receive the largest share for block production and consensus
pub const VALIDATOR_FEE_SHARE_BPS: u16 = 6800;

/// Relay share of protocol fees (20%)
/// Relay operators receive payment for message delivery
pub const RELAY_FEE_SHARE_BPS: u16 = 2000;

/// Treasury share of protocol fees (10%)
/// Treasury funds protocol development and operations
pub const TREASURY_FEE_SHARE_BPS: u16 = 1000;

/// Insurance fund share of protocol fees (2%)
/// Insurance fund provides user protection against losses
pub const INSURANCE_FUND_ALLOCATION_BPS: u16 = 200;

/// Burn rate for standard transfers (1%)
/// Applied to user-to-user transfers, NOT to fee distributions
/// From TokenSupplyConfig.burn_rate_bps default
pub const DEFAULT_BURN_RATE_BPS: u16 = 100;

/// Minimum reward payout in motes (1 DCHAT = 100_000_000 motes)
/// Rewards below this threshold are accumulated until they exceed it
pub const MIN_REWARD_PAYOUT_MOTES: u64 = 1_000_000; // 0.01 DCHAT

/// Verify fee shares sum to exactly 100% (10000 basis points)
const _: () = {
    assert!(
        VALIDATOR_FEE_SHARE_BPS
            + RELAY_FEE_SHARE_BPS
            + TREASURY_FEE_SHARE_BPS
            + INSURANCE_FUND_ALLOCATION_BPS
            == 10000
    );
};

// =============================================================================
// POOL STATE ACCOUNTING (Consensus-Verifiable)
// =============================================================================

/// Pool types in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PoolType {
    /// Validator reward pool - 68% of protocol fees
    #[default]
    ValidatorRewards,
    /// Relay reward pool - 20% of protocol fees
    RelayRewards,
    /// Treasury pool - 10% of protocol fees
    Treasury,
    /// Insurance fund - 2% of protocol fees for user protection
    InsuranceFund,
    /// Storage bond pool - collateral for storage commitments
    StorageBonds,
    /// Burn sink - tracked but removed from circulating supply
    BurnSink,
}

impl PoolType {
    /// Get all pool types (excluding burn sink for circulating supply)
    pub fn circulating_pools() -> &'static [PoolType] {
        &[
            PoolType::ValidatorRewards,
            PoolType::RelayRewards,
            PoolType::Treasury,
            PoolType::InsuranceFund,
            PoolType::StorageBonds,
        ]
    }

    /// Get all pool types including burn sink
    pub fn all_pools() -> &'static [PoolType] {
        &[
            PoolType::ValidatorRewards,
            PoolType::RelayRewards,
            PoolType::Treasury,
            PoolType::InsuranceFund,
            PoolType::StorageBonds,
            PoolType::BurnSink,
        ]
    }

    /// Get deterministic sink address for this pool
    pub fn sink_address(&self) -> UserId {
        match self {
            PoolType::Treasury => {
                UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
            }
            PoolType::ValidatorRewards => {
                UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap())
            }
            PoolType::RelayRewards => {
                UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap())
            }
            PoolType::BurnSink => {
                UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap())
            }
            PoolType::InsuranceFund => {
                UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000005").unwrap())
            }
            PoolType::StorageBonds => {
                UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000006").unwrap())
            }
        }
    }
}

/// State of a single reward/fee pool
///
/// All amounts are in motes (smallest unit, 8 decimals).
/// This struct is serialized into state root for consensus verification.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolState {
    /// Pool type identifier
    pub pool_type: Option<PoolType>,

    /// Current balance in motes (8 decimals)
    /// Invariant: balance >= pending_distributions
    pub balance: u64,

    /// Pending distributions (allocated but not yet claimed/paid out)
    pub pending_distributions: u64,

    /// Total deposited all-time (for audit trail)
    pub total_deposited: u64,

    /// Total withdrawn all-time (for audit trail)
    pub total_withdrawn: u64,

    /// Current epoch accrual (reset each epoch)
    pub epoch_accrual: u64,

    /// Last epoch when distribution occurred
    pub last_distribution_epoch: u64,

    /// State version (for encoding compatibility)
    pub version: u8,
}

impl PoolState {
    /// Create new pool state for a pool type
    pub fn new(pool_type: PoolType) -> Self {
        Self {
            pool_type: Some(pool_type),
            balance: 0,
            pending_distributions: 0,
            total_deposited: 0,
            total_withdrawn: 0,
            epoch_accrual: 0,
            last_distribution_epoch: 0,
            version: 1,
        }
    }

    /// Deposit funds into the pool with checked arithmetic
    ///
    /// Returns Err if overflow would occur.
    pub fn deposit(&mut self, amount: u64) -> Result<()> {
        self.balance = self.balance.checked_add(amount).ok_or_else(|| {
            Error::validation(format!(
                "Pool balance overflow: {} + {} exceeds u64",
                self.balance, amount
            ))
        })?;

        self.total_deposited = self.total_deposited.checked_add(amount).ok_or_else(|| {
            Error::validation(format!(
                "Pool total_deposited overflow: {} + {} exceeds u64",
                self.total_deposited, amount
            ))
        })?;

        self.epoch_accrual = self.epoch_accrual.checked_add(amount).ok_or_else(|| {
            Error::validation(format!(
                "Pool epoch_accrual overflow: {} + {} exceeds u64",
                self.epoch_accrual, amount
            ))
        })?;

        Ok(())
    }

    /// Withdraw funds from the pool with checked arithmetic
    ///
    /// Returns Err if underflow would occur.
    pub fn withdraw(&mut self, amount: u64) -> Result<()> {
        if amount > self.balance {
            return Err(Error::validation(format!(
                "Pool balance underflow: {} - {} would be negative",
                self.balance, amount
            )));
        }

        // Safe: we checked above
        self.balance -= amount;

        self.total_withdrawn = self.total_withdrawn.checked_add(amount).ok_or_else(|| {
            Error::validation(format!(
                "Pool total_withdrawn overflow: {} + {} exceeds u64",
                self.total_withdrawn, amount
            ))
        })?;

        Ok(())
    }

    /// Allocate pending distribution (marks funds as committed but not paid out)
    pub fn allocate_pending(&mut self, amount: u64) -> Result<()> {
        // Check we have sufficient uncommitted balance
        let available = self.balance.saturating_sub(self.pending_distributions);
        if amount > available {
            return Err(Error::validation(format!(
                "Insufficient uncommitted balance: available={}, requested={}",
                available, amount
            )));
        }

        self.pending_distributions =
            self.pending_distributions
                .checked_add(amount)
                .ok_or_else(|| {
                    Error::validation(format!(
                        "Pending distributions overflow: {} + {} exceeds u64",
                        self.pending_distributions, amount
                    ))
                })?;

        Ok(())
    }

    /// Clear pending distribution after payout
    pub fn clear_pending(&mut self, amount: u64) -> Result<()> {
        if amount > self.pending_distributions {
            return Err(Error::validation(format!(
                "Pending distributions underflow: {} - {} would be negative",
                self.pending_distributions, amount
            )));
        }

        self.pending_distributions -= amount;
        Ok(())
    }

    /// Reset epoch accrual (called at end of epoch)
    pub fn reset_epoch_accrual(&mut self, current_epoch: u64) {
        self.epoch_accrual = 0;
        self.last_distribution_epoch = current_epoch;
    }

    /// Available balance (not pending distribution)
    pub fn available_balance(&self) -> u64 {
        self.balance.saturating_sub(self.pending_distributions)
    }

    /// Compute state hash for consensus verification
    pub fn compute_hash(&self) -> [u8; 32] {
        use blake3::Hasher;
        let mut hasher = Hasher::new();

        hasher.update(&[self.version]);
        hasher.update(&self.balance.to_le_bytes());
        hasher.update(&self.pending_distributions.to_le_bytes());
        hasher.update(&self.total_deposited.to_le_bytes());
        hasher.update(&self.total_withdrawn.to_le_bytes());
        hasher.update(&self.epoch_accrual.to_le_bytes());
        hasher.update(&self.last_distribution_epoch.to_le_bytes());

        hasher.finalize().into()
    }
}

/// Unified pool state for all protocol pools
///
/// This aggregates all pool states and provides conservation invariants.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnifiedPoolState {
    /// Individual pool states
    pub pools: HashMap<PoolType, PoolState>,

    /// Total circulating supply (user wallets + all pools except burn)
    pub total_circulating: u64,

    /// Total burned all-time
    pub total_burned: u64,

    /// Genesis supply (immutable baseline)
    pub genesis_supply: u64,

    /// Current epoch
    pub current_epoch: u64,

    /// State version for encoding compatibility
    pub version: u8,
}

impl UnifiedPoolState {
    /// Create new unified pool state with genesis supply
    pub fn new(genesis_supply: u64) -> Self {
        let mut pools = HashMap::new();

        // Initialize all pool types
        for pool_type in PoolType::all_pools() {
            pools.insert(*pool_type, PoolState::new(*pool_type));
        }

        Self {
            pools,
            total_circulating: genesis_supply,
            total_burned: 0,
            genesis_supply,
            current_epoch: 0,
            version: 1,
        }
    }

    /// Get pool state by type
    pub fn get_pool(&self, pool_type: PoolType) -> Option<&PoolState> {
        self.pools.get(&pool_type)
    }

    /// Get mutable pool state
    pub fn get_pool_mut(&mut self, pool_type: PoolType) -> Option<&mut PoolState> {
        self.pools.get_mut(&pool_type)
    }

    /// Deposit to pool with conservation check
    pub fn deposit_to_pool(&mut self, pool_type: PoolType, amount: u64) -> Result<()> {
        let pool = self
            .pools
            .get_mut(&pool_type)
            .ok_or_else(|| Error::validation(format!("Unknown pool type: {:?}", pool_type)))?;

        pool.deposit(amount)?;

        // For burn sink, update total_burned and reduce circulating
        if pool_type == PoolType::BurnSink {
            self.total_burned = self.total_burned.checked_add(amount).ok_or_else(|| {
                Error::validation(format!(
                    "Total burned overflow: {} + {} exceeds u64",
                    self.total_burned, amount
                ))
            })?;

            // Reduce circulating supply by burn amount
            if amount > self.total_circulating {
                return Err(Error::validation(format!(
                    "Burn exceeds circulating: {} > {}",
                    amount, self.total_circulating
                )));
            }
            self.total_circulating -= amount;
        }

        Ok(())
    }

    /// Withdraw from pool with conservation check
    pub fn withdraw_from_pool(&mut self, pool_type: PoolType, amount: u64) -> Result<()> {
        if pool_type == PoolType::BurnSink {
            return Err(Error::validation(
                "Cannot withdraw from burn sink".to_string(),
            ));
        }

        let pool = self
            .pools
            .get_mut(&pool_type)
            .ok_or_else(|| Error::validation(format!("Unknown pool type: {:?}", pool_type)))?;

        pool.withdraw(amount)?;
        Ok(())
    }

    /// Verify conservation of value
    ///
    /// Invariant: genesis_supply = total_circulating + total_burned
    /// Invariant: sum(pool_balances excluding burn) <= total_circulating
    pub fn verify_conservation(&self) -> Result<()> {
        // Check genesis = circulating + burned
        let expected_genesis = self
            .total_circulating
            .checked_add(self.total_burned)
            .ok_or_else(|| {
                Error::validation("Conservation overflow: circulating + burned exceeds u64")
            })?;

        if expected_genesis != self.genesis_supply {
            return Err(Error::validation(format!(
                "Conservation violation: genesis={} but circulating+burned={}",
                self.genesis_supply, expected_genesis
            )));
        }

        // Check burn sink balance matches total_burned
        if let Some(burn_pool) = self.pools.get(&PoolType::BurnSink) {
            if burn_pool.balance != self.total_burned {
                return Err(Error::validation(format!(
                    "Burn sink mismatch: pool_balance={} but total_burned={}",
                    burn_pool.balance, self.total_burned
                )));
            }
        }

        Ok(())
    }

    /// Compute merkle root of all pool states for consensus
    pub fn compute_state_root(&self) -> [u8; 32] {
        use blake3::Hasher;

        let mut pool_hashes: Vec<[u8; 32]> = PoolType::all_pools()
            .iter()
            .filter_map(|pt| self.pools.get(pt))
            .map(|ps| ps.compute_hash())
            .collect();

        // Add metadata
        let mut hasher = Hasher::new();
        hasher.update(&[self.version]);
        hasher.update(&self.total_circulating.to_le_bytes());
        hasher.update(&self.total_burned.to_le_bytes());
        hasher.update(&self.genesis_supply.to_le_bytes());
        hasher.update(&self.current_epoch.to_le_bytes());
        pool_hashes.push(hasher.finalize().into());

        // Build merkle tree
        while pool_hashes.len() > 1 {
            let mut next_level = Vec::new();
            for chunk in pool_hashes.chunks(2) {
                let mut hasher = Hasher::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                next_level.push(hasher.finalize().into());
            }
            pool_hashes = next_level;
        }

        pool_hashes.first().copied().unwrap_or([0u8; 32])
    }

    /// Advance to next epoch
    pub fn advance_epoch(&mut self) -> u64 {
        self.current_epoch += 1;

        // Reset epoch accruals for all pools
        for pool in self.pools.values_mut() {
            pool.reset_epoch_accrual(self.current_epoch);
        }

        self.current_epoch
    }
}

/// Pool delta for a single block (for state transition encoding)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolDelta {
    /// Pool type
    pub pool_type: PoolType,
    /// Amount deposited this block
    pub deposited: u64,
    /// Amount withdrawn this block
    pub withdrawn: u64,
    /// Net change (can be negative, stored as signed)
    pub net_change: i64,
}

impl PoolDelta {
    /// Create delta from deposit/withdraw amounts
    pub fn new(pool_type: PoolType, deposited: u64, withdrawn: u64) -> Self {
        let net_change = deposited as i64 - withdrawn as i64;
        Self {
            pool_type,
            deposited,
            withdrawn,
            net_change,
        }
    }
}

// =============================================================================
// PROTOCOL FEE SINK ADDRESSES
// These are deterministic addresses used for protocol-level accounting
// =============================================================================

/// Well-known protocol fee sink addresses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolSinks {
    /// Treasury pool - receives 10% of protocol fees
    /// Funds protocol development, audits, operations
    pub treasury: UserId,

    /// Validator reward pool - receives 70% of protocol fees
    /// Distributed to validators proportional to stake
    pub validator_pool: UserId,

    /// Relay reward pool - receives 20% of protocol fees
    /// Distributed to relays proportional to uptime and messages
    pub relay_pool: UserId,

    /// Burn sink - tokens sent here are burned (tracked but removed from supply)
    pub burn_sink: UserId,

    /// Insurance fund - receives portion of fees for user protection
    pub insurance_fund: UserId,

    /// Storage bond pool - collateral for storage commitments
    pub storage_bonds: UserId,
}

impl Default for ProtocolSinks {
    fn default() -> Self {
        // Use deterministic UUIDs derived from PoolType for consistency
        Self {
            treasury: PoolType::Treasury.sink_address(),
            validator_pool: PoolType::ValidatorRewards.sink_address(),
            relay_pool: PoolType::RelayRewards.sink_address(),
            burn_sink: PoolType::BurnSink.sink_address(),
            insurance_fund: PoolType::InsuranceFund.sink_address(),
            storage_bonds: PoolType::StorageBonds.sink_address(),
        }
    }
}

impl ProtocolSinks {
    /// Check if an address is a protocol sink (internal accounting address)
    pub fn is_protocol_sink(&self, user_id: &UserId) -> bool {
        *user_id == self.treasury
            || *user_id == self.validator_pool
            || *user_id == self.relay_pool
            || *user_id == self.burn_sink
            || *user_id == self.insurance_fund
            || *user_id == self.storage_bonds
    }

    /// Get pool type for a sink address
    pub fn pool_type_for_sink(&self, user_id: &UserId) -> Option<PoolType> {
        if *user_id == self.treasury {
            Some(PoolType::Treasury)
        } else if *user_id == self.validator_pool {
            Some(PoolType::ValidatorRewards)
        } else if *user_id == self.relay_pool {
            Some(PoolType::RelayRewards)
        } else if *user_id == self.burn_sink {
            Some(PoolType::BurnSink)
        } else if *user_id == self.insurance_fund {
            Some(PoolType::InsuranceFund)
        } else if *user_id == self.storage_bonds {
            Some(PoolType::StorageBonds)
        } else {
            None
        }
    }
}

// =============================================================================
// FEE COLLECTION RECORD
// =============================================================================

/// Type of fee collected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeeType {
    /// Fee for sending a message
    MessageFee,
    /// Fee for token transfer (protocol fee, not burn)
    TransferFee,
    /// Fee for creating a channel
    ChannelCreationFee,
    /// Fee for premium features
    PremiumFeatureFee,
    /// Fee from marketplace transactions
    MarketplaceFee,
    /// Fee from storage micropayments
    StorageFee,
    /// Execution gas fees (program runtime)
    ExecutionGasFee,
    /// Program runtime fees (smart contract execution)
    ProgramRuntimeFee,
    /// Staking/delegation operation fees
    StakingOperationFee,
}

/// Record of a single fee collection event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeCollectionRecord {
    /// Unique record ID
    pub id: Uuid,
    /// Type of fee
    pub fee_type: FeeType,
    /// Gross amount paid by user
    pub gross_amount: u64,
    /// Amount burned (if applicable)
    pub burn_amount: u64,
    /// Amount to validator pool
    pub validator_share: u64,
    /// Amount to relay pool
    pub relay_share: u64,
    /// Amount to treasury
    pub treasury_share: u64,
    /// Amount to insurance fund
    pub insurance_share: u64,
    /// Direct recipient (e.g., specific relay for message fee)
    pub direct_recipient: Option<UserId>,
    /// Amount to direct recipient (net after protocol fee)
    pub direct_recipient_amount: u64,
    /// User who paid the fee
    pub payer: UserId,
    /// Block height when fee was collected
    pub block_height: u64,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Associated transaction ID
    pub tx_id: Uuid,
}

// =============================================================================
// BLOCK FEE ACCOUNTING (Consensus-Verifiable)
// =============================================================================

/// Block-level fee accounting for consensus verification
///
/// This structure is committed to the state root to make fee routing
/// deterministically verifiable by all validators.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlockFeeAccounting {
    /// State encoding version (for forward compatibility)
    pub version: u8,

    /// Block height
    pub block_height: u64,
    /// Block timestamp
    pub timestamp: DateTime<Utc>,

    // ---- Fee Totals ----
    /// Total fees collected in this block
    pub total_fees_collected: u64,
    /// Total amount burned in this block
    pub total_burned: u64,
    /// Total distributed to validator pool
    pub total_to_validator_pool: u64,
    /// Total distributed to relay pool
    pub total_to_relay_pool: u64,
    /// Total distributed to treasury
    pub total_to_treasury: u64,
    /// Total distributed to insurance fund
    pub total_to_insurance_fund: u64,
    /// Total direct payments to relays (message fees)
    pub total_direct_to_relays: u64,

    // ---- Individual Records ----
    /// Individual fee collection records (for audit)
    pub fee_records: Vec<FeeCollectionRecord>,

    // ---- Pool Deltas ----
    /// Per-pool balance changes in this block (for consensus verification)
    pub pool_deltas: Vec<PoolDelta>,

    // ---- Conservation Checks ----
    /// Pre-block total supply (for conservation verification)
    pub pre_block_supply: u64,
    /// Post-block total supply (should equal pre - burned)
    pub post_block_supply: u64,

    // ---- Merkle Commitment ----
    /// Merkle root of all fee records (for light client verification)
    pub fee_records_merkle_root: [u8; 32],
    /// Merkle root of pool states (for consensus verification)
    pub pool_state_root: [u8; 32],
}

impl BlockFeeAccounting {
    /// Create new block fee accounting
    pub fn new(block_height: u64, pre_block_supply: u64) -> Self {
        Self {
            version: 2, // Version 2 includes pool deltas and insurance
            block_height,
            timestamp: Utc::now(),
            pre_block_supply,
            post_block_supply: pre_block_supply,
            pool_deltas: Vec::new(),
            pool_state_root: [0u8; 32],
            ..Default::default()
        }
    }

    /// Add a fee collection record
    pub fn add_fee_record(&mut self, record: FeeCollectionRecord) {
        self.total_fees_collected = self
            .total_fees_collected
            .saturating_add(record.gross_amount);
        self.total_burned = self.total_burned.saturating_add(record.burn_amount);
        self.total_to_validator_pool = self
            .total_to_validator_pool
            .saturating_add(record.validator_share);
        self.total_to_relay_pool = self.total_to_relay_pool.saturating_add(record.relay_share);
        self.total_to_treasury = self.total_to_treasury.saturating_add(record.treasury_share);
        self.total_to_insurance_fund = self
            .total_to_insurance_fund
            .saturating_add(record.insurance_share);
        self.total_direct_to_relays = self
            .total_direct_to_relays
            .saturating_add(record.direct_recipient_amount);

        // Update post-block supply (subtract burns)
        self.post_block_supply = self.post_block_supply.saturating_sub(record.burn_amount);

        self.fee_records.push(record);
    }

    /// Compute pool deltas from fee records
    pub fn compute_pool_deltas(&mut self) {
        self.pool_deltas.clear();

        // Validator pool delta
        if self.total_to_validator_pool > 0 {
            self.pool_deltas.push(PoolDelta::new(
                PoolType::ValidatorRewards,
                self.total_to_validator_pool,
                0,
            ));
        }

        // Relay pool delta
        if self.total_to_relay_pool > 0 {
            self.pool_deltas.push(PoolDelta::new(
                PoolType::RelayRewards,
                self.total_to_relay_pool,
                0,
            ));
        }

        // Treasury delta
        if self.total_to_treasury > 0 {
            self.pool_deltas.push(PoolDelta::new(
                PoolType::Treasury,
                self.total_to_treasury,
                0,
            ));
        }

        // Insurance fund delta
        if self.total_to_insurance_fund > 0 {
            self.pool_deltas.push(PoolDelta::new(
                PoolType::InsuranceFund,
                self.total_to_insurance_fund,
                0,
            ));
        }

        // Burn sink delta
        if self.total_burned > 0 {
            self.pool_deltas
                .push(PoolDelta::new(PoolType::BurnSink, self.total_burned, 0));
        }
    }

    /// Verify conservation of value
    ///
    /// This ensures that:
    /// 1. gross_amount = burn + validator_share + relay_share + treasury_share + insurance_share + direct_recipient
    /// 2. post_block_supply = pre_block_supply - total_burned
    pub fn verify_conservation(&self) -> Result<()> {
        // Check each record
        for record in &self.fee_records {
            let sum = record.burn_amount
                + record.validator_share
                + record.relay_share
                + record.treasury_share
                + record.insurance_share
                + record.direct_recipient_amount;

            if sum != record.gross_amount {
                return Err(Error::validation(format!(
                    "Fee record {} violates conservation: gross={} but sum of parts={}",
                    record.id, record.gross_amount, sum
                )));
            }
        }

        // Check block-level conservation
        let expected_post = self.pre_block_supply.saturating_sub(self.total_burned);
        if self.post_block_supply != expected_post {
            return Err(Error::validation(format!(
                "Block {} violates supply conservation: expected {} but got {}",
                self.block_height, expected_post, self.post_block_supply
            )));
        }

        Ok(())
    }

    /// Compute merkle root of fee records
    pub fn compute_merkle_root(&mut self) {
        use blake3::Hasher;

        if self.fee_records.is_empty() {
            self.fee_records_merkle_root = [0u8; 32];
            return;
        }

        // Hash each record
        let mut hashes: Vec<[u8; 32]> = self
            .fee_records
            .iter()
            .map(|r| {
                let mut hasher = Hasher::new();
                hasher.update(&r.id.as_bytes()[..]);
                hasher.update(&r.gross_amount.to_le_bytes());
                hasher.update(&r.burn_amount.to_le_bytes());
                hasher.update(&r.validator_share.to_le_bytes());
                hasher.update(&r.relay_share.to_le_bytes());
                hasher.update(&r.treasury_share.to_le_bytes());
                hasher.finalize().into()
            })
            .collect();

        // Build merkle tree
        while hashes.len() > 1 {
            let mut next_level = Vec::new();
            for chunk in hashes.chunks(2) {
                let mut hasher = Hasher::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]); // Duplicate for odd count
                }
                next_level.push(hasher.finalize().into());
            }
            hashes = next_level;
        }

        self.fee_records_merkle_root = hashes[0];
    }
}

// =============================================================================
// FEE DISTRIBUTION MANAGER
// =============================================================================

/// Fee distribution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeDistributionConfig {
    /// Validator share in basis points (default: 7000 = 70%)
    pub validator_share_bps: u16,
    /// Relay share in basis points (default: 2000 = 20%)
    pub relay_share_bps: u16,
    /// Treasury share in basis points (default: 1000 = 10%)
    pub treasury_share_bps: u16,
    /// Burn rate in basis points (default: 100 = 1%)
    pub burn_rate_bps: u16,
    /// Insurance fund allocation in basis points (default: 200 = 2%)
    /// This is deducted from treasury share
    pub insurance_fund_bps: u16,
    /// Whether to apply burn to reward distributions (default: false)
    pub burn_on_rewards: bool,
    /// Whether to apply burn to internal pool transfers (default: false)
    pub burn_on_internal: bool,
    /// Genesis supply for conservation tracking
    pub genesis_supply: u64,
}

impl Default for FeeDistributionConfig {
    fn default() -> Self {
        Self {
            validator_share_bps: VALIDATOR_FEE_SHARE_BPS,
            relay_share_bps: RELAY_FEE_SHARE_BPS,
            treasury_share_bps: TREASURY_FEE_SHARE_BPS,
            burn_rate_bps: DEFAULT_BURN_RATE_BPS,
            insurance_fund_bps: INSURANCE_FUND_ALLOCATION_BPS,
            burn_on_rewards: false,
            burn_on_internal: false,
            genesis_supply: 100_000_000_000_0000_0000, // 100B tokens with 8 decimals
        }
    }
}

/// Manager for fee distribution and pool accounting
pub struct FeeDistributionManager {
    config: FeeDistributionConfig,
    sinks: ProtocolSinks,
    /// Unified pool state (consensus-verifiable)
    unified_pool_state: Arc<RwLock<UnifiedPoolState>>,
    /// Pool balances (sink address -> balance) - legacy compat
    pool_balances: Arc<RwLock<HashMap<UserId, u64>>>,
    /// Current block accounting
    current_block_accounting: Arc<RwLock<BlockFeeAccounting>>,
    /// Historical block accounting (block_height -> accounting)
    block_history: Arc<RwLock<HashMap<u64, BlockFeeAccounting>>>,
    /// Current block height
    current_block: Arc<RwLock<u64>>,
    /// Total burned all-time
    total_burned_all_time: Arc<RwLock<u64>>,
    /// Current epoch
    current_epoch: Arc<RwLock<u64>>,
}

impl FeeDistributionManager {
    /// Create new fee distribution manager
    pub fn new(config: FeeDistributionConfig) -> Self {
        let sinks = ProtocolSinks::default();
        let mut pool_balances = HashMap::new();

        // Initialize pool balances to zero (legacy - for backward compat)
        pool_balances.insert(sinks.treasury.clone(), 0);
        pool_balances.insert(sinks.validator_pool.clone(), 0);
        pool_balances.insert(sinks.relay_pool.clone(), 0);
        pool_balances.insert(sinks.burn_sink.clone(), 0);
        pool_balances.insert(sinks.insurance_fund.clone(), 0);
        pool_balances.insert(sinks.storage_bonds.clone(), 0);

        // Initialize unified pool state
        let unified_pool_state = UnifiedPoolState::new(config.genesis_supply);

        Self {
            config,
            sinks,
            unified_pool_state: Arc::new(RwLock::new(unified_pool_state)),
            pool_balances: Arc::new(RwLock::new(pool_balances)),
            current_block_accounting: Arc::new(RwLock::new(BlockFeeAccounting::new(0, 0))),
            block_history: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(0)),
            total_burned_all_time: Arc::new(RwLock::new(0)),
            current_epoch: Arc::new(RwLock::new(0)),
        }
    }

    /// Create manager with custom genesis supply
    pub fn with_genesis_supply(mut config: FeeDistributionConfig, genesis_supply: u64) -> Self {
        config.genesis_supply = genesis_supply;
        Self::new(config)
    }

    /// Get protocol sinks
    pub fn sinks(&self) -> &ProtocolSinks {
        &self.sinks
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> u64 {
        *self.current_epoch.read().unwrap()
    }

    /// Advance to next epoch and reset accruals
    pub fn advance_epoch(&self) -> u64 {
        let mut epoch = self.current_epoch.write().unwrap();
        *epoch += 1;

        // Reset epoch accruals in unified state
        let mut state = self.unified_pool_state.write().unwrap();
        state.advance_epoch();

        *epoch
    }

    /// Get unified pool state snapshot
    pub fn get_unified_pool_state(&self) -> UnifiedPoolState {
        self.unified_pool_state.read().unwrap().clone()
    }

    /// Get pool balance by type
    pub fn get_pool_balance_by_type(&self, pool_type: PoolType) -> u64 {
        let state = self.unified_pool_state.read().unwrap();
        state.get_pool(pool_type).map(|p| p.balance).unwrap_or(0)
    }

    /// Get pool epoch accrual by type
    pub fn get_pool_epoch_accrual(&self, pool_type: PoolType) -> u64 {
        let state = self.unified_pool_state.read().unwrap();
        state
            .get_pool(pool_type)
            .map(|p| p.epoch_accrual)
            .unwrap_or(0)
    }

    /// Verify conservation invariants
    pub fn verify_conservation(&self) -> Result<()> {
        self.unified_pool_state
            .read()
            .unwrap()
            .verify_conservation()
    }

    /// Get pool state root for consensus
    pub fn compute_pool_state_root(&self) -> [u8; 32] {
        self.unified_pool_state.read().unwrap().compute_state_root()
    }

    /// Start a new block for fee accounting
    pub fn start_block(&self, block_height: u64, current_supply: u64) {
        // Finalize previous block
        let prev_block = *self.current_block.read().unwrap();
        if prev_block > 0 {
            let mut current = self.current_block_accounting.write().unwrap();
            current.compute_merkle_root();
            current.compute_pool_deltas();
            current.pool_state_root = self.compute_pool_state_root();
            let finalized = current.clone();
            drop(current);

            // Store in history
            self.block_history
                .write()
                .unwrap()
                .insert(prev_block, finalized);
        }

        // Start new block
        *self.current_block.write().unwrap() = block_height;
        *self.current_block_accounting.write().unwrap() =
            BlockFeeAccounting::new(block_height, current_supply);
    }

    /// Calculate fee distribution for a given gross amount
    ///
    /// # Fee Split (after burn and direct recipient)
    /// - Validator: 68% (VALIDATOR_FEE_SHARE_BPS)
    /// - Relay Pool: 20% (RELAY_FEE_SHARE_BPS)  
    /// - Treasury: 10% (TREASURY_FEE_SHARE_BPS)
    /// - Insurance: 2% (INSURANCE_FUND_ALLOCATION_BPS)
    /// - Total: 100%
    ///
    /// # Arguments
    /// * `gross_amount` - Total fee amount paid by user (in motes)
    /// * `apply_burn` - Whether to apply 1% burn rate
    /// * `direct_recipient_share_bps` - Optional basis points for direct recipient (e.g., relay for message fees)
    ///
    /// # Returns
    /// Tuple of (burn, validator_share, relay_share, treasury_share, insurance_share, direct_recipient_amount)
    /// All amounts are in motes.
    pub fn calculate_distribution(
        &self,
        gross_amount: u64,
        apply_burn: bool,
        direct_recipient_share_bps: Option<u16>,
    ) -> (u64, u64, u64, u64, u64, u64) {
        // Use u128 for intermediate calculations to prevent overflow with large amounts
        let gross_u128 = gross_amount as u128;

        // Step 1: Calculate burn (if applicable) - 1% of gross
        let burn = if apply_burn {
            ((gross_u128 * self.config.burn_rate_bps as u128) / 10000) as u64
        } else {
            0
        };

        let after_burn = gross_amount.saturating_sub(burn);
        let after_burn_u128 = after_burn as u128;

        // Step 2: Calculate direct recipient share (e.g., 100% message fee to relay)
        let direct_share = if let Some(bps) = direct_recipient_share_bps {
            ((after_burn_u128 * bps as u128) / 10000) as u64
        } else {
            0
        };

        let protocol_portion = after_burn.saturating_sub(direct_share);
        let protocol_u128 = protocol_portion as u128;

        // Step 3: Split protocol portion according to config (68/20/10/2 = 100%)
        // All shares are calculated from the same base to ensure they sum correctly
        let validator_share =
            ((protocol_u128 * self.config.validator_share_bps as u128) / 10000) as u64;
        let relay_share = ((protocol_u128 * self.config.relay_share_bps as u128) / 10000) as u64;
        let insurance_share =
            ((protocol_u128 * self.config.insurance_fund_bps as u128) / 10000) as u64;

        // Treasury gets remainder to absorb rounding dust and ensure conservation
        let treasury_share = protocol_portion
            .saturating_sub(validator_share)
            .saturating_sub(relay_share)
            .saturating_sub(insurance_share);

        (
            burn,
            validator_share,
            relay_share,
            treasury_share,
            insurance_share,
            direct_share,
        )
    }

    /// Calculate distribution preserving backward compatibility (5-tuple)
    ///
    /// Returns (burn, validator_share, relay_share, treasury_share + insurance, direct_recipient_amount)
    pub fn calculate_distribution_compat(
        &self,
        gross_amount: u64,
        apply_burn: bool,
        direct_recipient_share_bps: Option<u16>,
    ) -> (u64, u64, u64, u64, u64) {
        let (burn, validator, relay, treasury, insurance, direct) =
            self.calculate_distribution(gross_amount, apply_burn, direct_recipient_share_bps);
        // Combine treasury + insurance for backward compat
        (burn, validator, relay, treasury + insurance, direct)
    }

    /// Record a fee collection with full distribution
    ///
    /// This is the main entry point for collecting fees with proper accounting.
    pub fn collect_fee(
        &self,
        fee_type: FeeType,
        gross_amount: u64,
        payer: UserId,
        direct_recipient: Option<UserId>,
        tx_id: Uuid,
    ) -> Result<FeeCollectionRecord> {
        // Determine if burn applies and direct recipient share
        let (apply_burn, direct_share_bps) = match fee_type {
            // Message fees: 100% goes to relay, no burn (relay is service provider)
            FeeType::MessageFee => (false, Some(10000)), // 100% to direct recipient

            // Transfer fees: burn applies, remaining split to pools
            FeeType::TransferFee => (true, None),

            // Channel creation: no burn, split to pools
            FeeType::ChannelCreationFee => (false, None),

            // Marketplace: burn applies, 90% to creator (direct), 10% to pools
            FeeType::MarketplaceFee => (true, Some(9000)), // 90% to creator

            // Storage/Premium: no burn, split to pools
            FeeType::StorageFee | FeeType::PremiumFeatureFee => (false, None),

            // Execution/Program fees: no burn, split to pools
            FeeType::ExecutionGasFee | FeeType::ProgramRuntimeFee => (false, None),

            // Staking operation fees: no burn, split to pools
            FeeType::StakingOperationFee => (false, None),
        };

        let (burn, validator_share, relay_share, treasury_share, insurance_share, direct_amount) =
            self.calculate_distribution(gross_amount, apply_burn, direct_share_bps);

        // Update unified pool state with checked arithmetic
        {
            let mut state = self.unified_pool_state.write().unwrap();

            if validator_share > 0 {
                state.deposit_to_pool(PoolType::ValidatorRewards, validator_share)?;
            }
            if relay_share > 0 {
                state.deposit_to_pool(PoolType::RelayRewards, relay_share)?;
            }
            if treasury_share > 0 {
                state.deposit_to_pool(PoolType::Treasury, treasury_share)?;
            }
            if insurance_share > 0 {
                state.deposit_to_pool(PoolType::InsuranceFund, insurance_share)?;
            }
            if burn > 0 {
                state.deposit_to_pool(PoolType::BurnSink, burn)?;
            }
        }

        // Update legacy pool balances for backward compat
        {
            let mut pools = self.pool_balances.write().unwrap();
            if let Some(balance) = pools.get_mut(&self.sinks.validator_pool) {
                *balance = balance.saturating_add(validator_share);
            }
            if let Some(balance) = pools.get_mut(&self.sinks.relay_pool) {
                *balance = balance.saturating_add(relay_share);
            }
            if let Some(balance) = pools.get_mut(&self.sinks.treasury) {
                *balance = balance.saturating_add(treasury_share);
            }
            if let Some(balance) = pools.get_mut(&self.sinks.insurance_fund) {
                *balance = balance.saturating_add(insurance_share);
            }
            if let Some(balance) = pools.get_mut(&self.sinks.burn_sink) {
                *balance = balance.saturating_add(burn);
            }
        }

        // Update total burned
        if burn > 0 {
            let mut burned = self.total_burned_all_time.write().unwrap();
            *burned = burned.saturating_add(burn);
        }

        // Create record
        let record = FeeCollectionRecord {
            id: Uuid::new_v4(),
            fee_type,
            gross_amount,
            burn_amount: burn,
            validator_share,
            relay_share,
            treasury_share,
            insurance_share,
            direct_recipient,
            direct_recipient_amount: direct_amount,
            payer,
            block_height: *self.current_block.read().unwrap(),
            timestamp: Utc::now(),
            tx_id,
        };

        // Add to block accounting
        self.current_block_accounting
            .write()
            .unwrap()
            .add_fee_record(record.clone());

        Ok(record)
    }

    /// Get pool balance (legacy API)
    pub fn get_pool_balance(&self, pool: &UserId) -> u64 {
        *self.pool_balances.read().unwrap().get(pool).unwrap_or(&0)
    }

    /// Withdraw from a pool for distribution (uses unified state)
    ///
    /// This debits the pool and returns the amount for distribution.
    /// Used when actually paying out rewards to validators/relays.
    pub fn withdraw_from_pool(&self, pool: &UserId, amount: u64) -> Result<u64> {
        // Determine pool type from sink address
        let pool_type = self
            .sinks
            .pool_type_for_sink(pool)
            .ok_or_else(|| Error::validation("Unknown pool sink address"))?;

        // Withdraw from unified state
        {
            let mut state = self.unified_pool_state.write().unwrap();
            state.withdraw_from_pool(pool_type, amount)?;
        }

        // Update legacy balances
        {
            let mut pools = self.pool_balances.write().unwrap();
            let balance = pools
                .get_mut(pool)
                .ok_or_else(|| Error::validation("Unknown pool"))?;

            if *balance < amount {
                return Err(Error::validation(format!(
                    "Insufficient pool balance: have {}, need {}",
                    *balance, amount
                )));
            }

            *balance -= amount;
        }

        Ok(amount)
    }

    /// Distribute rewards from a pool to multiple recipients
    ///
    /// This is a deterministic distribution that:
    /// 1. Allocates the total_amount from the pool as pending
    /// 2. Returns the per-recipient distribution
    pub fn distribute_rewards(
        &self,
        pool_type: PoolType,
        recipients: &[(UserId, u64)], // (recipient, share_weight)
        total_amount: u64,
    ) -> Result<Vec<(UserId, u64)>> {
        // Calculate total weight
        let total_weight: u64 = recipients.iter().map(|(_, w)| *w).sum();
        if total_weight == 0 {
            return Ok(Vec::new());
        }

        // Allocate from pool
        {
            let mut state = self.unified_pool_state.write().unwrap();
            let pool = state
                .get_pool_mut(pool_type)
                .ok_or_else(|| Error::validation(format!("Unknown pool type: {:?}", pool_type)))?;

            if pool.available_balance() < total_amount {
                return Err(Error::validation(format!(
                    "Insufficient available balance in {:?}: have {}, need {}",
                    pool_type,
                    pool.available_balance(),
                    total_amount
                )));
            }

            pool.allocate_pending(total_amount)?;
        }

        // Calculate per-recipient distribution
        let mut distributions = Vec::with_capacity(recipients.len());
        let mut distributed = 0u64;

        for (i, (recipient, weight)) in recipients.iter().enumerate() {
            let share = if i == recipients.len() - 1 {
                // Last recipient gets remainder to avoid rounding dust
                total_amount.saturating_sub(distributed)
            } else {
                ((total_amount as u128 * *weight as u128) / total_weight as u128) as u64
            };

            distributed = distributed.saturating_add(share);
            distributions.push((recipient.clone(), share));
        }

        Ok(distributions)
    }

    /// Complete a pending distribution (after actual payout)
    pub fn complete_distribution(&self, pool_type: PoolType, amount: u64) -> Result<()> {
        let mut state = self.unified_pool_state.write().unwrap();

        // Clear pending
        let pool = state
            .get_pool_mut(pool_type)
            .ok_or_else(|| Error::validation(format!("Unknown pool type: {:?}", pool_type)))?;
        pool.clear_pending(amount)?;

        // Withdraw the actual amount
        pool.withdraw(amount)?;

        Ok(())
    }

    /// Get current block fee accounting
    pub fn get_current_block_accounting(&self) -> BlockFeeAccounting {
        self.current_block_accounting.read().unwrap().clone()
    }

    /// Get historical block fee accounting
    pub fn get_block_accounting(&self, block_height: u64) -> Option<BlockFeeAccounting> {
        self.block_history
            .read()
            .unwrap()
            .get(&block_height)
            .cloned()
    }

    /// Get total burned all-time
    pub fn get_total_burned(&self) -> u64 {
        *self.total_burned_all_time.read().unwrap()
    }

    /// Verify current block accounting
    pub fn verify_current_block(&self) -> Result<()> {
        self.current_block_accounting
            .read()
            .unwrap()
            .verify_conservation()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_distribution_config_defaults() {
        let config = FeeDistributionConfig::default();
        // 68% + 20% + 10% + 2% = 100%
        assert_eq!(config.validator_share_bps, 6800); // 68%
        assert_eq!(config.relay_share_bps, 2000); // 20%
        assert_eq!(config.treasury_share_bps, 1000); // 10%
        assert_eq!(config.insurance_fund_bps, 200); // 2%
        assert_eq!(config.burn_rate_bps, 100); // 1% burn on transfers
                                               // All four shares must sum to exactly 100%
        assert_eq!(
            config.validator_share_bps
                + config.relay_share_bps
                + config.treasury_share_bps
                + config.insurance_fund_bps,
            10000
        );
    }

    #[test]
    fn test_protocol_sinks_are_deterministic() {
        let sinks1 = ProtocolSinks::default();
        let sinks2 = ProtocolSinks::default();
        assert_eq!(sinks1.treasury, sinks2.treasury);
        assert_eq!(sinks1.validator_pool, sinks2.validator_pool);
        assert_eq!(sinks1.relay_pool, sinks2.relay_pool);
        assert_eq!(sinks1.burn_sink, sinks2.burn_sink);
        assert_eq!(sinks1.insurance_fund, sinks2.insurance_fund);
        assert_eq!(sinks1.storage_bonds, sinks2.storage_bonds);
    }

    #[test]
    fn test_calculate_distribution_with_burn() {
        let manager = FeeDistributionManager::new(FeeDistributionConfig::default());

        // 1000 motes with 1% burn
        let (burn, validator, relay, treasury, insurance, direct) =
            manager.calculate_distribution(1000, true, None);

        assert_eq!(burn, 10); // 1% burn = 10 motes
        let after_burn = 990; // 990 motes remaining

        // All shares calculated from after_burn (68% + 20% + 10% + 2% = 100%)
        let expected_validator = (after_burn * VALIDATOR_FEE_SHARE_BPS as u64) / 10000; // 68%
        let expected_relay = (after_burn * RELAY_FEE_SHARE_BPS as u64) / 10000; // 20%
        let expected_insurance = (after_burn * INSURANCE_FUND_ALLOCATION_BPS as u64) / 10000; // 2%
        let expected_treasury =
            after_burn - expected_validator - expected_relay - expected_insurance; // 10% + dust

        assert_eq!(validator, expected_validator);
        assert_eq!(relay, expected_relay);
        assert_eq!(insurance, expected_insurance);
        assert_eq!(treasury, expected_treasury);
        assert_eq!(direct, 0);

        // Verify conservation: all shares must sum to original 1000 motes
        assert_eq!(
            burn + validator + relay + treasury + insurance + direct,
            1000
        );
    }

    #[test]
    fn test_calculate_distribution_message_fee() {
        let manager = FeeDistributionManager::new(FeeDistributionConfig::default());

        // Message fee: 100% to relay, no burn
        let (burn, validator, relay, treasury, insurance, direct) =
            manager.calculate_distribution(1000, false, Some(10000));

        assert_eq!(burn, 0);
        assert_eq!(validator, 0);
        assert_eq!(relay, 0);
        assert_eq!(treasury, 0);
        assert_eq!(insurance, 0);
        assert_eq!(direct, 1000); // 100% to relay

        // Verify conservation
        assert_eq!(
            burn + validator + relay + treasury + insurance + direct,
            1000
        );
    }

    #[test]
    fn test_collect_fee_updates_pools() {
        let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
        manager.start_block(1, 1_000_000);

        let payer = UserId(Uuid::new_v4());
        let record = manager
            .collect_fee(FeeType::TransferFee, 1000, payer, None, Uuid::new_v4())
            .unwrap();

        // Verify pools were credited
        assert!(manager.get_pool_balance(&manager.sinks.validator_pool) > 0);
        assert!(manager.get_pool_balance(&manager.sinks.relay_pool) > 0);
        assert!(manager.get_pool_balance(&manager.sinks.treasury) > 0);
        assert_eq!(
            manager.get_pool_balance(&manager.sinks.burn_sink),
            record.burn_amount
        );
    }

    #[test]
    fn test_block_accounting_conservation() {
        let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
        manager.start_block(1, 1_000_000);

        let payer = UserId(Uuid::new_v4());

        // Collect several fees
        for _ in 0..5 {
            manager
                .collect_fee(
                    FeeType::TransferFee,
                    1000,
                    payer.clone(),
                    None,
                    Uuid::new_v4(),
                )
                .unwrap();
        }

        // Verify conservation
        manager.verify_current_block().unwrap();
    }

    #[test]
    fn test_message_fee_no_burn() {
        let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
        manager.start_block(1, 1_000_000);

        let payer = UserId(Uuid::new_v4());
        let relay = UserId(Uuid::new_v4());

        let record = manager
            .collect_fee(
                FeeType::MessageFee,
                1000,
                payer,
                Some(relay),
                Uuid::new_v4(),
            )
            .unwrap();

        // Message fees should NOT be burned
        assert_eq!(record.burn_amount, 0);
        // Relay should get 100%
        assert_eq!(record.direct_recipient_amount, 1000);
    }

    #[test]
    fn test_pool_withdrawal() {
        let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
        manager.start_block(1, 1_000_000);

        let payer = UserId(Uuid::new_v4());
        manager
            .collect_fee(FeeType::TransferFee, 10000, payer, None, Uuid::new_v4())
            .unwrap();

        let treasury_balance = manager.get_pool_balance(&manager.sinks.treasury);
        assert!(treasury_balance > 0);

        // Withdraw half
        let withdrawn = manager
            .withdraw_from_pool(&manager.sinks.treasury, treasury_balance / 2)
            .unwrap();
        assert_eq!(withdrawn, treasury_balance / 2);

        // Verify remaining balance
        let remaining = manager.get_pool_balance(&manager.sinks.treasury);
        assert_eq!(remaining, treasury_balance - withdrawn);
    }
}
