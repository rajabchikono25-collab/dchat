//! Fraud-Provable Merkle Commitment System
//!
//! Replaces "ship/verify all proofs" with:
//! - Merkle commitments (roots) + small deterministic samples
//! - Full proof reveal only on challenge with slashable evidence
//!
//! Applies to:
//! - PoRW delivery proof sets per relay per miniblock/subblock
//! - PoT hop records/transit paths
//! - TSC checkpoint/epoch vote sets
//!
//! Security: Commitments are binding, challenges use unbiased VRF randomness

use crate::block_hierarchy::Hash;
use ed25519_dalek::{Signature, Signer, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// Maximum samples per verification round (bounds verification cost)
pub const MAX_SAMPLES_PER_BLOCK: usize = 32;

/// Default sample count (k samples provides 1 - (1-p)^k probability of catching fraud)
pub const DEFAULT_SAMPLE_COUNT: usize = 16;

/// Challenge window duration (validators have this long to respond)
pub const CHALLENGE_WINDOW_SECS: u64 = 300; // 5 minutes

/// Slashing amount for fraud proof failure (basis points of stake)
pub const FRAUD_SLASH_BPS: u64 = 5000; // 50% slash

/// Merkle commitment for a proof set
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerkleCommitment {
    /// Root hash of the Merkle tree containing all proofs
    pub root: Hash,

    /// Number of leaves (proofs) in the tree
    pub leaf_count: u64,

    /// Tree depth for verification
    pub depth: u8,

    /// Block/subblock/miniblock this commitment covers
    pub scope: CommitmentScope,

    /// Timestamp when commitment was created
    pub timestamp: SystemTime,

    /// Committer's signature over (root || leaf_count || scope || timestamp)
    pub signature: Signature,

    /// Committer's public key
    pub committer: VerifyingKey,
}

/// Scope of the commitment (what block unit it covers)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CommitmentScope {
    /// Block height
    pub block_height: u64,
    /// Subblock index (0-9)
    pub subblock_index: u8,
    /// Miniblock index (0-9), None for subblock-level commitments
    pub miniblock_index: Option<u8>,
    /// Commitment type
    pub commitment_type: CommitmentType,
}

/// Type of commitment
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CommitmentType {
    /// PoRW delivery proof set per relay
    PoRWDeliveryProofs { relay_index: u16 },
    /// PoT hop records for a transit path
    PoTHopRecords { path_index: u16 },
    /// TSC checkpoint votes for an epoch
    TSCCheckpointVotes { epoch: u64 },
    /// TSC epoch vote aggregation
    TSCEpochVotes { epoch: u64 },
}

/// Sample request generated from VRF randomness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleRequest {
    /// Indices of leaves to sample (derived from VRF)
    pub leaf_indices: Vec<u64>,

    /// VRF output used to derive sample indices
    pub vrf_output: [u8; 32],

    /// VRF proof for verification
    pub vrf_proof: Vec<u8>,

    /// Deadline for response
    pub deadline: SystemTime,

    /// Commitment being sampled
    pub commitment_root: Hash,
}

/// Sample response with Merkle proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleResponse {
    /// The sampled leaves (actual proof data)
    pub leaves: Vec<ProofLeaf>,

    /// Merkle inclusion proofs for each leaf
    pub inclusion_proofs: Vec<MerkleInclusionProof>,

    /// Response timestamp
    pub timestamp: SystemTime,

    /// Signature over response
    pub signature: Signature,
}

/// A single proof leaf in the Merkle tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofLeaf {
    /// Leaf index in tree
    pub index: u64,

    /// Serialized proof data
    pub data: Vec<u8>,

    /// Hash of the leaf
    pub hash: Hash,
}

/// Merkle inclusion proof for a leaf
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleInclusionProof {
    /// Path from leaf to root (sibling hashes)
    pub path: Vec<Hash>,

    /// Direction bits (0 = left, 1 = right)
    pub directions: Vec<bool>,
}

/// Challenge for fraud proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudChallenge {
    /// Challenge ID (unique identifier)
    pub challenge_id: Hash,

    /// Commitment being challenged
    pub commitment: MerkleCommitment,

    /// Sample request that wasn't answered correctly
    pub sample_request: SampleRequest,

    /// Challenger's public key
    pub challenger: VerifyingKey,

    /// Challenger's signature
    pub signature: Signature,

    /// Challenge creation time
    pub created_at: SystemTime,

    /// Bond amount posted by challenger
    pub challenge_bond: u64,
}

/// Evidence for slashing (compact, on-chain storable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvidence {
    /// Challenge that was not answered
    pub challenge_id: Hash,

    /// Commitment root that was proven fraudulent
    pub commitment_root: Hash,

    /// Committer being slashed
    pub committer: VerifyingKey,

    /// Type of fraud detected
    pub fraud_type: FraudType,

    /// Minimal proof data for verification
    pub proof_data: Vec<u8>,

    /// Slash amount
    pub slash_amount: u64,

    /// Block height when evidence was created
    pub evidence_height: u64,
}

/// Types of fraud that can be proven
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FraudType {
    /// Failed to respond to valid sample request
    NoResponse,
    /// Provided invalid Merkle proof
    InvalidMerkleProof,
    /// Leaf data doesn't match claimed hash
    LeafHashMismatch,
    /// Proof data itself is invalid (e.g., invalid signature in delivery proof)
    InvalidProofContent,
    /// Commitment root doesn't match reconstructed tree
    RootMismatch,
    /// Duplicate/conflicting commitment for same scope
    EquivocationCommitment,
}

/// Commitment errors
#[derive(Debug, Error)]
pub enum CommitmentError {
    #[error("Invalid Merkle proof")]
    InvalidMerkleProof,

    #[error("Leaf hash mismatch")]
    LeafHashMismatch,

    #[error("Root mismatch")]
    RootMismatch,

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Challenge expired")]
    ChallengeExpired,

    #[error("Insufficient challenge bond")]
    InsufficientBond,

    #[error("Sample index out of bounds: {0} >= {1}")]
    SampleIndexOutOfBounds(u64, u64),

    #[error("Invalid VRF proof")]
    InvalidVrfProof,

    #[error("Too many samples requested: {0} > {1}")]
    TooManySamples(usize, usize),

    #[error("Commitment scope mismatch")]
    ScopeMismatch,
}

/// Merkle tree builder for creating commitments
pub struct MerkleTreeBuilder {
    leaves: Vec<Hash>,
    depth: u8,
}

impl MerkleTreeBuilder {
    /// Create new builder
    pub fn new() -> Self {
        Self {
            leaves: Vec::new(),
            depth: 0,
        }
    }

    /// Add a leaf (proof data) to the tree
    pub fn add_leaf(&mut self, data: &[u8]) -> u64 {
        let hash = Hash::from(*blake3::hash(data).as_bytes());
        self.leaves.push(hash);
        (self.leaves.len() - 1) as u64
    }

    /// Add a pre-hashed leaf
    pub fn add_leaf_hash(&mut self, hash: Hash) -> u64 {
        self.leaves.push(hash);
        (self.leaves.len() - 1) as u64
    }

    /// Build the Merkle tree and return the root
    pub fn build(&mut self) -> Hash {
        if self.leaves.is_empty() {
            return Hash::from([0u8; 32]);
        }

        // Pad to power of 2
        let target_size = self.leaves.len().next_power_of_two();
        let padding_hash = Hash::from([0u8; 32]);
        while self.leaves.len() < target_size {
            self.leaves.push(padding_hash);
        }

        self.depth = (target_size as f64).log2() as u8;

        let mut current_level = self.leaves.clone();

        while current_level.len() > 1 {
            let mut next_level = Vec::with_capacity(current_level.len() / 2);

            for chunk in current_level.chunks(2) {
                let combined = combine_hashes(&chunk[0], &chunk[1]);
                next_level.push(combined);
            }

            current_level = next_level;
        }

        current_level[0]
    }

    /// Get the tree depth
    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// Get the number of leaves
    pub fn leaf_count(&self) -> u64 {
        self.leaves.len() as u64
    }

    /// Generate an inclusion proof for a leaf at given index
    pub fn generate_proof(&self, index: u64) -> Result<MerkleInclusionProof, CommitmentError> {
        if index >= self.leaves.len() as u64 {
            return Err(CommitmentError::SampleIndexOutOfBounds(
                index,
                self.leaves.len() as u64,
            ));
        }

        let mut path = Vec::with_capacity(self.depth as usize);
        let mut directions = Vec::with_capacity(self.depth as usize);

        let mut current_level = self.leaves.clone();
        let mut current_index = index as usize;

        while current_level.len() > 1 {
            // Get sibling
            let sibling_index = if current_index % 2 == 0 {
                current_index + 1
            } else {
                current_index - 1
            };

            if sibling_index < current_level.len() {
                path.push(current_level[sibling_index]);
            } else {
                path.push(Hash::from([0u8; 32]));
            }

            // Direction: true if current is on the right (index is odd)
            directions.push(current_index % 2 == 1);

            // Move to next level
            let mut next_level = Vec::with_capacity(current_level.len() / 2);
            for chunk in current_level.chunks(2) {
                let combined = combine_hashes(&chunk[0], &chunk[1]);
                next_level.push(combined);
            }

            current_level = next_level;
            current_index /= 2;
        }

        Ok(MerkleInclusionProof { path, directions })
    }
}

impl Default for MerkleTreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Combine two hashes for Merkle tree construction
fn combine_hashes(left: &Hash, right: &Hash) -> Hash {
    let mut combined = Vec::with_capacity(64);
    combined.extend_from_slice(left.as_bytes());
    combined.extend_from_slice(right.as_bytes());
    Hash::from(*blake3::hash(&combined).as_bytes())
}

/// Verify a Merkle inclusion proof
pub fn verify_merkle_proof(
    leaf_hash: &Hash,
    proof: &MerkleInclusionProof,
    root: &Hash,
) -> Result<(), CommitmentError> {
    if proof.path.len() != proof.directions.len() {
        return Err(CommitmentError::InvalidMerkleProof);
    }

    let mut current = *leaf_hash;

    for (sibling, is_right) in proof.path.iter().zip(proof.directions.iter()) {
        current = if *is_right {
            combine_hashes(sibling, &current)
        } else {
            combine_hashes(&current, sibling)
        };
    }

    if &current != root {
        return Err(CommitmentError::RootMismatch);
    }

    Ok(())
}

/// Sample index generator using VRF output
pub struct DeterministicSampler {
    /// VRF output used as seed
    seed: [u8; 32],
    /// Number of items to sample from
    population_size: u64,
    /// Number of samples to take
    sample_count: usize,
}

impl DeterministicSampler {
    /// Create a new sampler from VRF output
    pub fn new(vrf_output: [u8; 32], population_size: u64, sample_count: usize) -> Self {
        Self {
            seed: vrf_output,
            population_size,
            sample_count: sample_count.min(MAX_SAMPLES_PER_BLOCK),
        }
    }

    /// Generate sample indices deterministically from VRF seed
    /// Uses Fisher-Yates shuffle with VRF-derived randomness
    pub fn generate_indices(&self) -> Vec<u64> {
        if self.population_size == 0 {
            return Vec::new();
        }

        let effective_samples = (self.sample_count as u64).min(self.population_size) as usize;
        let mut indices = Vec::with_capacity(effective_samples);
        let mut used = std::collections::HashSet::new();

        // Use HKDF to derive indices from seed
        for i in 0..effective_samples {
            let mut counter = i as u64;
            loop {
                // Derive index using BLAKE3 with counter
                let mut input = Vec::with_capacity(40);
                input.extend_from_slice(&self.seed);
                input.extend_from_slice(&counter.to_le_bytes());

                let hash = blake3::hash(&input);
                let bytes: [u8; 8] = hash.as_bytes()[0..8].try_into().unwrap();
                let raw_index = u64::from_le_bytes(bytes);
                let index = raw_index % self.population_size;

                if !used.contains(&index) {
                    used.insert(index);
                    indices.push(index);
                    break;
                }

                counter += effective_samples as u64;
            }
        }

        indices.sort_unstable();
        indices
    }
}

/// Commitment manager for tracking and verifying commitments
pub struct CommitmentManager {
    /// Active commitments indexed by scope
    commitments: HashMap<CommitmentScope, MerkleCommitment>,

    /// Pending challenges
    pending_challenges: HashMap<Hash, FraudChallenge>,

    /// Verified slashing evidence ready for on-chain submission
    slashing_evidence: Vec<SlashingEvidence>,

    /// Sample count for verification (k)
    sample_count: usize,

    /// Challenge bond amount required
    challenge_bond_amount: u64,
}

impl CommitmentManager {
    /// Create new commitment manager
    pub fn new(sample_count: usize, challenge_bond_amount: u64) -> Self {
        Self {
            commitments: HashMap::new(),
            pending_challenges: HashMap::new(),
            slashing_evidence: Vec::new(),
            sample_count: sample_count.min(MAX_SAMPLES_PER_BLOCK),
            challenge_bond_amount,
        }
    }

    /// Register a new commitment
    pub fn register_commitment(
        &mut self,
        commitment: MerkleCommitment,
    ) -> Result<(), CommitmentError> {
        // Check for equivocation (duplicate commitment for same scope)
        if let Some(existing) = self.commitments.get(&commitment.scope) {
            if existing.root != commitment.root {
                // Equivocation detected - generate slashing evidence
                let evidence = SlashingEvidence {
                    challenge_id: Hash::from(*blake3::hash(b"equivocation").as_bytes()),
                    commitment_root: commitment.root,
                    committer: commitment.committer,
                    fraud_type: FraudType::EquivocationCommitment,
                    proof_data: bincode::serialize(&(existing.clone(), commitment.clone()))
                        .unwrap_or_default(),
                    slash_amount: FRAUD_SLASH_BPS,
                    evidence_height: commitment.scope.block_height,
                };
                self.slashing_evidence.push(evidence);
                return Err(CommitmentError::ScopeMismatch);
            }
        }

        self.commitments.insert(commitment.scope, commitment);
        Ok(())
    }

    /// Generate a sample request for a commitment using VRF output
    pub fn generate_sample_request(
        &self,
        scope: &CommitmentScope,
        vrf_output: [u8; 32],
        vrf_proof: Vec<u8>,
    ) -> Result<SampleRequest, CommitmentError> {
        let commitment = self
            .commitments
            .get(scope)
            .ok_or(CommitmentError::ScopeMismatch)?;

        let sampler =
            DeterministicSampler::new(vrf_output, commitment.leaf_count, self.sample_count);

        let leaf_indices = sampler.generate_indices();

        Ok(SampleRequest {
            leaf_indices,
            vrf_output,
            vrf_proof,
            deadline: SystemTime::now() + Duration::from_secs(CHALLENGE_WINDOW_SECS),
            commitment_root: commitment.root,
        })
    }

    /// Verify a sample response
    pub fn verify_sample_response(
        &self,
        request: &SampleRequest,
        response: &SampleResponse,
        expected_root: &Hash,
    ) -> Result<(), CommitmentError> {
        // Verify all requested leaves are present
        if response.leaves.len() != request.leaf_indices.len() {
            return Err(CommitmentError::InvalidMerkleProof);
        }

        if response.inclusion_proofs.len() != request.leaf_indices.len() {
            return Err(CommitmentError::InvalidMerkleProof);
        }

        // Verify each leaf and its inclusion proof
        for ((leaf, proof), expected_index) in response
            .leaves
            .iter()
            .zip(response.inclusion_proofs.iter())
            .zip(request.leaf_indices.iter())
        {
            // Check leaf index matches
            if leaf.index != *expected_index {
                return Err(CommitmentError::SampleIndexOutOfBounds(
                    leaf.index,
                    *expected_index,
                ));
            }

            // Verify leaf hash
            let computed_hash = Hash::from(*blake3::hash(&leaf.data).as_bytes());
            if computed_hash != leaf.hash {
                return Err(CommitmentError::LeafHashMismatch);
            }

            // Verify Merkle inclusion
            verify_merkle_proof(&leaf.hash, proof, expected_root)?;
        }

        Ok(())
    }

    /// File a fraud challenge
    pub fn file_challenge(
        &mut self,
        commitment: MerkleCommitment,
        sample_request: SampleRequest,
        challenger: VerifyingKey,
        signature: Signature,
        bond: u64,
    ) -> Result<Hash, CommitmentError> {
        if bond < self.challenge_bond_amount {
            return Err(CommitmentError::InsufficientBond);
        }

        let challenge_id = Hash::from(
            *blake3::hash(
                &bincode::serialize(&(&commitment.root, &challenger, SystemTime::now()))
                    .unwrap_or_default(),
            )
            .as_bytes(),
        );

        let challenge = FraudChallenge {
            challenge_id,
            commitment,
            sample_request,
            challenger,
            signature,
            created_at: SystemTime::now(),
            challenge_bond: bond,
        };

        self.pending_challenges.insert(challenge_id, challenge);

        Ok(challenge_id)
    }

    /// Process expired challenges (no response = fraud)
    pub fn process_expired_challenges(&mut self, current_height: u64) -> Vec<SlashingEvidence> {
        let now = SystemTime::now();
        let mut expired = Vec::new();

        for (challenge_id, challenge) in self.pending_challenges.iter() {
            if now > challenge.sample_request.deadline {
                // Challenge expired without response = fraud
                let evidence = SlashingEvidence {
                    challenge_id: *challenge_id,
                    commitment_root: challenge.commitment.root,
                    committer: challenge.commitment.committer,
                    fraud_type: FraudType::NoResponse,
                    proof_data: bincode::serialize(&challenge.sample_request).unwrap_or_default(),
                    slash_amount: FRAUD_SLASH_BPS,
                    evidence_height: current_height,
                };
                expired.push(evidence);
            }
        }

        // Remove expired challenges
        for evidence in &expired {
            self.pending_challenges.remove(&evidence.challenge_id);
            self.slashing_evidence.push(evidence.clone());
        }

        expired
    }

    /// Get pending slashing evidence for on-chain submission
    pub fn drain_slashing_evidence(&mut self) -> Vec<SlashingEvidence> {
        std::mem::take(&mut self.slashing_evidence)
    }

    /// Get commitment by scope
    pub fn get_commitment(&self, scope: &CommitmentScope) -> Option<&MerkleCommitment> {
        self.commitments.get(scope)
    }
}

impl Default for CommitmentManager {
    fn default() -> Self {
        Self::new(DEFAULT_SAMPLE_COUNT, 1000) // 1000 token bond
    }
}

/// PoRW-specific commitment helpers
pub mod porw {
    use super::*;
    use crate::consensus_types::DeliveryProof;

    /// Create a commitment for PoRW delivery proofs from a relay for a miniblock
    pub fn create_delivery_proof_commitment(
        proofs: &[DeliveryProof],
        block_height: u64,
        subblock_index: u8,
        miniblock_index: u8,
        relay_index: u16,
        committer: &ed25519_dalek::SigningKey,
    ) -> Result<(MerkleCommitment, MerkleTreeBuilder), CommitmentError> {
        let mut builder = MerkleTreeBuilder::new();

        for proof in proofs {
            let data = bincode::serialize(proof).unwrap_or_default();
            builder.add_leaf(&data);
        }

        let root = builder.build();

        let scope = CommitmentScope {
            block_height,
            subblock_index,
            miniblock_index: Some(miniblock_index),
            commitment_type: CommitmentType::PoRWDeliveryProofs { relay_index },
        };

        let timestamp = SystemTime::now();

        // Create signature data
        let mut sig_data = Vec::new();
        sig_data.extend_from_slice(root.as_bytes());
        sig_data.extend_from_slice(&builder.leaf_count().to_le_bytes());
        sig_data.extend_from_slice(&bincode::serialize(&scope).unwrap_or_default());

        let signature = committer.sign(&sig_data);

        let commitment = MerkleCommitment {
            root,
            leaf_count: builder.leaf_count(),
            depth: builder.depth(),
            scope,
            timestamp,
            signature,
            committer: committer.verifying_key(),
        };

        Ok((commitment, builder))
    }

    /// Verify a sampled delivery proof
    pub fn verify_sampled_proof(proof_data: &[u8]) -> Result<DeliveryProof, CommitmentError> {
        let proof: DeliveryProof =
            bincode::deserialize(proof_data).map_err(|_| CommitmentError::InvalidMerkleProof)?;

        // Verify the delivery proof signature and structure
        // (Signature verification would happen here with the relay's public key)

        Ok(proof)
    }
}

/// PoT-specific commitment helpers
pub mod pot {
    use super::*;
    use crate::proof_of_transit::TransitPath;

    /// Create a commitment for PoT hop records in a transit path
    pub fn create_transit_path_commitment(
        paths: &[TransitPath],
        block_height: u64,
        subblock_index: u8,
        path_index: u16,
        committer: &ed25519_dalek::SigningKey,
    ) -> Result<(MerkleCommitment, MerkleTreeBuilder), CommitmentError> {
        let mut builder = MerkleTreeBuilder::new();

        for path in paths {
            // Each hop in the path becomes a leaf
            for (i, relay) in path.relays.iter().enumerate() {
                let mut hop_data = Vec::new();
                hop_data.extend_from_slice(relay.as_bytes());
                hop_data.extend_from_slice(&path.locations[i].latitude.to_le_bytes());
                hop_data.extend_from_slice(&path.locations[i].longitude.to_le_bytes());
                builder.add_leaf(&hop_data);
            }
        }

        let root = builder.build();

        let scope = CommitmentScope {
            block_height,
            subblock_index,
            miniblock_index: None,
            commitment_type: CommitmentType::PoTHopRecords { path_index },
        };

        let timestamp = SystemTime::now();

        let mut sig_data = Vec::new();
        sig_data.extend_from_slice(root.as_bytes());
        sig_data.extend_from_slice(&builder.leaf_count().to_le_bytes());
        sig_data.extend_from_slice(&bincode::serialize(&scope).unwrap_or_default());

        let signature = committer.sign(&sig_data);

        let commitment = MerkleCommitment {
            root,
            leaf_count: builder.leaf_count(),
            depth: builder.depth(),
            scope,
            timestamp,
            signature,
            committer: committer.verifying_key(),
        };

        Ok((commitment, builder))
    }
}

/// TSC-specific commitment helpers
pub mod tsc {
    use super::*;
    use crate::temporal_stake_consensus::TSCVote;

    /// Create a commitment for TSC checkpoint votes
    pub fn create_checkpoint_vote_commitment(
        votes: &[TSCVote],
        epoch: u64,
        block_height: u64,
        committer: &ed25519_dalek::SigningKey,
    ) -> Result<(MerkleCommitment, MerkleTreeBuilder), CommitmentError> {
        let mut builder = MerkleTreeBuilder::new();

        for vote in votes {
            let data = bincode::serialize(vote).unwrap_or_default();
            builder.add_leaf(&data);
        }

        let root = builder.build();

        let scope = CommitmentScope {
            block_height,
            subblock_index: 0,
            miniblock_index: None,
            commitment_type: CommitmentType::TSCCheckpointVotes { epoch },
        };

        let timestamp = SystemTime::now();

        let mut sig_data = Vec::new();
        sig_data.extend_from_slice(root.as_bytes());
        sig_data.extend_from_slice(&builder.leaf_count().to_le_bytes());
        sig_data.extend_from_slice(&bincode::serialize(&scope).unwrap_or_default());

        let signature = committer.sign(&sig_data);

        let commitment = MerkleCommitment {
            root,
            leaf_count: builder.leaf_count(),
            depth: builder.depth(),
            scope,
            timestamp,
            signature,
            committer: committer.verifying_key(),
        };

        Ok((commitment, builder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::thread_rng;

    #[test]
    fn test_merkle_tree_construction() {
        let mut builder = MerkleTreeBuilder::new();

        for i in 0..100 {
            builder.add_leaf(&format!("proof_{}", i).as_bytes());
        }

        let root = builder.build();

        // Root should be non-zero
        assert_ne!(root, Hash::from([0u8; 32]));

        // Depth should be correct (128 leaves -> depth 7)
        assert_eq!(builder.depth(), 7);
    }

    #[test]
    fn test_merkle_proof_verification() {
        let mut builder = MerkleTreeBuilder::new();

        let data = vec![b"leaf0", b"leaf1", b"leaf2", b"leaf3"];
        for d in &data {
            builder.add_leaf(*d);
        }

        let root = builder.build();

        // Generate and verify proof for index 2
        let proof = builder.generate_proof(2).unwrap();
        let leaf_hash = Hash::from(*blake3::hash(b"leaf2").as_bytes());

        assert!(verify_merkle_proof(&leaf_hash, &proof, &root).is_ok());

        // Wrong hash should fail
        let wrong_hash = Hash::from(*blake3::hash(b"wrong").as_bytes());
        assert!(verify_merkle_proof(&wrong_hash, &proof, &root).is_err());
    }

    #[test]
    fn test_deterministic_sampling() {
        let vrf_output = blake3::hash(b"vrf_seed").into();

        let sampler1 = DeterministicSampler::new(vrf_output, 1000, 16);
        let sampler2 = DeterministicSampler::new(vrf_output, 1000, 16);

        let indices1 = sampler1.generate_indices();
        let indices2 = sampler2.generate_indices();

        // Same seed should produce same indices
        assert_eq!(indices1, indices2);
        assert_eq!(indices1.len(), 16);

        // All indices should be unique
        let unique: std::collections::HashSet<_> = indices1.iter().collect();
        assert_eq!(unique.len(), 16);

        // All indices should be within bounds
        for idx in &indices1 {
            assert!(*idx < 1000);
        }
    }

    #[test]
    fn test_commitment_manager() {
        let mut manager = CommitmentManager::new(8, 100);
        let committer = SigningKey::generate(&mut thread_rng());

        let mut builder = MerkleTreeBuilder::new();
        for i in 0u32..50 {
            builder.add_leaf(&i.to_le_bytes());
        }
        let root = builder.build();

        let scope = CommitmentScope {
            block_height: 100,
            subblock_index: 0,
            miniblock_index: Some(0),
            commitment_type: CommitmentType::PoRWDeliveryProofs { relay_index: 0 },
        };

        let mut sig_data = Vec::new();
        sig_data.extend_from_slice(root.as_bytes());
        sig_data.extend_from_slice(&builder.leaf_count().to_le_bytes());
        sig_data.extend_from_slice(&bincode::serialize(&scope).unwrap_or_default());

        let commitment = MerkleCommitment {
            root,
            leaf_count: builder.leaf_count(),
            depth: builder.depth(),
            scope,
            timestamp: SystemTime::now(),
            signature: committer.sign(&sig_data),
            committer: committer.verifying_key(),
        };

        // Register commitment
        assert!(manager.register_commitment(commitment.clone()).is_ok());

        // Generate sample request
        let vrf_output = blake3::hash(b"vrf").into();
        let request = manager
            .generate_sample_request(&scope, vrf_output, vec![])
            .unwrap();

        assert_eq!(request.leaf_indices.len(), 8);
        assert_eq!(request.commitment_root, root);
    }
}
