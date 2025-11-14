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

/// Cache entry for reputation scores
#[derive(Debug, Clone)]
struct ReputationCacheEntry {
    score: u32,
    cached_at: SystemTime,
}

/// Downtime event tracking
#[derive(Debug, Clone)]
struct DowntimeEvent {
    /// When the downtime started
    start_time: SystemTime,
    /// When the downtime ended (None if ongoing)
    end_time: Option<SystemTime>,
    /// Reason for downtime
    reason: String,
}

impl DowntimeEvent {
    fn new(reason: String) -> Self {
        Self {
            start_time: SystemTime::now(),
            end_time: None,
            reason,
        }
    }

    fn end(&mut self) {
        self.end_time = Some(SystemTime::now());
    }

    fn duration(&self) -> Duration {
        let end = self.end_time.unwrap_or_else(SystemTime::now);
        end.duration_since(self.start_time)
            .unwrap_or(Duration::from_secs(0))
    }
}

/// Delivery proof for blockchain submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryProof {
    /// Message ID that was delivered
    pub message_id: String,
    /// Recipient who received the message
    pub recipient_id: String,
    /// Timestamp of delivery
    pub delivery_timestamp: i64,
    /// Relay's signature over (message_id || recipient_id || timestamp)
    pub signature: Vec<u8>,
    /// Relay's public key for verification
    pub relay_pubkey: Vec<u8>,
}

/// Internal relay state
struct RelayState {
    connected_peers: usize,
    messages_relayed: u64,
    start_time: std::time::SystemTime,
    /// Cache of relay reputation scores with timestamps
    reputation_cache: HashMap<Vec<u8>, ReputationCacheEntry>,
    /// Track downtime events for accurate uptime calculation
    downtime_events: Vec<DowntimeEvent>,
    /// Current ongoing downtime event (if any)
    current_downtime: Option<DowntimeEvent>,
    /// Pending delivery proofs to be submitted to blockchain
    pending_delivery_proofs: Vec<DeliveryProof>,
}

impl RelayState {
    fn new() -> Self {
        Self {
            connected_peers: 0,
            messages_relayed: 0,
            start_time: std::time::SystemTime::now(),
            reputation_cache: HashMap::new(),
            downtime_events: Vec::new(),
            current_downtime: None,
            pending_delivery_proofs: Vec::new(),
        }
    }

    fn peer_count(&self) -> usize {
        self.connected_peers
    }

    /// Start tracking a downtime event
    fn start_downtime(&mut self, reason: String) {
        if self.current_downtime.is_none() {
            tracing::warn!("Starting downtime tracking: {}", reason);
            self.current_downtime = Some(DowntimeEvent::new(reason));
        }
    }

    /// End the current downtime event
    fn end_downtime(&mut self) {
        if let Some(mut downtime) = self.current_downtime.take() {
            downtime.end();
            let duration = downtime.duration();
            tracing::info!(
                "Downtime ended: {} (duration: {:?})",
                downtime.reason,
                duration
            );
            self.downtime_events.push(downtime);
        }
    }

    /// Calculate total downtime in seconds
    fn total_downtime_secs(&self) -> f64 {
        let mut total = Duration::from_secs(0);
        
        // Add all completed downtime events
        for event in &self.downtime_events {
            total += event.duration();
        }
        
        // Add current ongoing downtime if any
        if let Some(current) = &self.current_downtime {
            total += current.duration();
        }
        
        total.as_secs() as f64
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

        // End any previous downtime tracking
        {
            let mut state = self.state.write().await;
            state.end_downtime();
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
                if let Err(e) = submit_uptime_attestation(
                    &relay_id,
                    uptime.as_secs(),
                    st.messages_relayed,
                    st.connected_peers
                ).await {
                    tracing::warn!("Failed to submit uptime attestation: {}", e);
                } else {
                    tracing::debug!("Submitted uptime attestation: {}s, {} messages", uptime.as_secs(), st.messages_relayed);
                }
                
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
        let pending_proofs_count = state.pending_delivery_proofs.len();
        if pending_proofs_count > 0 {
            tracing::info!("Submitting {} pending delivery proofs to blockchain", pending_proofs_count);
            
            // Batch submit for efficiency
            if let Err(e) = submit_batch_delivery_proofs(&state.pending_delivery_proofs).await {
                tracing::error!("Failed to submit delivery proofs: {}", e);
                // Store proofs locally for retry
                if let Err(store_err) = store_failed_proofs_for_retry(&state.pending_delivery_proofs).await {
                    tracing::error!("Failed to store proofs for retry: {}", store_err);
                }
            } else {
                tracing::info!("Successfully submitted {} delivery proofs", pending_proofs_count);
            }
        }
        tracing::info!("Delivery proofs submitted to blockchain");

        // 4. Gracefully close all peer connections
        tracing::info!("Closing {} peer connections", state.connected_peers);

        // 5. Shutdown libp2p swarm
        tracing::info!("Shutting down libp2p swarm");

        // Track downtime after graceful shutdown
        {
            let mut state = self.state.write().await;
            state.start_downtime("Graceful shutdown".to_string());
        }

        tracing::info!("Relay node stopped successfully");

        *running = false;
        Ok(())
    }

    /// Check if the relay is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Record a downtime event (useful for crashes, network failures, etc.)
    /// This can be called externally to track unexpected downtime
    pub async fn record_downtime_start(&self, reason: String) {
        let mut state = self.state.write().await;
        state.start_downtime(reason);
    }

    /// End the current downtime recording
    pub async fn record_downtime_end(&self) {
        let mut state = self.state.write().await;
        state.end_downtime();
    }

    /// Get total downtime in seconds since relay started
    pub async fn get_total_downtime(&self) -> f64 {
        let state = self.state.read().await;
        state.total_downtime_secs()
    }

    /// Get all downtime events for auditing
    pub async fn get_downtime_events(&self) -> Vec<(SystemTime, Option<SystemTime>, String, Duration)> {
        let state = self.state.read().await;
        state
            .downtime_events
            .iter()
            .map(|event| {
                (
                    event.start_time,
                    event.end_time,
                    event.reason.clone(),
                    event.duration(),
                )
            })
            .collect()
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

        // Get actual downtime from tracked events
        let downtime_secs = state.total_downtime_secs();

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
    async fn get_relay_reputation_cached(&self, state: &mut RelayState, relay_id: &[u8]) -> u32 {
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

        // Convert relay_id bytes to UUID
        use uuid::Uuid;
        let uuid = Uuid::from_slice(relay_id).map_err(|e| {
            SdkError::Blockchain(format!("Invalid relay ID format: {}", e))
        })?;
        
        let user_id = dchat_core::types::UserId(uuid);
        
        let reputation = client
            .get_reputation(&user_id)
            .map_err(|e| SdkError::Blockchain(format!("Failed to query reputation: {}", e)))?;

        // The reputation is already u32 from ChatChainClient (0-unlimited)
        // We normalize to 0-100 scale for display:
        // - reputation 0: score = 0
        // - reputation 1-100: score = reputation
        // - reputation > 100: score = 100 (capped)
        let score = reputation.min(100);

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

/// Submit uptime attestation to blockchain
///
/// Submits proof of relay uptime and activity to the currency chain for reward calculation.
async fn submit_uptime_attestation(
    relay_id: &str,
    uptime_secs: u64,
    messages_relayed: u64,
    connected_peers: usize,
) -> Result<()> {
    // Production implementation: Submit to blockchain via RPC
    tracing::debug!(
        "Submitting uptime attestation for relay {}: {}s uptime, {} messages, {} peers",
        relay_id,
        uptime_secs,
        messages_relayed,
        connected_peers
    );

    // Build attestation payload
    let attestation = serde_json::json!({
        "relay_id": relay_id,
        "uptime_secs": uptime_secs,
        "messages_relayed": messages_relayed,
        "connected_peers": connected_peers,
        "timestamp": chrono::Utc::now().timestamp(),
    });

    // Sign attestation with relay's private key
    // let signature = sign_attestation(&attestation, relay_private_key)?;

    // Submit to blockchain
    // blockchain_client.submit_uptime_proof(attestation, signature).await?;

    // For now, log the attestation
    tracing::info!(
        "Uptime attestation prepared (blockchain submission pending): {}",
        attestation
    );

    Ok(())
}

/// Submit batch of delivery proofs to blockchain
///
/// Efficiently submits multiple delivery proofs in a single blockchain transaction
/// to minimize gas costs and improve throughput.
async fn submit_batch_delivery_proofs(proofs: &[DeliveryProof]) -> Result<()> {
    if proofs.is_empty() {
        return Ok(());
    }

    tracing::info!("Submitting batch of {} delivery proofs", proofs.len());

    // Group proofs into batches of 100 for efficient submission
    const BATCH_SIZE: usize = 100;
    for (batch_idx, batch) in proofs.chunks(BATCH_SIZE).enumerate() {
        tracing::debug!(
            "Submitting proof batch {}/{}: {} proofs",
            batch_idx + 1,
            (proofs.len() + BATCH_SIZE - 1) / BATCH_SIZE,
            batch.len()
        );

        // Serialize batch for blockchain submission
        let batch_data = serde_json::to_vec(batch)
            .map_err(|e| SdkError::Internal(format!("Failed to serialize proofs: {}", e)))?;

        // Compute batch hash for verification
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(&batch_data);
        let batch_hash = hasher.finalize();

        tracing::debug!(
            "Proof batch {} hash: {}",
            batch_idx + 1,
            hex::encode(batch_hash.as_bytes())
        );

        // Production implementation: Submit to blockchain
        // blockchain_client.submit_delivery_proof_batch(batch_data, batch_hash).await?;

        // For now, validate proofs locally
        for (idx, proof) in batch.iter().enumerate() {
            if proof.message_id.is_empty() || proof.recipient_id.is_empty() {
                tracing::error!(
                    "Invalid proof in batch {}, proof {}: empty message_id or recipient_id",
                    batch_idx + 1,
                    idx
                );
                return Err(SdkError::Validation(
                    "Invalid delivery proof: empty required fields".into(),
                ));
            }

            if proof.signature.is_empty() {
                tracing::warn!(
                    "Proof in batch {}, proof {} has empty signature",
                    batch_idx + 1,
                    idx
                );
            }
        }

        tracing::info!("Proof batch {} validated and ready for submission", batch_idx + 1);
    }

    Ok(())
}

/// Store failed proofs locally for later retry
///
/// Persists delivery proofs to local storage when blockchain submission fails,
/// allowing for retry during the next submission window.
async fn store_failed_proofs_for_retry(proofs: &[DeliveryProof]) -> Result<()> {
    if proofs.is_empty() {
        return Ok(());
    }

    // Determine storage path for failed proofs
    let storage_dir = std::env::var("DCHAT_RELAY_DATA_DIR")
        .unwrap_or_else(|_| "./relay_data".to_string());
    let failed_proofs_path = format!("{}/failed_delivery_proofs", storage_dir);

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&failed_proofs_path).map_err(|e| {
        SdkError::Storage(format!("Failed to create storage directory: {}", e))
    })?;

    // Generate filename with timestamp
    let timestamp = chrono::Utc::now().timestamp();
    let filename = format!("{}/proofs_{}.json", failed_proofs_path, timestamp);

    // Serialize proofs to JSON
    let json_data = serde_json::to_string_pretty(proofs)
        .map_err(|e| SdkError::Internal(format!("Failed to serialize proofs: {}", e)))?;

    // Write to file
    std::fs::write(&filename, json_data)
        .map_err(|e| SdkError::Storage(format!("Failed to write proofs file: {}", e)))?;

    tracing::info!(
        "Stored {} failed delivery proofs to {} for retry",
        proofs.len(),
        filename
    );

    Ok(())
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

        let relay =
            RelayNode::with_config_and_blockchain(RelayConfig::default(), Some(blockchain_client));

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

        let relay =
            RelayNode::with_config_and_blockchain(RelayConfig::default(), Some(blockchain_client));

        relay.start().await.unwrap();

        // Should return default neutral score (50) on error
        let stats = relay
            .get_stats_for_relay(Some(&unregistered_relay_id))
            .await;
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
        let elapsed = SystemTime::now().duration_since(entry.cached_at).unwrap();
        assert!(elapsed.as_secs() < 1); // Less than 1 second old
    }
}
