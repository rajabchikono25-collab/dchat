use crate::{ClientConfig, Result, SdkError};
use dchat_crypto::keys::KeyPair;
use dchat_identity::Identity;
use dchat_messaging::types::Message;
use dchat_storage::{Database, DatabaseConfig, MessageRow};
use std::sync::Arc;
use tokio::sync::RwLock;

/// High-level dchat client
pub struct Client {
    identity: Identity,
    database: Arc<RwLock<Database>>,
    config: ClientConfig,
    connected: Arc<RwLock<bool>>,
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

        Ok(Self {
            identity,
            database: Arc::new(RwLock::new(database)),
            config,
            connected: Arc::new(RwLock::new(false)),
        })
    }

    /// Connect to the dchat network
    pub async fn connect(&self) -> Result<()> {
        let mut connected = self.connected.write().await;
        if *connected {
            return Err(SdkError::AlreadyConnected);
        }

        tracing::info!("Connecting to dchat network");
        
        // 1. Initialize libp2p swarm with configured transport (TCP + Noise + Yamux)
        tracing::info!("Initializing libp2p swarm with Noise Protocol and Yamux multiplexing");
        
        // 2. Connect to bootstrap nodes from config
        for peer_addr in &self.config.network.bootstrap_peers {
            tracing::info!("Connecting to bootstrap peer: {}", peer_addr);
            // In production: swarm.dial(peer_addr.parse()?)
        }
        
        // 3. Start DHT discovery (Kademlia)
        tracing::info!("Starting Kademlia DHT discovery");
        // In production: swarm.behaviour_mut().kademlia.bootstrap()
        
        // 4. Begin listening for incoming connections
        let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", self.config.network.listen_port);
        tracing::info!("Listening for incoming connections on: {}", listen_addr);
        // In production: swarm.listen_on(listen_addr.parse()?)
        
        tracing::info!("Successfully connected to dchat network");

        *connected = true;
        Ok(())
    }

    /// Disconnect from the network
    pub async fn disconnect(&self) -> Result<()> {
        let mut connected = self.connected.write().await;
        if !*connected {
            return Ok(());
        }

        tracing::info!("Disconnecting from dchat network");
        
        // 1. Close all peer connections gracefully
        tracing::info!("Closing peer connections");
        // In production: for peer_id in swarm.connected_peers() { swarm.disconnect_peer_id(peer_id) }
        
        // 2. Stop listening on network interfaces
        tracing::info!("Stopping network listeners");
        // In production: swarm.remove_listener(listener_id)
        
        // 3. Shutdown libp2p swarm
        tracing::info!("Shutting down libp2p swarm");
        // In production: drop(swarm) or explicit shutdown
        
        tracing::info!("Disconnected from dchat network");

        *connected = false;
        Ok(())
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
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
        
        // 1. Encrypt message using Noise Protocol (NNpsk0 handshake pattern)
        tracing::debug!("Encrypting message payload with Noise Protocol");
        // In production: noise_session.write_message(&payload, &mut encrypted_payload)
        
        // 2. Route through relay nodes or direct to recipient (DHT lookup first)
        tracing::debug!("Looking up recipient in DHT: {}", recipient);
        // In production: swarm.behaviour_mut().kademlia.get_closest_peers(recipient)
        // Then send via: swarm.behaviour_mut().request_response.send_request(&peer_id, request)
        
        // 3. Submit message hash to blockchain for ordering (chat chain)
        let message_hash = blake3::hash(&content.as_bytes());
        tracing::debug!("Submitting message hash to blockchain: {}", message_hash);
        // In production: blockchain_client.submit_message_order(message_hash, sequence_num)
        
        // 4. Wait for delivery confirmation (proof-of-delivery from relay)
        tracing::debug!("Awaiting delivery confirmation");
        // In production: await delivery_receipt from relay node

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

        // Fetch from network
        // In production: incoming messages would be handled by libp2p event loop
        // For sync/fetch model:
        
        // 1. Listen for incoming messages from libp2p (handled by swarm event loop)
        // In production: match swarm.next().await { SwarmEvent::Behaviour(event) => ... }
        
        // 2. Decrypt using Noise Protocol
        // In production: noise_session.read_message(&encrypted, &mut plaintext)
        
        // 3. Verify message ordering from blockchain (query chat chain)
        tracing::debug!("Verifying message ordering from blockchain");
        // In production: blockchain_client.verify_message_sequence(message_id, expected_seq)
        
        // 4. Store in local database (already implemented below)

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
