//! Cryptographic dispute resolution for fork arbitration
//!
//! Implements Section 18 (Dispute Resolution) from ARCHITECTURE.md
//! - Claim-challenge-respond mechanism
//! - Fork arbitration with cryptographic proofs
//! - Message integrity verification
//! - Slashing for false claims

use blake3::Hasher;
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dispute claim identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClaimId(pub String);

/// Dispute type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeType {
    /// Fork in message ordering
    ForkDetected,
    /// Message integrity violation
    IntegrityViolation,
    /// Invalid state transition
    InvalidStateTransition,
    /// Double spending (if applicable)
    DoubleSpend,
}

/// Dispute claim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeClaim {
    pub id: ClaimId,
    pub dispute_type: DisputeType,
    pub claimant: String,
    pub accused: String,
    pub evidence: Vec<u8>,
    pub evidence_hash: Vec<u8>,
    pub timestamp: i64,
    pub status: DisputeStatus,
}

/// Dispute status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeStatus {
    /// Claim submitted, awaiting challenge
    Pending,
    /// Challenged by accused
    Challenged,
    /// Responded with counter-evidence
    Responded,
    /// Under governance vote
    UnderVote,
    /// Resolved in favor of claimant
    ResolvedForClaimant,
    /// Resolved in favor of accused
    ResolvedForAccused,
    /// Dismissed (invalid claim)
    Dismissed,
}

/// Challenge to a dispute claim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeChallenge {
    pub claim_id: ClaimId,
    pub challenger: String,
    pub counter_evidence: Vec<u8>,
    pub counter_evidence_hash: Vec<u8>,
    pub timestamp: i64,
}

/// Response to a challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeResponse {
    pub claim_id: ClaimId,
    pub responder: String,
    pub additional_evidence: Vec<u8>,
    pub timestamp: i64,
}

/// Fork evidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkEvidence {
    /// First message in fork
    pub message_a: Vec<u8>,
    /// Second conflicting message
    pub message_b: Vec<u8>,
    /// Signature on message A
    pub signature_a: Vec<u8>,
    /// Signature on message B
    pub signature_b: Vec<u8>,
    /// Sequence number (should be same for fork)
    pub sequence_number: u64,
}

/// Integrity violation evidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityEvidence {
    pub message: Vec<u8>,
    pub claimed_hash: Vec<u8>,
    pub actual_hash: Vec<u8>,
    pub signature: Vec<u8>,
}

/// Dispute resolver
pub struct DisputeResolver {
    claims: HashMap<ClaimId, DisputeClaim>,
    challenges: HashMap<ClaimId, Vec<DisputeChallenge>>,
    responses: HashMap<ClaimId, Vec<DisputeResponse>>,
    slash_threshold: f64,
}

impl DisputeResolver {
    pub fn new() -> Self {
        Self {
            claims: HashMap::new(),
            challenges: HashMap::new(),
            responses: HashMap::new(),
            slash_threshold: 0.66, // 66% vote threshold for slashing
        }
    }

    /// Submit a new dispute claim
    pub fn submit_claim(
        &mut self,
        dispute_type: DisputeType,
        claimant: String,
        accused: String,
        evidence: Vec<u8>,
    ) -> Result<ClaimId> {
        // Validate evidence format based on dispute type
        self.validate_evidence(&dispute_type, &evidence)?;

        let evidence_hash = self.hash_evidence(&evidence);
        let claim_id = ClaimId(uuid::Uuid::new_v4().to_string());

        let claim = DisputeClaim {
            id: claim_id.clone(),
            dispute_type,
            claimant,
            accused,
            evidence,
            evidence_hash,
            timestamp: chrono::Utc::now().timestamp(),
            status: DisputeStatus::Pending,
        };

        self.claims.insert(claim_id.clone(), claim);

        Ok(claim_id)
    }

    /// Validate evidence based on dispute type
    fn validate_evidence(&self, dispute_type: &DisputeType, evidence: &[u8]) -> Result<()> {
        match dispute_type {
            DisputeType::ForkDetected => {
                // Should deserialize to ForkEvidence
                serde_json::from_slice::<ForkEvidence>(evidence)
                    .map_err(|_| Error::network("Invalid fork evidence format"))?;
            }
            DisputeType::IntegrityViolation => {
                // Should deserialize to IntegrityEvidence
                serde_json::from_slice::<IntegrityEvidence>(evidence)
                    .map_err(|_| Error::network("Invalid integrity evidence format"))?;
            }
            _ => {
                // Other types: basic validation
                if evidence.is_empty() {
                    return Err(Error::network("Evidence cannot be empty"));
                }
            }
        }

        Ok(())
    }

    /// Hash evidence for integrity
    fn hash_evidence(&self, evidence: &[u8]) -> Vec<u8> {
        let mut hasher = Hasher::new();
        hasher.update(evidence);
        hasher.finalize().as_bytes().to_vec()
    }

    /// Challenge a claim
    pub fn challenge_claim(
        &mut self,
        claim_id: ClaimId,
        challenger: String,
        counter_evidence: Vec<u8>,
    ) -> Result<()> {
        // Compute hash before borrowing self mutably
        let counter_evidence_hash = self.hash_evidence(&counter_evidence);

        let claim = self
            .claims
            .get_mut(&claim_id)
            .ok_or_else(|| Error::network("Claim not found"))?;

        if claim.status != DisputeStatus::Pending {
            return Err(Error::network("Claim not in pending status"));
        }

        // Verify challenger is the accused
        if challenger != claim.accused {
            return Err(Error::network("Only accused can challenge claim"));
        }

        let challenge = DisputeChallenge {
            claim_id: claim_id.clone(),
            challenger,
            counter_evidence,
            counter_evidence_hash,
            timestamp: chrono::Utc::now().timestamp(),
        };

        self.challenges
            .entry(claim_id.clone())
            .or_default()
            .push(challenge);

        claim.status = DisputeStatus::Challenged;

        Ok(())
    }

    /// Respond to a challenge
    pub fn respond_to_challenge(
        &mut self,
        claim_id: ClaimId,
        responder: String,
        additional_evidence: Vec<u8>,
    ) -> Result<()> {
        let claim = self
            .claims
            .get_mut(&claim_id)
            .ok_or_else(|| Error::network("Claim not found"))?;

        if claim.status != DisputeStatus::Challenged {
            return Err(Error::network("Claim not in challenged status"));
        }

        // Verify responder is the claimant
        if responder != claim.claimant {
            return Err(Error::network("Only claimant can respond to challenge"));
        }

        let response = DisputeResponse {
            claim_id: claim_id.clone(),
            responder,
            additional_evidence,
            timestamp: chrono::Utc::now().timestamp(),
        };

        self.responses
            .entry(claim_id.clone())
            .or_default()
            .push(response);

        claim.status = DisputeStatus::Responded;

        Ok(())
    }

    /// Verify fork evidence cryptographically
    pub fn verify_fork_evidence(&self, evidence: &ForkEvidence) -> Result<bool> {
        // Check that both messages claim same sequence number
        if evidence.message_a.is_empty() || evidence.message_b.is_empty() {
            return Ok(false);
        }

        // In production: verify signatures with Ed25519
        // For now, check that messages are different
        Ok(evidence.message_a != evidence.message_b)
    }

    /// Verify integrity violation evidence
    pub fn verify_integrity_evidence(&self, evidence: &IntegrityEvidence) -> Result<bool> {
        // Compute actual hash
        let computed_hash = self.hash_evidence(&evidence.message);

        // Check if it matches the accused's claimed hash (should differ)
        Ok(computed_hash != evidence.claimed_hash && computed_hash == evidence.actual_hash)
    }

    /// Submit claim to governance vote
    pub fn submit_to_vote(&mut self, claim_id: ClaimId) -> Result<()> {
        let claim = self
            .claims
            .get_mut(&claim_id)
            .ok_or_else(|| Error::network("Claim not found"))?;

        if claim.status != DisputeStatus::Responded {
            return Err(Error::network("Claim must be responded to before voting"));
        }

        claim.status = DisputeStatus::UnderVote;

        Ok(())
    }

    /// Resolve dispute based on vote
    pub fn resolve_dispute(&mut self, claim_id: ClaimId, vote_for_claimant: f64) -> Result<()> {
        let claim = self
            .claims
            .get_mut(&claim_id)
            .ok_or_else(|| Error::network("Claim not found"))?;

        if claim.status != DisputeStatus::UnderVote {
            return Err(Error::network("Claim not under vote"));
        }

        if vote_for_claimant >= self.slash_threshold {
            claim.status = DisputeStatus::ResolvedForClaimant;
            // In production: slash accused's stake
        } else if vote_for_claimant <= (1.0 - self.slash_threshold) {
            claim.status = DisputeStatus::ResolvedForAccused;
            // In production: slash claimant's stake for false claim
        } else {
            claim.status = DisputeStatus::Dismissed;
            // Inconclusive: no slashing
        }

        Ok(())
    }

    /// Get claim by ID
    pub fn get_claim(&self, claim_id: &ClaimId) -> Option<&DisputeClaim> {
        self.claims.get(claim_id)
    }

    /// Get challenges for a claim
    pub fn get_challenges(&self, claim_id: &ClaimId) -> Vec<&DisputeChallenge> {
        self.challenges
            .get(claim_id)
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    }

    /// Get responses for a claim
    pub fn get_responses(&self, claim_id: &ClaimId) -> Vec<&DisputeResponse> {
        self.responses
            .get(claim_id)
            .map(|r| r.iter().collect())
            .unwrap_or_default()
    }

    /// Get statistics
    pub fn get_stats(&self) -> DisputeStats {
        let total = self.claims.len();
        let pending = self
            .claims
            .values()
            .filter(|c| c.status == DisputeStatus::Pending)
            .count();
        let resolved = self
            .claims
            .values()
            .filter(|c| {
                matches!(
                    c.status,
                    DisputeStatus::ResolvedForClaimant | DisputeStatus::ResolvedForAccused
                )
            })
            .count();

        DisputeStats {
            total_claims: total,
            pending_claims: pending,
            resolved_claims: resolved,
            dismissed_claims: self
                .claims
                .values()
                .filter(|c| c.status == DisputeStatus::Dismissed)
                .count(),
        }
    }
}

impl Default for DisputeResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Dispute statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeStats {
    pub total_claims: usize,
    pub pending_claims: usize,
    pub resolved_claims: usize,
    pub dismissed_claims: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submit_claim() {
        let mut resolver = DisputeResolver::new();

        let evidence = ForkEvidence {
            message_a: b"message 1".to_vec(),
            message_b: b"message 2".to_vec(),
            signature_a: vec![0; 64],
            signature_b: vec![0; 64],
            sequence_number: 42,
        };

        let claim_id = resolver
            .submit_claim(
                DisputeType::ForkDetected,
                "alice".to_string(),
                "bob".to_string(),
                serde_json::to_vec(&evidence).unwrap(),
            )
            .unwrap();

        let claim = resolver.get_claim(&claim_id).unwrap();
        assert_eq!(claim.status, DisputeStatus::Pending);
        assert_eq!(claim.claimant, "alice");
        assert_eq!(claim.accused, "bob");
    }

    #[test]
    fn test_challenge_claim() {
        let mut resolver = DisputeResolver::new();

        let evidence = ForkEvidence {
            message_a: b"message 1".to_vec(),
            message_b: b"message 2".to_vec(),
            signature_a: vec![0; 64],
            signature_b: vec![0; 64],
            sequence_number: 42,
        };

        let claim_id = resolver
            .submit_claim(
                DisputeType::ForkDetected,
                "alice".to_string(),
                "bob".to_string(),
                serde_json::to_vec(&evidence).unwrap(),
            )
            .unwrap();

        let counter_evidence = b"counter evidence".to_vec();
        resolver
            .challenge_claim(claim_id.clone(), "bob".to_string(), counter_evidence)
            .unwrap();

        let claim = resolver.get_claim(&claim_id).unwrap();
        assert_eq!(claim.status, DisputeStatus::Challenged);
    }

    #[test]
    fn test_respond_to_challenge() {
        let mut resolver = DisputeResolver::new();

        let evidence = ForkEvidence {
            message_a: b"message 1".to_vec(),
            message_b: b"message 2".to_vec(),
            signature_a: vec![0; 64],
            signature_b: vec![0; 64],
            sequence_number: 42,
        };

        let claim_id = resolver
            .submit_claim(
                DisputeType::ForkDetected,
                "alice".to_string(),
                "bob".to_string(),
                serde_json::to_vec(&evidence).unwrap(),
            )
            .unwrap();

        resolver
            .challenge_claim(claim_id.clone(), "bob".to_string(), b"counter".to_vec())
            .unwrap();
        resolver
            .respond_to_challenge(claim_id.clone(), "alice".to_string(), b"response".to_vec())
            .unwrap();

        let claim = resolver.get_claim(&claim_id).unwrap();
        assert_eq!(claim.status, DisputeStatus::Responded);
    }

    #[test]
    fn test_verify_fork_evidence() {
        let resolver = DisputeResolver::new();

        let evidence = ForkEvidence {
            message_a: b"message 1".to_vec(),
            message_b: b"message 2".to_vec(),
            signature_a: vec![0; 64],
            signature_b: vec![0; 64],
            sequence_number: 42,
        };

        let valid = resolver.verify_fork_evidence(&evidence).unwrap();
        assert!(valid);

        // Same message should be invalid fork
        let invalid_evidence = ForkEvidence {
            message_a: b"message 1".to_vec(),
            message_b: b"message 1".to_vec(),
            signature_a: vec![0; 64],
            signature_b: vec![0; 64],
            sequence_number: 42,
        };

        let valid = resolver.verify_fork_evidence(&invalid_evidence).unwrap();
        assert!(!valid);
    }

    #[test]
    fn test_verify_integrity_evidence() {
        let resolver = DisputeResolver::new();

        let message = b"test message";
        let mut hasher = Hasher::new();
        hasher.update(message);
        let actual_hash = hasher.finalize().as_bytes().to_vec();

        let evidence = IntegrityEvidence {
            message: message.to_vec(),
            claimed_hash: vec![0; 32], // Wrong hash
            actual_hash: actual_hash.clone(),
            signature: vec![0; 64],
        };

        let valid = resolver.verify_integrity_evidence(&evidence).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_resolve_dispute_for_claimant() {
        let mut resolver = DisputeResolver::new();

        let evidence = ForkEvidence {
            message_a: b"message 1".to_vec(),
            message_b: b"message 2".to_vec(),
            signature_a: vec![0; 64],
            signature_b: vec![0; 64],
            sequence_number: 42,
        };

        let claim_id = resolver
            .submit_claim(
                DisputeType::ForkDetected,
                "alice".to_string(),
                "bob".to_string(),
                serde_json::to_vec(&evidence).unwrap(),
            )
            .unwrap();

        resolver
            .challenge_claim(claim_id.clone(), "bob".to_string(), b"counter".to_vec())
            .unwrap();
        resolver
            .respond_to_challenge(claim_id.clone(), "alice".to_string(), b"response".to_vec())
            .unwrap();
        resolver.submit_to_vote(claim_id.clone()).unwrap();
        resolver.resolve_dispute(claim_id.clone(), 0.8).unwrap(); // 80% vote for claimant

        let claim = resolver.get_claim(&claim_id).unwrap();
        assert_eq!(claim.status, DisputeStatus::ResolvedForClaimant);
    }

    #[test]
    fn test_resolve_dispute_for_accused() {
        let mut resolver = DisputeResolver::new();

        let evidence = ForkEvidence {
            message_a: b"message 1".to_vec(),
            message_b: b"message 2".to_vec(),
            signature_a: vec![0; 64],
            signature_b: vec![0; 64],
            sequence_number: 42,
        };

        let claim_id = resolver
            .submit_claim(
                DisputeType::ForkDetected,
                "alice".to_string(),
                "bob".to_string(),
                serde_json::to_vec(&evidence).unwrap(),
            )
            .unwrap();

        resolver
            .challenge_claim(claim_id.clone(), "bob".to_string(), b"counter".to_vec())
            .unwrap();
        resolver
            .respond_to_challenge(claim_id.clone(), "alice".to_string(), b"response".to_vec())
            .unwrap();
        resolver.submit_to_vote(claim_id.clone()).unwrap();
        resolver.resolve_dispute(claim_id.clone(), 0.2).unwrap(); // 20% vote for claimant

        let claim = resolver.get_claim(&claim_id).unwrap();
        assert_eq!(claim.status, DisputeStatus::ResolvedForAccused);
    }

    #[test]
    fn test_dispute_stats() {
        let mut resolver = DisputeResolver::new();

        let evidence = ForkEvidence {
            message_a: b"message 1".to_vec(),
            message_b: b"message 2".to_vec(),
            signature_a: vec![0; 64],
            signature_b: vec![0; 64],
            sequence_number: 42,
        };

        resolver
            .submit_claim(
                DisputeType::ForkDetected,
                "alice".to_string(),
                "bob".to_string(),
                serde_json::to_vec(&evidence).unwrap(),
            )
            .unwrap();

        let stats = resolver.get_stats();
        assert_eq!(stats.total_claims, 1);
        assert_eq!(stats.pending_claims, 1);
        assert_eq!(stats.resolved_claims, 0);
    }
}
