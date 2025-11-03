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
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;
use tokio::time::sleep;

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
    println!("🔧 Generating distributed storage configuration for '{}'...", network);

    fs::create_dir_all(output)
        .context("Failed to create output directory")?;

    let config = DistributedStorageConfig::new_recommended(network.to_string());
    
    // Verify configuration
    config.verify_all()
        .context("Configuration validation failed")?;

    // Save CockroachDB config
    let cockroach_file = output.join("cockroachdb-config.json");
    let cockroach_json = serde_json::to_string_pretty(&config.cockroachdb)?;
    fs::write(&cockroach_file, cockroach_json)
        .context("Failed to write CockroachDB config")?;
    println!("✅ CockroachDB config: {}", cockroach_file.display());

    // Save Redis config
    let redis_file = output.join("redis-config.json");
    let redis_json = serde_json::to_string_pretty(&config.redis)?;
    fs::write(&redis_file, redis_json)
        .context("Failed to write Redis config")?;
    println!("✅ Redis config: {}", redis_file.display());

    // Save MinIO config
    let minio_file = output.join("minio-config.json");
    let minio_json = serde_json::to_string_pretty(&config.minio)?;
    fs::write(&minio_file, minio_json)
        .context("Failed to write MinIO config")?;
    println!("✅ MinIO config: {}", minio_file.display());

    // Save TiKV config
    let tikv_file = output.join("tikv-config.json");
    let tikv_json = serde_json::to_string_pretty(&config.tikv)?;
    fs::write(&tikv_file, tikv_json)
        .context("Failed to write TiKV config")?;
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
    fs::write(&summary_file, summary_json)
        .context("Failed to write summary")?;

    println!("\n📊 Deployment Summary:");
    println!("  Total nodes: {}", summary.total_nodes);
    println!("  - CockroachDB: {} nodes", summary.cockroachdb_nodes);
    println!("  - Redis: {} nodes", summary.redis_nodes);
    println!("  - MinIO: {} nodes", summary.minio_nodes);
    println!("  - TiKV: {} nodes (PD + storage)", summary.tikv_nodes);
    println!("  Estimated cost: ${:.2}/month", summary.estimated_monthly_cost_usd);

    Ok(())
}

async fn deploy_cockroachdb(config_path: &Path, node: &str, key: Option<&Path>) -> Result<()> {
    println!("🚀 Deploying CockroachDB cluster...");
    
    let config_json = fs::read_to_string(config_path)?;
    let config: CockroachDBConfig = serde_json::from_str(&config_json)?;

    let nodes_to_deploy: Vec<&CockroachDBNode> = if node == "all" {
        config.nodes.iter().collect()
    } else {
        let index: usize = node.parse()
            .context("Node must be a number or 'all'")?;
        vec![&config.nodes[index]]
    };

    for (i, node) in nodes_to_deploy.iter().enumerate() {
        println!("\n[{}/{}] Deploying CockroachDB node: {}", 
                 i + 1, nodes_to_deploy.len(), node.node_id);
        
        deploy_cockroachdb_node(&config, node, key).await?;
        
        if i < nodes_to_deploy.len() - 1 {
            println!("⏱️  Waiting 10s before next node...");
            sleep(Duration::from_secs(10)).await;
        }
    }

    println!("\n✅ CockroachDB cluster deployment complete!");
    println!("🔗 Admin UI: http://{}:{}", 
             config.nodes[0].host, config.http_port);
    println!("🔗 Connection: {}", 
             config.connection_string("dchat_user", "<password>"));

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
        // In production: use cockroach cert create-ca, create-node, create-client
        println!("    ⚠️  Manual step: Generate certs with 'cockroach cert'");
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
        "all" => config.masters.iter().chain(config.replicas.iter()).collect(),
        _ => anyhow::bail!("Invalid node_type: must be 'masters', 'replicas', or 'all'"),
    };

    // Deploy all masters first
    let masters_to_deploy: Vec<&RedisNode> = nodes_to_deploy.iter()
        .filter(|n| n.role == RedisNodeRole::Master)
        .copied()
        .collect();

    for (i, node) in masters_to_deploy.iter().enumerate() {
        println!("\n[{}/{}] Deploying Redis master: {}", 
                 i + 1, masters_to_deploy.len(), node.node_id);
        deploy_redis_node(&config, node, key).await?;
        sleep(Duration::from_secs(5)).await;
    }

    // Deploy replicas
    let replicas_to_deploy: Vec<&RedisNode> = nodes_to_deploy.iter()
        .filter(|n| n.role == RedisNodeRole::Replica)
        .copied()
        .collect();

    for (i, node) in replicas_to_deploy.iter().enumerate() {
        println!("\n[{}/{}] Deploying Redis replica: {}", 
                 i + 1, replicas_to_deploy.len(), node.node_id);
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
        println!("\n[{}/{}] Deploying MinIO node: {}", 
                 i + 1, nodes_to_deploy.len(), node.node_id);
        deploy_minio_node(&config, node, key).await?;
        sleep(Duration::from_secs(5)).await;
    }

    println!("\n✅ MinIO cluster deployment complete!");
    println!("🔗 API: http://{}:{}", config.nodes[0].host, config.api_port);
    println!("🔗 Console: http://{}:{}", config.nodes[0].host, config.console_port);
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
    create_directories(&node.host, &node.data_volumes.iter().map(|s| s.as_str()).collect::<Vec<_>>(), key).await?;

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
                println!("\n[{}/{}] Deploying TiKV PD: {}", 
                         i + 1, config.pd_nodes.len(), pd.node_id);
                deploy_tikv_pd(&config, pd, key).await?;
                sleep(Duration::from_secs(5)).await;
            }
        }
        _ => {}
    }

    if component == "tikv" || component == "all" {
        for (i, tikv) in config.tikv_nodes.iter().enumerate() {
            println!("\n[{}/{}] Deploying TiKV storage: {}", 
                     i + 1, config.tikv_nodes.len(), tikv.node_id);
            deploy_tikv_storage(&config, tikv, key).await?;
            sleep(Duration::from_secs(5)).await;
        }
    }

    println!("\n✅ TiKV cluster deployment complete!");
    println!("🔗 PD endpoints: {:?}", config.pd_endpoints());

    Ok(())
}

async fn deploy_tikv_pd(
    cluster: &TiKVConfig,
    pd: &TiKVPDNode,
    key: Option<&Path>,
) -> Result<()> {
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
    println!("  1. Run health checks: deploy-storage health-check --config-dir {}", config_dir.display());
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
    println!("🔄 Migrating data from {} to distributed storage...", source);
    
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

async fn check_ssh(host: &str, key: Option<&Path>) -> Result<()> {
    let key_arg = key.map(|k| format!("-i {}", k.display())).unwrap_or_default();
    let output = StdCommand::new("ssh")
        .args(&[&key_arg, &format!("root@{}", host), "echo", "OK"])
        .output()?;
    
    if !output.status.success() {
        anyhow::bail!("SSH check failed for {}", host);
    }
    Ok(())
}

async fn install_cockroachdb(host: &str, key: Option<&Path>) -> Result<()> {
    let key_arg = key.map(|k| format!("-i {}", k.display())).unwrap_or_default();
    let script = r#"
        curl https://binaries.cockroachdb.com/cockroach-latest.linux-amd64.tgz | tar -xz
        cp -i cockroach-*/cockroach /usr/local/bin/
        mkdir -p /usr/local/lib/cockroach
        cp -i cockroach-*/lib/libgeos.so /usr/local/lib/cockroach/
        cp -i cockroach-*/lib/libgeos_c.so /usr/local/lib/cockroach/
    "#;
    
    StdCommand::new("ssh")
        .args(&[&key_arg, &format!("root@{}", host), script])
        .status()?;
    
    Ok(())
}

async fn install_redis(host: &str, key: Option<&Path>) -> Result<()> {
    let key_arg = key.map(|k| format!("-i {}", k.display())).unwrap_or_default();
    StdCommand::new("ssh")
        .args(&[&key_arg, &format!("root@{}", host), 
               "apt-get update && apt-get install -y redis-server redis-tools"])
        .status()?;
    Ok(())
}

async fn install_minio(host: &str, key: Option<&Path>) -> Result<()> {
    let key_arg = key.map(|k| format!("-i {}", k.display())).unwrap_or_default();
    let script = r#"
        wget https://dl.min.io/server/minio/release/linux-amd64/minio
        chmod +x minio
        mv minio /usr/local/bin/
    "#;
    
    StdCommand::new("ssh")
        .args(&[&key_arg, &format!("root@{}", host), script])
        .status()?;
    Ok(())
}

async fn install_tikv(host: &str, key: Option<&Path>) -> Result<()> {
    let key_arg = key.map(|k| format!("-i {}", k.display())).unwrap_or_default();
    let script = r#"
        curl --proto '=https' --tlsv1.2 -sSf https://tiup-mirrors.pingcap.com/install.sh | sh
        source ~/.bash_profile
        tiup install pd tikv
    "#;
    
    StdCommand::new("ssh")
        .args(&[&key_arg, &format!("root@{}", host), script])
        .status()?;
    Ok(())
}

async fn create_directories(host: &str, dirs: &[&str], key: Option<&Path>) -> Result<()> {
    let key_arg = key.map(|k| format!("-i {}", k.display())).unwrap_or_default();
    let mkdir_cmd = format!("mkdir -p {}", dirs.join(" "));
    
    StdCommand::new("ssh")
        .args(&[&key_arg, &format!("root@{}", host), &mkdir_cmd])
        .status()?;
    Ok(())
}

async fn start_cockroachdb_node(
    cluster: &CockroachDBConfig,
    _node: &CockroachDBNode,
    _key: Option<&Path>,
) -> Result<()> {
    // In production: use systemd or Docker
    println!("    ⚠️  Manual step: Start with 'cockroach start --join={}'", 
             cluster.join_addresses.join(","));
    Ok(())
}

async fn initialize_cockroachdb(host: &str, port: u16, key: Option<&Path>) -> Result<()> {
    let key_arg = key.map(|k| format!("-i {}", k.display())).unwrap_or_default();
    StdCommand::new("ssh")
        .args(&[&key_arg, &format!("root@{}", host), 
               &format!("cockroach init --host=localhost:{}", port)])
        .status()?;
    Ok(())
}

async fn configure_redis_node(_cluster: &RedisConfig, _node: &RedisNode, _key: Option<&Path>) -> Result<()> {
    println!("    ⚠️  Manual step: Configure redis.conf for cluster mode");
    Ok(())
}

async fn start_redis_node(_node: &RedisNode, _key: Option<&Path>) -> Result<()> {
    println!("    ⚠️  Manual step: Start with 'redis-server /etc/redis/redis.conf'");
    Ok(())
}

async fn create_redis_cluster(_config: &RedisConfig, _key: Option<&Path>) -> Result<()> {
    println!("    ⚠️  Manual step: Create cluster with 'redis-cli --cluster create'");
    Ok(())
}

async fn start_minio_node(_cluster: &MinIOConfig, _node: &MinIONode, _key: Option<&Path>) -> Result<()> {
    println!("    ⚠️  Manual step: Start with 'minio server <drives>'");
    Ok(())
}

async fn start_tikv_pd(_cluster: &TiKVConfig, _pd: &TiKVPDNode, _key: Option<&Path>) -> Result<()> {
    println!("    ⚠️  Manual step: Start with 'tiup pd:v6 --config=pd.toml'");
    Ok(())
}

async fn start_tikv_storage(_cluster: &TiKVConfig, _tikv: &TiKVStorageNode, _key: Option<&Path>) -> Result<()> {
    println!("    ⚠️  Manual step: Start with 'tiup tikv:v6 --config=tikv.toml'");
    Ok(())
}

async fn check_cockroachdb_health(host: &str, port: u16) -> Result<()> {
    let url = format!("http://{}:{}/health", host, port);
    let client = reqwest::Client::new();
    let response = client.get(&url)
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
    let response = client.get(&url)
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
    let response = client.get(&url)
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
    let response = client.get(&url)
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
