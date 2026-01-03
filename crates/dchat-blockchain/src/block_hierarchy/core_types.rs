//! Core Types for Block Hierarchy
//!
//! Defines the fundamental data structures for the Block→Subblock→Miniblock pipeline
//! with commitment-first, parallel, and fraud-provable design.

use crate::canonical;
use dchat_chain::Transaction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use thiserror::Error;

// ─────────────────────────────────────────────────────────────────────────────
// Hash Type
// ─────────────────────────────────────────────────────────────────────────────

/// Blake3 hash wrapper with serde support
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash(blake3::Hash);

impl Hash {
    pub const ZERO: Self = Self(blake3::Hash::from_bytes([0u8; 32]));

    pub fn from(bytes: [u8; 32]) -> Self {
        Self(blake3::Hash::from(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

impl Default for Hash {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<blake3::Hash> for Hash {
    fn from(h: blake3::Hash) -> Self {
        Self(h)
    }
}

impl From<[u8; 32]> for Hash {
    fn from(bytes: [u8; 32]) -> Self {
        Self::from(bytes)
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
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

// ─────────────────────────────────────────────────────────────────────────────
// Lane Identifier for Deterministic Sharding
// ─────────────────────────────────────────────────────────────────────────────

/// Lane identifier for deterministic sharding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct LaneId(pub u16);

impl LaneId {
    /// Maximum number of lanes (power of 2 for efficient sharding)
    pub const MAX_LANES: u16 = 256;

    /// Create a new lane ID, clamped to valid range
    pub fn new(id: u16) -> Self {
        Self(id % Self::MAX_LANES)
    }

    /// Get the lane index
    pub fn index(&self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Execution Transaction (Low-Level for State Machine)
// ─────────────────────────────────────────────────────────────────────────────

/// Account address (32 bytes)
pub type Address = [u8; 32];

/// Low-level transaction for execution engine.
/// This is the canonical form used by the state machine, distinct from
/// dchat_chain::Transaction which is the high-level envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionTransaction {
    /// Unique transaction ID (derived from hash)
    pub tx_id: uuid::Uuid,
    /// Transaction hash (for deduplication and references)
    pub hash: Hash,
    /// Sender address (32-byte account identifier)
    pub sender: Address,
    /// Recipient address (None for contract creation)
    pub recipient: Option<Address>,
    /// Value to transfer (in base units)
    pub value: u64,
    /// Transaction payload (calldata or init code)
    pub payload: Vec<u8>,
    /// Sender's nonce (replay protection)
    pub nonce: u64,
    /// Maximum gas units for this transaction
    pub gas_limit: u64,
    /// Price per gas unit (in base units)
    pub gas_price: u64,
    /// Cryptographic signature
    pub signature: Vec<u8>,
}

impl ExecutionTransaction {
    /// Base gas cost per transaction
    pub const BASE_GAS: u64 = 21_000;
    /// Gas cost per byte of payload
    pub const GAS_PER_BYTE: u64 = 16;

    /// Compute transaction hash
    pub fn compute_hash(&self) -> Hash {
        let mut data = Vec::with_capacity(128 + self.payload.len());
        data.extend_from_slice(&self.sender);
        if let Some(ref r) = self.recipient {
            data.push(1);
            data.extend_from_slice(r);
        } else {
            data.push(0);
        }
        data.extend_from_slice(&self.value.to_le_bytes());
        data.extend_from_slice(&self.payload);
        data.extend_from_slice(&self.nonce.to_le_bytes());
        data.extend_from_slice(&self.gas_limit.to_le_bytes());
        data.extend_from_slice(&self.gas_price.to_le_bytes());
        canonical::domain_hash(canonical::DOMAIN_SEP_TX_V1, &data)
    }

    /// Compute gas cost for this transaction
    pub fn gas_cost(&self) -> u64 {
        Self::BASE_GAS + self.payload.len() as u64 * Self::GAS_PER_BYTE
    }

    /// Get execution lane (derived from sender address via BLAKE3)
    pub fn lane(&self) -> LaneId {
        let h = blake3::hash(&self.sender);
        let lane_raw = u16::from_le_bytes([h.as_bytes()[0], h.as_bytes()[1]]);
        LaneId::new(lane_raw)
    }

    /// Try to convert from high-level dchat_chain::Transaction
    /// Returns None if the transaction payload cannot be parsed as ExecutionTransaction
    pub fn from_chain_transaction(chain_tx: &Transaction) -> Option<Self> {
        // The chain transaction's payload should contain serialized execution data
        // Try to deserialize it
        bincode::deserialize(&chain_tx.payload).ok()
    }

    /// Create an ExecutionTransaction from raw components with deterministic ID
    pub fn new(
        sender: Address,
        recipient: Option<Address>,
        value: u64,
        payload: Vec<u8>,
        nonce: u64,
        gas_limit: u64,
        gas_price: u64,
        signature: Vec<u8>,
    ) -> Self {
        let mut tx = Self {
            tx_id: uuid::Uuid::nil(), // Initial value, computed below
            hash: Hash::ZERO,         // Initial value, computed below
            sender,
            recipient,
            value,
            payload,
            nonce,
            gas_limit,
            gas_price,
            signature,
        };
        // Compute hash and derive tx_id deterministically
        tx.hash = tx.compute_hash();
        tx.tx_id = uuid::Uuid::from_bytes({
            let h = tx.hash.as_bytes();
            [
                h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8], h[9], h[10], h[11], h[12],
                h[13], h[14], h[15],
            ]
        });
        tx
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Transaction Receipt
// ─────────────────────────────────────────────────────────────────────────────

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
    /// State root after this transaction executed
    pub post_state_root: Hash,
    /// Log bloom filter for efficient filtering (encoded as Vec for serde)
    #[serde(with = "serde_bytes")]
    pub log_bloom: Vec<u8>,
}

impl TxReceipt {
    /// Create a successful receipt
    pub fn success(tx_id: uuid::Uuid, tx_index: u32, gas_used: u64, post_state_root: Hash) -> Self {
        Self {
            tx_id,
            tx_index,
            gas_used,
            success: true,
            error_hash: Hash::ZERO,
            post_state_root,
            log_bloom: vec![0u8; 256],
        }
    }

    /// Create a failed receipt
    pub fn failure(tx_id: uuid::Uuid, tx_index: u32, error: &str) -> Self {
        Self {
            tx_id,
            tx_index,
            gas_used: 0,
            success: false,
            error_hash: canonical::domain_hash(canonical::DOMAIN_SEP_ERROR_V1, error.as_bytes()),
            post_state_root: Hash::ZERO,
            log_bloom: vec![0u8; 256],
        }
    }

    /// Compute deterministic hash of receipt
    pub fn hash(&self) -> Hash {
        canonical::domain_hash_parts(
            canonical::DOMAIN_SEP_RECEIPT_LEAF_V1,
            &[
                self.tx_id.as_bytes(),
                &self.tx_index.to_le_bytes(),
                &self.gas_used.to_le_bytes(),
                &[self.success as u8],
                self.error_hash.as_bytes(),
                self.post_state_root.as_bytes(),
            ],
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Miniblock Header (Thin, Commitment-First)
// ─────────────────────────────────────────────────────────────────────────────

/// Thin miniblock header (commitment-first) used for fast propagation.
///
/// This is the core of the header-first announcement pattern. Contains only
/// canonical, domain-separated commitments that can be verified without
/// fetching the full body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MiniblockHeader {
    /// Block height this miniblock belongs to
    pub block_height: u64,
    /// Subblock index within parent block (0-9)
    pub subblock_index: u16,
    /// Index within parent subblock (0-9)
    pub index: u16,
    /// Miniblock creation timestamp (for ordering, not determinism)
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
    /// Gas limit for this miniblock
    pub gas_limit: u64,
    /// Producer's public key hash (for accountability)
    pub producer: Hash,
}

impl MiniblockHeader {
    /// Compute canonical hash of header
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(canonical::DOMAIN_SEP_MINIBLOCK_HEADER_HASH_V1, &bytes)
    }

    /// Validate header bounds without needing body
    pub fn validate_bounds(&self) -> Result<(), BlockError> {
        if self.tx_count as usize > Miniblock::MAX_TXS {
            return Err(BlockError::TxLimitExceeded {
                count: self.tx_count as usize,
                max: Miniblock::MAX_TXS,
            });
        }
        if self.body_bytes as usize > Miniblock::MAX_BODY_BYTES {
            return Err(BlockError::BodySizeExceeded {
                size: self.body_bytes as usize,
                max: Miniblock::MAX_BODY_BYTES,
            });
        }
        if self.sigchecks > Miniblock::MAX_SIGCHECKS {
            return Err(BlockError::SigcheckLimitExceeded {
                count: self.sigchecks,
                max: Miniblock::MAX_SIGCHECKS,
            });
        }
        if self.gas_used > self.gas_limit {
            return Err(BlockError::GasLimitExceeded {
                used: self.gas_used,
                limit: self.gas_limit,
            });
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Miniblock Body (Fetched On-Demand)
// ─────────────────────────────────────────────────────────────────────────────

/// Miniblock body (fetched on demand).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniblockBody {
    pub transactions: Vec<Transaction>,
    pub receipts: Vec<TxReceipt>,
}

impl MiniblockBody {
    /// Create empty body
    pub fn empty() -> Self {
        Self {
            transactions: Vec::new(),
            receipts: Vec::new(),
        }
    }

    /// Compute total serialized size
    pub fn serialized_size(&self) -> usize {
        canonical::canonical_serialize(self).len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Miniblock (Header + Optional Body)
// ─────────────────────────────────────────────────────────────────────────────

/// Miniblock for transaction batching (20ms window)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Miniblock {
    /// Commitment-first header (always present)
    pub header: MiniblockHeader,
    /// Full body (may be absent during header-first propagation)
    pub body: Option<MiniblockBody>,
}

impl Miniblock {
    /// Max txs allowed in a miniblock.
    pub const MAX_TXS: usize = 500;
    /// Max serialized body bytes for early rejection.
    pub const MAX_BODY_BYTES: usize = 512 * 1024; // 512 KB
    /// Max signature checks per miniblock.
    pub const MAX_SIGCHECKS: u16 = 500;
    /// Default gas limit per miniblock.
    pub const DEFAULT_GAS_LIMIT: u64 = 30_000_000;

    /// Create a new miniblock with all required context.
    /// Computes header commitments from body automatically.
    pub fn new(
        block_height: u64,
        subblock_index: u16,
        index: u16,
        lane: LaneId,
        body: MiniblockBody,
    ) -> Result<Self, BlockError> {
        use super::lane_sharding::compute_body_commitments;
        use std::time::SystemTime;

        // Compute commitments from body
        let commitments = compute_body_commitments(lane, &body)?;

        let header = MiniblockHeader {
            block_height,
            subblock_index,
            index,
            lane,
            timestamp: SystemTime::now(),
            tx_root: commitments.tx_root,
            receipts_root: commitments.receipts_root,
            tx_count: commitments.tx_count,
            body_bytes: commitments.body_bytes,
            sigchecks: commitments.sigchecks,
            pre_state_hash: Hash::ZERO,  // Set during execution
            post_state_hash: Hash::ZERO, // Set during execution
            gas_used: 0,                 // Set during execution
            gas_limit: Self::DEFAULT_GAS_LIMIT,
            producer: Hash::ZERO, // Set by producer
        };

        Ok(Self {
            header,
            body: Some(body),
        })
    }

    /// Create a header-only miniblock (thin announcement).
    pub fn from_header(header: MiniblockHeader) -> Self {
        Self { header, body: None }
    }

    /// Create a miniblock with body
    pub fn with_body(header: MiniblockHeader, body: MiniblockBody) -> Self {
        Self {
            header,
            body: Some(body),
        }
    }

    /// Hash of miniblock header for subblock merkle roots.
    pub fn header_hash(&self) -> Hash {
        self.header.hash()
    }

    /// Check if body is present
    pub fn has_body(&self) -> bool {
        self.body.is_some()
    }

    /// Get body reference or error
    pub fn body_ref(&self) -> Result<&MiniblockBody, BlockError> {
        self.body.as_ref().ok_or(BlockError::MiniblockBodyMissing)
    }

    /// Verify that the body matches the commitments in the header.
    pub fn verify_body(&self) -> Result<(), BlockError> {
        let body = self.body_ref()?;
        let commitments = super::lane_sharding::compute_body_commitments(self.header.lane, body)?;

        if commitments.tx_root != self.header.tx_root {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "tx_root".to_string(),
            ));
        }
        if commitments.receipts_root != self.header.receipts_root {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "receipts_root".to_string(),
            ));
        }
        if commitments.tx_count != self.header.tx_count {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "tx_count".to_string(),
            ));
        }
        if commitments.body_bytes != self.header.body_bytes {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "body_bytes".to_string(),
            ));
        }
        if commitments.sigchecks != self.header.sigchecks {
            return Err(BlockError::MiniblockCommitmentMismatch(
                "sigchecks".to_string(),
            ));
        }

        Ok(())
    }

    /// Calculate miniblock hash (full miniblock including body if present)
    pub fn calculate_hash(&self) -> Hash {
        let serialized = canonical::canonical_serialize(self);
        Hash::from(*blake3::hash(&serialized).as_bytes())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Subblock (Parallel Execution Unit)
// ─────────────────────────────────────────────────────────────────────────────

/// Subblock for parallel execution (200ms window)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subblock {
    /// Block height this subblock belongs to
    pub block_height: u64,
    /// Index within parent block (0-9)
    pub index: u16,
    /// Subblock creation timestamp
    pub timestamp: SystemTime,
    /// Collection of miniblocks (max 10). Bodies may be absent (header-only propagation).
    pub miniblocks: Vec<Miniblock>,
    /// Execution result summary
    pub execution_result: ExecutionResult,
    /// Merkle root of all miniblock headers
    pub miniblock_headers_root: Hash,
    /// Merkle root of all miniblock receipts roots
    pub miniblock_receipts_root: Hash,
    /// Optional subblock certificate (aggregated/threshold attestation)
    pub certificate: Option<super::SubblockCertificate>,
}

impl Subblock {
    /// Maximum miniblocks per subblock
    pub const MAX_MINIBLOCKS: usize = 10;

    /// Create new subblock with given index
    pub fn new(block_height: u64, index: u16) -> Self {
        Self {
            block_height,
            index,
            timestamp: SystemTime::now(),
            miniblocks: Vec::with_capacity(Self::MAX_MINIBLOCKS),
            execution_result: ExecutionResult::default(),
            miniblock_headers_root: Hash::ZERO,
            miniblock_receipts_root: Hash::ZERO,
            certificate: None,
        }
    }

    /// Add miniblock to subblock (max 10)
    pub fn add_miniblock(&mut self, miniblock: Miniblock) -> Result<(), BlockError> {
        if self.miniblocks.len() >= Self::MAX_MINIBLOCKS {
            return Err(BlockError::MiniblockLimitExceeded);
        }
        // Enforce deterministic order
        if miniblock.header.index != self.miniblocks.len() as u16 {
            return Err(BlockError::InvalidMiniblockOrder);
        }
        // Enforce unique lane for parallel execution
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
        let serialized = canonical::canonical_serialize(self);
        Hash::from(*blake3::hash(&serialized).as_bytes())
    }

    /// Calculate and update merkle roots from miniblocks
    pub fn update_merkle_roots(&mut self) {
        use crate::hash_merkle::merkle_root;

        let header_hashes: Vec<Hash> = self.miniblocks.iter().map(|mb| mb.header_hash()).collect();

        let receipts_roots: Vec<Hash> = self
            .miniblocks
            .iter()
            .map(|mb| mb.header.receipts_root)
            .collect();

        self.miniblock_headers_root = merkle_root(&header_hashes);
        self.miniblock_receipts_root = merkle_root(&receipts_roots);
    }

    /// Check if all miniblocks have bodies
    pub fn all_bodies_present(&self) -> bool {
        self.miniblocks.iter().all(|mb| mb.has_body())
    }

    /// Verify all miniblock commitments
    pub fn verify_all_miniblocks(&self) -> Result<(), BlockError> {
        for mb in &self.miniblocks {
            mb.verify_body()?;
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Block (Main Consensus Unit)
// ─────────────────────────────────────────────────────────────────────────────

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
    /// BFT validator signatures (5-of-7 required) - aggregated
    pub aggregated_signature: Option<super::AggregatedBlockSignature>,
    /// Legacy per-validator signatures (for backwards compatibility)
    pub validator_signatures: Vec<ValidatorSignature>,
    /// PoRW relay votes for consensus
    pub relay_votes: Vec<RelayVote>,
    /// Finality proof from consensus layers
    pub finality_proof: FinalityProof,
    /// Data availability commitment
    pub da_commitment: Option<super::DataAvailabilityCommitment>,
    /// Merkle root of governance state (finalized proposals, vote tallies)
    /// Commits governance decisions to the blockchain for verification.
    #[serde(default)]
    pub governance_merkle_root: Option<Hash>,
}

impl Block {
    /// Maximum subblocks per block
    pub const MAX_SUBBLOCKS: usize = 10;

    /// Create new block with given height and previous hash
    pub fn new(height: u64, previous_hash: Hash) -> Self {
        Self {
            height,
            timestamp: SystemTime::now(),
            previous_hash,
            state_root: Hash::ZERO,
            subblocks: Vec::with_capacity(Self::MAX_SUBBLOCKS),
            aggregated_signature: None,
            validator_signatures: Vec::new(),
            relay_votes: Vec::new(),
            finality_proof: FinalityProof::default(),
            da_commitment: None,
            governance_merkle_root: None,
        }
    }

    /// Add subblock to block (max 10)
    pub fn add_subblock(&mut self, subblock: Subblock) -> Result<(), BlockError> {
        if self.subblocks.len() >= Self::MAX_SUBBLOCKS {
            return Err(BlockError::SubblockLimitExceeded);
        }
        // Enforce deterministic order
        if subblock.index != self.subblocks.len() as u16 {
            return Err(BlockError::InvalidSubblockOrder);
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
        let serialized = canonical::canonical_serialize(self);
        Hash::from(*blake3::hash(&serialized).as_bytes())
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

        // 3. Verify signatures (aggregated or legacy)
        if let Some(ref agg_sig) = self.aggregated_signature {
            if !agg_sig.verify_quorum()? {
                return Err(BlockError::InsufficientSignatures);
            }
        } else if self.validator_signatures.len() < 5 {
            return Err(BlockError::InsufficientSignatures);
        }

        // 4. Verify DA commitment if present
        if let Some(ref da) = self.da_commitment {
            da.validate()?;
        }

        Ok(())
    }

    /// Check if block has reached finality
    pub fn is_finalized(&self) -> bool {
        self.finality_proof.is_finalized()
    }

    /// Get block header for propagation (without bodies)
    pub fn header_only(&self) -> BlockHeader {
        BlockHeader {
            height: self.height,
            timestamp: self.timestamp,
            previous_hash: self.previous_hash,
            state_root: self.state_root,
            subblock_count: self.subblocks.len() as u8,
            transaction_count: self.transaction_count() as u32,
            da_commitment: self.da_commitment.clone(),
            governance_merkle_root: self.governance_merkle_root,
        }
    }
}

/// Block header for fast propagation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub height: u64,
    pub timestamp: SystemTime,
    pub previous_hash: Hash,
    pub state_root: Hash,
    pub subblock_count: u8,
    pub transaction_count: u32,
    pub da_commitment: Option<super::DataAvailabilityCommitment>,
    /// Merkle root of governance state (finalized proposals, vote tallies)
    /// This commits the governance state to the block, enabling verification
    /// of governance-based revocations and on-chain governance proofs.
    #[serde(default)]
    pub governance_merkle_root: Option<Hash>,
}

impl BlockHeader {
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/block/header/v1", &bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Supporting Types
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
// Global Account State (for consensus-level state tracking)
// ─────────────────────────────────────────────────────────────────────────────

/// Global account state for consensus-level transaction processing.
/// This tracks all accounts in the system and is used by the block production pipeline.
/// Distinct from execution::AccountState which tracks individual account state.
#[derive(Debug, Clone, Default)]
pub struct GlobalAccountState {
    /// Account balances (keyed by account ID)
    pub balances: HashMap<Vec<u8>, u64>,
    /// Account nonces (keyed by account ID)
    pub nonces: HashMap<Vec<u8>, u64>,
    /// Staking deposits (keyed by account ID)
    pub stakes: HashMap<Vec<u8>, u64>,
    /// User reputation scores (keyed by user ID)
    pub reputation: HashMap<Vec<u8>, i64>,
}

impl GlobalAccountState {
    /// Create new empty global state
    pub fn new() -> Self {
        Self::default()
    }

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

    /// Get stake for an account
    pub fn get_stake(&self, account: &[u8]) -> u64 {
        self.stakes.get(account).copied().unwrap_or(0)
    }

    /// Add stake for an account
    pub fn add_stake(&mut self, account: &[u8], amount: u64) -> Result<(), BlockError> {
        // Must have balance to stake
        self.debit(account, amount)?;
        *self.stakes.entry(account.to_vec()).or_insert(0) += amount;
        Ok(())
    }

    /// Get reputation for a user
    pub fn get_reputation(&self, user: &[u8]) -> i64 {
        self.reputation.get(user).copied().unwrap_or(0)
    }

    /// Adjust reputation for a user
    pub fn adjust_reputation(&mut self, user: &[u8], delta: i64) {
        *self.reputation.entry(user.to_vec()).or_insert(0) += delta;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

/// Errors in block operations
#[derive(Debug, Error)]
pub enum BlockError {
    #[error("Subblock limit exceeded (max 10)")]
    SubblockLimitExceeded,

    #[error("Miniblock limit exceeded (max 10)")]
    MiniblockLimitExceeded,

    #[error("Invalid subblock order")]
    InvalidSubblockOrder,

    #[error("Invalid miniblock order")]
    InvalidMiniblockOrder,

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

    #[error("Transaction count limit exceeded: {count} > {max}")]
    TxLimitExceeded { count: usize, max: usize },

    #[error("Body size limit exceeded: {size} > {max}")]
    BodySizeExceeded { size: usize, max: usize },

    #[error("Signature check limit exceeded: {count} > {max}")]
    SigcheckLimitExceeded { count: u16, max: u16 },

    #[error("Gas limit exceeded: {used} > {limit}")]
    GasLimitExceeded { used: u64, limit: u64 },

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Signature verification failed: {0}")]
    SignatureVerification(String),

    #[error("Fraud proof: {0}")]
    FraudProof(String),

    #[error("Erasure coding error: {0}")]
    ErasureCoding(String),

    #[error("DA sampling failed: {0}")]
    DaSamplingFailed(String),

    #[error("Equivocation detected: {0}")]
    Equivocation(String),

    #[error("Certificate verification failed: {0}")]
    CertificateVerification(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_operations() {
        let h1 = Hash::from([1u8; 32]);
        let h2 = Hash::from([1u8; 32]);
        let h3 = Hash::from([2u8; 32]);

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(Hash::ZERO, Hash::default());
    }

    #[test]
    fn test_lane_id() {
        let lane = LaneId::new(1000);
        assert!(lane.index() < LaneId::MAX_LANES);
    }

    #[test]
    fn test_block_creation() {
        let block = Block::new(1, Hash::ZERO);
        assert_eq!(block.height, 1);
        assert_eq!(block.subblocks.len(), 0);
    }

    #[test]
    fn test_subblock_limit() {
        let mut block = Block::new(1, Hash::ZERO);

        for i in 0..10 {
            let subblock = Subblock::new(1, i);
            assert!(block.add_subblock(subblock).is_ok());
        }

        let subblock = Subblock::new(1, 10);
        assert!(matches!(
            block.add_subblock(subblock),
            Err(BlockError::SubblockLimitExceeded)
        ));
    }

    #[test]
    fn test_miniblock_header_validation() {
        let header = MiniblockHeader {
            block_height: 1,
            subblock_index: 0,
            index: 0,
            timestamp: SystemTime::now(),
            lane: LaneId(0),
            tx_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            tx_count: 100,
            body_bytes: 1024,
            sigchecks: 50,
            pre_state_hash: Hash::ZERO,
            post_state_hash: Hash::ZERO,
            gas_used: 1_000_000,
            gas_limit: 30_000_000,
            producer: Hash::ZERO,
        };

        assert!(header.validate_bounds().is_ok());

        // Test exceeding limits
        let mut bad_header = header.clone();
        bad_header.tx_count = 1000;
        assert!(matches!(
            bad_header.validate_bounds(),
            Err(BlockError::TxLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_global_account_state_operations() {
        let mut state = GlobalAccountState::new();
        let account = b"test_account_001";

        // Test initial state
        assert_eq!(state.get_balance(account), 0);
        assert_eq!(state.get_nonce(account), 0);
        assert_eq!(state.get_stake(account), 0);
        assert_eq!(state.get_reputation(account), 0);

        // Test credit
        state.credit(account, 1000);
        assert_eq!(state.get_balance(account), 1000);

        // Test debit
        assert!(state.debit(account, 500).is_ok());
        assert_eq!(state.get_balance(account), 500);

        // Test insufficient balance
        assert!(state.debit(account, 1000).is_err());

        // Test nonce
        state.increment_nonce(account);
        assert_eq!(state.get_nonce(account), 1);
        state.increment_nonce(account);
        assert_eq!(state.get_nonce(account), 2);

        // Test staking
        state.credit(account, 500); // Add more balance
        assert!(state.add_stake(account, 300).is_ok());
        assert_eq!(state.get_stake(account), 300);
        assert_eq!(state.get_balance(account), 700); // 500 + 500 - 300

        // Test reputation
        state.adjust_reputation(account, 10);
        assert_eq!(state.get_reputation(account), 10);
        state.adjust_reputation(account, -5);
        assert_eq!(state.get_reputation(account), 5);
    }
}
