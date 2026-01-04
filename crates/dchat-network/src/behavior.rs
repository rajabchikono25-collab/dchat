//! Network behavior combining multiple libp2p protocols
//!
//! Includes full Relay v2 support with DCUtR for NAT traversal

use dchat_core::types::UserId;
use libp2p::{
    dcutr,
    gossipsub::{self, MessageId},
    identify, kad, mdns, ping, relay,
    request_response::{self, OutboundRequestId, ProtocolSupport},
    swarm::behaviour::toggle::Toggle,
    swarm::NetworkBehaviour,
    PeerId, StreamProtocol,
};
use libp2p_request_response::cbor;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use thiserror::Error;

/// Compute a stable channel message ID.
///
/// The ID is $\mathrm{SHA256}(sender || channel\_id || encrypted\_payload || timestamp\_le)$.
pub fn compute_channel_message_id(
    sender: &UserId,
    channel_id: &str,
    encrypted_payload: &[u8],
    timestamp: i64,
) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(sender.0.as_bytes());
    hasher.update(channel_id.as_bytes());
    hasher.update(encrypted_payload);
    hasher.update(timestamp.to_le_bytes());
    hasher.finalize().into()
}

const DCHAT_WIRE_MAGIC: [u8; 4] = *b"DCHT";
const DCHAT_WIRE_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireEnvelope {
    magic: [u8; 4],
    version: u16,
    message: DchatMessage,
}

#[derive(Serialize)]
struct WireEnvelopeRef<'a> {
    magic: [u8; 4],
    version: u16,
    message: &'a DchatMessage,
}

#[derive(Debug, Error)]
pub enum WireDecodeError {
    #[error("unsupported wire version: {version}")]
    UnsupportedVersion { version: u16 },

    #[error("invalid wire magic")]
    InvalidMagic,

    #[error("decode failed")]
    DecodeFailed,
}

pub fn encode_wire_message(message: &DchatMessage) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(&WireEnvelopeRef {
        magic: DCHAT_WIRE_MAGIC,
        version: DCHAT_WIRE_VERSION,
        message,
    })
}

pub fn decode_wire_message(data: &[u8]) -> Result<DchatMessage, WireDecodeError> {
    if let Ok(env) = bincode::deserialize::<WireEnvelope>(data) {
        if env.magic != DCHAT_WIRE_MAGIC {
            return Err(WireDecodeError::InvalidMagic);
        }
        if env.version != DCHAT_WIRE_VERSION {
            return Err(WireDecodeError::UnsupportedVersion {
                version: env.version,
            });
        }
        return Ok(env.message);
    }

    // Backward compatibility: accept legacy payloads that are a raw DchatMessage.
    if let Ok(msg) = bincode::deserialize::<DchatMessage>(data) {
        return Ok(msg);
    }

    Err(WireDecodeError::DecodeFailed)
}

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
        /// Stable content identifier (hash over sender/channel/payload/timestamp)
        message_id: [u8; 32],
        sender: UserId,
        channel_id: String,
        encrypted_payload: Vec<u8>,
        /// Sender-side creation time (unix seconds)
        timestamp: i64,
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
    /// Peer handshake for exchanging node information and known peers
    PeerHandshake {
        /// JSON-serialized handshake data (for forward compatibility)
        payload: Vec<u8>,
    },
}

/// Handshake request/response for peer exchange
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeData {
    pub data: Vec<u8>,
}

/// Combined network behavior for dchat
#[derive(NetworkBehaviour)]
pub struct DchatBehavior {
    /// Kademlia DHT for peer discovery and routing
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,

    /// mDNS for local network discovery (optional; disabled by default in production configs)
    pub mdns: Toggle<mdns::tokio::Behaviour>,

    /// Gossipsub for message propagation
    pub gossipsub: gossipsub::Behaviour,

    /// Identify protocol for peer information
    pub identify: identify::Behaviour,

    /// Ping for connection liveness
    pub ping: ping::Behaviour,

    /// Request-response for peer handshakes
    pub req_resp: cbor::Behaviour<HandshakeData, HandshakeData>,

    /// Relay client for NAT traversal (connect through relays)
    pub relay_client: relay::client::Behaviour,

    /// DCUtR for direct connection upgrade (hole punching)
    pub dcutr: dcutr::Behaviour,
}

impl DchatBehavior {
    /// Create a new dchat network behavior
    ///
    /// The relay_client must be created externally via `relay::client::new(peer_id)`
    /// so that the corresponding transport can be wrapped around the main transport.
    pub fn new(
        local_peer_id: PeerId,
        local_key: &libp2p::identity::Keypair,
        relay_client: relay::client::Behaviour,
        enable_mdns: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Kademlia configuration
        let store = kad::store::MemoryStore::new(local_peer_id);
        let kad_protocol = libp2p::StreamProtocol::new("/dchat/kad/1.0.0");
        let mut kad_config = kad::Config::new(kad_protocol);
        kad_config.set_query_timeout(Duration::from_secs(60));
        let kademlia = kad::Behaviour::with_config(local_peer_id, store, kad_config);

        // mDNS for local discovery (must be explicitly enabled)
        let mdns = if enable_mdns {
            let mdns_config = mdns::Config::default();
            Toggle::from(Some(mdns::tokio::Behaviour::new(
                mdns_config,
                local_peer_id,
            )?))
        } else {
            Toggle::from(None)
        };

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

        // Request-response for handshakes
        let protocols = std::iter::once((
            StreamProtocol::new("/dchat/handshake/1.0.0"),
            ProtocolSupport::Full,
        ));
        let req_resp_config =
            request_response::Config::default().with_request_timeout(Duration::from_secs(30));
        let req_resp = cbor::Behaviour::new(protocols, req_resp_config);

        // DCUtR for direct connection upgrade after relay (hole punching)
        let dcutr = dcutr::Behaviour::new(local_peer_id);

        Ok(Self {
            kademlia,
            mdns,
            gossipsub,
            identify,
            ping,
            req_resp,
            relay_client,
            dcutr,
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
        let data = encode_wire_message(message).map_err(|e| {
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
        let data = encode_wire_message(message).map_err(|e| {
            gossipsub::PublishError::TransformFailed(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Serialization failed: {}", e),
            ))
        })?;
        self.gossipsub.publish(topic, data)
    }

    /// Send a handshake request to a peer
    ///
    /// The handshake data is wrapped in a DchatMessage::PeerHandshake envelope
    /// so it can be properly decoded on the receiving end.
    pub fn send_handshake(&mut self, peer_id: PeerId, data: Vec<u8>) -> OutboundRequestId {
        // Wrap the raw handshake bytes in a DchatMessage envelope
        let handshake_msg = DchatMessage::PeerHandshake { payload: data };
        let wrapped_data = encode_wire_message(&handshake_msg).unwrap_or_else(|_| Vec::new()); // Fallback to empty on encode failure
        self.req_resp
            .send_request(&peer_id, HandshakeData { data: wrapped_data })
    }
}

/// Custom message ID function for gossipsub
fn message_id_fn(message: &gossipsub::Message) -> MessageId {
    if let Ok(dchat_msg) = decode_wire_message(&message.data) {
        if let DchatMessage::ChannelMessage { message_id, .. } = dchat_msg {
            return MessageId::from(hex::encode(message_id));
        }
    }

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
        let (_transport, relay_client) = relay::client::new(peer_id);
        let behavior = DchatBehavior::new(peer_id, &keypair, relay_client, false);
        assert!(behavior.is_ok());
    }

    #[test]
    fn test_channel_subscription() {
        let keypair = Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();
        let (_transport, relay_client) = relay::client::new(peer_id);
        let mut behavior = DchatBehavior::new(peer_id, &keypair, relay_client, false).unwrap();

        let result = behavior.subscribe_channel("test-channel");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
    }

    #[test]
    fn wire_roundtrip_envelope() {
        let sender = UserId::new();
        let timestamp = 1_735_000_000i64;
        let encrypted_payload = b"hello".to_vec();
        let channel_id = "global";
        let message_id =
            compute_channel_message_id(&sender, channel_id, &encrypted_payload, timestamp);

        let msg = DchatMessage::ChannelMessage {
            message_id,
            sender,
            channel_id: channel_id.to_string(),
            encrypted_payload,
            timestamp,
        };

        let bytes = encode_wire_message(&msg).expect("encode");
        let decoded = decode_wire_message(&bytes).expect("decode");

        match decoded {
            DchatMessage::ChannelMessage {
                message_id: mid,
                channel_id: cid,
                timestamp: ts,
                ..
            } => {
                assert_eq!(mid, message_id);
                assert_eq!(cid, channel_id);
                assert_eq!(ts, timestamp);
            }
            _ => panic!("unexpected message type"),
        }
    }

    #[test]
    fn wire_decode_legacy_fallback() {
        let sender = UserId::new();
        let timestamp = 1_735_000_001i64;
        let encrypted_payload = b"legacy".to_vec();
        let channel_id = "global";
        let message_id =
            compute_channel_message_id(&sender, channel_id, &encrypted_payload, timestamp);

        let msg = DchatMessage::ChannelMessage {
            message_id,
            sender,
            channel_id: channel_id.to_string(),
            encrypted_payload,
            timestamp,
        };

        let legacy_bytes = bincode::serialize(&msg).expect("legacy encode");
        let decoded = decode_wire_message(&legacy_bytes).expect("legacy decode");
        match decoded {
            DchatMessage::ChannelMessage {
                message_id: mid, ..
            } => assert_eq!(mid, message_id),
            _ => panic!("unexpected message type"),
        }
    }
}
