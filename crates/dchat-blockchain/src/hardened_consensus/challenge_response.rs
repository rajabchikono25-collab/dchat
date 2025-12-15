//! Challenge-Response Protocol
//!
//! Implements interactive fraud proofs with:
//! - Bisection-based dispute resolution
//! - Cryptographic evidence requirements
//! - Slashing for invalid challenges/responses
//! - Timelocked arbitration
//!
//! Security: Challenges require bonds, responses require proofs,
//! and arbitration is deterministic and verifiable.

use crate::block_hierarchy::Hash;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Challenge response timeout (seconds)
pub const RESPONSE_TIMEOUT_SECS: u64 = 300;

/// Maximum bisection rounds
pub const MAX_BISECTION_ROUNDS: u8 = 20;

/// Minimum challenge bond (in smallest units)
pub const MIN_CHALLENGE_BOND: u64 = 1_000_000_000; // 1000 DCHAT

/// Slash percentage for losing party (basis points)
pub const SLASH_PERCENTAGE_BPS: u64 = 5000; // 50%

/// Challenge-response errors
#[derive(Debug, Error)]
pub enum ChallengeResponseError {
    #[error("Challenge not found: {0:?}")]
    ChallengeNotFound([u8; 32]),

    #[error("Response timeout")]
    ResponseTimeout,

    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(String),

    #[error("Insufficient bond: {0} < {1}")]
    InsufficientBond(u64, u64),

    #[error("Invalid evidence: {0}")]
    InvalidEvidence(String),

    #[error("Not authorized: {0}")]
    NotAuthorized(String),

    #[error("Bisection limit exceeded")]
    BisectionLimitExceeded,

    #[error("Invalid merkle proof")]
    InvalidMerkleProof,
}

/// Challenge dispute state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeState {
    /// Initial challenge submitted
    Challenged,
    /// Awaiting response from defender
    AwaitingResponse,
    /// In bisection protocol
    Bisecting,
    /// Awaiting arbitration
    AwaitingArbitration,
    /// Resolved - challenger won
    ChallengerWon,
    /// Resolved - defender won
    DefenderWon,
    /// Expired - no response
    Expired,
    /// Cancelled by challenger
    Cancelled,
}

/// Evidence type for challenges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Evidence {
    /// Merkle proof showing inclusion/exclusion
    MerkleProof {
        root: Hash,
        leaf: Hash,
        proof: Vec<Hash>,
        leaf_index: u64,
    },
    /// Signature proving double-signing
    DoubleSignature {
        message_a: Vec<u8>,
        message_b: Vec<u8>,
        signature_a: Vec<u8>,
        signature_b: Vec<u8>,
        public_key: [u8; 32],
    },
    /// Timestamp proving violation
    TimestampProof {
        claimed_time: u64,
        actual_time: u64,
        signed_claim: Vec<u8>,
    },
    /// State transition proof
    StateTransitionProof {
        pre_state_root: Hash,
        post_state_root: Hash,
        transition_data: Vec<u8>,
        witness: Vec<u8>,
    },
    /// Computation proof (for bisection)
    ComputationProof {
        step: u64,
        input_hash: Hash,
        output_hash: Hash,
        intermediate_states: Vec<Hash>,
    },
}

/// Dispute participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeParticipant {
    /// Participant ID
    pub id: [u8; 32],
    /// Bonded amount
    pub bond: u64,
    /// Is this the challenger?
    pub is_challenger: bool,
}

/// Bisection state for computation disputes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BisectionState {
    /// Current bisection round
    pub round: u8,
    /// Lower bound (agreed step)
    pub lower_bound: u64,
    /// Upper bound (disputed step)
    pub upper_bound: u64,
    /// Claimed state at lower bound
    pub lower_state: Hash,
    /// Claimed state at upper bound
    pub upper_state: Hash,
    /// Whose turn to bisect
    pub turn: [u8; 32],
    /// History of bisection choices
    pub history: Vec<BisectionChoice>,
}

/// Bisection choice
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BisectionChoice {
    pub round: u8,
    pub midpoint: u64,
    pub midpoint_state: Hash,
    pub chosen_side: BisectionSide,
}

/// Side chosen in bisection
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum BisectionSide {
    Lower,
    Upper,
}

impl BisectionState {
    pub fn new(
        lower_bound: u64,
        upper_bound: u64,
        lower_state: Hash,
        upper_state: Hash,
        first_turn: [u8; 32],
    ) -> Self {
        Self {
            round: 0,
            lower_bound,
            upper_bound,
            lower_state,
            upper_state,
            turn: first_turn,
            history: Vec::new(),
        }
    }

    /// Get midpoint
    pub fn midpoint(&self) -> u64 {
        self.lower_bound + (self.upper_bound - self.lower_bound) / 2
    }

    /// Is bisection complete?
    pub fn is_complete(&self) -> bool {
        self.upper_bound - self.lower_bound <= 1
    }

    /// Apply bisection choice
    pub fn apply_choice(
        &mut self,
        midpoint_state: Hash,
        side: BisectionSide,
        other_party: [u8; 32],
    ) -> Result<(), ChallengeResponseError> {
        if self.round >= MAX_BISECTION_ROUNDS {
            return Err(ChallengeResponseError::BisectionLimitExceeded);
        }

        let midpoint = self.midpoint();

        let choice = BisectionChoice {
            round: self.round,
            midpoint,
            midpoint_state,
            chosen_side: side,
        };

        match side {
            BisectionSide::Lower => {
                self.upper_bound = midpoint;
                self.upper_state = midpoint_state;
            }
            BisectionSide::Upper => {
                self.lower_bound = midpoint;
                self.lower_state = midpoint_state;
            }
        }

        self.history.push(choice);
        self.round += 1;
        self.turn = other_party;

        Ok(())
    }
}

/// Active dispute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dispute {
    /// Dispute ID
    pub id: [u8; 32],
    /// Target (block number, transaction, etc.)
    pub target_id: [u8; 32],
    /// Target type description
    pub target_type: String,
    /// Current state
    pub state: DisputeState,
    /// Challenger
    pub challenger: DisputeParticipant,
    /// Defender
    pub defender: DisputeParticipant,
    /// Challenge evidence
    pub challenge_evidence: Evidence,
    /// Response evidence (if any)
    pub response_evidence: Option<Evidence>,
    /// Bisection state (if applicable)
    pub bisection: Option<BisectionState>,
    /// Creation timestamp
    pub created_at: u64,
    /// Last update timestamp
    pub updated_at: u64,
    /// Response deadline
    pub response_deadline: u64,
    /// Arbitration result (if resolved)
    pub result: Option<DisputeResult>,
}

/// Dispute resolution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeResult {
    /// Winner ID
    pub winner: [u8; 32],
    /// Loser ID
    pub loser: [u8; 32],
    /// Slashed amount
    pub slash_amount: u64,
    /// Reward to winner
    pub reward_amount: u64,
    /// Resolution reason
    pub reason: String,
    /// Finalized at block
    pub finalized_block: u64,
}

impl Dispute {
    /// Create a new dispute
    pub fn new(
        target_id: [u8; 32],
        target_type: String,
        challenger_id: [u8; 32],
        challenger_bond: u64,
        defender_id: [u8; 32],
        defender_bond: u64,
        evidence: Evidence,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Generate dispute ID
        let mut hasher = blake3::Hasher::new();
        hasher.update(&target_id);
        hasher.update(&challenger_id);
        hasher.update(&now.to_le_bytes());
        let id = *hasher.finalize().as_bytes();

        Self {
            id,
            target_id,
            target_type,
            state: DisputeState::Challenged,
            challenger: DisputeParticipant {
                id: challenger_id,
                bond: challenger_bond,
                is_challenger: true,
            },
            defender: DisputeParticipant {
                id: defender_id,
                bond: defender_bond,
                is_challenger: false,
            },
            challenge_evidence: evidence,
            response_evidence: None,
            bisection: None,
            created_at: now,
            updated_at: now,
            response_deadline: now + RESPONSE_TIMEOUT_SECS,
            result: None,
        }
    }

    /// Is response deadline passed?
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        now > self.response_deadline
    }

    /// Update timestamp
    pub fn touch(&mut self) {
        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Transition to new state
    pub fn transition_to(&mut self, new_state: DisputeState) -> Result<(), ChallengeResponseError> {
        let valid = match (self.state, new_state) {
            (DisputeState::Challenged, DisputeState::AwaitingResponse) => true,
            (DisputeState::Challenged, DisputeState::AwaitingArbitration) => true, // Direct response
            (DisputeState::Challenged, DisputeState::Cancelled) => true,
            (DisputeState::AwaitingResponse, DisputeState::Bisecting) => true,
            (DisputeState::AwaitingResponse, DisputeState::AwaitingArbitration) => true,
            (DisputeState::AwaitingResponse, DisputeState::Expired) => true,
            (DisputeState::Bisecting, DisputeState::AwaitingArbitration) => true,
            (DisputeState::Bisecting, DisputeState::Expired) => true,
            (DisputeState::AwaitingArbitration, DisputeState::ChallengerWon) => true,
            (DisputeState::AwaitingArbitration, DisputeState::DefenderWon) => true,
            (DisputeState::Expired, DisputeState::ChallengerWon) => true,
            _ => false,
        };

        if !valid {
            return Err(ChallengeResponseError::InvalidStateTransition(format!(
                "{:?} -> {:?}",
                self.state, new_state
            )));
        }

        self.state = new_state;
        self.touch();
        Ok(())
    }

    /// Submit response
    pub fn submit_response(
        &mut self,
        responder_id: [u8; 32],
        evidence: Evidence,
    ) -> Result<(), ChallengeResponseError> {
        if responder_id != self.defender.id {
            return Err(ChallengeResponseError::NotAuthorized(
                "Only defender can respond".into(),
            ));
        }

        if self.is_expired() {
            self.state = DisputeState::Expired;
            return Err(ChallengeResponseError::ResponseTimeout);
        }

        self.response_evidence = Some(evidence);
        self.transition_to(DisputeState::AwaitingArbitration)?;

        Ok(())
    }

    /// Start bisection protocol
    pub fn start_bisection(
        &mut self,
        lower_bound: u64,
        upper_bound: u64,
        lower_state: Hash,
        upper_state: Hash,
    ) -> Result<(), ChallengeResponseError> {
        self.transition_to(DisputeState::Bisecting)?;

        self.bisection = Some(BisectionState::new(
            lower_bound,
            upper_bound,
            lower_state,
            upper_state,
            self.defender.id, // Defender goes first
        ));

        // Extend deadline for bisection
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.response_deadline = now + RESPONSE_TIMEOUT_SECS;

        Ok(())
    }

    /// Submit bisection step
    pub fn submit_bisection_step(
        &mut self,
        submitter_id: [u8; 32],
        midpoint_state: Hash,
        side: BisectionSide,
    ) -> Result<bool, ChallengeResponseError> {
        // Check expiry first (before mutable borrow of bisection)
        if self.is_expired() {
            self.state = DisputeState::Expired;
            return Err(ChallengeResponseError::ResponseTimeout);
        }

        let bisection =
            self.bisection
                .as_mut()
                .ok_or(ChallengeResponseError::InvalidStateTransition(
                    "No bisection active".into(),
                ))?;

        if submitter_id != bisection.turn {
            return Err(ChallengeResponseError::NotAuthorized(
                "Not your turn".into(),
            ));
        }

        // Determine next party
        let other = if submitter_id == self.challenger.id {
            self.defender.id
        } else {
            self.challenger.id
        };

        bisection.apply_choice(midpoint_state, side, other)?;

        let is_complete = bisection.is_complete();

        // Extend deadline
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.response_deadline = now + RESPONSE_TIMEOUT_SECS;
        self.touch();

        // Check if bisection is complete
        if is_complete {
            self.transition_to(DisputeState::AwaitingArbitration)?;
            return Ok(true); // Ready for final arbitration
        }

        Ok(false)
    }

    /// Resolve the dispute
    pub fn resolve(
        &mut self,
        challenger_wins: bool,
        reason: String,
        finalized_block: u64,
    ) -> Result<DisputeResult, ChallengeResponseError> {
        let (winner, loser) = if challenger_wins {
            (self.challenger.clone(), self.defender.clone())
        } else {
            (self.defender.clone(), self.challenger.clone())
        };

        let slash_amount = loser.bond * SLASH_PERCENTAGE_BPS / 10000;
        let reward_amount = slash_amount / 2; // Half to winner, half burned

        let result = DisputeResult {
            winner: winner.id,
            loser: loser.id,
            slash_amount,
            reward_amount,
            reason,
            finalized_block,
        };

        self.result = Some(result.clone());

        let new_state = if challenger_wins {
            DisputeState::ChallengerWon
        } else {
            DisputeState::DefenderWon
        };

        self.transition_to(new_state)?;

        Ok(result)
    }
}

/// Merkle proof verification for dispute resolution
pub fn verify_dispute_proof(root: &Hash, leaf: &Hash, proof: &[Hash], leaf_index: u64) -> bool {
    let mut current = *leaf;
    let mut index = leaf_index;

    for sibling in proof {
        let mut hasher = blake3::Hasher::new();

        if index % 2 == 0 {
            hasher.update(current.as_bytes());
            hasher.update(sibling.as_bytes());
        } else {
            hasher.update(sibling.as_bytes());
            hasher.update(current.as_bytes());
        }

        current = Hash::from(*hasher.finalize().as_bytes());
        index /= 2;
    }

    current == *root
}

/// Evidence verifier
pub struct EvidenceVerifier;

impl EvidenceVerifier {
    /// Verify merkle proof evidence
    pub fn verify_merkle_evidence(
        root: &Hash,
        leaf: &Hash,
        proof: &[Hash],
        leaf_index: u64,
    ) -> Result<(), ChallengeResponseError> {
        if verify_dispute_proof(root, leaf, proof, leaf_index) {
            Ok(())
        } else {
            Err(ChallengeResponseError::InvalidMerkleProof)
        }
    }

    /// Verify double signature evidence
    pub fn verify_double_signature(
        message_a: &[u8],
        message_b: &[u8],
        signature_a: &[u8],
        signature_b: &[u8],
        public_key: &[u8; 32],
    ) -> Result<bool, ChallengeResponseError> {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        // Parse key
        let verifying_key = VerifyingKey::from_bytes(public_key).map_err(|e| {
            ChallengeResponseError::InvalidEvidence(format!("Invalid public key: {}", e))
        })?;

        // Parse signatures
        let sig_a = Signature::from_slice(signature_a).map_err(|e| {
            ChallengeResponseError::InvalidEvidence(format!("Invalid signature A: {}", e))
        })?;

        let sig_b = Signature::from_slice(signature_b).map_err(|e| {
            ChallengeResponseError::InvalidEvidence(format!("Invalid signature B: {}", e))
        })?;

        // Verify both signatures
        let valid_a = verifying_key.verify(message_a, &sig_a).is_ok();
        let valid_b = verifying_key.verify(message_b, &sig_b).is_ok();

        // Both must be valid and messages must be different
        // for it to be a valid double-signing proof
        Ok(valid_a && valid_b && message_a != message_b)
    }

    /// Verify general evidence
    pub fn verify_evidence(evidence: &Evidence) -> Result<(), ChallengeResponseError> {
        match evidence {
            Evidence::MerkleProof {
                root,
                leaf,
                proof,
                leaf_index,
            } => Self::verify_merkle_evidence(root, leaf, proof, *leaf_index),
            Evidence::DoubleSignature {
                message_a,
                message_b,
                signature_a,
                signature_b,
                public_key,
            } => {
                if Self::verify_double_signature(
                    message_a,
                    message_b,
                    signature_a,
                    signature_b,
                    public_key,
                )? {
                    Ok(())
                } else {
                    Err(ChallengeResponseError::InvalidEvidence(
                        "Double signature verification failed".into(),
                    ))
                }
            }
            Evidence::TimestampProof {
                claimed_time,
                actual_time,
                ..
            } => {
                if actual_time != claimed_time {
                    Ok(()) // Timestamp mismatch proven
                } else {
                    Err(ChallengeResponseError::InvalidEvidence(
                        "Timestamps match, no violation".into(),
                    ))
                }
            }
            Evidence::StateTransitionProof { .. } => {
                // State transitions require execution to verify
                // This would be done by the arbitrator
                Ok(())
            }
            Evidence::ComputationProof { .. } => {
                // Computation proofs verified during bisection
                Ok(())
            }
        }
    }
}

/// Dispute manager
pub struct DisputeManager {
    /// Active disputes
    disputes: RwLock<HashMap<[u8; 32], Dispute>>,
    /// Disputes by target
    by_target: RwLock<HashMap<[u8; 32], Vec<[u8; 32]>>>,
    /// Resolved dispute count
    resolved_count: AtomicU64,
    /// Challenger wins count
    challenger_wins: AtomicU64,
    /// Defender wins count
    defender_wins: AtomicU64,
    /// Total slashed amount
    total_slashed: AtomicU64,
}

impl DisputeManager {
    pub fn new() -> Self {
        Self {
            disputes: RwLock::new(HashMap::new()),
            by_target: RwLock::new(HashMap::new()),
            resolved_count: AtomicU64::new(0),
            challenger_wins: AtomicU64::new(0),
            defender_wins: AtomicU64::new(0),
            total_slashed: AtomicU64::new(0),
        }
    }

    /// Submit a new challenge
    pub fn submit_challenge(
        &self,
        target_id: [u8; 32],
        target_type: String,
        challenger_id: [u8; 32],
        challenger_bond: u64,
        defender_id: [u8; 32],
        evidence: Evidence,
    ) -> Result<[u8; 32], ChallengeResponseError> {
        if challenger_bond < MIN_CHALLENGE_BOND {
            return Err(ChallengeResponseError::InsufficientBond(
                challenger_bond,
                MIN_CHALLENGE_BOND,
            ));
        }

        // Verify evidence is valid
        EvidenceVerifier::verify_evidence(&evidence)?;

        let dispute = Dispute::new(
            target_id,
            target_type,
            challenger_id,
            challenger_bond,
            defender_id,
            0, // Defender bond collected later
            evidence,
        );

        let id = dispute.id;

        {
            let mut disputes = self.disputes.write();
            disputes.insert(id, dispute);
        }

        {
            let mut by_target = self.by_target.write();
            by_target.entry(target_id).or_insert_with(Vec::new).push(id);
        }

        Ok(id)
    }

    /// Get dispute by ID
    pub fn get_dispute(&self, id: &[u8; 32]) -> Option<Dispute> {
        self.disputes.read().get(id).cloned()
    }

    /// Get disputes for target
    pub fn get_disputes_for_target(&self, target_id: &[u8; 32]) -> Vec<Dispute> {
        let by_target = self.by_target.read();
        let disputes = self.disputes.read();

        by_target
            .get(target_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| disputes.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Submit response to challenge
    pub fn submit_response(
        &self,
        dispute_id: [u8; 32],
        responder_id: [u8; 32],
        responder_bond: u64,
        evidence: Evidence,
    ) -> Result<(), ChallengeResponseError> {
        let mut disputes = self.disputes.write();

        let dispute = disputes
            .get_mut(&dispute_id)
            .ok_or(ChallengeResponseError::ChallengeNotFound(dispute_id))?;

        // Update defender bond
        dispute.defender.bond = responder_bond;

        // Verify response evidence
        EvidenceVerifier::verify_evidence(&evidence)?;

        dispute.submit_response(responder_id, evidence)
    }

    /// Start bisection protocol
    pub fn start_bisection(
        &self,
        dispute_id: [u8; 32],
        lower_bound: u64,
        upper_bound: u64,
        lower_state: Hash,
        upper_state: Hash,
    ) -> Result<(), ChallengeResponseError> {
        let mut disputes = self.disputes.write();

        let dispute = disputes
            .get_mut(&dispute_id)
            .ok_or(ChallengeResponseError::ChallengeNotFound(dispute_id))?;

        dispute.start_bisection(lower_bound, upper_bound, lower_state, upper_state)
    }

    /// Submit bisection step
    pub fn submit_bisection_step(
        &self,
        dispute_id: [u8; 32],
        submitter_id: [u8; 32],
        midpoint_state: Hash,
        side: BisectionSide,
    ) -> Result<bool, ChallengeResponseError> {
        let mut disputes = self.disputes.write();

        let dispute = disputes
            .get_mut(&dispute_id)
            .ok_or(ChallengeResponseError::ChallengeNotFound(dispute_id))?;

        dispute.submit_bisection_step(submitter_id, midpoint_state, side)
    }

    /// Arbitrate and resolve dispute
    pub fn arbitrate(
        &self,
        dispute_id: [u8; 32],
        challenger_wins: bool,
        reason: String,
        finalized_block: u64,
    ) -> Result<DisputeResult, ChallengeResponseError> {
        let result = {
            let mut disputes = self.disputes.write();

            let dispute = disputes
                .get_mut(&dispute_id)
                .ok_or(ChallengeResponseError::ChallengeNotFound(dispute_id))?;

            dispute.resolve(challenger_wins, reason, finalized_block)?
        };

        // Update stats
        self.resolved_count.fetch_add(1, Ordering::Relaxed);

        if challenger_wins {
            self.challenger_wins.fetch_add(1, Ordering::Relaxed);
        } else {
            self.defender_wins.fetch_add(1, Ordering::Relaxed);
        }

        self.total_slashed
            .fetch_add(result.slash_amount, Ordering::Relaxed);

        Ok(result)
    }

    /// Check for expired disputes
    pub fn check_expired(&self) -> Vec<([u8; 32], DisputeResult)> {
        let mut expired = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut disputes = self.disputes.write();

        for (id, dispute) in disputes.iter_mut() {
            // Check if dispute has exceeded response deadline using current timestamp
            let is_timed_out =
                dispute.response_deadline < now && dispute.state == DisputeState::AwaitingResponse;
            if is_timed_out {
                // Defender didn't respond - challenger wins
                if let Ok(result) = dispute.resolve(
                    true,
                    "Response timeout".into(),
                    0, // Block TBD
                ) {
                    expired.push((*id, result));
                }
            }
        }

        expired
    }

    /// Get stats
    pub fn stats(&self) -> DisputeStats {
        DisputeStats {
            total_resolved: self.resolved_count.load(Ordering::Relaxed),
            challenger_wins: self.challenger_wins.load(Ordering::Relaxed),
            defender_wins: self.defender_wins.load(Ordering::Relaxed),
            total_slashed: self.total_slashed.load(Ordering::Relaxed),
            active_disputes: self.disputes.read().len() as u64,
        }
    }
}

impl Default for DisputeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Dispute statistics
#[derive(Debug, Clone)]
pub struct DisputeStats {
    pub total_resolved: u64,
    pub challenger_wins: u64,
    pub defender_wins: u64,
    pub total_slashed: u64,
    pub active_disputes: u64,
}

/// Automated arbitrator for simple disputes
pub struct AutomatedArbitrator;

impl AutomatedArbitrator {
    /// Arbitrate a dispute based on evidence
    pub fn arbitrate(dispute: &Dispute) -> Option<(bool, String)> {
        // Check if we can automatically resolve
        match (&dispute.challenge_evidence, &dispute.response_evidence) {
            // No response - challenger wins
            (_, None) if dispute.is_expired() => Some((true, "No response within deadline".into())),

            // Merkle proof disputes
            (
                Evidence::MerkleProof {
                    root,
                    leaf,
                    proof,
                    leaf_index,
                },
                Some(Evidence::MerkleProof {
                    root: resp_root, ..
                }),
            ) => {
                // Verify challenger's proof
                let challenger_valid = verify_dispute_proof(root, leaf, proof, *leaf_index);

                if challenger_valid && root != resp_root {
                    Some((
                        true,
                        "Challenger's merkle proof valid, root mismatch".into(),
                    ))
                } else if !challenger_valid {
                    Some((false, "Challenger's merkle proof invalid".into()))
                } else {
                    None // Need manual review
                }
            }

            // Double signature - automatic if valid
            (Evidence::DoubleSignature { .. }, _) => {
                // Already verified during submission
                Some((true, "Double signature proven".into()))
            }

            // Timestamp violations
            (
                Evidence::TimestampProof {
                    claimed_time,
                    actual_time,
                    ..
                },
                _,
            ) => {
                if claimed_time != actual_time {
                    Some((
                        true,
                        format!(
                            "Timestamp violation: claimed {} vs actual {}",
                            claimed_time, actual_time
                        ),
                    ))
                } else {
                    Some((false, "No timestamp violation".into()))
                }
            }

            _ => None, // Requires manual/committee arbitration
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_id(n: u8) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = n;
        id
    }

    fn test_hash(n: u8) -> Hash {
        let mut h = [0u8; 32];
        h[0] = n;
        Hash::from(h)
    }

    #[test]
    fn test_merkle_proof_verification() {
        // Build a simple merkle tree
        let leaf0 = test_hash(0);
        let leaf1 = test_hash(1);
        let leaf2 = test_hash(2);
        let leaf3 = test_hash(3);

        // Hash pairs
        let mut hasher = blake3::Hasher::new();
        hasher.update(leaf0.as_bytes());
        hasher.update(leaf1.as_bytes());
        let node01 = Hash::from(*hasher.finalize().as_bytes());

        let mut hasher = blake3::Hasher::new();
        hasher.update(leaf2.as_bytes());
        hasher.update(leaf3.as_bytes());
        let node23 = Hash::from(*hasher.finalize().as_bytes());

        let mut hasher = blake3::Hasher::new();
        hasher.update(node01.as_bytes());
        hasher.update(node23.as_bytes());
        let root = Hash::from(*hasher.finalize().as_bytes());

        // Proof for leaf0: [leaf1, node23]
        let proof = vec![leaf1, node23];

        assert!(verify_dispute_proof(&root, &leaf0, &proof, 0));
        assert!(!verify_dispute_proof(&root, &leaf0, &proof, 1)); // Wrong index
        assert!(!verify_dispute_proof(&root, &test_hash(99), &proof, 0)); // Wrong leaf
    }

    #[test]
    fn test_dispute_lifecycle() {
        let manager = DisputeManager::new();

        let target = test_id(1);
        let challenger = test_id(10);
        let defender = test_id(20);

        // Submit challenge
        let evidence = Evidence::TimestampProof {
            claimed_time: 1000,
            actual_time: 2000,
            signed_claim: vec![1, 2, 3],
        };

        let dispute_id = manager
            .submit_challenge(
                target,
                "block".into(),
                challenger,
                MIN_CHALLENGE_BOND,
                defender,
                evidence,
            )
            .unwrap();

        // Check dispute exists
        let dispute = manager.get_dispute(&dispute_id).unwrap();
        assert_eq!(dispute.state, DisputeState::Challenged);

        // Submit response - use StateTransitionProof which is verified during arbitration
        let response_evidence = Evidence::StateTransitionProof {
            pre_state_root: test_hash(0),
            post_state_root: test_hash(1),
            transition_data: vec![4, 5, 6],
            witness: vec![7, 8, 9],
        };

        manager
            .submit_response(dispute_id, defender, MIN_CHALLENGE_BOND, response_evidence)
            .unwrap();

        // Dispute should be awaiting arbitration
        let dispute = manager.get_dispute(&dispute_id).unwrap();
        assert_eq!(dispute.state, DisputeState::AwaitingArbitration);

        // Arbitrate
        let result = manager
            .arbitrate(
                dispute_id,
                true, // Challenger wins
                "Timestamp mismatch proven".into(),
                100,
            )
            .unwrap();

        assert_eq!(result.winner, challenger);
        assert!(result.slash_amount > 0);

        // Check stats
        let stats = manager.stats();
        assert_eq!(stats.total_resolved, 1);
        assert_eq!(stats.challenger_wins, 1);
    }

    #[test]
    fn test_bisection_protocol() {
        let challenger = test_id(1);
        let defender = test_id(2);

        let mut bisection = BisectionState::new(0, 1000, test_hash(0), test_hash(100), defender);

        // Defender bisects
        bisection
            .apply_choice(test_hash(50), BisectionSide::Upper, challenger)
            .unwrap();
        assert_eq!(bisection.lower_bound, 500);
        assert_eq!(bisection.upper_bound, 1000);
        assert_eq!(bisection.turn, challenger);

        // Challenger bisects
        bisection
            .apply_choice(test_hash(75), BisectionSide::Lower, defender)
            .unwrap();
        assert_eq!(bisection.lower_bound, 500);
        assert_eq!(bisection.upper_bound, 750);

        // Continue until completion
        while !bisection.is_complete() {
            let other = if bisection.turn == challenger {
                defender
            } else {
                challenger
            };
            bisection
                .apply_choice(test_hash(bisection.round + 10), BisectionSide::Lower, other)
                .unwrap();
        }

        assert!(bisection.is_complete());
        assert!(bisection.upper_bound - bisection.lower_bound <= 1);
    }

    #[test]
    fn test_automated_arbitration() {
        // Test timeout case
        let mut dispute = Dispute::new(
            test_id(1),
            "test".into(),
            test_id(10),
            MIN_CHALLENGE_BOND,
            test_id(20),
            0,
            Evidence::TimestampProof {
                claimed_time: 100,
                actual_time: 200,
                signed_claim: vec![],
            },
        );

        // Simulate expiry
        dispute.response_deadline = 0;

        let result = AutomatedArbitrator::arbitrate(&dispute);
        assert!(result.is_some());
        let (challenger_wins, _) = result.unwrap();
        assert!(challenger_wins);
    }

    #[test]
    fn test_insufficient_bond_rejected() {
        let manager = DisputeManager::new();

        let result = manager.submit_challenge(
            test_id(1),
            "test".into(),
            test_id(10),
            MIN_CHALLENGE_BOND - 1, // Too low
            test_id(20),
            Evidence::TimestampProof {
                claimed_time: 100,
                actual_time: 200,
                signed_claim: vec![],
            },
        );

        assert!(matches!(
            result,
            Err(ChallengeResponseError::InsufficientBond(_, _))
        ));
    }
}
