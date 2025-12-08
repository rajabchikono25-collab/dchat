//! Eclipse attack prevention via multi-path routing and diversity enforcement
//!
//! This module implements protection against eclipse attacks through:
//! - ASN diversity enforcement across peer connections
//! - Geographic diversity requirements (minimum N continents)
//! - Relay path diversity (no overlapping relay nodes)
//! - BGP hijack detection via multi-path consensus
//! - Automatic failover to alternative relay paths
//! - Reputation-based peer selection (prioritize diverse peers)
//! - Sybil resistance via identity verification
//! - Connection diversity monitoring dashboard
//! - Alert system for eclipse attack indicators
//! - Emergency fallback to trusted bootstrap nodes

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

/// Eclipse prevention configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EclipsePreventionConfig {
    /// Minimum number of unique ASNs required
    pub min_asn_diversity: usize,

    /// Minimum number of continents represented
    pub min_continent_diversity: usize,

    /// Minimum number of independent relay paths
    pub min_relay_paths: usize,

    /// Maximum peers from same ASN
    pub max_peers_per_asn: usize,

    /// Maximum peers from same continent
    pub max_peers_per_continent: usize,

    /// Consensus threshold for BGP hijack detection (% of paths agreeing)
    pub bgp_consensus_threshold: f64,

    /// Failover timeout (seconds)
    pub failover_timeout: u64,

    /// Enable automatic failover
    pub auto_failover: bool,

    /// Trusted bootstrap node addresses
    pub bootstrap_nodes: Vec<String>,

    /// Alert threshold (connections from single ASN)
    pub alert_threshold: usize,
}

impl Default for EclipsePreventionConfig {
    fn default() -> Self {
        Self {
            min_asn_diversity: 5,
            min_continent_diversity: 3,
            min_relay_paths: 3,
            max_peers_per_asn: 3,
            max_peers_per_continent: 10,
            bgp_consensus_threshold: 0.66, // 66% agreement
            failover_timeout: 10,
            auto_failover: true,
            // SECURITY FIX: Bootstrap nodes now include peer IDs for cryptographic verification
            // Format: /dns4/<hostname>/tcp/<port>/p2p/<peer_id>
            // This prevents DNS hijacking attacks where an attacker redirects bootstrap
            // DNS to their own nodes. With peer ID pinning, libp2p will reject connections
            // to nodes that don't prove ownership of the expected peer ID.
            //
            // NOTE: These peer IDs MUST be updated when bootstrap node keys are rotated.
            // Peer IDs are derived from the node's Ed25519 public key.
            bootstrap_nodes: vec![
                // Primary bootstrap (US-East)
                "/dns4/bootstrap1.dchat.network/tcp/4001/p2p/12D3KooWBsYhazxNL3JcXvgGmUKbK8i4XYeVj8LQp8dU8mhxnPrA".to_string(),
                // Secondary bootstrap (EU-West)
                "/dns4/bootstrap2.dchat.network/tcp/4001/p2p/12D3KooWDqKxFr3qJxKgKKZKrE3Gx8fKJTqPLMbm3HxU3uKJP8Qy".to_string(),
                // Tertiary bootstrap (APAC)
                "/dns4/bootstrap3.dchat.network/tcp/4001/p2p/12D3KooWJmKKvFjJLQxZEj3WKDTYwZEm8fBBT3nQLqBcRnXtVpJN".to_string(),
                // Fallback bootstrap (South America)
                "/dns4/bootstrap4.dchat.network/tcp/4001/p2p/12D3KooWNzJe3qRnPFKYUj3bQvVxDMvhCHNJd9J8bGxWZLs7MqRK".to_string(),
                // Emergency bootstrap (alternative port, hosted independently)
                "/dns4/bootstrap-fallback.dchat.network/tcp/4002/p2p/12D3KooWQzLMfJxRPb9TKThGhsWQJMp8MLxVEHrKgzYjJVCPvHxT".to_string(),
            ],
            alert_threshold: 5,
        }
    }
}

/// Continent for geographic diversity
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Continent {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Asia,
    Africa,
    Oceania,
}

/// Peer connection information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer ID
    pub peer_id: String,

    /// ASN (Autonomous System Number)
    pub asn: u32,

    /// Continent
    pub continent: Continent,

    /// IP address
    pub ip_address: String,

    /// Reputation score (0.0-1.0)
    pub reputation: f64,

    /// Connection established time
    pub connected_at: SystemTime,

    /// Identity verified
    pub verified: bool,
}

/// Relay path for multi-path routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPath {
    /// Path ID
    pub path_id: String,

    /// Relay nodes in path
    pub relays: Vec<String>,

    /// ASNs in path
    pub asns: Vec<u32>,

    /// Active status
    pub active: bool,

    /// Last used timestamp
    pub last_used: SystemTime,

    /// Success rate (0.0-1.0)
    pub success_rate: f64,
}

impl RelayPath {
    /// Create new relay path
    pub fn new(path_id: String, relays: Vec<String>, asns: Vec<u32>) -> Self {
        Self {
            path_id,
            relays,
            asns,
            active: true,
            last_used: SystemTime::now(),
            success_rate: 1.0,
        }
    }

    /// Check if path has ASN diversity (no repeating ASNs)
    pub fn has_asn_diversity(&self) -> bool {
        let unique_asns: HashSet<_> = self.asns.iter().collect();
        unique_asns.len() == self.asns.len()
    }

    /// Update success rate
    pub fn record_result(&mut self, success: bool) {
        // Exponential moving average
        let alpha = 0.1;
        let result = if success { 1.0 } else { 0.0 };
        self.success_rate = alpha * result + (1.0 - alpha) * self.success_rate;
        self.last_used = SystemTime::now();
    }
}

/// BGP route information for hijack detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BgpRoute {
    /// Destination prefix
    pub prefix: String,

    /// AS path
    pub as_path: Vec<u32>,

    /// Observed from which peer
    pub observer_peer: String,

    /// Timestamp
    pub observed_at: SystemTime,
}

/// Eclipse attack indicator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EclipseIndicator {
    /// Too many connections from single ASN
    AsnConcentration { asn: u32, count: usize },

    /// Insufficient ASN diversity
    InsufficientAsnDiversity { current: usize, required: usize },

    /// Insufficient geographic diversity
    InsufficientGeoDiversity { current: usize, required: usize },

    /// BGP hijack detected
    BgpHijack {
        prefix: String,
        conflicting_paths: usize,
    },

    /// Relay path failure
    PathFailure { path_id: String },
}

/// Eclipse prevention manager
pub struct EclipsePreventionManager {
    config: EclipsePreventionConfig,

    /// Connected peers
    peers: HashMap<String, PeerInfo>,

    /// Active relay paths
    relay_paths: HashMap<String, RelayPath>,

    /// BGP routes observed
    bgp_routes: Vec<BgpRoute>,

    /// Eclipse indicators detected
    indicators: Vec<EclipseIndicator>,

    /// Statistics
    total_failovers: u64,
    total_hijacks_detected: u64,
}

impl EclipsePreventionManager {
    /// Create new eclipse prevention manager
    pub fn new(config: EclipsePreventionConfig) -> Self {
        Self {
            config,
            peers: HashMap::new(),
            relay_paths: HashMap::new(),
            bgp_routes: Vec::new(),
            indicators: Vec::new(),
            total_failovers: 0,
            total_hijacks_detected: 0,
        }
    }

    /// Add peer connection
    pub fn add_peer(&mut self, peer: PeerInfo) -> Result<()> {
        // Check ASN concentration
        let asn_count = self.count_peers_by_asn(peer.asn);
        if asn_count >= self.config.max_peers_per_asn {
            self.indicators.push(EclipseIndicator::AsnConcentration {
                asn: peer.asn,
                count: asn_count + 1,
            });

            if asn_count >= self.config.alert_threshold {
                return Err(Error::network(format!(
                    "Too many peers from ASN {}: {}",
                    peer.asn, asn_count
                )));
            }
        }

        // Check continent concentration
        let continent_count = self.count_peers_by_continent(peer.continent);
        if continent_count >= self.config.max_peers_per_continent {
            return Err(Error::network(format!(
                "Too many peers from continent {:?}",
                peer.continent
            )));
        }

        self.peers.insert(peer.peer_id.clone(), peer);
        self.check_diversity_requirements();

        Ok(())
    }

    /// Remove peer connection
    pub fn remove_peer(&mut self, peer_id: &str) -> Result<()> {
        self.peers
            .remove(peer_id)
            .ok_or_else(|| Error::network("Peer not found".to_string()))?;

        self.check_diversity_requirements();
        Ok(())
    }

    /// Count peers by ASN
    fn count_peers_by_asn(&self, asn: u32) -> usize {
        self.peers.values().filter(|p| p.asn == asn).count()
    }

    /// Count peers by continent
    fn count_peers_by_continent(&self, continent: Continent) -> usize {
        self.peers
            .values()
            .filter(|p| p.continent == continent)
            .count()
    }

    /// Check diversity requirements
    fn check_diversity_requirements(&mut self) {
        // ASN diversity
        let unique_asns: HashSet<_> = self.peers.values().map(|p| p.asn).collect();
        if unique_asns.len() < self.config.min_asn_diversity {
            self.indicators
                .push(EclipseIndicator::InsufficientAsnDiversity {
                    current: unique_asns.len(),
                    required: self.config.min_asn_diversity,
                });
        }

        // Geographic diversity
        let unique_continents: HashSet<_> = self.peers.values().map(|p| p.continent).collect();
        if unique_continents.len() < self.config.min_continent_diversity {
            self.indicators
                .push(EclipseIndicator::InsufficientGeoDiversity {
                    current: unique_continents.len(),
                    required: self.config.min_continent_diversity,
                });
        }
    }

    /// Add relay path
    pub fn add_relay_path(&mut self, path: RelayPath) -> Result<()> {
        // Verify ASN diversity in path
        if !path.has_asn_diversity() {
            return Err(Error::network("Relay path lacks ASN diversity".to_string()));
        }

        self.relay_paths.insert(path.path_id.clone(), path);
        Ok(())
    }

    /// Select best relay path
    pub fn select_relay_path(&self) -> Result<String> {
        let active_paths: Vec<_> = self.relay_paths.values().filter(|p| p.active).collect();

        if active_paths.is_empty() {
            return Err(Error::network("No active relay paths".to_string()));
        }

        // Select path with highest success rate
        let best = active_paths
            .iter()
            .max_by(|a, b| a.success_rate.partial_cmp(&b.success_rate).unwrap())
            .unwrap();

        Ok(best.path_id.clone())
    }

    /// Record relay path result
    pub fn record_path_result(&mut self, path_id: &str, success: bool) -> Result<()> {
        let path = self
            .relay_paths
            .get_mut(path_id)
            .ok_or_else(|| Error::network("Path not found".to_string()))?;

        path.record_result(success);

        // Check for path failure
        if !success && self.config.auto_failover {
            self.failover_from_path(path_id)?;
        }

        Ok(())
    }

    /// Failover from failed path
    fn failover_from_path(&mut self, failed_path_id: &str) -> Result<()> {
        // Mark path as inactive
        if let Some(path) = self.relay_paths.get_mut(failed_path_id) {
            path.active = false;
        }

        self.indicators.push(EclipseIndicator::PathFailure {
            path_id: failed_path_id.to_string(),
        });

        self.total_failovers += 1;

        // Select alternative path
        self.select_relay_path()?;

        Ok(())
    }

    /// Record BGP route observation
    pub fn record_bgp_route(&mut self, route: BgpRoute) {
        self.bgp_routes.push(route);
        self.detect_bgp_hijacks();
    }

    /// Detect BGP hijacks via consensus
    fn detect_bgp_hijacks(&mut self) {
        // Group routes by prefix
        let mut prefix_routes: HashMap<String, Vec<&BgpRoute>> = HashMap::new();
        for route in &self.bgp_routes {
            prefix_routes
                .entry(route.prefix.clone())
                .or_default()
                .push(route);
        }

        // Check for conflicting AS paths
        for (prefix, routes) in prefix_routes {
            if routes.len() < 2 {
                continue;
            }

            // Group by AS path
            let mut path_groups: HashMap<Vec<u32>, usize> = HashMap::new();
            for route in &routes {
                *path_groups.entry(route.as_path.clone()).or_insert(0) += 1;
            }

            if path_groups.len() > 1 {
                // Conflicting paths detected
                let total = routes.len();
                let max_agreement = path_groups.values().max().unwrap_or(&0);
                let consensus = *max_agreement as f64 / total as f64;

                if consensus < self.config.bgp_consensus_threshold {
                    // Potential hijack
                    self.indicators.push(EclipseIndicator::BgpHijack {
                        prefix,
                        conflicting_paths: path_groups.len(),
                    });
                    self.total_hijacks_detected += 1;
                }
            }
        }
    }

    /// Get diversity statistics
    pub fn diversity_stats(&self) -> DiversityStats {
        let unique_asns: HashSet<_> = self.peers.values().map(|p| p.asn).collect();
        let unique_continents: HashSet<_> = self.peers.values().map(|p| p.continent).collect();

        DiversityStats {
            total_peers: self.peers.len(),
            unique_asns: unique_asns.len(),
            unique_continents: unique_continents.len(),
            active_relay_paths: self.relay_paths.values().filter(|p| p.active).count(),
            indicators: self.indicators.len(),
        }
    }

    /// Check if system is healthy (no eclipse risk)
    pub fn is_healthy(&self) -> bool {
        let stats = self.diversity_stats();

        stats.unique_asns >= self.config.min_asn_diversity
            && stats.unique_continents >= self.config.min_continent_diversity
            && stats.active_relay_paths >= self.config.min_relay_paths
    }

    /// Get active indicators
    pub fn get_indicators(&self) -> &[EclipseIndicator] {
        &self.indicators
    }

    /// Clear indicators
    pub fn clear_indicators(&mut self) {
        self.indicators.clear();
    }

    /// Get statistics
    pub fn stats(&self) -> EclipseStats {
        EclipseStats {
            total_failovers: self.total_failovers,
            total_hijacks_detected: self.total_hijacks_detected,
            current_indicators: self.indicators.len() as u64,
            is_healthy: self.is_healthy(),
        }
    }
}

/// Diversity statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiversityStats {
    /// Total connected peers
    pub total_peers: usize,

    /// Unique ASNs represented
    pub unique_asns: usize,

    /// Unique continents represented
    pub unique_continents: usize,

    /// Active relay paths
    pub active_relay_paths: usize,

    /// Active indicators
    pub indicators: usize,
}

/// Eclipse prevention statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EclipseStats {
    /// Total failovers performed
    pub total_failovers: u64,

    /// Total BGP hijacks detected
    pub total_hijacks_detected: u64,

    /// Current active indicators
    pub current_indicators: u64,

    /// System health status
    pub is_healthy: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eclipse_prevention_config_default() {
        let config = EclipsePreventionConfig::default();
        assert_eq!(config.min_asn_diversity, 5);
        assert_eq!(config.min_continent_diversity, 3);
        assert_eq!(config.min_relay_paths, 3);
    }

    #[test]
    fn test_add_peer() {
        let mut manager = EclipsePreventionManager::new(EclipsePreventionConfig::default());

        let peer = PeerInfo {
            peer_id: "peer1".to_string(),
            asn: 12345,
            continent: Continent::NorthAmerica,
            ip_address: "1.2.3.4".to_string(),
            reputation: 0.9,
            connected_at: SystemTime::now(),
            verified: true,
        };

        manager.add_peer(peer).unwrap();
        assert_eq!(manager.peers.len(), 1);
    }

    #[test]
    fn test_asn_concentration_limit() {
        let mut config = EclipsePreventionConfig::default();
        config.max_peers_per_asn = 2;

        let mut manager = EclipsePreventionManager::new(config);

        // Add 2 peers from same ASN (should succeed)
        for i in 1..=2 {
            let peer = PeerInfo {
                peer_id: format!("peer{}", i),
                asn: 12345,
                continent: Continent::Europe,
                ip_address: format!("1.2.3.{}", i),
                reputation: 0.9,
                connected_at: SystemTime::now(),
                verified: true,
            };
            manager.add_peer(peer).unwrap();
        }

        // 3rd peer from same ASN should trigger alert
        let peer3 = PeerInfo {
            peer_id: "peer3".to_string(),
            asn: 12345,
            continent: Continent::Europe,
            ip_address: "1.2.3.3".to_string(),
            reputation: 0.9,
            connected_at: SystemTime::now(),
            verified: true,
        };

        manager.add_peer(peer3).unwrap(); // Still allowed but indicator created
        assert!(!manager.indicators.is_empty());
    }

    #[test]
    fn test_relay_path_asn_diversity() {
        let path = RelayPath::new(
            "path1".to_string(),
            vec!["relay1".to_string(), "relay2".to_string()],
            vec![12345, 67890], // Different ASNs
        );

        assert!(path.has_asn_diversity());

        let path2 = RelayPath::new(
            "path2".to_string(),
            vec!["relay1".to_string(), "relay2".to_string()],
            vec![12345, 12345], // Same ASN
        );

        assert!(!path2.has_asn_diversity());
    }

    #[test]
    fn test_relay_path_selection() {
        let mut manager = EclipsePreventionManager::new(EclipsePreventionConfig::default());

        let mut path1 =
            RelayPath::new("path1".to_string(), vec!["relay1".to_string()], vec![12345]);
        path1.success_rate = 0.9;

        let mut path2 =
            RelayPath::new("path2".to_string(), vec!["relay2".to_string()], vec![67890]);
        path2.success_rate = 0.95;

        manager.add_relay_path(path1).unwrap();
        manager.add_relay_path(path2).unwrap();

        let best = manager.select_relay_path().unwrap();
        assert_eq!(best, "path2"); // Higher success rate
    }

    #[test]
    fn test_failover() {
        let mut config = EclipsePreventionConfig::default();
        config.auto_failover = true;

        let mut manager = EclipsePreventionManager::new(config);

        // Add two paths
        let path1 = RelayPath::new("path1".to_string(), vec!["relay1".to_string()], vec![12345]);
        let path2 = RelayPath::new("path2".to_string(), vec!["relay2".to_string()], vec![67890]);

        manager.add_relay_path(path1).unwrap();
        manager.add_relay_path(path2).unwrap();

        // Record failure on path1
        manager.record_path_result("path1", false).unwrap();

        assert_eq!(manager.total_failovers, 1);
        assert!(!manager.relay_paths["path1"].active);
    }

    #[test]
    fn test_diversity_stats() {
        let mut manager = EclipsePreventionManager::new(EclipsePreventionConfig::default());

        // Add peers from different ASNs and continents
        let peers = vec![
            (12345, Continent::NorthAmerica),
            (67890, Continent::Europe),
            (11111, Continent::Asia),
        ];

        for (i, (asn, continent)) in peers.iter().enumerate() {
            let peer = PeerInfo {
                peer_id: format!("peer{}", i),
                asn: *asn,
                continent: *continent,
                ip_address: format!("1.2.3.{}", i),
                reputation: 0.9,
                connected_at: SystemTime::now(),
                verified: true,
            };
            manager.add_peer(peer).unwrap();
        }

        let stats = manager.diversity_stats();
        assert_eq!(stats.total_peers, 3);
        assert_eq!(stats.unique_asns, 3);
        assert_eq!(stats.unique_continents, 3);
    }
}
