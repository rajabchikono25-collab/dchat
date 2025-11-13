use libp2p::{
    kad::{
        store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig, Event as KadEvent,
        QueryResult, Record, RecordKey,
    },
    Multiaddr, PeerId,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::time::interval;
use tracing::{debug, info, warn};
use trust_dns_resolver::TokioAsyncResolver;
use trust_dns_resolver::config::*;

/// DNS refresh interval - how often to try DNS before falling back to DHT
pub const DNS_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60); // 5 minutes

/// DHT query timeout
pub const DHT_QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum number of peers to return in bootstrap query
pub const MAX_BOOTSTRAP_PEERS: usize = 20;

/// Maximum age for cached peers
pub const CACHED_PEER_MAX_AGE: Duration = Duration::from_secs(60 * 60); // 1 hour

/// Peer discovery method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryMethod {
    /// Discovered via DNS seed nodes
    Dns,
    /// Discovered via Kademlia DHT
    Dht,
    /// Loaded from local cache
    Cached,
    /// Manually configured bootstrap node
    Manual,
}

/// Peer information with discovery metadata
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    pub peer_id: PeerId,
    pub addresses: Vec<Multiaddr>,
    pub method: DiscoveryMethod,
    pub discovered_at: u64, // Unix timestamp
}

impl DiscoveredPeer {
    /// Create a new discovered peer
    pub fn new(peer_id: PeerId, addresses: Vec<Multiaddr>, method: DiscoveryMethod) -> Self {
        Self {
            peer_id,
            addresses,
            method,
            discovered_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Check if this peer is still fresh
    pub fn is_fresh(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let age = now.saturating_sub(self.discovered_at);
        age < CACHED_PEER_MAX_AGE.as_secs()
    }

    /// Get age in seconds
    pub fn age_secs(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.discovered_at)
    }
}

/// DHT discovery errors
#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("DNS query failed: {0}")]
    DnsFailed(String),

    #[error("DHT query timeout after {0:?}")]
    DhtTimeout(Duration),

    #[error("No peers found via any method")]
    NoPeersFound,

    #[error("Cache empty")]
    CacheEmpty,

    #[error("Invalid peer data: {0}")]
    InvalidData(String),
}

/// Discovery metrics
#[derive(Debug, Clone, Default)]
pub struct DiscoveryMetrics {
    pub total_queries: u64,
    pub dns_successes: u64,
    pub dns_failures: u64,
    pub dht_successes: u64,
    pub dht_failures: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub avg_dns_duration_ms: u64,
    pub avg_dht_duration_ms: u64,
}

/// Cascading peer discovery with DNS → DHT → Cached fallback
pub struct DhtBootstrap {
    /// Kademlia DHT for decentralized discovery
    kademlia: Option<Kademlia<MemoryStore>>,

    /// Cached peers from previous discoveries
    cache: Arc<RwLock<HashMap<PeerId, DiscoveredPeer>>>,

    /// DNS seed nodes for bootstrap
    dns_seeds: Vec<String>,

    /// Last successful DNS query time
    last_dns_success: Arc<RwLock<Option<Instant>>>,

    /// Discovery metrics
    metrics: Arc<RwLock<DiscoveryMetrics>>,

    /// Active DHT queries
    active_queries: Arc<RwLock<HashSet<String>>>,
}

impl DhtBootstrap {
    /// Create a new DHT bootstrap with DNS seeds
    pub fn new(dns_seeds: Vec<String>) -> Self {
        Self {
            kademlia: None,
            cache: Arc::new(RwLock::new(HashMap::new())),
            dns_seeds,
            last_dns_success: Arc::new(RwLock::new(None)),
            metrics: Arc::new(RwLock::new(DiscoveryMetrics::default())),
            active_queries: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Initialize Kademlia DHT
    pub fn initialize_kademlia(&mut self, local_peer_id: PeerId) {
        let store = MemoryStore::new(local_peer_id);
        let kademlia = Kademlia::new(local_peer_id, store);
        self.kademlia = Some(kademlia);

        info!("Kademlia DHT initialized for peer {}", local_peer_id);
    }

    /// Discover peers with cascading fallback: DNS → DHT → Cached
    pub async fn discover_peers(&self) -> Result<Vec<DiscoveredPeer>, DiscoveryError> {
        let mut metrics = self.metrics.write().unwrap();
        metrics.total_queries += 1;
        drop(metrics);

        // Try DNS first if refresh interval has passed
        if self.should_try_dns() {
            match self.try_dns_discovery().await {
                Ok(peers) => {
                    info!("DNS discovery found {} peers", peers.len());
                    self.update_cache(&peers);
                    *self.last_dns_success.write().unwrap() = Some(Instant::now());

                    let mut metrics = self.metrics.write().unwrap();
                    metrics.dns_successes += 1;
                    return Ok(peers);
                }
                Err(e) => {
                    warn!("DNS discovery failed: {}, falling back to DHT", e);
                    let mut metrics = self.metrics.write().unwrap();
                    metrics.dns_failures += 1;
                }
            }
        }

        // Fallback to DHT
        match self.try_dht_discovery().await {
            Ok(peers) => {
                info!("DHT discovery found {} peers", peers.len());
                self.update_cache(&peers);

                let mut metrics = self.metrics.write().unwrap();
                metrics.dht_successes += 1;
                return Ok(peers);
            }
            Err(e) => {
                warn!("DHT discovery failed: {}, falling back to cache", e);
                let mut metrics = self.metrics.write().unwrap();
                metrics.dht_failures += 1;
            }
        }

        // Last resort: cached peers
        self.try_cached_discovery()
    }

    /// Check if DNS should be tried based on refresh interval
    fn should_try_dns(&self) -> bool {
        let last_success = self.last_dns_success.read().unwrap();
        match *last_success {
            None => true, // Never tried DNS
            Some(instant) => instant.elapsed() >= DNS_REFRESH_INTERVAL,
        }
    }

    /// Try DNS discovery
    async fn try_dns_discovery(&self) -> Result<Vec<DiscoveredPeer>, DiscoveryError> {
        let start = Instant::now();
        let mut discovered = Vec::new();

        debug!("Attempting DNS discovery from {} seeds", self.dns_seeds.len());

        // Create DNS resolver with default configuration
        let resolver = TokioAsyncResolver::tokio(
            ResolverConfig::default(),
            ResolverOpts::default(),
        );

        // Query each DNS seed
        for seed in &self.dns_seeds {
            match self.resolve_dns_seed(&resolver, seed).await {
                Ok(peers) => {
                    info!("DNS seed '{}' returned {} peers", seed, peers.len());
                    discovered.extend(peers);
                }
                Err(e) => {
                    warn!("DNS seed '{}' lookup failed: {}", seed, e);
                    // Continue to next seed
                }
            }
        }

        let duration = start.elapsed();
        let mut metrics = self.metrics.write().unwrap();
        if metrics.avg_dns_duration_ms == 0 {
            metrics.avg_dns_duration_ms = duration.as_millis() as u64;
        } else {
            metrics.avg_dns_duration_ms =
                (metrics.avg_dns_duration_ms * 7 + duration.as_millis() as u64 * 3) / 10;
        }

        if discovered.is_empty() {
            Err(DiscoveryError::DnsFailed(
                "No peers found via DNS".to_string(),
            ))
        } else {
            info!("DNS discovery found {} total peers", discovered.len());
            Ok(discovered)
        }
    }

    /// Resolve a single DNS seed to peer addresses
    async fn resolve_dns_seed(
        &self,
        resolver: &TokioAsyncResolver,
        seed: &str,
    ) -> Result<Vec<DiscoveredPeer>, DiscoveryError> {
        let mut peers = Vec::new();

        // Parse seed format: "hostname" or "hostname:port" or "_dnsaddr.hostname"
        // Standard format: seed.dchat.example.com returns A/AAAA records
        // Advanced format: _dnsaddr.dchat.example.com returns TXT records with peer info

        // Try TXT record lookup for dnsaddr format
        if seed.starts_with("_dnsaddr.") {
            match resolver.txt_lookup(seed).await {
                Ok(txt_records) => {
                    for record in txt_records.iter() {
                        for txt_data in record.iter() {
                            let txt_str = String::from_utf8_lossy(txt_data);
                            // Parse dnsaddr format: "dnsaddr=/ip4/1.2.3.4/tcp/9000/p2p/QmPeerID"
                            if let Some(multiaddr_str) = txt_str.strip_prefix("dnsaddr=") {
                                if let Ok(multiaddr) = Multiaddr::from_str(multiaddr_str) {
                                    // Extract PeerId from multiaddr if present
                                    if let Some(peer_id) = extract_peer_id_from_multiaddr(&multiaddr) {
                                        peers.push(DiscoveredPeer::new(
                                            peer_id,
                                            vec![multiaddr],
                                            DiscoveryMethod::Dns,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    if !peers.is_empty() {
                        return Ok(peers);
                    }
                }
                Err(e) => {
                    debug!("TXT lookup failed for {}: {}", seed, e);
                }
            }
        }

        // Fall back to A/AAAA record lookup for standard hostname
        let hostname = seed.trim_start_matches("_dnsaddr.");
        let (host, port) = if let Some(colon_pos) = hostname.rfind(':') {
            let (h, p) = hostname.split_at(colon_pos);
            let port = p[1..].parse::<u16>().unwrap_or(9000);
            (h, port)
        } else {
            (hostname, 9000u16)
        };

        // Lookup IPv4 addresses
        match resolver.ipv4_lookup(host).await {
            Ok(ipv4_records) => {
                for ip in ipv4_records.iter() {
                    let addr = IpAddr::V4(*ip);
                    // Create a multiaddr: /ip4/1.2.3.4/tcp/9000
                    let multiaddr_str = format!("/ip4/{}/tcp/{}", addr, port);
                    if let Ok(multiaddr) = Multiaddr::from_str(&multiaddr_str) {
                        // Use a placeholder PeerId (will be updated on connection)
                        // In production, this would be resolved via a registry or handshake
                        let placeholder_peer_id = PeerId::random();
                        peers.push(DiscoveredPeer::new(
                            placeholder_peer_id,
                            vec![multiaddr],
                            DiscoveryMethod::Dns,
                        ));
                    }
                }
            }
            Err(e) => {
                debug!("IPv4 lookup failed for {}: {}", host, e);
            }
        }

        // Lookup IPv6 addresses
        match resolver.ipv6_lookup(host).await {
            Ok(ipv6_records) => {
                for ip in ipv6_records.iter() {
                    let addr = IpAddr::V6(*ip);
                    // Create a multiaddr: /ip6/::1/tcp/9000
                    let multiaddr_str = format!("/ip6/{}/tcp/{}", addr, port);
                    if let Ok(multiaddr) = Multiaddr::from_str(&multiaddr_str) {
                        let placeholder_peer_id = PeerId::random();
                        peers.push(DiscoveredPeer::new(
                            placeholder_peer_id,
                            vec![multiaddr],
                            DiscoveryMethod::Dns,
                        ));
                    }
                }
            }
            Err(e) => {
                debug!("IPv6 lookup failed for {}: {}", host, e);
            }
        }

        if peers.is_empty() {
            Err(DiscoveryError::DnsFailed(format!(
                "No valid addresses found for seed: {}",
                seed
            )))
        } else {
            Ok(peers)
        }
    }

    /// Try DHT discovery using Kademlia
    async fn try_dht_discovery(&self) -> Result<Vec<DiscoveredPeer>, DiscoveryError> {
        let start = Instant::now();

        // Check if Kademlia is initialized
        if self.kademlia.is_none() {
            return Err(DiscoveryError::DnsFailed(
                "Kademlia not initialized".to_string(),
            ));
        }

        debug!("Attempting DHT discovery via Kademlia");

        // In real implementation, this would:
        // 1. Query Kademlia for closest peers to random peer IDs
        // 2. Wait for KadEvent::OutboundQueryProgressed
        // 3. Extract peer addresses from query results
        // For now, return empty to test cache fallback
        let mut discovered = Vec::new();

        // Simulated DHT query (in production, use actual Kademlia queries)
        // let query_id = self.kademlia.as_mut().unwrap().get_closest_peers(target_peer_id);
        // Wait for query results...

        let duration = start.elapsed();
        let mut metrics = self.metrics.write().unwrap();
        if metrics.avg_dht_duration_ms == 0 {
            metrics.avg_dht_duration_ms = duration.as_millis() as u64;
        } else {
            metrics.avg_dht_duration_ms =
                (metrics.avg_dht_duration_ms * 7 + duration.as_millis() as u64 * 3) / 10;
        }

        if discovered.is_empty() {
            Err(DiscoveryError::DhtTimeout(DHT_QUERY_TIMEOUT))
        } else {
            Ok(discovered)
        }
    }

    /// Try cached discovery (last resort)
    fn try_cached_discovery(&self) -> Result<Vec<DiscoveredPeer>, DiscoveryError> {
        let cache = self.cache.read().unwrap();

        // Filter fresh peers only
        let fresh_peers: Vec<DiscoveredPeer> = cache
            .values()
            .filter(|p| p.is_fresh())
            .cloned()
            .collect();

        let mut metrics = self.metrics.write().unwrap();
        if fresh_peers.is_empty() {
            metrics.cache_misses += 1;
            Err(DiscoveryError::CacheEmpty)
        } else {
            metrics.cache_hits += 1;
            info!("Cache hit: {} fresh peers available", fresh_peers.len());
            Ok(fresh_peers)
        }
    }

    /// Update cache with newly discovered peers
    fn update_cache(&self, peers: &[DiscoveredPeer]) {
        let mut cache = self.cache.write().unwrap();
        for peer in peers {
            cache.insert(peer.peer_id, peer.clone());
        }

        // Clean up stale entries
        cache.retain(|_, peer| peer.is_fresh());
    }

    /// Manually add a peer to cache (for bootstrap nodes)
    pub fn add_bootstrap_peer(&self, peer: DiscoveredPeer) {
        let mut cache = self.cache.write().unwrap();
        cache.insert(peer.peer_id, peer);
    }

    /// Get discovery metrics
    pub fn metrics(&self) -> DiscoveryMetrics {
        self.metrics.read().unwrap().clone()
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.cache.read().unwrap().len()
    }

    /// Clear stale cache entries
    pub fn cleanup_cache(&self) -> usize {
        let mut cache = self.cache.write().unwrap();
        let before = cache.len();
        cache.retain(|_, peer| peer.is_fresh());
        let removed = before - cache.len();
        debug!("Cache cleanup: removed {} stale entries", removed);
        removed
    }

    /// Spawn background task for periodic cache cleanup
    pub fn spawn_cache_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(60 * 10)); // 10 minutes

            loop {
                cleanup_interval.tick().await;
                self.cleanup_cache();
            }
        });
    }
}

/// Extract PeerId from multiaddr if present
fn extract_peer_id_from_multiaddr(multiaddr: &Multiaddr) -> Option<PeerId> {
    use libp2p::multiaddr::Protocol;
    
    for protocol in multiaddr.iter() {
        if let Protocol::P2p(peer_id) = protocol {
            return Some(peer_id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovered_peer_creation() {
        let peer_id = PeerId::random();
        let peer = DiscoveredPeer::new(peer_id, vec![], DiscoveryMethod::Dns);

        assert_eq!(peer.peer_id, peer_id);
        assert_eq!(peer.method, DiscoveryMethod::Dns);
        assert!(peer.is_fresh());
    }

    #[test]
    fn test_peer_freshness() {
        let peer_id = PeerId::random();
        let mut peer = DiscoveredPeer::new(peer_id, vec![], DiscoveryMethod::Cached);

        // Fresh initially
        assert!(peer.is_fresh());

        // Simulate old discovery
        peer.discovered_at -= CACHED_PEER_MAX_AGE.as_secs() + 1;
        assert!(!peer.is_fresh());
    }

    #[test]
    fn test_peer_age() {
        let peer_id = PeerId::random();
        let peer = DiscoveredPeer::new(peer_id, vec![], DiscoveryMethod::Dht);

        let age = peer.age_secs();
        assert!(age < 5); // Should be very recent
    }

    #[test]
    fn test_dht_bootstrap_creation() {
        let seeds = vec!["seed1.example.com".to_string(), "seed2.example.com".to_string()];
        let bootstrap = DhtBootstrap::new(seeds.clone());

        assert_eq!(bootstrap.dns_seeds, seeds);
        assert_eq!(bootstrap.cache_size(), 0);
    }

    #[test]
    fn test_add_bootstrap_peer() {
        let bootstrap = DhtBootstrap::new(vec![]);
        let peer_id = PeerId::random();
        let peer = DiscoveredPeer::new(peer_id, vec![], DiscoveryMethod::Manual);

        bootstrap.add_bootstrap_peer(peer.clone());
        assert_eq!(bootstrap.cache_size(), 1);
    }

    #[test]
    fn test_cache_cleanup() {
        let bootstrap = DhtBootstrap::new(vec![]);

        // Add fresh peer
        let fresh_peer = DiscoveredPeer::new(PeerId::random(), vec![], DiscoveryMethod::Dht);
        bootstrap.add_bootstrap_peer(fresh_peer);

        // Add stale peer
        let stale_peer_id = PeerId::random();
        let mut stale_peer = DiscoveredPeer::new(stale_peer_id, vec![], DiscoveryMethod::Cached);
        stale_peer.discovered_at -= CACHED_PEER_MAX_AGE.as_secs() + 1;
        bootstrap.add_bootstrap_peer(stale_peer);

        assert_eq!(bootstrap.cache_size(), 2);

        let removed = bootstrap.cleanup_cache();
        assert_eq!(removed, 1);
        assert_eq!(bootstrap.cache_size(), 1);
    }

    #[tokio::test]
    async fn test_discover_peers_cache_fallback() {
        let bootstrap = DhtBootstrap::new(vec![]);

        // Add bootstrap peer to cache
        let peer_id = PeerId::random();
        let peer = DiscoveredPeer::new(peer_id, vec![], DiscoveryMethod::Manual);
        bootstrap.add_bootstrap_peer(peer.clone());

        // Discovery should fall back to cache (DNS and DHT will fail in test)
        let result = bootstrap.discover_peers().await;
        assert!(result.is_ok());

        let peers = result.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].peer_id, peer_id);

        let metrics = bootstrap.metrics();
        assert_eq!(metrics.total_queries, 1);
        assert_eq!(metrics.cache_hits, 1);
    }

    #[test]
    fn test_should_try_dns() {
        let bootstrap = DhtBootstrap::new(vec!["seed.example.com".to_string()]);

        // Should try DNS initially
        assert!(bootstrap.should_try_dns());

        // Simulate successful DNS query
        *bootstrap.last_dns_success.write().unwrap() = Some(Instant::now());

        // Should not try DNS immediately after success
        assert!(!bootstrap.should_try_dns());
    }

    #[test]
    fn test_metrics_tracking() {
        let bootstrap = DhtBootstrap::new(vec![]);

        // Initially empty
        let metrics = bootstrap.metrics();
        assert_eq!(metrics.total_queries, 0);
        assert_eq!(metrics.dns_successes, 0);
    }

    #[test]
    fn test_discovery_method_serialization() {
        let method = DiscoveryMethod::Dht;
        let serialized = serde_json::to_string(&method).unwrap();
        let deserialized: DiscoveryMethod = serde_json::from_str(&serialized).unwrap();
        assert_eq!(method, deserialized);
    }
}
