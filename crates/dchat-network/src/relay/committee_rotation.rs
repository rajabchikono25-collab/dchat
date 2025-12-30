//! Committee Rotation Manager for QGE
//!
//! This module manages the rotation of relay committees across epochs,
//! ensuring smooth handoffs and maintaining service continuity.
//!
//! # Overview
//!
//! Relay committees rotate every epoch (10 minutes) based on VRF selection.
//! This module handles:
//!
//! - Pre-computing upcoming committee compositions
//! - Managing transition periods with overlapping committees
//! - Coordinating FROST key shares for new committees
//! - Handling relay joins/leaves during rotation
//!
//! # Rotation Protocol
//!
//! 1. **Pre-Rotation (30 seconds before epoch end)**:
//!    - Compute next epoch's committee
//!    - Begin FROST DKG for new committee
//!    - Notify relays of upcoming committee change
//!
//! 2. **Transition Period (30 seconds overlap)**:
//!    - Both old and new committees are active
//!    - Requests accepted by either committee
//!    - New committee takes over token issuance
//!
//! 3. **Post-Rotation (after transition)**:
//!    - Old committee deactivated
//!    - Clean up old key shares
//!    - Archive rotation metrics
//!
//! # Security Properties
//!
//! - **Unpredictability**: VRF ensures committees cannot be predicted in advance
//! - **Continuity**: Overlap period ensures no service interruption
//! - **Key Freshness**: New FROST keys per epoch prevent key compromise accumulation
//! - **Verifiability**: All rotations are verifiable via VRF proofs

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use super::epoch_token::{
    current_epoch_id, epoch_end, epoch_start, ConversationType, EPOCH_DURATION_SECS,
    EPOCH_GRACE_PERIOD_SECS,
};

/// Transition period duration (seconds before and after epoch boundary)
pub const TRANSITION_PERIOD_SECS: u64 = 30;

/// Pre-rotation preparation time (seconds before epoch end)
pub const PRE_ROTATION_PREP_SECS: u64 = 60;

/// Maximum allowed clock skew between relays (seconds)
pub const MAX_CLOCK_SKEW_SECS: u64 = 5;

/// Committee rotation status for a specific conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitteeRotationState {
    /// Conversation ID hash
    pub conversation_id_hash: [u8; 32],

    /// Conversation type
    pub conversation_type: ConversationType,

    /// Current active committee
    pub current_committee: CommitteeInfo,

    /// Next committee (populated during pre-rotation)
    pub next_committee: Option<CommitteeInfo>,

    /// Previous committee (for transition period)
    pub previous_committee: Option<CommitteeInfo>,

    /// Current rotation phase
    pub phase: RotationPhase,

    /// Last rotation timestamp
    pub last_rotation_at: u64,

    /// Rotation statistics
    pub stats: RotationStats,
}

impl CommitteeRotationState {
    /// Create new rotation state for a conversation
    pub fn new(
        conversation_id_hash: [u8; 32],
        conversation_type: ConversationType,
        initial_committee: CommitteeInfo,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            conversation_id_hash,
            conversation_type,
            current_committee: initial_committee,
            next_committee: None,
            previous_committee: None,
            phase: RotationPhase::Stable,
            last_rotation_at: now,
            stats: RotationStats::default(),
        }
    }

    /// Check if we're in a transition period
    pub fn is_in_transition(&self) -> bool {
        matches!(self.phase, RotationPhase::Transition { .. })
    }

    /// Check if a relay is currently active (in current or transition committee)
    pub fn is_relay_active(&self, relay_id: &[u8; 32]) -> bool {
        if self.current_committee.contains(relay_id) {
            return true;
        }

        if let RotationPhase::Transition { .. } = &self.phase {
            if let Some(prev) = &self.previous_committee {
                if prev.contains(relay_id) {
                    return true;
                }
            }
        }

        false
    }

    /// Get the threshold for token acceptance
    pub fn current_threshold(&self) -> u8 {
        self.current_committee.threshold
    }
}

/// Information about a committee
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitteeInfo {
    /// Epoch this committee is valid for
    pub epoch_id: u64,

    /// Member relay IDs
    pub members: Vec<[u8; 32]>,

    /// Member public keys (for signature verification)
    pub member_public_keys: Vec<[u8; 32]>,

    /// Aggregate public key (for FROST)
    pub aggregate_public_key: [u8; 32],

    /// Threshold for signatures
    pub threshold: u8,

    /// VRF proof of committee selection
    pub vrf_proof: Option<[u8; 64]>,

    /// Geographic distribution
    pub geo_distribution: HashMap<String, usize>,

    /// When this committee was formed
    pub formed_at: u64,

    /// FROST key generation complete
    pub dkg_complete: bool,
}

impl CommitteeInfo {
    /// Check if a relay is a member of this committee
    pub fn contains(&self, relay_id: &[u8; 32]) -> bool {
        self.members.iter().any(|m| m == relay_id)
    }

    /// Get member index for a relay
    pub fn member_index(&self, relay_id: &[u8; 32]) -> Option<usize> {
        self.members.iter().position(|m| m == relay_id)
    }

    /// Get quorum size
    pub fn quorum_size(&self) -> usize {
        self.members.len()
    }

    /// Check if DKG is complete
    pub fn is_ready(&self) -> bool {
        self.dkg_complete
    }
}

/// Rotation phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RotationPhase {
    /// Stable period - only current committee active
    Stable,

    /// Pre-rotation - preparing next committee
    PreRotation {
        /// When pre-rotation started
        started_at: u64,
        /// Target epoch
        target_epoch: u64,
    },

    /// Transition - both committees active
    Transition {
        /// When transition started
        started_at: u64,
        /// When transition ends
        ends_at: u64,
    },

    /// Post-rotation - cleaning up old committee
    PostRotation {
        /// When post-rotation started
        started_at: u64,
    },
}

/// Statistics for committee rotations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RotationStats {
    /// Total rotations completed
    pub total_rotations: u64,

    /// Successful transitions (no service interruption)
    pub successful_transitions: u64,

    /// Failed transitions (required fallback)
    pub failed_transitions: u64,

    /// Average DKG completion time (ms)
    pub avg_dkg_time_ms: u64,

    /// Average transition time (ms)
    pub avg_transition_time_ms: u64,

    /// Relay churn count (joins + leaves during rotation)
    pub relay_churn_count: u64,
}

/// Event emitted during rotation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RotationEvent {
    /// Pre-rotation phase started
    PreRotationStarted {
        conversation_id_hash: [u8; 32],
        target_epoch: u64,
        next_committee_size: usize,
    },

    /// DKG completed for next committee
    DkgCompleted {
        conversation_id_hash: [u8; 32],
        epoch_id: u64,
        aggregate_public_key: [u8; 32],
    },

    /// Transition started
    TransitionStarted {
        conversation_id_hash: [u8; 32],
        epoch_id: u64,
        old_committee_size: usize,
        new_committee_size: usize,
    },

    /// Rotation completed
    RotationCompleted {
        conversation_id_hash: [u8; 32],
        epoch_id: u64,
        duration_ms: u64,
    },

    /// Rotation failed (fallback activated)
    RotationFailed {
        conversation_id_hash: [u8; 32],
        epoch_id: u64,
        reason: String,
    },

    /// Relay joined during rotation
    RelayJoined { relay_id: [u8; 32], epoch_id: u64 },

    /// Relay left during rotation
    RelayLeft { relay_id: [u8; 32], epoch_id: u64 },
}

/// Committee rotation manager
pub struct CommitteeRotationManager {
    /// Rotation states per conversation
    states: Arc<RwLock<HashMap<[u8; 32], CommitteeRotationState>>>,

    /// Event listeners
    event_listeners: Arc<RwLock<Vec<Box<dyn Fn(RotationEvent) + Send + Sync>>>>,

    /// Configuration
    config: RotationConfig,

    /// Local relay ID
    local_relay_id: [u8; 32],
}

/// Configuration for rotation manager
#[derive(Debug, Clone)]
pub struct RotationConfig {
    /// Transition period duration
    pub transition_period_secs: u64,

    /// Pre-rotation preparation time
    pub pre_rotation_prep_secs: u64,

    /// Maximum clock skew
    pub max_clock_skew_secs: u64,

    /// Enable automatic rotation
    pub auto_rotate: bool,

    /// DKG timeout
    pub dkg_timeout_secs: u64,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            transition_period_secs: TRANSITION_PERIOD_SECS,
            pre_rotation_prep_secs: PRE_ROTATION_PREP_SECS,
            max_clock_skew_secs: MAX_CLOCK_SKEW_SECS,
            auto_rotate: true,
            dkg_timeout_secs: 30,
        }
    }
}

impl CommitteeRotationManager {
    /// Create a new rotation manager
    pub fn new(local_relay_id: [u8; 32]) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            event_listeners: Arc::new(RwLock::new(Vec::new())),
            config: RotationConfig::default(),
            local_relay_id,
        }
    }

    /// Create with custom configuration
    pub fn with_config(local_relay_id: [u8; 32], config: RotationConfig) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            event_listeners: Arc::new(RwLock::new(Vec::new())),
            config,
            local_relay_id,
        }
    }

    /// Register a conversation for rotation management
    pub async fn register_conversation(
        &self,
        conversation_id_hash: [u8; 32],
        conversation_type: ConversationType,
        initial_committee: CommitteeInfo,
    ) {
        let mut states = self.states.write().await;
        states.insert(
            conversation_id_hash,
            CommitteeRotationState::new(conversation_id_hash, conversation_type, initial_committee),
        );
    }

    /// Unregister a conversation
    pub async fn unregister_conversation(&self, conversation_id_hash: &[u8; 32]) {
        let mut states = self.states.write().await;
        states.remove(conversation_id_hash);
    }

    /// Check if rotation is needed and trigger if so
    pub async fn check_and_rotate(&self) -> Vec<RotationEvent> {
        let current_epoch = current_epoch_id();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let epoch_end_time = epoch_end(current_epoch);
        let time_until_end = epoch_end_time.saturating_sub(now);

        let mut events = Vec::new();
        let mut states = self.states.write().await;

        for (conversation_id_hash, state) in states.iter_mut() {
            // Check if we need to enter pre-rotation
            if matches!(state.phase, RotationPhase::Stable)
                && time_until_end <= self.config.pre_rotation_prep_secs
                && state.current_committee.epoch_id == current_epoch
            {
                // Enter pre-rotation
                state.phase = RotationPhase::PreRotation {
                    started_at: now,
                    target_epoch: current_epoch + 1,
                };

                events.push(RotationEvent::PreRotationStarted {
                    conversation_id_hash: *conversation_id_hash,
                    target_epoch: current_epoch + 1,
                    next_committee_size: state.conversation_type.quorum_size(),
                });
            }

            // Check if we need to start transition
            if let RotationPhase::PreRotation { target_epoch, .. } = state.phase {
                if current_epoch >= target_epoch {
                    // Start transition
                    state.phase = RotationPhase::Transition {
                        started_at: now,
                        ends_at: now + self.config.transition_period_secs,
                    };

                    let old_size = state.current_committee.members.len();
                    let new_size = state
                        .next_committee
                        .as_ref()
                        .map(|c| c.members.len())
                        .unwrap_or(0);

                    events.push(RotationEvent::TransitionStarted {
                        conversation_id_hash: *conversation_id_hash,
                        epoch_id: current_epoch,
                        old_committee_size: old_size,
                        new_committee_size: new_size,
                    });
                }
            }

            // Check if transition is complete
            if let RotationPhase::Transition {
                ends_at,
                started_at,
            } = state.phase
            {
                if now >= ends_at {
                    // Complete rotation
                    if let Some(next) = state.next_committee.take() {
                        state.previous_committee =
                            Some(std::mem::replace(&mut state.current_committee, next));
                        state.phase = RotationPhase::PostRotation { started_at: now };
                        state.last_rotation_at = now;
                        state.stats.total_rotations += 1;
                        state.stats.successful_transitions += 1;

                        let duration_ms = (now - started_at) * 1000;
                        events.push(RotationEvent::RotationCompleted {
                            conversation_id_hash: *conversation_id_hash,
                            epoch_id: current_epoch,
                            duration_ms,
                        });
                    }
                }
            }

            // Check if post-rotation cleanup is done
            if let RotationPhase::PostRotation { started_at } = state.phase {
                // After grace period, clean up previous committee
                if now >= started_at + EPOCH_GRACE_PERIOD_SECS as u64 {
                    state.previous_committee = None;
                    state.phase = RotationPhase::Stable;
                }
            }
        }

        // Emit events to listeners
        for event in &events {
            self.emit_event(event.clone()).await;
        }

        events
    }

    /// Set the next committee for a conversation (called after VRF selection)
    pub async fn set_next_committee(
        &self,
        conversation_id_hash: &[u8; 32],
        next_committee: CommitteeInfo,
    ) -> Result<()> {
        let mut states = self.states.write().await;
        let state = states
            .get_mut(conversation_id_hash)
            .ok_or_else(|| Error::not_found("Conversation not registered for rotation"))?;

        state.next_committee = Some(next_committee);
        Ok(())
    }

    /// Mark DKG as complete for next committee
    pub async fn mark_dkg_complete(
        &self,
        conversation_id_hash: &[u8; 32],
        aggregate_public_key: [u8; 32],
    ) -> Result<()> {
        let mut states = self.states.write().await;
        let state = states
            .get_mut(conversation_id_hash)
            .ok_or_else(|| Error::not_found("Conversation not registered for rotation"))?;

        if let Some(next) = &mut state.next_committee {
            next.aggregate_public_key = aggregate_public_key;
            next.dkg_complete = true;

            self.emit_event(RotationEvent::DkgCompleted {
                conversation_id_hash: *conversation_id_hash,
                epoch_id: next.epoch_id,
                aggregate_public_key,
            })
            .await;
        }

        Ok(())
    }

    /// Handle rotation failure (activate fallback)
    pub async fn handle_rotation_failure(
        &self,
        conversation_id_hash: &[u8; 32],
        reason: String,
    ) -> Result<()> {
        let mut states = self.states.write().await;
        let state = states
            .get_mut(conversation_id_hash)
            .ok_or_else(|| Error::not_found("Conversation not registered for rotation"))?;

        state.stats.failed_transitions += 1;

        // Clear next committee and return to stable with current
        state.next_committee = None;
        state.phase = RotationPhase::Stable;

        let current_epoch = current_epoch_id();

        self.emit_event(RotationEvent::RotationFailed {
            conversation_id_hash: *conversation_id_hash,
            epoch_id: current_epoch,
            reason,
        })
        .await;

        Ok(())
    }

    /// Get rotation state for a conversation
    pub async fn get_state(
        &self,
        conversation_id_hash: &[u8; 32],
    ) -> Option<CommitteeRotationState> {
        let states = self.states.read().await;
        states.get(conversation_id_hash).cloned()
    }

    /// Check if this relay is in the current or transitioning committee
    pub async fn is_local_relay_active(&self, conversation_id_hash: &[u8; 32]) -> bool {
        let states = self.states.read().await;
        states
            .get(conversation_id_hash)
            .map(|s| s.is_relay_active(&self.local_relay_id))
            .unwrap_or(false)
    }

    /// Get current active committee for a conversation
    pub async fn get_current_committee(
        &self,
        conversation_id_hash: &[u8; 32],
    ) -> Option<CommitteeInfo> {
        let states = self.states.read().await;
        states
            .get(conversation_id_hash)
            .map(|s| s.current_committee.clone())
    }

    /// Record relay join during rotation
    pub async fn record_relay_join(&self, relay_id: [u8; 32]) {
        let mut states = self.states.write().await;
        for (conversation_id_hash, state) in states.iter_mut() {
            if matches!(
                state.phase,
                RotationPhase::PreRotation { .. } | RotationPhase::Transition { .. }
            ) {
                state.stats.relay_churn_count += 1;

                // Don't await here, just emit
                let event = RotationEvent::RelayJoined {
                    relay_id,
                    epoch_id: current_epoch_id(),
                };
                // Would emit event here
                let _ = event;
            }
        }
    }

    /// Record relay leave during rotation
    pub async fn record_relay_leave(&self, relay_id: [u8; 32]) {
        let mut states = self.states.write().await;
        for (conversation_id_hash, state) in states.iter_mut() {
            if matches!(
                state.phase,
                RotationPhase::PreRotation { .. } | RotationPhase::Transition { .. }
            ) {
                state.stats.relay_churn_count += 1;
            }
        }
    }

    /// Add event listener
    pub async fn add_event_listener<F>(&self, listener: F)
    where
        F: Fn(RotationEvent) + Send + Sync + 'static,
    {
        let mut listeners = self.event_listeners.write().await;
        listeners.push(Box::new(listener));
    }

    /// Emit event to all listeners
    async fn emit_event(&self, event: RotationEvent) {
        let listeners = self.event_listeners.read().await;
        for listener in listeners.iter() {
            listener(event.clone());
        }
    }

    /// Get configuration
    pub fn config(&self) -> &RotationConfig {
        &self.config
    }

    /// Start background rotation check task
    pub fn start_rotation_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let check_interval = Duration::from_secs(5);
            loop {
                self.check_and_rotate().await;
                tokio::time::sleep(check_interval).await;
            }
        })
    }
}

/// Helper to create a committee info from relay selection
pub fn create_committee_info(
    epoch_id: u64,
    members: Vec<[u8; 32]>,
    member_public_keys: Vec<[u8; 32]>,
    threshold: u8,
    geo_distribution: HashMap<String, usize>,
    vrf_proof: Option<[u8; 64]>,
) -> CommitteeInfo {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    CommitteeInfo {
        epoch_id,
        members,
        member_public_keys,
        aggregate_public_key: [0u8; 32], // Will be set after DKG
        threshold,
        vrf_proof,
        geo_distribution,
        formed_at: now,
        dkg_complete: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_committee(epoch_id: u64, size: usize) -> CommitteeInfo {
        let members: Vec<[u8; 32]> = (0..size).map(|i| [i as u8; 32]).collect();
        let public_keys = members.clone();

        CommitteeInfo {
            epoch_id,
            members,
            member_public_keys: public_keys,
            aggregate_public_key: [0xAB; 32],
            threshold: ((size * 2 + 2) / 3) as u8, // 2/3 + 1 threshold
            vrf_proof: None,
            geo_distribution: HashMap::new(),
            formed_at: 0,
            dkg_complete: true,
        }
    }

    #[test]
    fn test_committee_contains() {
        let committee = create_test_committee(1, 7);
        assert!(committee.contains(&[0; 32]));
        assert!(committee.contains(&[6; 32]));
        assert!(!committee.contains(&[7; 32]));
    }

    #[test]
    fn test_committee_member_index() {
        let committee = create_test_committee(1, 7);
        assert_eq!(committee.member_index(&[0; 32]), Some(0));
        assert_eq!(committee.member_index(&[3; 32]), Some(3));
        assert_eq!(committee.member_index(&[10; 32]), None);
    }

    #[test]
    fn test_rotation_state_creation() {
        let committee = create_test_committee(1, 7);
        let state = CommitteeRotationState::new([0xAB; 32], ConversationType::Direct, committee);

        assert!(!state.is_in_transition());
        assert_eq!(state.current_threshold(), 5);
    }

    #[test]
    fn test_relay_active_check() {
        let committee = create_test_committee(1, 7);
        let state = CommitteeRotationState::new([0xAB; 32], ConversationType::Direct, committee);

        assert!(state.is_relay_active(&[0; 32]));
        assert!(state.is_relay_active(&[5; 32]));
        assert!(!state.is_relay_active(&[10; 32]));
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = CommitteeRotationManager::new([1; 32]);
        assert!(manager.config.auto_rotate);
    }

    #[tokio::test]
    async fn test_register_conversation() {
        let manager = CommitteeRotationManager::new([1; 32]);
        let committee = create_test_committee(current_epoch_id(), 7);

        manager
            .register_conversation([0xAB; 32], ConversationType::Direct, committee)
            .await;

        let state = manager.get_state(&[0xAB; 32]).await;
        assert!(state.is_some());
    }

    #[tokio::test]
    async fn test_unregister_conversation() {
        let manager = CommitteeRotationManager::new([1; 32]);
        let committee = create_test_committee(current_epoch_id(), 7);

        manager
            .register_conversation([0xAB; 32], ConversationType::Direct, committee)
            .await;

        manager.unregister_conversation(&[0xAB; 32]).await;

        let state = manager.get_state(&[0xAB; 32]).await;
        assert!(state.is_none());
    }

    #[tokio::test]
    async fn test_set_next_committee() {
        let manager = CommitteeRotationManager::new([1; 32]);
        let current = create_test_committee(current_epoch_id(), 7);
        let next = create_test_committee(current_epoch_id() + 1, 7);

        manager
            .register_conversation([0xAB; 32], ConversationType::Direct, current)
            .await;

        let result = manager.set_next_committee(&[0xAB; 32], next).await;
        assert!(result.is_ok());

        let state = manager.get_state(&[0xAB; 32]).await.unwrap();
        assert!(state.next_committee.is_some());
    }

    #[tokio::test]
    async fn test_is_local_relay_active() {
        let manager = CommitteeRotationManager::new([0; 32]); // Local relay is [0; 32]
        let committee = create_test_committee(current_epoch_id(), 7);

        manager
            .register_conversation([0xAB; 32], ConversationType::Direct, committee)
            .await;

        // [0; 32] is in the test committee
        assert!(manager.is_local_relay_active(&[0xAB; 32]).await);
    }

    #[tokio::test]
    async fn test_local_relay_not_active() {
        let manager = CommitteeRotationManager::new([100; 32]); // Local relay not in committee
        let committee = create_test_committee(current_epoch_id(), 7);

        manager
            .register_conversation([0xAB; 32], ConversationType::Direct, committee)
            .await;

        // [100; 32] is not in the test committee
        assert!(!manager.is_local_relay_active(&[0xAB; 32]).await);
    }
}
