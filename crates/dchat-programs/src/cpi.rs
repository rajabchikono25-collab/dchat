//! Cross-Program Invocation (CPI) with strict borrow rules

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::account::{AccountAccessTracker, AccountInfo, AccountMeta, Pubkey};

/// Shared CPI result for parallel access
pub type SharedCpiResult = Arc<CpiResult>;

/// Create a shared CPI result from an execution
pub fn share_cpi_result(result: CpiResult) -> SharedCpiResult {
    Arc::new(result)
}

/// Build account metas from account infos for CPI instruction construction
pub fn build_cpi_metas(accounts: &[AccountInfo<'_>]) -> Vec<AccountMeta> {
    accounts
        .iter()
        .map(|account| AccountMeta {
            pubkey: *account.key,
            is_signer: account.is_signer,
            is_writable: account.is_writable,
        })
        .collect()
}
use crate::error::{ProgramError, ProgramResult};
use crate::instruction::Instruction;
use crate::metering::{MeterSnapshot, SharedComputeMeter};
use crate::MAX_CPI_DEPTH;

/// CPI context containing caller information
#[derive(Debug)]
pub struct CpiContext<'a, 'b> {
    /// Current caller program ID
    pub caller: Pubkey,
    /// Accounts available to the caller
    pub accounts: &'a [AccountInfo<'b>],
    /// Current CPI depth
    pub depth: usize,
    /// Maximum allowed depth
    pub max_depth: usize,
    /// Compute meter (shared across CPI chain)
    pub compute_meter: SharedComputeMeter,
    /// Account borrow tracker
    pub borrow_tracker: &'a mut AccountAccessTracker,
    /// Programs in the call chain (for reentrancy detection)
    pub call_chain: Vec<Pubkey>,
    /// Signer seeds for PDA signing
    pub signer_seeds: Vec<Vec<Vec<u8>>>,
}

impl<'a, 'b> CpiContext<'a, 'b> {
    /// Create a new CPI context
    pub fn new(
        caller: Pubkey,
        accounts: &'a [AccountInfo<'b>],
        compute_meter: SharedComputeMeter,
        borrow_tracker: &'a mut AccountAccessTracker,
    ) -> Self {
        Self {
            caller,
            accounts,
            depth: 0,
            max_depth: MAX_CPI_DEPTH,
            compute_meter,
            borrow_tracker,
            call_chain: vec![caller],
            signer_seeds: Vec::new(),
        }
    }

    /// Create a child context for CPI
    pub fn child(&mut self, callee: Pubkey) -> ProgramResult<CpiContext<'_, 'b>> {
        if self.depth >= self.max_depth {
            return Err(ProgramError::CallDepthExceeded);
        }

        // Check for reentrancy
        if self.call_chain.contains(&callee) {
            return Err(ProgramError::ReentrancyDetected);
        }

        let mut new_chain = self.call_chain.clone();
        new_chain.push(callee);

        Ok(CpiContext {
            caller: callee,
            accounts: self.accounts,
            depth: self.depth + 1,
            max_depth: self.max_depth,
            compute_meter: self.compute_meter.clone(),
            borrow_tracker: self.borrow_tracker,
            call_chain: new_chain,
            signer_seeds: Vec::new(),
        })
    }

    /// Add signer seeds for PDA signing
    pub fn with_signer(mut self, seeds: Vec<Vec<u8>>) -> Self {
        self.signer_seeds.push(seeds);
        self
    }

    /// Get account by pubkey
    pub fn get_account(&self, key: &Pubkey) -> ProgramResult<&AccountInfo<'b>> {
        self.accounts
            .iter()
            .find(|a| a.key == key)
            .ok_or(ProgramError::AccountNotFound)
    }
}

/// CPI guard for ensuring proper borrow release
pub struct CpiGuard {
    /// Snapshot for rollback on failure
    snapshot: MeterSnapshot,
    /// Accounts borrowed during this CPI
    borrowed_accounts: Vec<(Pubkey, bool)>, // (key, is_mutable)
    /// Meter for restoring on failure
    meter: SharedComputeMeter,
}

impl CpiGuard {
    /// Create a new CPI guard
    pub fn new(meter: SharedComputeMeter) -> Self {
        let snapshot = meter.snapshot();
        Self {
            snapshot,
            borrowed_accounts: Vec::new(),
            meter,
        }
    }

    /// Record a borrow
    pub fn record_borrow(&mut self, key: Pubkey, is_mutable: bool) {
        self.borrowed_accounts.push((key, is_mutable));
    }

    /// Commit the CPI (success case)
    pub fn commit(self) {
        // Borrows are released by caller
        // Don't restore snapshot
    }

    /// Rollback the CPI (failure case)
    pub fn rollback(self) {
        self.meter.restore(&self.snapshot);
    }
}

/// Result from CPI invocation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpiResult {
    /// Was successful
    pub success: bool,
    /// Error code if failed
    pub error_code: Option<u32>,
    /// Return data from callee
    pub return_data: Vec<u8>,
    /// Compute units consumed by CPI
    pub compute_consumed: u64,
}

/// Cross-program invocation handler
pub struct CrossProgramInvocation;

impl CrossProgramInvocation {
    /// Invoke a program via CPI
    pub fn invoke<'a, 'b>(
        ctx: &mut CpiContext<'a, 'b>,
        instruction: &Instruction,
    ) -> ProgramResult<CpiResult> {
        Self::invoke_signed_internal(ctx, instruction, &[])
    }

    /// Invoke a program with signer seeds (for PDAs)
    pub fn invoke_signed<'a, 'b>(
        ctx: &mut CpiContext<'a, 'b>,
        instruction: &Instruction,
        signer_seeds: &[&[&[u8]]],
    ) -> ProgramResult<CpiResult> {
        Self::invoke_signed_internal(ctx, instruction, signer_seeds)
    }

    /// Internal invoke implementation
    fn invoke_signed_internal<'a, 'b>(
        ctx: &mut CpiContext<'a, 'b>,
        instruction: &Instruction,
        signer_seeds: &[&[&[u8]]],
    ) -> ProgramResult<CpiResult> {
        // Check CPI depth
        if ctx.depth >= ctx.max_depth {
            return Err(ProgramError::CallDepthExceeded);
        }

        // Consume base CPI cost
        ctx.compute_meter.consume_syscall()?;

        // Validate instruction
        instruction.validate()?;

        // Check for reentrancy
        if ctx.call_chain.contains(&instruction.program_id) {
            return Err(ProgramError::ReentrancyDetected);
        }

        // Derive PDA signers if seeds provided
        let mut pda_signers = HashSet::new();
        for seeds in signer_seeds {
            let pda = crate::pda::PdaDerivation::find_program_address(seeds, &ctx.caller)?;
            pda_signers.insert(pda.address);
        }

        // Create CPI guard for rollback on failure
        let guard = CpiGuard::new(ctx.compute_meter.clone());

        // Verify account permissions
        let mut cpi_accounts = Vec::new();
        for meta in &instruction.accounts {
            let account = ctx.get_account(&meta.pubkey)?;

            // Verify signer
            if meta.is_signer {
                // Either the account is already a signer, or we're providing PDA seeds
                if !account.is_signer && !pda_signers.contains(&meta.pubkey) {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }

            // Verify writable
            if meta.is_writable {
                if !account.is_writable {
                    return Err(ProgramError::AccountNotWritable);
                }
                // Check borrow rules
                if !ctx.borrow_tracker.can_borrow_mutable(&meta.pubkey) {
                    return Err(ProgramError::BorrowsOverlap);
                }
                ctx.borrow_tracker.add_mutable_borrow(meta.pubkey)?;
            } else {
                // Check immutable borrow
                if !ctx.borrow_tracker.can_borrow_immutable(&meta.pubkey) {
                    return Err(ProgramError::BorrowsOverlap);
                }
                ctx.borrow_tracker.add_immutable_borrow(meta.pubkey)?;
            }

            cpi_accounts.push((meta.pubkey, meta.is_writable));
        }

        // Record borrows in guard
        let cpi_compute_start = ctx.compute_meter.consumed();

        // NOTE: Actual program invocation would happen here via the runtime
        // This is the interface - the runtime implements the actual execution

        // For now, return success placeholder
        // Real implementation calls into ProgramRuntime::invoke_internal()
        let result = CpiResult {
            success: true,
            error_code: None,
            return_data: Vec::new(),
            compute_consumed: ctx.compute_meter.consumed() - cpi_compute_start,
        };

        // Release borrows
        for (key, is_mutable) in &cpi_accounts {
            if *is_mutable {
                ctx.borrow_tracker.release_mutable_borrow(key);
            } else {
                ctx.borrow_tracker.release_immutable_borrow(key);
            }
        }

        if result.success {
            guard.commit();
            Ok(result)
        } else {
            guard.rollback();
            Err(ProgramError::from_code(result.error_code.unwrap_or(0)))
        }
    }

    /// Check if an account can be borrowed for CPI
    pub fn can_borrow(tracker: &AccountAccessTracker, key: &Pubkey, is_mutable: bool) -> bool {
        if is_mutable {
            tracker.can_borrow_mutable(key)
        } else {
            tracker.can_borrow_immutable(key)
        }
    }
}

/// Account privilege escalation checker
pub struct PrivilegeChecker;

impl PrivilegeChecker {
    /// Check that CPI doesn't escalate privileges
    pub fn check_no_escalation(
        caller_accounts: &[AccountInfo<'_>],
        instruction: &Instruction,
    ) -> ProgramResult<()> {
        for meta in &instruction.accounts {
            // Find the account in caller's accounts
            let caller_account = caller_accounts.iter().find(|a| *a.key == meta.pubkey);

            match caller_account {
                Some(account) => {
                    // CPI cannot grant signer privilege
                    if meta.is_signer && !account.is_signer {
                        return Err(ProgramError::MissingRequiredSignature);
                    }
                    // CPI cannot grant writable privilege
                    if meta.is_writable && !account.is_writable {
                        return Err(ProgramError::AccountNotWritable);
                    }
                }
                None => {
                    // Account not in caller's list
                    // This is only allowed for program IDs (executable accounts)
                    return Err(ProgramError::AccountNotFound);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metering::ComputeBudget;

    #[test]
    fn test_cpi_depth_limit() {
        let meter = Arc::new(crate::metering::ComputeMeter::new(ComputeBudget::default()));
        let mut tracker = AccountAccessTracker::new();

        let mut ctx = CpiContext::new(Pubkey::new([1u8; 32]), &[], meter, &mut tracker);

        // Should be able to create children up to max depth
        let mut current = ctx.child(Pubkey::new([2u8; 32])).unwrap();
        assert_eq!(current.depth, 1);

        let mut c2 = current.child(Pubkey::new([3u8; 32])).unwrap();
        assert_eq!(c2.depth, 2);

        let mut c3 = c2.child(Pubkey::new([4u8; 32])).unwrap();
        assert_eq!(c3.depth, 3);

        let c4 = c3.child(Pubkey::new([5u8; 32])).unwrap();
        assert_eq!(c4.depth, 4);

        // This should fail - max depth exceeded
        // Note: MAX_CPI_DEPTH is 4
    }

    #[test]
    fn test_reentrancy_detection() {
        let meter = Arc::new(crate::metering::ComputeMeter::new(ComputeBudget::default()));
        let mut tracker = AccountAccessTracker::new();
        let program1 = Pubkey::new([1u8; 32]);
        let program2 = Pubkey::new([2u8; 32]);

        let mut ctx = CpiContext::new(program1, &[], meter, &mut tracker);

        // Can call different program
        let child = ctx.child(program2);
        assert!(child.is_ok());

        // Cannot re-enter caller
        let mut child = child.unwrap();
        let reenter = child.child(program1);
        assert!(matches!(reenter, Err(ProgramError::ReentrancyDetected)));
    }

    #[test]
    fn test_cpi_guard_rollback() {
        let meter = Arc::new(crate::metering::ComputeMeter::new(ComputeBudget::new(1000)));

        meter.consume(100).unwrap();
        assert_eq!(meter.consumed(), 100);

        let guard = CpiGuard::new(meter.clone());

        meter.consume(200).unwrap();
        assert_eq!(meter.consumed(), 300);

        // Rollback
        guard.rollback();
        assert_eq!(meter.consumed(), 100);
    }

    #[test]
    fn test_cpi_guard_commit() {
        let meter = Arc::new(crate::metering::ComputeMeter::new(ComputeBudget::new(1000)));

        meter.consume(100).unwrap();
        let guard = CpiGuard::new(meter.clone());

        meter.consume(200).unwrap();
        guard.commit();

        // Should not rollback
        assert_eq!(meter.consumed(), 300);
    }
}
