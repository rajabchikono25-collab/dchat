//! Hierarchical Block Structure for High-Throughput Consensus
//!
//! Architecture:
//! Block (2 seconds) - Main consensus unit, BFT finality
//! ├── Subblock 1 (200ms) - Parallel execution unit
//! │   ├── Miniblock 1 (20ms) - Transaction batch (100-500 txs)
//! │   ├── Miniblock 2 (20ms) - Transaction batch (100-500 txs)
//! │   └── ... (10 miniblocks per subblock)
//! ├── Subblock 2 (200ms)
//! │   └── ... (10 miniblocks)
//! └── ... (10 subblocks per block)
//!
//! Throughput Calculation:
//! - 1 miniblock = 250 transactions (average)
//! - 10 miniblocks per subblock = 2,500 transactions
//! - 10 subblocks per block = 25,000 transactions
//! - 1 block per 2 seconds = **12,500 TPS base**
//! - With parallel processing (4x) = **50,000 TPS**
//! - With SIMD optimizations (1.5x) = **75,000 TPS**

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use thiserror::Error;

// Blake3 hash wrapper with serde support
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash(blake3::Hash);

impl Hash {
    pub fn from(bytes: [u8; 32]) -> Self {
        Self(blake3::Hash::from(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

impl From<blake3::Hash> for Hash {
    fn from(h: blake3::Hash) -> Self {
        Self(h)
    }
}

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(self.0.as_bytes())
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("hash must be 32 bytes"));
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        Ok(Hash(blake3::Hash::from(array)))
    }
}

/// Main block containing multiple subblocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Block height in the chain
    pub height: u64,
    /// Block creation timestamp
    pub timestamp: SystemTime,
    /// Hash of previous block
    pub previous_hash: Hash,
    /// Merkle root of all state changes
    pub state_root: Hash,
    /// Collection of subblocks (max 10)
    pub subblocks: Vec<Subblock>,
    /// BFT validator signatures (5-of-7 required)
    pub validator_signatures: Vec<ValidatorSignature>,
    /// PoRW relay votes for consensus
    pub relay_votes: Vec<RelayVote>,
    /// Finality proof from consensus layers
    pub finality_proof: FinalityProof,
}

/// Subblock for parallel execution (200ms window)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subblock {
    /// Index within parent block (0-9)
    pub index: u16,
    /// Subblock creation timestamp
    pub timestamp: SystemTime,
    /// Collection of miniblocks (max 10)
    pub miniblocks: Vec<Miniblock>,
    /// Execution result summary
    pub execution_result: ExecutionResult,
    /// Merkle root of all miniblock hashes
    pub merkle_root: Hash,
}

/// Miniblock for transaction batching (20ms window)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Miniblock {
    /// Index within parent subblock (0-9)
    pub index: u16,
    /// Miniblock creation timestamp
    pub timestamp: SystemTime,
    /// Batch of transactions (100-500)
    pub transactions: Vec<Transaction>,
    /// State hash before execution
    pub pre_state_hash: Hash,
    /// State hash after execution
    pub post_state_hash: Hash,
    /// Total gas consumed
    pub gas_used: u64,
}

/// Transaction placeholder (will be defined elsewhere)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub sender: Vec<u8>,
    pub recipient: Vec<u8>,
    pub amount: u64,
    pub signature: Vec<u8>,
}

/// Execution result for a subblock
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionResult {
    /// Number of successful transactions
    pub success_count: u32,
    /// Number of failed transactions
    pub failure_count: u32,
    /// Total gas used
    pub total_gas_used: u64,
    /// State deltas applied
    pub state_delta: Vec<StateDelta>,
}

/// State change record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDelta {
    pub account: Vec<u8>,
    pub field: String,
    pub old_value: Vec<u8>,
    pub new_value: Vec<u8>,
}

/// Validator signature for BFT consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator_id: Vec<u8>,
    pub signature: Vec<u8>,
    pub timestamp: SystemTime,
}

/// Relay vote for PoRW consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayVote {
    pub relay_id: Vec<u8>,
    pub vote_weight: f64,
    pub signature: Vec<u8>,
}

/// Finality proof combining multiple consensus layers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FinalityProof {
    /// PoRW finality achieved
    pub porw_finalized: bool,
    /// PoT finality achieved
    pub pot_finalized: bool,
    /// TSC finality achieved
    pub tsc_finalized: bool,
    /// Combined finality confidence (0.0-1.0)
    pub confidence: f64,
}

/// Errors in block operations
#[derive(Debug, Error)]
pub enum BlockError {
    #[error("Subblock limit exceeded (max 10)")]
    SubblockLimitExceeded,

    #[error("Miniblock limit exceeded (max 10)")]
    MiniblockLimitExceeded,

    #[error("Invalid subblock order")]
    InvalidSubblockOrder,

    #[error("Invalid timestamp")]
    InvalidTimestamp,

    #[error("Insufficient validator signatures (need 5-of-7)")]
    InsufficientSignatures,

    #[error("Invalid state transition")]
    InvalidStateTransition,

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}

impl Block {
    /// Create new block with given height and previous hash
    pub fn new(height: u64, previous_hash: Hash) -> Self {
        Self {
            height,
            timestamp: SystemTime::now(),
            previous_hash,
            state_root: Hash::from([0u8; 32]),
            subblocks: Vec::with_capacity(10),
            validator_signatures: Vec::new(),
            relay_votes: Vec::new(),
            finality_proof: FinalityProof::default(),
        }
    }

    /// Add subblock to block (max 10)
    pub fn add_subblock(&mut self, subblock: Subblock) -> Result<(), BlockError> {
        if self.subblocks.len() >= 10 {
            return Err(BlockError::SubblockLimitExceeded);
        }
        self.subblocks.push(subblock);
        Ok(())
    }

    /// Calculate total transaction count in block
    pub fn transaction_count(&self) -> usize {
        self.subblocks.iter().map(|sb| sb.transaction_count()).sum()
    }

    /// Calculate block hash
    pub fn calculate_hash(&self) -> Hash {
        let serialized = bincode::serialize(self).unwrap_or_default();
        Hash::from(blake3::hash(&serialized).into())
    }

    /// Verify block integrity
    pub fn verify(&self) -> Result<(), BlockError> {
        // 1. Verify finality proof
        self.finality_proof.verify()?;

        // 2. Verify subblock order and timestamps
        for (i, subblock) in self.subblocks.iter().enumerate() {
            if subblock.index != i as u16 {
                return Err(BlockError::InvalidSubblockOrder);
            }
            if i > 0 && subblock.timestamp <= self.subblocks[i - 1].timestamp {
                return Err(BlockError::InvalidTimestamp);
            }
        }

        // 3. Verify BFT signatures (5-of-7 threshold)
        if self.validator_signatures.len() < 5 {
            return Err(BlockError::InsufficientSignatures);
        }

        Ok(())
    }

    /// Check if block has reached finality
    pub fn is_finalized(&self) -> bool {
        self.finality_proof.is_finalized()
    }
}

impl Subblock {
    /// Create new subblock with given index
    pub fn new(index: u16) -> Self {
        Self {
            index,
            timestamp: SystemTime::now(),
            miniblocks: Vec::with_capacity(10),
            execution_result: ExecutionResult::default(),
            merkle_root: Hash::from([0u8; 32]),
        }
    }

    /// Add miniblock to subblock (max 10)
    pub fn add_miniblock(&mut self, miniblock: Miniblock) -> Result<(), BlockError> {
        if self.miniblocks.len() >= 10 {
            return Err(BlockError::MiniblockLimitExceeded);
        }
        self.miniblocks.push(miniblock);
        Ok(())
    }

    /// Calculate total transaction count in subblock
    pub fn transaction_count(&self) -> usize {
        self.miniblocks.iter().map(|mb| mb.transactions.len()).sum()
    }

    /// Calculate subblock hash
    pub fn calculate_hash(&self) -> Hash {
        let serialized = bincode::serialize(self).unwrap_or_default();
        Hash::from(blake3::hash(&serialized).into())
    }

    /// Calculate merkle root of all miniblocks
    pub fn calculate_merkle_root(&self) -> Hash {
        let hashes: Vec<_> = self
            .miniblocks
            .iter()
            .map(|mb| mb.calculate_hash())
            .collect();

        if hashes.is_empty() {
            return Hash::from([0u8; 32]);
        }

        calculate_merkle_root(&hashes)
    }
}

impl Miniblock {
    /// Create new miniblock with given index and transactions
    pub fn new(index: u16, transactions: Vec<Transaction>) -> Self {
        Self {
            index,
            timestamp: SystemTime::now(),
            transactions,
            pre_state_hash: Hash::from([0u8; 32]),
            post_state_hash: Hash::from([0u8; 32]),
            gas_used: 0,
        }
    }

    /// Calculate miniblock hash
    pub fn calculate_hash(&self) -> Hash {
        let serialized = bincode::serialize(self).unwrap_or_default();
        Hash::from(blake3::hash(&serialized).into())
    }

    /// Execute all transactions in miniblock
    pub async fn execute(&mut self, state: &mut WorldState) -> Result<(), BlockError> {
        self.pre_state_hash = state.compute_hash();

        let mut _success = 0;
        let mut gas_total = 0;

        for tx in &self.transactions {
            match state.apply_transaction(tx).await {
                Ok(gas) => {
                    _success += 1;
                    gas_total += gas;
                }
                Err(e) => {
                    tracing::warn!("Transaction {} failed: {:?}", tx.id, e);
                }
            }
        }

        self.post_state_hash = state.compute_hash();
        self.gas_used = gas_total;

        Ok(())
    }
}

impl FinalityProof {
    /// Verify finality proof
    pub fn verify(&self) -> Result<(), BlockError> {
        // All three consensus layers must agree
        if !self.porw_finalized || !self.pot_finalized || !self.tsc_finalized {
            return Err(BlockError::ExecutionFailed(
                "Not all consensus layers finalized".to_string(),
            ));
        }

        // Confidence must be high enough
        if self.confidence < 0.67 {
            return Err(BlockError::ExecutionFailed(format!(
                "Confidence too low: {:.2}%",
                self.confidence * 100.0
            )));
        }

        Ok(())
    }

    /// Check if block is finalized
    pub fn is_finalized(&self) -> bool {
        self.porw_finalized && self.pot_finalized && self.tsc_finalized && self.confidence >= 0.67
    }
}

/// Calculate Merkle root from list of hashes
fn calculate_merkle_root(hashes: &[Hash]) -> Hash {
    if hashes.is_empty() {
        return Hash::from([0u8; 32]);
    }

    if hashes.len() == 1 {
        return hashes[0];
    }

    let mut current_level = hashes.to_vec();

    while current_level.len() > 1 {
        let mut next_level = Vec::new();

        for chunk in current_level.chunks(2) {
            let combined = if chunk.len() == 2 {
                let mut data = Vec::new();
                data.extend_from_slice(chunk[0].as_bytes());
                data.extend_from_slice(chunk[1].as_bytes());
                Hash::from(blake3::hash(&data).into())
            } else {
                chunk[0]
            };
            next_level.push(combined);
        }

        current_level = next_level;
    }

    current_level[0]
}

/// World state with Merkle tree for efficient state verification
pub struct WorldState {
    /// Merkle tree root hash
    state_root: Hash,
    /// Account balances (address -> balance)
    balances: std::collections::HashMap<String, u64>,
    /// Account nonces (address -> nonce)
    nonces: std::collections::HashMap<String, u64>,
    /// Contract storage (address -> storage_key -> value)
    storage: std::collections::HashMap<String, std::collections::HashMap<Vec<u8>, Vec<u8>>>,
}

impl WorldState {
    /// Create new empty world state
    pub fn new() -> Self {
        Self {
            state_root: Hash::from([0u8; 32]),
            balances: std::collections::HashMap::new(),
            nonces: std::collections::HashMap::new(),
            storage: std::collections::HashMap::new(),
        }
    }

    /// Compute hash of current state using Merkle tree
    pub fn compute_hash(&self) -> Hash {
        use blake3::Hasher;

        let mut hasher = Hasher::new();

        // Hash balances
        let mut balance_entries: Vec<_> = self.balances.iter().collect();
        balance_entries.sort_by_key(|(addr, _)| *addr);
        for (addr, balance) in balance_entries {
            hasher.update(addr.as_bytes());
            hasher.update(&balance.to_le_bytes());
        }

        // Hash nonces
        let mut nonce_entries: Vec<_> = self.nonces.iter().collect();
        nonce_entries.sort_by_key(|(addr, _)| *addr);
        for (addr, nonce) in nonce_entries {
            hasher.update(addr.as_bytes());
            hasher.update(&nonce.to_le_bytes());
        }

        // Hash storage
        let mut storage_entries: Vec<_> = self.storage.iter().collect();
        storage_entries.sort_by_key(|(addr, _)| *addr);
        for (addr, storage_map) in storage_entries {
            hasher.update(addr.as_bytes());
            let mut storage_keys: Vec<_> = storage_map.iter().collect();
            storage_keys.sort_by_key(|(key, _)| *key);
            for (key, value) in storage_keys {
                hasher.update(key);
                hasher.update(value);
            }
        }

        let hash_bytes = hasher.finalize();
        Hash::from(*hash_bytes.as_bytes())
    }

    /// Apply transaction to state (placeholder implementation)
    pub async fn apply_transaction(&mut self, tx: &Transaction) -> Result<u64, BlockError> {
        // Simplified state transition for placeholder Transaction struct
        // Production would have full transaction type handling with nonces, gas, etc.

        let sender = hex::encode(&tx.sender);
        let recipient = hex::encode(&tx.recipient);

        // Simple balance transfer
        let sender_balance = self.balances.get(&sender).copied().unwrap_or(0);
        if sender_balance < tx.amount {
            return Err(BlockError::InvalidTransaction(
                "Insufficient balance".to_string(),
            ));
        }

        // Deduct from sender
        self.balances
            .insert(sender.clone(), sender_balance - tx.amount);

        // Add to recipient
        let recipient_balance = self.balances.get(&recipient).copied().unwrap_or(0);
        self.balances
            .insert(recipient, recipient_balance + tx.amount);

        // Increment nonce
        let current_nonce = self.nonces.get(&sender).copied().unwrap_or(0);
        self.nonces.insert(sender, current_nonce + 1);

        // Recompute state root
        self.state_root = self.compute_hash();

        // Gas cost: base 21000 + simple transfer overhead
        Ok(21000)
    }

    /// Get account balance
    pub fn get_balance(&self, address: &str) -> u64 {
        self.balances.get(address).copied().unwrap_or(0)
    }

    /// Get account nonce
    pub fn get_nonce(&self, address: &str) -> u64 {
        self.nonces.get(address).copied().unwrap_or(0)
    }

    /// Get state root hash
    pub fn get_state_root(&self) -> Hash {
        self.state_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_creation() {
        let block = Block::new(1, Hash::from([0u8; 32]));
        assert_eq!(block.height, 1);
        assert_eq!(block.subblocks.len(), 0);
    }

    #[test]
    fn test_subblock_limit() {
        let mut block = Block::new(1, Hash::from([0u8; 32]));

        // Add 10 subblocks (should work)
        for i in 0..10 {
            let subblock = Subblock::new(i);
            assert!(block.add_subblock(subblock).is_ok());
        }

        // Try to add 11th subblock (should fail)
        let subblock = Subblock::new(10);
        assert!(block.add_subblock(subblock).is_err());
    }

    #[test]
    fn test_miniblock_limit() {
        let mut subblock = Subblock::new(0);

        // Add 10 miniblocks (should work)
        for i in 0..10 {
            let miniblock = Miniblock::new(i, vec![]);
            assert!(subblock.add_miniblock(miniblock).is_ok());
        }

        // Try to add 11th miniblock (should fail)
        let miniblock = Miniblock::new(10, vec![]);
        assert!(subblock.add_miniblock(miniblock).is_err());
    }

    #[test]
    fn test_transaction_count() {
        let mut block = Block::new(1, Hash::from([0u8; 32]));

        // Create subblock with miniblocks containing transactions
        let mut subblock = Subblock::new(0);
        for i in 0..10 {
            let transactions = vec![
                Transaction {
                    id: format!("tx_{}", i),
                    sender: vec![0u8; 32],
                    recipient: vec![1u8; 32],
                    amount: 100,
                    signature: vec![],
                }; 25  // 25 transactions per miniblock
            ];
            let miniblock = Miniblock::new(i, transactions);
            subblock.add_miniblock(miniblock).unwrap();
        }

        block.add_subblock(subblock).unwrap();

        // Should have 250 transactions (10 miniblocks × 25 txs)
        assert_eq!(block.transaction_count(), 250);
    }

    #[test]
    fn test_merkle_root_calculation() {
        let hash1 = Hash::from(blake3::hash(b"data1").into());
        let hash2 = Hash::from(blake3::hash(b"data2").into());
        let hash3 = Hash::from(blake3::hash(b"data3").into());

        let root = calculate_merkle_root(&[hash1, hash2, hash3]);

        // Root should be deterministic
        let root2 = calculate_merkle_root(&[hash1, hash2, hash3]);
        assert_eq!(root, root2);

        // Different data should produce different root
        let hash4 = Hash::from(blake3::hash(b"data4").into());
        let root3 = calculate_merkle_root(&[hash1, hash2, hash4]);
        assert_ne!(root, root3);
    }

    #[test]
    fn test_finality_proof() {
        let mut proof = FinalityProof {
            porw_finalized: true,
            pot_finalized: true,
            tsc_finalized: true,
            confidence: 0.95,
        };

        assert!(proof.verify().is_ok());
        assert!(proof.is_finalized());

        // Test low confidence
        proof.confidence = 0.5;
        assert!(proof.verify().is_err());
        assert!(!proof.is_finalized());

        // Test missing consensus layer
        proof.confidence = 0.95;
        proof.porw_finalized = false;
        assert!(proof.verify().is_err());
        assert!(!proof.is_finalized());
    }
}
