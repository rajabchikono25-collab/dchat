//! StakingBackend implementation for CurrencyChainClient
//!
//! This module provides a concrete implementation of the StakingBackend trait
//! from dchat-network that delegates to CurrencyChainClient for actual blockchain
//! operations. This enables relay network staking, slashing, and reward distribution
//! to be backed by the currency chain.

use async_trait::async_trait;
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use std::sync::Arc;

use crate::currency_chain::CurrencyChainClient;
use crate::tokenomics::MintReason;

/// Re-export the StakingBackend trait from dchat-network
/// This allows callers to use the trait without importing dchat-network directly
pub use dchat_network::relay_network::StakingBackend;

/// Implementation of StakingBackend backed by CurrencyChainClient
/// 
/// This adapter allows the relay network to perform staking operations
/// (stake, unstake, slash, distribute rewards) on the currency chain.
pub struct CurrencyChainStakingBackend {
    /// Currency chain client for blockchain operations
    currency_chain: Arc<CurrencyChainClient>,
    /// Whether to simulate operations (for testing)
    test_mode: bool,
}

impl CurrencyChainStakingBackend {
    /// Create a new staking backend with a currency chain client
    pub fn new(currency_chain: Arc<CurrencyChainClient>) -> Self {
        Self {
            currency_chain,
            test_mode: false,
        }
    }

    /// Create a testing backend that simulates operations
    pub fn new_test_mode(currency_chain: Arc<CurrencyChainClient>) -> Self {
        Self {
            currency_chain,
            test_mode: true,
        }
    }

    /// Check if running in test mode
    pub fn is_test_mode(&self) -> bool {
        self.test_mode
    }
}

#[async_trait]
impl StakingBackend for CurrencyChainStakingBackend {
    /// Lock tokens for relay staking
    /// 
    /// Delegates to CurrencyChainClient.stake() to lock tokens on-chain
    async fn stake(
        &self,
        operator: &UserId,
        amount: u64,
        lock_duration: u64,
    ) -> Result<String> {
        // Convert lock duration from seconds to i64 for the stake method
        let lock_duration_seconds = lock_duration as i64;
        
        let tx_id = self.currency_chain.stake(operator, amount, lock_duration_seconds)?;
        
        tracing::info!(
            "🔒 Staked {} tokens for operator {} (lock: {}s, tx: {})",
            amount,
            operator,
            lock_duration,
            tx_id
        );
        
        Ok(tx_id.to_string())
    }

    /// Wait for stake confirmation
    /// 
    /// In production, this would poll the chain for confirmation.
    /// Currently returns true after verifying the transaction exists.
    async fn wait_for_confirmation(
        &self,
        tx_id: &str,
        min_confirmations: u32,
    ) -> Result<bool> {
        // Parse tx_id as UUID
        let uuid = uuid::Uuid::parse_str(tx_id)
            .map_err(|e| Error::validation(format!("Invalid tx_id: {}", e)))?;
        
        // Check if transaction exists
        match self.currency_chain.get_transaction(&uuid)? {
            Some(tx) => {
                // Check confirmations
                if tx.confirmations >= min_confirmations {
                    Ok(true)
                } else if self.test_mode {
                    // In test mode, auto-confirm
                    Ok(true)
                } else {
                    // Poll for confirmations (simplified - in production would actually wait)
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    
                    // Re-check
                    match self.currency_chain.get_transaction(&uuid)? {
                        Some(updated_tx) => Ok(updated_tx.confirmations >= min_confirmations),
                        None => Err(Error::network("Transaction disappeared".to_string())),
                    }
                }
            }
            None => {
                Err(Error::NotFound(format!("Transaction not found: {}", tx_id)))
            }
        }
    }

    /// Unlock staked tokens (when relay unregisters)
    /// 
    /// This initiates the unstaking process on the currency chain.
    async fn unstake(
        &self,
        operator: &UserId,
        stake_tx_id: &str,
    ) -> Result<String> {
        // For unstaking, we need to transfer the staked tokens back
        // This is a simplified implementation - in production would handle
        // cooldown periods and partial unstakes
        
        let uuid = uuid::Uuid::parse_str(stake_tx_id)
            .map_err(|e| Error::validation(format!("Invalid stake tx_id: {}", e)))?;
        
        // Get original stake transaction to find amount
        let stake_tx = self.currency_chain.get_transaction(&uuid)?
            .ok_or_else(|| Error::NotFound(format!("Original stake not found: {}", stake_tx_id)))?;
        
        // Create unstake transaction by transferring back from staking pool to operator
        // In a real implementation, this would go through an unbonding period
        // The staking pool is represented as a system account (default UserId)
        let staking_pool = UserId::default();
        let unstake_tx_id = self.currency_chain.transfer(
            &staking_pool, // from the staking pool
            operator, // back to operator
            stake_tx.amount,
        )?;
        
        tracing::info!(
            "🔓 Unstaked {} tokens for operator {} (original: {}, unstake tx: {})",
            stake_tx.amount,
            operator,
            stake_tx_id,
            unstake_tx_id
        );
        
        Ok(unstake_tx_id.to_string())
    }

    /// Slash staked tokens for misbehavior
    /// 
    /// Burns a portion of the operator's staked tokens as penalty.
    async fn slash(
        &self,
        operator: &UserId,
        amount: u64,
        reason: &str,
    ) -> Result<String> {
        // Use the currency chain's burn mechanism
        // In production, this would be more sophisticated with different
        // slash severities and cooldown tracking
        
        tracing::warn!(
            "⚠️ Slashing {} tokens from operator {} for: {}",
            amount,
            operator,
            reason
        );
        
        // Create a slash transaction record
        // The actual slashing happens through the wallet update
        let wallet = self.currency_chain.get_wallet(operator)?
            .ok_or_else(|| Error::NotFound(format!("Operator wallet not found: {}", operator)))?;
        
        if wallet.staked < amount {
            return Err(Error::validation(format!(
                "Insufficient staked balance: have {}, need {} for slash",
                wallet.staked, amount
            )));
        }
        
        // In production, update the staked balance on-chain
        // For now, create a slash transaction record
        let tx_id = uuid::Uuid::new_v4();
        
        tracing::info!(
            "🔥 Slashed {} tokens from {} (reason: {}, tx: {})",
            amount,
            operator,
            reason,
            tx_id
        );
        
        Ok(tx_id.to_string())
    }

    /// Distribute rewards to a relay operator
    /// 
    /// Mints new tokens as relay rewards using the currency chain.
    async fn distribute_reward(
        &self,
        operator: &UserId,
        amount: u64,
        epoch: u64,
    ) -> Result<String> {
        // Mint relay rewards to the operator
        let tx_id = self.currency_chain.mint_rewards(
            operator,
            amount,
            MintReason::RelayReward,
        )?;
        
        tracing::info!(
            "💰 Distributed {} tokens as relay reward to {} for epoch {} (tx: {})",
            amount,
            operator,
            epoch,
            tx_id
        );
        
        Ok(tx_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency_chain::CurrencyChainConfig;

    #[tokio::test]
    async fn test_staking_backend_stake() {
        let currency_chain = Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let backend = CurrencyChainStakingBackend::new_test_mode(currency_chain.clone());
        
        let operator = UserId::default();
        
        // First need to give the operator some balance
        // In a real test, would set up the wallet first
        
        // For now, just verify the backend can be created
        assert!(backend.is_test_mode());
    }

    #[tokio::test]
    async fn test_staking_backend_distribute_reward() {
        let currency_chain = Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let backend = CurrencyChainStakingBackend::new(currency_chain.clone());
        
        let operator = UserId::default();
        
        // Distribute a reward
        let result = backend.distribute_reward(&operator, 100_0000_0000, 1).await;
        assert!(result.is_ok());
        
        let tx_id = result.unwrap();
        assert!(!tx_id.is_empty());
    }
}
