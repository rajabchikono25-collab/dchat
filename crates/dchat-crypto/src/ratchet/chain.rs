//! Chain Key and Message Key derivation
//!
//! Implements the symmetric ratchet portion of the Double Ratchet.

use dchat_core::error::{Error, Result};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::kdf::Hkdf;

/// Constant for root key derivation
pub const KDF_RK: &[u8] = b"dchat-rk";

/// Constant for chain key derivation
pub const KDF_CK: &[u8] = b"dchat-ck";

/// Chain Key - used to derive message keys and next chain key
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ChainKey {
    key: [u8; 32],
    index: u32,
}

impl ChainKey {
    /// Create a chain key from bytes
    pub fn new(key: [u8; 32], index: u32) -> Self {
        Self { key, index }
    }

    /// Create from the root key and DH output
    pub fn from_root_key(root_key: &[u8; 32], dh_output: &[u8; 32]) -> Result<(Self, [u8; 32])> {
        // KDF_RK(rk, dh_out) = HKDF(rk, dh_out, info, 64)
        // Returns (new_root_key, chain_key)
        let output = Hkdf::derive(Some(root_key), dh_output, KDF_RK, 64)?;

        let mut new_root_key = [0u8; 32];
        let mut chain_key = [0u8; 32];
        new_root_key.copy_from_slice(&output[0..32]);
        chain_key.copy_from_slice(&output[32..64]);

        Ok((Self::new(chain_key, 0), new_root_key))
    }

    /// Get the current message key and advance the chain
    pub fn derive_message_key(&mut self) -> Result<MessageKey> {
        // KDF_CK(ck) = HKDF(ck, 0x01, info, 64)
        // Returns (message_key, next_chain_key)
        let output = Hkdf::derive(Some(&self.key), &[0x01], KDF_CK, 64)?;

        let mut message_key = [0u8; 32];
        let mut next_chain_key = [0u8; 32];
        message_key.copy_from_slice(&output[0..32]);
        next_chain_key.copy_from_slice(&output[32..64]);

        let mk = MessageKey::new(message_key, self.index);

        // Advance the chain
        self.key.zeroize();
        self.key = next_chain_key;
        self.index += 1;

        Ok(mk)
    }

    /// Get current chain index
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Clone at current state (for skipping keys)
    pub fn clone_at(&self) -> Self {
        Self {
            key: self.key,
            index: self.index,
        }
    }

    /// Get the raw key bytes (for serialization)
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    /// Skip ahead to a specific index, collecting skipped message keys
    pub fn skip_to(&mut self, target_index: u32, max_skip: usize) -> Result<Vec<MessageKey>> {
        if target_index < self.index {
            return Err(Error::crypto(format!(
                "Cannot skip backward: current={}, target={}",
                self.index, target_index
            )));
        }

        let skip_count = (target_index - self.index) as usize;
        if skip_count > max_skip {
            return Err(Error::crypto(format!(
                "Too many messages to skip: {} (max: {})",
                skip_count, max_skip
            )));
        }

        let mut skipped = Vec::with_capacity(skip_count);
        while self.index < target_index {
            skipped.push(self.derive_message_key()?);
        }

        Ok(skipped)
    }
}

impl std::fmt::Debug for ChainKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChainKey")
            .field("index", &self.index)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

/// Message Key - used to encrypt/decrypt a single message
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MessageKey {
    key: [u8; 32],
    #[zeroize(skip)]
    index: u32,
}

impl MessageKey {
    /// Create a message key from bytes
    pub fn new(key: [u8; 32], index: u32) -> Self {
        Self { key, index }
    }

    /// Get the encryption key
    pub fn encryption_key(&self) -> &[u8; 32] {
        &self.key
    }

    /// Derive AEAD key and nonce from message key
    pub fn derive_aead_params(&self) -> Result<([u8; 32], [u8; 12])> {
        let output = Hkdf::derive(Some(&self.key), b"aead", b"dchat-aead", 44)?;

        let mut aead_key = [0u8; 32];
        let mut nonce = [0u8; 12];
        aead_key.copy_from_slice(&output[0..32]);
        nonce.copy_from_slice(&output[32..44]);

        Ok((aead_key, nonce))
    }

    /// Get the message index
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Encrypt plaintext using this message key
    pub fn encrypt(&self, plaintext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>> {
        use aes_gcm::{
            aead::{Aead, KeyInit, Payload},
            Aes256Gcm, Nonce,
        };

        let (aead_key, nonce_bytes) = self.derive_aead_params()?;
        let cipher = Aes256Gcm::new((&aead_key).into());
        let nonce = Nonce::from_slice(&nonce_bytes);

        cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|e| Error::crypto(format!("Message encryption failed: {}", e)))
    }

    /// Decrypt ciphertext using this message key
    pub fn decrypt(&self, ciphertext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>> {
        use aes_gcm::{
            aead::{Aead, KeyInit, Payload},
            Aes256Gcm, Nonce,
        };

        let (aead_key, nonce_bytes) = self.derive_aead_params()?;
        let cipher = Aes256Gcm::new((&aead_key).into());
        let nonce = Nonce::from_slice(&nonce_bytes);

        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: associated_data,
                },
            )
            .map_err(|e| Error::crypto(format!("Message decryption failed: {}", e)))
    }
}

impl std::fmt::Debug for MessageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageKey")
            .field("index", &self.index)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_key_derivation() {
        let mut chain = ChainKey::new([1u8; 32], 0);

        // Derive multiple message keys
        let mk1 = chain.derive_message_key().unwrap();
        let mk2 = chain.derive_message_key().unwrap();
        let mk3 = chain.derive_message_key().unwrap();

        // Indices should increment
        assert_eq!(mk1.index(), 0);
        assert_eq!(mk2.index(), 1);
        assert_eq!(mk3.index(), 2);
        assert_eq!(chain.index(), 3);

        // Keys should be different
        assert_ne!(mk1.encryption_key(), mk2.encryption_key());
        assert_ne!(mk2.encryption_key(), mk3.encryption_key());
    }

    #[test]
    fn test_message_key_encryption() {
        let chain = ChainKey::new([42u8; 32], 0);
        let mut chain_clone = chain.clone_at();
        let mk = chain_clone.derive_message_key().unwrap();

        let plaintext = b"Hello, World!";
        let aad = b"header data";

        let ciphertext = mk.encrypt(plaintext, aad).unwrap();
        assert_ne!(&ciphertext[..], plaintext);

        // Decrypt with same key
        let mk2 = {
            let mut c = chain.clone_at();
            c.derive_message_key().unwrap()
        };
        let decrypted = mk2.decrypt(&ciphertext, aad).unwrap();
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_chain_skip() {
        let mut chain = ChainKey::new([1u8; 32], 0);

        // Skip to index 5
        let skipped = chain.skip_to(5, 100).unwrap();
        assert_eq!(skipped.len(), 5);
        assert_eq!(chain.index(), 5);

        // Skipped keys should have correct indices
        for (i, mk) in skipped.iter().enumerate() {
            assert_eq!(mk.index() as usize, i);
        }
    }

    #[test]
    fn test_chain_skip_limit() {
        let mut chain = ChainKey::new([1u8; 32], 0);

        // Should fail if skipping too many
        let result = chain.skip_to(1001, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_root_key_derivation() {
        let root_key = [1u8; 32];
        let dh_output = [2u8; 32];

        let (chain, new_root) = ChainKey::from_root_key(&root_key, &dh_output).unwrap();

        // New root should be different from old
        assert_ne!(new_root, root_key);

        // Chain should start at index 0
        assert_eq!(chain.index(), 0);
    }
}
