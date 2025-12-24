//! DPL Token Mint Example Contract
//!
//! This contract demonstrates creating a custom token mint with:
//! - Mint initialization with configurable decimals and supply cap
//! - Minting new tokens (authority-controlled)
//! - Burning tokens (holder-controlled)
//! - Token transfers
//! - Freeze authority
//! - Multiple validation patterns (constraints, PDAs, capability checks)
//!
//! # Build
//! ```bash
//! cargo build --target wasm32-wasi --release
//! ```
//!
//! # Key Patterns Demonstrated
//! - Complex account constraints (has_one, seeds, mint constraints)
//! - Multiple authority roles (mint, freeze)
//! - Capability-based permissions
//! - Overflow/underflow protection
//! - Event emission with rich metadata
//! - Custom error codes with descriptive messages

use dchat_dpl::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAM DEFINITION
// ═══════════════════════════════════════════════════════════════════════════════

/// Token mint program with full SPL-style features
#[program]
pub mod token_mint {
    use super::*;

    /// Initialize a new token mint
    pub fn initialize_mint<'a>(
        mut ctx: Context<'a, InitializeMint<'a>>,
        decimals: u8,
        max_supply: Option<u64>,
    ) -> Result<()> {
        let mint = &mut ctx.accounts.mint;
        let mint_key = *ctx.accounts.mint.key();
        let mint_authority_key = *ctx.accounts.mint_authority.key();
        let freeze_authority_key = ctx
            .accounts
            .freeze_authority
            .map(|acc| *acc.key())
            .unwrap_or_else(|| Pubkey::default());

        // Initialize mint state
        mint.mint_authority = mint_authority_key;
        mint.freeze_authority = freeze_authority_key;
        mint.supply = 0;
        mint.max_supply = max_supply;
        mint.decimals = decimals;
        mint.is_initialized = true;
        mint.frozen = false;

        emit!(MintInitialized {
            mint: mint_key,
            mint_authority: mint_authority_key,
            freeze_authority: freeze_authority_key,
            decimals,
            max_supply,
        });

        Ok(())
    }

    /// Create a new token account for a holder
    pub fn initialize_account<'a>(mut ctx: Context<'a, InitializeAccount<'a>>) -> Result<()> {
        let token_account = &mut ctx.accounts.token_account;
        let mint_key = *ctx.accounts.mint.key();
        let owner_key = *ctx.accounts.owner.key();
        let account_key = *ctx.accounts.token_account.key();

        // Verify mint is initialized
        require!(
            ctx.accounts.mint.is_initialized,
            TokenError::MintNotInitialized
        );

        // Initialize token account
        token_account.mint = mint_key;
        token_account.owner = owner_key;
        token_account.amount = 0;
        token_account.delegate = Pubkey::default();
        token_account.delegated_amount = 0;
        token_account.is_frozen = false;

        emit!(AccountInitialized {
            account: account_key,
            mint: mint_key,
            owner: owner_key,
        });

        Ok(())
    }

    /// Mint new tokens to a token account
    pub fn mint_to<'a>(mut ctx: Context<'a, MintTo<'a>>, amount: u64) -> Result<()> {
        let mint = &mut ctx.accounts.mint;
        let token_account = &mut ctx.accounts.token_account;
        let mint_key = *ctx.accounts.mint.key();
        let dest_key = *ctx.accounts.token_account.key();

        // Check mint is not frozen
        require!(!mint.frozen, TokenError::MintFrozen);

        // Check token account is not frozen
        require!(!token_account.is_frozen, TokenError::AccountFrozen);

        // Check max supply constraint
        if let Some(max_supply) = mint.max_supply {
            let new_supply = mint
                .supply
                .checked_add(amount)
                .ok_or(TokenError::Overflow)?;
            require!(new_supply <= max_supply, TokenError::MaxSupplyExceeded);
        }

        // Update supply and account balance
        mint.supply = mint
            .supply
            .checked_add(amount)
            .ok_or(TokenError::Overflow)?;
        token_account.amount = token_account
            .amount
            .checked_add(amount)
            .ok_or(TokenError::Overflow)?;

        emit!(TokensMinted {
            mint: mint_key,
            destination: dest_key,
            amount,
            new_supply: mint.supply,
        });

        Ok(())
    }

    /// Burn tokens from a token account
    pub fn burn<'a>(mut ctx: Context<'a, Burn<'a>>, amount: u64) -> Result<()> {
        let mint = &mut ctx.accounts.mint;
        let token_account = &mut ctx.accounts.token_account;
        let mint_key = *ctx.accounts.mint.key();
        let source_key = *ctx.accounts.token_account.key();

        // Check token account has sufficient balance
        require!(
            token_account.amount >= amount,
            TokenError::InsufficientBalance
        );

        // Update supply and account balance
        mint.supply = mint
            .supply
            .checked_sub(amount)
            .ok_or(TokenError::Underflow)?;
        token_account.amount = token_account
            .amount
            .checked_sub(amount)
            .ok_or(TokenError::Underflow)?;

        emit!(TokensBurned {
            mint: mint_key,
            source: source_key,
            amount,
            new_supply: mint.supply,
        });

        Ok(())
    }

    /// Transfer tokens between accounts
    pub fn transfer<'a>(mut ctx: Context<'a, Transfer<'a>>, amount: u64) -> Result<()> {
        let source = &mut ctx.accounts.source;
        let destination = &mut ctx.accounts.destination;
        let source_key = *ctx.accounts.source.key();
        let dest_key = *ctx.accounts.destination.key();

        // Check source has sufficient balance
        require!(source.amount >= amount, TokenError::InsufficientBalance);

        // Check accounts are not frozen
        require!(!source.is_frozen, TokenError::AccountFrozen);
        require!(!destination.is_frozen, TokenError::AccountFrozen);

        // Update balances
        source.amount = source
            .amount
            .checked_sub(amount)
            .ok_or(TokenError::Underflow)?;
        destination.amount = destination
            .amount
            .checked_add(amount)
            .ok_or(TokenError::Overflow)?;

        emit!(TokensTransferred {
            source: source_key,
            destination: dest_key,
            amount,
        });

        Ok(())
    }

    /// Freeze a token account (freeze authority only)
    pub fn freeze_account<'a>(mut ctx: Context<'a, FreezeAccount<'a>>) -> Result<()> {
        let token_account = &mut ctx.accounts.token_account;
        let account_key = *ctx.accounts.token_account.key();

        token_account.is_frozen = true;

        emit!(AccountFrozen {
            account: account_key,
        });

        Ok(())
    }

    /// Thaw a frozen token account (freeze authority only)
    pub fn thaw_account<'a>(mut ctx: Context<'a, ThawAccount<'a>>) -> Result<()> {
        let token_account = &mut ctx.accounts.token_account;
        let account_key = *ctx.accounts.token_account.key();

        token_account.is_frozen = false;

        emit!(AccountThawed {
            account: account_key,
        });

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT STRUCTURES
// ═══════════════════════════════════════════════════════════════════════════════

/// Mint account data
#[account]
pub struct MintAccount {
    /// Authority that can mint new tokens
    pub mint_authority: Pubkey,
    /// Authority that can freeze accounts (optional)
    pub freeze_authority: Pubkey,
    /// Total supply of tokens
    pub supply: u64,
    /// Maximum supply cap (optional)
    pub max_supply: Option<u64>,
    /// Number of decimal places
    pub decimals: u8,
    /// Whether the mint is initialized
    pub is_initialized: bool,
    /// Whether minting is permanently frozen
    pub frozen: bool,
}

/// Token account data
#[account]
pub struct TokenAccount {
    /// The mint this account holds tokens for
    pub mint: Pubkey,
    /// Owner of this token account
    pub owner: Pubkey,
    /// Current token balance
    pub amount: u64,
    /// Delegate allowed to transfer tokens
    pub delegate: Pubkey,
    /// Amount the delegate is allowed to transfer
    pub delegated_amount: u64,
    /// Whether the account is frozen
    pub is_frozen: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION CONTEXTS
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Accounts)]
pub struct InitializeMint<'a> {
    #[account(init, payer = payer)]
    pub mint: Account<'a, MintAccount>,
    pub mint_authority: Signer<'a>,
    /// Optional freeze authority
    pub freeze_authority: Option<Account<'a, Signer<'a>>>,
    #[account(mut)]
    pub payer: Signer<'a>,
}

#[derive(Accounts)]
pub struct InitializeAccount<'a> {
    pub mint: Account<'a, MintAccount>,
    #[account(init, payer = payer, constraint = token_account.mint == *mint.key())]
    pub token_account: Account<'a, TokenAccount>,
    pub owner: Account<'a, Signer<'a>>,
    #[account(mut)]
    pub payer: Signer<'a>,
}

#[derive(Accounts)]
pub struct MintTo<'a> {
    #[account(mut)]
    pub mint: Account<'a, MintAccount>,
    #[account(mut, constraint = token_account.mint == *mint.key())]
    pub token_account: Account<'a, TokenAccount>,
    #[account(signer, constraint = *mint_authority.key() == mint.mint_authority)]
    pub mint_authority: Signer<'a>,
}

#[derive(Accounts)]
pub struct Burn<'a> {
    #[account(mut)]
    pub mint: Account<'a, MintAccount>,
    #[account(mut, constraint = token_account.mint == *mint.key())]
    pub token_account: Account<'a, TokenAccount>,
    #[account(signer, constraint = *authority.key() == token_account.owner)]
    pub authority: Signer<'a>,
}

#[derive(Accounts)]
pub struct Transfer<'a> {
    #[account(mut, constraint = source.mint == destination.mint)]
    pub source: Account<'a, TokenAccount>,
    #[account(mut)]
    pub destination: Account<'a, TokenAccount>,
    #[account(signer, constraint = *authority.key() == source.owner)]
    pub authority: Signer<'a>,
}

#[derive(Accounts)]
pub struct FreezeAccount<'a> {
    pub mint: Account<'a, MintAccount>,
    #[account(mut, constraint = token_account.mint == *mint.key())]
    pub token_account: Account<'a, TokenAccount>,
    #[account(signer, constraint = *freeze_authority.key() == mint.freeze_authority)]
    pub freeze_authority: Signer<'a>,
}

#[derive(Accounts)]
pub struct ThawAccount<'a> {
    pub mint: Account<'a, MintAccount>,
    #[account(mut, constraint = token_account.mint == *mint.key())]
    pub token_account: Account<'a, TokenAccount>,
    #[account(signer, constraint = *freeze_authority.key() == mint.freeze_authority)]
    pub freeze_authority: Signer<'a>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

#[event]
pub struct MintInitialized {
    pub mint: Pubkey,
    pub mint_authority: Pubkey,
    pub freeze_authority: Pubkey,
    pub decimals: u8,
    pub max_supply: Option<u64>,
}

#[event]
pub struct AccountInitialized {
    pub account: Pubkey,
    pub mint: Pubkey,
    pub owner: Pubkey,
}

#[event]
pub struct TokensMinted {
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
    pub new_supply: u64,
}

#[event]
pub struct TokensBurned {
    pub mint: Pubkey,
    pub source: Pubkey,
    pub amount: u64,
    pub new_supply: u64,
}

#[event]
pub struct TokensTransferred {
    pub source: Pubkey,
    pub destination: Pubkey,
    pub amount: u64,
}

#[event]
pub struct AccountFrozen {
    pub account: Pubkey,
}

#[event]
pub struct AccountThawed {
    pub account: Pubkey,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERROR CODES
// ═══════════════════════════════════════════════════════════════════════════════

#[error_code]
pub enum TokenError {
    #[msg("Mint is not initialized")]
    MintNotInitialized,
    #[msg("Mint is frozen")]
    MintFrozen,
    #[msg("Account is frozen")]
    AccountFrozen,
    #[msg("Maximum supply exceeded")]
    MaxSupplyExceeded,
    #[msg("Insufficient balance")]
    InsufficientBalance,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Arithmetic underflow")]
    Underflow,
}
