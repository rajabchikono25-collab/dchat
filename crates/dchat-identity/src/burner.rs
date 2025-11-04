//! Burner identities for privacy

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::{PublicKey, UserId};
use dchat_crypto::keys::KeyPair;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A temporary burner identity for privacy-sensitive communications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnerIdentity {
    pub burner_id: UserId,
    pub public_key: PublicKey,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_messages: Option<u64>,
    pub messages_sent: u64,
    pub parent_user_id: Option<UserId>, // Optional link to parent (if not fully anonymous)
    pub metadata: HashMap<String, String>,
}

impl BurnerIdentity {
    /// Create a new burner identity
    pub fn new(keypair: &KeyPair, parent_user_id: Option<UserId>) -> Self {
        Self {
            burner_id: UserId::new(),
            public_key: keypair.public_key().to_core_public_key(),
            created_at: Utc::now(),
            expires_at: None,
            max_messages: None,
            messages_sent: 0,
            parent_user_id,
            metadata: HashMap::new(),
        }
    }

    /// Create a time-limited burner
    pub fn with_expiry(keypair: &KeyPair, hours: i64, parent_user_id: Option<UserId>) -> Self {
        let mut identity = Self::new(keypair, parent_user_id);
        identity.expires_at = Some(Utc::now() + chrono::Duration::hours(hours));
        identity
    }

    /// Create a message-limited burner
    pub fn with_message_limit(
        keypair: &KeyPair,
        max_messages: u64,
        parent_user_id: Option<UserId>,
    ) -> Self {
        let mut identity = Self::new(keypair, parent_user_id);
        identity.max_messages = Some(max_messages);
        identity
    }

    /// Check if burner has expired
    pub fn is_expired(&self) -> bool {
        // Check time expiry
        if let Some(expires_at) = self.expires_at {
            if Utc::now() > expires_at {
                return true;
            }
        }

        // Check message limit
        if let Some(max_messages) = self.max_messages {
            if self.messages_sent >= max_messages {
                return true;
            }
        }

        false
    }

    /// Record a message sent
    pub fn record_message_sent(&mut self) {
        self.messages_sent += 1;
    }

    /// Check if burner can send more messages
    pub fn can_send_message(&self) -> bool {
        !self.is_expired()
    }

    /// Get remaining messages (if limited)
    pub fn remaining_messages(&self) -> Option<u64> {
        self.max_messages
            .map(|max| max.saturating_sub(self.messages_sent))
    }
}

/// Manages burner identities
pub struct BurnerManager {
    burners: HashMap<UserId, BurnerIdentity>,
    parent_index: HashMap<UserId, Vec<UserId>>, // parent -> burners
}

impl BurnerManager {
    /// Create a new burner manager
    pub fn new() -> Self {
        Self {
            burners: HashMap::new(),
            parent_index: HashMap::new(),
        }
    }

    /// Register a burner identity
    pub fn register_burner(&mut self, burner: BurnerIdentity) -> Result<()> {
        let burner_id = burner.burner_id.clone();

        // Add to parent index if linked
        if let Some(parent_id) = &burner.parent_user_id {
            self.parent_index
                .entry(parent_id.clone())
                .or_default()
                .push(burner_id.clone());
        }

        self.burners.insert(burner_id, burner);
        Ok(())
    }

    /// Get burner identity
    pub fn get_burner(&self, burner_id: &UserId) -> Option<&BurnerIdentity> {
        self.burners.get(burner_id)
    }

    /// Get mutable burner identity
    pub fn get_burner_mut(&mut self, burner_id: &UserId) -> Option<&mut BurnerIdentity> {
        self.burners.get_mut(burner_id)
    }

    /// Get all burners for a parent identity
    pub fn get_burners_for_parent(&self, parent_id: &UserId) -> Vec<&BurnerIdentity> {
        self.parent_index
            .get(parent_id)
            .map(|burner_ids| {
                burner_ids
                    .iter()
                    .filter_map(|id| self.burners.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Remove expired burners
    pub fn cleanup_expired_burners(&mut self) -> Vec<UserId> {
        let expired: Vec<UserId> = self
            .burners
            .iter()
            .filter(|(_, burner)| burner.is_expired())
            .map(|(id, _)| id.clone())
            .collect();

        for burner_id in &expired {
            if let Some(burner) = self.burners.remove(burner_id) {
                // Remove from parent index
                if let Some(parent_id) = burner.parent_user_id {
                    if let Some(burner_list) = self.parent_index.get_mut(&parent_id) {
                        burner_list.retain(|id| id != burner_id);
                    }
                }
            }
        }

        expired
    }

    /// Revoke a burner identity
    pub fn revoke_burner(&mut self, burner_id: &UserId) -> Result<BurnerIdentity> {
        let burner = self
            .burners
            .remove(burner_id)
            .ok_or_else(|| Error::identity("Burner not found"))?;

        // Remove from parent index
        if let Some(parent_id) = &burner.parent_user_id {
            if let Some(burner_list) = self.parent_index.get_mut(parent_id) {
                burner_list.retain(|id| id != burner_id);
            }
        }

        Ok(burner)
    }

    /// Get total number of active burners
    pub fn active_count(&self) -> usize {
        self.burners.values().filter(|b| !b.is_expired()).count()
    }
}

impl Default for BurnerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dchat_crypto::keys::KeyPair;

    #[test]
    fn test_burner_creation() {
        let keypair = KeyPair::generate();
        let parent_id = UserId::new();

        let burner = BurnerIdentity::new(&keypair, Some(parent_id));
        assert!(!burner.is_expired());
        assert_eq!(burner.messages_sent, 0);
    }

    #[test]
    fn test_message_limit() {
        let keypair = KeyPair::generate();
        let mut burner = BurnerIdentity::with_message_limit(&keypair, 3, None);

        assert!(burner.can_send_message());
        assert_eq!(burner.remaining_messages(), Some(3));

        burner.record_message_sent();
        burner.record_message_sent();
        burner.record_message_sent();

        assert!(!burner.can_send_message());
        assert_eq!(burner.remaining_messages(), Some(0));
        assert!(burner.is_expired());
    }

    #[test]
    fn test_burner_manager() {
        let mut manager = BurnerManager::new();
        let parent_id = UserId::new();

        // Create and register burners
        for _ in 0..3 {
            let keypair = KeyPair::generate();
            let burner = BurnerIdentity::new(&keypair, Some(parent_id.clone()));
            manager.register_burner(burner).unwrap();
        }

        // Get burners for parent
        let burners = manager.get_burners_for_parent(&parent_id);
        assert_eq!(burners.len(), 3);
    }
}
