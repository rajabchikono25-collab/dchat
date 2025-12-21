//! CPI borrow-rule enforcement tests
//!
//! Verifies:
//! - Borrow rules are enforced during CPI
//! - Reentrancy is detected and prevented
//! - CPI depth limits are enforced
//! - Privilege escalation is prevented

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use dchat_programs::account::{AccountAccessTracker, AccountInfo, AccountMeta, Pubkey};
use dchat_programs::cpi::{CpiContext, CpiResult, CrossProgramInvocation, PrivilegeChecker};
use dchat_programs::error::ProgramError;
use dchat_programs::instruction::Instruction;
use dchat_programs::metering::{ComputeBudget, ComputeMeter};
use dchat_programs::MAX_CPI_DEPTH;

fn pubkey_n(n: u8) -> Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    Pubkey::new(bytes)
}

/// Holder struct that owns data for AccountInfo references.
/// This pattern allows us to create AccountInfo with proper lifetimes in tests.
/// The holder owns the mutable data, and we can create AccountInfo references
/// that borrow from it.
struct AccountInfoHolder {
    key: Pubkey,
    owner: Pubkey,
    lamports: u64,
    data: Vec<u8>,
    is_signer: bool,
    is_writable: bool,
    executable: bool,
    rent_epoch: u64,
}

impl AccountInfoHolder {
    fn new(key: Pubkey, is_signer: bool, is_writable: bool, lamports: u64) -> Self {
        Self {
            key,
            owner: pubkey_n(0), // system program as default owner
            lamports,
            data: Vec::new(),
            is_signer,
            is_writable,
            executable: false,
            rent_epoch: 0,
        }
    }

    fn with_owner(mut self, owner: Pubkey) -> Self {
        self.owner = owner;
        self
    }

    fn with_data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }
}

/// Container that holds multiple accounts and provides AccountInfo references.
/// This manages the complex lifetime requirements by owning all the mutable state.
struct AccountInfoTestContext {
    /// Holders that own the account data
    holders: Vec<AccountInfoHolder>,
    /// Mutable lamports storage (we need separate storage for the mutable refs)
    lamports_storage: Vec<RefCell<u64>>,
    /// Mutable data storage
    data_storage: Vec<RefCell<Vec<u8>>>,
}

impl AccountInfoTestContext {
    fn new() -> Self {
        Self {
            holders: Vec::new(),
            lamports_storage: Vec::new(),
            data_storage: Vec::new(),
        }
    }

    fn add_account(&mut self, holder: AccountInfoHolder) -> usize {
        let idx = self.holders.len();
        self.lamports_storage.push(RefCell::new(holder.lamports));
        self.data_storage.push(RefCell::new(holder.data.clone()));
        self.holders.push(holder);
        idx
    }

    /// Create AccountInfo references for all accounts in this context.
    /// The returned AccountInfos borrow from self, so self must outlive them.
    fn account_infos(&self) -> Vec<AccountInfo<'_>> {
        self.holders
            .iter()
            .enumerate()
            .map(|(i, holder)| {
                // We need to create the Rc<RefCell<&mut T>> pattern
                // For tests, we use a simpler approach: create temporary mutable refs
                // Note: This is safe because we're in a single-threaded test context

                // Get mutable references to the storage
                let lamports_ref = unsafe {
                    // SAFETY: We're in a test context and ensure single-threaded access
                    let ptr = self.lamports_storage[i].as_ptr();
                    &mut *ptr
                };
                let data_ref = unsafe {
                    // SAFETY: We're in a test context and ensure single-threaded access
                    let ptr = self.data_storage[i].as_ptr();
                    (*ptr).as_mut_slice()
                };

                AccountInfo {
                    key: &holder.key,
                    is_signer: holder.is_signer,
                    is_writable: holder.is_writable,
                    lamports: Rc::new(RefCell::new(lamports_ref)),
                    data: Rc::new(RefCell::new(data_ref)),
                    owner: &holder.owner,
                    executable: holder.executable,
                    rent_epoch: holder.rent_epoch,
                }
            })
            .collect()
    }
}

/// Simple helper for single-account test cases using a scoped callback pattern.
/// This avoids lifetime issues by keeping everything in scope.
fn with_account_info<F, R>(
    key: Pubkey,
    is_signer: bool,
    is_writable: bool,
    lamports: u64,
    f: F,
) -> R
where
    F: FnOnce(AccountInfo<'_>) -> R,
{
    let owner = pubkey_n(0);
    let mut lamports_val = lamports;
    let mut data_val: Vec<u8> = Vec::new();

    let account_info = AccountInfo {
        key: &key,
        is_signer,
        is_writable,
        lamports: Rc::new(RefCell::new(&mut lamports_val)),
        data: Rc::new(RefCell::new(data_val.as_mut_slice())),
        owner: &owner,
        executable: false,
        rent_epoch: 0,
    };

    f(account_info)
}

/// Individual account storage - each account has its own owned data
struct AccountStorage {
    key: Pubkey,
    is_signer: bool,
    is_writable: bool,
    lamports: u64,
    data: Vec<u8>,
}

/// Storage container that owns all account data
struct AccountStorageContainer {
    accounts: Vec<AccountStorage>,
    owner: Pubkey,
}

impl AccountStorageContainer {
    fn new(accounts: Vec<(Pubkey, bool, bool, u64)>) -> Self {
        Self {
            accounts: accounts
                .into_iter()
                .map(|(key, is_signer, is_writable, lamports)| AccountStorage {
                    key,
                    is_signer,
                    is_writable,
                    lamports,
                    data: Vec::new(),
                })
                .collect(),
            owner: pubkey_n(0),
        }
    }

    /// Run a closure with AccountInfo references
    /// Uses destructuring to satisfy borrow checker
    fn with_account_infos<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&[AccountInfo<'_>]) -> R,
    {
        // For small test cases, we handle them explicitly to avoid borrow issues
        match self.accounts.len() {
            0 => f(&[]),
            1 => {
                // Extract fields to separate variables to avoid mixed borrow issues
                let AccountStorage {
                    key: ref key0,
                    is_signer: is_signer0,
                    is_writable: is_writable0,
                    lamports: ref mut lamports0,
                    data: ref mut data0,
                } = self.accounts[0];
                let info0 = AccountInfo {
                    key: key0,
                    is_signer: is_signer0,
                    is_writable: is_writable0,
                    lamports: Rc::new(RefCell::new(lamports0)),
                    data: Rc::new(RefCell::new(data0.as_mut_slice())),
                    owner: &self.owner,
                    executable: false,
                    rent_epoch: 0,
                };
                f(&[info0])
            }
            2 => {
                // Split first to get separate mutable access
                let (first, rest) = self.accounts.split_at_mut(1);
                let AccountStorage {
                    key: ref key0,
                    is_signer: is_signer0,
                    is_writable: is_writable0,
                    lamports: ref mut lamports0,
                    data: ref mut data0,
                } = first[0];
                let AccountStorage {
                    key: ref key1,
                    is_signer: is_signer1,
                    is_writable: is_writable1,
                    lamports: ref mut lamports1,
                    data: ref mut data1,
                } = rest[0];
                let info0 = AccountInfo {
                    key: key0,
                    is_signer: is_signer0,
                    is_writable: is_writable0,
                    lamports: Rc::new(RefCell::new(lamports0)),
                    data: Rc::new(RefCell::new(data0.as_mut_slice())),
                    owner: &self.owner,
                    executable: false,
                    rent_epoch: 0,
                };
                let info1 = AccountInfo {
                    key: key1,
                    is_signer: is_signer1,
                    is_writable: is_writable1,
                    lamports: Rc::new(RefCell::new(lamports1)),
                    data: Rc::new(RefCell::new(data1.as_mut_slice())),
                    owner: &self.owner,
                    executable: false,
                    rent_epoch: 0,
                };
                f(&[info0, info1])
            }
            3 => {
                let (first, rest) = self.accounts.split_at_mut(1);
                let (second, third) = rest.split_at_mut(1);
                let AccountStorage {
                    key: ref key0,
                    is_signer: is_signer0,
                    is_writable: is_writable0,
                    lamports: ref mut lamports0,
                    data: ref mut data0,
                } = first[0];
                let AccountStorage {
                    key: ref key1,
                    is_signer: is_signer1,
                    is_writable: is_writable1,
                    lamports: ref mut lamports1,
                    data: ref mut data1,
                } = second[0];
                let AccountStorage {
                    key: ref key2,
                    is_signer: is_signer2,
                    is_writable: is_writable2,
                    lamports: ref mut lamports2,
                    data: ref mut data2,
                } = third[0];
                let info0 = AccountInfo {
                    key: key0,
                    is_signer: is_signer0,
                    is_writable: is_writable0,
                    lamports: Rc::new(RefCell::new(lamports0)),
                    data: Rc::new(RefCell::new(data0.as_mut_slice())),
                    owner: &self.owner,
                    executable: false,
                    rent_epoch: 0,
                };
                let info1 = AccountInfo {
                    key: key1,
                    is_signer: is_signer1,
                    is_writable: is_writable1,
                    lamports: Rc::new(RefCell::new(lamports1)),
                    data: Rc::new(RefCell::new(data1.as_mut_slice())),
                    owner: &self.owner,
                    executable: false,
                    rent_epoch: 0,
                };
                let info2 = AccountInfo {
                    key: key2,
                    is_signer: is_signer2,
                    is_writable: is_writable2,
                    lamports: Rc::new(RefCell::new(lamports2)),
                    data: Rc::new(RefCell::new(data2.as_mut_slice())),
                    owner: &self.owner,
                    executable: false,
                    rent_epoch: 0,
                };
                f(&[info0, info1, info2])
            }
            _ => {
                // For larger counts, we'd need a macro or unsafe code
                // Tests in this file use at most 3 accounts
                panic!("AccountStorageContainer::with_account_infos supports up to 3 accounts in tests")
            }
        }
    }
}

/// Helper to run privilege check tests with properly scoped AccountInfo
fn test_privilege_check<F>(
    accounts: Vec<(Pubkey, bool, bool, u64)>, // (key, is_signer, is_writable, lamports)
    instruction: Instruction,
    check: F,
) where
    F: FnOnce(Result<(), ProgramError>),
{
    let mut container = AccountStorageContainer::new(accounts);
    container.with_account_infos(|account_infos| {
        let result = PrivilegeChecker::check_no_escalation(account_infos, &instruction);
        check(result);
    });
}

#[test]
fn test_borrow_tracker_creation() {
    let tracker = AccountAccessTracker::new();
    let account = pubkey_n(1);

    assert!(tracker.can_borrow_immutable(&account));
    assert!(tracker.can_borrow_mutable(&account));
}

#[test]
fn test_immutable_borrow() {
    let mut tracker = AccountAccessTracker::new();
    let account = pubkey_n(1);

    // Add immutable borrow
    assert!(tracker.add_immutable_borrow(account).is_ok());
    assert!(tracker.can_borrow_immutable(&account));

    // Can add multiple immutable borrows
    assert!(tracker.add_immutable_borrow(account).is_ok());
    assert!(tracker.can_borrow_immutable(&account));

    // Cannot get mutable borrow while immutably borrowed
    assert!(!tracker.can_borrow_mutable(&account));
}

#[test]
fn test_mutable_borrow() {
    let mut tracker = AccountAccessTracker::new();
    let account = pubkey_n(1);

    // Add mutable borrow
    assert!(tracker.add_mutable_borrow(account).is_ok());

    // Cannot add another mutable borrow
    assert!(!tracker.can_borrow_mutable(&account));

    // Cannot add immutable borrow while mutably borrowed
    assert!(!tracker.can_borrow_immutable(&account));
}

#[test]
fn test_borrow_release() {
    let mut tracker = AccountAccessTracker::new();
    let account = pubkey_n(1);

    // Add and release mutable borrow
    assert!(tracker.add_mutable_borrow(account).is_ok());
    assert!(!tracker.can_borrow_mutable(&account));

    tracker.release_mutable_borrow(&account);
    assert!(tracker.can_borrow_mutable(&account));
}

#[test]
fn test_immutable_borrow_release() {
    let mut tracker = AccountAccessTracker::new();
    let account = pubkey_n(1);

    // Add two immutable borrows
    assert!(tracker.add_immutable_borrow(account).is_ok());
    assert!(tracker.add_immutable_borrow(account).is_ok());

    // Release one - still cannot get mutable
    tracker.release_immutable_borrow(&account);
    assert!(!tracker.can_borrow_mutable(&account));

    // Release second - now can get mutable
    tracker.release_immutable_borrow(&account);
    assert!(tracker.can_borrow_mutable(&account));
}

#[test]
fn test_cpi_depth_enforcement() {
    let program_a = pubkey_n(1);
    let program_b = pubkey_n(2);
    let budget = ComputeBudget::default();
    let meter = Arc::new(ComputeMeter::new(budget));
    let mut tracker = AccountAccessTracker::new();

    // Start at depth 0
    let mut ctx = CpiContext::new(program_a, &[], meter.clone(), &mut tracker);
    assert_eq!(ctx.depth, 0);

    // Create nested context up to max depth
    for i in 1..=MAX_CPI_DEPTH {
        let child = ctx.child(pubkey_n((i + 10) as u8));
        if i <= MAX_CPI_DEPTH {
            assert!(child.is_ok());
        } else {
            assert!(matches!(child, Err(ProgramError::CallDepthExceeded)));
        }
    }
}

#[test]
fn test_reentrancy_detection() {
    let program_a = pubkey_n(1);
    let program_b = pubkey_n(2);
    let budget = ComputeBudget::default();
    let meter = Arc::new(ComputeMeter::new(budget));
    let mut tracker = AccountAccessTracker::new();

    let mut ctx = CpiContext::new(program_a, &[], meter.clone(), &mut tracker);

    // Child context for program B
    let mut child_b = ctx.child(program_b).expect("child b");

    // Try to re-enter program A from B - should fail
    let reentry = child_b.child(program_a);
    assert!(matches!(reentry, Err(ProgramError::ReentrancyDetected)));
}

#[test]
fn test_privilege_no_escalation_signer() {
    let account_key = pubkey_n(1);
    let program_id = pubkey_n(10);

    // CPI tries to make it a signer - should fail
    let instruction = Instruction {
        program_id,
        accounts: vec![AccountMeta::new(account_key, true)], // is_signer = true
        data: vec![],
    };

    // Caller has account as non-signer
    test_privilege_check(
        vec![(account_key, false, true, 1000)], // is_signer=false
        instruction,
        |result| {
            assert!(matches!(
                result,
                Err(ProgramError::MissingRequiredSignature)
            ));
        },
    );
}

#[test]
fn test_privilege_no_escalation_writable() {
    let account_key = pubkey_n(1);
    let program_id = pubkey_n(10);

    // CPI tries to make it writable - should fail
    let instruction = Instruction {
        program_id,
        accounts: vec![AccountMeta::new(account_key, false)], // is_writable = true
        data: vec![],
    };

    // Caller has account as read-only
    test_privilege_check(
        vec![(account_key, false, false, 1000)], // is_writable=false
        instruction,
        |result| {
            assert!(matches!(result, Err(ProgramError::AccountNotWritable)));
        },
    );
}

#[test]
fn test_privilege_allowed_same_privileges() {
    let account_key = pubkey_n(1);
    let program_id = pubkey_n(10);

    // CPI requests same privileges - should succeed
    let instruction = Instruction {
        program_id,
        accounts: vec![AccountMeta {
            pubkey: account_key,
            is_signer: true,
            is_writable: true,
        }],
        data: vec![],
    };

    // Caller has account as signer + writable
    test_privilege_check(
        vec![(account_key, true, true, 1000)],
        instruction,
        |result| {
            assert!(result.is_ok());
        },
    );
}

#[test]
fn test_privilege_allowed_reduced_privileges() {
    let account_key = pubkey_n(1);
    let program_id = pubkey_n(10);

    // CPI requests less privileges - should succeed
    let instruction = Instruction {
        program_id,
        accounts: vec![AccountMeta {
            pubkey: account_key,
            is_signer: false,   // less than caller
            is_writable: false, // less than caller
        }],
        data: vec![],
    };

    // Caller has account as signer + writable
    test_privilege_check(
        vec![(account_key, true, true, 1000)],
        instruction,
        |result| {
            assert!(result.is_ok());
        },
    );
}

#[test]
fn test_cpi_borrow_overlap_detection() {
    let account = pubkey_n(1);
    let mut tracker = AccountAccessTracker::new();

    // Simulate first CPI borrows mutably
    assert!(tracker.add_mutable_borrow(account).is_ok());

    // Second CPI trying to borrow same account should fail
    let can_borrow = CrossProgramInvocation::can_borrow(&tracker, &account, true);
    assert!(!can_borrow);

    // Even immutable borrow should fail
    let can_borrow = CrossProgramInvocation::can_borrow(&tracker, &account, false);
    assert!(!can_borrow);
}

#[test]
fn test_cpi_signer_seeds() {
    let program_a = pubkey_n(1);
    let budget = ComputeBudget::default();
    let meter = Arc::new(ComputeMeter::new(budget));
    let mut tracker = AccountAccessTracker::new();

    let ctx = CpiContext::new(program_a, &[], meter.clone(), &mut tracker);

    // Add signer seeds for PDA
    let seeds = vec![b"seed1".to_vec(), b"seed2".to_vec()];
    let ctx_with_signer = ctx.with_signer(seeds);

    assert_eq!(ctx_with_signer.signer_seeds.len(), 1);
}

#[test]
fn test_cpi_result_structure() {
    let result = CpiResult {
        success: true,
        error_code: None,
        return_data: vec![1, 2, 3, 4],
        compute_consumed: 1000,
    };

    assert!(result.success);
    assert!(result.error_code.is_none());
    assert_eq!(result.return_data.len(), 4);
    assert_eq!(result.compute_consumed, 1000);
}

#[test]
fn test_cpi_result_failure() {
    let result = CpiResult {
        success: false,
        error_code: Some(42),
        return_data: vec![],
        compute_consumed: 500,
    };

    assert!(!result.success);
    assert_eq!(result.error_code, Some(42));
}

#[test]
fn test_call_chain_tracking() {
    let program_a = pubkey_n(1);
    let program_b = pubkey_n(2);
    let program_c = pubkey_n(3);
    let budget = ComputeBudget::default();
    let meter = Arc::new(ComputeMeter::new(budget));
    let mut tracker = AccountAccessTracker::new();

    let mut ctx = CpiContext::new(program_a, &[], meter.clone(), &mut tracker);

    // Call chain starts with A
    assert!(ctx.call_chain.contains(&program_a));
    assert!(!ctx.call_chain.contains(&program_b));

    // Create child for B
    let mut child_b = ctx.child(program_b).unwrap();
    assert!(child_b.call_chain.contains(&program_a));
    assert!(child_b.call_chain.contains(&program_b));

    // Create child for C
    let child_c = child_b.child(program_c).unwrap();
    assert!(child_c.call_chain.contains(&program_a));
    assert!(child_c.call_chain.contains(&program_b));
    assert!(child_c.call_chain.contains(&program_c));
}

#[test]
fn test_compute_meter_shared_across_cpi() {
    let program_a = pubkey_n(1);
    let program_b = pubkey_n(2);
    let budget = ComputeBudget::new(10000);
    let meter = Arc::new(ComputeMeter::new(budget));
    let mut tracker = AccountAccessTracker::new();

    let mut ctx_a = CpiContext::new(program_a, &[], meter.clone(), &mut tracker);

    // Consume in parent
    assert!(ctx_a.compute_meter.consume(1000).is_ok());
    assert_eq!(ctx_a.compute_meter.consumed(), 1000);

    // Child shares same meter
    let child_b = ctx_a.child(program_b).unwrap();
    assert_eq!(child_b.compute_meter.consumed(), 1000);

    // Consume in child reflects in parent's meter
    assert!(child_b.compute_meter.consume(500).is_ok());
    // Note: consumed value now includes child consumption
    assert_eq!(meter.consumed(), 1500);
}

/// Property-based CPI tests
#[cfg(feature = "proptest")]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_cpi_depth_never_exceeds_max(depth in 0usize..20) {
            let program = pubkey_n(1);
            let budget = ComputeBudget::default();
            let meter = Arc::new(ComputeMeter::new(budget));
            let mut tracker = AccountAccessTracker::new();

            let mut ctx = CpiContext::new(program, &[], meter.clone(), &mut tracker);
            ctx.depth = depth;

            if depth >= MAX_CPI_DEPTH {
                let result = ctx.child(pubkey_n(99));
                prop_assert!(matches!(result, Err(ProgramError::CallDepthExceeded)));
            }
        }
    }
}
