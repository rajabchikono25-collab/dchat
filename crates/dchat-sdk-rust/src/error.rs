use thiserror::Error;

/// SDK error types
#[derive(Error, Debug)]
pub enum SdkError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Blockchain error: {0}")]
    Blockchain(String),

    #[error("Identity error: {0}")]
    Identity(String),

    #[error("Message error: {0}")]
    Message(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Already connected")]
    AlreadyConnected,

    #[error("Timeout")]
    Timeout,

    #[error(transparent)]
    Core(#[from] dchat_core::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// SDK result type
pub type Result<T> = std::result::Result<T, SdkError>;
