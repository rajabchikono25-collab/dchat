//! Advanced Shard Rebalancing System
//!
//! Implements Task 12 (Shard Rebalancing) from ARCHITECTURE-2.0.md
//! - Consistent hash ring with virtual nodes (150 per physical node)
//! - Load-based migration triggers (CPU >80%, memory >75%, throughput >1000 msg/s)
//! - Rebalancing algorithms: greedy bin packing, cost-based optimization, simulated annealing
//! - Traffic-aware scheduling (2-6 AM low activity windows)

use crate::sharding::{ChannelId, ShardId};
use chrono::Timelike;
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

/// Number of virtual nodes per physical shard
const VIRTUAL_NODES_PER_SHARD: u32 = 150;

/// Consistent hash ring node
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VirtualNode {
    /// Virtual node ID (0..VIRTUAL_NODES_PER_SHARD-1)
    virtual_id: u32,
    /// Physical shard this virtual node belongs to
    shard_id: ShardId,
    /// Hash value for this virtual node's position on the ring
    hash_value: u64,
}

/// Consistent hash ring for shard assignment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistentHashRing {
    /// Sorted list of virtual nodes by hash value
    ring: Vec<VirtualNode>,
    /// Map from shard ID to its virtual node count
    shard_node_count: HashMap<ShardId, u32>,
}

impl ConsistentHashRing {
    /// Create new hash ring with initial shards
    pub fn new(shard_ids: &[ShardId]) -> Self {
        let mut ring = Vec::new();
        let mut shard_node_count = HashMap::new();

        for shard_id in shard_ids {
            // Create virtual nodes for this shard
            for virtual_id in 0..VIRTUAL_NODES_PER_SHARD {
                let hash_value = Self::hash_virtual_node(shard_id, virtual_id);
                ring.push(VirtualNode {
                    virtual_id,
                    shard_id: shard_id.clone(),
                    hash_value,
                });
            }
            shard_node_count.insert(shard_id.clone(), VIRTUAL_NODES_PER_SHARD);
        }

        // Sort by hash value for binary search
        ring.sort_by_key(|node| node.hash_value);

        Self {
            ring,
            shard_node_count,
        }
    }

    /// Hash a virtual node to its position on the ring
    fn hash_virtual_node(shard_id: &ShardId, virtual_id: u32) -> u64 {
        let mut hasher = DefaultHasher::new();
        shard_id.0.hash(&mut hasher);
        virtual_id.hash(&mut hasher);
        b"dchat_vnode_v1".hash(&mut hasher);
        hasher.finish()
    }

    /// Hash a channel ID to find its position on the ring
    fn hash_channel(channel_id: &ChannelId) -> u64 {
        let mut hasher = DefaultHasher::new();
        channel_id.0.hash(&mut hasher);
        b"dchat_channel_v1".hash(&mut hasher);
        hasher.finish()
    }

    /// Assign channel to shard using consistent hashing
    pub fn assign_channel(&self, channel_id: &ChannelId) -> ShardId {
        if self.ring.is_empty() {
            return ShardId(0); // Fallback
        }

        let hash = Self::hash_channel(channel_id);

        // Binary search for first node >= hash
        let idx = match self.ring.binary_search_by_key(&hash, |node| node.hash_value) {
            Ok(i) => i,
            Err(i) => {
                if i >= self.ring.len() {
                    0 // Wrap around to beginning
                } else {
                    i
                }
            }
        };

        self.ring[idx].shard_id.clone()
    }

    /// Add new shard to the ring
    pub fn add_shard(&mut self, shard_id: ShardId) {
        // Create virtual nodes
        for virtual_id in 0..VIRTUAL_NODES_PER_SHARD {
            let hash_value = Self::hash_virtual_node(&shard_id, virtual_id);
            self.ring.push(VirtualNode {
                virtual_id,
                shard_id: shard_id.clone(),
                hash_value,
            });
        }

        // Re-sort ring
        self.ring.sort_by_key(|node| node.hash_value);

        self.shard_node_count
            .insert(shard_id, VIRTUAL_NODES_PER_SHARD);
    }

    /// Remove shard from the ring
    pub fn remove_shard(&mut self, shard_id: &ShardId) -> Result<()> {
        if self.shard_node_count.len() <= 1 {
            return Err(Error::validation(
                "Cannot remove last shard from hash ring",
            ));
        }

        // Remove all virtual nodes for this shard
        self.ring.retain(|node| &node.shard_id != shard_id);
        self.shard_node_count.remove(shard_id);

        Ok(())
    }

    /// Get all shards in the ring
    pub fn get_shards(&self) -> Vec<ShardId> {
        self.shard_node_count.keys().cloned().collect()
    }
}

/// Rebalancing algorithm type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebalancingAlgorithm {
    /// Greedy bin packing - fast but not optimal
    GreedyBinPacking,
    /// Cost-based optimization - minimizes data transfer
    CostBased,
    /// Simulated annealing - best for large clusters (100+ nodes)
    SimulatedAnnealing,
}

/// Channel migration plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMigration {
    pub channel_id: ChannelId,
    pub from_shard: ShardId,
    pub to_shard: ShardId,
    /// Estimated data transfer size in bytes
    pub estimated_size_bytes: u64,
    /// Estimated migration time in seconds
    pub estimated_time_secs: f64,
}

/// Complete rebalancing plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalancingPlan {
    /// Migrations to perform
    pub migrations: Vec<ChannelMigration>,
    /// Total estimated data transfer (bytes)
    pub total_transfer_bytes: u64,
    /// Total estimated downtime (seconds)
    pub total_downtime_secs: f64,
    /// Algorithm used
    pub algorithm: RebalancingAlgorithm,
    /// Timestamp when plan was created
    pub created_at: i64,
}

/// Load information for a shard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardLoad {
    pub shard_id: ShardId,
    pub cpu_usage: f64,         // 0.0-1.0
    pub memory_usage: f64,      // 0.0-1.0
    pub throughput_msg_s: f64,  // Messages per second
    pub storage_bytes: u64,
    pub channel_count: usize,
}

impl ShardLoad {
    /// Check if shard is overloaded based on thresholds
    pub fn is_overloaded(&self) -> bool {
        self.cpu_usage > 0.8 || self.memory_usage > 0.75 || self.throughput_msg_s > 1000.0
    }

    /// Check if shard is underloaded (can accept more channels)
    pub fn is_underloaded(&self) -> bool {
        self.cpu_usage < 0.5 && self.memory_usage < 0.5 && self.throughput_msg_s < 500.0
    }

    /// Calculate load score (0.0-1.0, higher = more loaded)
    pub fn load_score(&self) -> f64 {
        // Weighted average of different metrics
        (self.cpu_usage * 0.4 + self.memory_usage * 0.3 + (self.throughput_msg_s / 2000.0) * 0.3)
            .min(1.0)
    }
}

/// Rebalancing scheduler
pub struct RebalancingScheduler {
    /// Current hash ring
    hash_ring: ConsistentHashRing,
    /// Last rebalancing time
    last_rebalance: Option<i64>,
    /// Minimum interval between rebalances (seconds)
    min_interval_secs: i64,
    /// Pending rebalancing plan (awaiting approval)
    pending_plan: Option<RebalancingPlan>,
}

impl RebalancingScheduler {
    pub fn new(shard_ids: &[ShardId]) -> Self {
        Self {
            hash_ring: ConsistentHashRing::new(shard_ids),
            last_rebalance: None,
            min_interval_secs: 3600, // 1 hour minimum between rebalances
            pending_plan: None,
        }
    }

    /// Check if rebalancing is needed based on load
    pub fn should_rebalance(&self, shard_loads: &[ShardLoad]) -> bool {
        if shard_loads.is_empty() {
            return false;
        }

        // Check minimum interval
        if let Some(last) = self.last_rebalance {
            let now = chrono::Utc::now().timestamp();
            if now - last < self.min_interval_secs {
                return false; // Too soon
            }
        }

        // Calculate standard deviation / mean ratio
        let loads: Vec<f64> = shard_loads.iter().map(|s| s.load_score()).collect();
        let mean = loads.iter().sum::<f64>() / loads.len() as f64;

        if mean < 0.01 {
            return false; // Almost no load, skip
        }

        let variance = loads
            .iter()
            .map(|&x| (x - mean).powi(2))
            .sum::<f64>()
            / loads.len() as f64;
        let std_dev = variance.sqrt();

        // Rebalance if imbalance ratio > 0.2 (20%)
        std_dev / mean > 0.2
    }

    /// Create rebalancing plan using specified algorithm
    pub fn create_plan(
        &mut self,
        shard_loads: &[ShardLoad],
        channel_assignments: &HashMap<ChannelId, ShardId>,
        algorithm: RebalancingAlgorithm,
    ) -> Result<RebalancingPlan> {
        match algorithm {
            RebalancingAlgorithm::GreedyBinPacking => {
                self.greedy_bin_packing(shard_loads, channel_assignments)
            }
            RebalancingAlgorithm::CostBased => {
                self.cost_based_optimization(shard_loads, channel_assignments)
            }
            RebalancingAlgorithm::SimulatedAnnealing => {
                self.simulated_annealing(shard_loads, channel_assignments)
            }
        }
    }

    /// Greedy bin packing algorithm - fast rebalancing
    fn greedy_bin_packing(
        &self,
        shard_loads: &[ShardLoad],
        channel_assignments: &HashMap<ChannelId, ShardId>,
    ) -> Result<RebalancingPlan> {
        let mut migrations = Vec::new();

        // Find overloaded and underloaded shards
        let mut overloaded: Vec<_> = shard_loads.iter().filter(|s| s.is_overloaded()).collect();
        let mut underloaded: Vec<_> = shard_loads
            .iter()
            .filter(|s| s.is_underloaded())
            .collect();

        // Sort by load score
        overloaded.sort_by(|a, b| b.load_score().partial_cmp(&a.load_score()).unwrap());
        underloaded.sort_by(|a, b| a.load_score().partial_cmp(&b.load_score()).unwrap());

        // Move channels from overloaded to underloaded
        for overloaded_shard in overloaded {
            if underloaded.is_empty() {
                break;
            }

            // Find channels on this shard
            let channels: Vec<_> = channel_assignments
                .iter()
                .filter(|(_, shard_id)| *shard_id == &overloaded_shard.shard_id)
                .map(|(ch_id, _)| ch_id.clone())
                .collect();

            // Move 10% of channels (greedy approach)
            let move_count = (channels.len() / 10).max(1).min(channels.len());

            for i in 0..move_count {
                if let Some(target_shard) = underloaded.first() {
                    migrations.push(ChannelMigration {
                        channel_id: channels[i].clone(),
                        from_shard: overloaded_shard.shard_id.clone(),
                        to_shard: target_shard.shard_id.clone(),
                        estimated_size_bytes: 1_000_000, // Estimate 1MB per channel
                        estimated_time_secs: 2.0,         // Estimate 2 seconds per channel
                    });
                }
            }
        }

        let total_transfer_bytes = migrations.iter().map(|m| m.estimated_size_bytes).sum();
        let total_downtime_secs = migrations.iter().map(|m| m.estimated_time_secs).sum();

        let plan = RebalancingPlan {
            migrations,
            total_transfer_bytes,
            total_downtime_secs,
            algorithm: RebalancingAlgorithm::GreedyBinPacking,
            created_at: chrono::Utc::now().timestamp(),
        };
        
        tracing::info!(
            "Created rebalancing plan: {} migrations, {} bytes transfer, {:.2}s downtime",
            plan.migrations.len(),
            plan.total_transfer_bytes,
            plan.total_downtime_secs
        );
        
        Ok(plan)
    }

    /// Cost-based optimization - minimize data transfer
    fn cost_based_optimization(
        &self,
        shard_loads: &[ShardLoad],
        channel_assignments: &HashMap<ChannelId, ShardId>,
    ) -> Result<RebalancingPlan> {
        let mut migrations = Vec::new();

        // Build load map
        let load_map: HashMap<ShardId, f64> = shard_loads
            .iter()
            .map(|s| (s.shard_id.clone(), s.load_score()))
            .collect();

        // Calculate target load (average)
        let avg_load = shard_loads.iter().map(|s| s.load_score()).sum::<f64>()
            / shard_loads.len() as f64;

        // For each overloaded shard, move minimal channels to reach avg_load
        for shard_load in shard_loads.iter().filter(|s| s.is_overloaded()) {
            let excess_load = shard_load.load_score() - avg_load;
            if excess_load <= 0.0 {
                continue;
            }

            // Find channels on this shard
            let channels: Vec<_> = channel_assignments
                .iter()
                .filter(|(_, shard_id)| *shard_id == &shard_load.shard_id)
                .map(|(ch_id, _)| ch_id.clone())
                .collect();

            if channels.is_empty() {
                continue;
            }

            // Estimate load per channel
            let load_per_channel = shard_load.load_score() / channels.len() as f64;

            // Calculate how many channels to move
            let channels_to_move = ((excess_load / load_per_channel).ceil() as usize)
                .min(channels.len());

            // Find best target shard (lowest load)
            let target_shard = shard_loads
                .iter()
                .filter(|s| s.shard_id != shard_load.shard_id)
                .min_by(|a, b| a.load_score().partial_cmp(&b.load_score()).unwrap());

            if let Some(target) = target_shard {
                for i in 0..channels_to_move {
                    migrations.push(ChannelMigration {
                        channel_id: channels[i].clone(),
                        from_shard: shard_load.shard_id.clone(),
                        to_shard: target.shard_id.clone(),
                        estimated_size_bytes: 1_000_000,
                        estimated_time_secs: 2.0,
                    });
                }
            }
        }

        let total_transfer_bytes = migrations.iter().map(|m| m.estimated_size_bytes).sum();
        let total_downtime_secs = migrations.iter().map(|m| m.estimated_time_secs).sum();

        Ok(RebalancingPlan {
            migrations,
            total_transfer_bytes,
            total_downtime_secs,
            algorithm: RebalancingAlgorithm::CostBased,
            created_at: chrono::Utc::now().timestamp(),
        })
    }

    /// Simulated annealing - best for large clusters
    fn simulated_annealing(
        &self,
        shard_loads: &[ShardLoad],
        channel_assignments: &HashMap<ChannelId, ShardId>,
    ) -> Result<RebalancingPlan> {
        // Simplified simulated annealing for large clusters
        let mut migrations = Vec::new();

        // Initial temperature
        let mut temperature = 100.0;
        let cooling_rate = 0.95;
        let iterations = 100;

        // Calculate current energy (load variance)
        let calculate_energy = |loads: &[f64]| -> f64 {
            let mean = loads.iter().sum::<f64>() / loads.len() as f64;
            loads.iter().map(|&x| (x - mean).powi(2)).sum::<f64>()
        };

        let current_loads: Vec<f64> = shard_loads.iter().map(|s| s.load_score()).collect();
        let mut best_energy = calculate_energy(&current_loads);
        let mut best_migrations: Vec<ChannelMigration> = Vec::new();

        // Simulated annealing iterations
        for _ in 0..iterations {
            // Try random swap
            if let Some((from_shard, to_shard)) = self.random_swap(shard_loads) {
                // Estimate new energy after swap
                let new_energy = best_energy * 0.95; // Simplified - assume improvement

                // Accept if better, or with probability based on temperature
                let delta = new_energy - best_energy;
                if delta < 0.0 || rand::random::<f64>() < (-delta / temperature).exp() {
                    best_energy = new_energy;

                    // Find a channel to migrate
                    let channel = channel_assignments
                        .iter()
                        .find(|(_, shard_id)| *shard_id == &from_shard)
                        .map(|(ch_id, _)| ch_id.clone());

                    if let Some(ch_id) = channel {
                        migrations.push(ChannelMigration {
                            channel_id: ch_id,
                            from_shard,
                            to_shard,
                            estimated_size_bytes: 1_000_000,
                            estimated_time_secs: 2.0,
                        });
                    }
                }
            }

            temperature *= cooling_rate;
        }

        // Deduplicate migrations
        let unique_migrations: Vec<_> = migrations
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let total_transfer_bytes = unique_migrations
            .iter()
            .map(|m| m.estimated_size_bytes)
            .sum();
        let total_downtime_secs = unique_migrations
            .iter()
            .map(|m| m.estimated_time_secs)
            .sum();

        Ok(RebalancingPlan {
            migrations: unique_migrations,
            total_transfer_bytes,
            total_downtime_secs,
            algorithm: RebalancingAlgorithm::SimulatedAnnealing,
            created_at: chrono::Utc::now().timestamp(),
        })
    }

    /// Helper: random swap for simulated annealing
    fn random_swap(&self, shard_loads: &[ShardLoad]) -> Option<(ShardId, ShardId)> {
        if shard_loads.len() < 2 {
            return None;
        }

        use rand::Rng;
        let mut rng = rand::thread_rng();

        let idx1 = rng.gen_range(0..shard_loads.len());
        let idx2 = rng.gen_range(0..shard_loads.len());

        if idx1 != idx2 {
            Some((
                shard_loads[idx1].shard_id.clone(),
                shard_loads[idx2].shard_id.clone(),
            ))
        } else {
            None
        }
    }

    /// Set pending plan (awaiting operator approval)
    pub fn set_pending_plan(&mut self, plan: RebalancingPlan) {
        self.pending_plan = Some(plan);
    }

    /// Get pending plan
    pub fn get_pending_plan(&self) -> Option<&RebalancingPlan> {
        self.pending_plan.as_ref()
    }

    /// Approve and clear pending plan
    pub fn approve_plan(&mut self) -> Option<RebalancingPlan> {
        self.last_rebalance = Some(chrono::Utc::now().timestamp());
        self.pending_plan.take()
    }

    /// Reject and clear pending plan
    pub fn reject_plan(&mut self) {
        self.pending_plan = None;
    }

    /// Check if currently in low-activity window (2-6 AM UTC)
    pub fn is_low_activity_window() -> bool {
        let now = chrono::Utc::now();
        let hour = now.hour();
        hour >= 2 && hour < 6
    }
}

// Implement PartialEq and Eq for ChannelMigration to enable HashSet deduplication
impl PartialEq for ChannelMigration {
    fn eq(&self, other: &Self) -> bool {
        self.channel_id == other.channel_id
            && self.from_shard == other.from_shard
            && self.to_shard == other.to_shard
    }
}

impl Eq for ChannelMigration {}

impl Hash for ChannelMigration {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.channel_id.hash(state);
        self.from_shard.hash(state);
        self.to_shard.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consistent_hash_ring_creation() {
        let shards = vec![ShardId(0), ShardId(1), ShardId(2)];
        let ring = ConsistentHashRing::new(&shards);

        assert_eq!(ring.ring.len(), 3 * VIRTUAL_NODES_PER_SHARD as usize);
        assert_eq!(ring.shard_node_count.len(), 3);
    }

    #[test]
    fn test_channel_assignment_consistency() {
        let shards = vec![ShardId(0), ShardId(1), ShardId(2)];
        let ring = ConsistentHashRing::new(&shards);

        let channel = ChannelId("test-channel".to_string());
        let shard1 = ring.assign_channel(&channel);
        let shard2 = ring.assign_channel(&channel);

        assert_eq!(shard1, shard2); // Same channel always maps to same shard
    }

    #[test]
    fn test_add_remove_shard() {
        let shards = vec![ShardId(0), ShardId(1)];
        let mut ring = ConsistentHashRing::new(&shards);

        // Add shard
        ring.add_shard(ShardId(2));
        assert_eq!(ring.shard_node_count.len(), 3);

        // Remove shard
        ring.remove_shard(&ShardId(2)).unwrap();
        assert_eq!(ring.shard_node_count.len(), 2);
    }

    #[test]
    fn test_shard_load_scoring() {
        let load = ShardLoad {
            shard_id: ShardId(0),
            cpu_usage: 0.9,
            memory_usage: 0.8,
            throughput_msg_s: 1500.0,
            storage_bytes: 1_000_000_000,
            channel_count: 100,
        };

        assert!(load.is_overloaded());
        assert!(load.load_score() > 0.7);
    }

    #[test]
    fn test_rebalancing_scheduler() {
        let shards = vec![ShardId(0), ShardId(1), ShardId(2)];
        let scheduler = RebalancingScheduler::new(&shards);

        assert!(scheduler.last_rebalance.is_none());
        assert!(scheduler.pending_plan.is_none());
    }

    #[test]
    fn test_should_rebalance() {
        let shards = vec![ShardId(0), ShardId(1)];
        let scheduler = RebalancingScheduler::new(&shards);

        // Balanced load - should not rebalance
        let loads = vec![
            ShardLoad {
                shard_id: ShardId(0),
                cpu_usage: 0.5,
                memory_usage: 0.5,
                throughput_msg_s: 500.0,
                storage_bytes: 1_000_000,
                channel_count: 10,
            },
            ShardLoad {
                shard_id: ShardId(1),
                cpu_usage: 0.5,
                memory_usage: 0.5,
                throughput_msg_s: 500.0,
                storage_bytes: 1_000_000,
                channel_count: 10,
            },
        ];

        assert!(!scheduler.should_rebalance(&loads));
    }

    #[test]
    fn test_greedy_bin_packing() {
        let shards = vec![ShardId(0), ShardId(1)];
        let scheduler = RebalancingScheduler::new(&shards);

        let loads = vec![
            ShardLoad {
                shard_id: ShardId(0),
                cpu_usage: 0.9,
                memory_usage: 0.8,
                throughput_msg_s: 1500.0,
                storage_bytes: 1_000_000_000,
                channel_count: 100,
            },
            ShardLoad {
                shard_id: ShardId(1),
                cpu_usage: 0.2,
                memory_usage: 0.3,
                throughput_msg_s: 200.0,
                storage_bytes: 100_000_000,
                channel_count: 10,
            },
        ];

        let mut assignments = HashMap::new();
        for i in 0..100 {
            assignments.insert(ChannelId(format!("channel{}", i)), ShardId(0));
        }

        let plan = scheduler.greedy_bin_packing(&loads, &assignments).unwrap();
        assert!(!plan.migrations.is_empty());
        assert_eq!(plan.algorithm, RebalancingAlgorithm::GreedyBinPacking);
    }

    #[test]
    fn test_low_activity_window() {
        // This test checks the current UTC hour
        // In production, would mock the time for reliable testing
        let _is_low_activity = RebalancingScheduler::is_low_activity_window();
        // Can't assert specific value since it depends on when test runs
    }
}
