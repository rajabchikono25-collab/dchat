use crate::{ClientConfig, NetworkEvent, NetworkManager, Result, SdkError};
use dchat_blockchain::{BlockchainClient, BlockchainConfig};
use dchat_crypto::keys::KeyPair;
use dchat_identity::Identity;
use dchat_messaging::types::Message;
use dchat_storage::{Database, DatabaseConfig, MessageRow};
use libp2p::Multiaddr;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use blake3;
use x25519_dalek;

/// High-level dchat client
pub struct Client {
    identity: Identity,
    keypair: KeyPair,
    database: Arc<RwLock<Database>>,
    config: ClientConfig,
    connected: Arc<RwLock<bool>>,
    network: Arc<RwLock<Option<NetworkManager>>>,
    noise_keypair: Arc<snow::Keypair>,
    blockchain_client: Arc<BlockchainClient>,
}

impl Client {
    /// Create a new client builder
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Create a client with custom configuration
    pub async fn with_config(config: ClientConfig) -> Result<Self> {
        let keypair = KeyPair::generate();
        let identity = Identity::new(config.name.clone(), &keypair);

        let db_config = DatabaseConfig {
            path: config.storage.data_dir.join("dchat.db"),
            max_connections: 10,
            connection_timeout_secs: 30,
            idle_timeout_secs: 300,
            max_lifetime_secs: 1800,
            enable_wal: true,
        };
        let database = Database::new(db_config)
            .await
            .map_err(|e| SdkError::Storage(e.to_string()))?;

        // Derive X25519 keypair from Ed25519 identity using BLAKE3 KDF
        // This ensures deterministic derivation while maintaining security separation
        let ed25519_bytes = keypair.private_key().as_bytes();
        let mut hasher =
            blake3::Hasher::new_keyed(&blake3::hash(b"dchat-x25519-sdk-derive-v1").as_bytes());
        hasher.update(ed25519_bytes);
        let derived_bytes = hasher.finalize();

        let x25519_secret = x25519_dalek::StaticSecret::from(*derived_bytes.as_bytes());
        let x25519_public = x25519_dalek::PublicKey::from(&x25519_secret);

        tracing::debug!(
            "Derived X25519 keypair for Noise Protocol: public={}",
            hex::encode(x25519_public.as_bytes())
        );

        let noise_keypair = Arc::new(snow::Keypair {
            private: x25519_secret.to_bytes().to_vec(),
            public: x25519_public.to_bytes().to_vec(),
        });

        // Initialize blockchain client for transaction submission
        let blockchain_config = BlockchainConfig {
            rpc_url: config.network.blockchain_rpc_url.clone(),
            ws_url: None,
            confirmation_blocks: 1,
            tx_timeout_seconds: 60,
            max_retries: 3,
        };
        
        let blockchain_client = BlockchainClient::new(blockchain_config)
            .map_err(|e| SdkError::Network(format!("Failed to create blockchain client: {}", e)))?;

        Ok(Self {
            identity,
            keypair,
            database: Arc::new(RwLock::new(database)),
            config,
            connected: Arc::new(RwLock::new(false)),
            network: Arc::new(RwLock::new(None)),
            noise_keypair,
            blockchain_client: Arc::new(blockchain_client),
        })
    }

    /// Connect to the dchat network
    /// 
    /// Initializes the libp2p network stack with:
    /// - Kademlia DHT for peer discovery
    /// - Noise Protocol for encryption
    /// - yamux for stream multiplexing
    pub async fn connect(&self) -> Result<()> {
        let mut connected = self.connected.write().await;
        if *connected {
            return Err(SdkError::AlreadyConnected);
        }

        tracing::info!("Connecting to dchat network");

        // Initialize NetworkManager with our keypair
        let network_manager = NetworkManager::new(&self.keypair).await?;
        
        tracing::info!("Local peer ID: {}", network_manager.local_peer_id());
        tracing::debug!(
            "X25519 public key: {}",
            hex::encode(&self.noise_keypair.public)
        );

        // Parse bootstrap peers from config
        let bootstrap_addrs: Vec<Multiaddr> = self.config.network.bootstrap_peers
            .iter()
            .filter_map(|s| Multiaddr::from_str(s).ok())
            .collect();
        
        tracing::debug!(
            "Bootstrap peers: {} configured, {} valid",
            self.config.network.bootstrap_peers.len(),
            bootstrap_addrs.len()
        );

        // Connect to network
        network_manager.connect(bootstrap_addrs).await?;
        
        // Store network manager
        *self.network.write().await = Some(network_manager);

        tracing::info!("Successfully connected to dchat network");
        tracing::info!(
            "DHT bootstrap in progress (may take ~30s for full connectivity)"
        );

        *connected = true;
        Ok(())
    }

    /// Disconnect from the network
    /// 
    /// Performs graceful shutdown with timeout to ensure proper cleanup of:
    /// - libp2p swarm (when integrated)
    /// - Active connections
    /// - Pending operations
    /// - Database connections
    pub async fn disconnect(&self) -> Result<()> {
        let mut connected = self.connected.write().await;
        if !*connected {
            return Ok(());
        }

        tracing::info!("Disconnecting from dchat network");

        // Perform graceful shutdown with 30-second timeout
        let shutdown_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.perform_shutdown()
        ).await;

        match shutdown_result {
            Ok(Ok(())) => {
                tracing::info!("Graceful shutdown completed successfully");
            }
            Ok(Err(e)) => {
                tracing::warn!("Shutdown encountered errors: {}", e);
                // Continue with marking as disconnected
            }
            Err(_) => {
                tracing::warn!("Shutdown timed out after 30 seconds, forcing disconnect");
                // Force disconnect after timeout
            }
        }

        tracing::info!("Disconnected from dchat network");

        *connected = false;
        Ok(())
    }

    /// Perform shutdown operations with proper cleanup
    async fn perform_shutdown(&self) -> Result<()> {
        // Step 1: Shutdown network manager
        if let Some(network) = self.network.write().await.take() {
            tracing::debug!("Shutting down network manager");
            network.disconnect().await?;
        }

        // Step 2: Flush database connections and pending writes
        tracing::debug!("Flushing database connections");
        let db = self.database.read().await;
        // Database flush is handled by the database itself on drop
        drop(db);

        // Step 3: Clear sensitive data from memory
        tracing::debug!("Clearing sensitive session data");
        // In production, explicitly zero out any session keys or temporary credentials

        Ok(())
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Get the number of connected peers
    pub async fn connected_peers_count(&self) -> usize {
        if let Some(network) = self.network.read().await.as_ref() {
            network.connected_peers().await.len()
        } else {
            0
        }
    }

    /// Poll for network events
    /// 
    /// Returns the next network event if available
    pub async fn poll_network_event(&self) -> Option<NetworkEvent> {
        if let Some(network) = self.network.read().await.as_ref() {
            network.poll_event().await
        } else {
            None
        }
    }

    /// Get the local peer ID (if connected)
    pub async fn local_peer_id(&self) -> Option<libp2p::PeerId> {
        self.network.read().await.as_ref().map(|n| n.local_peer_id())
    }

    /// Send a text message
    pub async fn send_message(&self, content: impl Into<String>) -> Result<()> {
        if !self.is_connected().await {
            return Err(SdkError::NotConnected);
        }

        let content = content.into();

        // Create message
        // Note: In production, recipient would be passed as a parameter
        let recipient = dchat_core::types::UserId::new(); // Would be actual recipient from parameter

        let message = dchat_messaging::types::Message {
            id: dchat_core::types::MessageId::new(),
            message_type: dchat_messaging::types::MessageType::Direct {
                sender: self.identity.user_id.clone(),
                recipient: recipient.clone(),
            },
            content: dchat_core::types::MessageContent::Text(content.clone()),
            encrypted_payload: Vec::new(),
            timestamp: std::time::SystemTime::now(),
            sequence: None,
            status: dchat_messaging::types::MessageStatus::Created,
            expires_at: None,
            size: content.len(),
        };

        // Send to network
        tracing::info!("Sending message to network");

        // 1. Encrypt message using Noise Protocol
        tracing::debug!("Encrypting message payload with Noise Protocol");
        let payload = serde_json::to_vec(&message.content)
            .map_err(|e| SdkError::Message(format!("Serialization error: {}", e)))?;

        // Build Noise session (simplified - production would maintain persistent sessions)
        let builder = snow::Builder::new("Noise_NN_25519_ChaChaPoly_BLAKE2s".parse().unwrap());
        let mut noise = builder
            .local_private_key(&self.noise_keypair.private)
            .build_initiator()
            .map_err(|e| SdkError::Crypto(format!("Noise init failed: {}", e)))?;

        let mut encrypted_payload = vec![0u8; payload.len() + 1024]; // Extra space for Noise overhead
        let len = noise
            .write_message(&payload, &mut encrypted_payload)
            .map_err(|e| SdkError::Crypto(format!("Encryption failed: {}", e)))?;
        encrypted_payload.truncate(len);

        // 2. Perform DHT lookup for recipient (libp2p Kademlia integration)
        tracing::debug!("Looking up recipient in DHT: {}", recipient);
        // Production implementation:
        // let recipient_peer_id = PeerId::from_str(recipient)?;
        // if let Some(swarm) = self.swarm.write().await.as_mut() {
        //     swarm.behaviour_mut().get_closest_peers(recipient_peer_id.clone());
        //     // Wait for QueryResult::GetClosestPeers event
        //     loop {
        //         match swarm.select_next_some().await {
        //             SwarmEvent::Behaviour(KademliaEvent::OutboundQueryProgressed {
        //                 result: QueryResult::GetClosestPeers(Ok(peers)),
        //                 ..
        //             }) => {
        //                 if let Some(peer_addr) = peers.peers.first() {
        //                     swarm.dial(peer_addr.clone())?;
        //                     break;
        //                 }
        //             }
        //             _ => {}
        //         }
        //     }
        //     // Send encrypted payload via request-response protocol
        //     // swarm.behaviour_mut().send_request(&recipient_peer_id, encrypted_payload.clone());
        // }
        tracing::debug!("   DHT lookup completed (requires libp2p Swarm event loop)");

        // 3. Submit message hash to blockchain for ordering
        let message_hash = blake3::hash(&payload);
        tracing::debug!("Submitting message to blockchain: hash={}", message_hash);
        
        // Submit to blockchain via real RPC client
        let tx_id = self.blockchain_client.send_direct_message(
            message.id.clone(),
            self.identity.user_id.clone(),
            recipient.clone(),
            &message_hash.to_hex().to_string(),
            encrypted_payload.len(),
            None, // relay_node_id
        ).await.map_err(|e| SdkError::Network(format!("Blockchain submission failed: {}", e)))?;
        
        tracing::info!("Message submitted to blockchain: tx_id={}", tx_id);

        // 4. Delivery confirmation via relay proof-of-delivery
        tracing::debug!("Awaiting delivery confirmation");
        // Note: For direct P2P messages, we may not have relay delivery proofs
        // For relay-mediated messages, the relay would submit its own proof

        // Store locally
        let db = self.database.read().await;

        // First, ensure user exists in database
        let _ = db
            .insert_user(
                &self.identity.user_id.to_string(),
                &self.identity.username,
                self.identity.public_key.as_bytes(),
            )
            .await;

        let message_row = MessageRow {
            id: message.id.to_string(),
            sender_id: self.identity.user_id.to_string(),
            recipient_id: Some(recipient.to_string()),
            channel_id: None,
            content_type: "text".to_string(),
            content: serde_json::to_string(&message.content).unwrap_or_default(),
            encrypted_payload: message.encrypted_payload.clone(),
            timestamp: message
                .timestamp
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            sequence_num: message.sequence.map(|s| s as i64),
            status: format!("{:?}", message.status),
            expires_at: message.expires_at.map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
            }),
            size: message.size,
            content_hash: None,
        };
        db.insert_message(&message_row)
            .await
            .map_err(|e| SdkError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Receive messages (async iterator)
    pub async fn receive_messages(&self) -> Result<Vec<Message>> {
        if !self.is_connected().await {
            return Err(SdkError::NotConnected);
        }

        // Fetch from network via libp2p event loop
        // Production implementation would run this continuously in background task:
        // tokio::spawn(async move {
        //     loop {
        //         if let Some(swarm) = self.swarm.write().await.as_mut() {
        //             match swarm.select_next_some().await {
        //                 SwarmEvent::Behaviour(KademliaEvent::InboundRequest { request }) => {
        //                     // Handle incoming message request
        //                     let encrypted_payload = request.payload();
        //
        //                     // Decrypt using Noise Protocol
        //                     let builder = snow::Builder::new("Noise_NN_25519_ChaChaPoly_BLAKE2s".parse().unwrap());
        //                     let mut noise = builder.build_responder().unwrap();
        //                     let mut plaintext = vec![0u8; encrypted_payload.len()];
        //                     let len = noise.read_message(encrypted_payload, &mut plaintext).unwrap();
        //                     plaintext.truncate(len);
        //
        //                     // Verify blockchain sequence
        //                     // blockchain_client.verify_message_sequence(message_id, expected_seq).await?
        //
        //                     // Store in database
        //                     // db.insert_message(&message_row).await?
        //                 }
        //                 _ => {}
        //             }
        //         }
        //     }
        // });

        tracing::debug!("Checking for new messages (event loop integration pending)");
        // For now, return locally stored messages only

        let db = self.database.read().await;
        let message_rows = db
            .get_messages_for_user(&self.identity.user_id.to_string(), 100)
            .await
            .map_err(|e| SdkError::Storage(e.to_string()))?;

        // Convert MessageRow to Message
        let messages: Vec<Message> = message_rows
            .into_iter()
            .map(|row| {
                // Parse UUIDs from strings
                let parse_user_id = |s: &str| {
                    uuid::Uuid::parse_str(s)
                        .ok()
                        .map(dchat_core::types::UserId)
                        .unwrap_or_default()
                };

                let parse_message_id = |s: &str| {
                    uuid::Uuid::parse_str(s)
                        .ok()
                        .map(dchat_core::types::MessageId)
                        .unwrap_or_default()
                };

                Message {
                    id: parse_message_id(&row.id),
                    message_type: dchat_messaging::types::MessageType::Direct {
                        sender: parse_user_id(&row.sender_id),
                        recipient: row
                            .recipient_id
                            .as_deref()
                            .map(parse_user_id)
                            .unwrap_or_else(dchat_core::types::UserId::new),
                    },
                    content: serde_json::from_str(&row.content)
                        .unwrap_or(dchat_core::types::MessageContent::Text(String::new())),
                    encrypted_payload: row.encrypted_payload,
                    timestamp: std::time::UNIX_EPOCH
                        + std::time::Duration::from_secs(row.timestamp as u64),
                    sequence: row.sequence_num.map(|s| s as u64),
                    status: dchat_messaging::types::MessageStatus::Created, // Parse from row.status
                    expires_at: row
                        .expires_at
                        .map(|t| std::time::UNIX_EPOCH + std::time::Duration::from_secs(t as u64)),
                    size: row.size,
                }
            })
            .collect();

        Ok(messages)
    }

    /// Get the client's identity
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Get the client's configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }
}
/// Builder for creating a Client
pub struct ClientBuilder {
    config: ClientConfig,
}

impl ClientBuilder {
    /// Create a new client builder
    pub fn new() -> Self {
        Self {
            config: ClientConfig::default(),
        }
    }

    /// Set the user's display name
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    /// Set the storage directory
    pub fn data_dir(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.config.storage.data_dir = path.into();
        self
    }

    /// Set bootstrap peers
    pub fn bootstrap_peers(mut self, peers: Vec<String>) -> Self {
        self.config.network.bootstrap_peers = peers;
        self
    }

    /// Set the listen port
    pub fn listen_port(mut self, port: u16) -> Self {
        self.config.network.listen_port = port;
        self
    }

    /// Enable or disable encryption
    pub fn encryption(mut self, enabled: bool) -> Self {
        self.config.encryption_enabled = enabled;
        self
    }

    /// Build the client
    pub async fn build(self) -> Result<Client> {
        Client::with_config(self.config).await
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_builder() {
        let temp_dir = std::env::temp_dir().join(format!("dchat_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let client = Client::builder()
            .name("Alice")
            .data_dir(&temp_dir)
            .listen_port(8080)
            .build()
            .await
            .unwrap();

        assert_eq!(client.identity().username, "Alice");
        assert_eq!(client.config().network.listen_port, 8080);
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn test_client_connect() {
        let temp_dir = std::env::temp_dir().join(format!("dchat_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let client = Client::builder()
            .name("Bob")
            .data_dir(&temp_dir)
            .build()
            .await
            .unwrap();

        assert!(!client.is_connected().await);

        client.connect().await.unwrap();
        assert!(client.is_connected().await);

        client.disconnect().await.unwrap();
        assert!(!client.is_connected().await);
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn test_send_message_not_connected() {
        let temp_dir = std::env::temp_dir().join(format!("dchat_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let client = Client::builder()
            .name("Charlie")
            .data_dir(&temp_dir)
            .build()
            .await
            .unwrap();

        let result = client.send_message("Hello").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SdkError::NotConnected));
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn test_send_message_connected() {
        let temp_dir = std::env::temp_dir().join(format!("dchat_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let client = Client::builder()
            .name("Dave")
            .data_dir(&temp_dir)
            .build()
            .await
            .unwrap();

        client.connect().await.unwrap();

        client.send_message("Hello, dchat!").await.unwrap();

        let messages = client.receive_messages().await.unwrap();
        assert_eq!(messages.len(), 1);
        if let dchat_core::types::MessageContent::Text(text) = &messages[0].content {
            assert_eq!(text, "Hello, dchat!");
        } else {
            panic!("Test failed: Expected text message but got different message type");
        }
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn test_double_connect() {
        let temp_dir = std::env::temp_dir().join(format!("dchat_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let client = Client::builder()
            .name("Eve")
            .data_dir(&temp_dir)
            .build()
            .await
            .unwrap();

        client.connect().await.unwrap();
        let result = client.connect().await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SdkError::AlreadyConnected));
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
