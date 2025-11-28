//! Error types for dchat-data crate

use thiserror::Error;

/// Result type for data operations
pub type DataResult<T> = Result<T, DataError>;

/// Errors that can occur during data operations
#[derive(Debug, Error)]
pub enum DataError {
    /// Content ID computation or validation failed
    #[error("Content ID error: {0}")]
    ContentId(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deduplication error
    #[error("Deduplication error: {0}")]
    Deduplication(String),

    /// TTL configuration error
    #[error("TTL configuration error: {0}")]
    TtlConfig(String),

    /// Data expired
    #[error("Data expired at {0}")]
    Expired(String),

    /// Data integrity violation
    #[error("Data integrity error: {0}")]
    Integrity(String),

    /// Data not found
    #[error("Data not found: {0}")]
    NotFound(String),

    /// Storage quota exceeded
    #[error("Storage quota exceeded: {used} / {limit} bytes")]
    QuotaExceeded { used: u64, limit: u64 },

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<bincode::Error> for DataError {
    fn from(e: bincode::Error) -> Self {
        DataError::Serialization(e.to_string())
    }
}

impl From<serde_json::Error> for DataError {
    fn from(e: serde_json::Error) -> Self {
        DataError::Serialization(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = DataError::ContentId("invalid hash".to_string());
        assert!(err.to_string().contains("invalid hash"));
    }

    #[test]
    fn test_quota_exceeded_display() {
        let err = DataError::QuotaExceeded {
            used: 1024,
            limit: 512,
        };
        assert!(err.to_string().contains("1024"));
        assert!(err.to_string().contains("512"));
    }
}
