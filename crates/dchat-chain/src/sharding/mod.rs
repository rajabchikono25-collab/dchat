//! Advanced Shard Rebalancing Modules
//!
//! Task 12: Shard Rebalancing (ARCHITECTURE-2.0.md)

pub mod integration;
pub mod load_monitoring;
pub mod rebalancing;
pub mod state_migration;

// Re-export key types
pub use integration::ExtendedShardManager;
pub use load_monitoring::{LoadMetrics, LoadMetricsAggregator, PrometheusExporter};
pub use rebalancing::{
    ConsistentHashRing, RebalancingAlgorithm, RebalancingPlan, RebalancingScheduler, ShardLoad,
};
pub use state_migration::{
    MigrationCoordinator, MigrationId, MigrationPhase, RollbackManager, ShardSnapshot,
    StateVerification, StreamingTransfer, TransferStats, TwoPhaseCommit,
};
