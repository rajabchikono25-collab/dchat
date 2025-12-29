//! dchat-network: Peer-to-peer networking layer using libp2p
//!
//! This crate provides:
//! - Peer discovery via Kademlia DHT and mDNS
//! - Encrypted connections via Noise Protocol
//! - NAT traversal via relay and hole punching (DCUtR)
//! - Message routing and gossip protocols
//! - Relay node infrastructure
//! - Eclipse attack prevention
//! - Handshake rate limiting for DoS protection

pub mod behavior;
pub mod connection; // Sprint 9: Connection lifecycle management
pub mod discovery;
pub mod dns_discovery; // Mainnet: DNS-based peer discovery via subdomains
pub mod eclipse_prevention; // Phase 3: Eclipse attack prevention
pub mod gossip; // Sprint 9: Gossip protocol for message propagation
pub mod gossip_sync; // Phase 3: Gossip-based synchronization
pub mod handshake_rate_limit; // Phase 3: Handshake DoS protection
pub mod keystore; // Mainnet: Persistent relay X25519 keys
pub mod nat;
pub mod nat_traversal; // Phase 2: Enhanced NAT traversal (UPnP/TURN)
pub mod network; // Network modules: nat_telemetry, onion routing
pub mod onion_routing; // Phase 2: Metadata-resistant routing
pub mod rate_limit; // Sprint 5: Token bucket rate limiting
pub mod rate_limiting; // Phase 2: Reputation-based rate limiting
pub mod relay; // Relay modules: proof, reputation
pub mod relay_network; // Phase 3: Full relay network coordination
pub mod routing;
pub mod swarm;
pub mod transport;

// Re-export network submodules
pub use network::nat_telemetry;
pub use network::onion;

// Re-export relay submodules
pub use relay::epoch_token;
pub use relay::proof;
pub use relay::reputation;

// Re-export commonly used types from relay modules
pub use behavior::{DchatBehavior, DchatBehaviorEvent, DchatMessage};
pub use connection::{
    ConnectionConfig, ConnectionInfo, ConnectionManager, ConnectionState, ConnectionStats,
};
pub use discovery::{Discovery, DiscoveryConfig};
pub use dns_discovery::{DiscoveredPeer, DnsDiscoveryConfig, DnsDiscoveryManager, NodeType};
pub use eclipse_prevention::{
    DiversityStats, EclipseIndicator, EclipsePreventionManager, PeerInfo, RelayPath,
};
pub use gossip::{Gossip, GossipConfig, GossipMessage as GossipProtoMessage, MessageId};
pub use gossip_sync::{ConflictResolution, GossipMessage, GossipSyncManager, VectorClock};
pub use handshake_rate_limit::{
    HandshakeRateLimitConfig, HandshakeRateLimiter, RateLimitReason, RateLimitResult,
    RateLimitToken, RateLimiterStats,
};
pub use keystore::{default_keystore_path, RelayKeystore};
pub use nat::{NatConfig, NatTraversal};
pub use nat_traversal::{NatStrategy, NatTraversalManager, NatType};
pub use onion_routing::{CircuitId, CircuitStatus, OnionRoutingManager, RelayResult};
pub use rate_limit::{RateLimitConfig, RateLimiter};
pub use rate_limiting::{RateLimitManager, ReputationScore};
pub use relay::proof::{
    BatchAccumulator, BatchId, DeliveryProof, MessageId as RelayMessageId,
    ProofBatch as RelayProofBatch,
};
pub use relay::reputation::{
    RelayMetrics, RelayReputationScore, RelayReputationScorer, ReputationTier,
};
pub use relay::epoch_token::{
    ConversationType, EpochToken, EpochTokenIssuer, EpochTokenManager, EpochTokenRequest,
    EpochTokenResponse, EpochTokenShare, MembershipProof, TokenAggregationSession,
    TokenRejectionReason, current_epoch_id, epoch_end, epoch_id_for_timestamp, epoch_start,
    is_in_epoch_with_grace, EPOCH_DURATION_SECS, EPOCH_GRACE_PERIOD_SECS, MAX_CACHED_EPOCHS,
    MAX_TOKENS_PER_DEVICE_PER_EPOCH, QUORUM_SIZE_1TO1, QUORUM_SIZE_CHANNEL_LARGE,
    QUORUM_SIZE_CHANNEL_SMALL, THRESHOLD_1TO1, THRESHOLD_CHANNEL_LARGE, THRESHOLD_CHANNEL_SMALL,
};
// Note: RelayClient, RelayConfig, RelayNode were in old relay.rs (removed in Phase 3 migration)
pub use relay_network::{
    Continent, LoadStrategy, NetworkStats, ProofBatch, RelayInfo, RelayNetworkConfig,
    RelayNetworkManager, RewardDistribution, StakingBackend, RELAY_LOCK_DURATION,
    MIN_STAKE_CONFIRMATIONS,
};
pub use routing::{Router, RoutingTable};
pub use swarm::{NetworkConfig, NetworkEvent, NetworkManager};
pub use transport::build_transport;

// Re-export libp2p types for convenience
pub use libp2p::{Multiaddr, PeerId};
