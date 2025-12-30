// Peer discovery module
//
// Implements DHT-based peer discovery using Kademlia routing

pub mod bootstrap;
pub mod dht;
pub mod peer_info;
pub mod routing_table;

pub use bootstrap::Bootstrap;
pub use dht::{Dht, DhtConfig, DhtError};
pub use peer_info::{PeerCapabilities, PeerInfo};
pub use routing_table::{KBucket, RoutingTable};

use dchat_core::Result;
use libp2p::{Multiaddr, PeerId};
use std::collections::HashSet;
use std::time::Duration;

/// Discovery configuration (compatible with existing API)
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// Local peer ID
    pub local_peer_id: PeerId,

    /// Bootstrap nodes for initial DHT seeding
    pub bootstrap_nodes: Vec<(PeerId, Multiaddr)>,

    /// Enable mDNS for local discovery
    pub enable_mdns: bool,

    /// Minimum number of peers to maintain
    pub min_peers: usize,

    /// Maximum number of peers to maintain
    pub max_peers: usize,

    /// DHT query timeout
    pub query_timeout: Duration,

    /// K-bucket size
    pub k_bucket_size: usize,

    /// DHT alpha (concurrency parameter)
    pub alpha: usize,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            local_peer_id: PeerId::random(),
            bootstrap_nodes: vec![],
            enable_mdns: false,
            min_peers: 10,
            max_peers: 100,
            query_timeout: Duration::from_secs(60),
            k_bucket_size: 20,
            alpha: 3,
        }
    }
}

impl From<DiscoveryConfig> for DhtConfig {
    fn from(config: DiscoveryConfig) -> Self {
        DhtConfig {
            local_peer_id: config.local_peer_id,
            bootstrap_nodes: config
                .bootstrap_nodes
                .into_iter()
                .map(|(_, addr)| addr)
                .collect(),
            k_bucket_size: config.k_bucket_size,
            alpha: config.alpha,
            query_timeout: config.query_timeout,
        }
    }
}

/// Peer discovery manager
pub struct Discovery {
    dht: Dht,
    config: DiscoveryConfig,
    connected_peers: HashSet<PeerId>,
}

impl Discovery {
    /// Create a new discovery manager
    pub async fn new(config: DiscoveryConfig) -> Result<Self> {
        let dht_config = config.clone().into();
        let dht = Dht::new(dht_config).await?;
        Ok(Self {
            dht,
            config,
            connected_peers: HashSet::new(),
        })
    }

    /// Bootstrap the node into the network
    pub async fn bootstrap(&mut self) -> Result<()> {
        self.dht.bootstrap().await
    }

    /// Find peers closest to a given peer ID
    pub async fn find_peer(&self, peer_id: &PeerId) -> Result<Vec<PeerInfo>> {
        self.dht.find_peer(peer_id).await
    }

    /// Announce this node's presence to the network
    pub async fn announce(&self) -> Result<()> {
        self.dht.announce().await
    }

    /// Get the local routing table
    pub fn routing_table(&self) -> &RoutingTable {
        self.dht.routing_table()
    }

    /// Get all known peers
    pub fn known_peers(&self) -> Vec<PeerInfo> {
        self.routing_table().all_peers()
    }

    /// Get number of known peers
    pub fn peer_count(&self) -> usize {
        self.routing_table().peer_count()
    }

    /// Add a bootstrap node
    pub fn add_bootstrap_node(&mut self, peer_id: PeerId, addr: Multiaddr) {
        self.config.bootstrap_nodes.push((peer_id, addr));
    }

    /// Get bootstrap nodes
    pub fn bootstrap_nodes(&self) -> &[(PeerId, Multiaddr)] {
        &self.config.bootstrap_nodes
    }

    /// Register a discovered peer
    pub fn register_peer(&mut self, peer: PeerInfo) -> Result<()> {
        self.dht.add_peer(peer)
    }

    /// Mark a peer as connected
    pub fn peer_connected(&mut self, peer_id: PeerId) {
        self.connected_peers.insert(peer_id);
    }

    /// Mark a peer as disconnected
    pub fn peer_disconnected(&mut self, peer_id: &PeerId) {
        self.connected_peers.remove(peer_id);
    }

    /// Check if we need more peers
    pub fn needs_more_peers(&self) -> bool {
        self.connected_peers.len() < self.config.min_peers
    }

    /// Check if we have too many peers
    pub fn has_too_many_peers(&self) -> bool {
        self.connected_peers.len() > self.config.max_peers
    }

    /// Get connected peer count
    pub fn connected_count(&self) -> usize {
        self.connected_peers.len()
    }

    /// Get known peer count
    pub fn known_count(&self) -> usize {
        self.peer_count()
    }

    /// Get a peer to potentially disconnect (least recently used)
    pub fn get_excess_peer(&self) -> Option<PeerId> {
        if self.has_too_many_peers() {
            self.connected_peers.iter().next().copied()
        } else {
            None
        }
    }
}

/// Eclipse attack prevention
pub struct EclipseGuard {
    /// Track ASN diversity of connected peers
    peer_asns: std::collections::HashMap<PeerId, u32>,

    /// Maximum fraction of peers from same ASN
    max_asn_fraction: f64,
}

impl EclipseGuard {
    pub fn new(max_asn_fraction: f64) -> Self {
        Self {
            peer_asns: std::collections::HashMap::new(),
            max_asn_fraction,
        }
    }

    /// Check if connecting to a peer would violate diversity constraints
    pub fn should_allow_peer(&self, _peer_id: &PeerId, asn: u32, total_peers: usize) -> bool {
        if total_peers == 0 {
            return true;
        }

        let asn_count = self.peer_asns.values().filter(|&&a| a == asn).count();
        let fraction = (asn_count + 1) as f64 / (total_peers + 1) as f64;

        fraction <= self.max_asn_fraction
    }

    /// Register a peer's ASN
    pub fn register_peer(&mut self, peer_id: PeerId, asn: u32) {
        self.peer_asns.insert(peer_id, asn);
    }

    /// Remove a peer
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peer_asns.remove(peer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Multiaddr imported in parent scope

    fn test_config() -> DiscoveryConfig {
        DiscoveryConfig {
            local_peer_id: PeerId::random(),
            bootstrap_nodes: vec![],
            enable_mdns: true,
            min_peers: 10,
            max_peers: 100,
            query_timeout: Duration::from_secs(30),
            k_bucket_size: 20,
            alpha: 3,
        }
    }

    #[tokio::test]
    async fn test_discovery_creation() {
        let config = test_config();
        let discovery = Discovery::new(config).await;
        assert!(discovery.is_ok());
    }

    #[tokio::test]
    async fn test_peer_count() {
        let config = test_config();
        let discovery = Discovery::new(config).await.unwrap();
        assert_eq!(discovery.peer_count(), 0);
    }

    #[tokio::test]
    async fn test_discovery_basic() {
        let config = test_config();
        let mut discovery = Discovery::new(config).await.unwrap();

        assert_eq!(discovery.connected_count(), 0);
        assert!(discovery.needs_more_peers());

        let peer_id = PeerId::random();
        discovery.peer_connected(peer_id);
        assert_eq!(discovery.connected_count(), 1);
    }

    #[test]
    fn test_eclipse_guard() {
        let mut guard = EclipseGuard::new(0.5); // Max 50% from same ASN

        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        let peer3 = PeerId::random();
        let peer4 = PeerId::random();

        // First peer always allowed
        assert!(guard.should_allow_peer(&peer1, 100, guard.peer_asns.len()));
        guard.register_peer(peer1, 100);

        // Second peer from different ASN
        assert!(guard.should_allow_peer(&peer2, 200, guard.peer_asns.len()));
        guard.register_peer(peer2, 200);

        // Third peer from ASN 100 - should be rejected
        assert!(!guard.should_allow_peer(&peer3, 100, guard.peer_asns.len()));

        // Third peer from new ASN 300 - should be allowed
        assert!(guard.should_allow_peer(&peer3, 300, guard.peer_asns.len()));
        guard.register_peer(peer3, 300);

        // Fourth peer from ASN 100 - exactly at threshold
        assert!(guard.should_allow_peer(&peer4, 100, guard.peer_asns.len()));
    }
}
