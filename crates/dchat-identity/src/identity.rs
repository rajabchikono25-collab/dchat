//! Core identity management

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::{PublicKey, ReputationScore, UserId};
use dchat_crypto::keys::KeyPair;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a user's identity in the dchat system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub user_id: UserId,
    pub username: String,
    pub public_key: PublicKey,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub reputation: ReputationScore,
    pub created_at: DateTime<Utc>,
    pub verified: bool,
    pub badges: Vec<String>,
    pub metadata: HashMap<String, String>,
}

/// Maximum username length to prevent memory exhaustion attacks
const MAX_USERNAME_LENGTH: usize = 64;
/// Maximum bio length
const MAX_BIO_LENGTH: usize = 500;
/// Maximum display name length
const MAX_DISPLAY_NAME_LENGTH: usize = 100;
/// Maximum metadata entries per identity
const MAX_METADATA_ENTRIES: usize = 50;
/// Maximum metadata value length
const MAX_METADATA_VALUE_LENGTH: usize = 1000;

impl Identity {
    /// Create a new identity from a keypair
    /// 
    /// # Security
    /// - Username is validated for length and characters
    /// - Empty usernames are rejected
    pub fn new(username: String, keypair: &KeyPair) -> Self {
        // Validate and sanitize username
        let sanitized_username = Self::sanitize_username(&username);
        let public_key = keypair.public_key().to_core_public_key();

        Self {
            user_id: UserId::new(),
            username: sanitized_username,
            public_key,
            display_name: None,
            bio: None,
            reputation: ReputationScore::default(),
            created_at: Utc::now(),
            verified: false,
            badges: Vec::new(),
            metadata: HashMap::new(),
        }
    }
    
    /// Validate and create identity with error handling
    pub fn try_new(username: String, keypair: &KeyPair) -> Result<Self> {
        // Validate username
        if username.is_empty() {
            return Err(Error::identity("Username cannot be empty"));
        }
        if username.len() > MAX_USERNAME_LENGTH {
            return Err(Error::identity(format!(
                "Username exceeds maximum length of {} characters",
                MAX_USERNAME_LENGTH
            )));
        }
        // Check for valid characters (alphanumeric, underscore, hyphen)
        if !username.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(Error::identity(
                "Username can only contain alphanumeric characters, underscores, and hyphens"
            ));
        }
        // Prevent usernames that look like system accounts
        let lower = username.to_lowercase();
        if lower == "admin" || lower == "system" || lower == "root" || lower.starts_with("dchat_") {
            return Err(Error::identity("Reserved username"));
        }
        
        Ok(Self::new(username, keypair))
    }
    
    /// Sanitize username by truncating and removing invalid characters
    fn sanitize_username(username: &str) -> String {
        username
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .take(MAX_USERNAME_LENGTH)
            .collect()
    }

    /// Update display name
    /// 
    /// # Security
    /// - Truncates to MAX_DISPLAY_NAME_LENGTH to prevent memory exhaustion
    pub fn set_display_name(&mut self, display_name: String) {
        let truncated: String = display_name.chars().take(MAX_DISPLAY_NAME_LENGTH).collect();
        self.display_name = Some(truncated);
    }
    
    /// Update display name with validation
    pub fn try_set_display_name(&mut self, display_name: String) -> Result<()> {
        if display_name.len() > MAX_DISPLAY_NAME_LENGTH {
            return Err(Error::identity(format!(
                "Display name exceeds maximum length of {} characters",
                MAX_DISPLAY_NAME_LENGTH
            )));
        }
        self.display_name = Some(display_name);
        Ok(())
    }

    /// Update bio
    /// 
    /// # Security
    /// - Truncates to MAX_BIO_LENGTH to prevent memory exhaustion
    pub fn set_bio(&mut self, bio: String) {
        let truncated: String = bio.chars().take(MAX_BIO_LENGTH).collect();
        self.bio = Some(truncated);
    }
    
    /// Update bio with validation
    pub fn try_set_bio(&mut self, bio: String) -> Result<()> {
        if bio.len() > MAX_BIO_LENGTH {
            return Err(Error::identity(format!(
                "Bio exceeds maximum length of {} characters",
                MAX_BIO_LENGTH
            )));
        }
        self.bio = Some(bio);
        Ok(())
    }

    /// Add a badge
    pub fn add_badge(&mut self, badge: String) {
        if !self.badges.contains(&badge) {
            self.badges.push(badge);
        }
    }

    /// Remove a badge
    pub fn remove_badge(&mut self, badge: &str) {
        self.badges.retain(|b| b != badge);
    }

    /// Set metadata field
    /// 
    /// # Security
    /// - Limits total metadata entries
    /// - Truncates values to prevent memory exhaustion
    pub fn set_metadata(&mut self, key: String, value: String) {
        // Limit metadata entries to prevent abuse
        if self.metadata.len() >= MAX_METADATA_ENTRIES && !self.metadata.contains_key(&key) {
            tracing::warn!("Maximum metadata entries reached, ignoring new key: {}", key);
            return;
        }
        // Truncate value
        let truncated_value: String = value.chars().take(MAX_METADATA_VALUE_LENGTH).collect();
        self.metadata.insert(key, truncated_value);
    }
    
    /// Set metadata with validation
    pub fn try_set_metadata(&mut self, key: String, value: String) -> Result<()> {
        if self.metadata.len() >= MAX_METADATA_ENTRIES && !self.metadata.contains_key(&key) {
            return Err(Error::identity(format!(
                "Maximum metadata entries ({}) reached",
                MAX_METADATA_ENTRIES
            )));
        }
        if value.len() > MAX_METADATA_VALUE_LENGTH {
            return Err(Error::identity(format!(
                "Metadata value exceeds maximum length of {} characters",
                MAX_METADATA_VALUE_LENGTH
            )));
        }
        self.metadata.insert(key, value);
        Ok(())
    }

    /// Get metadata field
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Update reputation score
    pub fn update_reputation(&mut self, reputation: ReputationScore) {
        self.reputation = reputation;
    }

    /// Mark as verified
    pub fn set_verified(&mut self, verified: bool) {
        self.verified = verified;
    }

    /// Get identity fingerprint (hash of public key)
    pub fn fingerprint(&self) -> String {
        let hash = dchat_crypto::hash(self.public_key.as_bytes());
        hex::encode(&hash[..8])
    }
}

/// Manages user identities
pub struct IdentityManager {
    identities: HashMap<UserId, Identity>,
    username_index: HashMap<String, UserId>,
    pubkey_index: HashMap<Vec<u8>, UserId>,
}

impl IdentityManager {
    /// Create a new identity manager
    pub fn new() -> Self {
        Self {
            identities: HashMap::new(),
            username_index: HashMap::new(),
            pubkey_index: HashMap::new(),
        }
    }

    /// Register a new identity
    pub fn register_identity(&mut self, identity: Identity) -> Result<()> {
        // Check if username is already taken
        if self.username_index.contains_key(&identity.username) {
            return Err(Error::identity("Username already taken"));
        }

        // Check if public key is already registered
        let pubkey_bytes = identity.public_key.as_bytes().to_vec();
        if self.pubkey_index.contains_key(&pubkey_bytes) {
            return Err(Error::identity("Public key already registered"));
        }

        // Store identity
        let user_id = identity.user_id.clone();
        let username = identity.username.clone();

        self.username_index.insert(username, user_id.clone());
        self.pubkey_index.insert(pubkey_bytes, user_id.clone());
        self.identities.insert(user_id, identity);

        Ok(())
    }

    /// Get identity by user ID
    pub fn get_identity(&self, user_id: &UserId) -> Option<&Identity> {
        self.identities.get(user_id)
    }

    /// Get mutable identity by user ID
    pub fn get_identity_mut(&mut self, user_id: &UserId) -> Option<&mut Identity> {
        self.identities.get_mut(user_id)
    }

    /// Get identity by username
    pub fn get_identity_by_username(&self, username: &str) -> Option<&Identity> {
        self.username_index
            .get(username)
            .and_then(|user_id| self.identities.get(user_id))
    }

    /// Get identity by public key
    pub fn get_identity_by_pubkey(&self, pubkey: &PublicKey) -> Option<&Identity> {
        self.pubkey_index
            .get(pubkey.as_bytes())
            .and_then(|user_id| self.identities.get(user_id))
    }

    /// Update identity
    pub fn update_identity(
        &mut self,
        user_id: &UserId,
        update_fn: impl FnOnce(&mut Identity),
    ) -> Result<()> {
        let identity = self
            .identities
            .get_mut(user_id)
            .ok_or_else(|| Error::identity("Identity not found"))?;

        update_fn(identity);
        Ok(())
    }

    /// Remove identity
    pub fn remove_identity(&mut self, user_id: &UserId) -> Result<Identity> {
        let identity = self
            .identities
            .remove(user_id)
            .ok_or_else(|| Error::identity("Identity not found"))?;

        self.username_index.remove(&identity.username);
        self.pubkey_index.remove(identity.public_key.as_bytes());

        Ok(identity)
    }

    /// List all identities
    pub fn list_identities(&self) -> Vec<&Identity> {
        self.identities.values().collect()
    }

    /// Get total number of identities
    pub fn count(&self) -> usize {
        self.identities.len()
    }

    /// Check if username is available
    pub fn is_username_available(&self, username: &str) -> bool {
        !self.username_index.contains_key(username)
    }

    /// Check if public key is registered
    pub fn is_pubkey_registered(&self, pubkey: &PublicKey) -> bool {
        self.pubkey_index.contains_key(pubkey.as_bytes())
    }
}

impl Default for IdentityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dchat_crypto::keys::KeyPair;

    #[test]
    fn test_identity_creation() {
        let keypair = KeyPair::generate();
        let identity = Identity::new("alice".to_string(), &keypair);

        assert_eq!(identity.username, "alice");
        assert_eq!(identity.verified, false);
        assert_eq!(identity.badges.len(), 0);
    }

    #[test]
    fn test_identity_manager() {
        let mut manager = IdentityManager::new();

        let keypair = KeyPair::generate();
        let identity = Identity::new("alice".to_string(), &keypair);
        let user_id = identity.user_id.clone();

        // Register identity
        assert!(manager.register_identity(identity).is_ok());

        // Get identity by ID
        assert!(manager.get_identity(&user_id).is_some());

        // Get identity by username
        assert!(manager.get_identity_by_username("alice").is_some());

        // Check username availability
        assert!(!manager.is_username_available("alice"));
        assert!(manager.is_username_available("bob"));

        // Try to register same username
        let keypair2 = KeyPair::generate();
        let identity2 = Identity::new("alice".to_string(), &keypair2);
        assert!(manager.register_identity(identity2).is_err());
    }

    #[test]
    fn test_identity_updates() {
        let keypair = KeyPair::generate();
        let mut identity = Identity::new("alice".to_string(), &keypair);

        identity.set_display_name("Alice Wonderland".to_string());
        assert_eq!(identity.display_name, Some("Alice Wonderland".to_string()));

        identity.set_bio("Exploring the decentralized world".to_string());
        assert_eq!(
            identity.bio,
            Some("Exploring the decentralized world".to_string())
        );

        identity.add_badge("early_adopter".to_string());
        assert_eq!(identity.badges.len(), 1);

        identity.remove_badge("early_adopter");
        assert_eq!(identity.badges.len(), 0);
    }
}
