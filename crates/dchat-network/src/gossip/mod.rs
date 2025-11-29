// Gossip protocol module
//
// Implements epidemic-style message propagation with:
// - Configurable fanout for controlled flooding
// - Bloom filter deduplication
// - TTL management
// - Per-peer rate limiting

pub mod flood_control;
pub mod message_cache;
pub mod protocol;

pub use flood_control::{FloodControl, RateLimiter};
pub use message_cache::{MessageCache, MessageId};
pub use protocol::{GossipConfig, GossipMessage, GossipProtocol};

use dchat_core::Result;
use libp2p::PeerId;

/// Gossip manager for message propagation
pub struct Gossip {
    protocol: GossipProtocol,
}

impl Gossip {
    /// Create a new gossip manager
    pub fn new(config: GossipConfig) -> Result<Self> {
        let protocol = GossipProtocol::new(config)?;
        Ok(Self { protocol })
    }

    /// Broadcast a message to the network
    pub async fn broadcast(&mut self, payload: Vec<u8>) -> Result<MessageId> {
        self.protocol.broadcast(payload).await
    }

    /// Handle incoming gossip message
    pub async fn handle_message(&mut self, from: PeerId, message: GossipMessage) -> Result<()> {
        self.protocol.handle_incoming(from, message).await
    }

    /// Get message cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        self.protocol.cache_stats()
    }

    /// Perform periodic maintenance
    pub async fn maintain(&mut self) -> Result<()> {
        self.protocol.maintain().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use libp2p::PeerId;
    use rand::rngs::OsRng;
    use std::sync::Arc;
    use std::time::Duration;

    fn test_config() -> GossipConfig {
        let signing_key = Arc::new(SigningKey::generate(&mut OsRng));
        GossipConfig {
            local_peer_id: PeerId::random(),
            signing_key,
            fanout: 6,
            message_cache_size: 10000,
            max_ttl: 32,
            cache_ttl: Duration::from_secs(300),
            per_peer_rate_limit: 10,
            global_rate_limit: 1000,
        }
    }

    #[tokio::test]
    async fn test_gossip_creation() {
        let config = test_config();
        let gossip = Gossip::new(config);
        assert!(gossip.is_ok());
    }

    #[tokio::test]
    async fn test_broadcast() {
        let config = test_config();
        let mut gossip = Gossip::new(config).unwrap();

        let payload = b"test message".to_vec();
        let result = gossip.broadcast(payload).await;
        assert!(result.is_ok());
    }
}
