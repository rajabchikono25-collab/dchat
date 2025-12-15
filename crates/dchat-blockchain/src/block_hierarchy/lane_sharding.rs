//! Lane-Based Deterministic Sharding
//!
//! Enforces deterministic lane-based sharding by account/partition key so miniblock
//! verification/execution parallelizes cleanly with bounded worst-case cost.

use super::{BlockError, Hash, LaneId, MiniblockBody, TxReceipt};
use crate::canonical;
use crate::hash_merkle::merkle_root;
use dchat_chain::{
    CreateChannelTx, JoinChannelTx, PostToChannelTx, RegisterUserTx, SendDirectMessageTx,
    SubmitDeliveryProofTx, Transaction, TransactionType,
};

// ─────────────────────────────────────────────────────────────────────────────
// Body Commitments
// ─────────────────────────────────────────────────────────────────────────────

/// Computed commitments from a miniblock body
#[derive(Debug, Clone)]
pub struct BodyCommitments {
    pub tx_root: Hash,
    pub receipts_root: Hash,
    pub tx_count: u16,
    pub body_bytes: u32,
    pub sigchecks: u16,
}

/// Compute canonical commitments from a miniblock body.
///
/// This function validates lane sharding and computes domain-separated Merkle roots.
pub fn compute_body_commitments(
    lane: LaneId,
    body: &MiniblockBody,
) -> Result<BodyCommitments, BlockError> {
    // Enforce lane sharding deterministically
    validate_lane_sharding(lane, &body.transactions)?;

    let tx_count = body.transactions.len();
    let mut body_bytes: usize = 0;
    let mut sigchecks: u16 = 0;

    // Tx leaf hash list
    let mut tx_leaves = Vec::with_capacity(tx_count);
    for tx in &body.transactions {
        body_bytes = body_bytes
            .saturating_add(tx.payload.len())
            .saturating_add(tx.tx_hash.len())
            .saturating_add(64);

        // Count signature checks based on tx type
        sigchecks = sigchecks.saturating_add(sigchecks_for_tx(tx));

        let leaf = canonical::domain_hash_parts(
            canonical::DOMAIN_SEP_TX_LEAF_V1,
            &[
                tx.tx_id.as_bytes(),
                tx.tx_hash.as_bytes(),
                &[tx_type_id(tx.tx_type)],
            ],
        );
        tx_leaves.push(leaf);
    }

    // Receipt leaf list (must match tx count when present)
    if !body.receipts.is_empty() && body.receipts.len() != body.transactions.len() {
        return Err(BlockError::InvalidTransaction(
            "receipts length must match tx length".to_string(),
        ));
    }

    let mut receipt_leaves = Vec::new();
    if !body.receipts.is_empty() {
        receipt_leaves.reserve(body.receipts.len());
        for r in &body.receipts {
            let leaf = r.hash();
            receipt_leaves.push(leaf);
        }
    }

    let tx_root = merkle_root(&tx_leaves);
    let receipts_root = if receipt_leaves.is_empty() {
        Hash::ZERO
    } else {
        merkle_root(&receipt_leaves)
    };

    Ok(BodyCommitments {
        tx_root,
        receipts_root,
        tx_count: u16::try_from(tx_count).unwrap_or(u16::MAX),
        body_bytes: u32::try_from(body_bytes.min(u32::MAX as usize)).unwrap_or(u32::MAX),
        sigchecks,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Lane Assignment
// ─────────────────────────────────────────────────────────────────────────────

/// Deterministically derive the lane for a transaction.
///
/// This is used by producers to shard transactions into lane-scoped miniblocks.
/// The lane is derived from the transaction's primary account using BLAKE3.
pub fn lane_for_transaction(tx: &Transaction) -> Result<LaneId, BlockError> {
    let key_bytes: Vec<u8> = match tx.tx_type {
        TransactionType::RegisterUser => {
            let t: RegisterUserTx = serde_json::from_slice(&tx.payload)
                .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;
            t.user_id.to_string().into_bytes()
        }
        TransactionType::SendDirectMessage => {
            let t: SendDirectMessageTx = serde_json::from_slice(&tx.payload)
                .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;
            t.sender_id.to_string().into_bytes()
        }
        TransactionType::CreateChannel => {
            let t: CreateChannelTx = serde_json::from_slice(&tx.payload)
                .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;
            t.creator_id.to_string().into_bytes()
        }
        TransactionType::PostToChannel => {
            let t: PostToChannelTx = serde_json::from_slice(&tx.payload)
                .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;
            t.sender_id.to_string().into_bytes()
        }
        TransactionType::JoinChannel => {
            let t: JoinChannelTx = serde_json::from_slice(&tx.payload)
                .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;
            t.user_id.to_string().into_bytes()
        }
        TransactionType::UpdateProfile => {
            // Fallback: hash tx_hash (deterministic) for unknown payload.
            tx.tx_hash.as_bytes().to_vec()
        }
        TransactionType::SubmitDeliveryProof => {
            let t: SubmitDeliveryProofTx = serde_json::from_slice(&tx.payload)
                .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;
            t.recipient_id.to_string().into_bytes()
        }
    };

    let h = blake3::hash(&key_bytes);
    let lane_raw = u16::from_le_bytes(h.as_bytes()[0..2].try_into().unwrap_or([0u8; 2]));
    Ok(LaneId::new(lane_raw))
}

/// Validate that all transactions in a list belong to the specified lane.
pub fn validate_lane_sharding(lane: LaneId, txs: &[Transaction]) -> Result<(), BlockError> {
    for tx in txs {
        let tx_lane = lane_for_transaction(tx)?;
        if tx_lane != lane {
            return Err(BlockError::LaneViolation(format!(
                "tx {:?} belongs to lane {:?} but miniblock lane is {:?}",
                tx.tx_id, tx_lane, lane
            )));
        }
    }
    Ok(())
}

/// Shard transactions into lane-scoped groups.
///
/// Returns a map from LaneId to list of transactions for that lane.
pub fn shard_transactions(
    txs: Vec<Transaction>,
) -> Result<std::collections::HashMap<LaneId, Vec<Transaction>>, BlockError> {
    let mut shards = std::collections::HashMap::new();

    for tx in txs {
        let lane = lane_for_transaction(&tx)?;
        shards.entry(lane).or_insert_with(Vec::new).push(tx);
    }

    Ok(shards)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Get numeric ID for transaction type (for hashing)
fn tx_type_id(tx_type: TransactionType) -> u8 {
    match tx_type {
        TransactionType::RegisterUser => 1,
        TransactionType::SendDirectMessage => 2,
        TransactionType::CreateChannel => 3,
        TransactionType::PostToChannel => 4,
        TransactionType::JoinChannel => 5,
        TransactionType::UpdateProfile => 6,
        TransactionType::SubmitDeliveryProof => 7,
    }
}

/// Count signature checks required for a transaction
fn sigchecks_for_tx(tx: &Transaction) -> u16 {
    match tx.tx_type {
        TransactionType::SubmitDeliveryProof => 2, // relay + sender signatures
        TransactionType::RegisterUser => 1,
        TransactionType::SendDirectMessage => 1,
        TransactionType::CreateChannel => 1,
        TransactionType::PostToChannel => 1,
        TransactionType::JoinChannel => 1,
        TransactionType::UpdateProfile => 1,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Parallel Lane Executor
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for parallel lane execution
#[derive(Debug, Clone)]
pub struct ParallelLaneConfig {
    /// Maximum lanes to process in parallel
    pub max_parallel_lanes: usize,
    /// Timeout per lane execution (ms)
    pub lane_timeout_ms: u64,
    /// Enable rayon parallelism
    pub use_rayon: bool,
}

impl Default for ParallelLaneConfig {
    fn default() -> Self {
        Self {
            max_parallel_lanes: 10,
            lane_timeout_ms: 100,
            use_rayon: true,
        }
    }
}

/// Result of parallel lane execution
#[derive(Debug, Clone)]
pub struct LaneExecutionResult {
    pub lane: LaneId,
    pub success_count: u32,
    pub failure_count: u32,
    pub gas_used: u64,
    pub receipts: Vec<TxReceipt>,
    pub post_state_hash: Hash,
}

/// Execute multiple lanes in parallel
///
/// Each lane is executed independently and results are merged.
pub fn execute_lanes_parallel(
    lane_txs: std::collections::HashMap<LaneId, Vec<Transaction>>,
    config: &ParallelLaneConfig,
) -> Vec<LaneExecutionResult> {
    use rayon::prelude::*;

    let lanes: Vec<_> = lane_txs.into_iter().collect();

    if config.use_rayon && lanes.len() > 1 {
        lanes
            .into_par_iter()
            .map(|(lane, txs)| execute_single_lane(lane, txs))
            .collect()
    } else {
        lanes
            .into_iter()
            .map(|(lane, txs)| execute_single_lane(lane, txs))
            .collect()
    }
}

/// Execute a single lane's transactions
fn execute_single_lane(lane: LaneId, txs: Vec<Transaction>) -> LaneExecutionResult {
    let mut success_count = 0u32;
    let mut failure_count = 0u32;
    let mut gas_used = 0u64;
    let mut receipts = Vec::with_capacity(txs.len());

    for (idx, tx) in txs.iter().enumerate() {
        // Simulate execution (in production, this would call WorldState)
        let gas = estimate_tx_gas(tx);

        // Basic validation - mark as failure if gas exceeds limit
        let max_gas_per_tx: u64 = 15_000_000;
        if gas > max_gas_per_tx {
            failure_count += 1;
            receipts.push(TxReceipt::failure(
                tx.tx_id,
                idx as u32,
                "gas limit exceeded",
            ));
            continue;
        }

        success_count += 1;
        gas_used += gas;

        receipts.push(TxReceipt::success(
            tx.tx_id,
            idx as u32,
            gas,
            Hash::ZERO, // Would be computed from actual state
        ));
    }

    LaneExecutionResult {
        lane,
        success_count,
        failure_count,
        gas_used,
        receipts,
        post_state_hash: Hash::ZERO,
    }
}

/// Estimate gas for a transaction
fn estimate_tx_gas(tx: &Transaction) -> u64 {
    const BASE_GAS: u64 = 21000;

    match tx.tx_type {
        TransactionType::RegisterUser => BASE_GAS + 10000,
        TransactionType::SendDirectMessage => BASE_GAS + (tx.payload.len() as u64) / 100,
        TransactionType::CreateChannel => BASE_GAS + 50000,
        TransactionType::PostToChannel => BASE_GAS + (tx.payload.len() as u64) / 100,
        TransactionType::JoinChannel => BASE_GAS + 5000,
        TransactionType::UpdateProfile => BASE_GAS + 3000,
        TransactionType::SubmitDeliveryProof => BASE_GAS + 8000,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lane_assignment_determinism() {
        // Same transaction should always get same lane
        let tx1 = create_test_tx(TransactionType::RegisterUser, "user1");
        let tx2 = create_test_tx(TransactionType::RegisterUser, "user1");

        let lane1 = lane_for_transaction(&tx1).unwrap();
        let lane2 = lane_for_transaction(&tx2).unwrap();

        assert_eq!(lane1, lane2);
    }

    #[test]
    fn test_lane_sharding_validation() {
        let tx1 = create_test_tx(TransactionType::RegisterUser, "user1");
        let lane = lane_for_transaction(&tx1).unwrap();

        // Should pass with correct lane
        assert!(validate_lane_sharding(lane, &[tx1.clone()]).is_ok());

        // Different lane should fail
        let wrong_lane = LaneId::new(lane.index().wrapping_add(1));
        assert!(validate_lane_sharding(wrong_lane, &[tx1]).is_err());
    }

    #[test]
    fn test_shard_transactions() {
        let txs = vec![
            create_test_tx(TransactionType::RegisterUser, "user1"),
            create_test_tx(TransactionType::RegisterUser, "user2"),
            create_test_tx(TransactionType::RegisterUser, "user3"),
        ];

        let shards = shard_transactions(txs).unwrap();
        assert!(!shards.is_empty());

        // Verify all shards have valid lane assignments
        for (lane, lane_txs) in &shards {
            assert!(validate_lane_sharding(*lane, lane_txs).is_ok());
        }
    }

    fn create_test_tx(tx_type: TransactionType, user_hint: &str) -> Transaction {
        use dchat_core::types::UserId;

        let user_id = UserId::new();
        let payload = match tx_type {
            TransactionType::RegisterUser => {
                let register_tx = RegisterUserTx {
                    user_id: user_id.clone(),
                    username: format!("test_{}", user_hint),
                    public_key: hex::encode(vec![0u8; 32]),
                    timestamp: chrono::Utc::now(),
                    initial_reputation: 0,
                };
                serde_json::to_vec(&register_tx).unwrap()
            }
            _ => vec![],
        };

        Transaction::new(tx_type, payload)
    }
}
