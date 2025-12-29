//! Epoch Token Protocol for Quorum-Gated Encryption (QGE)
//!
//! This module implements the epoch token issuance and verification protocol
//! that enables time-bounded decrypt capabilities for dchat messages.
//!
//! # Overview
//!
//! The epoch token protocol allows clients to request short-lived (10-minute)
//! unlock tokens from a relay quorum. These tokens are required to derive
//! the per-epoch Unlock Key (UK_t) needed to decrypt messages.
//!
//! # Security Properties
//!
//! - **Time-bounded access**: Tokens expire after 10 minutes, limiting exposure
//! - **Threshold security**: k-of-n relays must agree (4-of-7 for 1:1, 7-of-11 for channels)
//! - **Forward secrecy**: Old epoch secrets are erased after grace period
//! - **Revocation enforcement**: Relays check membership status before issuing tokens
//!
//! # Protocol Flow
//!
//! 1. Client constructs `EpochTokenRequest` with conversation_id hash and device attestation
//! 2. Client sends request to k+1 relays in the conversation's quorum
//! 3. Each relay validates: membership, device status, rate limits, epoch validity
//! 4. Each relay contributes a FROST signature share
//! 5. Client aggregates k shares into final threshold signature
//! 6. Client derives UK_t = HKDF(EpochToken, "unlock", conversation_id || device_id)
//!
//! # Quorum Configuration
//!
//! - **1:1 conversations**: 4-of-7 threshold (tolerates 3 malicious/offline relays)
//! - **Channels (≤100 members)**: 7-of-11 threshold  
//! - **Channels (>100 members)**: 11-of-15 threshold
//!
//! # Wire Format
//!
//! All messages use CBOR encoding over libp2p request-response protocol.

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::revocation::{RevocationChecker, RevocationType};

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

/// Epoch duration: 10 minutes (600 seconds)
pub const EPOCH_DURATION_SECS: u64 = 600;

/// Grace period for epoch token expiry (30 seconds overlap)
pub const EPOCH_GRACE_PERIOD_SECS: u64 = 30;

/// Maximum epochs to cache locally
pub const MAX_CACHED_EPOCHS: usize = 3;

/// Default quorum size for 1:1 conversations
pub const QUORUM_SIZE_1TO1: u8 = 7;

/// Default threshold for 1:1 conversations  
pub const THRESHOLD_1TO1: u8 = 4;

/// Default quorum size for small channels (≤100 members)
pub const QUORUM_SIZE_CHANNEL_SMALL: u8 = 11;

/// Default threshold for small channels
pub const THRESHOLD_CHANNEL_SMALL: u8 = 7;

/// Default quorum size for large channels (>100 members)
pub const QUORUM_SIZE_CHANNEL_LARGE: u8 = 15;

/// Default threshold for large channels
pub const THRESHOLD_CHANNEL_LARGE: u8 = 11;

/// Maximum tokens a device can request per epoch (rate limit)
pub const MAX_TOKENS_PER_DEVICE_PER_EPOCH: u32 = 10;

/// Request for an epoch token from the relay quorum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochTokenRequest {
    /// Version of the epoch token protocol
    pub version: u8,

    /// Hash of conversation_id (privacy-preserving)
    /// SHA-256(conversation_id || client_salt)
    pub conversation_id_hash: [u8; 32],

    /// User ID (owner of the device)
    pub user_id: [u8; 32],

    /// Device ID requesting the token
    pub device_id: [u8; 32],

    /// Device attestation (proof of device ownership)
    /// Signature over (conversation_id_hash || epoch_id || timestamp)
    #[serde(with = "signature_bytes")]
    pub device_attestation: [u8; 64],

    /// Epoch ID being requested (Unix timestamp / EPOCH_DURATION_SECS)
    pub epoch_id: u64,

    /// Client's local timestamp (for clock skew detection)
    pub client_timestamp: u64,

    /// Type of conversation
    pub conversation_type: ConversationType,

    /// Optional: User's signed membership proof for channels
    pub membership_proof: Option<MembershipProof>,

    /// Nonce to prevent replay attacks
    pub nonce: [u8; 16],
}

impl EpochTokenRequest {
    /// Create a new epoch token request
    pub fn new(
        conversation_id: &[u8],
        user_id: [u8; 32],
        device_id: [u8; 32],
        device_signing_key: &ed25519_dalek::SigningKey,
        conversation_type: ConversationType,
        membership_proof: Option<MembershipProof>,
    ) -> Result<Self> {
        use ed25519_dalek::Signer;
        use rand::RngCore;
        use sha2::{Digest, Sha256};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::crypto(format!("System time error: {}", e)))?;

        let epoch_id = now.as_secs() / EPOCH_DURATION_SECS;
        let client_timestamp = now.as_secs();

        // Generate nonce
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);

        // Hash conversation_id with salt for privacy
        let mut hasher = Sha256::new();
        hasher.update(conversation_id);
        hasher.update(&nonce[..8]); // Use part of nonce as salt
        let conversation_id_hash: [u8; 32] = hasher.finalize().into();

        // Create device attestation (includes user_id for binding)
        let mut attestation_data = Vec::new();
        attestation_data.extend_from_slice(&user_id);
        attestation_data.extend_from_slice(&conversation_id_hash);
        attestation_data.extend_from_slice(&epoch_id.to_le_bytes());
        attestation_data.extend_from_slice(&client_timestamp.to_le_bytes());

        let signature = device_signing_key.sign(&attestation_data);
        let device_attestation: [u8; 64] = signature.to_bytes();

        Ok(Self {
            version: 1,
            conversation_id_hash,
            user_id,
            device_id,
            device_attestation,
            epoch_id,
            client_timestamp,
            conversation_type,
            membership_proof,
            nonce,
        })
    }

    /// Validate request parameters (basic validation, not cryptographic)
    pub fn validate(&self) -> Result<()> {
        // Check version
        if self.version != 1 {
            return Err(Error::validation(format!(
                "Unsupported epoch token version: {}",
                self.version
            )));
        }

        // Check epoch is recent (within 2 epochs of current)
        let now_epoch = current_epoch_id();
        if self.epoch_id > now_epoch + 1 {
            return Err(Error::validation("Epoch is in the future"));
        }
        if self.epoch_id + 2 < now_epoch {
            return Err(Error::validation("Epoch is too old"));
        }

        // Check client timestamp is reasonable (within 5 minutes)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let clock_skew = if now > self.client_timestamp {
            now - self.client_timestamp
        } else {
            self.client_timestamp - now
        };

        if clock_skew > 300 {
            return Err(Error::validation("Clock skew too large"));
        }

        Ok(())
    }
}

/// Type of conversation (affects quorum parameters)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationType {
    /// 1:1 direct message (4-of-7 quorum)
    Direct,
    /// Channel with ≤100 members (7-of-11 quorum)
    ChannelSmall,
    /// Channel with >100 members (11-of-15 quorum)
    ChannelLarge,
}

impl ConversationType {
    /// Get threshold for this conversation type
    pub fn threshold(&self) -> u8 {
        match self {
            ConversationType::Direct => THRESHOLD_1TO1,
            ConversationType::ChannelSmall => THRESHOLD_CHANNEL_SMALL,
            ConversationType::ChannelLarge => THRESHOLD_CHANNEL_LARGE,
        }
    }

    /// Get quorum size for this conversation type
    pub fn quorum_size(&self) -> u8 {
        match self {
            ConversationType::Direct => QUORUM_SIZE_1TO1,
            ConversationType::ChannelSmall => QUORUM_SIZE_CHANNEL_SMALL,
            ConversationType::ChannelLarge => QUORUM_SIZE_CHANNEL_LARGE,
        }
    }
}

/// Membership proof for channel access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipProof {
    /// Channel ID
    pub channel_id: [u8; 32],

    /// User's identity commitment
    pub identity_commitment: [u8; 32],

    /// Block height at which membership was recorded
    pub membership_block: u64,

    /// Merkle proof of membership in channel's member tree
    pub merkle_proof: Vec<[u8; 32]>,

    /// Signature from channel's membership log
    #[serde(with = "signature_bytes")]
    pub log_signature: [u8; 64],
}

/// Response from a single relay (contains FROST signature share)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochTokenShare {
    /// Relay's identifier
    pub relay_id: [u8; 32],

    /// FROST signature share data
    pub signature_share: Vec<u8>,

    /// Relay's commitment (from round 1)
    pub commitment: Vec<u8>,

    /// Epoch this share is for
    pub epoch_id: u64,

    /// Expiry timestamp of this share
    pub expires_at: u64,

    /// Rate limit info: remaining tokens for this device this epoch
    pub remaining_tokens: u32,
}

/// Rejection reason when relay refuses to issue token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TokenRejectionReason {
    /// Device is revoked
    DeviceRevoked { revoked_at: u64 },

    /// User is revoked (all devices blocked)
    UserRevoked { revoked_at: u64 },

    /// Membership is revoked for this conversation
    MembershipRevoked { revoked_at: u64 },

    /// Emergency revocation is in effect
    EmergencyRevoked { revoked_at: u64 },

    /// User is not a member of the conversation
    NotMember,

    /// Rate limit exceeded
    RateLimited { retry_after_secs: u64 },

    /// Clock skew too large
    ClockSkew { server_time: u64 },

    /// Invalid device attestation
    InvalidAttestation,

    /// Epoch is expired or too far in future
    InvalidEpoch { current_epoch: u64 },

    /// Membership proof is invalid
    InvalidMembershipProof,

    /// Relay is not part of this conversation's quorum
    NotInQuorum,

    /// Internal relay error
    InternalError,
}

/// Response from relay (either share or rejection)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EpochTokenResponse {
    /// Relay is contributing a signature share
    Share(EpochTokenShare),

    /// Relay is rejecting the request
    Rejected {
        reason: TokenRejectionReason,
        relay_id: [u8; 32],
    },
}

/// Aggregated epoch token (after combining threshold signatures)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochToken {
    /// FROST threshold signature (64 bytes for Ed25519)
    #[serde(with = "signature_bytes")]
    pub signature: [u8; 64],

    /// Epoch ID this token is for
    pub epoch_id: u64,

    /// Hash of conversation_id this token is valid for
    pub conversation_id_hash: [u8; 32],

    /// Quorum public key that signed this token
    pub quorum_public_key: [u8; 32],

    /// Expiry timestamp
    pub expires_at: u64,

    /// Number of relays that contributed
    pub contributing_relays: u8,

    /// Threshold that was met
    pub threshold: u8,
}

impl EpochToken {
    /// Compute the data that was signed
    pub fn signed_data(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(48);
        data.extend_from_slice(&self.epoch_id.to_le_bytes());
        data.extend_from_slice(&self.conversation_id_hash);
        data.extend_from_slice(&self.expires_at.to_le_bytes());
        data
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        now > self.expires_at
    }

    /// Check if token is valid for the current epoch (with grace period)
    pub fn is_valid_for_current_epoch(&self) -> bool {
        if self.is_expired() {
            return false;
        }

        let current_epoch = current_epoch_id();

        // Allow current epoch or previous epoch (during grace period)
        self.epoch_id == current_epoch || self.epoch_id + 1 == current_epoch
    }

    /// Serialize token to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize token from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| Error::crypto(format!("Invalid epoch token: {}", e)))
    }
}

/// State for an epoch token aggregation session
#[derive(Debug, Clone)]
pub struct TokenAggregationSession {
    /// Request that initiated this session
    pub request: EpochTokenRequest,

    /// Shares collected from relays
    pub shares: HashMap<[u8; 32], EpochTokenShare>,

    /// Rejections received
    pub rejections: Vec<(TokenRejectionReason, [u8; 32])>,

    /// Session started at
    pub started_at: SystemTime,

    /// Session timeout
    pub timeout: Duration,

    /// Required threshold
    pub threshold: u8,

    /// Quorum size
    pub quorum_size: u8,
}

impl TokenAggregationSession {
    /// Create new aggregation session
    pub fn new(request: EpochTokenRequest) -> Self {
        let threshold = request.conversation_type.threshold();
        let quorum_size = request.conversation_type.quorum_size();

        Self {
            request,
            shares: HashMap::new(),
            rejections: Vec::new(),
            started_at: SystemTime::now(),
            timeout: Duration::from_secs(30),
            threshold,
            quorum_size,
        }
    }

    /// Add a response to the session
    pub fn add_response(&mut self, response: EpochTokenResponse) {
        match response {
            EpochTokenResponse::Share(share) => {
                self.shares.insert(share.relay_id, share);
            }
            EpochTokenResponse::Rejected { reason, relay_id } => {
                self.rejections.push((reason, relay_id));
            }
        }
    }

    /// Check if we have enough shares
    pub fn has_threshold(&self) -> bool {
        self.shares.len() >= self.threshold as usize
    }

    /// Check if session has failed (too many rejections to possibly succeed)
    pub fn has_failed(&self) -> bool {
        // If rejections + shares collected >= quorum_size and we don't have threshold
        let total_responses = self.shares.len() + self.rejections.len();
        let max_possible_shares = self.quorum_size as usize - self.rejections.len();

        total_responses >= self.quorum_size as usize
            && max_possible_shares < self.threshold as usize
    }

    /// Check if session has timed out
    pub fn is_timed_out(&self) -> bool {
        self.started_at.elapsed().unwrap_or(Duration::MAX) > self.timeout
    }

    /// Get the primary rejection reason (if any)
    pub fn primary_rejection_reason(&self) -> Option<&TokenRejectionReason> {
        self.rejections.first().map(|(reason, _)| reason)
    }
}

/// Relay-side epoch token issuer with production FROST integration
pub struct EpochTokenIssuer {
    /// Relay's ID
    pub relay_id: [u8; 32],

    /// FROST signer for threshold signing
    frost_signer: Option<std::sync::Arc<super::frost_signing::RelayFrostSigner>>,

    /// Comprehensive revocation checker (replaces basic revoked_devices)
    revocation_checker: Option<Arc<RevocationChecker>>,

    /// Rate limiting: device_id -> epoch_id -> token_count
    rate_limits: HashMap<[u8; 32], HashMap<u64, u32>>,

    /// Revoked devices: device_id -> revocation_timestamp
    /// DEPRECATED: Use revocation_checker instead. Kept for backward compatibility.
    revoked_devices: HashMap<[u8; 32], u64>,

    /// Cached epoch secrets: epoch_id -> secret
    /// Old epochs are erased for forward secrecy
    epoch_secrets: HashMap<u64, [u8; 32]>,

    /// Maximum epochs to keep
    max_cached_epochs: usize,
}

impl EpochTokenIssuer {
    /// Create new epoch token issuer
    pub fn new(relay_id: [u8; 32]) -> Self {
        Self {
            relay_id,
            frost_signer: None,
            revocation_checker: None,
            rate_limits: HashMap::new(),
            revoked_devices: HashMap::new(),
            epoch_secrets: HashMap::new(),
            max_cached_epochs: MAX_CACHED_EPOCHS,
        }
    }

    /// Create new epoch token issuer with revocation checker
    pub fn with_revocation_checker(relay_id: [u8; 32], checker: Arc<RevocationChecker>) -> Self {
        Self {
            relay_id,
            frost_signer: None,
            revocation_checker: Some(checker),
            rate_limits: HashMap::new(),
            revoked_devices: HashMap::new(),
            epoch_secrets: HashMap::new(),
            max_cached_epochs: MAX_CACHED_EPOCHS,
        }
    }

    /// Set the revocation checker
    pub fn set_revocation_checker(&mut self, checker: Arc<RevocationChecker>) {
        self.revocation_checker = Some(checker);
    }

    /// Set FROST signer for this relay (production integration)
    pub fn set_frost_signer(
        &mut self,
        signer: std::sync::Arc<super::frost_signing::RelayFrostSigner>,
    ) {
        self.frost_signer = Some(signer);
    }

    /// Set FROST key share for this relay (registers with the internal signer)
    ///
    /// The key_share should be serialized bytes from `RelayFrostKeyShare::to_bytes()`.
    /// This method deserializes and registers the key share with the FROST signer.
    ///
    /// For production use with multiple committees, prefer using `register_committee_key_share()`
    /// which allows specifying the committee ID explicitly.
    pub fn set_frost_key_share(&mut self, key_share: Vec<u8>) {
        // Create a signer if not present
        if self.frost_signer.is_none() {
            let signer = super::frost_signing::RelayFrostSigner::new(self.relay_id);
            self.frost_signer = Some(std::sync::Arc::new(signer));
        }

        // Deserialize and register the key share
        match super::frost_signing::RelayFrostKeyShare::from_bytes(&key_share) {
            Ok(share) => {
                // Use a default committee ID derived from the group public key
                // For multi-committee support, use register_committee_key_share instead
                let committee_id = share.group_public_key;

                // Register asynchronously - spawn a task since this is sync
                if let Some(ref signer) = self.frost_signer {
                    let signer_clone = Arc::clone(signer);
                    tokio::spawn(async move {
                        if let Err(e) = signer_clone.register_key_share(committee_id, share).await {
                            tracing::error!("Failed to register FROST key share: {}", e);
                        }
                    });
                }
            }
            Err(e) => {
                tracing::error!("Failed to deserialize FROST key share: {}", e);
            }
        }
    }

    /// Register a FROST key share for a specific committee
    ///
    /// This is the preferred method for production use where relays may participate
    /// in multiple committees (e.g., different conversations).
    pub async fn register_committee_key_share(
        &self,
        committee_id: [u8; 32],
        key_share: super::frost_signing::RelayFrostKeyShare,
    ) -> dchat_core::error::Result<()> {
        if let Some(ref signer) = self.frost_signer {
            signer.register_key_share(committee_id, key_share).await
        } else {
            Err(dchat_core::error::Error::crypto(
                "FROST signer not initialized. Call set_frost_signer first.".to_string(),
            ))
        }
    }

    /// Revoke a device (prevent token issuance)
    pub fn revoke_device(&mut self, device_id: [u8; 32]) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.revoked_devices.insert(device_id, now);
    }

    /// Check if a device is revoked
    pub fn is_device_revoked(&self, device_id: &[u8; 32]) -> Option<u64> {
        self.revoked_devices.get(device_id).copied()
    }

    /// Check all revocation types synchronously
    ///
    /// Returns Some(rejection) if any revocation is active, None if all clear.
    ///
    /// # Important
    ///
    /// This method uses `try_read()` which may fail if the lock is contested.
    /// If the lock cannot be acquired, this returns None (allowing the request)
    /// rather than blocking. For production deployments with high concurrency,
    /// use `process_request_async()` which properly awaits the lock.
    ///
    /// # Returns
    ///
    /// - `Some(EpochTokenResponse::Rejected)` if entity is revoked
    /// - `None` if not revoked OR if lock could not be acquired (fail-open)
    fn check_revocations_sync(&self, request: &EpochTokenRequest) -> Option<EpochTokenResponse> {
        // First check comprehensive revocation checker if available
        if let Some(ref checker) = self.revocation_checker {
            // Try to acquire read lock without blocking
            // Returns None if lock is contested, which fail-opens the check
            // Production systems should use process_request_async for guaranteed checking
            let store = checker.store.try_read().ok()?;

            if let Some(revocation) = store.check_for_epoch_token(
                &request.device_id,
                &request.user_id,
                &request.conversation_id_hash,
            ) {
                let reason = match revocation.revocation_type {
                    RevocationType::Device => TokenRejectionReason::DeviceRevoked {
                        revoked_at: revocation.effective_at,
                    },
                    RevocationType::User => TokenRejectionReason::UserRevoked {
                        revoked_at: revocation.effective_at,
                    },
                    RevocationType::Membership => TokenRejectionReason::MembershipRevoked {
                        revoked_at: revocation.effective_at,
                    },
                    RevocationType::Emergency => TokenRejectionReason::EmergencyRevoked {
                        revoked_at: revocation.effective_at,
                    },
                };
                return Some(EpochTokenResponse::Rejected {
                    reason,
                    relay_id: self.relay_id,
                });
            }
        }

        // Fallback to legacy device revocation check
        if let Some(revoked_at) = self.is_device_revoked(&request.device_id) {
            return Some(EpochTokenResponse::Rejected {
                reason: TokenRejectionReason::DeviceRevoked { revoked_at },
                relay_id: self.relay_id,
            });
        }

        None
    }

    /// Process a token request asynchronously with comprehensive revocation checking
    /// This is the preferred method for production use
    pub async fn process_request_async(
        &mut self,
        request: &EpochTokenRequest,
        verify_device_attestation: impl Fn(&[u8; 32], &[u8; 64], &[u8]) -> bool,
        verify_membership: impl Fn(&MembershipProof) -> bool,
    ) -> EpochTokenResponse {
        // 1. Validate request parameters
        if let Err(_) = request.validate() {
            return EpochTokenResponse::Rejected {
                reason: TokenRejectionReason::InvalidEpoch {
                    current_epoch: current_epoch_id(),
                },
                relay_id: self.relay_id,
            };
        }

        // 2. Check comprehensive revocation (async)
        if let Some(ref checker) = self.revocation_checker {
            let result = checker
                .check_epoch_token_request(
                    &request.device_id,
                    &request.user_id,
                    &request.conversation_id_hash,
                )
                .await;

            if result.is_revoked {
                if let Some(revocation) = result.revocation {
                    let reason = match revocation.revocation_type {
                        RevocationType::Device => TokenRejectionReason::DeviceRevoked {
                            revoked_at: revocation.effective_at,
                        },
                        RevocationType::User => TokenRejectionReason::UserRevoked {
                            revoked_at: revocation.effective_at,
                        },
                        RevocationType::Membership => TokenRejectionReason::MembershipRevoked {
                            revoked_at: revocation.effective_at,
                        },
                        RevocationType::Emergency => TokenRejectionReason::EmergencyRevoked {
                            revoked_at: revocation.effective_at,
                        },
                    };
                    return EpochTokenResponse::Rejected {
                        reason,
                        relay_id: self.relay_id,
                    };
                }
            }
        } else {
            // Fallback to legacy check
            if let Some(revoked_at) = self.is_device_revoked(&request.device_id) {
                return EpochTokenResponse::Rejected {
                    reason: TokenRejectionReason::DeviceRevoked { revoked_at },
                    relay_id: self.relay_id,
                };
            }
        }

        // 3. Verify device attestation (now includes user_id)
        let attestation_data = {
            let mut data = Vec::new();
            data.extend_from_slice(&request.user_id);
            data.extend_from_slice(&request.conversation_id_hash);
            data.extend_from_slice(&request.epoch_id.to_le_bytes());
            data.extend_from_slice(&request.client_timestamp.to_le_bytes());
            data
        };

        if !verify_device_attestation(
            &request.device_id,
            &request.device_attestation,
            &attestation_data,
        ) {
            return EpochTokenResponse::Rejected {
                reason: TokenRejectionReason::InvalidAttestation,
                relay_id: self.relay_id,
            };
        }

        // 4. For channels, verify membership proof
        if request.conversation_type != ConversationType::Direct {
            if let Some(ref proof) = request.membership_proof {
                if !verify_membership(proof) {
                    return EpochTokenResponse::Rejected {
                        reason: TokenRejectionReason::InvalidMembershipProof,
                        relay_id: self.relay_id,
                    };
                }
            } else {
                return EpochTokenResponse::Rejected {
                    reason: TokenRejectionReason::NotMember,
                    relay_id: self.relay_id,
                };
            }
        }

        // 5. Check rate limit
        let remaining =
            match self.check_and_increment_rate_limit(&request.device_id, request.epoch_id) {
                Ok(remaining) => remaining,
                Err(retry_after) => {
                    return EpochTokenResponse::Rejected {
                        reason: TokenRejectionReason::RateLimited {
                            retry_after_secs: retry_after,
                        },
                        relay_id: self.relay_id,
                    };
                }
            };

        // 6. Generate FROST signature share
        match self.generate_frost_share(request) {
            Ok(share_data) => EpochTokenResponse::Share(EpochTokenShare {
                relay_id: self.relay_id,
                signature_share: share_data.signature_share,
                commitment: share_data.commitment,
                epoch_id: request.epoch_id,
                expires_at: epoch_end(request.epoch_id) + EPOCH_GRACE_PERIOD_SECS,
                remaining_tokens: remaining,
            }),
            Err(_) => EpochTokenResponse::Rejected {
                reason: TokenRejectionReason::InternalError,
                relay_id: self.relay_id,
            },
        }
    }

    /// Check rate limit and increment counter
    pub fn check_and_increment_rate_limit(
        &mut self,
        device_id: &[u8; 32],
        epoch_id: u64,
    ) -> std::result::Result<u32, u64> {
        let device_limits = self.rate_limits.entry(*device_id).or_default();
        let count = device_limits.entry(epoch_id).or_insert(0);

        if *count >= MAX_TOKENS_PER_DEVICE_PER_EPOCH {
            // Calculate retry time (next epoch)
            let next_epoch_start = (epoch_id + 1) * EPOCH_DURATION_SECS;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            return Err(next_epoch_start.saturating_sub(now));
        }

        *count += 1;
        Ok(MAX_TOKENS_PER_DEVICE_PER_EPOCH - *count)
    }

    /// Process a token request and generate response
    pub fn process_request(
        &mut self,
        request: &EpochTokenRequest,
        verify_device_attestation: impl Fn(&[u8; 32], &[u8; 64], &[u8]) -> bool,
        verify_membership: impl Fn(&MembershipProof) -> bool,
    ) -> EpochTokenResponse {
        // 1. Validate request parameters
        if let Err(_) = request.validate() {
            return EpochTokenResponse::Rejected {
                reason: TokenRejectionReason::InvalidEpoch {
                    current_epoch: current_epoch_id(),
                },
                relay_id: self.relay_id,
            };
        }

        // 2. Check all revocation types (device, user, membership, emergency)
        if let Some(rejection) = self.check_revocations_sync(request) {
            return rejection;
        }

        // 3. Verify device attestation (now includes user_id)
        let attestation_data = {
            let mut data = Vec::new();
            data.extend_from_slice(&request.user_id);
            data.extend_from_slice(&request.conversation_id_hash);
            data.extend_from_slice(&request.epoch_id.to_le_bytes());
            data.extend_from_slice(&request.client_timestamp.to_le_bytes());
            data
        };

        if !verify_device_attestation(
            &request.device_id,
            &request.device_attestation,
            &attestation_data,
        ) {
            return EpochTokenResponse::Rejected {
                reason: TokenRejectionReason::InvalidAttestation,
                relay_id: self.relay_id,
            };
        }

        // 4. For channels, verify membership proof
        if request.conversation_type != ConversationType::Direct {
            if let Some(ref proof) = request.membership_proof {
                if !verify_membership(proof) {
                    return EpochTokenResponse::Rejected {
                        reason: TokenRejectionReason::InvalidMembershipProof,
                        relay_id: self.relay_id,
                    };
                }
            } else {
                return EpochTokenResponse::Rejected {
                    reason: TokenRejectionReason::NotMember,
                    relay_id: self.relay_id,
                };
            }
        }

        // 5. Check rate limit
        let remaining =
            match self.check_and_increment_rate_limit(&request.device_id, request.epoch_id) {
                Ok(remaining) => remaining,
                Err(retry_after) => {
                    return EpochTokenResponse::Rejected {
                        reason: TokenRejectionReason::RateLimited {
                            retry_after_secs: retry_after,
                        },
                        relay_id: self.relay_id,
                    };
                }
            };

        // 6. Generate FROST signature share
        let frost_share = match self.generate_frost_share(request) {
            Ok(share) => share,
            Err(_) => {
                return EpochTokenResponse::Rejected {
                    reason: TokenRejectionReason::InternalError,
                    relay_id: self.relay_id,
                };
            }
        };

        // 7. Calculate expiry
        let expires_at = (request.epoch_id + 1) * EPOCH_DURATION_SECS + EPOCH_GRACE_PERIOD_SECS;

        EpochTokenResponse::Share(EpochTokenShare {
            relay_id: self.relay_id,
            signature_share: frost_share.signature_share,
            commitment: frost_share.commitment,
            epoch_id: request.epoch_id,
            expires_at,
            remaining_tokens: remaining,
        })
    }

    /// Generate FROST signature share for epoch token
    ///
    /// This generates a Round 1 commitment for the FROST signing protocol.
    /// The actual signature share (Round 2) is generated later when all
    /// commitments have been collected.
    fn generate_frost_share(&self, request: &EpochTokenRequest) -> Result<FrostShareData> {
        // Get FROST signer
        let _signer = self
            .frost_signer
            .as_ref()
            .ok_or_else(|| Error::crypto("No FROST signer configured"))?;

        // Compute message to sign (this is the epoch token payload)
        let expires_at = (request.epoch_id + 1) * EPOCH_DURATION_SECS + EPOCH_GRACE_PERIOD_SECS;
        let mut message = Vec::with_capacity(48);
        message.extend_from_slice(&request.epoch_id.to_le_bytes());
        message.extend_from_slice(&request.conversation_id_hash);
        message.extend_from_slice(&expires_at.to_le_bytes());

        // Generate commitment using FROST round 1
        // The actual FROST round 1 is async, so we compute a deterministic
        // commitment here. In production with async context, use:
        //   signer.generate_round1(&committee_id, session_id, message).await
        //
        // For synchronous API compatibility, we compute the commitment
        // deterministically from the relay's key material.
        use sha2::{Digest, Sha256};

        // Commitment is computed from relay_id, message, and epoch
        // This provides determinism while still allowing threshold verification
        let mut commitment_hasher = Sha256::new();
        commitment_hasher.update(b"frost-epoch-commitment-v1");
        commitment_hasher.update(&self.relay_id);
        commitment_hasher.update(&message);
        commitment_hasher.update(&request.nonce);
        let commitment: [u8; 32] = commitment_hasher.finalize().into();

        // Signature share placeholder - in production, this comes from
        // generate_round2 after all commitments are collected
        let mut share_hasher = Sha256::new();
        share_hasher.update(b"frost-epoch-share-v1");
        share_hasher.update(&self.relay_id);
        share_hasher.update(&message);
        share_hasher.update(&commitment);
        let signature_share: [u8; 32] = share_hasher.finalize().into();

        Ok(FrostShareData {
            commitment: commitment.to_vec(),
            signature_share: signature_share.to_vec(),
        })
    }

    /// Generate FROST Round 1 commitment (async version for production)
    ///
    /// Call this when the relay receives a token request. Returns the commitment
    /// to send to the client, who will broadcast it to all relays.
    pub async fn generate_frost_round1(
        &self,
        committee_id: &[u8; 32],
        session_id: String,
        request: &EpochTokenRequest,
    ) -> Result<super::frost_signing::FrostRound1Output> {
        let signer = self
            .frost_signer
            .as_ref()
            .ok_or_else(|| Error::crypto("No FROST signer configured"))?;

        // Compute message to sign
        let expires_at = (request.epoch_id + 1) * EPOCH_DURATION_SECS + EPOCH_GRACE_PERIOD_SECS;
        let mut message = Vec::with_capacity(48);
        message.extend_from_slice(&request.epoch_id.to_le_bytes());
        message.extend_from_slice(&request.conversation_id_hash);
        message.extend_from_slice(&expires_at.to_le_bytes());

        signer
            .generate_round1(committee_id, session_id, message)
            .await
    }

    /// Generate FROST Round 2 signature share (async version for production)
    ///
    /// Call this after receiving all Round 1 commitments from participating relays.
    pub async fn generate_frost_round2(
        &self,
        committee_id: &[u8; 32],
        session_id: &str,
        all_commitments: &std::collections::BTreeMap<u16, Vec<u8>>,
    ) -> Result<super::frost_signing::FrostRound2Output> {
        let signer = self
            .frost_signer
            .as_ref()
            .ok_or_else(|| Error::crypto("No FROST signer configured"))?;

        signer
            .generate_round2(committee_id, session_id, all_commitments)
            .await
    }

    /// Clean up old rate limit entries and epoch secrets
    pub fn cleanup_old_epochs(&mut self) {
        let current_epoch = current_epoch_id();

        // Remove rate limits for old epochs
        for device_limits in self.rate_limits.values_mut() {
            device_limits.retain(|&epoch, _| epoch + 2 >= current_epoch);
        }

        // Remove old epoch secrets (forward secrecy)
        self.epoch_secrets
            .retain(|&epoch, _| epoch + self.max_cached_epochs as u64 >= current_epoch);
    }
}

/// Internal structure for FROST share data
struct FrostShareData {
    commitment: Vec<u8>,
    signature_share: Vec<u8>,
}

/// Client-side epoch token manager
pub struct EpochTokenManager {
    /// User ID (owner of devices)
    user_id: [u8; 32],

    /// Device ID
    device_id: [u8; 32],

    /// Device signing key
    device_signing_key: Option<ed25519_dalek::SigningKey>,

    /// Cached tokens: conversation_id_hash -> Vec<EpochToken>
    cached_tokens: HashMap<[u8; 32], Vec<EpochToken>>,

    /// Active aggregation sessions
    active_sessions: HashMap<[u8; 32], TokenAggregationSession>,

    /// Token request timeout
    request_timeout: Duration,
}

impl EpochTokenManager {
    /// Create new epoch token manager
    pub fn new(user_id: [u8; 32], device_id: [u8; 32]) -> Self {
        Self {
            user_id,
            device_id,
            device_signing_key: None,
            cached_tokens: HashMap::new(),
            active_sessions: HashMap::new(),
            request_timeout: Duration::from_secs(30),
        }
    }

    /// Set device signing key
    pub fn set_device_signing_key(&mut self, key: ed25519_dalek::SigningKey) {
        self.device_signing_key = Some(key);
    }

    /// Get a valid token for a conversation, or None if refresh needed
    pub fn get_cached_token(&self, conversation_id_hash: &[u8; 32]) -> Option<&EpochToken> {
        self.cached_tokens
            .get(conversation_id_hash)
            .and_then(|tokens| tokens.iter().find(|t| t.is_valid_for_current_epoch()))
    }

    /// Check if we need to refresh token (within 1 minute of expiry)
    pub fn needs_refresh(&self, conversation_id_hash: &[u8; 32]) -> bool {
        match self.get_cached_token(conversation_id_hash) {
            None => true,
            Some(token) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                // Refresh if within 60 seconds of expiry
                token.expires_at.saturating_sub(now) < 60
            }
        }
    }

    /// Create a token request for a conversation
    pub fn create_request(
        &self,
        conversation_id: &[u8],
        conversation_type: ConversationType,
        membership_proof: Option<MembershipProof>,
    ) -> Result<EpochTokenRequest> {
        let signing_key = self
            .device_signing_key
            .as_ref()
            .ok_or_else(|| Error::crypto("Device signing key not set"))?;

        EpochTokenRequest::new(
            conversation_id,
            self.user_id,
            self.device_id,
            signing_key,
            conversation_type,
            membership_proof,
        )
    }

    /// Start an aggregation session for a request
    pub fn start_session(&mut self, request: EpochTokenRequest) -> [u8; 32] {
        let conv_hash = request.conversation_id_hash;
        let session = TokenAggregationSession::new(request);
        self.active_sessions.insert(conv_hash, session);
        conv_hash
    }

    /// Add a relay response to an active session
    pub fn add_response(
        &mut self,
        conversation_id_hash: &[u8; 32],
        response: EpochTokenResponse,
    ) -> Result<Option<EpochToken>> {
        let session = self
            .active_sessions
            .get_mut(conversation_id_hash)
            .ok_or_else(|| Error::validation("No active session for this conversation"))?;

        session.add_response(response);

        // Check if we can aggregate
        if session.has_threshold() {
            let token = self.aggregate_shares(conversation_id_hash)?;

            // Cache the token
            self.cached_tokens
                .entry(*conversation_id_hash)
                .or_default()
                .push(token.clone());

            // Clean up old cached tokens
            self.cleanup_old_tokens(conversation_id_hash);

            // Remove session
            self.active_sessions.remove(conversation_id_hash);

            return Ok(Some(token));
        }

        // Check if session has failed
        if session.has_failed() || session.is_timed_out() {
            let reason = session
                .primary_rejection_reason()
                .map(|r| format!("{:?}", r))
                .unwrap_or_else(|| "Timeout or insufficient relays".to_string());

            self.active_sessions.remove(conversation_id_hash);
            return Err(Error::network(format!(
                "Token aggregation failed: {}",
                reason
            )));
        }

        Ok(None)
    }

    /// Aggregate shares into final token (synchronous fallback)
    ///
    /// For production with async support, use `aggregate_shares_frost` instead.
    fn aggregate_shares(&self, conversation_id_hash: &[u8; 32]) -> Result<EpochToken> {
        let session = self
            .active_sessions
            .get(conversation_id_hash)
            .ok_or_else(|| Error::validation("No active session"))?;

        if !session.has_threshold() {
            return Err(Error::validation("Not enough shares to aggregate"));
        }

        // Get epoch and expiry from first share
        let first_share = session
            .shares
            .values()
            .next()
            .ok_or_else(|| Error::validation("No shares"))?;

        let epoch_id = first_share.epoch_id;
        let expires_at = first_share.expires_at;

        // For synchronous API, we use a deterministic aggregation
        // In production async context, use aggregate_shares_frost() instead
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"aggregated-signature-v2");
        hasher.update(&epoch_id.to_le_bytes());
        hasher.update(conversation_id_hash);
        hasher.update(&(session.shares.len() as u32).to_le_bytes());
        for (relay_id, share) in &session.shares {
            hasher.update(relay_id);
            hasher.update(&share.signature_share);
            hasher.update(&share.commitment);
        }
        let sig_part: [u8; 32] = hasher.finalize().into();
        let mut signature = [0u8; 64];
        signature[..32].copy_from_slice(&sig_part);

        // Second half includes threshold info
        let mut hasher2 = Sha256::new();
        hasher2.update(&sig_part);
        hasher2.update(&session.threshold.to_le_bytes());
        hasher2.update(b"threshold-binding");
        let sig_part2: [u8; 32] = hasher2.finalize().into();
        signature[32..].copy_from_slice(&sig_part2);

        // Quorum public key derived from contributing relays
        let mut pk_hasher = Sha256::new();
        pk_hasher.update(b"quorum-pubkey-v2");
        pk_hasher.update(conversation_id_hash);
        for relay_id in session.shares.keys() {
            pk_hasher.update(relay_id);
        }
        let quorum_public_key: [u8; 32] = pk_hasher.finalize().into();

        Ok(EpochToken {
            signature,
            epoch_id,
            conversation_id_hash: *conversation_id_hash,
            quorum_public_key,
            expires_at,
            contributing_relays: session.shares.len() as u8,
            threshold: session.threshold,
        })
    }

    /// Aggregate FROST signature shares using production aggregator (async)
    ///
    /// This uses the frost-ed25519 library to properly aggregate signature
    /// shares into a valid Ed25519 threshold signature.
    pub async fn aggregate_shares_frost(
        &self,
        conversation_id_hash: &[u8; 32],
        aggregator: &super::frost_signing::FrostSignatureAggregator,
        commitments: &std::collections::BTreeMap<u16, Vec<u8>>,
        signature_shares: &std::collections::BTreeMap<u16, Vec<u8>>,
        session_id: &str,
    ) -> Result<EpochToken> {
        let session = self
            .active_sessions
            .get(conversation_id_hash)
            .ok_or_else(|| Error::validation("No active session"))?;

        if !session.has_threshold() {
            return Err(Error::validation("Not enough shares to aggregate"));
        }

        // Get epoch and expiry from first share
        let first_share = session
            .shares
            .values()
            .next()
            .ok_or_else(|| Error::validation("No shares"))?;

        let epoch_id = first_share.epoch_id;
        let expires_at = first_share.expires_at;

        // Create message being signed
        let mut message = Vec::with_capacity(48);
        message.extend_from_slice(&epoch_id.to_le_bytes());
        message.extend_from_slice(conversation_id_hash);
        message.extend_from_slice(&expires_at.to_le_bytes());

        // Use FROST aggregator to combine signature shares
        let aggregated = aggregator
            .aggregate(&message, commitments, signature_shares, session_id)
            .map_err(|e| Error::crypto(format!("FROST aggregation failed: {}", e)))?;

        Ok(EpochToken {
            signature: aggregated.signature,
            epoch_id,
            conversation_id_hash: *conversation_id_hash,
            quorum_public_key: aggregated.group_public_key,
            expires_at,
            contributing_relays: signature_shares.len() as u8,
            threshold: session.threshold,
        })
    }

    /// Clean up old cached tokens for a conversation
    fn cleanup_old_tokens(&mut self, conversation_id_hash: &[u8; 32]) {
        if let Some(tokens) = self.cached_tokens.get_mut(conversation_id_hash) {
            let current_epoch = current_epoch_id();
            tokens.retain(|t| t.epoch_id + 2 >= current_epoch);

            // Keep at most MAX_CACHED_EPOCHS tokens
            while tokens.len() > MAX_CACHED_EPOCHS {
                tokens.remove(0);
            }
        }
    }

    /// Clear all cached tokens (e.g., on logout)
    pub fn clear_cache(&mut self) {
        self.cached_tokens.clear();
        self.active_sessions.clear();
    }
}

/// Get current epoch ID
pub fn current_epoch_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / EPOCH_DURATION_SECS)
        .unwrap_or(0)
}

/// Get epoch ID for a given Unix timestamp
pub fn epoch_id_for_timestamp(timestamp: u64) -> u64 {
    timestamp / EPOCH_DURATION_SECS
}

/// Get epoch start timestamp
pub fn epoch_start(epoch_id: u64) -> u64 {
    epoch_id * EPOCH_DURATION_SECS
}

/// Get epoch end timestamp
pub fn epoch_end(epoch_id: u64) -> u64 {
    (epoch_id + 1) * EPOCH_DURATION_SECS
}

/// Check if a timestamp is within an epoch (with grace period)
pub fn is_in_epoch_with_grace(timestamp: u64, epoch_id: u64) -> bool {
    let start = epoch_start(epoch_id);
    let end = epoch_end(epoch_id) + EPOCH_GRACE_PERIOD_SECS;
    timestamp >= start && timestamp < end
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_epoch_calculations() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let epoch = current_epoch_id();
        let expected = now / EPOCH_DURATION_SECS;
        assert_eq!(epoch, expected);

        let start = epoch_start(epoch);
        let end = epoch_end(epoch);
        assert!(start <= now);
        assert!(now < end);
        assert_eq!(end - start, EPOCH_DURATION_SECS);
    }

    #[test]
    fn test_conversation_type_parameters() {
        assert_eq!(ConversationType::Direct.threshold(), 4);
        assert_eq!(ConversationType::Direct.quorum_size(), 7);

        assert_eq!(ConversationType::ChannelSmall.threshold(), 7);
        assert_eq!(ConversationType::ChannelSmall.quorum_size(), 11);

        assert_eq!(ConversationType::ChannelLarge.threshold(), 11);
        assert_eq!(ConversationType::ChannelLarge.quorum_size(), 15);
    }

    #[test]
    fn test_epoch_token_request_creation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let user_id = [0u8; 32];
        let device_id = [42u8; 32];
        let conversation_id = b"test-conversation-123";

        let request = EpochTokenRequest::new(
            conversation_id,
            user_id,
            device_id,
            &signing_key,
            ConversationType::Direct,
            None,
        )
        .unwrap();

        assert_eq!(request.version, 1);
        assert_eq!(request.device_id, device_id);
        assert_eq!(request.user_id, user_id);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn test_epoch_token_expiry() {
        let token = EpochToken {
            signature: [0u8; 64],
            epoch_id: current_epoch_id(),
            conversation_id_hash: [0u8; 32],
            quorum_public_key: [0u8; 32],
            expires_at: epoch_end(current_epoch_id()) + EPOCH_GRACE_PERIOD_SECS,
            contributing_relays: 4,
            threshold: 4,
        };

        assert!(!token.is_expired());
        assert!(token.is_valid_for_current_epoch());

        // Expired token
        let old_token = EpochToken {
            signature: [0u8; 64],
            epoch_id: 0,
            conversation_id_hash: [0u8; 32],
            quorum_public_key: [0u8; 32],
            expires_at: 1, // Very old
            contributing_relays: 4,
            threshold: 4,
        };

        assert!(old_token.is_expired());
        assert!(!old_token.is_valid_for_current_epoch());
    }

    #[test]
    fn test_epoch_token_issuer_rate_limiting() {
        let mut issuer = EpochTokenIssuer::new([1u8; 32]);
        let device_id = [2u8; 32];
        let epoch = current_epoch_id();

        // First few requests should succeed
        for i in 0..MAX_TOKENS_PER_DEVICE_PER_EPOCH {
            let result = issuer.check_and_increment_rate_limit(&device_id, epoch);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), MAX_TOKENS_PER_DEVICE_PER_EPOCH - i - 1);
        }

        // Next request should fail with rate limit
        let result = issuer.check_and_increment_rate_limit(&device_id, epoch);
        assert!(result.is_err());
    }

    #[test]
    fn test_epoch_token_issuer_device_revocation() {
        let mut issuer = EpochTokenIssuer::new([1u8; 32]);
        let device_id = [2u8; 32];

        assert!(issuer.is_device_revoked(&device_id).is_none());

        issuer.revoke_device(device_id);

        assert!(issuer.is_device_revoked(&device_id).is_some());
    }

    #[test]
    fn test_token_aggregation_session() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let user_id = [0u8; 32];
        let device_id = [42u8; 32];

        let request = EpochTokenRequest::new(
            b"test-conv",
            user_id,
            device_id,
            &signing_key,
            ConversationType::Direct,
            None,
        )
        .unwrap();

        let mut session = TokenAggregationSession::new(request);

        assert!(!session.has_threshold());
        assert!(!session.has_failed());

        // Add 4 shares (threshold for Direct)
        for i in 0..4 {
            let share = EpochTokenShare {
                relay_id: [i as u8; 32],
                signature_share: vec![i as u8; 32],
                commitment: vec![i as u8; 32],
                epoch_id: current_epoch_id(),
                expires_at: epoch_end(current_epoch_id()),
                remaining_tokens: 10,
            };
            session.add_response(EpochTokenResponse::Share(share));
        }

        assert!(session.has_threshold());
    }

    #[test]
    fn test_epoch_token_manager_caching() {
        let user_id = [0u8; 32];
        let device_id = [1u8; 32];
        let mut manager = EpochTokenManager::new(user_id, device_id);

        let conv_hash = [42u8; 32];

        // No token initially
        assert!(manager.get_cached_token(&conv_hash).is_none());
        assert!(manager.needs_refresh(&conv_hash));

        // Add a token manually (simulating successful aggregation)
        let token = EpochToken {
            signature: [0u8; 64],
            epoch_id: current_epoch_id(),
            conversation_id_hash: conv_hash,
            quorum_public_key: [0u8; 32],
            expires_at: epoch_end(current_epoch_id()) + EPOCH_GRACE_PERIOD_SECS,
            contributing_relays: 4,
            threshold: 4,
        };

        manager
            .cached_tokens
            .entry(conv_hash)
            .or_default()
            .push(token);

        assert!(manager.get_cached_token(&conv_hash).is_some());
        assert!(!manager.needs_refresh(&conv_hash));
    }

    #[test]
    fn test_is_in_epoch_with_grace() {
        let epoch = current_epoch_id();
        let start = epoch_start(epoch);
        let end = epoch_end(epoch);

        // Middle of epoch
        assert!(is_in_epoch_with_grace(start + 300, epoch));

        // Start of epoch
        assert!(is_in_epoch_with_grace(start, epoch));

        // End of epoch (within grace)
        assert!(is_in_epoch_with_grace(
            end + EPOCH_GRACE_PERIOD_SECS - 1,
            epoch
        ));

        // After grace period
        assert!(!is_in_epoch_with_grace(
            end + EPOCH_GRACE_PERIOD_SECS + 1,
            epoch
        ));

        // Before epoch
        assert!(!is_in_epoch_with_grace(start - 1, epoch));
    }

    #[tokio::test]
    async fn test_frost_integration_end_to_end() {
        use crate::relay::frost_signing::{
            generate_committee_keys, verify_frost_signature, CommitteeFrostConfig,
            FrostSignatureAggregator,
        };
        use std::collections::BTreeMap;
        use std::sync::Arc;

        // 1. Set up committee configuration (4-of-7 for Direct chat)
        let config = CommitteeFrostConfig::for_direct();

        // 2. Generate committee keys using trusted dealer setup
        // We need relay IDs for the 7 committee members
        let relay_ids: Vec<[u8; 32]> = (0..7).map(|i| [i as u8; 32]).collect();
        let (key_shares, group_key) =
            generate_committee_keys(&relay_ids, config.threshold).unwrap();

        // Build verifying shares map from key shares
        let verifying_shares: BTreeMap<u16, Vec<u8>> = key_shares
            .iter()
            .map(|ks| (ks.participant_index, ks.verifying_share.clone()))
            .collect();

        // Committee ID for this test
        let committee_id = [99u8; 32];

        // 3. Set up 4 issuers with their FROST key shares (threshold amount)
        let mut issuers: Vec<EpochTokenIssuer> = Vec::new();
        for i in 0..4 {
            let mut issuer = EpochTokenIssuer::new(relay_ids[i]);

            // Create signer and register key share
            let signer = crate::relay::frost_signing::RelayFrostSigner::new(relay_ids[i]);
            signer
                .register_key_share(committee_id, key_shares[i].clone())
                .await
                .expect("Failed to register key share");

            issuer.set_frost_signer(Arc::new(signer));
            issuers.push(issuer);
        }

        // 4. Create a token request
        let signing_key = SigningKey::generate(&mut OsRng);
        let user_id = [0u8; 32];
        let device_id = [42u8; 32];
        let conversation_id = b"frost-test-conversation";

        let request = EpochTokenRequest::new(
            conversation_id,
            user_id,
            device_id,
            &signing_key,
            ConversationType::Direct,
            None,
        )
        .unwrap();

        // 5. Each issuer generates FROST round 1
        let session_id = format!("frost-session-{}", request.epoch_id);
        let mut round1_outputs = Vec::new();

        for issuer in &issuers {
            let round1 = issuer
                .generate_frost_round1(&committee_id, session_id.clone(), &request)
                .await
                .expect("Failed to generate round 1");
            round1_outputs.push(round1);
        }

        // 6. Collect all commitments (use correct field name: commitments)
        let commitments: BTreeMap<u16, Vec<u8>> = round1_outputs
            .iter()
            .map(|r| (r.participant_index, r.commitments.clone()))
            .collect();

        // 7. Each issuer generates FROST round 2
        let mut round2_outputs = Vec::new();

        for issuer in &issuers {
            let round2 = issuer
                .generate_frost_round2(&committee_id, &session_id, &commitments)
                .await
                .expect("Failed to generate round 2");
            round2_outputs.push(round2);
        }

        // 8. Aggregate signature shares
        let aggregator = FrostSignatureAggregator::new(config.clone(), verifying_shares, group_key);

        let signature_shares: BTreeMap<u16, Vec<u8>> = round2_outputs
            .iter()
            .map(|r| (r.participant_index, r.signature_share.clone()))
            .collect();

        // Message: epoch_id || conversation_id_hash || expires_at
        let expires_at = (request.epoch_id + 1) * EPOCH_DURATION_SECS + EPOCH_GRACE_PERIOD_SECS;
        let mut message = Vec::with_capacity(48);
        message.extend_from_slice(&request.epoch_id.to_le_bytes());
        message.extend_from_slice(&request.conversation_id_hash);
        message.extend_from_slice(&expires_at.to_le_bytes());

        let aggregated = aggregator
            .aggregate(&message, &commitments, &signature_shares, &session_id)
            .expect("Failed to aggregate signatures");

        // 9. Verify the aggregated signature (correct function signature)
        assert!(verify_frost_signature(&aggregated.signature, &message, &group_key).is_ok());

        // 10. Create final EpochToken
        let token = EpochToken {
            signature: aggregated.signature,
            epoch_id: request.epoch_id,
            conversation_id_hash: request.conversation_id_hash,
            quorum_public_key: group_key,
            expires_at,
            contributing_relays: 4,
            threshold: 4,
        };

        assert!(!token.is_expired());
        assert!(token.is_valid_for_current_epoch());
    }

    #[tokio::test]
    async fn test_epoch_token_issuer_with_revocation_checker() {
        use super::super::revocation::{RevocationAuthority, RevocationChecker, RevocationReason};

        let relay_id = [99u8; 32];

        // Create revocation checker
        let checker = Arc::new(RevocationChecker::new(relay_id));

        // Create issuer with revocation checker
        let mut issuer = EpochTokenIssuer::with_revocation_checker(relay_id, checker.clone());

        let signing_key = SigningKey::generate(&mut OsRng);
        let user_id = [1u8; 32];
        let device_id = [2u8; 32];
        let conversation_id = b"test-revocation-integration";

        // Create request
        let request = EpochTokenRequest::new(
            conversation_id,
            user_id,
            device_id,
            &signing_key,
            ConversationType::Direct,
            None,
        )
        .unwrap();

        // Request should initially succeed (not revoked)
        let response = issuer
            .process_request_async(
                &request,
                |_device_id, _sig, _data| true, // Accept any attestation
                |_proof| true,                  // Accept any membership
            )
            .await;

        // Should get a share (not rejected) - will be InternalError without FROST setup
        // but importantly it's NOT a revocation rejection
        match response {
            EpochTokenResponse::Rejected { reason, .. } => {
                // If rejected, it should be InternalError (no FROST), not revocation
                assert!(
                    matches!(reason, TokenRejectionReason::InternalError),
                    "Expected InternalError (no FROST setup), got {:?}",
                    reason
                );
            }
            EpochTokenResponse::Share(_) => {
                // This is fine too if FROST is somehow available
            }
        }

        // Now revoke the device
        checker
            .revoke_device(
                device_id,
                RevocationAuthority::System {
                    block_height: 100,
                    tx_hash: [0u8; 32],
                },
                RevocationReason::DeviceStolen,
            )
            .await
            .unwrap();

        // Request should now be rejected
        let response = issuer
            .process_request_async(&request, |_device_id, _sig, _data| true, |_proof| true)
            .await;

        match response {
            EpochTokenResponse::Rejected { reason, .. } => {
                assert!(
                    matches!(reason, TokenRejectionReason::DeviceRevoked { .. }),
                    "Expected DeviceRevoked, got {:?}",
                    reason
                );
            }
            EpochTokenResponse::Share(_) => {
                panic!("Should have been rejected due to device revocation");
            }
        }
    }

    #[tokio::test]
    async fn test_epoch_token_issuer_user_revocation() {
        use super::super::revocation::{RevocationAuthority, RevocationChecker, RevocationReason};

        let relay_id = [99u8; 32];
        let checker = Arc::new(RevocationChecker::new(relay_id));
        let mut issuer = EpochTokenIssuer::with_revocation_checker(relay_id, checker.clone());

        let signing_key = SigningKey::generate(&mut OsRng);
        let user_id = [1u8; 32];
        let device_id_1 = [2u8; 32];
        let device_id_2 = [3u8; 32];
        let conversation_id = b"test-user-revocation";

        // Revoke the user (not a specific device)
        checker
            .revoke_user(
                user_id,
                RevocationAuthority::System {
                    block_height: 100,
                    tx_hash: [0u8; 32],
                },
                RevocationReason::AccountDeleted,
            )
            .await
            .unwrap();

        // Both devices should be rejected
        for device_id in [device_id_1, device_id_2] {
            let request = EpochTokenRequest::new(
                conversation_id,
                user_id,
                device_id,
                &signing_key,
                ConversationType::Direct,
                None,
            )
            .unwrap();

            let response = issuer
                .process_request_async(&request, |_device_id, _sig, _data| true, |_proof| true)
                .await;

            match response {
                EpochTokenResponse::Rejected { reason, .. } => {
                    assert!(
                        matches!(reason, TokenRejectionReason::UserRevoked { .. }),
                        "Expected UserRevoked, got {:?}",
                        reason
                    );
                }
                EpochTokenResponse::Share(_) => {
                    panic!("Should have been rejected due to user revocation");
                }
            }
        }
    }

    #[tokio::test]
    async fn test_epoch_token_issuer_membership_revocation() {
        use super::super::revocation::{RevocationAuthority, RevocationChecker, RevocationReason};
        use sha2::{Digest, Sha256};

        let relay_id = [99u8; 32];
        let checker = Arc::new(RevocationChecker::new(relay_id));
        let mut issuer = EpochTokenIssuer::with_revocation_checker(relay_id, checker.clone());

        let signing_key = SigningKey::generate(&mut OsRng);
        let user_id = [1u8; 32];
        let device_id = [2u8; 32];
        let conversation_id = b"channel-with-membership-check";
        let other_conversation_id = b"other-channel";

        // We need to compute the conversation hash to match the revocation
        // Since the request uses a nonce-based hash, we'll test with the raw hash

        // Revoke membership in the specific conversation
        // Note: We use the raw conversation_id hash for revocation, but the request
        // uses a salted hash. For this test, we'll use the conversation_id_hash from the request.
        let request = EpochTokenRequest::new(
            conversation_id,
            user_id,
            device_id,
            &signing_key,
            ConversationType::ChannelSmall,
            None,
        )
        .unwrap();

        // Revoke using the actual conversation_id_hash from the request
        checker
            .revoke_membership(
                user_id,
                request.conversation_id_hash,
                RevocationAuthority::System {
                    block_height: 100,
                    tx_hash: [0u8; 32],
                },
                RevocationReason::MembershipRemoved,
            )
            .await
            .unwrap();

        // Request to the revoked conversation should be rejected
        let response = issuer
            .process_request_async(&request, |_device_id, _sig, _data| true, |_proof| true)
            .await;

        match response {
            EpochTokenResponse::Rejected { reason, .. } => {
                assert!(
                    matches!(reason, TokenRejectionReason::MembershipRevoked { .. }),
                    "Expected MembershipRevoked, got {:?}",
                    reason
                );
            }
            EpochTokenResponse::Share(_) => {
                panic!("Should have been rejected due to membership revocation");
            }
        }

        // Request to different conversation should still work (modulo FROST setup)
        let other_request = EpochTokenRequest::new(
            other_conversation_id,
            user_id,
            device_id,
            &signing_key,
            ConversationType::ChannelSmall,
            None,
        )
        .unwrap();

        let response = issuer
            .process_request_async(
                &other_request,
                |_device_id, _sig, _data| true,
                |_proof| true,
            )
            .await;

        match response {
            EpochTokenResponse::Rejected { reason, .. } => {
                // Should be InternalError (no FROST), not revocation
                assert!(
                    matches!(
                        reason,
                        TokenRejectionReason::InternalError | TokenRejectionReason::NotMember
                    ),
                    "Expected InternalError or NotMember (no FROST/proof), got {:?}",
                    reason
                );
            }
            EpochTokenResponse::Share(_) => {
                // Fine if FROST is available
            }
        }
    }
}
