//! Minimal Hash-based Merkle tree with inclusion proofs.
//!
//! Used for tx/receipt roots, miniblock root chains, and DA commitments.

use crate::block_hierarchy::Hash;
use crate::canonical::{domain_hash_parts, DOMAIN_SEP_MERKLE_NODE_V1};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashMerkleProof {
    pub path: Vec<Hash>,
    pub directions: Vec<bool>,
}

impl HashMerkleProof {
    pub fn verify(&self, leaf: Hash, expected_root: Hash) -> bool {
        if self.path.len() != self.directions.len() {
            return false;
        }

        let mut current = leaf;
        for (sib, is_right) in self.path.iter().zip(self.directions.iter()) {
            current = if *is_right {
                // current is right child
                domain_hash_parts(
                    DOMAIN_SEP_MERKLE_NODE_V1,
                    &[sib.as_bytes(), current.as_bytes()],
                )
            } else {
                // current is left child
                domain_hash_parts(
                    DOMAIN_SEP_MERKLE_NODE_V1,
                    &[current.as_bytes(), sib.as_bytes()],
                )
            };
        }

        current == expected_root
    }
}

pub fn merkle_root(leaves: &[Hash]) -> Hash {
    if leaves.is_empty() {
        return Hash::from([0u8; 32]);
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        for pair in level.chunks(2) {
            let combined = if pair.len() == 2 {
                domain_hash_parts(
                    DOMAIN_SEP_MERKLE_NODE_V1,
                    &[pair[0].as_bytes(), pair[1].as_bytes()],
                )
            } else {
                pair[0]
            };
            next.push(combined);
        }
        level = next;
    }

    level[0]
}

/// Generate inclusion proof for a leaf index.
///
/// Note: leaves are duplicated to next power-of-two for proof generation.
pub fn merkle_proof(leaves: &[Hash], leaf_index: usize) -> Option<(Hash, HashMerkleProof)> {
    if leaf_index >= leaves.len() {
        return None;
    }

    let leaf = leaves[leaf_index];
    if leaves.len() == 1 {
        return Some((
            leaf,
            HashMerkleProof {
                path: vec![],
                directions: vec![],
            },
        ));
    }

    let next_pow = leaves.len().next_power_of_two();
    let mut padded = Vec::with_capacity(next_pow);
    padded.extend_from_slice(leaves);
    while padded.len() < next_pow {
        padded.push(Hash::from([0u8; 32]));
    }

    let mut idx = leaf_index;
    let mut path = Vec::new();
    let mut directions = Vec::new();

    let mut level = padded;
    while level.len() > 1 {
        let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
        if sibling_idx < level.len() {
            path.push(level[sibling_idx]);
            directions.push(idx % 2 == 1);
        }

        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        for pair in level.chunks(2) {
            let combined = if pair.len() == 2 {
                domain_hash_parts(
                    DOMAIN_SEP_MERKLE_NODE_V1,
                    &[pair[0].as_bytes(), pair[1].as_bytes()],
                )
            } else {
                pair[0]
            };
            next.push(combined);
        }

        idx /= 2;
        level = next;
    }

    Some((leaf, HashMerkleProof { path, directions }))
}
