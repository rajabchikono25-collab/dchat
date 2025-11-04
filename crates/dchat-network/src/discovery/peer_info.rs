// Peer information structures

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Information about a peer in the network
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// The peer's unique identifier
    pub peer_id: PeerId,

    /// Known addresses for this peer
    pub addresses: Vec<Multiaddr>,

    /// Last time we saw this peer
    pub last_seen: Instant,

    /// Measured latency to this peer
    pub latency: Option<Duration>,

    /// Reputation score (for future use)
    pub reputation: i32,

    /// Peer capabilities
    pub capabilities: PeerCapabilities,
}

impl PeerInfo {
    /// Create a new PeerInfo with basic information
    pub fn new(peer_id: PeerId, addresses: Vec<Multiaddr>) -> Self {
        Self {
            peer_id,
            addresses,
            last_seen: Instant::now(),
            latency: None,
            reputation: 0,
            capabilities: PeerCapabilities::default(),
        }
    }

    /// Update the last seen timestamp
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Update latency measurement
    pub fn update_latency(&mut self, latency: Duration) {
        self.latency = Some(latency);
    }

    /// Check if peer is stale (not seen recently)
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_seen.elapsed() > timeout
    }

    /// Add a new address if not already present
    pub fn add_address(&mut self, addr: Multiaddr) {
        if !self.addresses.contains(&addr) {
            self.addresses.push(addr);
        }
    }

    /// Get the best address to connect to (prefer recent, low-latency)
    pub fn best_address(&self) -> Option<&Multiaddr> {
        self.addresses.first()
    }
}

/// Capabilities and features supported by a peer
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PeerCapabilities {
    /// Whether this peer is a relay node
    pub is_relay: bool,

    /// Whether this peer supports NAT traversal
    pub supports_nat_traversal: bool,

    /// Maximum bandwidth this peer can handle (bytes/sec)
    pub max_bandwidth: Option<u64>,

    /// Protocol version
    pub protocol_version: String,
}

impl PeerCapabilities {
    /// Create capabilities for a relay node
    pub fn relay() -> Self {
        Self {
            is_relay: true,
            supports_nat_traversal: true,
            max_bandwidth: Some(10_000_000), // 10 MB/s
            protocol_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Create capabilities for a regular user node
    pub fn user() -> Self {
        Self {
            is_relay: false,
            supports_nat_traversal: true,
            max_bandwidth: Some(1_000_000), // 1 MB/s
            protocol_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_peer_info_creation() {
        let peer_id = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();

        let info = PeerInfo::new(peer_id, vec![addr.clone()]);

        assert_eq!(info.peer_id, peer_id);
        assert_eq!(info.addresses.len(), 1);
        assert_eq!(info.addresses[0], addr);
        assert_eq!(info.reputation, 0);
    }

    #[test]
    fn test_peer_staleness() {
        let peer_id = PeerId::random();
        let info = PeerInfo::new(peer_id, vec![]);

        // Should not be stale immediately
        assert!(!info.is_stale(Duration::from_secs(1)));

        // Wait and check staleness
        thread::sleep(Duration::from_millis(100));
        assert!(info.is_stale(Duration::from_millis(50)));
    }

    #[test]
    fn test_add_address() {
        let peer_id = PeerId::random();
        let mut info = PeerInfo::new(peer_id, vec![]);

        let addr1: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.1/tcp/9001".parse().unwrap();

        info.add_address(addr1.clone());
        assert_eq!(info.addresses.len(), 1);

        info.add_address(addr2.clone());
        assert_eq!(info.addresses.len(), 2);

        // Adding duplicate should not increase count
        info.add_address(addr1.clone());
        assert_eq!(info.addresses.len(), 2);
    }

    #[test]
    fn test_peer_capabilities() {
        let relay_caps = PeerCapabilities::relay();
        assert!(relay_caps.is_relay);
        assert!(relay_caps.supports_nat_traversal);
        assert!(relay_caps.max_bandwidth.is_some());

        let user_caps = PeerCapabilities::user();
        assert!(!user_caps.is_relay);
        assert!(user_caps.supports_nat_traversal);
    }
}
