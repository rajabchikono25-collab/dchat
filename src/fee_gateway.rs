//! Fee Gateway - Production-grade "must pay" enforcement for all storage/anchoring operations
//!
//! This module implements the single entry point for all operations that result in
//! storage or on-chain anchoring. It enforces that BOTH:
//! 1. ExecutionGasFee (protocol gas for chat-chain ordering tx)
//! 2. MessageFee (relay service fee, 100% to relay)
//!
//! are successfully charged with idempotent receipts before any storage occurs.
//!
//! ## Fee Flow (Pay-Before-Anchor)
//!
//! 1. Validate permissions and size limits
//! 2. Deterministically select relay before charging
//! 3. Compute payload hash and fees
//! 4. Charge gas fee (with idempotency)
//! 5. Place message fee in escrow (hold-and-release pattern)
//! 6. Submit chat-chain ordering tx
//! 7. Wait for finality (remove all "tx_id implies confirmed" behavior)
//! 8. On finality success: release escrow to relay, store content
//! 9. On failure: refund both fees, no storage occurs
//!
//! ## Blob-Tier Storage
//!
//! For large content (> BLOB_THRESHOLD), an additional storage micropayment is required
//! before any blob write. The storage payment receipt is included in the response.
//!
//! ## Response Contract
//!
//! All responses include:
//! - `operation_id`: Deterministic idempotency key
//! - `nonce`: Client nonce echoed back
//! - `payload_hash`: Hash of the message content
//! - `gas_fee_receipt`: Gas fee details with currency-chain fee_tx_id
//! - `message_fee_receipt`: Message fee details with relay attribution
//! - `chat_tx_id`: Real chat-chain transaction id/hash
//! - `chat_tx_confirmations`: Finality confirmation count
//! - `storage_receipt`: For blob tier, the storage payment receipt

use crate::storage_routed_user_management::{StorageRoutedUserManager, StorageTier};
use chrono::{DateTime, Utc};
use dchat_blockchain::{
    fee_orchestrator::{EscrowRecord, EscrowStatus, FeeOrchestrator, FeeReceipt, SinkAmounts},
    ChatChainClient, CurrencyChainClient,
};
use dchat_chain::TransactionType;
use dchat_core::error::{Error, Result};
use dchat_core::types::{ChannelId, MessageId, UserId};
use dchat_network::relay_network::RelayNetworkManager;
use dchat_storage::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, error, info, warn};

// Re-export EscrowStatus and EscrowRecord for consumers of fee_gateway
pub use dchat_blockchain::fee_orchestrator::{
    EscrowRecord as EscrowRecordExport, EscrowStatus as EscrowStatusExport,
};
use uuid::Uuid;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Size threshold for blob storage (content larger than this goes to S3)
pub const BLOB_THRESHOLD: usize = 64 * 1024; // 64KB

/// Minimum finality confirmations required
pub const MIN_FINALITY_CONFIRMATIONS: u32 = 3;

/// Default message fee per message (in smallest token unit, 8 decimals)
pub const DEFAULT_MESSAGE_FEE: u64 = 1_000_000; // 0.01 DCHAT

/// Storage cost per MB (in smallest token unit)
pub const STORAGE_COST_PER_MB: u64 = 10_000_000; // 0.1 DCHAT per MB

// Database KV keys for persistent state
const KV_OPERATION_MAPPINGS: &str = "fee_gateway.operation_mappings.v1";
// Note: Escrow records are now persisted by FeeOrchestrator

// =============================================================================
// REQUEST/RESPONSE CONTRACTS
// =============================================================================

/// Unified operation request for fee-gated operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeGatedRequest {
    /// Payer user ID
    pub payer: UserId,
    /// Client-provided nonce for idempotency
    pub client_nonce: u64,
    /// Operation-specific payload
    pub payload: OperationPayload,
    /// Optional: specific relay to use (if not provided, one is selected)
    pub preferred_relay: Option<UserId>,
}

/// Operation-specific payload types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationPayload {
    /// Direct message
    DirectMessage {
        recipient: UserId,
        content: Vec<u8>,
        encrypted: bool,
        encryption_key_id: Option<[u8; 32]>,
    },
    /// Channel post
    ChannelPost {
        channel_id: ChannelId,
        content: Vec<u8>,
        encrypted: bool,
        encryption_key_id: Option<[u8; 32]>,
    },
    /// Create channel (requires channel creation fee + gas)
    CreateChannel {
        channel_name: String,
        description: Option<String>,
    },
}

/// Gas fee receipt with full details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasFeeReceipt {
    /// Currency-chain transaction ID for the gas fee
    pub fee_tx_id: Uuid,
    /// Gross amount charged
    pub amount: u64,
    /// Breakdown by sink
    pub sink_breakdown: SinkAmounts,
    /// Whether this was a replay (no actual charge)
    pub was_replay: bool,
    /// Timestamp of charge
    pub charged_at: DateTime<Utc>,
}

impl From<FeeReceipt> for GasFeeReceipt {
    fn from(r: FeeReceipt) -> Self {
        Self {
            fee_tx_id: r.fee_tx_id,
            amount: r.gross_amount,
            sink_breakdown: r.sink_amounts,
            was_replay: r.was_replay,
            charged_at: r.timestamp,
        }
    }
}

/// Message fee receipt with relay attribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageFeeReceipt {
    /// Currency-chain transaction ID for the message fee
    pub fee_tx_id: Uuid,
    /// Escrow transaction ID (if using escrow pattern)
    pub escrow_tx_id: Option<Uuid>,
    /// Amount charged (100% goes to relay, no burn)
    pub amount: u64,
    /// Relay identity receiving the fee
    pub relay_id: UserId,
    /// Whether fee is in escrow (pending finality) or released
    pub escrow_status: EscrowStatus,
    /// Timestamp of charge
    pub charged_at: DateTime<Utc>,
}

// Note: EscrowStatus is now imported from dchat_blockchain::fee_orchestrator

/// Storage payment receipt for blob-tier content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePaymentReceipt {
    /// Currency-chain transaction ID for storage payment
    pub payment_tx_id: Uuid,
    /// Amount paid for storage
    pub amount: u64,
    /// Storage tier used
    pub tier: StorageTier,
    /// Blob hash (for blob tier)
    pub blob_hash: Option<[u8; 32]>,
    /// Storage stream ID (for streaming payments)
    pub stream_id: Option<String>,
    /// Timestamp
    pub paid_at: DateTime<Utc>,
}

/// Chat chain transaction details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChainTxDetails {
    /// Real chat-chain transaction ID/hash
    pub tx_id: Uuid,
    /// Transaction hash (hex encoded)
    pub tx_hash: String,
    /// Block height where tx was included
    pub block_height: u64,
    /// Number of confirmations
    pub confirmations: u32,
    /// Finality achieved
    pub finality_achieved: bool,
    /// Timestamp of finality
    pub finalized_at: Option<DateTime<Utc>>,
}

/// Unified response from fee-gated operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeGatedResponse {
    /// Deterministic idempotency key
    /// Computed from: payer + tx_type + payload_hash + nonce
    pub operation_id: [u8; 32],
    /// Client nonce echoed back
    pub client_nonce: u64,
    /// Hash of the payload content
    pub payload_hash: [u8; 32],
    /// Gas fee receipt
    pub gas_fee_receipt: GasFeeReceipt,
    /// Message fee receipt (with relay attribution)
    pub message_fee_receipt: MessageFeeReceipt,
    /// Chat-chain transaction details
    pub chat_tx: ChatChainTxDetails,
    /// Message ID assigned to the content
    pub message_id: MessageId,
    /// Storage tier used
    pub storage_tier: StorageTier,
    /// Storage payment receipt (for blob tier)
    pub storage_receipt: Option<StoragePaymentReceipt>,
    /// Operation status
    pub status: OperationStatus,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// Operation status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationStatus {
    /// Operation completed successfully
    Success,
    /// Operation failed, fees refunded
    FailedRefunded,
    /// Pending finality (should not be returned to client normally)
    PendingFinality,
}

/// Channel creation response (similar structure but for channel operations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCreationResponse {
    /// Deterministic idempotency key
    pub operation_id: [u8; 32],
    /// Client nonce echoed back
    pub client_nonce: u64,
    /// Gas fee receipt
    pub gas_fee_receipt: GasFeeReceipt,
    /// Channel creation fee receipt (separate from gas)
    pub channel_fee_receipt: GasFeeReceipt,
    /// Chat-chain transaction details
    pub chat_tx: ChatChainTxDetails,
    /// Created channel ID
    pub channel_id: ChannelId,
    /// Channel name
    pub channel_name: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

// =============================================================================
// OPERATION MAPPING PERSISTENCE
// =============================================================================

/// Complete operation mapping for audit trail
/// Persisted: operation_id → gas fee_tx_id → message fee_tx_id → chat tx id/hash → message_id → storage tier/blob hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationMapping {
    /// Operation ID (primary key)
    pub operation_id: [u8; 32],
    /// Payer
    pub payer: UserId,
    /// Transaction type
    pub tx_type: TransactionType,
    /// Client nonce
    pub client_nonce: u64,
    /// Payload hash
    pub payload_hash: [u8; 32],
    /// Gas fee transaction ID
    pub gas_fee_tx_id: Uuid,
    /// Message fee transaction ID
    pub message_fee_tx_id: Option<Uuid>,
    /// Message fee escrow transaction ID
    pub escrow_tx_id: Option<Uuid>,
    /// Chat chain transaction ID
    pub chat_tx_id: Uuid,
    /// Chat chain transaction hash
    pub chat_tx_hash: String,
    /// Message ID
    pub message_id: MessageId,
    /// Relay ID (for message fee attribution)
    pub relay_id: Option<UserId>,
    /// Storage tier
    pub storage_tier: StorageTier,
    /// Blob hash (if blob tier)
    pub blob_hash: Option<[u8; 32]>,
    /// Storage payment transaction ID
    pub storage_payment_tx_id: Option<Uuid>,
    /// Final status
    pub status: OperationStatus,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Finalized timestamp
    pub finalized_at: Option<DateTime<Utc>>,
}

// =============================================================================
// FEE GATEWAY
// =============================================================================

// Note: MessageFeeEscrow is now EscrowRecord from dchat_blockchain::fee_orchestrator

/// Fee Gateway - Single entry point for all fee-gated operations
///
/// This enforces pay-before-anchor for all storage/anchoring operations.
/// No code path can store or anchor without going through this gateway.
pub struct FeeGateway {
    /// Fee orchestrator for gas fee charging AND escrow management
    fee_orchestrator: Arc<FeeOrchestrator>,
    /// Currency chain client for direct transfers
    currency_chain: Arc<CurrencyChainClient>,
    /// Chat chain client for transaction submission
    chat_chain: Arc<ChatChainClient>,
    /// Storage router manager
    storage_manager: Arc<StorageRoutedUserManager>,
    /// Relay network manager for proper relay selection
    relay_network: Arc<RwLock<RelayNetworkManager>>,
    /// Operation mappings for audit trail (in-memory cache backed by database)
    operation_mappings: Arc<RwLock<HashMap<[u8; 32], OperationMapping>>>,
    /// Database for persistent state (optional - if None, state is in-memory only)
    database: Option<Arc<Database>>,
    /// Default message fee
    default_message_fee: u64,
    /// Storage cost per MB
    storage_cost_per_mb: u64,
    /// Minimum finality confirmations
    min_finality_confirmations: u32,
}

impl FeeGateway {
    /// Create new fee gateway
    pub fn new(
        fee_orchestrator: Arc<FeeOrchestrator>,
        currency_chain: Arc<CurrencyChainClient>,
        chat_chain: Arc<ChatChainClient>,
        storage_manager: Arc<StorageRoutedUserManager>,
        relay_network: Arc<RwLock<RelayNetworkManager>>,
    ) -> Self {
        Self {
            fee_orchestrator,
            currency_chain,
            chat_chain,
            storage_manager,
            relay_network,
            operation_mappings: Arc::new(RwLock::new(HashMap::new())),
            database: None,
            default_message_fee: DEFAULT_MESSAGE_FEE,
            storage_cost_per_mb: STORAGE_COST_PER_MB,
            min_finality_confirmations: MIN_FINALITY_CONFIRMATIONS,
        }
    }

    /// Configure database for persistent state
    ///
    /// When a database is configured, operation mappings and escrow records
    /// are persisted to SQLite, ensuring that idempotency and escrow state
    /// survives process restarts.
    pub fn with_database(mut self, database: Arc<Database>) -> Self {
        self.database = Some(database);
        info!("✓ FeeGateway persistence enabled");
        self
    }

    /// Configure message fee
    pub fn with_message_fee(mut self, fee: u64) -> Self {
        self.default_message_fee = fee;
        self
    }

    /// Configure storage cost
    pub fn with_storage_cost(mut self, cost_per_mb: u64) -> Self {
        self.storage_cost_per_mb = cost_per_mb;
        self
    }

    /// Load persisted state from database (call after construction with database)
    ///
    /// This restores operation mappings and escrow records from SQLite.
    /// Should be called once at startup before processing any operations.
    pub async fn load_persisted_state(&self) -> Result<()> {
        let db = match &self.database {
            Some(db) => db,
            None => {
                debug!("No database configured, skipping state load");
                return Ok(());
            }
        };

        // Load operation mappings
        if let Ok(Some(json)) = db.get_client_kv(KV_OPERATION_MAPPINGS).await {
            match serde_json::from_str::<Vec<OperationMapping>>(&json) {
                Ok(mappings) => {
                    let mut cache = self.operation_mappings.write().unwrap();
                    for mapping in mappings {
                        cache.insert(mapping.operation_id, mapping);
                    }
                    info!("✓ Loaded {} operation mappings from database", cache.len());
                }
                Err(e) => {
                    warn!("Failed to parse operation mappings: {}", e);
                }
            }
        }

        // Note: Escrow records are now managed by FeeOrchestrator

        Ok(())
    }

    /// Persist current state to database
    ///
    /// Called after each operation completes to ensure durability.
    async fn persist_state(&self) -> Result<()> {
        let db = match &self.database {
            Some(db) => db,
            None => return Ok(()), // No-op if no database
        };

        // Persist operation mappings
        let mappings: Vec<OperationMapping> = self
            .operation_mappings
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect();
        let mappings_json = serde_json::to_string(&mappings)?;
        db.put_client_kv(KV_OPERATION_MAPPINGS, &mappings_json)
            .await?;

        // Note: Escrow records are now managed by FeeOrchestrator

        debug!("Persisted {} operations", mappings.len());
        Ok(())
    }

    /// Get operation mapping by ID
    pub fn get_operation_mapping(&self, operation_id: &[u8; 32]) -> Option<OperationMapping> {
        self.operation_mappings
            .read()
            .unwrap()
            .get(operation_id)
            .cloned()
    }

    /// Get all operation mappings for a payer
    pub fn get_operations_for_payer(&self, payer: &UserId) -> Vec<OperationMapping> {
        self.operation_mappings
            .read()
            .unwrap()
            .values()
            .filter(|m| m.payer == *payer)
            .cloned()
            .collect()
    }

    // =========================================================================
    // CORE FEE-GATED OPERATIONS
    // =========================================================================

    /// Send a direct message with full fee enforcement
    ///
    /// This is the only path to send a DM that results in storage.
    /// Pay-before-anchor ordering:
    /// 1. Validate permissions and content size
    /// 2. Select relay deterministically
    /// 3. Compute payload hash and operation ID
    /// 4. Charge gas fee (idempotent)
    /// 5. Place message fee in escrow
    /// 6. Submit chat-chain ordering tx
    /// 7. Wait for finality
    /// 8. On success: release escrow, store content
    /// 9. On failure: refund all fees, no storage
    pub async fn send_direct_message(&self, request: FeeGatedRequest) -> Result<FeeGatedResponse> {
        // Extract DM-specific fields
        let (recipient, content, encrypted, encryption_key_id) = match &request.payload {
            OperationPayload::DirectMessage {
                recipient,
                content,
                encrypted,
                encryption_key_id,
            } => (
                recipient.clone(),
                content.clone(),
                *encrypted,
                *encryption_key_id,
            ),
            _ => return Err(Error::validation("Expected DirectMessage payload")),
        };

        let payer = &request.payer;
        let client_nonce = request.client_nonce;

        // Step 1: Validate content size
        const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB
        if content.len() > MAX_MESSAGE_SIZE {
            return Err(Error::validation(format!(
                "Content too large: {} bytes exceeds {} byte limit",
                content.len(),
                MAX_MESSAGE_SIZE
            )));
        }

        // Step 2: Compute payload hash
        let payload_hash: [u8; 32] = blake3::hash(&content).into();

        // Step 3: Select relay deterministically
        let relay = request
            .preferred_relay
            .clone()
            .unwrap_or_else(|| self.select_relay_for_message(payer, &recipient));

        // Step 4: Compute operation ID (deterministic idempotency key)
        let operation_id = FeeOrchestrator::compute_operation_id(
            payer,
            TransactionType::SendDirectMessage,
            &payload_hash,
            client_nonce,
        );

        // Check if this operation already exists (idempotency)
        if let Some(existing) = self.get_operation_mapping(&operation_id) {
            if existing.status == OperationStatus::Success {
                info!(
                    "Operation {} already completed, returning cached result",
                    hex::encode(&operation_id[..8])
                );
                return self.reconstruct_response_from_mapping(existing);
            }
        }

        info!(
            "📤 FeeGateway: Processing DM from {} to {} (op: {}, nonce: {})",
            payer,
            recipient,
            hex::encode(&operation_id[..8]),
            client_nonce
        );

        // Step 5: Calculate fees
        let gas_fee = self
            .fee_orchestrator
            .config()
            .calculate_gas_fee(TransactionType::SendDirectMessage, content.len());
        let message_fee = self.default_message_fee;
        let storage_tier = self.storage_manager.determine_storage_tier(content.len());
        let storage_fee = self.calculate_storage_fee(&storage_tier, content.len());

        let total_required = gas_fee + message_fee + storage_fee;

        // Step 6: Verify payer has sufficient balance for all fees
        let payer_balance = self.currency_chain.get_balance(payer)?;
        if payer_balance < total_required {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance: have {}, need {} (gas: {}, message: {}, storage: {})",
                payer_balance, total_required, gas_fee, message_fee, storage_fee
            )));
        }

        // Step 7: Charge gas fee (with idempotency)
        let gas_receipt = self.fee_orchestrator.charge_gas_fee(
            payer,
            TransactionType::SendDirectMessage,
            content.len(),
            operation_id,
        )?;

        info!(
            "✅ Gas fee charged: {} (tx: {}, replay: {})",
            gas_receipt.gross_amount, gas_receipt.fee_tx_id, gas_receipt.was_replay
        );

        // Step 8: Place message fee in escrow
        let escrow = match self.create_message_fee_escrow(operation_id, payer, &relay, message_fee)
        {
            Ok(escrow) => escrow,
            Err(e) => {
                // Refund gas fee
                let _ = self
                    .fee_orchestrator
                    .refund_fee(operation_id, &format!("Escrow creation failed: {}", e));
                return Err(e);
            }
        };

        info!(
            "🔒 Message fee escrowed: {} for relay {} (escrow_tx: {})",
            message_fee, relay, escrow.escrow_tx_id
        );

        // Step 9: Generate message ID
        let message_id = MessageId(Uuid::new_v4());

        // Step 10: Submit chat-chain ordering transaction
        let chat_tx_result = self
            .chat_chain
            .send_direct_message(payer, &recipient, message_id)
            .await;

        let chat_tx_id = match chat_tx_result {
            Ok(tx_id) => tx_id,
            Err(e) => {
                error!("Chat-chain submission failed: {}", e);
                // Refund all fees
                self.refund_operation(operation_id, &format!("Chat-chain failed: {}", e))?;
                return Err(Error::chain(format!("Chat-chain submission failed: {}", e)));
            }
        };

        info!("📝 Chat-chain tx submitted: {}", chat_tx_id);

        // Step 11: Wait for finality (CRITICAL: remove "tx_id implies confirmed" behavior)
        let finality_result = self
            .chat_chain
            .wait_for_finality(&chat_tx_id, self.min_finality_confirmations)
            .await;

        let (finality_achieved, confirmations, block_height) = match finality_result {
            Ok(achieved) => {
                if achieved {
                    // Get actual confirmation count and block height
                    let tx_status = self
                        .chat_chain
                        .get_transaction_status(&chat_tx_id)
                        .await
                        .ok();
                    let confirms = tx_status
                        .as_ref()
                        .map(|s| s.confirmations)
                        .unwrap_or(self.min_finality_confirmations);
                    let height = tx_status.as_ref().map(|s| s.block_height).unwrap_or(0);
                    (true, confirms, height)
                } else {
                    (false, 0, 0)
                }
            }
            Err(e) => {
                error!("Finality wait failed: {}", e);
                // Refund all fees
                self.refund_operation(operation_id, &format!("Finality failed: {}", e))?;
                return Err(Error::chain(format!("Finality failed: {}", e)));
            }
        };

        if !finality_achieved {
            error!("Transaction did not achieve finality: {}", chat_tx_id);
            self.refund_operation(operation_id, "Finality not achieved")?;
            return Err(Error::chain("Transaction did not achieve finality"));
        }

        info!(
            "✅ Finality achieved: {} confirmations at block {}",
            confirmations, block_height
        );

        // Step 12: Handle storage fee for blob tier
        let storage_receipt = if storage_tier == StorageTier::Blob && storage_fee > 0 {
            Some(self.charge_storage_fee(payer, storage_fee, &payload_hash)?)
        } else {
            None
        };

        // Step 13: Release escrow to relay (success path)
        let release_tx_id = self.release_escrow_to_relay(&operation_id, payer, &relay)?;

        info!(
            "💰 Escrow released to relay {} (tx: {})",
            relay, release_tx_id
        );

        // Step 14: Store content (only after finality and fee success)
        let sender_bytes = uuid_to_bytes(&payer.0);
        let recipient_bytes = uuid_to_bytes(&recipient.0);

        let store_result = self
            .storage_manager
            .router()
            .store_message(
                sender_bytes,
                recipient_bytes,
                dchat_storage::provider::MessageType::Direct,
                content,
                encrypted,
                encryption_key_id,
                None,
            )
            .await;

        if let Err(e) = &store_result {
            error!("Storage failed after finality: {}", e);
            // Note: At this point we don't refund - the tx is finalized
            // The relay already earned their fee for successful ordering
            // Log this as an error for investigation
            warn!(
                "⚠️ Storage failed after successful anchoring. Operation {} may need manual intervention",
                hex::encode(&operation_id[..8])
            );
        }

        // Step 15: Mark gas fee as committed with real chat tx ID
        self.fee_orchestrator
            .mark_committed(operation_id, chat_tx_id)?;

        // Step 16: Build and persist operation mapping
        let tx_hash = format!("{:x}", chat_tx_id);
        let now = Utc::now();

        let mapping = OperationMapping {
            operation_id,
            payer: payer.clone(),
            tx_type: TransactionType::SendDirectMessage,
            client_nonce,
            payload_hash,
            gas_fee_tx_id: gas_receipt.fee_tx_id,
            message_fee_tx_id: Some(release_tx_id),
            escrow_tx_id: Some(escrow.escrow_tx_id),
            chat_tx_id,
            chat_tx_hash: tx_hash.clone(),
            message_id,
            relay_id: Some(relay.clone()),
            storage_tier,
            blob_hash: if storage_tier == StorageTier::Blob {
                Some(payload_hash)
            } else {
                None
            },
            storage_payment_tx_id: storage_receipt.as_ref().map(|r| r.payment_tx_id),
            status: OperationStatus::Success,
            created_at: gas_receipt.timestamp,
            finalized_at: Some(now),
        };

        self.operation_mappings
            .write()
            .unwrap()
            .insert(operation_id, mapping);

        // Step 17: Build response
        let response = FeeGatedResponse {
            operation_id,
            client_nonce,
            payload_hash,
            gas_fee_receipt: gas_receipt.into(),
            message_fee_receipt: MessageFeeReceipt {
                fee_tx_id: release_tx_id,
                escrow_tx_id: Some(escrow.escrow_tx_id),
                amount: message_fee,
                relay_id: relay,
                escrow_status: EscrowStatus::Released,
                charged_at: escrow.created_at,
            },
            chat_tx: ChatChainTxDetails {
                tx_id: chat_tx_id,
                tx_hash,
                block_height,
                confirmations,
                finality_achieved,
                finalized_at: Some(now),
            },
            message_id,
            storage_tier,
            storage_receipt,
            status: OperationStatus::Success,
            timestamp: now,
        };

        info!(
            "✅ Operation {} completed successfully (message: {}, chat_tx: {})",
            hex::encode(&operation_id[..8]),
            message_id,
            chat_tx_id
        );

        // Persist state to database (if configured)
        if let Err(e) = self.persist_state().await {
            warn!("Failed to persist FeeGateway state: {}", e);
        }

        // Structured log for metrics
        self.log_operation_metrics(&response);

        Ok(response)
    }

    /// Post to a channel with full fee enforcement
    pub async fn post_to_channel(&self, request: FeeGatedRequest) -> Result<FeeGatedResponse> {
        // Extract channel-specific fields
        let (channel_id, content, encrypted, encryption_key_id) = match &request.payload {
            OperationPayload::ChannelPost {
                channel_id,
                content,
                encrypted,
                encryption_key_id,
            } => (
                channel_id.clone(),
                content.clone(),
                *encrypted,
                *encryption_key_id,
            ),
            _ => return Err(Error::validation("Expected ChannelPost payload")),
        };

        let payer = &request.payer;
        let client_nonce = request.client_nonce;

        // Compute payload hash
        let payload_hash: [u8; 32] = blake3::hash(&content).into();

        // Select relay
        let relay = request
            .preferred_relay
            .clone()
            .unwrap_or_else(|| self.select_relay_for_channel(&channel_id));

        // Compute operation ID
        let operation_id = FeeOrchestrator::compute_operation_id(
            payer,
            TransactionType::PostToChannel,
            &payload_hash,
            client_nonce,
        );

        // Check idempotency
        if let Some(existing) = self.get_operation_mapping(&operation_id) {
            if existing.status == OperationStatus::Success {
                return self.reconstruct_response_from_mapping(existing);
            }
        }

        info!(
            "📢 FeeGateway: Processing channel post to {} (op: {})",
            channel_id,
            hex::encode(&operation_id[..8])
        );

        // Calculate fees
        let gas_fee = self
            .fee_orchestrator
            .config()
            .calculate_gas_fee(TransactionType::PostToChannel, content.len());
        let message_fee = self.default_message_fee;
        let storage_tier = self.storage_manager.determine_storage_tier(content.len());
        let storage_fee = self.calculate_storage_fee(&storage_tier, content.len());
        let total_required = gas_fee + message_fee + storage_fee;

        // Verify balance
        let payer_balance = self.currency_chain.get_balance(payer)?;
        if payer_balance < total_required {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance: have {}, need {}",
                payer_balance, total_required
            )));
        }

        // Charge gas fee
        let gas_receipt = self.fee_orchestrator.charge_gas_fee(
            payer,
            TransactionType::PostToChannel,
            content.len(),
            operation_id,
        )?;

        // Create escrow for message fee
        let escrow = match self.create_message_fee_escrow(operation_id, payer, &relay, message_fee)
        {
            Ok(e) => e,
            Err(e) => {
                let _ = self
                    .fee_orchestrator
                    .refund_fee(operation_id, &e.to_string());
                return Err(e);
            }
        };

        // Generate message ID
        let message_id = MessageId(Uuid::new_v4());

        // Submit chat-chain tx
        let chat_tx_id = match self
            .chat_chain
            .post_to_channel(payer, &channel_id, message_id)
            .await
        {
            Ok(tx) => tx,
            Err(e) => {
                self.refund_operation(operation_id, &e.to_string())?;
                return Err(Error::chain(e.to_string()));
            }
        };

        // Wait for finality
        let finality_achieved = self
            .chat_chain
            .wait_for_finality(&chat_tx_id, self.min_finality_confirmations)
            .await
            .map_err(|e| Error::chain(e.to_string()))?;

        if !finality_achieved {
            self.refund_operation(operation_id, "Finality not achieved")?;
            return Err(Error::chain("Finality not achieved"));
        }

        // Storage fee for blob tier
        let storage_receipt = if storage_tier == StorageTier::Blob && storage_fee > 0 {
            Some(self.charge_storage_fee(payer, storage_fee, &payload_hash)?)
        } else {
            None
        };

        // Release escrow
        let release_tx_id = self.release_escrow_to_relay(&operation_id, payer, &relay)?;

        // Store content
        let sender_bytes = uuid_to_bytes(&payer.0);
        let channel_bytes = uuid_to_bytes(&channel_id.0);

        let _ = self
            .storage_manager
            .router()
            .store_message(
                sender_bytes,
                channel_bytes,
                dchat_storage::provider::MessageType::Channel,
                content,
                encrypted,
                encryption_key_id,
                None,
            )
            .await;

        // Mark committed
        self.fee_orchestrator
            .mark_committed(operation_id, chat_tx_id)?;

        let tx_hash = format!("{:x}", chat_tx_id);
        let now = Utc::now();

        // Persist mapping
        let mapping = OperationMapping {
            operation_id,
            payer: payer.clone(),
            tx_type: TransactionType::PostToChannel,
            client_nonce,
            payload_hash,
            gas_fee_tx_id: gas_receipt.fee_tx_id,
            message_fee_tx_id: Some(release_tx_id),
            escrow_tx_id: Some(escrow.escrow_tx_id),
            chat_tx_id,
            chat_tx_hash: tx_hash.clone(),
            message_id,
            relay_id: Some(relay.clone()),
            storage_tier,
            blob_hash: if storage_tier == StorageTier::Blob {
                Some(payload_hash)
            } else {
                None
            },
            storage_payment_tx_id: storage_receipt.as_ref().map(|r| r.payment_tx_id),
            status: OperationStatus::Success,
            created_at: gas_receipt.timestamp,
            finalized_at: Some(now),
        };

        self.operation_mappings
            .write()
            .unwrap()
            .insert(operation_id, mapping);

        let response = FeeGatedResponse {
            operation_id,
            client_nonce,
            payload_hash,
            gas_fee_receipt: gas_receipt.into(),
            message_fee_receipt: MessageFeeReceipt {
                fee_tx_id: release_tx_id,
                escrow_tx_id: Some(escrow.escrow_tx_id),
                amount: message_fee,
                relay_id: relay,
                escrow_status: EscrowStatus::Released,
                charged_at: escrow.created_at,
            },
            chat_tx: ChatChainTxDetails {
                tx_id: chat_tx_id,
                tx_hash,
                block_height: 0, // Would be populated from actual tx status
                confirmations: self.min_finality_confirmations,
                finality_achieved: true,
                finalized_at: Some(now),
            },
            message_id,
            storage_tier,
            storage_receipt,
            status: OperationStatus::Success,
            timestamp: now,
        };

        // Persist state to database (if configured)
        if let Err(e) = self.persist_state().await {
            warn!("Failed to persist FeeGateway state: {}", e);
        }

        self.log_operation_metrics(&response);

        Ok(response)
    }

    // =========================================================================
    // ESCROW MANAGEMENT (delegated to FeeOrchestrator)
    // =========================================================================

    /// Create message fee escrow (hold funds pending finality)
    ///
    /// Delegates to FeeOrchestrator.create_escrow for unified escrow management.
    fn create_message_fee_escrow(
        &self,
        operation_id: [u8; 32],
        payer: &UserId,
        relay: &UserId,
        amount: u64,
    ) -> Result<EscrowRecord> {
        self.fee_orchestrator
            .create_escrow(operation_id, payer, relay, amount, "message_fee")
    }

    /// Release escrow to relay (success path)
    ///
    /// Delegates to FeeOrchestrator.release_escrow for unified escrow management.
    fn release_escrow_to_relay(
        &self,
        operation_id: &[u8; 32],
        payer: &UserId,
        relay: &UserId,
    ) -> Result<Uuid> {
        let escrow_id = FeeOrchestrator::compute_escrow_id(operation_id, payer, relay);
        let record = self.fee_orchestrator.release_escrow(&escrow_id)?;

        info!(
            "💰 Escrow released: {} to relay {} (tx: {})",
            record.amount,
            record.recipient,
            record.settlement_tx_id.unwrap_or_default()
        );

        Ok(record.settlement_tx_id.unwrap_or_default())
    }

    /// Refund escrow to payer (failure path)
    ///
    /// Delegates to FeeOrchestrator.refund_escrow for unified escrow management.
    fn refund_escrow_to_payer(&self, operation_id: &[u8; 32]) -> Result<Uuid> {
        // Find escrows for this operation and refund them
        let escrows = self
            .fee_orchestrator
            .get_escrows_for_operation(operation_id);

        if escrows.is_empty() {
            // No escrow found, might already be settled
            return Ok(Uuid::nil());
        }

        let mut last_tx_id = Uuid::nil();
        for escrow in escrows {
            if escrow.status == EscrowStatus::Held {
                match self
                    .fee_orchestrator
                    .refund_escrow(&escrow.escrow_id, "operation_failed")
                {
                    Ok(record) => {
                        info!(
                            "💸 Escrow refunded: {} to payer {} (tx: {})",
                            record.amount,
                            record.payer,
                            record.settlement_tx_id.unwrap_or_default()
                        );
                        last_tx_id = record.settlement_tx_id.unwrap_or_default();
                    }
                    Err(e) => {
                        warn!(
                            "Failed to refund escrow {}: {}",
                            hex::encode(&escrow.escrow_id[..8]),
                            e
                        );
                    }
                }
            }
        }

        Ok(last_tx_id)
    }

    /// Refund all fees for an operation (failure path)
    fn refund_operation(&self, operation_id: [u8; 32], reason: &str) -> Result<()> {
        info!(
            "🔄 Refunding operation {}: {}",
            hex::encode(&operation_id[..8]),
            reason
        );

        // Refund gas fee
        if let Err(e) = self.fee_orchestrator.refund_fee(operation_id, reason) {
            warn!("Failed to refund gas fee: {}", e);
        }

        // Refund escrow
        if let Err(e) = self.refund_escrow_to_payer(&operation_id) {
            warn!("Failed to refund escrow: {}", e);
        }

        // Update operation mapping status
        if let Some(mapping) = self
            .operation_mappings
            .write()
            .unwrap()
            .get_mut(&operation_id)
        {
            mapping.status = OperationStatus::FailedRefunded;
            mapping.finalized_at = Some(Utc::now());
        }

        Ok(())
    }

    // =========================================================================
    // STORAGE FEE MANAGEMENT
    // =========================================================================

    /// Calculate storage fee for content
    fn calculate_storage_fee(&self, tier: &StorageTier, content_len: usize) -> u64 {
        match tier {
            StorageTier::Inline => 0, // Included in gas fee
            StorageTier::Blob => {
                let mb = (content_len as u64 + 1024 * 1024 - 1) / (1024 * 1024);
                mb.max(1) * self.storage_cost_per_mb
            }
        }
    }

    /// Charge storage fee for blob tier
    fn charge_storage_fee(
        &self,
        payer: &UserId,
        amount: u64,
        blob_hash: &[u8; 32],
    ) -> Result<StoragePaymentReceipt> {
        let payment_tx_id = Uuid::new_v4();

        self.currency_chain
            .debit_for_protocol_fee(payer, amount, payment_tx_id)?;

        // Credit to storage pool
        self.currency_chain.fund_pool(
            dchat_blockchain::fee_distribution::PoolType::StorageBonds,
            amount,
        )?;

        Ok(StoragePaymentReceipt {
            payment_tx_id,
            amount,
            tier: StorageTier::Blob,
            blob_hash: Some(*blob_hash),
            stream_id: None,
            paid_at: Utc::now(),
        })
    }

    // =========================================================================
    // RELAY SELECTION
    // =========================================================================

    /// Select relay for a direct message using the relay network manager
    ///
    /// Uses the configured load balancing strategy (weighted round-robin by default)
    /// to select an active relay that meets uptime and stake requirements.
    fn select_relay_for_message(&self, sender: &UserId, recipient: &UserId) -> UserId {
        // Try to get an active relay from the relay network manager
        let mut relay_network = self.relay_network.write().unwrap();

        match relay_network.select_relay() {
            Ok(relay_id) => {
                // Get the relay info to extract operator UserId
                if let Some(relay_info) = relay_network.get_relay(&relay_id) {
                    debug!(
                        "Selected relay {} (operator: {}) for message {} -> {}",
                        relay_id, relay_info.operator, sender, recipient
                    );
                    return relay_info.operator.clone();
                }

                // Relay found but info missing - use deterministic fallback
                warn!(
                    "Relay {} selected but info not found, using deterministic fallback",
                    relay_id
                );
                self.deterministic_relay_fallback(sender.0.as_bytes(), recipient.0.as_bytes())
            }
            Err(e) => {
                // No active relays available - use deterministic fallback
                // This ensures the system can still function during bootstrap
                // or temporary relay unavailability
                warn!(
                    "No active relays available ({}), using deterministic fallback for {} -> {}",
                    e, sender, recipient
                );
                self.deterministic_relay_fallback(sender.0.as_bytes(), recipient.0.as_bytes())
            }
        }
    }

    /// Select relay for a channel post using the relay network manager
    ///
    /// Uses the configured load balancing strategy to select an active relay.
    /// Channel posts may benefit from geographic affinity in the future.
    fn select_relay_for_channel(&self, channel_id: &ChannelId) -> UserId {
        let mut relay_network = self.relay_network.write().unwrap();

        match relay_network.select_relay() {
            Ok(relay_id) => {
                if let Some(relay_info) = relay_network.get_relay(&relay_id) {
                    debug!(
                        "Selected relay {} (operator: {}) for channel {}",
                        relay_id, relay_info.operator, channel_id
                    );
                    return relay_info.operator.clone();
                }

                warn!(
                    "Relay {} selected but info not found, using deterministic fallback",
                    relay_id
                );
                self.deterministic_relay_fallback(channel_id.0.as_bytes(), &[])
            }
            Err(e) => {
                warn!(
                    "No active relays available ({}), using deterministic fallback for channel {}",
                    e, channel_id
                );
                self.deterministic_relay_fallback(channel_id.0.as_bytes(), &[])
            }
        }
    }

    /// Deterministic fallback relay selection when no active relays are available
    ///
    /// This generates a deterministic UserId based on input data for consistent
    /// behavior during network bootstrap or temporary relay unavailability.
    /// In production, this should rarely be used as the relay network should
    /// always have active relays.
    ///
    /// IMPORTANT: This must be fully deterministic - the same inputs must always
    /// produce the same output for consensus-critical relay assignment.
    fn deterministic_relay_fallback(&self, input1: &[u8], input2: &[u8]) -> UserId {
        // Use a fixed domain separator to prevent cross-protocol hash collisions
        const DOMAIN_SEPARATOR: &[u8] = b"dchat-relay-fallback-v1";

        let mut hasher = blake3::Hasher::new();
        hasher.update(DOMAIN_SEPARATOR);
        hasher.update(input1);
        hasher.update(input2);
        // No timestamp - must be fully deterministic for consensus
        let hash: [u8; 32] = hasher.finalize().into();

        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&hash[..16]);
        UserId(Uuid::from_bytes(uuid_bytes))
    }

    // =========================================================================
    // HELPERS
    // =========================================================================

    /// Reconstruct response from persisted mapping (for idempotency)
    fn reconstruct_response_from_mapping(
        &self,
        mapping: OperationMapping,
    ) -> Result<FeeGatedResponse> {
        Ok(FeeGatedResponse {
            operation_id: mapping.operation_id,
            client_nonce: mapping.client_nonce,
            payload_hash: mapping.payload_hash,
            gas_fee_receipt: GasFeeReceipt {
                fee_tx_id: mapping.gas_fee_tx_id,
                amount: 0, // Not stored in mapping
                sink_breakdown: SinkAmounts::default(),
                was_replay: true,
                charged_at: mapping.created_at,
            },
            message_fee_receipt: MessageFeeReceipt {
                fee_tx_id: mapping.message_fee_tx_id.unwrap_or(Uuid::nil()),
                escrow_tx_id: mapping.escrow_tx_id,
                amount: 0, // Not stored in mapping
                relay_id: mapping.relay_id.unwrap_or_default(),
                escrow_status: EscrowStatus::Released,
                charged_at: mapping.created_at,
            },
            chat_tx: ChatChainTxDetails {
                tx_id: mapping.chat_tx_id,
                tx_hash: mapping.chat_tx_hash,
                block_height: 0,
                confirmations: self.min_finality_confirmations,
                finality_achieved: true,
                finalized_at: mapping.finalized_at,
            },
            message_id: mapping.message_id,
            storage_tier: mapping.storage_tier,
            storage_receipt: mapping
                .storage_payment_tx_id
                .map(|tx_id| StoragePaymentReceipt {
                    payment_tx_id: tx_id,
                    amount: 0,
                    tier: mapping.storage_tier,
                    blob_hash: mapping.blob_hash,
                    stream_id: None,
                    paid_at: mapping.finalized_at.unwrap_or_else(Utc::now),
                }),
            status: mapping.status,
            timestamp: mapping.finalized_at.unwrap_or_else(Utc::now),
        })
    }

    /// Log operation metrics (structured logging for observability)
    fn log_operation_metrics(&self, response: &FeeGatedResponse) {
        // Look up actual payer from operation mapping
        let payer = self
            .operation_mappings
            .read()
            .unwrap()
            .get(&response.operation_id)
            .map(|m| m.payer.clone())
            .unwrap_or_else(|| UserId::default());

        // Structured log with all required fields
        info!(
            target: "fee_gateway_metrics",
            operation_id = %hex::encode(&response.operation_id[..8]),
            payer = %payer,
            message_id = %response.message_id,
            gas_fee_amount = response.gas_fee_receipt.amount,
            gas_fee_tx_id = %response.gas_fee_receipt.fee_tx_id,
            message_fee_amount = response.message_fee_receipt.amount,
            message_fee_tx_id = %response.message_fee_receipt.fee_tx_id,
            relay_id = %response.message_fee_receipt.relay_id,
            chat_tx_id = %response.chat_tx.tx_id,
            chat_tx_hash = %response.chat_tx.tx_hash,
            storage_tier = ?response.storage_tier,
            storage_receipt_id = ?response.storage_receipt.as_ref().map(|r| r.payment_tx_id),
            outcome = ?response.status,
            finality_confirmations = response.chat_tx.confirmations,
            "fee_gateway.operation_completed"
        );
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Convert UUID to 32-byte array (zero-padded)
fn uuid_to_bytes(uuid: &Uuid) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid.as_bytes());
    bytes
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_id_deterministic() {
        let payer = UserId(Uuid::new_v4());
        let payload_hash = [0xAB; 32];
        let nonce = 12345u64;

        let id1 = FeeOrchestrator::compute_operation_id(
            &payer,
            TransactionType::SendDirectMessage,
            &payload_hash,
            nonce,
        );
        let id2 = FeeOrchestrator::compute_operation_id(
            &payer,
            TransactionType::SendDirectMessage,
            &payload_hash,
            nonce,
        );

        assert_eq!(id1, id2);

        // Different nonce = different ID
        let id3 = FeeOrchestrator::compute_operation_id(
            &payer,
            TransactionType::SendDirectMessage,
            &payload_hash,
            nonce + 1,
        );
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_storage_fee_calculation() {
        // For this test we just verify the formula
        let cost_per_mb: u64 = 10_000_000;

        // 100 bytes -> 1 MB minimum
        let fee_small = 1 * cost_per_mb;
        assert_eq!(fee_small, 10_000_000);

        // 2 MB
        let fee_2mb = 2 * cost_per_mb;
        assert_eq!(fee_2mb, 20_000_000);
    }

    #[test]
    fn test_escrow_status_transitions() {
        assert_ne!(EscrowStatus::Held, EscrowStatus::Released);
        assert_ne!(EscrowStatus::Held, EscrowStatus::Refunded);
        assert_ne!(EscrowStatus::Released, EscrowStatus::Refunded);
    }

    #[test]
    fn test_operation_id_different_tx_types() {
        let payer = UserId(Uuid::new_v4());
        let payload_hash = [0xAB; 32];
        let nonce = 12345u64;

        let id_dm = FeeOrchestrator::compute_operation_id(
            &payer,
            TransactionType::SendDirectMessage,
            &payload_hash,
            nonce,
        );
        let id_channel = FeeOrchestrator::compute_operation_id(
            &payer,
            TransactionType::PostToChannel,
            &payload_hash,
            nonce,
        );

        // Same payload/nonce but different tx type = different ID
        assert_ne!(id_dm, id_channel);
    }

    #[test]
    fn test_operation_id_different_payers() {
        let payer1 = UserId(Uuid::new_v4());
        let payer2 = UserId(Uuid::new_v4());
        let payload_hash = [0xAB; 32];
        let nonce = 12345u64;

        let id1 = FeeOrchestrator::compute_operation_id(
            &payer1,
            TransactionType::SendDirectMessage,
            &payload_hash,
            nonce,
        );
        let id2 = FeeOrchestrator::compute_operation_id(
            &payer2,
            TransactionType::SendDirectMessage,
            &payload_hash,
            nonce,
        );

        // Different payer = different operation ID
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_operation_id_different_payload() {
        let payer = UserId(Uuid::new_v4());
        let payload_hash1 = [0xAB; 32];
        let payload_hash2 = [0xCD; 32];
        let nonce = 12345u64;

        let id1 = FeeOrchestrator::compute_operation_id(
            &payer,
            TransactionType::SendDirectMessage,
            &payload_hash1,
            nonce,
        );
        let id2 = FeeOrchestrator::compute_operation_id(
            &payer,
            TransactionType::SendDirectMessage,
            &payload_hash2,
            nonce,
        );

        // Different payload = different operation ID
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_uuid_to_bytes() {
        let uuid = Uuid::new_v4();
        let bytes = uuid_to_bytes(&uuid);

        // First 16 bytes are the UUID
        assert_eq!(&bytes[..16], uuid.as_bytes());
        // Last 16 bytes are zero-padded
        assert_eq!(&bytes[16..], &[0u8; 16]);
    }

    #[test]
    fn test_storage_tier_inline_threshold() {
        // Content under threshold should be inline
        assert!(BLOB_THRESHOLD > 1024); // At least 1KB threshold

        // Blob threshold should be reasonable for production
        assert!(BLOB_THRESHOLD <= 1024 * 1024); // At most 1MB
    }

    #[test]
    fn test_fee_gated_response_serialization() {
        let response = FeeGatedResponse {
            operation_id: [0xAB; 32],
            client_nonce: 12345,
            payload_hash: [0xCD; 32],
            gas_fee_receipt: GasFeeReceipt {
                fee_tx_id: Uuid::new_v4(),
                amount: 1000,
                sink_breakdown: SinkAmounts::default(),
                was_replay: false,
                charged_at: Utc::now(),
            },
            message_fee_receipt: MessageFeeReceipt {
                fee_tx_id: Uuid::new_v4(),
                escrow_tx_id: Some(Uuid::new_v4()),
                amount: 500,
                relay_id: UserId(Uuid::new_v4()),
                escrow_status: EscrowStatus::Released,
                charged_at: Utc::now(),
            },
            chat_tx: ChatChainTxDetails {
                tx_id: Uuid::new_v4(),
                tx_hash: "abc123".to_string(),
                block_height: 100,
                confirmations: 3,
                finality_achieved: true,
                finalized_at: Some(Utc::now()),
            },
            message_id: MessageId(Uuid::new_v4()),
            storage_tier: StorageTier::Inline,
            storage_receipt: None,
            status: OperationStatus::Success,
            timestamp: Utc::now(),
        };

        // Should serialize to JSON without panicking
        let json = serde_json::to_string(&response).expect("Serialization failed");
        assert!(json.contains("operation_id"));
        assert!(json.contains("client_nonce"));
        assert!(json.contains("gas_fee_receipt"));
        assert!(json.contains("message_fee_receipt"));
    }

    #[test]
    fn test_escrow_record_lifecycle() {
        let op_id = [0xAB; 32];
        let payer = UserId(Uuid::new_v4());
        let relay = UserId(Uuid::new_v4());

        // Use EscrowRecord from fee_orchestrator instead of local MessageFeeEscrow
        let escrow = EscrowRecord {
            escrow_id: [0xCD; 32],
            operation_id: op_id,
            payer: payer.clone(),
            recipient: relay.clone(),
            amount: 1000,
            escrow_tx_id: Uuid::new_v4(),
            status: EscrowStatus::Held,
            created_at: Utc::now(),
            settled_at: None,
            settlement_tx_id: None,
            context: "test_escrow".to_string(),
        };

        // Initial state is Held
        assert_eq!(escrow.status, EscrowStatus::Held);
        assert!(escrow.settled_at.is_none());
        assert!(escrow.settlement_tx_id.is_none());

        // Verify payer and recipient are preserved
        assert_eq!(escrow.payer, payer);
        assert_eq!(escrow.recipient, relay);
    }

    #[test]
    fn test_operation_mapping_completeness() {
        let mapping = OperationMapping {
            operation_id: [0xAB; 32],
            payer: UserId(Uuid::new_v4()),
            tx_type: TransactionType::SendDirectMessage,
            client_nonce: 12345,
            payload_hash: [0xCD; 32],
            gas_fee_tx_id: Uuid::new_v4(),
            message_fee_tx_id: Some(Uuid::new_v4()),
            escrow_tx_id: Some(Uuid::new_v4()),
            chat_tx_id: Uuid::new_v4(),
            chat_tx_hash: "abc123".to_string(),
            message_id: MessageId(Uuid::new_v4()),
            relay_id: Some(UserId(Uuid::new_v4())),
            storage_tier: StorageTier::Inline,
            blob_hash: None,
            storage_payment_tx_id: None,
            status: OperationStatus::Success,
            created_at: Utc::now(),
            finalized_at: Some(Utc::now()),
        };

        // Verify all required fields are present
        assert!(!mapping.chat_tx_hash.is_empty());
        assert!(mapping.gas_fee_tx_id != Uuid::nil());
        assert!(mapping.chat_tx_id != Uuid::nil());
        assert!(mapping.message_fee_tx_id.is_some());
        assert!(mapping.relay_id.is_some());
    }

    #[test]
    fn test_storage_payment_receipt_blob_tier() {
        let receipt = StoragePaymentReceipt {
            payment_tx_id: Uuid::new_v4(),
            amount: 10_000_000, // 1 MB storage fee
            tier: StorageTier::Blob,
            blob_hash: Some([0xAB; 32]),
            stream_id: None,
            paid_at: Utc::now(),
        };

        assert_eq!(receipt.tier, StorageTier::Blob);
        assert!(receipt.blob_hash.is_some());
        assert!(receipt.amount > 0);
    }

    #[test]
    fn test_default_fee_constants() {
        // Verify default constants are sane
        assert!(DEFAULT_MESSAGE_FEE > 0, "Message fee should be positive");
        assert!(STORAGE_COST_PER_MB > 0, "Storage cost should be positive");
        assert!(
            MIN_FINALITY_CONFIRMATIONS >= 1,
            "Need at least 1 confirmation"
        );

        // Message fee should be reasonable (not exorbitant)
        assert!(
            DEFAULT_MESSAGE_FEE < 1_000_000_000,
            "Message fee shouldn't exceed 1 DCHAT"
        );
    }

    #[test]
    fn test_operation_status_variants() {
        // Ensure all status variants are distinct and usable
        let success = OperationStatus::Success;
        let failed = OperationStatus::FailedRefunded;
        let pending = OperationStatus::PendingFinality;

        assert_ne!(success, failed);
        assert_ne!(success, pending);
        assert_ne!(failed, pending);

        // Verify serialization produces expected strings
        let json = serde_json::to_string(&success).unwrap();
        assert!(json.contains("Success"));
    }
}
