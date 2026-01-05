// Message deduplication cache using bloom filter

use dchat_core::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Message identifier (BLAKE3 hash)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId([u8; 32]);

impl MessageId {
    /// Create a message ID from payload
    pub fn from_payload(payload: &[u8]) -> Self {
        use blake3::Hasher;
        let hash = Hasher::new().update(payload).finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(hash.as_bytes());
        Self(id)
    }

    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

/// Message cache entry
#[derive(Debug, Clone)]
struct CacheEntry {
    id: MessageId,
    seen_at: Instant,
}

/// Message cache for deduplication
pub struct MessageCache {
    /// Bloom filter for fast probabilistic membership test
    bloom: BloomFilter,

    /// LRU cache for exact membership (backup for bloom false positives)
    recent_ids: VecDeque<CacheEntry>,

    /// Map for O(1) lookup
    id_map: HashMap<MessageId, Instant>,

    /// Maximum cache size
    max_size: usize,

    /// Time-to-live for cached entries
    ttl: Duration,
}

impl MessageCache {
    /// Create a new message cache
    pub fn new(max_size: usize, ttl: Duration) -> Result<Self> {
        let bloom = BloomFilter::new(max_size, 0.01)?; // 1% false positive rate

        Ok(Self {
            bloom,
            recent_ids: VecDeque::new(),
            id_map: HashMap::new(),
            max_size,
            ttl,
        })
    }

    /// Check if we've seen this message before
    pub fn has_seen(&self, id: &MessageId) -> bool {
        // Quick check with bloom filter (may have false positives)
        if !self.bloom.contains(id) {
            return false;
        }

        // Confirm with exact lookup
        self.id_map.contains_key(id)
    }

    /// Mark a message as seen
    pub fn mark_seen(&mut self, id: MessageId) {
        // Already seen?
        if self.id_map.contains_key(&id) {
            return;
        }

        // Add to bloom filter
        self.bloom.insert(&id);

        // Add to cache
        let entry = CacheEntry {
            id,
            seen_at: Instant::now(),
        };

        self.recent_ids.push_back(entry);
        self.id_map.insert(id, Instant::now());

        // Evict oldest if cache is full
        while self.recent_ids.len() > self.max_size {
            if let Some(old_entry) = self.recent_ids.pop_front() {
                self.id_map.remove(&old_entry.id);
            }
        }
    }

    /// Remove expired entries from cache
    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();

        // Remove expired entries from front of queue
        while let Some(entry) = self.recent_ids.front() {
            if now.duration_since(entry.seen_at) > self.ttl {
                let entry = self.recent_ids.pop_front().unwrap();
                self.id_map.remove(&entry.id);
            } else {
                break; // Queue is ordered by time, so we can stop
            }
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        (self.recent_ids.len(), self.max_size)
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.recent_ids.clear();
        self.id_map.clear();
        self.bloom.clear();
    }
}

/// Simple bloom filter implementation
struct BloomFilter {
    bits: Vec<bool>,
    num_hashes: usize,
    size: usize,
}

impl BloomFilter {
    /// Create a new bloom filter
    fn new(expected_items: usize, false_positive_rate: f64) -> Result<Self> {
        // Calculate optimal size and number of hash functions
        let size = Self::optimal_size(expected_items, false_positive_rate);
        let num_hashes = Self::optimal_hash_count(size, expected_items);

        Ok(Self {
            bits: vec![false; size],
            num_hashes,
            size,
        })
    }

    /// Calculate optimal bloom filter size
    fn optimal_size(n: usize, p: f64) -> usize {
        let size = -(n as f64 * p.ln()) / (2.0_f64.ln().powi(2));
        size.ceil() as usize
    }

    /// Calculate optimal number of hash functions
    fn optimal_hash_count(m: usize, n: usize) -> usize {
        let k = (m as f64 / n as f64) * 2.0_f64.ln();
        k.ceil() as usize
    }

    /// Check if an item might be in the set
    fn contains(&self, id: &MessageId) -> bool {
        for i in 0..self.num_hashes {
            let index = self.hash(id, i);
            if !self.bits[index] {
                return false;
            }
        }
        true
    }

    /// Insert an item into the set
    fn insert(&mut self, id: &MessageId) {
        for i in 0..self.num_hashes {
            let index = self.hash(id, i);
            self.bits[index] = true;
        }
    }

    /// Hash function (using message ID bytes + seed)
    fn hash(&self, id: &MessageId, seed: usize) -> usize {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        id.0.hash(&mut hasher);
        seed.hash(&mut hasher);

        (hasher.finish() as usize) % self.size
    }

    /// Clear the filter
    fn clear(&mut self) {
        self.bits.fill(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_message_id_creation() {
        let payload = b"test message";
        let id1 = MessageId::from_payload(payload);
        let id2 = MessageId::from_payload(payload);

        assert_eq!(id1, id2); // Same payload = same ID
    }

    #[test]
    fn test_message_id_different_payloads() {
        let id1 = MessageId::from_payload(b"message 1");
        let id2 = MessageId::from_payload(b"message 2");

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_cache_creation() {
        let cache = MessageCache::new(100, Duration::from_secs(60));
        assert!(cache.is_ok());
    }

    #[test]
    fn test_mark_seen() {
        let mut cache = MessageCache::new(100, Duration::from_secs(60)).unwrap();
        let id = MessageId::from_payload(b"test");

        assert!(!cache.has_seen(&id));
        cache.mark_seen(id);
        assert!(cache.has_seen(&id));
    }

    #[test]
    fn test_duplicate_detection() {
        let mut cache = MessageCache::new(100, Duration::from_secs(60)).unwrap();
        let id = MessageId::from_payload(b"duplicate test");

        // First time
        assert!(!cache.has_seen(&id));
        cache.mark_seen(id);

        // Second time
        assert!(cache.has_seen(&id));
        cache.mark_seen(id); // Should not add duplicate

        let (size, _) = cache.stats();
        assert_eq!(size, 1);
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = MessageCache::new(3, Duration::from_secs(60)).unwrap();

        let id1 = MessageId::from_payload(b"msg1");
        let id2 = MessageId::from_payload(b"msg2");
        let id3 = MessageId::from_payload(b"msg3");
        let id4 = MessageId::from_payload(b"msg4");

        cache.mark_seen(id1);
        cache.mark_seen(id2);
        cache.mark_seen(id3);

        let (size, _) = cache.stats();
        assert_eq!(size, 3);

        // Adding 4th should evict oldest (id1)
        cache.mark_seen(id4);

        let (size, _) = cache.stats();
        assert_eq!(size, 3);

        // id1 should no longer be in exact cache (may still be in bloom)
        assert!(cache.has_seen(&id2));
        assert!(cache.has_seen(&id3));
        assert!(cache.has_seen(&id4));
    }

    #[test]
    fn test_cache_expiration() {
        let mut cache = MessageCache::new(100, Duration::from_millis(100)).unwrap();
        let id = MessageId::from_payload(b"expiring message");

        cache.mark_seen(id);
        assert!(cache.has_seen(&id));

        // Wait for expiration
        thread::sleep(Duration::from_millis(150));

        // Cleanup expired
        cache.cleanup_expired();

        // Should be removed from exact cache
        let (size, _) = cache.stats();
        assert_eq!(size, 0);
    }

    #[test]
    fn test_bloom_filter() {
        let mut bloom = BloomFilter::new(1000, 0.01).unwrap();
        let id1 = MessageId::from_payload(b"test1");
        let id2 = MessageId::from_payload(b"test2");

        assert!(!bloom.contains(&id1));
        bloom.insert(&id1);
        assert!(bloom.contains(&id1));
        assert!(!bloom.contains(&id2));
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = MessageCache::new(100, Duration::from_secs(60)).unwrap();

        cache.mark_seen(MessageId::from_payload(b"msg1"));
        cache.mark_seen(MessageId::from_payload(b"msg2"));

        let (size, _) = cache.stats();
        assert_eq!(size, 2);

        cache.clear();

        let (size, _) = cache.stats();
        assert_eq!(size, 0);
    }
}
