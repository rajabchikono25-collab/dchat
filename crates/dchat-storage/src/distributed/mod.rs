// Distributed storage module
//
// This module provides distributed storage backends for production deployment:
// - CockroachDB: Multi-region SQL database
// - Redis Cluster: Distributed cache
// - MinIO/S3: Object storage for media
// - TiKV: Blockchain state storage

pub mod database;
pub mod cache;
pub mod object_storage;
pub mod tikv_backend;

pub use database::{DistributedDatabase, DatabaseConfig};
pub use cache::{DistributedCache, CacheConfig};
pub use object_storage::{DistributedObjectStorage, ObjectStorageConfig, StorageTier};
pub use tikv_backend::{TiKVStorage, TiKVConfig, ChainState, BlockMetadata};
