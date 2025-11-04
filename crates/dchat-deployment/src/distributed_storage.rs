// Distributed Storage Configuration and Management
//
// This module implements a 4-tier distributed storage architecture:
// 1. CockroachDB - Distributed SQL database for application data (5-node cluster)
// 2. Redis - Distributed cache and session storage (6-node cluster: 3 masters + 3 replicas)
// 3. MinIO - Distributed object storage for media/files (4-node cluster)
// 4. TiKV - Distributed key-value store for blockchain state (5-node cluster)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use thiserror::Error;

use crate::multi_region_config::GeographicRegion;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Invalid storage configuration: {0}")]
    InvalidConfig(String),
    #[error("Insufficient storage redundancy: {0}")]
    InsufficientRedundancy(String),
    #[error("Storage cluster unhealthy: {0}")]
    UnhealthyCluster(String),
}

/// Storage tier type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageTier {
    /// Hot tier: Frequently accessed data (Redis cache, TiKV blockchain state)
    Hot,
    /// Warm tier: Application data (CockroachDB)
    Warm,
    /// Cold tier: Archival data (MinIO with S3 backend)
    Cold,
    /// Archive tier: Long-term backup (S3 Glacier, tape)
    Archive,
}

/// Storage backend type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageBackendType {
    CockroachDB,
    Redis,
    MinIO,
    TiKV,
    PostgreSQL, // Legacy single-node
}

/// CockroachDB cluster configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CockroachDBConfig {
    /// Cluster name
    pub cluster_name: String,

    /// Node configurations
    pub nodes: Vec<CockroachDBNode>,

    /// Replication factor (default 3, recommended 5 for production)
    pub replication_factor: usize,

    /// Database name
    pub database_name: String,

    /// Join addresses (for new nodes)
    pub join_addresses: Vec<String>,

    /// SQL port (default 26257)
    pub sql_port: u16,

    /// HTTP port for admin UI (default 8080)
    pub http_port: u16,

    /// Enable encryption at rest
    pub encryption_at_rest: bool,

    /// Maximum connections per node
    pub max_connections: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CockroachDBNode {
    pub node_id: String,
    pub region: GeographicRegion,
    pub host: String,
    pub sql_address: SocketAddr,
    pub http_address: SocketAddr,
    pub store_path: String,
    pub cache_size_mb: usize,
    pub max_sql_memory_mb: usize,
}

impl CockroachDBConfig {
    /// Create recommended 5-node CockroachDB cluster
    pub fn new_recommended(cluster_name: String) -> Self {
        let regions = vec![
            GeographicRegion::USEast,
            GeographicRegion::USWest,
            GeographicRegion::EUWest,
            GeographicRegion::AsiaPacificSE,
            GeographicRegion::USEast, // 5th node in US East for quorum
        ];

        let nodes: Vec<CockroachDBNode> = regions
            .iter()
            .enumerate()
            .map(|(i, region)| {
                let node_id = format!("cockroach-{}-{}", region.dns_suffix(), i + 1);
                CockroachDBNode {
                    node_id: node_id.clone(),
                    region: *region,
                    host: format!("{}.dchat.network", node_id),
                    sql_address: format!("0.0.0.0:26257").parse().unwrap(),
                    http_address: format!("0.0.0.0:8080").parse().unwrap(),
                    store_path: "/data/cockroach".to_string(),
                    cache_size_mb: 4096,     // 4GB cache
                    max_sql_memory_mb: 8192, // 8GB SQL memory
                }
            })
            .collect();

        let join_addresses = nodes.iter().map(|n| format!("{}:26257", n.host)).collect();

        Self {
            cluster_name,
            nodes,
            replication_factor: 5,
            database_name: "dchat".to_string(),
            join_addresses,
            sql_port: 26257,
            http_port: 8080,
            encryption_at_rest: true,
            max_connections: 1000,
        }
    }

    /// Generate connection string for application
    pub fn connection_string(&self, username: &str, password: &str) -> String {
        let hosts = self
            .nodes
            .iter()
            .map(|n| format!("{}:{}", n.host, self.sql_port))
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "postgresql://{}:{}@{}/{}?sslmode=verify-full",
            username, password, hosts, self.database_name
        )
    }

    /// Verify cluster has sufficient redundancy
    pub fn verify_redundancy(&self) -> Result<(), StorageError> {
        if self.nodes.len() < 3 {
            return Err(StorageError::InsufficientRedundancy(format!(
                "CockroachDB requires at least 3 nodes (found {})",
                self.nodes.len()
            )));
        }

        if self.replication_factor < 3 {
            return Err(StorageError::InsufficientRedundancy(format!(
                "Replication factor must be at least 3 (found {})",
                self.replication_factor
            )));
        }

        if self.replication_factor > self.nodes.len() {
            return Err(StorageError::InsufficientRedundancy(format!(
                "Replication factor ({}) exceeds node count ({})",
                self.replication_factor,
                self.nodes.len()
            )));
        }

        Ok(())
    }
}

/// Redis cluster configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    /// Cluster name
    pub cluster_name: String,

    /// Master nodes (3 recommended for distributed cluster)
    pub masters: Vec<RedisNode>,

    /// Replica nodes (1 per master)
    pub replicas: Vec<RedisNode>,

    /// Redis port (default 6379)
    pub port: u16,

    /// Cluster bus port (default 16379)
    pub cluster_bus_port: u16,

    /// Maximum memory per node (MB)
    pub max_memory_mb: usize,

    /// Eviction policy (e.g., "allkeys-lru")
    pub eviction_policy: String,

    /// Enable persistence (AOF + RDB)
    pub persistence_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisNode {
    pub node_id: String,
    pub region: GeographicRegion,
    pub host: String,
    pub address: SocketAddr,
    pub role: RedisNodeRole,
    pub master_of: Option<String>, // For replicas: which master they replicate
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedisNodeRole {
    Master,
    Replica,
}

impl RedisConfig {
    /// Create recommended 6-node Redis cluster (3 masters + 3 replicas)
    pub fn new_recommended(cluster_name: String) -> Self {
        let master_regions = vec![
            GeographicRegion::USEast,
            GeographicRegion::EUWest,
            GeographicRegion::AsiaPacificSE,
        ];

        let mut masters = Vec::new();
        let mut replicas = Vec::new();

        for (i, region) in master_regions.iter().enumerate() {
            let master_id = format!("redis-master-{}", i + 1);
            masters.push(RedisNode {
                node_id: master_id.clone(),
                region: *region,
                host: format!("{}.dchat.network", master_id),
                address: format!("0.0.0.0:6379").parse().unwrap(),
                role: RedisNodeRole::Master,
                master_of: None,
            });

            let replica_id = format!("redis-replica-{}", i + 1);
            replicas.push(RedisNode {
                node_id: replica_id.clone(),
                region: *region,
                host: format!("{}.dchat.network", replica_id),
                address: format!("0.0.0.0:6379").parse().unwrap(),
                role: RedisNodeRole::Replica,
                master_of: Some(master_id.clone()),
            });
        }

        Self {
            cluster_name,
            masters,
            replicas,
            port: 6379,
            cluster_bus_port: 16379,
            max_memory_mb: 4096, // 4GB per node
            eviction_policy: "allkeys-lru".to_string(),
            persistence_enabled: true,
        }
    }

    /// Generate Redis cluster connection string
    pub fn connection_string(&self, password: Option<&str>) -> String {
        let hosts = self
            .masters
            .iter()
            .map(|n| format!("{}:{}", n.host, self.port))
            .collect::<Vec<_>>()
            .join(",");

        if let Some(pwd) = password {
            format!("redis://:{pwd}@{hosts}")
        } else {
            format!("redis://{hosts}")
        }
    }

    /// Verify cluster configuration
    pub fn verify_configuration(&self) -> Result<(), StorageError> {
        if self.masters.len() < 3 {
            return Err(StorageError::InvalidConfig(format!(
                "Redis cluster requires at least 3 masters (found {})",
                self.masters.len()
            )));
        }

        if self.replicas.len() != self.masters.len() {
            return Err(StorageError::InvalidConfig(format!(
                "Each master should have 1 replica ({} masters, {} replicas)",
                self.masters.len(),
                self.replicas.len()
            )));
        }

        Ok(())
    }
}

/// MinIO distributed object storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinIOConfig {
    /// Cluster name
    pub cluster_name: String,

    /// Storage nodes (minimum 4 for distributed mode)
    pub nodes: Vec<MinIONode>,

    /// S3 API port (default 9000)
    pub api_port: u16,

    /// Console port (default 9001)
    pub console_port: u16,

    /// Number of drives per node
    pub drives_per_node: usize,

    /// Erasure coding parity (default 2)
    pub parity: usize,

    /// Root user credentials
    pub root_user: String,

    /// Enable versioning
    pub versioning_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinIONode {
    pub node_id: String,
    pub region: GeographicRegion,
    pub host: String,
    pub api_address: SocketAddr,
    pub console_address: SocketAddr,
    pub data_volumes: Vec<String>, // Mount paths for data drives
}

impl MinIOConfig {
    /// Create recommended 4-node MinIO cluster
    pub fn new_recommended(cluster_name: String) -> Self {
        let regions = vec![
            GeographicRegion::USEast,
            GeographicRegion::USWest,
            GeographicRegion::EUWest,
            GeographicRegion::AsiaPacificSE,
        ];

        let nodes = regions
            .iter()
            .enumerate()
            .map(|(i, region)| {
                let node_id = format!("minio-{}", i + 1);
                MinIONode {
                    node_id: node_id.clone(),
                    region: *region,
                    host: format!("{}.dchat.network", node_id),
                    api_address: format!("0.0.0.0:9000").parse().unwrap(),
                    console_address: format!("0.0.0.0:9001").parse().unwrap(),
                    data_volumes: vec![
                        "/data/minio/drive1".to_string(),
                        "/data/minio/drive2".to_string(),
                        "/data/minio/drive3".to_string(),
                        "/data/minio/drive4".to_string(),
                    ],
                }
            })
            .collect();

        Self {
            cluster_name,
            nodes,
            api_port: 9000,
            console_port: 9001,
            drives_per_node: 4,
            parity: 2, // EC:2 (2 drives can fail)
            root_user: "dchat-admin".to_string(),
            versioning_enabled: true,
        }
    }

    /// Generate MinIO server command for distributed deployment
    pub fn server_command(&self) -> String {
        let drives = self
            .nodes
            .iter()
            .flat_map(|node| {
                (0..self.drives_per_node)
                    .map(move |i| format!("http://{}:9000/data/minio/drive{}", node.host, i + 1))
            })
            .collect::<Vec<_>>()
            .join(" ");

        format!("minio server {}", drives)
    }

    /// Verify minimum requirements for distributed mode
    pub fn verify_configuration(&self) -> Result<(), StorageError> {
        if self.nodes.len() < 4 {
            return Err(StorageError::InvalidConfig(format!(
                "MinIO distributed mode requires at least 4 nodes (found {})",
                self.nodes.len()
            )));
        }

        let total_drives = self.nodes.len() * self.drives_per_node;
        if total_drives < 4 {
            return Err(StorageError::InvalidConfig(format!(
                "MinIO requires at least 4 total drives (found {})",
                total_drives
            )));
        }

        if self.parity >= self.drives_per_node {
            return Err(StorageError::InvalidConfig(format!(
                "Parity ({}) must be less than drives per node ({})",
                self.parity, self.drives_per_node
            )));
        }

        Ok(())
    }
}

/// TiKV distributed key-value store configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiKVConfig {
    /// Cluster name
    pub cluster_name: String,

    /// Placement Driver (PD) nodes (3-5 recommended)
    pub pd_nodes: Vec<TiKVPDNode>,

    /// TiKV storage nodes (5+ recommended)
    pub tikv_nodes: Vec<TiKVStorageNode>,

    /// Replication factor (default 3)
    pub replication_factor: usize,

    /// PD port (default 2379)
    pub pd_port: u16,

    /// TiKV port (default 20160)
    pub tikv_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiKVPDNode {
    pub node_id: String,
    pub region: GeographicRegion,
    pub host: String,
    pub client_address: SocketAddr,
    pub peer_address: SocketAddr,
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiKVStorageNode {
    pub node_id: String,
    pub region: GeographicRegion,
    pub host: String,
    pub address: SocketAddr,
    pub status_address: SocketAddr,
    pub data_dir: String,
    pub capacity_gb: usize,
}

impl TiKVConfig {
    /// Create recommended TiKV cluster (3 PD + 5 TiKV nodes)
    pub fn new_recommended(cluster_name: String) -> Self {
        let pd_regions = vec![
            GeographicRegion::USEast,
            GeographicRegion::EUWest,
            GeographicRegion::AsiaPacificSE,
        ];

        let pd_nodes = pd_regions
            .iter()
            .enumerate()
            .map(|(i, region)| {
                let node_id = format!("tikv-pd-{}", i + 1);
                TiKVPDNode {
                    node_id: node_id.clone(),
                    region: *region,
                    host: format!("{}.dchat.network", node_id),
                    client_address: format!("0.0.0.0:2379").parse().unwrap(),
                    peer_address: format!("0.0.0.0:2380").parse().unwrap(),
                    data_dir: "/data/tikv/pd".to_string(),
                }
            })
            .collect();

        let tikv_regions = vec![
            GeographicRegion::USEast,
            GeographicRegion::USWest,
            GeographicRegion::EUWest,
            GeographicRegion::AsiaPacificSE,
            GeographicRegion::USEast, // 5th in US East
        ];

        let tikv_nodes = tikv_regions
            .iter()
            .enumerate()
            .map(|(i, region)| {
                let node_id = format!("tikv-storage-{}", i + 1);
                TiKVStorageNode {
                    node_id: node_id.clone(),
                    region: *region,
                    host: format!("{}.dchat.network", node_id),
                    address: format!("0.0.0.0:20160").parse().unwrap(),
                    status_address: format!("0.0.0.0:20180").parse().unwrap(),
                    data_dir: "/data/tikv/storage".to_string(),
                    capacity_gb: 500, // 500GB per node
                }
            })
            .collect();

        Self {
            cluster_name,
            pd_nodes,
            tikv_nodes,
            replication_factor: 3,
            pd_port: 2379,
            tikv_port: 20160,
        }
    }

    /// Generate PD endpoints for TiKV clients
    pub fn pd_endpoints(&self) -> Vec<String> {
        self.pd_nodes
            .iter()
            .map(|pd| format!("{}:{}", pd.host, self.pd_port))
            .collect()
    }

    /// Verify cluster configuration
    pub fn verify_configuration(&self) -> Result<(), StorageError> {
        if self.pd_nodes.len() < 3 || self.pd_nodes.len() % 2 == 0 {
            return Err(StorageError::InvalidConfig(format!(
                "TiKV requires odd number of PD nodes (3, 5, or 7), found {}",
                self.pd_nodes.len()
            )));
        }

        if self.tikv_nodes.len() < 3 {
            return Err(StorageError::InvalidConfig(format!(
                "TiKV requires at least 3 storage nodes (found {})",
                self.tikv_nodes.len()
            )));
        }

        if self.replication_factor > self.tikv_nodes.len() {
            return Err(StorageError::InvalidConfig(format!(
                "Replication factor ({}) exceeds node count ({})",
                self.replication_factor,
                self.tikv_nodes.len()
            )));
        }

        Ok(())
    }
}

/// Complete distributed storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedStorageConfig {
    pub network_name: String,
    pub cockroachdb: CockroachDBConfig,
    pub redis: RedisConfig,
    pub minio: MinIOConfig,
    pub tikv: TiKVConfig,
}

impl DistributedStorageConfig {
    /// Create recommended distributed storage configuration
    pub fn new_recommended(network_name: String) -> Self {
        Self {
            network_name: network_name.clone(),
            cockroachdb: CockroachDBConfig::new_recommended(format!("{}-cockroach", network_name)),
            redis: RedisConfig::new_recommended(format!("{}-redis", network_name)),
            minio: MinIOConfig::new_recommended(format!("{}-minio", network_name)),
            tikv: TiKVConfig::new_recommended(format!("{}-tikv", network_name)),
        }
    }

    /// Verify all storage backends
    pub fn verify_all(&self) -> Result<(), StorageError> {
        self.cockroachdb.verify_redundancy()?;
        self.redis.verify_configuration()?;
        self.minio.verify_configuration()?;
        self.tikv.verify_configuration()?;
        Ok(())
    }

    /// Get total node count across all storage backends
    pub fn total_node_count(&self) -> usize {
        self.cockroachdb.nodes.len()
            + self.redis.masters.len()
            + self.redis.replicas.len()
            + self.minio.nodes.len()
            + self.tikv.pd_nodes.len()
            + self.tikv.tikv_nodes.len()
    }

    /// Get storage tier mapping
    pub fn tier_mapping(&self) -> HashMap<StorageTier, Vec<StorageBackendType>> {
        let mut mapping = HashMap::new();

        mapping.insert(
            StorageTier::Hot,
            vec![StorageBackendType::Redis, StorageBackendType::TiKV],
        );

        mapping.insert(StorageTier::Warm, vec![StorageBackendType::CockroachDB]);

        mapping.insert(StorageTier::Cold, vec![StorageBackendType::MinIO]);

        mapping
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cockroachdb_config_creation() {
        let config = CockroachDBConfig::new_recommended("test-cluster".to_string());
        assert_eq!(config.nodes.len(), 5);
        assert_eq!(config.replication_factor, 5);
        config.verify_redundancy().expect("Redundancy check failed");
    }

    #[test]
    fn test_redis_config_creation() {
        let config = RedisConfig::new_recommended("test-cluster".to_string());
        assert_eq!(config.masters.len(), 3);
        assert_eq!(config.replicas.len(), 3);
        config.verify_configuration().expect("Config check failed");
    }

    #[test]
    fn test_minio_config_creation() {
        let config = MinIOConfig::new_recommended("test-cluster".to_string());
        assert_eq!(config.nodes.len(), 4);
        assert_eq!(config.drives_per_node, 4);
        config.verify_configuration().expect("Config check failed");
    }

    #[test]
    fn test_tikv_config_creation() {
        let config = TiKVConfig::new_recommended("test-cluster".to_string());
        assert_eq!(config.pd_nodes.len(), 3);
        assert_eq!(config.tikv_nodes.len(), 5);
        config.verify_configuration().expect("Config check failed");
    }

    #[test]
    fn test_distributed_storage_complete() {
        let config = DistributedStorageConfig::new_recommended("dchat-mainnet".to_string());
        config.verify_all().expect("Verification failed");

        // Total: 5 CockroachDB + 6 Redis + 4 MinIO + 3 PD + 5 TiKV = 23 nodes
        assert_eq!(config.total_node_count(), 23);
    }

    #[test]
    fn test_connection_strings() {
        let cockroach = CockroachDBConfig::new_recommended("test".to_string());
        let conn_str = cockroach.connection_string("user", "pass");
        assert!(conn_str.contains("postgresql://"));
        assert!(conn_str.contains("user:pass"));

        let redis = RedisConfig::new_recommended("test".to_string());
        let redis_str = redis.connection_string(Some("secret"));
        assert!(redis_str.contains("redis://"));
    }

    #[test]
    fn test_storage_tier_mapping() {
        let config = DistributedStorageConfig::new_recommended("test".to_string());
        let tiers = config.tier_mapping();

        assert!(tiers
            .get(&StorageTier::Hot)
            .unwrap()
            .contains(&StorageBackendType::Redis));
        assert!(tiers
            .get(&StorageTier::Hot)
            .unwrap()
            .contains(&StorageBackendType::TiKV));
        assert!(tiers
            .get(&StorageTier::Warm)
            .unwrap()
            .contains(&StorageBackendType::CockroachDB));
        assert!(tiers
            .get(&StorageTier::Cold)
            .unwrap()
            .contains(&StorageBackendType::MinIO));
    }
}
