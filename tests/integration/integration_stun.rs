use dchat_network::nat_traversal::{NatTraversal, NatConfig};
use std::env;

#[tokio::test]
async fn test_stun_against_google_servers() {
    let config = NatConfig {
        stun_servers: vec![
            "stun.l.google.com:19302".to_string(),
            "stun1.l.google.com:19302".to_string(),
        ],
        turn_servers: vec![],
        upnp_enabled: false,
        hole_punching_enabled: false,
    };

    let nat = NatTraversal::new(config).await.expect("Failed to create NAT traversal");
    
    let result = nat.detect_nat_type().await;
    assert!(result.is_ok(), "STUN detection failed: {:?}", result);
    
    let nat_type = result.unwrap();
    println!("Detected NAT type: {:?}", nat_type);
    
    let external_ip = nat.get_external_address().await;
    assert!(external_ip.is_ok(), "Failed to get external IP");
    
    println!("External IP: {:?}", external_ip.unwrap());
}

#[tokio::test]
async fn test_stun_consistency() {
    let config = NatConfig::default();
    let nat = NatTraversal::new(config).await.unwrap();
    
    let ip1 = nat.get_external_address().await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    let ip2 = nat.get_external_address().await.unwrap();
    
    assert_eq!(ip1, ip2, "STUN should return consistent external IP");
}
