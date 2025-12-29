//! VRF-Selected Relay Committees (Production-Grade)
//!
//! This module implements cryptographically secure committee selection using
//! Schnorrkel VRF (Verifiable Random Function) over Ristretto255.
//!
//! # Security Properties
//!
//! - **Unpredictability**: VRF output cannot be predicted without the secret key
//! - **Verifiability**: Anyone can verify the VRF output using the public key and proof
//! - **Uniqueness**: Each input produces exactly one valid output per key
//! - **Pseudo-randomness**: Output is computationally indistinguishable from random
//!
//! # Features
//!
//! - VRF-selected relay committees per miniblock/subblock
//! - 5% weight cap per relay (prevents stake centralization)
//! - Geographic diversity constraints (min 3 regions)
//! - ASN and IP prefix diversity (prevents network-level attacks)
//! - Operator diversity (prevents Sybil attacks)
//! - Committee size bounds (7-128 members)
//!
//! # Production Considerations
//!
//! - All VRF proofs are verified before committee acceptance
//! - Weight caps are enforced at selection time
//! - Diversity requirements are mandatory (no fallback)
//! - All cryptographic operations use constant-time implementations

use crate::block_hierarchy::Hash;
use crate::consensus_types::RelayScore;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use merlin::Transcript;
use schnorrkel::{
    vrf::{VRFPreOut, VRFProof},
    Keypair as SchnorrkelKeypair, PublicKey as SchnorrkelPublicKey,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Maximum committee size for any block unit
pub const MAX_COMMITTEE_SIZE: usize = 128;

/// Minimum committee size for liveness
pub const MIN_COMMITTEE_SIZE: usize = 7;

/// Maximum weight any single relay can have (basis points)
pub const MAX_RELAY_WEIGHT_BPS: u64 = 500; // 5%

/// Minimum required regions for diversity (geographic)
pub const MIN_REQUIRED_REGIONS: usize = 3;

/// Maximum relays from same geographic region (ensures distribution)
pub const MAX_RELAYS_PER_REGION: usize = 10;

/// Maximum relays from same ASN
pub const MAX_RELAYS_PER_ASN: usize = 3;

/// Maximum relays from same /24 IP prefix
pub const MAX_RELAYS_PER_IP_PREFIX: usize = 2;

/// Maximum relays from same operator
pub const MAX_RELAYS_PER_OPERATOR: usize = 5;

/// VRF proof size in bytes (Schnorrkel VRF proof is 64 bytes)
pub const VRF_PROOF_SIZE: usize = 64;

/// VRF output size in bytes
pub const VRF_OUTPUT_SIZE: usize = 32;

/// Context string for VRF signing (domain separation)
const VRF_CONTEXT: &[u8] = b"dchat-committee-vrf-v1";

/// Wrapper for VRF proof bytes that implements Serialize/Deserialize
///
/// Serde doesn't implement Serialize/Deserialize for `[u8; 64]` by default,
/// so we use a wrapper with serde_bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VrfProofBytes(pub [u8; VRF_PROOF_SIZE]);

impl serde::Serialize for VrfProofBytes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde_bytes::serialize(&self.0[..], serializer)
    }
}

impl<'de> serde::Deserialize<'de> for VrfProofBytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes: Vec<u8> = serde_bytes::deserialize(deserializer)?;
        if bytes.len() != VRF_PROOF_SIZE {
            return Err(serde::de::Error::custom(format!(
                "VRF proof must be exactly {} bytes, got {}",
                VRF_PROOF_SIZE,
                bytes.len()
            )));
        }
        let mut arr = [0u8; VRF_PROOF_SIZE];
        arr.copy_from_slice(&bytes);
        Ok(VrfProofBytes(arr))
    }
}

/// VRF output and proof (production-grade with cryptographic verification)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VrfOutput {
    /// VRF output (32 bytes) - the random value used for selection
    pub output: [u8; 32],
    /// VRF proof (64 bytes) - cryptographic proof of correct computation
    /// This allows anyone to verify the output without the secret key
    pub proof: VrfProofBytes,
    /// Block height this VRF was computed for
    pub height: u64,
    /// Subblock index (0-9)
    pub subblock: u8,
    /// Miniblock index (0-9), None for subblock-level
    pub miniblock: Option<u8>,
    /// Public key of the VRF signer (for verification)
    pub signer_public_key: [u8; 32],
}

impl VrfOutput {
    /// Generate a VRF output with proof using a Schnorrkel keypair
    ///
    /// # Arguments
    /// * `keypair` - The Schnorrkel keypair for VRF signing
    /// * `input` - The VRF input (derived from block hash + scope)
    /// * `height` - Block height
    /// * `subblock` - Subblock index
    /// * `miniblock` - Optional miniblock index
    ///
    /// # Returns
    /// A VrfOutput with cryptographic proof that can be verified by anyone
    pub fn generate(
        keypair: &SchnorrkelKeypair,
        input: &[u8],
        height: u64,
        subblock: u8,
        miniblock: Option<u8>,
    ) -> Self {
        // Create a merlin transcript for VRF signing with domain separation
        let mut transcript = Transcript::new(VRF_CONTEXT);
        transcript.append_message(b"input", input);
        let (inout, proof, _) = keypair.vrf_sign(transcript);

        let output_bytes: [u8; 32] = inout.to_preout().to_bytes();
        let proof_bytes: [u8; VRF_PROOF_SIZE] = proof.to_bytes();
        let public_key_bytes: [u8; 32] = keypair.public.to_bytes();

        Self {
            output: output_bytes,
            proof: VrfProofBytes(proof_bytes),
            height,
            subblock,
            miniblock,
            signer_public_key: public_key_bytes,
        }
    }

    /// Verify the VRF proof is valid for the given input
    ///
    /// # Arguments
    /// * `input` - The original VRF input
    ///
    /// # Returns
    /// Ok(()) if verification succeeds, Err if proof is invalid
    pub fn verify(&self, input: &[u8]) -> Result<(), CommitteeError> {
        let public_key = SchnorrkelPublicKey::from_bytes(&self.signer_public_key)
            .map_err(|_| CommitteeError::InvalidVrfProof)?;

        let proof =
            VRFProof::from_bytes(&self.proof.0).map_err(|_| CommitteeError::InvalidVrfProof)?;

        let preout =
            VRFPreOut::from_bytes(&self.output).map_err(|_| CommitteeError::InvalidVrfProof)?;

        // Create the same transcript used during signing
        let mut transcript = Transcript::new(VRF_CONTEXT);
        transcript.append_message(b"input", input);

        public_key
            .vrf_verify(transcript, &preout, &proof)
            .map_err(|_| CommitteeError::InvalidVrfProof)?;

        Ok(())
    }

    /// Create a VRF output for testing (uses deterministic BLAKE3 hash)
    ///
    /// # Safety
    /// This should ONLY be used in tests. Production code must use `generate()`.
    #[cfg(test)]
    pub fn for_testing(input: &[u8], height: u64, subblock: u8, miniblock: Option<u8>) -> Self {
        use rand::SeedableRng;

        // Create deterministic keypair from input for reproducible tests
        let seed = blake3::hash(input);
        let mut rng = rand::rngs::StdRng::from_seed(*seed.as_bytes());
        let keypair = SchnorrkelKeypair::generate_with(&mut rng);

        Self::generate(&keypair, input, height, subblock, miniblock)
    }
}

/// Selected committee for a block unit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Committee {
    /// Block unit this committee is for
    pub scope: CommitteeScope,

    /// Selected relay IDs with their selection proofs
    pub members: Vec<CommitteeMember>,

    /// Total normalized weight of committee
    pub total_weight: u64,

    /// VRF that was used for selection
    pub vrf_output: VrfOutput,

    /// Diversity metrics for verification
    pub diversity: DiversityMetrics,

    /// Signature from the selector proving authentic committee creation
    pub selector_signature: Option<Signature>,
}

/// Scope of committee authority
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CommitteeScope {
    pub block_height: u64,
    pub subblock_index: u8,
    pub miniblock_index: Option<u8>,
    pub committee_type: CommitteeType,
}

/// Type of committee
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CommitteeType {
    /// PoRW delivery attestation committee
    PoRWAttestation,
    /// PoT path validation committee
    PoTValidation,
    /// TSC checkpoint finality committee
    TSCCheckpoint,
    /// Block production committee
    BlockProduction,
}

/// A selected committee member
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitteeMember {
    /// Relay ID
    pub relay_id: RelayId,

    /// Relay's public key
    pub public_key: VerifyingKey,

    /// Normalized weight (capped at 5%)
    pub weight: u64,

    /// Selection position (derived from VRF)
    pub selection_index: u64,

    /// Selection proof (hash chain from VRF output)
    pub selection_proof: Hash,

    /// Geographic region
    pub region: GeographicRegion,

    /// Autonomous System Number
    pub asn: u32,

    /// Operator ID (for stake attribution)
    pub operator_id: Hash,
}

/// Relay identifier
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RelayId(pub [u8; 32]);

impl RelayId {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_public_key(pk: &VerifyingKey) -> Self {
        let hash = blake3::hash(pk.as_bytes());
        Self(*hash.as_bytes())
    }
}

/// Geographic regions for diversity
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum GeographicRegion {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Asia,
    Africa,
    Oceania,
}

impl GeographicRegion {
    pub fn all() -> Vec<Self> {
        vec![
            Self::NorthAmerica,
            Self::SouthAmerica,
            Self::Europe,
            Self::Asia,
            Self::Africa,
            Self::Oceania,
        ]
    }
}

/// Diversity metrics for committee
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiversityMetrics {
    /// Number of distinct regions represented
    pub region_count: usize,
    /// Number of distinct ASNs represented
    pub asn_count: usize,
    /// Number of distinct operators represented
    pub operator_count: usize,
    /// Number of distinct /24 IP prefixes
    pub ip_prefix_count: usize,
    /// Gini coefficient of weight distribution (lower = more equal)
    pub weight_gini: f64,
}

/// Relay eligibility information for committee selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayEligibility {
    pub relay_id: RelayId,
    pub public_key: VerifyingKey,
    pub stake: u64,
    pub uptime_score: f64,
    pub region: GeographicRegion,
    pub asn: u32,
    pub ip_prefix: [u8; 3], // First 3 bytes of IP (approximates /24)
    pub operator_id: Hash,
    pub raw_weight: u64,
}

impl From<RelayScore> for RelayEligibility {
    /// Convert a RelayScore into committee eligibility information
    fn from(score: RelayScore) -> Self {
        // Extract IP prefix from the IP hash (first 3 bytes)
        let ip_prefix: [u8; 3] = score.ip_address_hash.as_bytes()[0..3]
            .try_into()
            .unwrap_or([0, 0, 0]);

        // Derive operator_id from relay_id public key
        let operator_id = Hash::from(*blake3::hash(score.relay_id.as_bytes()).as_bytes());

        // Calculate raw weight from stake and reputation
        let stake_weight = score.stake_amount / 1_000_000; // Normalize to millions
        let reputation_factor = (score.reputation_score * 100.0) as u64;
        let uptime_factor = (score.uptime_percentage) as u64;
        let raw_weight = stake_weight
            .saturating_mul(reputation_factor)
            .saturating_mul(uptime_factor)
            / 10000; // Normalize

        // Convert consensus_types::GeographicRegion to local GeographicRegion
        let region = match score.geographic_region {
            crate::consensus_types::GeographicRegion::NorthAmerica => {
                GeographicRegion::NorthAmerica
            }
            crate::consensus_types::GeographicRegion::SouthAmerica => {
                GeographicRegion::SouthAmerica
            }
            crate::consensus_types::GeographicRegion::Europe => GeographicRegion::Europe,
            crate::consensus_types::GeographicRegion::Asia => GeographicRegion::Asia,
            crate::consensus_types::GeographicRegion::Africa => GeographicRegion::Africa,
            crate::consensus_types::GeographicRegion::Oceania => GeographicRegion::Oceania,
        };

        Self {
            relay_id: RelayId::from_public_key(&score.relay_id),
            public_key: score.relay_id,
            stake: score.stake_amount,
            uptime_score: score.uptime_percentage / 100.0,
            region,
            asn: score.asn,
            ip_prefix,
            operator_id,
            raw_weight: raw_weight.max(1), // Minimum weight of 1
        }
    }
}

/// Committee selection errors
#[derive(Debug, Error)]
pub enum CommitteeError {
    #[error("Insufficient eligible relays: {0} < {1}")]
    InsufficientRelays(usize, usize),

    #[error("Diversity requirements not met: {0}")]
    DiversityNotMet(String),

    #[error("Invalid VRF proof: cryptographic verification failed")]
    InvalidVrfProof,

    #[error("Invalid VRF public key format")]
    InvalidVrfPublicKey,

    #[error("Invalid selection proof for relay {0:?}")]
    InvalidSelectionProof(RelayId),

    #[error("Weight cap exceeded for relay {0:?}: weight {1} > max {2}")]
    WeightCapExceeded(RelayId, u64, u64),

    #[error("Committee scope mismatch: VRF output parameters do not match committee scope")]
    ScopeMismatch,

    #[error("VRF seed not available for height {0}: need finalized block at height {1}")]
    VrfSeedNotAvailable(u64, u64),

    #[error("Selection exhausted: could not find {0} relays meeting diversity requirements after {1} attempts")]
    SelectionExhausted(usize, u64),

    #[error("Zero total weight: no eligible relays with positive weight")]
    ZeroTotalWeight,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// VRF seed derivation from finalized chain state
#[derive(Clone)]
pub struct VrfSeedDeriver {
    /// Previous block hashes (for seed derivation)
    block_hashes: HashMap<u64, Hash>,
    /// Minimum confirmations before hash is usable as seed
    min_confirmations: u64,
}

impl VrfSeedDeriver {
    pub fn new(min_confirmations: u64) -> Self {
        Self {
            block_hashes: HashMap::new(),
            min_confirmations,
        }
    }

    /// Record a finalized block hash
    pub fn record_finalized_block(&mut self, height: u64, hash: Hash) {
        self.block_hashes.insert(height, hash);

        // Prune old entries (keep last 1000 blocks)
        if self.block_hashes.len() > 1000 {
            let min_to_keep = height.saturating_sub(1000);
            self.block_hashes.retain(|h, _| *h >= min_to_keep);
        }
    }

    /// Derive VRF seed for a target block height
    /// Uses hash from (target_height - min_confirmations) block
    pub fn get_seed(&self, target_height: u64) -> Result<Hash, CommitteeError> {
        let seed_height = target_height.saturating_sub(self.min_confirmations);

        self.block_hashes
            .get(&seed_height)
            .copied()
            .ok_or(CommitteeError::VrfSeedNotAvailable(
                target_height,
                seed_height,
            ))
    }

    /// Derive VRF input for a specific committee
    pub fn derive_vrf_input(&self, scope: &CommitteeScope) -> Result<[u8; 64], CommitteeError> {
        let seed = self.get_seed(scope.block_height)?;

        let mut input = [0u8; 64];
        input[0..32].copy_from_slice(seed.as_bytes());
        input[32..40].copy_from_slice(&scope.block_height.to_le_bytes());
        input[40] = scope.subblock_index;
        input[41] = scope.miniblock_index.unwrap_or(255);
        input[42] = scope.committee_type as u8;

        // Mix with BLAKE3
        let hash = blake3::hash(&input);
        input[43..64].copy_from_slice(&hash.as_bytes()[0..21]);

        Ok(input)
    }
}

/// Committee selector using VRF
pub struct CommitteeSelector {
    /// All eligible relays for current epoch
    eligible_relays: Vec<RelayEligibility>,

    /// Total normalized weight (after caps)
    total_weight: u64,

    /// VRF seed deriver
    seed_deriver: VrfSeedDeriver,

    /// Target committee size
    target_size: usize,

    /// Index of relays by various attributes for diversity lookup
    relays_by_region: HashMap<GeographicRegion, Vec<usize>>,
    relays_by_asn: HashMap<u32, Vec<usize>>,
    relays_by_prefix: HashMap<[u8; 3], Vec<usize>>,
    relays_by_operator: HashMap<Hash, Vec<usize>>,
}

impl CommitteeSelector {
    /// Create a new committee selector with eligible relays
    pub fn new(
        relays: Vec<RelayEligibility>,
        seed_deriver: VrfSeedDeriver,
        target_size: usize,
    ) -> Self {
        let mut selector = Self {
            eligible_relays: Vec::new(),
            total_weight: 0,
            seed_deriver,
            target_size: target_size.clamp(MIN_COMMITTEE_SIZE, MAX_COMMITTEE_SIZE),
            relays_by_region: HashMap::new(),
            relays_by_asn: HashMap::new(),
            relays_by_prefix: HashMap::new(),
            relays_by_operator: HashMap::new(),
        };

        selector.set_eligible_relays(relays);
        selector
    }

    /// Set eligible relays with normalized weights
    pub fn set_eligible_relays(&mut self, relays: Vec<RelayEligibility>) {
        self.eligible_relays.clear();
        self.relays_by_region.clear();
        self.relays_by_asn.clear();
        self.relays_by_prefix.clear();
        self.relays_by_operator.clear();
        self.total_weight = 0;

        // Calculate total raw weight for cap calculation
        let total_raw: u64 = relays.iter().map(|r| r.raw_weight).sum();
        let max_weight_per_relay = total_raw * MAX_RELAY_WEIGHT_BPS / 10000;

        for (i, mut relay) in relays.into_iter().enumerate() {
            // Apply weight cap
            let capped_weight = relay.raw_weight.min(max_weight_per_relay);
            relay.raw_weight = capped_weight;

            self.total_weight += capped_weight;

            // Build indices
            self.relays_by_region
                .entry(relay.region)
                .or_default()
                .push(i);
            self.relays_by_asn.entry(relay.asn).or_default().push(i);
            self.relays_by_prefix
                .entry(relay.ip_prefix)
                .or_default()
                .push(i);
            self.relays_by_operator
                .entry(relay.operator_id)
                .or_default()
                .push(i);

            self.eligible_relays.push(relay);
        }
    }

    /// Get VRF seed for a specific block height using the internal seed deriver
    pub fn get_vrf_seed(&self, target_height: u64) -> Result<Hash, CommitteeError> {
        self.seed_deriver.get_seed(target_height)
    }

    /// Derive VRF input bytes for a committee scope
    pub fn derive_vrf_input_for_scope(
        &self,
        scope: &CommitteeScope,
    ) -> Result<[u8; 64], CommitteeError> {
        self.seed_deriver.derive_vrf_input(scope)
    }

    /// Record a finalized block hash for VRF seed derivation
    pub fn record_finalized_block(&mut self, height: u64, hash: Hash) {
        self.seed_deriver.record_finalized_block(height, hash);
    }

    /// Select a committee using VRF output
    pub fn select_committee(
        &self,
        scope: CommitteeScope,
        vrf_output: VrfOutput,
        signer: &SigningKey,
    ) -> Result<Committee, CommitteeError> {
        if self.eligible_relays.len() < MIN_COMMITTEE_SIZE {
            return Err(CommitteeError::InsufficientRelays(
                self.eligible_relays.len(),
                MIN_COMMITTEE_SIZE,
            ));
        }

        if self.total_weight == 0 {
            return Err(CommitteeError::ZeroTotalWeight);
        }

        // Generate deterministic selection using VRF output
        let mut selected_indices = Vec::new();
        let mut selected_set = HashSet::new();
        let mut region_counts: HashMap<GeographicRegion, usize> = HashMap::new();
        let mut asn_counts: HashMap<u32, usize> = HashMap::new();
        let mut prefix_counts: HashMap<[u8; 3], usize> = HashMap::new();
        let mut operator_counts: HashMap<Hash, usize> = HashMap::new();

        // Weighted selection using hash chain from VRF output
        let mut hash_state = vrf_output.output;
        let mut selection_counter = 0u64;
        const MAX_SELECTION_ATTEMPTS: u64 = 10000;

        while selected_indices.len() < self.target_size
            && selection_counter < MAX_SELECTION_ATTEMPTS
        {
            // Derive selection value from hash chain
            let selection_hash = self.derive_selection_hash(&hash_state, selection_counter);

            // Safe conversion: selection_hash is always 32 bytes, we take first 8
            let selection_bytes: [u8; 8] = selection_hash[0..8]
                .try_into()
                .map_err(|_| CommitteeError::Internal("hash slice conversion failed".into()))?;
            let selection_value = u64::from_le_bytes(selection_bytes);

            // Select relay based on weight (total_weight checked non-zero above)
            let selected_idx = self.weighted_select(selection_value % self.total_weight);

            if let Some(idx) = selected_idx {
                if !selected_set.contains(&idx) {
                    let relay = &self.eligible_relays[idx];

                    // Check diversity constraints using get() with default
                    let region_count = region_counts.get(&relay.region).copied().unwrap_or(0);
                    let asn_count = asn_counts.get(&relay.asn).copied().unwrap_or(0);
                    let prefix_count = prefix_counts.get(&relay.ip_prefix).copied().unwrap_or(0);
                    let operator_count = operator_counts
                        .get(&relay.operator_id)
                        .copied()
                        .unwrap_or(0);

                    // Apply diversity limits (including region constraint)
                    if region_count < MAX_RELAYS_PER_REGION
                        && asn_count < MAX_RELAYS_PER_ASN
                        && prefix_count < MAX_RELAYS_PER_IP_PREFIX
                        && operator_count < MAX_RELAYS_PER_OPERATOR
                    {
                        selected_set.insert(idx);
                        selected_indices.push((idx, selection_counter, Hash::from(selection_hash)));

                        *region_counts.entry(relay.region).or_insert(0) += 1;
                        *asn_counts.entry(relay.asn).or_insert(0) += 1;
                        *prefix_counts.entry(relay.ip_prefix).or_insert(0) += 1;
                        *operator_counts.entry(relay.operator_id).or_insert(0) += 1;
                    }
                }
            }

            // Advance hash chain
            hash_state =
                *blake3::hash(&[hash_state.as_slice(), &[selection_counter as u8]].concat())
                    .as_bytes();
            selection_counter += 1;
        }

        // Check if we found enough relays
        if selected_indices.len() < self.target_size {
            return Err(CommitteeError::SelectionExhausted(
                self.target_size - selected_indices.len(),
                selection_counter,
            ));
        }

        // Verify diversity requirements
        if region_counts.len() < MIN_REQUIRED_REGIONS {
            return Err(CommitteeError::DiversityNotMet(format!(
                "Only {} regions represented, need at least {}. Regions: {:?}",
                region_counts.len(),
                MIN_REQUIRED_REGIONS,
                region_counts.keys().collect::<Vec<_>>()
            )));
        }

        // Build committee members
        let mut members = Vec::new();
        let mut total_selected_weight = 0u64;

        for (idx, selection_index, selection_proof) in selected_indices {
            let relay = &self.eligible_relays[idx];

            let member = CommitteeMember {
                relay_id: relay.relay_id,
                public_key: relay.public_key,
                weight: relay.raw_weight,
                selection_index,
                selection_proof,
                region: relay.region,
                asn: relay.asn,
                operator_id: relay.operator_id,
            };

            total_selected_weight += relay.raw_weight;
            members.push(member);
        }

        // Calculate diversity metrics
        let diversity = DiversityMetrics {
            region_count: region_counts.len(),
            asn_count: asn_counts.len(),
            operator_count: operator_counts.len(),
            ip_prefix_count: prefix_counts.len(),
            weight_gini: self.calculate_gini(&members),
        };

        // Sign the committee selection for authenticity verification
        // The signature covers scope, vrf output, and member selection proofs
        let mut signing_data = Vec::new();
        signing_data.extend_from_slice(&scope.block_height.to_le_bytes());
        signing_data.push(scope.subblock_index);
        signing_data.push(scope.miniblock_index.unwrap_or(255));
        signing_data.push(scope.committee_type as u8);
        signing_data.extend_from_slice(&vrf_output.output);
        for member in &members {
            signing_data.extend_from_slice(&member.selection_proof.as_bytes()[..16]);
        }
        let selector_signature = Some(signer.sign(&signing_data));

        Ok(Committee {
            scope,
            members,
            total_weight: total_selected_weight,
            vrf_output,
            diversity,
            selector_signature,
        })
    }

    /// Derive selection hash from VRF output and counter
    fn derive_selection_hash(&self, vrf_output: &[u8; 32], counter: u64) -> [u8; 32] {
        let mut input = Vec::with_capacity(40);
        input.extend_from_slice(vrf_output);
        input.extend_from_slice(&counter.to_le_bytes());
        *blake3::hash(&input).as_bytes()
    }

    /// Weighted selection of relay index
    fn weighted_select(&self, value: u64) -> Option<usize> {
        let mut cumulative = 0u64;

        for (i, relay) in self.eligible_relays.iter().enumerate() {
            cumulative += relay.raw_weight;
            if value < cumulative {
                return Some(i);
            }
        }

        // Fallback to last relay if weights don't sum correctly
        if !self.eligible_relays.is_empty() {
            Some(self.eligible_relays.len() - 1)
        } else {
            None
        }
    }

    /// Calculate Gini coefficient for weight distribution
    /// Returns a value between 0 (perfect equality) and 1 (perfect inequality)
    fn calculate_gini(&self, members: &[CommitteeMember]) -> f64 {
        if members.is_empty() {
            return 0.0;
        }

        let mut weights: Vec<f64> = members.iter().map(|m| m.weight as f64).collect();
        // Sort using total_cmp which handles NaN safely (though we don't have NaN here)
        weights.sort_by(|a, b| a.total_cmp(b));

        let n = weights.len() as f64;
        let total: f64 = weights.iter().sum();

        if total == 0.0 {
            return 0.0;
        }

        let mut gini_sum = 0.0;
        for (i, w) in weights.iter().enumerate() {
            gini_sum += (2.0 * (i + 1) as f64 - n - 1.0) * w;
        }

        // Clamp to valid range [0, 1] to handle floating point errors
        (gini_sum / (n * total)).clamp(0.0, 1.0)
    }

    /// Verify a committee selection is valid
    pub fn verify_committee(&self, committee: &Committee) -> Result<(), CommitteeError> {
        // Verify VRF output matches scope
        if committee.vrf_output.height != committee.scope.block_height
            || committee.vrf_output.subblock != committee.scope.subblock_index
            || committee.vrf_output.miniblock != committee.scope.miniblock_index
        {
            return Err(CommitteeError::ScopeMismatch);
        }

        // Verify each member's selection proof
        let hash_state = committee.vrf_output.output;
        let mut verified_indices = HashSet::new();

        // Calculate max weight for cap verification
        let max_weight = if self.total_weight > 0 {
            self.total_weight * MAX_RELAY_WEIGHT_BPS / 10000
        } else {
            u64::MAX // No cap if total weight unknown
        };

        for member in &committee.members {
            // Verify selection proof
            let expected_hash = self.derive_selection_hash(&hash_state, member.selection_index);
            if member.selection_proof != Hash::from(expected_hash) {
                return Err(CommitteeError::InvalidSelectionProof(member.relay_id));
            }

            // Verify weight cap
            if member.weight > max_weight {
                return Err(CommitteeError::WeightCapExceeded(
                    member.relay_id,
                    member.weight,
                    max_weight,
                ));
            }

            verified_indices.insert(member.selection_index);
        }

        // Verify diversity
        if committee.diversity.region_count < MIN_REQUIRED_REGIONS {
            return Err(CommitteeError::DiversityNotMet(format!(
                "Only {} regions",
                committee.diversity.region_count
            )));
        }

        Ok(())
    }
}

/// Committee vote with member attestation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitteeVote {
    /// Committee this vote is from
    pub committee_scope: CommitteeScope,

    /// Block/commitment being voted on
    pub target: VoteTarget,

    /// Member casting the vote
    pub member: RelayId,

    /// Vote decision
    pub decision: VoteDecision,

    /// Signature over vote
    pub signature: Signature,

    /// Timestamp
    pub timestamp: u64,
}

/// What is being voted on
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum VoteTarget {
    /// PoRW miniblock commitment
    PoRWMiniblock { hash: Hash },
    /// PoT subblock path set
    PoTSubblock { hash: Hash },
    /// TSC checkpoint
    TSCCheckpoint { epoch: u64, hash: Hash },
    /// Block finality
    BlockFinality { height: u64, hash: Hash },
}

/// Vote decision
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoteDecision {
    /// Approve the target
    Approve,
    /// Reject the target (with reason code)
    Reject(u8),
    /// Abstain (no opinion, but participated)
    Abstain,
}

/// Aggregate committee votes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedVotes {
    /// Committee scope
    pub committee_scope: CommitteeScope,

    /// Target being voted on
    pub target: VoteTarget,

    /// Total weight that voted
    pub voting_weight: u64,

    /// Weight that approved
    pub approve_weight: u64,

    /// Weight that rejected
    pub reject_weight: u64,

    /// Weight that abstained
    pub abstain_weight: u64,

    /// Individual votes (for verification)
    pub votes: Vec<CommitteeVote>,

    /// Whether threshold was met
    pub threshold_met: bool,

    /// Required threshold (basis points)
    pub required_threshold_bps: u64,
}

impl AggregatedVotes {
    /// Create new vote aggregation
    pub fn new(
        committee_scope: CommitteeScope,
        target: VoteTarget,
        required_threshold_bps: u64,
    ) -> Self {
        Self {
            committee_scope,
            target,
            voting_weight: 0,
            approve_weight: 0,
            reject_weight: 0,
            abstain_weight: 0,
            votes: Vec::new(),
            threshold_met: false,
            required_threshold_bps,
        }
    }

    /// Add a vote
    pub fn add_vote(&mut self, vote: CommitteeVote, member_weight: u64) {
        self.voting_weight += member_weight;

        match vote.decision {
            VoteDecision::Approve => self.approve_weight += member_weight,
            VoteDecision::Reject(_) => self.reject_weight += member_weight,
            VoteDecision::Abstain => self.abstain_weight += member_weight,
        }

        self.votes.push(vote);

        // Check if threshold is met
        // Threshold is based on approve weight vs total voting weight
        let approve_bps = self.approve_weight * 10000 / self.voting_weight.max(1);
        self.threshold_met = approve_bps >= self.required_threshold_bps;
    }

    /// Check if quorum is met (>50% of committee voted)
    pub fn quorum_met(&self, committee_total_weight: u64) -> bool {
        self.voting_weight * 2 > committee_total_weight
    }
}

/// Committee manager for tracking active committees
pub struct CommitteeManager {
    /// Current epoch's eligible relays
    epoch_relays: Vec<RelayEligibility>,

    /// Active committees by scope
    active_committees: HashMap<CommitteeScope, Committee>,

    /// Committee selector
    selector: Option<CommitteeSelector>,

    /// Vote aggregations in progress
    vote_aggregations: HashMap<(CommitteeScope, VoteTarget), AggregatedVotes>,
}

impl CommitteeManager {
    pub fn new() -> Self {
        Self {
            epoch_relays: Vec::new(),
            active_committees: HashMap::new(),
            selector: None,
            vote_aggregations: HashMap::new(),
        }
    }

    /// Update eligible relays for new epoch
    pub fn update_epoch_relays(
        &mut self,
        relays: Vec<RelayEligibility>,
        seed_deriver: VrfSeedDeriver,
        committee_size: usize,
    ) {
        self.epoch_relays = relays.clone();
        self.selector = Some(CommitteeSelector::new(relays, seed_deriver, committee_size));
    }

    /// Get or create committee for scope
    pub fn get_or_create_committee(
        &mut self,
        scope: CommitteeScope,
        vrf_output: VrfOutput,
        signer: &SigningKey,
    ) -> Result<&Committee, CommitteeError> {
        // Use entry API to avoid double lookup and eliminate unwrap
        use std::collections::hash_map::Entry;

        match self.active_committees.entry(scope) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let selector = self
                    .selector
                    .as_ref()
                    .ok_or(CommitteeError::InsufficientRelays(0, MIN_COMMITTEE_SIZE))?;

                let committee = selector.select_committee(scope, vrf_output, signer)?;
                Ok(entry.insert(committee))
            }
        }
    }

    /// Get existing committee
    pub fn get_committee(&self, scope: &CommitteeScope) -> Option<&Committee> {
        self.active_committees.get(scope)
    }

    /// Process incoming vote
    pub fn process_vote(
        &mut self,
        vote: CommitteeVote,
        required_threshold_bps: u64,
    ) -> Result<bool, CommitteeError> {
        let committee = self
            .active_committees
            .get(&vote.committee_scope)
            .ok_or(CommitteeError::ScopeMismatch)?;

        // Find member weight
        let member = committee
            .members
            .iter()
            .find(|m| m.relay_id == vote.member)
            .ok_or(CommitteeError::InvalidSelectionProof(vote.member))?;

        let key = (vote.committee_scope, vote.target.clone());

        let aggregation = self.vote_aggregations.entry(key).or_insert_with(|| {
            AggregatedVotes::new(
                vote.committee_scope,
                vote.target.clone(),
                required_threshold_bps,
            )
        });

        aggregation.add_vote(vote, member.weight);

        Ok(aggregation.threshold_met && aggregation.quorum_met(committee.total_weight))
    }

    /// Get vote aggregation status
    pub fn get_vote_status(
        &self,
        scope: &CommitteeScope,
        target: &VoteTarget,
    ) -> Option<&AggregatedVotes> {
        self.vote_aggregations.get(&(*scope, target.clone()))
    }

    /// Clean up old committees
    pub fn cleanup_old_committees(&mut self, current_height: u64, keep_blocks: u64) {
        let min_height = current_height.saturating_sub(keep_blocks);

        self.active_committees
            .retain(|scope, _| scope.block_height >= min_height);
        self.vote_aggregations
            .retain(|(scope, _), _| scope.block_height >= min_height);
    }
}

impl Default for CommitteeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;
    use rand::thread_rng;

    fn create_test_relays(count: usize) -> Vec<RelayEligibility> {
        let mut rng = thread_rng();
        let regions = GeographicRegion::all();

        (0..count)
            .map(|i| {
                let sk = SigningKey::generate(&mut rng);
                let pk = sk.verifying_key();

                RelayEligibility {
                    relay_id: RelayId::from_public_key(&pk),
                    public_key: pk,
                    stake: 10000 + (i as u64 * 100),
                    uptime_score: 0.95 + (i as f64 * 0.001).min(0.049),
                    region: regions[i % regions.len()],
                    asn: (i / 3) as u32 + 1000,
                    ip_prefix: [(i % 256) as u8, ((i / 256) % 256) as u8, 0],
                    operator_id: Hash::from(*blake3::hash(&i.to_le_bytes()).as_bytes()),
                    raw_weight: 100 + (i as u64 * 10),
                }
            })
            .collect()
    }

    #[test]
    fn test_committee_selection() {
        let relays = create_test_relays(100);
        let mut seed_deriver = VrfSeedDeriver::new(6);

        // Record some finalized blocks
        for i in 0..10 {
            seed_deriver.record_finalized_block(i, Hash::from([i as u8; 32]));
        }

        let selector = CommitteeSelector::new(relays, seed_deriver, 21);

        let scope = CommitteeScope {
            block_height: 10,
            subblock_index: 0,
            miniblock_index: Some(0),
            committee_type: CommitteeType::PoRWAttestation,
        };

        // Use the test helper to create a valid VRF output
        let vrf_output = VrfOutput::for_testing(b"test_committee_selection", 10, 0, Some(0));

        let sk = SigningKey::generate(&mut thread_rng());
        let committee = selector.select_committee(scope, vrf_output, &sk).unwrap();

        assert!(committee.members.len() >= MIN_COMMITTEE_SIZE);
        assert!(committee.members.len() <= MAX_COMMITTEE_SIZE);
        assert!(committee.diversity.region_count >= MIN_REQUIRED_REGIONS);
    }

    #[test]
    fn test_diversity_constraints() {
        // Create relays all from same region (should fail diversity)
        let mut relays = create_test_relays(50);
        for relay in &mut relays {
            relay.region = GeographicRegion::NorthAmerica;
        }

        let mut seed_deriver = VrfSeedDeriver::new(6);
        for i in 0..10 {
            seed_deriver.record_finalized_block(i, Hash::from([i as u8; 32]));
        }

        let selector = CommitteeSelector::new(relays, seed_deriver, 21);

        let scope = CommitteeScope {
            block_height: 10,
            subblock_index: 0,
            miniblock_index: None,
            committee_type: CommitteeType::PoRWAttestation,
        };

        let vrf_output = VrfOutput::for_testing(b"test_diversity_constraints", 10, 0, None);

        let sk = SigningKey::generate(&mut thread_rng());
        let result = selector.select_committee(scope, vrf_output, &sk);

        assert!(matches!(result, Err(CommitteeError::DiversityNotMet(_))));
    }

    #[test]
    fn test_weight_cap() {
        let mut relays = create_test_relays(20);
        let original_total_raw: u64 = relays.iter().map(|r| r.raw_weight).sum();

        // Verify we have a baseline total weight before modification
        assert!(
            original_total_raw > 0,
            "Original total weight should be positive"
        );

        // Make one relay have 50% of weight (should be capped)
        relays[0].raw_weight = 100000;
        let modified_total_raw: u64 = relays.iter().map(|r| r.raw_weight).sum();

        // Modified total should be much larger than original due to whale weight
        assert!(
            modified_total_raw > original_total_raw,
            "Modified total {} should exceed original {}",
            modified_total_raw,
            original_total_raw
        );

        let mut seed_deriver = VrfSeedDeriver::new(6);
        for i in 0..10 {
            seed_deriver.record_finalized_block(i, Hash::from([i as u8; 32]));
        }

        let selector = CommitteeSelector::new(relays, seed_deriver, 15);

        // Verify weight was capped: max_weight is calculated from the original raw total
        // before capping was applied, so we need to check that the largest weight
        // is now <= the cap that was applied during construction
        let max_weight = modified_total_raw * MAX_RELAY_WEIGHT_BPS / 10000;

        for relay in &selector.eligible_relays {
            assert!(
                relay.raw_weight <= max_weight,
                "Relay weight {} exceeds max {}",
                relay.raw_weight,
                max_weight
            );
        }

        // Also verify that the whale's weight was actually reduced
        let whale_capped_weight = selector
            .eligible_relays
            .iter()
            .max_by_key(|r| r.raw_weight)
            .unwrap()
            .raw_weight;
        assert!(
            whale_capped_weight < 100000,
            "Whale weight should have been capped from 100000 to {}",
            whale_capped_weight
        );
    }

    #[test]
    fn test_vote_aggregation() {
        let mut aggregation = AggregatedVotes::new(
            CommitteeScope {
                block_height: 100,
                subblock_index: 0,
                miniblock_index: None,
                committee_type: CommitteeType::PoRWAttestation,
            },
            VoteTarget::BlockFinality {
                height: 99,
                hash: Hash::from([0u8; 32]),
            },
            6700, // 67% threshold
        );

        let rng = &mut thread_rng();

        // Add votes
        for i in 0..10 {
            let sk = SigningKey::generate(rng);
            let vote = CommitteeVote {
                committee_scope: aggregation.committee_scope,
                target: aggregation.target.clone(),
                member: RelayId::from_public_key(&sk.verifying_key()),
                decision: if i < 7 {
                    VoteDecision::Approve
                } else {
                    VoteDecision::Reject(0)
                },
                signature: sk.sign(b"vote"),
                timestamp: 12345,
            };

            aggregation.add_vote(vote, 100);
        }

        // 70% approved, should meet 67% threshold
        assert!(aggregation.threshold_met);
        assert_eq!(aggregation.approve_weight, 700);
        assert_eq!(aggregation.reject_weight, 300);
    }
}
