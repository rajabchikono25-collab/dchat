// Gossip protocol implementation

use super::flood_control::FloodControl;
use super::message_cache::{MessageCache, MessageId};
use dchat_core::Result;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Gossip protocol configuration
#[derive(Debug, Clone)]
pub struct GossipConfig {
    /// Local peer ID
    pub local_peer_id: PeerId,
    
    /// Number of peers to forward messages to (fanout)
    pub fanout: usize,
    
    /// Maximum size of message cache
    pub message_cache_size: usize,
    
    /// Maximum TTL for messages
    pub max_ttl: u8,
    
    /// Cache time-to-live
    pub cache_ttl: Duration,
    
    /// Per-peer rate limit (messages per second)
    pub per_peer_rate_limit: u32,
    
    /// Global rate limit (messages per second)
    pub global_rate_limit: u32,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            local_peer_id: PeerId::random(),
            fanout: 6,
            message_cache_size: 10000,
            max_ttl: 32,
            cache_ttl: Duration::from_secs(300),
            per_peer_rate_limit: 10,
            global_rate_limit: 1000,
        }
    }
}

/// Gossip message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipMessage {
    /// Message ID (hash of payload)
    pub id: MessageId,
    
    /// Original sender
    #[serde(skip)]
    pub sender: Option<PeerId>,
    
    /// Time-to-live (hop count)
    pub ttl: u8,
    
    /// Encrypted payload
    pub payload: Vec<u8>,
    
    /// Unix timestamp
    pub timestamp: u64,
    
    /// Signature (placeholder for now)
    pub signature: Vec<u8>,
}

impl GossipMessage {
    /// Create a new gossip message
    pub fn new(payload: Vec<u8>, max_ttl: u8, sender: PeerId) -> Self {
        let id = MessageId::from_payload(&payload);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let signature = Self::sign_message(&id.to_string(), &payload, timestamp, max_ttl);
        
        Self {
            id,
            sender: Some(sender),
            ttl: max_ttl,
            payload: payload.clone(),
            timestamp,
            signature,
        }
    }

    /// Decrement TTL and return whether message should continue propagating
    pub fn decrement_ttl(&mut self) -> bool {
        if self.ttl > 0 {
            self.ttl -= 1;
            true
        } else {
            false
        }
    }

    /// Check if message is stale (older than 5 minutes)
    pub fn is_stale(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        now.saturating_sub(self.timestamp) > 300
    }

    /// Verify signature
    pub fn verify_signature(&self) -> bool {
        // For now, return true if signature exists
        // In production, this would verify using the sender's public key
        // which would be passed separately or included in the message
        !self.signature.is_empty()
    }
    
    /// Sign message with Ed25519 (helper function)
    fn sign_message(message_id: &str, payload: &[u8], timestamp: u64, ttl: u8) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        
        // Create deterministic signature from message components
        // In production, this would use actual Ed25519 signing with a private key
        let mut hasher = Sha256::new();
        hasher.update(message_id.as_bytes());
        hasher.update(payload);
        hasher.update(timestamp.to_le_bytes());
        hasher.update(&[ttl]);
        
        hasher.finalize().to_vec()
    }
}

/// Gossip protocol implementation
pub struct GossipProtocol {
    config: GossipConfig,
    message_cache: MessageCache,
    flood_control: FloodControl,
    connected_peers: HashMap<PeerId, PeerState>,
}

/// Per-peer state
#[derive(Debug)]
#[allow(dead_code)]
struct PeerState {
    /// Last seen timestamp
    last_seen: SystemTime,
    
    /// Latency estimate
    latency: Option<Duration>,
    
    /// Number of messages forwarded to this peer
    messages_sent: u64,
}

impl PeerState {
    fn new() -> Self {
        Self {
            last_seen: SystemTime::now(),
            latency: None,
            messages_sent: 0,
        }
    }
}

impl GossipProtocol {
    /// Create a new gossip protocol instance
    pub fn new(config: GossipConfig) -> Result<Self> {
        let message_cache = MessageCache::new(
            config.message_cache_size,
            config.cache_ttl,
        )?;
        
        let flood_control = FloodControl::new(
            config.per_peer_rate_limit,
            config.global_rate_limit,
        );
        
        Ok(Self {
            config,
            message_cache,
            flood_control,
            connected_peers: HashMap::new(),
        })
    }

    /// Broadcast a message to the network
    pub async fn broadcast(&mut self, payload: Vec<u8>) -> Result<MessageId> {
        let message = GossipMessage::new(
            payload,
            self.config.max_ttl,
            self.config.local_peer_id,
        );
        
        let message_id = message.id;
        
        // Mark as seen (don't process our own broadcast)
        self.message_cache.mark_seen(message_id);
        
        // Select peers to forward to
        let peers = self.select_forward_peers(&message, self.config.fanout);
        
        tracing::debug!(
            "Broadcasting message {:?} to {} peers",
            message_id,
            peers.len()
        );
        
        // Forward to selected peers (actual network send would happen here)
        for peer_id in peers {
            self.record_forward(&peer_id);
        }
        
        Ok(message_id)
    }

    /// Handle incoming gossip message
    pub async fn handle_incoming(
        &mut self,
        from: PeerId,
        mut message: GossipMessage,
    ) -> Result<()> {
        // Check rate limits
        if !self.flood_control.check_rate_limit(&from) {
            tracing::warn!("Rate limit exceeded for peer {:?}", from);
            return Ok(());
        }
        
        self.flood_control.record_message(&from);
        
        // Check if we've seen this message before
        if self.message_cache.has_seen(&message.id) {
            tracing::trace!("Duplicate message {:?}, ignoring", message.id);
            return Ok(());
        }
        
        // Verify signature
        if !message.verify_signature() {
            tracing::warn!("Invalid signature on message {:?}", message.id);
            return Ok(());
        }
        
        // Check if message is stale
        if message.is_stale() {
            tracing::debug!("Stale message {:?}, ignoring", message.id);
            return Ok(());
        }
        
        // Mark as seen
        self.message_cache.mark_seen(message.id);
        
        // Process message payload
        tracing::debug!(
            "Received gossip message {:?} from {:?}, TTL: {}",
            message.id,
            from,
            message.ttl
        );
        
        // Decrement TTL and check if we should forward
        if !message.decrement_ttl() {
            tracing::trace!("Message {:?} TTL expired, not forwarding", message.id);
            return Ok(());
        }
        
        // Select peers to forward to (exclude sender)
        let peers = self.select_forward_peers_excluding(&message, self.config.fanout, &from);
        
        if !peers.is_empty() {
            tracing::debug!(
                "Forwarding message {:?} to {} peers",
                message.id,
                peers.len()
            );
            
            // Forward to selected peers
            for peer_id in peers {
                self.record_forward(&peer_id);
            }
        }
        
        Ok(())
    }

    /// Check if a message should be forwarded
    pub fn should_forward(&self, message: &GossipMessage) -> bool {
        // Already seen?
        if self.message_cache.has_seen(&message.id) {
            return false;
        }
        
        // TTL expired?
        if message.ttl == 0 {
            return false;
        }
        
        // Message too old?
        if message.is_stale() {
            return false;
        }
        
        true
    }

    /// Select peers to forward message to
    fn select_forward_peers(&self, _message: &GossipMessage, count: usize) -> Vec<PeerId> {
        // Simple strategy: select random peers
        // In production, prioritize by:
        // - Latency (prefer low-latency peers)
        // - Reputation
        // - Geographic diversity
        
        self.connected_peers
            .keys()
            .take(count)
            .copied()
            .collect()
    }

    /// Select peers to forward message to, excluding a specific peer
    fn select_forward_peers_excluding(
        &self,
        _message: &GossipMessage,
        count: usize,
        exclude: &PeerId,
    ) -> Vec<PeerId> {
        self.connected_peers
            .keys()
            .filter(|&p| p != exclude)
            .take(count)
            .copied()
            .collect()
    }

    /// Record that we forwarded a message to a peer
    fn record_forward(&mut self, peer_id: &PeerId) {
        if let Some(state) = self.connected_peers.get_mut(peer_id) {
            state.messages_sent += 1;
        }
    }

    /// Add a connected peer
    pub fn add_peer(&mut self, peer_id: PeerId) {
        self.connected_peers.insert(peer_id, PeerState::new());
    }

    /// Remove a disconnected peer
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.connected_peers.remove(peer_id);
        self.flood_control.remove_peer(peer_id);
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        self.message_cache.stats()
    }

    /// Perform periodic maintenance
    pub async fn maintain(&mut self) -> Result<()> {
        // Cleanup expired messages from cache
        self.message_cache.cleanup_expired();
        
        // Reset flood control counters
        self.flood_control.reset_if_needed();
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> GossipConfig {
        GossipConfig {
            local_peer_id: PeerId::random(),
            fanout: 6,
            message_cache_size: 100,
            max_ttl: 32,
            cache_ttl: Duration::from_secs(300),
            per_peer_rate_limit: 10,
            global_rate_limit: 1000,
        }
    }

    #[tokio::test]
    async fn test_protocol_creation() {
        let config = test_config();
        let protocol = GossipProtocol::new(config);
        assert!(protocol.is_ok());
    }

    #[tokio::test]
    async fn test_message_creation() {
        let payload = b"test message".to_vec();
        let sender = PeerId::random();
        let message = GossipMessage::new(payload.clone(), 32, sender);
        
        assert_eq!(message.ttl, 32);
        assert_eq!(message.payload, payload);
        assert!(message.sender.is_some());
    }

    #[tokio::test]
    async fn test_ttl_decrement() {
        let payload = b"test".to_vec();
        let mut message = GossipMessage::new(payload, 3, PeerId::random());
        
        assert_eq!(message.ttl, 3);
        assert!(message.decrement_ttl());
        assert_eq!(message.ttl, 2);
        assert!(message.decrement_ttl());
        assert_eq!(message.ttl, 1);
        assert!(message.decrement_ttl());
        assert_eq!(message.ttl, 0);
        assert!(!message.decrement_ttl()); // Should return false when TTL is 0
    }

    #[tokio::test]
    async fn test_duplicate_detection() {
        let config = test_config();
        let mut protocol = GossipProtocol::new(config).unwrap();
        
        let payload = b"test message".to_vec();
        let sender = PeerId::random();
        let message = GossipMessage::new(payload, 32, sender);
        
        // First time should be processed
        assert!(protocol.should_forward(&message));
        
        // Mark as seen
        protocol.message_cache.mark_seen(message.id);
        
        // Second time should be rejected
        assert!(!protocol.should_forward(&message));
    }

    #[tokio::test]
    async fn test_peer_management() {
        let config = test_config();
        let mut protocol = GossipProtocol::new(config).unwrap();
        
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        
        assert_eq!(protocol.connected_peers.len(), 0);
        
        protocol.add_peer(peer1);
        assert_eq!(protocol.connected_peers.len(), 1);
        
        protocol.add_peer(peer2);
        assert_eq!(protocol.connected_peers.len(), 2);
        
        protocol.remove_peer(&peer1);
        assert_eq!(protocol.connected_peers.len(), 1);
    }

    #[tokio::test]
    async fn test_broadcast() {
        let config = test_config();
        let mut protocol = GossipProtocol::new(config).unwrap();
        
        // Add some peers
        for _ in 0..5 {
            protocol.add_peer(PeerId::random());
        }
        
        let payload = b"broadcast test".to_vec();
        let result = protocol.broadcast(payload).await;
        
        assert!(result.is_ok());
        let message_id = result.unwrap();
        
        // Should be in cache
        assert!(protocol.message_cache.has_seen(&message_id));
    }
}
