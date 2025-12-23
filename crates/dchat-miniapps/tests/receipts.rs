//! Receipt tests for dchat-miniapps
//!
//! Tests the receipt system for tracking intent execution results.

use dchat_miniapps::intent::IntentId;
use dchat_miniapps::receipt::{
    AccountChange, Attestation, ExecutionResult, Receipt, ReceiptId, ReceiptStatus,
};
use ed25519_dalek::{Signer, SigningKey};

fn signing_key_n(n: u8) -> SigningKey {
    let mut seed = [0u8; 32];
    seed[0] = n;
    SigningKey::from_bytes(&seed)
}

fn tx_hash_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

fn block_hash_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = n;
    bytes
}

// ===================== Receipt ID Tests =====================

#[test]
fn test_receipt_id_creation() {
    let id = ReceiptId::new();
    let id2 = ReceiptId::new();

    // IDs should be unique
    assert_ne!(id.0, id2.0);
}

#[test]
fn test_receipt_id_display() {
    let id = ReceiptId::new();
    let display = format!("{}", id);

    // Should display as UUID format
    assert!(!display.is_empty());
    assert!(display.contains("-")); // UUID format contains dashes
}

#[test]
fn test_receipt_id_parse() {
    let id = ReceiptId::new();
    let display = id.0.to_string();

    let parsed = ReceiptId::parse(&display).expect("valid UUID");
    assert_eq!(id.0, parsed.0);
}

#[test]
fn test_receipt_id_parse_invalid() {
    let result = ReceiptId::parse("not-a-valid-uuid");
    assert!(result.is_err());
}

// ===================== Receipt Status Tests =====================

#[test]
fn test_receipt_status_variants() {
    let pending = ReceiptStatus::Pending;
    let confirmed = ReceiptStatus::Confirmed;
    let rejected = ReceiptStatus::Rejected;
    let finalized = ReceiptStatus::Finalized;

    assert_ne!(pending, confirmed);
    assert_ne!(confirmed, rejected);
    assert_ne!(rejected, finalized);
}

// ===================== Execution Result Tests =====================

#[test]
fn test_execution_result_success() {
    let result = ExecutionResult {
        success: true,
        return_data: Some(vec![1, 2, 3, 4]),
        error: None,
        logs: vec!["Transfer executed".to_string()],
        account_changes: vec![],
        compute_units: 1000,
        fee: 100,
    };

    assert!(result.success);
    assert!(result.error.is_none());
    assert_eq!(result.compute_units, 1000);
}

#[test]
fn test_execution_result_failure() {
    let result = ExecutionResult {
        success: false,
        return_data: None,
        error: Some("Insufficient balance".to_string()),
        logs: vec!["Error: balance check failed".to_string()],
        account_changes: vec![],
        compute_units: 500,
        fee: 50, // Still charged some fee
    };

    assert!(!result.success);
    assert!(result.error.is_some());
    assert!(result.error.unwrap().contains("Insufficient"));
}

#[test]
fn test_account_change() {
    let change = AccountChange {
        address: [1u8; 32],
        prev_motes: 1_000_000,
        new_motes: 900_000,
        data_changed: false,
        owner_changed: false,
    };

    assert_eq!(change.prev_motes - change.new_motes, 100_000);
    assert!(!change.data_changed);
}

// ===================== Receipt Tests =====================

#[test]
fn test_receipt_creation() {
    let intent_id = IntentId::new();
    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec![],
        account_changes: vec![],
        compute_units: 1000,
        fee: 100,
    };

    let receipt = Receipt::new(
        intent_id,
        result,
        12345, // execution slot
        tx_hash_n(1),
        block_hash_n(1),
    );

    assert_eq!(receipt.intent_id, intent_id);
    assert_eq!(receipt.status, ReceiptStatus::Pending);
    assert_eq!(receipt.execution_slot, 12345);
    assert!(receipt.finalized_at.is_none());
}

#[test]
fn test_receipt_hash_deterministic() {
    let intent_id = IntentId::new();
    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec![],
        account_changes: vec![],
        compute_units: 1000,
        fee: 100,
    };

    let receipt = Receipt::new(
        intent_id,
        result.clone(),
        12345,
        tx_hash_n(1),
        block_hash_n(1),
    );

    let hash1 = receipt.hash();
    let hash2 = receipt.hash();

    assert_eq!(hash1, hash2);
}

#[test]
fn test_receipt_status_transitions() {
    let intent_id = IntentId::new();
    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec![],
        account_changes: vec![],
        compute_units: 1000,
        fee: 100,
    };

    let mut receipt = Receipt::new(intent_id, result, 12345, tx_hash_n(1), block_hash_n(1));

    // Initially pending
    assert_eq!(receipt.status, ReceiptStatus::Pending);

    // Confirm
    receipt.confirm();
    assert_eq!(receipt.status, ReceiptStatus::Confirmed);

    // Finalize
    receipt.finalize();
    assert_eq!(receipt.status, ReceiptStatus::Finalized);
    assert!(receipt.finalized_at.is_some());
}

#[test]
fn test_receipt_rejection() {
    let intent_id = IntentId::new();
    let result = ExecutionResult {
        success: false,
        return_data: None,
        error: Some("Transaction reverted".to_string()),
        logs: vec![],
        account_changes: vec![],
        compute_units: 100,
        fee: 10,
    };

    let mut receipt = Receipt::new(intent_id, result, 12345, tx_hash_n(1), block_hash_n(1));

    receipt.reject();
    assert_eq!(receipt.status, ReceiptStatus::Rejected);
}

// ===================== Attestation Tests =====================

#[test]
fn test_attestation_creation() {
    let signing_key = signing_key_n(1);
    let receipt_hash = [42u8; 32];
    let stake = 1000;

    let attestation = Attestation::create(&signing_key, receipt_hash, stake);

    assert_eq!(attestation.receipt_hash, receipt_hash);
    assert_eq!(attestation.stake, stake);
    assert_eq!(attestation.signer, signing_key.verifying_key().to_bytes());
}

#[test]
fn test_attestation_verification() {
    let signing_key = signing_key_n(1);
    let receipt_hash = [42u8; 32];

    let attestation = Attestation::create(&signing_key, receipt_hash, 1000);

    // Valid verification
    assert!(attestation.verify().is_ok());
}

#[test]
fn test_attestation_different_signers() {
    let key1 = signing_key_n(1);
    let key2 = signing_key_n(2);
    let receipt_hash = [42u8; 32];

    let att1 = Attestation::create(&key1, receipt_hash, 1000);
    let att2 = Attestation::create(&key2, receipt_hash, 2000);

    // Different signers produce different signatures
    assert_ne!(att1.signer, att2.signer);
    assert_ne!(att1.signature, att2.signature);

    // Both should verify successfully
    assert!(att1.verify().is_ok());
    assert!(att2.verify().is_ok());
}

// ===================== Integration Tests =====================

#[test]
fn test_receipt_with_account_changes() {
    let intent_id = IntentId::new();

    let sender_change = AccountChange {
        address: [1u8; 32],
        prev_motes: 1_000_000,
        new_motes: 899_900, // Sent 100,000 + 100 fee
        data_changed: false,
        owner_changed: false,
    };

    let recipient_change = AccountChange {
        address: [2u8; 32],
        prev_motes: 0,
        new_motes: 100_000,
        data_changed: false,
        owner_changed: false,
    };

    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec![
            "Transfer initiated".to_string(),
            "Balance check passed".to_string(),
            "Transfer completed".to_string(),
        ],
        account_changes: vec![sender_change, recipient_change],
        compute_units: 5000,
        fee: 100,
    };

    let receipt = Receipt::new(intent_id, result, 12345, tx_hash_n(1), block_hash_n(1));

    assert_eq!(receipt.result.account_changes.len(), 2);
    assert_eq!(receipt.result.logs.len(), 3);
}

#[test]
fn test_multiple_attestations_on_receipt() {
    let intent_id = IntentId::new();
    let result = ExecutionResult {
        success: true,
        return_data: None,
        error: None,
        logs: vec![],
        account_changes: vec![],
        compute_units: 1000,
        fee: 100,
    };

    let receipt = Receipt::new(intent_id, result, 12345, tx_hash_n(1), block_hash_n(1));

    let receipt_hash = receipt.hash();

    // Multiple signers attest to the same receipt
    let attestations: Vec<_> = (0..5)
        .map(|i| {
            let key = signing_key_n(i as u8);
            Attestation::create(&key, receipt_hash, 1000 * (i + 1) as u64)
        })
        .collect();

    // All attestations should verify
    for att in &attestations {
        assert!(att.verify().is_ok());
        assert_eq!(att.receipt_hash, receipt_hash);
    }

    // Calculate total stake
    let total_stake: u64 = attestations.iter().map(|a| a.stake).sum();
    assert_eq!(total_stake, 1000 + 2000 + 3000 + 4000 + 5000);
}
