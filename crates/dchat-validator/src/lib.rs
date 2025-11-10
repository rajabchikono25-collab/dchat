// dchat-validator crate
//
// This crate provides validator node functionality for the dchat blockchain network,
// including multi-region deployment, BFT consensus coordination, and health monitoring.

pub mod health;
pub mod multi_region;
pub mod validator; // Validator thresholds and core logic

// Re-export validator submodule
pub use validator::thresholds;

pub use multi_region::{
    BftConfig, GeographicRegion, HardwareRequirements, HealthStatus, MultiRegionCoordinator,
    MultiRegionError, RegionStats, ValidatorConfig, ValidatorHealth, ValidatorSignature,
};

pub use health::{
    BlockHeightCheck, EnhancedHealthCheck, EnhancedHealthChecker, HealthCheckError, HealthProbe,
    SignatureFreshnessCheck, HEALTH_CHECK_TIMEOUT_SECS, MAX_ACCEPTABLE_LATENCY_MS,
    MAX_BLOCK_HEIGHT_LAG, MAX_CONSECUTIVE_FAILURES, SIGNATURE_FRESHNESS_SECS,
};
