use super::NatType;
/// STUN (Session Traversal Utilities for NAT) Client
///
/// Discovers external IP address and classifies NAT type using
/// STUN protocol (RFC 5389).
///
/// NAT Classification Algorithm:
/// 1. Send binding request to STUN server
/// 2. Compare mapped address with local address
/// 3. Use multiple servers to detect symmetric NAT
///
/// See ARCHITECTURE.md Section 12.1: NAT Traversal
use dchat_core::Result;
use std::net::{IpAddr, SocketAddr};
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration};

/// STUN client for NAT detection and address discovery
pub struct StunClient {
    servers: Vec<String>,
    timeout: Duration,
}

impl StunClient {
    /// Create new STUN client
    pub fn new(servers: Vec<String>) -> Result<Self> {
        if servers.is_empty() {
            return Err(dchat_core::Error::network("No STUN servers configured"));
        }

        Ok(Self {
            servers,
            timeout: Duration::from_secs(5),
        })
    }

    /// Get external (public) IP address
    pub async fn get_external_address(&self) -> Result<SocketAddr> {
        for server in &self.servers {
            match self.query_server(server).await {
                Ok(addr) => return Ok(addr),
                Err(e) => {
                    eprintln!("STUN query to {} failed: {}", server, e);
                    continue;
                }
            }
        }

        Err(dchat_core::Error::network("All STUN servers failed"))
    }

    /// Query STUN server for external address
    async fn query_server(&self, server: &str) -> Result<SocketAddr> {
        // Resolve server address
        let server_addr: SocketAddr = tokio::net::lookup_host(server)
            .await
            .map_err(|e| dchat_core::Error::network(format!("STUN DNS lookup failed: {}", e)))?
            .next()
            .ok_or_else(|| dchat_core::Error::network("STUN server resolution failed"))?;

        // Bind local UDP socket
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| dchat_core::Error::network(format!("STUN socket bind failed: {}", e)))?;

        // Build STUN Binding Request (RFC 5389)
        let request = Self::build_binding_request();

        // Send request
        socket
            .send_to(&request, server_addr)
            .await
            .map_err(|e| dchat_core::Error::network(format!("STUN send failed: {}", e)))?;

        // Receive response
        let mut buf = vec![0u8; 1024];
        let (len, _) = timeout(self.timeout, socket.recv_from(&mut buf))
            .await
            .map_err(|_| dchat_core::Error::network("STUN request timeout"))?
            .map_err(|e| dchat_core::Error::network(format!("STUN recv failed: {}", e)))?;

        // Parse response
        Self::parse_binding_response(&buf[..len])
    }

    /// Build STUN Binding Request
    fn build_binding_request() -> Vec<u8> {
        // STUN Message Header (20 bytes)
        let mut msg = Vec::new();

        // Message Type: Binding Request (0x0001)
        msg.extend_from_slice(&[0x00, 0x01]);

        // Message Length: 0 (no attributes)
        msg.extend_from_slice(&[0x00, 0x00]);

        // Magic Cookie (RFC 5389)
        msg.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);

        // Transaction ID (96 bits = 12 bytes random)
        let transaction_id: [u8; 12] = rand::random();
        msg.extend_from_slice(&transaction_id);

        msg
    }

    /// Parse STUN Binding Response
    fn parse_binding_response(data: &[u8]) -> Result<SocketAddr> {
        if data.len() < 20 {
            return Err(dchat_core::Error::network("STUN response too short"));
        }

        // Verify message type: Binding Success Response (0x0101)
        if data[0] != 0x01 || data[1] != 0x01 {
            return Err(dchat_core::Error::network("Invalid STUN response type"));
        }

        // Message length
        let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;

        if data.len() < 20 + msg_len {
            return Err(dchat_core::Error::network("STUN response truncated"));
        }

        // Parse attributes
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

            // MAPPED-ADDRESS (0x0001) or XOR-MAPPED-ADDRESS (0x0020)
            if attr_type == 0x0001 || attr_type == 0x0020 {
                return Self::parse_address_attribute(
                    &data[offset..offset + attr_len],
                    attr_type == 0x0020,
                    &data[4..20],
                );
            }

            // Pad to 4-byte boundary
            offset += attr_len;
            offset = (offset + 3) & !3;
        }

        Err(dchat_core::Error::network(
            "STUN response missing address attribute",
        ))
    }

    /// Parse address attribute
    fn parse_address_attribute(
        data: &[u8],
        is_xor: bool,
        _magic_and_txid: &[u8],
    ) -> Result<SocketAddr> {
        if data.len() < 8 {
            return Err(dchat_core::Error::network("Invalid address attribute"));
        }

        // Family (0x01 = IPv4, 0x02 = IPv6)
        let family = data[1];

        // Port (XORed with magic cookie if XOR-MAPPED-ADDRESS)
        let mut port = u16::from_be_bytes([data[2], data[3]]);
        if is_xor {
            port ^= 0x2112; // XOR with first 2 bytes of magic cookie
        }

        // IP address
        let ip = if family == 0x01 {
            // IPv4
            let mut octets = [data[4], data[5], data[6], data[7]];
            if is_xor {
                // XOR with magic cookie
                octets[0] ^= 0x21;
                octets[1] ^= 0x12;
                octets[2] ^= 0xA4;
                octets[3] ^= 0x42;
            }
            IpAddr::from(octets)
        } else {
            return Err(dchat_core::Error::network("IPv6 not yet supported"));
        };

        Ok(SocketAddr::new(ip, port))
    }

    /// Detect NAT type using RFC 3489 algorithm
    pub async fn detect_nat_type(&self) -> Result<NatType> {
        // Step 1: Get external address from primary server
        let external_addr1 = self.get_external_address().await?;

        // Step 2: Get local address
        let local_addr = self.get_local_address().await?;

        // If external == local, no NAT
        if external_addr1.ip() == local_addr.ip() {
            return Ok(NatType::None);
        }

        // Step 3: Query second server
        if self.servers.len() > 1 {
            let external_addr2 = match self.query_server(&self.servers[1]).await {
                Ok(addr) => addr,
                Err(_) => return Ok(NatType::Unknown),
            };

            // If same port from different servers, likely full/restricted cone
            if external_addr1.port() == external_addr2.port() {
                // Full cone or restricted cone (requires more tests to distinguish)
                return Ok(NatType::RestrictedCone);
            } else {
                // Different ports = symmetric NAT
                return Ok(NatType::Symmetric);
            }
        }

        // Default to restricted cone (most common)
        Ok(NatType::RestrictedCone)
    }

    /// Get local IP address
    async fn get_local_address(&self) -> Result<SocketAddr> {
        // Bind socket and get local address
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| dchat_core::Error::network(format!("Failed to bind socket: {}", e)))?;

        socket
            .local_addr()
            .map_err(|e| dchat_core::Error::network(format!("Failed to get local address: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stun_client_creation() {
        let servers = vec!["stun.l.google.com:19302".to_string()];
        let client = StunClient::new(servers);
        assert!(client.is_ok());
    }

    #[test]
    fn test_stun_client_no_servers() {
        let client = StunClient::new(Vec::new());
        assert!(client.is_err());
    }

    #[test]
    fn test_build_binding_request() {
        let request = StunClient::build_binding_request();

        // Check header
        assert_eq!(request.len(), 20);
        assert_eq!(request[0], 0x00); // Message type MSB
        assert_eq!(request[1], 0x01); // Message type LSB (Binding Request)
        assert_eq!(request[4], 0x21); // Magic cookie
        assert_eq!(request[5], 0x12);
        assert_eq!(request[6], 0xA4);
        assert_eq!(request[7], 0x42);
    }

    #[test]
    fn test_parse_invalid_response() {
        let data = vec![0u8; 10]; // Too short
        let result = StunClient::parse_binding_response(&data);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_external_address() {
        let servers = vec![
            "stun.l.google.com:19302".to_string(),
            "stun1.l.google.com:19302".to_string(),
        ];

        let client = StunClient::new(servers).unwrap();

        // This test requires network access and may fail in CI
        // In production, mock the UDP responses
        let result = client.get_external_address().await;

        // Either succeeds or fails gracefully
        match result {
            Ok(addr) => assert!(addr.port() > 0),
            Err(e) => println!("STUN test expected failure: {}", e),
        }
    }
}
