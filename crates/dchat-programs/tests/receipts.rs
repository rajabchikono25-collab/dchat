//! Receipt verification tests
//!
//! Verifies:
//! - Receipt hash computation is deterministic
//! - Receipt verification works correctly
//! - Events are properly recorded
//! - Account deltas are correct

use dchat_programs::account::Pubkey;
use dchat_programs::error::ProgramError;
use dchat_programs::events::{
    AccountDelta, EventId, ExecutionReceipt, LogEntry, ProgramEvent, ReturnData,
};

fn pubkey_n(n: u8) -> Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    Pubkey::new(bytes)
}

fn tx_hash_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

#[test]
fn test_event_id_generation() {
    let tx_hash = tx_hash_n(1);
    let program_id = pubkey_n(10);

    let id1 = EventId::generate(&tx_hash, &program_id, 0);
    let id2 = EventId::generate(&tx_hash, &program_id, 0);

    // Same inputs = same ID
    assert_eq!(id1.0, id2.0);

    // Different index = different ID
    let id3 = EventId::generate(&tx_hash, &program_id, 1);
    assert_ne!(id1.0, id3.0);
}

#[test]
fn test_program_event_creation() {
    let tx_hash = tx_hash_n(1);
    let program_id = pubkey_n(10);
    let discriminator = ProgramEvent::compute_discriminator("Transfer");
    let data = vec![1, 2, 3, 4];

    let event = ProgramEvent::new(&tx_hash, program_id, discriminator, data.clone(), 100, 0, 0);

    assert_eq!(event.program_id, program_id);
    assert_eq!(event.discriminator, discriminator);
    assert_eq!(event.data, data);
    assert_eq!(event.slot, 100);
    assert_eq!(event.index, 0);
    assert!(!event.is_cpi);
    assert_eq!(event.cpi_depth, 0);
}

#[test]
fn test_program_event_cpi() {
    let tx_hash = tx_hash_n(1);
    let program_id = pubkey_n(10);
    let discriminator = ProgramEvent::compute_discriminator("InnerCall");

    let event = ProgramEvent::new(&tx_hash, program_id, discriminator, vec![], 100, 1, 2);

    assert!(event.is_cpi);
    assert_eq!(event.cpi_depth, 2);
}

#[test]
fn test_event_discriminator_uniqueness() {
    let d1 = ProgramEvent::compute_discriminator("Transfer");
    let d2 = ProgramEvent::compute_discriminator("Mint");
    let d3 = ProgramEvent::compute_discriminator("Burn");

    assert_ne!(d1, d2);
    assert_ne!(d2, d3);
    assert_ne!(d1, d3);
}

#[test]
fn test_event_discriminator_determinism() {
    for _ in 0..10 {
        let d = ProgramEvent::compute_discriminator("Transfer");
        let expected = ProgramEvent::compute_discriminator("Transfer");
        assert_eq!(d, expected);
    }
}

#[test]
fn test_log_entry_creation() {
    let program_id = pubkey_n(10);

    let log = LogEntry {
        message: "test message".to_string(),
        program_id,
        depth: 0,
    };

    assert_eq!(log.message, "test message");
    assert_eq!(log.program_id, program_id);
    assert_eq!(log.depth, 0);
}

#[test]
fn test_account_delta_compute() {
    let pubkey = pubkey_n(1);
    let prev_owner = pubkey_n(10);
    let new_owner = pubkey_n(20);
    let prev_data = vec![0, 1, 2];
    let new_data = vec![0, 1, 2, 3, 4];

    let delta = AccountDelta::compute(
        pubkey, prev_owner, new_owner, 1000, 2000, &prev_data, &new_data,
    );

    assert_eq!(delta.pubkey, pubkey);
    assert_eq!(delta.prev_owner, prev_owner);
    assert_eq!(delta.new_owner, new_owner);
    assert_eq!(delta.prev_motes, 1000);
    assert_eq!(delta.new_motes, 2000);
    assert!(delta.reallocated); // size changed
    assert_eq!(delta.prev_size, 3);
    assert_eq!(delta.new_size, 5);
    assert!(delta.is_modified());
}

#[test]
fn test_account_delta_no_modification() {
    let pubkey = pubkey_n(1);
    let owner = pubkey_n(10);
    let data = vec![0, 1, 2];

    let delta = AccountDelta::compute(pubkey, owner, owner, 1000, 1000, &data, &data);

    assert!(!delta.is_modified());
    assert!(!delta.reallocated);
}

#[test]
fn test_execution_receipt_success() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;
    let events = vec![];
    let logs = vec![];
    let deltas = vec![];

    let receipt = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        events,
        logs,
        deltas,
        None,
        vec![],
    );

    assert!(receipt.success);
    assert!(receipt.error_code.is_none());
    assert!(receipt.error_message.is_none());
    assert_eq!(receipt.compute_units_consumed, 50000);
    assert_eq!(receipt.fee_paid, 5000);
    assert_eq!(receipt.slot, slot);
}

#[test]
fn test_execution_receipt_failure() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;
    let error = ProgramError::InsufficientFunds;
    let logs = vec![];

    let receipt = ExecutionReceipt::failure(tx_hash, slot, &error, 25000, 2500, logs);

    assert!(!receipt.success);
    assert!(receipt.error_code.is_some());
    assert!(receipt.error_message.is_some());
    assert_eq!(receipt.compute_units_consumed, 25000);
    assert_eq!(receipt.fee_paid, 2500);
}

#[test]
fn test_receipt_hash_determinism() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;

    let receipt1 = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        vec![],
        vec![],
        vec![],
        None,
        vec![],
    );

    let receipt2 = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        vec![],
        vec![],
        vec![],
        None,
        vec![],
    );

    assert_eq!(receipt1.receipt_hash, receipt2.receipt_hash);
}

#[test]
fn test_receipt_hash_changes_on_data_change() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;

    let receipt1 = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        vec![],
        vec![],
        vec![],
        None,
        vec![],
    );

    let receipt2 = ExecutionReceipt::success(
        tx_hash,
        slot,
        60000,
        5000, // different CU
        vec![],
        vec![],
        vec![],
        None,
        vec![],
    );

    assert_ne!(receipt1.receipt_hash, receipt2.receipt_hash);
}

#[test]
fn test_receipt_verification() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;

    let receipt = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        vec![],
        vec![],
        vec![],
        None,
        vec![],
    );

    assert!(receipt.verify());
}

#[test]
fn test_receipt_verification_tampered() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;

    let mut receipt = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        vec![],
        vec![],
        vec![],
        None,
        vec![],
    );

    // Tamper with data
    receipt.compute_units_consumed = 99999;

    // Should fail verification
    assert!(!receipt.verify());
}

#[test]
fn test_receipt_serialization_roundtrip() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;
    let program_id = pubkey_n(10);
    let discriminator = ProgramEvent::compute_discriminator("Test");

    let events = vec![ProgramEvent::new(
        &tx_hash,
        program_id,
        discriminator,
        vec![1, 2, 3],
        slot,
        0,
        0,
    )];

    let logs = vec![LogEntry {
        message: "test log".to_string(),
        program_id,
        depth: 0,
    }];

    let receipt = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        events,
        logs,
        vec![],
        None,
        vec![program_id],
    );

    // Serialize
    let bytes = receipt.to_bytes();
    assert!(!bytes.is_empty());

    // Deserialize
    let restored = ExecutionReceipt::from_bytes(&bytes);
    assert!(restored.is_some());

    let restored = restored.unwrap();
    assert_eq!(restored.transaction_hash, receipt.transaction_hash);
    assert_eq!(restored.slot, receipt.slot);
    assert_eq!(restored.success, receipt.success);
    assert_eq!(restored.receipt_hash, receipt.receipt_hash);
}

#[test]
fn test_return_data_max_size() {
    assert!(ReturnData::MAX_SIZE > 0);
    assert!(ReturnData::MAX_SIZE <= 1024); // Reasonable limit
}

#[test]
fn test_return_data_in_receipt() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;
    let program_id = pubkey_n(10);

    let return_data = ReturnData {
        program_id,
        data: vec![1, 2, 3, 4, 5],
    };

    let receipt = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        vec![],
        vec![],
        vec![],
        Some(return_data.clone()),
        vec![program_id],
    );

    assert!(receipt.return_data.is_some());
    let rd = receipt.return_data.unwrap();
    assert_eq!(rd.program_id, program_id);
    assert_eq!(rd.data, vec![1, 2, 3, 4, 5]);
}

#[test]
fn test_event_size_calculation() {
    let tx_hash = tx_hash_n(1);
    let program_id = pubkey_n(10);
    let discriminator = [0u8; 8];
    let data = vec![0u8; 100];

    let event = ProgramEvent::new(&tx_hash, program_id, discriminator, data, 100, 0, 0);

    let size = event.size();
    assert!(size > 100); // At least the data size
    assert!(size < 200); // But not too much overhead
}

#[test]
fn test_account_delta_data_hash() {
    let pubkey = pubkey_n(1);
    let owner = pubkey_n(10);
    let data1 = vec![0, 1, 2];
    let data2 = vec![0, 1, 3]; // Different

    let delta = AccountDelta::compute(pubkey, owner, owner, 1000, 1000, &data1, &data2);

    // Data hashes should differ
    assert_ne!(delta.prev_data_hash, delta.new_data_hash);
    assert!(delta.is_modified());
}

#[test]
fn test_receipt_with_programs_invoked() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;
    let programs = vec![pubkey_n(10), pubkey_n(20), pubkey_n(30)];

    let receipt = ExecutionReceipt::success(
        tx_hash,
        slot,
        50000,
        5000,
        vec![],
        vec![],
        vec![],
        None,
        programs.clone(),
    );

    assert_eq!(receipt.programs_invoked.len(), 3);
    assert!(receipt.programs_invoked.contains(&pubkey_n(10)));
    assert!(receipt.programs_invoked.contains(&pubkey_n(20)));
    assert!(receipt.programs_invoked.contains(&pubkey_n(30)));
}

/// Property-based receipt tests
#[cfg(feature = "proptest")]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_receipt_hash_deterministic(
            slot in 0u64..1000000,
            cu in 0u64..1400000,
            fee in 0u64..1000000
        ) {
            let tx_hash = tx_hash_n(1);

            let r1 = ExecutionReceipt::success(tx_hash, slot, cu, fee, vec![], vec![], vec![], None, vec![]);
            let r2 = ExecutionReceipt::success(tx_hash, slot, cu, fee, vec![], vec![], vec![], None, vec![]);

            prop_assert_eq!(r1.receipt_hash, r2.receipt_hash);
        }

        #[test]
        fn prop_receipt_verifies(
            slot in 0u64..1000000,
            cu in 0u64..1400000
        ) {
            let tx_hash = tx_hash_n(1);
            let receipt = ExecutionReceipt::success(tx_hash, slot, cu, 100, vec![], vec![], vec![], None, vec![]);
            prop_assert!(receipt.verify());
        }
    }
}
