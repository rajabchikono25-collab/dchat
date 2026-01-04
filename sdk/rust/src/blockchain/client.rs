//! Blockchain client for transaction submission and confirmation

use crate::blockchain::transaction::{
    ChannelVisibility, CreateChannelTx, PostToChannelTx, RegisterUserTx,
    SendDirectMessageTx, TransactionReceipt,
};
use chrono::Utc;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;

/// Blockchain client configuration
#[derive(Debug, Clone)]
pub struct BlockchainConfig {
    /// JSON-RPC endpoint URL
    pub rpc_url: String,
    /// WebSocket URL (optional)
    pub ws_url: Option<String>,
    /// Number of confirmations required
    pub confirmation_blocks: u64,
    /// Confirmation timeout in seconds
    pub confirmation_timeout: u64,
    /// Maximum retry attempts
    pub max_retries: usize,
}

impl Default for BlockchainConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".to_string(),
            ws_url: Some("ws://localhost:8546".to_string()),
            confirmation_blocks: 6,
            confirmation_timeout: 300,
            max_retries: 3,
        }
    }
}

impl BlockchainConfig {
    /// Create a configuration for local development
    pub fn local() -> Self {
        Self::default()
    }

    /// Create a configuration for custom RPC endpoint
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            ..Default::default()
        }
    }
}

/// Blockchain client errors
#[derive(Debug, Error)]
pub enum BlockchainError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("Transaction confirmation timed out")]
    ConfirmationTimeout,

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Request error: {0}")]
    RequestError(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, BlockchainError>;

/// Blockchain client for transaction submission and confirmation
pub struct BlockchainClient {
    config: BlockchainConfig,
    http_client: Client,
    transaction_cache: Arc<RwLock<HashMap<String, TransactionReceipt>>>,
}

impl BlockchainClient {
    /// Create a new blockchain client
    pub fn new(config: BlockchainConfig) -> Self {
        Self {
            config,
            http_client: Client::new(),
            transaction_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a client for local development
    pub fn local() -> Self {
        Self::new(BlockchainConfig::local())
    }

    /// Register a new user on-chain
    pub async fn register_user(
        &self,
        user_id: String,
        username: String,
        public_key: String,
    ) -> Result<String> {
        let tx = RegisterUserTx {
            user_id,
            username,
            public_key,
            timestamp: Utc::now(),
            initial_reputation: 100,
        };

        self.submit_transaction("register_user", &tx).await
    }

    /// Send a direct message on-chain
    pub async fn send_direct_message(
        &self,
        message_id: String,
        sender_id: String,
        recipient_id: String,
        content_hash: String,
        payload_size: usize,
        relay_node_id: Option<String>,
    ) -> Result<String> {
        let tx = SendDirectMessageTx {
            message_id,
            sender_id,
            recipient_id,
            content_hash,
            payload_size,
            timestamp: Utc::now(),
            relay_node_id,
        };

        self.submit_transaction("send_direct_message", &tx).await
    }

    /// Create a new channel on-chain
    pub async fn create_channel(
        &self,
        channel_id: String,
        name: String,
        description: String,
        creator_id: String,
        visibility: ChannelVisibility,
        token_requirement: Option<String>,
    ) -> Result<String> {
        let tx = CreateChannelTx {
            channel_id,
            name,
            description,
            creator_id,
            visibility,
            timestamp: Utc::now(),
            token_requirement,
        };

        self.submit_transaction("create_channel", &tx).await
    }

    /// Post a message to a channel on-chain
    pub async fn post_to_channel(
        &self,
        message_id: String,
        channel_id: String,
        sender_id: String,
        content_hash: String,
        payload_size: usize,
    ) -> Result<String> {
        let tx = PostToChannelTx {
            message_id,
            channel_id,
            sender_id,
            content_hash,
            payload_size,
            timestamp: Utc::now(),
        };

        self.submit_transaction("post_to_channel", &tx).await
    }

    /// Wait for transaction confirmation
    pub async fn wait_for_confirmation(&self, tx_id: &str) -> Result<TransactionReceipt> {
        // Check cache first
        {
            let cache = self.transaction_cache.read().await;
            if let Some(receipt) = cache.get(tx_id) {
                if receipt.success || receipt.error.is_some() {
                    return Ok(receipt.clone());
                }
            }
        }

        let start = Instant::now();
        let timeout = Duration::from_secs(self.config.confirmation_timeout);

        loop {
            if start.elapsed() > timeout {
                return Err(BlockchainError::ConfirmationTimeout);
            }

            if let Some(receipt) = self.get_transaction_receipt(tx_id).await? {
                let mut cache = self.transaction_cache.write().await;
                cache.insert(tx_id.to_string(), receipt.clone());

                if receipt.success {
                    return Ok(receipt);
                } else if receipt.error.is_some() {
                    return Err(BlockchainError::TransactionFailed(
                        receipt.error.unwrap_or_default(),
                    ));
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Check if a transaction is confirmed
    pub async fn is_transaction_confirmed(&self, tx_id: &str) -> bool {
        match self.get_transaction_receipt(tx_id).await {
            Ok(Some(receipt)) => receipt.success,
            _ => false,
        }
    }

    /// Get transaction receipt
    pub async fn get_transaction_receipt(&self, tx_id: &str) -> Result<Option<TransactionReceipt>> {
        let response = self
            .http_client
            .post(&self.config.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_getTransactionReceipt",
                "params": [tx_id],
                "id": 1,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let data: Value = response.json().await?;

        if let Some(result) = data.get("result").and_then(|r| r.as_object()) {
            let receipt = TransactionReceipt {
                tx_id: result
                    .get("tx_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                tx_hash: result
                    .get("tx_hash")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                success: result.get("success").and_then(|v| v.as_bool()).unwrap_or_default(),
                block_height: result.get("block_height").and_then(|v| v.as_u64()),
                block_hash: result
                    .get("block_hash")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                timestamp: result
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                error: result
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            };
            return Ok(Some(receipt));
        }

        Ok(None)
    }

    /// Get current block number
    pub async fn get_block_number(&self) -> Result<u64> {
        let response = self
            .http_client
            .post(&self.config.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "eth_blockNumber",
                "params": [],
                "id": 1,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(BlockchainError::NetworkError(format!(
                "Failed to get block number: {}",
                response.status()
            )));
        }

        let data: Value = response.json().await?;
        let result = data
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BlockchainError::InvalidResponse("Missing result field".to_string()))?;

        u64::from_str_radix(result.trim_start_matches("0x"), 16)
            .map_err(|e| BlockchainError::InvalidResponse(e.to_string()))
    }

    /// Submit a transaction to the blockchain with retry logic
    async fn submit_transaction(&self, method: &str, params: &impl serde::Serialize) -> Result<String> {
        let mut attempts = 0;
        let mut delay = Duration::from_millis(100);
        let max_delay = Duration::from_secs(10);
        let backoff_multiplier = 2.0;

        loop {
            attempts += 1;

            match self.try_submit_transaction(method, params).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    // Check if error is retryable
                    let is_retryable = matches!(
                        &e,
                        BlockchainError::NetworkError(_) | BlockchainError::RequestError(_)
                    ) || (matches!(&e, BlockchainError::RpcError(msg) if 
                        msg.contains("timeout") || 
                        msg.contains("unavailable") || 
                        msg.contains("rate limit")));

                    if !is_retryable || attempts > self.config.max_retries {
                        return Err(e);
                    }

                    tokio::time::sleep(delay).await;
                    delay = Duration::from_secs_f64(
                        (delay.as_secs_f64() * backoff_multiplier).min(max_delay.as_secs_f64()),
                    );
                }
            }
        }
    }

    /// Single attempt to submit a transaction
    async fn try_submit_transaction(&self, method: &str, params: &impl serde::Serialize) -> Result<String> {
        let response = self
            .http_client
            .post(&self.config.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "method": format!("dchat_{}", method),
                "params": [params],
                "id": 1,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(BlockchainError::NetworkError(format!(
                "Failed to submit transaction: {}",
                response.status()
            )));
        }

        let data: Value = response.json().await?;

        if let Some(error) = data.get("error") {
            return Err(BlockchainError::RpcError(
                error
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error")
                    .to_string(),
            ));
        }

        data.get("result")
            .and_then(|r| r.get("tx_id"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| BlockchainError::InvalidResponse("Missing tx_id in response".to_string()))
    }
}

/// Hash content using SHA-256
pub fn hash_content(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}
