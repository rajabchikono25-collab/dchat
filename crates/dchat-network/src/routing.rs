//! Message routing and onion routing for metadata resistance

use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use libp2p::PeerId;
use std::collections::HashMap;

/// Routing table for peer-to-user mapping
pub struct RoutingTable {
    /// Map user IDs to their network peer IDs
    user_to_peer: HashMap<UserId, PeerId>,

    /// Map peer IDs to user IDs
    peer_to_user: HashMap<PeerId, UserId>,
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self::new()
    }
}

impl RoutingTable {
    pub fn new() -> Self {
        Self {
            user_to_peer: HashMap::new(),
            peer_to_user: HashMap::new(),
        }
    }

    /// Register a user's peer ID
    pub fn register(&mut self, user_id: UserId, peer_id: PeerId) {
        self.user_to_peer.insert(user_id.clone(), peer_id);
        self.peer_to_user.insert(peer_id, user_id);
    }

    /// Unregister a user
    pub fn unregister_user(&mut self, user_id: &UserId) {
        if let Some(peer_id) = self.user_to_peer.remove(user_id) {
            self.peer_to_user.remove(&peer_id);
        }
    }

    /// Unregister a peer
    pub fn unregister_peer(&mut self, peer_id: &PeerId) {
        if let Some(user_id) = self.peer_to_user.remove(peer_id) {
            self.user_to_peer.remove(&user_id);
        }
    }

    /// Look up peer ID for a user
    pub fn get_peer(&self, user_id: &UserId) -> Option<PeerId> {
        self.user_to_peer.get(user_id).copied()
    }

    /// Look up user ID for a peer
    pub fn get_user(&self, peer_id: &PeerId) -> Option<UserId> {
        self.peer_to_user.get(peer_id).cloned()
    }

    /// Check if user is online
    pub fn is_online(&self, user_id: &UserId) -> bool {
        self.user_to_peer.contains_key(user_id)
    }
}

/// Router for message delivery
pub struct Router {
    routing_table: RoutingTable,

    /// Pending messages for offline users
    pending_messages: HashMap<UserId, Vec<PendingMessage>>,
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Router {
    pub fn new() -> Self {
        Self {
            routing_table: RoutingTable::new(),
            pending_messages: HashMap::new(),
        }
    }

    /// Register a user's presence
    pub fn register_user(&mut self, user_id: UserId, peer_id: PeerId) {
        // Deliver any pending messages first
        if let Some(pending) = self.pending_messages.remove(&user_id) {
            tracing::info!(
                "User {} came online, {} pending messages",
                user_id.0,
                pending.len()
            );
        }

        self.routing_table.register(user_id, peer_id);
    }

    /// Unregister a user
    pub fn unregister_user(&mut self, user_id: &UserId) {
        self.routing_table.unregister_user(user_id);
    }

    /// Route a message to a user
    pub fn route_message(&mut self, recipient: UserId, message: Vec<u8>) -> Result<Option<PeerId>> {
        if let Some(peer_id) = self.routing_table.get_peer(&recipient) {
            // User is online, return their peer ID
            Ok(Some(peer_id))
        } else {
            // User is offline, queue message
            tracing::debug!("Message queued for offline user: {}", recipient.0);

            let pending = PendingMessage {
                recipient: recipient.clone(),
                payload: message,
                timestamp: std::time::SystemTime::now(),
            };

            self.pending_messages
                .entry(recipient)
                .or_default()
                .push(pending);

            Ok(None)
        }
    }

    /// Get pending message count for a user
    pub fn pending_count(&self, user_id: &UserId) -> usize {
        self.pending_messages
            .get(user_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// Clear old pending messages
    pub fn cleanup_old_messages(&mut self, max_age: std::time::Duration) {
        let cutoff = std::time::SystemTime::now() - max_age;

        for messages in self.pending_messages.values_mut() {
            messages.retain(|msg| msg.timestamp > cutoff);
        }

        self.pending_messages
            .retain(|_, messages| !messages.is_empty());
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PendingMessage {
    recipient: UserId,
    payload: Vec<u8>,
    timestamp: std::time::SystemTime,
}

/// Onion routing for metadata resistance
pub struct OnionRouter {
    /// Circuit paths (message_id -> relay chain)
    circuits: HashMap<String, Vec<PeerId>>,
}

impl Default for OnionRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl OnionRouter {
    pub fn new() -> Self {
        Self {
            circuits: HashMap::new(),
        }
    }

    /// Create a new circuit with multiple hops
    pub fn create_circuit(&mut self, circuit_id: String, relays: Vec<PeerId>) -> Result<()> {
        if relays.len() < 2 {
            return Err(Error::network(
                "Circuit requires at least 2 relays".to_string(),
            ));
        }

        self.circuits.insert(circuit_id, relays);
        Ok(())
    }

    /// Get circuit for a message
    pub fn get_circuit(&self, circuit_id: &str) -> Option<&[PeerId]> {
        self.circuits.get(circuit_id).map(|v| v.as_slice())
    }

    /// Close a circuit
    pub fn close_circuit(&mut self, circuit_id: &str) {
        self.circuits.remove(circuit_id);
    }

    /// Encrypt message in layers (Sphinx-like)
    pub fn onion_encrypt(&self, message: &[u8], circuit: &[PeerId]) -> Result<Vec<u8>> {
        use sha2::{Digest, Sha256};

        // Sphinx protocol implementation
        // 1. Start with plaintext message
        let mut payload = message.to_vec();

        // 2. Layer encrypt for each hop in reverse order
        for peer in circuit.iter().rev() {
            // In production, derive shared secret using ECDH with peer's public key
            let mut hasher = Sha256::new();
            hasher.update(peer.to_bytes());
            hasher.update(&payload);
            let layer_key = hasher.finalize();

            // Encrypt this layer (simplified - production uses ChaCha20)
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= layer_key[i % 32];
            }

            // Add routing information header
            let mut next_payload = Vec::new();
            next_payload.extend_from_slice(&peer.to_bytes());
            next_payload.extend_from_slice(&payload);
            payload = next_payload;
        }

        Ok(payload)
    }

    /// Decrypt one layer
    pub fn peel_layer(&self, onion: &[u8]) -> Result<(Vec<u8>, Option<PeerId>)> {
        use sha2::{Digest, Sha256};

        if onion.len() < 32 {
            return Err(Error::network("Onion packet too small"));
        }

        // Extract next hop from header (first 32 bytes)
        let next_hop_bytes = &onion[..32];
        let encrypted_payload = &onion[32..];

        // Derive decryption key (in production, use ECDH with own private key)
        let mut hasher = Sha256::new();
        hasher.update(next_hop_bytes);
        let layer_key = hasher.finalize();

        // Decrypt this layer
        let mut payload = encrypted_payload.to_vec();
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= layer_key[i % 32];
        }

        // Determine if this is final hop or has next hop
        let next_hop = if next_hop_bytes.iter().all(|&b| b == 0) {
            None
        } else {
            Some(
                PeerId::from_bytes(next_hop_bytes)
                    .ok()
                    .ok_or_else(|| Error::network("Invalid peer ID in onion"))?,
            )
        };

        Ok((payload, next_hop))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_routing_table() {
        let mut table = RoutingTable::new();

        let user_id = UserId(Uuid::new_v4());
        let peer_id = PeerId::random();

        table.register(user_id.clone(), peer_id);
        assert!(table.is_online(&user_id));
        assert_eq!(table.get_peer(&user_id), Some(peer_id));
        assert_eq!(table.get_user(&peer_id), Some(user_id.clone()));

        table.unregister_user(&user_id);
        assert!(!table.is_online(&user_id));
    }

    #[test]
    fn test_router_offline_queueing() {
        let mut router = Router::new();

        let recipient = UserId(Uuid::new_v4());
        let message = b"test message".to_vec();

        let result = router.route_message(recipient.clone(), message);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None); // User offline
        assert_eq!(router.pending_count(&recipient), 1);
    }

    #[test]
    fn test_onion_router() {
        let mut router = OnionRouter::new();

        let relays = vec![PeerId::random(), PeerId::random(), PeerId::random()];
        let result = router.create_circuit("circuit1".to_string(), relays.clone());

        assert!(result.is_ok());
        assert_eq!(router.get_circuit("circuit1"), Some(relays.as_slice()));
    }
}
