// dchat Deployment Infrastructure
// Handles multi-region validator deployment, relay network, and distributed storage

pub mod multi_region_config;
pub mod relay_network;
pub mod distributed_storage;

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
