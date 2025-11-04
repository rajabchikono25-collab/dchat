//! Bot permission system

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Bot permission
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BotPermission {
    /// Send messages
    SendMessages,

    /// Edit messages
    EditMessages,

    /// Delete messages
    DeleteMessages,

    /// Manage chat members
    ManageMembers,

    /// Change chat info
    ChangeInfo,

    /// Pin messages
    PinMessages,

    /// Invite users
    InviteUsers,

    /// Restrict members
    RestrictMembers,

    /// Promote members
    PromoteMembers,

    /// Delete chat
    DeleteChat,

    /// Read message history
    ReadHistory,

    /// Access inline queries
    InlineQueries,

    /// Access callback queries
    CallbackQueries,

    /// Custom permission
    Custom(String),
}

/// Bot scope
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BotScope {
    /// All chats
    All,

    /// Private chats only
    Private,

    /// Group chats only
    Groups,

    /// Supergroups only
    Supergroups,

    /// Channels only
    Channels,

    /// Specific chat
    Chat(String),

    /// Specific user
    User(dchat_core::types::UserId),
}

/// Permission set for a bot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotPermissions {
    /// Set of granted permissions
    permissions: HashSet<BotPermission>,

    /// Scope where permissions apply
    scope: BotScope,

    /// Is bot admin?
    is_admin: bool,
}

/// Permission manager
pub struct PermissionManager {
    /// Default permissions for new bots
    default_permissions: HashSet<BotPermission>,
}

impl BotPermissions {
    /// Create new bot permissions
    pub fn new(scope: BotScope) -> Self {
        Self {
            permissions: HashSet::new(),
            scope,
            is_admin: false,
        }
    }

    /// Create default permissions for regular bot
    pub fn default_bot() -> Self {
        let mut permissions = HashSet::new();
        permissions.insert(BotPermission::SendMessages);
        permissions.insert(BotPermission::ReadHistory);
        permissions.insert(BotPermission::InlineQueries);
        permissions.insert(BotPermission::CallbackQueries);

        Self {
            permissions,
            scope: BotScope::All,
            is_admin: false,
        }
    }

    /// Create admin permissions
    pub fn admin() -> Self {
        let mut permissions = HashSet::new();
        permissions.insert(BotPermission::SendMessages);
        permissions.insert(BotPermission::EditMessages);
        permissions.insert(BotPermission::DeleteMessages);
        permissions.insert(BotPermission::ManageMembers);
        permissions.insert(BotPermission::ChangeInfo);
        permissions.insert(BotPermission::PinMessages);
        permissions.insert(BotPermission::InviteUsers);
        permissions.insert(BotPermission::ReadHistory);
        permissions.insert(BotPermission::InlineQueries);
        permissions.insert(BotPermission::CallbackQueries);

        Self {
            permissions,
            scope: BotScope::All,
            is_admin: true,
        }
    }

    /// Grant a permission
    pub fn grant(&mut self, permission: BotPermission) {
        self.permissions.insert(permission);
    }

    /// Revoke a permission
    pub fn revoke(&mut self, permission: &BotPermission) {
        self.permissions.remove(permission);
    }

    /// Check if bot has permission
    pub fn has_permission(&self, permission: &BotPermission) -> bool {
        self.is_admin || self.permissions.contains(permission)
    }

    /// Check if bot is admin
    pub fn is_admin(&self) -> bool {
        self.is_admin
    }

    /// Set admin status
    pub fn set_admin(&mut self, is_admin: bool) {
        self.is_admin = is_admin;
    }

    /// Get scope
    pub fn scope(&self) -> &BotScope {
        &self.scope
    }

    /// Set scope
    pub fn set_scope(&mut self, scope: BotScope) {
        self.scope = scope;
    }

    /// Get all permissions
    pub fn all_permissions(&self) -> Vec<&BotPermission> {
        self.permissions.iter().collect()
    }
}

impl PermissionManager {
    /// Create new permission manager
    pub fn new() -> Self {
        let mut default_permissions = HashSet::new();
        default_permissions.insert(BotPermission::SendMessages);
        default_permissions.insert(BotPermission::ReadHistory);
        default_permissions.insert(BotPermission::InlineQueries);
        default_permissions.insert(BotPermission::CallbackQueries);

        Self {
            default_permissions,
        }
    }

    /// Get default permissions
    pub fn default_permissions(&self) -> &HashSet<BotPermission> {
        &self.default_permissions
    }

    /// Validate permission request
    pub fn validate_permission(
        &self,
        bot_permissions: &BotPermissions,
        requested_permission: &BotPermission,
        scope: &BotScope,
    ) -> bool {
        // Check if bot has the permission
        if !bot_permissions.has_permission(requested_permission) {
            return false;
        }

        // Check if scope matches
        self.scope_matches(&bot_permissions.scope, scope)
    }

    /// Check if scopes match
    fn scope_matches(&self, bot_scope: &BotScope, requested_scope: &BotScope) -> bool {
        match (bot_scope, requested_scope) {
            (BotScope::All, _) => true,
            (BotScope::Private, BotScope::Private) => true,
            (BotScope::Groups, BotScope::Groups) => true,
            (BotScope::Supergroups, BotScope::Supergroups) => true,
            (BotScope::Channels, BotScope::Channels) => true,
            (BotScope::Chat(a), BotScope::Chat(b)) => a == b,
            (BotScope::User(a), BotScope::User(b)) => a == b,
            _ => false,
        }
    }
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for BotPermissions {
    fn default() -> Self {
        Self::default_bot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use dchat_core::types::UserId;

    #[test]
    fn test_default_bot_permissions() {
        let perms = BotPermissions::default_bot();

        assert!(perms.has_permission(&BotPermission::SendMessages));
        assert!(perms.has_permission(&BotPermission::ReadHistory));
        assert!(!perms.has_permission(&BotPermission::DeleteMessages));
        assert!(!perms.is_admin());
    }

    #[test]
    fn test_admin_permissions() {
        let perms = BotPermissions::admin();

        assert!(perms.has_permission(&BotPermission::SendMessages));
        assert!(perms.has_permission(&BotPermission::DeleteMessages));
        assert!(perms.has_permission(&BotPermission::ManageMembers));
        assert!(perms.is_admin());
    }

    #[test]
    fn test_grant_revoke_permissions() {
        let mut perms = BotPermissions::new(BotScope::All);

        assert!(!perms.has_permission(&BotPermission::SendMessages));

        perms.grant(BotPermission::SendMessages);
        assert!(perms.has_permission(&BotPermission::SendMessages));

        perms.revoke(&BotPermission::SendMessages);
        assert!(!perms.has_permission(&BotPermission::SendMessages));
    }

    #[test]
    fn test_admin_has_all_permissions() {
        let mut perms = BotPermissions::new(BotScope::All);
        perms.set_admin(true);

        // Admin should have all permissions even if not explicitly granted
        assert!(perms.has_permission(&BotPermission::SendMessages));
        assert!(perms.has_permission(&BotPermission::DeleteMessages));
        assert!(perms.has_permission(&BotPermission::ManageMembers));
    }

    #[test]
    fn test_scope_matching() {
        let manager = PermissionManager::new();

        let _bot_perms = BotPermissions::default_bot();

        // All scope matches everything
        assert!(manager.scope_matches(&BotScope::All, &BotScope::Private));
        assert!(manager.scope_matches(&BotScope::All, &BotScope::Groups));

        // Specific scopes only match themselves
        assert!(manager.scope_matches(&BotScope::Private, &BotScope::Private));
        assert!(!manager.scope_matches(&BotScope::Private, &BotScope::Groups));

        // Chat scopes must match exactly
        assert!(manager.scope_matches(
            &BotScope::Chat("chat123".to_string()),
            &BotScope::Chat("chat123".to_string())
        ));
        assert!(!manager.scope_matches(
            &BotScope::Chat("chat123".to_string()),
            &BotScope::Chat("chat456".to_string())
        ));
    }

    #[test]
    fn test_validate_permission() {
        let manager = PermissionManager::new();
        let mut bot_perms = BotPermissions::default_bot();

        // Bot has SendMessages permission
        assert!(manager.validate_permission(
            &bot_perms,
            &BotPermission::SendMessages,
            &BotScope::All
        ));

        // Bot doesn't have DeleteMessages permission
        assert!(!manager.validate_permission(
            &bot_perms,
            &BotPermission::DeleteMessages,
            &BotScope::All
        ));

        // Grant DeleteMessages and check again
        bot_perms.grant(BotPermission::DeleteMessages);
        assert!(manager.validate_permission(
            &bot_perms,
            &BotPermission::DeleteMessages,
            &BotScope::All
        ));
    }
}
