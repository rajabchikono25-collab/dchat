/// Peer discovery and DHT bootstrap functionality.
///
/// This module provides decentralized peer discovery with cascading fallback:
/// DNS → DHT → Cached peers.

pub mod dht;

pub use dht::{
    DhtBootstrap, DiscoveredPeer, DiscoveryError, DiscoveryMetrics, DiscoveryMethod,
    CACHED_PEER_MAX_AGE, DHT_QUERY_TIMEOUT, DNS_REFRESH_INTERVAL, MAX_BOOTSTRAP_PEERS,
};
