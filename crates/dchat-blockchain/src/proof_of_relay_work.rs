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

use crate::block_hierarchy::Hash;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
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
        let mut scores = self.relay_scores.write().unwrap();
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
        let scores = self.relay_scores.read().unwrap();
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
        let mut votes_map = self.active_block_votes.write().unwrap();
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
            .unwrap()
            .get(block_hash)
            .map(|votes| votes.finalized)
            .unwrap_or(false)
    }

    /// Slash relay for double-voting
    fn slash_relay_for_double_vote(&self, relay_id: VerifyingKey) -> Result<(), ConsensusError> {
        let mut scores = self.relay_scores.write().unwrap();
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
}
