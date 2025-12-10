//! Key management and generation

use dchat_core::error::{Error, Result};
use ed25519_dalek::{SigningKey as Ed25519SigningKey, VerifyingKey as Ed25519VerifyingKey};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Generate cryptographically secure random bytes
/// 
/// # Arguments
/// * `len` - Number of random bytes to generate
/// 
/// # Returns
/// * `Ok(Vec<u8>)` - Vector of cryptographically secure random bytes
/// * `Err(KeyGenerationError)` - If the system CSPRNG fails
/// 
/// # Security Note
/// This function uses the system's cryptographic random number generator
/// (e.g., /dev/urandom on Unix, CryptGenRandom on Windows). It will fail
/// gracefully instead of panicking if entropy is unavailable.
pub fn generate_random_bytes(len: usize) -> std::result::Result<Vec<u8>, KeyGenerationError> {
    let mut bytes = vec![0u8; len];
    getrandom::getrandom(&mut bytes)?;
    Ok(bytes)
}

/// Generate random bytes into an existing buffer
/// 
/// # Arguments
/// * `buffer` - Mutable slice to fill with random bytes
/// 
/// # Returns
/// * `Ok(())` - Buffer filled with cryptographically secure random bytes
/// * `Err(KeyGenerationError)` - If the system CSPRNG fails
pub fn fill_random_bytes(buffer: &mut [u8]) -> std::result::Result<(), KeyGenerationError> {
    getrandom::getrandom(buffer)?;
    Ok(())
}

/// Error type for key generation failures
#[derive(Debug, thiserror::Error)]
pub enum KeyGenerationError {
    /// Random number generation failed - system entropy source unavailable
    #[error("Failed to generate random bytes: {0}")]
    RandomGenerationFailed(#[from] getrandom::Error),
}

/// A private key that automatically zeros itself when dropped
#[derive(Clone, ZeroizeOnDrop, Zeroize)]
pub struct PrivateKey {
    bytes: [u8; 32],
}

impl PrivateKey {
    /// Generate a new random private key
    /// 
    /// # Errors
    /// Returns `KeyGenerationError::RandomGenerationFailed` if the system's
    /// cryptographic random number generator fails or is unavailable.
    /// This can happen on:
    /// - Early boot before entropy is available
    /// - Virtualized environments without proper RNG passthrough
    /// - Systems with broken CSPRNG
    /// 
    /// # Security Note
    /// This method returns a Result instead of panicking to allow graceful
    /// error handling in production systems. Callers MUST handle this error
    /// appropriately - typically by logging and retrying, or failing safely.
    pub fn try_generate() -> std::result::Result<Self, KeyGenerationError> {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes)?;
        Ok(Self { bytes })
    }
    
    /// Generate a new random private key (panics on RNG failure)
    /// 
    /// # Panics
    /// Panics if the system's CSPRNG fails. Use `try_generate()` for
    /// production code that needs to handle RNG failures gracefully.
    /// 
    /// # Deprecated
    /// This method is provided for backward compatibility. New code should
    /// use `try_generate()` instead.
    #[deprecated(since = "0.2.0", note = "Use try_generate() instead for proper error handling")]
    pub fn generate() -> Self {
        Self::try_generate().expect("CSPRNG failure - system entropy unavailable")
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

/// A blockchain address derived from a public key
/// Uses the first 20 bytes of BLAKE3 hash of the public key (Ethereum-style)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address {
    bytes: [u8; 20],
}

impl Address {
    /// Create from existing bytes
    pub fn from_bytes(bytes: [u8; 20]) -> Self {
        Self { bytes }
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.bytes
    }

    /// Convert to hex string with 0x prefix
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.bytes))
    }

    /// Parse from hex string (with or without 0x prefix)
    pub fn from_hex(s: &str) -> Result<Self> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        let bytes = hex::decode(s)
            .map_err(|e| Error::crypto(format!("Invalid hex address: {}", e)))?;
        if bytes.len() != 20 {
            return Err(Error::crypto(format!(
                "Invalid address length: expected 20, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 20];
        arr.copy_from_slice(&bytes);
        Ok(Self { bytes: arr })
    }

    /// Convert to UserId for wallet operations
    pub fn to_user_id(&self) -> dchat_core::types::UserId {
        // Create a deterministic UUID from the address bytes
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes[..16].copy_from_slice(&self.bytes[..16]);
        dchat_core::types::UserId(uuid::Uuid::from_bytes(uuid_bytes))
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
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

    /// Derive blockchain address from this public key
    /// Uses BLAKE3 hash of the public key, taking first 20 bytes
    pub fn to_address(&self) -> Address {
        let hash = blake3::hash(&self.bytes);
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&hash.as_bytes()[..20]);
        Address::from_bytes(addr_bytes)
    }

    /// Convert to UserId for wallet/staking operations
    pub fn to_user_id(&self) -> dchat_core::types::UserId {
        self.to_address().to_user_id()
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
    /// 
    /// # Errors
    /// Returns `KeyGenerationError` if the system CSPRNG fails.
    pub fn try_generate() -> std::result::Result<Self, KeyGenerationError> {
        let private_key = PrivateKey::try_generate()?;
        let public_key = private_key.public_key();

        Ok(Self {
            private_key,
            public_key,
        })
    }
    
    /// Generate a new random keypair (panics on RNG failure)
    /// 
    /// # Panics
    /// Panics if the system's CSPRNG fails.
    /// 
    /// # Deprecated
    /// This method is provided for backward compatibility. New code should
    /// use `try_generate()` instead.
    #[deprecated(since = "0.2.0", note = "Use try_generate() instead for proper error handling")]
    #[allow(deprecated)]
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

    /// Derive blockchain address from this keypair's public key
    pub fn to_address(&self) -> Address {
        self.public_key.to_address()
    }

    /// Convert to UserId for wallet/staking operations
    pub fn to_user_id(&self) -> dchat_core::types::UserId {
        self.public_key.to_user_id()
    }

    /// Split into private and public keys
    pub fn into_keys(self) -> (PrivateKey, PublicKey) {
        (self.private_key, self.public_key)
    }
}

/// Derive keys using HKDF-based hierarchical deterministic key derivation
/// 
/// # Security
/// This implementation uses HKDF (RFC 5869) instead of simple hashing for proper
/// key derivation with domain separation. This provides:
/// - Proper cryptographic key expansion
/// - Domain separation via salt and info parameters
/// - Resistance to related-key attacks
pub struct KeyDerivation;

/// Domain separation salt for dchat key derivation
const KEY_DERIVATION_SALT: &[u8] = b"dchat-key-derivation-v1";

impl KeyDerivation {
    /// Derive a child private key from a parent key and index using HKDF
    /// 
    /// # Security
    /// Uses HKDF-SHA256 with domain separation to derive child keys safely.
    /// The salt provides domain separation, and the index is included in
    /// the info parameter to ensure unique keys for each derivation path.
    pub fn derive_private_key(parent_key: &PrivateKey, index: u32) -> Result<PrivateKey> {
        use hkdf::Hkdf;
        use sha2::Sha256;
        
        // Create HKDF instance with domain-separated salt
        let hkdf = Hkdf::<Sha256>::new(
            Some(KEY_DERIVATION_SALT),
            parent_key.as_bytes(),
        );
        
        // Derive key with index in info parameter for uniqueness
        let info = format!("dchat-child-key-{}", index);
        let mut okm = [0u8; 32];
        hkdf.expand(info.as_bytes(), &mut okm)
            .map_err(|e| dchat_core::Error::crypto(format!("HKDF expansion failed: {}", e)))?;
        
        Ok(PrivateKey::from_bytes(okm))
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
