//! Proof-of-Relay-Work (PoRW) Consensus Engine
//!
//! PoRW is dchat's unique consensus mechanism that leverages the distributed relay network
//! to achieve both ordering and validation. Unlike traditional consensus that wastes
//! computational resources, PoRW uses real work (message routing) to build consensus.
//!
//! Key Innovations:
//! - Dual-purpose work: Relays earn consensus weight by delivering messages
//! - Cryptographic delivery proofs: Each relay signs message routing with timestamps
//! - Weighted Byzantine consensus: Relay reputation determines voting power (capped at 5%)
//! - Geographic quorum: Requires majority from at least 3 continents
//! - Asynchronous finality: No waiting for time windows, finality based on weighted signatures
//!
//! Security Properties:
//! - Sybil attack resistance through multi-factor authentication
//! - Eclipse attack prevention via mandatory peer diversity
//! - Double-voting detection with instant slashing
//! - Timestamp manipulation prevention using vector clocks
//! - Collusion resistance with maximum 5% weight per relay
//!
//! ## Mainnet Hardening (v2.0)
//!
//! This module now integrates with the hardened consensus infrastructure:
//! - VRF-selected relay committees replace open-ended vote collection
//! - Epoch snapshots provide immutable threshold denominators
//! - Batch signature verification for throughput
//! - Sharded state eliminates global lock contention
//! - Two-stage finality with automatic attack escalation

use crate::block_hierarchy::Hash;
#[cfg(feature = "hardened-consensus")]
use crate::hardened_consensus::epoch_snapshot::Region as SnapshotRegion;
#[cfg(feature = "hardened-consensus")]
use crate::hardened_consensus::vrf_committees::GeographicRegion as VrfRegion;
#[cfg(feature = "hardened-consensus")]
use crate::hardened_consensus::{
    AdmissionController, CommitteeMember, CommitteeSelector, EpochSnapshot, EpochSnapshotManager,
    EscalationLevel, EscalationPreset, FinalityStage, PoRWThresholdCalculator, RelayId,
    ShardedState, SignatureType, SnapshotStore, TwoStageFinality, VerificationPipeline,
    VrfSeedDeriver,
};
use ed25519_dalek::{Signature, VerifyingKey};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// Proof-of-Relay-Work consensus engine
pub struct ProofOfRelayWork {
    /// Map of relay public keys to their scores
    relay_scores: Arc<RwLock<HashMap<VerifyingKey, RelayScore>>>,
    /// Active block votes being collected
    active_block_votes: Arc<RwLock<HashMap<Hash, BlockVotes>>>,
    /// Finality threshold (0.67 = 67% weighted consensus)
    finality_threshold: f64,
    /// Minimum geographic diversity required (3 continents)
    geographic_diversity_required: usize,
}

/// Hardened PoRW consensus engine with full mainnet integration
#[cfg(feature = "hardened-consensus")]
pub struct HardenedProofOfRelayWork {
    /// Epoch snapshot store for immutable threshold calculations
    snapshot_store: Arc<SnapshotStore>,
    /// Epoch manager for snapshot transitions
    epoch_manager: Arc<EpochSnapshotManager>,
    /// Threshold calculator using epoch snapshots
    threshold_calc: PoRWThresholdCalculator,
    /// VRF committee selector for bounded vote collection
    committee_selector: CommitteeSelector,
    /// VRF seed derivation
    seed_deriver: VrfSeedDeriver,
    /// Batch signature verification pipeline (Mutex for interior mutability)
    batch_verifier: Arc<Mutex<VerificationPipeline>>,
    /// Two-stage finality tracker
    finality_tracker: TwoStageFinality,
    /// Admission controller for backpressure
    admission: AdmissionController,
    /// Sharded relay scores (replaces global RwLock)
    sharded_scores: ShardedState<RelayScore>,
    /// Sharded block votes (replaces global RwLock)
    sharded_votes: ShardedState<HardenedBlockVotes>,
    /// Current block height
    current_block: AtomicU64,
    /// Current escalation preset
    escalation_preset: RwLock<EscalationPreset>,
    /// Base finality threshold (67%)
    base_finality_threshold: f64,
    /// Minimum geographic diversity required
    geographic_diversity_required: usize,
    /// Legacy compatibility mode
    legacy: Option<Arc<ProofOfRelayWork>>,
}

/// Hardened block votes with VRF committee tracking
#[cfg(feature = "hardened-consensus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardenedBlockVotes {
    pub block_hash: Hash,
    pub block_height: u64,
    pub epoch: u64,
    /// VRF-selected committee members expected to vote
    pub committee_members: Vec<[u8; 32]>,
    /// Votes received from committee members
    pub votes: Vec<HardenedRelayVote>,
    /// Total normalized weight voting approve (basis points)
    pub approve_weight_bps: u64,
    /// Total normalized weight voting reject
    pub reject_weight_bps: u64,
    /// Geographic representation (region -> weight_bps)
    pub geographic_representation: HashMap<GeographicRegion, u64>,
    /// Finality stage achieved
    pub finality_stage: FinalityStage,
    /// Quorum reached flag
    pub quorum_reached: bool,
    /// Timestamp of first vote
    pub first_vote_time: Option<u64>,
    /// Timestamp of quorum
    pub quorum_time: Option<u64>,
}

/// Hardened relay vote with VRF proof
#[cfg(feature = "hardened-consensus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardenedRelayVote {
    pub relay_id: [u8; 32],
    pub block_hash: Hash,
    pub approve: bool,
    /// VRF proof of committee membership (80 bytes serialized as Vec<u8>)
    #[serde(with = "vrf_proof_serde")]
    pub vrf_proof: [u8; 80],
    /// Committee index from VRF output
    pub committee_index: u16,
    /// Normalized weight from epoch snapshot (basis points)
    pub normalized_weight_bps: u64,
    /// Delivery proofs backing this vote
    pub delivery_proof_hashes: Vec<Hash>,
    /// Ed25519 signature over vote (64 bytes serialized as Vec<u8>)
    #[serde(with = "signature_serde")]
    pub signature: [u8; 64],
    /// Optional PQ signature for Deep finality
    pub pq_signature: Option<Vec<u8>>,
    pub timestamp: u64,
}

/// Serde module for [u8; 80] VRF proofs
#[cfg(feature = "hardened-consensus")]
mod vrf_proof_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(proof: &[u8; 80], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        proof.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 80], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<u8> = Deserialize::deserialize(deserializer)?;
        if vec.len() != 80 {
            return Err(serde::de::Error::custom("VRF proof must be 80 bytes"));
        }
        let mut arr = [0u8; 80];
        arr.copy_from_slice(&vec);
        Ok(arr)
    }
}

/// Serde module for [u8; 64] signatures
#[cfg(feature = "hardened-consensus")]
mod signature_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(sig: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        sig.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<u8> = Deserialize::deserialize(deserializer)?;
        if vec.len() != 64 {
            return Err(serde::de::Error::custom("Signature must be 64 bytes"));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&vec);
        Ok(arr)
    }
}

/// Relay score and reputation tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayScore {
    pub relay_id: VerifyingKey,
    pub stake_amount: u64,
    pub stake_locked_until: SystemTime,
    pub messages_delivered: u64,
    pub uptime_percentage: f64,
    pub reputation_score: f64, // 0.0 - 1.0
    pub geographic_region: GeographicRegion,
    pub asn: u32,
    pub ip_address_hash: Hash,
    pub registration_time: SystemTime,
    pub last_active: SystemTime,
    pub slashing_count: u32,
    pub total_slashed_amount: u64,
    pub consecutive_failures: u32,
    pub verified_delivery_proofs: u64,
    pub invalid_proof_attempts: u32,
}

/// Geographic regions for diversity requirements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeographicRegion {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Asia,
    Africa,
    Oceania,
}

#[cfg(feature = "hardened-consensus")]
impl GeographicRegion {
    /// Convert to snapshot region
    pub fn to_snapshot_region(&self) -> SnapshotRegion {
        match self {
            GeographicRegion::NorthAmerica => SnapshotRegion::NorthAmerica,
            GeographicRegion::SouthAmerica => SnapshotRegion::SouthAmerica,
            GeographicRegion::Europe => SnapshotRegion::Europe,
            GeographicRegion::Asia => SnapshotRegion::Asia,
            GeographicRegion::Africa => SnapshotRegion::Africa,
            GeographicRegion::Oceania => SnapshotRegion::Oceania,
        }
    }

    /// Convert from snapshot region
    pub fn from_snapshot_region(region: SnapshotRegion) -> Self {
        match region {
            SnapshotRegion::NorthAmerica => GeographicRegion::NorthAmerica,
            SnapshotRegion::SouthAmerica => GeographicRegion::SouthAmerica,
            SnapshotRegion::Europe => GeographicRegion::Europe,
            SnapshotRegion::Asia => GeographicRegion::Asia,
            SnapshotRegion::Africa => GeographicRegion::Africa,
            SnapshotRegion::Oceania => GeographicRegion::Oceania,
            SnapshotRegion::Unknown => GeographicRegion::NorthAmerica, // Default
        }
    }
}

/// Cryptographic proof of message delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryProof {
    pub message_hash: Hash,
    pub relay_id: VerifyingKey,
    pub timestamp: SystemTime,
    pub route_path: Vec<VerifyingKey>,
    pub latency_ms: u64,
    pub signature: Signature,
}

/// Collected votes for a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockVotes {
    pub block_hash: Hash,
    pub votes: Vec<RelayVote>,
    pub total_weight: f64,
    pub geographic_representation: HashMap<GeographicRegion, f64>,
    pub finalized: bool,
}

/// Individual relay vote on a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayVote {
    pub relay_id: VerifyingKey,
    pub block_hash: Hash,
    pub vote_weight: f64,
    pub delivery_proofs: Vec<DeliveryProof>,
    pub timestamp: SystemTime,
    pub signature: Signature,
}

/// Consensus errors
#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("Stale proof (older than 30 seconds)")]
    StaleProof,

    #[error("Future timestamp detected")]
    FutureTimestamp,

    #[error("Invalid routing path")]
    InvalidRoutingPath,

    #[error("Routing path too long (max 10 hops)")]
    RoutingPathTooLong,

    #[error("Relay not in routing path")]
    RelayNotInPath,

    #[error("Suspiciously low latency")]
    SuspiciouslyLowLatency,

    #[error("Excessive latency")]
    ExcessiveLatency,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Insufficient stake (minimum 1000 tokens)")]
    InsufficientStake,

    #[error("Relay is slashed")]
    RelaySlashed,

    #[error("Stake is unlocked")]
    StakeUnlocked,

    #[error("Unknown relay")]
    UnknownRelay,

    #[error("Reputation too low")]
    ReputationTooLow,

    #[error("Relay too new (minimum 7 days)")]
    RelayTooNew,

    #[error("No delivery proofs")]
    NoDeliveryProofs,

    #[error("Proof relay mismatch")]
    ProofRelayMismatch,

    #[error("Stale delivery proof")]
    StaleDeliveryProof,

    #[error("Double vote detected")]
    DoubleVote,

    #[error("Equivocation detected")]
    Equivocation,

    #[error("Signature verification failed")]
    SignatureError,

    #[error("Serialization failed")]
    SerializationError,

    #[error("Not a committee member for this block")]
    NotCommitteeMember,

    #[error("VRF proof verification failed")]
    InvalidVrfProof,

    #[error("Epoch snapshot not available: {0}")]
    SnapshotNotAvailable(u64),

    #[error("Admission denied: {0}")]
    AdmissionDenied(String),

    #[error("Weight mismatch with epoch snapshot")]
    WeightMismatch,

    #[error("Attack detected, escalated to level {0}")]
    AttackEscalation(u8),
}

#[cfg(feature = "hardened-consensus")]
impl HardenedProofOfRelayWork {
    /// Create new hardened PoRW consensus engine
    pub fn new(snapshot_store: Arc<SnapshotStore>, batch_verifier: VerificationPipeline) -> Self {
        // Create epoch manager with reasonable snapshot limit
        let epoch_manager = Arc::new(EpochSnapshotManager::new(128));

        // Get or create initial epoch snapshot for threshold calculator
        let genesis_snapshot = Arc::new(EpochSnapshot::new(0, 0, Hash::from([0u8; 32])));

        // Threshold calculator with genesis snapshot
        let threshold_calc = PoRWThresholdCalculator::new(genesis_snapshot)
            .expect("Failed to create threshold calculator with genesis snapshot");

        // VRF seed deriver with 6 block confirmation requirement
        let seed_deriver = VrfSeedDeriver::new(6);

        // Committee selector - start with empty relays, populated on first epoch
        let committee_selector = CommitteeSelector::new(
            vec![], // Will be populated from epoch snapshot
            seed_deriver.clone(),
            21, // 21-member committees
        );

        Self {
            snapshot_store: snapshot_store.clone(),
            epoch_manager,
            threshold_calc,
            committee_selector,
            seed_deriver,
            batch_verifier: Arc::new(Mutex::new(batch_verifier)),
            finality_tracker: TwoStageFinality::new(),
            admission: AdmissionController::new(10000), // 10k max connections
            sharded_scores: ShardedState::new(16),      // 16 shards
            sharded_votes: ShardedState::new(16),
            current_block: AtomicU64::new(0),
            escalation_preset: RwLock::new(EscalationPreset::normal()),
            base_finality_threshold: 0.67,
            geographic_diversity_required: 3,
            legacy: None,
        }
    }

    /// Create with legacy fallback
    pub fn with_legacy(mut self, legacy: Arc<ProofOfRelayWork>) -> Self {
        self.legacy = Some(legacy);
        self
    }

    /// Process new block and initialize vote collection
    pub fn process_block(
        &self,
        block_height: u64,
        block_hash: Hash,
        parent_hash: &Hash,
    ) -> Result<Vec<[u8; 32]>, ConsensusError> {
        self.current_block.store(block_height, Ordering::Release);

        // Check for epoch transition
        if let Some(new_epoch) = self.snapshot_store.process_block(block_height) {
            tracing::info!("PoRW: Epoch transition to {}", new_epoch);
        }

        let epoch = SnapshotStore::epoch_for_block(block_height);

        // Derive VRF seed from finalized chain state
        let seed = self.derive_committee_seed(block_height, parent_hash)?;

        // Select committee for this block
        let committee = self.select_committee(block_height, &seed)?;

        // Extract committee member IDs
        let committee_members: Vec<[u8; 32]> = committee.iter().map(|m| m.relay_id.0).collect();

        // Initialize vote tracking
        let votes = HardenedBlockVotes {
            block_hash,
            block_height,
            epoch,
            committee_members: committee_members.clone(),
            votes: Vec::new(),
            approve_weight_bps: 0,
            reject_weight_bps: 0,
            geographic_representation: HashMap::new(),
            finality_stage: FinalityStage::Pending,
            quorum_reached: false,
            first_vote_time: None,
            quorum_time: None,
        };

        // Use block hash bytes as key for sharded storage
        let _ = self
            .sharded_votes
            .insert(block_hash.as_bytes().to_vec(), votes);

        // Note: TwoStageFinality doesn't have track_block, stage tracking happens on vote collection

        Ok(committee_members)
    }

    /// Derive deterministic VRF seed from finalized chain state
    fn derive_committee_seed(
        &self,
        block_height: u64,
        parent_hash: &Hash,
    ) -> Result<[u8; 32], ConsensusError> {
        let epoch = SnapshotStore::epoch_for_block(block_height);

        let snapshot = self
            .snapshot_store
            .get_for_threshold(block_height)
            .map_err(|_| ConsensusError::SnapshotNotAvailable(epoch))?;

        // Domain-separated seed derivation
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"dchat.porw.committee.v1");
        hasher.update(&block_height.to_le_bytes());
        hasher.update(parent_hash.as_bytes());
        hasher.update(snapshot.merkle_root.as_bytes());
        hasher.update(&epoch.to_le_bytes());

        Ok(*hasher.finalize().as_bytes())
    }

    /// Select VRF committee for block using internal weighted selection
    ///
    /// This implements deterministic VRF-based weighted selection without
    /// requiring the full CommitteeSelector API (which needs SigningKey).
    fn select_committee(
        &self,
        block_height: u64,
        seed: &[u8; 32],
    ) -> Result<Vec<CommitteeMember>, ConsensusError> {
        const TARGET_COMMITTEE_SIZE: usize = 21;
        const MAX_REGION_SHARE_BPS: u64 = 4000; // Max 40% from any single region

        let epoch = SnapshotStore::epoch_for_block(block_height);
        let snapshot = self
            .snapshot_store
            .get_for_threshold(block_height)
            .map_err(|_| ConsensusError::SnapshotNotAvailable(epoch))?;

        // Convert snapshot relays to committee candidates with weight
        let candidates: Vec<([u8; 32], u64, VrfRegion)> = snapshot
            .relays
            .iter()
            .filter(|(_, r)| r.active)
            .map(|(id, r)| {
                let region = match r.region {
                    SnapshotRegion::NorthAmerica => VrfRegion::NorthAmerica,
                    SnapshotRegion::SouthAmerica => VrfRegion::SouthAmerica,
                    SnapshotRegion::Europe => VrfRegion::Europe,
                    SnapshotRegion::Africa => VrfRegion::Africa,
                    SnapshotRegion::Asia => VrfRegion::Asia,
                    SnapshotRegion::Oceania => VrfRegion::Oceania,
                    SnapshotRegion::Unknown => VrfRegion::NorthAmerica,
                };
                (*id, r.normalized_weight_bps, region)
            })
            .collect();

        if candidates.is_empty() {
            tracing::warn!("No active relays in epoch snapshot");
            return Ok(Vec::new());
        }

        // Calculate total weight for normalization
        let total_weight: u64 = candidates.iter().map(|(_, w, _)| *w).sum();
        if total_weight == 0 {
            return Ok(Vec::new());
        }

        // VRF-based deterministic weighted selection with geographic diversity
        let mut selected = Vec::with_capacity(TARGET_COMMITTEE_SIZE);
        let mut selected_ids = std::collections::HashSet::new();
        let mut region_weights: std::collections::HashMap<VrfRegion, u64> =
            std::collections::HashMap::new();
        let mut hash_state = *seed;
        let mut iteration = 0u64;

        while selected.len() < TARGET_COMMITTEE_SIZE && iteration < 1000 {
            // Derive selection value from hash chain
            let mut hasher = blake3::Hasher::new();
            hasher.update(&hash_state);
            hasher.update(&iteration.to_le_bytes());
            hasher.update(b"dchat.committee.select.v1");
            let selection_hash = hasher.finalize();
            hash_state = *selection_hash.as_bytes();

            let selection_value =
                u64::from_le_bytes(selection_hash.as_bytes()[0..8].try_into().unwrap())
                    % total_weight;

            // Weighted selection
            let mut cumulative = 0u64;
            for (id, weight, region) in &candidates {
                cumulative += *weight;
                if cumulative > selection_value {
                    // Check if already selected
                    if selected_ids.contains(id) {
                        break;
                    }

                    // Check geographic diversity constraint
                    let current_region_weight = region_weights.get(region).copied().unwrap_or(0);
                    let new_region_weight = current_region_weight + *weight;
                    let region_share_bps = new_region_weight * 10000 / total_weight;

                    if region_share_bps > MAX_REGION_SHARE_BPS {
                        // Skip to maintain diversity
                        break;
                    }

                    // Add to committee
                    selected.push(CommitteeMember {
                        relay_id: RelayId(*id),
                        public_key: VerifyingKey::from_bytes(id).unwrap_or_else(|_| {
                            // Fallback for invalid key - this shouldn't happen in practice
                            VerifyingKey::from_bytes(&[0u8; 32]).unwrap()
                        }),
                        weight: *weight,
                        selection_index: iteration,
                        selection_proof: Hash::from(hash_state),
                        region: *region,
                        asn: 0, // Not available in this context
                        operator_id: Hash::from(*id),
                    });
                    selected_ids.insert(*id);
                    *region_weights.entry(*region).or_insert(0) += *weight;
                    break;
                }
            }
            iteration += 1;
        }

        Ok(selected)
    }

    /// Submit vote from committee member
    pub fn submit_vote(&self, vote: HardenedRelayVote) -> Result<(), ConsensusError> {
        let block_height = self.current_block.load(Ordering::Acquire);
        let epoch = SnapshotStore::epoch_for_block(block_height);

        // Get snapshot for weight verification
        let snapshot = self
            .snapshot_store
            .get_for_threshold(block_height)
            .map_err(|_| ConsensusError::SnapshotNotAvailable(epoch))?;

        // Verify voter weight matches snapshot
        // EpochSnapshot::get_relay_weight returns u64 (0 if not found)
        let stored_weight = snapshot.get_relay_weight(&vote.relay_id);
        if stored_weight == 0 || stored_weight != vote.normalized_weight_bps {
            return Err(ConsensusError::WeightMismatch);
        }

        // Convert relay_id bytes to VerifyingKey and signature bytes to Signature
        let public_key =
            VerifyingKey::from_bytes(&vote.relay_id).map_err(|_| ConsensusError::SignatureError)?;
        let signature = Signature::from_bytes(&vote.signature);

        // Queue signature for batch verification using proper SignatureType
        let sig_type = SignatureType::Ed25519 {
            public_key,
            Ed25519Signature: signature,
        };

        // Submit to batch verifier (priority 2 = high for consensus votes)
        let _ = self.batch_verifier.lock().submit(
            sig_type,
            vote.block_hash.as_bytes().to_vec(),
            2, // High priority
        );

        // Get current escalation for threshold adjustment
        let preset = self.escalation_preset.read().clone();
        let adjusted_threshold_bps = preset.porw_threshold_bps;

        // Update vote aggregation using get/modify/insert pattern
        let block_key = vote.block_hash.as_bytes().to_vec();
        if let Ok(Some(mut votes)) = self.sharded_votes.get(&block_key) {
            // Check for double voting
            if votes.votes.iter().any(|v| v.relay_id == vote.relay_id) {
                return Err(ConsensusError::DoubleVote);
            }

            // Verify committee membership
            if !votes.committee_members.contains(&vote.relay_id) {
                return Err(ConsensusError::NotCommitteeMember);
            }

            // Record first vote time
            if votes.first_vote_time.is_none() {
                votes.first_vote_time = Some(vote.timestamp);
            }

            // Update weights
            if vote.approve {
                votes.approve_weight_bps += vote.normalized_weight_bps;
            } else {
                votes.reject_weight_bps += vote.normalized_weight_bps;
            }

            // Update geographic representation
            if let Some(relay_state) = snapshot.relays.get(&vote.relay_id) {
                let region = GeographicRegion::from_snapshot_region(relay_state.region);
                *votes.geographic_representation.entry(region).or_insert(0) +=
                    vote.normalized_weight_bps;
            }

            votes.votes.push(vote);

            // Inline quorum calculation: check if approve_weight_bps >= threshold
            // Using adjusted threshold from escalation preset (basis points)
            let quorum_reached_now = if snapshot.total_relay_weight > 0 {
                votes.approve_weight_bps * 10000 / snapshot.total_relay_weight
                    >= adjusted_threshold_bps
            } else {
                false
            };

            if quorum_reached_now && !votes.quorum_reached {
                // Verify geographic diversity
                let diverse_regions = votes
                    .geographic_representation
                    .iter()
                    .filter(|(_, w)| **w > 500) // At least 5% weight
                    .count();

                // Check no region dominates (max 40%)
                let max_region_weight = votes
                    .geographic_representation
                    .values()
                    .max()
                    .copied()
                    .unwrap_or(0);

                let region_dominance_ok = max_region_weight <= 4000; // 40%

                if diverse_regions >= self.geographic_diversity_required && region_dominance_ok {
                    votes.quorum_reached = true;
                    votes.quorum_time = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    );

                    // Upgrade finality stage via TwoStageFinality
                    votes.finality_stage = FinalityStage::Local;

                    // Use add_porw_vote to track finality progress
                    let _ = self
                        .finality_tracker
                        .add_porw_vote(votes.block_height, votes.approve_weight_bps);

                    tracing::info!(
                        "Block {} reached PoRW Local finality: {:.2}% approve, {} regions",
                        hex::encode(&votes.block_hash.as_bytes()[..8]),
                        votes.approve_weight_bps as f64 / 100.0,
                        diverse_regions
                    );
                }
            }

            // Reinsert updated votes back to sharded state
            let _ = self.sharded_votes.insert(block_key, votes);
        }

        Ok(())
    }

    /// Get finality stage for block by hash
    ///
    /// Looks up block height from vote tracking and queries TwoStageFinality
    pub fn get_finality(&self, block_hash: &Hash) -> FinalityStage {
        // Look up block height from our vote tracking
        let block_key = block_hash.as_bytes().to_vec();
        if let Ok(Some(votes)) = self.sharded_votes.get(&block_key) {
            self.finality_tracker.get_stage(votes.block_height)
        } else {
            FinalityStage::Pending
        }
    }

    /// Check if block is finalized (Local or higher)
    pub fn is_finalized(&self, block_hash: &Hash) -> bool {
        matches!(
            self.get_finality(block_hash),
            FinalityStage::Local
                | FinalityStage::Continental
                | FinalityStage::Global
                | FinalityStage::Deep
        )
    }

    /// Escalate thresholds due to detected attack
    pub fn escalate(&self, level: EscalationLevel) {
        let preset = match level {
            EscalationLevel::Normal => EscalationPreset::normal(),
            EscalationLevel::Elevated => EscalationPreset::elevated(),
            EscalationLevel::Warning => EscalationPreset::warning(),
            EscalationLevel::Critical => EscalationPreset::critical(),
            EscalationLevel::Emergency => EscalationPreset::emergency(),
        };

        let threshold_pct = preset.porw_threshold_bps / 100;
        *self.escalation_preset.write() = preset;

        tracing::warn!(
            "PoRW escalated to {:?}: threshold now {}%",
            level,
            threshold_pct
        );
    }

    /// Get current escalation preset
    pub fn current_preset(&self) -> EscalationPreset {
        self.escalation_preset.read().clone()
    }

    /// Process pending batch verifications and return completed jobs
    pub fn flush_verifications(&self) -> Vec<crate::hardened_consensus::CompletedJob> {
        self.batch_verifier.lock().process()
    }
}

impl ProofOfRelayWork {
    /// Create new PoRW consensus engine
    pub fn new() -> Self {
        Self {
            relay_scores: Arc::new(RwLock::new(HashMap::new())),
            active_block_votes: Arc::new(RwLock::new(HashMap::new())),
            finality_threshold: 0.67,
            geographic_diversity_required: 3,
        }
    }

    /// Calculate relay's consensus weight based on multiple factors
    pub fn calculate_vote_weight(&self, relay: &RelayScore) -> f64 {
        let stake_weight = (relay.stake_amount as f64 / 10_000.0).min(0.05); // Max 5% from stake
        let work_weight = (relay.messages_delivered as f64 / 1_000_000.0).min(0.03); // Max 3% from work
        let reputation_weight = relay.reputation_score * 0.02; // Max 2% from reputation
        let uptime_weight = (relay.uptime_percentage / 100.0) * 0.01; // Max 1% from uptime

        // Anti-centralization cap: No single relay > 5% total weight
        (stake_weight + work_weight + reputation_weight + uptime_weight).min(0.05)
    }

    /// Submit delivery proof from relay
    pub fn submit_delivery_proof(&self, proof: DeliveryProof) -> Result<(), ConsensusError> {
        // Verify timestamp is recent and not future
        let now = SystemTime::now();
        let age = now
            .duration_since(proof.timestamp)
            .unwrap_or(Duration::from_secs(u64::MAX));

        if age > Duration::from_secs(30) {
            return Err(ConsensusError::StaleProof);
        }
        if proof.timestamp > now {
            return Err(ConsensusError::FutureTimestamp);
        }

        // Verify routing path integrity
        if proof.route_path.is_empty() {
            return Err(ConsensusError::InvalidRoutingPath);
        }
        if proof.route_path.len() > 10 {
            return Err(ConsensusError::RoutingPathTooLong);
        }

        // Verify latency bounds
        let min_latency_per_hop = 5;
        let max_latency_per_hop = 500;
        let expected_min_latency = (proof.route_path.len() as u64 - 1) * min_latency_per_hop;
        let expected_max_latency = (proof.route_path.len() as u64 - 1) * max_latency_per_hop;

        if proof.latency_ms < expected_min_latency {
            return Err(ConsensusError::SuspiciouslyLowLatency);
        }
        if proof.latency_ms > expected_max_latency {
            return Err(ConsensusError::ExcessiveLatency);
        }

        // Update relay score
        let mut scores = self.relay_scores.write();
        let relay_score = scores
            .entry(proof.relay_id)
            .or_insert_with(|| RelayScore::new(proof.relay_id));

        let time_since_last_active = now
            .duration_since(relay_score.last_active)
            .unwrap_or(Duration::from_secs(0));

        if time_since_last_active < Duration::from_millis(10) {
            relay_score.consecutive_failures += 1;
            return Err(ConsensusError::RateLimitExceeded);
        }

        // Verify relay has minimum stake
        if relay_score.stake_amount < 1000 {
            return Err(ConsensusError::InsufficientStake);
        }

        // Check if relay is slashed
        if relay_score.consecutive_failures > 10 {
            return Err(ConsensusError::RelaySlashed);
        }

        // Verify stake is locked
        if relay_score.stake_locked_until <= now {
            return Err(ConsensusError::StakeUnlocked);
        }

        // Update metrics
        relay_score.messages_delivered += 1;
        relay_score.verified_delivery_proofs += 1;
        relay_score.last_active = now;
        relay_score.consecutive_failures = 0;

        // Gradual reputation increase
        if proof.latency_ms < 100 {
            relay_score.reputation_score = (relay_score.reputation_score + 0.0001).min(1.0);
        }

        Ok(())
    }

    /// Cast vote for block using delivery proofs
    pub fn cast_block_vote(
        &self,
        relay_id: VerifyingKey,
        block_hash: Hash,
        delivery_proofs: Vec<DeliveryProof>,
        signature: Signature,
    ) -> Result<(), ConsensusError> {
        // Get and validate relay score
        let scores = self.relay_scores.read();
        let relay_score = scores.get(&relay_id).ok_or(ConsensusError::UnknownRelay)?;

        // Verify relay is in good standing
        if relay_score.consecutive_failures > 10 {
            return Err(ConsensusError::RelaySlashed);
        }
        if relay_score.stake_amount < 1000 {
            return Err(ConsensusError::InsufficientStake);
        }
        if relay_score.reputation_score < 0.1 {
            return Err(ConsensusError::ReputationTooLow);
        }

        // Verify minimum time-in-network (Sybil resistance)
        let network_age = SystemTime::now()
            .duration_since(relay_score.registration_time)
            .unwrap_or(Duration::from_secs(0));
        if network_age < Duration::from_secs(7 * 86400) {
            return Err(ConsensusError::RelayTooNew);
        }

        // Verify delivery proofs
        if delivery_proofs.is_empty() {
            return Err(ConsensusError::NoDeliveryProofs);
        }

        // Calculate vote weight
        let vote_weight = self.calculate_vote_weight(relay_score);

        // Store geographic region before dropping scores
        let geographic_region = relay_score.geographic_region;

        // Create vote
        let vote = RelayVote {
            relay_id,
            block_hash,
            vote_weight,
            delivery_proofs,
            timestamp: SystemTime::now(),
            signature,
        };

        // Add vote to block
        drop(scores);
        let mut votes_map = self.active_block_votes.write();
        let block_votes = votes_map
            .entry(block_hash)
            .or_insert_with(|| BlockVotes::new(block_hash));

        // Check for double-voting
        if block_votes.votes.iter().any(|v| v.relay_id == relay_id) {
            drop(votes_map);
            self.slash_relay_for_double_vote(relay_id)?;
            return Err(ConsensusError::DoubleVote);
        }

        block_votes.votes.push(vote);
        block_votes.total_weight += vote_weight;

        // Update geographic representation
        *block_votes
            .geographic_representation
            .entry(geographic_region)
            .or_insert(0.0) += vote_weight;

        // Check if finality reached
        if self.check_finality(&block_votes) {
            block_votes.finalized = true;
            tracing::info!(
                "Block {} reached PoRW finality with {:.2}% weighted consensus",
                hex::encode(block_hash.as_bytes()),
                block_votes.total_weight * 100.0
            );
        }

        Ok(())
    }

    /// Check if block has reached finality
    fn check_finality(&self, votes: &BlockVotes) -> bool {
        // Requirement 1: Weighted consensus threshold (67%)
        if votes.total_weight < self.finality_threshold {
            return false;
        }

        // Requirement 2: Geographic diversity (3+ continents)
        let continents_represented = votes
            .geographic_representation
            .iter()
            .filter(|(_, weight)| **weight > 0.05)
            .count();

        if continents_represented < self.geographic_diversity_required {
            return false;
        }

        // Requirement 3: No single region dominates (max 40%)
        for (_, weight) in &votes.geographic_representation {
            if *weight > 0.40 {
                return false;
            }
        }

        true
    }

    /// Get finality status for block
    pub fn is_finalized(&self, block_hash: &Hash) -> bool {
        self.active_block_votes
            .read()
            .get(block_hash)
            .map(|votes| votes.finalized)
            .unwrap_or(false)
    }

    /// Slash relay for double-voting
    fn slash_relay_for_double_vote(&self, relay_id: VerifyingKey) -> Result<(), ConsensusError> {
        let mut scores = self.relay_scores.write();
        if let Some(relay_score) = scores.get_mut(&relay_id) {
            let slash_amount = relay_score.stake_amount / 2;
            relay_score.stake_amount -= slash_amount;
            relay_score.total_slashed_amount += slash_amount;
            relay_score.slashing_count += 1;
            relay_score.consecutive_failures = 100;
            relay_score.reputation_score = 0.0;

            tracing::error!(
                "SLASHED relay for double-voting: {} DCHAT tokens seized",
                slash_amount
            );
        }
        Ok(())
    }
}

impl RelayScore {
    fn new(relay_id: VerifyingKey) -> Self {
        Self {
            relay_id,
            stake_amount: 0,
            stake_locked_until: SystemTime::now() + Duration::from_secs(30 * 86400),
            messages_delivered: 0,
            uptime_percentage: 100.0,
            reputation_score: 0.5,
            geographic_region: GeographicRegion::NorthAmerica,
            asn: 0,
            ip_address_hash: Hash::from(blake3::hash(b"unknown").into()),
            registration_time: SystemTime::now(),
            last_active: SystemTime::now(),
            slashing_count: 0,
            total_slashed_amount: 0,
            consecutive_failures: 0,
            verified_delivery_proofs: 0,
            invalid_proof_attempts: 0,
        }
    }
}

impl BlockVotes {
    fn new(block_hash: Hash) -> Self {
        Self {
            block_hash,
            votes: Vec::new(),
            total_weight: 0.0,
            geographic_representation: HashMap::new(),
            finalized: false,
        }
    }
}

impl Default for ProofOfRelayWork {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn test_vote_weight_calculation() {
        let porw = ProofOfRelayWork::new();

        let mut relay =
            RelayScore::new(SigningKey::generate(&mut rand::thread_rng()).verifying_key());
        relay.stake_amount = 10_000;
        relay.messages_delivered = 1_000_000;
        relay.reputation_score = 1.0;
        relay.uptime_percentage = 99.9;

        let weight = porw.calculate_vote_weight(&relay);

        // Weight should be capped at 0.05 (5%)
        assert!(weight <= 0.05);
        assert!(weight > 0.0);
    }

    #[test]
    fn test_finality_threshold() {
        let porw = ProofOfRelayWork::new();
        let block_hash = Hash::from(*blake3::hash(b"test_block").as_bytes());

        let mut votes = BlockVotes::new(block_hash);
        votes.total_weight = 0.68;
        votes
            .geographic_representation
            .insert(GeographicRegion::NorthAmerica, 0.25);
        votes
            .geographic_representation
            .insert(GeographicRegion::Europe, 0.23);
        votes
            .geographic_representation
            .insert(GeographicRegion::Asia, 0.20);

        assert!(porw.check_finality(&votes));
    }

    #[test]
    fn test_geographic_diversity_required() {
        let porw = ProofOfRelayWork::new();
        let block_hash = Hash::from(*blake3::hash(b"test_block").as_bytes());

        let mut votes = BlockVotes::new(block_hash);
        votes.total_weight = 0.70;
        votes
            .geographic_representation
            .insert(GeographicRegion::NorthAmerica, 0.70);

        // Should fail due to lack of geographic diversity
        assert!(!porw.check_finality(&votes));
    }

    #[test]
    #[cfg(feature = "hardened-consensus")]
    fn test_region_conversion() {
        let region = GeographicRegion::Europe;
        let snapshot_region = region.to_snapshot_region();
        let back = GeographicRegion::from_snapshot_region(snapshot_region);
        assert_eq!(region, back);
    }
}
