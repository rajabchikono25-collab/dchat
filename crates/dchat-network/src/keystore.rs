//! Encrypted keystore for persistent relay X25519 keys
//!
//! Implements secure storage of relay node identity keys with:
//! - Ed25519-to-X25519 key derivation
//! - age encryption for at-rest protection
//! - File-based persistence with atomic writes
//! - DHT public key publishing integration

use dchat_core::error::{Error, Result};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use serde_json;
use std::path::{Path, PathBuf};
use x25519_dalek::{PublicKey, StaticSecret};

/// Relay node persistent identity
#[derive(Clone, Serialize, Deserialize)]
pub struct RelayKeystore {
    /// Ed25519 signing key (for blockchain identity)
    ed25519_signing_key: Vec<u8>, // 32 bytes
    /// Derived X25519 static secret (for onion routing ECDH)
    x25519_static_secret: Vec<u8>, // 32 bytes
    /// X25519 public key (advertised via DHT)
    x25519_public_key: Vec<u8>, // 32 bytes
    /// Creation timestamp
    created_at: u64,
}

impl RelayKeystore {
    /// Generate new relay identity from Ed25519 signing key
    ///
    /// Derives X25519 static secret using BLAKE3 key derivation
    pub fn from_ed25519(signing_key: &SigningKey) -> Result<Self> {
        let ed25519_bytes = signing_key.to_bytes();

        // Derive X25519 static secret from Ed25519 seed using BLAKE3 KDF
        // This ensures deterministic derivation while maintaining security separation
        let mut hasher =
            blake3::Hasher::new_keyed(&blake3::hash(b"dchat-x25519-derive-v1").as_bytes());
        hasher.update(&ed25519_bytes);
        let derived_bytes = hasher.finalize();

        let x25519_static_secret = StaticSecret::from(*derived_bytes.as_bytes());
        let x25519_public_key = PublicKey::from(&x25519_static_secret);

        Ok(Self {
            ed25519_signing_key: ed25519_bytes.to_vec(),
            x25519_static_secret: x25519_static_secret.to_bytes().to_vec(),
            x25519_public_key: x25519_public_key.to_bytes().to_vec(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| Error::internal(format!("System time error: {}", e)))?
                .as_secs(),
        })
    }

    /// Load keystore from encrypted file
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let encrypted_data = std::fs::read(path.as_ref())
            .map_err(|e| Error::storage(format!("Failed to read keystore: {}", e)))?;

        // Decrypt using age with passphrase from environment variable
        let passphrase = std::env::var("DCHAT_RELAY_KEYSTORE_PASSPHRASE")
            .map_err(|_| Error::crypto("DCHAT_RELAY_KEYSTORE_PASSPHRASE not set"))?;

        let decryptor = age::Decryptor::new(&encrypted_data[..])
            .map_err(|e| Error::crypto(format!("Failed to initialize decryptor: {}", e)))?;

        let mut decrypted = vec![];
        match decryptor {
            age::Decryptor::Passphrase(d) => {
                let mut reader = d
                    .decrypt(&age::secrecy::Secret::new(passphrase), None)
                    .map_err(|e| Error::crypto(format!("Decryption failed: {}", e)))?;
                std::io::Read::read_to_end(&mut reader, &mut decrypted)
                    .map_err(|e| Error::storage(format!("Failed to read decrypted data: {}", e)))?;
            }
            _ => return Err(Error::crypto("Unsupported age decryption format")),
        }

        serde_json::from_slice(&decrypted)
            .map_err(|e| Error::network(format!("Failed to deserialize keystore: {}", e)))
    }

    /// Save keystore to encrypted file
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        // Serialize keystore
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| Error::network(format!("Failed to serialize keystore: {}", e)))?;

        // Encrypt with age using passphrase
        let passphrase = std::env::var("DCHAT_RELAY_KEYSTORE_PASSPHRASE")
            .map_err(|_| Error::crypto("DCHAT_RELAY_KEYSTORE_PASSPHRASE not set"))?;

        let encryptor = age::Encryptor::with_user_passphrase(age::secrecy::Secret::new(passphrase));

        let mut encrypted = vec![];
        let mut writer = encryptor
            .wrap_output(&mut encrypted)
            .map_err(|e| Error::crypto(format!("Failed to initialize encryptor: {}", e)))?;

        std::io::Write::write_all(&mut writer, &json)
            .map_err(|e| Error::storage(format!("Failed to write encrypted data: {}", e)))?;

        writer
            .finish()
            .map_err(|e| Error::crypto(format!("Failed to finalize encryption: {}", e)))?;

        // Atomic write: write to temp file, then rename
        let temp_path = path.as_ref().with_extension("tmp");
        std::fs::write(&temp_path, &encrypted)
            .map_err(|e| Error::storage(format!("Failed to write temp keystore: {}", e)))?;

        std::fs::rename(&temp_path, path.as_ref())
            .map_err(|e| Error::storage(format!("Failed to atomically rename keystore: {}", e)))?;

        tracing::info!("✅ Relay keystore saved to {}", path.as_ref().display());
        Ok(())
    }

    /// Get X25519 static secret for ECDH operations
    pub fn x25519_static_secret(&self) -> Result<StaticSecret> {
        let bytes: [u8; 32] = self
            .x25519_static_secret
            .as_slice()
            .try_into()
            .map_err(|_| Error::crypto("Invalid X25519 static secret length"))?;
        Ok(StaticSecret::from(bytes))
    }

    /// Get X25519 public key for DHT advertisement
    pub fn x25519_public_key(&self) -> Result<PublicKey> {
        let bytes: [u8; 32] = self
            .x25519_public_key
            .as_slice()
            .try_into()
            .map_err(|_| Error::crypto("Invalid X25519 public key length"))?;
        Ok(PublicKey::from(bytes))
    }

    /// Get Ed25519 signing key for blockchain operations
    pub fn ed25519_signing_key(&self) -> Result<SigningKey> {
        let bytes: [u8; 32] = self
            .ed25519_signing_key
            .as_slice()
            .try_into()
            .map_err(|_| Error::crypto("Invalid Ed25519 signing key length"))?;
        Ok(SigningKey::from_bytes(&bytes))
    }
}

/// Default keystore path for relay nodes
pub fn default_keystore_path() -> PathBuf {
    let mut path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    path.push("relay_keystore.age");
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_keystore_derivation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let keystore = RelayKeystore::from_ed25519(&signing_key).unwrap();

        // Verify derivation is deterministic
        let keystore2 = RelayKeystore::from_ed25519(&signing_key).unwrap();
        assert_eq!(
            keystore.x25519_public_key, keystore2.x25519_public_key,
            "X25519 derivation must be deterministic"
        );
    }

    #[test]
    fn test_keystore_roundtrip() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let keystore = RelayKeystore::from_ed25519(&signing_key).unwrap();

        // Set passphrase for test
        std::env::set_var("DCHAT_RELAY_KEYSTORE_PASSPHRASE", "test-passphrase-123");

        let temp_path = std::env::temp_dir().join("test_keystore.age");

        // Save and load
        keystore.save(&temp_path).unwrap();
        let loaded = RelayKeystore::load(&temp_path).unwrap();

        assert_eq!(keystore.x25519_public_key, loaded.x25519_public_key);
        assert_eq!(keystore.ed25519_signing_key, loaded.ed25519_signing_key);

        // Cleanup
        std::fs::remove_file(&temp_path).ok();
    }
}
