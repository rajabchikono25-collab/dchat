//! Validator Chain Synchronization
//!
//! This module synchronizes the blockchain across all validators using
//! the VRF-based slot leader selection. It ensures:
//!
//! - **Consistency**: All validators agree on the same chain
//! - **Liveness**: The chain continues to make progress
//! - **Safety**: Invalid blocks are rejected
//! - **Finality**: Blocks become irreversible after sufficient attestations
//!
//! # Synchronization Protocol
//!
//! 1. **Leader Election**: Each slot has exactly one leader via VRF
//! 2. **Block Proposal**: The leader proposes a block for their slot
//! 3. **Block Propagation**: The proposal is broadcast to all validators
//! 4. **Attestation**: Validators verify and attest to the block
//! 5. **Finalization**: After 2/3+ attestations, the block is finalized
//!
//! # Fork Choice
//!
//! Uses a modified GHOST (Greedy Heaviest Observed Sub-Tree) rule:
//! - Weight blocks by total attestation stake
//! - Prefer chains with more finalized checkpoints
//! - Break ties using VRF-based randomness

use super::slot_leader_selection::{
    SlotId, SlotLeaderProof, SlotLeaderSelector, ValidatorInfo, DEFAULT_EPOCH_LENGTH_SLOTS,
    DEFAULT_SLOT_DURATION_MS,
};
use super::two_stage_finality::FinalityStage;
use crate::block_hierarchy::Hash;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use tracing::error;

/// Block attestation threshold for finality (basis points)
pub const FINALITY_THRESHOLD_BPS: u64 = 6667; // 66.67%

/// Maximum time to wait for block proposal (ms)
pub const BLOCK_PROPOSAL_TIMEOUT_MS: u64 = 1500;

/// Time before slot end to start attestation collection (ms)
pub const ATTESTATION_COLLECTION_START_MS: u64 = 1000;

/// Maximum blocks to keep in memory
pub const MAX_BLOCKS_IN_MEMORY: usize = 1000;

/// Maximum forks to track simultaneously
pub const MAX_TRACKED_FORKS: usize = 10;

/// Checkpoint interval in slots
pub const CHECKPOINT_INTERVAL_SLOTS: u64 = 32;

/// Validator chain sync errors
#[derive(Debug, Error)]
pub enum ChainSyncError {
    #[error("Block not found: {0}")]
    BlockNotFound(String),

    #[error("Invalid block: {0}")]
    InvalidBlock(String),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid attestation")]
    InvalidAttestation,

    #[error("Not slot leader")]
    NotSlotLeader,

    #[error("Slot already finalized")]
    SlotAlreadyFinalized,

    #[error("Fork too old: {0}")]
    ForkTooOld(u64),

    #[error("Missing parent block: {0}")]
    MissingParent(String),

    #[error("Sync timeout")]
    SyncTimeout,

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Block proposal from a slot leader
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockProposal {
    /// Slot this block is for
    pub slot: SlotId,
    /// Block hash
    pub block_hash: Hash,
    /// Parent block hash
    pub parent_hash: Hash,
    /// Parent slot
    pub parent_slot: SlotId,
    /// State root after executing block
    pub state_root: Hash,
    /// Transaction merkle root
    pub transactions_root: Hash,
    /// Number of transactions
    pub transaction_count: u32,
    /// Leadership proof (VRF)
    pub leader_proof: SlotLeaderProof,
    /// Block signature (Ed25519)
    pub signature: Vec<u8>,
    /// Timestamp
    pub timestamp: u64,
}

impl BlockProposal {
    /// Create a new block proposal
    pub fn new(
        slot: SlotId,
        parent_hash: Hash,
        parent_slot: SlotId,
        state_root: Hash,
        transactions_root: Hash,
        transaction_count: u32,
        leader_proof: SlotLeaderProof,
        signing_key: &SigningKey,
    ) -> Self {
        // Compute block hash
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"dchat-block-v1");
        hasher.update(&slot.epoch.to_le_bytes());
        hasher.update(&slot.slot.to_le_bytes());
        hasher.update(parent_hash.as_bytes());
        hasher.update(state_root.as_bytes());
        hasher.update(transactions_root.as_bytes());
        hasher.update(&transaction_count.to_le_bytes());
        let block_hash = Hash::from(*hasher.finalize().as_bytes());

        // Sign the block
        let signature = signing_key.sign(block_hash.as_bytes()).to_bytes().to_vec();

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            slot,
            block_hash,
            parent_hash,
            parent_slot,
            state_root,
            transactions_root,
            transaction_count,
            leader_proof,
            signature,
            timestamp,
        }
    }

    /// Verify the block proposal signature
    pub fn verify_signature(&self) -> Result<(), ChainSyncError> {
        let public_key = VerifyingKey::from_bytes(&self.leader_proof.leader_signing_key)
            .map_err(|_| ChainSyncError::InvalidSignature)?;

        let sig_bytes: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| ChainSyncError::InvalidSignature)?;
        let signature = Signature::from_bytes(&sig_bytes);

        public_key
            .verify(self.block_hash.as_bytes(), &signature)
            .map_err(|_| ChainSyncError::InvalidSignature)
    }
}

/// Attestation from a validator for a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockAttestation {
    /// Block being attested
    pub block_hash: Hash,
    /// Slot of the block
    pub slot: SlotId,
    /// Validator's public key
    pub validator_key: [u8; 32],
    /// Validator's stake weight (basis points)
    pub stake_weight_bps: u64,
    /// Signature over (block_hash || slot)
    pub signature: Vec<u8>,
    /// Timestamp
    pub timestamp: u64,
}

impl BlockAttestation {
    /// Create a new attestation
    pub fn create(
        block_hash: Hash,
        slot: SlotId,
        stake_weight_bps: u64,
        signing_key: &SigningKey,
    ) -> Self {
        // Sign (block_hash || slot)
        let mut msg = Vec::with_capacity(48);
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(&slot.epoch.to_le_bytes());
        msg.extend_from_slice(&slot.slot.to_le_bytes());

        let signature = signing_key.sign(&msg).to_bytes().to_vec();
        let validator_key = signing_key.verifying_key().to_bytes();

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            block_hash,
            slot,
            validator_key,
            stake_weight_bps,
            signature,
            timestamp,
        }
    }

    /// Verify the attestation signature
    pub fn verify(&self) -> Result<(), ChainSyncError> {
        let public_key = VerifyingKey::from_bytes(&self.validator_key)
            .map_err(|_| ChainSyncError::InvalidAttestation)?;

        let mut msg = Vec::with_capacity(48);
        msg.extend_from_slice(self.block_hash.as_bytes());
        msg.extend_from_slice(&self.slot.epoch.to_le_bytes());
        msg.extend_from_slice(&self.slot.slot.to_le_bytes());

        let sig_bytes: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| ChainSyncError::InvalidAttestation)?;
        let signature = Signature::from_bytes(&sig_bytes);

        public_key
            .verify(&msg, &signature)
            .map_err(|_| ChainSyncError::InvalidAttestation)
    }
}

/// Block status in the chain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockStatus {
    /// Block has been proposed but not yet attested
    Proposed,
    /// Block has some attestations but below threshold
    Pending,
    /// Block has reached finality threshold
    Finalized,
    /// Block has been orphaned (not on canonical chain)
    Orphaned,
}

/// Tracked block with attestations
#[derive(Debug, Clone)]
pub struct TrackedBlock {
    /// The block proposal
    pub proposal: BlockProposal,
    /// Collected attestations
    pub attestations: Vec<BlockAttestation>,
    /// Total attested stake (basis points)
    pub attested_stake_bps: u64,
    /// Block status
    pub status: BlockStatus,
    /// Finality stage
    pub finality: FinalityStage,
    /// When the block was first seen
    pub first_seen: Instant,
    /// Child blocks (forks)
    pub children: Vec<Hash>,
}

impl TrackedBlock {
    /// Create from a proposal
    pub fn from_proposal(proposal: BlockProposal) -> Self {
        Self {
            proposal,
            attestations: Vec::new(),
            attested_stake_bps: 0,
            status: BlockStatus::Proposed,
            finality: FinalityStage::Pending,
            first_seen: Instant::now(),
            children: Vec::new(),
        }
    }

    /// Add an attestation
    pub fn add_attestation(&mut self, attestation: BlockAttestation) -> bool {
        // Check for duplicate
        if self
            .attestations
            .iter()
            .any(|a| a.validator_key == attestation.validator_key)
        {
            return false;
        }

        self.attested_stake_bps += attestation.stake_weight_bps;
        self.attestations.push(attestation);

        // Update status
        if self.attested_stake_bps >= FINALITY_THRESHOLD_BPS {
            self.status = BlockStatus::Finalized;
            self.finality = FinalityStage::Local;
        } else if !self.attestations.is_empty() {
            self.status = BlockStatus::Pending;
        }

        true
    }

    /// Check if block is finalized
    pub fn is_finalized(&self) -> bool {
        self.status == BlockStatus::Finalized
    }
}

/// Checkpoint for finality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Checkpoint epoch
    pub epoch: u64,
    /// Slot that this checkpoint covers up to
    pub slot: SlotId,
    /// Block hash at checkpoint
    pub block_hash: Hash,
    /// State root at checkpoint
    pub state_root: Hash,
    /// Previous checkpoint hash
    pub previous_checkpoint: Option<Hash>,
    /// Aggregate attestation weight (basis points)
    pub total_attestation_bps: u64,
    /// Number of validators who attested
    pub attestation_count: u32,
    /// Timestamp
    pub timestamp: u64,
}

/// Chain state for a single fork
#[derive(Debug, Clone)]
pub struct ForkState {
    /// Fork ID (hash of first divergent block)
    pub fork_id: Hash,
    /// Head block hash
    pub head_hash: Hash,
    /// Head slot
    pub head_slot: SlotId,
    /// Total accumulated attestation weight
    pub total_weight: u64,
    /// Latest checkpoint on this fork
    pub latest_checkpoint: Option<Hash>,
    /// Blocks in this fork (ordered by slot)
    pub blocks: Vec<Hash>,
}

/// Validator chain synchronizer
pub struct ValidatorChainSync {
    /// Slot leader selector
    leader_selector: Arc<RwLock<SlotLeaderSelector>>,

    /// Tracked blocks: hash -> block
    blocks: Arc<RwLock<HashMap<Hash, TrackedBlock>>>,

    /// Block index by slot: slot -> block_hashes (can have multiple for forks)
    blocks_by_slot: Arc<RwLock<HashMap<SlotId, Vec<Hash>>>>,

    /// Current canonical head
    canonical_head: Arc<RwLock<Hash>>,

    /// Latest finalized block
    latest_finalized: Arc<RwLock<Hash>>,

    /// Checkpoints
    checkpoints: Arc<RwLock<HashMap<u64, Checkpoint>>>,

    /// Active forks
    forks: Arc<RwLock<Vec<ForkState>>>,

    /// Our validator info (if we're a validator)
    our_validator: Option<ValidatorInfo>,

    /// Our signing key (if we're a validator)
    our_signing_key: Option<SigningKey>,

    /// Genesis block hash
    genesis_hash: Hash,

    /// Epoch length
    epoch_length: u64,

    /// Slot duration (ms)
    slot_duration_ms: u64,

    /// Pending attestations to broadcast
    pending_attestations: Arc<RwLock<VecDeque<BlockAttestation>>>,

    /// Block proposal channel
    proposal_tx: broadcast::Sender<BlockProposal>,

    /// Attestation channel
    attestation_tx: broadcast::Sender<BlockAttestation>,
}

impl ValidatorChainSync {
    /// Create a new chain synchronizer
    pub fn new(
        leader_selector: Arc<RwLock<SlotLeaderSelector>>,
        genesis_hash: Hash,
        epoch_length: u64,
        slot_duration_ms: u64,
    ) -> Self {
        let (proposal_tx, _) = broadcast::channel(1000);
        let (attestation_tx, _) = broadcast::channel(10000);

        Self {
            leader_selector,
            blocks: Arc::new(RwLock::new(HashMap::new())),
            blocks_by_slot: Arc::new(RwLock::new(HashMap::new())),
            canonical_head: Arc::new(RwLock::new(genesis_hash)),
            latest_finalized: Arc::new(RwLock::new(genesis_hash)),
            checkpoints: Arc::new(RwLock::new(HashMap::new())),
            forks: Arc::new(RwLock::new(Vec::new())),
            our_validator: None,
            our_signing_key: None,
            genesis_hash,
            epoch_length,
            slot_duration_ms,
            pending_attestations: Arc::new(RwLock::new(VecDeque::new())),
            proposal_tx,
            attestation_tx,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(genesis_hash: Hash) -> Self {
        let leader_selector = Arc::new(RwLock::new(SlotLeaderSelector::with_defaults()));
        Self::new(
            leader_selector,
            genesis_hash,
            DEFAULT_EPOCH_LENGTH_SLOTS,
            DEFAULT_SLOT_DURATION_MS,
        )
    }

    /// Set our validator identity
    pub fn set_validator(&mut self, validator: ValidatorInfo, signing_key: SigningKey) {
        self.our_validator = Some(validator);
        self.our_signing_key = Some(signing_key);
    }

    /// Get slot duration in milliseconds
    pub fn get_slot_duration_ms(&self) -> u64 {
        self.slot_duration_ms
    }

    /// Get epoch length in slots
    pub fn get_epoch_length(&self) -> u64 {
        self.epoch_length
    }

    /// Subscribe to block proposals
    pub fn subscribe_proposals(&self) -> broadcast::Receiver<BlockProposal> {
        self.proposal_tx.subscribe()
    }

    /// Subscribe to attestations
    pub fn subscribe_attestations(&self) -> broadcast::Receiver<BlockAttestation> {
        self.attestation_tx.subscribe()
    }

    /// Get current canonical head
    pub async fn get_canonical_head(&self) -> Hash {
        *self.canonical_head.read().await
    }

    /// Get latest finalized block
    pub async fn get_latest_finalized(&self) -> Hash {
        *self.latest_finalized.read().await
    }

    /// Get block by hash
    pub async fn get_block(&self, hash: &Hash) -> Option<TrackedBlock> {
        self.blocks.read().await.get(hash).cloned()
    }

    /// Get blocks for a slot (may have multiple due to forks)
    pub async fn get_blocks_for_slot(&self, slot: SlotId) -> Vec<TrackedBlock> {
        let blocks = self.blocks.read().await;
        let by_slot = self.blocks_by_slot.read().await;

        by_slot
            .get(&slot)
            .map(|hashes| {
                hashes
                    .iter()
                    .filter_map(|h| blocks.get(h).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Process an incoming block proposal
    pub async fn process_proposal(&self, proposal: BlockProposal) -> Result<(), ChainSyncError> {
        // Verify the proposal signature
        proposal.verify_signature()?;

        // Verify the leadership proof
        let selector = self.leader_selector.read().await;
        let slot_seed = selector
            .get_slot_seed(proposal.slot)
            .map_err(|e| ChainSyncError::InvalidBlock(format!("{}", e)))?;
        proposal
            .leader_proof
            .verify(&slot_seed)
            .map_err(|_| ChainSyncError::InvalidBlock("Invalid leadership proof".into()))?;

        // Check parent exists (unless it's the genesis)
        if proposal.parent_hash != self.genesis_hash {
            let blocks = self.blocks.read().await;
            if !blocks.contains_key(&proposal.parent_hash) {
                return Err(ChainSyncError::MissingParent(hex::encode(
                    proposal.parent_hash.as_bytes(),
                )));
            }
        }

        // Add block to tracking
        {
            let mut blocks = self.blocks.write().await;
            let mut by_slot = self.blocks_by_slot.write().await;

            // Check for duplicate
            if blocks.contains_key(&proposal.block_hash) {
                return Ok(()); // Already have this block
            }

            let tracked = TrackedBlock::from_proposal(proposal.clone());
            blocks.insert(proposal.block_hash, tracked);

            by_slot
                .entry(proposal.slot)
                .or_insert_with(Vec::new)
                .push(proposal.block_hash);

            // Update parent's children
            if let Some(parent) = blocks.get_mut(&proposal.parent_hash) {
                parent.children.push(proposal.block_hash);
            }
        }

        // Broadcast to subscribers
        let _ = self.proposal_tx.send(proposal.clone());

        // Update fork tracking
        self.update_forks(&proposal).await;

        // If we're a validator, create and queue attestation
        if self.our_signing_key.is_some() && self.our_validator.is_some() {
            self.create_attestation(&proposal).await?;
        }

        Ok(())
    }

    /// Create attestation for a block
    async fn create_attestation(&self, proposal: &BlockProposal) -> Result<(), ChainSyncError> {
        let signing_key = self
            .our_signing_key
            .as_ref()
            .ok_or(ChainSyncError::Internal("No signing key".into()))?;
        let validator = self
            .our_validator
            .as_ref()
            .ok_or(ChainSyncError::Internal("No validator info".into()))?;

        let attestation = BlockAttestation::create(
            proposal.block_hash,
            proposal.slot,
            validator.weight_bps,
            signing_key,
        );

        // Queue for broadcast
        self.pending_attestations
            .write()
            .await
            .push_back(attestation.clone());

        // Process our own attestation
        self.process_attestation(attestation).await?;

        Ok(())
    }

    /// Process an incoming attestation
    pub async fn process_attestation(
        &self,
        attestation: BlockAttestation,
    ) -> Result<(), ChainSyncError> {
        // Verify attestation
        attestation.verify()?;

        // Add to block
        let mut blocks = self.blocks.write().await;
        if let Some(block) = blocks.get_mut(&attestation.block_hash) {
            let is_new = block.add_attestation(attestation.clone());

            if is_new {
                // Broadcast to subscribers
                let _ = self.attestation_tx.send(attestation);

                // Check if we should update canonical head
                if block.is_finalized() {
                    drop(blocks); // Release lock before calling update
                    self.update_canonical_head().await;
                }
            }
        }

        Ok(())
    }

    /// Update fork tracking after new block
    async fn update_forks(&self, proposal: &BlockProposal) {
        let mut forks = self.forks.write().await;

        // Check if this extends an existing fork
        let mut found_fork = false;
        for fork in forks.iter_mut() {
            if fork.head_hash == proposal.parent_hash {
                // Extends this fork
                fork.head_hash = proposal.block_hash;
                fork.head_slot = proposal.slot;
                fork.blocks.push(proposal.block_hash);
                found_fork = true;
                break;
            }
        }

        // Check if this creates a new fork
        if !found_fork {
            let by_slot = self.blocks_by_slot.read().await;
            if let Some(siblings) = by_slot.get(&proposal.parent_slot) {
                if siblings.len() > 1 {
                    // This is a fork
                    let new_fork = ForkState {
                        fork_id: proposal.block_hash,
                        head_hash: proposal.block_hash,
                        head_slot: proposal.slot,
                        total_weight: 0,
                        latest_checkpoint: None,
                        blocks: vec![proposal.block_hash],
                    };
                    forks.push(new_fork);
                }
            }
        }

        // Prune old forks
        let finalized = *self.latest_finalized.read().await;
        if let Some(finalized_block) = self.blocks.read().await.get(&finalized) {
            forks.retain(|f| f.head_slot >= finalized_block.proposal.slot);
        }

        // Limit number of tracked forks
        if forks.len() > MAX_TRACKED_FORKS {
            forks.sort_by(|a, b| b.total_weight.cmp(&a.total_weight));
            forks.truncate(MAX_TRACKED_FORKS);
        }
    }

    /// Update canonical head using GHOST fork choice
    async fn update_canonical_head(&self) {
        let blocks = self.blocks.read().await;
        let finalized = *self.latest_finalized.read().await;

        // Start from finalized block
        let mut current = finalized;
        let mut best_child = current;

        loop {
            if let Some(block) = blocks.get(&current) {
                if block.children.is_empty() {
                    break;
                }

                // Find child with most attestation weight
                let mut best_weight = 0u64;
                for child_hash in &block.children {
                    let weight = self.get_subtree_weight(&blocks, child_hash);
                    if weight > best_weight {
                        best_weight = weight;
                        best_child = *child_hash;
                    }
                }

                if best_child == current {
                    break;
                }
                current = best_child;
            } else {
                break;
            }
        }

        // Update canonical head
        if best_child != finalized {
            *self.canonical_head.write().await = best_child;
        }
    }

    /// Get total attestation weight of a subtree
    fn get_subtree_weight(&self, blocks: &HashMap<Hash, TrackedBlock>, root: &Hash) -> u64 {
        let mut total = 0u64;
        let mut stack = vec![*root];

        while let Some(hash) = stack.pop() {
            if let Some(block) = blocks.get(&hash) {
                total += block.attested_stake_bps;
                stack.extend(block.children.iter());
            }
        }

        total
    }

    /// Finalize a block and its ancestors
    pub async fn finalize_block(&self, block_hash: Hash) -> Result<(), ChainSyncError> {
        let mut blocks = self.blocks.write().await;

        // Finalize ancestors first
        let mut to_finalize = Vec::new();
        let mut current = block_hash;

        while current != self.genesis_hash && current != *self.latest_finalized.read().await {
            if let Some(block) = blocks.get(&current) {
                to_finalize.push(current);
                current = block.proposal.parent_hash;
            } else {
                break;
            }
        }

        // Finalize in order (ancestors first)
        to_finalize.reverse();
        for hash in to_finalize {
            if let Some(block) = blocks.get_mut(&hash) {
                block.status = BlockStatus::Finalized;
                block.finality = FinalityStage::Local;
            }
        }

        // Update latest finalized
        *self.latest_finalized.write().await = block_hash;

        // Orphan conflicting blocks
        self.orphan_conflicting_blocks(&mut blocks, block_hash);

        // Prune old blocks
        self.prune_old_blocks(&mut blocks).await;

        Ok(())
    }

    /// Mark conflicting blocks as orphaned
    fn orphan_conflicting_blocks(&self, blocks: &mut HashMap<Hash, TrackedBlock>, finalized: Hash) {
        // Get finalized block's ancestors
        let mut finalized_chain = HashSet::new();
        let mut current = finalized;

        while current != self.genesis_hash {
            finalized_chain.insert(current);
            if let Some(block) = blocks.get(&current) {
                current = block.proposal.parent_hash;
            } else {
                break;
            }
        }

        // Mark blocks not in finalized chain as orphaned
        for block in blocks.values_mut() {
            if !finalized_chain.contains(&block.proposal.block_hash)
                && block.status != BlockStatus::Orphaned
            {
                // Check if this block is an ancestor of finalized
                if !finalized_chain.contains(&block.proposal.parent_hash) {
                    block.status = BlockStatus::Orphaned;
                }
            }
        }
    }

    /// Remove old blocks to free memory
    async fn prune_old_blocks(&self, blocks: &mut HashMap<Hash, TrackedBlock>) {
        if blocks.len() <= MAX_BLOCKS_IN_MEMORY {
            return;
        }

        let finalized = *self.latest_finalized.read().await;
        let finalized_slot = blocks
            .get(&finalized)
            .map(|b| b.proposal.slot)
            .unwrap_or(SlotId::new(0, 0));

        // Keep blocks within recent slots
        let min_slot_to_keep = if finalized_slot.slot >= MAX_BLOCKS_IN_MEMORY as u64 {
            SlotId::new(
                finalized_slot.epoch,
                finalized_slot.slot - MAX_BLOCKS_IN_MEMORY as u64,
            )
        } else if finalized_slot.epoch > 0 {
            SlotId::new(finalized_slot.epoch - 1, 0)
        } else {
            SlotId::new(0, 0)
        };

        // Remove old blocks
        let to_remove: Vec<Hash> = blocks
            .iter()
            .filter(|(_, b)| {
                b.proposal.slot.absolute_slot(self.epoch_length)
                    < min_slot_to_keep.absolute_slot(self.epoch_length)
            })
            .map(|(h, _)| *h)
            .collect();

        for hash in to_remove {
            blocks.remove(&hash);
        }

        // Also clean up slot index
        let mut by_slot = self.blocks_by_slot.write().await;
        by_slot.retain(|slot, _| {
            slot.absolute_slot(self.epoch_length)
                >= min_slot_to_keep.absolute_slot(self.epoch_length)
        });
    }

    /// Create checkpoint if appropriate
    pub async fn maybe_create_checkpoint(&self, slot: SlotId) -> Option<Checkpoint> {
        // Create checkpoint every CHECKPOINT_INTERVAL_SLOTS
        if slot.absolute_slot(self.epoch_length) % CHECKPOINT_INTERVAL_SLOTS != 0 {
            return None;
        }

        let blocks = self.blocks.read().await;
        let head = *self.canonical_head.read().await;

        let block = blocks.get(&head)?;

        // Only checkpoint finalized blocks
        if !block.is_finalized() {
            return None;
        }

        // Get previous checkpoint
        let checkpoints = self.checkpoints.read().await;
        let previous_checkpoint = checkpoints
            .values()
            .max_by_key(|c| c.slot.absolute_slot(self.epoch_length))
            .map(|c| c.block_hash);
        drop(checkpoints);

        let checkpoint = Checkpoint {
            epoch: slot.epoch,
            slot,
            block_hash: head,
            state_root: block.proposal.state_root,
            previous_checkpoint,
            total_attestation_bps: block.attested_stake_bps,
            attestation_count: block.attestations.len() as u32,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        // Store checkpoint
        self.checkpoints
            .write()
            .await
            .insert(slot.epoch, checkpoint.clone());

        Some(checkpoint)
    }

    /// Get pending attestations to broadcast
    pub async fn drain_pending_attestations(&self) -> Vec<BlockAttestation> {
        self.pending_attestations.write().await.drain(..).collect()
    }

    /// Get sync status
    pub async fn get_sync_status(&self) -> SyncStatus {
        let blocks = self.blocks.read().await;
        let head = *self.canonical_head.read().await;
        let finalized = *self.latest_finalized.read().await;
        let forks = self.forks.read().await;

        SyncStatus {
            canonical_head: head,
            latest_finalized: finalized,
            total_blocks: blocks.len(),
            active_forks: forks.len(),
            is_synced: true, // TODO: Implement proper sync check
        }
    }

    /// Check if we should propose for a slot
    pub async fn should_propose(&self, slot: SlotId) -> bool {
        let validator = match &self.our_validator {
            Some(v) => v,
            None => return false,
        };

        let selector = self.leader_selector.read().await;
        selector.is_leader(slot, &validator.vrf_public_key)
    }

    /// Get the chain height (number of finalized blocks)
    pub async fn chain_height(&self) -> u64 {
        let finalized = *self.latest_finalized.read().await;
        let blocks = self.blocks.read().await;

        if let Some(block) = blocks.get(&finalized) {
            block.proposal.slot.absolute_slot(self.epoch_length)
        } else {
            0
        }
    }
}

/// Sync status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    /// Current canonical head
    pub canonical_head: Hash,
    /// Latest finalized block
    pub latest_finalized: Hash,
    /// Total blocks tracked
    pub total_blocks: usize,
    /// Number of active forks
    pub active_forks: usize,
    /// Whether we're synced with the network
    pub is_synced: bool,
}

/// Network message for chain sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainSyncMessage {
    /// New block proposal
    BlockProposal(BlockProposal),
    /// Attestation for a block
    Attestation(BlockAttestation),
    /// Request for a specific block
    RequestBlock { hash: Hash },
    /// Response with a block
    BlockResponse {
        proposal: Option<BlockProposal>,
        attestations: Vec<BlockAttestation>,
    },
    /// Request chain state
    RequestChainState,
    /// Chain state response
    ChainStateResponse { status: SyncStatus },
    /// New checkpoint
    NewCheckpoint(Checkpoint),
}

#[cfg(test)]
mod tests {
    use super::super::vrf_committees::GeographicRegion;
    use super::*;
    use rand::SeedableRng;
    use schnorrkel::Keypair as SchnorrkelKeypair;

    fn create_genesis_hash() -> Hash {
        Hash::from([0u8; 32])
    }

    fn create_test_signing_key(seed: u64) -> SigningKey {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        SigningKey::generate(&mut rng)
    }

    fn create_test_validator_info(seed: u64) -> ValidatorInfo {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let vrf_keypair = SchnorrkelKeypair::generate_with(&mut rng);
        let signing_key = SigningKey::generate(&mut rng);

        ValidatorInfo {
            vrf_public_key: vrf_keypair.public.to_bytes(),
            signing_public_key: signing_key.verifying_key().to_bytes(),
            stake_amount: 1000,
            weight_bps: 1000,
            region: GeographicRegion::NorthAmerica,
            is_active: true,
            last_leader_slot: None,
            blocks_produced: 0,
            blocks_missed: 0,
        }
    }

    #[tokio::test]
    async fn test_block_proposal_creation() {
        let slot = SlotId::new(0, 1);
        let parent_hash = create_genesis_hash();
        let state_root = Hash::from([1u8; 32]);
        let tx_root = Hash::from([2u8; 32]);

        let signing_key = create_test_signing_key(42);
        let vrf_keypair = {
            let mut rng = rand::rngs::StdRng::seed_from_u64(42);
            SchnorrkelKeypair::generate_with(&mut rng)
        };

        let leader_proof = SlotLeaderProof::generate(
            slot,
            &[0u8; 32],
            &vrf_keypair,
            &signing_key,
            1000,
            GeographicRegion::NorthAmerica,
        );

        let proposal = BlockProposal::new(
            slot,
            parent_hash,
            SlotId::new(0, 0),
            state_root,
            tx_root,
            5,
            leader_proof,
            &signing_key,
        );

        // Verify signature
        assert!(proposal.verify_signature().is_ok());
    }

    #[tokio::test]
    async fn test_attestation_creation() {
        let block_hash = Hash::from([1u8; 32]);
        let slot = SlotId::new(0, 1);
        let signing_key = create_test_signing_key(42);

        let attestation = BlockAttestation::create(block_hash, slot, 1000, &signing_key);

        // Verify attestation
        assert!(attestation.verify().is_ok());
    }

    #[tokio::test]
    async fn test_tracked_block_finality() {
        let signing_key = create_test_signing_key(42);
        let vrf_keypair = {
            let mut rng = rand::rngs::StdRng::seed_from_u64(42);
            SchnorrkelKeypair::generate_with(&mut rng)
        };

        let slot = SlotId::new(0, 1);
        let leader_proof = SlotLeaderProof::generate(
            slot,
            &[0u8; 32],
            &vrf_keypair,
            &signing_key,
            1000,
            GeographicRegion::NorthAmerica,
        );

        let proposal = BlockProposal::new(
            slot,
            Hash::from([0u8; 32]),
            SlotId::new(0, 0),
            Hash::from([1u8; 32]),
            Hash::from([2u8; 32]),
            5,
            leader_proof,
            &signing_key,
        );

        let mut tracked = TrackedBlock::from_proposal(proposal.clone());
        assert_eq!(tracked.status, BlockStatus::Proposed);

        // Add attestations until finality
        for i in 0..7 {
            let key = create_test_signing_key(100 + i);
            let attestation = BlockAttestation::create(
                proposal.block_hash,
                slot,
                1000, // Each 10%, need 67%
                &key,
            );
            tracked.add_attestation(attestation);
        }

        // 7000 bps > 6667 threshold
        assert!(tracked.is_finalized());
    }

    #[tokio::test]
    async fn test_chain_sync_basic() {
        let genesis = create_genesis_hash();
        let sync = ValidatorChainSync::with_defaults(genesis);

        let status = sync.get_sync_status().await;
        assert_eq!(status.canonical_head, genesis);
        assert_eq!(status.latest_finalized, genesis);
    }
}
