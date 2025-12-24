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
pub mod manifest;
pub mod pda;
pub mod serde;
pub mod syscall;

// ═══════════════════════════════════════════════════════════════════════════════
// PRELUDE
// ═══════════════════════════════════════════════════════════════════════════════

/// Prelude module - import everything needed for DPL programs
pub mod prelude {
    // Re-export all macros
    pub use dchat_dpl_macros::{account, error_code, event, program, Accounts, Instruction};

    // Core types
    pub use crate::account::{
        Account, AccountDeserialize, AccountInfo, AccountSerialize, FromAccountInfo, Pubkey,
        Signer, SystemAccount,
    };
    pub use crate::context::Context;
    pub use crate::error::{DplError, DplResult};
    pub use crate::event::emit_event;
    pub use crate::pda::derive_pda;
    pub use crate::serde::{DplDeserialize, DplSerialize};

    // Convenience type alias
    pub type Result<T> = core::result::Result<T, DplError>;

    // Placeholder types that will be provided by generated code
    pub struct Program<'info, T> {
        _phantom: core::marker::PhantomData<&'info T>,
    }

    pub struct System;
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

    // Emit macro for events
    #[macro_export]
    macro_rules! emit {
        ($event:expr) => {
            $crate::event::emit_event(&$event)
        };
    }
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
