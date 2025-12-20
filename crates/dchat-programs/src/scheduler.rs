//! Parallel execution scheduler with read/write account locks

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};
use crate::instruction::{CompiledInstruction, InstructionBatch};

/// Scheduler configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum parallel execution threads
    pub max_parallelism: usize,
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Lock timeout in milliseconds
    pub lock_timeout_ms: u64,
    /// Enable conflict detection logging
    pub log_conflicts: bool,
    /// Deterministic conflict resolution
    pub deterministic_resolution: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_parallelism: 8,
            max_batch_size: 64,
            lock_timeout_ms: 5000,
            log_conflicts: true,
            deterministic_resolution: true,
        }
    }
}

/// Lock type for an account
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockType {
    /// Read lock (shared)
    Read,
    /// Write lock (exclusive)
    Write,
}

/// Account lock state
#[derive(Debug, Clone)]
pub struct AccountLock {
    /// Account being locked
    pub account: Pubkey,
    /// Lock type
    pub lock_type: LockType,
    /// Transaction holding the lock
    pub holder: u64,
    /// Lock acquisition time (for timeout)
    pub acquired_at: std::time::Instant,
}

impl AccountLock {
    /// Create a new lock
    pub fn new(account: Pubkey, lock_type: LockType, holder: u64) -> Self {
        Self {
            account,
            lock_type,
            holder,
            acquired_at: std::time::Instant::now(),
        }
    }
}

/// Lock manager for accounts
#[derive(Debug)]
pub struct AccountLockManager {
    /// Read locks per account (can have multiple readers)
    read_locks: RwLock<HashMap<Pubkey, HashSet<u64>>>,
    /// Write locks per account (exclusive)
    write_locks: RwLock<HashMap<Pubkey, u64>>,
    /// Lock timeout
    lock_timeout: std::time::Duration,
}

impl AccountLockManager {
    /// Create new lock manager
    pub fn new(lock_timeout_ms: u64) -> Self {
        Self {
            read_locks: RwLock::new(HashMap::new()),
            write_locks: RwLock::new(HashMap::new()),
            lock_timeout: std::time::Duration::from_millis(lock_timeout_ms),
        }
    }

    /// Try to acquire locks for a transaction
    pub fn try_acquire_locks(
        &self,
        tx_id: u64,
        read_accounts: &[Pubkey],
        write_accounts: &[Pubkey],
    ) -> ProgramResult<Vec<AccountLock>> {
        let mut acquired_locks = Vec::new();

        // First, check if all locks can be acquired (deterministic check)
        {
            let read_guard = self.read_locks.read();
            let write_guard = self.write_locks.read();

            // Check write locks can be acquired
            for account in write_accounts {
                // Cannot acquire write lock if there's an existing write lock
                if write_guard.contains_key(account) {
                    return Err(ProgramError::AccountLocked);
                }
                // Cannot acquire write lock if there are read locks
                if let Some(readers) = read_guard.get(account) {
                    if !readers.is_empty() {
                        return Err(ProgramError::AccountLocked);
                    }
                }
            }

            // Check read locks can be acquired
            for account in read_accounts {
                // Cannot acquire read lock if there's a write lock
                if write_guard.contains_key(account) {
                    return Err(ProgramError::AccountLocked);
                }
            }
        }

        // Now actually acquire the locks
        {
            let mut read_guard = self.read_locks.write();
            let mut write_guard = self.write_locks.write();

            // Acquire write locks
            for account in write_accounts {
                write_guard.insert(*account, tx_id);
                acquired_locks.push(AccountLock::new(*account, LockType::Write, tx_id));
            }

            // Acquire read locks
            for account in read_accounts {
                read_guard
                    .entry(*account)
                    .or_insert_with(HashSet::new)
                    .insert(tx_id);
                acquired_locks.push(AccountLock::new(*account, LockType::Read, tx_id));
            }
        }

        Ok(acquired_locks)
    }

    /// Release locks for a transaction
    pub fn release_locks(&self, tx_id: u64, locks: &[AccountLock]) {
        let mut read_guard = self.read_locks.write();
        let mut write_guard = self.write_locks.write();

        for lock in locks {
            match lock.lock_type {
                LockType::Read => {
                    if let Some(readers) = read_guard.get_mut(&lock.account) {
                        readers.remove(&tx_id);
                        if readers.is_empty() {
                            read_guard.remove(&lock.account);
                        }
                    }
                }
                LockType::Write => {
                    if write_guard.get(&lock.account) == Some(&tx_id) {
                        write_guard.remove(&lock.account);
                    }
                }
            }
        }
    }

    /// Check if an account is locked
    pub fn is_locked(&self, account: &Pubkey) -> bool {
        let read_guard = self.read_locks.read();
        let write_guard = self.write_locks.read();

        write_guard.contains_key(account)
            || read_guard.get(account).map_or(false, |r| !r.is_empty())
    }

    /// Clear all locks (for testing/reset)
    pub fn clear_all(&self) {
        self.read_locks.write().clear();
        self.write_locks.write().clear();
    }
}

/// Transaction entry for scheduling
#[derive(Debug, Clone)]
pub struct ScheduledTransaction {
    /// Transaction ID (deterministic ordering key)
    pub id: u64,
    /// Read accounts
    pub read_accounts: Vec<Pubkey>,
    /// Write accounts
    pub write_accounts: Vec<Pubkey>,
    /// The instruction batch
    pub batch: InstructionBatch,
    /// Priority (higher = sooner)
    pub priority: u32,
    /// Attempt count (for retries)
    pub attempts: u32,
}

impl ScheduledTransaction {
    /// Create new scheduled transaction
    pub fn new(id: u64, batch: InstructionBatch, priority: u32) -> Self {
        // Extract read/write accounts from batch
        let mut read_accounts = Vec::new();
        let mut write_accounts = Vec::new();

        for (idx, key) in batch.account_keys.iter().enumerate() {
            let idx = idx as u8;
            if batch.writable_indices.contains(&idx) {
                write_accounts.push(*key);
            } else {
                read_accounts.push(*key);
            }
        }

        Self {
            id,
            read_accounts,
            write_accounts,
            batch,
            priority,
            attempts: 0,
        }
    }

    /// Check if this transaction conflicts with another
    pub fn conflicts_with(&self, other: &ScheduledTransaction) -> bool {
        // Write-write conflict
        for w1 in &self.write_accounts {
            if other.write_accounts.contains(w1) {
                return true;
            }
        }

        // Write-read conflict (our writes vs their reads)
        for w in &self.write_accounts {
            if other.read_accounts.contains(w) {
                return true;
            }
        }

        // Read-write conflict (our reads vs their writes)
        for r in &self.read_accounts {
            if other.write_accounts.contains(r) {
                return true;
            }
        }

        false
    }
}

/// Execution batch - set of non-conflicting transactions
#[derive(Debug, Clone)]
pub struct ExecutionBatch {
    /// Batch ID
    pub id: u64,
    /// Transactions in this batch (can run in parallel)
    pub transactions: Vec<ScheduledTransaction>,
    /// Total priority
    pub total_priority: u64,
}

impl ExecutionBatch {
    /// Create a new batch
    pub fn new(id: u64) -> Self {
        Self {
            id,
            transactions: Vec::new(),
            total_priority: 0,
        }
    }

    /// Try to add a transaction to the batch
    pub fn try_add(&mut self, tx: ScheduledTransaction) -> bool {
        // Check conflicts with existing transactions
        for existing in &self.transactions {
            if tx.conflicts_with(existing) {
                return false;
            }
        }

        self.total_priority += tx.priority as u64;
        self.transactions.push(tx);
        true
    }

    /// Get transaction count
    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}

/// Parallel execution scheduler
pub struct ParallelScheduler {
    /// Configuration
    config: SchedulerConfig,
    /// Lock manager
    lock_manager: Arc<AccountLockManager>,
    /// Pending transactions (deterministically ordered)
    pending: Mutex<VecDeque<ScheduledTransaction>>,
    /// Next transaction ID
    next_tx_id: Mutex<u64>,
    /// Next batch ID
    next_batch_id: Mutex<u64>,
    /// Conflict count metrics
    conflict_count: Mutex<u64>,
}

impl ParallelScheduler {
    /// Create a new scheduler
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            lock_manager: Arc::new(AccountLockManager::new(config.lock_timeout_ms)),
            pending: Mutex::new(VecDeque::new()),
            next_tx_id: Mutex::new(0),
            next_batch_id: Mutex::new(0),
            conflict_count: Mutex::new(0),
            config,
        }
    }

    /// Submit a transaction for scheduling
    pub fn submit(&self, batch: InstructionBatch, priority: u32) -> u64 {
        let mut next_id = self.next_tx_id.lock();
        let id = *next_id;
        *next_id += 1;

        let tx = ScheduledTransaction::new(id, batch, priority);
        self.pending.lock().push_back(tx);

        id
    }

    /// Schedule next execution batch
    pub fn schedule_batch(&self) -> Option<ExecutionBatch> {
        let mut pending = self.pending.lock();
        if pending.is_empty() {
            return None;
        }

        let mut batch_id = self.next_batch_id.lock();
        let mut batch = ExecutionBatch::new(*batch_id);
        *batch_id += 1;

        // Collect transactions that can run in parallel
        let mut remaining = VecDeque::new();

        while let Some(tx) = pending.pop_front() {
            if batch.len() >= self.config.max_batch_size {
                remaining.push_back(tx);
                continue;
            }

            // Try to add to batch (deterministic conflict resolution)
            if batch.try_add(tx.clone()) {
                // Successfully added
            } else {
                // Conflict detected
                if self.config.log_conflicts {
                    *self.conflict_count.lock() += 1;
                }
                remaining.push_back(tx);
            }
        }

        // Put remaining back
        *pending = remaining;

        if batch.is_empty() {
            // All transactions conflict with each other, force one through
            if let Some(tx) = pending.pop_front() {
                batch.transactions.push(tx);
            }
        }

        if batch.is_empty() {
            None
        } else {
            Some(batch)
        }
    }

    /// Acquire locks for a batch
    pub fn acquire_batch_locks(
        &self,
        batch: &ExecutionBatch,
    ) -> ProgramResult<Vec<Vec<AccountLock>>> {
        let mut all_locks = Vec::with_capacity(batch.transactions.len());

        for tx in &batch.transactions {
            match self
                .lock_manager
                .try_acquire_locks(tx.id, &tx.read_accounts, &tx.write_accounts)
            {
                Ok(locks) => all_locks.push(locks),
                Err(e) => {
                    // Release any locks we've already acquired
                    for (i, locks) in all_locks.iter().enumerate() {
                        let tx_id = batch.transactions[i].id;
                        self.lock_manager.release_locks(tx_id, locks);
                    }
                    return Err(e);
                }
            }
        }

        Ok(all_locks)
    }

    /// Release locks for a batch
    pub fn release_batch_locks(&self, batch: &ExecutionBatch, locks: Vec<Vec<AccountLock>>) {
        for (tx, tx_locks) in batch.transactions.iter().zip(locks.iter()) {
            self.lock_manager.release_locks(tx.id, tx_locks);
        }
    }

    /// Re-queue failed transaction
    pub fn requeue(&self, tx: ScheduledTransaction) {
        let mut tx = tx;
        tx.attempts += 1;
        // Lower priority for retries
        tx.priority = tx.priority.saturating_sub(tx.attempts);
        self.pending.lock().push_back(tx);
    }

    /// Get pending transaction count
    pub fn pending_count(&self) -> usize {
        self.pending.lock().len()
    }

    /// Get conflict count
    pub fn conflict_count(&self) -> u64 {
        *self.conflict_count.lock()
    }

    /// Clear all pending transactions
    pub fn clear(&self) {
        self.pending.lock().clear();
        self.lock_manager.clear_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx(id: u64, read: &[u8], write: &[u8]) -> ScheduledTransaction {
        ScheduledTransaction {
            id,
            read_accounts: read.iter().map(|b| Pubkey::new([*b; 32])).collect(),
            write_accounts: write.iter().map(|b| Pubkey::new([*b; 32])).collect(),
            batch: InstructionBatch {
                account_keys: vec![],
                signer_indices: vec![],
                writable_indices: vec![],
                instructions: vec![],
                recent_blockhash: [0u8; 32],
                intent_id: None,
            },
            priority: 100,
            attempts: 0,
        }
    }

    #[test]
    fn test_conflict_detection() {
        // No conflict - different accounts
        let tx1 = make_tx(1, &[1], &[2]);
        let tx2 = make_tx(2, &[3], &[4]);
        assert!(!tx1.conflicts_with(&tx2));

        // Write-write conflict
        let tx3 = make_tx(3, &[], &[1]);
        let tx4 = make_tx(4, &[], &[1]);
        assert!(tx3.conflicts_with(&tx4));

        // Read-write conflict
        let tx5 = make_tx(5, &[1], &[]);
        let tx6 = make_tx(6, &[], &[1]);
        assert!(tx5.conflicts_with(&tx6));
        assert!(tx6.conflicts_with(&tx5));

        // Read-read no conflict
        let tx7 = make_tx(7, &[1], &[]);
        let tx8 = make_tx(8, &[1], &[]);
        assert!(!tx7.conflicts_with(&tx8));
    }

    #[test]
    fn test_lock_manager() {
        let manager = AccountLockManager::new(5000);
        let account = Pubkey::new([1u8; 32]);

        // Acquire write lock
        let locks = manager.try_acquire_locks(1, &[], &[account]).unwrap();
        assert!(manager.is_locked(&account));

        // Should fail to acquire another write lock
        assert!(manager.try_acquire_locks(2, &[], &[account]).is_err());

        // Should fail to acquire read lock
        assert!(manager.try_acquire_locks(3, &[account], &[]).is_err());

        // Release lock
        manager.release_locks(1, &locks);
        assert!(!manager.is_locked(&account));

        // Now should be able to acquire read lock
        let read_locks = manager.try_acquire_locks(4, &[account], &[]).unwrap();

        // Multiple read locks should work
        let read_locks2 = manager.try_acquire_locks(5, &[account], &[]).unwrap();

        // But write lock should fail
        assert!(manager.try_acquire_locks(6, &[], &[account]).is_err());

        manager.release_locks(4, &read_locks);
        manager.release_locks(5, &read_locks2);
    }

    #[test]
    fn test_execution_batch() {
        let mut batch = ExecutionBatch::new(0);

        let tx1 = make_tx(1, &[1], &[2]);
        let tx2 = make_tx(2, &[3], &[4]);
        let tx3 = make_tx(3, &[], &[2]); // Conflicts with tx1

        assert!(batch.try_add(tx1));
        assert!(batch.try_add(tx2));
        assert!(!batch.try_add(tx3)); // Should fail - conflicts with tx1

        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn test_scheduler() {
        let config = SchedulerConfig::default();
        let scheduler = ParallelScheduler::new(config);

        // Submit non-conflicting transactions
        let batch1 = InstructionBatch {
            account_keys: vec![Pubkey::new([1u8; 32])],
            signer_indices: vec![0],
            writable_indices: vec![0],
            instructions: vec![],
            recent_blockhash: [0u8; 32],
            intent_id: None,
        };

        let batch2 = InstructionBatch {
            account_keys: vec![Pubkey::new([2u8; 32])],
            signer_indices: vec![0],
            writable_indices: vec![0],
            instructions: vec![],
            recent_blockhash: [0u8; 32],
            intent_id: None,
        };

        scheduler.submit(batch1, 100);
        scheduler.submit(batch2, 100);

        assert_eq!(scheduler.pending_count(), 2);

        let exec_batch = scheduler.schedule_batch().unwrap();
        assert_eq!(exec_batch.len(), 2); // Both should be scheduled together

        assert_eq!(scheduler.pending_count(), 0);
    }
}
