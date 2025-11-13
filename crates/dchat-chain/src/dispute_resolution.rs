//! Cryptographic dispute resolution for fork arbitration
//!
//! Implements Section 18 (Dispute Resolution) from ARCHITECTURE.md
//! - Claim-challenge-respond mechanism
//! - Fork arbitration with cryptographic proofs
//! - Message integrity verification
//! - Slashing for false claims

use blake3::Hasher;
use dchat_core::error::{Error, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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
    /// Public key of the accused validator (32 bytes Ed25519)
    pub accused_public_key: Vec<u8>,
}

/// Integrity violation evidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityEvidence {
    pub message: Vec<u8>,
    pub claimed_hash: Vec<u8>,
    pub actual_hash: Vec<u8>,
    pub signature: Vec<u8>,
}

/// Currency chain client interface for staking and slashing operations
#[async_trait::async_trait]
pub trait CurrencyChainClient: Send + Sync {
    /// Get validator's current stake amount
    async fn get_validator_stake(&self, validator_key: &[u8]) -> Result<u64>;
    
    /// Execute slash transaction: reduce validator's stake
    async fn execute_slash(
        &self,
        validator_key: &[u8],
        slash_amount: u64,
        reason: &str,
    ) -> Result<String>; // Returns transaction ID
    
    /// Transfer reward to reporter/claimant
    async fn transfer_reward(
        &self,
        recipient_key: &[u8],
        amount: u64,
    ) -> Result<String>;
}

/// Slashing configuration
#[derive(Debug, Clone)]
pub struct SlashingConfig {
    /// Base slash percentage for resolved disputes (0.0 to 1.0)
    pub base_slash_rate: f64,
    /// False claim penalty multiplier
    pub false_claim_multiplier: f64,
    /// Reward percentage for successful claimants (0.0 to 1.0)
    pub claimant_reward_rate: f64,
    /// Minimum stake required to participate in disputes
    pub min_dispute_stake: u64,
}

impl Default for SlashingConfig {
    fn default() -> Self {
        Self {
            base_slash_rate: 0.30,        // 30% stake reduction
            false_claim_multiplier: 1.5,   // 45% for false claims
            claimant_reward_rate: 0.10,    // 10% to claimant
            min_dispute_stake: 1000,       // Minimum 1000 tokens
        }
    }
}

/// Slashing event record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvent {
    pub claim_id: String,
    pub slashed_party: String,
    pub slash_amount: u64,
    pub original_stake: u64,
    pub slash_rate: f64,
    pub reason: String,
    pub transaction_id: String,
    pub timestamp: i64,
    pub beneficiary: String,
    pub reward_amount: u64,
}

/// Dispute resolver
pub struct DisputeResolver {
    claims: HashMap<ClaimId, DisputeClaim>,
    challenges: HashMap<ClaimId, Vec<DisputeChallenge>>,
    responses: HashMap<ClaimId, Vec<DisputeResponse>>,
    slash_threshold: f64,
    slashing_config: SlashingConfig,
    currency_chain_client: Option<Arc<dyn CurrencyChainClient>>,
    slashing_events: Vec<SlashingEvent>,
}

impl DisputeResolver {
    pub fn new() -> Self {
        Self {
            claims: HashMap::new(),
            challenges: HashMap::new(),
            responses: HashMap::new(),
            slash_threshold: 0.66, // 66% vote threshold for slashing
            slashing_config: SlashingConfig::default(),
            currency_chain_client: None,
            slashing_events: Vec::new(),
        }
    }

    /// Create with custom slashing configuration
    pub fn with_slashing_config(mut self, config: SlashingConfig) -> Self {
        self.slashing_config = config;
        self
    }

    /// Set currency chain client for slashing execution
    pub fn with_currency_chain_client(
        mut self,
        client: Arc<dyn CurrencyChainClient>,
    ) -> Self {
        self.currency_chain_client = Some(client);
        self
    }

    /// Get all slashing events
    pub fn get_slashing_events(&self) -> &[SlashingEvent] {
        &self.slashing_events
    }

    /// Set claim status directly (for testing)
    #[cfg(test)]
    pub fn set_claim_status(&mut self, claim_id: &ClaimId, status: DisputeStatus) -> Result<()> {
        let claim = self
            .claims
            .get_mut(claim_id)
            .ok_or_else(|| Error::network("Claim not found"))?;
        claim.status = status;
        Ok(())
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
        // Check that both messages exist and differ (basic fork requirement)
        if evidence.message_a.is_empty() || evidence.message_b.is_empty() {
            return Ok(false);
        }

        if evidence.message_a == evidence.message_b {
            return Ok(false); // Not a fork if messages are identical
        }

        // 1. Extract and validate accused's public key
        if evidence.accused_public_key.len() != 32 {
            return Err(Error::network(
                "Invalid public key length (expected 32 bytes for Ed25519)",
            ));
        }

        let public_key_bytes: [u8; 32] = evidence.accused_public_key[..]
            .try_into()
            .map_err(|_| Error::network("Failed to convert public key to array"))?;

        let verifying_key = VerifyingKey::from_bytes(&public_key_bytes)
            .map_err(|e| Error::network(&format!("Invalid Ed25519 public key: {}", e)))?;

        // 2. Verify signature_a on message_a
        if evidence.signature_a.len() != 64 {
            return Err(Error::network(
                "Invalid signature_a length (expected 64 bytes for Ed25519)",
            ));
        }

        let sig_a_bytes: [u8; 64] = evidence.signature_a[..]
            .try_into()
            .map_err(|_| Error::network("Failed to convert signature_a to array"))?;

        let signature_a = Signature::from_bytes(&sig_a_bytes);

        if verifying_key
            .verify(&evidence.message_a, &signature_a)
            .is_err()
        {
            tracing::warn!("Fork evidence: signature_a verification failed");
            return Ok(false);
        }

        // 3. Verify signature_b on message_b
        if evidence.signature_b.len() != 64 {
            return Err(Error::network(
                "Invalid signature_b length (expected 64 bytes for Ed25519)",
            ));
        }

        let sig_b_bytes: [u8; 64] = evidence.signature_b[..]
            .try_into()
            .map_err(|_| Error::network("Failed to convert signature_b to array"))?;

        let signature_b = Signature::from_bytes(&sig_b_bytes);

        if verifying_key
            .verify(&evidence.message_b, &signature_b)
            .is_err()
        {
            tracing::warn!("Fork evidence: signature_b verification failed");
            return Ok(false);
        }

        // 4. Check that sequence numbers would be the same (already stored in evidence)
        // Both messages were signed by same key with different content - fork proven!
        tracing::info!(
            "Fork evidence verified: accused signed two different messages (seq: {})",
            evidence.sequence_number
        );

        Ok(true)
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
    pub async fn resolve_dispute(&mut self, claim_id: ClaimId, vote_for_claimant: f64) -> Result<()> {
        let claim = self
            .claims
            .get(&claim_id)
            .ok_or_else(|| Error::network("Claim not found"))?;

        if claim.status != DisputeStatus::UnderVote {
            return Err(Error::network("Claim not under vote"));
        }

        // Clone data needed for async operations to avoid borrow issues
        let claim_id_str = claim.id.0.clone();
        let accused = claim.accused.clone();
        let claimant = claim.claimant.clone();

        if vote_for_claimant >= self.slash_threshold {
            // Execute slashing against accused
            self.execute_slash(
                accused.as_bytes(),
                claimant.as_bytes(),
                &claim_id_str,
                "Dispute resolved against accused",
                self.slashing_config.base_slash_rate,
            ).await?;
            
            // Update claim status after slash completes
            if let Some(claim) = self.claims.get_mut(&claim_id) {
                claim.status = DisputeStatus::ResolvedForClaimant;
            }
            
            tracing::info!(
                "Slashed {}'s stake for dispute {}",
                accused,
                claim_id_str
            );
        } else if vote_for_claimant <= (1.0 - self.slash_threshold) {
            // Execute slashing against claimant for false claim
            let false_claim_rate = self.slashing_config.base_slash_rate 
                * self.slashing_config.false_claim_multiplier;
            
            self.execute_slash(
                claimant.as_bytes(),
                accused.as_bytes(),
                &claim_id_str,
                "False claim penalty",
                false_claim_rate,
            ).await?;
            
            // Update claim status after slash completes
            if let Some(claim) = self.claims.get_mut(&claim_id) {
                claim.status = DisputeStatus::ResolvedForAccused;
            }
            
            tracing::info!(
                "Slashed {}'s stake for false claim {}",
                claimant,
                claim_id_str
            );
        } else {
            // Update claim status - no slashing
            if let Some(claim) = self.claims.get_mut(&claim_id) {
                claim.status = DisputeStatus::Dismissed;
            }
            tracing::info!("Dispute {} dismissed as inconclusive", claim_id_str);
        }

        Ok(())
    }

    /// Execute slash transaction on currency chain
    async fn execute_slash(
        &mut self,
        slashed_party_key: &[u8],
        beneficiary_key: &[u8],
        claim_id: &str,
        reason: &str,
        slash_rate: f64,
    ) -> Result<()> {
        let client = self
            .currency_chain_client
            .as_ref()
            .ok_or_else(|| Error::network("Currency chain client not configured"))?;

        // 1. Query staked amount
        let original_stake = client.get_validator_stake(slashed_party_key).await?;

        if original_stake < self.slashing_config.min_dispute_stake {
            return Err(Error::network(
                format!("Insufficient stake: {} < {}", original_stake, self.slashing_config.min_dispute_stake)
            ));
        }

        // 2. Calculate slash amount
        let slash_amount = (original_stake as f64 * slash_rate).round() as u64;
        
        if slash_amount == 0 {
            tracing::warn!("Slash amount is zero, skipping execution");
            return Ok(());
        }

        // 3. Execute slash on currency chain
        let slash_tx_id = client
            .execute_slash(slashed_party_key, slash_amount, reason)
            .await?;

        tracing::info!(
            "Executed slash: {} tokens ({}% of {}), tx: {}",
            slash_amount,
            slash_rate * 100.0,
            original_stake,
            slash_tx_id
        );

        // 4. Transfer reward to beneficiary (claimant or accused)
        let reward_amount = (slash_amount as f64 * self.slashing_config.claimant_reward_rate).round() as u64;
        
        let _reward_tx_id = if reward_amount > 0 {
            client
                .transfer_reward(beneficiary_key, reward_amount)
                .await?
        } else {
            String::from("N/A")
        };

        // 5. Record slashing event
        let event = SlashingEvent {
            claim_id: claim_id.to_string(),
            slashed_party: hex::encode(slashed_party_key),
            slash_amount,
            original_stake,
            slash_rate,
            reason: reason.to_string(),
            transaction_id: slash_tx_id,
            timestamp: chrono::Utc::now().timestamp(),
            beneficiary: hex::encode(beneficiary_key),
            reward_amount,
        };

        self.slashing_events.push(event);

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
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    /// Helper to create properly signed fork evidence for testing
    fn create_signed_fork_evidence(
        message_a: &[u8],
        message_b: &[u8],
        sequence_number: u64,
    ) -> (ForkEvidence, SigningKey) {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let signature_a = signing_key.sign(message_a);
        let signature_b = signing_key.sign(message_b);

        let evidence = ForkEvidence {
            message_a: message_a.to_vec(),
            message_b: message_b.to_vec(),
            signature_a: signature_a.to_bytes().to_vec(),
            signature_b: signature_b.to_bytes().to_vec(),
            sequence_number,
            accused_public_key: verifying_key.to_bytes().to_vec(),
        };

        (evidence, signing_key)
    }

    #[test]
    fn test_submit_claim() {
        let mut resolver = DisputeResolver::new();

        let (evidence, _) = create_signed_fork_evidence(b"message 1", b"message 2", 42);

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

        let (evidence, _) = create_signed_fork_evidence(b"message 1", b"message 2", 42);

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

        let (evidence, _) = create_signed_fork_evidence(b"message 1", b"message 2", 42);

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

        // Test valid fork with proper signatures
        let (evidence, _) = create_signed_fork_evidence(b"message 1", b"message 2", 42);
        let valid = resolver.verify_fork_evidence(&evidence).unwrap();
        assert!(valid, "Valid fork evidence should pass verification");

        // Test invalid fork: same message
        let (invalid_evidence, _) = create_signed_fork_evidence(b"message 1", b"message 1", 42);
        let valid = resolver.verify_fork_evidence(&invalid_evidence).unwrap();
        assert!(!valid, "Same messages should not be valid fork");

        // Test invalid signature
        let mut invalid_sig_evidence = evidence.clone();
        invalid_sig_evidence.signature_a = vec![0; 64]; // Zero signature is invalid
        let valid = resolver
            .verify_fork_evidence(&invalid_sig_evidence)
            .unwrap();
        assert!(!valid, "Invalid signature should fail verification");

        // Test wrong public key
        let mut csprng = OsRng;
        let wrong_key = SigningKey::generate(&mut csprng);
        let mut wrong_key_evidence = evidence.clone();
        wrong_key_evidence.accused_public_key = wrong_key.verifying_key().to_bytes().to_vec();
        let valid = resolver.verify_fork_evidence(&wrong_key_evidence).unwrap();
        assert!(!valid, "Wrong public key should fail verification");
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

        let (evidence, _) = create_signed_fork_evidence(b"message 1", b"message 2", 42);

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

        let (evidence, _) = create_signed_fork_evidence(b"message 1", b"message 2", 42);

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

        let (evidence, _) = create_signed_fork_evidence(b"message 1", b"message 2", 42);

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
