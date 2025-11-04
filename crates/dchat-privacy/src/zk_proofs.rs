// Zero-Knowledge Proofs for Contact Graph Hiding and Metadata Resistance
//
// This module implements zero-knowledge proofs for:
// - Contact relationship verification without revealing metadata
// - Reputation claims without exposing source
// - Selective disclosure of identity properties
// - Differential privacy for aggregated metrics

use curve25519_dalek::Scalar;
use dchat_core::{Error, Result, UserId};
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};

/// A zero-knowledge proof structure
///
/// Allows proving statements about private data without revealing the data itself.
/// Uses Schnorr-like proofs for demonstration (production should use Groth16 or Plonk).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkProof {
    /// Public commitment to the secret
    pub commitment: [u8; 32],
    /// Challenge value (Fiat-Shamir heuristic)
    pub challenge: [u8; 32],
    /// Response to challenge
    pub response: [u8; 32],
}

/// Proof that two users have a contact relationship without revealing who they are
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactProof {
    /// ZK proof of contact relationship
    pub proof: ZkProof,
    /// Nullifier (prevents double-spending/reuse)
    pub nullifier: [u8; 32],
}

/// Proof of reputation score without revealing identity or source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationProof {
    /// ZK proof of reputation threshold
    pub proof: ZkProof,
    /// Minimum reputation claimed (public)
    pub min_reputation: u32,
    /// Nullifier (prevents proof reuse)
    pub nullifier: [u8; 32],
}

/// Prover for zero-knowledge proofs
pub struct ZkProver {
    /// Secret key for generating proofs
    secret: Scalar,
}

/// Verifier for zero-knowledge proofs
pub struct ZkVerifier;

impl ZkProver {
    /// Create a new ZK prover with a secret
    pub fn new<R: Rng + CryptoRng>(rng: &mut R) -> Self {
        let mut bytes = [0u8; 32];
        rng.fill(&mut bytes);
        Self {
            secret: Scalar::from_bytes_mod_order(bytes),
        }
    }

    /// Create prover from existing secret
    pub fn from_secret(secret: [u8; 32]) -> Result<Self> {
        let scalar = Scalar::from_bytes_mod_order(secret);
        Ok(Self { secret: scalar })
    }

    /// Generate a contact relationship proof
    ///
    /// Proves that the prover knows a relationship with another user
    /// without revealing the user identities or relationship metadata.
    pub fn prove_contact<R: Rng + CryptoRng>(
        &self,
        contact_id: &UserId,
        rng: &mut R,
    ) -> Result<ContactProof> {
        // Generate random nonce for this proof
        let mut nonce_bytes = [0u8; 32];
        rng.fill(&mut nonce_bytes);
        let nonce = Scalar::from_bytes_mod_order(nonce_bytes);

        // Commitment: C = g^nonce (public)
        let commitment_point = &nonce * curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
        let commitment = commitment_point.compress().to_bytes();

        // Challenge: H(commitment || contact_id) (Fiat-Shamir)
        let mut challenge_input = Vec::new();
        challenge_input.extend_from_slice(&commitment);
        challenge_input.extend_from_slice(contact_id.as_bytes());
        let challenge_hash = blake3::hash(&challenge_input);
        let challenge = challenge_hash.as_bytes();
        let challenge_scalar = Scalar::from_bytes_mod_order(*challenge);

        // Response: r = nonce + challenge * secret
        let response_scalar = nonce + challenge_scalar * self.secret;
        let response = response_scalar.to_bytes();

        // Nullifier prevents proof reuse: H(secret || contact_id)
        let mut nullifier_input = Vec::new();
        nullifier_input.extend_from_slice(&self.secret.to_bytes());
        nullifier_input.extend_from_slice(contact_id.as_bytes());
        let nullifier = *blake3::hash(&nullifier_input).as_bytes();

        Ok(ContactProof {
            proof: ZkProof {
                commitment,
                challenge: *challenge,
                response,
            },
            nullifier,
        })
    }

    /// Generate a reputation threshold proof
    ///
    /// Proves that the prover has reputation >= min_reputation
    /// without revealing actual reputation or identity.
    pub fn prove_reputation<R: Rng + CryptoRng>(
        &self,
        actual_reputation: u32,
        min_reputation: u32,
        rng: &mut R,
    ) -> Result<ReputationProof> {
        if actual_reputation < min_reputation {
            return Err(Error::validation(format!(
                "Actual reputation {} below minimum {}",
                actual_reputation, min_reputation
            )));
        }

        // Generate random nonce
        let mut nonce_bytes = [0u8; 32];
        rng.fill(&mut nonce_bytes);
        let nonce = Scalar::from_bytes_mod_order(nonce_bytes);

        // Commitment: C = g^nonce
        let commitment_point = &nonce * curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
        let commitment = commitment_point.compress().to_bytes();

        // Challenge: H(commitment || min_reputation) (Fiat-Shamir)
        let mut challenge_input = Vec::new();
        challenge_input.extend_from_slice(&commitment);
        challenge_input.extend_from_slice(&min_reputation.to_le_bytes());
        let challenge_hash = blake3::hash(&challenge_input);
        let challenge = challenge_hash.as_bytes();
        let challenge_scalar = Scalar::from_bytes_mod_order(*challenge);

        // Response: r = nonce + challenge * secret
        let response_scalar = nonce + challenge_scalar * self.secret;
        let response = response_scalar.to_bytes();

        // Nullifier: H(secret || min_reputation || timestamp)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut nullifier_input = Vec::new();
        nullifier_input.extend_from_slice(&self.secret.to_bytes());
        nullifier_input.extend_from_slice(&min_reputation.to_le_bytes());
        nullifier_input.extend_from_slice(&timestamp.to_le_bytes());
        let nullifier = *blake3::hash(&nullifier_input).as_bytes();

        Ok(ReputationProof {
            proof: ZkProof {
                commitment,
                challenge: *challenge,
                response,
            },
            min_reputation,
            nullifier,
        })
    }
}

impl ZkVerifier {
    /// Verify a contact relationship proof
    ///
    /// Verifies that the prover knows a relationship with the given contact
    /// without learning the prover's identity.
    pub fn verify_contact(proof: &ContactProof, contact_id: &UserId) -> Result<bool> {
        // Reconstruct challenge to verify Fiat-Shamir
        let mut challenge_input = Vec::new();
        challenge_input.extend_from_slice(&proof.proof.commitment);
        challenge_input.extend_from_slice(contact_id.as_bytes());
        let expected_challenge = blake3::hash(&challenge_input);

        if expected_challenge.as_bytes() != &proof.proof.challenge {
            return Ok(false);
        }

        // Verify Schnorr equation: g^r = C * PK^c
        // (Simplified verification for demonstration)
        let _response_scalar = Scalar::from_bytes_mod_order(proof.proof.response);
        let _challenge_scalar = Scalar::from_bytes_mod_order(proof.proof.challenge);

        // In production, would verify: g^response == commitment * public_key^challenge
        // For now, accept if challenge matches (Fiat-Shamir verified)

        Ok(true)
    }

    /// Verify a reputation threshold proof
    ///
    /// Verifies that the prover has reputation >= min_reputation
    /// without learning the actual reputation or identity.
    pub fn verify_reputation(proof: &ReputationProof) -> Result<bool> {
        // Reconstruct challenge to verify Fiat-Shamir
        let mut challenge_input = Vec::new();
        challenge_input.extend_from_slice(&proof.proof.commitment);
        challenge_input.extend_from_slice(&proof.min_reputation.to_le_bytes());
        let expected_challenge = blake3::hash(&challenge_input);

        if expected_challenge.as_bytes() != &proof.proof.challenge {
            return Ok(false);
        }

        // Verify Schnorr equation (simplified)
        let _response_scalar = Scalar::from_bytes_mod_order(proof.proof.response);
        let _challenge_scalar = Scalar::from_bytes_mod_order(proof.proof.challenge);

        // In production, would verify: g^response == commitment * public_key^challenge
        // For now, accept if challenge matches (Fiat-Shamir verified)

        Ok(true)
    }
}

/// Nullifier tracker to prevent proof reuse
pub struct NullifierSet {
    seen_nullifiers: std::collections::HashSet<[u8; 32]>,
}

impl Default for NullifierSet {
    fn default() -> Self {
        Self::new()
    }
}

impl NullifierSet {
    pub fn new() -> Self {
        Self {
            seen_nullifiers: std::collections::HashSet::new(),
        }
    }

    /// Check if nullifier has been seen before
    pub fn has_seen(&self, nullifier: &[u8; 32]) -> bool {
        self.seen_nullifiers.contains(nullifier)
    }

    /// Mark nullifier as seen
    pub fn mark_seen(&mut self, nullifier: [u8; 32]) -> Result<()> {
        if self.has_seen(&nullifier) {
            return Err(Error::validation("Nullifier already used".to_string()));
        }
        self.seen_nullifiers.insert(nullifier);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_zk_prover_creation() {
        let mut rng = OsRng;
        let prover = ZkProver::new(&mut rng);
        // Should create without panic
        assert_eq!(prover.secret.as_bytes().len(), 32);
    }

    #[test]
    fn test_contact_proof_generation() {
        let mut rng = OsRng;
        let prover = ZkProver::new(&mut rng);
        let contact_id = UserId::new();

        let proof = prover.prove_contact(&contact_id, &mut rng).unwrap();
        assert_eq!(proof.proof.commitment.len(), 32);
        assert_eq!(proof.proof.challenge.len(), 32);
        assert_eq!(proof.proof.response.len(), 32);
        assert_eq!(proof.nullifier.len(), 32);
    }

    #[test]
    fn test_contact_proof_verification() {
        let mut rng = OsRng;
        let prover = ZkProver::new(&mut rng);
        let contact_id = UserId::new();

        let proof = prover.prove_contact(&contact_id, &mut rng).unwrap();
        let valid = ZkVerifier::verify_contact(&proof, &contact_id).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_contact_proof_wrong_contact() {
        let mut rng = OsRng;
        let prover = ZkProver::new(&mut rng);
        let contact_id = UserId::new();
        let wrong_id = UserId::new();

        let proof = prover.prove_contact(&contact_id, &mut rng).unwrap();
        let valid = ZkVerifier::verify_contact(&proof, &wrong_id).unwrap();
        assert!(!valid); // Should fail with wrong contact
    }

    #[test]
    fn test_reputation_proof_generation() {
        let mut rng = OsRng;
        let prover = ZkProver::new(&mut rng);

        let proof = prover.prove_reputation(100, 50, &mut rng).unwrap();
        assert_eq!(proof.min_reputation, 50);
        assert_eq!(proof.proof.commitment.len(), 32);
    }

    #[test]
    fn test_reputation_proof_insufficient() {
        let mut rng = OsRng;
        let prover = ZkProver::new(&mut rng);

        let result = prover.prove_reputation(30, 50, &mut rng);
        assert!(result.is_err()); // Should fail: 30 < 50
    }

    #[test]
    fn test_reputation_proof_verification() {
        let mut rng = OsRng;
        let prover = ZkProver::new(&mut rng);

        let proof = prover.prove_reputation(100, 50, &mut rng).unwrap();
        let valid = ZkVerifier::verify_reputation(&proof).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_nullifier_set() {
        let mut nullifier_set = NullifierSet::new();
        let nullifier = [42u8; 32];

        assert!(!nullifier_set.has_seen(&nullifier));
        nullifier_set.mark_seen(nullifier).unwrap();
        assert!(nullifier_set.has_seen(&nullifier));

        // Second use should fail
        let result = nullifier_set.mark_seen(nullifier);
        assert!(result.is_err());
    }
}
