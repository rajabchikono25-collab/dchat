//! Mainnet production configuration generator
//!
//! Generates production-ready config files for:
//! - Validators with DNS discovery
//! - Relay nodes
//! - Storage clusters (Redis, MinIO, TiKV, CockroachDB)
//! - Monitoring and health checks

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Mainnet configuration for a single server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainnetServerConfig {
    /// Server region identifier
    pub region: String,

    /// Server subdomain
    pub subdomain: String,

    /// Public IP (if static, otherwise resolved from DNS)
    pub public_ip: Option<String>,

    /// Node role
    pub role: NodeRole,

    /// Validator configuration (if role includes validator)
    pub validator: Option<ValidatorConfig>,

    /// Relay configurations (typically 2 per server)
    pub relays: Vec<RelayConfig>,

    /// Storage cluster configuration
    pub storage: StorageClusterConfig,

    /// TLS certificate paths
    pub tls: TlsConfig,

    /// Monitoring configuration
    pub monitoring: MonitoringConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    Validator,
    ValidatorWithRelays,
    RelayOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorConfig {
    /// Validator key file path
    pub key_file: String,

    /// Chain RPC endpoint
    pub chain_rpc: String,

    /// Initial stake amount
    pub stake: u64,

    /// Enable block production
    pub producer: bool,

    /// Listen port for validator p2p
    pub listen_port: u16,

    /// Health check port
    pub health_port: u16,

    /// Metrics port
    pub metrics_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    /// Relay identifier (relay1, relay2)
    pub identifier: String,

    /// Listen port
    pub listen_port: u16,

    /// Health check port
    pub health_port: u16,

    /// Metrics port
    pub metrics_port: u16,

    /// Minimum stake for relay incentives
    pub min_stake: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageClusterConfig {
    /// Redis cluster configuration
    pub redis: RedisClusterConfig,

    /// MinIO distributed object storage
    pub minio: MinioClusterConfig,

    /// TiKV distributed KV store (only on primary regions)
    pub tikv: Option<TikvClusterConfig>,

    /// CockroachDB connection (cloud-managed)
    pub cockroachdb: CockroachDbConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisClusterConfig {
    /// Redis port
    pub port: u16,

    /// Cluster mode enabled
    pub cluster_mode: bool,

    /// Cluster nodes (other servers' Redis endpoints)
    pub cluster_nodes: Vec<String>,

    /// Password (loaded from environment variable)
    pub password_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinioClusterConfig {
    /// MinIO API port
    pub port: u16,

    /// Console port
    pub console_port: u16,

    /// Distributed mode endpoints (all 7 servers)
    pub endpoints: Vec<String>,

    /// Access key (loaded from environment)
    pub access_key_env: String,

    /// Secret key (loaded from environment)
    pub secret_key_env: String,

    /// Number of drives per server
    pub drives_per_server: usize,

    /// Data directory base path
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TikvClusterConfig {
    /// PD (Placement Driver) port
    pub pd_port: u16,

    /// TiKV server port
    pub tikv_port: u16,

    /// PD endpoints (3 primary regions)
    pub pd_endpoints: Vec<String>,

    /// This server is PD leader
    pub is_pd: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CockroachDbConfig {
    /// Connection URL (loaded from environment)
    pub connection_url_env: String,

    /// Max connections
    pub max_connections: u32,

    /// TLS mode
    pub tls_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// TLS enabled
    pub enabled: bool,

    /// Certificate file path
    pub cert_path: String,

    /// Private key file path
    pub key_path: String,

    /// CA certificate path
    pub ca_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Prometheus enabled
    pub prometheus_enabled: bool,

    /// Prometheus scrape port
    pub prometheus_port: u16,

    /// Jaeger tracing endpoint
    pub jaeger_endpoint: Option<String>,

    /// Log level
    pub log_level: String,
}

/// Generate mainnet configuration for all 7 servers
pub fn generate_mainnet_configs() -> Vec<MainnetServerConfig> {
    let validators = vec![
        ("ohio", "validator1-ohio.schikuno.top", None, true),
        ("singapore", "validator1-singapore.schikuno.top", None, true),
        ("stockholm", "validator1-stockholm.schikuno.top", None, true),
        ("saopaulo", "validator1-saopaulo.schikuno.top", None, false),
        (
            "india",
            "validator1-india.schikuno.top",
            Some("74.225.183.196"),
            false,
        ),
        (
            "southafrica",
            "validator1-southafrica.schikuno.top",
            Some("4.221.211.71"),
            false,
        ),
        (
            "uae",
            "validator1-uae.schikuno.top",
            Some("4.161.34.228"),
            false,
        ),
    ];

    let all_subdomains: Vec<String> = validators
        .iter()
        .map(|(_, s, _, _)| s.to_string())
        .collect();

    validators
        .into_iter()
        .map(|(region, subdomain, ip, is_tikv_pd)| {
            MainnetServerConfig {
                region: region.to_string(),
                subdomain: subdomain.to_string(),
                public_ip: ip.map(|s| s.to_string()),
                role: NodeRole::ValidatorWithRelays,
                validator: Some(ValidatorConfig {
                    key_file: "/opt/dchat/keys/validator.key".to_string(),
                    chain_rpc: format!("https://{}", all_subdomains[0]),
                    stake: 10000,
                    producer: true,
                    listen_port: 7070,
                    health_port: 8080,
                    metrics_port: 9090,
                }),
                relays: vec![
                    RelayConfig {
                        identifier: "relay1".to_string(),
                        listen_port: 7071,
                        health_port: 8081,
                        metrics_port: 9091,
                        min_stake: 1000,
                    },
                    RelayConfig {
                        identifier: "relay2".to_string(),
                        listen_port: 7072,
                        health_port: 8082,
                        metrics_port: 9092,
                        min_stake: 1000,
                    },
                ],
                storage: StorageClusterConfig {
                    redis: RedisClusterConfig {
                        port: 6379,
                        cluster_mode: true,
                        cluster_nodes: all_subdomains
                            .iter()
                            .map(|s| format!("{}:6379", s))
                            .collect(),
                        password_env: "DCHAT_REDIS_PASSWORD".to_string(),
                    },
                    minio: MinioClusterConfig {
                        port: 9000,
                        console_port: 9001,
                        endpoints: all_subdomains
                            .iter()
                            .map(|s| format!("https://{}:9000", s))
                            .collect(),
                        access_key_env: "DCHAT_MINIO_ACCESS_KEY".to_string(),
                        secret_key_env: "DCHAT_MINIO_SECRET_KEY".to_string(),
                        drives_per_server: 1,
                        data_dir: "/opt/dchat/minio/data".to_string(),
                    },
                    tikv: if is_tikv_pd {
                        Some(TikvClusterConfig {
                            pd_port: 2379,
                            tikv_port: 20160,
                            pd_endpoints: vec![
                                format!("{}:2379", all_subdomains[0]), // Ohio
                                format!("{}:2379", all_subdomains[1]), // Singapore
                                format!("{}:2379", all_subdomains[2]), // Stockholm
                            ],
                            is_pd: true,
                        })
                    } else {
                        None
                    },
                    cockroachdb: CockroachDbConfig {
                        connection_url_env: "DCHAT_COCKROACHDB_URL".to_string(),
                        max_connections: 100,
                        tls_mode: "verify-full".to_string(),
                    },
                },
                tls: TlsConfig {
                    enabled: true,
                    cert_path: "/etc/dchat/certs/fullchain.pem".to_string(),
                    key_path: "/etc/dchat/certs/privkey.pem".to_string(),
                    ca_path: "/etc/dchat/certs/chain.pem".to_string(),
                },
                monitoring: MonitoringConfig {
                    prometheus_enabled: true,
                    prometheus_port: 9090,
                    jaeger_endpoint: Some(
                        "http://monitoring.schikuno.top:14268/api/traces".to_string(),
                    ),
                    log_level: "info".to_string(),
                },
            }
        })
        .collect()
}

/// Generate TOML configuration file for a server
pub fn generate_server_toml(config: &MainnetServerConfig) -> String {
    let mut toml = format!(
        r#"# Mainnet Production Configuration - {}
# Region: {}
# Generated: {}

[server]
region = "{}"
subdomain = "{}"
"#,
        config.subdomain,
        config.region,
        chrono::Utc::now().to_rfc3339(),
        config.region,
        config.subdomain
    );

    if let Some(ip) = &config.public_ip {
        toml.push_str(&format!("public_ip = \"{}\"\n", ip));
    }

    toml.push_str("\n[network]\n");
    toml.push_str("enable_dns_discovery = true\n");
    toml.push_str("base_domain = \"schikuno.top\"\n");
    toml.push_str("listen_addresses = [\"0.0.0.0:7070\"]\n");
    toml.push_str("max_connections = 100\n");
    toml.push_str("connection_timeout_ms = 30000\n");
    toml.push_str("enable_mdns = false\n");
    toml.push_str("enable_upnp = true\n");

    if let Some(validator) = &config.validator {
        toml.push_str("\n[validator]\n");
        toml.push_str(&format!("key_file = \"{}\"\n", validator.key_file));
        toml.push_str(&format!("chain_rpc = \"{}\"\n", validator.chain_rpc));
        toml.push_str(&format!("stake = {}\n", validator.stake));
        toml.push_str(&format!("producer = {}\n", validator.producer));
        toml.push_str(&format!("listen_port = {}\n", validator.listen_port));
        toml.push_str(&format!("health_port = {}\n", validator.health_port));
        toml.push_str(&format!("metrics_port = {}\n", validator.metrics_port));
    }

    if !config.relays.is_empty() {
        toml.push_str("\n[[relay]]\n");
        for (i, relay) in config.relays.iter().enumerate() {
            if i > 0 {
                toml.push_str("\n[[relay]]\n");
            }
            toml.push_str(&format!("identifier = \"{}\"\n", relay.identifier));
            toml.push_str(&format!("listen_port = {}\n", relay.listen_port));
            toml.push_str(&format!("health_port = {}\n", relay.health_port));
            toml.push_str(&format!("metrics_port = {}\n", relay.metrics_port));
            toml.push_str(&format!("min_stake = {}\n", relay.min_stake));
        }
    }

    toml.push_str("\n[storage.redis]\n");
    toml.push_str(&format!("port = {}\n", config.storage.redis.port));
    toml.push_str(&format!(
        "cluster_mode = {}\n",
        config.storage.redis.cluster_mode
    ));
    toml.push_str(&format!(
        "password_env = \"{}\"\n",
        config.storage.redis.password_env
    ));
    toml.push_str("cluster_nodes = [\n");
    for node in &config.storage.redis.cluster_nodes {
        toml.push_str(&format!("  \"{}\",\n", node));
    }
    toml.push_str("]\n");

    toml.push_str("\n[storage.minio]\n");
    toml.push_str(&format!("port = {}\n", config.storage.minio.port));
    toml.push_str(&format!(
        "console_port = {}\n",
        config.storage.minio.console_port
    ));
    toml.push_str(&format!(
        "access_key_env = \"{}\"\n",
        config.storage.minio.access_key_env
    ));
    toml.push_str(&format!(
        "secret_key_env = \"{}\"\n",
        config.storage.minio.secret_key_env
    ));
    toml.push_str(&format!(
        "data_dir = \"{}\"\n",
        config.storage.minio.data_dir
    ));
    toml.push_str("endpoints = [\n");
    for endpoint in &config.storage.minio.endpoints {
        toml.push_str(&format!("  \"{}\",\n", endpoint));
    }
    toml.push_str("]\n");

    if let Some(tikv) = &config.storage.tikv {
        toml.push_str("\n[storage.tikv]\n");
        toml.push_str(&format!("pd_port = {}\n", tikv.pd_port));
        toml.push_str(&format!("tikv_port = {}\n", tikv.tikv_port));
        toml.push_str(&format!("is_pd = {}\n", tikv.is_pd));
        toml.push_str("pd_endpoints = [\n");
        for endpoint in &tikv.pd_endpoints {
            toml.push_str(&format!("  \"{}\",\n", endpoint));
        }
        toml.push_str("]\n");
    }

    toml.push_str("\n[storage.cockroachdb]\n");
    toml.push_str(&format!(
        "connection_url_env = \"{}\"\n",
        config.storage.cockroachdb.connection_url_env
    ));
    toml.push_str(&format!(
        "max_connections = {}\n",
        config.storage.cockroachdb.max_connections
    ));
    toml.push_str(&format!(
        "tls_mode = \"{}\"\n",
        config.storage.cockroachdb.tls_mode
    ));

    toml.push_str("\n[tls]\n");
    toml.push_str(&format!("enabled = {}\n", config.tls.enabled));
    toml.push_str(&format!("cert_path = \"{}\"\n", config.tls.cert_path));
    toml.push_str(&format!("key_path = \"{}\"\n", config.tls.key_path));
    toml.push_str(&format!("ca_path = \"{}\"\n", config.tls.ca_path));

    toml.push_str("\n[monitoring]\n");
    toml.push_str(&format!(
        "prometheus_enabled = {}\n",
        config.monitoring.prometheus_enabled
    ));
    toml.push_str(&format!(
        "prometheus_port = {}\n",
        config.monitoring.prometheus_port
    ));
    if let Some(jaeger) = &config.monitoring.jaeger_endpoint {
        toml.push_str(&format!("jaeger_endpoint = \"{}\"\n", jaeger));
    }
    toml.push_str(&format!(
        "log_level = \"{}\"\n",
        config.monitoring.log_level
    ));

    toml
}

/// Write all mainnet configurations to files
pub async fn write_mainnet_configs(output_dir: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(output_dir).await?;

    let configs = generate_mainnet_configs();

    for config in &configs {
        let filename = format!("config-mainnet-{}.toml", config.region);
        let filepath = output_dir.join(&filename);

        let toml_content = generate_server_toml(&config);
        tokio::fs::write(&filepath, toml_content).await?;

        println!("✓ Generated: {}", filepath.display());
    }

    // Also write a summary JSON
    let summary_path = output_dir.join("mainnet-servers.json");
    let summary_json = serde_json::to_string_pretty(&configs)?;
    tokio::fs::write(&summary_path, summary_json).await?;
    println!("✓ Generated: {}", summary_path.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_configs() {
        let configs = generate_mainnet_configs();
        assert_eq!(configs.len(), 7, "Should generate 7 server configs");

        for config in configs {
            assert!(
                config.validator.is_some(),
                "All servers should have validators"
            );
            assert_eq!(config.relays.len(), 2, "Each server should have 2 relays");
        }
    }

    #[test]
    fn test_toml_generation() {
        let configs = generate_mainnet_configs();
        let toml = generate_server_toml(&configs[0]);

        assert!(toml.contains("[validator]"));
        assert!(toml.contains("[storage.redis]"));
        assert!(toml.contains("[storage.minio]"));
    }
}
