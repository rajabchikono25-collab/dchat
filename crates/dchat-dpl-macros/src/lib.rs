//! DPL Procedural Macros
//!
//! This crate provides procedural macros for authoring DPL (Dchat Program Language) contracts.
//!
//! # Macros
//!
//! - `#[program]` - Defines the program module with instruction handlers
//! - `#[derive(Instruction)]` - Generates instruction serialization/deserialization
//! - `#[derive(Accounts)]` - Validates and deserializes account inputs
//! - `#[account]` - Marks a struct as a program account with discriminator
//! - `#[event]` - Marks a struct as an emittable event
//! - `#[error_code]` - Defines program error codes
//!
//! # Example
//!
//! ```rust,ignore
//! use dchat_dpl::prelude::*;
//!
//! #[program]
//! pub mod my_program {
//!     pub fn initialize(ctx: Context<Initialize>, value: u64) -> Result<()> {
//!         ctx.accounts.data.value = value;
//!         Ok(())
//!     }
//! }
//!
//! #[derive(Accounts)]
//! pub struct Initialize<'info> {
//!     #[account(init, payer = authority, space = 8 + 8)]
//!     pub data: Account<'info, MyData>,
//!     #[account(mut)]
//!     pub authority: Signer<'info>,
//! }
//!
//! #[account]
//! pub struct MyData {
//!     pub value: u64,
//! }
//! ```

mod account;
mod accounts;
mod error_code;
mod event;
mod instruction;
mod program;

use proc_macro::TokenStream;

/// Marks a module as a DPL program.
///
/// This macro generates the program entrypoint, instruction dispatcher,
/// and IDL metadata.
///
/// # Example
///
/// ```rust,ignore
/// #[program]
/// pub mod counter {
///     use super::*;
///
///     pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
///         ctx.accounts.counter.count = 0;
///         Ok(())
///     }
///
///     pub fn increment(ctx: Context<Increment>) -> Result<()> {
///         ctx.accounts.counter.count += 1;
///         Ok(())
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn program(attr: TokenStream, item: TokenStream) -> TokenStream {
    program::program_impl(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Derives the `Instruction` trait for an enum.
///
/// Each variant represents a program instruction with its arguments.
/// The `tag` attribute specifies the 1-byte discriminator.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Instruction)]
/// pub enum MyInstruction {
///     #[tag(0)]
///     Initialize { value: u64 },
///     #[tag(1)]
///     Update { new_value: u64 },
/// }
/// ```
#[proc_macro_derive(Instruction, attributes(tag))]
pub fn derive_instruction(item: TokenStream) -> TokenStream {
    instruction::derive_instruction_impl(item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Derives the `Accounts` trait for account validation structs.
///
/// Supports the following constraint attributes:
/// - `#[account(mut)]` - Account must be mutable
/// - `#[account(signer)]` - Account must be a signer
/// - `#[account(init, payer = <acc>, space = <size>)]` - Initialize new account
/// - `#[account(seeds = [...], bump)]` - PDA derivation
/// - `#[account(has_one = <field>)]` - Field equality constraint
/// - `#[account(constraint = <expr>)]` - Custom constraint expression
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Accounts)]
/// pub struct Transfer<'info> {
///     #[account(mut)]
///     pub from: Account<'info, TokenAccount>,
///     #[account(mut)]
///     pub to: Account<'info, TokenAccount>,
///     pub authority: Signer<'info>,
/// }
/// ```
#[proc_macro_derive(Accounts, attributes(account, instruction))]
pub fn derive_accounts(item: TokenStream) -> TokenStream {
    accounts::derive_accounts_impl(item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Marks a struct as a program account.
///
/// This generates:
/// - An 8-byte discriminator based on the account name
/// - Borsh serialization/deserialization
/// - Space calculation helpers
///
/// # Example
///
/// ```rust,ignore
/// #[account]
/// pub struct Counter {
///     pub authority: Pubkey,
///     pub count: u64,
/// }
/// ```
#[proc_macro_attribute]
pub fn account(attr: TokenStream, item: TokenStream) -> TokenStream {
    account::account_impl(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Marks a struct as an emittable event.
///
/// Events are logged to the transaction log and can be parsed by clients.
///
/// # Example
///
/// ```rust,ignore
/// #[event]
/// pub struct TransferEvent {
///     pub from: Pubkey,
///     pub to: Pubkey,
///     pub amount: u64,
/// }
/// ```
#[proc_macro_attribute]
pub fn event(attr: TokenStream, item: TokenStream) -> TokenStream {
    event::event_impl(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}

/// Defines program error codes.
///
/// Each variant gets a unique error code starting from 6000.
/// The `msg` attribute provides a human-readable error message.
///
/// # Example
///
/// ```rust,ignore
/// #[error_code]
/// pub enum MyError {
///     #[msg("Insufficient funds for transfer")]
///     InsufficientFunds,
///     #[msg("Invalid authority")]
///     InvalidAuthority,
/// }
/// ```
#[proc_macro_attribute]
pub fn error_code(attr: TokenStream, item: TokenStream) -> TokenStream {
    error_code::error_code_impl(attr.into(), item.into())
        .unwrap_or_else(|e| e.into_compile_error())
        .into()
}
