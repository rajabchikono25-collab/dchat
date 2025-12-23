//! Currency Chain client for payments, staking, rewards, and economics

use chrono::Utc;
use dchat_core::error::{Error, Result};
use dchat_core::motes::MOTES_PER_DCHAT;
use dchat_core::types::UserId;
use dchat_privacy::blind_tokens::CurrencyChainClient as PrivacyCurrencyChainClient;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use uuid::Uuid;

#[cfg(not(any(test, feature = "test-mocks")))]
use crate::client::{ChainRpcClient, HttpRpcClient};
#[cfg(any(test, feature = "test-mocks"))]
use crate::client::{ChainRpcClient, HttpRpcClient, MockRpcClient};
use crate::tokenomics::{BurnReason, MintReason, TokenomicsManager};

/// Configuration for Currency Chain client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyChainConfig {
    /// RPC endpoint for currency chain node
    pub rpc_url: String,
    /// WebSocket endpoint for subscriptions
    pub ws_url: Option<String>,
    /// Confirmation threshold (number of blocks)
    pub confirmation_blocks: u32,
    /// Transaction timeout (seconds)
    pub tx_timeout_seconds: u64,
    /// Retry attempts for failed transactions
    pub max_retries: u32,
}

impl Default for CurrencyChainConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8546".to_string(),
            ws_url: Some("ws://localhost:8547".to_string()),
            confirmation_blocks: 6,
            tx_timeout_seconds: 300,
            max_retries: 3,
        }
    }
}

/// Transaction on currency chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyTransaction {
    pub id: Uuid,
    pub tx_type: String, // "payment", "stake", "reward", "slash", "swap"
    pub from: UserId,
    pub to: Option<UserId>,
    pub amount: u64,
    pub status: String, // "pending", "confirmed", "failed"
    pub confirmations: u32,
    pub block_height: u64,
    pub created_at: i64,
}

/// Wallet balance on currency chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub user_id: UserId,
    pub balance: u64,
    pub staked: u64,
    pub rewards_pending: u64,
}

/// Validator information for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Validator's display name
    pub name: String,
    /// Validator's unique identifier
    pub validator_id: String,
    /// Total stake (own + delegated)
    pub total_stake: u64,
    /// Validator's own stake
    pub self_stake: u64,
    /// Delegated stake from others
    pub delegated_stake: u64,
    /// Commission rate in percentage (e.g., 5.0 = 5%)
    pub commission_rate: f64,
    /// Estimated APY in percentage
    pub apy_estimate: f64,
    /// Uptime percentage (0-100)
    pub uptime_percent: f64,
    /// Whether the validator is active
    pub is_active: bool,
}

/// Reward history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardHistoryEntry {
    /// Timestamp of the reward
    pub timestamp: i64,
    /// Type of reward (staking, relay, referral, etc.)
    pub reward_type: String,
    /// Amount received
    pub amount: u64,
    /// Transaction hash
    pub tx_hash: Option<String>,
    /// Associated epoch or block
    pub epoch: Option<u64>,
}

/// Pending rewards breakdown by source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRewardsBreakdown {
    /// Staking rewards pending
    pub staking_rewards: u64,
    /// Relay rewards pending
    pub relay_rewards: u64,
    /// Referral rewards pending
    pub referral_rewards: u64,
}

/// All-time rewards breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllTimeRewardsBreakdown {
    /// Total staking rewards earned
    pub staking: u64,
    /// Total relay rewards earned
    pub relaying: u64,
    /// Total referral rewards earned
    pub referrals: u64,
    /// Total governance participation rewards
    pub governance: u64,
    /// Current staking APY estimate
    pub current_staking_apy: f64,
    /// Combined APY estimate (with active participation)
    pub combined_apy_estimate: f64,
}

/// Staking position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakePosition {
    pub user_id: UserId,
    pub amount: u64,
    pub locked_until: i64,
    pub rewards_earned: u64,
}

/// Storage bond record on currency chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBondRecord {
    /// Unique bond identifier
    pub bond_id: [u8; 32],
    /// User's public key
    pub user_key: [u8; 32],
    /// Bond amount in smallest units (8 decimals)
    pub amount: u64,
    /// Storage capacity being bonded for (bytes)
    pub storage_bytes: u64,
    /// Duration in days
    pub duration_days: u32,
    /// Expiration timestamp
    pub expires_at: i64,
    /// Transaction ID
    pub tx_id: Uuid,
    /// Bond status
    pub status: StorageBondStatus,
    /// Created timestamp
    pub created_at: i64,
}

/// Storage bond status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageBondStatus {
    /// Bond is active
    Active,
    /// Bond is in unbonding period
    Unbonding,
    /// Bond has been withdrawn
    Withdrawn,
    /// Bond was slashed
    Slashed,
}

/// Result of creating a storage bond
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStorageBondResult {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Bond ID
    pub bond_id: [u8; 32],
    /// Amount locked
    pub amount_locked: u64,
    /// Expiration timestamp
    pub expires_at: i64,
}

// =============================================================================
// PAYMENT CHANNEL ESCROW INFRASTRUCTURE
// =============================================================================

/// Payment channel escrow record on currency chain
///
/// This struct represents locked funds in an on-chain payment channel escrow.
/// Funds are locked when the channel opens and released during cooperative
/// close or after a dispute period for unilateral close.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentChannelEscrow {
    /// Unique channel identifier
    pub channel_id: String,
    /// Sender (funder) of the channel
    pub sender_id: UserId,
    /// Receiver (payee) of the channel
    pub receiver_id: UserId,
    /// Total capacity locked in escrow
    pub capacity: u64,
    /// Current sender balance (remaining after payments)
    pub sender_balance: u64,
    /// Current receiver balance (accumulated payments)
    pub receiver_balance: u64,
    /// Escrow status
    pub status: PaymentChannelEscrowStatus,
    /// Funding transaction ID
    pub funding_tx_id: Uuid,
    /// Settlement transaction ID (if settled)
    pub settlement_tx_id: Option<Uuid>,
    /// When the escrow was created
    pub created_at: i64,
    /// When the escrow was last updated
    pub updated_at: i64,
    /// Dispute deadline (if in dispute period)
    pub dispute_deadline: Option<i64>,
}

/// Payment channel escrow status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentChannelEscrowStatus {
    /// Escrow is locked and channel is operational
    Active,
    /// Unilateral close initiated, in dispute period
    Disputing,
    /// Escrow has been released (channel closed)
    Settled,
    /// Escrow was disputed and resolved
    Disputed,
}

/// Genesis allocation tracking
///
/// Tracks the initial token allocation from genesis block, including
/// remaining allocations and audit trail of all genesis transfers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAllocation {
    /// Total tokens allocated in genesis
    pub total_allocated: u64,
    /// Remaining tokens available for genesis transfers
    pub remaining: u64,
    /// Individual allocations by category
    pub allocations: HashMap<String, GenesisAllocationEntry>,
    /// Audit trail of all genesis transfers
    pub transfers: Vec<GenesisTransferRecord>,
}

/// Individual genesis allocation entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAllocationEntry {
    /// Category name (e.g., "foundation_validators", "community_pool")
    pub category: String,
    /// Total allocation for this category
    pub total: u64,
    /// Remaining allocation
    pub remaining: u64,
    /// Vesting schedule (if applicable)
    pub vesting_schedule: Option<VestingSchedule>,
    /// Recipients who have received from this allocation
    pub recipients: Vec<UserId>,
}

/// Vesting schedule for genesis allocations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VestingSchedule {
    /// Cliff period in days (no tokens before this)
    pub cliff_days: u32,
    /// Total vesting duration in days
    pub vesting_days: u32,
    /// Tokens already vested
    pub vested: u64,
    /// Start timestamp (genesis time)
    pub start_timestamp: i64,
}

/// Record of a genesis transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisTransferRecord {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Recipient
    pub recipient: UserId,
    /// Amount transferred
    pub amount: u64,
    /// Source category
    pub source_category: String,
    /// Timestamp
    pub timestamp: i64,
    /// Remaining in category after this transfer
    pub remaining_after: u64,
}

/// Unbonding record for tracking stake withdrawals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnbondingRecord {
    /// Unique unbonding request ID
    pub id: Uuid,
    /// User requesting unbonding
    pub user_id: UserId,
    /// Amount being unbonded
    pub amount: u64,
    /// When unbonding was initiated
    pub initiated_at: i64,
    /// When the cooldown period ends and withdrawal is allowed
    pub available_at: i64,
    /// Node type being unstaked from ("validator", "relay", "delegation")
    pub node_type: String,
    /// Operator address (if delegation)
    pub operator: Option<UserId>,
    /// Status
    pub status: UnbondingStatus,
}

/// Unbonding status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnbondingStatus {
    /// In cooldown period
    Pending,
    /// Ready for withdrawal
    Ready,
    /// Withdrawn to wallet
    Completed,
    /// Cancelled by user (re-staked)
    Cancelled,
    /// Slashed during unbonding
    Slashed,
}

/// Result of locking funds in payment channel escrow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockEscrowResult {
    /// Transaction ID for the lock operation
    pub tx_id: Uuid,
    /// Channel ID
    pub channel_id: String,
    /// Amount locked
    pub amount_locked: u64,
}

/// Result of releasing payment channel escrow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseEscrowResult {
    /// Transaction ID for the release operation
    pub tx_id: Uuid,
    /// Amount returned to sender
    pub sender_refund: u64,
    /// Amount paid to receiver
    pub receiver_payout: u64,
}

/// Currency Chain client for payments, staking, rewards, and economics
pub struct CurrencyChainClient {
    config: CurrencyChainConfig,
    /// RPC client for blockchain queries
    rpc_client: Arc<dyn ChainRpcClient>,
    /// Transaction cache
    transactions: Arc<RwLock<HashMap<Uuid, CurrencyTransaction>>>,
    /// Current block height
    current_block: Arc<RwLock<u64>>,
    /// User wallet balances
    wallets: Arc<RwLock<HashMap<UserId, Wallet>>>,
    /// Staking positions
    stakes: Arc<RwLock<HashMap<UserId, StakePosition>>>,
    /// Storage bonds
    storage_bonds: Arc<RwLock<HashMap<[u8; 32], StorageBondRecord>>>,
    /// Payment channel escrows (channel_id -> escrow record)
    payment_channel_escrows: Arc<RwLock<HashMap<String, PaymentChannelEscrow>>>,
    /// Tokenomics manager (optional - can be shared)
    tokenomics: Option<Arc<TokenomicsManager>>,
    /// Fee distribution manager for proper fee routing (70/20/10 split)
    fee_distribution: Option<Arc<crate::fee_distribution::FeeDistributionManager>>,
    /// Shutdown signal for block sync task
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Payment transaction hash -> (amount, recipient, confirmed)
    payment_records: Arc<RwLock<HashMap<String, (u64, String, bool)>>>,
    /// Redeemed token signature hashes (for blind token double-spend prevention)
    redeemed_tokens: Arc<RwLock<HashSet<[u8; 32]>>>,
    /// Genesis allocation tracking
    genesis_allocation: Arc<RwLock<GenesisAllocation>>,
    /// Unbonding queue (user -> list of unbonding records)
    unbonding_queue: Arc<RwLock<HashMap<UserId, Vec<UnbondingRecord>>>>,
}

impl CurrencyChainClient {
    /// Create default genesis allocation based on tokenomics spec
    fn default_genesis_allocation() -> GenesisAllocation {
        use crate::tokenomics::TokenSupplyConfig;

        let total_supply = TokenSupplyConfig::default().initial_supply; // 100B tokens

        // Allocations as per ARCHITECTURE-2.0.md Section 6.4
        let mut allocations = HashMap::new();

        // Foundation validators: 7% for initial 7 validators @ 10M each
        allocations.insert(
            "foundation_validators".to_string(),
            GenesisAllocationEntry {
                category: "foundation_validators".to_string(),
                total: total_supply * 7 / 100,
                remaining: total_supply * 7 / 100,
                vesting_schedule: Some(VestingSchedule {
                    cliff_days: 0,
                    vesting_days: 365,
                    vested: 0,
                    start_timestamp: Utc::now().timestamp(),
                }),
                recipients: Vec::new(),
            },
        );

        // Foundation relays: 2.8% for initial 14 relays @ 2M each
        allocations.insert(
            "foundation_relays".to_string(),
            GenesisAllocationEntry {
                category: "foundation_relays".to_string(),
                total: total_supply * 28 / 1000,
                remaining: total_supply * 28 / 1000,
                vesting_schedule: Some(VestingSchedule {
                    cliff_days: 0,
                    vesting_days: 365,
                    vested: 0,
                    start_timestamp: Utc::now().timestamp(),
                }),
                recipients: Vec::new(),
            },
        );

        // Foundation treasury: 15%
        allocations.insert(
            "foundation_treasury".to_string(),
            GenesisAllocationEntry {
                category: "foundation_treasury".to_string(),
                total: total_supply * 15 / 100,
                remaining: total_supply * 15 / 100,
                vesting_schedule: None, // Governance-locked
                recipients: Vec::new(),
            },
        );

        // Community incentives: 30% - linear 4 year vesting
        allocations.insert(
            "community_incentives".to_string(),
            GenesisAllocationEntry {
                category: "community_incentives".to_string(),
                total: total_supply * 30 / 100,
                remaining: total_supply * 30 / 100,
                vesting_schedule: Some(VestingSchedule {
                    cliff_days: 0,
                    vesting_days: 365 * 4,
                    vested: 0,
                    start_timestamp: Utc::now().timestamp(),
                }),
                recipients: Vec::new(),
            },
        );

        // Ecosystem grants: 15%
        allocations.insert(
            "ecosystem_grants".to_string(),
            GenesisAllocationEntry {
                category: "ecosystem_grants".to_string(),
                total: total_supply * 15 / 100,
                remaining: total_supply * 15 / 100,
                vesting_schedule: None, // Milestone-based
                recipients: Vec::new(),
            },
        );

        // Team: 15% - 1yr cliff + 3yr linear
        allocations.insert(
            "team".to_string(),
            GenesisAllocationEntry {
                category: "team".to_string(),
                total: total_supply * 15 / 100,
                remaining: total_supply * 15 / 100,
                vesting_schedule: Some(VestingSchedule {
                    cliff_days: 365,
                    vesting_days: 365 * 4, // 1 year cliff + 3 years
                    vested: 0,
                    start_timestamp: Utc::now().timestamp(),
                }),
                recipients: Vec::new(),
            },
        );

        // Public distribution: 5% - immediate
        allocations.insert(
            "public_distribution".to_string(),
            GenesisAllocationEntry {
                category: "public_distribution".to_string(),
                total: total_supply * 5 / 100,
                remaining: total_supply * 5 / 100,
                vesting_schedule: None, // Immediate
                recipients: Vec::new(),
            },
        );

        GenesisAllocation {
            total_allocated: total_supply,
            remaining: total_supply,
            allocations,
            transfers: Vec::new(),
        }
    }

    /// Create new currency chain client with production RPC
    pub fn new(config: CurrencyChainConfig) -> Result<Self> {
        let rpc_client = HttpRpcClient::new(config.rpc_url.clone())?;

        // Initialize fee distribution manager with default config
        let fee_distribution = Arc::new(crate::fee_distribution::FeeDistributionManager::new(
            crate::fee_distribution::FeeDistributionConfig::default(),
        ));

        Ok(Self {
            config,
            rpc_client: Arc::new(rpc_client),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            wallets: Arc::new(RwLock::new(HashMap::new())),
            stakes: Arc::new(RwLock::new(HashMap::new())),
            storage_bonds: Arc::new(RwLock::new(HashMap::new())),
            payment_channel_escrows: Arc::new(RwLock::new(HashMap::new())),
            tokenomics: None,
            fee_distribution: Some(fee_distribution),
            shutdown_tx: None,
            payment_records: Arc::new(RwLock::new(HashMap::new())),
            redeemed_tokens: Arc::new(RwLock::new(HashSet::new())),
            genesis_allocation: Arc::new(RwLock::new(Self::default_genesis_allocation())),
            unbonding_queue: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create new currency chain client with mock RPC for testing
    ///
    /// Only available in test builds or with the `test-mocks` feature.
    #[cfg(any(test, feature = "test-mocks"))]
    pub fn new_mock(config: CurrencyChainConfig) -> Self {
        let rpc_client = MockRpcClient::new();

        // Initialize fee distribution manager for tests
        let fee_distribution = Arc::new(crate::fee_distribution::FeeDistributionManager::new(
            crate::fee_distribution::FeeDistributionConfig::default(),
        ));

        Self {
            config,
            rpc_client: Arc::new(rpc_client),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            wallets: Arc::new(RwLock::new(HashMap::new())),
            stakes: Arc::new(RwLock::new(HashMap::new())),
            storage_bonds: Arc::new(RwLock::new(HashMap::new())),
            payment_channel_escrows: Arc::new(RwLock::new(HashMap::new())),
            tokenomics: None,
            fee_distribution: Some(fee_distribution),
            shutdown_tx: None,
            payment_records: Arc::new(RwLock::new(HashMap::new())),
            redeemed_tokens: Arc::new(RwLock::new(HashSet::new())),
            genesis_allocation: Arc::new(RwLock::new(Self::default_genesis_allocation())),
            unbonding_queue: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Credit a user account with tokens (for testing only)
    ///
    /// This bypasses normal token creation and directly credits the account.
    /// Only available in test builds or with the `test-mocks` feature.
    #[cfg(any(test, feature = "test-mocks"))]
    pub fn credit_for_testing(&self, user_id: &UserId, amount: u64) {
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets.entry(user_id.clone()).or_insert_with(|| Wallet {
            user_id: user_id.clone(),
            balance: 0,
            staked: 0,
            rewards_pending: 0,
        });
        wallet.balance += amount;
    }

    /// Create new currency chain client with tokenomics integration
    pub fn with_tokenomics(
        config: CurrencyChainConfig,
        tokenomics: Arc<TokenomicsManager>,
    ) -> Result<Self> {
        let rpc_client = HttpRpcClient::new(config.rpc_url.clone())?;

        // Initialize fee distribution manager
        let fee_distribution = Arc::new(crate::fee_distribution::FeeDistributionManager::new(
            crate::fee_distribution::FeeDistributionConfig::default(),
        ));

        Ok(Self {
            config,
            rpc_client: Arc::new(rpc_client),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            wallets: Arc::new(RwLock::new(HashMap::new())),
            stakes: Arc::new(RwLock::new(HashMap::new())),
            storage_bonds: Arc::new(RwLock::new(HashMap::new())),
            payment_channel_escrows: Arc::new(RwLock::new(HashMap::new())),
            tokenomics: Some(tokenomics),
            fee_distribution: Some(fee_distribution),
            shutdown_tx: None,
            payment_records: Arc::new(RwLock::new(HashMap::new())),
            redeemed_tokens: Arc::new(RwLock::new(HashSet::new())),
            genesis_allocation: Arc::new(RwLock::new(Self::default_genesis_allocation())),
            unbonding_queue: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start block synchronization from blockchain
    /// Polls for new blocks and updates transaction confirmations
    pub async fn start_sync(&mut self) -> Result<()> {
        use tokio::time::{sleep, Duration};

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        let rpc_client = Arc::clone(&self.rpc_client);
        let transactions = Arc::clone(&self.transactions);
        let current_block = Arc::clone(&self.current_block);
        let confirmation_blocks = self.config.confirmation_blocks;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Block sync task shutting down");
                        break;
                    }
                    _ = sleep(Duration::from_secs(2)) => {
                        // Poll for new block height
                        match rpc_client.get_current_height().await {
                            Ok(new_height) => {
                                let mut current = current_block.write().unwrap();
                                let old_height = *current;

                                if new_height > old_height {
                                    *current = new_height;
                                    drop(current);

                                    // Update transaction confirmations
                                    let mut txs = transactions.write().unwrap();
                                    for tx in txs.values_mut() {
                                        if tx.block_height > 0 && tx.status == "pending" {
                                            let confirmations = new_height.saturating_sub(tx.block_height) as u32;
                                            tx.confirmations = confirmations;

                                            if confirmations >= confirmation_blocks {
                                                tx.status = "confirmed".to_string();
                                                tracing::debug!(
                                                    "Transaction {} confirmed with {} confirmations",
                                                    tx.id, confirmations
                                                );
                                            }
                                        }
                                    }

                                    tracing::debug!(
                                        "Block sync: height {} -> {} ({} new blocks)",
                                        old_height, new_height, new_height - old_height
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to query block height: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop block synchronization
    pub async fn stop_sync(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }

    /// Get tokenomics manager reference
    pub fn get_tokenomics(&self) -> Option<Arc<TokenomicsManager>> {
        self.tokenomics.clone()
    }

    /// Create wallet for user
    pub fn create_wallet(&self, user_id: &UserId, initial_balance: u64) -> Result<Wallet> {
        let wallet = Wallet {
            user_id: user_id.clone(),
            balance: initial_balance,
            staked: 0,
            rewards_pending: 0,
        };
        self.wallets
            .write()
            .unwrap()
            .insert(user_id.clone(), wallet.clone());
        Ok(wallet)
    }

    /// Get wallet balance
    pub fn get_balance(&self, user_id: &UserId) -> Result<u64> {
        let wallets = self.wallets.read().unwrap();
        Ok(wallets.get(user_id).map(|w| w.balance).unwrap_or(0))
    }

    /// Get user's staked amount
    ///
    /// Returns the total amount of tokens this user has staked.
    /// Used for anti-bot verification (minimum stake requirements).
    pub fn get_staked_amount(&self, user_id: &UserId) -> Result<u64> {
        // First check the stakes HashMap for active stake positions
        let stakes = self.stakes.read().unwrap();
        if let Some(stake_position) = stakes.get(user_id) {
            return Ok(stake_position.amount);
        }

        // Also check wallet's staked field as fallback
        let wallets = self.wallets.read().unwrap();
        Ok(wallets.get(user_id).map(|w| w.staked).unwrap_or(0))
    }

    /// Transfer tokens between users
    ///
    /// # Fee Handling (Mainnet Production)
    ///
    /// For standard user-to-user transfers:
    /// - 1% of the amount is burned (deflationary)
    /// - The remaining 99% goes to the recipient
    /// - No protocol fee split on user-to-user transfers (fees come from message/channel fees)
    ///
    /// For protocol-internal transfers (rewards, refunds), use `transfer_internal`
    /// which bypasses the burn to ensure recipients receive full amount.
    pub fn transfer(&self, from: &UserId, to: &UserId, amount: u64) -> Result<Uuid> {
        let tx_id = Uuid::new_v4();
        let mut wallets = self.wallets.write().unwrap();

        let from_wallet = wallets
            .get_mut(from)
            .ok_or_else(|| Error::NotFound(format!("User not found: {}", from)))?;

        if from_wallet.balance < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance: have {}, need {}",
                from_wallet.balance, amount
            )));
        }

        // Calculate transaction fee burn (1% default)
        let burn_amount = if let Some(ref tokenomics) = self.tokenomics {
            (amount * tokenomics.get_statistics().burn_rate_bps as u64) / 10000
        } else {
            // Use default burn rate if tokenomics not configured
            (amount * crate::fee_distribution::DEFAULT_BURN_RATE_BPS as u64) / 10000
        };

        let net_amount = amount - burn_amount;

        from_wallet.balance -= amount;

        let to_wallet = wallets.entry(to.clone()).or_insert_with(|| Wallet {
            user_id: to.clone(),
            balance: 0,
            staked: 0,
            rewards_pending: 0,
        });

        to_wallet.balance += net_amount;

        // Burn transaction fee via tokenomics (tracks supply reduction)
        if burn_amount > 0 {
            if let Some(ref tokenomics) = self.tokenomics {
                let _ =
                    tokenomics.burn_tokens(burn_amount, BurnReason::TransactionFee, from.clone());
            }

            // Also record in fee distribution for consensus verification
            if let Some(ref fee_dist) = self.fee_distribution {
                // Record the burn in block accounting (no distribution for user transfers)
                let _ = fee_dist.collect_fee(
                    crate::fee_distribution::FeeType::TransferFee,
                    burn_amount, // Only the burn portion is "collected" as a fee
                    from.clone(),
                    None,
                    tx_id,
                );
            }
        }

        let tx = CurrencyTransaction {
            id: tx_id,
            tx_type: "payment".to_string(),
            from: from.clone(),
            to: Some(to.clone()),
            amount,
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: Utc::now().timestamp(),
        };

        self.transactions.write().unwrap().insert(tx_id, tx);

        Ok(tx_id)
    }

    /// Transfer tokens internally without burn (for protocol operations)
    ///
    /// Use this for:
    /// - Reward payouts to validators/relays
    /// - Refunds from payment channels
    /// - Treasury disbursements
    /// - Any protocol-to-user transfer where burn should not apply
    ///
    /// This ensures recipients receive the full intended amount.
    pub fn transfer_internal(
        &self,
        from: &UserId,
        to: &UserId,
        amount: u64,
        reason: &str,
    ) -> Result<Uuid> {
        let tx_id = Uuid::new_v4();
        let mut wallets = self.wallets.write().unwrap();

        let from_wallet = wallets
            .get_mut(from)
            .ok_or_else(|| Error::NotFound(format!("Source not found: {}", from)))?;

        if from_wallet.balance < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient pool balance: have {}, need {}",
                from_wallet.balance, amount
            )));
        }

        from_wallet.balance -= amount;

        let to_wallet = wallets.entry(to.clone()).or_insert_with(|| Wallet {
            user_id: to.clone(),
            balance: 0,
            staked: 0,
            rewards_pending: 0,
        });

        // Full amount to recipient - NO BURN
        to_wallet.balance += amount;

        let tx = CurrencyTransaction {
            id: tx_id,
            tx_type: format!("internal:{}", reason),
            from: from.clone(),
            to: Some(to.clone()),
            amount,
            status: "confirmed".to_string(), // Internal transfers are instant
            confirmations: 1,
            block_height: *self.current_block.read().unwrap(),
            created_at: Utc::now().timestamp(),
        };

        self.transactions.write().unwrap().insert(tx_id, tx);

        tracing::debug!(
            "Internal transfer: {} -> {} amount={} reason={}",
            from,
            to,
            amount,
            reason
        );

        Ok(tx_id)
    }

    /// Collect message fee with proper distribution
    ///
    /// Message fees are collected from sender and distributed to the relay
    /// that delivers the message. No burn is applied to message fees because
    /// the relay is providing a service and should receive the full fee.
    ///
    /// # Arguments
    /// * `sender` - User sending the message
    /// * `relay` - Relay that will deliver the message
    /// * `fee_amount` - Amount to collect as message fee
    ///
    /// # Returns
    /// Transaction ID for the fee collection
    pub fn collect_message_fee(
        &self,
        sender: &UserId,
        relay: &UserId,
        fee_amount: u64,
    ) -> Result<Uuid> {
        let tx_id = Uuid::new_v4();
        let mut wallets = self.wallets.write().unwrap();

        let sender_wallet = wallets
            .get_mut(sender)
            .ok_or_else(|| Error::NotFound(format!("Sender not found: {}", sender)))?;

        if sender_wallet.balance < fee_amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance for message fee: have {}, need {}",
                sender_wallet.balance, fee_amount
            )));
        }

        // Deduct full fee from sender
        sender_wallet.balance -= fee_amount;

        // Credit full fee to relay (no burn for service fees)
        let relay_wallet = wallets.entry(relay.clone()).or_insert_with(|| Wallet {
            user_id: relay.clone(),
            balance: 0,
            staked: 0,
            rewards_pending: 0,
        });
        relay_wallet.balance += fee_amount;

        // Record in fee distribution for block accounting
        if let Some(ref fee_dist) = self.fee_distribution {
            let _ = fee_dist.collect_fee(
                crate::fee_distribution::FeeType::MessageFee,
                fee_amount,
                sender.clone(),
                Some(relay.clone()),
                tx_id,
            );
        }

        let tx = CurrencyTransaction {
            id: tx_id,
            tx_type: "message_fee".to_string(),
            from: sender.clone(),
            to: Some(relay.clone()),
            amount: fee_amount,
            status: "confirmed".to_string(),
            confirmations: 1,
            block_height: *self.current_block.read().unwrap(),
            created_at: Utc::now().timestamp(),
        };

        self.transactions.write().unwrap().insert(tx_id, tx);

        tracing::debug!(
            "Message fee collected: {} from {} to relay {}",
            fee_amount,
            sender,
            relay
        );

        Ok(tx_id)
    }

    /// Get fee distribution manager
    pub fn get_fee_distribution(
        &self,
    ) -> Option<&Arc<crate::fee_distribution::FeeDistributionManager>> {
        self.fee_distribution.as_ref()
    }

    /// Stake tokens for rewards
    pub fn stake(&self, user_id: &UserId, amount: u64, lock_duration_seconds: i64) -> Result<Uuid> {
        let mut wallets = self.wallets.write().unwrap();

        let wallet = wallets
            .get_mut(user_id)
            .ok_or_else(|| Error::NotFound(format!("User not found: {}", user_id)))?;

        if wallet.balance < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance: have {}, need {}",
                wallet.balance, amount
            )));
        }

        wallet.balance -= amount;
        wallet.staked += amount;

        let locked_until = Utc::now().timestamp() + lock_duration_seconds;
        let stake_position = StakePosition {
            user_id: user_id.clone(),
            amount,
            locked_until,
            rewards_earned: 0,
        };

        self.stakes
            .write()
            .unwrap()
            .insert(user_id.clone(), stake_position);

        let tx = CurrencyTransaction {
            id: Uuid::new_v4(),
            tx_type: "stake".to_string(),
            from: user_id.clone(),
            to: None,
            amount,
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: Utc::now().timestamp(),
        };

        let tx_id = tx.id;
        self.transactions.write().unwrap().insert(tx_id, tx);

        Ok(tx_id)
    }

    /// Claim rewards
    pub fn claim_rewards(&self, user_id: &UserId) -> Result<Uuid> {
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets
            .get_mut(user_id)
            .ok_or_else(|| Error::NotFound(format!("User not found: {}", user_id)))?;

        let rewards = wallet.rewards_pending;
        wallet.balance += rewards;
        wallet.rewards_pending = 0;

        let tx = CurrencyTransaction {
            id: Uuid::new_v4(),
            tx_type: "reward".to_string(),
            from: user_id.clone(),
            to: None,
            amount: rewards,
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: Utc::now().timestamp(),
        };

        let tx_id = tx.id;
        self.transactions.write().unwrap().insert(tx_id, tx);

        Ok(tx_id)
    }

    /// Mint new tokens as rewards (inflationary)
    /// Only callable by consensus for block rewards, relay rewards, etc.
    ///
    /// # Arguments
    /// * `recipient` - User to receive the minted rewards
    /// * `amount` - Amount of tokens to mint
    /// * `reason` - The reason for minting (for tokenomics tracking)
    ///
    /// # Returns
    /// Transaction ID for the mint operation
    pub fn mint_rewards(
        &self,
        recipient: &UserId,
        amount: u64,
        reason: MintReason,
    ) -> Result<Uuid> {
        if amount == 0 {
            return Err(Error::validation("Mint amount must be greater than 0"));
        }

        // 1. Update tokenomics tracking (if available)
        if let Some(ref tokenomics) = self.tokenomics {
            tokenomics.record_mint(amount, reason.clone())?;
        }

        // 2. Get or create recipient wallet
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets.entry(recipient.clone()).or_insert_with(|| Wallet {
            user_id: recipient.clone(),
            balance: 0,
            staked: 0,
            rewards_pending: 0,
        });

        // 3. Add to recipient's rewards_pending (they need to claim to move to balance)
        wallet.rewards_pending += amount;

        // 4. Create transaction record
        let tx = CurrencyTransaction {
            id: Uuid::new_v4(),
            tx_type: format!("mint_{:?}", reason).to_lowercase(),
            from: UserId::default(), // System/minting account
            to: Some(recipient.clone()),
            amount,
            status: "confirmed".to_string(), // Minting is immediately confirmed
            confirmations: 1,
            block_height: *self.current_block.read().unwrap(),
            created_at: Utc::now().timestamp(),
        };

        let tx_id = tx.id;
        self.transactions.write().unwrap().insert(tx_id, tx);

        tracing::info!(
            "💰 Minted {} tokens to {} (reason: {:?}, tx: {})",
            amount,
            recipient,
            reason,
            tx_id
        );

        Ok(tx_id)
    }

    /// Get transaction by ID
    pub fn get_transaction(&self, tx_id: &Uuid) -> Result<Option<CurrencyTransaction>> {
        Ok(self.transactions.read().unwrap().get(tx_id).cloned())
    }

    /// Get current block height
    pub fn get_current_block(&self) -> u64 {
        *self.current_block.read().unwrap()
    }

    /// Get all transactions for a user
    pub fn get_user_transactions(&self, user_id: &UserId) -> Result<Vec<CurrencyTransaction>> {
        let txs = self.transactions.read().unwrap();
        Ok(txs
            .values()
            .filter(|tx| tx.from == *user_id)
            .cloned()
            .collect())
    }

    /// Get wallet info
    pub fn get_wallet(&self, user_id: &UserId) -> Result<Option<Wallet>> {
        Ok(self.wallets.read().unwrap().get(user_id).cloned())
    }

    /// Get current block height from blockchain
    pub async fn get_current_height(&self) -> Result<u64> {
        self.rpc_client.get_current_height().await
    }

    /// Record a confirmed payment transaction (for blind token verification)
    pub fn record_payment(&self, tx_hash: &str, amount: u64, recipient: &str) {
        self.payment_records
            .write()
            .unwrap()
            .insert(tx_hash.to_string(), (amount, recipient.to_string(), true));
    }

    /// Create a storage bond by locking tokens
    ///
    /// This deducts the bond amount from the user's wallet and locks it
    /// for the specified duration. The bond can earn interest based on
    /// the storage provided.
    ///
    /// # Arguments
    /// * `user_id` - The user creating the bond
    /// * `bond_id` - Unique identifier for the bond
    /// * `user_key` - User's public key (for verification)
    /// * `amount` - Amount to bond in smallest units (8 decimals)
    /// * `storage_bytes` - Storage capacity being bonded for
    /// * `duration_days` - Lock duration in days
    ///
    /// # Returns
    /// Result containing the bond creation details
    pub fn create_storage_bond(
        &self,
        user_id: &UserId,
        bond_id: [u8; 32],
        user_key: [u8; 32],
        amount: u64,
        storage_bytes: u64,
        duration_days: u32,
    ) -> Result<CreateStorageBondResult> {
        // 1. Validate input
        if amount == 0 {
            return Err(Error::validation("Bond amount must be greater than 0"));
        }
        if duration_days == 0 {
            return Err(Error::validation("Bond duration must be at least 1 day"));
        }

        // 2. Check for duplicate bond ID
        {
            let bonds = self.storage_bonds.read().unwrap();
            if bonds.contains_key(&bond_id) {
                return Err(Error::validation(format!(
                    "Bond already exists: {}",
                    hex::encode(bond_id)
                )));
            }
        }

        // 3. Deduct from user's wallet
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets
            .get_mut(user_id)
            .ok_or_else(|| Error::NotFound(format!("User wallet not found: {}", user_id)))?;

        if wallet.balance < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance: have {}, need {}",
                wallet.balance, amount
            )));
        }

        wallet.balance -= amount;
        wallet.staked += amount; // Track as staked

        // 4. Calculate expiration
        let now = Utc::now().timestamp();
        let expires_at = now + (duration_days as i64 * 86400);

        // 5. Create bond record
        let tx_id = Uuid::new_v4();
        let bond_record = StorageBondRecord {
            bond_id,
            user_key,
            amount,
            storage_bytes,
            duration_days,
            expires_at,
            tx_id,
            status: StorageBondStatus::Active,
            created_at: now,
        };

        self.storage_bonds
            .write()
            .unwrap()
            .insert(bond_id, bond_record);

        // 6. Create transaction record
        let tx = CurrencyTransaction {
            id: tx_id,
            tx_type: "storage_bond".to_string(),
            from: user_id.clone(),
            to: None,
            amount,
            status: "confirmed".to_string(),
            confirmations: 1,
            block_height: *self.current_block.read().unwrap(),
            created_at: now,
        };

        self.transactions.write().unwrap().insert(tx_id, tx);

        tracing::info!(
            "🔒 Created storage bond {} for {} ({} bytes, {} days, tx: {})",
            hex::encode(bond_id),
            user_id,
            storage_bytes,
            duration_days,
            tx_id
        );

        Ok(CreateStorageBondResult {
            tx_id,
            bond_id,
            amount_locked: amount,
            expires_at,
        })
    }

    /// Get storage bond by ID
    pub fn get_storage_bond(&self, bond_id: &[u8; 32]) -> Option<StorageBondRecord> {
        self.storage_bonds.read().unwrap().get(bond_id).cloned()
    }

    /// Initiate unbonding for a storage bond
    pub fn initiate_unbonding(
        &self,
        bond_id: &[u8; 32],
        unbonding_period_days: u32,
    ) -> Result<Uuid> {
        let mut bonds = self.storage_bonds.write().unwrap();
        let bond = bonds
            .get_mut(bond_id)
            .ok_or_else(|| Error::NotFound(format!("Bond not found: {}", hex::encode(bond_id))))?;

        if bond.status != StorageBondStatus::Active {
            return Err(Error::validation(format!(
                "Bond {} is not active, current status: {:?}",
                hex::encode(bond_id),
                bond.status
            )));
        }

        // Update bond status
        let now = Utc::now().timestamp();
        bond.status = StorageBondStatus::Unbonding;
        bond.expires_at = now + (unbonding_period_days as i64 * 86400);

        let tx_id = Uuid::new_v4();
        bond.tx_id = tx_id;

        tracing::info!(
            "⏳ Initiated unbonding for bond {}, completes in {} days (tx: {})",
            hex::encode(bond_id),
            unbonding_period_days,
            tx_id
        );

        Ok(tx_id)
    }

    /// Complete withdrawal of an unbonded storage bond
    pub fn complete_bond_withdrawal(
        &self,
        user_id: &UserId,
        bond_id: &[u8; 32],
    ) -> Result<(u64, Uuid)> {
        let mut bonds = self.storage_bonds.write().unwrap();
        let bond = bonds
            .get_mut(bond_id)
            .ok_or_else(|| Error::NotFound(format!("Bond not found: {}", hex::encode(bond_id))))?;

        if bond.status != StorageBondStatus::Unbonding {
            return Err(Error::validation(format!(
                "Bond {} is not in unbonding state",
                hex::encode(bond_id)
            )));
        }

        let now = Utc::now().timestamp();
        if now < bond.expires_at {
            return Err(Error::validation(format!(
                "Unbonding period not complete, {} seconds remaining",
                bond.expires_at - now
            )));
        }

        let amount = bond.amount;
        bond.status = StorageBondStatus::Withdrawn;

        // Return funds to user
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets
            .get_mut(user_id)
            .ok_or_else(|| Error::NotFound(format!("User wallet not found: {}", user_id)))?;

        wallet.staked = wallet.staked.saturating_sub(amount);
        wallet.balance += amount;

        let tx_id = Uuid::new_v4();

        // Create transaction record
        let tx = CurrencyTransaction {
            id: tx_id,
            tx_type: "bond_withdrawal".to_string(),
            from: UserId::default(),
            to: Some(user_id.clone()),
            amount,
            status: "confirmed".to_string(),
            confirmations: 1,
            block_height: *self.current_block.read().unwrap(),
            created_at: now,
        };

        drop(bonds);
        self.transactions.write().unwrap().insert(tx_id, tx);

        tracing::info!(
            "✅ Completed bond withdrawal {} for {}, returned {} (tx: {})",
            hex::encode(bond_id),
            user_id,
            amount,
            tx_id
        );

        Ok((amount, tx_id))
    }

    // ========================================================================
    // PRODUCTION GENESIS & STAKING FUNCTIONALITY
    // ========================================================================

    /// Transfer tokens from genesis allocation to a server's address
    ///
    /// This is called during mainnet initialization to fund foundation servers
    /// from the genesis allocation. The tokens come from the genesis pool.
    ///
    /// # Arguments
    /// * `recipient` - The recipient's UserId (derived from their public key)
    /// * `amount` - Amount of tokens to transfer from genesis
    /// * `source_description` - Description of the genesis source (e.g., "foundation_allocation")
    ///
    /// # Returns
    /// Transaction ID for the genesis transfer
    pub fn transfer_from_genesis(
        &self,
        recipient: &UserId,
        amount: u64,
        source_description: &str,
    ) -> Result<Uuid> {
        if amount == 0 {
            return Err(Error::validation(
                "Genesis transfer amount must be greater than 0",
            ));
        }

        // Map source_description to allocation category
        let category = match source_description {
            s if s.contains("validator") => "foundation_validators",
            s if s.contains("relay") => "foundation_relays",
            s if s.contains("treasury") => "foundation_treasury",
            s if s.contains("community") || s.contains("incentive") => "community_incentives",
            s if s.contains("grant") || s.contains("ecosystem") => "ecosystem_grants",
            s if s.contains("team") => "team",
            s if s.contains("public") || s.contains("faucet") || s.contains("airdrop") => {
                "public_distribution"
            }
            _ => source_description,
        };

        // Verify against remaining genesis allocation
        let mut genesis_allocation = self.genesis_allocation.write().unwrap();

        // Check total remaining allocation
        if genesis_allocation.remaining < amount {
            return Err(Error::validation(format!(
                "Insufficient total genesis allocation: requested {} but only {} remaining",
                amount, genesis_allocation.remaining
            )));
        }

        // Check category-specific allocation
        let allocation_entry = genesis_allocation
            .allocations
            .get_mut(category)
            .ok_or_else(|| {
                Error::validation(format!(
                    "Unknown genesis allocation category: '{}' (source: '{}')",
                    category, source_description
                ))
            })?;

        if allocation_entry.remaining < amount {
            return Err(Error::validation(format!(
                "Insufficient allocation in category '{}': requested {} but only {} remaining",
                category, amount, allocation_entry.remaining
            )));
        }

        // Check vesting schedule if applicable
        if let Some(ref vesting) = allocation_entry.vesting_schedule {
            let now = Utc::now().timestamp();
            let elapsed_days = (now - vesting.start_timestamp) / 86400;

            // During cliff period, no tokens can be released
            if elapsed_days < vesting.cliff_days as i64 {
                return Err(Error::validation(format!(
                    "Category '{}' is in cliff period: {} days remaining",
                    category,
                    vesting.cliff_days as i64 - elapsed_days
                )));
            }

            // Calculate vested amount based on linear vesting
            let vesting_days = vesting.vesting_days.saturating_sub(vesting.cliff_days);
            let elapsed_after_cliff = (elapsed_days - vesting.cliff_days as i64).max(0) as u64;
            let vesting_progress = if vesting_days > 0 {
                (elapsed_after_cliff as f64 / vesting_days as f64).min(1.0)
            } else {
                1.0
            };

            let max_vested = (allocation_entry.total as f64 * vesting_progress) as u64;
            let already_distributed = allocation_entry.total - allocation_entry.remaining;
            let available_to_distribute = max_vested.saturating_sub(already_distributed);

            if amount > available_to_distribute {
                return Err(Error::validation(format!(
                    "Vesting limit exceeded for '{}': requested {} but only {} vested and available",
                    category, amount, available_to_distribute
                )));
            }
        }

        // Deduct from allocation tracking
        allocation_entry.remaining -= amount;
        allocation_entry.recipients.push(recipient.clone());
        let remaining_after = allocation_entry.remaining;

        // Now we can safely access genesis_allocation again
        genesis_allocation.remaining -= amount;

        // Create transfer record for audit trail
        let now = Utc::now().timestamp();
        let tx_id = Uuid::new_v4();

        genesis_allocation.transfers.push(GenesisTransferRecord {
            tx_id,
            recipient: recipient.clone(),
            amount,
            source_category: category.to_string(),
            timestamp: now,
            remaining_after,
        });

        drop(genesis_allocation);

        // Get or create recipient wallet
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets.entry(recipient.clone()).or_insert_with(|| Wallet {
            user_id: recipient.clone(),
            balance: 0,
            staked: 0,
            rewards_pending: 0,
        });

        // Credit the recipient directly (genesis allocation)
        wallet.balance += amount;

        // Create transaction record
        let tx = CurrencyTransaction {
            id: tx_id,
            tx_type: format!("genesis_{}", source_description),
            from: UserId::default(), // Genesis/system account
            to: Some(recipient.clone()),
            amount,
            status: "confirmed".to_string(), // Genesis is immediately confirmed
            confirmations: 1,
            block_height: 0, // Genesis block
            created_at: now,
        };

        drop(wallets);
        self.transactions.write().unwrap().insert(tx_id, tx);

        tracing::info!(
            "💰 Genesis transfer: {} tokens to {} (category: {}, source: {}, tx: {})",
            amount,
            recipient,
            category,
            source_description,
            tx_id
        );

        Ok(tx_id)
    }

    /// Get genesis allocation status
    pub fn get_genesis_allocation_status(&self) -> GenesisAllocation {
        self.genesis_allocation.read().unwrap().clone()
    }

    /// Get genesis transfer audit trail for a category
    pub fn get_genesis_transfers_by_category(&self, category: &str) -> Vec<GenesisTransferRecord> {
        let allocation = self.genesis_allocation.read().unwrap();
        allocation
            .transfers
            .iter()
            .filter(|t| t.source_category == category)
            .cloned()
            .collect()
    }

    /// Automatically stake tokens for a validator or relay
    ///
    /// This is called during server initialization to automatically stake
    /// the server's allocated tokens. For validators and relays, staking
    /// is mandatory to participate in the network.
    ///
    /// # Arguments
    /// * `operator` - The operator's UserId
    /// * `amount` - Amount of tokens to stake
    /// * `node_type` - Type of node ("validator", "relay")
    /// * `lock_duration_days` - How long to lock the stake (in days)
    ///
    /// # Returns
    /// Transaction ID for the stake
    pub fn auto_stake_for_node(
        &self,
        operator: &UserId,
        amount: u64,
        node_type: &str,
        lock_duration_days: u32,
    ) -> Result<Uuid> {
        if amount == 0 {
            return Err(Error::validation("Stake amount must be greater than 0"));
        }

        // Lock duration in seconds
        let lock_duration_seconds = (lock_duration_days as i64) * 86400;

        // Perform the stake
        let tx_id = self.stake(operator, amount, lock_duration_seconds)?;

        tracing::info!(
            "🔒 Auto-staked {} tokens for {} node {} (lock: {} days, tx: {})",
            amount,
            node_type,
            operator,
            lock_duration_days,
            tx_id
        );

        Ok(tx_id)
    }

    /// Initiate unbonding for a staked position
    ///
    /// This starts the unbonding process for a validator or relay.
    /// The tokens will remain locked during the cooldown period.
    ///
    /// # Arguments
    /// * `operator` - The operator's UserId
    /// * `amount` - Amount of tokens to unbond (or 0 for all)
    ///
    /// # Returns
    /// The unbonding record with full details
    pub fn initiate_stake_unbonding(
        &self,
        operator: &UserId,
        amount: u64,
    ) -> Result<UnbondingRecord> {
        self.initiate_stake_unbonding_with_type(operator, amount, "validator")
    }

    /// Initiate unbonding with specific node type
    ///
    /// This starts the unbonding process for a validator, relay, or delegator.
    /// The tokens will remain locked during the cooldown period specific to the node type:
    /// - Validators: 14 days
    /// - Relays: 7 days
    /// - Delegations: 7 days
    ///
    /// # Arguments
    /// * `operator` - The operator's UserId
    /// * `amount` - Amount of tokens to unbond (or 0 for all)
    /// * `node_type` - Type of node ("validator", "relay", "delegation")
    ///
    /// # Returns
    /// The unbonding record with full details
    pub fn initiate_stake_unbonding_with_type(
        &self,
        operator: &UserId,
        amount: u64,
        node_type: &str,
    ) -> Result<UnbondingRecord> {
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets
            .get_mut(operator)
            .ok_or_else(|| Error::NotFound(format!("Operator wallet not found: {}", operator)))?;

        // Determine amount to unbond
        let unbond_amount = if amount == 0 { wallet.staked } else { amount };

        if unbond_amount > wallet.staked {
            return Err(Error::validation(format!(
                "Insufficient staked balance: have {}, requested {}",
                wallet.staked, unbond_amount
            )));
        }

        // Check if there's already an unbonding in progress for this amount
        let mut unbonding_queue = self.unbonding_queue.write().unwrap();
        let existing_unbondings = unbonding_queue.entry(operator.clone()).or_default();

        let pending_unbond_total: u64 = existing_unbondings
            .iter()
            .filter(|r| r.status == UnbondingStatus::Pending)
            .map(|r| r.amount)
            .sum();

        if unbond_amount + pending_unbond_total > wallet.staked {
            return Err(Error::validation(format!(
                "Cannot unbond {}: {} already pending, only {} total staked",
                unbond_amount, pending_unbond_total, wallet.staked
            )));
        }

        // Calculate cooldown period based on node type (per ARCHITECTURE-2.0.md)
        let cooldown_days = match node_type {
            "validator" => 14, // 2 weeks for validators
            "relay" => 7,      // 1 week for relays
            "delegation" => 7, // 1 week for delegations
            _ => 7,            // Default 1 week
        };

        let now = Utc::now().timestamp();
        let cooldown_seconds = cooldown_days * 86400;
        let available_at = now + cooldown_seconds;

        // Create unbonding record
        let unbonding_id = Uuid::new_v4();
        let unbonding_record = UnbondingRecord {
            id: unbonding_id,
            user_id: operator.clone(),
            amount: unbond_amount,
            initiated_at: now,
            available_at,
            node_type: node_type.to_string(),
            operator: None,
            status: UnbondingStatus::Pending,
        };

        // Clone for return before moving into queue
        let result = unbonding_record.clone();
        existing_unbondings.push(unbonding_record);

        // Mark tokens as "unbonding" - they're still staked but can't be used
        // We don't reduce staked balance until cooldown completes
        // This is tracked in the unbonding queue

        // Create transaction record
        let tx = CurrencyTransaction {
            id: unbonding_id,
            tx_type: format!("initiate_unbonding_{}", node_type),
            from: operator.clone(),
            to: None,
            amount: unbond_amount,
            status: "pending".to_string(), // Pending until cooldown expires
            confirmations: 0,
            block_height: *self.current_block.read().unwrap(),
            created_at: now,
        };

        drop(wallets);
        drop(unbonding_queue);
        self.transactions.write().unwrap().insert(unbonding_id, tx);

        tracing::info!(
            "⏳ Initiated stake unbonding for {} ({}): {} tokens, available after {} ({} days cooldown, tx: {})",
            operator,
            node_type,
            unbond_amount,
            chrono::DateTime::from_timestamp(available_at, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| "unknown".to_string()),
            cooldown_days,
            unbonding_id
        );

        Ok(result)
    }

    /// Complete unbonding and withdraw tokens to wallet
    ///
    /// This completes the unbonding process after the cooldown period has elapsed.
    /// Tokens are moved from staked balance to available balance.
    ///
    /// # Arguments
    /// * `operator` - The operator's UserId
    /// * `unbonding_id` - Optional specific unbonding record to complete (or None for all ready)
    ///
    /// # Returns
    /// Total amount withdrawn
    pub fn complete_unbonding(&self, operator: &UserId, unbonding_id: Option<Uuid>) -> Result<u64> {
        let now = Utc::now().timestamp();
        let mut total_withdrawn = 0u64;

        let mut unbonding_queue = self.unbonding_queue.write().unwrap();
        let unbondings = unbonding_queue
            .get_mut(operator)
            .ok_or_else(|| Error::NotFound(format!("No unbonding records for: {}", operator)))?;

        let mut completed_ids = Vec::new();

        for record in unbondings.iter_mut() {
            // Skip if looking for specific ID and this isn't it
            if let Some(target_id) = unbonding_id {
                if record.id != target_id {
                    continue;
                }
            }

            // Check if this record is ready
            if record.status != UnbondingStatus::Pending {
                continue;
            }

            if now < record.available_at {
                let remaining_seconds = record.available_at - now;
                let remaining_days = remaining_seconds / 86400;
                let remaining_hours = (remaining_seconds % 86400) / 3600;

                tracing::debug!(
                    "Unbonding {} not yet ready: {} days {} hours remaining",
                    record.id,
                    remaining_days,
                    remaining_hours
                );

                // If looking for specific ID, return error
                if unbonding_id.is_some() {
                    return Err(Error::validation(format!(
                        "Unbonding {} still in cooldown: {} days {} hours remaining",
                        record.id, remaining_days, remaining_hours
                    )));
                }
                continue;
            }

            // Mark as ready then complete
            record.status = UnbondingStatus::Ready;
            total_withdrawn += record.amount;
            completed_ids.push((record.id, record.amount));
        }

        if total_withdrawn == 0 {
            return Err(Error::validation(
                "No unbonding records ready for withdrawal",
            ));
        }

        drop(unbonding_queue);

        // Update wallet balances
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets
            .get_mut(operator)
            .ok_or_else(|| Error::NotFound(format!("Wallet not found: {}", operator)))?;

        // Move from staked to available
        wallet.staked = wallet.staked.saturating_sub(total_withdrawn);
        wallet.balance += total_withdrawn;

        drop(wallets);

        // Mark unbonding records as completed
        let mut unbonding_queue = self.unbonding_queue.write().unwrap();
        if let Some(unbondings) = unbonding_queue.get_mut(operator) {
            for record in unbondings.iter_mut() {
                if completed_ids.iter().any(|(id, _)| *id == record.id) {
                    record.status = UnbondingStatus::Completed;
                }
            }
        }

        // Create completion transactions
        for (id, amount) in &completed_ids {
            let tx = CurrencyTransaction {
                id: *id,
                tx_type: "complete_unbonding".to_string(),
                from: operator.clone(),
                to: Some(operator.clone()),
                amount: *amount,
                status: "confirmed".to_string(),
                confirmations: 1,
                block_height: *self.current_block.read().unwrap(),
                created_at: now,
            };
            self.transactions.write().unwrap().insert(*id, tx);
        }

        tracing::info!(
            "✅ Completed unbonding for {}: {} tokens withdrawn ({} records)",
            operator,
            total_withdrawn,
            completed_ids.len()
        );

        Ok(total_withdrawn)
    }

    /// Get pending unbonding records for a user
    pub fn get_pending_unbondings(&self, operator: &UserId) -> Vec<UnbondingRecord> {
        let unbonding_queue = self.unbonding_queue.read().unwrap();
        unbonding_queue
            .get(operator)
            .map(|records| {
                records
                    .iter()
                    .filter(|r| r.status == UnbondingStatus::Pending)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all unbonding records for a user
    pub fn get_all_unbondings(&self, operator: &UserId) -> Vec<UnbondingRecord> {
        let unbonding_queue = self.unbonding_queue.read().unwrap();
        unbonding_queue.get(operator).cloned().unwrap_or_default()
    }

    /// Cancel pending unbonding (re-stake the tokens)
    pub fn cancel_unbonding(&self, operator: &UserId, unbonding_id: Uuid) -> Result<()> {
        let mut unbonding_queue = self.unbonding_queue.write().unwrap();
        let unbondings = unbonding_queue
            .get_mut(operator)
            .ok_or_else(|| Error::NotFound(format!("No unbonding records for: {}", operator)))?;

        let record = unbondings
            .iter_mut()
            .find(|r| r.id == unbonding_id)
            .ok_or_else(|| {
                Error::NotFound(format!("Unbonding record not found: {}", unbonding_id))
            })?;

        if record.status != UnbondingStatus::Pending {
            return Err(Error::validation(format!(
                "Cannot cancel unbonding with status: {:?}",
                record.status
            )));
        }

        record.status = UnbondingStatus::Cancelled;

        tracing::info!(
            "🔄 Cancelled unbonding {} for {}: {} tokens re-staked",
            unbonding_id,
            operator,
            record.amount
        );

        Ok(())
    }

    /// Process unbonding queue - called periodically to update statuses
    pub fn process_unbonding_queue(&self) -> Vec<(UserId, Uuid, u64)> {
        let now = Utc::now().timestamp();
        let mut ready_records = Vec::new();

        let unbonding_queue = self.unbonding_queue.read().unwrap();
        for (user_id, records) in unbonding_queue.iter() {
            for record in records {
                if record.status == UnbondingStatus::Pending && now >= record.available_at {
                    ready_records.push((user_id.clone(), record.id, record.amount));
                }
            }
        }

        if !ready_records.is_empty() {
            tracing::info!(
                "📋 {} unbonding records ready for withdrawal",
                ready_records.len()
            );
        }

        ready_records
    }

    /// Calculate pending rewards for a staker
    ///
    /// Calculates the accumulated rewards based on:
    /// - Staked amount and duration
    /// - Block production (for validators)
    /// - Message relay (for relays)
    /// - Current epoch and reward rate
    ///
    /// # Arguments
    /// * `operator` - The operator's UserId
    /// * `blocks_produced` - Number of blocks produced (validators)
    /// * `messages_relayed` - Number of messages relayed (relays)
    /// * `current_epoch` - Current epoch number
    ///
    /// # Returns
    /// Total pending rewards (not yet claimed)
    pub fn calculate_pending_rewards(
        &self,
        operator: &UserId,
        blocks_produced: u64,
        messages_relayed: u64,
        current_epoch: u64,
    ) -> Result<u64> {
        let wallets = self.wallets.read().unwrap();
        let wallet = wallets
            .get(operator)
            .ok_or_else(|| Error::NotFound(format!("Operator wallet not found: {}", operator)))?;

        // Base reward rate: 5% APY (approximately 0.0137% per day)
        // Adjusted based on stake amount and participation
        const BASE_APY_BPS: u64 = 500; // 5% in basis points
        const BLOCKS_PER_EPOCH: u64 = 1000;
        const MESSAGES_PER_RELAY_REWARD: u64 = 100;

        // Calculate time-based staking rewards
        // Simplified: stake_reward = (staked * APY * epochs) / (365 * 10000)
        let staking_reward = (wallet.staked * BASE_APY_BPS * current_epoch) / (365 * 10000);

        // Calculate block production rewards (validators only)
        // Reward per block = 0.1 DCHAT (in motes, 8 decimals)
        // Normalize by epoch: more reward for high block production within epoch
        const REWARD_PER_BLOCK: u64 = 10_000_000; // 0.1 DCHAT = 10,000,000 motes
                                                  // Epoch efficiency bonus: if producing more than expected blocks per epoch
        let epoch_efficiency = if blocks_produced > 0 && current_epoch > 0 {
            let expected_blocks = current_epoch * BLOCKS_PER_EPOCH;
            // Bonus multiplier: 1.0 + (0.1 * efficiency_ratio) capped at 1.5x
            let efficiency_ratio = (blocks_produced as f64) / (expected_blocks as f64);
            (100 + ((efficiency_ratio * 10.0).min(50.0) as u64)).min(150)
        } else {
            100 // No bonus
        };
        let block_reward = (blocks_produced * REWARD_PER_BLOCK * epoch_efficiency) / 100;

        // Calculate relay rewards (relays only)
        // Reward per 100 messages = 0.01 DCHAT (in motes, 8 decimals)
        const REWARD_PER_MESSAGE_BATCH: u64 = 1_000_000; // 0.01 DCHAT = 1,000,000 motes
        let relay_reward =
            (messages_relayed / MESSAGES_PER_RELAY_REWARD) * REWARD_PER_MESSAGE_BATCH;

        // Total pending rewards (add to existing pending)
        let total_new_rewards = staking_reward + block_reward + relay_reward;
        let total_pending = wallet.rewards_pending + total_new_rewards;

        tracing::debug!(
            "Calculated rewards for {}: staking={}, blocks={}, relay={}, total_pending={}",
            operator,
            staking_reward,
            block_reward,
            relay_reward,
            total_pending
        );

        Ok(total_pending)
    }

    /// Update pending rewards for an operator
    ///
    /// This is called periodically to accumulate rewards into the pending balance.
    pub fn accumulate_rewards(
        &self,
        operator: &UserId,
        blocks_produced: u64,
        messages_relayed: u64,
        current_epoch: u64,
    ) -> Result<u64> {
        // Calculate rewards
        let new_rewards = {
            let wallets = self.wallets.read().unwrap();
            let wallet = wallets.get(operator).ok_or_else(|| {
                Error::NotFound(format!("Operator wallet not found: {}", operator))
            })?;

            const BASE_APY_BPS: u64 = 500;
            // Rewards in motes (8 decimals): 0.1 DCHAT per block, 0.01 DCHAT per 100 messages
            const REWARD_PER_BLOCK: u64 = 10_000_000; // 0.1 DCHAT = 10,000,000 motes
            const REWARD_PER_MESSAGE_BATCH: u64 = 1_000_000; // 0.01 DCHAT = 1,000,000 motes
            const MESSAGES_PER_RELAY_REWARD: u64 = 100;

            let staking_reward = (wallet.staked * BASE_APY_BPS * current_epoch) / (365 * 10000);
            let block_reward = blocks_produced * REWARD_PER_BLOCK;
            let relay_reward =
                (messages_relayed / MESSAGES_PER_RELAY_REWARD) * REWARD_PER_MESSAGE_BATCH;

            staking_reward + block_reward + relay_reward
        };

        // Update rewards_pending in wallet
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets
            .get_mut(operator)
            .ok_or_else(|| Error::NotFound(format!("Operator wallet not found: {}", operator)))?;

        wallet.rewards_pending += new_rewards;

        tracing::info!(
            "💰 Accumulated {} rewards for {} (total pending: {})",
            new_rewards,
            operator,
            wallet.rewards_pending
        );

        Ok(wallet.rewards_pending)
    }

    /// Generate or load server address from public key bytes
    ///
    /// This derives a deterministic UserId from a public key for use
    /// with wallet and staking operations.
    ///
    /// # Arguments
    /// * `public_key_bytes` - The 32-byte Ed25519 public key
    ///
    /// # Returns
    /// UserId derived from the public key
    pub fn address_from_public_key(public_key_bytes: &[u8; 32]) -> UserId {
        // Hash the public key with BLAKE3 and use first 16 bytes for UUID
        let hash = blake3::hash(public_key_bytes);
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&hash.as_bytes()[..16]);
        UserId(uuid::Uuid::from_bytes(uuid_bytes))
    }

    /// Slash staked tokens from an operator's account
    ///
    /// This is a production method that burns slashed tokens from the operator's
    /// staked balance. The burned tokens are removed from circulation.
    ///
    /// # Arguments
    /// * `operator` - The operator whose stake is being slashed
    /// * `amount` - Amount of tokens to slash (burn)
    ///
    /// # Returns
    /// Transaction ID for the slash operation
    ///
    /// # Security
    /// This method should only be called by authorized slashing mechanisms:
    /// - Direct slash for minor/medium offenses (by staking backend)
    /// - Governance-approved slash for major offenses
    pub fn slash_stake(&self, operator: &UserId, amount: u64) -> Result<Uuid> {
        if amount == 0 {
            return Err(Error::validation("Slash amount must be greater than 0"));
        }

        // 1. Get and validate operator's wallet
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets
            .get_mut(operator)
            .ok_or_else(|| Error::NotFound(format!("Operator wallet not found: {}", operator)))?;

        // 2. Verify sufficient staked balance
        if wallet.staked < amount {
            return Err(Error::validation(format!(
                "Insufficient staked balance for slash: have {}, need {}",
                wallet.staked, amount
            )));
        }

        // 3. Deduct from staked balance (tokens are burned, not transferred)
        wallet.staked -= amount;

        // 4. Record the burn via tokenomics (if available)
        if let Some(ref tokenomics) = self.tokenomics {
            let _ = tokenomics.burn_tokens(amount, BurnReason::Slash, operator.clone());
        }

        // 5. Create transaction record
        let now = Utc::now().timestamp();
        let tx = CurrencyTransaction {
            id: Uuid::new_v4(),
            tx_type: "slash".to_string(),
            from: operator.clone(),
            to: None, // Burned, not transferred
            amount,
            status: "confirmed".to_string(), // Slashes are immediately confirmed
            confirmations: 1,
            block_height: *self.current_block.read().unwrap(),
            created_at: now,
        };

        let tx_id = tx.id;
        drop(wallets);
        self.transactions.write().unwrap().insert(tx_id, tx);

        tracing::info!(
            "🔥 Slashed {} tokens from {} stake (burned, tx: {})",
            amount,
            operator,
            tx_id
        );

        Ok(tx_id)
    }

    /// Get transaction timestamp
    ///
    /// Returns the timestamp of when a transaction was created.
    /// Used for cooldown period calculations.
    pub fn get_transaction_timestamp(&self, tx_id: &Uuid) -> Result<chrono::DateTime<Utc>> {
        let txs = self.transactions.read().unwrap();
        let tx = txs
            .get(tx_id)
            .ok_or_else(|| Error::NotFound(format!("Transaction not found: {}", tx_id)))?;

        Ok(chrono::DateTime::from_timestamp(tx.created_at, 0).unwrap_or_else(Utc::now))
    }

    // ========================================================================
    // PAYMENT CHANNEL ESCROW OPERATIONS (MAINNET PRODUCTION)
    // ========================================================================

    /// Lock funds in payment channel escrow
    ///
    /// This is the production method for opening a payment channel.
    /// Funds are locked in escrow and can only be released via:
    /// - Cooperative close (both parties sign final state)
    /// - Unilateral close with dispute period
    /// - Dispute resolution
    ///
    /// # Arguments
    /// * `channel_id` - Unique identifier for the payment channel
    /// * `sender_id` - The sender (funder) of the channel
    /// * `receiver_id` - The receiver (payee) of the channel
    /// * `amount` - Amount of tokens to lock in escrow
    ///
    /// # Returns
    /// LockEscrowResult with transaction ID and lock details
    ///
    /// # Security
    /// - Funds are deducted from sender's balance
    /// - Escrowed funds cannot be spent until channel closes
    /// - Balance invariant: sender_balance + receiver_balance = capacity (always)
    pub fn lock_channel_escrow(
        &self,
        channel_id: &str,
        sender_id: &UserId,
        receiver_id: &UserId,
        amount: u64,
    ) -> Result<LockEscrowResult> {
        if amount == 0 {
            return Err(Error::validation("Escrow amount must be greater than 0"));
        }

        // 1. Check for duplicate channel
        {
            let escrows = self.payment_channel_escrows.read().unwrap();
            if escrows.contains_key(channel_id) {
                return Err(Error::validation(format!(
                    "Payment channel escrow already exists: {}",
                    channel_id
                )));
            }
        }

        // 2. Deduct from sender's wallet
        let mut wallets = self.wallets.write().unwrap();
        let sender_wallet = wallets
            .get_mut(sender_id)
            .ok_or_else(|| Error::NotFound(format!("Sender wallet not found: {}", sender_id)))?;

        if sender_wallet.balance < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance for escrow: have {}, need {}",
                sender_wallet.balance, amount
            )));
        }

        sender_wallet.balance -= amount;
        drop(wallets);

        // 3. Create escrow record
        let now = Utc::now().timestamp();
        let tx_id = Uuid::new_v4();

        let escrow = PaymentChannelEscrow {
            channel_id: channel_id.to_string(),
            sender_id: sender_id.clone(),
            receiver_id: receiver_id.clone(),
            capacity: amount,
            sender_balance: amount,
            receiver_balance: 0,
            status: PaymentChannelEscrowStatus::Active,
            funding_tx_id: tx_id,
            settlement_tx_id: None,
            created_at: now,
            updated_at: now,
            dispute_deadline: None,
        };

        self.payment_channel_escrows
            .write()
            .unwrap()
            .insert(channel_id.to_string(), escrow);

        // 4. Create transaction record
        let tx = CurrencyTransaction {
            id: tx_id,
            tx_type: "channel_escrow_lock".to_string(),
            from: sender_id.clone(),
            to: Some(receiver_id.clone()),
            amount,
            status: "confirmed".to_string(),
            confirmations: 1,
            block_height: *self.current_block.read().unwrap(),
            created_at: now,
        };

        self.transactions.write().unwrap().insert(tx_id, tx);

        tracing::info!(
            "🔒 Payment channel escrow locked: {} ({} tokens from {} to {}, tx: {})",
            channel_id,
            amount,
            sender_id,
            receiver_id,
            tx_id
        );

        Ok(LockEscrowResult {
            tx_id,
            channel_id: channel_id.to_string(),
            amount_locked: amount,
        })
    }

    /// Release payment channel escrow with final state distribution
    ///
    /// This is the production method for closing a payment channel.
    /// Funds are distributed according to the final state:
    /// - sender_balance goes back to sender's wallet
    /// - receiver_balance goes to receiver's wallet
    ///
    /// # Arguments
    /// * `channel_id` - The payment channel to close
    /// * `sender_balance` - Amount to return to sender
    /// * `receiver_balance` - Amount to pay to receiver
    /// * `is_dispute` - Whether this is a dispute resolution (affects tx type)
    ///
    /// # Returns
    /// ReleaseEscrowResult with transaction ID and distribution details
    ///
    /// # Security
    /// - Balance invariant: sender_balance + receiver_balance MUST equal capacity
    /// - Only callable when escrow is Active or Disputing
    /// - Caller must verify signatures before calling this method
    pub fn release_channel_escrow(
        &self,
        channel_id: &str,
        sender_balance: u64,
        receiver_balance: u64,
        is_dispute: bool,
    ) -> Result<ReleaseEscrowResult> {
        // 1. Get and validate escrow
        let mut escrows = self.payment_channel_escrows.write().unwrap();
        let escrow = escrows.get_mut(channel_id).ok_or_else(|| {
            Error::NotFound(format!("Payment channel escrow not found: {}", channel_id))
        })?;

        // 2. Verify escrow is in valid state
        if escrow.status == PaymentChannelEscrowStatus::Settled {
            return Err(Error::validation(format!(
                "Payment channel {} is already settled",
                channel_id
            )));
        }

        // 3. CRITICAL: Verify balance invariant
        let total = sender_balance + receiver_balance;
        if total != escrow.capacity {
            return Err(Error::validation(format!(
                "Balance invariant violation: {} + {} = {}, expected {}",
                sender_balance, receiver_balance, total, escrow.capacity
            )));
        }

        // 4. Store escrow details before dropping lock
        let sender_id = escrow.sender_id.clone();
        let receiver_id = escrow.receiver_id.clone();
        let funding_tx_id = escrow.funding_tx_id;

        // 5. Update escrow status
        let now = Utc::now().timestamp();
        let settlement_tx_id = Uuid::new_v4();
        escrow.sender_balance = sender_balance;
        escrow.receiver_balance = receiver_balance;
        escrow.status = if is_dispute {
            PaymentChannelEscrowStatus::Disputed
        } else {
            PaymentChannelEscrowStatus::Settled
        };
        escrow.settlement_tx_id = Some(settlement_tx_id);
        escrow.updated_at = now;
        drop(escrows);

        // 6. Distribute funds to wallets
        let mut wallets = self.wallets.write().unwrap();

        // Return sender's balance
        if sender_balance > 0 {
            let sender_wallet = wallets.entry(sender_id.clone()).or_insert_with(|| Wallet {
                user_id: sender_id.clone(),
                balance: 0,
                staked: 0,
                rewards_pending: 0,
            });
            sender_wallet.balance += sender_balance;
        }

        // Pay receiver
        if receiver_balance > 0 {
            let receiver_wallet = wallets
                .entry(receiver_id.clone())
                .or_insert_with(|| Wallet {
                    user_id: receiver_id.clone(),
                    balance: 0,
                    staked: 0,
                    rewards_pending: 0,
                });
            receiver_wallet.balance += receiver_balance;
        }
        drop(wallets);

        // 7. Create settlement transaction record
        let tx_type = if is_dispute {
            "channel_escrow_dispute"
        } else {
            "channel_escrow_release"
        };

        let tx = CurrencyTransaction {
            id: settlement_tx_id,
            tx_type: tx_type.to_string(),
            from: sender_id.clone(),
            to: Some(receiver_id.clone()),
            amount: receiver_balance, // Primary amount is receiver's payout
            status: "confirmed".to_string(),
            confirmations: 1,
            block_height: *self.current_block.read().unwrap(),
            created_at: now,
        };

        self.transactions
            .write()
            .unwrap()
            .insert(settlement_tx_id, tx);

        tracing::info!(
            "💰 Payment channel escrow released: {} (sender={}, receiver={}, type={}, tx: {}, funding_tx: {})",
            channel_id,
            sender_balance,
            receiver_balance,
            tx_type,
            settlement_tx_id,
            funding_tx_id
        );

        Ok(ReleaseEscrowResult {
            tx_id: settlement_tx_id,
            sender_refund: sender_balance,
            receiver_payout: receiver_balance,
        })
    }

    /// Get payment channel escrow by ID
    pub fn get_channel_escrow(&self, channel_id: &str) -> Option<PaymentChannelEscrow> {
        self.payment_channel_escrows
            .read()
            .unwrap()
            .get(channel_id)
            .cloned()
    }

    /// Initiate dispute period for unilateral close
    ///
    /// Sets the escrow to Disputing status with a dispute deadline.
    /// During this period, the counterparty can submit a newer state.
    ///
    /// # Arguments
    /// * `channel_id` - The payment channel
    /// * `dispute_window_seconds` - Duration of dispute period in seconds
    ///
    /// # Returns
    /// The dispute deadline timestamp
    pub fn initiate_channel_dispute(
        &self,
        channel_id: &str,
        dispute_window_seconds: i64,
    ) -> Result<i64> {
        let mut escrows = self.payment_channel_escrows.write().unwrap();
        let escrow = escrows.get_mut(channel_id).ok_or_else(|| {
            Error::NotFound(format!("Payment channel escrow not found: {}", channel_id))
        })?;

        if escrow.status != PaymentChannelEscrowStatus::Active {
            return Err(Error::validation(format!(
                "Payment channel {} is not active (status: {:?})",
                channel_id, escrow.status
            )));
        }

        let now = Utc::now().timestamp();
        let dispute_deadline = now + dispute_window_seconds;

        escrow.status = PaymentChannelEscrowStatus::Disputing;
        escrow.dispute_deadline = Some(dispute_deadline);
        escrow.updated_at = now;

        tracing::info!(
            "⏱️ Dispute period initiated for channel {}: deadline at {}",
            channel_id,
            dispute_deadline
        );

        Ok(dispute_deadline)
    }

    /// Query payment channel events from the blockchain
    ///
    /// This is used by the watchtower to detect ChannelCloseInitiated events
    /// for fraud detection and challenge submission.
    ///
    /// # Arguments
    /// * `event_type` - The type of event (e.g., "ChannelCloseInitiated", "ChannelChallenged")
    /// * `from_block` - Start block height (inclusive)
    /// * `to_block` - End block height (inclusive), None means latest
    ///
    /// # Returns
    /// Vector of event data as JSON values
    pub async fn get_payment_channel_events(
        &self,
        event_type: &str,
        from_block: u64,
        to_block: Option<u64>,
    ) -> Result<Vec<serde_json::Value>> {
        self.rpc_client
            .get_events(event_type, from_block, to_block)
            .await
    }
}

/// Implementation of dchat-privacy's CurrencyChainClient trait
/// Provides payment verification and token redemption tracking for blind tokens
impl PrivacyCurrencyChainClient for CurrencyChainClient {
    /// Verify payment transaction on currency chain
    fn verify_payment_transaction(
        &self,
        tx_hash: &str,
        expected_amount: u64,
        expected_recipient: &str,
    ) -> Result<bool> {
        let records = self.payment_records.read().unwrap();

        if let Some((amount, recipient, confirmed)) = records.get(tx_hash) {
            if *confirmed && *amount >= expected_amount && recipient == expected_recipient {
                return Ok(true);
            }
        }

        // Also check transaction cache for pending/confirmed transfers
        for tx in self.transactions.read().unwrap().values() {
            if tx.tx_type == "payment" && tx.status == "confirmed" {
                // Match by ID or check amount
                if tx.amount >= expected_amount {
                    if let Some(ref to) = tx.to {
                        if to.to_string() == expected_recipient {
                            return Ok(true);
                        }
                    }
                }
            }
        }

        Ok(false)
    }

    /// Check if token signature has been redeemed
    fn is_token_redeemed(&self, signature_hash: &[u8; 32]) -> Result<bool> {
        Ok(self
            .redeemed_tokens
            .read()
            .unwrap()
            .contains(signature_hash))
    }

    /// Mark token as redeemed on-chain
    fn mark_token_redeemed(&mut self, signature_hash: [u8; 32]) -> Result<()> {
        self.redeemed_tokens.write().unwrap().insert(signature_hash);
        Ok(())
    }
}

/// Additional methods for CurrencyChainClient (not part of PrivacyCurrencyChainClient trait)
impl CurrencyChainClient {
    /// Get the list of active validators from the blockchain
    pub async fn get_validators(&self) -> Result<Vec<ValidatorInfo>> {
        let params = serde_json::json!([]);

        match self.rpc_client.call_rpc("validators.list", params).await {
            Ok(result) => {
                if let Ok(validators) = serde_json::from_value::<Vec<ValidatorInfo>>(result) {
                    return Ok(validators);
                }
                tracing::warn!("Could not parse validator response from RPC");
                Ok(Vec::new())
            }
            Err(e) => {
                tracing::warn!("Failed to query validators from RPC: {}", e);
                Ok(Vec::new())
            }
        }
    }

    /// Delegate stake to a validator
    pub async fn delegate_stake(
        &self,
        user_id: &UserId,
        validator_id: &str,
        amount: u64,
    ) -> Result<String> {
        // Verify user has sufficient balance
        let wallet = self
            .get_wallet(user_id)?
            .ok_or_else(|| Error::NotFound(format!("Wallet not found: {}", user_id)))?;

        if wallet.balance < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance: have {}, need {}",
                wallet.balance, amount
            )));
        }

        // Submit delegation transaction via RPC
        let params = serde_json::json!({
            "delegator": user_id.0.to_string(),
            "validator": validator_id,
            "amount": amount
        });

        let result = self.rpc_client.call_rpc("staking.delegate", params).await?;

        if let Some(tx_hash) = result.get("tx_hash").and_then(|v| v.as_str()) {
            // Update local state
            let mut wallets = self.wallets.write().unwrap();
            if let Some(w) = wallets.get_mut(user_id) {
                w.balance -= amount;
                w.staked += amount;
            }
            return Ok(tx_hash.to_string());
        }

        Err(Error::chain_rpc("Invalid response from RPC"))
    }

    /// Get delegation amount for a user to a specific validator
    pub async fn get_delegation(&self, user_id: &UserId, validator_id: &str) -> Option<u64> {
        let params = serde_json::json!({
            "delegator": user_id.0.to_string(),
            "validator": validator_id
        });

        match self
            .rpc_client
            .call_rpc("staking.getDelegation", params)
            .await
        {
            Ok(result) => result.get("amount").and_then(|v| v.as_u64()),
            Err(_) => None,
        }
    }

    /// Get unbonding records for a user
    pub fn get_unbonding_records(&self, user_id: &UserId) -> Vec<UnbondingRecord> {
        self.get_all_unbondings(user_id)
            .into_iter()
            .filter(|r| matches!(r.status, UnbondingStatus::Pending))
            .collect()
    }

    /// Get reward history for a user
    pub async fn get_reward_history(
        &self,
        user_id: &UserId,
        limit: usize,
    ) -> Result<Vec<RewardHistoryEntry>> {
        let params = serde_json::json!({
            "user_id": user_id.0.to_string(),
            "limit": limit
        });

        match self.rpc_client.call_rpc("rewards.history", params).await {
            Ok(result) => {
                if let Ok(history) = serde_json::from_value::<Vec<RewardHistoryEntry>>(result) {
                    return Ok(history);
                }
                Ok(Vec::new())
            }
            Err(e) => {
                tracing::warn!("Failed to query reward history: {}", e);
                Ok(Vec::new())
            }
        }
    }

    /// Get pending rewards breakdown by type
    pub async fn get_pending_rewards_breakdown(
        &self,
        user_id: &UserId,
    ) -> Result<PendingRewardsBreakdown> {
        let params = serde_json::json!({
            "user_id": user_id.0.to_string()
        });

        match self
            .rpc_client
            .call_rpc("rewards.pendingBreakdown", params)
            .await
        {
            Ok(result) => {
                if let Ok(breakdown) = serde_json::from_value::<PendingRewardsBreakdown>(result) {
                    return Ok(breakdown);
                }
                // Return empty breakdown if parsing fails
                Ok(PendingRewardsBreakdown {
                    staking_rewards: 0,
                    relay_rewards: 0,
                    referral_rewards: 0,
                })
            }
            Err(e) => Err(Error::chain_rpc(format!(
                "Failed to query rewards breakdown: {}",
                e
            ))),
        }
    }

    /// Get all-time rewards breakdown for a user
    pub async fn get_all_time_rewards(&self, user_id: &UserId) -> Result<AllTimeRewardsBreakdown> {
        let params = serde_json::json!({
            "user_id": user_id.0.to_string()
        });

        match self
            .rpc_client
            .call_rpc("rewards.allTimeBreakdown", params)
            .await
        {
            Ok(result) => {
                if let Ok(breakdown) = serde_json::from_value::<AllTimeRewardsBreakdown>(result) {
                    return Ok(breakdown);
                }
                // Return default if parsing fails
                Ok(AllTimeRewardsBreakdown {
                    staking: 0,
                    relaying: 0,
                    referrals: 0,
                    governance: 0,
                    current_staking_apy: 12.0,
                    combined_apy_estimate: 15.0,
                })
            }
            Err(e) => Err(Error::chain_rpc(format!(
                "Failed to query all-time rewards: {}",
                e
            ))),
        }
    }

    /// Set auto-compound preference for a user
    pub async fn set_auto_compound(&self, user_id: &UserId, enable: bool) -> Result<()> {
        let params = serde_json::json!({
            "user_id": user_id.0.to_string(),
            "enable": enable
        });

        self.rpc_client
            .call_rpc("rewards.setAutoCompound", params)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_wallet() {
        let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        let user_id = UserId(Uuid::new_v4());
        let wallet = client.create_wallet(&user_id, 1000).unwrap();
        assert_eq!(wallet.balance, 1000);
    }

    #[test]
    fn test_transfer() {
        let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        let alice = UserId(Uuid::new_v4());
        let bob = UserId(Uuid::new_v4());

        client.create_wallet(&alice, 1000).unwrap();
        client.create_wallet(&bob, 0).unwrap();

        let tx_id = client.transfer(&alice, &bob, 100).unwrap();
        let tx = client.get_transaction(&tx_id).unwrap();
        assert!(tx.is_some());

        assert_eq!(client.get_balance(&alice).unwrap(), 900);
        assert_eq!(client.get_balance(&bob).unwrap(), 100);
    }

    #[test]
    fn test_stake() {
        let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        let user_id = UserId(Uuid::new_v4());

        client.create_wallet(&user_id, 1000).unwrap();
        let tx_id = client.stake(&user_id, 500, 86400).unwrap();
        let tx = client.get_transaction(&tx_id).unwrap();
        assert!(tx.is_some());

        let wallet = client.get_wallet(&user_id).unwrap().unwrap();
        assert_eq!(wallet.balance, 500);
        assert_eq!(wallet.staked, 500);
    }

    #[tokio::test]
    async fn test_block_sync() {
        let mut client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());

        // Start sync task
        client.start_sync().await.unwrap();

        // Give it time to poll once
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Stop sync
        client.stop_sync().await;
    }

    #[test]
    fn test_privacy_payment_verification() {
        let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());

        // Record a payment
        client.record_payment("tx_abc123", 1000, "recipient_wallet_1");

        // Verify payment via privacy trait
        let verified = client
            .verify_payment_transaction("tx_abc123", 1000, "recipient_wallet_1")
            .unwrap();
        assert!(verified);

        // Wrong amount should fail
        let verified = client
            .verify_payment_transaction(
                "tx_abc123",
                2000, // more than recorded
                "recipient_wallet_1",
            )
            .unwrap();
        assert!(!verified);

        // Wrong recipient should fail
        let verified = client
            .verify_payment_transaction("tx_abc123", 1000, "wrong_recipient")
            .unwrap();
        assert!(!verified);
    }

    #[test]
    fn test_privacy_token_redemption_tracking() {
        let mut client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());

        let token_hash: [u8; 32] = [0xCD; 32];

        // Initially not redeemed
        assert!(!client.is_token_redeemed(&token_hash).unwrap());

        // Mark as redeemed
        client.mark_token_redeemed(token_hash).unwrap();

        // Now should be redeemed
        assert!(client.is_token_redeemed(&token_hash).unwrap());
    }
}
