// Relay Network Deployment CLI
//
// Automates deployment of distributed relay network (20-50 nodes)
// across multiple geographic regions with health monitoring and incentives.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dchat_deployment::{RelayNetworkConfig, RelayReputation};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "deploy-relays")]
#[command(about = "Deploy and manage distributed relay network", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate relay network configuration
    GenerateConfig {
        /// Network name
        #[arg(long, default_value = "dchat-mainnet")]
        network: String,

        /// Base domain for relay hostnames (e.g. schikuno.top)
        #[arg(long, default_value = "schikuno.top")]
        domain: String,

        /// Number of relays to deploy (20-50)
        #[arg(long, default_value = "30")]
        count: usize,

        /// Output directory for configs
        #[arg(long, default_value = "./config/relays")]
        output: PathBuf,
    },

    /// Deploy a single relay to a server
    DeployRelay {
        /// Relay ID
        #[arg(long)]
        relay_id: String,

        /// Path to relay config file
        #[arg(long)]
        config: PathBuf,

        /// Server IP or hostname
        #[arg(long)]
        server: String,

        /// SSH user
        #[arg(long, default_value = "root")]
        user: String,

        /// SSH key path
        #[arg(long)]
        key: Option<PathBuf>,
    },

    /// Deploy all relays in the network
    DeployAll {
        /// Path to relay network config summary
        #[arg(long)]
        config: PathBuf,

        /// Server mapping JSON (relay_id -> server_ip)
        #[arg(long)]
        servers: PathBuf,

        /// SSH user
        #[arg(long, default_value = "root")]
        user: String,

        /// SSH key path
        #[arg(long)]
        key: Option<PathBuf>,

        /// Deploy in parallel (faster but riskier)
        #[arg(long)]
        parallel: bool,
    },

    /// Check health of all relays
    HealthCheck {
        /// Path to relay network config summary
        #[arg(long)]
        config: PathBuf,

        /// Health check timeout (seconds)
        #[arg(long, default_value = "30")]
        timeout: u64,
    },

    /// Calculate rewards for all relays
    CalculateRewards {
        /// Path to relay network config
        #[arg(long)]
        config: PathBuf,

        /// Path to relay reputation data
        #[arg(long)]
        reputations: PathBuf,

        /// Number of days to calculate
        #[arg(long, default_value = "1")]
        days: u64,
    },

    /// Monitor relay network health
    Monitor {
        /// Path to relay network config
        #[arg(long)]
        config: PathBuf,

        /// Check interval (seconds)
        #[arg(long, default_value = "30")]
        interval: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::GenerateConfig {
            network,
            domain,
            count,
            output,
        } => generate_relay_config(network, domain, count, output).await,
        Commands::DeployRelay {
            relay_id,
            config,
            server,
            user,
            key,
        } => deploy_relay(&relay_id, &config, &server, &user, key.as_deref()).await,
        Commands::DeployAll {
            config,
            servers,
            user,
            key,
            parallel,
        } => deploy_all_relays(&config, &servers, &user, key.as_deref(), parallel).await,
        Commands::HealthCheck { config, timeout } => health_check_all(&config, timeout).await,
        Commands::CalculateRewards {
            config,
            reputations,
            days,
        } => calculate_rewards(&config, &reputations, days).await,
        Commands::Monitor { config, interval } => monitor_network(&config, interval).await,
    }
}

async fn generate_relay_config(
    network: String,
    domain: String,
    count: usize,
    output: PathBuf,
) -> Result<()> {
    info!(
        "Generating relay network configuration for {} relays",
        count
    );

    let config = RelayNetworkConfig::new_recommended(network.clone(), domain.clone(), count)
        .context("Failed to create relay network config")?;

    config
        .verify_diversity()
        .context("Failed diversity verification")?;

    // Create output directory
    fs::create_dir_all(&output).context("Failed to create output directory")?;

    // Generate TOML for each relay
    for relay in &config.relays {
        let toml = config
            .generate_relay_toml(&relay.relay_id)
            .context(format!("Failed to generate TOML for {}", relay.relay_id))?;

        let filename = output.join(format!("{}.toml", relay.relay_id));
        fs::write(&filename, toml).context(format!("Failed to write {}", filename.display()))?;

        info!("Generated configuration: {}", filename.display());
    }

    // Save deployment summary as JSON
    let summary = serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    let summary_path = output.join("deployment-summary.json");
    fs::write(&summary_path, summary).context("Failed to write deployment summary")?;

    info!(
        "✅ Generated {} relay configurations in {:?}",
        config.relays.len(),
        output
    );
    info!("   - Target relay count: {}", config.target_relay_count);
    info!(
        "   - Minimum relays per region: {}",
        config.min_relays_per_region
    );
    info!(
        "   - Health check interval: {}s",
        config.health_check_interval
    );

    Ok(())
}

async fn deploy_relay(
    relay_id: &str,
    config_path: &PathBuf,
    server: &str,
    user: &str,
    key: Option<&std::path::Path>,
) -> Result<()> {
    info!("Deploying relay {} to {}", relay_id, server);

    // Step 1: Check SSH connectivity
    info!("Step 1/8: Checking SSH connectivity...");
    check_ssh_connectivity(server, user, key).await?;

    // Step 2: Install dependencies
    info!("Step 2/8: Installing dependencies...");
    install_relay_dependencies(server, user, key).await?;

    // Step 3: Copy relay configuration
    info!("Step 3/8: Copying relay configuration...");
    copy_relay_config(config_path, server, user, key, relay_id).await?;

    // Step 4: Generate relay keys
    info!("Step 4/8: Generating relay cryptographic keys...");
    generate_relay_keys(server, user, key, relay_id).await?;

    // Step 5: Deploy relay container
    info!("Step 5/8: Deploying relay container...");
    deploy_relay_container(server, user, key, relay_id).await?;

    // Step 6: Wait for startup
    info!("Step 6/8: Waiting for relay startup...");
    sleep(Duration::from_secs(10)).await;

    // Step 7: Health check
    info!("Step 7/8: Performing health check...");
    let config_content = fs::read_to_string(config_path)?;
    let relay_config: toml::Value = toml::from_str(&config_content)?;
    let rpc_address = relay_config
        .get("network")
        .and_then(|n| n.get("rpc_address"))
        .and_then(|a| a.as_str())
        .context("Missing rpc_address in config")?;

    check_relay_health(server, rpc_address, 60).await?;

    // Step 8: Register relay on-chain
    info!("Step 8/8: Registering relay on blockchain...");
    register_relay_onchain(server, user, key, relay_id).await?;

    info!("✅ Successfully deployed relay {} to {}", relay_id, server);
    Ok(())
}

async fn deploy_all_relays(
    config_path: &PathBuf,
    servers_path: &PathBuf,
    user: &str,
    key: Option<&std::path::Path>,
    parallel: bool,
) -> Result<()> {
    info!(
        "Deploying all relays in {} mode",
        if parallel { "PARALLEL" } else { "SEQUENTIAL" }
    );

    // Load config
    let config_json = fs::read_to_string(config_path)?;
    let network_config: RelayNetworkConfig = serde_json::from_str(&config_json)?;

    // Load server mapping
    let servers_json = fs::read_to_string(servers_path)?;
    let server_mapping: HashMap<String, String> = serde_json::from_str(&servers_json)?;

    if parallel {
        // Deploy all relays in parallel
        let mut handles = vec![];
        for relay in &network_config.relays {
            let server = server_mapping
                .get(&relay.relay_id)
                .context(format!("No server mapping for {}", relay.relay_id))?
                .clone();

            let relay_id = relay.relay_id.clone();
            let relay_id_for_handle = relay_id.clone();
            let config_path = config_path
                .parent()
                .unwrap()
                .join(format!("{}.toml", relay_id));
            let user = user.to_string();
            let key = key.map(|k| k.to_path_buf());

            let handle = tokio::spawn(async move {
                deploy_relay(&relay_id, &config_path, &server, &user, key.as_deref()).await
            });
            handles.push((relay_id_for_handle, handle));
        }

        // Wait for all deployments
        for (relay_id, handle) in handles {
            match handle.await {
                Ok(Ok(())) => info!("✅ Relay {} deployed successfully", relay_id),
                Ok(Err(e)) => error!("❌ Relay {} deployment failed: {}", relay_id, e),
                Err(e) => error!("❌ Relay {} task panicked: {}", relay_id, e),
            }
        }
    } else {
        // Deploy sequentially
        for relay in &network_config.relays {
            let server = server_mapping
                .get(&relay.relay_id)
                .context(format!("No server mapping for {}", relay.relay_id))?;

            let config_path = config_path
                .parent()
                .unwrap()
                .join(format!("{}.toml", relay.relay_id));

            match deploy_relay(&relay.relay_id, &config_path, server, user, key).await {
                Ok(()) => info!("✅ Relay {} deployed successfully", relay.relay_id),
                Err(e) => {
                    error!("❌ Relay {} deployment failed: {}", relay.relay_id, e);
                    warn!("Continuing with next relay...");
                }
            }

            // Small delay between deployments
            sleep(Duration::from_secs(5)).await;
        }
    }

    info!("✅ Relay deployment complete");
    Ok(())
}

async fn health_check_all(config_path: &PathBuf, timeout: u64) -> Result<()> {
    info!("Performing health check on all relays");

    let config_json = fs::read_to_string(config_path)?;
    let network_config: RelayNetworkConfig = serde_json::from_str(&config_json)?;

    let mut healthy = 0;
    let mut unhealthy = 0;

    for relay in &network_config.relays {
        let rpc_url = format!(
            "http://{}:{}/health",
            relay.public_address,
            relay.rpc_address.port()
        );

        match tokio::time::timeout(Duration::from_secs(timeout), reqwest::get(&rpc_url)).await {
            Ok(Ok(response)) if response.status().is_success() => {
                info!("✅ {} is HEALTHY", relay.relay_id);
                healthy += 1;
            }
            _ => {
                warn!("❌ {} is UNHEALTHY", relay.relay_id);
                unhealthy += 1;
            }
        }
    }

    info!(
        "\nHealth check complete: {} healthy, {} unhealthy",
        healthy, unhealthy
    );

    let health_percentage = healthy as f64 / (healthy + unhealthy) as f64;
    if health_percentage >= 0.8 {
        info!(
            "✅ Network health: {:.1}% (GOOD)",
            health_percentage * 100.0
        );
        Ok(())
    } else {
        error!(
            "❌ Network health: {:.1}% (POOR)",
            health_percentage * 100.0
        );
        Err(anyhow::anyhow!("Network health below 80%"))
    }
}

async fn calculate_rewards(
    config_path: &PathBuf,
    reputations_path: &PathBuf,
    days: u64,
) -> Result<()> {
    info!("Calculating rewards for {} days", days);

    let config_json = fs::read_to_string(config_path)?;
    let network_config: RelayNetworkConfig = serde_json::from_str(&config_json)?;

    let reputations_json = fs::read_to_string(reputations_path)?;
    let reputations: HashMap<String, RelayReputation> = serde_json::from_str(&reputations_json)?;

    let mut total_rewards = 0.0;

    info!(
        "\n{:<30} {:<10} {:<15} {:<10} {:<15}",
        "Relay ID", "Tier", "Messages", "Uptime", "Daily Reward"
    );
    info!("{}", "-".repeat(80));

    for relay in &network_config.relays {
        if let Some(reputation) = reputations.get(&relay.relay_id) {
            let uptime = reputation.uptime_percentage();
            let reward = relay.calculate_daily_reward(
                network_config.incentives.base_reward_per_day,
                reputation.messages_relayed / days,
                network_config.incentives.fee_per_message,
                uptime,
            );

            total_rewards += reward * days as f64;

            info!(
                "{:<30} {:<10?} {:<15} {:<10.1}% {:<15.2} DCHAT/day",
                relay.relay_id,
                relay.tier,
                reputation.messages_relayed,
                uptime * 100.0,
                reward
            );
        }
    }

    info!("{}", "-".repeat(80));
    info!("Total rewards ({} days): {:.2} DCHAT", days, total_rewards);

    Ok(())
}

async fn monitor_network(config_path: &PathBuf, interval: u64) -> Result<()> {
    info!("Starting network monitoring (interval: {}s)", interval);
    info!("Press Ctrl+C to stop");

    let config_json = fs::read_to_string(config_path)?;
    let network_config: RelayNetworkConfig = serde_json::from_str(&config_json)?;

    loop {
        let mut healthy = 0;
        let mut unhealthy = 0;
        let mut total_connections = 0;

        for relay in &network_config.relays {
            let rpc_url = format!(
                "http://{}:{}/health",
                relay.public_address,
                relay.rpc_address.port()
            );

            match reqwest::get(&rpc_url).await {
                Ok(response) if response.status().is_success() => {
                    healthy += 1;
                    // Try to get connection count from response
                    if let Ok(body) = response.json::<serde_json::Value>().await {
                        if let Some(connections) = body.get("connections").and_then(|c| c.as_u64())
                        {
                            total_connections += connections;
                        }
                    }
                }
                _ => unhealthy += 1,
            }
        }

        let health_percentage = healthy as f64 / (healthy + unhealthy) as f64;
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");

        info!(
            "[{}] Health: {:.1}% ({}/{}) | Connections: {} | Status: {}",
            timestamp,
            health_percentage * 100.0,
            healthy,
            healthy + unhealthy,
            total_connections,
            if health_percentage >= 0.8 {
                "✅ GOOD"
            } else {
                "⚠️ DEGRADED"
            }
        );

        sleep(Duration::from_secs(interval)).await;
    }
}

// Helper functions for deployment steps

async fn check_ssh_connectivity(
    server: &str,
    user: &str,
    _key: Option<&std::path::Path>,
) -> Result<()> {
    let test_cmd = format!("ssh -o ConnectTimeout=10 {}@{} 'echo OK'", user, server);
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&test_cmd)
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("SSH connectivity check failed"))
    }
}

async fn install_relay_dependencies(
    server: &str,
    user: &str,
    _key: Option<&std::path::Path>,
) -> Result<()> {
    let install_cmd = format!(
        "ssh {}@{} 'apt-get update && apt-get install -y docker.io docker-compose curl'",
        user, server
    );

    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&install_cmd)
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Failed to install dependencies"))
    }
}

async fn copy_relay_config(
    config_path: &PathBuf,
    server: &str,
    user: &str,
    _key: Option<&std::path::Path>,
    relay_id: &str,
) -> Result<()> {
    let scp_cmd = format!(
        "scp {} {}@{}:/data/{}.toml",
        config_path.display(),
        user,
        server,
        relay_id
    );

    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&scp_cmd)
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Failed to copy config"))
    }
}

async fn generate_relay_keys(
    server: &str,
    user: &str,
    _key: Option<&std::path::Path>,
    relay_id: &str,
) -> Result<()> {
    let keygen_cmd = format!(
        "ssh {}@{} 'dchat-keygen --relay --output /data/keys/{}.key'",
        user, server, relay_id
    );

    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&keygen_cmd)
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Failed to generate keys"))
    }
}

async fn deploy_relay_container(
    server: &str,
    user: &str,
    _key: Option<&std::path::Path>,
    relay_id: &str,
) -> Result<()> {
    let docker_cmd = format!(
        "ssh {}@{} 'docker run -d --name {} --restart=always \
        -v /data/{}.toml:/config.toml \
        -v /data/keys/{}.key:/keys/relay.key \
        -p 8080:8080 -p 9090:9090 \
        dchat/relay:latest --config /config.toml'",
        user, server, relay_id, relay_id, relay_id
    );

    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&docker_cmd)
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Failed to deploy container"))
    }
}

async fn check_relay_health(server: &str, rpc_address: &str, timeout_secs: u64) -> Result<()> {
    let rpc_port = rpc_address
        .split(':')
        .nth(1)
        .context("Invalid RPC address")?;

    let health_url = format!("http://{}:{}/health", server, rpc_port);

    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            return Err(anyhow::anyhow!("Health check timeout"));
        }

        match reqwest::get(&health_url).await {
            Ok(response) if response.status().is_success() => {
                info!("Relay is healthy");
                return Ok(());
            }
            _ => {
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

async fn register_relay_onchain(
    server: &str,
    user: &str,
    _key: Option<&std::path::Path>,
    relay_id: &str,
) -> Result<()> {
    let register_cmd = format!(
        "ssh {}@{} 'dchat-cli relay register --id {}'",
        user, server, relay_id
    );

    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&register_cmd)
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        warn!("Failed to register relay on-chain (may need manual registration)");
        Ok(()) // Don't fail deployment
    }
}
