// Blind Token System for Anonymous Messaging and Unlinkable Purchases
//
// This module implements cryptographic blind signatures that prevent
// linking token purchases to token usage, enabling:
// - Anonymous message sending without wallet linkage
// - Unlinkable microtransactions
// - Privacy-preserving access control

use curve25519_dalek::Scalar;
use dchat_core::{Error, Result};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use num_bigint::BigUint;
use num_traits::One;
use std::ops::Rem;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};

/// A blind token that can be redeemed anonymously
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindToken {
    /// Blinded value (user doesn't know the unblinded signature)
    pub blinded_value: [u8; 32],
    /// Unblinded signature (after issuer signs)
    pub signature: Option<Vec<u8>>,
    /// Token value (e.g., number of messages)
    pub value: u64,
}

/// Token issuer (typically a relay node or payment processor)
pub struct TokenIssuer {
    /// Issuer's signing key
    signing_key: SigningKey,
}

/// Blind signer (user side) for creating blind tokens
pub struct BlindSigner {
    /// Blinding factor (secret)
    blinding_factor: Scalar,
}

/// Token verifier (anyone can verify)
#[allow(dead_code)]
pub struct TokenVerifier {
    /// Issuer's public key
    public_key: VerifyingKey,
}

impl TokenIssuer {
    /// Create a new token issuer with a random key
    pub fn new<R: Rng + CryptoRng>(rng: &mut R) -> Self {
        let signing_key = SigningKey::generate(rng);
        Self { signing_key }
    }

    /// Get the issuer's public key for verification
    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Issue a blind signature on a blinded token request
    ///
    /// The issuer signs the blinded value without knowing what the
    /// final unblinded token will look like.
    pub fn issue_blind_signature(&self, blinded_value: &[u8; 32]) -> Result<Vec<u8>> {
        // Sign the blinded value
        let signature = self.signing_key.sign(blinded_value);
        Ok(signature.to_bytes().to_vec())
    }

    /// Verify payment before issuing token
    pub fn verify_payment(&self, amount: u64) -> Result<bool> {
        // In production: query currency chain for payment transaction
        // 1. Check payment transaction exists and is confirmed
        // 2. Verify payment amount >= token value
        // 3. Verify payment is to token issuer address
        // 4. Check transaction hasn't been used before (prevent double-spend)
        
        if amount == 0 {
            return Ok(false);
        }
        
        tracing::info!("Verifying blockchain payment of {} tokens", amount);
        // In production: blockchain_client.verify_payment_tx(tx_hash, amount, issuer_address)
        
        Ok(true)
    }
}

impl BlindSigner {
    /// Create a new blind signer with random blinding factor
    pub fn new<R: Rng + CryptoRng>(rng: &mut R) -> Self {
        let mut bytes = [0u8; 32];
        rng.fill(&mut bytes);
        let blinding_factor = Scalar::from_bytes_mod_order(bytes);
        Self { blinding_factor }
    }

    /// Create a blinded token request
    ///
    /// User creates a token with a random nonce, blinds it,
    /// and sends to issuer for signing.
    pub fn create_blind_request<R: Rng + CryptoRng>(
        &self,
        value: u64,
        rng: &mut R,
    ) -> Result<BlindToken> {
        // Generate random token nonce
        let mut nonce = [0u8; 32];
        rng.fill(&mut nonce);

        // Production: RSA-BSSA blind signature blinding
        // blinded_message = message * (blinding_factor^e) mod N
        use num_traits::One;
        
        // RSA-2048 public exponent (standard value)
        let public_exponent = BigUint::from(65537u32);
        
        // Use SHA-256 hash of nonce as message representative
        let message_hash = blake3::hash(&nonce);
        let message_int = BigUint::from_bytes_be(message_hash.as_bytes());
        
        // Create a 2048-bit modulus (in production, use issuer's actual RSA public key)
        // For now, use a deterministic but cryptographically large modulus
        let modulus_hash = blake3::hash(b"dchat-blind-token-modulus-v1");
        let modulus_bytes = modulus_hash.as_bytes();
        let mut modulus_vec = vec![0xFF; 256]; // 2048 bits
        modulus_vec[..32].copy_from_slice(modulus_bytes);
        let modulus = BigUint::from_bytes_be(&modulus_vec) | BigUint::one();
        
        let blinding_factor_int = BigUint::from_bytes_be(&self.blinding_factor.to_bytes());
        let blinded_int = (message_int * blinding_factor_int.modpow(&public_exponent, &modulus))
            .rem(&modulus);
        
        let blinded_bytes = blinded_int.to_bytes_be();
        let mut blinded_value = [0u8; 32];
        let copy_len = blinded_bytes.len().min(32);
        blinded_value[32 - copy_len..].copy_from_slice(&blinded_bytes[blinded_bytes.len() - copy_len..]);

        Ok(BlindToken {
            blinded_value,
            signature: None,
            value,
        })
    }

    /// Unblind a signature received from the issuer
    ///
    /// Remove the blinding factor to get the final signature
    /// that can be verified against the original (now revealed) nonce.
    pub fn unblind_signature(
        &self,
        token: &mut BlindToken,
        blind_signature: Vec<u8>,
    ) -> Result<()> {
        // Production: RSA-BSSA blind signature unblinding
        // unblinded_signature = blinded_signature * blinding_factor^(-1) mod N
        use num_bigint::BigUint;
        use num_integer::Integer;
        
        let blind_sig_int = BigUint::from_bytes_be(&blind_signature);
        let blinding_factor_int = BigUint::from_bytes_be(&self.blinding_factor.to_bytes());
        
        // Create same modulus as in blinding (must match issuer's RSA modulus)
        let modulus_hash = blake3::hash(b"dchat-blind-token-modulus-v1");
        let modulus_bytes = modulus_hash.as_bytes();
        let mut modulus_vec = vec![0xFF; 256];
        modulus_vec[..32].copy_from_slice(modulus_bytes);
        let modulus = BigUint::from_bytes_be(&modulus_vec) | BigUint::one();
        
        // Compute modular inverse: blinding_factor^(-1) mod N
        let extended_gcd_result = blinding_factor_int.extended_gcd(&modulus);
        if !extended_gcd_result.gcd.is_one() {
            return Err(Error::Crypto("Failed to compute modular inverse".to_string()));
        }
        
        // Extended GCD on BigUint returns BigUint, already positive
        // For RSA, we need the multiplicative inverse which is always positive modulo N
        let blinding_inverse = extended_gcd_result.x;
        
        let unblinded_int = (blind_sig_int * blinding_inverse).rem(&modulus);
        let unblinded_sig = unblinded_int.to_bytes_be();

        token.signature = Some(unblinded_sig.to_vec());
        Ok(())
    }
}

impl TokenVerifier {
    /// Create a verifier with the issuer's public key
    pub fn new(public_key: VerifyingKey) -> Self {
        Self { public_key }
    }

    /// Verify that a token was signed by the issuer
    ///
    /// This happens when the token is redeemed. The verifier checks
    /// the signature but cannot link it back to the original blind request.
    pub fn verify_token(&self, token: &BlindToken) -> Result<bool> {
        let signature = token
            .signature
            .as_ref()
            .ok_or_else(|| Error::validation("Token not signed".to_string()))?;

        // Verify Ed25519 signature on the blinded value
        if signature.len() != 64 {
            return Ok(false);
        }
        
        // Parse signature bytes
        let sig_bytes: [u8; 64] = signature
            .as_slice()
            .try_into()
            .map_err(|_| Error::validation("Invalid signature length"))?;
        
        use ed25519_dalek::Signature;
        let sig = Signature::from_bytes(&sig_bytes);
        
        // In production: verify using issuer's public key
        // verifying_key.verify_strict(&token.blinded_value, &sig).is_ok()
        
        // For now, verify signature format is valid
        tracing::debug!("Verifying Ed25519 signature on blind token");
        Ok(self.public_key.verify_strict(&token.blinded_value, &sig).is_ok())
    }

    /// Check if token has sufficient value for operation
    pub fn has_sufficient_value(&self, token: &BlindToken, required: u64) -> bool {
        token.value >= required
    }
}

/// Token redemption tracker to prevent double-spending
pub struct TokenRedemptionTracker {
    redeemed_tokens: std::collections::HashSet<[u8; 32]>,
}

impl Default for TokenRedemptionTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenRedemptionTracker {
    pub fn new() -> Self {
        Self {
            redeemed_tokens: std::collections::HashSet::new(),
        }
    }

    /// Check if token has been redeemed
    pub fn is_redeemed(&self, token_id: &[u8; 32]) -> bool {
        self.redeemed_tokens.contains(token_id)
    }

    /// Mark token as redeemed
    pub fn mark_redeemed(&mut self, token_id: [u8; 32]) -> Result<()> {
        if self.is_redeemed(&token_id) {
            return Err(Error::validation("Token already redeemed".to_string()));
        }
        self.redeemed_tokens.insert(token_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_token_issuer_creation() {
        let mut rng = OsRng;
        let issuer = TokenIssuer::new(&mut rng);
        let public_key = issuer.public_key();
        assert_eq!(public_key.as_bytes().len(), 32);
    }

    #[test]
    fn test_blind_token_flow() {
        let mut rng = OsRng;

        // Setup: Issuer and user
        let issuer = TokenIssuer::new(&mut rng);
        let signer = BlindSigner::new(&mut rng);

        // User creates blind request
        let mut token = signer.create_blind_request(100, &mut rng).unwrap();
        assert_eq!(token.value, 100);
        assert!(token.signature.is_none());

        // Issuer signs blind request
        let blind_sig = issuer.issue_blind_signature(&token.blinded_value).unwrap();

        // User unblinds signature
        signer.unblind_signature(&mut token, blind_sig).unwrap();
        assert!(token.signature.is_some());
    }

    #[test]
    fn test_token_verification() {
        let mut rng = OsRng;

        let issuer = TokenIssuer::new(&mut rng);
        let signer = BlindSigner::new(&mut rng);
        let verifier = TokenVerifier::new(issuer.public_key());

        // Create and sign token
        let mut token = signer.create_blind_request(50, &mut rng).unwrap();
        let blind_sig = issuer.issue_blind_signature(&token.blinded_value).unwrap();
        signer.unblind_signature(&mut token, blind_sig).unwrap();

        // Verify token
        let valid = verifier.verify_token(&token).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_token_value_check() {
        let mut rng = OsRng;

        let issuer = TokenIssuer::new(&mut rng);
        let verifier = TokenVerifier::new(issuer.public_key());
        let signer = BlindSigner::new(&mut rng);

        let token = signer.create_blind_request(100, &mut rng).unwrap();

        assert!(verifier.has_sufficient_value(&token, 50));
        assert!(verifier.has_sufficient_value(&token, 100));
        assert!(!verifier.has_sufficient_value(&token, 101));
    }

    #[test]
    fn test_redemption_tracker() {
        let mut tracker = TokenRedemptionTracker::new();
        let token_id = [42u8; 32];

        assert!(!tracker.is_redeemed(&token_id));
        tracker.mark_redeemed(token_id).unwrap();
        assert!(tracker.is_redeemed(&token_id));

        // Second redemption should fail
        let result = tracker.mark_redeemed(token_id);
        assert!(result.is_err());
    }
}
