//! dchat-storage: Local and distributed data persistence layer
//!
//! This crate provides:
//! - SQLite database for messages, identities, and metadata (local)
//! - Distributed storage architecture (CockroachDB, Redis, MinIO, TiKV)
//! - Encrypted backup and restore
//! - Message deduplication via content addressing
//! - TTL-based data lifecycle management
//! - Storage economics (bonds, quotas)

pub mod backup;
pub mod compression;
pub mod database;
pub mod deduplication;
pub mod distributed;
pub mod economics;
pub mod file_upload;
pub mod lifecycle;
pub mod migrations;
pub mod schema;
pub mod tier_management;

pub use backup::{BackupManager, EncryptedBackup};
pub use compression::{
    CompressionAlgorithm, CompressionConfig, CompressionEngine, CompressionLevel,
    CompressionResult,
};
pub use database::{Database, DatabaseConfig, MessageRow};
pub use deduplication::{ContentAddressable, DeduplicationStore};
pub use distributed::{
    ChainState, ConsistencyLevel, DistributedCache, DistributedDatabase,
    DistributedObjectStorage, ObjectMetadata, RegionHealth, StorageConfig, StorageHealthReport,
    StorageManager, TiKVStorage,
};
pub use economics::{
    EconomicsConfig, MicropaymentStream, StorageBond, StorageEconomicsManager,
};
pub use file_upload::{
    FileUploadManager, MediaFileType, StorageStats, UploadConfig, UploadedFile,
};
pub use lifecycle::{LifecycleManager, TtlConfig};
pub use migrations::{Migration, MigrationRunner, MIGRATIONS};
pub use schema::Schema;
pub use tier_management::{
    RetentionPolicyAdvanced, StorageTierAdvanced, TierMigrationManager,
};
