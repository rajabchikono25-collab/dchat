//! Chat Chain client for identity, messaging, channels, permissions, governance, and reputation

use chrono::Utc;
use dchat_chain::{Transaction, TransactionStatus, TransactionType};
use dchat_core::types::{ChannelId, MessageId, UserId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

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
    #[allow(dead_code)]
    config: ChatChainConfig,
    /// Transaction cache
    transactions: Arc<RwLock<HashMap<Uuid, Transaction>>>,
    /// Current block height
    current_block: Arc<RwLock<u64>>,
    /// Reputation scores per user
    reputation_scores: Arc<RwLock<HashMap<UserId, u32>>>,
    /// Channel ownership and metadata
    channels: Arc<RwLock<HashMap<ChannelId, ChannelMetadata>>>,
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
    /// Create new chat chain client
    pub fn new(config: ChatChainConfig) -> Self {
        Self {
            config,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            reputation_scores: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register user identity on chat chain
    pub fn register_user(&self, user_id: &UserId, public_key: Vec<u8>) -> Result<Uuid, String> {
        let tx_id = Uuid::new_v4();
        let payload_json = serde_json::json!({
            "public_key": hex::encode(&public_key),
            "timestamp": Utc::now().timestamp(),
        });
        let payload = serde_json::to_vec(&payload_json).map_err(|e| e.to_string())?;

        let tx = Transaction {
            tx_id,
            tx_type: TransactionType::RegisterUser,
            payload,
            tx_hash: format!("{:x}", uuid::Uuid::new_v4()),
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        };

        self.transactions.write().unwrap().insert(tx_id, tx);

        // Initialize reputation score
        self.reputation_scores
            .write()
            .unwrap()
            .insert(user_id.clone(), 50);

        Ok(tx_id)
    }

    /// Send direct message on chat chain (ordering only)
    pub fn send_direct_message(
        &self,
        _sender: &UserId,
        _recipient: &UserId,
        _message_id: MessageId,
    ) -> Result<Uuid, String> {
        let tx_id = Uuid::new_v4();
        let payload_json = serde_json::json!({
            "timestamp": Utc::now().timestamp(),
        });
        let payload = serde_json::to_vec(&payload_json).map_err(|e| e.to_string())?;

        let tx = Transaction {
            tx_id,
            tx_type: TransactionType::SendDirectMessage,
            payload,
            tx_hash: format!("{:x}", uuid::Uuid::new_v4()),
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        };

        self.transactions.write().unwrap().insert(tx_id, tx);
        Ok(tx_id)
    }

    /// Create channel on chat chain
    pub fn create_channel(
        &self,
        owner: &UserId,
        channel_id: &ChannelId,
        name: String,
    ) -> Result<Uuid, String> {
        let tx_id = Uuid::new_v4();
        let payload_json = serde_json::json!({
            "channel_id": channel_id,
            "name": name,
            "timestamp": Utc::now().timestamp(),
        });
        let payload = serde_json::to_vec(&payload_json).map_err(|e| e.to_string())?;

        let tx = Transaction {
            tx_id,
            tx_type: TransactionType::CreateChannel,
            payload,
            tx_hash: format!("{:x}", uuid::Uuid::new_v4()),
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        };

        self.transactions.write().unwrap().insert(tx_id, tx);

        // Store channel metadata
        let channel_meta = ChannelMetadata {
            channel_id: channel_id.clone(),
            owner: owner.clone(),
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
    pub fn post_to_channel(
        &self,
        _sender: &UserId,
        _channel_id: &ChannelId,
        _message_id: MessageId,
    ) -> Result<Uuid, String> {
        let tx_id = Uuid::new_v4();
        let payload_json = serde_json::json!({
            "timestamp": Utc::now().timestamp(),
        });
        let payload = serde_json::to_vec(&payload_json).map_err(|e| e.to_string())?;

        let tx = Transaction {
            tx_id,
            tx_type: TransactionType::PostToChannel,
            payload,
            tx_hash: format!("{:x}", uuid::Uuid::new_v4()),
            status: TransactionStatus::Pending,
            submitted_at: Utc::now(),
            confirmed_at: None,
            fee_paid: 0,
        };

        self.transactions.write().unwrap().insert(tx_id, tx);
        Ok(tx_id)
    }

    /// Get user's reputation score
    pub fn get_reputation(&self, user_id: &UserId) -> Result<u32, String> {
        Ok(self
            .reputation_scores
            .read()
            .unwrap()
            .get(user_id)
            .copied()
            .unwrap_or(0))
    }

    /// Update user's reputation score
    pub fn update_reputation(&self, user_id: &UserId, delta: i32) -> Result<u32, String> {
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
    pub fn get_transaction(&self, tx_id: &Uuid) -> Result<Transaction, String> {
        self.transactions
            .read()
            .unwrap()
            .get(tx_id)
            .cloned()
            .ok_or_else(|| "Transaction not found".to_string())
    }

    /// Get current block height
    pub fn get_current_block(&self) -> Result<u64, String> {
        Ok(*self.current_block.read().unwrap())
    }

    /// Advance block height (for testing)
    pub fn advance_block(&self) -> Result<u64, String> {
        let mut block = self.current_block.write().unwrap();
        *block += 1;
        Ok(*block)
    }

    /// Get user transactions
    pub fn get_user_transactions(&self, _user_id: &UserId) -> Result<Vec<Transaction>, String> {
        Ok(self
            .transactions
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect())
    }

    /// Wait for transaction to achieve finality (confirmed status)
    /// Returns true if transaction reaches finality within timeout
    pub async fn wait_for_finality(&self, tx_id: &Uuid, required_confirmations: u32) -> Result<bool, String> {
        use tokio::time::{sleep, Duration};
        
        // Maximum wait time: 30 seconds
        let max_attempts = 30;
        let mut attempts = 0;

        while attempts < max_attempts {
            if let Some(tx) = self.transactions.read().unwrap().get(tx_id) {
                match &tx.status {
                    TransactionStatus::Confirmed { .. } => {
                        // Transaction reached finality
                        return Ok(true);
                    }
                    TransactionStatus::Failed { .. } => {
                        // Transaction failed
                        return Ok(false);
                    }
                    TransactionStatus::TimedOut => {
                        // Transaction timed out
                        return Ok(false);
                    }
                    TransactionStatus::Pending => {
                        // Still pending, wait and retry
                        // In production, this would check block confirmations
                        // For now, simulate confirmation after minimum blocks
                        if attempts >= required_confirmations {
                            // Mark as confirmed after required confirmations
                            let current_block = *self.current_block.read().unwrap();
                            if let Some(tx) = self.transactions.write().unwrap().get_mut(tx_id) {
                                tx.status = TransactionStatus::Confirmed {
                                    block_height: current_block,
                                    block_hash: format!("{:x}", Uuid::new_v4()),
                                };
                                tx.confirmed_at = Some(Utc::now());
                            }
                            return Ok(true);
                        }
                    }
                }
            } else {
                return Err(format!("Transaction not found: {}", tx_id));
            }

            attempts += 1;
            sleep(Duration::from_secs(1)).await;
        }

        // Timeout reached
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_user() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new(config);

        let user_id = UserId(Uuid::new_v4());
        let result = client.register_user(&user_id, vec![1, 2, 3]);
        assert!(result.is_ok());

        let reputation = client.get_reputation(&user_id).unwrap();
        assert_eq!(reputation, 50); // Initial reputation
    }

    #[test]
    fn test_create_channel() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new(config);

        let owner = UserId(Uuid::new_v4());
        let channel_id = ChannelId(Uuid::new_v4());
        let result = client.create_channel(&owner, &channel_id, "Test Channel".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_reputation_tracking() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new(config);

        let user_id = UserId(Uuid::new_v4());
        client.register_user(&user_id, vec![1, 2, 3]).unwrap();

        // Increase reputation
        client.update_reputation(&user_id, 10).unwrap();
        let rep = client.get_reputation(&user_id).unwrap();
        assert_eq!(rep, 60);

        // Decrease reputation
        client.update_reputation(&user_id, -20).unwrap();
        let rep = client.get_reputation(&user_id).unwrap();
        assert_eq!(rep, 40);
    }

    #[test]
    fn test_block_advancement() {
        let config = ChatChainConfig::default();
        let client = ChatChainClient::new(config);

        let block1 = client.get_current_block().unwrap();
        let block2 = client.advance_block().unwrap();
        assert_eq!(block2, block1 + 1);
    }
}
