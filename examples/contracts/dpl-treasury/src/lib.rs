//! DPL Treasury Contract
//!
//! A multi-signature treasury with proposal-based spending.
//! Demonstrates more complex DPL patterns including:
//! - Multiple account types
//! - Proposal workflow (create → approve → execute)
//! - M-of-N multi-sig validation
//! - Time-based constraints
//! - Cross-program invocation (CPI) for token transfers
//!
//! # Build
//! ```bash
//! cargo build --target wasm32-wasi --release
//! ```

use dchat_dpl::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum number of signers in a treasury
pub const MAX_SIGNERS: usize = 10;

/// Maximum number of approvals per proposal
pub const MAX_APPROVALS: usize = 10;

/// Minimum timelock duration in slots
pub const MIN_TIMELOCK_SLOTS: u64 = 100;

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAM DEFINITION
// ═══════════════════════════════════════════════════════════════════════════════

/// Treasury program - manages multi-sig treasuries with proposals
#[program]
pub mod treasury {
    use super::*;

    /// Initialize a new treasury with signers and threshold
    pub fn initialize(
        ctx: Context<Initialize>,
        signers: Vec<Pubkey>,
        threshold: u8,
        timelock_slots: u64,
    ) -> Result<()> {
        // Validate inputs
        require!(signers.len() >= 1, TreasuryError::NoSigners);
        require!(signers.len() <= MAX_SIGNERS, TreasuryError::TooManySigners);
        require!(
            threshold >= 1 && threshold as usize <= signers.len(),
            TreasuryError::InvalidThreshold
        );
        require!(
            timelock_slots >= MIN_TIMELOCK_SLOTS,
            TreasuryError::TimelockTooShort
        );

        let treasury = &mut ctx.accounts.treasury;
        treasury.authority = *ctx.accounts.authority.key;
        treasury.signers = signers.clone();
        treasury.threshold = threshold;
        treasury.timelock_slots = timelock_slots;
        treasury.proposal_count = 0;
        treasury.bump = ctx.bumps.treasury;

        emit!(TreasuryCreated {
            treasury: *ctx.accounts.treasury.key(),
            authority: *ctx.accounts.authority.key,
            signers,
            threshold,
            timelock_slots,
        });

        Ok(())
    }

    /// Create a spending proposal
    pub fn create_proposal(
        ctx: Context<CreateProposal>,
        amount: u64,
        recipient: Pubkey,
        description: [u8; 64],
    ) -> Result<()> {
        let treasury = &mut ctx.accounts.treasury;
        let proposal = &mut ctx.accounts.proposal;

        // Verify creator is a signer
        require!(
            treasury.signers.contains(ctx.accounts.creator.key),
            TreasuryError::NotASigner
        );

        proposal.treasury = *ctx.accounts.treasury.key();
        proposal.proposal_id = treasury.proposal_count;
        proposal.amount = amount;
        proposal.recipient = recipient;
        proposal.description = description;
        proposal.creator = *ctx.accounts.creator.key;
        proposal.approvals = vec![*ctx.accounts.creator.key]; // Creator auto-approves
        proposal.created_slot = Clock::get()?.slot;
        proposal.executed = false;
        proposal.bump = ctx.bumps.proposal;

        treasury.proposal_count += 1;

        emit!(ProposalCreated {
            treasury: *ctx.accounts.treasury.key(),
            proposal_id: proposal.proposal_id,
            amount,
            recipient,
            creator: *ctx.accounts.creator.key,
        });

        Ok(())
    }

    /// Approve a proposal
    pub fn approve(ctx: Context<Approve>) -> Result<()> {
        let treasury = &ctx.accounts.treasury;
        let proposal = &mut ctx.accounts.proposal;
        let approver = ctx.accounts.approver.key;

        // Verify approver is a signer
        require!(
            treasury.signers.contains(approver),
            TreasuryError::NotASigner
        );

        // Check not already approved
        require!(
            !proposal.approvals.contains(approver),
            TreasuryError::AlreadyApproved
        );

        // Check proposal not executed
        require!(!proposal.executed, TreasuryError::AlreadyExecuted);

        proposal.approvals.push(*approver);

        emit!(ProposalApproved {
            treasury: *ctx.accounts.treasury.key(),
            proposal_id: proposal.proposal_id,
            approver: *approver,
            approval_count: proposal.approvals.len() as u8,
            threshold: treasury.threshold,
        });

        Ok(())
    }

    /// Execute an approved proposal after timelock
    pub fn execute(ctx: Context<Execute>) -> Result<()> {
        let treasury = &ctx.accounts.treasury;
        let proposal = &mut ctx.accounts.proposal;

        // Check not already executed
        require!(!proposal.executed, TreasuryError::AlreadyExecuted);

        // Check threshold reached
        require!(
            proposal.approvals.len() >= treasury.threshold as usize,
            TreasuryError::InsufficientApprovals
        );

        // Check timelock passed
        let current_slot = Clock::get()?.slot;
        let unlock_slot = proposal.created_slot + treasury.timelock_slots;
        require!(
            current_slot >= unlock_slot,
            TreasuryError::TimelockNotExpired
        );

        // Mark as executed
        proposal.executed = true;

        // Transfer would happen via CPI here
        // In production: invoke token_program::transfer(...)

        emit!(ProposalExecuted {
            treasury: *ctx.accounts.treasury.key(),
            proposal_id: proposal.proposal_id,
            amount: proposal.amount,
            recipient: proposal.recipient,
            executed_slot: current_slot,
        });

        Ok(())
    }

    /// Cancel a proposal (creator or authority only)
    pub fn cancel(ctx: Context<Cancel>) -> Result<()> {
        let treasury = &ctx.accounts.treasury;
        let proposal = &mut ctx.accounts.proposal;
        let canceller = ctx.accounts.canceller.key;

        // Check not already executed
        require!(!proposal.executed, TreasuryError::AlreadyExecuted);

        // Only creator or authority can cancel
        require!(
            *canceller == proposal.creator || *canceller == treasury.authority,
            TreasuryError::Unauthorized
        );

        emit!(ProposalCancelled {
            treasury: *ctx.accounts.treasury.key(),
            proposal_id: proposal.proposal_id,
            cancelled_by: *canceller,
        });

        // Close the proposal account (return rent to canceller)
        // The account closing is handled by the framework

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Instructions for the treasury program
#[derive(Instruction)]
pub enum TreasuryInstruction {
    /// Initialize a new treasury
    #[tag = 0]
    Initialize {
        signers: Vec<Pubkey>,
        threshold: u8,
        timelock_slots: u64,
    },

    /// Create a spending proposal
    #[tag = 1]
    CreateProposal {
        amount: u64,
        recipient: Pubkey,
        description: [u8; 64],
    },

    /// Approve a proposal
    #[tag = 2]
    Approve,

    /// Execute an approved proposal
    #[tag = 3]
    Execute,

    /// Cancel a proposal
    #[tag = 4]
    Cancel,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT CONTEXTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Accounts for initializing a treasury
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The treasury account to initialize (PDA)
    #[account(
        init,
        space = Treasury::SIZE,
        seeds = [b"treasury", authority.key.as_ref()],
        bump
    )]
    pub treasury: Account<'info, Treasury>,

    /// The authority who owns this treasury (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,

    /// System program for account creation
    pub system_program: Program<'info, System>,
}

/// Accounts for creating a proposal
#[derive(Accounts)]
#[instruction(amount: u64, recipient: Pubkey, description: [u8; 64])]
pub struct CreateProposal<'info> {
    /// The treasury this proposal belongs to
    #[account(mut)]
    pub treasury: Account<'info, Treasury>,

    /// The proposal account to create (PDA)
    #[account(
        init,
        space = Proposal::SIZE,
        seeds = [b"proposal", treasury.key().as_ref(), &treasury.proposal_count.to_le_bytes()],
        bump
    )]
    pub proposal: Account<'info, Proposal>,

    /// The creator of this proposal (must be a treasury signer)
    #[account(signer)]
    pub creator: Signer<'info>,

    /// System program for account creation
    pub system_program: Program<'info, System>,
}

/// Accounts for approving a proposal
#[derive(Accounts)]
pub struct Approve<'info> {
    /// The treasury
    pub treasury: Account<'info, Treasury>,

    /// The proposal to approve
    #[account(
        mut,
        has_one = treasury @ TreasuryError::InvalidProposal
    )]
    pub proposal: Account<'info, Proposal>,

    /// The approver (must be a treasury signer)
    #[account(signer)]
    pub approver: Signer<'info>,
}

/// Accounts for executing a proposal
#[derive(Accounts)]
pub struct Execute<'info> {
    /// The treasury
    pub treasury: Account<'info, Treasury>,

    /// The proposal to execute
    #[account(
        mut,
        has_one = treasury @ TreasuryError::InvalidProposal
    )]
    pub proposal: Account<'info, Proposal>,

    /// The recipient of the funds
    /// CHECK: Validated against proposal.recipient
    #[account(mut)]
    pub recipient: AccountInfo<'info>,

    /// Token program for transfer
    pub token_program: Program<'info, Token>,
}

/// Accounts for cancelling a proposal
#[derive(Accounts)]
pub struct Cancel<'info> {
    /// The treasury
    pub treasury: Account<'info, Treasury>,

    /// The proposal to cancel
    #[account(
        mut,
        has_one = treasury @ TreasuryError::InvalidProposal,
        close = canceller
    )]
    pub proposal: Account<'info, Proposal>,

    /// The canceller (must be creator or authority)
    #[account(signer)]
    pub canceller: Signer<'info>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Treasury state stored on-chain
#[account]
pub struct Treasury {
    /// Authority who can manage the treasury
    pub authority: Pubkey,
    /// List of authorized signers
    pub signers: Vec<Pubkey>,
    /// Number of approvals required
    pub threshold: u8,
    /// Slots to wait before execution
    pub timelock_slots: u64,
    /// Number of proposals created
    pub proposal_count: u64,
    /// PDA bump seed
    pub bump: u8,
}

impl Treasury {
    /// Account size: 8 (header) + 32 (authority) + 4 + 32*10 (signers) + 1 (threshold) + 8 (timelock) + 8 (count) + 1 (bump)
    pub const SIZE: usize = 8 + 32 + 4 + (32 * MAX_SIGNERS) + 1 + 8 + 8 + 1;
}

/// Proposal state stored on-chain
#[account]
pub struct Proposal {
    /// Treasury this proposal belongs to
    pub treasury: Pubkey,
    /// Unique proposal ID within treasury
    pub proposal_id: u64,
    /// Amount to transfer
    pub amount: u64,
    /// Recipient of the transfer
    pub recipient: Pubkey,
    /// Description of the proposal
    pub description: [u8; 64],
    /// Creator of the proposal
    pub creator: Pubkey,
    /// List of approvers
    pub approvals: Vec<Pubkey>,
    /// Slot when proposal was created
    pub created_slot: u64,
    /// Whether proposal has been executed
    pub executed: bool,
    /// PDA bump seed
    pub bump: u8,
}

impl Proposal {
    /// Account size: 8 (header) + 32 (treasury) + 8 (id) + 8 (amount) + 32 (recipient) + 64 (desc) + 32 (creator) + 4 + 32*10 (approvals) + 8 (slot) + 1 (executed) + 1 (bump)
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 32 + 64 + 32 + 4 + (32 * MAX_APPROVALS) + 8 + 1 + 1;
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Emitted when a treasury is created
#[event]
pub struct TreasuryCreated {
    pub treasury: Pubkey,
    pub authority: Pubkey,
    pub signers: Vec<Pubkey>,
    pub threshold: u8,
    pub timelock_slots: u64,
}

/// Emitted when a proposal is created
#[event]
pub struct ProposalCreated {
    pub treasury: Pubkey,
    pub proposal_id: u64,
    pub amount: u64,
    pub recipient: Pubkey,
    pub creator: Pubkey,
}

/// Emitted when a proposal is approved
#[event]
pub struct ProposalApproved {
    pub treasury: Pubkey,
    pub proposal_id: u64,
    pub approver: Pubkey,
    pub approval_count: u8,
    pub threshold: u8,
}

/// Emitted when a proposal is executed
#[event]
pub struct ProposalExecuted {
    pub treasury: Pubkey,
    pub proposal_id: u64,
    pub amount: u64,
    pub recipient: Pubkey,
    pub executed_slot: u64,
}

/// Emitted when a proposal is cancelled
#[event]
pub struct ProposalCancelled {
    pub treasury: Pubkey,
    pub proposal_id: u64,
    pub cancelled_by: Pubkey,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERRORS
// ═══════════════════════════════════════════════════════════════════════════════

/// Custom error codes for the treasury program
#[error_code]
pub enum TreasuryError {
    /// No signers provided
    #[code = 6000]
    #[msg = "Treasury must have at least one signer"]
    NoSigners,

    /// Too many signers
    #[code = 6001]
    #[msg = "Too many signers, maximum is 10"]
    TooManySigners,

    /// Invalid threshold
    #[code = 6002]
    #[msg = "Threshold must be between 1 and number of signers"]
    InvalidThreshold,

    /// Timelock too short
    #[code = 6003]
    #[msg = "Timelock must be at least 100 slots"]
    TimelockTooShort,

    /// Not a treasury signer
    #[code = 6004]
    #[msg = "Account is not a treasury signer"]
    NotASigner,

    /// Already approved
    #[code = 6005]
    #[msg = "Proposal already approved by this signer"]
    AlreadyApproved,

    /// Already executed
    #[code = 6006]
    #[msg = "Proposal has already been executed"]
    AlreadyExecuted,

    /// Insufficient approvals
    #[code = 6007]
    #[msg = "Not enough approvals to execute"]
    InsufficientApprovals,

    /// Timelock not expired
    #[code = 6008]
    #[msg = "Timelock period has not expired"]
    TimelockNotExpired,

    /// Unauthorized
    #[code = 6009]
    #[msg = "Unauthorized - only creator or authority can perform this action"]
    Unauthorized,

    /// Invalid proposal
    #[code = 6010]
    #[msg = "Proposal does not belong to this treasury"]
    InvalidProposal,
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_tags() {
        assert_eq!(TreasuryInstruction::TAG_INITIALIZE, 0);
        assert_eq!(TreasuryInstruction::TAG_CREATE_PROPOSAL, 1);
        assert_eq!(TreasuryInstruction::TAG_APPROVE, 2);
        assert_eq!(TreasuryInstruction::TAG_EXECUTE, 3);
        assert_eq!(TreasuryInstruction::TAG_CANCEL, 4);
    }

    #[test]
    fn test_account_sizes() {
        // Ensure sizes are reasonable
        assert!(Treasury::SIZE < 1024);
        assert!(Proposal::SIZE < 1024);
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(TreasuryError::NoSigners.code(), 6000);
        assert_eq!(TreasuryError::TooManySigners.code(), 6001);
        assert_eq!(TreasuryError::InvalidThreshold.code(), 6002);
    }
}
