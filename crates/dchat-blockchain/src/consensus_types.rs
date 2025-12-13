//! Consensus Types - Shared data structures for consensus modules
//!
//! This module contains the core data structures used across all consensus
//! implementations (PoRW, PoT, TSC, and hardened consensus). By centralizing
//! these types, we break circular dependencies between modules while ensuring
//! type consistency across the consensus layer.
//!
//! SECURITY: These types form the foundation of consensus security. Changes
//! require careful review for:
//! - Serialization compatibility (breaking changes = hard fork)
//! - Memory layout for signature verification
//! - Timing attack resistance in comparisons

use crate::block_hierarchy::Hash;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use thiserror::Error;

// ============================================================================
// Geographic Diversity Types
// ============================================================================

/// Geographic regions for diversity requirements
///
/// dchat requires geographic distribution to prevent:
/// - Jurisdiction-based attacks (all nodes in one legal system)
/// - Network partition attacks (single ISP/IX dependency)
/// - Latency manipulation (all nodes in one region)
///
/// SECURITY: Minimum 3 regions required for consensus quorum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeographicRegion {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Asia,
    Africa,
    Oceania,
}

impl GeographicRegion {
    /// All available regions
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

    /// Number of distinct regions (for diversity calculations)
    pub fn count() -> usize {
        6
    }

    /// Parse from string (for config files)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "northamerica" | "north_america" | "na" => Some(Self::NorthAmerica),
            "southamerica" | "south_america" | "sa" => Some(Self::SouthAmerica),
            "europe" | "eu" => Some(Self::Europe),
            "asia" | "as" => Some(Self::Asia),
            "africa" | "af" => Some(Self::Africa),
            "oceania" | "oc" | "australia" => Some(Self::Oceania),
            _ => None,
        }
    }

    /// Display name for logging/UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::NorthAmerica => "North America",
            Self::SouthAmerica => "South America",
            Self::Europe => "Europe",
            Self::Asia => "Asia",
            Self::Africa => "Africa",
            Self::Oceania => "Oceania",
        }
    }
}

impl Default for GeographicRegion {
    fn default() -> Self {
        Self::NorthAmerica
    }
}

impl std::fmt::Display for GeographicRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ============================================================================
// Relay Identity and Scoring
// ============================================================================

/// Relay score and reputation tracking
///
/// Each relay accumulates reputation through:
/// - Successful message delivery (verified by DeliveryProofs)
/// - Uptime and availability
/// - Stake lockup duration
///
/// SECURITY: Weight cap of 5% prevents any single relay from dominating consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayScore {
    /// Relay's Ed25519 public key (identity)
    pub relay_id: VerifyingKey,
    /// Staked DCHAT amount (lamports)
    pub stake_amount: u64,
    /// Stake unlock time (for time-weighted scoring)
    pub stake_locked_until: SystemTime,
    /// Total messages successfully delivered
    pub messages_delivered: u64,
    /// Uptime percentage (0.0 - 100.0)
    pub uptime_percentage: f64,
    /// Reputation score (0.0 - 1.0, normalized)
    pub reputation_score: f64,
    /// Geographic location
    pub geographic_region: GeographicRegion,
    /// Autonomous System Number (for network diversity)
    pub asn: u32,
    /// Hash of IP address (privacy-preserving network ID)
    pub ip_address_hash: Hash,
    /// When this relay first registered
    pub registration_time: SystemTime,
    /// Last seen activity
    pub last_active: SystemTime,
    /// Number of times slashed
    pub slashing_count: u32,
    /// Total amount slashed (lamports)
    pub total_slashed_amount: u64,
    /// Consecutive delivery failures (triggers suspension)
    pub consecutive_failures: u32,
    /// Verified delivery proofs submitted
    pub verified_delivery_proofs: u64,
    /// Invalid proof attempts (suspicious activity)
    pub invalid_proof_attempts: u32,
}

impl RelayScore {
    /// Calculate effective weight in basis points (capped at 500 = 5%)
    pub fn effective_weight_bps(&self, total_stake: u64) -> u64 {
        if total_stake == 0 {
            return 0;
        }

        // Base weight from stake
        let stake_weight = (self.stake_amount as f64 / total_stake as f64) * 10000.0;

        // Reputation multiplier (0.5x to 1.5x)
        let rep_multiplier = 0.5 + self.reputation_score;

        // Uptime multiplier (linear scaling)
        let uptime_multiplier = self.uptime_percentage / 100.0;

        // Combined weight
        let raw_weight = stake_weight * rep_multiplier * uptime_multiplier;

        // Cap at 5% (500 bps)
        (raw_weight as u64).min(500)
    }

    /// Check if relay is currently suspended
    pub fn is_suspended(&self) -> bool {
        self.consecutive_failures >= 10 || self.slashing_count >= 3
    }

    /// Check if relay is eligible for committee selection
    pub fn is_eligible(&self) -> bool {
        !self.is_suspended()
            && self.uptime_percentage >= 95.0
            && self.reputation_score >= 0.5
            && self.stake_amount >= 10_000_000_000 // 10,000 DCHAT minimum
    }
}

// ============================================================================
// Delivery Proofs
// ============================================================================

/// Cryptographic proof of message delivery
///
/// Each relay signs a proof when they successfully route a message.
/// These proofs accumulate to give relays consensus weight.
///
/// SECURITY:
/// - Timestamp prevents replay attacks
/// - Route path prevents path manipulation
/// - Latency bounds prevent timing attacks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryProof {
    /// Hash of the delivered message
    pub message_hash: Hash,
    /// Relay that created this proof
    pub relay_id: VerifyingKey,
    /// When the message was delivered
    pub timestamp: SystemTime,
    /// Full routing path (for verification)
    pub route_path: Vec<VerifyingKey>,
    /// End-to-end latency in milliseconds
    pub latency_ms: u64,
    /// Relay's signature over the proof
    pub signature: Signature,
}

impl DeliveryProof {
    /// Verify the signature on this proof
    pub fn verify(&self) -> bool {
        use ed25519_dalek::Verifier;

        // Reconstruct signed message
        let mut signed_data = Vec::new();
        signed_data.extend_from_slice(self.message_hash.as_bytes());
        signed_data.extend_from_slice(self.relay_id.as_bytes());

        // Add timestamp as bytes
        if let Ok(duration) = self.timestamp.duration_since(SystemTime::UNIX_EPOCH) {
            signed_data.extend_from_slice(&duration.as_secs().to_le_bytes());
        }

        // Add route path
        for pk in &self.route_path {
            signed_data.extend_from_slice(pk.as_bytes());
        }

        signed_data.extend_from_slice(&self.latency_ms.to_le_bytes());

        // Verify signature
        self.relay_id.verify(&signed_data, &self.signature).is_ok()
    }

    /// Check if proof is within acceptable time bounds
    pub fn is_fresh(&self, max_age_secs: u64) -> bool {
        if let Ok(age) = SystemTime::now().duration_since(self.timestamp) {
            age.as_secs() <= max_age_secs
        } else {
            false // Future timestamp = invalid
        }
    }

    /// Check if latency is within acceptable bounds
    pub fn latency_valid(&self) -> bool {
        // Minimum 5ms (speed of light constraint)
        // Maximum 30 seconds (network timeout)
        self.latency_ms >= 5 && self.latency_ms <= 30_000
    }
}

// ============================================================================
// Block Voting
// ============================================================================

/// Individual relay vote on a block
///
/// Relays vote on blocks by providing delivery proofs that demonstrate
/// their participation in the network. Vote weight is proportional to
/// stake and reputation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayVote {
    /// Voting relay's public key
    pub relay_id: VerifyingKey,
    /// Block being voted on
    pub block_hash: Hash,
    /// Vote weight (calculated from stake + reputation)
    pub vote_weight: f64,
    /// Delivery proofs backing this vote
    pub delivery_proofs: Vec<DeliveryProof>,
    /// Vote timestamp
    pub timestamp: SystemTime,
    /// Signature over vote
    pub signature: Signature,
}

impl RelayVote {
    /// Verify vote signature
    pub fn verify_signature(&self) -> bool {
        use ed25519_dalek::Verifier;

        let mut signed_data = Vec::new();
        signed_data.extend_from_slice(self.block_hash.as_bytes());
        signed_data.extend_from_slice(&self.vote_weight.to_le_bytes());

        if let Ok(duration) = self.timestamp.duration_since(SystemTime::UNIX_EPOCH) {
            signed_data.extend_from_slice(&duration.as_secs().to_le_bytes());
        }

        self.relay_id.verify(&signed_data, &self.signature).is_ok()
    }
}

/// Collected votes for a block
///
/// Aggregates all relay votes for a specific block and tracks
/// progress toward finality thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockVotes {
    /// Block being voted on
    pub block_hash: Hash,
    /// All collected votes
    pub votes: Vec<RelayVote>,
    /// Total weight of all votes (0.0 - 1.0)
    pub total_weight: f64,
    /// Weight by region (for geographic diversity check)
    pub geographic_representation: HashMap<GeographicRegion, f64>,
    /// Whether block has achieved finality
    pub finalized: bool,
}

impl BlockVotes {
    /// Create new empty vote collection
    pub fn new(block_hash: Hash) -> Self {
        Self {
            block_hash,
            votes: Vec::new(),
            total_weight: 0.0,
            geographic_representation: HashMap::new(),
            finalized: false,
        }
    }

    /// Add a vote to the collection
    pub fn add_vote(&mut self, vote: RelayVote, region: GeographicRegion) {
        self.total_weight += vote.vote_weight;
        *self.geographic_representation.entry(region).or_insert(0.0) += vote.vote_weight;
        self.votes.push(vote);
    }

    /// Check if quorum is reached (67% weight + 3 regions)
    pub fn check_quorum(&self, threshold: f64, min_regions: usize) -> bool {
        self.total_weight >= threshold && self.geographic_representation.len() >= min_regions
    }

    /// Number of unique voting relays
    pub fn voter_count(&self) -> usize {
        self.votes.len()
    }

    /// Number of regions represented
    pub fn region_count(&self) -> usize {
        self.geographic_representation.len()
    }
}

impl Default for BlockVotes {
    fn default() -> Self {
        Self {
            block_hash: Hash::from([0u8; 32]),
            votes: Vec::new(),
            total_weight: 0.0,
            geographic_representation: HashMap::new(),
            finalized: false,
        }
    }
}

// ============================================================================
// Consensus Errors
// ============================================================================

/// Consensus errors
///
/// All possible error conditions during consensus operations.
/// These are used for:
/// - Rejecting invalid proofs/votes
/// - Triggering slashing for malicious behavior
/// - Debugging consensus failures
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

    #[error("Duplicate timestamp in path")]
    DuplicateTimestamp,

    #[error("Backwards timestamp in path")]
    BackwardsTimestamp,

    #[error("Invalid relay signature")]
    InvalidRelaySignature,

    #[error("Relay not registered or suspended")]
    RelayNotRegistered,

    #[error("Relay slashed")]
    RelaySlashed,

    #[error("Insufficient stake")]
    InsufficientStake,

    #[error("Block not found: {:?}", .0)]
    BlockNotFound(Hash),

    #[error("Vote for unknown block")]
    UnknownBlock,

    #[error("Duplicate vote detected")]
    DuplicateVote,

    #[error("Conflicting vote detected (double voting)")]
    ConflictingVote,

    #[error("Geographic diversity not met (need {0} regions, have {1})")]
    GeographicDiversityNotMet(usize, usize),

    #[error("Weight cap exceeded")]
    WeightCapExceeded,

    #[error("Finality already achieved")]
    AlreadyFinalized,

    #[error("Quorum not reached")]
    QuorumNotReached,

    #[error("Invalid proof format")]
    InvalidProofFormat,

    #[error("Proof verification failed")]
    ProofVerificationFailed,

    #[error("Committee verification failed")]
    CommitteeVerificationFailed,

    #[error("Epoch mismatch")]
    EpochMismatch,

    #[error("Internal error: {0}")]
    Internal(String),
}

// ============================================================================
// Consensus Configuration
// ============================================================================

/// Constants for consensus parameters
pub mod constants {
    /// Proof-of-Relay-Work quorum threshold (basis points)
    /// 6667 = 66.67% (supermajority)
    pub const PORW_QUORUM_BPS: u64 = 6667;

    /// Temporal Stake Consensus quorum threshold (basis points)
    /// 5100 = 51% (simple majority)
    pub const TSC_QUORUM_BPS: u64 = 5100;

    /// Maximum weight any single relay can have (basis points)
    /// 500 = 5%
    pub const MAX_RELAY_WEIGHT_BPS: u64 = 500;

    /// Minimum required geographic regions for quorum
    pub const MIN_GEOGRAPHIC_REGIONS: usize = 3;

    /// Maximum routing path length
    pub const MAX_ROUTING_PATH_LENGTH: usize = 10;

    /// Maximum proof age in seconds
    pub const MAX_PROOF_AGE_SECS: u64 = 30;

    /// Blocks per epoch
    pub const EPOCH_LENGTH_BLOCKS: u64 = 1800;

    /// Maximum signatures per batch verification
    pub const MAX_BATCH_SIZE: usize = 64;

    /// Minimum latency in milliseconds (speed of light constraint)
    pub const MIN_LATENCY_MS: u64 = 5;

    /// Maximum latency in milliseconds
    pub const MAX_LATENCY_MS: u64 = 30_000;

    /// Minimum stake for relay eligibility (lamports)
    pub const MIN_RELAY_STAKE: u64 = 10_000_000_000; // 10,000 DCHAT

    /// Maximum consecutive failures before suspension
    pub const MAX_CONSECUTIVE_FAILURES: u32 = 10;

    /// Maximum slashing events before permanent ban
    pub const MAX_SLASHING_COUNT: u32 = 3;
}

// ============================================================================
// Type Aliases for Convenience
// ============================================================================

/// Relay identifier (32-byte public key hash)
pub type RelayId = [u8; 32];

/// Block height
pub type BlockHeight = u64;

/// Epoch number
pub type Epoch = u64;

/// Weight in basis points (0-10000)
pub type WeightBps = u64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geographic_region_all() {
        let regions = GeographicRegion::all();
        assert_eq!(regions.len(), 6);
        assert_eq!(GeographicRegion::count(), 6);
    }

    #[test]
    fn test_geographic_region_from_str() {
        assert_eq!(
            GeographicRegion::from_str("europe"),
            Some(GeographicRegion::Europe)
        );
        assert_eq!(
            GeographicRegion::from_str("EU"),
            Some(GeographicRegion::Europe)
        );
        assert_eq!(GeographicRegion::from_str("invalid"), None);
    }

    #[test]
    fn test_relay_score_weight_cap() {
        let mut score = RelayScore {
            relay_id: VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            stake_amount: 1_000_000_000_000, // 1 trillion
            stake_locked_until: SystemTime::now(),
            messages_delivered: 1000,
            uptime_percentage: 100.0,
            reputation_score: 1.0,
            geographic_region: GeographicRegion::Europe,
            asn: 12345,
            ip_address_hash: Hash::from([0u8; 32]),
            registration_time: SystemTime::now(),
            last_active: SystemTime::now(),
            slashing_count: 0,
            total_slashed_amount: 0,
            consecutive_failures: 0,
            verified_delivery_proofs: 100,
            invalid_proof_attempts: 0,
        };

        // Even with 100% stake, weight should be capped at 500 bps (5%)
        let weight = score.effective_weight_bps(1_000_000_000_000);
        assert!(weight <= 500, "Weight cap violated: {} > 500", weight);
    }

    #[test]
    fn test_block_votes_quorum() {
        let mut votes = BlockVotes::new(Hash::from([0u8; 32]));

        // Not enough weight
        assert!(!votes.check_quorum(0.67, 3));

        // Add weight but not enough regions
        votes.total_weight = 0.7;
        votes
            .geographic_representation
            .insert(GeographicRegion::Europe, 0.4);
        votes
            .geographic_representation
            .insert(GeographicRegion::Asia, 0.3);
        assert!(!votes.check_quorum(0.67, 3));

        // Add third region - now should pass
        votes
            .geographic_representation
            .insert(GeographicRegion::NorthAmerica, 0.0);
        assert!(votes.check_quorum(0.67, 3));
    }

    #[test]
    fn test_delivery_proof_latency_bounds() {
        let proof = DeliveryProof {
            message_hash: Hash::from([0u8; 32]),
            relay_id: VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            timestamp: SystemTime::now(),
            route_path: vec![],
            latency_ms: 100,
            signature: Signature::from_bytes(&[0u8; 64]),
        };

        assert!(proof.latency_valid());

        // Too fast (speed of light violation)
        let fast_proof = DeliveryProof {
            latency_ms: 1,
            ..proof.clone()
        };
        assert!(!fast_proof.latency_valid());

        // Too slow
        let slow_proof = DeliveryProof {
            latency_ms: 60_000,
            ..proof
        };
        assert!(!slow_proof.latency_valid());
    }
}
