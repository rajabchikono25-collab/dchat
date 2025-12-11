//! Cross-Chain Synchronization Module
//!
//! This module provides unified synchronization across all supported chains:
//! - Chat chain (identity, messaging, governance)
//! - Currency chain (payments, staking, economics)
//! - Solana (external chain for wDCHAT bridging)
//!
//! Features:
//! - Real-time state synchronization
//! - Slot/block-based finality tracking
//! - Chain-specific confirmation requirements
//! - Merkle proof verification
//! - Delta synchronization for efficiency

use crate::{ChainId, FinalityProof};
use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Maximum number of pending sync operations per chain
const MAX_PENDING_SYNCS: usize = 1000;

/// Sync state for tracking cross-chain synchronization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncState {
    /// Initial state, not yet synced
    Pending,
    /// Sync in progress
    InProgress,
    /// Successfully synchronized
    Completed,
    /// Sync failed with error
    Failed { error: String },
    /// Sync timed out
    TimedOut,
}

/// Type of sync operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncOperationType {
    /// Bridge transfer from DCHAT to external chain
    BridgeOut {
        /// Destination address on external chain
        destination_address: String,
        /// Amount of tokens to bridge
        amount: u64,
    },
    /// Bridge transfer from external chain to DCHAT
    BridgeIn {
        /// Source address on external chain
        source_address: String,
        /// Amount of tokens bridged
        amount: u64,
    },
    /// State sync for governance
    GovernanceSync {
        /// Proposal ID being synced
        proposal_id: String,
        /// Vote data
        vote_data: Vec<u8>,
    },
    /// State sync for identity
    IdentitySync {
        /// User ID being synced
        user_id: UserId,
        /// Identity state hash
        state_hash: String,
    },
    /// State sync for balances
    BalanceSync {
        /// Account address
        address: String,
        /// New balance
        balance: u64,
    },
    /// Merkle root update
    MerkleRootUpdate {
        /// New merkle root
        root: String,
        /// Block/slot number
        block_number: u64,
    },
}

/// A cross-chain sync operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOperation {
    /// Unique operation ID
    pub id: Uuid,
    /// Source chain
    pub source_chain: ChainId,
    /// Destination chain
    pub destination_chain: ChainId,
    /// Type of sync operation
    pub operation_type: SyncOperationType,
    /// Current state
    pub state: SyncState,
    /// Source transaction hash
    pub source_tx_hash: Option<String>,
    /// Destination transaction hash
    pub destination_tx_hash: Option<String>,
    /// Block/slot number on source chain
    pub source_block: Option<u64>,
    /// Block/slot number on destination chain
    pub destination_block: Option<u64>,
    /// Confirmations received
    pub confirmations: u32,
    /// Required confirmations
    pub required_confirmations: u32,
    /// Finality proof
    pub finality_proof: Option<FinalityProof>,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
    /// Timeout timestamp
    pub timeout_at: DateTime<Utc>,
    /// Retry count
    pub retry_count: u32,
    /// Maximum retries
    pub max_retries: u32,
}

impl SyncOperation {
    /// Create a new sync operation
    pub fn new(
        source_chain: ChainId,
        destination_chain: ChainId,
        operation_type: SyncOperationType,
        timeout_seconds: i64,
    ) -> Self {
        let required_confirmations = source_chain.required_confirmations();
        let now = Utc::now();

        Self {
            id: Uuid::new_v4(),
            source_chain,
            destination_chain,
            operation_type,
            state: SyncState::Pending,
            source_tx_hash: None,
            destination_tx_hash: None,
            source_block: None,
            destination_block: None,
            confirmations: 0,
            required_confirmations,
            finality_proof: None,
            created_at: now,
            updated_at: now,
            timeout_at: now + chrono::Duration::seconds(timeout_seconds),
            retry_count: 0,
            max_retries: 3,
        }
    }

    /// Check if the operation has reached finality
    pub fn is_finalized(&self) -> bool {
        self.confirmations >= self.required_confirmations
    }

    /// Check if the operation has timed out
    pub fn is_timed_out(&self) -> bool {
        Utc::now() > self.timeout_at
    }

    /// Check if the operation can be retried
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries && !self.is_timed_out()
    }
}

/// Chain-specific sync adapter trait
#[async_trait::async_trait]
pub trait ChainSyncAdapter: Send + Sync {
    /// Get the chain ID this adapter handles
    fn chain_id(&self) -> ChainId;

    /// Get the current block/slot number
    async fn get_current_block(&self) -> Result<u64>;

    /// Check transaction confirmation count
    async fn get_confirmations(&self, tx_hash: &str) -> Result<u32>;

    /// Submit a sync operation to this chain
    async fn submit_operation(&self, operation: &SyncOperation) -> Result<String>;

    /// Verify a finality proof
    async fn verify_finality_proof(&self, proof: &FinalityProof) -> Result<bool>;

    /// Get merkle root at block
    async fn get_merkle_root(&self, block_number: u64) -> Result<String>;
}

/// Cross-chain sync manager
pub struct CrossChainSyncManager {
    /// Pending sync operations by ID
    operations: Arc<RwLock<HashMap<Uuid, SyncOperation>>>,
    /// Operations queue by chain
    chain_queues: Arc<RwLock<HashMap<ChainId, VecDeque<Uuid>>>>,
    /// Chain adapters
    adapters: Arc<RwLock<HashMap<ChainId, Arc<dyn ChainSyncAdapter>>>>,
    /// Completed operations (for history)
    completed: Arc<RwLock<VecDeque<SyncOperation>>>,
    /// Maximum completed operations to keep
    max_completed: usize,
}

impl CrossChainSyncManager {
    /// Create a new cross-chain sync manager
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(HashMap::new())),
            chain_queues: Arc::new(RwLock::new(HashMap::new())),
            adapters: Arc::new(RwLock::new(HashMap::new())),
            completed: Arc::new(RwLock::new(VecDeque::new())),
            max_completed: 10000,
        }
    }

    /// Register a chain adapter
    pub async fn register_adapter(&self, adapter: Arc<dyn ChainSyncAdapter>) -> Result<()> {
        let chain_id = adapter.chain_id();
        let mut adapters = self.adapters.write().await;
        adapters.insert(chain_id.clone(), adapter);

        // Initialize queue for this chain
        let mut queues = self.chain_queues.write().await;
        queues.entry(chain_id).or_insert_with(VecDeque::new);

        Ok(())
    }

    /// Create a bridge-out operation (DCHAT -> external chain)
    pub async fn create_bridge_out(
        &self,
        destination_chain: ChainId,
        destination_address: String,
        amount: u64,
        timeout_seconds: i64,
    ) -> Result<Uuid> {
        if !destination_chain.is_external() {
            return Err(Error::validation(
                "Bridge-out destination must be an external chain",
            ));
        }

        let operation = SyncOperation::new(
            ChainId::CurrencyChain, // Always from currency chain for bridge-out
            destination_chain,
            SyncOperationType::BridgeOut {
                destination_address,
                amount,
            },
            timeout_seconds,
        );

        self.add_operation(operation).await
    }

    /// Create a bridge-in operation (external chain -> DCHAT)
    pub async fn create_bridge_in(
        &self,
        source_chain: ChainId,
        source_address: String,
        amount: u64,
        source_tx_hash: String,
        timeout_seconds: i64,
    ) -> Result<Uuid> {
        if !source_chain.is_external() {
            return Err(Error::validation(
                "Bridge-in source must be an external chain",
            ));
        }

        let mut operation = SyncOperation::new(
            source_chain,
            ChainId::CurrencyChain, // Always to currency chain for bridge-in
            SyncOperationType::BridgeIn {
                source_address,
                amount,
            },
            timeout_seconds,
        );
        operation.source_tx_hash = Some(source_tx_hash);

        self.add_operation(operation).await
    }

    /// Create a governance sync operation
    pub async fn create_governance_sync(
        &self,
        source_chain: ChainId,
        destination_chain: ChainId,
        proposal_id: String,
        vote_data: Vec<u8>,
        timeout_seconds: i64,
    ) -> Result<Uuid> {
        let operation = SyncOperation::new(
            source_chain,
            destination_chain,
            SyncOperationType::GovernanceSync {
                proposal_id,
                vote_data,
            },
            timeout_seconds,
        );

        self.add_operation(operation).await
    }

    /// Create an identity sync operation
    pub async fn create_identity_sync(
        &self,
        source_chain: ChainId,
        destination_chain: ChainId,
        user_id: UserId,
        state_hash: String,
        timeout_seconds: i64,
    ) -> Result<Uuid> {
        let operation = SyncOperation::new(
            source_chain,
            destination_chain,
            SyncOperationType::IdentitySync {
                user_id,
                state_hash,
            },
            timeout_seconds,
        );

        self.add_operation(operation).await
    }

    /// Create a balance sync operation
    pub async fn create_balance_sync(
        &self,
        source_chain: ChainId,
        destination_chain: ChainId,
        address: String,
        balance: u64,
        timeout_seconds: i64,
    ) -> Result<Uuid> {
        let operation = SyncOperation::new(
            source_chain,
            destination_chain,
            SyncOperationType::BalanceSync { address, balance },
            timeout_seconds,
        );

        self.add_operation(operation).await
    }

    /// Add an operation to the sync queue
    async fn add_operation(&self, operation: SyncOperation) -> Result<Uuid> {
        let id = operation.id;
        let source_chain = operation.source_chain.clone();

        // Check queue size
        let queues = self.chain_queues.read().await;
        if let Some(queue) = queues.get(&source_chain) {
            if queue.len() >= MAX_PENDING_SYNCS {
                return Err(Error::validation(format!(
                    "Too many pending syncs for chain {:?}",
                    source_chain
                )));
            }
        }
        drop(queues);

        // Add to operations map
        let mut operations = self.operations.write().await;
        operations.insert(id, operation);

        // Add to chain queue
        let mut queues = self.chain_queues.write().await;
        queues
            .entry(source_chain)
            .or_insert_with(VecDeque::new)
            .push_back(id);

        Ok(id)
    }

    /// Get an operation by ID
    pub async fn get_operation(&self, id: Uuid) -> Option<SyncOperation> {
        let operations = self.operations.read().await;
        operations.get(&id).cloned()
    }

    /// Update operation state
    pub async fn update_state(&self, id: Uuid, state: SyncState) -> Result<()> {
        let mut operations = self.operations.write().await;
        let operation = operations
            .get_mut(&id)
            .ok_or_else(|| Error::validation("Operation not found"))?;

        operation.state = state;
        operation.updated_at = Utc::now();

        Ok(())
    }

    /// Update operation confirmations
    pub async fn update_confirmations(&self, id: Uuid, confirmations: u32) -> Result<bool> {
        let mut operations = self.operations.write().await;
        let operation = operations
            .get_mut(&id)
            .ok_or_else(|| Error::validation("Operation not found"))?;

        operation.confirmations = confirmations;
        operation.updated_at = Utc::now();

        Ok(operation.is_finalized())
    }

    /// Set source transaction hash
    pub async fn set_source_tx(&self, id: Uuid, tx_hash: String, block: u64) -> Result<()> {
        let mut operations = self.operations.write().await;
        let operation = operations
            .get_mut(&id)
            .ok_or_else(|| Error::validation("Operation not found"))?;

        operation.source_tx_hash = Some(tx_hash);
        operation.source_block = Some(block);
        operation.state = SyncState::InProgress;
        operation.updated_at = Utc::now();

        Ok(())
    }

    /// Set destination transaction hash
    pub async fn set_destination_tx(&self, id: Uuid, tx_hash: String, block: u64) -> Result<()> {
        let mut operations = self.operations.write().await;
        let operation = operations
            .get_mut(&id)
            .ok_or_else(|| Error::validation("Operation not found"))?;

        operation.destination_tx_hash = Some(tx_hash);
        operation.destination_block = Some(block);
        operation.updated_at = Utc::now();

        Ok(())
    }

    /// Complete an operation
    pub async fn complete_operation(&self, id: Uuid) -> Result<()> {
        let mut operations = self.operations.write().await;
        let mut operation = operations
            .remove(&id)
            .ok_or_else(|| Error::validation("Operation not found"))?;

        operation.state = SyncState::Completed;
        operation.updated_at = Utc::now();

        // Remove from chain queue
        let mut queues = self.chain_queues.write().await;
        if let Some(queue) = queues.get_mut(&operation.source_chain) {
            queue.retain(|&op_id| op_id != id);
        }
        drop(queues);

        // Add to completed
        let mut completed = self.completed.write().await;
        completed.push_back(operation);
        while completed.len() > self.max_completed {
            completed.pop_front();
        }

        Ok(())
    }

    /// Fail an operation
    pub async fn fail_operation(&self, id: Uuid, error: String) -> Result<()> {
        let mut operations = self.operations.write().await;
        let operation = operations
            .get_mut(&id)
            .ok_or_else(|| Error::validation("Operation not found"))?;

        operation.state = SyncState::Failed { error };
        operation.updated_at = Utc::now();
        operation.retry_count += 1;

        Ok(())
    }

    /// Process pending operations for a chain
    pub async fn process_chain(&self, chain_id: &ChainId) -> Result<Vec<Uuid>> {
        let adapters = self.adapters.read().await;
        let adapter = adapters
            .get(chain_id)
            .ok_or_else(|| Error::validation(format!("No adapter for chain {:?}", chain_id)))?
            .clone();
        drop(adapters);

        let mut processed = Vec::new();
        let queues = self.chain_queues.read().await;
        let queue = match queues.get(chain_id) {
            Some(q) => q.clone(),
            None => return Ok(processed),
        };
        drop(queues);

        for op_id in queue.iter().take(10) {
            // Process in batches of 10
            if let Some(operation) = self.get_operation(*op_id).await {
                // Check timeout
                if operation.is_timed_out() {
                    self.update_state(*op_id, SyncState::TimedOut).await?;
                    continue;
                }

                // Check confirmations if we have a source tx
                if let Some(ref tx_hash) = operation.source_tx_hash {
                    match adapter.get_confirmations(tx_hash).await {
                        Ok(confirmations) => {
                            let is_finalized =
                                self.update_confirmations(*op_id, confirmations).await?;
                            if is_finalized {
                                // Try to submit to destination chain
                                let dest_adapters = self.adapters.read().await;
                                if let Some(dest_adapter) =
                                    dest_adapters.get(&operation.destination_chain)
                                {
                                    match dest_adapter.submit_operation(&operation).await {
                                        Ok(dest_tx_hash) => {
                                            let dest_block =
                                                dest_adapter.get_current_block().await.unwrap_or(0);
                                            self.set_destination_tx(*op_id, dest_tx_hash, dest_block)
                                                .await?;
                                            self.complete_operation(*op_id).await?;
                                            processed.push(*op_id);
                                        }
                                        Err(e) => {
                                            self.fail_operation(*op_id, e.to_string()).await?;
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to get confirmations for {}: {}",
                                tx_hash,
                                e
                            );
                        }
                    }
                }
            }
        }

        Ok(processed)
    }

    /// Get all pending operations for a chain
    pub async fn get_pending_operations(&self, chain_id: &ChainId) -> Vec<SyncOperation> {
        let queues = self.chain_queues.read().await;
        let queue = match queues.get(chain_id) {
            Some(q) => q.clone(),
            None => return Vec::new(),
        };
        drop(queues);

        let operations = self.operations.read().await;
        queue
            .iter()
            .filter_map(|id| operations.get(id).cloned())
            .collect()
    }

    /// Get completed operations (for history/audit)
    pub async fn get_completed_operations(&self, limit: usize) -> Vec<SyncOperation> {
        let completed = self.completed.read().await;
        completed.iter().rev().take(limit).cloned().collect()
    }

    /// Get sync statistics
    pub async fn get_stats(&self) -> SyncStats {
        let operations = self.operations.read().await;
        let completed = self.completed.read().await;

        let mut pending_by_chain = HashMap::new();
        for op in operations.values() {
            *pending_by_chain
                .entry(op.source_chain.clone())
                .or_insert(0) += 1;
        }

        let mut completed_by_chain = HashMap::new();
        let mut failed_count = 0;
        for op in completed.iter() {
            *completed_by_chain
                .entry(op.source_chain.clone())
                .or_insert(0) += 1;
            if matches!(op.state, SyncState::Failed { .. }) {
                failed_count += 1;
            }
        }

        SyncStats {
            total_pending: operations.len(),
            total_completed: completed.len(),
            total_failed: failed_count,
            pending_by_chain,
            completed_by_chain,
        }
    }

    /// Run the sync manager (main processing loop)
    pub async fn run(&self) -> Result<()> {
        let chains = vec![ChainId::ChatChain, ChainId::CurrencyChain, ChainId::Solana];

        loop {
            for chain in &chains {
                if let Err(e) = self.process_chain(chain).await {
                    tracing::warn!("Error processing chain {:?}: {}", chain, e);
                }
            }

            // Small delay between processing cycles
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }
}

impl Default for CrossChainSyncManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Sync statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStats {
    pub total_pending: usize,
    pub total_completed: usize,
    pub total_failed: usize,
    pub pending_by_chain: HashMap<ChainId, usize>,
    pub completed_by_chain: HashMap<ChainId, usize>,
}

/// Delta sync for efficient state synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaSync {
    /// Chain this delta applies to
    pub chain: ChainId,
    /// Starting block/slot
    pub from_block: u64,
    /// Ending block/slot
    pub to_block: u64,
    /// State changes
    pub changes: Vec<StateChange>,
    /// Merkle root after changes
    pub merkle_root: String,
    /// Proof of delta validity
    pub proof: Vec<u8>,
}

/// A single state change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    /// Key being changed
    pub key: String,
    /// Old value (None if new key)
    pub old_value: Option<Vec<u8>>,
    /// New value (None if deleted)
    pub new_value: Option<Vec<u8>>,
    /// Block/slot where change occurred
    pub block_number: u64,
}

impl DeltaSync {
    /// Create a new delta sync
    pub fn new(chain: ChainId, from_block: u64, to_block: u64) -> Self {
        Self {
            chain,
            from_block,
            to_block,
            changes: Vec::new(),
            merkle_root: String::new(),
            proof: Vec::new(),
        }
    }

    /// Add a state change
    pub fn add_change(&mut self, change: StateChange) {
        self.changes.push(change);
    }

    /// Compute merkle root from changes
    pub fn compute_merkle_root(&mut self) {
        use blake3::Hasher;

        let mut hasher = Hasher::new();
        for change in &self.changes {
            hasher.update(change.key.as_bytes());
            if let Some(ref old) = change.old_value {
                hasher.update(old);
            }
            if let Some(ref new) = change.new_value {
                hasher.update(new);
            }
            hasher.update(&change.block_number.to_le_bytes());
        }

        self.merkle_root = hex::encode(hasher.finalize().as_bytes());
    }

    /// Verify delta integrity
    pub fn verify(&self) -> bool {
        let mut expected = self.clone();
        expected.compute_merkle_root();
        self.merkle_root == expected.merkle_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_bridge_out() {
        let manager = CrossChainSyncManager::new();

        let id = manager
            .create_bridge_out(
                ChainId::Solana,
                "SoLaNaAdDrEsS123".to_string(),
                1000,
                3600,
            )
            .await
            .unwrap();

        let op = manager.get_operation(id).await.unwrap();
        assert_eq!(op.source_chain, ChainId::CurrencyChain);
        assert_eq!(op.destination_chain, ChainId::Solana);
        assert!(matches!(
            op.operation_type,
            SyncOperationType::BridgeOut { .. }
        ));
    }

    #[tokio::test]
    async fn test_create_bridge_in() {
        let manager = CrossChainSyncManager::new();

        let id = manager
            .create_bridge_in(
                ChainId::Solana,
                "SoLaNaAdDrEsS456".to_string(),
                2000,
                "tx_hash_123".to_string(),
                3600,
            )
            .await
            .unwrap();

        let op = manager.get_operation(id).await.unwrap();
        assert_eq!(op.source_chain, ChainId::Solana);
        assert_eq!(op.destination_chain, ChainId::CurrencyChain);
        assert!(matches!(
            op.operation_type,
            SyncOperationType::BridgeIn { .. }
        ));
    }

    #[tokio::test]
    async fn test_invalid_bridge_out() {
        let manager = CrossChainSyncManager::new();

        // Bridge out to internal chain should fail
        let result = manager
            .create_bridge_out(ChainId::ChatChain, "addr".to_string(), 1000, 3600)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_confirmations() {
        let manager = CrossChainSyncManager::new();

        let id = manager
            .create_bridge_out(ChainId::Solana, "addr".to_string(), 1000, 3600)
            .await
            .unwrap();

        // Not finalized yet
        let is_finalized = manager.update_confirmations(id, 10).await.unwrap();
        assert!(!is_finalized);

        // Now finalized (32 required for Solana via CurrencyChain source which needs 20)
        let is_finalized = manager.update_confirmations(id, 25).await.unwrap();
        assert!(is_finalized);
    }

    #[tokio::test]
    async fn test_delta_sync() {
        let mut delta = DeltaSync::new(ChainId::ChatChain, 100, 200);

        delta.add_change(StateChange {
            key: "user:alice:balance".to_string(),
            old_value: Some(vec![1, 0, 0, 0]),
            new_value: Some(vec![2, 0, 0, 0]),
            block_number: 150,
        });

        delta.compute_merkle_root();
        assert!(!delta.merkle_root.is_empty());
        assert!(delta.verify());
    }

    #[tokio::test]
    async fn test_sync_stats() {
        let manager = CrossChainSyncManager::new();

        manager
            .create_bridge_out(ChainId::Solana, "addr1".to_string(), 1000, 3600)
            .await
            .unwrap();
        manager
            .create_bridge_out(ChainId::Solana, "addr2".to_string(), 2000, 3600)
            .await
            .unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.total_pending, 2);
        assert_eq!(
            stats.pending_by_chain.get(&ChainId::CurrencyChain),
            Some(&2)
        );
    }

    #[tokio::test]
    async fn test_complete_operation() {
        let manager = CrossChainSyncManager::new();

        let id = manager
            .create_bridge_out(ChainId::Solana, "addr".to_string(), 1000, 3600)
            .await
            .unwrap();

        manager.complete_operation(id).await.unwrap();

        // Operation should be removed from pending
        assert!(manager.get_operation(id).await.is_none());

        // Should be in completed
        let completed = manager.get_completed_operations(10).await;
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].id, id);
    }
}
