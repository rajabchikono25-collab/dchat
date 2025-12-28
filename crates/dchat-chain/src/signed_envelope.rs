//! Unified Signed Transaction Envelope
//!
//! This module provides a single signature format for both chat-chain and currency-chain
//! transactions. All transactions are wrapped in a `SignedTransactionEnvelope` that:
//!
//! - Binds sender identity (UserId derived from public key)
//! - Provides replay protection via monotonic nonces
//! - Uses Ed25519 signatures over canonical (non-JSON) bytes
//! - Supports both Chat and Currency transaction domains
//!
//! # Light Client Flow
//!
//! 1. Derive wallet keys (BIP-44 path m/44'/1337'/account'/0/index)
//! 2. Compute UserId from public key: `address_from_public_key(&pubkey_32)`
//! 3. Build transaction payload (RegisterUser, Transfer, etc.)
//! 4. Create SignedTransactionEnvelope with nonce
//! 5. Sign using wallet's Ed25519 key
//! 6. Submit via JSON-RPC `chain_submitTransaction`
//! 7. Node verifies signature, nonce, and UserId binding before acceptance

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::currency_transactions::CurrencyTransactionType;
use crate::transactions::TransactionType;

/// Domain identifier for transaction routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EnvelopeDomain {
    /// Chat chain transactions (identity, messaging, channels)
    Chat = 0,
    /// Currency chain transactions (transfers, staking, rewards)
    Currency = 1,
}

impl EnvelopeDomain {
    /// Convert to byte for signing
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Parse from byte
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Chat),
            1 => Some(Self::Currency),
            _ => None,
        }
    }
}

/// Transaction type wrapper that works for both domains
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnifiedTransactionType {
    /// Chat chain transaction type
    Chat(TransactionType),
    /// Currency chain transaction type
    Currency(CurrencyTransactionType),
}

impl UnifiedTransactionType {
    /// Get the domain for this transaction type
    pub fn domain(&self) -> EnvelopeDomain {
        match self {
            Self::Chat(_) => EnvelopeDomain::Chat,
            Self::Currency(_) => EnvelopeDomain::Currency,
        }
    }

    /// Convert to bytes for signing (domain byte + type discriminant)
    pub fn to_signing_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(2);
        bytes.push(self.domain().to_byte());
        match self {
            Self::Chat(t) => bytes.push(*t as u8),
            Self::Currency(t) => bytes.push(*t as u8),
        }
        bytes
    }
}

/// Envelope version for forward compatibility
pub const ENVELOPE_VERSION: u8 = 1;

/// Chain ID constants
pub mod chain_ids {
    /// Mainnet chat chain
    pub const MAINNET_CHAT: u32 = 1337;
    /// Mainnet currency chain
    pub const MAINNET_CURRENCY: u32 = 1338;
    /// Testnet chat chain
    pub const TESTNET_CHAT: u32 = 13370;
    /// Testnet currency chain
    pub const TESTNET_CURRENCY: u32 = 13380;
}

/// Unified signed transaction envelope
///
/// This is the wire format for all signed transactions submitted via RPC.
/// The node verifies the signature and nonce before processing the inner payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTransactionEnvelope {
    /// Envelope format version (for upgrades)
    pub version: u8,

    /// Chain ID (prevents cross-chain replay)
    pub chain_id: u32,

    /// Sender's UserId (must equal address_from_public_key(public_key))
    pub sender: [u8; 16], // UUID bytes

    /// Replay protection nonce (must be strictly greater than last accepted)
    pub nonce: u64,

    /// Transaction domain (Chat or Currency)
    pub domain: EnvelopeDomain,

    /// Inner transaction type
    pub tx_type: UnifiedTransactionType,

    /// Serialized transaction payload (bincode-encoded inner tx)
    pub payload: Vec<u8>,

    /// BLAKE3 hash of payload (for quick integrity check)
    pub payload_hash: [u8; 32],

    /// Sender's Ed25519 public key (32 bytes)
    pub public_key: [u8; 32],

    /// Ed25519 signature over signing_bytes() (64 bytes)
    #[serde(with = "signature_bytes")]
    pub signature: [u8; 64],

    /// Submission timestamp (informational, not part of signed data)
    pub submitted_at: DateTime<Utc>,
}

/// Serde helper for [u8; 64] arrays
mod signature_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(data: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        data.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<u8> = Vec::deserialize(deserializer)?;
        if vec.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "Expected 64 bytes for signature, got {}",
                vec.len()
            )));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&vec);
        Ok(arr)
    }
}

impl SignedTransactionEnvelope {
    /// Create a new unsigned envelope (call sign() to complete)
    pub fn new(
        chain_id: u32,
        sender: Uuid,
        nonce: u64,
        tx_type: UnifiedTransactionType,
        payload: Vec<u8>,
        public_key: [u8; 32],
    ) -> Self {
        let payload_hash = blake3::hash(&payload);

        Self {
            version: ENVELOPE_VERSION,
            chain_id,
            sender: *sender.as_bytes(),
            nonce,
            domain: tx_type.domain(),
            tx_type,
            payload,
            payload_hash: *payload_hash.as_bytes(),
            public_key,
            signature: [0u8; 64], // Unsigned
            submitted_at: Utc::now(),
        }
    }

    /// Generate canonical bytes for signing
    ///
    /// Format (deterministic, no JSON):
    /// - version: 1 byte
    /// - chain_id: 4 bytes LE
    /// - sender: 16 bytes (UUID)
    /// - nonce: 8 bytes LE
    /// - domain: 1 byte
    /// - tx_type: 2 bytes (domain + discriminant)
    /// - payload_hash: 32 bytes (BLAKE3)
    ///
    /// Total: 64 bytes fixed + no variable-length fields in signed portion
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(64);

        // Version
        bytes.push(self.version);

        // Chain ID (little-endian)
        bytes.extend_from_slice(&self.chain_id.to_le_bytes());

        // Sender UUID bytes
        bytes.extend_from_slice(&self.sender);

        // Nonce (little-endian)
        bytes.extend_from_slice(&self.nonce.to_le_bytes());

        // Transaction type (includes domain)
        bytes.extend_from_slice(&self.tx_type.to_signing_bytes());

        // Payload hash (not the payload itself - deterministic)
        bytes.extend_from_slice(&self.payload_hash);

        bytes
    }

    /// Sign the envelope with an Ed25519 signing key
    pub fn sign(&mut self, signing_key: &ed25519_dalek::SigningKey) {
        use ed25519_dalek::Signer;

        let message = self.signing_bytes();
        let signature = signing_key.sign(&message);
        self.signature = signature.to_bytes();
    }

    /// Get sender as UUID
    pub fn sender_uuid(&self) -> Uuid {
        Uuid::from_bytes(self.sender)
    }

    /// Serialize for transmission (bincode)
    pub fn to_bytes(&self) -> Result<Vec<u8>, EnvelopeError> {
        bincode::serialize(self).map_err(|e| EnvelopeError::Serialization(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EnvelopeError> {
        bincode::deserialize(bytes).map_err(|e| EnvelopeError::Deserialization(e.to_string()))
    }
}

/// Envelope verification errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    /// Signature verification failed
    InvalidSignature(String),
    /// Payload hash mismatch
    PayloadHashMismatch,
    /// Nonce too low (replay attempt)
    NonceTooLow { expected_min: u64, got: u64 },
    /// UserId does not match public key
    UserIdMismatch { expected: Uuid, got: Uuid },
    /// Unknown envelope version
    UnsupportedVersion(u8),
    /// Wrong chain ID
    ChainIdMismatch { expected: u32, got: u32 },
    /// Domain mismatch between tx_type and domain field
    DomainMismatch,
    /// Serialization error
    Serialization(String),
    /// Deserialization error
    Deserialization(String),
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSignature(msg) => write!(f, "Invalid signature: {}", msg),
            Self::PayloadHashMismatch => write!(f, "Payload hash does not match"),
            Self::NonceTooLow { expected_min, got } => {
                write!(
                    f,
                    "Nonce too low: expected >= {}, got {}",
                    expected_min, got
                )
            }
            Self::UserIdMismatch { expected, got } => {
                write!(f, "UserId mismatch: expected {}, got {}", expected, got)
            }
            Self::UnsupportedVersion(v) => write!(f, "Unsupported envelope version: {}", v),
            Self::ChainIdMismatch { expected, got } => {
                write!(f, "Chain ID mismatch: expected {}, got {}", expected, got)
            }
            Self::DomainMismatch => write!(f, "Domain field does not match tx_type domain"),
            Self::Serialization(e) => write!(f, "Serialization error: {}", e),
            Self::Deserialization(e) => write!(f, "Deserialization error: {}", e),
        }
    }
}

impl std::error::Error for EnvelopeError {}

/// Envelope verifier with nonce tracking
pub struct EnvelopeVerifier {
    /// Expected chain ID
    chain_id: u32,
    /// Last accepted nonce per sender
    nonces: std::collections::HashMap<Uuid, u64>,
}

impl EnvelopeVerifier {
    /// Create a new verifier for the given chain
    pub fn new(chain_id: u32) -> Self {
        Self {
            chain_id,
            nonces: std::collections::HashMap::new(),
        }
    }

    /// Create with pre-loaded nonces (for recovery from persistent storage)
    pub fn with_nonces(chain_id: u32, nonces: std::collections::HashMap<Uuid, u64>) -> Self {
        Self { chain_id, nonces }
    }

    /// Get the last accepted nonce for a sender (0 if never seen)
    pub fn get_nonce(&self, sender: &Uuid) -> u64 {
        self.nonces.get(sender).copied().unwrap_or(0)
    }

    /// Get next expected nonce for a sender
    pub fn next_nonce(&self, sender: &Uuid) -> u64 {
        self.get_nonce(sender) + 1
    }

    /// Verify an envelope without updating state
    ///
    /// Checks:
    /// 1. Envelope version is supported
    /// 2. Chain ID matches
    /// 3. Domain field matches tx_type domain
    /// 4. Payload hash matches actual payload
    /// 5. UserId matches address_from_public_key(public_key)
    /// 6. Nonce is strictly greater than last accepted
    /// 7. Ed25519 signature is valid
    pub fn verify(&self, envelope: &SignedTransactionEnvelope) -> Result<(), EnvelopeError> {
        // 1. Version check
        if envelope.version != ENVELOPE_VERSION {
            return Err(EnvelopeError::UnsupportedVersion(envelope.version));
        }

        // 2. Chain ID check
        if envelope.chain_id != self.chain_id {
            return Err(EnvelopeError::ChainIdMismatch {
                expected: self.chain_id,
                got: envelope.chain_id,
            });
        }

        // 3. Domain consistency check
        if envelope.domain != envelope.tx_type.domain() {
            return Err(EnvelopeError::DomainMismatch);
        }

        // 4. Payload hash check
        let computed_hash = blake3::hash(&envelope.payload);
        if computed_hash.as_bytes() != &envelope.payload_hash {
            return Err(EnvelopeError::PayloadHashMismatch);
        }

        // 5. UserId binding check
        let expected_user_id = address_from_public_key(&envelope.public_key);
        let envelope_user_id = Uuid::from_bytes(envelope.sender);
        if expected_user_id != envelope_user_id {
            return Err(EnvelopeError::UserIdMismatch {
                expected: expected_user_id,
                got: envelope_user_id,
            });
        }

        // 6. Nonce check (must be strictly greater than last accepted)
        let last_nonce = self.get_nonce(&envelope_user_id);
        if envelope.nonce <= last_nonce {
            return Err(EnvelopeError::NonceTooLow {
                expected_min: last_nonce + 1,
                got: envelope.nonce,
            });
        }

        // 7. Signature verification
        self.verify_signature(envelope)?;

        Ok(())
    }

    /// Verify and accept an envelope, updating nonce state
    ///
    /// Returns the verified envelope's sender and nonce on success.
    pub fn verify_and_accept(
        &mut self,
        envelope: &SignedTransactionEnvelope,
    ) -> Result<(Uuid, u64), EnvelopeError> {
        self.verify(envelope)?;

        let sender = Uuid::from_bytes(envelope.sender);
        self.nonces.insert(sender, envelope.nonce);

        Ok((sender, envelope.nonce))
    }

    /// Verify Ed25519 signature
    fn verify_signature(&self, envelope: &SignedTransactionEnvelope) -> Result<(), EnvelopeError> {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        // Parse public key
        let verifying_key = VerifyingKey::from_bytes(&envelope.public_key)
            .map_err(|e| EnvelopeError::InvalidSignature(format!("Invalid public key: {}", e)))?;

        // Parse signature
        let signature = Signature::from_bytes(&envelope.signature);

        // Verify
        let message = envelope.signing_bytes();
        verifying_key
            .verify(&message, &signature)
            .map_err(|e| EnvelopeError::InvalidSignature(format!("Verification failed: {}", e)))?;

        Ok(())
    }

    /// Export nonces for persistence
    pub fn export_nonces(&self) -> &std::collections::HashMap<Uuid, u64> {
        &self.nonces
    }
}

/// Derive UserId from Ed25519 public key
///
/// This is the canonical mapping used across the system:
/// BLAKE3(public_key)[0..16] -> UUID
pub fn address_from_public_key(public_key_bytes: &[u8; 32]) -> Uuid {
    let hash = blake3::hash(public_key_bytes);
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&hash.as_bytes()[..16]);
    Uuid::from_bytes(uuid_bytes)
}

/// Builder for creating signed envelopes
pub struct EnvelopeBuilder {
    chain_id: u32,
    sender: Uuid,
    nonce: u64,
    tx_type: Option<UnifiedTransactionType>,
    payload: Option<Vec<u8>>,
    public_key: [u8; 32],
}

impl EnvelopeBuilder {
    /// Create a new builder
    pub fn new(chain_id: u32, public_key: [u8; 32]) -> Self {
        let sender = address_from_public_key(&public_key);
        Self {
            chain_id,
            sender,
            nonce: 0,
            tx_type: None,
            payload: None,
            public_key,
        }
    }

    /// Set the nonce
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }

    /// Set chat transaction type
    pub fn chat_tx(mut self, tx_type: TransactionType) -> Self {
        self.tx_type = Some(UnifiedTransactionType::Chat(tx_type));
        self
    }

    /// Set currency transaction type
    pub fn currency_tx(mut self, tx_type: CurrencyTransactionType) -> Self {
        self.tx_type = Some(UnifiedTransactionType::Currency(tx_type));
        self
    }

    /// Set payload (bincode-serialized inner transaction)
    pub fn payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Build and sign the envelope
    pub fn build_and_sign(
        self,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Result<SignedTransactionEnvelope, EnvelopeError> {
        let tx_type = self
            .tx_type
            .ok_or_else(|| EnvelopeError::Serialization("tx_type not set".to_string()))?;
        let payload = self
            .payload
            .ok_or_else(|| EnvelopeError::Serialization("payload not set".to_string()))?;

        let mut envelope = SignedTransactionEnvelope::new(
            self.chain_id,
            self.sender,
            self.nonce,
            tx_type,
            payload,
            self.public_key,
        );

        envelope.sign(signing_key);

        Ok(envelope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_address_from_public_key() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key().to_bytes();

        let user_id = address_from_public_key(&public_key);

        // Should be deterministic
        let user_id2 = address_from_public_key(&public_key);
        assert_eq!(user_id, user_id2);

        // Different keys should produce different IDs
        let signing_key2 = SigningKey::generate(&mut OsRng);
        let public_key2 = signing_key2.verifying_key().to_bytes();
        let user_id3 = address_from_public_key(&public_key2);
        assert_ne!(user_id, user_id3);
    }

    #[test]
    fn test_envelope_signing_and_verification() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key().to_bytes();
        let chain_id = chain_ids::TESTNET_CHAT;

        // Build and sign envelope
        let payload = b"test payload data".to_vec();
        let envelope = EnvelopeBuilder::new(chain_id, public_key)
            .nonce(1)
            .chat_tx(TransactionType::RegisterUser)
            .payload(payload)
            .build_and_sign(&signing_key)
            .unwrap();

        // Verify
        let mut verifier = EnvelopeVerifier::new(chain_id);
        assert!(verifier.verify(&envelope).is_ok());

        // Accept should update nonce
        let (sender, nonce) = verifier.verify_and_accept(&envelope).unwrap();
        assert_eq!(nonce, 1);
        assert_eq!(verifier.get_nonce(&sender), 1);
    }

    #[test]
    fn test_nonce_replay_protection() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key().to_bytes();
        let chain_id = chain_ids::TESTNET_CHAT;

        let mut verifier = EnvelopeVerifier::new(chain_id);

        // First envelope with nonce 1
        let envelope1 = EnvelopeBuilder::new(chain_id, public_key)
            .nonce(1)
            .chat_tx(TransactionType::RegisterUser)
            .payload(b"payload1".to_vec())
            .build_and_sign(&signing_key)
            .unwrap();

        verifier.verify_and_accept(&envelope1).unwrap();

        // Second envelope with same nonce should fail
        let envelope2 = EnvelopeBuilder::new(chain_id, public_key)
            .nonce(1)
            .chat_tx(TransactionType::SendDirectMessage)
            .payload(b"payload2".to_vec())
            .build_and_sign(&signing_key)
            .unwrap();

        let result = verifier.verify(&envelope2);
        assert!(matches!(result, Err(EnvelopeError::NonceTooLow { .. })));

        // Envelope with nonce 2 should succeed
        let envelope3 = EnvelopeBuilder::new(chain_id, public_key)
            .nonce(2)
            .chat_tx(TransactionType::SendDirectMessage)
            .payload(b"payload3".to_vec())
            .build_and_sign(&signing_key)
            .unwrap();

        assert!(verifier.verify_and_accept(&envelope3).is_ok());
    }

    #[test]
    fn test_wrong_chain_id_rejected() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key().to_bytes();

        let envelope = EnvelopeBuilder::new(chain_ids::TESTNET_CHAT, public_key)
            .nonce(1)
            .chat_tx(TransactionType::RegisterUser)
            .payload(b"payload".to_vec())
            .build_and_sign(&signing_key)
            .unwrap();

        // Verifier for different chain
        let verifier = EnvelopeVerifier::new(chain_ids::MAINNET_CHAT);
        let result = verifier.verify(&envelope);
        assert!(matches!(result, Err(EnvelopeError::ChainIdMismatch { .. })));
    }

    #[test]
    fn test_tampered_payload_rejected() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key().to_bytes();
        let chain_id = chain_ids::TESTNET_CHAT;

        let mut envelope = EnvelopeBuilder::new(chain_id, public_key)
            .nonce(1)
            .chat_tx(TransactionType::RegisterUser)
            .payload(b"original payload".to_vec())
            .build_and_sign(&signing_key)
            .unwrap();

        // Tamper with payload
        envelope.payload = b"tampered payload".to_vec();

        let verifier = EnvelopeVerifier::new(chain_id);
        let result = verifier.verify(&envelope);
        assert!(matches!(result, Err(EnvelopeError::PayloadHashMismatch)));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key().to_bytes();
        let chain_id = chain_ids::TESTNET_CHAT;

        let envelope = EnvelopeBuilder::new(chain_id, public_key)
            .nonce(42)
            .chat_tx(TransactionType::CreateChannel)
            .payload(b"channel creation payload".to_vec())
            .build_and_sign(&signing_key)
            .unwrap();

        let bytes = envelope.to_bytes().unwrap();
        let restored = SignedTransactionEnvelope::from_bytes(&bytes).unwrap();

        assert_eq!(envelope.version, restored.version);
        assert_eq!(envelope.chain_id, restored.chain_id);
        assert_eq!(envelope.sender, restored.sender);
        assert_eq!(envelope.nonce, restored.nonce);
        assert_eq!(envelope.payload, restored.payload);
        assert_eq!(envelope.signature, restored.signature);

        // Should still verify
        let verifier = EnvelopeVerifier::new(chain_id);
        assert!(verifier.verify(&restored).is_ok());
    }
}
