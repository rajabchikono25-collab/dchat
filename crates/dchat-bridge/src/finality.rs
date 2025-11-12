//! Finality tracking with BLS signature aggregation
//!
//! This module implements consensus-based finality verification for cross-chain transactions.
//! Validators use BLS12-381 signatures for efficient aggregation and verification.

#[cfg(test)]
use blst::min_pk::SecretKey;
use blst::min_pk::{AggregateSignature, PublicKey, Signature};
use chrono::{DateTime, Utc};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// BLS signature from a single validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator_id: UserId,
    pub signature: Vec<u8>,
    pub signed_at: DateTime<Utc>,
}

/// Aggregated finality proof with BLS signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedFinalityProof {
    pub tx_hash: String,
    pub block_number: u64,
    pub block_hash: String,
    pub confirmations: u32,
    pub required_confirmations: u32,
    pub required_signatures: usize,
    pub signatures: Vec<ValidatorSignature>,
    pub aggregated_signature: Option<Vec<u8>>,
    pub validator_pubkeys_map: HashMap<UserId, Vec<u8>>,
    pub created_at: DateTime<Utc>,
    pub finalized_at: Option<DateTime<Utc>>,
}

impl AggregatedFinalityProof {
    /// Create new finality proof awaiting validator signatures
    pub fn new(
        tx_hash: String,
        block_number: u64,
        block_hash: String,
        confirmations: u32,
        required_confirmations: u32,
        required_signatures: usize,
    ) -> Self {
        Self {
            tx_hash,
            block_number,
            block_hash,
            confirmations,
            required_confirmations,
            required_signatures,
            signatures: Vec::new(),
            aggregated_signature: None,
            validator_pubkeys_map: HashMap::new(),
            created_at: Utc::now(),
            finalized_at: None,
        }
    }

    /// Add validator signature
    pub fn add_signature(&mut self, signature: ValidatorSignature) -> Result<(), FinalityError> {
        // Check for duplicate validator
        if self
            .signatures
            .iter()
            .any(|s| s.validator_id == signature.validator_id)
        {
            return Err(FinalityError::DuplicateSignature);
        }

        self.signatures.push(signature);

        // Check if we have enough signatures to finalize
        if self.signatures.len() >= self.required_signatures {
            self.try_aggregate()?;
        }

        Ok(())
    }

    /// Attempt to aggregate BLS signatures
    fn try_aggregate(&mut self) -> Result<(), FinalityError> {
        if self.signatures.len() < self.required_signatures {
            return Err(FinalityError::InsufficientSignatures);
        }

        // Parse individual signatures
        let mut sigs = Vec::new();
        for validator_sig in &self.signatures {
            let sig = Signature::from_bytes(&validator_sig.signature)
                .map_err(|_| FinalityError::InvalidSignature)?;
            sigs.push(sig);
        }

        // Convert to slice of references for aggregation
        let sig_refs: Vec<&Signature> = sigs.iter().collect();

        // Aggregate signatures
        let aggregate = AggregateSignature::aggregate(&sig_refs, true)
            .map_err(|_| FinalityError::AggregationFailed)?;

        self.aggregated_signature = Some(aggregate.to_signature().to_bytes().to_vec());
        self.finalized_at = Some(Utc::now());

        Ok(())
    }

    /// Verify aggregated signature against message
    pub fn verify(&self, message: &[u8]) -> Result<bool, FinalityError> {
        let agg_sig_bytes = self
            .aggregated_signature
            .as_ref()
            .ok_or(FinalityError::NotFinalized)?;

        let agg_sig =
            Signature::from_bytes(agg_sig_bytes).map_err(|_| FinalityError::InvalidSignature)?;

        // Get public keys for validators who signed (in order of signatures)
        let mut pubkeys = Vec::new();
        for sig_entry in &self.signatures {
            let pk_bytes = self
                .validator_pubkeys_map
                .get(&sig_entry.validator_id)
                .ok_or(FinalityError::InvalidPublicKey)?;

            let pk =
                PublicKey::from_bytes(pk_bytes).map_err(|_| FinalityError::InvalidPublicKey)?;
            pubkeys.push(pk);
        }

        // Convert to slice of references for verification
        let pk_refs: Vec<&PublicKey> = pubkeys.iter().collect();

        // Verify aggregated signature with domain separation tag
        // For same message signed by multiple validators, use fast_aggregate_verify
        let dst = b"DCHAT_BRIDGE_FINALITY_V1";

        let result = agg_sig.fast_aggregate_verify(true, message, dst, pk_refs.as_slice());

        Ok(result == blst::BLST_ERROR::BLST_SUCCESS)
    }

    /// Check if proof is finalized
    pub fn is_finalized(&self) -> bool {
        self.aggregated_signature.is_some()
            && self.signatures.len() >= self.required_signatures
            && self.confirmations >= self.required_confirmations
    }
}

/// Finality tracking manager with validator consensus
pub struct FinalityTracker {
    /// Pending finality proofs awaiting signatures
    pending_proofs: HashMap<String, AggregatedFinalityProof>,

    /// Finalized proofs
    finalized_proofs: HashMap<String, AggregatedFinalityProof>,

    /// Validator BLS public keys
    validator_pubkeys: HashMap<UserId, Vec<u8>>,

    /// Required validator signatures (M-of-N)
    required_signatures: usize,
    total_validators: usize,
}

impl FinalityTracker {
    /// Create new finality tracker with M-of-N threshold
    pub fn new(required_signatures: usize, total_validators: usize) -> Result<Self, FinalityError> {
        if required_signatures > total_validators {
            return Err(FinalityError::InvalidThreshold);
        }

        if required_signatures == 0 || total_validators == 0 {
            return Err(FinalityError::InvalidThreshold);
        }

        Ok(Self {
            pending_proofs: HashMap::new(),
            finalized_proofs: HashMap::new(),
            validator_pubkeys: HashMap::new(),
            required_signatures,
            total_validators,
        })
    }

    /// Register validator BLS public key
    pub fn register_validator(
        &mut self,
        validator_id: UserId,
        pubkey: Vec<u8>,
    ) -> Result<(), FinalityError> {
        // Validate pubkey format
        PublicKey::from_bytes(&pubkey).map_err(|_| FinalityError::InvalidPublicKey)?;

        self.validator_pubkeys.insert(validator_id, pubkey);
        Ok(())
    }

    /// Initiate finality proof for a transaction
    pub fn initiate_proof(
        &mut self,
        tx_hash: String,
        block_number: u64,
        block_hash: String,
        confirmations: u32,
        required_confirmations: u32,
    ) -> Result<(), FinalityError> {
        if self.pending_proofs.contains_key(&tx_hash)
            || self.finalized_proofs.contains_key(&tx_hash)
        {
            return Err(FinalityError::ProofAlreadyExists);
        }

        let mut proof = AggregatedFinalityProof::new(
            tx_hash.clone(),
            block_number,
            block_hash,
            confirmations,
            required_confirmations,
            self.required_signatures,
        );

        // Add validator pubkeys
        proof.validator_pubkeys_map = self.validator_pubkeys.clone();

        self.pending_proofs.insert(tx_hash, proof);
        Ok(())
    }

    /// Submit validator signature for finality proof
    pub fn submit_signature(
        &mut self,
        tx_hash: &str,
        validator_id: UserId,
        signature: Vec<u8>,
    ) -> Result<bool, FinalityError> {
        // Verify validator is registered
        if !self.validator_pubkeys.contains_key(&validator_id) {
            return Err(FinalityError::UnknownValidator);
        }

        // Get pending proof
        let proof = self
            .pending_proofs
            .get_mut(tx_hash)
            .ok_or(FinalityError::ProofNotFound)?;

        // Add signature
        let validator_sig = ValidatorSignature {
            validator_id,
            signature,
            signed_at: Utc::now(),
        };

        proof.add_signature(validator_sig)?;

        // Check if finalized
        let is_finalized = proof.is_finalized();

        // Move to finalized if ready
        if is_finalized {
            let finalized_proof = self.pending_proofs.remove(tx_hash).unwrap();
            self.finalized_proofs
                .insert(tx_hash.to_string(), finalized_proof);
        }

        Ok(is_finalized)
    }

    /// Check if transaction has finalized proof
    pub fn is_finalized(&self, tx_hash: &str) -> bool {
        self.finalized_proofs.contains_key(tx_hash)
    }

    /// Get finalized proof
    pub fn get_finalized_proof(&self, tx_hash: &str) -> Option<&AggregatedFinalityProof> {
        self.finalized_proofs.get(tx_hash)
    }

    /// Get pending proof
    pub fn get_pending_proof(&self, tx_hash: &str) -> Option<&AggregatedFinalityProof> {
        self.pending_proofs.get(tx_hash)
    }

    /// Verify finalized proof
    pub fn verify_proof(&self, tx_hash: &str, message: &[u8]) -> Result<bool, FinalityError> {
        let proof = self
            .finalized_proofs
            .get(tx_hash)
            .ok_or(FinalityError::ProofNotFound)?;

        proof.verify(message)
    }

    /// Clean up old pending proofs (timeout after 1 hour)
    pub fn cleanup_expired_proofs(&mut self) -> Vec<String> {
        let now = Utc::now();
        let timeout = chrono::Duration::hours(1);

        let mut expired = Vec::new();

        self.pending_proofs.retain(|tx_hash, proof| {
            if now.signed_duration_since(proof.created_at) > timeout {
                expired.push(tx_hash.clone());
                false
            } else {
                true
            }
        });

        expired
    }
}

/// Finality error types
#[derive(Debug, Clone)]
pub enum FinalityError {
    InvalidThreshold,
    InvalidPublicKey,
    InvalidSignature,
    DuplicateSignature,
    InsufficientSignatures,
    AggregationFailed,
    UnknownValidator,
    ProofNotFound,
    ProofAlreadyExists,
    NotFinalized,
}

impl std::fmt::Display for FinalityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FinalityError::InvalidThreshold => write!(f, "Invalid signature threshold"),
            FinalityError::InvalidPublicKey => write!(f, "Invalid BLS public key"),
            FinalityError::InvalidSignature => write!(f, "Invalid BLS signature"),
            FinalityError::DuplicateSignature => write!(f, "Duplicate validator signature"),
            FinalityError::InsufficientSignatures => write!(f, "Insufficient validator signatures"),
            FinalityError::AggregationFailed => write!(f, "BLS signature aggregation failed"),
            FinalityError::UnknownValidator => write!(f, "Unknown validator"),
            FinalityError::ProofNotFound => write!(f, "Finality proof not found"),
            FinalityError::ProofAlreadyExists => write!(f, "Finality proof already exists"),
            FinalityError::NotFinalized => write!(f, "Proof not yet finalized"),
        }
    }
}

impl std::error::Error for FinalityError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_bls_keypair() -> (SecretKey, PublicKey) {
        let mut ikm = [0u8; 32];
        getrandom::getrandom(&mut ikm).unwrap();
        let sk = SecretKey::key_gen(&ikm, &[]).unwrap();
        let pk = sk.sk_to_pk();
        (sk, pk)
    }

    #[test]
    fn test_finality_tracker_creation() {
        let tracker = FinalityTracker::new(3, 5).unwrap();
        assert_eq!(tracker.required_signatures, 3);
        assert_eq!(tracker.total_validators, 5);
    }

    #[test]
    fn test_invalid_threshold() {
        let result = FinalityTracker::new(5, 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_validator_registration() {
        let mut tracker = FinalityTracker::new(2, 3).unwrap();
        let (_, pk) = generate_bls_keypair();
        let validator = UserId::new();

        tracker
            .register_validator(validator, pk.to_bytes().to_vec())
            .unwrap();
        assert_eq!(tracker.validator_pubkeys.len(), 1);
    }

    #[test]
    fn test_initiate_proof() {
        let mut tracker = FinalityTracker::new(2, 3).unwrap();

        tracker
            .initiate_proof(
                "tx_abc".to_string(),
                100,
                "block_hash_xyz".to_string(),
                15,
                12,
            )
            .unwrap();

        assert!(tracker.get_pending_proof("tx_abc").is_some());
    }

    #[test]
    fn test_bls_single_verify() {
        // Test single signature verification
        let (sk, pk) = generate_bls_keypair();
        let message = b"test message";
        let dst = b"DCHAT_BRIDGE_FINALITY_V1";

        let sig = sk.sign(message, dst, &[]);
        let result = sig.verify(true, message, dst, &[], &pk, true);
        assert_eq!(
            result,
            blst::BLST_ERROR::BLST_SUCCESS,
            "Single BLS signature should verify"
        );
    }

    #[test]
    fn test_bls_basic_aggregation() {
        // Test basic BLS signature aggregation outside of FinalityTracker
        let (sk1, pk1) = generate_bls_keypair();
        let (sk2, pk2) = generate_bls_keypair();

        let message = b"test message";
        let dst = b"DCHAT_BRIDGE_FINALITY_V1";

        let sig1 = sk1.sign(message, dst, &[]);
        let sig2 = sk2.sign(message, dst, &[]);

        let sigs = vec![&sig1, &sig2];
        let aggregate = AggregateSignature::aggregate(&sigs, true).unwrap();
        let agg_sig = aggregate.to_signature();

        let pks = vec![&pk1, &pk2];
        // For same message signed by multiple validators, use fast_aggregate_verify
        let result = agg_sig.fast_aggregate_verify(true, message, dst, pks.as_slice());
        assert_eq!(
            result,
            blst::BLST_ERROR::BLST_SUCCESS,
            "Basic BLS aggregation should verify"
        );
    }

    #[test]
    fn test_complete_finality_flow() {
        let mut tracker = FinalityTracker::new(2, 3).unwrap();

        // Generate 3 validators
        let (sk1, pk1) = generate_bls_keypair();
        let (sk2, pk2) = generate_bls_keypair();
        let (sk3, pk3) = generate_bls_keypair();

        let val1 = UserId::new();
        let val2 = UserId::new();
        let val3 = UserId::new();

        tracker
            .register_validator(val1.clone(), pk1.to_bytes().to_vec())
            .unwrap();
        tracker
            .register_validator(val2.clone(), pk2.to_bytes().to_vec())
            .unwrap();
        tracker
            .register_validator(val3.clone(), pk3.to_bytes().to_vec())
            .unwrap();

        // Initiate proof
        let tx_hash = "tx_finality_test";
        tracker
            .initiate_proof(
                tx_hash.to_string(),
                200,
                "block_finality".to_string(),
                20,
                12,
            )
            .unwrap();

        // Sign message with validators using matching DST
        let message = b"finalize tx_finality_test at block 200";
        let dst = b"DCHAT_BRIDGE_FINALITY_V1";
        let sig1 = sk1.sign(message, dst, &[]);
        let sig2 = sk2.sign(message, dst, &[]);

        // Submit signatures (need 2 of 3)
        let result1 = tracker
            .submit_signature(tx_hash, val1, sig1.to_bytes().to_vec())
            .unwrap();
        assert!(!result1); // Not finalized yet

        let result2 = tracker
            .submit_signature(tx_hash, val2, sig2.to_bytes().to_vec())
            .unwrap();
        assert!(result2); // Finalized with 2 signatures

        // Verify finalized
        assert!(tracker.is_finalized(tx_hash));
        let verify_result = tracker.verify_proof(tx_hash, message);
        if let Err(ref e) = verify_result {
            eprintln!("Verification error: {:?}", e);
        }
        assert!(verify_result.is_ok(), "Verification should succeed");
        assert!(verify_result.unwrap(), "Signature should be valid");
    }

    #[test]
    fn test_duplicate_signature_rejected() {
        let mut tracker = FinalityTracker::new(2, 2).unwrap();

        let (sk, pk) = generate_bls_keypair();
        let validator = UserId::new();

        tracker
            .register_validator(validator.clone(), pk.to_bytes().to_vec())
            .unwrap();

        let tx_hash = "tx_duplicate";
        tracker
            .initiate_proof(tx_hash.to_string(), 50, "block_dup".to_string(), 10, 10)
            .unwrap();

        let message = b"test message";
        let dst = b"DCHAT_BRIDGE_FINALITY_V1";
        let sig = sk.sign(message, dst, &[]);

        tracker
            .submit_signature(tx_hash, validator.clone(), sig.to_bytes().to_vec())
            .unwrap();

        // Try to submit again
        let result = tracker.submit_signature(tx_hash, validator, sig.to_bytes().to_vec());
        assert!(result.is_err());
    }

    #[test]
    fn test_cleanup_expired_proofs() {
        let mut tracker = FinalityTracker::new(1, 1).unwrap();

        tracker
            .initiate_proof("tx_old".to_string(), 1, "block_old".to_string(), 5, 10)
            .unwrap();

        // Manually set old timestamp
        if let Some(proof) = tracker.pending_proofs.get_mut("tx_old") {
            proof.created_at = Utc::now() - chrono::Duration::hours(2);
        }

        let expired = tracker.cleanup_expired_proofs();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], "tx_old");
        assert!(tracker.pending_proofs.is_empty());
    }
}
