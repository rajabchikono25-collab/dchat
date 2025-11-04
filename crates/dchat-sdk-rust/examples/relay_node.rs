//! Relay node example using dchat Rust SDK
//!
//! Run with: cargo run --package dchat-sdk-rust --example relay_node

use dchat_sdk_rust::{RelayConfig, RelayNode, Result};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 dchat SDK Relay Node Example\n");

    // Create a relay with custom configuration
    let config = RelayConfig {
        name: "MyRelayNode".to_string(),
        listen_addr: "0.0.0.0".to_string(),
        listen_port: 9000,
        staking_enabled: true,
        min_uptime_percent: 99.0,
    };

    let relay = RelayNode::with_config(config);

    println!("✅ Relay node created: {}", relay.config().name);
    println!(
        "📍 Listening on: {}:{}\n",
        relay.config().listen_addr,
        relay.config().listen_port
    );

    // Start the relay
    println!("🌐 Starting relay node...");
    relay.start().await?;
    println!("✅ Relay is running!\n");

    // Simulate running for a bit
    println!("⏳ Running for 5 seconds...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Get statistics
    println!("\n📊 Relay Statistics:");
    let stats = relay.get_stats().await;
    println!("  Connected peers: {}", stats.connected_peers);
    println!("  Messages relayed: {}", stats.messages_relayed);
    println!("  Uptime: {:.2}%", stats.uptime_percent);
    println!("  Reputation score: {}\n", stats.reputation_score);

    // Stop the relay
    println!("🛑 Stopping relay node...");
    relay.stop().await?;
    println!("✅ Relay stopped cleanly\n");

    println!("🎉 Example completed successfully!");

    Ok(())
}
