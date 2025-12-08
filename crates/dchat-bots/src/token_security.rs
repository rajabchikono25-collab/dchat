//! Bot token security with age encryption and rotation
//!
//! Provides secure token storage, rotation API, webhook verification,
//! and lifecycle management for bot authentication tokens.

use age::secrecy::SecretString;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use thiserror::Error;

/// Bot token errors
#[derive(Debug, Error)]
pub enum TokenError {
    #[error("Storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Token not found: {0}")]
    TokenNotFound(String),

    #[error("Token expired: {0}")]
    TokenExpired(String),

    #[error("Token revoked: {0}")]
    TokenRevoked(String),

    #[error("Invalid token format")]
    InvalidFormat,

    #[error("Webhook verification failed")]
    WebhookVerificationFailed,

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Bot authentication token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotToken {
    /// Unique token ID
    pub id: String,
    /// SHA-256 hash of the actual token (for verification)
    pub token_hash: String,
    /// Bot ID this token belongs to
    pub bot_id: String,
    /// Human-readable description
    pub description: String,
    /// Token creation timestamp
    pub created_at: DateTime<Utc>,
    /// Token expiration timestamp (None = never expires)
    pub expires_at: Option<DateTime<Utc>>,
    /// Last time token was used
    pub last_used: Option<DateTime<Utc>>,
    /// Whether token has been revoked
    pub revoked: bool,
    /// Revocation timestamp
    pub revoked_at: Option<DateTime<Utc>>,
    /// Token capabilities/scopes
    pub scopes: Vec<String>,
}

impl BotToken {
    /// Create a new token
    pub fn new(
        id: String,
        token_hash: String,
        bot_id: String,
        description: String,
        scopes: Vec<String>,
        expires_in_days: Option<i64>,
    ) -> Self {
        let created_at = Utc::now();
        let expires_at = expires_in_days.map(|days| created_at + Duration::days(days));

        Self {
            id,
            token_hash,
            bot_id,
            description,
            created_at,
            expires_at,
            last_used: None,
            revoked: false,
            revoked_at: None,
            scopes,
        }
    }

    /// Check if token is valid (not expired, not revoked)
    pub fn is_valid(&self) -> bool {
        if self.revoked {
            return false;
        }

        if let Some(expires_at) = self.expires_at {
            if Utc::now() > expires_at {
                return false;
            }
        }

        true
    }

    /// Mark token as used
    pub fn mark_used(&mut self) {
        self.last_used = Some(Utc::now());
    }

    /// Revoke token
    pub fn revoke(&mut self) {
        self.revoked = true;
        self.revoked_at = Some(Utc::now());
    }
}

/// Token store with age encryption
#[derive(Debug)]
pub struct BotTokenStore {
    /// Storage file path
    storage_path: PathBuf,
    /// In-memory token cache (token_hash -> token_id)
    token_cache: HashMap<String, String>,
    /// Active tokens (token_id -> token)
    active_tokens: HashMap<String, BotToken>,
    /// Encryption passphrase
    passphrase: String,
}

impl BotTokenStore {
    /// Create a new token store
    pub fn new(storage_path: impl Into<PathBuf>, passphrase: impl Into<String>) -> Self {
        Self {
            storage_path: storage_path.into(),
            token_cache: HashMap::new(),
            active_tokens: HashMap::new(),
            passphrase: passphrase.into(),
        }
    }

    /// Load tokens from encrypted storage
    pub fn load(&mut self) -> Result<(), TokenError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let encrypted = fs::read(&self.storage_path)?;

        // Decrypt using age
        let decryptor = match age::Decryptor::new(&encrypted[..]) {
            Ok(age::Decryptor::Passphrase(d)) => d,
            _ => return Err(TokenError::Encryption("Invalid encrypted data".to_string())),
        };

        let secret = SecretString::new(self.passphrase.clone());
        let mut decrypted = vec![];
        let mut reader = decryptor
            .decrypt(&secret, None)
            .map_err(|e| TokenError::Encryption(e.to_string()))?;

        std::io::copy(&mut reader, &mut decrypted)?;

        // Deserialize tokens
        let tokens: Vec<BotToken> = serde_json::from_slice(&decrypted)?;

        // Populate cache and active tokens
        self.token_cache.clear();
        self.active_tokens.clear();

        for token in tokens {
            if token.is_valid() {
                self.token_cache
                    .insert(token.token_hash.clone(), token.id.clone());
                self.active_tokens.insert(token.id.clone(), token);
            }
        }

        Ok(())
    }

    /// Save tokens to encrypted storage
    pub fn save(&self) -> Result<(), TokenError> {
        // Serialize all tokens (including revoked for audit trail)
        let all_tokens: Vec<BotToken> = self.active_tokens.values().cloned().collect();
        let serialized = serde_json::to_vec(&all_tokens)?;

        // Encrypt using age
        let encryptor =
            age::Encryptor::with_user_passphrase(SecretString::new(self.passphrase.clone()));

        let mut encrypted = vec![];
        let mut writer = encryptor
            .wrap_output(&mut encrypted)
            .map_err(|e| TokenError::Encryption(e.to_string()))?;
        writer.write_all(&serialized)?;
        writer
            .finish()
            .map_err(|e| TokenError::Encryption(e.to_string()))?;

        // Atomic write: write to temp file, then rename
        let temp_path = self.storage_path.with_extension("tmp");
        fs::write(&temp_path, &encrypted)?;
        fs::rename(&temp_path, &self.storage_path)?;

        Ok(())
    }

    /// Generate a new token
    pub fn generate_token(
        &mut self,
        bot_id: String,
        description: String,
        scopes: Vec<String>,
        expires_in_days: Option<i64>,
    ) -> Result<(String, BotToken), TokenError> {
        // Generate random token (32 bytes = 64 hex chars)
        let mut token_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let token = hex::encode(token_bytes);

        // Hash token for storage
        let token_hash = Self::hash_token(&token);

        // Generate token ID
        let token_id = format!("tok_{}", uuid::Uuid::new_v4().simple());

        // Create token record
        let bot_token = BotToken::new(
            token_id.clone(),
            token_hash.clone(),
            bot_id,
            description,
            scopes,
            expires_in_days,
        );

        // Store token
        self.active_tokens
            .insert(token_id.clone(), bot_token.clone());
        self.token_cache.insert(token_hash, token_id);

        // Persist to storage
        self.save()?;

        Ok((token, bot_token))
    }

    /// Verify a token and return its metadata
    pub fn verify_token(&mut self, token: &str) -> Result<BotToken, TokenError> {
        let token_hash = Self::hash_token(token);

        // First, try to find the token_id from cache or by searching
        let token_id = if let Some(id) = self.token_cache.get(&token_hash) {
            id.clone()
        } else {
            // Check if token exists in active_tokens but not in cache (e.g., after revocation)
            self.active_tokens
                .iter()
                .find(|(_, t)| t.token_hash == token_hash)
                .map(|(id, _)| id.clone())
                .ok_or_else(|| TokenError::TokenNotFound(token_hash.clone()))?
        };

        let bot_token = self
            .active_tokens
            .get_mut(&token_id)
            .ok_or_else(|| TokenError::TokenNotFound(token_id.clone()))?;

        if bot_token.revoked {
            return Err(TokenError::TokenRevoked(token_id.clone()));
        }

        if let Some(expires_at) = bot_token.expires_at {
            if Utc::now() > expires_at {
                return Err(TokenError::TokenExpired(token_id.clone()));
            }
        }

        // Mark as used
        bot_token.mark_used();

        Ok(bot_token.clone())
    }

    /// Revoke a token by ID
    pub fn revoke_token(&mut self, token_id: &str) -> Result<(), TokenError> {
        let bot_token = self
            .active_tokens
            .get_mut(token_id)
            .ok_or_else(|| TokenError::TokenNotFound(token_id.to_string()))?;

        bot_token.revoke();

        // Remove from cache
        self.token_cache.remove(&bot_token.token_hash);

        // Persist to storage
        self.save()?;

        Ok(())
    }

    /// Rotate a token (revoke old, generate new)
    pub fn rotate_token(&mut self, old_token_id: &str) -> Result<(String, BotToken), TokenError> {
        // Get old token details
        let old_token = self
            .active_tokens
            .get(old_token_id)
            .ok_or_else(|| TokenError::TokenNotFound(old_token_id.to_string()))?
            .clone();

        // Generate new token with same bot_id and scopes
        let (new_token_str, new_token) = self.generate_token(
            old_token.bot_id.clone(),
            format!("Rotated from {}", old_token_id),
            old_token.scopes.clone(),
            old_token.expires_at.map(|e| (e - Utc::now()).num_days()),
        )?;

        // Revoke old token
        self.revoke_token(old_token_id)?;

        Ok((new_token_str, new_token))
    }

    /// List all tokens for a bot
    pub fn list_tokens(&self, bot_id: &str) -> Vec<BotToken> {
        self.active_tokens
            .values()
            .filter(|t| t.bot_id == bot_id)
            .cloned()
            .collect()
    }

    /// Clean up expired tokens
    pub fn cleanup_expired(&mut self) -> Result<usize, TokenError> {
        let now = Utc::now();
        let mut removed = 0;

        let expired_ids: Vec<String> = self
            .active_tokens
            .iter()
            .filter(|(_, token)| {
                if let Some(expires_at) = token.expires_at {
                    now > expires_at + Duration::days(30) // Keep for 30 days after expiry
                } else {
                    false
                }
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in expired_ids {
            if let Some(token) = self.active_tokens.remove(&id) {
                self.token_cache.remove(&token.token_hash);
                removed += 1;
            }
        }

        if removed > 0 {
            self.save()?;
        }

        Ok(removed)
    }

    /// Hash a token using SHA-256
    fn hash_token(token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// Webhook signature verification
pub struct WebhookVerifier {
    /// Shared secret for HMAC
    secret: String,
}

impl WebhookVerifier {
    /// Create a new webhook verifier
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
        }
    }

    /// Compute HMAC-SHA256 signature
    pub fn compute_signature(&self, payload: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(payload);
        hex::encode(mac.finalize().into_bytes())
    }

    /// Verify webhook signature
    /// 
    /// # Security Note
    /// This uses constant-time comparison from the `subtle` crate to prevent
    /// timing side-channel attacks. An attacker observing response times cannot
    /// learn information about the expected signature.
    pub fn verify_signature(
        &self,
        payload: &[u8],
        provided_signature: &str,
    ) -> Result<(), TokenError> {
        use subtle::ConstantTimeEq;
        
        let expected_signature = self.compute_signature(payload);

        // SECURITY FIX: Use constant-time comparison to prevent timing attacks
        // The subtle crate's ConstantTimeEq ensures the comparison takes the
        // same amount of time regardless of where the first mismatch occurs.
        //
        // Note: We compare the bytes directly. Even the length comparison is
        // handled safely because ct_eq returns 0 for different-length slices.
        let expected_bytes = expected_signature.as_bytes();
        let provided_bytes = provided_signature.as_bytes();
        
        // Constant-time comparison - this will return false for different lengths
        // but won't leak which position differs
        let is_equal = expected_bytes.ct_eq(provided_bytes);
        
        if bool::from(is_equal) {
            Ok(())
        } else {
            Err(TokenError::WebhookVerificationFailed)
        }
    }
}

/// Token manager for high-level operations
pub struct TokenManager {
    store: BotTokenStore,
    webhook_verifier: WebhookVerifier,
}

impl TokenManager {
    /// Create a new token manager
    pub fn new(
        storage_path: impl Into<PathBuf>,
        passphrase: impl Into<String>,
        webhook_secret: impl Into<String>,
    ) -> Result<Self, TokenError> {
        let mut store = BotTokenStore::new(storage_path, passphrase);
        store.load()?;

        Ok(Self {
            store,
            webhook_verifier: WebhookVerifier::new(webhook_secret),
        })
    }

    /// Generate a new token
    pub fn generate(
        &mut self,
        bot_id: String,
        description: String,
        scopes: Vec<String>,
        expires_in_days: Option<i64>,
    ) -> Result<(String, BotToken), TokenError> {
        self.store
            .generate_token(bot_id, description, scopes, expires_in_days)
    }

    /// Verify a token
    pub fn verify(&mut self, token: &str) -> Result<BotToken, TokenError> {
        self.store.verify_token(token)
    }

    /// Revoke a token
    pub fn revoke(&mut self, token_id: &str) -> Result<(), TokenError> {
        self.store.revoke_token(token_id)
    }

    /// Rotate a token
    pub fn rotate(&mut self, old_token_id: &str) -> Result<(String, BotToken), TokenError> {
        self.store.rotate_token(old_token_id)
    }

    /// List tokens for a bot
    pub fn list(&self, bot_id: &str) -> Vec<BotToken> {
        self.store.list_tokens(bot_id)
    }

    /// Verify webhook signature
    pub fn verify_webhook(&self, payload: &[u8], signature: &str) -> Result<(), TokenError> {
        self.webhook_verifier.verify_signature(payload, signature)
    }

    /// Cleanup expired tokens
    pub fn cleanup(&mut self) -> Result<usize, TokenError> {
        self.store.cleanup_expired()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_store() -> (BotTokenStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().join("tokens.age");
        let store = BotTokenStore::new(storage_path, "test-passphrase");
        (store, temp_dir)
    }

    #[test]
    fn test_generate_token() {
        let (mut store, _temp) = setup_store();

        let result = store.generate_token(
            "bot123".to_string(),
            "Test token".to_string(),
            vec!["read".to_string(), "write".to_string()],
            Some(30),
        );

        assert!(result.is_ok());
        let (token, bot_token) = result.unwrap();
        assert_eq!(token.len(), 64); // 32 bytes hex encoded
        assert_eq!(bot_token.bot_id, "bot123");
        assert!(bot_token.is_valid());
    }

    #[test]
    fn test_verify_token() {
        let (mut store, _temp) = setup_store();

        let (token, _) = store
            .generate_token("bot123".to_string(), "Test".to_string(), vec![], None)
            .unwrap();

        let verified = store.verify_token(&token);
        assert!(verified.is_ok());
        assert_eq!(verified.unwrap().bot_id, "bot123");
    }

    #[test]
    fn test_invalid_token() {
        let (mut store, _temp) = setup_store();

        let result = store.verify_token("invalid_token");
        assert!(matches!(result, Err(TokenError::TokenNotFound(_))));
    }

    #[test]
    fn test_revoke_token() {
        let (mut store, _temp) = setup_store();

        let (token, bot_token) = store
            .generate_token("bot123".to_string(), "Test".to_string(), vec![], None)
            .unwrap();

        // Verify works before revocation
        assert!(store.verify_token(&token).is_ok());

        // Revoke
        store.revoke_token(&bot_token.id).unwrap();

        // Verify fails after revocation
        let result = store.verify_token(&token);
        assert!(matches!(result, Err(TokenError::TokenRevoked(_))));
    }

    #[test]
    fn test_rotate_token() {
        let (mut store, _temp) = setup_store();

        let (old_token, old_bot_token) = store
            .generate_token("bot123".to_string(), "Test".to_string(), vec![], None)
            .unwrap();

        let (new_token, new_bot_token) = store.rotate_token(&old_bot_token.id).unwrap();

        // Old token should be revoked
        assert!(matches!(
            store.verify_token(&old_token),
            Err(TokenError::TokenRevoked(_))
        ));

        // New token should work
        assert!(store.verify_token(&new_token).is_ok());
        assert_eq!(new_bot_token.bot_id, old_bot_token.bot_id);
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().join("tokens.age");

        // Generate and save
        let (token, bot_token) = {
            let mut store = BotTokenStore::new(storage_path.clone(), "test-pass");
            store
                .generate_token("bot123".to_string(), "Test".to_string(), vec![], None)
                .unwrap()
        };

        // Load in new store instance
        let mut store2 = BotTokenStore::new(storage_path, "test-pass");
        store2.load().unwrap();

        // Verify token still works
        let verified = store2.verify_token(&token).unwrap();
        assert_eq!(verified.id, bot_token.id);
        assert_eq!(verified.bot_id, "bot123");
    }

    #[test]
    fn test_token_expiration() {
        let (mut store, _temp) = setup_store();

        // Create token with 0 days expiry (expires immediately)
        let (token, _) = store
            .generate_token("bot123".to_string(), "Test".to_string(), vec![], Some(0))
            .unwrap();

        // Should fail verification due to expiration
        let result = store.verify_token(&token);
        assert!(matches!(result, Err(TokenError::TokenExpired(_))));
    }

    #[test]
    fn test_webhook_verification() {
        let verifier = WebhookVerifier::new("secret123");

        let payload = b"test payload";
        let signature = verifier.compute_signature(payload);

        // Valid signature should pass
        assert!(verifier.verify_signature(payload, &signature).is_ok());

        // Invalid signature should fail
        assert!(verifier
            .verify_signature(payload, "invalid_signature")
            .is_err());
    }

    #[test]
    fn test_token_manager() {
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().join("tokens.age");

        let mut manager = TokenManager::new(storage_path, "passphrase", "webhook-secret").unwrap();

        // Generate token
        let (token, bot_token) = manager
            .generate("bot123".to_string(), "Test".to_string(), vec![], None)
            .unwrap();

        // Verify token
        assert!(manager.verify(&token).is_ok());

        // Verify webhook
        let payload = b"webhook payload";
        let signature = manager.webhook_verifier.compute_signature(payload);
        assert!(manager.verify_webhook(payload, &signature).is_ok());

        // Revoke token
        assert!(manager.revoke(&bot_token.id).is_ok());
        assert!(matches!(
            manager.verify(&token),
            Err(TokenError::TokenRevoked(_))
        ));
    }

    #[test]
    fn test_list_tokens() {
        let (mut store, _temp) = setup_store();

        store
            .generate_token("bot123".to_string(), "Token 1".to_string(), vec![], None)
            .unwrap();
        store
            .generate_token("bot123".to_string(), "Token 2".to_string(), vec![], None)
            .unwrap();
        store
            .generate_token("bot456".to_string(), "Token 3".to_string(), vec![], None)
            .unwrap();

        let bot123_tokens = store.list_tokens("bot123");
        assert_eq!(bot123_tokens.len(), 2);

        let bot456_tokens = store.list_tokens("bot456");
        assert_eq!(bot456_tokens.len(), 1);
    }

    #[test]
    fn test_cleanup_expired() {
        let (mut store, _temp) = setup_store();

        // Create expired token (would need to manipulate time or wait 30 days in real scenario)
        // For now, just test that cleanup runs without error
        let result = store.cleanup_expired();
        assert!(result.is_ok());
    }
}
