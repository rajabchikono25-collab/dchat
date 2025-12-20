//! Error types for mini-app platform

use std::fmt;
use thiserror::Error;

/// Result type alias for mini-app operations
pub type MiniAppResult<T> = Result<T, MiniAppError>;

/// Mini-app error types
#[derive(Debug, Clone, Error)]
pub enum MiniAppError {
    // ─── Registration Errors ────────────────────────────────────────────────
    /// App not found in registry
    #[error("App not found: {0}")]
    AppNotFound(String),

    /// Developer not found
    #[error("Developer not found: {0}")]
    DeveloperNotFound(String),

    /// Developer not verified
    #[error("Developer not verified: {0}")]
    DeveloperNotVerified(String),

    /// App already registered
    #[error("App already registered: {0}")]
    AppAlreadyRegistered(String),

    /// Invalid app ID format
    #[error("Invalid app ID format: {0}")]
    InvalidAppId(String),

    /// Registration expired
    #[error("Registration expired")]
    RegistrationExpired,

    /// Registration revoked
    #[error("Registration revoked: {0}")]
    RegistrationRevoked(String),

    // ─── Manifest Errors ────────────────────────────────────────────────────
    /// Invalid manifest format
    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    /// Manifest too large
    #[error("Manifest exceeds size limit: {size} > {max}")]
    ManifestTooLarge { size: usize, max: usize },

    /// Invalid manifest version
    #[error("Invalid manifest version: {0}")]
    InvalidManifestVersion(String),

    /// Missing required field in manifest
    #[error("Missing required field: {0}")]
    MissingRequiredField(String),

    /// Invalid URL in manifest
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    // ─── Permission Errors ──────────────────────────────────────────────────
    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Permission not granted
    #[error("Permission not granted: {0}")]
    PermissionNotGranted(String),

    /// Permission expired
    #[error("Permission expired: {0}")]
    PermissionExpired(String),

    /// Too many permissions requested
    #[error("Too many permissions requested: {count} > {max}")]
    TooManyPermissions { count: usize, max: usize },

    /// Invalid permission scope
    #[error("Invalid permission scope: {0}")]
    InvalidPermissionScope(String),

    /// Permission already granted
    #[error("Permission already granted: {0}")]
    PermissionAlreadyGranted(String),

    // ─── Sandbox Errors ─────────────────────────────────────────────────────
    /// Sandbox creation failed
    #[error("Sandbox creation failed: {0}")]
    SandboxCreationFailed(String),

    /// Sandbox execution failed
    #[error("Sandbox execution failed: {0}")]
    SandboxExecutionFailed(String),

    /// Sandbox timeout
    #[error("Sandbox timeout after {0}ms")]
    SandboxTimeout(u64),

    /// Sandbox terminated
    #[error("Sandbox terminated: {0}")]
    SandboxTerminated(String),

    /// Invalid sandbox message
    #[error("Invalid sandbox message: {0}")]
    InvalidSandboxMessage(String),

    /// Sandbox resource limit exceeded
    #[error("Sandbox resource limit exceeded: {resource}: {used} > {limit}")]
    SandboxResourceLimitExceeded {
        resource: String,
        used: u64,
        limit: u64,
    },

    // ─── Intent Errors ──────────────────────────────────────────────────────
    /// Invalid intent format
    #[error("Invalid intent: {0}")]
    InvalidIntent(String),

    /// Intent not found
    #[error("Intent not found: {0}")]
    IntentNotFound(String),

    /// Intent expired
    #[error("Intent expired: {0}")]
    IntentExpired(String),

    /// Intent already executed
    #[error("Intent already executed: {0}")]
    IntentAlreadyExecuted(String),

    /// Intent rate limit exceeded
    #[error("Intent rate limit exceeded: {count}/{max} per minute")]
    IntentRateLimitExceeded { count: u32, max: u32 },

    /// Intent signature invalid
    #[error("Intent signature invalid")]
    IntentSignatureInvalid,

    /// Intent payload too large
    #[error("Intent payload too large: {size} > {max}")]
    IntentPayloadTooLarge { size: usize, max: usize },

    // ─── Receipt Errors ─────────────────────────────────────────────────────
    /// Receipt not found
    #[error("Receipt not found: {0}")]
    ReceiptNotFound(String),

    /// Receipt verification failed
    #[error("Receipt verification failed: {0}")]
    ReceiptVerificationFailed(String),

    /// Insufficient attestations
    #[error("Insufficient attestations: {count}/{required}")]
    InsufficientAttestations { count: usize, required: usize },

    /// Invalid attestation
    #[error("Invalid attestation: {0}")]
    InvalidAttestation(String),

    /// Attestation signature invalid
    #[error("Attestation signature invalid")]
    AttestationSignatureInvalid,

    /// Duplicate attestation
    #[error("Duplicate attestation from signer: {0}")]
    DuplicateAttestation(String),

    // ─── Wallet Errors ──────────────────────────────────────────────────────
    /// Wallet not connected
    #[error("Wallet not connected")]
    WalletNotConnected,

    /// Wallet connection failed
    #[error("Wallet connection failed: {0}")]
    WalletConnectionFailed(String),

    /// Sign request rejected
    #[error("Sign request rejected by user")]
    SignRequestRejected,

    /// Sign request timeout
    #[error("Sign request timeout")]
    SignRequestTimeout,

    /// Invalid sign request
    #[error("Invalid sign request: {0}")]
    InvalidSignRequest(String),

    /// Insufficient balance
    #[error("Insufficient balance: required {required}, available {available}")]
    InsufficientBalance { required: u64, available: u64 },

    // ─── Bot Errors ─────────────────────────────────────────────────────────
    /// Bot not found
    #[error("Bot not found: {0}")]
    BotNotFound(String),

    /// Bot not authorized
    #[error("Bot not authorized: {0}")]
    BotNotAuthorized(String),

    /// Bot authentication failed
    #[error("Bot authentication failed: {0}")]
    BotAuthenticationFailed(String),

    /// Invalid bot command
    #[error("Invalid bot command: {0}")]
    InvalidBotCommand(String),

    /// Bot rate limit exceeded
    #[error("Bot rate limit exceeded")]
    BotRateLimitExceeded,

    // ─── Bridge Errors ──────────────────────────────────────────────────────
    /// Bridge not connected
    #[error("Bridge not connected")]
    BridgeNotConnected,

    /// Bridge message failed
    #[error("Bridge message failed: {0}")]
    BridgeMessageFailed(String),

    /// Bridge timeout
    #[error("Bridge timeout")]
    BridgeTimeout,

    /// Invalid bridge message
    #[error("Invalid bridge message: {0}")]
    InvalidBridgeMessage(String),

    // ─── Crypto Errors ──────────────────────────────────────────────────────
    /// Signature verification failed
    #[error("Signature verification failed")]
    SignatureVerificationFailed,

    /// Invalid public key
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),

    /// Hash mismatch
    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    // ─── General Errors ─────────────────────────────────────────────────────
    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl MiniAppError {
    /// Get error code
    pub fn code(&self) -> u32 {
        match self {
            // Registration errors: 1000-1099
            Self::AppNotFound(_) => 1000,
            Self::DeveloperNotFound(_) => 1001,
            Self::DeveloperNotVerified(_) => 1002,
            Self::AppAlreadyRegistered(_) => 1003,
            Self::InvalidAppId(_) => 1004,
            Self::RegistrationExpired => 1005,
            Self::RegistrationRevoked(_) => 1006,

            // Manifest errors: 1100-1199
            Self::InvalidManifest(_) => 1100,
            Self::ManifestTooLarge { .. } => 1101,
            Self::InvalidManifestVersion(_) => 1102,
            Self::MissingRequiredField(_) => 1103,
            Self::InvalidUrl(_) => 1104,

            // Permission errors: 1200-1299
            Self::PermissionDenied(_) => 1200,
            Self::PermissionNotGranted(_) => 1201,
            Self::PermissionExpired(_) => 1202,
            Self::TooManyPermissions { .. } => 1203,
            Self::InvalidPermissionScope(_) => 1204,
            Self::PermissionAlreadyGranted(_) => 1205,

            // Sandbox errors: 1300-1399
            Self::SandboxCreationFailed(_) => 1300,
            Self::SandboxExecutionFailed(_) => 1301,
            Self::SandboxTimeout(_) => 1302,
            Self::SandboxTerminated(_) => 1303,
            Self::InvalidSandboxMessage(_) => 1304,
            Self::SandboxResourceLimitExceeded { .. } => 1305,

            // Intent errors: 1400-1499
            Self::InvalidIntent(_) => 1400,
            Self::IntentNotFound(_) => 1401,
            Self::IntentExpired(_) => 1402,
            Self::IntentAlreadyExecuted(_) => 1403,
            Self::IntentRateLimitExceeded { .. } => 1404,
            Self::IntentSignatureInvalid => 1405,
            Self::IntentPayloadTooLarge { .. } => 1406,

            // Receipt errors: 1500-1599
            Self::ReceiptNotFound(_) => 1500,
            Self::ReceiptVerificationFailed(_) => 1501,
            Self::InsufficientAttestations { .. } => 1502,
            Self::InvalidAttestation(_) => 1503,
            Self::AttestationSignatureInvalid => 1504,
            Self::DuplicateAttestation(_) => 1505,

            // Wallet errors: 1600-1699
            Self::WalletNotConnected => 1600,
            Self::WalletConnectionFailed(_) => 1601,
            Self::SignRequestRejected => 1602,
            Self::SignRequestTimeout => 1603,
            Self::InvalidSignRequest(_) => 1604,
            Self::InsufficientBalance { .. } => 1605,

            // Bot errors: 1700-1799
            Self::BotNotFound(_) => 1700,
            Self::BotNotAuthorized(_) => 1701,
            Self::BotAuthenticationFailed(_) => 1702,
            Self::InvalidBotCommand(_) => 1703,
            Self::BotRateLimitExceeded => 1704,

            // Bridge errors: 1800-1899
            Self::BridgeNotConnected => 1800,
            Self::BridgeMessageFailed(_) => 1801,
            Self::BridgeTimeout => 1802,
            Self::InvalidBridgeMessage(_) => 1803,

            // Crypto errors: 1900-1999
            Self::SignatureVerificationFailed => 1900,
            Self::InvalidPublicKey(_) => 1901,
            Self::HashMismatch { .. } => 1902,

            // General errors: 2000-2099
            Self::SerializationError(_) => 2000,
            Self::DeserializationError(_) => 2001,
            Self::IoError(_) => 2002,
            Self::InternalError(_) => 2003,
        }
    }

    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::SandboxTimeout(_)
                | Self::BridgeTimeout
                | Self::SignRequestTimeout
                | Self::IntentRateLimitExceeded { .. }
                | Self::BotRateLimitExceeded
        )
    }

    /// Check if error requires user action
    pub fn requires_user_action(&self) -> bool {
        matches!(
            self,
            Self::PermissionDenied(_)
                | Self::PermissionNotGranted(_)
                | Self::SignRequestRejected
                | Self::WalletNotConnected
                | Self::InsufficientBalance { .. }
        )
    }
}

impl From<std::io::Error> for MiniAppError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

impl From<serde_json::Error> for MiniAppError {
    fn from(err: serde_json::Error) -> Self {
        Self::DeserializationError(err.to_string())
    }
}

impl From<url::ParseError> for MiniAppError {
    fn from(err: url::ParseError) -> Self {
        Self::InvalidUrl(err.to_string())
    }
}

impl From<ed25519_dalek::SignatureError> for MiniAppError {
    fn from(_: ed25519_dalek::SignatureError) -> Self {
        Self::SignatureVerificationFailed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        assert_eq!(MiniAppError::AppNotFound("test".to_string()).code(), 1000);
        assert_eq!(
            MiniAppError::PermissionDenied("test".to_string()).code(),
            1200
        );
        assert_eq!(MiniAppError::SandboxTimeout(1000).code(), 1302);
    }

    #[test]
    fn test_is_recoverable() {
        assert!(MiniAppError::SandboxTimeout(1000).is_recoverable());
        assert!(MiniAppError::BridgeTimeout.is_recoverable());
        assert!(!MiniAppError::PermissionDenied("test".to_string()).is_recoverable());
    }

    #[test]
    fn test_requires_user_action() {
        assert!(MiniAppError::PermissionDenied("test".to_string()).requires_user_action());
        assert!(MiniAppError::SignRequestRejected.requires_user_action());
        assert!(!MiniAppError::SandboxTimeout(1000).requires_user_action());
    }
}
