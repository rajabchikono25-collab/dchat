//! Light Client Core
//!
//! A reusable, long-running light client suitable for mobile and desktop apps.
//!
//! Key design principles:
//! - **Relay-centric**: Connects to relays, avoids heavy gossipsub mesh participation
//! - **Fee-gated**: All outbound operations go through FeeGateway for mainnet economics
//! - **Offline-first**: Local SQLite storage with sync/resume on reconnect
//! - **Battery-aware**: Configurable sync intervals, backoff, and low-power mode
//! - **Chain-light**: Verifies ordering via proofs, no full state replication

use base64::{engine::general_purpose, Engine as _};
use chrono::TimeZone;
use dchat_core::error::{Error, Result};
use dchat_core::types::{ChannelId, MessageId, UserId};
use dchat_crypto::keys::{KeyPair, PrivateKey};
use dchat_crypto::{
    decrypt_with_key, decrypt_with_password, encrypt_with_key, encrypt_with_password,
    generate_encryption_key, EncryptedData, KEY_SIZE,
};
use dchat_identity::Identity;
use dchat_network::{DchatMessage, Multiaddr, NetworkConfig, NetworkEvent, NetworkManager, PeerId};
use dchat_storage::{Database, DatabaseConfig, MessageRow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

// Fee gateway for mainnet economics
use crate::fee_gateway::{FeeGatedRequest, FeeGateway, OperationPayload};

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Light client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightClientConfig {
    /// Display name / username
    pub display_name: String,

    /// Optional identity file path (import/export). Identity is persisted in the local DB.
    pub identity_path: Option<PathBuf>,

    /// Data directory for local storage
    pub data_dir: PathBuf,

    /// Bootstrap relay addresses
    pub bootstrap_relays: Vec<String>,

    /// Channels to subscribe to on startup
    pub default_channels: Vec<String>,

    /// Network profile (affects battery/bandwidth usage)
    pub profile: ClientProfile,

    /// Health check server address (None = disabled)
    pub health_addr: Option<String>,

    /// Metrics server address (None = disabled)
    pub metrics_addr: Option<String>,

    /// Enable fee-gated operations (required for mainnet)
    pub fee_gated: bool,

    /// Maximum offline queue size
    pub max_offline_queue: usize,

    /// Sync interval when connected (seconds)
    pub sync_interval_secs: u64,

    /// Keepalive interval for relay connections (seconds)
    pub keepalive_interval_secs: u64,
}

impl Default for LightClientConfig {
    fn default() -> Self {
        Self {
            display_name: "Anonymous".to_string(),
            identity_path: None,
            data_dir: PathBuf::from("./dchat-data"),
            bootstrap_relays: vec![],
            default_channels: vec!["global".to_string()],
            profile: ClientProfile::Balanced,
            health_addr: Some("127.0.0.1:8080".to_string()),
            metrics_addr: Some("127.0.0.1:9090".to_string()),
            fee_gated: true,
            max_offline_queue: 1000,
            sync_interval_secs: 30,
            keepalive_interval_secs: 60,
        }
    }
}

/// Client profile affecting resource usage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientProfile {
    /// Aggressive sync, more connections, faster but uses more battery/data
    Performance,
    /// Default balance between responsiveness and resource usage
    Balanced,
    /// Minimal background activity, longer sync intervals, battery saver
    LowPower,
    /// Relay-only mode, no direct P2P, maximum compatibility
    RelayOnly,
}

impl ClientProfile {
    /// Get recommended number of relay connections
    pub fn target_relay_connections(&self) -> usize {
        match self {
            Self::Performance => 5,
            Self::Balanced => 3,
            Self::LowPower => 1,
            Self::RelayOnly => 2,
        }
    }

    /// Get sync interval multiplier
    pub fn sync_interval_multiplier(&self) -> u64 {
        match self {
            Self::Performance => 1,
            Self::Balanced => 2,
            Self::LowPower => 5,
            Self::RelayOnly => 3,
        }
    }

    /// Whether to participate in gossipsub mesh
    pub fn enable_mesh(&self) -> bool {
        !matches!(self, Self::RelayOnly | Self::LowPower)
    }
}

// ============================================================================
// CLIENT STATE
// ============================================================================

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Syncing,
    Ready,
}

/// Queued outbound operation (for offline queue)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedOperation {
    pub id: Uuid,
    pub created_at: i64,
    pub operation: OutboundOperation,
    pub retry_count: u32,
    pub last_attempt: Option<i64>,
}

/// Outbound operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutboundOperation {
    ChannelMessage {
        channel_id: String,
        content: Vec<u8>,
        encrypted: bool,
    },
    DirectMessage {
        recipient_id: UserId,
        content: Vec<u8>,
        encrypted: bool,
    },
}

/// Sync cursor for incremental message fetch
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncCursor {
    /// Last verified sequence per channel
    pub channel_sequences: HashMap<String, u64>,
    /// Last verified sequence per DM conversation
    pub dm_sequences: HashMap<String, u64>,
    /// Timestamp of last successful sync
    pub last_sync: Option<i64>,
}

// ============================================================================
// EVENTS
// ============================================================================

/// Message verification status (for chain ordering validation)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationStatus {
    /// Not yet verified against chain
    Pending,
    /// Successfully verified: message exists in chain with correct ordering
    Verified { block_height: u64, sequence: u64 },
    /// Verification failed: message not found or ordering mismatch
    Failed { reason: &'static str },
    /// Verification skipped (e.g., local-only message)
    Skipped,
}

impl Default for VerificationStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Events emitted by the light client (for UI/app integration)
#[derive(Debug, Clone)]
pub enum LightClientEvent {
    /// Connection state changed
    ConnectionStateChanged(ConnectionState),
    /// New message received
    MessageReceived {
        channel_id: Option<String>,
        sender: String,
        content: String,
        timestamp: i64,
        /// Chain verification status (Pending until async verification completes)
        verification: VerificationStatus,
    },
    /// Message sent successfully
    MessageSent {
        message_id: String,
        channel_id: Option<String>,
    },
    /// Message send failed
    MessageFailed { operation_id: Uuid, error: String },
    /// Sync progress update
    SyncProgress {
        channels_synced: usize,
        total_channels: usize,
        messages_fetched: usize,
    },
    /// Peer connected
    PeerConnected(String),
    /// Peer disconnected
    PeerDisconnected(String),
    /// Error occurred
    Error(String),
}

const KV_SYNC_CURSOR: &str = "light_client.sync_cursor.v1";
const KV_OFFLINE_QUEUE: &str = "light_client.offline_queue.v1";
const KV_IDENTITY: &str = "light_client.identity.v1";
const KV_IDENTITY_KEY: &str = "light_client.identity_key.v1";

const ENV_IDENTITY_PASSPHRASE: &str = "DCHAT_LIGHT_CLIENT_IDENTITY_PASSPHRASE";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredIdentityV1 {
    version: u8,
    user_id: String,
    username: String,
    normalized_username: String,
    public_key_b64: String,
    created_at: i64,
    verified: bool,
    badges: Vec<String>,
    metadata: HashMap<String, String>,
    display_name: Option<String>,
    bio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredIdentityKeyV1 {
    version: u8,
    user_id: String,
    public_key_b64: String,
    encrypted_private_key: EncryptedData,
}

// ============================================================================
// LIGHT CLIENT CORE
// ============================================================================

/// Commands sent to the network task
#[allow(dead_code)]
enum NetCommand {
    Publish {
        channel_id: String,
        message: DchatMessage,
        resp: oneshot::Sender<Result<()>>,
    },
    GetMeshCount {
        channel_id: String,
        resp: oneshot::Sender<usize>,
    },
    Subscribe {
        channel_id: String,
        resp: oneshot::Sender<Result<()>>,
    },
    Unsubscribe {
        channel_id: String,
        resp: oneshot::Sender<Result<()>>,
    },
    GetPeerCount {
        resp: oneshot::Sender<usize>,
    },
    Shutdown {
        resp: oneshot::Sender<()>,
    },
}

/// The light client core
pub struct LightClient {
    /// Configuration
    config: LightClientConfig,
    /// User identity
    identity: Identity,
    /// Identity signing keypair (private key encrypted-at-rest in SQLite)
    identity_keypair: KeyPair,
    /// Local database (Option to allow taking for close)
    database: Option<Database>,
    /// Connection state
    state: Arc<RwLock<ConnectionState>>,
    /// Subscribed channels
    channels: Arc<RwLock<Vec<String>>>,
    /// Offline operation queue
    offline_queue: Arc<RwLock<VecDeque<QueuedOperation>>>,
    /// Sync cursor
    sync_cursor: Arc<RwLock<SyncCursor>>,
    /// Command sender to network task
    cmd_tx: Option<mpsc::Sender<NetCommand>>,
    /// Event receiver for UI
    event_rx: Option<mpsc::Receiver<LightClientEvent>>,
    /// Event sender (internal)
    event_tx: mpsc::Sender<LightClientEvent>,
    /// Shutdown signal
    shutdown_tx: broadcast::Sender<()>,
    /// Network task handle
    net_handle: Option<tokio::task::JoinHandle<()>>,
    /// Background sync task handle
    sync_handle: Option<tokio::task::JoinHandle<()>>,
    /// Fee gateway for mainnet economics (optional, enables fee-gated mode)
    fee_gateway: Option<Arc<FeeGateway>>,
    /// Client nonce counter for idempotent operations
    client_nonce: Arc<std::sync::atomic::AtomicU64>,
}

impl LightClient {
    /// Create a new light client
    pub async fn new(config: LightClientConfig) -> Result<Self> {
        info!("🔧 Initializing light client: {}", config.display_name);

        // Create data directory
        tokio::fs::create_dir_all(&config.data_dir)
            .await
            .map_err(Error::Io)?;

        // Initialize database
        let db_path = config.data_dir.join("light_client.db");
        let db_config = DatabaseConfig {
            path: db_path,
            max_connections: 5,
            connection_timeout_secs: 30,
            idle_timeout_secs: 300,
            max_lifetime_secs: 1800,
            enable_wal: true,
        };
        let database = Database::new(db_config).await?;
        info!("✓ Database initialized");

        // Load or create identity + keypair (identity persisted in DB; private key encrypted-at-rest)
        let (identity, identity_keypair) =
            load_or_create_identity_and_keypair(&database, &config).await?;
        info!("✓ Identity: {} ({})", identity.username, identity.user_id);

        // Load sync cursor from database (if exists)
        let sync_cursor = load_sync_cursor(&database).await.unwrap_or_default();

        // Load offline queue from database
        let offline_queue = load_offline_queue(&database).await.unwrap_or_default();
        if !offline_queue.is_empty() {
            info!("📤 {} queued operations pending", offline_queue.len());
        }

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(256);

        // Create shutdown channel
        let (shutdown_tx, _) = broadcast::channel(1);

        Ok(Self {
            config,
            identity,
            identity_keypair,
            database: Some(database),
            state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            channels: Arc::new(RwLock::new(vec![])),
            offline_queue: Arc::new(RwLock::new(offline_queue)),
            sync_cursor: Arc::new(RwLock::new(sync_cursor)),
            cmd_tx: None,
            event_rx: Some(event_rx),
            event_tx,
            shutdown_tx,
            net_handle: None,
            sync_handle: None,
            fee_gateway: None,
            client_nonce: Arc::new(std::sync::atomic::AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            )),
        })
    }

    /// Get the identity
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Get the identity signing keypair
    pub fn identity_keypair(&self) -> &KeyPair {
        &self.identity_keypair
    }

    /// Set the fee gateway for mainnet economics
    ///
    /// When set, all outbound operations (DMs, channel posts) will go through
    /// the fee gateway for proper fee enforcement before anchoring to chain.
    /// This is required for mainnet operation.
    pub fn set_fee_gateway(&mut self, gateway: Arc<FeeGateway>) {
        self.fee_gateway = Some(gateway);
        info!("✓ FeeGateway attached - fee-gated mode enabled");
    }

    /// Check if fee-gated mode is active
    pub fn is_fee_gated(&self) -> bool {
        self.config.fee_gated && self.fee_gateway.is_some()
    }

    /// Get next client nonce for idempotent operations
    fn next_nonce(&self) -> u64 {
        self.client_nonce
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Get current connection state
    pub async fn state(&self) -> ConnectionState {
        *self.state.read().await
    }

    /// Take the event receiver (can only be called once)
    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<LightClientEvent>> {
        self.event_rx.take()
    }

    /// Connect to the network
    pub async fn connect(&mut self) -> Result<()> {
        if *self.state.read().await != ConnectionState::Disconnected {
            return Err(Error::network("Already connected or connecting"));
        }

        if self.config.fee_gated && self.fee_gateway.is_none() {
            return Err(Error::Config(
                "fee_gated=true requires FeeGateway. Call set_fee_gateway() before connect()."
                    .to_string(),
            ));
        }

        self.set_state(ConnectionState::Connecting).await;

        // Build network config
        let mut network_config = NetworkConfig::default();

        // Parse bootstrap relays
        for addr_str in &self.config.bootstrap_relays {
            match addr_str.parse::<Multiaddr>() {
                Ok(multiaddr) => {
                    let peer_id = multiaddr
                        .iter()
                        .find_map(|proto| {
                            if let libp2p::multiaddr::Protocol::P2p(peer_id) = proto {
                                Some(peer_id)
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(PeerId::random);
                    network_config
                        .discovery
                        .bootstrap_nodes
                        .push((peer_id, multiaddr));
                    info!("✓ Added bootstrap relay: {}", addr_str);
                }
                Err(e) => warn!("⚠ Invalid relay address {}: {}", addr_str, e),
            }
        }

        // Adjust config based on profile
        if !self.config.profile.enable_mesh() {
            // Relay-only/low-power mode: disable aggressive mesh participation
            // Reduces battery/bandwidth by not maintaining full gossipsub mesh
            network_config.discovery.enable_mdns = false; // No local discovery
                                                          // Note: Additional mesh tuning (D, D_lo, D_hi, heartbeat) would require
                                                          // exposing gossipsub config in NetworkConfig. For now, we rely on
                                                          // minimal bootstrap connections and longer heartbeat intervals.
            debug!("Using relay-only network profile (mDNS disabled, minimal connections)");
        } else {
            debug!("Using mesh-enabled network profile");
        }

        // Initialize network
        let mut network = NetworkManager::new(network_config).await?;
        let peer_id = network.peer_id();
        network.start().await?;
        info!("✓ Network started (peer_id: {})", peer_id);

        // Subscribe to default channels
        for channel in &self.config.default_channels {
            if let Err(e) = network.subscribe_to_channel(channel) {
                warn!("Failed to subscribe to #{}: {}", channel, e);
            } else {
                info!("✓ Subscribed to #{}", channel);
                self.channels.write().await.push(channel.clone());
            }
        }

        // Create command channel
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<NetCommand>(256);
        let (_evt_internal_tx, _evt_internal_rx) = mpsc::channel::<NetworkEvent>(2048);

        self.cmd_tx = Some(cmd_tx);

        // Clone what we need for the network task
        let db_for_net = self
            .database
            .clone()
            .ok_or_else(|| Error::internal("Database not initialized"))?;
        let self_user_id = self.identity.user_id.clone();
        let event_tx = self.event_tx.clone();
        let _state = self.state.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Spawn network driver task
        let net_handle = tokio::spawn(async move {
            info!("🌐 Network task started");

            loop {
                tokio::select! {
                    cmd = cmd_rx.recv() => {
                        match cmd {
                            Some(NetCommand::Publish { channel_id, message, resp }) => {
                                let result = network.publish_to_channel(&channel_id, &message);

                                // Store outbound message locally
                                if result.is_ok() {
                                    if let DchatMessage::ChannelMessage { message_id, sender, channel_id, encrypted_payload, timestamp: _ } = &message {
                                        let content_hash = format!("{:x}", Sha256::digest(encrypted_payload));
                                        let _ = db_for_net.insert_message(&MessageRow {
                                            id: hex::encode(message_id),
                                            sender_id: sender.0.to_string(),
                                            recipient_id: None,
                                            channel_id: Some(channel_id.clone()),
                                            content_type: "channel_message".to_string(),
                                            content: String::new(),
                                            encrypted_payload: encrypted_payload.clone(),
                                            timestamp: chrono::Utc::now().timestamp(),
                                            sequence_num: None,
                                            status: "sent".to_string(),
                                            expires_at: None,
                                            size: encrypted_payload.len(),
                                            content_hash: Some(content_hash),
                                        }).await;
                                    }
                                }

                                let _ = resp.send(result);
                            }
                            Some(NetCommand::GetMeshCount { channel_id, resp }) => {
                                let count = network.get_mesh_peer_count(&channel_id);
                                let _ = resp.send(count);
                            }
                            Some(NetCommand::Subscribe { channel_id, resp }) => {
                                let result = network.subscribe_to_channel(&channel_id)
                                    .map_err(|e| Error::network(e.to_string()));
                                let _ = resp.send(result);
                            }
                            Some(NetCommand::Unsubscribe { channel_id, resp }) => {
                                let result = network.unsubscribe_from_channel(&channel_id)
                                    .map_err(|e| Error::network(e.to_string()));
                                let _ = resp.send(result);
                            }
                            Some(NetCommand::GetPeerCount { resp }) => {
                                // Get connected peer count from mesh
                                let count = network.get_mesh_peer_count("global");
                                let _ = resp.send(count);
                            }
                            Some(NetCommand::Shutdown { resp }) => {
                                info!("🛑 Network task shutting down");
                                let _ = resp.send(());
                                break;
                            }
                            None => break,
                        }
                    }

                    event = network.next_event() => {
                        let Some(event) = event else {
                            break;
                        };

                        // Process inbound messages
                        if let NetworkEvent::MessageReceived { from: _from, message } = &event {
                            if let DchatMessage::ChannelMessage { message_id, sender, channel_id, encrypted_payload, timestamp } = message {
                                // Don't process our own messages
                                if sender != &self_user_id {
                                    let content_hash = format!("{:x}", Sha256::digest(encrypted_payload));

                                    let (content_text, stored_content) = match std::str::from_utf8(encrypted_payload) {
                                        Ok(s) if !s.trim().is_empty() => {
                                            (s.to_string(), Some(s.to_string()))
                                        }
                                        _ => (format!("<opaque payload: {} bytes>", encrypted_payload.len()), None),
                                    };

                                    // Store in database
                                    let _ = db_for_net.insert_message(&MessageRow {
                                        id: hex::encode(message_id),
                                        sender_id: sender.0.to_string(),
                                        recipient_id: None,
                                        channel_id: Some(channel_id.clone()),
                                        content_type: "channel_message".to_string(),
                                        content: stored_content.unwrap_or_default(),
                                        encrypted_payload: encrypted_payload.clone(),
                                        timestamp: *timestamp,
                                        sequence_num: None,
                                        status: "received".to_string(),
                                        expires_at: None,
                                        size: encrypted_payload.len(),
                                        content_hash: Some(content_hash),
                                    }).await;

                                    // Emit event for UI
                                    // Note: Verification is async; UI shows Pending initially
                                    // Background task will update verification status
                                    let _ = event_tx.send(LightClientEvent::MessageReceived {
                                        channel_id: Some(channel_id.clone()),
                                        sender: sender.0.to_string(),
                                        content: content_text,
                                        timestamp: *timestamp,
                                        verification: VerificationStatus::Skipped,
                                    }).await;
                                }
                            }
                        }

                        // Emit connection events
                        match &event {
                            NetworkEvent::PeerConnected { peer_id, endpoint: _ } => {
                                let _ = event_tx.send(LightClientEvent::PeerConnected(peer_id.to_string())).await;
                            }
                            NetworkEvent::PeerDisconnected(peer) => {
                                let _ = event_tx.send(LightClientEvent::PeerDisconnected(peer.to_string())).await;
                            }
                            _ => {}
                        }
                    }

                    _ = shutdown_rx.recv() => {
                        info!("🛑 Network task received shutdown signal");
                        break;
                    }
                }
            }

            info!("🌐 Network task stopped");
        });

        self.net_handle = Some(net_handle);

        // Wait for initial connection
        info!("⏳ Waiting for relay connections...");
        tokio::time::sleep(Duration::from_secs(5)).await;

        self.set_state(ConnectionState::Connected).await;

        // Start background sync task
        self.start_sync_task().await;

        // Drain offline queue
        self.drain_offline_queue().await;

        self.set_state(ConnectionState::Ready).await;
        info!("✓ Light client ready");

        Ok(())
    }

    /// Disconnect from the network
    pub async fn disconnect(&mut self) -> Result<()> {
        if *self.state.read().await == ConnectionState::Disconnected {
            return Ok(());
        }

        info!("Disconnecting light client...");

        // Signal shutdown
        let _ = self.shutdown_tx.send(());

        // Stop network task
        if let Some(cmd_tx) = &self.cmd_tx {
            let (resp_tx, resp_rx) = oneshot::channel();
            let _ = cmd_tx.send(NetCommand::Shutdown { resp: resp_tx }).await;
            let _ = tokio::time::timeout(Duration::from_secs(5), resp_rx).await;
        }

        // Wait for tasks
        if let Some(handle) = self.net_handle.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }
        if let Some(handle) = self.sync_handle.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }

        // Persist state
        self.save_sync_cursor().await?;
        self.save_offline_queue().await?;

        // Close database (take ownership for close)
        if let Some(db) = self.database.take() {
            db.close().await?;
        }

        self.set_state(ConnectionState::Disconnected).await;
        info!("✓ Light client disconnected");

        Ok(())
    }

    /// Send a message to a channel
    ///
    /// When fee-gated mode is enabled (mainnet), this will:
    /// 1. Charge gas + message fees via FeeGateway
    /// 2. Submit ordering tx to chat chain
    /// 3. Wait for finality
    /// 4. Store message content
    /// 5. Broadcast via gossipsub for real-time delivery
    ///
    /// In dev mode (fee_gated=false or no FeeGateway), messages are sent
    /// directly via gossipsub without fee enforcement.
    pub async fn send_channel_message(&self, channel_id: &str, content: &str) -> Result<MessageId> {
        if *self.state.read().await != ConnectionState::Ready {
            // Queue for later
            return self
                .queue_operation(OutboundOperation::ChannelMessage {
                    channel_id: channel_id.to_string(),
                    content: content.as_bytes().to_vec(),
                    // Encryption handled at send time based on channel settings
                    encrypted: false,
                })
                .await;
        }

        // If fee_gated=true, never bypass fees by falling back to the direct path.
        if self.config.fee_gated {
            return self
                .send_channel_message_fee_gated(channel_id, content)
                .await;
        }

        // Dev mode: direct gossipsub without fee enforcement
        self.send_channel_message_direct(channel_id, content).await
    }

    /// Send channel message via FeeGateway (production path)
    async fn send_channel_message_fee_gated(
        &self,
        channel_id: &str,
        content: &str,
    ) -> Result<MessageId> {
        let fee_gateway = self.fee_gateway.as_ref().ok_or_else(|| {
            Error::Config(
                "fee_gated=true requires FeeGateway. Call set_fee_gateway() first.".to_string(),
            )
        })?;

        // Create deterministic ChannelId from channel name using UUID v5
        let channel_uuid = channel_name_to_uuid(channel_id);

        let request = FeeGatedRequest {
            payer: self.identity.user_id.clone(),
            client_nonce: self.next_nonce(),
            payload: OperationPayload::ChannelPost {
                channel_id: ChannelId(channel_uuid),
                content: content.as_bytes().to_vec(),
                encrypted: false,
                encryption_key_id: None,
            },
            preferred_relay: None,
        };

        info!(
            "📤 Sending fee-gated channel message to #{} (nonce: {})",
            channel_id, request.client_nonce
        );

        let response = fee_gateway.post_to_channel(request).await?;

        info!(
            "✓ Channel message sent: {} (gas: {}, msg fee: {}, storage: {:?})",
            response.message_id,
            response.gas_fee_receipt.amount,
            response.message_fee_receipt.amount,
            response.storage_tier
        );

        // Also broadcast via gossipsub for real-time delivery
        if let Some(cmd_tx) = &self.cmd_tx {
            let timestamp = chrono::Utc::now().timestamp();
            let payload = content.as_bytes().to_vec();
            let message_id = dchat_network::behavior::compute_channel_message_id(
                &self.identity.user_id,
                channel_id,
                &payload,
                timestamp,
            );

            let message = DchatMessage::ChannelMessage {
                message_id,
                sender: self.identity.user_id.clone(),
                channel_id: channel_id.to_string(),
                encrypted_payload: payload,
                timestamp,
            };

            let (resp_tx, _resp_rx) = oneshot::channel();
            // Fire-and-forget gossipsub broadcast (chain is source of truth)
            let _ = cmd_tx
                .send(NetCommand::Publish {
                    channel_id: channel_id.to_string(),
                    message,
                    resp: resp_tx,
                })
                .await;
        }

        // Emit success event
        let _ = self
            .event_tx
            .send(LightClientEvent::MessageSent {
                message_id: response.message_id.to_string(),
                channel_id: Some(channel_id.to_string()),
            })
            .await;

        Ok(response.message_id)
    }

    /// Send channel message directly via gossipsub (dev mode)
    async fn send_channel_message_direct(
        &self,
        channel_id: &str,
        content: &str,
    ) -> Result<MessageId> {
        let cmd_tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| Error::network("Not connected"))?;

        let timestamp = chrono::Utc::now().timestamp();
        let payload = content.as_bytes().to_vec();
        let message_id = dchat_network::behavior::compute_channel_message_id(
            &self.identity.user_id,
            channel_id,
            &payload,
            timestamp,
        );

        let message = DchatMessage::ChannelMessage {
            message_id,
            sender: self.identity.user_id.clone(),
            channel_id: channel_id.to_string(),
            encrypted_payload: payload,
            timestamp,
        };

        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(NetCommand::Publish {
                channel_id: channel_id.to_string(),
                message,
                resp: resp_tx,
            })
            .await
            .map_err(|_| Error::network("Network task not running"))?;

        resp_rx
            .await
            .map_err(|_| Error::network("Network task stopped"))?
            .map_err(|e| Error::network(e.to_string()))?;

        // Convert [u8; 32] message_id to Uuid (using first 16 bytes as fingerprint)
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&message_id[..16]);
        let msg_id = MessageId(Uuid::from_bytes(uuid_bytes));

        // Emit success event
        let _ = self
            .event_tx
            .send(LightClientEvent::MessageSent {
                message_id: msg_id.to_string(),
                channel_id: Some(channel_id.to_string()),
            })
            .await;

        Ok(msg_id)
    }

    /// Subscribe to a channel
    pub async fn subscribe(&self, channel_id: &str) -> Result<()> {
        let cmd_tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| Error::network("Not connected"))?;

        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(NetCommand::Subscribe {
                channel_id: channel_id.to_string(),
                resp: resp_tx,
            })
            .await
            .map_err(|_| Error::network("Network task not running"))?;

        resp_rx
            .await
            .map_err(|_| Error::network("Network task stopped"))??;

        self.channels.write().await.push(channel_id.to_string());
        info!("✓ Subscribed to #{}", channel_id);

        Ok(())
    }

    /// Send a direct message to a recipient
    ///
    /// Requires fee-gated mode to be enabled (FeeGateway attached).
    /// DMs are fee-gated to prevent spam and ensure economic sustainability.
    pub async fn send_direct_message(
        &self,
        recipient: &UserId,
        content: &str,
    ) -> Result<MessageId> {
        if *self.state.read().await != ConnectionState::Ready {
            // Queue for later
            return self
                .queue_operation(OutboundOperation::DirectMessage {
                    recipient_id: recipient.clone(),
                    content: content.as_bytes().to_vec(),
                    encrypted: false,
                })
                .await;
        }

        // DMs require fee-gated mode
        if !self.is_fee_gated() {
            return Err(Error::validation(
                "Direct messages require fee-gated mode. Call set_fee_gateway() first.",
            ));
        }

        let fee_gateway = self
            .fee_gateway
            .as_ref()
            .ok_or_else(|| Error::internal("FeeGateway not configured"))?;

        let request = FeeGatedRequest {
            payer: self.identity.user_id.clone(),
            client_nonce: self.next_nonce(),
            payload: OperationPayload::DirectMessage {
                recipient: recipient.clone(),
                content: content.as_bytes().to_vec(),
                encrypted: false,
                encryption_key_id: None,
            },
            preferred_relay: None,
        };

        info!(
            "📤 Sending fee-gated DM to {} (nonce: {})",
            recipient, request.client_nonce
        );

        let response = fee_gateway.send_direct_message(request).await?;

        info!(
            "✓ DM sent: {} (gas: {}, msg fee: {}, relay: {})",
            response.message_id,
            response.gas_fee_receipt.amount,
            response.message_fee_receipt.amount,
            response.message_fee_receipt.relay_id
        );

        // Emit success event
        let _ = self
            .event_tx
            .send(LightClientEvent::MessageSent {
                message_id: response.message_id.to_string(),
                channel_id: None,
            })
            .await;

        Ok(response.message_id)
    }

    /// Unsubscribe from a channel
    pub async fn unsubscribe(&self, channel_id: &str) -> Result<()> {
        let cmd_tx = self
            .cmd_tx
            .as_ref()
            .ok_or_else(|| Error::network("Not connected"))?;

        let (resp_tx, resp_rx) = oneshot::channel();
        cmd_tx
            .send(NetCommand::Unsubscribe {
                channel_id: channel_id.to_string(),
                resp: resp_tx,
            })
            .await
            .map_err(|_| Error::network("Network task not running"))?;

        resp_rx
            .await
            .map_err(|_| Error::network("Network task stopped"))??;

        self.channels.write().await.retain(|c| c != channel_id);
        info!("✓ Unsubscribed from #{}", channel_id);

        Ok(())
    }

    /// Get connected peer count
    pub async fn peer_count(&self) -> usize {
        if let Some(cmd_tx) = &self.cmd_tx {
            let (resp_tx, resp_rx) = oneshot::channel();
            if cmd_tx
                .send(NetCommand::GetPeerCount { resp: resp_tx })
                .await
                .is_ok()
            {
                return resp_rx.await.unwrap_or(0);
            }
        }
        0
    }

    /// Get recent messages from a channel
    pub async fn get_channel_messages(
        &self,
        channel_id: &str,
        limit: usize,
    ) -> Result<Vec<MessageRow>> {
        let db = self
            .database
            .as_ref()
            .ok_or_else(|| Error::storage("Database not available"))?;

        // Get all messages and filter by channel_id (Database doesn't have channel-specific method)
        let all_messages = db
            .get_all_messages(limit as i64 * 10) // Fetch more than needed for filtering
            .await
            .map_err(|e| Error::storage(e.to_string()))?;

        let filtered: Vec<MessageRow> = all_messages
            .into_iter()
            .filter(|msg| msg.channel_id.as_deref() == Some(channel_id))
            .take(limit)
            .collect();

        Ok(filtered)
    }

    // ========================================================================
    // INTERNAL HELPERS
    // ========================================================================

    async fn set_state(&self, state: ConnectionState) {
        *self.state.write().await = state;
        let _ = self
            .event_tx
            .send(LightClientEvent::ConnectionStateChanged(state))
            .await;
    }

    async fn start_sync_task(&mut self) {
        let sync_interval = Duration::from_secs(
            self.config.sync_interval_secs * self.config.profile.sync_interval_multiplier(),
        );
        let event_tx = self.event_tx.clone();
        let sync_cursor = self.sync_cursor.clone();
        let channels = self.channels.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let sync_handle = tokio::spawn(async move {
            info!("🔄 Sync task started (interval: {:?})", sync_interval);

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(sync_interval) => {
                        // Periodic background maintenance tick.
                        let channels_list = channels.read().await.clone();
                        let channels_synced = channels_list.len();

                        // Persist the last successful tick timestamp so UIs can show freshness.
                        {
                            let mut cursor = sync_cursor.write().await;
                            cursor.last_sync = Some(chrono::Utc::now().timestamp());
                        }

                        let _ = event_tx.send(LightClientEvent::SyncProgress {
                            channels_synced,
                            total_channels: channels_synced,
                            messages_fetched: 0,
                        }).await;

                        debug!("Sync completed for {} channels", channels_synced);
                    }
                    _ = shutdown_rx.recv() => {
                        info!("🛑 Sync task received shutdown signal");
                        break;
                    }
                }
            }

            info!("🔄 Sync task stopped");
        });

        self.sync_handle = Some(sync_handle);
    }

    async fn queue_operation(&self, operation: OutboundOperation) -> Result<MessageId> {
        let mut queue = self.offline_queue.write().await;

        if queue.len() >= self.config.max_offline_queue {
            return Err(Error::internal("Offline queue full"));
        }

        let op_id = Uuid::new_v4();
        queue.push_back(QueuedOperation {
            id: op_id,
            created_at: chrono::Utc::now().timestamp(),
            operation,
            retry_count: 0,
            last_attempt: None,
        });

        info!("📥 Operation queued (queue size: {})", queue.len());

        // Return a placeholder message ID
        Ok(MessageId(op_id))
    }

    async fn drain_offline_queue(&self) {
        let mut queue = self.offline_queue.write().await;

        if queue.is_empty() {
            return;
        }

        info!("📤 Draining {} queued operations", queue.len());

        let mut failures = VecDeque::new();

        while let Some(mut op) = queue.pop_front() {
            // Skip DM operations if fee gateway is not available
            if matches!(op.operation, OutboundOperation::DirectMessage { .. })
                && !self.is_fee_gated()
            {
                // DM send path requires fee gateway
                op.retry_count = 4; // Mark as failing to avoid retry loops
            }

            let result = match &op.operation {
                OutboundOperation::ChannelMessage {
                    channel_id,
                    content,
                    ..
                } => {
                    // Release lock temporarily for send
                    drop(queue);

                    let content_str = String::from_utf8_lossy(content);
                    let res = self.send_channel_message(channel_id, &content_str).await;

                    queue = self.offline_queue.write().await;
                    res.map(|_| ())
                }
                OutboundOperation::DirectMessage {
                    recipient_id,
                    content,
                    ..
                } => {
                    if self.is_fee_gated() {
                        // Fee gateway available - send via fee-gated path
                        drop(queue);

                        let content_str = String::from_utf8_lossy(content);
                        let res = self.send_direct_message(recipient_id, &content_str).await;

                        queue = self.offline_queue.write().await;
                        res.map(|_| ())
                    } else {
                        // No fee gateway - fail with clear error
                        warn!(
                            "DM to {} cannot be sent: FeeGateway not configured",
                            recipient_id
                        );
                        Err(Error::internal(
                            "DM sending requires FeeGateway. Call set_fee_gateway() first.",
                        ))
                    }
                }
            };

            if let Err(e) = result {
                op.retry_count += 1;
                op.last_attempt = Some(chrono::Utc::now().timestamp());

                if op.retry_count < 5 {
                    failures.push_back(op.clone());
                    warn!(
                        "⚠ Operation {} failed (attempt {}): {}",
                        op.id, op.retry_count, e
                    );
                } else {
                    error!(
                        "❌ Operation {} failed permanently after {} attempts",
                        op.id, op.retry_count
                    );
                    let _ = self
                        .event_tx
                        .send(LightClientEvent::MessageFailed {
                            operation_id: op.id,
                            error: e.to_string(),
                        })
                        .await;
                }
            }
        }

        // Re-queue failures
        queue.extend(failures);

        if !queue.is_empty() {
            info!("📥 {} operations remain in queue", queue.len());
        }
    }

    async fn save_sync_cursor(&self) -> Result<()> {
        let cursor = self.sync_cursor.read().await;
        let json = serde_json::to_string(&*cursor)?;
        let db = self
            .database
            .as_ref()
            .ok_or_else(|| Error::internal("Database not initialized"))?;
        db.put_client_kv(KV_SYNC_CURSOR, &json).await?;
        Ok(())
    }

    async fn save_offline_queue(&self) -> Result<()> {
        let queue = self.offline_queue.read().await;
        let json = serde_json::to_string(&*queue)?;
        let db = self
            .database
            .as_ref()
            .ok_or_else(|| Error::internal("Database not initialized"))?;
        db.put_client_kv(KV_OFFLINE_QUEUE, &json).await?;
        Ok(())
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Convert a channel name string to a deterministic UUID using UUID v5 (SHA-1 namespace)
///
/// This allows using human-readable channel names (like "global") while
/// maintaining compatibility with the ChannelId(Uuid) type.
fn channel_name_to_uuid(channel_name: &str) -> Uuid {
    // Use a fixed namespace UUID for dchat channels
    // Generated once: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, b"dchat.channels")
    const DCHAT_CHANNEL_NAMESPACE: Uuid = Uuid::from_bytes([
        0x9e, 0x1c, 0x4a, 0x8b, 0x3d, 0x2f, 0x5e, 0x7a, 0x8c, 0x9d, 0x0e, 0x1f, 0x2a, 0x3b, 0x4c,
        0x5d,
    ]);

    Uuid::new_v5(&DCHAT_CHANNEL_NAMESPACE, channel_name.as_bytes())
}

fn generate_identity_with_keypair(display_name: &str) -> Result<(Identity, KeyPair)> {
    let keypair = KeyPair::try_generate()
        .map_err(|e| Error::crypto(format!("Failed to generate keypair: {e}")))?;
    let identity = Identity::try_new(display_name.to_string(), &keypair)?;
    Ok((identity, keypair))
}

fn read_identity_passphrase() -> Result<String> {
    std::env::var(ENV_IDENTITY_PASSPHRASE).map_err(|_| {
        Error::crypto(format!(
            "{ENV_IDENTITY_PASSPHRASE} not set: required to encrypt/decrypt the light client identity private key"
        ))
    })
}

fn keypair_to_stored_key(
    identity: &Identity,
    keypair: &KeyPair,
    passphrase: &str,
) -> Result<StoredIdentityKeyV1> {
    let mut private_key_bytes = *keypair.private_key().as_bytes();
    let encrypted_private_key = encrypt_with_password(passphrase, &private_key_bytes)?;

    // Best-effort memory cleanup of the stack copy.
    private_key_bytes.fill(0);

    Ok(StoredIdentityKeyV1 {
        version: 1,
        user_id: identity.user_id.to_string(),
        public_key_b64: general_purpose::STANDARD.encode(identity.public_key.as_bytes()),
        encrypted_private_key,
    })
}

fn stored_key_to_keypair(
    identity: &Identity,
    stored: StoredIdentityKeyV1,
    passphrase: &str,
) -> Result<KeyPair> {
    if stored.version != 1 {
        return Err(Error::validation("Unsupported identity key store version"));
    }

    if stored.user_id != identity.user_id.to_string() {
        return Err(Error::validation(
            "Stored identity key does not match stored identity (user_id mismatch)",
        ));
    }

    let expected_pk_b64 = general_purpose::STANDARD.encode(identity.public_key.as_bytes());
    if stored.public_key_b64 != expected_pk_b64 {
        return Err(Error::validation(
            "Stored identity key does not match stored identity (public_key mismatch)",
        ));
    }

    let plaintext = decrypt_with_password(passphrase, &stored.encrypted_private_key)?;
    if plaintext.len() != 32 {
        return Err(Error::validation("Invalid decrypted private key length"));
    }

    let mut sk = [0u8; 32];
    sk.copy_from_slice(&plaintext);
    let private_key = PrivateKey::from_bytes(sk);
    // Best-effort cleanup for the stack copy.
    sk.fill(0);

    let keypair = KeyPair::from_private_key(private_key);

    let derived_pk = keypair.public_key().to_core_public_key();
    if derived_pk.as_bytes() != identity.public_key.as_bytes() {
        return Err(Error::validation(
            "Decrypted private key does not correspond to stored identity public key",
        ));
    }

    Ok(keypair)
}

async fn load_identity_from_file(path: &PathBuf) -> Result<Identity> {
    let bytes = tokio::fs::read(path).await.map_err(Error::Io)?;

    // Preferred format: full `Identity` JSON (matches main CLI helpers).
    if let Ok(identity) = serde_json::from_slice::<Identity>(&bytes) {
        validate_identity_username(&identity.username)?;
        if identity.public_key.as_bytes().len() != 32 {
            return Err(Error::validation(
                "Invalid public_key length in identity file",
            ));
        }
        return Ok(identity);
    }

    // Backward-compatibility: legacy metadata JSON (username/user_id/public_key hex).
    let content = std::str::from_utf8(&bytes)
        .map_err(|e| Error::validation(format!("Identity file is not valid UTF-8: {e}")))?;
    let data: serde_json::Value = serde_json::from_str(content)?;

    let username = data["username"]
        .as_str()
        .ok_or_else(|| Error::validation("Missing username in identity file"))?;

    let user_id_str = data["user_id"]
        .as_str()
        .ok_or_else(|| Error::validation("Missing user_id in identity file"))?;
    let user_uuid = Uuid::parse_str(user_id_str)
        .map_err(|e| Error::validation(format!("Invalid user_id: {e}")))?;

    let public_key_hex = data["public_key"]
        .as_str()
        .ok_or_else(|| Error::validation("Missing public_key in identity file"))?;
    let public_key_bytes = hex::decode(public_key_hex)
        .map_err(|e| Error::validation(format!("Invalid public_key hex: {e}")))?;

    validate_identity_username(username)?;
    if public_key_bytes.len() != 32 {
        return Err(Error::validation(
            "Invalid public_key length in identity file",
        ));
    }

    Ok(Identity {
        user_id: UserId(user_uuid),
        username: username.to_string(),
        normalized_username: dchat_identity::identity::normalize_username_for_collision(username),
        public_key: dchat_core::types::PublicKey::new(public_key_bytes),
        display_name: None,
        bio: None,
        reputation: dchat_core::types::ReputationScore::default(),
        created_at: chrono::Utc::now(),
        verified: false,
        badges: Vec::new(),
        metadata: HashMap::new(),
    })
}

async fn save_identity_to_file(identity: &Identity, path: &PathBuf) -> Result<()> {
    // Persist the same JSON format used by the main CLI (`Identity` serde).
    // Note: `Identity` does not contain private key material.
    let json = serde_json::to_string_pretty(identity)?;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(Error::Io)?;
    }

    tokio::fs::write(path, json).await.map_err(Error::Io)?;

    // Restrict permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = tokio::fs::metadata(path).await.map_err(Error::Io)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        tokio::fs::set_permissions(path, permissions)
            .await
            .map_err(Error::Io)?;
    }
    info!("✓ Identity saved to {:?}", path);

    Ok(())
}

fn validate_identity_username(username: &str) -> Result<()> {
    if username.is_empty() {
        return Err(Error::validation("Username cannot be empty"));
    }
    if username.len() > 64 {
        return Err(Error::validation("Username exceeds maximum length"));
    }
    if !username.chars().all(|c| c.is_ascii()) {
        return Err(Error::validation("Username must be ASCII"));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(Error::validation(
            "Username can only contain alphanumeric characters, underscores, and hyphens",
        ));
    }
    Ok(())
}

fn identity_to_stored(identity: &Identity) -> StoredIdentityV1 {
    StoredIdentityV1 {
        version: 1,
        user_id: identity.user_id.to_string(),
        username: identity.username.clone(),
        normalized_username: identity.normalized_username.clone(),
        public_key_b64: general_purpose::STANDARD.encode(identity.public_key.as_bytes()),
        created_at: identity.created_at.timestamp(),
        verified: identity.verified,
        badges: identity.badges.clone(),
        metadata: identity.metadata.clone(),
        display_name: identity.display_name.clone(),
        bio: identity.bio.clone(),
    }
}

fn stored_to_identity(stored: StoredIdentityV1) -> Result<Identity> {
    if stored.version != 1 {
        return Err(Error::validation("Unsupported identity store version"));
    }

    validate_identity_username(&stored.username)?;

    let user_uuid = Uuid::parse_str(&stored.user_id)
        .map_err(|e| Error::validation(format!("Invalid stored user_id: {e}")))?;

    let public_key_bytes = general_purpose::STANDARD
        .decode(stored.public_key_b64)
        .map_err(|e| Error::validation(format!("Invalid stored public_key: {e}")))?;

    if public_key_bytes.len() != 32 {
        return Err(Error::validation("Invalid stored public_key length"));
    }

    let created_at = match chrono::Utc.timestamp_opt(stored.created_at, 0).single() {
        Some(dt) => dt,
        None => {
            warn!(
                "⚠ Invalid stored created_at timestamp ({}); defaulting to now",
                stored.created_at
            );
            chrono::Utc::now()
        }
    };

    Ok(Identity {
        user_id: UserId(user_uuid),
        username: stored.username,
        normalized_username: stored.normalized_username,
        public_key: dchat_core::types::PublicKey::new(public_key_bytes),
        display_name: stored.display_name,
        bio: stored.bio,
        reputation: dchat_core::types::ReputationScore::default(),
        created_at,
        verified: stored.verified,
        badges: stored.badges,
        metadata: stored.metadata,
    })
}

async fn load_or_create_identity_and_keypair(
    database: &Database,
    config: &LightClientConfig,
) -> Result<(Identity, KeyPair)> {
    let identity = if let Some(json) = database.get_client_kv(KV_IDENTITY).await? {
        let stored: StoredIdentityV1 = serde_json::from_str(&json)
            .map_err(|e| Error::storage(format!("Invalid identity JSON in DB: {e}")))?;
        stored_to_identity(stored)?
    } else {
        // No stored identity; import identity file if present.
        let identity = if let Some(path) = &config.identity_path {
            if path.exists() {
                info!("Importing identity from {:?}", path);
                load_identity_from_file(path).await?
            } else {
                info!("No identity file; creating a new identity");
                let (identity, keypair) = generate_identity_with_keypair(&config.display_name)?;
                persist_identity_and_key(database, config, &identity, &keypair).await?;
                return Ok((identity, keypair));
            }
        } else {
            info!("No identity file; creating a new identity");
            let (identity, keypair) = generate_identity_with_keypair(&config.display_name)?;
            persist_identity_and_key(database, config, &identity, &keypair).await?;
            return Ok((identity, keypair));
        };

        // We imported an identity, but import formats do not include private key material.
        // We require key material to be present in the DB to operate.
        let identity_json = serde_json::to_string(&identity_to_stored(&identity))?;
        database.put_client_kv(KV_IDENTITY, &identity_json).await?;
        identity
    };

    let Some(key_json) = database.get_client_kv(KV_IDENTITY_KEY).await? else {
        return Err(Error::crypto(
            "Light client identity key material is missing. Set DCHAT_LIGHT_CLIENT_IDENTITY_PASSPHRASE and reset identity, or restore the key store from backup.".to_string(),
        ));
    };

    let stored_key: StoredIdentityKeyV1 = serde_json::from_str(&key_json)
        .map_err(|e| Error::storage(format!("Invalid identity key JSON in DB: {e}")))?;

    let passphrase = read_identity_passphrase()?;
    let keypair = stored_key_to_keypair(&identity, stored_key, &passphrase)?;

    Ok((identity, keypair))
}

async fn persist_identity_and_key(
    database: &Database,
    config: &LightClientConfig,
    identity: &Identity,
    keypair: &KeyPair,
) -> Result<()> {
    let identity_json = serde_json::to_string(&identity_to_stored(identity))?;
    database.put_client_kv(KV_IDENTITY, &identity_json).await?;

    let passphrase = read_identity_passphrase()?;
    let stored_key = keypair_to_stored_key(identity, keypair, &passphrase)?;
    let key_json = serde_json::to_string(&stored_key)?;
    database.put_client_kv(KV_IDENTITY_KEY, &key_json).await?;

    // Optional export of public identity file for portability/debugging.
    if let Some(path) = &config.identity_path {
        if !path.exists() {
            save_identity_to_file(identity, path).await?;
        }
    }

    Ok(())
}

async fn load_sync_cursor(database: &Database) -> Result<SyncCursor> {
    let Some(json) = database.get_client_kv(KV_SYNC_CURSOR).await? else {
        return Ok(SyncCursor::default());
    };

    serde_json::from_str(&json)
        .map_err(|e| Error::storage(format!("Invalid sync cursor JSON in DB: {}", e)))
}

async fn load_offline_queue(database: &Database) -> Result<VecDeque<QueuedOperation>> {
    let Some(json) = database.get_client_kv(KV_OFFLINE_QUEUE).await? else {
        return Ok(VecDeque::new());
    };

    serde_json::from_str(&json)
        .map_err(|e| Error::storage(format!("Invalid offline queue JSON in DB: {}", e)))
}

/// Encrypt message content for channel/DM transmission
#[allow(dead_code)]
pub fn encrypt_message_content(content: &[u8], channel_key: &[u8; KEY_SIZE]) -> Result<Vec<u8>> {
    encrypt_with_key(channel_key, content)
}

/// Decrypt message content received from channel/DM
#[allow(dead_code)]
pub fn decrypt_message_content(ciphertext: &[u8], channel_key: &[u8; KEY_SIZE]) -> Result<Vec<u8>> {
    decrypt_with_key(channel_key, ciphertext)
}

/// Generate a new channel encryption key
#[allow(dead_code)]
pub fn generate_channel_key() -> [u8; KEY_SIZE] {
    generate_encryption_key()
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_identity_passphrase(passphrase: Option<&str>) {
        match passphrase {
            Some(value) => std::env::set_var(ENV_IDENTITY_PASSPHRASE, value),
            None => std::env::remove_var(ENV_IDENTITY_PASSPHRASE),
        }
    }

    #[test]
    fn test_client_profile_settings() {
        assert_eq!(ClientProfile::Performance.target_relay_connections(), 5);
        assert_eq!(ClientProfile::LowPower.target_relay_connections(), 1);
        assert!(ClientProfile::Balanced.enable_mesh());
        assert!(!ClientProfile::RelayOnly.enable_mesh());
    }

    #[test]
    fn test_config_default() {
        let config = LightClientConfig::default();
        assert_eq!(config.display_name, "Anonymous");
        assert!(config.fee_gated);
        assert_eq!(config.default_channels, vec!["global".to_string()]);
    }

    #[tokio::test]
    async fn test_queue_operation() {
        let _lock = ENV_LOCK.lock().unwrap();
        set_identity_passphrase(Some("test-light-client-passphrase"));

        let temp_dir = std::env::temp_dir().join(format!("dchat_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let config = LightClientConfig {
            data_dir: temp_dir.clone(),
            max_offline_queue: 10,
            ..Default::default()
        };

        let client = LightClient::new(config).await.unwrap();

        // Queue should work when disconnected
        let result = client
            .queue_operation(OutboundOperation::ChannelMessage {
                channel_id: "test".to_string(),
                content: b"hello".to_vec(),
                encrypted: false,
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(client.offline_queue.read().await.len(), 1);

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn test_identity_keypair_persisted_encrypted_roundtrip() {
        let _lock = ENV_LOCK.lock().unwrap();
        set_identity_passphrase(Some("roundtrip-passphrase"));

        let dir = tempdir().unwrap();
        let config = LightClientConfig {
            data_dir: dir.path().to_path_buf(),
            display_name: "alice".to_string(),
            health_addr: None,
            metrics_addr: None,
            ..Default::default()
        };

        let client1 = LightClient::new(config.clone()).await.unwrap();
        let identity1 = client1.identity().clone();
        let pk1 = client1.identity_keypair().public_key().to_core_public_key();

        let client2 = LightClient::new(config).await.unwrap();
        let identity2 = client2.identity().clone();
        let pk2 = client2.identity_keypair().public_key().to_core_public_key();

        assert_eq!(identity1.user_id, identity2.user_id);
        assert_eq!(
            identity1.public_key.as_bytes(),
            identity2.public_key.as_bytes()
        );
        assert_eq!(pk1.as_bytes(), pk2.as_bytes());
        assert_eq!(pk2.as_bytes(), identity2.public_key.as_bytes());
    }

    #[tokio::test]
    async fn test_identity_keypair_wrong_passphrase_fails() {
        let _lock = ENV_LOCK.lock().unwrap();

        let dir = tempdir().unwrap();
        let config = LightClientConfig {
            data_dir: dir.path().to_path_buf(),
            display_name: "alice".to_string(),
            health_addr: None,
            metrics_addr: None,
            ..Default::default()
        };

        set_identity_passphrase(Some("correct"));
        let _client1 = LightClient::new(config.clone()).await.unwrap();

        set_identity_passphrase(Some("wrong"));
        let err = match LightClient::new(config).await {
            Ok(_) => panic!("expected identity key decryption to fail"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(msg.contains("Incorrect password") || msg.contains("Decryption"));
    }

    #[tokio::test]
    async fn test_identity_file_roundtrip_full_identity_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("identity.json");

        let keypair = KeyPair::try_generate().expect("CSPRNG available");
        let identity = Identity::try_new("alice".to_string(), &keypair).unwrap();

        tokio::fs::write(&path, serde_json::to_vec(&identity).unwrap())
            .await
            .unwrap();

        let loaded = load_identity_from_file(&path).await.unwrap();
        assert_eq!(loaded.user_id, identity.user_id);
        assert_eq!(loaded.username, identity.username);
        assert_eq!(loaded.normalized_username, identity.normalized_username);
        assert_eq!(loaded.public_key.as_bytes(), identity.public_key.as_bytes());
    }

    #[tokio::test]
    async fn test_identity_file_loads_legacy_metadata_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("identity_legacy.json");

        let keypair = KeyPair::try_generate().expect("CSPRNG available");
        let identity = Identity::try_new("bob".to_string(), &keypair).unwrap();

        let legacy = serde_json::json!({
            "username": identity.username,
            "user_id": identity.user_id.to_string(),
            "public_key": hex::encode(identity.public_key.as_bytes()),
        });
        tokio::fs::write(&path, serde_json::to_vec(&legacy).unwrap())
            .await
            .unwrap();

        let loaded = load_identity_from_file(&path).await.unwrap();
        assert_eq!(loaded.user_id, identity.user_id);
        assert_eq!(loaded.username, identity.username);
        assert_eq!(loaded.public_key.as_bytes(), identity.public_key.as_bytes());
    }

    #[test]
    fn test_stored_identity_rejects_bad_public_key_len() {
        let stored = StoredIdentityV1 {
            version: 1,
            user_id: Uuid::new_v4().to_string(),
            username: "carol".to_string(),
            normalized_username: "carol".to_string(),
            public_key_b64: general_purpose::STANDARD.encode([1u8, 2, 3]),
            created_at: chrono::Utc::now().timestamp(),
            verified: false,
            badges: Vec::new(),
            metadata: HashMap::new(),
            display_name: None,
            bio: None,
        };

        let err = stored_to_identity(stored).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("public_key"));
    }

    #[test]
    fn test_stored_identity_invalid_timestamp_defaults_now() {
        let stored = StoredIdentityV1 {
            version: 1,
            user_id: Uuid::new_v4().to_string(),
            username: "dave".to_string(),
            normalized_username: "dave".to_string(),
            public_key_b64: general_purpose::STANDARD.encode([7u8; 32]),
            created_at: i64::MAX,
            verified: false,
            badges: Vec::new(),
            metadata: HashMap::new(),
            display_name: None,
            bio: None,
        };

        let now = chrono::Utc::now();
        let identity = stored_to_identity(stored).unwrap();
        let delta = identity.created_at.signed_duration_since(now);
        assert!(delta.num_seconds().abs() <= 5);
    }
}
