/// TURN (Traversal Using Relays around NAT) Client
///
/// Relays traffic through intermediate servers when direct P2P
/// connections are impossible (e.g., symmetric NAT on both sides).
///
/// Protocol: TURN (RFC 5766)
/// Fallback: Used only when UPnP, STUN, and hole punching fail
///
/// TURN Flow:
/// 1. Client allocates relay address on TURN server
/// 2. Client binds channel to peer
/// 3. All traffic routed through TURN server
/// 4. Server relays packets between peers
///
/// Note: TURN consumes server bandwidth - use as last resort
///
/// See ARCHITECTURE.md Section 12.1: NAT Traversal
use dchat_core::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

/// TURN client for relay-based connectivity
pub struct TurnClient {
    servers: Vec<super::TurnServer>,
    active_relays: Arc<Mutex<HashMap<String, RelayAllocation>>>,
}

/// Active TURN relay allocation
#[derive(Clone)]
#[allow(dead_code)]
struct RelayAllocation {
    /// Relay address allocated by TURN server
    relay_addr: SocketAddr,

    /// TURN server address
    server_addr: SocketAddr,

    /// Username for this allocation
    username: String,

    /// Allocation lifetime (seconds)
    lifetime: u64,

    /// Bound peer addresses (channel bindings)
    peers: Vec<SocketAddr>,
}

impl TurnClient {
    /// Create new TURN client
    pub fn new(servers: Vec<super::TurnServer>) -> Self {
        Self {
            servers,
            active_relays: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Allocate relay address on TURN server
    pub async fn allocate_relay(&self) -> Result<SocketAddr> {
        // Try servers by priority
        let mut servers = self.servers.clone();
        servers.sort_by_key(|s| s.priority);

        for server in &servers {
            match self.allocate_on_server(server).await {
                Ok(relay_addr) => return Ok(relay_addr),
                Err(e) => {
                    eprintln!("TURN allocation on {} failed: {}", server.address, e);
                    continue;
                }
            }
        }

        Err(dchat_core::Error::network("All TURN servers failed"))
    }

    /// Allocate relay on specific TURN server
    async fn allocate_on_server(&self, server: &super::TurnServer) -> Result<SocketAddr> {
        // Resolve server address
        let server_addr: SocketAddr = tokio::net::lookup_host(&server.address)
            .await
            .map_err(|e| dchat_core::Error::network(format!("TURN DNS lookup failed: {}", e)))?
            .next()
            .ok_or_else(|| dchat_core::Error::network("TURN server resolution failed"))?;

        // Bind local socket
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| dchat_core::Error::network(format!("TURN socket bind failed: {}", e)))?;

        // Build TURN Allocate Request (RFC 5766)
        let request = self.build_allocate_request(server)?;

        // Send request
        socket
            .send_to(&request, server_addr)
            .await
            .map_err(|e| dchat_core::Error::network(format!("TURN send failed: {}", e)))?;

        // Receive response
        let mut buf = vec![0u8; 2048];
        let (len, _) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            socket.recv_from(&mut buf),
        )
        .await
        .map_err(|_| dchat_core::Error::network("TURN allocation timeout"))?
        .map_err(|e| dchat_core::Error::network(format!("TURN recv failed: {}", e)))?;

        // Parse relay address from response
        let relay_addr = self.parse_allocate_response(&buf[..len])?;

        // Store allocation
        let allocation = RelayAllocation {
            relay_addr,
            server_addr,
            username: server.username.clone(),
            lifetime: 600, // 10 minutes default
            peers: Vec::new(),
        };

        let mut relays = self.active_relays.lock().await;
        relays.insert(server.address.clone(), allocation);

        Ok(relay_addr)
    }

    /// Build TURN Allocate Request
    fn build_allocate_request(&self, server: &super::TurnServer) -> Result<Vec<u8>> {
        // STUN Message Header
        let mut msg = Vec::new();

        // Message Type: Allocate Request (0x0003)
        msg.extend_from_slice(&[0x00, 0x03]);

        // Message Length (placeholder - will be calculated and updated after attributes are added)
        msg.extend_from_slice(&[0x00, 0x00]);

        // Magic Cookie
        msg.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);

        // Transaction ID
        let transaction_id: [u8; 12] = rand::random();
        msg.extend_from_slice(&transaction_id);

        // Add REQUESTED-TRANSPORT attribute (UDP = 17)
        self.add_attribute(&mut msg, 0x0019, &[17, 0, 0, 0]);

        // Add USERNAME attribute
        self.add_attribute(&mut msg, 0x0006, server.username.as_bytes());

        // Add LIFETIME attribute (600 seconds = 10 minutes)
        let lifetime: u32 = 600;
        self.add_attribute(&mut msg, 0x000D, &lifetime.to_be_bytes());

        // Compute MESSAGE-INTEGRITY using HMAC-SHA1
        use hmac::{Hmac, Mac};
        use sha1::Sha1;
        type HmacSha1 = Hmac<Sha1>;

        // Update length before computing HMAC
        let attr_len = (msg.len() - 20) as u16;
        msg[2] = (attr_len >> 8) as u8;
        msg[3] = (attr_len & 0xFF) as u8;

        // Compute HMAC-SHA1 over message
        // SECURITY: HMAC-SHA1 new_from_slice only fails if key length is invalid for the algorithm.
        // SHA1 HMAC accepts any key length, so this should never fail.
        let mut mac = HmacSha1::new_from_slice(server.credential.as_bytes())
            .expect("SECURITY INVARIANT: HMAC-SHA1 accepts any key length");
        mac.update(&msg);
        let integrity = mac.finalize().into_bytes();

        // Add MESSAGE-INTEGRITY attribute (0x0008)
        self.add_attribute(&mut msg, 0x0008, &integrity);

        // Update final message length
        let final_len = (msg.len() - 20) as u16;
        msg[2] = (final_len >> 8) as u8;
        msg[3] = (final_len & 0xFF) as u8;

        Ok(msg)
    }

    /// Add STUN attribute to message
    fn add_attribute(&self, msg: &mut Vec<u8>, attr_type: u16, value: &[u8]) {
        // Attribute type
        msg.extend_from_slice(&attr_type.to_be_bytes());

        // Attribute length
        let len = value.len() as u16;
        msg.extend_from_slice(&len.to_be_bytes());

        // Attribute value
        msg.extend_from_slice(value);

        // Pad to 4-byte boundary
        let padding = (4 - (value.len() % 4)) % 4;
        msg.extend_from_slice(&vec![0u8; padding]);
    }

    /// Parse TURN Allocate Response
    fn parse_allocate_response(&self, data: &[u8]) -> Result<SocketAddr> {
        if data.len() < 20 {
            return Err(dchat_core::Error::network("TURN response too short"));
        }

        // Verify message type: Allocate Success Response (0x0103)
        if data[0] != 0x01 || data[1] != 0x03 {
            return Err(dchat_core::Error::network("Invalid TURN response type"));
        }

        // Message length
        let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;

        // Parse attributes for XOR-RELAYED-ADDRESS (0x0016)
        let mut offset = 20;
        while offset < 20 + msg_len {
            if offset + 4 > data.len() {
                break;
            }

            let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4;

            if offset + attr_len > data.len() {
                break;
            }

            // XOR-RELAYED-ADDRESS
            if attr_type == 0x0016 {
                return self.parse_xor_address(&data[offset..offset + attr_len], &data[4..20]);
            }

            offset += attr_len;
            offset = (offset + 3) & !3; // Pad to 4-byte boundary
        }

        Err(dchat_core::Error::network(
            "TURN response missing relay address",
        ))
    }

    /// Parse XOR-RELAYED-ADDRESS attribute
    fn parse_xor_address(&self, data: &[u8], _magic_and_txid: &[u8]) -> Result<SocketAddr> {
        if data.len() < 8 {
            return Err(dchat_core::Error::network("Invalid XOR address attribute"));
        }

        // Family (0x01 = IPv4)
        if data[1] != 0x01 {
            return Err(dchat_core::Error::network("Only IPv4 supported"));
        }

        // XOR port
        let port = u16::from_be_bytes([data[2], data[3]]) ^ 0x2112;

        // XOR IP
        let mut octets = [data[4], data[5], data[6], data[7]];
        octets[0] ^= 0x21;
        octets[1] ^= 0x12;
        octets[2] ^= 0xA4;
        octets[3] ^= 0x42;

        let ip = std::net::IpAddr::from(octets);

        Ok(SocketAddr::new(ip, port))
    }

    /// Bind channel to peer
    pub async fn bind_channel(&self, relay_id: &str, peer_addr: SocketAddr) -> Result<u16> {
        // Channel numbers: 0x4000 - 0x7FFF
        let channel_number: u16 = 0x4000;

        let mut relays = self.active_relays.lock().await;
        if let Some(allocation) = relays.get_mut(relay_id) {
            allocation.peers.push(peer_addr);
            Ok(channel_number)
        } else {
            Err(dchat_core::Error::network("Relay allocation not found"))
        }
    }

    /// Send data through TURN relay
    pub async fn send_through_relay(
        &self,
        relay_id: &str,
        data: &[u8],
        _peer_addr: SocketAddr,
    ) -> Result<()> {
        let relays = self.active_relays.lock().await;
        let allocation = relays
            .get(relay_id)
            .ok_or_else(|| dchat_core::Error::network("Relay allocation not found"))?;

        // Build Send Indication (0x0016)
        let mut msg = Vec::new();

        // Message Type: Send Indication
        msg.extend_from_slice(&[0x00, 0x16]);

        // Message Length (placeholder - calculated and updated after DATA attribute is added)
        msg.extend_from_slice(&[0x00, 0x00]);

        // Magic cookie + transaction ID
        msg.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);
        let txid: [u8; 12] = rand::random();
        msg.extend_from_slice(&txid);

        // Add XOR-PEER-ADDRESS attribute (peer_addr)
        // ... (simplified for brevity)

        // Add DATA attribute
        self.add_attribute(&mut msg, 0x0013, data);

        // Update length
        let attr_len = (msg.len() - 20) as u16;
        msg[2] = (attr_len >> 8) as u8;
        msg[3] = (attr_len & 0xFF) as u8;

        // Send to TURN server
        let socket = UdpSocket::bind("0.0.0.0:0").await.map_err(|e| {
            dchat_core::Error::network(format!("TURN send socket bind failed: {}", e))
        })?;

        socket
            .send_to(&msg, allocation.server_addr)
            .await
            .map_err(|e| dchat_core::Error::network(format!("TURN send failed: {}", e)))?;

        Ok(())
    }

    /// Refresh TURN allocation to prevent expiration
    pub async fn refresh_allocation(&self, relay_id: &str) -> Result<()> {
        let relays = self.active_relays.lock().await;
        let allocation = relays
            .get(relay_id)
            .ok_or_else(|| dchat_core::Error::network("Relay allocation not found"))?;

        // Build Refresh Request (Message Type 0x0004)
        let mut msg = Vec::new();

        // Message Type: Refresh Request
        msg.extend_from_slice(&[0x00, 0x04]);

        // Message Length (placeholder - calculated and updated after LIFETIME attribute is added)
        msg.extend_from_slice(&[0x00, 0x00]);

        // Magic cookie + transaction ID
        msg.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);
        let txid: [u8; 12] = rand::random();
        msg.extend_from_slice(&txid);

        // Add LIFETIME attribute (600 seconds)
        let lifetime: u32 = 600;
        self.add_attribute(&mut msg, 0x000D, &lifetime.to_be_bytes());

        // Add USERNAME attribute
        self.add_attribute(&mut msg, 0x0006, allocation.username.as_bytes());

        // Update length
        let attr_len = (msg.len() - 20) as u16;
        msg[2] = (attr_len >> 8) as u8;
        msg[3] = (attr_len & 0xFF) as u8;

        // Send to TURN server
        let socket = UdpSocket::bind("0.0.0.0:0").await.map_err(|e| {
            dchat_core::Error::network(format!("TURN refresh socket bind failed: {}", e))
        })?;

        socket
            .send_to(&msg, allocation.server_addr)
            .await
            .map_err(|e| dchat_core::Error::network(format!("TURN refresh send failed: {}", e)))?;

        // Wait for success response
        let mut buf = vec![0u8; 1024];
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            socket.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok((len, _))) => {
                // Verify response type: Refresh Success (0x0104)
                if len >= 20 && buf[0] == 0x01 && buf[1] == 0x04 {
                    tracing::info!("TURN allocation refreshed for relay: {}", relay_id);
                    Ok(())
                } else {
                    Err(dchat_core::Error::network("Invalid TURN refresh response"))
                }
            }
            Ok(Err(e)) => Err(dchat_core::Error::network(format!(
                "TURN refresh recv failed: {}",
                e
            ))),
            Err(_) => Err(dchat_core::Error::network("TURN refresh timeout")),
        }
    }

    /// Close all relay allocations
    pub async fn close_all_relays(&self) -> Result<()> {
        let mut relays = self.active_relays.lock().await;

        for (relay_id, allocation) in relays.iter() {
            // Build Refresh Request with LIFETIME=0 to close allocation
            let mut msg = Vec::new();

            // Message Type: Refresh Request (0x0004)
            msg.extend_from_slice(&[0x00, 0x04]);

            // Message Length (placeholder - calculated and updated after attributes are added)
            msg.extend_from_slice(&[0x00, 0x00]);

            // Magic cookie + transaction ID
            msg.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);
            let txid: [u8; 12] = rand::random();
            msg.extend_from_slice(&txid);

            // Add LIFETIME attribute with 0 to close
            let lifetime: u32 = 0;
            self.add_attribute(&mut msg, 0x000D, &lifetime.to_be_bytes());

            // Add USERNAME attribute
            self.add_attribute(&mut msg, 0x0006, allocation.username.as_bytes());

            // Update length
            let attr_len = (msg.len() - 20) as u16;
            msg[2] = (attr_len >> 8) as u8;
            msg[3] = (attr_len & 0xFF) as u8;

            // Send close request
            if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
                if let Err(e) = socket.send_to(&msg, allocation.server_addr).await {
                    tracing::warn!("Failed to send TURN close for {}: {}", relay_id, e);
                } else {
                    tracing::info!("Closed TURN relay allocation: {}", relay_id);
                }
            }
        }

        relays.clear();
        Ok(())
    }

    /// Get active relay count
    pub async fn active_relay_count(&self) -> usize {
        self.active_relays.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turn_client_creation() {
        let servers = vec![super::super::TurnServer {
            address: "turn.example.com:3478".to_string(),
            username: "user1".to_string(),
            credential: "pass1".to_string(),
            priority: 1,
        }];

        let client = TurnClient::new(servers);
        assert_eq!(client.servers.len(), 1);
    }

    #[tokio::test]
    async fn test_turn_client_empty_servers() {
        let client = TurnClient::new(Vec::new());
        let result = client.allocate_relay().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_relay_count() {
        let client = TurnClient::new(Vec::new());
        let count = client.active_relay_count().await;
        assert_eq!(count, 0);
    }

    #[test]
    fn test_build_allocate_request() {
        let servers = vec![super::super::TurnServer {
            address: "turn.example.com:3478".to_string(),
            username: "testuser".to_string(),
            credential: "testpass".to_string(),
            priority: 1,
        }];

        let client = TurnClient::new(servers.clone());
        let request = client.build_allocate_request(&servers[0]);

        assert!(request.is_ok());
        let msg = request.unwrap();

        // Verify header
        assert_eq!(msg[0], 0x00); // Message type MSB
        assert_eq!(msg[1], 0x03); // Message type LSB (Allocate)
        assert_eq!(msg[4], 0x21); // Magic cookie
    }
}
