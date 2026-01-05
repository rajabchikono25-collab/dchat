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
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use dchat_blockchain::{ChatChainClient, CrossChainBridge, CurrencyChainClient};
use dchat_core::error::{Error, Result};
use dchat_core::types::{ChannelId, MessageId, UserId};
use dchat_crypto::keys::KeyPair;
use dchat_identity::Identity;
use dchat_storage::provider::{
    BlobRef, MessageMetadata, MessageType, StorageRouter, StorageRouterConfig, StoredUser,
};
use dchat_storage::Database;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Size threshold for blob storage (content larger than this goes to S3)
const BLOB_THRESHOLD: usize = 64 * 1024; // 64KB

/// Minimum balance required for storage operations (in smallest token unit)
const MIN_STORAGE_BALANCE: u64 = 1_000_000; // 0.01 DCHAT (8 decimals)

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

        // Wait for blockchain finality confirmation
        let on_chain_confirmed = self
            .chat_chain
            .wait_for_finality(&tx_id, 3)
            .await
            .map_err(|e| Error::chain(e.to_string()))?;

        if !on_chain_confirmed {
            return Err(Error::chain("Transaction failed to achieve finality"));
        }

        // Convert user ID to 32-byte array for StorageRouter
        let user_id_bytes: [u8; 32] = {
            let mut bytes = [0u8; 32];
            let id_bytes = identity.user_id.as_bytes();
            bytes[..id_bytes.len().min(32)].copy_from_slice(&id_bytes[..id_bytes.len().min(32)]);
            bytes
        };

        // Create StoredUser record for tiered storage
        let stored_user = StoredUser {
            id: user_id_bytes,
            username: username.to_string(),
            public_key: public_key_bytes.to_vec(),
            created_at: chrono::Utc::now(),
            on_chain_confirmed,
            chain_tx_id: Some(tx_id.to_string()),
        };

        // Store user via StorageRouter (tiered: CockroachDB → SQLite → Redis)
        self.router.store_user(&stored_user).await.map_err(|e| {
            error!("Failed to store user via StorageRouter: {}", e);
            Error::storage(format!("Storage router error: {}", e))
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

    /// Get user profile (uses StorageRouter with tiered lookup)
    pub async fn get_user_profile(&self, user_id: &str) -> Result<UserProfile> {
        // Convert user_id string to 32-byte array
        let user_id_bytes: [u8; 32] = {
            let mut bytes = [0u8; 32];
            let id_bytes = user_id.as_bytes();
            bytes[..id_bytes.len().min(32)].copy_from_slice(&id_bytes[..id_bytes.len().min(32)]);
            bytes
        };

        // Use StorageRouter for tiered lookup: Redis → SQLite → CockroachDB
        let user = self
            .router
            .get_user(&user_id_bytes)
            .await
            .map_err(|e| Error::storage(format!("Failed to query user: {}", e)))?
            .ok_or_else(|| Error::storage(format!("User not found: {}", user_id)))?;

        let public_key_hex = hex::encode(&user.public_key);
        let created_at_rfc3339 = user.created_at.to_rfc3339();

        Ok(UserProfile {
            user_id: user_id.to_string(),
            username: user.username.clone(),
            display_name: Some(format!("@{}", user.username)),
            public_key: public_key_hex,
            reputation_score: 0,
            verified: user.on_chain_confirmed,
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
    ///
    /// Storage billing: Checks balance before storing and pays provider after upload.
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

        // Check storage balance before storing blob
        let balance = self.check_storage_balance(sender_id).await?;
        if !balance.has_sufficient_balance {
            return Err(Error::validation(format!(
                "Insufficient storage balance: {} < {} required. Please top up your account.",
                balance.available_balance, balance.minimum_required
            )));
        }

        // Calculate storage cost for this attachment (1 DCHAT per GB with 8 decimals)
        let attachment_size = attachment.len() as u64;
        let cost_per_gb: u64 = 1_0000_0000; // 1 DCHAT
        let storage_cost =
            ((attachment_size as f64 / (1024.0 * 1024.0 * 1024.0)) * cost_per_gb as f64) as u64;
        let storage_cost = storage_cost.max(1000); // Minimum 0.00001 DCHAT per blob

        if balance.available_balance < storage_cost {
            return Err(Error::validation(format!(
                "Insufficient balance for attachment: {} < {} required for {} bytes",
                balance.available_balance, storage_cost, attachment_size
            )));
        }

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

        // Pay storage provider for the blob
        if let Some(primary_location) = blob_ref.locations.first() {
            match self
                .pay_for_storage(sender_id, storage_cost, &primary_location.provider_id)
                .await
            {
                Ok(tx_id) => {
                    debug!(
                        "Storage payment submitted: {} for {} bytes (tx: {})",
                        storage_cost, attachment_size, tx_id
                    );
                }
                Err(e) => {
                    // Log warning but don't fail - the blob is already stored
                    // In production, this would trigger a retry queue or escrow
                    error!(
                        "Failed to pay storage provider: {}. Blob {} stored but unpaid.",
                        e,
                        blob_ref.hash_hex()
                    );
                }
            }
        }

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
            .map(|_cm| {
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

    // ==================== Currency Chain Integration ====================

    /// Check user's storage balance and ensure sufficient funds
    /// Uses currency_chain for balance verification
    pub async fn check_storage_balance(&self, user_id: &str) -> Result<StorageBalance> {
        let user_uuid = UserId(
            uuid::Uuid::parse_str(user_id)
                .map_err(|e| Error::validation(format!("Invalid user ID: {}", e)))?,
        );

        // Query balance from currency chain (sync method)
        let balance = self.currency_chain.get_balance(&user_uuid).map_err(|e| {
            error!("Failed to query balance: {}", e);
            Error::chain(format!("Balance query failed: {}", e))
        })?;

        // Calculate storage costs based on usage
        let storage_stats = self.get_storage_stats().await;
        let estimated_monthly_cost = self.estimate_monthly_storage_cost(user_id).await?;

        let has_sufficient_balance = balance >= MIN_STORAGE_BALANCE;
        let months_remaining = if estimated_monthly_cost > 0 {
            balance / estimated_monthly_cost
        } else {
            u64::MAX
        };

        debug!(
            "User {} balance: {} (min required: {}, monthly cost: {})",
            user_id, balance, MIN_STORAGE_BALANCE, estimated_monthly_cost
        );

        Ok(StorageBalance {
            available_balance: balance,
            minimum_required: MIN_STORAGE_BALANCE,
            estimated_monthly_cost,
            has_sufficient_balance,
            months_remaining,
            is_offline: storage_stats.is_offline,
        })
    }

    /// Estimate monthly storage cost based on current usage
    async fn estimate_monthly_storage_cost(&self, user_id: &str) -> Result<u64> {
        let user_bytes = uuid_str_to_bytes(user_id)?;

        // Get user's blob storage usage
        let blob_count = self.router.count_user_blobs(&user_bytes).await.unwrap_or(0);

        let total_blob_size = self
            .router
            .sum_user_blob_size(&user_bytes)
            .await
            .unwrap_or(0);

        // Estimate cost: 1 DCHAT per GB/month (8 decimals)
        let gb_used = (total_blob_size as f64) / (1024.0 * 1024.0 * 1024.0);
        let cost_per_gb: u64 = 1_0000_0000; // 1 DCHAT

        let estimated_cost = (gb_used * cost_per_gb as f64) as u64;

        debug!(
            "User {} storage: {} blobs, {} bytes, estimated cost: {}",
            user_id, blob_count, total_blob_size, estimated_cost
        );

        Ok(estimated_cost)
    }

    /// Pay for storage using currency chain
    /// This is called when storing large blobs to prepay for storage
    pub async fn pay_for_storage(
        &self,
        user_id: &str,
        amount: u64,
        provider_id: &[u8; 32],
    ) -> Result<String> {
        let user_uuid = UserId(
            uuid::Uuid::parse_str(user_id)
                .map_err(|e| Error::validation(format!("Invalid user ID: {}", e)))?,
        );

        // Verify balance first
        let balance = self.check_storage_balance(user_id).await?;
        if !balance.has_sufficient_balance {
            return Err(Error::validation(format!(
                "Insufficient balance: {} < {}",
                balance.available_balance, amount
            )));
        }

        // Convert provider ID to UserId (use first 16 bytes of provider_id for UUID)
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&provider_id[..16]);
        let provider_user_uuid = UserId(uuid::Uuid::from_bytes(uuid_bytes));

        // Submit payment transaction via currency chain with limited retries
        let retry_delays = [
            Duration::from_millis(200),
            Duration::from_millis(500),
            Duration::from_millis(1000),
        ];
        let mut last_err: Option<Error> = None;
        let tx_id = {
            let mut result = None;
            for (idx, delay) in retry_delays.iter().enumerate() {
                match self
                    .currency_chain
                    .transfer(&user_uuid, &provider_user_uuid, amount)
                {
                    Ok(tx) => {
                        result = Some(tx);
                        break;
                    }
                    Err(e) => {
                        let wrapped = Error::chain(format!("Payment transaction failed: {}", e));
                        error!("Storage payment attempt {} failed: {}", idx + 1, wrapped);
                        last_err = Some(wrapped);
                        if idx + 1 < retry_delays.len() {
                            tokio::time::sleep(*delay).await;
                        }
                    }
                }
            }
            result.ok_or_else(|| {
                last_err.unwrap_or_else(|| Error::chain("Payment transaction failed".to_string()))
            })?
        };

        info!(
            "Storage payment of {} submitted by {} to provider {} (tx: {})",
            amount,
            user_id,
            hex::encode(provider_id),
            tx_id
        );

        Ok(tx_id.to_string())
    }

    // ==================== Cross-Chain Bridge Integration ====================

    /// Sync user's storage state across chains
    /// Uses bridge for cross-chain state synchronization
    pub async fn sync_cross_chain_state(&self, user_id: &str) -> Result<CrossChainSyncResult> {
        let user_uuid = UserId(
            uuid::Uuid::parse_str(user_id)
                .map_err(|e| Error::validation(format!("Invalid user ID: {}", e)))?,
        );

        // Get current block heights from each chain
        let chat_chain_height = self.chat_chain.get_current_height().await.unwrap_or(0);

        let currency_chain_height = self.currency_chain.get_current_height().await.unwrap_or(0);

        // Get user's transactions to determine state
        let chat_txs = self
            .chat_chain
            .get_user_transactions(&user_uuid)
            .map_err(|e| Error::chain(format!("Failed to get chat chain txs: {}", e)))?;

        let currency_txs = self
            .currency_chain
            .get_user_transactions(&user_uuid)
            .map_err(|e| Error::chain(format!("Failed to get currency chain txs: {}", e)))?;

        // Check for pending cross-chain transactions via bridge
        let pending_bridge_txs = self
            .bridge
            .get_user_transactions(&user_uuid)
            .map_err(|e| Error::chain(format!("Failed to get bridge transactions: {}", e)))?;
        let needs_sync = pending_bridge_txs
            .iter()
            .any(|tx| tx.status == dchat_blockchain::CrossChainStatus::Pending);

        let sync_tx_id = if needs_sync {
            info!(
                "Cross-chain state mismatch for user {}, finalizing pending transactions",
                user_id
            );

            // Finalize any pending transactions
            self.bridge.finalize_pending_transactions().map_err(|e| {
                error!("Cross-chain sync failed: {}", e);
                Error::chain(format!("Bridge sync failed: {}", e))
            })?;

            // Return ID of the first pending transaction we finalized
            pending_bridge_txs.first().map(|tx| tx.id.to_string())
        } else {
            None
        };

        // Check if transactions are finalized
        let chat_finalized = chat_txs
            .iter()
            .all(|tx| matches!(tx.status, dchat_chain::TransactionStatus::Confirmed { .. }));

        let currency_finalized = currency_txs.iter().all(|tx| tx.status == "confirmed");

        Ok(CrossChainSyncResult {
            chat_chain_block: chat_chain_height,
            currency_chain_block: currency_chain_height,
            was_synchronized: needs_sync,
            sync_tx_id,
            chat_chain_finalized: chat_finalized,
            currency_chain_finalized: currency_finalized,
        })
    }

    /// Initiate cross-chain atomic transfer for storage bond
    pub async fn create_storage_bond(
        &self,
        user_id: &str,
        bond_amount: u64,
        duration_days: u32,
    ) -> Result<StorageBondResult> {
        let user_uuid = UserId(
            uuid::Uuid::parse_str(user_id)
                .map_err(|e| Error::validation(format!("Invalid user ID: {}", e)))?,
        );

        // Check balance first
        let balance = self.check_storage_balance(user_id).await?;
        if balance.available_balance < bond_amount {
            return Err(Error::validation(format!(
                "Insufficient balance for bond: {} < {}",
                balance.available_balance, bond_amount
            )));
        }

        // Generate bond ID and user key
        let bond_id: [u8; 32] = {
            let mut id = [0u8; 32];
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(user_id.as_bytes());
            hasher.update(&bond_amount.to_le_bytes());
            hasher.update(&chrono::Utc::now().timestamp().to_le_bytes());
            id.copy_from_slice(&hasher.finalize());
            id
        };

        let user_key: [u8; 32] = {
            let mut key = [0u8; 32];
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(b"dchat-user-key:");
            hasher.update(user_id.as_bytes());
            key.copy_from_slice(&hasher.finalize());
            key
        };

        // Calculate storage bytes based on bond amount (1 DCHAT = 1 GB)
        let storage_bytes = bond_amount * 1024 * 1024 * 1024 / 100_000_000; // 8 decimals

        // Create storage bond via currency chain (sync method)
        let bond_result = self
            .currency_chain
            .create_storage_bond(
                &user_uuid,
                bond_id,
                user_key,
                bond_amount,
                storage_bytes,
                duration_days,
            )
            .map_err(|e| {
                error!("Storage bond creation failed: {}", e);
                Error::chain(format!("Bond creation failed: {}", e))
            })?;

        // Register bond on chat chain for storage rights via bridge transaction
        let bridge_tx_id = self
            .bridge
            .register_user_with_stake(&user_uuid, user_key.to_vec(), bond_amount)
            .await
            .map_err(|e| Error::chain(format!("Chat chain bond registration failed: {}", e)))?;

        info!(
            "Storage bond created for user {}: {} tokens for {} days (bond_id: {}, tx: {})",
            user_id,
            bond_amount,
            duration_days,
            hex::encode(&bond_id),
            bond_result.tx_id
        );

        Ok(StorageBondResult {
            bond_id: hex::encode(&bond_id),
            amount: bond_amount,
            duration_days,
            chat_chain_tx: bridge_tx_id.to_string(),
            expires_at: chrono::Utc::now() + chrono::Duration::days(duration_days as i64),
        })
    }

    // ==================== Key Management Integration ====================

    /// Save user's encryption key to keys directory
    /// Uses keys_dir for secure key storage
    pub async fn save_encryption_key(
        &self,
        user_id: &str,
        key_id: &str,
        key_material: &[u8],
    ) -> Result<()> {
        // Create keys directory if needed
        let user_key_dir = self.keys_dir.join(user_id);
        fs::create_dir_all(&user_key_dir).map_err(|e| {
            error!("Failed to create key directory: {}", e);
            Error::storage(format!("Key directory creation failed: {}", e))
        })?;

        // Hash the key material for filename (don't expose raw key ID)
        let mut hasher = Sha256::new();
        hasher.update(key_id.as_bytes());
        let key_hash = hex::encode(&hasher.finalize()[..8]);

        let key_path = user_key_dir.join(format!("{}.key", key_hash));

        // Encrypt key material using ChaCha20-Poly1305 AEAD with derived key
        let encrypted_key = self.encrypt_key_material(user_id, key_material)?;

        fs::write(&key_path, &encrypted_key).map_err(|e| {
            error!("Failed to write key file: {}", e);
            Error::storage(format!("Key file write failed: {}", e))
        })?;

        info!(
            "Saved encryption key {} for user {} to {}",
            key_id,
            user_id,
            key_path.display()
        );

        Ok(())
    }

    /// Load user's encryption key from keys directory
    pub async fn load_encryption_key(&self, user_id: &str, key_id: &str) -> Result<Vec<u8>> {
        let mut hasher = Sha256::new();
        hasher.update(key_id.as_bytes());
        let key_hash = hex::encode(&hasher.finalize()[..8]);

        let key_path = self
            .keys_dir
            .join(user_id)
            .join(format!("{}.key", key_hash));

        if !key_path.exists() {
            return Err(Error::NotFound(format!(
                "Key {} not found for user {}",
                key_id, user_id
            )));
        }

        let encrypted_key = fs::read(&key_path).map_err(|e| {
            error!("Failed to read key file: {}", e);
            Error::storage(format!("Key file read failed: {}", e))
        })?;

        // Decrypt key material
        let key_material = self.decrypt_key_material(user_id, &encrypted_key)?;

        debug!("Loaded encryption key {} for user {}", key_id, user_id);

        Ok(key_material)
    }

    /// List all encryption keys for a user
    pub async fn list_encryption_keys(&self, user_id: &str) -> Result<Vec<String>> {
        let user_key_dir = self.keys_dir.join(user_id);

        if !user_key_dir.exists() {
            return Ok(vec![]);
        }

        let mut keys = Vec::new();
        let entries = fs::read_dir(&user_key_dir)
            .map_err(|e| Error::storage(format!("Failed to read key directory: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| Error::storage(e.to_string()))?;
            let path = entry.path();
            if path.extension().map(|e| e == "key").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    keys.push(stem.to_string_lossy().to_string());
                }
            }
        }

        Ok(keys)
    }

    /// Delete an encryption key
    pub async fn delete_encryption_key(&self, user_id: &str, key_id: &str) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(key_id.as_bytes());
        let key_hash = hex::encode(&hasher.finalize()[..8]);

        let key_path = self
            .keys_dir
            .join(user_id)
            .join(format!("{}.key", key_hash));

        if key_path.exists() {
            fs::remove_file(&key_path)
                .map_err(|e| Error::storage(format!("Failed to delete key file: {}", e)))?;
            info!("Deleted encryption key {} for user {}", key_id, user_id);
        }

        Ok(())
    }

    /// Encrypt key material using user-derived key with ChaCha20-Poly1305 AEAD
    /// Format v1: [0x01][16-byte salt][12-byte nonce][ciphertext+tag]
    /// Legacy fallback (v0): [12-byte nonce][ciphertext+tag] using static salt
    fn encrypt_key_material(&self, user_id: &str, key_material: &[u8]) -> Result<Vec<u8>> {
        // Generate per-record salt to prevent rainbow-table style reuse across users/keys
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);

        // Derive 32-byte key = SHA256("dchat-key-encryption-v2:" || user_id || salt)
        let mut hasher = Sha256::new();
        hasher.update(b"dchat-key-encryption-v2:");
        hasher.update(user_id.as_bytes());
        hasher.update(&salt);
        let derived_key: [u8; 32] = hasher.finalize().into();

        // Create cipher with derived key
        let cipher = ChaCha20Poly1305::new_from_slice(&derived_key)
            .map_err(|e| Error::internal(format!("Failed to create cipher: {}", e)))?;

        // Generate random 12-byte nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt with AEAD (includes authentication tag)
        let ciphertext = cipher
            .encrypt(nonce, key_material)
            .map_err(|e| Error::internal(format!("Encryption failed: {}", e)))?;

        // Build v1 envelope: [version][salt][nonce][ciphertext]
        let mut encrypted =
            Vec::with_capacity(1 + salt.len() + nonce_bytes.len() + ciphertext.len());
        encrypted.push(1);
        encrypted.extend_from_slice(&salt);
        encrypted.extend_from_slice(&nonce_bytes);
        encrypted.extend_from_slice(&ciphertext);

        Ok(encrypted)
    }

    /// Decrypt key material using ChaCha20-Poly1305 AEAD with backward compatibility
    /// Supports v1 envelopes (preferred) and legacy v0 format for existing files.
    fn decrypt_key_material(&self, user_id: &str, encrypted: &[u8]) -> Result<Vec<u8>> {
        // v1 format requires 1 + 16 + 12 + 16(tag) + 1(data) minimum
        const MIN_V1_LEN: usize = 1 + 16 + 12 + 16 + 1;
        const MIN_V0_LEN: usize = 12 + 16 + 1;

        if encrypted.len() < MIN_V0_LEN {
            return Err(Error::internal("Encrypted data too short"));
        }

        let (derived_key, nonce_slice, ciphertext) = if encrypted.first() == Some(&1) {
            if encrypted.len() < MIN_V1_LEN {
                return Err(Error::internal("Encrypted data too short (v1)"));
            }

            let salt = &encrypted[1..1 + 16];
            let nonce_start = 1 + 16;
            let nonce_end = nonce_start + 12;
            let nonce = &encrypted[nonce_start..nonce_end];
            let ciphertext = &encrypted[nonce_end..];

            let mut hasher = Sha256::new();
            hasher.update(b"dchat-key-encryption-v2:");
            hasher.update(user_id.as_bytes());
            hasher.update(salt);
            let derived_key: [u8; 32] = hasher.finalize().into();

            (derived_key, nonce, ciphertext)
        } else {
            // Legacy v0: derive using static salt to maintain backward compatibility
            if encrypted.len() < MIN_V0_LEN {
                return Err(Error::internal("Encrypted data too short (legacy)"));
            }

            let nonce = &encrypted[..12];
            let ciphertext = &encrypted[12..];

            let mut hasher = Sha256::new();
            hasher.update(b"dchat-key-encryption-v2:");
            hasher.update(user_id.as_bytes());
            hasher.update(b"\x00dchat-salt-2025\x00");
            let derived_key: [u8; 32] = hasher.finalize().into();

            (derived_key, nonce, ciphertext)
        };

        // Create cipher
        let cipher = ChaCha20Poly1305::new_from_slice(&derived_key)
            .map_err(|e| Error::internal(format!("Failed to create cipher: {}", e)))?;

        // Decrypt and verify authentication tag
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_slice), ciphertext)
            .map_err(|e| Error::internal(format!("Decryption failed (tampering?): {}", e)))?;

        Ok(plaintext)
    }

    // ==================== Blob Threshold Integration ====================

    /// Determine optimal storage tier based on content size
    /// Uses BLOB_THRESHOLD to decide between inline and blob storage
    pub fn determine_storage_tier(&self, content_size: usize) -> StorageTier {
        if content_size <= BLOB_THRESHOLD {
            StorageTier::Inline
        } else {
            StorageTier::Blob
        }
    }

    /// Store content with automatic tier selection
    pub async fn store_content_auto_tier(
        &self,
        sender_id: &str,
        recipient_id: &str,
        content: &[u8],
        encrypted: bool,
    ) -> Result<StorageResult> {
        let tier = self.determine_storage_tier(content.len());

        debug!(
            "Content size {} bytes -> tier {:?} (threshold: {})",
            content.len(),
            tier,
            BLOB_THRESHOLD
        );

        match tier {
            StorageTier::Inline => {
                // Store inline in message metadata
                let response = self
                    .send_direct_message(sender_id, recipient_id, content, encrypted, None)
                    .await?;

                Ok(StorageResult {
                    tier,
                    message_id: response.message_id,
                    blob_ref: None,
                    storage_cost: 0, // Inline storage is included in base fee
                })
            }
            StorageTier::Blob => {
                // Store as blob and send reference
                let blob_ref = self
                    .router
                    .store_blob(content.to_vec(), encrypted)
                    .await
                    .map_err(|e| Error::storage(format!("Blob storage failed: {}", e)))?;

                // Estimate and check cost
                let storage_cost = (content.len() as u64) / (1024 * 1024) * 1_000_000; // ~0.01 DCHAT per MB

                // Send reference as message
                let reference_content = serde_json::json!({
                    "type": "blob_reference",
                    "blob_hash": blob_ref.hash_hex(),
                    "size": blob_ref.size,
                    "locations": blob_ref.locations.len(),
                })
                .to_string();

                let response = self
                    .send_direct_message(
                        sender_id,
                        recipient_id,
                        reference_content.as_bytes(),
                        false,
                        None,
                    )
                    .await?;

                Ok(StorageResult {
                    tier,
                    message_id: response.message_id,
                    blob_ref: Some(blob_ref),
                    storage_cost,
                })
            }
        }
    }

    /// Get content by message ID, automatically handling tier retrieval
    pub async fn get_content_auto_tier(&self, message_id: &[u8; 32]) -> Result<Vec<u8>> {
        let metadata = self
            .router
            .get_message(message_id)
            .await
            .map_err(|e| Error::storage(format!("Message lookup failed: {}", e)))?
            .ok_or_else(|| Error::NotFound("Message not found".to_string()))?;

        match &metadata.blob_ref {
            Some(blob_ref) => {
                // Content stored as blob, retrieve from provider
                self.router
                    .retrieve_blob(&blob_ref.hash)
                    .await
                    .map_err(|e| Error::storage(format!("Blob retrieval failed: {}", e)))
            }
            None => {
                // Content stored inline
                metadata
                    .inline_content
                    .ok_or_else(|| Error::storage("Message has no content"))
            }
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

/// Storage balance information
#[derive(Debug, Clone)]
pub struct StorageBalance {
    /// Available balance in smallest token unit
    pub available_balance: u64,
    /// Minimum required balance for operations
    pub minimum_required: u64,
    /// Estimated monthly storage cost
    pub estimated_monthly_cost: u64,
    /// Whether balance is sufficient
    pub has_sufficient_balance: bool,
    /// Estimated months of storage remaining
    pub months_remaining: u64,
    /// Whether currently in offline mode
    pub is_offline: bool,
}

/// Cross-chain synchronization result
#[derive(Debug, Clone)]
pub struct CrossChainSyncResult {
    /// Last block on chat chain
    pub chat_chain_block: u64,
    /// Last block on currency chain
    pub currency_chain_block: u64,
    /// Whether sync was needed and performed
    pub was_synchronized: bool,
    /// Transaction ID if sync occurred
    pub sync_tx_id: Option<String>,
    /// Chat chain finality status
    pub chat_chain_finalized: bool,
    /// Currency chain finality status
    pub currency_chain_finalized: bool,
}

/// Storage bond creation result
#[derive(Debug, Clone)]
pub struct StorageBondResult {
    /// Bond identifier
    pub bond_id: String,
    /// Bonded amount
    pub amount: u64,
    /// Duration in days
    pub duration_days: u32,
    /// Chat chain transaction ID
    pub chat_chain_tx: String,
    /// Expiration timestamp
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Storage tier for content
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StorageTier {
    /// Content stored inline in message metadata (< BLOB_THRESHOLD)
    Inline,
    /// Content stored as blob in S3/IPFS (>= BLOB_THRESHOLD)
    Blob,
}

/// Result of auto-tier storage operation
#[derive(Debug, Clone)]
pub struct StorageResult {
    /// Storage tier used
    pub tier: StorageTier,
    /// Message ID for the stored content
    pub message_id: String,
    /// Blob reference if stored as blob
    pub blob_ref: Option<BlobRef>,
    /// Storage cost in smallest token unit
    pub storage_cost: u64,
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
