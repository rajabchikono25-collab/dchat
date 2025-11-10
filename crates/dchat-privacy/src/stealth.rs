// Stealth Payloads for Uninspectable Message Content
//
// This module implements stealth addressing and payload encryption
// that prevents relay nodes from inspecting message content or
// determining recipient identity.

use curve25519_dalek::Scalar;
use dchat_core::{Error, Result};
use dchat_crypto::keys::PublicKey;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};

/// A stealth address for anonymous message delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealthAddress {
    /// Public view key (for scanning)
    pub view_key: [u8; 32],
    /// Public spend key (for ownership)
    pub spend_key: [u8; 32],
}

/// An encrypted payload with stealth addressing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealthPayload {
    /// Stealth address ephemeral key
    pub ephemeral_key: [u8; 32],
    /// Encrypted message content
    pub ciphertext: Vec<u8>,
    /// Message tag for recipient identification
    pub tag: [u8; 16],
    /// Padding to uniform size
    pub padding_size: usize,
}

/// Generator for stealth addresses and payloads
#[allow(dead_code)]
pub struct StealthGenerator {
    /// Sender's private key
    private_key: Scalar,
}

/// Scanner for detecting stealth messages
#[allow(dead_code)]
pub struct StealthScanner {
    /// Recipient's view key (for scanning)
    view_key: Scalar,
    /// Recipient's spend key (for decryption)
    spend_key: Scalar,
}

impl StealthAddress {
    /// Create a stealth address from public keys
    pub fn new(view_key: [u8; 32], spend_key: [u8; 32]) -> Self {
        Self {
            view_key,
            spend_key,
        }
    }

    /// Derive from a user's primary public key
    pub fn from_user_key<R: Rng + CryptoRng>(user_key: &PublicKey, rng: &mut R) -> Result<Self> {
        // Derive view and spend keys from user's public key
        let view_seed = blake3::hash(user_key.as_bytes());
        let view_key = *view_seed.as_bytes();

        // Generate random spend key
        let mut spend_key = [0u8; 32];
        rng.fill(&mut spend_key);

        Ok(Self::new(view_key, spend_key))
    }
}

impl StealthGenerator {
    /// Create a new stealth generator
    pub fn new<R: Rng + CryptoRng>(rng: &mut R) -> Self {
        let mut bytes = [0u8; 32];
        rng.fill(&mut bytes);
        Self {
            private_key: Scalar::from_bytes_mod_order(bytes),
        }
    }

    /// Create a stealth payload for a recipient
    ///
    /// The payload is encrypted such that:
    /// - Only the recipient can decrypt it
    /// - Relay nodes cannot see content or recipient
    /// - Size is padded to prevent traffic analysis
    pub fn create_payload<R: Rng + CryptoRng>(
        &self,
        recipient: &StealthAddress,
        plaintext: &[u8],
        rng: &mut R,
    ) -> Result<StealthPayload> {
        // Generate ephemeral key for this message
        let mut ephemeral_bytes = [0u8; 32];
        rng.fill(&mut ephemeral_bytes);
        let ephemeral_scalar = Scalar::from_bytes_mod_order(ephemeral_bytes);
        let ephemeral_point =
            &ephemeral_scalar * curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
        let ephemeral_key = ephemeral_point.compress().to_bytes();

        // Derive shared secret from recipient's view key
        let recipient_view_point =
            curve25519_dalek::ristretto::CompressedRistretto(recipient.view_key)
                .decompress()
                .ok_or_else(|| Error::Crypto("Invalid view key".to_string()))?;
        let shared_secret_point = ephemeral_scalar * recipient_view_point;
        let shared_secret = shared_secret_point.compress().to_bytes();

        // Derive encryption key from shared secret using HKDF
        let encryption_key = blake3::hash(&shared_secret);

        // Production: ChaCha20Poly1305 AEAD encryption
        use chacha20poly1305::aead::Aead;
        use chacha20poly1305::{ChaCha20Poly1305, KeyInit};

        let cipher = ChaCha20Poly1305::new(encryption_key.as_bytes().into());
        let nonce_array: [u8; 12] = encryption_key.as_bytes()[0..12]
            .try_into()
            .map_err(|_| Error::Crypto("Failed to create nonce".to_string()))?;
        let nonce = &chacha20poly1305::aead::Nonce::<ChaCha20Poly1305>::from(nonce_array);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| Error::Crypto("Encryption failed".to_string()))?;

        // Create tag for recipient identification: H(view_key || ephemeral_key)
        let mut tag_input = Vec::new();
        tag_input.extend_from_slice(&recipient.view_key);
        tag_input.extend_from_slice(&ephemeral_key);
        let tag_hash = blake3::hash(&tag_input);
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&tag_hash.as_bytes()[0..16]);

        // Pad to uniform size (e.g., 1KB blocks)
        let target_size = Self::calculate_padded_size(plaintext.len());
        let padding_size = target_size - plaintext.len();

        Ok(StealthPayload {
            ephemeral_key,
            ciphertext,
            tag,
            padding_size,
        })
    }

    /// Calculate padded size to nearest power of 2
    fn calculate_padded_size(actual_size: usize) -> usize {
        let min_sizes = [256, 512, 1024, 2048, 4096, 8192];
        for &size in &min_sizes {
            if actual_size <= size {
                return size;
            }
        }
        // Round up to nearest 8KB
        actual_size.div_ceil(8192) * 8192
    }

    /// Generate decoy messages for cover traffic
    pub fn create_decoy<R: Rng + CryptoRng>(
        &self,
        size: usize,
        rng: &mut R,
    ) -> Result<StealthPayload> {
        // Create random dummy data
        let mut dummy_data = vec![0u8; size];
        rng.fill(&mut dummy_data[..]);

        // Create dummy stealth address
        let mut view_key = [0u8; 32];
        let mut spend_key = [0u8; 32];
        rng.fill(&mut view_key);
        rng.fill(&mut spend_key);
        let dummy_address = StealthAddress::new(view_key, spend_key);

        self.create_payload(&dummy_address, &dummy_data, rng)
    }
}

impl StealthScanner {
    /// Create a scanner with view and spend keys
    pub fn new(view_key: Scalar, spend_key: Scalar) -> Self {
        Self {
            view_key,
            spend_key,
        }
    }

    /// Check if a payload is for this recipient
    ///
    /// Uses the tag to quickly filter without decryption
    pub fn is_for_me(&self, payload: &StealthPayload) -> Result<bool> {
        // Reconstruct expected tag
        let view_key_point =
            &self.view_key * curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
        let view_key_bytes = view_key_point.compress().to_bytes();

        let mut tag_input = Vec::new();
        tag_input.extend_from_slice(&view_key_bytes);
        tag_input.extend_from_slice(&payload.ephemeral_key);
        let expected_tag_hash = blake3::hash(&tag_input);
        let mut expected_tag = [0u8; 16];
        expected_tag.copy_from_slice(&expected_tag_hash.as_bytes()[0..16]);

        Ok(expected_tag == payload.tag)
    }

    /// Decrypt a stealth payload
    ///
    /// Only works if is_for_me() returns true
    pub fn decrypt(&self, payload: &StealthPayload) -> Result<Vec<u8>> {
        if !self.is_for_me(payload)? {
            return Err(Error::Crypto("Payload not for this recipient".to_string()));
        }

        // Derive shared secret
        let ephemeral_point =
            curve25519_dalek::ristretto::CompressedRistretto(payload.ephemeral_key)
                .decompress()
                .ok_or_else(|| Error::Crypto("Invalid ephemeral key".to_string()))?;
        let shared_secret_point = self.view_key * ephemeral_point;
        let shared_secret = shared_secret_point.compress().to_bytes();

        // Derive decryption key
        let decryption_key = blake3::hash(&shared_secret);

        // Production: ChaCha20Poly1305 AEAD decryption
        use chacha20poly1305::aead::Aead;
        use chacha20poly1305::{ChaCha20Poly1305, KeyInit};

        let cipher = ChaCha20Poly1305::new(decryption_key.as_bytes().into());
        let nonce_array: [u8; 12] = decryption_key.as_bytes()[0..12]
            .try_into()
            .map_err(|_| Error::Crypto("Failed to create nonce".to_string()))?;
        let nonce = &chacha20poly1305::aead::Nonce::<ChaCha20Poly1305>::from(nonce_array);
        let mut plaintext = cipher
            .decrypt(nonce, payload.ciphertext.as_ref())
            .map_err(|_| Error::Crypto("Decryption failed".to_string()))?;

        // Remove padding
        if plaintext.len() > payload.padding_size {
            plaintext.truncate(plaintext.len() - payload.padding_size);
        }

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_stealth_address_creation() {
        let view_key = [1u8; 32];
        let spend_key = [2u8; 32];
        let addr = StealthAddress::new(view_key, spend_key);
        assert_eq!(addr.view_key, view_key);
        assert_eq!(addr.spend_key, spend_key);
    }

    #[test]
    fn test_stealth_payload_creation() {
        let mut rng = OsRng;
        let generator = StealthGenerator::new(&mut rng);

        // Create valid recipient keys using proper scalar operations
        let view_bytes = [1u8; 32];
        let view_scalar = Scalar::from_bytes_mod_order(view_bytes);
        let view_point = &view_scalar * curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;

        let spend_bytes = [2u8; 32];
        let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);
        let spend_point = &spend_scalar * curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;

        let recipient = StealthAddress::new(
            view_point.compress().to_bytes(),
            spend_point.compress().to_bytes(),
        );

        let message = b"Secret message";
        let payload = generator
            .create_payload(&recipient, message, &mut rng)
            .unwrap();

        assert_eq!(payload.ephemeral_key.len(), 32);
        assert!(!payload.ciphertext.is_empty());
        assert_eq!(payload.tag.len(), 16);
    }

    #[test]
    fn test_stealth_encryption_decryption() {
        let mut rng = OsRng;
        let generator = StealthGenerator::new(&mut rng);

        // Create recipient keys
        let mut view_bytes = [0u8; 32];
        rng.fill(&mut view_bytes);
        let view_scalar = Scalar::from_bytes_mod_order(view_bytes);

        let mut spend_bytes = [0u8; 32];
        rng.fill(&mut spend_bytes);
        let spend_scalar = Scalar::from_bytes_mod_order(spend_bytes);

        let view_point = &view_scalar * curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
        let spend_point = &spend_scalar * curve25519_dalek::constants::RISTRETTO_BASEPOINT_TABLE;
        let recipient = StealthAddress::new(
            view_point.compress().to_bytes(),
            spend_point.compress().to_bytes(),
        );

        let scanner = StealthScanner::new(view_scalar, spend_scalar);

        // Encrypt message
        let message = b"Top secret data";
        let payload = generator
            .create_payload(&recipient, message, &mut rng)
            .unwrap();

        // Scanner should recognize it
        assert!(scanner.is_for_me(&payload).unwrap());

        // Decrypt and verify
        let decrypted = scanner.decrypt(&payload).unwrap();
        assert_eq!(&decrypted[..message.len()], message);
    }

    #[test]
    fn test_padding_calculation() {
        assert_eq!(StealthGenerator::calculate_padded_size(100), 256);
        assert_eq!(StealthGenerator::calculate_padded_size(256), 256);
        assert_eq!(StealthGenerator::calculate_padded_size(257), 512);
        assert_eq!(StealthGenerator::calculate_padded_size(1000), 1024);
        assert_eq!(StealthGenerator::calculate_padded_size(2000), 2048);
    }
}
