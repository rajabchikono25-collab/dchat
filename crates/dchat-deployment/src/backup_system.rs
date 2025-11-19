// Disaster Recovery and Backup System Configuration
// Multi-layer backup (S3, GCS, IPFS, local) with verified restore testing

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Backup system errors
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Backup operation failed: {0}")]
    BackupFailed(String),
    #[error("Restore operation failed: {0}")]
    RestoreFailed(String),
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
    #[error("Encryption error: {0}")]
    EncryptionError(String),
}

/// Backup storage tier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackupTier {
    /// Hot backups - S3 (immediate access, recent data)
    Hot,
    /// Warm backups - GCS (archived data, cost-optimized)
    Warm,
    /// Cold backups - IPFS (permanent, content-addressed)
    Cold,
    /// Local replicas - Fast recovery option
    Local,
}

/// Backup type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupType {
    /// Full snapshot (complete database state)
    Full,
    /// Incremental (changes since last full)
    Incremental,
    /// WAL archive (continuous write-ahead logs)
    WAL,
}

/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// ZSTD - Fast with good compression ratio (3:1 typical)
    ZSTD,
    /// LZ4 - Very fast, lower compression (2:1 typical)
    LZ4,
    /// Snappy - Balanced speed/compression
    Snappy,
    /// None - No compression
    None,
}

/// Encryption configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Algorithm (AES-256-GCM)
    pub algorithm: String,
    /// Key management service
    pub kms_provider: String,
    /// KMS key ID or ARN
    pub kms_key_id: String,
    /// Key rotation enabled
    pub rotation_enabled: bool,
    /// Rotation period in days
    pub rotation_days: u32,
}

impl EncryptionConfig {
    pub fn new_production() -> Self {
        Self {
            algorithm: "AES-256-GCM".to_string(),
            kms_provider: "AWS KMS".to_string(),
            kms_key_id: "arn:aws:kms:us-east-1:123456789012:key/backup-master-key".to_string(),
            rotation_enabled: true,
            rotation_days: 90,
        }
    }
}

/// S3 backup configuration (Hot tier)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BackupConfig {
    /// S3 bucket name
    pub bucket: String,
    /// AWS region
    pub region: String,
    /// Storage class (STANDARD, INTELLIGENT_TIERING)
    pub storage_class: String,
    /// Lifecycle policy (transition to Glacier after N days)
    pub lifecycle_days: u32,
    /// Cross-region replication target
    pub replication_region: Option<String>,
    /// Versioning enabled
    pub versioning: bool,
    /// Encryption config
    pub encryption: EncryptionConfig,
}

impl S3BackupConfig {
    pub fn new_production() -> Self {
        Self {
            bucket: "dchat-backups-hot".to_string(),
            region: "us-east-1".to_string(),
            storage_class: "INTELLIGENT_TIERING".to_string(),
            lifecycle_days: 30,
            replication_region: Some("eu-west-1".to_string()),
            versioning: true,
            encryption: EncryptionConfig::new_production(),
        }
    }

    pub fn generate_s3_url(&self) -> String {
        format!("s3://{}/{}", self.bucket, "backups")
    }
}

/// GCS backup configuration (Warm tier)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCSBackupConfig {
    /// GCS bucket name
    pub bucket: String,
    /// Storage class (ARCHIVE, NEARLINE)
    pub storage_class: String,
    /// Retention days
    pub retention_days: u32,
    /// Multi-region (US, EU, ASIA)
    pub multi_region: String,
    /// Encryption config
    pub encryption: EncryptionConfig,
}

impl GCSBackupConfig {
    pub fn new_production() -> Self {
        Self {
            bucket: "dchat-backups-warm".to_string(),
            storage_class: "NEARLINE".to_string(),
            retention_days: 90,
            multi_region: "US".to_string(),
            encryption: EncryptionConfig::new_production(),
        }
    }

    pub fn generate_gcs_url(&self) -> String {
        format!("gs://{}/{}", self.bucket, "backups")
    }
}

/// IPFS backup configuration (Cold tier)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IPFSBackupConfig {
    /// IPFS node endpoints
    pub nodes: Vec<String>,
    /// Pinning services (Pinata, Infura)
    pub pinning_services: Vec<String>,
    /// Replication factor
    pub replication_factor: u32,
    /// Content addressing
    pub use_cidv1: bool,
}

impl IPFSBackupConfig {
    pub fn new_production() -> Self {
        Self {
            nodes: vec![
                "http://ipfs-node-1:5001".to_string(),
                "http://ipfs-node-2:5001".to_string(),
                "http://ipfs-node-3:5001".to_string(),
            ],
            pinning_services: vec!["pinata".to_string(), "infura".to_string()],
            replication_factor: 3,
            use_cidv1: true,
        }
    }
}

/// Local replica configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalReplicaConfig {
    /// Replica nodes
    pub nodes: Vec<LocalReplicaNode>,
    /// Streaming replication
    pub streaming: bool,
    /// Replication lag threshold (seconds)
    pub lag_threshold_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalReplicaNode {
    pub host: String,
    pub port: u16,
    pub data_dir: String,
    pub region: String,
}

impl LocalReplicaConfig {
    pub fn new_production() -> Self {
        Self {
            nodes: vec![
                LocalReplicaNode {
                    host: "replica-1.dchat.internal".to_string(),
                    port: 5432,
                    data_dir: "/var/lib/dchat/replica1".to_string(),
                    region: "us-west-2".to_string(),
                },
                LocalReplicaNode {
                    host: "replica-2.dchat.internal".to_string(),
                    port: 5432,
                    data_dir: "/var/lib/dchat/replica2".to_string(),
                    region: "eu-central-1".to_string(),
                },
                LocalReplicaNode {
                    host: "replica-3.dchat.internal".to_string(),
                    port: 5432,
                    data_dir: "/var/lib/dchat/replica3".to_string(),
                    region: "ap-southeast-1".to_string(),
                },
            ],
            streaming: true,
            lag_threshold_seconds: 30,
        }
    }
}

/// Snapshot schedule configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSchedule {
    /// Full snapshot interval (hours)
    pub full_interval_hours: u32,
    /// Incremental interval (hours)
    pub incremental_interval_hours: u32,
    /// Retention policy
    pub retention: RetentionPolicy,
    /// Compression algorithm
    pub compression: CompressionAlgorithm,
    /// Parallel snapshots
    pub parallel_jobs: u32,
}

impl SnapshotSchedule {
    pub fn new_production() -> Self {
        Self {
            full_interval_hours: 6, // 4 full snapshots per day
            incremental_interval_hours: 1,
            retention: RetentionPolicy::new_production(),
            compression: CompressionAlgorithm::ZSTD,
            parallel_jobs: 4,
        }
    }

    pub fn snapshots_per_day(&self) -> u32 {
        24 / self.full_interval_hours
    }
}

/// Retention policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Full backup retention (days)
    pub full_days: u32,
    /// Incremental retention (days)
    pub incremental_days: u32,
    /// Snapshot retention (days)
    pub snapshot_days: u32,
    /// WAL retention (days)
    pub wal_days: u32,
}

impl RetentionPolicy {
    pub fn new_production() -> Self {
        Self {
            full_days: 30,
            incremental_days: 90,
            snapshot_days: 365,
            wal_days: 14,
        }
    }
}

/// WAL archiving configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WALArchiveConfig {
    /// Archive location (S3 path)
    pub archive_location: String,
    /// Archive command
    pub archive_command: String,
    /// Archive timeout (seconds)
    pub archive_timeout: u32,
    /// Compression enabled
    pub compression: bool,
    /// Point-in-time recovery window (days)
    pub pitr_window_days: u32,
}

impl WALArchiveConfig {
    pub fn new_production() -> Self {
        Self {
            archive_location: "s3://dchat-backups-hot/wal-archive".to_string(),
            archive_command: "wal-g wal-push %p".to_string(),
            archive_timeout: 300,
            compression: true,
            pitr_window_days: 14,
        }
    }
}

/// Backend-specific backup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendBackupConfig {
    /// CockroachDB backup config
    pub cockroachdb: CockroachDBBackupConfig,
    /// Redis backup config
    pub redis: RedisBackupConfig,
    /// MinIO backup config
    pub minio: MinIOBackupConfig,
    /// TiKV backup config
    pub tikv: TiKVBackupConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CockroachDBBackupConfig {
    /// Backup destination (S3)
    pub destination: String,
    /// Full backup schedule (cron)
    pub full_schedule: String,
    /// Incremental schedule (cron)
    pub incremental_schedule: String,
    /// Revision history (days)
    pub revision_history_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisBackupConfig {
    /// RDB snapshot enabled
    pub rdb_enabled: bool,
    /// RDB schedule (seconds)
    pub rdb_schedule_seconds: Vec<(u32, u32)>, // (seconds, changes)
    /// AOF enabled
    pub aof_enabled: bool,
    /// AOF fsync policy
    pub aof_fsync: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinIOBackupConfig {
    /// Bucket versioning
    pub versioning: bool,
    /// Object lock (WORM)
    pub object_lock: bool,
    /// Lifecycle policy
    pub lifecycle_days: u32,
    /// Replication target
    pub replication_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiKVBackupConfig {
    /// Backup destination (S3)
    pub destination: String,
    /// Backup schedule (cron)
    pub schedule: String,
    /// Rate limit (MB/s)
    pub rate_limit_mb: u32,
    /// Checksum verification
    pub checksum_verify: bool,
}

impl BackendBackupConfig {
    pub fn new_production() -> Self {
        // Load S3 credentials from environment or AWS Secrets Manager
        let s3_bucket = std::env::var("DCHAT_BACKUP_S3_BUCKET")
            .unwrap_or_else(|_| "dchat-backups-hot".to_string());
        
        // Use AWS SDK for credential resolution (instance profile, env vars, or credentials file)
        // The AWS SDK will automatically resolve credentials in this order:
        // 1. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)
        // 2. Web Identity Token credentials from environment (EKS/ECS)
        // 3. ECS container credentials (ECS_CONTAINER_CREDENTIALS_RELATIVE_URI)
        // 4. EC2 instance profile credentials
        // 5. ~/.aws/credentials file
        //
        // For production, we should use IAM roles (instance profile) instead of hardcoded keys.
        // CockroachDB backup will use the AWS SDK's default credential chain.
        let cockroachdb_destination = format!(
            "s3://{}/cockroachdb",
            s3_bucket
        );
        
        let tikv_destination = format!(
            "s3://{}/tikv",
            s3_bucket
        );

        Self {
            cockroachdb: CockroachDBBackupConfig {
                destination: cockroachdb_destination,
                full_schedule: "0 */6 * * *".to_string(),  // Every 6 hours
                incremental_schedule: "0 * * * *".to_string(),  // Hourly
                revision_history_days: 14,
            },
            redis: RedisBackupConfig {
                rdb_enabled: true,
                rdb_schedule_seconds: vec![
                    (3600, 1),      // Save after 1 hour if 1 change
                    (300, 100),     // Save after 5 min if 100 changes
                    (60, 10000),    // Save after 1 min if 10000 changes
                ],
                aof_enabled: true,
                aof_fsync: "everysec".to_string(),
            },
            minio: MinIOBackupConfig {
                versioning: true,
                object_lock: false,
                lifecycle_days: 90,
                replication_target: Some("minio-replica.dchat.internal".to_string()),
            },
            tikv: TiKVBackupConfig {
                destination: tikv_destination,
                schedule: "0 */6 * * *".to_string(),  // Every 6 hours
                rate_limit_mb: 100,
                checksum_verify: true,
            },
        }
    }
    
    /// Validate backup configuration for production use
    /// Returns Ok(()) if valid, Err with details if placeholder values detected
    pub fn validate_for_production(&self) -> Result<(), String> {
        // Check for hardcoded credential placeholders in S3 URLs
        if self.cockroachdb.destination.contains("AWS_ACCESS_KEY_ID=xxx") ||
           self.cockroachdb.destination.contains("AWS_SECRET_ACCESS_KEY=xxx") {
            return Err(
                "CockroachDB backup destination contains placeholder credentials. \
                Set DCHAT_BACKUP_S3_BUCKET environment variable and use IAM roles for S3 access."
                .to_string()
            );
        }
        
        if self.tikv.destination.contains("AWS_ACCESS_KEY_ID=xxx") ||
           self.tikv.destination.contains("AWS_SECRET_ACCESS_KEY=xxx") {
            return Err(
                "TiKV backup destination contains placeholder credentials. \
                Set DCHAT_BACKUP_S3_BUCKET environment variable and use IAM roles for S3 access."
                .to_string()
            );
        }
        
        // Warn if using default bucket name (might be intentional in dev)
        if self.cockroachdb.destination.contains("dchat-backups-hot") &&
           std::env::var("DCHAT_BACKUP_S3_BUCKET").is_err() {
            tracing::warn!(
                "Using default S3 bucket 'dchat-backups-hot'. \
                Set DCHAT_BACKUP_S3_BUCKET to override."
            );
        }
        
        Ok(())
    }
}

/// Restore configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreConfig {
    /// Restore type
    pub restore_type: RestoreType,
    /// Source tier
    pub source_tier: BackupTier,
    /// Target timestamp (for PITR)
    pub target_time: Option<String>,
    /// Target database/tables
    pub target_objects: Vec<String>,
    /// Parallel restore jobs
    pub parallel_jobs: u32,
    /// Verification enabled
    pub verify_after_restore: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreType {
    /// Full restore (complete database)
    Full,
    /// Point-in-time restore (to specific timestamp)
    PointInTime,
    /// Partial restore (specific tables/data)
    Partial,
}

impl RestoreConfig {
    pub fn new_full_restore() -> Self {
        Self {
            restore_type: RestoreType::Full,
            source_tier: BackupTier::Hot,
            target_time: None,
            target_objects: vec!["*".to_string()],
            parallel_jobs: 4,
            verify_after_restore: true,
        }
    }

    pub fn new_pitr_restore(target_time: String) -> Self {
        Self {
            restore_type: RestoreType::PointInTime,
            source_tier: BackupTier::Hot,
            target_time: Some(target_time),
            target_objects: vec!["*".to_string()],
            parallel_jobs: 4,
            verify_after_restore: true,
        }
    }
}

/// Verification test configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    /// Test schedule (cron)
    pub schedule: String,
    /// Test databases
    pub test_databases: Vec<String>,
    /// Sample data size (MB)
    pub sample_size_mb: u32,
    /// Success threshold (%)
    pub success_threshold: f64,
    /// Alert on failure
    pub alert_on_failure: bool,
    /// Retention of test results (days)
    pub results_retention_days: u32,
}

impl VerificationConfig {
    pub fn new_production() -> Self {
        Self {
            schedule: "0 2 * * 0".to_string(), // Weekly on Sunday 2am
            test_databases: vec![
                "cockroachdb".to_string(),
                "redis".to_string(),
                "tikv".to_string(),
            ],
            sample_size_mb: 100,
            success_threshold: 99.9,
            alert_on_failure: true,
            results_retention_days: 90,
        }
    }
}

/// Monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMonitoring {
    /// Metrics collection enabled
    pub metrics_enabled: bool,
    /// Prometheus endpoint
    pub prometheus_endpoint: String,
    /// Alert rules
    pub alert_rules: Vec<AlertRule>,
    /// Dashboard URL
    pub dashboard_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub condition: String,
    pub severity: String,
    pub channels: Vec<String>,
}

impl BackupMonitoring {
    pub fn new_production() -> Self {
        Self {
            metrics_enabled: true,
            prometheus_endpoint: "http://prometheus:9090".to_string(),
            alert_rules: vec![
                AlertRule {
                    name: "BackupFailure".to_string(),
                    condition: "backup_success_rate < 0.95".to_string(),
                    severity: "critical".to_string(),
                    channels: vec!["slack".to_string(), "pagerduty".to_string()],
                },
                AlertRule {
                    name: "RestoreTimeSLA".to_string(),
                    condition: "restore_time_seconds > 3600".to_string(),
                    severity: "warning".to_string(),
                    channels: vec!["slack".to_string()],
                },
                AlertRule {
                    name: "StorageCostSpike".to_string(),
                    condition: "storage_cost_daily > 200".to_string(),
                    severity: "warning".to_string(),
                    channels: vec!["slack".to_string()],
                },
            ],
            dashboard_url: "https://grafana.dchat.internal/d/backups".to_string(),
        }
    }
}

/// Complete disaster recovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisasterRecoveryConfig {
    /// S3 hot backups
    pub s3: S3BackupConfig,
    /// GCS warm backups
    pub gcs: GCSBackupConfig,
    /// IPFS cold backups
    pub ipfs: IPFSBackupConfig,
    /// Local replicas
    pub local_replicas: LocalReplicaConfig,
    /// Snapshot schedule
    pub snapshot_schedule: SnapshotSchedule,
    /// WAL archiving
    pub wal_archive: WALArchiveConfig,
    /// Backend-specific configs
    pub backends: BackendBackupConfig,
    /// Verification tests
    pub verification: VerificationConfig,
    /// Monitoring
    pub monitoring: BackupMonitoring,
}

impl DisasterRecoveryConfig {
    pub fn new_production() -> Self {
        Self {
            s3: S3BackupConfig::new_production(),
            gcs: GCSBackupConfig::new_production(),
            ipfs: IPFSBackupConfig::new_production(),
            local_replicas: LocalReplicaConfig::new_production(),
            snapshot_schedule: SnapshotSchedule::new_production(),
            wal_archive: WALArchiveConfig::new_production(),
            backends: BackendBackupConfig::new_production(),
            verification: VerificationConfig::new_production(),
            monitoring: BackupMonitoring::new_production(),
        }
    }

    /// Get backup tier mapping
    pub fn get_tier_mapping(&self) -> HashMap<BackupTier, Vec<String>> {
        let mut mapping = HashMap::new();
        mapping.insert(BackupTier::Hot, vec![self.s3.generate_s3_url()]);
        mapping.insert(BackupTier::Warm, vec![self.gcs.generate_gcs_url()]);
        mapping.insert(BackupTier::Cold, self.ipfs.nodes.clone());
        mapping.insert(
            BackupTier::Local,
            self.local_replicas
                .nodes
                .iter()
                .map(|n| format!("{}:{}", n.host, n.port))
                .collect(),
        );
        mapping
    }

    /// Calculate total storage requirements (TB per day)
    pub fn calculate_daily_storage(&self) -> f64 {
        // Assume 100GB data per full snapshot
        let full_snapshot_gb = 100.0;
        let incremental_gb = 10.0;

        let full_per_day = self.snapshot_schedule.snapshots_per_day();
        let incremental_per_day = 24 / self.snapshot_schedule.incremental_interval_hours;

        let daily_gb = (full_per_day as f64 * full_snapshot_gb)
            + (incremental_per_day as f64 * incremental_gb);

        // Apply compression ratio
        let compression_ratio = match self.snapshot_schedule.compression {
            CompressionAlgorithm::ZSTD => 3.0,
            CompressionAlgorithm::LZ4 => 2.0,
            CompressionAlgorithm::Snappy => 2.5,
            CompressionAlgorithm::None => 1.0,
        };

        (daily_gb / compression_ratio) / 1024.0 // Convert to TB
    }

    /// Estimate monthly cost (USD)
    pub fn estimate_monthly_cost(&self) -> f64 {
        let daily_storage_tb = self.calculate_daily_storage();
        let monthly_storage_tb = daily_storage_tb * 30.0;

        // S3 Intelligent Tiering: $0.0125/GB/month = $12.50/TB/month
        let s3_cost = monthly_storage_tb * 1024.0 * 0.0125;

        // GCS Nearline: $0.010/GB/month = $10/TB/month
        let gcs_cost = (monthly_storage_tb * 0.5) * 1024.0 * 0.010;

        // IPFS pinning: ~$0.08/GB/month = $80/TB/month
        let ipfs_cost = (monthly_storage_tb * 0.2) * 1024.0 * 0.08;

        // Local replicas: $0.08/GB/month = $80/TB/month (EBS volumes)
        let local_cost = (monthly_storage_tb * 0.3) * 1024.0 * 0.08;

        s3_cost + gcs_cost + ipfs_cost + local_cost
    }

    /// Generate JSON configuration
    pub fn generate_json(&self) -> Result<String, BackupError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| BackupError::ConfigError(format!("JSON serialization failed: {}", e)))
    }

    /// Verify backup configuration
    pub fn verify_configuration(&self) -> Result<(), BackupError> {
        // Check snapshot schedule
        if self.snapshot_schedule.full_interval_hours == 0 {
            return Err(BackupError::ConfigError(
                "Snapshot interval cannot be 0".to_string(),
            ));
        }

        // Check retention policy
        if self.snapshot_schedule.retention.full_days == 0 {
            return Err(BackupError::ConfigError(
                "Retention days cannot be 0".to_string(),
            ));
        }

        // Check PITR window
        if self.wal_archive.pitr_window_days > self.snapshot_schedule.retention.wal_days {
            return Err(BackupError::ConfigError(
                "PITR window cannot exceed WAL retention".to_string(),
            ));
        }

        // Check replica count
        if self.local_replicas.nodes.len() < 2 {
            return Err(BackupError::ConfigError(
                "At least 2 local replicas required".to_string(),
            ));
        }

        // Check IPFS replication
        if self.ipfs.replication_factor < 3 {
            return Err(BackupError::ConfigError(
                "IPFS replication factor should be >= 3".to_string(),
            ));
        }

        Ok(())
    }

    /// Get recovery time objective (RTO) in minutes
    pub fn get_rto_minutes(&self, tier: BackupTier) -> u32 {
        match tier {
            BackupTier::Local => 15,  // 15 minutes from local replica
            BackupTier::Hot => 60,    // 1 hour from S3
            BackupTier::Warm => 240,  // 4 hours from GCS
            BackupTier::Cold => 1440, // 24 hours from IPFS
        }
    }

    /// Get recovery point objective (RPO) in minutes
    pub fn get_rpo_minutes(&self) -> u32 {
        // Based on WAL archiving frequency (continuous) and incremental backups
        if self.wal_archive.compression {
            5 // 5 minutes with WAL
        } else {
            self.snapshot_schedule.incremental_interval_hours * 60
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_config_creation() {
        let config = S3BackupConfig::new_production();
        assert_eq!(config.bucket, "dchat-backups-hot");
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.storage_class, "INTELLIGENT_TIERING");
        assert_eq!(config.lifecycle_days, 30);
        assert!(config.versioning);
        assert!(config.replication_region.is_some());
        assert_eq!(config.encryption.algorithm, "AES-256-GCM");
    }

    #[test]
    fn test_gcs_config_creation() {
        let config = GCSBackupConfig::new_production();
        assert_eq!(config.bucket, "dchat-backups-warm");
        assert_eq!(config.storage_class, "NEARLINE");
        assert_eq!(config.retention_days, 90);
        assert_eq!(config.multi_region, "US");
    }

    #[test]
    fn test_ipfs_config_creation() {
        let config = IPFSBackupConfig::new_production();
        assert_eq!(config.nodes.len(), 3);
        assert_eq!(config.pinning_services.len(), 2);
        assert_eq!(config.replication_factor, 3);
        assert!(config.use_cidv1);
    }

    #[test]
    fn test_local_replica_config() {
        let config = LocalReplicaConfig::new_production();
        assert_eq!(config.nodes.len(), 3);
        assert!(config.streaming);
        assert_eq!(config.lag_threshold_seconds, 30);
    }

    #[test]
    fn test_snapshot_schedule() {
        let schedule = SnapshotSchedule::new_production();
        assert_eq!(schedule.full_interval_hours, 6);
        assert_eq!(schedule.snapshots_per_day(), 4);
        assert_eq!(schedule.retention.full_days, 30);
        assert_eq!(schedule.retention.incremental_days, 90);
        assert_eq!(schedule.retention.snapshot_days, 365);
    }

    #[test]
    fn test_wal_archive_config() {
        let config = WALArchiveConfig::new_production();
        assert!(config.archive_location.starts_with("s3://"));
        assert!(config.compression);
        assert_eq!(config.pitr_window_days, 14);
        assert_eq!(config.archive_timeout, 300);
    }

    #[test]
    fn test_disaster_recovery_complete() {
        let config = DisasterRecoveryConfig::new_production();

        // Verify all components initialized
        assert_eq!(config.s3.bucket, "dchat-backups-hot");
        assert_eq!(config.gcs.bucket, "dchat-backups-warm");
        assert_eq!(config.ipfs.nodes.len(), 3);
        assert_eq!(config.local_replicas.nodes.len(), 3);
        assert_eq!(config.snapshot_schedule.full_interval_hours, 6);
        assert_eq!(config.wal_archive.pitr_window_days, 14);

        // Verify monitoring
        assert!(config.monitoring.metrics_enabled);
        assert_eq!(config.monitoring.alert_rules.len(), 3);

        // Verify verification config
        assert_eq!(config.verification.test_databases.len(), 3);
        assert!(config.verification.alert_on_failure);
    }

    #[test]
    fn test_tier_mapping() {
        let config = DisasterRecoveryConfig::new_production();
        let mapping = config.get_tier_mapping();

        assert_eq!(mapping.len(), 4);
        assert!(mapping.contains_key(&BackupTier::Hot));
        assert!(mapping.contains_key(&BackupTier::Warm));
        assert!(mapping.contains_key(&BackupTier::Cold));
        assert!(mapping.contains_key(&BackupTier::Local));

        assert_eq!(mapping.get(&BackupTier::Hot).unwrap().len(), 1);
        assert_eq!(mapping.get(&BackupTier::Cold).unwrap().len(), 3);
        assert_eq!(mapping.get(&BackupTier::Local).unwrap().len(), 3);
    }

    #[test]
    fn test_storage_calculation() {
        let config = DisasterRecoveryConfig::new_production();
        let daily_tb = config.calculate_daily_storage();

        // Should be reasonable (less than 1 TB per day with compression)
        assert!(daily_tb > 0.0);
        assert!(daily_tb < 1.0);
    }

    #[test]
    fn test_cost_estimation() {
        let config = DisasterRecoveryConfig::new_production();
        let monthly_cost = config.estimate_monthly_cost();

        // Should be reasonable (less than $1000/month)
        assert!(monthly_cost > 0.0);
        assert!(monthly_cost < 1000.0);
    }

    #[test]
    fn test_configuration_verification() {
        let config = DisasterRecoveryConfig::new_production();
        let result = config.verify_configuration();
        assert!(result.is_ok());
    }

    #[test]
    fn test_rto_rpo() {
        let config = DisasterRecoveryConfig::new_production();

        // RTO checks
        assert_eq!(config.get_rto_minutes(BackupTier::Local), 15);
        assert_eq!(config.get_rto_minutes(BackupTier::Hot), 60);
        assert_eq!(config.get_rto_minutes(BackupTier::Warm), 240);
        assert_eq!(config.get_rto_minutes(BackupTier::Cold), 1440);

        // RPO check (with WAL should be 5 minutes)
        assert_eq!(config.get_rpo_minutes(), 5);
    }

    #[test]
    fn test_restore_configs() {
        let full_restore = RestoreConfig::new_full_restore();
        assert_eq!(full_restore.restore_type, RestoreType::Full);
        assert!(full_restore.verify_after_restore);

        let pitr_restore = RestoreConfig::new_pitr_restore("2025-01-01T00:00:00Z".to_string());
        assert_eq!(pitr_restore.restore_type, RestoreType::PointInTime);
        assert!(pitr_restore.target_time.is_some());
    }

    #[test]
    fn test_backend_configs() {
        let config = BackendBackupConfig::new_production();

        // CockroachDB
        assert!(config.cockroachdb.destination.starts_with("s3://"));
        assert_eq!(config.cockroachdb.full_schedule, "0 */6 * * *");

        // Redis
        assert!(config.redis.rdb_enabled);
        assert!(config.redis.aof_enabled);
        assert_eq!(config.redis.rdb_schedule_seconds.len(), 3);

        // MinIO
        assert!(config.minio.versioning);
        assert!(config.minio.replication_target.is_some());

        // TiKV
        assert_eq!(config.tikv.rate_limit_mb, 100);
        assert!(config.tikv.checksum_verify);
    }
}
