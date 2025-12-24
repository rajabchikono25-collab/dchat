//! Context wrapper for DPL programs

use core::marker::PhantomData;

/// Execution context passed to instruction handlers
pub struct Context<'info, T> {
    /// Parsed and validated accounts
    pub accounts: T,
    /// Remaining accounts not parsed by the struct
    pub remaining_accounts: &'info [crate::account::AccountInfo<'info>],
    /// PDA bump seeds discovered during account parsing
    pub bumps: Bumps,
    /// Program ID
    pub program_id: &'info crate::account::Pubkey,
    _phantom: PhantomData<&'info ()>,
}

impl<'info, T> Context<'info, T> {
    /// Create a new context
    pub fn new(
        accounts: T,
        remaining_accounts: &'info [crate::account::AccountInfo<'info>],
        program_id: &'info crate::account::Pubkey,
    ) -> Self {
        Self {
            accounts,
            remaining_accounts,
            bumps: Bumps::default(),
            program_id,
            _phantom: PhantomData,
        }
    }
}

/// PDA bump seeds storage
#[derive(Debug, Default)]
pub struct Bumps {
    /// Storage for discovered bumps (name -> bump)
    /// In production, this is populated by #[derive(Accounts)]
    bumps: [u8; 16], // Support up to 16 PDAs per instruction
    count: usize,
}

impl Bumps {
    /// Get bump for an account by index
    pub fn get(&self, index: usize) -> Option<u8> {
        if index < self.count {
            Some(self.bumps[index])
        } else {
            None
        }
    }

    /// Set bump for an account
    pub fn set(&mut self, index: usize, bump: u8) {
        if index < 16 {
            self.bumps[index] = bump;
            if index >= self.count {
                self.count = index + 1;
            }
        }
    }
}
