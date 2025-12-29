//! dchat-crypto: Cryptographic primitives for dchat
//!
//! This crate provides the cryptographic foundation for dchat, including:
//! - Noise Protocol implementation for end-to-end encryption
//! - Key management and rotation
//! - Digital signatures
//! - Post-quantum cryptography support
//! - Zero-knowledge proofs
//! - BIP-39 mnemonic seed phrases

pub mod crypto; // Additional crypto modules (handshake, versioning)
mod encryption;
pub mod handshake;
pub mod kdf;
pub mod keys;
pub mod kms; // AWS KMS integration for secure key management
pub mod mnemonic; // BIP-39 mnemonic seed phrases
pub mod noise;
pub mod post_quantum;
pub mod ratchet; // Double Ratchet protocol for E2E encryption
pub mod rotation;
pub mod signatures;
pub mod suk; // Storage Unlock Key for Quorum-Gated Encryption
pub mod typed_handshake; // Typed state machine handshake with identity binding

// Re-export crypto submodule contents
pub use crypto::handshake as crypto_handshake;
pub use crypto::versioning as crypto_versioning;

pub use encryption::{
    decrypt_with_key, decrypt_with_password, encrypt_with_key, encrypt_with_password,
    generate_encryption_key, EncryptedData, KEY_SIZE, NONCE_SIZE,
};
pub use keys::{Address, KeyPair, PrivateKey, PublicKey as CryptoPublicKey};
pub use kms::{AwsKmsClient, Ed25519KmsWrapper, KmsError, KmsKeyType}; // Re-export KMS types
pub use mnemonic::{Mnemonic, MnemonicLength, Seed}; // BIP-39 mnemonic exports
pub use noise::{NoiseHandshake, NoiseSession};
pub use rotation::{KeyRotationManager, RotationPolicy};
pub use signatures::{
    sign, sign_with_private_key, verify, verify_with_public_key, SigningKey, VerifyingKey,
};
pub use suk::{
    EncryptedSuk, EpochToken, StorageUnlockKey, SukManager, UnlockKey, WrappedMessageKey,
    EPOCH_TOKEN_TTL_SECONDS, SUK_SIZE,
}; // Quorum-Gated Encryption exports
pub use typed_handshake::{
    HandshakeFailure, HandshakeMessage, HandshakePhase, HandshakeRejectReason, HandshakeRole,
    IdentityClaim, ProtocolVersion, TimeoutAwareHandshake, TypedHandshake, VerifiedPeerIdentity,
};

use dchat_core::error::{Error, Result};

/// Generate a secure random seed
pub fn generate_seed() -> [u8; 32] {
    use rand::RngCore;
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    seed
}

/// Secure hash function (BLAKE3)
pub fn hash(data: &[u8]) -> [u8; 32] {
    blake3::hash(data).into()
}

/// Secure hash function with custom output length
pub fn hash_with_length(data: &[u8], length: usize) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(data);
    let mut output = vec![0u8; length];
    hasher.finalize_xof().fill(&mut output);
    output
}

/// Constant-time comparison of byte arrays
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    constant_time_eq::constant_time_eq(a, b)
}

/// Derive a key from a password using Argon2
pub fn derive_key_from_password(
    password: &str,
    salt: &[u8; 16],
    output_length: usize,
) -> Result<Vec<u8>> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    use argon2::Argon2;
    use base64::engine::general_purpose;
    use base64::Engine;

    let salt_string = SaltString::encode_b64(salt)
        .map_err(|e| Error::crypto(format!("Salt encoding error: {}", e)))?;

    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt_string)
        .map_err(|e| Error::crypto(format!("Argon2 error: {}", e)))?
        .to_string();

    // Extract the hash portion and truncate to requested length
    let hash_part = password_hash
        .split('$')
        .next_back()
        .ok_or_else(|| Error::crypto("Invalid hash format".to_string()))?;

    // Add padding if necessary
    let mut hash_b64 = hash_part.to_string();
    while hash_b64.len() % 4 != 0 {
        hash_b64.push('=');
    }

    let decoded = general_purpose::STANDARD
        .decode(&hash_b64)
        .map_err(|e| Error::crypto(format!("Decode error: {}", e)))?;

    Ok(decoded[..output_length.min(decoded.len())].to_vec())
}
