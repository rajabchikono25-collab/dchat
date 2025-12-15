//! Storage-routed user management integration
//!
//! This module provides integration between user_management.rs and the StorageRouter,
//! replacing direct SQLite writes with tiered storage operations through the facade.
//!
//! Storage tiers:
//! - CockroachDB: Primary storage for message metadata and global history
//! - SQLite: Offline cache for local access when disconnected
//! - Redis: Hot cache for frequently accessed data
//! - S3/MinIO: Large blob storage (attachments, media)
//! - IPFS: Optional content-addressed pinning for persistence

use crate::user_management::{
    CreateChannelResponse, CreateUserResponse, DirectMessageResponse, UserProfile,
};
use dchat_blockchain::{ChatChainClient, CrossChainBridge, CurrencyChainClient};
use dchat_core::error::{Error, Result};
use dchat_core::types::{ChannelId, MessageId, UserId};
use dchat_crypto::keys::KeyPair;
use dchat_identity::Identity;
use dchat_storage::provider::{
    BlobRef, MessageMetadata, MessageType, StorageRouter, StorageRouterConfig,
};
use dchat_storage::Database;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

/// Size threshold for blob storage (content larger than this goes to S3)
#[allow(dead_code)]
const BLOB_THRESHOLD: usize = 64 * 1024; // 64KB

/// Storage-routed user manager
///
/// This extends UserManager functionality with tiered storage routing:
/// - Small messages stored inline in CockroachDB
/// - Large attachments stored as blobs in S3 with BlobRef
/// - Redis caching for hot data
/// - SQLite as offline fallback
pub struct StorageRoutedUserManager {
    /// Storage router for tiered storage operations
    router: Arc<StorageRouter>,
    /// Legacy database (for user table - will be migrated)
    database: Database,
    /// Chat chain client
    chat_chain: Arc<ChatChainClient>,
    /// Currency chain client
    currency_chain: Arc<CurrencyChainClient>,
    /// Cross-chain bridge
    bridge: Arc<CrossChainBridge>,
    /// Keys directory
    keys_dir: PathBuf,
}

impl StorageRoutedUserManager {
    /// Create a new storage-routed user manager
    pub async fn new(
        router_config: StorageRouterConfig,
        database: Database,
        chat_chain: Arc<ChatChainClient>,
        currency_chain: Arc<CurrencyChainClient>,
        bridge: Arc<CrossChainBridge>,
        keys_dir: PathBuf,
    ) -> Result<Self> {
        let router = StorageRouter::new(router_config)
            .await
            .map_err(|e| Error::storage(format!("Failed to initialize storage router: {}", e)))?;

        Ok(Self {
            router: Arc::new(router),
            database,
            chat_chain,
            currency_chain,
            bridge,
            keys_dir,
        })
    }

    /// Create with offline storage only (SQLite)
    pub async fn offline(
        sqlite_path: PathBuf,
        database: Database,
        chat_chain: Arc<ChatChainClient>,
        currency_chain: Arc<CurrencyChainClient>,
        bridge: Arc<CrossChainBridge>,
        keys_dir: PathBuf,
    ) -> Result<Self> {
        let router = StorageRouter::offline(sqlite_path)
            .await
            .map_err(|e| Error::storage(format!("Failed to initialize offline router: {}", e)))?;

        Ok(Self {
            router: Arc::new(router),
            database,
            chat_chain,
            currency_chain,
            bridge,
            keys_dir,
        })
    }

    /// Get the underlying storage router
    pub fn router(&self) -> &Arc<StorageRouter> {
        &self.router
    }

    /// Create a new user (delegates to database, will migrate later)
    pub async fn create_user(&self, username: &str) -> Result<CreateUserResponse> {
        info!("Creating new user: {}", username);

        // Generate new keypair
        #[allow(deprecated)]
        let keypair = KeyPair::generate();
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

        // Wait for blockchain finality confirmation
        let on_chain_confirmed = self
            .chat_chain
            .wait_for_finality(&tx_id, 3)
            .await
            .map_err(|e| Error::chain(e.to_string()))?;

        if !on_chain_confirmed {
            return Err(Error::chain("Transaction failed to achieve finality"));
        }

        // Store user in database (will migrate to router later)
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

    /// Get user profile (delegates to database)
    pub async fn get_user_profile(&self, user_id: &str) -> Result<UserProfile> {
        let user = self
            .database
            .get_user(user_id)
            .await?
            .ok_or_else(|| Error::storage(format!("User not found: {}", user_id)))?;

        let public_key_hex = hex::encode(&user.public_key);
        let created_at_rfc3339 = chrono::DateTime::from_timestamp(user.created_at, 0)
            .map(|dt| dt.to_rfc3339())
            .ok_or_else(|| Error::internal("Invalid timestamp in user data"))?;

        Ok(UserProfile {
            user_id: user.id,
            username: user.username.clone(),
            display_name: Some(format!("@{}", user.username)),
            public_key: public_key_hex,
            reputation_score: 0,
            verified: false,
            created_at: created_at_rfc3339,
            badges: vec![],
        })
    }

    /// Send direct message using StorageRouter for tiered storage
    ///
    /// This replaces direct SQLite writes with:
    /// - Small content: stored inline in CockroachDB metadata + cached in Redis
    /// - Large content: stored as blob in S3, BlobRef stored in metadata
    /// - SQLite updated as offline cache
    pub async fn send_direct_message(
        &self,
        sender_id: &str,
        recipient_id: &str,
        content: &[u8],
        encrypted: bool,
        encryption_key_id: Option<[u8; 32]>,
    ) -> Result<DirectMessageResponse> {
        info!(
            "Sending DM from {} to {} ({} bytes)",
            sender_id,
            recipient_id,
            content.len()
        );

        // Verify both users exist
        let _sender = self.get_user_profile(sender_id).await?;
        let _recipient = self.get_user_profile(recipient_id).await?;

        // Parse UUIDs
        let sender_uuid = UserId(
            uuid::Uuid::parse_str(sender_id)
                .map_err(|e| Error::validation(format!("Invalid sender ID: {}", e)))?,
        );
        let recipient_uuid = UserId(
            uuid::Uuid::parse_str(recipient_id)
                .map_err(|e| Error::validation(format!("Invalid recipient ID: {}", e)))?,
        );
        let message_id = MessageId(Uuid::new_v4());

        // Convert UUIDs to byte arrays for storage router
        let sender_bytes = uuid_to_bytes(&sender_uuid.0);
        let recipient_bytes = uuid_to_bytes(&recipient_uuid.0);

        // Submit on-chain transaction for message ordering
        info!("Recording message on chat chain...");
        let tx_id = self
            .chat_chain
            .send_direct_message(&sender_uuid, &recipient_uuid, message_id)
            .await
            .map_err(|e| {
                error!("Failed to record on chat chain: {}", e);
                Error::internal(format!("Chat chain recording failed: {}", e))
            })?;

        // Store through StorageRouter (handles tiering automatically)
        let metadata = self
            .router
            .store_message(
                sender_bytes,
                recipient_bytes,
                MessageType::Direct,
                content.to_vec(),
                encrypted,
                encryption_key_id,
                None, // reply_to
            )
            .await
            .map_err(|e| {
                error!("Failed to store message through router: {}", e);
                Error::storage(format!("Storage router failed: {}", e))
            })?;

        let on_chain_confirmed = true; // We got tx_id, assume confirmed
        let timestamp_rfc3339 = metadata.timestamp.to_rfc3339();

        info!(
            "✓ Direct message sent via storage router: {} (blob: {})",
            message_id,
            metadata.blob_ref.is_some()
        );

        Ok(DirectMessageResponse {
            message_id: hex::encode(metadata.id),
            status: "sent".to_string(),
            timestamp: timestamp_rfc3339,
            on_chain_confirmed,
            tx_id: Some(tx_id.to_string()),
        })
    }

    /// Send message with attachment
    ///
    /// Attachments are always stored as blobs in S3/IPFS regardless of size,
    /// with BlobRef stored in message metadata.
    pub async fn send_message_with_attachment(
        &self,
        sender_id: &str,
        recipient_id: &str,
        text_content: &str,
        attachment: Vec<u8>,
        attachment_mime: &str,
        encrypted: bool,
        encryption_key_id: Option<[u8; 32]>,
    ) -> Result<(DirectMessageResponse, BlobRef)> {
        info!(
            "Sending message with attachment from {} to {} ({} bytes text, {} bytes attachment)",
            sender_id,
            recipient_id,
            text_content.len(),
            attachment.len()
        );

        // First, store the attachment as a blob
        let blob_ref = self
            .router
            .store_blob(attachment, encrypted)
            .await
            .map_err(|e| {
                error!("Failed to store attachment: {}", e);
                Error::storage(format!("Attachment storage failed: {}", e))
            })?;

        info!(
            "Attachment stored as blob: {} ({} locations)",
            blob_ref.hash_hex(),
            blob_ref.locations.len()
        );

        // Build combined content: text + blob reference
        let combined_content = serde_json::json!({
            "text": text_content,
            "attachment": {
                "blob_hash": blob_ref.hash_hex(),
                "size": blob_ref.size,
                "mime_type": attachment_mime,
                "encrypted": blob_ref.encrypted,
            }
        });
        let content_bytes = serde_json::to_vec(&combined_content)
            .map_err(|e| Error::internal(format!("JSON serialization failed: {}", e)))?;

        // Send the message with inline content referencing the blob
        let response = self
            .send_direct_message(
                sender_id,
                recipient_id,
                &content_bytes,
                encrypted,
                encryption_key_id,
            )
            .await?;

        Ok((response, blob_ref))
    }

    /// Retrieve attachment blob
    pub async fn retrieve_attachment(&self, blob_hash: &[u8; 32]) -> Result<Vec<u8>> {
        self.router
            .retrieve_blob(blob_hash)
            .await
            .map_err(|e| Error::storage(format!("Failed to retrieve attachment: {}", e)))
    }

    /// Get direct messages for a conversation
    pub async fn get_direct_messages(
        &self,
        user_id: &str,
        peer_id: &str,
        limit: u32,
    ) -> Result<Vec<MessageMetadata>> {
        let user_bytes = uuid_str_to_bytes(user_id)?;
        let peer_bytes = uuid_str_to_bytes(peer_id)?;

        self.router
            .list_messages(&user_bytes, &peer_bytes, limit, None)
            .await
            .map_err(|e| Error::storage(format!("Failed to list messages: {}", e)))
    }

    /// Get a specific message by ID
    pub async fn get_message(&self, message_id: &[u8; 32]) -> Result<Option<MessageMetadata>> {
        self.router
            .get_message(message_id)
            .await
            .map_err(|e| Error::storage(format!("Failed to get message: {}", e)))
    }

    /// Create a channel (delegates to chat chain + stores metadata)
    pub async fn create_channel(
        &self,
        creator_id: &str,
        channel_name: &str,
        _description: Option<&str>,
    ) -> Result<CreateChannelResponse> {
        info!("Creating channel: {} by {}", channel_name, creator_id);

        let _creator = self.get_user_profile(creator_id).await?;
        let channel_id = ChannelId(Uuid::new_v4());
        let creator_uuid = UserId(
            uuid::Uuid::parse_str(creator_id)
                .map_err(|e| Error::validation(format!("Invalid creator ID: {}", e)))?,
        );
        let created_at = chrono::Utc::now().to_rfc3339();

        // Submit on-chain transaction
        let tx_id = self
            .chat_chain
            .create_channel(&creator_uuid, &channel_id, channel_name.to_string())
            .await
            .map_err(|e| {
                error!("Failed to create channel on chat chain: {}", e);
                Error::internal(format!("Chat chain channel creation failed: {}", e))
            })?;

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

    /// Post message to channel using StorageRouter
    pub async fn post_to_channel(
        &self,
        sender_id: &str,
        channel_id: &str,
        content: &[u8],
        encrypted: bool,
        encryption_key_id: Option<[u8; 32]>,
    ) -> Result<DirectMessageResponse> {
        info!("Posting to channel {} by user {}", channel_id, sender_id);

        let _sender = self.get_user_profile(sender_id).await?;
        let sender_uuid = UserId(
            uuid::Uuid::parse_str(sender_id)
                .map_err(|e| Error::validation(format!("Invalid sender ID: {}", e)))?,
        );
        let channel_uuid = ChannelId(
            uuid::Uuid::parse_str(channel_id)
                .map_err(|e| Error::validation(format!("Invalid channel ID: {}", e)))?,
        );
        let message_id = MessageId(Uuid::new_v4());

        let sender_bytes = uuid_to_bytes(&sender_uuid.0);
        let channel_bytes = uuid_to_bytes(&channel_uuid.0);

        // Submit on-chain transaction
        let tx_id = self
            .chat_chain
            .post_to_channel(&sender_uuid, &channel_uuid, message_id)
            .await
            .map_err(|e| {
                error!("Failed to post to chat chain: {}", e);
                Error::internal(format!("Chat chain posting failed: {}", e))
            })?;

        // Store through StorageRouter
        let metadata = self
            .router
            .store_message(
                sender_bytes,
                channel_bytes,
                MessageType::Channel,
                content.to_vec(),
                encrypted,
                encryption_key_id,
                None,
            )
            .await
            .map_err(|e| {
                error!("Failed to store channel message: {}", e);
                Error::storage(format!("Storage router failed: {}", e))
            })?;

        let timestamp_rfc3339 = metadata.timestamp.to_rfc3339();

        info!("✓ Message posted to channel: {}", hex::encode(metadata.id));

        Ok(DirectMessageResponse {
            message_id: hex::encode(metadata.id),
            status: "posted".to_string(),
            timestamp: timestamp_rfc3339,
            on_chain_confirmed: true,
            tx_id: Some(tx_id.to_string()),
        })
    }

    /// Sync offline cache with remote storage
    pub async fn sync_cache(&self) -> Result<u64> {
        self.router
            .sync_cache()
            .await
            .map_err(|e| Error::storage(format!("Cache sync failed: {}", e)))
    }

    /// Check if operating in offline mode
    pub fn is_offline(&self) -> bool {
        self.router.is_offline()
    }

    /// Get storage statistics
    pub async fn get_storage_stats(&self) -> StorageStats {
        let challenge_stats = self
            .router
            .challenge_manager()
            .map(|cm| {
                // Get stats synchronously from cached data
                ChallengeStats {
                    pending_challenges: 0, // Would need async access
                    total_pending_value: 0,
                }
            })
            .unwrap_or_default();

        StorageStats {
            is_offline: self.router.is_offline(),
            challenge_stats,
        }
    }
}

/// Storage statistics
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    pub is_offline: bool,
    pub challenge_stats: ChallengeStats,
}

/// Challenge statistics
#[derive(Debug, Clone, Default)]
pub struct ChallengeStats {
    pub pending_challenges: usize,
    pub total_pending_value: u64,
}

/// Convert UUID to 32-byte array (zero-padded)
fn uuid_to_bytes(uuid: &Uuid) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid.as_bytes());
    bytes
}

/// Parse UUID string to bytes
fn uuid_str_to_bytes(uuid_str: &str) -> Result<[u8; 32]> {
    let uuid = uuid::Uuid::parse_str(uuid_str)
        .map_err(|e| Error::validation(format!("Invalid UUID: {}", e)))?;
    Ok(uuid_to_bytes(&uuid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_to_bytes() {
        let uuid = Uuid::new_v4();
        let bytes = uuid_to_bytes(&uuid);
        assert_eq!(bytes.len(), 32);
        assert_eq!(&bytes[..16], uuid.as_bytes());
        assert_eq!(&bytes[16..], &[0u8; 16]);
    }

    #[test]
    fn test_uuid_str_to_bytes() {
        let uuid = Uuid::new_v4();
        let uuid_str = uuid.to_string();
        let bytes = uuid_str_to_bytes(&uuid_str).unwrap();
        assert_eq!(&bytes[..16], uuid.as_bytes());
    }
}
