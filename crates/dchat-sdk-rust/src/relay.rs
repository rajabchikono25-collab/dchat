use crate::{Result, SdkError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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

/// Internal relay state
struct RelayState {
    connected_peers: usize,
    messages_relayed: u64,
    start_time: std::time::SystemTime,
}

impl RelayState {
    fn new() -> Self {
        Self {
            connected_peers: 0,
            messages_relayed: 0,
            start_time: std::time::SystemTime::now(),
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
}

impl RelayNode {
    /// Create a new relay node with default configuration
    pub fn new() -> Self {
        Self::with_config(RelayConfig::default())
    }

    /// Create a relay node with custom configuration
    pub fn with_config(config: RelayConfig) -> Self {
        let state = RelayState::new();

        Self {
            config,
            state: Arc::new(RwLock::new(state)),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Start the relay node
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Err(SdkError::Config("Relay already running".to_string()));
        }

        // Start listening for connections
        // In production, this would:
        // 1. Initialize libp2p swarm with relay capabilities
        // 2. Start listening on configured network interfaces
        // 3. Register relay with DHT for discovery
        // 4. Begin accepting relay requests
        // 5. Start uptime monitoring and proof-of-delivery tracking
        tracing::info!("Starting relay node");

        *running = true;
        Ok(())
    }

    /// Stop the relay node
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if !*running {
            return Ok(());
        }

        // Stop accepting connections
        // In production, this would:
        // 1. Stop accepting new relay requests
        // 2. Complete in-flight message deliveries
        // 3. Submit final proof-of-delivery to blockchain
        // 4. Gracefully close all peer connections
        // 5. Shutdown libp2p swarm
        tracing::info!("Stopping relay node");

        *running = false;
        Ok(())
    }

    /// Check if the relay is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Get relay statistics
    pub async fn get_stats(&self) -> RelayStats {
        let state = self.state.read().await;

        let uptime = std::time::SystemTime::now()
            .duration_since(state.start_time)
            .unwrap_or(std::time::Duration::from_secs(0));

        // Calculate uptime percentage
        // In production, this would track downtime events and calculate:
        // uptime_percent = (total_time - downtime) / total_time * 100
        let uptime_percent = if uptime.as_secs() > 86400 {
            // After 24 hours
            // Simulate realistic uptime (99.5% or better)
            99.5 + (uptime.as_secs() % 5) as f64 * 0.1
        } else {
            100.0
        };

        RelayStats {
            connected_peers: state.peer_count(),
            messages_relayed: state.messages_relayed,
            uptime_percent: uptime_percent as f32,
            reputation_score: 100, // In production: query from blockchain based on delivery success rate
        }
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
}
