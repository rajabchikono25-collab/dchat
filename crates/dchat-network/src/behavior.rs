//! Network behavior combining multiple libp2p protocols

use dchat_core::types::UserId;
use libp2p::{
    gossipsub::{self, MessageId},
    identify, kad, mdns, ping,
    swarm::NetworkBehaviour,
    PeerId,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

/// Message types for the dchat protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DchatMessage {
    /// Direct encrypted message
    DirectMessage {
        sender: UserId,
        recipient: UserId,
        encrypted_payload: Vec<u8>,
    },
    /// Channel message
    ChannelMessage {
        sender: UserId,
        channel_id: String,
        encrypted_payload: Vec<u8>,
    },
    /// Relay proof-of-delivery
    DeliveryProof {
        message_id: String,
        relay_signature: Vec<u8>,
    },
    /// Sync request for offline messages
    SyncRequest { user_id: UserId, last_sequence: u64 },
    /// Validator block proposal for consensus
    ValidatorBlock {
        height: u64,
        validator_id: Vec<u8>,
        block_hash: Vec<u8>,
        signature: Vec<u8>,
        timestamp: u64,
        transactions: Vec<Vec<u8>>,
    },
    /// Validator block acknowledgment (vote)
    BlockAcknowledgment {
        block_height: u64,
        block_hash: Vec<u8>,
        validator_id: Vec<u8>,
        signature: Vec<u8>,
    },
}

/// Combined network behavior for dchat
#[derive(NetworkBehaviour)]
pub struct DchatBehavior {
    /// Kademlia DHT for peer discovery and routing
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,

    /// mDNS for local network discovery
    pub mdns: mdns::tokio::Behaviour,

    /// Gossipsub for message propagation
    pub gossipsub: gossipsub::Behaviour,

    /// Identify protocol for peer information
    pub identify: identify::Behaviour,

    /// Ping for connection liveness
    pub ping: ping::Behaviour,
}

impl DchatBehavior {
    /// Create a new dchat network behavior
    pub fn new(
        local_peer_id: PeerId,
        local_key: &libp2p::identity::Keypair,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Kademlia configuration
        let store = kad::store::MemoryStore::new(local_peer_id);
        let kad_protocol = libp2p::StreamProtocol::new("/dchat/kad/1.0.0");
        let mut kad_config = kad::Config::new(kad_protocol);
        kad_config.set_query_timeout(Duration::from_secs(60));
        let kademlia = kad::Behaviour::with_config(local_peer_id, store, kad_config);

        // mDNS for local discovery
        let mdns_config = mdns::Config::default();
        let mdns = mdns::tokio::Behaviour::new(mdns_config, local_peer_id)?;

        // Gossipsub configuration - optimized for 23-user network
        // Fixed: mesh_n_low must be >= 1 to prevent underflow panic at behaviour.rs:2135
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(1))
            .validation_mode(gossipsub::ValidationMode::Permissive) // Less strict for testing
            .message_id_fn(message_id_fn)
            .mesh_outbound_min(2) // Minimum outbound connections
            .mesh_n_low(3) // Min mesh peers (MUST be >= 1 to prevent panic)
            .mesh_n(6) // Target mesh peers (optimal for redundancy)
            .mesh_n_high(12) // Max mesh peers
            .flood_publish(true) // Send to ALL connected peers (not just mesh)
            .do_px() // Enable peer exchange
            .build()
            .map_err(|e| format!("Gossipsub config error: {}", e))?;

        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;

        // Identify protocol
        let identify = identify::Behaviour::new(
            identify::Config::new("/dchat/1.0.0".to_string(), local_key.public())
                .with_push_listen_addr_updates(true),
        );

        // Ping protocol
        let ping = ping::Behaviour::new(ping::Config::new());

        Ok(Self {
            kademlia,
            mdns,
            gossipsub,
            identify,
            ping,
        })
    }

    /// Subscribe to a channel topic
    pub fn subscribe_channel(
        &mut self,
        channel_id: &str,
    ) -> Result<bool, gossipsub::SubscriptionError> {
        let topic = gossipsub::IdentTopic::new(format!("dchat/channel/{}", channel_id));
        self.gossipsub.subscribe(&topic)
    }

    /// Unsubscribe from a channel topic
    pub fn unsubscribe_channel(
        &mut self,
        channel_id: &str,
    ) -> Result<bool, gossipsub::PublishError> {
        let topic = gossipsub::IdentTopic::new(format!("dchat/channel/{}", channel_id));
        self.gossipsub.unsubscribe(&topic)
    }

    /// Publish a message to a channel
    pub fn publish_to_channel(
        &mut self,
        channel_id: &str,
        message: &DchatMessage,
    ) -> Result<MessageId, gossipsub::PublishError> {
        let topic = gossipsub::IdentTopic::new(format!("dchat/channel/{}", channel_id));
        let data = bincode::serialize(message).map_err(|e| {
            gossipsub::PublishError::TransformFailed(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Serialization failed: {}", e),
            ))
        })?;
        self.gossipsub.publish(topic, data)
    }

    /// Subscribe to validator consensus topic
    pub fn subscribe_validators(&mut self) -> Result<bool, gossipsub::SubscriptionError> {
        let topic = gossipsub::IdentTopic::new("dchat/validators/consensus");
        self.gossipsub.subscribe(&topic)
    }

    /// Publish validator block to consensus network
    pub fn broadcast_validator_block(
        &mut self,
        message: &DchatMessage,
    ) -> Result<MessageId, gossipsub::PublishError> {
        let topic = gossipsub::IdentTopic::new("dchat/validators/consensus");
        let data = bincode::serialize(message).map_err(|e| {
            gossipsub::PublishError::TransformFailed(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Serialization failed: {}", e),
            ))
        })?;
        self.gossipsub.publish(topic, data)
    }
}

/// Custom message ID function for gossipsub
fn message_id_fn(message: &gossipsub::Message) -> MessageId {
    let mut hasher = DefaultHasher::new();
    message.data.hash(&mut hasher);
    MessageId::from(hasher.finish().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity::Keypair;

    #[test]
    fn test_behavior_creation() {
        let keypair = Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();
        let behavior = DchatBehavior::new(peer_id, &keypair);
        assert!(behavior.is_ok());
    }

    #[test]
    fn test_channel_subscription() {
        let keypair = Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();
        let mut behavior = DchatBehavior::new(peer_id, &keypair).unwrap();

        let result = behavior.subscribe_channel("test-channel");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }
}
