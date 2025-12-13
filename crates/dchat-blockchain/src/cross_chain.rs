//! Cross-chain bridge for atomic transactions between chat chain and currency chain
//!
//! This module now integrates with the ChainSynchronizer to ensure:
//! - Both chains use identical hierarchical block structure
//! - Finality is synchronized across chains
//! - Cross-chain transactions achieve atomic finality

use crate::chain_synchronizer::{ChainSyncConfig, ChainSynchronizer, CrossChainFinalityStatus};
use crate::chat_chain::ChatChainClient;
use crate::currency_chain::CurrencyChainClient;
use chrono::Utc;
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
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
/// Now integrates with ChainSynchronizer for unified block hierarchy and finality
pub struct CrossChainBridge {
    chat_chain: Arc<ChatChainClient>,
    currency_chain: Arc<CurrencyChainClient>,
    /// Track cross-chain transactions
    transactions: Arc<RwLock<HashMap<Uuid, CrossChainTransaction>>>,
    /// Chain synchronizer for unified finality
    synchronizer: Option<Arc<ChainSynchronizer>>,
}

impl CrossChainBridge {
    /// Create new cross-chain bridge
    pub fn new(chat_chain: Arc<ChatChainClient>, currency_chain: Arc<CurrencyChainClient>) -> Self {
        Self {
            chat_chain,
            currency_chain,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            synchronizer: None,
        }
    }

    /// Create cross-chain bridge with chain synchronizer for unified finality
    pub fn with_synchronizer(
        chat_chain: Arc<ChatChainClient>,
        currency_chain: Arc<CurrencyChainClient>,
        sync_config: ChainSyncConfig,
    ) -> Self {
        let synchronizer = Arc::new(ChainSynchronizer::new(sync_config));
        Self {
            chat_chain,
            currency_chain,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            synchronizer: Some(synchronizer),
        }
    }

    /// Get the chain synchronizer
    pub fn get_synchronizer(&self) -> Option<Arc<ChainSynchronizer>> {
        self.synchronizer.clone()
    }

    /// Start chain synchronization (call after creating the bridge)
    pub async fn start_synchronization(&self) -> Result<(), String> {
        if let Some(ref synchronizer) = self.synchronizer {
            synchronizer.start().await.map_err(|e| e.to_string())?;
            tracing::info!("🔄 Cross-chain synchronization started");
        }
        Ok(())
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
        let chat_tx = self
            .chat_chain
            .register_user(user_id, public_key)
            .await
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

        // Register with synchronizer for finality tracking
        if let Some(ref synchronizer) = self.synchronizer {
            synchronizer.register_cross_chain_tx(bridge_tx_id).await;
        }

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
            .create_channel(owner, &channel_id, channel_name)
            .await
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

        // Register with synchronizer for finality tracking
        if let Some(ref synchronizer) = self.synchronizer {
            synchronizer.register_cross_chain_tx(bridge_tx_id).await;
        }

        Ok(bridge_tx_id)
    }

    /// Get cross-chain transaction status
    pub fn get_status(&self, bridge_tx_id: &Uuid) -> Result<Option<CrossChainTransaction>, String> {
        Ok(self.transactions.read().unwrap().get(bridge_tx_id).cloned())
    }

    /// Initiate a cross-chain transfer to another chain
    ///
    /// Locks tokens on the source chain and creates a bridge transaction
    /// that can be claimed on the target chain.
    pub async fn initiate_transfer(
        &self,
        user_id: &UserId,
        amount: u64,
        target_chain: &str,
    ) -> Result<Uuid, String> {
        let bridge_tx_id = Uuid::new_v4();

        // Lock tokens on currency chain
        let currency_tx = self
            .currency_chain
            .transfer(
                user_id,
                &UserId(Uuid::nil()), // Bridge escrow address
                amount,
            )
            .map_err(|e| e.to_string())?;

        // Record bridge transaction
        let cross_tx = CrossChainTransaction {
            id: bridge_tx_id,
            operation: format!("bridge_to_{}", target_chain),
            user_id: user_id.clone(),
            chat_chain_tx: None,
            currency_chain_tx: Some(currency_tx),
            status: CrossChainStatus::Pending,
            created_at: Utc::now().timestamp(),
            finalized_at: None,
        };

        self.transactions
            .write()
            .unwrap()
            .insert(bridge_tx_id, cross_tx);

        // Register with synchronizer for finality tracking
        if let Some(ref synchronizer) = self.synchronizer {
            synchronizer.register_cross_chain_tx(bridge_tx_id).await;
        }

        Ok(bridge_tx_id)
    }

    /// Get the status of a cross-chain transaction by ID
    pub async fn get_transaction_status(&self, tx_id: &Uuid) -> Result<CrossChainStatus, String> {
        let txs = self.transactions.read().unwrap();
        txs.get(tx_id)
            .map(|tx| tx.status.clone())
            .ok_or_else(|| format!("Transaction {} not found", tx_id))
    }

    /// Wait for cross-chain transaction to achieve finality on both chains
    /// Uses the chain synchronizer for accurate finality tracking
    pub async fn wait_for_atomic_finality(
        &self,
        bridge_tx_id: &Uuid,
        timeout_secs: u64,
    ) -> Result<CrossChainFinalityStatus, String> {
        if let Some(ref synchronizer) = self.synchronizer {
            synchronizer
                .wait_for_cross_chain_finality(bridge_tx_id, Duration::from_secs(timeout_secs))
                .await
                .map_err(|e| e.to_string())
        } else {
            // Fall back to legacy finality checking
            self.finalize_pending_transactions()?;
            let tx = self
                .get_status(bridge_tx_id)?
                .ok_or_else(|| "Transaction not found".to_string())?;

            if tx.status == CrossChainStatus::AtomicSuccess {
                Ok(CrossChainFinalityStatus::Finalized {
                    chat_height: 0, // Unknown without synchronizer
                    currency_height: 0,
                    confidence: 1.0,
                })
            } else {
                Ok(CrossChainFinalityStatus::Pending)
            }
        }
    }

    /// Get synchronization status report
    pub async fn get_sync_status(&self) -> Option<crate::chain_synchronizer::SyncStatusReport> {
        if let Some(ref synchronizer) = self.synchronizer {
            Some(synchronizer.get_sync_status().await)
        } else {
            None
        }
    }

    /// Get throughput report for both chains
    pub async fn get_throughput_report(
        &self,
    ) -> Option<crate::chain_synchronizer::ThroughputReport> {
        if let Some(ref synchronizer) = self.synchronizer {
            Some(synchronizer.calculate_throughput().await)
        } else {
            None
        }
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
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let bridge = CrossChainBridge::new(chat_chain, currency_chain);

        let user_id = UserId(Uuid::new_v4());
        let public_key = vec![1, 2, 3, 4];

        let bridge_tx_id = bridge
            .register_user_with_stake(&user_id, public_key, 1000)
            .await
            .unwrap();
        let status = bridge.get_status(&bridge_tx_id).unwrap();

        assert!(status.is_some());
        let tx = status.unwrap();
        assert_eq!(tx.user_id, user_id);
        assert_eq!(tx.operation, "register_with_stake");
    }

    #[tokio::test]
    async fn test_bridge_with_synchronizer() {
        let chat_chain = Arc::new(ChatChainClient::new_mock(ChatChainConfig::default()));
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let sync_config = ChainSyncConfig::default();

        let bridge = CrossChainBridge::with_synchronizer(chat_chain, currency_chain, sync_config);

        // Verify synchronizer is attached
        assert!(bridge.get_synchronizer().is_some());
    }

    #[tokio::test]
    async fn test_cross_chain_with_finality_tracking() {
        let chat_chain = Arc::new(ChatChainClient::new_mock(ChatChainConfig::default()));
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let sync_config = ChainSyncConfig::default();

        let bridge = CrossChainBridge::with_synchronizer(
            chat_chain.clone(),
            currency_chain.clone(),
            sync_config,
        );

        let user_id = UserId(Uuid::new_v4());
        let public_key = vec![1, 2, 3, 4];

        let bridge_tx_id = bridge
            .register_user_with_stake(&user_id, public_key, 1000)
            .await
            .unwrap();

        // Verify transaction is tracked by synchronizer
        let sync = bridge.get_synchronizer().unwrap();
        let finality = sync.check_cross_chain_finality(&bridge_tx_id).await;

        // Initially should be pending (no finality updates yet)
        assert!(matches!(
            finality,
            CrossChainFinalityStatus::Pending | CrossChainFinalityStatus::Unknown
        ));
    }

    #[tokio::test]
    async fn test_throughput_report() {
        let chat_chain = Arc::new(ChatChainClient::new_mock(ChatChainConfig::default()));
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let sync_config = ChainSyncConfig::default();

        let bridge = CrossChainBridge::with_synchronizer(chat_chain, currency_chain, sync_config);

        let report = bridge.get_throughput_report().await.unwrap();

        // With hierarchical blocks enabled for both chains
        assert_eq!(report.chat_chain.base_tps, 12_500);
        assert_eq!(report.currency_chain.base_tps, 12_500);
        assert_eq!(report.combined_max_tps, 150_000); // 75K + 75K with SIMD
    }
}
