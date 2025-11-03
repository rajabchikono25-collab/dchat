// dchat-validator crate
//
// This crate provides validator node functionality for the dchat blockchain network,
// including multi-region deployment, BFT consensus coordination, and health monitoring.

pub mod multi_region;

pub use multi_region::{
    BftConfig, GeographicRegion, HardwareRequirements, HealthStatus, MultiRegionCoordinator,
    MultiRegionError, RegionStats, ValidatorConfig, ValidatorHealth, ValidatorSignature,
};
