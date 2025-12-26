//! dchat-programs: Production-grade Solana-accounts-style contract platform
//!
//! This crate implements a deterministic smart contract runtime for the dchat
//! currency chain, featuring:
//!
//! - **Account Model**: Solana-style Account/Instruction/AccountMeta with strict
//!   "touch only passed accounts" capability enforcement
//! - **Deterministic VM**: WebAssembly-based runtime with pinned ABI versions,
//!   forbidden non-deterministic operations (time/RNG/network/fs/threads/floats)
//! - **Metering**: Compute units, memory pricing, storage read/write costs,
//!   crypto syscall pricing with upfront fee reservation
//! - **Parallel Execution**: Read/write account locks with deterministic conflict resolution
//! - **Bytecode Validation**: Deploy-time format/import/opcode checks, size caps
//! - **Program Loader**: Deploy/upgrade/revocation with authority model and timelock
//! - **PDAs**: Program Derived Addresses for deterministic address derivation
//! - **CPI**: Cross-Program Invocation with shared CU budget and borrow rules
//! - **Capability Tokens**: Scoped permissions with expiry and transfer rules
//! - **Privacy Accounts**: Encrypted data with public commitments
//! - **System Programs**: Native programs for create/allocate/assign/transfer
//! - **Token Standard**: Mint/transfer/approve with associated token accounts
//!
//! # Security
//!
//! This crate is production-ready with the following guarantees:
//! - All execution is fully deterministic across nodes
//! - No placeholder or mock implementations in release builds
//! - Comprehensive metering prevents resource exhaustion
//! - Strict validation prevents malicious bytecode

#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(clippy::all)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

// ═══════════════════════════════════════════════════════════════════════════════
// DCHAT PROGRAM LANGUAGE V1 - CORE MODULES
// ═══════════════════════════════════════════════════════════════════════════════

/// Canonical binary ABI shared by host and guest (source of truth)
pub mod abi;
/// Guest-side SDK for contract authoring (wasm32-unknown-unknown)
pub mod guest;
/// Host-side copy-out commit with invariant enforcement
pub mod host_commit;

// ═══════════════════════════════════════════════════════════════════════════════
// EXISTING MODULES
// ═══════════════════════════════════════════════════════════════════════════════

pub mod account;
pub mod capability;
pub mod cpi;
pub mod error;
pub mod events;
pub mod instruction;
pub mod loader;
pub mod manifest;
pub mod metering;
pub mod pda;
pub mod privacy;
pub mod runtime;
pub mod scheduler;
pub mod syscalls;
pub mod system_program;
pub mod token;
pub mod validation;
pub mod vm;
pub mod wasi_shim;

// Re-export IDL from dchat-dpl for CLI and tooling access
pub use dchat_dpl::idl;

// Re-export borsh for IDL deserialization in CLI tools (via dchat-dpl)
pub use dchat_dpl::borsh;

// Re-export primary types for ergonomic API
// Note: Motes is the canonical currency unit (1 DCHAT = 100,000,000 motes)
#[allow(deprecated)]
pub use account::Lamports; // Deprecated alias for backward compatibility
pub use account::{
    Account, AccountData, AccountMeta, AccountState, Motes, Pubkey, RentEpoch, MOTES_PER_DCHAT,
};
pub use capability::{CapabilityId, CapabilityRegistry, CapabilityScope, CapabilityToken};
pub use cpi::{CpiContext, CpiExecutor, CpiGuard, CpiResult, CrossProgramInvocation};
pub use error::{ProgramError, ProgramResult};
pub use events::{EventFilter, ExecutionReceipt, ProgramEvent};
pub use instruction::{CompiledInstruction, Instruction, InstructionAccount, InstructionData};
pub use loader::LoaderInstruction;
pub use metering::{ComputeBudget, ComputeMeter, CryptoOpCosts, MemoryCosts, StorageCosts};
pub use pda::{PdaDerivation, ProgramDerivedAddress};
pub use privacy::{EncryptedBalance, PrivacyAccount, PrivacyProgram};
pub use runtime::{ExecutionContext, ProgramRuntime};
pub use scheduler::{AccountLock, ExecutionBatch, ParallelScheduler, SchedulerConfig};
pub use syscalls::{SyscallContext, SyscallHandler, SyscallRegistry};
pub use system_program::{SystemInstruction, SystemProgram};
pub use token::{AssociatedTokenAccount, Mint, TokenAccount, TokenInstruction, TokenProgram};
pub use validation::{BytecodeValidator, ValidationConfig, ValidationError, ValidationResult};
pub use vm::{DeterministicVm, VmConfig, VmInstance, VmMemory, VmState};

/// Protocol version for VM/ABI/syscalls compatibility
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum program size in bytes (10 MB)
pub const MAX_PROGRAM_SIZE: usize = 10 * 1024 * 1024;

/// Maximum instruction data size in bytes (10 KB)
pub const MAX_INSTRUCTION_DATA_SIZE: usize = 10 * 1024;

/// Maximum accounts per instruction
pub const MAX_ACCOUNTS_PER_INSTRUCTION: usize = 64;

/// Maximum CPI depth
pub const MAX_CPI_DEPTH: usize = 4;

/// Maximum compute units per transaction
pub const MAX_COMPUTE_UNITS: u64 = 1_400_000;

/// Default compute units per transaction
pub const DEFAULT_COMPUTE_UNITS: u64 = 200_000;

/// Maximum stack depth in VM
pub const MAX_STACK_DEPTH: usize = 64;

/// Maximum memory pages (64KB each)
pub const MAX_MEMORY_PAGES: u32 = 256; // 16 MB max

/// Maximum table elements
pub const MAX_TABLE_ELEMENTS: u32 = 10_000;

/// Maximum account data size in bytes (10 MB)
pub const MAX_ACCOUNT_SIZE: usize = 10 * 1024 * 1024;

/// Native program IDs
pub mod native_programs {
    use super::Pubkey;

    /// System program ID (all zeros except last byte = 1)
    pub const SYSTEM_PROGRAM_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 1,
    ]);

    /// Token program ID
    pub const TOKEN_PROGRAM_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 2,
    ]);

    /// Loader program ID
    pub const LOADER_PROGRAM_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 3,
    ]);

    /// Associated token account program ID
    pub const ATA_PROGRAM_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 4,
    ]);

    /// Capability program ID
    pub const CAPABILITY_PROGRAM_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 5,
    ]);

    /// Privacy program ID
    pub const PRIVACY_PROGRAM_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 6,
    ]);

    /// Rent sysvar ID
    pub const SYSVAR_RENT_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 0,
    ]);

    /// Clock sysvar ID
    pub const SYSVAR_CLOCK_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 1,
    ]);

    /// Recent blockhashes sysvar ID
    pub const SYSVAR_RECENT_BLOCKHASHES_ID: Pubkey = Pubkey([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 2,
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_constants() {
        assert!(MAX_PROGRAM_SIZE > 0);
        assert!(MAX_INSTRUCTION_DATA_SIZE > 0);
        assert!(MAX_ACCOUNTS_PER_INSTRUCTION > 0);
        assert!(MAX_CPI_DEPTH > 0);
        assert!(MAX_COMPUTE_UNITS > 0);
    }
}
