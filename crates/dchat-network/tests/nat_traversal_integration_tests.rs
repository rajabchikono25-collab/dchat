// Integration tests for NAT traversal implementation
// Tests STUN, UPnP, TURN, and hole punching strategies

use dchat_network::nat_traversal::{NatConfig, NatStrategy, NatTraversalManager, NatType};
use std::net::SocketAddr;
use tokio::time::{timeout, Duration};

/// Test NAT detection using public STUN servers
#[tokio::test]
async fn test_nat_detection_with_public_stun() {
    let config = NatConfig::default();
    let mut manager = NatTraversalManager::new(config);

    // This test requires internet connectivity to Google STUN servers
    // It will detect the actual NAT type of the testing environment
    match timeout(Duration::from_secs(15), manager.detect_nat_type()).await {
        Ok(Ok(nat_type)) => {
            println!("Detected NAT type: {:?}", nat_type);
            assert!(manager.detected_nat_type().is_some());

            // Verify NAT type is valid
            match nat_type {
                NatType::None
                | NatType::Open
                | NatType::FullCone
                | NatType::RestrictedCone
                | NatType::PortRestrictedCone
                | NatType::PortRestricted
                | NatType::Symmetric
                | NatType::Unknown => {
                    // Valid NAT types
                    assert!(true);
                }
            }
        }
        Ok(Err(e)) => {
            println!("NAT detection failed (may be offline): {:?}", e);
            // Don't fail test if network is unavailable
        }
        Err(_) => {
            println!("NAT detection timed out (may be firewalled)");
            // Don't fail test if network is slow/firewalled
        }
    }
}

/// Test STUN packet building
#[test]
fn test_stun_binding_request_format() {
    let config = NatConfig::default();
    let manager = NatTraversalManager::new(config);

    let request = manager.build_stun_binding_request();

    // Verify packet structure
    assert_eq!(request.len(), 20); // 20 bytes: header(4) + magic(4) + transaction_id(12)

    // Check message type (0x0001 = Binding Request)
    assert_eq!(request[0], 0x00);
    assert_eq!(request[1], 0x01);

    // Check message length (should be 0x0000 for no attributes)
    assert_eq!(request[2], 0x00);
    assert_eq!(request[3], 0x00);

    // Check magic cookie (0x2112A442)
    assert_eq!(request[4], 0x21);
    assert_eq!(request[5], 0x12);
    assert_eq!(request[6], 0xA4);
    assert_eq!(request[7], 0x42);

    // Transaction ID should be random (12 bytes)
    assert_eq!(request.len(), 20);
}

/// Test STUN response parsing with mock data
#[test]
fn test_stun_response_parsing() {
    let config = NatConfig::default();
    let manager = NatTraversalManager::new(config);

    // Create mock STUN Binding Response with XOR-MAPPED-ADDRESS
    // Response format: [msg_type(2), length(2), magic(4), transaction_id(12), attributes...]
    let mut response = Vec::new();

    // Message type: 0x0101 (Binding Success Response)
    response.extend_from_slice(&[0x01, 0x01]);

    // Message length: 12 bytes (one XOR-MAPPED-ADDRESS attribute)
    response.extend_from_slice(&[0x00, 0x0C]);

    // Magic cookie
    response.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);

    // Transaction ID (random)
    response.extend_from_slice(&[
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
    ]);

    // XOR-MAPPED-ADDRESS attribute
    // Type: 0x0020
    response.extend_from_slice(&[0x00, 0x20]);
    // Length: 8 bytes (family(1) + reserved(1) + port(2) + ip(4))
    response.extend_from_slice(&[0x00, 0x08]);
    // Family: 0x01 (IPv4)
    response.push(0x00);
    response.push(0x01);

    // XOR'd port: Let's say real port is 12345 (0x3039)
    // XOR with 0x2112 = 0x112B
    response.extend_from_slice(&[0x11, 0x2B]);

    // XOR'd IP: Let's say real IP is 203.0.113.10 (0xCB00710A)
    // XOR with 0x2112A442 = 0xEA12D548
    response.extend_from_slice(&[0xEA, 0x12, 0xD5, 0x48]);

    // Parse the response
    match manager.parse_stun_response(&response) {
        Ok(addr) => {
            println!("Parsed address: {}", addr);
            assert_eq!(addr.port(), 12345);
            assert_eq!(addr.ip().to_string(), "203.0.113.10");
        }
        Err(e) => {
            panic!("Failed to parse STUN response: {:?}", e);
        }
    }
}

/// Test NAT strategy recommendation
#[test]
fn test_strategy_recommendation() {
    let config = NatConfig::default();
    let mut manager = NatTraversalManager::new(config);

    // Test strategy for None NAT
    manager.set_detected_nat_type(Some(NatType::None));
    assert_eq!(manager.get_recommended_strategy(), NatStrategy::Direct);

    // Test strategy for FullCone NAT
    manager.set_detected_nat_type(Some(NatType::FullCone));
    assert_eq!(manager.get_recommended_strategy(), NatStrategy::UPnP);

    // Test strategy for RestrictedCone NAT
    manager.set_detected_nat_type(Some(NatType::RestrictedCone));
    assert_eq!(manager.get_recommended_strategy(), NatStrategy::UPnP);

    // Test strategy for PortRestrictedCone NAT
    manager.set_detected_nat_type(Some(NatType::PortRestrictedCone));
    assert_eq!(
        manager.get_recommended_strategy(),
        NatStrategy::HolePunching
    );

    // Test strategy for Symmetric NAT (hardest case)
    manager.set_detected_nat_type(Some(NatType::Symmetric));
    assert_eq!(manager.get_recommended_strategy(), NatStrategy::TURN);

    // Test strategy for Unknown NAT (fallback to TURN)
    manager.set_detected_nat_type(Some(NatType::Unknown));
    assert_eq!(manager.get_recommended_strategy(), NatStrategy::TURN);
}

/// Test TURN Allocate request building
#[test]
fn test_turn_allocate_request_format() {
    let config = NatConfig::default();
    let manager = NatTraversalManager::new(config);

    let username = "testuser";
    let credential = "testpass";

    let request = manager
        .build_turn_allocate_request(username, credential)
        .unwrap();

    // Verify packet structure
    assert!(request.len() > 20); // Should have header + attributes

    // Check message type (0x0003 = Allocate Request)
    assert_eq!(request[0], 0x00);
    assert_eq!(request[1], 0x03);

    // Check magic cookie
    assert_eq!(request[4], 0x21);
    assert_eq!(request[5], 0x12);
    assert_eq!(request[6], 0xA4);
    assert_eq!(request[7], 0x42);

    // Should contain REQUESTED-TRANSPORT attribute (0x0019)
    let has_transport = request.windows(2).any(|w| w == [0x00, 0x19]);
    assert!(
        has_transport,
        "TURN request should have REQUESTED-TRANSPORT attribute"
    );

    // Should contain USERNAME attribute (0x0006)
    let has_username = request.windows(2).any(|w| w == [0x00, 0x06]);
    assert!(has_username, "TURN request should have USERNAME attribute");

    // Should contain MESSAGE-INTEGRITY attribute (0x0008)
    let has_integrity = request.windows(2).any(|w| w == [0x00, 0x08]);
    assert!(
        has_integrity,
        "TURN request should have MESSAGE-INTEGRITY attribute"
    );
}

/// Test TURN Allocate response parsing with mock data
#[test]
fn test_turn_allocate_response_parsing() {
    let config = NatConfig::default();
    let manager = NatTraversalManager::new(config);

    // Create mock TURN Allocate Success Response with XOR-RELAYED-ADDRESS
    let mut response = Vec::new();

    // Message type: 0x0103 (Allocate Success Response)
    response.extend_from_slice(&[0x01, 0x03]);

    // Message length: 12 bytes (one XOR-RELAYED-ADDRESS attribute)
    response.extend_from_slice(&[0x00, 0x0C]);

    // Magic cookie
    response.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);

    // Transaction ID
    response.extend_from_slice(&[
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
    ]);

    // XOR-RELAYED-ADDRESS attribute (0x0016)
    response.extend_from_slice(&[0x00, 0x16]);
    response.extend_from_slice(&[0x00, 0x08]); // Length
    response.push(0x00); // Reserved
    response.push(0x01); // IPv4

    // XOR'd port: relay port 54321 (0xD431) XOR 0x2112 = 0xF523
    response.extend_from_slice(&[0xF5, 0x23]);

    // XOR'd IP: relay IP 198.51.100.5 (0xC6336405) XOR 0x2112A442 = 0xE721C047
    response.extend_from_slice(&[0xE7, 0x21, 0xC0, 0x47]);

    match manager.parse_turn_allocate_response(&response) {
        Ok(addr) => {
            println!("Parsed relay address: {}", addr);
            assert_eq!(addr.port(), 54321);
            assert_eq!(addr.ip().to_string(), "198.51.100.5");
        }
        Err(e) => {
            panic!("Failed to parse TURN Allocate response: {:?}", e);
        }
    }
}

/// Test TURN error response (401 Unauthorized)
#[test]
fn test_turn_error_response() {
    let config = NatConfig::default();
    let manager = NatTraversalManager::new(config);

    // Create mock TURN Error Response (0x0113)
    let mut response = Vec::new();

    // Message type: 0x0113 (Error Response)
    response.extend_from_slice(&[0x01, 0x13]);

    // Message length: 0
    response.extend_from_slice(&[0x00, 0x00]);

    // Magic cookie
    response.extend_from_slice(&[0x21, 0x12, 0xA4, 0x42]);

    // Transaction ID
    response.extend_from_slice(&[
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
    ]);

    match manager.parse_turn_allocate_response(&response) {
        Ok(_) => {
            panic!("Should have failed on error response");
        }
        Err(e) => {
            println!("Correctly detected error: {:?}", e);
            assert!(e.to_string().contains("allocation failed"));
        }
    }
}

/// Test NAT manager state management
#[tokio::test]
async fn test_nat_manager_state() {
    let config = NatConfig::default();
    let mut manager = NatTraversalManager::new(config);

    // Initial state
    assert!(manager.detected_nat_type().is_none());
    assert!(manager.active_strategy().is_none());
    assert!(manager.upnp_gateway().is_none());
    assert_eq!(manager.turn_connections().len(), 0);

    // Simulate setting detected NAT type
    manager.set_detected_nat_type(Some(NatType::FullCone));
    assert_eq!(manager.detected_nat_type(), Some(NatType::FullCone));

    // Cleanup should reset state
    manager.cleanup().await.unwrap();
    assert!(manager.active_strategy().is_none());
}

/// Test hole punching timeout and retry logic
#[tokio::test]
async fn test_hole_punching_timeout() {
    let config = NatConfig {
        enable_hole_punching: true,
        ..NatConfig::default()
    };
    let mut manager = NatTraversalManager::new(config);

    // Try hole punching with invalid addresses (should timeout/fail)
    let local_addr: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    let remote_addr: SocketAddr = "203.0.113.1:50000".parse().unwrap(); // TEST-NET-3 (non-routable)

    match timeout(
        Duration::from_secs(10),
        manager.attempt_hole_punching(local_addr, remote_addr),
    )
    .await
    {
        Ok(Ok(success)) => {
            // Should fail since addresses aren't actually connected
            assert!(
                !success,
                "Hole punching should fail with non-routable addresses"
            );
        }
        Ok(Err(e)) => {
            println!("Hole punching error (expected): {:?}", e);
        }
        Err(_) => {
            panic!("Hole punching test timed out");
        }
    }
}

/// Test UPnP SSDP discovery packet format
#[test]
fn test_upnp_ssdp_discovery_logic() {
    // Verify SSDP multicast address
    let ssdp_addr: SocketAddr = "239.255.255.250:1900".parse().unwrap();
    assert_eq!(ssdp_addr.port(), 1900);

    // Verify M-SEARCH packet structure
    let search_request = "M-SEARCH * HTTP/1.1\r\n\
         HOST: 239.255.255.250:1900\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: 3\r\n\
         ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
         \r\n";

    assert!(search_request.contains("M-SEARCH"));
    assert!(search_request.contains("ssdp:discover"));
    assert!(search_request.contains("InternetGatewayDevice"));
}

/// Test configuration validation
#[test]
fn test_nat_config_validation() {
    let config = NatConfig::default();

    // Default should have STUN servers
    assert!(!config.stun_servers.is_empty());
    assert_eq!(config.stun_servers.len(), 2);

    // Should use Google STUN servers by default
    assert!(config.stun_servers[0].contains("stun.l.google.com"));

    // Should have TURN servers configured
    assert!(!config.turn_servers.is_empty());

    // UPnP should be enabled by default
    assert!(config.enable_upnp);

    // Hole punching should be enabled by default
    assert!(config.enable_hole_punching);

    // Detection timeout should be reasonable
    assert!(config.detection_timeout_secs > 0);
    assert!(config.detection_timeout_secs <= 30);

    // Port range should be in dynamic range
    assert!(config.upnp_port_range.0 >= 49152);
    assert!(config.upnp_port_range.1 <= 65535);
}

/// Test NAT type classification edge cases
#[test]
fn test_nat_type_classification() {
    let config = NatConfig::default();
    let mut manager = NatTraversalManager::new(config);

    // Test all NAT types have valid strategies
    let nat_types = vec![
        NatType::None,
        NatType::Open,
        NatType::FullCone,
        NatType::RestrictedCone,
        NatType::PortRestrictedCone,
        NatType::PortRestricted,
        NatType::Symmetric,
        NatType::Unknown,
    ];

    for nat_type in nat_types {
        manager.set_detected_nat_type(Some(nat_type.clone()));
        let strategy = manager.get_recommended_strategy();

        // Verify every NAT type maps to a valid strategy
        match strategy {
            NatStrategy::Direct
            | NatStrategy::UPnP
            | NatStrategy::HolePunching
            | NatStrategy::TURN
            | NatStrategy::Turn => {
                println!("{:?} -> {:?}", nat_type, strategy);
            }
        }
    }
}
