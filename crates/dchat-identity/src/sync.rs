//! Device synchronization for multi-device identities

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Sync message for device coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    pub sync_id: String,
    pub user_id: UserId,
    pub device_id: String,
    pub message_type: SyncMessageType,
    pub timestamp: DateTime<Utc>,
    pub encrypted_payload: Vec<u8>,
}

/// Types of sync messages
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(max_pending_per_user: usize) -> Self {
        Self {
            pending_syncs: HashMap::new(),
            sync_history: HashMap::new(),
            max_pending_per_user,
        }
    }

    /// Add a sync message
    pub fn add_sync_message(&mut self, message: SyncMessage) -> Result<()> {
        let user_id = message.user_id.clone();
        let sync_id = message.sync_id.clone();

        // Get or create pending queue for user
        let queue = self.pending_syncs.entry(user_id).or_default();

        // Check if queue is full
        if queue.len() >= self.max_pending_per_user {
            // Remove oldest message
            if let Some(old_msg) = queue.pop_front() {
                self.sync_history.remove(&old_msg.sync_id);
            }
        }

        // Add to queue and history
        queue.push_back(message.clone());
        self.sync_history.insert(sync_id, message);

        Ok(())
    }

    /// Get pending sync messages for a user
    pub fn get_pending_syncs(&self, user_id: &UserId) -> Vec<&SyncMessage> {
        self.pending_syncs
            .get(user_id)
            .map(|queue| queue.iter().collect())
            .unwrap_or_default()
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
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_sync_manager() {
        let mut manager = SyncManager::new(10);
        let user_id = UserId::new();

        let message = SyncMessage {
            sync_id: Uuid::new_v4().to_string(),
            user_id: user_id.clone(),
            device_id: "device1".to_string(),
            message_type: SyncMessageType::DeviceAdded,
            timestamp: Utc::now(),
            encrypted_payload: vec![1, 2, 3],
        };

        // Add sync message
        assert!(manager.add_sync_message(message.clone()).is_ok());

        // Get pending syncs
        let pending = manager.get_pending_syncs(&user_id);
        assert_eq!(pending.len(), 1);

        // Acknowledge sync
        assert!(manager.acknowledge_sync(&message.sync_id).is_ok());
        let pending = manager.get_pending_syncs(&user_id);
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_sync_queue_limit() {
        let mut manager = SyncManager::new(3);
        let user_id = UserId::new();

        // Add 5 messages (limit is 3)
        for i in 0..5 {
            let message = SyncMessage {
                sync_id: format!("sync-{}", i),
                user_id: user_id.clone(),
                device_id: "device1".to_string(),
                message_type: SyncMessageType::IdentityUpdate,
                timestamp: Utc::now(),
                encrypted_payload: vec![i as u8],
            };
            manager.add_sync_message(message).unwrap();
        }

        // Should only have 3 messages
        let pending = manager.get_pending_syncs(&user_id);
        assert_eq!(pending.len(), 3);
    }
}
