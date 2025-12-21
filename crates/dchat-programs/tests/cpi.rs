//! CPI borrow-rule enforcement tests
//!
//! Verifies:
//! - Borrow rules are enforced during CPI
//! - Reentrancy is detected and prevented
//! - CPI depth limits are enforced
//! - Privilege escalation is prevented

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

fn create_account_info(
    key: Pubkey,
    is_signer: bool,
    is_writable: bool,
    lamports: u64,
) -> AccountInfo<'static> {
    AccountInfo {
        key: &key,
        is_signer,
        is_writable,
        lamports: &std::cell::RefCell::new(lamports),
        data: &std::cell::RefCell::new(vec![]),
        owner: &pubkey_n(0),
        executable: false,
        rent_epoch: 0,
    }
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

    // Caller has account as non-signer
    let caller_account = create_account_info(account_key, false, true, 1000);
    let caller_accounts = vec![caller_account];

    // CPI tries to make it a signer - should fail
    let instruction = Instruction {
        program_id,
        accounts: vec![AccountMeta::new(account_key, true)], // is_signer = true
        data: vec![],
    };

    let result = PrivilegeChecker::check_no_escalation(&caller_accounts, &instruction);
    assert!(matches!(
        result,
        Err(ProgramError::MissingRequiredSignature)
    ));
}

#[test]
fn test_privilege_no_escalation_writable() {
    let account_key = pubkey_n(1);
    let program_id = pubkey_n(10);

    // Caller has account as read-only
    let caller_account = create_account_info(account_key, false, false, 1000); // not writable
    let caller_accounts = vec![caller_account];

    // CPI tries to make it writable - should fail
    let instruction = Instruction {
        program_id,
        accounts: vec![AccountMeta::new(account_key, false)], // is_writable = true
        data: vec![],
    };

    let result = PrivilegeChecker::check_no_escalation(&caller_accounts, &instruction);
    assert!(matches!(result, Err(ProgramError::AccountNotWritable)));
}

#[test]
fn test_privilege_allowed_same_privileges() {
    let account_key = pubkey_n(1);
    let program_id = pubkey_n(10);

    // Caller has account as signer + writable
    let caller_account = create_account_info(account_key, true, true, 1000);
    let caller_accounts = vec![caller_account];

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

    let result = PrivilegeChecker::check_no_escalation(&caller_accounts, &instruction);
    assert!(result.is_ok());
}

#[test]
fn test_privilege_allowed_reduced_privileges() {
    let account_key = pubkey_n(1);
    let program_id = pubkey_n(10);

    // Caller has account as signer + writable
    let caller_account = create_account_info(account_key, true, true, 1000);
    let caller_accounts = vec![caller_account];

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

    let result = PrivilegeChecker::check_no_escalation(&caller_accounts, &instruction);
    assert!(result.is_ok());
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

    let ctx_a = CpiContext::new(program_a, &[], meter.clone(), &mut tracker);

    // Consume in parent
    assert!(ctx_a.compute_meter.consume(1000).is_ok());
    assert_eq!(ctx_a.compute_meter.consumed(), 1000);

    // Child shares same meter
    let child_b = ctx_a.child(program_b).unwrap();
    assert_eq!(child_b.compute_meter.consumed(), 1000);

    // Consume in child reflects in parent's meter
    assert!(child_b.compute_meter.consume(500).is_ok());
    assert_eq!(ctx_a.compute_meter.consumed(), 1500);
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
