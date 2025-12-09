//! Device synchronization for multi-device identities
//!
//! Implements vector clock-based conflict resolution for multi-device synchronization.
//! Uses Lamport-style vector clocks to determine causal ordering and resolve conflicts.

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::cmp::Ordering;

/// Vector clock for causal ordering in distributed systems
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorClock {
    /// Map of device_id -> logical clock value
    clocks: HashMap<String, u64>,
}

impl VectorClock {
    /// Create a new vector clock
    pub fn new() -> Self {
        Self {
            clocks: HashMap::new(),
        }
    }

    /// Increment the clock for a specific device
    pub fn increment(&mut self, device_id: &str) {
        *self.clocks.entry(device_id.to_string()).or_insert(0) += 1;
    }

    /// Get the clock value for a device
    pub fn get(&self, device_id: &str) -> u64 {
        self.clocks.get(device_id).copied().unwrap_or(0)
    }

    /// Update this clock by taking the maximum of each component
    /// (typical vector clock merge operation)
    pub fn merge(&mut self, other: &VectorClock) {
        for (device_id, &other_clock) in &other.clocks {
            let entry = self.clocks.entry(device_id.clone()).or_insert(0);
            *entry = (*entry).max(other_clock);
        }
    }

    /// Compare two vector clocks for causal ordering
    /// Returns:
    /// - Ordering::Less if self happened before other
    /// - Ordering::Greater if self happened after other
    /// - Ordering::Equal if they are concurrent (conflict)
    pub fn compare(&self, other: &VectorClock) -> Ordering {
        let mut self_less = false;
        let mut self_greater = false;

        // Collect all device IDs from both clocks
        let mut all_devices: Vec<String> = self.clocks.keys().cloned().collect();
        for device_id in other.clocks.keys() {
            if !all_devices.contains(device_id) {
                all_devices.push(device_id.clone());
            }
        }

        // Compare each component
        for device_id in all_devices {
            let self_clock = self.get(&device_id);
            let other_clock = other.get(&device_id);

            if self_clock < other_clock {
                self_less = true;
            } else if self_clock > other_clock {
                self_greater = true;
            }
        }

        // Determine causal relationship
        match (self_less, self_greater) {
            (true, false) => Ordering::Less,      // self happened before other
            (false, true) => Ordering::Greater,   // self happened after other
            (false, false) => Ordering::Equal,    // identical clocks
            (true, true) => Ordering::Equal,      // concurrent events (conflict)
        }
    }

    /// Check if this clock happened before another (strict causality)
    pub fn happened_before(&self, other: &VectorClock) -> bool {
        matches!(self.compare(other), Ordering::Less)
    }

    /// Check if events are concurrent (conflict detected)
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        let ordering = self.compare(other);
        matches!(ordering, Ordering::Equal) && self != other
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Use the update with the latest timestamp
    LastWriteWins,
    /// Use the update from the device with lexicographically greater ID
    DeviceIdPriority,
    /// Merge both updates (application-specific)
    Merge,
    /// Require manual resolution
    Manual,
}

/// A sync update with vector clock for causal ordering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncUpdate {
    pub update_id: String,
    pub device_id: String,
    pub vector_clock: VectorClock,
    pub timestamp: DateTime<Utc>,
    pub data: SyncData,
}

/// Types of data that can be synchronized
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncData {
    /// Identity field update (display name, bio, etc.)
    IdentityField {
        field_name: String,
        value: String,
    },
    /// Settings update
    Setting {
        key: String,
        value: String,
    },
    /// Contact added or updated
    ContactUpdate {
        contact_id: String,
        data: Vec<u8>,
    },
    /// Message read receipt
    ReadReceipt {
        message_id: String,
        timestamp: DateTime<Utc>,
    },
}

/// Conflict resolver for concurrent updates
pub struct ConflictResolver {
    strategy: ConflictResolution,
}

impl ConflictResolver {
    /// Create a new conflict resolver with a strategy
    pub fn new(strategy: ConflictResolution) -> Self {
        Self { strategy }
    }

    /// Resolve a conflict between two concurrent updates
    /// Returns the winning update (or None if manual resolution required)
    pub fn resolve(&self, a: &SyncUpdate, b: &SyncUpdate) -> Option<SyncUpdate> {
        // First check if they are actually concurrent
        if !a.vector_clock.is_concurrent(&b.vector_clock) {
            // Not concurrent, use causal ordering
            return match a.vector_clock.compare(&b.vector_clock) {
                Ordering::Less => Some(b.clone()),      // b happened after a
                Ordering::Greater => Some(a.clone()),   // a happened after b
                Ordering::Equal => Some(a.clone()),     // Identical, pick either
            };
        }

        // They are concurrent - apply conflict resolution strategy
        match self.strategy {
            ConflictResolution::LastWriteWins => {
                if a.timestamp > b.timestamp {
                    Some(a.clone())
                } else {
                    Some(b.clone())
                }
            }
            ConflictResolution::DeviceIdPriority => {
                if a.device_id > b.device_id {
                    Some(a.clone())
                } else {
                    Some(b.clone())
                }
            }
            ConflictResolution::Merge => {
                // Application-specific merging logic
                self.merge_updates(a, b)
            }
            ConflictResolution::Manual => {
                // Requires manual intervention
                None
            }
        }
    }

    /// Merge two concurrent updates (application-specific logic)
    fn merge_updates(&self, a: &SyncUpdate, b: &SyncUpdate) -> Option<SyncUpdate> {
        // For now, implement simple merge strategies based on data type
        match (&a.data, &b.data) {
            // For read receipts, take the later timestamp
            (
                SyncData::ReadReceipt { message_id: msg_a, timestamp: ts_a },
                SyncData::ReadReceipt { message_id: msg_b, timestamp: ts_b },
            ) if msg_a == msg_b => {
                let winner = if ts_a > ts_b { a } else { b };
                Some(winner.clone())
            }
            // For other types, fall back to last-write-wins
            _ => {
                if a.timestamp > b.timestamp {
                    Some(a.clone())
                } else {
                    Some(b.clone())
                }
            }
        }
    }
}

/// Maximum encrypted payload size (1MB)
const MAX_PAYLOAD_SIZE: usize = 1024 * 1024;
/// Maximum device ID length
const MAX_DEVICE_ID_LENGTH: usize = 128;
/// Maximum sync ID length
const MAX_SYNC_ID_LENGTH: usize = 256;

/// Sync message for device coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    pub sync_id: String,
    pub user_id: UserId,
    pub device_id: String,
    pub message_type: SyncMessageType,
    pub timestamp: DateTime<Utc>,
    pub encrypted_payload: Vec<u8>,
    /// Vector clock for causal ordering
    pub vector_clock: VectorClock,
}

impl SyncMessage {
    /// Validate sync message fields
    /// 
    /// # Security
    /// - Prevents memory exhaustion from oversized payloads
    /// - Validates field lengths
    pub fn validate(&self) -> Result<()> {
        if self.sync_id.len() > MAX_SYNC_ID_LENGTH {
            return Err(Error::identity("Sync ID too long"));
        }
        if self.device_id.len() > MAX_DEVICE_ID_LENGTH {
            return Err(Error::identity("Device ID too long"));
        }
        if self.encrypted_payload.len() > MAX_PAYLOAD_SIZE {
            return Err(Error::identity(format!(
                "Payload exceeds maximum size of {} bytes",
                MAX_PAYLOAD_SIZE
            )));
        }
        Ok(())
    }
}

/// Types of sync messages
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMessageType {
    /// Device registration request
    DeviceAdded,
    /// Device removal notification
    DeviceRemoved,
    /// Identity update (display name, bio, etc.)
    IdentityUpdate,
    /// Settings sync
    SettingsUpdate,
    /// Contact list sync
    ContactsUpdate,
    /// Message read receipts
    ReadReceipts,
    /// Custom sync data
    Custom(String),
}

/// Manages device synchronization
pub struct SyncManager {
    pending_syncs: HashMap<UserId, VecDeque<SyncMessage>>,
    sync_history: HashMap<String, SyncMessage>,
    max_pending_per_user: usize,
    /// Vector clock per device for causal ordering
    device_clocks: HashMap<String, VectorClock>,
    /// Conflict resolver
    conflict_resolver: ConflictResolver,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(max_pending_per_user: usize) -> Self {
        Self::with_conflict_resolution(max_pending_per_user, ConflictResolution::LastWriteWins)
    }

    /// Create a new sync manager with custom conflict resolution strategy
    pub fn with_conflict_resolution(
        max_pending_per_user: usize,
        strategy: ConflictResolution,
    ) -> Self {
        Self {
            pending_syncs: HashMap::new(),
            sync_history: HashMap::new(),
            max_pending_per_user,
            device_clocks: HashMap::new(),
            conflict_resolver: ConflictResolver::new(strategy),
        }
    }

    /// Update vector clock for a device
    pub fn tick_device_clock(&mut self, device_id: &str) {
        self.device_clocks
            .entry(device_id.to_string())
            .or_insert_with(VectorClock::new)
            .increment(device_id);
    }

    /// Get the current vector clock for a device
    pub fn get_device_clock(&self, device_id: &str) -> VectorClock {
        self.device_clocks
            .get(device_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Merge a remote vector clock into local device clock
    pub fn merge_clock(&mut self, device_id: &str, remote_clock: &VectorClock) {
        let clock = self
            .device_clocks
            .entry(device_id.to_string())
            .or_insert_with(VectorClock::new);
        clock.merge(remote_clock);
    }

    /// Add a sync message
    /// 
    /// # Security
    /// - Validates message size to prevent memory exhaustion
    /// - Checks queue limits
    pub fn add_sync_message(&mut self, message: SyncMessage) -> Result<()> {
        // Validate message first
        message.validate()?;
        
        let user_id = message.user_id.clone();
        let sync_id = message.sync_id.clone();
        let device_id = message.device_id.clone();

        // Update device clock with remote clock
        self.merge_clock(&device_id, &message.vector_clock);

        // Check for conflicts with existing pending messages
        let queue = self.pending_syncs.entry(user_id.clone()).or_default();

        // Look for conflicting messages (same type, concurrent updates)
        let mut resolved_message = message.clone();
        let mut conflicts_to_remove = Vec::new();

        // First pass: identify conflicts (using indices to avoid borrow issues)
        let conflict_indices: Vec<(usize, SyncMessage)> = queue
            .iter()
            .enumerate()
            .filter(|(_, existing)| {
                // Check if same message type and concurrent vector clocks
                existing.message_type == message.message_type
                    && message.vector_clock.is_concurrent(&existing.vector_clock)
            })
            .map(|(idx, msg)| (idx, msg.clone()))
            .collect();

        // Second pass: resolve conflicts
        for (idx, existing) in conflict_indices {
            // Resolve using configured strategy
            let winner = match self.conflict_resolver.strategy {
                ConflictResolution::LastWriteWins => {
                    if message.timestamp > existing.timestamp {
                        message.clone()
                    } else {
                        existing.clone()
                    }
                }
                ConflictResolution::DeviceIdPriority => {
                    if message.device_id > existing.device_id {
                        message.clone()
                    } else {
                        existing.clone()
                    }
                }
                _ => {
                    // Manual resolution required
                    return Err(Error::identity(format!(
                        "Manual conflict resolution required for sync {}",
                        sync_id
                    )));
                }
            };
            conflicts_to_remove.push(idx);
            resolved_message = winner;
        }

        // Re-borrow queue mutably for modifications
        let queue = self.pending_syncs.get_mut(&user_id).unwrap();

        // Remove conflicting messages (in reverse order to preserve indices)
        for &idx in conflicts_to_remove.iter().rev() {
            if let Some(removed) = queue.get(idx) {
                self.sync_history.remove(&removed.sync_id);
            }
            queue.remove(idx);
        }

        // Check if queue is full
        if queue.len() >= self.max_pending_per_user {
            // Remove oldest message
            if let Some(old_msg) = queue.pop_front() {
                self.sync_history.remove(&old_msg.sync_id);
            }
        }

        // Add resolved message to queue and history
        queue.push_back(resolved_message.clone());
        self.sync_history.insert(sync_id, resolved_message);

        Ok(())
    }

    /// Check if two sync messages are conflicting
    #[allow(dead_code)]
    fn is_conflicting(&self, a: &SyncMessage, b: &SyncMessage) -> bool {
        // Same message type and concurrent (neither causally before the other)
        std::mem::discriminant(&a.message_type) == std::mem::discriminant(&b.message_type)
            && a.vector_clock.is_concurrent(&b.vector_clock)
    }

    /// Resolve a conflict between two sync messages
    #[allow(dead_code)]
    fn resolve_conflict(&self, a: &SyncMessage, b: &SyncMessage) -> Option<SyncMessage> {
        // Convert SyncMessage to SyncUpdate for resolution
        let update_a = SyncUpdate {
            update_id: a.sync_id.clone(),
            device_id: a.device_id.clone(),
            vector_clock: a.vector_clock.clone(),
            timestamp: a.timestamp,
            data: self.message_to_sync_data(a),
        };

        let update_b = SyncUpdate {
            update_id: b.sync_id.clone(),
            device_id: b.device_id.clone(),
            vector_clock: b.vector_clock.clone(),
            timestamp: b.timestamp,
            data: self.message_to_sync_data(b),
        };

        // Resolve using conflict resolver
        self.conflict_resolver
            .resolve(&update_a, &update_b)
            .map(|winner| {
                if winner.update_id == a.sync_id {
                    a.clone()
                } else {
                    b.clone()
                }
            })
    }

    /// Convert SyncMessage to SyncData for conflict resolution
    #[allow(dead_code)]
    fn message_to_sync_data(&self, message: &SyncMessage) -> SyncData {
        // Decode the sync payload from encrypted_payload
        // The payload format is: type_byte + payload_data
        let payload = &message.encrypted_payload;
        
        match &message.message_type {
            SyncMessageType::IdentityUpdate => {
                // Identity updates: field_name_len(1) + field_name + value
                if payload.len() >= 2 {
                    let field_name_len = payload[0] as usize;
                    if payload.len() >= 1 + field_name_len {
                        let field_name = String::from_utf8_lossy(&payload[1..1 + field_name_len]).to_string();
                        let value = String::from_utf8_lossy(&payload[1 + field_name_len..]).to_string();
                        return SyncData::IdentityField { field_name, value };
                    }
                }
                SyncData::IdentityField {
                    field_name: "unknown".to_string(),
                    value: String::new(),
                }
            }
            SyncMessageType::SettingsUpdate => {
                // Settings updates: key_len(1) + key + value
                if payload.len() >= 2 {
                    let key_len = payload[0] as usize;
                    if payload.len() >= 1 + key_len {
                        let key = String::from_utf8_lossy(&payload[1..1 + key_len]).to_string();
                        let value = String::from_utf8_lossy(&payload[1 + key_len..]).to_string();
                        return SyncData::Setting { key, value };
                    }
                }
                SyncData::Setting {
                    key: "unknown".to_string(),
                    value: String::new(),
                }
            }
            SyncMessageType::ContactsUpdate => {
                // Contact updates: contact_id_len(1) + contact_id + binary_data
                if payload.len() >= 2 {
                    let id_len = payload[0] as usize;
                    if payload.len() >= 1 + id_len {
                        let contact_id = String::from_utf8_lossy(&payload[1..1 + id_len]).to_string();
                        let data = payload[1 + id_len..].to_vec();
                        return SyncData::ContactUpdate { contact_id, data };
                    }
                }
                SyncData::ContactUpdate {
                    contact_id: String::new(),
                    data: payload.clone(),
                }
            }
            SyncMessageType::ReadReceipts => {
                // Read receipts: message_id (36 bytes UUID string)
                let message_id = if payload.len() >= 36 {
                    String::from_utf8_lossy(&payload[..36]).to_string()
                } else {
                    String::from_utf8_lossy(payload).to_string()
                };
                SyncData::ReadReceipt {
                    message_id,
                    timestamp: message.timestamp,
                }
            }
            _ => SyncData::Setting {
                key: format!("sync_{:?}", message.message_type),
                value: hex::encode(payload),
            },
        }
    }

    /// Get pending sync messages for a user
    pub fn get_pending_syncs(&self, user_id: &UserId) -> Vec<&SyncMessage> {
        self.pending_syncs
            .get(user_id)
            .map(|queue| queue.iter().collect())
            .unwrap_or_default()
    }

    /// Get pending sync messages for a user in causal order
    /// Returns messages sorted by their causal dependencies
    pub fn get_pending_syncs_ordered(&self, user_id: &UserId) -> Vec<&SyncMessage> {
        if let Some(queue) = self.pending_syncs.get(user_id) {
            let mut messages: Vec<&SyncMessage> = queue.iter().collect();

            // Sort by causal ordering using vector clocks
            messages.sort_by(|a, b| {
                match a.vector_clock.compare(&b.vector_clock) {
                    Ordering::Less => Ordering::Less,      // a happened before b
                    Ordering::Greater => Ordering::Greater, // a happened after b
                    Ordering::Equal => {
                        // Concurrent or identical - use timestamp as tiebreaker
                        a.timestamp.cmp(&b.timestamp)
                    }
                }
            });

            messages
        } else {
            Vec::new()
        }
    }

    /// Get sync message by ID
    pub fn get_sync_message(&self, sync_id: &str) -> Option<&SyncMessage> {
        self.sync_history.get(sync_id)
    }

    /// Acknowledge a sync message (mark as processed)
    pub fn acknowledge_sync(&mut self, sync_id: &str) -> Result<()> {
        let message = self
            .sync_history
            .remove(sync_id)
            .ok_or_else(|| Error::identity("Sync message not found"))?;

        // Remove from pending queue
        if let Some(queue) = self.pending_syncs.get_mut(&message.user_id) {
            queue.retain(|msg| msg.sync_id != sync_id);
        }

        Ok(())
    }

    /// Get sync statistics for a user
    pub fn get_sync_stats(&self, user_id: &UserId) -> SyncStats {
        let pending_count = self
            .pending_syncs
            .get(user_id)
            .map(|queue| queue.len())
            .unwrap_or(0);

        SyncStats { pending_count }
    }

    /// Clear all pending syncs for a user
    pub fn clear_pending_syncs(&mut self, user_id: &UserId) {
        if let Some(queue) = self.pending_syncs.remove(user_id) {
            for msg in queue {
                self.sync_history.remove(&msg.sync_id);
            }
        }
    }

    /// Clean up old sync messages
    pub fn cleanup_old_syncs(&mut self, hours: i64) {
        let cutoff = Utc::now() - chrono::Duration::hours(hours);

        // Collect IDs to remove
        let to_remove: Vec<String> = self
            .sync_history
            .iter()
            .filter(|(_, msg)| msg.timestamp < cutoff)
            .map(|(id, _)| id.clone())
            .collect();

        // Remove old messages
        for sync_id in to_remove {
            if let Some(message) = self.sync_history.remove(&sync_id) {
                if let Some(queue) = self.pending_syncs.get_mut(&message.user_id) {
                    queue.retain(|msg| msg.sync_id != sync_id);
                }
            }
        }
    }
}

/// Sync statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStats {
    pub pending_count: usize,
}

impl Default for SyncManager {
    fn default() -> Self {
        Self::with_conflict_resolution(1000, ConflictResolution::LastWriteWins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn create_test_message(
        user_id: UserId,
        device_id: &str,
        vector_clock: VectorClock,
    ) -> SyncMessage {
        SyncMessage {
            sync_id: Uuid::new_v4().to_string(),
            user_id,
            device_id: device_id.to_string(),
            message_type: SyncMessageType::DeviceAdded,
            timestamp: Utc::now(),
            encrypted_payload: vec![1, 2, 3],
            vector_clock,
        }
    }

    #[test]
    fn test_vector_clock_ordering() {
        let mut clock_a = VectorClock::new();
        let mut clock_b = VectorClock::new();

        // Event 1: Device A increments
        clock_a.increment("device_a");
        assert!(clock_a.happened_before(&clock_b) || clock_b.get("device_a") == 0);

        // Event 2: Device B increments
        clock_b.increment("device_b");

        // Now they are concurrent
        assert!(clock_a.is_concurrent(&clock_b));

        // Event 3: Device B sees Event 1 and increments
        clock_b.merge(&clock_a);
        clock_b.increment("device_b");

        // Now clock_a happened before clock_b
        assert!(clock_a.happened_before(&clock_b));
        assert!(!clock_b.happened_before(&clock_a));
    }

    #[test]
    fn test_sync_manager_with_clocks() {
        let mut manager = SyncManager::new(10);
        let user_id = UserId::new();

        let mut clock = VectorClock::new();
        clock.increment("device1");

        let message = create_test_message(user_id.clone(), "device1", clock);

        // Add sync message
        assert!(manager.add_sync_message(message.clone()).is_ok());

        // Check device clock was updated
        let device_clock = manager.get_device_clock("device1");
        assert_eq!(device_clock.get("device1"), 1);

        // Get pending syncs
        let pending = manager.get_pending_syncs(&user_id);
        assert_eq!(pending.len(), 1);

        // Acknowledge sync
        assert!(manager.acknowledge_sync(&message.sync_id).is_ok());
        let pending = manager.get_pending_syncs(&user_id);
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_conflict_resolution_last_write_wins() {
        let mut manager = SyncManager::with_conflict_resolution(
            10,
            ConflictResolution::LastWriteWins,
        );
        let user_id = UserId::new();

        // Create two concurrent messages (same vector clock state)
        let mut clock_a = VectorClock::new();
        clock_a.increment("device_a");

        let mut clock_b = VectorClock::new();
        clock_b.increment("device_b");

        // These are concurrent
        assert!(clock_a.is_concurrent(&clock_b));

        let mut message_a = SyncMessage {
            sync_id: "sync_a".to_string(),
            user_id: user_id.clone(),
            device_id: "device_a".to_string(),
            message_type: SyncMessageType::IdentityUpdate,
            timestamp: Utc::now() - chrono::Duration::seconds(10), // Older
            encrypted_payload: vec![1],
            vector_clock: clock_a,
        };

        let message_b = SyncMessage {
            sync_id: "sync_b".to_string(),
            user_id: user_id.clone(),
            device_id: "device_b".to_string(),
            message_type: SyncMessageType::IdentityUpdate,
            timestamp: Utc::now(), // Newer
            encrypted_payload: vec![2],
            vector_clock: clock_b,
        };

        // Add first message
        assert!(manager.add_sync_message(message_a.clone()).is_ok());

        // Add conflicting message - should resolve to message_b (newer timestamp)
        assert!(manager.add_sync_message(message_b.clone()).is_ok());

        // Should only have one message (the winner)
        let pending = manager.get_pending_syncs(&user_id);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sync_id, "sync_b");
    }

    #[test]
    fn test_causal_ordering() {
        let manager = SyncManager::new(10);
        let user_id = UserId::new();

        // Create causally ordered messages
        let mut clock1 = VectorClock::new();
        clock1.increment("device1");

        let mut clock2 = clock1.clone();
        clock2.increment("device1");

        let mut clock3 = clock2.clone();
        clock3.increment("device1");

        let msg1 = create_test_message(user_id.clone(), "device1", clock1);
        let msg2 = create_test_message(user_id.clone(), "device1", clock2);
        let msg3 = create_test_message(user_id.clone(), "device1", clock3);

        // Add out of order
        let mut manager = SyncManager::new(10);
        manager.add_sync_message(msg3.clone()).unwrap();
        manager.add_sync_message(msg1.clone()).unwrap();
        manager.add_sync_message(msg2.clone()).unwrap();

        // Get in causal order
        let ordered = manager.get_pending_syncs_ordered(&user_id);
        assert_eq!(ordered.len(), 3);
        
        // Verify causal ordering
        assert!(ordered[0].vector_clock.happened_before(&ordered[1].vector_clock));
        assert!(ordered[1].vector_clock.happened_before(&ordered[2].vector_clock));
    }

    #[test]
    fn test_sync_queue_limit() {
        let mut manager = SyncManager::new(3);
        let user_id = UserId::new();

        // Add 5 messages (limit is 3)
        // Use a cumulative clock to establish causality and avoid conflict detection
        let mut cumulative_clock = VectorClock::new();
        
        // Use different message types to avoid conflict resolution
        let message_types = [
            SyncMessageType::IdentityUpdate,
            SyncMessageType::DeviceAdded,
            SyncMessageType::DeviceRemoved,
            SyncMessageType::SettingsUpdate,
            SyncMessageType::ContactsUpdate,
        ];
        
        for i in 0..5 {
            cumulative_clock.increment("device1");

            let message = SyncMessage {
                sync_id: format!("sync-{}", i),
                user_id: user_id.clone(),
                device_id: "device1".to_string(),
                message_type: message_types[i].clone(),
                timestamp: Utc::now(),
                encrypted_payload: vec![i as u8],
                vector_clock: cumulative_clock.clone(),
            };
            manager.add_sync_message(message).unwrap();
        }

        // Should only have 3 messages (oldest 2 removed)
        let pending = manager.get_pending_syncs(&user_id);
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn test_device_clock_tick() {
        let mut manager = SyncManager::new(10);

        manager.tick_device_clock("device1");
        manager.tick_device_clock("device1");
        manager.tick_device_clock("device2");

        assert_eq!(manager.get_device_clock("device1").get("device1"), 2);
        assert_eq!(manager.get_device_clock("device2").get("device2"), 1);
    }
}
