//! dchat-network: Peer-to-peer networking layer using libp2p
//!
//! This crate provides:
//! - Peer discovery via Kademlia DHT and mDNS
//! - Encrypted connections via Noise Protocol
//! - NAT traversal via relay and hole punching (DCUtR)
//! - Message routing and gossip protocols
//! - Relay node infrastructure
//! - Eclipse attack prevention

pub mod behavior;
pub mod connection; // Sprint 9: Connection lifecycle management
pub mod discovery;
pub mod eclipse_prevention; // Phase 3: Eclipse attack prevention
pub mod gossip; // Sprint 9: Gossip protocol for message propagation
pub mod gossip_sync; // Phase 3: Gossip-based synchronization
pub mod nat;
pub mod nat_traversal; // Phase 2: Enhanced NAT traversal (UPnP/TURN)
pub mod onion_routing; // Phase 2: Metadata-resistant routing
pub mod rate_limit; // Sprint 5: Token bucket rate limiting
pub mod rate_limiting; // Phase 2: Reputation-based rate limiting
pub mod relay;
pub mod relay_network; // Phase 3: Full relay network coordination
pub mod routing;
pub mod swarm;
pub mod transport;
pub use behavior::{DchatBehavior, DchatBehaviorEvent, DchatMessage};
pub use connection::{
    ConnectionConfig, ConnectionInfo, ConnectionManager, ConnectionState, ConnectionStats,
};
pub use discovery::{Discovery, DiscoveryConfig};
pub use eclipse_prevention::{
    DiversityStats, EclipseIndicator, EclipsePreventionManager, PeerInfo, RelayPath,
};
pub use gossip::{Gossip, GossipConfig, GossipMessage as GossipProtoMessage, MessageId};
pub use gossip_sync::{ConflictResolution, GossipMessage, GossipSyncManager, VectorClock};
pub use nat::{NatConfig, NatTraversal};
pub use nat_traversal::{NatStrategy, NatTraversalManager, NatType};
pub use onion_routing::{CircuitId, CircuitStatus, OnionRoutingManager};
pub use rate_limit::{RateLimitConfig, RateLimiter};
pub use rate_limiting::{RateLimitManager, ReputationScore};
pub use relay::{RelayClient, RelayConfig, RelayNode};
pub use relay_network::{
    Continent, LoadStrategy, NetworkStats, ProofBatch, RelayInfo, RelayNetworkManager,
};
pub use routing::{Router, RoutingTable};
pub use swarm::{NetworkConfig, NetworkEvent, NetworkManager};
pub use transport::build_transport;

// Re-export libp2p types for convenience
pub use libp2p::{Multiaddr, PeerId};
