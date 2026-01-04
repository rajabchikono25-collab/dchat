use chrono::{DateTime, Utc};
/// Mock Blockchain for Testing
///
/// Provides a simulated blockchain for integration tests
/// without requiring actual chain nodes.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransactionType {
    RegisterUser,
    SendDirectMessage,
    CreateChannel,
    PostToChannel,
    VoteOnGovernance,
}

#[derive(Debug, Clone)]
pub struct Transaction {
    pub tx_id: String,
    pub block_height: u64,
    pub tx_type: TransactionType,
    pub sender: String,
    pub data: HashMap<String, String>,
    pub timestamp: DateTime<Utc>,
    pub status: TransactionStatus,
    pub confirmations: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionStatus {
    Pending,
    Confirmed,
    Failed,
}

/// Mock blockchain state
pub struct MockBlockchain {
    transactions: Arc<Mutex<Vec<Transaction>>>,
    tx_by_id: Arc<Mutex<HashMap<String, Transaction>>>,
    current_block: Arc<Mutex<u64>>,
    confirmation_threshold: u32,
    auto_confirm: bool,
}

impl MockBlockchain {
    /// Create new mock blockchain
    pub fn new() -> Self {
        Self {
            transactions: Arc::new(Mutex::new(Vec::new())),
            tx_by_id: Arc::new(Mutex::new(HashMap::new())),
            current_block: Arc::new(Mutex::new(1)),
            confirmation_threshold: 6,
            auto_confirm: true,
        }
    }

    /// Create with custom confirmation threshold
    pub fn with_confirmation_threshold(mut self, threshold: u32) -> Self {
        self.confirmation_threshold = threshold;
        self
    }

    /// Set auto-confirm behavior
    pub fn with_auto_confirm(mut self, auto_confirm: bool) -> Self {
        self.auto_confirm = auto_confirm;
        self
    }

    /// Submit a transaction
    pub fn submit_transaction(
        &self,
        tx_type: TransactionType,
        sender: String,
        data: HashMap<String, String>,
    ) -> Result<String, String> {
        let tx_id = Uuid::new_v4().to_string();
        let current_block = *self.current_block.lock().unwrap();

        let tx = Transaction {
            tx_id: tx_id.clone(),
            block_height: current_block,
            tx_type,
            sender,
            data,
            timestamp: Utc::now(),
            status: if self.auto_confirm {
                TransactionStatus::Confirmed
            } else {
                TransactionStatus::Pending
            },
            confirmations: if self.auto_confirm {
                self.confirmation_threshold
            } else {
                0
            },
        };

        {
            let mut txs = self.transactions.lock().unwrap();
            txs.push(tx.clone());
        }

        {
            let mut tx_map = self.tx_by_id.lock().unwrap();
            tx_map.insert(tx_id.clone(), tx);
        }

        Ok(tx_id)
    }

    /// Get transaction by ID
    pub fn get_transaction(&self, tx_id: &str) -> Option<Transaction> {
        let tx_map = self.tx_by_id.lock().unwrap();
        tx_map.get(tx_id).cloned()
    }

    /// Get all transactions
    pub fn get_all_transactions(&self) -> Vec<Transaction> {
        let txs = self.transactions.lock().unwrap();
        txs.clone()
    }

    /// Get transactions by type
    pub fn get_transactions_by_type(&self, tx_type: TransactionType) -> Vec<Transaction> {
        let txs = self.transactions.lock().unwrap();
        txs.iter()
            .filter(|tx| tx.tx_type == tx_type)
            .cloned()
            .collect()
    }

    /// Get transactions by sender
    pub fn get_transactions_by_sender(&self, sender: &str) -> Vec<Transaction> {
        let txs = self.transactions.lock().unwrap();
        txs.iter()
            .filter(|tx| tx.sender == sender)
            .cloned()
            .collect()
    }

    /// Advance blockchain state (mine blocks)
    pub fn advance_blocks(&self, count: u64) {
        let mut block = self.current_block.lock().unwrap();
        *block += count;

        // Update confirmations
        if self.auto_confirm {
            let mut txs = self.transactions.lock().unwrap();
            for tx in txs.iter_mut() {
                if tx.status == TransactionStatus::Confirmed {
                    tx.confirmations = (*block - tx.block_height) as u32 + 1;
                }
            }
        }
    }

    /// Get current block height
    pub fn get_current_block(&self) -> u64 {
        *self.current_block.lock().unwrap()
    }

    /// Confirm a pending transaction
    pub fn confirm_transaction(&self, tx_id: &str) -> Result<(), String> {
        let mut tx_map = self.tx_by_id.lock().unwrap();

        if let Some(tx) = tx_map.get_mut(tx_id) {
            tx.status = TransactionStatus::Confirmed;
            tx.confirmations = self.confirmation_threshold;
            Ok(())
        } else {
            Err(format!("Transaction {} not found", tx_id))
        }
    }

    /// Reset blockchain state
    pub fn reset(&self) {
        {
            let mut txs = self.transactions.lock().unwrap();
            txs.clear();
        }
        {
            let mut tx_map = self.tx_by_id.lock().unwrap();
            tx_map.clear();
        }
        {
            let mut block = self.current_block.lock().unwrap();
            *block = 1;
        }
    }

    /// Get blockchain stats
    pub fn get_stats(&self) -> BlockchainStats {
        let txs = self.transactions.lock().unwrap();
        let block = self.current_block.lock().unwrap();

        let mut stats_by_type: HashMap<String, u32> = HashMap::new();
        let mut confirmed_count = 0u32;
        let mut pending_count = 0u32;

        for tx in txs.iter() {
            *stats_by_type
                .entry(format!("{:?}", tx.tx_type))
                .or_insert(0) += 1;

            match tx.status {
                TransactionStatus::Confirmed => confirmed_count += 1,
                TransactionStatus::Pending => pending_count += 1,
                TransactionStatus::Failed => {}
            }
        }

        BlockchainStats {
            total_transactions: txs.len() as u32,
            confirmed_transactions: confirmed_count,
            pending_transactions: pending_count,
            current_block_height: *block,
            transactions_by_type: stats_by_type,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlockchainStats {
    pub total_transactions: u32,
    pub confirmed_transactions: u32,
    pub pending_transactions: u32,
    pub current_block_height: u64,
    pub transactions_by_type: HashMap<String, u32>,
}

impl Default for MockBlockchain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_blockchain_creation() {
        let blockchain = MockBlockchain::new();
        assert_eq!(blockchain.get_current_block(), 1);
    }

    #[test]
    fn test_submit_transaction() {
        let blockchain = MockBlockchain::new();
        let mut data = HashMap::new();
        data.insert("user".to_string(), "alice".to_string());

        let tx_id = blockchain
            .submit_transaction(TransactionType::RegisterUser, "alice".to_string(), data)
            .unwrap();

        let tx = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.tx_type, TransactionType::RegisterUser);
        assert_eq!(tx.sender, "alice");
        assert!(tx.confirmations > 0); // auto-confirm
    }

    #[test]
    fn test_get_transactions_by_type() {
        let blockchain = MockBlockchain::new();
        let mut data = HashMap::new();
        data.insert("user".to_string(), "alice".to_string());

        blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                data.clone(),
            )
            .unwrap();
        blockchain
            .submit_transaction(TransactionType::RegisterUser, "bob".to_string(), data)
            .unwrap();

        let txs = blockchain.get_transactions_by_type(TransactionType::RegisterUser);
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn test_advance_blocks() {
        let blockchain = MockBlockchain::new();
        assert_eq!(blockchain.get_current_block(), 1);

        blockchain.advance_blocks(5);
        assert_eq!(blockchain.get_current_block(), 6);
    }

    #[test]
    fn test_blockchain_stats() {
        let blockchain = MockBlockchain::new();
        let mut data = HashMap::new();
        data.insert("user".to_string(), "alice".to_string());

        blockchain
            .submit_transaction(TransactionType::RegisterUser, "alice".to_string(), data)
            .unwrap();

        let stats = blockchain.get_stats();
        assert_eq!(stats.total_transactions, 1);
        assert_eq!(stats.confirmed_transactions, 1);
    }
}
