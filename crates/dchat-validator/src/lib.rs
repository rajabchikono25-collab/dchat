// dchat-validator crate
//
// This crate provides validator node functionality for the dchat blockchain network,
// including multi-region deployment, BFT consensus coordination, and health monitoring.

pub mod multi_region;
pub mod health;
pub mod validator; // Validator thresholds and core logic

// Re-export validator submodule
pub use validator::thresholds;

pub use multi_region::{
    BftConfig, GeographicRegion, HardwareRequirements, HealthStatus, MultiRegionCoordinator,
    MultiRegionError, RegionStats, ValidatorConfig, ValidatorHealth, ValidatorSignature,
};

pub use health::{
    EnhancedHealthChecker, EnhancedHealthCheck, HealthProbe,
    BlockHeightCheck, SignatureFreshnessCheck, HealthCheckError,
    MAX_CONSECUTIVE_FAILURES, MAX_ACCEPTABLE_LATENCY_MS, MAX_BLOCK_HEIGHT_LAG,
    SIGNATURE_FRESHNESS_SECS, HEALTH_CHECK_TIMEOUT_SECS,
};
