//! Key rotation management for forward secrecy

use crate::keys::{KeyPair, PrivateKey};
use chrono::{DateTime, Duration, Utc};
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Policy for when to rotate keys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationPolicy {
    /// Maximum age of a key before rotation
    pub max_age_hours: u32,
    /// Maximum number of messages sent with a key
    pub max_messages_per_key: u64,
    /// Rotate on specific events
    pub rotate_on_events: Vec<RotationEvent>,
}

/// Events that trigger key rotation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RotationEvent {
    /// Rotate when a peer comes online
    PeerOnline,
    /// Rotate when a new device is added
    NewDevice,
    /// Rotate when suspicious activity is detected
    SuspiciousActivity,
    /// Manual rotation request
    Manual,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self {
            max_age_hours: 168, // 1 week
            max_messages_per_key: 10000,
            rotate_on_events: vec![
                RotationEvent::NewDevice,
                RotationEvent::SuspiciousActivity,
                RotationEvent::Manual,
            ],
        }
    }
}

/// Tracks usage of a specific key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyUsage {
    pub created_at: DateTime<Utc>,
    pub messages_sent: u64,
    pub last_used: DateTime<Utc>,
    pub rotation_count: u32,
}

impl KeyUsage {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            created_at: now,
            messages_sent: 0,
            last_used: now,
            rotation_count: 0,
        }
    }

    pub fn record_message_sent(&mut self) {
        self.messages_sent += 1;
        self.last_used = Utc::now();
    }

    pub fn age(&self) -> Duration {
        Utc::now() - self.created_at
    }
}

impl Default for KeyUsage {
    fn default() -> Self {
        Self::new()
    }
}

/// Manages key rotation for forward secrecy
pub struct KeyRotationManager {
    policy: RotationPolicy,
    current_keys: HashMap<String, (KeyPair, KeyUsage)>,
    master_key: PrivateKey,
}

impl KeyRotationManager {
    /// Create a new key rotation manager
    pub fn new(master_key: PrivateKey, policy: RotationPolicy) -> Self {
        Self {
            policy,
            current_keys: HashMap::new(),
            master_key,
        }
    }

    /// Get or create a key for a specific purpose
    pub fn get_key(&mut self, purpose: &str) -> Result<&KeyPair> {
        if !self.current_keys.contains_key(purpose) || self.should_rotate(purpose)? {
            self.rotate_key(purpose)?;
        }

        // Safe: we just ensured the key exists via rotate_key or the contains_key check
        self.current_keys
            .get(purpose)
            .map(|(keypair, _)| keypair)
            .ok_or_else(|| Error::crypto(format!("Key not found for purpose: {}", purpose)))
    }

    /// Check if a key should be rotated based on policy
    pub fn should_rotate(&self, purpose: &str) -> Result<bool> {
        let Some((_, usage)) = self.current_keys.get(purpose) else {
            return Ok(true); // No key exists, need to create one
        };

        // Check age
        if usage.age().num_hours() as u32 >= self.policy.max_age_hours {
            return Ok(true);
        }

        // Check message count
        if usage.messages_sent >= self.policy.max_messages_per_key {
            return Ok(true);
        }

        Ok(false)
    }

    /// Force rotation of a key
    pub fn rotate_key(&mut self, purpose: &str) -> Result<()> {
        let rotation_count = self
            .current_keys
            .get(purpose)
            .map(|(_, usage)| usage.rotation_count + 1)
            .unwrap_or(0);

        let new_key = self.derive_key_for_purpose(purpose, rotation_count)?;
        let usage = KeyUsage::new();

        if let Some((_, old_usage)) = self
            .current_keys
            .insert(purpose.to_string(), (new_key, usage))
        {
            tracing::info!(
                "Rotated key for purpose '{}' (rotation #{})",
                purpose,
                old_usage.rotation_count + 1
            );
        } else {
            tracing::info!("Created initial key for purpose '{}'", purpose);
        }

        Ok(())
    }

    /// Record that a message was sent with a key
    pub fn record_message_sent(&mut self, purpose: &str) -> Result<()> {
        let Some((_, usage)) = self.current_keys.get_mut(purpose) else {
            return Err(Error::crypto(format!(
                "No key found for purpose: {}",
                purpose
            )));
        };

        usage.record_message_sent();
        Ok(())
    }

    /// Trigger rotation for a specific event
    pub fn trigger_rotation_event(&mut self, event: RotationEvent) -> Result<Vec<String>> {
        if !self.policy.rotate_on_events.contains(&event) {
            return Ok(Vec::new());
        }

        let purposes: Vec<String> = self.current_keys.keys().cloned().collect();
        let mut rotated = Vec::new();

        for purpose in purposes {
            self.rotate_key(&purpose)?;
            rotated.push(purpose);
        }

        Ok(rotated)
    }

    /// Get key usage statistics
    pub fn get_key_usage(&self, purpose: &str) -> Option<&KeyUsage> {
        self.current_keys.get(purpose).map(|(_, usage)| usage)
    }

    /// Get all current key purposes
    pub fn get_purposes(&self) -> Vec<String> {
        self.current_keys.keys().cloned().collect()
    }

    /// Update rotation policy
    pub fn update_policy(&mut self, policy: RotationPolicy) {
        self.policy = policy;
    }

    /// Get current policy
    pub fn get_policy(&self) -> &RotationPolicy {
        &self.policy
    }

    /// Derive a key for a specific purpose using the master key
    fn derive_key_for_purpose(&self, purpose: &str, rotation_count: u32) -> Result<KeyPair> {
        let purpose_key =
            crate::kdf::Hkdf::derive_purpose_key(&self.master_key, purpose, rotation_count)?;

        Ok(KeyPair::from_private_key(purpose_key))
    }
}

/// Helper for managing conversation-specific key rotation
pub struct ConversationKeyManager {
    rotation_manager: KeyRotationManager,
}

impl ConversationKeyManager {
    pub fn new(master_key: PrivateKey) -> Self {
        let policy = RotationPolicy {
            max_age_hours: 24,          // Rotate daily for conversations
            max_messages_per_key: 1000, // Rotate after 1000 messages
            rotate_on_events: vec![
                RotationEvent::PeerOnline,
                RotationEvent::SuspiciousActivity,
                RotationEvent::Manual,
            ],
        };

        Self {
            rotation_manager: KeyRotationManager::new(master_key, policy),
        }
    }

    /// Get encryption key for a conversation
    pub fn get_conversation_key(&mut self, peer_id: &str) -> Result<&KeyPair> {
        let purpose = format!("conversation:{}", peer_id);
        self.rotation_manager.get_key(&purpose)
    }

    /// Record message sent in conversation
    pub fn record_conversation_message(&mut self, peer_id: &str) -> Result<()> {
        let purpose = format!("conversation:{}", peer_id);
        self.rotation_manager.record_message_sent(&purpose)
    }

    /// Trigger rotation when peer comes online
    pub fn peer_came_online(&mut self, peer_id: &str) -> Result<()> {
        if self
            .rotation_manager
            .policy
            .rotate_on_events
            .contains(&RotationEvent::PeerOnline)
        {
            let purpose = format!("conversation:{}", peer_id);
            self.rotation_manager.rotate_key(&purpose)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_rotation_manager() {
        let master_key = PrivateKey::generate();
        let policy = RotationPolicy {
            max_age_hours: 1,
            max_messages_per_key: 2,
            rotate_on_events: vec![RotationEvent::Manual],
        };

        let mut manager = KeyRotationManager::new(master_key, policy);

        // Get initial key
        let key1 = manager.get_key("test").unwrap();
        let key1_public = key1.public_key().clone();

        // Same key should be returned
        let key2 = manager.get_key("test").unwrap();
        assert_eq!(key1_public, *key2.public_key());

        // Record messages to trigger rotation (need 3 to exceed limit of 2)
        manager.record_message_sent("test").unwrap();
        manager.record_message_sent("test").unwrap();
        manager.record_message_sent("test").unwrap();

        // Should rotate due to message count
        let key3 = manager.get_key("test").unwrap();
        assert_ne!(key1_public, *key3.public_key());
    }

    #[test]
    fn test_rotation_events() {
        let master_key = PrivateKey::generate();
        let mut manager = KeyRotationManager::new(master_key, RotationPolicy::default());

        // Create some keys
        manager.get_key("test1").unwrap();
        manager.get_key("test2").unwrap();

        // Trigger manual rotation
        let rotated = manager
            .trigger_rotation_event(RotationEvent::Manual)
            .unwrap();

        assert_eq!(rotated.len(), 2);
        assert!(rotated.contains(&"test1".to_string()));
        assert!(rotated.contains(&"test2".to_string()));
    }

    #[test]
    fn test_conversation_key_manager() {
        let master_key = PrivateKey::generate();
        let mut manager = ConversationKeyManager::new(master_key);

        let alice_key1 = manager.get_conversation_key("alice").unwrap();
        let alice_key1_public = alice_key1.public_key().clone();

        // Same conversation should return same key initially
        let alice_key2 = manager.get_conversation_key("alice").unwrap();
        assert_eq!(alice_key1_public, *alice_key2.public_key());

        // Different conversation should have different key
        let bob_key = manager.get_conversation_key("bob").unwrap();
        assert_ne!(alice_key1_public, *bob_key.public_key());
    }
}
