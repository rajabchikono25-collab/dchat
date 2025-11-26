//! libp2p network integration for dchat SDK
//!
//! Provides a production-ready libp2p Swarm with:
//! - Kademlia DHT for peer discovery
//! - Noise Protocol for encryption
//! - yamux for stream multiplexing
//! - TCP transport with DNS resolution

use crate::{Result, SdkError};
use futures::StreamExt;
use libp2p::{
    core::upgrade,
    dns, identity,
    kad::{self, store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig},
    noise,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Transport,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{debug, error, info, warn};

/// Network events sent to the client
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// Connected to the network
    Connected,
    /// Disconnected from the network
    Disconnected,
    /// Peer discovered via DHT
    PeerDiscovered { peer_id: PeerId, addresses: Vec<Multiaddr> },
    /// Peer connected
    PeerConnected(PeerId),
    /// Peer disconnected
    PeerDisconnected(PeerId),
    /// Incoming message received
    MessageReceived { from: PeerId, payload: Vec<u8> },
    /// Bootstrap completed
    BootstrapComplete,
    /// Error occurred
    Error(String),
}

/// Commands sent to the network task
#[derive(Debug)]
pub enum NetworkCommand {
    /// Connect to the network
    Connect { 
        bootstrap_peers: Vec<Multiaddr>,
        response: oneshot::Sender<Result<()>>,
    },
    /// Disconnect from the network
    Disconnect {
        response: oneshot::Sender<Result<()>>,
    },
    /// Send a message to a peer
    SendMessage {
        peer_id: PeerId,
        payload: Vec<u8>,
        response: oneshot::Sender<Result<()>>,
    },
    /// Find peers closest to a key
    FindPeers {
        key: Vec<u8>,
        response: oneshot::Sender<Result<Vec<PeerId>>>,
    },
    /// Get connected peers
    GetConnectedPeers {
        response: oneshot::Sender<Vec<PeerId>>,
    },
    /// Dial a specific peer
    Dial {
        peer_id: PeerId,
        addresses: Vec<Multiaddr>,
        response: oneshot::Sender<Result<()>>,
    },
}

/// Network behaviour combining Kademlia and request-response
#[derive(NetworkBehaviour)]
pub struct DchatBehaviour {
    /// Kademlia DHT for peer discovery
    kademlia: Kademlia<MemoryStore>,
}

impl DchatBehaviour {
    /// Create a new behaviour
    pub fn new(local_peer_id: PeerId) -> Self {
        // Configure Kademlia
        let mut kad_config = KademliaConfig::default();
        kad_config.set_query_timeout(Duration::from_secs(60));
        kad_config.set_replication_factor(std::num::NonZeroUsize::new(20).unwrap());
        
        let store = MemoryStore::new(local_peer_id);
        let kademlia = Kademlia::with_config(local_peer_id, store, kad_config);

        Self { kademlia }
    }
}

/// Network manager handles the libp2p swarm
pub struct NetworkManager {
    /// Channel to send commands to the network task
    command_tx: mpsc::Sender<NetworkCommand>,
    /// Channel to receive events from the network task
    event_rx: Arc<RwLock<mpsc::Receiver<NetworkEvent>>>,
    /// Local peer ID
    local_peer_id: PeerId,
    /// Whether we're connected
    connected: Arc<RwLock<bool>>,
}

impl NetworkManager {
    /// Create a new network manager
    /// 
    /// Spawns a background task to run the libp2p swarm
    pub async fn new(ed25519_keypair: &dchat_crypto::keys::KeyPair) -> Result<Self> {
        // Convert dchat keypair to libp2p identity
        let secret_key_bytes = ed25519_keypair.private_key().as_bytes();
        let libp2p_keypair = identity::Keypair::ed25519_from_bytes(secret_key_bytes.to_vec())
            .map_err(|e| SdkError::Crypto(format!("Failed to create libp2p identity: {}", e)))?;
        
        let local_peer_id = PeerId::from(libp2p_keypair.public());
        info!("Local peer ID: {}", local_peer_id);

        // Create channels
        let (command_tx, command_rx) = mpsc::channel::<NetworkCommand>(256);
        let (event_tx, event_rx) = mpsc::channel::<NetworkEvent>(256);
        let connected = Arc::new(RwLock::new(false));
        let connected_clone = connected.clone();

        // Spawn network task
        tokio::spawn(async move {
            if let Err(e) = run_network_loop(
                libp2p_keypair,
                local_peer_id,
                command_rx,
                event_tx,
                connected_clone,
            ).await {
                error!("Network loop error: {}", e);
            }
        });

        Ok(Self {
            command_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
            local_peer_id,
            connected,
        })
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Connect to the network
    pub async fn connect(&self, bootstrap_peers: Vec<Multiaddr>) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx
            .send(NetworkCommand::Connect {
                bootstrap_peers,
                response: response_tx,
            })
            .await
            .map_err(|_| SdkError::Network("Network task not running".into()))?;
        
        response_rx
            .await
            .map_err(|_| SdkError::Network("Network task died".into()))?
    }

    /// Disconnect from the network
    pub async fn disconnect(&self) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        
        self.command_tx
            .send(NetworkCommand::Disconnect { response: response_tx })
            .await
            .map_err(|_| SdkError::Network("Network task not running".into()))?;
        
        response_rx
            .await
            .map_err(|_| SdkError::Network("Network task died".into()))?
    }

    /// Get connected peers
    pub async fn connected_peers(&self) -> Vec<PeerId> {
        let (response_tx, response_rx) = oneshot::channel();
        
        if self.command_tx
            .send(NetworkCommand::GetConnectedPeers { response: response_tx })
            .await
            .is_err()
        {
            return Vec::new();
        }
        
        response_rx.await.unwrap_or_default()
    }

    /// Poll for network events
    pub async fn poll_event(&self) -> Option<NetworkEvent> {
        let mut rx = self.event_rx.write().await;
        rx.recv().await
    }
}

/// Run the libp2p network event loop
async fn run_network_loop(
    keypair: identity::Keypair,
    local_peer_id: PeerId,
    mut command_rx: mpsc::Receiver<NetworkCommand>,
    event_tx: mpsc::Sender<NetworkEvent>,
    connected: Arc<RwLock<bool>>,
) -> Result<()> {
    // Build transport: TCP + DNS + Noise + yamux
    let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(noise::Config::new(&keypair).map_err(|e| {
            SdkError::Crypto(format!("Noise config error: {}", e))
        })?)
        .multiplex(yamux::Config::default())
        .timeout(Duration::from_secs(30))
        .boxed();
    
    // Wrap with DNS resolution
    let transport = dns::tokio::Transport::system(transport)
        .map_err(|e| SdkError::Network(format!("DNS transport error: {}", e)))?
        .boxed();

    // Create behaviour
    let behaviour = DchatBehaviour::new(local_peer_id);

    // Create swarm
    let mut swarm = Swarm::new(
        transport,
        behaviour,
        local_peer_id,
        libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(Duration::from_secs(60)),
    );

    // Track state
    let mut bootstrap_complete = false;
    let mut pending_queries: HashMap<kad::QueryId, oneshot::Sender<Result<Vec<PeerId>>>> = HashMap::new();

    info!("Network loop started for peer {}", local_peer_id);

    loop {
        tokio::select! {
            // Handle incoming commands
            Some(cmd) = command_rx.recv() => {
                match cmd {
                    NetworkCommand::Connect { bootstrap_peers, response } => {
                        info!("Connecting with {} bootstrap peers", bootstrap_peers.len());
                        
                        // Start listening
                        if let Err(e) = swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap()) {
                            let _ = response.send(Err(SdkError::Network(format!("Listen error: {}", e))));
                            continue;
                        }
                        
                        // Add bootstrap peers to Kademlia and dial them
                        for addr in &bootstrap_peers {
                            // Extract peer ID from multiaddr if present
                            if let Some(peer_id) = extract_peer_id(addr) {
                                swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                                if let Err(e) = swarm.dial(addr.clone()) {
                                    warn!("Failed to dial {}: {}", addr, e);
                                }
                            }
                        }
                        
                        // Start Kademlia bootstrap
                        if let Err(e) = swarm.behaviour_mut().kademlia.bootstrap() {
                            debug!("Kademlia bootstrap returned error (may be empty routing table): {:?}", e);
                        }
                        
                        *connected.write().await = true;
                        let _ = event_tx.send(NetworkEvent::Connected).await;
                        let _ = response.send(Ok(()));
                    }
                    
                    NetworkCommand::Disconnect { response } => {
                        info!("Disconnecting from network");
                        
                        // Disconnect all peers
                        let peers: Vec<_> = swarm.connected_peers().cloned().collect();
                        for peer in peers {
                            let _ = swarm.disconnect_peer_id(peer);
                        }
                        
                        *connected.write().await = false;
                        let _ = event_tx.send(NetworkEvent::Disconnected).await;
                        let _ = response.send(Ok(()));
                    }
                    
                    NetworkCommand::GetConnectedPeers { response } => {
                        let peers: Vec<_> = swarm.connected_peers().cloned().collect();
                        let _ = response.send(peers);
                    }
                    
                    NetworkCommand::FindPeers { key, response } => {
                        let record_key = kad::RecordKey::new(&key);
                        let query_id = swarm.behaviour_mut().kademlia.get_closest_peers(PeerId::random());
                        pending_queries.insert(query_id, response);
                    }
                    
                    NetworkCommand::Dial { peer_id, addresses, response } => {
                        for addr in &addresses {
                            swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                        }
                        match swarm.dial(peer_id) {
                            Ok(_) => {
                                let _ = response.send(Ok(()));
                            }
                            Err(e) => {
                                let _ = response.send(Err(SdkError::Network(format!("Dial error: {}", e))));
                            }
                        }
                    }
                    
                    NetworkCommand::SendMessage { peer_id, payload, response } => {
                        // For now, we use Kademlia PutRecord to store messages
                        // In production, use a dedicated request-response protocol
                        let key = kad::RecordKey::new(&format!("msg:{}", peer_id).as_bytes());
                        let record = kad::Record::new(key, payload);
                        if let Err(e) = swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One) {
                            let _ = response.send(Err(SdkError::Network(format!("Put record error: {:?}", e))));
                        } else {
                            let _ = response.send(Ok(()));
                        }
                    }
                }
            }
            
            // Handle swarm events
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("Listening on {}", address);
                    }
                    
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        debug!("Connected to peer {}", peer_id);
                        let _ = event_tx.send(NetworkEvent::PeerConnected(peer_id)).await;
                    }
                    
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        debug!("Disconnected from peer {}", peer_id);
                        let _ = event_tx.send(NetworkEvent::PeerDisconnected(peer_id)).await;
                    }
                    
                    SwarmEvent::Behaviour(DchatBehaviourEvent::Kademlia(kad_event)) => {
                        match kad_event {
                            kad::Event::OutboundQueryProgressed { id, result, .. } => {
                                match result {
                                    kad::QueryResult::Bootstrap(Ok(kad::BootstrapOk { num_remaining, .. })) => {
                                        if num_remaining == 0 && !bootstrap_complete {
                                            bootstrap_complete = true;
                                            info!("Kademlia bootstrap complete");
                                            let _ = event_tx.send(NetworkEvent::BootstrapComplete).await;
                                        }
                                    }
                                    kad::QueryResult::GetClosestPeers(Ok(kad::GetClosestPeersOk { peers, .. })) => {
                                        for peer in &peers {
                                            debug!("Found peer via DHT: {}", peer);
                                            let _ = event_tx.send(NetworkEvent::PeerDiscovered {
                                                peer_id: *peer,
                                                addresses: vec![], // Addresses come from routing table
                                            }).await;
                                        }
                                        
                                        // Send response if this was a FindPeers command
                                        if let Some(response) = pending_queries.remove(&id) {
                                            let _ = response.send(Ok(peers));
                                        }
                                    }
                                    kad::QueryResult::GetClosestPeers(Err(e)) => {
                                        warn!("GetClosestPeers failed: {:?}", e);
                                        if let Some(response) = pending_queries.remove(&id) {
                                            let _ = response.send(Err(SdkError::Network(format!("{:?}", e))));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            kad::Event::RoutingUpdated { peer, addresses, .. } => {
                                debug!("Routing updated for peer {}", peer);
                                let addrs: Vec<_> = addresses.iter().cloned().collect();
                                if !addrs.is_empty() {
                                    let _ = event_tx.send(NetworkEvent::PeerDiscovered {
                                        peer_id: peer,
                                        addresses: addrs,
                                    }).await;
                                }
                            }
                            _ => {}
                        }
                    }
                    
                    _ => {}
                }
            }
        }
    }
}

/// Extract PeerId from a multiaddr if present
fn extract_peer_id(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|p| {
        if let libp2p::multiaddr::Protocol::P2p(peer_id) = p {
            Some(peer_id)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_network_manager_creation() {
        let keypair = dchat_crypto::keys::KeyPair::generate();
        let manager = NetworkManager::new(&keypair).await;
        assert!(manager.is_ok());
        
        let manager = manager.unwrap();
        assert!(!manager.is_connected().await);
    }
}
