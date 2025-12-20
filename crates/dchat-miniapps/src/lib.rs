//! dchat-miniapps: Telegram-style mini-app platform for dchat chat chain
//!
//! This crate implements a secure mini-app platform for the dchat chat chain,
//! featuring:
//!
//! - **On-Chain Registry**: Signed package provenance with developer verification
//! - **Client-Side Sandbox**: WebView/iframe isolation with message-passing bridge
//! - **Permission Model**: Deny-by-default permissions with user consent flows
//! - **Bot Bridge**: Authenticated identities for bots and automated flows
//! - **Wallet Integration**: Wallet-invisible flows with Intent→Execute→Receipt
//! - **Cross-Chain UX**: Sign on chat chain, execute on currency chain
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │                     Chat Chain                            │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │
//! │  │  Registry   │  │   Intent    │  │    Receipt      │   │
//! │  │  (on-chain) │  │  (pending)  │  │  (confirmed)    │   │
//! │  └─────────────┘  └─────────────┘  └─────────────────┘   │
//! └──────────────────────────────────────────────────────────┘
//!           ↓                 ↓                  ↑
//! ┌──────────────────────────────────────────────────────────┐
//! │                    Client Layer                           │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │
//! │  │  Sandbox    │  │  Wallet     │  │   Permissions   │   │
//! │  │  (WebView)  │  │  (hidden)   │  │   (enforced)    │   │
//! │  └─────────────┘  └─────────────┘  └─────────────────┘   │
//! └──────────────────────────────────────────────────────────┘
//!                             ↓
//! ┌──────────────────────────────────────────────────────────┐
//! │                   Currency Chain                          │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │
//! │  │  Programs   │  │  Accounts   │  │   Execution     │   │
//! │  └─────────────┘  └─────────────┘  └─────────────────┘   │
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! # Security Model
//!
//! - All mini-apps run in sandboxed environments (iframe/WebView)
//! - Communication via structured message-passing (no direct DOM access)
//! - Permissions must be explicitly granted by users
//! - Developer identity verified via on-chain registration
//! - Cross-chain intents require threshold attestations

#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(clippy::all)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

pub mod bot;
pub mod bridge;
pub mod error;
pub mod intent;
pub mod manifest;
pub mod permissions;
pub mod receipt;
pub mod registry;
pub mod sandbox;
pub mod wallet;

// Re-export primary types
pub use bot::{BotAuthentication, BotBridge, BotCommand, BotContext, BotHandler, BotIdentity};
pub use bridge::{BridgeMessage, BridgeMessageType, MessageBridge, MessageHandler};
pub use error::{MiniAppError, MiniAppResult};
pub use intent::{Intent, IntentBuilder, IntentId, IntentPayload, IntentStatus, PendingIntent};
pub use manifest::{AppManifest, AppMetadata, ManifestVersion, ResourceSpec, RuntimeRequirements};
pub use permissions::{
    Permission, PermissionGrant, PermissionRequest, PermissionScope, PermissionSet,
};
pub use receipt::{
    Attestation, AttestationSet, CrossChainReceipt, Receipt, ReceiptId, ReceiptStatus,
    ReceiptVerifier,
};
pub use registry::{
    AppId, AppRegistration, Developer, DeveloperRegistry, MiniAppRegistry, RegistrationStatus,
    VerifiedApp,
};
pub use sandbox::{SandboxConfig, SandboxContext, SandboxInstance, SandboxMessage, SandboxRuntime};
pub use wallet::{
    SignRequest, SignResponse, WalletConnection, WalletContext, WalletIntegration, WalletVisibility,
};

/// Protocol version for mini-app compatibility
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum manifest size in bytes (1 MB)
pub const MAX_MANIFEST_SIZE: usize = 1024 * 1024;

/// Maximum app bundle size in bytes (50 MB)
pub const MAX_BUNDLE_SIZE: usize = 50 * 1024 * 1024;

/// Maximum permissions per app
pub const MAX_PERMISSIONS_PER_APP: usize = 32;

/// Maximum intents per user per minute (rate limiting)
pub const MAX_INTENTS_PER_MINUTE: u32 = 60;

/// Intent expiry time in seconds
pub const INTENT_EXPIRY_SECONDS: u64 = 300; // 5 minutes

/// Minimum attestations required for receipt verification
pub const MIN_ATTESTATIONS: usize = 3;

/// Attestation threshold (percentage of signers required)
pub const ATTESTATION_THRESHOLD_PERCENT: u8 = 67;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_constants() {
        assert!(MAX_MANIFEST_SIZE > 0);
        assert!(MAX_BUNDLE_SIZE > 0);
        assert!(MAX_PERMISSIONS_PER_APP > 0);
        assert!(MAX_INTENTS_PER_MINUTE > 0);
        assert!(INTENT_EXPIRY_SECONDS > 0);
        assert!(MIN_ATTESTATIONS > 0);
        assert!(ATTESTATION_THRESHOLD_PERCENT <= 100);
    }
}
