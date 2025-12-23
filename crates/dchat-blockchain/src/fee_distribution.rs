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
// =============================================================================

/// Validator share of protocol fees (70%)
pub const VALIDATOR_FEE_SHARE_BPS: u16 = 7000;

/// Relay share of protocol fees (20%)
pub const RELAY_FEE_SHARE_BPS: u16 = 2000;

/// Treasury share of protocol fees (10%)
pub const TREASURY_FEE_SHARE_BPS: u16 = 1000;

/// Burn rate for standard transfers (1%)
/// From TokenSupplyConfig.burn_rate_bps default
pub const DEFAULT_BURN_RATE_BPS: u16 = 100;

/// Verify fee shares sum to 100%
const _: () = {
    assert!(VALIDATOR_FEE_SHARE_BPS + RELAY_FEE_SHARE_BPS + TREASURY_FEE_SHARE_BPS == 10000);
};

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
}

impl Default for ProtocolSinks {
    fn default() -> Self {
        // Use deterministic UUIDs derived from protocol constants
        // These are well-known addresses that can be verified by any node
        Self {
            treasury: UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()),
            validator_pool: UserId(
                Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
            ),
            relay_pool: UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap()),
            burn_sink: UserId(Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap()),
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
    /// Total direct payments to relays (message fees)
    pub total_direct_to_relays: u64,

    // ---- Individual Records ----
    /// Individual fee collection records (for audit)
    pub fee_records: Vec<FeeCollectionRecord>,

    // ---- Conservation Checks ----
    /// Pre-block total supply (for conservation verification)
    pub pre_block_supply: u64,
    /// Post-block total supply (should equal pre - burned)
    pub post_block_supply: u64,

    // ---- Merkle Commitment ----
    /// Merkle root of all fee records (for light client verification)
    pub fee_records_merkle_root: [u8; 32],
}

impl BlockFeeAccounting {
    /// Create new block fee accounting
    pub fn new(block_height: u64, pre_block_supply: u64) -> Self {
        Self {
            block_height,
            timestamp: Utc::now(),
            pre_block_supply,
            post_block_supply: pre_block_supply,
            ..Default::default()
        }
    }

    /// Add a fee collection record
    pub fn add_fee_record(&mut self, record: FeeCollectionRecord) {
        self.total_fees_collected += record.gross_amount;
        self.total_burned += record.burn_amount;
        self.total_to_validator_pool += record.validator_share;
        self.total_to_relay_pool += record.relay_share;
        self.total_to_treasury += record.treasury_share;
        self.total_direct_to_relays += record.direct_recipient_amount;

        // Update post-block supply (subtract burns)
        self.post_block_supply = self.post_block_supply.saturating_sub(record.burn_amount);

        self.fee_records.push(record);
    }

    /// Verify conservation of value
    ///
    /// This ensures that:
    /// 1. gross_amount = burn + validator_share + relay_share + treasury_share + direct_recipient
    /// 2. post_block_supply = pre_block_supply - total_burned
    pub fn verify_conservation(&self) -> Result<()> {
        // Check each record
        for record in &self.fee_records {
            let sum = record.burn_amount
                + record.validator_share
                + record.relay_share
                + record.treasury_share
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
    /// Whether to apply burn to reward distributions (default: false)
    pub burn_on_rewards: bool,
    /// Whether to apply burn to internal pool transfers (default: false)
    pub burn_on_internal: bool,
}

impl Default for FeeDistributionConfig {
    fn default() -> Self {
        Self {
            validator_share_bps: VALIDATOR_FEE_SHARE_BPS,
            relay_share_bps: RELAY_FEE_SHARE_BPS,
            treasury_share_bps: TREASURY_FEE_SHARE_BPS,
            burn_rate_bps: DEFAULT_BURN_RATE_BPS,
            burn_on_rewards: false,
            burn_on_internal: false,
        }
    }
}

/// Manager for fee distribution and pool accounting
pub struct FeeDistributionManager {
    config: FeeDistributionConfig,
    sinks: ProtocolSinks,
    /// Pool balances (sink address -> balance)
    pool_balances: Arc<RwLock<HashMap<UserId, u64>>>,
    /// Current block accounting
    current_block_accounting: Arc<RwLock<BlockFeeAccounting>>,
    /// Historical block accounting (block_height -> accounting)
    block_history: Arc<RwLock<HashMap<u64, BlockFeeAccounting>>>,
    /// Current block height
    current_block: Arc<RwLock<u64>>,
    /// Total burned all-time
    total_burned_all_time: Arc<RwLock<u64>>,
}

impl FeeDistributionManager {
    /// Create new fee distribution manager
    pub fn new(config: FeeDistributionConfig) -> Self {
        let sinks = ProtocolSinks::default();
        let mut pool_balances = HashMap::new();

        // Initialize pool balances to zero
        pool_balances.insert(sinks.treasury.clone(), 0);
        pool_balances.insert(sinks.validator_pool.clone(), 0);
        pool_balances.insert(sinks.relay_pool.clone(), 0);
        pool_balances.insert(sinks.burn_sink.clone(), 0);

        Self {
            config,
            sinks,
            pool_balances: Arc::new(RwLock::new(pool_balances)),
            current_block_accounting: Arc::new(RwLock::new(BlockFeeAccounting::new(0, 0))),
            block_history: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(0)),
            total_burned_all_time: Arc::new(RwLock::new(0)),
        }
    }

    /// Get protocol sinks
    pub fn sinks(&self) -> &ProtocolSinks {
        &self.sinks
    }

    /// Start a new block for fee accounting
    pub fn start_block(&self, block_height: u64, current_supply: u64) {
        // Finalize previous block
        let prev_block = *self.current_block.read().unwrap();
        if prev_block > 0 {
            let mut current = self.current_block_accounting.write().unwrap();
            current.compute_merkle_root();
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
    /// # Arguments
    /// * `gross_amount` - Total fee amount paid by user
    /// * `apply_burn` - Whether to apply burn rate
    /// * `direct_recipient_share_bps` - Optional basis points for direct recipient (e.g., relay)
    ///
    /// # Returns
    /// Tuple of (burn, validator_share, relay_share, treasury_share, direct_recipient_amount)
    pub fn calculate_distribution(
        &self,
        gross_amount: u64,
        apply_burn: bool,
        direct_recipient_share_bps: Option<u16>,
    ) -> (u64, u64, u64, u64, u64) {
        // Step 1: Calculate burn (if applicable)
        let burn = if apply_burn {
            (gross_amount * self.config.burn_rate_bps as u64) / 10000
        } else {
            0
        };

        let after_burn = gross_amount - burn;

        // Step 2: Calculate direct recipient share (e.g., message fee to relay)
        let direct_share = if let Some(bps) = direct_recipient_share_bps {
            (after_burn * bps as u64) / 10000
        } else {
            0
        };

        let protocol_portion = after_burn - direct_share;

        // Step 3: Split protocol portion according to config
        let validator_share = (protocol_portion * self.config.validator_share_bps as u64) / 10000;
        let relay_share = (protocol_portion * self.config.relay_share_bps as u64) / 10000;
        // Treasury gets remainder to avoid rounding dust
        let treasury_share = protocol_portion - validator_share - relay_share;

        (
            burn,
            validator_share,
            relay_share,
            treasury_share,
            direct_share,
        )
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
        };

        let (burn, validator_share, relay_share, treasury_share, direct_amount) =
            self.calculate_distribution(gross_amount, apply_burn, direct_share_bps);

        // Update pool balances
        {
            let mut pools = self.pool_balances.write().unwrap();
            *pools.get_mut(&self.sinks.validator_pool).unwrap() += validator_share;
            *pools.get_mut(&self.sinks.relay_pool).unwrap() += relay_share;
            *pools.get_mut(&self.sinks.treasury).unwrap() += treasury_share;
            *pools.get_mut(&self.sinks.burn_sink).unwrap() += burn;
        }

        // Update total burned
        if burn > 0 {
            *self.total_burned_all_time.write().unwrap() += burn;
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

    /// Get pool balance
    pub fn get_pool_balance(&self, pool: &UserId) -> u64 {
        *self.pool_balances.read().unwrap().get(pool).unwrap_or(&0)
    }

    /// Withdraw from a pool for distribution
    ///
    /// This debits the pool and returns the amount for distribution.
    /// Used when actually paying out rewards to validators/relays.
    pub fn withdraw_from_pool(&self, pool: &UserId, amount: u64) -> Result<u64> {
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
        Ok(amount)
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
        assert_eq!(config.validator_share_bps, 7000);
        assert_eq!(config.relay_share_bps, 2000);
        assert_eq!(config.treasury_share_bps, 1000);
        assert_eq!(config.burn_rate_bps, 100);
        assert_eq!(
            config.validator_share_bps + config.relay_share_bps + config.treasury_share_bps,
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
    }

    #[test]
    fn test_calculate_distribution_with_burn() {
        let manager = FeeDistributionManager::new(FeeDistributionConfig::default());

        // 1000 tokens with 1% burn
        let (burn, validator, relay, treasury, direct) =
            manager.calculate_distribution(1000, true, None);

        assert_eq!(burn, 10); // 1%
        let after_burn = 990;
        assert_eq!(validator, (after_burn * 7000) / 10000); // 693
        assert_eq!(relay, (after_burn * 2000) / 10000); // 198
                                                        // Treasury gets remainder
        assert_eq!(treasury, after_burn - validator - relay); // 99
        assert_eq!(direct, 0);

        // Verify conservation
        assert_eq!(burn + validator + relay + treasury + direct, 1000);
    }

    #[test]
    fn test_calculate_distribution_message_fee() {
        let manager = FeeDistributionManager::new(FeeDistributionConfig::default());

        // Message fee: 100% to relay, no burn
        let (burn, validator, relay, treasury, direct) =
            manager.calculate_distribution(1000, false, Some(10000));

        assert_eq!(burn, 0);
        assert_eq!(validator, 0);
        assert_eq!(relay, 0);
        assert_eq!(treasury, 0);
        assert_eq!(direct, 1000); // 100% to relay

        // Verify conservation
        assert_eq!(burn + validator + relay + treasury + direct, 1000);
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
