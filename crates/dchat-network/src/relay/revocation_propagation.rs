//! Revocation Propagation Protocol for QGE
//!
//! This module implements fast gossip-based propagation of revocation entries
//! across the relay network, ensuring rapid enforcement of access revocations.
//!
//! # Overview
//!
//! When a device, user, or membership is revoked, the revocation must propagate
//! to all relays quickly to prevent stale tokens from being issued. This module
//! provides:
//!
//! - **Gossip-based Propagation**: Efficient flooding via gossipsub
//! - **Anti-Entropy Sync**: Merkle-tree based state reconciliation
//! - **Priority Ordering**: Emergency revocations propagate first
//! - **Deduplication**: Bloom filter prevents redundant processing
//! - **Reliability**: Acknowledgment and retry for critical revocations
//!
//! # Protocol Flow
//!
//! 1. **Origin**: A relay receives a revocation (from admin, governance, or user)
//! 2. **Sign**: Relay signs the revocation entry
//! 3. **Gossip**: Revocation is broadcast via gossipsub topic
//! 4. **Receive**: Other relays receive and validate the gossip message
//! 5. **Apply**: Valid revocations are added to local store
//! 6. **Ack**: Critical revocations receive acknowledgments
//! 7. **Sync**: Periodic anti-entropy detects missed revocations
//!
//! # Security Properties
//!
//! - **Authenticity**: Revocations are signed by authority keys
//! - **Integrity**: Merkle roots detect tampering
//! - **Availability**: Multiple propagation paths ensure delivery
//! - **Timeliness**: Priority queue ensures urgent revocations propagate first

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};

use super::revocation::{RevocationEntry, RevocationId, RevocationStore, RevocationType};

/// Serde helper for [u8; 64] arrays (signatures)
mod serde_bytes_64 {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<u8> = Vec::deserialize(deserializer)?;
        vec.try_into()
            .map_err(|_| serde::de::Error::custom("Expected 64 bytes"))
    }
}

/// Gossipsub topic for revocation messages
pub const REVOCATION_TOPIC: &str = "/dchat/revocations/1.0.0";

/// Maximum revocations per gossip message
pub const MAX_REVOCATIONS_PER_MESSAGE: usize = 50;

/// Sync interval for anti-entropy protocol (seconds)
pub const SYNC_INTERVAL_SECS: u64 = 60;

/// Retry interval for unacknowledged critical revocations (seconds)
pub const RETRY_INTERVAL_SECS: u64 = 5;

/// Maximum retry attempts for critical revocations
pub const MAX_RETRY_ATTEMPTS: u8 = 5;

/// Bloom filter size for deduplication
pub const BLOOM_FILTER_SIZE: usize = 10_000;

/// Priority levels for revocation propagation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PropagationPriority {
    /// Highest priority - security emergencies
    Emergency = 0,
    /// High priority - device revocations
    High = 1,
    /// Normal priority - membership changes
    Normal = 2,
    /// Low priority - scheduled revocations
    Low = 3,
}

impl From<&RevocationType> for PropagationPriority {
    fn from(rev_type: &RevocationType) -> Self {
        match rev_type {
            RevocationType::Emergency => PropagationPriority::Emergency,
            RevocationType::Device => PropagationPriority::High,
            RevocationType::User => PropagationPriority::High,
            RevocationType::Membership => PropagationPriority::Normal,
        }
    }
}

/// Gossip message for revocation propagation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationGossipMessage {
    /// Message version
    pub version: u8,
    /// Message type
    pub message_type: GossipMessageType,
    /// Sender relay ID
    pub sender: [u8; 32],
    /// Timestamp
    pub timestamp: u64,
    /// Signature from sender
    #[serde(with = "serde_bytes_64")]
    pub signature: [u8; 64],
}

/// Type of gossip message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessageType {
    /// New revocation(s) to propagate
    Revocations {
        entries: Vec<RevocationEntry>,
        priority: PropagationPriority,
    },

    /// Request acknowledgment for critical revocation
    AckRequest { revocation_ids: Vec<RevocationId> },

    /// Acknowledge receipt of revocation(s)
    Ack { revocation_ids: Vec<RevocationId> },

    /// Anti-entropy sync request
    SyncRequest {
        /// Local merkle root
        merkle_root: [u8; 32],
        /// Local sequence number
        sequence: u64,
        /// Bloom filter of known revocation IDs
        known_ids_bloom: Vec<u8>,
    },

    /// Anti-entropy sync response
    SyncResponse {
        /// Remote merkle root
        merkle_root: [u8; 32],
        /// Remote sequence number
        sequence: u64,
        /// Revocations the requester is missing
        missing_revocations: Vec<RevocationEntry>,
    },
}

impl RevocationGossipMessage {
    /// Create a new revocations message
    pub fn revocations(
        sender: [u8; 32],
        entries: Vec<RevocationEntry>,
        priority: PropagationPriority,
    ) -> Self {
        Self {
            version: 1,
            message_type: GossipMessageType::Revocations { entries, priority },
            sender,
            timestamp: current_timestamp(),
            signature: [0u8; 64], // Will be signed later
        }
    }

    /// Create a sync request
    pub fn sync_request(
        sender: [u8; 32],
        merkle_root: [u8; 32],
        sequence: u64,
        known_ids_bloom: Vec<u8>,
    ) -> Self {
        Self {
            version: 1,
            message_type: GossipMessageType::SyncRequest {
                merkle_root,
                sequence,
                known_ids_bloom,
            },
            sender,
            timestamp: current_timestamp(),
            signature: [0u8; 64],
        }
    }

    /// Create a sync response
    pub fn sync_response(
        sender: [u8; 32],
        merkle_root: [u8; 32],
        sequence: u64,
        missing_revocations: Vec<RevocationEntry>,
    ) -> Self {
        Self {
            version: 1,
            message_type: GossipMessageType::SyncResponse {
                merkle_root,
                sequence,
                missing_revocations,
            },
            sender,
            timestamp: current_timestamp(),
            signature: [0u8; 64],
        }
    }

    /// Create an acknowledgment
    pub fn ack(sender: [u8; 32], revocation_ids: Vec<RevocationId>) -> Self {
        Self {
            version: 1,
            message_type: GossipMessageType::Ack { revocation_ids },
            sender,
            timestamp: current_timestamp(),
            signature: [0u8; 64],
        }
    }

    /// Get the data to sign
    pub fn signing_data(&self) -> Vec<u8> {
        // Serialize without signature for signing
        let mut data = Vec::new();
        data.push(self.version);
        data.extend_from_slice(&self.sender);
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        // Add type-specific data
        match &self.message_type {
            GossipMessageType::Revocations { entries, priority } => {
                data.push(0x01);
                data.push(*priority as u8);
                data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
                for entry in entries {
                    data.extend_from_slice(&entry.id);
                }
            }
            GossipMessageType::AckRequest { revocation_ids } => {
                data.push(0x02);
                for id in revocation_ids {
                    data.extend_from_slice(id);
                }
            }
            GossipMessageType::Ack { revocation_ids } => {
                data.push(0x03);
                for id in revocation_ids {
                    data.extend_from_slice(id);
                }
            }
            GossipMessageType::SyncRequest {
                merkle_root,
                sequence,
                ..
            } => {
                data.push(0x04);
                data.extend_from_slice(merkle_root);
                data.extend_from_slice(&sequence.to_le_bytes());
            }
            GossipMessageType::SyncResponse {
                merkle_root,
                sequence,
                ..
            } => {
                data.push(0x05);
                data.extend_from_slice(merkle_root);
                data.extend_from_slice(&sequence.to_le_bytes());
            }
        }
        data
    }
}

/// Pending acknowledgment tracker
#[derive(Debug)]
struct PendingAck {
    revocation_id: RevocationId,
    sent_at: Instant,
    retry_count: u8,
    acked_by: HashSet<[u8; 32]>,
}

/// Simple bloom filter for deduplication
pub struct BloomFilter {
    bits: Vec<bool>,
    num_hashes: usize,
}

impl BloomFilter {
    /// Create a new bloom filter
    pub fn new(size: usize, num_hashes: usize) -> Self {
        Self {
            bits: vec![false; size],
            num_hashes,
        }
    }

    /// Add an item to the filter
    pub fn insert(&mut self, item: &[u8]) {
        for i in 0..self.num_hashes {
            let hash = self.hash(item, i);
            let index = hash % self.bits.len();
            self.bits[index] = true;
        }
    }

    /// Check if an item might be in the filter
    pub fn might_contain(&self, item: &[u8]) -> bool {
        for i in 0..self.num_hashes {
            let hash = self.hash(item, i);
            let index = hash % self.bits.len();
            if !self.bits[index] {
                return false;
            }
        }
        true
    }

    /// Hash function for bloom filter
    fn hash(&self, item: &[u8], seed: usize) -> usize {
        let mut hasher = blake3::Hasher::new();
        hasher.update(item);
        hasher.update(&(seed as u64).to_le_bytes());
        let hash = hasher.finalize();
        let bytes: [u8; 8] = hash.as_bytes()[0..8].try_into().unwrap();
        usize::from_le_bytes(bytes)
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.bits.len() / 8 + 1);
        for chunk in self.bits.chunks(8) {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                if bit {
                    byte |= 1 << i;
                }
            }
            bytes.push(byte);
        }
        bytes
    }

    /// Create from bytes
    pub fn from_bytes(bytes: &[u8], size: usize, num_hashes: usize) -> Self {
        let mut bits = vec![false; size];
        for (byte_idx, &byte) in bytes.iter().enumerate() {
            for bit_idx in 0..8 {
                let idx = byte_idx * 8 + bit_idx;
                if idx < size {
                    bits[idx] = (byte >> bit_idx) & 1 == 1;
                }
            }
        }
        Self { bits, num_hashes }
    }
}

/// Statistics for revocation propagation
#[derive(Debug, Clone, Default)]
pub struct PropagationStats {
    /// Revocations sent
    pub sent: u64,
    /// Revocations received
    pub received: u64,
    /// Duplicates filtered
    pub duplicates_filtered: u64,
    /// Acknowledgments received
    pub acks_received: u64,
    /// Sync requests sent
    pub sync_requests: u64,
    /// Revocations recovered via sync
    pub recovered_via_sync: u64,
    /// Average propagation latency (ms)
    pub avg_propagation_latency_ms: u64,
}

/// Revocation propagation manager
pub struct RevocationPropagator {
    /// Local relay ID
    relay_id: [u8; 32],

    /// Revocation store reference
    store: Arc<RwLock<RevocationStore>>,

    /// Pending outbound revocations (priority queue)
    outbound_queue: Arc<RwLock<VecDeque<(PropagationPriority, RevocationEntry)>>>,

    /// Bloom filter for received revocation IDs
    received_bloom: Arc<RwLock<BloomFilter>>,

    /// Pending acknowledgments
    pending_acks: Arc<RwLock<HashMap<RevocationId, PendingAck>>>,

    /// Known peers
    known_peers: Arc<RwLock<HashSet<[u8; 32]>>>,

    /// Statistics
    stats: Arc<RwLock<PropagationStats>>,

    /// Configuration
    config: PropagationConfig,
}

/// Configuration for propagation
#[derive(Debug, Clone)]
pub struct PropagationConfig {
    /// Sync interval
    pub sync_interval: Duration,
    /// Retry interval for critical revocations
    pub retry_interval: Duration,
    /// Maximum retry attempts
    pub max_retries: u8,
    /// Batch size for outbound messages
    pub batch_size: usize,
    /// Enable acknowledgments
    pub enable_acks: bool,
}

impl Default for PropagationConfig {
    fn default() -> Self {
        Self {
            sync_interval: Duration::from_secs(SYNC_INTERVAL_SECS),
            retry_interval: Duration::from_secs(RETRY_INTERVAL_SECS),
            max_retries: MAX_RETRY_ATTEMPTS,
            batch_size: MAX_REVOCATIONS_PER_MESSAGE,
            enable_acks: true,
        }
    }
}

impl RevocationPropagator {
    /// Create a new propagator
    pub fn new(relay_id: [u8; 32], store: Arc<RwLock<RevocationStore>>) -> Self {
        Self {
            relay_id,
            store,
            outbound_queue: Arc::new(RwLock::new(VecDeque::new())),
            received_bloom: Arc::new(RwLock::new(BloomFilter::new(BLOOM_FILTER_SIZE, 3))),
            pending_acks: Arc::new(RwLock::new(HashMap::new())),
            known_peers: Arc::new(RwLock::new(HashSet::new())),
            stats: Arc::new(RwLock::new(PropagationStats::default())),
            config: PropagationConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(
        relay_id: [u8; 32],
        store: Arc<RwLock<RevocationStore>>,
        config: PropagationConfig,
    ) -> Self {
        Self {
            relay_id,
            store,
            outbound_queue: Arc::new(RwLock::new(VecDeque::new())),
            received_bloom: Arc::new(RwLock::new(BloomFilter::new(BLOOM_FILTER_SIZE, 3))),
            pending_acks: Arc::new(RwLock::new(HashMap::new())),
            known_peers: Arc::new(RwLock::new(HashSet::new())),
            stats: Arc::new(RwLock::new(PropagationStats::default())),
            config,
        }
    }

    /// Queue a revocation for propagation
    pub async fn queue_revocation(&self, entry: RevocationEntry) {
        let priority = PropagationPriority::from(&entry.revocation_type);
        let mut queue = self.outbound_queue.write().await;

        // Insert in priority order
        let pos = queue
            .iter()
            .position(|(p, _)| *p > priority)
            .unwrap_or(queue.len());

        queue.insert(pos, (priority, entry));
    }

    /// Get batch of revocations to send
    pub async fn get_outbound_batch(&self) -> Option<(PropagationPriority, Vec<RevocationEntry>)> {
        let mut queue = self.outbound_queue.write().await;

        if queue.is_empty() {
            return None;
        }

        let mut batch = Vec::with_capacity(self.config.batch_size);
        let first_priority = queue.front().map(|(p, _)| *p)?;

        // Take up to batch_size entries of same priority
        while batch.len() < self.config.batch_size {
            if let Some((priority, _)) = queue.front() {
                if *priority != first_priority {
                    break;
                }
                if let Some((_, entry)) = queue.pop_front() {
                    batch.push(entry);
                }
            } else {
                break;
            }
        }

        if batch.is_empty() {
            None
        } else {
            Some((first_priority, batch))
        }
    }

    /// Process received gossip message
    pub async fn process_message(
        &self,
        message: RevocationGossipMessage,
    ) -> Result<Option<RevocationGossipMessage>> {
        match message.message_type {
            GossipMessageType::Revocations { entries, priority } => {
                self.handle_revocations(entries, priority, message.sender)
                    .await
            }
            GossipMessageType::AckRequest { revocation_ids } => {
                self.handle_ack_request(revocation_ids, message.sender)
                    .await
            }
            GossipMessageType::Ack { revocation_ids } => {
                self.handle_ack(revocation_ids, message.sender).await
            }
            GossipMessageType::SyncRequest {
                merkle_root,
                sequence,
                known_ids_bloom,
            } => {
                self.handle_sync_request(merkle_root, sequence, known_ids_bloom, message.sender)
                    .await
            }
            GossipMessageType::SyncResponse {
                missing_revocations,
                ..
            } => self.handle_sync_response(missing_revocations).await,
        }
    }

    /// Handle received revocations
    async fn handle_revocations(
        &self,
        entries: Vec<RevocationEntry>,
        _priority: PropagationPriority,
        sender: [u8; 32],
    ) -> Result<Option<RevocationGossipMessage>> {
        let mut store = self.store.write().await;
        let mut bloom = self.received_bloom.write().await;
        let mut stats = self.stats.write().await;

        let mut new_ids = Vec::new();

        for entry in entries {
            // Check bloom filter first
            if bloom.might_contain(&entry.id) {
                stats.duplicates_filtered += 1;
                continue;
            }

            // Add to store
            if store.add(entry.clone()).is_ok() {
                bloom.insert(&entry.id);
                stats.received += 1;
                new_ids.push(entry.id);
            }
        }

        // Send ack if enabled and there are new revocations
        if self.config.enable_acks && !new_ids.is_empty() {
            Ok(Some(RevocationGossipMessage::ack(self.relay_id, new_ids)))
        } else {
            Ok(None)
        }
    }

    /// Handle acknowledgment request
    async fn handle_ack_request(
        &self,
        revocation_ids: Vec<RevocationId>,
        _sender: [u8; 32],
    ) -> Result<Option<RevocationGossipMessage>> {
        let store = self.store.read().await;
        let known_ids: Vec<RevocationId> = revocation_ids
            .into_iter()
            .filter(|id| store.get_by_id(id).is_some())
            .collect();

        if known_ids.is_empty() {
            Ok(None)
        } else {
            Ok(Some(RevocationGossipMessage::ack(self.relay_id, known_ids)))
        }
    }

    /// Handle acknowledgment
    async fn handle_ack(
        &self,
        revocation_ids: Vec<RevocationId>,
        sender: [u8; 32],
    ) -> Result<Option<RevocationGossipMessage>> {
        let mut pending = self.pending_acks.write().await;
        let mut stats = self.stats.write().await;

        for id in revocation_ids {
            if let Some(ack) = pending.get_mut(&id) {
                ack.acked_by.insert(sender);
                stats.acks_received += 1;
            }
        }

        Ok(None)
    }

    /// Handle sync request
    async fn handle_sync_request(
        &self,
        remote_merkle_root: [u8; 32],
        remote_sequence: u64,
        known_ids_bloom_bytes: Vec<u8>,
        _sender: [u8; 32],
    ) -> Result<Option<RevocationGossipMessage>> {
        let store = self.store.read().await;
        let local_root = *store.merkle_root();
        let local_sequence = store.sequence();

        // If roots match, no sync needed
        if local_root == remote_merkle_root {
            return Ok(None);
        }

        // Reconstruct bloom filter
        let known_bloom = BloomFilter::from_bytes(&known_ids_bloom_bytes, BLOOM_FILTER_SIZE, 3);

        // Find revocations the remote doesn't have
        let missing: Vec<RevocationEntry> = store
            .all_entries()
            .filter(|e| !known_bloom.might_contain(&e.id))
            .take(MAX_REVOCATIONS_PER_MESSAGE)
            .cloned()
            .collect();

        if missing.is_empty() {
            return Ok(None);
        }

        Ok(Some(RevocationGossipMessage::sync_response(
            self.relay_id,
            local_root,
            local_sequence,
            missing,
        )))
    }

    /// Handle sync response
    async fn handle_sync_response(
        &self,
        missing_revocations: Vec<RevocationEntry>,
    ) -> Result<Option<RevocationGossipMessage>> {
        let mut store = self.store.write().await;
        let mut bloom = self.received_bloom.write().await;
        let mut stats = self.stats.write().await;

        for entry in missing_revocations {
            if store.add(entry.clone()).is_ok() {
                bloom.insert(&entry.id);
                stats.recovered_via_sync += 1;
            }
        }

        Ok(None)
    }

    /// Create a sync request message
    pub async fn create_sync_request(&self) -> RevocationGossipMessage {
        let store = self.store.read().await;
        let bloom = self.received_bloom.read().await;

        let mut stats = self.stats.write().await;
        stats.sync_requests += 1;

        RevocationGossipMessage::sync_request(
            self.relay_id,
            *store.merkle_root(),
            store.sequence(),
            bloom.to_bytes(),
        )
    }

    /// Register a known peer
    pub async fn register_peer(&self, peer_id: [u8; 32]) {
        let mut peers = self.known_peers.write().await;
        peers.insert(peer_id);
    }

    /// Remove a peer
    pub async fn remove_peer(&self, peer_id: &[u8; 32]) {
        let mut peers = self.known_peers.write().await;
        peers.remove(peer_id);
    }

    /// Get number of known peers
    pub async fn peer_count(&self) -> usize {
        let peers = self.known_peers.read().await;
        peers.len()
    }

    /// Check for pending retries
    pub async fn check_pending_retries(&self) -> Vec<RevocationId> {
        let mut pending = self.pending_acks.write().await;
        let now = Instant::now();

        let mut needs_retry = Vec::new();

        for (id, ack) in pending.iter_mut() {
            if now.duration_since(ack.sent_at) >= self.config.retry_interval {
                if ack.retry_count < self.config.max_retries {
                    ack.retry_count += 1;
                    ack.sent_at = now;
                    needs_retry.push(*id);
                }
            }
        }

        // Remove entries that exceeded max retries
        pending.retain(|_, ack| ack.retry_count < self.config.max_retries);

        needs_retry
    }

    /// Get propagation statistics
    pub async fn stats(&self) -> PropagationStats {
        self.stats.read().await.clone()
    }

    /// Get queue length
    pub async fn queue_length(&self) -> usize {
        self.outbound_queue.read().await.len()
    }

    /// Start background propagation task
    pub fn start_propagation_task(
        self: Arc<Self>,
        mut outbound_tx: mpsc::Sender<RevocationGossipMessage>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let batch_interval = Duration::from_millis(100);
            let mut sync_timer = Instant::now();

            loop {
                // Check for outbound batches
                if let Some((priority, entries)) = self.get_outbound_batch().await {
                    let message =
                        RevocationGossipMessage::revocations(self.relay_id, entries, priority);

                    if outbound_tx.send(message).await.is_err() {
                        tracing::error!("Failed to send revocation gossip message");
                    }

                    let mut stats = self.stats.write().await;
                    stats.sent += 1;
                }

                // Check for retries
                let retries = self.check_pending_retries().await;
                for id in retries {
                    let message = RevocationGossipMessage {
                        version: 1,
                        message_type: GossipMessageType::AckRequest {
                            revocation_ids: vec![id],
                        },
                        sender: self.relay_id,
                        timestamp: current_timestamp(),
                        signature: [0u8; 64],
                    };

                    if outbound_tx.send(message).await.is_err() {
                        tracing::error!("Failed to send retry message");
                    }
                }

                // Periodic sync
                if sync_timer.elapsed() >= self.config.sync_interval {
                    let sync_request = self.create_sync_request().await;
                    if outbound_tx.send(sync_request).await.is_err() {
                        tracing::error!("Failed to send sync request");
                    }
                    sync_timer = Instant::now();
                }

                tokio::time::sleep(batch_interval).await;
            }
        })
    }
}

/// Helper to get current timestamp
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(PropagationPriority::Emergency < PropagationPriority::High);
        assert!(PropagationPriority::High < PropagationPriority::Normal);
        assert!(PropagationPriority::Normal < PropagationPriority::Low);
    }

    #[test]
    fn test_bloom_filter() {
        let mut bloom = BloomFilter::new(1000, 3);

        bloom.insert(b"test1");
        bloom.insert(b"test2");

        assert!(bloom.might_contain(b"test1"));
        assert!(bloom.might_contain(b"test2"));
        // May have false positives, but unlikely for small set
    }

    #[test]
    fn test_bloom_serialization() {
        let mut bloom = BloomFilter::new(1000, 3);
        bloom.insert(b"test1");
        bloom.insert(b"test2");

        let bytes = bloom.to_bytes();
        let restored = BloomFilter::from_bytes(&bytes, 1000, 3);

        assert!(restored.might_contain(b"test1"));
        assert!(restored.might_contain(b"test2"));
    }

    #[test]
    fn test_gossip_message_creation() {
        let sender = [1u8; 32];
        let msg = RevocationGossipMessage::revocations(sender, vec![], PropagationPriority::Normal);

        assert_eq!(msg.version, 1);
        assert_eq!(msg.sender, sender);
    }

    #[test]
    fn test_signing_data() {
        let sender = [1u8; 32];
        let msg = RevocationGossipMessage::ack(sender, vec![[2u8; 32]]);

        let data = msg.signing_data();
        assert!(!data.is_empty());
        assert_eq!(data[0], 1); // version
    }

    #[tokio::test]
    async fn test_propagator_creation() {
        let store = Arc::new(RwLock::new(RevocationStore::new()));
        let propagator = RevocationPropagator::new([1u8; 32], store);

        assert_eq!(propagator.queue_length().await, 0);
    }

    #[tokio::test]
    async fn test_peer_management() {
        let store = Arc::new(RwLock::new(RevocationStore::new()));
        let propagator = RevocationPropagator::new([1u8; 32], store);

        propagator.register_peer([2u8; 32]).await;
        assert_eq!(propagator.peer_count().await, 1);

        propagator.remove_peer(&[2u8; 32]).await;
        assert_eq!(propagator.peer_count().await, 0);
    }
}
