//! Shard Rebalancing Integration
//!
//! Extends ShardManager with advanced rebalancing capabilities.
//! Maintains backward compatibility with existing sharding.rs implementation.

use super::load_monitoring::LoadMetricsAggregator;
use super::rebalancing::{RebalancingAlgorithm, RebalancingPlan, RebalancingScheduler, ShardLoad};
use super::state_migration::{MigrationCoordinator, MigrationId, ShardSnapshot};
use crate::sharding::{ChannelId, ShardId, ShardManager};
use dchat_core::error::Result;
use std::collections::HashMap;

/// Extended shard manager with rebalancing
pub struct ExtendedShardManager {
    /// Base shard manager (existing implementation)
    base: ShardManager,
    /// Load monitoring
    load_aggregator: LoadMetricsAggregator,
    /// Rebalancing scheduler
    scheduler: RebalancingScheduler,
    /// Migration coordinator
    migration_coordinator: MigrationCoordinator,
}

impl ExtendedShardManager {
    pub fn new(base: ShardManager) -> Self {
        let shard_ids: Vec<ShardId> = (0..base.config.num_shards).map(ShardId).collect();

        let mut load_aggregator = LoadMetricsAggregator::new();
        for shard_id in &shard_ids {
            load_aggregator.register_shard(shard_id.clone());
        }

        Self {
            base,
            load_aggregator,
            scheduler: RebalancingScheduler::new(&shard_ids),
            migration_coordinator: MigrationCoordinator::new(),
        }
    }

    /// Access base shard manager
    pub fn base(&self) -> &ShardManager {
        &self.base
    }

    /// Access base shard manager mutably
    pub fn base_mut(&mut self) -> &mut ShardManager {
        &mut self.base
    }

    /// Record message for load tracking
    pub fn record_message(&mut self, shard_id: &ShardId) {
        self.load_aggregator.record_message(shard_id);
    }

    /// Update CPU and memory metrics
    pub fn update_cpu_memory(&mut self, shard_id: &ShardId, cpu_usage: f64, memory_bytes: u64) {
        self.load_aggregator
            .update_cpu_memory(shard_id, cpu_usage, memory_bytes);
    }

    /// Check if rebalancing is needed
    pub fn should_rebalance(&self) -> bool {
        let metrics = self.load_aggregator.collect_metrics();
        let shard_loads: Vec<ShardLoad> = metrics
            .iter()
            .map(|m| ShardLoad {
                shard_id: m.shard_id.clone(),
                cpu_usage: m.cpu_usage,
                memory_usage: m.memory_usage_bytes as f64 / (1024.0 * 1024.0 * 1024.0), // Convert to GB ratio
                throughput_msg_s: m.throughput_msg_per_sec,
                storage_bytes: m.storage_bytes,
                channel_count: self.base.get_channel_count(&m.shard_id).unwrap_or(0),
            })
            .collect();

        self.scheduler.should_rebalance(&shard_loads)
    }

    /// Create rebalancing plan
    pub fn create_rebalancing_plan(
        &mut self,
        algorithm: RebalancingAlgorithm,
    ) -> Result<RebalancingPlan> {
        let metrics = self.load_aggregator.collect_metrics();
        let shard_loads: Vec<ShardLoad> = metrics
            .iter()
            .map(|m| ShardLoad {
                shard_id: m.shard_id.clone(),
                cpu_usage: m.cpu_usage,
                memory_usage: m.memory_usage_bytes as f64 / (1024.0 * 1024.0 * 1024.0), // Convert to GB ratio
                throughput_msg_s: m.throughput_msg_per_sec,
                storage_bytes: m.storage_bytes,
                channel_count: self.base.get_channel_count(&m.shard_id).unwrap_or(0),
            })
            .collect();

        // Get channel assignments from base manager
        let channel_assignments = self.base.get_all_channel_assignments();

        self.scheduler
            .create_plan(&shard_loads, &channel_assignments, algorithm)
    }

    /// Set pending rebalancing plan (requires operator approval)
    pub fn set_pending_plan(&mut self, plan: RebalancingPlan) {
        self.scheduler.set_pending_plan(plan);
    }

    /// Get pending plan
    pub fn get_pending_plan(&self) -> Option<&RebalancingPlan> {
        self.scheduler.get_pending_plan()
    }

    /// Approve pending plan
    pub fn approve_plan(&mut self) -> Option<RebalancingPlan> {
        self.scheduler.approve_plan()
    }

    /// Reject pending plan
    pub fn reject_plan(&mut self) {
        self.scheduler.reject_plan();
    }

    /// Execute approved rebalancing plan
    pub fn execute_rebalancing(&mut self, plan: RebalancingPlan) -> Result<Vec<MigrationId>> {
        let mut migration_ids = Vec::new();

        // Group migrations by source shard
        let mut migrations_by_shard: HashMap<ShardId, Vec<ChannelId>> = HashMap::new();
        for migration in &plan.migrations {
            migrations_by_shard
                .entry(migration.from_shard.clone())
                .or_default()
                .push(migration.channel_id.clone());
        }

        // Execute migrations
        for (source_shard, channels) in migrations_by_shard {
            // Find destination shard (simplified - use first migration's target)
            let dest_shard = plan
                .migrations
                .iter()
                .find(|m| m.from_shard == source_shard)
                .map(|m| m.to_shard.clone())
                .unwrap_or_else(|| ShardId(0));

            // Create snapshot
            let snapshot = self.create_shard_snapshot(&source_shard, &channels)?;

            // Execute migration
            let _stats = self.migration_coordinator.execute_migration(
                source_shard,
                dest_shard,
                channels,
                snapshot,
            )?;

            migration_ids.push(MigrationId::new());
        }

        Ok(migration_ids)
    }

    /// Create snapshot of shard state
    ///
    /// Queries the shard manager for actual state data and creates a consistent snapshot
    fn create_shard_snapshot(
        &self,
        shard_id: &ShardId,
        channels: &[ChannelId],
    ) -> Result<ShardSnapshot> {
        // Query shard state from base manager (synchronous access)
        let shard_state = self
            .base
            .shards
            .try_read()
            .map_err(|_| dchat_core::error::Error::internal("Failed to acquire shard lock"))?
            .get(shard_id)
            .cloned()
            .ok_or_else(|| {
                dchat_core::error::Error::validation(format!("Shard {} not found", shard_id))
            })?;

        // Serialize the channel states
        let channel_data: Vec<(&ChannelId, bool)> = channels
            .iter()
            .map(|c| (c, shard_state.channels.contains(c)))
            .collect();

        let serialized_state = serde_json::to_vec(&channel_data)
            .map_err(|e| dchat_core::error::Error::validation(e.to_string()))?;

        // Compute state root as BLAKE3 hash of serialized state
        let state_root = blake3::hash(&serialized_state).as_bytes().to_vec();

        Ok(ShardSnapshot::new(
            shard_id.clone(),
            channels.to_vec(),
            state_root,
            shard_state.message_count,
            serialized_state,
        ))
    }

    /// Check if in low-activity window (for safe rebalancing)
    pub fn is_low_activity_window() -> bool {
        RebalancingScheduler::is_low_activity_window()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sharding::ShardConfig;

    #[test]
    fn test_extended_manager_creation() {
        let config = ShardConfig::default();
        let base = ShardManager::new(config);
        let _extended = ExtendedShardManager::new(base);
        // Manager created successfully
    }

    #[test]
    fn test_load_tracking() {
        let config = ShardConfig::default();
        let base = ShardManager::new(config);
        let mut extended = ExtendedShardManager::new(base);

        extended.record_message(&ShardId(0));
        extended.update_cpu_memory(&ShardId(0), 0.5, 1_000_000_000);

        // Should not trigger rebalancing with light load
        assert!(!extended.should_rebalance());
    }

    #[test]
    fn test_create_rebalancing_plan() {
        let config = ShardConfig::default();
        let base = ShardManager::new(config);
        let mut extended = ExtendedShardManager::new(base);

        // Try creating plan (may be empty if no imbalance)
        let result = extended.create_rebalancing_plan(RebalancingAlgorithm::GreedyBinPacking);

        assert!(result.is_ok());
    }

    #[test]
    fn test_operator_approval_workflow() {
        let config = ShardConfig::default();
        let base = ShardManager::new(config);
        let mut extended = ExtendedShardManager::new(base);

        let plan = extended
            .create_rebalancing_plan(RebalancingAlgorithm::GreedyBinPacking)
            .unwrap();

        // Set pending
        extended.set_pending_plan(plan.clone());
        assert!(extended.get_pending_plan().is_some());

        // Approve
        let approved = extended.approve_plan();
        assert!(approved.is_some());

        // No longer pending
        assert!(extended.get_pending_plan().is_none());
    }
}
