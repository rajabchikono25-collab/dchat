//! SPL Token Integration
//!
//! Complete SPL token support for dchat including:
//! - Token transfers
//! - Associated token accounts
//! - Mint/burn operations
//! - Token metadata

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};

use super::transaction::{AccountMeta, Instruction};
use crate::wallet::solana_compat::SolanaAddress;

/// SPL Token program ID
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Associated Token program ID  
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

/// Token 2022 program ID (Token Extensions)
pub const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// SPL Token helper
pub struct SplToken;

impl SplToken {
    /// Get the token program ID
    pub fn program_id() -> Result<SolanaAddress> {
        SolanaAddress::from_base58(TOKEN_PROGRAM_ID)
    }

    /// Get the associated token program ID
    pub fn associated_token_program_id() -> Result<SolanaAddress> {
        SolanaAddress::from_base58(ASSOCIATED_TOKEN_PROGRAM_ID)
    }

    /// Create associated token account instruction (convenience wrapper)
    pub fn create_associated_token_account_instruction(
        payer: &SolanaAddress,
        owner: &SolanaAddress,
        mint: &SolanaAddress,
    ) -> Result<Instruction> {
        AssociatedTokenAccount::create(payer, owner, mint)
    }

    /// Derive associated token address for an owner and mint
    pub fn get_associated_token_address(
        owner: &SolanaAddress,
        mint: &SolanaAddress,
    ) -> Result<SolanaAddress> {
        let token_program = Self::program_id()?;
        let ata_program = Self::associated_token_program_id()?;

        let seeds: &[&[u8]] = &[owner.as_bytes(), token_program.as_bytes(), mint.as_bytes()];

        let (pda, _) = SolanaAddress::derive_pda(seeds, &ata_program)?;
        Ok(pda)
    }

    // ============ Token Instructions ============

    /// Create InitializeMint instruction
    pub fn initialize_mint(
        mint: &SolanaAddress,
        mint_authority: &SolanaAddress,
        freeze_authority: Option<&SolanaAddress>,
        decimals: u8,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;
        let rent_sysvar = SolanaAddress::from_base58(super::RENT_SYSVAR_ID)?;

        let mut data = vec![0]; // InitializeMint instruction
        data.push(decimals);
        data.extend_from_slice(mint_authority.as_bytes());

        // Freeze authority (COption<Pubkey>)
        if let Some(freeze) = freeze_authority {
            data.push(1); // Some
            data.extend_from_slice(freeze.as_bytes());
        } else {
            data.push(0); // None
        }

        let accounts = vec![
            AccountMeta::writable(mint.clone()),
            AccountMeta::readonly(rent_sysvar),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create InitializeAccount instruction
    pub fn initialize_account(
        account: &SolanaAddress,
        mint: &SolanaAddress,
        owner: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;
        let rent_sysvar = SolanaAddress::from_base58(super::RENT_SYSVAR_ID)?;

        let data = vec![1]; // InitializeAccount instruction

        let accounts = vec![
            AccountMeta::writable(account.clone()),
            AccountMeta::readonly(mint.clone()),
            AccountMeta::readonly(owner.clone()),
            AccountMeta::readonly(rent_sysvar),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create Transfer instruction
    pub fn transfer(
        source: &SolanaAddress,
        destination: &SolanaAddress,
        authority: &SolanaAddress,
        amount: u64,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let mut data = vec![3]; // Transfer instruction
        data.extend_from_slice(&amount.to_le_bytes());

        let accounts = vec![
            AccountMeta::writable(source.clone()),
            AccountMeta::writable(destination.clone()),
            AccountMeta::signer_readonly(authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create TransferChecked instruction (with decimals verification)
    pub fn transfer_checked(
        source: &SolanaAddress,
        mint: &SolanaAddress,
        destination: &SolanaAddress,
        authority: &SolanaAddress,
        amount: u64,
        decimals: u8,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let mut data = vec![12]; // TransferChecked instruction
        data.extend_from_slice(&amount.to_le_bytes());
        data.push(decimals);

        let accounts = vec![
            AccountMeta::writable(source.clone()),
            AccountMeta::readonly(mint.clone()),
            AccountMeta::writable(destination.clone()),
            AccountMeta::signer_readonly(authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create MintTo instruction
    pub fn mint_to(
        mint: &SolanaAddress,
        destination: &SolanaAddress,
        mint_authority: &SolanaAddress,
        amount: u64,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let mut data = vec![7]; // MintTo instruction
        data.extend_from_slice(&amount.to_le_bytes());

        let accounts = vec![
            AccountMeta::writable(mint.clone()),
            AccountMeta::writable(destination.clone()),
            AccountMeta::signer_readonly(mint_authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create MintToChecked instruction
    pub fn mint_to_checked(
        mint: &SolanaAddress,
        destination: &SolanaAddress,
        mint_authority: &SolanaAddress,
        amount: u64,
        decimals: u8,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let mut data = vec![14]; // MintToChecked instruction
        data.extend_from_slice(&amount.to_le_bytes());
        data.push(decimals);

        let accounts = vec![
            AccountMeta::writable(mint.clone()),
            AccountMeta::writable(destination.clone()),
            AccountMeta::signer_readonly(mint_authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create Burn instruction
    pub fn burn(
        account: &SolanaAddress,
        mint: &SolanaAddress,
        authority: &SolanaAddress,
        amount: u64,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let mut data = vec![8]; // Burn instruction
        data.extend_from_slice(&amount.to_le_bytes());

        let accounts = vec![
            AccountMeta::writable(account.clone()),
            AccountMeta::writable(mint.clone()),
            AccountMeta::signer_readonly(authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create BurnChecked instruction
    pub fn burn_checked(
        account: &SolanaAddress,
        mint: &SolanaAddress,
        authority: &SolanaAddress,
        amount: u64,
        decimals: u8,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let mut data = vec![15]; // BurnChecked instruction
        data.extend_from_slice(&amount.to_le_bytes());
        data.push(decimals);

        let accounts = vec![
            AccountMeta::writable(account.clone()),
            AccountMeta::writable(mint.clone()),
            AccountMeta::signer_readonly(authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create Approve instruction (delegate)
    pub fn approve(
        source: &SolanaAddress,
        delegate: &SolanaAddress,
        owner: &SolanaAddress,
        amount: u64,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let mut data = vec![4]; // Approve instruction
        data.extend_from_slice(&amount.to_le_bytes());

        let accounts = vec![
            AccountMeta::writable(source.clone()),
            AccountMeta::readonly(delegate.clone()),
            AccountMeta::signer_readonly(owner.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create Revoke instruction
    pub fn revoke(source: &SolanaAddress, owner: &SolanaAddress) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let data = vec![5]; // Revoke instruction

        let accounts = vec![
            AccountMeta::writable(source.clone()),
            AccountMeta::signer_readonly(owner.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create SetAuthority instruction
    pub fn set_authority(
        account: &SolanaAddress,
        current_authority: &SolanaAddress,
        authority_type: AuthorityType,
        new_authority: Option<&SolanaAddress>,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let mut data = vec![6]; // SetAuthority instruction
        data.push(authority_type as u8);

        if let Some(new_auth) = new_authority {
            data.push(1); // Some
            data.extend_from_slice(new_auth.as_bytes());
        } else {
            data.push(0); // None
        }

        let accounts = vec![
            AccountMeta::writable(account.clone()),
            AccountMeta::signer_readonly(current_authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create CloseAccount instruction
    pub fn close_account(
        account: &SolanaAddress,
        destination: &SolanaAddress,
        owner: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let data = vec![9]; // CloseAccount instruction

        let accounts = vec![
            AccountMeta::writable(account.clone()),
            AccountMeta::writable(destination.clone()),
            AccountMeta::signer_readonly(owner.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create FreezeAccount instruction
    pub fn freeze_account(
        account: &SolanaAddress,
        mint: &SolanaAddress,
        freeze_authority: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let data = vec![10]; // FreezeAccount instruction

        let accounts = vec![
            AccountMeta::writable(account.clone()),
            AccountMeta::readonly(mint.clone()),
            AccountMeta::signer_readonly(freeze_authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create ThawAccount instruction
    pub fn thaw_account(
        account: &SolanaAddress,
        mint: &SolanaAddress,
        freeze_authority: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let data = vec![11]; // ThawAccount instruction

        let accounts = vec![
            AccountMeta::writable(account.clone()),
            AccountMeta::readonly(mint.clone()),
            AccountMeta::signer_readonly(freeze_authority.clone()),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create SyncNative instruction (for wrapped SOL)
    pub fn sync_native(account: &SolanaAddress) -> Result<Instruction> {
        let program_id = Self::program_id()?;

        let data = vec![17]; // SyncNative instruction

        let accounts = vec![AccountMeta::writable(account.clone())];

        Ok(Instruction::new(program_id, accounts, data))
    }
}

/// Associated Token Account helper
pub struct AssociatedTokenAccount;

impl AssociatedTokenAccount {
    /// Create or get associated token account instruction
    pub fn create(
        payer: &SolanaAddress,
        owner: &SolanaAddress,
        mint: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = SplToken::associated_token_program_id()?;
        let token_program_id = SplToken::program_id()?;
        let system_program = SolanaAddress::from_base58(super::SYSTEM_PROGRAM_ID)?;

        let associated_token = SplToken::get_associated_token_address(owner, mint)?;

        let data = vec![0]; // Create instruction

        let accounts = vec![
            AccountMeta::signer_writable(payer.clone()),
            AccountMeta::writable(associated_token),
            AccountMeta::readonly(owner.clone()),
            AccountMeta::readonly(mint.clone()),
            AccountMeta::readonly(system_program),
            AccountMeta::readonly(token_program_id),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Create associated token account idempotent (won't fail if exists)
    pub fn create_idempotent(
        payer: &SolanaAddress,
        owner: &SolanaAddress,
        mint: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = SplToken::associated_token_program_id()?;
        let token_program_id = SplToken::program_id()?;
        let system_program = SolanaAddress::from_base58(super::SYSTEM_PROGRAM_ID)?;

        let associated_token = SplToken::get_associated_token_address(owner, mint)?;

        let data = vec![1]; // CreateIdempotent instruction

        let accounts = vec![
            AccountMeta::signer_writable(payer.clone()),
            AccountMeta::writable(associated_token),
            AccountMeta::readonly(owner.clone()),
            AccountMeta::readonly(mint.clone()),
            AccountMeta::readonly(system_program),
            AccountMeta::readonly(token_program_id),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }

    /// Recover nested associated token account
    pub fn recover_nested(
        nested_owner: &SolanaAddress,
        nested_mint: &SolanaAddress,
        destination: &SolanaAddress,
        owner_mint: &SolanaAddress,
        wallet: &SolanaAddress,
    ) -> Result<Instruction> {
        let program_id = SplToken::associated_token_program_id()?;
        let token_program_id = SplToken::program_id()?;

        let nested_associated = SplToken::get_associated_token_address(nested_owner, nested_mint)?;
        let owner_associated = SplToken::get_associated_token_address(wallet, owner_mint)?;

        let data = vec![2]; // RecoverNested instruction

        let accounts = vec![
            AccountMeta::writable(nested_associated),
            AccountMeta::readonly(nested_mint.clone()),
            AccountMeta::writable(destination.clone()),
            AccountMeta::readonly(owner_associated),
            AccountMeta::readonly(owner_mint.clone()),
            AccountMeta::signer_readonly(wallet.clone()),
            AccountMeta::readonly(token_program_id),
        ];

        Ok(Instruction::new(program_id, accounts, data))
    }
}

/// Token authority types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AuthorityType {
    MintTokens = 0,
    FreezeAccount = 1,
    AccountOwner = 2,
    CloseAccount = 3,
}

/// Token account state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountState {
    Uninitialized,
    Initialized,
    Frozen,
}

/// Token account info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAccount {
    /// Token mint
    pub mint: SolanaAddress,
    /// Owner of the account
    pub owner: SolanaAddress,
    /// Amount of tokens
    pub amount: u64,
    /// Delegate (if any)
    pub delegate: Option<SolanaAddress>,
    /// Account state
    pub state: AccountState,
    /// Delegated amount
    pub delegated_amount: u64,
    /// Close authority (if any)
    pub close_authority: Option<SolanaAddress>,
}

impl TokenAccount {
    /// Token account size in bytes
    pub const LEN: usize = 165;

    /// Parse from account data
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < Self::LEN {
            return Err(Error::validation("Token account data too short"));
        }

        let mint = SolanaAddress::from_bytes(&data[0..32])?;
        let owner = SolanaAddress::from_bytes(&data[32..64])?;
        let amount = u64::from_le_bytes(data[64..72].try_into().unwrap());

        let delegate = if data[72] == 1 {
            Some(SolanaAddress::from_bytes(&data[76..108])?)
        } else {
            None
        };

        let state = match data[108] {
            0 => AccountState::Uninitialized,
            1 => AccountState::Initialized,
            2 => AccountState::Frozen,
            _ => return Err(Error::validation("Invalid account state")),
        };

        let delegated_amount = u64::from_le_bytes(data[121..129].try_into().unwrap());

        let close_authority = if data[129] == 1 {
            Some(SolanaAddress::from_bytes(&data[133..165])?)
        } else {
            None
        };

        Ok(Self {
            mint,
            owner,
            amount,
            delegate,
            state,
            delegated_amount,
            close_authority,
        })
    }
}

/// Mint info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintInfo {
    /// Mint authority (if any)
    pub mint_authority: Option<SolanaAddress>,
    /// Total supply
    pub supply: u64,
    /// Decimals
    pub decimals: u8,
    /// Is initialized
    pub is_initialized: bool,
    /// Freeze authority (if any)
    pub freeze_authority: Option<SolanaAddress>,
}

impl MintInfo {
    /// Mint account size in bytes
    pub const LEN: usize = 82;

    /// Parse from account data
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < Self::LEN {
            return Err(Error::validation("Mint data too short"));
        }

        let mint_authority = if data[0] == 1 {
            Some(SolanaAddress::from_bytes(&data[4..36])?)
        } else {
            None
        };

        let supply = u64::from_le_bytes(data[36..44].try_into().unwrap());
        let decimals = data[44];
        let is_initialized = data[45] == 1;

        let freeze_authority = if data[46] == 1 {
            Some(SolanaAddress::from_bytes(&data[50..82])?)
        } else {
            None
        };

        Ok(Self {
            mint_authority,
            supply,
            decimals,
            is_initialized,
            freeze_authority,
        })
    }
}

/// Token instruction types (for reference)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TokenInstruction {
    InitializeMint = 0,
    InitializeAccount = 1,
    InitializeMultisig = 2,
    Transfer = 3,
    Approve = 4,
    Revoke = 5,
    SetAuthority = 6,
    MintTo = 7,
    Burn = 8,
    CloseAccount = 9,
    FreezeAccount = 10,
    ThawAccount = 11,
    TransferChecked = 12,
    ApproveChecked = 13,
    MintToChecked = 14,
    BurnChecked = 15,
    InitializeAccount2 = 16,
    SyncNative = 17,
    InitializeAccount3 = 18,
    InitializeMultisig2 = 19,
    InitializeMint2 = 20,
    GetAccountDataSize = 21,
    InitializeImmutableOwner = 22,
    AmountToUiAmount = 23,
    UiAmountToAmount = 24,
}

/// Token transfer helper struct
#[derive(Debug, Clone)]
pub struct TokenTransfer {
    /// Source token account
    pub source: SolanaAddress,
    /// Destination token account  
    pub destination: SolanaAddress,
    /// Token mint
    pub mint: SolanaAddress,
    /// Transfer amount
    pub amount: u64,
    /// Decimals (for checked transfer)
    pub decimals: u8,
}

impl TokenTransfer {
    /// Create transfer instruction
    pub fn to_instruction(&self, authority: &SolanaAddress) -> Result<Instruction> {
        SplToken::transfer_checked(
            &self.source,
            &self.mint,
            &self.destination,
            authority,
            self.amount,
            self.decimals,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_associated_token_derivation() {
        let owner = SolanaAddress::from_bytes(&[1u8; 32]).unwrap();
        let mint = SolanaAddress::from_bytes(&[2u8; 32]).unwrap();

        let ata = SplToken::get_associated_token_address(&owner, &mint);
        assert!(ata.is_ok());
    }

    #[test]
    fn test_transfer_instruction() {
        let source = SolanaAddress::from_bytes(&[1u8; 32]).unwrap();
        let dest = SolanaAddress::from_bytes(&[2u8; 32]).unwrap();
        let authority = SolanaAddress::from_bytes(&[3u8; 32]).unwrap();

        let ix = SplToken::transfer(&source, &dest, &authority, 1000);
        assert!(ix.is_ok());

        let ix = ix.unwrap();
        assert_eq!(ix.data[0], 3); // Transfer instruction
        assert_eq!(ix.accounts.len(), 3);
    }
}
