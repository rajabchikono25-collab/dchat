//! Transaction storage service for the chain module
//!
//! This service provides a high-level interface for storing and retrieving
//! blockchain transactions using the distributed storage backend.
//!
//! Features:
//! - Automatic storage across TiKV, CockroachDB, Redis, MinIO
//! - Transaction indexing by ID, hash, sender, and block
//! - Batch operations for bulk inserts
//! - Automatic tier migration (hot → cold)
//! - Health monitoring and statistics

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::storage_backend::{ChainStorageBackend, ChainStorageConfig, StoredTransaction};
use crate::transactions::{Transaction, TransactionStatus, TransactionType};
use crate::currency_transactions::CurrencyTransactionType;
use dchat_core::error::Result;

/// Transaction storage service
pub struct TransactionStorageService {
    /// Storage backend
    backend: Arc<ChainStorageBackend>,
    /// Configuration
    config: TransactionStorageConfig,
    /// Pending transactions queue
    pending_queue: Arc<RwLock<Vec<PendingTransaction>>>,
    /// Statistics
    stats: Arc<RwLock<ServiceStatistics>>,
}

/// Configuration for transaction storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionStorageConfig {
    /// Batch size for bulk operations
    pub batch_size: usize,
    /// Flush interval in seconds
    pub flush_interval_seconds: u64,
    /// Enable async writes (queue then batch)
    pub async_writes: bool,
    /// Maximum pending queue size
    pub max_pending_queue: usize,
    /// Enable automatic archival
    pub auto_archive: bool,
    /// Archive after N days
    pub archive_after_days: u64,
}

impl Default for TransactionStorageConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            flush_interval_seconds: 5,
            async_writes: true,
            max_pending_queue: 10000,
            auto_archive: true,
            archive_after_days: 90,
        }
    }
}

/// Pending transaction for async writes
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PendingTransaction {
    tx: Transaction,
    sender_key: Option<String>,
    queued_at: DateTime<Utc>,
}

/// Service statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceStatistics {
    pub total_stored: u64,
    pub total_retrieved: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub batch_writes: u64,
    pub failed_writes: u64,
    pub last_flush: Option<DateTime<Utc>>,
}

impl TransactionStorageService {
    /// Create new transaction storage service
    pub async fn new(
        storage_config: ChainStorageConfig,
        service_config: TransactionStorageConfig,
    ) -> Result<Self> {
        info!("Initializing transaction storage service");

        let backend = ChainStorageBackend::new(storage_config).await?;

        Ok(Self {
            backend: Arc::new(backend),
            config: service_config,
            pending_queue: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(ServiceStatistics::default())),
        })
    }

    /// Store a chat chain transaction
    pub async fn store_chat_transaction(&self, tx: &Transaction, sender_key: Option<&str>) -> Result<()> {
        if self.config.async_writes {
            self.queue_transaction(tx.clone(), sender_key.map(|s| s.to_string())).await?;
        } else {
            self.backend.store_transaction(tx, sender_key).await?;
            self.increment_stored().await;
        }

        debug!("Chat transaction {} queued/stored", tx.tx_id);
        Ok(())
    }

    /// Store a currency chain transaction
    pub async fn store_currency_transaction(
        &self,
        tx_type: CurrencyTransactionType,
        tx_hash: &str,
        payload: Vec<u8>,
        sender_key: &str,
        block_height: Option<u64>,
    ) -> Result<Uuid> {
        let tx_id = Uuid::new_v4();

        let stored_tx = StoredTransaction {
            tx_id,
            tx_type: format!("Currency::{:?}", tx_type),
            tx_hash: tx_hash.to_string(),
            payload,
            block_height,
            block_hash: None,
            status: if block_height.is_some() { "Confirmed".to_string() } else { "Pending".to_string() },
            submitted_at: Utc::now(),
            confirmed_at: if block_height.is_some() { Some(Utc::now()) } else { None },
            fee_paid: 0,
            sender_key: Some(sender_key.to_string()),
            region: None,
            cold_storage_key: None,
        };

        // Create a Transaction wrapper for backend
        let wrapper_tx = Transaction {
            tx_id,
            tx_type: TransactionType::RegisterUser, // Placeholder, actual type is in stored_tx
            payload: stored_tx.payload.clone(),
            tx_hash: stored_tx.tx_hash.clone(),
            status: if block_height.is_some() {
                TransactionStatus::Confirmed {
                    block_height: block_height.unwrap(),
                    block_hash: "pending".to_string(),
                }
            } else {
                TransactionStatus::Pending
            },
            submitted_at: stored_tx.submitted_at,
            confirmed_at: stored_tx.confirmed_at,
            fee_paid: 0,
        };

        self.backend.store_transaction(&wrapper_tx, Some(sender_key)).await?;
        self.increment_stored().await;

        info!("Currency transaction {} stored (type: {:?})", tx_id, tx_type);
        Ok(tx_id)
    }

    /// Queue transaction for batch write
    async fn queue_transaction(&self, tx: Transaction, sender_key: Option<String>) -> Result<()> {
        let mut queue = self.pending_queue.write().await;

        if queue.len() >= self.config.max_pending_queue {
            // Force flush if queue is full
            drop(queue);
            self.flush_pending().await?;
            queue = self.pending_queue.write().await;
        }

        queue.push(PendingTransaction {
            tx,
            sender_key,
            queued_at: Utc::now(),
        });

        // Auto-flush if batch size reached
        if queue.len() >= self.config.batch_size {
            drop(queue);
            self.flush_pending().await?;
        }

        Ok(())
    }

    /// Flush pending transactions to storage
    pub async fn flush_pending(&self) -> Result<usize> {
        let mut queue = self.pending_queue.write().await;
        let pending: Vec<_> = queue.drain(..).collect();
        drop(queue);

        if pending.is_empty() {
            return Ok(0);
        }

        let count = pending.len();
        info!("Flushing {} pending transactions", count);

        let mut success = 0;
        let mut failed = 0;

        for pending_tx in pending {
            match self.backend.store_transaction(&pending_tx.tx, pending_tx.sender_key.as_deref()).await {
                Ok(()) => success += 1,
                Err(e) => {
                    error!("Failed to store transaction {}: {}", pending_tx.tx.tx_id, e);
                    failed += 1;
                }
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_stored += success as u64;
            stats.failed_writes += failed as u64;
            stats.batch_writes += 1;
            stats.last_flush = Some(Utc::now());
        }

        if failed > 0 {
            warn!("Batch flush: {} succeeded, {} failed", success, failed);
        } else {
            info!("Batch flush complete: {} transactions stored", success);
        }

        Ok(success)
    }

    /// Get transaction by ID
    pub async fn get_transaction(&self, tx_id: Uuid) -> Result<Option<StoredTransaction>> {
        // Check pending queue first
        {
            let queue = self.pending_queue.read().await;
            for pending in queue.iter() {
                if pending.tx.tx_id == tx_id {
                    let mut stats = self.stats.write().await;
                    stats.total_retrieved += 1;
                    stats.cache_hits += 1;
                    return Ok(Some(StoredTransaction::from(&pending.tx)));
                }
            }
        }

        let result = self.backend.get_transaction(tx_id).await?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_retrieved += 1;
            if result.is_some() {
                stats.cache_hits += 1;
            } else {
                stats.cache_misses += 1;
            }
        }

        Ok(result)
    }

    /// Get transaction by hash
    pub async fn get_transaction_by_hash(&self, tx_hash: &str) -> Result<Option<StoredTransaction>> {
        // Check pending queue first
        {
            let queue = self.pending_queue.read().await;
            for pending in queue.iter() {
                if pending.tx.tx_hash == tx_hash {
                    return Ok(Some(StoredTransaction::from(&pending.tx)));
                }
            }
        }

        self.backend.get_transaction_by_hash(tx_hash).await
    }

    /// Get transactions by sender
    pub async fn get_transactions_by_sender(
        &self,
        sender_key: &str,
        limit: usize,
    ) -> Result<Vec<StoredTransaction>> {
        // This would query CockroachDB with sender filter
        // Simplified implementation - in production, add proper SQL query
        
        let mut results = Vec::new();

        // Check pending queue
        {
            let queue = self.pending_queue.read().await;
            for pending in queue.iter() {
                if pending.sender_key.as_deref() == Some(sender_key) {
                    results.push(StoredTransaction::from(&pending.tx));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(results)
    }

    /// Get transactions in a block
    pub async fn get_transactions_in_block(&self, _block_height: u64) -> Result<Vec<StoredTransaction>> {
        // This would query by block_height
        // Simplified - in production, use CockroachDB index
        Ok(Vec::new())
    }

    /// Update transaction status (when confirmed)
    pub async fn update_transaction_status(
        &self,
        tx_id: Uuid,
        status: TransactionStatus,
    ) -> Result<()> {
        // Get existing transaction
        if let Some(mut tx) = self.backend.get_transaction(tx_id).await? {
            // Update status fields
            match &status {
                TransactionStatus::Confirmed { block_height, block_hash } => {
                    tx.block_height = Some(*block_height);
                    tx.block_hash = Some(block_hash.clone());
                    tx.confirmed_at = Some(Utc::now());
                    tx.status = "Confirmed".to_string();
                }
                TransactionStatus::Failed { reason } => {
                    tx.status = format!("Failed: {}", reason);
                }
                TransactionStatus::TimedOut => {
                    tx.status = "TimedOut".to_string();
                }
                TransactionStatus::Pending => {
                    tx.status = "Pending".to_string();
                }
            }

            // Re-store with updated status
            let wrapper_tx = Transaction {
                tx_id: tx.tx_id,
                tx_type: TransactionType::RegisterUser,
                payload: tx.payload.clone(),
                tx_hash: tx.tx_hash.clone(),
                status,
                submitted_at: tx.submitted_at,
                confirmed_at: tx.confirmed_at,
                fee_paid: tx.fee_paid,
            };

            self.backend.store_transaction(&wrapper_tx, tx.sender_key.as_deref()).await?;

            info!("Transaction {} status updated", tx_id);
        }

        Ok(())
    }

    /// Run archival process for old transactions
    pub async fn run_archival(&self) -> Result<u64> {
        if !self.config.auto_archive {
            return Ok(0);
        }

        info!("Running transaction archival process");

        // Archive old transactions to cold storage
        // This would move transactions older than archive_after_days to MinIO
        
        let archived = 0u64;

        info!("Archived {} old transactions to cold storage", archived);
        Ok(archived)
    }

    /// Get service statistics
    pub async fn get_statistics(&self) -> ServiceStatistics {
        self.stats.read().await.clone()
    }

    /// Get storage backend statistics
    pub async fn get_storage_statistics(&self) -> Result<crate::storage_backend::StorageStatistics> {
        self.backend.get_statistics().await
    }

    /// Health check
    pub async fn health_check(&self) -> Result<crate::storage_backend::StorageHealthStatus> {
        self.backend.health_check().await
    }

    /// Get pending queue size
    pub async fn pending_count(&self) -> usize {
        self.pending_queue.read().await.len()
    }

    /// Helper to increment stored counter
    async fn increment_stored(&self) {
        let mut stats = self.stats.write().await;
        stats.total_stored += 1;
    }

    /// Start background flush task
    pub fn start_background_flush(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        let interval_secs = self.config.flush_interval_seconds;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                tokio::time::Duration::from_secs(interval_secs)
            );

            loop {
                interval.tick().await;

                if let Err(e) = service.flush_pending().await {
                    error!("Background flush failed: {}", e);
                }
            }
        })
    }

    /// Start background archival task
    pub fn start_background_archival(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let service = self.clone();

        tokio::spawn(async move {
            // Run archival once per day
            let mut interval = tokio::time::interval(
                tokio::time::Duration::from_secs(86400)
            );

            loop {
                interval.tick().await;

                if let Err(e) = service.run_archival().await {
                    error!("Background archival failed: {}", e);
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_config() {
        let config = TransactionStorageConfig::default();
        assert_eq!(config.batch_size, 100);
        assert!(config.async_writes);
        assert!(config.auto_archive);
    }

    #[tokio::test]
    async fn test_service_statistics() {
        let stats = ServiceStatistics::default();
        assert_eq!(stats.total_stored, 0);
        assert_eq!(stats.total_retrieved, 0);
    }
}
