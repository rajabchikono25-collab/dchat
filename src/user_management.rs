//! User management and account operations
//!
//! Provides:
//! - User creation and registration with on-chain transactions
//! - Key pair generation and storage
//! - User profile management
//! - Direct messaging with blockchain confirmation
//! - Channel creation with on-chain registration

use crate::prelude::*;
use dchat_blockchain::{ChatChainClient, CrossChainBridge, CurrencyChainClient};
use dchat_core::error::{Error, Result};
use dchat_core::types::{ChannelId, UserId};
use dchat_crypto::keys::KeyPair;
use dchat_identity::Identity;
use dchat_storage::Database;
#[cfg(feature = "test-bypass")]
use dchat_storage::MessageRow;
use hex;
use serde::{Deserialize, Serialize};
#[cfg(feature = "test-bypass")]
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tracing::{error, info};
use uuid::Uuid;

// Prevent enabling the dangerous test-bypass feature in release builds
#[cfg(all(feature = "test-bypass", not(debug_assertions)))]
compile_error!("The 'test-bypass' feature must never be enabled in production builds.");

/// Response when creating a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserResponse {
    pub user_id: String,
    pub username: String,
    pub public_key: String,
    pub private_key: String,
    pub created_at: String,
    pub on_chain_confirmed: bool,
    pub tx_id: Option<String>,
    pub message: String,
}

/// User profile information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub public_key: String,
    pub reputation_score: i32,
    pub verified: bool,
    pub created_at: String,
    pub badges: Vec<String>,
}

/// Direct message creation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMessageRequest {
    pub sender_id: String,
    pub recipient_id: String,
    pub content: String,
    pub encrypted: bool,
}

/// Direct message response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMessageResponse {
    pub message_id: String,
    pub status: String,
    pub timestamp: String,
    pub on_chain_confirmed: bool,
    pub tx_id: Option<String>,
}

/// Channel creation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelRequest {
    pub creator_id: String,
    pub channel_name: String,
    pub description: Option<String>,
    pub token_gated: bool,
    pub entry_fee: Option<u64>,
}

/// Channel creation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChannelResponse {
    pub channel_id: String,
    pub channel_name: String,
    pub creator_id: String,
    pub created_at: String,
    pub on_chain_confirmed: bool,
    pub tx_id: Option<String>,
}

/// User manager for handling account operations
pub struct UserManager {
    database: Database,
    chat_chain: std::sync::Arc<ChatChainClient>,
    currency_chain: std::sync::Arc<CurrencyChainClient>,
    bridge: std::sync::Arc<CrossChainBridge>,
    keys_dir: PathBuf,
}

impl UserManager {
    /// Create a new user manager with parallel chains
    pub fn new(
        database: Database,
        chat_chain: std::sync::Arc<ChatChainClient>,
        currency_chain: std::sync::Arc<CurrencyChainClient>,
        bridge: std::sync::Arc<CrossChainBridge>,
        keys_dir: PathBuf,
    ) -> Self {
        Self {
            database,
            chat_chain,
            currency_chain,
            bridge,
            keys_dir,
        }
    }

    /// Create a new user with generated keypair and on-chain registration
    pub async fn create_user(&self, username: &str) -> Result<CreateUserResponse> {
        info!("Creating new user: {}", username);

        // Generate new keypair
        let keypair = KeyPair::try_generate()
            .map_err(|e| Error::crypto(format!("Key generation failed: {}", e)))?;
        let public_key_bytes = keypair.public_key().as_bytes();
        let public_key_hex = hex::encode(public_key_bytes);
        let private_key_bytes = keypair.private_key().as_bytes();
        let private_key_hex = hex::encode(private_key_bytes);

        // Create identity
        let identity = Identity::new(username.to_string(), &keypair);
        let user_id_uuid = UserId(
            uuid::Uuid::parse_str(&identity.user_id.to_string())
                .map_err(|e| Error::validation(format!("Invalid user ID: {}", e)))?,
        );
        let created_at_rfc3339 = chrono::Utc::now().to_rfc3339();

        // Submit on-chain transaction to chat chain
        info!("Registering user on chat chain...");
        let tx_id = self
            .chat_chain
            .register_user(&user_id_uuid, public_key_bytes.to_vec())
            .await
            .map_err(|e| {
                error!("Failed to register on chat chain: {}", e);
                Error::internal(format!("Chat chain registration failed: {}", e))
            })?;

        // Wait for blockchain finality confirmation (3 blocks)
        info!("Waiting for on-chain confirmation (finality threshold: 3 blocks)...");
        let on_chain_confirmed = self
            .chat_chain
            .wait_for_finality(&tx_id, 3)
            .await
            .map_err(|e| Error::chain(e.to_string()))?;
        if on_chain_confirmed {
            info!("✓ User registration confirmed on chat chain");
        } else {
            return Err(Error::chain("Transaction failed to achieve finality"));
        }

        // Store user in database after on-chain confirmation
        self.database
            .insert_user(&identity.user_id.to_string(), username, public_key_bytes)
            .await
            .map_err(|e| {
                error!("Failed to store user in database: {}", e);
                e
            })?;

        info!(
            "✓ User created successfully: {} ({})",
            username, identity.user_id
        );

        Ok(CreateUserResponse {
            user_id: identity.user_id.to_string(),
            username: username.to_string(),
            public_key: public_key_hex,
            private_key: private_key_hex,
            created_at: created_at_rfc3339,
            on_chain_confirmed,
            tx_id: Some(tx_id.to_string()),
            message: "User created and confirmed on-chain! Store your private key safely."
                .to_string(),
        })
    }

    /// Get user profile
    pub async fn get_user_profile(&self, user_id: &str) -> Result<UserProfile> {
        info!("Fetching user profile: {}", user_id);

        let user = self
            .database
            .get_user(user_id)
            .await?
            .ok_or_else(|| Error::storage(format!("User not found: {}", user_id)))?;

        let public_key_hex = hex::encode(&user.public_key);
        let created_at_rfc3339 =
            if let Some(dt) = chrono::DateTime::from_timestamp(user.created_at, 0) {
                dt.to_rfc3339()
            } else {
                return Err(Error::internal("Invalid timestamp in user data"));
            };

        Ok(UserProfile {
            user_id: user.id,
            username: user.username.clone(),
            display_name: Some(format!("@{}", user.username)),
            public_key: public_key_hex,
            reputation_score: 0, // Will be enhanced with on-chain data
            verified: false,
            created_at: created_at_rfc3339,
            badges: vec![],
        })
    }

    /// List all users from database
    pub async fn list_users(&self) -> Result<Vec<UserProfile>> {
        info!("Listing all users");

        // Fetch all users from database
        let users = self.database.list_all_users().await?;

        // Convert to UserProfile format
        let mut profiles = Vec::new();
        for user in users {
            let public_key_hex = hex::encode(&user.public_key);
            let created_at_rfc3339 =
                if let Some(dt) = chrono::DateTime::from_timestamp(user.created_at, 0) {
                    dt.to_rfc3339()
                } else {
                    return Err(Error::internal("Invalid timestamp in user data"));
                };

            profiles.push(UserProfile {
                user_id: user.id,
                username: user.username.clone(),
                display_name: Some(format!("@{}", user.username)),
                public_key: public_key_hex,
                reputation_score: 0,
                verified: false,
                created_at: created_at_rfc3339,
                badges: vec![],
            });
        }

        Ok(profiles)
    }

    /// Send direct message with on-chain confirmation
    ///
    /// # Deprecated
    ///
    /// **WARNING**: This method bypasses the fee gateway and does NOT charge message fees
    /// or enforce proper finality. Use `FeeGateway::send_direct_message()` instead.
    ///
    /// This bypass will be removed before mainnet. Any code path calling this method
    /// directly allows unpaid message storage, which is a critical security issue.
    ///
    /// # Availability
    ///
    /// This method is only available when the `test-bypass` feature is enabled.
    /// Production builds MUST NOT enable this feature.
    #[cfg(feature = "test-bypass")]
    #[deprecated(
        since = "0.1.0",
        note = "Use FeeGateway::send_direct_message() to enforce fee payment. This bypass will be removed."
    )]
    pub async fn send_direct_message(
        &self,
        sender_id: &str,
        recipient_id: &str,
        content: &str,
    ) -> Result<DirectMessageResponse> {
        info!("Sending DM from {} to {}", sender_id, recipient_id);

        // Verify both users exist
        let _sender = self.get_user_profile(sender_id).await?;
        let _recipient = self.get_user_profile(recipient_id).await?;

        // Generate message ID
        let message_id = MessageId(Uuid::new_v4());
        let sender_uuid = UserId(
            uuid::Uuid::parse_str(sender_id)
                .map_err(|e| Error::validation(format!("Invalid sender ID: {}", e)))?,
        );
        let recipient_uuid = UserId(
            uuid::Uuid::parse_str(recipient_id)
                .map_err(|e| Error::validation(format!("Invalid recipient ID: {}", e)))?,
        );
        let timestamp = chrono::Utc::now().timestamp();
        let timestamp_rfc3339 = chrono::Utc::now().to_rfc3339();

        // Calculate content hash (for database storage)
        let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        let _payload_size = content.len();

        // Submit on-chain transaction to chat chain for message ordering
        info!("Recording message on chat chain...");
        let tx_id = self
            .chat_chain
            .send_direct_message(&sender_uuid, &recipient_uuid, message_id)
            .await
            .map_err(|e| {
                error!("Failed to record on chat chain: {}", e);
                Error::internal(format!("Chat chain recording failed: {}", e))
            })?;

        // Simulate confirmation
        let on_chain_confirmed = true;

        // Store message in database
        self.database
            .insert_message(&MessageRow {
                id: message_id.to_string(),
                sender_id: sender_id.to_string(),
                recipient_id: Some(recipient_id.to_string()),
                channel_id: None,
                content_type: "direct_message".to_string(),
                content: content.to_string(),
                encrypted_payload: content.as_bytes().to_vec(),
                timestamp,
                sequence_num: None,
                status: if on_chain_confirmed {
                    "confirmed"
                } else {
                    "pending"
                }
                .to_string(),
                expires_at: None,
                size: content.len(),
                content_hash: Some(content_hash),
            })
            .await
            .map_err(|e| {
                error!("Failed to store message: {}", e);
                e
            })?;

        info!(
            "✓ Direct message sent and confirmed on-chain: {}",
            message_id
        );

        Ok(DirectMessageResponse {
            message_id: message_id.to_string(),
            status: "sent".to_string(),
            timestamp: timestamp_rfc3339,
            on_chain_confirmed,
            tx_id: Some(tx_id.to_string()),
        })
    }

    /// Create a new channel with on-chain registration
    pub async fn create_channel(
        &self,
        creator_id: &str,
        channel_name: &str,
        _description: Option<&str>,
    ) -> Result<CreateChannelResponse> {
        info!("Creating channel: {} by {}", channel_name, creator_id);

        // Verify creator exists
        let _creator = self.get_user_profile(creator_id).await?;

        // Generate channel ID
        let channel_id = ChannelId(Uuid::new_v4());
        let creator_uuid = UserId(
            uuid::Uuid::parse_str(creator_id)
                .map_err(|e| Error::validation(format!("Invalid creator ID: {}", e)))?,
        );
        let created_at = chrono::Utc::now().to_rfc3339();

        // Submit on-chain transaction to chat chain
        info!("Creating channel on chat chain...");
        let tx_id = self
            .chat_chain
            .create_channel(&creator_uuid, &channel_id, channel_name.to_string())
            .await
            .map_err(|e| {
                error!("Failed to create channel on chat chain: {}", e);
                Error::internal(format!("Chat chain channel creation failed: {}", e))
            })?;

        // Wait for blockchain confirmation
        info!("Waiting for on-chain channel creation confirmation...");
        let on_chain_confirmed = self
            .chat_chain
            .wait_for_finality(&tx_id, 3)
            .await
            .map_err(|e| Error::chain(e.to_string()))?;

        info!(
            "✓ Channel created and confirmed on-chain: {} ({})",
            channel_name, channel_id
        );

        Ok(CreateChannelResponse {
            channel_id: channel_id.to_string(),
            channel_name: channel_name.to_string(),
            creator_id: creator_id.to_string(),
            created_at,
            on_chain_confirmed,
            tx_id: Some(tx_id.to_string()),
        })
    }

    /// Post message to channel with on-chain confirmation
    ///
    /// # Deprecated
    ///
    /// **WARNING**: This method bypasses the fee gateway and does NOT charge message fees
    /// or enforce proper finality. Use `FeeGateway::post_to_channel()` instead.
    ///
    /// This bypass will be removed before mainnet. Any code path calling this method
    /// directly allows unpaid message storage, which is a critical security issue.
    ///
    /// # Availability
    ///
    /// This method is only available when the `test-bypass` feature is enabled.
    /// Production builds MUST NOT enable this feature.
    #[cfg(feature = "test-bypass")]
    #[deprecated(
        since = "0.1.0",
        note = "Use FeeGateway::post_to_channel() to enforce fee payment. This bypass will be removed."
    )]
    pub async fn post_to_channel(
        &self,
        sender_id: &str,
        channel_id: &str,
        content: &str,
    ) -> Result<DirectMessageResponse> {
        info!("Posting to channel {} by user {}", channel_id, sender_id);

        // Verify user exists
        let _sender = self.get_user_profile(sender_id).await?;

        // Generate message ID
        let message_id = MessageId(Uuid::new_v4());
        let sender_uuid = UserId(
            uuid::Uuid::parse_str(sender_id)
                .map_err(|e| Error::validation(format!("Invalid sender ID: {}", e)))?,
        );
        let channel_uuid = ChannelId(
            uuid::Uuid::parse_str(channel_id)
                .map_err(|e| Error::validation(format!("Invalid channel ID: {}", e)))?,
        );
        let timestamp = chrono::Utc::now().timestamp();
        let timestamp_rfc3339 = chrono::Utc::now().to_rfc3339();

        // Calculate content hash (for database storage)
        let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        let _payload_size = content.len();

        // Submit on-chain transaction to chat chain for message ordering
        info!("Posting message to chat chain...");
        let tx_id = self
            .chat_chain
            .post_to_channel(&sender_uuid, &channel_uuid, message_id)
            .await
            .map_err(|e| {
                error!("Failed to post to chat chain: {}", e);
                Error::internal(format!("Chat chain posting failed: {}", e))
            })?;

        // Wait for blockchain confirmation
        let on_chain_confirmed = self
            .chat_chain
            .wait_for_finality(&tx_id, 3)
            .await
            .map_err(|e| Error::chain(e.to_string()))?;

        // Store message in database
        self.database
            .insert_message(&MessageRow {
                id: message_id.to_string(),
                sender_id: sender_id.to_string(),
                recipient_id: None,
                channel_id: Some(channel_id.to_string()),
                content_type: "channel_message".to_string(),
                content: content.to_string(),
                encrypted_payload: content.as_bytes().to_vec(),
                timestamp,
                sequence_num: None,
                status: if on_chain_confirmed {
                    "confirmed"
                } else {
                    "pending"
                }
                .to_string(),
                expires_at: None,
                size: content.len(),
                content_hash: Some(content_hash),
            })
            .await
            .map_err(|e| {
                error!("Failed to store message: {}", e);
                e
            })?;

        info!(
            "✓ Message posted to channel and confirmed on-chain: {}",
            message_id
        );

        Ok(DirectMessageResponse {
            message_id: message_id.to_string(),
            status: "posted".to_string(),
            timestamp: timestamp_rfc3339,
            on_chain_confirmed,
            tx_id: Some(tx_id.to_string()),
        })
    }

    /// Get user's direct messages with on-chain confirmation status
    pub async fn get_direct_messages(&self, user_id: &str) -> Result<Vec<DirectMessageResponse>> {
        info!("Fetching DMs for user: {}", user_id);

        let messages = self.database.get_messages_for_user(user_id, 100).await?;

        let mut dms = Vec::new();
        for msg in messages {
            if msg.recipient_id.is_some() {
                let timestamp_rfc3339 =
                    if let Some(dt) = chrono::DateTime::from_timestamp(msg.timestamp, 0) {
                        dt.to_rfc3339()
                    } else {
                        return Err(Error::internal("Invalid message timestamp"));
                    };

                // Check if message status indicates on-chain confirmation
                let on_chain_confirmed = msg.status == "confirmed";

                dms.push(DirectMessageResponse {
                    message_id: msg.id.clone(),
                    status: msg.status,
                    timestamp: timestamp_rfc3339,
                    on_chain_confirmed,
                    tx_id: None, // Transaction ID stored separately on blockchain
                });
            }
        }

        Ok(dms)
    }

    /// Get channel messages with on-chain confirmation status
    pub async fn get_channel_messages(
        &self,
        channel_id: &str,
    ) -> Result<Vec<DirectMessageResponse>> {
        info!("Fetching messages for channel: {}", channel_id);

        // Get all messages from database and filter for this channel
        // Note: We need to retrieve messages that have channel_id set and no recipient_id
        let all_messages = self
            .database
            .get_all_messages(1000)
            .await
            .unwrap_or_default();

        let mut channel_msgs = Vec::new();
        for msg in all_messages {
            // Filter for channel messages: no recipient (NULL), matching channel_id
            if msg.recipient_id.is_none() && msg.channel_id.as_deref() == Some(channel_id) {
                let timestamp_rfc3339 =
                    if let Some(dt) = chrono::DateTime::from_timestamp(msg.timestamp, 0) {
                        dt.to_rfc3339()
                    } else {
                        return Err(Error::internal("Invalid message timestamp"));
                    };

                // Check if message status indicates on-chain confirmation
                let on_chain_confirmed = msg.status == "confirmed";

                channel_msgs.push(DirectMessageResponse {
                    message_id: msg.id.clone(),
                    status: msg.status,
                    timestamp: timestamp_rfc3339,
                    on_chain_confirmed,
                    tx_id: None, // Transaction ID stored separately on blockchain
                });
            }
        }

        Ok(channel_msgs)
    }

    /// Get user's token balance from currency chain
    pub async fn get_user_balance(&self, user_id: &str) -> Result<u64> {
        info!("Fetching balance for user: {}", user_id);

        let user_uuid = UserId(
            uuid::Uuid::parse_str(user_id)
                .map_err(|e| Error::validation(format!("Invalid user ID: {}", e)))?,
        );

        let balance = self
            .currency_chain
            .get_balance(&user_uuid)
            .map_err(|e| Error::chain(format!("Failed to get balance: {}", e)))?;

        Ok(balance)
    }

    /// Transfer tokens between users using the currency chain
    pub async fn transfer_tokens(
        &self,
        from_user_id: &str,
        to_user_id: &str,
        amount: u64,
    ) -> Result<String> {
        info!(
            "Transferring {} tokens from {} to {}",
            amount, from_user_id, to_user_id
        );

        // Verify both users exist
        let _sender = self.get_user_profile(from_user_id).await?;
        let _recipient = self.get_user_profile(to_user_id).await?;

        let from_uuid = UserId(
            uuid::Uuid::parse_str(from_user_id)
                .map_err(|e| Error::validation(format!("Invalid sender ID: {}", e)))?,
        );
        let to_uuid = UserId(
            uuid::Uuid::parse_str(to_user_id)
                .map_err(|e| Error::validation(format!("Invalid recipient ID: {}", e)))?,
        );

        // Execute transfer on currency chain
        let tx_id = self
            .currency_chain
            .transfer(&from_uuid, &to_uuid, amount)
            .map_err(|e| Error::chain(format!("Transfer failed: {}", e)))?;

        info!("✓ Transfer completed: {} tokens, tx_id: {}", amount, tx_id);

        Ok(tx_id.to_string())
    }

    /// Initiate cross-chain bridge transfer
    pub async fn bridge_transfer(
        &self,
        user_id: &str,
        amount: u64,
        target_chain: &str,
    ) -> Result<String> {
        info!(
            "Bridging {} tokens for user {} to {}",
            amount, user_id, target_chain
        );

        let user_uuid = UserId(
            uuid::Uuid::parse_str(user_id)
                .map_err(|e| Error::validation(format!("Invalid user ID: {}", e)))?,
        );

        // Lock tokens on currency chain and initiate bridge transfer
        let bridge_tx = self
            .bridge
            .initiate_transfer(&user_uuid, amount, target_chain)
            .await
            .map_err(|e| Error::chain(format!("Bridge transfer failed: {}", e)))?;

        info!(
            "✓ Bridge transfer initiated: {} tokens to {}, tx: {}",
            amount, target_chain, bridge_tx
        );

        Ok(bridge_tx.to_string())
    }

    /// Get cross-chain bridge status for a transfer
    pub async fn get_bridge_status(&self, bridge_tx_id: &str) -> Result<String> {
        let tx_uuid = uuid::Uuid::parse_str(bridge_tx_id)
            .map_err(|e| Error::validation(format!("Invalid bridge TX ID: {}", e)))?;

        let status = self
            .bridge
            .get_transaction_status(&tx_uuid)
            .await
            .map_err(|e| Error::chain(format!("Failed to get bridge status: {}", e)))?;

        Ok(format!("{:?}", status))
    }

    /// Export user keypair to file (uses keys_dir)
    pub async fn export_user_keys(&self, user_id: &str) -> Result<std::path::PathBuf> {
        info!("Exporting keys for user: {}", user_id);

        // Verify user exists
        let user = self.get_user_profile(user_id).await?;

        // Create user-specific key file path
        let key_file = self.keys_dir.join(format!("{}.key", user_id));

        // Export public key (private keys should be managed by user's local wallet)
        let key_data = serde_json::json!({
            "user_id": user.user_id,
            "username": user.username,
            "public_key": user.public_key,
            "exported_at": chrono::Utc::now().to_rfc3339(),
        });

        // Ensure keys directory exists
        tokio::fs::create_dir_all(&self.keys_dir)
            .await
            .map_err(|e| Error::storage(format!("Failed to create keys directory: {}", e)))?;

        let key_json = serde_json::to_string_pretty(&key_data)
            .map_err(|e| Error::storage(format!("Failed to serialize key data: {}", e)))?;
        tokio::fs::write(&key_file, key_json)
            .await
            .map_err(|e| Error::storage(format!("Failed to write key file: {}", e)))?;

        info!("✓ Keys exported to {:?}", key_file);

        Ok(key_file)
    }

    /// Import user keys from file (uses keys_dir)
    pub async fn import_user_keys(&self, key_file: &std::path::Path) -> Result<String> {
        info!("Importing keys from {:?}", key_file);

        let key_data = tokio::fs::read_to_string(key_file)
            .await
            .map_err(|e| Error::storage(format!("Failed to read key file: {}", e)))?;

        let key_info: serde_json::Value = serde_json::from_str(&key_data)
            .map_err(|e| Error::validation(format!("Invalid key file format: {}", e)))?;

        let user_id = key_info["user_id"]
            .as_str()
            .ok_or_else(|| Error::validation("Missing user_id in key file"))?;

        info!("✓ Keys imported for user: {}", user_id);

        Ok(user_id.to_string())
    }
}
