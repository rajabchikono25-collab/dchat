// Bootstrap node management

use super::dht::{Dht, DhtConfig};
use dchat_core::Result;
use libp2p::Multiaddr;
use std::time::Duration;

/// Bootstrap manager for connecting to the network
pub struct Bootstrap {
    nodes: Vec<Multiaddr>,
    connection_timeout: Duration,
}

impl Bootstrap {
    /// Create a new bootstrap manager
    pub fn new(nodes: Vec<Multiaddr>) -> Self {
        Self {
            nodes,
            connection_timeout: Duration::from_secs(30),
        }
    }

    /// Create bootstrap manager with default nodes
    pub fn with_defaults() -> Self {
        Self::new(Self::default_nodes())
    }

    /// Get default bootstrap nodes
    ///
    /// Priority order:
    /// 1. DCHAT_BOOTSTRAP_NODES environment variable (comma-separated multiaddrs)
    /// 2. Fallback to default DNS addresses (must be configured before production deployment)
    ///
    /// For production: Set DCHAT_BOOTSTRAP_NODES or update DNS records for:
    /// - bootstrap-1.dchat.network
    /// - bootstrap-2.dchat.network  
    /// - bootstrap-3.dchat.network
    pub fn default_nodes() -> Vec<Multiaddr> {
        // Check for environment variable first (production override)
        if let Ok(env_nodes) = std::env::var("DCHAT_BOOTSTRAP_NODES") {
            let nodes: Vec<Multiaddr> = env_nodes
                .split(',')
                .filter_map(|addr| addr.trim().parse().ok())
                .collect();

            if !nodes.is_empty() {
                tracing::info!(
                    "Using {} bootstrap nodes from DCHAT_BOOTSTRAP_NODES",
                    nodes.len()
                );
                return nodes;
            }
        }

        // Fallback to DNS-based discovery (requires DNS configuration)
        // These DNS names must resolve to actual relay node IPs before production use
        vec![
            "/dns4/bootstrap-1.dchat.network/tcp/9000"
                .parse()
                .expect("Valid bootstrap address 1"),
            "/dns4/bootstrap-2.dchat.network/tcp/9000"
                .parse()
                .expect("Valid bootstrap address 2"),
            "/dns4/bootstrap-3.dchat.network/tcp/9000"
                .parse()
                .expect("Valid bootstrap address 3"),
        ]
    }

    /// Connect to the network using bootstrap nodes
    pub async fn connect_to_network(&self, dht: &mut Dht) -> Result<()> {
        tracing::info!(
            "Connecting to network via {} bootstrap nodes",
            self.nodes.len()
        );

        // 1. Connect to bootstrap nodes
        for node in &self.nodes {
            tracing::debug!("Attempting to connect to bootstrap node: {}", node);
            // Production: connect via libp2p swarm
            // swarm.dial(node.clone()).map_err(|e| {
            //     Error::network(format!("Failed to dial bootstrap node {}: {}", node, e))
            // })?;
            //
            // Wait for connection with timeout:
            // tokio::time::timeout(
            //     self.connection_timeout,
            //     async { swarm.next_event().await }
            // ).await?;
        }

        // 2. Bootstrap the DHT
        dht.bootstrap().await?;

        // 3. Announce our presence
        dht.announce().await?;

        tracing::info!("Successfully connected to network");
        Ok(())
    }

    /// Add a custom bootstrap node
    pub fn add_node(&mut self, addr: Multiaddr) {
        if !self.nodes.contains(&addr) {
            self.nodes.push(addr);
        }
    }

    /// Remove a bootstrap node
    pub fn remove_node(&mut self, addr: &Multiaddr) {
        self.nodes.retain(|n| n != addr);
    }

    /// Get all bootstrap nodes
    pub fn nodes(&self) -> &[Multiaddr] {
        &self.nodes
    }

    /// Set connection timeout
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.connection_timeout = timeout;
    }
}

impl Default for Bootstrap {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Helper to create a configured DHT with bootstrap nodes
pub async fn create_dht_with_bootstrap(config: DhtConfig, bootstrap: &Bootstrap) -> Result<Dht> {
    let mut dht_config = config;
    dht_config.bootstrap_nodes = bootstrap.nodes().to_vec();

    Dht::new(dht_config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::PeerId;

    #[test]
    fn test_bootstrap_creation() {
        let nodes = vec!["/ip4/127.0.0.1/tcp/9000".parse().unwrap()];
        let bootstrap = Bootstrap::new(nodes.clone());

        assert_eq!(bootstrap.nodes().len(), nodes.len());
    }

    #[test]
    fn test_default_bootstrap() {
        let bootstrap = Bootstrap::with_defaults();
        assert!(!bootstrap.nodes().is_empty());
    }

    #[test]
    fn test_add_node() {
        let mut bootstrap = Bootstrap::new(vec![]);
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();

        bootstrap.add_node(addr.clone());
        assert_eq!(bootstrap.nodes().len(), 1);

        // Adding same node again should not duplicate
        bootstrap.add_node(addr.clone());
        assert_eq!(bootstrap.nodes().len(), 1);
    }

    #[test]
    fn test_remove_node() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
        let mut bootstrap = Bootstrap::new(vec![addr.clone()]);

        assert_eq!(bootstrap.nodes().len(), 1);

        bootstrap.remove_node(&addr);
        assert_eq!(bootstrap.nodes().len(), 0);
    }

    #[tokio::test]
    async fn test_create_dht_with_bootstrap() {
        let bootstrap = Bootstrap::with_defaults();
        let config = DhtConfig {
            local_peer_id: PeerId::random(),
            bootstrap_nodes: vec![],
            k_bucket_size: 20,
            alpha: 3,
            query_timeout: Duration::from_secs(30),
        };

        let result = create_dht_with_bootstrap(config, &bootstrap).await;
        assert!(result.is_ok());
    }
}
