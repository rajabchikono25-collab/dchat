use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::chat_chain::ChatChainClient;
use super::currency_chain::CurrencyChainClient;

/// Cross-chain transaction status tracking
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CrossChainStatus {
    /// Awaiting both chains
    #[serde(rename = "pending")]
    Pending,
    /// Chat chain confirmed
    #[serde(rename = "chat_chain_confirmed")]
    ChatChainConfirmed,
    /// Currency chain confirmed
    #[serde(rename = "currency_chain_confirmed")]
    CurrencyChainConfirmed,
    /// Both chains confirmed - atomic success
    #[serde(rename = "atomic_success")]
    AtomicSuccess,
    /// Transaction rolled back
    #[serde(rename = "rolled_back")]
    RolledBack,
    /// Permanent failure
    #[serde(rename = "failed")]
    Failed,
}

impl std::fmt::Display for CrossChainStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrossChainStatus::Pending => write!(f, "pending"),
            CrossChainStatus::ChatChainConfirmed => write!(f, "chat_chain_confirmed"),
            CrossChainStatus::CurrencyChainConfirmed => write!(f, "currency_chain_confirmed"),
            CrossChainStatus::AtomicSuccess => write!(f, "atomic_success"),
            CrossChainStatus::RolledBack => write!(f, "rolled_back"),
            CrossChainStatus::Failed => write!(f, "failed"),
        }
    }
}

/// Cross-chain atomic operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrossChainOperation {
    /// Register user with stake on both chains
    RegisterUserWithStake {
        user_id: String,
        public_key: Vec<u8>,
        stake_amount: u64,
    },
    /// Create channel with fee payment
    CreateChannelWithFee {
        owner: String,
        channel_name: String,
        creation_fee: u64,
    },
}

/// Cross-chain transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainTransaction {
    /// Transaction ID
    pub id: String,
    /// Operation type
    pub operation: CrossChainOperation,
    /// User initiating operation
    pub user_id: String,
    /// Chat chain transaction ID
    pub chat_chain_tx_id: Option<String>,
    /// Currency chain transaction ID
    pub currency_chain_tx_id: Option<String>,
    /// Current status
    pub status: CrossChainStatus,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Finalization timestamp
    pub finalized_at: Option<DateTime<Utc>>,
    /// Number of confirmations on chat chain
    pub chat_confirmations: u64,
    /// Number of confirmations on currency chain
    pub currency_confirmations: u64,
}

/// Cross-chain bridge coordinating both chains
pub struct CrossChainBridge {
    /// Chat chain client
    chat_chain: Arc<ChatChainClient>,
    /// Currency chain client
    currency_chain: Arc<CurrencyChainClient>,
    /// Pending transactions
    pending_txs: Arc<RwLock<HashMap<String, CrossChainTransaction>>>,
    /// RPC endpoint for bridge service
    pub bridge_rpc_url: String,
}

impl CrossChainBridge {
    /// Create new cross-chain bridge
    pub fn new(
        chat_chain: Arc<ChatChainClient>,
        currency_chain: Arc<CurrencyChainClient>,
        bridge_rpc_url: String,
    ) -> Self {
        Self {
            chat_chain,
            currency_chain,
            pending_txs: Arc::new(RwLock::new(HashMap::new())),
            bridge_rpc_url,
        }
    }

    /// Register user with stake atomically
    pub async fn register_user_with_stake(
        &self,
        user_id: &str,
        public_key: Vec<u8>,
        stake_amount: u64,
    ) -> Result<String, String> {
        let bridge_tx_id = Uuid::new_v4().to_string();

        // Create wallet on currency chain
        let _wallet = self
            .currency_chain
            .create_wallet(user_id, stake_amount * 2)
            .await
            .map_err(|e| format!("Failed to create wallet: {}", e))?;

        let currency_tx_id = Uuid::new_v4().to_string();

        // Register identity on chat chain
        let chat_tx_id = self
            .chat_chain
            .register_user(user_id, public_key.clone())
            .await
            .map_err(|e| format!("Failed to register user: {}", e))?;

        // Stake tokens on currency chain
        let _stake_tx_id = self
            .currency_chain
            .stake(user_id, stake_amount, 2592000) // 30 days
            .await
            .map_err(|e| format!("Failed to stake tokens: {}", e))?;

        // Record cross-chain transaction
        let operation = CrossChainOperation::RegisterUserWithStake {
            user_id: user_id.to_string(),
            public_key,
            stake_amount,
        };

        let cross_tx = CrossChainTransaction {
            id: bridge_tx_id.clone(),
            operation,
            user_id: user_id.to_string(),
            chat_chain_tx_id: Some(chat_tx_id),
            currency_chain_tx_id: Some(currency_tx_id),
            status: CrossChainStatus::Pending,
            created_at: Utc::now(),
            finalized_at: None,
            chat_confirmations: 0,
            currency_confirmations: 0,
        };

        // Store transaction
        let mut txs = self.pending_txs.write().await;
        txs.insert(bridge_tx_id.clone(), cross_tx.clone());
        drop(txs);

        // Wait for both confirmations
        self.wait_for_confirmations(&bridge_tx_id, 6).await?;

        Ok(bridge_tx_id)
    }

    /// Create channel with fee payment atomically
    pub async fn create_channel_with_fee(
        &self,
        owner: &str,
        channel_name: String,
        creation_fee: u64,
    ) -> Result<String, String> {
        let bridge_tx_id = Uuid::new_v4().to_string();
        let channel_id = Uuid::new_v4().to_string();

        // Transfer creation fee to treasury
        let currency_tx_id = self
            .currency_chain
            .transfer(owner, "treasury", creation_fee)
            .await
            .map_err(|e| format!("Failed to pay channel creation fee: {}", e))?;

        // Create channel on chat chain
        let chat_tx_id = self
            .chat_chain
            .create_channel(owner, &channel_id, channel_name.clone())
            .await
            .map_err(|e| format!("Failed to create channel: {}", e))?;

        // Record cross-chain transaction
        let operation = CrossChainOperation::CreateChannelWithFee {
            owner: owner.to_string(),
            channel_name,
            creation_fee,
        };

        let cross_tx = CrossChainTransaction {
            id: bridge_tx_id.clone(),
            operation,
            user_id: owner.to_string(),
            chat_chain_tx_id: Some(chat_tx_id),
            currency_chain_tx_id: Some(currency_tx_id),
            status: CrossChainStatus::Pending,
            created_at: Utc::now(),
            finalized_at: None,
            chat_confirmations: 0,
            currency_confirmations: 0,
        };

        // Store transaction
        let mut txs = self.pending_txs.write().await;
        txs.insert(bridge_tx_id.clone(), cross_tx.clone());
        drop(txs);

        // Wait for both confirmations
        self.wait_for_confirmations(&bridge_tx_id, 6).await?;

        Ok(bridge_tx_id)
    }

    /// Get transaction status
    pub async fn get_status(&self, bridge_tx_id: &str) -> Result<Option<CrossChainTransaction>, String> {
        let txs = self.pending_txs.read().await;
        Ok(txs.get(bridge_tx_id).cloned())
    }

    /// Get user's cross-chain transactions
    pub async fn get_user_transactions(
        &self,
        user_id: &str,
    ) -> Result<Vec<CrossChainTransaction>, String> {
        let txs = self.pending_txs.read().await;
        let user_txs: Vec<_> = txs
            .values()
            .filter(|tx| tx.user_id == user_id)
            .cloned()
            .collect();
        Ok(user_txs)
    }

    /// Wait for both chains to confirm a cross-chain transaction
    async fn wait_for_confirmations(&self, bridge_tx_id: &str, target_confirmations: u64) -> Result<(), String> {
        let max_attempts = 120; // 2 minutes with 1s polling
        let mut attempts = 0;

        loop {
            let mut txs = self.pending_txs.write().await;

            if let Some(tx) = txs.get_mut(bridge_tx_id) {
                // Check chat chain confirmations
                if let Some(chat_tx_id) = &tx.chat_chain_tx_id {
                    if let Ok(Some(chat_tx)) = self.chat_chain.get_transaction(chat_tx_id).await {
                        tx.chat_confirmations = chat_tx.confirmations as u64;
                    }
                }

                // Check currency chain confirmations
                if let Some(currency_tx_id) = &tx.currency_chain_tx_id {
                    if let Ok(Some(currency_tx)) = self.currency_chain.get_transaction(currency_tx_id).await {
                        tx.currency_confirmations = currency_tx.confirmations as u64;
                    }
                }

                // Update status based on confirmations
                if tx.chat_confirmations >= target_confirmations
                    && tx.currency_confirmations >= target_confirmations
                {
                    tx.status = CrossChainStatus::AtomicSuccess;
                    tx.finalized_at = Some(Utc::now());
                    drop(txs);
                    return Ok(());
                }
            } else {
                return Err("Transaction not found".to_string());
            }

            drop(txs);

            attempts += 1;
            if attempts >= max_attempts {
                let mut txs = self.pending_txs.write().await;
                if let Some(tx) = txs.get_mut(bridge_tx_id) {
                    tx.status = CrossChainStatus::Failed;
                    tx.finalized_at = Some(Utc::now());
                }
                return Err("Confirmation timeout".to_string());
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    /// Finalize all pending transactions
    pub async fn finalize_pending_transactions(&self) -> Result<(), String> {
        let txs = self.pending_txs.read().await;
        let pending: Vec<_> = txs
            .iter()
            .filter(|(_, tx)| tx.status == CrossChainStatus::Pending)
            .map(|(id, _)| id.clone())
            .collect();
        drop(txs);

        for tx_id in pending {
            let _ = self.wait_for_confirmations(&tx_id, 6).await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_user_with_stake() {
        let chat_chain = Arc::new(ChatChainClient::new("http://localhost:8545".to_string(), None));
        let currency_chain = Arc::new(CurrencyChainClient::new("http://localhost:8546".to_string(), None));
        let bridge = CrossChainBridge::new(chat_chain, currency_chain, "http://localhost:8548".to_string());

        let user_id = "alice";
        let public_key = vec![1, 2, 3, 4, 5];
        let stake_amount = 1000u64;

        let result = bridge
            .register_user_with_stake(user_id, public_key, stake_amount)
            .await;

        assert!(result.is_ok());
        let bridge_tx_id = result.unwrap();

        // Verify transaction was recorded
        let status = bridge.get_status(&bridge_tx_id).await;
        assert!(status.is_ok());
        let tx = status.unwrap();
        assert!(tx.is_some());
        let tx = tx.unwrap();
        assert_eq!(tx.user_id, user_id);
    }

    #[tokio::test]
    async fn test_create_channel_with_fee() {
        let chat_chain = Arc::new(ChatChainClient::new("http://localhost:8545".to_string(), None));
        let currency_chain = Arc::new(CurrencyChainClient::new("http://localhost:8546".to_string(), None));
        let bridge = CrossChainBridge::new(chat_chain, currency_chain, "http://localhost:8548".to_string());

        let owner = "alice";
        let channel_name = "general".to_string();
        let creation_fee = 100u64;

        let result = bridge
            .create_channel_with_fee(owner, channel_name, creation_fee)
            .await;

        assert!(result.is_ok());
        let bridge_tx_id = result.unwrap();

        // Verify transaction was recorded
        let status = bridge.get_status(&bridge_tx_id).await;
        assert!(status.is_ok());
        let tx = status.unwrap();
        assert!(tx.is_some());
        let tx = tx.unwrap();
        assert_eq!(tx.user_id, owner);
    }

    #[tokio::test]
    async fn test_get_user_transactions() {
        let chat_chain = Arc::new(ChatChainClient::new("http://localhost:8545".to_string(), None));
        let currency_chain = Arc::new(CurrencyChainClient::new("http://localhost:8546".to_string(), None));
        let bridge = CrossChainBridge::new(chat_chain, currency_chain, "http://localhost:8548".to_string());

        let user_id = "bob";
        let public_key = vec![5, 6, 7, 8, 9];
        let stake_amount = 500u64;

        let _ = bridge
            .register_user_with_stake(user_id, public_key, stake_amount)
            .await;

        // Get user transactions
        let result = bridge.get_user_transactions(user_id).await;
        assert!(result.is_ok());
        let txs = result.unwrap();
        assert!(!txs.is_empty());
        assert_eq!(txs[0].user_id, user_id);
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let chat_chain = Arc::new(ChatChainClient::new("http://localhost:8545".to_string(), None));
        let currency_chain = Arc::new(CurrencyChainClient::new("http://localhost:8546".to_string(), None));
        let bridge = Arc::new(CrossChainBridge::new(
            chat_chain,
            currency_chain,
            "http://localhost:8548".to_string(),
        ));

        let mut handles = vec![];

        for i in 0..5 {
            let bridge_clone = Arc::clone(&bridge);
            let handle = tokio::spawn(async move {
                let user_id = format!("user{}", i);
                let public_key = vec![i as u8, i as u8 + 1, i as u8 + 2];
                let _ = bridge_clone
                    .register_user_with_stake(&user_id, public_key, 1000)
                    .await;
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }

        // Verify all transactions were recorded
        let all_txs = bridge.pending_txs.read().await;
        assert_eq!(all_txs.len(), 5);
    }
}
