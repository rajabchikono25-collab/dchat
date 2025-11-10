/// Cryptographic integration for dchat networking layer.
///
/// This module provides higher-level cryptographic functionality that
/// integrates dchat-crypto primitives with networking and identity systems.

pub mod handshake;
pub mod versioning;

pub use handshake::{
    spawn_timeout_cleanup_task, HandshakeError, HandshakeMetrics, NegotiationError,
    NegotiationMetrics, NoiseHandshakeManager, VersionMessage, VersionNegotiator,
    HANDSHAKE_TIMEOUT, NEGOTIATION_TIMEOUT,
};
pub use versioning::{negotiate_version, NegotiationResult, ProtocolVersion, VersionError};
