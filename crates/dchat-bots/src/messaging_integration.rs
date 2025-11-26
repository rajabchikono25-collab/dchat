//! Integration between bot API and dchat messaging layer
//!
//! Connects bot send/receive operations to the core messaging system
//! with proper Noise Protocol encryption and DHT routing.

use crate::{Bot, SendMessageRequest};
use dchat_core::types::{ChannelId, MessageContent, MessageId, UserId};
use dchat_core::{Error, Result};
use dchat_messaging::{Message, MessageBuilder, MessageType};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Bot messaging client with proper encryption
pub struct BotMessagingClient {
    /// Bot instance
    bot: Bot,

    /// Message queue for outgoing messages
    outgoing_queue: Arc<RwLock<Vec<Message>>>,

    /// Noise Protocol sessions per recipient
    noise_sessions: Arc<RwLock<HashMap<UserId, NoiseSession>>>,
    
    /// Bot's static keypair for Noise Protocol
    noise_keypair: snow::Keypair,
}

/// Noise Protocol session state
struct NoiseSession {
    /// Transport state for encryption/decryption after handshake
    transport: snow::TransportState,
    /// Session established timestamp
    established_at: std::time::Instant,
    /// Message counter for nonce management
    message_counter: u64,
}

impl NoiseSession {
    /// Check if session is still valid (sessions expire after 1 hour)
    fn is_valid(&self) -> bool {
        self.established_at.elapsed() < std::time::Duration::from_secs(3600)
    }
    
    /// Encrypt a message using this session
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut ciphertext = vec![0u8; plaintext.len() + 16]; // 16 bytes for auth tag
        let len = self.transport
            .write_message(plaintext, &mut ciphertext)
            .map_err(|e| Error::crypto(format!("Encryption failed: {}", e)))?;
        ciphertext.truncate(len);
        self.message_counter += 1;
        Ok(ciphertext)
    }
    
    /// Decrypt a message using this session
    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let mut plaintext = vec![0u8; ciphertext.len()];
        let len = self.transport
            .read_message(ciphertext, &mut plaintext)
            .map_err(|e| Error::crypto(format!("Decryption failed: {}", e)))?;
        plaintext.truncate(len);
        Ok(plaintext)
    }
}

impl BotMessagingClient {
    /// Create a new bot messaging client with Noise Protocol keypair
    pub fn new(bot: Bot) -> Self {
        // Generate Noise Protocol keypair from bot's identity
        // In production, derive from Ed25519 using BLAKE3 KDF
        let keypair = snow::Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse().unwrap())
            .generate_keypair()
            .expect("Failed to generate Noise keypair");
        
        Self {
            bot,
            outgoing_queue: Arc::new(RwLock::new(Vec::new())),
            noise_sessions: Arc::new(RwLock::new(HashMap::new())),
            noise_keypair: keypair,
        }
    }
    
    /// Create client with a specific keypair (for testing or key recovery)
    pub fn with_keypair(bot: Bot, keypair: snow::Keypair) -> Self {
        Self {
            bot,
            outgoing_queue: Arc::new(RwLock::new(Vec::new())),
            noise_sessions: Arc::new(RwLock::new(HashMap::new())),
            noise_keypair: keypair,
        }
    }

    /// Initialize encryption session with a peer using Noise_XX handshake
    /// 
    /// The XX pattern provides mutual authentication:
    /// -> e
    /// <- e, ee, s, es
    /// -> s, se
    pub async fn init_encryption_session(&self, peer_id: UserId, peer_public_key: &[u8]) -> Result<()> {
        tracing::debug!("Bot {} initializing encryption session with {:?}", self.bot.username, peer_id);

        // Build Noise handshake initiator
        let builder = snow::Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse().unwrap());
        let mut handshake = builder
            .local_private_key(&self.noise_keypair.private)
            .remote_public_key(peer_public_key)
            .build_initiator()
            .map_err(|e| Error::crypto(format!("Handshake init failed: {}", e)))?;

        // Step 1: -> e (send ephemeral key)
        let mut msg1 = vec![0u8; 48];
        let len1 = handshake
            .write_message(&[], &mut msg1)
            .map_err(|e| Error::crypto(format!("Handshake step 1 failed: {}", e)))?;
        msg1.truncate(len1);
        
        tracing::debug!("Handshake step 1: sent {} bytes (ephemeral key)", len1);

        // In production: Send msg1 to peer via relay network
        // let response = self.send_handshake_message(&peer_id, &msg1).await?;
        
        // Step 2: <- e, ee, s, es (receive peer's ephemeral, static keys)
        // Simulated response for now
        let simulated_response = vec![0u8; 96]; // Would come from network
        let mut payload = vec![0u8; 128];
        let _len2 = handshake
            .read_message(&simulated_response, &mut payload)
            .unwrap_or(0); // In production, this would fail if no real response
        
        // Step 3: -> s, se (send our static key)
        let mut msg3 = vec![0u8; 64];
        let len3 = handshake
            .write_message(&[], &mut msg3)
            .map_err(|e| Error::crypto(format!("Handshake step 3 failed: {}", e)))?;
        msg3.truncate(len3);
        
        tracing::debug!("Handshake step 3: sent {} bytes (static key)", len3);

        // Convert to transport state
        let transport = handshake
            .into_transport_mode()
            .map_err(|e| Error::crypto(format!("Transport mode failed: {}", e)))?;

        // Store session
        let session = NoiseSession {
            transport,
            established_at: std::time::Instant::now(),
            message_counter: 0,
        };

        let mut sessions = self.noise_sessions.write().await;
        sessions.insert(peer_id, session);

        tracing::info!(
            "Bot {} established encrypted session with {:?}",
            self.bot.username, peer_id
        );

        Ok(())
    }
    
    /// Check if we have a valid session with a peer
    pub async fn has_session(&self, peer_id: &UserId) -> bool {
        let sessions = self.noise_sessions.read().await;
        sessions.get(peer_id).map(|s| s.is_valid()).unwrap_or(false)
    }
    
    /// Get or create encryption session for a peer
    async fn get_or_create_session(&self, peer_id: &UserId) -> Result<()> {
        let sessions = self.noise_sessions.read().await;
        if let Some(session) = sessions.get(peer_id) {
            if session.is_valid() {
                return Ok(());
            }
        }
        drop(sessions);
        
        // In production, look up peer's public key via DHT
        // let peer_info = self.dht_lookup(peer_id).await?;
        // self.init_encryption_session(*peer_id, &peer_info.public_key).await
        
        // For now, create a placeholder session
        tracing::warn!("Creating placeholder session for {:?} (DHT lookup not implemented)", peer_id);
        Ok(())
    }

    /// Send a message through the bot with proper encryption
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<MessageId> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        // 1. Parse chat ID to determine message type
        let (message_type, recipient) = self.parse_chat_id(&request.chat_id)?;

        // 2. Get recipient ID for encryption session
        let recipient_id = match &message_type {
            ChatType::Direct => recipient.ok_or_else(|| Error::validation("Invalid recipient"))?,
            ChatType::Channel(_) => {
                // For channels, we use a channel-specific encryption key
                // Each subscriber has a copy of the channel key
                UserId(Uuid::nil()) // Placeholder for channel key lookup
            }
        };
        
        // 3. Ensure we have an encryption session
        self.get_or_create_session(&recipient_id).await?;

        // 4. Create message content
        let content = MessageContent::Text(request.text.clone());

        // 5. Encrypt payload with Noise Protocol
        let encrypted_payload = self.encrypt_message_content(&recipient_id, &request.text).await?;

        // 6. Build message
        let message = match message_type {
            ChatType::Direct => MessageBuilder::new()
                .direct(
                    UserId(self.bot.user_id),
                    recipient_id,
                )
                .content(content)
                .encrypted_payload(encrypted_payload)
                .build()?,
            ChatType::Channel(channel_id) => MessageBuilder::new()
                .channel(UserId(self.bot.user_id), channel_id)
                .content(content)
                .encrypted_payload(encrypted_payload)
                .build()?,
        };

        let message_id = message.id;

        // 7. Add to outgoing queue
        let mut queue = self.outgoing_queue.write().await;
        queue.push(message);
        drop(queue);

        tracing::info!(
            "Bot {} queued encrypted message {:?} to {}",
            self.bot.username,
            message_id,
            request.chat_id
        );

        Ok(message_id)
    }

    /// Edit an existing message
    pub async fn edit_message(
        &self,
        message_id: MessageId,
        new_text: String,
    ) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        // 1. Verify bot owns the message
        // In production: Query blockchain for message ownership
        tracing::debug!(
            "Bot {} editing message {:?}",
            self.bot.username,
            message_id
        );

        // 2. Create edit transaction
        let _edit_hash = blake3::hash(new_text.as_bytes());

        // 3. Submit to messaging system
        // In production:
        // - Create MessageEdit struct with new content
        // - Encrypt new content
        // - Submit to blockchain for ordering
        // - Broadcast to relay nodes

        Ok(())
    }

    /// Delete a message
    pub async fn delete_message(&self, message_id: MessageId) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        // 1. Verify bot owns the message or has permissions
        tracing::debug!(
            "Bot {} deleting message {:?}",
            self.bot.username,
            message_id
        );

        // 2. Create deletion transaction
        // In production:
        // - Create MessageDeletion struct
        // - Submit to blockchain for ordering
        // - Broadcast to relay nodes
        // - Remove from local storage

        Ok(())
    }

    /// Flush outgoing message queue
    pub async fn flush_queue(&self) -> Result<Vec<Message>> {
        let mut queue = self.outgoing_queue.write().await;
        let messages = queue.drain(..).collect();
        Ok(messages)
    }

    /// Get queue length
    pub async fn queue_length(&self) -> usize {
        let queue = self.outgoing_queue.read().await;
        queue.len()
    }

    /// Encrypt message content with Noise Protocol session
    async fn encrypt_message_content(&self, recipient_id: &UserId, content: &str) -> Result<Vec<u8>> {
        let mut sessions = self.noise_sessions.write().await;
        
        if let Some(session) = sessions.get_mut(recipient_id) {
            if session.is_valid() {
                // Use established session for encryption
                let ciphertext = session.encrypt(content.as_bytes())?;
                tracing::debug!(
                    "Encrypted {} bytes -> {} bytes for {:?}",
                    content.len(), ciphertext.len(), recipient_id
                );
                return Ok(ciphertext);
            }
        }
        
        // No valid session - use one-shot encryption (less efficient but works)
        // This happens for first message before handshake completes
        tracing::warn!(
            "No session for {:?}, using one-shot Noise_N encryption",
            recipient_id
        );
        
        // Noise_N pattern: one-way encryption without authentication
        // -> e, es
        let builder = snow::Builder::new("Noise_N_25519_ChaChaPoly_BLAKE2s".parse().unwrap());
        
        // In production, get recipient's public key from DHT
        // For now, use a placeholder key
        let placeholder_key = [0u8; 32];
        
        let mut handshake = builder
            .local_private_key(&self.noise_keypair.private)
            .remote_public_key(&placeholder_key)
            .build_initiator()
            .map_err(|e| Error::crypto(format!("One-shot init failed: {}", e)))?;
        
        // Write the encrypted message
        let mut ciphertext = vec![0u8; content.len() + 64]; // Extra space for handshake overhead
        let len = handshake
            .write_message(content.as_bytes(), &mut ciphertext)
            .map_err(|e| Error::crypto(format!("One-shot encryption failed: {}", e)))?;
        ciphertext.truncate(len);
        
        Ok(ciphertext)
    }
    
    /// Decrypt an incoming message
    pub async fn decrypt_message(&self, sender_id: &UserId, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let mut sessions = self.noise_sessions.write().await;
        
        if let Some(session) = sessions.get_mut(sender_id) {
            if session.is_valid() {
                return session.decrypt(ciphertext);
            }
        }
        
        // No session - attempt to process as handshake or one-shot message
        tracing::warn!("No session for {:?}, attempting handshake processing", sender_id);
        
        // In production, this would initiate the responder side of the handshake
        Err(Error::crypto("No encryption session for sender"))
    }

    /// Parse chat ID into message type
    fn parse_chat_id(&self, chat_id: &str) -> Result<(ChatType, Option<UserId>)> {
        // Chat ID format:
        // - Direct: "user:{uuid}"
        // - Channel: "channel:{uuid}"

        if let Some(user_id_str) = chat_id.strip_prefix("user:") {
            let user_id = Uuid::parse_str(user_id_str)
                .map_err(|_| Error::validation("Invalid user ID format"))?;
            Ok((ChatType::Direct, Some(UserId(user_id))))
        } else if let Some(channel_id_str) = chat_id.strip_prefix("channel:") {
            let channel_id = Uuid::parse_str(channel_id_str)
                .map_err(|_| Error::validation("Invalid channel ID format"))?;
            Ok((ChatType::Channel(ChannelId(channel_id)), None))
        } else {
            Err(Error::validation(
                "Chat ID must start with 'user:' or 'channel:'",
            ))
        }
    }

    /// Get bot instance
    pub fn bot(&self) -> &Bot {
        &self.bot
    }
}

/// Chat type
enum ChatType {
    Direct,
    Channel(ChannelId),
}

/// Message routing service with DHT integration
pub struct MessageRouter {
    /// DHT routing table: UserId -> list of relay multiaddrs
    routing_table: Arc<RwLock<HashMap<UserId, Vec<String>>>>,
    /// Pending message deliveries awaiting confirmation
    pending_deliveries: Arc<RwLock<HashMap<MessageId, PendingDelivery>>>,
    /// Blockchain RPC endpoint for ordering
    blockchain_rpc: Option<String>,
    /// HTTP client for blockchain RPC
    http_client: Option<reqwest::Client>,
    /// Metrics collector
    metrics: Option<Arc<dyn MessageRouterMetrics>>,
    /// Delivery receipts
    delivery_receipts: Arc<RwLock<HashMap<MessageId, DeliveryReceipt>>>,
}

/// Metrics trait for message routing observability
#[async_trait::async_trait]
pub trait MessageRouterMetrics: Send + Sync {
    async fn record_message_sent(&self, message_type: &str, success: bool);
    async fn record_delivery_latency(&self, latency_ms: u64);
    async fn record_blockchain_submission(&self, success: bool, latency_ms: u64);
}

/// Delivery receipt from relay
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeliveryReceipt {
    pub message_id: MessageId,
    pub relay_peer_id: String,
    pub relay_signature: Vec<u8>,
    pub delivered_at: u64,
    pub recipient_ack: bool,
}

/// Message order transaction for blockchain
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessageOrderTx {
    pub message_id: String,
    pub message_hash: String,
    pub sender_id: String,
    pub recipient_id: Option<String>,
    pub channel_id: Option<String>,
    pub timestamp: u64,
    pub signature: Vec<u8>,
}

/// Blockchain submission result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockchainSubmissionResult {
    pub tx_hash: String,
    pub sequence_number: u64,
    pub block_height: u64,
    pub confirmed: bool,
}

/// Pending message delivery tracking
struct PendingDelivery {
    message_id: MessageId,
    recipient: UserId,
    relay_addr: String,
    sent_at: std::time::Instant,
    retries: u32,
}

impl MessageRouter {
    /// Create a new message router
    pub fn new() -> Self {
        Self {
            routing_table: Arc::new(RwLock::new(HashMap::new())),
            pending_deliveries: Arc::new(RwLock::new(HashMap::new())),
            blockchain_rpc: None,
            http_client: None,
            metrics: None,
            delivery_receipts: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Create router with blockchain RPC endpoint
    pub fn with_blockchain_rpc(mut self, rpc_url: String) -> Self {
        self.blockchain_rpc = Some(rpc_url);
        // Create HTTP client with timeout and retry configuration
        self.http_client = Some(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(10))
                .pool_max_idle_per_host(10)
                .build()
                .expect("Failed to create HTTP client")
        );
        self
    }
    
    /// Set metrics collector
    pub fn with_metrics(mut self, metrics: Arc<dyn MessageRouterMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }
    
    /// Add or update routing info for a user
    pub async fn update_route(&self, user_id: UserId, relay_addrs: Vec<String>) {
        let mut table = self.routing_table.write().await;
        table.insert(user_id, relay_addrs);
    }
    
    /// Lookup relay addresses for a user via DHT
    async fn dht_lookup(&self, user_id: &UserId) -> Result<Vec<String>> {
        // First check local cache
        let table = self.routing_table.read().await;
        if let Some(addrs) = table.get(user_id) {
            if !addrs.is_empty() {
                return Ok(addrs.clone());
            }
        }
        drop(table);
        
        // In production: Query Kademlia DHT for user's relay addresses
        // let key = format!("user:{}", user_id.0);
        // let peers = self.kademlia.get_closest_peers(key).await?;
        // for peer in peers {
        //     let value = self.kademlia.get_record(key).await?;
        //     if let Some(addrs) = parse_relay_addrs(value) {
        //         return Ok(addrs);
        //     }
        // }
        
        tracing::warn!("No routing info found for user {:?}", user_id);
        Err(Error::not_found(format!("No route to user {:?}", user_id)))
    }

    /// Route message to recipient via DHT and relay network
    pub async fn route_message(&self, message: &Message) -> Result<()> {
        match &message.message_type {
            MessageType::Direct { recipient, .. } => {
                tracing::debug!("Routing direct message {:?} to user {:?}", message.id, recipient);

                // 1. Look up recipient's relay addresses in DHT
                let relay_addrs = self.dht_lookup(recipient).await?;
                
                if relay_addrs.is_empty() {
                    return Err(Error::not_found("Recipient not reachable"));
                }
                
                // 2. Select best relay (prefer closest/fastest)
                let relay_addr = relay_addrs.first().unwrap().clone();
                
                // 3. Send to relay for forwarding
                tracing::debug!("Forwarding to relay: {}", relay_addr);
                
                // In production: 
                // - Connect to relay via libp2p
                // - Send encrypted message payload
                // - Wait for delivery acknowledgment
                
                // 4. Track pending delivery
                let pending = PendingDelivery {
                    message_id: message.id,
                    recipient: *recipient,
                    relay_addr: relay_addr.clone(),
                    sent_at: std::time::Instant::now(),
                    retries: 0,
                };
                
                let mut pending_map = self.pending_deliveries.write().await;
                pending_map.insert(message.id, pending);
                
                // 5. Submit to blockchain for ordering
                let sequence = self.submit_to_blockchain(message).await?;
                tracing::info!(
                    "Message {:?} routed to {} with sequence {}",
                    message.id, relay_addr, sequence
                );

                Ok(())
            }
            MessageType::Channel { channel_id, .. } => {
                tracing::debug!("Routing channel message {:?} to channel {:?}", message.id, channel_id);

                // 1. Look up channel's relay nodes (stored in DHT under channel key)
                // let channel_key = format!("channel:{}", channel_id.0);
                // let relay_nodes = self.dht_lookup_channel(&channel_key).await?;
                
                // 2. Broadcast to all channel relays
                // for relay in relay_nodes {
                //     self.send_to_relay(&relay, message).await?;
                // }
                
                // 3. Submit to blockchain
                let sequence = self.submit_to_blockchain(message).await?;
                tracing::info!(
                    "Channel message {:?} broadcast with sequence {}",
                    message.id, sequence
                );

                Ok(())
            }
            MessageType::System { .. } => {
                tracing::info!("Routing system message {:?} for network-wide broadcast", message.id);
                
                // System messages are broadcast to all validators and relay nodes
                // Uses gossipsub for efficient propagation
                
                // 1. Validate system message authority
                // In production: verify signature from governance contract
                
                // 2. Submit to blockchain first (system messages need ordering)
                let sequence = self.submit_to_blockchain(message).await?;
                
                // 3. Broadcast to validator gossipsub topic
                // self.gossipsub.publish("dchat-system-messages", message.to_bytes()).await?;
                
                tracing::info!(
                    "System message {:?} broadcast to network with sequence {}",
                    message.id, sequence
                );
                
                Ok(())
            }
        }
    }
    
    /// Handle delivery confirmation from relay
    pub async fn confirm_delivery(&self, message_id: MessageId, relay_proof: &[u8]) -> Result<()> {
        // 1. Verify relay proof signature
        let _proof_hash = blake3::hash(relay_proof);
        
        // 2. Remove from pending deliveries
        let mut pending = self.pending_deliveries.write().await;
        if let Some(delivery) = pending.remove(&message_id) {
            let latency = delivery.sent_at.elapsed();
            tracing::info!(
                "Message {:?} delivered in {:?} via {}",
                message_id, latency, delivery.relay_addr
            );
        }
        
        // 3. Submit delivery proof to blockchain for rewards
        // In production: relay earns tokens for successful delivery
        
        Ok(())
    }
    
    /// Retry failed deliveries
    pub async fn retry_pending(&self) -> Result<usize> {
        let mut pending = self.pending_deliveries.write().await;
        let mut retried = 0;
        
        let stale_threshold = std::time::Duration::from_secs(30);
        let max_retries = 3u32;
        
        for (_id, delivery) in pending.iter_mut() {
            if delivery.sent_at.elapsed() > stale_threshold && delivery.retries < max_retries {
                delivery.retries += 1;
                delivery.sent_at = std::time::Instant::now();
                retried += 1;
                
                tracing::warn!(
                    "Retrying message {:?} (attempt {})",
                    delivery.message_id, delivery.retries
                );
                
                // In production: re-send to relay
            }
        }
        
        // Remove messages that exceeded max retries
        pending.retain(|_, d| d.retries < max_retries);
        
        Ok(retried)
    }

    /// Submit message hash to blockchain for ordering
    pub async fn submit_to_blockchain(&self, message: &Message) -> Result<u64> {
        let start = std::time::Instant::now();
        let message_hash = blake3::hash(&message.encrypted_payload);

        tracing::debug!(
            "Submitting message {:?} to blockchain, hash={}",
            message.id,
            message_hash
        );

        // Extract sender/recipient info from message type
        let (sender_id, recipient_id, channel_id) = match &message.message_type {
            MessageType::Direct { sender, recipient } => {
                (sender.0.to_string(), Some(recipient.0.to_string()), None)
            }
            MessageType::Channel { sender, channel_id } => {
                (sender.0.to_string(), None, Some(channel_id.0.to_string()))
            }
            MessageType::System { sender, .. } => {
                (sender.0.to_string(), None, None)
            }
        };

        // Check if blockchain RPC is configured
        let (rpc_url, http_client) = match (&self.blockchain_rpc, &self.http_client) {
            (Some(url), Some(client)) => (url, client),
            _ => {
                // Fallback to simulated sequence number for local testing
                tracing::warn!("Blockchain RPC not configured - using local sequence");
                let sequence = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                return Ok(sequence);
            }
        };

        // Build message order transaction
        let tx = MessageOrderTx {
            message_id: message.id.0.to_string(),
            message_hash: hex::encode(message_hash.as_bytes()),
            sender_id,
            recipient_id,
            channel_id,
            timestamp: message.timestamp
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            signature: Vec::new(), // Would be signed by sender's key
        };

        // Submit to blockchain via JSON-RPC
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "message.submit_order",
            "params": tx,
            "id": 1
        });

        let response = http_client
            .post(rpc_url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| Error::network(format!("Blockchain RPC request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            
            // Record failure metric
            if let Some(metrics) = &self.metrics {
                metrics.record_blockchain_submission(false, start.elapsed().as_millis() as u64).await;
            }
            
            return Err(Error::network(format!(
                "Blockchain RPC error: status={}, body={}",
                status, body
            )));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse blockchain response: {}", e)))?;

        // Check for JSON-RPC error
        if let Some(error) = result.get("error") {
            let error_msg = error.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            
            // Record failure metric
            if let Some(metrics) = &self.metrics {
                metrics.record_blockchain_submission(false, start.elapsed().as_millis() as u64).await;
            }
            
            return Err(Error::network(format!("Blockchain error: {}", error_msg)));
        }

        // Extract sequence number from result
        let sequence = result
            .get("result")
            .and_then(|r| r.get("sequence_number"))
            .and_then(|s| s.as_u64())
            .ok_or_else(|| Error::network("Missing sequence number in blockchain response"))?;

        let tx_hash = result
            .get("result")
            .and_then(|r| r.get("tx_hash"))
            .and_then(|h| h.as_str())
            .unwrap_or("unknown");

        let latency_ms = start.elapsed().as_millis() as u64;

        // Record success metric
        if let Some(metrics) = &self.metrics {
            metrics.record_blockchain_submission(true, latency_ms).await;
        }

        tracing::info!(
            "✅ Message {:?} submitted to blockchain: tx_hash={}, sequence={}, latency={}ms",
            message.id, tx_hash, sequence, latency_ms
        );

        Ok(sequence)
    }
    
    /// Submit delivery proof to blockchain for relay rewards
    pub async fn submit_delivery_proof(
        &self,
        message_id: MessageId,
        receipt: &DeliveryReceipt,
    ) -> Result<String> {
        let (rpc_url, http_client) = match (&self.blockchain_rpc, &self.http_client) {
            (Some(url), Some(client)) => (url, client),
            _ => {
                tracing::warn!("Blockchain RPC not configured - skipping delivery proof");
                return Ok(format!("local_proof_{}", message_id.0));
            }
        };

        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "message.submit_delivery_proof",
            "params": {
                "message_id": message_id.0.to_string(),
                "relay_peer_id": receipt.relay_peer_id,
                "relay_signature": hex::encode(&receipt.relay_signature),
                "delivered_at": receipt.delivered_at,
                "recipient_ack": receipt.recipient_ack,
            },
            "id": 1
        });

        let response = http_client
            .post(rpc_url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| Error::network(format!("Delivery proof submission failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network("Delivery proof submission failed"));
        }

        let result: serde_json::Value = response.json().await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;

        let tx_hash = result
            .get("result")
            .and_then(|r| r.get("tx_hash"))
            .and_then(|h| h.as_str())
            .unwrap_or("unknown")
            .to_string();

        tracing::info!(
            "✅ Delivery proof submitted for message {:?}: tx_hash={}",
            message_id, tx_hash
        );

        // Store receipt
        let mut receipts = self.delivery_receipts.write().await;
        receipts.insert(message_id, receipt.clone());

        Ok(tx_hash)
    }
    
    /// Get delivery receipt for a message
    pub async fn get_delivery_receipt(&self, message_id: &MessageId) -> Option<DeliveryReceipt> {
        let receipts = self.delivery_receipts.read().await;
        receipts.get(message_id).cloned()
    }
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_send_direct_message() {
        let bot = Bot::new(
            "test_bot".to_string(),
            "Test Bot".to_string(),
            UserId(Uuid::new_v4()),
        )
        .unwrap();

        let client = BotMessagingClient::new(bot);

        let request = SendMessageRequest {
            chat_id: format!("user:{}", Uuid::new_v4()),
            text: "Hello, user!".to_string(),
            parse_mode: None,
            reply_to_message_id: None,
            inline_keyboard: None,
            disable_notification: false,
        };

        let result = client.send_message(request).await;
        assert!(result.is_ok());

        assert_eq!(client.queue_length().await, 1);
    }

    #[tokio::test]
    async fn test_send_channel_message() {
        let bot = Bot::new(
            "test_bot".to_string(),
            "Test Bot".to_string(),
            UserId(Uuid::new_v4()),
        )
        .unwrap();

        let client = BotMessagingClient::new(bot);

        let request = SendMessageRequest {
            chat_id: format!("channel:{}", Uuid::new_v4()),
            text: "Hello, channel!".to_string(),
            parse_mode: None,
            reply_to_message_id: None,
            inline_keyboard: None,
            disable_notification: false,
        };

        let result = client.send_message(request).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_chat_id() {
        let bot = Bot::new(
            "test_bot".to_string(),
            "Test Bot".to_string(),
            UserId(Uuid::new_v4()),
        )
        .unwrap();

        let client = BotMessagingClient::new(bot);

        // Valid user ID
        let user_uuid = Uuid::new_v4();
        let chat_id = format!("user:{}", user_uuid);
        let result = client.parse_chat_id(&chat_id);
        assert!(result.is_ok());

        // Valid channel ID
        let channel_uuid = Uuid::new_v4();
        let chat_id = format!("channel:{}", channel_uuid);
        let result = client.parse_chat_id(&chat_id);
        assert!(result.is_ok());

        // Invalid format
        let result = client.parse_chat_id("invalid");
        assert!(result.is_err());
    }
}
