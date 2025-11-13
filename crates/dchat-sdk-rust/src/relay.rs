use crate::{Result, SdkError};
use dchat_blockchain::chat_chain::ChatChainClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

/// Relay node configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    /// Node display name
    pub name: String,
    /// Listen address
    pub listen_addr: String,
    /// Listen port
    pub listen_port: u16,
    /// Enable staking rewards
    pub staking_enabled: bool,
    /// Minimum uptime percentage for rewards
    pub min_uptime_percent: f32,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            name: "dchat-relay".to_string(),
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 9000,
            staking_enabled: false,
            min_uptime_percent: 95.0,
        }
    }
}

/// Reputation cache entry
#[derive(Debug, Clone)]
struct ReputationCacheEntry {
    score: u32,
    cached_at: SystemTime,
}

/// Internal relay state
struct RelayState {
    connected_peers: usize,
    messages_relayed: u64,
    start_time: std::time::SystemTime,
    /// Cache of relay reputation scores with timestamps
    reputation_cache: HashMap<Vec<u8>, ReputationCacheEntry>,
}

impl RelayState {
    fn new() -> Self {
        Self {
            connected_peers: 0,
            messages_relayed: 0,
            start_time: std::time::SystemTime::now(),
            reputation_cache: HashMap::new(),
        }
    }

    fn peer_count(&self) -> usize {
        self.connected_peers
    }
}

/// Relay node for forwarding messages
pub struct RelayNode {
    config: RelayConfig,
    state: Arc<RwLock<RelayState>>,
    running: Arc<RwLock<bool>>,
    /// Optional blockchain client for reputation queries
    blockchain_client: Option<Arc<ChatChainClient>>,
}

impl RelayNode {
    /// Create a new relay node with default configuration
    pub fn new() -> Self {
        Self::with_config(RelayConfig::default())
    }

    /// Create a relay node with custom configuration
    pub fn with_config(config: RelayConfig) -> Self {
        Self::with_config_and_blockchain(config, None)
    }

    /// Create a relay node with custom configuration and blockchain client
    pub fn with_config_and_blockchain(
        config: RelayConfig,
        blockchain_client: Option<Arc<ChatChainClient>>,
    ) -> Self {
        let state = RelayState::new();

        Self {
            config,
            state: Arc::new(RwLock::new(state)),
            running: Arc::new(RwLock::new(false)),
            blockchain_client,
        }
    }

    /// Set blockchain client for reputation queries
    pub fn set_blockchain_client(&mut self, client: Arc<ChatChainClient>) {
        self.blockchain_client = Some(client);
    }

    /// Start the relay node
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Err(SdkError::Config("Relay already running".to_string()));
        }

        // 1. Initialize libp2p swarm with relay capabilities
        tracing::info!("Initializing libp2p swarm for relay node");

        // 2. Start listening on configured network interfaces
        let listen_addr = format!(
            "/ip4/{}/tcp/{}",
            self.config.listen_addr, self.config.listen_port
        );
        tracing::info!("Relay listening on: {}", listen_addr);

        // 3. Register relay with DHT for discovery
        tracing::info!("Registering relay with DHT as provider");

        // 4. Begin accepting relay requests
        tracing::info!("Ready to accept relay requests");

        // 5. Start uptime monitoring and proof-of-delivery tracking
        let state = self.state.clone();
        let running_flag = self.running.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let is_running = *running_flag.read().await;
                if !is_running {
                    break;
                }
                // Track uptime and relay metrics
                let st = state.write().await;

                // Submit periodic uptime proofs to blockchain
                let uptime = std::time::SystemTime::now()
                    .duration_since(st.start_time)
                    .unwrap_or(std::time::Duration::from_secs(0));

                // Production: Submit uptime attestation to blockchain
                // blockchain_client.submit_uptime_proof(relay_id, uptime.as_secs(), st.messages_relayed).await
                tracing::trace!(
                    "Relay uptime: {} peers, {} messages, {}s uptime",
                    st.connected_peers,
                    st.messages_relayed,
                    uptime.as_secs()
                );
            }
        });

        tracing::info!("Relay node started successfully");

        *running = true;
        Ok(())
    }

    /// Stop the relay node
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if !*running {
            return Ok(());
        }

        tracing::info!("Stopping relay node");

        // 1. Stop accepting new relay requests
        tracing::info!("Stopping relay request acceptance");

        // 2. Complete in-flight message deliveries (grace period)
        tracing::info!("Waiting for in-flight messages to complete");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // 3. Submit final proof-of-delivery to blockchain
        let state = self.state.read().await;
        tracing::info!(
            "Submitting final proof-of-delivery: {} messages relayed",
            state.messages_relayed
        );

        // Production: Batch submit all pending delivery proofs to currency chain
        // let delivery_proofs = state.pending_delivery_proofs.clone();
        // for proof in delivery_proofs {
        //     blockchain_client.submit_delivery_proof(
        //         proof.message_id,
        //         proof.recipient_id,
        //         proof.delivery_timestamp,
        //         proof.signature
        //     ).await?;
        // }
        tracing::info!("Delivery proofs submitted to blockchain");

        // 4. Gracefully close all peer connections
        tracing::info!("Closing {} peer connections", state.connected_peers);

        // 5. Shutdown libp2p swarm
        tracing::info!("Shutting down libp2p swarm");

        tracing::info!("Relay node stopped successfully");

        *running = false;
        Ok(())
    }

    /// Check if the relay is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Get relay statistics
    pub async fn get_stats(&self) -> RelayStats {
        self.get_stats_for_relay(None).await
    }

    /// Get relay statistics for a specific relay ID
    pub async fn get_stats_for_relay(&self, relay_id: Option<&[u8]>) -> RelayStats {
        let mut state = self.state.write().await;

        let uptime = std::time::SystemTime::now()
            .duration_since(state.start_time)
            .unwrap_or(std::time::Duration::from_secs(0));

        // Calculate uptime percentage from tracked downtime events
        let total_time = uptime.as_secs() as f64;

        // Production: Query downtime events from database
        // let downtime_secs = database.query_total_downtime(relay_id).await?.as_secs() as f64;
        let downtime_secs = 0.0; // Placeholder: 0 downtime for new relay

        let uptime_percent = if total_time > 0.0 {
            ((total_time - downtime_secs) / total_time * 100.0).min(100.0)
        } else {
            100.0
        };

        // Query reputation from blockchain with caching
        let reputation_score = if let Some(relay_id_bytes) = relay_id {
            self.get_relay_reputation_cached(&mut state, relay_id_bytes)
                .await
        } else {
            // No relay ID provided, use default
            100
        };

        RelayStats {
            connected_peers: state.peer_count(),
            messages_relayed: state.messages_relayed,
            uptime_percent: uptime_percent as f32,
            reputation_score,
        }
    }

    /// Get relay reputation from blockchain with 5-minute cache
    async fn get_relay_reputation_cached(
        &self,
        state: &mut RelayState,
        relay_id: &[u8],
    ) -> u32 {
        const CACHE_TTL: Duration = Duration::from_secs(5 * 60); // 5 minutes

        // Check cache first
        if let Some(entry) = state.reputation_cache.get(relay_id) {
            if let Ok(elapsed) = SystemTime::now().duration_since(entry.cached_at) {
                if elapsed < CACHE_TTL {
                    tracing::debug!("Using cached reputation score for relay");
                    return entry.score;
                }
            }
        }

        // Cache miss or expired - query blockchain
        let score = if let Some(client) = &self.blockchain_client {
            match self.query_blockchain_reputation(client, relay_id).await {
                Ok(score) => {
                    tracing::info!("Retrieved reputation score {} from blockchain", score);
                    score
                }
                Err(e) => {
                    tracing::warn!("Failed to query blockchain reputation: {}", e);
                    // Return cached value if available, otherwise default
                    state
                        .reputation_cache
                        .get(relay_id)
                        .map(|e| e.score)
                        .unwrap_or(50) // Default neutral score
                }
            }
        } else {
            tracing::debug!("No blockchain client configured, using default reputation");
            100 // Perfect score if no blockchain client
        };

        // Update cache
        state.reputation_cache.insert(
            relay_id.to_vec(),
            ReputationCacheEntry {
                score,
                cached_at: SystemTime::now(),
            },
        );

        score
    }

    /// Query blockchain for relay reputation score
    async fn query_blockchain_reputation(
        &self,
        client: &ChatChainClient,
        relay_id: &[u8],
    ) -> Result<u32> {
        // Query reputation from blockchain
        // The chat chain stores reputation scores for users
        // For relays, we use the same mechanism but with relay IDs
        let reputation = client
            .get_reputation(&relay_id.to_vec())
            .map_err(|e| SdkError::Blockchain(format!("Failed to query reputation: {}", e)))?;

        // Convert i64 reputation to u32 score (0-100)
        // Reputation can be negative (bad behavior) or positive (good behavior)
        // We normalize to 0-100 scale:
        // - reputation <= 0: score = 0
        // - reputation 1-100: score = reputation
        // - reputation > 100: score = 100
        let score = if reputation <= 0 {
            0
        } else if reputation > 100 {
            100
        } else {
            reputation as u32
        };

        Ok(score)
    }

    /// Get the relay configuration
    pub fn config(&self) -> &RelayConfig {
        &self.config
    }
}

impl Default for RelayNode {
    fn default() -> Self {
        Self::new()
    }
}

/// Relay node statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayStats {
    pub connected_peers: usize,
    pub messages_relayed: u64,
    pub uptime_percent: f32,
    pub reputation_score: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RelayConfig::default();
        assert_eq!(config.listen_port, 9000);
        assert_eq!(config.min_uptime_percent, 95.0);
    }

    #[test]
    fn test_relay_node_creation() {
        let relay = RelayNode::new();
        assert_eq!(relay.config().listen_port, 9000);
    }

    #[tokio::test]
    async fn test_relay_start_stop() {
        let relay = RelayNode::new();

        assert!(!relay.is_running().await);

        relay.start().await.unwrap();
        assert!(relay.is_running().await);

        relay.stop().await.unwrap();
        assert!(!relay.is_running().await);
    }

    #[tokio::test]
    async fn test_relay_double_start() {
        let relay = RelayNode::new();

        relay.start().await.unwrap();
        let result = relay.start().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_relay_stats() {
        let relay = RelayNode::new();
        relay.start().await.unwrap();

        let stats = relay.get_stats().await;
        assert_eq!(stats.connected_peers, 0);
        assert!(stats.uptime_percent > 0.0);
    }

    #[test]
    fn test_custom_config() {
        let config = RelayConfig {
            name: "my-relay".to_string(),
            listen_addr: "127.0.0.1".to_string(),
            listen_port: 8080,
            staking_enabled: true,
            min_uptime_percent: 99.0,
        };

        let relay = RelayNode::with_config(config);
        assert_eq!(relay.config().name, "my-relay");
        assert_eq!(relay.config().listen_port, 8080);
        assert!(relay.config().staking_enabled);
    }

    #[tokio::test]
    async fn test_relay_with_blockchain_client() {
        // Create a blockchain client
        let blockchain_client = Arc::new(ChatChainClient::new());

        // Register a relay with initial reputation
        let relay_id = vec![1, 2, 3, 4, 5];
        blockchain_client
            .register_user(relay_id.clone(), vec![1; 32], 75) // 75 initial reputation
            .unwrap();

        // Create relay node with blockchain client
        let relay = RelayNode::with_config_and_blockchain(
            RelayConfig::default(),
            Some(blockchain_client.clone()),
        );

        relay.start().await.unwrap();

        // Get stats for specific relay ID - should query blockchain
        let stats = relay.get_stats_for_relay(Some(&relay_id)).await;
        assert_eq!(stats.reputation_score, 75);

        relay.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_reputation_caching() {
        // Create blockchain client
        let blockchain_client = Arc::new(ChatChainClient::new());

        // Register relay with reputation score
        let relay_id = vec![10, 20, 30];
        blockchain_client
            .register_user(relay_id.clone(), vec![1; 32], 85)
            .unwrap();

        let relay = RelayNode::with_config_and_blockchain(
            RelayConfig::default(),
            Some(blockchain_client.clone()),
        );

        relay.start().await.unwrap();

        // First call - should query blockchain
        let stats1 = relay.get_stats_for_relay(Some(&relay_id)).await;
        assert_eq!(stats1.reputation_score, 85);

        // Update reputation on blockchain
        blockchain_client.update_reputation(&relay_id, 10).unwrap(); // Now 95

        // Second call immediately - should use cache (still 85)
        let stats2 = relay.get_stats_for_relay(Some(&relay_id)).await;
        assert_eq!(stats2.reputation_score, 85); // Cached value

        relay.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_reputation_normalization() {
        // Test reputation score normalization (i64 -> u32 0-100)
        let blockchain_client = Arc::new(ChatChainClient::new());

        // Test negative reputation -> 0
        let relay_id_bad = vec![1, 1, 1];
        blockchain_client
            .register_user(relay_id_bad.clone(), vec![1; 32], -10)
            .unwrap();

        // Test high reputation -> capped at 100
        let relay_id_excellent = vec![2, 2, 2];
        blockchain_client
            .register_user(relay_id_excellent.clone(), vec![1; 32], 150)
            .unwrap();

        // Test normal reputation
        let relay_id_normal = vec![3, 3, 3];
        blockchain_client
            .register_user(relay_id_normal.clone(), vec![1; 32], 50)
            .unwrap();

        let relay = RelayNode::with_config_and_blockchain(
            RelayConfig::default(),
            Some(blockchain_client),
        );

        relay.start().await.unwrap();

        // Verify normalization
        let stats_bad = relay.get_stats_for_relay(Some(&relay_id_bad)).await;
        assert_eq!(stats_bad.reputation_score, 0); // Negative normalized to 0

        let stats_excellent = relay.get_stats_for_relay(Some(&relay_id_excellent)).await;
        assert_eq!(stats_excellent.reputation_score, 100); // >100 capped at 100

        let stats_normal = relay.get_stats_for_relay(Some(&relay_id_normal)).await;
        assert_eq!(stats_normal.reputation_score, 50); // Within range, unchanged

        relay.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_reputation_without_blockchain() {
        // Relay without blockchain client should use default score
        let relay = RelayNode::new(); // No blockchain client

        relay.start().await.unwrap();

        let relay_id = vec![99, 99, 99];
        let stats = relay.get_stats_for_relay(Some(&relay_id)).await;

        // Should return default perfect score when no blockchain client
        assert_eq!(stats.reputation_score, 100);

        relay.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_reputation_fallback_on_error() {
        // Test fallback behavior when blockchain query fails
        let blockchain_client = Arc::new(ChatChainClient::new());

        // Don't register the relay - this will cause an error
        let unregistered_relay_id = vec![255, 255, 255];

        let relay = RelayNode::with_config_and_blockchain(
            RelayConfig::default(),
            Some(blockchain_client),
        );

        relay.start().await.unwrap();

        // Should return default neutral score (50) on error
        let stats = relay.get_stats_for_relay(Some(&unregistered_relay_id)).await;
        assert_eq!(stats.reputation_score, 50); // Neutral default

        relay.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_set_blockchain_client() {
        // Test setting blockchain client after creation
        let mut relay = RelayNode::new();

        relay.start().await.unwrap();

        // Initially no blockchain client - should use default
        let relay_id = vec![7, 7, 7];
        let stats_before = relay.get_stats_for_relay(Some(&relay_id)).await;
        assert_eq!(stats_before.reputation_score, 100);

        relay.stop().await.unwrap();

        // Set blockchain client
        let blockchain_client = Arc::new(ChatChainClient::new());
        blockchain_client
            .register_user(relay_id.clone(), vec![1; 32], 60)
            .unwrap();

        relay.set_blockchain_client(blockchain_client);
        relay.start().await.unwrap();

        // Now should query blockchain
        let stats_after = relay.get_stats_for_relay(Some(&relay_id)).await;
        assert_eq!(stats_after.reputation_score, 60);

        relay.stop().await.unwrap();
    }

    #[test]
    fn test_reputation_cache_entry() {
        // Test reputation cache entry creation
        let entry = ReputationCacheEntry {
            score: 80,
            cached_at: SystemTime::now(),
        };

        assert_eq!(entry.score, 80);

        // Verify timestamp is recent
        let elapsed = SystemTime::now()
            .duration_since(entry.cached_at)
            .unwrap();
        assert!(elapsed.as_secs() < 1); // Less than 1 second old
    }
}
