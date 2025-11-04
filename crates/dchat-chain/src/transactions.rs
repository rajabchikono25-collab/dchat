//! On-chain transaction types for user operations
//!
//! This module defines blockchain transaction types for:
//! - User registration and identity management
//! - Direct messaging and channel communication
//! - Channel creation and access control
//! - Message confirmation and proof-of-delivery

use chrono::{DateTime, Utc};
use dchat_core::types::{ChannelId, MessageId, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Transaction type identifier
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionType {
    /// Register a new user identity on-chain
    RegisterUser,
    /// Send a direct message (stores hash/metadata)
    SendDirectMessage,
    /// Create a new channel
    CreateChannel,
    /// Post message to channel
    PostToChannel,
    /// Join a channel
    JoinChannel,
    /// Update user profile
    UpdateProfile,
}

/// On-chain user registration transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterUserTx {
    /// Unique user identifier
    pub user_id: UserId,
    /// Username
    pub username: String,
    /// Ed25519 public key (hex encoded)
    pub public_key: String,
    /// Registration timestamp
    pub timestamp: DateTime<Utc>,
    /// Initial reputation score (default 0)
    pub initial_reputation: i64,
}

/// On-chain direct message transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendDirectMessageTx {
    /// Unique message identifier
    pub message_id: MessageId,
    /// Sender user ID
    pub sender_id: UserId,
    /// Recipient user ID
    pub recipient_id: UserId,
    /// Message content hash (SHA-256, for integrity verification)
    pub content_hash: String,
    /// Message timestamp
    pub timestamp: DateTime<Utc>,
    /// Encrypted payload size (for relay reward calculation)
    pub payload_size: usize,
    /// Optional relay node that delivered
    pub relay_node_id: Option<String>,
}

/// On-chain channel creation transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelTx {
    /// Unique channel identifier
    pub channel_id: ChannelId,
    /// Channel name
    pub name: String,
    /// Channel description
    pub description: String,
    /// Channel creator user ID
    pub creator_id: UserId,
    /// Channel visibility (public/private/token-gated)
    pub visibility: ChannelVisibility,
    /// Creation timestamp
    pub timestamp: DateTime<Utc>,
    /// Required stake for moderation (if any)
    pub stake_amount: Option<u64>,
}

/// On-chain channel message transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToChannelTx {
    /// Unique message identifier
    pub message_id: MessageId,
    /// Channel identifier
    pub channel_id: ChannelId,
    /// Sender user ID
    pub sender_id: UserId,
    /// Message content hash (SHA-256)
    pub content_hash: String,
    /// Message timestamp
    pub timestamp: DateTime<Utc>,
    /// Payload size
    pub payload_size: usize,
}

/// On-chain channel join transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinChannelTx {
    /// Channel identifier
    pub channel_id: ChannelId,
    /// User joining the channel
    pub user_id: UserId,
    /// Join timestamp
    pub timestamp: DateTime<Utc>,
    /// Optional access token (for token-gated channels)
    pub access_token: Option<String>,
}

/// Channel visibility types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelVisibility {
    /// Anyone can join
    Public,
    /// Invite-only
    Private,
    /// Requires specific token/NFT
    TokenGated { token_id: String },
}

/// Transaction status on blockchain
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Transaction submitted to mempool
    Pending,
    /// Transaction included in block
    Confirmed {
        block_height: u64,
        block_hash: String,
    },
    /// Transaction failed validation
    Failed { reason: String },
    /// Transaction timed out
    TimedOut,
}

/// Generic blockchain transaction wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Transaction identifier
    pub tx_id: Uuid,
    /// Transaction type
    pub tx_type: TransactionType,
    /// Transaction payload (serialized specific transaction)
    pub payload: Vec<u8>,
    /// Transaction hash (SHA-256 of payload)
    pub tx_hash: String,
    /// Current status
    pub status: TransactionStatus,
    /// Submission timestamp
    pub submitted_at: DateTime<Utc>,
    /// Confirmation timestamp (if confirmed)
    pub confirmed_at: Option<DateTime<Utc>>,
    /// Gas/fee paid
    pub fee_paid: u64,
}

impl Transaction {
    /// Create a new transaction
    pub fn new(tx_type: TransactionType, payload: Vec<u8>) -> Self {
        use sha2::{Digest, Sha256};

        let tx_hash = format!("{:x}", Sha256::digest(&payload));

        Self {
            tx_id: Uuid::new_v4(),
            tx_type,
            payload,
            tx_hash,
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        }
    }

    /// Check if transaction is confirmed
    pub fn is_confirmed(&self) -> bool {
        matches!(self.status, TransactionStatus::Confirmed { .. })
    }

    /// Check if transaction is pending
    pub fn is_pending(&self) -> bool {
        matches!(self.status, TransactionStatus::Pending)
    }

    /// Check if transaction failed
    pub fn is_failed(&self) -> bool {
        matches!(
            self.status,
            TransactionStatus::Failed { .. } | TransactionStatus::TimedOut
        )
    }
}

/// Transaction receipt after confirmation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Block height where confirmed
    pub block_height: u64,
    /// Block hash
    pub block_hash: String,
    /// Transaction index in block
    pub tx_index: u32,
    /// Gas used
    pub gas_used: u64,
    /// Confirmation timestamp
    pub confirmed_at: DateTime<Utc>,
    /// Success or failure
    pub success: bool,
    /// Optional error message
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_register_user_tx() {
        let user_id = UserId::new();
        let tx = RegisterUserTx {
            user_id: user_id.clone(),
            username: "alice".to_string(),
            public_key: "deadbeef".to_string(),
            timestamp: Utc::now(),
            initial_reputation: 0,
        };

        assert_eq!(tx.user_id, user_id);
        assert_eq!(tx.username, "alice");
    }

    #[test]
    fn test_transaction_status() {
        let payload = b"test_payload".to_vec();
        let tx = Transaction::new(TransactionType::RegisterUser, payload);

        assert!(tx.is_pending());
        assert!(!tx.is_confirmed());
        assert!(!tx.is_failed());
    }

    #[test]
    fn test_channel_visibility() {
        let public_channel = CreateChannelTx {
            channel_id: ChannelId::new(),
            name: "general".to_string(),
            description: "Public channel".to_string(),
            creator_id: UserId::new(),
            visibility: ChannelVisibility::Public,
            timestamp: Utc::now(),
            stake_amount: None,
        };

        assert_eq!(public_channel.visibility, ChannelVisibility::Public);
    }
}
