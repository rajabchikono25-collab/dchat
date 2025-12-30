//! FROST Threshold Signing for Epoch Token Issuance
//!
//! This module integrates the FROST (Flexible Round-Optimized Schnorr Threshold)
//! signing protocol from `dchat-identity::mpc_frost` with the epoch token system.
//!
//! # Overview
//!
//! Epoch tokens require k-of-n threshold signatures from a relay committee.
//! This module provides:
//!
//! - Committee-specific FROST key generation and management
//! - Round 1 (commitment) generation for relay participation
//! - Round 2 (signature share) generation
//! - Signature aggregation on the client side
//! - Verification of aggregated threshold signatures
//!
//! # Protocol Flow
//!
//! ```text
//! ┌─────────┐         ┌─────────────────────┐         ┌─────────┐
//! │ Client  │         │  Relay Committee    │         │ Storage │
//! └────┬────┘         │  (k-of-n relays)    │         └────┬────┘
//!      │              └──────────┬──────────┘              │
//!      │  1. Request Token       │                         │
//!      │─────────────────────────>                         │
//!      │                         │                         │
//!      │  2. Round 1 Commitment  │                         │
//!      │<─────────────────────────                         │
//!      │        (from each relay)│                         │
//!      │                         │                         │
//!      │  3. Broadcast Commitments                         │
//!      │─────────────────────────>                         │
//!      │                         │                         │
//!      │  4. Round 2 Sig Share   │                         │
//!      │<─────────────────────────                         │
//!      │        (from each relay)│                         │
//!      │                         │                         │
//!      │  5. Aggregate Signature │                         │
//!      │  (client-side)          │                         │
//!      │                         │                         │
//!      │  6. Store encrypted msg │                         │
//!      │───────────────────────────────────────────────────>
//! ```
//!
//! # Security Properties
//!
//! - Uses audited frost-ed25519 library (NCC Group Security Audit, April 2023)
//! - Existentially unforgeable under chosen message attack (EU-CMA)
//! - Robust against malicious signers (abort detection)
//! - Forward secrecy via ephemeral nonces
//! - Compatible with standard Ed25519 signature verification

use dchat_core::error::{Error, Result};
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Serde helper for [u8; 64] arrays (signatures)
mod signature_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec = Vec::<u8>::deserialize(deserializer)?;
        if vec.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "Expected 64 bytes, got {}",
                vec.len()
            )));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&vec);
        Ok(arr)
    }
}

/// FROST signing errors specific to epoch token issuance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrostSigningError {
    /// Insufficient signers available
    InsufficientSigners { required: u8, available: u8 },
    /// Invalid signature share
    InvalidSignatureShare { relay_id: [u8; 32], reason: String },
    /// Key share not found for relay
    KeyShareNotFound { relay_id: [u8; 32] },
    /// Session not found or expired
    SessionNotFound { session_id: String },
    /// Invalid session state
    InvalidSessionState { expected: String, actual: String },
    /// Aggregation failed
    AggregationFailed { reason: String },
    /// Verification failed
    VerificationFailed { reason: String },
    /// Serialization error
    SerializationError { reason: String },
    /// Key generation failed
    KeyGenerationFailed { reason: String },
    /// Timeout
    Timeout,
}

(impl std::fmt::Display for FrostSigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientSigners {
                required,
                available,
            } => {
                write!(
                    f,
                    "Insufficient signers: need {}, have {}",
                    required, available
                )
            }
            Self::InvalidSignatureShare { relay_id, reason } => {
                write!(
                    f,
                    "Invalid signature share from {}: {}",
                    hex::encode(&relay_id[..8]),
                    reason
                )
            }
            Self::KeyShareNotFound { relay_id } => {
                write!(
                    f,
                    "Key share not found for relay {}",
                    hex::encode(&relay_id[..8])
                )
            }
            Self::SessionNotFound { session_id } => {
                write!(f, "Session not found: {}", session_id)
            }
            Self::InvalidSessionState { expected, actual } => {
                write!(
                    f,
                    "Invalid session state: expected {}, got {}",
                    expected, actual
                )
            }
            Self::AggregationFailed { reason } => {
                write!(f, "Signature aggregation failed: {}", reason)
            }
            Self::VerificationFailed { reason } => {
                write!(f, "Verification failed: {}", reason)
            }
            Self::SerializationError { reason } => {
                write!(f, "Serialization error: {}", reason)
            }
            Self::KeyGenerationFailed { reason } => {
                write!(f, "Key generation failed: {}", reason)
            }
            Self::Timeout => write!(f, "Operation timed out"),
        }
    }
}

impl std::error::Error for FrostSigningError {}

/// Configuration for committee FROST signing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitteeFrostConfig {
    /// Threshold (minimum signers required)
    pub threshold: u8,
    /// Total committee size
    pub committee_size: u8,
    /// Timeout for signature collection (seconds)
    pub timeout_secs: u64,
    /// Maximum concurrent signing sessions
    pub max_sessions: usize,
}

impl CommitteeFrostConfig {
    /// Create config for 1:1 conversations (4-of-7)
    pub fn for_direct() -> Self {
        Self {
            threshold: 4,
            committee_size: 7,
            timeout_secs: 30,
            max_sessions: 100,
        }
    }

    /// Create config for small channels (7-of-11)
    pub fn for_channel_small() -> Self {
        Self {
            threshold: 7,
            committee_size: 11,
            timeout_secs: 30,
            max_sessions: 100,
        }
    }

    /// Create config for large channels (11-of-15)
    pub fn for_channel_large() -> Self {
        Self {
            threshold: 11,
            committee_size: 15,
            timeout_secs: 30,
            max_sessions: 100,
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.threshold == 0 {
            return Err(Error::validation("Threshold must be at least 1"));
        }
        if self.threshold > self.committee_size {
            return Err(Error::validation(format!(
                "Threshold ({}) cannot exceed committee size ({})",
                self.threshold, self.committee_size
            )));
        }
        if self.committee_size > 255 {
            return Err(Error::validation("Committee size cannot exceed 255"));
        }
        Ok(())
    }
}

/// Relay's FROST key material for a specific committee
#[derive(Clone)]
pub struct RelayFrostKeyShare {
    /// Relay's position in the committee (1-indexed per FROST spec)
    pub participant_index: u16,
    /// Relay's identifier
    pub relay_id: [u8; 32],
    /// Serialized signing share (secret - keep protected!)
    signing_share: Vec<u8>,
    /// Serialized verifying share (public)
    pub verifying_share: Vec<u8>,
    /// Committee's group public key
    pub group_public_key: [u8; 32],
    /// All participants' verifying shares (for aggregation)
    pub all_verifying_shares: BTreeMap<u16, Vec<u8>>,
    /// Threshold for this committee
    pub threshold: u8,
}

impl RelayFrostKeyShare {
    /// Get the signing share bytes (for internal FROST operations)
    pub fn signing_share_bytes(&self) -> &[u8] {
        &self.signing_share
    }

    /// Serialize for secure storage
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| Error::crypto(format!("Failed to serialize key share: {}", e)))
    }

    /// Deserialize from secure storage
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data)
            .map_err(|e| Error::crypto(format!("Failed to deserialize key share: {}", e)))
    }
}

impl Serialize for RelayFrostKeyShare {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("RelayFrostKeyShare", 7)?;
        state.serialize_field("participant_index", &self.participant_index)?;
        state.serialize_field("relay_id", &self.relay_id)?;
        state.serialize_field("signing_share", &self.signing_share)?;
        state.serialize_field("verifying_share", &self.verifying_share)?;
        state.serialize_field("group_public_key", &self.group_public_key)?;
        state.serialize_field("all_verifying_shares", &self.all_verifying_shares)?;
        state.serialize_field("threshold", &self.threshold)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for RelayFrostKeyShare {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            participant_index: u16,
            relay_id: [u8; 32],
            signing_share: Vec<u8>,
            verifying_share: Vec<u8>,
            group_public_key: [u8; 32],
            all_verifying_shares: BTreeMap<u16, Vec<u8>>,
            threshold: u8,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(Self {
            participant_index: helper.participant_index,
            relay_id: helper.relay_id,
            signing_share: helper.signing_share,
            verifying_share: helper.verifying_share,
            group_public_key: helper.group_public_key,
            all_verifying_shares: helper.all_verifying_shares,
            threshold: helper.threshold,
        })
    }
}

/// Round 1 output from a relay (commitment)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostRound1Output {
    /// Relay ID
    pub relay_id: [u8; 32],
    /// Participant index in FROST protocol
    pub participant_index: u16,
    /// Serialized signing commitments
    pub commitments: Vec<u8>,
    /// Session this belongs to
    pub session_id: String,
}

/// Round 1 state that relay must keep (nonces - secret!)
pub struct FrostRound1State {
    /// Session ID
    pub session_id: String,
    /// Signing nonces (keep secret until round 2)
    pub nonces: frost::round1::SigningNonces,
    /// Message being signed
    pub message: Vec<u8>,
    /// Created at timestamp
    pub created_at: std::time::Instant,
}

/// Round 2 output from a relay (signature share)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostRound2Output {
    /// Relay ID
    pub relay_id: [u8; 32],
    /// Participant index in FROST protocol
    pub participant_index: u16,
    /// Serialized signature share
    pub signature_share: Vec<u8>,
    /// Session this belongs to
    pub session_id: String,
}

/// Aggregated FROST signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedFrostSignature {
    /// Final signature bytes (64 bytes for Ed25519)
    #[serde(with = "signature_bytes")]
    pub signature: [u8; 64],
    /// Group public key that can verify this signature
    pub group_public_key: [u8; 32],
    /// Participant indices that contributed
    pub contributors: Vec<u16>,
    /// Session ID
    pub session_id: String,
}

/// Relay-side FROST signer for epoch tokens
pub struct RelayFrostSigner {
    /// This relay's ID
    relay_id: [u8; 32],
    /// Key shares for different committees (keyed by committee_id)
    key_shares: Arc<RwLock<BTreeMap<[u8; 32], RelayFrostKeyShare>>>,
    /// Active round 1 states (keyed by session_id)
    round1_states: Arc<RwLock<BTreeMap<String, FrostRound1State>>>,
    /// Maximum sessions to track
    max_sessions: usize,
}

impl RelayFrostSigner {
    /// Create a new relay FROST signer
    pub fn new(relay_id: [u8; 32]) -> Self {
        Self {
            relay_id,
            key_shares: Arc::new(RwLock::new(BTreeMap::new())),
            round1_states: Arc::new(RwLock::new(BTreeMap::new())),
            max_sessions: 1000,
        }
    }

    /// Register a key share for a committee
    pub async fn register_key_share(
        &self,
        committee_id: [u8; 32],
        key_share: RelayFrostKeyShare,
    ) -> Result<()> {
        let mut shares = self.key_shares.write().await;
        shares.insert(committee_id, key_share);
        Ok(())
    }

    /// Get key share for a committee
    pub async fn get_key_share(&self, committee_id: &[u8; 32]) -> Option<RelayFrostKeyShare> {
        let shares = self.key_shares.read().await;
        shares.get(committee_id).cloned()
    }

    /// Generate Round 1 commitment for signing
    pub async fn generate_round1(
        &self,
        committee_id: &[u8; 32],
        session_id: String,
        message: Vec<u8>,
    ) -> Result<FrostRound1Output> {
        use rand::rngs::OsRng;

        // Get key share
        let key_share = self.get_key_share(committee_id).await.ok_or_else(|| {
            Error::crypto(format!(
                "No key share for committee {}",
                hex::encode(committee_id)
            ))
        })?;

        // Reconstruct signing share
        let signing_share = frost::keys::SigningShare::deserialize(&key_share.signing_share)
            .map_err(|e| Error::crypto(format!("Failed to deserialize signing share: {:?}", e)))?;

        // Generate nonces and commitments
        let mut rng = OsRng;
        let (nonces, commitments) = frost::round1::commit(&signing_share, &mut rng);

        // Serialize commitments
        let commitments_bytes = commitments
            .serialize()
            .map_err(|e| Error::crypto(format!("Failed to serialize commitments: {:?}", e)))?;

        // Store round 1 state
        let state = FrostRound1State {
            session_id: session_id.clone(),
            nonces,
            message,
            created_at: std::time::Instant::now(),
        };

        {
            let mut states = self.round1_states.write().await;

            // Clean up old sessions
            if states.len() >= self.max_sessions {
                let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(300);
                states.retain(|_, s| s.created_at > cutoff);
            }

            states.insert(session_id.clone(), state);
        }

        Ok(FrostRound1Output {
            relay_id: self.relay_id,
            participant_index: key_share.participant_index,
            commitments: commitments_bytes,
            session_id,
        })
    }

    /// Generate Round 2 signature share
    pub async fn generate_round2(
        &self,
        committee_id: &[u8; 32],
        session_id: &str,
        all_commitments: &BTreeMap<u16, Vec<u8>>,
    ) -> Result<FrostRound2Output> {
        // Get key share
        let key_share = self.get_key_share(committee_id).await.ok_or_else(|| {
            Error::crypto(format!(
                "No key share for committee {}",
                hex::encode(committee_id)
            ))
        })?;

        // We need to extract the nonces before borrowing mutably
        let (nonces, message) = {
            let mut states = self.round1_states.write().await;
            let state = states.remove(session_id).ok_or_else(|| {
                Error::crypto(format!("No round 1 state for session {}", session_id))
            })?;
            (state.nonces, state.message)
        };

        // Build key package
        let identifier = frost::Identifier::try_from(key_share.participant_index)
            .map_err(|e| Error::crypto(format!("Invalid identifier: {:?}", e)))?;

        let signing_share = frost::keys::SigningShare::deserialize(&key_share.signing_share)
            .map_err(|e| Error::crypto(format!("Failed to deserialize signing share: {:?}", e)))?;

        let verifying_share = frost::keys::VerifyingShare::deserialize(&key_share.verifying_share)
            .map_err(|e| {
                Error::crypto(format!("Failed to deserialize verifying share: {:?}", e))
            })?;

        let verifying_key = frost::VerifyingKey::deserialize(&key_share.group_public_key)
            .map_err(|e| Error::crypto(format!("Failed to deserialize verifying key: {:?}", e)))?;

        let key_package = frost::keys::KeyPackage::new(
            identifier,
            signing_share,
            verifying_share,
            verifying_key,
            key_share.threshold as u16,
        );

        // Deserialize all commitments
        let mut commitments_map: BTreeMap<frost::Identifier, frost::round1::SigningCommitments> =
            BTreeMap::new();

        for (&participant_index, commitment_bytes) in all_commitments {
            let id = frost::Identifier::try_from(participant_index)
                .map_err(|e| Error::crypto(format!("Invalid identifier: {:?}", e)))?;

            let commitments = frost::round1::SigningCommitments::deserialize(commitment_bytes)
                .map_err(|e| {
                    Error::crypto(format!("Failed to deserialize commitments: {:?}", e))
                })?;

            commitments_map.insert(id, commitments);
        }

        // Create signing package
        let signing_package = frost::SigningPackage::new(commitments_map, &message);

        // Generate signature share
        let signature_share = frost::round2::sign(&signing_package, &nonces, &key_package)
            .map_err(|e| Error::crypto(format!("Round 2 signing failed: {:?}", e)))?;

        // Serialize signature share
        let share_bytes = signature_share.serialize();

        Ok(FrostRound2Output {
            relay_id: self.relay_id,
            participant_index: key_share.participant_index,
            signature_share: share_bytes,
            session_id: session_id.to_string(),
        })
    }

    /// Clean up old sessions
    pub async fn cleanup_old_sessions(&self) {
        let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(300);
        let mut states = self.round1_states.write().await;
        states.retain(|_, s| s.created_at > cutoff);
    }
}

/// Client-side FROST signature aggregator
pub struct FrostSignatureAggregator {
    /// Configuration
    config: CommitteeFrostConfig,
    /// All verifying shares for the committee
    verifying_shares: BTreeMap<u16, Vec<u8>>,
    /// Group public key
    group_public_key: [u8; 32],
}

impl FrostSignatureAggregator {
    /// Create a new aggregator for a committee
    pub fn new(
        config: CommitteeFrostConfig,
        verifying_shares: BTreeMap<u16, Vec<u8>>,
        group_public_key: [u8; 32],
    ) -> Self {
        Self {
            config,
            verifying_shares,
            group_public_key,
        }
    }

    /// Aggregate signature shares into a final signature
    pub fn aggregate(
        &self,
        message: &[u8],
        commitments: &BTreeMap<u16, Vec<u8>>,
        signature_shares: &BTreeMap<u16, Vec<u8>>,
        session_id: &str,
    ) -> std::result::Result<AggregatedFrostSignature, FrostSigningError> {
        // Check threshold
        if signature_shares.len() < self.config.threshold as usize {
            return Err(FrostSigningError::InsufficientSigners {
                required: self.config.threshold,
                available: signature_shares.len() as u8,
            });
        }

        // Deserialize commitments
        let mut commitments_map: BTreeMap<frost::Identifier, frost::round1::SigningCommitments> =
            BTreeMap::new();

        for (&participant_index, commitment_bytes) in commitments {
            let id = frost::Identifier::try_from(participant_index).map_err(|e| {
                FrostSigningError::SerializationError {
                    reason: format!("Invalid identifier: {:?}", e),
                }
            })?;

            let commitment = frost::round1::SigningCommitments::deserialize(commitment_bytes)
                .map_err(|e| FrostSigningError::SerializationError {
                    reason: format!("Failed to deserialize commitment: {:?}", e),
                })?;

            commitments_map.insert(id, commitment);
        }

        // Create signing package
        let signing_package = frost::SigningPackage::new(commitments_map, message);

        // Deserialize signature shares
        let mut shares_map: BTreeMap<frost::Identifier, frost::round2::SignatureShare> =
            BTreeMap::new();

        for (&participant_index, share_bytes) in signature_shares {
            let id = frost::Identifier::try_from(participant_index).map_err(|e| {
                FrostSigningError::SerializationError {
                    reason: format!("Invalid identifier: {:?}", e),
                }
            })?;

            let share = frost::round2::SignatureShare::deserialize(share_bytes).map_err(|e| {
                FrostSigningError::InvalidSignatureShare {
                    relay_id: [0u8; 32], // We don't have relay_id here
                    reason: format!("Failed to deserialize: {:?}", e),
                }
            })?;

            shares_map.insert(id, share);
        }

        // Build verifying shares for public key package
        let mut verifying_shares_map: BTreeMap<frost::Identifier, frost::keys::VerifyingShare> =
            BTreeMap::new();

        for (&participant_index, share_bytes) in &self.verifying_shares {
            let id = frost::Identifier::try_from(participant_index).map_err(|e| {
                FrostSigningError::SerializationError {
                    reason: format!("Invalid identifier: {:?}", e),
                }
            })?;

            let verifying_share =
                frost::keys::VerifyingShare::deserialize(share_bytes).map_err(|e| {
                    FrostSigningError::SerializationError {
                        reason: format!("Failed to deserialize verifying share: {:?}", e),
                    }
                })?;

            verifying_shares_map.insert(id, verifying_share);
        }

        // Get verifying key
        let verifying_key =
            frost::VerifyingKey::deserialize(&self.group_public_key).map_err(|e| {
                FrostSigningError::SerializationError {
                    reason: format!("Failed to deserialize verifying key: {:?}", e),
                }
            })?;

        // Create public key package
        let pubkey_package =
            frost::keys::PublicKeyPackage::new(verifying_shares_map, verifying_key);

        // Aggregate
        let signature =
            frost::aggregate(&signing_package, &shares_map, &pubkey_package).map_err(|e| {
                FrostSigningError::AggregationFailed {
                    reason: format!("{:?}", e),
                }
            })?;

        // Serialize signature
        let signature_bytes =
            signature
                .serialize()
                .map_err(|e| FrostSigningError::SerializationError {
                    reason: format!("Failed to serialize signature: {:?}", e),
                })?;

        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&signature_bytes);

        let contributors: Vec<u16> = signature_shares.keys().copied().collect();

        Ok(AggregatedFrostSignature {
            signature: sig_array,
            group_public_key: self.group_public_key,
            contributors,
            session_id: session_id.to_string(),
        })
    }
}

/// Verify a FROST signature
pub fn verify_frost_signature(
    signature: &[u8; 64],
    message: &[u8],
    group_public_key: &[u8; 32],
) -> std::result::Result<bool, FrostSigningError> {
    let sig = frost::Signature::deserialize(signature).map_err(|e| {
        FrostSigningError::VerificationFailed {
            reason: format!("Invalid signature format: {:?}", e),
        }
    })?;

    let vk = frost::VerifyingKey::deserialize(group_public_key).map_err(|e| {
        FrostSigningError::VerificationFailed {
            reason: format!("Invalid public key format: {:?}", e),
        }
    })?;

    Ok(vk.verify(message, &sig).is_ok())
}

/// Committee registration information stored on-chain
///
/// When a relay committee is formed (e.g., for a new conversation),
/// the committee's group public key and member list are registered
/// on-chain for verification purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitteeRegistration {
    /// Unique identifier for the committee (typically derived from conversation/channel ID)
    pub committee_id: [u8; 32],
    /// The FROST group public key for this committee
    pub group_public_key: [u8; 32],
    /// IDs of all relay members in the committee
    pub member_relay_ids: Vec<[u8; 32]>,
    /// Threshold required for valid signatures
    pub threshold: u8,
    /// Block height when this committee was registered
    pub registered_at_block: u64,
    /// Block height when this committee expires (0 = never)
    pub expires_at_block: u64,
    /// Whether this committee is currently active
    pub is_active: bool,
    /// Transaction hash of the registration transaction
    pub registration_tx_hash: [u8; 32],
}

impl CommitteeRegistration {
    /// Create a new committee registration
    pub fn new(
        committee_id: [u8; 32],
        group_public_key: [u8; 32],
        member_relay_ids: Vec<[u8; 32]>,
        threshold: u8,
        registered_at_block: u64,
        registration_tx_hash: [u8; 32],
    ) -> Self {
        Self {
            committee_id,
            group_public_key,
            member_relay_ids,
            threshold,
            registered_at_block,
            expires_at_block: 0,
            is_active: true,
            registration_tx_hash,
        }
    }

    /// Compute the committee ID from relay member IDs (deterministic)
    ///
    /// This creates a canonical committee ID from the sorted set of relay IDs.
    /// Used when forming a new committee to derive its ID.
    pub fn compute_committee_id(relay_ids: &[[u8; 32]]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"dchat-committee-id-v1");

        // Sort relay IDs for deterministic ordering
        let mut sorted_ids = relay_ids.to_vec();
        sorted_ids.sort();

        for id in sorted_ids {
            hasher.update(id);
        }
        hasher.finalize().into()
    }

    /// Verify that a set of relay IDs matches this committee
    pub fn verify_membership(&self, relay_ids: &[[u8; 32]]) -> bool {
        if relay_ids.len() < self.threshold as usize {
            return false;
        }
        // Check all provided relay IDs are members
        relay_ids
            .iter()
            .all(|id| self.member_relay_ids.contains(id))
    }

    /// Serialize for on-chain storage
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| {
            Error::crypto(format!("Failed to serialize committee registration: {}", e))
        })
    }

    /// Deserialize from on-chain storage
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| {
            Error::crypto(format!(
                "Failed to deserialize committee registration: {}",
                e
            ))
        })
    }
}

/// Committee registry for tracking FROST committees
///
/// This registry provides the authoritative source for committee group public keys.
/// In production, this is synchronized with on-chain committee registrations.
///
/// # On-Chain Integration
///
/// Committees register their group public keys on-chain during DKG completion.
/// The registry caches these registrations locally and validates them against
/// the blockchain when needed.
///
/// # Security Properties
///
/// - Committee public keys are verified against on-chain registrations
/// - Expired or revoked committees are automatically invalidated
/// - Cache is periodically synchronized with blockchain state
pub struct CommitteeRegistry {
    /// Local cache of committee registrations
    committees: Arc<RwLock<BTreeMap<[u8; 32], CommitteeRegistration>>>,
    /// Relay ID to committee mappings (which committees a relay participates in)
    relay_committees: Arc<RwLock<BTreeMap<[u8; 32], Vec<[u8; 32]>>>>,
    /// Last blockchain sync height
    last_sync_block: Arc<RwLock<u64>>,
    /// Blockchain RPC endpoint for verification
    blockchain_rpc_url: Option<String>,
}

impl CommitteeRegistry {
    /// Create a new committee registry
    pub fn new() -> Self {
        Self {
            committees: Arc::new(RwLock::new(BTreeMap::new())),
            relay_committees: Arc::new(RwLock::new(BTreeMap::new())),
            last_sync_block: Arc::new(RwLock::new(0)),
            blockchain_rpc_url: None,
        }
    }

    /// Create a registry with blockchain RPC for on-chain verification
    pub fn with_blockchain_rpc(rpc_url: String) -> Self {
        Self {
            committees: Arc::new(RwLock::new(BTreeMap::new())),
            relay_committees: Arc::new(RwLock::new(BTreeMap::new())),
            last_sync_block: Arc::new(RwLock::new(0)),
            blockchain_rpc_url: Some(rpc_url),
        }
    }

    /// Register a new committee (called after DKG completion)
    ///
    /// In production, this should be called after the committee registration
    /// transaction has been confirmed on-chain.
    pub async fn register_committee(&self, registration: CommitteeRegistration) -> Result<()> {
        // Validate the registration
        if registration.member_relay_ids.len() < registration.threshold as usize {
            return Err(Error::validation(format!(
                "Committee size ({}) is less than threshold ({})",
                registration.member_relay_ids.len(),
                registration.threshold
            )));
        }

        let committee_id = registration.committee_id;
        let relay_ids = registration.member_relay_ids.clone();
        let relay_count = relay_ids.len();

        // Store in committee map
        {
            let mut committees = self.committees.write().await;
            committees.insert(committee_id, registration);
        }

        // Update relay -> committee mappings
        {
            let mut relay_committees = self.relay_committees.write().await;
            for relay_id in relay_ids {
                relay_committees
                    .entry(relay_id)
                    .or_insert_with(Vec::new)
                    .push(committee_id);
            }
        }

        tracing::info!(
            "✅ Registered committee {} with {} members",
            hex::encode(&committee_id[..8]),
            relay_count
        );

        Ok(())
    }

    /// Get a committee's group public key
    ///
    /// This is the primary method for looking up committee public keys
    /// for signature verification.
    pub async fn get_group_public_key(&self, committee_id: &[u8; 32]) -> Option<[u8; 32]> {
        let committees = self.committees.read().await;
        committees
            .get(committee_id)
            .filter(|c| c.is_active)
            .map(|c| c.group_public_key)
    }

    /// Get full committee registration
    pub async fn get_committee(&self, committee_id: &[u8; 32]) -> Option<CommitteeRegistration> {
        let committees = self.committees.read().await;
        committees.get(committee_id).cloned()
    }

    /// Look up committee by relay member IDs
    ///
    /// When verifying a FROST signature, we may only have the relay IDs that
    /// participated. This method finds the matching committee.
    pub async fn find_committee_by_relays(
        &self,
        relay_ids: &[[u8; 32]],
    ) -> Option<CommitteeRegistration> {
        // Compute the canonical committee ID from the relay set
        let expected_committee_id = CommitteeRegistration::compute_committee_id(relay_ids);

        let committees = self.committees.read().await;

        // First try exact match by computed committee ID
        if let Some(registration) = committees.get(&expected_committee_id) {
            if registration.is_active && registration.verify_membership(relay_ids) {
                return Some(registration.clone());
            }
        }

        // Fallback: search for any committee where all provided relays are members
        for registration in committees.values() {
            if registration.is_active && registration.verify_membership(relay_ids) {
                return Some(registration.clone());
            }
        }

        None
    }

    /// Get all committees a relay participates in
    pub async fn get_relay_committees(&self, relay_id: &[u8; 32]) -> Vec<[u8; 32]> {
        let relay_committees = self.relay_committees.read().await;
        relay_committees.get(relay_id).cloned().unwrap_or_default()
    }

    /// Deactivate a committee (e.g., when expired or superseded)
    pub async fn deactivate_committee(&self, committee_id: &[u8; 32]) -> Result<()> {
        let mut committees = self.committees.write().await;
        if let Some(committee) = committees.get_mut(committee_id) {
            committee.is_active = false;
            tracing::info!("Committee {} deactivated", hex::encode(&committee_id[..8]));
            Ok(())
        } else {
            Err(Error::validation(format!(
                "Committee {} not found",
                hex::encode(committee_id)
            )))
        }
    }

    /// Synchronize with blockchain state
    ///
    /// This method should be called periodically to ensure the local cache
    /// is up-to-date with on-chain committee registrations.
    pub async fn sync_with_blockchain(&self) -> Result<u64> {
        let rpc_url = match &self.blockchain_rpc_url {
            Some(url) => url.clone(),
            None => {
                tracing::debug!("No blockchain RPC configured, skipping sync");
                return Ok(0);
            }
        };

        let last_sync = *self.last_sync_block.read().await;

        // Query blockchain for committee registration events since last sync
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "chain_getCommitteeRegistrations",
            "params": {
                "from_block": last_sync,
                "to_block": null
            },
            "id": 1
        });

        let response = client
            .post(&rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "RPC returned error: {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;

        // Process registrations from response
        let mut new_registrations = 0u64;
        let mut max_block = last_sync;

        if let Some(registrations) = json["result"]["registrations"].as_array() {
            for reg_json in registrations {
                // Parse registration from JSON
                if let Ok(registration) =
                    serde_json::from_value::<CommitteeRegistration>(reg_json.clone())
                {
                    if registration.registered_at_block > max_block {
                        max_block = registration.registered_at_block;
                    }
                    if let Err(e) = self.register_committee(registration).await {
                        tracing::warn!("Failed to register committee from blockchain: {}", e);
                    } else {
                        new_registrations += 1;
                    }
                }
            }
        }

        // Update last sync block
        *self.last_sync_block.write().await = max_block;

        tracing::info!(
            "✅ Blockchain sync complete: {} new registrations, synced to block {}",
            new_registrations,
            max_block
        );

        Ok(new_registrations)
    }

    /// Verify a FROST signature using the registry
    ///
    /// This is the production method for verifying FROST signatures from
    /// relay quorums. It looks up the committee's group public key from
    /// the registry and verifies the signature.
    pub async fn verify_frost_signature_with_registry(
        &self,
        relay_ids: &[[u8; 32]],
        signature: &[u8; 64],
        message: &[u8],
        threshold: u8,
    ) -> Result<()> {
        // Verify we have enough relays
        if relay_ids.len() < threshold as usize {
            return Err(Error::crypto(format!(
                "Insufficient relay signatures: {} < {}",
                relay_ids.len(),
                threshold
            )));
        }

        // Look up the committee registration
        let committee = self
            .find_committee_by_relays(relay_ids)
            .await
            .ok_or_else(|| {
                Error::crypto(format!(
                    "No registered committee found for relay set (first relay: {})",
                    hex::encode(&relay_ids[0][..8])
                ))
            })?;

        // Verify the signature threshold matches
        if threshold < committee.threshold {
            return Err(Error::crypto(format!(
                "Signature threshold {} is below committee minimum {}",
                threshold, committee.threshold
            )));
        }

        // Verify the FROST signature
        match verify_frost_signature(signature, message, &committee.group_public_key) {
            Ok(true) => {
                tracing::debug!(
                    "✅ FROST signature verified for committee {}",
                    hex::encode(&committee.committee_id[..8])
                );
                Ok(())
            }
            Ok(false) => Err(Error::crypto("FROST signature verification failed")),
            Err(e) => Err(Error::crypto(format!("FROST verification error: {}", e))),
        }
    }

    /// Get statistics about the registry
    pub async fn stats(&self) -> CommitteeRegistryStats {
        let committees = self.committees.read().await;
        let active = committees.values().filter(|c| c.is_active).count();
        let total = committees.len();
        let last_sync = *self.last_sync_block.read().await;

        CommitteeRegistryStats {
            total_committees: total,
            active_committees: active,
            inactive_committees: total - active,
            last_sync_block: last_sync,
        }
    }
}

impl Default for CommitteeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the committee registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitteeRegistryStats {
    /// Total number of committees in registry
    pub total_committees: usize,
    /// Number of active committees
    pub active_committees: usize,
    /// Number of inactive/expired committees
    pub inactive_committees: usize,
    /// Last blockchain sync block height
    pub last_sync_block: u64,
}

/// Global committee registry singleton
///
/// This provides a shared registry instance for the entire relay node.
/// In production, this is initialized at startup and synchronized with
/// blockchain state periodically.
static GLOBAL_REGISTRY: std::sync::OnceLock<CommitteeRegistry> = std::sync::OnceLock::new();

/// Initialize the global committee registry
pub fn init_global_registry(rpc_url: Option<String>) -> &'static CommitteeRegistry {
    GLOBAL_REGISTRY.get_or_init(|| match rpc_url {
        Some(url) => CommitteeRegistry::with_blockchain_rpc(url),
        None => CommitteeRegistry::new(),
    })
}

/// Get the global committee registry
pub fn global_registry() -> Option<&'static CommitteeRegistry> {
    GLOBAL_REGISTRY.get()
}

/// Generate FROST key shares for a new committee using trusted dealer
///
/// This should be called during committee formation. For fully distributed
/// key generation without a dealer, use the DKG protocol.
pub fn generate_committee_keys(
    relay_ids: &[[u8; 32]],
    threshold: u8,
) -> std::result::Result<(Vec<RelayFrostKeyShare>, [u8; 32]), FrostSigningError> {
    use rand::rngs::OsRng;

    let committee_size = relay_ids.len();
    if committee_size > 255 {
        return Err(FrostSigningError::KeyGenerationFailed {
            reason: "Committee size cannot exceed 255".to_string(),
        });
    }
    if threshold as usize > committee_size {
        return Err(FrostSigningError::KeyGenerationFailed {
            reason: format!(
                "Threshold ({}) cannot exceed committee size ({})",
                threshold, committee_size
            ),
        });
    }

    let mut rng = OsRng;

    // Generate keys using trusted dealer
    let (shares, pubkey_package) = frost::keys::generate_with_dealer(
        committee_size as u16,
        threshold as u16,
        frost::keys::IdentifierList::Default,
        &mut rng,
    )
    .map_err(|e| FrostSigningError::KeyGenerationFailed {
        reason: format!("FROST key generation failed: {:?}", e),
    })?;

    // Get group public key
    let group_public_key_bytes = pubkey_package.verifying_key().serialize().map_err(|e| {
        FrostSigningError::SerializationError {
            reason: format!("Failed to serialize group public key: {:?}", e),
        }
    })?;

    let mut group_public_key = [0u8; 32];
    group_public_key.copy_from_slice(&group_public_key_bytes);

    // Build verifying shares map
    let mut all_verifying_shares: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
    for (id, _) in &shares {
        let participant_index = id.serialize().first().copied().ok_or_else(|| {
            FrostSigningError::SerializationError {
                reason: "Empty identifier".to_string(),
            }
        })? as u16;

        let verifying_share = pubkey_package.verifying_shares().get(id).ok_or_else(|| {
            FrostSigningError::KeyGenerationFailed {
                reason: "Missing verifying share".to_string(),
            }
        })?;

        let serialized =
            verifying_share
                .serialize()
                .map_err(|e| FrostSigningError::SerializationError {
                    reason: format!("Failed to serialize verifying share: {:?}", e),
                })?;

        all_verifying_shares.insert(participant_index, serialized);
    }

    // Create key shares for each relay
    let mut key_shares = Vec::with_capacity(committee_size);

    for (idx, (frost_id, secret_share)) in shares.into_iter().enumerate() {
        let participant_index = frost_id.serialize().first().copied().ok_or_else(|| {
            FrostSigningError::SerializationError {
                reason: "Empty identifier".to_string(),
            }
        })? as u16;

        // Convert to key package to get signing share
        let key_package = frost::keys::KeyPackage::try_from(secret_share).map_err(|e| {
            FrostSigningError::KeyGenerationFailed {
                reason: format!("Failed to create key package: {:?}", e),
            }
        })?;

        let signing_share_bytes = key_package.signing_share().serialize();

        let verifying_share_bytes = all_verifying_shares
            .get(&participant_index)
            .cloned()
            .ok_or_else(|| FrostSigningError::KeyGenerationFailed {
                reason: "Missing verifying share in map".to_string(),
            })?;

        key_shares.push(RelayFrostKeyShare {
            participant_index,
            relay_id: relay_ids[idx],
            signing_share: signing_share_bytes,
            verifying_share: verifying_share_bytes,
            group_public_key,
            all_verifying_shares: all_verifying_shares.clone(),
            threshold,
        });
    }

    tracing::info!(
        "✅ Generated FROST keys for committee: {} relays, threshold {}",
        committee_size,
        threshold
    );

    Ok((key_shares, group_public_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let valid = CommitteeFrostConfig::for_direct();
        assert!(valid.validate().is_ok());

        let invalid_threshold = CommitteeFrostConfig {
            threshold: 0,
            committee_size: 7,
            timeout_secs: 30,
            max_sessions: 100,
        };
        assert!(invalid_threshold.validate().is_err());

        let invalid_size = CommitteeFrostConfig {
            threshold: 5,
            committee_size: 3,
            timeout_secs: 30,
            max_sessions: 100,
        };
        assert!(invalid_size.validate().is_err());
    }

    #[test]
    fn test_generate_committee_keys() {
        let relay_ids: Vec<[u8; 32]> = (0..7).map(|i| [i as u8; 32]).collect();

        let result = generate_committee_keys(&relay_ids, 4);
        assert!(result.is_ok());

        let (key_shares, group_pk) = result.unwrap();
        assert_eq!(key_shares.len(), 7);

        // All key shares should have the same group public key
        for share in &key_shares {
            assert_eq!(share.group_public_key, group_pk);
            assert_eq!(share.threshold, 4);
        }

        // Participant indices should be unique
        let indices: std::collections::HashSet<_> =
            key_shares.iter().map(|s| s.participant_index).collect();
        assert_eq!(indices.len(), 7);
    }

    #[tokio::test]
    async fn test_relay_frost_signer() {
        let relay_id = [1u8; 32];
        let signer = RelayFrostSigner::new(relay_id);

        // Generate keys
        let relay_ids: Vec<[u8; 32]> = (0..7).map(|i| [i as u8 + 1; 32]).collect();
        let (key_shares, _) = generate_committee_keys(&relay_ids, 4).unwrap();

        // Find our key share
        let our_share = key_shares
            .iter()
            .find(|s| s.relay_id == relay_id)
            .unwrap()
            .clone();

        // Register key share
        let committee_id = [42u8; 32];
        signer
            .register_key_share(committee_id, our_share)
            .await
            .unwrap();

        // Generate round 1
        let session_id = "test-session".to_string();
        let message = b"test message".to_vec();

        let round1 = signer
            .generate_round1(&committee_id, session_id.clone(), message)
            .await
            .unwrap();

        assert_eq!(round1.relay_id, relay_id);
        assert_eq!(round1.session_id, session_id);
        assert!(!round1.commitments.is_empty());
    }

    #[tokio::test]
    async fn test_full_signing_flow() {
        // Generate keys for 7 relays
        let relay_ids: Vec<[u8; 32]> = (0..7).map(|i| [i as u8 + 1; 32]).collect();
        let (key_shares, group_pk) = generate_committee_keys(&relay_ids, 4).unwrap();

        let committee_id = [42u8; 32];
        let message = b"epoch token data".to_vec();
        let session_id = "signing-session-1".to_string();

        // Create signers for each relay
        let mut signers: Vec<RelayFrostSigner> = Vec::new();
        for share in &key_shares {
            let signer = RelayFrostSigner::new(share.relay_id);
            signer
                .register_key_share(committee_id, share.clone())
                .await
                .unwrap();
            signers.push(signer);
        }

        // Round 1: Collect commitments from threshold signers (first 4)
        let mut round1_outputs = Vec::new();
        for signer in signers.iter().take(4) {
            let output = signer
                .generate_round1(&committee_id, session_id.clone(), message.clone())
                .await
                .unwrap();
            round1_outputs.push(output);
        }

        // Build commitments map
        let commitments: BTreeMap<u16, Vec<u8>> = round1_outputs
            .iter()
            .map(|o| (o.participant_index, o.commitments.clone()))
            .collect();

        // Round 2: Collect signature shares
        let mut round2_outputs = Vec::new();
        for (i, signer) in signers.iter().take(4).enumerate() {
            // Get the signer's participant index
            let participant_idx = round1_outputs[i].participant_index;

            let output = signer
                .generate_round2(&committee_id, &session_id, &commitments)
                .await
                .unwrap();
            assert_eq!(output.participant_index, participant_idx);
            round2_outputs.push(output);
        }

        // Build signature shares map
        let signature_shares: BTreeMap<u16, Vec<u8>> = round2_outputs
            .iter()
            .map(|o| (o.participant_index, o.signature_share.clone()))
            .collect();

        // Aggregate
        let verifying_shares = key_shares[0].all_verifying_shares.clone();
        let aggregator = FrostSignatureAggregator::new(
            CommitteeFrostConfig::for_direct(),
            verifying_shares,
            group_pk,
        );

        let aggregated = aggregator
            .aggregate(&message, &commitments, &signature_shares, &session_id)
            .unwrap();

        assert_eq!(aggregated.contributors.len(), 4);
        assert_eq!(aggregated.group_public_key, group_pk);

        // Verify
        let is_valid = verify_frost_signature(&aggregated.signature, &message, &group_pk).unwrap();
        assert!(is_valid);

        // Verify with wrong message should fail
        let is_invalid =
            verify_frost_signature(&aggregated.signature, b"wrong message", &group_pk).unwrap();
        assert!(!is_invalid);
    }

    #[test]
    fn test_insufficient_signers() {
        let relay_ids: Vec<[u8; 32]> = (0..7).map(|i| [i as u8 + 1; 32]).collect();
        let (key_shares, group_pk) = generate_committee_keys(&relay_ids, 4).unwrap();

        let verifying_shares = key_shares[0].all_verifying_shares.clone();
        let aggregator = FrostSignatureAggregator::new(
            CommitteeFrostConfig::for_direct(),
            verifying_shares,
            group_pk,
        );

        // Only 3 shares (below threshold of 4)
        let commitments: BTreeMap<u16, Vec<u8>> = BTreeMap::new();
        let signature_shares: BTreeMap<u16, Vec<u8>> =
            (1..=3).map(|i| (i as u16, vec![0u8; 32])).collect();

        let result = aggregator.aggregate(b"test", &commitments, &signature_shares, "session");

        assert!(matches!(
            result,
            Err(FrostSigningError::InsufficientSigners { .. })
        ));
    }

    #[test]
    fn test_key_share_serialization() {
        let relay_ids: Vec<[u8; 32]> = (0..7).map(|i| [i as u8 + 1; 32]).collect();
        let (key_shares, _) = generate_committee_keys(&relay_ids, 4).unwrap();

        let original = &key_shares[0];
        let bytes = original.to_bytes().unwrap();
        let restored = RelayFrostKeyShare::from_bytes(&bytes).unwrap();

        assert_eq!(original.participant_index, restored.participant_index);
        assert_eq!(original.relay_id, restored.relay_id);
        assert_eq!(original.group_public_key, restored.group_public_key);
        assert_eq!(original.threshold, restored.threshold);
    }

    #[test]
    fn test_committee_registration_creation() {
        let committee_id = [1u8; 32];
        let group_public_key = [2u8; 32];
        let relay_ids: Vec<[u8; 32]> = (0..5).map(|i| [i as u8 + 10; 32]).collect();
        let threshold = 3;

        let registration = CommitteeRegistration::new(
            committee_id,
            group_public_key,
            relay_ids.clone(),
            threshold,
            1000,
            [3u8; 32],
        );

        assert_eq!(registration.committee_id, committee_id);
        assert_eq!(registration.group_public_key, group_public_key);
        assert_eq!(registration.member_relay_ids.len(), 5);
        assert_eq!(registration.threshold, 3);
        assert!(registration.is_active);
    }

    #[test]
    fn test_committee_registration_membership_verification() {
        let relay_ids: Vec<[u8; 32]> = (0..5).map(|i| [i as u8 + 10; 32]).collect();
        let registration =
            CommitteeRegistration::new([1u8; 32], [2u8; 32], relay_ids.clone(), 3, 1000, [3u8; 32]);

        // Valid: 3 members (equals threshold)
        assert!(registration.verify_membership(&relay_ids[0..3]));

        // Valid: 4 members (above threshold)
        assert!(registration.verify_membership(&relay_ids[0..4]));

        // Invalid: 2 members (below threshold)
        assert!(!registration.verify_membership(&relay_ids[0..2]));

        // Invalid: non-member relay
        let fake_relay = [99u8; 32];
        assert!(!registration.verify_membership(&[fake_relay]));
    }

    #[test]
    fn test_committee_registration_serialization() {
        let relay_ids: Vec<[u8; 32]> = (0..5).map(|i| [i as u8 + 10; 32]).collect();
        let registration =
            CommitteeRegistration::new([1u8; 32], [2u8; 32], relay_ids, 3, 1000, [3u8; 32]);

        let bytes = registration.to_bytes().unwrap();
        let restored = CommitteeRegistration::from_bytes(&bytes).unwrap();

        assert_eq!(registration.committee_id, restored.committee_id);
        assert_eq!(registration.group_public_key, restored.group_public_key);
        assert_eq!(registration.threshold, restored.threshold);
        assert_eq!(
            registration.member_relay_ids.len(),
            restored.member_relay_ids.len()
        );
    }

    #[test]
    fn test_compute_committee_id_deterministic() {
        let relay_ids: Vec<[u8; 32]> = vec![[1u8; 32], [2u8; 32], [3u8; 32]];

        // Same order should produce same ID
        let id1 = CommitteeRegistration::compute_committee_id(&relay_ids);
        let id2 = CommitteeRegistration::compute_committee_id(&relay_ids);
        assert_eq!(id1, id2);

        // Different order should produce same ID (sorted internally)
        let relay_ids_reversed: Vec<[u8; 32]> = vec![[3u8; 32], [2u8; 32], [1u8; 32]];
        let id3 = CommitteeRegistration::compute_committee_id(&relay_ids_reversed);
        assert_eq!(id1, id3);

        // Different relays should produce different ID
        let different_relays: Vec<[u8; 32]> = vec![[4u8; 32], [5u8; 32], [6u8; 32]];
        let id4 = CommitteeRegistration::compute_committee_id(&different_relays);
        assert_ne!(id1, id4);
    }

    #[tokio::test]
    async fn test_committee_registry_basic_operations() {
        let registry = CommitteeRegistry::new();

        let relay_ids: Vec<[u8; 32]> = (0..5).map(|i| [i as u8 + 10; 32]).collect();
        let committee_id = [1u8; 32];
        let group_public_key = [2u8; 32];

        let registration = CommitteeRegistration::new(
            committee_id,
            group_public_key,
            relay_ids.clone(),
            3,
            1000,
            [3u8; 32],
        );

        // Register committee
        registry.register_committee(registration).await.unwrap();

        // Retrieve by committee ID
        let retrieved = registry.get_group_public_key(&committee_id).await;
        assert_eq!(retrieved, Some(group_public_key));

        // Retrieve full registration
        let full = registry.get_committee(&committee_id).await;
        assert!(full.is_some());
        assert_eq!(full.unwrap().threshold, 3);

        // Check stats
        let stats = registry.stats().await;
        assert_eq!(stats.total_committees, 1);
        assert_eq!(stats.active_committees, 1);
    }

    #[tokio::test]
    async fn test_committee_registry_find_by_relays() {
        let registry = CommitteeRegistry::new();

        let relay_ids: Vec<[u8; 32]> = (0..5).map(|i| [i as u8 + 10; 32]).collect();
        let committee_id = CommitteeRegistration::compute_committee_id(&relay_ids);
        let group_public_key = [2u8; 32];

        let registration = CommitteeRegistration::new(
            committee_id,
            group_public_key,
            relay_ids.clone(),
            3,
            1000,
            [3u8; 32],
        );

        registry.register_committee(registration).await.unwrap();

        // Find by exact relay set
        let found = registry.find_committee_by_relays(&relay_ids).await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().group_public_key, group_public_key);

        // Find by subset (threshold met)
        let subset: Vec<[u8; 32]> = relay_ids[0..3].to_vec();
        let found_subset = registry.find_committee_by_relays(&subset).await;
        assert!(found_subset.is_some());

        // Not found with unknown relays
        let unknown: Vec<[u8; 32]> = vec![[99u8; 32], [98u8; 32], [97u8; 32]];
        let not_found = registry.find_committee_by_relays(&unknown).await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_committee_registry_deactivation() {
        let registry = CommitteeRegistry::new();

        let relay_ids: Vec<[u8; 32]> = (0..5).map(|i| [i as u8 + 10; 32]).collect();
        let committee_id = [1u8; 32];

        let registration =
            CommitteeRegistration::new(committee_id, [2u8; 32], relay_ids, 3, 1000, [3u8; 32]);

        registry.register_committee(registration).await.unwrap();

        // Committee should be active
        assert!(registry.get_group_public_key(&committee_id).await.is_some());

        // Deactivate
        registry.deactivate_committee(&committee_id).await.unwrap();

        // Should no longer be returned by get_group_public_key (filters inactive)
        assert!(registry.get_group_public_key(&committee_id).await.is_none());

        // But still exists in full registration query
        let full = registry.get_committee(&committee_id).await;
        assert!(full.is_some());
        assert!(!full.unwrap().is_active);

        // Stats should show inactive
        let stats = registry.stats().await;
        assert_eq!(stats.inactive_committees, 1);
        assert_eq!(stats.active_committees, 0);
    }

    #[tokio::test]
    async fn test_committee_registry_verify_signature() {
        let registry = CommitteeRegistry::new();

        // Generate actual FROST keys
        let relay_ids: Vec<[u8; 32]> = (0..7).map(|i| [i as u8 + 1; 32]).collect();
        let (key_shares, group_pk) = generate_committee_keys(&relay_ids, 4).unwrap();

        let committee_id = CommitteeRegistration::compute_committee_id(&relay_ids);

        // Register the committee with actual group public key
        let registration = CommitteeRegistration::new(
            committee_id,
            group_pk,
            relay_ids.clone(),
            4,
            1000,
            [0u8; 32],
        );

        registry.register_committee(registration).await.unwrap();

        // Perform signing with first 4 relays
        let message = b"test message for verification";
        let session_id = "verify-test";

        // Create signers and sign
        let mut signers: Vec<RelayFrostSigner> = Vec::new();
        for share in &key_shares {
            let signer = RelayFrostSigner::new(share.relay_id);
            signer
                .register_key_share(committee_id, share.clone())
                .await
                .unwrap();
            signers.push(signer);
        }

        // Round 1
        let mut round1_outputs = Vec::new();
        for signer in signers.iter().take(4) {
            let output = signer
                .generate_round1(&committee_id, session_id.to_string(), message.to_vec())
                .await
                .unwrap();
            round1_outputs.push(output);
        }

        let commitments: BTreeMap<u16, Vec<u8>> = round1_outputs
            .iter()
            .map(|o| (o.participant_index, o.commitments.clone()))
            .collect();

        // Round 2
        let mut round2_outputs = Vec::new();
        for signer in signers.iter().take(4) {
            let output = signer
                .generate_round2(&committee_id, session_id, &commitments)
                .await
                .unwrap();
            round2_outputs.push(output);
        }

        let signature_shares: BTreeMap<u16, Vec<u8>> = round2_outputs
            .iter()
            .map(|o| (o.participant_index, o.signature_share.clone()))
            .collect();

        // Aggregate
        let aggregator = FrostSignatureAggregator::new(
            CommitteeFrostConfig::for_direct(),
            key_shares[0].all_verifying_shares.clone(),
            group_pk,
        );

        let aggregated = aggregator
            .aggregate(message, &commitments, &signature_shares, session_id)
            .unwrap();

        // Verify using registry
        let signing_relay_ids: Vec<[u8; 32]> = relay_ids[0..4].to_vec();
        let result = registry
            .verify_frost_signature_with_registry(
                &signing_relay_ids,
                &aggregated.signature,
                message,
                4,
            )
            .await;

        assert!(
            result.is_ok(),
            "Signature verification failed: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_relay_committee_mappings() {
        let registry = CommitteeRegistry::new();

        let relay_id = [42u8; 32];
        let relay_ids: Vec<[u8; 32]> = vec![relay_id, [1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]];

        // Register two committees that include the same relay
        let committee1_id = [100u8; 32];
        let committee2_id = [200u8; 32];

        let reg1 = CommitteeRegistration::new(
            committee1_id,
            [10u8; 32],
            relay_ids.clone(),
            3,
            1000,
            [0u8; 32],
        );

        let reg2 = CommitteeRegistration::new(
            committee2_id,
            [20u8; 32],
            relay_ids.clone(),
            3,
            2000,
            [0u8; 32],
        );

        registry.register_committee(reg1).await.unwrap();
        registry.register_committee(reg2).await.unwrap();

        // Relay should be in both committees
        let committees = registry.get_relay_committees(&relay_id).await;
        assert_eq!(committees.len(), 2);
        assert!(committees.contains(&committee1_id));
        assert!(committees.contains(&committee2_id));
    }
}
