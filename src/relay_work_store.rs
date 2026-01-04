//! Relay Work Event Store for Validator Nodes
//!
//! Tracks relay work events (Proof of Delivery, Proof of Relay Work, Proof of Transit)
//! for epoch-based reward distribution. Events are accumulated during each epoch
//! and consumed when rewards are distributed.

use dchat_blockchain::{RegisteredRelay, RelayWorkEvent, WorkEventType};
use dchat_core::types::UserId;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use tracing::{debug, info, warn};

/// Genesis timestamp for block height calculations (placeholder - should come from chain config)
/// This is approximately Jan 1, 2025 00:00:00 UTC
pub const DEFAULT_GENESIS_TIMESTAMP: u64 = 1735689600;

/// Default block time in seconds
pub const DEFAULT_BLOCK_TIME_SECS: u64 = 6;

/// Store for relay work events during an epoch
///
/// Validators use this to track relay activity for reward distribution.
/// Events are deduplicated by event_id to prevent double-counting.
pub struct RelayWorkEventStore {
    /// Work events indexed by epoch
    events_by_epoch: RwLock<HashMap<u64, Vec<RelayWorkEvent>>>,
    /// Seen event IDs for deduplication (per epoch)
    seen_events: RwLock<HashMap<u64, HashSet<[u8; 32]>>>,
    /// Current epoch for event collection
    current_epoch: RwLock<u64>,
    /// Maximum epochs to retain (for memory management)
    max_retained_epochs: u64,
}

impl RelayWorkEventStore {
    /// Create a new work event store
    pub fn new() -> Self {
        Self {
            events_by_epoch: RwLock::new(HashMap::new()),
            seen_events: RwLock::new(HashMap::new()),
            current_epoch: RwLock::new(0),
            max_retained_epochs: 5,
        }
    }

    /// Create a work event store with custom retention
    pub fn with_retention(max_retained_epochs: u64) -> Self {
        Self {
            events_by_epoch: RwLock::new(HashMap::new()),
            seen_events: RwLock::new(HashMap::new()),
            current_epoch: RwLock::new(0),
            max_retained_epochs,
        }
    }

    /// Set the current epoch (called on epoch transitions)
    pub fn set_current_epoch(&self, epoch: u64) {
        let mut current = self.current_epoch.write().unwrap();
        if epoch > *current {
            info!(
                "📊 RelayWorkEventStore: advancing epoch {} -> {}",
                *current, epoch
            );
            *current = epoch;
            drop(current);
            self.cleanup_old_epochs(epoch);
        }
    }

    /// Get the current epoch
    pub fn current_epoch(&self) -> u64 {
        *self.current_epoch.read().unwrap()
    }

    /// Record a relay work event
    ///
    /// Returns true if the event was recorded (not a duplicate)
    pub fn record_event(&self, event: RelayWorkEvent) -> bool {
        let epoch = self.epoch_for_block(event.block_height);

        // Check for duplicate
        {
            let seen = self.seen_events.read().unwrap();
            if let Some(epoch_seen) = seen.get(&epoch) {
                if epoch_seen.contains(&event.event_id) {
                    debug!(
                        "Duplicate work event ignored: relay={} type={:?}",
                        event.relay_id, event.event_type
                    );
                    return false;
                }
            }
        }

        // Record the event
        {
            let mut events = self.events_by_epoch.write().unwrap();
            let mut seen = self.seen_events.write().unwrap();

            seen.entry(epoch).or_default().insert(event.event_id);
            events.entry(epoch).or_default().push(event.clone());
        }

        debug!(
            "Recorded work event: relay={} type={:?} block={}",
            event.relay_id, event.event_type, event.block_height
        );
        true
    }

    /// Record a Proof of Delivery event
    pub fn record_proof_of_delivery(
        &self,
        relay_id: &str,
        block_height: u64,
        proof_hash: [u8; 32],
    ) -> bool {
        self.record_event(RelayWorkEvent {
            relay_id: relay_id.to_string(),
            block_height,
            event_id: proof_hash,
            event_type: WorkEventType::ProofOfDelivery,
        })
    }

    /// Record a Proof of Relay Work event
    pub fn record_proof_of_relay_work(
        &self,
        relay_id: &str,
        block_height: u64,
        work_hash: [u8; 32],
    ) -> bool {
        self.record_event(RelayWorkEvent {
            relay_id: relay_id.to_string(),
            block_height,
            event_id: work_hash,
            event_type: WorkEventType::ProofOfRelayWork,
        })
    }

    /// Record a Proof of Transit event
    pub fn record_proof_of_transit(
        &self,
        relay_id: &str,
        block_height: u64,
        transit_hash: [u8; 32],
    ) -> bool {
        self.record_event(RelayWorkEvent {
            relay_id: relay_id.to_string(),
            block_height,
            event_id: transit_hash,
            event_type: WorkEventType::ProofOfTransit,
        })
    }

    /// Get all work events for an epoch
    pub fn get_events_for_epoch(&self, epoch: u64) -> Vec<RelayWorkEvent> {
        let events = self.events_by_epoch.read().unwrap();
        events.get(&epoch).cloned().unwrap_or_default()
    }

    /// Get event count for an epoch
    pub fn event_count_for_epoch(&self, epoch: u64) -> usize {
        let events = self.events_by_epoch.read().unwrap();
        events.get(&epoch).map(|e| e.len()).unwrap_or(0)
    }

    /// Get event count per relay for an epoch
    pub fn events_per_relay_for_epoch(&self, epoch: u64) -> HashMap<String, usize> {
        let events = self.events_by_epoch.read().unwrap();
        let mut counts = HashMap::new();
        if let Some(epoch_events) = events.get(&epoch) {
            for event in epoch_events {
                *counts.entry(event.relay_id.clone()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Calculate epoch from block height
    fn epoch_for_block(&self, block_height: u64) -> u64 {
        // EPOCH_LENGTH_BLOCKS = 1800 (from hardened_consensus::threshold_normalization)
        // This is ~1 hour at 2s blocks or ~3 hours at 6s blocks
        const EPOCH_LENGTH_BLOCKS: u64 = 1800;
        block_height / EPOCH_LENGTH_BLOCKS
    }

    /// Cleanup old epochs to prevent memory growth
    fn cleanup_old_epochs(&self, current_epoch: u64) {
        if current_epoch <= self.max_retained_epochs {
            return;
        }

        let cutoff = current_epoch - self.max_retained_epochs;

        let mut events = self.events_by_epoch.write().unwrap();
        let mut seen = self.seen_events.write().unwrap();

        let epochs_to_remove: Vec<u64> = events.keys().filter(|&&e| e < cutoff).cloned().collect();

        for epoch in epochs_to_remove {
            let removed_count = events.remove(&epoch).map(|e| e.len()).unwrap_or(0);
            seen.remove(&epoch);
            debug!(
                "Cleaned up {} work events from epoch {} (cutoff={})",
                removed_count, epoch, cutoff
            );
        }
    }

    /// Get statistics for monitoring
    pub fn stats(&self) -> RelayWorkStoreStats {
        let events = self.events_by_epoch.read().unwrap();
        let current = *self.current_epoch.read().unwrap();

        let total_events: usize = events.values().map(|e| e.len()).sum();
        let epochs_stored = events.len();
        let current_epoch_events = events.get(&current).map(|e| e.len()).unwrap_or(0);

        RelayWorkStoreStats {
            current_epoch: current,
            total_events,
            epochs_stored,
            current_epoch_events,
        }
    }
}

impl Default for RelayWorkEventStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for the relay work event store
#[derive(Debug, Clone)]
pub struct RelayWorkStoreStats {
    pub current_epoch: u64,
    pub total_events: usize,
    pub epochs_stored: usize,
    pub current_epoch_events: usize,
}

/// Relay Registry Store for tracking registered relays
///
/// Maintains the set of registered relays and their metadata for
/// epoch-based eligibility computation.
pub struct RelayRegistryStore {
    /// Registered relays indexed by relay_id
    relays: RwLock<HashMap<String, RegisteredRelay>>,
    /// Genesis timestamp for block height calculations
    genesis_timestamp: u64,
    /// Block time in seconds
    block_time_secs: u64,
}

impl RelayRegistryStore {
    /// Create a new relay registry store
    pub fn new() -> Self {
        Self {
            relays: RwLock::new(HashMap::new()),
            genesis_timestamp: DEFAULT_GENESIS_TIMESTAMP,
            block_time_secs: DEFAULT_BLOCK_TIME_SECS,
        }
    }

    /// Create with custom genesis parameters
    pub fn with_genesis(genesis_timestamp: u64, block_time_secs: u64) -> Self {
        Self {
            relays: RwLock::new(HashMap::new()),
            genesis_timestamp,
            block_time_secs,
        }
    }

    /// Register a new relay or update existing registration
    pub fn register_relay(
        &self,
        relay_id: String,
        operator: UserId,
        stake: u64,
        registered_at_block: u64,
    ) {
        let relay = RegisteredRelay {
            relay_id: relay_id.clone(),
            operator,
            stake,
            registered_at_block,
            is_suspended: false,
        };

        let mut relays = self.relays.write().unwrap();
        relays.insert(relay_id.clone(), relay);
        info!(
            "📝 Registered relay: {} (stake={}, block={})",
            relay_id, stake, registered_at_block
        );
    }

    /// Update relay stake
    pub fn update_stake(&self, relay_id: &str, new_stake: u64) {
        let mut relays = self.relays.write().unwrap();
        if let Some(relay) = relays.get_mut(relay_id) {
            relay.stake = new_stake;
            debug!("Updated stake for relay {}: {}", relay_id, new_stake);
        }
    }

    /// Suspend a relay (e.g., for slashing)
    pub fn suspend_relay(&self, relay_id: &str) {
        let mut relays = self.relays.write().unwrap();
        if let Some(relay) = relays.get_mut(relay_id) {
            relay.is_suspended = true;
            warn!("⚠️ Relay {} suspended", relay_id);
        }
    }

    /// Unsuspend a relay
    pub fn unsuspend_relay(&self, relay_id: &str) {
        let mut relays = self.relays.write().unwrap();
        if let Some(relay) = relays.get_mut(relay_id) {
            relay.is_suspended = false;
            info!("✅ Relay {} unsuspended", relay_id);
        }
    }

    /// Remove a relay from the registry
    pub fn unregister_relay(&self, relay_id: &str) {
        let mut relays = self.relays.write().unwrap();
        if relays.remove(relay_id).is_some() {
            info!("🗑️ Unregistered relay: {}", relay_id);
        }
    }

    /// Get all registered relays
    pub fn get_all_relays(&self) -> Vec<RegisteredRelay> {
        let relays = self.relays.read().unwrap();
        relays.values().cloned().collect()
    }

    /// Get a specific relay
    pub fn get_relay(&self, relay_id: &str) -> Option<RegisteredRelay> {
        let relays = self.relays.read().unwrap();
        relays.get(relay_id).cloned()
    }

    /// Get count of registered relays
    pub fn relay_count(&self) -> usize {
        self.relays.read().unwrap().len()
    }

    /// Get count of active (non-suspended) relays
    pub fn active_relay_count(&self) -> usize {
        let relays = self.relays.read().unwrap();
        relays.values().filter(|r| !r.is_suspended).count()
    }

    /// Sync from RelayNetworkManager
    ///
    /// Imports relay registration data from the network manager into
    /// the format required for eligibility computation.
    pub fn sync_from_relay_network<F>(&self, current_block: u64, get_relays: F)
    where
        F: Fn() -> Vec<(String, UserId, u64, u64, bool)>,
    {
        let relay_data = get_relays();
        let mut relays = self.relays.write().unwrap();

        // Update registry with network manager data
        for (relay_id, operator, stake, registered_at_block, is_suspended) in relay_data {
            let entry = relays
                .entry(relay_id.clone())
                .or_insert_with(|| RegisteredRelay {
                    relay_id: relay_id.clone(),
                    operator: operator.clone(),
                    stake,
                    registered_at_block,
                    is_suspended,
                });

            // Update mutable fields
            entry.stake = stake;
            entry.is_suspended = is_suspended;
        }

        debug!(
            "Synced relay registry: {} relays at block {}",
            relays.len(),
            current_block
        );
    }
}

impl Default for RelayRegistryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_work_event_recording() {
        let store = RelayWorkEventStore::new();

        let event_id = [1u8; 32];
        let recorded = store.record_proof_of_delivery("relay1", 100, event_id);
        assert!(recorded);

        // Duplicate should be rejected
        let recorded_again = store.record_proof_of_delivery("relay1", 100, event_id);
        assert!(!recorded_again);

        // Different event should be recorded
        let different_id = [2u8; 32];
        let recorded_different = store.record_proof_of_delivery("relay1", 100, different_id);
        assert!(recorded_different);
    }

    #[test]
    fn test_relay_registry() {
        let store = RelayRegistryStore::new();
        let operator = UserId(Uuid::new_v4());

        store.register_relay("relay1".to_string(), operator.clone(), 1000, 50);

        assert_eq!(store.relay_count(), 1);
        assert_eq!(store.active_relay_count(), 1);

        let relay = store.get_relay("relay1").unwrap();
        assert_eq!(relay.stake, 1000);
        assert!(!relay.is_suspended);

        store.suspend_relay("relay1");
        assert_eq!(store.active_relay_count(), 0);

        store.unsuspend_relay("relay1");
        assert_eq!(store.active_relay_count(), 1);
    }

    #[test]
    fn test_epoch_cleanup() {
        let store = RelayWorkEventStore::with_retention(2);

        // Record events in epochs 0, 1, 2
        for epoch in 0..3 {
            let block_height = epoch * 14400; // EPOCH_LENGTH_BLOCKS
            store.record_proof_of_delivery("relay1", block_height, [epoch as u8; 32]);
        }

        assert_eq!(store.event_count_for_epoch(0), 1);
        assert_eq!(store.event_count_for_epoch(1), 1);
        assert_eq!(store.event_count_for_epoch(2), 1);

        // Advance to epoch 5, which should cleanup epochs 0, 1, 2
        store.set_current_epoch(5);

        // Old epochs should be cleaned up
        assert_eq!(store.event_count_for_epoch(0), 0);
        assert_eq!(store.event_count_for_epoch(1), 0);
        assert_eq!(store.event_count_for_epoch(2), 0);
    }
}
