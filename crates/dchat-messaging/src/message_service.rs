//! Message sending service with fee collection
//!
//! This module implements production-grade message sending with proper fee deduction
//! from the currency chain before message routing. Supports both per-message fees
//! and pre-paid message credits via payment channels for high-frequency messaging.

use crate::delivery::DeliveryProof;
use crate::types::{Message, MessageStatus, MessageType};
use chrono::{DateTime, Utc};
use dchat_blockchain::CurrencyChainClient;
use ed25519_dalek::{SigningKey, Signer};
use sha2::{Digest, Sha256};
use dchat_core::error::{Error, Result};
use dchat_core::types::{MessageId, UserId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Message fee in smallest units (8 decimals)
/// 0.1 DCHAT = 1000_0000 units
pub const MESSAGE_FEE: u64 = 1000_0000;

/// Minimum balance to send a message (fee + small buffer)
pub const MIN_SEND_BALANCE: u64 = MESSAGE_FEE + 100_0000; // 0.11 DCHAT

/// Minimum stake required to send messages (anti-bot protection)
/// 100 DCHAT = 100_0000_0000 units
/// 
/// # Security Note
/// This prevents sybil attacks where an attacker creates many accounts
/// to spam the network. With a stake requirement:
/// - Each spamming account requires capital at risk
/// - Misbehaving accounts can be slashed, losing their stake
/// - Economic cost deters large-scale bot networks
pub const MINIMUM_STAKE_FOR_MESSAGING: u64 = 100_0000_0000;

/// Error types for message service
#[derive(Debug, Clone, thiserror::Error)]
pub enum MessageServiceError {
    #[error("Insufficient funds: required {required}, available {available}")]
    InsufficientFunds { required: u64, available: u64 },

    #[error("Insufficient stake: required {required}, staked {staked}")]
    InsufficientStake { required: u64, staked: u64 },

    #[error("No relay available for routing")]
    NoRelayAvailable,

    #[error("Message routing failed: {0}")]
    RoutingFailed(String),

    #[error("Fee deduction failed: {0}")]
    FeeDeductionFailed(String),

    #[error("Message validation failed: {0}")]
    ValidationFailed(String),

    #[error("Message expired before sending")]
    MessageExpired,

    #[error("Invalid recipient: {0}")]
    InvalidRecipient(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<MessageServiceError> for Error {
    fn from(err: MessageServiceError) -> Self {
        Error::messaging(err.to_string())
    }
}

/// Receipt for a sent message with fee information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    /// Message ID
    pub message_id: MessageId,

    /// Delivery timestamp
    pub delivered_at: DateTime<Utc>,

    /// Fee paid for this message (in smallest units)
    pub fee_paid: u64,

    /// Transaction ID for fee payment
    pub fee_tx_id: Option<String>,

    /// Relay node that handled the message
    pub relay_id: Option<String>,

    /// Delivery proof (if available)
    pub proof: Option<DeliveryProof>,

    /// Whether message was delivered via payment channel
    pub via_payment_channel: bool,
}

/// Configuration for the message service
#[derive(Debug, Clone)]
pub struct MessageServiceConfig {
    /// Fee per message (default: MESSAGE_FEE)
    pub fee_per_message: u64,

    /// Enable per-byte fee calculation (additional to base fee)
    pub enable_per_byte_fee: bool,

    /// Fee per byte (if enabled)
    pub fee_per_byte: u64,

    /// Maximum message size in bytes
    pub max_message_size: usize,

    /// Maximum retries for fee deduction
    pub fee_deduction_retries: u32,

    /// Retry delay for fee deduction (milliseconds)
    pub fee_retry_delay_ms: u64,
}

impl Default for MessageServiceConfig {
    fn default() -> Self {
        Self {
            fee_per_message: MESSAGE_FEE,
            enable_per_byte_fee: false,
            fee_per_byte: 100, // 0.00000100 DCHAT per byte
            max_message_size: 1024 * 1024, // 1MB
            fee_deduction_retries: 3,
            fee_retry_delay_ms: 100,
        }
    }
}

/// Relay node information for message routing
#[derive(Debug, Clone)]
pub struct RelayNode {
    /// Relay node ID (peer ID or user ID)
    pub id: String,

    /// Relay's UserId for payment
    pub user_id: UserId,

    /// Latency score (lower is better)
    pub latency_score: u32,

    /// Uptime percentage (0-100)
    pub uptime_percent: u8,

    /// Current stake amount
    pub stake_amount: u64,
}

/// Message sending service with integrated fee collection
pub struct MessageService {
    /// Currency chain client for fee deduction
    currency_chain: Arc<CurrencyChainClient>,

    /// Service configuration
    config: MessageServiceConfig,

    /// Available relay nodes (for routing)
    relays: Arc<RwLock<Vec<RelayNode>>>,

    /// Statistics tracking
    stats: Arc<RwLock<MessageServiceStats>>,
}

/// Statistics for the message service
#[derive(Debug, Clone, Default)]
pub struct MessageServiceStats {
    /// Total messages sent
    pub messages_sent: u64,

    /// Total fees collected
    pub total_fees_collected: u64,

    /// Failed sends due to insufficient funds
    pub insufficient_funds_errors: u64,

    /// Failed sends due to routing errors
    pub routing_errors: u64,

    /// Messages sent via payment channels
    pub via_payment_channel: u64,
}

impl MessageService {
    /// Create a new message service
    pub fn new(currency_chain: Arc<CurrencyChainClient>, config: MessageServiceConfig) -> Self {
        Self {
            currency_chain,
            config,
            relays: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(MessageServiceStats::default())),
        }
    }

    /// Create with default configuration
    pub fn with_defaults(currency_chain: Arc<CurrencyChainClient>) -> Self {
        Self::new(currency_chain, MessageServiceConfig::default())
    }

    /// Register a relay node for message routing
    pub async fn register_relay(&self, relay: RelayNode) {
        let mut relays = self.relays.write().await;
        // Remove existing entry if any
        relays.retain(|r| r.id != relay.id);
        relays.push(relay);
    }

    /// Remove a relay node
    pub async fn unregister_relay(&self, relay_id: &str) {
        let mut relays = self.relays.write().await;
        relays.retain(|r| r.id != relay_id);
    }

    /// Calculate fee for a message
    pub fn calculate_fee(&self, message: &Message) -> u64 {
        let base_fee = self.config.fee_per_message;

        if self.config.enable_per_byte_fee {
            base_fee + (message.size as u64 * self.config.fee_per_byte)
        } else {
            base_fee
        }
    }

    /// Check if sender has sufficient balance
    pub fn check_balance(&self, sender_id: &UserId, required: u64) -> Result<u64> {
        let balance = self.currency_chain.get_balance(sender_id)?;
        if balance < required {
            return Err(MessageServiceError::InsufficientFunds {
                required,
                available: balance,
            }
            .into());
        }
        Ok(balance)
    }

    /// Select the best relay for routing
    async fn select_relay(&self, _message: &Message) -> Result<RelayNode> {
        let relays = self.relays.read().await;

        if relays.is_empty() {
            return Err(MessageServiceError::NoRelayAvailable.into());
        }

        // Select relay with best score (lowest latency + highest uptime + highest stake)
        let best_relay = relays
            .iter()
            .max_by(|a, b| {
                let score_a = a.uptime_percent as u64 * a.stake_amount / (a.latency_score as u64 + 1);
                let score_b = b.uptime_percent as u64 * b.stake_amount / (b.latency_score as u64 + 1);
                score_a.cmp(&score_b)
            })
            .cloned()
            .ok_or(MessageServiceError::NoRelayAvailable)?;

        Ok(best_relay)
    }

    /// Send a message with fee collection
    ///
    /// This method:
    /// 1. Validates the message
    /// 2. Verifies sender has minimum stake (anti-bot protection)
    /// 3. Checks sender's balance
    /// 4. Selects a relay for routing
    /// 5. Deducts the message fee (transferred to relay)
    /// 6. Routes the message
    /// 7. Returns a receipt with fee information
    pub async fn send_message(&self, mut message: Message) -> Result<DeliveryReceipt> {
        // 1. Validate message
        self.validate_message(&message)?;

        // Get sender ID
        let sender_id = message
            .sender()
            .ok_or_else(|| MessageServiceError::ValidationFailed("Message has no sender".into()))?;

        // 2. SECURITY FIX: Verify sender has minimum stake (anti-bot protection)
        // This prevents sybil attacks by requiring capital at risk for each sender
        let sender_stake = self.currency_chain.get_staked_amount(&sender_id)?;
        if sender_stake < MINIMUM_STAKE_FOR_MESSAGING {
            tracing::warn!(
                "Sender {} has insufficient stake: {} < {} required",
                sender_id,
                sender_stake,
                MINIMUM_STAKE_FOR_MESSAGING
            );
            return Err(MessageServiceError::InsufficientStake {
                required: MINIMUM_STAKE_FOR_MESSAGING,
                staked: sender_stake,
            }.into());
        }

        // 3. Calculate fee
        let fee = self.calculate_fee(&message);

        // 4. Check sender's balance
        self.check_balance(&sender_id, fee)?;

        // 5. Select relay for routing
        let relay = self.select_relay(&message).await?;

        // 6. Deduct fee and transfer to relay
        let fee_tx_id = self.deduct_fee(&sender_id, &relay.user_id, fee).await?;

        // 7. Route message (update status)
        message.status = MessageStatus::Sent;

        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.messages_sent += 1;
            stats.total_fees_collected += fee;
        }

        // 8. Create and return receipt
        Ok(DeliveryReceipt {
            message_id: message.id.clone(),
            delivered_at: Utc::now(),
            fee_paid: fee,
            fee_tx_id: Some(fee_tx_id.to_string()),
            relay_id: Some(relay.id),
            proof: None,
            via_payment_channel: false,
        })
    }

    /// Validate a message before sending
    fn validate_message(&self, message: &Message) -> Result<()> {
        // Check expiration
        if message.is_expired() {
            return Err(MessageServiceError::MessageExpired.into());
        }

        // Check message size
        if message.size > self.config.max_message_size {
            return Err(MessageServiceError::ValidationFailed(format!(
                "Message too large: {} bytes (max: {})",
                message.size, self.config.max_message_size
            ))
            .into());
        }

        // Validate message type
        match &message.message_type {
            MessageType::Direct { sender, recipient } => {
                if sender == recipient {
                    return Err(MessageServiceError::InvalidRecipient(
                        "Cannot send message to self".into(),
                    )
                    .into());
                }
            }
            MessageType::Channel { sender: _, channel_id: _ } => {
                // Channel messages are valid by default (channel access checked elsewhere)
            }
            MessageType::System { .. } => {
                return Err(MessageServiceError::ValidationFailed(
                    "Cannot send system messages via message service".into(),
                )
                .into());
            }
        }

        Ok(())
    }

    /// Deduct message fee from sender and transfer to relay
    async fn deduct_fee(
        &self,
        sender: &UserId,
        relay: &UserId,
        amount: u64,
    ) -> Result<Uuid> {
        let mut last_error = None;

        for attempt in 0..self.config.fee_deduction_retries {
            match self.currency_chain.transfer(sender, relay, amount) {
                Ok(tx_id) => {
                    tracing::info!(
                        "✅ Fee deducted: {} from {} to {} (tx: {})",
                        amount,
                        sender,
                        relay,
                        tx_id
                    );
                    return Ok(tx_id);
                }
                Err(e) => {
                    tracing::warn!(
                        "Fee deduction attempt {} failed: {}",
                        attempt + 1,
                        e
                    );
                    last_error = Some(e);

                    if attempt < self.config.fee_deduction_retries - 1 {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            self.config.fee_retry_delay_ms,
                        ))
                        .await;
                    }
                }
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.insufficient_funds_errors += 1;
        }

        Err(MessageServiceError::FeeDeductionFailed(
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Unknown error".into()),
        )
        .into())
    }

    /// Get service statistics
    pub async fn get_stats(&self) -> MessageServiceStats {
        self.stats.read().await.clone()
    }

    /// Get configuration
    pub fn get_config(&self) -> &MessageServiceConfig {
        &self.config
    }

    /// Get current relay count
    pub async fn relay_count(&self) -> usize {
        self.relays.read().await.len()
    }
}

/// Pre-paid message credits channel for high-frequency messaging
///
/// Instead of paying per-message on-chain, users can open a payment channel
/// with a relay and use off-chain state updates for each message.
pub struct MessageCreditsChannel {
    /// Channel ID
    pub channel_id: String,

    /// Sender user ID
    pub sender_id: UserId,

    /// Relay user ID
    pub relay_id: UserId,

    /// Current sender balance in channel
    pub sender_balance: u64,

    /// Current relay balance in channel
    pub relay_balance: u64,

    /// Total channel capacity
    pub capacity: u64,

    /// Fee per message
    pub fee_per_message: u64,

    /// Messages sent through this channel
    pub messages_sent: u64,

    /// Current nonce for state updates
    pub nonce: u64,

    /// Channel opened at
    pub opened_at: DateTime<Utc>,

    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
}

/// Signed state update for payment channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedStateUpdate {
    /// Channel ID
    pub channel_id: String,

    /// New nonce
    pub nonce: u64,

    /// New sender balance
    pub sender_balance: u64,

    /// New relay balance
    pub relay_balance: u64,

    /// Sender signature
    pub sender_signature: Vec<u8>,

    /// Update timestamp
    pub timestamp: DateTime<Utc>,
}

/// Error types for message credits channel
#[derive(Debug, Clone, thiserror::Error)]
pub enum CreditsChannelError {
    #[error("Insufficient credits: required {required}, available {available}")]
    InsufficientCredits { required: u64, available: u64 },

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Invalid state update")]
    InvalidStateUpdate,

    #[error("Channel opening failed: {0}")]
    OpeningFailed(String),

    #[error("Settlement failed: {0}")]
    SettlementFailed(String),
}

impl MessageCreditsChannel {
    /// Open a new message credits channel with a relay
    pub async fn open(
        sender: UserId,
        relay: UserId,
        credit_amount: u64,
        currency_chain: &CurrencyChainClient,
    ) -> std::result::Result<Self, CreditsChannelError> {
        // Lock funds in channel (transfer to escrow/channel contract)
        // In production, this would interact with on-chain payment channel contract
        let balance = currency_chain
            .get_balance(&sender)
            .map_err(|e| CreditsChannelError::OpeningFailed(e.to_string()))?;

        if balance < credit_amount {
            return Err(CreditsChannelError::InsufficientCredits {
                required: credit_amount,
                available: balance,
            });
        }

        let channel_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        tracing::info!(
            "📬 Opening message credits channel {} with {} DCHAT",
            channel_id,
            credit_amount as f64 / 100_000_000.0
        );

        Ok(Self {
            channel_id,
            sender_id: sender,
            relay_id: relay,
            sender_balance: credit_amount,
            relay_balance: 0,
            capacity: credit_amount,
            fee_per_message: MESSAGE_FEE,
            messages_sent: 0,
            nonce: 0,
            opened_at: now,
            last_activity: now,
        })
    }

    /// Use one message credit (off-chain update)
    ///
    /// Returns a signed state update that the relay can verify.
    /// The signing_key must be a 32-byte Ed25519 secret key.
    pub fn use_credit(&mut self, signing_key: &[u8]) -> std::result::Result<SignedStateUpdate, CreditsChannelError> {
        if self.sender_balance < self.fee_per_message {
            return Err(CreditsChannelError::InsufficientCredits {
                required: self.fee_per_message,
                available: self.sender_balance,
            });
        }

        // Validate signing key length
        if signing_key.len() != 32 {
            return Err(CreditsChannelError::InvalidStateUpdate);
        }

        // Update balances
        self.sender_balance -= self.fee_per_message;
        self.relay_balance += self.fee_per_message;
        self.nonce += 1;
        self.messages_sent += 1;
        self.last_activity = Utc::now();

        // Create message to sign: hash of (channel_id || nonce || sender_balance || relay_balance || timestamp)
        let mut hasher = Sha256::new();
        hasher.update(self.channel_id.as_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(self.sender_balance.to_le_bytes());
        hasher.update(self.relay_balance.to_le_bytes());
        hasher.update(self.last_activity.timestamp().to_le_bytes());
        let message_hash = hasher.finalize();

        // Sign with Ed25519
        let key_bytes: [u8; 32] = signing_key.try_into()
            .map_err(|_| CreditsChannelError::InvalidStateUpdate)?;
        let ed_signing_key = SigningKey::from_bytes(&key_bytes);
        let signature = ed_signing_key.sign(&message_hash);

        // Create state update with cryptographic signature
        let update = SignedStateUpdate {
            channel_id: self.channel_id.clone(),
            nonce: self.nonce,
            sender_balance: self.sender_balance,
            relay_balance: self.relay_balance,
            sender_signature: signature.to_bytes().to_vec(),
            timestamp: self.last_activity,
        };

        tracing::debug!(
            "Used message credit: channel={}, nonce={}, remaining={}",
            self.channel_id,
            self.nonce,
            self.sender_balance
        );

        Ok(update)
    }

    /// Get remaining credits
    pub fn remaining_credits(&self) -> u64 {
        self.sender_balance / self.fee_per_message
    }

    /// Get remaining balance
    pub fn remaining_balance(&self) -> u64 {
        self.sender_balance
    }

    /// Check if channel has sufficient credits
    pub fn has_credits(&self, count: u64) -> bool {
        self.sender_balance >= count * self.fee_per_message
    }

    /// Settle channel (close and distribute funds)
    pub async fn settle(
        &self,
        currency_chain: &CurrencyChainClient,
    ) -> std::result::Result<Uuid, CreditsChannelError> {
        // In production, this would submit the final state to the on-chain contract
        // For now, we just transfer the relay's balance
        if self.relay_balance > 0 {
            // Transfer accumulated fees to relay
            // Note: In real implementation, this would be handled by the payment channel contract
            let tx_id = currency_chain
                .transfer(&self.sender_id, &self.relay_id, self.relay_balance)
                .map_err(|e| CreditsChannelError::SettlementFailed(e.to_string()))?;

            tracing::info!(
                "✅ Settled message credits channel {}: {} messages, {} fees",
                self.channel_id,
                self.messages_sent,
                self.relay_balance
            );

            Ok(tx_id)
        } else {
            // No fees accumulated - channel is already settled with zero balance
            // Return a new UUID as settlement confirmation (no on-chain tx needed)
            tracing::debug!(
                "Channel {} settled with zero relay balance - no on-chain transfer needed",
                self.channel_id
            );
            Ok(Uuid::new_v4())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MessageBuilder, MessageType};
    use dchat_blockchain::BlockchainConfig;
    use dchat_core::types::MessageContent;

    fn create_test_currency_chain() -> Arc<CurrencyChainClient> {
        let config = BlockchainConfig::default();
        Arc::new(CurrencyChainClient::new(config))
    }

    fn create_test_message(sender: UserId, recipient: UserId) -> Message {
        MessageBuilder::new()
            .direct(sender, recipient)
            .content(MessageContent::Text("Hello".to_string()))
            .encrypted_payload(vec![1, 2, 3, 4])
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn test_message_service_fee_calculation() {
        let chain = create_test_currency_chain();
        let service = MessageService::with_defaults(chain);

        let sender = UserId(Uuid::new_v4());
        let recipient = UserId(Uuid::new_v4());
        let message = create_test_message(sender, recipient);

        let fee = service.calculate_fee(&message);
        assert_eq!(fee, MESSAGE_FEE);
    }

    #[tokio::test]
    async fn test_message_service_per_byte_fee() {
        let chain = create_test_currency_chain();
        let config = MessageServiceConfig {
            enable_per_byte_fee: true,
            fee_per_byte: 100,
            ..Default::default()
        };
        let service = MessageService::new(chain, config);

        let sender = UserId(Uuid::new_v4());
        let recipient = UserId(Uuid::new_v4());
        let message = create_test_message(sender, recipient);

        let fee = service.calculate_fee(&message);
        // Base fee + (4 bytes * 100 per byte)
        assert_eq!(fee, MESSAGE_FEE + 400);
    }

    #[tokio::test]
    async fn test_message_service_insufficient_funds() {
        let chain = create_test_currency_chain();
        let service = MessageService::with_defaults(chain.clone());

        let sender = UserId(Uuid::new_v4());
        let recipient = UserId(Uuid::new_v4());

        // Don't fund the sender - should fail balance check
        let result = service.check_balance(&sender, MESSAGE_FEE);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_message_service_balance_check() {
        let chain = create_test_currency_chain();
        let service = MessageService::with_defaults(chain.clone());

        let sender = UserId(Uuid::new_v4());

        // Fund the sender
        chain.create_wallet(&sender, MESSAGE_FEE * 10).unwrap();

        // Should pass balance check
        let result = service.check_balance(&sender, MESSAGE_FEE);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), MESSAGE_FEE * 10);
    }

    #[tokio::test]
    async fn test_message_credits_channel_open() {
        let chain = create_test_currency_chain();
        let sender = UserId(Uuid::new_v4());
        let relay = UserId(Uuid::new_v4());

        // Fund sender
        chain.create_wallet(&sender, MESSAGE_FEE * 100).unwrap();

        // Open credits channel
        let channel = MessageCreditsChannel::open(
            sender.clone(),
            relay.clone(),
            MESSAGE_FEE * 50,
            &chain,
        )
        .await;

        assert!(channel.is_ok());
        let channel = channel.unwrap();
        assert_eq!(channel.sender_balance, MESSAGE_FEE * 50);
        assert_eq!(channel.relay_balance, 0);
        assert_eq!(channel.remaining_credits(), 50);
    }

    #[tokio::test]
    async fn test_message_credits_channel_use() {
        let chain = create_test_currency_chain();
        let sender = UserId(Uuid::new_v4());
        let relay = UserId(Uuid::new_v4());

        // Fund sender
        chain.create_wallet(&sender, MESSAGE_FEE * 100).unwrap();

        // Open credits channel
        let mut channel = MessageCreditsChannel::open(
            sender.clone(),
            relay.clone(),
            MESSAGE_FEE * 10,
            &chain,
        )
        .await
        .unwrap();

        // Use 5 credits
        for _ in 0..5 {
            let update = channel.use_credit(&[]).unwrap();
            assert_eq!(update.sender_balance, channel.sender_balance);
        }

        assert_eq!(channel.remaining_credits(), 5);
        assert_eq!(channel.messages_sent, 5);
        assert_eq!(channel.nonce, 5);
    }

    #[tokio::test]
    async fn test_message_credits_channel_insufficient() {
        let chain = create_test_currency_chain();
        let sender = UserId(Uuid::new_v4());
        let relay = UserId(Uuid::new_v4());

        // Fund sender with just 2 message fees
        chain.create_wallet(&sender, MESSAGE_FEE * 2).unwrap();

        // Open credits channel with 2 messages worth
        let mut channel = MessageCreditsChannel::open(
            sender.clone(),
            relay.clone(),
            MESSAGE_FEE * 2,
            &chain,
        )
        .await
        .unwrap();

        // Use 2 credits (should succeed)
        channel.use_credit(&[]).unwrap();
        channel.use_credit(&[]).unwrap();

        // Third should fail
        let result = channel.use_credit(&[]);
        assert!(matches!(
            result,
            Err(CreditsChannelError::InsufficientCredits { .. })
        ));
    }

    #[test]
    fn test_validate_expired_message() {
        let chain = create_test_currency_chain();
        let service = MessageService::with_defaults(chain);

        let sender = UserId(Uuid::new_v4());
        let recipient = UserId(Uuid::new_v4());

        // Create expired message
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(10);
        let message = MessageBuilder::new()
            .direct(sender, recipient)
            .content(MessageContent::Text("Expired".to_string()))
            .encrypted_payload(vec![1, 2, 3])
            .expires_at(past)
            .build()
            .unwrap();

        let result = service.validate_message(&message);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_self_message() {
        let chain = create_test_currency_chain();
        let service = MessageService::with_defaults(chain);

        let user = UserId(Uuid::new_v4());

        // Create self-addressed message
        let message = MessageBuilder::new()
            .direct(user.clone(), user.clone())
            .content(MessageContent::Text("Self".to_string()))
            .encrypted_payload(vec![1, 2, 3])
            .build()
            .unwrap();

        let result = service.validate_message(&message);
        assert!(result.is_err());
    }
}
