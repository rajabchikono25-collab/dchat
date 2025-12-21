//! System program for fundamental account operations

use serde::{Deserialize, Serialize};

use crate::account::{Account, AccountMeta, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::instruction::Instruction;
use crate::metering::ComputeMeter;

/// System program ID
pub const SYSTEM_PROGRAM_ID: Pubkey = crate::native_programs::SYSTEM_PROGRAM_ID;

/// Minimum lamports for rent exemption per byte
pub const LAMPORTS_PER_BYTE_YEAR: u64 = 3480;

/// Epochs per year (assuming 2-day epochs)
pub const EPOCHS_PER_YEAR: u64 = 182;

/// Minimum account balance for rent exemption
pub fn minimum_balance(data_len: usize) -> u64 {
    // Account overhead: 128 bytes
    let total_size = (data_len + 128) as u64;
    // 2 years rent exemption
    total_size * LAMPORTS_PER_BYTE_YEAR * 2 / EPOCHS_PER_YEAR
}

/// System instruction types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemInstruction {
    /// Create a new account
    CreateAccount {
        /// Lamports to transfer to new account
        lamports: u64,
        /// Space in bytes to allocate
        space: u64,
        /// Owner program of new account
        owner: Pubkey,
    },

    /// Assign account to a program
    Assign {
        /// New owner program
        owner: Pubkey,
    },

    /// Transfer lamports between accounts
    Transfer {
        /// Amount to transfer
        lamports: u64,
    },

    /// Create account with seed
    CreateAccountWithSeed {
        /// Base pubkey
        base: Pubkey,
        /// Seed string
        seed: String,
        /// Lamports to transfer
        lamports: u64,
        /// Space to allocate
        space: u64,
        /// Owner program
        owner: Pubkey,
    },

    /// Allocate space to an account
    Allocate {
        /// Space in bytes
        space: u64,
    },

    /// Allocate space with seed
    AllocateWithSeed {
        /// Base pubkey
        base: Pubkey,
        /// Seed string
        seed: String,
        /// Space in bytes
        space: u64,
        /// Owner program
        owner: Pubkey,
    },

    /// Assign account with seed
    AssignWithSeed {
        /// Base pubkey
        base: Pubkey,
        /// Seed string
        seed: String,
        /// New owner program
        owner: Pubkey,
    },

    /// Transfer lamports with seed
    TransferWithSeed {
        /// Amount to transfer
        lamports: u64,
        /// Seed for source address derivation
        from_seed: String,
        /// Source owner
        from_owner: Pubkey,
    },

    /// Advance nonce account
    AdvanceNonceAccount,

    /// Withdraw from nonce account
    WithdrawNonceAccount {
        /// Amount to withdraw
        lamports: u64,
    },

    /// Initialize nonce account
    InitializeNonceAccount {
        /// Authority for the nonce account
        authority: Pubkey,
    },

    /// Authorize new nonce authority
    AuthorizeNonceAccount {
        /// New authority
        new_authority: Pubkey,
    },

    /// Upgrade nonce account version
    UpgradeNonceAccount,
}

impl SystemInstruction {
    /// Serialize instruction to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize instruction from bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidInstructionData)
    }
}

/// System program implementation
pub struct SystemProgram;

impl SystemProgram {
    /// Create account instruction
    pub fn create_account(
        from: Pubkey,
        to: Pubkey,
        lamports: u64,
        space: u64,
        owner: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: SYSTEM_PROGRAM_ID,
            accounts: vec![AccountMeta::new(from, true), AccountMeta::new(to, true)],
            data: SystemInstruction::CreateAccount {
                lamports,
                space,
                owner,
            }
            .to_bytes(),
        }
    }

    /// Transfer instruction
    pub fn transfer(from: Pubkey, to: Pubkey, lamports: u64) -> Instruction {
        Instruction {
            program_id: SYSTEM_PROGRAM_ID,
            accounts: vec![AccountMeta::new(from, true), AccountMeta::new(to, false)],
            data: SystemInstruction::Transfer { lamports }.to_bytes(),
        }
    }

    /// Assign instruction
    pub fn assign(account: Pubkey, owner: Pubkey) -> Instruction {
        Instruction {
            program_id: SYSTEM_PROGRAM_ID,
            accounts: vec![AccountMeta::new(account, true)],
            data: SystemInstruction::Assign { owner }.to_bytes(),
        }
    }

    /// Allocate instruction
    pub fn allocate(account: Pubkey, space: u64) -> Instruction {
        Instruction {
            program_id: SYSTEM_PROGRAM_ID,
            accounts: vec![AccountMeta::new(account, true)],
            data: SystemInstruction::Allocate { space }.to_bytes(),
        }
    }

    /// Create account with seed instruction
    pub fn create_account_with_seed(
        from: Pubkey,
        to: Pubkey,
        base: Pubkey,
        seed: String,
        lamports: u64,
        space: u64,
        owner: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: SYSTEM_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(from, true),
                AccountMeta::new(to, false),
                AccountMeta::new_readonly(base, true),
            ],
            data: SystemInstruction::CreateAccountWithSeed {
                base,
                seed,
                lamports,
                space,
                owner,
            }
            .to_bytes(),
        }
    }

    /// Initialize nonce account
    pub fn initialize_nonce_account(nonce_account: Pubkey, authority: Pubkey) -> Instruction {
        Instruction {
            program_id: SYSTEM_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(nonce_account, false),
                AccountMeta::new_readonly(
                    crate::native_programs::SYSVAR_RECENT_BLOCKHASHES_ID,
                    false,
                ),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_RENT_ID, false),
            ],
            data: SystemInstruction::InitializeNonceAccount { authority }.to_bytes(),
        }
    }

    /// Advance nonce
    pub fn advance_nonce_account(nonce_account: Pubkey, authority: Pubkey) -> Instruction {
        Instruction {
            program_id: SYSTEM_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(nonce_account, false),
                AccountMeta::new_readonly(
                    crate::native_programs::SYSVAR_RECENT_BLOCKHASHES_ID,
                    false,
                ),
                AccountMeta::new_readonly(authority, true),
            ],
            data: SystemInstruction::AdvanceNonceAccount.to_bytes(),
        }
    }
}

/// System program processor
pub struct SystemProgramProcessor;

impl SystemProgramProcessor {
    /// Process a system instruction
    pub fn process(
        instruction: &SystemInstruction,
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        // Consume base cost
        meter.consume(100)?;

        match instruction {
            SystemInstruction::CreateAccount {
                lamports,
                space,
                owner,
            } => Self::process_create_account(accounts, *lamports, *space, owner, meter),
            SystemInstruction::Transfer { lamports } => {
                Self::process_transfer(accounts, *lamports, meter)
            }
            SystemInstruction::Assign { owner } => Self::process_assign(accounts, owner, meter),
            SystemInstruction::Allocate { space } => {
                Self::process_allocate(accounts, *space, meter)
            }
            SystemInstruction::CreateAccountWithSeed {
                lamports,
                space,
                owner,
                ..
            } => Self::process_create_account(accounts, *lamports, *space, owner, meter),
            SystemInstruction::AdvanceNonceAccount => Self::process_advance_nonce(accounts, meter),
            SystemInstruction::WithdrawNonceAccount { lamports } => {
                Self::process_withdraw_nonce(accounts, *lamports, meter)
            }
            SystemInstruction::InitializeNonceAccount { authority } => {
                Self::process_initialize_nonce(accounts, authority, meter)
            }
            SystemInstruction::AuthorizeNonceAccount { new_authority } => {
                Self::process_authorize_nonce(accounts, new_authority, meter)
            }
            _ => {
                // Other instructions
                Ok(())
            }
        }
    }

    /// Create a new account
    fn process_create_account(
        accounts: &mut [&mut Account],
        lamports: u64,
        space: u64,
        owner: &Pubkey,
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Use split_at_mut to avoid double mutable borrow
        let (first, rest) = accounts.split_at_mut(1);
        let from = &mut first[0];
        let to = &mut rest[0];

        // Consume cost based on space
        meter.consume(space * 10)?;

        // Check from has enough lamports
        if from.lamports < lamports {
            return Err(ProgramError::InsufficientFunds);
        }

        // Check to account is not already initialized
        if !to.data.is_empty() || to.lamports > 0 {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        // Check space limit
        if space > crate::MAX_ACCOUNT_SIZE as u64 {
            return Err(ProgramError::InvalidAccountDataSize);
        }

        // Check minimum balance for rent exemption
        let min_balance = minimum_balance(space as usize);
        if lamports < min_balance {
            return Err(ProgramError::AccountNotRentExempt);
        }

        // Transfer lamports
        from.lamports = from
            .lamports
            .checked_sub(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        to.lamports = to
            .lamports
            .checked_add(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        // Allocate space
        to.data.set_from_bytes(vec![0u8; space as usize]);

        // Set owner
        to.owner = *owner;

        Ok(())
    }

    /// Transfer lamports between accounts
    fn process_transfer(
        accounts: &mut [&mut Account],
        lamports: u64,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Use split_at_mut to avoid double mutable borrow
        let (first, rest) = accounts.split_at_mut(1);
        let from = &mut first[0];
        let to = &mut rest[0];

        // Check from has enough lamports
        if from.lamports < lamports {
            return Err(ProgramError::InsufficientFunds);
        }

        // Check from is owned by system program (or is the signer)
        if from.owner != SYSTEM_PROGRAM_ID {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // Transfer
        from.lamports = from
            .lamports
            .checked_sub(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        to.lamports = to
            .lamports
            .checked_add(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        Ok(())
    }

    /// Assign account to a program
    fn process_assign(
        accounts: &mut [&mut Account],
        owner: &Pubkey,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.is_empty() {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let account = &mut accounts[0];

        // Can only assign if currently owned by system program
        if account.owner != SYSTEM_PROGRAM_ID {
            return Err(ProgramError::InvalidAccountOwner);
        }

        account.owner = *owner;

        Ok(())
    }

    /// Allocate space to an account
    fn process_allocate(
        accounts: &mut [&mut Account],
        space: u64,
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.is_empty() {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let account = &mut accounts[0];

        // Consume cost based on space
        meter.consume(space * 10)?;

        // Can only allocate if owned by system program
        if account.owner != SYSTEM_PROGRAM_ID {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // Can only allocate if currently empty
        if !account.data.is_empty() {
            return Err(ProgramError::AccountDataNotEmpty);
        }

        // Check space limit
        if space > crate::MAX_ACCOUNT_SIZE as u64 {
            return Err(ProgramError::InvalidAccountDataSize);
        }

        account.data.set_from_bytes(vec![0u8; space as usize]);

        Ok(())
    }

    /// Advance nonce account
    fn process_advance_nonce(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.is_empty() {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        meter.consume(50)?;

        let nonce_account = &mut accounts[0];

        // Verify it's a nonce account
        if nonce_account.data.len() < NonceState::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut state = NonceState::from_bytes(nonce_account.data.as_slice())?;

        if !state.initialized {
            return Err(ProgramError::AccountNotInitialized);
        }

        // Generate new nonce
        let mut hasher = blake3::Hasher::new();
        hasher.update(&state.nonce);
        hasher.update(&state.fee_calculator_lamports_per_signature.to_le_bytes());
        state.nonce = hasher.finalize().into();

        nonce_account.data.set_from_bytes(state.to_bytes());

        Ok(())
    }

    /// Withdraw from nonce account
    fn process_withdraw_nonce(
        accounts: &mut [&mut Account],
        lamports: u64,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Use split_at_mut to avoid double mutable borrow
        let (first, rest) = accounts.split_at_mut(1);
        let nonce_account = &mut first[0];
        let to = &mut rest[0];

        // Verify nonce account
        if nonce_account.data.len() < NonceState::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }

        // Check rent exemption after withdrawal
        let min_balance = minimum_balance(nonce_account.data.len());
        if nonce_account
            .lamports
            .checked_sub(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?
            < min_balance
        {
            return Err(ProgramError::AccountNotRentExempt);
        }

        // Transfer
        nonce_account.lamports = nonce_account
            .lamports
            .checked_sub(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        to.lamports = to
            .lamports
            .checked_add(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        Ok(())
    }

    /// Initialize nonce account
    fn process_initialize_nonce(
        accounts: &mut [&mut Account],
        authority: &Pubkey,
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.is_empty() {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        meter.consume(100)?;

        let nonce_account = &mut accounts[0];

        // Check not already initialized
        if !nonce_account.data.is_empty() {
            let existing = NonceState::from_bytes(nonce_account.data.as_slice())?;
            if existing.initialized {
                return Err(ProgramError::AccountAlreadyInitialized);
            }
        }

        // Ensure enough space
        if nonce_account.data.len() < NonceState::SIZE {
            nonce_account.data.resize(NonceState::SIZE, 0);
        }

        // Initialize state
        let state = NonceState {
            initialized: true,
            authority: *authority,
            nonce: [0u8; 32], // Will be set on first advance
            fee_calculator_lamports_per_signature: 5000,
        };

        nonce_account.data.set_from_bytes(state.to_bytes());

        Ok(())
    }

    /// Authorize new nonce authority
    fn process_authorize_nonce(
        accounts: &mut [&mut Account],
        new_authority: &Pubkey,
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.is_empty() {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let nonce_account = &mut accounts[0];

        if nonce_account.data.len() < NonceState::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut state = NonceState::from_bytes(nonce_account.data.as_slice())?;

        if !state.initialized {
            return Err(ProgramError::AccountNotInitialized);
        }

        state.authority = *new_authority;
        nonce_account.data.set_from_bytes(state.to_bytes());

        Ok(())
    }
}

/// Nonce account state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonceState {
    /// Whether initialized
    pub initialized: bool,
    /// Authority that can advance nonce
    pub authority: Pubkey,
    /// Current nonce value
    pub nonce: [u8; 32],
    /// Fee calculator
    pub fee_calculator_lamports_per_signature: u64,
}

impl NonceState {
    /// Size of nonce state
    pub const SIZE: usize = 1 + 32 + 32 + 8; // initialized + authority + nonce + fee

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
}

/// Address derivation with seed
pub fn create_with_seed(base: &Pubkey, seed: &str, program_id: &Pubkey) -> ProgramResult<Pubkey> {
    if seed.len() > 32 {
        return Err(ProgramError::SeedTooLong);
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(&base.0);
    hasher.update(seed.as_bytes());
    hasher.update(&program_id.0);

    Ok(Pubkey::new(hasher.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimum_balance() {
        let balance_0 = minimum_balance(0);
        let balance_100 = minimum_balance(100);
        let balance_1000 = minimum_balance(1000);

        // Larger accounts need more lamports
        assert!(balance_100 > balance_0);
        assert!(balance_1000 > balance_100);
    }

    #[test]
    fn test_create_account_instruction() {
        let from = Pubkey::new([1u8; 32]);
        let to = Pubkey::new([2u8; 32]);
        let owner = Pubkey::new([3u8; 32]);

        let ix = SystemProgram::create_account(from, to, 1000000, 100, owner);

        assert_eq!(ix.program_id, SYSTEM_PROGRAM_ID);
        assert_eq!(ix.accounts.len(), 2);
        assert!(ix.accounts[0].is_signer);
        assert!(ix.accounts[0].is_writable);
        assert!(ix.accounts[1].is_signer);
        assert!(ix.accounts[1].is_writable);
    }

    #[test]
    fn test_transfer_instruction() {
        let from = Pubkey::new([1u8; 32]);
        let to = Pubkey::new([2u8; 32]);

        let ix = SystemProgram::transfer(from, to, 1000);

        assert_eq!(ix.program_id, SYSTEM_PROGRAM_ID);
        assert_eq!(ix.accounts.len(), 2);
        assert!(ix.accounts[0].is_signer);
        assert!(!ix.accounts[1].is_signer);
    }

    #[test]
    fn test_create_with_seed() {
        let base = Pubkey::new([1u8; 32]);
        let program = Pubkey::new([2u8; 32]);

        let addr1 = create_with_seed(&base, "test", &program).unwrap();
        let addr2 = create_with_seed(&base, "test", &program).unwrap();
        let addr3 = create_with_seed(&base, "other", &program).unwrap();

        // Same inputs = same output
        assert_eq!(addr1, addr2);
        // Different seed = different output
        assert_ne!(addr1, addr3);
    }

    #[test]
    fn test_nonce_state_serialization() {
        let state = NonceState {
            initialized: true,
            authority: Pubkey::new([1u8; 32]),
            nonce: [2u8; 32],
            fee_calculator_lamports_per_signature: 5000,
        };

        let bytes = state.to_bytes();
        let recovered = NonceState::from_bytes(&bytes).unwrap();

        assert_eq!(state.initialized, recovered.initialized);
        assert_eq!(state.authority, recovered.authority);
        assert_eq!(state.nonce, recovered.nonce);
        assert_eq!(
            state.fee_calculator_lamports_per_signature,
            recovered.fee_calculator_lamports_per_signature
        );
    }

    #[test]
    fn test_system_instruction_serialization() {
        let ix = SystemInstruction::CreateAccount {
            lamports: 1000000,
            space: 100,
            owner: Pubkey::new([1u8; 32]),
        };

        let bytes = ix.to_bytes();
        let recovered = SystemInstruction::from_bytes(&bytes).unwrap();

        assert_eq!(ix, recovered);
    }
}
