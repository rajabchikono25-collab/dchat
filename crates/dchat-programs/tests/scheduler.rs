//! Parallel scheduler correctness tests
//!
//! Verifies:
//! - No data races between concurrent transactions
//! - Correct lock conflict detection
//! - Deterministic conflict resolution
//! - Proper lock acquisition and release

use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use dchat_programs::account::Pubkey;
use dchat_programs::error::ProgramError;
use dchat_programs::instruction::InstructionBatch;
use dchat_programs::scheduler::{
    AccountLock, AccountLockManager, ExecutionBatch, LockType, ParallelScheduler,
    ScheduledTransaction, SchedulerConfig,
};

fn random_pubkey() -> Pubkey {
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = (i * 7 + 13) as u8;
    }
    Pubkey::new(bytes)
}

fn pubkey_n(n: u8) -> Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    Pubkey::new(bytes)
}

#[test]
fn test_lock_manager_creation() {
    let manager = AccountLockManager::new(5000);
    let account = pubkey_n(1);

    assert!(!manager.is_locked(&account));
}

#[test]
fn test_read_lock_acquisition() {
    let manager = AccountLockManager::new(5000);
    let account = pubkey_n(1);

    // Acquire read lock
    let result = manager.try_acquire_locks(1, &[account], &[]);
    assert!(result.is_ok());

    let locks = result.unwrap();
    assert_eq!(locks.len(), 1);
    assert_eq!(locks[0].lock_type, LockType::Read);
    assert!(manager.is_locked(&account));

    // Release lock
    manager.release_locks(1, &locks);
    assert!(!manager.is_locked(&account));
}

#[test]
fn test_write_lock_acquisition() {
    let manager = AccountLockManager::new(5000);
    let account = pubkey_n(1);

    // Acquire write lock
    let result = manager.try_acquire_locks(1, &[], &[account]);
    assert!(result.is_ok());

    let locks = result.unwrap();
    assert_eq!(locks.len(), 1);
    assert_eq!(locks[0].lock_type, LockType::Write);
    assert!(manager.is_locked(&account));

    // Release lock
    manager.release_locks(1, &locks);
    assert!(!manager.is_locked(&account));
}

#[test]
fn test_multiple_readers_allowed() {
    let manager = AccountLockManager::new(5000);
    let account = pubkey_n(1);

    // First reader
    let result1 = manager.try_acquire_locks(1, &[account], &[]);
    assert!(result1.is_ok());

    // Second reader - should succeed
    let result2 = manager.try_acquire_locks(2, &[account], &[]);
    assert!(result2.is_ok());

    // Third reader - should succeed
    let result3 = manager.try_acquire_locks(3, &[account], &[]);
    assert!(result3.is_ok());

    // Release all
    manager.release_locks(1, &result1.unwrap());
    manager.release_locks(2, &result2.unwrap());
    manager.release_locks(3, &result3.unwrap());

    assert!(!manager.is_locked(&account));
}

#[test]
fn test_write_excludes_readers() {
    let manager = AccountLockManager::new(5000);
    let account = pubkey_n(1);

    // Acquire read lock first
    let read_lock = manager.try_acquire_locks(1, &[account], &[]).unwrap();

    // Try to acquire write lock - should fail
    let result = manager.try_acquire_locks(2, &[], &[account]);
    assert!(matches!(result, Err(ProgramError::AccountLocked)));

    // Release read lock
    manager.release_locks(1, &read_lock);

    // Now write should succeed
    let result = manager.try_acquire_locks(2, &[], &[account]);
    assert!(result.is_ok());
}

#[test]
fn test_write_excludes_writers() {
    let manager = AccountLockManager::new(5000);
    let account = pubkey_n(1);

    // Acquire write lock first
    let write_lock = manager.try_acquire_locks(1, &[], &[account]).unwrap();

    // Try to acquire another write lock - should fail
    let result = manager.try_acquire_locks(2, &[], &[account]);
    assert!(matches!(result, Err(ProgramError::AccountLocked)));

    // Release first write lock
    manager.release_locks(1, &write_lock);

    // Now second write should succeed
    let result = manager.try_acquire_locks(2, &[], &[account]);
    assert!(result.is_ok());
}

#[test]
fn test_readers_wait_for_writer() {
    let manager = AccountLockManager::new(5000);
    let account = pubkey_n(1);

    // Acquire write lock first
    let write_lock = manager.try_acquire_locks(1, &[], &[account]).unwrap();

    // Try to acquire read lock - should fail
    let result = manager.try_acquire_locks(2, &[account], &[]);
    assert!(matches!(result, Err(ProgramError::AccountLocked)));

    // Release write lock
    manager.release_locks(1, &write_lock);

    // Now read should succeed
    let result = manager.try_acquire_locks(2, &[account], &[]);
    assert!(result.is_ok());
}

#[test]
fn test_transaction_conflict_detection() {
    let account_a = pubkey_n(1);
    let account_b = pubkey_n(2);
    let account_c = pubkey_n(3);

    // Transaction 1: writes A, reads B
    let batch1 = InstructionBatch::default();
    let tx1 = ScheduledTransaction::new_with_accounts(
        1,
        batch1,
        vec![account_b], // read accounts
        vec![account_a], // write accounts
        1,
    );

    // Transaction 2: reads A, writes C
    let batch2 = InstructionBatch::default();
    let tx2 = ScheduledTransaction::new_with_accounts(
        2,
        batch2,
        vec![account_a], // read accounts
        vec![account_c], // write accounts
        1,
    );

    // Should conflict (tx1 writes A, tx2 reads A)
    assert!(tx1.conflicts_with(&tx2));
    assert!(tx2.conflicts_with(&tx1));
}

#[test]
fn test_no_conflict_independent_accounts() {
    let account_a = pubkey_n(1);
    let account_b = pubkey_n(2);
    let account_c = pubkey_n(3);
    let account_d = pubkey_n(4);

    // Transaction 1: writes A, B
    let batch1 = InstructionBatch::default();
    let tx1 = ScheduledTransaction::new_with_accounts(
        1,
        batch1,
        vec![],                     // read accounts
        vec![account_a, account_b], // write accounts
        1,
    );

    // Transaction 2: writes C, D
    let batch2 = InstructionBatch::default();
    let tx2 = ScheduledTransaction::new_with_accounts(
        2,
        batch2,
        vec![],                     // read accounts
        vec![account_c, account_d], // write accounts
        1,
    );

    // Should NOT conflict
    assert!(!tx1.conflicts_with(&tx2));
    assert!(!tx2.conflicts_with(&tx1));
}

#[test]
fn test_read_read_no_conflict() {
    let account_a = pubkey_n(1);

    // Transaction 1: reads A
    let batch1 = InstructionBatch::default();
    let tx1 = ScheduledTransaction::new_with_accounts(
        1,
        batch1,
        vec![account_a], // read accounts
        vec![],          // write accounts
        1,
    );

    // Transaction 2: reads A
    let batch2 = InstructionBatch::default();
    let tx2 = ScheduledTransaction::new_with_accounts(
        2,
        batch2,
        vec![account_a], // read accounts
        vec![],          // write accounts
        1,
    );

    // Should NOT conflict (both reading)
    assert!(!tx1.conflicts_with(&tx2));
}

#[test]
fn test_execution_batch_add() {
    let account_a = pubkey_n(1);
    let account_b = pubkey_n(2);

    let mut batch = ExecutionBatch::new(1);

    // Add first transaction (writes A)
    let tx_batch1 = InstructionBatch::default();
    let tx1 = ScheduledTransaction::new_with_accounts(1, tx_batch1, vec![], vec![account_a], 1);
    assert!(batch.try_add(tx1));

    // Add non-conflicting transaction (writes B)
    let tx_batch2 = InstructionBatch::default();
    let tx2 = ScheduledTransaction::new_with_accounts(2, tx_batch2, vec![], vec![account_b], 1);
    assert!(batch.try_add(tx2));

    assert_eq!(batch.transactions.len(), 2);
}

#[test]
fn test_execution_batch_rejects_conflict() {
    let account_a = pubkey_n(1);

    let mut batch = ExecutionBatch::new(1);

    // Add first transaction writing A
    let tx_batch1 = InstructionBatch::default();
    let tx1 = ScheduledTransaction::new_with_accounts(1, tx_batch1, vec![], vec![account_a], 1);
    assert!(batch.try_add(tx1));

    // Try to add conflicting transaction (also writing A)
    let tx_batch2 = InstructionBatch::default();
    let tx2 = ScheduledTransaction::new_with_accounts(2, tx_batch2, vec![], vec![account_a], 1);

    // Should be rejected
    assert!(!batch.try_add(tx2));
    assert_eq!(batch.transactions.len(), 1);
}

#[test]
fn test_deterministic_conflict_resolution() {
    // Same conflicts should resolve the same way every time
    let account = pubkey_n(1);

    for _ in 0..10 {
        let mut batch = ExecutionBatch::new(1);

        // Create transactions in same order, all writing to the same account
        let txs: Vec<_> = (0..5)
            .map(|i| {
                let tx_batch = InstructionBatch::default();
                ScheduledTransaction::new_with_accounts(
                    i as u64,
                    tx_batch,
                    vec![],
                    vec![account],
                    i as u32,
                )
            })
            .collect();

        // Only first should be added (all conflict on same account)
        for tx in txs {
            let _ = batch.try_add(tx);
        }

        // Should always have exactly 1 transaction (the first one)
        assert_eq!(batch.transactions.len(), 1);
        assert_eq!(batch.transactions[0].id, 0);
    }
}

#[test]
fn test_scheduler_config_defaults() {
    let config = SchedulerConfig::default();

    assert!(config.max_parallelism > 0);
    assert!(config.max_batch_size > 0);
    assert!(config.lock_timeout_ms > 0);
    assert!(config.deterministic_resolution);
}

#[test]
fn test_concurrent_lock_safety() {
    use std::sync::atomic::{AtomicU64, Ordering};

    let manager = Arc::new(AccountLockManager::new(5000));
    let account = pubkey_n(1);
    let counter = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let m = Arc::clone(&manager);
            let c = Arc::clone(&counter);
            thread::spawn(move || {
                for _ in 0..100 {
                    // Try to acquire write lock
                    if let Ok(locks) = m.try_acquire_locks(i, &[], &[account]) {
                        // Critical section
                        c.fetch_add(1, Ordering::SeqCst);
                        thread::sleep(Duration::from_micros(10));
                        c.fetch_sub(1, Ordering::SeqCst);

                        // Verify we're alone in critical section
                        assert_eq!(c.load(Ordering::SeqCst), 0);

                        m.release_locks(i, &locks);
                    }
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_clear_all_locks() {
    let manager = AccountLockManager::new(5000);
    let accounts: Vec<_> = (0..10).map(pubkey_n).collect();

    // Acquire various locks
    for (i, acc) in accounts.iter().enumerate() {
        if i % 2 == 0 {
            manager.try_acquire_locks(i as u64, &[], &[*acc]).ok();
        } else {
            manager.try_acquire_locks(i as u64, &[*acc], &[]).ok();
        }
    }

    // Verify some are locked
    assert!(accounts.iter().any(|a| manager.is_locked(a)));

    // Clear all
    manager.clear_all();

    // All should be unlocked
    for acc in &accounts {
        assert!(!manager.is_locked(acc));
    }
}

/// Property-based scheduler tests
#[cfg(feature = "proptest")]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_conflict_symmetric(
            write1 in prop::collection::vec(0u8..10, 0..5),
            write2 in prop::collection::vec(0u8..10, 0..5)
        ) {
            let writes1: Vec<Pubkey> = write1.into_iter().map(pubkey_n).collect();
            let writes2: Vec<Pubkey> = write2.into_iter().map(pubkey_n).collect();

            let mut batch1 = InstructionBatch::default();
            batch1.account_keys = writes1.clone();
            batch1.writable_indices = (0..writes1.len() as u8).collect();
            let tx1 = ScheduledTransaction::new(1, batch1, 1);

            let mut batch2 = InstructionBatch::default();
            batch2.account_keys = writes2.clone();
            batch2.writable_indices = (0..writes2.len() as u8).collect();
            let tx2 = ScheduledTransaction::new(2, batch2, 1);

            // Conflict should be symmetric
            prop_assert_eq!(tx1.conflicts_with(&tx2), tx2.conflicts_with(&tx1));
        }
    }
}
