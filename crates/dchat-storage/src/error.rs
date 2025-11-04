//! Error types for dchat-storage

use thiserror::Error;

/// Storage error type
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Object storage error: {0}")]
    ObjectStorage(String),

    #[error("TiKV error: {0}")]
    TiKV(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Operation timeout")]
    Timeout,

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Backup error: {0}")]
    Backup(String),

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Deduplication error: {0}")]
    Deduplication(String),

    #[error("Economics error: {0}")]
    Economics(String),

    #[error("Tier management error: {0}")]
    TierManagement(String),

    #[error("Lifecycle error: {0}")]
    Lifecycle(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias for storage operations
pub type StorageResult<T> = std::result::Result<T, StorageError>;

impl StorageError {
    /// Create a database error
    pub fn database(msg: impl Into<String>) -> Self {
        Self::Database(msg.into())
    }

    /// Create a cache error
    pub fn cache(msg: impl Into<String>) -> Self {
        Self::Cache(msg.into())
    }

    /// Create an object storage error
    pub fn object_storage(msg: impl Into<String>) -> Self {
        Self::ObjectStorage(msg.into())
    }

    /// Create a TiKV error
    pub fn tikv(msg: impl Into<String>) -> Self {
        Self::TiKV(msg.into())
    }

    /// Create a serialization error
    pub fn serialization(msg: impl Into<String>) -> Self {
        Self::Serialization(msg.into())
    }

    /// Create a configuration error
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Create an internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

/// Convert from dchat-core Error to StorageError
impl From<dchat_core::error::Error> for StorageError {
    fn from(err: dchat_core::error::Error) -> Self {
        match err {
            dchat_core::error::Error::Storage(s) => Self::Internal(s),
            dchat_core::error::Error::Serialization(e) => Self::Serialization(e.to_string()),
            dchat_core::error::Error::Io(e) => Self::Io(e),
            dchat_core::error::Error::Timeout => Self::Timeout,
            dchat_core::error::Error::NotFound(s) => Self::NotFound(s),
            dchat_core::error::Error::AlreadyExists(s) => Self::AlreadyExists(s),
            dchat_core::error::Error::InvalidInput(s) => Self::InvalidInput(s),
            dchat_core::error::Error::Config(s) => Self::Config(s),
            other => Self::Internal(other.to_string()),
        }
    }
}

/// Convert StorageError to dchat-core Error
impl From<StorageError> for dchat_core::error::Error {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::Database(s) => Self::Storage(format!("Database: {}", s)),
            StorageError::Cache(s) => Self::Storage(format!("Cache: {}", s)),
            StorageError::ObjectStorage(s) => Self::Storage(format!("Object storage: {}", s)),
            StorageError::TiKV(s) => Self::Storage(format!("TiKV: {}", s)),
            StorageError::Serialization(s) => Self::Internal(format!("Serialization: {}", s)),
            StorageError::Io(e) => Self::Io(e),
            StorageError::Timeout => Self::Timeout,
            StorageError::Config(s) => Self::Config(s),
            StorageError::NotFound(s) => Self::NotFound(s),
            StorageError::AlreadyExists(s) => Self::AlreadyExists(s),
            StorageError::InvalidInput(s) => Self::InvalidInput(s),
            other => Self::Storage(other.to_string()),
        }
    }
}
