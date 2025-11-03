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
// TODO: Fix API compatibility issues before enabling
// pub mod cache;
// pub mod object_storage;
// pub mod tikv_backend;

pub use database::{DistributedDatabase, DatabaseConfig};
// pub use cache::{DistributedCache, CacheConfig};
// pub use object_storage::{DistributedObjectStorage, ObjectStorageConfig, StorageTier};
// pub use tikv_backend::{TiKVStorage, TiKVConfig, ChainState, BlockMetadata};

// Stub types for compilation - replace with real implementations when dependencies are configured
#[derive(Debug, Clone)]
pub struct CacheConfig;
#[derive(Debug, Clone)]
pub struct DistributedCache;
#[derive(Debug, Clone)]
pub struct ObjectStorageConfig;
#[derive(Debug, Clone)]
pub struct DistributedObjectStorage;
#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub key: String,
    pub size: u64,
    pub content_type: String,
}
#[derive(Debug, Clone, Copy)]
pub enum StorageTier { Hot, Warm, Cold, Archive }
#[derive(Debug, Clone)]
pub struct TiKVConfig;
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
