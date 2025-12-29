//! Skipped Message Keys Storage
//!
//! When messages arrive out of order, we need to store the skipped
//! message keys so we can decrypt them when they eventually arrive.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::chain::MessageKey;
use super::DEFAULT_MAX_SKIP;

/// Maximum number of message keys to skip per ratchet public key
pub const MAX_SKIP: usize = DEFAULT_MAX_SKIP;

/// Key for the skipped keys map: (ratchet_public_key, message_index)
type SkippedKeyId = ([u8; 32], u32);

/// Entry in the skipped keys store
#[derive(Clone)]
struct SkippedKeyEntry {
    key: [u8; 32],
    created_at: u64,
}

impl Zeroize for SkippedKeyEntry {
    fn zeroize(&mut self) {
        self.key.zeroize();
    }
}

/// Storage for skipped message keys
pub struct SkippedMessageKeys {
    /// Map from (ratchet_public_key, message_index) to message key
    keys: HashMap<SkippedKeyId, SkippedKeyEntry>,
    /// Maximum age before keys are discarded
    max_age_secs: u64,
    /// Maximum total keys to store
    max_keys: usize,
}

impl SkippedMessageKeys {
    /// Create a new skipped keys store
    pub fn new(max_age_secs: u64) -> Self {
        Self {
            keys: HashMap::new(),
            max_age_secs,
            max_keys: MAX_SKIP * 10, // Allow 10 ratchet steps worth of skips
        }
    }

    /// Store a skipped message key
    pub fn store(&mut self, ratchet_key: [u8; 32], message_key: MessageKey) -> Result<()> {
        // Check limits
        if self.keys.len() >= self.max_keys {
            // Evict oldest entries
            self.evict_oldest(self.max_keys / 10);
        }

        let id = (ratchet_key, message_key.index());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.keys.insert(id, SkippedKeyEntry {
            key: *message_key.encryption_key(),
            created_at: now,
        });

        Ok(())
    }

    /// Store multiple skipped keys from the same ratchet
    pub fn store_many(&mut self, ratchet_key: [u8; 32], keys: Vec<MessageKey>) -> Result<()> {
        for mk in keys {
            self.store(ratchet_key, mk)?;
        }
        Ok(())
    }

    /// Try to retrieve a skipped message key
    pub fn take(&mut self, ratchet_key: &[u8; 32], message_index: u32) -> Option<MessageKey> {
        let id = (*ratchet_key, message_index);
        self.keys.remove(&id).map(|entry| {
            MessageKey::new(entry.key, message_index)
        })
    }

    /// Check if we have a key for this message
    pub fn contains(&self, ratchet_key: &[u8; 32], message_index: u32) -> bool {
        self.keys.contains_key(&(*ratchet_key, message_index))
    }

    /// Remove expired keys
    pub fn cleanup_expired(&mut self) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let expired: Vec<_> = self.keys
            .iter()
            .filter(|(_, entry)| now.saturating_sub(entry.created_at) > self.max_age_secs)
            .map(|(id, _)| *id)
            .collect();

        let count = expired.len();
        for id in expired {
            if let Some(mut entry) = self.keys.remove(&id) {
                entry.zeroize();
            }
        }
        count
    }

    /// Remove keys for a specific ratchet public key
    pub fn remove_for_ratchet(&mut self, ratchet_key: &[u8; 32]) -> usize {
        let to_remove: Vec<_> = self.keys
            .keys()
            .filter(|(rk, _)| rk == ratchet_key)
            .cloned()
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            if let Some(mut entry) = self.keys.remove(&id) {
                entry.zeroize();
            }
        }
        count
    }

    /// Number of stored keys
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Evict the oldest entries
    fn evict_oldest(&mut self, count: usize) {
        let mut entries: Vec<_> = self.keys.iter()
            .map(|(id, entry)| (*id, entry.created_at))
            .collect();

        entries.sort_by_key(|(_, created)| *created);

        for (id, _) in entries.into_iter().take(count) {
            if let Some(mut entry) = self.keys.remove(&id) {
                entry.zeroize();
            }
        }
    }

    /// Clear all stored keys
    pub fn clear(&mut self) {
        for (_, mut entry) in self.keys.drain() {
            entry.zeroize();
        }
    }

    /// Serialize for persistence (keys are encrypted externally)
    pub fn to_bytes(&self) -> Vec<u8> {
        let serializable: Vec<([u8; 32], u32, [u8; 32], u64)> = self.keys
            .iter()
            .map(|((rk, idx), entry)| (*rk, *idx, entry.key, entry.created_at))
            .collect();
        
        bincode::serialize(&serializable).unwrap_or_default()
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8], max_age_secs: u64) -> Result<Self> {
        let entries: Vec<([u8; 32], u32, [u8; 32], u64)> = bincode::deserialize(data)
            .map_err(|e| Error::crypto(format!("Failed to deserialize skipped keys: {}", e)))?;

        let mut store = Self::new(max_age_secs);
        
        for (rk, idx, key, created_at) in entries {
            store.keys.insert((rk, idx), SkippedKeyEntry { key, created_at });
        }

        // Cleanup any expired entries
        store.cleanup_expired();

        Ok(store)
    }
}

impl Drop for SkippedMessageKeys {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve() {
        let mut store = SkippedMessageKeys::new(3600);
        let ratchet_key = [1u8; 32];
        let message_key = MessageKey::new([2u8; 32], 5);

        store.store(ratchet_key, message_key).unwrap();
        assert_eq!(store.len(), 1);
        assert!(store.contains(&ratchet_key, 5));

        let retrieved = store.take(&ratchet_key, 5).unwrap();
        assert_eq!(retrieved.index(), 5);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_take_removes_key() {
        let mut store = SkippedMessageKeys::new(3600);
        let ratchet_key = [1u8; 32];
        let message_key = MessageKey::new([2u8; 32], 0);

        store.store(ratchet_key, message_key).unwrap();
        
        // First take should succeed
        assert!(store.take(&ratchet_key, 0).is_some());
        
        // Second take should fail (key consumed)
        assert!(store.take(&ratchet_key, 0).is_none());
    }

    #[test]
    fn test_store_many() {
        let mut store = SkippedMessageKeys::new(3600);
        let ratchet_key = [1u8; 32];
        
        let keys: Vec<_> = (0..5)
            .map(|i| MessageKey::new([i as u8; 32], i))
            .collect();

        store.store_many(ratchet_key, keys).unwrap();
        assert_eq!(store.len(), 5);

        for i in 0..5 {
            assert!(store.contains(&ratchet_key, i));
        }
    }

    #[test]
    fn test_remove_for_ratchet() {
        let mut store = SkippedMessageKeys::new(3600);
        let rk1 = [1u8; 32];
        let rk2 = [2u8; 32];

        store.store(rk1, MessageKey::new([0u8; 32], 0)).unwrap();
        store.store(rk1, MessageKey::new([0u8; 32], 1)).unwrap();
        store.store(rk2, MessageKey::new([0u8; 32], 0)).unwrap();

        assert_eq!(store.len(), 3);

        let removed = store.remove_for_ratchet(&rk1);
        assert_eq!(removed, 2);
        assert_eq!(store.len(), 1);
        assert!(store.contains(&rk2, 0));
    }

    #[test]
    fn test_serialization() {
        let mut store = SkippedMessageKeys::new(3600);
        let ratchet_key = [1u8; 32];

        store.store(ratchet_key, MessageKey::new([2u8; 32], 0)).unwrap();
        store.store(ratchet_key, MessageKey::new([3u8; 32], 1)).unwrap();

        let bytes = store.to_bytes();
        let restored = SkippedMessageKeys::from_bytes(&bytes, 3600).unwrap();

        assert_eq!(restored.len(), 2);
        assert!(restored.contains(&ratchet_key, 0));
        assert!(restored.contains(&ratchet_key, 1));
    }
}
