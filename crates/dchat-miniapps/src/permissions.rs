//! Permission model - deny-by-default permissions with user consent

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{MiniAppError, MiniAppResult};
use crate::registry::AppId;

/// Permission type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    // ─── User Data Permissions ──────────────────────────────────────────────
    /// Read user's basic profile (name, avatar)
    ReadProfile,
    /// Read user's contact list
    ReadContacts,
    /// Read user's chat history (specific channel)
    ReadChatHistory,
    /// Access user's public key
    ReadPublicKey,

    // ─── Wallet Permissions ─────────────────────────────────────────────────
    /// View wallet balance
    ViewBalance,
    /// Request payments (show payment UI)
    RequestPayment,
    /// Send tokens (requires explicit approval per transaction)
    SendTokens,
    /// Access wallet address
    WalletAddress,

    // ─── Messaging Permissions ──────────────────────────────────────────────
    /// Send messages on behalf of user
    SendMessages,
    /// Create message attachments
    SendAttachments,
    /// Access message reactions
    AccessReactions,

    // ─── Device Permissions ─────────────────────────────────────────────────
    /// Access device camera
    Camera,
    /// Access device microphone
    Microphone,
    /// Access device location
    Location,
    /// Access device clipboard
    Clipboard,
    /// Push notifications
    Notifications,
    /// Biometric authentication
    Biometrics,

    // ─── Storage Permissions ────────────────────────────────────────────────
    /// Local storage (sandboxed)
    LocalStorage,
    /// Cloud storage (user's dchat storage)
    CloudStorage,
    /// File system access (sandboxed)
    FileSystem,

    // ─── Network Permissions ────────────────────────────────────────────────
    /// Access to external network
    Network,
    /// WebSocket connections
    WebSocket,

    // ─── System Permissions ─────────────────────────────────────────────────
    /// Run in background
    Background,
    /// Keep screen awake
    KeepAwake,
    /// Vibration
    Vibrate,
    /// Share content
    Share,

    // ─── Special Permissions ────────────────────────────────────────────────
    /// Cross-chain operations
    CrossChain,
    /// Bot actions
    BotActions,
    /// Inline queries
    InlineQueries,
}

impl Permission {
    /// Get permission display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ReadProfile => "Read Profile",
            Self::ReadContacts => "Read Contacts",
            Self::ReadChatHistory => "Read Chat History",
            Self::ReadPublicKey => "Read Public Key",
            Self::ViewBalance => "View Balance",
            Self::RequestPayment => "Request Payment",
            Self::SendTokens => "Send Tokens",
            Self::WalletAddress => "Wallet Address",
            Self::SendMessages => "Send Messages",
            Self::SendAttachments => "Send Attachments",
            Self::AccessReactions => "Access Reactions",
            Self::Camera => "Camera",
            Self::Microphone => "Microphone",
            Self::Location => "Location",
            Self::Clipboard => "Clipboard",
            Self::Notifications => "Notifications",
            Self::Biometrics => "Biometrics",
            Self::LocalStorage => "Local Storage",
            Self::CloudStorage => "Cloud Storage",
            Self::FileSystem => "File System",
            Self::Network => "Network",
            Self::WebSocket => "WebSocket",
            Self::Background => "Background",
            Self::KeepAwake => "Keep Awake",
            Self::Vibrate => "Vibrate",
            Self::Share => "Share",
            Self::CrossChain => "Cross-Chain",
            Self::BotActions => "Bot Actions",
            Self::InlineQueries => "Inline Queries",
        }
    }

    /// Get permission description
    pub fn description(&self) -> &'static str {
        match self {
            Self::ReadProfile => "Access your name and avatar",
            Self::ReadContacts => "View your contact list",
            Self::ReadChatHistory => "Read messages in specific chats",
            Self::ReadPublicKey => "Access your public cryptographic key",
            Self::ViewBalance => "View your wallet balance",
            Self::RequestPayment => "Show payment requests",
            Self::SendTokens => "Transfer tokens from your wallet",
            Self::WalletAddress => "Access your wallet address",
            Self::SendMessages => "Send messages on your behalf",
            Self::SendAttachments => "Attach files to messages",
            Self::AccessReactions => "See and add reactions to messages",
            Self::Camera => "Take photos and videos",
            Self::Microphone => "Record audio",
            Self::Location => "Access your location",
            Self::Clipboard => "Read and write clipboard",
            Self::Notifications => "Send push notifications",
            Self::Biometrics => "Use fingerprint or face recognition",
            Self::LocalStorage => "Store data on your device",
            Self::CloudStorage => "Store data in your dchat cloud",
            Self::FileSystem => "Access sandboxed files",
            Self::Network => "Connect to external servers",
            Self::WebSocket => "Maintain real-time connections",
            Self::Background => "Run when app is in background",
            Self::KeepAwake => "Prevent device from sleeping",
            Self::Vibrate => "Make device vibrate",
            Self::Share => "Share content with other apps",
            Self::CrossChain => "Execute cross-chain transactions",
            Self::BotActions => "Perform automated actions",
            Self::InlineQueries => "Respond to inline mentions",
        }
    }

    /// Get risk level
    pub fn risk_level(&self) -> RiskLevel {
        match self {
            // Low risk - minimal data access
            Self::LocalStorage
            | Self::Vibrate
            | Self::Share
            | Self::Notifications
            | Self::KeepAwake
            | Self::AccessReactions => RiskLevel::Low,

            // Medium risk - some user data
            Self::ReadProfile
            | Self::ReadPublicKey
            | Self::WalletAddress
            | Self::ViewBalance
            | Self::Network
            | Self::WebSocket
            | Self::Clipboard
            | Self::Background
            | Self::BotActions
            | Self::InlineQueries => RiskLevel::Medium,

            // High risk - sensitive data or actions
            Self::ReadContacts
            | Self::ReadChatHistory
            | Self::SendMessages
            | Self::SendAttachments
            | Self::RequestPayment
            | Self::Camera
            | Self::Microphone
            | Self::Location
            | Self::CloudStorage
            | Self::FileSystem
            | Self::Biometrics => RiskLevel::High,

            // Critical risk - financial actions
            Self::SendTokens | Self::CrossChain => RiskLevel::Critical,
        }
    }

    /// Check if permission requires explicit per-use approval
    pub fn requires_per_use_approval(&self) -> bool {
        matches!(self, Self::SendTokens | Self::CrossChain | Self::Biometrics)
    }
}

/// Permission risk level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Low risk
    Low,
    /// Medium risk
    Medium,
    /// High risk
    High,
    /// Critical risk (requires special approval)
    Critical,
}

impl RiskLevel {
    /// Get display color (for UI)
    pub fn color(&self) -> &'static str {
        match self {
            Self::Low => "#34C759",      // Green
            Self::Medium => "#FF9500",   // Orange
            Self::High => "#FF3B30",     // Red
            Self::Critical => "#AF52DE", // Purple
        }
    }
}

/// Permission scope (optional restrictions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionScope {
    /// Specific channels this permission applies to
    pub channels: Option<Vec<String>>,
    /// Maximum amount for financial permissions
    pub max_amount: Option<u64>,
    /// Maximum uses count
    pub max_uses: Option<u32>,
    /// Time window for rate limiting
    pub time_window: Option<Duration>,
    /// Custom restrictions
    pub custom: Option<serde_json::Value>,
}

impl PermissionScope {
    /// Create unrestricted scope
    pub fn unrestricted() -> Self {
        Self {
            channels: None,
            max_amount: None,
            max_uses: None,
            time_window: None,
            custom: None,
        }
    }

    /// Create scope with channel restriction
    pub fn channels(channels: Vec<String>) -> Self {
        Self {
            channels: Some(channels),
            ..Self::unrestricted()
        }
    }

    /// Create scope with amount limit
    pub fn with_limit(max_amount: u64) -> Self {
        Self {
            max_amount: Some(max_amount),
            ..Self::unrestricted()
        }
    }
}

/// Set of permissions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionSet {
    permissions: HashSet<Permission>,
}

impl PermissionSet {
    /// Create empty set
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from iterator
    pub fn from_iter<I: IntoIterator<Item = Permission>>(iter: I) -> Self {
        Self {
            permissions: iter.into_iter().collect(),
        }
    }

    /// Insert permission
    pub fn insert(&mut self, permission: Permission) {
        self.permissions.insert(permission);
    }

    /// Add permission (alias for insert)
    pub fn add(&mut self, permission: Permission) {
        self.insert(permission);
    }

    /// Remove permission
    pub fn remove(&mut self, permission: &Permission) -> bool {
        self.permissions.remove(permission)
    }

    /// Clear all permissions
    pub fn clear(&mut self) {
        self.permissions.clear();
    }

    /// Check if permission is in set
    pub fn contains(&self, permission: &Permission) -> bool {
        self.permissions.contains(permission)
    }

    /// Check if permission is in set (alias for contains)
    pub fn has(&self, permission: &Permission) -> bool {
        self.contains(permission)
    }

    /// Get number of permissions
    pub fn len(&self) -> usize {
        self.permissions.len()
    }

    /// Get number of permissions (alias for len)
    pub fn count(&self) -> usize {
        self.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.permissions.is_empty()
    }

    /// Iterate over permissions
    pub fn iter(&self) -> impl Iterator<Item = &Permission> {
        self.permissions.iter()
    }

    /// Get permissions sorted by risk level
    pub fn sorted_by_risk(&self) -> Vec<Permission> {
        let mut perms: Vec<_> = self.permissions.iter().cloned().collect();
        perms.sort_by(|a, b| b.risk_level().cmp(&a.risk_level()));
        perms
    }

    /// Get highest risk level in set
    pub fn max_risk_level(&self) -> Option<RiskLevel> {
        self.permissions.iter().map(|p| p.risk_level()).max()
    }

    /// Check if set contains any critical permissions
    pub fn has_critical(&self) -> bool {
        self.permissions
            .iter()
            .any(|p| p.risk_level() == RiskLevel::Critical)
    }
}

/// Permission request from app
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// App requesting permissions
    pub app_id: AppId,
    /// Requested permissions
    pub permissions: Vec<Permission>,
    /// Optional scopes for each permission
    pub scopes: Vec<Option<PermissionScope>>,
    /// Reason for each permission
    pub reasons: Vec<String>,
    /// Request timestamp
    pub requested_at: DateTime<Utc>,
    /// Request ID (for tracking)
    pub request_id: uuid::Uuid,
}

impl PermissionRequest {
    /// Create new permission request
    pub fn new(app_id: AppId, permissions: Vec<Permission>) -> Self {
        Self {
            app_id,
            scopes: vec![None; permissions.len()],
            reasons: vec![String::new(); permissions.len()],
            permissions,
            requested_at: Utc::now(),
            request_id: uuid::Uuid::new_v4(),
        }
    }

    /// Add scope for permission
    pub fn with_scope(mut self, index: usize, scope: PermissionScope) -> Self {
        if index < self.scopes.len() {
            self.scopes[index] = Some(scope);
        }
        self
    }

    /// Add reason for permission
    pub fn with_reason(mut self, index: usize, reason: &str) -> Self {
        if index < self.reasons.len() {
            self.reasons[index] = reason.to_string();
        }
        self
    }

    /// Validate request
    pub fn validate(&self) -> MiniAppResult<()> {
        if self.permissions.is_empty() {
            return Err(MiniAppError::InvalidPermissionScope(
                "no permissions requested".to_string(),
            ));
        }

        if self.permissions.len() > crate::MAX_PERMISSIONS_PER_APP {
            return Err(MiniAppError::TooManyPermissions {
                count: self.permissions.len(),
                max: crate::MAX_PERMISSIONS_PER_APP,
            });
        }

        // Check for duplicates
        let unique: HashSet<_> = self.permissions.iter().collect();
        if unique.len() != self.permissions.len() {
            return Err(MiniAppError::InvalidPermissionScope(
                "duplicate permissions".to_string(),
            ));
        }

        Ok(())
    }
}

/// Granted permission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGrant {
    /// App that was granted permission
    pub app_id: AppId,
    /// User who granted permission
    pub user_id: [u8; 32],
    /// Granted permission
    pub permission: Permission,
    /// Scope (restrictions)
    pub scope: PermissionScope,
    /// When granted
    pub granted_at: DateTime<Utc>,
    /// When expires (if any)
    pub expires_at: Option<DateTime<Utc>>,
    /// Number of times used
    pub use_count: u32,
    /// Last used timestamp
    pub last_used_at: Option<DateTime<Utc>>,
    /// Whether permission is currently active
    pub active: bool,
}

impl PermissionGrant {
    /// Create new grant
    pub fn new(
        app_id: AppId,
        user_id: [u8; 32],
        permission: Permission,
        scope: PermissionScope,
        duration: Option<Duration>,
    ) -> Self {
        let now = Utc::now();
        Self {
            app_id,
            user_id,
            permission,
            scope,
            granted_at: now,
            expires_at: duration.map(|d| now + d),
            use_count: 0,
            last_used_at: None,
            active: true,
        }
    }

    /// Check if grant is still valid
    pub fn is_valid(&self) -> bool {
        if !self.active {
            return false;
        }

        if let Some(expires_at) = self.expires_at {
            if Utc::now() > expires_at {
                return false;
            }
        }

        if let Some(max_uses) = self.scope.max_uses {
            if self.use_count >= max_uses {
                return false;
            }
        }

        true
    }

    /// Record usage
    pub fn record_use(&mut self) -> MiniAppResult<()> {
        if !self.is_valid() {
            return Err(MiniAppError::PermissionExpired(
                self.permission.display_name().to_string(),
            ));
        }

        self.use_count += 1;
        self.last_used_at = Some(Utc::now());
        Ok(())
    }

    /// Revoke grant
    pub fn revoke(&mut self) {
        self.active = false;
    }
}

/// Permission manager for a user
#[derive(Default)]
pub struct PermissionManager {
    /// Grants by (app_id, permission)
    grants: std::collections::HashMap<(AppId, Permission), PermissionGrant>,
}

impl PermissionManager {
    /// Create new manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant permission
    pub fn grant(
        &mut self,
        app_id: AppId,
        user_id: [u8; 32],
        permission: Permission,
        scope: PermissionScope,
        duration: Option<Duration>,
    ) -> MiniAppResult<()> {
        let key = (app_id, permission);

        if self.grants.contains_key(&key) {
            return Err(MiniAppError::PermissionAlreadyGranted(
                permission.display_name().to_string(),
            ));
        }

        let grant = PermissionGrant::new(app_id, user_id, permission, scope, duration);
        self.grants.insert(key, grant);
        Ok(())
    }

    /// Check if permission is granted
    pub fn check(&self, app_id: &AppId, permission: &Permission) -> bool {
        self.grants
            .get(&(*app_id, *permission))
            .map(|g| g.is_valid())
            .unwrap_or(false)
    }

    /// Use permission (records usage)
    pub fn use_permission(&mut self, app_id: &AppId, permission: &Permission) -> MiniAppResult<()> {
        let key = (*app_id, *permission);
        let grant = self.grants.get_mut(&key).ok_or_else(|| {
            MiniAppError::PermissionNotGranted(permission.display_name().to_string())
        })?;

        grant.record_use()
    }

    /// Revoke permission
    pub fn revoke(&mut self, app_id: &AppId, permission: &Permission) -> MiniAppResult<()> {
        let key = (*app_id, *permission);
        let grant = self.grants.get_mut(&key).ok_or_else(|| {
            MiniAppError::PermissionNotGranted(permission.display_name().to_string())
        })?;

        grant.revoke();
        Ok(())
    }

    /// Revoke all permissions for an app
    pub fn revoke_all(&mut self, app_id: &AppId) {
        for ((aid, _), grant) in self.grants.iter_mut() {
            if aid == app_id {
                grant.revoke();
            }
        }
    }

    /// Get all grants for an app
    pub fn get_grants(&self, app_id: &AppId) -> Vec<&PermissionGrant> {
        self.grants
            .iter()
            .filter(|((aid, _), _)| aid == app_id)
            .map(|(_, g)| g)
            .collect()
    }

    /// Get all active grants for an app
    pub fn get_active_grants(&self, app_id: &AppId) -> Vec<&PermissionGrant> {
        self.get_grants(app_id)
            .into_iter()
            .filter(|g| g.is_valid())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_risk_levels() {
        assert_eq!(Permission::LocalStorage.risk_level(), RiskLevel::Low);
        assert_eq!(Permission::ReadProfile.risk_level(), RiskLevel::Medium);
        assert_eq!(Permission::Camera.risk_level(), RiskLevel::High);
        assert_eq!(Permission::SendTokens.risk_level(), RiskLevel::Critical);
    }

    #[test]
    fn test_permission_set() {
        let mut set = PermissionSet::new();
        set.insert(Permission::ReadProfile);
        set.insert(Permission::SendTokens);

        assert!(set.contains(&Permission::ReadProfile));
        assert!(!set.contains(&Permission::Camera));
        assert!(set.has_critical());
    }

    #[test]
    fn test_permission_grant() {
        let app_id = AppId([1u8; 32]);
        let user_id = [2u8; 32];

        let mut grant = PermissionGrant::new(
            app_id,
            user_id,
            Permission::ReadProfile,
            PermissionScope::unrestricted(),
            None,
        );

        assert!(grant.is_valid());
        assert!(grant.record_use().is_ok());
        assert_eq!(grant.use_count, 1);
    }

    #[test]
    fn test_permission_grant_expiry() {
        let app_id = AppId([1u8; 32]);
        let user_id = [2u8; 32];

        let grant = PermissionGrant::new(
            app_id,
            user_id,
            Permission::ReadProfile,
            PermissionScope::unrestricted(),
            Some(Duration::seconds(-1)), // Already expired
        );

        assert!(!grant.is_valid());
    }

    #[test]
    fn test_permission_manager() {
        let mut manager = PermissionManager::new();
        let app_id = AppId([1u8; 32]);
        let user_id = [2u8; 32];

        // Grant permission
        manager
            .grant(
                app_id,
                user_id,
                Permission::ReadProfile,
                PermissionScope::unrestricted(),
                None,
            )
            .unwrap();

        // Check granted
        assert!(manager.check(&app_id, &Permission::ReadProfile));
        assert!(!manager.check(&app_id, &Permission::Camera));

        // Use permission
        manager
            .use_permission(&app_id, &Permission::ReadProfile)
            .unwrap();

        // Revoke
        manager.revoke(&app_id, &Permission::ReadProfile).unwrap();
        assert!(!manager.check(&app_id, &Permission::ReadProfile));
    }

    #[test]
    fn test_permission_request_validation() {
        let app_id = AppId([1u8; 32]);

        // Valid request
        let request =
            PermissionRequest::new(app_id, vec![Permission::ReadProfile, Permission::Camera]);
        assert!(request.validate().is_ok());

        // Empty request
        let empty = PermissionRequest::new(app_id, vec![]);
        assert!(empty.validate().is_err());

        // Duplicate permissions
        let dupes = PermissionRequest::new(
            app_id,
            vec![Permission::ReadProfile, Permission::ReadProfile],
        );
        assert!(dupes.validate().is_err());
    }

    #[test]
    fn test_scope_with_limit() {
        let scope = PermissionScope::with_limit(1000);
        assert_eq!(scope.max_amount, Some(1000));
    }
}
