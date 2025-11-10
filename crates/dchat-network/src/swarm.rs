//! Network swarm management

use crate::{
    behavior::{DchatBehavior, DchatMessage},
    discovery::{Discovery, DiscoveryConfig},
    nat::{NatConfig, NatTraversal},
    routing::Router,
    transport::build_transport,
};
use dchat_core::error::{Error, Result};
use futures::StreamExt;
use libp2p::{
    gossipsub, identify, kad, mdns,
    swarm::{Swarm, SwarmEvent},
    Multiaddr, PeerId,
};

/// Network manager configuration
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Listen addresses
    pub listen_addrs: Vec<Multiaddr>,

    /// Discovery configuration
    pub discovery: DiscoveryConfig,

    /// NAT traversal configuration
    pub nat: NatConfig,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addrs: vec![
                "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
                "/ip6/::/tcp/0".parse().unwrap(),
            ],
            discovery: DiscoveryConfig::default(),
            nat: NatConfig::default(),
        }
    }
}

/// Network events
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// New peer discovered
    PeerDiscovered(PeerId),

    /// Peer connected
    PeerConnected(PeerId),

    /// Peer disconnected
    PeerDisconnected(PeerId),

    /// Message received
    MessageReceived { from: PeerId, message: DchatMessage },

    /// DHT query completed
    DhtQueryComplete,
}

/// Network manager
#[allow(dead_code)]
pub struct NetworkManager {
    swarm: Swarm<DchatBehavior>,
    config: NetworkConfig,
    discovery: Discovery,
    nat: NatTraversal,
    router: Router,
}

impl NetworkManager {
    /// Create a new network manager
    pub async fn new(config: NetworkConfig) -> Result<Self> {
        // Generate local keypair
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = local_key.public().to_peer_id();

        tracing::info!("Local peer ID: {}", local_peer_id);

        // Build transport
        let transport = build_transport(&local_key)?;

        // Create behavior
        let behavior = DchatBehavior::new(local_peer_id, &local_key)
            .map_err(|e| Error::network(format!("Failed to create behavior: {}", e)))?;

        // Build swarm using new API
        let swarm_config = libp2p::swarm::Config::with_tokio_executor();
        let swarm = Swarm::new(transport, behavior, local_peer_id, swarm_config);

        let discovery = Discovery::new(config.discovery.clone()).await?;
        let nat = NatTraversal::new(config.nat.clone()).await?;
        let router = Router::new();

        Ok(Self {
            swarm,
            config,
            discovery,
            nat,
            router,
        })
    }

    /// Start the network manager
    pub async fn start(&mut self) -> Result<()> {
        // Listen on configured addresses
        for addr in &self.config.listen_addrs {
            self.swarm
                .listen_on(addr.clone())
                .map_err(|e| Error::network(format!("Failed to listen: {}", e)))?;
        }

        // Bootstrap DHT with known peers
        let bootstrap_nodes = self.discovery.bootstrap_nodes();
        if !bootstrap_nodes.is_empty() {
            tracing::info!(
                "📡 Configuring {} bootstrap peers from config",
                bootstrap_nodes.len()
            );

            for (peer_id, addr) in bootstrap_nodes {
                tracing::info!("  → Bootstrap peer: {} at {}", peer_id, addr);
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(peer_id, addr.clone());

                // Actively dial each bootstrap peer
                match self.swarm.dial(addr.clone()) {
                    Ok(_) => tracing::debug!("Dialing bootstrap peer: {}", peer_id),
                    Err(e) => tracing::warn!("Failed to dial bootstrap peer {}: {}", peer_id, e),
                }
            }

            // Bootstrap the DHT with configured bootstrap nodes
            self.swarm
                .behaviour_mut()
                .kademlia
                .bootstrap()
                .map_err(|e| Error::network(format!("DHT bootstrap failed: {}", e)))?;
            tracing::info!(
                "✓ DHT bootstrapped with {} bootstrap nodes",
                bootstrap_nodes.len()
            );
        } else {
            // No bootstrap nodes configured - will rely on mDNS for local discovery
            // and wait for other peers to connect
            tracing::info!(
                "No bootstrap nodes configured - will use mDNS for local peer discovery"
            );
        }

        tracing::info!(
            "Network started, listening on {} addresses",
            self.config.listen_addrs.len()
        );

        Ok(())
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Get local peer ID (alias for compatibility)
    pub fn peer_id(&self) -> PeerId {
        self.local_peer_id()
    }

    /// Get listen addresses
    pub fn listeners(&self) -> Vec<Multiaddr> {
        self.swarm.listeners().cloned().collect()
    }

    /// Dial a peer
    pub fn dial(&mut self, addr: Multiaddr) -> Result<()> {
        self.swarm
            .dial(addr)
            .map_err(|e| Error::network(format!("Failed to dial: {}", e)))
    }

    /// Subscribe to a channel
    pub fn subscribe_channel(&mut self, channel_id: &str) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .subscribe_channel(channel_id)
            .map_err(|e| Error::network(format!("Subscribe failed: {}", e)))?;
        Ok(())
    }

    /// Subscribe to a channel topic
    pub fn subscribe_to_channel(&mut self, channel_id: &str) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .subscribe_channel(channel_id)
            .map_err(|e| Error::network(format!("Subscribe failed: {}", e)))?;
        tracing::info!("📢 Subscribed to channel: {}", channel_id);
        Ok(())
    }

    /// Publish message to channel
    pub fn publish_to_channel(&mut self, channel_id: &str, message: &DchatMessage) -> Result<()> {
        self.swarm
            .behaviour_mut()
            .publish_to_channel(channel_id, message)
            .map_err(|e| Error::network(format!("Publish failed: {}", e)))?;
        Ok(())
    }

    /// Get gossipsub mesh peer count for debugging
    pub fn get_mesh_peer_count(&mut self, channel_id: &str) -> usize {
        let topic_hash = gossipsub::IdentTopic::new(format!("dchat/channel/{}", channel_id)).hash();
        self.swarm
            .behaviour_mut()
            .gossipsub
            .mesh_peers(&topic_hash)
            .count()
    }

    /// Get all mesh peers for a channel
    pub fn get_mesh_peers(&mut self, channel_id: &str) -> Vec<PeerId> {
        let topic_hash = gossipsub::IdentTopic::new(format!("dchat/channel/{}", channel_id)).hash();
        self.swarm
            .behaviour_mut()
            .gossipsub
            .mesh_peers(&topic_hash)
            .copied()
            .collect()
    }

    /// Process network events
    pub async fn next_event(&mut self) -> Option<NetworkEvent> {
        loop {
            let event = self.swarm.select_next_some().await;

            match event {
                SwarmEvent::Behaviour(event) => {
                    if let Some(net_event) = self.handle_behavior_event(event) {
                        return Some(net_event);
                    }
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    tracing::info!("🔗 Connection established with peer: {}", peer_id);
                    self.discovery.peer_connected(peer_id);
                    return Some(NetworkEvent::PeerConnected(peer_id));
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    tracing::info!("🔌 Connection closed with peer: {}", peer_id);
                    self.discovery.peer_disconnected(&peer_id);
                    return Some(NetworkEvent::PeerDisconnected(peer_id));
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!("Listening on: {}", address);
                }
                _ => {}
            }
        }
    }

    fn handle_behavior_event(
        &mut self,
        event: crate::behavior::DchatBehaviorEvent,
    ) -> Option<NetworkEvent> {
        use crate::behavior::DchatBehaviorEvent;

        match event {
            DchatBehaviorEvent::Mdns(mdns::Event::Discovered(peers)) => {
                for (peer_id, addr) in peers {
                    tracing::info!("✨ mDNS discovered peer: {} at {}", peer_id, addr);
                    // Register peer with default address
                    let peer_info = crate::discovery::PeerInfo::new(peer_id, vec![]);
                    let _ = self.discovery.register_peer(peer_info);
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr);
                }
                None
            }
            DchatBehaviorEvent::Mdns(mdns::Event::Expired(peers)) => {
                for (peer_id, _) in peers {
                    tracing::debug!("mDNS peer expired: {}", peer_id);
                }
                None
            }
            DchatBehaviorEvent::Gossipsub(gossipsub::Event::Message { message, .. }) => {
                if let Ok(dchat_msg) = bincode::deserialize::<DchatMessage>(&message.data) {
                    Some(NetworkEvent::MessageReceived {
                        from: message.source.unwrap_or(PeerId::random()),
                        message: dchat_msg,
                    })
                } else {
                    None
                }
            }
            DchatBehaviorEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic }) => {
                tracing::info!("🔔 Peer {} subscribed to topic: {}", peer_id, topic);
                None
            }
            DchatBehaviorEvent::Gossipsub(gossipsub::Event::Unsubscribed { peer_id, topic }) => {
                tracing::info!("🔕 Peer {} unsubscribed from topic: {}", peer_id, topic);
                None
            }
            DchatBehaviorEvent::Gossipsub(gossipsub::Event::GossipsubNotSupported { peer_id }) => {
                tracing::warn!("⚠️  Peer {} does not support gossipsub", peer_id);
                None
            }
            DchatBehaviorEvent::Identify(identify::Event::Received {
                peer_id,
                info,
                connection_id: _,
            }) => {
                tracing::info!(
                    "Identified peer: {} with {} addresses",
                    peer_id,
                    info.listen_addrs.len()
                );
                for addr in info.listen_addrs {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr);
                }
                None
            }
            DchatBehaviorEvent::Kademlia(kad::Event::OutboundQueryProgressed {
                result: kad::QueryResult::Bootstrap(Ok(_)),
                ..
            }) => {
                tracing::info!("DHT bootstrap successful");
                Some(NetworkEvent::DhtQueryComplete)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_manager_creation() {
        let config = NetworkConfig::default();
        let manager = NetworkManager::new(config).await;
        assert!(manager.is_ok());
    }
}
