/// Cryptographic proof-of-delivery for relay reward distribution.
///
/// This module implements the proof-of-delivery system that enables relays to
/// cryptographically prove they successfully delivered messages, earning rewards.
///
/// # Reward Mechanism
///
/// Relays earn rewards by:
/// 1. Successfully delivering messages to recipients
/// 2. Collecting recipient signatures acknowledging delivery
/// 3. Submitting proofs on-chain for reward claims
/// 4. Proofs are batched (100 per submission) for efficiency
///
/// # Security Properties
///
/// - **Non-forgeable**: Recipient signatures prevent relay from forging deliveries
/// - **Replay-resistant**: Message IDs and timestamps prevent proof reuse
/// - **Sybil-resistant**: Relay reputation scoring prevents spam attacks
/// - **Auditable**: All proofs stored on-chain for dispute resolution
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Unique identifier for a delivered message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub [u8; 32]);

impl MessageId {
    /// Creates a new message ID from a byte array.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Creates a message ID from a slice.
    ///
    /// # Errors
    /// Returns `None` if the slice is not exactly 32 bytes.
    ///
    /// # Security
    /// Always validate input length to prevent buffer overflows and ensure
    /// message ID integrity.
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        if slice.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(slice);
        Some(Self(bytes))
    }

    /// Returns the inner byte array.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Proof that a relay successfully delivered a message.
///
/// This struct contains all information needed to verify delivery and reward the relay:
/// - Message identifier
/// - Recipient's acknowledgment signature
/// - Relay's identity
/// - Timestamp of delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryProof {
    /// ID of the delivered message
    pub message_id: MessageId,

    /// Relay node's public key (identity) - stored as bytes for serialization
    #[serde(with = "serde_verifying_key")]
    pub relay_key: VerifyingKey,

    /// Recipient's public key (who received the message) - stored as bytes for serialization
    #[serde(with = "serde_verifying_key")]
    pub recipient_key: VerifyingKey,

    /// Recipient's signature acknowledging delivery
    ///
    /// Signature is over: message_id || relay_key || timestamp
    #[serde(with = "serde_signature")]
    pub recipient_signature: Signature,

    /// Unix timestamp when delivery occurred
    pub timestamp: u64,

    /// Relay's signature over the entire proof
    ///
    /// Signature is over: message_id || recipient_key || recipient_signature || timestamp
    #[serde(with = "serde_signature")]
    pub relay_signature: Signature,
}

// Custom serialization for VerifyingKey
mod serde_verifying_key {
    use ed25519_dalek::VerifyingKey;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(key: &VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(key.as_bytes())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let byte_array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("Invalid public key length"))?;
        VerifyingKey::from_bytes(&byte_array).map_err(serde::de::Error::custom)
    }
}

// Custom serialization for Signature
mod serde_signature {
    use ed25519_dalek::Signature;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(sig: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&sig.to_bytes())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let byte_array: [u8; 64] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("Invalid signature length"))?;
        Ok(Signature::from_bytes(&byte_array))
    }
}

impl DeliveryProof {
    /// Maximum age of a valid proof (7 days in seconds).
    ///
    /// Proofs older than this are rejected to prevent replay attacks.
    pub const MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;

    /// Creates a new delivery proof.
    ///
    /// # Arguments
    ///
    /// * `message_id` - Unique identifier of the delivered message
    /// * `relay_key` - Relay node's signing key (for relay_signature)
    /// * `recipient_key` - Recipient's public key
    /// * `recipient_signature` - Recipient's acknowledgment signature
    ///
    /// # Returns
    ///
    /// A complete `DeliveryProof` signed by the relay.
    pub fn new(
        message_id: MessageId,
        relay_key: &SigningKey,
        recipient_key: VerifyingKey,
        recipient_signature: Signature,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create the message for relay's signature
        let mut relay_sig_message = Vec::new();
        relay_sig_message.extend_from_slice(message_id.as_bytes());
        relay_sig_message.extend_from_slice(recipient_key.as_bytes());
        relay_sig_message.extend_from_slice(&recipient_signature.to_bytes());
        relay_sig_message.extend_from_slice(&timestamp.to_le_bytes());

        let relay_signature = relay_key.sign(&relay_sig_message);

        Self {
            message_id,
            relay_key: relay_key.verifying_key(),
            recipient_key,
            recipient_signature,
            timestamp,
            relay_signature,
        }
    }

    /// Verifies the cryptographic validity of the delivery proof.
    ///
    /// Checks:
    /// 1. Recipient's signature is valid (proves recipient acknowledged delivery)
    /// 2. Relay's signature is valid (proves proof authenticity)
    /// 3. Proof is not expired (prevents replay attacks)
    ///
    /// # Returns
    ///
    /// - `Ok(())` if proof is valid
    /// - `Err(String)` describing the validation failure
    pub fn verify(&self) -> Result<(), String> {
        // Check proof age
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now.saturating_sub(self.timestamp) > Self::MAX_AGE_SECS {
            return Err("Proof is expired".to_string());
        }

        // Verify recipient's signature (message_id || relay_key || timestamp)
        let mut recipient_msg = Vec::new();
        recipient_msg.extend_from_slice(self.message_id.as_bytes());
        recipient_msg.extend_from_slice(self.relay_key.as_bytes());
        recipient_msg.extend_from_slice(&self.timestamp.to_le_bytes());

        self.recipient_key
            .verify(&recipient_msg, &self.recipient_signature)
            .map_err(|_| "Invalid recipient signature".to_string())?;

        // Verify relay's signature (message_id || recipient_key || recipient_signature || timestamp)
        let mut relay_msg = Vec::new();
        relay_msg.extend_from_slice(self.message_id.as_bytes());
        relay_msg.extend_from_slice(self.recipient_key.as_bytes());
        relay_msg.extend_from_slice(&self.recipient_signature.to_bytes());
        relay_msg.extend_from_slice(&self.timestamp.to_le_bytes());

        self.relay_key
            .verify(&relay_msg, &self.relay_signature)
            .map_err(|_| "Invalid relay signature".to_string())?;

        Ok(())
    }

    /// Returns the age of the proof in seconds.
    pub fn age_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.timestamp)
    }

    /// Checks if the proof is expired.
    pub fn is_expired(&self) -> bool {
        self.age_secs() > Self::MAX_AGE_SECS
    }
}

/// Helper function for recipients to create acknowledgment signatures.
///
/// Recipients call this when they successfully receive a message to generate
/// the signature required for the relay's proof-of-delivery.
///
/// # Arguments
///
/// * `message_id` - ID of the received message
/// * `relay_key` - Public key of the relay who delivered it
/// * `timestamp` - Delivery timestamp (provided by relay)
/// * `recipient_key` - Recipient's signing key
///
/// # Returns
///
/// Signature that the relay can include in their `DeliveryProof`.
pub fn create_recipient_acknowledgment(
    message_id: MessageId,
    relay_key: &VerifyingKey,
    timestamp: u64,
    recipient_key: &SigningKey,
) -> Signature {
    let mut message = Vec::new();
    message.extend_from_slice(message_id.as_bytes());
    message.extend_from_slice(relay_key.as_bytes());
    message.extend_from_slice(&timestamp.to_le_bytes());

    recipient_key.sign(&message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_delivery_proof_creation() {
        let mut csprng = OsRng;
        let relay_key = SigningKey::generate(&mut csprng);
        let recipient_key = SigningKey::generate(&mut csprng);

        let message_id = MessageId::new([1u8; 32]);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Recipient creates acknowledgment
        let recipient_sig = create_recipient_acknowledgment(
            message_id,
            &relay_key.verifying_key(),
            timestamp,
            &recipient_key,
        );

        // Relay creates proof
        let proof = DeliveryProof::new(
            message_id,
            &relay_key,
            recipient_key.verifying_key(),
            recipient_sig,
        );

        // Verify proof
        assert!(proof.verify().is_ok());
        assert_eq!(proof.message_id, message_id);
        assert_eq!(proof.relay_key, relay_key.verifying_key());
    }

    #[test]
    fn test_delivery_proof_verification() {
        let mut csprng = OsRng;
        let relay_key = SigningKey::generate(&mut csprng);
        let recipient_key = SigningKey::generate(&mut csprng);

        let message_id = MessageId::new([2u8; 32]);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let recipient_sig = create_recipient_acknowledgment(
            message_id,
            &relay_key.verifying_key(),
            timestamp,
            &recipient_key,
        );

        let proof = DeliveryProof::new(
            message_id,
            &relay_key,
            recipient_key.verifying_key(),
            recipient_sig,
        );

        // Valid proof should verify
        assert!(proof.verify().is_ok());
    }

    #[test]
    fn test_invalid_recipient_signature() {
        let mut csprng = OsRng;
        let relay_key = SigningKey::generate(&mut csprng);
        let recipient_key = SigningKey::generate(&mut csprng);
        let wrong_key = SigningKey::generate(&mut csprng);

        let message_id = MessageId::new([3u8; 32]);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create signature with wrong key
        let wrong_sig = create_recipient_acknowledgment(
            message_id,
            &relay_key.verifying_key(),
            timestamp,
            &wrong_key,
        );

        let proof = DeliveryProof::new(
            message_id,
            &relay_key,
            recipient_key.verifying_key(),
            wrong_sig,
        );

        // Should fail verification
        assert!(proof.verify().is_err());
    }

    #[test]
    fn test_proof_expiration() {
        let mut csprng = OsRng;
        let relay_key = SigningKey::generate(&mut csprng);
        let recipient_key = SigningKey::generate(&mut csprng);

        let message_id = MessageId::new([4u8; 32]);

        // Create proof with old timestamp
        let old_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - DeliveryProof::MAX_AGE_SECS
            - 1;

        let recipient_sig = create_recipient_acknowledgment(
            message_id,
            &relay_key.verifying_key(),
            old_timestamp,
            &recipient_key,
        );

        let mut proof = DeliveryProof::new(
            message_id,
            &relay_key,
            recipient_key.verifying_key(),
            recipient_sig,
        );

        // Manually set old timestamp (simulating expired proof)
        proof.timestamp = old_timestamp;

        // Should fail due to expiration
        assert!(proof.is_expired());
        assert!(proof.verify().is_err());
    }

    #[test]
    fn test_message_id_operations() {
        let bytes = [42u8; 32];
        let msg_id = MessageId::new(bytes);

        assert_eq!(msg_id.as_bytes(), &bytes);
        assert_eq!(Some(msg_id), MessageId::from_slice(&bytes));

        // Test invalid length returns None
        assert_eq!(None, MessageId::from_slice(&[0u8; 31]));
        assert_eq!(None, MessageId::from_slice(&[0u8; 33]));
        assert_eq!(None, MessageId::from_slice(&[]));
    }

    #[test]
    fn test_proof_age_calculation() {
        let mut csprng = OsRng;
        let relay_key = SigningKey::generate(&mut csprng);
        let recipient_key = SigningKey::generate(&mut csprng);

        let message_id = MessageId::new([5u8; 32]);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let recipient_sig = create_recipient_acknowledgment(
            message_id,
            &relay_key.verifying_key(),
            timestamp,
            &recipient_key,
        );

        let proof = DeliveryProof::new(
            message_id,
            &relay_key,
            recipient_key.verifying_key(),
            recipient_sig,
        );

        // Fresh proof should have age close to 0
        assert!(proof.age_secs() < 5); // Allow 5 seconds tolerance
        assert!(!proof.is_expired());
    }
}
