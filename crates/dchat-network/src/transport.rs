//! Transport layer configuration for libp2p
//!
//! Supports QUIC (preferred) with TCP fallback for maximum compatibility.
//!
//! QUIC benefits:
//! - 0-RTT connection establishment (vs 3-RTT for TCP+Noise)
//! - Built-in multiplexing (no Yamux overhead)
//! - Connection migration (survives IP changes on mobile)
//! - Better NAT traversal (UDP-based)
//! - No head-of-line blocking

use dchat_core::error::{Error, Result};
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade},
    dns, identity, noise, quic, tcp, yamux, PeerId, Transport,
};
use std::time::Duration;

/// Transport configuration options
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Enable QUIC transport (recommended)
    pub enable_quic: bool,
    /// Enable TCP transport (fallback)
    pub enable_tcp: bool,
    /// Connection timeout in seconds
    pub connection_timeout_secs: u64,
    /// Enable TCP nodelay for lower latency
    pub tcp_nodelay: bool,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            enable_quic: true,
            enable_tcp: true,
            connection_timeout_secs: 20,
            tcp_nodelay: true,
        }
    }
}

/// Build the transport stack for libp2p
///
/// Stack priority:
/// 1. QUIC (UDP) - preferred, 0-RTT, built-in encryption + muxing
/// 2. TCP + Noise + Yamux - fallback for restrictive networks
pub fn build_transport(keypair: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    build_transport_with_config(keypair, &TransportConfig::default())
}

/// Build transport with custom configuration
pub fn build_transport_with_config(
    keypair: &identity::Keypair,
    config: &TransportConfig,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    // Build TCP + Noise + Yamux transport (fallback)
    let tcp_transport = if config.enable_tcp {
        let tcp_config = tcp::Config::default().nodelay(config.tcp_nodelay);
        let tcp = tcp::tokio::Transport::new(tcp_config);

        let dns_transport = dns::tokio::Transport::system(tcp)
            .map_err(|e| Error::network(format!("DNS transport error: {}", e)))?;

        let noise_config = noise::Config::new(keypair)
            .map_err(|e| Error::crypto(format!("Noise config error: {}", e)))?;

        let yamux_config = yamux::Config::default();

        Some(
            dns_transport
                .upgrade(upgrade::Version::V1)
                .authenticate(noise_config)
                .multiplex(yamux_config)
                .timeout(Duration::from_secs(config.connection_timeout_secs)),
        )
    } else {
        None
    };

    // Build QUIC transport (preferred)
    let quic_transport = if config.enable_quic {
        let quic_config = quic::Config::new(keypair);
        Some(
            quic::tokio::Transport::new(quic_config)
                .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer))),
        )
    } else {
        None
    };

    // Combine transports: prefer QUIC, fallback to TCP
    let transport = match (quic_transport, tcp_transport) {
        (Some(quic), Some(tcp)) => {
            // QUIC preferred, TCP fallback
            quic.or_transport(tcp.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer))))
                .map(|either, _| match either {
                    futures::future::Either::Left((peer_id, muxer)) => (peer_id, muxer),
                    futures::future::Either::Right((peer_id, muxer)) => (peer_id, muxer),
                })
                .boxed()
        }
        (Some(quic), None) => {
            // QUIC only
            quic.boxed()
        }
        (None, Some(tcp)) => {
            // TCP only (fallback)
            tcp.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
                .boxed()
        }
        (None, None) => {
            return Err(Error::network("No transport enabled - enable QUIC or TCP"));
        }
    };

    Ok(transport)
}

/// Build TCP-only transport (for testing or restricted environments)
pub fn build_tcp_transport(keypair: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    build_transport_with_config(
        keypair,
        &TransportConfig {
            enable_quic: false,
            enable_tcp: true,
            ..Default::default()
        },
    )
}

/// Build QUIC-only transport (for maximum performance)
pub fn build_quic_transport(
    keypair: &identity::Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    build_transport_with_config(
        keypair,
        &TransportConfig {
            enable_quic: true,
            enable_tcp: false,
            ..Default::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    #[test]
    fn test_build_transport() {
        let keypair = Keypair::generate_ed25519();
        let transport = build_transport(&keypair);
        assert!(transport.is_ok());
    }

    #[test]
    fn test_build_tcp_transport() {
        let keypair = Keypair::generate_ed25519();
        let transport = build_tcp_transport(&keypair);
        assert!(transport.is_ok());
    }

    #[test]
    fn test_build_quic_transport() {
        let keypair = Keypair::generate_ed25519();
        let transport = build_quic_transport(&keypair);
        assert!(transport.is_ok());
    }

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert!(config.enable_quic);
        assert!(config.enable_tcp);
        assert_eq!(config.connection_timeout_secs, 20);
    }
}
