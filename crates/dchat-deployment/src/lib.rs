// dchat Deployment Infrastructure
// Handles multi-region validator deployment, relay network, distributed storage, and disaster recovery

pub mod multi_region_config;
pub mod relay_network;
pub mod distributed_storage;
pub mod backup_system;
pub mod health_monitor;

pub use multi_region_config::{
    GeographicRegion, ValidatorConfig, ConsensusConfig, StorageConfig,
    StorageBackend, MultiRegionConfig, ConfigError,
};

pub use relay_network::{
    RelayConfig, RelayNetworkConfig, RelayTier, RelayReputation,
    IncentiveConfig, RelayError,
};

pub use distributed_storage::{
    CockroachDBConfig, CockroachDBNode, DistributedStorageConfig, MinIOConfig, MinIONode,
    RedisConfig, RedisNode, RedisNodeRole, StorageBackendType, StorageError, StorageTier,
    TiKVConfig, TiKVPDNode, TiKVStorageNode,
};

pub use backup_system::{
    DisasterRecoveryConfig, S3BackupConfig, GCSBackupConfig, IPFSBackupConfig,
    LocalReplicaConfig, SnapshotSchedule, WALArchiveConfig, BackendBackupConfig,
    RestoreConfig, VerificationConfig, BackupMonitoring, BackupTier, BackupType,
    RestoreType, CompressionAlgorithm, EncryptionConfig, BackupError,
};

pub use health_monitor::{
    HealthMonitorConfig, HealthCheckConfig, DNSFailoverConfig, AutoScalingConfig,
    AlertChannel, BFTMonitorConfig, PrometheusConfig, GrafanaConfig,
    ComponentType, HealthStatus, HealthCheckResult, ComponentHealthTracker,
    Alert, HealthError, FailoverPolicy,
};
