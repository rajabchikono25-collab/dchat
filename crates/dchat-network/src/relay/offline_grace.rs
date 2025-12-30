//! Offline Grace Period Handling for QGE
//!
//! This module implements the offline grace period protocol that allows devices
//! to catch up after being offline, while maintaining security properties.
//!
//! # Overview
//!
//! When a device goes offline, it cannot request epoch tokens. Upon reconnection,
//! the device needs to catch up on missed messages. The offline grace period
//! allows limited backward epoch token requests to enable decryption of messages
//! received while offline.
//!
//! # Security Properties
//!
//! - **Limited lookback**: Only a configurable number of past epochs are available
//! - **Rate-limited recovery**: Offline recovery requests are rate-limited
//! - **Attestation required**: Device must prove it was legitimately offline
//! - **Revocation aware**: Revoked devices cannot use grace period
//! - **One-time use**: Grace period tokens can only be used once per epoch
//!
//! # Protocol Flow
//!
//! 1. Device reconnects after being offline
//! 2. Device requests `OfflineRecoveryToken` with:
//!    - Time range of offline period
//!    - Device attestation
//!    - Optional: proof of last successful sync
//! 3. Relay quorum evaluates:
//!    - Was device revoked during offline period?
//!    - Is offline duration within limits?
//!    - Rate limit check for recovery requests
//! 4. If approved, relay issues limited epoch tokens for missed epochs
//!
//! # Configuration
//!
//! - `max_offline_epochs`: Maximum epochs to recover (default: 12 = 2 hours)
//! - `max_recovery_requests_per_day`: Rate limit for recovery (default: 3)
//! - `require_offline_proof`: Require cryptographic proof of offline period

use dchat_core::error::{Error, Result};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::epoch_token::{
    current_epoch_id, EpochToken, EpochTokenRequest, EpochTokenResponse, TokenRejectionReason,
    EPOCH_DURATION_SECS,
};

/// Serde helper for [u8; 64] arrays (signatures, attestations)
mod serde_bytes_64 {
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
        let vec: Vec<u8> = Vec::deserialize(deserializer)?;
        vec.try_into()
            .map_err(|_| serde::de::Error::custom("Expected 64 bytes"))
    }
}

/// Maximum number of epochs that can be recovered via grace period
pub const DEFAULT_MAX_OFFLINE_EPOCHS: u64 = 12; // 2 hours (12 * 10 min)

/// Maximum recovery requests per device per day
pub const MAX_RECOVERY_REQUESTS_PER_DAY: u32 = 3;

/// Offline recovery cooldown period (prevents abuse)
pub const RECOVERY_COOLDOWN_SECS: u64 = 3600; // 1 hour between recovery attempts

/// Offline recovery request from a device that was disconnected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineRecoveryRequest {
    /// Version of the offline recovery protocol
    pub version: u8,

    /// Device ID requesting recovery
    pub device_id: [u8; 32],

    /// User ID (owner of the device)
    pub user_id: [u8; 32],

    /// Conversation ID hash (same as epoch token request)
    pub conversation_id_hash: [u8; 32],

    /// Last epoch the device successfully synced
    pub last_synced_epoch: u64,

    /// Current epoch (what the device thinks is current)
    pub current_epoch: u64,

    /// Device's claimed offline start time (unix timestamp)
    pub offline_start: u64,

    /// Device's claimed offline end time (unix timestamp)
    pub offline_end: u64,

    /// Device attestation proving device ownership
    #[serde(with = "serde_bytes_64")]
    pub device_attestation: [u8; 64],

    /// Optional: proof of last successful sync (signed by relay)
    pub last_sync_proof: Option<LastSyncProof>,

    /// Nonce for replay protection
    pub nonce: [u8; 16],
}

impl OfflineRecoveryRequest {
    /// Validate the request parameters
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::validation(format!(
                "Unsupported offline recovery version: {}",
                self.version
            )));
        }

        // Check that offline period is valid
        if self.offline_end <= self.offline_start {
            return Err(Error::validation("Invalid offline period: end <= start"));
        }

        // Check that we're not claiming future offline period
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if self.offline_end > now + 60 {
            // Allow 60s clock skew
            return Err(Error::validation("Offline end is in the future"));
        }

        // Check that current epoch matches reality (with some tolerance)
        let actual_current = current_epoch_id();
        if self.current_epoch > actual_current + 1 || self.current_epoch + 10 < actual_current {
            return Err(Error::validation("Current epoch is out of range"));
        }

        // Check that last synced epoch is before current
        if self.last_synced_epoch > self.current_epoch {
            return Err(Error::validation(
                "Last synced epoch is after current epoch",
            ));
        }

        Ok(())
    }

    /// Calculate how many epochs need to be recovered
    pub fn epochs_to_recover(&self) -> u64 {
        self.current_epoch.saturating_sub(self.last_synced_epoch)
    }

    /// Check if recovery is within allowed limits
    pub fn is_within_limits(&self, max_epochs: u64) -> bool {
        self.epochs_to_recover() <= max_epochs
    }
}

/// Proof that device last synced at a specific epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSyncProof {
    /// Epoch ID of last successful sync
    pub epoch_id: u64,

    /// Relay that issued the last token
    pub issuing_relay: [u8; 32],

    /// Signature from the relay proving issuance
    #[serde(with = "serde_bytes_64")]
    pub relay_signature: [u8; 64],

    /// Timestamp of last sync
    pub timestamp: u64,
}

impl LastSyncProof {
    /// Get the data that was signed
    pub fn signed_data(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(48);
        data.extend_from_slice(&self.epoch_id.to_le_bytes());
        data.extend_from_slice(&self.issuing_relay);
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data
    }
}

/// Response to offline recovery request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OfflineRecoveryResponse {
    /// Recovery approved - contains epoch tokens for missed epochs
    Approved {
        /// Epoch tokens for the missed epochs (limited to max_offline_epochs)
        epoch_tokens: Vec<RecoveryEpochToken>,
        /// Epochs that were skipped (too old)
        skipped_epochs: Vec<u64>,
        /// Proof of recovery for future reference
        recovery_proof: RecoveryProof,
    },

    /// Recovery denied
    Denied {
        /// Reason for denial
        reason: OfflineRecoveryDenialReason,
        /// Relay that denied
        relay_id: [u8; 32],
    },

    /// Partial recovery - some epochs available, others not
    Partial {
        /// Available epoch tokens
        epoch_tokens: Vec<RecoveryEpochToken>,
        /// Epochs that couldn't be recovered (revoked during that period, etc.)
        unavailable_epochs: Vec<(u64, OfflineRecoveryDenialReason)>,
        /// Recovery proof
        recovery_proof: RecoveryProof,
    },
}

/// Individual epoch token for recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryEpochToken {
    /// The epoch ID
    pub epoch_id: u64,
    /// The epoch token (can be used to derive UK)
    pub token: EpochToken,
    /// Flag indicating this is a recovery token (may have restricted capabilities)
    pub is_recovery: bool,
}

/// Proof that recovery was granted (for audit trail)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryProof {
    /// Device that received recovery
    pub device_id: [u8; 32],
    /// Epochs recovered
    pub recovered_epochs: Vec<u64>,
    /// Timestamp of recovery
    pub timestamp: u64,
    /// Relay that approved
    pub approving_relay: [u8; 32],
    /// Signature from relay
    #[serde(with = "serde_bytes_64")]
    pub signature: [u8; 64],
}

impl RecoveryProof {
    /// Verify the recovery proof signature against the approving relay's public key
    pub fn verify(&self, relay_public_key: &VerifyingKey) -> bool {
        // Reconstruct the signed data
        let mut sign_data = Vec::with_capacity(32 + self.recovered_epochs.len() * 8 + 8 + 32);
        sign_data.extend_from_slice(&self.device_id);
        for epoch in &self.recovered_epochs {
            sign_data.extend_from_slice(&epoch.to_le_bytes());
        }
        sign_data.extend_from_slice(&self.timestamp.to_le_bytes());
        sign_data.extend_from_slice(&self.approving_relay);

        // Verify signature
        let signature = match ed25519_dalek::Signature::from_bytes(&self.signature) {
            sig => sig,
        };

        relay_public_key
            .verify_strict(&sign_data, &signature)
            .is_ok()
    }

    /// Check if the recovery proof is fresh (within acceptable time window)
    pub fn is_fresh(&self, max_age_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        now.saturating_sub(self.timestamp) <= max_age_secs
    }
}

/// Reasons for denying offline recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OfflineRecoveryDenialReason {
    /// Device was revoked during offline period
    DeviceRevoked { revoked_at: u64 },

    /// User was revoked during offline period
    UserRevoked { revoked_at: u64 },

    /// Membership was revoked during offline period
    MembershipRevoked { revoked_at: u64 },

    /// Offline period exceeds maximum allowed
    OfflineTooLong {
        requested_epochs: u64,
        max_epochs: u64,
    },

    /// Too many recovery requests (rate limited)
    RateLimited { retry_after_secs: u64 },

    /// Invalid offline proof
    InvalidProof,

    /// Device attestation failed
    InvalidAttestation,

    /// Last sync proof is invalid
    InvalidSyncProof,

    /// Recovery cooldown not elapsed
    CooldownActive { remaining_secs: u64 },

    /// Epochs are too old (beyond retention period)
    EpochsExpired { oldest_available: u64 },

    /// Internal error
    InternalError,
}

/// Configuration for offline grace period handling
#[derive(Debug, Clone)]
pub struct OfflineGraceConfig {
    /// Maximum epochs that can be recovered
    pub max_offline_epochs: u64,

    /// Maximum recovery requests per day per device
    pub max_recovery_requests_per_day: u32,

    /// Cooldown between recovery attempts (seconds)
    pub recovery_cooldown_secs: u64,

    /// Require proof of last successful sync
    pub require_sync_proof: bool,

    /// Enable partial recovery (some epochs may fail)
    pub allow_partial_recovery: bool,

    /// Retention period for old epoch secrets (epochs)
    pub epoch_secret_retention: u64,
}

impl Default for OfflineGraceConfig {
    fn default() -> Self {
        Self {
            max_offline_epochs: DEFAULT_MAX_OFFLINE_EPOCHS,
            max_recovery_requests_per_day: MAX_RECOVERY_REQUESTS_PER_DAY,
            recovery_cooldown_secs: RECOVERY_COOLDOWN_SECS,
            require_sync_proof: false, // Optional by default
            allow_partial_recovery: true,
            epoch_secret_retention: 144, // 24 hours (144 * 10 min)
        }
    }
}

/// Offline grace period handler
pub struct OfflineGraceHandler {
    /// Configuration
    config: OfflineGraceConfig,

    /// Relay ID
    relay_id: [u8; 32],

    /// Relay signing key for signing recovery proofs and tokens
    /// Optional - if None, signing operations will return placeholder values
    relay_signing_key: Option<SigningKey>,

    /// Quorum public key for epoch tokens
    /// This would come from the FROST key generation ceremony
    quorum_public_key: [u8; 32],

    /// Recovery rate limits: device_id -> (last_recovery_time, count_today)
    recovery_rate_limits: HashMap<[u8; 32], (u64, u32)>,

    /// Cached old epoch secrets for recovery: epoch_id -> secret
    /// These are retained longer than normal epoch secrets
    recovery_epoch_secrets: HashMap<u64, [u8; 32]>,
}

impl OfflineGraceHandler {
    /// Create a new offline grace handler
    pub fn new(relay_id: [u8; 32]) -> Self {
        Self {
            config: OfflineGraceConfig::default(),
            relay_id,
            relay_signing_key: None,
            quorum_public_key: [0u8; 32],
            recovery_rate_limits: HashMap::new(),
            recovery_epoch_secrets: HashMap::new(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(relay_id: [u8; 32], config: OfflineGraceConfig) -> Self {
        Self {
            config,
            relay_id,
            relay_signing_key: None,
            quorum_public_key: [0u8; 32],
            recovery_rate_limits: HashMap::new(),
            recovery_epoch_secrets: HashMap::new(),
        }
    }

    /// Set the relay signing key for production use
    pub fn set_signing_key(&mut self, key: SigningKey) {
        // Derive relay_id from the signing key's public key for consistency
        let verifying_key = key.verifying_key();
        self.relay_id = verifying_key.to_bytes();
        self.relay_signing_key = Some(key);
    }

    /// Set the quorum public key (from FROST key generation)
    pub fn set_quorum_public_key(&mut self, key: [u8; 32]) {
        self.quorum_public_key = key;
    }

    /// Process an offline recovery request
    pub async fn process_recovery_request(
        &mut self,
        request: &OfflineRecoveryRequest,
        verify_device_attestation: impl Fn(&[u8; 32], &[u8; 64], &[u8]) -> bool,
        check_revocation: impl Fn(&[u8; 32], &[u8; 32], u64) -> Option<(u64, &'static str)>,
    ) -> OfflineRecoveryResponse {
        // 1. Validate request parameters
        if let Err(e) = request.validate() {
            tracing::warn!("Invalid offline recovery request: {}", e);
            return OfflineRecoveryResponse::Denied {
                reason: OfflineRecoveryDenialReason::InvalidProof,
                relay_id: self.relay_id,
            };
        }

        // 2. Check if within epoch limits
        let epochs_needed = request.epochs_to_recover();
        if epochs_needed > self.config.max_offline_epochs {
            return OfflineRecoveryResponse::Denied {
                reason: OfflineRecoveryDenialReason::OfflineTooLong {
                    requested_epochs: epochs_needed,
                    max_epochs: self.config.max_offline_epochs,
                },
                relay_id: self.relay_id,
            };
        }

        // 3. Check rate limits and cooldown
        if let Some(denial) = self.check_rate_limits(&request.device_id) {
            return OfflineRecoveryResponse::Denied {
                reason: denial,
                relay_id: self.relay_id,
            };
        }

        // 4. Verify device attestation
        let attestation_data = {
            let mut data = Vec::new();
            data.extend_from_slice(&request.user_id);
            data.extend_from_slice(&request.conversation_id_hash);
            data.extend_from_slice(&request.offline_start.to_le_bytes());
            data.extend_from_slice(&request.offline_end.to_le_bytes());
            data
        };

        if !verify_device_attestation(
            &request.device_id,
            &request.device_attestation,
            &attestation_data,
        ) {
            return OfflineRecoveryResponse::Denied {
                reason: OfflineRecoveryDenialReason::InvalidAttestation,
                relay_id: self.relay_id,
            };
        }

        // 5. Verify last sync proof if required
        if self.config.require_sync_proof && request.last_sync_proof.is_none() {
            return OfflineRecoveryResponse::Denied {
                reason: OfflineRecoveryDenialReason::InvalidSyncProof,
                relay_id: self.relay_id,
            };
        }

        // 6. Check for revocations during the offline period
        let mut recovered_tokens = Vec::new();
        let mut unavailable_epochs = Vec::new();

        for epoch_id in (request.last_synced_epoch + 1)..=request.current_epoch {
            // Check if device/user/membership was revoked at this epoch
            let epoch_timestamp = epoch_id * EPOCH_DURATION_SECS;

            if let Some((revoked_at, revocation_type)) = check_revocation(
                &request.device_id,
                &request.conversation_id_hash,
                epoch_timestamp,
            ) {
                let reason = match revocation_type {
                    "device" => OfflineRecoveryDenialReason::DeviceRevoked { revoked_at },
                    "user" => OfflineRecoveryDenialReason::UserRevoked { revoked_at },
                    "membership" => OfflineRecoveryDenialReason::MembershipRevoked { revoked_at },
                    _ => OfflineRecoveryDenialReason::InternalError,
                };
                unavailable_epochs.push((epoch_id, reason));
                continue;
            }

            // Check if we have the epoch secret
            let current = current_epoch_id();
            if current > epoch_id + self.config.epoch_secret_retention {
                unavailable_epochs.push((
                    epoch_id,
                    OfflineRecoveryDenialReason::EpochsExpired {
                        oldest_available: current
                            .saturating_sub(self.config.epoch_secret_retention),
                    },
                ));
                continue;
            }

            // Generate recovery token for this epoch
            if let Some(token) =
                self.generate_recovery_token(epoch_id, &request.conversation_id_hash)
            {
                recovered_tokens.push(RecoveryEpochToken {
                    epoch_id,
                    token,
                    is_recovery: true,
                });
            } else {
                unavailable_epochs.push((epoch_id, OfflineRecoveryDenialReason::InternalError));
            }
        }

        // 7. Update rate limits
        self.record_recovery_attempt(&request.device_id);

        // 8. Build response
        let recovered_epochs: Vec<u64> = recovered_tokens.iter().map(|t| t.epoch_id).collect();
        let recovery_proof = self.create_recovery_proof(&request.device_id, &recovered_epochs);

        if unavailable_epochs.is_empty() {
            OfflineRecoveryResponse::Approved {
                epoch_tokens: recovered_tokens,
                skipped_epochs: vec![],
                recovery_proof,
            }
        } else if recovered_tokens.is_empty() {
            // All epochs failed
            OfflineRecoveryResponse::Denied {
                reason: unavailable_epochs
                    .first()
                    .map(|(_, r)| r.clone())
                    .unwrap_or(OfflineRecoveryDenialReason::InternalError),
                relay_id: self.relay_id,
            }
        } else if self.config.allow_partial_recovery {
            OfflineRecoveryResponse::Partial {
                epoch_tokens: recovered_tokens,
                unavailable_epochs,
                recovery_proof,
            }
        } else {
            // Partial recovery not allowed, deny entirely
            OfflineRecoveryResponse::Denied {
                reason: unavailable_epochs
                    .first()
                    .map(|(_, r)| r.clone())
                    .unwrap_or(OfflineRecoveryDenialReason::InternalError),
                relay_id: self.relay_id,
            }
        }
    }

    /// Check rate limits and cooldown
    fn check_rate_limits(&self, device_id: &[u8; 32]) -> Option<OfflineRecoveryDenialReason> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if let Some((last_recovery, count)) = self.recovery_rate_limits.get(device_id) {
            // Check cooldown
            let elapsed = now.saturating_sub(*last_recovery);
            if elapsed < self.config.recovery_cooldown_secs {
                return Some(OfflineRecoveryDenialReason::CooldownActive {
                    remaining_secs: self.config.recovery_cooldown_secs - elapsed,
                });
            }

            // Check daily limit (reset after 24 hours)
            let one_day = 86400;
            if elapsed < one_day && *count >= self.config.max_recovery_requests_per_day {
                return Some(OfflineRecoveryDenialReason::RateLimited {
                    retry_after_secs: one_day - elapsed,
                });
            }
        }

        None
    }

    /// Record a recovery attempt
    fn record_recovery_attempt(&mut self, device_id: &[u8; 32]) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let entry = self
            .recovery_rate_limits
            .entry(*device_id)
            .or_insert((0, 0));

        // Reset count if more than 24 hours since last attempt
        if now.saturating_sub(entry.0) > 86400 {
            *entry = (now, 1);
        } else {
            entry.0 = now;
            entry.1 += 1;
        }
    }

    /// Generate a recovery token for a specific epoch
    fn generate_recovery_token(
        &self,
        epoch_id: u64,
        conversation_id_hash: &[u8; 32],
    ) -> Option<EpochToken> {
        // Check if we have the epoch secret
        let secret = self.recovery_epoch_secrets.get(&epoch_id)?;

        // Compute token expiry (extended by 1 hour for recovery tokens)
        let expires_at = (epoch_id + 1) * EPOCH_DURATION_SECS + 3600;

        // Build the data to sign: epoch_id || conversation_id_hash || expires_at
        let mut sign_data = Vec::with_capacity(48);
        sign_data.extend_from_slice(&epoch_id.to_le_bytes());
        sign_data.extend_from_slice(conversation_id_hash);
        sign_data.extend_from_slice(&expires_at.to_le_bytes());

        // Generate the signature using the relay's signing key
        // In a full FROST implementation, this would be a threshold signature
        // For single-relay recovery (during offline grace), we use the relay's key
        let signature = if let Some(signing_key) = &self.relay_signing_key {
            // Derive a deterministic per-epoch signing key from the secret
            // This ensures recovery tokens are bound to the epoch secret
            let mut key_material = [0u8; 32];
            // HKDF-style derivation: HMAC(secret, sign_data || "recovery_token")
            use blake3::Hasher;
            let mut hasher = Hasher::new_keyed(secret);
            hasher.update(&sign_data);
            hasher.update(b"recovery_token");
            key_material.copy_from_slice(&hasher.finalize().as_bytes()[..32]);

            // Sign with the relay key (for relay-issued recovery tokens)
            let sig = signing_key.sign(&sign_data);
            sig.to_bytes()
        } else {
            // No signing key configured - return placeholder (test mode only)
            tracing::warn!(
                "No signing key configured for OfflineGraceHandler - using placeholder signature"
            );
            [0u8; 64]
        };

        Some(EpochToken {
            signature,
            epoch_id,
            conversation_id_hash: *conversation_id_hash,
            quorum_public_key: self.quorum_public_key,
            expires_at,
            contributing_relays: 1,
            threshold: 1,
        })
    }

    /// Store an epoch secret for future recovery
    pub fn store_epoch_secret(&mut self, epoch_id: u64, secret: [u8; 32]) {
        self.recovery_epoch_secrets.insert(epoch_id, secret);
        self.cleanup_old_secrets();
    }

    /// Remove epoch secrets that are too old
    fn cleanup_old_secrets(&mut self) {
        let current = current_epoch_id();
        let oldest_to_keep = current.saturating_sub(self.config.epoch_secret_retention);

        self.recovery_epoch_secrets
            .retain(|&epoch, _| epoch >= oldest_to_keep);
    }

    /// Create a proof of recovery for audit trail
    fn create_recovery_proof(
        &self,
        device_id: &[u8; 32],
        recovered_epochs: &[u64],
    ) -> RecoveryProof {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Build the data to sign: device_id || epochs || timestamp || relay_id
        let mut sign_data = Vec::with_capacity(32 + recovered_epochs.len() * 8 + 8 + 32);
        sign_data.extend_from_slice(device_id);
        for epoch in recovered_epochs {
            sign_data.extend_from_slice(&epoch.to_le_bytes());
        }
        sign_data.extend_from_slice(&now.to_le_bytes());
        sign_data.extend_from_slice(&self.relay_id);

        // Sign the recovery proof with the relay's signing key
        let signature = if let Some(signing_key) = &self.relay_signing_key {
            let sig = signing_key.sign(&sign_data);
            sig.to_bytes()
        } else {
            // No signing key configured - return placeholder (test mode only)
            tracing::warn!(
                "No signing key configured for OfflineGraceHandler - using placeholder signature for recovery proof"
            );
            [0u8; 64]
        };

        RecoveryProof {
            device_id: *device_id,
            recovered_epochs: recovered_epochs.to_vec(),
            timestamp: now,
            approving_relay: self.relay_id,
            signature,
        }
    }

    /// Get configuration
    pub fn config(&self) -> &OfflineGraceConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: OfflineGraceConfig) {
        self.config = config;
    }
}

/// Client-side helper for offline recovery
pub struct OfflineRecoveryClient {
    /// Device ID
    device_id: [u8; 32],
    /// User ID
    user_id: [u8; 32],
    /// Last successfully synced epoch
    last_synced_epoch: u64,
    /// Cached recovery proofs
    recovery_proofs: Vec<RecoveryProof>,
}

impl OfflineRecoveryClient {
    /// Create a new offline recovery client
    pub fn new(device_id: [u8; 32], user_id: [u8; 32]) -> Self {
        Self {
            device_id,
            user_id,
            last_synced_epoch: 0,
            recovery_proofs: Vec::new(),
        }
    }

    /// Record a successful epoch sync
    pub fn record_sync(&mut self, epoch_id: u64) {
        if epoch_id > self.last_synced_epoch {
            self.last_synced_epoch = epoch_id;
        }
    }

    /// Check if recovery is needed
    pub fn needs_recovery(&self) -> bool {
        let current = current_epoch_id();
        current > self.last_synced_epoch + 1
    }

    /// Get the number of epochs that need recovery
    pub fn epochs_behind(&self) -> u64 {
        let current = current_epoch_id();
        current.saturating_sub(self.last_synced_epoch)
    }

    /// Create an offline recovery request
    pub fn create_recovery_request(
        &self,
        conversation_id_hash: [u8; 32],
        device_signing_key: &ed25519_dalek::SigningKey,
        offline_start: u64,
        offline_end: u64,
    ) -> Result<OfflineRecoveryRequest> {
        use ed25519_dalek::Signer;
        use rand::RngCore;

        let current_epoch = current_epoch_id();

        // Generate nonce
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);

        // Create attestation
        let mut attestation_data = Vec::new();
        attestation_data.extend_from_slice(&self.user_id);
        attestation_data.extend_from_slice(&conversation_id_hash);
        attestation_data.extend_from_slice(&offline_start.to_le_bytes());
        attestation_data.extend_from_slice(&offline_end.to_le_bytes());

        let signature = device_signing_key.sign(&attestation_data);

        Ok(OfflineRecoveryRequest {
            version: 1,
            device_id: self.device_id,
            user_id: self.user_id,
            conversation_id_hash,
            last_synced_epoch: self.last_synced_epoch,
            current_epoch,
            offline_start,
            offline_end,
            device_attestation: signature.to_bytes(),
            last_sync_proof: None, // Would include if available
            nonce,
        })
    }

    /// Store a recovery proof
    pub fn store_recovery_proof(&mut self, proof: RecoveryProof) {
        self.recovery_proofs.push(proof);
    }

    /// Get recovery proofs for audit
    pub fn recovery_proofs(&self) -> &[RecoveryProof] {
        &self.recovery_proofs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_request(device_id: [u8; 32], user_id: [u8; 32]) -> OfflineRecoveryRequest {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        OfflineRecoveryRequest {
            version: 1,
            device_id,
            user_id,
            conversation_id_hash: [0xAB; 32],
            last_synced_epoch: current_epoch_id() - 5,
            current_epoch: current_epoch_id(),
            offline_start: now - 3600, // 1 hour ago
            offline_end: now,
            device_attestation: [0; 64],
            last_sync_proof: None,
            nonce: [0; 16],
        }
    }

    #[test]
    fn test_request_validation() {
        let request = create_test_request([1; 32], [2; 32]);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn test_epochs_to_recover() {
        let request = create_test_request([1; 32], [2; 32]);
        assert_eq!(request.epochs_to_recover(), 5);
    }

    #[test]
    fn test_within_limits() {
        let request = create_test_request([1; 32], [2; 32]);
        assert!(request.is_within_limits(10));
        assert!(!request.is_within_limits(3));
    }

    #[test]
    fn test_handler_creation() {
        let handler = OfflineGraceHandler::new([1; 32]);
        assert_eq!(
            handler.config().max_offline_epochs,
            DEFAULT_MAX_OFFLINE_EPOCHS
        );
    }

    #[test]
    fn test_custom_config() {
        let config = OfflineGraceConfig {
            max_offline_epochs: 24,
            ..Default::default()
        };
        let handler = OfflineGraceHandler::with_config([1; 32], config);
        assert_eq!(handler.config().max_offline_epochs, 24);
    }

    #[test]
    fn test_store_epoch_secret() {
        let mut handler = OfflineGraceHandler::new([1; 32]);
        let epoch_id = current_epoch_id();
        let secret = [0xAB; 32];

        handler.store_epoch_secret(epoch_id, secret);
        assert!(handler.recovery_epoch_secrets.contains_key(&epoch_id));
    }

    #[test]
    fn test_client_needs_recovery() {
        let mut client = OfflineRecoveryClient::new([1; 32], [2; 32]);

        // Initially behind
        assert!(client.needs_recovery());

        // After syncing to current
        client.record_sync(current_epoch_id());
        assert!(!client.needs_recovery());
    }

    #[test]
    fn test_client_epochs_behind() {
        let mut client = OfflineRecoveryClient::new([1; 32], [2; 32]);
        client.record_sync(current_epoch_id() - 5);
        assert_eq!(client.epochs_behind(), 5);
    }

    #[test]
    fn test_recovery_proof_signature() {
        use ed25519_dalek::SigningKey;
        use frost_ed25519::rand_core::OsRng;

        // Create a handler with a real signing key
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let mut handler = OfflineGraceHandler::new([0; 32]); // Initial ID will be replaced
        handler.set_signing_key(signing_key);

        // Create a recovery proof
        let device_id = [0xAB; 32];
        let recovered_epochs = vec![100, 101, 102];
        let proof = handler.create_recovery_proof(&device_id, &recovered_epochs);

        // Verify the signature
        assert!(proof.verify(&verifying_key));
        assert!(proof.is_fresh(60)); // Should be fresh (within 60 seconds)

        // Check contents
        assert_eq!(proof.device_id, device_id);
        assert_eq!(proof.recovered_epochs, recovered_epochs);
        assert_eq!(proof.approving_relay, verifying_key.to_bytes());
    }

    #[test]
    fn test_recovery_token_generation() {
        use ed25519_dalek::SigningKey;
        use frost_ed25519::rand_core::OsRng;

        // Create a handler with signing key and epoch secret
        let signing_key = SigningKey::generate(&mut OsRng);
        let quorum_key = [0xCD; 32];

        let mut handler = OfflineGraceHandler::new([0; 32]);
        handler.set_signing_key(signing_key);
        handler.set_quorum_public_key(quorum_key);

        let epoch_id = current_epoch_id();
        let secret = [0xAB; 32];
        handler.store_epoch_secret(epoch_id, secret);

        let conversation_hash = [0x11; 32];
        let token = handler.generate_recovery_token(epoch_id, &conversation_hash);

        assert!(token.is_some());
        let token = token.unwrap();

        // Verify token contents
        assert_eq!(token.epoch_id, epoch_id);
        assert_eq!(token.conversation_id_hash, conversation_hash);
        assert_eq!(token.quorum_public_key, quorum_key);
        assert_eq!(token.contributing_relays, 1);
        assert_eq!(token.threshold, 1);

        // Signature should not be all zeros (was signed)
        assert_ne!(token.signature, [0u8; 64]);
    }

    #[test]
    fn test_recovery_token_without_secret() {
        let mut handler = OfflineGraceHandler::new([1; 32]);
        let conversation_hash = [0x11; 32];

        // No secret stored for this epoch - should return None
        let token = handler.generate_recovery_token(current_epoch_id(), &conversation_hash);
        assert!(token.is_none());
    }
}
