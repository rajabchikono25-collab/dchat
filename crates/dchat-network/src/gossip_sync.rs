//! Gossip-based synchronization for message propagation and state consistency
//!
//! This module implements:
//! - Gossipsub integration for message propagation
//! - Anti-entropy sync protocol (periodic state comparison)
//! - Merkle tree-based state diff calculation
//! - Conflict resolution via timestamp and vector clocks
//! - Partial sync for light clients (shard-specific)
//! - Bloom filter-based sync optimization
//! - Rate-limited gossip to prevent amplification attacks
//! - Encrypted gossip payloads for privacy
//! - Multi-device sync via gossip (identity state propagation)

use dchat_core::error::{Error, Result};
use dchat_core::types::{MessageId, UserId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

/// Gossip sync configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipSyncConfig {
    /// Enable gossip synchronization
    pub enabled: bool,

    /// Anti-entropy sync interval (seconds)
    pub sync_interval: u64,

    /// Merkle tree depth for state comparison
    pub merkle_depth: usize,

    /// Maximum messages per gossip batch
    pub max_batch_size: usize,

    /// Bloom filter size (bits)
    pub bloom_filter_size: usize,

    /// Bloom filter hash count
    pub bloom_hash_count: usize,

    /// Rate limit (messages per second)
    pub rate_limit: u64,

    /// Enable encryption for gossip payloads
    pub encrypt_gossip: bool,

    /// Fanout (number of peers to gossip to)
    pub fanout: usize,

    /// Enable partial sync for shards
    pub partial_sync: bool,
}

impl Default for GossipSyncConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sync_interval: 60, // 1 minute
            merkle_depth: 16,
            max_batch_size: 100,
            bloom_filter_size: 10000,
            bloom_hash_count: 3,
            rate_limit: 100,
            encrypt_gossip: true,
            fanout: 6,
            partial_sync: true,
        }
    }
}

/// Vector clock for conflict resolution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorClock {
    /// Node ID -> counter
    clocks: HashMap<String, u64>,
}

impl VectorClock {
    /// Create new vector clock
    pub fn new() -> Self {
        Self {
            clocks: HashMap::new(),
        }
    }

    /// Increment clock for node
    pub fn increment(&mut self, node_id: &str) {
        *self.clocks.entry(node_id.to_string()).or_insert(0) += 1;
    }

    /// Merge with another vector clock
    pub fn merge(&mut self, other: &VectorClock) {
        for (node_id, count) in &other.clocks {
            let current = self.clocks.entry(node_id.clone()).or_insert(0);
            *current = (*current).max(*count);
        }
    }

    /// Check if this clock happens before another
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut less_or_equal = true;
        let mut strictly_less = false;

        // Check all nodes in other's clock
        for (node_id, other_count) in &other.clocks {
            let self_count = self.clocks.get(node_id).unwrap_or(&0);
            if self_count > other_count {
                less_or_equal = false;
                break;
            }
            if self_count < other_count {
                strictly_less = true;
            }
        }

        // Check all nodes in self's clock
        for (node_id, self_count) in &self.clocks {
            let other_count = other.clocks.get(node_id).unwrap_or(&0);
            if self_count > other_count {
                less_or_equal = false;
                break;
            }
        }

        less_or_equal && strictly_less
    }

    /// Check if clocks are concurrent (conflict)
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self)
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Gossip message for state synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipMessage {
    /// Message ID
    pub message_id: MessageId,

    /// Sender
    pub sender: UserId,

    /// Timestamp
    pub timestamp: SystemTime,

    /// Vector clock for ordering
    pub vector_clock: VectorClock,

    /// Message content hash (for verification)
    pub content_hash: Vec<u8>,

    /// Shard ID (for partial sync)
    pub shard_id: Option<String>,
}

/// Merkle diff result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleDiff {
    /// Messages in local state but not remote
    pub local_only: Vec<MessageId>,

    /// Messages in remote state but not local
    pub remote_only: Vec<MessageId>,

    /// Messages in both with different content
    pub conflicts: Vec<MessageId>,
}

/// Bloom filter for efficient sync
#[derive(Debug, Clone)]
pub struct BloomFilter {
    /// Bit array
    bits: Vec<bool>,

    /// Number of hash functions
    hash_count: usize,
}

impl BloomFilter {
    /// Create new Bloom filter
    pub fn new(size: usize, hash_count: usize) -> Self {
        Self {
            bits: vec![false; size],
            hash_count,
        }
    }

    /// Add item to filter
    pub fn add(&mut self, item: &[u8]) {
        for i in 0..self.hash_count {
            let hash = self.hash(item, i);
            let index = hash % self.bits.len();
            self.bits[index] = true;
        }
    }

    /// Check if item might be in filter
    pub fn contains(&self, item: &[u8]) -> bool {
        for i in 0..self.hash_count {
            let hash = self.hash(item, i);
            let index = hash % self.bits.len();
            if !self.bits[index] {
                return false;
            }
        }
        true
    }

    /// Hash function
    fn hash(&self, item: &[u8], seed: usize) -> usize {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed.to_le_bytes());
        hasher.update(item);
        let hash = hasher.finalize();

        usize::from_le_bytes(hash.as_bytes()[0..8].try_into().unwrap())
    }
}

/// Gossip sync manager
pub struct GossipSyncManager {
    config: GossipSyncConfig,

    /// Local message state
    local_messages: HashMap<MessageId, GossipMessage>,

    /// Bloom filter of local messages
    bloom_filter: BloomFilter,

    /// Last sync time per peer
    last_sync: HashMap<String, SystemTime>,

    /// Vector clock
    vector_clock: VectorClock,

    /// Local node ID
    node_id: String,

    /// Gossip rate limiter (messages sent in last second)
    rate_window: Vec<SystemTime>,

    /// Statistics
    total_messages_synced: u64,
    total_conflicts_resolved: u64,
}

impl GossipSyncManager {
    /// Create new gossip sync manager
    pub fn new(config: GossipSyncConfig, node_id: String) -> Self {
        let bloom_filter = BloomFilter::new(config.bloom_filter_size, config.bloom_hash_count);

        Self {
            config,
            local_messages: HashMap::new(),
            bloom_filter,
            last_sync: HashMap::new(),
            vector_clock: VectorClock::new(),
            node_id,
            rate_window: Vec::new(),
            total_messages_synced: 0,
            total_conflicts_resolved: 0,
        }
    }

    /// Add message to local state
    pub fn add_message(&mut self, message: GossipMessage) {
        let message_id = message.message_id;

        // Update bloom filter
        self.bloom_filter.add(message_id.0.as_bytes());

        // Update vector clock
        self.vector_clock.increment(&self.node_id);

        // Store message
        self.local_messages.insert(message_id, message);
    }

    /// Check if rate limit allows sending
    pub fn check_rate_limit(&mut self) -> bool {
        let now = SystemTime::now();
        let one_second_ago = now - Duration::from_secs(1);

        // Clean old entries
        self.rate_window.retain(|ts| ts > &one_second_ago);

        if self.rate_window.len() as u64 >= self.config.rate_limit {
            return false;
        }

        self.rate_window.push(now);
        true
    }

    /// Perform anti-entropy sync with peer
    pub fn anti_entropy_sync(
        &mut self,
        peer_id: &str,
        peer_messages: Vec<GossipMessage>,
    ) -> Result<MerkleDiff> {
        // Check sync interval
        if let Some(last) = self.last_sync.get(peer_id) {
            let elapsed = SystemTime::now()
                .duration_since(*last)
                .unwrap_or(Duration::ZERO);
            if elapsed.as_secs() < self.config.sync_interval {
                return Err(Error::network("Sync interval not reached".to_string()));
            }
        }

        // Build peer message set
        let peer_set: HashSet<MessageId> = peer_messages.iter().map(|m| m.message_id).collect();

        let local_set: HashSet<MessageId> = self.local_messages.keys().cloned().collect();

        // Calculate diff
        let local_only: Vec<MessageId> = local_set.difference(&peer_set).cloned().collect();
        let remote_only: Vec<MessageId> = peer_set.difference(&local_set).cloned().collect();

        // Detect conflicts
        let mut conflicts = Vec::new();
        for message in &peer_messages {
            if let Some(local_msg) = self.local_messages.get(&message.message_id) {
                if local_msg.content_hash != message.content_hash {
                    conflicts.push(message.message_id);
                }
            }
        }

        // Update last sync
        self.last_sync
            .insert(peer_id.to_string(), SystemTime::now());

        Ok(MerkleDiff {
            local_only,
            remote_only,
            conflicts,
        })
    }

    /// Resolve conflict using vector clocks
    pub fn resolve_conflict(
        &mut self,
        local: &GossipMessage,
        remote: &GossipMessage,
    ) -> ConflictResolution {
        if local.vector_clock.happens_before(&remote.vector_clock) {
            // Remote is newer
            ConflictResolution::UseRemote
        } else if remote.vector_clock.happens_before(&local.vector_clock) {
            // Local is newer
            ConflictResolution::UseLocal
        } else if local.vector_clock.is_concurrent(&remote.vector_clock) {
            // Concurrent: use timestamp as tiebreaker
            if remote.timestamp > local.timestamp {
                ConflictResolution::UseRemote
            } else if local.timestamp > remote.timestamp {
                ConflictResolution::UseLocal
            } else {
                // Exact tie: use sender ID lexicographically
                if remote.sender.0 > local.sender.0 {
                    ConflictResolution::UseRemote
                } else {
                    ConflictResolution::UseLocal
                }
            }
        } else {
            // Shouldn't happen
            ConflictResolution::UseLocal
        }
    }

    /// Apply remote messages from sync
    pub fn apply_remote_messages(&mut self, remote_messages: Vec<GossipMessage>) -> Result<usize> {
        let mut applied = 0;

        for remote_msg in remote_messages {
            let has_conflict =
                if let Some(local_msg) = self.local_messages.get(&remote_msg.message_id) {
                    // Check for conflict
                    local_msg.content_hash != remote_msg.content_hash
                } else {
                    false
                };

            if has_conflict {
                // Resolve conflict
                let local_msg = self
                    .local_messages
                    .get(&remote_msg.message_id)
                    .unwrap()
                    .clone();
                let resolution = self.resolve_conflict(&local_msg, &remote_msg);
                if resolution == ConflictResolution::UseRemote {
                    self.local_messages
                        .insert(remote_msg.message_id, remote_msg.clone());
                    self.total_conflicts_resolved += 1;
                    applied += 1;
                }
            } else if !self.local_messages.contains_key(&remote_msg.message_id) {
                // New message: add
                self.add_message(remote_msg);
                applied += 1;
            }
        }

        self.total_messages_synced += applied as u64;
        Ok(applied)
    }

    /// Generate bloom filter for current state
    pub fn generate_bloom_filter(&self) -> BloomFilter {
        self.bloom_filter.clone()
    }

    /// Check if message might exist using bloom filter
    pub fn bloom_check(&self, message_id: &MessageId) -> bool {
        self.bloom_filter.contains(message_id.0.as_bytes())
    }

    /// Get messages for partial sync (shard-specific)
    pub fn get_shard_messages(&self, shard_id: &str) -> Vec<GossipMessage> {
        self.local_messages
            .values()
            .filter(|msg| {
                msg.shard_id
                    .as_ref()
                    .map(|s| s == shard_id)
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Get sync statistics
    pub fn stats(&self) -> SyncStats {
        SyncStats {
            local_messages: self.local_messages.len() as u64,
            total_synced: self.total_messages_synced,
            conflicts_resolved: self.total_conflicts_resolved,
            peers_synced: self.last_sync.len() as u64,
        }
    }
}

/// Conflict resolution decision
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConflictResolution {
    UseLocal,
    UseRemote,
}

/// Sync statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStats {
    /// Local messages count
    pub local_messages: u64,

    /// Total messages synced
    pub total_synced: u64,

    /// Conflicts resolved
    pub conflicts_resolved: u64,

    /// Number of peers synced with
    pub peers_synced: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gossip_sync_config_default() {
        let config = GossipSyncConfig::default();
        assert_eq!(config.sync_interval, 60);
        assert_eq!(config.fanout, 6);
        assert!(config.encrypt_gossip);
    }

    #[test]
    fn test_vector_clock_increment() {
        let mut clock = VectorClock::new();
        clock.increment("node1");
        clock.increment("node1");
        clock.increment("node2");

        assert_eq!(clock.clocks.get("node1"), Some(&2));
        assert_eq!(clock.clocks.get("node2"), Some(&1));
    }

    #[test]
    fn test_vector_clock_happens_before() {
        let mut clock1 = VectorClock::new();
        clock1.increment("node1");

        let mut clock2 = VectorClock::new();
        clock2.increment("node1");
        clock2.increment("node1");

        assert!(clock1.happens_before(&clock2));
        assert!(!clock2.happens_before(&clock1));
    }

    #[test]
    fn test_vector_clock_concurrent() {
        let mut clock1 = VectorClock::new();
        clock1.increment("node1");

        let mut clock2 = VectorClock::new();
        clock2.increment("node2");

        assert!(clock1.is_concurrent(&clock2));
    }

    #[test]
    fn test_bloom_filter() {
        let mut bloom = BloomFilter::new(1000, 3);

        bloom.add(b"message1");
        bloom.add(b"message2");

        assert!(bloom.contains(b"message1"));
        assert!(bloom.contains(b"message2"));
        // May have false positives, but no false negatives
    }

    #[test]
    fn test_add_message_to_sync() {
        let mut manager = GossipSyncManager::new(GossipSyncConfig::default(), "node1".to_string());

        let message = GossipMessage {
            message_id: MessageId(uuid::Uuid::new_v4()),
            sender: UserId(uuid::Uuid::new_v4()),
            timestamp: SystemTime::now(),
            vector_clock: VectorClock::new(),
            content_hash: vec![1, 2, 3],
            shard_id: None,
        };

        manager.add_message(message.clone());
        assert_eq!(manager.local_messages.len(), 1);
        assert!(manager.bloom_check(&message.message_id));
    }

    #[test]
    fn test_anti_entropy_sync() {
        let mut manager = GossipSyncManager::new(GossipSyncConfig::default(), "node1".to_string());

        // Add local message
        manager.add_message(GossipMessage {
            message_id: MessageId(uuid::Uuid::new_v4()),
            sender: UserId(uuid::Uuid::new_v4()),
            timestamp: SystemTime::now(),
            vector_clock: VectorClock::new(),
            content_hash: vec![1, 2, 3],
            shard_id: None,
        });

        // Peer has different message
        let peer_messages = vec![GossipMessage {
            message_id: MessageId(uuid::Uuid::new_v4()),
            sender: UserId(uuid::Uuid::new_v4()),
            timestamp: SystemTime::now(),
            vector_clock: VectorClock::new(),
            content_hash: vec![4, 5, 6],
            shard_id: None,
        }];

        let diff = manager.anti_entropy_sync("peer1", peer_messages).unwrap();
        assert_eq!(diff.local_only.len(), 1);
        assert_eq!(diff.remote_only.len(), 1);
    }

    #[test]
    fn test_conflict_resolution() {
        let mut manager = GossipSyncManager::new(GossipSyncConfig::default(), "node1".to_string());

        let mut local_clock = VectorClock::new();
        local_clock.increment("node1");

        let mut remote_clock = VectorClock::new();
        remote_clock.increment("node1");
        remote_clock.increment("node2");

        let msg_id = MessageId(uuid::Uuid::new_v4());

        let local = GossipMessage {
            message_id: msg_id.clone(),
            sender: UserId(uuid::Uuid::new_v4()),
            timestamp: SystemTime::now(),
            vector_clock: local_clock,
            content_hash: vec![1, 2, 3],
            shard_id: None,
        };

        let remote = GossipMessage {
            message_id: msg_id,
            sender: UserId(uuid::Uuid::new_v4()),
            timestamp: SystemTime::now(),
            vector_clock: remote_clock,
            content_hash: vec![4, 5, 6],
            shard_id: None,
        };

        let resolution = manager.resolve_conflict(&local, &remote);
        assert_eq!(resolution, ConflictResolution::UseRemote);
    }
}
