//! Storage Unlock Key (SUK) - Quorum-Gated Encryption Component
//!
//! The Storage Unlock Key (SUK) is a per-conversation symmetric key that wraps
//! message keys for at-rest storage. It provides the "revocable read access"
//! property: if a device cannot obtain a valid EpochToken from the relay quorum,
//! it cannot decrypt the SUK and thus cannot read messages.
//!
//! # Key Hierarchy
//!
//! ```text
//! EpochToken_t (from quorum)
//!      │
//!      ├── HKDF ──► UK_t (Unlock Key for epoch t)
//!      │                   │
//!      │                   ▼
//!      │           ┌──────────────┐
//!      │           │ Encrypted    │
//!      │           │ SUK Blob     │ ◄── stored in local DB
//!      │           └──────────────┘
//!      │                   │
//!      │            AEAD_Decrypt(UK_t, ...)
//!      │                   │
//!      │                   ▼
//!      │           ┌──────────────┐
//!      │           │     SUK      │ ◄── plaintext, only in memory
//!      │           └──────────────┘
//!      │                   │
//!      │            AEAD_Decrypt(SUK, wrapped_msg_key)
//!      │                   │
//!      │                   ▼
//!      │           ┌──────────────┐
//!      │           │  MessageKey  │ ◄── unwrapped per-message key
//!      │           └──────────────┘
//!      │                   │
//!      │            AEAD_Decrypt(MessageKey, ciphertext)
//!      │                   │
//!      ▼                   ▼
//!   10-min TTL        Plaintext Message
//! ```
//!
//! # Security Properties
//!
//! 1. **Time-Bounded Access**: Without a valid EpochToken, SUK cannot be decrypted
//! 2. **Per-Conversation Isolation**: Each conversation has its own SUK
//! 3. **Forward Secrecy**: Old epochs can be erased, making old encrypted SUKs unrecoverable
//! 4. **Revocation Support**: Emergency lock or device revocation stops epoch token issuance
//! 5. **Zeroization**: SUK is zeroized when dropped from memory
//!
//! # Usage
//!
//! ```rust,ignore
//! use dchat_crypto::suk::{StorageUnlockKey, SukManager, EpochToken};
//!
//! // Create a new SUK for a conversation
//! let suk = StorageUnlockKey::generate()?;
//!
//! // Encrypt the SUK for storage (requires epoch token)
//! let epoch_token = /* obtain from quorum */;
//! let encrypted_suk = suk.encrypt_for_storage(&epoch_token, &conversation_id, &device_id)?;
//!
//! // Later: decrypt SUK (requires valid epoch token)
//! let suk = StorageUnlockKey::decrypt_from_storage(&encrypted_suk, &epoch_token, &conversation_id, &device_id)?;
//!
//! // Wrap a message key for storage
//! let wrapped_key = suk.wrap_message_key(&message_key, &message_id)?;
//!
//! // Unwrap to decrypt message
//! let message_key = suk.unwrap_message_key(&wrapped_key, &message_id)?;
//! ```

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::kdf::Hkdf;

/// Size of the SUK in bytes (256-bit)
pub const SUK_SIZE: usize = 32;

/// Size of the nonce for AEAD operations
pub const NONCE_SIZE: usize = 12;

/// Size of the authentication tag
pub const TAG_SIZE: usize = 16;

/// Epoch token validity period in seconds (10 minutes)
pub const EPOCH_TOKEN_TTL_SECONDS: u64 = 600;

/// Epoch token from quorum - used to derive unlock keys
#[derive(Clone, Serialize, Deserialize)]
pub struct EpochToken {
    /// Token data (FROST threshold signature or derived value)
    token: Vec<u8>,
    /// Epoch identifier (unix timestamp / TTL)
    pub epoch_id: u64,
    /// Expiration timestamp
    pub expires_at: u64,
    /// Quorum threshold used (e.g., 4 for 4-of-7)
    pub threshold: u8,
    /// Total quorum size (e.g., 7 for 4-of-7)
    pub quorum_size: u8,
}

impl EpochToken {
    /// Create a new epoch token
    pub fn new(token: Vec<u8>, epoch_id: u64, threshold: u8, quorum_size: u8) -> Self {
        Self {
            token,
            epoch_id,
            expires_at: epoch_id * EPOCH_TOKEN_TTL_SECONDS + EPOCH_TOKEN_TTL_SECONDS,
            threshold,
            quorum_size,
        }
    }

    /// Check if token is still valid
    pub fn is_valid(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now < self.expires_at
    }

    /// Get the token data for key derivation
    pub fn as_bytes(&self) -> &[u8] {
        &self.token
    }

    /// Time remaining until expiration
    pub fn ttl_seconds(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.expires_at.saturating_sub(now)
    }

    /// Calculate current epoch ID from timestamp
    pub fn current_epoch_id() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() / EPOCH_TOKEN_TTL_SECONDS)
            .unwrap_or(0)
    }
}

impl std::fmt::Debug for EpochToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EpochToken")
            .field("epoch_id", &self.epoch_id)
            .field("expires_at", &self.expires_at)
            .field(
                "threshold",
                &format!("{}-of-{}", self.threshold, self.quorum_size),
            )
            .field("valid", &self.is_valid())
            .field("token", &"[REDACTED]")
            .finish()
    }
}

/// Unlock Key derived from EpochToken for a specific conversation/device
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct UnlockKey {
    key: [u8; 32],
    /// Metadata for key lifecycle tracking (not included in zeroize)
    #[zeroize(skip)]
    metadata: UnlockKeyMetadata,
}

/// Metadata for unlock key lifecycle tracking
#[derive(Debug, Clone, Default)]
pub struct UnlockKeyMetadata {
    /// Epoch ID this key was derived for
    pub epoch_id: u64,
    /// Conversation ID hash (first 8 bytes for logging)
    pub conversation_prefix: [u8; 8],
    /// Device ID hash (first 8 bytes for logging)
    pub device_prefix: [u8; 8],
    /// Derivation timestamp
    pub derived_at: u64,
    /// Number of times this key has been used
    pub use_count: u32,
    /// Maximum allowed uses per epoch (anti-harvesting)
    pub max_uses: u32,
}

/// Configuration for unlock key derivation
#[derive(Debug, Clone)]
pub struct UnlockKeyDerivationConfig {
    /// Per-device salt for additional isolation
    pub device_salt: Option<[u8; 16]>,
    /// Maximum unlock operations per epoch (anti-harvesting)
    pub max_uses_per_epoch: u32,
    /// Require attestation binding
    pub require_attestation: bool,
    /// Optional attestation data to bind to the key
    pub attestation_binding: Option<Vec<u8>>,
}

impl Default for UnlockKeyDerivationConfig {
    fn default() -> Self {
        Self {
            device_salt: None,
            max_uses_per_epoch: 1000, // Default limit per epoch
            require_attestation: false,
            attestation_binding: None,
        }
    }
}

impl UnlockKey {
    /// Derive unlock key from epoch token (basic version for backward compatibility)
    pub fn derive(
        epoch_token: &EpochToken,
        conversation_id: &[u8; 32],
        device_id: &[u8; 32],
    ) -> Result<Self> {
        Self::derive_with_config(
            epoch_token,
            conversation_id,
            device_id,
            &UnlockKeyDerivationConfig::default(),
        )
    }

    /// Derive unlock key with advanced configuration
    ///
    /// # Security Properties
    ///
    /// - **Per-device isolation**: Optional device salt prevents cross-device correlation
    /// - **Anti-harvesting**: Tracks use count to limit key reuse attacks
    /// - **Attestation binding**: Can bind key derivation to device attestation
    /// - **Epoch binding**: Key is cryptographically bound to specific epoch
    ///
    /// # Derivation Formula
    ///
    /// ```text
    /// salt = device_salt || epoch_id.to_le_bytes()
    /// info = b"dchat-unlock-v1" || conversation_id || device_id || attestation_binding?
    /// UK = HKDF-SHA256(salt, epoch_token, info, 32)
    /// ```
    pub fn derive_with_config(
        epoch_token: &EpochToken,
        conversation_id: &[u8; 32],
        device_id: &[u8; 32],
        config: &UnlockKeyDerivationConfig,
    ) -> Result<Self> {
        // Validate epoch token
        if !epoch_token.is_valid() {
            return Err(Error::crypto("Cannot derive key from expired epoch token"));
        }

        // Build salt: device_salt (optional) || epoch_id
        let mut salt = Vec::with_capacity(24);
        if let Some(device_salt) = &config.device_salt {
            salt.extend_from_slice(device_salt);
        }
        salt.extend_from_slice(&epoch_token.epoch_id.to_le_bytes());

        // Build info: protocol_version || conversation_id || device_id || attestation?
        let mut info = Vec::with_capacity(80);
        info.extend_from_slice(b"dchat-unlock-v1");
        info.extend_from_slice(conversation_id);
        info.extend_from_slice(device_id);

        // Bind to attestation if provided
        if let Some(attestation) = &config.attestation_binding {
            // Hash attestation to fixed size
            let attestation_hash = blake3::hash(attestation);
            info.extend_from_slice(attestation_hash.as_bytes());
        }

        // Use HKDF with optional salt
        let salt_slice = if salt.is_empty() {
            None
        } else {
            Some(salt.as_slice())
        };

        let derived = Hkdf::derive(salt_slice, epoch_token.as_bytes(), &info, 32)?;

        let mut key = [0u8; 32];
        key.copy_from_slice(&derived);

        // Build metadata
        let mut conversation_prefix = [0u8; 8];
        let mut device_prefix = [0u8; 8];
        conversation_prefix.copy_from_slice(&conversation_id[..8]);
        device_prefix.copy_from_slice(&device_id[..8]);

        let derived_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(Self {
            key,
            metadata: UnlockKeyMetadata {
                epoch_id: epoch_token.epoch_id,
                conversation_prefix,
                device_prefix,
                derived_at,
                use_count: 0,
                max_uses: config.max_uses_per_epoch,
            },
        })
    }

    /// Record a key usage and check if limit exceeded
    ///
    /// Returns error if max uses exceeded (anti-harvesting protection)
    pub fn record_use(&mut self) -> Result<()> {
        self.metadata.use_count += 1;
        if self.metadata.use_count > self.metadata.max_uses {
            return Err(Error::crypto(format!(
                "Unlock key use limit exceeded: {} > {}",
                self.metadata.use_count, self.metadata.max_uses
            )));
        }
        Ok(())
    }

    /// Check if key is still within usage limits
    pub fn is_within_limits(&self) -> bool {
        self.metadata.use_count < self.metadata.max_uses
    }

    /// Get the key bytes for AEAD operations
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    /// Get key metadata (for logging/monitoring)
    pub fn metadata(&self) -> &UnlockKeyMetadata {
        &self.metadata
    }

    /// Get the epoch ID this key was derived for
    pub fn epoch_id(&self) -> u64 {
        self.metadata.epoch_id
    }

    /// Check if key is for a specific epoch
    pub fn is_for_epoch(&self, epoch_id: u64) -> bool {
        self.metadata.epoch_id == epoch_id
    }

    /// Get remaining uses before limit
    pub fn remaining_uses(&self) -> u32 {
        self.metadata
            .max_uses
            .saturating_sub(self.metadata.use_count)
    }
}

impl std::fmt::Debug for UnlockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockKey")
            .field("epoch_id", &self.metadata.epoch_id)
            .field(
                "conversation",
                &hex::encode(&self.metadata.conversation_prefix),
            )
            .field("device", &hex::encode(&self.metadata.device_prefix))
            .field("use_count", &self.metadata.use_count)
            .field("max_uses", &self.metadata.max_uses)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

/// Builder for creating unlock keys with custom configuration
pub struct UnlockKeyBuilder {
    epoch_token: EpochToken,
    conversation_id: [u8; 32],
    device_id: [u8; 32],
    config: UnlockKeyDerivationConfig,
}

impl UnlockKeyBuilder {
    /// Create a new builder
    pub fn new(epoch_token: EpochToken, conversation_id: [u8; 32], device_id: [u8; 32]) -> Self {
        Self {
            epoch_token,
            conversation_id,
            device_id,
            config: UnlockKeyDerivationConfig::default(),
        }
    }

    /// Set device salt for per-device isolation
    pub fn with_device_salt(mut self, salt: [u8; 16]) -> Self {
        self.config.device_salt = Some(salt);
        self
    }

    /// Set maximum uses per epoch (anti-harvesting)
    pub fn with_max_uses(mut self, max_uses: u32) -> Self {
        self.config.max_uses_per_epoch = max_uses;
        self
    }

    /// Bind key to device attestation
    pub fn with_attestation_binding(mut self, attestation: Vec<u8>) -> Self {
        self.config.attestation_binding = Some(attestation);
        self.config.require_attestation = true;
        self
    }

    /// Build the unlock key
    pub fn build(self) -> Result<UnlockKey> {
        UnlockKey::derive_with_config(
            &self.epoch_token,
            &self.conversation_id,
            &self.device_id,
            &self.config,
        )
    }
}

/// Storage Unlock Key - per-conversation key for wrapping message keys
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct StorageUnlockKey {
    key: [u8; SUK_SIZE],
    /// Conversation this SUK belongs to
    #[zeroize(skip)]
    conversation_id: [u8; 32],
    /// Generation counter (incremented on rotation)
    #[zeroize(skip)]
    generation: u64,
}

impl StorageUnlockKey {
    /// Generate a new random SUK for a conversation
    pub fn generate(conversation_id: [u8; 32]) -> Result<Self> {
        use rand::RngCore;
        let mut key = [0u8; SUK_SIZE];
        rand::thread_rng().fill_bytes(&mut key);

        Ok(Self {
            key,
            conversation_id,
            generation: 0,
        })
    }

    /// Create from existing bytes (for testing or recovery)
    pub fn from_bytes(key: [u8; SUK_SIZE], conversation_id: [u8; 32], generation: u64) -> Self {
        Self {
            key,
            conversation_id,
            generation,
        }
    }

    /// Get conversation ID
    pub fn conversation_id(&self) -> &[u8; 32] {
        &self.conversation_id
    }

    /// Get generation counter
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Encrypt SUK for at-rest storage using the unlock key
    pub fn encrypt_for_storage(&self, unlock_key: &UnlockKey) -> Result<EncryptedSuk> {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce,
        };

        let cipher = Aes256Gcm::new(unlock_key.as_bytes().into());

        // Use conversation_id + generation as nonce determinant
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        nonce_bytes[0..8].copy_from_slice(&self.generation.to_le_bytes());
        nonce_bytes[8..12].copy_from_slice(&self.conversation_id[0..4]);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Plaintext includes key + generation
        let mut plaintext = Vec::with_capacity(SUK_SIZE + 8);
        plaintext.extend_from_slice(&self.key);
        plaintext.extend_from_slice(&self.generation.to_le_bytes());

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| Error::crypto(format!("SUK encryption failed: {}", e)))?;

        Ok(EncryptedSuk {
            ciphertext,
            nonce: nonce_bytes,
            conversation_id: self.conversation_id,
        })
    }

    /// Decrypt SUK from storage using the unlock key
    pub fn decrypt_from_storage(encrypted: &EncryptedSuk, unlock_key: &UnlockKey) -> Result<Self> {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce,
        };

        let cipher = Aes256Gcm::new(unlock_key.as_bytes().into());
        let nonce = Nonce::from_slice(&encrypted.nonce);

        let plaintext = cipher
            .decrypt(nonce, encrypted.ciphertext.as_ref())
            .map_err(|e| {
                Error::crypto(format!(
                    "SUK decryption failed (invalid epoch token?): {}",
                    e
                ))
            })?;

        if plaintext.len() != SUK_SIZE + 8 {
            return Err(Error::crypto("Invalid SUK plaintext length".to_string()));
        }

        let mut key = [0u8; SUK_SIZE];
        key.copy_from_slice(&plaintext[0..SUK_SIZE]);

        let generation = u64::from_le_bytes(
            plaintext[SUK_SIZE..SUK_SIZE + 8]
                .try_into()
                .map_err(|_| Error::crypto("Invalid generation bytes".to_string()))?,
        );

        Ok(Self {
            key,
            conversation_id: encrypted.conversation_id,
            generation,
        })
    }

    /// Wrap a message key for storage
    pub fn wrap_message_key(
        &self,
        message_key: &[u8; 32],
        message_id: &[u8; 32],
    ) -> Result<WrappedMessageKey> {
        use aes_gcm::{
            aead::{Aead, KeyInit, Payload},
            Aes256Gcm, Nonce,
        };

        let cipher = Aes256Gcm::new((&self.key).into());

        // Derive nonce from message_id
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        nonce_bytes.copy_from_slice(&message_id[0..NONCE_SIZE]);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // AAD includes conversation ID and SUK generation for binding
        let mut aad = Vec::with_capacity(40);
        aad.extend_from_slice(&self.conversation_id);
        aad.extend_from_slice(&self.generation.to_le_bytes());

        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: message_key,
                    aad: &aad,
                },
            )
            .map_err(|e| Error::crypto(format!("Message key wrapping failed: {}", e)))?;

        Ok(WrappedMessageKey {
            ciphertext,
            nonce: nonce_bytes,
            suk_generation: self.generation,
        })
    }

    /// Unwrap a message key for decryption
    pub fn unwrap_message_key(&self, wrapped: &WrappedMessageKey) -> Result<[u8; 32]> {
        use aes_gcm::{
            aead::{Aead, KeyInit, Payload},
            Aes256Gcm, Nonce,
        };

        // Check SUK generation matches
        if wrapped.suk_generation != self.generation {
            return Err(Error::crypto(format!(
                "SUK generation mismatch: wrapped={}, current={}",
                wrapped.suk_generation, self.generation
            )));
        }

        let cipher = Aes256Gcm::new((&self.key).into());
        let nonce = Nonce::from_slice(&wrapped.nonce);

        // AAD for decryption must match encryption
        let mut aad = Vec::with_capacity(40);
        aad.extend_from_slice(&self.conversation_id);
        aad.extend_from_slice(&self.generation.to_le_bytes());

        let plaintext = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &wrapped.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|e| Error::crypto(format!("Message key unwrapping failed: {}", e)))?;

        if plaintext.len() != 32 {
            return Err(Error::crypto("Invalid message key length".to_string()));
        }

        let mut message_key = [0u8; 32];
        message_key.copy_from_slice(&plaintext);
        Ok(message_key)
    }

    /// Rotate SUK - generates new key with incremented generation
    ///
    /// This should be called when:
    /// - A device is revoked from the conversation
    /// - A member leaves a channel
    /// - Periodic rotation policy triggers
    ///
    /// The new SUK must be distributed to all remaining authorized devices.
    pub fn rotate(&mut self) -> Result<()> {
        use rand::RngCore;

        let mut new_key = [0u8; SUK_SIZE];
        rand::thread_rng().fill_bytes(&mut new_key);

        // Zeroize old key before replacing
        self.key.zeroize();
        self.key = new_key;
        self.generation += 1;

        Ok(())
    }
}

impl std::fmt::Debug for StorageUnlockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageUnlockKey")
            .field("conversation_id", &hex::encode(&self.conversation_id[0..8]))
            .field("generation", &self.generation)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

/// Encrypted SUK blob for storage
#[derive(Clone, Serialize, Deserialize)]
pub struct EncryptedSuk {
    /// AEAD ciphertext (SUK + generation + tag)
    pub ciphertext: Vec<u8>,
    /// Nonce used for encryption
    pub nonce: [u8; NONCE_SIZE],
    /// Conversation this SUK belongs to
    pub conversation_id: [u8; 32],
}

impl std::fmt::Debug for EncryptedSuk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedSuk")
            .field("conversation_id", &hex::encode(&self.conversation_id[0..8]))
            .field("ciphertext_len", &self.ciphertext.len())
            .finish()
    }
}

/// Wrapped message key for at-rest storage
#[derive(Clone, Serialize, Deserialize)]
pub struct WrappedMessageKey {
    /// AEAD ciphertext (32-byte key + 16-byte tag)
    pub ciphertext: Vec<u8>,
    /// Nonce used (derived from message_id)
    pub nonce: [u8; NONCE_SIZE],
    /// SUK generation that wrapped this key
    pub suk_generation: u64,
}

impl WrappedMessageKey {
    /// Serialize for database storage
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize from database
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| Error::crypto(format!("WrappedMessageKey deserialization failed: {}", e)))
    }
}

impl std::fmt::Debug for WrappedMessageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WrappedMessageKey")
            .field("suk_generation", &self.suk_generation)
            .field("ciphertext_len", &self.ciphertext.len())
            .finish()
    }
}

/// Manager for SUK lifecycle operations
pub struct SukManager {
    /// Cache of decrypted SUKs (in memory only)
    cache: std::collections::HashMap<[u8; 32], StorageUnlockKey>,
    /// Device ID for this manager
    device_id: [u8; 32],
}

impl SukManager {
    /// Create a new SUK manager for a device
    pub fn new(device_id: [u8; 32]) -> Self {
        Self {
            cache: std::collections::HashMap::new(),
            device_id,
        }
    }

    /// Get or create SUK for a conversation
    pub fn get_or_create(
        &mut self,
        conversation_id: [u8; 32],
        epoch_token: &EpochToken,
        encrypted_suk: Option<&EncryptedSuk>,
    ) -> Result<&StorageUnlockKey> {
        if !self.cache.contains_key(&conversation_id) {
            let suk = if let Some(encrypted) = encrypted_suk {
                // Decrypt existing SUK
                let unlock_key = UnlockKey::derive(epoch_token, &conversation_id, &self.device_id)?;
                StorageUnlockKey::decrypt_from_storage(encrypted, &unlock_key)?
            } else {
                // Create new SUK for new conversation
                StorageUnlockKey::generate(conversation_id)?
            };
            self.cache.insert(conversation_id, suk);
        }

        Ok(self.cache.get(&conversation_id).unwrap())
    }

    /// Get cached SUK (without epoch token)
    pub fn get_cached(&self, conversation_id: &[u8; 32]) -> Option<&StorageUnlockKey> {
        self.cache.get(conversation_id)
    }

    /// Clear a specific SUK from cache (on conversation close or error)
    pub fn clear(&mut self, conversation_id: &[u8; 32]) {
        self.cache.remove(conversation_id);
    }

    /// Clear all cached SUKs (on lock or logout)
    pub fn clear_all(&mut self) {
        self.cache.clear();
    }

    /// Number of cached SUKs
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Device ID for this manager
    pub fn device_id(&self) -> &[u8; 32] {
        &self.device_id
    }
}

impl Drop for SukManager {
    fn drop(&mut self) {
        // Ensure all SUKs are zeroized when manager is dropped
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_epoch_token() -> EpochToken {
        let token = vec![1u8; 64]; // Simulated threshold signature
        EpochToken::new(token, EpochToken::current_epoch_id(), 4, 7)
    }

    fn create_test_ids() -> ([u8; 32], [u8; 32], [u8; 32]) {
        let conversation_id = crate::hash(b"test-conversation");
        let device_id = crate::hash(b"test-device");
        let message_id = crate::hash(b"test-message");
        (conversation_id, device_id, message_id)
    }

    #[test]
    fn test_suk_roundtrip() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();

        // Generate SUK
        let suk = StorageUnlockKey::generate(conversation_id).unwrap();
        assert_eq!(suk.generation(), 0);

        // Encrypt for storage
        let unlock_key = UnlockKey::derive(&epoch_token, &conversation_id, &device_id).unwrap();
        let encrypted = suk.encrypt_for_storage(&unlock_key).unwrap();

        // Decrypt from storage
        let recovered = StorageUnlockKey::decrypt_from_storage(&encrypted, &unlock_key).unwrap();
        assert_eq!(recovered.generation(), suk.generation());
        assert_eq!(recovered.conversation_id(), suk.conversation_id());
    }

    #[test]
    fn test_message_key_wrapping() {
        let (conversation_id, _, message_id) = create_test_ids();

        let suk = StorageUnlockKey::generate(conversation_id).unwrap();

        // Generate a message key
        let mut message_key = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut message_key);

        // Wrap and unwrap
        let wrapped = suk.wrap_message_key(&message_key, &message_id).unwrap();
        let recovered = suk.unwrap_message_key(&wrapped).unwrap();

        assert_eq!(message_key, recovered);
    }

    #[test]
    fn test_wrong_epoch_token_fails() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token1 = create_test_epoch_token();

        // Different token data
        let epoch_token2 = EpochToken::new(vec![2u8; 64], EpochToken::current_epoch_id(), 4, 7);

        let suk = StorageUnlockKey::generate(conversation_id).unwrap();

        // Encrypt with token 1
        let unlock_key1 = UnlockKey::derive(&epoch_token1, &conversation_id, &device_id).unwrap();
        let encrypted = suk.encrypt_for_storage(&unlock_key1).unwrap();

        // Try to decrypt with token 2 - should fail
        let unlock_key2 = UnlockKey::derive(&epoch_token2, &conversation_id, &device_id).unwrap();
        let result = StorageUnlockKey::decrypt_from_storage(&encrypted, &unlock_key2);

        assert!(result.is_err());
    }

    #[test]
    fn test_suk_rotation() {
        let (conversation_id, _, message_id) = create_test_ids();

        let mut suk = StorageUnlockKey::generate(conversation_id).unwrap();
        let gen0 = suk.generation();

        // Wrap a message key with gen 0
        let mut message_key = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut message_key);
        let wrapped_gen0 = suk.wrap_message_key(&message_key, &message_id).unwrap();

        // Rotate
        suk.rotate().unwrap();
        assert_eq!(suk.generation(), gen0 + 1);

        // Try to unwrap with new generation - should fail due to generation mismatch
        let result = suk.unwrap_message_key(&wrapped_gen0);
        assert!(result.is_err());
    }

    #[test]
    fn test_suk_manager() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();

        let mut manager = SukManager::new(device_id);
        assert_eq!(manager.cache_size(), 0);

        // Get or create (creates new)
        let _suk = manager
            .get_or_create(conversation_id, &epoch_token, None)
            .unwrap();
        assert_eq!(manager.cache_size(), 1);

        // Get again (from cache)
        let _suk = manager.get_cached(&conversation_id).unwrap();
        assert_eq!(manager.cache_size(), 1);

        // Clear
        manager.clear(&conversation_id);
        assert_eq!(manager.cache_size(), 0);
    }

    #[test]
    fn test_epoch_token_validity() {
        let token = EpochToken::new(vec![1u8; 64], EpochToken::current_epoch_id(), 4, 7);

        assert!(token.is_valid());
        assert!(token.ttl_seconds() <= EPOCH_TOKEN_TTL_SECONDS);
    }

    #[test]
    fn test_unlock_key_with_device_salt() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();

        let salt: [u8; 16] = [0xAB; 16];

        // Derive with salt
        let config_with_salt = UnlockKeyDerivationConfig {
            device_salt: Some(salt),
            ..Default::default()
        };

        let key_with_salt = UnlockKey::derive_with_config(
            &epoch_token,
            &conversation_id,
            &device_id,
            &config_with_salt,
        )
        .unwrap();

        // Derive without salt
        let key_without_salt =
            UnlockKey::derive(&epoch_token, &conversation_id, &device_id).unwrap();

        // Keys should be different
        assert_ne!(key_with_salt.as_bytes(), key_without_salt.as_bytes());
    }

    #[test]
    fn test_unlock_key_use_limits() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();

        let config = UnlockKeyDerivationConfig {
            max_uses_per_epoch: 3,
            ..Default::default()
        };

        let mut unlock_key =
            UnlockKey::derive_with_config(&epoch_token, &conversation_id, &device_id, &config)
                .unwrap();

        // First 3 uses should succeed
        assert!(unlock_key.record_use().is_ok());
        assert!(unlock_key.record_use().is_ok());
        assert!(unlock_key.record_use().is_ok());

        // 4th use should fail (exceeds limit)
        assert!(unlock_key.record_use().is_err());
        assert!(!unlock_key.is_within_limits());
    }

    #[test]
    fn test_unlock_key_remaining_uses() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();

        let config = UnlockKeyDerivationConfig {
            max_uses_per_epoch: 5,
            ..Default::default()
        };

        let mut unlock_key =
            UnlockKey::derive_with_config(&epoch_token, &conversation_id, &device_id, &config)
                .unwrap();

        assert_eq!(unlock_key.remaining_uses(), 5);
        unlock_key.record_use().unwrap();
        assert_eq!(unlock_key.remaining_uses(), 4);
        unlock_key.record_use().unwrap();
        assert_eq!(unlock_key.remaining_uses(), 3);
    }

    #[test]
    fn test_unlock_key_attestation_binding() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();

        let attestation1 = b"attestation-data-1".to_vec();
        let attestation2 = b"attestation-data-2".to_vec();

        let key1 = UnlockKeyBuilder::new(epoch_token.clone(), conversation_id, device_id)
            .with_attestation_binding(attestation1)
            .build()
            .unwrap();

        let key2 = UnlockKeyBuilder::new(epoch_token.clone(), conversation_id, device_id)
            .with_attestation_binding(attestation2)
            .build()
            .unwrap();

        // Different attestations should produce different keys
        assert_ne!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn test_unlock_key_builder() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();
        let salt: [u8; 16] = [0xCD; 16];

        let unlock_key = UnlockKeyBuilder::new(epoch_token.clone(), conversation_id, device_id)
            .with_device_salt(salt)
            .with_max_uses(100)
            .with_attestation_binding(b"test-attestation".to_vec())
            .build()
            .unwrap();

        assert_eq!(unlock_key.epoch_id(), epoch_token.epoch_id);
        assert!(unlock_key.is_for_epoch(epoch_token.epoch_id));
        assert_eq!(unlock_key.remaining_uses(), 100);
    }

    #[test]
    fn test_unlock_key_epoch_check() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();

        let unlock_key = UnlockKey::derive(&epoch_token, &conversation_id, &device_id).unwrap();

        assert!(unlock_key.is_for_epoch(epoch_token.epoch_id));
        assert!(!unlock_key.is_for_epoch(epoch_token.epoch_id + 1));
    }

    #[test]
    fn test_unlock_key_metadata() {
        let (conversation_id, device_id, _) = create_test_ids();
        let epoch_token = create_test_epoch_token();

        let unlock_key = UnlockKey::derive(&epoch_token, &conversation_id, &device_id).unwrap();

        let metadata = unlock_key.metadata();
        assert_eq!(metadata.epoch_id, epoch_token.epoch_id);
        assert_eq!(metadata.conversation_prefix, conversation_id[..8]);
        assert_eq!(metadata.device_prefix, device_id[..8]);
        assert_eq!(metadata.use_count, 0);
    }
}
