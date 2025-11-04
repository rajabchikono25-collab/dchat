// Automated Multi-Region Validator Deployment Script
// Provisions servers, deploys validators, and configures distributed infrastructure

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dchat_deployment::MultiRegionConfig;
use std::path::PathBuf;
use std::process::Command;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "deploy-validators")]
#[command(about = "Deploy dchat validators across multiple geographic regions")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate multi-region configuration files
    GenerateConfig {
        /// Network name (e.g., dchat-mainnet, dchat-testnet)
        #[arg(short, long)]
        network: String,

        /// Base domain (e.g., dchat.network)
        #[arg(short, long)]
        domain: String,

        /// Output directory for configuration files
        #[arg(short, long, default_value = "./config/validators")]
        output: PathBuf,
    },

    /// Deploy validator to a specific region
    DeployValidator {
        /// Validator ID to deploy
        #[arg(short, long)]
        validator_id: String,

        /// Configuration file path
        #[arg(short, long)]
        config: PathBuf,

        /// Server IP address or hostname
        #[arg(short, long)]
        server: String,

        /// SSH user
        #[arg(short, long, default_value = "root")]
        user: String,

        /// SSH key path
        #[arg(short, long)]
        key: Option<PathBuf>,
    },

    /// Deploy all validators from configuration
    DeployAll {
        /// Configuration file path
        #[arg(short, long)]
        config: PathBuf,

        /// Server mapping file (JSON: validator_id -> server_ip)
        #[arg(short, long)]
        servers: PathBuf,

        /// SSH user
        #[arg(short, long, default_value = "root")]
        user: String,

        /// SSH key path
        #[arg(short, long)]
        key: Option<PathBuf>,

        /// Deploy in parallel (default: sequential for safety)
        #[arg(short, long)]
        parallel: bool,
    },

    /// Health check for deployed validators
    HealthCheck {
        /// Configuration file path
        #[arg(short, long)]
        config: PathBuf,

        /// Timeout in seconds
        #[arg(short, long, default_value = "60")]
        timeout: u64,
    },

    /// Generate Kubernetes manifests
    GenerateK8s {
        /// Configuration file path
        #[arg(short, long)]
        config: PathBuf,

        /// Output directory for K8s manifests
        #[arg(short, long, default_value = "./k8s")]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::GenerateConfig {
            network,
            domain,
            output,
        } => generate_config(network, domain, output).await?,

        Commands::DeployValidator {
            validator_id,
            config,
            server,
            user,
            key,
        } => deploy_validator(&validator_id, &config, &server, &user, key.as_ref()).await?,

        Commands::DeployAll {
            config,
            servers,
            user,
            key,
            parallel,
        } => deploy_all(&config, &servers, &user, key.as_ref(), parallel).await?,

        Commands::HealthCheck { config, timeout } => health_check(&config, timeout).await?,

        Commands::GenerateK8s { config, output } => generate_k8s(&config, output).await?,
    }

    Ok(())
}

/// Generate multi-region configuration files
async fn generate_config(network: String, domain: String, output: PathBuf) -> Result<()> {
    info!(
        "Generating multi-region configuration for {} on {}",
        network, domain
    );

    let config = MultiRegionConfig::new_recommended(network.clone(), domain.clone());

    // Verify configuration meets requirements
    config
        .verify_decentralization()
        .context("Configuration failed decentralization requirements")?;

    // Create output directory
    tokio::fs::create_dir_all(&output)
        .await
        .context("Failed to create output directory")?;

    // Generate TOML files for each validator
    for validator in &config.validators {
        let validator_toml = config
            .generate_toml(&validator.validator_id)
            .context("Failed to generate TOML")?;

        let output_file = output.join(format!("{}.toml", validator.validator_id));
        tokio::fs::write(&output_file, validator_toml)
            .await
            .context(format!("Failed to write {:?}", output_file))?;

        info!("Generated configuration: {:?}", output_file);
    }

    // Generate summary JSON
    let summary = serde_json::to_string_pretty(&config)?;
    let summary_file = output.join("deployment-summary.json");
    tokio::fs::write(&summary_file, summary)
        .await
        .context("Failed to write summary")?;

    info!(
        "✅ Generated {} validator configurations in {:?}",
        config.validators.len(),
        output
    );
    info!("   - BFT threshold: {:.1}%", config.bft_threshold * 100.0);
    info!("   - Geographic regions: {}", count_unique_regions(&config));
    info!(
        "   - Required signatures: {} of {}",
        config.validators[0].consensus.required_signatures,
        config.validators[0].consensus.total_validators
    );

    Ok(())
}

/// Deploy a single validator to a server
async fn deploy_validator(
    validator_id: &str,
    config_path: &PathBuf,
    server: &str,
    user: &str,
    key: Option<&PathBuf>,
) -> Result<()> {
    info!("Deploying validator {} to {}", validator_id, server);

    // Load configuration
    let config_toml = tokio::fs::read_to_string(config_path).await?;
    let _config: toml::Value = toml::from_str(&config_toml)?;

    // Step 1: Check server connectivity
    info!("Step 1/7: Checking server connectivity...");
    check_ssh_connectivity(server, user, key).await?;

    // Step 2: Install dependencies
    info!("Step 2/7: Installing dependencies...");
    install_dependencies(server, user, key).await?;

    // Step 3: Copy validator configuration
    info!("Step 3/7: Copying validator configuration...");
    copy_config(server, user, key, config_path, validator_id).await?;

    // Step 4: Generate validator keys
    info!("Step 4/7: Generating validator keys...");
    generate_keys(server, user, key, validator_id).await?;

    // Step 5: Deploy validator container
    info!("Step 5/7: Deploying validator container...");
    deploy_container(server, user, key, validator_id).await?;

    // Step 6: Wait for startup
    info!("Step 6/7: Waiting for validator startup...");
    sleep(Duration::from_secs(10)).await;

    // Step 7: Health check
    info!("Step 7/7: Performing health check...");
    check_validator_health(server, validator_id).await?;

    info!(
        "✅ Successfully deployed validator {} to {}",
        validator_id, server
    );

    Ok(())
}

/// Deploy all validators from configuration
async fn deploy_all(
    config_path: &PathBuf,
    servers_path: &PathBuf,
    user: &str,
    key: Option<&PathBuf>,
    parallel: bool,
) -> Result<()> {
    info!("Deploying all validators from {:?}", config_path);

    // Load server mapping
    let servers_json = tokio::fs::read_to_string(servers_path).await?;
    let servers: serde_json::Value = serde_json::from_str(&servers_json)?;

    // Load configuration summary
    let config_dir = config_path.parent().unwrap();
    let summary_path = config_dir.join("deployment-summary.json");
    let summary_json = tokio::fs::read_to_string(&summary_path).await?;
    let config: MultiRegionConfig = serde_json::from_str(&summary_json)?;

    if parallel {
        info!(
            "Deploying {} validators in PARALLEL mode",
            config.validators.len()
        );
        warn!("Parallel deployment is faster but riskier. Use sequential for production.");

        let mut tasks = Vec::new();
        for validator in &config.validators {
            let server = servers[&validator.validator_id]
                .as_str()
                .context("Server not found in mapping")?
                .to_string();
            let validator_config = config_dir.join(format!("{}.toml", validator.validator_id));
            let validator_id = validator.validator_id.clone();
            let user = user.to_string();
            let key = key.cloned();

            let task = tokio::spawn(async move {
                deploy_validator(
                    &validator_id,
                    &validator_config,
                    &server,
                    &user,
                    key.as_ref(),
                )
                .await
            });
            tasks.push(task);
        }

        // Wait for all deployments
        for task in tasks {
            task.await??;
        }
    } else {
        info!(
            "Deploying {} validators in SEQUENTIAL mode",
            config.validators.len()
        );

        for validator in &config.validators {
            let server = servers[&validator.validator_id]
                .as_str()
                .context("Server not found in mapping")?;
            let validator_config = config_dir.join(format!("{}.toml", validator.validator_id));

            deploy_validator(
                &validator.validator_id,
                &validator_config,
                server,
                user,
                key,
            )
            .await?;

            // Wait between deployments to avoid overload
            sleep(Duration::from_secs(5)).await;
        }
    }

    info!("✅ Successfully deployed all validators");

    Ok(())
}

/// Perform health check on all validators
async fn health_check(config_path: &PathBuf, timeout_secs: u64) -> Result<()> {
    info!("Performing health check (timeout: {}s)", timeout_secs);

    let config_dir = config_path.parent().unwrap();
    let summary_path = config_dir.join("deployment-summary.json");
    let summary_json = tokio::fs::read_to_string(&summary_path).await?;
    let config: MultiRegionConfig = serde_json::from_str(&summary_json)?;

    let mut healthy = 0;
    let mut unhealthy = 0;

    for validator in &config.validators {
        let rpc_url = format!(
            "http://{}:{}/health",
            validator.public_address,
            validator.rpc_address.port()
        );

        match check_health_endpoint(&rpc_url, timeout_secs).await {
            Ok(true) => {
                info!("✅ {} is HEALTHY", validator.validator_id);
                healthy += 1;
            }
            Ok(false) => {
                error!("❌ {} is UNHEALTHY", validator.validator_id);
                unhealthy += 1;
            }
            Err(e) => {
                error!("❌ {} is UNREACHABLE: {}", validator.validator_id, e);
                unhealthy += 1;
            }
        }
    }

    info!(
        "Health check complete: {} healthy, {} unhealthy",
        healthy, unhealthy
    );

    // Check BFT threshold
    let required = config.validators[0].consensus.required_signatures;
    if healthy < required {
        error!(
            "⚠️  CRITICAL: Only {}/{} validators healthy, below BFT threshold!",
            healthy, required
        );
        anyhow::bail!("Insufficient healthy validators for consensus");
    } else {
        info!(
            "✅ Network has {}/{} healthy validators (above BFT threshold)",
            healthy,
            config.validators.len()
        );
    }

    Ok(())
}

/// Generate Kubernetes manifests
async fn generate_k8s(config_path: &PathBuf, output: PathBuf) -> Result<()> {
    info!("Generating Kubernetes manifests from {:?}", config_path);

    let summary_json = tokio::fs::read_to_string(config_path).await?;
    let config: MultiRegionConfig = serde_json::from_str(&summary_json)?;

    tokio::fs::create_dir_all(&output).await?;

    // Generate StatefulSet manifest
    let statefulset = generate_statefulset_yaml(&config)?;
    let statefulset_path = output.join("validator-statefulset.yaml");
    tokio::fs::write(&statefulset_path, statefulset).await?;
    info!("Generated: {:?}", statefulset_path);

    // Generate Service manifest
    let service = generate_service_yaml(&config)?;
    let service_path = output.join("validator-service.yaml");
    tokio::fs::write(&service_path, service).await?;
    info!("Generated: {:?}", service_path);

    // Generate ConfigMap for each validator
    for validator in &config.validators {
        let configmap = generate_configmap_yaml(&config, &validator.validator_id)?;
        let configmap_path = output.join(format!("{}-configmap.yaml", validator.validator_id));
        tokio::fs::write(&configmap_path, configmap).await?;
    }

    info!("✅ Generated Kubernetes manifests in {:?}", output);

    Ok(())
}

// Helper functions

async fn check_ssh_connectivity(server: &str, user: &str, key: Option<&PathBuf>) -> Result<()> {
    let mut cmd = Command::new("ssh");
    cmd.arg(format!("{}@{}", user, server))
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("StrictHostKeyChecking=no");

    if let Some(key_path) = key {
        cmd.arg("-i").arg(key_path);
    }

    cmd.arg("echo 'OK'");

    let output = cmd.output().context("SSH connection failed")?;

    if !output.status.success() {
        anyhow::bail!(
            "SSH connection test failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

async fn install_dependencies(server: &str, user: &str, key: Option<&PathBuf>) -> Result<()> {
    let script = r#"
        apt-get update && apt-get install -y docker.io docker-compose curl
        systemctl start docker
        systemctl enable docker
    "#;

    execute_remote_command(server, user, key, script).await
}

async fn copy_config(
    server: &str,
    user: &str,
    key: Option<&PathBuf>,
    config_path: &PathBuf,
    validator_id: &str,
) -> Result<()> {
    let mut cmd = Command::new("scp");
    if let Some(key_path) = key {
        cmd.arg("-i").arg(key_path);
    }

    cmd.arg(config_path)
        .arg(format!("{}@{}:/data/{}.toml", user, server, validator_id));

    let output = cmd.output()?;
    if !output.status.success() {
        anyhow::bail!("SCP failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(())
}

async fn generate_keys(
    server: &str,
    user: &str,
    key: Option<&PathBuf>,
    validator_id: &str,
) -> Result<()> {
    let script = format!(
        r#"
        mkdir -p /data/keys
        dchat-keygen --output /data/keys/{}.key
        "#,
        validator_id
    );

    execute_remote_command(server, user, key, &script).await
}

async fn deploy_container(
    server: &str,
    user: &str,
    key: Option<&PathBuf>,
    validator_id: &str,
) -> Result<()> {
    let script = format!(
        r#"
        docker run -d --name dchat-validator-{} \
          --restart unless-stopped \
          -p 7070:7070 -p 9545:9545 \
          -v /data:/data \
          -e RUST_LOG=info \
          dchat/validator:latest \
          --config /data/{}.toml
        "#,
        validator_id, validator_id
    );

    execute_remote_command(server, user, key, &script).await
}

async fn check_validator_health(server: &str, _validator_id: &str) -> Result<()> {
    let url = format!("http://{}:9545/health", server);
    check_health_endpoint(&url, 60).await?;
    Ok(())
}

async fn check_health_endpoint(url: &str, timeout_secs: u64) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()?;

    let response = client.get(url).send().await?;
    Ok(response.status().is_success())
}

async fn execute_remote_command(
    server: &str,
    user: &str,
    key: Option<&PathBuf>,
    script: &str,
) -> Result<()> {
    let mut cmd = Command::new("ssh");
    cmd.arg(format!("{}@{}", user, server));

    if let Some(key_path) = key {
        cmd.arg("-i").arg(key_path);
    }

    cmd.arg(script);

    let output = cmd.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "Remote command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

fn count_unique_regions(config: &MultiRegionConfig) -> usize {
    let mut regions = std::collections::HashSet::new();
    for validator in &config.validators {
        regions.insert(validator.region);
    }
    regions.len()
}

fn generate_statefulset_yaml(config: &MultiRegionConfig) -> Result<String> {
    Ok(format!(
        r#"apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: dchat-validator
  namespace: dchat-prod
spec:
  serviceName: dchat-validator
  replicas: {}
  selector:
    matchLabels:
      app: dchat-validator
  template:
    metadata:
      labels:
        app: dchat-validator
    spec:
      affinity:
        podAntiAffinity:
          requiredDuringSchedulingIgnoredDuringExecution:
          - labelSelector:
              matchExpressions:
              - key: app
                operator: In
                values:
                - dchat-validator
            topologyKey: topology.kubernetes.io/region
      containers:
      - name: validator
        image: dchat/validator:latest
        ports:
        - containerPort: 7070
          name: p2p
        - containerPort: 9545
          name: rpc
        volumeMounts:
        - name: validator-data
          mountPath: /data
        resources:
          requests:
            cpu: "2000m"
            memory: "4Gi"
          limits:
            cpu: "4000m"
            memory: "8Gi"
  volumeClaimTemplates:
  - metadata:
      name: validator-data
    spec:
      accessModes: ["ReadWriteOnce"]
      storageClassName: fast-ssd
      resources:
        requests:
          storage: 100Gi
"#,
        config.validators.len()
    ))
}

fn generate_service_yaml(_config: &MultiRegionConfig) -> Result<String> {
    Ok(r#"apiVersion: v1
kind: Service
metadata:
  name: dchat-validator
  namespace: dchat-prod
spec:
  clusterIP: None
  selector:
    app: dchat-validator
  ports:
  - name: p2p
    port: 7070
    targetPort: 7070
  - name: rpc
    port: 9545
    targetPort: 9545
"#
    .to_string())
}

fn generate_configmap_yaml(config: &MultiRegionConfig, validator_id: &str) -> Result<String> {
    let toml = config.generate_toml(validator_id)?;
    Ok(format!(
        r#"apiVersion: v1
kind: ConfigMap
metadata:
  name: {}-config
  namespace: dchat-prod
data:
  config.toml: |
{}
"#,
        validator_id,
        toml.lines()
            .map(|line| format!("    {}", line))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
