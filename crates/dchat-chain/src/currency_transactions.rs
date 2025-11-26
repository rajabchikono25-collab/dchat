//! Currency chain transaction types
//!
//! Defines all transaction types for the currency chain including:
//! - Token transfers
//! - Staking operations (stake, unstake, delegate)
//! - Reward distribution
//! - Slashing penalties
//! - Payment verification for relay rewards

use chrono::{DateTime, Utc};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Currency chain transaction type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CurrencyTransactionType {
    /// Transfer tokens between accounts
    Transfer,
    /// Stake tokens for validator/relay
    Stake,
    /// Unstake tokens (with timelock)
    Unstake,
    /// Delegate stake to validator
    Delegate,
    /// Undelegate stake
    Undelegate,
    /// Claim rewards from staking/relay
    ClaimRewards,
    /// Slash validator/relay stake (penalty)
    Slash,
    /// Distribute block rewards
    BlockReward,
    /// Relay payment for message delivery
    RelayPayment,
    /// Token-gated channel access payment
    ChannelAccess,
}

/// Token transfer transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Sender account ID
    pub from: UserId,
    /// Recipient account ID
    pub to: UserId,
    /// Amount in smallest unit (like satoshis)
    pub amount: u128,
    /// Optional memo/note
    pub memo: Option<String>,
    /// Transaction fee
    pub fee: u64,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Nonce (for ordering)
    pub nonce: u64,
    /// Signature (Ed25519)
    pub signature: Vec<u8>,
}

/// Staking transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Staker account ID
    pub staker: UserId,
    /// Amount to stake
    pub amount: u128,
    /// Staking type
    pub stake_type: StakeType,
    /// Lock duration in days (optional)
    pub lock_duration_days: Option<u32>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Vec<u8>,
}

/// Staking type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StakeType {
    /// Stake as validator node
    Validator,
    /// Stake as relay node
    Relay,
    /// Stake for reputation/Sybil resistance
    Reputation,
}

/// Unstaking transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnstakeTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Unstaker account ID
    pub unstaker: UserId,
    /// Amount to unstake
    pub amount: u128,
    /// Original stake type
    pub stake_type: StakeType,
    /// Unlock timestamp (unstaking timelock)
    pub unlock_at: DateTime<Utc>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Vec<u8>,
}

/// Delegation transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Delegator account ID
    pub delegator: UserId,
    /// Validator account ID to delegate to
    pub validator: UserId,
    /// Amount to delegate
    pub amount: u128,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Vec<u8>,
}

/// Undelegation transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndelegateTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Delegator account ID
    pub delegator: UserId,
    /// Validator account ID
    pub validator: UserId,
    /// Amount to undelegate
    pub amount: u128,
    /// Unlock timestamp
    pub unlock_at: DateTime<Utc>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Vec<u8>,
}

/// Rewards claim transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimRewardsTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Claimer account ID
    pub claimer: UserId,
    /// Reward type
    pub reward_type: RewardType,
    /// Amount claimed
    pub amount: u128,
    /// Rewards for blocks/messages
    pub from_height: u64,
    pub to_height: u64,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Vec<u8>,
}

/// Reward type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RewardType {
    /// Block production rewards (validators)
    BlockProduction,
    /// Message relay rewards
    MessageRelay,
    /// Staking delegation rewards
    StakingDelegation,
    /// Proof-of-delivery rewards
    ProofOfDelivery,
}

/// Slashing transaction (punishment)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Validator being slashed
    pub validator: UserId,
    /// Slashing reason
    pub reason: SlashReason,
    /// Amount slashed
    pub slash_amount: u128,
    /// Remaining stake after slash
    pub remaining_stake: u128,
    /// Evidence hash
    pub evidence_hash: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Authorized by (governance vote ID)
    pub authorized_by: Uuid,
}

/// Slashing reason
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashReason {
    /// Double signing blocks
    DoubleSign,
    /// Excessive downtime
    Downtime,
    /// Invalid proof-of-delivery
    InvalidProof,
    /// Censorship attack
    Censorship,
    /// Byzantine behavior
    Byzantine,
}

/// Block reward distribution transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRewardTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Block height
    pub block_height: u64,
    /// Block proposer
    pub proposer: UserId,
    /// Total reward amount
    pub total_reward: u128,
    /// Proposer reward (typically 40%)
    pub proposer_reward: u128,
    /// Validator rewards (split among all validators)
    pub validator_rewards: Vec<(UserId, u128)>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// Relay payment transaction (micropayment for message delivery)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPaymentTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Payer (message sender)
    pub payer: UserId,
    /// Relay node receiving payment
    pub relay: UserId,
    /// Number of messages delivered
    pub message_count: u32,
    /// Total bytes relayed
    pub bytes_relayed: u64,
    /// Payment amount
    pub amount: u128,
    /// Proof-of-delivery hash
    pub pod_hash: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Vec<u8>,
}

/// Channel access payment (token-gated channels)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAccessTx {
    /// Transaction ID
    pub tx_id: Uuid,
    /// User paying for access
    pub user: UserId,
    /// Channel ID
    pub channel_id: Uuid,
    /// Channel creator (receives payment)
    pub creator: UserId,
    /// Access fee amount
    pub amount: u128,
    /// Access duration in days
    pub duration_days: u32,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Nonce
    pub nonce: u64,
    /// Signature
    pub signature: Vec<u8>,
}

/// Account balance
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Balance {
    /// Available balance
    pub available: u128,
    /// Staked balance (locked)
    pub staked: u128,
    /// Delegated balance
    pub delegated: u128,
    /// Pending unstakes
    pub unstaking: u128,
    /// Rewards pending claim
    pub unclaimed_rewards: u128,
    /// Total balance
    pub total: u128,
}

impl Balance {
    /// Calculate and update total balance, returning the new total
    pub fn calculate_total(&mut self) -> u128 {
        self.total = self.available
            + self.staked
            + self.delegated
            + self.unstaking
            + self.unclaimed_rewards;
        self.total
    }
}

/// Staking info for an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingInfo {
    /// Account ID
    pub account: UserId,
    /// Total staked amount
    pub total_staked: u128,
    /// Validator stake
    pub validator_stake: u128,
    /// Relay stake
    pub relay_stake: u128,
    /// Reputation stake
    pub reputation_stake: u128,
    /// Delegated to validators
    pub delegations: Vec<Delegation>,
    /// Pending unstakes
    pub pending_unstakes: Vec<PendingUnstake>,
}

/// Delegation info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    /// Validator being delegated to
    pub validator: UserId,
    /// Amount delegated
    pub amount: u128,
    /// Delegation timestamp
    pub delegated_at: DateTime<Utc>,
}

/// Pending unstake
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingUnstake {
    /// Amount unstaking
    pub amount: u128,
    /// Stake type
    pub stake_type: StakeType,
    /// Unlock timestamp
    pub unlock_at: DateTime<Utc>,
    /// Original stake timestamp
    pub staked_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_calculation() {
        let mut balance = Balance {
            available: 1000,
            staked: 500,
            delegated: 200,
            unstaking: 100,
            unclaimed_rewards: 50,
            total: 0,
        };

        balance.calculate_total();
        assert_eq!(balance.total, 1850);
    }

    #[test]
    fn test_transfer_tx_creation() {
        let tx = TransferTx {
            tx_id: Uuid::new_v4(),
            from: UserId::new(),
            to: UserId::new(),
            amount: 1000,
            memo: Some("Test transfer".to_string()),
            fee: 10,
            timestamp: Utc::now(),
            nonce: 1,
            signature: vec![],
        };

        assert_eq!(tx.amount, 1000);
        assert_eq!(tx.fee, 10);
    }

    #[test]
    fn test_stake_tx_types() {
        let validator_stake = StakeTx {
            tx_id: Uuid::new_v4(),
            staker: UserId::new(),
            amount: 10000,
            stake_type: StakeType::Validator,
            lock_duration_days: Some(30),
            timestamp: Utc::now(),
            nonce: 1,
            signature: vec![],
        };

        assert_eq!(validator_stake.stake_type, StakeType::Validator);
        assert_eq!(validator_stake.lock_duration_days, Some(30));
    }
}
