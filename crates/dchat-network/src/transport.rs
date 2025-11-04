//! Transport layer configuration for libp2p

use dchat_core::error::{Error, Result};
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade},
    dns, identity, noise, tcp, yamux, PeerId, Transport,
};
use std::time::Duration;

/// Build the transport stack for libp2p
///
/// Stack: TCP/WebSocket → DNS → Noise → Yamux
pub fn build_transport(keypair: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    // TCP transport with custom configuration
    let tcp_config = tcp::Config::default().nodelay(true);

    let tcp_transport = tcp::tokio::Transport::new(tcp_config);

    // DNS resolution (no WebSocket for now due to transport ownership)
    let dns_transport = dns::tokio::Transport::system(tcp_transport)
        .map_err(|e| Error::network(format!("DNS transport error: {}", e)))?;

    // Noise protocol for encryption
    let noise_config = noise::Config::new(keypair)
        .map_err(|e| Error::crypto(format!("Noise config error: {}", e)))?;

    // Yamux multiplexing
    let yamux_config = yamux::Config::default();

    // Build the complete transport
    let transport = dns_transport
        .upgrade(upgrade::Version::V1)
        .authenticate(noise_config)
        .multiplex(yamux_config)
        .timeout(Duration::from_secs(20))
        .boxed();

    Ok(transport)
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
}
