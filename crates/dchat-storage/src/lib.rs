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
pub mod error;
pub mod file_upload;
pub mod ipfs;
pub mod lifecycle;
pub mod migrations;
pub mod resilience;
pub mod schema;
pub mod tier_management;

pub use backup::{BackupManager, EncryptedBackup};
pub use compression::{
    CompressionAlgorithm, CompressionConfig, CompressionEngine, CompressionLevel, CompressionResult,
};
pub use database::{Database, DatabaseConfig, MessageRow};
pub use deduplication::{ContentAddressable, DeduplicationStore};
pub use distributed::{
    // Base backends
    BlockMetadata, CacheConfig, ChainState, DatabaseConfig as DistributedDatabaseConfig,
    DistributedCache, DistributedDatabase, DistributedObjectStorage, ObjectMetadata,
    ObjectStorageConfig, StorageTier, TiKVConfig, TiKVStorage,
    // Resilient wrappers (production recommended)
    CacheStats, ResilientCache, ResilientCacheConfig, ResilientDatabase, ResilientDatabaseConfig,
    ResilientObjectStorage, ResilientObjectStorageConfig, ResilientTiKVConfig,
    ResilientTiKVStorage, SyncResult,
};
pub use resilience::{
    BackendHealth, CircuitBreaker, CircuitBreakerConfig, CircuitState, FallbackManager,
    HealthMonitor, HealthMonitorConfig, HealthStatus, LocalCache, LocalCacheConfig,
    RetryConfig, RetryExecutor,
};
pub use economics::{
    // SQLite-based economics
    EconomicsConfig, MicropaymentStream, StorageBond, StorageEconomicsManager,
    // Production-grade bonds (recommended for production)
    BondCreationResult, BondError, BondOperation, BondSignature, BondStatistics, BondStatus,
    CreateBondRequest, ProductionBond, ProductionBondConfig, ProductionBondManager,
    StorageProvider, WithdrawBondRequest, WithdrawalResult,
};
pub use error::{StorageError, StorageResult};
pub use file_upload::{FileUploadManager, MediaFileType, StorageStats, UploadConfig, UploadedFile};
pub use ipfs::{Cid, IpfsClient, IpfsConfig, IpfsDirectory, IpfsFile, PinStatus, PinType};
pub use lifecycle::{LifecycleManager, TtlConfig};
pub use migrations::{Migration, MigrationRunner, MIGRATIONS};
pub use schema::Schema;
pub use tier_management::{RetentionPolicyAdvanced, StorageTierAdvanced, TierMigrationManager};
