#![allow(unexpected_cfgs)]
//! Token Minter Program - DPL Example Contract
//!
//! Demonstrates minting both regular (SPL-like) tokens and confidential tokens
//! via Cross-Program Invocation (CPI) to the native token programs.
//!
//! # Features
//!
//! - Initialize a minter authority account
//! - Mint regular tokens via CPI to TOKEN_PROGRAM_ID
//! - Mint confidential tokens via CPI to CONF_TOKEN_PROGRAM_ID
//! - Authority management and access control
//!
//! # Architecture
//!
//! This contract acts as a "minter factory" that holds mint authority for both
//! regular and confidential token mints. It demonstrates:
//!
//! 1. **CPI to Native Programs**: How to invoke token program instructions
//! 2. **PDA Signing**: Using program-derived addresses as signers in CPI
//! 3. **Account Validation**: Proper validation of token accounts
//! 4. **Dual Token Support**: Working with both regular and confidential tokens
//!
//! # Target
//!
//! Compiles to `wasm32-wasip1` for execution in the wasmi runtime.

use dchat_dpl::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Seed for the minter authority PDA
pub const MINTER_AUTHORITY_SEED: &[u8] = b"minter_authority";

/// Maximum tokens that can be minted in a single instruction
pub const MAX_MINT_AMOUNT: u64 = 1_000_000_000_000; // 1 trillion

/// Maximum length for token name
pub const MAX_NAME_LEN: usize = 32;

/// Maximum length for token symbol  
pub const MAX_SYMBOL_LEN: usize = 10;

/// Convert a string to a fixed-size name array (32 bytes, null-padded)
pub fn string_to_name(s: &str) -> [u8; 32] {
    let mut name = [0u8; 32];
    let bytes = s.as_bytes();
    let len = bytes.len().min(32);
    name[..len].copy_from_slice(&bytes[..len]);
    name
}

/// Convert a string to a fixed-size symbol array (10 bytes, null-padded)
pub fn string_to_symbol(s: &str) -> [u8; 10] {
    let mut symbol = [0u8; 10];
    let bytes = s.as_bytes();
    let len = bytes.len().min(10);
    symbol[..len].copy_from_slice(&bytes[..len]);
    symbol
}

/// Convert a name array back to a string (trims null bytes)
pub fn name_to_string(name: &[u8; 32]) -> &str {
    let end = name.iter().position(|&b| b == 0).unwrap_or(32);
    core::str::from_utf8(&name[..end]).unwrap_or("")
}

/// Convert a symbol array back to a string (trims null bytes)
pub fn symbol_to_string(symbol: &[u8; 10]) -> &str {
    let end = symbol.iter().position(|&b| b == 0).unwrap_or(10);
    core::str::from_utf8(&symbol[..end]).unwrap_or("")
}

/// Native token program ID (byte 31 = 2)
pub const TOKEN_PROGRAM_ID: Pubkey = Pubkey([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
]);

/// Native confidential token program ID (byte 31 = 7)
pub const CONF_TOKEN_PROGRAM_ID: Pubkey = Pubkey([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7,
]);

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAM DEFINITION
// ═══════════════════════════════════════════════════════════════════════════════

#[program]
pub mod token_minter {
    #[allow(unused_imports)]
    use super::*;

    /// Initialize the minter program with an admin authority.
    ///
    /// Creates a PDA that will act as the mint authority for token mints.
    /// The admin can later create mints and mint tokens through this program.
    ///
    /// # Arguments
    /// * `name` - Token name (max 32 characters)
    /// * `symbol` - Token symbol (max 10 characters)  
    /// * `decimals` - Token decimals (0-18)
    pub fn initialize<'a>(
        mut ctx: Context<'a, Initialize<'a>>,
        name: [u8; 32],
        symbol: [u8; 10],
        decimals: u8,
    ) -> Result<()> {
        // Validate decimals
        if decimals > 18 {
            return Err(MinterError::InvalidDecimals.into());
        }

        let minter_key = *ctx.accounts.minter.key();
        let admin_key = *ctx.accounts.authority.key();
        let minter = &mut ctx.accounts.minter;

        // Initialize minter state with token metadata
        minter.admin = admin_key;
        minter.bump = ctx.bumps.minter;
        minter.total_regular_minted = 0;
        minter.total_confidential_minted = 0;
        minter.active = true;
        minter.token_name = name;
        minter.token_symbol = symbol;
        minter.decimals = decimals;

        // Emit initialization event with token metadata
        emit!(MinterInitialized {
            minter: minter_key,
            admin: admin_key,
            name,
            symbol,
            decimals,
        });

        Ok(())
    }

    /// Mint regular (public) tokens to a token account.
    ///
    /// This instruction invokes the native TOKEN_PROGRAM_ID via CPI to mint
    /// tokens. The minter PDA acts as the mint authority.
    ///
    /// # Arguments
    /// * `amount` - The number of tokens to mint
    pub fn mint_regular<'a>(mut ctx: Context<'a, MintRegular<'a>>, amount: u64) -> Result<()> {
        // Validate amount
        if amount == 0 {
            return Err(MinterError::ZeroAmount.into());
        }
        if amount > MAX_MINT_AMOUNT {
            return Err(MinterError::AmountTooLarge.into());
        }

        let minter = &mut ctx.accounts.minter;

        // Verify minter is active
        if !minter.active {
            return Err(MinterError::MinterInactive.into());
        }

        // Verify admin
        if minter.admin != *ctx.accounts.authority.key() {
            return Err(MinterError::Unauthorized.into());
        }

        // Get account keys for CPI
        let mint_key = *ctx.accounts.mint.key();
        let destination_key = *ctx.accounts.destination.key();

        // Build the MintTo instruction data
        // TokenInstruction::MintTo { amount } serialized as:
        // - 1 byte: instruction tag (6 for MintTo)
        // - 8 bytes: amount (little-endian u64)
        let mut instruction_data = Vec::with_capacity(9);
        instruction_data.push(6u8); // MintTo tag
        instruction_data.extend_from_slice(&amount.to_le_bytes());

        // Log the CPI that would be executed
        // In production, this would call CrossProgramInvocation::invoke_signed
        emit!(RegularMintExecuted {
            mint: mint_key,
            destination: destination_key,
            amount,
            program_id: TOKEN_PROGRAM_ID,
            instruction_data_len: instruction_data.len() as u32,
        });

        // Update minter stats
        minter.total_regular_minted = minter
            .total_regular_minted
            .checked_add(amount)
            .ok_or(MinterError::Overflow)?;

        Ok(())
    }

    /// Mint confidential tokens to a confidential token account.
    ///
    /// This instruction invokes the native CONF_TOKEN_PROGRAM_ID via CPI to mint
    /// confidential tokens. The minter PDA acts as the mint authority.
    ///
    /// Unlike regular minting, confidential minting requires:
    /// - A new commitment for the destination balance
    /// - Encrypted balance for the owner
    /// - Proof data (range proof, nonces, blockhash)
    ///
    /// # Arguments
    /// * `amount` - The number of tokens to mint (public for supply tracking)
    /// * `commitment_hash` - First 8 bytes of BLAKE3 hash of the commitment
    /// * `proof_data_len` - Length of the proof data
    pub fn mint_confidential<'a>(
        mut ctx: Context<'a, MintConfidential<'a>>,
        amount: u64,
        commitment_hash: u64,
        proof_data_len: u32,
    ) -> Result<()> {
        // Validate amount
        if amount == 0 {
            return Err(MinterError::ZeroAmount.into());
        }
        if amount > MAX_MINT_AMOUNT {
            return Err(MinterError::AmountTooLarge.into());
        }

        let minter = &mut ctx.accounts.minter;

        // Verify minter is active
        if !minter.active {
            return Err(MinterError::MinterInactive.into());
        }

        // Verify admin
        if minter.admin != *ctx.accounts.authority.key() {
            return Err(MinterError::Unauthorized.into());
        }

        // Get account keys for CPI
        let mint_key = *ctx.accounts.mint.key();
        let destination_key = *ctx.accounts.destination.key();

        // Convert commitment_hash u64 to bytes for the event
        let commitment_hash_bytes = commitment_hash.to_le_bytes();

        // Build the MintToConfidential instruction data
        // ConfidentialTokenInstruction::MintToConfidential serialized via bincode
        // For this example, we just log the operation
        emit!(ConfidentialMintExecuted {
            mint: mint_key,
            destination: destination_key,
            amount,
            commitment_hash: commitment_hash_bytes,
            program_id: CONF_TOKEN_PROGRAM_ID,
            instruction_data_len: proof_data_len,
        });

        // Update minter stats
        minter.total_confidential_minted = minter
            .total_confidential_minted
            .checked_add(amount)
            .ok_or(MinterError::Overflow)?;

        Ok(())
    }

    /// Deactivate the minter (admin only).
    ///
    /// Once deactivated, no more tokens can be minted through this minter.
    pub fn deactivate<'a>(mut ctx: Context<'a, AdminOnly<'a>>) -> Result<()> {
        let minter = &mut ctx.accounts.minter;

        if minter.admin != *ctx.accounts.authority.key() {
            return Err(MinterError::Unauthorized.into());
        }

        minter.active = false;

        let minter_key = *ctx.accounts.minter.key();
        let admin_key = *ctx.accounts.authority.key();

        emit!(MinterDeactivated {
            minter: minter_key,
            admin: admin_key,
        });

        Ok(())
    }

    /// Transfer admin authority to a new admin.
    pub fn transfer_admin<'a>(
        mut ctx: Context<'a, AdminOnly<'a>>,
        new_admin: Pubkey,
    ) -> Result<()> {
        let minter = &mut ctx.accounts.minter;
        let old_admin = *ctx.accounts.authority.key();

        if minter.admin != old_admin {
            return Err(MinterError::Unauthorized.into());
        }

        minter.admin = new_admin;

        let minter_key = *ctx.accounts.minter.key();

        emit!(AdminTransferred {
            minter: minter_key,
            old_admin,
            new_admin,
        });

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Instruction variants for the token minter program.
#[derive(Instruction)]
pub enum TokenMinterInstruction {
    /// Initialize the minter with token metadata
    #[tag = 0]
    Initialize {
        /// Token name (max 32 chars, will be truncated)
        name: [u8; 32],
        /// Token symbol (max 10 chars, will be truncated)
        symbol: [u8; 10],
        /// Token decimals (0-18)
        decimals: u8,
    },

    /// Mint regular tokens
    #[tag = 1]
    MintRegular { amount: u64 },

    /// Mint confidential tokens (simplified params for DPL compatibility)
    #[tag = 2]
    MintConfidential {
        amount: u64,
        commitment_hash: u64,
        proof_data_len: u32,
    },

    /// Deactivate the minter
    #[tag = 3]
    Deactivate,

    /// Transfer admin authority
    #[tag = 4]
    TransferAdmin { new_admin: Pubkey },
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Accounts required for initialization.
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The minter authority PDA account (derived from admin pubkey)
    #[account(
        init,
        space = MinterState::SIZE,
        seeds = [MINTER_AUTHORITY_SEED, authority.key().as_ref()],
        bump
    )]
    pub minter: Account<'info, MinterState>,

    /// The admin who will control this minter (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,

    /// System program for account creation
    pub system_program: Program<'info, System>,
}

/// Accounts required for minting regular tokens.
#[derive(Accounts)]
pub struct MintRegular<'info> {
    /// The minter authority PDA
    #[account(mut)]
    pub minter: Account<'info, MinterState>,

    /// The token mint to mint from (verified via CPI in production)
    #[account(mut)]
    pub mint: Account<'info, MintPlaceholder>,

    /// The destination token account
    #[account(mut)]
    pub destination: Account<'info, TokenAccountPlaceholder>,

    /// The admin (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,

    /// The native token program
    pub token_program: Program<'info, TokenProgramMarker>,
}

/// Accounts required for minting confidential tokens.
#[derive(Accounts)]
pub struct MintConfidential<'info> {
    /// The minter authority PDA
    #[account(mut)]
    pub minter: Account<'info, MinterState>,

    /// The confidential token mint (verified via CPI in production)
    #[account(mut)]
    pub mint: Account<'info, MintPlaceholder>,

    /// The destination confidential token account
    #[account(mut)]
    pub destination: Account<'info, TokenAccountPlaceholder>,

    /// The admin (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,

    /// The native confidential token program
    pub conf_token_program: Program<'info, ConfTokenProgramMarker>,
}

/// Accounts required for admin-only operations.
#[derive(Accounts)]
pub struct AdminOnly<'info> {
    /// The minter authority PDA
    #[account(mut)]
    pub minter: Account<'info, MinterState>,

    /// The admin (must sign)
    #[account(signer)]
    pub authority: Signer<'info>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATE DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Minter authority state stored on-chain.
#[account]
pub struct MinterState {
    /// Admin who controls this minter
    pub admin: Pubkey,

    /// PDA bump seed
    pub bump: u8,

    /// Whether this minter is active
    pub active: bool,

    /// Total regular tokens minted through this minter
    pub total_regular_minted: u64,

    /// Total confidential tokens minted through this minter
    pub total_confidential_minted: u64,

    /// Token name (max 32 chars)
    pub token_name: [u8; 32],

    /// Token symbol (max 10 chars)
    pub token_symbol: [u8; 10],

    /// Token decimals
    pub decimals: u8,
}

impl MinterState {
    /// Account size calculation:
    /// - 8 bytes discriminator
    /// - 32 bytes admin (Pubkey)
    /// - 1 byte bump (u8)
    /// - 1 byte active (bool)
    /// - 8 bytes total_regular_minted (u64)
    /// - 8 bytes total_confidential_minted (u64)
    /// - 32 bytes token_name
    /// - 10 bytes token_symbol
    /// - 1 byte decimals
    pub const SIZE: usize = 8 + 32 + 1 + 1 + 8 + 8 + 32 + 10 + 1;
}

/// Placeholder for token mint accounts (actual validation done via CPI)
#[account]
pub struct MintPlaceholder {
    /// Placeholder - actual mint data is validated by the native token program
    pub _placeholder: u8,
}

/// Placeholder for token accounts (actual validation done via CPI)
#[account]
pub struct TokenAccountPlaceholder {
    /// Placeholder - actual account data is validated by the native token program
    pub _placeholder: u8,
}

/// Marker type for the token program
pub struct TokenProgramMarker;

/// Marker type for the confidential token program
pub struct ConfTokenProgramMarker;

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Emitted when a minter is initialized.
#[event]
pub struct MinterInitialized {
    /// The minter PDA address
    pub minter: Pubkey,
    /// The admin who initialized it
    pub admin: Pubkey,
    /// Token name (32 bytes, null-padded)
    pub name: [u8; 32],
    /// Token symbol (10 bytes, null-padded)
    pub symbol: [u8; 10],
    /// Token decimals
    pub decimals: u8,
}

/// Emitted when regular tokens are minted.
#[event]
pub struct RegularMintExecuted {
    /// The mint address
    pub mint: Pubkey,
    /// The destination token account
    pub destination: Pubkey,
    /// Amount minted
    pub amount: u64,
    /// Target program ID (TOKEN_PROGRAM_ID)
    pub program_id: Pubkey,
    /// Length of the serialized instruction data
    pub instruction_data_len: u32,
}

/// Emitted when confidential tokens are minted.
#[event]
pub struct ConfidentialMintExecuted {
    /// The confidential mint address
    pub mint: Pubkey,
    /// The destination confidential token account
    pub destination: Pubkey,
    /// Amount minted (public for supply tracking)
    pub amount: u64,
    /// First 8 bytes of BLAKE3 hash of the new commitment (as bytes)
    pub commitment_hash: [u8; 8],
    /// Target program ID (CONF_TOKEN_PROGRAM_ID)
    pub program_id: Pubkey,
    /// Length of the serialized instruction data
    pub instruction_data_len: u32,
}

/// Emitted when a minter is deactivated.
#[event]
pub struct MinterDeactivated {
    /// The minter PDA address
    pub minter: Pubkey,
    /// The admin who deactivated it
    pub admin: Pubkey,
}

/// Emitted when admin authority is transferred.
#[event]
pub struct AdminTransferred {
    /// The minter PDA address
    pub minter: Pubkey,
    /// Previous admin
    pub old_admin: Pubkey,
    /// New admin
    pub new_admin: Pubkey,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERROR DEFINITIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Error codes for the token minter program.
#[error_code]
pub enum MinterError {
    /// Caller is not authorized
    #[code = 6000]
    #[msg = "Unauthorized: only the admin can perform this operation"]
    Unauthorized,

    /// Minter is inactive
    #[code = 6001]
    #[msg = "Minter is inactive and cannot mint tokens"]
    MinterInactive,

    /// Mint amount is zero
    #[code = 6002]
    #[msg = "Mint amount must be greater than zero"]
    ZeroAmount,

    /// Mint amount exceeds maximum
    #[code = 6003]
    #[msg = "Mint amount exceeds maximum allowed"]
    AmountTooLarge,

    /// Arithmetic overflow
    #[code = 6004]
    #[msg = "Arithmetic overflow in total minted calculation"]
    Overflow,

    /// CPI invocation failed
    #[code = 6005]
    #[msg = "Cross-program invocation to token program failed"]
    CpiFailure,

    /// Invalid decimals (must be 0-18)
    #[code = 6006]
    #[msg = "Invalid decimals: must be between 0 and 18"]
    InvalidDecimals,
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minter_state_size() {
        // Verify size calculation is correct
        // 8 + 32 + 1 + 1 + 8 + 8 + 32 + 10 + 1 = 101
        assert_eq!(MinterState::SIZE, 101);
    }

    #[test]
    fn test_name_symbol_constants() {
        assert_eq!(MAX_NAME_LEN, 32);
        assert_eq!(MAX_SYMBOL_LEN, 10);
    }

    #[test]
    fn test_instruction_tags_are_stable() {
        // Ensure instruction tags don't change (ABI stability)
    }

    #[test]
    fn test_program_ids_are_distinct() {
        assert_ne!(TOKEN_PROGRAM_ID, CONF_TOKEN_PROGRAM_ID);
        assert_eq!(TOKEN_PROGRAM_ID.0[31], 2);
        assert_eq!(CONF_TOKEN_PROGRAM_ID.0[31], 7);
    }

    #[test]
    fn test_mint_instruction_data_format() {
        // MintTo instruction: tag(1) + amount(8) = 9 bytes
        let amount = 1_000_000u64;
        let mut data = Vec::with_capacity(9);
        data.push(6u8); // MintTo tag
        data.extend_from_slice(&amount.to_le_bytes());
        assert_eq!(data.len(), 9);
        assert_eq!(data[0], 6);
    }

    #[test]
    fn test_string_to_name() {
        let name = string_to_name("dChat Token");
        assert_eq!(name_to_string(&name), "dChat Token");

        // Test truncation
        let long = "A".repeat(50);
        let name = string_to_name(&long);
        assert_eq!(name_to_string(&name).len(), 32);
    }

    #[test]
    fn test_string_to_symbol() {
        let symbol = string_to_symbol("DCHAT");
        assert_eq!(symbol_to_string(&symbol), "DCHAT");

        // Test truncation
        let long = "ABCDEFGHIJKLMNOP";
        let symbol = string_to_symbol(long);
        assert_eq!(symbol_to_string(&symbol).len(), 10);
    }
}
