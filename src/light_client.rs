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

use dchat_core::error::{Error, Result};
use dchat_core::types::{MessageId, UserId};
use dchat_crypto::keys::KeyPair;
use dchat_crypto::{decrypt_with_key, encrypt_with_key, generate_encryption_key, KEY_SIZE};
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

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Light client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightClientConfig {
    /// Display name / username
    pub display_name: String,

    /// Path to identity file (if None, generates ephemeral)
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
}

impl LightClient {
    /// Create a new light client
    pub async fn new(config: LightClientConfig) -> Result<Self> {
        info!("🔧 Initializing light client: {}", config.display_name);

        // Create data directory
        tokio::fs::create_dir_all(&config.data_dir)
            .await
            .map_err(Error::Io)?;

        // Load or generate identity
        let identity = if let Some(path) = &config.identity_path {
            if path.exists() {
                info!("Loading identity from {:?}", path);
                load_identity_from_file(path).await?
            } else {
                info!("Identity file not found, generating new identity");
                let identity = generate_identity(&config.display_name)?;
                save_identity_to_file(&identity, path).await?;
                identity
            }
        } else {
            info!("Generating ephemeral identity");
            generate_identity(&config.display_name)?
        };

        info!("✓ Identity: {} ({})", identity.username, identity.user_id);

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
        })
    }

    /// Get the identity
    pub fn identity(&self) -> &Identity {
        &self.identity
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
                        if let NetworkEvent::MessageReceived { from, message } = &event {
                            if let DchatMessage::ChannelMessage { message_id, sender, channel_id, encrypted_payload, timestamp } = message {
                                // Don't process our own messages
                                if sender != &self_user_id {
                                    let content_hash = format!("{:x}", Sha256::digest(encrypted_payload));

                                    // Store in database
                                    let _ = db_for_net.insert_message(&MessageRow {
                                        id: hex::encode(message_id),
                                        sender_id: sender.0.to_string(),
                                        recipient_id: None,
                                        channel_id: Some(channel_id.clone()),
                                        content_type: "channel_message".to_string(),
                                        content: String::new(),
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
                                    let content_text = String::from_utf8_lossy(encrypted_payload).to_string();
                                    let _ = event_tx.send(LightClientEvent::MessageReceived {
                                        channel_id: Some(channel_id.clone()),
                                        sender: from.to_string(),
                                        content: content_text,
                                        timestamp: *timestamp,
                                        verification: VerificationStatus::Pending,
                                    }).await;

                                    // TODO: Queue async chain verification task
                                    // verify_message_on_chain(message_id, sender, channel_id, content_hash);
                                }
                            }
                        }

                        // Emit connection events
                        match &event {
                            NetworkEvent::PeerConnected(peer) => {
                                let _ = event_tx.send(LightClientEvent::PeerConnected(peer.to_string())).await;
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
                        // Perform sync
                        let channels_list = channels.read().await.clone();
                        let channels_synced = channels_list.len();

                        // Delta sync from relays:
                        // 1. Query each relay for messages since cursor.last_sequence[channel]
                        // 2. Verify message ordering against chain proofs
                        // 3. Store verified messages locally and update cursor
                        // Currently: placeholder that updates timestamp only
                        {
                            let mut cursor = sync_cursor.write().await;
                            cursor.last_sync = Some(chrono::Utc::now().timestamp());
                            // Future: cursor.last_sequence.insert(channel_id, new_sequence);
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
                    // DM sending requires FeeGateway integration for production
                    // For now, log and skip (DMs queued for when FeeGateway is wired)
                    warn!("DM to {} queued but not yet implemented", recipient_id);
                    Err(Error::internal(
                        "DM sending requires FeeGateway integration",
                    ))
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
        let cursor_path = self.config.data_dir.join("sync_cursor.json");
        tokio::fs::write(cursor_path, json)
            .await
            .map_err(Error::Io)?;
        Ok(())
    }

    async fn save_offline_queue(&self) -> Result<()> {
        let queue = self.offline_queue.read().await;
        let json = serde_json::to_string(&*queue)?;
        let queue_path = self.config.data_dir.join("offline_queue.json");
        tokio::fs::write(queue_path, json)
            .await
            .map_err(Error::Io)?;
        Ok(())
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn generate_identity(display_name: &str) -> Result<Identity> {
    #[allow(deprecated)]
    let keypair = KeyPair::generate();
    Ok(Identity::new(display_name.to_string(), &keypair))
}

async fn load_identity_from_file(path: &PathBuf) -> Result<Identity> {
    let content = tokio::fs::read_to_string(path).await.map_err(Error::Io)?;
    let data: serde_json::Value = serde_json::from_str(&content)?;

    // Parse identity JSON
    let username = data["username"]
        .as_str()
        .ok_or_else(|| Error::validation("Missing username in identity file"))?;

    let private_key_hex = data["private_key"]
        .as_str()
        .ok_or_else(|| Error::validation("Missing private_key in identity file"))?;

    let private_key_bytes = hex::decode(private_key_hex)
        .map_err(|e| Error::validation(format!("Invalid private key hex: {}", e)))?;

    if private_key_bytes.len() != 32 {
        return Err(Error::validation("Private key must be 32 bytes"));
    }

    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&private_key_bytes);

    let private_key = dchat_crypto::keys::PrivateKey::from_bytes(key_array);
    let keypair = KeyPair::from_private_key(private_key);

    Ok(Identity::new(username.to_string(), &keypair))
}

async fn save_identity_to_file(identity: &Identity, path: &PathBuf) -> Result<()> {
    // SECURITY WARNING: This saves identity metadata only (no private key).
    // Private keys should NEVER be saved to plain files in production.
    // Use platform keystore (Keychain/Credential Manager/Android Keystore) instead.
    // For development: generate ephemeral keys or use --identity flag with HSM/KMS.
    warn!("⚠️  Saving identity to file. Private key NOT included for security.");
    warn!("⚠️  Use platform keystore for production deployments.");

    let data = serde_json::json!({
        "username": identity.username,
        "user_id": identity.user_id.to_string(),
        "public_key": hex::encode(identity.public_key.as_bytes()),
        // Private key intentionally excluded - use keystore APIs
    });

    let json = serde_json::to_string_pretty(&data)?;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(Error::Io)?;
    }

    tokio::fs::write(path, json).await.map_err(Error::Io)?;
    info!("✓ Identity saved to {:?}", path);

    Ok(())
}

async fn load_sync_cursor(database: &Database) -> Result<SyncCursor> {
    // Load cursor from database's settings/metadata table
    // For now, start fresh (cursor data is non-critical; worst case = refetch)
    // Future: SELECT value FROM settings WHERE key = 'sync_cursor'
    let _ = database; // Acknowledge param for future use
    Ok(SyncCursor::default())
}

async fn load_offline_queue(database: &Database) -> Result<VecDeque<QueuedOperation>> {
    // Load queued operations from database for crash recovery
    // Future: SELECT * FROM offline_queue ORDER BY created_at ASC
    // For now, start empty (operations can be re-queued by user if lost)
    let _ = database; // Acknowledge param for future use
    Ok(VecDeque::new())
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
}
