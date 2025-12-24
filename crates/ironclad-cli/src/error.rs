//! Ironclad error types

use thiserror::Error;

#[derive(Error, Debug)]
pub enum IroncladError {
    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Build failed: {0}")]
    BuildFailed(String),

    #[error("Test failed: {0}")]
    TestFailed(String),

    #[error("Verification failed: {0}")]
    VerificationFailed(String),

    #[error("Deployment failed: {0}")]
    DeploymentFailed(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("WASM parsing error: {0}")]
    WasmError(String),

    #[error("IDL error: {0}")]
    IdlError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Manifest error: {0}")]
    ManifestError(String),

    #[error("Schema hash mismatch: {0}")]
    SchemaMismatch(String),

    #[error("Missing manifest in WASM: {0}")]
    MissingManifest(String),

    #[error("Invalid template: {0}")]
    InvalidTemplate(String),

    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("{0}")]
    Custom(String),
}

pub type IroncladResult<T> = Result<T, IroncladError>;
