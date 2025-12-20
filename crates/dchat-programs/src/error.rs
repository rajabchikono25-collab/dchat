//! Error types for the program runtime

use std::fmt;
use thiserror::Error;

/// Result type for program operations
pub type ProgramResult<T> = Result<T, ProgramError>;

/// Program execution error
#[derive(Debug, Clone, Error)]
pub enum ProgramError {
    /// Custom program error with code
    #[error("Custom error: {0}")]
    Custom(u32),

    /// Invalid instruction data
    #[error("Invalid instruction data")]
    InvalidInstructionData,

    /// Invalid account data
    #[error("Invalid account data")]
    InvalidAccountData,

    /// Account not rent exempt
    #[error("Account not rent exempt")]
    AccountNotRentExempt,

    /// Insufficient funds for operation
    #[error("Insufficient funds")]
    InsufficientFunds,

    /// Account already initialized
    #[error("Account already initialized")]
    AccountAlreadyInitialized,

    /// Account not initialized
    #[error("Uninitialized account")]
    UninitializedAccount,

    /// Account owner mismatch
    #[error("Incorrect program id")]
    IncorrectProgramId,

    /// Missing required signature
    #[error("Missing required signature")]
    MissingRequiredSignature,

    /// Account already in use
    #[error("Account already in use")]
    AccountAlreadyInUse,

    /// Account not writable
    #[error("Account is not writable")]
    AccountNotWritable,

    /// Account not signer
    #[error("Account is not a signer")]
    AccountNotSigner,

    /// Invalid seeds for PDA
    #[error("Invalid seeds for PDA")]
    InvalidSeeds,

    /// Invalid account owner
    #[error("Invalid account owner")]
    InvalidAccountOwner,

    /// Arithmetic overflow
    #[error("Arithmetic overflow")]
    ArithmeticOverflow,

    /// Immutable account modified
    #[error("Cannot modify immutable account")]
    ImmutableModification,

    /// Executable account modified
    #[error("Cannot modify executable account")]
    ExecutableModification,

    /// Account data size changed
    #[error("Account data size changed")]
    AccountDataSizeChanged,

    /// Account data too large
    #[error("Account data too large")]
    AccountDataTooLarge,

    /// Insufficient compute units
    #[error("Compute budget exceeded")]
    ComputeBudgetExceeded,

    /// Memory limit exceeded
    #[error("Memory limit exceeded")]
    MemoryLimitExceeded,

    /// Stack overflow
    #[error("Stack overflow")]
    StackOverflow,

    /// Call depth exceeded
    #[error("Call depth exceeded")]
    CallDepthExceeded,

    /// Invalid program
    #[error("Invalid program")]
    InvalidProgram,

    /// Program not executable
    #[error("Program not executable")]
    ProgramNotExecutable,

    /// Borrows overlap (CPI safety)
    #[error("Account borrows overlap")]
    BorrowsOverlap,

    /// Maximum accounts exceeded
    #[error("Too many accounts")]
    MaxAccountsExceeded,

    /// Maximum instruction data exceeded
    #[error("Instruction data too large")]
    MaxInstructionDataExceeded,

    /// Account not found
    #[error("Account not found")]
    AccountNotFound,

    /// Program account mismatch
    #[error("Program account mismatch")]
    ProgramAccountMismatch,

    /// Invalid authority
    #[error("Invalid authority")]
    InvalidAuthority,

    /// Capability expired
    #[error("Capability expired")]
    CapabilityExpired,

    /// Capability not found
    #[error("Capability not found")]
    CapabilityNotFound,

    /// Capability scope mismatch
    #[error("Capability scope mismatch")]
    CapabilityScopeMismatch,

    /// Invalid capability transfer
    #[error("Invalid capability transfer")]
    InvalidCapabilityTransfer,

    /// Privacy commitment mismatch
    #[error("Privacy commitment mismatch")]
    PrivacyCommitmentMismatch,

    /// Decryption failed
    #[error("Decryption failed")]
    DecryptionFailed,

    /// Selective disclosure failed
    #[error("Selective disclosure verification failed")]
    SelectiveDisclosureFailed,

    /// Token mint authority mismatch
    #[error("Token mint authority mismatch")]
    MintAuthorityMismatch,

    /// Token freeze authority mismatch
    #[error("Token freeze authority mismatch")]
    FreezeAuthorityMismatch,

    /// Token account frozen
    #[error("Token account is frozen")]
    TokenAccountFrozen,

    /// Invalid mint
    #[error("Invalid mint")]
    InvalidMint,

    /// Invalid token account
    #[error("Invalid token account")]
    InvalidTokenAccount,

    /// Mint decimals mismatch
    #[error("Mint decimals mismatch")]
    MintDecimalsMismatch,

    /// Max supply exceeded
    #[error("Max supply exceeded")]
    MaxSupplyExceeded,

    /// Timelock not expired
    #[error("Timelock not expired")]
    TimelockNotExpired,

    /// Program frozen
    #[error("Program is frozen")]
    ProgramFrozen,

    /// Upgrade authority mismatch
    #[error("Upgrade authority mismatch")]
    UpgradeAuthorityMismatch,

    /// Invalid bytecode
    #[error("Invalid bytecode: {0}")]
    InvalidBytecode(String),

    /// Syscall error
    #[error("Syscall error: {0}")]
    SyscallError(String),

    /// VM execution error
    #[error("VM error: {0}")]
    VmError(String),

    /// Reentrancy detected
    #[error("Reentrancy detected")]
    ReentrancyDetected,

    /// Account locked
    #[error("Account is locked by another transaction")]
    AccountLocked,

    /// Scheduler conflict
    #[error("Scheduler conflict detected")]
    SchedulerConflict,

    /// Rate limited
    #[error("Rate limited: {0}")]
    RateLimited(String),

    /// Invalid fee
    #[error("Invalid fee")]
    InvalidFee,

    /// Fee not reserved
    #[error("Fee not reserved")]
    FeeNotReserved,

    /// Internal error (should not happen)
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl ProgramError {
    /// Convert to error code for on-chain representation
    pub fn to_code(&self) -> u32 {
        match self {
            ProgramError::Custom(code) => *code,
            ProgramError::InvalidInstructionData => 1,
            ProgramError::InvalidAccountData => 2,
            ProgramError::AccountNotRentExempt => 3,
            ProgramError::InsufficientFunds => 4,
            ProgramError::AccountAlreadyInitialized => 5,
            ProgramError::UninitializedAccount => 6,
            ProgramError::IncorrectProgramId => 7,
            ProgramError::MissingRequiredSignature => 8,
            ProgramError::AccountAlreadyInUse => 9,
            ProgramError::AccountNotWritable => 10,
            ProgramError::AccountNotSigner => 11,
            ProgramError::InvalidSeeds => 12,
            ProgramError::InvalidAccountOwner => 13,
            ProgramError::ArithmeticOverflow => 14,
            ProgramError::ImmutableModification => 15,
            ProgramError::ExecutableModification => 16,
            ProgramError::AccountDataSizeChanged => 17,
            ProgramError::AccountDataTooLarge => 18,
            ProgramError::ComputeBudgetExceeded => 19,
            ProgramError::MemoryLimitExceeded => 20,
            ProgramError::StackOverflow => 21,
            ProgramError::CallDepthExceeded => 22,
            ProgramError::InvalidProgram => 23,
            ProgramError::ProgramNotExecutable => 24,
            ProgramError::BorrowsOverlap => 25,
            ProgramError::MaxAccountsExceeded => 26,
            ProgramError::MaxInstructionDataExceeded => 27,
            ProgramError::AccountNotFound => 28,
            ProgramError::ProgramAccountMismatch => 29,
            ProgramError::InvalidAuthority => 30,
            ProgramError::CapabilityExpired => 31,
            ProgramError::CapabilityNotFound => 32,
            ProgramError::CapabilityScopeMismatch => 33,
            ProgramError::InvalidCapabilityTransfer => 34,
            ProgramError::PrivacyCommitmentMismatch => 35,
            ProgramError::DecryptionFailed => 36,
            ProgramError::SelectiveDisclosureFailed => 37,
            ProgramError::MintAuthorityMismatch => 38,
            ProgramError::FreezeAuthorityMismatch => 39,
            ProgramError::TokenAccountFrozen => 40,
            ProgramError::InvalidMint => 41,
            ProgramError::InvalidTokenAccount => 42,
            ProgramError::MintDecimalsMismatch => 43,
            ProgramError::MaxSupplyExceeded => 44,
            ProgramError::TimelockNotExpired => 45,
            ProgramError::ProgramFrozen => 46,
            ProgramError::UpgradeAuthorityMismatch => 47,
            ProgramError::InvalidBytecode(_) => 48,
            ProgramError::SyscallError(_) => 49,
            ProgramError::VmError(_) => 50,
            ProgramError::ReentrancyDetected => 51,
            ProgramError::AccountLocked => 52,
            ProgramError::SchedulerConflict => 53,
            ProgramError::RateLimited(_) => 54,
            ProgramError::InvalidFee => 55,
            ProgramError::FeeNotReserved => 56,
            ProgramError::InternalError(_) => u32::MAX,
        }
    }

    /// Create from error code
    pub fn from_code(code: u32) -> Self {
        match code {
            1 => ProgramError::InvalidInstructionData,
            2 => ProgramError::InvalidAccountData,
            3 => ProgramError::AccountNotRentExempt,
            4 => ProgramError::InsufficientFunds,
            5 => ProgramError::AccountAlreadyInitialized,
            6 => ProgramError::UninitializedAccount,
            7 => ProgramError::IncorrectProgramId,
            8 => ProgramError::MissingRequiredSignature,
            9 => ProgramError::AccountAlreadyInUse,
            10 => ProgramError::AccountNotWritable,
            11 => ProgramError::AccountNotSigner,
            12 => ProgramError::InvalidSeeds,
            13 => ProgramError::InvalidAccountOwner,
            14 => ProgramError::ArithmeticOverflow,
            15 => ProgramError::ImmutableModification,
            16 => ProgramError::ExecutableModification,
            17 => ProgramError::AccountDataSizeChanged,
            18 => ProgramError::AccountDataTooLarge,
            19 => ProgramError::ComputeBudgetExceeded,
            20 => ProgramError::MemoryLimitExceeded,
            21 => ProgramError::StackOverflow,
            22 => ProgramError::CallDepthExceeded,
            23 => ProgramError::InvalidProgram,
            24 => ProgramError::ProgramNotExecutable,
            25 => ProgramError::BorrowsOverlap,
            26 => ProgramError::MaxAccountsExceeded,
            27 => ProgramError::MaxInstructionDataExceeded,
            28 => ProgramError::AccountNotFound,
            29 => ProgramError::ProgramAccountMismatch,
            30 => ProgramError::InvalidAuthority,
            31 => ProgramError::CapabilityExpired,
            32 => ProgramError::CapabilityNotFound,
            33 => ProgramError::CapabilityScopeMismatch,
            34 => ProgramError::InvalidCapabilityTransfer,
            35 => ProgramError::PrivacyCommitmentMismatch,
            36 => ProgramError::DecryptionFailed,
            37 => ProgramError::SelectiveDisclosureFailed,
            38 => ProgramError::MintAuthorityMismatch,
            39 => ProgramError::FreezeAuthorityMismatch,
            40 => ProgramError::TokenAccountFrozen,
            41 => ProgramError::InvalidMint,
            42 => ProgramError::InvalidTokenAccount,
            43 => ProgramError::MintDecimalsMismatch,
            44 => ProgramError::MaxSupplyExceeded,
            45 => ProgramError::TimelockNotExpired,
            46 => ProgramError::ProgramFrozen,
            47 => ProgramError::UpgradeAuthorityMismatch,
            48 => ProgramError::InvalidBytecode("unknown".to_string()),
            49 => ProgramError::SyscallError("unknown".to_string()),
            50 => ProgramError::VmError("unknown".to_string()),
            51 => ProgramError::ReentrancyDetected,
            52 => ProgramError::AccountLocked,
            53 => ProgramError::SchedulerConflict,
            54 => ProgramError::RateLimited("unknown".to_string()),
            55 => ProgramError::InvalidFee,
            56 => ProgramError::FeeNotReserved,
            _ => ProgramError::Custom(code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_roundtrip() {
        let errors = vec![
            ProgramError::InvalidInstructionData,
            ProgramError::InsufficientFunds,
            ProgramError::ComputeBudgetExceeded,
            ProgramError::ReentrancyDetected,
        ];

        for err in errors {
            let code = err.to_code();
            let recovered = ProgramError::from_code(code);
            assert_eq!(code, recovered.to_code());
        }
    }

    #[test]
    fn test_custom_error() {
        let custom = ProgramError::Custom(12345);
        assert_eq!(custom.to_code(), 12345);
    }
}
