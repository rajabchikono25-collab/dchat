//! Error types for DPL programs

use thiserror::Error;

/// DPL error type
#[derive(Debug, Error)]
pub enum DplError {
    /// Account not found
    #[error("Account not found: {0}")]
    AccountNotFound(u8),

    /// Account constraint violation
    #[error("Constraint violation: {0}")]
    ConstraintViolation(&'static str),

    /// Invalid account owner
    #[error("Invalid account owner")]
    InvalidOwner,

    /// Account not signer
    #[error("Account must be signer")]
    MissingSigner,

    /// Account not writable
    #[error("Account must be writable")]
    NotWritable,

    /// Account already initialized
    #[error("Account already initialized")]
    AlreadyInitialized,

    /// Account not initialized
    #[error("Account not initialized")]
    NotInitialized,

    /// Invalid instruction data
    #[error("Invalid instruction data")]
    InvalidInstructionData,

    /// Invalid account data
    #[error("Invalid account data")]
    InvalidAccountData,

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
    #[error("Custom error {code}: {message}")]
    Custom {
        /// Error code
        code: u32,
        /// Error message
        message: &'static str,
    },
}

impl DplError {
    /// Get the error code
    pub fn code(&self) -> u32 {
        match self {
            Self::AccountNotFound(_) => 1,
            Self::ConstraintViolation(_) => 2,
            Self::InvalidOwner => 3,
            Self::MissingSigner => 4,
            Self::NotWritable => 5,
            Self::AlreadyInitialized => 6,
            Self::NotInitialized => 7,
            Self::InvalidInstructionData => 8,
            Self::InvalidAccountData => 9,
            Self::SerializationError(_) => 10,
            Self::DeserializationError(_) => 11,
            Self::ProgramError(code) => *code,
            Self::InsufficientFunds => 12,
            Self::Overflow => 13,
            Self::Underflow => 14,
            Self::InvalidPda => 15,
            Self::Custom { code, .. } => *code,
        }
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
            message: "Unknown error",
        }
    }
}
