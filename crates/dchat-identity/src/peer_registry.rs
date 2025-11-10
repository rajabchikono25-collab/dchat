// Peer Identity Registry
//
// Maintains mapping of PeerIds to authenticated identities after handshake completion.
// Replaces placeholder PeerIds with actual authenticated identities for reputation tracking,
// slashing correlation, and operational diagnostics.

use dchat_network::PeerId;
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime};

/// Authenticated peer identity after successful handshake
#[derive(Debug, Clone)]
pub struct AuthenticatedPeer {
    /// Libp2p peer ID (not serializable)
    pub peer_id: PeerId,
    
    /// Ed25519 public key from identity
    pub public_key: VerifyingKey,
    
    /// Geographic region (if validator)
    pub region: Option<String>,
    
    /// How this peer was discovered
    pub discovered_via: DiscoveryMethod,
    
    /// First time we saw this peer
    pub first_seen: SystemTime,
    
    /// Last activity timestamp (not serializable)
    pub last_active: Instant,
    
    /// Validator stake amount (if validator)
    pub stake_amount: Option<u64>,
    
    /// Role in the network
    pub role: PeerRole,
    
    /// Reputation score (0.0 to 1.0)
    pub reputation: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerRole {
    Validator,
    Relay,
    User,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryMethod {
    Dns(String),      // DNS subdomain used
    Dht,              // DHT discovery
    Bootstrap,        // Manual bootstrap list
    Gossip,           // Learned via gossip
    Cached,           // From persistent cache
}

/// Thread-safe peer registry
#[derive(Debug, Clone)]
pub struct PeerRegistry {
    peers: Arc<RwLock<HashMap<PeerId, AuthenticatedPeer>>>,
}

impl PeerRegistry {
    /// Create a new empty peer registry
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Register a new authenticated peer after handshake
    pub fn register_peer(&self, peer: AuthenticatedPeer) -> Result<(), RegistryError> {
        let mut peers = self.peers.write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        peers.insert(peer.peer_id, peer);
        Ok(())
    }
    
    /// Update peer after DNS discovery completes handshake
    pub fn update_after_handshake(
        &self,
        peer_id: PeerId,
        public_key: VerifyingKey,
        discovered_via: DiscoveryMethod,
    ) -> Result<(), RegistryError> {
        let mut peers = self.peers.write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        if let Some(peer) = peers.get_mut(&peer_id) {
            peer.public_key = public_key;
            peer.discovered_via = discovered_via;
            peer.last_active = Instant::now();
        } else {
            // Create new entry
            peers.insert(peer_id, AuthenticatedPeer {
                peer_id,
                public_key,
                region: None,
                discovered_via,
                first_seen: SystemTime::now(),
                last_active: Instant::now(),
                stake_amount: None,
                role: PeerRole::Unknown,
                reputation: 1.0, // Start with full reputation
            });
        }
        
        Ok(())
    }
    
    /// Get authenticated peer info
    pub fn get_peer(&self, peer_id: &PeerId) -> Result<Option<AuthenticatedPeer>, RegistryError> {
        let peers = self.peers.read()
            .map_err(|_| RegistryError::LockPoisoned)?;
        Ok(peers.get(peer_id).cloned())
    }
    
    /// Update peer's last activity
    pub fn mark_active(&self, peer_id: &PeerId) -> Result<(), RegistryError> {
        let mut peers = self.peers.write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.last_active = Instant::now();
        }
        
        Ok(())
    }
    
    /// Set peer's region (for validators)
    pub fn set_region(&self, peer_id: &PeerId, region: String) -> Result<(), RegistryError> {
        let mut peers = self.peers.write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.region = Some(region);
        }
        
        Ok(())
    }
    
    /// Set peer's stake amount (for validators/relays)
    pub fn set_stake(&self, peer_id: &PeerId, stake: u64) -> Result<(), RegistryError> {
        let mut peers = self.peers.write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.stake_amount = Some(stake);
        }
        
        Ok(())
    }
    
    /// Set peer's role
    pub fn set_role(&self, peer_id: &PeerId, role: PeerRole) -> Result<(), RegistryError> {
        let mut peers = self.peers.write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.role = role;
        }
        
        Ok(())
    }
    
    /// Update peer's reputation score
    pub fn update_reputation(&self, peer_id: &PeerId, reputation: f64) -> Result<(), RegistryError> {
        let mut peers = self.peers.write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.reputation = reputation.clamp(0.0, 1.0);
        }
        
        Ok(())
    }
    
    /// Get all peers matching a filter
    pub fn get_peers_by_role(&self, role: PeerRole) -> Result<Vec<AuthenticatedPeer>, RegistryError> {
        let peers = self.peers.read()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        Ok(peers.values()
            .filter(|p| p.role == role)
            .cloned()
            .collect())
    }
    
    /// Get all validators
    pub fn get_validators(&self) -> Result<Vec<AuthenticatedPeer>, RegistryError> {
        self.get_peers_by_role(PeerRole::Validator)
    }
    
    /// Get all relays
    pub fn get_relays(&self) -> Result<Vec<AuthenticatedPeer>, RegistryError> {
        self.get_peers_by_role(PeerRole::Relay)
    }
    
    /// Get validator count by region
    pub fn get_region_distribution(&self) -> Result<HashMap<String, usize>, RegistryError> {
        let peers = self.peers.read()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        let mut distribution = HashMap::new();
        
        for peer in peers.values() {
            if peer.role == PeerRole::Validator {
                if let Some(ref region) = peer.region {
                    *distribution.entry(region.clone()).or_insert(0usize) += 1;
                }
            }
        }
        
        Ok(distribution)
    }
    
    /// Remove inactive peers (haven't been seen in threshold duration)
    pub fn prune_inactive(&self, inactive_threshold: std::time::Duration) -> Result<usize, RegistryError> {
        let mut peers = self.peers.write()
            .map_err(|_| RegistryError::LockPoisoned)?;
        
        let before_count = peers.len();
        let now = Instant::now();
        
        peers.retain(|_, peer| {
            now.duration_since(peer.last_active) < inactive_threshold
        });
        
        Ok(before_count - peers.len())
    }
    
    /// Get total peer count
    pub fn peer_count(&self) -> Result<usize, RegistryError> {
        let peers = self.peers.read()
            .map_err(|_| RegistryError::LockPoisoned)?;
        Ok(peers.len())
    }
    
    /// Get peer count by role
    pub fn role_count(&self, role: PeerRole) -> Result<usize, RegistryError> {
        let peers = self.peers.read()
            .map_err(|_| RegistryError::LockPoisoned)?;
        Ok(peers.values().filter(|p| p.role == role).count())
    }
}

impl Default for PeerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Registry lock poisoned")]
    LockPoisoned,
    
    #[error("Peer not found: {0}")]
    PeerNotFound(PeerId),
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_peer_registration() {
        let registry = PeerRegistry::new();
        let peer_id = PeerId::random();
        let public_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng()).verifying_key();
        
        let peer = AuthenticatedPeer {
            peer_id,
            public_key,
            region: Some("us-east".to_string()),
            discovered_via: DiscoveryMethod::Dns("validator1.dchat.network".to_string()),
            first_seen: SystemTime::now(),
            last_active: Instant::now(),
            stake_amount: Some(10_000_000),
            role: PeerRole::Validator,
            reputation: 1.0,
        };
        
        registry.register_peer(peer.clone()).unwrap();
        
        let retrieved = registry.get_peer(&peer_id).unwrap().unwrap();
        assert_eq!(retrieved.peer_id, peer_id);
        assert_eq!(retrieved.role, PeerRole::Validator);
    }
    
    #[test]
    fn test_region_distribution() {
        let registry = PeerRegistry::new();
        
        for i in 0..7 {
            let peer_id = PeerId::random();
            let public_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng()).verifying_key();
            let region = match i % 3 {
                0 => "us-east",
                1 => "eu-west",
                _ => "asia-se",
            };
            
            let peer = AuthenticatedPeer {
                peer_id,
                public_key,
                region: Some(region.to_string()),
                discovered_via: DiscoveryMethod::Dns(format!("validator{}.dchat.network", i)),
                first_seen: SystemTime::now(),
                last_active: Instant::now(),
                stake_amount: Some(10_000_000),
                role: PeerRole::Validator,
                reputation: 1.0,
            };
            
            registry.register_peer(peer).unwrap();
        }
        
        let distribution = registry.get_region_distribution().unwrap();
        assert_eq!(distribution.len(), 3);
    }
}
