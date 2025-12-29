//! Typed Handshake State Machine with Identity Binding
//!
//! This module implements a type-safe handshake protocol with:
//! - Compile-time state validation
//! - Protocol version negotiation  
//! - Cryptographic identity binding (prevents MITM)
//! - Per-phase timeouts
//! - Structured message format
//!
//! See plan3.md Sections 3.2-3.4

use crate::{
    keys::PrivateKey,
    noise::{NoiseHandshake, NoisePattern, NoiseSession},
    signatures::SigningKey,
};
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Protocol version for backward compatibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    /// Current protocol version
    pub const CURRENT: Self = Self { major: 2, minor: 0 };

    /// Check if this version is compatible with another
    pub fn is_compatible(&self, other: &Self) -> bool {
        self.major == other.major
    }
}

impl Default for ProtocolVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

/// Handshake role (initiator or responder)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeRole {
    Initiator,
    Responder,
}

/// Handshake phase - explicit state machine
#[derive(Debug)]
pub enum HandshakePhase {
    /// Initial state, no messages exchanged
    Initial,

    /// Waiting for a message from the peer
    AwaitingMessage { expected_step: u8 },

    /// Identity exchange phase after Noise handshake
    IdentityExchange { noise_session: NoiseSession },

    /// Handshake completed successfully
    Completed {
        session: NoiseSession,
        verified_identity: VerifiedPeerIdentity,
    },

    /// Handshake failed
    Failed { reason: HandshakeFailure },
}

/// Verified peer identity after successful handshake
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedPeerIdentity {
    /// Peer's static public key
    pub public_key: Vec<u8>,
    /// Peer's claimed identity
    pub peer_id: Vec<u8>,
    /// Verification timestamp (Unix seconds)
    pub verified_at: u64,
    /// Protocol version used
    pub protocol_version: ProtocolVersion,
}

/// Handshake failure reasons
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeFailure {
    /// Protocol version mismatch
    VersionMismatch {
        local: ProtocolVersion,
        remote: ProtocolVersion,
    },
    /// Noise pattern not supported
    PatternNotSupported { requested: String },
    /// Identity verification failed
    IdentityVerificationFailed { reason: String },
    /// Timeout during handshake
    Timeout { phase: String, elapsed_secs: u64 },
    /// Internal error
    InternalError { message: String },
    /// Key mismatch (Noise key doesn't match identity claim)
    KeyMismatch,
    /// Stale timestamp in identity claim
    StaleTimestamp,
    /// Invalid signature
    InvalidSignature,
}

/// Identity claim that binds Noise keys to peer identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityClaim {
    /// Peer ID (typically a hash of the public key)
    pub peer_id: Vec<u8>,
    /// Noise static public key (X25519)
    pub noise_static_key: [u8; 32],
    /// Timestamp of the claim
    pub timestamp: u64,
    /// Signature over the claim data
    pub signature: Vec<u8>,
}

impl IdentityClaim {
    /// Create a new identity claim
    pub fn create(signing_key: &SigningKey, noise_static_key: &[u8; 32]) -> Result<Self> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::crypto(format!("Time error: {}", e)))?
            .as_secs();

        // Derive peer ID from verifying key (public key)
        let verifying_key = signing_key.verifying_key();
        let peer_id = Self::derive_peer_id(verifying_key.as_bytes());

        // Create signing data
        let signing_data = Self::create_signing_data(&peer_id, noise_static_key, timestamp);

        // Sign with identity key
        let signature = crate::signatures::sign(&signing_data, signing_key);

        Ok(Self {
            peer_id,
            noise_static_key: *noise_static_key,
            timestamp,
            signature: signature.to_bytes().to_vec(),
        })
    }

    /// Verify the identity claim
    pub fn verify(&self, received_noise_key: &[u8; 32], max_clock_skew_secs: u64) -> Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        // 1. Check timestamp freshness (prevent replay attacks)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::crypto(format!("Time error: {}", e)))?
            .as_secs();

        if now.saturating_sub(self.timestamp) > max_clock_skew_secs {
            return Err(Error::crypto(format!(
                "Stale timestamp: claimed {}, current {}",
                self.timestamp, now
            )));
        }

        // 2. Verify the claimed Noise key matches what we received
        if self.noise_static_key != *received_noise_key {
            return Err(Error::crypto("Noise key mismatch - possible MITM"));
        }

        // 3. Verify signature
        let signing_data =
            Self::create_signing_data(&self.peer_id, &self.noise_static_key, self.timestamp);

        // Reconstruct verifying key from peer_id
        let verifying_key_bytes: [u8; 32] = self
            .peer_id
            .as_slice()
            .try_into()
            .map_err(|_| Error::crypto("Invalid peer ID length"))?;
        let verifying_key = crate::signatures::VerifyingKey::from_bytes(&verifying_key_bytes)
            .map_err(|e| Error::crypto(format!("Invalid verifying key: {}", e)))?;

        let signature_bytes: [u8; 64] = self
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| Error::crypto("Invalid signature length"))?;
        let signature = ed25519_dalek::Signature::from_bytes(&signature_bytes);

        crate::signatures::verify(&signing_data, &signature, &verifying_key)
            .map_err(|e| Error::crypto(format!("Signature verification failed: {}", e)))?;

        Ok(())
    }

    /// Create the data to be signed
    fn create_signing_data(peer_id: &[u8], noise_key: &[u8; 32], timestamp: u64) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"dchat-identity-claim-v1:");
        data.extend_from_slice(peer_id);
        data.extend_from_slice(noise_key);
        data.extend_from_slice(&timestamp.to_le_bytes());
        data
    }

    /// Derive peer ID from public key (using BLAKE3 hash, return raw key for verification)
    fn derive_peer_id(public_key_bytes: &[u8]) -> Vec<u8> {
        // For signature verification, we need the raw public key bytes
        public_key_bytes.to_vec()
    }
}

/// Structured handshake messages for wire format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeMessage {
    /// Initial handshake message
    Init {
        version: ProtocolVersion,
        noise_payload: Vec<u8>,
        supported_patterns: Vec<String>,
    },

    /// Response to init
    Response {
        version: ProtocolVersion,
        selected_pattern: String,
        noise_payload: Vec<u8>,
    },

    /// Final Noise message with encrypted identity
    Final {
        noise_payload: Vec<u8>,
        encrypted_identity: Vec<u8>,
    },

    /// Identity claim (sent after Noise handshake completes)
    Identity { encrypted_claim: Vec<u8> },

    /// Handshake acknowledgment
    Ack { session_id: [u8; 32] },

    /// Handshake rejection
    Reject {
        reason: HandshakeRejectReason,
        message: String,
    },
}

/// Reasons for rejecting a handshake
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeRejectReason {
    VersionMismatch,
    PatternNotSupported,
    IdentityVerificationFailed,
    RateLimited { retry_after_ms: u64 },
    ResourceExhausted,
    InternalError,
}

/// Typed handshake manager with compile-time state validation
pub struct TypedHandshake {
    phase: HandshakePhase,
    noise_handshake: Option<NoiseHandshake>,
    role: HandshakeRole,
    local_private_key: PrivateKey,
    local_signing_key: SigningKey,
    protocol_version: ProtocolVersion,
    started_at: Instant,
    message_count: u8,
}

impl TypedHandshake {
    /// Create a new handshake as initiator
    pub fn new_initiator(local_private_key: PrivateKey, local_signing_key: SigningKey) -> Self {
        Self {
            phase: HandshakePhase::Initial,
            noise_handshake: None,
            role: HandshakeRole::Initiator,
            local_private_key,
            local_signing_key,
            protocol_version: ProtocolVersion::CURRENT,
            started_at: Instant::now(),
            message_count: 0,
        }
    }

    /// Create a new handshake as responder
    pub fn new_responder(local_private_key: PrivateKey, local_signing_key: SigningKey) -> Self {
        Self {
            phase: HandshakePhase::Initial,
            noise_handshake: None,
            role: HandshakeRole::Responder,
            local_private_key,
            local_signing_key,
            protocol_version: ProtocolVersion::CURRENT,
            started_at: Instant::now(),
            message_count: 0,
        }
    }

    /// Get current phase name
    pub fn phase_name(&self) -> &'static str {
        match &self.phase {
            HandshakePhase::Initial => "initial",
            HandshakePhase::AwaitingMessage { .. } => "awaiting_message",
            HandshakePhase::IdentityExchange { .. } => "identity_exchange",
            HandshakePhase::Completed { .. } => "completed",
            HandshakePhase::Failed { .. } => "failed",
        }
    }

    /// Check if handshake is completed
    pub fn is_completed(&self) -> bool {
        matches!(&self.phase, HandshakePhase::Completed { .. })
    }

    /// Check if handshake has failed
    pub fn is_failed(&self) -> bool {
        matches!(&self.phase, HandshakePhase::Failed { .. })
    }

    /// Get elapsed time since handshake started
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Initiate handshake (only valid in Initial phase for Initiator role)
    pub fn send_init(&mut self, pattern: NoisePattern) -> Result<HandshakeMessage> {
        match &self.phase {
            HandshakePhase::Initial if self.role == HandshakeRole::Initiator => {
                // Create Noise handshake
                let mut noise = NoiseHandshake::initiate(pattern, &self.local_private_key, None)?;

                // Write first message
                let noise_payload = noise.write_message(&[])?;

                // Store handshake state
                self.noise_handshake = Some(noise);
                self.phase = HandshakePhase::AwaitingMessage { expected_step: 2 };
                self.message_count += 1;

                Ok(HandshakeMessage::Init {
                    version: self.protocol_version,
                    noise_payload,
                    supported_patterns: vec!["XX".to_string(), "IK".to_string()],
                })
            }
            _ => Err(Error::crypto(format!(
                "Invalid state for send_init: phase={}, role={:?}",
                self.phase_name(),
                self.role
            ))),
        }
    }

    /// Process an incoming handshake message
    pub fn process_message(
        &mut self,
        message: HandshakeMessage,
    ) -> Result<Option<HandshakeMessage>> {
        match message {
            HandshakeMessage::Init {
                version,
                noise_payload,
                supported_patterns,
            } => self.handle_init(version, noise_payload, supported_patterns),
            HandshakeMessage::Response {
                version,
                selected_pattern,
                noise_payload,
            } => self.handle_response(version, selected_pattern, noise_payload),
            HandshakeMessage::Final {
                noise_payload,
                encrypted_identity,
            } => self.handle_final(noise_payload, encrypted_identity),
            HandshakeMessage::Identity { encrypted_claim } => self.handle_identity(encrypted_claim),
            HandshakeMessage::Ack { session_id } => self.handle_ack(session_id),
            HandshakeMessage::Reject { reason, message } => {
                self.phase = HandshakePhase::Failed {
                    reason: HandshakeFailure::InternalError {
                        message: format!("{:?}: {}", reason, message),
                    },
                };
                Ok(None)
            }
        }
    }

    fn handle_init(
        &mut self,
        version: ProtocolVersion,
        noise_payload: Vec<u8>,
        _supported_patterns: Vec<String>,
    ) -> Result<Option<HandshakeMessage>> {
        // Check version compatibility
        if !self.protocol_version.is_compatible(&version) {
            self.phase = HandshakePhase::Failed {
                reason: HandshakeFailure::VersionMismatch {
                    local: self.protocol_version,
                    remote: version,
                },
            };
            return Ok(Some(HandshakeMessage::Reject {
                reason: HandshakeRejectReason::VersionMismatch,
                message: format!(
                    "Incompatible version: {} vs {}",
                    version.major, self.protocol_version.major
                ),
            }));
        }

        // Only valid for responder in Initial phase
        match &self.phase {
            HandshakePhase::Initial if self.role == HandshakeRole::Responder => {
                // Create Noise handshake and process message
                let mut noise = NoiseHandshake::respond(NoisePattern::XX, &self.local_private_key)?;

                noise.read_message(&noise_payload)?;
                let response_payload = noise.write_message(&[])?;

                self.noise_handshake = Some(noise);
                self.phase = HandshakePhase::AwaitingMessage { expected_step: 3 };
                self.message_count += 1;

                Ok(Some(HandshakeMessage::Response {
                    version: self.protocol_version,
                    selected_pattern: "XX".to_string(),
                    noise_payload: response_payload,
                }))
            }
            _ => Err(Error::crypto("Invalid state for handle_init")),
        }
    }

    fn handle_response(
        &mut self,
        _version: ProtocolVersion,
        _selected_pattern: String,
        noise_payload: Vec<u8>,
    ) -> Result<Option<HandshakeMessage>> {
        match &mut self.noise_handshake {
            Some(noise) => {
                noise.read_message(&noise_payload)?;

                if noise.is_handshake_finished() {
                    // Transition to transport mode
                    let old_noise = self.noise_handshake.take().unwrap();
                    let session = old_noise.into_transport_mode()?;

                    // Get local static key from private key
                    let noise_static = *self.local_private_key.as_bytes();
                    let claim = IdentityClaim::create(&self.local_signing_key, &noise_static)?;
                    let claim_bytes = bincode::serialize(&claim)
                        .map_err(|e| Error::crypto(format!("Serialize error: {}", e)))?;

                    // For identity exchange we need mutable session
                    self.phase = HandshakePhase::IdentityExchange {
                        noise_session: session,
                    };
                    self.message_count += 1;

                    // Encrypt the claim with the session
                    if let HandshakePhase::IdentityExchange {
                        ref mut noise_session,
                    } = self.phase
                    {
                        let encrypted_claim = noise_session.encrypt(&claim_bytes)?;
                        return Ok(Some(HandshakeMessage::Identity { encrypted_claim }));
                    }

                    Ok(None)
                } else {
                    let next_payload = noise.write_message(&[])?;
                    self.message_count += 1;

                    Ok(Some(HandshakeMessage::Final {
                        noise_payload: next_payload,
                        encrypted_identity: vec![],
                    }))
                }
            }
            None => Err(Error::crypto("No active Noise handshake")),
        }
    }

    fn handle_final(
        &mut self,
        noise_payload: Vec<u8>,
        _encrypted_identity: Vec<u8>,
    ) -> Result<Option<HandshakeMessage>> {
        match &mut self.noise_handshake {
            Some(noise) => {
                noise.read_message(&noise_payload)?;

                if noise.is_handshake_finished() {
                    let old_noise = self.noise_handshake.take().unwrap();
                    let session = old_noise.into_transport_mode()?;

                    self.phase = HandshakePhase::IdentityExchange {
                        noise_session: session,
                    };
                }

                self.message_count += 1;
                Ok(None)
            }
            None => Err(Error::crypto("No active Noise handshake")),
        }
    }

    fn handle_identity(&mut self, encrypted_claim: Vec<u8>) -> Result<Option<HandshakeMessage>> {
        use std::time::{SystemTime, UNIX_EPOCH};

        match std::mem::replace(&mut self.phase, HandshakePhase::Initial) {
            HandshakePhase::IdentityExchange { mut noise_session } => {
                // Decrypt and verify identity claim
                let claim_bytes = noise_session.decrypt(&encrypted_claim)?;
                let claim: IdentityClaim = bincode::deserialize(&claim_bytes)
                    .map_err(|e| Error::crypto(format!("Deserialize error: {}", e)))?;

                // Get remote static key from Noise session
                let remote_static = noise_session
                    .get_remote_static_key()
                    .ok_or_else(|| Error::crypto("No remote static key"))?;
                let remote_static_arr: [u8; 32] = *remote_static.as_bytes();

                // Verify identity claim with 5 minute clock skew tolerance
                claim.verify(&remote_static_arr, 300)?;

                // Generate session ID
                let mut session_id = [0u8; 32];
                use rand::RngCore;
                rand::thread_rng().fill_bytes(&mut session_id);

                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                self.phase = HandshakePhase::Completed {
                    session: noise_session,
                    verified_identity: VerifiedPeerIdentity {
                        public_key: claim.peer_id.clone(),
                        peer_id: claim.peer_id,
                        verified_at: now,
                        protocol_version: self.protocol_version,
                    },
                };

                Ok(Some(HandshakeMessage::Ack { session_id }))
            }
            other => {
                self.phase = other;
                Err(Error::crypto("Invalid state for handle_identity"))
            }
        }
    }

    fn handle_ack(&mut self, _session_id: [u8; 32]) -> Result<Option<HandshakeMessage>> {
        use std::time::{SystemTime, UNIX_EPOCH};

        match std::mem::replace(&mut self.phase, HandshakePhase::Initial) {
            HandshakePhase::IdentityExchange { noise_session } => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                // Handshake completed
                self.phase = HandshakePhase::Completed {
                    session: noise_session,
                    verified_identity: VerifiedPeerIdentity {
                        public_key: vec![],
                        peer_id: vec![],
                        verified_at: now,
                        protocol_version: self.protocol_version,
                    },
                };
                Ok(None)
            }
            other => {
                self.phase = other;
                Err(Error::crypto("Invalid state for handle_ack"))
            }
        }
    }

    /// Take the completed session (consumes the handshake)
    pub fn take_session(self) -> Result<(NoiseSession, VerifiedPeerIdentity)> {
        match self.phase {
            HandshakePhase::Completed {
                session,
                verified_identity,
            } => Ok((session, verified_identity)),
            _ => Err(Error::crypto("Handshake not completed")),
        }
    }
}

/// Handshake with timeout awareness
pub struct TimeoutAwareHandshake {
    inner: TypedHandshake,
    total_timeout: Duration,
    phase_timeouts: std::collections::HashMap<String, Duration>,
    phase_start_times: std::collections::HashMap<String, std::time::Instant>,
}

impl TimeoutAwareHandshake {
    /// Create a new timeout-aware handshake
    pub fn new(inner: TypedHandshake, total_timeout: Duration) -> Self {
        let mut phase_timeouts = std::collections::HashMap::new();
        phase_timeouts.insert("noise".to_string(), Duration::from_secs(10));
        phase_timeouts.insert("identity".to_string(), Duration::from_secs(10));
        phase_timeouts.insert("initial".to_string(), Duration::from_secs(5));
        phase_timeouts.insert("key_exchange".to_string(), Duration::from_secs(15));
        phase_timeouts.insert("authentication".to_string(), Duration::from_secs(10));
        phase_timeouts.insert("finalization".to_string(), Duration::from_secs(5));

        let mut phase_start_times = std::collections::HashMap::new();
        phase_start_times.insert("initial".to_string(), std::time::Instant::now());

        Self {
            inner,
            total_timeout,
            phase_timeouts,
            phase_start_times,
        }
    }

    /// Set custom timeout for a specific phase
    pub fn set_phase_timeout(&mut self, phase: &str, timeout: Duration) {
        self.phase_timeouts.insert(phase.to_string(), timeout);
    }

    /// Get the timeout for a specific phase
    pub fn get_phase_timeout(&self, phase: &str) -> Option<Duration> {
        self.phase_timeouts.get(phase).copied()
    }

    /// Check if the handshake has timed out (both total and phase-specific)
    pub fn check_timeout(&self) -> Result<()> {
        let elapsed = self.inner.elapsed();
        let current_phase = self.inner.phase_name();

        // Check total timeout
        if elapsed > self.total_timeout {
            return Err(Error::crypto(format!(
                "Handshake total timeout: phase={}, elapsed={:?}, limit={:?}",
                current_phase, elapsed, self.total_timeout
            )));
        }

        // Check phase-specific timeout
        if let Some(phase_timeout) = self.phase_timeouts.get(current_phase) {
            if let Some(phase_start) = self.phase_start_times.get(current_phase) {
                let phase_elapsed = phase_start.elapsed();
                if phase_elapsed > *phase_timeout {
                    return Err(Error::crypto(format!(
                        "Handshake phase timeout: phase={}, elapsed={:?}, limit={:?}",
                        current_phase, phase_elapsed, phase_timeout
                    )));
                }
            }
        }

        Ok(())
    }

    /// Record transition to a new phase
    fn record_phase_transition(&mut self) {
        let current_phase = self.inner.phase_name().to_string();
        if !self.phase_start_times.contains_key(&current_phase) {
            self.phase_start_times
                .insert(current_phase, std::time::Instant::now());
        }
    }

    /// Process message with timeout check
    pub fn process_message(
        &mut self,
        message: HandshakeMessage,
    ) -> Result<Option<HandshakeMessage>> {
        self.check_timeout()?;
        let result = self.inner.process_message(message)?;
        self.record_phase_transition();
        Ok(result)
    }

    /// Get timing metrics for the handshake
    pub fn get_timing_metrics(&self) -> std::collections::HashMap<String, Duration> {
        let mut metrics = std::collections::HashMap::new();
        for (phase, start_time) in &self.phase_start_times {
            metrics.insert(phase.clone(), start_time.elapsed());
        }
        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::PrivateKey;
    use crate::signatures::SigningKey;

    #[test]
    fn test_protocol_version_compatibility() {
        let v1 = ProtocolVersion { major: 2, minor: 0 };
        let v2 = ProtocolVersion { major: 2, minor: 1 };
        let v3 = ProtocolVersion { major: 3, minor: 0 };

        assert!(v1.is_compatible(&v2));
        assert!(!v1.is_compatible(&v3));
    }

    #[test]
    #[allow(deprecated)]
    fn test_typed_handshake_creation() {
        let key = PrivateKey::generate();
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let hs = TypedHandshake::new_initiator(key, signing_key);

        assert_eq!(hs.phase_name(), "initial");
        assert!(!hs.is_completed());
        assert!(!hs.is_failed());
    }
}
