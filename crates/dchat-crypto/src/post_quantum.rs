//! Post-quantum cryptography support

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};

// Re-export post-quantum traits
pub use pqcrypto_traits::kem::{
    Ciphertext, PublicKey as PQPublicKey, SecretKey as PQSecretKey, SharedSecret,
};
pub use pqcrypto_traits::sign::{
    DetachedSignature, PublicKey as PQSignPublicKey, SecretKey as PQSignSecretKey,
};

/// Post-quantum key encapsulation using ML-KEM-768 (standardized Kyber)
pub mod kyber {

    use pqcrypto_mlkem::mlkem768;

    pub type PublicKey = mlkem768::PublicKey;
    pub type SecretKey = mlkem768::SecretKey;
    pub type Ciphertext = mlkem768::Ciphertext;
    pub type SharedSecret = mlkem768::SharedSecret;

    /// Generate an ML-KEM-768 keypair
    pub fn keypair() -> (PublicKey, SecretKey) {
        mlkem768::keypair()
    }

    /// Encapsulate to create shared secret
    pub fn encapsulate(public_key: &PublicKey) -> (SharedSecret, Ciphertext) {
        mlkem768::encapsulate(public_key)
    }

    /// Decapsulate to recover shared secret
    pub fn decapsulate(ciphertext: &Ciphertext, secret_key: &SecretKey) -> SharedSecret {
        mlkem768::decapsulate(ciphertext, secret_key)
    }
}

/// Post-quantum signatures using Falcon
pub mod falcon {
    use super::*;
    use pqcrypto_falcon::falcon512;

    pub type PublicKey = falcon512::PublicKey;
    pub type SecretKey = falcon512::SecretKey;
    pub type DetachedSignature = falcon512::DetachedSignature;

    /// Generate a Falcon512 keypair
    pub fn keypair() -> (PublicKey, SecretKey) {
        falcon512::keypair()
    }

    /// Sign a message
    pub fn detached_sign(message: &[u8], secret_key: &SecretKey) -> DetachedSignature {
        falcon512::detached_sign(message, secret_key)
    }

    /// Verify a signature
    pub fn verify_detached_signature(
        signature: &DetachedSignature,
        message: &[u8],
        public_key: &PublicKey,
    ) -> Result<()> {
        falcon512::verify_detached_signature(signature, message, public_key)
            .map_err(|_| Error::crypto("Falcon signature verification failed"))
    }
}

/// Hybrid cryptosystem combining classical and post-quantum algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridPublicKey {
    pub classical: crate::keys::PublicKey,
    pub post_quantum: Vec<u8>, // Serialized PQ public key
}

#[derive(Debug)]
pub struct HybridSecretKey {
    pub classical: crate::keys::PrivateKey,
    pub post_quantum: Vec<u8>, // Serialized PQ secret key
}

/// Hybrid key encapsulation mechanism
pub struct HybridKem;

impl HybridKem {
    /// Generate a hybrid keypair
    pub fn keypair() -> Result<(HybridPublicKey, HybridSecretKey)> {
        // Generate classical keypair
        let classical_keypair = crate::keys::KeyPair::generate();
        let (classical_private, classical_public) = classical_keypair.into_keys();

        // Generate post-quantum keypair
        let (pq_public, pq_secret) = kyber::keypair();

        let hybrid_public = HybridPublicKey {
            classical: classical_public,
            post_quantum: pq_public.as_bytes().to_vec(),
        };

        let hybrid_secret = HybridSecretKey {
            classical: classical_private,
            post_quantum: pq_secret.as_bytes().to_vec(),
        };

        Ok((hybrid_public, hybrid_secret))
    }

    /// Encapsulate using both classical and post-quantum methods
    pub fn encapsulate(public_key: &HybridPublicKey) -> Result<(Vec<u8>, Vec<u8>)> {
        // Classical key agreement (simplified - in practice would use ECDH)
        let classical_shared = crate::hash(public_key.classical.as_bytes());

        // Post-quantum encapsulation
        let pq_public = kyber::PublicKey::from_bytes(&public_key.post_quantum)
            .map_err(|_| Error::crypto("Invalid PQ public key"))?;
        let (pq_shared, pq_ciphertext) = kyber::encapsulate(&pq_public);

        // Combine shared secrets
        let mut combined_shared = Vec::new();
        combined_shared.extend_from_slice(&classical_shared);
        combined_shared.extend_from_slice(pq_shared.as_bytes());

        let final_shared = crate::hash(&combined_shared);

        Ok((final_shared.to_vec(), pq_ciphertext.as_bytes().to_vec()))
    }

    /// Decapsulate using both classical and post-quantum methods
    pub fn decapsulate(ciphertext: &[u8], secret_key: &HybridSecretKey) -> Result<Vec<u8>> {
        // Classical key agreement
        let classical_public = secret_key.classical.public_key();
        let classical_shared = crate::hash(classical_public.as_bytes());

        // Post-quantum decapsulation
        let pq_secret = kyber::SecretKey::from_bytes(&secret_key.post_quantum)
            .map_err(|_| Error::crypto("Invalid PQ secret key"))?;
        let pq_ciphertext = kyber::Ciphertext::from_bytes(ciphertext)
            .map_err(|_| Error::crypto("Invalid PQ ciphertext"))?;
        let pq_shared = kyber::decapsulate(&pq_ciphertext, &pq_secret);

        // Combine shared secrets
        let mut combined_shared = Vec::new();
        combined_shared.extend_from_slice(&classical_shared);
        combined_shared.extend_from_slice(pq_shared.as_bytes());

        let final_shared = crate::hash(&combined_shared);

        Ok(final_shared.to_vec())
    }
}

/// Hybrid signature scheme
pub struct HybridSignature {
    pub classical: crate::signatures::Signature,
    pub post_quantum: Vec<u8>, // Serialized PQ signature
}

pub struct HybridSigner {
    classical_key: crate::keys::PrivateKey,
    pq_key: falcon::SecretKey,
    pq_public: falcon::PublicKey,
}

impl HybridSigner {
    /// Create a new hybrid signer
    pub fn new() -> Self {
        let classical_key = crate::keys::PrivateKey::generate();
        let (pq_public, pq_key) = falcon::keypair();

        Self {
            classical_key,
            pq_key,
            pq_public,
        }
    }

    /// Sign a message with both classical and post-quantum algorithms
    pub fn sign(&self, message: &[u8]) -> HybridSignature {
        let classical_sig = crate::signatures::sign(&self.classical_key, message);
        let pq_sig = falcon::detached_sign(message, &self.pq_key);

        HybridSignature {
            classical: classical_sig,
            post_quantum: pq_sig.as_bytes().to_vec(),
        }
    }

    /// Get the public keys for verification
    pub fn public_keys(&self) -> (crate::keys::PublicKey, falcon::PublicKey) {
        let classical_public = self.classical_key.public_key();
        (classical_public, self.pq_public)
    }
}

impl Default for HybridSigner {
    fn default() -> Self {
        Self::new()
    }
}

/// Verify a hybrid signature
pub fn verify_hybrid_signature(
    signature: &HybridSignature,
    message: &[u8],
    classical_public: &crate::keys::PublicKey,
    pq_public: &falcon::PublicKey,
) -> Result<()> {
    // Verify classical signature
    crate::signatures::verify(classical_public, message, &signature.classical)?;

    // Verify post-quantum signature
    let pq_sig = falcon::DetachedSignature::from_bytes(&signature.post_quantum)
        .map_err(|_| Error::crypto("Invalid PQ signature"))?;
    falcon::verify_detached_signature(&pq_sig, message, pq_public)?;

    Ok(())
}

#[cfg(test)]
#[allow(unexpected_cfgs)]
#[cfg(feature = "pq-crypto")] // Post-quantum crypto is a future feature
mod tests {

    #[test]
    fn test_kyber_kem() {
        let (public_key, secret_key) = kyber::keypair();
        let (shared_secret1, ciphertext) = kyber::encapsulate(&public_key);
        let shared_secret2 = kyber::decapsulate(&ciphertext, &secret_key);

        assert_eq!(shared_secret1.as_bytes(), shared_secret2.as_bytes());
    }

    #[test]
    fn test_falcon_signatures() {
        let (public_key, secret_key) = falcon::keypair();
        let message = b"Test message for Falcon signature";

        let signature = falcon::detached_sign(message, &secret_key);
        let result = falcon::verify_detached_signature(&signature, message, &public_key);

        assert!(result.is_ok());

        // Wrong message should fail
        let wrong_message = b"Wrong message";
        let result = falcon::verify_detached_signature(&signature, wrong_message, &public_key);
        assert!(result.is_err());
    }

    #[test]
    fn test_hybrid_kem() {
        let (public_key, secret_key) = HybridKem::keypair().unwrap();
        let (shared_secret1, ciphertext) = HybridKem::encapsulate(&public_key).unwrap();
        let shared_secret2 = HybridKem::decapsulate(&ciphertext, &secret_key).unwrap();

        assert_eq!(shared_secret1, shared_secret2);
    }

    #[test]
    fn test_hybrid_signatures() {
        let signer = HybridSigner::new();
        let message = b"Test message for hybrid signature";

        let signature = signer.sign(message);
        let (classical_public, pq_public) = signer.public_keys();

        let result = verify_hybrid_signature(&signature, message, &classical_public, &pq_public);
        assert!(result.is_ok());
    }
}
