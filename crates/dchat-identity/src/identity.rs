//! Core identity management

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::{PublicKey, ReputationScore, UserId};
use dchat_crypto::keys::KeyPair;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

// =============================================================================
// PROOF-OF-WORK REGISTRATION PROTECTION
// =============================================================================
// 
// SECURITY FIX: Anti-bot protection for identity registration
// 
// This implements a proof-of-work challenge that must be solved before
// registering a new identity. This creates a computational cost for 
// registration that:
// - Deters automated bot account creation
// - Prevents sybil attacks (one actor creating many identities)
// - Maintains decentralization (no centralized CAPTCHA needed)
// 
// The difficulty is calibrated to take ~1-5 seconds on typical hardware,
// making mass account creation economically infeasible.
// =============================================================================

/// Proof-of-work difficulty (number of leading zero bits required)
/// 
/// SECURITY NOTE: Each additional bit doubles the difficulty.
/// - 16 bits ≈ 0.1-0.5 seconds
/// - 18 bits ≈ 0.5-2 seconds  
/// - 20 bits ≈ 2-10 seconds (current setting)
/// - 22 bits ≈ 10-40 seconds
const POW_DIFFICULTY_BITS: u32 = 20;

/// Challenge expiration time in seconds
/// Prevents pre-computation attacks by limiting challenge validity
const POW_CHALLENGE_EXPIRY_SECS: i64 = 300; // 5 minutes

/// Represents a proof-of-work challenge for registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationChallenge {
    /// Unique challenge ID
    pub challenge_id: [u8; 32],
    /// The public key this challenge is bound to
    pub public_key_hash: [u8; 32],
    /// Timestamp when the challenge was issued
    pub issued_at: DateTime<Utc>,
    /// Required difficulty (leading zero bits)
    pub difficulty: u32,
}

impl RegistrationChallenge {
    /// Generate a new registration challenge for a public key
    /// 
    /// # Security
    /// - Challenge is bound to a specific public key to prevent reuse
    /// - Random component prevents challenge pre-computation
    /// - Timestamp enables expiration enforcement
    pub fn new(public_key: &PublicKey) -> Self {
        let mut challenge_id = [0u8; 32];
        // Use getrandom for secure random challenge
        getrandom::getrandom(&mut challenge_id)
            .expect("Failed to generate challenge randomness");
        
        // Hash the public key for binding
        let mut hasher = Sha256::new();
        hasher.update(public_key.as_bytes());
        let public_key_hash: [u8; 32] = hasher.finalize().into();
        
        Self {
            challenge_id,
            public_key_hash,
            issued_at: Utc::now(),
            difficulty: POW_DIFFICULTY_BITS,
        }
    }
    
    /// Get the data that must be hashed for PoW
    pub fn challenge_bytes(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(72);
        data.extend_from_slice(&self.challenge_id);
        data.extend_from_slice(&self.public_key_hash);
        data.extend_from_slice(&self.issued_at.timestamp().to_le_bytes());
        data.extend_from_slice(&self.difficulty.to_le_bytes());
        data
    }
    
    /// Check if the challenge has expired
    pub fn is_expired(&self) -> bool {
        let now = Utc::now();
        let elapsed = now.signed_duration_since(self.issued_at);
        elapsed.num_seconds() > POW_CHALLENGE_EXPIRY_SECS
    }
}

/// Proof-of-work solution submitted by the client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationProof {
    /// The original challenge
    pub challenge: RegistrationChallenge,
    /// The nonce that satisfies the difficulty requirement
    pub nonce: u64,
}

impl RegistrationProof {
    /// Solve a registration challenge (find valid nonce)
    /// 
    /// # Returns
    /// A proof with a nonce that produces a hash with required leading zero bits
    /// 
    /// # Security
    /// This is computationally expensive by design. On typical hardware:
    /// - 20 bits difficulty: 2-10 seconds
    /// - This cost deters bot registration while remaining acceptable for humans
    pub fn solve(challenge: &RegistrationChallenge) -> Self {
        let challenge_bytes = challenge.challenge_bytes();
        let required_zeros = challenge.difficulty;
        
        let mut nonce: u64 = 0;
        loop {
            let hash = Self::compute_hash(&challenge_bytes, nonce);
            if Self::check_difficulty(&hash, required_zeros) {
                break;
            }
            nonce = nonce.wrapping_add(1);
        }
        
        Self {
            challenge: challenge.clone(),
            nonce,
        }
    }
    
    /// Verify the proof is valid
    /// 
    /// # Security Checks
    /// 1. Challenge not expired
    /// 2. Public key matches the challenge binding
    /// 3. Nonce produces hash with required leading zeros
    pub fn verify(&self, public_key: &PublicKey) -> Result<()> {
        // Check expiration
        if self.challenge.is_expired() {
            return Err(Error::identity("Registration challenge has expired"));
        }
        
        // Verify public key binding
        let mut hasher = Sha256::new();
        hasher.update(public_key.as_bytes());
        let pk_hash: [u8; 32] = hasher.finalize().into();
        
        if pk_hash != self.challenge.public_key_hash {
            return Err(Error::identity("Challenge is not bound to this public key"));
        }
        
        // Verify the proof-of-work
        let challenge_bytes = self.challenge.challenge_bytes();
        let hash = Self::compute_hash(&challenge_bytes, self.nonce);
        
        if !Self::check_difficulty(&hash, self.challenge.difficulty) {
            return Err(Error::identity("Invalid proof-of-work solution"));
        }
        
        Ok(())
    }
    
    /// Compute hash of challenge + nonce
    fn compute_hash(challenge_bytes: &[u8], nonce: u64) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(challenge_bytes);
        hasher.update(&nonce.to_le_bytes());
        hasher.finalize().into()
    }
    
    /// Check if hash has required number of leading zero bits
    fn check_difficulty(hash: &[u8; 32], required_bits: u32) -> bool {
        let full_bytes = (required_bits / 8) as usize;
        let remaining_bits = required_bits % 8;
        
        // Check full zero bytes
        for i in 0..full_bytes {
            if hash[i] != 0 {
                return false;
            }
        }
        
        // Check remaining bits in the next byte
        if remaining_bits > 0 && full_bytes < 32 {
            let mask = 0xFF << (8 - remaining_bits);
            if hash[full_bytes] & mask != 0 {
                return false;
            }
        }
        
        true
    }
}


/// Represents a user's identity in the dchat system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub user_id: UserId,
    pub username: String,
    /// Normalized username for collision detection (NFKC normalized, lowercase)
    pub normalized_username: String,
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

/// Homoglyph character mappings for common confusable characters
/// Maps Unicode characters that look similar to ASCII to their ASCII equivalent
fn normalize_homoglyphs(c: char) -> Option<char> {
    // Common confusable characters mapped to ASCII equivalents
    // This prevents attacks like "аdmin" (Cyrillic 'а') vs "admin" (Latin 'a')
    match c {
        // Cyrillic lookalikes
        'а' | 'А' => Some('a'), // Cyrillic a
        'е' | 'Е' => Some('e'), // Cyrillic e
        'о' | 'О' => Some('o'), // Cyrillic o
        'р' | 'Р' => Some('p'), // Cyrillic r (looks like p)
        'с' | 'С' => Some('c'), // Cyrillic s (looks like c)
        'х' | 'Х' => Some('x'), // Cyrillic kha
        'у' | 'У' => Some('y'), // Cyrillic u (looks like y)
        'і' | 'І' => Some('i'), // Ukrainian i
        'ј' => Some('j'),       // Cyrillic je
        
        // Greek lookalikes
        'α' | 'Α' => Some('a'), // Greek alpha
        'β' | 'Β' => Some('b'), // Greek beta
        'ε' | 'Ε' => Some('e'), // Greek epsilon
        'η' | 'Η' => Some('h'), // Greek eta
        'ι' | 'Ι' => Some('i'), // Greek iota
        'κ' | 'Κ' => Some('k'), // Greek kappa
        'ν' | 'Ν' => Some('n'), // Greek nu
        'ο' | 'Ο' => Some('o'), // Greek omicron
        'ρ' | 'Ρ' => Some('p'), // Greek rho
        'τ' | 'Τ' => Some('t'), // Greek tau
        'υ' | 'Υ' => Some('u'), // Greek upsilon
        'χ' | 'Χ' => Some('x'), // Greek chi
        
        // Number lookalikes
        'ℓ' => Some('l'),       // Script l
        '⁰' => Some('o'),       // Superscript zero
        '¹' | 'ı' => Some('i'), // Superscript one / dotless i
        '⁵' => Some('s'),       // Superscript five
        
        // Other confusables
        'ℯ' => Some('e'),       // Euler's number
        'ℴ' => Some('o'),       // Script o
        '℮' => Some('e'),       // Estimated symbol
        'ⅰ' | 'Ⅰ' => Some('i'), // Roman numeral one
        'ⅼ' | 'Ⅼ' => Some('l'), // Roman numeral fifty
        '﹢' => Some('+'),       // Small plus
        
        // Full-width characters
        c if c >= '\u{FF01}' && c <= '\u{FF5E}' => {
            // Full-width ASCII variants (！ to ～)
            Some(((c as u32) - 0xFF01 + 0x21) as u8 as char)
        }
        
        // Pass through ASCII as-is
        c if c.is_ascii() => Some(c),
        
        // Reject other non-ASCII characters for usernames
        _ => None,
    }
}

/// Normalize a username for collision detection
/// 
/// # Security Note
/// This function performs multiple normalizations to prevent homoglyph attacks:
/// 1. NFKC normalization (Unicode compatibility decomposition + canonical composition)
/// 2. Homoglyph mapping (convert lookalike characters to ASCII)
/// 3. Case folding (lowercase)
/// 
/// The result is used for collision detection - two usernames with the same
/// normalized form are considered duplicates even if they look different.
pub fn normalize_username_for_collision(username: &str) -> String {
    username
        // Step 1: NFKC normalization
        // This converts compatibility characters to their base forms
        // e.g., 'ﬁ' (fi ligature) -> "fi", '①' -> "1"
        .nfkc()
        // Step 2: Map homoglyphs to ASCII equivalents
        .filter_map(normalize_homoglyphs)
        // Step 3: Convert to lowercase
        .flat_map(|c| c.to_lowercase())
        .collect()
}

impl Identity {
    /// Create a new identity from a keypair
    /// 
    /// # Security
    /// - Username is validated for length and characters
    /// - Empty usernames are rejected
    /// - Homoglyph normalization is applied for collision detection
    pub fn new(username: String, keypair: &KeyPair) -> Self {
        // Validate and sanitize username
        let sanitized_username = Self::sanitize_username(&username);
        let normalized_username = normalize_username_for_collision(&sanitized_username);
        let public_key = keypair.public_key().to_core_public_key();

        Self {
            user_id: UserId::new(),
            username: sanitized_username,
            normalized_username,
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
    /// 
    /// # Security
    /// - Enforces ASCII-only usernames to prevent homoglyph attacks
    /// - Applies NFKC normalization for collision detection
    /// - Rejects reserved system usernames
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
        
        // SECURITY FIX: Enforce ASCII-only usernames to prevent homoglyph attacks
        // Non-ASCII characters (like Cyrillic 'а' vs Latin 'a') can create
        // visually identical usernames that are technically different, enabling
        // impersonation attacks.
        if !username.chars().all(|c| c.is_ascii()) {
            return Err(Error::identity(
                "Username must contain only ASCII characters to prevent impersonation"
            ));
        }
        
        // Check for valid characters (alphanumeric, underscore, hyphen)
        if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
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
    /// Also enforces ASCII-only to prevent homoglyph attacks
    fn sanitize_username(username: &str) -> String {
        username
            .chars()
            // SECURITY: Only allow ASCII alphanumeric, underscore, hyphen
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(MAX_USERNAME_LENGTH)
            .collect()
    }
    
    /// Get the normalized username for collision detection
    pub fn normalized_username(&self) -> &str {
        &self.normalized_username
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
    /// Index by normalized username for homoglyph collision detection
    normalized_username_index: HashMap<String, UserId>,
    pubkey_index: HashMap<Vec<u8>, UserId>,
    /// Whether to require proof-of-work for registration (enabled in production)
    require_pow: bool,
}

impl IdentityManager {
    /// Create a new identity manager
    pub fn new() -> Self {
        Self {
            identities: HashMap::new(),
            username_index: HashMap::new(),
            normalized_username_index: HashMap::new(),
            pubkey_index: HashMap::new(),
            require_pow: true, // Default to enabled for security
        }
    }
    
    /// Create identity manager with PoW disabled (for testing only)
    #[cfg(test)]
    pub fn new_without_pow() -> Self {
        Self {
            identities: HashMap::new(),
            username_index: HashMap::new(),
            normalized_username_index: HashMap::new(),
            pubkey_index: HashMap::new(),
            require_pow: false,
        }
    }
    
    /// Enable or disable proof-of-work requirement
    /// 
    /// # Security Warning
    /// Disabling PoW removes bot protection. Only disable in test environments.
    pub fn set_require_pow(&mut self, require: bool) {
        if !require {
            tracing::warn!(
                "SECURITY: Proof-of-work requirement disabled - this should only be used in testing"
            );
        }
        self.require_pow = require;
    }
    
    /// Generate a registration challenge for a new user
    /// 
    /// The client must solve this challenge (compute proof-of-work) before
    /// registration can proceed. This prevents bot account creation.
    pub fn create_registration_challenge(&self, public_key: &PublicKey) -> RegistrationChallenge {
        RegistrationChallenge::new(public_key)
    }

    /// Register a new identity with proof-of-work verification
    /// 
    /// # Security
    /// - Verifies proof-of-work to prevent bot registration
    /// - Checks for both exact username matches and homoglyph collisions
    /// - Two usernames that look similar but differ only in Unicode representation
    ///   will be rejected.
    pub fn register_identity_with_pow(
        &mut self, 
        identity: Identity,
        proof: &RegistrationProof,
    ) -> Result<()> {
        // SECURITY FIX: Verify proof-of-work before registration
        // This creates a computational cost that deters automated account creation
        if self.require_pow {
            proof.verify(&identity.public_key)?;
            tracing::info!(
                "Registration PoW verified for user {} (nonce: {})",
                identity.username,
                proof.nonce
            );
        }
        
        // Proceed with standard registration checks
        self.register_identity_internal(identity)
    }

    /// Register a new identity (legacy method - internal use only)
    /// 
    /// # Security
    /// This checks for both exact username matches and homoglyph collisions.
    /// Two usernames that look similar but differ only in Unicode representation
    /// will be rejected.
    /// 
    /// # Warning
    /// Prefer `register_identity_with_pow` for production use.
    pub fn register_identity(&mut self, identity: Identity) -> Result<()> {
        if self.require_pow {
            tracing::warn!(
                "SECURITY: register_identity called without PoW verification - \
                use register_identity_with_pow in production"
            );
        }
        self.register_identity_internal(identity)
    }
    
    /// Internal registration logic
    fn register_identity_internal(&mut self, identity: Identity) -> Result<()> {
        // Check if exact username is already taken
        if self.username_index.contains_key(&identity.username) {
            return Err(Error::identity("Username already taken"));
        }
        
        // SECURITY FIX: Check for homoglyph collisions using normalized username
        // This prevents registering "аdmin" (Cyrillic) when "admin" (Latin) exists
        if self.normalized_username_index.contains_key(&identity.normalized_username) {
            return Err(Error::identity(
                "Username too similar to existing username (possible homoglyph attack)"
            ));
        }

        // Check if public key is already registered
        let pubkey_bytes = identity.public_key.as_bytes().to_vec();
        if self.pubkey_index.contains_key(&pubkey_bytes) {
            return Err(Error::identity("Public key already registered"));
        }

        // Store identity
        let user_id = identity.user_id.clone();
        let username = identity.username.clone();
        let normalized_username = identity.normalized_username.clone();

        self.username_index.insert(username, user_id.clone());
        self.normalized_username_index.insert(normalized_username, user_id.clone());
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
        self.normalized_username_index.remove(&identity.normalized_username);
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
        // Use non-PoW manager for testing
        let mut manager = IdentityManager::new_without_pow();

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
    
    #[test]
    fn test_pow_check_difficulty() {
        // Test that check_difficulty correctly identifies leading zeros
        
        // 16 leading zero bits = 2 zero bytes
        let hash_16_zeros: [u8; 32] = [
            0x00, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert!(RegistrationProof::check_difficulty(&hash_16_zeros, 16));
        assert!(!RegistrationProof::check_difficulty(&hash_16_zeros, 17));
        
        // 20 leading zero bits = 2 zero bytes + 4 zero bits
        let hash_20_zeros: [u8; 32] = [
            0x00, 0x00, 0x0F, 0x34, 0x56, 0x78, 0x9a, 0xbc,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        assert!(RegistrationProof::check_difficulty(&hash_20_zeros, 20));
        assert!(!RegistrationProof::check_difficulty(&hash_20_zeros, 21));
    }
    
    #[test]
    fn test_pow_challenge_binding() {
        // Test that challenges are bound to specific public keys
        let keypair1 = KeyPair::generate();
        let keypair2 = KeyPair::generate();
        
        let pk1 = keypair1.public_key().to_core_public_key();
        let pk2 = keypair2.public_key().to_core_public_key();
        
        // Create challenge for pk1
        let challenge = RegistrationChallenge::new(&pk1);
        
        // Create a proof with very low difficulty for testing (we'll manually set difficulty)
        let mut test_challenge = challenge.clone();
        test_challenge.difficulty = 1; // Very easy to solve
        
        // Solve the challenge
        let proof = RegistrationProof::solve(&test_challenge);
        
        // Should verify with correct public key
        // Note: We need to use the same test_challenge difficulty
        let mut verifiable_proof = proof.clone();
        verifiable_proof.challenge.difficulty = 1;
        
        // The proof is bound to pk1's hash, so verify should fail for pk2
        // Since the challenge contains pk1's hash
        assert!(verifiable_proof.verify(&pk2).is_err());
    }
    
    #[test]
    fn test_registration_with_pow() {
        let mut manager = IdentityManager::new(); // PoW enabled
        
        let keypair = KeyPair::generate();
        let pk = keypair.public_key().to_core_public_key();
        
        // Create and solve challenge (with easy difficulty for testing)
        let mut challenge = manager.create_registration_challenge(&pk);
        challenge.difficulty = 8; // Easy difficulty for fast testing
        
        let proof = RegistrationProof::solve(&challenge);
        
        // Create identity
        let identity = Identity::new("pow_user".to_string(), &keypair);
        
        // Should succeed with valid proof
        assert!(manager.register_identity_with_pow(identity, &proof).is_ok());
    }
}

