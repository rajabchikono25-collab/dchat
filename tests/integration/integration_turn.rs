use dchat_network::nat_traversal::{NatTraversal, NatConfig};
use std::env;

#[tokio::test]
#[ignore] // Run with: cargo test --test integration_turn -- --ignored
async fn test_turn_allocation() {
    let turn_server = env::var("TURN_SERVER")
        .unwrap_or_else(|_| "turn.dchat.io:3478".to_string());
    let turn_username = env::var("TURN_USERNAME")
        .expect("TURN_USERNAME not set");
    let turn_secret = env::var("TURN_SECRET")
        .expect("TURN_SECRET not set");

    let config = NatConfig {
        stun_servers: vec![],
        turn_servers: vec![format!("{}:{}@{}", turn_username, turn_secret, turn_server)],
        upnp_enabled: false,
        hole_punching_enabled: false,
    };

    let nat = NatTraversal::new(config).await.unwrap();
    
    let allocation = nat.allocate_turn_relay().await;
    assert!(allocation.is_ok(), "TURN allocation failed: {:?}", allocation);
    
    let relay_addr = allocation.unwrap();
    println!("Allocated relay: {:?}", relay_addr);
}
