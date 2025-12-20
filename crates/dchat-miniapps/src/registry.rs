//! Mini-app registry - on-chain registration with signed package provenance

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{MiniAppError, MiniAppResult};
use crate::manifest::AppManifest;
use crate::permissions::PermissionSet;

/// Unique app identifier (deterministic from developer + name)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppId(pub [u8; 32]);

impl AppId {
    /// Create app ID from developer and app name
    pub fn derive(developer_id: &DeveloperId, app_name: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&developer_id.0);
        hasher.update(app_name.as_bytes());
        Self(hasher.finalize().into())
    }

    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from hex string
    pub fn from_hex(s: &str) -> MiniAppResult<Self> {
        let bytes =
            hex::decode(s).map_err(|_| MiniAppError::InvalidAppId("invalid hex".to_string()))?;
        if bytes.len() != 32 {
            return Err(MiniAppError::InvalidAppId("invalid length".to_string()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

impl std::fmt::Display for AppId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Developer identifier (derived from public key)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeveloperId(pub [u8; 32]);

impl DeveloperId {
    /// Create from public key
    pub fn from_public_key(pubkey: &VerifyingKey) -> Self {
        let hash = blake3::hash(pubkey.as_bytes());
        Self(hash.into())
    }

    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Display for DeveloperId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Developer registration status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeveloperStatus {
    /// Pending verification
    Pending,
    /// Verified developer
    Verified,
    /// Suspended (can be reinstated)
    Suspended,
    /// Permanently banned
    Banned,
}

/// Developer information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Developer {
    /// Developer ID
    pub id: DeveloperId,
    /// Display name
    pub name: String,
    /// Public key for signature verification
    pub public_key: [u8; 32],
    /// Developer status
    pub status: DeveloperStatus,
    /// Verification timestamp
    pub verified_at: Option<DateTime<Utc>>,
    /// Registration timestamp
    pub registered_at: DateTime<Utc>,
    /// Optional website URL
    pub website: Option<String>,
    /// Optional contact email
    pub email: Option<String>,
    /// Reputation score (0-1000)
    pub reputation: u16,
    /// Total downloads across all apps
    pub total_downloads: u64,
    /// Number of registered apps
    pub app_count: u32,
}

impl Developer {
    /// Create new developer
    pub fn new(name: String, public_key: [u8; 32]) -> Self {
        let verifying_key = VerifyingKey::from_bytes(&public_key).expect("valid public key");
        let id = DeveloperId::from_public_key(&verifying_key);

        Self {
            id,
            name,
            public_key,
            status: DeveloperStatus::Pending,
            verified_at: None,
            registered_at: Utc::now(),
            website: None,
            email: None,
            reputation: 0,
            total_downloads: 0,
            app_count: 0,
        }
    }

    /// Check if developer is verified
    pub fn is_verified(&self) -> bool {
        self.status == DeveloperStatus::Verified
    }

    /// Check if developer can register apps
    pub fn can_register_apps(&self) -> bool {
        matches!(self.status, DeveloperStatus::Verified)
    }

    /// Verify signature from this developer
    pub fn verify_signature(&self, message: &[u8], signature: &[u8; 64]) -> MiniAppResult<()> {
        let verifying_key = VerifyingKey::from_bytes(&self.public_key)
            .map_err(|_| MiniAppError::InvalidPublicKey("invalid developer key".to_string()))?;

        let sig = Signature::from_bytes(signature);
        verifying_key.verify(message, &sig)?;
        Ok(())
    }
}

/// Registration status for an app
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistrationStatus {
    /// Pending review
    Pending,
    /// Approved and active
    Active,
    /// Suspended (can be reinstated)
    Suspended,
    /// Permanently revoked
    Revoked,
    /// Deprecated (still accessible but no updates)
    Deprecated,
}

/// App registration entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRegistration {
    /// App ID
    pub app_id: AppId,
    /// Developer ID
    pub developer_id: DeveloperId,
    /// App name (unique per developer)
    pub name: String,
    /// Current version
    pub version: semver::Version,
    /// App manifest
    pub manifest: AppManifest,
    /// Content hash of app bundle
    pub bundle_hash: [u8; 32],
    /// Registration status
    pub status: RegistrationStatus,
    /// Registration timestamp
    pub registered_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    /// Developer signature over registration
    pub signature: [u8; 64],
    /// Download count
    pub downloads: u64,
    /// User rating (0-500, representing 0-5 stars * 100)
    pub rating: u16,
    /// Number of ratings
    pub rating_count: u32,
    /// Permissions declared by app
    pub permissions: PermissionSet,
    /// Optional expiry date
    pub expires_at: Option<DateTime<Utc>>,
}

impl AppRegistration {
    /// Check if registration is active
    pub fn is_active(&self) -> bool {
        if self.status != RegistrationStatus::Active {
            return false;
        }

        if let Some(expires_at) = self.expires_at {
            if Utc::now() > expires_at {
                return false;
            }
        }

        true
    }

    /// Compute signature payload
    pub fn signature_payload(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&self.app_id.0);
        payload.extend_from_slice(&self.developer_id.0);
        payload.extend_from_slice(self.name.as_bytes());
        payload.extend_from_slice(self.version.to_string().as_bytes());
        payload.extend_from_slice(&self.bundle_hash);
        payload
    }

    /// Verify developer signature
    pub fn verify_signature(&self, developer: &Developer) -> MiniAppResult<()> {
        let payload = self.signature_payload();
        developer.verify_signature(&payload, &self.signature)
    }
}

/// Verified app (after all checks pass)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedApp {
    /// Registration data
    pub registration: AppRegistration,
    /// Developer info
    pub developer: Developer,
    /// Verification timestamp
    pub verified_at: DateTime<Utc>,
    /// Verification hash (hash of all verification inputs)
    pub verification_hash: [u8; 32],
}

impl VerifiedApp {
    /// Create verified app after validation
    pub fn new(registration: AppRegistration, developer: Developer) -> MiniAppResult<Self> {
        // Verify developer is allowed
        if !developer.can_register_apps() {
            return Err(MiniAppError::DeveloperNotVerified(developer.id.to_hex()));
        }

        // Verify signature
        registration.verify_signature(&developer)?;

        // Compute verification hash
        let mut hasher = blake3::Hasher::new();
        hasher.update(&registration.app_id.0);
        hasher.update(&registration.bundle_hash);
        hasher.update(&registration.signature);
        hasher.update(&developer.public_key);
        let verification_hash = hasher.finalize().into();

        Ok(Self {
            registration,
            developer,
            verified_at: Utc::now(),
            verification_hash,
        })
    }
}

/// Developer registry (manages developer accounts)
#[derive(Default)]
pub struct DeveloperRegistry {
    /// Developers by ID
    developers: RwLock<HashMap<DeveloperId, Developer>>,
    /// Developers by public key (for lookup)
    by_pubkey: RwLock<HashMap<[u8; 32], DeveloperId>>,
}

impl DeveloperRegistry {
    /// Create new registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register new developer
    pub fn register(&self, developer: Developer) -> MiniAppResult<DeveloperId> {
        let mut developers = self.developers.write();
        let mut by_pubkey = self.by_pubkey.write();

        // Check if public key already registered
        if by_pubkey.contains_key(&developer.public_key) {
            return Err(MiniAppError::DeveloperNotVerified(
                "public key already registered".to_string(),
            ));
        }

        let id = developer.id;
        by_pubkey.insert(developer.public_key, id);
        developers.insert(id, developer);

        Ok(id)
    }

    /// Get developer by ID
    pub fn get(&self, id: &DeveloperId) -> Option<Developer> {
        self.developers.read().get(id).cloned()
    }

    /// Get developer by public key
    pub fn get_by_pubkey(&self, pubkey: &[u8; 32]) -> Option<Developer> {
        let id = self.by_pubkey.read().get(pubkey).cloned()?;
        self.get(&id)
    }

    /// Verify developer
    pub fn verify(&self, id: &DeveloperId) -> MiniAppResult<()> {
        let mut developers = self.developers.write();
        let developer = developers
            .get_mut(id)
            .ok_or_else(|| MiniAppError::DeveloperNotFound(id.to_hex()))?;

        developer.status = DeveloperStatus::Verified;
        developer.verified_at = Some(Utc::now());
        Ok(())
    }

    /// Suspend developer
    pub fn suspend(&self, id: &DeveloperId, reason: &str) -> MiniAppResult<()> {
        let mut developers = self.developers.write();
        let developer = developers
            .get_mut(id)
            .ok_or_else(|| MiniAppError::DeveloperNotFound(id.to_hex()))?;

        developer.status = DeveloperStatus::Suspended;
        Ok(())
    }

    /// Update reputation
    pub fn update_reputation(&self, id: &DeveloperId, delta: i16) -> MiniAppResult<()> {
        let mut developers = self.developers.write();
        let developer = developers
            .get_mut(id)
            .ok_or_else(|| MiniAppError::DeveloperNotFound(id.to_hex()))?;

        let new_rep = (developer.reputation as i32 + delta as i32).clamp(0, 1000) as u16;
        developer.reputation = new_rep;
        Ok(())
    }
}

/// Mini-app registry (manages app registrations)
pub struct MiniAppRegistry {
    /// App registrations by ID
    apps: RwLock<HashMap<AppId, AppRegistration>>,
    /// Apps by developer
    by_developer: RwLock<HashMap<DeveloperId, Vec<AppId>>>,
    /// Developer registry
    developers: Arc<DeveloperRegistry>,
}

impl MiniAppRegistry {
    /// Create new registry
    pub fn new(developers: Arc<DeveloperRegistry>) -> Self {
        Self {
            apps: RwLock::new(HashMap::new()),
            by_developer: RwLock::new(HashMap::new()),
            developers,
        }
    }

    /// Register new app
    pub fn register(&self, registration: AppRegistration) -> MiniAppResult<AppId> {
        // Verify developer exists and can register
        let developer = self
            .developers
            .get(&registration.developer_id)
            .ok_or_else(|| MiniAppError::DeveloperNotFound(registration.developer_id.to_hex()))?;

        if !developer.can_register_apps() {
            return Err(MiniAppError::DeveloperNotVerified(developer.id.to_hex()));
        }

        // Verify signature
        registration.verify_signature(&developer)?;

        let app_id = registration.app_id;

        // Check if already registered
        {
            let apps = self.apps.read();
            if apps.contains_key(&app_id) {
                return Err(MiniAppError::AppAlreadyRegistered(app_id.to_hex()));
            }
        }

        // Register app
        {
            let mut apps = self.apps.write();
            let mut by_developer = self.by_developer.write();

            apps.insert(app_id, registration.clone());
            by_developer
                .entry(registration.developer_id)
                .or_default()
                .push(app_id);
        }

        Ok(app_id)
    }

    /// Get app by ID
    pub fn get(&self, app_id: &AppId) -> Option<AppRegistration> {
        self.apps.read().get(app_id).cloned()
    }

    /// Get verified app (includes developer info)
    pub fn get_verified(&self, app_id: &AppId) -> MiniAppResult<VerifiedApp> {
        let registration = self
            .apps
            .read()
            .get(app_id)
            .cloned()
            .ok_or_else(|| MiniAppError::AppNotFound(app_id.to_hex()))?;

        if !registration.is_active() {
            return Err(MiniAppError::RegistrationRevoked(app_id.to_hex()));
        }

        let developer = self
            .developers
            .get(&registration.developer_id)
            .ok_or_else(|| MiniAppError::DeveloperNotFound(registration.developer_id.to_hex()))?;

        VerifiedApp::new(registration, developer)
    }

    /// Get apps by developer
    pub fn get_by_developer(&self, developer_id: &DeveloperId) -> Vec<AppRegistration> {
        let by_developer = self.by_developer.read();
        let apps = self.apps.read();

        by_developer
            .get(developer_id)
            .map(|ids| ids.iter().filter_map(|id| apps.get(id).cloned()).collect())
            .unwrap_or_default()
    }

    /// Update app
    pub fn update(
        &self,
        app_id: &AppId,
        new_version: semver::Version,
        new_bundle_hash: [u8; 32],
        new_manifest: AppManifest,
        signature: [u8; 64],
    ) -> MiniAppResult<()> {
        let mut apps = self.apps.write();
        let registration = apps
            .get_mut(app_id)
            .ok_or_else(|| MiniAppError::AppNotFound(app_id.to_hex()))?;

        // Verify developer can still update
        let developer = self
            .developers
            .get(&registration.developer_id)
            .ok_or_else(|| MiniAppError::DeveloperNotFound(registration.developer_id.to_hex()))?;

        if !developer.can_register_apps() {
            return Err(MiniAppError::DeveloperNotVerified(developer.id.to_hex()));
        }

        // Version must be higher
        if new_version <= registration.version {
            return Err(MiniAppError::InvalidManifestVersion(format!(
                "new version {} must be higher than current {}",
                new_version, registration.version
            )));
        }

        // Update fields
        registration.version = new_version;
        registration.bundle_hash = new_bundle_hash;
        registration.manifest = new_manifest;
        registration.signature = signature;
        registration.updated_at = Utc::now();

        // Verify new signature
        registration.verify_signature(&developer)?;

        Ok(())
    }

    /// Suspend app
    pub fn suspend(&self, app_id: &AppId, reason: &str) -> MiniAppResult<()> {
        let mut apps = self.apps.write();
        let registration = apps
            .get_mut(app_id)
            .ok_or_else(|| MiniAppError::AppNotFound(app_id.to_hex()))?;

        registration.status = RegistrationStatus::Suspended;
        Ok(())
    }

    /// Revoke app
    pub fn revoke(&self, app_id: &AppId, reason: &str) -> MiniAppResult<()> {
        let mut apps = self.apps.write();
        let registration = apps
            .get_mut(app_id)
            .ok_or_else(|| MiniAppError::AppNotFound(app_id.to_hex()))?;

        registration.status = RegistrationStatus::Revoked;
        Ok(())
    }

    /// Increment download count
    pub fn record_download(&self, app_id: &AppId) -> MiniAppResult<()> {
        let mut apps = self.apps.write();
        let registration = apps
            .get_mut(app_id)
            .ok_or_else(|| MiniAppError::AppNotFound(app_id.to_hex()))?;

        registration.downloads += 1;
        Ok(())
    }

    /// Add rating
    pub fn add_rating(&self, app_id: &AppId, rating: u16) -> MiniAppResult<()> {
        if rating > 500 {
            return Err(MiniAppError::InvalidManifest(
                "rating must be 0-500".to_string(),
            ));
        }

        let mut apps = self.apps.write();
        let registration = apps
            .get_mut(app_id)
            .ok_or_else(|| MiniAppError::AppNotFound(app_id.to_hex()))?;

        // Update running average
        let total = registration.rating as u64 * registration.rating_count as u64 + rating as u64;
        registration.rating_count += 1;
        registration.rating = (total / registration.rating_count as u64) as u16;

        Ok(())
    }

    /// List all active apps
    pub fn list_active(&self) -> Vec<AppRegistration> {
        self.apps
            .read()
            .values()
            .filter(|r| r.is_active())
            .cloned()
            .collect()
    }

    /// Search apps by name
    pub fn search(&self, query: &str) -> Vec<AppRegistration> {
        let query_lower = query.to_lowercase();
        self.apps
            .read()
            .values()
            .filter(|r| r.is_active() && r.name.to_lowercase().contains(&query_lower))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn create_test_developer() -> (Developer, SigningKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let public_key = signing_key.verifying_key();
        let developer = Developer::new("Test Developer".to_string(), public_key.to_bytes());
        (developer, signing_key)
    }

    #[test]
    fn test_app_id_derivation() {
        let developer_id = DeveloperId([1u8; 32]);
        let app_id = AppId::derive(&developer_id, "myapp");

        // Same inputs should produce same ID
        let app_id2 = AppId::derive(&developer_id, "myapp");
        assert_eq!(app_id, app_id2);

        // Different name should produce different ID
        let app_id3 = AppId::derive(&developer_id, "otherapp");
        assert_ne!(app_id, app_id3);
    }

    #[test]
    fn test_developer_registration() {
        let registry = DeveloperRegistry::new();
        let (developer, _) = create_test_developer();
        let id = developer.id;

        let result = registry.register(developer);
        assert!(result.is_ok());

        let loaded = registry.get(&id);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().name, "Test Developer");
    }

    #[test]
    fn test_developer_verification() {
        let registry = DeveloperRegistry::new();
        let (developer, _) = create_test_developer();
        let id = developer.id;

        registry.register(developer).unwrap();

        // Initially not verified
        let dev = registry.get(&id).unwrap();
        assert!(!dev.is_verified());

        // Verify
        registry.verify(&id).unwrap();

        let dev = registry.get(&id).unwrap();
        assert!(dev.is_verified());
        assert!(dev.can_register_apps());
    }

    #[test]
    fn test_signature_verification() {
        let (developer, signing_key) = create_test_developer();

        let message = b"test message";
        let signature = signing_key.sign(message);

        let result = developer.verify_signature(message, &signature.to_bytes());
        assert!(result.is_ok());

        // Wrong message should fail
        let wrong_message = b"wrong message";
        let result = developer.verify_signature(wrong_message, &signature.to_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn test_app_id_hex_roundtrip() {
        let app_id = AppId([42u8; 32]);
        let hex = app_id.to_hex();
        let parsed = AppId::from_hex(&hex).unwrap();
        assert_eq!(app_id, parsed);
    }
}
