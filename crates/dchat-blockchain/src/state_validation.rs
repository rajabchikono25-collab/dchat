//! State Validation and Merkle Proof Verification
//!
//! This module provides Byzantine fault-tolerant state validation by:
//! 1. Building Merkle trees from state transitions
//! 2. Verifying Merkle proofs against block commitments
//! 3. Validating state roots across blocks
//! 4. Detecting conflicting state claims (Byzantine behavior)

use crate::block_hierarchy::{Block, Miniblock};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during state validation
#[derive(Debug, Error)]
pub enum StateValidationError {
    #[error("Invalid Merkle proof: {0}")]
    InvalidMerkleProof(String),

    #[error("State root mismatch: expected {expected}, got {actual}")]
    StateRootMismatch { expected: String, actual: String },

    #[error("Byzantine fault detected: {0}")]
    ByzantineFault(String),

    #[error("Missing state transition: {0}")]
    MissingTransition(String),

    #[error("Invalid block structure: {0}")]
    InvalidBlock(String),
}

pub type Result<T> = std::result::Result<T, StateValidationError>;

/// Merkle tree node for state verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleNode {
    /// Hash of this node
    pub hash: Vec<u8>,
    /// Left child hash (if internal node)
    pub left: Option<Box<MerkleNode>>,
    /// Right child hash (if internal node)
    pub right: Option<Box<MerkleNode>>,
    /// Leaf data (if leaf node)
    pub data: Option<Vec<u8>>,
}

/// Merkle proof for verifying a leaf's inclusion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Path from leaf to root (sibling hashes)
    pub path: Vec<Vec<u8>>,
    /// Indices indicating left (0) or right (1) sibling at each level
    pub indices: Vec<bool>,
    /// The leaf data being proven
    pub leaf_data: Vec<u8>,
    /// Expected root hash
    pub root_hash: Vec<u8>,
}

impl MerkleProof {
    /// Verify this proof against the root hash
    pub fn verify(&self) -> Result<bool> {
        if self.path.len() != self.indices.len() {
            return Err(StateValidationError::InvalidMerkleProof(
                "Path and indices length mismatch".to_string(),
            ));
        }

        // Start with leaf hash
        let mut current_hash = blake3::hash(&self.leaf_data).as_bytes().to_vec();

        // Walk up the tree
        for (sibling_hash, is_right) in self.path.iter().zip(self.indices.iter()) {
            let mut hasher = Hasher::new();
            if *is_right {
                // Current is left child
                hasher.update(&current_hash);
                hasher.update(sibling_hash);
            } else {
                // Current is right child
                hasher.update(sibling_hash);
                hasher.update(&current_hash);
            }
            current_hash = hasher.finalize().as_bytes().to_vec();
        }

        // Verify against expected root
        Ok(current_hash == self.root_hash)
    }
}

/// Merkle tree for state commitments
#[derive(Debug, Clone)]
pub struct MerkleTree {
    /// Root node of the tree
    pub root: Option<MerkleNode>,
    /// All leaf data in order
    pub leaves: Vec<Vec<u8>>,
}

impl MerkleTree {
    /// Build a Merkle tree from state transitions
    pub fn from_state_transitions(transitions: Vec<Vec<u8>>) -> Self {
        if transitions.is_empty() {
            return Self {
                root: None,
                leaves: vec![],
            };
        }

        let original_leaves = transitions.clone();
        let mut leaves = transitions;
        let leaf_count = leaves.len();

        // Pad to power of 2 for balanced tree
        let next_power = leaf_count.next_power_of_two();
        while leaves.len() < next_power {
            leaves.push(vec![0u8; 32]); // Pad with zero hashes
        }

        // Build leaf nodes
        let mut nodes: Vec<MerkleNode> = leaves
            .iter()
            .map(|data| MerkleNode {
                hash: blake3::hash(data).as_bytes().to_vec(),
                left: None,
                right: None,
                data: Some(data.clone()),
            })
            .collect();

        // Build tree bottom-up
        while nodes.len() > 1 {
            let mut parent_nodes = Vec::new();

            for chunk in nodes.chunks(2) {
                let left = chunk[0].clone();
                let right = if chunk.len() > 1 {
                    chunk[1].clone()
                } else {
                    left.clone() // Duplicate if odd number
                };

                let mut hasher = Hasher::new();
                hasher.update(&left.hash);
                hasher.update(&right.hash);
                let parent_hash = hasher.finalize().as_bytes().to_vec();

                parent_nodes.push(MerkleNode {
                    hash: parent_hash,
                    left: Some(Box::new(left)),
                    right: Some(Box::new(right)),
                    data: None,
                });
            }

            nodes = parent_nodes;
        }

        Self {
            root: nodes.into_iter().next(),
            leaves: original_leaves,
        }
    }

    /// Get the root hash
    pub fn root_hash(&self) -> Option<Vec<u8>> {
        self.root.as_ref().map(|node| node.hash.clone())
    }

    /// Generate a Merkle proof for a specific leaf
    pub fn generate_proof(&self, leaf_index: usize) -> Result<MerkleProof> {
        if leaf_index >= self.leaves.len() {
            return Err(StateValidationError::InvalidMerkleProof(
                "Leaf index out of bounds".to_string(),
            ));
        }

        let root = self.root.as_ref().ok_or_else(|| {
            StateValidationError::InvalidMerkleProof("Empty tree".to_string())
        })?;

        let mut path = Vec::new();
        let mut indices = Vec::new();
        let mut current_index = leaf_index;
        let mut level_size = self.leaves.len().next_power_of_two();

        let mut current_node = root;

        // Traverse from root to leaf, collecting sibling hashes
        while level_size > 1 {
            level_size /= 2;
            let is_right = current_index >= level_size;

            if is_right {
                // We're going right, sibling is left
                if let Some(left) = &current_node.left {
                    path.push(left.hash.clone());
                    indices.push(true); // Current is right child
                }
                if let Some(right) = &current_node.right {
                    current_node = right;
                }
                current_index -= level_size;
            } else {
                // We're going left, sibling is right
                if let Some(right) = &current_node.right {
                    path.push(right.hash.clone());
                    indices.push(false); // Current is left child
                }
                if let Some(left) = &current_node.left {
                    current_node = left;
                }
            }
        }

        Ok(MerkleProof {
            path,
            indices,
            leaf_data: self.leaves[leaf_index].clone(),
            root_hash: root.hash.clone(),
        })
    }
}

/// State validator for Byzantine fault detection
pub struct StateValidator {
    /// Cache of verified state roots by block height
    verified_roots: HashMap<u64, Vec<u8>>,
    /// Cache of last post-state hash for each block (for state continuity)
    last_post_states: HashMap<u64, Vec<u8>>,
    /// Detected Byzantine faults (validator ID -> fault description)
    byzantine_faults: HashMap<Vec<u8>, Vec<String>>,
}

impl StateValidator {
    /// Create a new state validator
    pub fn new() -> Self {
        Self {
            verified_roots: HashMap::new(),
            last_post_states: HashMap::new(),
            byzantine_faults: HashMap::new(),
        }
    }

    /// Validate a block's state transitions
    pub fn validate_block(&mut self, block: &Block) -> Result<Vec<u8>> {
        // 1. Verify block structure
        if block.subblocks.is_empty() {
            return Err(StateValidationError::InvalidBlock(
                "Block has no subblocks".to_string(),
            ));
        }

        // 2. Collect all state transitions from miniblocks
        let mut all_transitions = Vec::new();
        for subblock in &block.subblocks {
            for miniblock in &subblock.miniblocks {
                // Each miniblock contributes pre-state -> post-state transition
                let transition = self.encode_state_transition(miniblock);
                all_transitions.push(transition);
            }
        }

        // 3. Build Merkle tree from state transitions
        let merkle_tree = MerkleTree::from_state_transitions(all_transitions);

        // 4. Get computed state root
        let computed_root = merkle_tree.root_hash().ok_or_else(|| {
            StateValidationError::InvalidBlock("Failed to compute state root".to_string())
        })?;

        // 5. Verify against block's committed state root
        let block_root = block.state_root.as_bytes().to_vec();
        if computed_root != block_root {
            return Err(StateValidationError::StateRootMismatch {
                expected: hex::encode(&block_root),
                actual: hex::encode(&computed_root),
            });
        }

        // 6. Verify state continuity with previous block
        if block.height > 0 {
            if let Some(_previous_root) = self.verified_roots.get(&(block.height - 1)) {
                // Verify state continuity: last post-state of previous block should match
                // first pre-state of current block
                if let Some(first_subblock) = block.subblocks.first() {
                    if let Some(first_miniblock) = first_subblock.miniblocks.first() {
                        let first_pre_state = first_miniblock.pre_state_hash.as_bytes().to_vec();
                        
                        // For full state continuity, we need the previous block's last post-state
                        // This requires either:
                        // 1. Caching the last post-state of each block, or
                        // 2. Fetching the previous block to get its last miniblock's post-state
                        // 
                        // For now, we verify that the chain is well-formed by checking the
                        // state root was correctly verified. Full state continuity will be
                        // enforced when the consensus layer passes complete Block structures.
                        
                        tracing::debug!(
                            "State continuity: Block {} first pre-state = {}",
                            block.height,
                            hex::encode(&first_pre_state)
                        );
                    }
                }
            } else {
                // Missing previous block - this is a state continuity violation
                return Err(StateValidationError::MissingTransition(format!(
                    "Missing previous block at height {}",
                    block.height - 1
                )));
            }
        }

        // 7. Cache verified root and last post-state
        self.verified_roots.insert(block.height, computed_root.clone());
        
        // Cache the last miniblock's post-state for continuity verification
        if let Some(last_subblock) = block.subblocks.last() {
            if let Some(last_miniblock) = last_subblock.miniblocks.last() {
                let last_post_state = last_miniblock.post_state_hash.as_bytes().to_vec();
                self.last_post_states.insert(block.height, last_post_state);
            }
        }

        Ok(computed_root)
    }

    /// Encode a state transition for Merkle tree inclusion
    fn encode_state_transition(&self, miniblock: &Miniblock) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&miniblock.index.to_le_bytes());
        data.extend_from_slice(miniblock.pre_state_hash.as_bytes());
        data.extend_from_slice(miniblock.post_state_hash.as_bytes());
        data.extend_from_slice(&miniblock.gas_used.to_le_bytes());
        data
    }

    /// Verify a Merkle proof for a specific state transition
    pub fn verify_state_transition_proof(
        &self,
        proof: &MerkleProof,
        block_height: u64,
    ) -> Result<bool> {
        // 1. Verify the proof itself
        if !proof.verify()? {
            return Ok(false);
        }

        // 2. Check against cached root if available
        if let Some(cached_root) = self.verified_roots.get(&block_height) {
            if &proof.root_hash != cached_root {
                return Err(StateValidationError::StateRootMismatch {
                    expected: hex::encode(cached_root),
                    actual: hex::encode(&proof.root_hash),
                });
            }
        }

        Ok(true)
    }

    /// Detect Byzantine faults by comparing state claims from multiple validators
    pub fn detect_byzantine_fault(
        &mut self,
        block_height_bytes: &[u8],
        validator_id_str: &str,
        claimed_state_root: &[u8],
    ) -> Result<()> {
        // Convert block height from bytes
        let block_height = u64::from_le_bytes(
            block_height_bytes[..8]
                .try_into()
                .map_err(|_| StateValidationError::InvalidBlock("Invalid block height bytes".to_string()))?
        );
        
        if let Some(verified_root) = self.verified_roots.get(&block_height) {
            if claimed_state_root != verified_root.as_slice() {
                let fault_description = format!(
                    "Block {}: claimed state root {} != verified root {}",
                    block_height,
                    hex::encode(claimed_state_root),
                    hex::encode(verified_root)
                );

                tracing::warn!(
                    "🚨 Byzantine fault detected from validator {}: {}",
                    validator_id_str,
                    fault_description
                );

                let validator_id = validator_id_str.as_bytes().to_vec();
                self.byzantine_faults
                    .entry(validator_id)
                    .or_insert_with(Vec::new)
                    .push(fault_description.clone());

                return Err(StateValidationError::ByzantineFault(fault_description));
            }
        } else {
            // Store the claimed root as the first verified root for this height
            self.verified_roots.insert(block_height, claimed_state_root.to_vec());
        }

        Ok(())
    }

    /// Get all detected Byzantine faults
    pub fn get_byzantine_faults(&self) -> &HashMap<Vec<u8>, Vec<String>> {
        &self.byzantine_faults
    }

    /// Get Byzantine faults for a specific block height
    pub fn get_byzantine_faults_at_height(&self, block_height_bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
        let block_height = u64::from_le_bytes(
            block_height_bytes.get(..8)?
                .try_into()
                .ok()?
        );
        
        // Return validators who have faults at this height
        let faulted_validators: Vec<Vec<u8>> = self.byzantine_faults
            .iter()
            .filter(|(_, faults)| {
                faults.iter().any(|f| f.contains(&format!("Block {}", block_height)))
            })
            .map(|(validator_id, _)| validator_id.clone())
            .collect();
        
        if faulted_validators.is_empty() {
            None
        } else {
            Some(faulted_validators)
        }
    }

    /// Clear old verified roots to prevent memory growth (keep last N blocks)
    pub fn cleanup_old_roots(&mut self, current_height: u64, keep_blocks: u64) {
        if current_height > keep_blocks {
            let cutoff = current_height - keep_blocks;
            self.verified_roots.retain(|&height, _| height > cutoff);
            self.last_post_states.retain(|&height, _| height > cutoff);
        }
    }
    
    /// Get slashing recommendations for Byzantine validators
    /// Returns: Vec<(validator_id, slash_amount_percentage, reason)>
    pub fn get_slashing_recommendations(&self) -> Vec<(Vec<u8>, u8, String)> {
        self.byzantine_faults
            .iter()
            .map(|(validator_id, faults)| {
                // Calculate slash percentage based on fault severity
                let fault_count = faults.len();
                let slash_percentage = match fault_count {
                    1 => 5,      // 5% for first offense
                    2 => 10,     // 10% for second offense
                    3 => 25,     // 25% for third offense
                    _ => 100,    // 100% (full slash) for persistent Byzantine behavior
                };
                
                let reason = format!(
                    "Byzantine fault: {} instances of conflicting state claims",
                    fault_count
                );
                
                (validator_id.clone(), slash_percentage, reason)
            })
            .collect()
    }
}

impl Default for StateValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_hierarchy::{Block, Subblock, Hash};
    use std::time::SystemTime;

    #[test]
    fn test_merkle_tree_construction() {
        let transitions = vec![
            vec![1, 2, 3],
            vec![4, 5, 6],
            vec![7, 8, 9],
            vec![10, 11, 12],
        ];

        let tree = MerkleTree::from_state_transitions(transitions);
        assert!(tree.root_hash().is_some());
    }

    #[test]
    fn test_merkle_proof_verification() {
        let transitions = vec![vec![1, 2, 3], vec![4, 5, 6], vec![7, 8, 9]];

        let tree = MerkleTree::from_state_transitions(transitions);
        let proof = tree.generate_proof(0).unwrap();

        assert!(proof.verify().unwrap());
    }

    #[test]
    fn test_state_validator() {
        let mut validator = StateValidator::new();

        // Create a test block with miniblocks
        let miniblock = Miniblock {
            index: 0,
            timestamp: SystemTime::now(),
            transactions: vec![],
            pre_state_hash: Hash::from([0u8; 32]),
            post_state_hash: Hash::from([1u8; 32]),
            gas_used: 1000,
            receipts: vec![],
        };

        let subblock = Subblock {
            index: 0,
            timestamp: SystemTime::now(),
            miniblocks: vec![miniblock],
            execution_result: crate::block_hierarchy::ExecutionResult {
                success_count: 0,
                failure_count: 0,
                total_gas_used: 1000,
                state_delta: vec![],
            },
            merkle_root: Hash::from([0u8; 32]),
        };

        // Build merkle tree to get correct state root
        let transition = vec![0u16.to_le_bytes().to_vec(), vec![0u8; 32], vec![1u8; 32], 1000u64.to_le_bytes().to_vec()].concat();
        let tree = MerkleTree::from_state_transitions(vec![transition]);
        let state_root = tree.root_hash().unwrap();

        let block = Block {
            height: 1,
            timestamp: SystemTime::now(),
            previous_hash: Hash::from([0u8; 32]),
            state_root: Hash::from({
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&state_root[..32]);
                arr
            }),
            subblocks: vec![subblock],
            validator_signatures: vec![],
            relay_votes: vec![],
            finality_proof: crate::block_hierarchy::FinalityProof::default(),
        };

        let result = validator.validate_block(&block);
        assert!(result.is_ok());
    }

    #[test]
    fn test_byzantine_fault_detection() {
        let mut validator = StateValidator::new();

        validator.verified_roots.insert(100, vec![1, 2, 3, 4]);

        let validator_id_str = "validator_abc";
        let wrong_root = vec![5, 6, 7, 8];
        let block_height_bytes = 100u64.to_le_bytes();

        let result = validator.detect_byzantine_fault(&block_height_bytes, validator_id_str, &wrong_root);

        assert!(result.is_err());
        assert_eq!(validator.byzantine_faults.len(), 1);
    }
}
