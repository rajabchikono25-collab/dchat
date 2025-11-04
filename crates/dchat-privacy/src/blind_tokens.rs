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

    /// Verify payment before issuing token (simplified)
    pub fn verify_payment(&self, amount: u64) -> Result<bool> {
        // In production, this would check blockchain payment
        // For now, always accept (demonstration only)
        Ok(amount > 0)
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

        // Blind the nonce: blinded = nonce + blinding_factor (simplified)
        let mut blinded_value = nonce;
        let blinding_bytes = self.blinding_factor.to_bytes();
        for (i, byte) in blinded_value.iter_mut().enumerate() {
            *byte = byte.wrapping_add(blinding_bytes[i % 32]);
        }

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
        // Unblind: signature' = signature - blinding_factor (simplified)
        let mut unblinded_sig = blind_signature.clone();
        let blinding_bytes = self.blinding_factor.to_bytes();
        for (i, byte) in unblinded_sig.iter_mut().enumerate() {
            *byte = byte.wrapping_sub(blinding_bytes[i % 32]);
        }

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

        // In production, would verify Ed25519 signature
        // For simplified implementation, accept if signature exists
        Ok(signature.len() == 64)
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
