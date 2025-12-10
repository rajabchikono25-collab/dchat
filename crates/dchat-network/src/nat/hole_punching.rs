/// UDP Hole Punching
///
/// Coordinates simultaneous UDP packet exchange to establish
/// direct P2P connections through NAT devices.
///
/// Algorithm:
/// 1. Both peers learn their external addresses via STUN
/// 2. Peers exchange external addresses via signaling server
/// 3. Both peers send UDP packets to each other's external address
/// 4. NAT devices create bidirectional mappings
/// 5. Direct P2P connection established
///
/// Works for: Full cone, restricted cone, port-restricted cone NAT
/// Fails for: Symmetric NAT (requires TURN relay)
///
/// See ARCHITECTURE.md Section 12.1: NAT Traversal
use dchat_core::Result;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::time::{sleep, Duration};

/// UDP hole puncher
pub struct HolePuncher {
    port_range: (u16, u16),
    punch_attempts: u32,
    punch_interval: Duration,
}

impl HolePuncher {
    /// Create new hole puncher
    pub fn new(port_range: (u16, u16)) -> Self {
        Self {
            port_range,
            punch_attempts: 10,                         // Try 10 times
            punch_interval: Duration::from_millis(200), // 200ms between attempts
        }
    }

    /// Coordinate hole punch with remote peer
    ///
    /// Both peers must call this simultaneously with each other's
    /// external address. The signaling server coordinates timing.
    pub async fn coordinate_punch(
        &self,
        peer_external: SocketAddr,
        local_port: u16,
    ) -> Result<SocketAddr> {
        // Bind to specific local port
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", local_port))
            .await
            .map_err(|e| dchat_core::Error::network(format!("Hole punch bind failed: {}", e)))?;

        // Set socket to non-blocking for simultaneous send/receive
        socket.set_ttl(64).ok();

        // Spawn concurrent send and receive tasks for bidirectional punching
        let socket_send = std::sync::Arc::new(socket);
        let socket_recv = socket_send.clone();

        let send_handle = {
            let socket = socket_send.clone();
            tokio::spawn(async move {
                for attempt in 0..10 {
                    let punch_msg = format!("PUNCH:{}", attempt);
                    if let Err(e) = socket.send_to(punch_msg.as_bytes(), peer_external).await {
                        tracing::warn!("Hole punch send failed (attempt {}): {}", attempt, e);
                    }
                    sleep(Duration::from_millis(200)).await;
                }
            })
        };

        // Receive loop
        for attempt in 0..self.punch_attempts {
            let mut buf = vec![0u8; 1024];

            match tokio::time::timeout(self.punch_interval, socket_recv.recv_from(&mut buf)).await {
                Ok(Ok((len, addr))) => {
                    let msg = String::from_utf8_lossy(&buf[..len]);

                    // Check if this is a valid punch response
                    if msg.starts_with("PUNCH:") {
                        // Send ACK back
                        let ack_msg = format!("ACK:{}", attempt);
                        let _ = socket_recv.send_to(ack_msg.as_bytes(), addr).await;

                        tracing::info!("UDP hole punch successful with peer at {}", addr);

                        // Cancel send task
                        send_handle.abort();

                        return Ok(addr);
                    } else if msg.starts_with("ACK:") {
                        // Peer acknowledged our punch
                        tracing::info!("Received ACK from peer at {}", addr);

                        send_handle.abort();

                        return Ok(addr);
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!("Hole punch recv error (attempt {}): {}", attempt, e);
                }
                Err(_) => {
                    // Timeout, continue
                    tracing::trace!("Hole punch timeout (attempt {})", attempt);
                }
            }
        }

        send_handle.abort();

        Err(dchat_core::Error::network(
            "UDP hole punch failed: no response from peer after all attempts",
        ))
    }

    /// Attempt hole punch with port prediction
    ///
    /// For port-restricted cone NAT, try multiple sequential ports
    /// to increase success rate.
    pub async fn predict_and_punch(
        &self,
        peer_external: SocketAddr,
        local_port: u16,
    ) -> Result<SocketAddr> {
        // Try base port first
        if let Ok(addr) = self.coordinate_punch(peer_external, local_port).await {
            return Ok(addr);
        }

        // Try sequential ports (port prediction)
        let base_port = peer_external.port();

        for offset in 1..=5 {
            let predicted_port = base_port.wrapping_add(offset);

            // Skip if outside port range
            if predicted_port < self.port_range.0 || predicted_port > self.port_range.1 {
                continue;
            }

            let mut predicted_addr = peer_external;
            predicted_addr.set_port(predicted_port);

            match self.coordinate_punch(predicted_addr, local_port).await {
                Ok(addr) => return Ok(addr),
                Err(_) => continue,
            }
        }

        Err(dchat_core::Error::network(
            "Port prediction hole punch failed",
        ))
    }

    /// Send keepalive to maintain NAT mapping
    pub async fn send_keepalive(socket: &UdpSocket, peer_addr: SocketAddr) -> Result<()> {
        socket
            .send_to(b"KEEPALIVE", peer_addr)
            .await
            .map_err(|e| dchat_core::Error::network(format!("Keepalive send failed: {}", e)))?;

        Ok(())
    }

    /// Keepalive loop to prevent NAT mapping timeout
    pub async fn keepalive_loop(socket: UdpSocket, peer_addr: SocketAddr, interval: Duration) {
        loop {
            sleep(interval).await;

            if let Err(e) = Self::send_keepalive(&socket, peer_addr).await {
                eprintln!("Keepalive failed: {}", e);
                break;
            }
        }
    }

    /// Check if hole punch is possible for given NAT types
    pub fn is_possible(local_nat: super::NatType, remote_nat: super::NatType) -> bool {
        use super::NatType;

        match (local_nat, remote_nat) {
            // Symmetric NAT - hole punching usually fails
            (NatType::Symmetric, _) | (_, NatType::Symmetric) => false,

            // Both public IPs - direct connection
            (NatType::None, NatType::None) => true,

            // One public - easy
            (NatType::None, _) | (_, NatType::None) => true,

            // Both cone NAT - hole punching works
            (NatType::FullCone, NatType::FullCone) => true,
            (NatType::FullCone, NatType::RestrictedCone) => true,
            (NatType::FullCone, NatType::PortRestrictedCone) => true,
            (NatType::RestrictedCone, NatType::FullCone) => true,
            (NatType::RestrictedCone, NatType::RestrictedCone) => true,
            (NatType::RestrictedCone, NatType::PortRestrictedCone) => true,
            (NatType::PortRestrictedCone, NatType::FullCone) => true,
            (NatType::PortRestrictedCone, NatType::RestrictedCone) => true,
            (NatType::PortRestrictedCone, NatType::PortRestrictedCone) => true,

            // Unknown - assume possible
            _ => true,
        }
    }
}

/// Hole punch coordinator (runs on signaling server)
pub struct HolePunchCoordinator {
    pending_punches: std::collections::HashMap<String, PunchRequest>,
}

/// Pending hole punch request
#[derive(Clone)]
pub struct PunchRequest {
    /// Peer ID requesting punch
    pub peer_id: String,
    /// External address to punch to
    pub external_addr: SocketAddr,
    /// When request was made
    pub timestamp: std::time::Instant,
}

impl Default for HolePunchCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl HolePunchCoordinator {
    /// Create new coordinator
    pub fn new() -> Self {
        Self {
            pending_punches: std::collections::HashMap::new(),
        }
    }

    /// Register punch request
    pub fn register_punch(
        &mut self,
        peer_id: String,
        external_addr: SocketAddr,
    ) -> Option<SocketAddr> {
        // Check if other peer is waiting
        for (waiting_peer_id, request) in &self.pending_punches {
            if waiting_peer_id != &peer_id {
                // Found waiting peer - return their address
                return Some(request.external_addr);
            }
        }

        // No waiting peer - add to pending
        self.pending_punches.insert(
            peer_id.clone(),
            PunchRequest {
                peer_id,
                external_addr,
                timestamp: std::time::Instant::now(),
            },
        );

        None
    }

    /// Clean up expired punch requests (> 30 seconds old)
    pub fn cleanup_expired(&mut self) {
        let now = std::time::Instant::now();
        self.pending_punches
            .retain(|_, req| now.duration_since(req.timestamp) < Duration::from_secs(30));
    }
}

#[cfg(test)]
mod tests {
    use super::super::NatType;
    use super::*;

    #[test]
    fn test_hole_puncher_creation() {
        let puncher = HolePuncher::new((49152, 65535));
        assert_eq!(puncher.port_range, (49152, 65535));
        assert_eq!(puncher.punch_attempts, 10);
    }

    #[test]
    fn test_hole_punch_possibility() {
        use NatType::*;

        // Public IPs - always possible
        assert!(HolePuncher::is_possible(None, None));
        assert!(HolePuncher::is_possible(None, FullCone));

        // Cone NAT combinations - possible
        assert!(HolePuncher::is_possible(FullCone, FullCone));
        assert!(HolePuncher::is_possible(RestrictedCone, RestrictedCone));
        assert!(HolePuncher::is_possible(PortRestrictedCone, RestrictedCone));

        // Symmetric NAT - not possible
        assert!(!HolePuncher::is_possible(Symmetric, Symmetric));
        assert!(!HolePuncher::is_possible(Symmetric, FullCone));
        assert!(!HolePuncher::is_possible(RestrictedCone, Symmetric));
    }

    #[tokio::test]
    async fn test_coordinate_punch_timeout() {
        let puncher = HolePuncher::new((49152, 65535));

        // Try to punch to non-existent peer
        let peer_addr: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let result = puncher.coordinate_punch(peer_addr, 50000).await;

        // Should timeout/fail
        assert!(result.is_err());
    }

    #[test]
    fn test_coordinator_creation() {
        let coordinator = HolePunchCoordinator::new();
        assert_eq!(coordinator.pending_punches.len(), 0);
    }

    #[test]
    fn test_coordinator_register_punch() {
        let mut coordinator = HolePunchCoordinator::new();

        let addr1: SocketAddr = "1.2.3.4:5000".parse().unwrap();
        let addr2: SocketAddr = "5.6.7.8:6000".parse().unwrap();

        // First peer registers
        let result = coordinator.register_punch("peer1".to_string(), addr1);
        assert!(result.is_none()); // No waiting peer

        // Second peer registers - should get first peer's address
        let result = coordinator.register_punch("peer2".to_string(), addr2);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), addr1);
    }

    #[test]
    fn test_coordinator_cleanup() {
        let mut coordinator = HolePunchCoordinator::new();

        let addr: SocketAddr = "1.2.3.4:5000".parse().unwrap();
        coordinator.register_punch("peer1".to_string(), addr);

        assert_eq!(coordinator.pending_punches.len(), 1);

        // Cleanup (won't remove as not expired)
        coordinator.cleanup_expired();
        assert_eq!(coordinator.pending_punches.len(), 1);
    }
}
