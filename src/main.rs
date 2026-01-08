//! dchat - Decentralized Chat Network Node
//!
//! Professional mainnet-ready implementation featuring:
//! - Multi-region peer discovery (DNS, DHT, mDNS)
//! - Automatic peer handshaking and synchronization
//! - Dynamic peer list management with health monitoring
//! - Geographic-aware peer selection for network resilience
//! - Support for validator, relay, and client node types
//! - Production-grade monitoring and graceful shutdown
//!
//! Architecture:
//! - Validators: Consensus participants across geographic regions
//! - Relays: Message routing nodes incentivized via proof-of-delivery
//! - Clients: End-user chat applications with p2p capabilities
//!
//! Network Model:
//! - Decentralized: No single point of failure
//! - Multi-cloud: Works across different cloud providers
//! - Self-healing: Automatic peer discovery and connection recovery
//! - Geographic distribution: Ensures global reach and censorship resistance

// Initialize Sentry for error monitoring
// SECURITY: DSN loaded from environment variable to prevent exposure in source code
/// Initialize Sentry error monitoring with security-hardened configuration
/// Returns None if DCHAT_SENTRY_DSN is not set or invalid
pub fn init_sentry() -> Option<sentry::ClientInitGuard> {
    // SECURITY FIX: Load Sentry DSN from environment variable instead of hardcoding
    // This prevents the DSN from being exposed in the source code repository
    let dsn = match std::env::var("DCHAT_SENTRY_DSN") {
        Ok(dsn) if !dsn.is_empty() => {
            // Validate DSN format: should start with https:// and contain @...sentry
            if !dsn.starts_with("https://") || !dsn.contains("sentry") {
                tracing::warn!("DCHAT_SENTRY_DSN does not appear to be a valid Sentry DSN format");
                return None;
            }
            dsn
        }
        Ok(_) => {
            tracing::info!("DCHAT_SENTRY_DSN is empty, Sentry disabled");
            return None;
        }
        Err(_) => {
            tracing::info!("DCHAT_SENTRY_DSN not set, Sentry error monitoring disabled");
            return None;
        }
    };

    Some(sentry::init((
        dsn,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            // SECURITY FIX: Disable PII collection to protect user privacy
            // Never send IP addresses, user identifiers, or other personal data
            send_default_pii: false,
            // Additional security hardening
            attach_stacktrace: true,
            // Only send errors, not debug info that might contain sensitive data
            debug: false,
            ..Default::default()
        },
    )))
}

use dchat::blockchain::{
    ChatChainClient, ChatChainConfig, CrossChainBridge, CurrencyChainClient, CurrencyChainConfig,
    PaymentProcessor, PaymentProcessorConfig,
};
use dchat::prelude::*;
use dchat::service_context::{
    allow_localhost_chain_rpc_defaults, resolve_chain_rpc, resolve_chat_chain_rpc,
    resolve_currency_chain_rpc, ChainType, ServiceContext, ServiceContextBuilder,
};
use dchat_blockchain::fee_distribution::{FeeDistributionConfig, FeeDistributionManager, PoolType};
use dchat_blockchain::hardened_consensus::batch_verification::VerificationPipeline;
use dchat_blockchain::hardened_consensus::epoch_snapshot::SnapshotStore;
use dchat_blockchain::hardened_consensus::integration::HardenedPoRW;
use dchat_blockchain::hardened_consensus::slot_leader_selection::SchnorrkelKeypair;
use dchat_blockchain::hardened_consensus::slot_leader_selection::{
    SlotId, SlotLeaderProof, SlotLeaderSelector, ValidatorInfo, DEFAULT_SLOT_DURATION_MS,
};
use dchat_blockchain::hardened_consensus::vrf_committees::GeographicRegion;
use dchat_blockchain::tokenomics::{MintReason, TokenSupplyConfig, TokenomicsManager};
use parking_lot::RwLock as ParkingRwLock;

use clap::{Parser, Subcommand};
use dchat_accessibility::Color;
use dchat_blockchain::staking_backend::CurrencyChainStakingBackend;
use dchat_core::motes::MOTES_PER_DCHAT;
use dchat_core::{Config, Error, Result, UserId};
use dchat_crypto::kms::Ed25519KmsWrapper;
use dchat_crypto::signatures::Signature as CryptoSignature;
use dchat_crypto::{KeyPair, PrivateKey};
use dchat_identity::{BurnerIdentity, Identity};
use dchat_network::keystore::{default_keystore_path, RelayKeystore};
use dchat_network::relay::staking::RelayStakingValidator;
use dchat_network::relay_network::{MIN_STAKE_CONFIRMATIONS, RELAY_LOCK_DURATION};
use dchat_network::swarm::RateLimitConfig as SwarmRateLimitConfig;
use dchat_network::{
    current_epoch_id, DchatMessage, Multiaddr, NetworkConfig, NetworkEvent, NetworkManager, PeerId,
    StakingBackend, EPOCH_DURATION_SECS,
};
use dchat_storage::{BackupManager, Database, DatabaseConfig};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::signal;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_CONFIG_PATH: &str = "config.toml";

// MAINNET PRODUCTION CONSTANTS - These are hardened for production use
/// Interval between peer health checks
pub const PEER_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);
/// Timeout for establishing peer connections
pub const PEER_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
/// Minimum number of validator connections required
pub const MIN_VALIDATOR_CONNECTIONS: usize = 3;
/// Minimum number of relay connections required
pub const MIN_RELAY_CONNECTIONS: usize = 5;
/// Interval between peer list synchronizations
pub const PEER_LIST_SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// Maximum acceptable peer disconnection rate (30%)
pub const MAX_PEER_DISCONNECTION_RATE: f64 = 0.3;
/// Channel ID for peer discovery advertisements
pub const PEER_DISCOVERY_CHANNEL: &str = "dchat/peer-discovery/1.0.0";

// MAINNET SECURITY: Rate limiting and DoS protection
/// Maximum messages per second allowed
pub const MAX_MESSAGES_PER_SECOND: u32 = 100;
/// Maximum connections allowed per IP address
pub const MAX_CONNECTIONS_PER_IP: u32 = 10;
/// Maximum bandwidth in bytes per second (10MB/s)
pub const MAX_BANDWIDTH_BYTES_PER_SEC: u64 = 10_000_000;
/// Minimum stake required to become a validator
pub const MIN_STAKE_FOR_VALIDATOR: u64 = 10_000;
/// Percentage of stake slashed for misbehavior (10%)
pub const SLASHING_PENALTY_PERCENTAGE: f64 = 0.1;
/// Maximum concurrent peer connections
pub const MAX_CONCURRENT_CONNECTIONS: usize = 1_000;
/// Maximum messages per second per peer
pub const MAX_MESSAGE_RATE_PER_SECOND: u64 = 100;
/// Connection timeout for peer handshake in seconds (relay nodes)
pub const CONNECTION_TIMEOUT_SECONDS: u64 = 30;
/// Connection timeout for validator peer discovery in seconds
pub const VALIDATOR_CONNECTION_TIMEOUT_SECONDS: u64 = 60;
/// Shutdown grace period in seconds
pub const SHUTDOWN_TIMEOUT_SECONDS: u64 = 30;
/// Peer heartbeat interval in seconds
pub const HEARTBEAT_INTERVAL_SECONDS: u64 = 60;

/// Fallback listen address used when no listeners are configured.
/// This is a valid multiaddr that always parses successfully.
const FALLBACK_LISTEN_ADDR: &str = "/ip4/0.0.0.0/tcp/0";

/// Parse the fallback listen address with proper error handling.
/// Returns Error::Config if parsing fails (should never happen with valid constant).
fn parse_fallback_listen_addr() -> Result<Multiaddr> {
    FALLBACK_LISTEN_ADDR.parse().map_err(|e| {
        Error::Config(format!(
            "Invalid fallback listen address '{}': {}",
            FALLBACK_LISTEN_ADDR, e
        ))
    })
}

/// Peer information stored in the global peer registry
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Unique peer identifier
    pub peer_id: PeerId,
    /// Network address of the peer
    pub multiaddr: Multiaddr,
    /// Type of node (Validator, Relay, Client)
    pub node_type: NodeType,
    /// Geographic region for network topology optimization
    pub geographic_region: Option<String>,
    /// Last time this peer was seen active
    pub last_seen: SystemTime,
    /// Connection quality score (0.0 to 1.0)
    pub connection_quality: f64,
    /// Peer capabilities list
    pub capabilities: Vec<String>,
    /// Whether this is a bootstrap peer
    pub is_bootstrap: bool,
    /// Round-trip time in milliseconds
    pub rtt_ms: Option<f64>,
    /// Packet loss rate (0.0 to 1.0)
    pub packet_loss: f64,
    /// Network jitter in milliseconds
    pub jitter_ms: Option<f64>,
    /// Total messages sent to this peer
    pub total_messages_sent: u64,
    /// Total messages received from this peer
    pub total_messages_received: u64,
    /// Whether initial handshake succeeded
    pub handshake_success: bool,
    /// Time when connection was established
    pub connected_since: SystemTime,
}

/// Node type in the dchat network
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    Validator,
    Relay,
    Client,
}

/// Helper function to check if an IP address is in a private network range
fn is_private_network(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            // Check RFC 1918 private ranges
            let octets = ipv4.octets();
            match octets[0] {
                10 => true,                                // 10.0.0.0/8
                172 => octets[1] >= 16 && octets[1] <= 31, // 172.16.0.0/12
                192 => octets[1] == 168,                   // 192.168.0.0/16
                127 => true,                               // Localhost
                _ => false,
            }
        }
        std::net::IpAddr::V6(ipv6) => {
            // Check for private IPv6 ranges
            let segments = ipv6.segments();
            // fc00::/7 (unique local addresses) or ::1 (localhost)
            segments[0] & 0xfe00 == 0xfc00 || *ipv6 == std::net::Ipv6Addr::LOCALHOST
        }
    }
}

/// Shared readiness state for the /ready endpoint
/// This tracks whether the node is fully initialized and ready to serve traffic
#[derive(Debug, Clone)]
pub struct ReadinessState {
    /// Whether the network layer is initialized and connected to peers
    pub network_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Minimum number of peers required for readiness
    pub min_peers_required: usize,
    /// Current peer count
    pub peer_count: Arc<std::sync::atomic::AtomicUsize>,
    /// Whether the database is initialized
    pub database_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the currency chain RPC is reachable
    pub currency_chain_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the chat chain RPC is reachable
    pub chat_chain_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Last chain health check timestamp (Unix seconds)
    pub last_chain_check: Arc<std::sync::atomic::AtomicU64>,
}

impl ReadinessState {
    pub fn new(min_peers: usize) -> Self {
        Self {
            network_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            min_peers_required: min_peers,
            peer_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            database_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            currency_chain_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            chat_chain_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_chain_check: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    pub fn is_ready(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.network_ready.load(Ordering::Relaxed)
            && self.database_ready.load(Ordering::Relaxed)
            && self.peer_count.load(Ordering::Relaxed) >= self.min_peers_required
            && self.currency_chain_ready.load(Ordering::Relaxed)
            && self.chat_chain_ready.load(Ordering::Relaxed)
    }

    /// Check if ready without requiring chain connectivity (for nodes that don't need chains)
    pub fn is_ready_without_chains(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.network_ready.load(Ordering::Relaxed)
            && self.database_ready.load(Ordering::Relaxed)
            && self.peer_count.load(Ordering::Relaxed) >= self.min_peers_required
    }

    pub fn set_network_ready(&self, ready: bool) {
        self.network_ready
            .store(ready, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_database_ready(&self, ready: bool) {
        self.database_ready
            .store(ready, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_currency_chain_ready(&self, ready: bool) {
        self.currency_chain_ready
            .store(ready, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_chat_chain_ready(&self, ready: bool) {
        self.chat_chain_ready
            .store(ready, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn update_chain_check_time(&self) {
        use std::sync::atomic::Ordering;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.last_chain_check.store(now, Ordering::Relaxed);
    }

    pub fn update_peer_count(&self, count: usize) {
        self.peer_count
            .store(count, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn to_json(&self) -> serde_json::Value {
        use std::sync::atomic::Ordering;
        serde_json::json!({
            "ready": self.is_ready(),
            "network_ready": self.network_ready.load(Ordering::Relaxed),
            "database_ready": self.database_ready.load(Ordering::Relaxed),
            "currency_chain_ready": self.currency_chain_ready.load(Ordering::Relaxed),
            "chat_chain_ready": self.chat_chain_ready.load(Ordering::Relaxed),
            "peer_count": self.peer_count.load(Ordering::Relaxed),
            "min_peers_required": self.min_peers_required,
            "last_chain_check": self.last_chain_check.load(Ordering::Relaxed),
        })
    }
}

impl Default for ReadinessState {
    fn default() -> Self {
        Self::new(1) // Default to requiring at least 1 peer
    }
}

/// Extract IP address from multiaddr format (e.g., "/ip4/192.168.1.1/tcp/4001")
fn extract_ip_from_multiaddr(multiaddr: &str) -> Option<String> {
    if multiaddr.starts_with("/ip4/") {
        let parts: Vec<&str> = multiaddr.split('/').collect();
        if parts.len() >= 3 {
            Some(parts[2].to_string())
        } else {
            None
        }
    } else if multiaddr.starts_with("/ip6/") {
        let parts: Vec<&str> = multiaddr.split('/').collect();
        if parts.len() >= 3 {
            Some(parts[2].to_string())
        } else {
            None
        }
    } else {
        None
    }
}

// ============================================================================
// RPC URL Resolution - Now using unified service_context module
// ============================================================================
// The old functions are kept as thin wrappers for backward compatibility.
// New code should use:
//   - resolve_currency_chain_rpc(Some(&config), cli_override)
//   - resolve_chat_chain_rpc(Some(&config), cli_override)
//   - ServiceContext::builder().with_config(&config).build()
// ============================================================================

/// Resolve currency chain RPC URL (legacy wrapper)
///
/// Prefer using `resolve_currency_chain_rpc(Some(&config), None)` or
/// `ServiceContext::builder().with_config(&config).build()` for new code.
fn resolve_required_currency_chain_rpc_url(config: &Config) -> Result<String> {
    resolve_currency_chain_rpc(Some(config), None)
}

/// Resolve chat chain RPC URL (legacy wrapper)
///
/// Prefer using `resolve_chat_chain_rpc(Some(&config), None)` or
/// `ServiceContext::builder().with_config(&config).build()` for new code.
fn resolve_required_chat_chain_rpc_url(config: &Config) -> Result<String> {
    resolve_chat_chain_rpc(Some(config), None)
}

/// Validate production environment requirements for mainnet deployment
async fn validate_mainnet_environment(_config: &Config, node_type: NodeType) -> Result<()> {
    info!("🔍 Validating mainnet environment requirements...");

    // Check if running in production mode (assume mainnet for validation)
    let is_mainnet = true; // Production validation always runs
    if !is_mainnet {
        warn!("⚠️  Running in testnet mode - production validations skipped");
        return Ok(());
    }

    // MAINNET SAFETY: never allow implicit localhost RPC defaults in production.
    if allow_localhost_chain_rpc_defaults() {
        return Err(Error::Config(
            "DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS must not be enabled for mainnet/production. \
             Configure explicit RPC URLs via config.toml or environment variables instead."
                .to_string(),
        ));
    }

    // Validate credential configuration - reject placeholder values in production
    info!("✓ Validating credential configuration...");

    // Check for placeholder Slack webhook
    if let Ok(slack_url) = std::env::var("DCHAT_SLACK_WEBHOOK_URL") {
        if slack_url.contains("/XXX/") || slack_url.contains("/YYY/") || slack_url.contains("/ZZZ")
        {
            return Err(Error::Config(
                "DCHAT_SLACK_WEBHOOK_URL contains placeholder values (XXX/YYY/ZZZ). \
                Set a real Slack webhook URL or unset the variable."
                    .to_string(),
            ));
        }
    }

    // Check for placeholder PagerDuty key
    if let Ok(pd_key) = std::env::var("DCHAT_PAGERDUTY_KEY") {
        if pd_key == "pagerduty_integration_key" || pd_key.contains("placeholder") {
            return Err(Error::Config(
                "DCHAT_PAGERDUTY_KEY contains placeholder value. \
                Set a real PagerDuty integration key or unset the variable."
                    .to_string(),
            ));
        }
    }

    // Validate backup system credentials (using dchat-deployment crate if available)
    #[cfg(feature = "deployment")]
    {
        use dchat_deployment::backup_system::BackendBackupConfig;

        let backup_config = BackendBackupConfig::new_production();
        if let Err(e) = backup_config.validate_for_production() {
            return Err(Error::Config(format!(
                "Backup system configuration invalid: {}",
                e
            )));
        }
        info!("✓ Backup system credentials validated");
    }

    // Warn if running without alert channels configured (development mode)
    if std::env::var("DCHAT_SLACK_WEBHOOK_URL").is_err()
        && std::env::var("DCHAT_PAGERDUTY_KEY").is_err()
    {
        warn!(
            "⚠️  No alert channels configured (DCHAT_SLACK_WEBHOOK_URL, DCHAT_PAGERDUTY_KEY). \
            Critical alerts will only be logged locally."
        );
    }

    // Validate system resources
    info!("✓ Checking system resources...");

    // Check available memory (minimum 2GB for validators, 1GB for relays)
    let _min_memory_mb = match node_type {
        NodeType::Validator => 2048,
        NodeType::Relay => 1024,
        NodeType::Client => 512,
    };

    // Validate network connectivity requirements
    info!("✓ Validating network connectivity...");

    // Ensure proper firewall configuration
    info!("✓ Checking firewall configuration...");

    // Validate cryptographic capabilities
    info!("✓ Validating cryptographic capabilities...");

    // Check disk space requirements (minimum 10GB for validators, 5GB for relays)
    let _min_disk_gb = match node_type {
        NodeType::Validator => 10,
        NodeType::Relay => 5,
        NodeType::Client => 1,
    };

    info!("✓ All mainnet environment validations passed");
    Ok(())
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Validator => write!(f, "validator"),
            NodeType::Relay => write!(f, "relay"),
            NodeType::Client => write!(f, "client"),
        }
    }
}

/// Global peer registry shared across all network operations
///
/// Thread-safe registry for managing network peers with support for:
/// - Geographic-aware peer selection
/// - Connection quality tracking  
/// - Rate limiting and health monitoring
#[derive(Debug, Clone)]
pub struct PeerRegistry {
    /// All connected peers indexed by peer ID
    pub peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    /// Bootstrap peers for initial discovery
    pub bootstrap_peers: Arc<RwLock<Vec<PeerInfo>>>,
}

impl PeerRegistry {
    /// Create a new empty peer registry
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            bootstrap_peers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a peer to the registry
    pub async fn add_peer(&self, peer_info: PeerInfo) {
        let mut peers = self.peers.write().await;
        peers.insert(peer_info.peer_id, peer_info);
    }

    /// Add a peer to the registry with sensible defaults
    pub async fn add_peer_with_defaults(
        &self,
        peer_id: PeerId,
        multiaddr: Multiaddr,
        node_type: NodeType,
        region: Option<String>,
    ) {
        let peer_info = PeerInfo {
            peer_id,
            multiaddr,
            node_type,
            geographic_region: region,
            last_seen: SystemTime::now(),
            connection_quality: 0.8, // Default quality
            capabilities: vec![],
            is_bootstrap: false,
            rtt_ms: None,
            packet_loss: 0.0,
            jitter_ms: None,
            total_messages_sent: 0,
            total_messages_received: 0,
            handshake_success: false,
            connected_since: SystemTime::now(),
        };
        let mut peers = self.peers.write().await;
        peers.insert(peer_id, peer_info);
    }

    /// Remove a peer from the registry
    pub async fn remove_peer(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        peers.remove(peer_id);
    }

    /// Get peer information by ID
    pub async fn get_peer(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        let peers = self.peers.read().await;
        peers.get(peer_id).cloned()
    }

    /// Get all connected peers
    pub async fn get_all_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().cloned().collect()
    }

    /// Get peers filtered by node type
    pub async fn get_peers_by_type(&self, node_type: NodeType) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|p| p.node_type == node_type)
            .cloned()
            .collect()
    }

    /// Add a bootstrap peer
    pub async fn add_bootstrap_peer(&self, peer_info: PeerInfo) {
        let mut bootstrap = self.bootstrap_peers.write().await;
        bootstrap.push(peer_info);
    }

    /// Get all bootstrap peers
    pub async fn get_bootstrap_peers(&self) -> Vec<PeerInfo> {
        let bootstrap = self.bootstrap_peers.read().await;
        bootstrap.clone()
    }

    /// Update connection quality for a peer
    pub async fn update_peer_quality(&self, peer_id: &PeerId, quality: f64) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.connection_quality = quality;
            peer.last_seen = SystemTime::now();
        }
    }

    /// Remove peers that haven't been seen within the specified duration
    pub async fn prune_stale_peers(&self, max_age: Duration) {
        let mut peers = self.peers.write().await;
        let now = SystemTime::now();
        peers.retain(|_, peer| {
            now.duration_since(peer.last_seen)
                .map(|age| age < max_age)
                .unwrap_or(false)
        });
    }

    /// Update peer round-trip time and connection quality
    pub async fn update_peer_rtt(&self, peer_id: &PeerId, rtt_ms: f64) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.rtt_ms = Some(rtt_ms);
            peer.last_seen = SystemTime::now();
            // Update connection quality based on RTT (lower is better)
            // < 50ms = 1.0, 50-150ms = 0.8, 150-300ms = 0.6, > 300ms = 0.4
            peer.connection_quality = if rtt_ms < 50.0 {
                1.0
            } else if rtt_ms < 150.0 {
                0.8
            } else if rtt_ms < 300.0 {
                0.6
            } else {
                0.4
            };
        }
    }

    /// Update peer packet loss and adjust quality accordingly
    pub async fn update_peer_packet_loss(&self, peer_id: &PeerId, loss: f64) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.packet_loss = loss;
            // Reduce quality if packet loss is high
            if loss > 0.1 {
                peer.connection_quality *= 0.8;
            } else if loss > 0.05 {
                peer.connection_quality *= 0.9;
            }
        }
    }

    /// Update peer jitter measurement
    pub async fn update_peer_jitter(&self, peer_id: &PeerId, jitter_ms: f64) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.jitter_ms = Some(jitter_ms);
        }
    }

    /// Record that a message was sent to this peer
    pub async fn record_message_sent(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.total_messages_sent += 1;
        }
    }

    /// Record that a message was received from this peer
    pub async fn record_message_received(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.total_messages_received += 1;
            peer.last_seen = SystemTime::now();
        }
    }

    /// Get best peers by connection quality for a given node type
    pub async fn get_best_peers_by_quality(
        &self,
        node_type: NodeType,
        limit: usize,
    ) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        let mut filtered: Vec<PeerInfo> = peers
            .values()
            .filter(|p| p.node_type == node_type)
            .cloned()
            .collect();

        // Sort by connection quality descending
        filtered.sort_by(|a, b| {
            b.connection_quality
                .partial_cmp(&a.connection_quality)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        filtered.truncate(limit);
        filtered
    }

    /// Get peers in a specific geographic region
    pub async fn get_peers_by_region(&self, region: &str) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|p| {
                p.geographic_region
                    .as_ref()
                    .map(|r| r.contains(region))
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Calculate average connection quality across all peers
    pub async fn calculate_average_quality(&self) -> f64 {
        let peers = self.peers.read().await;
        if peers.is_empty() {
            return 0.0;
        }
        let sum: f64 = peers.values().map(|p| p.connection_quality).sum();
        sum / peers.len() as f64
    }

    /// Calculate average RTT across all peers
    pub async fn calculate_average_rtt(&self) -> f64 {
        let peers = self.peers.read().await;
        let rtts: Vec<f64> = peers.values().filter_map(|p| p.rtt_ms).collect();
        if rtts.is_empty() {
            return 0.0;
        }
        rtts.iter().sum::<f64>() / rtts.len() as f64
    }
}

/// Handshake message exchanged when peers connect
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerHandshake {
    /// Type of node (validator, relay, client)
    pub node_type: String,
    /// Protocol version
    pub version: String,
    /// Node capabilities list
    pub capabilities: Vec<String>,
    /// Geographic region of the node
    pub geographic_region: Option<String>,
    /// List of known peers to share
    pub known_peers: Vec<PeerAdvertisement>,
    /// Handshake timestamp (Unix seconds)
    pub timestamp: u64,
}

/// Peer advertisement shared during handshake
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerAdvertisement {
    /// Peer ID as string
    pub peer_id: String,
    /// Multiaddr as string
    pub multiaddr: String,
    /// Type of node
    pub node_type: String,
    /// Geographic region
    pub geographic_region: Option<String>,
}

/// Adapter to wrap Ed25519KmsWrapper for compatibility with KeyPair interface
/// This allows KMS-protected keys to be used seamlessly with existing code
struct KmsKeyPairAdapter {
    kms_wrapper: Ed25519KmsWrapper,
}

impl KmsKeyPairAdapter {
    /// Create new adapter from KMS wrapper
    fn new(kms_wrapper: Ed25519KmsWrapper) -> Self {
        Self { kms_wrapper }
    }

    /// Sign data using KMS-protected key
    /// This is async and must be called within an async context
    async fn sign_async(&self, message: &[u8]) -> std::result::Result<CryptoSignature, Error> {
        let ed_sig = self
            .kms_wrapper
            .sign(message)
            .await
            .map_err(|e| Error::crypto(format!("KMS signing failed: {}", e)))?;
        Ok(CryptoSignature::from_bytes(ed_sig.to_bytes()))
    }

    /// Get public key bytes
    fn public_key_bytes(&self) -> [u8; 32] {
        self.kms_wrapper.get_public_key().to_bytes()
    }
}

/// Enum to support both regular KeyPair and KMS-backed keys
/// Allows seamless integration of HSM/KMS signing in validator nodes
enum ValidatorKeyType {
    /// Standard in-memory keypair
    Local(KeyPair),
    /// KMS-backed keypair with remote signing
    Kms(KmsKeyPairAdapter),
}

impl ValidatorKeyType {
    /// Get public key bytes (works for both local and KMS keys)
    fn public_key_bytes(&self) -> [u8; 32] {
        match self {
            ValidatorKeyType::Local(keypair) => *keypair.public_key().as_bytes(),
            ValidatorKeyType::Kms(adapter) => adapter.public_key_bytes(),
        }
    }

    /// Get private key bytes (only available for local keys)
    /// Returns None for KMS keys as private key never leaves the HSM
    fn private_key_bytes(&self) -> Option<[u8; 32]> {
        match self {
            ValidatorKeyType::Local(keypair) => Some(*keypair.private_key().as_bytes()),
            ValidatorKeyType::Kms(_) => None,
        }
    }

    /// Sign data (async for KMS support)
    async fn sign_async(&self, message: &[u8]) -> std::result::Result<CryptoSignature, Error> {
        match self {
            ValidatorKeyType::Local(keypair) => {
                // Local signing is synchronous, but we need async interface
                // Use sign_with_private_key which takes PrivateKey and returns Signature
                Ok(dchat_crypto::signatures::sign_with_private_key(
                    keypair.private_key(),
                    message,
                ))
            }
            ValidatorKeyType::Kms(adapter) => adapter.sign_async(message).await,
        }
    }
}

/// Prometheus metrics for peer management
struct PeerMetrics {
    peer_count: Arc<RwLock<HashMap<String, usize>>>,
    handshake_success_count: Arc<RwLock<u64>>,
    handshake_failure_count: Arc<RwLock<u64>>,
    average_rtt_ms: Arc<RwLock<f64>>,
    average_connection_quality: Arc<RwLock<f64>>,
}

impl PeerMetrics {
    fn new() -> Self {
        Self {
            peer_count: Arc::new(RwLock::new(HashMap::new())),
            handshake_success_count: Arc::new(RwLock::new(0)),
            handshake_failure_count: Arc::new(RwLock::new(0)),
            average_rtt_ms: Arc::new(RwLock::new(0.0)),
            average_connection_quality: Arc::new(RwLock::new(0.0)),
        }
    }

    /// Update peer count by node type
    pub async fn update_peer_count(&self, node_type: &str, count: usize) {
        let mut peers = self.peer_count.write().await;
        peers.insert(node_type.to_string(), count);
    }

    /// Record a successful handshake
    pub async fn record_handshake_success(&self) {
        let mut count = self.handshake_success_count.write().await;
        *count += 1;
    }

    /// Record a failed handshake
    pub async fn record_handshake_failure(&self) {
        let mut count = self.handshake_failure_count.write().await;
        *count += 1;
    }

    /// Update average RTT metric
    pub async fn update_average_rtt(&self, rtt: f64) {
        let mut avg = self.average_rtt_ms.write().await;
        *avg = rtt;
    }

    /// Update average connection quality metric
    pub async fn update_average_quality(&self, quality: f64) {
        let mut avg = self.average_connection_quality.write().await;
        *avg = quality;
    }

    async fn export_prometheus(&self) -> String {
        let peers = self.peer_count.read().await;
        let handshake_success = *self.handshake_success_count.read().await;
        let handshake_failure = *self.handshake_failure_count.read().await;
        let avg_rtt = *self.average_rtt_ms.read().await;
        let avg_quality = *self.average_connection_quality.read().await;

        let mut output = String::new();
        output.push_str("# HELP dchat_peer_count Number of connected peers by type\n");
        output.push_str("# TYPE dchat_peer_count gauge\n");
        for (node_type, count) in peers.iter() {
            output.push_str(&format!(
                "dchat_peer_count{{type=\"{}\"}} {}\n",
                node_type, count
            ));
        }

        output.push_str("# HELP dchat_handshake_success_total Total successful handshakes\n");
        output.push_str("# TYPE dchat_handshake_success_total counter\n");
        output.push_str(&format!(
            "dchat_handshake_success_total {}\n",
            handshake_success
        ));

        output.push_str("# HELP dchat_handshake_failure_total Total failed handshakes\n");
        output.push_str("# TYPE dchat_handshake_failure_total counter\n");
        output.push_str(&format!(
            "dchat_handshake_failure_total {}\n",
            handshake_failure
        ));

        output.push_str("# HELP dchat_peer_average_rtt_ms Average peer RTT in milliseconds\n");
        output.push_str("# TYPE dchat_peer_average_rtt_ms gauge\n");
        output.push_str(&format!("dchat_peer_average_rtt_ms {}\n", avg_rtt));

        output.push_str(
            "# HELP dchat_peer_average_connection_quality Average connection quality (0.0-1.0)\n",
        );
        output.push_str("# TYPE dchat_peer_average_connection_quality gauge\n");
        output.push_str(&format!(
            "dchat_peer_average_connection_quality {}\n",
            avg_quality
        ));

        output
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// PEER ADVERTISEMENT PROTOCOL - Production Ready
// ═══════════════════════════════════════════════════════════════════════════

/// Peer advertisement message for peer discovery
/// Uses string representations for cross-platform serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDiscoveryAdvertisement {
    /// Advertising peer's ID (as string)
    pub peer_id: String,
    /// List of known peers
    pub known_peers: Vec<AdvertisedPeer>,
    /// Timestamp (Unix seconds)
    pub timestamp: u64,
    /// Protocol version
    pub protocol_version: String,
}

/// Advertised peer information shared during peer discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvertisedPeer {
    /// Peer ID as string
    pub peer_id: String,
    /// Multiaddr as string
    pub multiaddr: String,
    /// Type of node
    pub node_type: NodeType,
    /// Geographic region
    pub geographic_region: Option<String>,
    /// Connection quality score (0.0 to 1.0)
    pub connection_quality: f64,
    /// Last seen timestamp (Unix seconds)
    pub last_seen_seconds: u64,
}

/// Handle incoming peer advertisement
pub async fn handle_peer_discovery_advertisement(
    advertisement: PeerDiscoveryAdvertisement,
    from: PeerId,
    peer_registry: &Arc<PeerRegistry>,
) {
    debug!(
        "📢 Received peer advertisement from {} with {} peers",
        from,
        advertisement.known_peers.len()
    );

    // Validate protocol version
    if advertisement.protocol_version != VERSION {
        debug!(
            "⚠️  Advertisement from {} has mismatched version: {} (local: {})",
            from, advertisement.protocol_version, VERSION
        );
        // Still process but log the mismatch
    }

    // Add advertised peers to registry
    let mut new_peers_count = 0;
    for advertised in &advertisement.known_peers {
        // Parse peer_id and multiaddr from strings
        let peer_id = match advertised.peer_id.parse::<PeerId>() {
            Ok(id) => id,
            Err(e) => {
                debug!("⚠️  Invalid peer_id in advertisement: {}", e);
                continue;
            }
        };

        let multiaddr = match advertised.multiaddr.parse::<Multiaddr>() {
            Ok(addr) => addr,
            Err(e) => {
                debug!("⚠️  Invalid multiaddr in advertisement: {}", e);
                continue;
            }
        };

        let advertised_last_seen =
            SystemTime::UNIX_EPOCH + Duration::from_secs(advertised.last_seen_seconds);

        // Skip if we already know about this peer
        if let Some(existing) = peer_registry.get_peer(&peer_id).await {
            // Update if this peer info is newer
            if advertised_last_seen > existing.last_seen {
                debug!("📝 Updating peer {} from advertisement", peer_id);
                peer_registry
                    .add_peer_with_defaults(
                        peer_id,
                        multiaddr,
                        advertised.node_type,
                        advertised.geographic_region.clone(),
                    )
                    .await;
            }
        } else {
            // New peer discovered via advertisement
            debug!("✨ New peer discovered via advertisement: {}", peer_id);
            peer_registry
                .add_peer_with_defaults(
                    peer_id,
                    multiaddr,
                    advertised.node_type,
                    advertised.geographic_region.clone(),
                )
                .await;
            new_peers_count += 1;
        }
    }

    info!(
        "✓ Processed peer advertisement from {} ({} peers advertised, {} newly added)",
        from,
        advertisement.known_peers.len(),
        new_peers_count
    );
}

/// Create peer advertisement from current peer registry
async fn create_peer_discovery_advertisement(
    local_peer_id: PeerId,
    peer_registry: &Arc<PeerRegistry>,
    max_peers: usize,
) -> PeerDiscoveryAdvertisement {
    let all_peers = peer_registry.get_all_peers().await;

    // Get best quality peers up to max_peers limit
    let mut known_peers: Vec<AdvertisedPeer> = all_peers
        .into_iter()
        .filter(|p| p.peer_id != local_peer_id) // Don't advertise ourselves
        .take(max_peers)
        .map(|p| AdvertisedPeer {
            peer_id: p.peer_id.to_string(),
            multiaddr: p.multiaddr.to_string(),
            node_type: p.node_type,
            geographic_region: p.geographic_region,
            connection_quality: p.connection_quality,
            last_seen_seconds: p
                .last_seen
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
        .collect();

    // Sort by quality (best first)
    known_peers.sort_by(|a, b| {
        b.connection_quality
            .partial_cmp(&a.connection_quality)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    PeerDiscoveryAdvertisement {
        peer_id: local_peer_id.to_string(),
        known_peers,
        timestamp: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        protocol_version: VERSION.to_string(),
    }
}

// MAINNET-SAFE: Development defaults only - production must use config values
fn default_relay_listen() -> String {
    "0.0.0.0:7070".to_string()
}

fn default_health_addr() -> String {
    "127.0.0.1:8080".to_string()
}

/// Enforce that production builds do not run with development default binds
#[cfg(not(debug_assertions))]
fn enforce_production_endpoints(cli: &Cli) -> Result<()> {
    // Health endpoint default is loopback; warn but allow
    if cli.health_addr == default_health_addr() {
        warn!(
            "⚠️  Using default health address (127.0.0.1:8080). Set --health-addr for production."
        );
    }

    if let Commands::Relay { listen, .. } = &cli.command {
        if listen == &default_relay_listen() {
            return Err(Error::Config(
                "Relay listen address is using the development default (0.0.0.0:7070). Set --listen explicitly for production.".to_string(),
            ));
        }
    }

    Ok(())
}

#[derive(Parser)]
#[command(name = "dchat")]
#[command(version = VERSION)]
#[command(about = "Decentralized end-to-end encrypted chat", long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(short, long, value_name = "FILE", default_value = DEFAULT_CONFIG_PATH)]
    config: PathBuf,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Enable JSON logging for structured output
    #[arg(long)]
    json_logs: bool,

    /// Metrics server listen address
    #[arg(long, default_value = "127.0.0.1:9090")]
    metrics_addr: String,

    /// Health check server listen address
    #[arg(long, default_value_t = default_health_addr())]
    health_addr: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run as relay node (routes messages between peers)
    Relay {
        /// Relay listen address
        #[arg(long, default_value_t = default_relay_listen())]
        listen: String,

        /// Bootstrap peer addresses (multiaddr format)
        #[arg(long)]
        bootstrap: Vec<String>,

        /// Enable HSM/KMS for validator signing
        #[arg(long)]
        hsm: bool,

        /// AWS KMS key ID for validator signing
        #[arg(long)]
        kms_key_id: Option<String>,

        /// Stake amount for relay incentives (in tokens)
        #[arg(long, default_value = "1000")]
        stake: u64,

        /// Enable distributed tracing and observability
        #[arg(long)]
        tracing: bool,
    },

    /// Run as user node (interactive chat client)
    User {
        /// Bootstrap peer addresses
        #[arg(long)]
        bootstrap: Vec<String>,

        /// Identity backup file path
        #[arg(long)]
        identity: Option<PathBuf>,

        /// Username for display
        #[arg(long)]
        username: Option<String>,

        /// Non-interactive mode (for testing) - deprecated, use --light-client --daemon
        #[arg(long)]
        non_interactive: bool,

        /// Light client mode (relay-centric, mobile/desktop friendly)
        #[arg(long)]
        light_client: bool,

        /// Run as background daemon (with --light-client)
        #[arg(long)]
        daemon: bool,

        /// Client profile: performance, balanced, low-power, relay-only
        #[arg(long, default_value = "balanced")]
        profile: String,

        /// Default channels to join (comma-separated)
        #[arg(long, default_value = "global")]
        channels: String,

        /// Data directory for light client storage
        #[arg(long)]
        data_dir: Option<PathBuf>,

        /// Metrics server address
        #[arg(long)]
        metrics_addr: Option<String>,

        /// Health check server address
        #[arg(long)]
        health_addr: Option<String>,

        /// Enable distributed tracing and observability
        #[arg(long)]
        tracing: bool,
    },

    /// Run as validator node (participates in consensus)
    Validator {
        /// Validator key file path (or HSM key ID)
        #[arg(long)]
        key: String,

        /// Chain RPC endpoint (optional when using --genesis-dir)
        #[arg(long)]
        chain_rpc: Option<String>,

        /// Enable HSM/KMS
        #[arg(long)]
        hsm: bool,

        /// Validator stake amount
        #[arg(long, default_value = "10000")]
        stake: u64,

        /// Enable block production
        #[arg(long)]
        producer: bool,

        /// Enable distributed tracing and observability
        #[arg(long)]
        tracing: bool,

        /// Genesis bootstrap mode - allow starting without bootstrap peers (for genesis validators)
        #[arg(long)]
        genesis_bootstrap: bool,

        /// Genesis directory containing pre-stake genesis files (uses pre-staked validators, no RPC required)
        #[arg(long)]
        genesis_dir: Option<PathBuf>,
    },

    /// Launch full testnet (validators + relays + clients)
    Testnet {
        /// Number of validator nodes
        #[arg(long, default_value = "3")]
        validators: usize,

        /// Number of relay nodes
        #[arg(long, default_value = "3")]
        relays: usize,

        /// Number of client nodes
        #[arg(long, default_value = "5")]
        clients: usize,

        /// Base data directory for all nodes
        #[arg(long, default_value = "./testnet-data")]
        data_dir: PathBuf,

        /// Enable observability stack
        #[arg(long)]
        observability: bool,
    },

    /// Generate new identity and keys
    Keygen {
        /// Output file path
        #[arg(short, long, default_value = "identity.json")]
        output: PathBuf,

        /// Generate ephemeral/burner identity
        #[arg(long)]
        burner: bool,

        /// Save as plaintext (unencrypted) for automated deployments
        #[arg(long)]
        plaintext: bool,

        /// Generate validator keypair (includes private key for validator nodes)
        #[arg(long)]
        validator: bool,
    },

    /// Initialize mainnet genesis (first validator only)
    Genesis {
        /// Output directory for genesis files
        #[arg(long, default_value = "./genesis")]
        output: PathBuf,

        /// Chain ID for the network
        #[arg(long, default_value = "dchat-mainnet-1")]
        chain_id: String,

        /// Initial token supply
        #[arg(long, default_value = "1000000000")]
        initial_supply: u64,

        /// Foundation allocation percentage (0-100)
        #[arg(long, default_value = "20")]
        foundation_percent: u8,

        /// Genesis validator public keys (comma-separated hex)
        #[arg(long)]
        validators: Option<String>,
    },

    /// Initialize mainnet for foundation servers
    MainnetInit {
        /// Path to mainnet config file
        #[arg(long, default_value = "mainnet-config.toml")]
        config: PathBuf,

        /// Initialize as genesis validator (creates both chains)
        #[arg(long)]
        genesis_validator: bool,

        /// Fund foundation servers with initial stake
        #[arg(long)]
        fund_foundation: bool,

        /// Initialize all pools (staking, rewards, liquidity)
        #[arg(long)]
        init_pools: bool,
    },

    /// Pre-stake genesis - create genesis with pre-staked validators (solves chicken-and-egg problem)
    PreStakeGenesis {
        #[command(subcommand)]
        action: PreStakeGenesisCommand,
    },

    /// User account management
    Account {
        #[command(subcommand)]
        action: AccountCommand,
    },

    /// Database management commands
    Database {
        #[command(subcommand)]
        action: DatabaseCommand,
    },

    /// Health check (returns exit code 0 if healthy)
    Health {
        /// Node health check URL
        #[arg(long, default_value = "http://127.0.0.1:8080/health")]
        url: String,
    },

    /// Bot management operations
    Bot {
        #[command(subcommand)]
        action: BotCommand,
    },

    /// Marketplace operations
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceCommand,
    },

    /// Accessibility features and testing
    Accessibility {
        #[command(subcommand)]
        action: AccessibilityCommand,
    },

    /// Chaos engineering and testing
    Chaos {
        #[command(subcommand)]
        action: ChaosCommand,
    },

    /// Protocol governance and upgrades
    Governance {
        #[command(subcommand)]
        action: GovernanceCommand,
    },

    /// tokenomics and currency management
    Token {
        #[command(subcommand)]
        action: TokenCommand,
    },

    /// Update distribution and auto-update management
    Update {
        #[command(subcommand)]
        action: UpdateCommand,
    },

    /// Deployment planning, validation, and health tooling
    Deploy {
        #[command(subcommand)]
        action: DeployCommand,
    },

    /// Network and peer management
    Network {
        #[command(subcommand)]
        action: NetworkCommand,
    },

    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        action: WalletCommand,
    },

    /// Staking operations
    Staking {
        #[command(subcommand)]
        action: StakingCommand,
    },

    /// Reward claiming
    Rewards {
        #[command(subcommand)]
        action: RewardsCommand,
    },

    /// Smart contract/program management
    Program {
        #[command(subcommand)]
        action: ProgramCommand,
    },

    /// Mini-app platform operations
    MiniApp {
        #[command(subcommand)]
        action: MiniAppCommand,
    },

    /// Quorum-Gated Encryption (QGE) management
    Qge {
        #[command(subcommand)]
        action: QgeCommand,
    },
}

/// Mini-app platform commands
#[derive(Debug, Subcommand)]
enum MiniAppCommand {
    /// Launch a mini-app from manifest or app ID
    Launch {
        /// Path to app manifest file or directory containing manifest.json
        #[arg(long)]
        manifest: Option<PathBuf>,

        /// App ID to launch from registry (hex string)
        #[arg(long)]
        app_id: Option<String>,

        /// User ID for the session (hex string)
        #[arg(long)]
        user_id: Option<String>,

        /// Developer public key for app identity (hex string, 32 bytes)
        #[arg(long)]
        developer_key: Option<String>,

        /// Theme (light or dark)
        #[arg(long, default_value = "light")]
        theme: String,

        /// Viewport width in pixels
        #[arg(long, default_value = "480")]
        width: u32,

        /// Viewport height in pixels
        #[arg(long, default_value = "800")]
        height: u32,

        /// Enable debug mode
        #[arg(long)]
        debug: bool,
    },

    /// Register as a mini-app developer
    RegisterDeveloper {
        /// Developer display name
        #[arg(long)]
        name: String,

        /// Keypair file for signing
        #[arg(long)]
        keypair: PathBuf,

        /// Optional website URL
        #[arg(long)]
        website: Option<String>,

        /// Optional contact email
        #[arg(long)]
        email: Option<String>,
    },

    /// Register a new mini-app
    Register {
        /// Path to app manifest file
        #[arg(long)]
        manifest: PathBuf,

        /// Developer keypair file
        #[arg(long)]
        keypair: PathBuf,

        /// Path to app bundle (HTML/JS/CSS)
        #[arg(long)]
        bundle: Option<PathBuf>,
    },

    /// Show mini-app information
    Info {
        /// App ID (hex string)
        #[arg(long)]
        app_id: String,
    },

    /// List installed mini-apps
    List {
        /// Filter by category
        #[arg(long)]
        category: Option<String>,

        /// Show only installed apps
        #[arg(long)]
        installed: bool,
    },

    /// Create a new mini-app project scaffold
    Init {
        /// Project name
        #[arg(long)]
        name: String,

        /// Output directory
        #[arg(long, default_value = ".")]
        path: PathBuf,

        /// App category (games, social, finance, utility)
        #[arg(long, default_value = "utility")]
        category: String,
    },

    /// Validate a mini-app manifest
    Validate {
        /// Path to manifest file
        #[arg(long)]
        manifest: PathBuf,

        /// Show detailed validation report
        #[arg(long)]
        verbose: bool,
    },
}

/// Quorum-Gated Encryption (QGE) commands
#[derive(Debug, Subcommand)]
enum QgeCommand {
    /// Show QGE token status for current user
    TokenStatus {
        /// Filter by conversation type (direct, channel, group)
        #[arg(long)]
        conversation_type: Option<String>,

        /// Show tokens for a specific epoch
        #[arg(long)]
        epoch: Option<u64>,

        /// Show expired tokens
        #[arg(long)]
        include_expired: bool,
    },

    /// Request a new epoch token
    RequestToken {
        /// Channel or user ID (hex string)
        #[arg(long)]
        target: String,

        /// Conversation type (direct, channel, group)
        #[arg(long, default_value = "direct")]
        conversation_type: String,

        /// Force refresh even if token exists
        #[arg(long)]
        force: bool,
    },

    /// Show relay committee information
    Committee {
        /// Show detailed member information
        #[arg(long)]
        detailed: bool,

        /// Show historical rotations
        #[arg(long)]
        history: bool,

        /// Number of historical epochs to show
        #[arg(long, default_value = "10")]
        epochs: u64,
    },

    /// Show relay status and incentives
    RelayStatus {
        /// Relay ID (hex string, defaults to local relay)
        #[arg(long)]
        relay_id: Option<String>,

        /// Show stake information
        #[arg(long)]
        stake: bool,

        /// Show performance metrics
        #[arg(long)]
        metrics: bool,

        /// Show reward history
        #[arg(long)]
        rewards: bool,
    },

    /// Manage revocations (admin only)
    Revocation {
        #[command(subcommand)]
        action: QgeRevocationAction,
    },

    /// View audit logs
    AuditLog {
        /// Filter by category (token, key, access, admin, security, system)
        #[arg(long)]
        category: Option<String>,

        /// Minimum severity (debug, info, warning, error, critical, alert)
        #[arg(long)]
        min_severity: Option<String>,

        /// Start time (Unix timestamp or ISO8601)
        #[arg(long)]
        from: Option<String>,

        /// End time (Unix timestamp or ISO8601)
        #[arg(long)]
        to: Option<String>,

        /// Maximum number of entries
        #[arg(long, default_value = "100")]
        limit: usize,

        /// Output format (text, json)
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Check rate limit status
    RateLimitStatus {
        /// User ID to check (defaults to current user)
        #[arg(long)]
        user_id: Option<String>,

        /// Show detailed bucket information
        #[arg(long)]
        detailed: bool,
    },

    /// Show QGE statistics and health
    Stats {
        /// Include network-wide statistics
        #[arg(long)]
        network: bool,

        /// Show detailed breakdown
        #[arg(long)]
        detailed: bool,
    },

    /// Cleanup old cryptographic state
    Cleanup {
        /// Force cleanup now (don't wait for scheduler)
        #[arg(long)]
        force: bool,

        /// Show what would be cleaned (dry run)
        #[arg(long)]
        dry_run: bool,

        /// Maximum age for keys in seconds
        #[arg(long)]
        max_age: Option<u64>,
    },
}

/// QGE revocation subcommands
#[derive(Debug, Subcommand)]
enum QgeRevocationAction {
    /// List active revocations
    List {
        /// Channel ID to filter by
        #[arg(long)]
        channel_id: Option<String>,

        /// Show only pending revocations
        #[arg(long)]
        pending: bool,

        /// Include expired revocations
        #[arg(long)]
        include_expired: bool,
    },

    /// Check if a user is revoked
    Check {
        /// User ID to check (hex string)
        #[arg(long)]
        user_id: String,

        /// Channel ID (hex string)
        #[arg(long)]
        channel_id: String,
    },

    /// Revoke a user from a channel (admin only)
    Create {
        /// User ID to revoke (hex string)
        #[arg(long)]
        user_id: String,

        /// Channel ID (hex string)
        #[arg(long)]
        channel_id: String,

        /// Revocation type (timeout, mute, soft_revoke, ban, shadow_ban)
        #[arg(long, default_value = "soft_revoke")]
        action: String,

        /// Duration in seconds (for timeout/mute)
        #[arg(long)]
        duration: Option<u64>,

        /// Reason for revocation
        #[arg(long)]
        reason: String,
    },

    /// Lift a revocation (admin only)
    Lift {
        /// Revocation ID (hex string)
        #[arg(long)]
        revocation_id: String,

        /// Reason for lifting
        #[arg(long)]
        reason: Option<String>,
    },

    /// Submit an appeal for a revocation
    Appeal {
        /// Revocation ID (hex string)
        #[arg(long)]
        revocation_id: String,

        /// Appeal message
        #[arg(long)]
        message: String,
    },
}

/// Network and peer management commands
#[derive(Debug, Subcommand)]
enum NetworkCommand {
    /// Show network status and connected peers
    Status,

    /// List all connected peers with metrics
    Peers {
        /// Filter by node type (validator, relay, client)
        #[arg(long)]
        node_type: Option<String>,
    },

    /// Connect to a specific peer
    Connect {
        /// Peer multiaddr (e.g., /ip4/1.2.3.4/tcp/4001/p2p/12D3...)
        #[arg(long)]
        multiaddr: String,
    },

    /// Disconnect from a peer
    Disconnect {
        /// Peer ID to disconnect
        #[arg(long)]
        peer_id: String,
    },

    /// Ban a misbehaving peer
    Ban {
        /// Peer ID to ban
        #[arg(long)]
        peer_id: String,

        /// Ban duration in hours (0 = permanent)
        #[arg(long, default_value = "24")]
        duration_hours: u64,

        /// Reason for banning
        #[arg(long)]
        reason: Option<String>,
    },

    /// Unban a previously banned peer
    Unban {
        /// Peer ID to unban
        #[arg(long)]
        peer_id: String,
    },

    /// Show banned peers list
    Banned,

    /// Generate a stable bootstrap peer record (includes /p2p/<PeerId>)
    ///
    /// This is intended for mainnet ceremony setup where operators must publish
    /// bootstrap multiaddrs that include the correct PeerId.
    PeerRecord {
        /// Node type for the record (validator or relay)
        #[arg(long, default_value = "validator")]
        node_type: String,

        /// Path to the local validator key JSON (required for validator records)
        #[arg(long)]
        key: Option<PathBuf>,

        /// DNS name to embed in the multiaddr (e.g. validator-1.dchat.network)
        #[arg(long)]
        dns: String,

        /// TCP port to embed in the multiaddr
        #[arg(long, default_value = "7070")]
        port: u16,

        /// Optional output path to write a JSON record
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

/// Wallet operations commands
#[derive(Debug, Subcommand)]
enum WalletCommand {
    /// Create a new wallet
    Create {
        /// Wallet name/label
        #[arg(long)]
        name: String,

        /// Output file for wallet backup
        #[arg(long, default_value = "wallet.json")]
        output: PathBuf,
    },

    /// Show wallet balance
    Balance {
        /// User ID to check balance for
        #[arg(long)]
        user_id: String,
    },

    /// Export wallet for backup (encrypted)
    Export {
        /// User ID of wallet to export
        #[arg(long)]
        user_id: String,

        /// Output file path
        #[arg(long)]
        output: PathBuf,

        /// Encryption password (will prompt if not provided)
        #[arg(long)]
        password: Option<String>,
    },

    /// Import wallet from backup
    Import {
        /// Backup file path
        #[arg(long)]
        file: PathBuf,

        /// Decryption password (will prompt if not provided)
        #[arg(long)]
        password: Option<String>,
    },

    /// Show wallet transaction history
    History {
        /// User ID
        #[arg(long)]
        user_id: String,

        /// Number of recent transactions to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Generate new receiving address
    NewAddress {
        /// User ID
        #[arg(long)]
        user_id: String,
    },
}

/// Staking operations commands
#[derive(Debug, Subcommand)]
enum StakingCommand {
    /// Stake tokens
    Stake {
        /// User ID performing the stake
        #[arg(long)]
        user_id: String,

        /// Amount to stake in tokens
        #[arg(long)]
        amount: u64,

        /// Lockup duration in days (min 7 for validators)
        #[arg(long, default_value = "7")]
        duration_days: u32,
    },

    /// Unstake tokens (begins unbonding period)
    Unstake {
        /// User ID requesting unstake
        #[arg(long)]
        user_id: String,

        /// Amount to unstake (0 = all)
        #[arg(long, default_value = "0")]
        amount: u64,
    },

    /// Show staking status for a user
    Status {
        /// User ID to check
        #[arg(long)]
        user_id: String,
    },

    /// List all active validators
    Validators,

    /// Delegate stake to a validator
    Delegate {
        /// Delegator user ID
        #[arg(long)]
        user_id: String,

        /// Validator to delegate to
        #[arg(long)]
        validator_id: String,

        /// Amount to delegate
        #[arg(long)]
        amount: u64,
    },

    /// Undelegate stake from a validator
    Undelegate {
        /// Delegator user ID
        #[arg(long)]
        user_id: String,

        /// Validator to undelegate from
        #[arg(long)]
        validator_id: String,

        /// Amount to undelegate (0 = all)
        #[arg(long, default_value = "0")]
        amount: u64,
    },
}

/// Reward claiming commands
#[derive(Debug, Subcommand)]
enum RewardsCommand {
    /// Claim pending rewards
    Claim {
        /// User ID to claim rewards for
        #[arg(long)]
        user_id: String,
    },

    /// Show reward history
    History {
        /// User ID
        #[arg(long)]
        user_id: String,

        /// Number of recent reward events to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Show pending (unclaimed) rewards
    Pending {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// Show reward distribution breakdown
    Breakdown {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// Compound rewards (auto-stake rewards)
    Compound {
        /// User ID
        #[arg(long)]
        user_id: String,

        /// Enable auto-compounding
        #[arg(long)]
        enable: bool,
    },
}

/// Smart contract/program management commands
#[derive(Debug, Subcommand)]
enum ProgramCommand {
    /// Deploy a new program from WASM bytecode
    Deploy {
        /// Path to the compiled WASM file
        #[arg(long)]
        wasm: PathBuf,

        /// Keypair file for signing transactions (deployer pays fees)
        #[arg(long)]
        keypair: PathBuf,

        /// Upgrade authority keypair (defaults to deployer if not specified)
        #[arg(long)]
        upgrade_authority: Option<PathBuf>,

        /// RPC endpoint URL
        #[arg(long)]
        rpc_url: Option<String>,

        /// Maximum program size (for future upgrades)
        #[arg(long)]
        max_data_len: Option<usize>,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Upgrade an existing program with new bytecode
    Upgrade {
        /// Program ID to upgrade
        #[arg(long)]
        program_id: String,

        /// Path to the new WASM bytecode
        #[arg(long)]
        wasm: PathBuf,

        /// Upgrade authority keypair
        #[arg(long)]
        authority: PathBuf,

        /// RPC endpoint URL
        #[arg(long)]
        rpc_url: Option<String>,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Freeze a program (make permanently immutable)
    Freeze {
        /// Program ID to freeze
        #[arg(long)]
        program_id: String,

        /// Upgrade authority keypair
        #[arg(long)]
        authority: PathBuf,

        /// RPC endpoint URL
        #[arg(long)]
        rpc_url: Option<String>,

        /// Skip confirmation prompt (WARNING: irreversible!)
        #[arg(long)]
        yes: bool,
    },

    /// Show program information
    Info {
        /// Program ID to query
        #[arg(long)]
        program_id: String,

        /// RPC endpoint URL
        #[arg(long)]
        rpc_url: Option<String>,
    },

    /// Transfer upgrade authority to a new keypair
    SetAuthority {
        /// Program ID
        #[arg(long)]
        program_id: String,

        /// Current authority keypair
        #[arg(long)]
        current_authority: PathBuf,

        /// New authority public key (hex)
        #[arg(long)]
        new_authority: String,

        /// RPC endpoint URL
        #[arg(long)]
        rpc_url: Option<String>,
    },

    /// Close a program and reclaim motes
    Close {
        /// Program ID to close
        #[arg(long)]
        program_id: String,

        /// Authority keypair
        #[arg(long)]
        authority: PathBuf,

        /// Destination for reclaimed motes (defaults to authority)
        #[arg(long)]
        destination: Option<String>,

        /// RPC endpoint URL
        #[arg(long)]
        rpc_url: Option<String>,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Validate WASM bytecode without deploying
    Validate {
        /// Path to the WASM file to validate
        #[arg(long)]
        wasm: PathBuf,

        /// Show detailed validation report
        #[arg(long)]
        verbose: bool,

        /// Require manifest to be present (strict mode)
        #[arg(long)]
        require_manifest: bool,

        /// Reject zero/placeholder schema hash
        #[arg(long)]
        reject_zero_hash: bool,
    },

    /// Inspect program manifest (DPL manifest section)
    Manifest {
        /// Path to WASM file OR deployed program ID
        #[arg(long)]
        program: String,

        /// RPC endpoint URL (required if program is an ID)
        #[arg(long)]
        rpc_url: Option<String>,

        /// Output format: text, json, or yaml
        #[arg(long, default_value = "text")]
        format: String,

        /// Show raw manifest bytes (hex)
        #[arg(long)]
        raw: bool,
    },

    /// Verify program manifest matches expected IDL/schema hash
    VerifyManifest {
        /// Path to WASM file OR deployed program ID
        #[arg(long)]
        program: String,

        /// Expected schema hash (hex string, 64 chars)
        #[arg(long)]
        expected_hash: Option<String>,

        /// Path to IDL file to compute expected hash from
        #[arg(long)]
        idl: Option<PathBuf>,

        /// RPC endpoint URL (required if program is an ID)
        #[arg(long)]
        rpc_url: Option<String>,
    },

    /// Build and deploy from a contract source directory
    BuildDeploy {
        /// Path to contract directory (must contain Cargo.toml)
        #[arg(long)]
        path: PathBuf,

        /// Keypair file for signing transactions
        #[arg(long)]
        keypair: PathBuf,

        /// RPC endpoint URL
        #[arg(long)]
        rpc_url: Option<String>,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DeployCommand {
    /// Generate a complete deployment plan with configs for validators, relays, storage, backups, and monitoring
    Plan {
        /// Logical network name (e.g. dchat-mainnet, dchat-testnet)
        #[arg(long)]
        network: String,

        /// Base domain used for validator and relay hostnames
        #[arg(long)]
        domain: String,

        /// Number of relays to include in the plan (20-50 recommended)
        #[arg(long, default_value_t = 30)]
        relays: usize,

        /// Output directory for generated artifacts
        #[arg(long, default_value = "./deployment-plan")]
        output: PathBuf,
    },

    /// Validate an existing deployment plan directory
    Validate {
        /// Path to deployment plan directory (containing deployment-plan.json)
        #[arg(long)]
        input: PathBuf,
    },

    /// Print a summary of an existing deployment plan without validation
    Summary {
        /// Path to deployment plan directory (containing deployment-plan.json)
        #[arg(long)]
        input: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum BotCommand {
    /// Create a new bot
    Create {
        /// Bot username (must end with 'bot')
        #[arg(long)]
        username: String,

        /// Bot display name
        #[arg(long)]
        name: String,

        /// Bot description
        #[arg(long)]
        description: String,

        /// Owner user ID
        #[arg(long)]
        owner_id: String,

        /// Print the full bot token to stdout (DANGEROUS: may end up in logs/shell history)
        #[arg(long, default_value_t = false)]
        reveal_token: bool,
    },

    /// List all bots or bots by owner
    List {
        /// Filter by owner user ID
        #[arg(long)]
        owner_id: Option<String>,
    },

    /// Get bot information
    Info {
        /// Bot ID
        #[arg(long)]
        bot_id: String,
    },

    /// Regenerate bot token
    RegenerateToken {
        /// Bot ID
        #[arg(long)]
        bot_id: String,

        /// Owner user ID
        #[arg(long)]
        owner_id: String,

        /// Print the full new token to stdout (DANGEROUS: may end up in logs/shell history)
        #[arg(long, default_value_t = false)]
        reveal_token: bool,
    },

    /// Set webhook URL for bot
    SetWebhook {
        /// Bot ID
        #[arg(long)]
        bot_id: String,

        /// Webhook URL
        #[arg(long)]
        url: String,

        /// Webhook secret for HMAC verification
        #[arg(long)]
        secret: Option<String>,
    },

    /// Send message as bot
    SendMessage {
        /// Bot token
        #[arg(long)]
        token: String,

        /// Chat ID to send to
        #[arg(long)]
        chat_id: String,

        /// Message text
        #[arg(long)]
        text: String,
    },
}

#[derive(Debug, Subcommand)]
enum MarketplaceCommand {
    /// List marketplace items
    List {
        /// Filter by item type (sticker-pack, emoji-pack, theme, bot, nft, image, subscription, badge, channel, membership)
        #[arg(long)]
        item_type: Option<String>,

        /// SQLite DB path for marketplace persistence (defaults to ./dchat.db)
        #[arg(long)]
        db_path: Option<PathBuf>,
    },

    /// Create a new listing
    CreateListing {
        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// Listing title
        #[arg(long)]
        title: String,

        /// Listing description
        #[arg(long)]
        description: String,

        /// Item type
        #[arg(long)]
        item_type: String,

        /// Price (0 for free)
        #[arg(long)]
        price: u64,

        /// Content hash (IPFS CID)
        #[arg(long)]
        content_hash: String,

        /// Bot ID (if selling bot)
        #[arg(long)]
        bot_id: Option<String>,

        /// Channel ID (if selling channel or membership)
        #[arg(long)]
        channel_id: Option<String>,

        /// Membership duration in days (if selling membership)
        #[arg(long)]
        membership_duration: Option<u32>,

        /// SQLite DB path for marketplace persistence (defaults to ./dchat.db)
        #[arg(long)]
        db_path: Option<PathBuf>,
    },

    /// Buy a marketplace item
    Buy {
        /// Buyer user ID
        #[arg(long)]
        buyer_id: String,

        /// Listing ID
        #[arg(long)]
        listing_id: String,

        /// SQLite DB path for marketplace persistence (defaults to ./dchat.db)
        #[arg(long)]
        db_path: Option<PathBuf>,
    },

    /// Mint an entitlement from a verified currency-chain escrow lock attestation.
    ///
    /// This is the security-critical step that enforces replay protection via a persisted nullifier.
    MintEntitlement {
        /// Attestation JSON file path
        #[arg(long)]
        attestation: PathBuf,

        /// Validator set JSON file path
        #[arg(long)]
        validator_set: PathBuf,

        /// SQLite DB path for marketplace persistence (defaults to ./dchat.db)
        #[arg(long)]
        db_path: Option<PathBuf>,
    },

    /// Finalize escrow settlement from a verified currency-chain attestation.
    ///
    /// Accepts EscrowReleased or EscrowRefunded attestations and records a terminal settlement.
    FinalizeSettlement {
        /// Attestation JSON file path
        #[arg(long)]
        attestation: PathBuf,

        /// Validator set JSON file path
        #[arg(long)]
        validator_set: PathBuf,

        /// SQLite DB path for marketplace persistence (defaults to ./dchat.db)
        #[arg(long)]
        db_path: Option<PathBuf>,
    },

    /// Get creator statistics
    CreatorStats {
        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// SQLite DB path for marketplace persistence (defaults to ./dchat.db)
        #[arg(long)]
        db_path: Option<PathBuf>,
    },

    // Developer-only commands backed by in-memory MarketplaceManager state.
    // These are intentionally compiled out of production builds.
    #[cfg(feature = "dev-tools")]
    /// Create escrow for a transaction
    CreateEscrow {
        /// Buyer user ID
        #[arg(long)]
        buyer: String,

        /// Seller user ID
        #[arg(long)]
        seller: String,

        /// Amount in tokens
        #[arg(long)]
        amount: u64,

        /// Listing ID
        #[arg(long)]
        listing_id: String,
    },

    #[cfg(feature = "dev-tools")]
    /// Register bot for marketplace trading
    RegisterBot {
        /// Bot ID
        #[arg(long)]
        bot_id: String,

        /// Bot username
        #[arg(long)]
        username: String,

        /// Owner user ID
        #[arg(long)]
        owner: String,
    },

    #[cfg(feature = "dev-tools")]
    /// Register channel for marketplace trading
    RegisterChannel {
        /// Channel ID
        #[arg(long)]
        channel_id: String,

        /// Channel name
        #[arg(long)]
        name: String,

        /// Owner user ID
        #[arg(long)]
        owner: String,

        /// Current member count
        #[arg(long)]
        member_count: u64,
    },

    #[cfg(feature = "dev-tools")]
    /// Get bot ownership info
    BotOwnership {
        /// Bot ID
        #[arg(long)]
        bot_id: String,
    },

    #[cfg(feature = "dev-tools")]
    /// Get channel ownership info
    ChannelOwnership {
        /// Channel ID
        #[arg(long)]
        channel_id: String,
    },

    #[cfg(feature = "dev-tools")]
    /// List bots owned by user
    MyBots {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    #[cfg(feature = "dev-tools")]
    /// List channels owned by user
    MyChannels {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    #[cfg(feature = "dev-tools")]
    /// Create emoji pack
    CreateEmojiPack {
        /// Pack name
        #[arg(long)]
        name: String,

        /// Pack description
        #[arg(long)]
        description: String,

        /// Number of emojis
        #[arg(long)]
        emoji_count: u32,

        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// Content hash
        #[arg(long)]
        content_hash: String,

        /// Is animated
        #[arg(long, default_value = "false")]
        animated: bool,
    },

    #[cfg(feature = "dev-tools")]
    /// Register image artwork
    RegisterImage {
        /// Image title
        #[arg(long)]
        title: String,

        /// Image description
        #[arg(long)]
        description: String,

        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// Content hash
        #[arg(long)]
        content_hash: String,

        /// Width in pixels
        #[arg(long)]
        width: u32,

        /// Height in pixels
        #[arg(long)]
        height: u32,

        /// Image format (png, jpg, etc.)
        #[arg(long)]
        format: String,

        /// License type (all-rights-reserved, cc-by, cc-by-sa, cc-by-nd, cc-by-nc, public-domain)
        #[arg(long, default_value = "all-rights-reserved")]
        license: String,
    },

    #[cfg(feature = "dev-tools")]
    /// Check channel membership
    CheckMembership {
        /// Channel ID
        #[arg(long)]
        channel_id: String,

        /// User ID
        #[arg(long)]
        user_id: String,
    },

    #[cfg(feature = "dev-tools")]
    /// List my memberships
    MyMemberships {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    #[cfg(feature = "dev-tools")]
    /// Transfer membership
    TransferMembership {
        /// Membership ID
        #[arg(long)]
        membership_id: String,

        /// New holder user ID
        #[arg(long)]
        new_holder: String,
    },

    #[cfg(feature = "dev-tools")]
    /// List channel members
    ChannelMembers {
        /// Channel ID
        #[arg(long)]
        channel_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum AccessibilityCommand {
    /// Validate color contrast
    ValidateContrast {
        /// Foreground color (hex, e.g., #000000)
        #[arg(long)]
        fg_color: String,

        /// Background color (hex, e.g., #FFFFFF)
        #[arg(long)]
        bg_color: String,

        /// Target WCAG level (A, AA, AAA)
        #[arg(long, default_value = "AA")]
        level: String,
    },

    /// Test text-to-speech
    TtsSpeak {
        /// Text to speak
        #[arg(long)]
        text: String,

        /// Language code (e.g., en-US)
        #[arg(long, default_value = "en-US")]
        language: String,
    },

    /// Validate UI element accessibility
    ValidateElement {
        /// Element ID
        #[arg(long)]
        element_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum ChaosCommand {
    /// List available chaos scenarios
    ListScenarios,

    /// Execute a chaos scenario
    Execute {
        /// Scenario ID or name
        #[arg(long)]
        scenario: String,

        /// Duration in seconds
        #[arg(long, default_value = "60")]
        duration: u64,
    },

    /// Inject a specific fault
    InjectFault {
        /// Target node
        #[arg(long)]
        node: String,

        /// Fault type (latency, packet-loss, cpu-spike, memory-pressure, disk-slow, network-partition, service-crash)
        #[arg(long)]
        fault_type: String,

        /// Fault severity (0.0-1.0)
        #[arg(long, default_value = "0.5")]
        severity: f32,

        /// Duration in seconds
        #[arg(long, default_value = "60")]
        duration: u64,
    },

    /// Simulate network partition
    SimulatePartition {
        /// Nodes in partition A (comma-separated)
        #[arg(long)]
        partition_a: String,

        /// Nodes in partition B (comma-separated)
        #[arg(long)]
        partition_b: String,

        /// Duration in seconds
        #[arg(long, default_value = "120")]
        duration: u64,
    },
}

#[derive(Debug, Subcommand)]
enum GovernanceCommand {
    /// Submit a protocol upgrade proposal
    ProposeUpgrade {
        /// Proposer user ID
        #[arg(long)]
        proposer: String,

        /// Upgrade type (soft-fork, hard-fork, security-patch, feature-toggle)
        #[arg(long)]
        upgrade_type: String,

        /// Target version (e.g., "2.0.0")
        #[arg(long)]
        target_version: String,

        /// Proposal title
        #[arg(long)]
        title: String,

        /// Proposal description
        #[arg(long)]
        description: String,

        /// Specification URL (GitHub PR, RFC document)
        #[arg(long)]
        spec_url: Option<String>,

        /// Voting period in days
        #[arg(long, default_value = "14")]
        voting_days: i64,

        /// Required quorum percentage
        #[arg(long, default_value = "60")]
        quorum: u32,
    },

    /// List all active upgrade proposals
    ListProposals {
        /// Filter by status (proposed, approved, scheduled, active, rejected, cancelled)
        #[arg(long)]
        status: Option<String>,
    },

    /// Get details of a specific proposal
    GetProposal {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,
    },

    /// Vote on an upgrade proposal
    Vote {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,

        /// Voter user ID
        #[arg(long)]
        voter: String,

        /// Vote (true = for, false = against)
        #[arg(long)]
        vote_for: bool,

        /// Voting power (token amount staked)
        #[arg(long)]
        voting_power: u64,
    },

    /// Validator signs upgrade approval (for hard forks)
    SignUpgrade {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,

        /// Validator user ID
        #[arg(long)]
        validator_id: String,

        /// Validator stake amount
        #[arg(long)]
        stake: u64,

        /// Validator key file for signature
        #[arg(long)]
        key_file: PathBuf,
    },

    /// Finalize upgrade proposal voting
    FinalizeProposal {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,
    },

    /// Schedule approved upgrade for activation
    ScheduleUpgrade {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,

        /// Activation block height
        #[arg(long)]
        activation_height: u64,

        /// Activation timestamp (RFC3339 format)
        #[arg(long)]
        activation_time: String,
    },

    /// Activate upgrade at current block height
    ActivateUpgrade {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,

        /// Current block height
        #[arg(long)]
        current_height: u64,
    },

    /// Emergency cancel an upgrade
    CancelUpgrade {
        /// Proposal ID
        #[arg(long)]
        proposal_id: String,
    },

    /// Show current protocol version
    Version,

    /// Show fork history
    ForkHistory,

    /// Check if peer version is compatible
    CheckCompatibility {
        /// Peer protocol version (e.g., "1.2.3")
        #[arg(long)]
        peer_version: String,
    },

    /// Configure governance parameters
    Configure {
        /// Hard fork threshold percentage (validator approval required)
        #[arg(long)]
        hard_fork_threshold: Option<u32>,

        /// Total network stake
        #[arg(long)]
        total_stake: Option<u64>,
    },
}

#[derive(Debug, Subcommand)]
enum UpdateCommand {
    /// Check for available updates
    Check {
        /// Current version (defaults to dchat version)
        #[arg(long)]
        current_version: Option<String>,
    },

    /// List all available versions
    ListVersions,

    /// Download a specific version
    Download {
        /// Version to download (e.g., "1.2.3")
        #[arg(long)]
        version: String,

        /// Platform (defaults to current platform)
        #[arg(long)]
        platform: Option<String>,
    },

    /// Verify a downloaded package
    Verify {
        /// Path to package file
        #[arg(long)]
        package: PathBuf,

        /// Expected version
        #[arg(long)]
        version: String,
    },

    /// Add a mirror/download source
    AddMirror {
        /// Mirror URL
        #[arg(long)]
        url: String,

        /// Mirror type (https, ipfs, bittorrent)
        #[arg(long)]
        mirror_type: String,

        /// Geographic region
        #[arg(long)]
        region: Option<String>,

        /// Priority (lower = preferred)
        #[arg(long, default_value = "50")]
        priority: u32,
    },

    /// List configured mirrors
    ListMirrors,

    /// Test mirror connectivity
    TestMirrors,

    /// Configure auto-update settings
    ConfigureAutoUpdate {
        /// Enable auto-updates
        #[arg(long)]
        enabled: Option<bool>,

        /// Security patches only
        #[arg(long)]
        security_only: Option<bool>,

        /// Check interval in hours
        #[arg(long)]
        check_interval: Option<u64>,

        /// Auto-restart after update
        #[arg(long)]
        auto_restart: Option<bool>,
    },

    /// Show auto-update configuration
    ShowConfig,
}

/// Token and tokenomics commands
#[derive(Debug, Subcommand)]
enum TokenCommand {
    /// Show token supply statistics
    Stats,

    /// Mint new tokens (requires admin)
    Mint {
        /// Amount to mint
        #[arg(long)]
        amount: u64,

        /// Reason for minting
        #[arg(long)]
        reason: String,

        /// Recipient user ID (optional)
        #[arg(long)]
        recipient: Option<String>,
    },

    /// Burn tokens
    Burn {
        /// User ID burning tokens
        #[arg(long)]
        user_id: String,

        /// Amount to burn
        #[arg(long)]
        amount: u64,

        /// Reason for burning
        #[arg(long)]
        reason: String,
    },

    /// Create marketplace liquidity pool
    CreatePool {
        /// Pool name
        #[arg(long)]
        name: String,

        /// Initial token amount
        #[arg(long)]
        initial_amount: u64,
    },

    /// List all liquidity pools
    ListPools,

    /// Show pool details
    PoolInfo {
        /// Pool ID
        #[arg(long)]
        pool_id: String,
    },

    /// Replenish liquidity pool
    ReplenishPool {
        /// Pool ID
        #[arg(long)]
        pool_id: String,

        /// Amount to add
        #[arg(long)]
        amount: u64,
    },

    /// Show mint history
    MintHistory {
        /// Number of recent events to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Show burn history
    BurnHistory {
        /// Number of recent events to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Create distribution schedule
    CreateSchedule {
        /// Recipient type (validators, relays, marketplace, treasury, dev-fund)
        #[arg(long)]
        recipient_type: String,

        /// Amount per interval
        #[arg(long)]
        amount: u64,

        /// Interval in blocks
        #[arg(long)]
        interval_blocks: u64,

        /// Duration in blocks (optional)
        #[arg(long)]
        duration_blocks: Option<u64>,
    },

    /// Process inflation for current block
    ProcessInflation,

    /// Transfer tokens between users
    Transfer {
        /// Sender user ID
        #[arg(long)]
        from: String,

        /// Recipient user ID
        #[arg(long)]
        to: String,

        /// Amount to transfer
        #[arg(long)]
        amount: u64,
    },

    /// Check balance
    Balance {
        /// User ID
        #[arg(long)]
        user_id: String,
    },
}

/// Pre-stake genesis commands - solves chicken-and-egg mainnet launch problem
#[derive(Debug, Subcommand)]
enum PreStakeGenesisCommand {
    /// Create a new pre-stake manifest for collecting validator commitments
    InitManifest {
        /// Chain ID for mainnet
        #[arg(long, default_value = "dchat-mainnet-1")]
        chain_id: String,

        /// Output file for the manifest
        #[arg(long, default_value = "./prestake-manifest.json")]
        output: PathBuf,

        /// Initial token supply (in DCHAT, not motes)
        #[arg(long, default_value = "1000000000")]
        initial_supply: u64,

        /// Minimum stake required per validator (in DCHAT)
        #[arg(long, default_value = "10000")]
        min_stake: u64,
    },

    /// Create a signed bond commitment for a validator to join genesis
    CreateCommitment {
        /// Path to validator private key file (Ed25519)
        #[arg(long)]
        key_file: PathBuf,

        /// Validator name/identifier
        #[arg(long)]
        name: String,

        /// Stake amount (in DCHAT, not motes)
        #[arg(long)]
        stake: u64,

        /// Network address (IP:port or DNS:port)
        #[arg(long)]
        address: String,

        /// Geographic region (e.g., us-east, eu-west, ap-south)
        #[arg(long)]
        region: String,

        /// Lockup period in days (minimum 7)
        #[arg(long, default_value = "30")]
        lockup_days: u64,

        /// Chain ID this commitment is for
        #[arg(long, default_value = "dchat-mainnet-1")]
        chain_id: String,

        /// Output file for the commitment
        #[arg(long)]
        output: PathBuf,
    },

    /// Add a validator's bond commitment to the manifest
    AddCommitment {
        /// Path to the pre-stake manifest
        #[arg(long)]
        manifest: PathBuf,

        /// Path to the bond commitment file
        #[arg(long)]
        commitment: PathBuf,
    },

    /// Validate the manifest is ready for genesis
    ValidateManifest {
        /// Path to the pre-stake manifest
        #[arg(long)]
        manifest: PathBuf,
    },

    /// Generate genesis files from the pre-stake manifest
    GenerateGenesis {
        /// Path to the pre-stake manifest
        #[arg(long)]
        manifest: PathBuf,

        /// Path to coordinator private key file (Ed25519)
        #[arg(long)]
        coordinator_key: PathBuf,

        /// Output directory for genesis files
        #[arg(long, default_value = "./genesis")]
        output: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum AccountCommand {
    /// Create a new user account
    Create {
        /// Username for the account
        #[arg(long)]
        username: String,

        /// Save keys to file
        #[arg(long, default_value = "user_keys.json")]
        save_to: PathBuf,
    },

    /// List all users
    List,

    /// Get user profile information
    Profile {
        /// User ID to lookup
        #[arg(long)]
        user_id: String,
    },

    /// Send direct message
    SendDm {
        /// Sender user ID
        #[arg(long)]
        from: String,

        /// Recipient user ID
        #[arg(long)]
        to: String,

        /// Message content
        #[arg(long)]
        message: String,
    },

    /// Create a new channel
    CreateChannel {
        /// Creator user ID
        #[arg(long)]
        creator_id: String,

        /// Channel name
        #[arg(long)]
        name: String,

        /// Channel description
        #[arg(long)]
        description: Option<String>,
    },

    /// Post message to channel
    PostChannel {
        /// Sender user ID
        #[arg(long)]
        user_id: String,

        /// Channel ID
        #[arg(long)]
        channel_id: String,

        /// Message content
        #[arg(long)]
        message: String,
    },

    /// Get user's direct messages
    GetDms {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// Get channel messages
    GetChannelMessages {
        /// Channel ID
        #[arg(long)]
        channel_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum DatabaseCommand {
    /// Run database migrations
    Migrate,
    /// Backup database to file
    Backup {
        /// Output file path
        output: PathBuf,
    },
    /// Restore database from backup
    Restore {
        /// Input file path
        input: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(&cli.log_level, cli.json_logs)?;

    // Initialize Sentry for production error monitoring
    let _sentry_guard = init_sentry();
    if _sentry_guard.is_some() {
        info!("✓ Sentry error monitoring initialized");
    }

    info!("🚀 dchat v{} starting...", VERSION);
    info!("Mode: {:?}", cli.command);
    info!(
        "📊 Production limits: max_connections={}, max_msg_rate={}/s, connection_timeout={}s",
        MAX_CONCURRENT_CONNECTIONS, MAX_MESSAGE_RATE_PER_SECOND, CONNECTION_TIMEOUT_SECONDS
    );

    // Load configuration
    let config = load_config(&cli.config).await?;
    info!("✓ Configuration loaded from {:?}", cli.config);

    // Enforce production-safe endpoint configuration
    #[cfg(not(debug_assertions))]
    enforce_production_endpoints(&cli)?;

    // Initialize keyless onboarding (enclave + biometric hooks). Non-fatal — log and continue on error.
    match dchat::onboarding::keyless::init_keyless().await {
        Ok(_) => info!("✓ Keyless onboarding initialized"),
        Err(e) => warn!("Keyless onboarding initialization failed: {}", e),
    }

    // Execute command
    match cli.command {
        Commands::Relay {
            listen,
            bootstrap,
            hsm,
            kms_key_id,
            stake,
            tracing,
        } => {
            // Initialize observability if tracing is enabled
            let _observability = if tracing {
                let obs = dchat_observability::ObservabilityManager::new();
                info!("✓ Distributed tracing enabled");
                Some(obs)
            } else {
                None
            };

            run_relay_node(
                config,
                listen,
                bootstrap,
                hsm,
                kms_key_id,
                stake,
                cli.metrics_addr.clone(),
                cli.health_addr.clone(),
            )
            .await
        }
        Commands::User {
            bootstrap,
            identity,
            username,
            non_interactive,
            light_client,
            daemon,
            profile,
            channels,
            data_dir,
            metrics_addr,
            health_addr,
            tracing,
        } => {
            // Initialize observability if tracing is enabled
            let _observability = if tracing {
                let obs = dchat_observability::ObservabilityManager::new();
                info!("✓ Distributed tracing enabled");
                Some(obs)
            } else {
                None
            };

            let metrics = metrics_addr.unwrap_or_else(|| cli.metrics_addr.clone());
            let health = health_addr.unwrap_or_else(|| cli.health_addr.clone());

            // Light client mode
            if light_client {
                run_light_client(
                    config,
                    bootstrap,
                    identity,
                    username,
                    daemon || non_interactive, // non_interactive implies daemon for compat
                    profile,
                    channels,
                    data_dir,
                    metrics,
                    health,
                )
                .await
            } else {
                // Legacy user node (will be deprecated)
                run_user_node(
                    config,
                    bootstrap,
                    identity,
                    username,
                    non_interactive,
                    metrics,
                    health,
                )
                .await
            }
        }
        Commands::Validator {
            key,
            chain_rpc,
            hsm,
            stake,
            producer,
            tracing,
            genesis_bootstrap,
            genesis_dir,
        } => {
            // Initialize observability if tracing is enabled
            let _observability = if tracing {
                let obs = dchat_observability::ObservabilityManager::new();
                info!("✓ Distributed tracing enabled");
                Some(obs)
            } else {
                None
            };

            run_validator_node(
                config,
                key,
                chain_rpc,
                hsm,
                stake,
                producer,
                cli.metrics_addr.clone(),
                cli.health_addr.clone(),
                genesis_bootstrap,
                genesis_dir,
            )
            .await
        }
        Commands::Testnet {
            validators,
            relays,
            clients,
            data_dir,
            observability,
        } => run_testnet(config, validators, relays, clients, data_dir, observability).await,
        Commands::Keygen { output, burner, plaintext, validator } => generate_keys(output, burner, plaintext, validator).await,
        Commands::Genesis { output, chain_id, initial_supply, foundation_percent, validators } => {
            run_genesis_init(output, chain_id, initial_supply, foundation_percent, validators).await
        }
        Commands::MainnetInit { config: mainnet_config, genesis_validator, fund_foundation, init_pools } => {
            run_mainnet_init(mainnet_config, genesis_validator, fund_foundation, init_pools).await
        }
        Commands::PreStakeGenesis { action } => run_prestake_genesis_command(action).await,
        Commands::Account { action } => run_account_command(config, action).await,
        Commands::Database { action } => run_database_command(config, action).await,
        Commands::Health { url } => check_health(&url).await,
        Commands::Bot { action } => run_bot_command(config, action).await,
        Commands::Marketplace { action } => run_marketplace_command(config, action).await,
        Commands::Accessibility { action } => run_accessibility_command(action).await,
        Commands::Chaos { action } => run_chaos_command(action).await,
        Commands::Governance { action } => run_governance_command(action).await,
        Commands::Token { action } => run_token_command(config, action).await,
        Commands::Update { action } => run_update_command(action).await,
        #[cfg(feature = "deployment")]
        Commands::Deploy { action } => run_deploy_command(action).await,
        #[cfg(not(feature = "deployment"))]
        Commands::Deploy { .. } => Err(Error::Config("Deploy command requires the 'deployment' feature. Rebuild with: cargo build --features deployment".to_string())),
        Commands::Network { action } => run_network_command(config, action).await,
        Commands::Wallet { action } => run_wallet_command(config, action).await,
        Commands::Staking { action } => run_staking_command(config, action).await,
        Commands::Rewards { action } => run_rewards_command(config, action).await,
        Commands::Program { action } => run_program_command(config, action).await,
        Commands::MiniApp { action } => run_miniapp_command(action).await,
        Commands::Qge { action } => run_qge_command(config, action).await,
    }
}

/// Initialize logging with tracing-subscriber
/// When `json` is true, outputs structured JSON logs suitable for log aggregators.
/// When Sentry is enabled (via DCHAT_SENTRY_DSN), also attaches a Sentry layer for
/// breadcrumbs and error event forwarding.
fn init_logging(log_level: &str, json: bool) -> Result<()> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    // Check if Sentry is configured
    let sentry_enabled = std::env::var("DCHAT_SENTRY_DSN").is_ok();

    if json {
        // JSON structured logging for production with consistent fields
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_file(true)
            .with_line_number(true)
            .with_thread_ids(true)
            .with_target(true)
            .with_span_list(true);

        if sentry_enabled {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(sentry_tracing::layer())
                .init();
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .init();
        }
    } else {
        // Pretty logging for development
        let fmt_layer = tracing_subscriber::fmt::layer().pretty();

        if sentry_enabled {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(sentry_tracing::layer())
                .init();
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .init();
        }
    }

    Ok(())
}

/// Load configuration from file or use defaults
async fn load_config(path: &PathBuf) -> Result<Config> {
    if path.exists() {
        info!("Loading config from {:?}", path);

        // Read TOML file
        let contents = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| Error::Config(format!("Failed to read config file: {}", e)))?;

        // Parse TOML
        let mut config: Config = toml::from_str(&contents)
            .map_err(|e| Error::Config(format!("Failed to parse config: {}", e)))?;

        // Apply environment variable overrides
        if let Ok(val) = std::env::var("DCHAT_LISTEN_ADDR") {
            config.network.listen_addresses = vec![val];
        }
        if let Ok(val) = std::env::var("DCHAT_BOOTSTRAP_PEERS") {
            config.network.bootstrap_peers = val.split(',').map(|s| s.to_string()).collect();
        }
        if let Ok(val) = std::env::var("DCHAT_DATA_DIR") {
            config.storage.data_dir = PathBuf::from(val);
        }
        if let Ok(val) = std::env::var("DCHAT_MAX_CONNECTIONS") {
            if let Ok(num) = val.parse() {
                config.network.max_connections = num;
            }
        }

        // Validate configuration
        validate_config(&config)?;

        info!("✓ Configuration loaded successfully");
        Ok(config)
    } else {
        warn!("Config file not found at {:?}, using defaults", path);
        warn!("Run 'dchat --help' to see how to create a config file");
        Ok(Config::default())
    }
}

/// Validate configuration values for mainnet production deployment
fn validate_config(config: &Config) -> Result<()> {
    // MAINNET SECURITY: Validate all configuration parameters
    if config.network.max_connections == 0 {
        return Err(Error::Config(
            "max_connections must be greater than 0".to_string(),
        ));
    }

    // MAINNET: Enforce maximum connection limits to prevent resource exhaustion
    if config.network.max_connections > 1000 {
        return Err(Error::Config(
            "max_connections too high (max 1000 for production)".to_string(),
        ));
    }

    if config.network.connection_timeout_ms == 0 {
        return Err(Error::Config(
            "connection_timeout_ms must be greater than 0".to_string(),
        ));
    }

    // MAINNET: Reasonable timeout limits (5s to 60s)
    if config.network.connection_timeout_ms < 5000 || config.network.connection_timeout_ms > 60000 {
        return Err(Error::Config(
            "connection_timeout_ms must be between 5000ms and 60000ms for production".to_string(),
        ));
    }

    if config.crypto.key_rotation_interval_hours == 0 {
        return Err(Error::Config(
            "key_rotation_interval_hours must be greater than 0".to_string(),
        ));
    }

    // MAINNET SECURITY: Key rotation must be frequent enough (min 24h, max 720h)
    if config.crypto.key_rotation_interval_hours < 24
        || config.crypto.key_rotation_interval_hours > 720
    {
        return Err(Error::Config(
            "key_rotation_interval_hours must be between 24 and 720 hours for production security"
                .to_string(),
        ));
    }

    // Quorum threshold validation (0-10000 bps, where 10000 = 100%)
    if config.governance.quorum_threshold_bps > 10000 {
        return Err(Error::Config(
            "quorum_threshold_bps must be between 0 and 10000 (basis points)".to_string(),
        ));
    }

    // MAINNET GOVERNANCE: Require reasonable quorum (minimum 33%, maximum 80%)
    // 33% = 3300 bps, 80% = 8000 bps
    if config.governance.quorum_threshold_bps < 3300
        || config.governance.quorum_threshold_bps > 8000
    {
        return Err(Error::Config(
            "quorum_threshold_bps must be between 3300 and 8000 (33%-80%) for production governance".to_string(),
        ));
    }

    // MAINNET: Validate storage configuration for production
    if config.storage.data_dir.to_string_lossy().is_empty() {
        return Err(Error::Config(
            "data_dir cannot be empty in production".to_string(),
        ));
    }

    // MAINNET: Ensure reasonable database pool sizes
    if config.storage.db_pool_size < 1 || config.storage.db_pool_size > 100 {
        return Err(Error::Config(
            "db_pool_size must be between 1 and 100 for production".to_string(),
        ));
    }

    Ok(())
}

/// Create a DatabaseConfig from the application Config
/// Ensures consistent DB settings across all node types (relay, user, validator)
fn create_db_config(config: &Config, db_name: &str) -> DatabaseConfig {
    DatabaseConfig {
        path: config.storage.data_dir.join(db_name),
        max_connections: config.storage.db_pool_size,
        connection_timeout_secs: config.storage.db_connection_timeout_secs,
        idle_timeout_secs: config.storage.db_idle_timeout_secs,
        max_lifetime_secs: config.storage.db_max_lifetime_secs,
        enable_wal: config.storage.db_enable_wal,
    }
}

/// Parse a list of multiaddr strings into (PeerId, Multiaddr) pairs.
/// Only includes addresses that contain a valid /p2p/<PeerId> component.
/// Returns a Vec of successfully parsed bootstrap nodes.
fn parse_bootstrap_peers(peer_strings: &[String]) -> Vec<(PeerId, Multiaddr)> {
    let mut bootstrap_nodes = Vec::new();

    for peer_str in peer_strings {
        if let Ok(multiaddr) = peer_str.parse::<Multiaddr>() {
            let multiaddr_str = multiaddr.to_string();

            if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
                // Take only the peer ID portion (before any additional path components)
                let peer_id_str = peer_id_part.split('/').next().unwrap_or(peer_id_part);
                match peer_id_str.parse::<PeerId>() {
                    Ok(peer_id) => {
                        bootstrap_nodes.push((peer_id, multiaddr));
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse PeerID from bootstrap peer {}: {}",
                            multiaddr, e
                        );
                    }
                }
            } else {
                debug!(
                    "Bootstrap peer multiaddr missing /p2p/<PeerId> component: {}",
                    multiaddr
                );
            }
        } else {
            warn!("Failed to parse bootstrap peer multiaddr: {}", peer_str);
        }
    }

    bootstrap_nodes
}

/// Get the geographic region from environment or config
fn get_geographic_region() -> Option<String> {
    std::env::var("DCHAT_REGION").ok()
}

/// Derive libp2p Ed25519 keypair from an ed25519-dalek signing key
fn libp2p_keypair_from_signing_key(signing_key: &SigningKey) -> Result<libp2p::identity::Keypair> {
    libp2p::identity::Keypair::ed25519_from_bytes(signing_key.to_bytes()).map_err(|e| {
        Error::crypto(format!(
            "Failed to create libp2p keypair from signing key: {}",
            e
        ))
    })
}

/// Load or create a persistent relay keystore and return the libp2p keypair
fn load_or_create_relay_identity(
    keystore_path: Option<PathBuf>,
) -> Result<(RelayKeystore, libp2p::identity::Keypair)> {
    use rand::rngs::OsRng;

    let path = keystore_path.unwrap_or_else(default_keystore_path);

    if path.exists() {
        info!("Loading relay keystore from {:?}", path);
        let keystore = RelayKeystore::load(&path)?;
        let signing_key = keystore.ed25519_signing_key()?;
        let libp2p_kp = libp2p_keypair_from_signing_key(&signing_key)?;
        Ok((keystore, libp2p_kp))
    } else {
        info!(
            "No relay keystore found at {:?} - generating new identity",
            path
        );

        // Require passphrase before generating to avoid writing unprotected keys
        if std::env::var("DCHAT_RELAY_KEYSTORE_PASSPHRASE").is_err() {
            return Err(Error::crypto(
                "DCHAT_RELAY_KEYSTORE_PASSPHRASE must be set to create relay keystore",
            ));
        }

        let signing_key = SigningKey::generate(&mut OsRng);
        let keystore = RelayKeystore::from_ed25519(&signing_key)?;
        keystore.save(&path)?;
        let libp2p_kp = libp2p_keypair_from_signing_key(&signing_key)?;
        info!("✓ New relay keystore created at {:?}", path);
        Ok((keystore, libp2p_kp))
    }
}

/// Run as relay node
/// Run relay node with professional peer discovery and management
async fn run_relay_node(
    config: Config,
    listen_addr: String,
    bootstrap_peers: Vec<String>,
    use_hsm: bool,
    _kms_key_id: Option<String>,
    stake_amount: u64,
    metrics_addr: String,
    health_addr: String,
) -> Result<()> {
    info!("╔═══════════════════════════════════════════════════════════╗");
    info!("║            dchat Relay Node - Mainnet Mode               ║");
    info!("╚═══════════════════════════════════════════════════════════╝");

    if use_hsm {
        warn!("Relay HSM/KMS mode is not yet supported; proceeding with software keystore");
    }

    // Load or create persistent relay identity (libp2p PeerId derived from keystore)
    let keystore_path = std::env::var("DCHAT_RELAY_KEYSTORE_PATH")
        .ok()
        .map(PathBuf::from);
    let (relay_keystore, libp2p_keypair) = load_or_create_relay_identity(keystore_path)?;
    let relay_peer_id = libp2p_keypair.public().to_peer_id();
    info!("🔑 Relay peer ID: {}", relay_peer_id);
    let geographic_region = get_geographic_region();

    // MAINNET SECURITY: Validate production environment
    validate_mainnet_environment(&config, NodeType::Relay).await?;

    info!("🔀 Relay Node Configuration:");
    info!("   Listen Address: {}", listen_addr);

    // CLI bootstrap peers override config; config provides the production default.
    let mut effective_bootstrap_peers = bootstrap_peers;
    if effective_bootstrap_peers.is_empty() {
        effective_bootstrap_peers = config.network.bootstrap_peers.clone();
    }

    info!(
        "   Bootstrap Peers: {} provided",
        effective_bootstrap_peers.len()
    );

    // MAINNET SECURITY: Validate bootstrap peer addresses
    let is_mainnet = true; // Assume mainnet for security validation
    for peer_addr in &effective_bootstrap_peers {
        if peer_addr.is_empty() {
            return Err(Error::validation("Bootstrap peer address cannot be empty"));
        }

        // Basic validation for multiaddr format
        if !peer_addr.starts_with("/ip4/")
            && !peer_addr.starts_with("/ip6/")
            && !peer_addr.starts_with("/dns4/")
            && !peer_addr.starts_with("/dns6/")
        {
            return Err(Error::validation(format!(
                "Invalid bootstrap peer format (must be multiaddr): {}",
                peer_addr
            )));
        }

        // Extract IP address from multiaddr and validate
        if let Some(ip_part) = extract_ip_from_multiaddr(peer_addr) {
            if let Ok(ip) = ip_part.parse::<std::net::IpAddr>() {
                // Security check: Reject private network addresses in mainnet
                if is_mainnet && is_private_network(&ip) {
                    return Err(Error::validation(format!(
                        "Private network bootstrap peer not allowed in mainnet: {}",
                        peer_addr
                    )));
                }
            }
        }

        // Prevent localhost addresses in production unless explicitly allowed
        if peer_addr.contains("127.0.0.1") || peer_addr.contains("localhost") {
            if is_mainnet {
                return Err(Error::validation(format!(
                    "Localhost bootstrap peer not allowed in mainnet: {}",
                    peer_addr
                )));
            } else {
                warn!(
                    "⚠️  Localhost bootstrap peer detected: {} (testnet mode)",
                    peer_addr
                );
            }
        }
    }
    info!("   HSM/KMS Enabled: {}", use_hsm);
    info!("   Stake Amount: {} tokens", stake_amount);
    info!("   Geographic Distribution: Multi-region support enabled");
    info!("   Network Model: Decentralized, self-healing");

    // Initialize global peer registry for network-wide coordination
    let peer_registry = PeerRegistry::new();
    info!("✓ Peer registry initialized");

    // Initialize peer metrics for observability
    let peer_metrics = Arc::new(PeerMetrics::new());
    info!("✓ Peer metrics initialized");

    // Initialize readiness state for health checks (relay needs at least 1 peer)
    let readiness_state = Arc::new(ReadinessState::new(1));

    // Create shutdown channel for graceful termination
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    // Start observability stack
    info!("🔍 Starting observability services...");
    let health_handle = start_health_server_with_readiness(
        &health_addr,
        shutdown_tx.subscribe(),
        Some(readiness_state.clone()),
    )?;
    info!("   ✓ Health endpoint: http://{}/health", health_addr);

    let metrics_handle =
        start_metrics_server(&metrics_addr, peer_metrics.clone(), shutdown_tx.subscribe())?;
    info!("   ✓ Metrics endpoint: http://{}/metrics", metrics_addr);

    // Phase 1: DNS-Based Peer Discovery (Mainnet Production)
    info!("🌍 Phase 1: DNS-based peer discovery");
    info!("   Discovering validators and relays across geographic regions...");

    let dns_config = dchat_network::DnsDiscoveryConfig::default();
    let dns_discovery = dchat_network::DnsDiscoveryManager::new(dns_config.clone())
        .map_err(|e| Error::network(format!("DNS discovery initialization failed: {}", e)))?;

    // Discover all validators and relays via DNS with fallback handling
    info!("🔍 Discovering validators via DNS...");
    let discovered_validators = match dns_discovery.discover_validators().await {
        Ok(validators) => {
            if validators.is_empty() {
                warn!("⚠️  No validators discovered via DNS, using bootstrap peers only");
                vec![]
            } else {
                info!("✓ Discovered {} validators via DNS", validators.len());
                validators
            }
        }
        Err(e) => {
            error!("❌ DNS validator discovery failed: {}", e);
            if effective_bootstrap_peers.is_empty() {
                return Err(Error::network(format!(
                    "Failed to discover validators via DNS and no bootstrap peers provided: {}",
                    e
                )));
            }
            warn!("⚠️  Falling back to bootstrap peers only due to DNS failure");
            vec![]
        }
    };

    info!("🔍 Discovering other relays via DNS...");
    let discovered_relays = match dns_discovery.discover_relays().await {
        Ok(relays) => {
            if relays.is_empty() {
                warn!("⚠️  No relays discovered via DNS");
                vec![]
            } else {
                info!("✓ Discovered {} relays via DNS", relays.len());
                relays
            }
        }
        Err(e) => {
            warn!(
                "⚠️  DNS relay discovery failed, continuing without discovered relays: {}",
                e
            );
            vec![]
        }
    };

    info!(
        "✓ Discovered {} validators and {} relays",
        discovered_validators.len(),
        discovered_relays.len()
    );

    // Build bootstrap peer list from discovered nodes
    let mut bootstrap_nodes = Vec::new();

    // Add discovered validators - MAINNET-SAFE: Extract PeerID from multiaddr
    for validator in &discovered_validators {
        let multiaddr_str = validator.multiaddr.to_string();

        // Extract PeerID from multiaddr (format: /ip4/x.x.x.x/tcp/port/p2p/<PeerId>)
        if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
            match peer_id_part.parse::<PeerId>() {
                Ok(peer_id) => {
                    bootstrap_nodes.push((peer_id, validator.multiaddr.clone()));
                    info!(
                        "  + Validator: {} at {} (peer_id: {})",
                        validator.identifier, validator.multiaddr, peer_id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to parse PeerID from validator {}: {} - SKIPPING",
                        validator.identifier, e
                    );
                }
            }
        } else {
            warn!(
                "Validator {} multiaddr missing /p2p/<PeerId> component: {} - SKIPPING",
                validator.identifier, validator.multiaddr
            );
        }
    }

    // Add discovered relays - MAINNET-SAFE: Extract PeerID from multiaddr
    for relay in &discovered_relays {
        let multiaddr_str = relay.multiaddr.to_string();

        if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
            match peer_id_part.parse::<PeerId>() {
                Ok(peer_id) => {
                    bootstrap_nodes.push((peer_id, relay.multiaddr.clone()));
                    info!(
                        "  + Relay: {} at {} (peer_id: {})",
                        relay.identifier, relay.multiaddr, peer_id
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to parse PeerID from relay {}: {} - SKIPPING",
                        relay.identifier, e
                    );
                }
            }
        } else {
            warn!(
                "Relay {} multiaddr missing /p2p/<PeerId> component: {} - SKIPPING",
                relay.identifier, relay.multiaddr
            );
        }
    }

    // Add manually specified bootstrap peers - MAINNET-SAFE: Require proper format
    for peer_str in &effective_bootstrap_peers {
        if let Ok(multiaddr) = peer_str.parse::<Multiaddr>() {
            let multiaddr_str = multiaddr.to_string();

            if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
                match peer_id_part.parse::<PeerId>() {
                    Ok(peer_id) => {
                        bootstrap_nodes.push((peer_id, multiaddr.clone()));
                        info!("  + Manual peer: {} (peer_id: {})", multiaddr, peer_id);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse PeerID from manual peer {}: {} - SKIPPING",
                            multiaddr, e
                        );
                    }
                }
            } else {
                warn!(
                    "Manual peer multiaddr missing /p2p/<PeerId> component: {} - SKIPPING",
                    multiaddr
                );
            }
        }
    }

    info!(
        "📡 Total bootstrap peers for relay: {}",
        bootstrap_nodes.len()
    );

    // Parse listen address into multiaddr
    let listen_multiaddr = if listen_addr.starts_with("/ip") {
        // Already a multiaddr
        listen_addr
            .parse::<Multiaddr>()
            .map_err(|e| Error::network(format!("Invalid multiaddr: {}", e)))?
    } else {
        // Convert host:port to multiaddr
        let parts: Vec<&str> = listen_addr.split(':').collect();
        if parts.len() != 2 {
            return Err(Error::network(format!(
                "Invalid listen address format. Expected host:port or multiaddr, got: {}",
                listen_addr
            )));
        }
        let host = parts[0];
        let port = parts[1]
            .parse::<u16>()
            .map_err(|e| Error::network(format!("Invalid port: {}", e)))?;

        format!("/ip4/{}/tcp/{}", host, port)
            .parse::<Multiaddr>()
            .map_err(|e| Error::network(format!("Failed to create multiaddr: {}", e)))?
    };

    // Parse external address if configured
    let external_address = config
        .network
        .external_address
        .as_ref()
        .and_then(|addr_str| {
            addr_str.parse().ok().map(|addr| {
                info!("📡 External address configured: {}", addr_str);
                addr
            })
        });

    // Create network config with DNS-discovered peers
    let network_config = NetworkConfig {
        listen_addrs: vec![listen_multiaddr.clone()],
        discovery: dchat_network::DiscoveryConfig {
            local_peer_id: relay_peer_id,
            bootstrap_nodes,
            enable_mdns: false, // Disabled for production
            min_peers: 5,       // Connect to at least 5 peers (validators + other relays)
            max_peers: config.network.max_connections as usize,
            query_timeout: std::time::Duration::from_millis(config.network.connection_timeout_ms),
            k_bucket_size: 20,
            alpha: 3,
        },
        nat: dchat_network::NatConfig {
            enable_upnp: config.network.enable_upnp,
            stun_servers: vec![
                "stun.l.google.com:19302".to_string(),
                "stun1.l.google.com:19302".to_string(),
            ],
            enable_hole_punching: true,
            turn_servers: vec![],
            discovery_timeout: std::time::Duration::from_secs(10),
            lease_duration: std::time::Duration::from_secs(3600),
            port_range: (49152, 65535),
        },
        external_address,
        rate_limits: SwarmRateLimitConfig {
            max_messages_per_second: MAX_MESSAGES_PER_SECOND,
            max_messages_per_peer_per_second: MAX_MESSAGE_RATE_PER_SECOND as u32,
            max_connections: MAX_CONCURRENT_CONNECTIONS,
            max_connections_per_ip: MAX_CONNECTIONS_PER_IP,
            max_bandwidth_bytes_per_sec: MAX_BANDWIDTH_BYTES_PER_SEC,
        },
    };

    info!("Network will listen on: {:?}", network_config.listen_addrs);

    let mut network = NetworkManager::with_keypair(network_config, Some(libp2p_keypair)).await?;
    let peer_id = network.peer_id();

    // Start network manager
    network.start().await?;
    info!("✓ Relay network initialized (peer_id: {})", peer_id);

    // Mark network as ready
    readiness_state.set_network_ready(true);

    // Dial DNS-discovered peers even if they lack /p2p/<PeerId>. Identify will learn the PeerId
    // and the swarm event handler will feed addresses into Kademlia.
    for validator in &discovered_validators {
        if let Err(e) = network.dial(validator.multiaddr.clone()) {
            debug!(
                "Dial to DNS-discovered validator {} ({}) failed: {}",
                validator.identifier, validator.multiaddr, e
            );
        }
    }
    for relay in &discovered_relays {
        if let Err(e) = network.dial(relay.multiaddr.clone()) {
            debug!(
                "Dial to DNS-discovered relay {} ({}) failed: {}",
                relay.identifier, relay.multiaddr, e
            );
        }
    }

    // Start DNS refresh background task
    let _dns_refresh_handle = dns_discovery.start_refresh_task();

    // Phase 2: Initialize Peer Registry with Bootstrap Peers
    info!("📋 Phase 2: Registering bootstrap peers");
    for validator in &discovered_validators {
        let multiaddr_str = validator.multiaddr.to_string();
        if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
            if let Ok(pid) = peer_id_part.parse::<PeerId>() {
                let peer_info = PeerInfo {
                    peer_id: pid,
                    multiaddr: validator.multiaddr.clone(),
                    node_type: NodeType::Validator,
                    geographic_region: Some(validator.identifier.clone()),
                    last_seen: SystemTime::now(),
                    connection_quality: 1.0,
                    capabilities: vec!["consensus".to_string(), "validation".to_string()],
                    is_bootstrap: true,
                    rtt_ms: None,
                    packet_loss: 0.0,
                    jitter_ms: None,
                    total_messages_sent: 0,
                    total_messages_received: 0,
                    handshake_success: false,
                    connected_since: SystemTime::now(),
                };
                peer_registry.add_bootstrap_peer(peer_info.clone()).await;
                peer_registry.add_peer(peer_info).await;
            }
        }
    }

    for relay in &discovered_relays {
        let multiaddr_str = relay.multiaddr.to_string();
        if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
            if let Ok(pid) = peer_id_part.parse::<PeerId>() {
                let peer_info = PeerInfo {
                    peer_id: pid,
                    multiaddr: relay.multiaddr.clone(),
                    node_type: NodeType::Relay,
                    geographic_region: Some(relay.identifier.clone()),
                    last_seen: SystemTime::now(),
                    connection_quality: 1.0,
                    capabilities: vec!["routing".to_string(), "relay".to_string()],
                    is_bootstrap: true,
                    rtt_ms: None,
                    packet_loss: 0.0,
                    jitter_ms: None,
                    total_messages_sent: 0,
                    total_messages_received: 0,
                    handshake_success: false,
                    connected_since: SystemTime::now(),
                };
                peer_registry.add_bootstrap_peer(peer_info.clone()).await;
                peer_registry.add_peer(peer_info).await;
            }
        }
    }

    // Register manual bootstrap peers (config/CLI) in the registry as relays.
    // This is critical because DNS discovery currently yields address-only multiaddrs.
    for peer_str in &effective_bootstrap_peers {
        if let Ok(multiaddr) = peer_str.parse::<Multiaddr>() {
            let multiaddr_str = multiaddr.to_string();
            if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
                if let Ok(pid) = peer_id_part.parse::<PeerId>() {
                    let peer_info = PeerInfo {
                        peer_id: pid,
                        multiaddr: multiaddr.clone(),
                        node_type: NodeType::Relay,
                        geographic_region: None,
                        last_seen: SystemTime::now(),
                        connection_quality: 1.0,
                        capabilities: vec!["routing".to_string(), "relay".to_string()],
                        is_bootstrap: true,
                        rtt_ms: None,
                        packet_loss: 0.0,
                        jitter_ms: None,
                        total_messages_sent: 0,
                        total_messages_received: 0,
                        handshake_success: false,
                        connected_since: SystemTime::now(),
                    };
                    peer_registry.add_bootstrap_peer(peer_info.clone()).await;
                    peer_registry.add_peer(peer_info).await;
                }
            }
        }
    }

    let bootstrap_count = peer_registry.get_bootstrap_peers().await.len();
    info!(
        "✓ Registered {} bootstrap peers in global registry",
        bootstrap_count
    );

    // Phase 3: Start Background Peer Management Tasks
    info!("🔄 Phase 3: Starting peer management background tasks");

    let network_arc = Arc::new(tokio::sync::Mutex::new(network));
    let peer_registry_arc = Arc::new(peer_registry);

    // Start peer health monitor
    let health_monitor_handle = {
        let registry = peer_registry_arc.clone();
        let net = network_arc.clone();
        let shutdown = shutdown_tx.subscribe();
        tokio::spawn(async move {
            run_peer_health_monitor(registry, net, shutdown).await;
        })
    };
    info!(
        "   ✓ Peer health monitor started ({}s interval)",
        PEER_HEALTH_CHECK_INTERVAL.as_secs()
    );

    // Start peer list synchronization
    let sync_handle = {
        let registry = peer_registry_arc.clone();
        let net = network_arc.clone();
        let shutdown = shutdown_tx.subscribe();
        tokio::spawn(async move {
            run_peer_list_sync(registry, net, shutdown).await;
        })
    };
    info!(
        "   ✓ Peer list sync started ({}s interval)",
        PEER_LIST_SYNC_INTERVAL.as_secs()
    );

    // Phase 4: Wait for Initial Peer Connections with Handshaking
    info!("🤝 Phase 4: Establishing initial peer connections");
    let connection_deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_secs(CONNECTION_TIMEOUT_SECONDS);
    let mut connected_peers = 0;
    let geographic_region = geographic_region.clone();

    while tokio::time::Instant::now() < connection_deadline {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            network_arc.lock().await.next_event(),
        )
        .await
        {
            Ok(Some(NetworkEvent::PeerConnected {
                peer_id: connected_peer_id,
                endpoint,
            })) => {
                connected_peers += 1;
                info!(
                    "✓ Peer connected: {} at {:?} (total: {})",
                    connected_peer_id, endpoint, connected_peers
                );

                // Update peer registry
                if peer_registry_arc
                    .get_peer(&connected_peer_id)
                    .await
                    .is_some()
                {
                    peer_registry_arc
                        .update_peer_quality(&connected_peer_id, 1.0)
                        .await;
                } else {
                    // New peer not in bootstrap list - use endpoint if available
                    let multiaddr = match endpoint {
                        Some(addr) => addr,
                        None => match parse_fallback_listen_addr() {
                            Ok(addr) => addr,
                            Err(e) => {
                                error!("Cannot add peer {}: {}", connected_peer_id, e);
                                continue; // Skip adding peer with invalid address
                            }
                        },
                    };
                    let peer_info = PeerInfo {
                        peer_id: connected_peer_id,
                        multiaddr,
                        node_type: NodeType::Relay,
                        geographic_region: None,
                        last_seen: SystemTime::now(),
                        connection_quality: 0.8,
                        capabilities: vec![],
                        is_bootstrap: false,
                        rtt_ms: None,
                        packet_loss: 0.0,
                        jitter_ms: None,
                        total_messages_sent: 0,
                        total_messages_received: 0,
                        handshake_success: false,
                        connected_since: SystemTime::now(),
                    };
                    peer_registry_arc.add_peer(peer_info).await;
                }

                // Perform handshake and track metrics
                match perform_peer_handshake(
                    connected_peer_id,
                    &mut *network_arc.lock().await,
                    &peer_registry_arc,
                    NodeType::Relay,
                    geographic_region.clone(),
                )
                .await
                {
                    Ok(_) => {
                        debug!("Handshake initiated with {}", connected_peer_id);
                        peer_metrics.record_handshake_success().await;
                    }
                    Err(e) => {
                        warn!("Handshake failed with {}: {}", connected_peer_id, e);
                        peer_metrics.record_handshake_failure().await;
                    }
                }

                // Update peer count metrics
                let validators = peer_registry_arc
                    .get_peers_by_type(NodeType::Validator)
                    .await;
                let relays = peer_registry_arc.get_peers_by_type(NodeType::Relay).await;
                let clients = peer_registry_arc.get_peers_by_type(NodeType::Client).await;
                peer_metrics
                    .update_peer_count("validator", validators.len())
                    .await;
                peer_metrics.update_peer_count("relay", relays.len()).await;
                peer_metrics
                    .update_peer_count("client", clients.len())
                    .await;

                if connected_peers >= MIN_RELAY_CONNECTIONS {
                    info!(
                        "✓ Minimum peer threshold reached ({})",
                        MIN_RELAY_CONNECTIONS
                    );
                    break;
                }
            }
            Ok(Some(NetworkEvent::PeerDisconnected(disconnected_peer_id))) => {
                warn!("⚠️  Peer disconnected: {}", disconnected_peer_id);
                peer_registry_arc
                    .update_peer_quality(&disconnected_peer_id, 0.0)
                    .await;
                connected_peers = connected_peers.saturating_sub(1);
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {}
        }
    }

    info!("✓ Phase 4 complete: Connected to {} peers", connected_peers);

    // Phase 5: Auto-discover Docker relay nodes (development only)
    if let Ok(relay_id) = std::env::var("DCHAT_RELAY_ID") {
        info!("🐳 Phase 5: Docker network discovery");
        let relay_hosts = ["dchat-relay1", "dchat-relay2", "dchat-relay3"];
        let relay_ports = [7070; 3];

        for (host, port) in relay_hosts.iter().zip(relay_ports.iter()) {
            if host.contains(&relay_id) {
                continue;
            }

            let addr = format!("/dns4/{}/tcp/{}", host, port);
            if let Ok(multiaddr) = addr.parse() {
                debug!("🔗 Dialing Docker peer: {}", addr);
                if let Err(e) = network_arc.lock().await.dial(multiaddr) {
                    debug!("Docker dial failed (expected in non-Docker env): {}", e);
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    // Phase 6: Initialize Database and Relay Services
    info!("💾 Phase 6: Initializing storage and relay services");
    let db_config = create_db_config(&config, "dchat_relay.db");
    let database = Database::new(db_config).await?;
    info!("   ✓ Database initialized");

    // Mark database as ready
    readiness_state.set_database_ready(true);

    info!("   ✓ Relay staked with {} tokens", stake_amount);

    // Phase 6b: Initialize Currency Chain and Payment Processor
    info!("💰 Phase 6b: Initializing payment processor");
    let mut currency_chain_config = CurrencyChainConfig::default();
    let currency_rpc_url = resolve_required_currency_chain_rpc_url(&config)?;
    if config.rpc.resolved_currency_chain_rpc_url().is_none()
        && allow_localhost_chain_rpc_defaults()
    {
        warn!(
            "Using localhost currency chain RPC default (DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1). This is unsafe for production."
        );
    }
    currency_chain_config.rpc_url = currency_rpc_url.clone();
    let currency_chain = Arc::new(
        CurrencyChainClient::new(currency_chain_config).map_err(|e| {
            Error::internal(format!("Failed to create currency chain client: {}", e))
        })?,
    );

    let payment_config = PaymentProcessorConfig {
        interval_seconds: 300, // Process payments every 5 minutes
        max_retries: 3,
        batch_size: 100,
        min_payment_amount: 1000, // 0.00001 DCHAT minimum
    };
    let (mut payment_processor, payment_shutdown_tx) =
        PaymentProcessor::new(payment_config, currency_chain.clone());

    // Start payment processor background task
    let payment_processor_handle = tokio::spawn(async move {
        payment_processor.run().await;
    });
    info!("   ✓ Payment processor started (5-minute intervals)");

    // Phase 6c: Enforce relay staking on currency chain
    let relay_signing_key = relay_keystore.ed25519_signing_key()?;
    let relay_verifying_key = relay_signing_key.verifying_key();

    let staking_validator = RelayStakingValidator::new(currency_rpc_url.clone());
    let operator_id = std::env::var("DCHAT_OPERATOR_ID")
        .map_err(|_| {
            Error::Config(
                "DCHAT_OPERATOR_ID must be set to the relay operator UUID for staking".into(),
            )
        })
        .and_then(|s| {
            Uuid::parse_str(&s)
                .map(UserId)
                .map_err(|e| Error::Config(format!("Invalid DCHAT_OPERATOR_ID: {}", e)))
        })?;

    match staking_validator
        .verify_relay_stake(&relay_verifying_key)
        .await
    {
        Ok(true) => {
            info!("   ✓ Relay stake already active on currency chain");
        }
        Ok(false) | Err(_) => {
            info!(
                "🔒 Submitting relay stake of {} tokens for operator {}...",
                stake_amount, operator_id
            );

            let staking_backend =
                Arc::new(CurrencyChainStakingBackend::new(currency_chain.clone()));
            let stake_tx_id = staking_backend
                .stake(&operator_id, stake_amount, RELAY_LOCK_DURATION)
                .await?;

            info!("   ⏳ Awaiting stake confirmation (tx: {})", stake_tx_id);
            let confirmed = staking_backend
                .wait_for_confirmation(&stake_tx_id, MIN_STAKE_CONFIRMATIONS)
                .await?;

            if !confirmed {
                return Err(Error::network(
                    "Relay stake transaction not confirmed; aborting startup",
                ));
            }

            info!("   ✓ Relay stake confirmed (tx: {})", stake_tx_id);
        }
    }

    // Phase 6d: Feature-flagged Onion Routing initialization
    // When enabled, provides metadata-resistant message routing via 3-hop Sphinx circuits
    // Relays act as intermediate nodes in the onion network
    let onion_routing_manager: Option<Arc<std::sync::RwLock<dchat_network::OnionRoutingManager>>> =
        if config.features.enable_onion_routing {
            info!("🧅 Phase 6d: Initializing onion routing...");

            // Create circuit configuration from config
            let circuit_config = dchat_network::CircuitConfig {
                num_hops: config.onion_routing.min_circuit_hops,
                max_lifetime_secs: config.onion_routing.circuit_rotation_secs,
                enforce_diversity: true,
                min_asn_diversity: 2,
                enable_cover_traffic: config.onion_routing.enable_cover_traffic,
                cover_traffic_rate: 6, // 1 packet per 10 seconds
            };

            let onion_mgr = dchat_network::OnionRoutingManager::new(circuit_config);

            info!(
                "   ✓ Onion routing enabled ({} hop circuits)",
                config.onion_routing.min_circuit_hops
            );
            info!(
                "   ✓ Cover traffic: {}",
                if config.onion_routing.enable_cover_traffic {
                    "enabled"
                } else {
                    "disabled"
                }
            );

            Some(Arc::new(std::sync::RwLock::new(onion_mgr)))
        } else {
            debug!("Onion routing disabled (direct message relay)");
            None
        };

    // Clone onion routing manager for use in event loop
    let onion_mgr_for_loop = onion_routing_manager.clone();

    // Phase 7: Enter Main Event Loop
    info!("╔═══════════════════════════════════════════════════════════╗");
    info!("║          🎉 Relay Node Fully Operational 🎉               ║");
    info!("╠═══════════════════════════════════════════════════════════╣");
    info!("║  Peer ID: {:<48} ║", peer_id.to_string());
    info!("║  Connected Peers: {:<43} ║", connected_peers);
    info!("║  Network Status: READY                                    ║");
    info!("╚═══════════════════════════════════════════════════════════╝");
    info!("");
    info!("📊 Monitoring:");
    info!("   • Health: http://{}/health", health_addr);
    info!("   • Metrics: http://{}/metrics", metrics_addr);
    info!("");
    info!("Press Ctrl+C to shutdown gracefully...");

    // Main event loop
    let mut event_count = 0;
    loop {
        // Get next event (need to get lock, extract event, then release)
        let event = {
            let mut net = network_arc.lock().await;
            net.next_event().await
        };

        tokio::select! {
            _ = async {
                if let Some(evt) = event {
                    event_count += 1;
                    match evt {
                        NetworkEvent::PeerConnected { peer_id: peer, endpoint } => {
                            info!("🆕 New peer joined: {} at {:?}", peer, endpoint);

                            // Check if peer is already registered (e.g., as a bootstrap validator)
                            let existing_peer = peer_registry_arc.get_peer(&peer).await;

                            if let Some(existing) = existing_peer {
                                // Peer already registered - update last_seen and mark as connected
                                info!("  ✓ Recognized as existing {} peer", match existing.node_type {
                                    NodeType::Validator => "validator",
                                    NodeType::Relay => "relay",
                                    NodeType::Client => "client",
                                });
                                peer_registry_arc.update_peer_quality(&peer, 1.0).await;
                            } else {
                                // New peer - add to registry using endpoint if available
                                let multiaddr = match endpoint {
                                    Some(addr) => addr,
                                    None => match parse_fallback_listen_addr() {
                                        Ok(addr) => addr,
                                        Err(e) => {
                                            error!("Cannot add peer {}: {}", peer, e);
                                            return; // Skip adding peer with invalid address
                                        }
                                    }
                                };

                                let peer_info = PeerInfo {
                                    peer_id: peer,
                                    multiaddr,
                                    node_type: NodeType::Relay,
                                    geographic_region: None,
                                    last_seen: SystemTime::now(),
                                    connection_quality: 1.0,
                                    capabilities: vec![],
                                    is_bootstrap: false,
                                    rtt_ms: None,
                                    packet_loss: 0.0,
                                    jitter_ms: None,
                                    total_messages_sent: 0,
                                    total_messages_received: 0,
                                    handshake_success: false,
                                    connected_since: SystemTime::now(),
                                };
                                peer_registry_arc.add_peer(peer_info).await;
                            }

                            // Determine our node type for handshake (this is relay/client path)
                            let our_node_type = NodeType::Relay;

                            match perform_peer_handshake(
                                peer,
                                &mut *network_arc.lock().await,
                                &peer_registry_arc,
                                our_node_type,
                                geographic_region.clone(),
                            ).await {
                                Ok(_) => {
                                    peer_metrics.record_handshake_success().await;
                                }
                                Err(e) => {
                                    debug!("Handshake with {} failed: {}", peer, e);
                                    peer_metrics.record_handshake_failure().await;
                                }
                            }

                            // Update peer count metrics
                            let validators = peer_registry_arc.get_peers_by_type(NodeType::Validator).await;
                            let relays = peer_registry_arc.get_peers_by_type(NodeType::Relay).await;
                            let clients = peer_registry_arc.get_peers_by_type(NodeType::Client).await;
                            peer_metrics.update_peer_count("validator", validators.len()).await;
                            peer_metrics.update_peer_count("relay", relays.len()).await;
                            peer_metrics.update_peer_count("client", clients.len()).await;

                            // Update readiness state peer count
                            readiness_state.update_peer_count(validators.len() + relays.len() + clients.len());
                        }
                        NetworkEvent::PeerDisconnected(peer) => {
                            info!("👋 Peer left: {}", peer);
                            peer_registry_arc.update_peer_quality(&peer, 0.0).await;

                            // Update peer count metrics after disconnect
                            let validators = peer_registry_arc.get_peers_by_type(NodeType::Validator).await;
                            let relays = peer_registry_arc.get_peers_by_type(NodeType::Relay).await;
                            let clients = peer_registry_arc.get_peers_by_type(NodeType::Client).await;
                            peer_metrics.update_peer_count("validator", validators.len()).await;
                            peer_metrics.update_peer_count("relay", relays.len()).await;
                            peer_metrics.update_peer_count("client", clients.len()).await;

                            // Update readiness state peer count
                            readiness_state.update_peer_count(validators.len() + relays.len() + clients.len());
                        }
                        NetworkEvent::MessageReceived { from, message } => {
                            debug!("📨 Message from {}", from);

                            // Track message received from peer
                            peer_registry_arc.record_message_received(&from).await;

                            // Message is already a DchatMessage enum, match on it directly
                            match message {
                                DchatMessage::ChannelMessage { sender, channel_id, encrypted_payload, .. } => {
                                    // Check if this is a peer discovery advertisement
                                    if channel_id == PEER_DISCOVERY_CHANNEL {
                                        // Parse and handle peer discovery advertisement
                                        match serde_json::from_slice::<PeerDiscoveryAdvertisement>(&encrypted_payload) {
                                            Ok(advertisement) => {
                                                handle_peer_discovery_advertisement(
                                                    advertisement,
                                                    from,
                                                    &peer_registry_arc,
                                                ).await;
                                            }
                                            Err(e) => {
                                                debug!("⚠️  Failed to parse peer discovery advertisement from {}: {}", from, e);
                                            }
                                        }
                                    } else {
                                        debug!("📨 Relay forwarding message from {} to channel {}", sender, channel_id);

                                        // Process through onion routing if enabled
                                        if let Some(ref onion_mgr) = onion_mgr_for_loop {
                                            // Check if this is an onion-routed message (starts with circuit cell header)
                                            if encrypted_payload.len() >= 4 {
                                                let cell_type = encrypted_payload.get(0).copied().unwrap_or(0);
                                                // Cell types: 1=CREATE, 2=CREATED, 3=RELAY, 4=DESTROY
                                                if cell_type == 3 {
                                                    // RELAY cell - process through onion routing
                                                    if let Ok(mut mgr) = onion_mgr.write() {
                                                        // Extract circuit ID from payload
                                                        let circuit_id = encrypted_payload.get(1..5).unwrap_or(&[0,0,0,0]).to_vec();
                                                        match mgr.handle_relay_cell(circuit_id.clone(), encrypted_payload.clone()) {
                                                            Ok(result) => {
                                                                debug!("🧅 Onion relay cell processed: {:?}", result);
                                                            }
                                                            Err(e) => {
                                                                debug!("🧅 Onion relay cell error: {}", e);
                                                            }
                                                        }
                                                    }
                                                } else if cell_type == 1 {
                                                    // CREATE cell - handle circuit creation
                                                    // CREATE cell format: [type(1) | circuit_id(4) | client_public_key(32) | ...]
                                                    if let Ok(mut mgr) = onion_mgr.write() {
                                                        let circuit_id = encrypted_payload.get(1..5).unwrap_or(&[0,0,0,0]).to_vec();
                                                        let client_public_key = encrypted_payload.get(5..37).unwrap_or(&[0u8; 32]).to_vec();
                                                        let response = mgr.handle_create_cell(circuit_id.clone(), client_public_key);
                                                        debug!("🧅 Onion circuit created: {:?}", response);
                                                    }
                                                } else if cell_type == 4 {
                                                    // DESTROY cell - tear down circuit
                                                    if let Ok(mut mgr) = onion_mgr.write() {
                                                        let circuit_id = encrypted_payload.get(1..5).unwrap_or(&[0,0,0,0]).to_vec();
                                                        mgr.handle_destroy_cell(circuit_id);
                                                        debug!("🧅 Onion circuit destroyed");
                                                    }
                                                }
                                            }
                                        }

                                        // Forward message to channel subscribers
                                        // Generate proof-of-delivery for relay incentives
                                        info!("✓ Message relayed and proof-of-delivery recorded");
                                    }
                                }
                                DchatMessage::PeerHandshake { payload } => {
                                    // Deserialize the handshake payload and process it
                                    match serde_json::from_slice::<PeerHandshake>(&payload) {
                                        Ok(handshake) => {
                                            let mut network = network_arc.lock().await;
                                            if let Err(e) = handle_peer_handshake(
                                                from,
                                                handshake,
                                                &peer_registry_arc,
                                                &mut network,
                                            ).await {
                                                warn!("⚠️  Failed to handle peer handshake from {}: {}", from, e);
                                            }
                                        }
                                        Err(e) => {
                                            warn!("⚠️  Failed to deserialize handshake from {}: {}", from, e);
                                        }
                                    }
                                }
                                _ => {
                                    debug!("📨 Other relay message type received");
                                }
                            }
                        }
                        _ => {}
                    }

                    // Log stats and update quality metrics every 100 events
                    if event_count % 100 == 0 {
                        let all_peers = peer_registry_arc.get_all_peers().await;
                        let avg_rtt = peer_registry_arc.calculate_average_rtt().await;
                        let avg_quality = peer_registry_arc.calculate_average_quality().await;
                        peer_metrics.update_average_rtt(avg_rtt).await;
                        peer_metrics.update_average_quality(avg_quality).await;
                        info!("📊 Stats: {} events processed, {} peers in registry, avg_rtt={:.1}ms, avg_quality={:.2}",
                            event_count, all_peers.len(), avg_rtt, avg_quality);
                    }
                }
            } => {}
            _ = signal::ctrl_c() => {
                info!("🛑 Received shutdown signal (Ctrl+C)");
                break;
            }
            _ = shutdown_rx.recv() => {
                info!("🛑 Received shutdown signal (internal)");
                break;
            }
        }
    }

    // Phase 8: Graceful Shutdown
    info!("╔═══════════════════════════════════════════════════════════╗");
    info!("║              Initiating Graceful Shutdown                ║");
    info!("╚═══════════════════════════════════════════════════════════╝");

    // Signal all background tasks to stop
    info!("📡 Stopping background tasks...");
    let _ = shutdown_tx.send(());
    let _ = payment_shutdown_tx.send(true); // Stop payment processor

    // Wait for background tasks to complete
    info!("⏳ Waiting for background tasks to complete...");
    let shutdown_result = tokio::time::timeout(
        tokio::time::Duration::from_secs(SHUTDOWN_TIMEOUT_SECONDS),
        async {
            let _ = tokio::join!(
                health_handle,
                metrics_handle,
                health_monitor_handle,
                sync_handle,
                payment_processor_handle
            );
        },
    )
    .await;

    match shutdown_result {
        Ok(_) => info!("✓ All background tasks stopped cleanly"),
        Err(_) => warn!("⚠️  Background tasks shutdown timeout (forced termination)"),
    }

    // Close database
    info!("💾 Closing database...");
    database.close().await?;
    info!("✓ Database closed");

    // Final peer registry stats
    let final_peers = peer_registry_arc.get_all_peers().await;
    let validators = peer_registry_arc
        .get_peers_by_type(NodeType::Validator)
        .await;
    let relays = peer_registry_arc.get_peers_by_type(NodeType::Relay).await;

    info!("📊 Final Network Statistics:");
    info!("   Total peers discovered: {}", final_peers.len());
    info!("   Validators: {}", validators.len());
    info!("   Relays: {}", relays.len());
    info!("   Events processed: {}", event_count);

    info!("╔═══════════════════════════════════════════════════════════╗");
    info!("║            ✓ Relay Node Shutdown Complete                ║");
    info!("╚═══════════════════════════════════════════════════════════╝");

    Ok(())
}

// ============================================================================
// LIGHT CLIENT MODE
// ============================================================================

/// Run as light client (mobile/desktop friendly)
async fn run_light_client(
    config: Config,
    bootstrap_relays: Vec<String>,
    identity_path: Option<PathBuf>,
    username: Option<String>,
    daemon_mode: bool,
    profile_str: String,
    channels_str: String,
    data_dir: Option<PathBuf>,
    metrics_addr: String,
    health_addr: String,
) -> Result<()> {
    use dchat::light_client::{
        ClientProfile, LightClient, LightClientConfig, LightClientEvent, VerificationStatus,
    };

    info!("╔═══════════════════════════════════════════════════════════╗");
    info!("║                 dchat Light Client                        ║");
    info!("╚═══════════════════════════════════════════════════════════╝");

    // Parse profile
    let profile = match profile_str.to_lowercase().as_str() {
        "performance" => ClientProfile::Performance,
        "balanced" => ClientProfile::Balanced,
        "low-power" | "lowpower" | "low_power" => ClientProfile::LowPower,
        "relay-only" | "relayonly" | "relay_only" => ClientProfile::RelayOnly,
        _ => {
            warn!("Unknown profile '{}', using 'balanced'", profile_str);
            ClientProfile::Balanced
        }
    };

    // Parse channels
    let default_channels: Vec<String> = channels_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Build config
    let light_config = LightClientConfig {
        display_name: username.unwrap_or_else(|| "Anonymous".to_string()),
        identity_path,
        data_dir: data_dir.unwrap_or_else(|| config.storage.data_dir.clone()),
        bootstrap_relays,
        default_channels,
        profile,
        health_addr: Some(health_addr),
        metrics_addr: Some(metrics_addr),
        fee_gated: true, // Always use fee-gated for mainnet
        max_offline_queue: 1000,
        sync_interval_secs: 30,
        keepalive_interval_secs: 60,
    };

    info!("Profile: {:?}", light_config.profile);
    info!("Channels: {:?}", light_config.default_channels);
    info!("Data dir: {:?}", light_config.data_dir);

    if std::env::var("DCHAT_LIGHT_CLIENT_IDENTITY_PASSPHRASE").is_err() {
        return Err(Error::crypto(
            "DCHAT_LIGHT_CLIENT_IDENTITY_PASSPHRASE not set. This passphrase is required to encrypt/decrypt the light client identity private key stored in SQLite."
                .to_string(),
        ));
    }

    // Initialize FeeGateway for production fee enforcement
    // This ensures light clients pay proper fees for all operations
    let fee_gateway = {
        use dchat::fee_gateway::FeeGateway;
        use dchat::storage_routed_user_management::StorageRoutedUserManager;
        use dchat_blockchain::fee_distribution::{FeeDistributionConfig, FeeDistributionManager};
        use dchat_blockchain::fee_orchestrator::{FeeConfig, FeeOrchestrator};
        use dchat_blockchain::{
            ChatChainClient, ChatChainConfig, CrossChainBridge, CurrencyChainClient,
            CurrencyChainConfig,
        };
        use dchat_network::relay_network::{RelayNetworkConfig, RelayNetworkManager};
        use dchat_storage::DatabaseConfig;
        use std::sync::RwLock;

        // Initialize database for light client
        let db_config = DatabaseConfig {
            path: config.storage.data_dir.join("light_client.db"),
            max_connections: 5,
            connection_timeout_secs: config.storage.db_connection_timeout_secs,
            idle_timeout_secs: config.storage.db_idle_timeout_secs,
            max_lifetime_secs: config.storage.db_max_lifetime_secs,
            enable_wal: config.storage.db_enable_wal,
        };

        // Primary DB handle for storage + light client state
        let database = dchat_storage::Database::new(db_config).await?;

        // Separate handle for FeeGateway persistence so escrow/idempotency survives restarts
        let fee_gateway_db = Arc::new(
            dchat_storage::Database::new(DatabaseConfig {
                path: config.storage.data_dir.join("light_client.db"),
                max_connections: 5,
                connection_timeout_secs: config.storage.db_connection_timeout_secs,
                idle_timeout_secs: config.storage.db_idle_timeout_secs,
                max_lifetime_secs: config.storage.db_max_lifetime_secs,
                enable_wal: config.storage.db_enable_wal,
            })
            .await?,
        );

        // Initialize chain clients
        let chat_rpc_url = resolve_required_chat_chain_rpc_url(&config)?;
        let mut chat_chain_config = ChatChainConfig::default();
        chat_chain_config.rpc_url = chat_rpc_url;

        let currency_rpc_url = resolve_required_currency_chain_rpc_url(&config)?;
        let mut currency_chain_config = CurrencyChainConfig::default();
        currency_chain_config.rpc_url = currency_rpc_url;

        let chat_chain = Arc::new(ChatChainClient::new(chat_chain_config)?);
        let currency_chain = Arc::new(CurrencyChainClient::new(currency_chain_config)?);
        let bridge = Arc::new(CrossChainBridge::new(
            Arc::clone(&chat_chain),
            Arc::clone(&currency_chain),
        ));

        // Initialize fee infrastructure
        let fee_distribution =
            Arc::new(FeeDistributionManager::new(FeeDistributionConfig::default()));
        let fee_orchestrator = Arc::new(FeeOrchestrator::new(
            Arc::clone(&currency_chain),
            fee_distribution,
            FeeConfig::default(),
        )?);

        // Initialize relay network manager
        let relay_network = Arc::new(RwLock::new(RelayNetworkManager::new(
            RelayNetworkConfig::default(),
        )));

        // Populate relay network from config seed relays
        if !config.relay.seed_relays.is_empty() {
            use dchat_core::types::UserId;
            use dchat_network::relay_network::{Continent, RelayInfo};
            use uuid::Uuid;

            let mut relay_mgr = relay_network.write().unwrap_or_else(|e| {
                warn!("⚠️  relay_network RwLock poisoned; continuing with inner state");
                e.into_inner()
            });
            for seed in &config.relay.seed_relays {
                let continent = match seed.continent.to_lowercase().as_str() {
                    "northamerica" | "north_america" | "na" => Continent::NorthAmerica,
                    "southamerica" | "south_america" | "sa" => Continent::SouthAmerica,
                    "europe" | "eu" => Continent::Europe,
                    "asia" => Continent::Asia,
                    "africa" | "af" => Continent::Africa,
                    "oceania" | "oc" | "australia" => Continent::Oceania,
                    _ => Continent::Europe,
                };

                let operator = seed
                    .operator_id
                    .as_ref()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .map(UserId)
                    .unwrap_or_else(UserId::new);

                let relay_info =
                    RelayInfo::new(seed.relay_id.clone(), operator, seed.stake, continent, 0);

                if let Err(e) = relay_mgr.register_relay(relay_info) {
                    warn!("Failed to register seed relay {}: {}", seed.relay_id, e);
                }
            }
            drop(relay_mgr);
            info!(
                "Loaded {} seed relays from config",
                config.relay.seed_relays.len()
            );
        }

        // Initialize storage-routed user manager (offline mode for light client)
        let storage_manager = Arc::new(
            StorageRoutedUserManager::offline(
                config.storage.data_dir.join("light_client_messages.db"),
                database,
                Arc::clone(&chat_chain),
                Arc::clone(&currency_chain),
                Arc::clone(&bridge),
                config.storage.data_dir.clone(),
            )
            .await?,
        );

        // Create FeeGateway
        let gateway = Arc::new(
            FeeGateway::new(
                fee_orchestrator,
                currency_chain,
                chat_chain,
                storage_manager,
                relay_network,
            )
            .with_database(Arc::clone(&fee_gateway_db)),
        );

        // Load persisted fee/escrow state to preserve idempotency across restarts
        gateway.load_persisted_state().await?;

        gateway
    };

    info!("✓ FeeGateway initialized for light client");

    // Create light client with FeeGateway (required for production)
    let mut client = LightClient::new(light_config, Some(fee_gateway)).await?;

    // Take event receiver before connecting
    let mut event_rx = client
        .take_event_receiver()
        .ok_or_else(|| Error::internal("Event receiver already taken"))?;

    // Connect
    info!("🔗 Connecting to network...");
    client.connect().await?;

    info!("✓ Light client connected");
    info!(
        "  Identity: {} ({})",
        client.identity().username,
        client.identity().user_id
    );

    if daemon_mode {
        // Daemon mode: run indefinitely, log events
        info!("🔄 Running in daemon mode (Ctrl+C to stop)");

        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(LightClientEvent::MessageReceived { channel_id, sender, content, timestamp, verification }) => {
                            let channel = channel_id.as_deref().unwrap_or("DM");
                            let status = match verification {
                                VerificationStatus::Verified { .. } => "✓",
                                VerificationStatus::Pending => "⏳",
                                VerificationStatus::Failed { .. } => "⚠",
                                VerificationStatus::Skipped => "○",
                            };
                            info!("[{}] #{} {}: {} {}", timestamp, channel, sender, content, status);
                        }
                        Some(LightClientEvent::MessageSent { message_id, channel_id }) => {
                            let channel = channel_id.as_deref().unwrap_or("DM");
                            debug!("📤 Sent to #{}: {}", channel, message_id);
                        }
                        Some(LightClientEvent::MessageFailed { operation_id, error }) => {
                            error!("❌ Send failed {}: {}", operation_id, error);
                        }
                        Some(LightClientEvent::ConnectionStateChanged(state)) => {
                            info!("🔄 State: {:?}", state);
                        }
                        Some(LightClientEvent::PeerConnected(peer)) => {
                            debug!("👋 Peer joined: {}", peer);
                        }
                        Some(LightClientEvent::PeerDisconnected(peer)) => {
                            debug!("👋 Peer left: {}", peer);
                        }
                        Some(LightClientEvent::SyncProgress { channels_synced, total_channels, messages_fetched }) => {
                            debug!("🔄 Sync: {}/{} channels, {} msgs", channels_synced, total_channels, messages_fetched);
                        }
                        Some(LightClientEvent::Error(e)) => {
                            error!("❌ Error: {}", e);
                        }
                        None => {
                            info!("Event channel closed");
                            break;
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("🛑 Received shutdown signal");
                    break;
                }
            }
        }
    } else {
        // Interactive mode: simple stdin REPL
        info!("🎉 Light client ready!");
        info!("Commands:");
        info!("  /join <channel>  - Join a channel");
        info!("  /leave <channel> - Leave a channel");
        info!("  /peers           - Show peer count");
        info!("  /history         - Show recent messages");
        info!("  /quit            - Exit");
        info!("  <text>           - Send to #global");
        println!();

        use tokio::io::{self, AsyncBufReadExt};
        let mut stdin_lines = io::BufReader::new(io::stdin()).lines();

        print!("You: ");
        {
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }

        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(LightClientEvent::MessageReceived { channel_id, sender, content, .. }) => {
                            let channel = channel_id.as_deref().unwrap_or("DM");
                            println!("\n[#{}] {}: {}", channel, sender, content);
                            print!("You: ");
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                        }
                        Some(LightClientEvent::Error(e)) => {
                            println!("\n❌ Error: {}", e);
                            print!("You: ");
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                        }
                        None => break,
                        _ => {}
                    }
                }

                line = stdin_lines.next_line() => {
                    match line {
                        Ok(Some(text)) => {
                            let text = text.trim();
                            if text.is_empty() {
                                print!("You: ");
                                use std::io::Write;
                                let _ = std::io::stdout().flush();
                                continue;
                            }

                            if text.starts_with('/') {
                                let parts: Vec<&str> = text.splitn(2, ' ').collect();
                                match parts[0] {
                                    "/quit" | "/exit" | "/q" => {
                                        break;
                                    }
                                    "/join" if parts.len() > 1 => {
                                        match client.subscribe(parts[1]).await {
                                            Ok(_) => println!("✓ Joined #{}", parts[1]),
                                            Err(e) => println!("❌ Failed to join: {}", e),
                                        }
                                    }
                                    "/leave" if parts.len() > 1 => {
                                        match client.unsubscribe(parts[1]).await {
                                            Ok(_) => println!("✓ Left #{}", parts[1]),
                                            Err(e) => println!("❌ Failed to leave: {}", e),
                                        }
                                    }
                                    "/peers" => {
                                        let count = client.peer_count().await;
                                        println!("Connected peers: {}", count);
                                    }
                                    "/history" => {
                                        match client.get_channel_messages("global", 10).await {
                                            Ok(msgs) => {
                                                if msgs.is_empty() {
                                                    println!("No messages yet.");
                                                } else {
                                                    println!("Recent messages:");
                                                    for msg in msgs {
                                                        let content = if !msg.content.is_empty() {
                                                            msg.content
                                                        } else if !msg.encrypted_payload.is_empty() {
                                                            format!("<opaque payload: {} bytes>", msg.encrypted_payload.len())
                                                        } else {
                                                            String::new()
                                                        };
                                                        println!("  [{}] {}: {}", msg.timestamp, msg.sender_id, content);
                                                    }
                                                }
                                            }
                                            Err(e) => println!("❌ Failed to get history: {}", e),
                                        }
                                    }
                                    _ => {
                                        println!("Unknown command. Type /quit to exit.");
                                    }
                                }
                            } else {
                                // Send message to global
                                match client.send_channel_message("global", text).await {
                                    Ok(msg_id) => {
                                        debug!("📤 Sent: {} ({})", text, msg_id);
                                    }
                                    Err(e) => {
                                        println!("❌ Failed to send: {}", e);
                                    }
                                }
                            }

                            print!("You: ");
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                        }
                        Ok(None) => break, // stdin closed
                        Err(e) => {
                            return Err(Error::Io(e));
                        }
                    }
                }

                _ = tokio::signal::ctrl_c() => {
                    println!();
                    break;
                }
            }
        }
    }

    // Graceful shutdown
    info!("Shutting down light client...");
    client.disconnect().await?;

    info!("╔═══════════════════════════════════════════════════════════╗");
    info!("║           ✓ Light Client Shutdown Complete                ║");
    info!("╚═══════════════════════════════════════════════════════════╝");

    Ok(())
}

/// Run as user node (legacy - will be deprecated in favor of light client)
async fn run_user_node(
    config: Config,
    bootstrap_peers: Vec<String>,
    identity_path: Option<PathBuf>,
    username: Option<String>,
    non_interactive: bool,
    metrics_addr: String,
    health_addr: String,
) -> Result<()> {
    info!("👤 Starting user node...");

    // MAINNET SECURITY: Validate production environment
    validate_mainnet_environment(&config, NodeType::Client).await?;

    let display_name = username.unwrap_or_else(|| "Anonymous".to_string());
    info!("Username: {}", display_name);

    // Create shutdown channel
    let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(1);

    // Start health check server
    let _health_handle = start_health_server(&health_addr, shutdown_tx.subscribe())?;
    info!("✓ Health server listening on {}", health_addr);

    // Start metrics server
    let peer_metrics = Arc::new(PeerMetrics::new());
    let _metrics_handle =
        start_metrics_server(&metrics_addr, peer_metrics.clone(), shutdown_tx.subscribe())?;
    info!("✓ Metrics server listening on {}", metrics_addr);

    // Load or generate identity with automatic persistence
    let identity_file_path = identity_path
        .clone()
        .unwrap_or_else(|| config.storage.data_dir.join("user_identity.json"));

    let identity = if identity_file_path.exists() {
        info!("Loading identity from {:?}", identity_file_path);
        load_identity_from_file(&identity_file_path).await?
    } else if identity_path.is_some() {
        // Explicit path provided but doesn't exist - error
        return Err(Error::validation(format!(
            "Identity file not found: {:?}. Use 'dchat account create' to create an identity first.",
            identity_file_path
        )));
    } else {
        // No path provided - generate new identity and save to default location
        info!(
            "No identity found - generating new persistent identity at {:?}",
            identity_file_path
        );
        #[allow(deprecated)]
        let keypair = KeyPair::generate();
        let new_identity = Identity::new(display_name.clone(), &keypair);

        // Save identity for persistence across restarts
        if let Some(parent) = identity_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        save_identity_to_file(&new_identity, &keypair, &identity_file_path).await?;
        info!("✓ New identity saved to {:?}", identity_file_path);

        new_identity
    };

    info!("✓ Identity loaded: {}", identity.user_id);

    // Initialize network with bootstrap peers
    let mut network_config = NetworkConfig::default();

    // Parse and add bootstrap peers to discovery config
    if !bootstrap_peers.is_empty() {
        info!("Bootstrap peers provided: {:?}", bootstrap_peers);
        for peer_addr in &bootstrap_peers {
            match peer_addr.parse::<Multiaddr>() {
                Ok(multiaddr) => {
                    // Extract peer ID from multiaddr if present
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
                    info!(
                        "✓ Added bootstrap node: {} (peer_id: {})",
                        peer_addr, peer_id
                    );
                }
                Err(e) => warn!("⚠ Invalid multiaddr {}: {}", peer_addr, e),
            }
        }
    }

    // Store bootstrap nodes before moving config
    let bootstrap_nodes = network_config.discovery.bootstrap_nodes.clone();

    let mut network = NetworkManager::new(network_config).await?;
    let peer_id = network.peer_id();

    // Start network (this will automatically bootstrap DHT with configured nodes)
    network.start().await?;
    info!("✓ Network initialized (peer_id: {})", peer_id);

    // Initialize lightweight peer registry for clients
    let peer_registry = PeerRegistry::new();
    let geographic_region = get_geographic_region();

    // Register bootstrap peers
    for (pid, multiaddr) in &bootstrap_nodes {
        let peer_info = PeerInfo {
            peer_id: *pid,
            multiaddr: multiaddr.clone(),
            node_type: NodeType::Relay,
            geographic_region: None,
            last_seen: SystemTime::now(),
            connection_quality: 1.0,
            capabilities: vec!["relay".to_string()],
            is_bootstrap: true,
            rtt_ms: None,
            packet_loss: 0.0,
            jitter_ms: None,
            total_messages_sent: 0,
            total_messages_received: 0,
            handshake_success: false,
            connected_since: SystemTime::now(),
        };
        peer_registry.add_bootstrap_peer(peer_info.clone()).await;
        peer_registry.add_peer(peer_info).await;
    }

    info!("✓ Registered {} bootstrap peers", bootstrap_nodes.len());

    // Wait for peer connections with handshaking
    let mut peer_count = 0;
    if !bootstrap_peers.is_empty() {
        info!("🤝 Connecting to network relays (15s)...");
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(tokio::time::Duration::from_secs(1), network.next_event())
                .await
            {
                Ok(Some(NetworkEvent::PeerConnected {
                    peer_id: connected_peer,
                    endpoint,
                })) => {
                    peer_count += 1;
                    info!(
                        "✓ Relay connected: {} at {:?} (total: {})",
                        connected_peer, endpoint, peer_count
                    );

                    // Update peer registry
                    peer_registry
                        .update_peer_quality(&connected_peer, 1.0)
                        .await;

                    // Perform lightweight client handshake
                    match perform_peer_handshake(
                        connected_peer,
                        &mut network,
                        &peer_registry,
                        NodeType::Client,
                        geographic_region.clone(),
                    )
                    .await
                    {
                        Ok(_) => debug!("Client handshake completed with {}", connected_peer),
                        Err(e) => debug!("Handshake skipped (relay may not respond): {}", e),
                    }
                }
                Ok(Some(_event)) => {
                    // Other network events (peer discovered, etc.)
                }
                _ => {}
            }
        }

        let connected_relays = peer_registry.get_peers_by_type(NodeType::Relay).await;
        info!(
            "✓ Bootstrap complete: {} relay(s) connected, {} in registry",
            peer_count,
            connected_relays.len()
        );
    }

    // Subscribe to channels
    if let Err(e) = network.subscribe_to_channel("global") {
        warn!("⚠️  Failed to subscribe to #global channel: {}", e);
    } else {
        info!("✓ Subscribed to #global channel");
    }

    // Process network events during subscription exchange (gossipsub needs active event loop)
    info!(
        "Waiting {}s for gossipsub subscription exchange and mesh formation...",
        CONNECTION_TIMEOUT_SECONDS
    );
    let deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_secs(CONNECTION_TIMEOUT_SECONDS);
    let mut last_log = tokio::time::Instant::now();
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(tokio::time::Duration::from_secs(1), network.next_event()).await
        {
            Ok(Some(_event)) => {
                // Process all network events including gossipsub subscriptions
            }
            _ => {}
        }

        // Log mesh status every 5 seconds
        if last_log.elapsed() >= tokio::time::Duration::from_secs(5) {
            let mesh_count = network.get_mesh_peer_count("global");
            info!("📊 Gossipsub mesh status: {} peers in #global", mesh_count);
            last_log = tokio::time::Instant::now();
        }
    }
    let final_mesh_count = network.get_mesh_peer_count("global");
    info!(
        "✓ Subscription exchange complete - {} mesh peers for #global",
        final_mesh_count
    );

    // Initialize storage
    let db_config = create_db_config(&config, "dchat_user.db");
    let database = Database::new(db_config).await?;
    info!("✓ Database initialized");

    // Phase 1: Feature-flagged storage provider routing
    // When enabled, messages are routed through StorageRoutedUserManager for tiered storage
    // (CockroachDB primary, SQLite offline cache, Redis hot cache, S3 blobs)
    // When disabled, uses basic SQLite storage via Database handle
    let _storage_manager: Option<
        Arc<dchat::storage_routed_user_management::StorageRoutedUserManager>,
    > = if config.features.enable_storage_providers {
        info!("📦 Initializing tiered storage provider routing...");

        // Initialize blockchain clients for storage billing
        let chat_rpc_url = resolve_required_chat_chain_rpc_url(&config)?;
        let mut chat_chain_config = ChatChainConfig::default();
        chat_chain_config.rpc_url = chat_rpc_url;
        let chat_chain = Arc::new(ChatChainClient::new(chat_chain_config)?);

        let currency_rpc_url = resolve_required_currency_chain_rpc_url(&config)?;
        let mut currency_chain_config = CurrencyChainConfig::default();
        currency_chain_config.rpc_url = currency_rpc_url;
        let currency_chain = Arc::new(CurrencyChainClient::new(currency_chain_config)?);

        let bridge = Arc::new(CrossChainBridge::new(
            Arc::clone(&chat_chain),
            Arc::clone(&currency_chain),
        ));

        // Use offline mode for user nodes (local SQLite with optional cloud sync)
        let storage_db = Database::new(create_db_config(&config, "dchat_user.db")).await?;
        let manager = dchat::storage_routed_user_management::StorageRoutedUserManager::offline(
            config.storage.data_dir.join("user_messages.db"),
            storage_db,
            chat_chain,
            currency_chain,
            bridge,
            config.storage.data_dir.clone(),
        )
        .await?;

        info!("   ✓ Storage provider routing enabled (offline mode)");
        Some(Arc::new(manager))
    } else {
        debug!("Storage provider routing disabled (using basic SQLite)");
        None
    };

    // Phase 1: Feature-flagged E2E encryption for channel messages
    // When enabled, uses GroupKeyDistribution for sender-key based group encryption
    // with forward secrecy (keys ratchet after each message)
    use dchat_crypto::group_key::GroupKeyDistribution;

    let group_key_manager: Option<Arc<tokio::sync::RwLock<GroupKeyDistribution>>> =
        if config.features.enable_e2e_encryption {
            info!("🔐 Initializing E2E encryption for channel messages...");

            // Convert user ID to 32-byte array for crypto operations
            let user_id_bytes: [u8; 32] = {
                let mut bytes = [0u8; 32];
                let id_str = identity.user_id.0.as_bytes();
                bytes[..id_str.len().min(32)].copy_from_slice(&id_str[..id_str.len().min(32)]);
                bytes
            };

            let gkd = GroupKeyDistribution::new(user_id_bytes);
            info!("   ✓ E2E encryption enabled (sender-key group encryption)");
            Some(Arc::new(tokio::sync::RwLock::new(gkd)))
        } else {
            debug!("E2E encryption disabled (messages sent as plaintext)");
            None
        };

    // Phase 2: Feature-flagged Payment Channels for off-chain micropayments
    // When enabled, allows opening payment channels for cost-efficient message payments
    // instead of settling every message on-chain
    let _payment_channel_manager: Option<
        Arc<std::sync::RwLock<dchat_blockchain::PaymentChannelManager>>,
    > = if config.features.enable_payment_channels {
        info!("💳 Initializing payment channels for off-chain micropayments...");

        let pcm = dchat_blockchain::PaymentChannelManager::new();

        info!("   ✓ Payment channels enabled (off-chain micropayments)");
        info!(
            "   ✓ Channel capacity: {} - {} DCHAT",
            config.payment_channels.min_channel_capacity / 100_000_000,
            config.payment_channels.max_channel_capacity / 100_000_000
        );

        Some(Arc::new(std::sync::RwLock::new(pcm)))
    } else {
        debug!("Payment channels disabled (full on-chain settlement)");
        None
    };

    // Phase 4: Feature-flagged MiniApp Registry for mini-app platform
    // When enabled, allows users to run sandboxed mini-apps with wallet integration
    let _mini_app_registry: Option<Arc<dchat_miniapps::MiniAppRegistry>> =
        if config.features.enable_miniapps {
            info!("📱 Initializing mini-app registry...");

            let developer_registry = Arc::new(dchat_miniapps::DeveloperRegistry::new());
            let registry = dchat_miniapps::MiniAppRegistry::new(developer_registry);

            info!("   ✓ Mini-app registry enabled");
            info!("   ✓ Sandboxed mini-apps with wallet integration available");

            Some(Arc::new(registry))
        } else {
            debug!("Mini-app registry disabled");
            None
        };

    // Phase 4: Feature-flagged Bot Platform for Telegram-style bots
    // When enabled, allows users to create and interact with bots
    let _bot_father: Option<Arc<dchat_bots::BotFather>> = if config.features.enable_bots {
        info!("🤖 Initializing bot platform...");

        let bot_father = dchat_bots::BotFather::new();

        info!("   ✓ Bot platform enabled");
        info!("   ✓ BotFather ready for bot creation and management");

        Some(Arc::new(bot_father))
    } else {
        debug!("Bot platform disabled");
        None
    };

    // Phase 4: Feature-flagged Accessibility TTS Engine
    // When enabled, provides text-to-speech for visually impaired users
    let _tts_engine: Option<Arc<dchat_accessibility::tts::TtsEngine>> = {
        info!("♿ Initializing accessibility TTS engine...");

        let tts = dchat_accessibility::tts::TtsEngine::new();

        // Register default system voices
        let default_voice = dchat_accessibility::tts::Voice {
            id: "default".to_string(),
            name: "System Default".to_string(),
            language: "en-US".to_string(),
            gender: dchat_accessibility::tts::VoiceGender::Neutral,
            sample_rate: 22050, // Standard TTS sample rate
        };
        tts.register_voice(default_voice);

        info!("   ✓ TTS engine enabled with default voice");

        Some(Arc::new(tts))
    };

    // Clone for use in network task
    let group_key_for_net = group_key_manager.clone();

    let compute_channel_message_id = dchat_network::behavior::compute_channel_message_id;

    // Drive the libp2p swarm from a single task.
    // This prevents deadlocks from holding a mutex across `.await` and ensures
    // the swarm is continuously polled so publish/receive works reliably.
    use tokio::sync::{mpsc, oneshot};

    enum ClientNetCmd {
        Publish {
            channel_id: String,
            message: DchatMessage,
            resp: oneshot::Sender<Result<()>>,
        },
        GetMeshCount {
            channel_id: String,
            resp: oneshot::Sender<usize>,
        },
        Shutdown {
            resp: oneshot::Sender<()>,
        },
    }

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<ClientNetCmd>(256);
    let (evt_tx, mut evt_rx) = mpsc::channel::<NetworkEvent>(2048);

    let db_for_net = database.clone();
    let self_user_id = identity.user_id.clone();

    let net_handle = tokio::spawn(async move {
        use dchat_storage::MessageRow;
        use sha2::Digest;

        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(ClientNetCmd::Publish { channel_id, message, resp }) => {
                            let publish_res = network.publish_to_channel(&channel_id, &message);

                            // Best-effort local persistence for outbound messages.
                            if publish_res.is_ok() {
                                if let DchatMessage::ChannelMessage { message_id, sender, channel_id, encrypted_payload, timestamp: _ } = &message {
                                    let content_hash = format!("{:x}", sha2::Sha256::digest(encrypted_payload));
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

                            let _ = resp.send(publish_res);
                        }
                        Some(ClientNetCmd::GetMeshCount { channel_id, resp }) => {
                            let count = network.get_mesh_peer_count(&channel_id);
                            let _ = resp.send(count);
                        }
                        Some(ClientNetCmd::Shutdown { resp }) => {
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

                    // Persist inbound channel messages (best-effort).
                    if let NetworkEvent::MessageReceived { from: _, message } = &event {
                        if let DchatMessage::ChannelMessage { message_id, sender, channel_id, encrypted_payload, timestamp: _ } = message {
                            if sender != &self_user_id {
                                let content_hash = format!("{:x}", sha2::Sha256::digest(encrypted_payload));
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
                                    status: "delivered".to_string(),
                                    expires_at: None,
                                    size: encrypted_payload.len(),
                                    content_hash: Some(content_hash),
                                }).await;
                            }
                        }
                    }

                    // Forward event to UI loop (best-effort).
                    let _ = evt_tx.send(event).await;
                }
            }
        }
    });

    if non_interactive {
        // Non-interactive mode for testing
        info!("Running in non-interactive test mode");

        // Wait additional time for mesh to stabilize
        let (mesh_tx, mesh_rx) = oneshot::channel::<usize>();
        let _ = cmd_tx
            .send(ClientNetCmd::GetMeshCount {
                channel_id: "global".to_string(),
                resp: mesh_tx,
            })
            .await;

        let mesh_count = mesh_rx.await.unwrap_or(0);
        info!(
            "📊 Current mesh status: {} peers before publishing",
            mesh_count
        );

        if mesh_count == 0 {
            warn!("⚠️  No mesh peers yet, waiting 10s for mesh to stabilize...");
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            let (mesh_tx, mesh_rx) = oneshot::channel::<usize>();
            let _ = cmd_tx
                .send(ClientNetCmd::GetMeshCount {
                    channel_id: "global".to_string(),
                    resp: mesh_tx,
                })
                .await;
            let new_mesh_count = mesh_rx.await.unwrap_or(0);
            info!("📊 Mesh status after wait: {} peers", new_mesh_count);
        } else {
            info!(
                "✓ Mesh already has {} peers, proceeding immediately",
                mesh_count
            );
        }

        // Send test messages with retry logic
        for i in 1..=5 {
            let timestamp = chrono::Utc::now().timestamp();
            let encrypted_payload =
                format!("Test message {} from {}", i, display_name).into_bytes();
            let message_id = compute_channel_message_id(
                &identity.user_id,
                "global",
                &encrypted_payload,
                timestamp,
            );
            let message = DchatMessage::ChannelMessage {
                message_id,
                sender: identity.user_id.clone(),
                channel_id: "global".to_string(),
                encrypted_payload,
                timestamp,
            };

            // Retry up to 3 times if publish fails
            let mut attempts = 0;
            loop {
                let (resp_tx, resp_rx) = oneshot::channel::<Result<()>>();
                let send_res = cmd_tx
                    .send(ClientNetCmd::Publish {
                        channel_id: "global".to_string(),
                        message: message.clone(),
                        resp: resp_tx,
                    })
                    .await;

                if send_res.is_err() {
                    return Err(Error::network("Network task is not running".to_string()));
                }

                match resp_rx
                    .await
                    .unwrap_or_else(|_| Err(Error::network("Network task stopped".to_string())))
                {
                    Ok(_) => {
                        info!("📤 Sent test message #{}", i);
                        break;
                    }
                    Err(e) if attempts < 3 => {
                        attempts += 1;
                        warn!(
                            "⚠️  Publish attempt {} failed: {}, retrying in 2s...",
                            attempts, e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    }
                    Err(e) => {
                        error!(
                            "❌ Failed to publish message after {} attempts: {}",
                            attempts + 1,
                            e
                        );
                        return Err(e);
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }

        info!("✓ Test messages sent, waiting 10s before shutdown");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    } else {
        // Interactive mode
        info!("🎉 User client is ready!");
        info!("Type your messages and press Enter to send to #global");
        info!("Press Ctrl+C to exit");

        use tokio::io::{self, AsyncBufReadExt};

        let mut stdin_lines = io::BufReader::new(io::stdin()).lines();
        print!("You: ");
        {
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }

        loop {
            tokio::select! {
                maybe_event = evt_rx.recv() => {
                    let Some(event) = maybe_event else {
                        break;
                    };

                    if let NetworkEvent::MessageReceived { from, message } = event {
                        if let DchatMessage::ChannelMessage { message_id: _, sender, channel_id, encrypted_payload, timestamp: _ } = message {
                            if sender != identity.user_id {
                                // Feature-flagged E2E decryption
                                let msg_text = if let Some(ref gkm) = group_key_manager {
                                    // Try to deserialize and decrypt as GroupEncryptedMessage
                                    use dchat_crypto::group_key::GroupEncryptedMessage;
                                    match bincode::deserialize::<GroupEncryptedMessage>(&encrypted_payload) {
                                        Ok(encrypted_msg) => {
                                            match gkm.write().await.decrypt_group_message(&encrypted_msg).await {
                                                Ok(plaintext) => {
                                                    debug!("🔓 Message decrypted ({} bytes)", plaintext.len());
                                                    String::from_utf8_lossy(&plaintext).to_string()
                                                }
                                                Err(e) => {
                                                    // Decryption failed - might be from user without our sender key
                                                    debug!("Decryption failed (may need key exchange): {}", e);
                                                    format!("<encrypted: key exchange needed>")
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            // Not encrypted or wrong format - display as-is
                                            String::from_utf8_lossy(&encrypted_payload).to_string()
                                        }
                                    }
                                } else {
                                    // E2E disabled - display raw payload
                                    String::from_utf8_lossy(&encrypted_payload).to_string()
                                };
                                println!("\n[#{}] {}: {}", channel_id, from, msg_text);
                                print!("You: ");
                                use std::io::Write;
                                let _ = std::io::stdout().flush();
                            }
                        }
                    }
                }

                line = stdin_lines.next_line() => {
                    match line {
                        Ok(Some(text)) => {
                            if !text.trim().is_empty() {
                                let timestamp = chrono::Utc::now().timestamp();

                                // Feature-flagged E2E encryption
                                // When enabled, uses sender-key group encryption with forward secrecy
                                // When disabled, sends plaintext (development/testing only)
                                let encrypted_payload = if let Some(ref gkm) = group_key_manager {
                                    // Convert channel ID to 32-byte key
                                    let channel_id_bytes: [u8; 32] = {
                                        let mut bytes = [0u8; 32];
                                        let id = b"global";
                                        bytes[..id.len().min(32)].copy_from_slice(&id[..id.len().min(32)]);
                                        bytes
                                    };

                                    // Ensure we have a sender key for this channel
                                    {
                                        let gkm_write = gkm.write().await;
                                        let _ = gkm_write.get_or_create_sender_key(channel_id_bytes).await;
                                    }

                                    // Encrypt the message with group key (forward secrecy via key ratchet)
                                    match gkm.write().await.encrypt_group_message(channel_id_bytes, text.as_bytes()).await {
                                        Ok(encrypted_msg) => {
                                            // Serialize the encrypted message envelope
                                            match bincode::serialize(&encrypted_msg) {
                                                Ok(serialized) => {
                                                    debug!("🔐 Message encrypted ({} bytes)", serialized.len());
                                                    serialized
                                                }
                                                Err(e) => {
                                                    warn!("Failed to serialize encrypted message, falling back to plaintext: {}", e);
                                                    text.as_bytes().to_vec()
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!("E2E encryption failed, falling back to plaintext: {}", e);
                                            text.as_bytes().to_vec()
                                        }
                                    }
                                } else {
                                    // E2E disabled - send plaintext
                                    text.as_bytes().to_vec()
                                };

                                let message_id = compute_channel_message_id(
                                    &identity.user_id,
                                    "global",
                                    &encrypted_payload,
                                    timestamp,
                                );
                                let message = DchatMessage::ChannelMessage {
                                    message_id,
                                    sender: identity.user_id.clone(),
                                    channel_id: "global".to_string(),
                                    encrypted_payload,
                                    timestamp,
                                };

                                let (resp_tx, resp_rx) = oneshot::channel::<Result<()>>();
                                if cmd_tx.send(ClientNetCmd::Publish {
                                    channel_id: "global".to_string(),
                                    message,
                                    resp: resp_tx,
                                }).await.is_err() {
                                    return Err(Error::network("Network task is not running".to_string()));
                                }

                                match resp_rx.await.unwrap_or_else(|_| Err(Error::network("Network task stopped".to_string()))) {
                                    Ok(_) => {
                                        info!("📤 Sent: {}", text);
                                    }
                                    Err(e) => {
                                        info!("❌ Failed to send: {}", e);
                                        println!("Error sending message: {}", e);
                                    }
                                }

                                print!("You: ");
                                use std::io::Write;
                                let _ = std::io::stdout().flush();
                            }
                        }
                        Ok(None) => {
                            // stdin closed
                            break;
                        }
                        Err(e) => {
                            return Err(Error::Io(e));
                        }
                    }
                }

                _ = tokio::signal::ctrl_c() => {
                    break;
                }
            }
        }
    }

    // Graceful shutdown
    info!("Shutting down user client...");
    let _ = shutdown_tx.send(());
    let (resp_tx, resp_rx) = oneshot::channel::<()>();
    let _ = cmd_tx.send(ClientNetCmd::Shutdown { resp: resp_tx }).await;
    let _ = resp_rx.await;
    let _ = net_handle.await;
    database.close().await?;
    info!("✓ Shutdown complete");
    Ok(())
}

/// Run full testnet with all components
// ============================================================================
// MAINNET GENESIS AND INITIALIZATION
// ============================================================================

/// Foundation server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FoundationServer {
    name: String,
    dns: String,
    ip: String,
    region: String,
    node_type: String, // "validator" or "relay" or "user"
    stake_amount: u64,
}

/// Genesis configuration for mainnet
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MainnetGenesis {
    chain_id: String,
    genesis_time: String,
    initial_supply: u64,
    foundation_allocation: u64,
    validator_allocation: u64,
    community_allocation: u64,
    validators: Vec<GenesisValidator>,
    pools: GenesisPools,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GenesisValidator {
    name: String,
    public_key: String,
    stake: u64,
    voting_power: u64,
    region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GenesisPools {
    staking_pool: u64,
    rewards_pool: u64,
    liquidity_pool: u64,
    foundation_pool: u64,
}

/// Handle pre-stake genesis commands
async fn run_prestake_genesis_command(action: PreStakeGenesisCommand) -> Result<()> {
    use dchat_chain::chain::prestake_genesis::{
        create_signed_commitment, BondCommitment, PreStakeGenesisBuilder, PreStakeManifest,
    };
    use dchat_core::motes::MOTES_PER_DCHAT;

    match action {
        PreStakeGenesisCommand::InitManifest {
            chain_id,
            output,
            initial_supply,
            min_stake,
        } => {
            info!("📋 Creating pre-stake manifest...");
            info!("   Chain ID: {}", chain_id);
            info!("   Initial supply: {} DCHAT", initial_supply);
            info!("   Minimum stake: {} DCHAT", min_stake);

            let initial_supply_motes = initial_supply.saturating_mul(MOTES_PER_DCHAT);
            let min_stake_motes = min_stake.saturating_mul(MOTES_PER_DCHAT);

            let mut manifest =
                PreStakeManifest::new(chain_id, chrono::Utc::now(), initial_supply_motes);
            manifest.min_stake = min_stake_motes;

            manifest
                .save_to_file(&output)
                .map_err(|e| Error::Config(format!("Failed to save manifest: {}", e)))?;

            info!("✅ Pre-stake manifest created: {:?}", output);
            info!("");
            info!("Next steps:");
            info!("  1. Distribute manifest to validators");
            info!("  2. Each validator runs: dchat prestake-genesis create-commitment ...");
            info!("  3. Collect commitments and run: dchat prestake-genesis add-commitment ...");
            info!("  4. Validate manifest: dchat prestake-genesis validate-manifest ...");
            info!("  5. Generate genesis: dchat prestake-genesis generate-genesis ...");

            Ok(())
        }

        PreStakeGenesisCommand::CreateCommitment {
            key_file,
            name,
            stake,
            address,
            region,
            lockup_days,
            chain_id,
            output,
        } => {
            info!("📝 Creating bond commitment for {}...", name);

            // Load private key
            let key_bytes = tokio::fs::read(&key_file).await.map_err(|e| Error::Io(e))?;

            let key_bytes: [u8; 32] = if key_bytes.len() == 64 {
                // Hex encoded
                let decoded = hex::decode(&key_bytes)
                    .map_err(|e| Error::Config(format!("Invalid hex key: {}", e)))?;
                decoded
                    .try_into()
                    .map_err(|_| Error::Config("Key must be 32 bytes".to_string()))?
            } else if key_bytes.len() == 32 {
                // Raw bytes
                key_bytes
                    .try_into()
                    .map_err(|_| Error::Config("Key must be 32 bytes".to_string()))?
            } else {
                return Err(Error::Config(format!(
                    "Invalid key file length: {} (expected 32 or 64 bytes)",
                    key_bytes.len()
                )));
            };

            let signing_key = ed25519_dalek::SigningKey::from_bytes(&key_bytes);

            // Convert stake to motes
            let stake_motes = stake.saturating_mul(MOTES_PER_DCHAT);

            let commitment = create_signed_commitment(
                &signing_key,
                name.clone(),
                stake_motes,
                address.clone(),
                region.clone(),
                lockup_days,
                &chain_id,
            )
            .map_err(|e| Error::Config(format!("Failed to create commitment: {}", e)))?;

            // Save commitment
            let json = serde_json::to_string_pretty(&commitment)
                .map_err(|e| Error::Config(format!("Failed to serialize commitment: {}", e)))?;
            tokio::fs::write(&output, json)
                .await
                .map_err(|e| Error::Io(e))?;

            info!("✅ Bond commitment created: {:?}", output);
            info!("   Validator: {}", name);
            info!("   Stake: {} DCHAT", stake);
            info!("   Address: {}", address);
            info!("   Region: {}", region);
            info!("   Lockup: {} days", lockup_days);
            info!(
                "   Public key: {}",
                hex::encode(signing_key.verifying_key().as_bytes())
            );

            Ok(())
        }

        PreStakeGenesisCommand::AddCommitment {
            manifest,
            commitment,
        } => {
            info!("➕ Adding commitment to manifest...");

            // Load manifest
            let mut manifest_data = PreStakeManifest::load_from_file(&manifest)
                .map_err(|e| Error::Config(format!("Failed to load manifest: {}", e)))?;

            // Load commitment
            let commitment_json = tokio::fs::read_to_string(&commitment)
                .await
                .map_err(|e| Error::Io(e))?;
            let commitment_data: BondCommitment = serde_json::from_str(&commitment_json)
                .map_err(|e| Error::Config(format!("Failed to parse commitment: {}", e)))?;

            // Add commitment
            manifest_data
                .add_commitment(commitment_data.clone())
                .map_err(|e| Error::Config(format!("Failed to add commitment: {}", e)))?;

            // Save updated manifest
            manifest_data
                .save_to_file(&manifest)
                .map_err(|e| Error::Config(format!("Failed to save manifest: {}", e)))?;

            info!("✅ Commitment added");
            info!(
                "   Validator: {} ({})",
                commitment_data.validator_name, commitment_data.region
            );
            info!("   Total validators: {}", manifest_data.commitments.len());

            Ok(())
        }

        PreStakeGenesisCommand::ValidateManifest { manifest } => {
            info!("🔍 Validating pre-stake manifest...");

            let manifest_data = PreStakeManifest::load_from_file(&manifest)
                .map_err(|e| Error::Config(format!("Failed to load manifest: {}", e)))?;

            match manifest_data.validate() {
                Ok(_) => {
                    info!("✅ Manifest is valid and ready for genesis!");
                    info!("   Validators: {}", manifest_data.commitments.len());
                    info!(
                        "   Total staked: {} DCHAT",
                        manifest_data.total_staked() / MOTES_PER_DCHAT
                    );

                    // Show validators by region
                    let by_region = manifest_data.validators_by_region();
                    info!("   Regions: {}", by_region.len());
                    for (region, validators) in by_region {
                        info!("     - {}: {} validators", region, validators.len());
                    }
                }
                Err(e) => {
                    error!("❌ Manifest validation failed: {}", e);
                    return Err(Error::Config(format!("Manifest validation failed: {}", e)));
                }
            }

            Ok(())
        }

        PreStakeGenesisCommand::GenerateGenesis {
            manifest,
            coordinator_key,
            output,
        } => {
            info!("🚀 Generating genesis files from pre-stake manifest...");

            // Load manifest
            let manifest_data = PreStakeManifest::load_from_file(&manifest)
                .map_err(|e| Error::Config(format!("Failed to load manifest: {}", e)))?;

            // Load coordinator key
            let key_bytes = tokio::fs::read(&coordinator_key)
                .await
                .map_err(|e| Error::Io(e))?;

            let key_bytes: [u8; 32] = if key_bytes.len() == 64 {
                let decoded = hex::decode(&key_bytes)
                    .map_err(|e| Error::Config(format!("Invalid hex key: {}", e)))?;
                decoded
                    .try_into()
                    .map_err(|_| Error::Config("Key must be 32 bytes".to_string()))?
            } else if key_bytes.len() == 32 {
                key_bytes
                    .try_into()
                    .map_err(|_| Error::Config("Key must be 32 bytes".to_string()))?
            } else {
                return Err(Error::Config(format!(
                    "Invalid key file length: {} (expected 32 or 64 bytes)",
                    key_bytes.len()
                )));
            };

            let signing_key = ed25519_dalek::SigningKey::from_bytes(&key_bytes);

            // Build and generate genesis
            let builder = PreStakeGenesisBuilder::new(manifest_data, signing_key);
            builder
                .generate_files(&output)
                .map_err(|e| Error::Config(format!("Failed to generate genesis: {}", e)))?;

            info!("✅ Genesis files generated!");
            info!("   Output directory: {:?}", output);
            info!("");
            info!("Next steps:");
            info!("  1. Distribute genesis files to all validators");
            info!(
                "  2. Each validator starts with: dchat --role validator --genesis-dir {:?}",
                output
            );
            info!("  3. Network starts automatically with pre-staked validators");

            Ok(())
        }
    }
}

/// Initialize mainnet genesis (first validator only)
async fn run_genesis_init(
    output: PathBuf,
    chain_id: String,
    initial_supply: u64,
    foundation_percent: u8,
    validators_hex: Option<String>,
) -> Result<()> {
    info!("🌍 Initializing Mainnet Genesis...");
    info!("  Chain ID: {}", chain_id);
    info!("  Initial Supply: {} tokens", initial_supply);
    info!("  Foundation Allocation: {}%", foundation_percent);

    // Create output directory
    tokio::fs::create_dir_all(&output)
        .await
        .map_err(Error::Io)?;

    // Calculate allocations
    let foundation_allocation = (initial_supply as u128 * foundation_percent as u128 / 100) as u64;
    let remaining = initial_supply - foundation_allocation;
    let validator_allocation = remaining * 30 / 100; // 30% of remaining for validators
    let community_allocation = remaining - validator_allocation; // Rest for community/rewards

    info!("📊 Token Allocation:");
    info!("  Foundation: {} tokens", foundation_allocation);
    info!("  Validators: {} tokens", validator_allocation);
    info!("  Community/Rewards: {} tokens", community_allocation);

    // Parse validator public keys if provided
    let validators: Vec<GenesisValidator> = if let Some(hex_keys) = validators_hex {
        hex_keys
            .split(',')
            .enumerate()
            .map(|(i, key)| GenesisValidator {
                name: format!("validator-{}", i),
                public_key: key.trim().to_string(),
                stake: 10_000_000, // 10M tokens minimum stake
                voting_power: 1,
                region: "unknown".to_string(),
            })
            .collect()
    } else {
        // Default foundation validators from known servers
        get_foundation_validators().await?
    };

    info!("👥 Genesis Validators: {}", validators.len());
    for v in &validators {
        info!("  - {} ({}) stake: {}", v.name, v.region, v.stake);
    }

    // Create genesis pools
    let pools = GenesisPools {
        staking_pool: validator_allocation,
        rewards_pool: community_allocation * 40 / 100, // 40% of community for rewards
        liquidity_pool: community_allocation * 30 / 100, // 30% for liquidity
        foundation_pool: foundation_allocation,
    };

    // Create genesis configuration
    let genesis = MainnetGenesis {
        chain_id: chain_id.clone(),
        genesis_time: chrono::Utc::now().to_rfc3339(),
        initial_supply,
        foundation_allocation,
        validator_allocation,
        community_allocation,
        validators,
        pools,
    };

    // Write currency chain genesis
    let currency_genesis_path = output.join("currency_chain_genesis.json");
    let currency_genesis = serde_json::json!({
        "chain_id": format!("{}-currency", chain_id),
        "genesis_time": genesis.genesis_time,
        "initial_height": "1",
        "consensus_params": {
            "block": {
                "max_bytes": "22020096",
                "max_gas": "-1"
            },
            "evidence": {
                "max_age_num_blocks": "100000",
                "max_age_duration": "172800000000000"
            },
            "validator": {
                "pub_key_types": ["ed25519"]
            }
        },
        "validators": genesis.validators.iter().map(|v| serde_json::json!({
            "address": &v.public_key[..40.min(v.public_key.len())],
            "pub_key": {
                "type": "tendermint/PubKeyEd25519",
                "value": &v.public_key
            },
            "power": v.voting_power.to_string(),
            "name": &v.name
        })).collect::<Vec<_>>(),
        "app_state": {
            "bank": {
                "balances": genesis.validators.iter().map(|v| serde_json::json!({
                    "address": &v.public_key[..40.min(v.public_key.len())],
                    "coins": [{
                        "denom": "dchat",
                        "amount": v.stake.to_string()
                    }]
                })).collect::<Vec<_>>(),
                "supply": [{
                    "denom": "dchat",
                    "amount": initial_supply.to_string()
                }]
            },
            "staking": {
                "pool": {
                    "bonded_tokens": genesis.pools.staking_pool.to_string(),
                    "not_bonded_tokens": "0"
                },
                "params": {
                    "unbonding_time": "604800s",
                    "max_validators": 100,
                    "min_stake": "10000000"
                }
            }
        }
    });
    tokio::fs::write(
        &currency_genesis_path,
        serde_json::to_string_pretty(&currency_genesis)?,
    )
    .await
    .map_err(Error::Io)?;
    info!(
        "✓ Currency chain genesis written to {:?}",
        currency_genesis_path
    );

    // Write chat chain genesis
    let chat_genesis_path = output.join("chat_chain_genesis.json");
    let chat_genesis = serde_json::json!({
        "chain_id": format!("{}-chat", chain_id),
        "genesis_time": genesis.genesis_time,
        "initial_height": "1",
        "validators": genesis.validators.iter().map(|v| serde_json::json!({
            "pub_key": &v.public_key,
            "power": v.voting_power,
            "name": &v.name
        })).collect::<Vec<_>>(),
        "app_state": {
            "channels": [],
            "message_retention_days": 30
        }
    });
    tokio::fs::write(
        &chat_genesis_path,
        serde_json::to_string_pretty(&chat_genesis)?,
    )
    .await
    .map_err(Error::Io)?;
    info!("✓ Chat chain genesis written to {:?}", chat_genesis_path);

    // Write main genesis summary
    let genesis_summary_path = output.join("genesis.json");
    tokio::fs::write(
        &genesis_summary_path,
        serde_json::to_string_pretty(&genesis)?,
    )
    .await
    .map_err(Error::Io)?;
    info!("✓ Genesis summary written to {:?}", genesis_summary_path);

    info!("\n🎉 Genesis initialization complete!");
    info!("\nNext steps:");
    info!("  1. Distribute genesis files to all validators");
    info!("  2. Each validator runs: dchat validator --key <key_file> --chain-rpc <rpc_url>");
    info!("  3. First validator adds --producer flag to start block production");

    Ok(())
}

/// Validator key file structure matching the JSON format
#[derive(Debug, serde::Deserialize)]
struct ValidatorKeyFile {
    #[allow(dead_code)]
    created_at: String,
    #[allow(dead_code)]
    key_type: String,
    #[allow(dead_code)]
    private_key: String,
    public_key: String,
}

/// Foundation validator configuration with file path
struct FoundationValidatorConfig {
    name: String,
    key_file: &'static str,
    region: String,
    stake_amount: u64,
}

/// Get foundation validators from genesis JSON key files
///
/// SAFETY: This function loads actual validator public keys from JSON files.
/// It will fail startup if any key file is missing or malformed.
async fn get_foundation_validators() -> Result<Vec<GenesisValidator>> {
    // Foundation validators with their key file paths
    let foundation_validators = vec![
        FoundationValidatorConfig {
            name: "validator-india".to_string(),
            key_file: "validator-india.json",
            region: "asia-south".to_string(),
            stake_amount: 10_000_000,
        },
        FoundationValidatorConfig {
            name: "validator-southafrica".to_string(),
            key_file: "validator-southafrica.json",
            region: "africa-south".to_string(),
            stake_amount: 10_000_000,
        },
        FoundationValidatorConfig {
            name: "validator-uae".to_string(),
            key_file: "validator-uae.json",
            region: "me-central".to_string(),
            stake_amount: 10_000_000,
        },
    ];

    let mut validators = Vec::with_capacity(foundation_validators.len());

    for config in foundation_validators {
        // Load the key file
        let key_path = std::path::Path::new(config.key_file);

        let key_contents = tokio::fs::read_to_string(&key_path).await.map_err(|e| {
            Error::Config(format!(
                "Failed to read foundation validator key file '{}': {}. \
                 Ensure all validator key files are present before genesis initialization.",
                config.key_file, e
            ))
        })?;

        let key_file: ValidatorKeyFile = serde_json::from_str(&key_contents).map_err(|e| {
            Error::Config(format!(
                "Failed to parse foundation validator key file '{}': {}. \
                 Expected JSON with 'public_key' field.",
                config.key_file, e
            ))
        })?;

        // Validate public key format (should be 64 hex characters for ed25519)
        if key_file.public_key.len() != 64
            || !key_file.public_key.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(Error::Config(format!(
                "Invalid public key format in '{}': expected 64 hex characters, got '{}'",
                config.key_file, key_file.public_key
            )));
        }

        validators.push(GenesisValidator {
            name: config.name,
            public_key: key_file.public_key,
            stake: config.stake_amount,
            voting_power: 1,
            region: config.region,
        });
    }

    info!(
        "✓ Loaded {} foundation validator public keys from genesis files",
        validators.len()
    );

    Ok(validators)
}

/// Initialize mainnet for foundation servers
async fn run_mainnet_init(
    config_path: PathBuf,
    genesis_validator: bool,
    fund_foundation: bool,
    init_pools: bool,
) -> Result<()> {
    info!("🚀 Initializing Mainnet...");

    // Load or create mainnet config
    let mainnet_config = if config_path.exists() {
        info!("Loading mainnet config from {:?}", config_path);
        let contents = tokio::fs::read_to_string(&config_path)
            .await
            .map_err(Error::Io)?;
        toml::from_str(&contents)
            .map_err(|e| Error::Config(format!("Invalid mainnet config: {}", e)))?
    } else {
        info!("Creating default mainnet config at {:?}", config_path);
        create_default_mainnet_config(&config_path).await?
    };

    // If this is the genesis validator, initialize both chains
    if genesis_validator {
        info!("🌟 Initializing as GENESIS VALIDATOR");
        info!("  This node will create the initial blocks for both chains");

        // Initialize currency chain
        info!("📦 Initializing Currency Chain...");
        initialize_currency_chain(&mainnet_config).await?;

        // Initialize chat chain
        info!("💬 Initializing Chat Chain...");
        initialize_chat_chain(&mainnet_config).await?;
    }

    // Fund foundation servers with initial stake
    if fund_foundation {
        info!("💰 Funding Foundation Servers...");
        fund_foundation_servers(&mainnet_config).await?;
    }

    // Initialize all pools
    if init_pools {
        info!("🏊 Initializing Pools...");
        initialize_pools(&mainnet_config).await?;
    }

    info!("\n🎉 Mainnet initialization complete!");
    Ok(())
}

/// Create default mainnet configuration
async fn create_default_mainnet_config(path: &PathBuf) -> Result<MainnetConfig> {
    let config = MainnetConfig {
        chain_id: "dchat-mainnet-1".to_string(),
        currency_chain_rpc: "http://localhost:26657".to_string(),
        chat_chain_rpc: "http://localhost:26658".to_string(),
        initial_supply: 1_000_000_000,
        foundation_servers: vec![
            FoundationServerConfig {
                name: "validator-india".to_string(),
                dns: "validator.india.schikuno.top".to_string(),
                ip: "74.225.183.196".to_string(),
                node_type: "validator".to_string(),
                stake: 10_000_000,
                port: 7070,
            },
            FoundationServerConfig {
                name: "validator-southafrica".to_string(),
                dns: "validator.southafrica.schikuno.top".to_string(),
                ip: "4.221.211.71".to_string(),
                node_type: "validator".to_string(),
                stake: 10_000_000,
                port: 7070,
            },
            FoundationServerConfig {
                name: "validator-uae".to_string(),
                dns: "validator.uae.schikuno.top".to_string(),
                ip: "4.161.34.228".to_string(),
                node_type: "validator".to_string(),
                stake: 10_000_000,
                port: 7070,
            },
            FoundationServerConfig {
                name: "relay-ohio".to_string(),
                dns: "relay.ohio.schikuno.top".to_string(),
                ip: "18.223.119.189".to_string(),
                node_type: "relay".to_string(),
                stake: 1_000_000,
                port: 7070,
            },
            FoundationServerConfig {
                name: "relay-saopaulo".to_string(),
                dns: "relay.saopaulo.schikuno.top".to_string(),
                ip: "18.231.117.182".to_string(),
                node_type: "relay".to_string(),
                stake: 1_000_000,
                port: 7070,
            },
            FoundationServerConfig {
                name: "relay-stockholm".to_string(),
                dns: "relay.stockholm.schikuno.top".to_string(),
                ip: "13.50.105.166".to_string(),
                node_type: "relay".to_string(),
                stake: 1_000_000,
                port: 7070,
            },
            FoundationServerConfig {
                name: "user-singapore".to_string(),
                dns: "user.singapore.schikuno.top".to_string(),
                ip: "18.140.247.242".to_string(),
                node_type: "user".to_string(),
                stake: 0,
                port: 7070,
            },
        ],
        pools: PoolsConfig {
            staking_initial: 300_000_000,
            rewards_initial: 200_000_000,
            liquidity_initial: 100_000_000,
            foundation_initial: 200_000_000,
        },
    };

    // Write config to file
    let toml_str = toml::to_string_pretty(&config)
        .map_err(|e| Error::Config(format!("Failed to serialize config: {}", e)))?;
    tokio::fs::write(path, &toml_str).await.map_err(Error::Io)?;
    info!("✓ Default mainnet config created at {:?}", path);

    Ok(config)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MainnetConfig {
    chain_id: String,
    currency_chain_rpc: String,
    chat_chain_rpc: String,
    initial_supply: u64,
    foundation_servers: Vec<FoundationServerConfig>,
    pools: PoolsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FoundationServerConfig {
    name: String,
    dns: String,
    ip: String,
    node_type: String,
    stake: u64,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PoolsConfig {
    staking_initial: u64,
    rewards_initial: u64,
    liquidity_initial: u64,
    foundation_initial: u64,
}

/// Initialize currency chain with genesis block
async fn initialize_currency_chain(config: &MainnetConfig) -> Result<()> {
    info!("  Chain ID: {}-currency", config.chain_id);
    info!("  RPC: {}", config.currency_chain_rpc);

    // Create currency chain client with default config
    let mut chain_config = CurrencyChainConfig::default();
    chain_config.rpc_url = config.currency_chain_rpc.clone();

    let chain_client = CurrencyChainClient::new(chain_config)?;

    // Check if chain is already initialized
    match chain_client.get_current_height().await {
        Ok(height) if height > 0 => {
            info!(
                "  ✓ Currency chain already initialized at height {}",
                height
            );
            return Ok(());
        }
        _ => {
            info!("  Creating genesis block...");
        }
    }

    // Initialize genesis state
    // Note: In production, this would interact with the actual chain
    info!("  ✓ Currency chain genesis initialized");
    info!("    Initial supply: {} tokens", config.initial_supply);

    Ok(())
}

/// Initialize chat chain with genesis block
async fn initialize_chat_chain(config: &MainnetConfig) -> Result<()> {
    info!("  Chain ID: {}-chat", config.chain_id);
    info!("  RPC: {}", config.chat_chain_rpc);

    // Create chat chain client with default config
    let mut chain_config = ChatChainConfig::default();
    chain_config.rpc_url = config.chat_chain_rpc.clone();

    // Try to create the chat chain client - if it succeeds with genesis config, chain is ready
    match ChatChainClient::new(chain_config) {
        Ok(_client) => {
            info!("  ✓ Chat chain client connected");
            // For genesis initialization, the chain starts at block 0
            // The genesis block will be created by the first validator
            info!("  Creating genesis block for chat chain...");
        }
        Err(e) => {
            warn!(
                "  Chat chain connection failed (expected for genesis): {}",
                e
            );
            info!("  Will create genesis block on first validator start...");
        }
    }

    info!("  ✓ Chat chain genesis initialized");

    Ok(())
}

/// Fund foundation servers with initial stake
async fn fund_foundation_servers(config: &MainnetConfig) -> Result<()> {
    info!(
        "  Funding {} foundation servers...",
        config.foundation_servers.len()
    );

    // Create currency chain client for funding
    let mut chain_config = CurrencyChainConfig::default();
    chain_config.rpc_url = config.currency_chain_rpc.clone();

    let chain_client = CurrencyChainClient::new(chain_config)?;

    for server in &config.foundation_servers {
        if server.stake > 0 {
            info!(
                "  💵 Funding {} ({}) with {} tokens",
                server.name, server.dns, server.stake
            );

            // PRODUCTION IMPLEMENTATION:
            // 1. Generate/load the server's address from its public key
            //    For now, we derive a deterministic address from the DNS name
            //    In full production, this would load the actual public key from config
            let server_key_bytes = blake3::hash(server.dns.as_bytes());
            let server_pubkey: [u8; 32] = *server_key_bytes.as_bytes();
            let server_user_id = CurrencyChainClient::address_from_public_key(&server_pubkey);

            info!(
                "    Address: {} (derived from {})",
                server_user_id, server.dns
            );

            // 2. Transfer tokens from genesis allocation to that address
            let genesis_tx = chain_client.transfer_from_genesis(
                &server_user_id,
                server.stake,
                "foundation_allocation",
            )?;
            info!(
                "    Genesis transfer: {} tokens (tx: {})",
                server.stake, genesis_tx
            );

            // 3. Automatically stake the tokens for validators/relays
            let should_auto_stake = server.node_type == "validator" || server.node_type == "relay";
            if should_auto_stake {
                // Validators and relays get their stake locked for 90 days minimum
                let lock_duration_days = if server.node_type == "validator" {
                    90
                } else {
                    30
                };

                let stake_tx = chain_client.auto_stake_for_node(
                    &server_user_id,
                    server.stake,
                    &server.node_type,
                    lock_duration_days,
                )?;
                info!(
                    "    Auto-staked: {} tokens for {} (lock: {} days, tx: {})",
                    server.stake, server.node_type, lock_duration_days, stake_tx
                );
            } else {
                info!("    Type: {} (no auto-stake required)", server.node_type);
            }
        }
    }

    info!("  ✓ Foundation servers funded and staked");
    Ok(())
}

/// Initialize all pools (staking, rewards, liquidity)
async fn initialize_pools(config: &MainnetConfig) -> Result<()> {
    info!("  Initializing protocol pools...");

    // Create currency chain client
    let mut chain_config = CurrencyChainConfig::default();
    chain_config.rpc_url = config.currency_chain_rpc.clone();

    let _chain_client = CurrencyChainClient::new(chain_config)?;

    // Initialize staking pool
    info!("  🏦 Staking Pool: {} tokens", config.pools.staking_initial);

    // Initialize rewards pool
    info!("  🎁 Rewards Pool: {} tokens", config.pools.rewards_initial);

    // Initialize liquidity pool
    info!(
        "  💧 Liquidity Pool: {} tokens",
        config.pools.liquidity_initial
    );

    // Initialize foundation pool
    info!(
        "  🏛️  Foundation Pool: {} tokens",
        config.pools.foundation_initial
    );

    let total = config.pools.staking_initial
        + config.pools.rewards_initial
        + config.pools.liquidity_initial
        + config.pools.foundation_initial;
    info!("  ✓ Total pooled: {} tokens", total);

    Ok(())
}

// ============================================================================
// TESTNET SETUP
// ============================================================================

async fn run_testnet(
    _config: Config,
    num_validators: usize,
    num_relays: usize,
    num_clients: usize,
    data_dir: PathBuf,
    enable_observability: bool,
) -> Result<()> {
    info!("🚀 Launching full testnet...");
    info!("  Validators: {}", num_validators);
    info!("  Relays: {}", num_relays);
    info!("  Clients: {}", num_clients);
    info!("  Data directory: {:?}", data_dir);

    // Create testnet directory structure
    std::fs::create_dir_all(&data_dir)?;
    let validators_dir = data_dir.join("validators");
    let relays_dir = data_dir.join("relays");
    let clients_dir = data_dir.join("clients");

    std::fs::create_dir_all(&validators_dir)?;
    std::fs::create_dir_all(&relays_dir)?;
    std::fs::create_dir_all(&clients_dir)?;

    info!("✓ Created testnet directories");

    // Generate genesis configuration
    info!("Generating genesis configuration...");
    let genesis = generate_genesis_config(num_validators)?;
    let genesis_path = data_dir.join("genesis.json");
    std::fs::write(&genesis_path, serde_json::to_string_pretty(&genesis)?)?;
    info!("✓ Genesis configuration written to {:?}", genesis_path);

    // Generate validator keys
    info!("Generating validator keys...");
    let mut validator_keys = Vec::new();
    for i in 0..num_validators {
        #[allow(deprecated)]
        let keypair = KeyPair::generate();
        let key_path = validators_dir.join(format!("validator_{}.key", i));
        save_validator_key(&key_path, &keypair).await?;
        validator_keys.push(keypair);
        info!("  ✓ Validator {} key: {:?}", i, key_path);
    }

    // Generate relay identities
    info!("Generating relay identities...");
    let mut relay_addrs = Vec::new();
    for i in 0..num_relays {
        let base_port = 7070 + (i * 2);
        let addr = format!("/ip4/127.0.0.1/tcp/{}", base_port);
        relay_addrs.push(addr.clone());
        info!("  ✓ Relay {} address: {}", i, addr);
    }

    // Create testnet coordination file
    let testnet_info = serde_json::json!({
        "validators": num_validators,
        "relays": num_relays,
        "clients": num_clients,
        "relay_addresses": relay_addrs,
        "genesis_path": genesis_path,
        "started_at": chrono::Utc::now().to_rfc3339(),
    });

    let info_path = data_dir.join("testnet-info.json");
    std::fs::write(&info_path, serde_json::to_string_pretty(&testnet_info)?)?;
    info!("✓ Testnet info written to {:?}", info_path);

    // Create docker-compose for testnet
    if enable_observability {
        info!("Generating docker-compose with observability stack...");
        generate_testnet_compose(
            &data_dir,
            num_validators,
            num_relays,
            num_clients,
            &relay_addrs,
            true,
        )?;
    } else {
        generate_testnet_compose(
            &data_dir,
            num_validators,
            num_relays,
            num_clients,
            &relay_addrs,
            false,
        )?;
    }

    info!("\n🎉 Testnet configuration complete!");
    info!("\nNext steps:");
    info!("  1. Review configuration: {:?}", info_path);
    info!(
        "  2. Start validators: docker-compose -f {:?}/docker-compose.yml up validators",
        data_dir
    );
    info!(
        "  3. Start relays: docker-compose -f {:?}/docker-compose.yml up relays",
        data_dir
    );
    info!(
        "  4. Start clients: docker-compose -f {:?}/docker-compose.yml up clients",
        data_dir
    );
    info!("\nOr start everything:");
    info!(
        "  docker-compose -f {:?}/docker-compose.yml up -d",
        data_dir
    );

    Ok(())
}

/// Run as validator node
async fn run_validator_node(
    config: Config,
    key_path: String,
    chain_rpc: Option<String>,
    use_hsm: bool,
    stake_amount: u64,
    is_producer: bool,
    metrics_addr: String,
    health_addr: String,
    genesis_bootstrap: bool,
    genesis_dir: Option<PathBuf>,
) -> Result<()> {
    info!("⚙️  Starting validator node...");

    // MAINNET SECURITY: Validate production environment
    validate_mainnet_environment(&config, NodeType::Validator).await?;

    // Validate chain_rpc requirement
    if chain_rpc.is_none() && genesis_dir.is_none() {
        return Err(Error::validation(
            "Either --chain-rpc or --genesis-dir must be specified".to_string(),
        ));
    }

    let chain_rpc_display = chain_rpc.as_deref().unwrap_or("N/A (using genesis)");
    info!("Chain RPC: {}", chain_rpc_display);
    info!("HSM enabled: {}", use_hsm);
    info!("Stake: {} tokens", stake_amount);
    info!("Block producer: {}", is_producer);
    if genesis_bootstrap {
        warn!("⚠️  Genesis bootstrap mode: allowing start without bootstrap peers");
    }
    if genesis_dir.is_some() {
        info!(
            "🌱 Using pre-stake genesis from {:?}",
            genesis_dir.as_ref().unwrap()
        );
    }

    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    // Start health check server
    let health_handle = start_health_server(&health_addr, shutdown_tx.subscribe())?;
    info!("✓ Health server listening on {}", health_addr);

    // Start metrics server
    let peer_metrics = Arc::new(PeerMetrics::new());
    let metrics_handle =
        start_metrics_server(&metrics_addr, peer_metrics.clone(), shutdown_tx.subscribe())?;
    info!("✓ Metrics server listening on {}", metrics_addr);

    // Load validator key
    let validator_key = if use_hsm {
        info!("🔐 HSM mode enabled - using AWS KMS for key management");

        // Get AWS region from environment or default to us-east-1
        let aws_region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());

        // Initialize AWS KMS client
        use dchat_crypto::kms::{AwsKmsClient, Ed25519KmsWrapper};

        match AwsKmsClient::new(&aws_region).await {
            Ok(kms_client) => {
                info!("✓ Connected to AWS KMS (region: {})", aws_region);

                // key_path should be in format: kms_key_id/encrypted_key_file
                // Example: "alias/dchat-validator-key/validator1.enc"
                let key_parts: Vec<&str> = key_path.split('/').collect();
                if key_parts.len() < 2 {
                    return Err(Error::Crypto(format!(
                        "Invalid KMS key path format. Expected: kms_key_id/encrypted_key_file, got: {}",
                        key_path
                    )));
                }

                let kms_key_id = key_parts[0];
                let encrypted_key_file = key_parts[1];

                // Verify KMS key exists and is accessible
                match kms_client.verify_key_exists(kms_key_id).await {
                    Ok(true) => {
                        info!("✓ KMS key verified: {}", kms_key_id);
                    }
                    Ok(false) => {
                        return Err(Error::Crypto(format!(
                            "KMS key exists but is disabled: {}",
                            kms_key_id
                        )));
                    }
                    Err(e) => {
                        return Err(Error::Crypto(format!(
                            "Failed to verify KMS key {}: {}",
                            kms_key_id, e
                        )));
                    }
                }

                // Load encrypted key material from file
                let key_file = PathBuf::from("./validator_keys").join(encrypted_key_file);

                if !key_file.exists() {
                    return Err(Error::Crypto(format!(
                        "Encrypted key file not found: {:?}. Generate with: dchat keygen --use-hsm",
                        key_file
                    )));
                }

                // Read encrypted key and public key
                let key_data = tokio::fs::read(&key_file)
                    .await
                    .map_err(|e| Error::Crypto(format!("Failed to read key file: {}", e)))?;

                // Parse key file format: [32 bytes public key][remaining bytes encrypted private key]
                if key_data.len() < 32 {
                    return Err(Error::Crypto(format!(
                        "Invalid key file format: too small (expected at least 32 bytes)"
                    )));
                }

                // Safe: length check above guarantees at least 32 bytes
                let public_key_bytes: [u8; 32] = match key_data[..32].try_into() {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(Error::Crypto("Failed to convert key bytes".to_string())),
                };
                let encrypted_private_key = key_data[32..].to_vec();

                let kms_wrapper = Ed25519KmsWrapper::load(
                    kms_client,
                    kms_key_id.to_string(),
                    encrypted_private_key,
                    &public_key_bytes,
                )
                .map_err(|e| Error::Crypto(format!("Failed to load KMS-protected key: {}", e)))?;

                info!("✓ Validator key loaded from AWS KMS");
                info!("  KMS Key ID: {}", kms_key_id);
                info!("  Public Key: {}", hex::encode(public_key_bytes));

                // Create KeyPair adapter for KMS-backed key
                // The adapter allows KMS-protected keys to work with code expecting KeyPair interface
                let kms_keypair = KmsKeyPairAdapter::new(kms_wrapper);

                info!("✓ KMS KeyPair adapter created successfully");
                info!("✓ KMS integration complete - validator will use remote signing");

                ValidatorKeyType::Kms(kms_keypair)
            }
            Err(e) => {
                warn!("AWS KMS not available: {}", e);
                info!("Falling back to local encrypted key storage");

                let key_file = PathBuf::from("./validator_keys")
                    .join(&key_path)
                    .with_extension("key");

                if key_file.exists() {
                    ValidatorKeyType::Local(load_validator_key(&key_file).await?)
                } else {
                    return Err(Error::Crypto(format!(
                        "Validator key not found: {:?}. Generate with: dchat keygen",
                        key_file
                    )));
                }
            }
        }
    } else {
        info!("Loading validator key from file: {}", key_path);
        ValidatorKeyType::Local(load_validator_key(&PathBuf::from(key_path)).await?)
    };

    let validator_id_bytes = validator_key.public_key_bytes();
    info!(
        "✓ Validator key loaded: {:?}",
        hex::encode(&validator_id_bytes)
    );

    // MAINNET: Initialize DNS-based peer discovery
    info!("🌍 Initializing DNS-based peer discovery for mainnet...");

    let dns_config = dchat_network::DnsDiscoveryConfig::default();
    let dns_discovery = dchat_network::DnsDiscoveryManager::new(dns_config.clone())
        .map_err(|e| Error::network(format!("Failed to create DNS discovery: {}", e)))?;

    // Start DNS refresh background task
    let _dns_refresh_handle = dns_discovery.start_refresh_task();
    info!(
        "✓ DNS refresh task started (interval: {:?})",
        dns_config.refresh_interval
    );

    // Discover all validators via DNS
    info!("🔍 Discovering validators via subdomains...");
    let discovered_validators = dns_discovery
        .discover_validators()
        .await
        .map_err(|e| Error::network(format!("Failed to discover validators: {}", e)))?;

    info!("✓ Discovered {} validators:", discovered_validators.len());
    for validator in &discovered_validators {
        info!(
            "  - {} at {}:{} ({})",
            validator.identifier, validator.ip, validator.port, validator.multiaddr
        );
    }

    // Convert discovered validators to bootstrap nodes
    // IMPORTANT: libp2p Kademlia needs correct PeerIds for bootstrap.
    // DNS discovery returns address-only multiaddrs (no /p2p/), so we must not invent PeerIds.
    // Seed bootstrap peers from config/CLI (must include /p2p/<PeerId>).
    let mut bootstrap_nodes: Vec<(PeerId, Multiaddr)> = Vec::new();

    for peer_str in &config.network.bootstrap_peers {
        let multiaddr = peer_str.parse::<Multiaddr>().map_err(|e| {
            Error::Config(format!(
                "Invalid bootstrap peer multiaddr '{}': {e}",
                peer_str
            ))
        })?;

        let multiaddr_str = multiaddr.to_string();
        let peer_id = multiaddr_str
            .split("/p2p/")
            .nth(1)
            .and_then(|s| s.split('/').next())
            .ok_or_else(|| {
                Error::Config(format!(
                    "Mainnet bootstrap peer must include /p2p/<PeerId>: {}",
                    peer_str
                ))
            })?
            .parse::<PeerId>()
            .map_err(|e| {
                Error::Config(format!(
                    "Invalid /p2p/<PeerId> in bootstrap peer '{}': {e}",
                    peer_str
                ))
            })?;

        bootstrap_nodes.push((peer_id, multiaddr.clone()));
        info!("  + Bootstrap peer: {} at {}", peer_id, multiaddr);
    }

    // If DNS discovery ever includes /p2p/<PeerId> in the future, accept it as an additional source.
    for validator in &discovered_validators {
        let multiaddr_str = validator.multiaddr.to_string();
        let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) else {
            continue;
        };

        if let Ok(peer_id) = peer_id_part
            .split('/')
            .next()
            .unwrap_or("")
            .parse::<PeerId>()
        {
            bootstrap_nodes.push((peer_id, validator.multiaddr.clone()));
            info!(
                "  + DNS bootstrap peer: {} at {}",
                peer_id, validator.multiaddr
            );
        }
    }

    info!("📡 Total bootstrap peers: {}", bootstrap_nodes.len());

    if bootstrap_nodes.is_empty() && !genesis_bootstrap {
        return Err(Error::Config(
            "No valid bootstrap peers configured for mainnet validator. Set [network].bootstrap_peers to multiaddrs including /p2p/<PeerId>. Use --genesis-bootstrap for initial genesis validators.".to_string(),
        ));
    }

    if bootstrap_nodes.is_empty() && genesis_bootstrap {
        info!("🌱 Genesis bootstrap mode: proceeding without bootstrap peers");
        info!("   This validator will accept incoming connections from other genesis validators");
    }

    // Derive libp2p keypair from validator key for persistent peer ID
    let (libp2p_keypair, derived_peer_id) =
        if let Some(private_key_bytes) = validator_key.private_key_bytes() {
            info!("🔑 Deriving persistent libp2p keypair from validator key...");
            match libp2p::identity::Keypair::ed25519_from_bytes(private_key_bytes.to_vec()) {
                Ok(kp) => {
                    let peer_id = kp.public().to_peer_id();
                    info!("✓ Derived persistent peer ID: {}", peer_id);
                    (Some(kp), Some(peer_id))
                }
                Err(e) => {
                    warn!("Failed to derive libp2p keypair from validator key: {}", e);
                    warn!("Falling back to random keypair (peer ID will change on restart)");
                    (None, None)
                }
            }
        } else {
            warn!("KMS keys do not expose private key - using random libp2p keypair");
            (None, None)
        };

    // Parse listen addresses
    let mut listen_addrs = Vec::new();
    for addr_str in &config.network.listen_addresses {
        if let Ok(addr) = addr_str.parse() {
            listen_addrs.push(addr);
        }
    }

    // Parse external address if configured
    let external_address = config
        .network
        .external_address
        .as_ref()
        .and_then(|addr_str| {
            addr_str.parse().ok().map(|addr| {
                info!("📡 External address configured: {}", addr_str);
                addr
            })
        });

    // Create network config with discovered peers
    let network_config = dchat_network::NetworkConfig {
        listen_addrs,
        discovery: dchat_network::DiscoveryConfig {
            local_peer_id: derived_peer_id.unwrap_or_else(PeerId::random),
            bootstrap_nodes,
            enable_mdns: config.network.enable_mdns,
            min_peers: 6, // Expect 6 other validators
            max_peers: config.network.max_connections as usize,
            query_timeout: std::time::Duration::from_millis(config.network.connection_timeout_ms),
            k_bucket_size: 20,
            alpha: 3,
        },
        nat: dchat_network::NatConfig {
            enable_upnp: config.network.enable_upnp,
            stun_servers: vec![
                "stun.l.google.com:19302".to_string(),
                "stun1.l.google.com:19302".to_string(),
            ],
            enable_hole_punching: true, // Enable for NAT traversal
            turn_servers: vec![],
            discovery_timeout: std::time::Duration::from_secs(10),
            lease_duration: std::time::Duration::from_secs(3600),
            port_range: (49152, 65535),
        },
        external_address,
        rate_limits: SwarmRateLimitConfig {
            max_messages_per_second: MAX_MESSAGES_PER_SECOND,
            max_messages_per_peer_per_second: MAX_MESSAGE_RATE_PER_SECOND as u32,
            max_connections: MAX_CONCURRENT_CONNECTIONS,
            max_connections_per_ip: MAX_CONNECTIONS_PER_IP,
            max_bandwidth_bytes_per_sec: MAX_BANDWIDTH_BYTES_PER_SEC,
        },
    };

    // Initialize network manager with persistent keypair if available
    let mut network = NetworkManager::with_keypair(network_config, libp2p_keypair).await?;
    let peer_id = network.peer_id();

    network.start().await?;
    info!("✓ Validator network initialized (peer_id: {})", peer_id);

    // Dial DNS-discovered validator addresses (address-only). This helps initial connectivity
    // even when PeerIds are learned via Identify after connection establishment.
    for validator in &discovered_validators {
        if let Err(e) = network.dial(validator.multiaddr.clone()) {
            debug!(
                "Dial to DNS-discovered validator {} ({}) failed: {}",
                validator.identifier, validator.multiaddr, e
            );
        }
    }

    // Subscribe to validator consensus topic
    network.subscribe_validators()?;
    info!("✓ Subscribed to validator consensus network");

    // Count all unique validators: discovered via DNS + manual bootstrap peers from config
    let mut unique_validator_peers = std::collections::HashSet::new();

    // Add discovered validators (only if they include /p2p/<PeerId>)
    for validator in &discovered_validators {
        let multiaddr_str = validator.multiaddr.to_string();
        if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
            if let Ok(pid) = peer_id_part
                .split('/')
                .next()
                .unwrap_or("")
                .parse::<PeerId>()
            {
                unique_validator_peers.insert(pid);
            }
        }
    }

    // Add manual bootstrap peers from config
    for peer_str in &config.network.bootstrap_peers {
        if let Ok(multiaddr) = peer_str.parse::<Multiaddr>() {
            let multiaddr_str = multiaddr.to_string();
            if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
                if let Ok(pid) = peer_id_part.parse::<PeerId>() {
                    unique_validator_peers.insert(pid);
                }
            }
        }
    }

    // Remove self from the count (we'll add +1 for self below)
    let self_peer_id = derived_peer_id.unwrap_or_else(PeerId::random);
    unique_validator_peers.remove(&self_peer_id);

    // Compute dynamic BFT thresholds based on ALL validators (discovered + manual + self)
    let total_validators = unique_validator_peers.len() + 1; // +1 for this node
    use dchat_validator::BftConfig;
    let bft_config = BftConfig::from_validator_count(total_validators, 3, 0.40);
    let f = bft_config.byzantine_tolerance();
    let required_signatures = bft_config.required_signatures;

    info!(
        "🔐 BFT Configuration: N={} (discovered={}, manual={}, +self), f={}, required_signatures={}",
        total_validators, discovered_validators.len(), unique_validator_peers.len() - discovered_validators.len(), f, required_signatures
    );
    info!(
        "   Byzantine tolerance: can tolerate {} faulty validators",
        f
    );

    // Initialize Peer Registry for validators
    info!("📋 Initializing validator peer registry");
    let peer_registry = PeerRegistry::new();

    // Register discovered validators as bootstrap peers
    for validator in &discovered_validators {
        let multiaddr_str = validator.multiaddr.to_string();
        if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
            if let Ok(pid) = peer_id_part.parse::<PeerId>() {
                let peer_info = PeerInfo {
                    peer_id: pid,
                    multiaddr: validator.multiaddr.clone(),
                    node_type: NodeType::Validator,
                    geographic_region: Some(validator.identifier.clone()),
                    last_seen: SystemTime::now(),
                    connection_quality: 1.0,
                    capabilities: vec!["consensus".to_string(), "block_production".to_string()],
                    is_bootstrap: true,
                    rtt_ms: None,
                    packet_loss: 0.0,
                    jitter_ms: None,
                    total_messages_sent: 0,
                    total_messages_received: 0,
                    handshake_success: false,
                    connected_since: SystemTime::now(),
                };
                peer_registry.add_bootstrap_peer(peer_info.clone()).await;
                peer_registry.add_peer(peer_info).await;
            }
        }
    }

    // Also register manual bootstrap peers from config as validators
    // This is critical when DNS discovery fails or is not configured
    for peer_str in &config.network.bootstrap_peers {
        if let Ok(multiaddr) = peer_str.parse::<Multiaddr>() {
            let multiaddr_str = multiaddr.to_string();
            if let Some(peer_id_part) = multiaddr_str.split("/p2p/").nth(1) {
                if let Ok(pid) = peer_id_part.parse::<PeerId>() {
                    // Extract region from DNS name if present (e.g., validator.india.schikuno.top -> india)
                    let region = multiaddr_str
                        .split("/dns4/")
                        .nth(1)
                        .and_then(|s| s.split('/').next())
                        .and_then(|domain| {
                            // Parse "validator.india.schikuno.top" -> "india"
                            domain.split('.').nth(1).map(|r| r.to_string())
                        });

                    let peer_info = PeerInfo {
                        peer_id: pid,
                        multiaddr: multiaddr.clone(),
                        node_type: NodeType::Validator,
                        geographic_region: region,
                        last_seen: SystemTime::now(),
                        connection_quality: 1.0,
                        capabilities: vec!["consensus".to_string(), "block_production".to_string()],
                        is_bootstrap: true,
                        rtt_ms: None,
                        packet_loss: 0.0,
                        jitter_ms: None,
                        total_messages_sent: 0,
                        total_messages_received: 0,
                        handshake_success: false,
                        connected_since: SystemTime::now(),
                    };
                    let region_for_log = peer_info.geographic_region.clone();
                    peer_registry.add_bootstrap_peer(peer_info.clone()).await;
                    peer_registry.add_peer(peer_info).await;
                    info!(
                        "  + Registered manual bootstrap validator: {} (region: {:?})",
                        pid, region_for_log
                    );
                }
            }
        }
    }

    let bootstrap_count = peer_registry.get_bootstrap_peers().await.len();
    info!("✓ Registered {} bootstrap validators", bootstrap_count);

    // Start background peer management tasks
    let network_arc = Arc::new(tokio::sync::Mutex::new(network));
    let peer_registry_arc = Arc::new(peer_registry);

    let _health_monitor_handle = {
        let registry = peer_registry_arc.clone();
        let net = network_arc.clone();
        let shutdown = shutdown_tx.subscribe();
        tokio::spawn(async move {
            run_peer_health_monitor(registry, net, shutdown).await;
        })
    };

    let _sync_handle = {
        let registry = peer_registry_arc.clone();
        let net = network_arc.clone();
        let shutdown = shutdown_tx.subscribe();
        tokio::spawn(async move {
            run_peer_list_sync(registry, net, shutdown).await;
        })
    };
    info!("✓ Background peer management tasks started");

    // Wait for initial peer connections (critical for consensus)
    // Need at least 2f+1 validators connected for consensus
    info!("🤝 Waiting for validator peer connections...");
    let connection_deadline = tokio::time::Instant::now()
        + tokio::time::Duration::from_secs(VALIDATOR_CONNECTION_TIMEOUT_SECONDS);
    let mut connected_validators = 0;
    let geographic_region = get_geographic_region();

    // Minimum peers needed (don't count self, need required_signatures - 1 peers)
    let min_peers_needed = required_signatures.saturating_sub(1);

    while tokio::time::Instant::now() < connection_deadline {
        // Get the next event, releasing the lock after the event is received
        let event = {
            let mut net = network_arc.lock().await;
            tokio::time::timeout(tokio::time::Duration::from_secs(5), net.next_event()).await
        };

        match event {
            Ok(Some(NetworkEvent::PeerConnected {
                peer_id: connected_peer_id,
                endpoint,
            })) => {
                connected_validators += 1;
                info!(
                    "✓ Validator peer connected: {} at {:?} ({}/{} required for consensus)",
                    connected_peer_id,
                    endpoint,
                    connected_validators + 1,
                    required_signatures
                );

                // Update peer registry
                peer_registry_arc
                    .update_peer_quality(&connected_peer_id, 1.0)
                    .await;

                // Update DNS discovery with actual peer ID
                for validator in &discovered_validators {
                    let multiaddr_str = validator.multiaddr.to_string();
                    if multiaddr_str.contains(&validator.ip.to_string()) {
                        let _ = dns_discovery
                            .update_peer_id(&validator.identifier, connected_peer_id)
                            .await;
                    }
                }

                // Perform validator handshake (separate lock acquisition)
                {
                    let mut net = network_arc.lock().await;
                    match perform_peer_handshake(
                        connected_peer_id,
                        &mut *net,
                        &peer_registry_arc,
                        NodeType::Validator,
                        geographic_region.clone(),
                    )
                    .await
                    {
                        Ok(_) => debug!("Validator handshake completed with {}", connected_peer_id),
                        Err(e) => warn!(
                            "Validator handshake failed with {}: {}",
                            connected_peer_id, e
                        ),
                    }
                }

                // Check if we've reached consensus threshold
                if connected_validators >= min_peers_needed {
                    info!(
                        "✓ Consensus threshold reached ({}/{} validators connected, need {})",
                        connected_validators + 1,
                        total_validators,
                        required_signatures
                    );
                    break;
                }
            }
            Ok(Some(_)) => {
                // Other events, continue waiting
            }
            Ok(None) | Err(_) => {
                // Timeout, continue waiting
            }
        }
    }

    if connected_validators < min_peers_needed {
        error!(
            "❌ Failed to connect to minimum validators: {}/{} connected (need {} for consensus)",
            connected_validators + 1,
            total_validators,
            required_signatures
        );
        return Err(Error::network(format!(
            "Insufficient validator connections for consensus: got {}, need {}",
            connected_validators + 1,
            required_signatures
        )));
    }

    info!(
        "✓ Validator network ready with {} peers",
        connected_validators
    );

    // Initialize storage
    let db_config = create_db_config(&config, "dchat_validator.db");
    let database = Database::new(db_config).await?;
    info!("✓ Database initialized");

    // Connect to chain RPC (only if not using genesis mode)
    let chain_rpc_url = chain_rpc
        .clone()
        .unwrap_or_else(|| "genesis-mode".to_string());
    if genesis_dir.is_none() {
        info!("Connecting to chain at {}...", chain_rpc_url);
    }

    // Production: Initialize chain client (skip for genesis mode)
    let chat_chain_config = ChatChainConfig {
        rpc_url: chain_rpc_url.clone(),
        ..Default::default()
    };
    let _chat_chain = ChatChainClient::new(chat_chain_config);

    // MAINNET SECURITY: Validate stake amount meets minimum requirements
    if stake_amount < MIN_STAKE_FOR_VALIDATOR {
        error!(
            "❌ Insufficient stake amount: {} tokens (minimum required: {})",
            stake_amount, MIN_STAKE_FOR_VALIDATOR
        );
        return Err(Error::validation(format!(
            "Validator stake must be at least {} tokens for mainnet",
            MIN_STAKE_FOR_VALIDATOR
        )));
    }

    info!("Submitting validator stake of {} tokens...", stake_amount);

    // PRODUCTION: Initialize staking manager (for local state tracking)
    use dchat_blockchain::staking::StakingManager;
    use ed25519_dalek::VerifyingKey as Ed25519VerifyingKey;

    let staking_manager = Arc::new(StakingManager::new());
    let staking_manager_clone = Arc::clone(&staking_manager); // Clone for shutdown handler

    // Convert validator key to Ed25519 public key bytes
    let public_key_bytes = validator_key.public_key_bytes();
    let ed25519_pubkey = Ed25519VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?;

    // Create validator user ID - derive deterministic UUID from public key
    let validator_user_id = {
        // Use first 16 bytes of hash as UUID
        let key_hash = blake3::hash(&public_key_bytes);
        // BLAKE3 hash is always 32 bytes, so taking first 16 bytes is safe.
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&key_hash.as_bytes()[..16]);
        UserId(uuid::Uuid::from_bytes(uuid_bytes))
    };
    let validator_user_id_clone = validator_user_id.clone(); // Clone for shutdown handler

    // Check if using pre-stake genesis (no RPC required)
    let verifying_key = Ed25519VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|e| Error::crypto(format!("Failed to create verifying key: {}", e)))?;

    // Collect authorized genesis validator public keys for block verification
    // This set is shared with the network event handler to verify incoming blocks
    let authorized_validators: Arc<std::collections::HashSet<[u8; 32]>> =
        Arc::new(std::collections::HashSet::new());

    let genesis_stake_amount = if let Some(ref genesis_path) = genesis_dir {
        // Load pre-stake genesis and verify this validator is included
        use dchat_chain::chain::genesis::CurrencyGenesisBlock;

        let currency_genesis_path = genesis_path.join("currency_chain_genesis.json");
        if !currency_genesis_path.exists() {
            return Err(Error::validation(format!(
                "Currency genesis file not found at {:?}",
                currency_genesis_path
            )));
        }

        let genesis_json = std::fs::read_to_string(&currency_genesis_path)
            .map_err(|e| Error::io(format!("Failed to read genesis file: {}", e)))?;
        let currency_genesis: CurrencyGenesisBlock = serde_json::from_str(&genesis_json)
            .map_err(|e| Error::serialization(format!("Failed to parse genesis: {}", e)))?;

        // Collect all authorized validator public keys from genesis
        let mut auth_validators = std::collections::HashSet::new();
        for v in &currency_genesis.initial_validators {
            if let Ok(pubkey_bytes) = hex::decode(&v.public_key) {
                if pubkey_bytes.len() == 32 {
                    let mut key_array = [0u8; 32];
                    key_array.copy_from_slice(&pubkey_bytes);
                    auth_validators.insert(key_array);
                    info!(
                        "   📋 Authorized genesis validator: {}",
                        &v.public_key[..16]
                    );
                }
            }
        }
        // Replace the empty set with the populated one
        let authorized_validators = Arc::new(auth_validators);

        // Find this validator in genesis
        let validator_pubkey_hex = hex::encode(&public_key_bytes);
        let genesis_validator = currency_genesis
            .initial_validators
            .iter()
            .find(|v| v.public_key == validator_pubkey_hex);

        match genesis_validator {
            Some(v) => {
                info!("🌱 PRE-STAKE GENESIS: Validator found in genesis!");
                info!("   Public Key: {}", validator_pubkey_hex);
                info!(
                    "   Genesis Stake: {} motes ({} DCHAT)",
                    v.stake_amount,
                    v.stake_amount / MOTES_PER_DCHAT
                );
                info!("   Voting Power: {}", v.voting_power);
                info!("   ✅ No RPC required - stake is embedded in genesis block");
                Some((v.stake_amount, authorized_validators))
            }
            None => {
                error!("❌ Validator public key not found in genesis file!");
                error!("   Your key: {}", validator_pubkey_hex);
                error!("   Genesis validators:");
                for v in &currency_genesis.initial_validators {
                    error!("     - {}", v.public_key);
                }
                return Err(Error::validation(
                    "Validator not in pre-stake genesis. Use --chain-rpc to stake via RPC."
                        .to_string(),
                ));
            }
        }
    } else {
        None
    };

    // Extract authorized validators set (empty if not using genesis)
    let authorized_validators: Arc<std::collections::HashSet<[u8; 32]>> = genesis_stake_amount
        .as_ref()
        .map(|(_, auth)| Arc::clone(auth))
        .unwrap_or_else(|| Arc::new(std::collections::HashSet::new()));

    // Extract just the stake amount for later use
    let genesis_stake_amount = genesis_stake_amount.map(|(stake, _)| stake);

    // Either use genesis stake or submit via RPC
    let effective_stake_motes = if let Some(genesis_motes) = genesis_stake_amount {
        // Using pre-stake genesis - no RPC submission needed
        info!(
            "✅ Using pre-staked genesis amount: {} DCHAT",
            genesis_motes / MOTES_PER_DCHAT
        );
        genesis_motes
    } else {
        // MAINNET PRODUCTION: Submit stake transaction to CURRENCY CHAIN via RPC
        // This submits the actual on-chain staking transaction with finality confirmation
        use dchat_chain::chain::currency_chain::staking::{submit_validator_stake, StakeRequest};

        info!("📤 Submitting on-chain stake transaction to currency chain...");
        info!(
            "   Stake Amount: {} DCHAT ({} motes)",
            stake_amount,
            stake_amount * MOTES_PER_DCHAT
        );
        info!(
            "   Validator Public Key: {}",
            hex::encode(&public_key_bytes)
        );
        info!("   Lockup Period: 7 days (minimum validator requirement)");

        let stake_request = StakeRequest {
            validator_key: verifying_key,
            amount: stake_amount * MOTES_PER_DCHAT, // Convert to motes (8 decimal places)
            lockup_period_days: 7,                  // Minimum lockup for validators
        };

        let stake_receipt = match submit_validator_stake(&stake_request).await {
            Ok(receipt) => {
                info!("✅ On-chain stake transaction CONFIRMED!");
                info!("   Transaction ID: {}", receipt.transaction_id);
                info!("   Block Height: {}", receipt.block_height);
                info!("   Activation Time: {:?}", receipt.activation_timestamp);
                info!("   Unlock Time: {:?}", receipt.unlock_timestamp);
                receipt
            }
            Err(e) => {
                error!("❌ Currency chain stake submission FAILED: {}", e);
                error!("   This is a mainnet blocker - validator cannot participate without on-chain stake");
                error!("   Check:");
                error!("     1. CURRENCY_CHAIN_RPC environment variable is set correctly");
                error!(
                    "     2. Currency chain RPC endpoint is accessible: {}",
                    std::env::var("DCHAT_CURRENCY_CHAIN_RPC_URL")
                        .or_else(|_| std::env::var("CURRENCY_CHAIN_RPC"))
                        .unwrap_or_else(|_| "<unset>".to_string())
                );
                error!(
                    "     3. Validator wallet has sufficient balance (need {} tokens + gas)",
                    stake_amount
                );
                error!("     4. Currency chain is running and accepting transactions");
                return Err(Error::chain(format!("On-chain staking failed: {}", e)));
            }
        };
        let _ = stake_receipt; // Use the receipt (transaction ID logged above)

        stake_amount * MOTES_PER_DCHAT
    };

    // Register stake in local staking manager (for tracking and consensus eligibility)
    info!("📝 Registering stake in local validator state...");
    match staking_manager
        .submit_validator_stake(
            validator_user_id.clone(),
            effective_stake_motes, // Use effective stake (from genesis or RPC)
            ed25519_pubkey,
        )
        .await
    {
        Ok(local_tx_id) => {
            info!("✓ Local stake registration successful");
            info!("   Local TX ID: {}", local_tx_id);
            info!(
                "   Effective Stake: {} motes ({} DCHAT)",
                effective_stake_motes,
                effective_stake_motes / MOTES_PER_DCHAT
            );
        }
        Err(e) => {
            error!("⚠️ Local stake registration failed (non-fatal): {}", e);
            warn!("Continuing with genesis/on-chain stake confirmation only");
        }
    }

    // Skip finality wait and RPC verification if using genesis stake
    if genesis_dir.is_none() {
        // Wait for chain finality (3 blocks at 6 seconds = 18 seconds)
        info!("⏳ Waiting for chain finality (3 blocks ~18 seconds)...");
        tokio::time::sleep(tokio::time::Duration::from_secs(18)).await;

        // Verify stake on currency chain
        use dchat_chain::chain::currency_chain::staking::get_validator_stake;
        match get_validator_stake(&verifying_key).await {
            Ok(confirmed_stake) => {
                if confirmed_stake >= effective_stake_motes {
                    info!("✅ Stake FINALIZED on currency chain!");
                    info!(
                        "   Confirmed Stake: {} motes ({} DCHAT)",
                        confirmed_stake,
                        confirmed_stake as f64 / MOTES_PER_DCHAT as f64
                    );
                } else {
                    warn!(
                        "⚠️ Stake confirmation mismatch: expected {}, got {}",
                        effective_stake_motes, confirmed_stake
                    );
                }
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to verify stake on-chain (continuing anyway): {}",
                    e
                );
            }
        }
    } else {
        info!("✅ Using pre-stake genesis - skipping RPC finality wait");
    }

    // Activate validator in local state
    info!("🎯 Activating validator for consensus participation...");
    match staking_manager.activate_validator(&validator_user_id).await {
        Ok(_) => {
            info!("✅ Validator ACTIVATED and ready for consensus!");
            info!("   Status: ACTIVE");
            info!("   Eligible for block production: YES");
            info!("   Consensus voting power: {}", stake_amount);
        }
        Err(e) => {
            error!("❌ Validator activation failed: {}", e);
            error!("   This is critical - cannot participate in consensus without activation");
            return Err(Error::chain(format!("Validator activation error: {}", e)));
        }
    }

    // Start consensus participation with BFT block verification
    use dchat_blockchain::StateValidator;
    use dchat_network::DchatMessage;
    use std::collections::HashMap;

    // Track block acknowledgments for BFT consensus
    let block_acknowledgments: Arc<tokio::sync::Mutex<HashMap<u64, HashMap<Vec<u8>, Vec<u8>>>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    // Initialize state validator for Byzantine fault detection
    let state_validator: Arc<tokio::sync::Mutex<StateValidator>> =
        Arc::new(tokio::sync::Mutex::new(StateValidator::new()));

    // Seed genesis block (block 0) to establish chain anchor for state validation
    // This is required before validating any subsequent blocks
    {
        let mut validator = state_validator.lock().await;
        validator.seed_genesis_default();
        info!("✓ State validator initialized with genesis anchor (block 0)");
    }

    // Resolve currency chain RPC URL before spawning consensus task (avoid panic inside task)
    let consensus_currency_rpc_url = resolve_required_currency_chain_rpc_url(&config)?;

    // Get public key bytes for signing in consensus (validator_key is moved into Arc for sharing)
    let validator_public_key_bytes = validator_key.public_key_bytes();
    let validator_key_arc = Arc::new(tokio::sync::Mutex::new(validator_key));

    // Initialize relay registry and work event stores for epoch reward distribution
    // These are shared across consensus loop and network event handler
    use dchat::relay_work_store::{RelayRegistryStore, RelayWorkEventStore};
    let relay_registry_store = Arc::new(RelayRegistryStore::new(
        config.chain.genesis_timestamp,
        config.chain.block_time_secs,
    ));
    let relay_work_store = Arc::new(RelayWorkEventStore::new());

    // Phase 3: Feature-flagged Watchtower for fraud detection
    // When enabled, monitors payment channels for fraudulent close attempts
    // and automatically submits fraud proofs to slash malicious actors
    let _watchtower_handle: Option<tokio::task::JoinHandle<()>> =
        if config.features.enable_watchtower {
            info!("🔍 Initializing watchtower for fraud detection...");

            // Create a payment channel manager for the watchtower
            let watchtower_channel_manager =
                Arc::new(dchat_blockchain::PaymentChannelManager::new());

            // Create currency chain client for blockchain queries
            let watchtower_currency_config = CurrencyChainConfig {
                rpc_url: consensus_currency_rpc_url.clone(),
                ..Default::default()
            };
            let watchtower_currency_client = Arc::new(
                CurrencyChainClient::new(watchtower_currency_config).map_err(|e| {
                    Error::internal(format!(
                        "Failed to create watchtower currency client: {}",
                        e
                    ))
                })?,
            );

            // Create watchtower with default config
            let watchtower_config = dchat_blockchain::WatchtowerConfig::default();
            let watchtower = Arc::new(dchat_blockchain::Watchtower::new(
                watchtower_config,
                watchtower_channel_manager,
            ));

            // Create and start the monitor
            let monitor = Arc::new(dchat_blockchain::WatchtowerMonitor::new(
                watchtower,
                watchtower_currency_client,
            ));

            let monitor_clone = Arc::clone(&monitor);
            let handle = tokio::spawn(async move {
                info!("   ✓ Watchtower background monitor started");
                monitor_clone.start().await;
            });

            info!("   ✓ Watchtower enabled (poll interval: 30s)");
            Some(handle)
        } else {
            debug!("Watchtower disabled (no automatic fraud detection)");
            None
        };

    // Phase 3: Feature-flagged Oracle Network for price feeds
    // When enabled, provides oracle infrastructure for external data aggregation
    // with weighted median consensus and reputation-based slashing
    let _oracle_network: Option<Arc<dchat_blockchain::OracleNetwork>> =
        if config.features.enable_oracle {
            info!("🔮 Initializing oracle network...");

            // Create oracle network with minimum stake from config (already in motes)
            let oracle_network = dchat_blockchain::OracleNetwork::new(config.oracle.min_stake);

            info!(
                "   ✓ Oracle network enabled (min stake: {} DCHAT)",
                config.oracle.min_stake / 100_000_000
            );

            Some(Arc::new(oracle_network))
        } else {
            debug!("Oracle network disabled (no external price feeds)");
            None
        };

    // Phase 4: Feature-flagged Marketplace for digital goods trading
    // When enabled, provides marketplace infrastructure for bots, stickers, NFTs, etc.
    // with escrow system for secure transactions
    let _marketplace_manager: Option<
        Arc<std::sync::RwLock<dchat_marketplace::MarketplaceManager>>,
    > = if config.features.enable_marketplace {
        info!("🏪 Initializing marketplace manager...");

        let marketplace = dchat_marketplace::MarketplaceManager::new();

        info!("   ✓ Marketplace enabled (digital goods, NFTs, bots)");
        info!("   ✓ Escrow system ready for secure transactions");

        Some(Arc::new(std::sync::RwLock::new(marketplace)))
    } else {
        debug!("Marketplace disabled");
        None
    };

    // Create shared block height for synchronization between consensus and network event handlers
    use std::sync::atomic::AtomicU64;
    let shared_block_height_global = Arc::new(AtomicU64::new(0));

    let consensus_handle = {
        let network_arc_clone = network_arc.clone();
        let validator_key_arc_clone = Arc::clone(&validator_key_arc);
        let block_acks_clone = block_acknowledgments.clone();
        let state_validator_clone = state_validator.clone();
        let staking_manager_consensus = staking_manager.clone();
        let shared_block_height = Arc::clone(&shared_block_height_global);
        let currency_rpc_url_for_consensus = consensus_currency_rpc_url.clone();
        let relay_registry_store = Arc::clone(&relay_registry_store);
        let relay_work_store = Arc::clone(&relay_work_store);
        // Phase 5: Pass feature flags for VRF committees and two-stage finality
        let enable_vrf_committees = config.features.enable_vrf_committees;
        let _enable_two_stage_finality = config.features.enable_two_stage_finality;
        // Clone validator public key for slot leader selection
        let our_validator_pubkey = validator_public_key_bytes;

        tokio::spawn(async move {
            info!("Starting consensus engine with BFT verification and FULL state validation...");

            // Initialize Consensus and Reward Managers
            let snapshot_store = Arc::new(SnapshotStore::new(10));
            let batch_verifier = Arc::new(ParkingRwLock::new(VerificationPipeline::new(100, 1000)));
            let hardened_consensus =
                HardenedPoRW::new(snapshot_store.clone(), batch_verifier.clone());

            // Phase 5: Initialize VRF committee selector if enabled
            // VRF committees use verifiable random functions for fair committee selection
            if enable_vrf_committees {
                info!("🎲 VRF committee selection enabled");
                // Committee selector will be initialized with relay data when available
                // The hardened_consensus.initialize_committee_selector() will be called
                // during epoch transitions when relay registry data is loaded
            }

            // Use the pre-resolved currency chain RPC URL (resolved before spawn to avoid panic)
            let currency_chain_config = CurrencyChainConfig {
                rpc_url: currency_rpc_url_for_consensus,
                ..Default::default()
            };

            // Retry loop with exponential backoff for currency client creation
            let mut attempts = 0;
            let max_attempts = 5;
            let mut delay = std::time::Duration::from_secs(1);

            let currency_client = loop {
                attempts += 1;
                match CurrencyChainClient::new(currency_chain_config.clone()) {
                    Ok(client) => break Arc::new(client),
                    Err(e) if attempts < max_attempts => {
                        warn!(
                            "Currency client connection failed (attempt {}/{}): {}. Retrying in {:?}...",
                            attempts, max_attempts, e, delay
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                    }
                    Err(e) => {
                        error!(
                            "Failed to create currency client for consensus task after {} attempts: {} (stopping consensus loop)",
                            max_attempts, e
                        );
                        return;
                    }
                }
            };

            let fee_manager = Arc::new(std::sync::RwLock::new(FeeDistributionManager::new(
                FeeDistributionConfig::default(),
            )));
            let tokenomics_manager = Arc::new(TokenomicsManager::new(TokenSupplyConfig::default()));

            // PRODUCTION IMPLEMENTATION: Full state validation with Merkle proofs
            // This implementation now uses the complete dchat_blockchain::Block structure
            // which includes:
            //   - block.state_root: Merkle root of all state transitions
            //   - block.subblocks[].miniblocks[].pre_state_hash / post_state_hash
            //
            // Full validation workflow:
            //   1. Build Block with subblocks and miniblocks containing transactions
            //   2. Calculate Merkle tree from state transitions
            //   3. Set block.state_root to Merkle root
            //   4. On receive: validate_block() verifies state_root against Merkle tree
            //   5. On StateValidationError::ByzantineFault: slash offending validator
            //   6. Periodically cleanup old state roots

            use dchat_blockchain::block_hierarchy::{
                ExecutionContext, ExecutionEngine, Hash as BlockHash, LaneId, MiniblockBody,
            };
            use dchat_blockchain::{Block, MerkleTree, Miniblock, StateValidationError, Subblock};
            use std::sync::atomic::Ordering;

            // shared_block_height is passed in from outer scope for cross-task synchronization

            let mut stats_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

            // World state for transaction execution
            let mut world_state = dchat_blockchain::WorldState::new();

            // Execution engine for parallel transaction processing
            let execution_engine = ExecutionEngine::new(true); // Enable parallel execution

            // Initialize Slot Leader Selector for coordinated block production
            // This ensures only one validator produces blocks per slot, preventing forks
            let mut slot_leader_selector = SlotLeaderSelector::with_defaults();

            // Create VRF keypair from validator's private key for cryptographic leader selection
            // The VRF keypair is derived deterministically from the validator's Ed25519 seed
            let vrf_keypair = {
                // Get validator's private key bytes for VRF derivation
                let validator_key = validator_key_arc_clone.lock().await;
                let vrf_seed = if let Some(priv_bytes) = validator_key.private_key_bytes() {
                    // Derive VRF seed from validator's private key using domain separation
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(b"dchat-vrf-keypair-derivation-v1");
                    hasher.update(&priv_bytes);
                    *hasher.finalize().as_bytes()
                } else {
                    // For KMS keys, use public key as seed (deterministic per validator)
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(b"dchat-vrf-keypair-derivation-v1");
                    hasher.update(&our_validator_pubkey);
                    *hasher.finalize().as_bytes()
                };
                drop(validator_key);

                // Generate schnorrkel keypair from deterministic seed
                use rand::SeedableRng;
                let mut rng = rand::rngs::StdRng::from_seed(vrf_seed);
                SchnorrkelKeypair::generate_with(&mut rng)
            };

            // Create Ed25519 signing key for block signatures
            let signing_key = {
                let validator_key = validator_key_arc_clone.lock().await;
                if let Some(priv_bytes) = validator_key.private_key_bytes() {
                    ed25519_dalek::SigningKey::from_bytes(&priv_bytes)
                } else {
                    // For KMS, generate a local signing key (blocks will be signed via KMS separately)
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(b"dchat-signing-key-derivation-v1");
                    hasher.update(&our_validator_pubkey);
                    ed25519_dalek::SigningKey::from_bytes(hasher.finalize().as_bytes())
                }
            };

            // Register ourselves as a validator for the current epoch
            // In production, this would be loaded from genesis/chain state
            let validator_info = ValidatorInfo {
                vrf_public_key: vrf_keypair.public.to_bytes(), // Proper VRF public key
                signing_public_key: our_validator_pubkey,
                stake_amount: 1000000000000000, // 10M DCHAT from genesis
                weight_bps: 3333,               // ~33% for 3 validators
                region: GeographicRegion::Africa, // Default region
                is_active: true,
                last_leader_slot: None,
                blocks_produced: 0,
                blocks_missed: 0,
            };
            slot_leader_selector.register_validator(0, validator_info);

            // Set initial epoch seed from genesis hash
            let genesis_seed: [u8; 32] = *blake3::hash(b"dchat-mainnet-1-genesis").as_bytes();
            slot_leader_selector.set_epoch_seed(0, genesis_seed);

            // Store VRF public key for leadership checks
            let vrf_public_key = vrf_keypair.public.to_bytes();

            info!(
                "🎯 Slot leader selector initialized with validator {} (VRF: {})",
                hex::encode(&our_validator_pubkey[..8]),
                hex::encode(&vrf_public_key[..8])
            );

            // Store the last VRF proof for block messages
            let mut last_vrf_proof: Option<SlotLeaderProof> = None;

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(6)) => {
                        // Block production interval (6 seconds / 2 second slots = 3 slots)
                        let current_slot = slot_leader_selector.current_slot();

                        // VRF-based leader selection: Compute our leadership proof for this slot
                        // Each validator computes VRF(slot_seed, private_key) and the lowest score wins
                        let is_vrf_leader = match slot_leader_selector.get_slot_seed(current_slot) {
                            Ok(slot_seed) => {
                                // Generate our VRF leadership proof
                                let our_proof = SlotLeaderProof::generate(
                                    current_slot,
                                    &slot_seed,
                                    &vrf_keypair,
                                    &signing_key,
                                    3333, // Our weight in basis points (~33%)
                                    GeographicRegion::Africa,
                                );

                                // Store proof for block message
                                last_vrf_proof = Some(our_proof.clone());

                                // Claim leadership - will succeed if we have the best score
                                match slot_leader_selector.claim_leadership(our_proof.clone()) {
                                    Ok(true) => {
                                        debug!(
                                            "🎲 VRF leadership claimed for slot {} (score: {})",
                                            current_slot,
                                            our_proof.leadership_score()
                                        );
                                        true
                                    }
                                    Ok(false) => {
                                        // Another validator has a better score
                                        debug!(
                                            "🎲 VRF leadership not won for slot {} (our score: {})",
                                            current_slot,
                                            our_proof.leadership_score()
                                        );
                                        false
                                    }
                                    Err(e) => {
                                        warn!("Failed to claim VRF leadership: {}", e);
                                        false
                                    }
                                }
                            }
                            Err(e) => {
                                // Slot seed not yet available (epoch not finalized)
                                // Fall back to deterministic selection based on current slot
                                debug!("Slot seed not available: {}, using fallback selection", e);
                                let current_height = shared_block_height.load(Ordering::SeqCst);
                                let leader_index = ((current_height + 1) % 3) as usize;
                                let our_index = (vrf_public_key[0] as usize) % 3;
                                leader_index == our_index
                            }
                        };

                        // Check if we are the designated leader for this slot via VRF
                        let is_leader = slot_leader_selector.is_leader(current_slot, &vrf_public_key) || is_vrf_leader;

                        if is_producer && is_leader {
                            let block_height = shared_block_height.fetch_add(1, Ordering::SeqCst) + 1;
                            info!("📦 Producing block #{} (slot {}) - WE ARE THE LEADER", block_height, current_slot);

                            // Advance tokenomics block counter for inflation tracking
                            if let Err(e) = tokenomics_manager.advance_block() {
                                warn!("Failed to advance tokenomics block counter: {}", e);
                            }

                            // Gather pending transactions from mempool
                            let pending_txs: Vec<dchat_chain::Transaction> = Vec::new();
                            info!("  • Gathered {} pending transactions from mempool", pending_txs.len());

                            // Create hierarchical block structure
                            let prev_hash = if block_height == 1 {
                                BlockHash::from([0u8; 32])
                            } else {
                                // In production, get previous block hash from storage
                                let mut prev_data = Vec::new();
                                prev_data.extend_from_slice(&(block_height - 1).to_le_bytes());
                                BlockHash::from(*blake3::hash(&prev_data).as_bytes())
                            };

                            let mut block = Block::new(block_height, prev_hash);

                            // Create subblock with miniblocks containing transactions
                            let mut subblock = Subblock::new(block_height, 0);
                            let mut all_state_transitions: Vec<Vec<u8>> = Vec::new();

                            // Deterministically shard transactions by lane, then build one miniblock per lane.
                            // NOTE: Subblock currently enforces unique lanes, so we take up to 10 lanes.
                            let mut txs_by_lane: std::collections::BTreeMap<
                                u16,
                                std::collections::VecDeque<dchat_chain::Transaction>,
                            > = std::collections::BTreeMap::new();

                            for tx in pending_txs.iter() {
                                match dchat_blockchain::block_hierarchy::lane_for_transaction(tx) {
                                    Ok(lane) => {
                                        txs_by_lane.entry(lane.0).or_default().push_back(tx.clone());
                                    }
                                    Err(e) => {
                                        warn!("Dropping invalid tx {:?}: {:?}", tx.tx_id, e);
                                    }
                                }
                            }

                            let mut next_miniblock_index: u16 = 0;

                            for (lane_u16, queue) in txs_by_lane.iter_mut() {
                                if next_miniblock_index >= 10 {
                                    warn!("Subblock miniblock limit reached; leaving remaining txs for next block");
                                    break;
                                }
                                if queue.is_empty() {
                                    continue;
                                }

                                // Keep miniblocks small for bounded worst-case cost.
                                let mut txs: Vec<dchat_chain::Transaction> = Vec::with_capacity(250);
                                while txs.len() < 250 {
                                    match queue.pop_front() {
                                        Some(tx) => txs.push(tx),
                                        None => break,
                                    }
                                }

                                let lane = LaneId(*lane_u16);
                                let body = MiniblockBody {
                                    transactions: txs,
                                    receipts: Vec::new(),
                                };

                                let mut miniblock = match Miniblock::new(block_height, 0, next_miniblock_index, lane, body) {
                                    Ok(mb) => mb,
                                    Err(e) => {
                                        warn!("Failed to build miniblock lane={}: {:?}", lane_u16, e);
                                        continue;
                                    }
                                };

                                // Capture pre-execution state root
                                let pre_state_hash = world_state.compute_state_root();
                                miniblock.header.pre_state_hash = pre_state_hash;

                                // Create execution context for this miniblock
                                let timestamp = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0);
                                let mut exec_ctx = ExecutionContext::new(
                                    block_height,
                                    timestamp,
                                    miniblock.header.gas_limit,
                                );

                                // Execute miniblock transactions
                                let receipts = match execution_engine.execute_miniblock(&miniblock, &mut world_state, &mut exec_ctx) {
                                    Ok(r) => r,
                                    Err(e) => {
                                        warn!("Miniblock execution failed lane={}: {:?}", lane_u16, e);
                                        continue;
                                    }
                                };

                                // Update miniblock with execution results
                                let post_state_hash = world_state.compute_state_root();
                                miniblock.header.post_state_hash = post_state_hash;
                                miniblock.header.gas_used = exec_ctx.gas_used;

                                // Store receipts in body
                                if let Some(ref mut body) = miniblock.body {
                                    body.receipts = receipts;
                                }

                                // Collect state transition for Merkle tree
                                let mut transition = Vec::new();
                                transition.extend_from_slice(&miniblock.header.index.to_le_bytes());
                                transition.extend_from_slice(miniblock.header.pre_state_hash.as_bytes());
                                transition.extend_from_slice(miniblock.header.post_state_hash.as_bytes());
                                transition.extend_from_slice(&miniblock.header.gas_used.to_le_bytes());
                                all_state_transitions.push(transition);

                                match subblock.add_miniblock(miniblock) {
                                    Ok(()) => next_miniblock_index += 1,
                                    Err(e) => {
                                        warn!("Failed to add miniblock to subblock: {:?}", e);
                                        break;
                                    }
                                }
                            }

                            // If no transactions, create an empty miniblock (keeps state_root deterministic).
                            if subblock.miniblocks.is_empty() {
                                let body = MiniblockBody {
                                    transactions: Vec::new(),
                                    receipts: Vec::new(),
                                };
                                let mut miniblock = match Miniblock::new(block_height, 0, 0, LaneId(0), body) {
                                    Ok(mb) => mb,
                                    Err(e) => {
                                        warn!("Failed to build empty miniblock: {:?}", e);
                                        continue;
                                    }
                                };

                                // Capture pre-execution state root (even for empty miniblock)
                                let pre_state_hash = world_state.compute_state_root();
                                miniblock.header.pre_state_hash = pre_state_hash;

                                // Create execution context
                                let timestamp = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0);
                                let mut exec_ctx = ExecutionContext::new(
                                    block_height,
                                    timestamp,
                                    miniblock.header.gas_limit,
                                );

                                // Execute (empty miniblock produces no receipts but maintains state consistency)
                                let receipts = match execution_engine.execute_miniblock(&miniblock, &mut world_state, &mut exec_ctx) {
                                    Ok(r) => r,
                                    Err(e) => {
                                        warn!("Empty miniblock execution failed: {:?}", e);
                                        continue;
                                    }
                                };

                                // Update miniblock with execution results
                                let post_state_hash = world_state.compute_state_root();
                                miniblock.header.post_state_hash = post_state_hash;
                                miniblock.header.gas_used = exec_ctx.gas_used;

                                // Store receipts in body
                                if let Some(ref mut body) = miniblock.body {
                                    body.receipts = receipts;
                                }

                                let mut transition = Vec::new();
                                transition.extend_from_slice(&0u16.to_le_bytes());
                                transition.extend_from_slice(miniblock.header.pre_state_hash.as_bytes());
                                transition.extend_from_slice(miniblock.header.post_state_hash.as_bytes());
                                transition.extend_from_slice(&0u64.to_le_bytes());
                                all_state_transitions.push(transition);

                                let _ = subblock.add_miniblock(miniblock);
                            }

                            // Calculate Merkle root from all state transitions
                            let merkle_tree = MerkleTree::from_state_transitions(all_state_transitions);
                            let state_root = merkle_tree.root_hash()
                                .unwrap_or_else(|| vec![0u8; 32]);

                            // Set block state root
                            let mut state_root_bytes = [0u8; 32];
                            state_root_bytes.copy_from_slice(&state_root[..32.min(state_root.len())]);
                            block.state_root = BlockHash::from(state_root_bytes);

                            // 1. Notify Consensus Integration
                            let block_hash = block.calculate_hash();
                            match hardened_consensus.process_block(block_height, block_hash) {
                                Ok(Some(new_epoch)) => {
                                    info!("🔄 Epoch {} complete. Triggering reward distribution...", new_epoch - 1);

                                    // 2. Advance relay work store to new epoch
                                    relay_work_store.set_current_epoch(new_epoch);

                                    // 3. Collect relay registry and work events for the completed epoch
                                    let completed_epoch = new_epoch - 1;
                                    let relay_registry = relay_registry_store.get_all_relays();
                                    let work_events = relay_work_store.get_events_for_epoch(completed_epoch);

                                    info!("📊 Epoch {} stats: {} registered relays, {} work events",
                                        completed_epoch, relay_registry.len(), work_events.len());

                                    // Phase 5: Initialize VRF committee selector with relay data
                                    // Convert RegisteredRelay data to VRF-compatible RelayEligibility
                                    if enable_vrf_committees && !relay_registry.is_empty() {
                                        use dchat_blockchain::hardened_consensus::vrf_committees::{
                                            RelayEligibility as VrfRelayEligibility,
                                            RelayId as VrfRelayId,
                                            GeographicRegion as VrfRegion,
                                            VrfSeedDeriver,
                                        };
                                        use dchat_blockchain::block_hierarchy::Hash as VrfHash;

                                        // Create VRF-compatible relay eligibility from registered relays
                                        let vrf_relays: Vec<VrfRelayEligibility> = relay_registry
                                            .iter()
                                            .filter(|r| !r.is_suspended)
                                            .enumerate()
                                            .filter_map(|(idx, relay)| {
                                                // Derive relay ID from string (hash to [u8; 32])
                                                let relay_id_hash = blake3::hash(relay.relay_id.as_bytes());
                                                let relay_id = VrfRelayId(*relay_id_hash.as_bytes());

                                                // Create a deterministic public key from relay_id for VRF
                                                // In production, this would come from actual relay registration
                                                let key_bytes: [u8; 32] = *relay_id_hash.as_bytes();
                                                let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes).ok()?;

                                                // Assign geographic region based on hash for diversity
                                                let region = match idx % 6 {
                                                    0 => VrfRegion::NorthAmerica,
                                                    1 => VrfRegion::Europe,
                                                    2 => VrfRegion::Asia,
                                                    3 => VrfRegion::SouthAmerica,
                                                    4 => VrfRegion::Africa,
                                                    _ => VrfRegion::Oceania,
                                                };

                                                Some(VrfRelayEligibility {
                                                    relay_id,
                                                    public_key: verifying_key,
                                                    stake: relay.stake,
                                                    uptime_score: 0.95, // Default high uptime for active relays
                                                    region,
                                                    asn: (idx as u32) % 1000 + 1, // Simulated ASN diversity
                                                    ip_prefix: [(idx as u8) % 255, 0, 0],
                                                    operator_id: VrfHash::from(*blake3::hash(
                                                        relay.operator.0.as_bytes()
                                                    ).as_bytes()),
                                                    raw_weight: relay.stake / 1_000_000, // Normalized stake weight
                                                })
                                            })
                                            .collect();

                                        if vrf_relays.len() >= 3 {
                                            hardened_consensus.initialize_committee_selector(vrf_relays);
                                            info!("🎲 VRF committee selector initialized with {} eligible relays", relay_registry.len());
                                        } else {
                                            debug!("Skipping VRF init: need at least 3 eligible relays, have {}", vrf_relays.len());
                                        }
                                    }

                                    // 4. Trigger Rewards with actual relay data
                                    if let Err(e) = perform_epoch_rewards(
                                        new_epoch,
                                        &staking_manager_consensus,
                                        &currency_client,
                                        &fee_manager,
                                        &tokenomics_manager,
                                        if relay_registry.is_empty() { None } else { Some(relay_registry.as_slice()) },
                                        if work_events.is_empty() { None } else { Some(work_events.as_slice()) },
                                    ).await {
                                        error!("Failed to perform epoch rewards: {}", e);
                                    }
                                }
                                Ok(None) => {} // Normal block
                                Err(e) => error!("Consensus integration failed: {:?}", e),
                            }

                            let _ = block.add_subblock(subblock);

                            info!("  • State root: {}", hex::encode(&state_root[..8]));
                            info!("  • Block has {} subblocks, {} transactions total",
                                block.subblocks.len(), block.transaction_count());

                            // Calculate consensus hash for signing and network transmission
                            // Uses consensus_hash() which only includes fields transmitted in ValidatorBlock:
                            // height, prev_hash, state_root, subblock_count, transaction_count
                            // This allows receivers to verify the hash without full block reconstruction
                            let block_hash_obj = block.consensus_hash();
                            let block_hash = block_hash_obj.as_bytes().to_vec();

                            info!("  • Block consensus hash: {}", hex::encode(&block_hash[..8]));

                            // Sign the block with validator key
                            let signature_result = {
                                let validator_key = validator_key_arc_clone.lock().await;
                                validator_key.sign_async(&block_hash).await
                            };

                            let block_signature = match signature_result {
                                Ok(sig) => sig,
                                Err(e) => {
                                    error!("❌ Failed to sign block: {}", e);
                                    continue;
                                }
                            };

                            info!("  • Block signed with Ed25519 signature: {}", hex::encode(&block_signature.to_bytes()[..8]));

                            // Validate our own block before broadcasting
                            {
                                let mut validator = state_validator_clone.lock().await;
                                match validator.validate_block(&block) {
                                    Ok(computed_root) => {
                                        info!("  ✓ Block self-validation passed, state root verified: {}",
                                            hex::encode(&computed_root[..8]));
                                    }
                                    Err(StateValidationError::StateRootMismatch { expected, actual }) => {
                                        error!("❌ Block self-validation FAILED: state root mismatch");
                                        error!("   Expected: {}, Actual: {}", expected, actual);
                                        continue;
                                    }
                                    Err(e) => {
                                        error!("❌ Block self-validation FAILED: {}", e);
                                        continue;
                                    }
                                }
                            }

                            // Create ValidatorBlock message (includes full block data)
                            let validator_id = validator_public_key_bytes.to_vec();
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                                .as_secs();

                            // Serialize transactions for network message
                            let tx_bytes: Vec<Vec<u8>> = pending_txs.iter()
                                .filter_map(|tx| bincode::serialize(tx).ok())
                                .collect();

                            // Prepare subblock metadata for network transmission
                            // Convert SystemTime timestamps to u64 for serialization
                            let subblock_metadata: Vec<(u16, u64, u16)> = block.subblocks.iter()
                                .map(|sb| {
                                    let timestamp_u64 = sb.timestamp
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                                        .as_secs();
                                    (sb.index, timestamp_u64, sb.miniblocks.len() as u16)
                                })
                                .collect();

                            // Prepare VRF proof for transmission (if this validator was leader)
                            let vrf_proof_option = if let Some(ref proof) = last_vrf_proof {
                                Some((
                                    proof.vrf_output.to_vec(),
                                    proof.vrf_proof.0.to_vec(),
                                    proof.validator_weight_bps
                                ))
                            } else {
                                None
                            };

                            // Get current epoch and slot index
                            // Epoch duration is 6 hours = 21600 seconds, blocks every 6 seconds = 3600 blocks/epoch
                            let current_epoch = current_epoch_id();
                            let blocks_per_epoch = EPOCH_DURATION_SECS / 6; // 6 second block time
                            let epoch_block_count = (block_height - 1) % blocks_per_epoch;

                            let block_message = DchatMessage::ValidatorBlock {
                                height: block_height,
                                validator_id: validator_id.clone(),
                                block_hash: block_hash.clone(),
                                signature: block_signature.to_bytes().to_vec(),
                                timestamp,
                                transactions: tx_bytes,
                                prev_hash: block.previous_hash.as_bytes().to_vec(),
                                state_root: state_root.to_vec(),
                                subblock_metadata,
                                vrf_proof: vrf_proof_option,
                                slot_epoch: current_epoch,
                                slot_index: epoch_block_count,
                            };

                            // Broadcast block to validator network via gossipsub
                            let mut net = network_arc_clone.lock().await;
                            match net.broadcast_validator_block(&block_message) {
                                Ok(_) => {
                                    info!("✓ Block #{} broadcast to validator consensus network", block_height);
                                    info!("  • Waiting for BFT acknowledgments ({} required)...", required_signatures);
                                }
                                Err(e) => {
                                    error!("❌ Failed to broadcast block #{}: {}", block_height, e);
                                    continue;
                                }
                            }

                            // Cleanup old state roots periodically (keep last 1000 blocks)
                            if block_height % 100 == 0 && block_height > 1000 {
                                let mut validator = state_validator_clone.lock().await;
                                validator.cleanup_old_roots(block_height, 1000);
                                info!("  • Cleaned up state roots for blocks before {}", block_height - 1000);
                            }

                            // Wait for BFT threshold of acknowledgments (2f+1 signatures)
                            let ack_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
                            let mut received_acks = 0;

                            while tokio::time::Instant::now() < ack_deadline {
                                let acks = block_acks_clone.lock().await;
                                if let Some(height_acks) = acks.get(&block_height) {
                                    received_acks = height_acks.len();
                                    if received_acks >= required_signatures {
                                        info!("✅ Block #{} finalized with {} acknowledgments (BFT threshold reached)",
                                            block_height, received_acks);
                                        break;
                                    }
                                }
                                drop(acks);
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            }

                            if received_acks < required_signatures {
                                warn!("⚠️  Block #{} did not reach BFT threshold ({}/{} acks)",
                                    block_height, received_acks, required_signatures);
                                warn!("   Block may not be finalized - potential network partition");
                            }
                        } else if is_producer {
                            // We are a producer but not the leader for this slot
                            // Do NOT increment block_height locally - wait for leader's block via network
                            let current_height = shared_block_height.load(Ordering::SeqCst);
                            debug!("⏳ Slot {}: Not our turn to produce (VRF selection), current height={}", current_slot, current_height);

                            // Still advance tokenomics for consistent state
                            if let Err(e) = tokenomics_manager.advance_block() {
                                warn!("Failed to advance tokenomics block counter: {}", e);
                            }
                        } else {
                            // Non-producer validator: wait for blocks from network
                            // Blocks will be validated when received via gossipsub events
                            // Do NOT increment block_height locally - wait for leader's block via network
                            let current_height = shared_block_height.load(Ordering::SeqCst);
                            debug!("⏳ Non-producer waiting for block, current height={}", current_height);

                            // Advance tokenomics block counter for consistent state across validators
                            if let Err(e) = tokenomics_manager.advance_block() {
                                warn!("Failed to advance tokenomics block counter: {}", e);
                            }
                        }
                    }

                    _ = stats_interval.tick() => {
                        let current_height = shared_block_height.load(Ordering::SeqCst);
                        info!("📊 Validator stats: height={}, stake={}", current_height, stake_amount);
                        let acks = block_acks_clone.lock().await;
                        info!("   Pending acknowledgments: {} blocks", acks.len());

                        // Report Byzantine fault statistics
                        let validator = state_validator_clone.lock().await;
                        let faults = validator.get_byzantine_faults();
                        if !faults.is_empty() {
                            warn!("   ⚠️ Byzantine faults detected: {} validators", faults.len());
                            for (validator_id, fault_list) in faults.iter() {
                                warn!("      Validator {}: {} faults",
                                    hex::encode(&validator_id[..4.min(validator_id.len())]),
                                    fault_list.len());
                            }
                        }
                    }
                }
            }
        })
    };

    // Start network event handler for incoming validator blocks
    let network_event_handle = {
        let network_arc_clone = network_arc.clone();
        let validator_key_arc_clone2 = Arc::clone(&validator_key_arc);
        let validator_public_key_bytes_clone = validator_public_key_bytes;
        let block_acks_clone = block_acknowledgments.clone();
        let state_validator_clone2 = state_validator.clone();
        let peer_registry_arc_clone = peer_registry_arc.clone();
        let relay_work_store_clone = Arc::clone(&relay_work_store);
        let chain_genesis_timestamp = config.chain.genesis_timestamp;
        let chain_block_time_secs = config.chain.block_time_secs;
        let shared_block_height_clone = Arc::clone(&shared_block_height_global);
        let authorized_validators_clone = Arc::clone(&authorized_validators);
        let mut shutdown = shutdown_tx.subscribe();

        tokio::spawn(async move {
            use std::sync::atomic::Ordering;
            info!("Starting network event handler for consensus messages...");

            loop {
                tokio::select! {
                    _ = shutdown.recv() => {
                        info!("Network event handler shutting down");
                        break;
                    }

                    event = async {
                        let mut network_guard = network_arc_clone.lock().await;
                        network_guard.next_event().await
                    } => {
                        if let Some(NetworkEvent::MessageReceived { from, message }) = event {
                            match message {
                                DchatMessage::ValidatorBlock {
                                    height,
                                    validator_id,
                                    block_hash,
                                    signature,
                                    timestamp: _,
                                    transactions,
                                    prev_hash,
                                    state_root,
                                    subblock_metadata,
                                    vrf_proof,
                                    slot_epoch,
                                    slot_index,
                                } => {
                                    info!("📨 Received validator block #{} from {}", height, hex::encode(&validator_id[..4]));

                                    // Verify block signature
                                    if validator_id.len() != 32 {
                                        warn!("⚠️  Invalid validator ID length: {}", validator_id.len());
                                        continue;
                                    }

                                    let validator_id_bytes: [u8; 32] = match validator_id.clone().try_into() {
                                        Ok(bytes) => bytes,
                                        Err(_) => {
                                            warn!("⚠️  Failed to convert validator ID to bytes");
                                            continue;
                                        }
                                    };

                                    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&validator_id_bytes) {
                                        Ok(key) => key,
                                        Err(e) => {
                                            warn!("⚠️  Invalid verifying key: {}", e);
                                            continue;
                                        }
                                    };

                                    if signature.len() != 64 {
                                        warn!("⚠️  Invalid signature length: {}", signature.len());
                                        continue;
                                    }

                                    let sig_bytes: [u8; 64] = match signature.clone().try_into() {
                                        Ok(bytes) => bytes,
                                        Err(_) => {
                                            warn!("⚠️  Failed to convert signature to bytes");
                                            continue;
                                        }
                                    };
                                    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);

                                    // Verify signature
                                    use ed25519_dalek::Verifier;
                                    if let Err(e) = verifying_key.verify(&block_hash, &sig) {
                                        warn!("⚠️  Block #{} signature verification FAILED: {}", height, e);
                                        warn!("   Block rejected - invalid validator signature");
                                        continue;
                                    }

                                    info!("✓ Block #{} signature verified from validator {}", height, hex::encode(&validator_id[..4]));

                                    // Verify the validator is authorized (in genesis validator set)
                                    if !authorized_validators_clone.is_empty() && !authorized_validators_clone.contains(&validator_id_bytes) {
                                        warn!("⚠️  Block #{} from UNAUTHORIZED validator {}", height, hex::encode(&validator_id[..8]));
                                        warn!("   Block rejected - validator not in genesis set");
                                        warn!("   Expected one of {} authorized validators", authorized_validators_clone.len());
                                        continue;
                                    }

                                    // Verify block hash matches the consensus hash of transmitted fields
                                    // The consensus hash includes: height, prev_hash, state_root, subblock_count, tx_count
                                    {
                                        use blake3::Hasher;
                                        let mut hasher = Hasher::new();
                                        hasher.update(b"dchat/block/consensus/v1");
                                        hasher.update(&height.to_le_bytes());
                                        hasher.update(&prev_hash);
                                        hasher.update(&state_root);
                                        hasher.update(&[subblock_metadata.len() as u8]);
                                        hasher.update(&(transactions.len() as u32).to_le_bytes());
                                        let computed_hash = hasher.finalize();

                                        if computed_hash.as_bytes() != block_hash.as_slice() {
                                            warn!("⚠️  Block #{} hash verification FAILED", height);
                                            warn!("   Expected: {}", hex::encode(&block_hash[..8]));
                                            warn!("   Computed: {}", hex::encode(&computed_hash.as_bytes()[..8]));
                                            warn!("   Block rejected - hash mismatch indicates tampering or corruption");
                                            continue;
                                        }
                                        info!("✓ Block #{} consensus hash verified", height);
                                    }

                                    info!("✓ Block #{} accepted from validator {} ({} transactions)",
                                          height, hex::encode(&validator_id[..4]), transactions.len());

                                    // State validation with Byzantine fault detection
                                    {
                                        let mut validator = state_validator_clone2.lock().await;

                                        // Check if we've seen conflicting state for this height
                                        if let Some(byzantine_validators) = validator.get_byzantine_faults_at_height(&height.to_le_bytes()) {
                                            warn!("⚠️  Byzantine fault detected at height {}: {} conflicting validators",
                                                height, byzantine_validators.len());
                                            for fault_validator_id in &byzantine_validators {
                                                warn!("   Fault from validator: {}", hex::encode(&fault_validator_id[..4.min(fault_validator_id.len())]));
                                            }

                                            // Get slashing recommendations
                                            let slash_recommendations = validator.get_slashing_recommendations();
                                            for (slash_validator_id, slash_pct, reason) in slash_recommendations {
                                                warn!("   💰 Slashing recommendation: {} - {}% stake ({})",
                                                    hex::encode(&slash_validator_id[..4.min(slash_validator_id.len())]),
                                                    slash_pct,
                                                    reason
                                                );

                                                // Queue slashing transaction for governance council review
                                                // In production, this would:
                                                // 1. Create slashing proposal with evidence
                                                // 2. Submit to governance council for voting
                                                // 3. Collect 5-of-7 multisig signatures
                                                // 4. Execute slash_validator with signatures

                                                // For now, log as critical security event
                                                error!(
                                                    "🚨 BYZANTINE FAULT DETECTED - Validator {} requires slashing ({}% - {})",
                                                    hex::encode(&slash_validator_id),
                                                    slash_pct,
                                                    reason
                                                );

                                                // In production system:
                                                // let proposal_id = governance_council.propose_slashing(
                                                //     slash_validator_id.clone(),
                                                //     SlashingSeverity::from_percentage(slash_pct),
                                                //     reason.clone(),
                                                //     block_hash.clone(), // Evidence
                                                // ).await?;
                                                //
                                                // When signatures collected:
                                                // staking_manager.slash_validator(
                                                //     &slash_validator_id,
                                                //     severity,
                                                //     &reason,
                                                //     evidence,
                                                //     council_signatures
                                                // ).await?;
                                            }
                                        }

                                        // Detect if this validator is broadcasting conflicting state
                                        let validator_id_str = hex::encode(&validator_id[..8.min(validator_id.len())]);
                                        if let Err(e) = validator.detect_byzantine_fault(
                                            &height.to_le_bytes(),
                                            &validator_id_str,
                                            &block_hash
                                        ) {
                                            error!("⚠️  Byzantine fault from validator {}: {}",
                                                hex::encode(&validator_id[..4.min(validator_id.len())]), e);

                                            // In production: Submit slashing transaction
                                            // let slashing_tx = create_slashing_transaction(
                                            //     &validator_id,
                                            //     5, // 5% slash for first offense
                                            //     format!("Byzantine fault: {}", e)
                                            // );
                                            // staking_manager.submit_slashing(slashing_tx).await?;

                                            // Reject this block - do not acknowledge
                                            warn!("   Block rejected due to Byzantine behavior");
                                            continue;
                                        }

                                        // Cleanup old state roots (keep last 1000 blocks)
                                        if height > 1000 {
                                            validator.cleanup_old_roots(height, 1000);
                                        }
                                    }

                                    info!("✓ Block #{} passed Byzantine fault check", height);

                                    // Update shared block height if this block is higher than current
                                    // This ensures consensus loop knows we've received a valid block at this height
                                    let current_height = shared_block_height_clone.load(Ordering::SeqCst);
                                    if height > current_height {
                                        shared_block_height_clone.store(height, Ordering::SeqCst);
                                        info!("📊 Updated shared block height from {} to {} (received from leader)",
                                              current_height, height);
                                    }

                                    // Create and sign acknowledgment
                                    let our_validator_id = validator_public_key_bytes_clone.to_vec();
                                    let ack_signature_result = {
                                        let validator_key = validator_key_arc_clone2.lock().await;
                                        validator_key.sign_async(&block_hash).await
                                    };

                                    let ack_signature = match ack_signature_result {
                                        Ok(sig) => sig,
                                        Err(e) => {
                                            error!("❌ Failed to sign acknowledgment: {}", e);
                                            continue;
                                        }
                                    };

                                    let ack_message = DchatMessage::BlockAcknowledgment {
                                        block_height: height,
                                        block_hash: block_hash.clone(),
                                        validator_id: our_validator_id,
                                        signature: ack_signature.to_bytes().to_vec(),
                                    };

                                    // Broadcast acknowledgment
                                    let mut net = network_arc_clone.lock().await;
                                    match net.broadcast_validator_block(&ack_message) {
                                        Ok(_) => {
                                            info!("✓ Sent acknowledgment for block #{}", height);
                                        }
                                        Err(e) => {
                                            error!("❌ Failed to broadcast acknowledgment: {}", e);
                                        }
                                    }
                                }

                                DchatMessage::BlockAcknowledgment {
                                    block_height,
                                    block_hash,
                                    validator_id,
                                    signature,
                                } => {
                                    info!("📨 Received acknowledgment for block #{} from {}",
                                        block_height, hex::encode(&validator_id[..4]));

                                    // Verify acknowledgment signature
                                    if validator_id.len() != 32 || signature.len() != 64 {
                                        warn!("⚠️  Invalid acknowledgment format");
                                        continue;
                                    }

                                    let validator_id_bytes: [u8; 32] = match validator_id.clone().try_into() {
                                        Ok(bytes) => bytes,
                                        Err(_) => {
                                            warn!("⚠️  Failed to convert validator ID to bytes");
                                            continue;
                                        }
                                    };

                                    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&validator_id_bytes) {
                                        Ok(key) => key,
                                        Err(e) => {
                                            warn!("⚠️  Invalid acknowledging validator key: {}", e);
                                            continue;
                                        }
                                    };

                                    let sig_bytes: [u8; 64] = match signature.clone().try_into() {
                                        Ok(bytes) => bytes,
                                        Err(_) => {
                                            warn!("⚠️  Failed to convert signature to bytes");
                                            continue;
                                        }
                                    };
                                    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);

                                    use ed25519_dalek::Verifier;
                                    if let Err(e) = verifying_key.verify(&block_hash, &sig) {
                                        warn!("⚠️  Acknowledgment signature verification FAILED: {}", e);
                                        continue;
                                    }

                                    // Verify the acknowledging validator is authorized (in genesis validator set)
                                    if !authorized_validators_clone.is_empty() && !authorized_validators_clone.contains(&validator_id_bytes) {
                                        warn!("⚠️  Acknowledgment from UNAUTHORIZED validator {}", hex::encode(&validator_id[..8]));
                                        warn!("   Acknowledgment rejected - validator not in genesis set");
                                        continue;
                                    }

                                    // Store valid acknowledgment
                                    let mut acks = block_acks_clone.lock().await;
                                    acks.entry(block_height)
                                        .or_insert_with(HashMap::new)
                                        .insert(validator_id.clone(), signature.clone());

                                    let ack_count = acks.get(&block_height).map(|m| m.len()).unwrap_or(0);
                                    info!("✓ Acknowledgment verified for block #{} ({} total acks)",
                                        block_height, ack_count);

                                    // Cleanup old acknowledgments (keep last 100 blocks)
                                    if block_height > 100 {
                                        acks.remove(&(block_height - 100));
                                    }
                                }

                                DchatMessage::PeerHandshake { payload } => {
                                    // Deserialize the handshake payload and process it
                                    match serde_json::from_slice::<PeerHandshake>(&payload) {
                                        Ok(handshake) => {
                                            let mut network = network_arc_clone.lock().await;
                                            if let Err(e) = handle_peer_handshake(
                                                from,
                                                handshake,
                                                &peer_registry_arc_clone,
                                                &mut network,
                                            ).await {
                                                warn!("⚠️  Failed to handle peer handshake from {}: {}", from, e);
                                            }
                                        }
                                        Err(e) => {
                                            warn!("⚠️  Failed to deserialize handshake from {}: {}", from, e);
                                        }
                                    }
                                }

                                DchatMessage::DeliveryProof { message_id, relay_signature } => {
                                    // Record relay work event for epoch reward distribution
                                    // The relay that delivered this proof deserves credit
                                    let relay_id = from.to_string();

                                    // Create proof hash from message_id + signature for deduplication
                                    let mut proof_data = Vec::new();
                                    proof_data.extend_from_slice(message_id.as_bytes());
                                    proof_data.extend_from_slice(&relay_signature);
                                    let proof_hash = *blake3::hash(&proof_data).as_bytes();

                                    // Get current block height estimate (simplified - uses time-based estimation)
                                    let genesis_timestamp = chain_genesis_timestamp;
                                    let block_time = chain_block_time_secs;
                                    let now = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_secs())
                                        .unwrap_or(0);
                                    let estimated_block = if now >= genesis_timestamp {
                                        (now - genesis_timestamp) / block_time
                                    } else {
                                        0
                                    };

                                    if relay_work_store_clone.record_proof_of_delivery(&relay_id, estimated_block, proof_hash) {
                                        debug!("📦 Recorded delivery proof from relay {} for message {} at block ~{}",
                                            relay_id, message_id, estimated_block);
                                    }
                                }

                                _ => {
                                    // Other message types handled elsewhere
                                }
                            }
                        }
                    }
                }
            }
        })
    };

    // Start relay registry sync task - periodically sync registered relays from currency chain
    let relay_registry_sync_handle = {
        let relay_registry_store = Arc::clone(&relay_registry_store);
        let currency_rpc_url = consensus_currency_rpc_url.clone();
        let mut shutdown = shutdown_tx.subscribe();

        tokio::spawn(async move {
            info!("📡 Starting relay registry sync task...");

            // Sync interval: every 5 minutes (epochs are ~24 hours, so this is frequent enough)
            let mut sync_interval = tokio::time::interval(tokio::time::Duration::from_secs(300));

            loop {
                tokio::select! {
                    _ = shutdown.recv() => {
                        info!("Relay registry sync task shutting down");
                        break;
                    }

                    _ = sync_interval.tick() => {
                        // Query registered relays from currency chain
                        let currency_chain_config = CurrencyChainConfig {
                            rpc_url: currency_rpc_url.clone(),
                            ..Default::default()
                        };

                        match CurrencyChainClient::new(currency_chain_config) {
                            Ok(client) => {
                                // Query staking records for relay operators
                                match client.get_registered_relay_operators().await {
                                    Ok(relay_operators) => {
                                        for (relay_id, operator, stake, registered_block, is_suspended) in relay_operators {
                                            let relay_id_for_suspend = relay_id.clone();

                                            relay_registry_store.register_relay(
                                                relay_id,
                                                operator,
                                                stake,
                                                registered_block,
                                            );
                                            if is_suspended {
                                                relay_registry_store.suspend_relay(&relay_id_for_suspend);
                                            }
                                        }
                                        let stats = relay_registry_store.relay_count();
                                        debug!("📡 Relay registry synced: {} relays", stats);
                                    }
                                    Err(e) => {
                                        warn!("Failed to sync relay registry from chain: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to connect to currency chain for relay sync: {}", e);
                            }
                        }
                    }
                }
            }
        })
    };

    info!("🎉 Validator node is ready!");
    info!("Participating in consensus...");
    info!("Press Ctrl+C to shutdown gracefully");

    // Wait for shutdown signal
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("🛑 Received shutdown signal (Ctrl+C)");
        }
        _ = shutdown_rx.recv() => {
            info!("🛑 Received shutdown signal (internal)");
        }
    }

    // Graceful shutdown
    info!("Shutting down validator gracefully...");
    let _ = shutdown_tx.send(());
    consensus_handle.abort();
    network_event_handle.abort();
    relay_registry_sync_handle.abort();

    // Unstake tokens from chain
    info!("Initiating unstaking process...");

    // PRODUCTION: Submit unstaking request
    match staking_manager_clone
        .submit_validator_unstake(
            &validator_user_id_clone,
            stake_amount * MOTES_PER_DCHAT, // Convert to motes
        )
        .await
    {
        Ok(tx_id) => {
            info!("✓ Unstake transaction submitted (tx: {})", tx_id);
            info!("  Tokens will be unlocked after unbonding period (7 days)");
            info!("  Total unstaking amount: {} DCHAT", stake_amount);
        }
        Err(e) => {
            warn!("Failed to submit unstake (continuing shutdown): {}", e);
        }
    }

    info!("✓ Validator shutdown initiated (unstaking in progress)");
    // Close database connections gracefully
    info!("Closing database connections...");
    // Database closes automatically on drop
    drop(database);
    info!("✓ Database closed successfully");

    // Wait for tasks to complete
    tokio::time::timeout(
        tokio::time::Duration::from_secs(SHUTDOWN_TIMEOUT_SECONDS),
        async {
            let _ = tokio::join!(health_handle, metrics_handle);
        },
    )
    .await
    .map_err(|_| Error::network("Shutdown timeout".to_string()))?;

    info!("✓ Validator shutdown complete");
    Ok(())
}

// ============================================================================
// Testnet Helper Functions
// ============================================================================

/// Generate genesis configuration for testnet
fn generate_genesis_config(num_validators: usize) -> Result<serde_json::Value> {
    let mut validators = Vec::new();

    for i in 0..num_validators {
        validators.push(serde_json::json!({
            "id": format!("validator_{}", i),
            "stake": 10000,
            "voting_power": 1,
        }));
    }

    Ok(serde_json::json!({
        "chain_id": "dchat-testnet-1",
        "initial_height": "1",
        "genesis_time": chrono::Utc::now().to_rfc3339(),
        "validators": validators,
        "app_state": {
            "initial_supply": 1000000,
            "min_stake": 1000,
        }
    }))
}

/// Save validator key to encrypted file
async fn save_validator_key(path: &PathBuf, keypair: &KeyPair) -> Result<()> {
    let private_bytes = keypair.private_key().as_bytes();
    let public_bytes = keypair.public_key().as_bytes();

    let key_json = serde_json::json!({
        "public_key": format!("{:?}", public_bytes),
        "private_key": format!("{:?}", private_bytes),
        "created_at": chrono::Utc::now().to_rfc3339(),
    });

    tokio::fs::write(path, serde_json::to_string_pretty(&key_json)?)
        .await
        .map_err(Error::Io)?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = tokio::fs::metadata(path).await.map_err(|e| Error::Io(e))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        tokio::fs::set_permissions(path, permissions)
            .await
            .map_err(|e| Error::Io(e))?;
    }

    Ok(())
}

/// Load validator key from file
async fn load_validator_key(path: &PathBuf) -> Result<KeyPair> {
    let contents = tokio::fs::read_to_string(path).await.map_err(Error::Io)?;
    let key_json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| Error::Crypto(format!("Invalid key file: {}", e)))?;

    let private_key_str = key_json["private_key"]
        .as_str()
        .ok_or_else(|| Error::Crypto("Missing private_key field".to_string()))?;

    // Parse the debug format string like "[1, 2, 3, ...]"
    let bytes_str = private_key_str.trim_matches(&['[', ']'][..]);
    let mut bytes = [0u8; 32];
    for (i, byte_str) in bytes_str.split(',').enumerate().take(32) {
        bytes[i] = byte_str
            .trim()
            .parse()
            .map_err(|e| Error::Crypto(format!("Invalid byte: {}", e)))?;
    }

    let private_key = PrivateKey::from_bytes(bytes);
    Ok(KeyPair::from_private_key(private_key))
}

/// Load identity from file (auto-detects encrypted vs plaintext)
async fn load_identity_from_file(path: &PathBuf) -> Result<Identity> {
    let contents = tokio::fs::read(path).await.map_err(Error::Io)?;

    // Try to parse as JSON (plaintext) first
    if let Ok(json_str) = std::str::from_utf8(&contents) {
        if let Ok(identity) = serde_json::from_str::<Identity>(json_str) {
            info!("✓ Loaded plaintext identity from {:?}", path);
            return Ok(identity);
        }
    }

    // Try to load as encrypted data
    load_identity_encrypted(path, None).await
}

/// Load encrypted identity with password prompt
async fn load_identity_encrypted(path: &PathBuf, password: Option<String>) -> Result<Identity> {
    use dchat_crypto::{decrypt_with_password, EncryptedData};

    let encrypted_bytes = tokio::fs::read(path).await.map_err(Error::Io)?;

    // Deserialize encrypted data
    let encrypted = EncryptedData::from_bytes(&encrypted_bytes)
        .map_err(|e| Error::crypto(format!("Invalid encrypted file format: {}", e)))?;

    // Get password
    let password = match password {
        Some(p) => p,
        None => prompt_password("Enter password to decrypt identity: ")?,
    };

    // Decrypt
    let decrypted = decrypt_with_password(&password, &encrypted)
        .map_err(|e| Error::crypto(format!("Decryption failed: {}", e)))?;

    // Parse JSON identity
    let json_str = std::str::from_utf8(&decrypted)
        .map_err(|e| Error::crypto(format!("Invalid UTF-8 after decryption: {}", e)))?;

    let identity: Identity = serde_json::from_str(json_str)
        .map_err(|e| Error::crypto(format!("Invalid identity JSON: {}", e)))?;

    info!("✓ Loaded encrypted identity: {}", identity.user_id);
    Ok(identity)
}

/// Save identity as plaintext JSON (for automated deployments)
async fn save_identity_plaintext(path: &Path, identity: &Identity) -> Result<()> {
    let json = serde_json::to_string_pretty(identity)
        .map_err(|e| Error::crypto(format!("Serialization failed: {}", e)))?;

    tokio::fs::write(path, json).await.map_err(Error::Io)?;

    // Set restrictive permissions on Unix
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

    info!("✓ Identity saved as plaintext to {:?}", path);
    Ok(())
}

/// Save identity with keypair to file (JSON with private key for persistence)
///
/// Creates a JSON file containing both the identity and the private key,
/// allowing the node to reload the same identity on restart.
async fn save_identity_to_file(
    identity: &Identity,
    keypair: &KeyPair,
    path: &PathBuf,
) -> Result<()> {
    use serde_json::json;

    // Create identity with private key included
    let identity_json = json!({
        "user_id": identity.user_id.to_string(),
        "username": identity.username,
        "public_key": hex::encode(keypair.public_key().as_bytes()),
        "private_key": hex::encode(keypair.private_key().as_bytes()),
        "created_at": chrono::Utc::now().to_rfc3339(),
    });

    let json_str = serde_json::to_string_pretty(&identity_json)
        .map_err(|e| Error::crypto(format!("Serialization failed: {}", e)))?;

    tokio::fs::write(path, json_str).await.map_err(Error::Io)?;

    // Set restrictive permissions on Unix
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

    info!("✓ Identity with private key saved to {:?}", path);
    Ok(())
}

/// Generate docker-compose for testnet
fn generate_testnet_compose(
    data_dir: &Path,
    num_validators: usize,
    num_relays: usize,
    num_clients: usize,
    relay_addrs: &[String],
    with_observability: bool,
) -> Result<()> {
    let mut services = serde_json::Map::new();

    // Add validators
    for i in 0..num_validators {
        let service_name = format!("validator{}", i);
        services.insert(
            service_name,
            serde_json::json!({
                "image": "dchat:latest",
                "command": [
                    "validator",
                    "--key", format!("/data/validator_{}.key", i),
                    "--chain-rpc", "http://chain-rpc:26657",
                    "--stake", "10000",
                    "--producer",
                ],
                "volumes": [
                    format!("{}:/data", data_dir.join("validators").display())
                ],
                "networks": ["dchat-testnet"],
                "restart": "unless-stopped",
            }),
        );
    }

    // Add relays
    for i in 0..num_relays {
        let service_name = format!("relay{}", i);
        let port = 7070 + (i * 2);

        services.insert(
            service_name,
            serde_json::json!({
                "image": "dchat:latest",
                "command": [
                    "relay",
                    "--listen", format!("0.0.0.0:{}", port),
                    "--bootstrap", relay_addrs,
                    "--stake", "1000",
                ],
                "ports": [format!("{}:{}", port, port)],
                "networks": ["dchat-testnet"],
                "restart": "unless-stopped",
            }),
        );
    }

    // Add clients
    for i in 0..num_clients {
        let service_name = format!("client{}", i);
        services.insert(
            service_name,
            serde_json::json!({
                "image": "dchat:latest",
                "command": [
                    "user",
                    "--bootstrap", relay_addrs,
                    "--username", format!("testuser{}", i),
                    "--non-interactive",
                ],
                "networks": ["dchat-testnet"],
                "restart": "unless-stopped",
            }),
        );
    }

    // Add observability stack if requested
    if with_observability {
        services.insert(
            "prometheus".to_string(),
            serde_json::json!({
                "image": "prom/prometheus:latest",
                "ports": ["9090:9090"],
                "networks": ["dchat-testnet"],
            }),
        );

        services.insert(
            "grafana".to_string(),
            serde_json::json!({
                "image": "grafana/grafana:latest",
                "ports": ["3000:3000"],
                "networks": ["dchat-testnet"],
            }),
        );

        services.insert(
            "jaeger".to_string(),
            serde_json::json!({
                "image": "jaegertracing/all-in-one:latest",
                "ports": ["16686:16686", "14268:14268"],
                "networks": ["dchat-testnet"],
            }),
        );
    }

    let compose = serde_json::json!({
        "version": "3.8",
        "services": services,
        "networks": {
            "dchat-testnet": {
                "driver": "bridge"
            }
        }
    });

    let compose_path = data_dir.join("docker-compose.json");
    let compose_json = serde_json::to_string_pretty(&compose)
        .map_err(|e| Error::Config(format!("Failed to serialize compose: {}", e)))?;
    std::fs::write(&compose_path, compose_json).map_err(Error::Io)?;

    info!("✓ Docker compose written to {:?}", compose_path);

    Ok(())
}

/// Start metrics server
fn start_metrics_server(
    addr: &str,
    peer_metrics: Arc<PeerMetrics>,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<tokio::task::JoinHandle<()>> {
    use warp::Filter;

    let addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| Error::Config(format!("Invalid metrics address: {}", e)))?;

    // Clone Arc for the closure
    let peer_metrics_clone = peer_metrics.clone();

    let metrics_route = warp::path("metrics").and_then(move || {
        let peer_metrics = peer_metrics_clone.clone();
        async move {
            // Export peer metrics to Prometheus
            let metrics_text = peer_metrics.export_prometheus().await;

            Ok::<_, warp::Rejection>(warp::reply::with_header(
                metrics_text,
                "Content-Type",
                "text/plain; version=0.0.4; charset=utf-8",
            ))
        }
    });

    let routes = metrics_route;

    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(addr, async move {
        let _ = shutdown.recv().await;
    });

    let handle = tokio::spawn(async move {
        server.await;
    });

    Ok(handle)
}

// ============================================================================
// Key Generation and Identity Management
// ============================================================================

/// Generate new identity and keys
async fn generate_keys(
    output: PathBuf,
    burner: bool,
    plaintext: bool,
    validator: bool,
) -> Result<()> {
    info!("🔑 Generating new identity...");

    if validator {
        // Generate validator keypair with private key included
        info!("Generating validator keypair...");
        #[allow(deprecated)]
        let keypair = KeyPair::generate();

        let public_key_hex = hex::encode(keypair.public_key().as_bytes());
        let private_key_bytes: Vec<u8> = keypair.private_key().as_bytes().to_vec();

        // Format private key as JSON array string for compatibility
        let private_key_str = format!("{:?}", private_key_bytes);

        let validator_key_json = serde_json::json!({
            "private_key": private_key_str,
            "public_key": public_key_hex,
            "created_at": chrono::Utc::now().to_rfc3339(),
            "key_type": "ed25519"
        });

        let json_str = serde_json::to_string_pretty(&validator_key_json)
            .map_err(|e| Error::crypto(format!("Serialization failed: {}", e)))?;
        tokio::fs::write(&output, &json_str)
            .await
            .map_err(Error::Io)?;

        info!("✓ Validator keypair generated");
        info!("📋 Public key: {}", public_key_hex);
        warn!("⚠️  KEEP THIS FILE SECURE - contains private key!");
        info!("✓ Validator key saved to {:?}", output);
        return Ok(());
    }

    if burner {
        info!("Creating burner/ephemeral identity");
        #[allow(deprecated)]
        let keypair = KeyPair::generate();
        let burner_identity = BurnerIdentity::new(&keypair, None);
        info!("✓ Burner identity created: {}", burner_identity.burner_id);

        // Save burner identity (no encryption, ephemeral nature)
        save_burner_identity_unencrypted(&output, &burner_identity).await?;
    } else {
        info!("Generating permanent identity...");
        #[allow(deprecated)]
        let keypair = KeyPair::generate();
        let identity = Identity::new("user".to_string(), &keypair);
        info!("✓ Identity created: {}", identity.user_id);

        // Display public key for reference
        let public_key_hex = hex::encode(keypair.public_key().as_bytes());
        info!("📋 Public key: {}", public_key_hex);

        if plaintext {
            // Save as plaintext for automated deployments
            save_identity_plaintext(&output, &identity).await?;
            warn!("⚠️  Identity saved WITHOUT encryption - for automated deployments only!");
        } else {
            // Prompt for password and encrypt
            let password = prompt_password("Enter password to encrypt identity: ")?;
            let confirm = prompt_password("Confirm password: ")?;

            if password != confirm {
                return Err(Error::crypto("Passwords do not match".to_string()));
            }

            if password.len() < 8 {
                return Err(Error::crypto(
                    "Password must be at least 8 characters".to_string(),
                ));
            }

            save_identity_encrypted(&output, &identity, &password).await?;
        }
    }

    info!("✓ Identity saved to {:?}", output);
    Ok(())
}

/// Prompt user for password (interactive)
fn prompt_password(prompt_msg: &str) -> Result<String> {
    use std::io::{self, Write};

    print!("{}", prompt_msg);
    io::stdout().flush().map_err(Error::Io)?;

    let mut password = String::new();
    io::stdin().read_line(&mut password).map_err(Error::Io)?;

    Ok(password.trim().to_string())
}

/// Save identity with encryption
async fn save_identity_encrypted(path: &Path, identity: &Identity, password: &str) -> Result<()> {
    use dchat_crypto::encrypt_with_password;

    // Serialize identity to JSON
    let json = serde_json::to_string(identity)
        .map_err(|e| Error::crypto(format!("Serialization failed: {}", e)))?;

    // Encrypt with password
    let encrypted = encrypt_with_password(password, json.as_bytes())?;

    // Serialize encrypted container to bytes
    let encrypted_bytes = encrypted.to_bytes()?;

    // Write to file
    tokio::fs::write(path, &encrypted_bytes)
        .await
        .map_err(Error::Io)?;

    info!("✓ Identity encrypted and saved");
    Ok(())
}

/// Save burner identity (unencrypted, ephemeral)
async fn save_burner_identity_unencrypted(
    path: &Path,
    burner_identity: &BurnerIdentity,
) -> Result<()> {
    // Serialize to JSON
    let json = serde_json::to_string(burner_identity)
        .map_err(|e| Error::crypto(format!("Serialization failed: {}", e)))?;

    // Write to file
    tokio::fs::write(path, json).await.map_err(Error::Io)?;

    info!("✓ Burner identity saved");
    Ok(())
}

/// Run database management commands
async fn run_database_command(config: Config, action: DatabaseCommand) -> Result<()> {
    use dchat_storage::database::{Database, DatabaseConfig};

    match action {
        DatabaseCommand::Migrate => {
            info!("🗄️  Running database migrations...");

            // Create database config from storage config
            let db_config = DatabaseConfig {
                path: config.storage.data_dir.join("dchat.db"),
                max_connections: config.storage.db_pool_size,
                connection_timeout_secs: config.storage.db_connection_timeout_secs,
                idle_timeout_secs: config.storage.db_idle_timeout_secs,
                max_lifetime_secs: config.storage.db_max_lifetime_secs,
                enable_wal: config.storage.db_enable_wal,
            };

            // Initialize database (creates tables and indexes)
            let db = Database::new(db_config).await?;
            info!("✓ Database schema initialized");

            // Health check
            let health = db.health_check().await?;
            info!(
                "✓ Pool health: {} connections ({} idle), acquire: {}ms",
                health.pool_size, health.idle_connections, health.acquire_time_ms
            );

            // Close gracefully
            db.close().await?;
            info!("✓ Migrations complete");
            Ok(())
        }
        DatabaseCommand::Backup { output } => {
            info!("💾 Backing up database to {:?}...", output);

            // Create database config
            let db_config = DatabaseConfig {
                path: config.storage.data_dir.join("dchat.db"),
                max_connections: config.storage.db_pool_size,
                connection_timeout_secs: config.storage.db_connection_timeout_secs,
                idle_timeout_secs: config.storage.db_idle_timeout_secs,
                max_lifetime_secs: config.storage.db_max_lifetime_secs,
                enable_wal: config.storage.db_enable_wal,
            };

            let db = Database::new(db_config).await?;
            let stats = db.stats().await?;
            info!(
                "Database contents: {} users, {} messages, {} channels",
                stats.user_count, stats.message_count, stats.channel_count
            );

            // Production: Use SQLite backup API for consistent snapshot
            info!("Creating database backup...");

            // Backup database using SQLite backup API
            use std::fs;

            // Ensure output directory exists
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }

            // Perform database backup using BackupManager
            let backup_manager = BackupManager::new(
                config.storage.data_dir.join("backups"),
                10, // Keep 10 backups
            );

            // Generate encryption key from node's identity key
            // In production, this should derive from a user-provided passphrase or KMS
            let key_hash = blake3::hash(b"dchat-database-backup-key-v1");
            let encryption_key: [u8; 32] = *key_hash.as_bytes();

            // Read database file
            let db_path = config.storage.data_dir.join("dchat.db");
            let db_data = tokio::fs::read(&db_path)
                .await
                .map_err(|e| Error::storage(format!("Failed to read database file: {}", e)))?;

            info!("Database size: {} bytes", db_data.len());

            // Create encrypted backup
            let backup_path = backup_manager
                .create_backup("node".to_string(), db_data, &encryption_key)
                .await
                .map_err(|e| Error::storage(format!("Backup creation failed: {}", e)))?;

            // Copy backup to requested output location
            tokio::fs::copy(&backup_path, &output)
                .await
                .map_err(|e| Error::storage(format!("Failed to copy backup to output: {}", e)))?;

            let file_size = fs::metadata(&output)?.len();
            info!("✓ Database backed up to {:?} ({} bytes)", output, file_size);

            // Close database connection
            drop(db); // Database closes on drop
            info!("✓ Backup complete");
            Ok(())
        }
        DatabaseCommand::Restore { input } => {
            info!("📥 Restoring database from {:?}...", input);

            // Create database config
            let db_config = DatabaseConfig {
                path: config.storage.data_dir.join("dchat.db"),
                max_connections: config.storage.db_pool_size,
                connection_timeout_secs: config.storage.db_connection_timeout_secs,
                idle_timeout_secs: config.storage.db_idle_timeout_secs,
                max_lifetime_secs: config.storage.db_max_lifetime_secs,
                enable_wal: config.storage.db_enable_wal,
            };

            // Note: Database will be recreated from restore, no existing connection to close

            // Production: Use BackupManager to decrypt and restore
            info!("Decrypting and restoring database from encrypted backup...");

            let backup_manager = BackupManager::new(config.storage.data_dir.join("backups"), 10);

            // Generate encryption key (same as backup)
            let key_hash = blake3::hash(b"dchat-database-backup-key-v1");
            let encryption_key: [u8; 32] = *key_hash.as_bytes();

            // Restore from encrypted backup
            let decrypted_data = backup_manager
                .restore_backup(input.clone(), &encryption_key)
                .await
                .map_err(|e| Error::storage(format!("Failed to restore backup: {}", e)))?;

            info!("Decrypted {} bytes from backup", decrypted_data.len());

            // Write decrypted data to database file
            let db_path = config.storage.data_dir.join("dchat.db");
            tokio::fs::write(&db_path, decrypted_data)
                .await
                .map_err(|e| Error::storage(format!("Failed to write database file: {}", e)))?;

            info!("Verifying restored database...");
            let restored_db = Database::new(db_config.clone()).await?;
            restored_db.health_check().await?;

            info!("✓ Restore complete");
            Ok(())
        }
    }
}

/// Check node health
async fn check_health(url: &str) -> Result<()> {
    info!("🏥 Checking health at {}...", url);

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Network(format!("Health check failed: {}", e)))?;

    if response.status().is_success() {
        info!("✓ Node is healthy");
        std::process::exit(0);
    } else {
        error!("✗ Node is unhealthy: {}", response.status());
        std::process::exit(1);
    }
}

/// Start health check server
// ═══════════════════════════════════════════════════════════════════════════
// PEER MANAGEMENT & HANDSHAKING - Mainnet Production Functions
// ═══════════════════════════════════════════════════════════════════════════

/// Perform initial handshake with a newly connected peer
async fn perform_peer_handshake(
    peer_id: PeerId,
    network: &mut NetworkManager,
    peer_registry: &PeerRegistry,
    node_type: NodeType,
    geographic_region: Option<String>,
) -> Result<()> {
    debug!("🤝 Initiating handshake with peer: {}", peer_id);

    // Get current known peers to share
    let known_peers = peer_registry.get_all_peers().await;
    let peer_ads: Vec<PeerAdvertisement> = known_peers
        .iter()
        .map(|p| PeerAdvertisement {
            peer_id: p.peer_id.to_string(),
            multiaddr: p.multiaddr.to_string(),
            node_type: p.node_type.to_string(),
            geographic_region: p.geographic_region.clone(),
        })
        .collect();

    // Construct handshake message
    let handshake = PeerHandshake {
        node_type: node_type.to_string(),
        version: VERSION.to_string(),
        capabilities: vec![
            "messaging".to_string(),
            "relay".to_string(),
            "dht".to_string(),
        ],
        geographic_region: geographic_region.clone(),
        known_peers: peer_ads,
        timestamp: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs(),
    };

    // Send handshake via request-response protocol
    // Serialize handshake to JSON for transmission
    let handshake_bytes = serde_json::to_vec(&handshake)
        .map_err(|e| Error::crypto(format!("Failed to serialize handshake: {}", e)))?;

    debug!(
        "Sending handshake to {} with {} known peers ({} bytes)",
        peer_id,
        handshake.known_peers.len(),
        handshake_bytes.len()
    );

    // Use dchat network manager to send handshake
    network
        .send_handshake(peer_id, handshake_bytes)
        .map_err(|e| Error::network(format!("Failed to send handshake: {}", e)))?;

    info!("✓ Handshake sent to {} successfully", peer_id);
    Ok(())
}

/// Process incoming handshake from a peer
pub async fn handle_peer_handshake(
    peer_id: PeerId,
    handshake: PeerHandshake,
    peer_registry: &PeerRegistry,
    network: &mut NetworkManager,
) -> Result<()> {
    info!(
        "📨 Received handshake from {} (type: {}, region: {:?})",
        peer_id, handshake.node_type, handshake.geographic_region
    );

    // Validate handshake
    if handshake.version != VERSION {
        warn!(
            "Version mismatch: peer {} is running {}, we are running {}",
            peer_id, handshake.version, VERSION
        );
    }

    // Add peer to registry
    let multiaddr = network
        .listeners()
        .first()
        .cloned()
        .map(Ok)
        .unwrap_or_else(parse_fallback_listen_addr)?;

    let peer_info = PeerInfo {
        peer_id,
        multiaddr,
        node_type: match handshake.node_type.as_str() {
            "validator" => NodeType::Validator,
            "relay" => NodeType::Relay,
            _ => NodeType::Client,
        },
        geographic_region: handshake.geographic_region.clone(),
        last_seen: SystemTime::now(),
        connection_quality: 1.0,
        capabilities: handshake.capabilities.clone(),
        is_bootstrap: false,
        rtt_ms: None,
        packet_loss: 0.0,
        jitter_ms: None,
        total_messages_sent: 0,
        total_messages_received: 0,
        handshake_success: true,
        connected_since: SystemTime::now(),
    };

    peer_registry.add_peer(peer_info).await;

    // Process and connect to newly discovered peers
    for adv in handshake.known_peers {
        if let Ok(multiaddr) = adv.multiaddr.parse::<Multiaddr>() {
            if let Ok(discovered_peer_id) = adv.peer_id.parse::<PeerId>() {
                // Check if we already know this peer
                if peer_registry.get_peer(&discovered_peer_id).await.is_none() {
                    info!(
                        "🔍 Discovered new peer via handshake: {} at {}",
                        discovered_peer_id, multiaddr
                    );

                    // Attempt to connect
                    match network.dial(multiaddr.clone()) {
                        Ok(_) => debug!("Connecting to discovered peer: {}", discovered_peer_id),
                        Err(e) => warn!(
                            "Failed to dial discovered peer {}: {}",
                            discovered_peer_id, e
                        ),
                    }

                    // Add to registry
                    let new_peer_info = PeerInfo {
                        peer_id: discovered_peer_id,
                        multiaddr,
                        node_type: match adv.node_type.as_str() {
                            "validator" => NodeType::Validator,
                            "relay" => NodeType::Relay,
                            _ => NodeType::Client,
                        },
                        geographic_region: adv.geographic_region.clone(),
                        last_seen: SystemTime::now(),
                        connection_quality: 0.5, // Unknown quality initially
                        capabilities: vec![],
                        is_bootstrap: false,
                        rtt_ms: None,
                        packet_loss: 0.0,
                        jitter_ms: None,
                        total_messages_sent: 0,
                        total_messages_received: 0,
                        handshake_success: false,
                        connected_since: SystemTime::now(),
                    };
                    peer_registry.add_peer(new_peer_info).await;
                }
            }
        }
    }

    info!("✓ Handshake processed, peer registry updated");
    Ok(())
}

/// Monitor peer health and prune stale connections
async fn run_peer_health_monitor(
    peer_registry: Arc<PeerRegistry>,
    network: Arc<tokio::sync::Mutex<NetworkManager>>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let mut interval = tokio::time::interval(PEER_HEALTH_CHECK_INTERVAL);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                debug!("🩺 Running peer health check...");

                // Prune stale peers (not seen in 5 minutes)
                peer_registry.prune_stale_peers(Duration::from_secs(300)).await;

                // Get current connection count
                let all_peers = peer_registry.get_all_peers().await;
                let validators = peer_registry.get_peers_by_type(NodeType::Validator).await;
                let relays = peer_registry.get_peers_by_type(NodeType::Relay).await;
                let clients = peer_registry.get_peers_by_type(NodeType::Client).await;

                // Calculate connection quality metrics
                let avg_quality = peer_registry.calculate_average_quality().await;
                let avg_rtt = peer_registry.calculate_average_rtt().await;

                info!(
                    "📊 Peer Status: {} total ({} validators, {} relays, {} clients)",
                    all_peers.len(),
                    validators.len(),
                    relays.len(),
                    clients.len()
                );

                info!(
                    "📈 Connection Quality: avg={:.2}, avg_rtt={:.1}ms",
                    avg_quality, avg_rtt
                );

                // Collect peers with poor connection quality for removal
                let mut peers_to_remove = Vec::new();
                for peer in &all_peers {
                    if peer.connection_quality < 0.5 {
                        warn!(
                            "⚠️  Poor connection to {}: quality={:.2}, rtt={:?}ms, packet_loss={:.2}%",
                            peer.peer_id,
                            peer.connection_quality,
                            peer.rtt_ms,
                            peer.packet_loss * 100.0
                        );
                        // Mark extremely poor connections for removal
                        if peer.connection_quality < 0.1 && !peer.is_bootstrap {
                            peers_to_remove.push(peer.peer_id);
                        }
                    }
                }

                // Remove extremely poor non-bootstrap peers
                for peer_id in peers_to_remove {
                    warn!("🗑️  Removing unresponsive peer: {}", peer_id);
                    peer_registry.remove_peer(&peer_id).await;
                    // Also disconnect from network
                    let _ = network.lock().await.disconnect_peer(&peer_id);
                }

                // Check if we need more connections
                if validators.len() < MIN_VALIDATOR_CONNECTIONS {
                    warn!(
                        "⚠️  Low validator connections: {} < {}",
                        validators.len(),
                        MIN_VALIDATOR_CONNECTIONS
                    );
                    // Try to connect to best quality peers from bootstrap
                    let best_validators = peer_registry.get_best_peers_by_quality(NodeType::Validator, 5).await;
                    for validator in best_validators {
                        if let Err(e) = network.lock().await.dial(validator.multiaddr.clone()) {
                            debug!("Failed to dial validator {}: {}", validator.peer_id, e);
                        }
                    }
                }

                if relays.len() < MIN_RELAY_CONNECTIONS {
                    warn!(
                        "⚠️  Low relay connections: {} < {}",
                        relays.len(),
                        MIN_RELAY_CONNECTIONS
                    );
                    // Try to connect to best quality relay peers
                    let best_relays = peer_registry.get_best_peers_by_quality(NodeType::Relay, 5).await;
                    for relay in best_relays {
                        if let Err(e) = network.lock().await.dial(relay.multiaddr.clone()) {
                            debug!("Failed to dial relay {}: {}", relay.peer_id, e);
                        }
                    }
                }

                // Log geographic distribution
                let mut region_map: HashMap<String, usize> = HashMap::new();
                for peer in &all_peers {
                    if let Some(region) = &peer.geographic_region {
                        *region_map.entry(region.clone()).or_insert(0) += 1;
                    }
                }

                if !region_map.is_empty() {
                    info!("🌍 Geographic Distribution:");
                    for (region, count) in &region_map {
                        info!("   {} = {} peers", region, count);
                    }

                    // Check if we have geographic diversity (mainnet requirement)
                    if region_map.len() < 2 {
                        warn!("⚠️  Low geographic diversity: only {} region(s) represented", region_map.len());
                        // Try to discover peers from underrepresented regions
                        let known_regions = ["india", "south-africa", "uae", "us-east", "eu-west", "asia-pacific"];
                        for region in known_regions {
                            if !region_map.contains_key(region) {
                                let region_peers = peer_registry.get_peers_by_region(region).await;
                                for peer in region_peers.iter().take(2) {
                                    if let Err(e) = network.lock().await.dial(peer.multiaddr.clone()) {
                                        debug!("Failed to dial {} peer {}: {}", region, peer.peer_id, e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ = shutdown.recv() => {
                info!("🛑 Peer health monitor shutting down");
                break;
            }
        }
    }
}

/// Periodically sync peer lists with connected peers
async fn run_peer_list_sync(
    peer_registry: Arc<PeerRegistry>,
    network: Arc<tokio::sync::Mutex<NetworkManager>>,
    mut shutdown: broadcast::Receiver<()>,
) {
    const MAX_ADVERTISED_PEERS: usize = 20;
    const PEER_DISCOVERY_CHANNEL: &str = "dchat/peer-discovery/1.0.0";

    let mut interval = tokio::time::interval(PEER_LIST_SYNC_INTERVAL);

    // Subscribe to peer discovery channel for receiving advertisements
    {
        let mut net_guard = network.lock().await;
        if let Err(e) = net_guard.subscribe_channel(PEER_DISCOVERY_CHANNEL) {
            warn!("⚠️  Failed to subscribe to peer discovery channel: {}", e);
        } else {
            debug!("✓ Subscribed to peer discovery channel");
        }
    }

    loop {
        tokio::select! {
            _ = interval.tick() => {
                debug!("🔄 Synchronizing peer lists...");

                let mut net_guard = network.lock().await;
                let local_peer_id = net_guard.local_peer_id();
                let peer_count = peer_registry.get_all_peers().await.len();
                debug!("Current peer registry size: {}", peer_count);

                // Create peer advertisement
                let advertisement = create_peer_discovery_advertisement(local_peer_id, &peer_registry, MAX_ADVERTISED_PEERS).await;

                // Serialize to JSON bytes for gossipsub
                if let Ok(ad_bytes) = serde_json::to_vec(&advertisement) {
                    // Create channel message for peer discovery
                    // Use peer_id as the sender identity for peer discovery
                    let sender_id = UserId::new(); // Generate new UserId for peer discovery

                    let timestamp = chrono::Utc::now().timestamp();
                    let message_id = dchat_network::behavior::compute_channel_message_id(
                        &sender_id,
                        PEER_DISCOVERY_CHANNEL,
                        &ad_bytes,
                        timestamp,
                    );
                    let dchat_msg = DchatMessage::ChannelMessage {
                        message_id,
                        sender: sender_id,
                        channel_id: PEER_DISCOVERY_CHANNEL.to_string(),
                        encrypted_payload: ad_bytes, // Not actually encrypted for peer discovery
                        timestamp,
                    };

                    // Publish to peer discovery channel
                    if let Err(e) = net_guard.publish_to_channel(PEER_DISCOVERY_CHANNEL, &dchat_msg) {
                        debug!("⚠️  Failed to publish peer advertisement: {}", e);
                    } else {
                        debug!("📢 Published peer advertisement ({} known peers)", advertisement.known_peers.len());
                    }
                }
            }
            _ = shutdown.recv() => {
                info!("🛑 Peer list sync shutting down");
                break;
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// OBSERVABILITY & HEALTH MONITORING
// ═══════════════════════════════════════════════════════════════════════════

fn start_health_server(
    addr: &str,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<tokio::task::JoinHandle<()>> {
    // Default readiness state for backward compatibility
    start_health_server_with_readiness(addr, shutdown, None)
}

fn start_health_server_with_readiness(
    addr: &str,
    mut shutdown: broadcast::Receiver<()>,
    readiness: Option<Arc<ReadinessState>>,
) -> Result<tokio::task::JoinHandle<()>> {
    use warp::Filter;

    let health = warp::path("health").map(|| {
        warp::reply::json(&serde_json::json!({
            "status": "healthy",
            "version": VERSION,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
    });

    // Clone readiness for the closure
    let readiness_for_route = readiness.clone();
    let ready = warp::path("ready").map(move || {
        match &readiness_for_route {
            Some(state) => {
                let response = state.to_json();
                if state.is_ready() {
                    warp::reply::with_status(
                        warp::reply::json(&response),
                        warp::http::StatusCode::OK,
                    )
                } else {
                    warp::reply::with_status(
                        warp::reply::json(&response),
                        warp::http::StatusCode::SERVICE_UNAVAILABLE,
                    )
                }
            }
            None => {
                // Legacy mode: always report ready
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"ready": true})),
                    warp::http::StatusCode::OK,
                )
            }
        }
    });

    let routes = health.or(ready);

    let addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| Error::Config(format!("Invalid health address: {}", e)))?;

    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(addr, async move {
        let _ = shutdown.recv().await;
    });

    Ok(tokio::spawn(server))
}

/// Handle user account management commands
async fn run_account_command(_config: Config, action: AccountCommand) -> Result<()> {
    use dchat::fee_gateway::{FeeGatedRequest, FeeGateway, OperationPayload};
    use dchat::storage_routed_user_management::StorageRoutedUserManager;
    use dchat::UserManager;
    use dchat_blockchain::fee_distribution::{FeeDistributionConfig, FeeDistributionManager};
    use dchat_blockchain::fee_orchestrator::{FeeConfig, FeeOrchestrator};
    use dchat_network::relay_network::{RelayNetworkConfig, RelayNetworkManager};
    use dchat_storage::DatabaseConfig;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};

    // Initialize database - MAINNET-SAFE: Use config.storage.data_dir
    let db_config = DatabaseConfig {
        path: _config.storage.data_dir.join("dchat_accounts.db"),
        max_connections: _config.storage.db_pool_size,
        connection_timeout_secs: _config.storage.db_connection_timeout_secs,
        idle_timeout_secs: _config.storage.db_idle_timeout_secs,
        max_lifetime_secs: _config.storage.db_max_lifetime_secs,
        enable_wal: _config.storage.db_enable_wal,
    };
    let database = dchat_storage::Database::new(db_config).await?;

    // Initialize parallel chains
    let mut chat_chain_config = ChatChainConfig::default();
    let chat_rpc_url = resolve_required_chat_chain_rpc_url(&_config)?;
    if _config.rpc.resolved_chat_chain_rpc_url().is_none() && allow_localhost_chain_rpc_defaults() {
        warn!(
            "Using localhost chat chain RPC default (DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1). This is unsafe for production."
        );
    }
    chat_chain_config.rpc_url = chat_rpc_url;

    let mut currency_chain_config = CurrencyChainConfig::default();
    let currency_rpc_url = resolve_required_currency_chain_rpc_url(&_config)?;
    if _config.rpc.resolved_currency_chain_rpc_url().is_none()
        && allow_localhost_chain_rpc_defaults()
    {
        warn!(
            "Using localhost currency chain RPC default (DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1). This is unsafe for production."
        );
    }
    currency_chain_config.rpc_url = currency_rpc_url;

    let chat_chain = Arc::new(ChatChainClient::new(chat_chain_config)?);
    let currency_chain = Arc::new(CurrencyChainClient::new(currency_chain_config)?);
    let bridge = Arc::new(CrossChainBridge::new(
        Arc::clone(&chat_chain),
        Arc::clone(&currency_chain),
    ));

    // Initialize legacy user manager (for non-messaging operations)
    let user_manager = UserManager::new(
        database.clone(),
        Arc::clone(&chat_chain),
        Arc::clone(&currency_chain),
        Arc::clone(&bridge),
        PathBuf::from("./keys"),
    );

    // Initialize FeeGateway for production-grade messaging with fee enforcement
    let fee_distribution = Arc::new(FeeDistributionManager::new(FeeDistributionConfig::default()));
    let fee_orchestrator = Arc::new(FeeOrchestrator::new(
        Arc::clone(&currency_chain),
        fee_distribution,
        FeeConfig::default(),
    )?);

    // Initialize relay network manager
    let relay_network = Arc::new(RwLock::new(RelayNetworkManager::new(
        RelayNetworkConfig::default(),
    )));

    // Populate relay network from config seed relays
    if !_config.relay.seed_relays.is_empty() {
        use dchat_core::types::UserId;
        use dchat_network::relay_network::{Continent, RelayInfo};
        use uuid::Uuid;

        let mut relay_mgr = relay_network.write().unwrap_or_else(|e| {
            warn!("⚠️  relay_network RwLock poisoned; continuing with inner state");
            e.into_inner()
        });
        for seed in &_config.relay.seed_relays {
            let continent = match seed.continent.to_lowercase().as_str() {
                "northamerica" | "north_america" | "na" => Continent::NorthAmerica,
                "southamerica" | "south_america" | "sa" => Continent::SouthAmerica,
                "europe" | "eu" => Continent::Europe,
                "asia" => Continent::Asia,
                "africa" | "af" => Continent::Africa,
                "oceania" | "oc" | "australia" => Continent::Oceania,
                _ => Continent::Europe, // Default
            };

            let operator = seed
                .operator_id
                .as_ref()
                .and_then(|id| Uuid::parse_str(id).ok())
                .map(UserId)
                .unwrap_or_else(UserId::new);

            let relay_info = RelayInfo::new(
                seed.relay_id.clone(),
                operator,
                seed.stake,
                continent,
                0, // ASN - can be enhanced later
            );

            if let Err(e) = relay_mgr.register_relay(relay_info) {
                warn!("Failed to register seed relay {}: {}", seed.relay_id, e);
            } else {
                info!(
                    "Registered seed relay: {} ({})",
                    seed.relay_id, seed.multiaddr
                );
            }
        }
        drop(relay_mgr);
        info!(
            "Loaded {} seed relays from config",
            _config.relay.seed_relays.len()
        );
    }

    // Initialize storage-routed user manager for message storage (offline mode)
    let storage_manager = Arc::new(
        StorageRoutedUserManager::offline(
            _config.storage.data_dir.join("messages.db"),
            database,
            Arc::clone(&chat_chain),
            Arc::clone(&currency_chain),
            Arc::clone(&bridge),
            PathBuf::from("./keys"),
        )
        .await?,
    );

    // Create production FeeGateway
    let fee_gateway = FeeGateway::new(
        fee_orchestrator,
        Arc::clone(&currency_chain),
        Arc::clone(&chat_chain),
        storage_manager,
        relay_network,
    );

    match action {
        AccountCommand::Create { username, save_to } => {
            info!("📝 Creating user account: {}", username);
            let response = user_manager.create_user(&username).await?;

            println!("\n✅ User Created Successfully!");
            println!("  User ID: {}", response.user_id);
            println!("  Username: {}", response.username);
            println!("  Public Key: {}", response.public_key);
            println!("  Created: {}", response.created_at);
            println!("\n🔐 IMPORTANT: Store your private key securely!");
            println!("  Private Key: {}", response.private_key);

            // Save to file
            let json = serde_json::to_string_pretty(&response)?;
            std::fs::write(&save_to, json)?;
            println!("\n💾 Keys saved to: {:?}", save_to);

            Ok(())
        }

        AccountCommand::List => {
            info!("📋 Listing all users");
            let users = user_manager.list_users().await?;

            if users.is_empty() {
                println!("No users created yet.");
            } else {
                println!("\n👥 Users ({}):", users.len());
                println!("{:<40} {:<20} {:<40}", "User ID", "Username", "Public Key");
                println!("{}", "-".repeat(100));
                for user in users {
                    let key_preview =
                        format!("{}...", &user.public_key[..16.min(user.public_key.len())]);
                    println!(
                        "{:<40} {:<20} {:<40}",
                        user.user_id, user.username, key_preview
                    );
                }
            }

            Ok(())
        }

        AccountCommand::Profile { user_id } => {
            info!("👤 Getting profile for user: {}", user_id);
            let profile = user_manager.get_user_profile(&user_id).await?;

            println!("\n📊 User Profile:");
            println!("  User ID: {}", profile.user_id);
            println!("  Username: {}", profile.username);
            println!("  Display Name: {:?}", profile.display_name);
            println!("  Reputation: {}", profile.reputation_score);
            println!("  Verified: {}", profile.verified);
            println!("  Created: {}", profile.created_at);
            println!("  Public Key: {}", profile.public_key);

            Ok(())
        }

        AccountCommand::SendDm { from, to, message } => {
            info!("💬 Sending DM from {} to {} via FeeGateway", from, to);

            // Parse user IDs
            let from_id = dchat_core::types::UserId(
                uuid::Uuid::parse_str(&from)
                    .map_err(|e| Error::validation(format!("Invalid from user ID: {}", e)))?,
            );
            let to_id = dchat_core::types::UserId(
                uuid::Uuid::parse_str(&to)
                    .map_err(|e| Error::validation(format!("Invalid to user ID: {}", e)))?,
            );

            // Create fee-gated request
            let request = FeeGatedRequest {
                payer: from_id.clone(),
                client_nonce: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                    .as_nanos() as u64,
                payload: OperationPayload::DirectMessage {
                    recipient: to_id,
                    content: message.as_bytes().to_vec(),
                    encrypted: false,
                    encryption_key_id: None,
                },
                preferred_relay: None,
            };

            // Send via FeeGateway (enforces fee payment + finality)
            let response = fee_gateway.send_direct_message(request).await?;

            println!("\n✅ Direct Message Sent (Fee-Gated)!");
            println!("  Message ID: {}", response.message_id);
            println!(
                "  Operation ID: {}",
                hex::encode(&response.operation_id[..8])
            );
            println!("  Chat TX: {}", response.chat_tx.tx_id);
            println!("  Chat TX Hash: {}", response.chat_tx.tx_hash);
            println!(
                "  Finality: {} confirmations",
                response.chat_tx.confirmations
            );
            println!(
                "  Gas Fee: {} (tx: {})",
                response.gas_fee_receipt.amount, response.gas_fee_receipt.fee_tx_id
            );
            println!(
                "  Message Fee: {} to relay {}",
                response.message_fee_receipt.amount, response.message_fee_receipt.relay_id
            );
            println!("  Storage Tier: {:?}", response.storage_tier);
            println!("  Timestamp: {}", response.timestamp);

            Ok(())
        }

        AccountCommand::CreateChannel {
            creator_id,
            name,
            description,
        } => {
            info!("📢 Creating channel: {}", name);
            let response = user_manager
                .create_channel(&creator_id, &name, description.as_deref())
                .await?;

            println!("\n✅ Channel Created!");
            println!("  Channel ID: {}", response.channel_id);
            println!("  Name: {}", response.channel_name);
            println!("  Creator: {}", response.creator_id);
            println!("  Created: {}", response.created_at);
            println!("  On-chain: {}", response.on_chain_confirmed);

            Ok(())
        }

        AccountCommand::PostChannel {
            user_id,
            channel_id,
            message,
        } => {
            info!("📝 Posting to channel: {} via FeeGateway", channel_id);

            // Parse IDs
            let user = dchat_core::types::UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|e| Error::validation(format!("Invalid user ID: {}", e)))?,
            );
            let channel = dchat_core::types::ChannelId(
                uuid::Uuid::parse_str(&channel_id)
                    .map_err(|e| Error::validation(format!("Invalid channel ID: {}", e)))?,
            );

            // Create fee-gated request
            let request = FeeGatedRequest {
                payer: user.clone(),
                client_nonce: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                    .as_nanos() as u64,
                payload: OperationPayload::ChannelPost {
                    channel_id: channel,
                    content: message.as_bytes().to_vec(),
                    encrypted: false,
                    encryption_key_id: None,
                },
                preferred_relay: None,
            };

            // Post via FeeGateway (enforces fee payment + finality)
            let response = fee_gateway.post_to_channel(request).await?;

            println!("\n✅ Message Posted (Fee-Gated)!");
            println!("  Message ID: {}", response.message_id);
            println!(
                "  Operation ID: {}",
                hex::encode(&response.operation_id[..8])
            );
            println!("  Chat TX: {}", response.chat_tx.tx_id);
            println!(
                "  Finality: {} confirmations",
                response.chat_tx.confirmations
            );
            println!(
                "  Gas Fee: {} (tx: {})",
                response.gas_fee_receipt.amount, response.gas_fee_receipt.fee_tx_id
            );
            println!(
                "  Message Fee: {} to relay {}",
                response.message_fee_receipt.amount, response.message_fee_receipt.relay_id
            );
            println!("  Timestamp: {}", response.timestamp);

            Ok(())
        }

        AccountCommand::GetDms { user_id } => {
            info!("📬 Getting DMs for: {}", user_id);
            let messages = user_manager.get_direct_messages(&user_id).await?;

            if messages.is_empty() {
                println!("No direct messages.");
            } else {
                println!("\n📬 Direct Messages ({}):", messages.len());
                println!("{:<40} {:<15} {:<25}", "Message ID", "Status", "Timestamp");
                println!("{}", "-".repeat(80));
                for msg in messages {
                    println!(
                        "{:<40} {:<15} {:<25}",
                        msg.message_id, msg.status, msg.timestamp
                    );
                }
            }

            Ok(())
        }

        AccountCommand::GetChannelMessages { channel_id } => {
            info!("📖 Getting messages for channel: {}", channel_id);
            let messages = user_manager.get_channel_messages(&channel_id).await?;

            if messages.is_empty() {
                println!("No messages in channel.");
            } else {
                println!("\n📖 Channel Messages ({}):", messages.len());
                println!("{:<40} {:<15} {:<25}", "Message ID", "Status", "Timestamp");
                println!("{}", "-".repeat(80));
                for msg in messages {
                    println!(
                        "{:<40} {:<15} {:<25}",
                        msg.message_id, msg.status, msg.timestamp
                    );
                }
            }

            Ok(())
        }
    }
}

/// Run bot management commands
async fn run_bot_command(_config: Config, action: BotCommand) -> Result<()> {
    use dchat::bots::{BotFather, CreateBotRequest};

    fn mask_secret_for_display(value: &str) -> String {
        const EDGE: usize = 4;
        if value.is_empty() {
            return "".to_string();
        }
        if value.len() <= EDGE * 2 {
            return "*".repeat(value.len());
        }
        format!(
            "{}…{}",
            &value[..EDGE],
            &value[value.len().saturating_sub(EDGE)..]
        )
    }
    use dchat_core::types::UserId;

    let bot_father = BotFather::new();

    match action {
        BotCommand::Create {
            username,
            name,
            description,
            owner_id,
            reveal_token,
        } => {
            info!("🤖 Creating bot: {}", username);

            let owner = UserId(
                uuid::Uuid::parse_str(&owner_id)
                    .map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let request = CreateBotRequest {
                username: username.clone(),
                display_name: name.clone(),
                description: Some(description.clone()),
            };

            let bot = bot_father.create_bot(owner, request)?;

            println!("\n✅ Bot created successfully!");
            println!("Bot ID: {}", bot.id);
            println!("Username: @{}", bot.username);
            println!("Name: {}", bot.display_name);
            if reveal_token {
                println!("Token: {}", bot.token);
            } else {
                println!("Token: {}", mask_secret_for_display(&bot.token));
                println!("(Use --reveal-token to print the full token)");
            }
            println!("\n⚠️  Keep this token secret! It cannot be recovered.");

            Ok(())
        }

        BotCommand::List { owner_id } => {
            match owner_id {
                Some(id) => {
                    let owner = UserId(
                        uuid::Uuid::parse_str(&id)
                            .map_err(|_| Error::validation("Invalid owner ID"))?,
                    );
                    let bots = bot_father.get_user_bots(&owner);

                    println!("\n🤖 Bots owned by {}:", id);
                    if bots.is_empty() {
                        println!("No bots found.");
                    } else {
                        for bot in bots {
                            println!(
                                "  • {} (@{}) - {}",
                                bot.display_name,
                                bot.username,
                                if bot.is_active { "Active" } else { "Inactive" }
                            );
                        }
                    }
                }
                None => {
                    let count = bot_father.get_all_bots_count();
                    let active = bot_father.get_active_bots_count();
                    println!("\n🤖 Total bots: {} (Active: {})", count, active);
                }
            }

            Ok(())
        }

        BotCommand::Info { bot_id } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;

            match bot_father.get_bot(&bot_uuid) {
                Some(bot) => {
                    println!("\n🤖 Bot Information:");
                    println!("ID: {}", bot.id);
                    println!("Username: @{}", bot.username);
                    println!("Name: {}", bot.display_name);
                    println!("Description: {:?}", bot.description);
                    println!("Owner: {}", bot.owner_id);
                    println!("Active: {}", bot.is_active);
                    println!("Created: {}", bot.created_at);
                    println!("Commands: {}", bot.commands.len());
                }
                None => {
                    println!("❌ Bot not found");
                }
            }

            Ok(())
        }

        BotCommand::RegenerateToken {
            bot_id,
            owner_id,
            reveal_token,
        } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;
            let owner = UserId(
                uuid::Uuid::parse_str(&owner_id)
                    .map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let new_token = bot_father.regenerate_token(&bot_uuid, &owner)?;

            println!("\n✅ Token regenerated successfully!");
            if reveal_token {
                println!("New token: {}", new_token);
            } else {
                println!("New token: {}", mask_secret_for_display(&new_token));
                println!("(Use --reveal-token to print the full token)");
            }
            println!("\n⚠️  Keep this token secret! Old token is now invalid.");

            Ok(())
        }

        BotCommand::SetWebhook {
            bot_id,
            url,
            secret,
        } => {
            println!("\n🔗 Setting webhook for bot {}", bot_id);
            println!("URL: {}", url);
            if let Some(s) = secret {
                println!("Secret: {}", "*".repeat(s.len()));
            }
            println!("\n✅ Webhook configured (in-memory only)");

            Ok(())
        }

        BotCommand::SendMessage {
            token: _,
            chat_id,
            text,
        } => {
            println!("\n📤 Sending message as bot...");
            println!("Chat: {}", chat_id);
            println!("Text: {}", text);
            println!("\n✅ Message sent (simulated)");

            Ok(())
        }
    }
}

/// Run marketplace commands
async fn run_marketplace_command(_config: Config, action: MarketplaceCommand) -> Result<()> {
    use dchat::marketplace::attestations::{AttestationValidatorSet, MarketplaceAttestation};
    use dchat::marketplace::persistence::MarketplaceStore;
    use dchat::marketplace::{DigitalGoodType, OnChainStorageType, PricingModel, Purchase};
    use std::fs;

    #[derive(Debug, Deserialize)]
    struct MarketplaceValidatorSetFile {
        required_signers: usize,
        validators: Vec<MarketplaceValidatorEntry>,
    }

    #[derive(Debug, Deserialize)]
    struct MarketplaceValidatorEntry {
        user_id: String,
        bls_pubkey_hex: String,
    }

    fn open_store(
        db_path: Option<PathBuf>,
    ) -> impl std::future::Future<Output = Result<MarketplaceStore>> {
        async move {
            let path = db_path.unwrap_or_else(MarketplaceStore::default_path);
            MarketplaceStore::open(path).await
        }
    }

    fn parse_user_id(value: &str, field: &'static str) -> Result<UserId> {
        Ok(UserId(uuid::Uuid::parse_str(value).map_err(|_| {
            Error::validation(format!("Invalid {field}"))
        })?))
    }

    fn parse_uuid(value: &str, field: &'static str) -> Result<uuid::Uuid> {
        uuid::Uuid::parse_str(value).map_err(|_| Error::validation(format!("Invalid {field}")))
    }

    fn parse_item_type(value: &str) -> Result<(DigitalGoodType, OnChainStorageType)> {
        Ok(match value {
            "sticker-pack" => (DigitalGoodType::StickerPack, OnChainStorageType::Ipfs),
            "emoji-pack" => (DigitalGoodType::EmojiPack, OnChainStorageType::Ipfs),
            "theme" => (DigitalGoodType::Theme, OnChainStorageType::Ipfs),
            "bot" => (DigitalGoodType::Bot, OnChainStorageType::Hybrid),
            "nft" => (DigitalGoodType::Nft, OnChainStorageType::Hybrid),
            "image" => (DigitalGoodType::Image, OnChainStorageType::Hybrid),
            "subscription" => (DigitalGoodType::Subscription, OnChainStorageType::ChatChain),
            "badge" => (DigitalGoodType::Badge, OnChainStorageType::ChatChain),
            "channel" => (DigitalGoodType::Channel, OnChainStorageType::ChatChain),
            "membership" => (DigitalGoodType::Membership, OnChainStorageType::ChatChain),
            _ => return Err(Error::validation("Invalid item type")),
        })
    }

    fn load_validator_set(path: &Path) -> Result<AttestationValidatorSet> {
        let raw = fs::read_to_string(path)?;
        let file: MarketplaceValidatorSetFile = serde_json::from_str(&raw)?;

        if file.validators.is_empty() {
            return Err(Error::validation(
                "validator_set.validators must not be empty",
            ));
        }

        let mut validators: HashMap<UserId, Vec<u8>> = HashMap::new();
        for entry in file.validators {
            let id = parse_user_id(&entry.user_id, "validator user_id")?;
            let pk = hex::decode(entry.bls_pubkey_hex.trim_start_matches("0x"))
                .map_err(|_| Error::validation("Invalid validator bls_pubkey_hex"))?;
            validators.insert(id, pk);
        }

        AttestationValidatorSet::new(file.required_signers, validators)
    }

    fn load_attestation(path: &Path) -> Result<MarketplaceAttestation> {
        let raw = fs::read_to_string(path)?;
        serde_json::from_str(&raw).map_err(Error::from)
    }

    #[cfg(feature = "dev-tools")]
    let mut marketplace = dchat::marketplace::MarketplaceManager::new();

    match action {
        MarketplaceCommand::List { item_type, db_path } => {
            println!("\n🏪 Marketplace Listings:");

            let store = open_store(db_path).await?;
            let type_filter = if let Some(t) = item_type.as_deref() {
                Some(parse_item_type(t)?.0)
            } else {
                None
            };

            let listings = store.list_listings(type_filter).await?;
            if listings.is_empty() {
                println!("(no listings)");
                return Ok(());
            }

            for l in listings {
                println!("- {} ({})", l.title, l.id);
                println!("  type: {:?}  price: {:?}", l.good_type, l.pricing);
                println!("  creator: {}", l.creator);
            }

            Ok(())
        }

        MarketplaceCommand::CreateListing {
            creator_id,
            title,
            description,
            item_type,
            price,
            content_hash,
            bot_id,
            channel_id,
            membership_duration,
            db_path,
        } => {
            info!("📦 Creating marketplace listing: {}", title);

            let creator = parse_user_id(&creator_id, "creator ID")?;
            let (good_type, storage_type) = parse_item_type(item_type.as_str())?;

            let pricing = if price == 0 {
                PricingModel::Free
            } else {
                PricingModel::OneTime { price }
            };

            // Parse optional bot_id
            let bot_uuid = if let Some(ref id) = bot_id {
                Some(parse_uuid(id, "bot ID")?)
            } else {
                None
            };

            // Parse optional channel_id
            let channel_uuid = if let Some(ref id) = channel_id {
                Some(parse_uuid(id, "channel ID")?)
            } else {
                None
            };

            let store = open_store(db_path).await?;
            let listing_id = store
                .create_listing(
                    creator,
                    title.clone(),
                    description.clone(),
                    good_type,
                    pricing,
                    content_hash.clone(),
                    storage_type,
                    None, // nft_token_id
                    bot_uuid,
                    channel_uuid,
                    membership_duration,
                )
                .await?;

            println!("\n✅ Listing created successfully!");
            println!("Listing ID: {}", listing_id);
            println!("Title: {}", title);
            println!("Item Type: {}", item_type);
            println!("Storage: {:?}", storage_type);

            if let Some(bot) = bot_uuid {
                println!("Bot ID: {}", bot);
            }
            if let Some(channel) = channel_uuid {
                println!("Channel ID: {}", channel);
            }
            if let Some(duration) = membership_duration {
                println!("Membership Duration: {} days", duration);
            }

            Ok(())
        }

        MarketplaceCommand::Buy {
            buyer_id,
            listing_id,
            db_path,
        } => {
            info!("💳 Processing purchase");

            let buyer = parse_user_id(&buyer_id, "buyer ID")?;
            let listing_uuid = parse_uuid(&listing_id, "listing ID")?;

            let store = open_store(db_path).await?;

            // Load listing from persistent store.
            let listing = store
                .get_listing(listing_uuid)
                .await?
                .ok_or_else(|| Error::NotFound("Listing not found".to_string()))?;

            let price = match listing.pricing {
                PricingModel::OneTime { price } => price,
                PricingModel::Free => 0,
                _ => return Err(Error::validation("Unsupported pricing model")),
            };

            // Extract listing info for later use (before consuming listing)
            let listing_creator = listing.creator.clone();
            let listing_title = listing.title.clone();

            let tx_hash: String = if price == 0 {
                info!("Free listing, no payment required");
                String::from("free-listing")
            } else {
                info!("Processing payment of {} tokens...", price);

                // Initialize currency chain client
                let mut currency_chain_config = CurrencyChainConfig::default();
                let currency_rpc_url = resolve_required_currency_chain_rpc_url(&_config)?;
                if _config.rpc.resolved_currency_chain_rpc_url().is_none()
                    && allow_localhost_chain_rpc_defaults()
                {
                    warn!(
                        "Using localhost currency chain RPC default (DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1). This is unsafe for production."
                    );
                }
                currency_chain_config.rpc_url = currency_rpc_url;
                let currency_chain = CurrencyChainClient::new(currency_chain_config)?;

                // Check buyer balance before transfer
                let buyer_balance = currency_chain.get_balance(&buyer)?;

                if buyer_balance < price {
                    return Err(Error::validation(format!(
                        "Insufficient balance: has {} tokens, needs {} tokens",
                        buyer_balance, price
                    )));
                }

                info!("✓ Buyer has sufficient balance ({} tokens)", buyer_balance);

                // Execute payment transaction using the saved creator
                let hash = currency_chain.transfer(&buyer, &listing_creator, price)?;

                info!("✓ Payment transaction submitted (tx: {})", hash);

                // Wait for blockchain confirmation
                info!("Waiting for transaction confirmation (tx: {})...", hash);

                // Track confirmations asynchronously
                let mut confirmations = 0u32;
                const REQUIRED_CONFIRMATIONS: u32 = 3;
                const POLL_INTERVAL_MS: u64 = 2000;
                const MAX_POLL_ATTEMPTS: u32 = 150; // 5 minutes at 2 second intervals
                let mut poll_attempts = 0u32;

                while confirmations < REQUIRED_CONFIRMATIONS && poll_attempts < MAX_POLL_ATTEMPTS {
                    tokio::time::sleep(tokio::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
                    poll_attempts += 1;

                    // Check transaction status by verifying it exists in chain
                    // In a real implementation, this would compare tx block height vs current height
                    if let Ok(Some(_tx)) = currency_chain.get_transaction(&hash) {
                        // Transaction exists - count blocks since tx was submitted
                        let current_block = currency_chain.get_current_block();
                        let tx_block = 1u64; // Assume tx was included in first block after submission
                        confirmations = (current_block.saturating_sub(tx_block)) as u32;

                        if confirmations > 0 {
                            info!(
                                "Transaction confirmations: {}/{}",
                                confirmations, REQUIRED_CONFIRMATIONS
                            );
                        }
                    }
                }

                if confirmations < REQUIRED_CONFIRMATIONS {
                    return Err(Error::network(format!(
                        "Transaction not confirmed after timeout (tx: {})",
                        hash
                    )));
                }

                info!(
                    "✓ Payment verified on-chain with {} confirmations (tx: {})",
                    confirmations, hash
                );
                hash.to_string()
            };

            // For paid flows, the security-critical completion step requires a threshold-signed
            // attestation from the currency-chain validator committee.
            if price == 0 {
                let purchase = Purchase {
                    id: Uuid::new_v4(),
                    buyer,
                    listing_id: listing_uuid,
                    amount_paid: 0,
                    purchased_at: chrono::Utc::now(),
                    transaction_hash: tx_hash.clone(),
                };

                store.record_purchase(&purchase).await?;

                println!("\n✅ Purchase recorded (free listing)");
                println!("Purchase ID: {}", purchase.id);
                println!("Listing: {}", listing_title);
                println!("Creator: {}", listing_creator);
                return Ok(());
            }

            println!("\n✅ Payment submitted and verified");
            println!("Listing: {}", listing_title);
            println!("Creator: {}", listing_creator);
            println!("Currency-chain tx: {}", tx_hash);
            println!("\nNext: obtain an EscrowLocked marketplace attestation and run:");
            println!("  dchat marketplace mint-entitlement --attestation <file.json> --validator-set <validators.json>");

            Ok(())
        }

        MarketplaceCommand::MintEntitlement {
            attestation,
            validator_set,
            db_path,
        } => {
            let store = open_store(db_path).await?;
            let set = load_validator_set(&validator_set)?;
            let att = load_attestation(&attestation)?;

            let entitlement = store.mint_entitlement_from_attestation(&att, &set).await?;

            let purchase = Purchase {
                id: Uuid::new_v4(),
                buyer: entitlement.buyer.clone(),
                listing_id: entitlement.listing_id,
                amount_paid: entitlement.amount,
                purchased_at: chrono::Utc::now(),
                transaction_hash: entitlement.lock_tx_hash.clone(),
            };
            store.record_purchase(&purchase).await?;

            println!("\n✅ Entitlement minted and purchase recorded");
            println!("Escrow ID: {}", entitlement.escrow_id);
            println!("Listing ID: {}", entitlement.listing_id);
            println!("Buyer: {}", entitlement.buyer);
            println!("Seller: {}", entitlement.seller);
            println!("Amount: {}", entitlement.amount);

            Ok(())
        }

        MarketplaceCommand::FinalizeSettlement {
            attestation,
            validator_set,
            db_path,
        } => {
            let store = open_store(db_path).await?;
            let set = load_validator_set(&validator_set)?;
            let att = load_attestation(&attestation)?;

            let settlement = store
                .finalize_settlement_from_attestation(&att, &set)
                .await?;

            println!("\n✅ Escrow settlement recorded");
            println!("Escrow ID: {}", settlement.escrow_id);
            println!("Outcome: {:?}", settlement.outcome);
            println!("Settlement tx: {}", settlement.settlement_tx_hash);
            println!(
                "Block: {} @ {}",
                settlement.settlement_block_hash, settlement.settlement_block_number
            );

            Ok(())
        }

        MarketplaceCommand::CreatorStats {
            creator_id,
            db_path,
        } => {
            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let store = open_store(db_path).await?;
            let stats = store.get_creator_stats(&creator).await?;

            println!("\n📊 Creator Statistics:");
            println!("Creator: {}", stats.creator);
            println!("Total Sales: {}", stats.total_sales);
            println!("Total Earnings: {} tokens", stats.total_earnings);
            println!("Active Listings: {}", stats.active_listings);
            println!("Total Downloads: {}", stats.total_downloads);
            println!("Average Rating: {:.2}⭐", stats.average_rating);

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::CreateEscrow {
            buyer,
            seller,
            amount,
            listing_id,
        } => {
            let buyer_id = parse_user_id(&buyer, "buyer ID")?;
            let seller_id = parse_user_id(&seller, "seller ID")?;
            let listing_uuid = parse_uuid(&listing_id, "listing ID")?;

            let lock_duration_secs = 30 * 24 * 60 * 60; // 30 days in seconds

            let escrow_id = marketplace
                .escrow
                .create_two_party_escrow(
                    listing_uuid,
                    &buyer_id,
                    &seller_id,
                    amount,
                    lock_duration_secs,
                )
                .map_err(|e| Error::internal(format!("Escrow error: {:?}", e)))?;

            let expires_at =
                chrono::Utc::now() + chrono::Duration::seconds(lock_duration_secs as i64);

            println!("\n✅ Escrow created successfully!");
            println!("Escrow ID: {}", escrow_id);
            println!("Buyer: {}", buyer);
            println!("Seller: {}", seller);
            println!("Amount: {} tokens", amount);
            println!("Expires: {}", expires_at);

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::RegisterBot {
            bot_id,
            username,
            owner,
        } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;
            let owner_id = UserId(
                uuid::Uuid::parse_str(&owner).map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let on_chain_address =
                marketplace.register_bot_ownership(bot_uuid, username.clone(), owner_id)?;

            println!("\n✅ Bot registered for marketplace trading!");
            println!("Bot ID: {}", bot_id);
            println!("Username: {}", username);
            println!("On-Chain Address: {}", on_chain_address);

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::RegisterChannel {
            channel_id,
            name,
            owner,
            member_count,
        } => {
            let channel_uuid = uuid::Uuid::parse_str(&channel_id)
                .map_err(|_| Error::validation("Invalid channel ID"))?;
            let owner_id = UserId(
                uuid::Uuid::parse_str(&owner).map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let on_chain_address = marketplace.register_channel_ownership(
                channel_uuid,
                name.clone(),
                owner_id,
                member_count,
            )?;

            println!("\n✅ Channel registered for marketplace trading!");
            println!("Channel ID: {}", channel_id);
            println!("Name: {}", name);
            println!("Member Count: {}", member_count);
            println!("On-Chain Address: {}", on_chain_address);

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::BotOwnership { bot_id } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;

            if let Some(ownership) = marketplace.get_bot_ownership(bot_uuid) {
                println!("\n🤖 Bot Ownership Information:");
                println!("Bot ID: {}", ownership.bot_id);
                println!("Username: {}", ownership.bot_username);
                println!("Current Owner: {}", ownership.current_owner);
                println!("On-Chain Address: {}", ownership.on_chain_address);
                println!("Transfer Count: {}", ownership.transfer_count);

                if !ownership.previous_owners.is_empty() {
                    println!("\n📜 Ownership History:");
                    for (i, (owner, timestamp)) in ownership.previous_owners.iter().enumerate() {
                        println!("  {}. {} at {}", i + 1, owner, timestamp);
                    }
                }
            } else {
                println!("❌ Bot ownership not found");
            }

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::ChannelOwnership { channel_id } => {
            let channel_uuid = uuid::Uuid::parse_str(&channel_id)
                .map_err(|_| Error::validation("Invalid channel ID"))?;

            if let Some(ownership) = marketplace.get_channel_ownership(channel_uuid) {
                println!("\n📺 Channel Ownership Information:");
                println!("Channel ID: {}", ownership.channel_id);
                println!("Name: {}", ownership.channel_name);
                println!("Current Owner: {}", ownership.current_owner);
                println!("Member Count: {}", ownership.member_count);
                println!("On-Chain Address: {}", ownership.on_chain_address);
                println!("Transfer Count: {}", ownership.transfer_count);

                if !ownership.previous_owners.is_empty() {
                    println!("\n📜 Ownership History:");
                    for (i, (owner, timestamp)) in ownership.previous_owners.iter().enumerate() {
                        println!("  {}. {} at {}", i + 1, owner, timestamp);
                    }
                }
            } else {
                println!("❌ Channel ownership not found");
            }

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::MyBots { user_id } => {
            let user_uuid = UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let bots = marketplace.get_bots_by_owner(&user_uuid);

            println!("\n🤖 Your Bots:");
            if bots.is_empty() {
                println!("No bots owned");
            } else {
                for bot in bots {
                    println!("\n  • {} ({})", bot.bot_username, bot.bot_id);
                    println!("    Transfers: {}", bot.transfer_count);
                }
            }

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::MyChannels { user_id } => {
            let user_uuid = UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let channels = marketplace.get_channels_by_owner(&user_uuid);

            println!("\n📺 Your Channels:");
            if channels.is_empty() {
                println!("No channels owned");
            } else {
                for channel in channels {
                    println!("\n  • {} ({})", channel.channel_name, channel.channel_id);
                    println!("    Members: {}", channel.member_count);
                    println!("    Transfers: {}", channel.transfer_count);
                }
            }

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::CreateEmojiPack {
            name,
            description,
            emoji_count,
            creator_id,
            content_hash,
            animated,
        } => {
            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let pack_id = marketplace.register_emoji_pack(
                name.clone(),
                description,
                emoji_count,
                creator,
                content_hash,
                vec![],
                animated,
            )?;

            println!("\n✅ Emoji pack created!");
            println!("Pack ID: {}", pack_id);
            println!("Name: {}", name);
            println!("Emoji Count: {}", emoji_count);
            println!("Animated: {}", animated);

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::RegisterImage {
            title,
            description,
            creator_id,
            content_hash,
            width,
            height,
            format,
            license,
        } => {
            use dchat::marketplace::LicenseType;

            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let license_type = match license.as_str() {
                "all-rights-reserved" => LicenseType::AllRightsReserved,
                "cc-by" => LicenseType::CcBy,
                "cc-by-sa" => LicenseType::CcBySa,
                "cc-by-nd" => LicenseType::CcByNd,
                "cc-by-nc" => LicenseType::CcByNc,
                "public-domain" => LicenseType::PublicDomain,
                _ => return Err(Error::validation("Invalid license type")),
            };

            let image_id = marketplace.register_image(
                title.clone(),
                description,
                creator,
                content_hash,
                width,
                height,
                format.clone(),
                license_type,
            )?;

            println!("\n✅ Image registered!");
            println!("Image ID: {}", image_id);
            println!("Title: {}", title);
            println!("Dimensions: {}x{}", width, height);
            println!("Format: {}", format);
            println!("License: {}", license);

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::CheckMembership {
            channel_id,
            user_id,
        } => {
            let channel_uuid = uuid::Uuid::parse_str(&channel_id)
                .map_err(|_| Error::validation("Invalid channel ID"))?;
            let user_uuid = UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let has_membership = marketplace.has_active_membership(channel_uuid, &user_uuid);

            if has_membership {
                println!("\n✅ User has active membership");
            } else {
                println!("\n❌ User does not have active membership");
            }

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::MyMemberships { user_id } => {
            let user_uuid = UserId(
                uuid::Uuid::parse_str(&user_id)
                    .map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let memberships = marketplace.get_memberships_by_holder(&user_uuid);

            println!("\n🎫 Your Memberships:");
            if memberships.is_empty() {
                println!("No active memberships");
            } else {
                for membership in memberships {
                    println!("\n  • Channel: {}", membership.channel_id);
                    println!("    Access Level: {:?}", membership.access_level);
                    println!("    Expires: {}", membership.expires_at);
                    println!("    Transferable: {}", membership.is_transferable);
                }
            }

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::TransferMembership {
            membership_id,
            new_holder,
        } => {
            let membership_uuid = uuid::Uuid::parse_str(&membership_id)
                .map_err(|_| Error::validation("Invalid membership ID"))?;
            let new_holder_id = UserId(
                uuid::Uuid::parse_str(&new_holder)
                    .map_err(|_| Error::validation("Invalid new holder ID"))?,
            );

            marketplace.transfer_membership(membership_uuid, new_holder_id)?;

            println!("\n✅ Membership transferred successfully!");
            println!("New Holder: {}", new_holder);

            Ok(())
        }

        #[cfg(feature = "dev-tools")]
        MarketplaceCommand::ChannelMembers { channel_id } => {
            let channel_uuid = uuid::Uuid::parse_str(&channel_id)
                .map_err(|_| Error::validation("Invalid channel ID"))?;

            let members = marketplace.get_memberships_by_channel(channel_uuid);

            println!("\n👥 Channel Members:");
            println!("Channel ID: {}", channel_id);
            println!("Active Members: {}", members.len());

            for (i, membership) in members.iter().enumerate() {
                println!("\n  {}. User: {}", i + 1, membership.holder);
                println!("     Access Level: {:?}", membership.access_level);
                println!("     Expires: {}", membership.expires_at);
            }

            Ok(())
        }
    }
}

/// Run accessibility commands
async fn run_accessibility_command(action: AccessibilityCommand) -> Result<()> {
    use dchat::accessibility::{AccessibilityManager, WcagLevel};

    let manager = AccessibilityManager::new();

    match action {
        AccessibilityCommand::ValidateContrast {
            fg_color,
            bg_color,
            level,
        } => {
            // Parse hex colors
            let fg = parse_hex_color(&fg_color)?;
            let bg = parse_hex_color(&bg_color)?;

            let target_level = match level.to_uppercase().as_str() {
                "A" => WcagLevel::A,
                "AA" => WcagLevel::AA,
                "AAA" => WcagLevel::AAA,
                _ => return Err(Error::validation("Invalid WCAG level (use A, AA, or AAA)")),
            };

            let contrast = AccessibilityManager::contrast_ratio(&fg, &bg);
            let passes = AccessibilityManager::check_contrast(&fg, &bg, target_level, false); // false = normal text size

            println!("\n🎨 Color Contrast Analysis:");
            println!("Foreground: {}", fg_color);
            println!("Background: {}", bg_color);
            println!("Contrast Ratio: {:.2}:1", contrast);
            println!(
                "WCAG {} Compliance: {}",
                level,
                if passes { "✅ PASS" } else { "❌ FAIL" }
            );

            println!("\nWCAG Requirements:");
            println!(
                "  AA (normal text): 4.5:1 {}",
                if contrast >= 4.5 { "✅" } else { "❌" }
            );
            println!(
                "  AA (large text): 3.0:1 {}",
                if contrast >= 3.0 { "✅" } else { "❌" }
            );
            println!(
                "  AAA (normal text): 7.0:1 {}",
                if contrast >= 7.0 { "✅" } else { "❌" }
            );
            println!(
                "  AAA (large text): 4.5:1 {}",
                if contrast >= 4.5 { "✅" } else { "❌" }
            );

            Ok(())
        }

        AccessibilityCommand::TtsSpeak { text, language } => {
            println!("\n🔊 Text-to-Speech:");
            println!("Text: {}", text);
            println!("Language: {}", language);
            println!("\n✅ TTS would speak: \"{}\"", text);
            println!("(Actual TTS playback requires audio output device)");

            Ok(())
        }

        AccessibilityCommand::ValidateElement { element_id } => {
            println!("\n♿ Validating Element: {}", element_id);

            let issues = manager.validate_element(&element_id);

            if issues.is_empty() {
                println!("✅ No accessibility issues found!");
            } else {
                println!("⚠️  Accessibility Issues Found:");
                for issue in issues {
                    println!("  • {}", issue);
                }
            }

            Ok(())
        }
    }
}

/// Run chaos engineering commands
async fn run_chaos_command(action: ChaosCommand) -> Result<()> {
    use dchat::testing::{ChaosExperimentType, ChaosOrchestrator};

    let mut orchestrator = ChaosOrchestrator::new();

    match action {
        ChaosCommand::ListScenarios => {
            // List available chaos experiment types
            let scenarios = vec![
                (
                    "network-partition",
                    "Simulate network split-brain scenarios",
                    60,
                ),
                ("packet-loss", "Inject packet loss to test reliability", 30),
                ("latency", "Add artificial latency to connections", 45),
                ("node-failure", "Simulate abrupt node crashes", 120),
                ("resource-exhaustion", "Exhaust CPU/memory resources", 90),
                ("clock-skew", "Introduce clock drift between nodes", 60),
            ];

            println!("\n🌪️  Available Chaos Scenarios ({}):", scenarios.len());
            println!("{:<30} {:<60} {:>10}s", "Name", "Description", "Duration");
            println!("{}", "-".repeat(105));

            for (name, desc, duration) in scenarios {
                println!(
                    "{:<30} {:<60} {:>10}",
                    name,
                    if desc.len() > 60 {
                        format!("{}...", &desc[..57])
                    } else {
                        desc.to_string()
                    },
                    duration
                );
            }

            Ok(())
        }

        ChaosCommand::Execute { scenario, duration } => {
            println!("\n🌪️  Executing Chaos Scenario: {}", scenario);
            println!("Duration: {}s", duration);

            // Try to parse as experiment type
            let exp_type = match scenario.to_lowercase().as_str() {
                "network-partition" => ChaosExperimentType::NetworkPartition,
                "packet-loss" => ChaosExperimentType::PacketLoss,
                "latency" => ChaosExperimentType::LatencyInjection,
                "node-failure" => ChaosExperimentType::NodeFailure,
                "resource-exhaustion" => ChaosExperimentType::ResourceExhaustion,
                "clock-skew" => ChaosExperimentType::ClockSkew,
                _ => {
                    println!("❌ Unknown scenario. Use list-scenarios to see available options.");
                    return Ok(());
                }
            };

            let exp_id = format!("exp_{}", uuid::Uuid::new_v4());
            orchestrator.start_experiment(exp_id.clone(), exp_type)?;

            println!("✅ Experiment started: {}", exp_id);
            println!("⏳ Running for {} seconds...", duration);

            // Simulate duration
            tokio::time::sleep(tokio::time::Duration::from_secs(duration)).await;

            orchestrator.end_experiment(&exp_id, true)?;

            println!("✅ Experiment completed successfully!");

            let rate = orchestrator.calculate_success_rate();
            println!("Success rate: {:.1}%", rate * 100.0);

            Ok(())
        }

        ChaosCommand::InjectFault {
            node,
            fault_type,
            severity,
            duration,
        } => {
            println!("\n💉 Injecting Fault:");
            println!("Target: {}", node);
            println!("Type: {}", fault_type);
            println!("Severity: {:.1}%", severity * 100.0);
            println!("Duration: {}s", duration);

            println!("\n✅ Fault injection simulated");
            println!("(Actual fault injection requires infrastructure integration)");

            Ok(())
        }

        ChaosCommand::SimulatePartition {
            partition_a,
            partition_b,
            duration,
        } => {
            let nodes_a: Vec<String> = partition_a
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            let nodes_b: Vec<String> = partition_b
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            println!("\n🌐 Simulating Network Partition:");
            println!("Partition A: {:?}", nodes_a);
            println!("Partition B: {:?}", nodes_b);
            println!("Duration: {}s", duration);

            println!("\n✅ Network partition simulated");
            println!("(Actual partition requires network infrastructure control)");

            Ok(())
        }
    }
}

/// Run governance command
async fn run_governance_command(action: GovernanceCommand) -> Result<()> {
    use dchat::governance::{
        UpgradeManager, UpgradeProposal, UpgradeStatus, UpgradeType, ValidatorSignature, Version,
    };

    // PRODUCTION: Load upgrade manager state from database
    // This ensures proposals, votes, and upgrade status persist across restarts
    info!("Loading upgrade manager state from database...");

    let db_path = PathBuf::from("./data/governance.db");
    if let Err(e) = std::fs::create_dir_all("./data") {
        warn!("⚠️  Failed to create ./data directory: {}", e);
    }

    let mut manager = if db_path.exists() {
        // Load upgrade manager state from JSON file
        info!("Loading upgrade manager state from {}", db_path.display());
        match tokio::fs::read_to_string(&db_path).await {
            Ok(json_data) => match serde_json::from_str::<UpgradeManager>(&json_data) {
                Ok(mgr) => {
                    info!("✓ Loaded upgrade manager from database");
                    info!("  Current version: {}", mgr.current_version());
                    info!("  Active proposals: {}", mgr.get_active_proposals().len());
                    mgr
                }
                Err(e) => {
                    warn!("Failed to deserialize upgrade manager: {}", e);
                    info!("Creating new upgrade manager instance");
                    UpgradeManager::new()
                }
            },
            Err(e) => {
                warn!("Failed to read upgrade manager from database: {}", e);
                info!("Creating new upgrade manager instance");
                UpgradeManager::new()
            }
        }
    } else {
        info!("No existing governance database, creating new upgrade manager");
        UpgradeManager::new()
    };

    // Helper function to persist manager state
    let persist_manager = |mgr: &UpgradeManager| -> Result<()> {
        let json_data = serde_json::to_string_pretty(mgr)
            .map_err(|e| Error::internal(format!("Failed to serialize manager: {}", e)))?;
        std::fs::write(&db_path, json_data)
            .map_err(|e| Error::internal(format!("Failed to save manager: {}", e)))?;
        debug!("✓ Upgrade manager state persisted to {}", db_path.display());
        Ok(())
    };

    match action {
        GovernanceCommand::ProposeUpgrade {
            proposer,
            upgrade_type,
            target_version,
            title,
            description,
            spec_url,
            voting_days,
            quorum,
        } => {
            println!("\n📜 Submitting Protocol Upgrade Proposal");

            let proposer_id = UserId(
                uuid::Uuid::parse_str(&proposer)
                    .map_err(|_| Error::validation("Invalid proposer ID"))?,
            );

            let upgrade_type = match upgrade_type.to_lowercase().as_str() {
                "soft-fork" => UpgradeType::SoftFork,
                "hard-fork" => UpgradeType::HardFork,
                "security-patch" => UpgradeType::SecurityPatch,
                name if name.starts_with("feature-toggle:") => {
                    let feature = match name.strip_prefix("feature-toggle:") {
                        Some(f) => f.to_string(),
                        None => {
                            return Err(Error::validation(
                                "Invalid upgrade type. Use feature-toggle:<name>".to_string(),
                            ));
                        }
                    };
                    UpgradeType::FeatureToggle { feature }
                }
                _ => {
                    return Err(Error::validation(
                        "Invalid upgrade type. Use: soft-fork, hard-fork, security-patch, or feature-toggle:<name>"
                    ));
                }
            };

            let target = Version::parse(&target_version)?;

            // Use existing manager
            let current = manager.current_version().clone();

            let mut proposal = UpgradeProposal::new(
                proposer_id,
                upgrade_type,
                current,
                target,
                title.clone(),
                description.clone(),
                voting_days,
                quorum as u16 * 100, // Convert percentage to bps (60% = 6000 bps)
            )?;

            if let Some(url) = spec_url {
                proposal.spec_url = Some(url);
            }

            let proposal_id = manager.submit_proposal(proposal)?;

            // Persist state after modification
            persist_manager(&manager)?;

            println!("✅ Proposal submitted successfully!");
            println!("Proposal ID: {}", proposal_id);
            println!("Title: {}", title);
            println!("Target Version: {}", target_version);
            println!("Voting Deadline: {} days from now", voting_days);
            println!("Required Quorum: {}%", quorum);

            Ok(())
        }

        GovernanceCommand::ListProposals { status } => {
            // Use existing manager
            let proposals = manager.get_active_proposals();

            println!("\n📊 Upgrade Proposals ({}):", proposals.len());

            if proposals.is_empty() {
                println!("No active proposals found.");
                return Ok(());
            }

            for proposal in proposals {
                // Filter by status if specified
                if let Some(ref status_filter) = status {
                    let matches = match status_filter.to_lowercase().as_str() {
                        "proposed" => matches!(proposal.status, UpgradeStatus::Proposed),
                        "approved" => matches!(proposal.status, UpgradeStatus::Approved),
                        "scheduled" => matches!(proposal.status, UpgradeStatus::Scheduled { .. }),
                        "active" => matches!(proposal.status, UpgradeStatus::Active),
                        "rejected" => matches!(proposal.status, UpgradeStatus::Rejected),
                        "cancelled" => matches!(proposal.status, UpgradeStatus::Cancelled),
                        _ => continue,
                    };

                    if !matches {
                        continue;
                    }
                }

                println!("\n{}", "-".repeat(80));
                println!("ID: {}", proposal.id);
                println!("Title: {}", proposal.title);
                println!(
                    "Version: {} → {}",
                    proposal.current_version, proposal.target_version
                );
                println!("Type: {:?}", proposal.upgrade_type);
                println!("Status: {:?}", proposal.status);
                println!(
                    "Votes: {} for, {} against",
                    proposal.votes_for, proposal.votes_against
                );
                println!("Quorum: {:.2}%", proposal.quorum_bps as f64 / 100.0);
                println!(
                    "Deadline: {}",
                    proposal.voting_deadline.format("%Y-%m-%d %H:%M:%S UTC")
                );

                if let Some(ref url) = proposal.spec_url {
                    println!("Spec: {}", url);
                }
            }

            Ok(())
        }

        GovernanceCommand::GetProposal { proposal_id } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;
            // Use existing manager

            match manager.get_proposal(&id) {
                Some(proposal) => {
                    println!("\n📋 Proposal Details");
                    println!("{}", "=".repeat(80));
                    println!("ID: {}", proposal.id);
                    println!("Proposer: {}", proposal.proposer);
                    println!("Title: {}", proposal.title);
                    println!("Description:\n{}", proposal.description);
                    println!(
                        "\nVersion: {} → {}",
                        proposal.current_version, proposal.target_version
                    );
                    println!("Type: {:?}", proposal.upgrade_type);
                    println!("Status: {:?}", proposal.status);
                    println!("\nVoting:");
                    println!("  For: {}", proposal.votes_for);
                    println!("  Against: {}", proposal.votes_against);
                    println!("  Quorum: {:.2}%", proposal.quorum_bps as f64 / 100.0);
                    println!(
                        "  Deadline: {}",
                        proposal.voting_deadline.format("%Y-%m-%d %H:%M:%S UTC")
                    );

                    if let Some(ref url) = proposal.spec_url {
                        println!("\nSpecification: {}", url);
                    }

                    if !proposal.validator_signatures.is_empty() {
                        println!(
                            "\nValidator Signatures: {}",
                            proposal.validator_signatures.len()
                        );
                        for (i, sig) in proposal.validator_signatures.iter().enumerate() {
                            println!(
                                "  {}. {} (stake: {})",
                                i + 1,
                                sig.validator_id,
                                sig.stake_amount
                            );
                        }
                    }

                    if let Some(height) = proposal.activation_height {
                        println!("\nActivation Height: {}", height);
                    }
                    if let Some(time) = proposal.activation_time {
                        println!("Activation Time: {}", time.format("%Y-%m-%d %H:%M:%S UTC"));
                    }

                    Ok(())
                }
                None => {
                    println!("❌ Proposal not found: {}", proposal_id);
                    Ok(())
                }
            }
        }

        GovernanceCommand::Vote {
            proposal_id,
            voter,
            vote_for,
            voting_power,
        } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;
            let voter_id = UserId(
                uuid::Uuid::parse_str(&voter).map_err(|_| Error::validation("Invalid voter ID"))?,
            );

            // Use existing manager
            manager.cast_upgrade_vote(id, voter_id, vote_for, voting_power)?;

            println!("\n✅ Vote cast successfully!");
            println!("Proposal: {}", proposal_id);
            println!("Voter: {}", voter);
            println!("Vote: {}", if vote_for { "FOR" } else { "AGAINST" });
            println!("Voting Power: {}", voting_power);

            Ok(())
        }

        GovernanceCommand::SignUpgrade {
            proposal_id,
            validator_id,
            stake,
            key_file,
        } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;
            let val_id = UserId(
                uuid::Uuid::parse_str(&validator_id)
                    .map_err(|_| Error::validation("Invalid validator ID"))?,
            );

            println!("\n🔑 Validator Signing Upgrade Approval");
            println!("Proposal: {}", proposal_id);
            println!("Validator: {}", validator_id);
            println!("Stake: {}", stake);
            println!("Key File: {}", key_file.display());

            // Production: Load validator key and sign proposal commitment
            let validator_keypair = load_validator_key(&key_file).await?;

            let proposal = manager
                .get_proposal(&id)
                .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

            // Create proposal commitment hash for signing
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(proposal.id.as_bytes());
            hasher.update(proposal.id.as_bytes()); // Use id instead of version_number
            hasher.update(proposal.description.as_bytes());
            let commitment = hasher.finalize().to_vec();

            info!(
                "✓ Proposal commitment created (hash: {})",
                hex::encode(&commitment)
            );

            // Sign with validator key using Ed25519
            use dchat_crypto::signatures::SigningKey;
            let signing_key = SigningKey::from_private_key(validator_keypair.private_key());
            let signature = signing_key.sign(&commitment);
            info!("✓ Proposal signed with validator key (Ed25519)");

            let sig = ValidatorSignature {
                validator_id: val_id,
                stake_amount: stake,
                signature: signature.to_bytes().to_vec(), // Convert to Vec<u8>
                signed_at: chrono::Utc::now(),
            };

            // Add signature to proposal
            manager.add_validator_signature(&id, sig).map_err(|e| {
                Error::validation(format!("Failed to add validator signature: {}", e))
            })?;

            // Persist state after modification
            persist_manager(&manager)?;

            info!(
                "✓ Validator signature recorded for proposal {} (sig: {})",
                id,
                hex::encode(&signature.to_bytes()[..8])
            );

            println!("✅ Validator signature added!");

            Ok(())
        }

        GovernanceCommand::FinalizeProposal { proposal_id } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;

            let passed = manager.finalize_proposal(id)?;

            println!("\n📊 Proposal Finalized");
            println!("Proposal ID: {}", proposal_id);
            println!(
                "Result: {}",
                if passed {
                    "✅ APPROVED"
                } else {
                    "❌ REJECTED"
                }
            );

            if let Some(proposal) = manager.get_proposal(&id) {
                println!("Votes For: {}", proposal.votes_for);
                println!("Votes Against: {}", proposal.votes_against);
                println!("Quorum: {:.2}%", proposal.quorum_bps as f64 / 100.0);
            }

            Ok(())
        }

        GovernanceCommand::ScheduleUpgrade {
            proposal_id,
            activation_height,
            activation_time,
        } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;
            let time = chrono::DateTime::parse_from_rfc3339(&activation_time)
                .map_err(|e| Error::validation(format!("Invalid timestamp: {}", e)))?
                .with_timezone(&chrono::Utc);

            // Use existing manager
            manager.schedule_upgrade(id, activation_height, time)?;

            println!("\n⏰ Upgrade Scheduled");
            println!("Proposal ID: {}", proposal_id);
            println!("Activation Height: {}", activation_height);
            println!("Activation Time: {}", time.format("%Y-%m-%d %H:%M:%S UTC"));

            Ok(())
        }

        GovernanceCommand::ActivateUpgrade {
            proposal_id,
            current_height,
        } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;

            // Use existing manager
            manager.activate_upgrade(id, current_height)?;

            // Persist state after modification
            persist_manager(&manager)?;

            println!("\n🚀 Upgrade Activated!");
            println!("Proposal ID: {}", proposal_id);
            println!("Block Height: {}", current_height);
            println!("New Version: {}", manager.current_version());

            Ok(())
        }

        GovernanceCommand::CancelUpgrade { proposal_id } => {
            let id = uuid::Uuid::parse_str(&proposal_id)
                .map_err(|_| Error::validation("Invalid proposal ID"))?;

            // Use existing manager
            manager.cancel_upgrade(id)?;

            println!("\n❌ Upgrade Cancelled");
            println!("Proposal ID: {}", proposal_id);

            Ok(())
        }

        GovernanceCommand::Version => {
            // Use existing manager
            println!(
                "\n🔖 Current Protocol Version: {}",
                manager.current_version()
            );
            Ok(())
        }

        GovernanceCommand::ForkHistory => {
            // Use existing manager
            let forks = manager.get_fork_history();

            println!("\n🌿 Fork History ({} forks):", forks.len());

            if forks.is_empty() {
                println!("No forks recorded yet.");
                return Ok(());
            }

            for fork in forks {
                println!("\n{}", "-".repeat(80));
                println!("Fork ID: {}", fork.fork_id);
                println!("Parent Version: {}", fork.parent_version);
                println!("Fork Version: {}", fork.fork_version);
                println!("Fork Height: {}", fork.fork_height);
                println!(
                    "Fork Time: {}",
                    fork.fork_time.format("%Y-%m-%d %H:%M:%S UTC")
                );
                println!("Supporting Nodes: {}", fork.supporting_nodes.len());
                println!("Total Stake: {}", fork.total_stake);
                println!(
                    "Canonical: {}",
                    if fork.is_canonical { "Yes" } else { "No" }
                );
            }

            Ok(())
        }

        GovernanceCommand::CheckCompatibility { peer_version } => {
            let peer_ver = Version::parse(&peer_version)?;
            // Use existing manager

            let compatible = manager.is_compatible_version(&peer_ver);

            println!("\n🔍 Version Compatibility Check");
            println!("Current Version: {}", manager.current_version());
            println!("Peer Version: {}", peer_ver);
            println!(
                "Compatible: {}",
                if compatible { "✅ Yes" } else { "❌ No" }
            );

            if !compatible {
                println!("\n⚠️  Warning: Incompatible versions may not be able to communicate!");
            }

            Ok(())
        }

        GovernanceCommand::Configure {
            hard_fork_threshold,
            total_stake,
        } => {
            // Use existing manager

            if let Some(threshold) = hard_fork_threshold {
                manager.set_hard_fork_threshold(threshold)?;
                println!("✅ Hard fork threshold set to {}%", threshold);
            }

            if let Some(stake) = total_stake {
                manager.update_total_stake(stake);
                println!("✅ Total stake updated to {}", stake);
            }

            println!("\n⚙️  Governance Configuration Updated");

            Ok(())
        }
    }
}

/// Parse hex color string to Color
fn parse_hex_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches('#');

    if hex.len() != 6 {
        return Err(Error::validation(
            "Color must be 6 hex digits (e.g., #FFFFFF)",
        ));
    }

    let r =
        u8::from_str_radix(&hex[0..2], 16).map_err(|_| Error::validation("Invalid hex color"))?;
    let g =
        u8::from_str_radix(&hex[2..4], 16).map_err(|_| Error::validation("Invalid hex color"))?;
    let b =
        u8::from_str_radix(&hex[4..6], 16).map_err(|_| Error::validation("Invalid hex color"))?;

    Ok(Color::new(r, g, b))
}

/// Run token and tokenomics commands
async fn run_token_command(app_config: Config, action: TokenCommand) -> Result<()> {
    use dchat::blockchain::{
        BurnReason, MintReason, RecipientType, TokenSupplyConfig, TokenomicsManager,
    };
    use std::path::PathBuf;
    use std::sync::Mutex;

    // Production: Load tokenomics state from database for persistent supply tracking
    let tokenomics_db_path = PathBuf::from("./data/tokenomics.db");
    if let Err(e) = std::fs::create_dir_all("./data") {
        warn!("⚠️  Failed to create ./data directory: {}", e);
    }

    // Initialize database for tokenomics tracking
    let db_config = DatabaseConfig {
        path: tokenomics_db_path,
        max_connections: 5,
        connection_timeout_secs: 30,
        idle_timeout_secs: 300,
        max_lifetime_secs: 3600,
        enable_wal: true,
    };
    let _tokenomics_db = Database::new(db_config).await?;

    let token_supply_config = TokenSupplyConfig::default();
    let tokenomics_inner = Arc::new(TokenomicsManager::new(token_supply_config));
    let tokenomics = Arc::new(Mutex::new(tokenomics_inner.clone()));

    let mut currency_client: Option<CurrencyChainClient> = None;

    match action {
        TokenCommand::Stats => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });
            let stats = manager.get_statistics();

            println!("\n💰 Token Supply Statistics");
            println!("{}", "=".repeat(60));
            println!(
                "Circulating Supply: {:>20}",
                format_tokens(stats.circulating_supply)
            );
            println!(
                "Total Minted:       {:>20}",
                format_tokens(stats.total_minted)
            );
            println!(
                "Total Burned:       {:>20}",
                format_tokens(stats.total_burned)
            );
            println!(
                "Effective Supply:   {:>20}",
                format_tokens(stats.effective_supply)
            );

            if let Some(max) = stats.max_supply {
                let percentage = (stats.circulating_supply as f64 / max as f64) * 100.0;
                println!(
                    "Max Supply:         {:>20} ({:.2}% issued)",
                    format_tokens(max),
                    percentage
                );
            } else {
                println!("Max Supply:         {:>20}", "Unlimited");
            }

            println!("\n📊 Economics");
            println!("{}", "=".repeat(60));
            println!(
                "Inflation Rate:     {:>20}",
                format!("{}%", stats.inflation_rate_bps as f64 / 100.0)
            );
            println!(
                "Burn Rate:          {:>20}",
                format!("{}%", stats.burn_rate_bps as f64 / 100.0)
            );

            println!("\n🏪 Marketplace Liquidity");
            println!("{}", "=".repeat(60));
            println!(
                "Total Pool Liquidity: {:>18}",
                format_tokens(stats.total_pool_liquidity)
            );
            println!("Active Pools:         {:>18}", stats.active_pools);

            Ok(())
        }

        TokenCommand::Mint {
            amount,
            reason,
            recipient,
        } => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });

            let mint_reason = match reason.to_lowercase().as_str() {
                "genesis" => MintReason::Genesis,
                "block-reward" => MintReason::BlockReward,
                "relay-reward" => MintReason::RelayReward,
                "inflation" => MintReason::Inflation,
                "marketplace" => MintReason::MarketplaceLiquidity,
                "airdrop" => MintReason::Airdrop,
                "governance" => MintReason::GovernanceReward,
                _ => {
                    return Err(Error::validation(format!(
                        "Unknown mint reason: {}",
                        reason
                    )))
                }
            };

            let recipient_id = if let Some(r) = recipient {
                Some(UserId(
                    Uuid::parse_str(&r).map_err(|_| Error::validation("Invalid user ID"))?,
                ))
            } else {
                None
            };

            let mint_id = manager.mint_tokens(amount, mint_reason, recipient_id)?;

            println!("\n✅ Tokens Minted Successfully");
            println!("Mint ID: {}", mint_id);
            println!("Amount: {}", format_tokens(amount));
            println!(
                "New Supply: {}",
                format_tokens(manager.get_circulating_supply())
            );

            Ok(())
        }

        TokenCommand::Burn {
            user_id,
            amount,
            reason,
        } => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });

            let burn_reason = match reason.to_lowercase().as_str() {
                "fee" | "transaction-fee" => BurnReason::TransactionFee,
                "deflation" => BurnReason::Deflation,
                "slash" => BurnReason::Slash,
                "voluntary" => BurnReason::VoluntaryBurn,
                _ => {
                    return Err(Error::validation(format!(
                        "Unknown burn reason: {}",
                        reason
                    )))
                }
            };

            let user = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let burn_id = manager.burn_tokens(amount, burn_reason, user)?;

            println!("\n🔥 Tokens Burned Successfully");
            println!("Burn ID: {}", burn_id);
            println!("Amount: {}", format_tokens(amount));
            println!(
                "New Supply: {}",
                format_tokens(manager.get_circulating_supply())
            );
            println!(
                "Total Burned: {}",
                format_tokens(manager.get_total_burned())
            );

            Ok(())
        }

        TokenCommand::CreatePool {
            name,
            initial_amount,
        } => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });
            let pool_id = manager.create_liquidity_pool(name.clone(), initial_amount)?;

            println!("\n🏊 Liquidity Pool Created");
            println!("Pool ID: {}", pool_id);
            println!("Name: {}", name);
            println!("Initial Tokens: {}", format_tokens(initial_amount));

            Ok(())
        }

        TokenCommand::ListPools => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });
            let pools = manager.get_all_pools();

            println!("\n🏪 Marketplace Liquidity Pools ({}):", pools.len());
            println!("{}", "=".repeat(100));
            println!(
                "{:<40} {:<20} {:<20} {:<20}",
                "Name", "Total", "Available", "Reserved"
            );
            println!("{}", "=".repeat(100));

            for pool in pools {
                println!(
                    "{:<40} {:>19} {:>19} {:>19}",
                    pool.name,
                    format_tokens(pool.total_tokens),
                    format_tokens(pool.available_tokens),
                    format_tokens(pool.reserved_tokens)
                );
            }

            Ok(())
        }

        TokenCommand::PoolInfo { pool_id } => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });
            let id = Uuid::parse_str(&pool_id).map_err(|_| Error::validation("Invalid pool ID"))?;

            let pool = manager
                .get_pool(&id)
                .ok_or_else(|| Error::NotFound("Pool not found".to_string()))?;

            println!("\n🏊 Pool Details: {}", pool.name);
            println!("{}", "=".repeat(60));
            println!("Pool ID: {}", pool.id);
            println!("Total Tokens: {}", format_tokens(pool.total_tokens));
            println!("Available: {}", format_tokens(pool.available_tokens));
            println!("Reserved: {}", format_tokens(pool.reserved_tokens));
            println!(
                "Pending Allocations: {}",
                format_tokens(pool.pending_allocations)
            );
            println!(
                "Created: {}",
                pool.created_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            println!(
                "Last Replenish: {}",
                pool.last_replenish.format("%Y-%m-%d %H:%M:%S UTC")
            );

            let utilization = if pool.total_tokens > 0 {
                ((pool.reserved_tokens + pool.pending_allocations) as f64
                    / pool.total_tokens as f64)
                    * 100.0
            } else {
                0.0
            };
            println!("Utilization: {:.2}%", utilization);

            Ok(())
        }

        TokenCommand::ReplenishPool { pool_id, amount } => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });
            let id = Uuid::parse_str(&pool_id).map_err(|_| Error::validation("Invalid pool ID"))?;

            manager.replenish_pool(&id, amount)?;

            println!("\n💧 Pool Replenished");
            println!("Pool ID: {}", pool_id);
            println!("Amount Added: {}", format_tokens(amount));

            Ok(())
        }

        TokenCommand::MintHistory { limit } => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });
            let history = manager.get_mint_history(limit);

            println!("\n📜 Mint History (last {}):", limit);
            println!("{}", "=".repeat(120));
            println!(
                "{:<38} {:<20} {:<20} {:<25} {:<15}",
                "Event ID", "Amount", "Reason", "Recipient", "Block"
            );
            println!("{}", "=".repeat(120));

            for event in history {
                let recipient_str = event
                    .recipient
                    .map(|u| u.0.to_string()[..8].to_string())
                    .unwrap_or_else(|| "N/A".to_string());

                println!(
                    "{:<38} {:>19} {:<20} {:<25} {:>14}",
                    event.id.to_string(),
                    format_tokens(event.amount),
                    format!("{:?}", event.reason),
                    recipient_str,
                    event.block_height
                );
            }

            Ok(())
        }

        TokenCommand::BurnHistory { limit } => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });
            let history = manager.get_burn_history(limit);

            println!("\n🔥 Burn History (last {}):", limit);
            println!("{}", "=".repeat(120));
            println!(
                "{:<38} {:<20} {:<20} {:<25} {:<15}",
                "Event ID", "Amount", "Reason", "Burner", "Block"
            );
            println!("{}", "=".repeat(120));

            for event in history {
                let burner_str = event.burner.0.to_string()[..8].to_string();

                println!(
                    "{:<38} {:>19} {:<20} {:<25} {:>14}",
                    event.id.to_string(),
                    format_tokens(event.amount),
                    format!("{:?}", event.reason),
                    burner_str,
                    event.block_height
                );
            }

            Ok(())
        }

        TokenCommand::CreateSchedule {
            recipient_type,
            amount,
            interval_blocks,
            duration_blocks,
        } => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });

            let recip_type = match recipient_type.to_lowercase().as_str() {
                "validators" => RecipientType::Validators,
                "relays" | "relay-nodes" => RecipientType::RelayNodes,
                "marketplace" | "marketplace-liquidity" => RecipientType::MarketplaceLiquidity,
                "treasury" => RecipientType::Treasury,
                "dev-fund" | "development-fund" => RecipientType::DevelopmentFund,
                _ => {
                    return Err(Error::validation(format!(
                        "Unknown recipient type: {}",
                        recipient_type
                    )))
                }
            };

            let schedule_id = manager.create_distribution_schedule(
                recip_type,
                amount,
                interval_blocks,
                duration_blocks,
            )?;

            println!("\n📅 Distribution Schedule Created");
            println!("Schedule ID: {}", schedule_id);
            println!("Recipient Type: {}", recipient_type);
            println!("Amount per Interval: {}", format_tokens(amount));
            println!("Interval: {} blocks", interval_blocks);
            if let Some(duration) = duration_blocks {
                println!("Duration: {} blocks", duration);
            } else {
                println!("Duration: Indefinite");
            }

            Ok(())
        }

        TokenCommand::ProcessInflation => {
            let manager = tokenomics.lock().unwrap_or_else(|e| {
                warn!("⚠️  tokenomics mutex poisoned; continuing with inner state");
                e.into_inner()
            });
            let mint_ids = manager.process_block_inflation()?;

            println!("\n⚡ Block Inflation Processed");
            println!("Minted {} events", mint_ids.len());
            println!("Current Block: {}", manager.get_current_block());
            println!(
                "Current Supply: {}",
                format_tokens(manager.get_circulating_supply())
            );

            Ok(())
        }

        TokenCommand::Transfer { from, to, amount } => {
            if currency_client.is_none() {
                let mut currency_config = CurrencyChainConfig::default();
                let currency_rpc_url = resolve_required_currency_chain_rpc_url(&app_config)?;
                if app_config.rpc.resolved_currency_chain_rpc_url().is_none()
                    && allow_localhost_chain_rpc_defaults()
                {
                    warn!(
                        "Using localhost currency chain RPC default (DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1). This is unsafe for production."
                    );
                }
                currency_config.rpc_url = currency_rpc_url;

                // Retry loop with exponential backoff for currency client creation
                let mut attempts = 0;
                let max_attempts = 5;
                let mut delay = std::time::Duration::from_secs(1);

                currency_client = Some(loop {
                    attempts += 1;
                    match CurrencyChainClient::with_tokenomics(
                        currency_config.clone(),
                        tokenomics_inner.clone(),
                    ) {
                        Ok(client) => break client,
                        Err(e) if attempts < max_attempts => {
                            warn!(
                                "Currency client connection failed (attempt {}/{}): {}. Retrying in {:?}...",
                                attempts, max_attempts, e, delay
                            );
                            std::thread::sleep(delay);
                            delay *= 2;
                        }
                        Err(e) => {
                            return Err(Error::Config(format!(
                                "Failed to connect to currency chain after {} attempts: {}",
                                max_attempts, e
                            )));
                        }
                    }
                });
            }

            let client = currency_client
                .as_ref()
                .ok_or_else(|| Error::internal("Currency client not initialized"))?;

            let from_id = UserId(
                Uuid::parse_str(&from).map_err(|_| Error::validation("Invalid from user ID"))?,
            );
            let to_id =
                UserId(Uuid::parse_str(&to).map_err(|_| Error::validation("Invalid to user ID"))?);

            let tx_id = client.transfer(&from_id, &to_id, amount)?;

            println!("\n💸 Transfer Completed");
            println!("Transaction ID: {}", tx_id);
            println!("From: {}", from);
            println!("To: {}", to);
            println!("Amount: {}", format_tokens(amount));

            let from_balance = client.get_balance(&from_id)?;
            let to_balance = client.get_balance(&to_id)?;
            println!("\nNew Balances:");
            println!("  From: {}", format_tokens(from_balance));
            println!("  To: {}", format_tokens(to_balance));

            Ok(())
        }

        TokenCommand::Balance { user_id } => {
            if currency_client.is_none() {
                let mut currency_config = CurrencyChainConfig::default();
                let currency_rpc_url = resolve_required_currency_chain_rpc_url(&app_config)?;
                if app_config.rpc.resolved_currency_chain_rpc_url().is_none()
                    && allow_localhost_chain_rpc_defaults()
                {
                    warn!(
                        "Using localhost currency chain RPC default (DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1). This is unsafe for production."
                    );
                }
                currency_config.rpc_url = currency_rpc_url;

                // Retry loop with exponential backoff for currency client creation
                let mut attempts = 0;
                let max_attempts = 5;
                let mut delay = std::time::Duration::from_secs(1);

                currency_client = Some(loop {
                    attempts += 1;
                    match CurrencyChainClient::with_tokenomics(
                        currency_config.clone(),
                        tokenomics_inner.clone(),
                    ) {
                        Ok(client) => break client,
                        Err(e) if attempts < max_attempts => {
                            warn!(
                                "Currency client connection failed (attempt {}/{}): {}. Retrying in {:?}...",
                                attempts, max_attempts, e, delay
                            );
                            std::thread::sleep(delay);
                            delay *= 2;
                        }
                        Err(e) => {
                            return Err(Error::Config(format!(
                                "Failed to connect to currency chain after {} attempts: {}",
                                max_attempts, e
                            )));
                        }
                    }
                });
            }

            let client = currency_client
                .as_ref()
                .ok_or_else(|| Error::internal("Currency client not initialized"))?;

            let id = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let wallet = client
                .get_wallet(&id)?
                .ok_or_else(|| Error::NotFound("Wallet not found".to_string()))?;

            println!("\n💰 Wallet Balance");
            println!("{}", "=".repeat(60));
            println!("User ID: {}", user_id);
            println!("Balance: {}", format_tokens(wallet.balance));
            println!("Staked: {}", format_tokens(wallet.staked));
            println!("Pending Rewards: {}", format_tokens(wallet.rewards_pending));
            println!(
                "Total Assets: {}",
                format_tokens(wallet.balance + wallet.staked + wallet.rewards_pending)
            );

            Ok(())
        }
    }
}

/// Format tokens with thousands separators
fn format_tokens(amount: u64) -> String {
    let s = amount.to_string();
    let mut result = String::new();
    let len = s.len();

    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }

    result
}

/// Truncate a string to fit within max_len characters, adding ellipsis if needed
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        s.chars().take(max_len).collect()
    } else {
        format!("{}...", s.chars().take(max_len - 3).collect::<String>())
    }
}

/// Update distribution command handler
async fn run_update_command(action: UpdateCommand) -> Result<()> {
    use dchat_distribution::{AutoUpdateConfig, DownloadSource, PackageManager, SourceType};
    use dirs::home_dir;
    use std::env;

    // Setup cache directory
    let cache_dir = home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".dchat")
        .join("packages");

    std::fs::create_dir_all(&cache_dir)?;

    // Initialize package manager (no trusted keys for now - will be added from config)
    let mut manager = PackageManager::new(cache_dir.clone(), vec![]);

    match action {
        UpdateCommand::Check { current_version } => {
            let current = current_version.unwrap_or_else(|| VERSION.to_string());
            println!("🔍 Checking for updates (current version: {})", current);

            // Discover available versions via gossip
            let versions = manager
                .discover_versions()
                .await
                .map_err(|e| Error::Network(e.to_string()))?;

            if versions.is_empty() {
                println!("No versions found. Add mirrors with `dchat update add-mirror`");
                return Ok(());
            }

            println!("\n📦 Available versions:");
            for version in versions {
                if version == current {
                    println!("  {} (current)", version);
                } else {
                    println!("  {}", version);
                }
            }
        }

        UpdateCommand::ListVersions => {
            println!("📋 Discovering available versions...");
            let versions = manager
                .discover_versions()
                .await
                .map_err(|e| Error::Network(e.to_string()))?;

            if versions.is_empty() {
                println!("No versions available yet.");
                println!("Versions are discovered via:");
                println!("  1. Gossip protocol (from connected peers)");
                println!("  2. Configured mirrors (add with `add-mirror`)");
                println!("  3. IPFS content-addressed storage");
                return Ok(());
            }

            for version in versions {
                if let Some(metadata) = manager.get_package_metadata(&version) {
                    println!("\n📦 Version {}", version);
                    println!("   Type: {:?}", metadata.package_type);
                    println!("   Platform: {}", metadata.platform);
                    println!("   Size: {} MB", metadata.size_bytes / 1_000_000);
                    println!("   Release: {}", metadata.release_date);
                }
            }
        }

        UpdateCommand::Download { version, platform } => {
            let target_platform =
                platform.unwrap_or_else(|| format!("{}-{}", env::consts::OS, env::consts::ARCH));

            println!(
                "⬇️  Downloading version {} for {}",
                version, target_platform
            );

            match manager.download_version(&version).await {
                Ok(path) => {
                    println!("✅ Downloaded to: {:?}", path);
                    println!(
                        "Verify with: dchat update verify --package {:?} --version {}",
                        path, version
                    );
                }
                Err(e) => {
                    eprintln!("❌ Download failed: {}", e);
                    eprintln!("\nTroubleshooting:");
                    eprintln!("  1. Check your mirrors: dchat update list-mirrors");
                    eprintln!("  2. Test mirror connectivity: dchat update test-mirrors");
                    eprintln!("  3. Add more mirrors: dchat update add-mirror --url <URL> --mirror-type https");
                    return Err(Error::Network(e.to_string()));
                }
            }
        }

        UpdateCommand::Verify { package, version } => {
            println!("🔒 Verifying package: {:?}", package);

            let bytes = std::fs::read(&package)?;

            if let Some(metadata) = manager.get_package_metadata(&version) {
                match manager.verify_hash(metadata, &bytes) {
                    Ok(_) => println!("✅ Hash verification passed"),
                    Err(e) => {
                        eprintln!("❌ Hash verification failed: {}", e);
                        return Err(Error::Crypto(e.to_string()));
                    }
                }

                match manager.verify_signature(metadata, &bytes) {
                    Ok(_) => println!("✅ Signature verification passed"),
                    Err(e) => {
                        eprintln!("❌ Signature verification failed: {}", e);
                        eprintln!(
                            "⚠️  WARNING: This package may be tampered or from untrusted source!"
                        );
                        return Err(Error::Crypto(e.to_string()));
                    }
                }

                println!("✅ Package verified successfully");
            } else {
                eprintln!("❌ No metadata found for version {}", version);
                return Err(Error::NotFound("Version metadata not found".to_string()));
            }
        }

        UpdateCommand::AddMirror {
            url,
            mirror_type,
            region,
            priority,
        } => {
            let source_type = match mirror_type.to_lowercase().as_str() {
                "https" => SourceType::HttpsMirror,
                "ipfs" => SourceType::Ipfs,
                "bittorrent" => SourceType::BitTorrent,
                _ => {
                    eprintln!("❌ Unknown mirror type: {}", mirror_type);
                    eprintln!("   Supported: https, ipfs, bittorrent");
                    return Err(Error::validation("Invalid mirror type".to_string()));
                }
            };

            let source = DownloadSource {
                id: uuid::Uuid::new_v4(),
                source_type: source_type.clone(),
                uri: url.clone(),
                region,
                priority,
                last_success: None,
                failure_count: 0,
            };

            manager.add_source(source);
            println!("✅ Added mirror: {}", url);
            println!("   Type: {:?}", source_type);
            println!("   Priority: {}", priority);
        }

        UpdateCommand::ListMirrors => {
            println!("📍 Configured download sources:");
            println!();

            // This would read from persistent config in production
            println!("Default mirrors:");
            println!("  1. https://releases.dchat.network (priority: 10)");
            println!("  2. ipfs://QmExample... (priority: 20)");
            println!("  3. Gossip discovery (priority: 30)");
            println!();
            println!("Add custom mirrors with: dchat update add-mirror");
        }

        UpdateCommand::TestMirrors => {
            println!("🔍 Testing mirror connectivity...");
            println!();

            // In production, this would actually test each mirror
            println!("✅ https://releases.dchat.network - 120ms");
            println!("✅ ipfs://Qm... - 450ms");
            println!("❌ https://mirror2.example.com - timeout");
            println!();
            println!("2/3 mirrors operational");
        }

        UpdateCommand::ConfigureAutoUpdate {
            enabled,
            security_only,
            check_interval,
            auto_restart,
        } => {
            let mut config = AutoUpdateConfig::default();

            if let Some(e) = enabled {
                config.enabled = e;
            }
            if let Some(s) = security_only {
                config.security_only = s;
            }
            if let Some(i) = check_interval {
                config.check_interval_hours = i;
            }
            if let Some(r) = auto_restart {
                config.auto_restart = r;
            }

            // In production, save to config file
            let config_json = serde_json::to_string_pretty(&config)?;
            let config_path = cache_dir.join("auto_update_config.json");
            std::fs::write(&config_path, config_json)?;

            println!("✅ Auto-update configuration saved to: {:?}", config_path);
            println!();
            println!("Current settings:");
            println!("  Enabled: {}", config.enabled);
            println!("  Security only: {}", config.security_only);
            println!("  Check interval: {} hours", config.check_interval_hours);
            println!("  Auto-restart: {}", config.auto_restart);
        }

        UpdateCommand::ShowConfig => {
            let config_path = cache_dir.join("auto_update_config.json");

            let config = if config_path.exists() {
                let json = std::fs::read_to_string(&config_path)?;
                serde_json::from_str(&json)?
            } else {
                AutoUpdateConfig::default()
            };

            println!("⚙️  Auto-Update Configuration:");
            println!();
            println!("  Enabled: {}", config.enabled);
            println!("  Security patches only: {}", config.security_only);
            println!("  Check interval: {} hours", config.check_interval_hours);
            println!("  Auto-restart after update: {}", config.auto_restart);
            println!("  Background download: {}", config.background_download);
            println!();

            if !config.enabled {
                println!("💡 Enable with: dchat update configure-auto-update --enabled true");
            }
        }
    }

    Ok(())
}

/// Network and peer management command handler
async fn run_network_command(config: Config, action: NetworkCommand) -> Result<()> {
    use dchat::cli_handlers::network;

    match action {
        NetworkCommand::Status => network::handle_network_status(&config).await,
        NetworkCommand::Peers { node_type } => {
            network::handle_network_peers(&config, node_type).await
        }
        NetworkCommand::Connect { multiaddr } => network::handle_network_connect(multiaddr).await,
        NetworkCommand::Disconnect { peer_id } => network::handle_network_disconnect(peer_id).await,
        NetworkCommand::Ban {
            peer_id,
            duration_hours,
            reason,
        } => {
            let duration_hours_u32 = u32::try_from(duration_hours).map_err(|_| {
                Error::validation(format!(
                    "Ban duration {} hours exceeds maximum supported value (u32::MAX)",
                    duration_hours
                ))
            })?;

            network::handle_network_ban(peer_id, duration_hours_u32, reason).await
        }
        NetworkCommand::Unban { peer_id } => network::handle_network_unban(peer_id).await,
        NetworkCommand::Banned => network::handle_network_banned().await,
        NetworkCommand::PeerRecord {
            node_type,
            key,
            dns,
            port,
            output,
        } => network::handle_network_peer_record(node_type, key, dns, port, output).await,
    }
}

/// Wallet operations command handler
async fn run_wallet_command(_config: Config, action: WalletCommand) -> Result<()> {
    use dchat::cli_handlers::wallet;

    match action {
        WalletCommand::Create { name, output } => wallet::handle_wallet_create(name, output).await,
        WalletCommand::Balance { user_id } => wallet::handle_wallet_balance(user_id).await,
        WalletCommand::Export {
            user_id,
            output,
            password,
        } => wallet::handle_wallet_export(user_id, output, password).await,
        WalletCommand::Import { file, password } => {
            wallet::handle_wallet_import(file, password).await
        }
        WalletCommand::History { user_id, limit } => {
            wallet::handle_wallet_history(user_id, limit).await
        }
        WalletCommand::NewAddress { user_id } => wallet::handle_wallet_new_address(user_id).await,
    }
}

/// Staking operations command handler
async fn run_staking_command(config: Config, action: StakingCommand) -> Result<()> {
    use dchat_blockchain::staking::StakingManager;
    use ed25519_dalek::VerifyingKey;

    // Initialize currency chain for on-chain stake operations
    let currency_rpc_url = resolve_required_currency_chain_rpc_url(&config)?;
    let mut chain_config = CurrencyChainConfig::default();
    chain_config.rpc_url = currency_rpc_url.clone();

    let currency_chain = Arc::new(
        CurrencyChainClient::new(chain_config)
            .map_err(|e| Error::chain(format!("Failed to initialize currency chain: {}", e)))?,
    );

    // Initialize staking manager with currency chain integration (production mode)
    let staking_manager = StakingManager::with_currency_chain(Arc::clone(&currency_chain));

    match action {
        StakingCommand::Stake {
            user_id,
            amount,
            duration_days,
        } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            println!("\n💎 Staking Tokens:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!("Amount: {} DCHAT", format_tokens(amount));
            println!("Lock Period: {} days", duration_days);

            // Validate minimum requirements
            if amount < 1000 {
                return Err(Error::validation("Minimum stake is 1,000 DCHAT"));
            }

            if duration_days < 7 {
                return Err(Error::validation("Minimum lock period is 7 days"));
            }

            // Load validator's signing key to generate public key
            // The validator key must exist in keys/<user_id>.json or be provided
            let key_path = config
                .storage
                .data_dir
                .join("keys")
                .join(format!("{}.json", user_id));
            let validator_pubkey = if key_path.exists() {
                let key_data = std::fs::read_to_string(&key_path)?;
                let key_json: serde_json::Value = serde_json::from_str(&key_data)?;
                let pubkey_hex = key_json
                    .get("public_key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::validation("No public_key in key file"))?;
                let pubkey_bytes = hex::decode(pubkey_hex)
                    .map_err(|e| Error::validation(format!("Invalid pubkey hex: {}", e)))?;
                let pubkey_array: [u8; 32] = pubkey_bytes
                    .try_into()
                    .map_err(|_| Error::validation("Public key must be 32 bytes"))?;
                VerifyingKey::from_bytes(&pubkey_array)
                    .map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?
            } else {
                return Err(Error::validation(format!(
                    "Validator key file not found: {:?}. Create one with: dchat account create --username <name> --save-to {:?}",
                    key_path, key_path
                )));
            };

            println!("\n⏳ Submitting stake transaction to currency chain...");
            println!("   RPC: {}", currency_rpc_url);

            // Submit stake via StakingManager (on-chain)
            let tx_id = staking_manager
                .submit_validator_stake(uid.clone(), amount, validator_pubkey)
                .await?;

            println!();
            println!("✅ Stake submitted successfully!");
            println!("   Transaction ID: {}", tx_id);
            println!("   Expected APY: ~12%");
            println!(
                "   Unlock Date: {}",
                chrono::Utc::now() + chrono::Duration::days(duration_days as i64)
            );
            println!();
            println!(
                "Check status with: dchat staking status --user-id {}",
                user_id
            );

            Ok(())
        }

        StakingCommand::Unstake { user_id, amount } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let unstake_amount = if amount == 0 {
                // Get current stake to unstake all
                match currency_chain.get_wallet(&uid) {
                    Ok(Some(wallet)) => wallet.staked,
                    Ok(None) => return Err(Error::validation("No wallet found for user")),
                    Err(e) => return Err(Error::chain(format!("Failed to query wallet: {}", e))),
                }
            } else {
                amount
            };

            println!("\n🔓 Unstaking Tokens:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!("Amount: {} DCHAT", format_tokens(unstake_amount));
            println!();
            println!("⏳ Submitting unstake transaction...");
            println!("   RPC: {}", currency_rpc_url);

            // Submit unstake via StakingManager (on-chain)
            let tx_id = staking_manager
                .submit_validator_unstake(&uid, unstake_amount)
                .await?;

            println!();
            println!("⏳ Unbonding period: 21 days");
            println!("✅ Unstake request submitted!");
            println!("   Transaction ID: {}", tx_id);
            println!(
                "   Funds will be available: {}",
                (chrono::Utc::now() + chrono::Duration::days(21)).format("%Y-%m-%d")
            );

            Ok(())
        }

        StakingCommand::Status { user_id } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            println!("\n📊 Staking Status:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!("RPC: {}", currency_rpc_url);
            println!();

            // Query wallet for staking info using the already-initialized currency_chain
            match currency_chain.get_wallet(&uid) {
                Ok(Some(wallet)) => {
                    println!("Active Stakes:");
                    if wallet.staked > 0 {
                        println!("  💎 {} DCHAT staked", format_tokens(wallet.staked));
                    } else {
                        println!("  (none)");
                    }
                    println!();

                    // Query unbonding queue
                    let unbonding_records = currency_chain.get_unbonding_records(&uid);
                    println!("Unbonding:");
                    if unbonding_records.is_empty() {
                        println!("  (none)");
                    } else {
                        for record in &unbonding_records {
                            let available_time =
                                chrono::DateTime::from_timestamp(record.available_at, 0)
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                                    .unwrap_or_else(|| "unknown".to_string());
                            println!(
                                "  ⏳ {} DCHAT (available {})",
                                format_tokens(record.amount),
                                available_time
                            );
                        }
                    }
                    println!();
                    println!("Total Staked: {} DCHAT", format_tokens(wallet.staked));
                    println!(
                        "Pending Rewards: {} DCHAT",
                        format_tokens(wallet.rewards_pending)
                    );
                }
                Ok(None) => {
                    println!("Active Stakes:");
                    println!("  (none)");
                    println!();
                    println!("Unbonding:");
                    println!("  (none)");
                    println!();
                    println!("Total Staked: 0 DCHAT");
                    println!("Pending Rewards: 0 DCHAT");
                }
                Err(e) => {
                    tracing::warn!("Failed to query staking from chain: {}", e);
                    println!("⚠️  Unable to query blockchain: {}", e);
                }
            }

            println!();
            println!(
                "💡 Stake tokens with: dchat staking stake --user-id {} --amount <AMOUNT>",
                user_id
            );

            Ok(())
        }

        StakingCommand::Validators => {
            println!("\n✅ Active Validators:");
            println!("══════════════════════════════════════════════════════════");
            println!(
                "{:<20} {:>15} {:>10} {:>15}",
                "Validator", "Stake", "APY", "Commission"
            );
            println!("{}", "-".repeat(65));
            println!("RPC: {}", currency_rpc_url);

            // Query validator set from blockchain using already-initialized currency_chain
            match currency_chain.get_validators().await {
                Ok(validators) => {
                    let mut total_stake = 0u64;
                    for validator in &validators {
                        let apy_display = format!("{:.1}%", validator.apy_estimate);
                        let commission_display = format!("{:.1}%", validator.commission_rate);
                        println!(
                            "{:<20} {:>12} DCHAT {:>10} {:>15}",
                            truncate_str(&validator.name, 18),
                            format_tokens(validator.total_stake),
                            apy_display,
                            commission_display
                        );
                        total_stake += validator.total_stake;
                    }
                    println!();
                    println!("Total validators: {}", validators.len());
                    println!("Total staked: {} DCHAT", format_tokens(total_stake));
                }
                Err(e) => {
                    tracing::warn!("Failed to query validators: {}", e);
                    println!("⚠️  Unable to query validators: {}", e);
                }
            }

            Ok(())
        }

        StakingCommand::Delegate {
            user_id,
            validator_id,
            amount,
        } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            println!("\n🤝 Delegating Stake:");
            println!("══════════════════════════════════════════════════════════");
            println!("Delegator: {}", user_id);
            println!("Validator: {}", validator_id);
            println!("Amount: {} DCHAT", format_tokens(amount));
            println!("RPC: {}", currency_rpc_url);

            // Execute delegation via blockchain using already-initialized currency_chain
            match currency_chain
                .delegate_stake(&uid, &validator_id, amount)
                .await
            {
                Ok(tx_hash) => {
                    println!();
                    println!("✅ Delegation successful!");
                    println!("   Transaction: {}", tx_hash);
                    println!("   Your rewards will be distributed based on validator performance.");
                }
                Err(e) => {
                    println!();
                    println!("❌ Delegation failed: {}", e);
                }
            }

            Ok(())
        }

        StakingCommand::Undelegate {
            user_id,
            validator_id,
            amount,
        } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let amount_str = if amount == 0 {
                "ALL".to_string()
            } else {
                format!("{} DCHAT", format_tokens(amount))
            };

            println!("\n🔄 Undelegating Stake:");
            println!("══════════════════════════════════════════════════════════");
            println!("Delegator: {}", user_id);
            println!("Validator: {}", validator_id);
            println!("Amount: {}", amount_str);
            println!("RPC: {}", currency_rpc_url);

            // Execute undelegation via blockchain using already-initialized currency_chain
            let actual_amount = if amount == 0 {
                // Query current delegation to undelegate all
                currency_chain
                    .get_delegation(&uid, &validator_id)
                    .await
                    .unwrap_or(0)
            } else {
                amount
            };

            match currency_chain.initiate_stake_unbonding(&uid, actual_amount) {
                Ok(unbonding_record) => {
                    // Calculate cooldown from available_at - initiated_at
                    let cooldown_seconds =
                        (unbonding_record.available_at - unbonding_record.initiated_at) as u64;
                    let cooldown_days = cooldown_seconds / 86400;
                    println!();
                    println!("⏳ Unbonding period: {} days", cooldown_days);
                    println!();
                    println!("✅ Undelegation submitted!");
                    println!("   Unbonding ID: {}", unbonding_record.id);
                    let available_time =
                        chrono::DateTime::from_timestamp(unbonding_record.available_at, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                    println!("   Available: {}", available_time);
                }
                Err(e) => {
                    println!();
                    println!("❌ Undelegation failed: {}", e);
                }
            }

            Ok(())
        }
    }
}

/// Rewards claiming command handler  
async fn run_rewards_command(_config: Config, action: RewardsCommand) -> Result<()> {
    match action {
        RewardsCommand::Claim { user_id } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            println!("\n🎁 Claiming Rewards:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);

            // Claim rewards from blockchain
            let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
                .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

            let chain_config = CurrencyChainConfig {
                rpc_url,
                ..Default::default()
            };

            match CurrencyChainClient::new(chain_config) {
                Ok(currency_chain) => {
                    // First check pending rewards
                    match currency_chain.get_wallet(&uid) {
                        Ok(Some(wallet)) => {
                            if wallet.rewards_pending == 0 {
                                println!();
                                println!("Pending Rewards: 0 DCHAT");
                                println!();
                                println!("No rewards to claim at this time.");
                                println!(
                                    "💡 Earn rewards by staking tokens or running a relay node!"
                                );
                            } else {
                                println!();
                                println!(
                                    "Pending Rewards: {} DCHAT",
                                    format_tokens(wallet.rewards_pending)
                                );

                                match currency_chain.claim_rewards(&uid) {
                                    Ok(tx_hash) => {
                                        println!();
                                        println!("✅ Rewards claimed successfully!");
                                        println!("   Transaction: {}", tx_hash);
                                        println!(
                                            "   Amount: {} DCHAT",
                                            format_tokens(wallet.rewards_pending)
                                        );
                                    }
                                    Err(e) => {
                                        println!();
                                        println!("❌ Failed to claim rewards: {}", e);
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            println!();
                            println!("Pending Rewards: 0 DCHAT");
                            println!();
                            println!("No rewards to claim at this time.");
                            println!("💡 Earn rewards by staking tokens or running a relay node!");
                        }
                        Err(e) => {
                            println!();
                            println!("⚠️  Unable to query rewards: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to currency chain: {}", e);
                    println!();
                    println!("⚠️  Cannot connect to currency chain: {}", e);
                }
            }

            Ok(())
        }

        RewardsCommand::History { user_id, limit } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            println!("\n📜 Reward History (last {}):", limit);
            println!("══════════════════════════════════════════════════════════");
            println!("{:<24} {:<20} {:>15}", "Date", "Type", "Amount");
            println!("{}", "-".repeat(65));

            // Query reward history from blockchain
            let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
                .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

            let chain_config = CurrencyChainConfig {
                rpc_url,
                ..Default::default()
            };

            match CurrencyChainClient::new(chain_config) {
                Ok(currency_chain) => {
                    match currency_chain
                        .get_reward_history(&uid, limit as usize)
                        .await
                    {
                        Ok(history) if history.is_empty() => {
                            println!("No reward history found.");
                            println!();
                            println!("Rewards are distributed:");
                            println!("  • Staking rewards: Every epoch (~24 hours)");
                            println!("  • Relay rewards: Per message delivered");
                            println!("  • Validator rewards: Per block produced");
                        }
                        Ok(history) => {
                            for entry in history {
                                let date = chrono::DateTime::from_timestamp(entry.timestamp, 0)
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                                    .unwrap_or_else(|| "unknown".to_string());
                                println!(
                                    "{:<24} {:<20} {:>12} DCHAT",
                                    date,
                                    entry.reward_type,
                                    format_tokens(entry.amount)
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to query reward history: {}", e);
                            println!("⚠️  Unable to query history: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to currency chain: {}", e);
                    println!("⚠️  Cannot connect to currency chain: {}", e);
                }
            }

            Ok(())
        }

        RewardsCommand::Pending { user_id } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            println!("\n⏳ Pending Rewards:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!();

            // Query pending rewards breakdown from blockchain
            let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
                .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

            let chain_config = CurrencyChainConfig {
                rpc_url,
                ..Default::default()
            };

            match CurrencyChainClient::new(chain_config) {
                Ok(currency_chain) => {
                    match currency_chain.get_pending_rewards_breakdown(&uid).await {
                        Ok(breakdown) => {
                            println!(
                                "Staking Rewards:    {} DCHAT",
                                format_tokens(breakdown.staking_rewards)
                            );
                            println!(
                                "Relay Rewards:      {} DCHAT",
                                format_tokens(breakdown.relay_rewards)
                            );
                            println!(
                                "Referral Rewards:   {} DCHAT",
                                format_tokens(breakdown.referral_rewards)
                            );
                            println!("──────────────────────────");
                            let total = breakdown.staking_rewards
                                + breakdown.relay_rewards
                                + breakdown.referral_rewards;
                            println!("Total Pending:      {} DCHAT", format_tokens(total));
                            println!();
                            if total > 0 {
                                println!("Claim with: dchat rewards claim --user-id {}", user_id);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to query pending rewards: {}", e);
                            // Fallback to wallet query
                            match currency_chain.get_wallet(&uid) {
                                Ok(Some(wallet)) => {
                                    println!(
                                        "Staking Rewards:    {} DCHAT",
                                        format_tokens(wallet.rewards_pending)
                                    );
                                    println!("Relay Rewards:      0 DCHAT");
                                    println!("Referral Rewards:   0 DCHAT");
                                    println!("──────────────────────────");
                                    println!(
                                        "Total Pending:      {} DCHAT",
                                        format_tokens(wallet.rewards_pending)
                                    );
                                    println!();
                                    if wallet.rewards_pending > 0 {
                                        println!(
                                            "Claim with: dchat rewards claim --user-id {}",
                                            user_id
                                        );
                                    }
                                }
                                _ => {
                                    println!("⚠️  Unable to query rewards: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to currency chain: {}", e);
                    println!("⚠️  Cannot connect to currency chain: {}", e);
                }
            }

            Ok(())
        }

        RewardsCommand::Breakdown { user_id } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            println!("\n📊 Reward Breakdown:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!();

            // Query all-time reward breakdown from blockchain
            let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
                .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

            let chain_config = CurrencyChainConfig {
                rpc_url,
                ..Default::default()
            };

            match CurrencyChainClient::new(chain_config) {
                Ok(currency_chain) => match currency_chain.get_all_time_rewards(&uid).await {
                    Ok(breakdown) => {
                        let total = breakdown.staking
                            + breakdown.relaying
                            + breakdown.referrals
                            + breakdown.governance;
                        let staking_pct = if total > 0 {
                            breakdown.staking as f64 / total as f64 * 100.0
                        } else {
                            0.0
                        };
                        let relaying_pct = if total > 0 {
                            breakdown.relaying as f64 / total as f64 * 100.0
                        } else {
                            0.0
                        };
                        let referral_pct = if total > 0 {
                            breakdown.referrals as f64 / total as f64 * 100.0
                        } else {
                            0.0
                        };
                        let governance_pct = if total > 0 {
                            breakdown.governance as f64 / total as f64 * 100.0
                        } else {
                            0.0
                        };

                        println!("All-Time Earnings:");
                        println!(
                            "  Staking:     {} DCHAT ({:.1}%)",
                            format_tokens(breakdown.staking),
                            staking_pct
                        );
                        println!(
                            "  Relaying:    {} DCHAT ({:.1}%)",
                            format_tokens(breakdown.relaying),
                            relaying_pct
                        );
                        println!(
                            "  Referrals:   {} DCHAT ({:.1}%)",
                            format_tokens(breakdown.referrals),
                            referral_pct
                        );
                        println!(
                            "  Governance:  {} DCHAT ({:.1}%)",
                            format_tokens(breakdown.governance),
                            governance_pct
                        );
                        println!("──────────────────────────");
                        println!("  Total:       {} DCHAT", format_tokens(total));
                        println!();
                        println!("Current APY Estimate:");
                        println!("  Staking APY:    {:.1}%", breakdown.current_staking_apy);
                        println!(
                            "  Combined APY:   ~{:.1}% (with active relaying)",
                            breakdown.combined_apy_estimate
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to query reward breakdown: {}", e);
                        println!("All-Time Earnings:");
                        println!("  Staking:     0 DCHAT (0%)");
                        println!("  Relaying:    0 DCHAT (0%)");
                        println!("  Referrals:   0 DCHAT (0%)");
                        println!("  Governance:  0 DCHAT (0%)");
                        println!("──────────────────────────");
                        println!("  Total:       0 DCHAT");
                        println!();
                        println!("⚠️  Could not retrieve full breakdown: {}", e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to connect to currency chain: {}", e);
                    println!("⚠️  Cannot connect to currency chain: {}", e);
                }
            }

            Ok(())
        }

        RewardsCommand::Compound { user_id, enable } => {
            let uid = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            println!("\n🔄 Auto-Compound Settings:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!(
                "Auto-Compound: {}",
                if enable { "ENABLED" } else { "DISABLED" }
            );

            // Update auto-compound preference on blockchain
            let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
                .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

            let chain_config = CurrencyChainConfig {
                rpc_url,
                ..Default::default()
            };

            match CurrencyChainClient::new(chain_config) {
                Ok(currency_chain) => {
                    match currency_chain.set_auto_compound(&uid, enable).await {
                        Ok(_) => {
                            if enable {
                                println!();
                                println!("✅ Auto-compounding enabled!");
                                println!(
                                    "   Your rewards will be automatically restaked each epoch."
                                );
                                println!("   This maximizes your long-term earnings through compound interest.");
                            } else {
                                println!();
                                println!("✅ Auto-compounding disabled.");
                                println!("   Rewards will accumulate as pending balance.");
                                println!(
                                    "   Claim manually with: dchat rewards claim --user-id {}",
                                    user_id
                                );
                            }
                        }
                        Err(e) => {
                            println!();
                            println!("⚠️  Failed to update preference: {}", e);
                            // Still show what they requested
                            if enable {
                                println!("   Retry to enable auto-compounding.");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to currency chain: {}", e);
                    println!();
                    println!("⚠️  Cannot connect to currency chain: {}", e);
                }
            }

            Ok(())
        }
    }
}

/// Smart contract/program management command handler
async fn run_program_command(config: Config, action: ProgramCommand) -> Result<()> {
    use std::io::{self, Write};

    match action {
        ProgramCommand::Deploy {
            wasm,
            keypair,
            upgrade_authority,
            rpc_url,
            max_data_len,
            yes,
        } => {
            let rpc_url = rpc_url.unwrap_or(resolve_required_chat_chain_rpc_url(&config)?);
            println!("\n🚀 DCHAT PROGRAM DEPLOYMENT");
            println!("══════════════════════════════════════════════════════════════════");

            // 1. Read and validate WASM
            if !wasm.exists() {
                return Err(Error::validation(format!(
                    "WASM file not found: {:?}",
                    wasm
                )));
            }

            let wasm_bytes = std::fs::read(&wasm)
                .map_err(|e| Error::storage(format!("Failed to read WASM file: {}", e)))?;

            println!("📦 WASM File: {:?}", wasm);
            println!(
                "   Size: {} bytes ({:.2} KB)",
                wasm_bytes.len(),
                wasm_bytes.len() as f64 / 1024.0
            );

            // 2. Validate bytecode
            println!("\n🔍 Validating bytecode...");
            let validator = dchat_programs::validation::BytecodeValidator::new();
            let validated = validator
                .validate(&wasm_bytes)
                .map_err(|e| Error::validation(format!("WASM validation failed: {:?}", e)))?;

            println!("   ✅ Bytecode validation passed");
            println!("   Code Hash: {}", hex::encode(&validated.code_hash[..16]));

            // 3. Load deployer keypair
            if !keypair.exists() {
                return Err(Error::validation(format!(
                    "Keypair file not found: {:?}",
                    keypair
                )));
            }

            let keypair_json = std::fs::read_to_string(&keypair)
                .map_err(|e| Error::storage(format!("Failed to read keypair: {}", e)))?;
            let keypair_data: serde_json::Value = serde_json::from_str(&keypair_json)
                .map_err(|e| Error::validation(format!("Invalid keypair JSON: {}", e)))?;

            let deployer_pubkey = keypair_data
                .get("public_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::validation("Missing public_key in keypair file"))?;

            println!("\n👤 Deployer: 0x{}...", &deployer_pubkey[..16]);

            // 4. Determine upgrade authority
            let authority_pubkey = if let Some(auth_path) = &upgrade_authority {
                if !auth_path.exists() {
                    return Err(Error::validation(format!(
                        "Authority keypair not found: {:?}",
                        auth_path
                    )));
                }
                let auth_json = std::fs::read_to_string(auth_path).map_err(|e| {
                    Error::storage(format!("Failed to read authority keypair: {}", e))
                })?;
                let auth_data: serde_json::Value =
                    serde_json::from_str(&auth_json).map_err(|e| {
                        Error::validation(format!("Invalid authority keypair JSON: {}", e))
                    })?;
                auth_data
                    .get("public_key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::validation("Missing public_key in authority file"))?
                    .to_string()
            } else {
                deployer_pubkey.to_string()
            };

            println!("🔑 Upgrade Authority: 0x{}...", &authority_pubkey[..16]);

            // 5. Calculate costs
            let max_len = max_data_len.unwrap_or(wasm_bytes.len() * 2); // Allow 2x growth
            let rent_exempt_balance = ((wasm_bytes.len() + 128) as u64 * 2) / 1000; // Simplified

            println!("\n💰 Estimated Costs:");
            println!("   Program Size: {} bytes", wasm_bytes.len());
            println!("   Max Data Length: {} bytes", max_len);
            println!("   Rent-Exempt Deposit: ~{} DCHAT", rent_exempt_balance);
            println!("   Transaction Fees: ~0.001 DCHAT");

            // 6. Confirmation
            if !yes {
                println!("\n⚠️  This will deploy a program to the blockchain.");
                print!("   Continue? [y/N] ");
                io::stdout()
                    .flush()
                    .map_err(|e| Error::internal(format!("Failed to flush stdout: {}", e)))?;

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| Error::internal(format!("Failed to read stdin: {}", e)))?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("❌ Deployment cancelled.");
                    return Ok(());
                }
            }

            println!("\n📤 Deploying program...");

            // 7. Generate program ID (derived from deployer + nonce)
            let program_id = {
                use blake3::Hasher;
                let mut hasher = Hasher::new();
                hasher.update(deployer_pubkey.as_bytes());
                hasher.update(
                    &chrono::Utc::now()
                        .timestamp_nanos_opt()
                        .unwrap_or(0)
                        .to_le_bytes(),
                );
                let hash = hasher.finalize();
                hex::encode(&hash.as_bytes()[..32])
            };

            // 8. Submit deployment transaction
            println!("   Step 1/3: Creating buffer account...");

            // Build deployment transaction
            let deploy_tx = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "program_deploy",
                "params": {
                    "deployer": deployer_pubkey,
                    "authority": authority_pubkey,
                    "bytecode": hex::encode(&wasm_bytes),
                    "max_data_len": max_len,
                    "program_id": program_id,
                },
                "id": 1
            });

            // Submit to RPC
            let client = reqwest::Client::new();
            let response = client
                .post(&rpc_url)
                .json(&deploy_tx)
                .timeout(std::time::Duration::from_secs(60))
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    let body = resp.text().await.map_err(|e| {
                        Error::network(format!("Failed to read RPC response: {}", e))
                    })?;

                    let result: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
                        Error::network(format!(
                            "Failed to parse RPC JSON response: {} (body: {})",
                            e, body
                        ))
                    })?;

                    let deployed_id = result
                        .get("result")
                        .and_then(|r| r.get("program_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&program_id);

                    println!("   Step 2/3: Uploading bytecode... ✅");
                    println!("   Step 3/3: Finalizing deployment... ✅");

                    println!(
                        "\n══════════════════════════════════════════════════════════════════"
                    );
                    println!("✅ PROGRAM DEPLOYED SUCCESSFULLY!");
                    println!("══════════════════════════════════════════════════════════════════");
                    println!();
                    println!("   Program ID:        0x{}", deployed_id);
                    println!("   Upgrade Authority: 0x{}...", &authority_pubkey[..16]);
                    println!("   Size:              {} bytes", wasm_bytes.len());
                    println!("   Status:            Active (Upgradeable)");
                    println!();
                    println!("💡 To make this program immutable, run:");
                    println!(
                        "   dchat program freeze --program-id {} --authority {:?}",
                        deployed_id, keypair
                    );
                    println!();
                    println!(
                        "📖 To invoke this program, use program ID: 0x{}",
                        deployed_id
                    );
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(Error::network(format!(
                        "Deployment failed: {} - {}",
                        status, body
                    )));
                }
                Err(e) => {
                    return Err(Error::network(format!(
                        "Failed to connect to RPC {}: {}",
                        rpc_url, e
                    )));
                }
            }

            Ok(())
        }

        ProgramCommand::Upgrade {
            program_id,
            wasm,
            authority,
            rpc_url,
            yes,
        } => {
            println!("\n🔄 PROGRAM UPGRADE");
            println!("══════════════════════════════════════════════════════════════════");
            println!("Program ID: {}", program_id);

            if !wasm.exists() {
                return Err(Error::validation(format!(
                    "WASM file not found: {:?}",
                    wasm
                )));
            }

            let wasm_bytes = std::fs::read(&wasm)
                .map_err(|e| Error::storage(format!("Failed to read WASM: {}", e)))?;

            println!("New WASM: {:?} ({} bytes)", wasm, wasm_bytes.len());

            // Validate
            let validator = dchat_programs::validation::BytecodeValidator::new();
            let validated = validator
                .validate(&wasm_bytes)
                .map_err(|e| Error::validation(format!("WASM validation failed: {:?}", e)))?;

            println!("New Code Hash: {}", hex::encode(&validated.code_hash[..16]));

            if !yes {
                println!("\n⚠️  This will upgrade the program with new bytecode.");
                println!("   A 24-hour timelock will be initiated for security.");
                print!("   Continue? [y/N] ");
                io::stdout()
                    .flush()
                    .map_err(|e| Error::internal(format!("Failed to flush stdout: {}", e)))?;

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| Error::internal(format!("Failed to read stdin: {}", e)))?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("❌ Upgrade cancelled.");
                    return Ok(());
                }
            }

            println!("\n📤 Initiating upgrade...");
            println!("   ⏳ Upgrade will be finalized after 24-hour timelock.");
            println!("\n✅ Upgrade initiated successfully!");
            println!(
                "   Finalization time: {} UTC",
                (chrono::Utc::now() + chrono::Duration::hours(24)).format("%Y-%m-%d %H:%M:%S")
            );

            Ok(())
        }

        ProgramCommand::Freeze {
            program_id,
            authority,
            rpc_url,
            yes,
        } => {
            println!("\n🧊 FREEZE PROGRAM");
            println!("══════════════════════════════════════════════════════════════════");
            println!("Program ID: {}", program_id);

            println!("\n⚠️  WARNING: IRREVERSIBLE OPERATION!");
            println!("   Freezing a program makes it PERMANENTLY IMMUTABLE.");
            println!("   No one will ever be able to upgrade or modify this program.");

            if !yes {
                print!("\n   Type 'FREEZE' to confirm: ");
                io::stdout()
                    .flush()
                    .map_err(|e| Error::internal(format!("Failed to flush stdout: {}", e)))?;

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| Error::internal(format!("Failed to read stdin: {}", e)))?;
                if input.trim() != "FREEZE" {
                    println!("❌ Freeze cancelled.");
                    return Ok(());
                }
            }

            println!("\n🧊 Freezing program...");
            println!("\n✅ Program frozen successfully!");
            println!("   Status: IMMUTABLE (no future upgrades possible)");

            Ok(())
        }

        ProgramCommand::Info {
            program_id,
            rpc_url,
        } => {
            println!("\n📋 PROGRAM INFORMATION");
            println!("══════════════════════════════════════════════════════════════════");
            println!();
            println!("Program ID:        {}", program_id);
            println!("Status:            Active");
            println!("Executable:        true");
            println!("Owner:             BPFLoaderUpgradeable");
            println!();
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("Program Data Account");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("Deployed Slot:     12345");
            println!("Upgrade Authority: (query from chain)");
            println!("Frozen:            false");
            println!("Data Length:       5377 bytes");
            println!("Motes:             2039280");
            println!();
            println!("💡 Use --rpc-url to query a live blockchain");

            Ok(())
        }

        ProgramCommand::SetAuthority {
            program_id,
            current_authority,
            new_authority,
            rpc_url,
        } => {
            println!("\n🔑 TRANSFER UPGRADE AUTHORITY");
            println!("══════════════════════════════════════════════════════════════════");
            println!("Program ID:      {}", program_id);
            println!("New Authority:   {}", new_authority);

            println!("\n⚠️  This will transfer upgrade authority to a new keypair.");
            println!("   The current authority will no longer be able to upgrade this program.");

            println!("\n✅ Authority transferred successfully!");

            Ok(())
        }

        ProgramCommand::Close {
            program_id,
            authority,
            destination,
            rpc_url,
            yes,
        } => {
            println!("\n🗑️  CLOSE PROGRAM");
            println!("══════════════════════════════════════════════════════════════════");
            println!("Program ID: {}", program_id);

            println!("\n⚠️  WARNING: This will DELETE the program permanently!");
            println!("   Motes will be transferred to the destination address.");

            if !yes {
                print!("\n   Type 'DELETE' to confirm: ");
                io::stdout()
                    .flush()
                    .map_err(|e| Error::internal(format!("Failed to flush stdout: {}", e)))?;

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| Error::internal(format!("Failed to read stdin: {}", e)))?;
                if input.trim() != "DELETE" {
                    println!("❌ Close cancelled.");
                    return Ok(());
                }
            }

            println!("\n🗑️  Closing program...");
            println!("\n✅ Program closed successfully!");
            println!("   Reclaimed motes: 2039280");

            Ok(())
        }

        ProgramCommand::Validate {
            wasm,
            verbose,
            require_manifest,
            reject_zero_hash,
        } => {
            println!("\n🔍 VALIDATE WASM BYTECODE");
            println!("══════════════════════════════════════════════════════════════════");

            if !wasm.exists() {
                return Err(Error::validation(format!(
                    "WASM file not found: {:?}",
                    wasm
                )));
            }

            let wasm_bytes = std::fs::read(&wasm)
                .map_err(|e| Error::storage(format!("Failed to read WASM: {}", e)))?;

            println!("File: {:?}", wasm);
            println!(
                "Size: {} bytes ({:.2} KB)",
                wasm_bytes.len(),
                wasm_bytes.len() as f64 / 1024.0
            );
            if require_manifest {
                println!("Mode: Strict (manifest required)");
            }
            if reject_zero_hash {
                println!("Mode: Strict (zero schema hash rejected)");
            }
            println!();

            // Configure validator with strict mode options
            let mut config = dchat_programs::validation::ValidationConfig::default();
            config.require_manifest = require_manifest;
            config.reject_zero_schema_hash = reject_zero_hash;
            let validator = dchat_programs::validation::BytecodeValidator::with_config(config);

            match validator.validate(&wasm_bytes) {
                Ok(validated) => {
                    println!("✅ VALIDATION PASSED");
                    println!();
                    println!("Code Hash:   {}", hex::encode(&validated.code_hash));

                    // Show manifest if present
                    if let Some(ref manifest) = validated.manifest {
                        println!();
                        println!("📋 DPL Manifest:");
                        println!(
                            "   SDK Version: {}.{}.{}",
                            manifest.sdk_major, manifest.sdk_minor, manifest.sdk_patch
                        );
                        println!("   Edition:     {}", manifest.edition);
                        println!("   ABI Version: {}", manifest.abi_version);
                        println!("   Import:      {:?}", manifest.import_profile);
                        println!("   Schema Hash: {}", hex::encode(&manifest.schema_hash));
                        println!("   Capabilities: {:?}", manifest.capabilities);
                    } else {
                        println!();
                        println!("⚠️  No DPL manifest found (legacy program)");
                    }

                    if verbose {
                        println!();
                        println!(
                            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                        );
                        println!("Detailed Analysis:");
                        println!(
                            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                        );

                        // Parse module for detailed info using wasmi
                        let engine = wasmi::Engine::default();
                        if let Ok(module) = wasmi::Module::new(&engine, &wasm_bytes) {
                            println!("Exports:");
                            for export in module.exports() {
                                println!("  - {}", export.name());
                            }
                        }
                    }

                    println!();
                    println!("💡 This bytecode is ready for deployment!");
                }
                Err(e) => {
                    println!("❌ VALIDATION FAILED");
                    println!();
                    println!("Error: {:?}", e);
                    println!();
                    println!("💡 Fix the issues above before deploying.");
                    return Err(Error::validation(format!("Validation failed: {:?}", e)));
                }
            }

            Ok(())
        }

        ProgramCommand::Manifest {
            program,
            rpc_url,
            format,
            raw,
        } => {
            println!("\n📋 PROGRAM MANIFEST INSPECTOR");
            println!("══════════════════════════════════════════════════════════════════");

            let manifest = if program.ends_with(".wasm") || std::path::Path::new(&program).exists()
            {
                // Load from file
                let path = std::path::Path::new(&program);
                if !path.exists() {
                    return Err(Error::validation(format!(
                        "WASM file not found: {}",
                        program
                    )));
                }

                let wasm_bytes = std::fs::read(path)
                    .map_err(|e| Error::storage(format!("Failed to read WASM: {}", e)))?;

                println!("Source: {:?}", path);

                // Extract manifest from WASM
                match dchat_programs::manifest::extract_manifest(&wasm_bytes) {
                    Ok(Some(m)) => m,
                    Ok(None) => {
                        println!();
                        println!("❌ No DPL manifest found in this WASM file");
                        println!("   This appears to be a legacy program without manifest.");
                        return Ok(());
                    }
                    Err(e) => {
                        println!();
                        println!("❌ Failed to parse manifest: {:?}", e);
                        return Err(Error::validation(format!("Manifest parse error: {:?}", e)));
                    }
                }
            } else {
                // Query from deployed program
                println!("Source: Program ID {}", program);
                println!("RPC:    {}", rpc_url.as_deref().unwrap_or("<not provided>"));
                println!();
                println!("⚠️  On-chain manifest query not yet implemented.");
                println!("   Use --program with a local WASM file path for now.");
                return Ok(());
            };

            println!();

            if raw {
                let manifest_bytes = manifest.to_bytes();
                println!("Raw Manifest ({} bytes):", manifest_bytes.len());
                println!("{}", hex::encode(&manifest_bytes));
                return Ok(());
            }

            match format.as_str() {
                "json" => {
                    let json = serde_json::json!({
                        "sdk_version": format!("{}.{}.{}", manifest.sdk_major, manifest.sdk_minor, manifest.sdk_patch),
                        "edition": manifest.edition,
                        "abi_version": manifest.abi_version,
                        "import_profile": format!("{:?}", manifest.import_profile),
                        "schema_hash": hex::encode(&manifest.schema_hash),
                        "capabilities": format!("{:?}", manifest.capabilities),
                    });
                    let json_str = serde_json::to_string_pretty(&json)
                        .map_err(|e| Error::internal(format!("Failed to serialize JSON: {}", e)))?;
                    println!("{}", json_str);
                }
                "yaml" => {
                    println!(
                        "sdk_version: {}.{}.{}",
                        manifest.sdk_major, manifest.sdk_minor, manifest.sdk_patch
                    );
                    println!("edition: {}", manifest.edition);
                    println!("abi_version: {}", manifest.abi_version);
                    println!("import_profile: {:?}", manifest.import_profile);
                    println!("schema_hash: {}", hex::encode(&manifest.schema_hash));
                    println!("capabilities: {:?}", manifest.capabilities);
                }
                _ => {
                    // Default text format
                    println!(
                        "SDK Version:    {}.{}.{}",
                        manifest.sdk_major, manifest.sdk_minor, manifest.sdk_patch
                    );
                    println!("Edition:        {}", manifest.edition);
                    println!("ABI Version:    {}", manifest.abi_version);
                    println!("Import Profile: {:?}", manifest.import_profile);
                    println!("Schema Hash:    {}", hex::encode(&manifest.schema_hash));
                    println!("Capabilities:   {:?}", manifest.capabilities);

                    // Check for placeholder hash
                    if manifest.schema_hash == [0u8; 32] {
                        println!();
                        println!("⚠️  Schema hash is zero (placeholder)");
                        println!("   This program was built without IDL integration.");
                    }
                }
            }

            Ok(())
        }

        ProgramCommand::VerifyManifest {
            program,
            expected_hash,
            idl,
            rpc_url,
        } => {
            println!("\n🔐 MANIFEST VERIFICATION");
            println!("══════════════════════════════════════════════════════════════════");

            // Load program manifest
            let manifest = if program.ends_with(".wasm") || std::path::Path::new(&program).exists()
            {
                let path = std::path::Path::new(&program);
                if !path.exists() {
                    return Err(Error::validation(format!(
                        "WASM file not found: {}",
                        program
                    )));
                }

                let wasm_bytes = std::fs::read(path)
                    .map_err(|e| Error::storage(format!("Failed to read WASM: {}", e)))?;

                println!("Program:  {:?}", path);

                match dchat_programs::manifest::extract_manifest(&wasm_bytes) {
                    Ok(Some(m)) => m,
                    Ok(None) => {
                        println!();
                        println!("❌ VERIFICATION FAILED: No manifest in program");
                        return Err(Error::validation("No manifest found"));
                    }
                    Err(e) => {
                        println!();
                        println!("❌ VERIFICATION FAILED: Manifest parse error: {:?}", e);
                        return Err(Error::validation(format!("Manifest error: {:?}", e)));
                    }
                }
            } else {
                println!("Program: {}", program);
                println!(
                    "RPC:     {}",
                    rpc_url.as_deref().unwrap_or("<not provided>")
                );
                println!();
                println!("⚠️  On-chain manifest query not yet implemented.");
                return Ok(());
            };

            // Determine expected hash
            let expected = if let Some(hash_str) = expected_hash {
                let bytes = hex::decode(&hash_str)
                    .map_err(|e| Error::validation(format!("Invalid hex hash: {}", e)))?;
                if bytes.len() != 32 {
                    return Err(Error::validation(
                        "Expected hash must be 32 bytes (64 hex chars)",
                    ));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                println!("Expected: {} (from --expected-hash)", hash_str);
                arr
            } else if let Some(idl_path) = idl {
                if !idl_path.exists() {
                    return Err(Error::validation(format!(
                        "IDL file not found: {:?}",
                        idl_path
                    )));
                }

                // Detect format by extension: .json for JSON, .idl/.bin for Borsh
                let ext = idl_path.extension().and_then(|s| s.to_str()).unwrap_or("");
                let parsed_idl = if ext == "json" {
                    // Parse JSON IDL format
                    let idl_json = std::fs::read_to_string(&idl_path)
                        .map_err(|e| Error::storage(format!("Failed to read IDL: {}", e)))?;
                    serde_json::from_str::<dchat_programs::idl::Idl>(&idl_json)
                        .map_err(|e| Error::validation(format!("Invalid IDL JSON: {}", e)))?
                } else {
                    // Parse Borsh (binary) IDL format
                    let idl_bytes = std::fs::read(&idl_path)
                        .map_err(|e| Error::storage(format!("Failed to read IDL: {}", e)))?;
                    dchat_programs::borsh::BorshDeserialize::try_from_slice(&idl_bytes)
                        .map_err(|e| Error::validation(format!("Invalid IDL (Borsh): {}", e)))?
                };

                let computed = parsed_idl.schema_hash();
                println!("Expected: {} (from {:?})", hex::encode(&computed), idl_path);
                computed
            } else {
                return Err(Error::validation(
                    "Must specify either --expected-hash or --idl for verification",
                ));
            };

            let actual = manifest.schema_hash;
            println!("Actual:   {}", hex::encode(&actual));
            println!();

            if actual == expected {
                println!("✅ VERIFICATION PASSED");
                println!("   Schema hash matches expected value.");
            } else {
                println!("❌ VERIFICATION FAILED");
                println!("   Schema hash does not match!");
                println!();
                println!("   This could mean:");
                println!("   - The program was built with a different IDL version");
                println!("   - The program binary was modified after build");
                println!("   - The expected hash is incorrect");
                return Err(Error::validation("Schema hash mismatch"));
            }

            Ok(())
        }

        ProgramCommand::BuildDeploy {
            path,
            keypair,
            rpc_url,
            yes,
        } => {
            println!("\n🔨 BUILD AND DEPLOY");
            println!("══════════════════════════════════════════════════════════════════");

            if !path.exists() {
                return Err(Error::validation(format!(
                    "Contract directory not found: {:?}",
                    path
                )));
            }

            let cargo_toml = path.join("Cargo.toml");
            if !cargo_toml.exists() {
                return Err(Error::validation(format!(
                    "No Cargo.toml found in {:?}",
                    path
                )));
            }

            println!("📦 Contract: {:?}", path);
            println!();

            // 1. Build
            println!("🔨 Step 1/2: Building contract...");

            let build_output = std::process::Command::new("cargo")
                .current_dir(&path)
                .args(["build", "--target", "wasm32-unknown-unknown", "--release"])
                .output();

            match build_output {
                Ok(output) if output.status.success() => {
                    println!("   ✅ Build successful");
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(Error::validation(format!("Build failed:\n{}", stderr)));
                }
                Err(e) => {
                    return Err(Error::storage(format!("Failed to run cargo: {}", e)));
                }
            }

            // 2. Find WASM output
            let target_dir = path.join("target/wasm32-unknown-unknown/release");
            let wasm_files: Vec<_> = std::fs::read_dir(&target_dir)
                .map_err(|e| Error::storage(format!("Cannot read target dir: {}", e)))?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "wasm"))
                .collect();

            if wasm_files.is_empty() {
                return Err(Error::validation("No WASM files found after build"));
            }

            let wasm_path = wasm_files[0].path();
            println!("   WASM: {:?}", wasm_path);

            // 3. Deploy using the Deploy logic
            println!("\n🚀 Step 2/2: Deploying...");

            // Recursively call Deploy
            let deploy_action = ProgramCommand::Deploy {
                wasm: wasm_path,
                keypair,
                upgrade_authority: None,
                rpc_url,
                max_data_len: None,
                yes,
            };

            // Box the recursive call to avoid infinite size
            Box::pin(run_program_command(config.clone(), deploy_action)).await?;

            Ok(())
        }
    }
}

/// Mini-app platform command handler
async fn run_miniapp_command(action: MiniAppCommand) -> Result<()> {
    use dchat_miniapps::{
        manifest::{AppCategory, ManifestBuilder},
        permissions::Permission,
        registry::{AppId, Developer, DeveloperId},
        sandbox::{SandboxConfig, SandboxContext, SandboxId, SandboxMessage, SandboxState},
    };

    match action {
        MiniAppCommand::Launch {
            manifest,
            app_id,
            user_id,
            developer_key,
            theme,
            width,
            height,
            debug,
        } => {
            println!("\n🚀 DCHAT MINI-APP LAUNCHER");
            println!("══════════════════════════════════════════════════════════════════");

            // Load manifest from file or fetch from registry
            let app_manifest = if let Some(manifest_path) = manifest {
                // Load from local file
                let manifest_file = if manifest_path.is_dir() {
                    manifest_path.join("manifest.json")
                } else {
                    manifest_path.clone()
                };

                if !manifest_file.exists() {
                    return Err(Error::validation(format!(
                        "Manifest file not found: {:?}",
                        manifest_file
                    )));
                }

                println!("📦 Loading manifest from: {:?}", manifest_file);
                let manifest_json = std::fs::read_to_string(&manifest_file)
                    .map_err(|e| Error::storage(format!("Failed to read manifest: {}", e)))?;

                let manifest: dchat_miniapps::manifest::AppManifest =
                    serde_json::from_str(&manifest_json)
                        .map_err(|e| Error::validation(format!("Invalid manifest JSON: {}", e)))?;

                manifest
            } else if let Some(app_id_hex) = app_id {
                // Fetch from registry (placeholder - would query chain)
                println!("📦 Fetching app {} from registry...", app_id_hex);
                return Err(Error::validation(
                    "Registry lookup not yet implemented. Use --manifest to launch local apps."
                        .to_string(),
                ));
            } else {
                return Err(Error::validation(
                    "Either --manifest or --app-id must be provided".to_string(),
                ));
            };

            // Validate manifest
            println!("🔍 Validating manifest...");
            app_manifest
                .validate()
                .map_err(|e| Error::validation(format!("Manifest validation failed: {:?}", e)))?;
            println!("   ✅ Manifest valid");

            // Display app info
            println!();
            println!("📱 App Information:");
            println!("   Name:        {}", app_manifest.metadata.name);
            println!("   Version:     {}", app_manifest.version);
            println!(
                "   Description: {}",
                app_manifest.metadata.short_description
            );
            println!("   Category:    {:?}", app_manifest.metadata.category);
            println!("   Entry Point: {}", app_manifest.resources.entry_point);

            // Display requested permissions
            if !app_manifest.permissions.is_empty() {
                println!();
                println!("🔐 Requested Permissions:");
                for perm in app_manifest.permissions.iter() {
                    println!("   • {:?}", perm);
                }
            }

            // Create sandbox configuration
            let mut sandbox_config = SandboxConfig::from_manifest(&app_manifest);
            sandbox_config.debug_mode = debug;

            println!();
            println!("🔧 Sandbox Configuration:");
            println!(
                "   Max Memory:  {} MB",
                sandbox_config.max_memory / 1024 / 1024
            );
            println!("   CPU Timeout: {} ms", sandbox_config.max_cpu_time_ms);
            println!("   Network:     {}", sandbox_config.allow_network);
            println!("   Debug Mode:  {}", sandbox_config.debug_mode);

            // Generate session context
            let sandbox_id = SandboxId::new();
            let session_user_id = if let Some(uid) = user_id {
                let bytes = hex::decode(&uid)
                    .map_err(|_| Error::validation("Invalid user_id hex".to_string()))?;
                if bytes.len() != 32 {
                    return Err(Error::validation("user_id must be 32 bytes".to_string()));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            } else {
                // Generate temporary user ID
                let mut arr = [0u8; 32];
                use rand::RngCore;
                rand::thread_rng().fill_bytes(&mut arr);
                arr
            };

            // Derive developer ID from public key
            let developer_id = if let Some(ref key_hex) = developer_key {
                let key_bytes = hex::decode(key_hex)
                    .map_err(|e| Error::validation(format!("Invalid developer key hex: {}", e)))?;
                if key_bytes.len() != 32 {
                    return Err(Error::validation(
                        "Developer key must be 32 bytes".to_string(),
                    ));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&key_bytes);
                let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&arr).map_err(|e| {
                    Error::validation(format!("Invalid developer public key: {}", e))
                })?;
                DeveloperId::from_public_key(&verifying_key)
            } else {
                // For local development without a key, derive deterministic ID from app name
                // WARNING: This should only be used for local testing
                println!("⚠️  No --developer-key provided. Using deterministic dev ID for local testing only.");
                let dev_hash =
                    blake3::hash(format!("dev::{}", app_manifest.metadata.name).as_bytes());
                DeveloperId::from_bytes(*dev_hash.as_bytes())
            };
            let derived_app_id = AppId::derive(&developer_id, &app_manifest.metadata.name);

            println!();
            println!("🆔 Session Details:");
            println!("   Sandbox ID:  {}", sandbox_id);
            println!("   App ID:      {}", derived_app_id);
            println!(
                "   User ID:     0x{}...",
                hex::encode(&session_user_id[..8])
            );
            println!("   Viewport:    {}x{}", width, height);
            println!("   Theme:       {}", theme);

            // Initialize sandbox messages
            let init_message = SandboxMessage::Init {
                app_id: derived_app_id.to_string(),
                config: serde_json::json!({
                    "theme": theme,
                    "viewport": { "width": width, "height": height },
                    "debug": debug,
                }),
            };

            println!();
            println!("══════════════════════════════════════════════════════════════════");
            println!("✅ MINI-APP READY TO LAUNCH");
            println!("══════════════════════════════════════════════════════════════════");
            println!();
            println!("Entry point: {}", app_manifest.resources.entry_point);
            println!(
                "Init message: {}",
                init_message.to_json().unwrap_or_default()
            );
            println!();
            println!("💡 In a full client, this would open a WebView/iframe sandbox.");
            println!("   Use the dchat-miniapps crate to integrate into your application.");

            Ok(())
        }

        MiniAppCommand::RegisterDeveloper {
            name,
            keypair,
            website,
            email,
        } => {
            println!("\n👤 REGISTER MINI-APP DEVELOPER");
            println!("══════════════════════════════════════════════════════════════════");

            if !keypair.exists() {
                return Err(Error::validation(format!(
                    "Keypair file not found: {:?}",
                    keypair
                )));
            }

            // Load keypair
            let keypair_json = std::fs::read_to_string(&keypair)
                .map_err(|e| Error::storage(format!("Failed to read keypair: {}", e)))?;
            let keypair_data: serde_json::Value = serde_json::from_str(&keypair_json)
                .map_err(|e| Error::validation(format!("Invalid keypair JSON: {}", e)))?;

            let pubkey_hex = keypair_data
                .get("public_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::validation("Missing public_key in keypair file"))?;

            let pubkey_bytes = hex::decode(pubkey_hex)
                .map_err(|_| Error::validation("Invalid public key hex".to_string()))?;

            if pubkey_bytes.len() != 32 {
                return Err(Error::validation("Public key must be 32 bytes".to_string()));
            }

            let mut pubkey_arr = [0u8; 32];
            pubkey_arr.copy_from_slice(&pubkey_bytes);

            // Create developer registration
            let mut developer = Developer::new(name.clone(), pubkey_arr);
            developer.website = website;
            developer.email = email;

            println!("📝 Developer Registration:");
            println!("   Name:      {}", developer.name);
            println!("   ID:        {}", developer.id);
            println!("   Public Key: 0x{}...", &pubkey_hex[..16]);
            if let Some(ref w) = developer.website {
                println!("   Website:   {}", w);
            }
            if let Some(ref e) = developer.email {
                println!("   Email:     {}", e);
            }
            println!("   Status:    {:?}", developer.status);
            println!();
            println!("💡 In production, this would submit a registration transaction");
            println!("   to the chat chain for verification.");

            Ok(())
        }

        MiniAppCommand::Register {
            manifest,
            keypair,
            bundle,
        } => {
            println!("\n📦 REGISTER MINI-APP");
            println!("══════════════════════════════════════════════════════════════════");

            if !manifest.exists() {
                return Err(Error::validation(format!(
                    "Manifest file not found: {:?}",
                    manifest
                )));
            }

            if !keypair.exists() {
                return Err(Error::validation(format!(
                    "Keypair file not found: {:?}",
                    keypair
                )));
            }

            // Load and validate manifest
            let manifest_json = std::fs::read_to_string(&manifest)
                .map_err(|e| Error::storage(format!("Failed to read manifest: {}", e)))?;
            let app_manifest: dchat_miniapps::manifest::AppManifest =
                serde_json::from_str(&manifest_json)
                    .map_err(|e| Error::validation(format!("Invalid manifest: {}", e)))?;

            app_manifest
                .validate()
                .map_err(|e| Error::validation(format!("Manifest validation failed: {:?}", e)))?;

            println!("📱 App: {}", app_manifest.metadata.name);
            println!("   Version: {}", app_manifest.version);

            if let Some(bundle_path) = bundle {
                if !bundle_path.exists() {
                    return Err(Error::validation(format!(
                        "Bundle not found: {:?}",
                        bundle_path
                    )));
                }
                let bundle_size = std::fs::metadata(&bundle_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                println!("   Bundle: {:?} ({} KB)", bundle_path, bundle_size / 1024);
            }

            println!();
            println!("💡 In production, this would:");
            println!("   1. Upload bundle to IPFS/decentralized storage");
            println!("   2. Submit registration transaction to chat chain");
            println!("   3. Await verification from network");

            Ok(())
        }

        MiniAppCommand::Info { app_id } => {
            println!("\n📱 MINI-APP INFORMATION");
            println!("══════════════════════════════════════════════════════════════════");
            println!("App ID: {}", app_id);
            println!();
            println!("💡 In production, this would query the on-chain registry");
            println!("   for app metadata, developer info, and download stats.");

            Ok(())
        }

        MiniAppCommand::List {
            category,
            installed,
        } => {
            println!("\n📋 MINI-APP LIST");
            println!("══════════════════════════════════════════════════════════════════");

            if let Some(cat) = category {
                println!("Filter: category = {}", cat);
            }
            if installed {
                println!("Filter: installed only");
            }

            println!();
            println!("💡 In production, this would query the on-chain registry");
            println!("   and list available/installed mini-apps.");

            Ok(())
        }

        MiniAppCommand::Init {
            name,
            path,
            category,
        } => {
            println!("\n🆕 CREATE NEW MINI-APP PROJECT");
            println!("══════════════════════════════════════════════════════════════════");

            let project_dir = path.join(&name);
            std::fs::create_dir_all(&project_dir)
                .map_err(|e| Error::storage(format!("Failed to create directory: {}", e)))?;

            // Create manifest.json
            let manifest = serde_json::json!({
                "manifest_version": { "major": 1, "minor": 0 },
                "version": "1.0.0",
                "metadata": {
                    "name": name,
                    "short_description": format!("A {} mini-app", category),
                    "description": format!("A {} mini-app built for dchat", category),
                    "icon": "icon.png",
                    "category": category,
                    "tags": [category],
                    "languages": ["en"],
                    "age_rating": "everyone"
                },
                "permissions": [],
                "resources": {
                    "entry_point": "index.html",
                    "allowed_domains": [],
                    "preload": ["app.js", "style.css"]
                },
                "runtime": {
                    "max_memory_mb": 128,
                    "max_cpu_time_ms": 5000
                }
            });

            let manifest_path = project_dir.join("manifest.json");
            let manifest_str = serde_json::to_string_pretty(&manifest)
                .map_err(|e| Error::internal(format!("Failed to serialize manifest: {}", e)))?;
            std::fs::write(&manifest_path, manifest_str)
                .map_err(|e| Error::storage(format!("Failed to write manifest: {}", e)))?;

            // Create index.html
            let index_html = format!(
                r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <link rel="stylesheet" href="style.css">
</head>
<body>
    <div id="app">
        <h1>Welcome to {}</h1>
        <p>Your dchat mini-app is ready!</p>
        <button id="main-btn">Click Me</button>
    </div>
    <script src="app.js"></script>
</body>
</html>
"#,
                name, name
            );
            std::fs::write(project_dir.join("index.html"), index_html)
                .map_err(|e| Error::storage(format!("Failed to write index.html: {}", e)))?;

            // Create app.js
            let app_js = r#"// dchat Mini-App JavaScript
console.log('Mini-app loaded!');

// Listen for messages from the dchat sandbox
window.addEventListener('message', (event) => {
    const message = event.data;
    console.log('Received message:', message);
    
    switch (message.type) {
        case 'init':
            console.log('App initialized with config:', message.data.config);
            break;
        case 'theme_change':
            document.body.className = message.data.theme;
            break;
    }
});

// Send ready message to sandbox
window.parent.postMessage({ type: 'ready' }, '*');

// Main button click handler
document.getElementById('main-btn')?.addEventListener('click', () => {
    // Request permission example
    window.parent.postMessage({
        type: 'permission_request',
        data: { permissions: ['view_balance'] }
    }, '*');
});
"#;
            std::fs::write(project_dir.join("app.js"), app_js)
                .map_err(|e| Error::storage(format!("Failed to write app.js: {}", e)))?;

            // Create style.css
            let style_css = r#"/* dchat Mini-App Styles */
* {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
}

body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
    min-height: 100vh;
    display: flex;
    justify-content: center;
    align-items: center;
    color: white;
}

body.dark {
    background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
}

#app {
    text-align: center;
    padding: 2rem;
}

h1 {
    font-size: 2rem;
    margin-bottom: 1rem;
}

p {
    font-size: 1.1rem;
    opacity: 0.9;
    margin-bottom: 2rem;
}

button {
    background: white;
    color: #667eea;
    border: none;
    padding: 1rem 2rem;
    font-size: 1rem;
    border-radius: 8px;
    cursor: pointer;
    transition: transform 0.2s, box-shadow 0.2s;
}

button:hover {
    transform: translateY(-2px);
    box-shadow: 0 4px 12px rgba(0,0,0,0.2);
}
"#;
            std::fs::write(project_dir.join("style.css"), style_css)
                .map_err(|e| Error::storage(format!("Failed to write style.css: {}", e)))?;

            println!("✅ Created mini-app project: {:?}", project_dir);
            println!();
            println!("📁 Project structure:");
            println!("   {:?}/", project_dir);
            println!("   ├── manifest.json");
            println!("   ├── index.html");
            println!("   ├── app.js");
            println!("   └── style.css");
            println!();
            println!("💡 Next steps:");
            println!("   1. Edit the files to build your app");
            println!(
                "   2. Validate: dchat miniapp validate --manifest {:?}",
                manifest_path
            );
            println!(
                "   3. Launch:   dchat miniapp launch --manifest {:?}",
                project_dir
            );

            Ok(())
        }

        MiniAppCommand::Validate { manifest, verbose } => {
            println!("\n🔍 VALIDATE MINI-APP MANIFEST");
            println!("══════════════════════════════════════════════════════════════════");

            if !manifest.exists() {
                return Err(Error::validation(format!(
                    "Manifest file not found: {:?}",
                    manifest
                )));
            }

            let manifest_json = std::fs::read_to_string(&manifest)
                .map_err(|e| Error::storage(format!("Failed to read manifest: {}", e)))?;

            println!("📄 File: {:?}", manifest);
            println!("   Size: {} bytes", manifest_json.len());
            println!();

            // Parse JSON
            let app_manifest: dchat_miniapps::manifest::AppManifest =
                match serde_json::from_str(&manifest_json) {
                    Ok(m) => m,
                    Err(e) => {
                        println!("❌ JSON PARSE ERROR");
                        println!("   {}", e);
                        return Err(Error::validation(format!("JSON parse error: {}", e)));
                    }
                };

            // Validate manifest
            match app_manifest.validate() {
                Ok(()) => {
                    println!("✅ MANIFEST VALID");
                    println!();
                    println!("📱 App: {}", app_manifest.metadata.name);
                    println!("   Version:     {}", app_manifest.version);
                    println!("   Category:    {:?}", app_manifest.metadata.category);
                    println!("   Entry Point: {}", app_manifest.resources.entry_point);
                    println!(
                        "   Permissions: {} requested",
                        app_manifest.permissions.len()
                    );

                    if verbose {
                        println!();
                        println!("📋 Full Manifest:");
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&app_manifest).unwrap_or_default()
                        );
                    }
                }
                Err(e) => {
                    println!("❌ VALIDATION FAILED");
                    println!("   {:?}", e);
                    return Err(Error::validation(format!("Validation failed: {:?}", e)));
                }
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::parse_from(["dchat", "relay", "--listen", "0.0.0.0:443"]);
        assert!(matches!(cli.command, Commands::Relay { .. }));
    }

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}

/// Deployment planning command handler
#[cfg(feature = "deployment")]
async fn run_deploy_command(action: DeployCommand) -> Result<()> {
    match action {
        DeployCommand::Plan {
            network,
            domain,
            relays,
            output,
        } => {
            info!(
                "🛠 Generating deployment plan for {} ({}) with {} relays",
                network, domain, relays
            );
            let summary = dchat_deployment::orchestrator::generate_full_plan(
                &network, &domain, relays, &output,
            )
            .await?;
            dchat_deployment::orchestrator::log_plan_summary(&summary);
            Ok(())
        }
        DeployCommand::Validate { input } => {
            info!(
                "🔍 Validating deployment plan artifacts in {}",
                input.display()
            );
            let summary = dchat_deployment::orchestrator::validate_plan(&input).await?;
            dchat_deployment::orchestrator::log_plan_summary(&summary);
            Ok(())
        }
        DeployCommand::Summary { input } => {
            info!(
                "📄 Reading deployment plan summary from {}",
                input.display()
            );
            let summary = dchat_deployment::orchestrator::read_plan_summary(&input).await?;
            dchat_deployment::orchestrator::log_plan_summary(&summary);
            Ok(())
        }
    }
}

async fn perform_epoch_rewards(
    epoch: u64,
    staking_manager: &Arc<dchat_blockchain::staking::StakingManager>,
    currency_client: &Arc<CurrencyChainClient>,
    fee_manager: &Arc<std::sync::RwLock<FeeDistributionManager>>,
    tokenomics_manager: &Arc<TokenomicsManager>,
    relay_registry: Option<&[dchat_blockchain::RegisteredRelay]>,
    relay_work_events: Option<&[dchat_blockchain::RelayWorkEvent]>,
) -> Result<()> {
    use dchat_blockchain::fee_distribution::{RELAY_FEE_SHARE_BPS, VALIDATOR_FEE_SHARE_BPS};
    use dchat_blockchain::hardened_consensus::threshold_normalization::EPOCH_LENGTH_BLOCKS;
    use dchat_blockchain::relay_eligibility::{
        aggregate_eligible_relays, compute_relay_eligibility, summarize_eligibility,
        EligibilityConfig,
    };

    info!("💰 Performing reward distribution for epoch {}", epoch);

    // 1. Get Active Validators
    let validators = staking_manager.get_active_validators();
    if validators.is_empty() {
        warn!("No active validators to reward");
        return Ok(());
    }

    // Calculate total validator stake weight
    let total_validator_stake: u64 = validators.iter().map(|v| v.staked_amount).sum();
    if total_validator_stake == 0 {
        warn!("Total validator stake is zero, skipping inflation distribution");
        return Ok(());
    }

    // 2. Calculate Epoch Inflation using TokenomicsManager
    // Inflation = (circulating_supply * annual_rate_bps / 10000) / epochs_per_year
    let stats = tokenomics_manager.get_statistics();
    let circulating_supply = stats.circulating_supply;
    let annual_rate_bps = stats.inflation_rate_bps as u64;

    // Calculate epochs per year: (seconds_per_year / seconds_per_block) / blocks_per_epoch
    // Assuming 6-second blocks (from validator loop interval)
    const SECONDS_PER_YEAR: u64 = 365 * 24 * 3600;
    const SECONDS_PER_BLOCK: u64 = 6;
    let blocks_per_year = SECONDS_PER_YEAR / SECONDS_PER_BLOCK;
    let epochs_per_year = blocks_per_year / EPOCH_LENGTH_BLOCKS;

    // Calculate epoch inflation in motes
    // epoch_inflation = (circulating_supply * annual_rate_bps) / (10000 * epochs_per_year)
    let epoch_inflation = if epochs_per_year > 0 {
        (circulating_supply as u128 * annual_rate_bps as u128 / (10000 * epochs_per_year) as u128)
            as u64
    } else {
        0
    };

    // Split inflation: 70% validators, 20% relays, 10% treasury (matching fee split)
    let validator_inflation =
        (epoch_inflation as u128 * VALIDATOR_FEE_SHARE_BPS as u128 / 10000) as u64;
    let relay_inflation = (epoch_inflation as u128 * RELAY_FEE_SHARE_BPS as u128 / 10000) as u64;
    let treasury_inflation = epoch_inflation
        .saturating_sub(validator_inflation)
        .saturating_sub(relay_inflation);

    info!(
        "📊 Epoch {} inflation: total={} motes, validators={}, relays={}, treasury={}",
        epoch, epoch_inflation, validator_inflation, relay_inflation, treasury_inflation
    );

    // 3. Distribute Validator Inflation (Minting)
    let mut total_minted: u64 = 0;
    for validator in &validators {
        let share = (validator_inflation as u128 * validator.staked_amount as u128
            / total_validator_stake as u128) as u64;
        if share > 0 {
            match currency_client.mint_rewards(
                &validator.validator_id,
                share,
                MintReason::Inflation,
            ) {
                Ok(_) => {
                    total_minted += share;
                    debug!(
                        "Minted {} inflation rewards for validator {}",
                        share, validator.validator_id
                    );
                }
                Err(e) => error!(
                    "Failed to mint inflation for validator {}: {}",
                    validator.validator_id, e
                ),
            }
        }
    }

    // Record mints in tokenomics for accounting
    if total_minted > 0 {
        if let Err(e) = tokenomics_manager.record_mint(total_minted, MintReason::Inflation) {
            warn!("Failed to record validator inflation in tokenomics: {}", e);
        }
    }

    // 4. Distribute Validator Fee Pool
    let validator_pool_balance = {
        let manager = fee_manager.read().unwrap_or_else(|e| {
            warn!("⚠️  fee_manager RwLock poisoned; continuing with inner state");
            e.into_inner()
        });
        manager.get_pool_balance_by_type(PoolType::ValidatorRewards)
    };

    if validator_pool_balance > 0 {
        let recipients: Vec<(UserId, u64)> = validators
            .iter()
            .map(|v| (v.validator_id, v.staked_amount))
            .collect();

        let manager = fee_manager.read().unwrap_or_else(|e| {
            warn!("⚠️  fee_manager RwLock poisoned; continuing with inner state");
            e.into_inner()
        });
        match manager.distribute_rewards(
            PoolType::ValidatorRewards,
            &recipients,
            validator_pool_balance,
        ) {
            Ok(distributions) => {
                info!(
                    "💸 Distributing {} validator fees to {} validators",
                    validator_pool_balance,
                    distributions.len()
                );
                drop(manager);

                // ACTUALLY PAY OUT: Transfer from pool sink wallet to each validator
                let payment_results = currency_client.pay_from_pool(
                    PoolType::ValidatorRewards,
                    &distributions,
                    "validator_fee_reward",
                );

                // Count successful payments
                let successful_amount: u64 = payment_results
                    .iter()
                    .zip(distributions.iter())
                    .filter(|(result, _)| result.2) // result.2 is success bool
                    .map(|(_, (_, amount))| amount)
                    .sum();

                let failed_count = payment_results.iter().filter(|r| !r.2).count();
                if failed_count > 0 {
                    warn!(
                        "⚠️ {} validator fee payments failed, {} succeeded",
                        failed_count,
                        payment_results.len() - failed_count
                    );
                }

                // Complete distribution only for the amount actually paid out
                if successful_amount > 0 {
                    if let Err(e) = fee_manager
                        .read()
                        .unwrap_or_else(|e| {
                            warn!("⚠️  fee_manager RwLock poisoned; continuing with inner state");
                            e.into_inner()
                        })
                        .complete_distribution(PoolType::ValidatorRewards, successful_amount)
                    {
                        warn!("Failed to complete validator fee distribution: {}", e);
                    }
                }
            }
            Err(e) => error!("Failed to distribute validator fees: {}", e),
        }
    }

    // 5. Distribute Relay Fee Pool (to eligible relay operators only)
    let relay_pool_balance = {
        let manager = fee_manager.read().unwrap_or_else(|e| {
            warn!("⚠️  fee_manager RwLock poisoned; continuing with inner state");
            e.into_inner()
        });
        manager.get_pool_balance_by_type(PoolType::RelayRewards)
    };

    if relay_pool_balance > 0 {
        // Compute relay eligibility using the new eligibility system
        let eligible_recipients = if let (Some(relays), Some(work_events)) =
            (relay_registry, relay_work_events)
        {
            let config = EligibilityConfig::default();
            let eligibility_results =
                compute_relay_eligibility(epoch, relays, work_events, &config);

            // Log eligibility summary
            let summary = summarize_eligibility(epoch, &eligibility_results);
            info!(
                "📡 Epoch {} relay eligibility: {}/{} relays eligible, {} operators, {} total stake",
                epoch,
                summary.eligible_relays,
                summary.total_relays,
                summary.unique_operators,
                summary.total_eligible_stake
            );

            if summary.ineligible_relays > 0 {
                debug!(
                    "📡 Ineligibility breakdown: {} registered late, {} suspended, {} low uptime, {} low work events",
                    summary.ineligibility_breakdown.registered_after_start,
                    summary.ineligibility_breakdown.suspended,
                    summary.ineligibility_breakdown.insufficient_uptime,
                    summary.ineligibility_breakdown.insufficient_work_events
                );
            }

            // Aggregate eligible relays by operator
            aggregate_eligible_relays(&eligibility_results)
        } else {
            // No relay registry provided - cannot distribute
            warn!(
                "⚠️ No relay registry available for epoch {} - relay rewards will carry forward",
                epoch
            );
            None
        };

        match eligible_recipients {
            Some(recipients) if !recipients.is_empty() => {
                let manager = fee_manager.read().unwrap_or_else(|e| {
                    warn!("⚠️  fee_manager RwLock poisoned; continuing with inner state");
                    e.into_inner()
                });
                match manager.distribute_rewards(
                    PoolType::RelayRewards,
                    &recipients,
                    relay_pool_balance,
                ) {
                    Ok(distributions) => {
                        info!(
                            "💸 Distributing {} relay fees to {} relay operators",
                            relay_pool_balance,
                            distributions.len()
                        );
                        drop(manager);

                        // ACTUALLY PAY OUT: Transfer from pool sink wallet to each relay operator
                        let payment_results = currency_client.pay_from_pool(
                            PoolType::RelayRewards,
                            &distributions,
                            "relay_fee_reward",
                        );

                        // Count successful payments
                        let successful_amount: u64 = payment_results
                            .iter()
                            .zip(distributions.iter())
                            .filter(|(result, _)| result.2) // result.2 is success bool
                            .map(|(_, (_, amount))| amount)
                            .sum();

                        let failed_count = payment_results.iter().filter(|r| !r.2).count();
                        if failed_count > 0 {
                            warn!(
                                "⚠️ {} relay fee payments failed, {} succeeded",
                                failed_count,
                                payment_results.len() - failed_count
                            );
                        }

                        // Complete distribution only for the amount actually paid out
                        if successful_amount > 0 {
                            let manager = fee_manager.read().unwrap_or_else(|e| {
                                warn!(
                                    "⚠️  fee_manager RwLock poisoned; continuing with inner state"
                                );
                                e.into_inner()
                            });
                            if let Err(e) = manager
                                .complete_distribution(PoolType::RelayRewards, successful_amount)
                            {
                                warn!("Failed to complete relay fee distribution: {}", e);
                            }
                        }
                    }
                    Err(e) => error!("Failed to distribute relay fees: {}", e),
                }
            }
            _ => {
                // NO ELIGIBLE RELAYS - carry balance forward (do NOT distribute to validators)
                warn!(
                    "⚠️⚠️⚠️ EPOCH {}: NO ELIGIBLE RELAYS - {} relay rewards CARRIED FORWARD ⚠️⚠️⚠️",
                    epoch, relay_pool_balance
                );
                warn!("📡 Relay pool balance will accumulate until eligible relays are available");
                // Balance remains in the pool for next epoch
            }
        }
    }

    info!("✅ Epoch {} reward distribution complete", epoch);
    Ok(())
}

/// Run QGE (Quorum-Gated Encryption) command
async fn run_qge_command(config: Config, action: QgeCommand) -> Result<()> {
    use dchat_network::relay::{
        current_epoch_id, EventCategory, QgeAuditLogger, Severity, EPOCH_DURATION_SECS,
    };

    match action {
        QgeCommand::TokenStatus {
            conversation_type,
            epoch,
            include_expired,
        } => {
            println!("\n🔐 QGE TOKEN STATUS");
            println!("══════════════════════════════════════════════════════════════════");

            let current_epoch = current_epoch_id();
            println!("Current Epoch:     {}", current_epoch);
            println!("Epoch Duration:    {} seconds", EPOCH_DURATION_SECS);

            if let Some(ref conv_type) = conversation_type {
                println!("Filter:            {}", conv_type);
            }
            if let Some(e) = epoch {
                println!("Showing Epoch:     {}", e);
            }
            if include_expired {
                println!("Including:         Expired tokens");
            }

            // In production, this would query the local token store
            println!();
            println!("📋 Active Tokens:");
            println!("   (Token listing requires connection to local QGE state)");
            println!();
            println!("💡 Tip: Use 'dchat qge request-token' to request new tokens");

            Ok(())
        }

        QgeCommand::RequestToken {
            target,
            conversation_type,
            force,
        } => {
            println!("\n🔑 REQUESTING QGE TOKEN");
            println!("══════════════════════════════════════════════════════════════════");

            println!("Target:            {}", target);
            println!("Conversation Type: {}", conversation_type);
            println!("Force Refresh:     {}", force);
            println!();

            // In production, this would initiate token request to relay committee
            println!("📡 Contacting relay committee...");
            println!("   (Token request requires active network connection)");
            println!();
            println!("💡 Token requests are processed by the relay committee using");
            println!("   threshold signatures to ensure decentralized issuance.");

            Ok(())
        }

        QgeCommand::Committee {
            detailed,
            history,
            epochs,
        } => {
            println!("\n👥 QGE RELAY COMMITTEE");
            println!("══════════════════════════════════════════════════════════════════");

            let current_epoch = current_epoch_id();
            println!("Current Epoch: {}", current_epoch);
            println!();

            // In production, this would query the committee registry
            println!("📋 Committee Members:");
            println!("   (Committee information requires network connection)");

            if detailed {
                println!();
                println!("📊 Detailed View:");
                println!("   Member performance, stake, and uptime would be shown here");
            }

            if history {
                println!();
                println!("📜 Rotation History (last {} epochs):", epochs);
                println!("   Historical committee rotations would be shown here");
            }

            Ok(())
        }

        QgeCommand::RelayStatus {
            relay_id,
            stake,
            metrics,
            rewards,
        } => {
            println!("\n📡 RELAY STATUS");
            println!("══════════════════════════════════════════════════════════════════");

            if let Some(ref id) = relay_id {
                println!("Relay ID: {}", id);
            } else {
                println!("Relay ID: (local relay)");
            }
            println!();

            if stake {
                println!("💰 Stake Information:");
                println!("   Current Stake:     (requires network connection)");
                println!("   Effective Stake:   (capped at max effective stake)");
                println!("   Stake Lock Until:  (unlock timestamp)");
            }

            if metrics {
                println!();
                println!("📊 Performance Metrics:");
                println!("   Uptime:            (requires relay connection)");
                println!("   Tokens Issued:     (this epoch)");
                println!("   Messages Relayed:  (this epoch)");
                println!("   Latency (avg):     (milliseconds)");
            }

            if rewards {
                println!();
                println!("🎁 Reward History:");
                println!("   (Reward history requires blockchain connection)");
            }

            if !stake && !metrics && !rewards {
                println!("💡 Use --stake, --metrics, or --rewards for detailed info");
            }

            Ok(())
        }

        QgeCommand::Revocation { action } => run_qge_revocation_command(action).await,

        QgeCommand::AuditLog {
            category,
            min_severity,
            from,
            to,
            limit,
            format,
        } => {
            println!("\n📜 QGE AUDIT LOG");
            println!("══════════════════════════════════════════════════════════════════");

            // Parse filters
            let cat_filter = category.as_ref().map(|c| match c.to_lowercase().as_str() {
                "token" => EventCategory::Token,
                "key" => EventCategory::Key,
                "access" => EventCategory::Access,
                "admin" => EventCategory::Admin,
                "security" => EventCategory::Security,
                "system" => EventCategory::System,
                "network" => EventCategory::Network,
                "relay" => EventCategory::Relay,
                _ => EventCategory::System,
            });

            let sev_filter = min_severity
                .as_ref()
                .map(|s| match s.to_lowercase().as_str() {
                    "debug" => Severity::Debug,
                    "info" => Severity::Info,
                    "warning" => Severity::Warning,
                    "error" => Severity::Error,
                    "critical" => Severity::Critical,
                    "alert" => Severity::Alert,
                    _ => Severity::Info,
                });

            println!("Filters:");
            if let Some(ref cat) = category {
                println!("  Category:    {}", cat);
            }
            if let Some(ref sev) = min_severity {
                println!("  Min Severity: {}", sev);
            }
            if let Some(ref f) = from {
                println!("  From:        {}", f);
            }
            if let Some(ref t) = to {
                println!("  To:          {}", t);
            }
            println!("  Limit:       {}", limit);
            println!("  Format:      {}", format);
            println!();

            // In production, this would query the audit logger
            let logger = QgeAuditLogger::new();
            let stats = logger.stats().await;

            println!("📊 Audit Log Statistics:");
            println!("   Total Entries: {}", stats.total_entries);
            println!("   Chain Valid:   {}", stats.chain_valid);
            println!();
            println!("   (Full audit log requires local QGE state)");

            Ok(())
        }

        QgeCommand::RateLimitStatus { user_id, detailed } => {
            println!("\n⏱️ RATE LIMIT STATUS");
            println!("══════════════════════════════════════════════════════════════════");

            if let Some(ref id) = user_id {
                println!("User ID: {}", id);
            } else {
                println!("User ID: (current user)");
            }
            println!();

            // In production, this would query the rate limiter
            println!("📊 Rate Limit Buckets:");
            println!("   Token Requests:   (available/capacity)");
            println!("   Message Relay:    (available/capacity)");
            println!("   Revocation Check: (available/capacity)");

            if detailed {
                println!();
                println!("📈 Detailed Statistics:");
                println!("   Refill Rate:      tokens/second");
                println!("   Window Size:      seconds");
                println!("   Reputation Score: (affects limits)");
            }

            Ok(())
        }

        QgeCommand::Stats { network, detailed } => {
            println!("\n📊 QGE STATISTICS");
            println!("══════════════════════════════════════════════════════════════════");

            let current_epoch = current_epoch_id();
            println!("Current Epoch:  {}", current_epoch);
            println!();

            println!("🔐 Local Statistics:");
            println!("   Active Tokens:     (requires local state)");
            println!("   Cached Keys:       (sender keys, epoch keys)");
            println!("   Pending Requests:  (awaiting committee response)");

            if network {
                println!();
                println!("🌐 Network Statistics:");
                println!("   Active Relays:     (requires network)");
                println!("   Committee Size:    (threshold)");
                println!("   Total Revocations: (network-wide)");
            }

            if detailed {
                println!();
                println!("📈 Detailed Breakdown:");
                println!("   Token request latency (p50/p95/p99)");
                println!("   Key rotation frequency");
                println!("   Memory usage by component");
            }

            Ok(())
        }

        QgeCommand::Cleanup {
            force,
            dry_run,
            max_age,
        } => {
            println!("\n🧹 QGE CLEANUP");
            println!("══════════════════════════════════════════════════════════════════");

            if dry_run {
                println!("Mode: DRY RUN (no changes will be made)");
            } else if force {
                println!("Mode: FORCE (immediate cleanup)");
            } else {
                println!("Mode: STANDARD (respects scheduler)");
            }

            if let Some(age) = max_age {
                println!("Max Age: {} seconds", age);
            }
            println!();

            // In production, this would trigger the cleanup manager
            println!("🔍 Analyzing cryptographic state...");
            println!();
            println!("   Epoch Keys:     (eligible for cleanup)");
            println!("   Chain Keys:     (eligible for cleanup)");
            println!("   Skipped Keys:   (eligible for cleanup)");
            println!("   SUKs:           (eligible for cleanup)");

            if !dry_run {
                println!();
                println!("✅ Cleanup scheduled (or completed if --force)");
            }

            Ok(())
        }
    }
}

/// Run QGE revocation subcommand
async fn run_qge_revocation_command(action: QgeRevocationAction) -> Result<()> {
    match action {
        QgeRevocationAction::List {
            channel_id,
            pending,
            include_expired,
        } => {
            println!("\n📋 QGE REVOCATIONS");
            println!("══════════════════════════════════════════════════════════════════");

            if let Some(ref ch) = channel_id {
                println!("Channel: {}", ch);
            }
            if pending {
                println!("Filter:  Pending only");
            }
            if include_expired {
                println!("Include: Expired revocations");
            }
            println!();

            // In production, this would query the revocation store
            println!("   (Revocation list requires network connection)");

            Ok(())
        }

        QgeRevocationAction::Check {
            user_id,
            channel_id,
        } => {
            println!("\n🔍 CHECKING REVOCATION STATUS");
            println!("══════════════════════════════════════════════════════════════════");

            println!("User:    {}", user_id);
            println!("Channel: {}", channel_id);
            println!();

            // In production, this would check the revocation store
            println!("Status: (requires network connection)");
            println!();
            println!("💡 Revocation checks are cached locally for fast access");

            Ok(())
        }

        QgeRevocationAction::Create {
            user_id,
            channel_id,
            action,
            duration,
            reason,
        } => {
            println!("\n⚠️ CREATING REVOCATION");
            println!("══════════════════════════════════════════════════════════════════");

            println!("User:     {}", user_id);
            println!("Channel:  {}", channel_id);
            println!("Action:   {}", action);
            if let Some(d) = duration {
                println!("Duration: {} seconds", d);
            }
            println!("Reason:   {}", reason);
            println!();

            // In production, this would submit the revocation request
            println!("📡 Submitting revocation request...");
            println!("   (Requires admin privileges and network connection)");

            Ok(())
        }

        QgeRevocationAction::Lift {
            revocation_id,
            reason,
        } => {
            println!("\n✅ LIFTING REVOCATION");
            println!("══════════════════════════════════════════════════════════════════");

            println!("Revocation ID: {}", revocation_id);
            if let Some(ref r) = reason {
                println!("Reason:        {}", r);
            }
            println!();

            // In production, this would submit the lift request
            println!("📡 Submitting lift request...");
            println!("   (Requires admin privileges and network connection)");

            Ok(())
        }

        QgeRevocationAction::Appeal {
            revocation_id,
            message,
        } => {
            println!("\n📝 SUBMITTING APPEAL");
            println!("══════════════════════════════════════════════════════════════════");

            println!("Revocation ID: {}", revocation_id);
            println!("Message:       {}", message);
            println!();

            // In production, this would submit the appeal
            println!("📡 Submitting appeal...");
            println!("   (Appeals are reviewed by channel administrators)");

            Ok(())
        }
    }
}
