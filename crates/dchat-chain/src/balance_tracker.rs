//! Balance tracker for currency chain
//!
//! Maintains account balances across transaction history, tracking
//! available funds, staked amounts, delegations, pending unstakes,
//! and unclaimed rewards.

use crate::currency_transaction_parser::{ParsedTransaction, TransactionData};
use crate::currency_transactions::*;
use dchat_core::types::UserId;
use dchat_core::{Error, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Balance tracker maintaining account state
pub struct BalanceTracker {
    /// Account balances
    balances: Arc<RwLock<HashMap<UserId, Balance>>>,
    /// Staking information per account
    staking_info: Arc<RwLock<HashMap<UserId, StakingInfo>>>,
    /// Total supply in circulation
    total_supply: Arc<RwLock<u128>>,
    /// Total staked across all accounts
    total_staked: Arc<RwLock<u128>>,
}

impl BalanceTracker {
    /// Create new balance tracker
    pub fn new() -> Self {
        Self {
            balances: Arc::new(RwLock::new(HashMap::new())),
            staking_info: Arc::new(RwLock::new(HashMap::new())),
            total_supply: Arc::new(RwLock::new(0)),
            total_staked: Arc::new(RwLock::new(0)),
        }
    }

    /// Process a transaction and update balances
    pub async fn process_transaction(&self, transaction: &ParsedTransaction) -> Result<()> {
        match &transaction.data {
            TransactionData::Transfer(tx) => self.process_transfer(tx).await,
            TransactionData::Stake(tx) => self.process_stake(tx).await,
            TransactionData::Unstake(tx) => self.process_unstake(tx).await,
            TransactionData::Delegate(tx) => self.process_delegate(tx).await,
            TransactionData::Undelegate(tx) => self.process_undelegate(tx).await,
            TransactionData::ClaimRewards(tx) => self.process_claim_rewards(tx).await,
            TransactionData::Slash(tx) => self.process_slash(tx).await,
            TransactionData::BlockReward(tx) => self.process_block_reward(tx).await,
            TransactionData::RelayPayment(tx) => self.process_relay_payment(tx).await,
            TransactionData::ChannelAccess(tx) => self.process_channel_access(tx).await,
        }
    }

    /// Process transfer transaction
    async fn process_transfer(&self, tx: &TransferTx) -> Result<()> {
        let mut balances = self.balances.write().await;

        // Deduct from sender (amount + fee)
        let sender_balance = balances.entry(tx.from).or_insert(Balance::default());
        if sender_balance.available < tx.amount + tx.fee as u128 {
            return Err(Error::validation("Insufficient balance for transfer"));
        }
        sender_balance.available -= tx.amount + tx.fee as u128;
        sender_balance.total = sender_balance.calculate_total();

        // Add to recipient
        let recipient_balance = balances.entry(tx.to).or_insert(Balance::default());
        recipient_balance.available += tx.amount;
        recipient_balance.total = recipient_balance.calculate_total();

        debug!(
            "Transfer: {} -> {} amount: {} fee: {}",
            tx.from.0, tx.to.0, tx.amount, tx.fee
        );

        Ok(())
    }

    /// Process stake transaction
    async fn process_stake(&self, tx: &StakeTx) -> Result<()> {
        let mut balances = self.balances.write().await;
        let mut staking_info = self.staking_info.write().await;
        let mut total_staked = self.total_staked.write().await;

        // Move from available to staked
        let balance = balances.entry(tx.staker).or_insert(Balance::default());
        if balance.available < tx.amount {
            return Err(Error::validation("Insufficient balance for staking"));
        }
        balance.available -= tx.amount;
        balance.staked += tx.amount;
        balance.total = balance.calculate_total();

        // Update staking info
        let info = staking_info.entry(tx.staker).or_insert(StakingInfo {
            account: tx.staker,
            total_staked: 0,
            validator_stake: 0,
            relay_stake: 0,
            reputation_stake: 0,
            delegations: Vec::new(),
            pending_unstakes: Vec::new(),
        });

        match tx.stake_type {
            StakeType::Validator => info.validator_stake += tx.amount,
            StakeType::Relay => info.relay_stake += tx.amount,
            StakeType::Reputation => info.reputation_stake += tx.amount,
        }

        info.total_staked += tx.amount;
        *total_staked += tx.amount;

        info!(
            "Stake: {} amount: {} type: {:?}",
            tx.staker.0, tx.amount, tx.stake_type
        );

        Ok(())
    }

    /// Process unstake transaction
    async fn process_unstake(&self, tx: &UnstakeTx) -> Result<()> {
        let mut balances = self.balances.write().await;
        let mut staking_info = self.staking_info.write().await;
        let mut total_staked = self.total_staked.write().await;

        // Move from staked to unstaking (with timelock)
        let balance = balances.entry(tx.unstaker).or_insert(Balance::default());
        if balance.staked < tx.amount {
            return Err(Error::validation("Insufficient staked balance"));
        }
        balance.staked -= tx.amount;
        balance.unstaking += tx.amount;
        balance.total = balance.calculate_total();

        // Update staking info
        let info = staking_info.entry(tx.unstaker).or_insert(StakingInfo {
            account: tx.unstaker,
            total_staked: 0,
            validator_stake: 0,
            relay_stake: 0,
            reputation_stake: 0,
            delegations: Vec::new(),
            pending_unstakes: Vec::new(),
        });

        match tx.stake_type {
            StakeType::Validator => {
                info.validator_stake = info.validator_stake.saturating_sub(tx.amount)
            }
            StakeType::Relay => info.relay_stake = info.relay_stake.saturating_sub(tx.amount),
            StakeType::Reputation => {
                info.reputation_stake = info.reputation_stake.saturating_sub(tx.amount)
            }
        }

        info.total_staked = info.total_staked.saturating_sub(tx.amount);

        // Add to pending unstakes
        info.pending_unstakes.push(PendingUnstake {
            amount: tx.amount,
            stake_type: tx.stake_type.clone(),
            unlock_at: tx.unlock_at,
            staked_at: tx.timestamp,
        });

        *total_staked = total_staked.saturating_sub(tx.amount);

        info!(
            "Unstake: {} amount: {} type: {:?} unlock_at: {}",
            tx.unstaker.0, tx.amount, tx.stake_type, tx.unlock_at
        );

        Ok(())
    }

    /// Process delegate transaction
    async fn process_delegate(&self, tx: &DelegateTx) -> Result<()> {
        let mut balances = self.balances.write().await;
        let mut staking_info = self.staking_info.write().await;

        // Move from available to delegated
        let balance = balances.entry(tx.delegator).or_insert(Balance::default());
        if balance.available < tx.amount {
            return Err(Error::validation("Insufficient balance for delegation"));
        }
        balance.available -= tx.amount;
        balance.delegated += tx.amount;
        balance.total = balance.calculate_total();

        // Update staking info
        let info = staking_info.entry(tx.delegator).or_insert(StakingInfo {
            account: tx.delegator,
            total_staked: 0,
            validator_stake: 0,
            relay_stake: 0,
            reputation_stake: 0,
            delegations: Vec::new(),
            pending_unstakes: Vec::new(),
        });

        info.delegations.push(Delegation {
            validator: tx.validator,
            amount: tx.amount,
            delegated_at: tx.timestamp,
        });

        info!(
            "Delegate: {} -> {} amount: {}",
            tx.delegator.0, tx.validator.0, tx.amount
        );

        Ok(())
    }

    /// Process undelegate transaction
    async fn process_undelegate(&self, tx: &UndelegateTx) -> Result<()> {
        let mut balances = self.balances.write().await;
        let mut staking_info = self.staking_info.write().await;

        // Move from delegated to unstaking (with timelock)
        let balance = balances.entry(tx.delegator).or_insert(Balance::default());
        if balance.delegated < tx.amount {
            return Err(Error::validation("Insufficient delegated balance"));
        }
        balance.delegated -= tx.amount;
        balance.unstaking += tx.amount;
        balance.total = balance.calculate_total();

        // Update staking info - remove from delegations
        if let Some(info) = staking_info.get_mut(&tx.delegator) {
            // Find and reduce delegation
            if let Some(delegation) = info
                .delegations
                .iter_mut()
                .find(|d| d.validator == tx.validator)
            {
                if delegation.amount >= tx.amount {
                    delegation.amount -= tx.amount;

                    // Remove delegation if amount is now zero
                    if delegation.amount == 0 {
                        info.delegations.retain(|d| d.validator != tx.validator);
                    }
                }
            }
        }

        info!(
            "Undelegate: {} <- {} amount: {} unlock_at: {}",
            tx.delegator.0, tx.validator.0, tx.amount, tx.unlock_at
        );

        Ok(())
    }

    /// Process claim rewards transaction
    async fn process_claim_rewards(&self, tx: &ClaimRewardsTx) -> Result<()> {
        let mut balances = self.balances.write().await;

        // Move from unclaimed_rewards to available
        let balance = balances.entry(tx.claimer).or_insert(Balance::default());
        if balance.unclaimed_rewards < tx.amount {
            warn!("Claiming more rewards than available, adjusting");
        }
        balance.unclaimed_rewards = balance.unclaimed_rewards.saturating_sub(tx.amount);
        balance.available += tx.amount;
        balance.total = balance.calculate_total();

        info!(
            "ClaimRewards: {} amount: {} type: {:?} blocks: {}-{}",
            tx.claimer.0, tx.amount, tx.reward_type, tx.from_height, tx.to_height
        );

        Ok(())
    }

    /// Process slash transaction
    async fn process_slash(&self, tx: &SlashTx) -> Result<()> {
        let mut balances = self.balances.write().await;
        let mut staking_info = self.staking_info.write().await;
        let mut total_staked = self.total_staked.write().await;

        // Reduce validator's staked balance
        let balance = balances.entry(tx.validator).or_insert(Balance::default());
        balance.staked = balance.staked.saturating_sub(tx.slash_amount);
        balance.total = balance.calculate_total();

        // Update staking info
        if let Some(info) = staking_info.get_mut(&tx.validator) {
            // Reduce validator stake proportionally
            let total_validator_stake =
                info.validator_stake + info.relay_stake + info.reputation_stake;
            if total_validator_stake > 0 {
                let ratio = tx.slash_amount as f64 / total_validator_stake as f64;
                info.validator_stake = (info.validator_stake as f64 * (1.0 - ratio)) as u128;
                info.relay_stake = (info.relay_stake as f64 * (1.0 - ratio)) as u128;
                info.reputation_stake = (info.reputation_stake as f64 * (1.0 - ratio)) as u128;
            }
        }

        *total_staked = total_staked.saturating_sub(tx.slash_amount);

        warn!(
            "Slash: {} amount: {} reason: {:?} remaining: {} authorized_by: {}",
            tx.validator.0, tx.slash_amount, tx.reason, tx.remaining_stake, tx.authorized_by
        );

        Ok(())
    }

    /// Process block reward transaction
    async fn process_block_reward(&self, tx: &BlockRewardTx) -> Result<()> {
        let mut balances = self.balances.write().await;
        let mut total_supply = self.total_supply.write().await;

        // Mint new tokens for proposer (40%)
        let proposer_balance = balances.entry(tx.proposer).or_insert(Balance::default());
        proposer_balance.unclaimed_rewards += tx.proposer_reward;
        proposer_balance.total = proposer_balance.calculate_total();

        // Mint new tokens for validators (60%)
        for (validator, reward) in &tx.validator_rewards {
            let validator_balance = balances.entry(*validator).or_insert(Balance::default());
            validator_balance.unclaimed_rewards += reward;
            validator_balance.total = validator_balance.calculate_total();
        }

        *total_supply += tx.total_reward;

        info!(
            "BlockReward: height {} proposer: {} total: {} proposer_share: {}",
            tx.block_height, tx.proposer.0, tx.total_reward, tx.proposer_reward
        );

        Ok(())
    }

    /// Process relay payment transaction
    async fn process_relay_payment(&self, tx: &RelayPaymentTx) -> Result<()> {
        let mut balances = self.balances.write().await;

        // Transfer from payer to relay
        let payer_balance = balances.entry(tx.payer).or_insert(Balance::default());
        if payer_balance.available < tx.amount {
            return Err(Error::validation("Insufficient balance for relay payment"));
        }
        payer_balance.available -= tx.amount;
        payer_balance.total = payer_balance.calculate_total();

        let relay_balance = balances.entry(tx.relay).or_insert(Balance::default());
        relay_balance.available += tx.amount;
        relay_balance.total = relay_balance.calculate_total();

        info!(
            "RelayPayment: {} -> {} amount: {} messages: {} bytes: {}",
            tx.payer.0, tx.relay.0, tx.amount, tx.message_count, tx.bytes_relayed
        );

        Ok(())
    }

    /// Process channel access payment transaction
    async fn process_channel_access(&self, tx: &ChannelAccessTx) -> Result<()> {
        let mut balances = self.balances.write().await;

        // Transfer from user to channel creator
        let user_balance = balances.entry(tx.user).or_insert(Balance::default());
        if user_balance.available < tx.amount {
            return Err(Error::validation("Insufficient balance for channel access"));
        }
        user_balance.available -= tx.amount;
        user_balance.total = user_balance.calculate_total();

        let creator_balance = balances.entry(tx.creator).or_insert(Balance::default());
        creator_balance.available += tx.amount;
        creator_balance.total = creator_balance.calculate_total();

        info!(
            "ChannelAccess: {} -> {} amount: {} channel: {} duration: {} days",
            tx.user.0, tx.creator.0, tx.amount, tx.channel_id, tx.duration_days
        );

        Ok(())
    }

    /// Get balance for an account
    pub async fn get_balance(&self, user_id: UserId) -> Balance {
        self.balances
            .read()
            .await
            .get(&user_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get staking info for an account
    pub async fn get_staking_info(&self, user_id: UserId) -> Option<StakingInfo> {
        self.staking_info.read().await.get(&user_id).cloned()
    }

    /// Get total supply
    pub async fn get_total_supply(&self) -> u128 {
        *self.total_supply.read().await
    }

    /// Get total staked
    pub async fn get_total_staked(&self) -> u128 {
        *self.total_staked.read().await
    }

    /// Process completed timelocks (move from unstaking to available)
    pub async fn process_completed_timelocks(
        &self,
        current_time: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let mut balances = self.balances.write().await;
        let mut staking_info = self.staking_info.write().await;

        for (user_id, info) in staking_info.iter_mut() {
            let mut completed_amount = 0u128;

            // Find completed unstakes
            info.pending_unstakes.retain(|unstake| {
                if unstake.unlock_at <= current_time {
                    completed_amount += unstake.amount;
                    false // Remove from pending
                } else {
                    true // Keep pending
                }
            });

            // Move from unstaking to available
            if completed_amount > 0 {
                if let Some(balance) = balances.get_mut(user_id) {
                    balance.unstaking = balance.unstaking.saturating_sub(completed_amount);
                    balance.available += completed_amount;
                    balance.total = balance.calculate_total();

                    info!(
                        "Timelock completed: {} amount: {}",
                        user_id.0, completed_amount
                    );
                }
            }
        }

        Ok(())
    }
}

impl Default for BalanceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_transfer() {
        let tracker = BalanceTracker::new();

        let from = UserId(Uuid::new_v4());
        let to = UserId(Uuid::new_v4());

        // Set initial balance
        tracker.balances.write().await.insert(
            from,
            Balance {
                available: 1000,
                staked: 0,
                delegated: 0,
                unstaking: 0,
                unclaimed_rewards: 0,
                total: 1000,
            },
        );

        // Create transfer
        let tx = TransferTx {
            tx_id: Uuid::new_v4(),
            from,
            to,
            amount: 500,
            memo: None,
            fee: 10,
            timestamp: chrono::Utc::now(),
            nonce: 1,
            signature: vec![],
        };

        tracker.process_transfer(&tx).await.unwrap();

        let from_balance = tracker.get_balance(from).await;
        let to_balance = tracker.get_balance(to).await;

        assert_eq!(from_balance.available, 490); // 1000 - 500 - 10
        assert_eq!(to_balance.available, 500);
    }

    #[tokio::test]
    async fn test_stake() {
        let tracker = BalanceTracker::new();

        let staker = UserId(Uuid::new_v4());

        // Set initial balance
        tracker.balances.write().await.insert(
            staker,
            Balance {
                available: 1000,
                staked: 0,
                delegated: 0,
                unstaking: 0,
                unclaimed_rewards: 0,
                total: 1000,
            },
        );

        // Create stake
        let tx = StakeTx {
            tx_id: Uuid::new_v4(),
            staker,
            amount: 500,
            stake_type: StakeType::Validator,
            lock_duration_days: Some(30),
            timestamp: chrono::Utc::now(),
            nonce: 1,
            signature: vec![],
        };

        tracker.process_stake(&tx).await.unwrap();

        let balance = tracker.get_balance(staker).await;
        assert_eq!(balance.available, 500);
        assert_eq!(balance.staked, 500);
        assert_eq!(tracker.get_total_staked().await, 500);
    }
}
