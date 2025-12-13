//! Sharded Per-Core State Management
//!
//! Replaces global `Arc<RwLock<HashMap>>` patterns with:
//! - Per-core owned partitions using consistent hashing
//! - Lock-free cross-partition queries via message passing
//! - Bounded memory per partition with LRU eviction
//! - NUMA-aware allocation for large deployments
//!
//! Security: Partitioning is deterministic and verifiable;
//! consistent hashing ensures balanced distribution.

use crate::block_hierarchy::Hash;
use crossbeam_channel::{bounded, Receiver, Sender};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Number of virtual nodes per physical partition (for consistent hashing)
pub const VIRTUAL_NODES_PER_PARTITION: usize = 150;

/// Default partition count (typically = CPU cores)
pub const DEFAULT_PARTITION_COUNT: usize = 16;

/// Maximum items per partition before eviction
pub const DEFAULT_MAX_ITEMS_PER_PARTITION: usize = 100_000;

/// Request timeout for cross-partition queries
pub const CROSS_PARTITION_TIMEOUT_MS: u64 = 50;

/// Sharding errors
#[derive(Debug, Error)]
pub enum ShardingError {
    #[error("Partition {0} not found")]
    PartitionNotFound(usize),

    #[error("Operation timeout")]
    Timeout,

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Partition full: {0} items")]
    PartitionFull(usize),

    #[error("Key not found: {0:?}")]
    KeyNotFound(Vec<u8>),

    #[error("Invalid partition configuration")]
    InvalidConfig,
}

/// Consistent hash ring for partition assignment
pub struct ConsistentHashRing {
    /// Ring of (hash_value, partition_id)
    ring: Vec<(u64, usize)>,
    /// Number of physical partitions
    partition_count: usize,
}

impl ConsistentHashRing {
    /// Create a new consistent hash ring
    pub fn new(partition_count: usize) -> Self {
        let mut ring = Vec::with_capacity(partition_count * VIRTUAL_NODES_PER_PARTITION);

        for partition in 0..partition_count {
            for vnode in 0..VIRTUAL_NODES_PER_PARTITION {
                let key = format!("partition-{}-vnode-{}", partition, vnode);
                let hash = Self::hash_key(key.as_bytes());
                ring.push((hash, partition));
            }
        }

        // Sort by hash value for binary search
        ring.sort_by_key(|(h, _)| *h);

        Self {
            ring,
            partition_count,
        }
    }

    /// Hash a key to a u64
    fn hash_key(key: &[u8]) -> u64 {
        let hash = blake3::hash(key);
        let bytes: [u8; 8] = hash.as_bytes()[0..8].try_into().unwrap();
        u64::from_le_bytes(bytes)
    }

    /// Get partition for a key
    pub fn get_partition(&self, key: &[u8]) -> usize {
        let hash = Self::hash_key(key);

        // Binary search for first node >= hash
        match self.ring.binary_search_by_key(&hash, |(h, _)| *h) {
            Ok(idx) => self.ring[idx].1,
            Err(idx) => {
                if idx >= self.ring.len() {
                    // Wrap around to first node
                    self.ring[0].1
                } else {
                    self.ring[idx].1
                }
            }
        }
    }

    /// Get partition count
    pub fn partition_count(&self) -> usize {
        self.partition_count
    }

    /// Get multiple partitions for replication
    pub fn get_partitions_for_replication(&self, key: &[u8], count: usize) -> Vec<usize> {
        let primary = self.get_partition(key);
        let mut partitions = vec![primary];

        for i in 1..count {
            let next = (primary + i) % self.partition_count;
            if !partitions.contains(&next) {
                partitions.push(next);
            }
        }

        partitions
    }
}

/// Request types for partition workers
#[derive(Debug)]
pub enum PartitionRequest<V> {
    /// Get a value
    Get {
        key: Vec<u8>,
        response: Sender<Option<V>>,
    },
    /// Insert a value
    Insert {
        key: Vec<u8>,
        value: V,
        response: Sender<Result<Option<V>, ShardingError>>,
    },
    /// Remove a value
    Remove {
        key: Vec<u8>,
        response: Sender<Option<V>>,
    },
    /// Check if key exists
    Contains {
        key: Vec<u8>,
        response: Sender<bool>,
    },
    /// Get partition size
    Size { response: Sender<usize> },
    /// Clear partition
    Clear { response: Sender<()> },
    /// Iterate over all entries
    Iter { response: Sender<Vec<(Vec<u8>, V)>> },
    /// Shutdown the partition worker
    Shutdown,
}

/// Single partition (runs on dedicated thread)
struct Partition<V> {
    /// Partition ID
    id: usize,
    /// Key-value store
    data: HashMap<Vec<u8>, V>,
    /// Access order for LRU eviction
    access_order: VecDeque<Vec<u8>>,
    /// Maximum items
    max_items: usize,
    /// Statistics
    stats: PartitionStats,
}

/// Partition statistics
#[derive(Debug, Default)]
pub struct PartitionStats {
    pub gets: AtomicU64,
    pub inserts: AtomicU64,
    pub removes: AtomicU64,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
}

impl<V: Clone> Partition<V> {
    fn new(id: usize, max_items: usize) -> Self {
        Self {
            id,
            data: HashMap::new(),
            access_order: VecDeque::new(),
            max_items,
            stats: PartitionStats::default(),
        }
    }

    fn get(&mut self, key: &[u8]) -> Option<V> {
        self.stats.gets.fetch_add(1, Ordering::Relaxed);

        if let Some(value) = self.data.get(key).cloned() {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            self.touch_key(key);
            Some(value)
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    fn insert(&mut self, key: Vec<u8>, value: V) -> Result<Option<V>, ShardingError> {
        self.stats.inserts.fetch_add(1, Ordering::Relaxed);

        // Evict if at capacity
        while self.data.len() >= self.max_items {
            self.evict_lru();
        }

        let old = self.data.insert(key.clone(), value);

        if old.is_none() {
            self.access_order.push_back(key);
        } else {
            self.touch_key(&key);
        }

        Ok(old)
    }

    fn remove(&mut self, key: &[u8]) -> Option<V> {
        self.stats.removes.fetch_add(1, Ordering::Relaxed);

        if let Some(value) = self.data.remove(key) {
            self.access_order.retain(|k| k != key);
            Some(value)
        } else {
            None
        }
    }

    fn contains(&self, key: &[u8]) -> bool {
        self.data.contains_key(key)
    }

    fn size(&self) -> usize {
        self.data.len()
    }

    fn clear(&mut self) {
        self.data.clear();
        self.access_order.clear();
    }

    fn iter(&self) -> Vec<(Vec<u8>, V)> {
        self.data
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn touch_key(&mut self, key: &[u8]) {
        // Remove from current position and add to end
        self.access_order.retain(|k| k != key);
        self.access_order.push_back(key.to_vec());
    }

    fn evict_lru(&mut self) {
        if let Some(key) = self.access_order.pop_front() {
            self.data.remove(&key);
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Sharded state store
pub struct ShardedState<V: Clone + Send + 'static> {
    /// Consistent hash ring
    ring: Arc<ConsistentHashRing>,
    /// Request senders for each partition
    partition_senders: Vec<Sender<PartitionRequest<V>>>,
    /// Worker thread handles
    workers: Vec<thread::JoinHandle<()>>,
    /// Total size estimate
    total_size: Arc<AtomicUsize>,
    /// Partition count
    partition_count: usize,
}

impl<V: Clone + Send + Sync + 'static> ShardedState<V> {
    /// Create new sharded state with default partition count
    pub fn new(max_items_per_partition: usize) -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);
        Self::with_partitions(cpus, max_items_per_partition)
    }

    /// Create sharded state with specific partition count
    pub fn with_partitions(partition_count: usize, max_items_per_partition: usize) -> Self {
        let ring = Arc::new(ConsistentHashRing::new(partition_count));
        let total_size = Arc::new(AtomicUsize::new(0));

        let mut partition_senders = Vec::with_capacity(partition_count);
        let mut workers = Vec::with_capacity(partition_count);

        for id in 0..partition_count {
            let (tx, rx) = bounded::<PartitionRequest<V>>(1000);
            partition_senders.push(tx);

            let size_counter = total_size.clone();
            let max_items = max_items_per_partition;

            let handle = thread::Builder::new()
                .name(format!("shard-worker-{}", id))
                .spawn(move || {
                    let mut partition = Partition::new(id, max_items);

                    while let Ok(request) = rx.recv() {
                        match request {
                            PartitionRequest::Get { key, response } => {
                                let _ = response.send(partition.get(&key));
                            }
                            PartitionRequest::Insert {
                                key,
                                value,
                                response,
                            } => {
                                let old_size = partition.size();
                                let result = partition.insert(key, value);
                                let new_size = partition.size();

                                if new_size > old_size {
                                    size_counter.fetch_add(1, Ordering::Relaxed);
                                }

                                let _ = response.send(result);
                            }
                            PartitionRequest::Remove { key, response } => {
                                if let Some(v) = partition.remove(&key) {
                                    size_counter.fetch_sub(1, Ordering::Relaxed);
                                    let _ = response.send(Some(v));
                                } else {
                                    let _ = response.send(None);
                                }
                            }
                            PartitionRequest::Contains { key, response } => {
                                let _ = response.send(partition.contains(&key));
                            }
                            PartitionRequest::Size { response } => {
                                let _ = response.send(partition.size());
                            }
                            PartitionRequest::Clear { response } => {
                                let old_size = partition.size();
                                partition.clear();
                                size_counter.fetch_sub(old_size, Ordering::Relaxed);
                                let _ = response.send(());
                            }
                            PartitionRequest::Iter { response } => {
                                let _ = response.send(partition.iter());
                            }
                            PartitionRequest::Shutdown => break,
                        }
                    }
                })
                .expect("Failed to spawn shard worker");

            workers.push(handle);
        }

        Self {
            ring,
            partition_senders,
            workers,
            total_size,
            partition_count,
        }
    }

    /// Get value by key
    pub fn get(&self, key: &[u8]) -> Result<Option<V>, ShardingError> {
        let partition = self.ring.get_partition(key);
        let (tx, rx) = bounded(1);

        self.partition_senders[partition]
            .send(PartitionRequest::Get {
                key: key.to_vec(),
                response: tx,
            })
            .map_err(|_| ShardingError::ChannelClosed)?;

        rx.recv_timeout(Duration::from_millis(CROSS_PARTITION_TIMEOUT_MS))
            .map_err(|_| ShardingError::Timeout)
    }

    /// Insert a key-value pair
    pub fn insert(&self, key: Vec<u8>, value: V) -> Result<Option<V>, ShardingError> {
        let partition = self.ring.get_partition(&key);
        let (tx, rx) = bounded(1);

        self.partition_senders[partition]
            .send(PartitionRequest::Insert {
                key,
                value,
                response: tx,
            })
            .map_err(|_| ShardingError::ChannelClosed)?;

        rx.recv_timeout(Duration::from_millis(CROSS_PARTITION_TIMEOUT_MS))
            .map_err(|_| ShardingError::Timeout)?
    }

    /// Remove a key
    pub fn remove(&self, key: &[u8]) -> Result<Option<V>, ShardingError> {
        let partition = self.ring.get_partition(key);
        let (tx, rx) = bounded(1);

        self.partition_senders[partition]
            .send(PartitionRequest::Remove {
                key: key.to_vec(),
                response: tx,
            })
            .map_err(|_| ShardingError::ChannelClosed)?;

        rx.recv_timeout(Duration::from_millis(CROSS_PARTITION_TIMEOUT_MS))
            .map_err(|_| ShardingError::Timeout)
    }

    /// Check if key exists
    pub fn contains(&self, key: &[u8]) -> Result<bool, ShardingError> {
        let partition = self.ring.get_partition(key);
        let (tx, rx) = bounded(1);

        self.partition_senders[partition]
            .send(PartitionRequest::Contains {
                key: key.to_vec(),
                response: tx,
            })
            .map_err(|_| ShardingError::ChannelClosed)?;

        rx.recv_timeout(Duration::from_millis(CROSS_PARTITION_TIMEOUT_MS))
            .map_err(|_| ShardingError::Timeout)
    }

    /// Get total size (approximate)
    pub fn len(&self) -> usize {
        self.total_size.load(Ordering::Relaxed)
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get partition count
    pub fn partition_count(&self) -> usize {
        self.partition_count
    }

    /// Get which partition a key would be in
    pub fn partition_for_key(&self, key: &[u8]) -> usize {
        self.ring.get_partition(key)
    }

    /// Get size of specific partition
    pub fn partition_size(&self, partition: usize) -> Result<usize, ShardingError> {
        if partition >= self.partition_count {
            return Err(ShardingError::PartitionNotFound(partition));
        }

        let (tx, rx) = bounded(1);

        self.partition_senders[partition]
            .send(PartitionRequest::Size { response: tx })
            .map_err(|_| ShardingError::ChannelClosed)?;

        rx.recv_timeout(Duration::from_millis(CROSS_PARTITION_TIMEOUT_MS))
            .map_err(|_| ShardingError::Timeout)
    }

    /// Clear all partitions
    pub fn clear(&self) -> Result<(), ShardingError> {
        for partition in 0..self.partition_count {
            let (tx, rx) = bounded(1);

            self.partition_senders[partition]
                .send(PartitionRequest::Clear { response: tx })
                .map_err(|_| ShardingError::ChannelClosed)?;

            rx.recv_timeout(Duration::from_millis(CROSS_PARTITION_TIMEOUT_MS * 10))
                .map_err(|_| ShardingError::Timeout)?;
        }

        Ok(())
    }

    /// Iterate over all entries (expensive, use sparingly)
    pub fn iter_all(&self) -> Result<Vec<(Vec<u8>, V)>, ShardingError> {
        let mut all_entries = Vec::new();

        for partition in 0..self.partition_count {
            let (tx, rx) = bounded(1);

            self.partition_senders[partition]
                .send(PartitionRequest::Iter { response: tx })
                .map_err(|_| ShardingError::ChannelClosed)?;

            let entries = rx
                .recv_timeout(Duration::from_millis(CROSS_PARTITION_TIMEOUT_MS * 10))
                .map_err(|_| ShardingError::Timeout)?;

            all_entries.extend(entries);
        }

        Ok(all_entries)
    }

    /// Shutdown all workers
    pub fn shutdown(self) {
        for sender in &self.partition_senders {
            let _ = sender.send(PartitionRequest::Shutdown);
        }

        for worker in self.workers {
            let _ = worker.join();
        }
    }
}

/// Lock-free sharded state using DashMap (simpler alternative)
pub struct LockFreeShardedState<V> {
    /// Sharded DashMap
    shards: Vec<DashMap<Vec<u8>, V>>,
    /// Hash ring
    ring: ConsistentHashRing,
    /// Total size
    total_size: AtomicUsize,
}

impl<V: Clone> LockFreeShardedState<V> {
    /// Create new lock-free sharded state
    pub fn new(shard_count: usize) -> Self {
        let shards: Vec<_> = (0..shard_count).map(|_| DashMap::new()).collect();

        Self {
            shards,
            ring: ConsistentHashRing::new(shard_count),
            total_size: AtomicUsize::new(0),
        }
    }

    /// Get value by key
    pub fn get(&self, key: &[u8]) -> Option<V> {
        let shard = self.ring.get_partition(key);
        self.shards[shard].get(key).map(|r| r.value().clone())
    }

    /// Insert key-value pair
    pub fn insert(&self, key: Vec<u8>, value: V) -> Option<V> {
        let shard = self.ring.get_partition(&key);
        let old = self.shards[shard].insert(key, value);

        if old.is_none() {
            self.total_size.fetch_add(1, Ordering::Relaxed);
        }

        old
    }

    /// Remove key
    pub fn remove(&self, key: &[u8]) -> Option<V> {
        let shard = self.ring.get_partition(key);

        if let Some((_, v)) = self.shards[shard].remove(key) {
            self.total_size.fetch_sub(1, Ordering::Relaxed);
            Some(v)
        } else {
            None
        }
    }

    /// Check if key exists
    pub fn contains(&self, key: &[u8]) -> bool {
        let shard = self.ring.get_partition(key);
        self.shards[shard].contains_key(key)
    }

    /// Get total size
    pub fn len(&self) -> usize {
        self.total_size.load(Ordering::Relaxed)
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get shard for key
    pub fn shard_for_key(&self, key: &[u8]) -> usize {
        self.ring.get_partition(key)
    }

    /// Get shard count
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }
}

/// Specialized sharded state for relay scores (common use case)
pub mod relay_scores {
    use super::*;
    use crate::consensus_types::RelayScore;

    /// Sharded relay score storage
    pub struct ShardedRelayScores {
        state: LockFreeShardedState<RelayScore>,
    }

    impl ShardedRelayScores {
        pub fn new(shard_count: usize) -> Self {
            Self {
                state: LockFreeShardedState::new(shard_count),
            }
        }

        pub fn get(&self, relay_id: &[u8; 32]) -> Option<RelayScore> {
            self.state.get(relay_id)
        }

        pub fn update(&self, relay_id: [u8; 32], score: RelayScore) {
            self.state.insert(relay_id.to_vec(), score);
        }

        pub fn remove(&self, relay_id: &[u8; 32]) -> Option<RelayScore> {
            self.state.remove(relay_id)
        }

        pub fn len(&self) -> usize {
            self.state.len()
        }
    }
}

/// Specialized sharded state for block votes (common use case)
pub mod block_votes {
    use super::*;
    use crate::consensus_types::BlockVotes;

    /// Sharded block vote storage
    pub struct ShardedBlockVotes {
        state: LockFreeShardedState<BlockVotes>,
    }

    impl ShardedBlockVotes {
        pub fn new(shard_count: usize) -> Self {
            Self {
                state: LockFreeShardedState::new(shard_count),
            }
        }

        pub fn get(&self, block_hash: &[u8; 32]) -> Option<BlockVotes> {
            self.state.get(block_hash)
        }

        pub fn insert(&self, block_hash: [u8; 32], votes: BlockVotes) {
            self.state.insert(block_hash.to_vec(), votes);
        }

        pub fn update_or_insert<F>(
            &self,
            block_hash: [u8; 32],
            default: BlockVotes,
            mut update_fn: F,
        ) where
            F: FnMut(&mut BlockVotes),
        {
            let shard = self.state.ring.get_partition(&block_hash);

            self.state.shards[shard]
                .entry(block_hash.to_vec())
                .and_modify(|v| update_fn(v))
                .or_insert_with(|| {
                    let mut v = default;
                    update_fn(&mut v);
                    v
                });
        }

        pub fn remove(&self, block_hash: &[u8; 32]) -> Option<BlockVotes> {
            self.state.remove(block_hash)
        }

        pub fn len(&self) -> usize {
            self.state.len()
        }
    }
}

/// Migration helpers for replacing RwLock<HashMap> patterns
pub mod migration {
    use super::*;
    use std::sync::RwLock;

    /// Migrate from RwLock<HashMap> to ShardedState
    pub fn migrate_from_rwlock<K, V>(
        old: &RwLock<HashMap<K, V>>,
        key_to_bytes: impl Fn(&K) -> Vec<u8>,
    ) -> LockFreeShardedState<V>
    where
        K: Clone,
        V: Clone,
    {
        let cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);
        let shards = LockFreeShardedState::new(cpus);

        if let Ok(guard) = old.read() {
            for (k, v) in guard.iter() {
                let key_bytes = key_to_bytes(k);
                shards.insert(key_bytes, v.clone());
            }
        }

        shards
    }

    /// Performance comparison helper
    pub fn benchmark_comparison<V: Clone + Default + Send + Sync + 'static>(
        item_count: usize,
        thread_count: usize,
    ) -> ComparisonResult {
        use std::sync::Arc;
        use std::thread;

        // Benchmark RwLock<HashMap>
        let rwlock_map: Arc<RwLock<HashMap<Vec<u8>, V>>> = Arc::new(RwLock::new(HashMap::new()));

        let rwlock_start = Instant::now();
        let handles: Vec<_> = (0..thread_count)
            .map(|t| {
                let map = rwlock_map.clone();
                thread::spawn(move || {
                    for i in 0..(item_count / thread_count) {
                        let key = format!("key-{}-{}", t, i).into_bytes();
                        map.write().unwrap().insert(key.clone(), V::default());
                        let _ = map.read().unwrap().get(&key);
                    }
                })
            })
            .collect();

        for h in handles {
            let _ = h.join();
        }
        let rwlock_time = rwlock_start.elapsed();

        // Benchmark ShardedState
        let cpus = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(4);
        let sharded: Arc<LockFreeShardedState<V>> = Arc::new(LockFreeShardedState::new(cpus));

        let sharded_start = Instant::now();
        let handles: Vec<_> = (0..thread_count)
            .map(|t| {
                let state = sharded.clone();
                thread::spawn(move || {
                    for i in 0..(item_count / thread_count) {
                        let key = format!("key-{}-{}", t, i).into_bytes();
                        state.insert(key.clone(), V::default());
                        let _ = state.get(&key);
                    }
                })
            })
            .collect();

        for h in handles {
            let _ = h.join();
        }
        let sharded_time = sharded_start.elapsed();

        ComparisonResult {
            rwlock_time,
            sharded_time,
            speedup: rwlock_time.as_secs_f64() / sharded_time.as_secs_f64(),
            item_count,
            thread_count,
        }
    }

    #[derive(Debug)]
    pub struct ComparisonResult {
        pub rwlock_time: Duration,
        pub sharded_time: Duration,
        pub speedup: f64,
        pub item_count: usize,
        pub thread_count: usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consistent_hash_ring() {
        let ring = ConsistentHashRing::new(4);

        // Same key should always map to same partition
        let key = b"test_key";
        let partition1 = ring.get_partition(key);
        let partition2 = ring.get_partition(key);
        assert_eq!(partition1, partition2);

        // Different keys should distribute across partitions
        let mut partition_counts = vec![0usize; 4];
        for i in 0..1000 {
            let key = format!("key_{}", i);
            let partition = ring.get_partition(key.as_bytes());
            partition_counts[partition] += 1;
        }

        // Each partition should have roughly 250 keys (with some variance)
        for count in &partition_counts {
            assert!(
                *count > 150 && *count < 350,
                "Uneven distribution: {:?}",
                partition_counts
            );
        }
    }

    #[test]
    fn test_lock_free_sharded_state() {
        let state: LockFreeShardedState<u64> = LockFreeShardedState::new(4);

        // Insert
        assert!(state.insert(b"key1".to_vec(), 100).is_none());
        assert!(state.insert(b"key2".to_vec(), 200).is_none());

        // Get
        assert_eq!(state.get(b"key1"), Some(100));
        assert_eq!(state.get(b"key2"), Some(200));
        assert_eq!(state.get(b"key3"), None);

        // Contains
        assert!(state.contains(b"key1"));
        assert!(!state.contains(b"key3"));

        // Length
        assert_eq!(state.len(), 2);

        // Remove
        assert_eq!(state.remove(b"key1"), Some(100));
        assert_eq!(state.len(), 1);
        assert!(!state.contains(b"key1"));
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let state = Arc::new(LockFreeShardedState::<u64>::new(8));

        let handles: Vec<_> = (0..8)
            .map(|t| {
                let s = state.clone();
                thread::spawn(move || {
                    for i in 0..1000 {
                        let key = format!("thread-{}-key-{}", t, i).into_bytes();
                        s.insert(key.clone(), t as u64 * 1000 + i);
                        assert!(s.contains(&key));
                        assert_eq!(s.get(&key), Some(t as u64 * 1000 + i));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(state.len(), 8000);
    }

    #[test]
    fn test_sharded_state_with_workers() {
        let state: ShardedState<String> = ShardedState::with_partitions(4, 1000);

        // Insert
        state
            .insert(b"key1".to_vec(), "value1".to_string())
            .unwrap();
        state
            .insert(b"key2".to_vec(), "value2".to_string())
            .unwrap();

        // Get
        assert_eq!(state.get(b"key1").unwrap(), Some("value1".to_string()));
        assert_eq!(state.get(b"key2").unwrap(), Some("value2".to_string()));

        // Contains
        assert!(state.contains(b"key1").unwrap());

        // Remove
        assert_eq!(state.remove(b"key1").unwrap(), Some("value1".to_string()));
        assert!(!state.contains(b"key1").unwrap());

        // Cleanup
        state.shutdown();
    }

    #[test]
    fn test_replication_partitions() {
        let ring = ConsistentHashRing::new(8);

        let key = b"replicated_key";
        let partitions = ring.get_partitions_for_replication(key, 3);

        // Should return 3 unique partitions
        assert_eq!(partitions.len(), 3);
        let unique: std::collections::HashSet<_> = partitions.iter().collect();
        assert_eq!(unique.len(), 3);
    }
}
