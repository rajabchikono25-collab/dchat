//! dchat-core: Core types and utilities for the dchat system
//!
//! This crate provides fundamental types, traits, and utilities used across
//! the dchat decentralized messaging system.

pub mod config;
pub mod error;
pub mod events;
pub mod motes;
pub mod retry;
pub mod types;

// Re-export config submodule contents
pub use config::constants;

pub use config::Config;

// Re-export motes module for monetary units (canonical smallest unit)
pub use error::{Error, Result};
pub use events::{Event, EventBus};
pub use motes::{
    dchat_to_motes, format_motes, format_motes_compact, motes_to_dchat, motes_to_whole_dchat,
    parse_dchat, DisplayMotes, Motes, DCHAT_DECIMALS, MAX_SUPPLY_MOTES, MOTES_PER_DCHAT,
};
pub use retry::{
    categorize_error, with_retry, with_retry_if, with_retry_sync, RetryCategory, RetryConfig,
};
pub use types::*;

/// Version information for the dchat protocol
pub const PROTOCOL_VERSION: &str = "0.1.0";

/// Maximum message size in bytes (1MB)
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Maximum channel name length
pub const MAX_CHANNEL_NAME_LENGTH: usize = 64;

/// Maximum username length  
pub const MAX_USERNAME_LENGTH: usize = 32;
