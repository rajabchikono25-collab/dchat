//! DPL Counter Contract
//!
//! This is a simple counter contract that demonstrates the DPL macro system.
//! It uses #[program], #[derive(Instruction)], #[derive(Accounts)], #[account],
//! #[event], and #[error_code] macros for clean, type-safe contract authoring.
//!
//! # Build
//! ```bash
//! cargo build --target wasm32-wasi --release
//! ```
//!
//! # Features Demonstrated
//! - Type-safe instruction routing with stable tags
//! - Account validation with constraints
//! - Event emission
//! - Custom error codes
//! - PDA derivation
//!
//! Compare to examples/contracts/counter for the legacy no_std approach.

use dchat_dpl::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAM DEFINITION
// ═══════════════════════════════════════════════════════════════════════════════

/// Counter program - manages simple u64 counters
#[program]
pub mod counter {
    use super::*;

    /// Initialize a new counter with a starting value
    pub fn initialize<'a>(mut ctx: Context<'a, Initialize<'a>>, initial_value: u64) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let authority_key = *ctx.accounts.authority.key();
        let counter = &mut ctx.accounts.counter;

        counter.value = initial_value;
        counter.authority = authority_key;
        counter.bump = ctx.bumps.counter;

        emit!(CounterInitialized {
            counter: counter_key,
            authority: authority_key,
            initial_value,
        });

        Ok(())
    }

    /// Increment the counter by 1
    pub fn increment<'a>(mut ctx: Context<'a, Increment<'a>>) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let counter = &mut ctx.accounts.counter;
        let old_value = counter.value;

        counter.value = counter.value.checked_add(1).ok_or(CounterError::Overflow)?;

        emit!(CounterChanged {
            counter: counter_key,
            old_value,
            new_value: counter.value,
        });

        Ok(())
    }

    /// Decrement the counter by 1
    pub fn decrement<'a>(mut ctx: Context<'a, Decrement<'a>>) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let counter = &mut ctx.accounts.counter;
        let old_value = counter.value;

        counter.value = counter
            .value
            .checked_sub(1)
            .ok_or(CounterError::Underflow)?;

        emit!(CounterChanged {
            counter: counter_key,
            old_value,
            new_value: counter.value,
        });

        Ok(())
    }

    /// Set the counter to a specific value (authority only)
    pub fn set<'a>(mut ctx: Context<'a, Set<'a>>, new_value: u64) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let counter = &mut ctx.accounts.counter;
        let old_value = counter.value;

        counter.value = new_value;

        emit!(CounterChanged {
            counter: counter_key,
            old_value,
            new_value,
        });

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Instructions for the counter program
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

    /// Set the counter to a specific value
    #[tag = 3]
    Set { new_value: u64 },
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT CONTEXTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Accounts for initializing a counter
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The counter account to initialize (PDA)
    #[account(
        init,
        space = Counter::SIZE,
        seeds = [b"counter", authority.key().as_ref()],
        bump
    )]
    pub counter: Account<'info, Counter>,

    /// The authority who owns this counter (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,

    /// System program for account creation
    pub system_program: Program<'info, System>,
}

/// Accounts for incrementing a counter
#[derive(Accounts)]
pub struct Increment<'info> {
    /// The counter account to increment
    #[account(mut)]
    pub counter: Account<'info, Counter>,
}

/// Accounts for decrementing a counter
#[derive(Accounts)]
pub struct Decrement<'info> {
    /// The counter account to decrement
    #[account(mut)]
    pub counter: Account<'info, Counter>,
}

/// Accounts for setting a counter value
#[derive(Accounts)]
pub struct Set<'info> {
    /// The counter account to modify
    #[account(
        mut,
        has_one = authority @ CounterError::Unauthorized
    )]
    pub counter: Account<'info, Counter>,

    /// The authority who owns this counter (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Counter state stored on-chain
#[account]
pub struct Counter {
    /// Current counter value
    pub value: u64,
    /// Authority who can set the value
    pub authority: Pubkey,
    /// PDA bump seed
    pub bump: u8,
}

impl Counter {
    /// Account size: 8 (header) + 8 (value) + 32 (authority) + 1 (bump) = 49 bytes
    pub const SIZE: usize = 8 + 8 + 32 + 1;
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Emitted when a counter is initialized
#[event]
pub struct CounterInitialized {
    /// Counter account address
    pub counter: Pubkey,
    /// Authority address
    pub authority: Pubkey,
    /// Initial value
    pub initial_value: u64,
}

/// Emitted when a counter value changes
#[event]
pub struct CounterChanged {
    /// Counter account address
    pub counter: Pubkey,
    /// Previous value
    pub old_value: u64,
    /// New value
    pub new_value: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERRORS
// ═══════════════════════════════════════════════════════════════════════════════

/// Custom error codes for the counter program
#[error_code]
pub enum CounterError {
    /// Counter overflow
    #[code = 6000]
    #[msg = "Counter overflow - maximum value reached"]
    Overflow,

    /// Counter underflow
    #[code = 6001]
    #[msg = "Counter underflow - cannot go below zero"]
    Underflow,

    /// Unauthorized action
    #[code = 6002]
    #[msg = "Unauthorized - only the authority can perform this action"]
    Unauthorized,
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_tags() {
        // Ensure tags are stable and don't change
        assert_eq!(CounterInstruction::TAG_INITIALIZE, 0);
        assert_eq!(CounterInstruction::TAG_INCREMENT, 1);
        assert_eq!(CounterInstruction::TAG_DECREMENT, 2);
        assert_eq!(CounterInstruction::TAG_SET, 3);
    }

    #[test]
    fn test_counter_size() {
        assert_eq!(Counter::SIZE, 49);
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(CounterError::Overflow.code(), 6000);
        assert_eq!(CounterError::Underflow.code(), 6001);
        assert_eq!(CounterError::Unauthorized.code(), 6002);
    }
}
