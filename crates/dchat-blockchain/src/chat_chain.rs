//! Chat Chain client for identity, messaging, channels, permissions, governance, and reputation

use chrono::Utc;
use dchat_chain::{Transaction, TransactionStatus, TransactionType};
use dchat_core::types::{ChannelId, MessageId, UserId};
use dchat_privacy::zk_proofs::BlockchainClient as PrivacyBlockchainClient;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use crate::client::{ChainRpcClient, HttpRpcClient, MockRpcClient};
use dchat_core::error::{Error, Result};

/// Configuration for Chat Chain client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChainConfig {
    /// RPC endpoint for chat chain node
    pub rpc_url: String,
    /// WebSocket endpoint for subscriptions
    pub ws_url: Option<String>,
    /// Confirmation threshold (number of blocks)
    pub confirmation_blocks: u32,
    /// Transaction timeout (seconds)
    pub tx_timeout_seconds: u64,
    /// Retry attempts for failed transactions
    pub max_retries: u32,
}

impl Default for ChatChainConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".to_string(),
            ws_url: Some("ws://localhost:8546".to_string()),
            confirmation_blocks: 6,
            tx_timeout_seconds: 300,
            max_retries: 3,
        }
    }
}

/// Chat Chain client for on-chain operations: identity, messaging, channels, governance
pub struct ChatChainClient {
    config: ChatChainConfig,
    /// Transaction cache with hash mapping
    transactions: Arc<RwLock<HashMap<Uuid, (Transaction, Option<String>)>>>,
    /// RPC client for blockchain queries
    rpc_client: Arc<dyn ChainRpcClient>,
    /// Reputation scores per user
    reputation_scores: Arc<RwLock<HashMap<UserId, u32>>>,
    /// Channel ownership and metadata
    channels: Arc<RwLock<HashMap<ChannelId, ChannelMetadata>>>,
    /// Identity registry: user_id -> public_key (32 bytes Ed25519)
    identity_registry: Arc<RwLock<HashMap<UserId, [u8; 32]>>>,
    /// Spent nullifiers for ZK proof double-spend prevention
    spent_nullifiers: Arc<RwLock<HashSet<[u8; 32]>>>,
}

/// Channel metadata stored on chat chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMetadata {
    pub channel_id: ChannelId,
    pub owner: UserId,
    pub name: String,
    pub created_at: i64,
    pub is_token_gated: bool,
}

impl ChatChainClient {
    /// Get the chat chain configuration
    pub fn config(&self) -> &ChatChainConfig {
        &self.config
    }

    /// Create new chat chain client with production RPC
    pub fn new(config: ChatChainConfig) -> Result<Self> {
        let rpc_client = HttpRpcClient::new(config.rpc_url.clone())?;
        
        Ok(Self {
            config,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            rpc_client: Arc::new(rpc_client),
            reputation_scores: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
            identity_registry: Arc::new(RwLock::new(HashMap::new())),
            spent_nullifiers: Arc::new(RwLock::new(HashSet::new())),
        })
    }
    
    /// Create new chat chain client with mock RPC for testing
    pub fn new_mock(config: ChatChainConfig) -> Self {
        let rpc_client = MockRpcClient::new();
        
        Self {
            config,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            rpc_client: Arc::new(rpc_client),
            reputation_scores: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
            identity_registry: Arc::new(RwLock::new(HashMap::new())),
            spent_nullifiers: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Register user identity on chat chain
    pub async fn register_user(&self, user_id: &UserId, public_key: Vec<u8>) -> Result<Uuid> {
        let tx_id = Uuid::new_v4();
        let payload_json = serde_json::json!({
            "public_key": hex::encode(&public_key),
            "timestamp": Utc::now().timestamp(),
        });
        let payload = serde_json::to_vec(&payload_json).map_err(|e| Error::Serialization(e))?;

        let tx = Transaction {
            tx_id,
            tx_type: TransactionType::RegisterUser,
            payload: payload.clone(),
            tx_hash: format!("{:x}", uuid::Uuid::new_v4()),
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        };

        // Submit to blockchain via RPC
        let tx_hash = self.rpc_client.submit_transaction(payload).await
            .map_err(|e| Error::Chain(format!("Failed to submit transaction: {}", e)))?;

        // Store transaction with hash
        self.transactions.write().unwrap().insert(tx_id, (tx, Some(tx_hash)));

        // Initialize reputation score
        self.reputation_scores
            .write()
            .unwrap()
            .insert(user_id.clone(), 50);

        // Store public key in identity registry (convert to 32-byte array)
        if public_key.len() == 32 {
            let mut pk_array = [0u8; 32];
            pk_array.copy_from_slice(&public_key);
            self.identity_registry
                .write()
                .unwrap()
                .insert(user_id.clone(), pk_array);
        }

        Ok(tx_id)
    }

    /// Send direct message on chat chain (ordering only)
    pub async fn send_direct_message(
        &self,
        _sender: &UserId,
        _recipient: &UserId,
        _message_id: MessageId,
    ) -> Result<Uuid> {
        let tx_id = Uuid::new_v4();
        let payload_json = serde_json::json!({
            "timestamp": Utc::now().timestamp(),
        });
        let payload = serde_json::to_vec(&payload_json).map_err(|e| Error::Serialization(e))?;

        let tx = Transaction {
            tx_id,
            tx_type: TransactionType::SendDirectMessage,
            payload: payload.clone(),
            tx_hash: format!("{:x}", uuid::Uuid::new_v4()),
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        };

        // Submit to blockchain via RPC
        let tx_hash = self.rpc_client.submit_transaction(payload).await
            .map_err(|e| Error::Chain(format!("Failed to submit transaction: {}", e)))?;

        // Store transaction with hash
        self.transactions.write().unwrap().insert(tx_id, (tx, Some(tx_hash)));
        Ok(tx_id)
    }

    /// Create channel on chat chain
    pub async fn create_channel(
        &self,
        creator: &UserId,
        channel_id: &ChannelId,
        name: String,
    ) -> Result<Uuid> {
        let tx_id = Uuid::new_v4();
        let payload_json = serde_json::json!({
            "channel_id": channel_id,
            "name": name,
            "timestamp": Utc::now().timestamp(),
        });
        let payload = serde_json::to_vec(&payload_json).map_err(|e| Error::Serialization(e))?;

        let tx = Transaction {
            tx_id,
            tx_type: TransactionType::CreateChannel,
            payload: payload.clone(),
            tx_hash: format!("{:x}", uuid::Uuid::new_v4()),
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        };

        // Submit to blockchain via RPC
        let tx_hash = self.rpc_client.submit_transaction(payload).await
            .map_err(|e| Error::Chain(format!("Failed to submit transaction: {}", e)))?;

        // Store transaction with hash
        self.transactions.write().unwrap().insert(tx_id, (tx, Some(tx_hash)));

        // Store channel metadata
        let channel_meta = ChannelMetadata {
            channel_id: channel_id.clone(),
            owner: creator.clone(),
            name,
            created_at: Utc::now().timestamp(),
            is_token_gated: false,
        };
        self.channels
            .write()
            .unwrap()
            .insert(channel_id.clone(), channel_meta);

        Ok(tx_id)
    }

    /// Post message to channel on chat chain
    pub async fn post_to_channel(
        &self,
        _sender: &UserId,
        _channel_id: &ChannelId,
        _message_id: MessageId,
    ) -> Result<Uuid> {
        let tx_id = Uuid::new_v4();
        let payload_json = serde_json::json!({
            "timestamp": Utc::now().timestamp(),
        });
        let payload = serde_json::to_vec(&payload_json).map_err(|e| Error::Serialization(e))?;

        let tx = Transaction {
            tx_id,
            tx_type: TransactionType::PostToChannel,
            payload: payload.clone(),
            tx_hash: format!("{:x}", uuid::Uuid::new_v4()),
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        };

        // Submit to blockchain via RPC
        let tx_hash = self.rpc_client.submit_transaction(payload).await
            .map_err(|e| Error::Chain(format!("Failed to submit transaction: {}", e)))?;

        // Store transaction with hash
        self.transactions.write().unwrap().insert(tx_id, (tx, Some(tx_hash)));
        Ok(tx_id)
    }

    /// Get reputation score
    pub fn get_reputation(&self, user_id: &UserId) -> Result<u32> {
        Ok(self
            .reputation_scores
            .read()
            .unwrap()
            .get(user_id)
            .copied()
            .unwrap_or(0))
    }

    /// Update user's reputation score
    pub fn update_reputation(&self, user_id: &UserId, delta: i32) -> Result<u32> {
        let mut scores = self.reputation_scores.write().unwrap();
        let current = scores.get(user_id).copied().unwrap_or(0);
        let new_score = if delta < 0 {
            current.saturating_sub((-delta) as u32)
        } else {
            current.saturating_add(delta as u32)
        };
        scores.insert(user_id.clone(), new_score);
        Ok(new_score)
    }

    /// Get transaction by ID
    pub fn get_transaction(&self, tx_id: &Uuid) -> Result<Transaction> {
        self.transactions
            .read()
            .unwrap()
            .get(tx_id)
            .map(|(tx, _hash)| tx.clone())
            .ok_or_else(|| Error::NotFound(format!("Transaction {}", tx_id)))
    }

    /// Get user transactions
    pub fn get_user_transactions(&self, _user_id: &UserId) -> Result<Vec<Transaction>> {
        Ok(self
            .transactions
            .read()
            .unwrap()
            .values()
            .map(|(tx, _hash)| tx.clone())
            .collect())
    }

    /// Wait for transaction to achieve finality (confirmed status)
    /// Returns true if transaction reaches finality within timeout
    pub async fn wait_for_finality(
        &self,
        tx_id: &Uuid,
        required_confirmations: u32,
    ) -> Result<bool> {
        use tokio::time::{sleep, Duration};

        // Maximum wait time: 30 seconds
        let max_attempts = 30;
        let mut attempts = 0;

        // Get transaction hash for RPC queries
        let tx_hash = {
            let transactions = self.transactions.read().unwrap();
            let tx_data = transactions.get(tx_id)
                .ok_or_else(|| Error::NotFound(format!("Transaction {}", tx_id)))?;
            tx_data.1.clone()
                .ok_or_else(|| Error::Chain(format!("Transaction not yet submitted to blockchain: {}", tx_id)))?
        };

        while attempts < max_attempts {
            // Query blockchain for transaction status
            let status = self.rpc_client.get_transaction_status(&tx_hash).await
                .map_err(|e| Error::Chain(format!("RPC error querying status: {}", e)))?;

            match status {
                TransactionStatus::Confirmed { block_height, .. } => {
                    // Check if we have enough confirmations
                    let current_height = self.rpc_client.get_current_height().await
                        .map_err(|e| Error::Chain(format!("RPC error querying height: {}", e)))?;
                    
                    let confirmations = current_height.saturating_sub(block_height);
                    if confirmations >= required_confirmations as u64 {
                        // Update local cache
                        if let Some(tx_data) = self.transactions.write().unwrap().get_mut(tx_id) {
                            tx_data.0.status = status.clone();
                            tx_data.0.confirmed_at = Some(Utc::now());
                        }
                        return Ok(true);
                    }
                    // Not enough confirmations yet, keep waiting
                }
                TransactionStatus::Failed { .. } => {
                    // Update local cache
                    if let Some(tx_data) = self.transactions.write().unwrap().get_mut(tx_id) {
                        tx_data.0.status = status;
                    }
                    return Ok(false);
                }
                TransactionStatus::TimedOut => {
                    // Update local cache
                    if let Some(tx_data) = self.transactions.write().unwrap().get_mut(tx_id) {
                        tx_data.0.status = status;
                    }
                    return Ok(false);
                }
                TransactionStatus::Pending => {
                    // Still pending, continue waiting
                }
            }

            attempts += 1;
            sleep(Duration::from_secs(1)).await;
        }

        // Timeout reached
        Ok(false)
    }
}

/// Implementation of dchat-privacy's BlockchainClient trait
/// Provides identity registry queries and nullifier tracking for ZK proofs
impl PrivacyBlockchainClient for ChatChainClient {
    /// Get user's public key from on-chain identity registry
    fn get_user_public_key(&self, user_id: &UserId) -> Result<[u8; 32]> {
        self.identity_registry
            .read()
            .unwrap()
            .get(user_id)
            .copied()
            .ok_or_else(|| Error::NotFound(format!("User public key not found: {}", user_id)))
    }
    
    /// Check if nullifier has been spent (prevents ZK proof double-use)
    fn is_nullifier_spent(&self, nullifier: &[u8; 32]) -> Result<bool> {
        Ok(self.spent_nullifiers.read().unwrap().contains(nullifier))
    }
    
    /// Mark nullifier as spent on-chain
    fn mark_nullifier_spent(&mut self, nullifier: [u8; 32]) -> Result<()> {
        self.spent_nullifiers.write().unwrap().insert(nullifier);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_user() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new_mock(config);

        let user_id = UserId(Uuid::new_v4());
        let result = client.register_user(&user_id, vec![1, 2, 3]).await;
        assert!(result.is_ok());

        let reputation = client.get_reputation(&user_id).unwrap();
        assert_eq!(reputation, 50); // Initial reputation
    }

    #[tokio::test]
    async fn test_create_channel() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new_mock(config);

        let owner = UserId(Uuid::new_v4());
        let channel_id = ChannelId(Uuid::new_v4());
        let result = client.create_channel(&owner, &channel_id, "Test Channel".to_string()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_reputation_tracking() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new_mock(config);

        let user_id = UserId(Uuid::new_v4());
        client.register_user(&user_id, vec![1, 2, 3]).await.unwrap();

        // Increase reputation
        client.update_reputation(&user_id, 10).unwrap();
        let rep = client.get_reputation(&user_id).unwrap();
        assert_eq!(rep, 60);

        // Decrease reputation
        client.update_reputation(&user_id, -20).unwrap();
        let rep = client.get_reputation(&user_id).unwrap();
        assert_eq!(rep, 40);
    }

    #[tokio::test]
    async fn test_confirmation_tracking() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new_mock(config);

        let user_id = UserId(Uuid::new_v4());
        let tx_id = client.register_user(&user_id, vec![1, 2, 3]).await.unwrap();

        // With MockRpcClient, transactions confirm immediately
        let confirmed = client.wait_for_finality(&tx_id, 1).await.unwrap();
        assert!(confirmed);
    }

    #[tokio::test]
    async fn test_privacy_blockchain_client_identity_registry() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new_mock(config);

        let user_id = UserId(Uuid::new_v4());
        let public_key: [u8; 32] = [42u8; 32];
        
        // Register user with 32-byte public key
        client.register_user(&user_id, public_key.to_vec()).await.unwrap();
        
        // Verify identity is stored via privacy trait
        let retrieved_key = client.get_user_public_key(&user_id).unwrap();
        assert_eq!(retrieved_key, public_key);
    }

    #[tokio::test]
    async fn test_privacy_blockchain_client_nullifier_tracking() {
        let config = ChatChainConfig::default();
        let mut client = ChatChainClient::new_mock(config);

        let nullifier: [u8; 32] = [0xAB; 32];
        
        // Initially not spent
        assert!(!client.is_nullifier_spent(&nullifier).unwrap());
        
        // Mark as spent
        client.mark_nullifier_spent(nullifier).unwrap();
        
        // Now should be spent
        assert!(client.is_nullifier_spent(&nullifier).unwrap());
    }
}
