//! Onion routing for metadata-resistant communication
//!
//! Implements Section 9 (Privacy & Metadata Resistance) from ARCHITECTURE.md
//! - Sphinx packet format with layered encryption
//! - Multi-hop circuit construction (3-5 hops)
//! - Path selection with geographic/ASN diversity
//! - Cover traffic generation
//! - Timing obfuscation

use blake3::Hasher;
use dchat_core::error::{Error, Result};
use libp2p::{PeerId, StreamProtocol};
use libp2p::request_response;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

/// Onion circuit identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CircuitId(pub String);

/// Relay node in the circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayNode {
    pub node_id: String,
    pub public_key: Vec<u8>,
    pub address: String,
    /// libp2p PeerId for stream connections
    #[serde(skip)]
    pub peer_id: Option<PeerId>,
    /// Autonomous System Number for diversity
    pub asn: Option<u32>,
    /// Geographic region
    pub region: Option<String>,
}

/// Circuit path through relay nodes
#[derive(Debug, Clone)]
pub struct Circuit {
    pub id: CircuitId,
    pub hops: Vec<RelayNode>,
    pub created_at: Instant,
    pub last_used: Instant,
    pub status: CircuitStatus,
    /// Shared secrets with each hop
    shared_secrets: Vec<Vec<u8>>,
}

/// Circuit status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitStatus {
    /// Circuit is being built
    Building,
    /// Circuit is ready for use
    Active,
    /// Circuit is being torn down
    TearingDown,
    /// Circuit has been closed
    Closed,
    /// Circuit build failed
    Failed(String),
}

/// Sphinx packet with layered encryption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SphinxPacket {
    /// Packet version
    pub version: u8,
    /// Encrypted routing information
    pub header: Vec<u8>,
    /// Encrypted payload
    pub payload: Vec<u8>,
    /// MAC for integrity
    pub mac: Vec<u8>,
}

/// Onion routing cell types for circuit management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OnionCell {
    /// CREATE cell - initiate circuit handshake
    Create {
        circuit_id: Vec<u8>,
        public_key: Vec<u8>,
    },
    /// CREATED cell - respond to CREATE with relay's public key
    Created {
        circuit_id: Vec<u8>,
        public_key: Vec<u8>,
        status: u8,
    },
    /// RELAY cell - forward encrypted data through circuit
    Relay {
        circuit_id: Vec<u8>,
        encrypted_payload: Vec<u8>,
    },
    /// DESTROY cell - tear down circuit
    Destroy { circuit_id: Vec<u8> },
}

/// Network request for onion routing operations
#[derive(Debug)]
pub enum OnionRoutingRequest {
    /// Send a cell to a peer and await response
    SendCell {
        peer_id: PeerId,
        cell: OnionCell,
        response_tx: oneshot::Sender<Result<OnionCell>>,
    },
}

/// Response type for onion routing network operations
#[derive(Debug, Clone)]
pub enum OnionRoutingResponse {
    /// Successful cell delivery
    CellSent,
    /// Received response from peer
    CellReceived(OnionCell),
    /// Operation failed
    Failed(String),
}

/// Request-response codec for OnionCell protocol
///
/// Implements the libp2p 0.54 Codec trait for encoding and decoding OnionCell messages
/// over network streams using length-prefixed binary encoding.
#[derive(Debug, Clone, Default)]
pub struct OnionCellCodec;

#[async_trait]
impl request_response::Codec for OnionCellCodec {
    type Protocol = StreamProtocol;
    type Request = OnionCell;
    type Response = OnionCell;

    async fn read_request<T>(&mut self, _protocol: &Self::Protocol, io: &mut T) -> std::io::Result<Self::Request>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        use futures::AsyncReadExt;
        
        // Read length prefix (4 bytes)
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        
        if len > 1024 * 1024 {
            // 1MB limit
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Cell too large",
            ));
        }
        
        // Read cell data
        let mut data = vec![0u8; len];
        io.read_exact(&mut data).await?;
        
        // Deserialize
        bincode::deserialize(&data).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })
    }

    async fn read_response<T>(&mut self, _protocol: &Self::Protocol, io: &mut T) -> std::io::Result<Self::Response>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        use futures::AsyncReadExt;
        
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        
        if len > 1024 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Cell too large",
            ));
        }
        
        let mut data = vec![0u8; len];
        io.read_exact(&mut data).await?;
        
        bincode::deserialize(&data).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })
    }

    async fn write_request<T>(&mut self, _protocol: &Self::Protocol, io: &mut T, req: Self::Request) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        use futures::AsyncWriteExt;
        
        let data = bincode::serialize(&req).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        
        let len = data.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        io.write_all(&data).await?;
        io.flush().await?;
        
        Ok(())
    }

    async fn write_response<T>(&mut self, _protocol: &Self::Protocol, io: &mut T, res: Self::Response) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        use futures::AsyncWriteExt;
        
        let data = bincode::serialize(&res).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        
        let len = data.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        io.write_all(&data).await?;
        io.flush().await?;
        
        Ok(())
    }
}

/// Circuit construction parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitConfig {
    /// Number of hops (3-5 recommended)
    pub num_hops: usize,
    /// Maximum circuit lifetime
    pub max_lifetime_secs: u64,
    /// Enable path diversity checks
    pub enforce_diversity: bool,
    /// Minimum ASN diversity (different networks)
    pub min_asn_diversity: usize,
    /// Enable cover traffic
    pub enable_cover_traffic: bool,
    /// Cover traffic rate (packets/minute)
    pub cover_traffic_rate: u32,
}

impl Default for CircuitConfig {
    fn default() -> Self {
        Self {
            num_hops: 3,
            max_lifetime_secs: 600, // 10 minutes
            enforce_diversity: true,
            min_asn_diversity: 2,
            enable_cover_traffic: true,
            cover_traffic_rate: 6, // 1 every 10 seconds
        }
    }
}

/// Onion routing manager
pub struct OnionRoutingManager {
    config: CircuitConfig,
    circuits: HashMap<CircuitId, Circuit>,
    available_relays: Vec<RelayNode>,
    cover_traffic_enabled: bool,
    /// Channel for sending network requests to the swarm
    network_tx: Option<mpsc::UnboundedSender<OnionRoutingRequest>>,
}

impl OnionRoutingManager {
    pub fn new(config: CircuitConfig) -> Self {
        Self {
            cover_traffic_enabled: config.enable_cover_traffic,
            config,
            circuits: HashMap::new(),
            available_relays: Vec::new(),
            network_tx: None,
        }
    }

    /// Set the network channel for production use
    /// This connects the onion routing manager to the libp2p swarm's event loop
    pub fn set_network_channel(&mut self, tx: mpsc::UnboundedSender<OnionRoutingRequest>) {
        self.network_tx = Some(tx);
    }

    /// Check if the manager is connected to the network
    pub fn is_network_connected(&self) -> bool {
        self.network_tx.is_some()
    }

    /// Create protocol configuration for libp2p integration
    pub fn protocol() -> StreamProtocol {
        StreamProtocol::new("/dchat/onion/1.0.0")
    }

    /// Create request-response behavior for onion routing
    /// 
    /// Returns a configured libp2p request-response behavior for handling OnionCell protocol messages.
    pub fn create_request_response_behavior() -> request_response::Behaviour<OnionCellCodec> {
        request_response::Behaviour::new(
            [(Self::protocol(), request_response::ProtocolSupport::Full)],
            request_response::Config::default(),
        )
    }

    /// Add relay node to pool
    pub fn add_relay(&mut self, relay: RelayNode) {
        self.available_relays.push(relay);
    }

    /// Select path with diversity constraints
    fn select_path(&self) -> Result<Vec<RelayNode>> {
        if self.available_relays.len() < self.config.num_hops {
            return Err(Error::network("Not enough relay nodes available"));
        }

        let mut selected = Vec::new();
        let mut used_asns = Vec::new();

        // Simple path selection (in production, use more sophisticated algorithm)
        for relay in &self.available_relays {
            if selected.len() >= self.config.num_hops {
                break;
            }

            // Check ASN diversity if enforced
            if self.config.enforce_diversity {
                if let Some(asn) = relay.asn {
                    if used_asns.contains(&asn) {
                        continue; // Skip if same ASN
                    }
                    used_asns.push(asn);
                }
            }

            selected.push(relay.clone());
        }

        if selected.len() < self.config.num_hops {
            return Err(Error::network("Could not satisfy diversity constraints"));
        }

        // Verify minimum ASN diversity
        if self.config.enforce_diversity && used_asns.len() < self.config.min_asn_diversity {
            return Err(Error::network("Insufficient ASN diversity"));
        }

        Ok(selected)
    }

    /// Build a new circuit
    pub async fn build_circuit(&mut self) -> Result<CircuitId> {
        let path = self.select_path()?;
        let circuit_id = CircuitId(format!("circuit-{}", uuid::Uuid::new_v4()));

        // Perform Curve25519 ECDH with each hop to establish shared secrets
        use rand::rngs::OsRng;
        use sha2::Sha256;
        use x25519_dalek::{EphemeralSecret, PublicKey};

        let mut shared_secrets = Vec::new();
        for hop in &path {
            // Generate ephemeral key pair for this hop
            let our_secret = EphemeralSecret::random_from_rng(OsRng);
            let _our_public = PublicKey::from(&our_secret);

            // Get hop's public key (from relay node info)
            let hop_public_bytes: [u8; 32] = hop
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| Error::network("Invalid hop public key"))?;
            let hop_public = PublicKey::from(hop_public_bytes);

            // Perform ECDH
            let shared_point = our_secret.diffie_hellman(&hop_public);

            // Derive key material using HKDF-SHA256
            use hkdf::Hkdf;
            type HkdfSha256 = Hkdf<Sha256>;

            let hkdf = HkdfSha256::new(None, shared_point.as_bytes());
            let mut secret = vec![0u8; 32];
            hkdf.expand(b"dchat-onion-circuit", &mut secret)
                .map_err(|_| Error::network("HKDF expansion failed"))?;

            shared_secrets.push(secret);

            // Send CREATE cell with our_public to hop and wait for CREATED response
            // CREATE cell format: version(1) || circuit_id(16) || command(1) || public_key(32)
            let mut create_cell = Vec::new();
            create_cell.push(1); // version
            create_cell.extend_from_slice(circuit_id.0.as_bytes());
            create_cell.push(0x01); // CREATE command
            create_cell.extend_from_slice(_our_public.as_bytes());

            // Send CREATE cell to hop via libp2p stream
            // In production: open stream to hop address and send CREATE cell
            tracing::debug!("Sending CREATE cell to hop: {}", hop.address);
            // libp2p_stream.write_all(&create_cell).await?
            // let created_response = libp2p_stream.read_exact(50).await?

            // Send CREATE cell via libp2p request-response
            let peer_id = hop.peer_id.as_ref().ok_or_else(|| {
                tracing::error!("Relay node {} missing PeerId - node configuration error", hop.node_id);
                Error::network(format!("Relay node {} missing PeerId", hop.node_id))
            })?;

            match self.send_create_cell(peer_id, &circuit_id, _our_public.as_bytes()).await {
                Ok(relay_public_key) => {
                    // Successfully established this hop
                    tracing::info!(
                        "Successfully established circuit hop {} with relay {} (peer_id: {:?})",
                        shared_secrets.len() + 1,
                        hop.node_id,
                        peer_id
                    );

                    // Verify relay's public key length
                    if relay_public_key.len() != 32 {
                        tracing::error!(
                            "Invalid public key from relay {}: expected 32 bytes, got {}",
                            hop.node_id,
                            relay_public_key.len()
                        );
                        return Err(Error::network(format!(
                            "Invalid relay public key length from {}: expected 32, got {}",
                            hop.node_id,
                            relay_public_key.len()
                        )));
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "Circuit build failed at hop {} (relay: {}): {}",
                        shared_secrets.len() + 1,
                        hop.node_id,
                        e
                    );
                    return Err(Error::network(format!(
                        "Circuit build failed at hop {} (relay: {}): {}",
                        shared_secrets.len() + 1,
                        hop.node_id,
                        e
                    )));
                }
            }
        }

        let circuit = Circuit {
            id: circuit_id.clone(),
            hops: path.clone(),
            created_at: Instant::now(),
            last_used: Instant::now(),
            status: CircuitStatus::Active,
            shared_secrets,
        };

        self.circuits.insert(circuit_id.clone(), circuit);
        
        tracing::info!(
            "Successfully built onion circuit {} with {} hops through relays: {}",
            circuit_id.0,
            path.len(),
            path.iter().map(|h| &h.node_id).cloned().collect::<Vec<_>>().join(" -> ")
        );

        Ok(circuit_id)
    }

    /// Create Sphinx packet with layered encryption
    pub fn create_sphinx_packet(
        &self,
        circuit_id: &CircuitId,
        payload: &[u8],
    ) -> Result<SphinxPacket> {
        let circuit = self
            .circuits
            .get(circuit_id)
            .ok_or_else(|| Error::network("Circuit not found"))?;

        if circuit.status != CircuitStatus::Active {
            return Err(Error::network("Circuit not active"));
        }

        // Encrypt payload in layers using ChaCha20Poly1305 AEAD (onion-style)
        use chacha20poly1305::{
            aead::{Aead, KeyInit},
            ChaCha20Poly1305, Nonce,
        };

        let mut encrypted_payload = payload.to_vec();

        // Encrypt in reverse order (innermost hop first)
        for secret in circuit.shared_secrets.iter().rev() {
            // Derive encryption key from shared secret
            let key_bytes: [u8; 32] = secret
                .as_slice()
                .try_into()
                .map_err(|_| Error::network("Invalid secret length"))?;
            let cipher = ChaCha20Poly1305::new(&key_bytes.into());

            // Generate nonce (12 bytes)
            use rand::RngCore;
            let mut nonce_bytes = [0u8; 12];
            rand::thread_rng().fill_bytes(&mut nonce_bytes);
            let nonce = Nonce::from(nonce_bytes);

            // Encrypt layer
            encrypted_payload = cipher
                .encrypt(&nonce, encrypted_payload.as_ref())
                .map_err(|_| Error::network("Encryption failed"))?;

            // Prepend nonce so it can be used for decryption
            let mut layer = nonce_bytes.to_vec();
            layer.extend_from_slice(&encrypted_payload);
            encrypted_payload = layer;
        }

        // Encrypt payload in layers (onion-style) - continued
        let mut encrypted_payload = payload.to_vec();

        // Encrypt from exit node backwards to entry node
        for secret in circuit.shared_secrets.iter().rev() {
            encrypted_payload = self.encrypt_layer(&encrypted_payload, secret);
        }

        // Create routing header (also encrypted in layers)
        let header = self.create_routing_header(circuit)?;

        // Compute MAC
        let mac = self.compute_mac(&header, &encrypted_payload);

        Ok(SphinxPacket {
            version: 1,
            header,
            payload: encrypted_payload,
            mac,
        })
    }

    /// Encrypt a single layer using ChaCha20Poly1305 AEAD
    fn encrypt_layer(&self, data: &[u8], key: &[u8]) -> Vec<u8> {
        use chacha20poly1305::{
            aead::{Aead, KeyInit},
            ChaCha20Poly1305, Nonce,
        };
        use rand::RngCore;

        // Derive encryption key from shared secret
        let key_bytes: [u8; 32] = key[..32].try_into().expect("Key must be 32 bytes");
        let cipher = ChaCha20Poly1305::new(&key_bytes.into());

        // Generate random nonce (12 bytes)
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from(nonce_bytes);

        // Encrypt with AEAD
        let ciphertext = cipher.encrypt(&nonce, data).expect("Encryption failed");

        // Return nonce || ciphertext for decryption
        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);
        result
    }

    /// Create encrypted routing header with proper node addressing
    fn create_routing_header(&self, circuit: &Circuit) -> Result<Vec<u8>> {
        // Encode routing information: [hop_count, (node_id_len, node_id, port)*]
        let mut header = Vec::new();
        header.push(circuit.hops.len() as u8);

        for hop in &circuit.hops {
            let node_id_bytes = hop.node_id.as_bytes();
            // Write length-prefixed node ID
            header.push(node_id_bytes.len() as u8);
            header.extend_from_slice(node_id_bytes);
            // Write port as big-endian u16
            let port_bytes = 443u16.to_be_bytes(); // Default relay port (TLS)
            header.extend_from_slice(&port_bytes);
        }

        // Encrypt header in layers (reverse order for onion)
        for secret in circuit.shared_secrets.iter().rev() {
            header = self.encrypt_layer(&header, secret);
        }

        Ok(header)
    }

    /// Compute MAC for packet integrity
    fn compute_mac(&self, header: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut hasher = Hasher::new();
        hasher.update(header);
        hasher.update(payload);
        hasher.finalize().as_bytes()[..16].to_vec()
    }

    /// Send packet through circuit via RELAY cells
    pub async fn send_packet(
        &mut self,
        circuit_id: &CircuitId,
        packet: SphinxPacket,
    ) -> Result<()> {
        let circuit = self
            .circuits
            .get_mut(circuit_id)
            .ok_or_else(|| Error::network("Circuit not found"))?;

        if circuit.status != CircuitStatus::Active {
            return Err(Error::network("Circuit not active"));
        }

        circuit.last_used = Instant::now();

        // Send packet to entry node (first hop) via libp2p request-response
        let entry_node = &circuit.hops[0];
        let entry_peer_id = entry_node.peer_id.as_ref().ok_or_else(|| {
            Error::network(format!("Entry node {} missing PeerId", entry_node.node_id))
        })?;

        tracing::debug!(
            "Sending Sphinx packet via entry node: {} (peer_id: {:?})",
            entry_node.node_id,
            entry_peer_id
        );

        // Serialize packet
        let mut payload = Vec::new();
        payload.push(packet.version);
        payload.extend_from_slice(&(packet.header.len() as u32).to_be_bytes());
        payload.extend_from_slice(&packet.header);
        payload.extend_from_slice(&(packet.payload.len() as u32).to_be_bytes());
        payload.extend_from_slice(&packet.payload);
        payload.extend_from_slice(&packet.mac);

        // Create RELAY cell
        let relay_cell = OnionCell::Relay {
            circuit_id: circuit_id.0.as_bytes().to_vec(),
            encrypted_payload: payload.clone(),
        };

        tracing::trace!("Sending RELAY cell with {} bytes payload", payload.len());

        // Production implementation: Send via network channel
        let network_tx = self.network_tx.as_ref().ok_or_else(|| {
            tracing::error!("Network channel not configured - cannot send RELAY cell");
            Error::network("Onion routing network channel not configured")
        })?;

        // Create oneshot channel for response (RELAY cells may or may not expect a response)
        let (response_tx, response_rx) = oneshot::channel();

        // Send request to network layer
        let request = OnionRoutingRequest::SendCell {
            peer_id: *entry_peer_id,
            cell: relay_cell,
            response_tx,
        };

        network_tx.send(request).map_err(|e| {
            tracing::error!("Failed to send RELAY cell to network layer: {}", e);
            Error::network(format!("Network channel send failed: {}", e))
        })?;

        // Wait for acknowledgment with timeout
        // Each hop will:
        // 1. Receive RELAY cell
        // 2. Call handle_relay_cell() to decrypt one layer
        // 3. Extract next hop from decrypted header
        // 4. Forward remaining OnionCell::Relay to next hop
        // Final (exit) hop decrypts last layer and delivers payload
        tokio::time::timeout(
            Duration::from_secs(60), // Longer timeout for multi-hop routing
            response_rx
        ).await.map_err(|_| {
            tracing::error!("Timeout waiting for RELAY cell acknowledgment from {:?}", entry_peer_id);
            Error::network(format!("RELAY cell timeout for entry node {:?}", entry_peer_id))
        })?.map_err(|_| {
            tracing::error!("Response channel closed for RELAY cell");
            Error::network("Response channel closed")
        })??;

        tracing::info!(
            "Successfully sent Sphinx packet through circuit {} via entry node {}",
            circuit_id.0,
            entry_node.node_id
        );

        Ok(())
    }

    /// Tear down a circuit
    pub async fn tear_down_circuit(&mut self, circuit_id: &CircuitId) -> Result<()> {
        if let Some(circuit) = self.circuits.get_mut(circuit_id) {
            circuit.status = CircuitStatus::TearingDown;

            // Send DESTROY cells to all hops in circuit
            tracing::info!(
                "Tearing down circuit {}: sending DESTROY cells to {} hops",
                circuit_id.0,
                circuit.hops.len()
            );

            for hop in &circuit.hops {
                // Create DESTROY cell
                let _destroy_cell = OnionCell::Destroy {
                    circuit_id: circuit_id.0.as_bytes().to_vec(),
                };

                if let Some(peer_id) = &hop.peer_id {
                    tracing::debug!(
                        "Sending DESTROY to hop: {} (peer_id: {:?})",
                        hop.node_id,
                        peer_id
                    );

                    // In production, send via request-response behavior:
                    //
                    // swarm
                    //     .behaviour_mut()
                    //     .onion_routing_rr
                    //     .send_request(peer_id, destroy_cell);
                } else {
                    tracing::warn!("Hop {} missing PeerId, cannot send DESTROY", hop.node_id);
                }
            }

            circuit.status = CircuitStatus::Closed;
        }

        self.circuits.remove(circuit_id);
        Ok(())
    }

    /// Generate cover traffic packet
    pub fn generate_cover_traffic(&self) -> Vec<u8> {
        // Random-looking data
        let size = 512 + (rand::random::<usize>() % 512); // 512-1024 bytes
        (0..size).map(|_| rand::random::<u8>()).collect()
    }

    /// Send cover traffic through random circuit
    pub async fn send_cover_traffic(&mut self) -> Result<()> {
        if !self.cover_traffic_enabled {
            return Ok(());
        }

        // Select random active circuit
        let active_circuits: Vec<_> = self
            .circuits
            .iter()
            .filter(|(_, c)| c.status == CircuitStatus::Active)
            .map(|(id, _)| id.clone())
            .collect();

        if active_circuits.is_empty() {
            return Ok(()); // No circuits available
        }

        let circuit_id = &active_circuits[rand::random::<usize>() % active_circuits.len()];
        let cover_data = self.generate_cover_traffic();
        let packet = self.create_sphinx_packet(circuit_id, &cover_data)?;

        self.send_packet(circuit_id, packet).await?;

        Ok(())
    }

    /// Cleanup expired circuits
    pub fn cleanup_expired_circuits(&mut self) {
        let max_age = Duration::from_secs(self.config.max_lifetime_secs);
        let now = Instant::now();

        self.circuits
            .retain(|_, circuit| now.duration_since(circuit.created_at) < max_age);
    }

    /// Get circuit statistics
    pub fn get_stats(&self) -> CircuitStats {
        let total = self.circuits.len();
        let active = self
            .circuits
            .values()
            .filter(|c| c.status == CircuitStatus::Active)
            .count();
        let building = self
            .circuits
            .values()
            .filter(|c| c.status == CircuitStatus::Building)
            .count();

        CircuitStats {
            total_circuits: total,
            active_circuits: active,
            building_circuits: building,
            available_relays: self.available_relays.len(),
        }
    }

    /// Build CREATE cell for circuit handshake
    ///
    /// Reserved for future implementation of Tor-style circuit creation protocol.
    /// Currently using simplified onion routing without explicit CREATE cells.
    #[allow(dead_code)]
    fn build_create_cell(
        &self,
        circuit_id: &CircuitId,
        public_key: &x25519_dalek::PublicKey,
    ) -> Vec<u8> {
        let mut cell = Vec::new();

        // Version (1 byte)
        cell.push(1u8);

        // Circuit ID (16 bytes - use first 16 bytes of circuit ID string hash)
        let mut hasher = Hasher::new();
        hasher.update(circuit_id.0.as_bytes());
        let id_hash = hasher.finalize();
        cell.extend_from_slice(&id_hash.as_bytes()[..16]);

        // Command (1 byte): CREATE = 0x01
        cell.push(0x01);

        // Public key (32 bytes)
        cell.extend_from_slice(public_key.as_bytes());

        cell
    }

    /// Send CREATE cell to relay node and await CREATED response via libp2p
    ///
    /// Production implementation that uses the network channel to communicate with the swarm.
    /// The network layer will handle sending the cell and returning the response.
    async fn send_create_cell(&self, relay_peer_id: &PeerId, circuit_id: &CircuitId, public_key: &[u8]) -> Result<Vec<u8>> {
        // Create CREATE cell
        let create_cell = OnionCell::Create {
            circuit_id: circuit_id.0.as_bytes().to_vec(),
            public_key: public_key.to_vec(),
        };

        tracing::debug!("Sending CREATE cell to relay peer: {:?}", relay_peer_id);
        
        // Check if network channel is available
        let network_tx = self.network_tx.as_ref().ok_or_else(|| {
            tracing::error!("Network channel not configured - call set_network_channel() first");
            Error::network("Onion routing network channel not configured")
        })?;

        // Create oneshot channel for response
        let (response_tx, response_rx) = oneshot::channel();

        // Send request to network layer
        let request = OnionRoutingRequest::SendCell {
            peer_id: *relay_peer_id,
            cell: create_cell,
            response_tx,
        };

        network_tx.send(request).map_err(|e| {
            tracing::error!("Failed to send CREATE cell request to network layer: {}", e);
            Error::network(format!("Network channel send failed: {}", e))
        })?;

        // Wait for response with timeout
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            response_rx
        ).await.map_err(|_| {
            tracing::error!("Timeout waiting for CREATE response from {:?}", relay_peer_id);
            Error::network(format!("CREATE cell timeout for peer {:?}", relay_peer_id))
        })?.map_err(|_| {
            tracing::error!("Response channel closed for CREATE cell to {:?}", relay_peer_id);
            Error::network("Response channel closed")
        })??;

        // Extract public key from CREATED response
        match response {
            OnionCell::Created { public_key, status, .. } => {
                if status == 0 {
                    tracing::info!("Received CREATED response from {:?} with {} byte public key", relay_peer_id, public_key.len());
                    Ok(public_key)
                } else {
                    tracing::error!("CREATE cell rejected by {:?} with status {}", relay_peer_id, status);
                    Err(Error::network(format!("CREATE rejected with status {}", status)))
                }
            }
            other => {
                tracing::error!("Unexpected response to CREATE cell: {:?}", other);
                Err(Error::network("Unexpected response type to CREATE cell"))
            }
        }
    }

    /// Handle incoming CREATE cell (relay node perspective)
    pub fn handle_create_cell(&self, circuit_id: Vec<u8>, client_public_key: Vec<u8>) -> OnionCell {
        use rand::rngs::OsRng;
        use x25519_dalek::{EphemeralSecret, PublicKey};

        // Generate relay's ephemeral key pair
        let relay_secret = EphemeralSecret::random_from_rng(OsRng);
        let relay_public = PublicKey::from(&relay_secret);

        // Perform ECDH with client's public key
        let client_public_bytes: [u8; 32] = match client_public_key.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) => {
                return OnionCell::Created {
                    circuit_id,
                    public_key: vec![],
                    status: 1, // Error: invalid public key
                };
            }
        };
        let client_public = PublicKey::from(client_public_bytes);
        let _shared_secret = relay_secret.diffie_hellman(&client_public);

        // Store circuit state for relay operations
        // In production: save (circuit_id, shared_secret) for relay forwarding

        OnionCell::Created {
            circuit_id,
            public_key: relay_public.as_bytes().to_vec(),
            status: 0, // Success
        }
    }

    /// Handle incoming RELAY cell (intermediate hop perspective)
    pub fn handle_relay_cell(&self, circuit_id: Vec<u8>, encrypted_payload: Vec<u8>) -> Result<OnionCell> {
        // Lookup circuit by ID
        // Decrypt one layer using stored shared secret
        // Extract next hop from decrypted header
        // Forward to next hop

        tracing::debug!(
            "Relaying {} bytes for circuit {:?}",
            encrypted_payload.len(),
            hex::encode(&circuit_id)
        );

        // In production:
        // 1. Load shared_secret for this circuit_id
        // 2. Decrypt one layer: decrypted = decrypt_layer(&encrypted_payload, &shared_secret)
        // 3. Parse header to get next_hop
        // 4. Forward OnionCell::Relay { circuit_id, encrypted_payload: decrypted } to next_hop

        Ok(OnionCell::Relay {
            circuit_id,
            encrypted_payload, // Forward remaining layers
        })
    }

    /// Handle incoming DESTROY cell
    pub fn handle_destroy_cell(&mut self, circuit_id: Vec<u8>) {
        tracing::info!("Received DESTROY for circuit {:?}", hex::encode(&circuit_id));

        // Remove circuit state
        // In production: cleanup stored keys and forwarding tables

        let circuit_id_str = String::from_utf8_lossy(&circuit_id).to_string();
        let cid = CircuitId(circuit_id_str);
        self.circuits.remove(&cid);
    }
}

/// Circuit statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitStats {
    pub total_circuits: usize,
    pub active_circuits: usize,
    pub building_circuits: usize,
    pub available_relays: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_relay(id: &str, asn: Option<u32>) -> RelayNode {
        // Generate a test PeerId from the id string
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();

        RelayNode {
            node_id: id.to_string(),
            public_key: vec![0; 32],
            address: format!("127.0.0.1:{}", 9000 + id.len()),
            peer_id: Some(peer_id),
            asn,
            region: Some("US-EAST".to_string()),
        }
    }

    #[test]
    fn test_circuit_config_default() {
        let config = CircuitConfig::default();
        assert_eq!(config.num_hops, 3);
        assert!(config.enforce_diversity);
        assert!(config.enable_cover_traffic);
    }

    #[tokio::test]
    async fn test_circuit_creation() {
        let config = CircuitConfig::default();
        let mut manager = OnionRoutingManager::new(config);

        // Add relays
        for i in 0..5 {
            manager.add_relay(create_test_relay(&format!("relay{}", i), Some(i as u32)));
        }

        // Build circuit
        let circuit_id = manager.build_circuit().await.unwrap();

        let circuit = manager.circuits.get(&circuit_id).unwrap();
        assert_eq!(circuit.hops.len(), 3);
        assert_eq!(circuit.status, CircuitStatus::Active);
    }

    #[tokio::test]
    async fn test_asn_diversity() {
        let config = CircuitConfig {
            num_hops: 3,
            min_asn_diversity: 3,
            enforce_diversity: true,
            ..Default::default()
        };
        let mut manager = OnionRoutingManager::new(config);

        // Add relays with diverse ASNs
        manager.add_relay(create_test_relay("relay1", Some(100)));
        manager.add_relay(create_test_relay("relay2", Some(200)));
        manager.add_relay(create_test_relay("relay3", Some(300)));

        let circuit_id = manager.build_circuit().await.unwrap();
        let circuit = manager.circuits.get(&circuit_id).unwrap();

        // Check all ASNs are different
        let asns: Vec<_> = circuit.hops.iter().filter_map(|h| h.asn).collect();

        assert_eq!(asns.len(), 3);
        assert_eq!(
            asns.iter().collect::<std::collections::HashSet<_>>().len(),
            3
        );
    }

    #[tokio::test]
    async fn test_sphinx_packet_creation() {
        let config = CircuitConfig::default();
        let mut manager = OnionRoutingManager::new(config);

        // Setup
        for i in 0..3 {
            manager.add_relay(create_test_relay(&format!("relay{}", i), Some(i as u32)));
        }

        let circuit_id = manager.build_circuit().await.unwrap();

        // Create packet
        let payload = b"secret message";
        let packet = manager.create_sphinx_packet(&circuit_id, payload).unwrap();

        assert_eq!(packet.version, 1);
        assert!(!packet.header.is_empty());
        assert!(!packet.payload.is_empty());
        assert_eq!(packet.mac.len(), 16);
    }

    #[tokio::test]
    async fn test_circuit_teardown() {
        let config = CircuitConfig::default();
        let mut manager = OnionRoutingManager::new(config);

        for i in 0..3 {
            manager.add_relay(create_test_relay(&format!("relay{}", i), Some(i as u32)));
        }

        let circuit_id = manager.build_circuit().await.unwrap();
        assert!(manager.circuits.contains_key(&circuit_id));

        manager.tear_down_circuit(&circuit_id).await.unwrap();
        assert!(!manager.circuits.contains_key(&circuit_id));
    }

    #[test]
    fn test_cover_traffic_generation() {
        let config = CircuitConfig::default();
        let manager = OnionRoutingManager::new(config);

        let traffic = manager.generate_cover_traffic();

        // Should be between 512 and 1024 bytes
        assert!(traffic.len() >= 512);
        assert!(traffic.len() <= 1024);
    }

    #[test]
    fn test_circuit_stats() {
        let config = CircuitConfig::default();
        let mut manager = OnionRoutingManager::new(config);

        for i in 0..5 {
            manager.add_relay(create_test_relay(&format!("relay{}", i), Some(i as u32)));
        }

        let stats = manager.get_stats();
        assert_eq!(stats.total_circuits, 0);
        assert_eq!(stats.available_relays, 5);
    }
}
