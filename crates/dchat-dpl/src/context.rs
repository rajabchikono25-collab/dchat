//! Context wrapper for DPL programs

use core::marker::PhantomData;

/// Trait for account structures that can be deserialized and validated
pub trait Accounts<'info>: Sized {
    /// The bumps structure type
    type Bumps: Default;

    /// Try to deserialize and validate accounts from raw data
    fn try_accounts(
        ctx: &ContextInfo,
        accounts_data: &[u8],
        bumps: &mut Self::Bumps,
    ) -> crate::error::DplResult<Self>;
}

/// Context info passed during account parsing
pub struct ContextInfo {
    /// Program ID
    pub program_id: crate::account::Pubkey,
}

impl ContextInfo {
    /// Create a new context info
    pub fn new(program_id: crate::account::Pubkey) -> Self {
        Self { program_id }
    }
}

/// Execution context passed to instruction handlers
pub struct Context<'info, T>
where
    T: Accounts<'info>,
{
    /// Parsed and validated accounts
    pub accounts: T,
    /// Remaining accounts not parsed by the struct
    pub remaining_accounts: &'info [crate::account::AccountInfo<'info>],
    /// PDA bump seeds discovered during account parsing
    pub bumps: T::Bumps,
    /// Program ID
    pub program_id: &'info crate::account::Pubkey,
    _phantom: PhantomData<&'info ()>,
}

impl<'info, T> Context<'info, T>
where
    T: Accounts<'info>,
{
    /// Create a new context
    pub fn new(
        accounts: T,
        remaining_accounts: &'info [crate::account::AccountInfo<'info>],
        bumps: T::Bumps,
        program_id: &'info crate::account::Pubkey,
    ) -> Self {
        Self {
            accounts,
            remaining_accounts,
            bumps,
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
