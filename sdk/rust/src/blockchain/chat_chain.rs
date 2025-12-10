// Chat Chain client for Rust SDK
// Handles identity, messaging, channels, governance on chat chain

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use base64::Engine;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatChainTxType {
    #[serde(rename = "register_user")]
    RegisterUser,
    #[serde(rename = "send_direct_message")]
    SendDirectMessage,
    #[serde(rename = "create_channel")]
    CreateChannel,
    #[serde(rename = "post_to_channel")]
    PostToChannel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChainTransaction {
    pub id: String,
    pub tx_type: ChatChainTxType,
    pub sender: String,
    pub data: serde_json::Value,
    pub status: String,
    pub confirmations: u32,
    pub block_height: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct ChatChainClient {
    /// RPC endpoint URL for chat chain
    pub rpc_url: String,
    /// Optional WebSocket URL for real-time updates
    pub ws_url: Option<String>,
    transactions: Arc<RwLock<HashMap<String, ChatChainTransaction>>>,
    current_block: Arc<RwLock<u64>>,
    reputation_scores: Arc<RwLock<HashMap<String, u32>>>,
}

impl ChatChainClient {
    /// Create new chat chain client
    pub fn new(rpc_url: String, ws_url: Option<String>) -> Self {
        Self {
            rpc_url,
            ws_url,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            reputation_scores: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register user identity on chat chain
    pub async fn register_user(
        &self,
        user_id: &str,
        public_key: Vec<u8>,
    ) -> anyhow::Result<String> {
        let tx_id = uuid::Uuid::new_v4().to_string();
        let tx = ChatChainTransaction {
            id: tx_id.clone(),
            tx_type: ChatChainTxType::RegisterUser,
            sender: user_id.to_string(),
            data: serde_json::json!({
                "public_key": base64::engine::general_purpose::STANDARD.encode(&public_key),
                "timestamp": chrono::Utc::now().timestamp(),
            }),
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: chrono::Utc::now().timestamp(),
        };

        self.transactions.write().await.insert(tx_id.clone(), tx);
        self.reputation_scores.write().await.insert(user_id.to_string(), 50);

        Ok(tx_id)
    }

    /// Send direct message on chat chain
    pub async fn send_direct_message(
        &self,
        sender: &str,
        recipient: &str,
        message_id: &str,
    ) -> anyhow::Result<String> {
        let tx_id = uuid::Uuid::new_v4().to_string();
        let tx = ChatChainTransaction {
            id: tx_id.clone(),
            tx_type: ChatChainTxType::SendDirectMessage,
            sender: sender.to_string(),
            data: serde_json::json!({
                "recipient": recipient,
                "message_id": message_id,
                "timestamp": chrono::Utc::now().timestamp(),
            }),
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: chrono::Utc::now().timestamp(),
        };

        self.transactions.write().await.insert(tx_id.clone(), tx);
        Ok(tx_id)
    }

    /// Create channel on chat chain
    pub async fn create_channel(
        &self,
        owner: &str,
        channel_id: &str,
        name: String,
    ) -> anyhow::Result<String> {
        let tx_id = uuid::Uuid::new_v4().to_string();
        let tx = ChatChainTransaction {
            id: tx_id.clone(),
            tx_type: ChatChainTxType::CreateChannel,
            sender: owner.to_string(),
            data: serde_json::json!({
                "channel_id": channel_id,
                "name": name,
                "timestamp": chrono::Utc::now().timestamp(),
            }),
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: chrono::Utc::now().timestamp(),
        };

        self.transactions.write().await.insert(tx_id.clone(), tx);
        Ok(tx_id)
    }

    /// Get transaction by ID
    pub async fn get_transaction(&self, tx_id: &str) -> anyhow::Result<Option<ChatChainTransaction>> {
        Ok(self.transactions.read().await.get(tx_id).cloned())
    }

    /// Get current block height
    pub async fn get_current_block(&self) -> u64 {
        *self.current_block.read().await
    }

    /// Advance block height
    pub async fn advance_block(&self) {
        let mut block = self.current_block.write().await;
        *block += 1;

        let mut txs = self.transactions.write().await;
        for tx in txs.values_mut() {
            if tx.status == "pending" || tx.status == "confirmed" {
                tx.confirmations += 1;
                if tx.confirmations >= 6 {
                    tx.status = "confirmed".to_string();
                }
                tx.block_height = *block;
            }
        }
    }

    /// Get reputation score
    pub async fn get_reputation(&self, user_id: &str) -> anyhow::Result<u32> {
        Ok(*self.reputation_scores.read().await.get(user_id).unwrap_or(&50))
    }

    /// Get user transactions
    pub async fn get_user_transactions(&self, user_id: &str) -> anyhow::Result<Vec<ChatChainTransaction>> {
        let txs = self.transactions.read().await;
        Ok(txs
            .values()
            .filter(|tx| tx.sender == user_id)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_user() {
        let client = ChatChainClient::new("http://localhost:8545".to_string(), None);
        let public_key = vec![1, 2, 3, 4];
        let tx_id = client.register_user("alice", public_key).await.unwrap();
        let tx = client.get_transaction(&tx_id).await.unwrap();
        assert!(tx.is_some());
    }

    #[tokio::test]
    async fn test_create_channel() {
        let client = ChatChainClient::new("http://localhost:8545".to_string(), None);
        let tx_id = client
            .create_channel("alice", "general", "General".to_string())
            .await
            .unwrap();
        let tx = client.get_transaction(&tx_id).await.unwrap();
        assert!(tx.is_some());
    }
}
