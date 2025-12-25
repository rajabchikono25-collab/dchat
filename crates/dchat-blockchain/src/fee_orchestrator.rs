//! Fee Orchestrator - Production-grade end-to-end fee charging pipeline
//!
//! This module provides the unified entry point for charging fees on all chat-chain
//! transactions. It orchestrates:
//!
//! 1. **Idempotency**: Deterministic deduplication keyed by operation_id
//! 2. **Wallet Debits**: Real token movement from payer to protocol sinks
//! 3. **Pool Funding**: Actual balance increases in validator/relay/treasury/insurance pools
//! 4. **Consensus Accounting**: Recording in FeeDistributionManager for block verification
//! 5. **Structured Receipts**: Full audit trail with tx IDs and amounts
//! 6. **Refunds**: Automatic rollback on chat-chain submission failure
//!
//! ## Fee Types
//!
//! - `ExecutionGasFee`: Per-transaction base fee + optional per-byte fee
//! - `ChannelCreationFee`: One-time fee for creating channels (pool-funded, no burn)
//!
//! ## Distribution (68/20/10/2)
//!
//! For pool-funded fees (ExecutionGasFee, ChannelCreationFee):
//! - 68% to Validator Rewards pool
//! - 20% to Relay Rewards pool
//! - 10% to Treasury
//! - 2% to Insurance Fund
//! - No burn (burn only applies to user-to-user transfers)

use crate::currency_chain::CurrencyChainClient;
use crate::fee_distribution::{
    FeeCollectionRecord, FeeDistributionManager, FeeType, PoolType, ProtocolSinks,
};
use chrono::{DateTime, Utc};
use dchat_chain::TransactionType;
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

// =============================================================================
// FEE CONFIGURATION
// =============================================================================

/// Fee configuration for chat-chain transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
    /// Base fee per transaction type (in smallest token units)
    pub base_fees: HashMap<TransactionType, u64>,
    /// Per-byte fee for payload data (0 = disabled)
    pub per_byte_fee: u64,
    /// One-time channel creation fee
    pub channel_creation_fee: u64,
    /// Maximum fee cap (prevents overflow attacks)
    pub max_fee_cap: u64,
    /// Minimum balance required to submit transactions
    pub min_balance_for_tx: u64,
}

impl Default for FeeConfig {
    fn default() -> Self {
        let mut base_fees = HashMap::new();
        // Base fees in smallest token units (1 DCHAT = 1_000_000_000 units)
        base_fees.insert(TransactionType::RegisterUser, 10_000); // 0.00001 DCHAT
        base_fees.insert(TransactionType::SendDirectMessage, 1_000); // 0.000001 DCHAT
        base_fees.insert(TransactionType::CreateChannel, 50_000); // 0.00005 DCHAT (gas only)
        base_fees.insert(TransactionType::PostToChannel, 1_000); // 0.000001 DCHAT
        base_fees.insert(TransactionType::JoinChannel, 5_000); // 0.000005 DCHAT
        base_fees.insert(TransactionType::UpdateProfile, 5_000); // 0.000005 DCHAT
        base_fees.insert(TransactionType::SubmitDeliveryProof, 2_000); // 0.000002 DCHAT

        Self {
            base_fees,
            per_byte_fee: 1,                 // 1 unit per byte of payload
            channel_creation_fee: 1_000_000, // 0.001 DCHAT one-time fee
            max_fee_cap: 100_000_000_000,    // 100 DCHAT max
            min_balance_for_tx: 10_000,      // 0.00001 DCHAT minimum
        }
    }
}

impl FeeConfig {
    /// Calculate gas fee for a transaction type with given payload size
    pub fn calculate_gas_fee(&self, tx_type: TransactionType, payload_bytes: usize) -> u64 {
        let base = self.base_fees.get(&tx_type).copied().unwrap_or(5_000);
        let byte_fee = (payload_bytes as u64).saturating_mul(self.per_byte_fee);
        let total = base.saturating_add(byte_fee);
        total.min(self.max_fee_cap)
    }
}

// =============================================================================
// FEE RECEIPT
// =============================================================================

/// Structured receipt from fee charging operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeReceipt {
    /// Unique operation ID (used for idempotency)
    pub operation_id: [u8; 32],
    /// Transaction ID for the currency movement
    pub fee_tx_id: Uuid,
    /// Fee type charged
    pub fee_type: FeeType,
    /// Gross amount charged from payer
    pub gross_amount: u64,
    /// Payer user ID
    pub payer: UserId,
    /// Whether this was a deduplicated replay (no actual charge)
    pub was_replay: bool,
    /// Timestamp of the operation
    pub timestamp: DateTime<Utc>,
    /// Context label for logging
    pub context_label: String,
    /// Breakdown of amounts per sink
    pub sink_amounts: SinkAmounts,
    /// Associated fee collection record (for consensus verification)
    pub fee_record: Option<FeeCollectionRecord>,
}

/// Breakdown of amounts credited to each sink
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SinkAmounts {
    /// Amount to validator rewards pool
    pub validator_pool: u64,
    /// Amount to relay rewards pool
    pub relay_pool: u64,
    /// Amount to treasury
    pub treasury: u64,
    /// Amount to insurance fund
    pub insurance_fund: u64,
    /// Amount burned (zero for pool-funded fees)
    pub burned: u64,
    /// Amount to direct recipient (e.g., relay for message fees)
    pub direct_recipient: u64,
}

impl SinkAmounts {
    /// Total of all sink amounts (should equal gross_amount)
    pub fn total(&self) -> u64 {
        self.validator_pool
            .saturating_add(self.relay_pool)
            .saturating_add(self.treasury)
            .saturating_add(self.insurance_fund)
            .saturating_add(self.burned)
            .saturating_add(self.direct_recipient)
    }
}

/// Refund receipt for failed operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundReceipt {
    /// Original operation ID
    pub operation_id: [u8; 32],
    /// Refund transaction ID
    pub refund_tx_id: Uuid,
    /// Amount refunded
    pub refund_amount: u64,
    /// Original fee receipt
    pub original_receipt: FeeReceipt,
    /// Timestamp of refund
    pub timestamp: DateTime<Utc>,
    /// Reason for refund
    pub reason: String,
}

// =============================================================================
// IDEMPOTENCY STATE
// =============================================================================

/// State of a fee operation for idempotency tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationState {
    /// Fee was charged, pending chat-chain submission
    Charged {
        receipt: FeeReceipt,
        charged_at: DateTime<Utc>,
    },
    /// Chat-chain submission succeeded, fee is final
    Committed {
        receipt: FeeReceipt,
        chat_tx_id: Uuid,
        committed_at: DateTime<Utc>,
    },
    /// Chat-chain submission failed, fee was refunded
    Refunded {
        receipt: RefundReceipt,
        refunded_at: DateTime<Utc>,
    },
}

// =============================================================================
// FEE ORCHESTRATOR
// =============================================================================

/// Fee Orchestrator - unified entry point for fee charging with idempotency
///
/// This orchestrates the complete fee charging pipeline:
/// 1. Idempotency check (prevent double-charging on retries)
/// 2. Debit payer wallet in currency chain
/// 3. Credit protocol pool sinks with real balances
/// 4. Record in FeeDistributionManager for consensus verification
/// 5. Return structured receipt
pub struct FeeOrchestrator {
    /// Currency chain client for wallet operations
    currency_chain: Arc<CurrencyChainClient>,
    /// Fee distribution manager for consensus accounting
    fee_distribution: Arc<FeeDistributionManager>,
    /// Fee configuration
    config: FeeConfig,
    /// Idempotency state (operation_id -> state)
    idempotency_state: Arc<RwLock<HashMap<[u8; 32], OperationState>>>,
    /// Protocol sinks for pool addresses
    sinks: ProtocolSinks,
}

impl FeeOrchestrator {
    /// Create new fee orchestrator
    pub fn new(
        currency_chain: Arc<CurrencyChainClient>,
        fee_distribution: Arc<FeeDistributionManager>,
        config: FeeConfig,
    ) -> Self {
        Self {
            currency_chain,
            fee_distribution,
            config,
            idempotency_state: Arc::new(RwLock::new(HashMap::new())),
            sinks: ProtocolSinks::default(),
        }
    }

    /// Get fee configuration
    pub fn config(&self) -> &FeeConfig {
        &self.config
    }

    /// Get currency chain client reference
    pub fn currency_chain(&self) -> &Arc<CurrencyChainClient> {
        &self.currency_chain
    }

    /// Get fee distribution manager reference
    pub fn fee_distribution(&self) -> &Arc<FeeDistributionManager> {
        &self.fee_distribution
    }

    /// Get protocol sinks
    pub fn sinks(&self) -> &ProtocolSinks {
        &self.sinks
    }

    /// Charge a fee with full orchestration
    ///
    /// This is the main entry point for charging fees. It handles:
    /// - Idempotency (same operation_id won't double-charge)
    /// - Real wallet debits
    /// - Pool funding with real balances
    /// - Consensus-verifiable accounting
    ///
    /// # Arguments
    /// * `payer` - User ID to charge
    /// * `fee_type` - Type of fee (ExecutionGasFee, ChannelCreationFee, etc.)
    /// * `gross_amount` - Total amount to charge
    /// * `direct_recipient` - Optional direct recipient (e.g., relay for message fees)
    /// * `operation_id` - Deterministic ID for idempotency (e.g., hash of tx params)
    /// * `context_label` - Human-readable label for logging
    ///
    /// # Returns
    /// FeeReceipt with all details including whether it was a replay
    pub fn charge_fee(
        &self,
        payer: &UserId,
        fee_type: FeeType,
        gross_amount: u64,
        direct_recipient: Option<&UserId>,
        operation_id: [u8; 32],
        context_label: &str,
    ) -> Result<FeeReceipt> {
        // Step 1: Idempotency check
        {
            let state = self.idempotency_state.read().unwrap();
            if let Some(existing) = state.get(&operation_id) {
                match existing {
                    OperationState::Charged { receipt, .. }
                    | OperationState::Committed { receipt, .. } => {
                        // Already charged - return existing receipt marked as replay
                        let mut replay_receipt = receipt.clone();
                        replay_receipt.was_replay = true;
                        tracing::info!(
                            "Fee already charged for operation {:?}, returning cached receipt",
                            hex::encode(&operation_id[..8])
                        );
                        return Ok(replay_receipt);
                    }
                    OperationState::Refunded { .. } => {
                        // Was refunded - can re-charge
                        tracing::info!(
                            "Previous charge for operation {:?} was refunded, allowing re-charge",
                            hex::encode(&operation_id[..8])
                        );
                    }
                }
            }
        }

        // Step 2: Validate payer has sufficient balance
        let payer_balance = self.currency_chain.get_balance(payer)?;
        if payer_balance < gross_amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance for fee: have {}, need {}",
                payer_balance, gross_amount
            )));
        }

        // Step 3: Calculate distribution (no burn for ExecutionGasFee and ChannelCreationFee)
        let (
            burn_amount,
            validator_share,
            relay_share,
            treasury_share,
            insurance_share,
            direct_share,
        ) = self
            .fee_distribution
            .calculate_distribution(gross_amount, false, None);

        // Generate transaction ID for the fee movement
        let fee_tx_id = Uuid::new_v4();

        // Step 4: Debit payer wallet
        // We use transfer_internal to move from payer to a temporary "protocol" holding
        // then distribute to pools
        self.currency_chain
            .debit_for_protocol_fee(payer, gross_amount, fee_tx_id)?;

        // Step 5: Credit pool sinks with real balances
        if validator_share > 0 {
            self.currency_chain
                .fund_pool(PoolType::ValidatorRewards, validator_share)?;
        }
        if relay_share > 0 {
            self.currency_chain
                .fund_pool(PoolType::RelayRewards, relay_share)?;
        }
        if treasury_share > 0 {
            self.currency_chain
                .fund_pool(PoolType::Treasury, treasury_share)?;
        }
        if insurance_share > 0 {
            self.currency_chain
                .fund_pool(PoolType::InsuranceFund, insurance_share)?;
        }

        // Handle direct recipient (e.g., message fees to relay)
        if let Some(recipient) = direct_recipient {
            if direct_share > 0 {
                self.currency_chain.credit_direct_recipient(
                    recipient,
                    direct_share,
                    "fee_direct_recipient",
                )?;
            }
        }

        // Step 6: Record in FeeDistributionManager for consensus accounting
        let fee_record = self.fee_distribution.collect_fee(
            fee_type,
            gross_amount,
            payer.clone(),
            direct_recipient.cloned(),
            fee_tx_id,
        )?;

        // Step 7: Build receipt
        let sink_amounts = SinkAmounts {
            validator_pool: validator_share,
            relay_pool: relay_share,
            treasury: treasury_share,
            insurance_fund: insurance_share,
            burned: burn_amount,
            direct_recipient: direct_share,
        };

        // Verify conservation
        debug_assert_eq!(
            sink_amounts.total(),
            gross_amount,
            "Fee distribution must conserve value"
        );

        let receipt = FeeReceipt {
            operation_id,
            fee_tx_id,
            fee_type,
            gross_amount,
            payer: payer.clone(),
            was_replay: false,
            timestamp: Utc::now(),
            context_label: context_label.to_string(),
            sink_amounts,
            fee_record: Some(fee_record),
        };

        // Step 8: Store in idempotency state
        {
            let mut state = self.idempotency_state.write().unwrap();
            state.insert(
                operation_id,
                OperationState::Charged {
                    receipt: receipt.clone(),
                    charged_at: Utc::now(),
                },
            );
        }

        tracing::info!(
            "✅ Fee charged: {} from {} for {} (tx: {}, op: {})",
            gross_amount,
            payer,
            context_label,
            fee_tx_id,
            hex::encode(&operation_id[..8])
        );

        Ok(receipt)
    }

    /// Mark an operation as committed (chat-chain tx succeeded)
    ///
    /// This updates the idempotency state to indicate the fee is final.
    pub fn mark_committed(&self, operation_id: [u8; 32], chat_tx_id: Uuid) -> Result<()> {
        let mut state = self.idempotency_state.write().unwrap();

        // Clone the receipt if exists and in Charged state
        let receipt_clone = state
            .get(&operation_id)
            .and_then(|existing| match existing {
                OperationState::Charged { receipt, .. } => Some(receipt.clone()),
                _ => None,
            });

        if let Some(receipt) = receipt_clone {
            state.insert(
                operation_id,
                OperationState::Committed {
                    receipt,
                    chat_tx_id,
                    committed_at: Utc::now(),
                },
            );
            tracing::info!(
                "Fee committed for operation {:?} with chat-chain tx {}",
                hex::encode(&operation_id[..8]),
                chat_tx_id
            );
            return Ok(());
        }

        // Check for other states
        match state.get(&operation_id) {
            Some(OperationState::Committed { .. }) => {
                // Already committed, idempotent
                Ok(())
            }
            Some(OperationState::Refunded { .. }) => Err(Error::InvalidInput(
                "Cannot commit a refunded operation".to_string(),
            )),
            None => Err(Error::NotFound(format!(
                "Operation not found: {:?}",
                hex::encode(&operation_id[..8])
            ))),
            _ => unreachable!(),
        }
    }

    /// Refund a charged fee (chat-chain tx failed)
    ///
    /// This reverses the fee charge and records a refund.
    pub fn refund_fee(&self, operation_id: [u8; 32], reason: &str) -> Result<RefundReceipt> {
        let existing = {
            let state = self.idempotency_state.read().unwrap();
            state.get(&operation_id).cloned()
        };

        match existing {
            Some(OperationState::Charged { receipt, .. }) => {
                // Refund the payer
                let refund_tx_id = self.currency_chain.refund_protocol_fee(
                    &receipt.payer,
                    receipt.gross_amount,
                    &receipt.sink_amounts,
                )?;

                let refund_receipt = RefundReceipt {
                    operation_id,
                    refund_tx_id,
                    refund_amount: receipt.gross_amount,
                    original_receipt: receipt,
                    timestamp: Utc::now(),
                    reason: reason.to_string(),
                };

                // Update idempotency state
                {
                    let mut state = self.idempotency_state.write().unwrap();
                    state.insert(
                        operation_id,
                        OperationState::Refunded {
                            receipt: refund_receipt.clone(),
                            refunded_at: Utc::now(),
                        },
                    );
                }

                tracing::info!(
                    "💸 Fee refunded: {} to {} for operation {:?} (reason: {})",
                    refund_receipt.refund_amount,
                    refund_receipt.original_receipt.payer,
                    hex::encode(&operation_id[..8]),
                    reason
                );

                Ok(refund_receipt)
            }
            Some(OperationState::Committed { .. }) => Err(Error::InvalidInput(
                "Cannot refund a committed operation".to_string(),
            )),
            Some(OperationState::Refunded { receipt, .. }) => {
                // Already refunded, return existing
                Ok(receipt)
            }
            None => Err(Error::NotFound(format!(
                "Operation not found: {:?}",
                hex::encode(&operation_id[..8])
            ))),
        }
    }

    /// Calculate deterministic operation ID from transaction parameters
    pub fn compute_operation_id(
        payer: &UserId,
        tx_type: TransactionType,
        payload_hash: &[u8; 32],
        nonce: u64,
    ) -> [u8; 32] {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(payer.0.as_bytes());
        hasher.update(&[tx_type as u8]);
        hasher.update(payload_hash);
        hasher.update(&nonce.to_le_bytes());
        hasher.finalize().into()
    }

    /// Charge gas fee for a chat-chain transaction
    ///
    /// Convenience method that calculates the gas fee and charges it.
    pub fn charge_gas_fee(
        &self,
        payer: &UserId,
        tx_type: TransactionType,
        payload_bytes: usize,
        operation_id: [u8; 32],
    ) -> Result<FeeReceipt> {
        let gas_fee = self.config.calculate_gas_fee(tx_type, payload_bytes);
        let context = format!("{:?} gas fee ({} bytes)", tx_type, payload_bytes);

        self.charge_fee(
            payer,
            FeeType::ExecutionGasFee,
            gas_fee,
            None,
            operation_id,
            &context,
        )
    }

    /// Charge channel creation fee (separate from gas fee)
    ///
    /// This is a one-time fee for creating a channel, pool-funded with no burn.
    pub fn charge_channel_creation_fee(
        &self,
        payer: &UserId,
        operation_id: [u8; 32],
    ) -> Result<FeeReceipt> {
        let fee = self.config.channel_creation_fee;

        self.charge_fee(
            payer,
            FeeType::ChannelCreationFee,
            fee,
            None,
            operation_id,
            "channel creation fee",
        )
    }

    /// Get operation state for debugging/auditing
    pub fn get_operation_state(&self, operation_id: &[u8; 32]) -> Option<OperationState> {
        self.idempotency_state
            .read()
            .unwrap()
            .get(operation_id)
            .cloned()
    }

    /// Get all operations (for debugging/auditing)
    pub fn get_all_operations(&self) -> HashMap<[u8; 32], OperationState> {
        self.idempotency_state.read().unwrap().clone()
    }

    /// Clear expired operations from idempotency state
    /// Operations older than `max_age` are removed to prevent memory growth
    pub fn cleanup_expired_operations(&self, max_age: chrono::Duration) {
        let now = Utc::now();
        let mut state = self.idempotency_state.write().unwrap();
        state.retain(|_, op_state| {
            let timestamp = match op_state {
                OperationState::Charged { charged_at, .. } => *charged_at,
                OperationState::Committed { committed_at, .. } => *committed_at,
                OperationState::Refunded { refunded_at, .. } => *refunded_at,
            };
            now.signed_duration_since(timestamp) < max_age
        });
    }
}

// =============================================================================
// CHAT-CHAIN TRANSACTION WRAPPER
// =============================================================================

/// Result of a fee-gated chat-chain transaction
#[derive(Debug, Clone)]
pub struct FeeGatedResult<T> {
    /// The result from the chat-chain operation
    pub result: T,
    /// Gas fee receipt
    pub gas_fee_receipt: FeeReceipt,
    /// Optional additional fee receipt (e.g., channel creation fee)
    pub additional_fee_receipt: Option<FeeReceipt>,
}

/// Chat-chain transaction wrapper with fee gating
///
/// This wraps chat-chain operations to automatically:
/// 1. Charge gas fee before submission
/// 2. Charge any additional fees (e.g., channel creation)
/// 3. Submit the chat-chain transaction
/// 4. Refund on failure
pub struct FeeGatedChatChain {
    orchestrator: Arc<FeeOrchestrator>,
}

impl FeeGatedChatChain {
    /// Create new fee-gated chat chain wrapper
    pub fn new(orchestrator: Arc<FeeOrchestrator>) -> Self {
        Self { orchestrator }
    }

    /// Get the fee orchestrator
    pub fn orchestrator(&self) -> &Arc<FeeOrchestrator> {
        &self.orchestrator
    }

    /// Execute a fee-gated chat-chain transaction
    ///
    /// This handles the full lifecycle:
    /// 1. Charge gas fee
    /// 2. Charge additional fees if applicable
    /// 3. Execute the chat-chain operation
    /// 4. Mark committed on success, refund on failure
    pub async fn execute_with_fees<F, Fut, T>(
        &self,
        payer: &UserId,
        tx_type: TransactionType,
        payload: &[u8],
        nonce: u64,
        additional_fee: Option<(FeeType, u64)>,
        operation: F,
    ) -> Result<FeeGatedResult<T>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        // Compute deterministic operation IDs
        let payload_hash: [u8; 32] = blake3::hash(payload).into();
        let gas_op_id = FeeOrchestrator::compute_operation_id(payer, tx_type, &payload_hash, nonce);
        let additional_op_id = {
            let mut id = gas_op_id;
            id[31] ^= 0x01; // Different ID for additional fee
            id
        };

        // Step 1: Charge gas fee
        let gas_receipt =
            self.orchestrator
                .charge_gas_fee(payer, tx_type, payload.len(), gas_op_id)?;

        // Step 2: Charge additional fee if specified
        let additional_receipt = if let Some((fee_type, amount)) = additional_fee {
            Some(self.orchestrator.charge_fee(
                payer,
                fee_type,
                amount,
                None,
                additional_op_id,
                &format!("{:?} additional fee", fee_type),
            )?)
        } else {
            None
        };

        // Step 3: Execute the chat-chain operation
        match operation().await {
            Ok(result) => {
                // Success - mark fees as committed
                // Use a synthetic chat_tx_id since we don't have the real one here
                let chat_tx_id = Uuid::new_v4();
                self.orchestrator.mark_committed(gas_op_id, chat_tx_id)?;
                if additional_receipt.is_some() {
                    self.orchestrator
                        .mark_committed(additional_op_id, chat_tx_id)?;
                }

                Ok(FeeGatedResult {
                    result,
                    gas_fee_receipt: gas_receipt,
                    additional_fee_receipt: additional_receipt,
                })
            }
            Err(e) => {
                // Failure - refund all fees
                let _ = self
                    .orchestrator
                    .refund_fee(gas_op_id, &format!("Chat-chain tx failed: {}", e));
                if additional_receipt.is_some() {
                    let _ = self
                        .orchestrator
                        .refund_fee(additional_op_id, &format!("Chat-chain tx failed: {}", e));
                }

                Err(e)
            }
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency_chain::CurrencyChainConfig;
    use crate::fee_distribution::FeeDistributionConfig;

    fn create_test_orchestrator() -> (FeeOrchestrator, UserId) {
        let currency_config = CurrencyChainConfig::default();
        let currency_chain = Arc::new(CurrencyChainClient::new_mock(currency_config));

        let fee_distribution =
            Arc::new(FeeDistributionManager::new(FeeDistributionConfig::default()));
        fee_distribution.start_block(1, 1_000_000_000);

        let fee_config = FeeConfig::default();

        // Create a test user with balance
        let payer = UserId(Uuid::new_v4());
        currency_chain
            .create_wallet(&payer, 100_000_000_000)
            .unwrap(); // 100 DCHAT

        let orchestrator = FeeOrchestrator::new(currency_chain, fee_distribution, fee_config);

        (orchestrator, payer)
    }

    #[test]
    fn test_charge_gas_fee() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [0u8; 32];
        let receipt = orchestrator
            .charge_gas_fee(&payer, TransactionType::RegisterUser, 100, operation_id)
            .unwrap();

        assert!(!receipt.was_replay);
        assert_eq!(receipt.fee_type, FeeType::ExecutionGasFee);
        assert!(receipt.gross_amount > 0);

        // Verify sink amounts sum to gross amount
        assert_eq!(receipt.sink_amounts.total(), receipt.gross_amount);

        // Verify payer balance decreased
        let new_balance = orchestrator.currency_chain.get_balance(&payer).unwrap();
        assert!(new_balance < 100_000_000_000);
    }

    #[test]
    fn test_idempotency_prevents_double_charge() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [1u8; 32];

        // First charge
        let receipt1 = orchestrator
            .charge_gas_fee(&payer, TransactionType::RegisterUser, 100, operation_id)
            .unwrap();
        assert!(!receipt1.was_replay);

        let balance_after_first = orchestrator.currency_chain.get_balance(&payer).unwrap();

        // Second charge with same operation_id
        let receipt2 = orchestrator
            .charge_gas_fee(&payer, TransactionType::RegisterUser, 100, operation_id)
            .unwrap();
        assert!(receipt2.was_replay);
        assert_eq!(receipt2.fee_tx_id, receipt1.fee_tx_id);

        // Balance should not change
        let balance_after_second = orchestrator.currency_chain.get_balance(&payer).unwrap();
        assert_eq!(balance_after_first, balance_after_second);
    }

    #[test]
    fn test_refund_on_failure() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [2u8; 32];
        let initial_balance = orchestrator.currency_chain.get_balance(&payer).unwrap();

        // Charge fee
        let receipt = orchestrator
            .charge_gas_fee(&payer, TransactionType::CreateChannel, 500, operation_id)
            .unwrap();

        let charged_balance = orchestrator.currency_chain.get_balance(&payer).unwrap();
        assert!(charged_balance < initial_balance);

        // Refund
        let refund = orchestrator
            .refund_fee(operation_id, "test failure")
            .unwrap();
        assert_eq!(refund.refund_amount, receipt.gross_amount);

        // Balance should be restored
        let refunded_balance = orchestrator.currency_chain.get_balance(&payer).unwrap();
        assert_eq!(refunded_balance, initial_balance);
    }

    #[test]
    fn test_pool_balances_increase() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [3u8; 32];

        // Get initial pool balances
        let initial_validator = orchestrator
            .currency_chain
            .get_pool_balance(PoolType::ValidatorRewards);
        let initial_relay = orchestrator
            .currency_chain
            .get_pool_balance(PoolType::RelayRewards);
        let initial_treasury = orchestrator
            .currency_chain
            .get_pool_balance(PoolType::Treasury);
        let initial_insurance = orchestrator
            .currency_chain
            .get_pool_balance(PoolType::InsuranceFund);

        // Charge fee
        let receipt = orchestrator
            .charge_gas_fee(
                &payer,
                TransactionType::SendDirectMessage,
                200,
                operation_id,
            )
            .unwrap();

        // Verify pool balances increased
        let final_validator = orchestrator
            .currency_chain
            .get_pool_balance(PoolType::ValidatorRewards);
        let final_relay = orchestrator
            .currency_chain
            .get_pool_balance(PoolType::RelayRewards);
        let final_treasury = orchestrator
            .currency_chain
            .get_pool_balance(PoolType::Treasury);
        let final_insurance = orchestrator
            .currency_chain
            .get_pool_balance(PoolType::InsuranceFund);

        assert_eq!(
            final_validator - initial_validator,
            receipt.sink_amounts.validator_pool
        );
        assert_eq!(final_relay - initial_relay, receipt.sink_amounts.relay_pool);
        assert_eq!(
            final_treasury - initial_treasury,
            receipt.sink_amounts.treasury
        );
        assert_eq!(
            final_insurance - initial_insurance,
            receipt.sink_amounts.insurance_fund
        );
    }

    #[test]
    fn test_channel_creation_fee() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [4u8; 32];

        let receipt = orchestrator
            .charge_channel_creation_fee(&payer, operation_id)
            .unwrap();

        assert_eq!(receipt.fee_type, FeeType::ChannelCreationFee);
        assert_eq!(
            receipt.gross_amount,
            orchestrator.config.channel_creation_fee
        );
        assert_eq!(receipt.sink_amounts.burned, 0); // No burn for channel fees
    }

    #[test]
    fn test_fee_accounting_matches_wallet_movements() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [5u8; 32];
        let initial_balance = orchestrator.currency_chain.get_balance(&payer).unwrap();

        // Charge fee
        let receipt = orchestrator
            .charge_gas_fee(&payer, TransactionType::PostToChannel, 1000, operation_id)
            .unwrap();

        // Verify payer debit matches gross amount
        let final_balance = orchestrator.currency_chain.get_balance(&payer).unwrap();
        assert_eq!(initial_balance - final_balance, receipt.gross_amount);

        // Verify FeeDistributionManager recorded the same amounts
        if let Some(fee_record) = &receipt.fee_record {
            assert_eq!(fee_record.gross_amount, receipt.gross_amount);
            assert_eq!(
                fee_record.validator_share,
                receipt.sink_amounts.validator_pool
            );
            assert_eq!(fee_record.relay_share, receipt.sink_amounts.relay_pool);
            assert_eq!(fee_record.treasury_share, receipt.sink_amounts.treasury);
            assert_eq!(
                fee_record.insurance_share,
                receipt.sink_amounts.insurance_fund
            );
        }
    }

    #[test]
    fn test_insufficient_balance_rejected() {
        let (orchestrator, _payer) = create_test_orchestrator();

        // Create a user with very low balance
        let poor_user = UserId(Uuid::new_v4());
        orchestrator
            .currency_chain
            .create_wallet(&poor_user, 100) // Very low balance
            .unwrap();

        let operation_id = [6u8; 32];

        // Should fail due to insufficient balance
        let result = orchestrator.charge_fee(
            &poor_user,
            FeeType::ExecutionGasFee,
            1_000_000, // More than balance
            None,
            operation_id,
            "test",
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Insufficient"));
    }

    #[test]
    fn test_mark_committed() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [7u8; 32];

        // Charge fee
        orchestrator
            .charge_gas_fee(&payer, TransactionType::JoinChannel, 50, operation_id)
            .unwrap();

        // Mark committed
        let chat_tx_id = Uuid::new_v4();
        orchestrator
            .mark_committed(operation_id, chat_tx_id)
            .unwrap();

        // Verify state
        let state = orchestrator.get_operation_state(&operation_id).unwrap();
        match state {
            OperationState::Committed {
                chat_tx_id: stored_id,
                ..
            } => {
                assert_eq!(stored_id, chat_tx_id);
            }
            _ => panic!("Expected Committed state"),
        }
    }

    #[test]
    fn test_cannot_refund_committed() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [8u8; 32];

        // Charge and commit
        orchestrator
            .charge_gas_fee(&payer, TransactionType::UpdateProfile, 50, operation_id)
            .unwrap();
        orchestrator
            .mark_committed(operation_id, Uuid::new_v4())
            .unwrap();

        // Try to refund - should fail
        let result = orchestrator.refund_fee(operation_id, "attempted refund");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("committed operation"));
    }

    #[test]
    fn test_can_recharge_after_refund() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [9u8; 32];

        // First charge
        orchestrator
            .charge_gas_fee(&payer, TransactionType::RegisterUser, 100, operation_id)
            .unwrap();

        // Refund
        orchestrator.refund_fee(operation_id, "test").unwrap();

        // Can charge again with same operation_id
        let receipt = orchestrator
            .charge_gas_fee(&payer, TransactionType::RegisterUser, 100, operation_id)
            .unwrap();

        assert!(!receipt.was_replay); // Should NOT be replay since it was refunded
    }

    #[test]
    fn test_compute_operation_id_deterministic() {
        let payer = UserId(Uuid::new_v4());
        let tx_type = TransactionType::SendDirectMessage;
        let payload_hash = [0xABu8; 32];
        let nonce = 12345u64;

        let id1 = FeeOrchestrator::compute_operation_id(&payer, tx_type, &payload_hash, nonce);
        let id2 = FeeOrchestrator::compute_operation_id(&payer, tx_type, &payload_hash, nonce);

        assert_eq!(id1, id2);

        // Different nonce = different ID
        let id3 = FeeOrchestrator::compute_operation_id(&payer, tx_type, &payload_hash, nonce + 1);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_conservation_across_charge_and_refund() {
        let (orchestrator, payer) = create_test_orchestrator();

        let operation_id = [10u8; 32];
        let initial_balance = orchestrator.currency_chain.get_balance(&payer).unwrap();

        // Charge
        orchestrator
            .charge_gas_fee(&payer, TransactionType::CreateChannel, 500, operation_id)
            .unwrap();

        // Refund
        orchestrator.refund_fee(operation_id, "test").unwrap();

        // Balance should be exactly restored (conservation)
        let final_balance = orchestrator.currency_chain.get_balance(&payer).unwrap();
        assert_eq!(initial_balance, final_balance);
    }
}
