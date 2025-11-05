//! Password-based encryption utilities
//!
//! This module provides authenticated encryption using AES-256-GCM
//! with Argon2 key derivation for password-based encryption.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2, PasswordHash, PasswordVerifier,
};
use dchat_core::error::{Error, Result};
use rand::RngCore;

/// Encrypted data container with metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EncryptedData {
    /// Format version for future compatibility
    pub version: u8,
    /// Argon2 salt (16 bytes)
    pub salt: [u8; 16],
    /// AES-GCM nonce (12 bytes)
    pub nonce: [u8; 12],
    /// Encrypted data with authentication tag
    pub ciphertext: Vec<u8>,
    /// Argon2 parameters hash for verification
    pub argon2_hash: String,
}

impl EncryptedData {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| Error::crypto(format!("Failed to serialize encrypted data: {}", e)))
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data)
            .map_err(|e| Error::crypto(format!("Failed to deserialize encrypted data: {}", e)))
    }
}

/// Encrypt data with a password using AES-256-GCM and Argon2 key derivation
///
/// # Security Properties
/// - Key derivation: Argon2id with default parameters (secure against GPU attacks)
/// - Encryption: AES-256-GCM (authenticated encryption, NIST approved)
/// - Random salt and nonce for each encryption (prevents rainbow tables)
/// - Authentication tag prevents tampering
///
/// # Arguments
/// * `password` - User password for encryption
/// * `plaintext` - Data to encrypt
///
/// # Returns
/// `EncryptedData` containing version, salt, nonce, ciphertext, and hash
pub fn encrypt_with_password(password: &str, plaintext: &[u8]) -> Result<EncryptedData> {
    // Generate random salt for Argon2
    let mut salt_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| Error::crypto(format!("Salt encoding failed: {}", e)))?;

    // Derive encryption key using Argon2id
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| Error::crypto(format!("Argon2 key derivation failed: {}", e)))?;

    // Extract 32-byte key from hash
    let hash_string = password_hash.to_string();
    let key_bytes = extract_key_from_hash(&hash_string)?;

    // Generate random nonce for AES-GCM
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    // Create nonce from array
    let nonce = Nonce::from(nonce_bytes);

    // Create cipher and encrypt
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| Error::crypto(format!("Cipher initialization failed: {}", e)))?;

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| Error::crypto(format!("Encryption failed: {}", e)))?;

    Ok(EncryptedData {
        version: 1,
        salt: salt_bytes,
        nonce: nonce_bytes,
        ciphertext,
        argon2_hash: hash_string,
    })
}

/// Decrypt data with a password
///
/// # Arguments
/// * `password` - User password for decryption
/// * `encrypted` - Encrypted data container
///
/// # Returns
/// Decrypted plaintext bytes
///
/// # Errors
/// Returns error if:
/// - Password is incorrect
/// - Data has been tampered with (authentication fails)
/// - Unsupported version
pub fn decrypt_with_password(password: &str, encrypted: &EncryptedData) -> Result<Vec<u8>> {
    // Check version
    if encrypted.version != 1 {
        return Err(Error::crypto(format!(
            "Unsupported encryption version: {}",
            encrypted.version
        )));
    }

    // Verify password and derive key
    let parsed_hash = PasswordHash::new(&encrypted.argon2_hash)
        .map_err(|e| Error::crypto(format!("Invalid password hash: {}", e)))?;

    let argon2 = Argon2::default();
    argon2
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| Error::crypto("Incorrect password".to_string()))?;

    // Extract key from verified hash
    let key_bytes = extract_key_from_hash(&encrypted.argon2_hash)?;

    // Create cipher and decrypt
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| Error::crypto(format!("Cipher initialization failed: {}", e)))?;

    // Create nonce from array
    let nonce = Nonce::from(encrypted.nonce);
    let plaintext = cipher
        .decrypt(&nonce, encrypted.ciphertext.as_ref())
        .map_err(|_| {
            Error::crypto("Decryption failed: data may be corrupted or tampered with".to_string())
        })?;

    Ok(plaintext)
}

/// Extract 32-byte encryption key from Argon2 hash string
fn extract_key_from_hash(hash_string: &str) -> Result<Vec<u8>> {
    use base64::engine::general_purpose;
    use base64::Engine;

    // Argon2 hash format: $argon2id$v=19$m=19456,t=2,p=1$SALT$HASH
    // The hash portion uses standard base64 (not URL-safe)
    let parts: Vec<&str> = hash_string.split('$').collect();
    if parts.len() < 6 {
        return Err(Error::crypto("Invalid Argon2 hash format".to_string()));
    }

    // Decode the hash portion (base64 - try with padding if needed)
    let mut hash_b64 = parts[5].to_string();

    // Add padding if necessary
    while !hash_b64.len().is_multiple_of(4) {
        hash_b64.push('=');
    }

    let decoded = general_purpose::STANDARD
        .decode(&hash_b64)
        .map_err(|e| Error::crypto(format!("Hash decoding failed: {}", e)))?;

    // Argon2 default produces 32 bytes, but we might get more
    if decoded.is_empty() {
        return Err(Error::crypto(
            "Hash decode produced empty result".to_string(),
        ));
    }

    Ok(decoded[..32.min(decoded.len())].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "correct horse battery staple";
        let plaintext = b"This is a secret message!";

        // Encrypt
        let encrypted = encrypt_with_password(password, plaintext).unwrap();

        // Verify structure
        assert_eq!(encrypted.version, 1);
        assert_eq!(encrypted.salt.len(), 16);
        assert_eq!(encrypted.nonce.len(), 12);
        assert!(!encrypted.ciphertext.is_empty());
        assert!(!encrypted.argon2_hash.is_empty());

        // Decrypt
        let decrypted = decrypt_with_password(password, &encrypted).unwrap();
        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_decrypt_wrong_password() {
        let password = "correct horse battery staple";
        let plaintext = b"Secret data";

        let encrypted = encrypt_with_password(password, plaintext).unwrap();

        // Try with wrong password
        let wrong_password = "incorrect horse battery staple";
        let result = decrypt_with_password(wrong_password, &encrypted);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Incorrect password"));
    }

    #[test]
    fn test_decrypt_tampered_data() {
        let password = "correct horse battery staple";
        let plaintext = b"Secret data";

        let mut encrypted = encrypt_with_password(password, plaintext).unwrap();

        // Tamper with ciphertext
        encrypted.ciphertext[0] ^= 1;

        // Decryption should fail due to authentication tag mismatch
        let result = decrypt_with_password(password, &encrypted);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Decryption failed"));
    }

    #[test]
    fn test_encrypt_deterministic_with_different_salt() {
        let password = "test password";
        let plaintext = b"test data";

        // Encrypt twice
        let encrypted1 = encrypt_with_password(password, plaintext).unwrap();
        let encrypted2 = encrypt_with_password(password, plaintext).unwrap();

        // Salts and nonces should be different (random)
        assert_ne!(encrypted1.salt, encrypted2.salt);
        assert_ne!(encrypted1.nonce, encrypted2.nonce);
        assert_ne!(encrypted1.ciphertext, encrypted2.ciphertext);

        // But both should decrypt to same plaintext
        let decrypted1 = decrypt_with_password(password, &encrypted1).unwrap();
        let decrypted2 = decrypt_with_password(password, &encrypted2).unwrap();
        assert_eq!(decrypted1, decrypted2);
        assert_eq!(plaintext, decrypted1.as_slice());
    }

    #[test]
    fn test_serialize_deserialize() {
        let password = "test password";
        let plaintext = b"test data";

        let encrypted = encrypt_with_password(password, plaintext).unwrap();

        // Serialize to bytes
        let bytes = encrypted.to_bytes().unwrap();

        // Deserialize
        let deserialized = EncryptedData::from_bytes(&bytes).unwrap();

        // Should be identical
        assert_eq!(encrypted.version, deserialized.version);
        assert_eq!(encrypted.salt, deserialized.salt);
        assert_eq!(encrypted.nonce, deserialized.nonce);
        assert_eq!(encrypted.ciphertext, deserialized.ciphertext);
        assert_eq!(encrypted.argon2_hash, deserialized.argon2_hash);

        // Should decrypt correctly
        let decrypted = decrypt_with_password(password, &deserialized).unwrap();
        assert_eq!(plaintext, decrypted.as_slice());
    }
}
