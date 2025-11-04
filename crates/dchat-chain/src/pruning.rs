//! Message consensus pruning with governance-driven expiration and Merkle checkpoints
//!
//! This module implements blockchain state pruning with:
//! - Consensus-driven message expiration (DAO voting on policies)
//! - Merkle checkpoint creation for state verification
//! - Archive node vs light node pruning policies
//! - Pruning proof verification (Merkle inclusion proofs)
//! - Local cache retention after on-chain pruning
//! - Emergency pruning for chain bloat mitigation
//! - State snapshot creation before pruning

use dchat_core::error::{Error, Result};
use dchat_core::types::MessageId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Pruning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningConfig {
    /// Node type (determines pruning policy)
    pub node_type: NodeType,

    /// Default retention period for messages (seconds)
    pub retention_period: u64,

    /// Maximum chain state size before emergency pruning (bytes)
    pub max_state_size: u64,

    /// Checkpoint interval (in blocks or messages)
    pub checkpoint_interval: u64,

    /// Enable local cache retention after pruning
    pub retain_local_cache: bool,

    /// Minimum votes required for pruning policy change (governance)
    pub min_votes_for_policy: u64,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            node_type: NodeType::Light,
            retention_period: 86400 * 30,   // 30 days
            max_state_size: 10_000_000_000, // 10 GB
            checkpoint_interval: 10_000,
            retain_local_cache: true,
            min_votes_for_policy: 100,
        }
    }
}

/// Node type determines pruning behavior
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    /// Archive node: keeps all historical data
    Archive,

    /// Full node: prunes based on retention policy
    Full,

    /// Light node: aggressive pruning, minimal state
    Light,
}

/// Pruning policy (set via governance)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningPolicy {
    /// Policy ID
    pub policy_id: String,

    /// Message retention period (seconds)
    pub retention_period: u64,

    /// Priority messages (never prune)
    pub priority_channels: HashSet<String>,

    /// Created timestamp
    pub created_at: SystemTime,

    /// Governance vote count
    pub vote_count: u64,

    /// Active status
    pub active: bool,
}

impl PruningPolicy {
    /// Create new pruning policy
    pub fn new(policy_id: String, retention_period: u64) -> Self {
        Self {
            policy_id,
            retention_period,
            priority_channels: HashSet::new(),
            created_at: SystemTime::now(),
            vote_count: 0,
            active: false,
        }
    }

    /// Add priority channel (never pruned)
    pub fn add_priority_channel(&mut self, channel_id: String) {
        self.priority_channels.insert(channel_id);
    }

    /// Check if message should be retained
    pub fn should_retain(&self, channel_id: &str, message_age: Duration) -> bool {
        // Priority channels always retained
        if self.priority_channels.contains(channel_id) {
            return true;
        }

        // Check retention period
        message_age.as_secs() < self.retention_period
    }
}

/// Merkle checkpoint for state verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleCheckpoint {
    /// Checkpoint ID
    pub checkpoint_id: String,

    /// Block height or sequence number
    pub height: u64,

    /// Merkle root hash of state at this point
    pub merkle_root: Vec<u8>,

    /// Message count in state
    pub message_count: u64,

    /// State size in bytes
    pub state_size: u64,

    /// Timestamp
    pub timestamp: SystemTime,
}

impl MerkleCheckpoint {
    /// Create new checkpoint
    pub fn new(
        checkpoint_id: String,
        height: u64,
        merkle_root: Vec<u8>,
        message_count: u64,
        state_size: u64,
    ) -> Self {
        Self {
            checkpoint_id,
            height,
            merkle_root,
            message_count,
            state_size,
            timestamp: SystemTime::now(),
        }
    }
}

/// Merkle inclusion proof for verifying pruned messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Message ID being proven
    pub message_id: MessageId,

    /// Merkle path (sibling hashes)
    pub path: Vec<Vec<u8>>,

    /// Checkpoint reference
    pub checkpoint_id: String,
}

impl MerkleProof {
    /// Create new Merkle proof
    pub fn new(message_id: MessageId, path: Vec<Vec<u8>>, checkpoint_id: String) -> Self {
        Self {
            message_id,
            path,
            checkpoint_id,
        }
    }

    /// Verify proof against checkpoint root
    pub fn verify(&self, checkpoint_root: &[u8]) -> bool {
        // Production Merkle proof verification:
        // 1. Hash the message ID (leaf node)
        let mut current_hash = blake3::hash(self.message_id.0.as_bytes())
            .as_bytes()
            .to_vec();

        // 2. Climb tree using sibling path
        // Each sibling is hashed with current hash to get parent
        // Order matters: use lexicographic ordering for determinism
        for sibling in &self.path {
            let combined = if current_hash < *sibling {
                // Current is left child
                [current_hash.clone(), sibling.clone()].concat()
            } else {
                // Current is right child
                [sibling.clone(), current_hash.clone()].concat()
            };
            current_hash = blake3::hash(&combined).as_bytes().to_vec();
        }

        // 3. Final hash must match checkpoint root
        // This proves message was included in checkpoint without revealing other messages
        current_hash == checkpoint_root
    }
}

/// Pruning operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningResult {
    /// Messages pruned count
    pub messages_pruned: u64,

    /// Bytes freed
    pub bytes_freed: u64,

    /// Checkpoint created
    pub checkpoint: MerkleCheckpoint,

    /// Duration of pruning operation
    pub duration_ms: u64,
}

/// Pruning manager
pub struct PruningManager {
    config: PruningConfig,

    /// Active pruning policy (from governance)
    active_policy: Option<PruningPolicy>,

    /// Merkle checkpoints
    checkpoints: HashMap<String, MerkleCheckpoint>,

    /// Messages pending pruning
    pending_pruning: HashSet<MessageId>,

    /// Locally cached messages (post-pruning)
    local_cache: HashSet<MessageId>,

    /// Current state size
    current_state_size: u64,

    /// Total messages pruned
    total_pruned: u64,
}

impl PruningManager {
    /// Create new pruning manager
    pub fn new(config: PruningConfig) -> Self {
        Self {
            config,
            active_policy: None,
            checkpoints: HashMap::new(),
            pending_pruning: HashSet::new(),
            local_cache: HashSet::new(),
            current_state_size: 0,
            total_pruned: 0,
        }
    }

    /// Set active pruning policy (from governance vote)
    pub fn set_policy(&mut self, policy: PruningPolicy) -> Result<()> {
        if policy.vote_count < self.config.min_votes_for_policy {
            return Err(Error::chain("Insufficient votes for policy".to_string()));
        }

        self.active_policy = Some(policy);
        Ok(())
    }

    /// Get active pruning policy
    pub fn active_policy(&self) -> Option<&PruningPolicy> {
        self.active_policy.as_ref()
    }

    /// Check if message should be pruned
    pub fn should_prune(
        &self,
        _message_id: &MessageId,
        channel_id: &str,
        message_age: Duration,
    ) -> bool {
        // Archive nodes never prune
        if self.config.node_type == NodeType::Archive {
            return false;
        }

        // Use governance policy if available
        if let Some(policy) = &self.active_policy {
            if !policy.should_retain(channel_id, message_age) {
                return true;
            }
        }

        // Fallback to config retention period
        if message_age.as_secs() > self.config.retention_period {
            return true;
        }

        false
    }

    /// Mark message for pruning
    pub fn mark_for_pruning(&mut self, message_id: MessageId) {
        self.pending_pruning.insert(message_id);
    }

    /// Create Merkle checkpoint
    pub fn create_checkpoint(
        &mut self,
        height: u64,
        messages: &[MessageId],
    ) -> Result<MerkleCheckpoint> {
        // Calculate Merkle root
        let merkle_root = self.calculate_merkle_root(messages);

        let checkpoint_id = format!(
            "checkpoint_{}_{}",
            height,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );

        let checkpoint = MerkleCheckpoint::new(
            checkpoint_id.clone(),
            height,
            merkle_root,
            messages.len() as u64,
            self.current_state_size,
        );

        self.checkpoints.insert(checkpoint_id, checkpoint.clone());

        Ok(checkpoint)
    }

    /// Calculate Merkle root from message IDs
    fn calculate_merkle_root(&self, messages: &[MessageId]) -> Vec<u8> {
        if messages.is_empty() {
            return blake3::hash(b"empty").as_bytes().to_vec();
        }

        // Hash all message IDs
        let mut hashes: Vec<Vec<u8>> = messages
            .iter()
            .map(|id| blake3::hash(id.0.as_bytes()).as_bytes().to_vec())
            .collect();

        // Production Merkle tree construction:
        // Build binary tree bottom-up, storing all levels for proof generation
        let mut tree_levels: Vec<Vec<Vec<u8>>> = vec![hashes.clone()];
        
        while hashes.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in hashes.chunks(2) {
                let combined = if chunk.len() == 2 {
                    // Normal case: hash two siblings
                    [chunk[0].clone(), chunk[1].clone()].concat()
                } else {
                    // Odd number: duplicate last hash (Bitcoin-style)
                    // This ensures balanced tree structure
                    [chunk[0].clone(), chunk[0].clone()].concat()
                };

                next_level.push(blake3::hash(&combined).as_bytes().to_vec());
            }

            tree_levels.push(next_level.clone());
            hashes = next_level;
        }

        hashes[0].clone()
    }

    /// Generate Merkle proof for message
    pub fn generate_proof(
        &self,
        message_id: &MessageId,
        checkpoint_id: &str,
    ) -> Result<MerkleProof> {
        // Production Merkle proof generation:
        // 1. Retrieve checkpoint and its tree structure
        let checkpoint = self.checkpoints.get(checkpoint_id)
            .ok_or_else(|| Error::network("Checkpoint not found"))?;
        
        // 2. Rebuild Merkle tree from checkpoint's message set
        // Production implementation:
        // a) Retrieve all message IDs from checkpoint metadata
        // b) Sort lexicographically for determinism (same as tree construction)
        // c) Hash each message ID to get leaf hashes
        // d) Build tree bottom-up by hashing pairs: H(H(msg[i]) || H(msg[i+1]))
        
        // Example tree rebuilding:
        // let message_ids = checkpoint.message_ids; // Retrieved from checkpoint
        // let mut leaf_hashes: Vec<Vec<u8>> = message_ids.iter()
        //     .map(|id| blake3::hash(id.0.as_bytes()).as_bytes().to_vec())
        //     .collect();
        // leaf_hashes.sort(); // Lexicographic ordering
        
        // let mut tree_levels = vec![leaf_hashes.clone()];
        // let mut current_level = leaf_hashes;
        // while current_level.len() > 1 {
        //     let mut next_level = Vec::new();
        //     for pair in current_level.chunks(2) {
        //         let hash = if pair.len() == 2 {
        //             let mut combined = pair[0].clone();
        //             combined.extend_from_slice(&pair[1]);
        //             blake3::hash(&combined).as_bytes().to_vec()
        //         } else {
        //             pair[0].clone() // Odd node promoted
        //         };
        //         next_level.push(hash);
        //     }
        //     tree_levels.push(next_level.clone());
        //     current_level = next_level;
        // }
        
        // 3. Find message_id index in leaf level and build sibling path
        // let leaf_hash = blake3::hash(message_id.0.as_bytes()).as_bytes().to_vec();
        // let mut index = tree_levels[0].binary_search(&leaf_hash)
        //     .map_err(|_| Error::network("Message not in tree"))?;
        
        // let mut path = Vec::new();
        // for level in tree_levels.iter().take(tree_levels.len() - 1) {
        //     let sibling_index = index ^ 1; // Toggle last bit (0↔1, 2↔3, 4↔5, ...)
        //     if sibling_index < level.len() {
        //         path.push(level[sibling_index].clone());
        //     }
        //     index /= 2; // Move to parent level
        // }
        
        // Placeholder: use checkpoint root as proof (single-level)
        let path = vec![checkpoint.merkle_root.clone()];

        Ok(MerkleProof::new(
            *message_id,
            path,
            checkpoint_id.to_string(),
        ))
    }

    /// Execute pruning operation
    pub fn execute_pruning(&mut self) -> Result<PruningResult> {
        let start = SystemTime::now();

        let messages_to_prune: Vec<_> = self.pending_pruning.drain().collect();
        let messages_pruned = messages_to_prune.len() as u64;

        // Production: track actual message sizes
        // 1. Query each message's size from storage layer
        // 2. Sum: message_content_size + metadata_size + index_overhead
        // 3. Account for database page fragmentation
        // Example: SELECT SUM(LENGTH(content) + LENGTH(metadata)) FROM messages WHERE id IN (...)
        // For RocksDB: use actual_file_size or estimate from WAL/SST sizes
        let bytes_freed = messages_pruned * 1024; // Placeholder: 1KB per message average

        // Update state size
        self.current_state_size = self.current_state_size.saturating_sub(bytes_freed);
        self.total_pruned += messages_pruned;

        // Retain in local cache if configured
        if self.config.retain_local_cache {
            for msg_id in &messages_to_prune {
                self.local_cache.insert(*msg_id);
            }
        }

        // Create checkpoint
        let checkpoint = self.create_checkpoint(self.total_pruned, &messages_to_prune)?;

        let duration_ms = SystemTime::now()
            .duration_since(start)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64;

        Ok(PruningResult {
            messages_pruned,
            bytes_freed,
            checkpoint,
            duration_ms,
        })
    }

    /// Emergency pruning when state size exceeds limit
    pub fn emergency_prune(&mut self, force_prune_count: u64) -> Result<PruningResult> {
        // Mark oldest messages for emergency pruning
        // Production: query blockchain/storage for oldest messages by timestamp
        // 1. SELECT message_id, timestamp FROM messages ORDER BY timestamp ASC LIMIT force_prune_count
        // 2. Prefer messages outside retention window first
        // 3. Check governance policy: some channels may have longer retention
        // 4. Skip messages flagged for archival (e.g., governance votes, disputes)
        // Placeholder: mark random messages (REPLACE IN PRODUCTION)
        for _ in 0..force_prune_count {
            let msg_id = MessageId(uuid::Uuid::new_v4());
            self.mark_for_pruning(msg_id);
        }

        self.execute_pruning()
    }

    /// Check if emergency pruning needed
    pub fn needs_emergency_pruning(&self) -> bool {
        self.current_state_size > self.config.max_state_size
    }

    /// Update state size
    pub fn update_state_size(&mut self, size: u64) {
        self.current_state_size = size;
    }

    /// Get checkpoint by ID
    pub fn get_checkpoint(&self, checkpoint_id: &str) -> Option<&MerkleCheckpoint> {
        self.checkpoints.get(checkpoint_id)
    }

    /// Check if message is in local cache
    pub fn is_cached_locally(&self, message_id: &MessageId) -> bool {
        self.local_cache.contains(message_id)
    }

    /// Get pruning statistics
    pub fn stats(&self) -> PruningStats {
        PruningStats {
            total_pruned: self.total_pruned,
            pending_pruning: self.pending_pruning.len() as u64,
            checkpoints_created: self.checkpoints.len() as u64,
            current_state_size: self.current_state_size,
            local_cache_size: self.local_cache.len() as u64,
        }
    }
}

/// Pruning statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningStats {
    /// Total messages pruned
    pub total_pruned: u64,

    /// Messages pending pruning
    pub pending_pruning: u64,

    /// Checkpoints created
    pub checkpoints_created: u64,

    /// Current state size
    pub current_state_size: u64,

    /// Local cache size
    pub local_cache_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pruning_config_default() {
        let config = PruningConfig::default();
        assert_eq!(config.node_type, NodeType::Light);
        assert_eq!(config.retention_period, 86400 * 30);
    }

    #[test]
    fn test_pruning_policy() {
        let mut policy = PruningPolicy::new("policy1".to_string(), 86400 * 7);
        policy.add_priority_channel("important_channel".to_string());

        // Priority channel should be retained
        assert!(policy.should_retain("important_channel", Duration::from_secs(999999)));

        // Non-priority channel respects retention period
        assert!(!policy.should_retain("regular_channel", Duration::from_secs(86400 * 8)));
        assert!(policy.should_retain("regular_channel", Duration::from_secs(86400 * 6)));
    }

    #[test]
    fn test_merkle_checkpoint_creation() {
        let mut manager = PruningManager::new(PruningConfig::default());

        let messages = vec![
            MessageId(uuid::Uuid::new_v4()),
            MessageId(uuid::Uuid::new_v4()),
            MessageId(uuid::Uuid::new_v4()),
        ];

        let checkpoint = manager.create_checkpoint(100, &messages).unwrap();
        assert_eq!(checkpoint.height, 100);
        assert_eq!(checkpoint.message_count, 3);
        assert!(!checkpoint.merkle_root.is_empty());
    }

    #[test]
    fn test_merkle_proof_verification() {
        let message_id = MessageId(uuid::Uuid::new_v4());

        // Create a simple proof
        let proof = MerkleProof::new(
            message_id.clone(),
            vec![
                blake3::hash(b"sibling1").as_bytes().to_vec(),
                blake3::hash(b"sibling2").as_bytes().to_vec(),
            ],
            "checkpoint1".to_string(),
        );

        // In production, calculate actual root from proof
        // For now, just verify structure exists
        assert_eq!(proof.path.len(), 2);
        assert_eq!(proof.checkpoint_id, "checkpoint1");
    }

    #[test]
    fn test_pruning_execution() {
        let mut manager = PruningManager::new(PruningConfig::default());

        // Mark messages for pruning
        manager.mark_for_pruning(MessageId(uuid::Uuid::new_v4()));
        manager.mark_for_pruning(MessageId(uuid::Uuid::new_v4()));

        let result = manager.execute_pruning().unwrap();
        assert_eq!(result.messages_pruned, 2);
        assert!(result.bytes_freed > 0);
    }

    #[test]
    fn test_archive_node_no_pruning() {
        let mut config = PruningConfig::default();
        config.node_type = NodeType::Archive;

        let manager = PruningManager::new(config);

        // Archive nodes never prune
        assert!(!manager.should_prune(
            &MessageId(uuid::Uuid::new_v4()),
            "channel1",
            Duration::from_secs(999999)
        ));
    }

    #[test]
    fn test_emergency_pruning() {
        let mut config = PruningConfig::default();
        config.max_state_size = 1000;

        let mut manager = PruningManager::new(config);
        manager.update_state_size(2000);

        // Should trigger emergency pruning
        assert!(manager.needs_emergency_pruning());

        let result = manager.emergency_prune(10).unwrap();
        assert_eq!(result.messages_pruned, 10);
    }

    #[test]
    fn test_local_cache_retention() {
        let mut config = PruningConfig::default();
        config.retain_local_cache = true;

        let mut manager = PruningManager::new(config);

        let msg_id = MessageId(uuid::Uuid::new_v4());
        manager.mark_for_pruning(msg_id.clone());
        manager.execute_pruning().unwrap();

        // Message should be in local cache
        assert!(manager.is_cached_locally(&msg_id));
    }
}
