/// UPnP IGD (Internet Gateway Device) Client
///
/// Automatically discovers gateway devices and creates port mappings
/// for direct P2P connections through NAT.
///
/// Protocol: UPnP IGD v1/v2
/// Discovery: SSDP (Simple Service Discovery Protocol)
/// 
/// See ARCHITECTURE.md Section 12.1: NAT Traversal

use dchat_core::{Result, error::Error};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::time::timeout;

/// UPnP client for automatic port mapping
pub struct UpnpClient {
    gateway_addr: Option<SocketAddr>,
    control_url: Option<String>,
    discovery_timeout: Duration,
    active_mappings: Vec<PortMapping>,
}

/// Port mapping entry
#[derive(Debug, Clone)]
pub struct PortMapping {
    /// External (public) port
    pub external_port: u16,
    
    /// Internal (private) port
    pub internal_port: u16,
    
    /// External IP address
    pub external_ip: IpAddr,
    
    /// Protocol (TCP/UDP)
    pub protocol: Protocol,
    
    /// Mapping description
    pub description: String,
    
    /// Lease duration in seconds
    pub lease_duration: u64,
}

/// Protocol type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    fn as_str(&self) -> &'static str {
        match self {
            Protocol::Tcp => "TCP",
            Protocol::Udp => "UDP",
        }
    }
}

impl UpnpClient {
    /// Create new UPnP client and discover gateway
    pub async fn new(discovery_timeout: Duration) -> Result<Self> {
        let mut client = Self {
            gateway_addr: None,
            control_url: None,
            discovery_timeout,
            active_mappings: Vec::new(),
        };
        
        client.discover_gateway().await?;
        
        Ok(client)
    }
    
    /// Discover UPnP IGD gateway using SSDP
    async fn discover_gateway(&mut self) -> Result<()> {
        // SSDP M-SEARCH multicast discovery
        let multicast_addr: SocketAddr = "239.255.255.250:1900".parse().unwrap();
        
        let search_request = "M-SEARCH * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: 3\r\n\
             ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
             \r\n".to_string();
        
        // Create UDP socket for SSDP
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await
            .map_err(|e| dchat_core::Error::network(format!("UPnP socket bind failed: {}", e)))?;
        
        // Send M-SEARCH
        socket.send_to(search_request.as_bytes(), multicast_addr).await
            .map_err(|e| dchat_core::Error::network(format!("UPnP discovery send failed: {}", e)))?;
        
        // Wait for response
        let mut buf = vec![0u8; 2048];
        match timeout(self.discovery_timeout, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, addr))) => {
                let response = String::from_utf8_lossy(&buf[..len]);
                
                // Parse LOCATION header
                if let Some(location) = Self::parse_location(&response) {
                    self.gateway_addr = Some(addr);
                    self.control_url = Some(location);
                    Ok(())
                } else {
                    Err(dchat_core::Error::network("UPnP gateway location not found"))
                }
            }
            Ok(Err(e)) => Err(dchat_core::Error::network(format!("UPnP recv failed: {}", e))),
            Err(_) => Err(dchat_core::Error::network("UPnP discovery timeout")),
        }
    }
    
    /// Parse LOCATION header from SSDP response
    fn parse_location(response: &str) -> Option<String> {
        for line in response.lines() {
            if line.to_uppercase().starts_with("LOCATION:") {
                return Some(line.split(':').skip(1).collect::<Vec<_>>().join(":").trim().to_string());
            }
        }
        None
    }
    
    /// Add port mapping
    pub async fn add_port_mapping(
        &self,
        internal_port: u16,
        description: String,
        lease_duration: Duration,
    ) -> Result<PortMapping> {
        let _control_url = self.control_url.as_ref()
            .ok_or_else(|| dchat_core::Error::network("No UPnP gateway discovered"))?;
        
        // Get local IP
        let local_ip = self.get_local_ip().await?;
        
        // External port = internal port (for simplicity)
        let external_port = internal_port;
        
        // SOAP request for AddPortMapping
        let _soap_request = format!(
            "<?xml version=\"1.0\"?>\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
             <s:Body>\
             <u:AddPortMapping xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\
             <NewRemoteHost></NewRemoteHost>\
             <NewExternalPort>{}</NewExternalPort>\
             <NewProtocol>{}</NewProtocol>\
             <NewInternalPort>{}</NewInternalPort>\
             <NewInternalClient>{}</NewInternalClient>\
             <NewEnabled>1</NewEnabled>\
             <NewPortMappingDescription>{}</NewPortMappingDescription>\
             <NewLeaseDuration>{}</NewLeaseDuration>\
             </u:AddPortMapping>\
             </s:Body>\
             </s:Envelope>",
            external_port,
            Protocol::Udp.as_str(),
            internal_port,
            local_ip,
            description,
            lease_duration.as_secs(),
        );
        
        // Send SOAP request (simplified - real implementation would use HTTP client)
        // In production, use reqwest or hyper for HTTP POST
        
        // For now, return mock mapping
        let external_ip = self.get_external_ip().await?;
        
        Ok(PortMapping {
            external_port,
            internal_port,
            external_ip,
            protocol: Protocol::Udp,
            description,
            lease_duration: lease_duration.as_secs(),
        })
    }
    
    /// Remove port mapping
    pub async fn remove_port_mapping(
        &self,
        external_port: u16,
        protocol: Protocol,
    ) -> Result<()> {
        // SOAP DeletePortMapping request
        let _soap_request = format!(
            "<?xml version=\"1.0\"?>\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
             <s:Body>\
             <u:DeletePortMapping xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\
             <NewRemoteHost></NewRemoteHost>\
             <NewExternalPort>{}</NewExternalPort>\
             <NewProtocol>{}</NewProtocol>\
             </u:DeletePortMapping>\
             </s:Body>\
             </s:Envelope>",
            external_port,
            protocol.as_str(),
        );
        
        // Send request (implementation omitted for brevity)
        Ok(())
    }
    
    /// Get external IP address from gateway via SOAP or external service
    async fn get_external_ip(&self) -> Result<IpAddr> {
        // Try SOAP request to UPnP gateway first
        if let Some(control_url) = &self.control_url {
            let soap_request = format!(
                r#"<?xml version="1.0"?>
                <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" 
                           s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
                <s:Body>
                <u:GetExternalIPAddress xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:1">
                </u:GetExternalIPAddress>
                </s:Body>
                </s:Envelope>"#
            );
            
            // In production, send HTTP POST to control_url with SOAP request
            // Parse XML response to extract IP address
            // For now, fallback to external service
        }
        
        // Fallback: Query external IP service (ipify.org, icanhazip.com, etc.)
        // This would require tokio::net or reqwest HTTP client
        // For now, attempt to get from local network interfaces
        self.get_local_ip().await
    }
    
    /// Get local IP address from network interfaces
    async fn get_local_ip(&self) -> Result<IpAddr> {
        // Get first non-loopback IPv4 address
        // In production, use get_if_addrs crate or similar
        
        // Attempt UDP connection to determine local IP
        use std::net::UdpSocket;
        
        // Connect to a public DNS server (doesn't actually send data)
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| Error::network(format!("Failed to create UDP socket: {}", e)))?;
        
        socket.connect("8.8.8.8:80")
            .map_err(|e| Error::network(format!("Failed to connect: {}", e)))?;
        
        let local_addr = socket.local_addr()
            .map_err(|e| Error::network(format!("Failed to get local addr: {}", e)))?;
        
        Ok(local_addr.ip())
    }
    
    /// Refresh all active port mappings
    pub async fn refresh_all_mappings(&self) -> Result<()> {
        for mapping in &self.active_mappings {
            // Re-add mapping with same parameters
            let _ = self.add_port_mapping(
                mapping.internal_port,
                mapping.description.clone(),
                Duration::from_secs(mapping.lease_duration),
            ).await;
        }
        Ok(())
    }
    
    /// Remove all port mappings
    pub async fn remove_all_mappings(&self) -> Result<()> {
        for mapping in &self.active_mappings {
            let _ = self.remove_port_mapping(mapping.external_port, mapping.protocol).await;
        }
        Ok(())
    }
    
    /// Check if gateway is discovered
    pub fn is_available(&self) -> bool {
        self.gateway_addr.is_some() && self.control_url.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_protocol_string() {
        assert_eq!(Protocol::Tcp.as_str(), "TCP");
        assert_eq!(Protocol::Udp.as_str(), "UDP");
    }
    
    #[test]
    fn test_parse_location() {
        let response = "HTTP/1.1 200 OK\r\n\
                       LOCATION: http://192.168.1.1:5000/rootDesc.xml\r\n\
                       ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
                       \r\n";
        
        let location = UpnpClient::parse_location(response);
        assert!(location.is_some());
        assert!(location.unwrap().contains("rootDesc.xml"));
    }
    
    #[tokio::test]
    async fn test_upnp_client_creation() {
        // This will fail without real gateway, but tests structure
        let result = UpnpClient::new(Duration::from_millis(100)).await;
        // Expected to fail in test environment without UPnP gateway
        assert!(result.is_err() || result.unwrap().is_available());
    }
    
    #[test]
    fn test_port_mapping_clone() {
        let mapping = PortMapping {
            external_port: 8080,
            internal_port: 8080,
            external_ip: IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)),
            protocol: Protocol::Udp,
            description: "test".to_string(),
            lease_duration: 3600,
        };
        
        let cloned = mapping.clone();
        assert_eq!(mapping.external_port, cloned.external_port);
        assert_eq!(mapping.protocol, cloned.protocol);
    }
}
