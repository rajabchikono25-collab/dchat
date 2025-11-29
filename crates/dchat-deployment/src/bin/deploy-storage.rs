// Distributed Storage Deployment CLI
//
// This tool deploys the complete 4-tier distributed storage architecture:
// 1. CockroachDB cluster (5 nodes, multi-region SQL)
// 2. Redis cluster (6 nodes: 3 masters + 3 replicas)
// 3. MinIO distributed storage (4 nodes, object storage)
// 4. TiKV cluster (3 PD + 5 storage nodes)

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dchat_deployment::distributed_storage::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;
use tokio::time::sleep;

/// Validates a hostname to prevent command injection attacks.
/// Only allows alphanumeric characters, dots, hyphens, and underscores.
fn validate_hostname(host: &str) -> Result<()> {
    let hostname_regex = Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._-]*[a-zA-Z0-9]$|^[a-zA-Z0-9]$")
        .expect("Invalid regex");
    
    // Also check for IP addresses
    let ipv4_regex = Regex::new(r"^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$")
        .expect("Invalid regex");
    
    if host.is_empty() {
        anyhow::bail!("Hostname cannot be empty");
    }
    
    if host.len() > 253 {
        anyhow::bail!("Hostname too long (max 253 characters)");
    }
    
    // Check for dangerous characters that could enable command injection
    if host.contains(';') || host.contains('|') || host.contains('&') ||
       host.contains('$') || host.contains('`') || host.contains('\\') ||
       host.contains('"') || host.contains('\'') || host.contains('\n') ||
       host.contains('\r') || host.contains(' ') {
        anyhow::bail!("Hostname contains invalid characters: {}", host);
    }
    
    if !hostname_regex.is_match(host) && !ipv4_regex.is_match(host) {
        anyhow::bail!("Invalid hostname format: {}", host);
    }
    
    Ok(())
}

/// Validates a file path to prevent path traversal attacks.
fn validate_path(path: &str) -> Result<()> {
    if path.contains("..") {
        anyhow::bail!("Path traversal detected in: {}", path);
    }
    if path.contains(';') || path.contains('|') || path.contains('&') ||
       path.contains('$') || path.contains('`') {
        anyhow::bail!("Path contains dangerous characters: {}", path);
    }
    Ok(())
}

#[derive(Parser)]
#[command(name = "deploy-storage")]
#[command(about = "Deploy distributed storage infrastructure for dchat")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate storage configuration files
    GenerateConfig {
        /// Output directory for configs
        #[arg(short, long, default_value = "./storage-configs")]
        output: PathBuf,

        /// Network name
        #[arg(short, long, default_value = "dchat-mainnet")]
        network: String,
    },

    /// Deploy CockroachDB cluster
    DeployCockroachDB {
        /// Path to config file
        #[arg(short, long)]
        config: PathBuf,

        /// Node index to deploy (0-based), or "all" for all nodes
        #[arg(short, long, default_value = "all")]
        node: String,

        /// SSH key path
        #[arg(short, long)]
        key: Option<PathBuf>,
    },

    /// Deploy Redis cluster
    DeployRedis {
        /// Path to config file
        #[arg(short, long)]
        config: PathBuf,

        /// Node type: "masters", "replicas", or "all"
        #[arg(short, long, default_value = "all")]
        node_type: String,

        /// SSH key path
        #[arg(short, long)]
        key: Option<PathBuf>,
    },

    /// Deploy MinIO cluster
    DeployMinIO {
        /// Path to config file
        #[arg(short, long)]
        config: PathBuf,

        /// Node index to deploy (0-based), or "all" for all nodes
        #[arg(short, long, default_value = "all")]
        node: String,

        /// SSH key path
        #[arg(short, long)]
        key: Option<PathBuf>,
    },

    /// Deploy TiKV cluster
    DeployTiKV {
        /// Path to config file
        #[arg(short, long)]
        config: PathBuf,

        /// Component: "pd", "tikv", or "all"
        #[arg(short = 't', long, default_value = "all")]
        component: String,

        /// SSH key path
        #[arg(short, long)]
        key: Option<PathBuf>,
    },

    /// Deploy complete storage infrastructure
    DeployAll {
        /// Path to config directory
        #[arg(short, long)]
        config_dir: PathBuf,

        /// SSH key path
        #[arg(short, long)]
        key: Option<PathBuf>,

        /// Parallel deployment (faster but riskier)
        #[arg(short, long)]
        parallel: bool,
    },

    /// Check health of all storage systems
    HealthCheck {
        /// Path to config directory
        #[arg(short, long)]
        config_dir: PathBuf,

        /// Timeout in seconds
        #[arg(short, long, default_value = "60")]
        timeout: u64,
    },

    /// Migrate data from legacy PostgreSQL/SQLite
    Migrate {
        /// Source database type: "postgresql" or "sqlite"
        #[arg(short, long)]
        source: String,

        /// Source connection string or path
        #[arg(short = 'c', long)]
        source_connection: String,

        /// Target storage config directory
        #[arg(short, long)]
        target_config: PathBuf,

        /// Dry run (don't actually migrate)
        #[arg(short, long)]
        dry_run: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct DeploymentSummary {
    network_name: String,
    total_nodes: usize,
    cockroachdb_nodes: usize,
    redis_nodes: usize,
    minio_nodes: usize,
    tikv_nodes: usize,
    estimated_monthly_cost_usd: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::GenerateConfig { output, network } => {
            generate_config(&output, &network).await?;
        }
        Commands::DeployCockroachDB { config, node, key } => {
            deploy_cockroachdb(&config, &node, key.as_deref()).await?;
        }
        Commands::DeployRedis {
            config,
            node_type,
            key,
        } => {
            deploy_redis(&config, &node_type, key.as_deref()).await?;
        }
        Commands::DeployMinIO { config, node, key } => {
            deploy_minio(&config, &node, key.as_deref()).await?;
        }
        Commands::DeployTiKV {
            config,
            component,
            key,
        } => {
            deploy_tikv(&config, &component, key.as_deref()).await?;
        }
        Commands::DeployAll {
            config_dir,
            key,
            parallel,
        } => {
            deploy_all(&config_dir, key.as_deref(), parallel).await?;
        }
        Commands::HealthCheck {
            config_dir,
            timeout,
        } => {
            health_check(&config_dir, timeout).await?;
        }
        Commands::Migrate {
            source,
            source_connection,
            target_config,
            dry_run,
        } => {
            migrate(&source, &source_connection, &target_config, dry_run).await?;
        }
    }

    Ok(())
}

async fn generate_config(output: &Path, network: &str) -> Result<()> {
    println!(
        "🔧 Generating distributed storage configuration for '{}'...",
        network
    );

    fs::create_dir_all(output).context("Failed to create output directory")?;

    let config = DistributedStorageConfig::new_recommended(network.to_string());

    // Verify configuration
    config
        .verify_all()
        .context("Configuration validation failed")?;

    // Save CockroachDB config
    let cockroach_file = output.join("cockroachdb-config.json");
    let cockroach_json = serde_json::to_string_pretty(&config.cockroachdb)?;
    fs::write(&cockroach_file, cockroach_json).context("Failed to write CockroachDB config")?;
    println!("✅ CockroachDB config: {}", cockroach_file.display());

    // Save Redis config
    let redis_file = output.join("redis-config.json");
    let redis_json = serde_json::to_string_pretty(&config.redis)?;
    fs::write(&redis_file, redis_json).context("Failed to write Redis config")?;
    println!("✅ Redis config: {}", redis_file.display());

    // Save MinIO config
    let minio_file = output.join("minio-config.json");
    let minio_json = serde_json::to_string_pretty(&config.minio)?;
    fs::write(&minio_file, minio_json).context("Failed to write MinIO config")?;
    println!("✅ MinIO config: {}", minio_file.display());

    // Save TiKV config
    let tikv_file = output.join("tikv-config.json");
    let tikv_json = serde_json::to_string_pretty(&config.tikv)?;
    fs::write(&tikv_file, tikv_json).context("Failed to write TiKV config")?;
    println!("✅ TiKV config: {}", tikv_file.display());

    // Create deployment summary
    let summary = DeploymentSummary {
        network_name: network.to_string(),
        total_nodes: config.total_node_count(),
        cockroachdb_nodes: config.cockroachdb.nodes.len(),
        redis_nodes: config.redis.masters.len() + config.redis.replicas.len(),
        minio_nodes: config.minio.nodes.len(),
        tikv_nodes: config.tikv.pd_nodes.len() + config.tikv.tikv_nodes.len(),
        estimated_monthly_cost_usd: estimate_cost(&config),
    };

    let summary_file = output.join("deployment-summary.json");
    let summary_json = serde_json::to_string_pretty(&summary)?;
    fs::write(&summary_file, summary_json).context("Failed to write summary")?;

    println!("\n📊 Deployment Summary:");
    println!("  Total nodes: {}", summary.total_nodes);
    println!("  - CockroachDB: {} nodes", summary.cockroachdb_nodes);
    println!("  - Redis: {} nodes", summary.redis_nodes);
    println!("  - MinIO: {} nodes", summary.minio_nodes);
    println!("  - TiKV: {} nodes (PD + storage)", summary.tikv_nodes);
    println!(
        "  Estimated cost: ${:.2}/month",
        summary.estimated_monthly_cost_usd
    );

    Ok(())
}

async fn deploy_cockroachdb(config_path: &Path, node: &str, key: Option<&Path>) -> Result<()> {
    println!("🚀 Deploying CockroachDB cluster...");

    let config_json = fs::read_to_string(config_path)?;
    let config: CockroachDBConfig = serde_json::from_str(&config_json)?;

    let nodes_to_deploy: Vec<&CockroachDBNode> = if node == "all" {
        config.nodes.iter().collect()
    } else {
        let index: usize = node.parse().context("Node must be a number or 'all'")?;
        vec![&config.nodes[index]]
    };

    for (i, node) in nodes_to_deploy.iter().enumerate() {
        println!(
            "\n[{}/{}] Deploying CockroachDB node: {}",
            i + 1,
            nodes_to_deploy.len(),
            node.node_id
        );

        deploy_cockroachdb_node(&config, node, key).await?;

        if i < nodes_to_deploy.len() - 1 {
            println!("⏱️  Waiting 10s before next node...");
            sleep(Duration::from_secs(10)).await;
        }
    }

    println!("\n✅ CockroachDB cluster deployment complete!");
    println!(
        "🔗 Admin UI: http://{}:{}",
        config.nodes[0].host, config.http_port
    );
    println!(
        "🔗 Connection: {}",
        config.connection_string("dchat_user", "<password>")
    );

    Ok(())
}

async fn deploy_cockroachdb_node(
    cluster: &CockroachDBConfig,
    node: &CockroachDBNode,
    key: Option<&Path>,
) -> Result<()> {
    // Step 1: Check SSH connectivity
    println!("  [1/8] Checking SSH connectivity to {}...", node.host);
    check_ssh(&node.host, key).await?;

    // Step 2: Install dependencies
    println!("  [2/8] Installing CockroachDB...");
    install_cockroachdb(&node.host, key).await?;

    // Step 3: Create directories
    println!("  [3/8] Creating data directories...");
    create_directories(&node.host, &[&node.store_path], key).await?;

    // Step 4: Generate TLS certificates (for production)
    if cluster.encryption_at_rest {
        println!("  [4/8] Generating TLS certificates...");
        generate_cockroachdb_certs(cluster, node, key).await?;
    } else {
        println!("  [4/8] Skipping TLS (insecure mode)");
    }

    // Step 5: Start CockroachDB node
    println!("  [5/8] Starting CockroachDB node...");
    start_cockroachdb_node(cluster, node, key).await?;

    // Step 6: Wait for node to be ready
    println!("  [6/8] Waiting for node to be ready...");
    sleep(Duration::from_secs(10)).await;

    // Step 7: Initialize cluster (first node only)
    if node.node_id.ends_with("-1") {
        println!("  [7/8] Initializing cluster...");
        initialize_cockroachdb(&node.host, cluster.sql_port, key).await?;
    } else {
        println!("  [7/8] Joining existing cluster...");
    }

    // Step 8: Health check
    println!("  [8/8] Health check...");
    check_cockroachdb_health(&node.host, cluster.http_port).await?;

    println!("  ✅ Node {} deployed successfully", node.node_id);
    Ok(())
}

async fn deploy_redis(config_path: &Path, node_type: &str, key: Option<&Path>) -> Result<()> {
    println!("🚀 Deploying Redis cluster ({})...", node_type);

    let config_json = fs::read_to_string(config_path)?;
    let config: RedisConfig = serde_json::from_str(&config_json)?;

    let nodes_to_deploy: Vec<&RedisNode> = match node_type {
        "masters" => config.masters.iter().collect(),
        "replicas" => config.replicas.iter().collect(),
        "all" => config
            .masters
            .iter()
            .chain(config.replicas.iter())
            .collect(),
        _ => anyhow::bail!("Invalid node_type: must be 'masters', 'replicas', or 'all'"),
    };

    // Deploy all masters first
    let masters_to_deploy: Vec<&RedisNode> = nodes_to_deploy
        .iter()
        .filter(|n| n.role == RedisNodeRole::Master)
        .copied()
        .collect();

    for (i, node) in masters_to_deploy.iter().enumerate() {
        println!(
            "\n[{}/{}] Deploying Redis master: {}",
            i + 1,
            masters_to_deploy.len(),
            node.node_id
        );
        deploy_redis_node(&config, node, key).await?;
        sleep(Duration::from_secs(5)).await;
    }

    // Deploy replicas
    let replicas_to_deploy: Vec<&RedisNode> = nodes_to_deploy
        .iter()
        .filter(|n| n.role == RedisNodeRole::Replica)
        .copied()
        .collect();

    for (i, node) in replicas_to_deploy.iter().enumerate() {
        println!(
            "\n[{}/{}] Deploying Redis replica: {}",
            i + 1,
            replicas_to_deploy.len(),
            node.node_id
        );
        deploy_redis_node(&config, node, key).await?;
        sleep(Duration::from_secs(5)).await;
    }

    // Create cluster
    if node_type == "all" && nodes_to_deploy.len() >= 6 {
        println!("\n📡 Creating Redis cluster...");
        create_redis_cluster(&config, key).await?;
    }

    println!("\n✅ Redis cluster deployment complete!");
    println!("🔗 Connection: {}", config.connection_string(None));

    Ok(())
}

async fn deploy_redis_node(
    cluster: &RedisConfig,
    node: &RedisNode,
    key: Option<&Path>,
) -> Result<()> {
    println!("  [1/6] Checking SSH connectivity to {}...", node.host);
    check_ssh(&node.host, key).await?;

    println!("  [2/6] Installing Redis...");
    install_redis(&node.host, key).await?;

    println!("  [3/6] Configuring Redis node...");
    configure_redis_node(cluster, node, key).await?;

    println!("  [4/6] Starting Redis container...");
    start_redis_node(node, key).await?;

    println!("  [5/6] Waiting for node to be ready...");
    sleep(Duration::from_secs(5)).await;

    println!("  [6/6] Health check...");
    check_redis_health(&node.host, cluster.port).await?;

    println!("  ✅ Node {} deployed successfully", node.node_id);
    Ok(())
}

async fn deploy_minio(config_path: &Path, node: &str, key: Option<&Path>) -> Result<()> {
    println!("🚀 Deploying MinIO cluster...");

    let config_json = fs::read_to_string(config_path)?;
    let config: MinIOConfig = serde_json::from_str(&config_json)?;

    let nodes_to_deploy: Vec<&MinIONode> = if node == "all" {
        config.nodes.iter().collect()
    } else {
        let index: usize = node.parse()?;
        vec![&config.nodes[index]]
    };

    for (i, node) in nodes_to_deploy.iter().enumerate() {
        println!(
            "\n[{}/{}] Deploying MinIO node: {}",
            i + 1,
            nodes_to_deploy.len(),
            node.node_id
        );
        deploy_minio_node(&config, node, key).await?;
        sleep(Duration::from_secs(5)).await;
    }

    println!("\n✅ MinIO cluster deployment complete!");
    println!(
        "🔗 API: http://{}:{}",
        config.nodes[0].host, config.api_port
    );
    println!(
        "🔗 Console: http://{}:{}",
        config.nodes[0].host, config.console_port
    );
    println!("📝 Server command: {}", config.server_command());

    Ok(())
}

async fn deploy_minio_node(
    cluster: &MinIOConfig,
    node: &MinIONode,
    key: Option<&Path>,
) -> Result<()> {
    println!("  [1/6] Checking SSH connectivity to {}...", node.host);
    check_ssh(&node.host, key).await?;

    println!("  [2/6] Installing MinIO...");
    install_minio(&node.host, key).await?;

    println!("  [3/6] Creating data volumes...");
    create_directories(
        &node.host,
        &node
            .data_volumes
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
        key,
    )
    .await?;

    println!("  [4/6] Starting MinIO container...");
    start_minio_node(cluster, node, key).await?;

    println!("  [5/6] Waiting for node to be ready...");
    sleep(Duration::from_secs(10)).await;

    println!("  [6/6] Health check...");
    check_minio_health(&node.host, cluster.api_port).await?;

    println!("  ✅ Node {} deployed successfully", node.node_id);
    Ok(())
}

async fn deploy_tikv(config_path: &Path, component: &str, key: Option<&Path>) -> Result<()> {
    println!("🚀 Deploying TiKV cluster ({})...", component);

    let config_json = fs::read_to_string(config_path)?;
    let config: TiKVConfig = serde_json::from_str(&config_json)?;

    match component {
        "pd" | "all" => {
            for (i, pd) in config.pd_nodes.iter().enumerate() {
                println!(
                    "\n[{}/{}] Deploying TiKV PD: {}",
                    i + 1,
                    config.pd_nodes.len(),
                    pd.node_id
                );
                deploy_tikv_pd(&config, pd, key).await?;
                sleep(Duration::from_secs(5)).await;
            }
        }
        _ => {}
    }

    if component == "tikv" || component == "all" {
        for (i, tikv) in config.tikv_nodes.iter().enumerate() {
            println!(
                "\n[{}/{}] Deploying TiKV storage: {}",
                i + 1,
                config.tikv_nodes.len(),
                tikv.node_id
            );
            deploy_tikv_storage(&config, tikv, key).await?;
            sleep(Duration::from_secs(5)).await;
        }
    }

    println!("\n✅ TiKV cluster deployment complete!");
    println!("🔗 PD endpoints: {:?}", config.pd_endpoints());

    Ok(())
}

async fn deploy_tikv_pd(cluster: &TiKVConfig, pd: &TiKVPDNode, key: Option<&Path>) -> Result<()> {
    println!("  [1/6] Checking SSH connectivity to {}...", pd.host);
    check_ssh(&pd.host, key).await?;

    println!("  [2/6] Installing TiKV PD...");
    install_tikv(&pd.host, key).await?;

    println!("  [3/6] Creating data directory...");
    create_directories(&pd.host, &[&pd.data_dir], key).await?;

    println!("  [4/6] Starting PD container...");
    start_tikv_pd(cluster, pd, key).await?;

    println!("  [5/6] Waiting for PD to be ready...");
    sleep(Duration::from_secs(10)).await;

    println!("  [6/6] Health check...");
    check_tikv_pd_health(&pd.host, cluster.pd_port).await?;

    println!("  ✅ PD {} deployed successfully", pd.node_id);
    Ok(())
}

async fn deploy_tikv_storage(
    cluster: &TiKVConfig,
    tikv: &TiKVStorageNode,
    key: Option<&Path>,
) -> Result<()> {
    println!("  [1/6] Checking SSH connectivity to {}...", tikv.host);
    check_ssh(&tikv.host, key).await?;

    println!("  [2/6] Installing TiKV storage...");
    install_tikv(&tikv.host, key).await?;

    println!("  [3/6] Creating data directory...");
    create_directories(&tikv.host, &[&tikv.data_dir], key).await?;

    println!("  [4/6] Starting TiKV container...");
    start_tikv_storage(cluster, tikv, key).await?;

    println!("  [5/6] Waiting for TiKV to be ready...");
    sleep(Duration::from_secs(10)).await;

    println!("  [6/6] Health check...");
    check_tikv_storage_health(&tikv.host, tikv.status_address.port()).await?;

    println!("  ✅ Storage {} deployed successfully", tikv.node_id);
    Ok(())
}

async fn deploy_all(config_dir: &Path, key: Option<&Path>, parallel: bool) -> Result<()> {
    println!("🚀 Deploying complete distributed storage infrastructure...");
    println!("📁 Config directory: {}", config_dir.display());

    let cockroach_config = config_dir.join("cockroachdb-config.json");
    let redis_config = config_dir.join("redis-config.json");
    let minio_config = config_dir.join("minio-config.json");
    let tikv_config = config_dir.join("tikv-config.json");

    if parallel {
        println!("⚡ Parallel deployment mode (faster but riskier)");
        // In production: use tokio::spawn for parallel deployment
        // For now, deploy sequentially
    }

    // Deploy in order: TiKV PD → TiKV Storage → CockroachDB → Redis → MinIO
    println!("\n📦 Step 1: Deploying TiKV cluster...");
    deploy_tikv(&tikv_config, "all", key).await?;

    println!("\n📦 Step 2: Deploying CockroachDB cluster...");
    deploy_cockroachdb(&cockroach_config, "all", key).await?;

    println!("\n📦 Step 3: Deploying Redis cluster...");
    deploy_redis(&redis_config, "all", key).await?;

    println!("\n📦 Step 4: Deploying MinIO cluster...");
    deploy_minio(&minio_config, "all", key).await?;

    println!("\n✅ Complete distributed storage deployment finished!");
    println!("\n📊 Next steps:");
    println!(
        "  1. Run health checks: deploy-storage health-check --config-dir {}",
        config_dir.display()
    );
    println!("  2. Migrate data: deploy-storage migrate --source postgresql --source-connection <conn> --target-config {}", config_dir.display());
    println!("  3. Configure application to use new storage backends");

    Ok(())
}

async fn health_check(config_dir: &Path, _timeout: u64) -> Result<()> {
    println!("🔍 Checking health of all storage systems...");

    let mut all_healthy = true;

    // Check CockroachDB
    println!("\n[1/4] CockroachDB health check...");
    let cockroach_config = config_dir.join("cockroachdb-config.json");
    if cockroach_config.exists() {
        let config_json = fs::read_to_string(&cockroach_config)?;
        let config: CockroachDBConfig = serde_json::from_str(&config_json)?;
        for node in &config.nodes {
            match check_cockroachdb_health(&node.host, config.http_port).await {
                Ok(_) => println!("  ✅ {} healthy", node.node_id),
                Err(e) => {
                    println!("  ❌ {} unhealthy: {}", node.node_id, e);
                    all_healthy = false;
                }
            }
        }
    }

    // Check Redis
    println!("\n[2/4] Redis health check...");
    let redis_config = config_dir.join("redis-config.json");
    if redis_config.exists() {
        let config_json = fs::read_to_string(&redis_config)?;
        let config: RedisConfig = serde_json::from_str(&config_json)?;
        for node in config.masters.iter().chain(config.replicas.iter()) {
            match check_redis_health(&node.host, config.port).await {
                Ok(_) => println!("  ✅ {} healthy", node.node_id),
                Err(e) => {
                    println!("  ❌ {} unhealthy: {}", node.node_id, e);
                    all_healthy = false;
                }
            }
        }
    }

    // Check MinIO
    println!("\n[3/4] MinIO health check...");
    let minio_config = config_dir.join("minio-config.json");
    if minio_config.exists() {
        let config_json = fs::read_to_string(&minio_config)?;
        let config: MinIOConfig = serde_json::from_str(&config_json)?;
        for node in &config.nodes {
            match check_minio_health(&node.host, config.api_port).await {
                Ok(_) => println!("  ✅ {} healthy", node.node_id),
                Err(e) => {
                    println!("  ❌ {} unhealthy: {}", node.node_id, e);
                    all_healthy = false;
                }
            }
        }
    }

    // Check TiKV
    println!("\n[4/4] TiKV health check...");
    let tikv_config = config_dir.join("tikv-config.json");
    if tikv_config.exists() {
        let config_json = fs::read_to_string(&tikv_config)?;
        let config: TiKVConfig = serde_json::from_str(&config_json)?;
        for pd in &config.pd_nodes {
            match check_tikv_pd_health(&pd.host, config.pd_port).await {
                Ok(_) => println!("  ✅ {} healthy", pd.node_id),
                Err(e) => {
                    println!("  ❌ {} unhealthy: {}", pd.node_id, e);
                    all_healthy = false;
                }
            }
        }
        for tikv in &config.tikv_nodes {
            match check_tikv_storage_health(&tikv.host, tikv.status_address.port()).await {
                Ok(_) => println!("  ✅ {} healthy", tikv.node_id),
                Err(e) => {
                    println!("  ❌ {} unhealthy: {}", tikv.node_id, e);
                    all_healthy = false;
                }
            }
        }
    }

    if all_healthy {
        println!("\n✅ All storage systems are healthy!");
        Ok(())
    } else {
        anyhow::bail!("⚠️  Some storage systems are unhealthy")
    }
}

async fn migrate(
    source: &str,
    source_connection: &str,
    target_config: &Path,
    dry_run: bool,
) -> Result<()> {
    println!(
        "🔄 Migrating data from {} to distributed storage...",
        source
    );

    if dry_run {
        println!("🔍 DRY RUN MODE - no data will be modified");
    }

    println!("📊 Migration plan:");
    println!("  Source: {} ({})", source, source_connection);
    println!("  Target config: {}", target_config.display());

    match source {
        "postgresql" => {
            println!("\n📝 PostgreSQL → CockroachDB migration:");
            println!("  1. Export schema: pg_dump --schema-only");
            println!("  2. Convert types: TEXT → STRING, SERIAL → INT");
            println!("  3. Import schema to CockroachDB");
            println!("  4. Bulk copy data: COPY TO CSV → IMPORT INTO");
            println!("  5. Verify row counts and indexes");

            if !dry_run {
                println!("\n⚠️  Actual migration not implemented yet");
                println!("    Use cockroach import or pg_dump → cockroach sql");
            }
        }
        "sqlite" => {
            println!("\n📝 SQLite → TiKV migration:");
            println!("  1. Read all key-value pairs from SQLite");
            println!("  2. Batch insert into TiKV using RawClient");
            println!("  3. Verify key counts");

            if !dry_run {
                println!("\n⚠️  Actual migration not implemented yet");
                println!("    Use TiKV RawClient API for bulk inserts");
            }
        }
        _ => anyhow::bail!("Unsupported source: {}", source),
    }

    Ok(())
}

// Helper functions for SSH and deployment

/// Securely execute SSH command with proper argument handling.
/// Validates hostname to prevent command injection.
fn build_ssh_command(host: &str, key: Option<&Path>) -> Result<StdCommand> {
    validate_hostname(host)?;
    
    let mut cmd = StdCommand::new("ssh");
    
    // Use strict host key checking in production
    cmd.arg("-o").arg("StrictHostKeyChecking=accept-new")
       .arg("-o").arg("ConnectTimeout=30")
       .arg("-o").arg("BatchMode=yes");
    
    if let Some(key_path) = key {
        // Validate key path exists
        if !key_path.exists() {
            anyhow::bail!("SSH key file does not exist: {}", key_path.display());
        }
        cmd.arg("-i").arg(key_path);
    }
    
    cmd.arg(format!("root@{}", host));
    
    Ok(cmd)
}

async fn check_ssh(host: &str, key: Option<&Path>) -> Result<()> {
    let mut cmd = build_ssh_command(host, key)?;
    cmd.args(["echo", "OK"]);
    
    let output = cmd.output()?;

    if !output.status.success() {
        anyhow::bail!("SSH check failed for {}: {}", host, String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

async fn install_cockroachdb(host: &str, key: Option<&Path>) -> Result<()> {
    let script = r#"set -euo pipefail
curl -fsSL https://binaries.cockroachdb.com/cockroach-latest.linux-amd64.tgz | tar -xz
cp -i cockroach-*/cockroach /usr/local/bin/
mkdir -p /usr/local/lib/cockroach
cp -i cockroach-*/lib/libgeos.so /usr/local/lib/cockroach/
cp -i cockroach-*/lib/libgeos_c.so /usr/local/lib/cockroach/"#;

    let mut cmd = build_ssh_command(host, key)?;
    cmd.arg("bash").arg("-c").arg(script);
    
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Failed to install CockroachDB on {}", host);
    }

    Ok(())
}

async fn install_redis(host: &str, key: Option<&Path>) -> Result<()> {
    let script = "set -euo pipefail; apt-get update && apt-get install -y redis-server redis-tools";
    
    let mut cmd = build_ssh_command(host, key)?;
    cmd.arg("bash").arg("-c").arg(script);
    
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Failed to install Redis on {}", host);
    }
    Ok(())
}

async fn install_minio(host: &str, key: Option<&Path>) -> Result<()> {
    let script = r#"set -euo pipefail
wget -q https://dl.min.io/server/minio/release/linux-amd64/minio
chmod +x minio
mv minio /usr/local/bin/"#;

    let mut cmd = build_ssh_command(host, key)?;
    cmd.arg("bash").arg("-c").arg(script);
    
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Failed to install MinIO on {}", host);
    }
    Ok(())
}

async fn install_tikv(host: &str, key: Option<&Path>) -> Result<()> {
    let script = r#"set -euo pipefail
curl --proto '=https' --tlsv1.2 -sSf https://tiup-mirrors.pingcap.com/install.sh | sh
source ~/.bash_profile
tiup install pd tikv"#;

    let mut cmd = build_ssh_command(host, key)?;
    cmd.arg("bash").arg("-c").arg(script);
    
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Failed to install TiKV on {}", host);
    }
    Ok(())
}

async fn create_directories(host: &str, dirs: &[&str], key: Option<&Path>) -> Result<()> {
    // Validate all directory paths
    for dir in dirs {
        validate_path(dir)?;
    }
    
    let mkdir_cmd = format!("mkdir -p {}", dirs.join(" "));

    let mut cmd = build_ssh_command(host, key)?;
    cmd.arg("bash").arg("-c").arg(&mkdir_cmd);
    
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Failed to create directories on {}", host);
    }
    Ok(())
}

async fn generate_cockroachdb_certs(
    cluster: &CockroachDBConfig,
    node: &CockroachDBNode,
    key: Option<&Path>,
) -> Result<()> {
    // Validate all hostnames in the cluster
    validate_hostname(&node.host)?;
    for addr in &cluster.join_addresses {
        let host = addr.split(':').next().unwrap_or(addr);
        validate_hostname(host)?;
    }
    
    let certs_dir = "/var/lib/cockroach/certs";
    let ca_key_dir = "/var/lib/cockroach/ca-key"; // CA key stored securely
    
    // Build list of all node addresses for the certificate
    let all_node_addresses: Vec<String> = cluster
        .join_addresses
        .iter()
        .map(|addr| addr.split(':').next().unwrap_or(addr).to_string())
        .collect();
    
    let node_addresses = all_node_addresses.join(",");
    
    // Check if CA already exists (first node creates it, others copy)
    let is_first_node = node.node_id.ends_with("-1");
    
    if is_first_node {
        // First node: create CA and node certificates
        let cert_script = format!(
            r#"
# Create certificate directories
mkdir -p {} {}
chmod 700 {} {}

# Create CA certificate (only on first node)
if [ ! -f {}/ca.crt ]; then
    cockroach cert create-ca \
        --certs-dir={} \
        --ca-key={}/ca.key \
        --lifetime=87600h
    echo "CA certificate created"
fi

# Create node certificate
cockroach cert create-node \
    {} localhost 127.0.0.1 {} \
    --certs-dir={} \
    --ca-key={}/ca.key \
    --lifetime=8760h

# Create client certificate for root user
cockroach cert create-client root \
    --certs-dir={} \
    --ca-key={}/ca.key \
    --lifetime=8760h

# Set proper permissions
chmod 644 {}/*.crt
chmod 600 {}/*.key
chown -R cockroach:cockroach {} {}

echo "Certificates generated successfully"
ls -la {}
"#,
            certs_dir, ca_key_dir,
            certs_dir, ca_key_dir,
            certs_dir,
            certs_dir, ca_key_dir,
            node.host, node_addresses,
            certs_dir, ca_key_dir,
            certs_dir, ca_key_dir,
            certs_dir, certs_dir,
            certs_dir, ca_key_dir,
            certs_dir
        );
        
        let mut cmd = build_ssh_command(&node.host, key)?;
        cmd.arg("bash").arg("-c").arg(&cert_script);
        
        let status = cmd.status()?;
        
        if !status.success() {
            return Err(anyhow::anyhow!(
                "Failed to generate certificates on node {}",
                node.node_id
            ));
        }
    } else {
        // Non-first nodes: copy CA cert from first node, then generate node cert
        let first_node_host = cluster
            .join_addresses
            .first()
            .map(|addr| addr.split(':').next().unwrap_or(addr))
            .ok_or_else(|| anyhow::anyhow!("No join addresses configured"))?;
        
        // Validate first_node_host before using in embedded script
        validate_hostname(first_node_host)?;
        
        let cert_script = format!(
            r#"
# Create certificate directories
mkdir -p {} {}
chmod 700 {} {}

# Copy CA certificate from first node (assumes SSH access between nodes)
# Using accept-new for TOFU (Trust On First Use) model
scp -o StrictHostKeyChecking=accept-new -o BatchMode=yes {}:{}/ca.crt {}/ 
scp -o StrictHostKeyChecking=accept-new -o BatchMode=yes {}:{}/ca.key {}/

# Create node certificate
cockroach cert create-node \
    {} localhost 127.0.0.1 {} \
    --certs-dir={} \
    --ca-key={}/ca.key \
    --lifetime=8760h

# Create client certificate for root user
cockroach cert create-client root \
    --certs-dir={} \
    --ca-key={}/ca.key \
    --lifetime=8760h

# Set proper permissions
chmod 644 {}/*.crt
chmod 600 {}/*.key
chown -R cockroach:cockroach {} {}

echo "Certificates generated successfully"
ls -la {}
"#,
            certs_dir, ca_key_dir,
            certs_dir, ca_key_dir,
            first_node_host, certs_dir, certs_dir,
            first_node_host, ca_key_dir, ca_key_dir,
            node.host, node_addresses,
            certs_dir, ca_key_dir,
            certs_dir, ca_key_dir,
            certs_dir, certs_dir,
            certs_dir, ca_key_dir,
            certs_dir
        );
        
        let mut cmd = build_ssh_command(&node.host, key)?;
        cmd.arg("bash").arg("-c").arg(&cert_script);
        
        let status = cmd.status()?;
        
        if !status.success() {
            return Err(anyhow::anyhow!(
                "Failed to generate certificates on node {}",
                node.node_id
            ));
        }
    }
    
    println!("    ✅ TLS certificates generated for node {}", node.node_id);
    Ok(())
}

async fn start_cockroachdb_node(
    cluster: &CockroachDBConfig,
    node: &CockroachDBNode,
    key: Option<&Path>,
) -> Result<()> {
    // Validate hostnames
    validate_hostname(&node.host)?;
    for addr in &cluster.join_addresses {
        let host = addr.split(':').next().unwrap_or(addr);
        validate_hostname(host)?;
    }
    validate_path(&node.store_path)?;
    
    let join_addresses = cluster.join_addresses.join(",");
    
    // Build the cockroach start command
    let mut start_cmd = format!(
        "cockroach start --store={} --listen-addr={}:{} --http-addr={}:{} --join={}",
        node.store_path,
        node.host,
        cluster.sql_port,
        node.host,
        cluster.http_port,
        join_addresses
    );
    
    // Add TLS options if encryption is enabled
    if cluster.encryption_at_rest {
        start_cmd.push_str(" --certs-dir=/var/lib/cockroach/certs");
    } else {
        start_cmd.push_str(" --insecure");
    }
    
    // Add background flag
    start_cmd.push_str(" --background");
    
    // Create systemd service file for production
    let service_content = format!(
        r#"[Unit]
Description=CockroachDB node {}
After=network.target

[Service]
Type=simple
User=cockroach
Group=cockroach
ExecStart=/usr/local/bin/{}
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
"#,
        node.node_id, start_cmd.replace(" --background", "")
    );
    
    // Write service file and start via systemd
    let setup_script = format!(
        r#"
cat > /etc/systemd/system/cockroachdb.service << 'EOF'
{}
EOF
systemctl daemon-reload
systemctl enable cockroachdb
systemctl start cockroachdb
sleep 5
systemctl status cockroachdb --no-pager
"#,
        service_content
    );
    
    let mut cmd = build_ssh_command(&node.host, key)?;
    cmd.arg("bash").arg("-c").arg(&setup_script);
    
    let status = cmd.status()?;
    
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Failed to start CockroachDB node {} via systemd",
            node.node_id
        ));
    }
    
    println!("    ✅ CockroachDB node {} started via systemd", node.node_id);
    Ok(())
}

async fn initialize_cockroachdb(host: &str, port: u16, key: Option<&Path>) -> Result<()> {
    validate_hostname(host)?;
    
    let init_cmd = format!("cockroach init --host=localhost:{}", port);
    
    let mut cmd = build_ssh_command(host, key)?;
    cmd.arg("bash").arg("-c").arg(&init_cmd);
    
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Failed to initialize CockroachDB on {}", host);
    }
    Ok(())
}

async fn configure_redis_node(
    cluster: &RedisConfig,
    node: &RedisNode,
    key: Option<&Path>,
) -> Result<()> {
    // Validate hostname
    validate_hostname(&node.host)?;
    
    let port = node.address.port();
    let max_memory = format!("{}mb", cluster.max_memory_mb);
    
    // Generate redis.conf for cluster mode
    let redis_config = format!(
        r#"# Redis Cluster Configuration
port {}
cluster-enabled yes
cluster-config-file nodes-{}.conf
cluster-node-timeout 5000
appendonly yes
appendfsync everysec
maxmemory {}
maxmemory-policy allkeys-lru
bind 0.0.0.0
protected-mode no
daemonize no
logfile /var/log/redis/redis-{}.log
dir /var/lib/redis/{}

# Performance tuning
tcp-backlog 511
timeout 0
tcp-keepalive 300
databases 16
save 900 1
save 300 10
save 60 10000
stop-writes-on-bgsave-error yes
rdbcompression yes
rdbchecksum yes
dbfilename dump-{}.rdb
"#,
        port,
        port,
        max_memory,
        port,
        port,
        port
    );
    
    // Create config and directories on remote node
    let setup_script = format!(
        r#"
mkdir -p /etc/redis /var/lib/redis/{} /var/log/redis
cat > /etc/redis/redis-{}.conf << 'EOF'
{}
EOF
chown -R redis:redis /var/lib/redis /var/log/redis /etc/redis
"#,
        port, port, redis_config
    );
    
    let mut cmd = build_ssh_command(&node.host, key)?;
    cmd.arg("bash").arg("-c").arg(&setup_script);
    
    let status = cmd.status()?;
    
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Failed to configure Redis node {}:{}",
            node.host,
            port
        ));
    }
    
    println!("    ✅ Redis configuration created for {}:{}", node.host, port);
    Ok(())
}

async fn start_redis_node(node: &RedisNode, key: Option<&Path>) -> Result<()> {
    // Validate hostname
    validate_hostname(&node.host)?;
    
    let port = node.address.port();
    
    // Create systemd service for Redis node
    let service_content = format!(
        r#"[Unit]
Description=Redis Cluster Node on port {}
After=network.target

[Service]
Type=simple
User=redis
Group=redis
ExecStart=/usr/bin/redis-server /etc/redis/redis-{}.conf
ExecStop=/usr/bin/redis-cli -p {} shutdown
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
"#,
        port, port, port
    );
    
    let setup_script = format!(
        r#"
cat > /etc/systemd/system/redis-{}.service << 'EOF'
{}
EOF
systemctl daemon-reload
systemctl enable redis-{}
systemctl start redis-{}
sleep 2
redis-cli -p {} ping
"#,
        port, service_content, port, port, port
    );
    
    let mut cmd = build_ssh_command(&node.host, key)?;
    cmd.arg("bash").arg("-c").arg(&setup_script);
    
    let status = cmd.status()?;
    
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Failed to start Redis node {}:{}",
            node.host,
            port
        ));
    }
    
    println!("    ✅ Redis node started on {}:{}", node.host, port);
    Ok(())
}

async fn create_redis_cluster(config: &RedisConfig, key: Option<&Path>) -> Result<()> {
    // Validate all hostnames
    for m in &config.masters {
        validate_hostname(&m.host)?;
    }
    for r in &config.replicas {
        validate_hostname(&r.host)?;
    }
    
    // Build the list of master nodes for cluster creation
    let master_nodes: Vec<String> = config
        .masters
        .iter()
        .map(|m| format!("{}:{}", m.host, m.address.port()))
        .collect();
    
    if master_nodes.is_empty() {
        return Err(anyhow::anyhow!("No master nodes configured for Redis cluster"));
    }
    
    // Use the first master to initiate cluster creation
    let first_master = &config.masters[0];
    let first_port = first_master.address.port();
    
    // Create cluster with all masters
    let cluster_create_cmd = format!(
        "redis-cli --cluster create {} --cluster-replicas 0 --cluster-yes",
        master_nodes.join(" ")
    );
    
    let mut cmd = build_ssh_command(&first_master.host, key)?;
    cmd.arg("bash").arg("-c").arg(&cluster_create_cmd);
    
    let status = cmd.status()?;
    
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to create Redis cluster with masters"));
    }
    
    println!("    ✅ Redis cluster created with {} masters", master_nodes.len());
    
    // Add replicas to the cluster
    for replica in &config.replicas {
        let replica_port = replica.address.port();
        
        // Find the master this replica should follow based on master_of field
        let master = if let Some(ref master_id) = replica.master_of {
            config.masters.iter().find(|m| &m.node_id == master_id)
        } else {
            // Fallback: round-robin assignment
            let master_idx = config.replicas.iter().position(|r| r.node_id == replica.node_id).unwrap_or(0)
                % config.masters.len();
            Some(&config.masters[master_idx])
        };
        
        if let Some(master) = master {
            let master_port = master.address.port();
        
            // Get the master node ID
            let get_master_id_cmd = format!(
                "redis-cli -h {} -p {} cluster nodes | grep myself | cut -d' ' -f1",
                master.host, master_port
            );
            
            let mut id_cmd = build_ssh_command(&first_master.host, key)?;
            id_cmd.arg("bash").arg("-c").arg(&get_master_id_cmd);
            
            let master_id_output = id_cmd.output()?;
            
            let master_id = String::from_utf8_lossy(&master_id_output.stdout)
                .trim()
                .to_string();
            
            if !master_id.is_empty() {
                // Add replica to cluster
                let add_replica_cmd = format!(
                    "redis-cli --cluster add-node {}:{} {}:{} --cluster-slave --cluster-master-id {}",
                    replica.host, replica_port, master.host, master_port, master_id
                );
                
                let mut add_cmd = build_ssh_command(&first_master.host, key)?;
                add_cmd.arg("bash").arg("-c").arg(&add_replica_cmd);
                
                let add_status = add_cmd.status()?;
                
                if add_status.success() {
                    println!("    ✅ Added replica {}:{} -> master {}:{}", 
                        replica.host, replica_port, master.host, master_port);
                }
            }
        }
    }
    
    // Verify cluster health
    let check_cmd = format!(
        "redis-cli -h {} -p {} cluster info | grep cluster_state",
        first_master.host, first_port
    );
    
    let mut verify_cmd = build_ssh_command(&first_master.host, key)?;
    verify_cmd.arg("bash").arg("-c").arg(&check_cmd);
    
    let check_output = verify_cmd.output()?;
    
    let cluster_state = String::from_utf8_lossy(&check_output.stdout);
    if cluster_state.contains("cluster_state:ok") {
        println!("    ✅ Redis cluster is healthy");
    } else {
        println!("    ⚠️  Redis cluster state: {}", cluster_state.trim());
    }
    
    Ok(())
}

async fn start_minio_node(
    cluster: &MinIOConfig,
    node: &MinIONode,
    key: Option<&Path>,
) -> Result<()> {
    // Validate hostnames and paths
    validate_hostname(&node.host)?;
    for vol in &node.data_volumes {
        validate_path(vol)?;
    }
    for n in &cluster.nodes {
        validate_hostname(&n.host)?;
    }
    
    let api_port = node.api_address.port();
    
    // Build the drives list for distributed mode
    let drives: Vec<String> = cluster
        .nodes
        .iter()
        .flat_map(|n| {
            let n_port = n.api_address.port();
            n.data_volumes.iter().map(move |d| format!("http://{}:{}{}", n.host, n_port, d))
        })
        .collect();
    
    let drives_arg = drives.join(" ");
    
    // Create MinIO systemd service - use environment variables for credentials
    let service_content = format!(
        r#"[Unit]
Description=MinIO Object Storage
After=network.target

[Service]
Type=simple
User=minio
Group=minio
Environment="MINIO_ROOT_USER={}"
Environment="MINIO_ROOT_PASSWORD=$(cat /etc/minio/minio-secret)"
Environment="MINIO_VOLUMES={}"
ExecStart=/usr/local/bin/minio server --console-address :{} $MINIO_VOLUMES
Restart=always
RestartSec=10
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
"#,
        cluster.root_user,
        drives_arg,
        cluster.console_port
    );
    
    // Create directories for all drives on this node
    let mkdir_cmds: Vec<String> = node
        .data_volumes
        .iter()
        .map(|d| format!("mkdir -p {}", d))
        .collect();
    
    let setup_script = format!(
        r#"
# Create minio user if not exists
id -u minio &>/dev/null || useradd -r -s /sbin/nologin minio

# Create drive directories
{}

# Set ownership
chown -R minio:minio {}

# Install systemd service
cat > /etc/systemd/system/minio.service << 'EOF'
{}
EOF

systemctl daemon-reload
systemctl enable minio
systemctl start minio
sleep 3
systemctl status minio --no-pager
"#,
        mkdir_cmds.join("\n"),
        node.data_volumes.join(" "),
        service_content
    );
    
    let mut cmd = build_ssh_command(&node.host, key)?;
    cmd.arg("bash").arg("-c").arg(&setup_script);
    
    let status = cmd.status()?;
    
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Failed to start MinIO node on {}",
            node.host
        ));
    }
    
    println!("    ✅ MinIO node started on {}:{}", node.host, api_port);
    Ok(())
}

async fn start_tikv_pd(cluster: &TiKVConfig, pd: &TiKVPDNode, key: Option<&Path>) -> Result<()> {
    // Validate hostnames and paths
    validate_hostname(&pd.host)?;
    validate_path(&pd.data_dir)?;
    for p in &cluster.pd_nodes {
        validate_hostname(&p.host)?;
    }
    
    let client_port = pd.client_address.port();
    let peer_port = pd.peer_address.port();
    
    // Build initial cluster string for PD nodes
    let initial_cluster: Vec<String> = cluster
        .pd_nodes
        .iter()
        .map(|p| format!("{}=http://{}:{}", p.node_id, p.host, p.peer_address.port()))
        .collect();
    
    // Generate PD configuration
    let pd_config = format!(
        r#"# TiKV PD Configuration
name = "{}"
data-dir = "{}"
client-urls = "http://{}:{}"
peer-urls = "http://{}:{}"
initial-cluster = "{}"
initial-cluster-state = "new"

[log]
level = "info"

[schedule]
max-store-down-time = "30m"
leader-schedule-limit = 4
region-schedule-limit = 2048
replica-schedule-limit = 64
"#,
        pd.node_id,
        pd.data_dir,
        pd.host,
        client_port,
        pd.host,
        peer_port,
        initial_cluster.join(",")
    );
    
    // Create systemd service for PD
    let service_content = format!(
        r#"[Unit]
Description=TiKV PD Server {}
After=network.target

[Service]
Type=simple
User=tikv
Group=tikv
ExecStart=/usr/local/bin/pd-server --config=/etc/tikv/pd.toml
Restart=always
RestartSec=10
LimitNOFILE=1000000

[Install]
WantedBy=multi-user.target
"#,
        pd.node_id
    );
    
    let setup_script = format!(
        r#"
# Create tikv user if not exists
id -u tikv &>/dev/null || useradd -r -s /sbin/nologin tikv

# Create directories
mkdir -p /etc/tikv {} /var/log/tikv
chown -R tikv:tikv {} /var/log/tikv

# Write PD config
cat > /etc/tikv/pd.toml << 'EOF'
{}
EOF

# Install systemd service
cat > /etc/systemd/system/tikv-pd.service << 'EOF'
{}
EOF

systemctl daemon-reload
systemctl enable tikv-pd
systemctl start tikv-pd
sleep 5
systemctl status tikv-pd --no-pager
"#,
        pd.data_dir,
        pd.data_dir,
        pd_config,
        service_content
    );
    
    let mut cmd = build_ssh_command(&pd.host, key)?;
    cmd.arg("bash").arg("-c").arg(&setup_script);
    
    let status = cmd.status()?;
    
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Failed to start TiKV PD node {}",
            pd.node_id
        ));
    }
    
    println!("    ✅ TiKV PD node {} started on {}:{}", pd.node_id, pd.host, client_port);
    Ok(())
}

async fn start_tikv_storage(
    cluster: &TiKVConfig,
    tikv: &TiKVStorageNode,
    key: Option<&Path>,
) -> Result<()> {
    // Validate hostnames and paths
    validate_hostname(&tikv.host)?;
    validate_path(&tikv.data_dir)?;
    for p in &cluster.pd_nodes {
        validate_hostname(&p.host)?;
    }
    
    let tikv_port = tikv.address.port();
    let status_port = tikv.status_address.port();
    
    // Build PD endpoints string
    let pd_endpoints: Vec<String> = cluster
        .pd_nodes
        .iter()
        .map(|p| format!("http://{}:{}", p.host, p.client_address.port()))
        .collect();
    
    // Generate TiKV configuration
    let tikv_config = format!(
        r#"# TiKV Storage Configuration
[server]
addr = "{}:{}"
status-addr = "{}:{}"

[storage]
data-dir = "{}"

[pd]
endpoints = [{}]

[raftstore]
capacity = "100GB"
raft-base-tick-interval = "1s"
raft-heartbeat-ticks = 2
raft-election-timeout-ticks = 10

[rocksdb]
max-open-files = 40960

[rocksdb.defaultcf]
block-cache-size = "1GB"

[rocksdb.writecf]
block-cache-size = "256MB"

[rocksdb.raftcf]
block-cache-size = "128MB"

[raftdb]
max-open-files = 40960
"#,
        tikv.host,
        tikv_port,
        tikv.host,
        status_port,
        tikv.data_dir,
        pd_endpoints.iter().map(|e| format!("\"{}\"", e)).collect::<Vec<_>>().join(", ")
    );
    
    // Create systemd service for TiKV
    let service_content = format!(
        r#"[Unit]
Description=TiKV Storage Server {}:{}
After=network.target tikv-pd.service

[Service]
Type=simple
User=tikv
Group=tikv
ExecStart=/usr/local/bin/tikv-server --config=/etc/tikv/tikv.toml
Restart=always
RestartSec=10
LimitNOFILE=1000000

[Install]
WantedBy=multi-user.target
"#,
        tikv.host, tikv_port
    );
    
    let setup_script = format!(
        r#"
# Create tikv user if not exists
id -u tikv &>/dev/null || useradd -r -s /sbin/nologin tikv

# Create directories
mkdir -p /etc/tikv {} /var/log/tikv
chown -R tikv:tikv {} /var/log/tikv

# Write TiKV config
cat > /etc/tikv/tikv.toml << 'EOF'
{}
EOF

# Install systemd service
cat > /etc/systemd/system/tikv-storage.service << 'EOF'
{}
EOF

systemctl daemon-reload
systemctl enable tikv-storage
systemctl start tikv-storage
sleep 5
systemctl status tikv-storage --no-pager
"#,
        tikv.data_dir,
        tikv.data_dir,
        tikv_config,
        service_content
    );
    
    let mut cmd = build_ssh_command(&tikv.host, key)?;
    cmd.arg("bash").arg("-c").arg(&setup_script);
    
    let status = cmd.status()?;
    
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Failed to start TiKV storage node on {}:{}",
            tikv.host,
            tikv_port
        ));
    }
    
    println!("    ✅ TiKV storage node started on {}:{}", tikv.host, tikv_port);
    Ok(())
}

async fn check_cockroachdb_health(host: &str, port: u16) -> Result<()> {
    let url = format!("http://{}:{}/health", host, port);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("CockroachDB health check failed: {}", response.status())
    }
}

async fn check_redis_health(host: &str, port: u16) -> Result<()> {
    // In production: use redis-rs crate
    let output = StdCommand::new("redis-cli")
        .args(&["-h", host, "-p", &port.to_string(), "PING"])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!("Redis health check failed")
    }
}

async fn check_minio_health(host: &str, port: u16) -> Result<()> {
    let url = format!("http://{}:{}/minio/health/live", host, port);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("MinIO health check failed: {}", response.status())
    }
}

async fn check_tikv_pd_health(host: &str, port: u16) -> Result<()> {
    let url = format!("http://{}:{}/pd/health", host, port);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("TiKV PD health check failed: {}", response.status())
    }
}

async fn check_tikv_storage_health(host: &str, port: u16) -> Result<()> {
    let url = format!("http://{}:{}/status", host, port);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("TiKV storage health check failed: {}", response.status())
    }
}

fn estimate_cost(config: &DistributedStorageConfig) -> f64 {
    // Rough estimate based on cloud provider pricing
    // CockroachDB: 5 nodes × $150/month = $750
    // Redis: 6 nodes × $50/month = $300
    // MinIO: 4 nodes × $100/month = $400
    // TiKV: 8 nodes × $100/month = $800
    // Total: ~$2,250/month

    let cockroach_cost = config.cockroachdb.nodes.len() as f64 * 150.0;
    let redis_cost = (config.redis.masters.len() + config.redis.replicas.len()) as f64 * 50.0;
    let minio_cost = config.minio.nodes.len() as f64 * 100.0;
    let tikv_cost = (config.tikv.pd_nodes.len() + config.tikv.tikv_nodes.len()) as f64 * 100.0;

    cockroach_cost + redis_cost + minio_cost + tikv_cost
}
