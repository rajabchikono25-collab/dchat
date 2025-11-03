// dchat Deployment Infrastructure
// Handles multi-region validator deployment and relay network management

pub mod multi_region_config;
pub mod relay_network;

pub use multi_region_config::{
    GeographicRegion, ValidatorConfig, ConsensusConfig, StorageConfig,
    StorageBackend, MultiRegionConfig, ConfigError,
};

pub use relay_network::{
    RelayConfig, RelayNetworkConfig, RelayTier, RelayReputation,
    IncentiveConfig, RelayError,
};
