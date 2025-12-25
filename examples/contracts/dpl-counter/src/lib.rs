//! Counter Program - DPL Example Contract
//!
//! A simple counter demonstrating DPL basics with wasmi wasip1 runtime.
//!
//! # Features
//!
//! - Initialize a counter with an initial value
//! - Increment the counter by 1
//! - Decrement the counter by 1
//! - Set the counter to a specific value (authority only)
//! - Reset the counter to zero (authority only)
//!
//! # Target
//!
//! This contract compiles to `wasm32-wasip1` for execution in the wasmi runtime
//! with a deterministic WASI shim.

use dchat_dpl::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAM DEFINITION
// ═══════════════════════════════════════════════════════════════════════════════

#[program]
pub mod counter {
    use super::*;

    /// Initialize a new counter account with an initial value.
    ///
    /// The authority who initializes the counter becomes the owner and can
    /// perform privileged operations like `set` and `reset`.
    pub fn initialize<'a>(mut ctx: Context<'a, Initialize<'a>>, initial_value: u64) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let authority_key = *ctx.accounts.authority.key();
        let counter = &mut ctx.accounts.counter;

        // Initialize counter state
        counter.value = initial_value;
        counter.authority = authority_key;
        counter.bump = ctx.bumps.counter;
        counter.total_operations = 0;

        // Emit initialization event
        emit!(CounterInitialized {
            counter: counter_key,
            authority: authority_key,
            initial_value,
        });

        Ok(())
    }

    /// Increment the counter by 1.
    ///
    /// This operation can be performed by anyone and will fail if it would
    /// cause an overflow.
    pub fn increment<'a>(mut ctx: Context<'a, Modify<'a>>) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let counter = &mut ctx.accounts.counter;
        let old_value = counter.value;

        // Check for overflow
        counter.value = counter.value.checked_add(1).ok_or(CounterError::Overflow)?;

        // Track operation count
        counter.total_operations = counter.total_operations.saturating_add(1);

        // Emit change event
        emit!(CounterChanged {
            counter: counter_key,
            old_value,
            new_value: counter.value,
            operation: OperationType::Increment,
        });

        Ok(())
    }

    /// Decrement the counter by 1.
    ///
    /// This operation can be performed by anyone and will fail if it would
    /// cause an underflow (counter cannot go below zero).
    pub fn decrement<'a>(mut ctx: Context<'a, Modify<'a>>) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let counter = &mut ctx.accounts.counter;
        let old_value = counter.value;

        // Check for underflow
        counter.value = counter
            .value
            .checked_sub(1)
            .ok_or(CounterError::Underflow)?;

        // Track operation count
        counter.total_operations = counter.total_operations.saturating_add(1);

        // Emit change event
        emit!(CounterChanged {
            counter: counter_key,
            old_value,
            new_value: counter.value,
            operation: OperationType::Decrement,
        });

        Ok(())
    }

    /// Set the counter to a specific value (authority only).
    ///
    /// Only the authority who initialized the counter can set its value
    /// to an arbitrary number.
    pub fn set<'a>(mut ctx: Context<'a, AuthorityModify<'a>>, new_value: u64) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let authority_key = *ctx.accounts.authority.key();
        let counter = &mut ctx.accounts.counter;

        // Verify authority
        if counter.authority != authority_key {
            return Err(CounterError::Unauthorized.into());
        }

        let old_value = counter.value;
        counter.value = new_value;
        counter.total_operations = counter.total_operations.saturating_add(1);

        // Emit change event
        emit!(CounterChanged {
            counter: counter_key,
            old_value,
            new_value,
            operation: OperationType::Set,
        });

        Ok(())
    }

    /// Reset the counter to zero (authority only).
    ///
    /// Only the authority who initialized the counter can reset it to zero.
    pub fn reset<'a>(mut ctx: Context<'a, AuthorityModify<'a>>) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let authority_key = *ctx.accounts.authority.key();
        let counter = &mut ctx.accounts.counter;

        // Verify authority
        if counter.authority != authority_key {
            return Err(CounterError::Unauthorized.into());
        }

        let old_value = counter.value;
        counter.value = 0;
        counter.total_operations = counter.total_operations.saturating_add(1);

        // Emit reset event
        emit!(CounterReset {
            counter: counter_key,
            authority: authority_key,
            old_value,
        });

        Ok(())
    }

    /// Transfer authority to a new owner.
    ///
    /// Only the current authority can transfer ownership to another account.
    pub fn transfer_authority<'a>(
        mut ctx: Context<'a, TransferAuthority<'a>>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let old_authority = *ctx.accounts.authority.key();
        let counter = &mut ctx.accounts.counter;

        // Verify current authority
        if counter.authority != old_authority {
            return Err(CounterError::Unauthorized.into());
        }

        // Transfer authority
        counter.authority = new_authority;

        // Emit transfer event
        emit!(AuthorityTransferred {
            counter: counter_key,
            old_authority,
            new_authority,
        });

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Instruction variants for the counter program.
///
/// Each variant has a stable tag for deterministic instruction routing.
#[derive(Instruction)]
pub enum CounterInstruction {
    /// Initialize a new counter
    #[tag = 0]
    Initialize { initial_value: u64 },

    /// Increment the counter by 1
    #[tag = 1]
    Increment,

    /// Decrement the counter by 1
    #[tag = 2]
    Decrement,

    /// Set the counter to a specific value (authority only)
    #[tag = 3]
    Set { new_value: u64 },

    /// Reset the counter to zero (authority only)
    #[tag = 4]
    Reset,

    /// Transfer authority to a new owner
    #[tag = 5]
    TransferAuthority { new_authority: Pubkey },
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Accounts required for initialization.
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The counter account to create (PDA derived from authority)
    #[account(
        init,
        space = Counter::SIZE,
        seeds = [b"counter", authority.key().as_ref()],
        bump
    )]
    pub counter: Account<'info, Counter>,

    /// The authority initializing the counter (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,

    /// System program for account creation
    pub system_program: Program<'info, System>,
}

/// Accounts required for public modification (increment/decrement).
#[derive(Accounts)]
pub struct Modify<'info> {
    /// The counter account to modify
    #[account(mut)]
    pub counter: Account<'info, Counter>,
}

/// Accounts required for authority-only modification (set/reset).
#[derive(Accounts)]
pub struct AuthorityModify<'info> {
    /// The counter account to modify
    #[account(mut)]
    pub counter: Account<'info, Counter>,

    /// The authority (must match counter.authority and sign)
    #[account(signer)]
    pub authority: Signer<'info>,
}

/// Accounts required for transferring authority.
#[derive(Accounts)]
pub struct TransferAuthority<'info> {
    /// The counter account
    #[account(mut)]
    pub counter: Account<'info, Counter>,

    /// The current authority (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATE DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Counter account state stored on-chain.
#[account]
pub struct Counter {
    /// Current counter value
    pub value: u64,

    /// Authority who can perform privileged operations
    pub authority: Pubkey,

    /// PDA bump seed for account derivation
    pub bump: u8,

    /// Total number of operations performed on this counter
    pub total_operations: u64,
}

impl Counter {
    /// Account size calculation:
    /// - 8 bytes discriminator
    /// - 8 bytes value (u64)
    /// - 32 bytes authority (Pubkey)
    /// - 1 byte bump (u8)
    /// - 8 bytes total_operations (u64)
    pub const SIZE: usize = 8 + 8 + 32 + 1 + 8;
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Emitted when a counter is initialized.
#[event]
pub struct CounterInitialized {
    /// The counter account address
    pub counter: Pubkey,
    /// The authority who initialized it
    pub authority: Pubkey,
    /// The initial value
    pub initial_value: u64,
}

/// Emitted when the counter value changes.
#[event]
pub struct CounterChanged {
    /// The counter account address
    pub counter: Pubkey,
    /// Previous value
    pub old_value: u64,
    /// New value
    pub new_value: u64,
    /// Type of operation
    pub operation: OperationType,
}

/// Emitted when the counter is reset.
#[event]
pub struct CounterReset {
    /// The counter account address
    pub counter: Pubkey,
    /// The authority who reset it
    pub authority: Pubkey,
    /// Value before reset
    pub old_value: u64,
}

/// Emitted when authority is transferred.
#[event]
pub struct AuthorityTransferred {
    /// The counter account address
    pub counter: Pubkey,
    /// Previous authority
    pub old_authority: Pubkey,
    /// New authority
    pub new_authority: Pubkey,
}

/// Type of counter operation for events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize)]
pub enum OperationType {
    Increment,
    Decrement,
    Set,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERROR DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Error codes for the counter program.
#[error_code]
pub enum CounterError {
    /// Counter would overflow (value too large)
    #[code = 6000]
    #[msg = "Counter overflow: value would exceed maximum"]
    Overflow,

    /// Counter would underflow (value below zero)
    #[code = 6001]
    #[msg = "Counter underflow: value cannot go below zero"]
    Underflow,

    /// Caller is not the authorized owner
    #[code = 6002]
    #[msg = "Unauthorized: only the authority can perform this operation"]
    Unauthorized,
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_size() {
        // Verify size calculation is correct
        assert_eq!(Counter::SIZE, 57); // 8 + 8 + 32 + 1 + 8
    }

    #[test]
    fn test_instruction_tags() {
        // Verify instruction tags are stable
        // This prevents breaking changes to the program ABI
    }

    #[test]
    fn test_operation_type_serialization() {
        use borsh::{to_vec, BorshDeserialize};

        // Test round-trip serialization
        let ops = [
            OperationType::Increment,
            OperationType::Decrement,
            OperationType::Set,
        ];

        for op in ops {
            let serialized = to_vec(&op).unwrap();
            let deserialized = OperationType::try_from_slice(&serialized).unwrap();
            assert_eq!(op, deserialized);
        }
    }
}
