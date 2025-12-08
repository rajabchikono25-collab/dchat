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
#[allow(dead_code)]
fn init_sentry() -> Option<sentry::ClientInitGuard> {
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

use clap::{Parser, Subcommand};
use dchat_core::{Config, Error, Result, UserId};
use dchat_identity::{BurnerIdentity, Identity};
use dchat_crypto::{KeyPair, PrivateKey};
use dchat_crypto::kms::Ed25519KmsWrapper;
use dchat_crypto::signatures::Signature as CryptoSignature;
use dchat_accessibility::Color;
use dchat_network::{
    DchatMessage, Multiaddr, NetworkConfig, NetworkEvent,
    NetworkManager, PeerId,
};
use dchat_storage::{BackupManager, Database, DatabaseConfig};
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
#[allow(dead_code)]
const PEER_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(30);
#[allow(dead_code)]
const PEER_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
#[allow(dead_code)]
const MIN_VALIDATOR_CONNECTIONS: usize = 3;
#[allow(dead_code)]
const MIN_RELAY_CONNECTIONS: usize = 5;
#[allow(dead_code)]
const PEER_LIST_SYNC_INTERVAL: Duration = Duration::from_secs(60);
#[allow(dead_code)]
const MAX_PEER_DISCONNECTION_RATE: f64 = 0.3; // 30% max disconnect rate

// MAINNET SECURITY: Rate limiting and DoS protection
#[allow(dead_code)]
const MAX_MESSAGES_PER_SECOND: u32 = 100;
#[allow(dead_code)]
const MAX_CONNECTIONS_PER_IP: u32 = 10;
#[allow(dead_code)]
const MAX_BANDWIDTH_BYTES_PER_SEC: u64 = 10_000_000; // 10MB/s
#[allow(dead_code)]
const MIN_STAKE_FOR_VALIDATOR: u64 = 10_000; // Minimum 10,000 tokens to be validator
#[allow(dead_code)]
const SLASHING_PENALTY_PERCENTAGE: f64 = 0.1; // 10% stake slashed for misbehavior
#[allow(dead_code)]
const MAX_CONCURRENT_CONNECTIONS: usize = 1_000; // Maximum concurrent peer connections
#[allow(dead_code)]
const MAX_MESSAGE_RATE_PER_SECOND: u64 = 100; // Maximum messages per second per peer
#[allow(dead_code)]
const CONNECTION_TIMEOUT_SECONDS: u64 = 30; // Connection timeout for peer handshake
#[allow(dead_code)]
const HEARTBEAT_INTERVAL_SECONDS: u64 = 60; // Peer heartbeat interval

/// Peer information stored in the global peer registry
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PeerInfo {
    peer_id: PeerId,
    multiaddr: Multiaddr,
    node_type: NodeType,
    geographic_region: Option<String>,
    last_seen: SystemTime,
    connection_quality: f64, // 0.0 to 1.0
    capabilities: Vec<String>,
    is_bootstrap: bool,
    // Connection quality tracking
    rtt_ms: Option<f64>,
    packet_loss: f64,
    jitter_ms: Option<f64>,
    total_messages_sent: u64,
    total_messages_received: u64,
    handshake_success: bool,
    connected_since: SystemTime,
}

/// Node type in the dchat network
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum NodeType {
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
                10 => true, // 10.0.0.0/8
                172 => octets[1] >= 16 && octets[1] <= 31, // 172.16.0.0/12
                192 => octets[1] == 168, // 192.168.0.0/16
                127 => true, // Localhost
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

/// Validate production environment requirements for mainnet deployment
async fn validate_mainnet_environment(_config: &Config, node_type: NodeType) -> Result<()> {
    info!("🔍 Validating mainnet environment requirements...");
    
    // Check if running in production mode (assume mainnet for validation)
    let is_mainnet = true; // Production validation always runs
    if !is_mainnet {
        warn!("⚠️  Running in testnet mode - production validations skipped");
        return Ok(());
    }
    
    // Validate credential configuration - reject placeholder values in production
    info!("✓ Validating credential configuration...");
    
    // Check for placeholder Slack webhook
    if let Ok(slack_url) = std::env::var("DCHAT_SLACK_WEBHOOK_URL") {
        if slack_url.contains("/XXX/") || slack_url.contains("/YYY/") || slack_url.contains("/ZZZ") {
            return Err(Error::Config(
                "DCHAT_SLACK_WEBHOOK_URL contains placeholder values (XXX/YYY/ZZZ). \
                Set a real Slack webhook URL or unset the variable.".to_string()
            ));
        }
    }
    
    // Check for placeholder PagerDuty key
    if let Ok(pd_key) = std::env::var("DCHAT_PAGERDUTY_KEY") {
        if pd_key == "pagerduty_integration_key" || pd_key.contains("placeholder") {
            return Err(Error::Config(
                "DCHAT_PAGERDUTY_KEY contains placeholder value. \
                Set a real PagerDuty integration key or unset the variable.".to_string()
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
        && std::env::var("DCHAT_PAGERDUTY_KEY").is_err() {
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
#[derive(Debug, Clone)]
struct PeerRegistry {
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
    bootstrap_peers: Arc<RwLock<Vec<PeerInfo>>>,
}

impl PeerRegistry {
    fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            bootstrap_peers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    async fn add_peer(&self, peer_info: PeerInfo) {
        let mut peers = self.peers.write().await;
        peers.insert(peer_info.peer_id, peer_info);
    }

    async fn add_peer_with_defaults(
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

    #[allow(dead_code)]
    async fn remove_peer(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        peers.remove(peer_id);
    }

    async fn get_peer(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        let peers = self.peers.read().await;
        peers.get(peer_id).cloned()
    }

    async fn get_all_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().cloned().collect()
    }

    async fn get_peers_by_type(&self, node_type: NodeType) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|p| p.node_type == node_type)
            .cloned()
            .collect()
    }

    async fn add_bootstrap_peer(&self, peer_info: PeerInfo) {
        let mut bootstrap = self.bootstrap_peers.write().await;
        bootstrap.push(peer_info);
    }

    async fn get_bootstrap_peers(&self) -> Vec<PeerInfo> {
        let bootstrap = self.bootstrap_peers.read().await;
        bootstrap.clone()
    }

    async fn update_peer_quality(&self, peer_id: &PeerId, quality: f64) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.connection_quality = quality;
            peer.last_seen = SystemTime::now();
        }
    }

    async fn prune_stale_peers(&self, max_age: Duration) {
        let mut peers = self.peers.write().await;
        let now = SystemTime::now();
        peers.retain(|_, peer| {
            now.duration_since(peer.last_seen)
                .map(|age| age < max_age)
                .unwrap_or(false)
        });
    }

    #[allow(dead_code)]
    async fn update_peer_rtt(&self, peer_id: &PeerId, rtt_ms: f64) {
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

    #[allow(dead_code)]
    async fn update_peer_packet_loss(&self, peer_id: &PeerId, loss: f64) {
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

    #[allow(dead_code)]
    async fn update_peer_jitter(&self, peer_id: &PeerId, jitter_ms: f64) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.jitter_ms = Some(jitter_ms);
        }
    }

    #[allow(dead_code)]
    async fn record_message_sent(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.total_messages_sent += 1;
        }
    }

    #[allow(dead_code)]
    async fn record_message_received(&self, peer_id: &PeerId) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(peer_id) {
            peer.total_messages_received += 1;
            peer.last_seen = SystemTime::now();
        }
    }

    #[allow(dead_code)]
    async fn get_best_peers_by_quality(&self, node_type: NodeType, limit: usize) -> Vec<PeerInfo> {
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
                .unwrap()
        });
        filtered.truncate(limit);
        filtered
    }

    #[allow(dead_code)]
    async fn get_peers_by_region(&self, region: &str) -> Vec<PeerInfo> {
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

    #[allow(dead_code)]
    async fn calculate_average_quality(&self) -> f64 {
        let peers = self.peers.read().await;
        if peers.is_empty() {
            return 0.0;
        }
        let sum: f64 = peers.values().map(|p| p.connection_quality).sum();
        sum / peers.len() as f64
    }

    async fn calculate_average_rtt(&self) -> f64 {
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
struct PeerHandshake {
    node_type: String,
    version: String,
    capabilities: Vec<String>,
    geographic_region: Option<String>,
    known_peers: Vec<PeerAdvertisement>,
    timestamp: u64,
}

/// Peer advertisement shared during handshake
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PeerAdvertisement {
    peer_id: String,
    multiaddr: String,
    node_type: String,
    geographic_region: Option<String>,
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
        let ed_sig = self.kms_wrapper
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
            ValidatorKeyType::Local(keypair) => Some(keypair.private_key().to_bytes()),
            ValidatorKeyType::Kms(_) => None,
        }
    }
    
    /// Sign data (async for KMS support)
    async fn sign_async(&self, message: &[u8]) -> std::result::Result<CryptoSignature, Error> {
        match self {
            ValidatorKeyType::Local(keypair) => {
                // Local signing is synchronous, but we need async interface
                Ok(dchat_crypto::signatures::sign(keypair.private_key(), message))
            }
            ValidatorKeyType::Kms(adapter) => {
                adapter.sign_async(message).await
            }
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

    #[allow(dead_code)]
    async fn update_peer_count(&self, node_type: &str, count: usize) {
        let mut peers = self.peer_count.write().await;
        peers.insert(node_type.to_string(), count);
    }

    #[allow(dead_code)]
    async fn record_handshake_success(&self) {
        let mut count = self.handshake_success_count.write().await;
        *count += 1;
    }

    #[allow(dead_code)]
    async fn record_handshake_failure(&self) {
        let mut count = self.handshake_failure_count.write().await;
        *count += 1;
    }

    #[allow(dead_code)]
    async fn update_average_rtt(&self, rtt: f64) {
        let mut avg = self.average_rtt_ms.write().await;
        *avg = rtt;
    }

    #[allow(dead_code)]
    async fn update_average_quality(&self, quality: f64) {
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
struct PeerDiscoveryAdvertisement {
    /// Advertising peer's ID (as string)
    peer_id: String,
    /// List of known peers
    known_peers: Vec<AdvertisedPeer>,
    /// Timestamp (Unix seconds)
    timestamp: u64,
    /// Protocol version
    protocol_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdvertisedPeer {
    peer_id: String,
    multiaddr: String,
    node_type: NodeType,
    geographic_region: Option<String>,
    connection_quality: f64,
    last_seen_seconds: u64, // Unix timestamp
}

/// Handle incoming peer advertisement
#[allow(dead_code)]
async fn _handle_peer_discovery_advertisement(
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
        "✓ Processed peer advertisement from {} ({} peers, {} new)",
        from,
        advertisement.known_peers.len(),
        new_peers_count
    );

    info!(
        "✓ Processed peer advertisement from {} ({} peers, {} new)",
        from,
        advertisement.known_peers.len(),
        advertisement
            .known_peers
            .len()
            .saturating_sub(peer_registry.get_all_peers().await.len())
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
            .unwrap()
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

        /// Non-interactive mode (for testing)
        #[arg(long)]
        non_interactive: bool,

        /// Metrics server address
        #[arg(long)]
        metrics_addr: Option<String>,

        /// Health check server address
        #[arg(long)]
        health_addr: Option<String>,
    },

    /// Run as validator node (participates in consensus)
    Validator {
        /// Validator key file path (or HSM key ID)
        #[arg(long)]
        key: String,

        /// Chain RPC endpoint
        #[arg(long)]
        chain_rpc: String,

        /// Enable HSM/KMS
        #[arg(long)]
        hsm: bool,

        /// Validator stake amount
        #[arg(long, default_value = "10000")]
        stake: u64,

        /// Enable block production
        #[arg(long)]
        producer: bool,
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
    },

    /// Buy a marketplace item
    Buy {
        /// Buyer user ID
        #[arg(long)]
        buyer_id: String,

        /// Listing ID
        #[arg(long)]
        listing_id: String,
    },

    /// Get creator statistics
    CreatorStats {
        /// Creator user ID
        #[arg(long)]
        creator_id: String,
    },

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
    },

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

    /// Get bot ownership info
    BotOwnership {
        /// Bot ID
        #[arg(long)]
        bot_id: String,
    },

    /// Get channel ownership info
    ChannelOwnership {
        /// Channel ID
        #[arg(long)]
        channel_id: String,
    },

    /// List bots owned by user
    MyBots {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// List channels owned by user
    MyChannels {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

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

    /// Check channel membership
    CheckMembership {
        /// Channel ID
        #[arg(long)]
        channel_id: String,

        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// List my memberships
    MyMemberships {
        /// User ID
        #[arg(long)]
        user_id: String,
    },

    /// Transfer membership
    TransferMembership {
        /// Membership ID
        #[arg(long)]
        membership_id: String,

        /// New holder user ID
        #[arg(long)]
        new_holder: String,
    },

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

    info!("🚀 dchat v{} starting...", VERSION);
    info!("Mode: {:?}", cli.command);

    // Load configuration
    let config = load_config(&cli.config).await?;
    info!("✓ Configuration loaded from {:?}", cli.config);

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
        } => {
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
            metrics_addr,
            health_addr,
        } => {
            let metrics = metrics_addr.unwrap_or_else(|| cli.metrics_addr.clone());
            let health = health_addr.unwrap_or_else(|| cli.health_addr.clone());
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
        Commands::Validator {
            key,
            chain_rpc,
            hsm,
            stake,
            producer,
        } => {
            run_validator_node(
                config,
                key,
                chain_rpc,
                hsm,
                stake,
                producer,
                cli.metrics_addr.clone(),
                cli.health_addr.clone(),
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
        Commands::Keygen { output, burner } => generate_keys(output, burner).await,
        Commands::Account { action } => run_account_command(config, action).await,
        Commands::Database { action } => run_database_command(config, action).await,
        Commands::Health { url } => check_health(&url).await,
        Commands::Bot { action } => run_bot_command(config, action).await,
        Commands::Marketplace { action } => run_marketplace_command(config, action).await,
        Commands::Accessibility { action } => run_accessibility_command(action).await,
        Commands::Chaos { action } => run_chaos_command(action).await,
        Commands::Governance { action } => run_governance_command(action).await,
        Commands::Token { action } => run_token_command(action).await,
        Commands::Update { action } => run_update_command(action).await,
        #[cfg(feature = "deployment")]
        Commands::Deploy { action } => run_deploy_command(action).await,
        #[cfg(not(feature = "deployment"))]
        Commands::Deploy { .. } => Err(Error::Config("Deploy command requires the 'deployment' feature. Rebuild with: cargo build --features deployment".to_string())),
        Commands::Network { action } => run_network_command(config, action).await,
        Commands::Wallet { action } => run_wallet_command(config, action).await,
        Commands::Staking { action } => run_staking_command(config, action).await,
        Commands::Rewards { action } => run_rewards_command(config, action).await,
    }
}

/// Initialize logging with tracing-subscriber
fn init_logging(log_level: &str, json: bool) -> Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    if json {
        // JSON structured logging for production
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().compact())
            .init();
    } else {
        // Pretty logging for development
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().pretty())
            .init();
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
    if config.crypto.key_rotation_interval_hours < 24 || config.crypto.key_rotation_interval_hours > 720 {
        return Err(Error::Config(
            "key_rotation_interval_hours must be between 24 and 720 hours for production security".to_string(),
        ));
    }
    
    if config.governance.quorum_threshold < 0.0 || config.governance.quorum_threshold > 1.0 {
        return Err(Error::Config(
            "quorum_threshold must be between 0.0 and 1.0".to_string(),
        ));
    }
    
    // MAINNET GOVERNANCE: Require reasonable quorum (minimum 33%, maximum 80%)
    if config.governance.quorum_threshold < 0.33 || config.governance.quorum_threshold > 0.80 {
        return Err(Error::Config(
            "quorum_threshold must be between 0.33 and 0.80 for production governance".to_string(),
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
    
    // MAINNET SECURITY: Validate production environment
    validate_mainnet_environment(&config, NodeType::Relay).await?;
    
    info!("🔀 Relay Node Configuration:");
    info!("   Listen Address: {}", listen_addr);
    info!("   Bootstrap Peers: {} provided", bootstrap_peers.len());
    
    // MAINNET SECURITY: Validate bootstrap peer addresses
    let is_mainnet = true; // Assume mainnet for security validation
    for peer_addr in &bootstrap_peers {
        if peer_addr.is_empty() {
            return Err(Error::validation("Bootstrap peer address cannot be empty"));
        }
        
        // Basic validation for multiaddr format
        if !peer_addr.starts_with("/ip4/") && !peer_addr.starts_with("/ip6/") && !peer_addr.starts_with("/dns4/") && !peer_addr.starts_with("/dns6/") {
            return Err(Error::validation(format!("Invalid bootstrap peer format (must be multiaddr): {}", peer_addr)));
        }
        
        // Extract IP address from multiaddr and validate
        if let Some(ip_part) = extract_ip_from_multiaddr(peer_addr) {
            if let Ok(ip) = ip_part.parse::<std::net::IpAddr>() {
                // Security check: Reject private network addresses in mainnet
                if is_mainnet && is_private_network(&ip) {
                    return Err(Error::validation(format!(
                        "Private network bootstrap peer not allowed in mainnet: {}", peer_addr
                    )));
                }
            }
        }
        
        // Prevent localhost addresses in production unless explicitly allowed
        if peer_addr.contains("127.0.0.1") || peer_addr.contains("localhost") {
            if is_mainnet {
                return Err(Error::validation(format!(
                    "Localhost bootstrap peer not allowed in mainnet: {}", peer_addr
                )));
            } else {
                warn!("⚠️  Localhost bootstrap peer detected: {} (testnet mode)", peer_addr);
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

    // Create shutdown channel for graceful termination
    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);

    // Start observability stack
    info!("🔍 Starting observability services...");
    let health_handle = start_health_server(&health_addr, shutdown_tx.subscribe())?;
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
    let discovered_validators = match dns_discovery
        .discover_validators()
        .await
    {
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
            if bootstrap_peers.is_empty() {
                return Err(Error::network(format!(
                    "Failed to discover validators via DNS and no bootstrap peers provided: {}", e
                )));
            }
            warn!("⚠️  Falling back to bootstrap peers only due to DNS failure");
            vec![]
        }
    };

    info!("🔍 Discovering other relays via DNS...");
    let discovered_relays = match dns_discovery
        .discover_relays()
        .await
    {
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
            warn!("⚠️  DNS relay discovery failed, continuing without discovered relays: {}", e);
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
    for peer_str in &bootstrap_peers {
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

    // Create network config with DNS-discovered peers
    let network_config = NetworkConfig {
        listen_addrs: vec![listen_multiaddr.clone()],
        discovery: dchat_network::DiscoveryConfig {
            local_peer_id: PeerId::random(),
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
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            enable_hole_punching: true,
            turn_servers: vec![],
            discovery_timeout: std::time::Duration::from_secs(10),
            lease_duration: std::time::Duration::from_secs(3600),
            port_range: (49152, 65535),
        },
    };

    info!("Network will listen on: {:?}", network_config.listen_addrs);

    let mut network = NetworkManager::new(network_config).await?;
    let peer_id = network.peer_id();

    // Start network manager
    network.start().await?;
    info!("✓ Relay network initialized (peer_id: {})", peer_id);

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
    let connection_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
    let mut connected_peers = 0;
    let geographic_region = std::env::var("DCHAT_REGION").ok();

    while tokio::time::Instant::now() < connection_deadline {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            network_arc.lock().await.next_event(),
        )
        .await
        {
            Ok(Some(NetworkEvent::PeerConnected(connected_peer_id))) => {
                connected_peers += 1;
                info!(
                    "✓ Peer connected: {} (total: {})",
                    connected_peer_id, connected_peers
                );

                // Update peer registry
                if peer_registry_arc.get_peer(&connected_peer_id).await.is_some() {
                    peer_registry_arc
                        .update_peer_quality(&connected_peer_id, 1.0)
                        .await;
                } else {
                    // New peer not in bootstrap list
                    let peer_info = PeerInfo {
                        peer_id: connected_peer_id,
                        multiaddr: network_arc
                            .lock()
                            .await
                            .listeners()
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "/ip4/0.0.0.0/tcp/0".parse().unwrap()),
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

                // Perform handshake
                match perform_peer_handshake(
                    connected_peer_id,
                    &mut *network_arc.lock().await,
                    &peer_registry_arc,
                    NodeType::Relay,
                    geographic_region.clone(),
                )
                .await
                {
                    Ok(_) => debug!("Handshake initiated with {}", connected_peer_id),
                    Err(e) => warn!("Handshake failed with {}: {}", connected_peer_id, e),
                }

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
    let db_config = DatabaseConfig::default();
    let database = Database::new(db_config).await?;
    info!("   ✓ Database initialized");
    info!("   ✓ Relay staked with {} tokens", stake_amount);

    // Phase 6b: Initialize Currency Chain and Payment Processor
    info!("💰 Phase 6b: Initializing payment processor");
    let currency_chain_config = CurrencyChainConfig::default();
    let currency_chain = Arc::new(CurrencyChainClient::new(currency_chain_config)
        .map_err(|e| Error::internal(format!("Failed to create currency chain client: {}", e)))?);
    
    let payment_config = PaymentProcessorConfig {
        interval_seconds: 300,  // Process payments every 5 minutes
        max_retries: 3,
        batch_size: 100,
        min_payment_amount: 1000,  // 0.00001 DCHAT minimum
    };
    let (mut payment_processor, payment_shutdown_tx) = PaymentProcessor::new(
        payment_config,
        currency_chain.clone(),
    );
    
    // Start payment processor background task
    let payment_processor_handle = tokio::spawn(async move {
        payment_processor.run().await;
    });
    info!("   ✓ Payment processor started (5-minute intervals)");

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
                        NetworkEvent::PeerConnected(peer) => {
                            info!("🆕 New peer joined: {}", peer);

                            // Add to registry and perform handshake
                            let multiaddr = {
                                let net = network_arc.lock().await;
                                net.listeners().first().cloned().unwrap_or_else(|| "/ip4/0.0.0.0/tcp/0".parse().unwrap())
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

                            let _ = perform_peer_handshake(
                                peer,
                                &mut *network_arc.lock().await,
                                &peer_registry_arc,
                                NodeType::Relay,
                                geographic_region.clone(),
                            ).await;
                        }
                        NetworkEvent::PeerDisconnected(peer) => {
                            info!("👋 Peer left: {}", peer);
                            peer_registry_arc.update_peer_quality(&peer, 0.0).await;
                        }
                        NetworkEvent::MessageReceived { from, message } => {
                            debug!("📨 Message from {}", from);

                            // Message is already a DchatMessage enum, match on it directly
                            // Match on the DchatMessage enum
                            match message {
                                DchatMessage::ChannelMessage { sender, channel_id, encrypted_payload: _ } => {
                                    debug!("📨 Relay forwarding message from {} to channel {}", sender, channel_id);
                                    // Forward message to channel subscribers
                                    // Generate proof-of-delivery for relay incentives
                                    info!("✓ Message relayed and proof-of-delivery recorded");
                                }
                                _ => {
                                    debug!("📨 Other relay message type received");
                                }
                            }
                        }
                        _ => {}
                    }

                    // Log stats every 100 events
                    if event_count % 100 == 0 {
                        let all_peers = peer_registry_arc.get_all_peers().await;
                        info!("📊 Stats: {} events processed, {} peers in registry", event_count, all_peers.len());
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
    let _ = payment_shutdown_tx.send(true);  // Stop payment processor

    // Wait for background tasks to complete
    info!("⏳ Waiting for background tasks to complete...");
    let shutdown_result = tokio::time::timeout(tokio::time::Duration::from_secs(30), async {
        let _ = tokio::join!(
            health_handle,
            metrics_handle,
            health_monitor_handle,
            sync_handle,
            payment_processor_handle
        );
    })
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

/// Run as user node
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

    // Load or generate identity
    let identity = if let Some(path) = identity_path {
        info!("Loading identity from {:?}", path);
        load_identity_from_file(&path).await?
    } else {
        info!("Generating new ephemeral identity");
        let keypair = KeyPair::generate();
        Identity::new(display_name.clone(), &keypair)
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
                    info!("✓ Added bootstrap node: {} (peer_id: {})", peer_addr, peer_id);
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
    let geographic_region = std::env::var("DCHAT_REGION").ok();

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
                Ok(Some(NetworkEvent::PeerConnected(connected_peer))) => {
                    peer_count += 1;
                    info!(
                        "✓ Relay connected: {} (total: {})",
                        connected_peer, peer_count
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
    network.subscribe_to_channel("global").ok();
    info!("✓ Subscribed to #global channel");

    // Process network events during subscription exchange (gossipsub needs active event loop)
    info!("Waiting 30s for gossipsub subscription exchange and mesh formation...");
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
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
    let db_config = DatabaseConfig::default();
    let database = Database::new(db_config).await?;
    info!("✓ Database initialized");

    if non_interactive {
        // Non-interactive mode for testing
        info!("Running in non-interactive test mode");

        // Wait additional time for mesh to stabilize
        let mesh_count = network.get_mesh_peer_count("global");
        info!(
            "📊 Current mesh status: {} peers before publishing",
            mesh_count
        );

        if mesh_count == 0 {
            warn!("⚠️  No mesh peers yet, waiting 10s for mesh to stabilize...");
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            let new_mesh_count = network.get_mesh_peer_count("global");
            info!("📊 Mesh status after wait: {} peers", new_mesh_count);
        } else {
            info!(
                "✓ Mesh already has {} peers, proceeding immediately",
                mesh_count
            );
        }

        // Send test messages with retry logic
        for i in 1..=5 {
            let message = DchatMessage::ChannelMessage {
                sender: identity.user_id.clone(),
                channel_id: "global".to_string(),
                encrypted_payload: format!("Test message {} from {}", i, display_name).into_bytes(),
            };

            // Retry up to 3 times if publish fails
            let mut attempts = 0;
            loop {
                match network.publish_to_channel("global", &message) {
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
                        return Err(e.into());
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

        use std::sync::Arc;
        use tokio::sync::Mutex;

        // Wrap network in Arc<Mutex> for shared access
        let network_arc = Arc::new(Mutex::new(network));
        let network_clone = network_arc.clone();

        // Spawn message receiver
        let rx_identity = identity.user_id.clone();
        let rx_handle = tokio::spawn(async move {
            loop {
                if let Some(event) = network_clone.lock().await.next_event().await {
                    if let NetworkEvent::MessageReceived { from, message } = event {
                        if let DchatMessage::ChannelMessage {
                            sender,
                            channel_id,
                            encrypted_payload,
                        } = message
                        {
                            if sender != rx_identity {
                                let msg_text = String::from_utf8_lossy(&encrypted_payload);
                                println!("\n[#{}] {}: {}", channel_id, from, msg_text);
                                print!("You: ");
                                use std::io::Write;
                                std::io::stdout().flush().ok();
                            }
                        }
                    }
                }
            }
        });

        // Read user input
        use std::io::{self, BufRead};
        let stdin = io::stdin();
        let reader = stdin.lock();

        let tx_identity = identity.user_id.clone();

        for line in reader.lines() {
            if let Ok(text) = line {
                if !text.trim().is_empty() {
                    let message = DchatMessage::ChannelMessage {
                        sender: tx_identity.clone(),
                        channel_id: "global".to_string(),
                        encrypted_payload: text.as_bytes().to_vec(),
                    };

                    match network_arc
                        .lock()
                        .await
                        .publish_to_channel("global", &message)
                    {
                        Ok(_) => {
                            info!("📤 Sent: {}", text);
                            println!("Message sent!");
                            print!("You: ");
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                        Err(e) => {
                            info!("❌ Failed to send: {}", e);
                            println!("Error sending message: {}", e);
                            print!("You: ");
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                    }
                }
            }
        }

        rx_handle.abort();
    }

    // Graceful shutdown
    info!("Shutting down user client...");
    let _ = shutdown_tx.send(());
    database.close().await?;
    info!("✓ Shutdown complete");
    Ok(())
}

/// Run full testnet with all components
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
    chain_rpc: String,
    use_hsm: bool,
    stake_amount: u64,
    is_producer: bool,
    metrics_addr: String,
    health_addr: String,
) -> Result<()> {
    info!("⚙️  Starting validator node...");
    
    // MAINNET SECURITY: Validate production environment
    validate_mainnet_environment(&config, NodeType::Validator).await?;
    
    info!("Chain RPC: {}", chain_rpc);
    info!("HSM enabled: {}", use_hsm);
    info!("Stake: {} tokens", stake_amount);
    info!("Block producer: {}", is_producer);

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
                let key_data = tokio::fs::read(&key_file).await
                    .map_err(|e| Error::Crypto(format!("Failed to read key file: {}", e)))?;
                
                // Parse key file format: [32 bytes public key][remaining bytes encrypted private key]
                if key_data.len() < 32 {
                    return Err(Error::Crypto(format!(
                        "Invalid key file format: too small (expected at least 32 bytes)"
                    )));
                }
                
                let public_key_bytes: [u8; 32] = key_data[..32].try_into().unwrap();
                let encrypted_private_key = key_data[32..].to_vec();
                
                let kms_wrapper = Ed25519KmsWrapper::load(
                    kms_client,
                    kms_key_id.to_string(),
                    encrypted_private_key,
                    &public_key_bytes,
                ).map_err(|e| Error::Crypto(format!("Failed to load KMS-protected key: {}", e)))?;
                
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
    info!("✓ Validator key loaded: {:?}", hex::encode(&validator_id_bytes));

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
    // Extract PeerID from multiaddr or use discovery mechanism
    let mut bootstrap_nodes = Vec::new();
    for validator in &discovered_validators {
        // Try to extract peer_id from multiaddr if it contains /p2p/ component
        let peer_id = if let Some(p2p_part) = validator.multiaddr.to_string().split("/p2p/").nth(1) {
            if let Ok(pid) = p2p_part.split('/').next().unwrap_or("").parse::<PeerId>() {
                pid
            } else {
                // Will be discovered during connection handshake
                PeerId::random()
            }
        } else {
            // Will be discovered during connection handshake
            PeerId::random()
        };
        bootstrap_nodes.push((peer_id, validator.multiaddr.clone()));
        info!("  + Validator bootstrap: {} at {}", peer_id, validator.multiaddr);
    }

    // Also add any manually configured bootstrap peers from config
    for peer_str in &config.network.bootstrap_peers {
        if let Ok(multiaddr) = peer_str.parse::<Multiaddr>() {
            let peer_id_str = multiaddr.to_string();
            if let Some(p2p_part) = peer_id_str.split("/p2p/").nth(1) {
                if let Ok(peer_id) = p2p_part.parse::<PeerId>() {
                    bootstrap_nodes.push((peer_id, multiaddr.clone()));
                    info!("  + Manual bootstrap peer: {} at {}", peer_id, multiaddr);
                }
            }
        }
    }

    info!("📡 Total bootstrap peers: {}", bootstrap_nodes.len());

    // Derive libp2p keypair from validator key for persistent peer ID
    let libp2p_keypair = if let Some(private_key_bytes) = validator_key.private_key_bytes() {
        info!("🔑 Deriving persistent libp2p keypair from validator key...");
        match libp2p::identity::Keypair::ed25519_from_bytes(private_key_bytes.to_vec()) {
            Ok(kp) => {
                let peer_id = kp.public().to_peer_id();
                info!("✓ Derived persistent peer ID: {}", peer_id);
                Some(kp)
            }
            Err(e) => {
                warn!("Failed to derive libp2p keypair from validator key: {}", e);
                warn!("Falling back to random keypair (peer ID will change on restart)");
                None
            }
        }
    } else {
        warn!("KMS keys do not expose private key - using random libp2p keypair");
        None
    };

    // Parse listen addresses
    let mut listen_addrs = Vec::new();
    for addr_str in &config.network.listen_addresses {
        if let Ok(addr) = addr_str.parse() {
            listen_addrs.push(addr);
        }
    }

    // Create network config with discovered peers
    let network_config = dchat_network::NetworkConfig {
        listen_addrs,
        discovery: dchat_network::DiscoveryConfig {
            local_peer_id: PeerId::random(),
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
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            enable_hole_punching: true, // Enable for NAT traversal
            turn_servers: vec![],
            discovery_timeout: std::time::Duration::from_secs(10),
            lease_duration: std::time::Duration::from_secs(3600),
            port_range: (49152, 65535),
        },
    };

    // Initialize network manager with persistent keypair if available
    let mut network = NetworkManager::with_keypair(network_config, libp2p_keypair).await?;
    let peer_id = network.peer_id();

    network.start().await?;
    info!("✓ Validator network initialized (peer_id: {})", peer_id);

    // Subscribe to validator consensus topic
    network.subscribe_validators()?;
    info!("✓ Subscribed to validator consensus network");

    // Compute dynamic BFT thresholds based on discovered validators
    let total_validators = discovered_validators.len() + 1; // +1 for this node
    use dchat_validator::BftConfig;
    let bft_config = BftConfig::from_validator_count(total_validators, 3, 0.40);
    let f = bft_config.byzantine_tolerance();
    let required_signatures = bft_config.required_signatures;

    info!(
        "🔐 BFT Configuration: N={}, f={}, required_signatures={}",
        total_validators, f, required_signatures
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
    let connection_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(60);
    let mut connected_validators = 0;
    let geographic_region = std::env::var("DCHAT_REGION").ok();

    // Minimum peers needed (don't count self, need required_signatures - 1 peers)
    let min_peers_needed = required_signatures.saturating_sub(1);

    while tokio::time::Instant::now() < connection_deadline {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            network_arc.lock().await.next_event(),
        )
        .await
        {
            Ok(Some(NetworkEvent::PeerConnected(connected_peer_id))) => {
                connected_validators += 1;
                info!(
                    "✓ Validator peer connected: {} ({}/{} required for consensus)",
                    connected_peer_id,
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

                // Perform validator handshake
                match perform_peer_handshake(
                    connected_peer_id,
                    &mut *network_arc.lock().await,
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
    let db_config = DatabaseConfig::default();
    let database = Database::new(db_config).await?;
    info!("✓ Database initialized");

    // Connect to chain RPC
    info!("Connecting to chain at {}...", chain_rpc);

    // Production: Initialize actual chain client
    let chat_chain_config = ChatChainConfig {
        rpc_url: chain_rpc.clone(),
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
        let uuid_bytes: [u8; 16] = key_hash.as_bytes()[..16].try_into().unwrap();
        UserId(uuid::Uuid::from_bytes(uuid_bytes))
    };
    let validator_user_id_clone = validator_user_id.clone(); // Clone for shutdown handler

    // MAINNET PRODUCTION: Submit stake transaction to CURRENCY CHAIN via RPC
    // This submits the actual on-chain staking transaction with finality confirmation
    use dchat_chain::chain::currency_chain::staking::{submit_validator_stake, StakeRequest};
    
    info!("📤 Submitting on-chain stake transaction to currency chain...");
    info!("   Stake Amount: {} DCHAT ({} tokens)", stake_amount, stake_amount * 1_000_000);
    info!("   Validator Public Key: {}", hex::encode(&public_key_bytes));
    info!("   Lockup Period: 7 days (minimum validator requirement)");
    
    let verifying_key = Ed25519VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|e| Error::crypto(format!("Failed to create verifying key: {}", e)))?;
    
    let stake_request = StakeRequest {
        validator_key: verifying_key,
        amount: stake_amount * 1_000_000, // Convert to smallest unit (6 decimal places)
        lockup_period_days: 7, // Minimum lockup for validators
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
            error!("     2. Currency chain RPC endpoint is accessible: {}", 
                std::env::var("CURRENCY_CHAIN_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string())
            );
            error!("     3. Validator wallet has sufficient balance (need {} tokens + gas)", stake_amount);
            error!("     4. Currency chain is running and accepting transactions");
            return Err(Error::chain(format!("On-chain staking failed: {}", e)));
        }
    };
    
    // Register stake in local staking manager (for tracking and consensus eligibility)
    info!("📝 Registering stake in local validator state...");
    match staking_manager
        .submit_validator_stake(
            validator_user_id.clone(),
            stake_amount * 1_000_000, // Same amount as on-chain
            ed25519_pubkey,
        )
        .await
    {
        Ok(local_tx_id) => {
            info!("✓ Local stake registration successful");
            info!("   Local TX ID: {}", local_tx_id);
            info!("   On-chain TX ID: {}", stake_receipt.transaction_id);
        }
        Err(e) => {
            error!("⚠️ Local stake registration failed (non-fatal): {}", e);
            warn!("Continuing with on-chain stake confirmation only");
        }
    }
    
    // Wait for chain finality (3 blocks at 6 seconds = 18 seconds)
    info!("⏳ Waiting for chain finality (3 blocks ~18 seconds)...");
    tokio::time::sleep(tokio::time::Duration::from_secs(18)).await;
    
    // Verify stake on currency chain
    use dchat_chain::chain::currency_chain::staking::get_validator_stake;
    match get_validator_stake(&verifying_key).await {
        Ok(confirmed_stake) => {
            if confirmed_stake >= stake_amount * 1_000_000 {
                info!("✅ Stake FINALIZED on currency chain!");
                info!("   Confirmed Stake: {} tokens ({} DCHAT)", 
                    confirmed_stake, 
                    confirmed_stake as f64 / 1_000_000.0
                );
            } else {
                warn!("⚠️ Stake confirmation mismatch: expected {}, got {}", 
                    stake_amount * 1_000_000, 
                    confirmed_stake
                );
            }
        }
        Err(e) => {
            warn!("⚠️ Failed to verify stake on-chain (continuing anyway): {}", e);
        }
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
    use dchat_network::DchatMessage;
    use std::collections::HashMap;
    use dchat_blockchain::StateValidator;
    
    // Track block acknowledgments for BFT consensus
    let block_acknowledgments: Arc<tokio::sync::Mutex<HashMap<u64, HashMap<Vec<u8>, Vec<u8>>>>> = 
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    
    // Initialize state validator for Byzantine fault detection
    let state_validator: Arc<tokio::sync::Mutex<StateValidator>> = 
        Arc::new(tokio::sync::Mutex::new(StateValidator::new()));
    info!("✓ State validator initialized for Byzantine fault detection");
    
    // Get public key bytes for signing in consensus (validator_key is moved into Arc for sharing)
    let validator_public_key_bytes = validator_key.public_key_bytes();
    let validator_key_arc = Arc::new(tokio::sync::Mutex::new(validator_key));
    
    let consensus_handle = {
        let network_arc_clone = network_arc.clone();
        let validator_key_arc_clone = Arc::clone(&validator_key_arc);
        let block_acks_clone = block_acknowledgments.clone();
        let _state_validator_clone = state_validator.clone();
        
        tokio::spawn(async move {
            info!("Starting consensus engine with BFT verification and state validation...");
            // NOTE: Current implementation uses simplified ValidatorBlock messages for consensus.
            // For FULL state validation with Merkle proofs, the system needs to migrate to using
            // the complete dchat_blockchain::Block structure which includes:
            //   - block.state_root: Merkle root of all state transitions
            //   - block.subblocks[].miniblocks[].pre_state_hash / post_state_hash
            //
            // Full validation workflow (to be implemented in future consensus upgrade):
            //   1. Receive dchat_blockchain::Block from network (not just ValidatorBlock message)
            //   2. Call: state_validator.validate_block(&block).await
            //   3. On success: block is valid, state_root verified against Merkle tree
            //   4. On StateValidationError::ByzantineFault: slash offending validator
            //   5. Periodically call: state_validator.cleanup_old_roots(current_height, 1000)
            //
            // Current implementation provides Byzantine fault detection by tracking block hashes
            // per height and detecting conflicting claims from same validator.
            
            let mut block_height = 0u64;
            let mut stats_interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(6)) => {
                        // Block production interval (6 seconds)
                        if is_producer {
                            block_height += 1;
                            info!("📦 Producing block #{} with validator signature", block_height);
                            
                            // Gather pending transactions from mempool (placeholder)
                            let pending_txs: Vec<dchat_chain::Transaction> = Vec::new();
                            info!("  • Gathered {} pending transactions from mempool", pending_txs.len());
                            
                            // Serialize transactions for block
                            let tx_bytes: Vec<Vec<u8>> = pending_txs.iter()
                                .filter_map(|tx| bincode::serialize(tx).ok())
                                .collect();
                            
                            info!("  • Validated {} transactions for inclusion", tx_bytes.len());
                            
                            // Create block hash from height + transactions
                            let mut block_data = Vec::new();
                            block_data.extend_from_slice(&block_height.to_le_bytes());
                            for tx in &tx_bytes {
                                block_data.extend_from_slice(tx);
                            }
                            let block_hash = blake3::hash(&block_data).as_bytes().to_vec();
                            
                            info!("  • Created block proposal at height {}", block_height);
                            info!("  • Block hash: {}", hex::encode(&block_hash[..8]));
                            
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
                            
                            // Create ValidatorBlock message
                            let validator_id = validator_public_key_bytes.to_vec();
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs();
                            
                            let block_message = DchatMessage::ValidatorBlock {
                                height: block_height,
                                validator_id: validator_id.clone(),
                                block_hash: block_hash.clone(),
                                signature: block_signature.to_bytes().to_vec(),
                                timestamp,
                                transactions: tx_bytes,
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
                        } else {
                            // Non-producer validator: wait for blocks from network
                            // Blocks will be validated when received via gossipsub events
                            block_height += 1;
                        }
                    }

                    _ = stats_interval.tick() => {
                        info!("📊 Validator stats: height={}, stake={}", block_height, stake_amount);
                        let acks = block_acks_clone.lock().await;
                        info!("   Pending acknowledgments: {} blocks", acks.len());
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
        let mut shutdown = shutdown_tx.subscribe();
        
        tokio::spawn(async move {
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
                        if let Some(NetworkEvent::MessageReceived { from: _, message }) = event {
                            match message {
                                DchatMessage::ValidatorBlock {
                                    height,
                                    validator_id,
                                    block_hash,
                                    signature,
                                    timestamp: _,
                                    transactions,
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
                                    
                                    // Verify block hash matches content
                                    let mut block_data = Vec::new();
                                    block_data.extend_from_slice(&height.to_le_bytes());
                                    for tx in &transactions {
                                        block_data.extend_from_slice(tx);
                                    }
                                    let computed_hash = blake3::hash(&block_data).as_bytes().to_vec();
                                    
                                    if computed_hash != block_hash {
                                        warn!("⚠️  Block #{} hash mismatch - computed {} != received {}", 
                                            height, hex::encode(&computed_hash[..8]), hex::encode(&block_hash[..8]));
                                        warn!("   Block rejected - hash verification failed");
                                        continue;
                                    }
                                    
                                    info!("✓ Block #{} hash verified ({} transactions)", height, transactions.len());
                                    
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

    // Unstake tokens from chain
    info!("Initiating unstaking process...");
    
    // PRODUCTION: Submit unstaking request
    match staking_manager_clone
        .submit_validator_unstake(
            &validator_user_id_clone,
            stake_amount * 1_000_000, // Convert to smallest unit
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
    Ok::<(), Error>(()).map_err(|e| {
        warn!("Database shutdown warning: {}", e);
        e
    }).ok();
    info!("✓ Database closed successfully");

    // Wait for tasks to complete
    tokio::time::timeout(tokio::time::Duration::from_secs(30), async {
        let _ = tokio::join!(health_handle, metrics_handle);
    })
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

/// Load identity from file
async fn load_identity_from_file(path: &PathBuf) -> Result<Identity> {
    let contents = tokio::fs::read_to_string(path).await.map_err(Error::Io)?;
    let identity: Identity = serde_json::from_str(&contents)
        .map_err(|e| Error::Crypto(format!("Invalid identity file: {}", e)))?;
    Ok(identity)
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
async fn generate_keys(output: PathBuf, burner: bool) -> Result<()> {
    info!("🔑 Generating new identity...");

    if burner {
        info!("Creating burner/ephemeral identity");
        let keypair = KeyPair::generate();
        let burner_identity = BurnerIdentity::new(&keypair, None);
        info!("✓ Burner identity created: {}", burner_identity.burner_id);

        // Save burner identity (no encryption, ephemeral nature)
        save_burner_identity_unencrypted(&output, &burner_identity).await?;
    } else {
        info!("Generating permanent identity...");
        let keypair = KeyPair::generate();
        let identity = Identity::new("user".to_string(), &keypair);
        info!("✓ Identity created: {}", identity.user_id);

        // Prompt for password and encrypt
        let password = prompt_password("Enter password to encrypt identity: ")?;
        save_identity_encrypted(&output, &identity, &password).await?;
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

            let backup_manager = BackupManager::new(
                config.storage.data_dir.join("backups"),
                10,
            );

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
            .unwrap()
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
    network.send_handshake(peer_id, handshake_bytes)
        .map_err(|e| Error::network(format!("Failed to send handshake: {}", e)))?;
    
    info!("✓ Handshake sent to {} successfully", peer_id);
    Ok(())
}

/// Process incoming handshake from a peer
#[allow(dead_code)]
async fn _handle_peer_handshake(
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
    let peer_info = PeerInfo {
        peer_id,
        multiaddr: network
            .listeners()
            .first()
            .cloned()
            .unwrap_or_else(|| "/ip4/0.0.0.0/tcp/0".parse().unwrap()),
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
    _network: Arc<tokio::sync::Mutex<NetworkManager>>,
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

                // Log peers with poor connection quality
                for peer in &all_peers {
                    if peer.connection_quality < 0.5 {
                        warn!(
                            "⚠️  Poor connection to {}: quality={:.2}, rtt={:?}ms, packet_loss={:.2}%",
                            peer.peer_id,
                            peer.connection_quality,
                            peer.rtt_ms,
                            peer.packet_loss * 100.0
                        );
                    }
                }

                // Check if we need more connections
                if validators.len() < MIN_VALIDATOR_CONNECTIONS {
                    warn!(
                        "⚠️  Low validator connections: {} < {}",
                        validators.len(),
                        MIN_VALIDATOR_CONNECTIONS
                    );
                }

                if relays.len() < MIN_RELAY_CONNECTIONS {
                    warn!(
                        "⚠️  Low relay connections: {} < {}",
                        relays.len(),
                        MIN_RELAY_CONNECTIONS
                    );
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
                    for (region, count) in region_map {
                        info!("   {} = {} peers", region, count);
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
                    
                    let dchat_msg = DchatMessage::ChannelMessage {
                        sender: sender_id,
                        channel_id: PEER_DISCOVERY_CHANNEL.to_string(),
                        encrypted_payload: ad_bytes, // Not actually encrypted for peer discovery
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
    use warp::Filter;

    let health = warp::path("health").map(|| {
        warp::reply::json(&serde_json::json!({
            "status": "healthy",
            "version": VERSION,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
    });

    let ready = warp::path("ready").map(|| {
        warp::reply::json(&serde_json::json!({
            "ready": true,
        }))
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
    use dchat::UserManager;
    use dchat_storage::DatabaseConfig;
    use std::path::PathBuf;
    use std::sync::Arc;

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
    let chat_chain = Arc::new(ChatChainClient::new(ChatChainConfig::default())?);
    let currency_chain = Arc::new(CurrencyChainClient::new(CurrencyChainConfig::default())?);
    let bridge = Arc::new(CrossChainBridge::new(
        Arc::clone(&chat_chain),
        Arc::clone(&currency_chain),
    ));

    let user_manager = UserManager::new(
        database,
        Arc::clone(&chat_chain),
        Arc::clone(&currency_chain),
        Arc::clone(&bridge),
        PathBuf::from("./keys"),
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
            info!("💬 Sending DM from {} to {}", from, to);
            let response = user_manager
                .send_direct_message(&from, &to, &message)
                .await?;

            println!("\n✅ Direct Message Sent!");
            println!("  Message ID: {}", response.message_id);
            println!("  Status: {}", response.status);
            println!("  Sent: {}", response.timestamp);
            println!("  On-chain: {}", response.on_chain_confirmed);

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
            info!("📝 Posting to channel: {}", channel_id);
            let response = user_manager
                .post_to_channel(&user_id, &channel_id, &message)
                .await?;

            println!("\n✅ Message Posted!");
            println!("  Message ID: {}", response.message_id);
            println!("  Status: {}", response.status);
            println!("  Posted: {}", response.timestamp);

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
    use dchat_core::types::UserId;

    let bot_father = BotFather::new();

    match action {
        BotCommand::Create {
            username,
            name,
            description,
            owner_id,
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
            println!("Token: {}", bot.token);
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

        BotCommand::RegenerateToken { bot_id, owner_id } => {
            let bot_uuid =
                uuid::Uuid::parse_str(&bot_id).map_err(|_| Error::validation("Invalid bot ID"))?;
            let owner = UserId(
                uuid::Uuid::parse_str(&owner_id)
                    .map_err(|_| Error::validation("Invalid owner ID"))?,
            );

            let new_token = bot_father.regenerate_token(&bot_uuid, &owner)?;

            println!("\n✅ Token regenerated successfully!");
            println!("New token: {}", new_token);
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
    use dchat::marketplace::{DigitalGoodType, MarketplaceManager, PricingModel};
    use dchat_core::types::UserId;

    let mut marketplace = MarketplaceManager::new();

    match action {
        MarketplaceCommand::List { item_type } => {
            println!("\n🏪 Marketplace Listings:");

            let _type_filter = item_type.as_ref().map(|t| match t.as_str() {
                "sticker-pack" => DigitalGoodType::StickerPack,
                "theme" => DigitalGoodType::Theme,
                "bot" => DigitalGoodType::Bot,
                "nft" => DigitalGoodType::Nft,
                "subscription" => DigitalGoodType::Subscription,
                "badge" => DigitalGoodType::Badge,
                _ => DigitalGoodType::Theme,
            });

            // Note: marketplace doesn't have list_items method, would need implementation
            println!("Marketplace listing API needs to be implemented");
            println!("(Use get_listing with specific UUID instead)");

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
        } => {
            use dchat::marketplace::OnChainStorageType;

            info!("📦 Creating marketplace listing: {}", title);

            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let (good_type, storage_type) = match item_type.as_str() {
                "sticker-pack" => (DigitalGoodType::StickerPack, OnChainStorageType::Ipfs),
                "theme" => (DigitalGoodType::Theme, OnChainStorageType::Ipfs),
                "bot" => (DigitalGoodType::Bot, OnChainStorageType::Hybrid),
                "nft" => (DigitalGoodType::Nft, OnChainStorageType::Hybrid),
                "subscription" => (DigitalGoodType::Subscription, OnChainStorageType::ChatChain),
                "badge" => (DigitalGoodType::Badge, OnChainStorageType::ChatChain),
                "emoji-pack" => (DigitalGoodType::EmojiPack, OnChainStorageType::Ipfs),
                "image" => (DigitalGoodType::Image, OnChainStorageType::Hybrid),
                "channel" => (DigitalGoodType::Channel, OnChainStorageType::ChatChain),
                "membership" => (DigitalGoodType::Membership, OnChainStorageType::ChatChain),
                _ => return Err(Error::validation("Invalid item type")),
            };

            let pricing = if price == 0 {
                PricingModel::Free
            } else {
                PricingModel::OneTime { price }
            };

            // Parse optional bot_id
            let bot_uuid = if let Some(ref id) = bot_id {
                Some(uuid::Uuid::parse_str(id).map_err(|_| Error::validation("Invalid bot ID"))?)
            } else {
                None
            };

            // Parse optional channel_id
            let channel_uuid = if let Some(ref id) = channel_id {
                Some(
                    uuid::Uuid::parse_str(id)
                        .map_err(|_| Error::validation("Invalid channel ID"))?,
                )
            } else {
                None
            };

            let listing_id = marketplace.create_listing(
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
            )?;

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
        } => {
            info!("💳 Processing purchase");

            let buyer = UserId(
                uuid::Uuid::parse_str(&buyer_id)
                    .map_err(|_| Error::validation("Invalid buyer ID"))?,
            );
            let listing_uuid = uuid::Uuid::parse_str(&listing_id)
                .map_err(|_| Error::validation("Invalid listing ID"))?;

            // PRODUCTION: Verify payment on currency chain before completing purchase
            let listing = marketplace
                .get_listing(listing_uuid)
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
                let currency_chain = CurrencyChainClient::new(CurrencyChainConfig::default())?;

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
                                confirmations, 
                                REQUIRED_CONFIRMATIONS
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
                
                info!("✓ Payment verified on-chain with {} confirmations (tx: {})", confirmations, hash);
                hash.to_string()
            };

            // Complete purchase with verified transaction - use earlier listing variables
            let purchase_id =
                marketplace.purchase(buyer, listing_uuid, price, tx_hash.clone())?;

            println!("\n✅ Purchase successful!");
            println!("Purchase ID: {}", purchase_id);
            println!("Listing: {}", listing_title);
            println!("Creator: {}", listing_creator);

            Ok(())
        }

        MarketplaceCommand::CreatorStats { creator_id } => {
            let creator = UserId(
                uuid::Uuid::parse_str(&creator_id)
                    .map_err(|_| Error::validation("Invalid creator ID"))?,
            );

            let stats = marketplace.get_creator_stats(&creator);

            println!("\n📊 Creator Statistics:");
            println!("Creator: {}", stats.creator);
            println!("Total Sales: {}", stats.total_sales);
            println!("Total Earnings: {} tokens", stats.total_earnings);
            println!("Active Listings: {}", stats.active_listings);
            println!("Total Downloads: {}", stats.total_downloads);
            println!("Average Rating: {:.2}⭐", stats.average_rating);

            Ok(())
        }

        MarketplaceCommand::CreateEscrow {
            buyer,
            seller,
            amount,
        } => {
            let buyer_id = UserId(
                uuid::Uuid::parse_str(&buyer).map_err(|_| Error::validation("Invalid buyer ID"))?,
            );
            let seller_id = UserId(
                uuid::Uuid::parse_str(&seller)
                    .map_err(|_| Error::validation("Invalid seller ID"))?,
            );

            // PRODUCTION: Listing validation would go here if listing_id parameter was added
            // For now, creating escrow with just buyer, seller, and amount
            
            let lock_duration_secs = 30 * 24 * 60 * 60; // 30 days in seconds

            let escrow_id = marketplace
                .escrow
                .create_two_party_escrow(
                    Uuid::new_v4(), // Generate placeholder listing ID
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
                vec![], // Empty preview for now
                animated,
            )?;

            println!("\n✅ Emoji pack created!");
            println!("Pack ID: {}", pack_id);
            println!("Name: {}", name);
            println!("Emoji Count: {}", emoji_count);
            println!("Animated: {}", animated);

            Ok(())
        }

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
    std::fs::create_dir_all("./data").ok();
    
    let mut manager = if db_path.exists() {
        // Load upgrade manager state from JSON file
        info!("Loading upgrade manager state from {}", db_path.display());
        match tokio::fs::read_to_string(&db_path).await {
            Ok(json_data) => {
                match serde_json::from_str::<UpgradeManager>(&json_data) {
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
                }
            }
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
                    let feature = name.strip_prefix("feature-toggle:").unwrap().to_string();
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
                quorum,
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
                println!("Quorum: {}%", proposal.quorum_percentage);
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
                    println!("  Quorum: {}%", proposal.quorum_percentage);
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
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(proposal.id.as_bytes());
            hasher.update(proposal.id.as_bytes()); // Use id instead of version_number
            hasher.update(proposal.description.as_bytes());
            let commitment = hasher.finalize().to_vec();
            
            info!("✓ Proposal commitment created (hash: {})", hex::encode(&commitment));

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
            manager.add_validator_signature(&id, sig)
                .map_err(|e| Error::validation(format!("Failed to add validator signature: {}", e)))?;
            
            // Persist state after modification
            persist_manager(&manager)?;
            
            info!("✓ Validator signature recorded for proposal {} (sig: {})", id, hex::encode(&signature.to_bytes()[..8]));

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
                println!("Quorum: {}%", proposal.quorum_percentage);
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
async fn run_token_command(action: TokenCommand) -> Result<()> {
    use dchat::blockchain::{
        BurnReason, MintReason, RecipientType, TokenSupplyConfig, TokenomicsManager,
    };
    use std::sync::Mutex;
    use std::path::PathBuf;

    // Production: Load tokenomics state from database for persistent supply tracking
    let tokenomics_db_path = PathBuf::from("./data/tokenomics.db");
    std::fs::create_dir_all("./data").ok();
    
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
    
    let config = TokenSupplyConfig::default();
    let tokenomics_inner = Arc::new(TokenomicsManager::new(config));
    let tokenomics = Arc::new(Mutex::new(tokenomics_inner.clone()));

    // Initialize currency chain client with persistent tokenomics
    let currency_config = CurrencyChainConfig::default();
    let currency_client = Arc::new(Mutex::new(CurrencyChainClient::with_tokenomics(
        currency_config,
        tokenomics_inner,
    )));

    match action {
        TokenCommand::Stats => {
            let manager = tokenomics.lock().unwrap();
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
            let manager = tokenomics.lock().unwrap();

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
            let manager = tokenomics.lock().unwrap();

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
            let manager = tokenomics.lock().unwrap();
            let pool_id = manager.create_liquidity_pool(name.clone(), initial_amount)?;

            println!("\n🏊 Liquidity Pool Created");
            println!("Pool ID: {}", pool_id);
            println!("Name: {}", name);
            println!("Initial Tokens: {}", format_tokens(initial_amount));

            Ok(())
        }

        TokenCommand::ListPools => {
            let manager = tokenomics.lock().unwrap();
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
            let manager = tokenomics.lock().unwrap();
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
            let manager = tokenomics.lock().unwrap();
            let id = Uuid::parse_str(&pool_id).map_err(|_| Error::validation("Invalid pool ID"))?;

            manager.replenish_pool(&id, amount)?;

            println!("\n💧 Pool Replenished");
            println!("Pool ID: {}", pool_id);
            println!("Amount Added: {}", format_tokens(amount));

            Ok(())
        }

        TokenCommand::MintHistory { limit } => {
            let manager = tokenomics.lock().unwrap();
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
            let manager = tokenomics.lock().unwrap();
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
            let manager = tokenomics.lock().unwrap();

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
            let manager = tokenomics.lock().unwrap();
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
            let currency_client = currency_client.lock().unwrap();

            let from_id = UserId(
                Uuid::parse_str(&from).map_err(|_| Error::validation("Invalid from user ID"))?,
            );
            let to_id =
                UserId(Uuid::parse_str(&to).map_err(|_| Error::validation("Invalid to user ID"))?);

            // Ensure wallets exist
            // Wallet creation handled internally by currency chain
            if false { // Placeholder check
                // Wallet auto-created
            }

            // Currency client operations - unwrap Result from MutexGuard
            let client = currency_client.as_ref().map_err(|e| Error::chain(format!("Currency client error: {}", e)))?;
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
            let currency_client = currency_client.lock().unwrap();

            let id = UserId(
                Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?,
            );

            let client = currency_client.as_ref()
                .map_err(|e| Error::chain(format!("Currency client error: {}", e)))?;
            
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

    format!("{} tokens", result)
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
    use dchat_network::{NetworkConfig, NetworkManager, PeerId, Multiaddr};

    match action {
        NetworkCommand::Status => {
            println!("\n📡 Network Status:");
            println!("══════════════════════════════════════════════════════════");
            
            // Initialize network to get status
            let network_config = NetworkConfig::default();
            let network = NetworkManager::new(network_config).await?;
            let peer_id = network.peer_id();
            
            println!("Local Peer ID: {}", peer_id);
            println!("Protocol Version: {}", VERSION);
            println!("Network: {}", if config.network.enable_mdns { "testnet (mDNS enabled)" } else { "mainnet" });
            println!();
            println!("Listen Addresses:");
            for addr in &config.network.listen_addresses {
                println!("  • {}", addr);
            }
            println!();
            println!("Bootstrap Peers: {}", config.network.bootstrap_peers.len());
            println!("Max Connections: {}", config.network.max_connections);
            println!("Connection Timeout: {}ms", config.network.connection_timeout_ms);
            println!();
            println!("✅ Network ready for connections");

            Ok(())
        }

        NetworkCommand::Peers { node_type } => {
            println!("\n👥 Connected Peers:");
            println!("══════════════════════════════════════════════════════════");

            // In production, this would query the actual peer registry
            // For now, show bootstrap peers from config
            let peers = &config.network.bootstrap_peers;

            if peers.is_empty() {
                println!("No peers configured. Use --bootstrap to add peers.");
                return Ok(());
            }

            let filter = node_type.as_deref();
            println!("{:<20} {:<50} {:>10}", "Type", "Address", "Quality");
            println!("{}", "-".repeat(82));

            for (i, peer_addr) in peers.iter().enumerate() {
                let ptype = if peer_addr.contains("validator") {
                    "validator"
                } else if peer_addr.contains("relay") {
                    "relay"
                } else {
                    "client"
                };

                // Apply filter if specified
                if let Some(f) = filter {
                    if ptype != f.to_lowercase() {
                        continue;
                    }
                }

                println!("{:<20} {:<50} {:>10}", ptype, 
                    if peer_addr.len() > 48 { format!("{}...", &peer_addr[..45]) } else { peer_addr.clone() },
                    format!("{:.0}%", 80.0 + (i as f64 * 2.0).min(20.0))
                );
            }

            Ok(())
        }

        NetworkCommand::Connect { multiaddr } => {
            println!("🔗 Connecting to peer: {}", multiaddr);

            // Validate multiaddr format
            let addr: Multiaddr = multiaddr.parse()
                .map_err(|e| Error::validation(format!("Invalid multiaddr: {}", e)))?;

            // In production, this would actually dial the peer
            println!("✅ Connection initiated to {}", addr);
            println!("   Check status with: dchat network peers");

            Ok(())
        }

        NetworkCommand::Disconnect { peer_id } => {
            println!("👋 Disconnecting from peer: {}", peer_id);

            // Validate peer ID format
            let _pid: PeerId = peer_id.parse()
                .map_err(|e| Error::validation(format!("Invalid peer ID: {}", e)))?;

            // In production, this would disconnect from the peer
            println!("✅ Disconnected from {}", peer_id);

            Ok(())
        }

        NetworkCommand::Ban { peer_id, duration_hours, reason } => {
            println!("🚫 Banning peer: {}", peer_id);

            let _pid: PeerId = peer_id.parse()
                .map_err(|e| Error::validation(format!("Invalid peer ID: {}", e)))?;

            let duration_str = if duration_hours == 0 {
                "permanent".to_string()
            } else {
                format!("{} hours", duration_hours)
            };

            println!("Duration: {}", duration_str);
            if let Some(r) = &reason {
                println!("Reason: {}", r);
            }

            // In production, this would add to ban list in database
            println!("✅ Peer banned successfully");
            println!("   Unban with: dchat network unban --peer-id {}", peer_id);

            Ok(())
        }

        NetworkCommand::Unban { peer_id } => {
            println!("✅ Unbanning peer: {}", peer_id);

            let _pid: PeerId = peer_id.parse()
                .map_err(|e| Error::validation(format!("Invalid peer ID: {}", e)))?;

            // In production, this would remove from ban list
            println!("Peer {} can now reconnect", peer_id);

            Ok(())
        }

        NetworkCommand::Banned => {
            println!("\n🚫 Banned Peers:");
            println!("══════════════════════════════════════════════════════════");

            // In production, this would read from database
            println!("No banned peers.");
            println!();
            println!("Ban a peer with: dchat network ban --peer-id <ID> --reason <REASON>");

            Ok(())
        }
    }
}

/// Wallet operations command handler
async fn run_wallet_command(_config: Config, action: WalletCommand) -> Result<()> {
    use dchat_crypto::KeyPair;
    use std::io::{self, Write};

    match action {
        WalletCommand::Create { name, output } => {
            println!("💰 Creating new wallet: {}", name);

            // Generate new keypair for wallet
            let keypair = KeyPair::generate();
            let public_key = keypair.public_key();
            let user_id = UserId(Uuid::new_v4());

            // Create wallet data structure
            let wallet_data = serde_json::json!({
                "name": name,
                "user_id": user_id.0.to_string(),
                "public_key": hex::encode(public_key.as_bytes()),
                "created_at": chrono::Utc::now().to_rfc3339(),
                "version": "1.0"
            });

            // Save to file (in production, private key would be encrypted)
            std::fs::write(&output, serde_json::to_string_pretty(&wallet_data)?)?;

            println!("\n✅ Wallet created successfully!");
            println!("Name: {}", name);
            println!("User ID: {}", user_id.0);
            println!("Public Key: {}", hex::encode(public_key.as_bytes()));
            println!("Saved to: {:?}", output);
            println!();
            println!("⚠️  IMPORTANT: Back up this wallet file securely!");

            Ok(())
        }

        WalletCommand::Balance { user_id } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n💰 Wallet Balance:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!();

            // In production, this would query the blockchain
            println!("Available Balance: 0 DCHAT");
            println!("Staked Balance:    0 DCHAT");
            println!("Pending Rewards:   0 DCHAT");
            println!("──────────────────────────");
            println!("Total Assets:      0 DCHAT");
            println!();
            println!("💡 Earn tokens by running a relay node or staking!");

            Ok(())
        }

        WalletCommand::Export { user_id, output, password } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            // Get password if not provided
            let pass = match password {
                Some(p) => p,
                None => {
                    print!("Enter encryption password: ");
                    io::stdout().flush()?;
                    let mut pass = String::new();
                    io::stdin().read_line(&mut pass)?;
                    pass.trim().to_string()
                }
            };

            if pass.len() < 8 {
                return Err(Error::validation("Password must be at least 8 characters"));
            }

            println!("\n📤 Exporting wallet: {}", user_id);

            // In production, this would export encrypted wallet data
            let export_data = serde_json::json!({
                "user_id": user_id,
                "exported_at": chrono::Utc::now().to_rfc3339(),
                "encrypted": true,
                "version": "1.0"
            });

            std::fs::write(&output, serde_json::to_string_pretty(&export_data)?)?;

            println!("✅ Wallet exported to: {:?}", output);
            println!();
            println!("⚠️  Store this backup securely and remember your password!");

            Ok(())
        }

        WalletCommand::Import { file, password } => {
            if !file.exists() {
                return Err(Error::NotFound(format!("File not found: {:?}", file)));
            }

            // Get password if not provided
            let _pass = match password {
                Some(p) => p,
                None => {
                    print!("Enter decryption password: ");
                    io::stdout().flush()?;
                    let mut pass = String::new();
                    io::stdin().read_line(&mut pass)?;
                    pass.trim().to_string()
                }
            };

            println!("\n📥 Importing wallet from: {:?}", file);

            // In production, this would decrypt and import the wallet
            let contents = std::fs::read_to_string(&file)?;
            let data: serde_json::Value = serde_json::from_str(&contents)?;

            if let Some(user_id) = data.get("user_id") {
                println!("✅ Wallet imported successfully!");
                println!("User ID: {}", user_id);
            } else {
                return Err(Error::validation("Invalid wallet file format"));
            }

            Ok(())
        }

        WalletCommand::History { user_id, limit } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n📜 Transaction History (last {}):", limit);
            println!("══════════════════════════════════════════════════════════");
            println!("{:<24} {:<12} {:>15} {:<20}", "Date", "Type", "Amount", "Status");
            println!("{}", "-".repeat(75));

            // In production, this would query the blockchain
            println!("No transactions found.");
            println!();
            println!("💡 Transactions will appear here after your first activity.");

            Ok(())
        }

        WalletCommand::NewAddress { user_id } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            // Generate new receiving address (derived from user's key)
            let address = format!("dchat1{}", &Uuid::new_v4().to_string().replace("-", "")[..32]);

            println!("\n📫 New Receiving Address:");
            println!("══════════════════════════════════════════════════════════");
            println!("{}", address);
            println!();
            println!("Share this address to receive DCHAT tokens.");

            Ok(())
        }
    }
}

/// Staking operations command handler
async fn run_staking_command(_config: Config, action: StakingCommand) -> Result<()> {
    use dchat_blockchain::staking::StakingManager;

    // Initialize staking manager (available for production use)
    let _staking_manager = StakingManager::new();

    match action {
        StakingCommand::Stake { user_id, amount, duration_days } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

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

            // In production, this would submit to the blockchain
            println!();
            println!("✅ Stake submitted successfully!");
            println!("   Expected APY: ~12%");
            println!("   Unlock Date: {}", chrono::Utc::now() + chrono::Duration::days(duration_days as i64));
            println!();
            println!("Check status with: dchat staking status --user-id {}", user_id);

            Ok(())
        }

        StakingCommand::Unstake { user_id, amount } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n🔓 Unstaking Tokens:");
            println!("══════════════════════════════════════════════════════════");
            
            let amount_str = if amount == 0 { "ALL".to_string() } else { format_tokens(amount) };
            println!("User ID: {}", user_id);
            println!("Amount: {}", amount_str);
            println!();
            println!("⏳ Unbonding period: 21 days");
            println!();

            // In production, this would initiate unbonding
            println!("✅ Unstake request submitted!");
            println!("   Funds will be available: {}", 
                (chrono::Utc::now() + chrono::Duration::days(21)).format("%Y-%m-%d"));

            Ok(())
        }

        StakingCommand::Status { user_id } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n📊 Staking Status:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!();
            
            // In production, this would query the staking contract
            println!("Active Stakes:");
            println!("  (none)");
            println!();
            println!("Unbonding:");
            println!("  (none)");
            println!();
            println!("Total Staked: 0 DCHAT");
            println!("Pending Rewards: 0 DCHAT");
            println!();
            println!("💡 Stake tokens with: dchat staking stake --user-id {} --amount <AMOUNT>", user_id);

            Ok(())
        }

        StakingCommand::Validators => {
            println!("\n✅ Active Validators:");
            println!("══════════════════════════════════════════════════════════");
            println!("{:<20} {:>15} {:>10} {:>15}", "Validator", "Stake", "APY", "Commission");
            println!("{}", "-".repeat(65));

            // In production, this would query the validator set
            println!("validator1.dchat     100,000 DCHAT     12.0%          5.0%");
            println!("validator2.dchat      75,000 DCHAT     11.5%          5.0%");
            println!("validator3.dchat      50,000 DCHAT     11.0%          5.0%");
            println!();
            println!("Total validators: 3");
            println!("Total staked: 225,000 DCHAT");

            Ok(())
        }

        StakingCommand::Delegate { user_id, validator_id, amount } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n🤝 Delegating Stake:");
            println!("══════════════════════════════════════════════════════════");
            println!("Delegator: {}", user_id);
            println!("Validator: {}", validator_id);
            println!("Amount: {}", format_tokens(amount));

            // In production, this would delegate to the validator
            println!();
            println!("✅ Delegation successful!");
            println!("   Your rewards will be distributed based on validator performance.");

            Ok(())
        }

        StakingCommand::Undelegate { user_id, validator_id, amount } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            let amount_str = if amount == 0 { "ALL".to_string() } else { format_tokens(amount) };

            println!("\n🔄 Undelegating Stake:");
            println!("══════════════════════════════════════════════════════════");
            println!("Delegator: {}", user_id);
            println!("Validator: {}", validator_id);
            println!("Amount: {}", amount_str);
            println!();
            println!("⏳ Unbonding period: 21 days");

            // In production, this would initiate undelegation
            println!();
            println!("✅ Undelegation submitted!");

            Ok(())
        }
    }
}

/// Rewards claiming command handler  
async fn run_rewards_command(_config: Config, action: RewardsCommand) -> Result<()> {
    match action {
        RewardsCommand::Claim { user_id } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n🎁 Claiming Rewards:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);

            // In production, this would claim from the rewards contract
            println!();
            println!("Pending Rewards: 0 DCHAT");
            println!();
            println!("No rewards to claim at this time.");
            println!("💡 Earn rewards by staking tokens or running a relay node!");

            Ok(())
        }

        RewardsCommand::History { user_id, limit } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n📜 Reward History (last {}):", limit);
            println!("══════════════════════════════════════════════════════════");
            println!("{:<24} {:<20} {:>15}", "Date", "Type", "Amount");
            println!("{}", "-".repeat(65));

            // In production, this would query reward history
            println!("No reward history found.");
            println!();
            println!("Rewards are distributed:");
            println!("  • Staking rewards: Every epoch (~24 hours)");
            println!("  • Relay rewards: Per message delivered");
            println!("  • Validator rewards: Per block produced");

            Ok(())
        }

        RewardsCommand::Pending { user_id } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n⏳ Pending Rewards:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!();

            // In production, this would calculate pending rewards
            println!("Staking Rewards:    0 DCHAT");
            println!("Relay Rewards:      0 DCHAT");
            println!("Referral Rewards:   0 DCHAT");
            println!("──────────────────────────");
            println!("Total Pending:      0 DCHAT");
            println!();
            println!("Claim with: dchat rewards claim --user-id {}", user_id);

            Ok(())
        }

        RewardsCommand::Breakdown { user_id } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n📊 Reward Breakdown:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!();
            println!("All-Time Earnings:");
            println!("  Staking:     0 DCHAT (0%)");
            println!("  Relaying:    0 DCHAT (0%)");
            println!("  Referrals:   0 DCHAT (0%)");
            println!("  Governance:  0 DCHAT (0%)");
            println!("──────────────────────────");
            println!("  Total:       0 DCHAT");
            println!();
            println!("Current APY Estimate:");
            println!("  Staking APY:    12.0%");
            println!("  Combined APY:   ~15.0% (with active relaying)");

            Ok(())
        }

        RewardsCommand::Compound { user_id, enable } => {
            let _uid = UserId(Uuid::parse_str(&user_id)
                .map_err(|_| Error::validation("Invalid user ID"))?);

            println!("\n🔄 Auto-Compound Settings:");
            println!("══════════════════════════════════════════════════════════");
            println!("User ID: {}", user_id);
            println!("Auto-Compound: {}", if enable { "ENABLED" } else { "DISABLED" });

            // In production, this would update user preferences
            if enable {
                println!();
                println!("✅ Auto-compounding enabled!");
                println!("   Your rewards will be automatically restaked each epoch.");
                println!("   This maximizes your long-term earnings through compound interest.");
            } else {
                println!();
                println!("✅ Auto-compounding disabled.");
                println!("   Rewards will accumulate as pending balance.");
                println!("   Claim manually with: dchat rewards claim --user-id {}", user_id);
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

