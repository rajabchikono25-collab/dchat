//! Error types for dchat

use thiserror::Error;

/// Main error type for dchat operations
#[derive(Error, Debug)]
pub enum Error {
    #[error("Cryptographic error: {0}")]
    Crypto(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Identity error: {0}")]
    Identity(String),

    #[error("Messaging error: {0}")]
    Messaging(String),

    #[error("Chain error: {0}")]
    Chain(String),

    #[error("Governance error: {0}")]
    Governance(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Resource already exists: {0}")]
    AlreadyExists(String),

    #[error("Operation timeout")]
    Timeout,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias for dchat operations
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Create a crypto error
    pub fn crypto(msg: impl Into<String>) -> Self {
        Self::Crypto(msg.into())
    }

    /// Create a network error
    pub fn network(msg: impl Into<String>) -> Self {
        Self::Network(msg.into())
    }

    /// Create a storage error
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    /// Create an identity error
    pub fn identity(msg: impl Into<String>) -> Self {
        Self::Identity(msg.into())
    }

    /// Create a messaging error
    pub fn messaging(msg: impl Into<String>) -> Self {
        Self::Messaging(msg.into())
    }

    /// Create a chain error
    pub fn chain(msg: impl Into<String>) -> Self {
        Self::Chain(msg.into())
    }

    /// Create an internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Create a validation error
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    /// Create a chain RPC error
    pub fn chain_rpc(msg: impl Into<String>) -> Self {
        Self::Chain(format!("RPC error: {}", msg.into()))
    }

    /// Create a rate limit error
    pub fn rate_limit(msg: impl Into<String>) -> Self {
        Self::PermissionDenied(msg.into())
    }

    /// Create an authentication error
    pub fn unauthenticated(msg: impl Into<String>) -> Self {
        Self::PermissionDenied(format!("Unauthenticated: {}", msg.into()))
    }

    /// Create an unavailable error (e.g., resource or service not available)
    pub fn unavailable(msg: impl Into<String>) -> Self {
        Self::Internal(format!("Unavailable: {}", msg.into()))
    }

    /// Create a not found error
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    /// Create an IO error with custom message
    pub fn io(msg: impl Into<String>) -> Self {
        Self::Internal(format!("IO: {}", msg.into()))
    }

    /// Create a serialization error with custom message
    pub fn serialization(msg: impl Into<String>) -> Self {
        Self::Internal(format!("Serialization: {}", msg.into()))
    }
}
