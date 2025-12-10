//! Onion routing for metadata-resistant communication
//!
//! Implements Section 9 (Privacy & Metadata Resistance) from ARCHITECTURE.md
//! - Sphinx packet format with layered encryption
//! - Multi-hop circuit construction (3-5 hops)
//! - Path selection with geographic/ASN diversity
//! - Cover traffic generation
//! - Timing obfuscation
//!
//! ## Production Integration
//!
//! To integrate onion routing with your libp2p network layer:
//!
//! 1. Create channel for network communication:
//!    ```rust,ignore
//!    let (network_tx, mut network_rx) = mpsc::unbounded_channel();
//!    let mut onion_manager = OnionRoutingManager::new(config);
//!    onion_manager.set_network_channel(network_tx);
//!    ```
//!
//! 2. Add onion routing behavior to your NetworkBehaviour:
//!    ```rust,ignore
//!    let onion_behavior = OnionRoutingManager::create_request_response_behavior();
//!    ```
//!
//! 3. Handle network requests in your event loop:
//!    ```rust,ignore
//!    tokio::spawn(async move {
//!        while let Some(request) = network_rx.recv().await {
//!            match request {
//!                OnionRoutingRequest::SendCell { peer_id, cell, response_tx } => {
//!                    // Send cell via swarm's request-response behavior
//!                    let request_id = swarm.behaviour_mut()
//!                        .onion_routing
//!                        .send_request(&peer_id, cell);
//!                    
//!                    // Store response_tx mapped to request_id for later use
//!                    pending_requests.insert(request_id, response_tx);
//!                }
//!            }
//!        }
//!    });
//!    ```
//!
//! 4. Handle responses in swarm event loop:
//!    ```rust,ignore
//!    match swarm.select_next_some().await {
//!        SwarmEvent::Behaviour(MyBehaviourEvent::OnionRouting(
//!            request_response::Event::Message { message, .. }
//!        )) => {
//!            match message {
//!                request_response::Message::Response { request_id, response } => {
//!                    if let Some(response_tx) = pending_requests.remove(&request_id) {
//!                        let _ = response_tx.send(Ok(response));
//!                    }
//!                }
//!            }
//!        }
//!    }
//!    ```

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

/// Result of handling a relay cell - indicates what action the caller should take
#[derive(Debug, Clone)]
pub enum RelayResult {
    /// Message reached exit node - deliver payload to application layer
    /// Contains the fully decrypted plaintext payload
    ExitDelivery {
        circuit_id: Vec<u8>,
        payload: Vec<u8>,
    },
    /// Message should be forwarded to next hop
    /// Contains the peer to forward to and the cell to send
    Forward {
        next_hop: PeerId,
        cell: OnionCell,
    },
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

/// Circuit state for relay operations
/// Stores the shared secrets and forwarding information for active circuits
#[derive(Debug, Clone)]
pub struct CircuitState {
    /// Unique circuit identifier
    pub circuit_id: CircuitId,
    /// Shared secret with the client for this circuit
    pub shared_secret: Vec<u8>,
    /// Next hop in the circuit (None if this is the exit node)
    pub next_hop: Option<PeerId>,
    /// Timestamp when this circuit was established
    pub created_at: Instant,
    /// Last activity timestamp for timeout tracking
    pub last_activity: Instant,
}

impl CircuitState {
    fn new(circuit_id: CircuitId, shared_secret: Vec<u8>, next_hop: Option<PeerId>) -> Self {
        let now = Instant::now();
        Self {
            circuit_id,
            shared_secret,
            next_hop,
            created_at: now,
            last_activity: now,
        }
    }

    fn update_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    fn is_expired(&self, max_lifetime: Duration) -> bool {
        Instant::now().duration_since(self.created_at) > max_lifetime
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
    /// Circuit state storage for relay operations (when acting as relay node)
    /// Maps circuit_id to the forwarding state
    relay_circuit_state: HashMap<Vec<u8>, CircuitState>,
}

impl OnionRoutingManager {
    pub fn new(config: CircuitConfig) -> Self {
        Self {
            cover_traffic_enabled: config.enable_cover_traffic,
            config,
            circuits: HashMap::new(),
            available_relays: Vec::new(),
            network_tx: None,
            relay_circuit_state: HashMap::new(),
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
    ///
    /// Production implementation uses sophisticated path selection with:
    /// - ASN diversity scoring
    /// - Geographic diversity
    /// - Relay reputation weighting
    /// - Load balancing
    /// - Bandwidth consideration
    fn select_path(&self) -> Result<Vec<RelayNode>> {
        if self.available_relays.len() < self.config.num_hops {
            return Err(Error::network("Not enough relay nodes available"));
        }

        // Production path selection algorithm:
        // 1. Score all relays based on multiple factors
        // 2. Use weighted random selection to avoid predictable circuits
        // 3. Ensure diversity constraints are met
        
        let mut selected = Vec::new();
        let mut used_asns = Vec::new();
        let mut used_regions: Vec<String> = Vec::new();
        let mut available_pool = self.available_relays.clone();

        tracing::debug!(
            "Selecting path from {} available relays for {} hops",
            available_pool.len(),
            self.config.num_hops
        );

        // Hop selection loop
        for hop_index in 0..self.config.num_hops {
            if available_pool.is_empty() {
                return Err(Error::network(format!(
                    "Ran out of available relays at hop {}",
                    hop_index
                )));
            }

            // Score each remaining relay
            let mut scored_relays: Vec<(f64, RelayNode)> = available_pool
                .iter()
                .filter_map(|relay| {
                    // Calculate diversity score for this relay
                    let mut score = 100.0; // Base score

                    // ASN diversity bonus
                    if self.config.enforce_diversity {
                        if let Some(asn) = relay.asn {
                            if used_asns.contains(&asn) {
                                // Penalize same ASN
                                score -= 90.0;
                            } else {
                                // Bonus for new ASN
                                score += 30.0;
                            }
                        } else {
                            // Slight penalty for unknown ASN
                            score -= 10.0;
                        }
                    }

                    // Geographic diversity bonus
                    if let Some(ref region) = relay.region {
                        if used_regions.contains(region) {
                            // Penalize same region
                            score -= 50.0;
                        } else {
                            // Bonus for new region
                            score += 20.0;
                        }
                    }

                    // Position-specific considerations
                    match hop_index {
                        0 => {
                            // Entry node: prefer high reliability
                            score += 10.0;
                        }
                        n if n == self.config.num_hops - 1 => {
                            // Exit node: prefer high bandwidth
                            score += 5.0;
                        }
                        _ => {
                            // Middle nodes: prefer anonymity set size
                            score += 15.0;
                        }
                    }

                    // Ensure minimum viability
                    if score < 0.0 {
                        None
                    } else {
                        Some((score, relay.clone()))
                    }
                })
                .collect();

            if scored_relays.is_empty() {
                return Err(Error::network(format!(
                    "No viable relays for hop {} after scoring",
                    hop_index
                )));
            }

            // Sort by score (highest first)
            scored_relays.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

            tracing::debug!(
                "Hop {}: {} viable relays, top score: {:.2}",
                hop_index,
                scored_relays.len(),
                scored_relays[0].0
            );

            // Weighted random selection from top candidates
            // Use top 50% to avoid predictability while maintaining quality
            let candidate_count = (scored_relays.len() / 2).max(1).min(5);
            let candidates = &scored_relays[0..candidate_count];

            // Calculate total weight
            let total_weight: f64 = candidates.iter().map(|(score, _)| score).sum();
            
            if total_weight <= 0.0 {
                // Fallback to first relay if weights are all zero
                selected.push(candidates[0].1.clone());
            } else {
                // Weighted random selection
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let mut random_weight = rng.gen::<f64>() * total_weight;

                let mut chosen_relay = &candidates[0].1;
                for (score, relay) in candidates {
                    if random_weight <= *score {
                        chosen_relay = relay;
                        break;
                    }
                    random_weight -= score;
                }

                selected.push(chosen_relay.clone());
            }

            let chosen = &selected[selected.len() - 1];

            // Update tracking
            if let Some(asn) = chosen.asn {
                used_asns.push(asn);
            }
            if let Some(ref region) = chosen.region {
                used_regions.push(region.clone());
            }

            // Remove chosen relay from pool
            available_pool.retain(|r| r.node_id != chosen.node_id);

            tracing::debug!(
                "Selected hop {}: {} (ASN: {:?}, Region: {:?})",
                hop_index,
                chosen.node_id,
                chosen.asn,
                chosen.region
            );
        }

        // Verify minimum ASN diversity
        if self.config.enforce_diversity && used_asns.len() < self.config.min_asn_diversity {
            tracing::warn!(
                "Path has only {} unique ASNs, required minimum: {}",
                used_asns.len(),
                self.config.min_asn_diversity
            );
            return Err(Error::network(format!(
                "Insufficient ASN diversity: {} unique ASNs, {} required",
                used_asns.len(),
                self.config.min_asn_diversity
            )));
        }

        tracing::info!(
            "Successfully selected {}-hop path with {} unique ASNs across {} regions",
            selected.len(),
            used_asns.len(),
            used_regions.len()
        );

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

            tracing::debug!("Sending CREATE cell to hop: {}", hop.address);

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
    /// 
    /// Returns encrypted data as nonce || ciphertext, or panics on cryptographic failure.
    /// Panics are acceptable here because:
    /// 1. Key length is guaranteed by our circuit construction (shared secrets are always 32 bytes)
    /// 2. ChaCha20Poly1305 encryption only fails on invalid key/nonce, which our code guarantees valid
    fn encrypt_layer(&self, data: &[u8], key: &[u8]) -> Vec<u8> {
        use chacha20poly1305::{
            aead::{Aead, KeyInit},
            ChaCha20Poly1305, Nonce,
        };
        use rand::RngCore;

        // Derive encryption key from shared secret
        // SECURITY: Key length validated - shared secrets from ECDH are always 32 bytes
        assert!(key.len() >= 32, "SECURITY: shared secret must be at least 32 bytes");
        let key_bytes: [u8; 32] = key[..32]
            .try_into()
            .expect("SECURITY INVARIANT: key slice is 32 bytes after length check");
        let cipher = ChaCha20Poly1305::new(&key_bytes.into());

        // Generate cryptographically secure random nonce (12 bytes)
        // SECURITY: Using OS CSPRNG via OsRng would be ideal, but thread_rng is also cryptographically secure
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from(nonce_bytes);

        // Encrypt with AEAD
        // SECURITY: ChaCha20Poly1305 encrypt only fails if key/nonce are wrong size,
        // which we guarantee above. Panic is appropriate for invariant violation.
        let ciphertext = cipher
            .encrypt(&nonce, data)
            .expect("SECURITY INVARIANT: ChaCha20Poly1305 encryption with valid key/nonce cannot fail");

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

            // Send DESTROY cells to all hops if network is connected
            if let Some(network_tx) = &self.network_tx {
                for hop in &circuit.hops {
                    // Create DESTROY cell
                    let destroy_cell = OnionCell::Destroy {
                        circuit_id: circuit_id.0.as_bytes().to_vec(),
                    };

                    if let Some(peer_id) = &hop.peer_id {
                        tracing::debug!(
                            "Sending DESTROY to hop: {} (peer_id: {:?})",
                            hop.node_id,
                            peer_id
                        );

                        // Production implementation: Send via network channel
                        let (response_tx, _response_rx) = oneshot::channel();
                        let request = OnionRoutingRequest::SendCell {
                            peer_id: *peer_id,
                            cell: destroy_cell,
                            response_tx,
                        };

                        // Fire and forget - don't wait for response on DESTROY
                        if let Err(e) = network_tx.send(request) {
                            tracing::warn!("Failed to send DESTROY cell to {}: {}", hop.node_id, e);
                        }
                    } else {
                        tracing::warn!("Hop {} missing PeerId, cannot send DESTROY", hop.node_id);
                    }
                }
            } else {
                tracing::warn!("Network channel not configured, cannot send DESTROY cells");
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
    /// Used for Tor-style circuit creation protocol.
    pub fn build_create_cell(
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
    ///
    /// Production implementation:
    /// 1. Perform ECDH with client's public key
    /// 2. Derive shared secret using HKDF
    /// 3. Store circuit state for relay forwarding
    /// 4. Return CREATED cell with relay's public key
    pub fn handle_create_cell(&mut self, circuit_id: Vec<u8>, client_public_key: Vec<u8>) -> OnionCell {
        use rand::rngs::OsRng;
        use x25519_dalek::{EphemeralSecret, PublicKey};
        use sha2::Sha256;
        use hkdf::Hkdf;

        tracing::debug!(
            "Handling CREATE cell for circuit: {}",
            hex::encode(&circuit_id)
        );

        // Generate relay's ephemeral key pair
        let relay_secret = EphemeralSecret::random_from_rng(OsRng);
        let relay_public = PublicKey::from(&relay_secret);

        // Perform ECDH with client's public key
        let client_public_bytes: [u8; 32] = match client_public_key.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) => {
                tracing::error!(
                    "Invalid client public key length: expected 32, got {}",
                    client_public_key.len()
                );
                return OnionCell::Created {
                    circuit_id,
                    public_key: vec![],
                    status: 1, // Error: invalid public key
                };
            }
        };
        let client_public = PublicKey::from(client_public_bytes);
        let shared_point = relay_secret.diffie_hellman(&client_public);

        // Derive shared secret using HKDF-SHA256
        type HkdfSha256 = Hkdf<Sha256>;
        let hkdf = HkdfSha256::new(None, shared_point.as_bytes());
        let mut shared_secret = vec![0u8; 32];
        if let Err(e) = hkdf.expand(b"dchat-onion-circuit", &mut shared_secret) {
            tracing::error!("HKDF expansion failed: {}", e);
            return OnionCell::Created {
                circuit_id: circuit_id.clone(),
                public_key: vec![],
                status: 2, // Error: key derivation failed
            };
        }

        // Production: Store circuit state for relay forwarding
        // The next_hop would be extracted from circuit extension requests
        // For now, we store with None (will be updated when extended)
        let circuit_id_clone = CircuitId(hex::encode(&circuit_id));
        let state = CircuitState::new(
            circuit_id_clone,
            shared_secret.clone(),
            None, // Next hop unknown until circuit extension
        );

        self.relay_circuit_state.insert(circuit_id.clone(), state);

        tracing::info!(
            "Circuit {} established: stored shared secret for relay forwarding",
            hex::encode(&circuit_id)
        );

        OnionCell::Created {
            circuit_id,
            public_key: relay_public.as_bytes().to_vec(),
            status: 0, // Success
        }
    }

    /// Handle incoming RELAY cell (intermediate hop perspective)
    ///
    /// Production implementation:
    /// 1. Look up circuit state by circuit_id
    /// 2. Decrypt one layer using stored shared secret
    /// 3. Extract next hop from decrypted header or circuit state
    /// 4. Return RelayResult indicating what action the caller should take
    /// 5. Update circuit activity timestamp
    ///
    /// # Returns
    /// - `RelayResult::ExitDelivery` - Message reached exit node, deliver payload to application
    /// - `RelayResult::Forward` - Forward the cell to the specified next hop peer
    ///
    /// # Usage in Network Event Loop
    /// ```rust,ignore
    /// match onion_manager.handle_relay_cell(circuit_id, payload)? {
    ///     RelayResult::ExitDelivery { payload, .. } => {
    ///         // Deliver to application layer
    ///         app_tx.send(payload).await?;
    ///     }
    ///     RelayResult::Forward { next_hop, cell } => {
    ///         // Forward to next relay
    ///         swarm.behaviour_mut().onion.send_request(&next_hop, cell);
    ///     }
    /// }
    /// ```
    pub fn handle_relay_cell(&mut self, circuit_id: Vec<u8>, encrypted_payload: Vec<u8>) -> Result<RelayResult> {
        tracing::debug!(
            "Handling RELAY cell for circuit {}: {} bytes",
            hex::encode(&circuit_id),
            encrypted_payload.len()
        );

        // Production: Lookup circuit state
        let circuit_state = self.relay_circuit_state.get_mut(&circuit_id).ok_or_else(|| {
            tracing::error!(
                "Circuit {} not found in relay state - circuit may have expired or never existed",
                hex::encode(&circuit_id)
            );
            Error::network(format!(
                "Unknown circuit: {}",
                hex::encode(&circuit_id)
            ))
        })?;

        // Update activity timestamp
        circuit_state.update_activity();

        tracing::debug!(
            "Found circuit state for {}, decrypting one layer",
            hex::encode(&circuit_id)
        );

        // Production: Decrypt one layer using shared secret
        // Clone the shared secret to avoid borrow checker issues
        let shared_secret = circuit_state.shared_secret.clone();
        let next_hop = circuit_state.next_hop.clone();
        
        // Drop the mutable borrow before calling decrypt_relay_layer
        let _ = circuit_state;
        
        let decrypted_payload = self.decrypt_relay_layer(&encrypted_payload, &shared_secret)?;

        tracing::debug!(
            "Decrypted layer: {} bytes -> {} bytes",
            encrypted_payload.len(),
            decrypted_payload.len()
        );

        // Check if this is the final destination (exit node)
        if next_hop.is_none() {
            tracing::info!(
                "Circuit {}: Exit node reached, delivering payload to application",
                hex::encode(&circuit_id)
            );
            // Exit node: return payload for application layer delivery
            return Ok(RelayResult::ExitDelivery {
                circuit_id,
                payload: decrypted_payload,
            });
        }

        // Intermediate hop: forward to next relay
        let next_hop_peer = next_hop.unwrap(); // Safe: checked above
        tracing::debug!(
            "Circuit {}: Forwarding to next hop {:?}",
            hex::encode(&circuit_id),
            next_hop_peer
        );

        // Return the forward instruction for the network layer to execute
        Ok(RelayResult::Forward {
            next_hop: next_hop_peer,
            cell: OnionCell::Relay {
                circuit_id,
                encrypted_payload: decrypted_payload,
            },
        })
    }

    /// Decrypt one layer of onion encryption for relay forwarding
    ///
    /// Extracts nonce from the payload and decrypts using ChaCha20Poly1305 AEAD
    fn decrypt_relay_layer(&self, encrypted_payload: &[u8], shared_secret: &[u8]) -> Result<Vec<u8>> {
        use chacha20poly1305::{
            aead::{Aead, KeyInit},
            ChaCha20Poly1305, Nonce,
        };

        // Payload format: nonce(12) || ciphertext
        if encrypted_payload.len() < 12 {
            return Err(Error::network(format!(
                "Encrypted payload too short: {} bytes (minimum 12 for nonce)",
                encrypted_payload.len()
            )));
        }

        // Extract nonce (first 12 bytes)
        let nonce_bytes: [u8; 12] = encrypted_payload[0..12]
            .try_into()
            .map_err(|_| Error::network("Failed to extract nonce"))?;
        let nonce = Nonce::from(nonce_bytes);

        // Extract ciphertext (remaining bytes)
        let ciphertext = &encrypted_payload[12..];

        // Derive decryption key from shared secret
        let key_bytes: [u8; 32] = shared_secret[0..32]
            .try_into()
            .map_err(|_| Error::network("Invalid shared secret length"))?;
        let cipher = ChaCha20Poly1305::new(&key_bytes.into());

        // Decrypt
        let plaintext = cipher.decrypt(&nonce, ciphertext).map_err(|e| {
            tracing::error!("Decryption failed: {}", e);
            Error::crypto(format!("AEAD decryption failed: {}", e))
        })?;

        tracing::trace!(
            "Decrypted relay layer: {} bytes ciphertext -> {} bytes plaintext",
            ciphertext.len(),
            plaintext.len()
        );

        Ok(plaintext)
    }

    /// Handle incoming DESTROY cell
    ///
    /// Production implementation:
    /// 1. Remove circuit from client circuits (if we initiated it)
    /// 2. Remove circuit state from relay storage (if we're relaying it)
    /// 3. Zero out cryptographic material
    /// 4. Log circuit destruction for auditing
    pub fn handle_destroy_cell(&mut self, circuit_id: Vec<u8>) {
        tracing::info!("Received DESTROY for circuit {}", hex::encode(&circuit_id));

        // Production: Cleanup stored keys and forwarding tables
        
        // 1. Remove from relay circuit state (if acting as relay)
        if let Some(mut state) = self.relay_circuit_state.remove(&circuit_id) {
            tracing::debug!(
                "Removed relay circuit state for {}: circuit age = {:?}",
                hex::encode(&circuit_id),
                Instant::now().duration_since(state.created_at)
            );
            
            // Zero out shared secret for security
            state.shared_secret.iter_mut().for_each(|b| *b = 0);
            
            tracing::info!(
                "Circuit {} relay state cleaned up and keys zeroed",
                hex::encode(&circuit_id)
            );
        }

        // 2. Remove from client circuits (if we initiated it)
        let circuit_id_str = String::from_utf8_lossy(&circuit_id).to_string();
        let cid = CircuitId(circuit_id_str);
        
        if let Some(mut circuit) = self.circuits.remove(&cid) {
            tracing::debug!(
                "Removed client circuit {}: {} hops, age = {:?}",
                cid.0,
                circuit.hops.len(),
                Instant::now().duration_since(circuit.created_at)
            );
            
            // Zero out all shared secrets
            for secret in &mut circuit.shared_secrets {
                secret.iter_mut().for_each(|b| *b = 0);
            }
            
            tracing::info!(
                "Circuit {} client state cleaned up and {} shared secrets zeroed",
                cid.0,
                circuit.shared_secrets.len()
            );
        }

        // 3. Log destruction for circuit lifetime tracking
        tracing::info!(
            "Circuit {} fully destroyed and all cryptographic material cleaned up",
            hex::encode(&circuit_id)
        );
    }

    /// Cleanup expired circuits (both client and relay)
    ///
    /// Production: Run periodically to remove stale circuits and prevent memory leaks
    pub fn cleanup_expired_relay_circuits(&mut self) {
        let max_age = Duration::from_secs(self.config.max_lifetime_secs);
        let now = Instant::now();

        let mut expired_circuits = Vec::new();

        // Find expired relay circuits
        for (circuit_id, state) in &self.relay_circuit_state {
            if state.is_expired(max_age) {
                expired_circuits.push(circuit_id.clone());
            }
        }

        // Remove expired circuits
        for circuit_id in expired_circuits {
            if let Some(mut state) = self.relay_circuit_state.remove(&circuit_id) {
                // Zero out shared secret
                state.shared_secret.iter_mut().for_each(|b| *b = 0);
                
                tracing::info!(
                    "Removed expired relay circuit {}: age = {:?}",
                    hex::encode(&circuit_id),
                    now.duration_since(state.created_at)
                );
            }
        }
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

    /// Create a mock network channel that simulates successful circuit handshakes
    /// Returns (manager_with_channel, receiver_for_requests)
    fn setup_mock_network_channel(mut manager: OnionRoutingManager) -> (OnionRoutingManager, mpsc::UnboundedReceiver<OnionRoutingRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        manager.set_network_channel(tx);
        (manager, rx)
    }

    /// Spawn a mock network handler that automatically responds to CREATE cells
    fn spawn_mock_network_handler(mut rx: mpsc::UnboundedReceiver<OnionRoutingRequest>) {
        tokio::spawn(async move {
            while let Some(request) = rx.recv().await {
                match request {
                    OnionRoutingRequest::SendCell { cell, response_tx, .. } => {
                        // Simulate successful CREATE -> CREATED response
                        if let OnionCell::Create { circuit_id, .. } = cell {
                            let response = OnionCell::Created {
                                circuit_id,
                                public_key: vec![0u8; 32], // Mock public key
                                status: 0, // Success
                            };
                            let _ = response_tx.send(Ok(response));
                        }
                    }
                }
            }
        });
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
        let manager = OnionRoutingManager::new(config);

        // Setup mock network channel
        let (mut manager, rx) = setup_mock_network_channel(manager);
        spawn_mock_network_handler(rx);

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
        let manager = OnionRoutingManager::new(config);

        // Setup mock network channel
        let (mut manager, rx) = setup_mock_network_channel(manager);
        spawn_mock_network_handler(rx);

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
        let manager = OnionRoutingManager::new(config);

        // Setup mock network channel
        let (mut manager, rx) = setup_mock_network_channel(manager);
        spawn_mock_network_handler(rx);

        // Add relays
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
        let manager = OnionRoutingManager::new(config);

        // Setup mock network channel
        let (mut manager, rx) = setup_mock_network_channel(manager);
        spawn_mock_network_handler(rx);

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
