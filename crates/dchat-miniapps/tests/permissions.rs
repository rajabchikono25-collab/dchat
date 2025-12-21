//! Permission system tests for dchat-miniapps
//!
//! Verifies:
//! - Permission parsing and validation
//! - PermissionSet operations
//! - Risk level calculations
//! - Privacy budget tracking

use dchat_miniapps::permissions::{
    Permission, PermissionSet, PrivacyBudget, PrivacyOperation, RiskLevel,
};

#[test]
fn test_permission_basic_types() {
    // Test basic permission creation
    let read_messages = Permission::ReadMessages;
    let send_messages = Permission::SendMessages;
    let read_contacts = Permission::ReadContacts;

    // They should be distinct
    assert_ne!(read_messages, send_messages);
    assert_ne!(send_messages, read_contacts);
}

#[test]
fn test_permission_wallet_variants() {
    let view_balance = Permission::ViewWalletBalance;
    let request_payment = Permission::RequestPayment { max_amount: 1000 };
    let auto_pay = Permission::AutoPayment {
        max_per_tx: 100,
        max_daily: 500,
    };

    // All distinct
    assert_ne!(view_balance, request_payment);
    assert_ne!(request_payment, auto_pay);
}

#[test]
fn test_permission_set_creation() {
    let set = PermissionSet::new();

    assert!(set.is_empty());
    assert_eq!(set.count(), 0);
}

#[test]
fn test_permission_set_add_remove() {
    let mut set = PermissionSet::new();

    set.add(Permission::ReadMessages);
    assert!(set.has(&Permission::ReadMessages));
    assert_eq!(set.count(), 1);

    set.add(Permission::SendMessages);
    assert!(set.has(&Permission::SendMessages));
    assert_eq!(set.count(), 2);

    set.remove(&Permission::ReadMessages);
    assert!(!set.has(&Permission::ReadMessages));
    assert_eq!(set.count(), 1);
}

#[test]
fn test_permission_set_no_duplicates() {
    let mut set = PermissionSet::new();

    set.add(Permission::ReadMessages);
    set.add(Permission::ReadMessages);
    set.add(Permission::ReadMessages);

    // Should only count once
    assert_eq!(set.count(), 1);
}

#[test]
fn test_permission_set_union() {
    let mut set1 = PermissionSet::new();
    set1.add(Permission::ReadMessages);
    set1.add(Permission::SendMessages);

    let mut set2 = PermissionSet::new();
    set2.add(Permission::SendMessages);
    set2.add(Permission::ReadContacts);

    let union = set1.union(&set2);

    assert!(union.has(&Permission::ReadMessages));
    assert!(union.has(&Permission::SendMessages));
    assert!(union.has(&Permission::ReadContacts));
    assert_eq!(union.count(), 3);
}

#[test]
fn test_permission_set_intersection() {
    let mut set1 = PermissionSet::new();
    set1.add(Permission::ReadMessages);
    set1.add(Permission::SendMessages);

    let mut set2 = PermissionSet::new();
    set2.add(Permission::SendMessages);
    set2.add(Permission::ReadContacts);

    let intersection = set1.intersection(&set2);

    assert!(!intersection.has(&Permission::ReadMessages));
    assert!(intersection.has(&Permission::SendMessages));
    assert!(!intersection.has(&Permission::ReadContacts));
    assert_eq!(intersection.count(), 1);
}

#[test]
fn test_permission_set_is_subset() {
    let mut small = PermissionSet::new();
    small.add(Permission::ReadMessages);

    let mut large = PermissionSet::new();
    large.add(Permission::ReadMessages);
    large.add(Permission::SendMessages);

    assert!(small.is_subset_of(&large));
    assert!(!large.is_subset_of(&small));
}

#[test]
fn test_risk_level_ordering() {
    assert!(RiskLevel::Low < RiskLevel::Medium);
    assert!(RiskLevel::Medium < RiskLevel::High);
    assert!(RiskLevel::High < RiskLevel::Critical);
}

#[test]
fn test_permission_risk_level() {
    // Basic permissions should be low risk
    let read = Permission::ReadMessages;
    assert_eq!(read.risk_level(), RiskLevel::Low);

    // Wallet operations should be higher risk
    let view_balance = Permission::ViewWalletBalance;
    assert!(view_balance.risk_level() >= RiskLevel::Medium);

    // Auto-payment should be high risk
    let auto_pay = Permission::AutoPayment {
        max_per_tx: 100,
        max_daily: 500,
    };
    assert!(auto_pay.risk_level() >= RiskLevel::High);
}

#[test]
fn test_permission_set_max_risk_level() {
    let mut set = PermissionSet::new();
    set.add(Permission::ReadMessages); // Low
    assert_eq!(set.max_risk_level(), RiskLevel::Low);

    set.add(Permission::ViewWalletBalance); // Medium
    assert_eq!(set.max_risk_level(), RiskLevel::Medium);

    set.add(Permission::AutoPayment {
        max_per_tx: 100,
        max_daily: 500,
    }); // High
    assert_eq!(set.max_risk_level(), RiskLevel::High);
}

#[test]
fn test_privacy_budget_creation() {
    let budget = PrivacyBudget::new(100);

    assert_eq!(budget.remaining(), 100);
    assert_eq!(budget.used(), 0);
    assert!(!budget.is_exhausted());
}

#[test]
fn test_privacy_budget_consumption() {
    let mut budget = PrivacyBudget::new(100);

    // Use some budget
    assert!(budget.consume(30).is_ok());
    assert_eq!(budget.remaining(), 70);
    assert_eq!(budget.used(), 30);

    // Use more
    assert!(budget.consume(50).is_ok());
    assert_eq!(budget.remaining(), 20);

    // Cannot exceed
    assert!(budget.consume(30).is_err());
    assert_eq!(budget.remaining(), 20);
}

#[test]
fn test_privacy_budget_exhaustion() {
    let mut budget = PrivacyBudget::new(100);

    assert!(budget.consume(100).is_ok());
    assert!(budget.is_exhausted());
    assert_eq!(budget.remaining(), 0);

    // Any further consumption fails
    assert!(budget.consume(1).is_err());
}

#[test]
fn test_privacy_operation_cost() {
    let contact_read = PrivacyOperation::ContactRead;
    assert!(contact_read.cost() > 0);

    let message_read = PrivacyOperation::MessageRead;
    assert!(message_read.cost() > 0);

    let profile_access = PrivacyOperation::ProfileAccess;
    assert!(profile_access.cost() > 0);

    // Some operations might have different costs
    let location_access = PrivacyOperation::LocationAccess;
    assert!(location_access.cost() >= contact_read.cost());
}

#[test]
fn test_privacy_budget_operation_tracking() {
    let mut budget = PrivacyBudget::new(100);

    let op = PrivacyOperation::ContactRead;
    assert!(budget.use_for_operation(&op).is_ok());

    // Budget should decrease by operation cost
    assert!(budget.used() >= op.cost());
}

#[test]
fn test_permission_category() {
    assert_eq!(Permission::ReadMessages.category(), "messaging");
    assert_eq!(Permission::SendMessages.category(), "messaging");
    assert_eq!(Permission::ReadContacts.category(), "contacts");
    assert_eq!(Permission::ViewWalletBalance.category(), "wallet");
}

#[test]
fn test_permission_requires_confirmation() {
    // Low risk permissions don't need extra confirmation
    assert!(!Permission::ReadMessages.requires_confirmation());

    // High risk permissions need confirmation
    let auto_pay = Permission::AutoPayment {
        max_per_tx: 100,
        max_daily: 500,
    };
    assert!(auto_pay.requires_confirmation());
}

#[test]
fn test_permission_set_serialization() {
    let mut set = PermissionSet::new();
    set.add(Permission::ReadMessages);
    set.add(Permission::SendMessages);
    set.add(Permission::ReadContacts);

    // Serialize
    let bytes = set.to_bytes();
    assert!(!bytes.is_empty());

    // Deserialize
    let restored = PermissionSet::from_bytes(&bytes);
    assert!(restored.is_ok());

    let restored = restored.unwrap();
    assert!(restored.has(&Permission::ReadMessages));
    assert!(restored.has(&Permission::SendMessages));
    assert!(restored.has(&Permission::ReadContacts));
    assert_eq!(restored.count(), 3);
}

#[test]
fn test_permission_display() {
    let perm = Permission::ReadMessages;
    let display = format!("{}", perm);
    assert!(!display.is_empty());
    assert!(display.contains("message") || display.contains("Message"));
}

#[test]
fn test_permission_set_deny_by_default() {
    let set = PermissionSet::new();

    // Any permission check on empty set should fail
    assert!(!set.has(&Permission::ReadMessages));
    assert!(!set.has(&Permission::SendMessages));
    assert!(!set.has(&Permission::ViewWalletBalance));
}

#[test]
fn test_permission_with_constraints() {
    let payment = Permission::RequestPayment { max_amount: 1000 };

    // Extract constraints
    match payment {
        Permission::RequestPayment { max_amount } => {
            assert_eq!(max_amount, 1000);
        }
        _ => panic!("wrong permission type"),
    }
}

#[test]
fn test_auto_payment_limits() {
    let auto_pay = Permission::AutoPayment {
        max_per_tx: 100,
        max_daily: 500,
    };

    match auto_pay {
        Permission::AutoPayment {
            max_per_tx,
            max_daily,
        } => {
            assert_eq!(max_per_tx, 100);
            assert_eq!(max_daily, 500);
            // Daily limit should be >= per tx limit
            assert!(max_daily >= max_per_tx);
        }
        _ => panic!("wrong permission type"),
    }
}

#[test]
fn test_permission_data_access() {
    let storage_read = Permission::StorageRead {
        namespace: "user".to_string(),
    };
    let storage_write = Permission::StorageWrite {
        namespace: "user".to_string(),
    };

    // Different operations on same namespace are different permissions
    assert_ne!(storage_read, storage_write);

    // Same operation on different namespaces are different
    let other_read = Permission::StorageRead {
        namespace: "settings".to_string(),
    };
    assert_ne!(storage_read, other_read);
}

#[test]
fn test_permission_set_clear() {
    let mut set = PermissionSet::new();
    set.add(Permission::ReadMessages);
    set.add(Permission::SendMessages);
    set.add(Permission::ReadContacts);

    assert_eq!(set.count(), 3);

    set.clear();

    assert!(set.is_empty());
    assert_eq!(set.count(), 0);
}

/// Property-based permission tests
#[cfg(feature = "proptest")]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_privacy_budget_never_negative(initial in 0u64..10000, consume in 0u64..10000) {
            let mut budget = PrivacyBudget::new(initial);
            let _ = budget.consume(consume);
            prop_assert!(budget.remaining() >= 0);
            prop_assert!(budget.used() <= initial);
        }

        #[test]
        fn prop_permission_set_union_commutative(a: Vec<u8>, b: Vec<u8>) {
            let mut set1 = PermissionSet::new();
            let mut set2 = PermissionSet::new();

            // Add some permissions based on input
            if a.contains(&1) { set1.add(Permission::ReadMessages); }
            if a.contains(&2) { set1.add(Permission::SendMessages); }
            if b.contains(&1) { set2.add(Permission::ReadMessages); }
            if b.contains(&3) { set2.add(Permission::ReadContacts); }

            let union1 = set1.union(&set2);
            let union2 = set2.union(&set1);

            prop_assert_eq!(union1.count(), union2.count());
        }
    }
}
