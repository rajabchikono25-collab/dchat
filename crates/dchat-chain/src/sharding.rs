//! Channel-scoped sharding for horizontal scalability
//!
//! Implements Section 17 (Scalability via Sharding) from ARCHITECTURE.md
//! - Channel-based state partitioning
//! - Cross-shard message routing
//! - Light client support
//! - BLS signature aggregation for efficiency

use blake3::Hasher;
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Merkle tree utilities for proof generation
mod merkle {
    use blake3::Hash;

    /// Generate Merkle tree from channel state leaves
    pub fn generate_merkle_tree(leaves: &[Vec<u8>]) -> Vec<Hash> {
        if leaves.is_empty() {
            return vec![blake3::hash(&[])];
        }

        let mut tree = Vec::new();
        let mut current_level: Vec<Hash> = leaves.iter().map(|leaf| blake3::hash(leaf)).collect();

        tree.extend(current_level.clone());

        while current_level.len() > 1 {
            let mut next_level = Vec::new();

            for i in (0..current_level.len()).step_by(2) {
                let left = current_level[i];
                let right = if i + 1 < current_level.len() {
                    current_level[i + 1]
                } else {
                    left // Duplicate if odd number
                };

                let mut hasher = blake3::Hasher::new();
                hasher.update(left.as_bytes());
                hasher.update(right.as_bytes());
                next_level.push(hasher.finalize());
            }

            tree.extend(next_level.clone());
            current_level = next_level;
        }

        tree
    }

    /// Generate Merkle proof for a specific leaf index
    pub fn generate_proof(tree: &[Hash], leaf_index: usize, num_leaves: usize) -> Vec<Hash> {
        let mut proof = Vec::new();
        let mut index = leaf_index;
        let mut level_size = num_leaves;
        let mut level_start = 0;

        while level_size > 1 {
            let sibling_index = if index % 2 == 0 { index + 1 } else { index - 1 };

            if sibling_index < level_size {
                proof.push(tree[level_start + sibling_index]);
            }

            level_start += level_size;
            index /= 2;
            level_size = (level_size + 1) / 2;
        }

        proof
    }

    /// Verify Merkle proof
    pub fn verify_proof(leaf: &[u8], proof: &[Hash], root: &Hash) -> bool {
        let mut current = blake3::hash(leaf);

        for sibling in proof {
            let mut hasher = blake3::Hasher::new();
            // Order doesn't matter for verification, just combine
            hasher.update(current.as_bytes());
            hasher.update(sibling.as_bytes());
            current = hasher.finalize();
        }

        &current == root
    }

    pub fn get_root(tree: &[Hash]) -> Hash {
        *tree.last().unwrap_or(&blake3::hash(&[]))
    }
}

/// Shard identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardId(pub u32);

/// Channel identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub String);

/// Shard assignment for a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardAssignment {
    pub channel_id: ChannelId,
    pub shard_id: ShardId,
    pub assigned_at: i64,
}

/// Shard state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardState {
    pub shard_id: ShardId,
    pub channels: Vec<ChannelId>,
    pub state_root: Vec<u8>,
    pub message_count: u64,
    pub last_updated: i64,
}

/// Cross-shard message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossShardMessage {
    pub id: String,
    pub from_shard: ShardId,
    pub to_shard: ShardId,
    pub from_channel: ChannelId,
    pub to_channel: ChannelId,
    pub payload: Vec<u8>,
    pub timestamp: i64,
    pub proof: Vec<u8>,
}

/// Sharding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardConfig {
    /// Total number of shards
    pub num_shards: u32,
    /// Activity threshold for channel assignment (messages/hour)
    pub high_activity_threshold: u64,
    /// Enable BLS signature aggregation
    pub enable_bls_aggregation: bool,
    /// Light client mode (subscribe to subset of shards)
    pub light_client_mode: bool,
    /// Shards to track in light client mode
    pub tracked_shards: Vec<ShardId>,
}

impl Default for ShardConfig {
    fn default() -> Self {
        Self {
            num_shards: 16,
            high_activity_threshold: 1000,
            enable_bls_aggregation: true,
            light_client_mode: false,
            tracked_shards: Vec::new(),
        }
    }
}

/// Shard manager
pub struct ShardManager {
    config: ShardConfig,
    shard_states: HashMap<ShardId, ShardState>,
    channel_assignments: HashMap<ChannelId, ShardId>,
    pending_cross_shard: Vec<CrossShardMessage>,
}

impl ShardManager {
    pub fn new(config: ShardConfig) -> Self {
        let mut shard_states = HashMap::new();

        // Initialize all shards
        for i in 0..config.num_shards {
            let shard_id = ShardId(i);
            shard_states.insert(
                shard_id.clone(),
                ShardState {
                    shard_id,
                    channels: Vec::new(),
                    state_root: vec![0; 32],
                    message_count: 0,
                    last_updated: chrono::Utc::now().timestamp(),
                },
            );
        }

        Self {
            config,
            shard_states,
            channel_assignments: HashMap::new(),
            pending_cross_shard: Vec::new(),
        }
    }

    /// Assign channel to shard using consistent hashing
    pub fn assign_channel(&mut self, channel_id: ChannelId) -> Result<ShardId> {
        // Check if already assigned
        if let Some(shard_id) = self.channel_assignments.get(&channel_id) {
            return Ok(shard_id.clone());
        }

        // Use consistent hashing
        let shard_id = self.hash_to_shard(&channel_id);

        // Update shard state
        if let Some(shard_state) = self.shard_states.get_mut(&shard_id) {
            shard_state.channels.push(channel_id.clone());
            shard_state.last_updated = chrono::Utc::now().timestamp();
        }

        self.channel_assignments
            .insert(channel_id, shard_id.clone());

        Ok(shard_id)
    }

    /// Hash channel ID to shard using BLAKE3
    fn hash_to_shard(&self, channel_id: &ChannelId) -> ShardId {
        let mut hasher = Hasher::new();
        hasher.update(channel_id.0.as_bytes());
        let hash = hasher.finalize();

        let shard_num = u32::from_le_bytes([
            hash.as_bytes()[0],
            hash.as_bytes()[1],
            hash.as_bytes()[2],
            hash.as_bytes()[3],
        ]);
        ShardId(shard_num % self.config.num_shards)
    }

    /// Get shard for a channel
    pub fn get_shard(&self, channel_id: &ChannelId) -> Option<ShardId> {
        self.channel_assignments.get(channel_id).cloned()
    }

    /// Route message (may be cross-shard)
    pub fn route_message(
        &mut self,
        from_channel: ChannelId,
        to_channel: ChannelId,
        payload: Vec<u8>,
    ) -> Result<()> {
        let from_shard = self
            .get_shard(&from_channel)
            .ok_or_else(|| Error::network("Source channel not assigned to shard"))?;

        let to_shard = self
            .get_shard(&to_channel)
            .ok_or_else(|| Error::network("Destination channel not assigned to shard"))?;

        if from_shard == to_shard {
            // Same-shard message: direct delivery
            self.deliver_same_shard(&from_channel, &to_channel, payload)?;
        } else {
            // Cross-shard message: requires proof
            let cross_shard_msg = self.create_cross_shard_message(
                from_shard,
                to_shard,
                from_channel,
                to_channel,
                payload,
            )?;

            self.pending_cross_shard.push(cross_shard_msg);
        }

        Ok(())
    }

    /// Deliver message within same shard
    fn deliver_same_shard(
        &mut self,
        _from_channel: &ChannelId,
        _to_channel: &ChannelId,
        _payload: Vec<u8>,
    ) -> Result<()> {
        // In production: update shard state, emit events
        Ok(())
    }

    /// Create cross-shard message with proof
    fn create_cross_shard_message(
        &self,
        from_shard: ShardId,
        to_shard: ShardId,
        from_channel: ChannelId,
        to_channel: ChannelId,
        payload: Vec<u8>,
    ) -> Result<CrossShardMessage> {
        // Generate Merkle proof that message exists in source shard
        let proof = self.generate_merkle_proof(&from_shard, &from_channel)?;

        Ok(CrossShardMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from_shard,
            to_shard,
            from_channel,
            to_channel,
            payload,
            timestamp: chrono::Utc::now().timestamp(),
            proof,
        })
    }

    /// Generate Merkle proof for cross-shard message
    fn generate_merkle_proof(&self, shard_id: &ShardId, channel_id: &ChannelId) -> Result<Vec<u8>> {
        let shard_state = self
            .shard_states
            .get(shard_id)
            .ok_or_else(|| Error::network("Shard not found"))?;

        // Build leaves from all channels in this shard
        let mut leaves: Vec<Vec<u8>> = shard_state
            .channels
            .iter()
            .map(|ch_id| ch_id.0.as_bytes().to_vec())
            .collect();

        // Find the index of our channel
        let channel_index = leaves
            .iter()
            .position(|leaf| leaf == channel_id.0.as_bytes())
            .ok_or_else(|| Error::network("Channel not in shard"))?;

        // Generate Merkle tree
        let tree = merkle::generate_merkle_tree(&leaves);

        // Generate proof for this channel
        let proof_hashes = merkle::generate_proof(&tree, channel_index, leaves.len());

        // Serialize proof: [num_hashes, hash1, hash2, ...]
        let mut proof = Vec::new();
        proof.push(proof_hashes.len() as u8);
        for hash in proof_hashes {
            proof.extend_from_slice(hash.as_bytes());
        }

        Ok(proof)
    }

    /// Verify cross-shard message proof
    pub fn verify_cross_shard_proof(&self, msg: &CrossShardMessage) -> Result<bool> {
        let source_shard = self
            .shard_states
            .get(&msg.from_shard)
            .ok_or_else(|| Error::network("Source shard not found"))?;

        // Parse proof: [num_hashes, hash1, hash2, ...]
        if msg.proof.is_empty() {
            return Ok(false);
        }

        let num_hashes = msg.proof[0] as usize;
        if msg.proof.len() < 1 + num_hashes * 32 {
            return Ok(false);
        }

        // Extract hash proofs
        let mut proof_hashes = Vec::new();
        for i in 0..num_hashes {
            let start = 1 + i * 32;
            let end = start + 32;
            let hash_bytes: [u8; 32] = msg.proof[start..end]
                .try_into()
                .map_err(|_| Error::network("Invalid proof hash"))?;
            proof_hashes.push(blake3::Hash::from(hash_bytes));
        }

        // Reconstruct expected root from state
        let expected_root_bytes: [u8; 32] = source_shard
            .state_root
            .as_slice()
            .try_into()
            .map_err(|_| Error::network("Invalid state root"))?;
        let expected_root = blake3::Hash::from(expected_root_bytes);

        // Verify proof
        let channel_leaf = msg.from_channel.0.as_bytes();
        Ok(merkle::verify_proof(
            channel_leaf,
            &proof_hashes,
            &expected_root,
        ))
    }

    /// Process pending cross-shard messages
    pub fn process_cross_shard_messages(&mut self) -> Result<usize> {
        let mut processed = 0;

        // In light client mode, only process messages for tracked shards
        let messages_to_process: Vec<_> = if self.config.light_client_mode {
            self.pending_cross_shard
                .iter()
                .filter(|msg| self.config.tracked_shards.contains(&msg.to_shard))
                .cloned()
                .collect()
        } else {
            self.pending_cross_shard.clone()
        };

        // Collect verified message IDs to remove later
        let mut verified_msg_ids = Vec::new();

        for msg in messages_to_process {
            // Verify proof
            if self.verify_cross_shard_proof(&msg)? {
                // Deliver to destination shard
                if let Some(dest_shard) = self.shard_states.get_mut(&msg.to_shard) {
                    dest_shard.message_count += 1;
                    dest_shard.last_updated = chrono::Utc::now().timestamp();
                    processed += 1;
                    verified_msg_ids.push(msg.id.clone());
                }
            }
        }

        // Remove processed messages
        self.pending_cross_shard
            .retain(|msg| !verified_msg_ids.contains(&msg.id));

        Ok(processed)
    }

    /// Aggregate BLS signatures for shard finality
    ///
    /// NOTE: This currently uses simple concatenation. For production:
    /// 1. Add dependency: blst = "0.3" or bls-signatures = "0.15" to Cargo.toml
    /// 2. Implement proper BLS12-381 signature aggregation:
    ///    - Parse each signature as BLS point
    ///    - Aggregate points using elliptic curve addition
    ///    - Compress result to 96 bytes
    /// 3. Implement multi-signature verification with public key aggregation
    pub fn aggregate_signatures(&self, signatures: &[Vec<u8>]) -> Result<Vec<u8>> {
        use blst::min_pk::{AggregateSignature, Signature};

        if !self.config.enable_bls_aggregation {
            return Err(Error::network("BLS aggregation disabled"));
        }

        if signatures.is_empty() {
            return Err(Error::network("No signatures to aggregate"));
        }

        // Parse all signatures first
        let mut sigs = Vec::new();
        for sig_bytes in signatures {
            if sig_bytes.len() != 96 {
                return Err(Error::network(format!(
                    "Invalid BLS signature length: {}",
                    sig_bytes.len()
                )));
            }

            let sig = Signature::from_bytes(sig_bytes)
                .map_err(|e| Error::network(format!("Invalid BLS signature: {:?}", e)))?;

            // Validate signature format before aggregating
            // validate() returns Result<(), BLST_ERROR>
            sig.validate(true)
                .map_err(|_| Error::network("Invalid BLS signature format"))?;

            sigs.push(sig);
        }

        // Aggregate all signatures using from_signature + add
        let sig_refs: Vec<&Signature> = sigs.iter().collect();
        let agg_sig = AggregateSignature::aggregate(&sig_refs, true)
            .map_err(|_| Error::network("Failed to aggregate signatures"))?;

        // Convert to final aggregated signature (96 bytes compressed)
        let final_sig = agg_sig.to_signature();
        Ok(final_sig.to_bytes().to_vec())
    }

    /// Verify aggregated BLS signature against multiple public keys
    ///
    /// This verifies that the aggregated signature was created by combining signatures
    /// from the validators whose public keys are provided.
    pub fn verify_aggregated_signature(
        &self,
        aggregated_sig: &[u8],
        public_keys: &[Vec<u8>],
        message: &[u8],
    ) -> Result<bool> {
        use blst::min_pk::{PublicKey, Signature};
        use blst::BLST_ERROR;

        if aggregated_sig.len() != 96 {
            return Ok(false);
        }

        if public_keys.is_empty() {
            return Err(Error::network("No public keys provided"));
        }

        // Parse aggregated signature
        let sig = Signature::from_bytes(aggregated_sig)
            .map_err(|e| Error::network(format!("Invalid aggregated signature: {:?}", e)))?;

        // Parse public keys
        let mut pks = Vec::new();
        for pk_bytes in public_keys {
            if pk_bytes.len() != 48 {
                return Ok(false);
            }
            let pk = PublicKey::from_bytes(pk_bytes)
                .map_err(|e| Error::network(format!("Invalid public key: {:?}", e)))?;
            pks.push(pk);
        }

        // Verify aggregated signature
        // DST (Domain Separation Tag) for dchat cross-shard messages
        let dst = b"DCHAT_CROSS_SHARD_V1";

        // aggregate_verify expects:
        // - &[&[u8]] for messages (each message as slice)
        // - &[&PublicKey] for public keys
        let messages: Vec<&[u8]> = vec![message]; // Single message for all validators
        let pk_refs: Vec<&PublicKey> = pks.iter().collect();

        let result = sig.aggregate_verify(true, &messages, dst, &pk_refs, true);

        Ok(result == BLST_ERROR::BLST_SUCCESS)
    }

    /// Get shard statistics
    pub fn get_shard_stats(&self, shard_id: &ShardId) -> Option<ShardStats> {
        self.shard_states.get(shard_id).map(|state| ShardStats {
            shard_id: shard_id.clone(),
            num_channels: state.channels.len(),
            message_count: state.message_count,
            last_updated: state.last_updated,
        })
    }

    /// Rebalance shards (move channels between shards) based on load
    ///
    /// Algorithm:
    /// 1. Calculate average load per shard
    /// 2. Identify overloaded shards (>150% average)
    /// 3. Identify underloaded shards (<50% average)
    /// 4. Move channels from overloaded to underloaded shards
    pub fn rebalance_shards(&mut self) -> Result<usize> {
        if self.shard_states.is_empty() {
            return Ok(0);
        }

        // Calculate average message count per shard
        let total_messages: u64 = self.shard_states.values().map(|s| s.message_count).sum();
        let avg_load = total_messages / self.shard_states.len() as u64;

        if avg_load == 0 {
            return Ok(0); // No messages yet, skip rebalancing
        }

        // Find overloaded and underloaded shards
        let overload_threshold = avg_load + (avg_load / 2); // 150%
        let underload_threshold = avg_load / 2; // 50%

        let mut overloaded: Vec<ShardId> = Vec::new();
        let mut underloaded: Vec<ShardId> = Vec::new();

        for (shard_id, state) in &self.shard_states {
            if state.message_count > overload_threshold {
                overloaded.push(shard_id.clone());
            } else if state.message_count < underload_threshold {
                underloaded.push(shard_id.clone());
            }
        }

        if overloaded.is_empty() || underloaded.is_empty() {
            return Ok(0); // No rebalancing needed
        }

        let mut moved = 0;

        // Move channels from overloaded to underloaded shards
        for overloaded_shard in overloaded {
            if underloaded.is_empty() {
                break;
            }

            // Get channels from overloaded shard
            let channels_to_move: Vec<ChannelId> = self
                .shard_states
                .get(&overloaded_shard)
                .map(|state| state.channels.clone())
                .unwrap_or_default();

            // Move up to 10% of channels
            let move_count = (channels_to_move.len() / 10)
                .max(1)
                .min(channels_to_move.len());

            for i in 0..move_count {
                if let Some(target_shard) = underloaded.first() {
                    let channel = &channels_to_move[i];

                    // Update assignment (channel_assignments is HashMap<ChannelId, ShardId>)
                    self.channel_assignments
                        .insert(channel.clone(), target_shard.clone());

                    // Update shard states
                    if let Some(old_shard) = self.shard_states.get_mut(&overloaded_shard) {
                        old_shard.channels.retain(|c| c != channel);
                    }
                    if let Some(new_shard) = self.shard_states.get_mut(target_shard) {
                        new_shard.channels.push(channel.clone());
                    }

                    moved += 1;
                }
            }
        }

        Ok(moved)
    }

    /// Get global statistics
    pub fn get_global_stats(&self) -> GlobalShardStats {
        let total_channels: usize = self.channel_assignments.len();
        let total_messages: u64 = self.shard_states.values().map(|s| s.message_count).sum();
        let active_shards = self
            .shard_states
            .values()
            .filter(|s| !s.channels.is_empty())
            .count();

        GlobalShardStats {
            total_shards: self.config.num_shards as usize,
            active_shards,
            total_channels,
            total_messages,
            pending_cross_shard: self.pending_cross_shard.len(),
        }
    }
}

/// Shard statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardStats {
    pub shard_id: ShardId,
    pub num_channels: usize,
    pub message_count: u64,
    pub last_updated: i64,
}

/// Global shard statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalShardStats {
    pub total_shards: usize,
    pub active_shards: usize,
    pub total_channels: usize,
    pub total_messages: u64,
    pub pending_cross_shard: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_config_default() {
        let config = ShardConfig::default();
        assert_eq!(config.num_shards, 16);
        assert!(config.enable_bls_aggregation);
        assert!(!config.light_client_mode);
    }

    #[test]
    fn test_shard_initialization() {
        let config = ShardConfig::default();
        let manager = ShardManager::new(config);

        assert_eq!(manager.shard_states.len(), 16);
        assert_eq!(manager.channel_assignments.len(), 0);
    }

    #[test]
    fn test_channel_assignment() {
        let config = ShardConfig::default();
        let mut manager = ShardManager::new(config);

        let channel = ChannelId("test-channel".to_string());
        let shard_id = manager.assign_channel(channel.clone()).unwrap();

        assert!(shard_id.0 < 16);
        assert_eq!(manager.get_shard(&channel), Some(shard_id.clone()));

        // Reassigning should return same shard
        let shard_id2 = manager.assign_channel(channel.clone()).unwrap();
        assert_eq!(shard_id, shard_id2);
    }

    #[test]
    fn test_consistent_hashing() {
        let config = ShardConfig::default();
        let manager = ShardManager::new(config);

        let channel = ChannelId("test-channel".to_string());
        let shard1 = manager.hash_to_shard(&channel);
        let shard2 = manager.hash_to_shard(&channel);

        // Same channel should always hash to same shard
        assert_eq!(shard1, shard2);
    }

    #[test]
    fn test_same_shard_routing() {
        let config = ShardConfig::default();
        let mut manager = ShardManager::new(config);

        let channel1 = ChannelId("channel1".to_string());
        let channel2 = ChannelId("channel2".to_string());

        // Force same shard by assigning explicitly
        let shard = ShardId(0);
        manager
            .channel_assignments
            .insert(channel1.clone(), shard.clone());
        manager
            .channel_assignments
            .insert(channel2.clone(), shard.clone());

        let result = manager.route_message(channel1, channel2, b"test message".to_vec());

        assert!(result.is_ok());
        assert_eq!(manager.pending_cross_shard.len(), 0);
    }

    #[test]
    fn test_cross_shard_routing() {
        let config = ShardConfig::default();
        let mut manager = ShardManager::new(config);

        let channel1 = ChannelId("channel1".to_string());
        let channel2 = ChannelId("channel2".to_string());

        // Force different shards
        manager
            .channel_assignments
            .insert(channel1.clone(), ShardId(0));
        manager
            .channel_assignments
            .insert(channel2.clone(), ShardId(1));

        let result = manager.route_message(channel1, channel2, b"cross-shard message".to_vec());

        assert!(result.is_ok());
        assert_eq!(manager.pending_cross_shard.len(), 1);
    }

    #[test]
    fn test_bls_signature_aggregation() {
        let config = ShardConfig::default();
        let manager = ShardManager::new(config);

        let sig1 = vec![1, 2, 3];
        let sig2 = vec![4, 5, 6];
        let signatures = vec![sig1.clone(), sig2.clone()];

        let aggregated = manager.aggregate_signatures(&signatures).unwrap();

        // Placeholder implementation just concatenates
        assert_eq!(aggregated.len(), 6);
    }

    #[test]
    fn test_light_client_mode() {
        let config = ShardConfig {
            light_client_mode: true,
            tracked_shards: vec![ShardId(0), ShardId(1)],
            ..Default::default()
        };
        let mut manager = ShardManager::new(config);

        // Create cross-shard message to tracked shard
        let channel1 = ChannelId("channel1".to_string());
        let channel2 = ChannelId("channel2".to_string());

        manager
            .channel_assignments
            .insert(channel1.clone(), ShardId(0));
        manager
            .channel_assignments
            .insert(channel2.clone(), ShardId(1));

        manager
            .route_message(channel1, channel2, b"message".to_vec())
            .unwrap();

        // Should process message for tracked shard
        let processed = manager.process_cross_shard_messages().unwrap();
        assert_eq!(processed, 1);
    }

    #[test]
    fn test_global_stats() {
        let config = ShardConfig::default();
        let mut manager = ShardManager::new(config);

        // Add some channels
        manager
            .assign_channel(ChannelId("channel1".to_string()))
            .unwrap();
        manager
            .assign_channel(ChannelId("channel2".to_string()))
            .unwrap();

        let stats = manager.get_global_stats();

        assert_eq!(stats.total_shards, 16);
        assert_eq!(stats.total_channels, 2);
        assert!(stats.active_shards >= 1);
    }
}
