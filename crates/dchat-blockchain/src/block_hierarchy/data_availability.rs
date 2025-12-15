//! Data Availability Commitments
//!
//! Implements block-level data availability commitments (chunk/erasure roots) and
//! data-availability sampling so nodes/light clients can detect withholding without
//! downloading everything.

use super::{BlockError, Hash};
use crate::canonical;
use crate::hash_merkle::{merkle_proof, merkle_root, HashMerkleProof};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Default chunk size for DA (bytes)
pub const DA_CHUNK_SIZE: usize = 512;
/// Default samples per DA check
pub const DA_SAMPLES_PER_CHECK: usize = 16;
/// Maximum chunks per block
pub const MAX_CHUNKS_PER_BLOCK: usize = 65536;
/// Domain separator for chunk hashing
pub const DOMAIN_SEP_DA_CHUNK: &[u8] = b"dchat/da/chunk/v1";
/// Domain separator for shard hashing
pub const DOMAIN_SEP_DA_SHARD: &[u8] = b"dchat/da/shard/v1";

// ─────────────────────────────────────────────────────────────────────────────
// Data Availability Commitment
// ─────────────────────────────────────────────────────────────────────────────

/// Block-level data availability commitment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataAvailabilityCommitment {
    /// Block height this commitment covers
    pub block_height: u64,
    /// Merkle root of original data chunks
    pub chunk_root: Hash,
    /// Merkle root of erasure-coded shards
    pub erasure_root: Hash,
    /// Number of original data chunks
    pub chunk_count: u32,
    /// Number of erasure-coded shards (original + parity)
    pub shard_count: u32,
    /// Chunk size in bytes
    pub chunk_size: u32,
    /// Total data size (before erasure coding)
    pub data_size: u64,
    /// Erasure coding parameters: data shards
    pub erasure_data_shards: u16,
    /// Erasure coding parameters: parity shards
    pub erasure_parity_shards: u16,
    /// Commitment creation timestamp
    pub timestamp: SystemTime,
}

impl DataAvailabilityCommitment {
    /// Create new DA commitment from data
    pub fn from_data(
        block_height: u64,
        data: &[u8],
        chunk_size: usize,
    ) -> Result<Self, BlockError> {
        let chunk_size = chunk_size.max(1);
        let chunks = chunk_data(data, chunk_size);
        let chunk_hashes: Vec<Hash> = chunks
            .iter()
            .enumerate()
            .map(|(i, chunk)| hash_chunk(i, chunk))
            .collect();

        let chunk_root = merkle_root(&chunk_hashes);

        // Erasure coding with Reed-Solomon
        let erasure_data_shards = chunks.len().max(1) as u16;
        let erasure_parity_shards = (erasure_data_shards / 2).max(1);

        let shard_hashes = create_erasure_shard_hashes(&chunks, erasure_parity_shards as usize)?;
        let erasure_root = merkle_root(&shard_hashes);

        Ok(Self {
            block_height,
            chunk_root,
            erasure_root,
            chunk_count: chunks.len() as u32,
            shard_count: shard_hashes.len() as u32,
            chunk_size: chunk_size as u32,
            data_size: data.len() as u64,
            erasure_data_shards,
            erasure_parity_shards,
            timestamp: SystemTime::now(),
        })
    }

    /// Validate commitment parameters
    pub fn validate(&self) -> Result<(), BlockError> {
        if self.chunk_count == 0 && self.data_size > 0 {
            return Err(BlockError::DataAvailability(
                "invalid chunk count for non-empty data".to_string(),
            ));
        }
        if self.chunk_count > MAX_CHUNKS_PER_BLOCK as u32 {
            return Err(BlockError::DataAvailability(format!(
                "chunk count {} exceeds max {}",
                self.chunk_count, MAX_CHUNKS_PER_BLOCK
            )));
        }
        if self.erasure_data_shards == 0 && self.chunk_count > 0 {
            return Err(BlockError::DataAvailability(
                "invalid erasure data shards".to_string(),
            ));
        }
        Ok(())
    }

    /// Compute commitment hash
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/da/commitment/v1", &bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DA Sampling
// ─────────────────────────────────────────────────────────────────────────────

/// Sample request for data availability checking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaSampleRequest {
    /// Block height to sample
    pub block_height: u64,
    /// Chunk indices to sample (derived from randomness)
    pub chunk_indices: Vec<u32>,
    /// Shard indices to sample (for erasure-coded data)
    pub shard_indices: Vec<u32>,
    /// Randomness seed used to derive indices
    pub randomness_seed: Hash,
    /// Request timestamp
    pub timestamp: SystemTime,
}

impl DaSampleRequest {
    /// Generate sample request from randomness
    pub fn from_randomness(
        block_height: u64,
        commitment: &DataAvailabilityCommitment,
        randomness: &[u8],
        sample_count: usize,
    ) -> Self {
        let seed = Hash::from(*blake3::hash(randomness).as_bytes());

        // Deterministically derive chunk indices from seed
        let chunk_indices = derive_sample_indices(&seed, commitment.chunk_count, sample_count);
        let shard_indices = derive_sample_indices(&seed, commitment.shard_count, sample_count);

        Self {
            block_height,
            chunk_indices,
            shard_indices,
            randomness_seed: seed,
            timestamp: SystemTime::now(),
        }
    }
}

/// Sample response with proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaSampleResponse {
    /// Block height
    pub block_height: u64,
    /// Chunk samples with inclusion proofs
    pub chunk_samples: Vec<ChunkSample>,
    /// Shard samples with inclusion proofs
    pub shard_samples: Vec<ShardSample>,
    /// Response timestamp
    pub timestamp: SystemTime,
}

impl DaSampleResponse {
    /// Verify all samples against commitment
    pub fn verify(&self, commitment: &DataAvailabilityCommitment) -> Result<bool, BlockError> {
        // Verify chunk samples
        for sample in &self.chunk_samples {
            if !sample.verify_against_root(commitment.chunk_root)? {
                return Ok(false);
            }
        }

        // Verify shard samples
        for sample in &self.shard_samples {
            if !sample.verify_against_root(commitment.erasure_root)? {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

/// Individual chunk sample with proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkSample {
    /// Chunk index
    pub index: u32,
    /// Chunk data
    pub data: Vec<u8>,
    /// Merkle inclusion proof
    pub proof: HashMerkleProof,
}

impl ChunkSample {
    /// Verify sample against expected root
    pub fn verify_against_root(&self, expected_root: Hash) -> Result<bool, BlockError> {
        let leaf_hash = hash_chunk(self.index as usize, &self.data);
        Ok(self.proof.verify(leaf_hash, expected_root))
    }
}

/// Individual shard sample with proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardSample {
    /// Shard index
    pub index: u32,
    /// Shard data
    pub data: Vec<u8>,
    /// Is this a parity shard?
    pub is_parity: bool,
    /// Merkle inclusion proof
    pub proof: HashMerkleProof,
}

impl ShardSample {
    /// Verify sample against expected root
    pub fn verify_against_root(&self, expected_root: Hash) -> Result<bool, BlockError> {
        let leaf_hash = hash_shard(self.index as usize, &self.data, self.is_parity);
        Ok(self.proof.verify(leaf_hash, expected_root))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DA Sampler
// ─────────────────────────────────────────────────────────────────────────────

/// Data availability sampler for light clients
pub struct DaSampler {
    /// Number of samples per check
    samples_per_check: usize,
    /// Minimum successful samples for availability confirmation
    min_successful_samples: usize,
}

impl DaSampler {
    /// Create new sampler with default parameters
    pub fn new() -> Self {
        Self {
            samples_per_check: DA_SAMPLES_PER_CHECK,
            min_successful_samples: DA_SAMPLES_PER_CHECK * 3 / 4, // 75%
        }
    }

    /// Create sampler with custom parameters
    pub fn with_params(samples_per_check: usize, min_successful: usize) -> Self {
        Self {
            samples_per_check,
            min_successful_samples: min_successful,
        }
    }

    /// Generate sample request
    pub fn create_request(
        &self,
        block_height: u64,
        commitment: &DataAvailabilityCommitment,
        randomness: &[u8],
    ) -> DaSampleRequest {
        DaSampleRequest::from_randomness(
            block_height,
            commitment,
            randomness,
            self.samples_per_check,
        )
    }

    /// Verify sample response
    pub fn verify_samples(
        &self,
        response: &DaSampleResponse,
        commitment: &DataAvailabilityCommitment,
    ) -> Result<DaSamplingResult, BlockError> {
        let mut chunk_successes = 0;
        let mut chunk_failures = 0;

        for sample in &response.chunk_samples {
            if sample.verify_against_root(commitment.chunk_root)? {
                chunk_successes += 1;
            } else {
                chunk_failures += 1;
            }
        }

        let mut shard_successes = 0;
        let mut shard_failures = 0;

        for sample in &response.shard_samples {
            if sample.verify_against_root(commitment.erasure_root)? {
                shard_successes += 1;
            } else {
                shard_failures += 1;
            }
        }

        let total_successes = chunk_successes + shard_successes;
        let available = total_successes >= self.min_successful_samples;

        Ok(DaSamplingResult {
            block_height: response.block_height,
            chunk_successes,
            chunk_failures,
            shard_successes,
            shard_failures,
            available,
            confidence: total_successes as f64 / (self.samples_per_check * 2) as f64,
        })
    }
}

impl Default for DaSampler {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of DA sampling
#[derive(Debug, Clone)]
pub struct DaSamplingResult {
    pub block_height: u64,
    pub chunk_successes: usize,
    pub chunk_failures: usize,
    pub shard_successes: usize,
    pub shard_failures: usize,
    pub available: bool,
    pub confidence: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// DA Provider
// ─────────────────────────────────────────────────────────────────────────────

/// Provides DA samples on request
pub struct DaProvider {
    /// Original data chunks
    chunks: Vec<Vec<u8>>,
    /// Erasure-coded shards
    shards: Vec<Vec<u8>>,
    /// Chunk hashes for proof generation
    chunk_hashes: Vec<Hash>,
    /// Shard hashes for proof generation
    shard_hashes: Vec<Hash>,
    /// Block height
    block_height: u64,
}

impl DaProvider {
    /// Create provider from block data
    pub fn from_data(
        block_height: u64,
        data: &[u8],
        chunk_size: usize,
        parity_shards: usize,
    ) -> Result<Self, BlockError> {
        let chunks = chunk_data(data, chunk_size);
        let chunk_hashes: Vec<Hash> = chunks
            .iter()
            .enumerate()
            .map(|(i, chunk)| hash_chunk(i, chunk))
            .collect();

        let (shards, shard_hashes) = create_erasure_shards(&chunks, parity_shards)?;

        Ok(Self {
            chunks,
            shards,
            chunk_hashes,
            shard_hashes,
            block_height,
        })
    }

    /// Generate sample response for request
    pub fn respond(&self, request: &DaSampleRequest) -> Result<DaSampleResponse, BlockError> {
        let mut chunk_samples = Vec::with_capacity(request.chunk_indices.len());
        for &idx in &request.chunk_indices {
            let idx = idx as usize;
            if idx >= self.chunks.len() {
                continue;
            }
            let (leaf, proof) = merkle_proof(&self.chunk_hashes, idx)
                .ok_or_else(|| BlockError::DataAvailability("chunk proof failed".to_string()))?;

            // Verify leaf matches stored hash (security: prevent proof manipulation)
            if leaf != self.chunk_hashes[idx] {
                return Err(BlockError::DataAvailability(
                    "chunk leaf hash mismatch - possible proof manipulation".to_string(),
                ));
            }

            chunk_samples.push(ChunkSample {
                index: idx as u32,
                data: self.chunks[idx].clone(),
                proof,
            });
        }

        let mut shard_samples = Vec::with_capacity(request.shard_indices.len());
        for &idx in &request.shard_indices {
            let idx = idx as usize;
            if idx >= self.shards.len() {
                continue;
            }
            let (leaf, proof) = merkle_proof(&self.shard_hashes, idx)
                .ok_or_else(|| BlockError::DataAvailability("shard proof failed".to_string()))?;

            // Verify leaf matches stored hash (security: prevent proof manipulation)
            if leaf != self.shard_hashes[idx] {
                return Err(BlockError::DataAvailability(
                    "shard leaf hash mismatch - possible proof manipulation".to_string(),
                ));
            }

            shard_samples.push(ShardSample {
                index: idx as u32,
                data: self.shards[idx].clone(),
                is_parity: idx >= self.chunks.len(),
                proof,
            });
        }

        Ok(DaSampleResponse {
            block_height: self.block_height,
            chunk_samples,
            shard_samples,
            timestamp: SystemTime::now(),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Split data into fixed-size chunks
fn chunk_data(data: &[u8], chunk_size: usize) -> Vec<Vec<u8>> {
    if data.is_empty() {
        return vec![];
    }

    let chunk_size = chunk_size.max(1);
    let mut chunks = Vec::with_capacity((data.len() + chunk_size - 1) / chunk_size);

    for chunk in data.chunks(chunk_size) {
        // Pad last chunk if needed
        let mut padded = chunk.to_vec();
        while padded.len() < chunk_size {
            padded.push(0);
        }
        chunks.push(padded);
    }

    chunks
}

/// Hash a data chunk with domain separation
fn hash_chunk(index: usize, data: &[u8]) -> Hash {
    canonical::domain_hash_parts(DOMAIN_SEP_DA_CHUNK, &[&(index as u64).to_le_bytes(), data])
}

/// Hash an erasure shard with domain separation
fn hash_shard(index: usize, data: &[u8], is_parity: bool) -> Hash {
    canonical::domain_hash_parts(
        DOMAIN_SEP_DA_SHARD,
        &[&(index as u64).to_le_bytes(), &[is_parity as u8], data],
    )
}

/// Derive sample indices from randomness seed
fn derive_sample_indices(seed: &Hash, max_index: u32, count: usize) -> Vec<u32> {
    if max_index == 0 {
        return vec![];
    }

    let mut indices = Vec::with_capacity(count);
    let mut counter = 0u64;

    while indices.len() < count && indices.len() < max_index as usize {
        let mut hasher = blake3::Hasher::new();
        hasher.update(seed.as_bytes());
        hasher.update(&counter.to_le_bytes());
        let hash = hasher.finalize();

        let idx = u32::from_le_bytes(hash.as_bytes()[0..4].try_into().unwrap()) % max_index;
        if !indices.contains(&idx) {
            indices.push(idx);
        }
        counter += 1;

        // Prevent infinite loop
        if counter > max_index as u64 * 10 {
            break;
        }
    }

    indices
}

/// Create erasure-coded shard hashes only
fn create_erasure_shard_hashes(
    chunks: &[Vec<u8>],
    parity_shards: usize,
) -> Result<Vec<Hash>, BlockError> {
    use reed_solomon_erasure::galois_8::ReedSolomon;

    if chunks.is_empty() {
        return Ok(vec![]);
    }

    let data_shards = chunks.len();
    let total_shards = data_shards + parity_shards;

    // All chunks must be same size
    let chunk_size = chunks.first().map(|c| c.len()).unwrap_or(0);

    let rs = ReedSolomon::new(data_shards, parity_shards)
        .map_err(|e| BlockError::ErasureCoding(e.to_string()))?;

    // Create shard array with data + parity
    let mut shards: Vec<Vec<u8>> = chunks.to_vec();
    for _ in 0..parity_shards {
        shards.push(vec![0u8; chunk_size]);
    }

    // Verify shard count matches expected (security: prevent truncation attacks)
    if shards.len() != total_shards {
        return Err(BlockError::ErasureCoding(format!(
            "shard count mismatch: expected {}, got {}",
            total_shards,
            shards.len()
        )));
    }

    // Encode parity
    rs.encode(&mut shards)
        .map_err(|e| BlockError::ErasureCoding(e.to_string()))?;

    // Hash all shards
    let hashes: Vec<Hash> = shards
        .iter()
        .enumerate()
        .map(|(i, shard)| hash_shard(i, shard, i >= data_shards))
        .collect();

    Ok(hashes)
}

/// Create erasure-coded shards with full data
fn create_erasure_shards(
    chunks: &[Vec<u8>],
    parity_shards: usize,
) -> Result<(Vec<Vec<u8>>, Vec<Hash>), BlockError> {
    use reed_solomon_erasure::galois_8::ReedSolomon;

    if chunks.is_empty() {
        return Ok((vec![], vec![]));
    }

    let data_shards = chunks.len();
    let chunk_size = chunks.first().map(|c| c.len()).unwrap_or(0);

    let rs = ReedSolomon::new(data_shards, parity_shards)
        .map_err(|e| BlockError::ErasureCoding(e.to_string()))?;

    // Create shard array with data + parity
    let mut shards: Vec<Vec<u8>> = chunks.to_vec();
    for _ in 0..parity_shards {
        shards.push(vec![0u8; chunk_size]);
    }

    // Encode parity
    rs.encode(&mut shards)
        .map_err(|e| BlockError::ErasureCoding(e.to_string()))?;

    // Hash all shards
    let hashes: Vec<Hash> = shards
        .iter()
        .enumerate()
        .map(|(i, shard)| hash_shard(i, shard, i >= data_shards))
        .collect();

    Ok((shards, hashes))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_da_commitment_creation() {
        let data = vec![0u8; 4096];
        let commitment = DataAvailabilityCommitment::from_data(1, &data, DA_CHUNK_SIZE).unwrap();

        assert_eq!(commitment.block_height, 1);
        assert!(commitment.chunk_count > 0);
        assert!(commitment.shard_count >= commitment.chunk_count);
        assert!(commitment.validate().is_ok());
    }

    #[test]
    fn test_da_sampling() {
        let data = vec![42u8; 8192];
        let commitment = DataAvailabilityCommitment::from_data(1, &data, DA_CHUNK_SIZE).unwrap();

        let provider = DaProvider::from_data(
            1,
            &data,
            DA_CHUNK_SIZE,
            commitment.erasure_parity_shards as usize,
        )
        .unwrap();

        let sampler = DaSampler::new();
        let request = sampler.create_request(1, &commitment, b"test_randomness");
        let response = provider.respond(&request).unwrap();
        let result = sampler.verify_samples(&response, &commitment).unwrap();

        assert!(result.available);
        assert!(result.confidence > 0.5);
    }

    #[test]
    fn test_chunk_data() {
        let data = vec![1u8; 1000];
        let chunks = chunk_data(&data, 256);

        assert_eq!(chunks.len(), 4); // 1000 / 256 = 3.9, rounds up to 4
        assert_eq!(chunks[0].len(), 256);
        assert_eq!(chunks[3].len(), 256); // Padded
    }

    #[test]
    fn test_derive_sample_indices() {
        let seed = Hash::from([1u8; 32]);
        let indices = derive_sample_indices(&seed, 100, 10);

        assert_eq!(indices.len(), 10);
        for idx in &indices {
            assert!(*idx < 100);
        }

        // Should be deterministic
        let indices2 = derive_sample_indices(&seed, 100, 10);
        assert_eq!(indices, indices2);
    }
}
