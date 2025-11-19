//! NAT traversal implementation (UPnP and TURN fallback)
//!
//! Implements Section 12 (NAT Traversal) from ARCHITECTURE.md
//! - Automatic UPnP port mapping
//! - TURN server fallback for symmetric NATs
//! - Hole punching for direct P2P connections
//! - Eclipse attack prevention via relay diversity

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

/// NAT traversal strategy
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NatStrategy {
    /// Direct connection (no NAT)
    Direct,
    /// UPnP port mapping
    UPnP,
    /// TURN relay server
    TURN,
    /// Turn relay server (alternative name)
    Turn,
    /// Hole punching
    HolePunching,
}

/// NAT type detected
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NatType {
    /// No NAT (public IP)
    None,
    /// Open Internet (public IP, no NAT)
    Open,
    /// Full cone NAT (easy traversal)
    FullCone,
    /// Restricted cone NAT (moderate difficulty)
    RestrictedCone,
    /// Port-restricted cone NAT (difficult)
    PortRestrictedCone,
    /// Port-restricted NAT (alternative name)
    PortRestricted,
    /// Symmetric NAT (requires TURN)
    Symmetric,
    /// Unknown NAT type
    Unknown,
}

/// NAT traversal configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatConfig {
    /// Enable UPnP automatic port mapping
    pub enable_upnp: bool,
    /// STUN server addresses for NAT detection
    pub stun_servers: Vec<String>,
    /// TURN server addresses for relay
    pub turn_servers: Vec<String>,
    /// Enable hole punching
    pub enable_hole_punching: bool,
    /// Timeout for NAT detection
    pub detection_timeout_secs: u64,
    /// Port range for UPnP mapping
    pub upnp_port_range: (u16, u16),
}

impl Default for NatConfig {
    fn default() -> Self {
        Self {
            enable_upnp: true,
            stun_servers: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            turn_servers: vec![
                "turn:relay1.dchat.network:3478".to_string(),
                "turn:relay2.dchat.network:3478".to_string(),
            ],
            enable_hole_punching: true,
            detection_timeout_secs: 10,
            upnp_port_range: (49152, 65535), // Dynamic/private ports
        }
    }
}

/// NAT traversal manager
pub struct NatTraversalManager {
    config: NatConfig,
    detected_nat_type: Option<NatType>,
    active_strategy: Option<NatStrategy>,
    upnp_gateway: Option<UpnpGateway>,
    turn_connections: Vec<TurnConnection>,
}

/// UPnP gateway information
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct UpnpGateway {
    gateway_addr: SocketAddr,
    external_ip: IpAddr,
    mapped_port: u16,
    internal_port: u16,
}

/// TURN server connection
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TurnConnection {
    server_addr: String,
    allocated_addr: Option<SocketAddr>,
    username: String,
    credential: String,
}

impl NatTraversalManager {
    /// Create a new NAT traversal manager
    pub fn new(config: NatConfig) -> Self {
        Self {
            config,
            detected_nat_type: None,
            active_strategy: None,
            upnp_gateway: None,
            turn_connections: Vec::new(),
        }
    }

    /// Detect NAT type using STUN protocol (RFC 5389)
    pub async fn detect_nat_type(&mut self) -> Result<NatType> {
        use std::time::Duration;
        use tokio::net::UdpSocket;
        use tokio::time::timeout;

        // Parse STUN server addresses
        let primary_stun: SocketAddr = self
            .config
            .stun_servers
            .first()
            .ok_or_else(|| Error::network("No STUN servers configured"))?
            .parse()
            .map_err(|_| Error::network("Invalid STUN server address"))?;

        let secondary_stun: SocketAddr = self
            .config
            .stun_servers
            .get(1)
            .unwrap_or(&self.config.stun_servers[0])
            .parse()
            .map_err(|_| Error::network("Invalid secondary STUN server address"))?;

        // Bind local UDP socket
        let local_socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| Error::network(format!("Failed to bind socket: {}", e)))?;
        let local_addr = local_socket
            .local_addr()
            .map_err(|e| Error::network(format!("Failed to get local address: {}", e)))?;

        // Test 1: Send STUN binding request to primary server
        let stun_request = self.build_stun_binding_request();
        local_socket
            .send_to(&stun_request, primary_stun)
            .await
            .map_err(|e| Error::network(format!("STUN send failed: {}", e)))?;

        let mut buf = vec![0u8; 1024];
        let response1 = timeout(Duration::from_secs(5), local_socket.recv_from(&mut buf))
            .await
            .map_err(|_| Error::network("STUN request timeout"))??
            .0;

        let mapped_addr1 = self.parse_stun_response(&buf[..response1])?;

        // Compare local and mapped addresses
        let nat_type = if mapped_addr1 == local_addr {
            // Public IP, no NAT
            NatType::Open
        } else {
            // Behind NAT - need more tests
            // Test 2: Request from different port on same server
            local_socket
                .send_to(&stun_request, secondary_stun)
                .await
                .map_err(|e| Error::network(format!("Secondary STUN send failed: {}", e)))?;

            let response2 = timeout(Duration::from_secs(5), local_socket.recv_from(&mut buf))
                .await
                .map_err(|_| Error::network("Secondary STUN timeout"))??
                .0;

            let mapped_addr2 = self.parse_stun_response(&buf[..response2])?;

            if mapped_addr1.port() == mapped_addr2.port() && mapped_addr1.ip() == mapped_addr2.ip()
            {
                // Same mapping from different servers
                NatType::FullCone
            } else if mapped_addr1.ip() == mapped_addr2.ip() {
                // Same IP, different port
                NatType::PortRestricted
            } else {
                // Different IP and port
                NatType::Symmetric
            }
        };

        self.detected_nat_type = Some(nat_type.clone());
        Ok(nat_type)
    }

    fn build_stun_binding_request(&self) -> Vec<u8> {
        // STUN Binding Request format (RFC 5389)
        let mut request = Vec::new();

        // Message Type: 0x0001 (Binding Request)
        request.extend_from_slice(&[0x00, 0x01]);

        // Message Length: 0 (no attributes for basic request)
        request.extend_from_slice(&[0x00, 0x00]);

        // Magic Cookie: 0x2112A442
        request.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);

        // Transaction ID: 96-bit random
        let mut transaction_id = [0u8; 12];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut transaction_id);
        request.extend_from_slice(&transaction_id);

        request
    }

    fn parse_stun_response(&self, data: &[u8]) -> Result<SocketAddr> {
        if data.len() < 20 {
            return Err(Error::network("Invalid STUN response: too short"));
        }

        // Check magic cookie
        if data[4..8] != [0x21, 0x12, 0xA4, 0x42] {
            return Err(Error::network("Invalid STUN response: bad magic cookie"));
        }

        // Parse attributes to find XOR-MAPPED-ADDRESS (0x0020)
        let mut offset = 20;
        let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;

        while offset < 20 + msg_len {
            if offset + 4 > data.len() {
                break;
            }

            let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;

            if attr_type == 0x0020 && offset + 4 + attr_len <= data.len() {
                // XOR-MAPPED-ADDRESS found
                let family = data[offset + 5];
                let port_xor = u16::from_be_bytes([data[offset + 6], data[offset + 7]]);
                let port = port_xor ^ 0x2112;

                if family == 0x01 {
                    // IPv4
                    let ip_xor = u32::from_be_bytes([
                        data[offset + 8],
                        data[offset + 9],
                        data[offset + 10],
                        data[offset + 11],
                    ]);
                    let ip = ip_xor ^ 0x2112A442;
                    let ip_bytes = ip.to_be_bytes();

                    return Ok(SocketAddr::new(
                        std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                            ip_bytes[0],
                            ip_bytes[1],
                            ip_bytes[2],
                            ip_bytes[3],
                        )),
                        port,
                    ));
                }
            }

            offset += 4 + attr_len;
            // Align to 4-byte boundary
            offset = (offset + 3) & !3;
        }

        Err(Error::network(
            "XOR-MAPPED-ADDRESS not found in STUN response",
        ))
    }

    /// Attempt UPnP port mapping using IGD protocol
    pub async fn setup_upnp(&mut self, internal_port: u16) -> Result<SocketAddr> {
        if !self.config.enable_upnp {
            return Err(Error::network("UPnP is disabled"));
        }

        // 1. Discover UPnP gateway using SSDP
        let gateway_addr = self.discover_upnp_gateway().await?;

        // 2. Get gateway's external IP address
        let external_ip = self.get_upnp_external_ip(&gateway_addr).await?;

        // 3. Request port mapping
        let external_port = self
            .request_upnp_port_mapping(
                &gateway_addr,
                internal_port,
                internal_port, // Try to map to same port externally
                "TCP",
                3600, // Lease duration: 1 hour
            )
            .await?;

        let gateway = UpnpGateway {
            gateway_addr,
            external_ip,
            internal_port,
            mapped_port: external_port,
        };

        let external_addr = SocketAddr::new(gateway.external_ip, gateway.mapped_port);
        self.upnp_gateway = Some(gateway);
        self.active_strategy = Some(NatStrategy::UPnP);

        Ok(external_addr)
    }

    async fn discover_upnp_gateway(&self) -> Result<SocketAddr> {
        use std::time::Duration;
        use tokio::net::UdpSocket;
        use tokio::time::timeout;

        // SSDP discovery multicast
        let ssdp_addr: SocketAddr = "239.255.255.250:1900".parse().unwrap();
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| Error::network(format!("Failed to bind for SSDP: {}", e)))?;

        socket
            .set_broadcast(true)
            .map_err(|e| Error::network(format!("Failed to set broadcast: {}", e)))?;

        // SSDP M-SEARCH request
        let search_request = "M-SEARCH * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: 3\r\n\
             ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
             \r\n"
            .to_string();

        socket
            .send_to(search_request.as_bytes(), ssdp_addr)
            .await
            .map_err(|e| Error::network(format!("SSDP send failed: {}", e)))?;

        // Wait for response
        let mut buf = vec![0u8; 2048];
        let (len, _) = timeout(Duration::from_secs(5), socket.recv_from(&mut buf))
            .await
            .map_err(|_| Error::network("UPnP gateway discovery timeout"))??;

        let response = String::from_utf8_lossy(&buf[..len]);

        // Parse LOCATION header to get gateway control URL
        for line in response.lines() {
            if line.to_lowercase().starts_with("location:") {
                let url = line
                    .split_whitespace()
                    .nth(1)
                    .ok_or_else(|| Error::network("Invalid LOCATION header"))?;

                // Extract host:port from URL
                if let Some(host_start) = url.find("://") {
                    let host_part = &url[host_start + 3..];
                    if let Some(host_end) = host_part.find('/') {
                        let host_port = &host_part[..host_end];
                        return host_port
                            .parse()
                            .map_err(|_| Error::network("Invalid gateway address"));
                    }
                }
            }
        }

        Err(Error::network("Gateway address not found in SSDP response"))
    }

    async fn get_upnp_external_ip(&self, gateway: &SocketAddr) -> Result<std::net::IpAddr> {
        // Send SOAP request to get external IP
        let soap_request = "<?xml version=\"1.0\"?>\n\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
                         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\n\
               <s:Body>\n\
                 <u:GetExternalIPAddress xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\"/>\n\
               </s:Body>\n\
             </s:Envelope>";

        let control_url = format!("http://{}/ctl/IPConn", gateway);

        let client = reqwest::Client::new();
        let response = client
            .post(&control_url)
            .header("SOAPAction", "\"urn:schemas-upnp-org:service:WANIPConnection:1#GetExternalIPAddress\"")
            .header("Content-Type", "text/xml; charset=\"utf-8\"")
            .body(soap_request)
            .send()
            .await
            .map_err(|e| Error::network(format!("UPnP SOAP request failed: {}", e)))?;

        let response_text = response
            .text()
            .await
            .map_err(|e| Error::network(format!("Failed to read UPnP response: {}", e)))?;

        // Parse XML response to extract external IP
        use quick_xml::de::from_str;
        use serde::Deserialize;

        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct Envelope {
            #[serde(rename = "Body")]
            body: Body,
        }

        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct Body {
            #[serde(rename = "GetExternalIPAddressResponse")]
            response: GetExternalIPAddressResponse,
        }

        #[derive(Deserialize, Debug)]
        #[allow(dead_code)]
        struct GetExternalIPAddressResponse {
            #[serde(rename = "NewExternalIPAddress")]
            external_ip: String,
        }

        let envelope: Envelope = from_str(&response_text)
            .map_err(|e| Error::network(format!("Failed to parse UPnP response: {}", e)))?;

        envelope
            .body
            .response
            .external_ip
            .parse()
            .map_err(|_| Error::network("Invalid IP address in UPnP response"))
    }

    async fn request_upnp_port_mapping(
        &self,
        gateway: &SocketAddr,
        internal_port: u16,
        external_port: u16,
        protocol: &str,
        lease_duration: u32,
    ) -> Result<u16> {
        // Get local IP address
        let local_ip = local_ip_address::local_ip()
            .map_err(|e| Error::network(format!("Failed to get local IP: {}", e)))?;

        // Send SOAP AddPortMapping request
        let soap_request = format!(
            "<?xml version=\"1.0\"?>\n\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
                         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\n\
               <s:Body>\n\
                 <u:AddPortMapping xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\n\
                   <NewRemoteHost></NewRemoteHost>\n\
                   <NewExternalPort>{}</NewExternalPort>\n\
                   <NewProtocol>{}</NewProtocol>\n\
                   <NewInternalPort>{}</NewInternalPort>\n\
                   <NewInternalClient>{}</NewInternalClient>\n\
                   <NewEnabled>1</NewEnabled>\n\
                   <NewPortMappingDescription>dchat P2P</NewPortMappingDescription>\n\
                   <NewLeaseDuration>{}</NewLeaseDuration>\n\
                 </u:AddPortMapping>\n\
               </s:Body>\n\
             </s:Envelope>",
            external_port, protocol, internal_port, local_ip, lease_duration
        );

        let control_url = format!("http://{}/ctl/IPConn", gateway);

        let client = reqwest::Client::new();
        let response = client
            .post(&control_url)
            .header("SOAPAction", "\"urn:schemas-upnp-org:service:WANIPConnection:1#AddPortMapping\"")
            .header("Content-Type", "text/xml; charset=\"utf-8\"")
            .body(soap_request)
            .send()
            .await
            .map_err(|e| Error::network(format!("UPnP AddPortMapping request failed: {}", e)))?;

        if response.status().is_success() {
            Ok(external_port)
        } else {
            Err(Error::network(format!(
                "UPnP port mapping failed with status: {}",
                response.status()
            )))
        }
    }

    /// Setup TURN relay connection (RFC 5766)
    pub async fn setup_turn(&mut self, username: String, credential: String) -> Result<SocketAddr> {
        use std::time::Duration;
        use tokio::net::UdpSocket;
        use tokio::time::timeout;

        if self.config.turn_servers.is_empty() {
            return Err(Error::network("No TURN servers configured"));
        }

        let turn_server: SocketAddr = self.config.turn_servers[0]
            .parse()
            .map_err(|_| Error::network("Invalid TURN server address"))?;

        let local_socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| Error::network(format!("Failed to bind socket: {}", e)))?;

        // 1. Send TURN Allocate Request
        let allocate_request = self.build_turn_allocate_request(&username, &credential)?;
        local_socket
            .send_to(&allocate_request, turn_server)
            .await
            .map_err(|e| Error::network(format!("TURN send failed: {}", e)))?;

        // 2. Receive Allocate Response
        let mut buf = vec![0u8; 2048];
        let (len, _) = timeout(Duration::from_secs(10), local_socket.recv_from(&mut buf))
            .await
            .map_err(|_| Error::network("TURN allocate timeout"))??;

        // 3. Parse allocated relay address
        let relay_addr = self.parse_turn_allocate_response(&buf[..len])?;

        self.active_strategy = Some(NatStrategy::Turn);

        Ok(relay_addr)
    }

    fn build_turn_allocate_request(&self, username: &str, credential: &str) -> Result<Vec<u8>> {
        // TURN Allocate Request (RFC 5766)
        let mut request = Vec::new();

        // Message Type: 0x0003 (Allocate Request)
        request.extend_from_slice(&[0x00, 0x03]);

        // Message Length (placeholder - calculated at end after all attributes are added)
        let length_pos = request.len();
        request.extend_from_slice(&[0x00, 0x00]);

        // Magic Cookie
        request.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);

        // Transaction ID
        let mut transaction_id = [0u8; 12];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut transaction_id);
        request.extend_from_slice(&transaction_id);

        // Add REQUESTED-TRANSPORT attribute (0x0019)
        // UDP = 17
        request.extend_from_slice(&[0x00, 0x19]); // Type
        request.extend_from_slice(&[0x00, 0x04]); // Length
        request.extend_from_slice(&[17, 0x00, 0x00, 0x00]); // UDP + padding

        // Add USERNAME attribute (0x0006)
        let username_bytes = username.as_bytes();
        request.extend_from_slice(&[0x00, 0x06]); // Type
        request.extend_from_slice(&(username_bytes.len() as u16).to_be_bytes()); // Length
        request.extend_from_slice(username_bytes);
        // Padding to 4-byte boundary
        let padding = (4 - (username_bytes.len() % 4)) % 4;
        request.extend_from_slice(&vec![0u8; padding]);

        // Add MESSAGE-INTEGRITY attribute (0x0008)
        // HMAC-SHA1 of message with credential
        use sha1::{Digest, Sha1};
        let mut hmac_key = Sha1::new();
        hmac_key.update(credential.as_bytes());
        let key = hmac_key.finalize();

        // Calculate HMAC over message so far
        use hmac::{Hmac, Mac};
        type HmacSha1 = Hmac<Sha1>;
        let mut mac =
            HmacSha1::new_from_slice(&key).map_err(|_| Error::network("Failed to create HMAC"))?;
        mac.update(&request);
        let message_integrity = mac.finalize().into_bytes();

        request.extend_from_slice(&[0x00, 0x08]); // Type
        request.extend_from_slice(&[0x00, 0x14]); // Length (20 bytes for SHA1)
        request.extend_from_slice(&message_integrity);

        // Update message length
        let msg_length = (request.len() - 20) as u16;
        request[length_pos..length_pos + 2].copy_from_slice(&msg_length.to_be_bytes());

        Ok(request)
    }

    fn parse_turn_allocate_response(&self, data: &[u8]) -> Result<SocketAddr> {
        if data.len() < 20 {
            return Err(Error::network("Invalid TURN response: too short"));
        }

        // Check for error response (0x0113)
        let msg_type = u16::from_be_bytes([data[0], data[1]]);
        if msg_type == 0x0113 {
            return Err(Error::network("TURN allocation failed - check credentials"));
        }

        // Parse XOR-RELAYED-ADDRESS attribute (0x0016)
        let mut offset = 20;
        let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;

        while offset < 20 + msg_len {
            if offset + 4 > data.len() {
                break;
            }

            let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;

            if attr_type == 0x0016 && offset + 4 + attr_len <= data.len() {
                // XOR-RELAYED-ADDRESS found
                let family = data[offset + 5];
                let port_xor = u16::from_be_bytes([data[offset + 6], data[offset + 7]]);
                let port = port_xor ^ 0x2112;

                if family == 0x01 {
                    // IPv4
                    let ip_xor = u32::from_be_bytes([
                        data[offset + 8],
                        data[offset + 9],
                        data[offset + 10],
                        data[offset + 11],
                    ]);
                    let ip = ip_xor ^ 0x2112A442;
                    let ip_bytes = ip.to_be_bytes();

                    return Ok(SocketAddr::new(
                        std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                            ip_bytes[0],
                            ip_bytes[1],
                            ip_bytes[2],
                            ip_bytes[3],
                        )),
                        port,
                    ));
                }
            }

            offset += 4 + attr_len;
            // Align to 4-byte boundary
            offset = (offset + 3) & !3;
        }

        Err(Error::network(
            "XOR-RELAYED-ADDRESS not found in TURN response",
        ))
    }

    /// Setup TURN relay connection (legacy interface)
    pub async fn _setup_turn_legacy(
        &mut self,
        username: String,
        credential: String,
    ) -> Result<SocketAddr> {
        if self.config.turn_servers.is_empty() {
            return Err(Error::network("No TURN servers configured"));
        }

        // Select TURN server (could use load balancing or latency-based selection)
        let server_addr = self.config.turn_servers[0].clone();

        // Implementation would establish TURN connection
        // Steps:
        // 1. Connect to TURN server
        // 2. Authenticate with credentials
        // 3. Allocate relay address
        // 4. Maintain connection with keep-alives

        // Real TURN allocation using the new setup_turn method
        let allocated_addr = self
            .setup_turn(username.clone(), credential.clone())
            .await?;

        let turn_conn = TurnConnection {
            server_addr: server_addr.clone(),
            allocated_addr: Some(allocated_addr),
            username,
            credential,
        };

        self.turn_connections.push(turn_conn);
        self.active_strategy = Some(NatStrategy::TURN);

        Ok(allocated_addr)
    }

    /// Attempt hole punching with remote peer
    pub async fn attempt_hole_punching(
        &mut self,
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
    ) -> Result<bool> {
        if !self.config.enable_hole_punching {
            return Err(Error::network("Hole punching is disabled"));
        }

        use std::time::Duration;
        use tokio::net::UdpSocket;
        use tokio::time::{sleep, timeout};

        // Bind to local address
        let socket = UdpSocket::bind(local_addr)
            .await
            .map_err(|e| Error::network(format!("Failed to bind for hole punching: {}", e)))?;

        // Simultaneous packet sending - both peers do this
        // Send multiple packets to create NAT binding
        let punch_packet = b"DCHAT_HOLE_PUNCH";
        
        for attempt in 0..5 {
            // Send punch packet to remote's public address
            socket
                .send_to(punch_packet, remote_addr)
                .await
                .map_err(|e| Error::network(format!("Hole punch send failed: {}", e)))?;

            // Wait with exponential backoff
            sleep(Duration::from_millis(200 * (1 << attempt))).await;

            // Try to receive response (non-blocking)
            let mut buf = [0u8; 1024];
            match timeout(Duration::from_millis(500), socket.recv_from(&mut buf)).await {
                Ok(Ok((len, addr))) => {
                    if addr == remote_addr && &buf[..len] == punch_packet {
                        // Successfully established bidirectional connection
                        self.active_strategy = Some(NatStrategy::HolePunching);
                        return Ok(true);
                    }
                }
                _ => continue, // Timeout or error, try again
            }
        }

        // Hole punching failed after retries
        Ok(false)
    }

    /// Get recommended strategy for current NAT type
    pub fn get_recommended_strategy(&self) -> NatStrategy {
        match self.detected_nat_type {
            Some(NatType::None) => NatStrategy::Direct,
            Some(NatType::FullCone) | Some(NatType::RestrictedCone) => NatStrategy::UPnP,
            Some(NatType::PortRestrictedCone) => NatStrategy::HolePunching,
            Some(NatType::Symmetric) => NatStrategy::TURN,
            _ => NatStrategy::TURN, // Default to TURN for unknown
        }
    }

    /// Establish connection using best available strategy
    pub async fn establish_connection(
        &mut self,
        local_port: u16,
        remote_addr: Option<SocketAddr>,
    ) -> Result<SocketAddr> {
        // Detect NAT type if not already done
        if self.detected_nat_type.is_none() {
            self.detect_nat_type().await?;
        }

        let strategy = self.get_recommended_strategy();

        match strategy {
            NatStrategy::Direct => {
                // Use local address directly
                Ok(SocketAddr::new("0.0.0.0".parse().unwrap(), local_port))
            }
            NatStrategy::UPnP => self.setup_upnp(local_port).await,
            NatStrategy::HolePunching => {
                if let Some(remote) = remote_addr {
                    let local = SocketAddr::new("0.0.0.0".parse().unwrap(), local_port);
                    if self.attempt_hole_punching(local, remote).await? {
                        return Ok(local);
                    }
                }
                // Fallback to TURN if hole punching fails
                self.setup_turn("user".to_string(), "pass".to_string())
                    .await
            }
            NatStrategy::TURN | NatStrategy::Turn => {
                self.setup_turn("user".to_string(), "pass".to_string())
                    .await
            }
        }
    }

    /// Release UPnP port mapping
    pub async fn release_upnp(&mut self) -> Result<()> {
        if let Some(gateway) = &self.upnp_gateway {
            // Send SOAP DeletePortMapping request
            let soap_request = format!(
                "<?xml version=\"1.0\"?>\n\
                 <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
                             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\n\
                   <s:Body>\n\
                     <u:DeletePortMapping xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\n\
                       <NewRemoteHost></NewRemoteHost>\n\
                       <NewExternalPort>{}</NewExternalPort>\n\
                       <NewProtocol>UDP</NewProtocol>\n\
                     </u:DeletePortMapping>\n\
                   </s:Body>\n\
                 </s:Envelope>",
                gateway.mapped_port
            );

            let control_url = format!("http://{}/ctl/IPConn", gateway.gateway_addr);

            let client = reqwest::Client::new();
            let _response = client
                .post(&control_url)
                .header("SOAPAction", "\"urn:schemas-upnp-org:service:WANIPConnection:1#DeletePortMapping\"")
                .header("Content-Type", "text/xml; charset=\"utf-8\"")
                .body(soap_request)
                .send()
                .await
                .map_err(|e| Error::network(format!("UPnP DeletePortMapping request failed: {}", e)))?;

            self.upnp_gateway = None;
        }
        Ok(())
    }

    /// Close TURN connections
    pub async fn close_turn_connections(&mut self) -> Result<()> {
        use tokio::net::UdpSocket;

        // Send Refresh request with lifetime=0 to each TURN server
        for conn in &self.turn_connections {
            if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
                // Build TURN Refresh request with lifetime=0
                let mut refresh_request = Vec::new();

                // Message Type: 0x0004 (Refresh Request)
                refresh_request.extend_from_slice(&[0x00, 0x04]);

                // Message Length (placeholder)
                let length_pos = refresh_request.len();
                refresh_request.extend_from_slice(&[0x00, 0x00]);

                // Magic Cookie
                refresh_request.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);

                // Transaction ID
                let mut transaction_id = [0u8; 12];
                use rand::RngCore;
                rand::thread_rng().fill_bytes(&mut transaction_id);
                refresh_request.extend_from_slice(&transaction_id);

                // Add LIFETIME attribute (0x000D) with value 0
                refresh_request.extend_from_slice(&[0x00, 0x0D]); // Type
                refresh_request.extend_from_slice(&[0x00, 0x04]); // Length
                refresh_request.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Lifetime = 0

                // Update message length
                let msg_length = (refresh_request.len() - 20) as u16;
                refresh_request[length_pos..length_pos + 2]
                    .copy_from_slice(&msg_length.to_be_bytes());

                // Send refresh to TURN server (ignore errors during cleanup)
                if let Ok(server_addr) = conn.server_addr.parse::<SocketAddr>() {
                    let _ = socket.send_to(&refresh_request, server_addr).await;
                }
            }
        }

        self.turn_connections.clear();
        Ok(())
    }

    /// Get current external address
    pub fn get_external_address(&self) -> Option<SocketAddr> {
        match &self.active_strategy {
            Some(NatStrategy::UPnP) => self
                .upnp_gateway
                .as_ref()
                .map(|gw| SocketAddr::new(gw.external_ip, gw.mapped_port)),
            Some(NatStrategy::TURN) => self
                .turn_connections
                .first()
                .and_then(|conn| conn.allocated_addr),
            _ => None,
        }
    }

    /// Cleanup all NAT traversal resources
    pub async fn cleanup(&mut self) -> Result<()> {
        self.release_upnp().await?;
        self.close_turn_connections().await?;
        self.active_strategy = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_nat_config_default() {
        let config = NatConfig::default();
        assert!(config.enable_upnp);
        assert!(config.enable_hole_punching);
        assert_eq!(config.turn_servers.len(), 2);
    }

    #[tokio::test]
    async fn test_recommended_strategy() {
        let config = NatConfig::default();
        let mut manager = NatTraversalManager::new(config);

        // Test different NAT types
        manager.detected_nat_type = Some(NatType::None);
        assert_eq!(manager.get_recommended_strategy(), NatStrategy::Direct);

        manager.detected_nat_type = Some(NatType::FullCone);
        assert_eq!(manager.get_recommended_strategy(), NatStrategy::UPnP);

        manager.detected_nat_type = Some(NatType::Symmetric);
        assert_eq!(manager.get_recommended_strategy(), NatStrategy::TURN);
    }

    #[tokio::test]
    async fn test_nat_manager_creation() {
        let config = NatConfig::default();
        let manager = NatTraversalManager::new(config);

        assert!(manager.detected_nat_type.is_none());
        assert!(manager.active_strategy.is_none());
        assert!(manager.upnp_gateway.is_none());
        assert_eq!(manager.turn_connections.len(), 0);
    }
}
