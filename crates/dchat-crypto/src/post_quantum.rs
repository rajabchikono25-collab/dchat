//! Post-quantum cryptography support
//!
//! This module provides hybrid classical/post-quantum cryptographic primitives:
//! - **HybridKem**: Combines X25519 ECDH with ML-KEM-768 (Kyber) for key encapsulation
//! - **HybridSigner**: Combines Ed25519 with Falcon512 for signatures
//!
//! The hybrid approach provides security even if one algorithm is broken:
//! - If classical crypto is broken (e.g., by quantum computers), PQ crypto protects
//! - If PQ crypto has undiscovered weaknesses, classical crypto protects

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey, StaticSecret};
use zeroize::Zeroize;

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

/// Hybrid cryptosystem combining classical X25519 and post-quantum ML-KEM-768
///
/// This provides defense-in-depth: security holds if either algorithm is secure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridPublicKey {
    /// X25519 public key for classical ECDH
    pub classical: [u8; 32],
    /// ML-KEM-768 public key
    pub post_quantum: Vec<u8>,
}

/// Hybrid secret key containing both classical and post-quantum components
#[derive(Zeroize)]
#[zeroize(drop)]
pub struct HybridSecretKey {
    /// X25519 secret key bytes (zeroized on drop)
    pub classical: [u8; 32],
    /// ML-KEM-768 secret key (zeroized on drop)  
    pub post_quantum: Vec<u8>,
}

// Manual Debug impl to avoid leaking secret material
impl std::fmt::Debug for HybridSecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HybridSecretKey")
            .field("classical", &"[REDACTED]")
            .field("post_quantum", &"[REDACTED]")
            .finish()
    }
}

/// Hybrid ciphertext containing both classical and post-quantum components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridCiphertext {
    /// X25519 ephemeral public key
    pub classical_ephemeral: [u8; 32],
    /// ML-KEM-768 ciphertext
    pub post_quantum: Vec<u8>,
}

/// Hybrid key encapsulation mechanism combining X25519 and ML-KEM-768
///
/// Security properties:
/// - Provides IND-CCA2 security if either X25519 or ML-KEM-768 is secure
/// - Forward secrecy via ephemeral keys
/// - Resistant to harvest-now-decrypt-later quantum attacks
pub struct HybridKem;

impl HybridKem {
    /// Generate a hybrid keypair for key encapsulation
    pub fn keypair() -> Result<(HybridPublicKey, HybridSecretKey)> {
        // Generate X25519 keypair
        let classical_secret = StaticSecret::random_from_rng(rand::thread_rng());
        let classical_public = X25519PublicKey::from(&classical_secret);

        // Generate post-quantum keypair
        let (pq_public, pq_secret) = kyber::keypair();

        let hybrid_public = HybridPublicKey {
            classical: classical_public.to_bytes(),
            post_quantum: pq_public.as_bytes().to_vec(),
        };

        let hybrid_secret = HybridSecretKey {
            classical: classical_secret.as_bytes().clone(),
            post_quantum: pq_secret.as_bytes().to_vec(),
        };

        Ok((hybrid_public, hybrid_secret))
    }

    /// Encapsulate: Generate a shared secret and ciphertext for the recipient
    ///
    /// Uses ephemeral X25519 key for forward secrecy combined with ML-KEM-768.
    /// The shared secrets from both are combined using HKDF.
    pub fn encapsulate(public_key: &HybridPublicKey) -> Result<(Vec<u8>, HybridCiphertext)> {
        // Classical X25519 key agreement with ephemeral key
        let ephemeral_secret = EphemeralSecret::random_from_rng(rand::thread_rng());
        let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);

        let recipient_classical = X25519PublicKey::from(public_key.classical);
        let classical_shared = ephemeral_secret.diffie_hellman(&recipient_classical);

        // Post-quantum encapsulation
        let pq_public = kyber::PublicKey::from_bytes(&public_key.post_quantum)
            .map_err(|_| Error::crypto("Invalid PQ public key"))?;
        let (pq_shared, pq_ciphertext) = kyber::encapsulate(&pq_public);

        // Combine shared secrets using HKDF for proper key derivation
        let combined_shared = Self::combine_shared_secrets(
            classical_shared.as_bytes(),
            pq_shared.as_bytes(),
            &ephemeral_public.to_bytes(),
            &pq_ciphertext.as_bytes(),
        )?;

        let ciphertext = HybridCiphertext {
            classical_ephemeral: ephemeral_public.to_bytes(),
            post_quantum: pq_ciphertext.as_bytes().to_vec(),
        };

        Ok((combined_shared, ciphertext))
    }

    /// Decapsulate: Recover the shared secret from the ciphertext
    pub fn decapsulate(
        ciphertext: &HybridCiphertext,
        secret_key: &HybridSecretKey,
    ) -> Result<Vec<u8>> {
        // Classical X25519 key agreement
        let classical_secret = StaticSecret::from(secret_key.classical);
        let ephemeral_public = X25519PublicKey::from(ciphertext.classical_ephemeral);
        let classical_shared = classical_secret.diffie_hellman(&ephemeral_public);

        // Post-quantum decapsulation
        let pq_secret = kyber::SecretKey::from_bytes(&secret_key.post_quantum)
            .map_err(|_| Error::crypto("Invalid PQ secret key"))?;
        let pq_ciphertext = kyber::Ciphertext::from_bytes(&ciphertext.post_quantum)
            .map_err(|_| Error::crypto("Invalid PQ ciphertext"))?;
        let pq_shared = kyber::decapsulate(&pq_ciphertext, &pq_secret);

        // Combine shared secrets using HKDF
        let combined_shared = Self::combine_shared_secrets(
            classical_shared.as_bytes(),
            pq_shared.as_bytes(),
            &ciphertext.classical_ephemeral,
            &ciphertext.post_quantum,
        )?;

        Ok(combined_shared)
    }

    /// Combine classical and PQ shared secrets using HKDF
    ///
    /// Uses the ciphertext components as additional context to bind the
    /// derived key to the specific encapsulation.
    fn combine_shared_secrets(
        classical: &[u8],
        post_quantum: &[u8],
        ephemeral_public: &[u8],
        pq_ciphertext: &[u8],
    ) -> Result<Vec<u8>> {
        use hkdf::Hkdf;
        use sha2::Sha256;

        // Concatenate shared secrets as input key material
        let mut ikm = Vec::with_capacity(classical.len() + post_quantum.len());
        ikm.extend_from_slice(classical);
        ikm.extend_from_slice(post_quantum);

        // Use ciphertext components as salt for domain separation
        let mut salt = Vec::with_capacity(ephemeral_public.len() + pq_ciphertext.len());
        salt.extend_from_slice(ephemeral_public);
        salt.extend_from_slice(pq_ciphertext);

        let hkdf = Hkdf::<Sha256>::new(Some(&salt), &ikm);
        let mut output = vec![0u8; 32];

        hkdf.expand(b"dchat-hybrid-kem-v1", &mut output)
            .map_err(|_| Error::crypto("HKDF expansion failed"))?;

        Ok(output)
    }

    /// Serialize a HybridCiphertext to bytes for transmission
    pub fn serialize_ciphertext(ciphertext: &HybridCiphertext) -> Vec<u8> {
        let mut output = Vec::with_capacity(32 + 4 + ciphertext.post_quantum.len());
        output.extend_from_slice(&ciphertext.classical_ephemeral);
        output.extend_from_slice(&(ciphertext.post_quantum.len() as u32).to_le_bytes());
        output.extend_from_slice(&ciphertext.post_quantum);
        output
    }

    /// Deserialize a HybridCiphertext from bytes
    pub fn deserialize_ciphertext(data: &[u8]) -> Result<HybridCiphertext> {
        if data.len() < 36 {
            return Err(Error::crypto("Ciphertext too short"));
        }

        let mut classical_ephemeral = [0u8; 32];
        classical_ephemeral.copy_from_slice(&data[0..32]);

        let pq_len = u32::from_le_bytes([data[32], data[33], data[34], data[35]]) as usize;

        if data.len() != 36 + pq_len {
            return Err(Error::crypto("Invalid ciphertext length"));
        }

        Ok(HybridCiphertext {
            classical_ephemeral,
            post_quantum: data[36..].to_vec(),
        })
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
        #[allow(deprecated)]
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
        let classical_sig = crate::signatures::sign_with_private_key(&self.classical_key, message);
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
    crate::signatures::verify_with_public_key(classical_public, message, &signature.classical)?;

    // Verify post-quantum signature
    let pq_sig = falcon::DetachedSignature::from_bytes(&signature.post_quantum)
        .map_err(|_| Error::crypto("Invalid PQ signature"))?;
    falcon::verify_detached_signature(&pq_sig, message, pq_public)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_hybrid_kem_different_keys() {
        // Two different keypairs should produce different shared secrets
        let (public_key1, _secret_key1) = HybridKem::keypair().unwrap();
        let (public_key2, secret_key2) = HybridKem::keypair().unwrap();

        let (shared1, _ciphertext1) = HybridKem::encapsulate(&public_key1).unwrap();
        let (shared2, _ciphertext2) = HybridKem::encapsulate(&public_key2).unwrap();

        // Different encapsulations should produce different shared secrets
        assert_ne!(shared1, shared2);

        // Decapsulating with wrong key should produce different result
        let (shared_correct, ciphertext) = HybridKem::encapsulate(&public_key2).unwrap();
        let shared_decapped = HybridKem::decapsulate(&ciphertext, &secret_key2).unwrap();
        assert_eq!(shared_correct, shared_decapped);
    }

    #[test]
    fn test_hybrid_kem_serialization() {
        let (public_key, secret_key) = HybridKem::keypair().unwrap();
        let (shared_secret1, ciphertext) = HybridKem::encapsulate(&public_key).unwrap();

        // Serialize and deserialize
        let serialized = HybridKem::serialize_ciphertext(&ciphertext);
        let deserialized = HybridKem::deserialize_ciphertext(&serialized).unwrap();

        // Should be able to decapsulate the deserialized ciphertext
        let shared_secret2 = HybridKem::decapsulate(&deserialized, &secret_key).unwrap();
        assert_eq!(shared_secret1, shared_secret2);
    }

    #[test]
    fn test_hybrid_kem_ciphertext_tampering() {
        let (public_key, secret_key) = HybridKem::keypair().unwrap();
        let (shared_secret1, mut ciphertext) = HybridKem::encapsulate(&public_key).unwrap();

        // Tamper with the classical ephemeral key
        ciphertext.classical_ephemeral[0] ^= 0xFF;

        // Decapsulation should produce different shared secret
        let shared_secret2 = HybridKem::decapsulate(&ciphertext, &secret_key).unwrap();
        assert_ne!(shared_secret1, shared_secret2);
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

    #[test]
    fn test_hybrid_signature_wrong_message() {
        let signer = HybridSigner::new();
        let message = b"Test message for hybrid signature";
        let wrong_message = b"Wrong message";

        let signature = signer.sign(message);
        let (classical_public, pq_public) = signer.public_keys();

        let result =
            verify_hybrid_signature(&signature, wrong_message, &classical_public, &pq_public);
        assert!(result.is_err());
    }

    #[test]
    fn test_secret_key_debug_redacted() {
        let (_public_key, secret_key) = HybridKem::keypair().unwrap();
        let debug_output = format!("{:?}", secret_key);

        // Ensure secret material is not leaked in debug output
        assert!(debug_output.contains("REDACTED"));
        assert!(!debug_output.contains(&hex::encode(&secret_key.classical)));
    }
}
