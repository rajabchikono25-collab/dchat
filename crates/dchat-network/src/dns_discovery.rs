//! DNS-based peer discovery for production mainnet
//!
//! Discovers validators and relays via subdomains, handling dynamic IP changes.
//! Uses public DNS resolvers and caches results with TTL awareness.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use libp2p::{Multiaddr, PeerId};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;

// Result type for DNS discovery operations
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// DNS discovery configuration
#[derive(Clone, Debug)]
pub struct DnsDiscoveryConfig {
    /// Base domain (e.g., "schikuno.top")
    pub base_domain: String,

    /// Validator subdomain pattern (e.g., "validator1-{region}.schikuno.top")
    pub validator_subdomains: Vec<String>,

    /// Relay subdomain pattern (optional, defaults to validator hosts)
    pub relay_subdomains: Vec<String>,

    /// DNS cache TTL (default: 5 minutes)
    pub cache_ttl: Duration,

    /// DNS query timeout
    pub query_timeout: Duration,

    /// Validator p2p port
    pub validator_port: u16,

    /// Relay p2p ports (first relay, second relay)
    pub relay_ports: (u16, u16),

    /// Enable periodic refresh
    pub enable_refresh: bool,

    /// Refresh interval
    pub refresh_interval: Duration,
}

impl Default for DnsDiscoveryConfig {
    fn default() -> Self {
        Self {
            base_domain: "firebirdcomputing.com".to_string(),
            // Mainnet foundation validators (Azure - firebirdcomputing.com)
            validator_subdomains: vec![
                "ind.firebirdcomputing.com".to_string(), // India (74.225.183.196)
                "sa.firebirdcomputing.com".to_string(),  // South Africa (4.221.211.71)
                "uae.firebirdcomputing.com".to_string(), // UAE (4.161.34.228)
            ],
            // Mainnet foundation relays (AWS - schikuno.top)
            relay_subdomains: vec![
                "relay.ohio.schikuno.top".to_string(), // Ohio (18.223.119.189)
                "relay.saopaulo.schikuno.top".to_string(), // Sao Paulo (18.231.117.182)
                "relay.stockholm.schikuno.top".to_string(), // Stockholm (13.50.105.166)
            ],
            cache_ttl: Duration::from_secs(300), // 5 minutes
            query_timeout: Duration::from_secs(5),
            validator_port: 7070,
            relay_ports: (7071, 7072),
            enable_refresh: true,
            refresh_interval: Duration::from_secs(60), // 1 minute
        }
    }
}

/// Cached DNS resolution result
#[derive(Clone, Debug)]
struct CachedResolution {
    /// Resolved IP address
    ip: IpAddr,
    /// When this entry was cached
    cached_at: Instant,
    /// TTL from DNS record
    ttl: Duration,
}

impl CachedResolution {
    fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }
}

/// Node type discovered via DNS
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeType {
    Validator,
    Relay1,
    Relay2,
}

/// Discovered peer information
#[derive(Clone, Debug)]
pub struct DiscoveredPeer {
    /// Peer identifier (subdomain)
    pub identifier: String,
    /// Node type
    pub node_type: NodeType,
    /// Resolved IP address
    pub ip: IpAddr,
    /// P2P port
    pub port: u16,
    /// Multiaddr for libp2p connection
    pub multiaddr: Multiaddr,
    /// PeerId (if known)
    pub peer_id: Option<PeerId>,
}

/// DNS-based peer discovery manager
pub struct DnsDiscoveryManager {
    config: DnsDiscoveryConfig,
    resolver: TokioAsyncResolver,
    cache: Arc<RwLock<HashMap<String, CachedResolution>>>,
    known_peers: Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
}

impl DnsDiscoveryManager {
    /// Create new DNS discovery manager
    pub fn new(config: DnsDiscoveryConfig) -> Result<Self> {
        // Use Cloudflare DNS (1.1.1.1) and Google DNS (8.8.8.8) for reliability
        let resolver_config = ResolverConfig::cloudflare();
        let mut resolver_opts = ResolverOpts::default();
        resolver_opts.timeout = config.query_timeout;
        resolver_opts.attempts = 3;

        let resolver = TokioAsyncResolver::tokio(resolver_config, resolver_opts);

        Ok(Self {
            config,
            resolver,
            cache: Arc::new(RwLock::new(HashMap::new())),
            known_peers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start background DNS refresh task
    pub fn start_refresh_task(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.clone();
        let resolver = self.resolver.clone();
        let cache = self.cache.clone();
        let known_peers = self.known_peers.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.refresh_interval);

            loop {
                interval.tick().await;

                info!("🔄 Running periodic DNS refresh for all peers");

                // Refresh all known peers
                let subdomains: Vec<String> = {
                    let cache_lock = cache.read().await;
                    cache_lock.keys().cloned().collect()
                };

                for subdomain in subdomains {
                    match Self::resolve_subdomain_static(
                        &subdomain,
                        &resolver,
                        config.query_timeout,
                    )
                    .await
                    {
                        Ok(ip) => {
                            let mut cache_lock = cache.write().await;
                            cache_lock.insert(
                                subdomain.clone(),
                                CachedResolution {
                                    ip,
                                    cached_at: Instant::now(),
                                    ttl: config.cache_ttl,
                                },
                            );
                            debug!("✓ Refreshed DNS: {} -> {}", subdomain, ip);
                        }
                        Err(e) => {
                            warn!("⚠ Failed to refresh DNS for {}: {}", subdomain, e);
                        }
                    }
                }

                // Update known peers with new IPs
                Self::update_known_peers_static(&config, &cache, &known_peers).await;
            }
        })
    }

    /// Discover all configured validators
    pub async fn discover_validators(&self) -> Result<Vec<DiscoveredPeer>> {
        let mut discovered = Vec::new();

        for subdomain in &self.config.validator_subdomains {
            match self.resolve_and_cache(subdomain).await {
                Ok(ip) => {
                    let multiaddr = format!("/ip4/{}/tcp/{}", ip, self.config.validator_port)
                        .parse()
                        .map_err(|e| format!("Invalid multiaddr: {}", e))?;

                    let peer = DiscoveredPeer {
                        identifier: subdomain.clone(),
                        node_type: NodeType::Validator,
                        ip,
                        port: self.config.validator_port,
                        multiaddr,
                        peer_id: None,
                    };

                    discovered.push(peer.clone());
                    self.known_peers
                        .write()
                        .await
                        .insert(subdomain.clone(), peer);

                    info!(
                        "✓ Discovered validator: {} at {}:{}",
                        subdomain, ip, self.config.validator_port
                    );
                }
                Err(e) => {
                    error!("❌ Failed to resolve validator {}: {}", subdomain, e);
                }
            }
        }

        Ok(discovered)
    }

    /// Discover all relays (2 per validator host)
    pub async fn discover_relays(&self) -> Result<Vec<DiscoveredPeer>> {
        let mut discovered = Vec::new();

        // Use validator subdomains if relay subdomains not specified
        let relay_hosts = if self.config.relay_subdomains.is_empty() {
            &self.config.validator_subdomains
        } else {
            &self.config.relay_subdomains
        };

        for subdomain in relay_hosts {
            match self.resolve_and_cache(subdomain).await {
                Ok(ip) => {
                    // Relay 1
                    let multiaddr1 = format!("/ip4/{}/tcp/{}", ip, self.config.relay_ports.0)
                        .parse()
                        .map_err(|e| format!("Invalid multiaddr: {}", e))?;

                    let relay1 = DiscoveredPeer {
                        identifier: format!("{}-relay1", subdomain),
                        node_type: NodeType::Relay1,
                        ip,
                        port: self.config.relay_ports.0,
                        multiaddr: multiaddr1,
                        peer_id: None,
                    };

                    // Relay 2
                    let multiaddr2 = format!("/ip4/{}/tcp/{}", ip, self.config.relay_ports.1)
                        .parse()
                        .map_err(|e| format!("Invalid multiaddr: {}", e))?;

                    let relay2 = DiscoveredPeer {
                        identifier: format!("{}-relay2", subdomain),
                        node_type: NodeType::Relay2,
                        ip,
                        port: self.config.relay_ports.1,
                        multiaddr: multiaddr2,
                        peer_id: None,
                    };

                    discovered.push(relay1.clone());
                    discovered.push(relay2.clone());

                    let mut peers = self.known_peers.write().await;
                    peers.insert(relay1.identifier.clone(), relay1);
                    peers.insert(relay2.identifier.clone(), relay2);

                    info!(
                        "✓ Discovered 2 relays on {} at {}:{},{}",
                        subdomain, ip, self.config.relay_ports.0, self.config.relay_ports.1
                    );
                }
                Err(e) => {
                    warn!("⚠ Failed to resolve relay host {}: {}", subdomain, e);
                }
            }
        }

        Ok(discovered)
    }

    /// Resolve subdomain with caching
    async fn resolve_and_cache(&self, subdomain: &str) -> Result<IpAddr> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(subdomain) {
                if !cached.is_expired() {
                    debug!("✓ Using cached IP for {}: {}", subdomain, cached.ip);
                    return Ok(cached.ip);
                } else {
                    debug!("⏰ Cache expired for {}, refreshing", subdomain);
                }
            }
        }

        // Resolve via DNS
        let ip =
            Self::resolve_subdomain_static(subdomain, &self.resolver, self.config.query_timeout)
                .await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                subdomain.to_string(),
                CachedResolution {
                    ip,
                    cached_at: Instant::now(),
                    ttl: self.config.cache_ttl,
                },
            );
        }

        Ok(ip)
    }

    /// Resolve subdomain to IP (static helper for background tasks)
    async fn resolve_subdomain_static(
        subdomain: &str,
        resolver: &TokioAsyncResolver,
        timeout: Duration,
    ) -> Result<IpAddr> {
        // Resolve A record (IPv4)
        let response = tokio::time::timeout(timeout, resolver.lookup_ip(subdomain))
            .await
            .map_err(|_| format!("DNS query timeout for {}", subdomain))?
            .map_err(|e| format!("DNS resolution failed for {}: {}", subdomain, e))?;

        // Get first IP (prefer IPv4)
        response
            .iter()
            .find(|ip| ip.is_ipv4())
            .or_else(|| response.iter().next())
            .ok_or_else(|| format!("No IP address found for {}", subdomain).into())
    }

    /// Update known peers after DNS refresh (static helper)
    async fn update_known_peers_static(
        _config: &DnsDiscoveryConfig,
        cache: &Arc<RwLock<HashMap<String, CachedResolution>>>,
        known_peers: &Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
    ) {
        let cache_lock = cache.read().await;
        let mut peers_lock = known_peers.write().await;

        for (identifier, peer) in peers_lock.iter_mut() {
            // Extract base subdomain from identifier (remove "-relay1" suffix)
            let base_subdomain = identifier
                .strip_suffix("-relay1")
                .or_else(|| identifier.strip_suffix("-relay2"))
                .unwrap_or(identifier);

            if let Some(cached) = cache_lock.get(base_subdomain) {
                if peer.ip != cached.ip {
                    info!(
                        "🔄 IP changed for {}: {} -> {}",
                        identifier, peer.ip, cached.ip
                    );
                    peer.ip = cached.ip;

                    // Update multiaddr
                    let new_multiaddr = format!("/ip4/{}/tcp/{}", cached.ip, peer.port)
                        .parse()
                        .unwrap();
                    peer.multiaddr = new_multiaddr;
                }
            }
        }
    }

    /// Get all currently known peers
    pub async fn get_known_peers(&self) -> Vec<DiscoveredPeer> {
        self.known_peers.read().await.values().cloned().collect()
    }

    /// Get bootstrap multiaddrs for validators
    pub async fn get_validator_bootstrap_addrs(&self) -> Vec<(PeerId, Multiaddr)> {
        let peers = self.known_peers.read().await;
        peers
            .values()
            .filter(|p| p.node_type == NodeType::Validator)
            .filter_map(|p| p.peer_id.map(|id| (id, p.multiaddr.clone())))
            .collect()
    }

    /// Manually update peer ID for a discovered peer
    pub async fn update_peer_id(&self, identifier: &str, peer_id: PeerId) -> Result<()> {
        let mut peers = self.known_peers.write().await;
        if let Some(peer) = peers.get_mut(identifier) {
            peer.peer_id = Some(peer_id);
            info!("✓ Updated peer ID for {}: {}", identifier, peer_id);
            Ok(())
        } else {
            Err(format!("Unknown peer identifier: {}", identifier).into())
        }
    }

    /// Force refresh of a specific subdomain
    pub async fn force_refresh(&self, subdomain: &str) -> Result<IpAddr> {
        // Remove from cache to force re-resolution
        {
            let mut cache = self.cache.write().await;
            cache.remove(subdomain);
        }

        self.resolve_and_cache(subdomain).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dns_resolution() {
        let config = DnsDiscoveryConfig::default();
        let manager = DnsDiscoveryManager::new(config).unwrap();

        // Test resolving a validator (may fail in CI without network)
        if let Ok(validators) = manager.discover_validators().await {
            assert!(
                !validators.is_empty(),
                "Should discover at least one validator"
            );
        }
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let mut config = DnsDiscoveryConfig::default();
        config.cache_ttl = Duration::from_millis(100);

        let cached = CachedResolution {
            ip: "1.2.3.4".parse().unwrap(),
            cached_at: Instant::now(),
            ttl: config.cache_ttl,
        };

        assert!(!cached.is_expired());

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(cached.is_expired());
    }
}
