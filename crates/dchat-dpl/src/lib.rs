//! Dchat Program Language (DPL) SDK
//!
//! DPL is a Rust-based contract authoring framework that provides ergonomic
//! macros and utilities for writing dchat currency chain programs. It builds
//! on top of the dchat-programs ABI/VM without changing the on-chain execution
//! model.
//!
//! # Features
//!
//! - **Type-safe instruction routing** with stable tags via `#[derive(Instruction)]`
//! - **Account validation** with constraints via `#[derive(Accounts)]`
//! - **State management** with automatic serialization via `#[account]`
//! - **Event emission** with deterministic discriminators via `#[event]`
//! - **Error handling** with stable codes via `#[error_code]`
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use dchat_dpl::prelude::*;
//!
//! #[program]
//! pub mod my_program {
//!     use super::*;
//!
//!     pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
//!         // Your logic here
//!         Ok(())
//!     }
//! }
//!
//! #[derive(Accounts)]
//! pub struct Initialize<'info> {
//!     #[account(init, space = 8 + 32)]
//!     pub data: Account<'info, MyData>,
//!     #[account(signer)]
//!     pub authority: Signer<'info>,
//! }
//!
//! #[account]
//! pub struct MyData {
//!     pub value: u64,
//! }
//! ```
//!
//! # Compilation Target
//!
//! DPL programs compile to `wasm32-wasi` (not `wasm32-unknown-unknown`), which
//! allows use of Rust's standard library while the runtime provides a
//! deterministic WASI shim.
//!
//! # Edition
//!
//! DPL Edition 2025 - first production release.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

// Re-export borsh for use by generated code
pub use borsh;

// Re-export the proc-macro crate
pub use dchat_dpl_macros::*;

// ═══════════════════════════════════════════════════════════════════════════════
// MODULES
// ═══════════════════════════════════════════════════════════════════════════════

pub mod abi;
pub mod account;
pub mod context;
pub mod error;
pub mod event;
pub mod idl;
pub mod manifest;
pub mod pda;
pub mod serde;
pub mod syscall;

// Build-time helpers (only available with "build" feature and std)
#[cfg(all(feature = "build", feature = "std"))]
pub mod build;

// ═══════════════════════════════════════════════════════════════════════════════
// ROOT-LEVEL RE-EXPORTS (for macro-generated code)
// ═══════════════════════════════════════════════════════════════════════════════

// Types that macros reference as dchat_dpl::X
pub use account::{
    Account, AccountDeserialize, AccountInfo, AccountSerialize, FromAccountEntry, FromAccountInfo,
    Program, Pubkey, Signer, System, SystemAccount,
};
pub use context::{Accounts, Bumps, Context, ContextInfo};
pub use error::{DplError, DplResult};
pub use event::{emit_event, Event};
pub use pda::derive_pda;
pub use serde::{DplDeserialize, DplSerialize};

// IDL types for schema generation
pub use idl::{account_discriminator, event_discriminator, instruction_discriminator};
pub use idl::{Idl, IdlAccountDef, IdlAccountMeta, IdlEvent, IdlField, IdlInstruction, IdlType};

// Syscall types for program introspection
pub use syscall::{get_program_manifest, verify_program_schema, ProgramManifest};

/// Convenience Result type alias
pub type Result<T> = core::result::Result<T, DplError>;

// ═══════════════════════════════════════════════════════════════════════════════
// PRELUDE
// ═══════════════════════════════════════════════════════════════════════════════

/// Prelude module - import everything needed for DPL programs
pub mod prelude {
    // Re-export all macros
    pub use dchat_dpl_macros::{account, error_code, event, program, Accounts, Instruction};

    // Core types
    pub use crate::account::{
        Account, AccountDeserialize, AccountInfo, AccountSerialize, FromAccountInfo, Program,
        Pubkey, Signer, System, SystemAccount,
    };
    pub use crate::context::Context;
    pub use crate::error::{DplError, DplResult};
    pub use crate::event::emit_event;
    pub use crate::pda::derive_pda;
    pub use crate::serde::{DplDeserialize, DplSerialize};

    // Convenience type alias
    pub type Result<T> = core::result::Result<T, DplError>;

    // Token placeholder
    pub struct Token;

    // Clock placeholder
    pub struct Clock {
        pub slot: u64,
        pub epoch: u64,
        pub unix_timestamp: i64,
    }

    impl Clock {
        pub fn get() -> DplResult<Self> {
            // In production, this reads from a sysvar
            Ok(Clock {
                slot: 0,
                epoch: 0,
                unix_timestamp: 0,
            })
        }
    }

    // Require macro for constraint checking
    #[macro_export]
    macro_rules! require {
        ($cond:expr, $err:expr) => {
            if !$cond {
                return Err($err.into());
            }
        };
    }

    // Re-export emit macro
    pub use crate::emit;
}

// ═══════════════════════════════════════════════════════════════════════════════
// VERSION CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// DPL SDK version string
pub const DPL_VERSION: &str = "0.1.0";

/// DPL SDK major version
pub const DPL_VERSION_MAJOR: u16 = 0;

/// DPL SDK minor version
pub const DPL_VERSION_MINOR: u16 = 1;

/// DPL SDK patch version
pub const DPL_VERSION_PATCH: u16 = 0;

/// DPL edition year
pub const DPL_EDITION: u16 = 2025;

/// ABI version for runtime compatibility
pub const ABI_VERSION: u8 = 1;
