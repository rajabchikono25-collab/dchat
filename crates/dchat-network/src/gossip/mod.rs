// Gossip protocol module
//
// Implements epidemic-style message propagation with:
// - Configurable fanout for controlled flooding
// - Bloom filter deduplication
// - TTL management
// - Per-peer rate limiting
//
// # Production Requirements
//
// For production deployments, ALWAYS use `Gossip::new_with_keystore()` to ensure:
// - Persistent identity across restarts
// - Consistent reputation tied to stable identity
// - Recipients can verify message authenticity

pub mod flood_control;
pub mod message_cache;
pub mod protocol;

pub use flood_control::{FloodControl, RateLimiter};
pub use message_cache::{MessageCache, MessageId};
pub use protocol::{GossipConfig, GossipMessage, GossipProtocol, MAX_GOSSIP_PAYLOAD_SIZE};

use crate::keystore::RelayKeystore;
use dchat_core::Result;
use libp2p::PeerId;

/// Gossip manager for message propagation
pub struct Gossip {
    protocol: GossipProtocol,
    /// Whether this gossip instance is using persistent keys (production mode)
    is_production: bool,
}

impl Gossip {
    /// Create a new gossip manager with the given config
    ///
    /// # Warning
    /// This method should only be used for testing. For production deployments,
    /// use `Gossip::new_with_keystore()` to ensure persistent identity.
    pub fn new(config: GossipConfig) -> Result<Self> {
        let protocol = GossipProtocol::new(config)?;
        Ok(Self {
            protocol,
            is_production: false,
        })
    }

    /// Create a new gossip manager with persistent keystore for production
    ///
    /// This is the recommended way to create a Gossip instance for production deployments.
    /// The keystore provides:
    /// - Persistent identity across restarts
    /// - Encrypted key storage with passphrase protection
    /// - Deterministic peer ID derivation from signing key
    ///
    /// # Arguments
    /// * `keystore` - A RelayKeystore loaded from encrypted storage
    ///
    /// # Example
    /// ```ignore
    /// use dchat_network::keystore::RelayKeystore;
    /// use dchat_network::gossip::Gossip;
    ///
    /// // Load keystore (requires DCHAT_RELAY_KEYSTORE_PASSPHRASE env var)
    /// let keystore = RelayKeystore::load("/path/to/relay_keystore.age")?;
    ///
    /// // Create gossip with persistent identity
    /// let gossip = Gossip::new_with_keystore(&keystore)?;
    ///
    /// // Verify we're in production mode
    /// assert!(gossip.is_production_mode());
    /// ```
    pub fn new_with_keystore(keystore: &RelayKeystore) -> Result<Self> {
        let protocol = GossipProtocol::new_with_keystore(keystore)?;

        tracing::info!("✅ Gossip initialized with persistent keystore identity");

        Ok(Self {
            protocol,
            is_production: true,
        })
    }

    /// Check if this gossip instance is using persistent keys (production mode)
    ///
    /// Returns `true` if initialized with `new_with_keystore()`, `false` otherwise.
    ///
    /// # Production Enforcement
    /// Applications should check this at startup and fail if not in production mode
    /// when deploying to mainnet:
    /// ```ignore
    /// if !gossip.is_production_mode() && is_mainnet {
    ///     panic!("Mainnet requires persistent gossip keys!");
    /// }
    /// ```
    pub fn is_production_mode(&self) -> bool {
        self.is_production
    }

    /// Get the local peer ID for this gossip instance
    pub fn local_peer_id(&self) -> PeerId {
        self.protocol.local_peer_id()
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

/// Validate that gossip is configured for production
///
/// This function should be called at application startup to ensure
/// the gossip configuration meets production requirements.
///
/// # Panics
/// Panics if `require_production` is true but gossip is not in production mode.
pub fn validate_production_gossip(gossip: &Gossip, require_production: bool) {
    if require_production && !gossip.is_production_mode() {
        panic!(
            "🚨 CRITICAL: Gossip not configured for production! \
            Use Gossip::new_with_keystore() with a persistent keystore. \
            Ephemeral keys are only acceptable for testing/development."
        );
    }

    if gossip.is_production_mode() {
        tracing::info!(
            "✅ Gossip production validation passed: using persistent identity {}",
            gossip.local_peer_id()
        );
    } else {
        tracing::warn!("⚠️ Gossip using ephemeral identity - FOR TESTING ONLY");
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

    #[tokio::test]
    async fn test_ephemeral_not_production() {
        let config = test_config();
        let gossip = Gossip::new(config).unwrap();
        assert!(!gossip.is_production_mode());
    }

    #[test]
    fn test_validate_production_gossip_allows_testing() {
        let config = test_config();
        let gossip = Gossip::new(config).unwrap();
        // Should not panic when require_production is false
        validate_production_gossip(&gossip, false);
    }
}
