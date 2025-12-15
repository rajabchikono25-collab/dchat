//! Erasure Coding for Block Dissemination
//!
//! Implements erasure coding at the block boundary for parallel dissemination
//! and resilience against partial withholding/outages.

use super::{BlockError, Hash};
use crate::canonical;
use reed_solomon_erasure::galois_8::ReedSolomon;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Default data shards for erasure coding
pub const DEFAULT_DATA_SHARDS: usize = 4;
/// Default parity shards for erasure coding
pub const DEFAULT_PARITY_SHARDS: usize = 2;
/// Minimum shard size in bytes
pub const MIN_SHARD_SIZE: usize = 64;
/// Maximum total shards
pub const MAX_TOTAL_SHARDS: usize = 256;

// ─────────────────────────────────────────────────────────────────────────────
// Erasure Encoder
// ─────────────────────────────────────────────────────────────────────────────

/// Erasure encoder for block data
pub struct ErasureEncoder {
    data_shards: usize,
    parity_shards: usize,
    rs: ReedSolomon,
}

impl ErasureEncoder {
    /// Create new encoder with specified parameters
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self, BlockError> {
        if data_shards == 0 || parity_shards == 0 {
            return Err(BlockError::ErasureCoding(
                "shards must be non-zero".to_string(),
            ));
        }
        if data_shards + parity_shards > MAX_TOTAL_SHARDS {
            return Err(BlockError::ErasureCoding(format!(
                "total shards {} exceeds max {}",
                data_shards + parity_shards,
                MAX_TOTAL_SHARDS
            )));
        }

        let rs = ReedSolomon::new(data_shards, parity_shards)
            .map_err(|e| BlockError::ErasureCoding(e.to_string()))?;

        Ok(Self {
            data_shards,
            parity_shards,
            rs,
        })
    }

    /// Create encoder with default parameters
    pub fn default_params() -> Result<Self, BlockError> {
        Self::new(DEFAULT_DATA_SHARDS, DEFAULT_PARITY_SHARDS)
    }

    /// Encode data into shards
    pub fn encode(&self, data: &[u8]) -> Result<EncodedBlock, BlockError> {
        if data.is_empty() {
            return Ok(EncodedBlock {
                shards: vec![],
                shard_size: 0,
                data_shards: self.data_shards,
                parity_shards: self.parity_shards,
                original_size: 0,
                shard_hashes: vec![],
            });
        }

        // Calculate shard size (must be equal for all shards)
        let shard_size = (data.len() + self.data_shards - 1) / self.data_shards;
        let shard_size = shard_size.max(MIN_SHARD_SIZE);

        // Pad data to multiple of data_shards * shard_size
        let padded_size = shard_size * self.data_shards;
        let mut padded_data = data.to_vec();
        padded_data.resize(padded_size, 0);

        // Split into data shards
        let mut shards: Vec<Vec<u8>> = padded_data.chunks(shard_size).map(|c| c.to_vec()).collect();

        // Add parity shards
        for _ in 0..self.parity_shards {
            shards.push(vec![0u8; shard_size]);
        }

        // Encode parity
        self.rs
            .encode(&mut shards)
            .map_err(|e| BlockError::ErasureCoding(e.to_string()))?;

        // Hash all shards
        let shard_hashes: Vec<Hash> = shards
            .iter()
            .enumerate()
            .map(|(i, shard)| {
                canonical::domain_hash_parts(
                    b"dchat/erasure/shard/v1",
                    &[&(i as u32).to_le_bytes(), shard],
                )
            })
            .collect();

        Ok(EncodedBlock {
            shards,
            shard_size,
            data_shards: self.data_shards,
            parity_shards: self.parity_shards,
            original_size: data.len(),
            shard_hashes,
        })
    }

    /// Decode data from shards (handles missing shards)
    pub fn decode(&self, encoded: &mut EncodedBlock) -> Result<Vec<u8>, BlockError> {
        if encoded.shards.is_empty() {
            return Ok(vec![]);
        }

        // Check we have enough shards
        let present_count = encoded.shards.iter().filter(|s| !s.is_empty()).count();

        if present_count < self.data_shards {
            return Err(BlockError::ErasureCoding(format!(
                "insufficient shards: {} < {}",
                present_count, self.data_shards
            )));
        }

        // Convert to option refs for reconstruction
        let mut shard_opts: Vec<Option<Vec<u8>>> = encoded
            .shards
            .iter()
            .map(|s| if s.is_empty() { None } else { Some(s.clone()) })
            .collect();

        // Reconstruct if needed
        if present_count < encoded.shards.len() {
            self.rs
                .reconstruct(&mut shard_opts)
                .map_err(|e| BlockError::ErasureCoding(e.to_string()))?;
        }

        // Extract data shards
        let mut data = Vec::with_capacity(encoded.original_size);
        for i in 0..self.data_shards {
            if let Some(ref shard) = shard_opts[i] {
                data.extend_from_slice(shard);
            }
        }

        // Truncate to original size
        data.truncate(encoded.original_size);

        Ok(data)
    }

    /// Get data shard count
    pub fn data_shards(&self) -> usize {
        self.data_shards
    }

    /// Get parity shard count
    pub fn parity_shards(&self) -> usize {
        self.parity_shards
    }

    /// Get total shard count
    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Encoded Block
// ─────────────────────────────────────────────────────────────────────────────

/// Erasure-encoded block data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedBlock {
    /// All shards (data + parity)
    pub shards: Vec<Vec<u8>>,
    /// Size of each shard
    pub shard_size: usize,
    /// Number of data shards
    pub data_shards: usize,
    /// Number of parity shards
    pub parity_shards: usize,
    /// Original data size before encoding
    pub original_size: usize,
    /// Hash of each shard for verification
    pub shard_hashes: Vec<Hash>,
}

impl EncodedBlock {
    /// Verify shard integrity
    pub fn verify_shard(&self, index: usize) -> Result<bool, BlockError> {
        if index >= self.shards.len() {
            return Err(BlockError::ErasureCoding(format!(
                "shard index {} out of bounds",
                index
            )));
        }

        let expected_hash = &self.shard_hashes[index];
        let actual_hash = canonical::domain_hash_parts(
            b"dchat/erasure/shard/v1",
            &[&(index as u32).to_le_bytes(), &self.shards[index]],
        );

        Ok(*expected_hash == actual_hash)
    }

    /// Verify all shards
    pub fn verify_all_shards(&self) -> Result<Vec<bool>, BlockError> {
        let mut results = Vec::with_capacity(self.shards.len());
        for i in 0..self.shards.len() {
            results.push(self.verify_shard(i)?);
        }
        Ok(results)
    }

    /// Mark shard as missing (for reconstruction)
    pub fn mark_missing(&mut self, index: usize) {
        if index < self.shards.len() {
            self.shards[index].clear();
        }
    }

    /// Count present shards
    pub fn present_count(&self) -> usize {
        self.shards.iter().filter(|s| !s.is_empty()).count()
    }

    /// Can data be reconstructed?
    pub fn can_reconstruct(&self) -> bool {
        self.present_count() >= self.data_shards
    }

    /// Get merkle root of all shard hashes
    pub fn merkle_root(&self) -> Hash {
        crate::hash_merkle::merkle_root(&self.shard_hashes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shard Dissemination
// ─────────────────────────────────────────────────────────────────────────────

/// Individual shard for network dissemination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisseminationShard {
    /// Block height
    pub block_height: u64,
    /// Shard index
    pub index: u32,
    /// Shard data
    pub data: Vec<u8>,
    /// Shard hash
    pub hash: Hash,
    /// Is this a parity shard?
    pub is_parity: bool,
    /// Total shards in this encoding
    pub total_shards: u32,
    /// Data shards count
    pub data_shards: u32,
    /// Original data size
    pub original_size: u64,
}

impl DisseminationShard {
    /// Verify shard hash
    pub fn verify_hash(&self) -> bool {
        let expected = canonical::domain_hash_parts(
            b"dchat/erasure/shard/v1",
            &[&self.index.to_le_bytes(), &self.data],
        );
        expected == self.hash
    }
}

/// Create dissemination shards from encoded block
pub fn create_dissemination_shards(
    block_height: u64,
    encoded: &EncodedBlock,
) -> Vec<DisseminationShard> {
    encoded
        .shards
        .iter()
        .enumerate()
        .map(|(i, shard)| DisseminationShard {
            block_height,
            index: i as u32,
            data: shard.clone(),
            hash: encoded.shard_hashes[i],
            is_parity: i >= encoded.data_shards,
            total_shards: encoded.shards.len() as u32,
            data_shards: encoded.data_shards as u32,
            original_size: encoded.original_size as u64,
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Parallel Dissemination
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for parallel shard dissemination
#[derive(Debug, Clone)]
pub struct DisseminationConfig {
    /// Maximum shards to send per peer
    pub shards_per_peer: usize,
    /// Target peer count for full coverage
    pub target_peers: usize,
    /// Timeout for shard requests (ms)
    pub request_timeout_ms: u64,
}

impl Default for DisseminationConfig {
    fn default() -> Self {
        Self {
            shards_per_peer: 2,
            target_peers: 10,
            request_timeout_ms: 1000,
        }
    }
}

/// Shard assignment for a peer
#[derive(Debug, Clone)]
pub struct PeerShardAssignment {
    pub peer_id: Vec<u8>,
    pub shard_indices: Vec<u32>,
}

/// Create peer shard assignments for dissemination
///
/// Ensures each shard is assigned to at least `redundancy` peers.
pub fn create_peer_assignments(
    total_shards: usize,
    peers: &[Vec<u8>],
    redundancy: usize,
) -> Vec<PeerShardAssignment> {
    if peers.is_empty() || total_shards == 0 {
        return vec![];
    }

    let mut assignments: Vec<PeerShardAssignment> = peers
        .iter()
        .map(|p| PeerShardAssignment {
            peer_id: p.clone(),
            shard_indices: vec![],
        })
        .collect();

    // Assign each shard to `redundancy` peers using round-robin
    for shard_idx in 0..total_shards {
        for r in 0..redundancy {
            let peer_idx = (shard_idx + r * total_shards) % peers.len();
            assignments[peer_idx].shard_indices.push(shard_idx as u32);
        }
    }

    assignments
}

// ─────────────────────────────────────────────────────────────────────────────
// Shard Collector
// ─────────────────────────────────────────────────────────────────────────────

/// Collects shards for block reconstruction
pub struct ShardCollector {
    block_height: u64,
    expected_shards: usize,
    data_shards: usize,
    original_size: usize,
    collected: Vec<Option<Vec<u8>>>,
    hashes: Vec<Option<Hash>>,
}

impl ShardCollector {
    /// Create new collector
    pub fn new(
        block_height: u64,
        total_shards: usize,
        data_shards: usize,
        original_size: usize,
    ) -> Self {
        Self {
            block_height,
            expected_shards: total_shards,
            data_shards,
            original_size,
            collected: vec![None; total_shards],
            hashes: vec![None; total_shards],
        }
    }

    /// Add a received shard
    pub fn add_shard(&mut self, shard: DisseminationShard) -> Result<bool, BlockError> {
        if shard.block_height != self.block_height {
            return Err(BlockError::ErasureCoding("wrong block height".to_string()));
        }

        let idx = shard.index as usize;
        if idx >= self.expected_shards {
            return Err(BlockError::ErasureCoding(format!(
                "shard index {} out of bounds",
                idx
            )));
        }

        // Verify shard hash
        if !shard.verify_hash() {
            return Err(BlockError::ErasureCoding(
                "shard hash verification failed".to_string(),
            ));
        }

        self.collected[idx] = Some(shard.data);
        self.hashes[idx] = Some(shard.hash);

        Ok(self.can_reconstruct())
    }

    /// Check if we have enough shards to reconstruct
    pub fn can_reconstruct(&self) -> bool {
        let present = self.collected.iter().filter(|s| s.is_some()).count();
        present >= self.data_shards
    }

    /// Get number of collected shards
    pub fn collected_count(&self) -> usize {
        self.collected.iter().filter(|s| s.is_some()).count()
    }

    /// Reconstruct original data
    pub fn reconstruct(&self) -> Result<Vec<u8>, BlockError> {
        if !self.can_reconstruct() {
            return Err(BlockError::ErasureCoding(format!(
                "insufficient shards: {} < {}",
                self.collected_count(),
                self.data_shards
            )));
        }

        let parity_shards = self.expected_shards - self.data_shards;
        let encoder = ErasureEncoder::new(self.data_shards, parity_shards)?;

        // Build encoded block from collected shards
        let shards: Vec<Vec<u8>> = self
            .collected
            .iter()
            .map(|s| s.clone().unwrap_or_default())
            .collect();

        let shard_hashes: Vec<Hash> = self
            .hashes
            .iter()
            .map(|h| h.unwrap_or(Hash::ZERO))
            .collect();

        let shard_size = shards.first().map(|s| s.len()).unwrap_or(0);

        let mut encoded = EncodedBlock {
            shards,
            shard_size,
            data_shards: self.data_shards,
            parity_shards,
            original_size: self.original_size,
            shard_hashes,
        };

        encoder.decode(&mut encoded)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode() {
        let encoder = ErasureEncoder::default_params().unwrap();
        let data = b"Hello, this is test data for erasure coding!".to_vec();

        let mut encoded = encoder.encode(&data).unwrap();
        assert_eq!(encoded.shards.len(), 6); // 4 data + 2 parity

        let decoded = encoder.decode(&mut encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_reconstruction_with_missing() {
        let encoder = ErasureEncoder::default_params().unwrap();
        let data = b"Test data for reconstruction with missing shards".to_vec();

        let mut encoded = encoder.encode(&data).unwrap();

        // Remove 2 shards (we have 2 parity, so should still work)
        encoded.mark_missing(0);
        encoded.mark_missing(3);

        assert!(encoded.can_reconstruct());
        let decoded = encoder.decode(&mut encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_too_many_missing() {
        let encoder = ErasureEncoder::default_params().unwrap();
        let data = b"Test data".to_vec();

        let mut encoded = encoder.encode(&data).unwrap();

        // Remove 3 shards (we only have 2 parity)
        encoded.mark_missing(0);
        encoded.mark_missing(1);
        encoded.mark_missing(2);

        assert!(!encoded.can_reconstruct());
        assert!(encoder.decode(&mut encoded).is_err());
    }

    #[test]
    fn test_shard_verification() {
        let encoder = ErasureEncoder::default_params().unwrap();
        let data = b"Data for shard verification".to_vec();

        let encoded = encoder.encode(&data).unwrap();

        for i in 0..encoded.shards.len() {
            assert!(encoded.verify_shard(i).unwrap());
        }
    }

    #[test]
    fn test_dissemination_shards() {
        let encoder = ErasureEncoder::default_params().unwrap();
        let data = b"Data for dissemination".to_vec();

        let encoded = encoder.encode(&data).unwrap();
        let shards = create_dissemination_shards(1, &encoded);

        assert_eq!(shards.len(), 6);

        for shard in &shards {
            assert!(shard.verify_hash());
            assert_eq!(shard.block_height, 1);
        }
    }

    #[test]
    fn test_peer_assignments() {
        let peers: Vec<Vec<u8>> = (0..5).map(|i| vec![i as u8]).collect();
        let assignments = create_peer_assignments(6, &peers, 2);

        assert_eq!(assignments.len(), 5);

        // Each shard should be assigned to 2 peers
        let mut shard_counts = vec![0usize; 6];
        for assignment in &assignments {
            for &idx in &assignment.shard_indices {
                shard_counts[idx as usize] += 1;
            }
        }

        for count in shard_counts {
            assert!(count >= 2);
        }
    }

    #[test]
    fn test_shard_collector() {
        let encoder = ErasureEncoder::default_params().unwrap();
        let data = b"Data for collector test".to_vec();

        let encoded = encoder.encode(&data).unwrap();
        let shards = create_dissemination_shards(1, &encoded);

        let mut collector = ShardCollector::new(
            1,
            encoded.shards.len(),
            encoded.data_shards,
            encoded.original_size,
        );

        // Add shards one by one
        for (i, shard) in shards.into_iter().enumerate() {
            let can_reconstruct = collector.add_shard(shard).unwrap();

            // Should be able to reconstruct after data_shards received
            if i >= encoded.data_shards - 1 {
                assert!(can_reconstruct);
            }
        }

        let reconstructed = collector.reconstruct().unwrap();
        assert_eq!(reconstructed, data);
    }
}
