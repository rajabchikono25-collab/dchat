//! Cross-chain bridge for atomic transactions between chat chain and currency chain

use crate::chat_chain::ChatChainClient;
use crate::currency_chain::CurrencyChainClient;
use chrono::Utc;
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Atomic cross-chain transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainTransaction {
    pub id: Uuid,
    pub operation: String, // "register_with_stake", "channel_creation_with_fee", etc.
    pub user_id: UserId,
    pub chat_chain_tx: Option<Uuid>,
    pub currency_chain_tx: Option<Uuid>,
    pub status: CrossChainStatus,
    pub created_at: i64,
    pub finalized_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CrossChainStatus {
    Pending,
    ChatChainConfirmed,
    CurrencyChainConfirmed,
    AtomicSuccess,
    RolledBack,
    Failed,
}

/// Bridge for coordinating transactions between chat and currency chains
pub struct CrossChainBridge {
    chat_chain: Arc<ChatChainClient>,
    currency_chain: Arc<CurrencyChainClient>,
    /// Track cross-chain transactions
    transactions: Arc<RwLock<HashMap<Uuid, CrossChainTransaction>>>,
}

impl CrossChainBridge {
    /// Create new cross-chain bridge
    pub fn new(chat_chain: Arc<ChatChainClient>, currency_chain: Arc<CurrencyChainClient>) -> Self {
        Self {
            chat_chain,
            currency_chain,
            transactions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register user with initial stake (atomic operation)
    pub async fn register_user_with_stake(
        &self,
        user_id: &UserId,
        public_key: Vec<u8>,
        stake_amount: u64,
    ) -> Result<Uuid, String> {
        let bridge_tx_id = Uuid::new_v4();

        // Step 1: Create wallet on currency chain
        let _wallet = self
            .currency_chain
            .create_wallet(user_id, stake_amount)
            .map_err(|e| e.to_string())?;

        // Step 2: Register identity on chat chain
        let chat_tx = self.chat_chain.register_user(user_id, public_key).await
            .map_err(|e| e.to_string())?;

        // Step 3: Stake tokens on currency chain
        let currency_tx = self
            .currency_chain
            .stake(user_id, stake_amount, 86400)
            .map_err(|e| e.to_string())?;

        // Record cross-chain transaction
        let cross_tx = CrossChainTransaction {
            id: bridge_tx_id,
            operation: "register_with_stake".to_string(),
            user_id: user_id.clone(),
            chat_chain_tx: Some(chat_tx),
            currency_chain_tx: Some(currency_tx),
            status: CrossChainStatus::Pending,
            created_at: Utc::now().timestamp(),
            finalized_at: None,
        };

        self.transactions
            .write()
            .unwrap()
            .insert(bridge_tx_id, cross_tx);

        Ok(bridge_tx_id)
    }

    /// Create channel with creation fee (atomic operation)
    pub async fn create_channel_with_fee(
        &self,
        owner: &UserId,
        channel_name: String,
        creation_fee: u64,
    ) -> Result<Uuid, String> {
        use dchat_core::types::ChannelId;

        let bridge_tx_id = Uuid::new_v4();
        let channel_id = ChannelId(uuid::Uuid::new_v4());

        // Step 1: Pay creation fee on currency chain
        let fee_tx = self
            .currency_chain
            .transfer(owner, &UserId(uuid::Uuid::new_v4()), creation_fee)
            .map_err(|e| e.to_string())?;

        // Step 2: Create channel on chat chain
        let chat_tx = self
            .chat_chain
            .create_channel(owner, &channel_id, channel_name).await
            .map_err(|e| e.to_string())?;

        // Record cross-chain transaction
        let cross_tx = CrossChainTransaction {
            id: bridge_tx_id,
            operation: "channel_creation_with_fee".to_string(),
            user_id: owner.clone(),
            chat_chain_tx: Some(chat_tx),
            currency_chain_tx: Some(fee_tx),
            status: CrossChainStatus::Pending,
            created_at: Utc::now().timestamp(),
            finalized_at: None,
        };

        self.transactions
            .write()
            .unwrap()
            .insert(bridge_tx_id, cross_tx);

        Ok(bridge_tx_id)
    }

    /// Get cross-chain transaction status
    pub fn get_status(&self, bridge_tx_id: &Uuid) -> Result<Option<CrossChainTransaction>, String> {
        Ok(self.transactions.read().unwrap().get(bridge_tx_id).cloned())
    }

    /// Check and finalize cross-chain transactions
    pub fn finalize_pending_transactions(&self) -> Result<(), String> {
        let mut txs = self.transactions.write().unwrap();

        for tx in txs.values_mut() {
            if tx.status == CrossChainStatus::Pending {
                // Check if both chains confirmed
                let chat_confirmed = if let Some(chat_tx_id) = tx.chat_chain_tx {
                    match self.chat_chain.get_transaction(&chat_tx_id) {
                        Ok(chat_tx) => matches!(
                            chat_tx.status,
                            dchat_chain::TransactionStatus::Confirmed { .. }
                        ),
                        _ => false,
                    }
                } else {
                    false
                };

                let currency_confirmed = if let Some(currency_tx_id) = tx.currency_chain_tx {
                    match self.currency_chain.get_transaction(&currency_tx_id) {
                        Ok(tx_opt) => tx_opt.map(|t| t.status == "confirmed").unwrap_or(false),
                        _ => false,
                    }
                } else {
                    false
                };

                if chat_confirmed && currency_confirmed {
                    tx.status = CrossChainStatus::AtomicSuccess;
                    tx.finalized_at = Some(Utc::now().timestamp());
                }
            }
        }

        Ok(())
    }

    /// Get all cross-chain transactions for a user
    pub fn get_user_transactions(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<CrossChainTransaction>, String> {
        let txs = self.transactions.read().unwrap();
        Ok(txs
            .values()
            .filter(|tx| tx.user_id == *user_id)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_chain::ChatChainConfig;
    use crate::currency_chain::CurrencyChainConfig;

    #[tokio::test]
    async fn test_register_user_with_stake() {
        let chat_chain = Arc::new(ChatChainClient::new_mock(ChatChainConfig::default()));
        let currency_chain = Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let bridge = CrossChainBridge::new(chat_chain, currency_chain);

        let user_id = UserId(Uuid::new_v4());
        let public_key = vec![1, 2, 3, 4];

        let bridge_tx_id = bridge
            .register_user_with_stake(&user_id, public_key, 1000).await
            .unwrap();
        let status = bridge.get_status(&bridge_tx_id).unwrap();

        assert!(status.is_some());
        let tx = status.unwrap();
        assert_eq!(tx.user_id, user_id);
        assert_eq!(tx.operation, "register_with_stake");
    }
}
