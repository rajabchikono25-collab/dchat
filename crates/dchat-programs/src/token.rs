//! Token program for fungible and non-fungible tokens

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::account::{Account, AccountMeta, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::instruction::Instruction;
use crate::metering::ComputeMeter;
use crate::pda::PdaDerivation;

/// Token program ID
pub const TOKEN_PROGRAM_ID: Pubkey = crate::native_programs::TOKEN_PROGRAM_ID;

/// Associated Token Account program ID
pub const ATA_PROGRAM_ID: Pubkey = crate::native_programs::ATA_PROGRAM_ID;

/// Maximum decimals for a token
pub const MAX_DECIMALS: u8 = 18;

/// Mint account state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mint {
    /// Mint authority (can mint new tokens)
    pub mint_authority: Option<Pubkey>,
    /// Total supply of tokens
    pub supply: u64,
    /// Number of decimals
    pub decimals: u8,
    /// Is this mint initialized
    pub is_initialized: bool,
    /// Freeze authority (can freeze token accounts)
    pub freeze_authority: Option<Pubkey>,
}

impl Mint {
    /// Size of mint account
    pub const SIZE: usize = 82;

    /// Create new mint
    pub fn new(decimals: u8, mint_authority: Pubkey, freeze_authority: Option<Pubkey>) -> Self {
        Self {
            mint_authority: Some(mint_authority),
            supply: 0,
            decimals,
            is_initialized: true,
            freeze_authority,
        }
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < Self::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = bincode::serialize(self).unwrap_or_default();
        data.resize(Self::SIZE, 0);
        data
    }
}

/// Token account state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAccount {
    /// The mint this account holds tokens of
    pub mint: Pubkey,
    /// The owner of this account
    pub owner: Pubkey,
    /// Amount of tokens
    pub amount: u64,
    /// Delegate authority
    pub delegate: Option<Pubkey>,
    /// Delegated amount
    pub delegated_amount: u64,
    /// Account state
    pub state: AccountState,
    /// Is this a native token account (wrapped SOL equivalent)
    pub is_native: Option<u64>,
    /// Close authority
    pub close_authority: Option<Pubkey>,
}

impl TokenAccount {
    /// Size of token account
    pub const SIZE: usize = 165;

    /// Create new token account
    pub fn new(mint: Pubkey, owner: Pubkey) -> Self {
        Self {
            mint,
            owner,
            amount: 0,
            delegate: None,
            delegated_amount: 0,
            state: AccountState::Initialized,
            is_native: None,
            close_authority: None,
        }
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < Self::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = bincode::serialize(self).unwrap_or_default();
        data.resize(Self::SIZE, 0);
        data
    }

    /// Check if account is frozen
    pub fn is_frozen(&self) -> bool {
        self.state == AccountState::Frozen
    }
}

/// Token account state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountState {
    /// Account is not yet initialized
    Uninitialized,
    /// Account is initialized
    Initialized,
    /// Account is frozen
    Frozen,
}

/// Token instruction types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TokenInstruction {
    /// Initialize a new mint
    InitializeMint {
        decimals: u8,
        mint_authority: Pubkey,
        freeze_authority: Option<Pubkey>,
    },

    /// Initialize a new token account
    InitializeAccount,

    /// Initialize a token account with explicit owner
    InitializeAccount2 { owner: Pubkey },

    /// Initialize multisig account
    InitializeMultisig { m: u8 },

    /// Transfer tokens
    Transfer { amount: u64 },

    /// Approve delegate
    Approve { amount: u64 },

    /// Revoke delegate
    Revoke,

    /// Set authority
    SetAuthority {
        authority_type: AuthorityType,
        new_authority: Option<Pubkey>,
    },

    /// Mint new tokens
    MintTo { amount: u64 },

    /// Burn tokens
    Burn { amount: u64 },

    /// Close account
    CloseAccount,

    /// Freeze account
    FreezeAccount,

    /// Thaw account
    ThawAccount,

    /// Transfer with checked decimals
    TransferChecked { amount: u64, decimals: u8 },

    /// Approve with checked decimals
    ApproveChecked { amount: u64, decimals: u8 },

    /// Mint with checked decimals
    MintToChecked { amount: u64, decimals: u8 },

    /// Burn with checked decimals
    BurnChecked { amount: u64, decimals: u8 },

    /// Sync native account
    SyncNative,
}

/// Authority type for SetAuthority instruction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityType {
    /// Mint authority
    MintTokens,
    /// Freeze authority
    FreezeAccount,
    /// Token account owner
    AccountOwner,
    /// Close authority
    CloseAccount,
}

/// Token program implementation
pub struct TokenProgram;

impl TokenProgram {
    /// Initialize mint instruction
    pub fn initialize_mint(
        mint: Pubkey,
        mint_authority: Pubkey,
        freeze_authority: Option<Pubkey>,
        decimals: u8,
    ) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(mint, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_RENT_ID, false),
            ],
            data: bincode::serialize(&TokenInstruction::InitializeMint {
                decimals,
                mint_authority,
                freeze_authority,
            })
            .unwrap_or_default(),
        }
    }

    /// Initialize account instruction
    pub fn initialize_account(account: Pubkey, mint: Pubkey, owner: Pubkey) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(owner, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_RENT_ID, false),
            ],
            data: bincode::serialize(&TokenInstruction::InitializeAccount).unwrap_or_default(),
        }
    }

    /// Transfer instruction
    pub fn transfer(
        source: Pubkey,
        destination: Pubkey,
        authority: Pubkey,
        amount: u64,
    ) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(source, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: bincode::serialize(&TokenInstruction::Transfer { amount }).unwrap_or_default(),
        }
    }

    /// Transfer checked instruction
    pub fn transfer_checked(
        source: Pubkey,
        mint: Pubkey,
        destination: Pubkey,
        authority: Pubkey,
        amount: u64,
        decimals: u8,
    ) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(source, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: bincode::serialize(&TokenInstruction::TransferChecked { amount, decimals })
                .unwrap_or_default(),
        }
    }

    /// Approve instruction
    pub fn approve(source: Pubkey, delegate: Pubkey, owner: Pubkey, amount: u64) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(source, false),
                AccountMeta::new_readonly(delegate, false),
                AccountMeta::new_readonly(owner, true),
            ],
            data: bincode::serialize(&TokenInstruction::Approve { amount }).unwrap_or_default(),
        }
    }

    /// Revoke instruction
    pub fn revoke(source: Pubkey, owner: Pubkey) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(source, false),
                AccountMeta::new_readonly(owner, true),
            ],
            data: bincode::serialize(&TokenInstruction::Revoke).unwrap_or_default(),
        }
    }

    /// Mint to instruction
    pub fn mint_to(
        mint: Pubkey,
        destination: Pubkey,
        mint_authority: Pubkey,
        amount: u64,
    ) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(mint, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(mint_authority, true),
            ],
            data: bincode::serialize(&TokenInstruction::MintTo { amount }).unwrap_or_default(),
        }
    }

    /// Burn instruction
    pub fn burn(account: Pubkey, mint: Pubkey, owner: Pubkey, amount: u64) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new(mint, false),
                AccountMeta::new_readonly(owner, true),
            ],
            data: bincode::serialize(&TokenInstruction::Burn { amount }).unwrap_or_default(),
        }
    }

    /// Close account instruction
    pub fn close_account(account: Pubkey, destination: Pubkey, owner: Pubkey) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(owner, true),
            ],
            data: bincode::serialize(&TokenInstruction::CloseAccount).unwrap_or_default(),
        }
    }

    /// Freeze account instruction
    pub fn freeze_account(account: Pubkey, mint: Pubkey, freeze_authority: Pubkey) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(freeze_authority, true),
            ],
            data: bincode::serialize(&TokenInstruction::FreezeAccount).unwrap_or_default(),
        }
    }

    /// Thaw account instruction
    pub fn thaw_account(account: Pubkey, mint: Pubkey, freeze_authority: Pubkey) -> Instruction {
        Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(freeze_authority, true),
            ],
            data: bincode::serialize(&TokenInstruction::ThawAccount).unwrap_or_default(),
        }
    }
}

/// Token program processor
pub struct TokenProgramProcessor;

impl TokenProgramProcessor {
    /// Process a token instruction
    pub fn process(
        data: &[u8],
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        let instruction: TokenInstruction =
            bincode::deserialize(data).map_err(|_| ProgramError::InvalidInstructionData)?;

        meter.consume(100)?;

        match instruction {
            TokenInstruction::InitializeMint {
                decimals,
                mint_authority,
                freeze_authority,
            } => Self::process_initialize_mint(
                accounts,
                decimals,
                mint_authority,
                freeze_authority,
                meter,
            ),
            TokenInstruction::InitializeAccount => {
                Self::process_initialize_account(accounts, meter)
            }
            TokenInstruction::InitializeAccount2 { owner } => {
                Self::process_initialize_account2(accounts, owner, meter)
            }
            TokenInstruction::Transfer { amount } => {
                Self::process_transfer(accounts, amount, meter)
            }
            TokenInstruction::TransferChecked { amount, decimals } => {
                Self::process_transfer_checked(accounts, amount, decimals, meter)
            }
            TokenInstruction::Approve { amount } => Self::process_approve(accounts, amount, meter),
            TokenInstruction::Revoke => Self::process_revoke(accounts, meter),
            TokenInstruction::MintTo { amount } => Self::process_mint_to(accounts, amount, meter),
            TokenInstruction::Burn { amount } => Self::process_burn(accounts, amount, meter),
            TokenInstruction::CloseAccount => Self::process_close_account(accounts, meter),
            TokenInstruction::FreezeAccount => Self::process_freeze_account(accounts, meter),
            TokenInstruction::ThawAccount => Self::process_thaw_account(accounts, meter),
            _ => Ok(()),
        }
    }

    /// Initialize a mint
    fn process_initialize_mint(
        accounts: &mut [&mut Account],
        decimals: u8,
        mint_authority: Pubkey,
        freeze_authority: Option<Pubkey>,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.is_empty() {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        if decimals > MAX_DECIMALS {
            return Err(ProgramError::InvalidDecimals);
        }

        let mint_account = &mut accounts[0];

        // Check not already initialized
        if mint_account.data.len() >= Mint::SIZE {
            if let Ok(existing) = Mint::from_bytes(&mint_account.data) {
                if existing.is_initialized {
                    return Err(ProgramError::AccountAlreadyInitialized);
                }
            }
        }

        let mint = Mint::new(decimals, mint_authority, freeze_authority);
        mint_account.data = mint.to_bytes();

        Ok(())
    }

    /// Initialize a token account
    fn process_initialize_account(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let token_account = &mut accounts[0];
        let mint_account = &accounts[1];
        let owner = accounts[2].key;

        // Verify mint is initialized
        let mint = Mint::from_bytes(&mint_account.data)?;
        if !mint.is_initialized {
            return Err(ProgramError::UninitializedMint);
        }

        let account = TokenAccount::new(mint_account.key, owner);
        token_account.data = account.to_bytes();
        token_account.owner = TOKEN_PROGRAM_ID;

        Ok(())
    }

    /// Initialize a token account with explicit owner
    fn process_initialize_account2(
        accounts: &mut [&mut Account],
        owner: Pubkey,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let token_account = &mut accounts[0];
        let mint_account = &accounts[1];

        // Verify mint is initialized
        let mint = Mint::from_bytes(&mint_account.data)?;
        if !mint.is_initialized {
            return Err(ProgramError::UninitializedMint);
        }

        let account = TokenAccount::new(mint_account.key, owner);
        token_account.data = account.to_bytes();
        token_account.owner = TOKEN_PROGRAM_ID;

        Ok(())
    }

    /// Transfer tokens
    fn process_transfer(
        accounts: &mut [&mut Account],
        amount: u64,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let source_account = &mut accounts[0];
        let dest_account = &mut accounts[1];
        let authority = &accounts[2];

        let mut source = TokenAccount::from_bytes(&source_account.data)?;
        let mut dest = TokenAccount::from_bytes(&dest_account.data)?;

        // Check frozen
        if source.is_frozen() || dest.is_frozen() {
            return Err(ProgramError::AccountFrozen);
        }

        // Verify authority
        if source.owner != authority.key {
            // Check delegate
            match source.delegate {
                Some(delegate) if delegate == authority.key => {
                    if source.delegated_amount < amount {
                        return Err(ProgramError::InsufficientDelegatedFunds);
                    }
                    source.delegated_amount -= amount;
                }
                _ => return Err(ProgramError::InvalidAccountOwner),
            }
        }

        // Check same mint
        if source.mint != dest.mint {
            return Err(ProgramError::MintMismatch);
        }

        // Check balance
        if source.amount < amount {
            return Err(ProgramError::InsufficientFunds);
        }

        // Transfer
        source.amount = source
            .amount
            .checked_sub(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        dest.amount = dest
            .amount
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        source_account.data = source.to_bytes();
        dest_account.data = dest.to_bytes();

        Ok(())
    }

    /// Transfer with decimal check
    fn process_transfer_checked(
        accounts: &mut [&mut Account],
        amount: u64,
        decimals: u8,
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 4 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Verify decimals match mint
        let mint = Mint::from_bytes(&accounts[1].data)?;
        if mint.decimals != decimals {
            return Err(ProgramError::InvalidDecimals);
        }

        // Reorder accounts for transfer
        let mut reordered = vec![accounts[0], accounts[2], accounts[3]];
        Self::process_transfer(&mut reordered, amount, meter)
    }

    /// Approve delegate
    fn process_approve(
        accounts: &mut [&mut Account],
        amount: u64,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let source_account = &mut accounts[0];
        let delegate = accounts[1].key;
        let owner = &accounts[2];

        let mut source = TokenAccount::from_bytes(&source_account.data)?;

        // Verify owner
        if source.owner != owner.key {
            return Err(ProgramError::InvalidAccountOwner);
        }

        source.delegate = Some(delegate);
        source.delegated_amount = amount;

        source_account.data = source.to_bytes();

        Ok(())
    }

    /// Revoke delegate
    fn process_revoke(accounts: &mut [&mut Account], _meter: &ComputeMeter) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let source_account = &mut accounts[0];
        let owner = &accounts[1];

        let mut source = TokenAccount::from_bytes(&source_account.data)?;

        // Verify owner
        if source.owner != owner.key {
            return Err(ProgramError::InvalidAccountOwner);
        }

        source.delegate = None;
        source.delegated_amount = 0;

        source_account.data = source.to_bytes();

        Ok(())
    }

    /// Mint tokens to account
    fn process_mint_to(
        accounts: &mut [&mut Account],
        amount: u64,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let mint_account = &mut accounts[0];
        let dest_account = &mut accounts[1];
        let authority = &accounts[2];

        let mut mint = Mint::from_bytes(&mint_account.data)?;
        let mut dest = TokenAccount::from_bytes(&dest_account.data)?;

        // Verify mint authority
        match mint.mint_authority {
            Some(auth) if auth == authority.key => {}
            _ => return Err(ProgramError::InvalidMintAuthority),
        }

        // Verify same mint
        if dest.mint != mint_account.key {
            return Err(ProgramError::MintMismatch);
        }

        // Check frozen
        if dest.is_frozen() {
            return Err(ProgramError::AccountFrozen);
        }

        // Mint
        mint.supply = mint
            .supply
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        dest.amount = dest
            .amount
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        mint_account.data = mint.to_bytes();
        dest_account.data = dest.to_bytes();

        Ok(())
    }

    /// Burn tokens
    fn process_burn(
        accounts: &mut [&mut Account],
        amount: u64,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let source_account = &mut accounts[0];
        let mint_account = &mut accounts[1];
        let owner = &accounts[2];

        let mut source = TokenAccount::from_bytes(&source_account.data)?;
        let mut mint = Mint::from_bytes(&mint_account.data)?;

        // Verify owner
        if source.owner != owner.key {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // Verify same mint
        if source.mint != mint_account.key {
            return Err(ProgramError::MintMismatch);
        }

        // Check balance
        if source.amount < amount {
            return Err(ProgramError::InsufficientFunds);
        }

        // Burn
        source.amount = source
            .amount
            .checked_sub(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        mint.supply = mint
            .supply
            .checked_sub(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        source_account.data = source.to_bytes();
        mint_account.data = mint.to_bytes();

        Ok(())
    }

    /// Close token account
    fn process_close_account(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let source_account = &mut accounts[0];
        let dest_account = &mut accounts[1];
        let owner = &accounts[2];

        let source = TokenAccount::from_bytes(&source_account.data)?;

        // Verify owner
        if source.owner != owner.key {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // Must have zero balance
        if source.amount != 0 {
            return Err(ProgramError::NonZeroBalance);
        }

        // Transfer lamports to destination
        let lamports = source_account.lamports;
        dest_account.lamports = dest_account
            .lamports
            .checked_add(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        // Clear account
        source_account.lamports = 0;
        source_account.data.clear();
        source_account.owner = crate::native_programs::SYSTEM_PROGRAM_ID;

        Ok(())
    }

    /// Freeze account
    fn process_freeze_account(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let token_account = &mut accounts[0];
        let mint_account = &accounts[1];
        let authority = &accounts[2];

        let mut account = TokenAccount::from_bytes(&token_account.data)?;
        let mint = Mint::from_bytes(&mint_account.data)?;

        // Verify freeze authority
        match mint.freeze_authority {
            Some(auth) if auth == authority.key => {}
            _ => return Err(ProgramError::InvalidFreezeAuthority),
        }

        account.state = AccountState::Frozen;
        token_account.data = account.to_bytes();

        Ok(())
    }

    /// Thaw account
    fn process_thaw_account(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let token_account = &mut accounts[0];
        let mint_account = &accounts[1];
        let authority = &accounts[2];

        let mut account = TokenAccount::from_bytes(&token_account.data)?;
        let mint = Mint::from_bytes(&mint_account.data)?;

        // Verify freeze authority
        match mint.freeze_authority {
            Some(auth) if auth == authority.key => {}
            _ => return Err(ProgramError::InvalidFreezeAuthority),
        }

        account.state = AccountState::Initialized;
        token_account.data = account.to_bytes();

        Ok(())
    }
}

/// Associated Token Account derivation
pub struct AssociatedTokenAccount;

impl AssociatedTokenAccount {
    /// Derive ATA address
    pub fn derive_address(wallet: &Pubkey, mint: &Pubkey) -> ProgramResult<Pubkey> {
        let seeds: &[&[u8]] = &[&wallet.0, &TOKEN_PROGRAM_ID.0, &mint.0];

        let pda = PdaDerivation::find_program_address(seeds, &ATA_PROGRAM_ID)?;
        Ok(pda.address)
    }

    /// Create ATA instruction
    pub fn create_instruction(
        payer: Pubkey,
        wallet: Pubkey,
        mint: Pubkey,
    ) -> ProgramResult<Instruction> {
        let ata = Self::derive_address(&wallet, &mint)?;

        Ok(Instruction {
            program_id: ATA_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(wallet, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(crate::native_programs::SYSTEM_PROGRAM_ID, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            data: vec![0], // Create instruction
        })
    }

    /// Create ATA idempotent instruction (doesn't fail if exists)
    pub fn create_idempotent_instruction(
        payer: Pubkey,
        wallet: Pubkey,
        mint: Pubkey,
    ) -> ProgramResult<Instruction> {
        let ata = Self::derive_address(&wallet, &mint)?;

        Ok(Instruction {
            program_id: ATA_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(wallet, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(crate::native_programs::SYSTEM_PROGRAM_ID, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            data: vec![1], // Create idempotent instruction
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mint_serialization() {
        let mint = Mint::new(9, Pubkey::new([1u8; 32]), None);
        let bytes = mint.to_bytes();
        let recovered = Mint::from_bytes(&bytes).unwrap();

        assert_eq!(mint.decimals, recovered.decimals);
        assert_eq!(mint.mint_authority, recovered.mint_authority);
        assert_eq!(mint.supply, recovered.supply);
        assert!(recovered.is_initialized);
    }

    #[test]
    fn test_token_account_serialization() {
        let account = TokenAccount::new(Pubkey::new([1u8; 32]), Pubkey::new([2u8; 32]));
        let bytes = account.to_bytes();
        let recovered = TokenAccount::from_bytes(&bytes).unwrap();

        assert_eq!(account.mint, recovered.mint);
        assert_eq!(account.owner, recovered.owner);
        assert_eq!(account.amount, recovered.amount);
        assert_eq!(account.state, AccountState::Initialized);
    }

    #[test]
    fn test_ata_derivation() {
        let wallet = Pubkey::new([1u8; 32]);
        let mint = Pubkey::new([2u8; 32]);

        let ata1 = AssociatedTokenAccount::derive_address(&wallet, &mint).unwrap();
        let ata2 = AssociatedTokenAccount::derive_address(&wallet, &mint).unwrap();

        // Same inputs = same output
        assert_eq!(ata1, ata2);

        // Different mint = different ATA
        let other_mint = Pubkey::new([3u8; 32]);
        let ata3 = AssociatedTokenAccount::derive_address(&wallet, &other_mint).unwrap();
        assert_ne!(ata1, ata3);
    }

    #[test]
    fn test_transfer_instruction() {
        let source = Pubkey::new([1u8; 32]);
        let dest = Pubkey::new([2u8; 32]);
        let authority = Pubkey::new([3u8; 32]);

        let ix = TokenProgram::transfer(source, dest, authority, 1000);

        assert_eq!(ix.program_id, TOKEN_PROGRAM_ID);
        assert_eq!(ix.accounts.len(), 3);
        assert!(ix.accounts[2].is_signer);
    }

    #[test]
    fn test_mint_to_instruction() {
        let mint = Pubkey::new([1u8; 32]);
        let dest = Pubkey::new([2u8; 32]);
        let authority = Pubkey::new([3u8; 32]);

        let ix = TokenProgram::mint_to(mint, dest, authority, 1000000);

        assert_eq!(ix.program_id, TOKEN_PROGRAM_ID);
        assert_eq!(ix.accounts.len(), 3);
    }

    #[test]
    fn test_account_state() {
        let mut account = TokenAccount::new(Pubkey::new([1u8; 32]), Pubkey::new([2u8; 32]));

        assert!(!account.is_frozen());

        account.state = AccountState::Frozen;
        assert!(account.is_frozen());
    }
}
