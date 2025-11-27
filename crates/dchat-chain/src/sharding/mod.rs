//! Advanced Shard Rebalancing Modules
//!
//! Task 12: Shard Rebalancing (ARCHITECTURE-2.0.md)
//!
//! This module provides channel-scoped sharding for horizontal scalability:
//! - ShardId and ChannelId for identifying shards and channels
//! - ShardManager for coordinating shard operations
//! - Load monitoring and automatic rebalancing
//! - State migration with two-phase commit

pub mod integration;
pub mod load_monitoring;
pub mod rebalancing;
pub mod state_migration;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Shard identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardId(pub u32);

impl ShardId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for ShardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "shard-{}", self.0)
    }
}

/// Channel identifier (re-export from dchat-core if available, or define here)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub Uuid);

impl ChannelId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
    
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for ChannelId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "channel-{}", self.0)
    }
}

/// Shard configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardConfig {
    /// Number of shards in the system
    pub num_shards: u32,
    /// Maximum channels per shard before rebalancing
    pub max_channels_per_shard: usize,
    /// Maximum messages per shard before rebalancing
    pub max_messages_per_shard: u64,
    /// Rebalancing threshold (0.0-1.0)
    pub rebalance_threshold: f64,
    /// Enable automatic rebalancing
    pub auto_rebalance: bool,
}

impl Default for ShardConfig {
    fn default() -> Self {
        Self {
            num_shards: 16,
            max_channels_per_shard: 10000,
            max_messages_per_shard: 10_000_000,
            rebalance_threshold: 0.8,
            auto_rebalance: true,
        }
    }
}

/// Shard state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardState {
    pub id: ShardId,
    pub channels: Vec<ChannelId>,
    pub message_count: u64,
    pub last_updated: i64,
    pub is_migrating: bool,
}

/// Shard manager for coordinating shard operations
pub struct ShardManager {
    pub config: ShardConfig,
    shards: Arc<RwLock<HashMap<ShardId, ShardState>>>,
    channel_to_shard: Arc<RwLock<HashMap<ChannelId, ShardId>>>,
}

impl ShardManager {
    /// Create a new shard manager
    pub fn new(config: ShardConfig) -> Self {
        let mut shards = HashMap::new();
        
        // Initialize shards
        for i in 0..config.num_shards {
            let shard_id = ShardId(i);
            shards.insert(shard_id, ShardState {
                id: shard_id,
                channels: Vec::new(),
                message_count: 0,
                last_updated: chrono::Utc::now().timestamp(),
                is_migrating: false,
            });
        }
        
        Self {
            config,
            shards: Arc::new(RwLock::new(shards)),
            channel_to_shard: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Get shard for a channel using consistent hashing
    pub async fn get_shard_for_channel(&self, channel_id: &ChannelId) -> ShardId {
        // Check cache first
        let cache = self.channel_to_shard.read().await;
        if let Some(shard_id) = cache.get(channel_id) {
            return *shard_id;
        }
        drop(cache);
        
        // Use consistent hashing
        let hash = blake3::hash(channel_id.0.as_bytes());
        let hash_bytes = hash.as_bytes();
        let hash_value = u32::from_le_bytes([hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3]]);
        let shard_id = ShardId(hash_value % self.config.num_shards);
        
        // Update cache
        let mut cache = self.channel_to_shard.write().await;
        cache.insert(*channel_id, shard_id);
        
        shard_id
    }
    
    /// Assign channel to shard
    pub async fn assign_channel(&self, channel_id: ChannelId, shard_id: ShardId) {
        let mut shards = self.shards.write().await;
        if let Some(shard) = shards.get_mut(&shard_id) {
            if !shard.channels.contains(&channel_id) {
                shard.channels.push(channel_id);
                shard.last_updated = chrono::Utc::now().timestamp();
            }
        }
        
        let mut cache = self.channel_to_shard.write().await;
        cache.insert(channel_id, shard_id);
    }
    
    /// Get shard state
    pub async fn get_shard(&self, shard_id: &ShardId) -> Option<ShardState> {
        let shards = self.shards.read().await;
        shards.get(shard_id).cloned()
    }
    
    /// Increment message count for shard
    pub async fn increment_message_count(&self, shard_id: &ShardId, count: u64) {
        let mut shards = self.shards.write().await;
        if let Some(shard) = shards.get_mut(shard_id) {
            shard.message_count += count;
            shard.last_updated = chrono::Utc::now().timestamp();
        }
    }
    
    /// Get all shards
    pub async fn get_all_shards(&self) -> Vec<ShardState> {
        let shards = self.shards.read().await;
        shards.values().cloned().collect()
    }
    
    /// Check if rebalancing is needed
    pub async fn needs_rebalancing(&self) -> bool {
        let shards = self.shards.read().await;
        
        let total_messages: u64 = shards.values().map(|s| s.message_count).sum();
        let avg_messages = total_messages / (shards.len() as u64).max(1);
        
        for shard in shards.values() {
            let load_factor = shard.message_count as f64 / avg_messages.max(1) as f64;
            if load_factor > 1.0 + self.config.rebalance_threshold {
                return true;
            }
        }
        
        false
    }
    
    /// Get configuration
    pub fn config(&self) -> &ShardConfig {
        &self.config
    }
    
    /// Get channel count for a specific shard (synchronous for integration)
    pub fn get_channel_count(&self, shard_id: &ShardId) -> Option<usize> {
        // Use try_read for non-blocking access in sync context
        self.shards.try_read().ok().and_then(|shards| {
            shards.get(shard_id).map(|s| s.channels.len())
        })
    }
    
    /// Get all channel to shard assignments (synchronous for integration)
    pub fn get_all_channel_assignments(&self) -> HashMap<ChannelId, ShardId> {
        self.channel_to_shard.try_read()
            .map(|cache| cache.clone())
            .unwrap_or_default()
    }
}

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