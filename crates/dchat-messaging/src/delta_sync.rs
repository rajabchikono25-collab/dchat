//! Delta synchronization for efficient message syncing
//!
//! Uses bloom filters and sequence numbers to minimize bandwidth
//! when syncing messages between peers.

use dchat_core::error::{Error, Result};
use dchat_core::types::MessageId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Configuration for delta sync
#[derive(Debug, Clone)]
pub struct DeltaSyncConfig {
    /// Expected number of items in bloom filter
    pub bloom_expected_items: usize,
    /// False positive probability for bloom filter
    pub bloom_fp_rate: f64,
    /// Maximum messages per delta batch
    pub max_batch_size: usize,
    /// Merkle tree branching factor
    pub merkle_branching_factor: usize,
}

impl Default for DeltaSyncConfig {
    fn default() -> Self {
        Self {
            bloom_expected_items: 10000,
            bloom_fp_rate: 0.01, // 1% false positive rate
            max_batch_size: 100,
            merkle_branching_factor: 16,
        }
    }
}

/// Simple bloom filter for message ID deduplication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BloomFilter {
    bits: Vec<u64>,
    num_hashes: usize,
    num_bits: usize,
}

impl BloomFilter {
    /// Create a new bloom filter
    pub fn new(expected_items: usize, fp_rate: f64) -> Self {
        // Calculate optimal size: m = -n * ln(p) / (ln(2)^2)
        let num_bits = (-(expected_items as f64) * fp_rate.ln() / (2.0_f64.ln().powi(2)))
            .ceil() as usize;
        let num_bits = num_bits.max(64); // Minimum size
        
        // Calculate optimal hash functions: k = (m/n) * ln(2)
        let num_hashes = ((num_bits as f64 / expected_items as f64) * 2.0_f64.ln())
            .ceil() as usize;
        let num_hashes = num_hashes.max(1).min(16); // Between 1 and 16 hash functions
        
        let num_words = (num_bits + 63) / 64;
        
        Self {
            bits: vec![0; num_words],
            num_hashes,
            num_bits,
        }
    }
    
    /// Insert an item into the bloom filter
    pub fn insert<T: Hash>(&mut self, item: &T) {
        for i in 0..self.num_hashes {
            let bit_idx = self.hash_with_seed(item, i) % self.num_bits;
            let word_idx = bit_idx / 64;
            let bit_offset = bit_idx % 64;
            self.bits[word_idx] |= 1 << bit_offset;
        }
    }
    
    /// Check if an item might be in the bloom filter
    pub fn might_contain<T: Hash>(&self, item: &T) -> bool {
        for i in 0..self.num_hashes {
            let bit_idx = self.hash_with_seed(item, i) % self.num_bits;
            let word_idx = bit_idx / 64;
            let bit_offset = bit_idx % 64;
            if (self.bits[word_idx] & (1 << bit_offset)) == 0 {
                return false;
            }
        }
        true
    }
    
    /// Hash an item with a seed for multiple hash functions
    fn hash_with_seed<T: Hash>(&self, item: &T, seed: usize) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        seed.hash(&mut hasher);
        item.hash(&mut hasher);
        hasher.finish() as usize
    }
    
    /// Get approximate number of bits set
    pub fn approximate_count(&self) -> usize {
        let set_bits: usize = self.bits.iter().map(|w| w.count_ones() as usize).sum();
        let ratio = set_bits as f64 / self.num_bits as f64;
        if ratio >= 1.0 {
            return self.num_bits; // Saturated
        }
        // n ≈ -m/k * ln(1 - X/m) where X is set bits
        (-(self.num_bits as f64) / self.num_hashes as f64 * (1.0 - ratio).ln()) as usize
    }
    
    /// Merge another bloom filter into this one (OR operation)
    pub fn merge(&mut self, other: &BloomFilter) {
        if self.bits.len() == other.bits.len() {
            for (a, b) in self.bits.iter_mut().zip(other.bits.iter()) {
                *a |= *b;
            }
        }
    }
    
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
    
    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| Error::validation(format!("Failed to deserialize bloom filter: {}", e)))
    }
}

/// Represents a tracked message for delta sync
#[derive(Debug, Clone)]
pub struct TrackedMessage {
    pub id: MessageId,
    pub sequence: u64,
    pub hash: [u8; 32],
    pub size: usize,
    pub timestamp: u64,
}

/// Delta representing missing messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    /// Starting sequence number
    pub from_sequence: u64,
    /// Ending sequence number (current)
    pub to_sequence: u64,
    /// Message IDs that are missing on the peer
    pub missing_message_ids: Vec<MessageId>,
    /// Optional Merkle proof for verification
    pub merkle_proof: Option<Vec<[u8; 32]>>,
    /// Total size of messages in bytes
    pub total_size: usize,
}

impl Delta {
    /// Check if the delta is empty
    pub fn is_empty(&self) -> bool {
        self.missing_message_ids.is_empty()
    }
    
    /// Get the number of missing messages
    pub fn count(&self) -> usize {
        self.missing_message_ids.len()
    }
}

/// Delta sync state for efficient message synchronization
pub struct DeltaSync {
    /// Configuration
    config: DeltaSyncConfig,
    /// Last known sequence per peer (peer_id_bytes -> sequence)
    peer_sequences: HashMap<Vec<u8>, u64>,
    /// Recent messages with their metadata
    messages: Vec<TrackedMessage>,
    /// Current sequence number
    current_sequence: u64,
    /// Bloom filter for quick membership checks
    message_bloom: BloomFilter,
    /// Merkle tree root hash
    merkle_root: [u8; 32],
}

impl DeltaSync {
    /// Create a new delta sync instance
    pub fn new(config: DeltaSyncConfig) -> Self {
        let message_bloom = BloomFilter::new(
            config.bloom_expected_items,
            config.bloom_fp_rate,
        );
        
        Self {
            config,
            peer_sequences: HashMap::new(),
            messages: Vec::new(),
            current_sequence: 0,
            message_bloom,
            merkle_root: [0u8; 32],
        }
    }
    
    /// Add a message to the sync tracker
    pub fn track_message(&mut self, id: MessageId, hash: [u8; 32], size: usize) {
        self.current_sequence += 1;
        
        let tracked = TrackedMessage {
            id: id.clone(),
            sequence: self.current_sequence,
            hash,
            size,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        // Add to bloom filter
        self.message_bloom.insert(&id);
        
        // Add to tracked messages
        self.messages.push(tracked);
        
        // Update Merkle root
        self.update_merkle_root();
        
        // Prune old messages if needed
        self.prune_old_messages();
    }
    
    /// Compute what messages a peer is missing
    pub fn compute_delta(&self, _peer_id: &[u8], peer_sequence: u64, peer_bloom: Option<&BloomFilter>) -> Delta {
        let mut missing_message_ids = Vec::new();
        let mut total_size = 0;
        
        for msg in &self.messages {
            // Skip messages the peer already has by sequence
            if msg.sequence <= peer_sequence {
                continue;
            }
            
            // Skip messages the peer might already have (bloom filter check)
            if let Some(bloom) = peer_bloom {
                if bloom.might_contain(&msg.id) {
                    continue; // Probably already has it
                }
            }
            
            missing_message_ids.push(msg.id.clone());
            total_size += msg.size;
            
            // Limit batch size
            if missing_message_ids.len() >= self.config.max_batch_size {
                break;
            }
        }
        
        Delta {
            from_sequence: peer_sequence,
            to_sequence: self.current_sequence,
            missing_message_ids,
            merkle_proof: self.generate_merkle_proof(peer_sequence),
            total_size,
        }
    }
    
    /// Update the peer's known sequence number
    pub fn update_peer_sequence(&mut self, peer_id: &[u8], sequence: u64) {
        self.peer_sequences.insert(peer_id.to_vec(), sequence);
    }
    
    /// Get the current sequence number
    pub fn current_sequence(&self) -> u64 {
        self.current_sequence
    }
    
    /// Get our bloom filter for sharing with peers
    pub fn get_bloom_filter(&self) -> &BloomFilter {
        &self.message_bloom
    }
    
    /// Get the Merkle root for verification
    pub fn merkle_root(&self) -> [u8; 32] {
        self.merkle_root
    }
    
    /// Generate sync request data to send to a peer
    pub fn create_sync_request(&self) -> SyncRequest {
        SyncRequest {
            sequence: self.current_sequence,
            bloom_filter: self.message_bloom.to_bytes(),
            merkle_root: self.merkle_root,
        }
    }
    
    /// Process a sync request from a peer
    pub fn process_sync_request(&self, peer_id: &[u8], request: &SyncRequest) -> Result<Delta> {
        let peer_bloom = if !request.bloom_filter.is_empty() {
            Some(BloomFilter::from_bytes(&request.bloom_filter)?)
        } else {
            None
        };
        
        Ok(self.compute_delta(peer_id, request.sequence, peer_bloom.as_ref()))
    }
    
    /// Mark messages as received from a delta response
    pub fn acknowledge_delta(&mut self, peer_id: &[u8], delta: &Delta) {
        // Update peer sequence to the new watermark
        self.update_peer_sequence(peer_id, delta.to_sequence);
    }
    
    /// Check if we need to sync with a peer
    pub fn needs_sync(&self, peer_id: &[u8]) -> bool {
        match self.peer_sequences.get(peer_id) {
            Some(&seq) => seq < self.current_sequence,
            None => self.current_sequence > 0,
        }
    }
    
    /// Get statistics about the sync state
    pub fn stats(&self) -> DeltaSyncStats {
        DeltaSyncStats {
            tracked_messages: self.messages.len(),
            current_sequence: self.current_sequence,
            tracked_peers: self.peer_sequences.len(),
            bloom_approximate_items: self.message_bloom.approximate_count(),
        }
    }
    
    /// Update the Merkle tree root hash
    fn update_merkle_root(&mut self) {
        if self.messages.is_empty() {
            self.merkle_root = [0u8; 32];
            return;
        }
        
        // Simple Merkle tree: hash all message hashes together
        let hashes: Vec<[u8; 32]> = self.messages.iter().map(|m| m.hash).collect();
        self.merkle_root = self.compute_merkle_root(&hashes);
    }
    
    /// Compute Merkle root from a list of hashes
    fn compute_merkle_root(&self, hashes: &[[u8; 32]]) -> [u8; 32] {
        if hashes.is_empty() {
            return [0u8; 32];
        }
        
        if hashes.len() == 1 {
            return hashes[0];
        }
        
        // Pair up hashes and combine
        let mut next_level = Vec::new();
        for chunk in hashes.chunks(2) {
            let combined = if chunk.len() == 2 {
                self.hash_pair(&chunk[0], &chunk[1])
            } else {
                self.hash_pair(&chunk[0], &chunk[0]) // Duplicate odd element
            };
            next_level.push(combined);
        }
        
        self.compute_merkle_root(&next_level)
    }
    
    /// Hash two 32-byte hashes together
    fn hash_pair(&self, left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(left);
        hasher.update(right);
        let result = hasher.finalize();
        let mut output = [0u8; 32];
        output.copy_from_slice(&result);
        output
    }
    
    /// Generate a Merkle proof for messages after a given sequence
    fn generate_merkle_proof(&self, from_sequence: u64) -> Option<Vec<[u8; 32]>> {
        // Simplified: just return the hashes of relevant messages
        let proof: Vec<[u8; 32]> = self.messages
            .iter()
            .filter(|m| m.sequence > from_sequence)
            .take(10) // Limit proof size
            .map(|m| m.hash)
            .collect();
        
        if proof.is_empty() {
            None
        } else {
            Some(proof)
        }
    }
    
    /// Prune old messages to limit memory usage
    fn prune_old_messages(&mut self) {
        const MAX_TRACKED_MESSAGES: usize = 50000;
        
        if self.messages.len() > MAX_TRACKED_MESSAGES {
            // Remove oldest messages, keep most recent
            let to_remove = self.messages.len() - MAX_TRACKED_MESSAGES;
            self.messages.drain(0..to_remove);
            
            // Note: We don't remove from bloom filter (it's a probabilistic structure)
            // The bloom filter will be rebuilt periodically
        }
    }
    
    /// Rebuild the bloom filter (call periodically to reduce false positives)
    pub fn rebuild_bloom_filter(&mut self) {
        self.message_bloom = BloomFilter::new(
            self.config.bloom_expected_items,
            self.config.bloom_fp_rate,
        );
        
        for msg in &self.messages {
            self.message_bloom.insert(&msg.id);
        }
    }
}

impl Default for DeltaSync {
    fn default() -> Self {
        Self::new(DeltaSyncConfig::default())
    }
}

/// Sync request sent to a peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    /// Our current sequence number
    pub sequence: u64,
    /// Serialized bloom filter
    pub bloom_filter: Vec<u8>,
    /// Our Merkle root
    pub merkle_root: [u8; 32],
}

/// Sync response with delta information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Delta of missing messages
    pub delta: Delta,
    /// Sender's current sequence
    pub sender_sequence: u64,
    /// Sender's Merkle root
    pub sender_merkle_root: [u8; 32],
}

/// Statistics about delta sync state
#[derive(Debug, Clone)]
pub struct DeltaSyncStats {
    pub tracked_messages: usize,
    pub current_sequence: u64,
    pub tracked_peers: usize,
    pub bloom_approximate_items: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bloom_filter_basic() {
        let mut bloom = BloomFilter::new(1000, 0.01);
        
        bloom.insert(&"hello");
        bloom.insert(&"world");
        
        assert!(bloom.might_contain(&"hello"));
        assert!(bloom.might_contain(&"world"));
        // False positives are possible but unlikely
    }
    
    #[test]
    fn test_bloom_filter_false_positive_rate() {
        let mut bloom = BloomFilter::new(1000, 0.01);
        
        // Insert 1000 items
        for i in 0..1000 {
            bloom.insert(&i);
        }
        
        // Check items that weren't inserted
        let mut false_positives = 0;
        for i in 1000..2000 {
            if bloom.might_contain(&i) {
                false_positives += 1;
            }
        }
        
        // False positive rate should be around 1% (allow some variance)
        let fp_rate = false_positives as f64 / 1000.0;
        assert!(fp_rate < 0.05, "False positive rate too high: {}", fp_rate);
    }
    
    #[test]
    fn test_delta_sync_basic() {
        let mut sync = DeltaSync::new(DeltaSyncConfig::default());
        
        // Track some messages
        for i in 0..10 {
            let id = MessageId::new();
            let hash = [i as u8; 32];
            sync.track_message(id, hash, 100);
        }
        
        assert_eq!(sync.current_sequence(), 10);
        
        // Compute delta for a peer that has seen up to sequence 5
        let peer_id = b"peer1";
        let delta = sync.compute_delta(peer_id, 5, None);
        
        assert_eq!(delta.from_sequence, 5);
        assert_eq!(delta.to_sequence, 10);
        assert_eq!(delta.count(), 5); // Messages 6-10
    }
    
    #[test]
    fn test_delta_sync_with_bloom() {
        let mut sync = DeltaSync::new(DeltaSyncConfig::default());
        let mut peer_bloom = BloomFilter::new(100, 0.01);
        
        let mut ids = Vec::new();
        
        // Track 10 messages
        for i in 0..10 {
            let id = MessageId::new();
            ids.push(id.clone());
            sync.track_message(id, [i as u8; 32], 100);
        }
        
        // Peer already has messages 6-10 (in their bloom filter)
        for id in &ids[5..10] {
            peer_bloom.insert(id);
        }
        
        // Compute delta - should only show messages 1-5
        let peer_id = b"peer1";
        let delta = sync.compute_delta(peer_id, 0, Some(&peer_bloom));
        
        // Should have 5 messages (the ones not in peer's bloom)
        assert_eq!(delta.count(), 5);
    }
    
    #[test]
    fn test_sync_request_response() {
        let mut sync1 = DeltaSync::new(DeltaSyncConfig::default());
        let sync2 = DeltaSync::new(DeltaSyncConfig::default());
        
        // Sync1 has some messages
        for i in 0..5 {
            sync1.track_message(MessageId::new(), [i as u8; 32], 100);
        }
        
        // Sync2 creates a request (it has nothing)
        let request = sync2.create_sync_request();
        
        // Sync1 processes the request
        let delta = sync1.process_sync_request(b"peer2", &request).unwrap();
        
        // Delta should contain all 5 messages
        assert_eq!(delta.count(), 5);
    }
}
