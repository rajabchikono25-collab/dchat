// dchat Deployment Infrastructure
// Handles multi-region validator deployment, relay network, distributed storage, and disaster recovery

pub mod backup_system;
pub mod distributed_storage;
pub mod health_monitor;
pub mod mainnet_config;
pub mod multi_region_config;
pub mod orchestrator;
pub mod relay_network;

pub use mainnet_config::{
    CockroachDbConfig, MainnetServerConfig, MinioClusterConfig, MonitoringConfig, NodeRole,
    RedisClusterConfig, RelayConfig as MainnetRelayConfig, StorageClusterConfig, TikvClusterConfig,
    TlsConfig, ValidatorConfig as MainnetValidatorConfig,
};

pub use multi_region_config::{
    ConfigError, ConsensusConfig, GeographicRegion, MultiRegionConfig, StorageBackend,
    StorageConfig, ValidatorConfig,
};

pub use relay_network::{
    IncentiveConfig, RelayConfig, RelayError, RelayNetworkConfig, RelayReputation, RelayTier,
};

pub use distributed_storage::{
    CockroachDBConfig, CockroachDBNode, DistributedStorageConfig, MinIOConfig, MinIONode,
    RedisConfig, RedisNode, RedisNodeRole, StorageBackendType, StorageError, StorageTier,
    TiKVConfig, TiKVPDNode, TiKVStorageNode,
};

pub use backup_system::{
    BackendBackupConfig, BackupError, BackupMonitoring, BackupTier, BackupType,
    CompressionAlgorithm, DisasterRecoveryConfig, EncryptionConfig, GCSBackupConfig,
    IPFSBackupConfig, LocalReplicaConfig, RestoreConfig, RestoreType, S3BackupConfig,
    SnapshotSchedule, VerificationConfig, WALArchiveConfig,
};

pub use health_monitor::{
    Alert, AlertChannel, AutoScalingConfig, BFTMonitorConfig, ComponentHealthTracker,
    ComponentType, DNSFailoverConfig, FailoverPolicy, GrafanaConfig, HealthCheckConfig,
    HealthCheckResult, HealthError, HealthMonitorConfig, HealthStatus, PrometheusConfig,
};
