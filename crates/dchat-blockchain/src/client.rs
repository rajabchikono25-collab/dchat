//! Blockchain client implementation for transaction submission and querying

use chrono::{DateTime, Utc};
use dchat_chain::{
    CreateChannelTx, PostToChannelTx, RegisterUserTx, SendDirectMessageTx, Transaction,
    TransactionReceipt, TransactionStatus, TransactionType,
};
use dchat_core::error::{Error, Result};
use dchat_core::types::{ChannelId, MessageId, UserId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Trait for blockchain RPC client abstraction
#[async_trait::async_trait]
pub trait ChainRpcClient: Send + Sync {
    /// Submit transaction to blockchain and return transaction hash
    async fn submit_transaction(&self, tx_bytes: Vec<u8>) -> Result<String>;
    
    /// Query transaction status by hash
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<TransactionStatus>;
    
    /// Get current block height
    async fn get_current_height(&self) -> Result<u64>;
    
    /// Get transaction receipt
    async fn get_transaction_receipt(&self, tx_hash: &str) -> Result<Option<TransactionReceipt>>;
}

/// Production HTTP RPC client implementation
pub struct HttpRpcClient {
    rpc_url: String,
    client: reqwest::Client,
}

impl HttpRpcClient {
    pub fn new(rpc_url: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;
        
        Ok(Self { rpc_url, client })
    }
}

#[async_trait::async_trait]
impl ChainRpcClient for HttpRpcClient {
    async fn submit_transaction(&self, tx_bytes: Vec<u8>) -> Result<String> {
        use serde_json::json;
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "chain_submitTransaction",
            "params": [hex::encode(&tx_bytes)],
            "id": 1
        });
        
        let response = self.client
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;
        
        if !response.status().is_success() {
            return Err(Error::network(format!("RPC returned error: {}", response.status())));
        }
        
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;
        
        if let Some(error) = json.get("error") {
            return Err(Error::network(format!("RPC error: {:?}", error)));
        }
        
        let tx_hash = json["result"]
            .as_str()
            .ok_or_else(|| Error::network("Missing transaction hash in response"))?;
        
        Ok(tx_hash.to_string())
    }
    
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<TransactionStatus> {
        use serde_json::json;
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "chain_getTransactionStatus",
            "params": [tx_hash],
            "id": 1
        });
        
        let response = self.client
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;
        
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;
        
        if let Some(error) = json.get("error") {
            return Err(Error::network(format!("RPC error: {:?}", error)));
        }
        
        let status_str = json["result"]["status"]
            .as_str()
            .ok_or_else(|| Error::network("Missing status in response"))?;
        
        match status_str {
            "confirmed" => {
                let block_height = json["result"]["blockHeight"].as_u64().unwrap_or(0);
                let block_hash = json["result"]["blockHash"].as_str().unwrap_or("").to_string();
                Ok(TransactionStatus::Confirmed { block_height, block_hash })
            }
            "pending" => Ok(TransactionStatus::Pending),
            "failed" => {
                let reason = json["result"]["reason"].as_str().unwrap_or("Unknown").to_string();
                Ok(TransactionStatus::Failed { reason })
            }
            _ => Ok(TransactionStatus::Pending)
        }
    }
    
    async fn get_current_height(&self) -> Result<u64> {
        use serde_json::json;
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "chain_getBlockHeight",
            "params": [],
            "id": 1
        });
        
        let response = self.client
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;
        
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;
        
        if let Some(error) = json.get("error") {
            return Err(Error::network(format!("RPC error: {:?}", error)));
        }
        
        let height = json["result"]
            .as_u64()
            .ok_or_else(|| Error::network("Invalid block height in response"))?;
        
        Ok(height)
    }
    
    async fn get_transaction_receipt(&self, tx_hash: &str) -> Result<Option<TransactionReceipt>> {
        use serde_json::json;
        
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "chain_getTransactionReceipt",
            "params": [tx_hash],
            "id": 1
        });
        
        let response = self.client
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;
        
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;
        
        if json["result"].is_null() {
            return Ok(None);
        }
        
        // Parse transaction receipt from JSON
        // For now, return None if not found
        Ok(None)
    }
}

/// Mock RPC client for testing (simulated responses)
pub struct MockRpcClient {
    transactions: Arc<RwLock<HashMap<String, TransactionStatus>>>,
    current_block: Arc<RwLock<u64>>,
}

impl MockRpcClient {
    pub fn new() -> Self {
        Self {
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
        }
    }
}

#[async_trait::async_trait]
impl ChainRpcClient for MockRpcClient {
    async fn submit_transaction(&self, _tx_bytes: Vec<u8>) -> Result<String> {
        // Generate mock transaction hash
        let tx_hash = format!("{:x}", Uuid::new_v4());
        
        // Store as pending
        self.transactions.write().unwrap().insert(
            tx_hash.clone(),
            TransactionStatus::Pending,
        );
        
        Ok(tx_hash)
    }
    
    async fn get_transaction_status(&self, tx_hash: &str) -> Result<TransactionStatus> {
        let txs = self.transactions.read().unwrap();
        Ok(txs.get(tx_hash)
            .cloned()
            .unwrap_or(TransactionStatus::Pending))
    }
    
    async fn get_current_height(&self) -> Result<u64> {
        Ok(*self.current_block.read().unwrap())
    }
    
    async fn get_transaction_receipt(&self, _tx_hash: &str) -> Result<Option<TransactionReceipt>> {
        Ok(None)
    }
}

/// Configuration for blockchain client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainConfig {
    /// RPC endpoint for blockchain node
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

impl Default for BlockchainConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".to_string(),
            ws_url: Some("ws://localhost:8546".to_string()),
            confirmation_blocks: 6,
            tx_timeout_seconds: 300, // 5 minutes
            max_retries: 3,
        }
    }
}

/// Blockchain client for interacting with the chat chain
pub struct BlockchainClient {
    config: BlockchainConfig,
    /// Transaction cache with hash mapping
    transactions: Arc<RwLock<HashMap<Uuid, (Transaction, Option<String>)>>>,
    /// RPC client for blockchain interaction
    rpc_client: Arc<dyn ChainRpcClient>,
}

impl BlockchainClient {
    /// Create a new blockchain client with production RPC
    pub fn new(config: BlockchainConfig) -> Result<Self> {
        let rpc_client = HttpRpcClient::new(config.rpc_url.clone())?;
        
        Ok(Self {
            config,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            rpc_client: Arc::new(rpc_client),
        })
    }
    
    /// Create a client with mock RPC for testing
    pub fn new_mock(config: BlockchainConfig) -> Self {
        let rpc_client = MockRpcClient::new();
        
        Self {
            config,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            rpc_client: Arc::new(rpc_client),
        }
    }

    /// Create a client with default configuration
    pub fn default() -> Result<Self> {
        Self::new(BlockchainConfig::default())
    }

    /// Submit a user registration transaction
    pub async fn register_user(
        &self,
        user_id: UserId,
        username: &str,
        public_key: &str,
    ) -> Result<Uuid> {
        let tx_payload = RegisterUserTx {
            user_id,
            username: username.to_string(),
            public_key: public_key.to_string(),
            timestamp: Utc::now(),
            initial_reputation: 0,
        };

        let payload_bytes = serde_json::to_vec(&tx_payload)
            .map_err(|e| Error::internal(format!("Failed to serialize tx: {}", e)))?;

        let transaction = Transaction::new(TransactionType::RegisterUser, payload_bytes);
        let tx_id = transaction.tx_id;

        // Submit transaction to blockchain via RPC
        let tx_bytes = bincode::serialize(&transaction)
            .map_err(|e| Error::internal(format!("Failed to serialize transaction: {}", e)))?;
        
        let tx_hash = self.rpc_client.submit_transaction(tx_bytes).await?;
        
        // Store transaction with hash mapping
        self.transactions
            .write()
            .unwrap()
            .insert(tx_id, (transaction.clone(), Some(tx_hash.clone())));
        
        tracing::info!("Submitted transaction {} with hash {}", tx_id, tx_hash);

        Ok(tx_id)
    }

    /// Submit a direct message transaction
    pub async fn send_direct_message(
        &self,
        message_id: MessageId,
        sender_id: UserId,
        recipient_id: UserId,
        content_hash: &str,
        payload_size: usize,
        relay_node_id: Option<String>,
    ) -> Result<Uuid> {
        let tx_payload = SendDirectMessageTx {
            message_id,
            sender_id,
            recipient_id,
            content_hash: content_hash.to_string(),
            timestamp: Utc::now(),
            payload_size,
            relay_node_id,
        };

        let payload_bytes = serde_json::to_vec(&tx_payload)
            .map_err(|e| Error::internal(format!("Failed to serialize tx: {}", e)))?;

        let transaction = Transaction::new(TransactionType::SendDirectMessage, payload_bytes);
        let tx_id = transaction.tx_id;

        // Submit transaction to blockchain via RPC
        let tx_bytes = bincode::serialize(&transaction)
            .map_err(|e| Error::internal(format!("Failed to serialize transaction: {}", e)))?;
        
        let tx_hash = self.rpc_client.submit_transaction(tx_bytes).await?;
        
        self.transactions
            .write()
            .unwrap()
            .insert(tx_id, (transaction.clone(), Some(tx_hash)));

        Ok(tx_id)
    }

    /// Submit a channel creation transaction
    pub async fn create_channel(
        &self,
        channel_id: ChannelId,
        name: &str,
        description: &str,
        creator_id: UserId,
    ) -> Result<Uuid> {
        let tx_payload = CreateChannelTx {
            channel_id,
            name: name.to_string(),
            description: description.to_string(),
            creator_id,
            visibility: dchat_chain::ChannelVisibility::Public,
            timestamp: Utc::now(),
            stake_amount: None,
        };

        let payload_bytes = serde_json::to_vec(&tx_payload)
            .map_err(|e| Error::internal(format!("Failed to serialize tx: {}", e)))?;

        let transaction = Transaction::new(TransactionType::CreateChannel, payload_bytes);
        let tx_id = transaction.tx_id;

        // Submit transaction to blockchain via RPC
        let tx_bytes = bincode::serialize(&transaction)
            .map_err(|e| Error::internal(format!("Failed to serialize transaction: {}", e)))?;
        
        let tx_hash = self.rpc_client.submit_transaction(tx_bytes).await?;
        
        self.transactions
            .write()
            .unwrap()
            .insert(tx_id, (transaction.clone(), Some(tx_hash)));

        Ok(tx_id)
    }

    /// Submit a channel message transaction
    pub async fn post_to_channel(
        &self,
        message_id: MessageId,
        channel_id: ChannelId,
        sender_id: UserId,
        content_hash: &str,
        payload_size: usize,
    ) -> Result<Uuid> {
        let tx_payload = PostToChannelTx {
            message_id,
            channel_id,
            sender_id,
            content_hash: content_hash.to_string(),
            timestamp: Utc::now(),
            payload_size,
        };

        let payload_bytes = serde_json::to_vec(&tx_payload)
            .map_err(|e| Error::internal(format!("Failed to serialize tx: {}", e)))?;

        let transaction = Transaction::new(TransactionType::PostToChannel, payload_bytes);
        let tx_id = transaction.tx_id;

        // Submit transaction to blockchain via RPC
        let tx_bytes = bincode::serialize(&transaction)
            .map_err(|e| Error::internal(format!("Failed to serialize transaction: {}", e)))?;
        
        let tx_hash = self.rpc_client.submit_transaction(tx_bytes).await?;
        
        self.transactions
            .write()
            .unwrap()
            .insert(tx_id, (transaction.clone(), Some(tx_hash)));

        Ok(tx_id)
    }

    /// Submit a delivery proof transaction for relay reward
    pub async fn submit_delivery_proof(
        &self,
        message_id: MessageId,
        relay_peer_id: String,
        recipient_id: UserId,
        recipient_signature: &[u8],
        timestamp: DateTime<Utc>,
        content_hash: String,
        reward_amount: u64,
    ) -> Result<Uuid> {
        use dchat_chain::SubmitDeliveryProofTx;

        tracing::info!(
            "📦 Submitting delivery proof for message {} via relay {}",
            message_id.0,
            &relay_peer_id
        );

        let tx_payload = SubmitDeliveryProofTx {
            message_id,
            relay_peer_id,
            recipient_id,
            recipient_signature: hex::encode(recipient_signature),
            timestamp,
            content_hash,
            reward_amount,
        };

        let payload_bytes = serde_json::to_vec(&tx_payload)
            .map_err(|e| Error::internal(format!("Failed to serialize tx: {}", e)))?;

        let transaction = Transaction::new(TransactionType::SubmitDeliveryProof, payload_bytes);
        let tx_id = transaction.tx_id;

        // Submit transaction to blockchain via RPC
        let tx_bytes = bincode::serialize(&transaction)
            .map_err(|e| Error::internal(format!("Failed to serialize transaction: {}", e)))?;
        
        let tx_hash = self.rpc_client.submit_transaction(tx_bytes).await?;
        
        self.transactions
            .write()
            .unwrap()
            .insert(tx_id, (transaction.clone(), Some(tx_hash)));

        tracing::info!("✅ Delivery proof submitted, tx_id: {}", tx_id);
        Ok(tx_id)
    }

    /// Check if a transaction is confirmed on-chain
    pub async fn is_transaction_confirmed(&self, tx_id: Uuid) -> Result<bool> {
        let transactions = self.transactions.read().unwrap();

        if let Some((tx, _hash)) = transactions.get(&tx_id) {
            Ok(matches!(tx.status, TransactionStatus::Confirmed { .. }))
        } else {
            Err(Error::validation("Transaction not found"))
        }
    }

    /// Wait for transaction confirmation
    pub async fn wait_for_confirmation(&self, tx_id: Uuid) -> Result<TransactionReceipt> {
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let rpc_url = std::env::var("BLOCKCHAIN_RPC_URL")
            .unwrap_or_else(|_| "http://localhost:8545".to_string());

        // Poll for confirmation with exponential backoff
        for attempt in 0..30 {
            let query = json!({
                "jsonrpc": "2.0",
                "method": "get_transaction_receipt",
                "params": {
                    "tx_id": tx_id.to_string(),
                },
                "id": 1,
            });

            let client = HttpClient::new();
            match client
                .post(&rpc_url)
                .json(&query)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        if let Some(result) = body.get("result") {
                            if !result.is_null() {
                                // Transaction confirmed
                                let block_height = result["block_height"].as_u64().unwrap_or(0);
                                let block_hash = result["block_hash"]
                                    .as_str()
                                    .unwrap_or("unknown")
                                    .to_string();
                                let success = result["success"].as_bool().unwrap_or(false);
                                let error = result["error"].as_str().map(|s| s.to_string());

                                // Update local transaction status
                                let mut transactions = self.transactions.write().unwrap();
                                if let Some((tx, _hash)) = transactions.get_mut(&tx_id) {
                                    tx.status = TransactionStatus::Confirmed {
                                        block_height,
                                        block_hash: block_hash.clone(),
                                    };
                                    tx.confirmed_at = Some(Utc::now());
                                }

                                tracing::info!(
                                    "✅ Transaction confirmed: {} at block {}",
                                    tx_id,
                                    block_height
                                );

                                return Ok(TransactionReceipt {
                                    tx_id,
                                    block_height,
                                    block_hash,
                                    tx_index: result["tx_index"].as_u64().unwrap_or(0) as u32,
                                    gas_used: result["gas_used"].as_u64().unwrap_or(21000),
                                    confirmed_at: Utc::now(),
                                    success,
                                    error,
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("RPC request failed (attempt {}): {}", attempt + 1, e);
                }
            }

            // Wait with exponential backoff (100ms, 200ms, 400ms, ...)
            let delay_ms = 100 * 2u64.pow(attempt.min(5));
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }

        Err(Error::validation(
            "Transaction confirmation timeout after 30 attempts",
        ))
    }

    /// Get transaction status
    pub fn get_transaction_status(&self, tx_id: Uuid) -> Option<TransactionStatus> {
        self.transactions
            .read()
            .unwrap()
            .get(&tx_id)
            .map(|(tx, _hash)| tx.status.clone())
    }

    /// Get transaction by ID
    pub fn get_transaction(&self, tx_id: Uuid) -> Option<Transaction> {
        self.transactions.read().unwrap().get(&tx_id).map(|(tx, _hash)| tx.clone())
    }

    /// Get current blockchain height
    pub async fn get_current_height(&self) -> Result<u64> {
        self.rpc_client.get_current_height().await
    }

    /// Submit transaction to blockchain (internal)
    async fn submit_transaction_to_chain(&self, transaction: Transaction) -> Result<()> {
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let rpc_url = std::env::var("BLOCKCHAIN_RPC_URL")
            .unwrap_or_else(|_| "http://localhost:8545".to_string());

        // Serialize transaction payload
        let tx_bytes = bincode::serialize(&transaction)
            .map_err(|e| Error::network(format!("Failed to serialize transaction: {}", e)))?;

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "submit_transaction",
            "params": {
                "tx_id": transaction.tx_id.to_string(),
                "tx_type": format!("{:?}", transaction.tx_type),
                "tx_hash": transaction.tx_hash.clone(),
                "data": hex::encode(&tx_bytes),
                "fee": transaction.fee_paid,
            },
            "id": 1,
        });

        tracing::info!(
            "📤 Submitting transaction {} to blockchain",
            transaction.tx_id
        );
        tracing::debug!("   RPC endpoint: {}", rpc_url);
        tracing::debug!("   Type: {:?}", transaction.tx_type);

        let client = HttpClient::new();
        let response = client
            .post(&rpc_url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to submit transaction: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Blockchain RPC error: status {}",
                response.status()
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;

        if let Some(error) = response_body.get("error") {
            return Err(Error::network(format!(
                "Blockchain rejected transaction: {}",
                error
            )));
        }

        tracing::info!(
            "✅ Transaction {} submitted successfully",
            transaction.tx_id
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_user() {
        let client = BlockchainClient::default();
        let user_id = UserId::new();

        let tx_id = client
            .register_user(user_id, "alice", "deadbeef")
            .await
            .unwrap();

        assert!(client.get_transaction(tx_id).is_some());
    }

    #[tokio::test]
    async fn test_wait_for_confirmation() {
        let client = BlockchainClient::default();
        let user_id = UserId::new();

        let tx_id = client
            .register_user(user_id, "bob", "cafebabe")
            .await
            .unwrap();

        let receipt = client.wait_for_confirmation(tx_id).await.unwrap();
        assert!(receipt.success);
        assert!(client.is_transaction_confirmed(tx_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_send_direct_message() {
        let client = BlockchainClient::default();
        let sender = UserId::new();
        let recipient = UserId::new();
        let message_id = MessageId::new();

        let tx_id = client
            .send_direct_message(message_id, sender, recipient, "hash123", 100, None)
            .await
            .unwrap();

        assert!(client.get_transaction(tx_id).is_some());
    }

    #[tokio::test]
    async fn test_create_channel() {
        let client = BlockchainClient::default();
        let creator = UserId::new();
        let channel_id = ChannelId::new();

        let tx_id = client
            .create_channel(channel_id, "general", "General discussion", creator)
            .await
            .unwrap();

        let receipt = client.wait_for_confirmation(tx_id).await.unwrap();
        assert!(receipt.success);
    }
}
