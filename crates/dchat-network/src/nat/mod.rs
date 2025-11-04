/// NAT Traversal - Unified API for connectivity across NAT boundaries
///
/// This module provides comprehensive NAT traversal strategies:
/// 1. UPnP IGD - Automatic port mapping
/// 2. STUN - Public address discovery
/// 3. UDP Hole Punching - Direct P2P connections
/// 4. TURN Relay - Fallback for symmetric NAT
///
/// Architecture:
/// - Tries UPnP first (fastest, most reliable)
/// - Falls back to STUN + hole punching for restricted NAT
/// - Uses TURN relay only when direct connection impossible
///
/// See ARCHITECTURE.md Section 12: Network Resilience
use dchat_core::Result;
use std::net::SocketAddr;
use std::time::Duration;

pub mod hole_punching;
pub mod stun;
pub mod turn;
pub mod upnp;

pub use hole_punching::HolePuncher;
pub use stun::StunClient;
pub use turn::TurnClient;
pub use upnp::UpnpClient;

/// NAT traversal configuration
#[derive(Debug, Clone)]
pub struct NatConfig {
    /// Enable UPnP IGD
    pub enable_upnp: bool,

    /// STUN server addresses
    pub stun_servers: Vec<String>,

    /// Enable UDP hole punching
    pub enable_hole_punching: bool,

    /// TURN server configuration (optional)
    pub turn_servers: Vec<TurnServer>,

    /// Timeout for NAT discovery
    pub discovery_timeout: Duration,

    /// Port mapping lease time (UPnP)
    pub lease_duration: Duration,

    /// External port range for hole punching
    pub port_range: (u16, u16),
}

impl Default for NatConfig {
    fn default() -> Self {
        Self {
            enable_upnp: true,
            stun_servers: vec![
                "stun.l.google.com:19302".to_string(),
                "stun1.l.google.com:19302".to_string(),
                "stun2.l.google.com:19302".to_string(),
            ],
            enable_hole_punching: true,
            turn_servers: Vec::new(),
            discovery_timeout: Duration::from_secs(5),
            lease_duration: Duration::from_secs(3600), // 1 hour
            port_range: (49152, 65535),                // Dynamic ports
        }
    }
}

/// TURN server configuration
#[derive(Debug, Clone)]
pub struct TurnServer {
    /// Server address
    pub address: String,

    /// Username for authentication
    pub username: String,

    /// Password/credential
    pub credential: String,

    /// Server priority (lower = higher priority)
    pub priority: u8,
}

/// NAT type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    /// No NAT - Public IP
    None,

    /// Full cone NAT - Easiest to traverse
    FullCone,

    /// Restricted cone NAT - Requires hole punching
    RestrictedCone,

    /// Port restricted cone NAT
    PortRestrictedCone,

    /// Symmetric NAT - Hardest, requires TURN
    Symmetric,

    /// Unknown/detection failed
    Unknown,
}

impl NatType {
    /// Check if direct P2P connection is possible
    pub fn supports_direct_connection(&self) -> bool {
        matches!(
            self,
            NatType::None
                | NatType::FullCone
                | NatType::RestrictedCone
                | NatType::PortRestrictedCone
        )
    }

    /// Check if TURN relay is required
    pub fn requires_turn(&self) -> bool {
        matches!(self, NatType::Symmetric)
    }
}

/// NAT traversal manager
pub struct NatTraversal {
    config: NatConfig,
    upnp: Option<UpnpClient>,
    stun: StunClient,
    hole_puncher: Option<HolePuncher>,
    turn: Option<TurnClient>,
    detected_type: NatType,
    external_addr: Option<SocketAddr>,
}

impl NatTraversal {
    /// Create new NAT traversal manager
    pub async fn new(config: NatConfig) -> Result<Self> {
        // Initialize UPnP if enabled
        let upnp = if config.enable_upnp {
            match UpnpClient::new(config.discovery_timeout).await {
                Ok(client) => Some(client),
                Err(e) => {
                    eprintln!("UPnP initialization failed: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Initialize STUN client
        let stun = StunClient::new(config.stun_servers.clone())?;

        // Initialize hole puncher if enabled
        let hole_puncher = if config.enable_hole_punching {
            Some(HolePuncher::new(config.port_range))
        } else {
            None
        };

        // Initialize TURN client if configured
        let turn = if !config.turn_servers.is_empty() {
            Some(TurnClient::new(config.turn_servers.clone()))
        } else {
            None
        };

        Ok(Self {
            config,
            upnp,
            stun,
            hole_puncher,
            turn,
            detected_type: NatType::Unknown,
            external_addr: None,
        })
    }

    /// Detect NAT type and external address
    pub async fn detect(&mut self) -> Result<(NatType, Option<SocketAddr>)> {
        // Try STUN to get external address
        let external_addr = self.stun.get_external_address().await?;
        self.external_addr = Some(external_addr);

        // Classify NAT type based on STUN results
        let nat_type = self.stun.detect_nat_type().await?;
        self.detected_type = nat_type;

        Ok((nat_type, Some(external_addr)))
    }

    /// Attempt to establish connectivity using best strategy
    pub async fn establish_connectivity(&mut self, local_port: u16) -> Result<ConnectivityInfo> {
        // Strategy 1: Try UPnP first (fastest)
        if let Some(upnp) = &self.upnp {
            if let Ok(mapping) = upnp
                .add_port_mapping(local_port, "dchat".to_string(), self.config.lease_duration)
                .await
            {
                return Ok(ConnectivityInfo {
                    method: TraversalMethod::Upnp,
                    external_addr: SocketAddr::new(mapping.external_ip, mapping.external_port),
                    local_addr: SocketAddr::new("0.0.0.0".parse().unwrap(), local_port),
                    nat_type: self.detected_type,
                });
            }
        }

        // Strategy 2: STUN + hole punching for restricted NAT
        if self.detected_type.supports_direct_connection() {
            if let Some(addr) = self.external_addr {
                return Ok(ConnectivityInfo {
                    method: TraversalMethod::Stun,
                    external_addr: addr,
                    local_addr: SocketAddr::new("0.0.0.0".parse().unwrap(), local_port),
                    nat_type: self.detected_type,
                });
            }
        }

        // Strategy 3: TURN relay as last resort
        if let Some(turn) = &self.turn {
            let relay_addr = turn.allocate_relay().await?;
            return Ok(ConnectivityInfo {
                method: TraversalMethod::Turn,
                external_addr: relay_addr,
                local_addr: SocketAddr::new("0.0.0.0".parse().unwrap(), local_port),
                nat_type: self.detected_type,
            });
        }

        Err(dchat_core::Error::network(
            "Failed to establish connectivity",
        ))
    }

    /// Coordinate hole punch with remote peer
    pub async fn coordinate_hole_punch(
        &self,
        peer_external: SocketAddr,
        local_port: u16,
    ) -> Result<SocketAddr> {
        if let Some(puncher) = &self.hole_puncher {
            puncher.coordinate_punch(peer_external, local_port).await
        } else {
            Err(dchat_core::Error::network("Hole punching not enabled"))
        }
    }

    /// Get current NAT type
    pub fn nat_type(&self) -> NatType {
        self.detected_type
    }

    /// Get external address
    pub fn external_address(&self) -> Option<SocketAddr> {
        self.external_addr
    }

    /// Refresh port mappings (for UPnP)
    pub async fn refresh_mappings(&self) -> Result<()> {
        if let Some(upnp) = &self.upnp {
            upnp.refresh_all_mappings().await?;
        }
        Ok(())
    }

    /// Clean up resources
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(upnp) = &self.upnp {
            upnp.remove_all_mappings().await?;
        }

        if let Some(turn) = &mut self.turn {
            turn.close_all_relays().await?;
        }

        Ok(())
    }
}

/// Connectivity information after successful traversal
#[derive(Debug, Clone)]
pub struct ConnectivityInfo {
    /// Method used for traversal
    pub method: TraversalMethod,

    /// External (public) address
    pub external_addr: SocketAddr,

    /// Local (private) address
    pub local_addr: SocketAddr,

    /// Detected NAT type
    pub nat_type: NatType,
}

/// NAT traversal method used
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalMethod {
    /// UPnP port mapping
    Upnp,

    /// STUN + direct connection
    Stun,

    /// UDP hole punching
    HolePunching,

    /// TURN relay
    Turn,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nat_config_default() {
        let config = NatConfig::default();
        assert!(config.enable_upnp);
        assert!(!config.stun_servers.is_empty());
        assert!(config.enable_hole_punching);
    }

    #[test]
    fn test_nat_type_classification() {
        assert!(NatType::None.supports_direct_connection());
        assert!(NatType::FullCone.supports_direct_connection());
        assert!(NatType::RestrictedCone.supports_direct_connection());
        assert!(!NatType::Symmetric.supports_direct_connection());

        assert!(!NatType::FullCone.requires_turn());
        assert!(NatType::Symmetric.requires_turn());
    }

    #[tokio::test]
    async fn test_nat_traversal_creation() {
        let config = NatConfig {
            enable_upnp: false, // Disable for testing
            ..Default::default()
        };

        let result = NatTraversal::new(config).await;
        assert!(result.is_ok());
    }
}
