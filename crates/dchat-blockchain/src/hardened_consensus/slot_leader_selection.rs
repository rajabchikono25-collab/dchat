//! VRF-Based Slot Leader Selection
//!
//! This module implements single-leader selection per slot using VRF (Verifiable
//! Random Functions). Each slot has exactly one leader who is responsible for
//! proposing the next block. The selection is:
//!
//! - **Deterministic**: All validators can independently compute who the leader is
//! - **Unpredictable**: The leader cannot be known until the VRF is revealed
//! - **Verifiable**: Anyone can verify the leader's claim using the VRF proof
//! - **Fair**: Leaders are selected proportionally to their stake weight
//!
//! # Slot Structure
//!
//! The slot structure aligns with the existing block hierarchy:
//! - 1 Epoch = N Blocks (configurable, default 7200 blocks = 4 hours)
//! - 1 Block = 10 Subblocks (2 seconds per block)
//! - 1 Subblock = 10 Miniblocks (200ms per subblock)
//! - 1 Miniblock = 20ms
//!
//! Each slot corresponds to one block production opportunity.
//!
//! # Leader Selection Algorithm
//!
//! 1. Derive slot seed from finalized block hash + slot number
//! 2. Each validator computes VRF(slot_seed, private_key) → (output, proof)
//! 3. Leader is selected based on weighted VRF output comparison
//! 4. Leader proposes block and includes VRF proof for verification

use super::vrf_committees::{
    GeographicRegion, VrfProofBytes, VrfSeedDeriver, VRF_OUTPUT_SIZE, VRF_PROOF_SIZE,
};
use crate::block_hierarchy::Hash;
use ed25519_dalek::SigningKey;
use merlin::Transcript;
use schnorrkel::{
    vrf::{VRFPreOut, VRFProof},
    Keypair as SchnorrkelKeypair, PublicKey as SchnorrkelPublicKey,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// VRF context for slot leader selection (domain separation)
const SLOT_VRF_CONTEXT: &[u8] = b"dchat-slot-leader-vrf-v1";

/// Default slot duration in milliseconds (2 seconds = 1 block)
pub const DEFAULT_SLOT_DURATION_MS: u64 = 2000;

/// Default epoch length in slots (4 hours = 7200 slots)
pub const DEFAULT_EPOCH_LENGTH_SLOTS: u64 = 7200;

/// Minimum time before next slot to announce leadership (ms)
pub const LEADERSHIP_ANNOUNCE_BUFFER_MS: u64 = 500;

/// Maximum stake weight per validator (basis points, 10% = 1000 bps)
pub const MAX_VALIDATOR_WEIGHT_BPS: u64 = 1000;

/// Slot identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SlotId {
    /// Epoch number
    pub epoch: u64,
    /// Slot number within epoch
    pub slot: u64,
}

impl SlotId {
    /// Create a new slot ID
    pub fn new(epoch: u64, slot: u64) -> Self {
        Self { epoch, slot }
    }

    /// Get absolute slot number (across all epochs)
    pub fn absolute_slot(&self, epoch_length: u64) -> u64 {
        self.epoch * epoch_length + self.slot
    }

    /// Create from absolute slot number
    pub fn from_absolute(absolute: u64, epoch_length: u64) -> Self {
        Self {
            epoch: absolute / epoch_length,
            slot: absolute % epoch_length,
        }
    }

    /// Get the block height this slot corresponds to
    pub fn block_height(&self, epoch_length: u64) -> u64 {
        self.absolute_slot(epoch_length)
    }

    /// Get next slot
    pub fn next(&self, epoch_length: u64) -> Self {
        if self.slot + 1 >= epoch_length {
            Self {
                epoch: self.epoch + 1,
                slot: 0,
            }
        } else {
            Self {
                epoch: self.epoch,
                slot: self.slot + 1,
            }
        }
    }

    /// Get previous slot
    pub fn prev(&self, epoch_length: u64) -> Option<Self> {
        if self.slot > 0 {
            Some(Self {
                epoch: self.epoch,
                slot: self.slot - 1,
            })
        } else if self.epoch > 0 {
            Some(Self {
                epoch: self.epoch - 1,
                slot: epoch_length - 1,
            })
        } else {
            None
        }
    }
}

impl std::fmt::Display for SlotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "e{}:s{}", self.epoch, self.slot)
    }
}

/// VRF leadership proof for a slot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotLeaderProof {
    /// Slot this proof is for
    pub slot: SlotId,
    /// VRF output (32 bytes) - deterministic from slot seed + key
    pub vrf_output: [u8; VRF_OUTPUT_SIZE],
    /// VRF proof (64 bytes) - proves correct computation
    pub vrf_proof: VrfProofBytes,
    /// Public key of the leader (Schnorrkel)
    pub leader_public_key: [u8; 32],
    /// Ed25519 verifying key for block signing
    pub leader_signing_key: [u8; 32],
    /// Validator's normalized weight at epoch start (basis points)
    pub validator_weight_bps: u64,
    /// Geographic region of validator
    pub region: GeographicRegion,
    /// Timestamp when proof was generated
    pub timestamp: u64,
}

impl SlotLeaderProof {
    /// Generate a leadership proof for a slot
    pub fn generate(
        slot: SlotId,
        slot_seed: &[u8; 32],
        vrf_keypair: &SchnorrkelKeypair,
        signing_key: &SigningKey,
        validator_weight_bps: u64,
        region: GeographicRegion,
    ) -> Self {
        // Create VRF input from slot seed and slot ID
        let mut vrf_input = Vec::with_capacity(48);
        vrf_input.extend_from_slice(slot_seed);
        vrf_input.extend_from_slice(&slot.epoch.to_le_bytes());
        vrf_input.extend_from_slice(&slot.slot.to_le_bytes());

        // Compute VRF
        let mut transcript = Transcript::new(SLOT_VRF_CONTEXT);
        transcript.append_message(b"slot_input", &vrf_input);
        let (inout, proof, _) = vrf_keypair.vrf_sign(transcript);

        let vrf_output: [u8; 32] = inout.to_preout().to_bytes();
        let vrf_proof_bytes: [u8; VRF_PROOF_SIZE] = proof.to_bytes();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            slot,
            vrf_output,
            vrf_proof: VrfProofBytes(vrf_proof_bytes),
            leader_public_key: vrf_keypair.public.to_bytes(),
            leader_signing_key: signing_key.verifying_key().to_bytes(),
            validator_weight_bps,
            region,
            timestamp,
        }
    }

    /// Verify the VRF proof is valid for the given slot seed
    pub fn verify(&self, slot_seed: &[u8; 32]) -> Result<(), SlotLeaderError> {
        // Reconstruct VRF input
        let mut vrf_input = Vec::with_capacity(48);
        vrf_input.extend_from_slice(slot_seed);
        vrf_input.extend_from_slice(&self.slot.epoch.to_le_bytes());
        vrf_input.extend_from_slice(&self.slot.slot.to_le_bytes());

        // Parse public key
        let public_key = SchnorrkelPublicKey::from_bytes(&self.leader_public_key)
            .map_err(|_| SlotLeaderError::InvalidVrfProof)?;

        // Parse proof
        let proof = VRFProof::from_bytes(&self.vrf_proof.0)
            .map_err(|_| SlotLeaderError::InvalidVrfProof)?;

        // Parse preout
        let preout = VRFPreOut::from_bytes(&self.vrf_output)
            .map_err(|_| SlotLeaderError::InvalidVrfProof)?;

        // Verify
        let mut transcript = Transcript::new(SLOT_VRF_CONTEXT);
        transcript.append_message(b"slot_input", &vrf_input);

        public_key
            .vrf_verify(transcript, &preout, &proof)
            .map_err(|_| SlotLeaderError::InvalidVrfProof)?;

        Ok(())
    }

    /// Compute the leadership score from VRF output and weight
    /// Returns a comparable value where lower = better (more likely to be leader)
    pub fn leadership_score(&self) -> u128 {
        // Convert VRF output to u128 for high precision
        let vrf_value = u128::from_le_bytes(
            self.vrf_output[0..16]
                .try_into()
                .expect("vrf_output is 32 bytes"),
        );

        // Score = VRF / weight
        // Higher weight = lower score (better chance to be leader)
        // For same VRF output, validator with more stake gets lower (better) score
        if self.validator_weight_bps > 0 {
            vrf_value / (self.validator_weight_bps as u128)
        } else {
            u128::MAX
        }
    }
}

/// Slot leader selection errors
#[derive(Debug, Error)]
pub enum SlotLeaderError {
    #[error("Invalid VRF proof")]
    InvalidVrfProof,

    #[error("Slot seed not available for epoch {0}")]
    SlotSeedNotAvailable(u64),

    #[error("No validators registered for epoch {0}")]
    NoValidatorsRegistered(u64),

    #[error("Not the slot leader")]
    NotSlotLeader,

    #[error("Slot already has a leader: {0}")]
    SlotAlreadyClaimed(String),

    #[error("Leadership claim too early: {0}ms before slot")]
    LeadershipClaimTooEarly(u64),

    #[error("Leadership claim too late: {0}ms after slot")]
    LeadershipClaimTooLate(u64),

    #[error("Weight exceeds maximum: {0} > {1}")]
    WeightExceedsMaximum(u64, u64),

    #[error("Validator not registered: {0:?}")]
    ValidatorNotRegistered([u8; 32]),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Validator info for leader selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Validator's VRF public key (Schnorrkel)
    pub vrf_public_key: [u8; 32],
    /// Validator's Ed25519 signing public key
    pub signing_public_key: [u8; 32],
    /// Stake amount in base units
    pub stake_amount: u64,
    /// Normalized weight (basis points, capped at MAX_VALIDATOR_WEIGHT_BPS)
    pub weight_bps: u64,
    /// Geographic region
    pub region: GeographicRegion,
    /// Whether validator is active
    pub is_active: bool,
    /// Last slot this validator was leader
    pub last_leader_slot: Option<SlotId>,
    /// Total blocks produced
    pub blocks_produced: u64,
    /// Blocks missed when leader
    pub blocks_missed: u64,
}

impl ValidatorInfo {
    /// Calculate uptime score (0.0 - 1.0)
    pub fn uptime_score(&self) -> f64 {
        let total = self.blocks_produced + self.blocks_missed;
        if total == 0 {
            1.0 // New validator gets benefit of the doubt
        } else {
            self.blocks_produced as f64 / total as f64
        }
    }
}

/// Slot leader selector
pub struct SlotLeaderSelector {
    /// Epoch length in slots
    epoch_length: u64,

    /// Slot duration in milliseconds
    slot_duration_ms: u64,

    /// Registered validators per epoch: epoch -> (vrf_pubkey -> info)
    validators: HashMap<u64, HashMap<[u8; 32], ValidatorInfo>>,

    /// Epoch seeds: epoch -> seed derived from finalized blocks
    epoch_seeds: HashMap<u64, [u8; 32]>,

    /// Claimed leadership proofs: SlotId -> proof
    claimed_slots: HashMap<SlotId, SlotLeaderProof>,

    /// Genesis timestamp (Unix epoch seconds)
    genesis_timestamp: u64,

    /// Current epoch (updated as chain progresses)
    current_epoch: u64,

    /// VRF seed deriver for chain finality
    seed_deriver: VrfSeedDeriver,
}

impl SlotLeaderSelector {
    /// Create a new slot leader selector
    pub fn new(epoch_length: u64, slot_duration_ms: u64, genesis_timestamp: u64) -> Self {
        Self {
            epoch_length,
            slot_duration_ms,
            validators: HashMap::new(),
            epoch_seeds: HashMap::new(),
            claimed_slots: HashMap::new(),
            genesis_timestamp,
            current_epoch: 0,
            seed_deriver: VrfSeedDeriver::new(6), // 6 block confirmations
        }
    }

    /// Get current epoch
    pub fn get_current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Set current epoch
    pub fn set_current_epoch(&mut self, epoch: u64) {
        self.current_epoch = epoch;
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        let genesis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self::new(
            DEFAULT_EPOCH_LENGTH_SLOTS,
            DEFAULT_SLOT_DURATION_MS,
            genesis,
        )
    }

    /// Get current slot based on wall clock time
    pub fn current_slot(&self) -> SlotId {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.slot_for_timestamp(now)
    }

    /// Get slot for a given Unix timestamp
    pub fn slot_for_timestamp(&self, timestamp: u64) -> SlotId {
        if timestamp < self.genesis_timestamp {
            return SlotId::new(0, 0);
        }

        let elapsed_ms = (timestamp - self.genesis_timestamp) * 1000;
        let absolute_slot = elapsed_ms / self.slot_duration_ms;

        SlotId::from_absolute(absolute_slot, self.epoch_length)
    }

    /// Get timestamp for start of a slot
    pub fn slot_start_timestamp(&self, slot: SlotId) -> u64 {
        let absolute_slot = slot.absolute_slot(self.epoch_length);
        self.genesis_timestamp + (absolute_slot * self.slot_duration_ms) / 1000
    }

    /// Get timestamp for end of a slot
    pub fn slot_end_timestamp(&self, slot: SlotId) -> u64 {
        self.slot_start_timestamp(slot.next(self.epoch_length))
    }

    /// Register a validator for an epoch
    pub fn register_validator(&mut self, epoch: u64, validator: ValidatorInfo) {
        let epoch_validators = self.validators.entry(epoch).or_insert_with(HashMap::new);
        epoch_validators.insert(validator.vrf_public_key, validator);
    }

    /// Update validators for a new epoch with stake snapshot
    pub fn update_epoch_validators(&mut self, epoch: u64, validators: Vec<ValidatorInfo>) {
        // Calculate total stake for normalization
        let total_stake: u64 = validators.iter().map(|v| v.stake_amount).sum();

        let mut epoch_validators = HashMap::new();

        for mut validator in validators {
            if total_stake > 0 {
                // Normalize weight to basis points
                let raw_weight_bps = (validator.stake_amount * 10000) / total_stake;
                // Cap at maximum
                validator.weight_bps = raw_weight_bps.min(MAX_VALIDATOR_WEIGHT_BPS);
            } else {
                validator.weight_bps = 0;
            }

            epoch_validators.insert(validator.vrf_public_key, validator);
        }

        self.validators.insert(epoch, epoch_validators);
    }

    /// Set the seed for an epoch (derived from finalized chain state)
    pub fn set_epoch_seed(&mut self, epoch: u64, seed: [u8; 32]) {
        self.epoch_seeds.insert(epoch, seed);
    }

    /// Record a finalized block for seed derivation
    pub fn record_finalized_block(&mut self, height: u64, hash: Hash) {
        self.seed_deriver.record_finalized_block(height, hash);

        // Derive epoch seed when we have enough confirmations
        let epoch = height / self.epoch_length;
        if !self.epoch_seeds.contains_key(&epoch) {
            if let Ok(seed) = self.seed_deriver.get_seed(height) {
                let mut epoch_seed = [0u8; 32];
                epoch_seed.copy_from_slice(seed.as_bytes());
                self.epoch_seeds.insert(epoch, epoch_seed);
            }
        }
    }

    /// Get the slot seed for leader selection
    pub fn get_slot_seed(&self, slot: SlotId) -> Result<[u8; 32], SlotLeaderError> {
        // Get epoch seed
        let epoch_seed = self
            .epoch_seeds
            .get(&slot.epoch)
            .ok_or(SlotLeaderError::SlotSeedNotAvailable(slot.epoch))?;

        // Derive slot-specific seed
        let mut hasher = blake3::Hasher::new();
        hasher.update(SLOT_VRF_CONTEXT);
        hasher.update(epoch_seed);
        hasher.update(&slot.epoch.to_le_bytes());
        hasher.update(&slot.slot.to_le_bytes());

        Ok(*hasher.finalize().as_bytes())
    }

    /// Select the leader for a slot based on submitted proofs
    ///
    /// All validators compute their VRF and submit proofs.
    /// The validator with the lowest leadership_score() wins.
    pub fn select_leader(&self, slot: SlotId) -> Result<SlotLeaderProof, SlotLeaderError> {
        // Check if we already have a claimed leader
        if let Some(proof) = self.claimed_slots.get(&slot) {
            return Ok(proof.clone());
        }

        Err(SlotLeaderError::NotSlotLeader)
    }

    /// Submit a leadership claim for a slot
    pub fn claim_leadership(&mut self, proof: SlotLeaderProof) -> Result<bool, SlotLeaderError> {
        // Verify the proof
        let slot_seed = self.get_slot_seed(proof.slot)?;
        proof.verify(&slot_seed)?;

        // Check validator is registered for this epoch
        let validators = self
            .validators
            .get(&proof.slot.epoch)
            .ok_or(SlotLeaderError::NoValidatorsRegistered(proof.slot.epoch))?;

        let validator = validators.get(&proof.leader_public_key).ok_or(
            SlotLeaderError::ValidatorNotRegistered(proof.leader_public_key),
        )?;

        // Verify weight matches
        if validator.weight_bps != proof.validator_weight_bps {
            return Err(SlotLeaderError::WeightExceedsMaximum(
                proof.validator_weight_bps,
                validator.weight_bps,
            ));
        }

        // Check if there's already a claimed leader with better score
        if let Some(existing) = self.claimed_slots.get(&proof.slot) {
            if existing.leadership_score() <= proof.leadership_score() {
                return Ok(false); // Existing leader is better or equal
            }
        }

        // This claim is the best so far
        self.claimed_slots.insert(proof.slot, proof);
        Ok(true)
    }

    /// Check if a validator is the leader for a slot
    pub fn is_leader(&self, slot: SlotId, vrf_public_key: &[u8; 32]) -> bool {
        self.claimed_slots
            .get(&slot)
            .map(|proof| &proof.leader_public_key == vrf_public_key)
            .unwrap_or(false)
    }

    /// Get the leader for a slot (if determined)
    pub fn get_leader(&self, slot: SlotId) -> Option<&SlotLeaderProof> {
        self.claimed_slots.get(&slot)
    }

    /// Compute your leadership proof for a slot (call this to check if you should propose)
    pub fn compute_leadership_proof(
        &self,
        slot: SlotId,
        vrf_keypair: &SchnorrkelKeypair,
        signing_key: &SigningKey,
    ) -> Result<SlotLeaderProof, SlotLeaderError> {
        let slot_seed = self.get_slot_seed(slot)?;

        // Get validator info
        let validators = self
            .validators
            .get(&slot.epoch)
            .ok_or(SlotLeaderError::NoValidatorsRegistered(slot.epoch))?;

        let vrf_public_key = vrf_keypair.public.to_bytes();
        let validator = validators
            .get(&vrf_public_key)
            .ok_or(SlotLeaderError::ValidatorNotRegistered(vrf_public_key))?;

        Ok(SlotLeaderProof::generate(
            slot,
            &slot_seed,
            vrf_keypair,
            signing_key,
            validator.weight_bps,
            validator.region,
        ))
    }

    /// Get all registered validators for an epoch
    pub fn get_validators(&self, epoch: u64) -> Option<&HashMap<[u8; 32], ValidatorInfo>> {
        self.validators.get(&epoch)
    }

    /// Get validator count for epoch
    pub fn validator_count(&self, epoch: u64) -> usize {
        self.validators.get(&epoch).map(|v| v.len()).unwrap_or(0)
    }

    /// Clear old epochs to free memory
    pub fn cleanup_old_epochs(&mut self, current_epoch: u64, keep_epochs: u64) {
        let min_epoch = current_epoch.saturating_sub(keep_epochs);

        self.validators.retain(|&e, _| e >= min_epoch);
        self.epoch_seeds.retain(|&e, _| e >= min_epoch);
        self.claimed_slots.retain(|slot, _| slot.epoch >= min_epoch);
    }

    /// Mark a leader as having produced a block
    pub fn record_block_produced(&mut self, slot: SlotId, vrf_public_key: &[u8; 32]) {
        if let Some(validators) = self.validators.get_mut(&slot.epoch) {
            if let Some(validator) = validators.get_mut(vrf_public_key) {
                validator.blocks_produced += 1;
                validator.last_leader_slot = Some(slot);
            }
        }
    }

    /// Mark a leader as having missed their slot
    pub fn record_block_missed(&mut self, slot: SlotId, vrf_public_key: &[u8; 32]) {
        if let Some(validators) = self.validators.get_mut(&slot.epoch) {
            if let Some(validator) = validators.get_mut(vrf_public_key) {
                validator.blocks_missed += 1;
            }
        }
    }
}

impl Default for SlotLeaderSelector {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Leadership schedule for an epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeadershipSchedule {
    /// Epoch this schedule is for
    pub epoch: u64,
    /// Expected leaders per slot (computed after all claims received)
    pub schedule: HashMap<u64, SlotLeaderProof>,
    /// Total slots in epoch
    pub total_slots: u64,
    /// Slots with confirmed leaders
    pub confirmed_slots: u64,
}

impl LeadershipSchedule {
    /// Create a new empty schedule
    pub fn new(epoch: u64, epoch_length: u64) -> Self {
        Self {
            epoch,
            schedule: HashMap::new(),
            total_slots: epoch_length,
            confirmed_slots: 0,
        }
    }

    /// Add a leader to the schedule
    pub fn add_leader(&mut self, slot: u64, proof: SlotLeaderProof) {
        if !self.schedule.contains_key(&slot) {
            self.confirmed_slots += 1;
        }
        self.schedule.insert(slot, proof);
    }

    /// Get leader for a slot
    pub fn get_leader(&self, slot: u64) -> Option<&SlotLeaderProof> {
        self.schedule.get(&slot)
    }

    /// Check if schedule is complete
    pub fn is_complete(&self) -> bool {
        self.confirmed_slots == self.total_slots
    }

    /// Get completion percentage
    pub fn completion_percentage(&self) -> f64 {
        if self.total_slots == 0 {
            0.0
        } else {
            (self.confirmed_slots as f64 / self.total_slots as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn create_test_validator(index: u8, stake: u64) -> ValidatorInfo {
        let mut rng = rand::rngs::StdRng::seed_from_u64(index as u64);
        let keypair = SchnorrkelKeypair::generate_with(&mut rng);

        ValidatorInfo {
            vrf_public_key: keypair.public.to_bytes(),
            signing_public_key: [index; 32],
            stake_amount: stake,
            weight_bps: 0, // Will be normalized
            region: GeographicRegion::NorthAmerica,
            is_active: true,
            last_leader_slot: None,
            blocks_produced: 0,
            blocks_missed: 0,
        }
    }

    #[test]
    fn test_slot_id_arithmetic() {
        let slot = SlotId::new(0, 5);
        assert_eq!(slot.absolute_slot(100), 5);

        let next = slot.next(100);
        assert_eq!(next.slot, 6);
        assert_eq!(next.epoch, 0);

        let epoch_end = SlotId::new(0, 99);
        let next_epoch = epoch_end.next(100);
        assert_eq!(next_epoch.epoch, 1);
        assert_eq!(next_epoch.slot, 0);

        let prev = next.prev(100);
        assert_eq!(prev, Some(slot));
    }

    #[test]
    fn test_validator_registration() {
        let mut selector = SlotLeaderSelector::with_defaults();

        let validators = vec![
            create_test_validator(1, 1000),
            create_test_validator(2, 2000),
            create_test_validator(3, 3000),
        ];

        selector.update_epoch_validators(0, validators);

        assert_eq!(selector.validator_count(0), 3);

        // Check weights are normalized
        let epoch_validators = selector.get_validators(0).unwrap();
        let total_weight: u64 = epoch_validators.values().map(|v| v.weight_bps).sum();
        // Should be close to 10000 but capped weights may reduce it
        assert!(total_weight <= 10000);
    }

    #[test]
    fn test_leadership_score() {
        let slot = SlotId::new(0, 0);

        // Create two proofs with different weights
        let proof1 = SlotLeaderProof {
            slot,
            vrf_output: [1u8; 32], // Same VRF output
            vrf_proof: VrfProofBytes([0u8; 64]),
            leader_public_key: [1u8; 32],
            leader_signing_key: [1u8; 32],
            validator_weight_bps: 1000, // 10% weight
            region: GeographicRegion::NorthAmerica,
            timestamp: 0,
        };

        let proof2 = SlotLeaderProof {
            slot,
            vrf_output: [1u8; 32], // Same VRF output
            vrf_proof: VrfProofBytes([0u8; 64]),
            leader_public_key: [2u8; 32],
            leader_signing_key: [2u8; 32],
            validator_weight_bps: 500, // 5% weight
            region: GeographicRegion::Europe,
            timestamp: 0,
        };

        // Higher weight should give lower (better) score
        assert!(proof1.leadership_score() < proof2.leadership_score());
    }

    #[test]
    fn test_epoch_seed_derivation() {
        let mut selector = SlotLeaderSelector::new(100, 2000, 0);

        // Record some finalized blocks
        for i in 0..10 {
            selector.record_finalized_block(i, Hash::from([i as u8; 32]));
        }

        // Set epoch seed manually
        selector.set_epoch_seed(0, [42u8; 32]);

        // Should be able to get slot seed
        let slot = SlotId::new(0, 5);
        let seed = selector.get_slot_seed(slot);
        assert!(seed.is_ok());

        // Different slots should have different seeds
        let slot2 = SlotId::new(0, 6);
        let seed2 = selector.get_slot_seed(slot2).unwrap();
        assert_ne!(seed.unwrap(), seed2);
    }
}
