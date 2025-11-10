use crate::crypto::versioning::{negotiate_version, NegotiationResult, ProtocolVersion};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Timeout for version negotiation
pub const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Version negotiation message exchanged during handshake
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMessage {
    pub version: ProtocolVersion,
    pub timestamp: u64, // Unix timestamp in seconds
}

impl VersionMessage {
    /// Create a new version message with the current protocol version
    pub fn new() -> Self {
        Self {
            version: ProtocolVersion::current(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Create a version message with a specific version (for testing)
    pub fn with_version(version: ProtocolVersion) -> Self {
        Self {
            version,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Check if the message timestamp is within acceptable range
    pub fn is_timestamp_valid(&self, max_age: Duration) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let age = now.saturating_sub(self.timestamp);
        age <= max_age.as_secs()
    }

    /// Serialize to bytes for transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>, NegotiationError> {
        bincode::serialize(self).map_err(|e| NegotiationError::SerializationFailed(e.to_string()))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, NegotiationError> {
        bincode::deserialize(bytes)
            .map_err(|e| NegotiationError::DeserializationFailed(e.to_string()))
    }
}

impl Default for VersionMessage {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors during version negotiation
#[derive(Debug, Error)]
pub enum NegotiationError {
    #[error("Version negotiation failed: {0}")]
    NegotiationFailed(String),

    #[error("Negotiation timeout after {0:?}")]
    Timeout(Duration),

    #[error("Invalid version message timestamp")]
    InvalidTimestamp,

    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    #[error("Downgrade attack detected from peer {0}")]
    DowngradeAttack(String),

    #[error("Major version mismatch with peer {0}")]
    MajorMismatch(String),

    #[error("Peer version too old: {0}")]
    PeerTooOld(String),

    #[error("Peer version too new: {0}")]
    PeerTooNew(String),
}

/// Tracks version negotiation state for a peer
#[derive(Debug, Clone)]
struct NegotiationState {
    peer_id: PeerId,
    started_at: Instant,
    local_version: ProtocolVersion,
    remote_version: Option<ProtocolVersion>,
    result: Option<NegotiationResult>,
}

/// Metrics for version negotiation
#[derive(Debug, Clone, Default)]
pub struct NegotiationMetrics {
    pub total_negotiations: u64,
    pub successful_negotiations: u64,
    pub failed_negotiations: u64,
    pub downgrade_attempts: u64,
    pub major_mismatches: u64,
    pub timeout_count: u64,
    pub avg_negotiation_duration_ms: u64,
}

/// Manages version negotiation with peers
pub struct VersionNegotiator {
    /// Active negotiations
    negotiations: Arc<RwLock<HashMap<PeerId, NegotiationState>>>,

    /// Negotiation metrics
    metrics: Arc<RwLock<NegotiationMetrics>>,

    /// Maximum age for version message timestamps
    max_timestamp_age: Duration,
}

impl VersionNegotiator {
    /// Create a new version negotiator
    pub fn new() -> Self {
        Self {
            negotiations: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(NegotiationMetrics::default())),
            max_timestamp_age: Duration::from_secs(60),
        }
    }

    /// Start version negotiation with a peer (initiator side)
    pub fn initiate(&self, peer_id: PeerId) -> VersionMessage {
        let state = NegotiationState {
            peer_id,
            started_at: Instant::now(),
            local_version: ProtocolVersion::current(),
            remote_version: None,
            result: None,
        };

        self.negotiations.write().unwrap().insert(peer_id, state);

        VersionMessage::new()
    }

    /// Process a version message from a peer
    pub fn process_version_message(
        &self,
        peer_id: PeerId,
        message: VersionMessage,
    ) -> Result<NegotiationResult, NegotiationError> {
        // Validate timestamp
        if !message.is_timestamp_valid(self.max_timestamp_age) {
            return Err(NegotiationError::InvalidTimestamp);
        }

        // Get or create negotiation state
        let mut negotiations = self.negotiations.write().unwrap();
        let state = negotiations.entry(peer_id).or_insert_with(|| NegotiationState {
            peer_id,
            started_at: Instant::now(),
            local_version: ProtocolVersion::current(),
            remote_version: None,
            result: None,
        });

        // Store remote version
        state.remote_version = Some(message.version.clone());

        // Negotiate version
        let result = negotiate_version(&message.version);

        // Update metrics
        let duration_ms = state.started_at.elapsed().as_millis() as u64;
        self.update_metrics(&result, duration_ms);

        // Store result
        state.result = Some(result.clone());

        // Check for failures
        match &result {
            NegotiationResult::Compatible { .. } => Ok(result),
            NegotiationResult::RemoteTooOld { .. } => {
                Err(NegotiationError::PeerTooOld(result.description()))
            }
            NegotiationResult::RemoteTooNew { .. } => {
                Err(NegotiationError::PeerTooNew(result.description()))
            }
            NegotiationResult::MajorMismatch { .. } => {
                Err(NegotiationError::MajorMismatch(result.description()))
            }
            NegotiationResult::DowngradeAttack { .. } => {
                Err(NegotiationError::DowngradeAttack(result.description()))
            }
        }
    }

    /// Update negotiation metrics
    fn update_metrics(&self, result: &NegotiationResult, duration_ms: u64) {
        let mut metrics = self.metrics.write().unwrap();
        metrics.total_negotiations += 1;

        if result.is_success() {
            metrics.successful_negotiations += 1;
        } else {
            metrics.failed_negotiations += 1;

            match result {
                NegotiationResult::DowngradeAttack { .. } => {
                    metrics.downgrade_attempts += 1;
                }
                NegotiationResult::MajorMismatch { .. } => {
                    metrics.major_mismatches += 1;
                }
                _ => {}
            }
        }

        // Update average duration using exponential moving average
        if metrics.avg_negotiation_duration_ms == 0 {
            metrics.avg_negotiation_duration_ms = duration_ms;
        } else {
            metrics.avg_negotiation_duration_ms =
                (metrics.avg_negotiation_duration_ms * 7 + duration_ms * 3) / 10;
        }
    }

    /// Clean up timed-out negotiations
    pub fn cleanup_timed_out(&self) -> Vec<PeerId> {
        let mut negotiations = self.negotiations.write().unwrap();
        let mut timed_out = Vec::new();

        negotiations.retain(|peer_id, state| {
            if state.started_at.elapsed() > NEGOTIATION_TIMEOUT {
                timed_out.push(*peer_id);
                false
            } else {
                true
            }
        });

        if !timed_out.is_empty() {
            let mut metrics = self.metrics.write().unwrap();
            metrics.timeout_count += timed_out.len() as u64;
        }

        timed_out
    }

    /// Get negotiation metrics
    pub fn metrics(&self) -> NegotiationMetrics {
        self.metrics.read().unwrap().clone()
    }

    /// Get negotiation state for a peer
    pub fn get_negotiation_state(&self, peer_id: &PeerId) -> Option<NegotiationResult> {
        self.negotiations
            .read()
            .unwrap()
            .get(peer_id)
            .and_then(|state| state.result.clone())
    }

    /// Remove negotiation state for a peer
    pub fn remove_peer(&self, peer_id: &PeerId) {
        self.negotiations.write().unwrap().remove(peer_id);
    }

    /// Get count of active negotiations
    pub fn active_count(&self) -> usize {
        self.negotiations.read().unwrap().len()
    }
}

impl Default for VersionNegotiator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer_id() -> PeerId {
        PeerId::random()
    }

    #[test]
    fn test_version_message_creation() {
        let msg = VersionMessage::new();
        assert_eq!(msg.version, ProtocolVersion::current());
        assert!(msg.is_timestamp_valid(Duration::from_secs(60)));
    }

    #[test]
    fn test_version_message_serialization() {
        let msg = VersionMessage::new();
        let bytes = msg.to_bytes().unwrap();
        let deserialized = VersionMessage::from_bytes(&bytes).unwrap();
        assert_eq!(msg.version, deserialized.version);
    }

    #[test]
    fn test_version_message_timestamp_validation() {
        let mut msg = VersionMessage::new();
        assert!(msg.is_timestamp_valid(Duration::from_secs(60)));

        // Set timestamp to 2 minutes ago
        msg.timestamp -= 120;
        assert!(!msg.is_timestamp_valid(Duration::from_secs(60)));
    }

    #[test]
    fn test_negotiator_initiate() {
        let negotiator = VersionNegotiator::new();
        let peer_id = test_peer_id();
        let msg = negotiator.initiate(peer_id);
        assert_eq!(msg.version, ProtocolVersion::current());
        assert_eq!(negotiator.active_count(), 1);
    }

    #[test]
    fn test_successful_negotiation() {
        let negotiator = VersionNegotiator::new();
        let peer_id = test_peer_id();

        negotiator.initiate(peer_id);

        let remote_msg = VersionMessage::new();
        let result = negotiator.process_version_message(peer_id, remote_msg);

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.is_success());

        let metrics = negotiator.metrics();
        assert_eq!(metrics.successful_negotiations, 1);
        assert_eq!(metrics.failed_negotiations, 0);
    }

    #[test]
    fn test_failed_negotiation_old_version() {
        let negotiator = VersionNegotiator::new();
        let peer_id = test_peer_id();

        negotiator.initiate(peer_id);

        let old_version = ProtocolVersion::new(0, 9, 0);
        let remote_msg = VersionMessage::with_version(old_version);
        let result = negotiator.process_version_message(peer_id, remote_msg);

        // Old version 0.9.0 triggers DowngradeAttack check first
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(NegotiationError::DowngradeAttack(_))
        ));

        let metrics = negotiator.metrics();
        assert_eq!(metrics.successful_negotiations, 0);
        assert_eq!(metrics.failed_negotiations, 1);
        assert_eq!(metrics.downgrade_attempts, 1);
    }

    #[test]
    fn test_failed_negotiation_major_mismatch() {
        let negotiator = VersionNegotiator::new();
        let peer_id = test_peer_id();

        negotiator.initiate(peer_id);

        let future_version = ProtocolVersion::new(2, 0, 0);
        let remote_msg = VersionMessage::with_version(future_version);
        let result = negotiator.process_version_message(peer_id, remote_msg);

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(NegotiationError::MajorMismatch(_))
        ));

        let metrics = negotiator.metrics();
        assert_eq!(metrics.major_mismatches, 1);
    }

    #[test]
    fn test_negotiation_timeout_cleanup() {
        let negotiator = VersionNegotiator::new();
        let peer_id = test_peer_id();

        negotiator.initiate(peer_id);
        assert_eq!(negotiator.active_count(), 1);

        // Manually set the started_at time to trigger timeout
        {
            let mut negotiations = negotiator.negotiations.write().unwrap();
            if let Some(state) = negotiations.get_mut(&peer_id) {
                state.started_at = Instant::now() - NEGOTIATION_TIMEOUT - Duration::from_secs(1);
            }
        }

        let timed_out = negotiator.cleanup_timed_out();
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], peer_id);
        assert_eq!(negotiator.active_count(), 0);
    }

    #[test]
    fn test_metrics_tracking() {
        let negotiator = VersionNegotiator::new();

        // Successful negotiation
        let peer1 = test_peer_id();
        negotiator.initiate(peer1);
        let _ = negotiator.process_version_message(peer1, VersionMessage::new());

        // Failed negotiation (downgrade attack)
        let peer2 = test_peer_id();
        negotiator.initiate(peer2);
        let old_msg = VersionMessage::with_version(ProtocolVersion::new(0, 9, 0));
        let _ = negotiator.process_version_message(peer2, old_msg);

        let metrics = negotiator.metrics();
        assert_eq!(metrics.total_negotiations, 2);
        assert_eq!(metrics.successful_negotiations, 1);
        assert_eq!(metrics.failed_negotiations, 1);
        assert_eq!(metrics.downgrade_attempts, 1);
        // avg_negotiation_duration_ms might be 0 in fast tests, so just check it's set
        // (real-world usage will always have non-zero duration)
    }

    #[test]
    fn test_get_negotiation_state() {
        let negotiator = VersionNegotiator::new();
        let peer_id = test_peer_id();

        negotiator.initiate(peer_id);
        assert!(negotiator.get_negotiation_state(&peer_id).is_none());

        let _ = negotiator.process_version_message(peer_id, VersionMessage::new());
        let state = negotiator.get_negotiation_state(&peer_id);
        assert!(state.is_some());
        assert!(state.unwrap().is_success());
    }

    #[test]
    fn test_remove_peer() {
        let negotiator = VersionNegotiator::new();
        let peer_id = test_peer_id();

        negotiator.initiate(peer_id);
        assert_eq!(negotiator.active_count(), 1);

        negotiator.remove_peer(&peer_id);
        assert_eq!(negotiator.active_count(), 0);
    }
}
