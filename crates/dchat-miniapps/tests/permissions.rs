//! Permission system tests for dchat-miniapps
//!
//! Verifies:
//! - Permission types and variants
//! - PermissionSet operations
//! - Risk level calculations
//! - Permission scopes and grants

use dchat_miniapps::permissions::{Permission, PermissionScope, PermissionSet, RiskLevel};

#[test]
fn test_permission_basic_types() {
    // Test basic permission creation
    let read_profile = Permission::ReadProfile;
    let send_messages = Permission::SendMessages;
    let read_contacts = Permission::ReadContacts;

    // They should be distinct
    assert_ne!(read_profile, send_messages);
    assert_ne!(send_messages, read_contacts);
}

#[test]
fn test_permission_wallet_variants() {
    let view_balance = Permission::ViewBalance;
    let request_payment = Permission::RequestPayment;
    let send_tokens = Permission::SendTokens;
    let wallet_address = Permission::WalletAddress;

    // All distinct
    assert_ne!(view_balance, request_payment);
    assert_ne!(request_payment, send_tokens);
    assert_ne!(send_tokens, wallet_address);
}

#[test]
fn test_permission_messaging_variants() {
    let send_messages = Permission::SendMessages;
    let send_attachments = Permission::SendAttachments;
    let access_reactions = Permission::AccessReactions;

    assert_ne!(send_messages, send_attachments);
    assert_ne!(send_attachments, access_reactions);
}

#[test]
fn test_permission_device_variants() {
    let camera = Permission::Camera;
    let microphone = Permission::Microphone;
    let location = Permission::Location;
    let clipboard = Permission::Clipboard;
    let notifications = Permission::Notifications;
    let biometrics = Permission::Biometrics;

    // All should be distinct
    let perms = [
        camera,
        microphone,
        location,
        clipboard,
        notifications,
        biometrics,
    ];
    for i in 0..perms.len() {
        for j in (i + 1)..perms.len() {
            assert_ne!(perms[i], perms[j]);
        }
    }
}

#[test]
fn test_permission_set_creation() {
    let set = PermissionSet::new();

    assert!(set.is_empty());
    assert_eq!(set.len(), 0);
    assert_eq!(set.count(), 0);
}

#[test]
fn test_permission_set_add_has() {
    let mut set = PermissionSet::new();

    set.add(Permission::ReadProfile);
    assert!(set.has(&Permission::ReadProfile));
    assert!(set.contains(&Permission::ReadProfile));
    assert_eq!(set.count(), 1);

    set.add(Permission::SendMessages);
    assert!(set.has(&Permission::SendMessages));
    assert_eq!(set.count(), 2);
}

#[test]
fn test_permission_set_remove() {
    let mut set = PermissionSet::new();

    set.add(Permission::ReadProfile);
    set.add(Permission::SendMessages);
    assert_eq!(set.count(), 2);

    set.remove(&Permission::ReadProfile);
    assert!(!set.has(&Permission::ReadProfile));
    assert!(set.has(&Permission::SendMessages));
    assert_eq!(set.count(), 1);
}

#[test]
fn test_permission_set_clear() {
    let mut set = PermissionSet::new();

    set.add(Permission::ReadProfile);
    set.add(Permission::SendMessages);
    set.add(Permission::ReadContacts);
    assert_eq!(set.count(), 3);

    set.clear();
    assert!(set.is_empty());
    assert_eq!(set.count(), 0);
}

#[test]
fn test_permission_set_no_duplicates() {
    let mut set = PermissionSet::new();

    set.add(Permission::ReadProfile);
    set.add(Permission::ReadProfile);
    set.add(Permission::ReadProfile);

    // Should only count once
    assert_eq!(set.count(), 1);
}

#[test]
fn test_permission_set_from_iter() {
    let permissions = vec![
        Permission::ReadProfile,
        Permission::SendMessages,
        Permission::ViewBalance,
    ];

    let set = PermissionSet::from_iter(permissions);

    assert_eq!(set.count(), 3);
    assert!(set.has(&Permission::ReadProfile));
    assert!(set.has(&Permission::SendMessages));
    assert!(set.has(&Permission::ViewBalance));
}

#[test]
fn test_permission_set_insert() {
    let mut set = PermissionSet::new();

    set.insert(Permission::Camera);
    set.insert(Permission::Microphone);

    assert!(set.contains(&Permission::Camera));
    assert!(set.contains(&Permission::Microphone));
    assert_eq!(set.len(), 2);
}

#[test]
fn test_permission_risk_level_low() {
    let low_risk_perms = [
        Permission::LocalStorage,
        Permission::Vibrate,
        Permission::Share,
        Permission::Notifications,
        Permission::KeepAwake,
        Permission::AccessReactions,
    ];

    for perm in low_risk_perms {
        assert_eq!(
            perm.risk_level(),
            RiskLevel::Low,
            "{:?} should be Low risk",
            perm
        );
    }
}

#[test]
fn test_permission_risk_level_medium() {
    let medium_risk_perms = [
        Permission::ReadProfile,
        Permission::ReadPublicKey,
        Permission::WalletAddress,
        Permission::ViewBalance,
        Permission::Network,
        Permission::WebSocket,
        Permission::Clipboard,
        Permission::Background,
        Permission::BotActions,
        Permission::InlineQueries,
    ];

    for perm in medium_risk_perms {
        assert_eq!(
            perm.risk_level(),
            RiskLevel::Medium,
            "{:?} should be Medium risk",
            perm
        );
    }
}

#[test]
fn test_permission_risk_level_high() {
    let high_risk_perms = [
        Permission::ReadContacts,
        Permission::ReadChatHistory,
        Permission::SendMessages,
        Permission::SendAttachments,
        Permission::RequestPayment,
        Permission::Camera,
        Permission::Microphone,
        Permission::Location,
        Permission::CloudStorage,
        Permission::FileSystem,
        Permission::Biometrics,
    ];

    for perm in high_risk_perms {
        assert_eq!(
            perm.risk_level(),
            RiskLevel::High,
            "{:?} should be High risk",
            perm
        );
    }
}

#[test]
fn test_permission_risk_level_critical() {
    let critical_risk_perms = [Permission::SendTokens, Permission::CrossChain];

    for perm in critical_risk_perms {
        assert_eq!(
            perm.risk_level(),
            RiskLevel::Critical,
            "{:?} should be Critical risk",
            perm
        );
    }
}

#[test]
fn test_risk_level_ordering() {
    assert!(RiskLevel::Low < RiskLevel::Medium);
    assert!(RiskLevel::Medium < RiskLevel::High);
    assert!(RiskLevel::High < RiskLevel::Critical);
}

#[test]
fn test_permission_display_name() {
    assert_eq!(Permission::ReadProfile.display_name(), "Read Profile");
    assert_eq!(Permission::SendMessages.display_name(), "Send Messages");
    assert_eq!(Permission::ViewBalance.display_name(), "View Balance");
    assert_eq!(Permission::SendTokens.display_name(), "Send Tokens");
    assert_eq!(Permission::CrossChain.display_name(), "Cross-Chain");
}

#[test]
fn test_permission_description() {
    let desc = Permission::ReadProfile.description();
    assert!(!desc.is_empty());

    let desc = Permission::SendTokens.description();
    assert!(!desc.is_empty());
}

#[test]
fn test_permission_requires_per_use_approval() {
    // These should require per-use approval
    assert!(Permission::SendTokens.requires_per_use_approval());
    assert!(Permission::CrossChain.requires_per_use_approval());
    assert!(Permission::Biometrics.requires_per_use_approval());

    // These should NOT require per-use approval
    assert!(!Permission::ReadProfile.requires_per_use_approval());
    assert!(!Permission::SendMessages.requires_per_use_approval());
    assert!(!Permission::ViewBalance.requires_per_use_approval());
}

#[test]
fn test_permission_set_sorted_by_risk() {
    let mut set = PermissionSet::new();
    set.add(Permission::LocalStorage); // Low
    set.add(Permission::SendTokens); // Critical
    set.add(Permission::ReadProfile); // Medium
    set.add(Permission::Camera); // High

    let sorted = set.sorted_by_risk();

    // Should be sorted from highest to lowest risk
    assert!(sorted.len() == 4);
    assert_eq!(sorted[0].risk_level(), RiskLevel::Critical);
    assert_eq!(sorted[sorted.len() - 1].risk_level(), RiskLevel::Low);
}

#[test]
fn test_permission_set_max_risk_level() {
    let mut set = PermissionSet::new();

    // Empty set has no max risk
    assert!(set.max_risk_level().is_none());

    set.add(Permission::LocalStorage); // Low
    assert_eq!(set.max_risk_level(), Some(RiskLevel::Low));

    set.add(Permission::ReadProfile); // Medium
    assert_eq!(set.max_risk_level(), Some(RiskLevel::Medium));

    set.add(Permission::Camera); // High
    assert_eq!(set.max_risk_level(), Some(RiskLevel::High));

    set.add(Permission::SendTokens); // Critical
    assert_eq!(set.max_risk_level(), Some(RiskLevel::Critical));
}

#[test]
fn test_permission_set_has_critical() {
    let mut set = PermissionSet::new();

    set.add(Permission::ReadProfile);
    set.add(Permission::Camera);
    assert!(!set.has_critical());

    set.add(Permission::SendTokens);
    assert!(set.has_critical());
}

#[test]
fn test_permission_set_iteration() {
    let mut set = PermissionSet::new();
    set.add(Permission::ReadProfile);
    set.add(Permission::SendMessages);
    set.add(Permission::ViewBalance);

    let collected: Vec<_> = set.iter().collect();
    assert_eq!(collected.len(), 3);
}

#[test]
fn test_permission_scope_unrestricted() {
    let scope = PermissionScope::unrestricted();

    assert!(scope.channels.is_none());
    assert!(scope.max_amount.is_none());
    assert!(scope.max_uses.is_none());
    assert!(scope.time_window.is_none());
    assert!(scope.custom.is_none());
}

#[test]
fn test_permission_scope_channels() {
    let channels = vec!["channel1".to_string(), "channel2".to_string()];
    let scope = PermissionScope::channels(channels.clone());

    assert_eq!(scope.channels, Some(channels));
}

#[test]
fn test_permission_scope_with_limit() {
    let scope = PermissionScope::with_limit(1000);

    assert_eq!(scope.max_amount, Some(1000));
}

#[test]
fn test_risk_level_color() {
    assert_eq!(RiskLevel::Low.color(), "#34C759");
    assert_eq!(RiskLevel::Medium.color(), "#FF9500");
    assert_eq!(RiskLevel::High.color(), "#FF3B30");
    assert_eq!(RiskLevel::Critical.color(), "#AF52DE");
}

#[test]
fn test_permission_serialization() {
    let perm = Permission::SendTokens;
    let json = serde_json::to_string(&perm).expect("serialize");

    let deserialized: Permission = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(perm, deserialized);
}

#[test]
fn test_risk_level_serialization() {
    let levels = [
        RiskLevel::Low,
        RiskLevel::Medium,
        RiskLevel::High,
        RiskLevel::Critical,
    ];

    for level in levels {
        let json = serde_json::to_string(&level).expect("serialize");
        let deserialized: RiskLevel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(level, deserialized);
    }
}

#[test]
fn test_all_permissions_have_display_name() {
    let all_perms = [
        Permission::ReadProfile,
        Permission::ReadContacts,
        Permission::ReadChatHistory,
        Permission::ReadPublicKey,
        Permission::ViewBalance,
        Permission::RequestPayment,
        Permission::SendTokens,
        Permission::WalletAddress,
        Permission::SendMessages,
        Permission::SendAttachments,
        Permission::AccessReactions,
        Permission::Camera,
        Permission::Microphone,
        Permission::Location,
        Permission::Clipboard,
        Permission::Notifications,
        Permission::Biometrics,
        Permission::LocalStorage,
        Permission::CloudStorage,
        Permission::FileSystem,
        Permission::Network,
        Permission::WebSocket,
        Permission::Background,
        Permission::KeepAwake,
        Permission::Vibrate,
        Permission::Share,
        Permission::CrossChain,
        Permission::BotActions,
        Permission::InlineQueries,
    ];

    for perm in all_perms {
        let name = perm.display_name();
        assert!(!name.is_empty(), "{:?} should have a display name", perm);

        let desc = perm.description();
        assert!(!desc.is_empty(), "{:?} should have a description", perm);
    }
}
