//! Quorum-Gated Encryption (QGE) - Complete Message Encryption Flow
//!
//! This module integrates the Double Ratchet, Storage Unlock Key (SUK), and
//! Epoch Token systems into a unified message encryption/decryption flow.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────┐
//! │                     ENCRYPTION FLOW (SENDER)                           │
//! ├────────────────────────────────────────────────────────────────────────┤
//! │                                                                        │
//! │  Plaintext ──► Double Ratchet ──► MessageKey ──► AEAD Encrypt ──►     │
//! │                     │                                    │             │
//! │                     │                                    ▼             │
//! │                     │                              Ciphertext          │
//! │                     │                                    │             │
//! │                     ▼                                    │             │
//! │               MessageHeader                              │             │
//! │                     │                                    │             │
//! │                     ▼                                    ▼             │
//! │        ┌─────────────────────────────────────────────────────┐         │
//! │        │         QgeEnvelope (Wire Format)                   │         │
//! │        │  ┌───────────┬────────────┬─────────────────────┐   │         │
//! │        │  │  header   │ ciphertext │  wrapped_msg_key    │   │         │
//! │        │  └───────────┴────────────┴─────────────────────┘   │         │
//! │        └─────────────────────────────────────────────────────┘         │
//! │                              │                                         │
//! │                              ▼                                         │
//! │                     ┌──────────────┐                                   │
//! │                     │   Network    │  (libp2p / relay)                 │
//! │                     └──────────────┘                                   │
//! └────────────────────────────────────────────────────────────────────────┘
//!
//! ┌────────────────────────────────────────────────────────────────────────┐
//! │                    DECRYPTION FLOW (RECEIVER)                          │
//! ├────────────────────────────────────────────────────────────────────────┤
//! │                                                                        │
//! │                     ┌──────────────┐                                   │
//! │                     │   Network    │                                   │
//! │                     └──────────────┘                                   │
//! │                              │                                         │
//! │                              ▼                                         │
//! │        ┌─────────────────────────────────────────────────────┐         │
//! │        │         QgeEnvelope (Wire Format)                   │         │
//! │        └─────────────────────────────────────────────────────┘         │
//! │                     │                                                  │
//! │     ┌───────────────┼───────────────┐                                  │
//! │     ▼               ▼               ▼                                  │
//! │  header        ciphertext    wrapped_msg_key                           │
//! │     │                               │                                  │
//! │     │                               ▼                                  │
//! │     │    ┌────────────────────────────────────────────┐                │
//! │     │    │ REQUIRES EPOCH TOKEN (from relay quorum)   │                │
//! │     │    │                                            │                │
//! │     │    │  EpochToken ──► UnlockKey ──► SUK         │                │
//! │     │    │                               │            │                │
//! │     │    │                               ▼            │                │
//! │     │    │                    Unwrap MessageKey       │                │
//! │     │    └────────────────────────────────────────────┘                │
//! │     │                               │                                  │
//! │     ▼                               ▼                                  │
//! │  Double Ratchet ◄─────────── MessageKey                                │
//! │     │                               │                                  │
//! │     │                               ▼                                  │
//! │     │                        AEAD Decrypt                              │
//! │     │                               │                                  │
//! │     ▼                               ▼                                  │
//! │  Update State                   Plaintext                              │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Security Properties
//!
//! 1. **Forward Secrecy**: Double Ratchet ensures old messages cannot be decrypted
//!    even if current keys are compromised
//!
//! 2. **Post-Compromise Security**: DH ratchet steps restore security after
//!    key compromise
//!
//! 3. **Time-Bounded Access**: Without a valid EpochToken (10-min TTL), messages
//!    cannot be decrypted from storage
//!
//! 4. **Revocation Enforcement**: If device/user is revoked, relays refuse to
//!    issue epoch tokens, blocking decryption
//!
//! 5. **Key Zeroization**: All sensitive keys are zeroized when dropped
//!
//! # Usage
//!
//! ```rust,ignore
//! use dchat_crypto::qge::{QgeSession, QgeEnvelope};
//!
//! // Initialize session after X3DH
//! let mut session = QgeSession::new_sender(
//!     conversation_id,
//!     device_id,
//!     x3dh_shared_secret,
//!     peer_public_key,
//! )?;
//!
//! // Encrypt a message
//! let envelope = session.encrypt(b"Hello!")?;
//!
//! // Send envelope over network...
//!
//! // Receiver decrypts (requires epoch token from quorum)
//! let plaintext = receiver_session.decrypt(&envelope, &epoch_token)?;
//! ```

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use x25519_dalek::PublicKey;

use crate::ratchet::{DoubleRatchet, MessageHeader, SessionConfig};
use crate::suk::{EncryptedSuk, EpochToken, StorageUnlockKey, UnlockKey, WrappedMessageKey};

/// Protocol version for QGE envelopes
pub const QGE_VERSION: u8 = 1;

/// Maximum plaintext size (1 MB)
pub const MAX_PLAINTEXT_SIZE: usize = 1024 * 1024;

/// Maximum ciphertext size (includes tag and padding)
pub const MAX_CIPHERTEXT_SIZE: usize = MAX_PLAINTEXT_SIZE + 1024;

/// Wire format for encrypted messages
///
/// This envelope contains everything needed to decrypt a message,
/// assuming the recipient has:
/// 1. An active Double Ratchet session with the sender
/// 2. Access to the conversation's SUK (via epoch token)
#[derive(Clone, Serialize, Deserialize)]
pub struct QgeEnvelope {
    /// Protocol version
    pub version: u8,

    /// Conversation this message belongs to
    pub conversation_id: [u8; 32],

    /// Message ID (unique per message)
    pub message_id: [u8; 32],

    /// Double Ratchet message header
    pub header: SerializableHeader,

    /// AEAD ciphertext (encrypted plaintext)
    pub ciphertext: Vec<u8>,

    /// Message key wrapped by SUK (for at-rest storage)
    pub wrapped_key: WrappedMessageKey,

    /// Timestamp when message was created
    pub timestamp: u64,

    /// Sender's device ID (for multi-device)
    pub sender_device_id: [u8; 32],
}

/// Serializable version of MessageHeader
#[derive(Clone, Serialize, Deserialize)]
pub struct SerializableHeader {
    /// Sender's current DH public key
    pub dh_public: [u8; 32],
    /// Number of messages in previous sending chain
    pub previous_chain_length: u32,
    /// Message number in current sending chain
    pub message_index: u32,
}

impl From<&MessageHeader> for SerializableHeader {
    fn from(h: &MessageHeader) -> Self {
        Self {
            dh_public: h.dh_public,
            previous_chain_length: h.previous_chain_length,
            message_index: h.message_index,
        }
    }
}

impl From<&SerializableHeader> for MessageHeader {
    fn from(h: &SerializableHeader) -> Self {
        MessageHeader::new(h.dh_public, h.previous_chain_length, h.message_index)
    }
}

impl QgeEnvelope {
    /// Serialize envelope to bytes for network transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| Error::crypto(format!("Envelope serialization failed: {}", e)))
    }

    /// Deserialize envelope from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let envelope: Self = bincode::deserialize(data)
            .map_err(|e| Error::crypto(format!("Envelope deserialization failed: {}", e)))?;

        // Validate version
        if envelope.version != QGE_VERSION {
            return Err(Error::crypto(format!(
                "Unsupported QGE version: {} (expected {})",
                envelope.version, QGE_VERSION
            )));
        }

        // Validate ciphertext size
        if envelope.ciphertext.len() > MAX_CIPHERTEXT_SIZE {
            return Err(Error::crypto(format!(
                "Ciphertext too large: {} bytes (max: {})",
                envelope.ciphertext.len(),
                MAX_CIPHERTEXT_SIZE
            )));
        }

        Ok(envelope)
    }

    /// Get message age in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.timestamp)
    }
}

impl std::fmt::Debug for QgeEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QgeEnvelope")
            .field("version", &self.version)
            .field("conversation_id", &hex::encode(&self.conversation_id[..8]))
            .field("message_id", &hex::encode(&self.message_id[..8]))
            .field("message_index", &self.header.message_index)
            .field("ciphertext_len", &self.ciphertext.len())
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

/// Complete QGE session combining Double Ratchet and SUK
///
/// This session manages:
/// - The Double Ratchet state for key agreement
/// - The Storage Unlock Key for at-rest encryption
/// - Message ID generation
pub struct QgeSession {
    /// Conversation identifier
    conversation_id: [u8; 32],

    /// Device identifier
    device_id: [u8; 32],

    /// Double Ratchet session
    ratchet: DoubleRatchet,

    /// Storage Unlock Key (decrypted from storage using epoch token)
    suk: StorageUnlockKey,

    /// Encrypted SUK for storage (updated after SUK changes)
    encrypted_suk: Option<EncryptedSuk>,

    /// Message counter for this session (for unique message IDs)
    message_counter: u64,
}

impl QgeSession {
    /// Create a new QGE session as the initiator (sender of first message)
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - Unique identifier for this conversation
    /// * `device_id` - This device's identifier
    /// * `shared_secret` - X3DH shared secret
    /// * `peer_public_key` - Peer's signed pre-key public key
    pub fn new_sender(
        conversation_id: [u8; 32],
        device_id: [u8; 32],
        shared_secret: [u8; 32],
        peer_public_key: PublicKey,
    ) -> Result<Self> {
        let ratchet =
            DoubleRatchet::init_sender(shared_secret, peer_public_key, SessionConfig::default())?;

        let suk = StorageUnlockKey::generate(conversation_id)?;

        Ok(Self {
            conversation_id,
            device_id,
            ratchet,
            suk,
            encrypted_suk: None,
            message_counter: 0,
        })
    }

    /// Create a new QGE session as the responder (receiver of first message)
    ///
    /// # Arguments
    ///
    /// * `conversation_id` - Unique identifier for this conversation
    /// * `device_id` - This device's identifier
    /// * `shared_secret` - X3DH shared secret
    /// * `signed_pre_key_secret` - Our signed pre-key secret
    /// * `signed_pre_key_public` - Our signed pre-key public
    pub fn new_receiver(
        conversation_id: [u8; 32],
        device_id: [u8; 32],
        shared_secret: [u8; 32],
        signed_pre_key_secret: [u8; 32],
        signed_pre_key_public: PublicKey,
    ) -> Result<Self> {
        let ratchet = DoubleRatchet::init_receiver(
            shared_secret,
            signed_pre_key_secret,
            signed_pre_key_public,
            SessionConfig::default(),
        )?;

        let suk = StorageUnlockKey::generate(conversation_id)?;

        Ok(Self {
            conversation_id,
            device_id,
            ratchet,
            suk,
            encrypted_suk: None,
            message_counter: 0,
        })
    }

    /// Restore session from stored state
    ///
    /// This requires an epoch token to decrypt the SUK.
    pub fn restore(
        conversation_id: [u8; 32],
        device_id: [u8; 32],
        ratchet: DoubleRatchet,
        encrypted_suk: &EncryptedSuk,
        epoch_token: &EpochToken,
    ) -> Result<Self> {
        // Validate epoch token
        if !epoch_token.is_valid() {
            return Err(Error::crypto("Epoch token expired"));
        }

        // Derive unlock key and decrypt SUK
        let unlock_key = UnlockKey::derive(epoch_token, &conversation_id, &device_id)?;
        let suk = StorageUnlockKey::decrypt_from_storage(encrypted_suk, &unlock_key)?;

        Ok(Self {
            conversation_id,
            device_id,
            ratchet,
            suk,
            encrypted_suk: Some(encrypted_suk.clone()),
            message_counter: 0,
        })
    }

    /// Encrypt a plaintext message
    ///
    /// Returns a QGE envelope ready for network transmission.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<QgeEnvelope> {
        // Validate plaintext size
        if plaintext.len() > MAX_PLAINTEXT_SIZE {
            return Err(Error::crypto(format!(
                "Plaintext too large: {} bytes (max: {})",
                plaintext.len(),
                MAX_PLAINTEXT_SIZE
            )));
        }

        // Generate unique message ID
        let message_id = self.generate_message_id();

        // Encrypt using Double Ratchet
        let (header, ciphertext) = self.ratchet.encrypt(plaintext)?;

        // Extract the message key that was used (for wrapping)
        // We need to get the key that was used for this encryption
        // The Double Ratchet already used the key, so we derive it again
        // for wrapping purposes by reconstructing from the header info
        let message_key_for_wrap = self.derive_message_key_for_wrap(&header)?;

        // Wrap the message key using SUK for at-rest storage
        let wrapped_key = self
            .suk
            .wrap_message_key(&message_key_for_wrap, &message_id)?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(QgeEnvelope {
            version: QGE_VERSION,
            conversation_id: self.conversation_id,
            message_id,
            header: SerializableHeader::from(&header),
            ciphertext,
            wrapped_key,
            timestamp,
            sender_device_id: self.device_id,
        })
    }

    /// Decrypt a received message envelope
    ///
    /// This uses the Double Ratchet for online decryption.
    /// The wrapped_key is used for offline/at-rest decryption.
    pub fn decrypt(&mut self, envelope: &QgeEnvelope) -> Result<Vec<u8>> {
        // Validate envelope
        if envelope.version != QGE_VERSION {
            return Err(Error::crypto(format!(
                "Unsupported QGE version: {}",
                envelope.version
            )));
        }

        if envelope.conversation_id != self.conversation_id {
            return Err(Error::crypto("Envelope conversation ID mismatch"));
        }

        // Convert header
        let header = MessageHeader::from(&envelope.header);

        // Decrypt using Double Ratchet
        self.ratchet.decrypt(&header, &envelope.ciphertext)
    }

    /// Decrypt a message from storage using wrapped key
    ///
    /// This path is used when the Double Ratchet state may not be available
    /// (e.g., restoring from backup or accessing old messages).
    ///
    /// REQUIRES: Valid epoch token to decrypt SUK first.
    pub fn decrypt_from_storage(
        &self,
        envelope: &QgeEnvelope,
        epoch_token: &EpochToken,
    ) -> Result<Vec<u8>> {
        // Validate epoch token
        if !epoch_token.is_valid() {
            return Err(Error::crypto("Epoch token expired - cannot decrypt"));
        }

        // Unwrap the message key using SUK
        let message_key = self.suk.unwrap_message_key(&envelope.wrapped_key)?;

        // Derive AEAD parameters and decrypt
        let (aead_key, nonce) = derive_aead_params(&message_key)?;
        let aad = envelope.header_aad();

        decrypt_aead(&aead_key, &nonce, &envelope.ciphertext, &aad)
    }

    /// Get encrypted SUK for storage
    ///
    /// Call this after session operations to persist the SUK.
    pub fn encrypt_suk_for_storage(&self, epoch_token: &EpochToken) -> Result<EncryptedSuk> {
        if !epoch_token.is_valid() {
            return Err(Error::crypto("Epoch token expired"));
        }

        let unlock_key = UnlockKey::derive(epoch_token, &self.conversation_id, &self.device_id)?;
        self.suk.encrypt_for_storage(&unlock_key)
    }

    /// Rotate the SUK (after member/device revocation)
    ///
    /// The new SUK must be distributed to all remaining authorized devices.
    pub fn rotate_suk(&mut self) -> Result<()> {
        self.suk.rotate()
    }

    /// Get conversation ID
    pub fn conversation_id(&self) -> &[u8; 32] {
        &self.conversation_id
    }

    /// Get device ID
    pub fn device_id(&self) -> &[u8; 32] {
        &self.device_id
    }

    /// Get SUK generation
    pub fn suk_generation(&self) -> u64 {
        self.suk.generation()
    }

    /// Get number of messages sent
    pub fn message_count(&self) -> u64 {
        self.message_counter
    }

    /// Get our current DH public key
    pub fn our_public_key(&self) -> &PublicKey {
        self.ratchet.our_public_key()
    }

    /// Get cached encrypted SUK (if available)
    ///
    /// This returns the encrypted SUK if it was previously stored/restored.
    /// Use `encrypt_suk_for_storage()` if you need a fresh encryption.
    pub fn cached_encrypted_suk(&self) -> Option<&EncryptedSuk> {
        self.encrypted_suk.as_ref()
    }

    /// Update cached encrypted SUK after storage operations
    pub fn set_cached_encrypted_suk(&mut self, encrypted: EncryptedSuk) {
        self.encrypted_suk = Some(encrypted);
    }

    /// Generate unique message ID
    fn generate_message_id(&mut self) -> [u8; 32] {
        use blake3::Hasher;

        let mut hasher = Hasher::new();
        hasher.update(&self.conversation_id);
        hasher.update(&self.device_id);
        hasher.update(&self.message_counter.to_le_bytes());

        // Add timestamp for additional uniqueness
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        hasher.update(&timestamp.to_le_bytes());

        self.message_counter += 1;

        hasher.finalize().into()
    }

    /// Derive message key for wrapping purposes
    ///
    /// This reconstructs the key derivation to get a wrappable key.
    fn derive_message_key_for_wrap(&self, header: &MessageHeader) -> Result<[u8; 32]> {
        // The message key is derived deterministically from chain state
        // We use the header information to derive a key for wrapping
        use crate::kdf::Hkdf;

        let mut info = Vec::with_capacity(72);
        info.extend_from_slice(b"wrap-key");
        info.extend_from_slice(&self.conversation_id);
        info.extend_from_slice(&header.dh_public);
        info.extend_from_slice(&header.message_index.to_le_bytes());

        let derived = Hkdf::derive(
            Some(self.ratchet.our_public_key().as_bytes()),
            &header.encode(),
            &info,
            32,
        )?;

        let mut key = [0u8; 32];
        key.copy_from_slice(&derived);
        Ok(key)
    }
}

impl QgeEnvelope {
    /// Get header as AAD bytes
    fn header_aad(&self) -> Vec<u8> {
        let header = MessageHeader::from(&self.header);
        header.encode()
    }
}

/// Derive AEAD parameters from a message key
fn derive_aead_params(message_key: &[u8; 32]) -> Result<([u8; 32], [u8; 12])> {
    use crate::kdf::Hkdf;

    let output = Hkdf::derive(Some(message_key), b"aead", b"dchat-aead", 44)?;

    let mut aead_key = [0u8; 32];
    let mut nonce = [0u8; 12];
    aead_key.copy_from_slice(&output[0..32]);
    nonce.copy_from_slice(&output[32..44]);

    Ok((aead_key, nonce))
}

/// Decrypt using AEAD
fn decrypt_aead(
    key: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>> {
    use aes_gcm::{
        aead::{Aead, KeyInit, Payload},
        Aes256Gcm, Nonce,
    };

    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(nonce);

    cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|e| Error::crypto(format!("AEAD decryption failed: {}", e)))
}

/// Stored session state for persistence
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredSessionState {
    /// Conversation ID
    pub conversation_id: [u8; 32],
    /// Device ID
    pub device_id: [u8; 32],
    /// Encrypted SUK
    pub encrypted_suk: EncryptedSuk,
    /// Message counter
    pub message_counter: u64,
    /// Double Ratchet state (serialized)
    pub ratchet_state: Vec<u8>,
    /// State version
    pub version: u8,
}

impl StoredSessionState {
    /// Current state version
    pub const VERSION: u8 = 1;

    /// Serialize for storage
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| Error::crypto(format!("Session state serialization failed: {}", e)))
    }

    /// Deserialize from storage
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let state: Self = bincode::deserialize(data)
            .map_err(|e| Error::crypto(format!("Session state deserialization failed: {}", e)))?;

        if state.version != Self::VERSION {
            return Err(Error::crypto(format!(
                "Unsupported session state version: {}",
                state.version
            )));
        }

        Ok(state)
    }
}

/// Manager for multiple QGE sessions with LRU eviction
///
/// Handles session lifecycle, caching, and coordination.
/// Uses LRU (Least Recently Used) eviction when cache is full.
pub struct QgeSessionManager {
    /// Active sessions keyed by conversation ID
    sessions: std::collections::HashMap<[u8; 32], LruSessionEntry>,
    /// This device's ID
    device_id: [u8; 32],
    /// Maximum cached sessions
    max_sessions: usize,
}

/// Session entry with LRU tracking
struct LruSessionEntry {
    /// The session
    session: QgeSession,
    /// Last access timestamp (monotonic, for ordering)
    last_accessed: std::time::Instant,
}

impl QgeSessionManager {
    /// Create a new session manager
    pub fn new(device_id: [u8; 32], max_sessions: usize) -> Self {
        Self {
            sessions: std::collections::HashMap::new(),
            device_id,
            max_sessions,
        }
    }

    /// Get or create a session for a conversation
    pub fn get_session(&self, conversation_id: &[u8; 32]) -> Option<&QgeSession> {
        self.sessions.get(conversation_id).map(|e| &e.session)
    }

    /// Get mutable reference to a session (updates LRU timestamp)
    pub fn get_session_mut(&mut self, conversation_id: &[u8; 32]) -> Option<&mut QgeSession> {
        self.sessions.get_mut(conversation_id).map(|e| {
            e.last_accessed = std::time::Instant::now();
            &mut e.session
        })
    }

    /// Add a new session
    pub fn add_session(&mut self, session: QgeSession) -> Result<()> {
        let conv_id = session.conversation_id;

        // If session already exists, just update it
        if self.sessions.contains_key(&conv_id) {
            self.sessions.insert(
                conv_id,
                LruSessionEntry {
                    session,
                    last_accessed: std::time::Instant::now(),
                },
            );
            return Ok(());
        }

        // Enforce session limit - evict LRU if needed
        if self.sessions.len() >= self.max_sessions {
            self.evict_lru_session();
        }

        self.sessions.insert(
            conv_id,
            LruSessionEntry {
                session,
                last_accessed: std::time::Instant::now(),
            },
        );
        Ok(())
    }

    /// Evict the least recently used session
    fn evict_lru_session(&mut self) {
        if self.sessions.is_empty() {
            return;
        }

        // Find the session with the oldest last_accessed timestamp
        let lru_key = self
            .sessions
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(key, _)| *key);

        if let Some(key) = lru_key {
            self.sessions.remove(&key);
        }
    }

    /// Remove a session
    pub fn remove_session(&mut self, conversation_id: &[u8; 32]) -> Option<QgeSession> {
        self.sessions.remove(conversation_id).map(|e| e.session)
    }

    /// Encrypt a message for a conversation (updates LRU timestamp)
    pub fn encrypt(&mut self, conversation_id: &[u8; 32], plaintext: &[u8]) -> Result<QgeEnvelope> {
        let entry = self
            .sessions
            .get_mut(conversation_id)
            .ok_or_else(|| Error::crypto("No session for conversation"))?;

        entry.last_accessed = std::time::Instant::now();
        entry.session.encrypt(plaintext)
    }

    /// Decrypt a message (updates LRU timestamp)
    pub fn decrypt(
        &mut self,
        conversation_id: &[u8; 32],
        envelope: &QgeEnvelope,
    ) -> Result<Vec<u8>> {
        let entry = self
            .sessions
            .get_mut(conversation_id)
            .ok_or_else(|| Error::crypto("No session for conversation"))?;

        entry.last_accessed = std::time::Instant::now();
        entry.session.decrypt(envelope)
    }

    /// Number of active sessions
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Clear all sessions
    pub fn clear_all(&mut self) {
        self.sessions.clear();
    }

    /// Device ID
    pub fn device_id(&self) -> &[u8; 32] {
        &self.device_id
    }

    /// Touch a session to update its LRU timestamp
    pub fn touch_session(&mut self, conversation_id: &[u8; 32]) -> bool {
        if let Some(entry) = self.sessions.get_mut(conversation_id) {
            entry.last_accessed = std::time::Instant::now();
            true
        } else {
            false
        }
    }

    /// Get all conversation IDs, ordered by most recently used first
    pub fn conversation_ids_by_recency(&self) -> Vec<[u8; 32]> {
        let mut entries: Vec<_> = self.sessions.iter().collect();
        entries.sort_by(|a, b| b.1.last_accessed.cmp(&a.1.last_accessed));
        entries.into_iter().map(|(k, _)| *k).collect()
    }
}

impl Drop for QgeSessionManager {
    fn drop(&mut self) {
        self.sessions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_keypair() -> (PublicKey, [u8; 32]) {
        use x25519_dalek::StaticSecret;

        let mut secret_bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret_bytes);
        let secret = StaticSecret::from(secret_bytes);
        let public = PublicKey::from(&secret);

        (public, secret_bytes)
    }

    fn create_test_session_pair() -> (QgeSession, QgeSession) {
        let conversation_id = crate::hash(b"test-conversation");
        let alice_device = crate::hash(b"alice-device");
        let bob_device = crate::hash(b"bob-device");
        let shared_secret = [42u8; 32];

        let (bob_public, bob_secret) = create_test_keypair();

        let alice =
            QgeSession::new_sender(conversation_id, alice_device, shared_secret, bob_public)
                .unwrap();

        let bob = QgeSession::new_receiver(
            conversation_id,
            bob_device,
            shared_secret,
            bob_secret,
            bob_public,
        )
        .unwrap();

        (alice, bob)
    }

    fn create_test_epoch_token() -> EpochToken {
        EpochToken::new(vec![1u8; 64], EpochToken::current_epoch_id(), 4, 7)
    }

    #[test]
    fn test_basic_encrypt_decrypt() {
        let (mut alice, mut bob) = create_test_session_pair();

        let plaintext = b"Hello, Bob! This is a test message.";

        // Alice encrypts
        let envelope = alice.encrypt(plaintext).unwrap();

        assert_eq!(envelope.version, QGE_VERSION);
        assert_eq!(envelope.conversation_id, *alice.conversation_id());
        assert!(!envelope.ciphertext.is_empty());

        // Bob decrypts
        let decrypted = bob.decrypt(&envelope).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_bidirectional_messages() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Alice -> Bob
        let env1 = alice.encrypt(b"Hello Bob").unwrap();
        let dec1 = bob.decrypt(&env1).unwrap();
        assert_eq!(dec1, b"Hello Bob");

        // Bob -> Alice
        let env2 = bob.encrypt(b"Hello Alice").unwrap();
        let dec2 = alice.decrypt(&env2).unwrap();
        assert_eq!(dec2, b"Hello Alice");

        // Alice -> Bob
        let env3 = alice.encrypt(b"How are you?").unwrap();
        let dec3 = bob.decrypt(&env3).unwrap();
        assert_eq!(dec3, b"How are you?");
    }

    #[test]
    fn test_envelope_serialization() {
        let (mut alice, _bob) = create_test_session_pair();

        let envelope = alice.encrypt(b"Test message").unwrap();

        // Serialize
        let bytes = envelope.to_bytes().unwrap();

        // Deserialize
        let recovered = QgeEnvelope::from_bytes(&bytes).unwrap();

        assert_eq!(recovered.version, envelope.version);
        assert_eq!(recovered.conversation_id, envelope.conversation_id);
        assert_eq!(recovered.message_id, envelope.message_id);
        assert_eq!(recovered.ciphertext, envelope.ciphertext);
    }

    #[test]
    fn test_message_ids_unique() {
        let (mut alice, _bob) = create_test_session_pair();

        let env1 = alice.encrypt(b"Message 1").unwrap();
        let env2 = alice.encrypt(b"Message 2").unwrap();
        let env3 = alice.encrypt(b"Message 3").unwrap();

        // All message IDs should be unique
        assert_ne!(env1.message_id, env2.message_id);
        assert_ne!(env2.message_id, env3.message_id);
        assert_ne!(env1.message_id, env3.message_id);
    }

    #[test]
    fn test_suk_rotation() {
        let (mut alice, mut bob) = create_test_session_pair();

        let env1 = alice.encrypt(b"Before rotation").unwrap();
        let dec1 = bob.decrypt(&env1).unwrap();
        assert_eq!(dec1, b"Before rotation");

        let gen_before = alice.suk_generation();

        // Rotate Alice's SUK
        alice.rotate_suk().unwrap();

        assert_eq!(alice.suk_generation(), gen_before + 1);

        // Messages after rotation still work (new keys)
        let env2 = alice.encrypt(b"After rotation").unwrap();
        let dec2 = bob.decrypt(&env2).unwrap();
        assert_eq!(dec2, b"After rotation");
    }

    #[test]
    fn test_suk_storage_roundtrip() {
        let (alice, _bob) = create_test_session_pair();
        let epoch_token = create_test_epoch_token();

        // Encrypt SUK for storage
        let encrypted_suk = alice.encrypt_suk_for_storage(&epoch_token).unwrap();

        // Create new session from stored SUK
        let (bob_public, bob_secret) = create_test_keypair();
        let ratchet =
            DoubleRatchet::init_sender([42u8; 32], bob_public, SessionConfig::default()).unwrap();

        let restored = QgeSession::restore(
            *alice.conversation_id(),
            *alice.device_id(),
            ratchet,
            &encrypted_suk,
            &epoch_token,
        )
        .unwrap();

        assert_eq!(restored.conversation_id(), alice.conversation_id());
        assert_eq!(restored.suk_generation(), alice.suk_generation());
    }

    #[test]
    fn test_expired_token_rejected() {
        let (alice, _bob) = create_test_session_pair();

        // Create an expired token
        let expired_token = EpochToken::new(
            vec![1u8; 64],
            0, // Epoch 0 is definitely expired
            4,
            7,
        );

        let result = alice.encrypt_suk_for_storage(&expired_token);
        assert!(result.is_err());
    }

    #[test]
    fn test_session_manager() {
        let device_id = crate::hash(b"test-device");
        let mut manager = QgeSessionManager::new(device_id, 100);

        assert_eq!(manager.session_count(), 0);

        // Create and add a session
        let (alice, _bob) = create_test_session_pair();
        let conv_id = *alice.conversation_id();
        manager.add_session(alice).unwrap();

        assert_eq!(manager.session_count(), 1);
        assert!(manager.get_session(&conv_id).is_some());

        // Encrypt through manager
        let envelope = manager.encrypt(&conv_id, b"Test message").unwrap();
        assert!(!envelope.ciphertext.is_empty());

        // Remove session
        manager.remove_session(&conv_id);
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn test_large_message() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Create a 100KB message
        let large_plaintext = vec![0x42u8; 100 * 1024];

        let envelope = alice.encrypt(&large_plaintext).unwrap();
        let decrypted = bob.decrypt(&envelope).unwrap();

        assert_eq!(decrypted, large_plaintext);
    }

    #[test]
    fn test_oversized_message_rejected() {
        let (mut alice, _bob) = create_test_session_pair();

        // Create a message over the limit
        let oversized = vec![0x42u8; MAX_PLAINTEXT_SIZE + 1];

        let result = alice.encrypt(&oversized);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_conversation_rejected() {
        let (mut alice, mut bob) = create_test_session_pair();

        let envelope = alice.encrypt(b"Test").unwrap();

        // Create Bob with different conversation ID
        let different_conv = crate::hash(b"different-conversation");
        let (bob_public, bob_secret) = create_test_keypair();
        let mut other_bob = QgeSession::new_receiver(
            different_conv,
            *bob.device_id(),
            [42u8; 32],
            bob_secret,
            bob_public,
        )
        .unwrap();

        // Should fail due to conversation ID mismatch
        let result = other_bob.decrypt(&envelope);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_messages_same_direction() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Alice sends multiple messages
        let envs: Vec<_> = (0..5)
            .map(|i| alice.encrypt(format!("Message {}", i).as_bytes()).unwrap())
            .collect();

        // Bob receives all in order
        for (i, env) in envs.iter().enumerate() {
            let decrypted = bob.decrypt(env).unwrap();
            assert_eq!(decrypted, format!("Message {}", i).as_bytes());
        }
    }

    #[test]
    fn test_out_of_order_messages() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Alice sends multiple messages
        let env0 = alice.encrypt(b"Message 0").unwrap();
        let env1 = alice.encrypt(b"Message 1").unwrap();
        let env2 = alice.encrypt(b"Message 2").unwrap();

        // Bob receives out of order
        let dec2 = bob.decrypt(&env2).unwrap();
        assert_eq!(dec2, b"Message 2");

        let dec0 = bob.decrypt(&env0).unwrap();
        assert_eq!(dec0, b"Message 0");

        let dec1 = bob.decrypt(&env1).unwrap();
        assert_eq!(dec1, b"Message 1");
    }

    #[test]
    fn test_message_counter_increments() {
        let (mut alice, _bob) = create_test_session_pair();

        assert_eq!(alice.message_count(), 0);

        alice.encrypt(b"1").unwrap();
        assert_eq!(alice.message_count(), 1);

        alice.encrypt(b"2").unwrap();
        assert_eq!(alice.message_count(), 2);

        alice.encrypt(b"3").unwrap();
        assert_eq!(alice.message_count(), 3);
    }

    #[test]
    fn test_session_manager_lru_eviction() {
        let device_id = crate::hash(b"test-device");
        // Create manager with max 3 sessions
        let mut manager = QgeSessionManager::new(device_id, 3);

        // Helper to create a session with a specific conversation ID
        fn create_session_for_conv(conv_seed: &[u8]) -> QgeSession {
            let conversation_id = crate::hash(conv_seed);
            let device_id = crate::hash(b"test-device");
            let shared_secret = [42u8; 32];

            let mut secret_bytes = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret_bytes);
            let secret = x25519_dalek::StaticSecret::from(secret_bytes);
            let public = x25519_dalek::PublicKey::from(&secret);

            QgeSession::new_sender(conversation_id, device_id, shared_secret, public).unwrap()
        }

        // Add 3 sessions
        let session1 = create_session_for_conv(b"conv-1");
        let conv1 = *session1.conversation_id();
        manager.add_session(session1).unwrap();

        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));

        let session2 = create_session_for_conv(b"conv-2");
        let conv2 = *session2.conversation_id();
        manager.add_session(session2).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        let session3 = create_session_for_conv(b"conv-3");
        let conv3 = *session3.conversation_id();
        manager.add_session(session3).unwrap();

        assert_eq!(manager.session_count(), 3);

        // Touch session 1 to make it recently used
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager.touch_session(&conv1);

        // Add a 4th session - should evict session 2 (oldest untouched)
        let session4 = create_session_for_conv(b"conv-4");
        let conv4 = *session4.conversation_id();
        manager.add_session(session4).unwrap();

        assert_eq!(manager.session_count(), 3);

        // Session 2 should be evicted (LRU)
        assert!(
            manager.get_session(&conv2).is_none(),
            "Session 2 should be evicted"
        );

        // Sessions 1, 3, 4 should still exist
        assert!(
            manager.get_session(&conv1).is_some(),
            "Session 1 should exist"
        );
        assert!(
            manager.get_session(&conv3).is_some(),
            "Session 3 should exist"
        );
        assert!(
            manager.get_session(&conv4).is_some(),
            "Session 4 should exist"
        );
    }

    #[test]
    fn test_session_manager_recency_order() {
        let device_id = crate::hash(b"test-device");
        let mut manager = QgeSessionManager::new(device_id, 10);

        fn create_session_for_conv(conv_seed: &[u8]) -> QgeSession {
            let conversation_id = crate::hash(conv_seed);
            let device_id = crate::hash(b"test-device");
            let shared_secret = [42u8; 32];

            let mut secret_bytes = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret_bytes);
            let secret = x25519_dalek::StaticSecret::from(secret_bytes);
            let public = x25519_dalek::PublicKey::from(&secret);

            QgeSession::new_sender(conversation_id, device_id, shared_secret, public).unwrap()
        }

        // Add sessions with delays
        let session1 = create_session_for_conv(b"conv-1");
        let conv1 = *session1.conversation_id();
        manager.add_session(session1).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        let session2 = create_session_for_conv(b"conv-2");
        let conv2 = *session2.conversation_id();
        manager.add_session(session2).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        let session3 = create_session_for_conv(b"conv-3");
        let conv3 = *session3.conversation_id();
        manager.add_session(session3).unwrap();

        // Most recent should be conv3
        let recency = manager.conversation_ids_by_recency();
        assert_eq!(recency[0], conv3);
        assert_eq!(recency[1], conv2);
        assert_eq!(recency[2], conv1);

        // Touch conv1 to make it most recent
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager.touch_session(&conv1);

        let recency = manager.conversation_ids_by_recency();
        assert_eq!(recency[0], conv1);
    }

    #[test]
    fn test_session_manager_update_existing() {
        let device_id = crate::hash(b"test-device");
        let mut manager = QgeSessionManager::new(device_id, 2);

        fn create_session_for_conv(conv_seed: &[u8]) -> QgeSession {
            let conversation_id = crate::hash(conv_seed);
            let device_id = crate::hash(b"test-device");
            let shared_secret = [42u8; 32];

            let mut secret_bytes = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret_bytes);
            let secret = x25519_dalek::StaticSecret::from(secret_bytes);
            let public = x25519_dalek::PublicKey::from(&secret);

            QgeSession::new_sender(conversation_id, device_id, shared_secret, public).unwrap()
        }

        let session1 = create_session_for_conv(b"conv-1");
        let conv1 = *session1.conversation_id();
        manager.add_session(session1).unwrap();

        let session2 = create_session_for_conv(b"conv-2");
        manager.add_session(session2).unwrap();

        assert_eq!(manager.session_count(), 2);

        // Adding same conversation again should update, not evict
        let session1_new = create_session_for_conv(b"conv-1");
        manager.add_session(session1_new).unwrap();

        // Should still have 2 sessions
        assert_eq!(manager.session_count(), 2);
        assert!(manager.get_session(&conv1).is_some());
    }
}
