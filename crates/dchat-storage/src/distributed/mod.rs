//! Distributed storage module
//!
//! This module provides distributed storage backends for production deployment:
//! - CockroachDB: Multi-region SQL database
//! - Redis Cluster: Distributed cache
//! - MinIO/S3: Object storage for media
//! - TiKV: Blockchain state storage
//!
//! Each backend has a resilient wrapper with:
//! - Circuit breaker to prevent cascade failures
//! - Local cache for hot data
//! - Automatic retry with exponential backoff
//! - Write-behind queues for graceful degradation
//! - Health monitoring and metrics
//!
//! PRODUCTION NOTE: Distributed storage backends now ENABLED
//! Dependencies updated:
//! - redis 0.25 (cluster async support)
//! - rust-s3 0.35 (S3Error API compatibility fixed)
//! - tikv-client 0.3 (Key type conversions correct)
//! - parking_lot 0.12 (synchronization primitives)

// Base storage backends
pub mod cache;
pub mod database;
pub mod object_storage;
pub mod tikv_backend;

// Resilient wrappers with fault tolerance
pub mod resilient_cache;
pub mod resilient_database;
pub mod resilient_object_storage;
pub mod resilient_tikv;

// Re-export base types
pub use cache::{CacheConfig, DistributedCache};
pub use database::{DatabaseConfig, DistributedDatabase};
pub use object_storage::{
    DistributedObjectStorage, ObjectMetadata, ObjectStorageConfig, StorageTier,
};
pub use tikv_backend::{BlockMetadata, ChainState, TiKVConfig, TiKVStorage};

// Re-export resilient wrappers (production recommended)
pub use resilient_cache::{ResilientCache, ResilientCacheConfig};
pub use resilient_database::{ResilientDatabase, ResilientDatabaseConfig};
pub use resilient_object_storage::{ResilientObjectStorage, ResilientObjectStorageConfig};
pub use resilient_tikv::{CacheStats, ResilientTiKVConfig, ResilientTiKVStorage, SyncResult};
