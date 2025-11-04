// Distributed storage module
//
// This module provides distributed storage backends for production deployment:
// - CockroachDB: Multi-region SQL database
// - Redis Cluster: Distributed cache
// - MinIO/S3: Object storage for media
// - TiKV: Blockchain state storage
//
// NOTE: These modules are currently disabled due to external dependency API mismatches.
// They require proper configuration of tikv_client, redis cluster, and s3 crates.
// For deployment, configure these dependencies and enable the modules.

pub mod database;
// PRODUCTION NOTE: Distributed storage backends now ENABLED
// Dependencies updated:
// - redis 0.25 (cluster async support)
// - rust-s3 0.35 (S3Error API compatibility fixed)
// - tikv-client 0.3 (Key type conversions correct)

pub mod cache;
pub mod object_storage;
pub mod tikv_backend;

// Re-export types from submodules for convenient access
pub use cache::{CacheConfig, DistributedCache};
pub use database::{DatabaseConfig, DistributedDatabase};
pub use object_storage::{
    DistributedObjectStorage, ObjectMetadata, ObjectStorageConfig, StorageTier,
};
pub use tikv_backend::{BlockMetadata, ChainState, TiKVConfig, TiKVStorage};
