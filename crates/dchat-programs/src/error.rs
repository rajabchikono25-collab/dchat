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

    /// Account frozen (generic)
    #[error("Account is frozen")]
    AccountFrozen,

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

    /// Seed too long
    #[error("Seed too long")]
    SeedTooLong,

    /// Not enough account keys
    #[error("Not enough account keys")]
    NotEnoughAccountKeys,

    /// Invalid decimals
    #[error("Invalid decimals")]
    InvalidDecimals,

    /// Uninitialized mint
    #[error("Uninitialized mint")]
    UninitializedMint,

    /// Insufficient delegated funds
    #[error("Insufficient delegated funds")]
    InsufficientDelegatedFunds,

    /// Mint mismatch
    #[error("Mint mismatch")]
    MintMismatch,

    /// Non-zero balance
    #[error("Non-zero balance")]
    NonZeroBalance,

    /// Invalid mint authority
    #[error("Invalid mint authority")]
    InvalidMintAuthority,

    /// Invalid freeze authority
    #[error("Invalid freeze authority")]
    InvalidFreezeAuthority,

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

    /// Memory access violation
    #[error("Memory access violation")]
    MemoryAccessViolation,

    /// Invalid argument
    #[error("Invalid argument")]
    InvalidArgument,

    /// Return data too large
    #[error("Return data too large")]
    ReturnDataTooLarge,

    /// Computational budget exceeded (alias)
    #[error("Computational budget exceeded")]
    ComputationalBudgetExceeded,

    /// Heap exhausted
    #[error("Heap exhausted")]
    HeapExhausted,

    /// Syscall not found
    #[error("Syscall not found")]
    SyscallNotFound,

    /// Invalid account data size
    #[error("Invalid account data size")]
    InvalidAccountDataSize,

    /// Account data not empty
    #[error("Account data not empty")]
    AccountDataNotEmpty,

    /// Account not initialized
    #[error("Account not initialized")]
    AccountNotInitialized,

    /// Account not executable
    #[error("Account is not executable")]
    AccountNotExecutable,

    /// Unsupported program
    #[error("Unsupported program")]
    UnsupportedProgram,

    /// Log buffer full
    #[error("Log buffer full")]
    LogBufferFull,

    /// Memory overlap
    #[error("Memory overlap")]
    MemoryOverlap,

    /// Capability revoked
    #[error("Capability has been revoked")]
    CapabilityRevoked,

    /// Capability exhausted
    #[error("Capability uses exhausted")]
    CapabilityExhausted,

    /// Capability out of scope
    #[error("Capability out of scope")]
    CapabilityOutOfScope,

    /// Capability transfer limit exceeded
    #[error("Capability transfer limit exceeded")]
    CapabilityTransferLimitExceeded,

    /// Duplicate capability
    #[error("Duplicate capability")]
    DuplicateCapability,

    /// Capability unauthorized
    #[error("Capability unauthorized")]
    CapabilityUnauthorized,

    /// Invalid upgrade authority
    #[error("Invalid upgrade authority")]
    InvalidUpgradeAuthority,

    /// Program not upgradeable
    #[error("Program is not upgradeable")]
    ProgramNotUpgradeable,

    /// Upgrade already pending
    #[error("Upgrade already pending")]
    UpgradeAlreadyPending,

    /// No upgrade pending
    #[error("No upgrade pending")]
    NoUpgradePending,

    /// Upgrade timelock active
    #[error("Upgrade timelock is still active")]
    UpgradeTimelockActive,

    /// Buffer mismatch
    #[error("Buffer mismatch")]
    BufferMismatch,

    /// Program hash mismatch
    #[error("Program hash mismatch")]
    ProgramHashMismatch,

    /// Range proof invalid
    #[error("Range proof invalid")]
    RangeProofInvalid,

    /// Invalid balance proof
    #[error("Invalid balance proof")]
    InvalidBalanceProof,
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
            ProgramError::AccountFrozen => 57,
            ProgramError::InvalidMint => 41,
            ProgramError::InvalidTokenAccount => 42,
            ProgramError::MintDecimalsMismatch => 43,
            ProgramError::MaxSupplyExceeded => 44,
            ProgramError::SeedTooLong => 58,
            ProgramError::NotEnoughAccountKeys => 59,
            ProgramError::InvalidDecimals => 60,
            ProgramError::UninitializedMint => 61,
            ProgramError::InsufficientDelegatedFunds => 62,
            ProgramError::MintMismatch => 63,
            ProgramError::NonZeroBalance => 64,
            ProgramError::InvalidMintAuthority => 65,
            ProgramError::InvalidFreezeAuthority => 66,
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
            ProgramError::MemoryAccessViolation => 67,
            ProgramError::InvalidArgument => 68,
            ProgramError::ReturnDataTooLarge => 69,
            ProgramError::ComputationalBudgetExceeded => 70,
            ProgramError::HeapExhausted => 71,
            ProgramError::SyscallNotFound => 72,
            ProgramError::InvalidAccountDataSize => 73,
            ProgramError::AccountDataNotEmpty => 74,
            ProgramError::AccountNotInitialized => 75,
            ProgramError::AccountNotExecutable => 76,
            ProgramError::UnsupportedProgram => 77,
            ProgramError::LogBufferFull => 78,
            ProgramError::MemoryOverlap => 79,
            ProgramError::CapabilityRevoked => 80,
            ProgramError::CapabilityExhausted => 81,
            ProgramError::CapabilityOutOfScope => 82,
            ProgramError::CapabilityTransferLimitExceeded => 83,
            ProgramError::DuplicateCapability => 84,
            ProgramError::CapabilityUnauthorized => 85,
            ProgramError::InvalidUpgradeAuthority => 86,
            ProgramError::ProgramNotUpgradeable => 87,
            ProgramError::UpgradeAlreadyPending => 88,
            ProgramError::NoUpgradePending => 89,
            ProgramError::UpgradeTimelockActive => 90,
            ProgramError::BufferMismatch => 91,
            ProgramError::ProgramHashMismatch => 92,
            ProgramError::RangeProofInvalid => 93,
            ProgramError::InvalidBalanceProof => 94,
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
            57 => ProgramError::AccountFrozen,
            58 => ProgramError::SeedTooLong,
            59 => ProgramError::NotEnoughAccountKeys,
            60 => ProgramError::InvalidDecimals,
            61 => ProgramError::UninitializedMint,
            62 => ProgramError::InsufficientDelegatedFunds,
            63 => ProgramError::MintMismatch,
            64 => ProgramError::NonZeroBalance,
            65 => ProgramError::InvalidMintAuthority,
            66 => ProgramError::InvalidFreezeAuthority,
            67 => ProgramError::MemoryAccessViolation,
            68 => ProgramError::InvalidArgument,
            69 => ProgramError::ReturnDataTooLarge,
            70 => ProgramError::ComputationalBudgetExceeded,
            71 => ProgramError::HeapExhausted,
            72 => ProgramError::SyscallNotFound,
            73 => ProgramError::InvalidAccountDataSize,
            74 => ProgramError::AccountDataNotEmpty,
            75 => ProgramError::AccountNotInitialized,
            76 => ProgramError::AccountNotExecutable,
            77 => ProgramError::UnsupportedProgram,
            78 => ProgramError::LogBufferFull,
            79 => ProgramError::MemoryOverlap,
            80 => ProgramError::CapabilityRevoked,
            81 => ProgramError::CapabilityExhausted,
            82 => ProgramError::CapabilityOutOfScope,
            83 => ProgramError::CapabilityTransferLimitExceeded,
            84 => ProgramError::DuplicateCapability,
            85 => ProgramError::CapabilityUnauthorized,
            86 => ProgramError::InvalidUpgradeAuthority,
            87 => ProgramError::ProgramNotUpgradeable,
            88 => ProgramError::UpgradeAlreadyPending,
            89 => ProgramError::NoUpgradePending,
            90 => ProgramError::UpgradeTimelockActive,
            91 => ProgramError::BufferMismatch,
            92 => ProgramError::ProgramHashMismatch,
            93 => ProgramError::RangeProofInvalid,
            94 => ProgramError::InvalidBalanceProof,
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
