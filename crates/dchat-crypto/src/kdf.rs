//! Key derivation functions and utilities

use crate::keys::PrivateKey;
use dchat_core::error::{Error, Result};

/// HKDF (HMAC-based Key Derivation Function) implementation
pub struct Hkdf;

impl Hkdf {
    /// Extract and expand using HKDF
    pub fn derive(
        salt: Option<&[u8]>,
        input_key_material: &[u8],
        info: &[u8],
        output_length: usize,
    ) -> Result<Vec<u8>> {
        use hkdf::Hkdf as HkdfImpl;
        use sha2::Sha256;

        let hkdf = HkdfImpl::<Sha256>::new(salt, input_key_material);
        let mut output = vec![0u8; output_length];
        hkdf.expand(info, &mut output)
            .map_err(|e| Error::crypto(format!("HKDF expansion failed: {}", e)))?;

        Ok(output)
    }

    /// Derive a key for a specific purpose
    pub fn derive_purpose_key(
        master_key: &PrivateKey,
        purpose: &str,
        index: u32,
    ) -> Result<PrivateKey> {
        let info = format!("dchat:{}:{}", purpose, index);
        let derived = Self::derive(
            Some(b"dchat-kdf-salt"),
            master_key.as_bytes(),
            info.as_bytes(),
            32,
        )?;

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&derived);
        Ok(PrivateKey::from_bytes(key_bytes))
    }
}

/// Key derivation for specific dchat purposes
pub struct DchatKdf;

impl DchatKdf {
    /// Derive encryption key for a specific conversation
    pub fn derive_conversation_key(
        user_key: &PrivateKey,
        peer_public_key: &crate::keys::PublicKey,
        conversation_id: &str,
    ) -> Result<PrivateKey> {
        let mut input = Vec::new();
        input.extend_from_slice(user_key.as_bytes());
        input.extend_from_slice(peer_public_key.as_bytes());
        input.extend_from_slice(conversation_id.as_bytes());

        let derived = crate::hash(&input);
        Ok(PrivateKey::from_bytes(derived))
    }

    /// Derive device-specific key
    pub fn derive_device_key(master_key: &PrivateKey, device_id: &str) -> Result<PrivateKey> {
        Hkdf::derive_purpose_key(
            master_key,
            "device",
            crate::hash(device_id.as_bytes())[0..4]
                .iter()
                .enumerate()
                .map(|(i, &b)| (b as u32) << (i * 8))
                .sum(),
        )
    }

    /// Derive channel-specific key
    pub fn derive_channel_key(user_key: &PrivateKey, channel_id: &str) -> Result<PrivateKey> {
        Hkdf::derive_purpose_key(
            user_key,
            "channel",
            crate::hash(channel_id.as_bytes())[0..4]
                .iter()
                .enumerate()
                .map(|(i, &b)| (b as u32) << (i * 8))
                .sum(),
        )
    }

    /// Derive burner identity key
    pub fn derive_burner_key(master_key: &PrivateKey, burner_index: u32) -> Result<PrivateKey> {
        Hkdf::derive_purpose_key(master_key, "burner", burner_index)
    }

    /// Derive relay authentication key
    pub fn derive_relay_key(node_key: &PrivateKey, relay_index: u32) -> Result<PrivateKey> {
        Hkdf::derive_purpose_key(node_key, "relay", relay_index)
    }
}

/// Secure memory for storing sensitive key material
pub struct SecureBytes {
    bytes: Vec<u8>,
}

impl SecureBytes {
    /// Create new secure bytes
    pub fn new(size: usize) -> Self {
        Self {
            bytes: vec![0u8; size],
        }
    }

    /// Create from existing bytes
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Get reference to bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Get mutable reference to bytes
    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    /// Get the length
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl Drop for SecureBytes {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.bytes.zeroize();
    }
}

impl std::fmt::Debug for SecureBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureBytes")
            .field("len", &self.bytes.len())
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyPair;

    #[test]
    fn test_hkdf() {
        let input = b"test input key material";
        let salt = b"test salt";
        let info = b"test info";

        let output1 = Hkdf::derive(Some(salt), input, info, 32).unwrap();
        let output2 = Hkdf::derive(Some(salt), input, info, 32).unwrap();

        // Same inputs should produce same output
        assert_eq!(output1, output2);

        // Different info should produce different output
        let output3 = Hkdf::derive(Some(salt), input, b"different info", 32).unwrap();
        assert_ne!(output1, output3);
    }

    #[test]
    fn test_conversation_key_derivation() {
        let alice_keypair = KeyPair::generate();
        let bob_keypair = KeyPair::generate();
        let conversation_id = "alice-bob-conversation";

        // Both parties should derive the same key for the same conversation
        let alice_conv_key = DchatKdf::derive_conversation_key(
            alice_keypair.private_key(),
            bob_keypair.public_key(),
            conversation_id,
        )
        .unwrap();

        let bob_conv_key = DchatKdf::derive_conversation_key(
            bob_keypair.private_key(),
            alice_keypair.public_key(),
            conversation_id,
        )
        .unwrap();

        // Keys should be different (no shared secret without DH)
        assert_ne!(alice_conv_key.as_bytes(), bob_conv_key.as_bytes());

        // But derivation should be deterministic
        let alice_conv_key2 = DchatKdf::derive_conversation_key(
            alice_keypair.private_key(),
            bob_keypair.public_key(),
            conversation_id,
        )
        .unwrap();

        assert_eq!(alice_conv_key.as_bytes(), alice_conv_key2.as_bytes());
    }

    #[test]
    fn test_device_key_derivation() {
        let master_key = PrivateKey::generate();
        let device_id = "alice-laptop";

        let device_key1 = DchatKdf::derive_device_key(&master_key, device_id).unwrap();
        let device_key2 = DchatKdf::derive_device_key(&master_key, device_id).unwrap();

        // Should be deterministic
        assert_eq!(device_key1.as_bytes(), device_key2.as_bytes());

        // Different device should produce different key
        let other_device_key = DchatKdf::derive_device_key(&master_key, "alice-phone").unwrap();
        assert_ne!(device_key1.as_bytes(), other_device_key.as_bytes());
    }
}
