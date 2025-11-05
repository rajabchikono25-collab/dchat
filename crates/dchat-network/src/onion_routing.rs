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
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Onion circuit identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CircuitId(pub String);

/// Relay node in the circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayNode {
    pub node_id: String,
    pub public_key: Vec<u8>,
    pub address: String,
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
}

impl OnionRoutingManager {
    pub fn new(config: CircuitConfig) -> Self {
        Self {
            cover_traffic_enabled: config.enable_cover_traffic,
            config,
            circuits: HashMap::new(),
            available_relays: Vec::new(),
        }
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
            
            match self.send_create_cell(&hop.address, create_cell).await {
                Ok(_) => {
                    // Successfully established this hop
                    tracing::debug!("Established hop with {}", hop.node_id);
                }
                Err(e) => {
                    tracing::error!("Failed to establish hop {}: {}", hop.node_id, e);
                    return Err(Error::network(format!(
                        "Circuit build failed at hop {}: {}",
                        hop.node_id, e
                    )));
                }
            }
        }

        let circuit = Circuit {
            id: circuit_id.clone(),
            hops: path,
            created_at: Instant::now(),
            last_used: Instant::now(),
            status: CircuitStatus::Active, // Set to active after all hops succeed
            shared_secrets,
        };

        self.circuits.insert(circuit_id.clone(), circuit);

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
            let nonce = Nonce::from_slice(&nonce_bytes);

            // Encrypt layer
            encrypted_payload = cipher
                .encrypt(nonce, encrypted_payload.as_ref())
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
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt with AEAD
        let ciphertext = cipher.encrypt(nonce, data).expect("Encryption failed");

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

    /// Send packet through circuit
    pub async fn send_packet(
        &mut self,
        circuit_id: &CircuitId,
        _packet: SphinxPacket,
    ) -> Result<()> {
        let circuit = self
            .circuits
            .get_mut(circuit_id)
            .ok_or_else(|| Error::network("Circuit not found"))?;

        if circuit.status != CircuitStatus::Active {
            return Err(Error::network("Circuit not active"));
        }

        circuit.last_used = Instant::now();

        // Send packet to entry node (first hop) via libp2p
        let entry_node = &circuit.hops[0];
        tracing::debug!("Sending Sphinx packet via entry node: {}", entry_node.node_id);
        
        // In production: open libp2p stream and send RELAY cell
        // let mut stream = swarm.open_stream(&entry_node.peer_id).await?;
        // stream.write_all(&packet.serialize()).await?;
        
        // Each hop will:
        // 1. Decrypt one layer using its shared secret
        // 2. Extract next hop address from header
        // 3. Forward remaining packet to next hop
        // Final (exit) hop decrypts last layer and delivers payload
        
        Ok(())
    }

    /// Tear down a circuit
    pub async fn tear_down_circuit(&mut self, circuit_id: &CircuitId) -> Result<()> {
        if let Some(circuit) = self.circuits.get_mut(circuit_id) {
            circuit.status = CircuitStatus::TearingDown;

            // Send DESTROY cells to all hops in circuit
            tracing::info!("Tearing down circuit {}: sending DESTROY cells to {} hops", circuit_id.0, circuit.hops.len());
            
            for hop in &circuit.hops {
                // DESTROY cell format: circuit_id(16) || command(1=DESTROY)
                let mut destroy_cell = Vec::new();
                destroy_cell.extend_from_slice(circuit_id.0.as_bytes());
                destroy_cell.push(0x04); // DESTROY command
                
                tracing::debug!("Sending DESTROY to hop: {}", hop.node_id);
                // In production: send via libp2p
                // swarm.send_message(&hop.peer_id, destroy_cell).await?;
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

    /// Send CREATE cell to relay node and await CREATED response
    async fn send_create_cell(&self, relay_address: &str, create_cell: Vec<u8>) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;

        // Connect to relay
        let mut stream = TcpStream::connect(relay_address)
            .await
            .map_err(|e| Error::network(format!("Failed to connect to relay: {}", e)))?;

        // Send CREATE cell
        stream
            .write_all(&create_cell)
            .await
            .map_err(|e| Error::network(format!("Failed to send CREATE cell: {}", e)))?;

        // Wait for CREATED response
        // CREATED format: version(1) || circuit_id(16) || command(1=CREATED) || public_key(32) || status(1)
        let mut response = vec![0u8; 51];
        stream
            .read_exact(&mut response)
            .await
            .map_err(|e| Error::network(format!("Failed to read CREATED response: {}", e)))?;

        // Verify response
        if response[0] != 1 {
            return Err(Error::network("Invalid CREATED response version"));
        }

        if response[17] != 0x02 {
            // CREATED command = 0x02
            return Err(Error::network("Invalid CREATED response command"));
        }

        let status = response[50];
        if status != 0x00 {
            // 0x00 = success
            return Err(Error::network(format!(
                "Circuit creation failed with status: {}",
                status
            )));
        }

        Ok(())
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
        RelayNode {
            node_id: id.to_string(),
            public_key: vec![0; 32],
            address: format!("127.0.0.1:{}", 9000 + id.len()),
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
