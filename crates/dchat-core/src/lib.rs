//! dchat-core: Core types and utilities for the dchat system
//!
//! This crate provides fundamental types, traits, and utilities used across
//! the dchat decentralized messaging system.

pub mod config;
pub mod error;
pub mod events;
pub mod types;

pub use config::Config;
pub use error::{Error, Result};
pub use events::{Event, EventBus};
pub use types::*;

/// Version information for the dchat protocol
pub const PROTOCOL_VERSION: &str = "0.1.0";

/// Maximum message size in bytes (1MB)
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Maximum channel name length
pub const MAX_CHANNEL_NAME_LENGTH: usize = 64;

/// Maximum username length  
pub const MAX_USERNAME_LENGTH: usize = 32;
