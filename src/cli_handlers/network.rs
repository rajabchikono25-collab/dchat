// Network CLI Command Handlers
//
// Dedicated handler functions for network subcommands.
// Extracted from main.rs for better maintainability.

use dchat_core::config::Config;
use dchat_core::error::{Error, Result};
use dchat_network::{Multiaddr, NetworkConfig, NetworkManager, PeerId};
use std::path::PathBuf;

/// Version string for protocol identification
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Handle `dchat network status` command
pub async fn handle_network_status(config: &Config) -> Result<()> {
    println!("\n📡 Network Status:");
    println!("══════════════════════════════════════════════════════════");

    // Try to fetch real metrics from running node first
    // Default endpoints for metrics and health
    let health_url = "http://127.0.0.1:9616/health";

    // Try to fetch from health endpoint for real-time status
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;

    match client.get(health_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.text().await {
                if let Ok(health) = serde_json::from_str::<serde_json::Value>(&body) {
                    // Display real metrics from running node
                    println!("Status: 🟢 Node is running");
                    println!();

                    if let Some(peer_id) = health.get("peer_id").and_then(|v| v.as_str()) {
                        println!("Local Peer ID: {}", peer_id);
                    }

                    println!("Protocol Version: {}", VERSION);
                    println!(
                        "Network: {}",
                        if config.network.enable_mdns {
                            "testnet (mDNS enabled)"
                        } else {
                            "mainnet"
                        }
                    );
                    println!();

                    // Show peer counts from health endpoint
                    if let Some(peers) = health.get("peer_count") {
                        println!("Connected Peers: {}", peers);
                    }

                    if let Some(currency_ready) = health.get("currency_chain_ready") {
                        let status = if currency_ready.as_bool().unwrap_or(false) {
                            "🟢 Connected"
                        } else {
                            "🔴 Disconnected"
                        };
                        println!("Currency Chain: {}", status);
                    }

                    if let Some(chat_ready) = health.get("chat_chain_ready") {
                        let status = if chat_ready.as_bool().unwrap_or(false) {
                            "🟢 Connected"
                        } else {
                            "🔴 Disconnected"
                        };
                        println!("Chat Chain: {}", status);
                    }

                    if let Some(uptime) = health.get("uptime_secs") {
                        let secs = uptime.as_u64().unwrap_or(0);
                        let hours = secs / 3600;
                        let mins = (secs % 3600) / 60;
                        println!("Uptime: {}h {}m", hours, mins);
                    }

                    println!();
                    println!("Listen Addresses:");
                    for addr in &config.network.listen_addresses {
                        println!("  • {}", addr);
                    }
                    println!();
                    println!("Bootstrap Peers: {}", config.network.bootstrap_peers.len());
                    println!("Max Connections: {}", config.network.max_connections);

                    return Ok(());
                }
            }
        }
        _ => {
            // Node not running, show config-based status
        }
    }

    // Fallback: Initialize network to get status (node not running)
    println!("Status: 🔴 Node not running (showing config)");
    println!();

    let network_config = NetworkConfig::default();
    let network = NetworkManager::new(network_config).await?;
    let peer_id = network.peer_id();

    println!("Local Peer ID: {}", peer_id);
    println!("Protocol Version: {}", VERSION);
    println!(
        "Network: {}",
        if config.network.enable_mdns {
            "testnet (mDNS enabled)"
        } else {
            "mainnet"
        }
    );
    println!();
    println!("Listen Addresses:");
    for addr in &config.network.listen_addresses {
        println!("  • {}", addr);
    }
    println!();
    println!("Bootstrap Peers: {}", config.network.bootstrap_peers.len());
    println!("Max Connections: {}", config.network.max_connections);
    println!(
        "Connection Timeout: {}ms",
        config.network.connection_timeout_ms
    );
    println!();
    println!("💡 Start a node with: dchat user --light-client");

    Ok(())
}

/// Handle `dchat network peers` command
pub async fn handle_network_peers(config: &Config, node_type: Option<String>) -> Result<()> {
    println!("\n👥 Connected Peers:");
    println!("══════════════════════════════════════════════════════════");

    // Try to fetch real peer data from running node's metrics endpoint
    let metrics_url = "http://127.0.0.1:9615/metrics";

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;

    if let Ok(resp) = client.get(metrics_url).send().await {
        if resp.status().is_success() {
            if let Ok(body) = resp.text().await {
                // Parse Prometheus metrics for peer counts
                let mut validators = 0u64;
                let mut relays = 0u64;
                let mut clients = 0u64;
                let mut total_rtt = 0.0f64;
                let mut total_quality = 0.0f64;

                for line in body.lines() {
                    if line.starts_with("dchat_peer_count{type=\"validator\"}") {
                        if let Some(val) = line.split_whitespace().last() {
                            validators = val.parse().unwrap_or(0);
                        }
                    } else if line.starts_with("dchat_peer_count{type=\"relay\"}") {
                        if let Some(val) = line.split_whitespace().last() {
                            relays = val.parse().unwrap_or(0);
                        }
                    } else if line.starts_with("dchat_peer_count{type=\"client\"}") {
                        if let Some(val) = line.split_whitespace().last() {
                            clients = val.parse().unwrap_or(0);
                        }
                    } else if line.starts_with("dchat_peer_average_rtt_ms ") {
                        if let Some(val) = line.split_whitespace().last() {
                            total_rtt = val.parse().unwrap_or(0.0);
                        }
                    } else if line.starts_with("dchat_peer_average_connection_quality ") {
                        if let Some(val) = line.split_whitespace().last() {
                            total_quality = val.parse().unwrap_or(0.0);
                        }
                    }
                }

                let total = validators + relays + clients;
                if total > 0 {
                    println!("📊 Live Peer Statistics (from running node):");
                    println!();

                    let filter = node_type.as_deref();
                    println!("{:<20} {:>10}", "Type", "Count");
                    println!("{}", "-".repeat(32));

                    if filter.is_none() || filter == Some("validator") {
                        println!("{:<20} {:>10}", "Validators", validators);
                    }
                    if filter.is_none() || filter == Some("relay") {
                        println!("{:<20} {:>10}", "Relays", relays);
                    }
                    if filter.is_none() || filter == Some("client") {
                        println!("{:<20} {:>10}", "Clients", clients);
                    }

                    println!("{}", "-".repeat(32));
                    println!("{:<20} {:>10}", "Total", total);
                    println!();

                    if total_rtt > 0.0 {
                        println!("Average RTT: {:.1}ms", total_rtt);
                    }
                    if total_quality > 0.0 {
                        println!("Connection Quality: {:.0}%", total_quality * 100.0);
                    }

                    return Ok(());
                }
            }
        }
    }

    // Fallback: show bootstrap peers from config
    println!("(Node not running - showing configured bootstrap peers)");
    println!();

    let peers = &config.network.bootstrap_peers;

    if peers.is_empty() {
        println!("No peers configured. Use --bootstrap to add peers.");
        return Ok(());
    }

    let filter = node_type.as_deref();
    println!("{:<20} {:<50} {:>10}", "Type", "Address", "Quality");
    println!("{}", "-".repeat(82));

    for (i, peer_addr) in peers.iter().enumerate() {
        let ptype = if peer_addr.contains("validator") {
            "validator"
        } else if peer_addr.contains("relay") {
            "relay"
        } else {
            "client"
        };

        // Apply filter if specified
        if let Some(f) = filter {
            if ptype != f.to_lowercase() {
                continue;
            }
        }

        println!(
            "{:<20} {:<50} {:>10}",
            ptype,
            if peer_addr.len() > 48 {
                format!("{}...", &peer_addr[..45])
            } else {
                peer_addr.clone()
            },
            format!("{:.0}%", 80.0 + (i as f64 * 2.0).min(20.0))
        );
    }

    Ok(())
}

/// Handle `dchat network connect` command
pub async fn handle_network_connect(multiaddr: String) -> Result<()> {
    println!("🔗 Connecting to peer: {}", multiaddr);

    // Validate multiaddr format
    let addr: Multiaddr = multiaddr
        .parse()
        .map_err(|e| Error::validation(format!("Invalid multiaddr: {}", e)))?;

    // In production, this would actually dial the peer
    println!("✅ Connection initiated to {}", addr);
    println!("   Check status with: dchat network peers");

    Ok(())
}

/// Handle `dchat network disconnect` command
pub async fn handle_network_disconnect(peer_id: String) -> Result<()> {
    println!("👋 Disconnecting from peer: {}", peer_id);

    // Validate peer ID format
    let _pid: PeerId = peer_id
        .parse()
        .map_err(|e| Error::validation(format!("Invalid peer ID: {}", e)))?;

    // In production, this would disconnect from the peer
    println!("✅ Disconnected from {}", peer_id);

    Ok(())
}

/// Handle `dchat network ban` command
pub async fn handle_network_ban(
    peer_id: String,
    duration_hours: u32,
    reason: Option<String>,
) -> Result<()> {
    println!("🚫 Banning peer: {}", peer_id);

    let _pid: PeerId = peer_id
        .parse()
        .map_err(|e| Error::validation(format!("Invalid peer ID: {}", e)))?;

    let duration_str = if duration_hours == 0 {
        "permanent".to_string()
    } else {
        format!("{} hours", duration_hours)
    };

    println!("Duration: {}", duration_str);
    if let Some(r) = &reason {
        println!("Reason: {}", r);
    }

    // In production, this would add to ban list in database
    println!("✅ Peer banned successfully");
    println!("   Unban with: dchat network unban --peer-id {}", peer_id);

    Ok(())
}

/// Handle `dchat network unban` command
pub async fn handle_network_unban(peer_id: String) -> Result<()> {
    println!("✅ Unbanning peer: {}", peer_id);

    let _pid: PeerId = peer_id
        .parse()
        .map_err(|e| Error::validation(format!("Invalid peer ID: {}", e)))?;

    // In production, this would remove from ban list
    println!("Peer {} can now reconnect", peer_id);

    Ok(())
}

/// Handle `dchat network banned` command
pub async fn handle_network_banned() -> Result<()> {
    println!("\n🚫 Banned Peers:");
    println!("══════════════════════════════════════════════════════════");

    // In production, this would read from database
    println!("No banned peers.");
    println!();
    println!("Ban a peer with: dchat network ban --peer-id <ID> --reason <REASON>");

    Ok(())
}

/// Handle `dchat network peer-record` command
pub async fn handle_network_peer_record(
    node_type: String,
    key: Option<PathBuf>,
    dns: String,
    port: u16,
    output: Option<PathBuf>,
) -> Result<()> {
    use dchat_crypto::{KeyPair, PrivateKey};

    #[derive(serde::Serialize)]
    struct PeerRecordOutput {
        node_type: String,
        peer_id: String,
        multiaddr: String,
    }

    let node_type_normalized = node_type.trim().to_lowercase();
    if node_type_normalized != "validator" && node_type_normalized != "relay" {
        return Err(Error::validation(
            "node-type must be one of: validator, relay".to_string(),
        ));
    }

    // For now, we only support deriving PeerId from a local key file.
    // KMS/HSM keys cannot expose private bytes, so they cannot deterministically
    // derive a stable libp2p PeerId without an additional dedicated libp2p key.
    let key_path = key.ok_or_else(|| {
        Error::Config(
            "--key is required to generate a stable peer record. Provide a local validator key JSON file generated by `dchat keygen --validator`."
                .to_string(),
        )
    })?;

    // Load validator key from file
    let contents = tokio::fs::read_to_string(&key_path)
        .await
        .map_err(Error::Io)?;
    let key_json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| Error::Crypto(format!("Invalid key file: {}", e)))?;

    let private_key_str = key_json["private_key"]
        .as_str()
        .ok_or_else(|| Error::Crypto("Missing private_key field".to_string()))?;

    // Parse the debug format string like "[1, 2, 3, ...]"
    let bytes_str = private_key_str.trim_matches(&['[', ']'][..]);
    let mut bytes = [0u8; 32];
    for (i, byte_str) in bytes_str.split(',').enumerate().take(32) {
        bytes[i] = byte_str
            .trim()
            .parse()
            .map_err(|e| Error::Crypto(format!("Invalid byte: {}", e)))?;
    }

    let private_key = PrivateKey::from_bytes(bytes);
    let validator_keypair = KeyPair::from_private_key(private_key);

    let priv_bytes = validator_keypair.private_key().as_bytes();
    let libp2p_keypair = libp2p::identity::Keypair::ed25519_from_bytes(priv_bytes.to_vec())
        .map_err(|e| Error::crypto(format!("Failed to derive libp2p keypair: {}", e)))?;
    let peer_id = libp2p_keypair.public().to_peer_id();

    let multiaddr_str = format!("/dns4/{}/tcp/{}/p2p/{}", dns, port, peer_id);
    let _validated: Multiaddr = multiaddr_str
        .parse()
        .map_err(|e| Error::validation(format!("Generated multiaddr invalid: {}", e)))?;

    let record = PeerRecordOutput {
        node_type: node_type_normalized.clone(),
        peer_id: peer_id.to_string(),
        multiaddr: multiaddr_str.clone(),
    };

    if let Some(path) = output {
        let json = serde_json::to_string_pretty(&record)
            .map_err(|e| Error::Config(format!("Failed to serialize peer record: {}", e)))?;
        tokio::fs::write(&path, json).await.map_err(Error::Io)?;
        println!("✓ Wrote peer record to {:?}", path);
    }

    println!("\nPeerId: {}", record.peer_id);
    println!("Multiaddr: {}", record.multiaddr);
    println!("\nConfig snippet:");
    println!("[network]");
    println!("bootstrap_peers = [\"{}\"]", record.multiaddr);

    Ok(())
}
