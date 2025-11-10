/// Handshake integration layer for dchat networking.
///
/// This module bridges the dchat-crypto handshake system with the dchat
/// networking and identity infrastructure.
pub mod negotiation;
pub mod noise;

pub use negotiation::{
    NegotiationError, NegotiationMetrics, VersionMessage, VersionNegotiator, NEGOTIATION_TIMEOUT,
};
pub use noise::{
    spawn_timeout_cleanup_task, HandshakeError, HandshakeMetrics, NoiseHandshakeManager, Result,
    HANDSHAKE_TIMEOUT,
};
