// Currency Chain client for Rust SDK
// Handles payments, staking, rewards on currency chain

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CurrencyChainTxType {
    #[serde(rename = "payment")]
    Payment,
    #[serde(rename = "stake")]
    Stake,
    #[serde(rename = "unstake")]
    Unstake,
    #[serde(rename = "reward")]
    Reward,
    #[serde(rename = "slash")]
    Slash,
    #[serde(rename = "swap")]
    Swap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub user_id: String,
    pub balance: u64,
    pub staked: u64,
    pub rewards_pending: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyChainTransaction {
    pub id: String,
    pub tx_type: CurrencyChainTxType,
    pub from: String,
    pub to: Option<String>,
    pub amount: u64,
    pub status: String,
    pub confirmations: u32,
    pub block_height: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct CurrencyChainClient {
    /// RPC endpoint URL for currency chain
    pub rpc_url: String,
    /// Optional WebSocket URL for real-time updates
    pub ws_url: Option<String>,
    transactions: Arc<RwLock<HashMap<String, CurrencyChainTransaction>>>,
    current_block: Arc<RwLock<u64>>,
    wallets: Arc<RwLock<HashMap<String, Wallet>>>,
}

impl CurrencyChainClient {
    /// Create new currency chain client
    pub fn new(rpc_url: String, ws_url: Option<String>) -> Self {
        Self {
            rpc_url,
            ws_url,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            wallets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create wallet for user
    pub async fn create_wallet(&self, user_id: &str, initial_balance: u64) -> anyhow::Result<Wallet> {
        let wallet = Wallet {
            user_id: user_id.to_string(),
            balance: initial_balance,
            staked: 0,
            rewards_pending: 0,
        };
        self.wallets.write().await.insert(user_id.to_string(), wallet.clone());
        Ok(wallet)
    }

    /// Get wallet balance
    pub async fn get_wallet(&self, user_id: &str) -> anyhow::Result<Option<Wallet>> {
        Ok(self.wallets.read().await.get(user_id).cloned())
    }

    /// Get user balance
    pub async fn get_balance(&self, user_id: &str) -> anyhow::Result<u64> {
        Ok(self.wallets.read().await.get(user_id).map(|w| w.balance).unwrap_or(0))
    }

    /// Transfer tokens between users
    pub async fn transfer(&self, from: &str, to: &str, amount: u64) -> anyhow::Result<String> {
        let mut wallets = self.wallets.write().await;

        let from_wallet = wallets
            .get_mut(from)
            .ok_or_else(|| anyhow::anyhow!("From wallet not found"))?;

        if from_wallet.balance < amount {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        from_wallet.balance -= amount;

        let to_wallet = wallets
            .entry(to.to_string())
            .or_insert_with(|| Wallet {
                user_id: to.to_string(),
                balance: 0,
                staked: 0,
                rewards_pending: 0,
            });

        to_wallet.balance += amount;

        let tx_id = uuid::Uuid::new_v4().to_string();
        let tx = CurrencyChainTransaction {
            id: tx_id.clone(),
            tx_type: CurrencyChainTxType::Payment,
            from: from.to_string(),
            to: Some(to.to_string()),
            amount,
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: chrono::Utc::now().timestamp(),
        };

        drop(wallets);
        self.transactions.write().await.insert(tx_id.clone(), tx);

        Ok(tx_id)
    }

    /// Stake tokens for rewards
    pub async fn stake(&self, user_id: &str, amount: u64, _lock_duration_seconds: i64) -> anyhow::Result<String> {
        let mut wallets = self.wallets.write().await;

        let wallet = wallets
            .get_mut(user_id)
            .ok_or_else(|| anyhow::anyhow!("Wallet not found"))?;

        if wallet.balance < amount {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        wallet.balance -= amount;
        wallet.staked += amount;

        let tx_id = uuid::Uuid::new_v4().to_string();
        let tx = CurrencyChainTransaction {
            id: tx_id.clone(),
            tx_type: CurrencyChainTxType::Stake,
            from: user_id.to_string(),
            to: None,
            amount,
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: chrono::Utc::now().timestamp(),
        };

        drop(wallets);
        self.transactions.write().await.insert(tx_id.clone(), tx);

        Ok(tx_id)
    }

    /// Get transaction by ID
    pub async fn get_transaction(&self, tx_id: &str) -> anyhow::Result<Option<CurrencyChainTransaction>> {
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

    /// Get user transactions
    pub async fn get_user_transactions(&self, user_id: &str) -> anyhow::Result<Vec<CurrencyChainTransaction>> {
        let txs = self.transactions.read().await;
        Ok(txs
            .values()
            .filter(|tx| tx.from == user_id)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_wallet() {
        let client = CurrencyChainClient::new("http://localhost:8546".to_string(), None);
        let wallet = client.create_wallet("alice", 1000).await.unwrap();
        assert_eq!(wallet.balance, 1000);
    }

    #[tokio::test]
    async fn test_transfer() {
        let client = CurrencyChainClient::new("http://localhost:8546".to_string(), None);
        client.create_wallet("alice", 1000).await.unwrap();
        client.create_wallet("bob", 0).await.unwrap();

        let _tx_id = client.transfer("alice", "bob", 100).await.unwrap();

        assert_eq!(client.get_balance("alice").await.unwrap(), 900);
        assert_eq!(client.get_balance("bob").await.unwrap(), 100);
    }
}
