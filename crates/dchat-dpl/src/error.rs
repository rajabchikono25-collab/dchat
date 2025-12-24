//! Error types for DPL programs

use thiserror::Error;

/// DPL error type
#[derive(Debug, Error)]
pub enum DplError {
    /// Account not found
    #[error("Account not found: {0}")]
    AccountNotFound(u8),

    /// Account constraint violation
    #[error("Constraint violation")]
    ConstraintViolation,

    /// Has one constraint failed
    #[error("Has one constraint failed")]
    ConstraintHasOne,

    /// Invalid account owner
    #[error("Invalid account owner")]
    InvalidOwner,

    /// Account not signer
    #[error("Account must be signer")]
    MissingSigner,

    /// Account not writable
    #[error("Account must be writable")]
    AccountNotMutable,

    /// Account already initialized
    #[error("Account already initialized")]
    AlreadyInitialized,

    /// Account not initialized
    #[error("Account not initialized")]
    NotInitialized,

    /// Invalid instruction data
    #[error("Invalid instruction data")]
    InvalidInstructionData,

    /// Invalid instruction discriminator
    #[error("Invalid instruction discriminator")]
    InvalidInstructionDiscriminator,

    /// Invalid account data
    #[error("Invalid account data")]
    InvalidAccountData,

    /// Account discriminator mismatch
    #[error("Account discriminator mismatch")]
    AccountDiscriminatorMismatch,

    /// Account data too small
    #[error("Account data too small")]
    AccountDataTooSmall,

    /// Invalid magic bytes
    #[error("Invalid magic bytes")]
    InvalidMagic,

    /// Not enough accounts provided
    #[error("Not enough accounts provided")]
    NotEnoughAccounts,

    /// Invalid program ID
    #[error("Invalid program ID")]
    InvalidProgramId,

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// Program error with code
    #[error("Program error: {0}")]
    ProgramError(u32),

    /// Insufficient funds
    #[error("Insufficient funds")]
    InsufficientFunds,

    /// Arithmetic overflow
    #[error("Arithmetic overflow")]
    Overflow,

    /// Arithmetic underflow
    #[error("Arithmetic underflow")]
    Underflow,

    /// Invalid PDA
    #[error("Invalid PDA")]
    InvalidPda,

    /// Custom error with code and message
    #[error("Custom error {code}: {msg}")]
    Custom {
        /// Error code
        code: u32,
        /// Error message
        msg: String,
    },
}

impl DplError {
    /// Get the error code
    pub fn code(&self) -> u32 {
        match self {
            Self::AccountNotFound(_) => 1,
            Self::ConstraintViolation => 2,
            Self::ConstraintHasOne => 3,
            Self::InvalidOwner => 4,
            Self::MissingSigner => 5,
            Self::AccountNotMutable => 6,
            Self::AlreadyInitialized => 7,
            Self::NotInitialized => 8,
            Self::InvalidInstructionData => 9,
            Self::InvalidInstructionDiscriminator => 10,
            Self::InvalidAccountData => 11,
            Self::AccountDataTooSmall => 12,
            Self::InvalidMagic => 13,
            Self::AccountDiscriminatorMismatch => 22,
            Self::NotEnoughAccounts => 14,
            Self::InvalidProgramId => 15,
            Self::SerializationError(_) => 16,
            Self::DeserializationError(_) => 17,
            Self::ProgramError(code) => *code,
            Self::InsufficientFunds => 18,
            Self::Overflow => 19,
            Self::Underflow => 20,
            Self::InvalidPda => 21,
            Self::Custom { code, .. } => *code,
        }
    }
}

impl From<DplError> for u64 {
    fn from(e: DplError) -> Self {
        e.code() as u64
    }
}

impl From<std::io::Error> for DplError {
    fn from(e: std::io::Error) -> Self {
        DplError::SerializationError(e.to_string())
    }
}

/// Result type for DPL operations
pub type DplResult<T> = Result<T, DplError>;

/// Trait for types that can be converted to DplError
pub trait IntoDplError {
    /// Convert to DplError
    fn into_dpl_error(self) -> DplError;
}

impl<E: core::fmt::Display> IntoDplError for E {
    fn into_dpl_error(self) -> DplError {
        DplError::Custom {
            code: 0,
            msg: "Unknown error".to_string(),
        }
    }
}
