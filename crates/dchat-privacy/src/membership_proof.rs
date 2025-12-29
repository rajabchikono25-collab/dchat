//! Zero-Knowledge Membership Proofs for Channel Access Verification
//!
//! This module implements Groth16 ZK-SNARKs for privacy-preserving channel
//! membership verification. Relays can verify a user is a channel member
//! without learning which specific member they are.
//!
//! # Security Properties
//!
//! - **Privacy**: Verifier learns nothing about prover's identity
//! - **Soundness**: Only valid members can create valid proofs
//! - **Non-transferable**: Proofs bound to device + epoch (cannot be shared)
//! - **Replay-resistant**: Nullifier prevents reuse in same epoch
//!
//! # Circuit Design
//!
//! Public inputs:
//! - `merkle_root`: Root of channel's member Merkle tree
//! - `nullifier`: Poseidon(identity_secret, channel_id, epoch_id)
//! - `epoch_id`: Current epoch (prevents cross-epoch replay)
//! - `channel_id_hash`: Poseidon(channel_id)
//!
//! Private inputs (witness):
//! - `identity_secret`: User's secret key material
//! - `leaf_value`: Poseidon(identity_secret) - user's commitment in tree
//! - `merkle_path`: Sibling nodes for Merkle path
//! - `path_indices`: Left/right indicators for each level
//! - `channel_id`: Actual channel identifier
//!
//! # Usage
//!
//! ```ignore
//! // Client generates proof
//! let prover = MembershipProver::new(identity_secret, &keys);
//! let proof = prover.prove_membership(
//!     &channel_id,
//!     epoch_id,
//!     &merkle_path,
//!     &path_indices,
//!     merkle_root,
//! )?;
//!
//! // Relay verifies proof
//! let verifier = MembershipVerifier::new(&keys);
//! let valid = verifier.verify(&proof, merkle_root, epoch_id)?;
//! ```

use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};

use ark_bn254::{Bn254, Fr as Bn254Fr};
use ark_ff::PrimeField;
use ark_groth16::{prepare_verifying_key, Groth16, PreparedVerifyingKey, ProvingKey, VerifyingKey};
use ark_r1cs_std::{
    alloc::AllocVar, boolean::Boolean, eq::EqGadget, fields::fp::FpVar, select::CondSelectGadget,
};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use ark_std::UniformRand;
use rand::{CryptoRng, Rng};

use ark_crypto_primitives::sponge::{
    constraints::CryptographicSpongeVar, poseidon::constraints::PoseidonSpongeVar,
    poseidon::PoseidonConfig,
};

use crate::zk_proofs::{get_poseidon_config, poseidon_hash, poseidon_hash_2, ZkProof};

/// Maximum Merkle tree depth supported (2^20 = 1M members max)
pub const MAX_MERKLE_DEPTH: usize = 20;

/// Minimum Merkle tree depth (2^4 = 16 members min for privacy)
pub const MIN_MERKLE_DEPTH: usize = 4;

/// Helper to convert bytes to field element
fn bytes_to_fr(bytes: &[u8; 32]) -> Bn254Fr {
    Bn254Fr::from_le_bytes_mod_order(bytes)
}

/// Helper to convert field element to bytes
fn fr_to_bytes(fr: &Bn254Fr) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    let bigint = fr.into_bigint();
    let limbs = bigint.as_ref();
    for (i, limb) in limbs.iter().enumerate() {
        let limb_bytes = limb.to_le_bytes();
        let start = i * 8;
        let end = core::cmp::min(start + 8, 32);
        bytes[start..end].copy_from_slice(&limb_bytes[..end - start]);
    }
    bytes
}

/// Zero-knowledge proof of channel membership
///
/// This proof demonstrates that the prover is a member of a channel's
/// Merkle tree without revealing which member they are.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkMembershipProof {
    /// Groth16 ZK proof
    pub proof: ZkProof,

    /// Merkle root of the channel's member tree
    pub merkle_root: [u8; 32],

    /// Nullifier = Poseidon(identity_secret, channel_id, epoch_id)
    /// Prevents replay within same epoch
    pub nullifier: [u8; 32],

    /// Epoch this proof is valid for
    pub epoch_id: u64,

    /// Hash of channel ID (public binding)
    pub channel_id_hash: [u8; 32],

    /// Tree depth used (for verification)
    pub tree_depth: u8,
}

impl ZkMembershipProof {
    /// Check if this proof is for the expected channel and epoch
    pub fn matches_context(&self, channel_id: &[u8; 32], epoch_id: u64) -> bool {
        let expected_hash = poseidon_hash(&[bytes_to_fr(channel_id)]);
        let actual_hash = bytes_to_fr(&self.channel_id_hash);
        self.epoch_id == epoch_id && expected_hash == actual_hash
    }
}

/// Groth16 circuit for proving Merkle tree membership
///
/// This circuit enforces:
/// 1. leaf = Poseidon(identity_secret)
/// 2. Merkle path from leaf to root is valid
/// 3. nullifier = Poseidon(identity_secret, channel_id, epoch_id)
/// 4. channel_id_hash = Poseidon(channel_id)
#[derive(Clone)]
pub struct MembershipCircuit {
    /// Poseidon configuration
    pub poseidon_config: PoseidonConfig<Bn254Fr>,

    /// Tree depth (constant for this circuit instance)
    pub tree_depth: usize,

    // === Private inputs (witness) ===
    /// User's identity secret
    pub identity_secret: Option<Bn254Fr>,

    /// Channel identifier
    pub channel_id: Option<Bn254Fr>,

    /// Merkle path sibling nodes (from leaf to root)
    pub merkle_path: Option<Vec<Bn254Fr>>,

    /// Path indices (0 = left, 1 = right)
    pub path_indices: Option<Vec<bool>>,

    // === Public inputs ===
    /// Merkle root
    pub merkle_root: Option<Bn254Fr>,

    /// Nullifier
    pub nullifier: Option<Bn254Fr>,

    /// Epoch ID
    pub epoch_id: Option<Bn254Fr>,

    /// Hash of channel_id
    pub channel_id_hash: Option<Bn254Fr>,
}

impl MembershipCircuit {
    /// Create empty circuit for setup (no witness values)
    pub fn empty(tree_depth: usize) -> Self {
        assert!(
            tree_depth >= MIN_MERKLE_DEPTH && tree_depth <= MAX_MERKLE_DEPTH,
            "Tree depth must be between {} and {}",
            MIN_MERKLE_DEPTH,
            MAX_MERKLE_DEPTH
        );

        Self {
            poseidon_config: get_poseidon_config(),
            tree_depth,
            identity_secret: None,
            channel_id: None,
            merkle_path: None,
            path_indices: None,
            merkle_root: None,
            nullifier: None,
            epoch_id: None,
            channel_id_hash: None,
        }
    }

    /// Create circuit with witness values for proof generation
    #[allow(clippy::too_many_arguments)]
    pub fn with_witness(
        tree_depth: usize,
        identity_secret: Bn254Fr,
        channel_id: Bn254Fr,
        merkle_path: Vec<Bn254Fr>,
        path_indices: Vec<bool>,
        merkle_root: Bn254Fr,
        epoch_id: u64,
    ) -> Result<Self> {
        if tree_depth < MIN_MERKLE_DEPTH || tree_depth > MAX_MERKLE_DEPTH {
            return Err(Error::validation(format!(
                "Tree depth {} out of range [{}, {}]",
                tree_depth, MIN_MERKLE_DEPTH, MAX_MERKLE_DEPTH
            )));
        }

        if merkle_path.len() != tree_depth {
            return Err(Error::validation(format!(
                "Merkle path length {} doesn't match tree depth {}",
                merkle_path.len(),
                tree_depth
            )));
        }

        if path_indices.len() != tree_depth {
            return Err(Error::validation(format!(
                "Path indices length {} doesn't match tree depth {}",
                path_indices.len(),
                tree_depth
            )));
        }

        // Compute derived values
        let epoch_id_fr = Bn254Fr::from(epoch_id);
        let channel_id_hash = poseidon_hash(&[channel_id]);
        let nullifier = poseidon_hash(&[identity_secret, channel_id, epoch_id_fr]);

        Ok(Self {
            poseidon_config: get_poseidon_config(),
            tree_depth,
            identity_secret: Some(identity_secret),
            channel_id: Some(channel_id),
            merkle_path: Some(merkle_path),
            path_indices: Some(path_indices),
            merkle_root: Some(merkle_root),
            nullifier: Some(nullifier),
            epoch_id: Some(epoch_id_fr),
            channel_id_hash: Some(channel_id_hash),
        })
    }
}

impl ConstraintSynthesizer<Bn254Fr> for MembershipCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Bn254Fr>,
    ) -> core::result::Result<(), SynthesisError> {
        // === Allocate private inputs ===
        let identity_secret = FpVar::new_witness(cs.clone(), || {
            self.identity_secret
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let channel_id = FpVar::new_witness(cs.clone(), || {
            self.channel_id.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate Merkle path siblings
        let merkle_path: Vec<FpVar<Bn254Fr>> = (0..self.tree_depth)
            .map(|i| {
                FpVar::new_witness(cs.clone(), || {
                    self.merkle_path
                        .as_ref()
                        .and_then(|p| p.get(i).copied())
                        .ok_or(SynthesisError::AssignmentMissing)
                })
            })
            .collect::<core::result::Result<Vec<_>, _>>()?;

        // Allocate path indices as booleans
        let path_indices: Vec<Boolean<Bn254Fr>> = (0..self.tree_depth)
            .map(|i| {
                Boolean::new_witness(cs.clone(), || {
                    self.path_indices
                        .as_ref()
                        .and_then(|p| p.get(i).copied())
                        .ok_or(SynthesisError::AssignmentMissing)
                })
            })
            .collect::<core::result::Result<Vec<_>, _>>()?;

        // === Allocate public inputs ===
        let merkle_root_pub = FpVar::new_input(cs.clone(), || {
            self.merkle_root.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let nullifier_pub = FpVar::new_input(cs.clone(), || {
            self.nullifier.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let epoch_id_pub = FpVar::new_input(cs.clone(), || {
            self.epoch_id.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let channel_id_hash_pub = FpVar::new_input(cs.clone(), || {
            self.channel_id_hash
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // === Constraint 1: leaf = Poseidon(identity_secret) ===
        let mut leaf_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);
        leaf_sponge.absorb(&identity_secret)?;
        let leaf = leaf_sponge.squeeze_field_elements(1)?[0].clone();

        // === Constraint 2: Verify Merkle path from leaf to root ===
        // At each level: node = Poseidon(left, right)
        // path_index[i] = 0 means current is left child, sibling is right
        // path_index[i] = 1 means current is right child, sibling is left
        let mut current = leaf;

        for i in 0..self.tree_depth {
            let sibling = &merkle_path[i];
            let is_right = &path_indices[i];

            // Select left and right based on path index
            let left = FpVar::conditionally_select(is_right, sibling, &current)?;
            let right = FpVar::conditionally_select(is_right, &current, sibling)?;

            // Hash to get parent
            let mut parent_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);
            parent_sponge.absorb(&left)?;
            parent_sponge.absorb(&right)?;
            current = parent_sponge.squeeze_field_elements(1)?[0].clone();
        }

        // Enforce computed root equals public root
        current.enforce_equal(&merkle_root_pub)?;

        // === Constraint 3: nullifier = Poseidon(identity_secret, channel_id, epoch_id) ===
        let mut nullifier_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);
        nullifier_sponge.absorb(&identity_secret)?;
        nullifier_sponge.absorb(&channel_id)?;
        nullifier_sponge.absorb(&epoch_id_pub)?;
        let computed_nullifier = nullifier_sponge.squeeze_field_elements(1)?[0].clone();
        computed_nullifier.enforce_equal(&nullifier_pub)?;

        // === Constraint 4: channel_id_hash = Poseidon(channel_id) ===
        let mut channel_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);
        channel_sponge.absorb(&channel_id)?;
        let computed_channel_hash = channel_sponge.squeeze_field_elements(1)?[0].clone();
        computed_channel_hash.enforce_equal(&channel_id_hash_pub)?;

        Ok(())
    }
}

/// Proving and verifying keys for membership proofs
pub struct MembershipKeys {
    /// Poseidon configuration
    pub poseidon_config: PoseidonConfig<Bn254Fr>,

    /// Tree depth these keys are for
    pub tree_depth: usize,

    /// Proving key
    pub pk: ProvingKey<Bn254>,

    /// Verifying key
    pub vk: VerifyingKey<Bn254>,

    /// Prepared verifying key (faster verification)
    pub pvk: PreparedVerifyingKey<Bn254>,
}

impl MembershipKeys {
    /// Generate new proving and verifying keys (TRUSTED SETUP)
    ///
    /// # Security Note
    ///
    /// In production, keys MUST be generated through an MPC ceremony.
    /// See: https://eprint.iacr.org/2017/1050
    pub fn setup<R: Rng + CryptoRng>(tree_depth: usize, rng: &mut R) -> Result<Self> {
        if tree_depth < MIN_MERKLE_DEPTH || tree_depth > MAX_MERKLE_DEPTH {
            return Err(Error::validation(format!(
                "Tree depth {} out of range [{}, {}]",
                tree_depth, MIN_MERKLE_DEPTH, MAX_MERKLE_DEPTH
            )));
        }

        let circuit = MembershipCircuit::empty(tree_depth);
        let poseidon_config = circuit.poseidon_config.clone();

        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(circuit, rng)
            .map_err(|e| Error::validation(format!("Membership circuit setup failed: {:?}", e)))?;

        let pvk = prepare_verifying_key(&vk);

        Ok(Self {
            poseidon_config,
            tree_depth,
            pk,
            vk,
            pvk,
        })
    }

    /// Serialize keys for storage
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();

        // Write tree depth
        bytes.extend_from_slice(&(self.tree_depth as u32).to_le_bytes());

        // Serialize proving key
        self.pk
            .serialize_compressed(&mut bytes)
            .map_err(|e| Error::crypto(format!("Failed to serialize proving key: {}", e)))?;

        // Serialize verifying key
        self.vk
            .serialize_compressed(&mut bytes)
            .map_err(|e| Error::crypto(format!("Failed to serialize verifying key: {}", e)))?;

        Ok(bytes)
    }

    /// Deserialize keys from bytes
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 {
            return Err(Error::crypto("Key data too short"));
        }

        // Read tree depth
        let tree_depth = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;

        if tree_depth < MIN_MERKLE_DEPTH || tree_depth > MAX_MERKLE_DEPTH {
            return Err(Error::crypto(format!(
                "Invalid tree depth {} in serialized keys",
                tree_depth
            )));
        }

        // Deserialize proving key
        let mut cursor = &bytes[4..];
        let pk = ProvingKey::deserialize_compressed(&mut cursor)
            .map_err(|e| Error::crypto(format!("Failed to deserialize proving key: {}", e)))?;

        // Deserialize verifying key
        let vk = VerifyingKey::deserialize_compressed(&mut cursor)
            .map_err(|e| Error::crypto(format!("Failed to deserialize verifying key: {}", e)))?;

        let pvk = prepare_verifying_key(&vk);
        let poseidon_config = get_poseidon_config();

        Ok(Self {
            poseidon_config,
            tree_depth,
            pk,
            vk,
            pvk,
        })
    }
}

/// Prover for channel membership ZK proofs
pub struct MembershipProver<'a> {
    /// User's identity secret
    identity_secret: Bn254Fr,

    /// Reference to proving keys
    keys: &'a MembershipKeys,
}

impl<'a> MembershipProver<'a> {
    /// Create prover from identity secret bytes
    pub fn new(identity_secret: &[u8; 32], keys: &'a MembershipKeys) -> Self {
        Self {
            identity_secret: bytes_to_fr(identity_secret),
            keys,
        }
    }

    /// Create prover with random identity secret (for testing)
    pub fn random<R: Rng + CryptoRng>(rng: &mut R, keys: &'a MembershipKeys) -> Self {
        Self {
            identity_secret: Bn254Fr::rand(rng),
            keys,
        }
    }

    /// Get identity secret as bytes
    pub fn identity_secret_bytes(&self) -> [u8; 32] {
        fr_to_bytes(&self.identity_secret)
    }

    /// Compute the leaf value (identity commitment) for this prover
    ///
    /// This value should be added to the channel's member Merkle tree
    pub fn compute_leaf(&self) -> [u8; 32] {
        let leaf = poseidon_hash(&[self.identity_secret]);
        fr_to_bytes(&leaf)
    }

    /// Generate a membership proof
    ///
    /// # Arguments
    ///
    /// * `channel_id` - Channel identifier
    /// * `epoch_id` - Current epoch (for nullifier binding)
    /// * `merkle_path` - Sibling nodes from leaf to root
    /// * `path_indices` - Left(false)/Right(true) indicators
    /// * `merkle_root` - Expected root of the member tree
    pub fn prove_membership<R: Rng + CryptoRng>(
        &self,
        channel_id: &[u8; 32],
        epoch_id: u64,
        merkle_path: &[[u8; 32]],
        path_indices: &[bool],
        merkle_root: &[u8; 32],
        rng: &mut R,
    ) -> Result<ZkMembershipProof> {
        // Validate input lengths
        if merkle_path.len() != self.keys.tree_depth {
            return Err(Error::validation(format!(
                "Merkle path length {} doesn't match key tree depth {}",
                merkle_path.len(),
                self.keys.tree_depth
            )));
        }

        if path_indices.len() != self.keys.tree_depth {
            return Err(Error::validation(format!(
                "Path indices length {} doesn't match key tree depth {}",
                path_indices.len(),
                self.keys.tree_depth
            )));
        }

        // Convert to field elements
        let channel_id_fr = bytes_to_fr(channel_id);
        let merkle_root_fr = bytes_to_fr(merkle_root);
        let merkle_path_fr: Vec<Bn254Fr> = merkle_path.iter().map(bytes_to_fr).collect();

        // Create circuit with witness
        let circuit = MembershipCircuit::with_witness(
            self.keys.tree_depth,
            self.identity_secret,
            channel_id_fr,
            merkle_path_fr,
            path_indices.to_vec(),
            merkle_root_fr,
            epoch_id,
        )?;

        // Generate Groth16 proof
        let proof = Groth16::<Bn254>::prove(&self.keys.pk, circuit, rng)
            .map_err(|e| Error::crypto(format!("Failed to generate membership proof: {:?}", e)))?;

        // Compute derived values for proof structure
        let epoch_id_fr = Bn254Fr::from(epoch_id);
        let nullifier_fr = poseidon_hash(&[self.identity_secret, channel_id_fr, epoch_id_fr]);
        let channel_id_hash_fr = poseidon_hash(&[channel_id_fr]);

        Ok(ZkMembershipProof {
            proof: ZkProof::from_groth16(&proof)?,
            merkle_root: *merkle_root,
            nullifier: fr_to_bytes(&nullifier_fr),
            epoch_id,
            channel_id_hash: fr_to_bytes(&channel_id_hash_fr),
            tree_depth: self.keys.tree_depth as u8,
        })
    }
}

/// Verifier for channel membership ZK proofs
pub struct MembershipVerifier<'a> {
    /// Reference to verifying keys
    keys: &'a MembershipKeys,
}

impl<'a> MembershipVerifier<'a> {
    /// Create verifier with keys
    pub fn new(keys: &'a MembershipKeys) -> Self {
        Self { keys }
    }

    /// Verify a membership proof
    ///
    /// # Arguments
    ///
    /// * `proof` - The ZK membership proof to verify
    /// * `expected_merkle_root` - The current root of the channel's member tree
    /// * `expected_epoch_id` - The epoch this proof should be valid for
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Proof is valid
    /// * `Ok(false)` - Proof is invalid (failed verification)
    /// * `Err(_)` - Verification error (malformed proof, etc.)
    pub fn verify(
        &self,
        proof: &ZkMembershipProof,
        expected_merkle_root: &[u8; 32],
        expected_epoch_id: u64,
    ) -> Result<bool> {
        // Check tree depth matches
        if proof.tree_depth as usize != self.keys.tree_depth {
            return Err(Error::validation(format!(
                "Proof tree depth {} doesn't match verifier tree depth {}",
                proof.tree_depth, self.keys.tree_depth
            )));
        }

        // Check epoch matches
        if proof.epoch_id != expected_epoch_id {
            return Ok(false);
        }

        // Check merkle root matches
        if proof.merkle_root != *expected_merkle_root {
            return Ok(false);
        }

        // Prepare public inputs: [merkle_root, nullifier, epoch_id, channel_id_hash]
        let merkle_root_fr = bytes_to_fr(expected_merkle_root);
        let nullifier_fr = bytes_to_fr(&proof.nullifier);
        let epoch_id_fr = Bn254Fr::from(expected_epoch_id);
        let channel_id_hash_fr = bytes_to_fr(&proof.channel_id_hash);

        let public_inputs = vec![
            merkle_root_fr,
            nullifier_fr,
            epoch_id_fr,
            channel_id_hash_fr,
        ];

        // Deserialize and verify Groth16 proof
        let groth16_proof = proof.proof.to_groth16()?;

        let valid = Groth16::<Bn254>::verify_with_processed_vk(
            &self.keys.pvk,
            &public_inputs,
            &groth16_proof,
        )
        .map_err(|e| Error::crypto(format!("Groth16 verification failed: {:?}", e)))?;

        Ok(valid)
    }

    /// Verify and check nullifier hasn't been used
    ///
    /// This is the production verification flow that also checks the
    /// nullifier against a spent set to prevent replay attacks.
    pub fn verify_with_nullifier_check(
        &self,
        proof: &ZkMembershipProof,
        expected_merkle_root: &[u8; 32],
        expected_epoch_id: u64,
        nullifier_set: &mut NullifierSet,
    ) -> Result<bool> {
        // First check if nullifier was already used
        if nullifier_set.has_seen(&proof.nullifier) {
            return Err(Error::validation(
                "Membership proof nullifier already used (replay attack detected)",
            ));
        }

        // Verify the proof
        let valid = self.verify(proof, expected_merkle_root, expected_epoch_id)?;

        // If valid, mark nullifier as used
        if valid {
            nullifier_set.mark_seen(proof.nullifier)?;
        }

        Ok(valid)
    }
}

/// Nullifier set for tracking used membership proofs per epoch
pub struct NullifierSet {
    /// Seen nullifiers
    seen: std::collections::HashSet<[u8; 32]>,
    /// Epoch this set is for (cleared on epoch change)
    epoch_id: u64,
}

impl NullifierSet {
    /// Create new nullifier set for an epoch
    pub fn new(epoch_id: u64) -> Self {
        Self {
            seen: std::collections::HashSet::new(),
            epoch_id,
        }
    }

    /// Check if this is still the current epoch, reset if not
    pub fn check_epoch(&mut self, current_epoch: u64) {
        if self.epoch_id != current_epoch {
            self.seen.clear();
            self.epoch_id = current_epoch;
        }
    }

    /// Check if nullifier has been seen
    pub fn has_seen(&self, nullifier: &[u8; 32]) -> bool {
        self.seen.contains(nullifier)
    }

    /// Mark nullifier as seen
    pub fn mark_seen(&mut self, nullifier: [u8; 32]) -> Result<()> {
        if self.has_seen(&nullifier) {
            return Err(Error::validation("Nullifier already used"));
        }
        self.seen.insert(nullifier);
        Ok(())
    }

    /// Get count of seen nullifiers
    pub fn count(&self) -> usize {
        self.seen.len()
    }
}

/// Helper: Build a Merkle tree from leaf values
///
/// Returns (root, all_nodes) where all_nodes[depth][index] gives the node value
pub fn build_merkle_tree(leaves: &[[u8; 32]]) -> ([u8; 32], Vec<Vec<[u8; 32]>>) {
    if leaves.is_empty() {
        return ([0u8; 32], vec![]);
    }

    // Pad leaves to next power of 2
    let mut padded = leaves.to_vec();
    let target_size = padded.len().next_power_of_two();
    while padded.len() < target_size {
        padded.push([0u8; 32]); // Zero leaves as padding
    }

    let mut tree: Vec<Vec<[u8; 32]>> = vec![padded.clone()];
    let mut current_level = padded;

    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity(current_level.len() / 2);

        for chunk in current_level.chunks(2) {
            let left = bytes_to_fr(&chunk[0]);
            let right = bytes_to_fr(&chunk[1]);
            let parent = poseidon_hash_2(&left, &right);
            next_level.push(fr_to_bytes(&parent));
        }

        tree.push(next_level.clone());
        current_level = next_level;
    }

    (current_level[0], tree)
}

/// Helper: Get Merkle path for a leaf at given index
///
/// Returns (path, indices) where path[i] is sibling at level i
/// and indices[i] is true if the target is the right child
pub fn get_merkle_path(
    tree: &[Vec<[u8; 32]>],
    leaf_index: usize,
) -> Result<(Vec<[u8; 32]>, Vec<bool>)> {
    if tree.is_empty() {
        return Err(Error::validation("Empty tree"));
    }

    let mut path = Vec::new();
    let mut indices = Vec::new();
    let mut idx = leaf_index;

    for level in tree.iter().take(tree.len() - 1) {
        let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };

        if sibling_idx >= level.len() {
            return Err(Error::validation("Leaf index out of bounds"));
        }

        path.push(level[sibling_idx]);
        indices.push(idx % 2 != 0); // true if we're the right child

        idx /= 2;
    }

    Ok((path, indices))
}

/// Compute Merkle root from a leaf and its path
pub fn compute_merkle_root(
    leaf: &[u8; 32],
    path: &[[u8; 32]],
    indices: &[bool],
) -> Result<[u8; 32]> {
    if path.len() != indices.len() {
        return Err(Error::validation("Path and indices must have same length"));
    }

    let mut current = bytes_to_fr(leaf);

    for (sibling, is_right) in path.iter().zip(indices.iter()) {
        let sibling_fr = bytes_to_fr(sibling);

        let (left, right) = if *is_right {
            (sibling_fr, current)
        } else {
            (current, sibling_fr)
        };

        current = poseidon_hash_2(&left, &right);
    }

    Ok(fr_to_bytes(&current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    fn setup_keys(depth: usize) -> MembershipKeys {
        let mut rng = OsRng;
        MembershipKeys::setup(depth, &mut rng).expect("Key setup should succeed")
    }

    #[test]
    fn test_merkle_tree_build_and_path() {
        // Create 8 leaves
        let leaves: Vec<[u8; 32]> = (0..8u8)
            .map(|i| {
                let mut leaf = [0u8; 32];
                leaf[0] = i;
                leaf
            })
            .collect();

        let (root, tree) = build_merkle_tree(&leaves);

        // Tree should have 4 levels: 8 -> 4 -> 2 -> 1
        assert_eq!(tree.len(), 4);
        assert_eq!(tree[0].len(), 8);
        assert_eq!(tree[1].len(), 4);
        assert_eq!(tree[2].len(), 2);
        assert_eq!(tree[3].len(), 1);
        assert_eq!(tree[3][0], root);

        // Verify path for leaf 3
        let (path, indices) = get_merkle_path(&tree, 3).unwrap();
        assert_eq!(path.len(), 3); // depth = log2(8) = 3

        // Recompute root from path
        let computed_root = compute_merkle_root(&leaves[3], &path, &indices).unwrap();
        assert_eq!(computed_root, root);
    }

    #[test]
    fn test_membership_proof_generation_and_verification() {
        let mut rng = OsRng;
        let depth = 4; // 16 leaves max
        let keys = setup_keys(depth);

        // Create prover with random secret
        let prover = MembershipProver::random(&mut rng, &keys);
        let leaf = prover.compute_leaf();

        // Build tree with this leaf and some others
        let mut leaves = vec![[0u8; 32]; 16];
        leaves[5] = leaf; // Put our leaf at index 5

        let (root, tree) = build_merkle_tree(&leaves);
        let (path, indices) = get_merkle_path(&tree, 5).unwrap();

        // Generate proof
        let channel_id = [42u8; 32];
        let epoch_id = 12345u64;

        let path_refs: Vec<[u8; 32]> = path.clone();
        let proof = prover
            .prove_membership(&channel_id, epoch_id, &path_refs, &indices, &root, &mut rng)
            .expect("Proof generation should succeed");

        // Verify proof
        let verifier = MembershipVerifier::new(&keys);
        let valid = verifier
            .verify(&proof, &root, epoch_id)
            .expect("Verification should succeed");

        assert!(valid, "Valid membership proof should verify");
    }

    #[test]
    fn test_membership_proof_wrong_root_fails() {
        let mut rng = OsRng;
        let depth = 4;
        let keys = setup_keys(depth);

        let prover = MembershipProver::random(&mut rng, &keys);
        let leaf = prover.compute_leaf();

        let mut leaves = vec![[0u8; 32]; 16];
        leaves[5] = leaf;

        let (root, tree) = build_merkle_tree(&leaves);
        let (path, indices) = get_merkle_path(&tree, 5).unwrap();

        let channel_id = [42u8; 32];
        let epoch_id = 12345u64;

        let proof = prover
            .prove_membership(&channel_id, epoch_id, &path, &indices, &root, &mut rng)
            .unwrap();

        // Try to verify with wrong root
        let wrong_root = [99u8; 32];
        let verifier = MembershipVerifier::new(&keys);
        let valid = verifier.verify(&proof, &wrong_root, epoch_id).unwrap();

        assert!(!valid, "Proof with wrong root should fail");
    }

    #[test]
    fn test_membership_proof_wrong_epoch_fails() {
        let mut rng = OsRng;
        let depth = 4;
        let keys = setup_keys(depth);

        let prover = MembershipProver::random(&mut rng, &keys);
        let leaf = prover.compute_leaf();

        let mut leaves = vec![[0u8; 32]; 16];
        leaves[5] = leaf;

        let (root, tree) = build_merkle_tree(&leaves);
        let (path, indices) = get_merkle_path(&tree, 5).unwrap();

        let channel_id = [42u8; 32];
        let epoch_id = 12345u64;

        let proof = prover
            .prove_membership(&channel_id, epoch_id, &path, &indices, &root, &mut rng)
            .unwrap();

        // Try to verify with wrong epoch
        let verifier = MembershipVerifier::new(&keys);
        let valid = verifier.verify(&proof, &root, epoch_id + 1).unwrap();

        assert!(!valid, "Proof with wrong epoch should fail");
    }

    #[test]
    fn test_membership_proof_non_member_fails() {
        let mut rng = OsRng;
        let depth = 4;
        let keys = setup_keys(depth);

        // Create prover with random secret
        let prover = MembershipProver::random(&mut rng, &keys);
        let _leaf = prover.compute_leaf(); // NOT added to tree

        // Build tree without our leaf
        let leaves = vec![[0u8; 32]; 16];
        let (root, tree) = build_merkle_tree(&leaves);

        // Try to use path for index 5 (which has a zero leaf, not our leaf)
        let (path, indices) = get_merkle_path(&tree, 5).unwrap();

        let channel_id = [42u8; 32];
        let epoch_id = 12345u64;

        // The circuit will compute: leaf' = Poseidon(identity_secret)
        // Then walk up the Merkle tree using the provided path.
        // Since our leaf' != tree[5] (which is [0u8; 32]), the computed
        // root will be different from the expected root.
        //
        // arkworks Groth16 panics on unsatisfied constraints in debug mode.
        // In release mode, proof generation may succeed but verification fails.
        // We use catch_unwind to handle the debug-mode panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prover.prove_membership(&channel_id, epoch_id, &path, &indices, &root, &mut rng)
        }));

        match result {
            // Panic in debug mode = circuit constraints not satisfied
            Err(_) => {
                // Expected: arkworks panics when constraints aren't satisfied
            }
            // No panic = check if proof was generated (shouldn't happen with wrong witness)
            Ok(proof_result) => {
                match proof_result {
                    // Proof generation error (also acceptable)
                    Err(_) => {}
                    // If proof somehow generated, it should fail verification
                    Ok(p) => {
                        let verifier = MembershipVerifier::new(&keys);
                        let valid = verifier.verify(&p, &root, epoch_id).unwrap();
                        assert!(!valid, "Forged proof should not verify");
                    }
                }
            }
        }
    }

    #[test]
    fn test_nullifier_prevents_replay() {
        let mut rng = OsRng;
        let depth = 4;
        let keys = setup_keys(depth);

        let prover = MembershipProver::random(&mut rng, &keys);
        let leaf = prover.compute_leaf();

        let mut leaves = vec![[0u8; 32]; 16];
        leaves[5] = leaf;

        let (root, tree) = build_merkle_tree(&leaves);
        let (path, indices) = get_merkle_path(&tree, 5).unwrap();

        let channel_id = [42u8; 32];
        let epoch_id = 12345u64;

        let proof = prover
            .prove_membership(&channel_id, epoch_id, &path, &indices, &root, &mut rng)
            .unwrap();

        let verifier = MembershipVerifier::new(&keys);
        let mut nullifier_set = NullifierSet::new(epoch_id);

        // First verification should succeed
        let valid1 = verifier
            .verify_with_nullifier_check(&proof, &root, epoch_id, &mut nullifier_set)
            .unwrap();
        assert!(valid1, "First verification should succeed");

        // Second verification should fail (replay)
        let result =
            verifier.verify_with_nullifier_check(&proof, &root, epoch_id, &mut nullifier_set);
        assert!(result.is_err(), "Replay should be rejected");
    }

    #[test]
    fn test_nullifier_set_epoch_reset() {
        let mut nullifier_set = NullifierSet::new(100);
        let nullifier = [42u8; 32];

        nullifier_set.mark_seen(nullifier).unwrap();
        assert!(nullifier_set.has_seen(&nullifier));

        // Epoch change should clear the set
        nullifier_set.check_epoch(101);
        assert!(!nullifier_set.has_seen(&nullifier));
    }

    #[test]
    fn test_proof_context_matching() {
        let mut rng = OsRng;
        let depth = 4;
        let keys = setup_keys(depth);

        let prover = MembershipProver::random(&mut rng, &keys);
        let leaf = prover.compute_leaf();

        let mut leaves = vec![[0u8; 32]; 16];
        leaves[5] = leaf;

        let (root, tree) = build_merkle_tree(&leaves);
        let (path, indices) = get_merkle_path(&tree, 5).unwrap();

        let channel_id = [42u8; 32];
        let epoch_id = 12345u64;

        let proof = prover
            .prove_membership(&channel_id, epoch_id, &path, &indices, &root, &mut rng)
            .unwrap();

        // Should match correct context
        assert!(proof.matches_context(&channel_id, epoch_id));

        // Should not match wrong channel
        let wrong_channel = [99u8; 32];
        assert!(!proof.matches_context(&wrong_channel, epoch_id));

        // Should not match wrong epoch
        assert!(!proof.matches_context(&channel_id, epoch_id + 1));
    }

    #[test]
    fn test_key_serialization_roundtrip() {
        let mut rng = OsRng;
        let depth = 4;
        let keys = setup_keys(depth);

        let serialized = keys.serialize().expect("Serialization should succeed");
        let deserialized =
            MembershipKeys::deserialize(&serialized).expect("Deserialization should succeed");

        assert_eq!(deserialized.tree_depth, depth);

        // Verify keys work after deserialization
        let prover = MembershipProver::random(&mut rng, &deserialized);
        let leaf = prover.compute_leaf();

        let mut leaves = vec![[0u8; 32]; 16];
        leaves[0] = leaf;

        let (root, tree) = build_merkle_tree(&leaves);
        let (path, indices) = get_merkle_path(&tree, 0).unwrap();

        let proof = prover
            .prove_membership(&[1u8; 32], 1, &path, &indices, &root, &mut rng)
            .expect("Proof with deserialized keys should work");

        let verifier = MembershipVerifier::new(&deserialized);
        let valid = verifier.verify(&proof, &root, 1).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_different_tree_depths() {
        let mut rng = OsRng;

        for depth in [MIN_MERKLE_DEPTH, 8, 12, MAX_MERKLE_DEPTH] {
            let keys = setup_keys(depth);
            let prover = MembershipProver::random(&mut rng, &keys);
            let leaf = prover.compute_leaf();

            let num_leaves = 1 << depth;
            let mut leaves = vec![[0u8; 32]; num_leaves];
            leaves[0] = leaf;

            let (root, tree) = build_merkle_tree(&leaves);
            let (path, indices) = get_merkle_path(&tree, 0).unwrap();

            assert_eq!(path.len(), depth, "Path length should match tree depth");

            let proof = prover
                .prove_membership(&[1u8; 32], 1, &path, &indices, &root, &mut rng)
                .expect(&format!("Proof with depth {} should work", depth));

            assert_eq!(
                proof.tree_depth, depth as u8,
                "Proof tree depth should match"
            );

            let verifier = MembershipVerifier::new(&keys);
            let valid = verifier.verify(&proof, &root, 1).unwrap();
            assert!(valid, "Proof with depth {} should verify", depth);
        }
    }
}
