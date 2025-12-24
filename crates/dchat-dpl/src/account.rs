//! Account types for DPL programs

use crate::error::DplError;
use core::marker::PhantomData;

/// 32-byte public key
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct Pubkey(pub [u8; 32]);

impl Pubkey {
    /// Create a new pubkey from bytes
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Create from slice
    pub fn from_slice(data: &[u8]) -> Option<Self> {
        if data.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(data);
        Some(Self(bytes))
    }
}

impl AsRef<[u8]> for Pubkey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Account info passed to programs
#[derive(Debug, Clone)]
pub struct AccountInfo<'a> {
    /// Account public key
    pub key: &'a Pubkey,
    /// Is this account a signer?
    pub is_signer: bool,
    /// Is this account writable?
    pub is_writable: bool,
    /// Account balance in motes
    pub motes: u64,
    /// Account data
    pub data: &'a [u8],
    /// Owner program
    pub owner: &'a Pubkey,
}

impl<'a> AccountInfo<'a> {
    /// Get the account key
    pub fn key(&self) -> &Pubkey {
        self.key
    }
}

/// A signer account - proves the holder signed the transaction
pub struct Signer<'info> {
    info: AccountInfo<'info>,
}

impl<'info> Signer<'info> {
    /// Get the signer's public key
    pub fn key(&self) -> &Pubkey {
        self.info.key
    }
    /// Whether this signer actually signed
    pub fn is_signer(&self) -> bool {
        self.info.is_signer
    }
}

/// System account - just a pubkey with no data
pub struct SystemAccount<'info> {
    info: AccountInfo<'info>,
}

impl<'info> SystemAccount<'info> {
    /// Get the account's public key
    pub fn key(&self) -> &Pubkey {
        self.info.key
    }
}

/// Typed account wrapper
pub struct Account<'info, T> {
    info: AccountInfo<'info>,
    _phantom: PhantomData<T>,
}

impl<'info, T> Account<'info, T> {
    /// Get the account key
    pub fn key(&self) -> &Pubkey {
        self.info.key
    }
    /// Whether this account is a signer
    pub fn is_signer(&self) -> bool {
        self.info.is_signer
    }
    /// Whether this account is writable
    pub fn is_writable(&self) -> bool {
        self.info.is_writable
    }
    /// Get the raw account data bytes
    pub fn data(&self) -> &'info [u8] {
        self.info.data
    }
}

impl<'info, T> core::ops::Deref for Account<'info, T>
where
    T: AccountDeserialize,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // In production, this would deserialize from info.data
        unimplemented!("Account deref requires runtime")
    }
}

impl<'info, T> core::ops::DerefMut for Account<'info, T>
where
    T: AccountDeserialize + AccountSerialize,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        // In production, this would deserialize from info.data
        unimplemented!("Account deref_mut requires runtime")
    }
}

/// Trait for deserializing account data (structs) from raw bytes
pub trait AccountDeserialize: Sized {
    /// Deserialize from raw account bytes
    fn try_deserialize(buf: &mut &[u8]) -> Result<Self, crate::error::DplError>;
}

/// Trait for serializing account data
pub trait AccountSerialize {
    /// Serialize to an io::Write
    fn try_serialize(&self, writer: &mut impl std::io::Write)
        -> Result<(), crate::error::DplError>;
}

/// Trait for creating account from AccountInfo
pub trait FromAccountInfo<'info>: Sized {
    /// Create from account info
    fn from_account_info(info: &AccountInfo<'info>) -> Result<Self, crate::error::DplError>;
}

/// Trait for parsing account constraints
pub trait AccountsParse<'info>: Sized {
    /// Parse and validate accounts from info slice
    fn try_accounts(
        infos: &[AccountInfo<'info>],
        ix_data: &[u8],
    ) -> Result<Self, crate::error::DplError>;
}

/// Trait for validating account constraints
pub trait AccountsValidate {
    /// Validate all account constraints
    fn validate(&self) -> Result<(), crate::error::DplError>;
}

/// System program type marker
pub struct System;

/// Program account wrapper - represents a program account
pub struct Program<'info, T> {
    info: AccountInfo<'info>,
    _phantom: PhantomData<T>,
}

impl<'info, T> Program<'info, T> {
    /// Get the program key
    pub fn key(&self) -> &Pubkey {
        self.info.key
    }
}

// Borsh support for Pubkey so it can be used in #[account] and #[event] structs
impl borsh::BorshSerialize for Pubkey {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.0)
    }
}

impl borsh::BorshDeserialize for Pubkey {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Ok(Pubkey(bytes))
    }
}

/// Trait for constructing wrapper types from ABI entries
pub trait FromAccountEntry<'info>: Sized {
    fn from_entry(entry: &mut crate::abi::AccountEntry<'info>) -> Result<Self, DplError>;
}

impl<'info, T> FromAccountEntry<'info> for Account<'info, T> {
    fn from_entry(entry: &mut crate::abi::AccountEntry<'info>) -> Result<Self, DplError> {
        // Store key and owner in leaked boxes to get 'info lifetime references
        // In a real runtime, these would be stored in a pre-allocated arena
        let key: &'info Pubkey = Box::leak(Box::new(entry.key));
        let owner: &'info Pubkey = Box::leak(Box::new(entry.owner));

        let info = AccountInfo {
            key,
            is_signer: entry.is_signer(),
            is_writable: entry.is_writable(),
            motes: entry.motes,
            data: entry.data,
            owner,
        };
        Ok(Account {
            info,
            _phantom: PhantomData,
        })
    }
}

impl<'info> FromAccountEntry<'info> for Signer<'info> {
    fn from_entry(entry: &mut crate::abi::AccountEntry<'info>) -> Result<Self, DplError> {
        // Validate that this account actually signed the transaction
        if !entry.is_signer() {
            return Err(DplError::MissingSigner);
        }

        // Store key and owner in leaked boxes to get 'info lifetime references
        let key: &'info Pubkey = Box::leak(Box::new(entry.key));
        let owner: &'info Pubkey = Box::leak(Box::new(entry.owner));

        let info = AccountInfo {
            key,
            is_signer: true,
            is_writable: entry.is_writable(),
            motes: entry.motes,
            data: entry.data,
            owner,
        };
        Ok(Signer { info })
    }
}

impl<'info, T> FromAccountEntry<'info> for Program<'info, T> {
    fn from_entry(entry: &mut crate::abi::AccountEntry<'info>) -> Result<Self, DplError> {
        // Store key and owner in leaked boxes to get 'info lifetime references
        let key: &'info Pubkey = Box::leak(Box::new(entry.key));
        let owner: &'info Pubkey = Box::leak(Box::new(entry.owner));

        let info = AccountInfo {
            key,
            is_signer: entry.is_signer(),
            is_writable: entry.is_writable(),
            motes: entry.motes,
            data: entry.data,
            owner,
        };
        Ok(Program {
            info,
            _phantom: PhantomData,
        })
    }
}
