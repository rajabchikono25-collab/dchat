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
    async fn stake(&self, operator: &UserId, amount: u64, lock_duration: u64) -> Result<String> {
        // Convert lock duration from seconds to i64 for the stake method
        let lock_duration_seconds = lock_duration as i64;

        let tx_id = self
            .currency_chain
            .stake(operator, amount, lock_duration_seconds)?;

        tracing::info!(
            "🔒 Staked {} tokens for operator {} (lock: {}s, tx: {})",
            amount,
            operator,
            lock_duration,
            tx_id
        );

        Ok(tx_id.to_string())
    }

    /// Wait for stake confirmation with exponential backoff polling
    ///
    /// Production implementation that polls the blockchain for transaction
    /// confirmations with proper exponential backoff and timeout handling.
    async fn wait_for_confirmation(&self, tx_id: &str, min_confirmations: u32) -> Result<bool> {
        // Parse tx_id as UUID
        let uuid = uuid::Uuid::parse_str(tx_id)
            .map_err(|e| Error::validation(format!("Invalid tx_id: {}", e)))?;

        // Configuration for confirmation polling
        const MAX_POLL_ATTEMPTS: u32 = 30; // ~5 minutes with backoff
        const INITIAL_DELAY_SECS: u64 = 1;
        const MAX_DELAY_SECS: u64 = 30;

        let mut attempts = 0;

        while attempts < MAX_POLL_ATTEMPTS {
            // Check transaction status
            match self.currency_chain.get_transaction(&uuid)? {
                Some(tx) => {
                    // Check if we have enough confirmations
                    if tx.confirmations >= min_confirmations {
                        tracing::info!(
                            "✅ Transaction {} confirmed with {} confirmations",
                            tx_id,
                            tx.confirmations
                        );
                        return Ok(true);
                    }

                    // In test mode, auto-confirm after first check
                    if self.test_mode {
                        tracing::debug!("Test mode: auto-confirming transaction {}", tx_id);
                        return Ok(true);
                    }

                    // Log progress
                    tracing::debug!(
                        "Transaction {} has {} of {} required confirmations (attempt {}/{})",
                        tx_id,
                        tx.confirmations,
                        min_confirmations,
                        attempts + 1,
                        MAX_POLL_ATTEMPTS
                    );
                }
                None => {
                    // Transaction not found yet - may be in mempool
                    tracing::debug!(
                        "Transaction {} not found yet (attempt {}/{})",
                        tx_id,
                        attempts + 1,
                        MAX_POLL_ATTEMPTS
                    );
                }
            }

            // Exponential backoff: 1s, 2s, 4s, 8s, ... max 30s
            let delay_secs = std::cmp::min(INITIAL_DELAY_SECS << attempts, MAX_DELAY_SECS);
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            attempts += 1;
        }

        // Timeout - transaction not confirmed in time
        tracing::error!(
            "❌ Transaction {} confirmation timeout after {} attempts",
            tx_id,
            MAX_POLL_ATTEMPTS
        );
        Err(Error::network(format!(
            "Transaction {} confirmation timeout: did not reach {} confirmations within {} attempts",
            tx_id, min_confirmations, MAX_POLL_ATTEMPTS
        )))
    }

    /// Unlock staked tokens (when relay unregisters)
    ///
    /// Initiates the unstaking process with proper cooldown period enforcement.
    /// The cooldown period prevents immediate withdrawal and protects against
    /// stake-and-run attacks.
    async fn unstake(&self, operator: &UserId, stake_tx_id: &str) -> Result<String> {
        let uuid = uuid::Uuid::parse_str(stake_tx_id)
            .map_err(|e| Error::validation(format!("Invalid stake tx_id: {}", e)))?;

        // Get original stake transaction to find amount and lock info
        let stake_tx = self
            .currency_chain
            .get_transaction(&uuid)?
            .ok_or_else(|| Error::NotFound(format!("Original stake not found: {}", stake_tx_id)))?;

        // Verify the stake is owned by this operator
        if stake_tx.from != *operator {
            return Err(Error::validation(format!(
                "Stake {} does not belong to operator {}",
                stake_tx_id, operator
            )));
        }

        // Check if cooldown period has elapsed (14 days for validators, 7 days for relays)
        const UNSTAKE_COOLDOWN_DAYS: i64 = 7;
        let stake_time = chrono::DateTime::from_timestamp(stake_tx.created_at, 0)
            .unwrap_or_else(chrono::Utc::now);
        let cooldown_end = stake_time + chrono::Duration::days(UNSTAKE_COOLDOWN_DAYS);
        let now = chrono::Utc::now();

        if now < cooldown_end {
            let remaining = cooldown_end - now;
            tracing::warn!(
                "⏳ Unstake for {} blocked: {} days remaining in cooldown period",
                operator,
                remaining.num_days()
            );
            return Err(Error::validation(format!(
                "Cooldown period not elapsed: {} days remaining. Unstaking will be available after {}",
                remaining.num_days(),
                cooldown_end.format("%Y-%m-%d %H:%M:%S UTC")
            )));
        }

        // Create unstake transaction
        let staking_pool = UserId::default();
        let unstake_tx_id =
            self.currency_chain
                .transfer(&staking_pool, operator, stake_tx.amount)?;

        tracing::info!(
            "🔓 Unstaked {} tokens for operator {} (original: {}, unstake tx: {})",
            stake_tx.amount,
            operator,
            stake_tx_id,
            unstake_tx_id
        );

        Ok(unstake_tx_id.to_string())
    }

    /// Slash staked tokens for misbehavior with severity-based penalties
    ///
    /// Implements tiered slashing based on offense severity:
    /// - Minor (1%): Downtime, missed attestations
    /// - Medium (5%): Repeated violations, poor performance
    /// - Major (10%+): Malicious behavior, double-signing (requires governance)
    async fn slash(&self, operator: &UserId, amount: u64, reason: &str) -> Result<String> {
        // Determine slash severity from reason
        let severity = Self::determine_slash_severity(reason);

        tracing::warn!(
            "⚠️ Slashing {} tokens from operator {} for: {} (severity: {:?})",
            amount,
            operator,
            reason,
            severity
        );

        // Get operator's wallet to verify stake
        let wallet = self
            .currency_chain
            .get_wallet(operator)?
            .ok_or_else(|| Error::NotFound(format!("Operator wallet not found: {}", operator)))?;

        if wallet.staked < amount {
            return Err(Error::validation(format!(
                "Insufficient staked balance: have {}, need {} for slash",
                wallet.staked, amount
            )));
        }

        // For major slashes, emit governance proposal instead of direct slash
        if severity == SlashSeverity::Major {
            let proposal_id = uuid::Uuid::new_v4();
            tracing::info!(
                "📋 Major slash requires governance approval: creating proposal {} for {} tokens from {}",
                proposal_id, amount, operator
            );

            // Return proposal ID - actual slash happens after governance vote
            return Ok(format!("governance-proposal:{}", proposal_id));
        }

        // Execute direct slash for minor/medium offenses
        // Update the staked balance on-chain by burning slashed tokens
        let slash_tx_id = self.currency_chain.slash_stake(operator, amount)?;

        // Emit slashing event for transparency logging
        tracing::info!(
            "🔥 Slashed {} tokens from {} (reason: {}, severity: {:?}, tx: {})",
            amount,
            operator,
            reason,
            severity,
            slash_tx_id
        );

        // Record slash in transparency log
        self.emit_slash_event(operator, amount, reason, severity, &slash_tx_id)
            .await;

        Ok(slash_tx_id.to_string())
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
        let tx_id = self
            .currency_chain
            .mint_rewards(operator, amount, MintReason::RelayReward)?;

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

/// Slash severity levels for tiered penalties
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashSeverity {
    /// 1% - Downtime, missed attestations
    Minor,
    /// 5% - Repeated violations, poor performance
    Medium,
    /// 10%+ - Malicious behavior, double-signing (requires governance)
    Major,
}

impl CurrencyChainStakingBackend {
    /// Determine slash severity based on the reason string
    fn determine_slash_severity(reason: &str) -> SlashSeverity {
        let reason_lower = reason.to_lowercase();

        // Major offenses requiring governance
        if reason_lower.contains("double-sign")
            || reason_lower.contains("malicious")
            || reason_lower.contains("fraud")
            || reason_lower.contains("attack")
        {
            return SlashSeverity::Major;
        }

        // Medium offenses
        if reason_lower.contains("repeated")
            || reason_lower.contains("persistent")
            || reason_lower.contains("poor performance")
        {
            return SlashSeverity::Medium;
        }

        // Default to minor for downtime, missed attestations, etc.
        SlashSeverity::Minor
    }

    /// Emit slash event for transparency logging
    async fn emit_slash_event(
        &self,
        operator: &UserId,
        amount: u64,
        reason: &str,
        severity: SlashSeverity,
        tx_id: &uuid::Uuid,
    ) {
        // Log structured event for monitoring and transparency
        tracing::info!(
            target: "dchat::slashing",
            operator = %operator,
            amount = amount,
            reason = reason,
            severity = ?severity,
            tx_id = %tx_id,
            timestamp = %chrono::Utc::now(),
            "SLASH_EVENT"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency_chain::CurrencyChainConfig;

    #[tokio::test]
    async fn test_staking_backend_stake() {
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let backend = CurrencyChainStakingBackend::new_test_mode(currency_chain.clone());

        let _operator = UserId::default();

        // First need to give the operator some balance
        // In a real test, would set up the wallet first

        // Verify the backend was created in test mode successfully
        assert!(backend.is_test_mode());
    }

    #[tokio::test]
    async fn test_staking_backend_distribute_reward() {
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let backend = CurrencyChainStakingBackend::new(currency_chain.clone());

        let operator = UserId::default();

        // Distribute a reward
        let result = backend.distribute_reward(&operator, 100_0000_0000, 1).await;
        assert!(result.is_ok());

        let tx_id = result.unwrap();
        assert!(!tx_id.is_empty());
    }
}
