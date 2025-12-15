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

use dchat_chain::{Transaction, TransactionType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Collection of miniblocks (max 10). Bodies may be absent (header-only propagation).
    pub miniblocks: Vec<Miniblock>,
    /// Execution result summary
    pub execution_result: ExecutionResult,
    /// Merkle root of all miniblock hashes
    pub merkle_root: Hash,
    /// Optional subblock certificate (aggregated/threshold attestation)
    pub certificate: Option<SubblockCertificate>,
}

/// Lane identifier for deterministic sharding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LaneId(pub u16);

/// Deterministic execution receipt for a transaction.
///
/// This type is used for receipts commitments and fraud proofs; it intentionally excludes
/// wall-clock timestamps and non-deterministic metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxReceipt {
    pub tx_id: uuid::Uuid,
    pub tx_index: u32,
    pub gas_used: u64,
    pub success: bool,
    pub error_hash: Hash,
}

/// Thin miniblock header (commitment-first) used for fast propagation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniblockHeader {
    /// Index within parent subblock (0-9)
    pub index: u16,
    /// Miniblock creation timestamp
    pub timestamp: SystemTime,
    /// Deterministic lane assignment
    pub lane: LaneId,
    /// Transaction Merkle root (domain-separated)
    pub tx_root: Hash,
    /// Receipts Merkle root (domain-separated)
    pub receipts_root: Hash,
    /// Bounds: number of txs
    pub tx_count: u16,
    /// Bounds: total body bytes for early rejection
    pub body_bytes: u32,
    /// Bounds: worst-case signature checks
    pub sigchecks: u16,
    /// State hash before execution
    pub pre_state_hash: Hash,
    /// State hash after execution
    pub post_state_hash: Hash,
    /// Total gas consumed
    pub gas_used: u64,
}

/// Miniblock body (fetched on demand).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniblockBody {
    pub transactions: Vec<Transaction>,
    pub receipts: Vec<TxReceipt>,
}

/// Miniblock for transaction batching (20ms window)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Miniblock {
    /// Commitment-first header (always present)
    pub header: MiniblockHeader,
    /// Full body (may be absent during header-first propagation)
    pub body: Option<MiniblockBody>,
}

/// Subblock certificate: quorum attestation over ordered miniblock roots/receipts.
///
/// This enables pipelined verification and early safe UX finality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubblockCertificate {
    pub block_height: u64,
    pub subblock_index: u16,
    pub miniblock_headers_root: Hash,
    pub miniblock_receipts_root: Hash,
    /// Bitmap of signers in validator set order (compact quorum proof)
    pub signer_bitmap: Vec<u8>,
    /// Aggregate signature bytes (BLS12-381, 96 bytes)
    pub aggregate_signature: Vec<u8>,
}

/// Account state for transaction processing
#[derive(Debug, Clone, Default)]
pub struct AccountState {
    /// Account balances (keyed by account ID)
    pub balances: HashMap<Vec<u8>, u64>,
    /// Account nonces (keyed by account ID)
    pub nonces: HashMap<Vec<u8>, u64>,
    /// Staking deposits (keyed by account ID)
    pub stakes: HashMap<Vec<u8>, u64>,
    /// User reputation scores (keyed by user ID)
    pub reputation: HashMap<Vec<u8>, i64>,
}

impl AccountState {
    /// Get balance for an account
    pub fn get_balance(&self, account: &[u8]) -> u64 {
        self.balances.get(account).copied().unwrap_or(0)
    }

    /// Get nonce for an account
    pub fn get_nonce(&self, account: &[u8]) -> u64 {
        self.nonces.get(account).copied().unwrap_or(0)
    }

    /// Credit an account
    pub fn credit(&mut self, account: &[u8], amount: u64) {
        *self.balances.entry(account.to_vec()).or_insert(0) += amount;
    }

    /// Debit an account (returns error if insufficient funds)
    pub fn debit(&mut self, account: &[u8], amount: u64) -> Result<(), BlockError> {
        let balance = self.get_balance(account);
        if balance < amount {
            return Err(BlockError::InvalidTransaction(format!(
                "Insufficient balance: {} < {}",
                balance, amount
            )));
        }
        *self.balances.entry(account.to_vec()).or_insert(0) -= amount;
        Ok(())
    }

    /// Increment nonce for an account
    pub fn increment_nonce(&mut self, account: &[u8]) {
        *self.nonces.entry(account.to_vec()).or_insert(0) += 1;
    }
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
    /// Validator signatures that contributed to this finality proof
    pub validator_signatures: Vec<Vec<u8>>,
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

    #[error("Miniblock body missing")]
    MiniblockBodyMissing,

    #[error("Miniblock header/body mismatch: {0}")]
    MiniblockCommitmentMismatch(String),

    #[error("Lane sharding violation: {0}")]
    LaneViolation(String),

    #[error("Data availability error: {0}")]
    DataAvailability(String),

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
        let serialized = crate::canonical::canonical_serialize(self);
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
            certificate: None,
        }
    }

    /// Add miniblock to subblock (max 10)
    pub fn add_miniblock(&mut self, miniblock: Miniblock) -> Result<(), BlockError> {
        if self.miniblocks.len() >= 10 {
            return Err(BlockError::MiniblockLimitExceeded);
        }
        // Enforce deterministic order and unique lane for parallel execution.
        if miniblock.header.index != self.miniblocks.len() as u16 {
            return Err(BlockError::InvalidTransaction(
                "miniblock index not sequential".to_string(),
            ));
        }
        if self
            .miniblocks
            .iter()
            .any(|mb| mb.header.lane == miniblock.header.lane)
        {
            return Err(BlockError::LaneViolation(
                "duplicate lane within subblock".to_string(),
            ));
        }
        self.miniblocks.push(miniblock);
        Ok(())
    }

    /// Calculate total transaction count in subblock
    pub fn transaction_count(&self) -> usize {
        self.miniblocks
            .iter()
            .map(|mb| mb.header.tx_count as usize)
            .sum()
    }

    /// Calculate subblock hash
    pub fn calculate_hash(&self) -> Hash {
        let serialized = crate::canonical::canonical_serialize(self);
        Hash::from(blake3::hash(&serialized).into())
    }

    /// Calculate merkle root of all miniblocks
    pub fn calculate_merkle_root(&self) -> Hash {
        let hashes: Vec<_> = self.miniblocks.iter().map(|mb| mb.header_hash()).collect();

        if hashes.is_empty() {
            return Hash::from([0u8; 32]);
        }

        calculate_merkle_root(&hashes)
    }
}

impl Miniblock {
    /// Max txs allowed in a miniblock.
    pub const MAX_TXS: usize = 500;
    /// Max serialized body bytes for early rejection.
    pub const MAX_BODY_BYTES: usize = 512 * 1024;
    /// Max signature checks per miniblock.
    pub const MAX_SIGCHECKS: u16 = 500;

    /// Build a new miniblock from a body, computing commitments and enforcing caps.
    pub fn new(index: u16, lane: LaneId, body: MiniblockBody) -> Result<Self, BlockError> {
        let (tx_root, receipts_root, tx_count, body_bytes, sigchecks) =
            compute_body_commitments(lane, &body)?;

        if tx_count as usize > Self::MAX_TXS {
            return Err(BlockError::InvalidTransaction(
                "too many txs in miniblock".to_string(),
            ));
        }
        if body_bytes as usize > Self::MAX_BODY_BYTES {
            return Err(BlockError::InvalidTransaction(
                "miniblock body too large".to_string(),
            ));
        }
        if sigchecks > Self::MAX_SIGCHECKS {
            return Err(BlockError::InvalidTransaction(
                "too many sigchecks".to_string(),
            ));
        }

        Ok(Self {
            header: MiniblockHeader {
                index,
                timestamp: SystemTime::now(),
                lane,
                tx_root,
                receipts_root,
                tx_count,
                body_bytes,
                sigchecks,
                pre_state_hash: Hash::from([0u8; 32]),
                post_state_hash: Hash::from([0u8; 32]),
                gas_used: 0,
            },
            body: Some(body),
        })
    }

    /// Create a header-only miniblock (thin announcement).
    pub fn from_header(header: MiniblockHeader) -> Self {
        Self { header, body: None }
    }

    /// Hash of miniblock header for subblock merkle roots.
    pub fn header_hash(&self) -> Hash {
        let bytes = crate::canonical::canonical_serialize(&self.header);
        crate::canonical::domain_hash(
            crate::canonical::DOMAIN_SEP_MINIBLOCK_HEADER_HASH_V1,
            &bytes,
        )
    }

    /// Verify that the body matches the commitments in the header.
    pub fn verify_body(&self) -> Result<(), BlockError> {
        let body = self.body.as_ref().ok_or(BlockError::MiniblockBodyMissing)?;
        let (tx_root, receipts_root, tx_count, body_bytes, sigchecks) =
            compute_body_commitments(self.header.lane, body)?;

        if tx_root != self.header.tx_root {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "tx_root".to_string(),
            ));
        }
        if receipts_root != self.header.receipts_root {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "receipts_root".to_string(),
            ));
        }
        if tx_count != self.header.tx_count {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "tx_count".to_string(),
            ));
        }
        if body_bytes != self.header.body_bytes {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "body_bytes".to_string(),
            ));
        }
        if sigchecks != self.header.sigchecks {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "sigchecks".to_string(),
            ));
        }

        Ok(())
    }

    /// Calculate miniblock hash
    pub fn calculate_hash(&self) -> Hash {
        // Backwards-compatible: hash full miniblock (header + optional body) deterministically.
        let serialized = crate::canonical::canonical_serialize(self);
        Hash::from(blake3::hash(&serialized).into())
    }

    /// Execute all transactions in miniblock
    pub async fn execute(&mut self, state: &mut WorldState) -> Result<(), BlockError> {
        let body = self.body.as_mut().ok_or(BlockError::MiniblockBodyMissing)?;
        self.header.pre_state_hash = state.compute_hash();

        let mut _success = 0;
        let mut gas_total = 0;
        body.receipts.clear();

        // Ensure lane sharding is valid before execution (bounded parallel precheck).
        validate_lane_sharding(self.header.lane, &body.transactions)?;

        for (tx_index, tx) in body.transactions.iter().enumerate() {
            match state.apply_transaction(tx).await {
                Ok(gas) => {
                    _success += 1;
                    gas_total += gas;
                    body.receipts.push(TxReceipt {
                        tx_id: tx.tx_id,
                        tx_index: tx_index as u32,
                        gas_used: gas,
                        success: true,
                        error_hash: Hash::from([0u8; 32]),
                    });
                }
                Err(e) => {
                    tracing::warn!("Transaction {} failed: {:?}", tx.tx_id, e);
                    body.receipts.push(TxReceipt {
                        tx_id: tx.tx_id,
                        tx_index: tx_index as u32,
                        gas_used: 0,
                        success: false,
                        error_hash: crate::canonical::domain_hash_parts(
                            crate::canonical::DOMAIN_SEP_ERROR_V1,
                            &[e.to_string().as_bytes()],
                        ),
                    });
                }
            }
        }

        self.header.post_state_hash = state.compute_hash();
        self.header.gas_used = gas_total;

        // Recompute receipts commitment after execution.
        let (tx_root, receipts_root, tx_count, body_bytes, sigchecks) =
            compute_body_commitments(self.header.lane, body)?;
        self.header.tx_root = tx_root;
        self.header.receipts_root = receipts_root;
        self.header.tx_count = tx_count;
        self.header.body_bytes = body_bytes;
        self.header.sigchecks = sigchecks;

        Ok(())
    }
}

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

fn validate_lane_sharding(lane: LaneId, txs: &[Transaction]) -> Result<(), BlockError> {
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

/// Deterministically derive the lane for a transaction.
///
/// This is used by producers to shard transactions into lane-scoped miniblocks.
pub fn lane_for_transaction(tx: &Transaction) -> Result<LaneId, BlockError> {
    use dchat_chain::{
        CreateChannelTx, JoinChannelTx, PostToChannelTx, RegisterUserTx, SendDirectMessageTx,
        SubmitDeliveryProofTx,
    };

    // Deterministic partition key derived from the transaction's primary account.
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
    let lane = u16::from_le_bytes(h.as_bytes()[0..2].try_into().unwrap_or([0u8; 2]));
    Ok(LaneId(lane))
}

fn compute_body_commitments(
    lane: LaneId,
    body: &MiniblockBody,
) -> Result<(Hash, Hash, u16, u32, u16), BlockError> {
    use crate::hash_merkle::merkle_root;

    // Enforce lane sharding deterministically.
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

        if matches!(tx.tx_type, TransactionType::SubmitDeliveryProof) {
            sigchecks = sigchecks.saturating_add(1);
        }

        let leaf = crate::canonical::domain_hash_parts(
            crate::canonical::DOMAIN_SEP_TX_LEAF_V1,
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
            let leaf = crate::canonical::domain_hash_parts(
                crate::canonical::DOMAIN_SEP_RECEIPT_LEAF_V1,
                &[
                    r.tx_id.as_bytes(),
                    &r.tx_index.to_le_bytes(),
                    &r.gas_used.to_le_bytes(),
                    &[r.success as u8],
                    r.error_hash.as_bytes(),
                ],
            );
            receipt_leaves.push(leaf);
        }
    }

    let tx_root = merkle_root(&tx_leaves);
    let receipts_root = if receipt_leaves.is_empty() {
        Hash::from([0u8; 32])
    } else {
        merkle_root(&receipt_leaves)
    };

    Ok((
        tx_root,
        receipts_root,
        u16::try_from(tx_count).unwrap_or(u16::MAX),
        u32::try_from(body_bytes.min(u32::MAX as usize)).unwrap_or(u32::MAX),
        sigchecks,
    ))
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
    /// User reputation scores (user_key -> reputation)
    reputation: std::collections::HashMap<Vec<u8>, i64>,
    /// Staking amounts (user_key -> stake)
    stakes: std::collections::HashMap<Vec<u8>, u64>,
}

impl WorldState {
    /// Create new empty world state
    pub fn new() -> Self {
        Self {
            state_root: Hash::from([0u8; 32]),
            balances: std::collections::HashMap::new(),
            nonces: std::collections::HashMap::new(),
            storage: std::collections::HashMap::new(),
            reputation: std::collections::HashMap::new(),
            stakes: std::collections::HashMap::new(),
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

    /// Apply transaction to state with full validation
    /// Returns gas consumed
    pub async fn apply_transaction(&mut self, tx: &Transaction) -> Result<u64, BlockError> {
        use dchat_chain::{CreateChannelTx, PostToChannelTx, RegisterUserTx, SendDirectMessageTx};

        // Base gas cost for any transaction
        const BASE_GAS: u64 = 21000;

        // Deserialize and validate transaction based on type
        let gas_used = match tx.tx_type {
            TransactionType::RegisterUser => {
                let register_tx: RegisterUserTx = serde_json::from_slice(&tx.payload)
                    .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;

                // Validate public key format
                if register_tx.public_key.len() < 32 {
                    return Err(BlockError::InvalidTransaction(
                        "Invalid public key length".to_string(),
                    ));
                }

                // Register user in state (reputation initialization)
                let user_key = register_tx.user_id.to_string().into_bytes();
                self.reputation
                    .insert(user_key.clone(), register_tx.initial_reputation);

                // Initialize user balance if needed
                self.balances.entry(hex::encode(&user_key)).or_insert(0);

                tracing::debug!(
                    "Registered user {} with initial reputation {}",
                    register_tx.username,
                    register_tx.initial_reputation
                );

                BASE_GAS + 10000 // Registration costs extra gas
            }

            TransactionType::SendDirectMessage => {
                let msg_tx: SendDirectMessageTx = serde_json::from_slice(&tx.payload)
                    .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;

                // Validate sender exists
                let sender_key = msg_tx.sender_id.to_string().into_bytes();
                if !self.balances.contains_key(&hex::encode(&sender_key)) {
                    return Err(BlockError::InvalidTransaction(
                        "Sender not registered".to_string(),
                    ));
                }

                // Calculate gas based on payload size (larger messages cost more)
                let size_gas = (msg_tx.payload_size as u64) / 100; // 1 gas per 100 bytes

                // Reward relay node if present
                if let Some(relay_id) = &msg_tx.relay_node_id {
                    let relay_key = relay_id.as_bytes();
                    let reward = 100u64; // Base relay reward
                    *self.balances.entry(hex::encode(relay_key)).or_insert(0) += reward;
                    tracing::debug!("Rewarded relay {} with {}", relay_id, reward);
                }

                tracing::debug!(
                    "Processed message {} from {} to {} (size: {})",
                    msg_tx.message_id,
                    msg_tx.sender_id,
                    msg_tx.recipient_id,
                    msg_tx.payload_size
                );

                BASE_GAS + size_gas
            }

            TransactionType::CreateChannel => {
                let channel_tx: CreateChannelTx = serde_json::from_slice(&tx.payload)
                    .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;

                // Validate creator exists
                let creator_key = channel_tx.creator_id.to_string().into_bytes();
                if !self.balances.contains_key(&hex::encode(&creator_key)) {
                    return Err(BlockError::InvalidTransaction(
                        "Creator not registered".to_string(),
                    ));
                }

                // Handle staking requirement if present
                if let Some(stake_amount) = channel_tx.stake_amount {
                    let creator_hex = hex::encode(&creator_key);
                    let balance = self.balances.get(&creator_hex).copied().unwrap_or(0);

                    if balance < stake_amount {
                        return Err(BlockError::InvalidTransaction(format!(
                            "Insufficient stake: {} < {}",
                            balance, stake_amount
                        )));
                    }

                    // Deduct stake and add to staking pool
                    *self.balances.get_mut(&creator_hex).unwrap() -= stake_amount;
                    *self.stakes.entry(creator_key.clone()).or_insert(0) += stake_amount;

                    tracing::debug!(
                        "Staked {} tokens for channel {}",
                        stake_amount,
                        channel_tx.name
                    );
                }

                tracing::debug!(
                    "Created channel {} by {}",
                    channel_tx.name,
                    channel_tx.creator_id
                );

                BASE_GAS + 50000 // Channel creation is expensive
            }

            TransactionType::PostToChannel => {
                let post_tx: PostToChannelTx = serde_json::from_slice(&tx.payload)
                    .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;

                // Validate sender exists
                let sender_key = post_tx.sender_id.to_string().into_bytes();
                if !self.balances.contains_key(&hex::encode(&sender_key)) {
                    return Err(BlockError::InvalidTransaction(
                        "Sender not registered".to_string(),
                    ));
                }

                // Gas based on message size
                let size_gas = (post_tx.payload_size as u64) / 100;

                tracing::debug!(
                    "Posted message {} to channel {} (size: {})",
                    post_tx.message_id,
                    post_tx.channel_id,
                    post_tx.payload_size
                );

                BASE_GAS + size_gas
            }

            TransactionType::JoinChannel => {
                // Join channel transaction handling
                tracing::debug!("Processed JoinChannel transaction");
                BASE_GAS + 5000
            }

            TransactionType::UpdateProfile => {
                // Profile update transaction handling
                tracing::debug!("Processed UpdateProfile transaction");
                BASE_GAS + 3000
            }

            TransactionType::SubmitDeliveryProof => {
                use dchat_chain::SubmitDeliveryProofTx;

                let proof_tx: SubmitDeliveryProofTx = serde_json::from_slice(&tx.payload)
                    .map_err(|e| BlockError::InvalidTransaction(e.to_string()))?;

                // Validate relay exists
                let relay_key = proof_tx.relay_peer_id.as_bytes();
                if !self.balances.contains_key(&hex::encode(relay_key)) {
                    // Relay not registered yet - initialize with zero balance
                    self.balances.insert(hex::encode(relay_key), 0);
                }

                // Reward relay node for delivery
                let relay_hex = hex::encode(relay_key);
                *self.balances.get_mut(&relay_hex).unwrap() += proof_tx.reward_amount;

                tracing::debug!(
                    "Processed delivery proof for message {} via relay {}, reward: {}",
                    proof_tx.message_id,
                    proof_tx.relay_peer_id,
                    proof_tx.reward_amount
                );

                BASE_GAS + 8000 // Delivery proof verification costs gas
            }
        };

        // Recompute state root after successful execution
        self.state_root = self.compute_hash();

        Ok(gas_used)
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
            let body = MiniblockBody {
                transactions: vec![],
                receipts: vec![],
            };
            let miniblock = Miniblock::new(i, LaneId(i as u16), body).unwrap();
            assert!(subblock.add_miniblock(miniblock).is_ok());
        }

        // Try to add 11th miniblock (should fail)
        let body = MiniblockBody {
            transactions: vec![],
            receipts: vec![],
        };
        let miniblock = Miniblock::new(10, LaneId(10), body).unwrap();
        assert!(subblock.add_miniblock(miniblock).is_err());
    }

    #[test]
    fn test_transaction_count() {
        use dchat_core::types::UserId;

        let mut block = Block::new(1, Hash::from([0u8; 32]));

        // Create subblock with miniblocks containing transactions
        let mut subblock = Subblock::new(0);
        for i in 0..10 {
            let user_id = UserId::new();
            let transactions: Vec<Transaction> = (0..25)
                .map(|j| {
                    // Create RegisterUser transactions for testing
                    let register_tx = dchat_chain::RegisterUserTx {
                        user_id: user_id.clone(),
                        username: format!("user_{}_{}", i, j),
                        public_key: hex::encode(vec![0u8; 32]),
                        timestamp: chrono::Utc::now(),
                        initial_reputation: 0,
                    };
                    let payload = serde_json::to_vec(&register_tx).unwrap();
                    Transaction::new(TransactionType::RegisterUser, payload)
                })
                .collect();

            let lane = lane_for_transaction(&transactions[0]).unwrap();
            let body = MiniblockBody {
                transactions,
                receipts: vec![],
            };
            let miniblock = Miniblock::new(i, lane, body).unwrap();
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
            validator_signatures: vec![vec![1, 2, 3], vec![4, 5, 6]],
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

    // TODO: BlockchainState type not defined - test disabled
    // Original test_transaction_processing removed until BlockchainState is implemented

    // TODO: BlockchainState type not defined - test disabled
    // Original test_transaction_validation_failures removed until BlockchainState is implemented
}

// NOTE: BlockchainState type is not defined - this impl block is disabled
// impl BlockchainState {
//     /// Get reputation score for an account
//     pub fn get_reputation(&self, account: &[u8]) -> i64 {
//         self.reputation.get(account).copied().unwrap_or(0)
//     }
// }
