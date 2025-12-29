//! Digital signatures using Ed25519

use crate::keys::{PrivateKey, PublicKey};
use dchat_core::error::{Error, Result};
pub use ed25519_dalek::Signature as Ed25519Signature;
use ed25519_dalek::{
    Signer, SigningKey as Ed25519SigningKey, Verifier, VerifyingKey as Ed25519VerifyingKey,
};

/// A signing key for creating digital signatures
#[derive(Debug)]
pub struct SigningKey {
    inner: Ed25519SigningKey,
}

impl SigningKey {
    /// Create from a private key
    pub fn from_private_key(private_key: &PrivateKey) -> Self {
        let inner = Ed25519SigningKey::from_bytes(private_key.as_bytes());
        Self { inner }
    }

    /// Generate a new random signing key
    pub fn generate<R: rand::CryptoRng + rand::RngCore>(csprng: &mut R) -> Self {
        let inner = Ed25519SigningKey::generate(csprng);
        Self { inner }
    }

    /// Create from raw bytes
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let inner = Ed25519SigningKey::from_bytes(bytes);
        Self { inner }
    }

    /// Get the corresponding verifying key
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey {
            inner: self.inner.verifying_key(),
        }
    }

    /// Sign a message
    pub fn sign(&self, message: &[u8]) -> Signature {
        let sig = self.inner.sign(message);
        Signature {
            bytes: sig.to_bytes(),
        }
    }
}

/// A verifying key for verifying digital signatures
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyingKey {
    inner: Ed25519VerifyingKey,
}

impl VerifyingKey {
    /// Create from a public key
    pub fn from_public_key(public_key: &PublicKey) -> Result<Self> {
        let inner = Ed25519VerifyingKey::from_bytes(public_key.as_bytes())
            .map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?;

        Ok(Self { inner })
    }

    /// Create from raw bytes
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self> {
        let inner = Ed25519VerifyingKey::from_bytes(bytes)
            .map_err(|e| Error::crypto(format!("Invalid verifying key: {}", e)))?;
        Ok(Self { inner })
    }

    /// Verify a signature
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<()> {
        let sig = Ed25519Signature::from_bytes(&signature.bytes);
        self.inner
            .verify(message, &sig)
            .map_err(|e| Error::crypto(format!("Signature verification failed: {}", e)))
    }

    /// Get the raw bytes of the public key
    pub fn to_bytes(&self) -> [u8; 32] {
        self.inner.to_bytes()
    }

    /// Get a reference to the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.inner.as_bytes()
    }
}

/// A digital signature
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    bytes: [u8; 64],
}

impl Signature {
    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self { bytes }
    }

    /// Get the raw bytes
    pub fn to_bytes(&self) -> [u8; 64] {
        self.bytes
    }

    /// Convert to dchat-core Signature type
    pub fn to_core_signature(&self) -> dchat_core::types::Signature {
        dchat_core::types::Signature::new(self.bytes.to_vec())
    }
}

impl TryFrom<&dchat_core::types::Signature> for Signature {
    type Error = Error;

    fn try_from(sig: &dchat_core::types::Signature) -> Result<Self> {
        if sig.as_bytes().len() != 64 {
            return Err(Error::crypto("Invalid signature length"));
        }

        let mut bytes = [0u8; 64];
        bytes.copy_from_slice(sig.as_bytes());
        Ok(Self { bytes })
    }
}

/// Sign a message using a private key (convenience wrapper)
pub fn sign_with_private_key(private_key: &PrivateKey, message: &[u8]) -> Signature {
    let signing_key = SigningKey::from_private_key(private_key);
    signing_key.sign(message)
}

/// Sign a message using a signing key
pub fn sign(message: &[u8], signing_key: &SigningKey) -> Ed25519Signature {
    signing_key.inner.sign(message)
}

/// Verify a signature using an ed25519 signature and verifying key
pub fn verify(
    message: &[u8],
    signature: &Ed25519Signature,
    verifying_key: &VerifyingKey,
) -> Result<()> {
    verifying_key
        .inner
        .verify(message, signature)
        .map_err(|e| Error::crypto(format!("Signature verification failed: {}", e)))
}

/// Verify a signature using a public key (convenience wrapper)
pub fn verify_with_public_key(
    public_key: &PublicKey,
    message: &[u8],
    signature: &Signature,
) -> Result<()> {
    let verifying_key = VerifyingKey::from_public_key(public_key)?;
    verifying_key.verify(message, signature)
}

/// Sign a message and return both signature and signing public key
pub fn sign_with_key(private_key: &PrivateKey, message: &[u8]) -> (Signature, PublicKey) {
    let signing_key = SigningKey::from_private_key(private_key);
    let signature = signing_key.sign(message);
    let public_key = private_key.public_key();
    (signature, public_key)
}

/// A helper for batch signature verification
pub struct BatchVerifier {
    verifications: Vec<(PublicKey, Vec<u8>, Signature)>,
}

impl BatchVerifier {
    /// Create a new batch verifier
    pub fn new() -> Self {
        Self {
            verifications: Vec::new(),
        }
    }

    /// Add a signature to verify
    pub fn add(&mut self, public_key: PublicKey, message: Vec<u8>, signature: Signature) {
        self.verifications.push((public_key, message, signature));
    }

    /// Verify all signatures in the batch
    pub fn verify_all(self) -> Result<()> {
        for (public_key, message, signature) in self.verifications {
            verify_with_public_key(&public_key, &message, &signature)?;
        }
        Ok(())
    }

    /// Get the number of signatures to verify
    pub fn len(&self) -> usize {
        self.verifications.len()
    }

    /// Check if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.verifications.is_empty()
    }
}

impl Default for BatchVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyPair;

    #[test]
    #[allow(deprecated)]
    fn test_sign_and_verify() {
        let keypair = KeyPair::generate();
        let message = b"Hello, world!";

        // Sign message
        let signature = sign_with_private_key(keypair.private_key(), message);

        // Verify signature
        assert!(verify_with_public_key(keypair.public_key(), message, &signature).is_ok());

        // Verify with wrong message should fail
        let wrong_message = b"Wrong message";
        assert!(verify_with_public_key(keypair.public_key(), wrong_message, &signature).is_err());

        // Verify with wrong key should fail
        let wrong_keypair = KeyPair::generate();
        assert!(verify_with_public_key(wrong_keypair.public_key(), message, &signature).is_err());
    }

    #[test]
    #[allow(deprecated)]
    fn test_batch_verification() {
        let mut batch = BatchVerifier::new();

        // Add multiple signatures
        for i in 0..5 {
            let keypair = KeyPair::generate();
            let message = format!("Message {}", i).into_bytes();
            let signature = sign_with_private_key(keypair.private_key(), &message);

            batch.add(keypair.public_key().clone(), message, signature);
        }

        // Verify all at once
        assert!(batch.verify_all().is_ok());
    }

    #[test]
    #[allow(deprecated)]
    fn test_signature_serialization() {
        let keypair = KeyPair::generate();
        let message = b"Test message";
        let signature = sign_with_private_key(keypair.private_key(), message);

        // Convert to core type and back
        let core_sig = signature.to_core_signature();
        let recovered_sig = Signature::try_from(&core_sig).unwrap();

        assert_eq!(signature, recovered_sig);

        // Verify recovered signature
        assert!(verify_with_public_key(keypair.public_key(), message, &recovered_sig).is_ok());
    }
}
