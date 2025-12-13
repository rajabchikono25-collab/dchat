//! Solana Account Structures
//!
//! Account types and system program integration.

use dchat_core::error::Result;
use serde::{Deserialize, Serialize};

use super::transaction::{AccountMeta, Instruction};
use crate::wallet::solana_compat::SolanaAddress;

/// System program ID
pub const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";

/// System program helper
pub struct SystemProgram;

impl SystemProgram {
    /// Get system program ID
    pub fn id() -> Result<SolanaAddress> {
        SolanaAddress::from_base58(SYSTEM_PROGRAM_ID)
    }

    /// Create account instruction
    pub fn create_account(
        payer: &SolanaAddress,
        new_account: &SolanaAddress,
        lamports: u64,
        space: u64,
        owner: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = Self::id()?;

        let mut data = vec![0, 0, 0, 0]; // CreateAccount instruction (u32)
        data.extend_from_slice(&lamports.to_le_bytes());
        data.extend_from_slice(&space.to_le_bytes());
        data.extend_from_slice(owner.as_bytes());

        let accounts = vec![
            AccountMeta::signer_writable(payer.clone()),
            AccountMeta::signer_writable(new_account.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Transfer lamports instruction
    pub fn transfer(
        from: &SolanaAddress,
        to: &SolanaAddress,
        lamports: u64,
    ) -> Result<Instruction> {
        let program_id = Self::id()?;

        let mut data = vec![2, 0, 0, 0]; // Transfer instruction (u32)
        data.extend_from_slice(&lamports.to_le_bytes());

        let accounts = vec![
            AccountMeta::signer_writable(from.clone()),
            AccountMeta::writable(to.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Allocate space instruction
    pub fn allocate(account: &SolanaAddress, space: u64) -> Result<Instruction> {
        let program_id = Self::id()?;

        let mut data = vec![8, 0, 0, 0]; // Allocate instruction (u32)
        data.extend_from_slice(&space.to_le_bytes());

        let accounts = vec![AccountMeta::signer_writable(account.clone())];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Assign account to program instruction
    pub fn assign(account: &SolanaAddress, owner: &SolanaAddress) -> Result<Instruction> {
        let program_id = Self::id()?;

        let mut data = vec![1, 0, 0, 0]; // Assign instruction (u32)
        data.extend_from_slice(owner.as_bytes());

        let accounts = vec![AccountMeta::signer_writable(account.clone())];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create account with seed instruction
    pub fn create_account_with_seed(
        payer: &SolanaAddress,
        new_account: &SolanaAddress,
        base: &SolanaAddress,
        seed: &str,
        lamports: u64,
        space: u64,
        owner: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = Self::id()?;

        let mut data = vec![3, 0, 0, 0]; // CreateAccountWithSeed instruction (u32)
        data.extend_from_slice(base.as_bytes());

        // Seed string (length-prefixed)
        let seed_bytes = seed.as_bytes();
        data.extend_from_slice(&(seed_bytes.len() as u64).to_le_bytes());
        data.extend_from_slice(seed_bytes);

        data.extend_from_slice(&lamports.to_le_bytes());
        data.extend_from_slice(&space.to_le_bytes());
        data.extend_from_slice(owner.as_bytes());

        let accounts = vec![
            AccountMeta::signer_writable(payer.clone()),
            AccountMeta::writable(new_account.clone()),
            AccountMeta::signer_readonly(base.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Advance nonce account instruction
    pub fn advance_nonce(
        nonce_account: &SolanaAddress,
        nonce_authority: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = Self::id()?;
        let recent_blockhashes = SolanaAddress::from_base58(super::RECENT_BLOCKHASHES_ID)?;

        let data = vec![4, 0, 0, 0]; // AdvanceNonceAccount instruction (u32)

        let accounts = vec![
            AccountMeta::writable(nonce_account.clone()),
            AccountMeta::readonly(recent_blockhashes),
            AccountMeta::signer_readonly(nonce_authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Initialize nonce account instruction
    pub fn initialize_nonce(
        nonce_account: &SolanaAddress,
        nonce_authority: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = Self::id()?;
        let rent_sysvar = SolanaAddress::from_base58(super::RENT_SYSVAR_ID)?;
        let recent_blockhashes = SolanaAddress::from_base58(super::RECENT_BLOCKHASHES_ID)?;

        let mut data = vec![6, 0, 0, 0]; // InitializeNonceAccount instruction (u32)
        data.extend_from_slice(nonce_authority.as_bytes());

        let accounts = vec![
            AccountMeta::writable(nonce_account.clone()),
            AccountMeta::readonly(recent_blockhashes),
            AccountMeta::readonly(rent_sysvar),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Withdraw from nonce account instruction
    pub fn withdraw_nonce(
        nonce_account: &SolanaAddress,
        nonce_authority: &SolanaAddress,
        destination: &SolanaAddress,
        lamports: u64,
    ) -> Result<Instruction> {
        let program_id = Self::id()?;
        let rent_sysvar = SolanaAddress::from_base58(super::RENT_SYSVAR_ID)?;
        let recent_blockhashes = SolanaAddress::from_base58(super::RECENT_BLOCKHASHES_ID)?;

        let mut data = vec![5, 0, 0, 0]; // WithdrawNonceAccount instruction (u32)
        data.extend_from_slice(&lamports.to_le_bytes());

        let accounts = vec![
            AccountMeta::writable(nonce_account.clone()),
            AccountMeta::writable(destination.clone()),
            AccountMeta::readonly(recent_blockhashes),
            AccountMeta::readonly(rent_sysvar),
            AccountMeta::signer_readonly(nonce_authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Transfer with seed instruction
    pub fn transfer_with_seed(
        from: &SolanaAddress,
        from_base: &SolanaAddress,
        seed: &str,
        from_owner: &SolanaAddress,
        to: &SolanaAddress,
        lamports: u64,
    ) -> Result<Instruction> {
        let program_id = Self::id()?;

        let mut data = vec![11, 0, 0, 0]; // TransferWithSeed instruction (u32)
        data.extend_from_slice(&lamports.to_le_bytes());

        // Seed string
        let seed_bytes = seed.as_bytes();
        data.extend_from_slice(&(seed_bytes.len() as u64).to_le_bytes());
        data.extend_from_slice(seed_bytes);

        data.extend_from_slice(from_owner.as_bytes());

        let accounts = vec![
            AccountMeta::writable(from.clone()),
            AccountMeta::signer_readonly(from_base.clone()),
            AccountMeta::writable(to.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }
}

/// Account info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    /// Account public key
    pub pubkey: SolanaAddress,
    /// Lamports balance
    pub lamports: u64,
    /// Owner program
    pub owner: SolanaAddress,
    /// Account data
    pub data: Vec<u8>,
    /// Is executable
    pub executable: bool,
    /// Rent epoch
    pub rent_epoch: u64,
}

impl AccountInfo {
    /// Check if account is system program owned
    pub fn is_system_owned(&self) -> bool {
        self.owner.to_base58() == SYSTEM_PROGRAM_ID
    }

    /// Check if account is empty (no data, minimal lamports)
    pub fn is_empty(&self) -> bool {
        self.data.is_empty() && self.lamports == 0
    }
}

/// Account metadata for transactions
#[derive(Debug, Clone)]
pub struct SolanaAccount {
    /// Account address
    pub address: SolanaAddress,
    /// Is this account a signer
    pub is_signer: bool,
    /// Is this account writable
    pub is_writable: bool,
}

impl SolanaAccount {
    /// Create a new signer writable account
    pub fn signer_writable(address: SolanaAddress) -> Self {
        Self {
            address,
            is_signer: true,
            is_writable: true,
        }
    }

    /// Create a new writable account (not signer)
    pub fn writable(address: SolanaAddress) -> Self {
        Self {
            address,
            is_signer: false,
            is_writable: true,
        }
    }

    /// Create a new readonly account
    pub fn readonly(address: SolanaAddress) -> Self {
        Self {
            address,
            is_signer: false,
            is_writable: false,
        }
    }

    /// Convert to AccountMeta
    pub fn to_meta(&self) -> AccountMeta {
        AccountMeta {
            pubkey: self.address.clone(),
            is_signer: self.is_signer,
            is_writable: self.is_writable,
        }
    }
}

/// Rent calculation
pub struct Rent;

impl Rent {
    /// Rent sysvar ID
    pub const SYSVAR_ID: &'static str = "SysvarRent111111111111111111111111111111111";

    /// Lamports per byte-year
    pub const LAMPORTS_PER_BYTE_YEAR: u64 = 3480;

    /// Exemption threshold (years)
    pub const EXEMPTION_THRESHOLD: f64 = 2.0;

    /// Account storage overhead
    pub const ACCOUNT_STORAGE_OVERHEAD: u64 = 128;

    /// Calculate minimum balance for rent exemption
    pub fn minimum_balance(data_len: usize) -> u64 {
        let bytes = data_len as u64 + Self::ACCOUNT_STORAGE_OVERHEAD;
        bytes * Self::LAMPORTS_PER_BYTE_YEAR as u64 * Self::EXEMPTION_THRESHOLD as u64
    }

    /// Get rent sysvar address
    pub fn sysvar_id() -> Result<SolanaAddress> {
        SolanaAddress::from_base58(Self::SYSVAR_ID)
    }
}

/// System instruction types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SystemInstruction {
    CreateAccount = 0,
    Assign = 1,
    Transfer = 2,
    CreateAccountWithSeed = 3,
    AdvanceNonceAccount = 4,
    WithdrawNonceAccount = 5,
    InitializeNonceAccount = 6,
    AuthorizeNonceAccount = 7,
    Allocate = 8,
    AllocateWithSeed = 9,
    AssignWithSeed = 10,
    TransferWithSeed = 11,
    UpgradeNonceAccount = 12,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_program_id() {
        let id = SystemProgram::id().unwrap();
        assert_eq!(id.to_base58(), SYSTEM_PROGRAM_ID);
    }

    #[test]
    fn test_transfer_instruction() {
        let from = SolanaAddress::from_bytes(&[1u8; 32]).unwrap();
        let to = SolanaAddress::from_bytes(&[2u8; 32]).unwrap();

        let ix = SystemProgram::transfer(&from, &to, 1_000_000).unwrap();

        assert_eq!(ix.accounts.len(), 2);
        assert_eq!(&ix.data[0..4], &[2, 0, 0, 0]); // Transfer instruction
    }

    #[test]
    fn test_rent_calculation() {
        // Token account is 165 bytes
        let rent = Rent::minimum_balance(165);
        assert!(rent > 0);
    }

    #[test]
    fn test_account_meta_conversion() {
        let addr = SolanaAddress::from_bytes(&[1u8; 32]).unwrap();
        let account = SolanaAccount::signer_writable(addr);
        let meta = account.to_meta();

        assert!(meta.is_signer);
        assert!(meta.is_writable);
    }
}
