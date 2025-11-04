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
// Enable production-ready distributed storage implementations
// TODO: Fix type errors in cache.rs and object_storage.rs before enabling
// pub mod cache;
// pub mod object_storage;
// TODO: Fix TiKV API compatibility before enabling
// pub mod tikv_backend;

pub use database::{DistributedDatabase, DatabaseConfig};
// pub use cache::{DistributedCache, CacheConfig};
// pub use object_storage::{DistributedObjectStorage, ObjectStorageConfig, StorageTier, ObjectMetadata};

// Stub types until cache and object_storage are fixed
#[derive(Debug, Clone)]
pub struct DistributedCache;
#[derive(Debug, Clone)]
pub struct CacheConfig;
#[derive(Debug, Clone)]
pub struct DistributedObjectStorage;
#[derive(Debug, Clone)]
pub struct ObjectStorageConfig;
#[derive(Debug, Clone)]
pub struct StorageTier;
#[derive(Debug, Clone)]
pub struct ObjectMetadata;
// pub use tikv_backend::{TiKVStorage, TiKVConfig, ChainState, BlockMetadata};

// TiKV stub types (will be enabled after dependency resolution)
#[derive(Debug, Clone)]
pub struct TiKVConfig {
    pub pd_endpoints: Vec<String>,
    pub enable_pessimistic_txn: bool,
}
#[derive(Debug, Clone)]
pub struct TiKVStorage;
#[derive(Debug, Clone)]
pub struct ChainState {
    pub block_height: u64,
    pub block_hash: [u8; 32],
    pub state_root: [u8; 32],
}
#[derive(Debug, Clone)]
pub struct BlockMetadata {
    pub height: u64,
    pub timestamp: i64,
}
