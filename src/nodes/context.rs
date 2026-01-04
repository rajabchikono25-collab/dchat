//! Node Context and Shared Types
//!
//! This module provides shared context and types used across all node types:
//! - NodeContext: Shared initialization for peer management, health, metrics
//! - NodeType: Enum for validator, relay, client nodes
//! - ReadinessState: Health/readiness tracking for Kubernetes probes
//! - PeerInfo: Peer connection metadata
//! - PeerRegistry: Thread-safe peer management
//! - PeerMetrics: Prometheus-compatible metrics

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

// Re-export PeerId and Multiaddr from dchat_network
pub use dchat_network::{Multiaddr, PeerId};

/// Node type in the dchat network
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    Validator,
    Relay,
    Client,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Validator => write!(f, "Validator"),
            NodeType::Relay => write!(f, "Relay"),
            NodeType::Client => write!(f, "Client"),
        }
    }
}

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Unique peer identifier
    pub peer_id: PeerId,
    /// Network address of the peer
    pub multiaddr: Multiaddr,
    /// Type of node (Validator, Relay, Client)
    pub node_type: NodeType,
    /// Geographic region for network topology optimization
    pub geographic_region: Option<String>,
    /// Last time this peer was seen active
    pub last_seen: SystemTime,
    /// Connection quality score (0.0 to 1.0)
    pub connection_quality: f64,
    /// Peer capabilities list
    pub capabilities: Vec<String>,
    /// Whether this is a bootstrap peer
    pub is_bootstrap: bool,
    /// Round-trip time in milliseconds
    pub rtt_ms: Option<f64>,
    /// Packet loss rate (0.0 to 1.0)
    pub packet_loss: f64,
    /// Network jitter in milliseconds
    pub jitter_ms: Option<f64>,
    /// Total messages sent to this peer
    pub total_messages_sent: u64,
    /// Total messages received from this peer
    pub total_messages_received: u64,
    /// Whether initial handshake succeeded
    pub handshake_success: bool,
    /// Time when connection was established
    pub connected_since: SystemTime,
}

impl PeerInfo {
    /// Create a new PeerInfo with sensible defaults
    pub fn new(peer_id: PeerId, multiaddr: Multiaddr, node_type: NodeType) -> Self {
        Self {
            peer_id,
            multiaddr,
            node_type,
            geographic_region: None,
            last_seen: SystemTime::now(),
            connection_quality: 0.8,
            capabilities: vec![],
            is_bootstrap: false,
            rtt_ms: None,
            packet_loss: 0.0,
            jitter_ms: None,
            total_messages_sent: 0,
            total_messages_received: 0,
            handshake_success: false,
            connected_since: SystemTime::now(),
        }
    }

    /// Create with geographic region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.geographic_region = Some(region.into());
        self
    }

    /// Mark as bootstrap peer
    pub fn as_bootstrap(mut self) -> Self {
        self.is_bootstrap = true;
        self
    }
}

/// Shared readiness state for the /ready endpoint
///
/// This tracks whether the node is fully initialized and ready to serve traffic.
/// Used by Kubernetes liveness/readiness probes.
#[derive(Debug, Clone)]
pub struct ReadinessState {
    /// Whether the network layer is initialized and connected to peers
    pub network_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Minimum number of peers required for readiness
    pub min_peers_required: usize,
    /// Current peer count
    pub peer_count: Arc<std::sync::atomic::AtomicUsize>,
    /// Whether the database is initialized
    pub database_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the currency chain RPC is reachable
    pub currency_chain_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the chat chain RPC is reachable
    pub chat_chain_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Last chain health check timestamp (Unix seconds)
    pub last_chain_check: Arc<std::sync::atomic::AtomicU64>,
}

impl ReadinessState {
    /// Create a new readiness state with minimum peer requirement
    pub fn new(min_peers: usize) -> Self {
        Self {
            network_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            min_peers_required: min_peers,
            peer_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            database_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            currency_chain_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            chat_chain_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_chain_check: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Check if all readiness conditions are met
    pub fn is_ready(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.network_ready.load(Ordering::Relaxed)
            && self.database_ready.load(Ordering::Relaxed)
            && self.peer_count.load(Ordering::Relaxed) >= self.min_peers_required
            && self.currency_chain_ready.load(Ordering::Relaxed)
            && self.chat_chain_ready.load(Ordering::Relaxed)
    }

    /// Check if ready without requiring chain connectivity (for nodes that don't need chains)
    pub fn is_ready_without_chains(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.network_ready.load(Ordering::Relaxed)
            && self.database_ready.load(Ordering::Relaxed)
            && self.peer_count.load(Ordering::Relaxed) >= self.min_peers_required
    }

    pub fn set_network_ready(&self, ready: bool) {
        self.network_ready
            .store(ready, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_database_ready(&self, ready: bool) {
        self.database_ready
            .store(ready, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_currency_chain_ready(&self, ready: bool) {
        self.currency_chain_ready
            .store(ready, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_chat_chain_ready(&self, ready: bool) {
        self.chat_chain_ready
            .store(ready, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn update_chain_check_time(&self) {
        use std::sync::atomic::Ordering;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.last_chain_check.store(now, Ordering::Relaxed);
    }

    pub fn update_peer_count(&self, count: usize) {
        self.peer_count
            .store(count, std::sync::atomic::Ordering::Relaxed);
    }

    /// Convert to JSON for /ready endpoint
    pub fn to_json(&self) -> serde_json::Value {
        use std::sync::atomic::Ordering;
        serde_json::json!({
            "ready": self.is_ready(),
            "network_ready": self.network_ready.load(Ordering::Relaxed),
            "database_ready": self.database_ready.load(Ordering::Relaxed),
            "currency_chain_ready": self.currency_chain_ready.load(Ordering::Relaxed),
            "chat_chain_ready": self.chat_chain_ready.load(Ordering::Relaxed),
            "peer_count": self.peer_count.load(Ordering::Relaxed),
            "min_peers_required": self.min_peers_required,
            "last_chain_check": self.last_chain_check.load(Ordering::Relaxed),
        })
    }
}

impl Default for ReadinessState {
    fn default() -> Self {
        Self::new(1) // Default to requiring at least 1 peer
    }
}

/// Thread-safe registry for managing network peers
///
/// Supports:
/// - Geographic-aware peer selection
/// - Connection quality tracking  
/// - Rate limiting and health monitoring
#[derive(Debug, Clone)]
pub struct PeerRegistry {
    /// All connected peers indexed by peer ID
    pub peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    /// Bootstrap peers for initial discovery
    pub bootstrap_peers: Arc<RwLock<Vec<PeerInfo>>>,
}

impl PeerRegistry {
    /// Create a new empty peer registry
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            bootstrap_peers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a peer to the registry
    pub async fn add_peer(&self, peer_info: PeerInfo) {
        let mut peers = self.peers.write().await;
        peers.insert(peer_info.peer_id, peer_info);
    }

    /// Add a peer to the registry with sensible defaults
    pub async fn add_peer_with_defaults(
        &self,
        peer_id: PeerId,
        multiaddr: Multiaddr,
        node_type: NodeType,
        region: Option<String>,
    ) {
        let mut peer_info = PeerInfo::new(peer_id, multiaddr, node_type);
        peer_info.geographic_region = region;
        self.add_peer(peer_info).await;
    }

    /// Remove a peer from the registry
    pub async fn remove_peer(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        peers.remove(peer_id);
    }

    /// Get peer information by ID
    pub async fn get_peer(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        let peers = self.peers.read().await;
        peers.get(peer_id).cloned()
    }

    /// Get all connected peers
    pub async fn get_all_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().cloned().collect()
    }

    /// Get current peer count
    pub async fn peer_count(&self) -> usize {
        let peers = self.peers.read().await;
        peers.len()
    }

    /// Get peers filtered by node type
    pub async fn get_peers_by_type(&self, node_type: NodeType) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|p| p.node_type == node_type)
            .cloned()
            .collect()
    }

    /// Add a bootstrap peer
    pub async fn add_bootstrap_peer(&self, peer_info: PeerInfo) {
        let mut bootstrap = self.bootstrap_peers.write().await;
        bootstrap.push(peer_info);
    }

    /// Get all bootstrap peers
    pub async fn get_bootstrap_peers(&self) -> Vec<PeerInfo> {
        let bootstrap = self.bootstrap_peers.read().await;
        bootstrap.clone()
    }

    /// Update connection quality for a peer
    pub async fn update_peer_quality(&self, peer_id: &PeerId, quality: f64) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.connection_quality = quality;
            peer.last_seen = SystemTime::now();
        }
    }

    /// Remove peers that haven't been seen within the specified duration
    pub async fn prune_stale_peers(&self, max_age: Duration) {
        let mut peers = self.peers.write().await;
        let now = SystemTime::now();
        peers.retain(|_, peer| {
            now.duration_since(peer.last_seen)
                .map(|age| age < max_age)
                .unwrap_or(false)
        });
    }

    /// Update peer round-trip time and connection quality
    pub async fn update_peer_rtt(&self, peer_id: &PeerId, rtt_ms: f64) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.rtt_ms = Some(rtt_ms);
            peer.last_seen = SystemTime::now();
            // Update connection quality based on RTT (lower is better)
            peer.connection_quality = if rtt_ms < 50.0 {
                1.0
            } else if rtt_ms < 150.0 {
                0.8
            } else if rtt_ms < 300.0 {
                0.6
            } else {
                0.4
            };
        }
    }

    /// Record that a message was sent to this peer
    pub async fn record_message_sent(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.total_messages_sent += 1;
        }
    }

    /// Record that a message was received from this peer
    pub async fn record_message_received(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.total_messages_received += 1;
            peer.last_seen = SystemTime::now();
        }
    }

    /// Get best peers by connection quality for a given node type
    pub async fn get_best_peers_by_quality(
        &self,
        node_type: NodeType,
        limit: usize,
    ) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        let mut filtered: Vec<PeerInfo> = peers
            .values()
            .filter(|p| p.node_type == node_type)
            .cloned()
            .collect();

        filtered.sort_by(|a, b| {
            b.connection_quality
                .partial_cmp(&a.connection_quality)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        filtered.truncate(limit);
        filtered
    }

    /// Calculate average connection quality across all peers
    pub async fn calculate_average_quality(&self) -> f64 {
        let peers = self.peers.read().await;
        if peers.is_empty() {
            return 0.0;
        }
        let sum: f64 = peers.values().map(|p| p.connection_quality).sum();
        sum / peers.len() as f64
    }

    /// Calculate average RTT across all peers
    pub async fn calculate_average_rtt(&self) -> f64 {
        let peers = self.peers.read().await;
        let rtts: Vec<f64> = peers.values().filter_map(|p| p.rtt_ms).collect();
        if rtts.is_empty() {
            return 0.0;
        }
        rtts.iter().sum::<f64>() / rtts.len() as f64
    }
}

impl Default for PeerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Prometheus-compatible metrics for peer management
#[derive(Debug, Clone)]
pub struct PeerMetrics {
    /// Peer count by node type
    pub peer_count: Arc<RwLock<HashMap<String, usize>>>,
    /// Total successful handshakes
    pub handshake_success_count: Arc<RwLock<u64>>,
    /// Total failed handshakes
    pub handshake_failure_count: Arc<RwLock<u64>>,
    /// Average round-trip time in milliseconds
    pub average_rtt_ms: Arc<RwLock<f64>>,
    /// Average connection quality (0.0 to 1.0)
    pub average_connection_quality: Arc<RwLock<f64>>,
}

impl PeerMetrics {
    /// Create a new metrics instance
    pub fn new() -> Self {
        Self {
            peer_count: Arc::new(RwLock::new(HashMap::new())),
            handshake_success_count: Arc::new(RwLock::new(0)),
            handshake_failure_count: Arc::new(RwLock::new(0)),
            average_rtt_ms: Arc::new(RwLock::new(0.0)),
            average_connection_quality: Arc::new(RwLock::new(0.0)),
        }
    }

    /// Update peer count for a node type
    pub async fn update_peer_count(&self, node_type: &str, count: usize) {
        let mut counts = self.peer_count.write().await;
        counts.insert(node_type.to_string(), count);
    }

    /// Record a successful handshake
    pub async fn record_handshake_success(&self) {
        let mut count = self.handshake_success_count.write().await;
        *count += 1;
    }

    /// Record a failed handshake
    pub async fn record_handshake_failure(&self) {
        let mut count = self.handshake_failure_count.write().await;
        *count += 1;
    }

    /// Update average RTT
    pub async fn update_average_rtt(&self, rtt_ms: f64) {
        let mut avg = self.average_rtt_ms.write().await;
        *avg = rtt_ms;
    }

    /// Update average connection quality
    pub async fn update_average_quality(&self, quality: f64) {
        let mut avg = self.average_connection_quality.write().await;
        *avg = quality;
    }

    /// Get metrics as Prometheus-compatible text format
    pub async fn to_prometheus(&self) -> String {
        let mut output = String::new();

        // Peer counts by type
        let counts = self.peer_count.read().await;
        for (node_type, count) in counts.iter() {
            output.push_str(&format!(
                "dchat_peer_count{{node_type=\"{}\"}} {}\n",
                node_type, count
            ));
        }

        // Handshake counts
        let success = *self.handshake_success_count.read().await;
        let failure = *self.handshake_failure_count.read().await;
        output.push_str(&format!("dchat_handshake_success_total {}\n", success));
        output.push_str(&format!("dchat_handshake_failure_total {}\n", failure));

        // Average metrics
        let rtt = *self.average_rtt_ms.read().await;
        let quality = *self.average_connection_quality.read().await;
        output.push_str(&format!("dchat_average_rtt_ms {:.2}\n", rtt));
        output.push_str(&format!(
            "dchat_average_connection_quality {:.2}\n",
            quality
        ));

        output
    }
}

impl Default for PeerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared context for all node types
///
/// Provides common initialization for:
/// - Peer registry and metrics
/// - Readiness state tracking
/// - Health/metrics endpoint serving
pub struct NodeContext {
    /// Node type (Validator, Relay, Client)
    pub node_type: NodeType,
    /// Peer registry for connection management
    pub peer_registry: Arc<PeerRegistry>,
    /// Peer metrics for observability
    pub peer_metrics: Arc<PeerMetrics>,
    /// Readiness state for health probes
    pub readiness_state: Arc<ReadinessState>,
}

impl NodeContext {
    /// Create a new node context
    pub fn new(node_type: NodeType, min_peers: usize) -> Self {
        Self {
            node_type,
            peer_registry: Arc::new(PeerRegistry::new()),
            peer_metrics: Arc::new(PeerMetrics::new()),
            readiness_state: Arc::new(ReadinessState::new(min_peers)),
        }
    }

    /// Create a relay node context (requires 0 peers minimum)
    pub fn for_relay() -> Self {
        Self::new(NodeType::Relay, 0)
    }

    /// Create a validator node context (requires 1 peer minimum)
    pub fn for_validator() -> Self {
        Self::new(NodeType::Validator, 1)
    }

    /// Create a client node context (requires 1 peer minimum)
    pub fn for_client() -> Self {
        Self::new(NodeType::Client, 1)
    }

    /// Update metrics from the peer registry
    pub async fn sync_metrics(&self) {
        let peer_count = self.peer_registry.peer_count().await;
        self.readiness_state.update_peer_count(peer_count);

        // Update metrics by node type
        let validators = self
            .peer_registry
            .get_peers_by_type(NodeType::Validator)
            .await
            .len();
        let relays = self
            .peer_registry
            .get_peers_by_type(NodeType::Relay)
            .await
            .len();
        let clients = self
            .peer_registry
            .get_peers_by_type(NodeType::Client)
            .await
            .len();

        self.peer_metrics
            .update_peer_count("validator", validators)
            .await;
        self.peer_metrics.update_peer_count("relay", relays).await;
        self.peer_metrics.update_peer_count("client", clients).await;

        // Update quality metrics
        let avg_quality = self.peer_registry.calculate_average_quality().await;
        let avg_rtt = self.peer_registry.calculate_average_rtt().await;
        self.peer_metrics.update_average_quality(avg_quality).await;
        self.peer_metrics.update_average_rtt(avg_rtt).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_display() {
        assert_eq!(NodeType::Validator.to_string(), "Validator");
        assert_eq!(NodeType::Relay.to_string(), "Relay");
        assert_eq!(NodeType::Client.to_string(), "Client");
    }

    #[test]
    fn test_readiness_state_default() {
        let state = ReadinessState::default();
        assert!(!state.is_ready());
        assert_eq!(state.min_peers_required, 1);
    }

    #[test]
    fn test_readiness_state_ready() {
        let state = ReadinessState::new(0);
        state.set_network_ready(true);
        state.set_database_ready(true);
        state.set_currency_chain_ready(true);
        state.set_chat_chain_ready(true);
        assert!(state.is_ready());
    }

    #[tokio::test]
    async fn test_peer_registry_basic() {
        let registry = PeerRegistry::new();

        // Should start empty
        assert_eq!(registry.peer_count().await, 0);

        // Add a peer - need to create a valid PeerId
        // For testing, we'll just verify the structure works
        assert!(registry.get_all_peers().await.is_empty());
    }

    #[tokio::test]
    async fn test_peer_metrics() {
        let metrics = PeerMetrics::new();

        metrics.record_handshake_success().await;
        metrics.record_handshake_success().await;
        metrics.record_handshake_failure().await;

        assert_eq!(*metrics.handshake_success_count.read().await, 2);
        assert_eq!(*metrics.handshake_failure_count.read().await, 1);
    }

    #[test]
    fn test_node_context_factory_methods() {
        let relay_ctx = NodeContext::for_relay();
        assert_eq!(relay_ctx.node_type, NodeType::Relay);
        assert_eq!(relay_ctx.readiness_state.min_peers_required, 0);

        let validator_ctx = NodeContext::for_validator();
        assert_eq!(validator_ctx.node_type, NodeType::Validator);
        assert_eq!(validator_ctx.readiness_state.min_peers_required, 1);

        let client_ctx = NodeContext::for_client();
        assert_eq!(client_ctx.node_type, NodeType::Client);
        assert_eq!(client_ctx.readiness_state.min_peers_required, 1);
    }
}
