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

impl std::fmt::Display for FrostSigningError {
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
}
