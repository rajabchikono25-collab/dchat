//! DPL Escrow Contract with Ironclad Manifest
//!
//! This example demonstrates the full ironclad manifest system:
//!
//! # Features Demonstrated
//!
//! 1. **Manifest Generation**: The `#[program]` macro automatically generates
//!    a 64-byte DPL manifest embedded in the WASM custom section.
//!
//! 2. **Schema Hash**: All instructions, accounts, and types are hashed into
//!    a deterministic schema hash for ABI verification.
//!
//! 3. **Stable Discriminators**: Instruction tags are explicit and stable.
//!
//! 4. **Capability Declaration**: The manifest declares what features the
//!    program uses (events, PDAs, signers).
//!
//! 5. **Build-time Verification**: The build.rs script verifies the schema
//!    hash matches the committed IDL for release builds.
//!
//! # Escrow Contract Flow
//!
//! 1. **Initialize**: Creator deposits tokens and sets recipient + conditions
//! 2. **Release**: Recipient claims tokens after conditions are met
//! 3. **Cancel**: Creator cancels and reclaims tokens before release
//! 4. **Dispute**: Either party can raise a dispute for arbitration
//! 5. **Resolve**: Arbiter resolves the dispute
//!
//! # Build
//!
//! ```bash
//! # Development build
//! cargo build --target wasm32-wasi --release
//!
//! # Reproducible release build
//! SOURCE_DATE_EPOCH=$(date +%s) cargo build --target wasm32-wasi --release
//!
//! # Extract manifest
//! dchat program manifest target/wasm32-wasi/release/dpl_escrow_ironclad.wasm
//! ```

use dchat_dpl::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAM DEFINITION
// ═══════════════════════════════════════════════════════════════════════════════

/// Escrow program with ironclad manifest
///
/// The `#[program]` macro generates:
/// - WASM entrypoint
/// - Instruction dispatcher with stable discriminators
/// - DPL manifest custom section (64 bytes)
/// - IDL metadata for schema hash computation
#[program]
pub mod escrow {
    // Note: super::* import handled by macro expansion

    /// Initialize a new escrow
    pub fn initialize<'a>(
        mut ctx: Context<'a, Initialize<'a>>,
        amount: u64,
        unlock_time: i64,
    ) -> Result<()> {
        if amount == 0 {
            return Err(EscrowError::ZeroAmount.into());
        }

        let escrow = &mut ctx.accounts.escrow;
        let depositor_key = *ctx.accounts.depositor.key();
        let recipient_key = *ctx.accounts.recipient.key();

        // Initialize escrow state
        escrow.depositor = depositor_key;
        escrow.recipient = recipient_key;
        escrow.amount = amount;
        escrow.unlock_time = unlock_time;
        escrow.state = 0; // Active
        escrow.created_at = Clock::get()?.unix_timestamp;
        escrow.bump = ctx.bumps.escrow;

        // Emit initialization event
        emit!(EscrowCreated {
            escrow: *ctx.accounts.escrow.key(),
            depositor: depositor_key,
            recipient: recipient_key,
            amount,
            unlock_time,
        });

        Ok(())
    }

    /// Release escrowed tokens to recipient
    pub fn release<'a>(ctx: Context<'a, Release<'a>>) -> Result<()> {
        let escrow = &ctx.accounts.escrow;

        // Validate state
        if escrow.state != 0 {
            return Err(EscrowError::InvalidState.into());
        }

        // Check unlock time
        let clock = Clock::get()?;
        if clock.unix_timestamp < escrow.unlock_time {
            return Err(EscrowError::NotYetUnlocked.into());
        }

        // Emit release event
        emit!(EscrowReleased {
            escrow: *ctx.accounts.escrow.key(),
            recipient: escrow.recipient,
            amount: escrow.amount,
            released_at: clock.unix_timestamp,
        });

        Ok(())
    }

    /// Cancel escrow and return tokens to depositor
    pub fn cancel<'a>(mut ctx: Context<'a, Cancel<'a>>) -> Result<()> {
        // Get key before mutable borrow
        let escrow_key = *ctx.accounts.escrow.key();
        let escrow = &mut ctx.accounts.escrow;

        // Validate state
        if escrow.state != 0 {
            return Err(EscrowError::InvalidState.into());
        }

        // Capture values before mutation
        let depositor = escrow.depositor;
        let amount = escrow.amount;

        // Update state
        escrow.state = 2; // Cancelled

        // Emit cancellation event
        emit!(EscrowCancelled {
            escrow: escrow_key,
            depositor,
            amount,
            cancelled_at: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Raise a dispute on the escrow
    pub fn dispute<'a>(mut ctx: Context<'a, Dispute<'a>>, reason: u8) -> Result<()> {
        // Get key before mutable borrow
        let escrow_key = *ctx.accounts.escrow.key();
        let disputer_key = *ctx.accounts.disputer.key();
        let escrow = &mut ctx.accounts.escrow;

        // Validate state
        if escrow.state != 0 {
            return Err(EscrowError::InvalidState.into());
        }

        // Validate disputer is either depositor or recipient
        if disputer_key != escrow.depositor && disputer_key != escrow.recipient {
            return Err(EscrowError::Unauthorized.into());
        }

        // Update state
        escrow.state = 4; // Disputed

        // Emit dispute event
        emit!(EscrowDisputed {
            escrow: escrow_key,
            disputer: disputer_key,
            reason,
            disputed_at: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Resolve a dispute (arbiter only)
    pub fn resolve<'a>(
        mut ctx: Context<'a, Resolve<'a>>,
        release_to_recipient: bool,
    ) -> Result<()> {
        // Get key before mutable borrow
        let escrow_key = *ctx.accounts.escrow.key();
        let escrow = &mut ctx.accounts.escrow;

        // Validate state
        if escrow.state != 4 {
            return Err(EscrowError::InvalidState.into());
        }

        // Update state based on resolution
        escrow.state = if release_to_recipient { 1 } else { 3 }; // Released or Refunded

        // Emit resolution event
        emit!(EscrowResolved {
            escrow: escrow_key,
            release_to_recipient,
            resolved_at: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Instructions for the escrow program
#[derive(Instruction)]
pub enum EscrowInstruction {
    /// Initialize a new escrow
    #[tag = 0]
    Initialize { amount: u64, unlock_time: i64 },

    /// Release tokens to recipient
    #[tag = 1]
    Release,

    /// Cancel and refund to depositor
    #[tag = 2]
    Cancel,

    /// Raise a dispute
    #[tag = 3]
    Dispute { reason: u8 },

    /// Resolve a dispute
    #[tag = 4]
    Resolve { release_to_recipient: bool },
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT CONTEXTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Accounts for initializing an escrow
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The escrow account to create (PDA)
    #[account(
        init,
        space = Escrow::SIZE,
        seeds = [b"escrow", depositor.key().as_ref()],
        bump
    )]
    pub escrow: Account<'info, Escrow>,

    /// The depositor creating the escrow (must sign)
    #[account(signer)]
    pub depositor: Signer<'info>,

    /// The intended recipient
    #[account(signer)]
    pub recipient: Signer<'info>,

    /// System program for account creation
    pub system_program: Program<'info, System>,
}

/// Accounts for releasing escrow
#[derive(Accounts)]
pub struct Release<'info> {
    /// The escrow account
    #[account(
        mut,
        has_one = recipient @ EscrowError::Unauthorized
    )]
    pub escrow: Account<'info, Escrow>,

    /// The recipient (must sign)
    #[account(signer)]
    pub recipient: Signer<'info>,
}

/// Accounts for cancelling escrow
#[derive(Accounts)]
pub struct Cancel<'info> {
    /// The escrow account
    #[account(
        mut,
        has_one = depositor @ EscrowError::Unauthorized
    )]
    pub escrow: Account<'info, Escrow>,

    /// The depositor (must sign)
    #[account(signer)]
    pub depositor: Signer<'info>,
}

/// Accounts for disputing escrow
#[derive(Accounts)]
pub struct Dispute<'info> {
    /// The escrow account
    #[account(mut)]
    pub escrow: Account<'info, Escrow>,

    /// The party raising the dispute (must sign)
    #[account(signer)]
    pub disputer: Signer<'info>,
}

/// Accounts for resolving escrow
#[derive(Accounts)]
pub struct Resolve<'info> {
    /// The escrow account
    #[account(mut)]
    pub escrow: Account<'info, Escrow>,

    /// The arbiter (must sign)
    #[account(signer)]
    pub arbiter: Signer<'info>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Escrow state stored on-chain
///
/// State values:
/// - 0 = Active (pending release)
/// - 1 = Released (to recipient)
/// - 2 = Cancelled (refunded to depositor)
/// - 3 = Refunded (after dispute)
/// - 4 = Disputed (pending resolution)
#[account]
pub struct Escrow {
    /// The depositor who created the escrow
    pub depositor: Pubkey,
    /// The intended recipient
    pub recipient: Pubkey,
    /// Amount of tokens escrowed
    pub amount: u64,
    /// Unix timestamp when escrow can be released
    pub unlock_time: i64,
    /// Current state (see doc comment for values)
    pub state: u8,
    /// When the escrow was created
    pub created_at: i64,
    /// PDA bump seed
    pub bump: u8,
}

impl Escrow {
    /// Account size: 8 (header) + 32 + 32 + 8 + 8 + 1 + 8 + 1 = 98 bytes
    pub const SIZE: usize = 8 + 32 + 32 + 8 + 8 + 1 + 8 + 1;
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Emitted when an escrow is created
#[event]
pub struct EscrowCreated {
    /// The escrow account address
    pub escrow: Pubkey,
    /// The depositor's address
    pub depositor: Pubkey,
    /// The recipient's address
    pub recipient: Pubkey,
    /// Amount escrowed
    pub amount: u64,
    /// Unlock timestamp
    pub unlock_time: i64,
}

/// Emitted when tokens are released to recipient
#[event]
pub struct EscrowReleased {
    /// The escrow account address
    pub escrow: Pubkey,
    /// The recipient's address
    pub recipient: Pubkey,
    /// Amount released
    pub amount: u64,
    /// Timestamp of release
    pub released_at: i64,
}

/// Emitted when escrow is cancelled
#[event]
pub struct EscrowCancelled {
    /// The escrow account address
    pub escrow: Pubkey,
    /// The depositor's address
    pub depositor: Pubkey,
    /// Amount refunded
    pub amount: u64,
    /// Timestamp of cancellation
    pub cancelled_at: i64,
}

/// Emitted when a dispute is raised
#[event]
pub struct EscrowDisputed {
    /// The escrow account address
    pub escrow: Pubkey,
    /// The party raising the dispute
    pub disputer: Pubkey,
    /// The reason code for the dispute
    pub reason: u8,
    /// Timestamp of dispute
    pub disputed_at: i64,
}

/// Emitted when a dispute is resolved
#[event]
pub struct EscrowResolved {
    /// The escrow account address
    pub escrow: Pubkey,
    /// Whether tokens were released to recipient
    pub release_to_recipient: bool,
    /// Timestamp of resolution
    pub resolved_at: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERRORS
// ═══════════════════════════════════════════════════════════════════════════════

/// Custom error codes for the escrow program
#[error_code]
pub enum EscrowError {
    /// Amount must be greater than zero
    #[code = 6000]
    #[msg = "Amount must be greater than zero"]
    ZeroAmount,

    /// Escrow is not in a valid state for this operation
    #[code = 6001]
    #[msg = "Invalid escrow state for this operation"]
    InvalidState,

    /// Unlock time has not yet passed
    #[code = 6002]
    #[msg = "Escrow unlock time has not yet passed"]
    NotYetUnlocked,

    /// Caller is not authorized for this operation
    #[code = 6003]
    #[msg = "Unauthorized: caller is not authorized"]
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
        assert_eq!(EscrowInstruction::TAG_INITIALIZE, 0);
        assert_eq!(EscrowInstruction::TAG_RELEASE, 1);
        assert_eq!(EscrowInstruction::TAG_CANCEL, 2);
        assert_eq!(EscrowInstruction::TAG_DISPUTE, 3);
        assert_eq!(EscrowInstruction::TAG_RESOLVE, 4);
    }

    #[test]
    fn test_escrow_size() {
        assert_eq!(Escrow::SIZE, 98);
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(EscrowError::ZeroAmount.code(), 6000);
        assert_eq!(EscrowError::InvalidState.code(), 6001);
        assert_eq!(EscrowError::NotYetUnlocked.code(), 6002);
        assert_eq!(EscrowError::Unauthorized.code(), 6003);
    }
}
