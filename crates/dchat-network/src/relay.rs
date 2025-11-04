//! Relay node infrastructure for incentivized message delivery

use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::swarm::NetworkManager;

/// Relay node configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    /// Enable relay functionality
    pub enabled: bool,

    /// Maximum concurrent relayed connections
    pub max_connections: usize,

    /// Bandwidth limit in bytes per second
    pub bandwidth_limit: u64,

    /// Minimum stake required for relay operations
    pub min_stake: u64,

    /// Reward rate per message relayed
    pub reward_per_message: u64,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Relay enabled by default when running as relay node
            max_connections: 100,
            bandwidth_limit: 10_000_000, // 10 MB/s
            min_stake: 1000,
            reward_per_message: 1,
        }
    }
}

/// Relay node operator
pub struct RelayNode {
    config: RelayConfig,
    peer_id: PeerId,

    /// Network manager for P2P communication
    network: NetworkManager,

    /// Track relayed messages for proof-of-delivery
    relayed_messages: HashMap<String, RelayProof>,

    /// Uptime tracking
    start_time: SystemTime,

    /// Total messages relayed
    total_messages: u64,

    /// Total bandwidth used
    total_bandwidth: u64,
}

impl RelayNode {
    /// Create a new relay node
    pub fn new(config: RelayConfig, peer_id: PeerId, network: NetworkManager) -> Self {
        Self {
            config,
            peer_id,
            network,
            relayed_messages: HashMap::new(),
            start_time: SystemTime::now(),
            total_messages: 0,
            total_bandwidth: 0,
        }
    }

    /// Check if relay is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Record a relayed message
    pub fn record_relay(
        &mut self,
        message_id: String,
        sender: UserId,
        recipient: UserId,
        size: usize,
    ) -> Result<RelayProof> {
        if !self.config.enabled {
            return Err(Error::network("Relay not enabled".to_string()));
        }

        // Check bandwidth limit
        if self.total_bandwidth + size as u64 > self.config.bandwidth_limit {
            return Err(Error::network("Bandwidth limit exceeded".to_string()));
        }

        let proof = RelayProof {
            message_id: message_id.clone(),
            relay_peer_id: self.peer_id.to_string(),
            sender,
            recipient,
            timestamp: SystemTime::now(),
            size,
        };

        self.relayed_messages.insert(message_id, proof.clone());
        self.total_messages += 1;
        self.total_bandwidth += size as u64;

        tracing::debug!(
            "Relayed message {}, total: {}",
            proof.message_id,
            self.total_messages
        );

        Ok(proof)
    }

    /// Get relay statistics
    pub fn stats(&self) -> RelayStats {
        let uptime = SystemTime::now()
            .duration_since(self.start_time)
            .unwrap_or(Duration::from_secs(0));

        RelayStats {
            peer_id: self.peer_id,
            total_messages: self.total_messages,
            total_bandwidth: self.total_bandwidth,
            uptime,
            active_connections: self.relayed_messages.len(),
        }
    }

    /// Calculate earned rewards
    pub fn calculate_rewards(&self) -> u64 {
        self.total_messages * self.config.reward_per_message
    }

    /// Run the relay node event loop
    /// This is a long-running async task that handles relay operations
    pub async fn run(&mut self) -> Result<()> {
        if !self.config.enabled {
            return Err(Error::network("Relay is not enabled".to_string()));
        }

        tracing::info!("🔀 Relay node starting (peer_id: {})", self.peer_id);
        tracing::info!("   Max connections: {}", self.config.max_connections);
        tracing::info!("   Bandwidth limit: {} bytes", self.config.bandwidth_limit);

        // Subscribe to global channel to participate in gossipsub mesh
        self.network.subscribe_to_channel("global").ok();
        tracing::info!("📡 Subscribed to #global channel for gossipsub mesh participation");

        // Stats reporting interval
        let mut stats_interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        let mut test_publish_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        let mut message_counter: u64 = 0;

        // Main event loop: poll network events
        loop {
            tokio::select! {
                // Poll network events (peer connections, messages, etc.)
                event = self.network.next_event() => {
                    if let Some(net_event) = event {
                        self.handle_network_event(net_event).await?;
                    }
                }

                // Publish test messages periodically
                _ = test_publish_interval.tick() => {
                    message_counter += 1;
                    if let Err(e) = self.publish_test_message(message_counter).await {
                        tracing::warn!("Failed to publish test message: {}", e);
                    }
                }

                // Report stats periodically
                _ = stats_interval.tick() => {
                    let stats = self.stats();
                    tracing::info!(
                        "📊 Relay stats: {} messages relayed, {} bytes transferred, {}s uptime",
                        stats.total_messages,
                        stats.total_bandwidth,
                        stats.uptime.as_secs()
                    );
                }
            }
        }
    }

    /// Publish a test message to verify gossipsub propagation
    async fn publish_test_message(&mut self, counter: u64) -> Result<()> {
        use crate::behavior::DchatMessage;
        use dchat_core::types::UserId;

        // Create a test user ID (in production this would be a real user)
        let sender = UserId::new();

        let test_message = DchatMessage::ChannelMessage {
            sender,
            channel_id: "test-mesh".to_string(),
            encrypted_payload: format!("Test message #{} from relay {}", counter, self.peer_id)
                .into_bytes(),
        };

        self.network
            .publish_to_channel("test-mesh", &test_message)?;
        tracing::info!(
            "📤 Published test message #{} to test-mesh channel",
            counter
        );
        Ok(())
    }

    /// Handle network events
    async fn handle_network_event(&mut self, event: crate::swarm::NetworkEvent) -> Result<()> {
        use crate::behavior::DchatMessage;
        use crate::swarm::NetworkEvent;

        match event {
            NetworkEvent::PeerConnected(peer_id) => {
                tracing::info!("✅ Relay connected to peer: {}", peer_id);
                self.total_messages += 1; // Count connection as activity
                Ok(())
            }
            NetworkEvent::PeerDisconnected(peer_id) => {
                tracing::info!("❌ Relay disconnected from peer: {}", peer_id);
                Ok(())
            }
            NetworkEvent::MessageReceived { from, message } => {
                // Process and relay the message
                match &message {
                    DchatMessage::ChannelMessage {
                        sender: _,
                        channel_id,
                        encrypted_payload,
                    } => {
                        let payload_str = String::from_utf8_lossy(encrypted_payload);
                        tracing::info!(
                            "📨 Relay received channel message from {} in channel '{}': {}",
                            from,
                            channel_id,
                            payload_str
                        );

                        // Track message size for bandwidth monitoring
                        let message_size = encrypted_payload.len();
                        self.total_bandwidth += message_size as u64;
                        self.total_messages += 1;

                        // Generate proof of relay for this message
                        let _message_id = format!(
                            "msg_{}_{}",
                            from,
                            SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap()
                                .as_millis()
                        );

                        // In a full implementation, we would:
                        // 1. Verify the sender's signature
                        // 2. Forward to recipient if they're connected
                        // 3. Submit proof to chain for rewards

                        tracing::debug!(
                            "🔄 Message relayed: {} bytes, total bandwidth: {} bytes",
                            message_size,
                            self.total_bandwidth
                        );
                    }
                    DchatMessage::DirectMessage {
                        sender,
                        recipient,
                        encrypted_payload,
                    } => {
                        tracing::info!(
                            "📬 Relay received direct message from {} to {} ({} bytes)",
                            sender,
                            recipient,
                            encrypted_payload.len()
                        );

                        let message_size = encrypted_payload.len();
                        self.total_bandwidth += message_size as u64;
                        self.total_messages += 1;

                        // Generate relay proof
                        let message_id = format!(
                            "dm_{}_{}",
                            from,
                            SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap()
                                .as_millis()
                        );
                        let proof = self.record_relay(
                            message_id,
                            sender.clone(),
                            recipient.clone(),
                            message_size,
                        )?;

                        tracing::debug!(
                            "✅ Generated relay proof for direct message: {:?}",
                            proof.message_id
                        );
                    }
                    DchatMessage::DeliveryProof {
                        message_id,
                        relay_signature: _,
                    } => {
                        tracing::info!("📋 Received delivery proof for message: {}", message_id);
                    }
                    DchatMessage::SyncRequest {
                        user_id,
                        last_sequence,
                    } => {
                        tracing::info!(
                            "🔄 Sync request from {} (last_seq: {})",
                            user_id,
                            last_sequence
                        );
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Proof of message delivery by relay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayProof {
    pub message_id: String,
    pub relay_peer_id: String, // PeerId serialized to string
    pub sender: UserId,
    pub recipient: UserId,
    pub timestamp: SystemTime,
    pub size: usize,
}

/// Relay node statistics
#[derive(Debug, Clone)]
pub struct RelayStats {
    pub peer_id: PeerId,
    pub total_messages: u64,
    pub total_bandwidth: u64,
    pub uptime: Duration,
    pub active_connections: usize,
}

/// Relay client for using relay services
pub struct RelayClient {
    /// Connected relay nodes
    available_relays: HashMap<PeerId, RelayNodeInfo>,
}

impl Default for RelayClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayClient {
    pub fn new() -> Self {
        Self {
            available_relays: HashMap::new(),
        }
    }

    /// Register an available relay node
    pub fn register_relay(&mut self, peer_id: PeerId, info: RelayNodeInfo) {
        self.available_relays.insert(peer_id, info);
    }

    /// Remove a relay node
    pub fn remove_relay(&mut self, peer_id: &PeerId) {
        self.available_relays.remove(peer_id);
    }

    /// Select best relay for a connection
    pub fn select_relay(&self) -> Option<(PeerId, &RelayNodeInfo)> {
        // Select relay with lowest latency and highest reputation
        self.available_relays
            .iter()
            .max_by_key(|(_, info)| {
                let latency_score = 1000 - info.avg_latency_ms.min(1000);
                let reputation_score = info.reputation * 10;
                latency_score + reputation_score as u64
            })
            .map(|(peer_id, info)| (*peer_id, info))
    }

    /// Get number of available relays
    pub fn available_count(&self) -> usize {
        self.available_relays.len()
    }
}

/// Information about a relay node
#[derive(Debug, Clone)]
pub struct RelayNodeInfo {
    pub peer_id: PeerId,
    pub avg_latency_ms: u64,
    pub reputation: u32,
    pub available_bandwidth: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_relay_node() {
        let config = RelayConfig {
            enabled: true,
            ..Default::default()
        };
        let peer_id = PeerId::random();
        let network = NetworkManager::new(crate::NetworkConfig::default())
            .await
            .unwrap();
        let mut relay = RelayNode::new(config, peer_id, network);

        assert!(relay.is_enabled());

        let sender = UserId(Uuid::new_v4());
        let recipient = UserId(Uuid::new_v4());
        let result = relay.record_relay("msg1".to_string(), sender, recipient, 1024);

        assert!(result.is_ok());
        assert_eq!(relay.total_messages, 1);
        assert_eq!(relay.calculate_rewards(), 1);
    }

    #[test]
    fn test_relay_client() {
        let mut client = RelayClient::new();

        let peer_id = PeerId::random();
        let info = RelayNodeInfo {
            peer_id,
            avg_latency_ms: 50,
            reputation: 100,
            available_bandwidth: 1_000_000,
        };

        client.register_relay(peer_id, info);
        assert_eq!(client.available_count(), 1);

        let selected = client.select_relay();
        assert!(selected.is_some());
    }
}
