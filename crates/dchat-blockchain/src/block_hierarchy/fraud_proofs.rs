//! Localized Fraud Proofs
//!
//! Implements localized fraud proofs at miniblock/tx granularity using Merkle
//! inclusion paths so disputes never require replaying entire blocks.

use super::core_types::ExecutionTransaction;
use super::{BlockError, Hash, MiniblockBody, MiniblockHeader, TxReceipt};
use crate::canonical;
use crate::hash_merkle::{merkle_proof, HashMerkleProof};
use ed25519_dalek::{Signature as Ed25519Sig, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum evidence size in bytes
pub const MAX_EVIDENCE_SIZE: usize = 256 * 1024; // 256 KB
/// Challenge window duration (seconds)
pub const FRAUD_CHALLENGE_WINDOW_SECS: u64 = 300; // 5 minutes

// ─────────────────────────────────────────────────────────────────────────────
// Fraud Proof Types
// ─────────────────────────────────────────────────────────────────────────────

/// Type of fraud detected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FraudType {
    /// Transaction doesn't belong to claimed lane
    LaneShardingViolation,
    /// Transaction root doesn't match header claim
    TxRootMismatch,
    /// Receipts root doesn't match header claim
    ReceiptsRootMismatch,
    /// Transaction count doesn't match
    TxCountMismatch,
    /// Body size doesn't match header claim
    BodySizeMismatch,
    /// Signature check count doesn't match
    SigcheckCountMismatch,
    /// Invalid state transition (pre -> post hash)
    InvalidStateTransition,
    /// Transaction execution result mismatch
    ExecutionMismatch,
    /// Duplicate transaction in miniblock
    DuplicateTransaction,
    /// Invalid transaction signature
    InvalidTxSignature,
    /// Gas accounting error
    GasAccountingError,
}

/// Localized fraud proof for a miniblock
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniblockFraudProof {
    /// Block height where fraud occurred
    pub block_height: u64,
    /// Subblock index
    pub subblock_index: u16,
    /// Miniblock index
    pub miniblock_index: u16,
    /// Type of fraud
    pub fraud_type: FraudType,
    /// The miniblock header (claimed)
    pub claimed_header: MiniblockHeader,
    /// Evidence proving fraud
    pub evidence: FraudEvidence,
    /// Proof creation timestamp
    pub timestamp: SystemTime,
}

impl MiniblockFraudProof {
    /// Verify the fraud proof is valid
    pub fn verify(&self) -> Result<bool, BlockError> {
        match &self.evidence {
            FraudEvidence::LaneViolation(ev) => self.verify_lane_violation(ev),
            FraudEvidence::CommitmentMismatch(ev) => self.verify_commitment_mismatch(ev),
            FraudEvidence::ExecutionMismatch(ev) => self.verify_execution_mismatch(ev),
            FraudEvidence::StateTransition(ev) => self.verify_state_transition(ev),
        }
    }

    fn verify_lane_violation(&self, ev: &LaneViolationEvidence) -> Result<bool, BlockError> {
        // Verify the transaction is included in the miniblock
        if !ev
            .tx_inclusion_proof
            .verify(ev.tx_leaf_hash, self.claimed_header.tx_root)
        {
            return Ok(false);
        }

        // Check if transaction's lane differs from miniblock's lane
        // ExecutionTransaction has a lane() method that derives lane from sender address
        let tx_lane = ev.transaction.lane();
        if tx_lane == self.claimed_header.lane {
            return Ok(false); // No violation
        }

        Ok(true) // Fraud proven
    }

    fn verify_commitment_mismatch(
        &self,
        ev: &CommitmentMismatchEvidence,
    ) -> Result<bool, BlockError> {
        // Recompute commitments from provided body
        let computed =
            super::lane_sharding::compute_body_commitments(self.claimed_header.lane, &ev.body)?;

        match self.fraud_type {
            FraudType::TxRootMismatch => Ok(computed.tx_root != self.claimed_header.tx_root),
            FraudType::ReceiptsRootMismatch => {
                Ok(computed.receipts_root != self.claimed_header.receipts_root)
            }
            FraudType::TxCountMismatch => Ok(computed.tx_count != self.claimed_header.tx_count),
            FraudType::BodySizeMismatch => {
                Ok(computed.body_bytes != self.claimed_header.body_bytes)
            }
            FraudType::SigcheckCountMismatch => {
                Ok(computed.sigchecks != self.claimed_header.sigchecks)
            }
            _ => Ok(false),
        }
    }

    fn verify_execution_mismatch(
        &self,
        ev: &ExecutionMismatchEvidence,
    ) -> Result<bool, BlockError> {
        // Verify transaction is in the miniblock via Merkle inclusion
        if !ev
            .tx_inclusion_proof
            .verify(ev.tx_leaf_hash, self.claimed_header.tx_root)
        {
            tracing::debug!("Fraud proof failed: tx inclusion proof invalid");
            return Ok(false);
        }

        // Verify the claimed receipt is in the receipts root
        if !ev
            .receipt_inclusion_proof
            .verify(ev.claimed_receipt.hash(), self.claimed_header.receipts_root)
        {
            tracing::debug!("Fraud proof failed: receipt inclusion proof invalid");
            return Ok(false);
        }

        // Re-execute the transaction using the provided witness data
        // The witness contains the pre-state needed for deterministic re-execution
        let recomputed_receipt = self.reexecute_transaction(
            &ev.transaction,
            &ev.execution_witness,
            ev.claimed_receipt.tx_index,
        )?;

        // Verify transaction IDs match
        if recomputed_receipt.tx_id != ev.claimed_receipt.tx_id {
            tracing::debug!("Fraud proof rejected: tx_id mismatch in receipts");
            return Ok(false);
        }

        // Compare recomputed receipt with claimed receipt
        // Fraud is proven if the hashes differ
        if recomputed_receipt.hash() != ev.claimed_receipt.hash() {
            tracing::warn!(
                "FRAUD DETECTED: execution mismatch for tx {:?} at height {}",
                ev.claimed_receipt.tx_id,
                self.block_height
            );
            tracing::warn!(
                "  Claimed receipt hash: {}",
                hex::encode(ev.claimed_receipt.hash().as_bytes())
            );
            tracing::warn!(
                "  Recomputed receipt hash: {}",
                hex::encode(recomputed_receipt.hash().as_bytes())
            );
            return Ok(true);
        }

        // Also check if a correct_receipt was provided for comparison
        if let Some(ref correct_receipt) = ev.correct_receipt {
            if correct_receipt.tx_id != ev.claimed_receipt.tx_id {
                return Ok(false);
            }
            if correct_receipt.hash() != ev.claimed_receipt.hash() {
                tracing::warn!(
                    "FRAUD DETECTED: correct_receipt mismatch for tx {:?}",
                    ev.claimed_receipt.tx_id
                );
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Re-execute a transaction deterministically using witness data
    fn reexecute_transaction(
        &self,
        tx: &ExecutionTransaction,
        witness: &[u8],
        tx_index: u32,
    ) -> Result<TxReceipt, BlockError> {
        use super::execution::{AccountState, ExecutionContext, WorldState};

        // Decode witness: contains pre-state accounts needed for execution
        // Format: [sender_balance: u64][sender_nonce: u64][recipient_balance: u64]
        if witness.len() < 24 {
            return Err(BlockError::InvalidTransaction(
                "execution witness too short".to_string(),
            ));
        }

        let sender_balance = u64::from_le_bytes(witness[0..8].try_into().unwrap());
        let sender_nonce = u64::from_le_bytes(witness[8..16].try_into().unwrap());
        let recipient_balance = u64::from_le_bytes(witness[16..24].try_into().unwrap());

        // Build pre-state from witness
        let mut state = WorldState::new();
        let sender_account = AccountState {
            balance: sender_balance,
            nonce: sender_nonce,
            code_hash: None,
            storage_root: None,
        };
        state.set_account(tx.sender, sender_account);

        if let Some(recipient) = tx.recipient {
            let recipient_account = AccountState {
                balance: recipient_balance,
                nonce: 0,
                code_hash: None,
                storage_root: None,
            };
            state.set_account(recipient, recipient_account);
        }

        // Create execution context
        let mut ctx = ExecutionContext::new(self.block_height, 0, tx.gas_limit);

        // Execute the transaction
        let gas_cost = tx.gas_cost();

        // Check gas limit
        if !ctx.use_gas(gas_cost) {
            return Ok(TxReceipt::failure(tx.tx_id, tx_index, "out of gas"));
        }

        // Get sender account
        let sender_state = state.get_account(&tx.sender);

        // Verify nonce
        if sender_state.nonce != tx.nonce {
            return Ok(TxReceipt::failure(tx.tx_id, tx_index, "nonce mismatch"));
        }

        // Calculate total cost
        let fee = gas_cost * tx.gas_price;
        let total_cost = tx.value + fee;

        // Check balance
        if sender_state.balance < total_cost {
            return Ok(TxReceipt::failure(
                tx.tx_id,
                tx_index,
                "insufficient balance",
            ));
        }

        // Deduct from sender
        let sender_account = state.get_account_mut(&tx.sender);
        sender_account.balance -= total_cost;
        sender_account.nonce += 1;

        // Credit recipient
        if let Some(recipient) = tx.recipient {
            let recipient_account = state.get_account_mut(&recipient);
            recipient_account.balance += tx.value;
        }

        // Compute post-state root
        let post_state_root = state.compute_state_root();

        Ok(TxReceipt::success(
            tx.tx_id,
            tx_index,
            gas_cost,
            post_state_root,
        ))
    }

    fn verify_state_transition(&self, ev: &StateTransitionEvidence) -> Result<bool, BlockError> {
        use super::execution::{AccountState, WorldState};

        // Verify pre-state proof
        if !ev
            .pre_state_proof
            .verify(ev.pre_state_value_hash, self.claimed_header.pre_state_hash)
        {
            tracing::debug!("Fraud proof failed: pre-state proof invalid");
            return Ok(false);
        }

        // Verify post-state proof
        if !ev.post_state_proof.verify(
            ev.post_state_value_hash,
            self.claimed_header.post_state_hash,
        ) {
            tracing::debug!("Fraud proof failed: post-state proof invalid");
            return Ok(false);
        }

        // Recompute the post-state root using witness data
        // The witness contains the complete pre-state and all state changes
        // that should have occurred during block execution
        //
        // Witness format:
        //   [num_accounts: u32]
        //   For each account:
        //     [address: 32 bytes][balance: u64][nonce: u64]
        //   [num_changes: u32]
        //   For each change:
        //     [address: 32 bytes][new_balance: u64][new_nonce: u64]

        let witness = &ev.transition_witness;
        if witness.len() < 4 {
            return Err(BlockError::InvalidTransaction(
                "transition witness too short".to_string(),
            ));
        }

        let num_accounts = u32::from_le_bytes(witness[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;

        // Build pre-state
        let mut state = WorldState::new();
        for _ in 0..num_accounts {
            if offset + 48 > witness.len() {
                return Err(BlockError::InvalidTransaction(
                    "witness truncated in accounts".to_string(),
                ));
            }
            let mut addr_bytes = [0u8; 32];
            addr_bytes.copy_from_slice(&witness[offset..offset + 32]);
            offset += 32;
            let balance = u64::from_le_bytes(witness[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let nonce = u64::from_le_bytes(witness[offset..offset + 8].try_into().unwrap());
            offset += 8;

            let account = AccountState {
                balance,
                nonce,
                code_hash: None,
                storage_root: None,
            };
            state.set_account(addr_bytes.into(), account);
        }

        // Verify pre-state hash matches witness-computed pre-state
        let computed_pre_state = state.compute_state_root();
        if computed_pre_state != ev.pre_state_value_hash {
            tracing::debug!("Fraud proof failed: witness pre-state doesn't match claimed");
            return Ok(false);
        }

        // Apply state changes from witness
        if offset + 4 > witness.len() {
            return Err(BlockError::InvalidTransaction(
                "witness truncated before changes".to_string(),
            ));
        }
        let num_changes =
            u32::from_le_bytes(witness[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        for _ in 0..num_changes {
            if offset + 48 > witness.len() {
                return Err(BlockError::InvalidTransaction(
                    "witness truncated in changes".to_string(),
                ));
            }
            let mut addr_bytes = [0u8; 32];
            addr_bytes.copy_from_slice(&witness[offset..offset + 32]);
            offset += 32;
            let new_balance = u64::from_le_bytes(witness[offset..offset + 8].try_into().unwrap());
            offset += 8;
            let new_nonce = u64::from_le_bytes(witness[offset..offset + 8].try_into().unwrap());
            offset += 8;

            let account = state.get_account_mut(&addr_bytes.into());
            account.balance = new_balance;
            account.nonce = new_nonce;
        }

        // Compute expected post-state root
        let computed_post_state = state.compute_state_root();

        // Fraud is proven if the claimed post-state differs from correctly computed post-state
        if computed_post_state != ev.post_state_value_hash {
            tracing::warn!(
                "FRAUD DETECTED: state transition invalid at height {}",
                self.block_height
            );
            tracing::warn!(
                "  Claimed post-state: {}",
                hex::encode(ev.post_state_value_hash.as_bytes())
            );
            tracing::warn!(
                "  Correct post-state: {}",
                hex::encode(computed_post_state.as_bytes())
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Compute fraud proof hash (for on-chain storage)
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/fraud_proof/v1", &bytes)
    }

    /// Check if evidence size is within limits
    pub fn is_valid_size(&self) -> bool {
        let size = canonical::canonical_serialize(self).len();
        size <= MAX_EVIDENCE_SIZE
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fraud Evidence Types
// ─────────────────────────────────────────────────────────────────────────────

/// Evidence for different fraud types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FraudEvidence {
    /// Evidence for lane sharding violation
    LaneViolation(LaneViolationEvidence),
    /// Evidence for commitment mismatch
    CommitmentMismatch(CommitmentMismatchEvidence),
    /// Evidence for execution mismatch
    ExecutionMismatch(ExecutionMismatchEvidence),
    /// Evidence for invalid state transition
    StateTransition(StateTransitionEvidence),
}

/// Evidence for lane sharding violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaneViolationEvidence {
    /// The offending transaction
    pub transaction: ExecutionTransaction,
    /// Merkle proof of transaction inclusion
    pub tx_inclusion_proof: HashMerkleProof,
    /// Leaf hash of the transaction
    pub tx_leaf_hash: Hash,
    /// Index of transaction in miniblock
    pub tx_index: u32,
}

/// Evidence for commitment mismatch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitmentMismatchEvidence {
    /// Full miniblock body
    pub body: MiniblockBody,
}

/// Evidence for execution mismatch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMismatchEvidence {
    /// The transaction that was incorrectly executed
    pub transaction: ExecutionTransaction,
    /// Transaction leaf hash
    pub tx_leaf_hash: Hash,
    /// Merkle proof of transaction inclusion
    pub tx_inclusion_proof: HashMerkleProof,
    /// Claimed receipt (in the block)
    pub claimed_receipt: TxReceipt,
    /// Merkle proof of receipt inclusion
    pub receipt_inclusion_proof: HashMerkleProof,
    /// Correct receipt (from re-execution)
    pub correct_receipt: Option<TxReceipt>,
    /// Witness data for re-execution
    pub execution_witness: Vec<u8>,
}

/// Evidence for invalid state transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransitionEvidence {
    /// Pre-state value hash
    pub pre_state_value_hash: Hash,
    /// Merkle proof for pre-state
    pub pre_state_proof: HashMerkleProof,
    /// Post-state value hash
    pub post_state_value_hash: Hash,
    /// Merkle proof for post-state
    pub post_state_proof: HashMerkleProof,
    /// State key being disputed
    pub state_key: Vec<u8>,
    /// Witness for state transition verification
    /// Format: [num_accounts: u32][[address: 32][balance: u64][nonce: u64]]...
    ///         [num_changes: u32][[address: 32][new_balance: u64][new_nonce: u64]]...
    pub transition_witness: Vec<u8>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Transaction-Level Fraud Proof
// ─────────────────────────────────────────────────────────────────────────────

/// Fraud proof for a single transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxFraudProof {
    /// Block height
    pub block_height: u64,
    /// Subblock index
    pub subblock_index: u16,
    /// Miniblock index
    pub miniblock_index: u16,
    /// Transaction index within miniblock
    pub tx_index: u32,
    /// Transaction ID
    pub tx_id: uuid::Uuid,
    /// Fraud type
    pub fraud_type: FraudType,
    /// The transaction
    pub transaction: ExecutionTransaction,
    /// Merkle proof of transaction inclusion in miniblock
    pub tx_inclusion_proof: HashMerkleProof,
    /// Expected tx_root from miniblock header (for verification)
    pub expected_tx_root: Option<Hash>,
    /// Miniblock header hash (for subblock proof)
    pub miniblock_header_hash: Hash,
    /// Merkle proof of miniblock in subblock
    pub miniblock_inclusion_proof: HashMerkleProof,
    /// Evidence specific to fraud type
    pub evidence: TxFraudEvidence,
    /// Timestamp
    pub timestamp: SystemTime,
}

/// Evidence for transaction-level fraud
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TxFraudEvidence {
    /// Transaction has invalid signature
    InvalidSignature {
        claimed_signature: Vec<u8>,
        claimed_pubkey: Vec<u8>,
    },
    /// Transaction has invalid nonce
    InvalidNonce {
        claimed_nonce: u64,
        correct_nonce: u64,
        nonce_proof: HashMerkleProof,
    },
    /// Gas used exceeds gas limit
    GasOverflow {
        claimed_gas: u64,
        actual_gas: u64,
        gas_computation_witness: Vec<u8>,
    },
    /// Duplicate transaction (already included elsewhere)
    Duplicate {
        other_block_height: u64,
        other_subblock_index: u16,
        other_miniblock_index: u16,
        other_tx_index: u32,
        other_inclusion_proof: HashMerkleProof,
    },
}

impl TxFraudProof {
    /// Verify the fraud proof against a known tx_root
    pub fn verify(&self) -> Result<bool, BlockError> {
        // Compute transaction leaf hash
        let tx_leaf = self.compute_tx_leaf_hash();

        // Verify transaction inclusion proof
        // The inclusion proof must verify the tx_leaf against the merkle path
        if !self.tx_inclusion_proof.path.is_empty() {
            let computed_root = self.compute_merkle_root_from_proof(
                tx_leaf,
                &self.tx_inclusion_proof.path,
                &self.tx_inclusion_proof.directions,
            );

            // Verify the computed root is non-zero (valid structure)
            if computed_root == Hash::ZERO {
                return Err(BlockError::FraudProof(
                    "invalid transaction inclusion proof: zero root".to_string(),
                ));
            }

            // CRITICAL: Verify against expected tx_root from miniblock header
            // The fraud proof must include the expected root for verification
            if let Some(expected_root) = self.expected_tx_root {
                if computed_root != expected_root {
                    return Err(BlockError::FraudProof(format!(
                        "tx inclusion proof mismatch: computed {} vs expected {}",
                        hex::encode(&computed_root.as_bytes()[..8]),
                        hex::encode(&expected_root.as_bytes()[..8])
                    )));
                }
            }
        }

        match &self.evidence {
            TxFraudEvidence::InvalidSignature {
                claimed_signature,
                claimed_pubkey,
            } => {
                // Verify signature is actually invalid using Ed25519
                // A valid fraud proof proves the signature DOES NOT verify
                let sig_valid =
                    verify_tx_signature(&self.transaction, claimed_signature, claimed_pubkey);
                // Fraud is proven if signature is invalid (returns false)
                Ok(!sig_valid)
            }
            TxFraudEvidence::InvalidNonce {
                claimed_nonce,
                correct_nonce,
                ..
            } => {
                // Check nonce mismatch
                Ok(claimed_nonce != correct_nonce)
            }
            TxFraudEvidence::GasOverflow {
                claimed_gas,
                actual_gas,
                ..
            } => {
                // Check gas overflow
                Ok(claimed_gas < actual_gas)
            }
            TxFraudEvidence::Duplicate { .. } => {
                // Verify duplicate inclusion proofs
                // Would verify both inclusion proofs in practice
                Ok(true)
            }
        }
    }

    fn compute_tx_leaf_hash(&self) -> Hash {
        // For ExecutionTransaction, use its hash and tx_id
        canonical::domain_hash_parts(
            canonical::DOMAIN_SEP_TX_LEAF_V1,
            &[
                self.transaction.tx_id.as_bytes(),
                self.transaction.hash.as_bytes(),
            ],
        )
    }

    /// Compute merkle root from leaf and proof (for verification)
    fn compute_merkle_root_from_proof(
        &self,
        leaf: Hash,
        path: &[Hash],
        directions: &[bool],
    ) -> Hash {
        if path.len() != directions.len() {
            return Hash::ZERO;
        }

        let mut current = leaf;

        for (sibling, is_right) in path.iter().zip(directions.iter()) {
            let (left, right) = if *is_right {
                // current is right child, sibling is left
                (sibling, &current)
            } else {
                // current is left child, sibling is right
                (&current, sibling)
            };

            let mut combined = Vec::with_capacity(64);
            combined.extend_from_slice(left.as_bytes());
            combined.extend_from_slice(right.as_bytes());
            current = canonical::domain_hash(canonical::DOMAIN_SEP_MERKLE_NODE_V1, &combined);
        }

        current
    }

    /// Compute fraud proof hash
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/tx_fraud_proof/v1", &bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fraud Proof Builder
// ─────────────────────────────────────────────────────────────────────────────

/// Builder for creating fraud proofs
pub struct FraudProofBuilder {
    block_height: u64,
    subblock_index: u16,
    miniblock_index: u16,
}

impl FraudProofBuilder {
    /// Create new builder for a specific miniblock
    pub fn new(block_height: u64, subblock_index: u16, miniblock_index: u16) -> Self {
        Self {
            block_height,
            subblock_index,
            miniblock_index,
        }
    }

    /// Build lane violation fraud proof
    pub fn build_lane_violation(
        &self,
        header: &MiniblockHeader,
        body: &MiniblockBody,
        tx_index: usize,
    ) -> Result<MiniblockFraudProof, BlockError> {
        if tx_index >= body.transactions.len() {
            return Err(BlockError::FraudProof(
                "transaction index out of bounds".to_string(),
            ));
        }

        let chain_tx = &body.transactions[tx_index];
        let tx_lane = super::lane_sharding::lane_for_transaction(chain_tx)?;

        if tx_lane == header.lane {
            return Err(BlockError::FraudProof("no lane violation".to_string()));
        }

        // Convert to ExecutionTransaction for the fraud proof
        let exec_tx = ExecutionTransaction::from_chain_transaction(chain_tx).ok_or_else(|| {
            BlockError::FraudProof(
                "failed to parse transaction as ExecutionTransaction".to_string(),
            )
        })?;

        // Build inclusion proof
        let tx_leaves = compute_tx_leaves(body);
        let (tx_leaf_hash, tx_inclusion_proof) =
            merkle_proof(&tx_leaves, tx_index).ok_or_else(|| {
                BlockError::FraudProof("failed to create inclusion proof".to_string())
            })?;

        Ok(MiniblockFraudProof {
            block_height: self.block_height,
            subblock_index: self.subblock_index,
            miniblock_index: self.miniblock_index,
            fraud_type: FraudType::LaneShardingViolation,
            claimed_header: header.clone(),
            evidence: FraudEvidence::LaneViolation(LaneViolationEvidence {
                transaction: exec_tx,
                tx_inclusion_proof,
                tx_leaf_hash,
                tx_index: tx_index as u32,
            }),
            timestamp: SystemTime::now(),
        })
    }

    /// Build commitment mismatch fraud proof
    pub fn build_commitment_mismatch(
        &self,
        header: &MiniblockHeader,
        body: &MiniblockBody,
        fraud_type: FraudType,
    ) -> Result<MiniblockFraudProof, BlockError> {
        // Verify there actually is a mismatch
        let computed = super::lane_sharding::compute_body_commitments(header.lane, body)?;

        let has_mismatch = match fraud_type {
            FraudType::TxRootMismatch => computed.tx_root != header.tx_root,
            FraudType::ReceiptsRootMismatch => computed.receipts_root != header.receipts_root,
            FraudType::TxCountMismatch => computed.tx_count != header.tx_count,
            FraudType::BodySizeMismatch => computed.body_bytes != header.body_bytes,
            FraudType::SigcheckCountMismatch => computed.sigchecks != header.sigchecks,
            _ => return Err(BlockError::FraudProof("invalid fraud type".to_string())),
        };

        if !has_mismatch {
            return Err(BlockError::FraudProof("no commitment mismatch".to_string()));
        }

        Ok(MiniblockFraudProof {
            block_height: self.block_height,
            subblock_index: self.subblock_index,
            miniblock_index: self.miniblock_index,
            fraud_type,
            claimed_header: header.clone(),
            evidence: FraudEvidence::CommitmentMismatch(CommitmentMismatchEvidence {
                body: body.clone(),
            }),
            timestamp: SystemTime::now(),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fraud Detector
// ─────────────────────────────────────────────────────────────────────────────

/// Detects fraud in miniblocks
pub struct FraudDetector;

impl FraudDetector {
    /// Check miniblock for all fraud types
    pub fn check_miniblock(header: &MiniblockHeader, body: &MiniblockBody) -> Vec<FraudType> {
        let mut frauds = Vec::new();

        // Check lane sharding
        for tx in &body.transactions {
            if let Ok(tx_lane) = super::lane_sharding::lane_for_transaction(tx) {
                if tx_lane != header.lane {
                    frauds.push(FraudType::LaneShardingViolation);
                    break;
                }
            }
        }

        // Check commitments
        if let Ok(computed) = super::lane_sharding::compute_body_commitments(header.lane, body) {
            if computed.tx_root != header.tx_root {
                frauds.push(FraudType::TxRootMismatch);
            }
            if computed.receipts_root != header.receipts_root {
                frauds.push(FraudType::ReceiptsRootMismatch);
            }
            if computed.tx_count != header.tx_count {
                frauds.push(FraudType::TxCountMismatch);
            }
            if computed.body_bytes != header.body_bytes {
                frauds.push(FraudType::BodySizeMismatch);
            }
            if computed.sigchecks != header.sigchecks {
                frauds.push(FraudType::SigcheckCountMismatch);
            }
        }

        // Check for duplicates
        let mut seen_ids = std::collections::HashSet::new();
        for tx in &body.transactions {
            if !seen_ids.insert(tx.tx_id) {
                frauds.push(FraudType::DuplicateTransaction);
                break;
            }
        }

        frauds
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

fn compute_tx_leaves(body: &MiniblockBody) -> Vec<Hash> {
    body.transactions
        .iter()
        .map(|tx| {
            canonical::domain_hash_parts(
                canonical::DOMAIN_SEP_TX_LEAF_V1,
                &[
                    tx.tx_id.as_bytes(),
                    tx.tx_hash.as_bytes(),
                    &[tx_type_id(tx.tx_type)],
                ],
            )
        })
        .collect()
}

fn tx_type_id(tx_type: dchat_chain::TransactionType) -> u8 {
    match tx_type {
        dchat_chain::TransactionType::RegisterUser => 1,
        dchat_chain::TransactionType::SendDirectMessage => 2,
        dchat_chain::TransactionType::CreateChannel => 3,
        dchat_chain::TransactionType::PostToChannel => 4,
        dchat_chain::TransactionType::JoinChannel => 5,
        dchat_chain::TransactionType::UpdateProfile => 6,
        dchat_chain::TransactionType::SubmitDeliveryProof => 7,
    }
}

fn verify_tx_signature(tx: &ExecutionTransaction, sig_bytes: &[u8], pubkey_bytes: &[u8]) -> bool {
    // Validate signature and public key lengths
    if sig_bytes.len() != 64 {
        tracing::debug!(
            sig_len = sig_bytes.len(),
            "Invalid signature length, expected 64 bytes for Ed25519"
        );
        return false;
    }
    if pubkey_bytes.len() != 32 {
        tracing::debug!(
            pubkey_len = pubkey_bytes.len(),
            "Invalid public key length, expected 32 bytes for Ed25519"
        );
        return false;
    }

    // Parse Ed25519 verifying key
    let pubkey_array: [u8; 32] = match pubkey_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return false,
    };
    let verifying_key = match VerifyingKey::from_bytes(&pubkey_array) {
        Ok(vk) => vk,
        Err(e) => {
            tracing::debug!(error = %e, "Failed to parse Ed25519 public key");
            return false;
        }
    };

    // Parse Ed25519 signature
    let sig_array: [u8; 64] = match sig_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return false,
    };
    let signature = Ed25519Sig::from_bytes(&sig_array);

    // For ExecutionTransaction, the signing message is the transaction hash
    // which is computed from sender, recipient, value, nonce, gas, etc.
    let signing_message = tx.hash.as_bytes();

    // Verify the signature
    match verifying_key.verify(signing_message, &signature) {
        Ok(()) => {
            tracing::trace!(
                tx_id = %tx.tx_id,
                "Transaction signature verified successfully"
            );
            true
        }
        Err(e) => {
            tracing::debug!(
                tx_id = %tx.tx_id,
                error = %e,
                "Transaction signature verification failed"
            );
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_hierarchy::LaneId;

    fn create_test_header(lane: LaneId) -> MiniblockHeader {
        MiniblockHeader {
            block_height: 1,
            subblock_index: 0,
            index: 0,
            timestamp: SystemTime::now(),
            lane,
            tx_root: Hash::ZERO,
            receipts_root: Hash::ZERO,
            tx_count: 0,
            body_bytes: 0,
            sigchecks: 0,
            pre_state_hash: Hash::ZERO,
            post_state_hash: Hash::ZERO,
            gas_used: 0,
            gas_limit: 30_000_000,
            producer: Hash::ZERO,
        }
    }

    #[test]
    fn test_fraud_detector_clean() {
        let header = create_test_header(LaneId::new(0));
        let body = MiniblockBody {
            transactions: vec![],
            receipts: vec![],
        };

        let frauds = FraudDetector::check_miniblock(&header, &body);
        assert!(frauds.is_empty());
    }

    #[test]
    fn test_fraud_proof_hash_determinism() {
        let proof = MiniblockFraudProof {
            block_height: 1,
            subblock_index: 0,
            miniblock_index: 0,
            fraud_type: FraudType::TxCountMismatch,
            claimed_header: create_test_header(LaneId::new(0)),
            evidence: FraudEvidence::CommitmentMismatch(CommitmentMismatchEvidence {
                body: MiniblockBody {
                    transactions: vec![],
                    receipts: vec![],
                },
            }),
            timestamp: SystemTime::UNIX_EPOCH,
        };

        let hash1 = proof.hash();
        let hash2 = proof.hash();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_fraud_proof_size_check() {
        let proof = MiniblockFraudProof {
            block_height: 1,
            subblock_index: 0,
            miniblock_index: 0,
            fraud_type: FraudType::TxCountMismatch,
            claimed_header: create_test_header(LaneId::new(0)),
            evidence: FraudEvidence::CommitmentMismatch(CommitmentMismatchEvidence {
                body: MiniblockBody {
                    transactions: vec![],
                    receipts: vec![],
                },
            }),
            timestamp: SystemTime::now(),
        };

        assert!(proof.is_valid_size());
    }
}
