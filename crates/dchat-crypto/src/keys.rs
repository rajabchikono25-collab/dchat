//! Key management and generation

use dchat_core::error::{Error, Result};
use ed25519_dalek::{SigningKey as Ed25519SigningKey, VerifyingKey as Ed25519VerifyingKey};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A private key that automatically zeros itself when dropped
#[derive(Clone, ZeroizeOnDrop, Zeroize)]
pub struct PrivateKey {
    bytes: [u8; 32],
}

impl PrivateKey {
    /// Generate a new random private key
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        Self { bytes }
    }

    /// Create from existing bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Get the raw bytes (use carefully)
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Derive the corresponding public key
    pub fn public_key(&self) -> PublicKey {
        let signing_key = Ed25519SigningKey::from_bytes(&self.bytes);
        let verifying_key = signing_key.verifying_key();
        PublicKey {
            bytes: verifying_key.to_bytes(),
        }
    }
}

impl std::fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivateKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// A public key
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKey {
    bytes: [u8; 32],
}

impl PublicKey {
    /// Create from existing bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Convert to dchat-core PublicKey type
    pub fn to_core_public_key(&self) -> dchat_core::types::PublicKey {
        dchat_core::types::PublicKey::new(self.bytes.to_vec())
    }
}

impl From<Ed25519VerifyingKey> for PublicKey {
    fn from(key: Ed25519VerifyingKey) -> Self {
        Self {
            bytes: key.to_bytes(),
        }
    }
}

impl TryFrom<&dchat_core::types::PublicKey> for PublicKey {
    type Error = Error;

    fn try_from(key: &dchat_core::types::PublicKey) -> Result<Self> {
        if key.as_bytes().len() != 32 {
            return Err(Error::crypto("Invalid public key length"));
        }

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(key.as_bytes());
        Ok(Self { bytes })
    }
}

/// A keypair containing both private and public keys
#[derive(Debug)]
pub struct KeyPair {
    private_key: PrivateKey,
    public_key: PublicKey,
}

impl KeyPair {
    /// Generate a new random keypair
    pub fn generate() -> Self {
        let private_key = PrivateKey::generate();
        let public_key = private_key.public_key();

        Self {
            private_key,
            public_key,
        }
    }

    /// Create from existing private key
    pub fn from_private_key(private_key: PrivateKey) -> Self {
        let public_key = private_key.public_key();
        Self {
            private_key,
            public_key,
        }
    }

    /// Get the private key
    pub fn private_key(&self) -> &PrivateKey {
        &self.private_key
    }

    /// Get the public key
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Split into private and public keys
    pub fn into_keys(self) -> (PrivateKey, PublicKey) {
        (self.private_key, self.public_key)
    }
}

/// Derive keys using BIP-32 style hierarchical deterministic key derivation
pub struct KeyDerivation;

impl KeyDerivation {
    /// Derive a child private key from a parent key and index
    pub fn derive_private_key(parent_key: &PrivateKey, index: u32) -> Result<PrivateKey> {
        let mut input = Vec::with_capacity(36);
        input.extend_from_slice(parent_key.as_bytes());
        input.extend_from_slice(&index.to_be_bytes());

        let derived = crate::hash(&input);
        Ok(PrivateKey::from_bytes(derived))
    }

    /// Derive multiple child keys from a parent key
    pub fn derive_key_path(master_key: &PrivateKey, path: &[u32]) -> Result<PrivateKey> {
        let mut current_key = master_key.clone();

        for &index in path {
            current_key = Self::derive_private_key(&current_key, index)?;
        }

        Ok(current_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = KeyPair::generate();

        // Public key should be derivable from private key
        let derived_public = keypair.private_key().public_key();
        assert_eq!(*keypair.public_key(), derived_public);
    }

    #[test]
    fn test_key_derivation() {
        let master_key = PrivateKey::generate();

        // Derive the same child key twice
        let child1 = KeyDerivation::derive_private_key(&master_key, 0).unwrap();
        let child2 = KeyDerivation::derive_private_key(&master_key, 0).unwrap();

        // Should be identical
        assert_eq!(child1.as_bytes(), child2.as_bytes());

        // Different indices should produce different keys
        let child3 = KeyDerivation::derive_private_key(&master_key, 1).unwrap();
        assert_ne!(child1.as_bytes(), child3.as_bytes());
    }

    #[test]
    fn test_hierarchical_derivation() {
        let master_key = PrivateKey::generate();
        let path = [44, 0, 0, 0]; // BIP-44 style path

        let derived = KeyDerivation::derive_key_path(&master_key, &path).unwrap();

        // Manual derivation should match
        let step1 = KeyDerivation::derive_private_key(&master_key, 44).unwrap();
        let step2 = KeyDerivation::derive_private_key(&step1, 0).unwrap();
        let step3 = KeyDerivation::derive_private_key(&step2, 0).unwrap();
        let step4 = KeyDerivation::derive_private_key(&step3, 0).unwrap();

        assert_eq!(derived.as_bytes(), step4.as_bytes());
    }
}
