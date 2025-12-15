//! dchat-storage: Local and distributed data persistence layer
//!
//! This crate provides:
//! - SQLite database for messages, identities, and metadata (local)
//! - Distributed storage architecture (CockroachDB, Redis, MinIO, TiKV)
//! - Encrypted backup and restore
//! - Message deduplication via content addressing
//! - TTL-based data lifecycle management
//! - Storage economics (bonds, quotas)
//! - Provider marketplace (ObjectS3, IpfsPinning, ArchiveObject)
//! - Tiered storage routing with automatic replication

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
pub mod provider;
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
    BlockMetadata,
    CacheConfig,
    // Resilient wrappers (production recommended)
    CacheStats,
    ChainState,
    DatabaseConfig as DistributedDatabaseConfig,
    DistributedCache,
    DistributedDatabase,
    DistributedObjectStorage,
    ObjectMetadata,
    ObjectStorageConfig,
    ResilientCache,
    ResilientCacheConfig,
    ResilientDatabase,
    ResilientDatabaseConfig,
    ResilientObjectStorage,
    ResilientObjectStorageConfig,
    ResilientTiKVConfig,
    ResilientTiKVStorage,
    StorageTier,
    SyncResult,
    TiKVConfig,
    TiKVStorage,
};
pub use economics::{
    // Production-grade bonds (recommended for production)
    BondCreationResult,
    BondError,
    BondOperation,
    BondSignature,
    BondStatistics,
    BondStatus,
    CreateBondRequest,
    // SQLite-based economics
    EconomicsConfig,
    MicropaymentStream,
    ProductionBond,
    ProductionBondConfig,
    ProductionBondManager,
    StorageBond,
    StorageEconomicsManager,
    StorageProvider,
    WithdrawBondRequest,
    WithdrawalResult,
};
pub use error::{StorageError, StorageResult};
pub use file_upload::{FileUploadManager, MediaFileType, StorageStats, UploadConfig, UploadedFile};
pub use ipfs::{Cid, IpfsClient, IpfsConfig, IpfsDirectory, IpfsFile, PinStatus, PinType};
pub use lifecycle::{LifecycleManager, TtlConfig};
pub use migrations::{Migration, MigrationRunner, MIGRATIONS};
pub use provider::{
    // Provider capabilities
    ArchiveObjectCapability,
    // Blob references
    BlobCodec,
    BlobLocation,
    BlobRef,
    // Challenge system
    ChallengeProof,
    ChallengeResult,
    ChallengeStatus,
    IpfsPinningCapability,
    LocationStatus,
    ObjectS3Capability,
    ProviderCapabilities,
    ProviderCapability,
    // Provider registry
    ProviderRegistry,
    ProviderRegistryConfig,
    // Provider selection
    ProviderSelection,
    RegisteredProvider,
    ReplicationConfig,
    SelectionCriteria,
    StorageChallenge,
    StorageChallengeManager,
    StorageLimits,
    // Storage router
    StorageRouter,
    StorageRouterConfig,
};
pub use resilience::{
    BackendHealth, CircuitBreaker, CircuitBreakerConfig, CircuitState, FallbackManager,
    HealthMonitor, HealthMonitorConfig, HealthStatus, LocalCache, LocalCacheConfig, RetryConfig,
    RetryExecutor,
};
pub use schema::Schema;
pub use tier_management::{RetentionPolicyAdvanced, StorageTierAdvanced, TierMigrationManager};
