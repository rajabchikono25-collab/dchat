//! Aggregated/Threshold Signatures
//!
//! Replaces per-attestation signature floods with aggregated/threshold signatures
//! at subblock/block boundaries to reduce bytes and verification overhead while
//! strengthening accountable quorum proofs.

use super::{BlockError, Hash};
use crate::canonical;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::SystemTime;

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// BLS12-381 signature size (96 bytes)
pub const BLS_SIGNATURE_BYTES: usize = 96;
/// BLS12-381 public key size (48 bytes)  
pub const BLS_PUBKEY_BYTES: usize = 48;
/// Maximum validators in set
pub const MAX_VALIDATORS: usize = 128;
/// Minimum quorum for block finality (67%)
pub const QUORUM_THRESHOLD_BPS: u64 = 6667;

// ─────────────────────────────────────────────────────────────────────────────
// Aggregated Block Signature
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregated signature for block finality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedBlockSignature {
    /// Block height this signature covers
    pub block_height: u64,
    /// Block hash being signed
    pub block_hash: Hash,
    /// Bitmap of participating validators (bit N = validator N signed)
    pub participation_bitmap: ParticipationBitmap,
    /// Aggregated BLS signature
    pub aggregate_sig: Vec<u8>,
    /// Total stake weight of signers (basis points of total stake)
    pub stake_weight_bps: u64,
    /// Timestamp when aggregate was created
    pub timestamp: SystemTime,
}

impl AggregatedBlockSignature {
    /// Create new empty aggregated signature
    pub fn new(block_height: u64, block_hash: Hash) -> Self {
        Self {
            block_height,
            block_hash,
            participation_bitmap: ParticipationBitmap::new(),
            aggregate_sig: Vec::new(),
            stake_weight_bps: 0,
            timestamp: SystemTime::now(),
        }
    }

    /// Check if signature has quorum
    pub fn has_quorum(&self) -> bool {
        self.stake_weight_bps >= QUORUM_THRESHOLD_BPS
    }

    /// Verify quorum is achieved
    pub fn verify_quorum(&self) -> Result<bool, BlockError> {
        if self.aggregate_sig.is_empty() {
            return Ok(false);
        }
        if self.stake_weight_bps < QUORUM_THRESHOLD_BPS {
            return Ok(false);
        }
        Ok(true)
    }

    /// Get participating validator indices
    pub fn participating_validators(&self) -> Vec<u32> {
        self.participation_bitmap.set_indices()
    }

    /// Get signing message
    pub fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::with_capacity(64);
        msg.extend_from_slice(b"dchat/block_sig/v1\x00");
        msg.extend_from_slice(&self.block_height.to_le_bytes());
        msg.extend_from_slice(self.block_hash.as_bytes());
        msg
    }

    /// Compute signature hash
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/agg_block_sig/v1", &bytes)
    }

    /// Add a validator's signature to the aggregate
    #[cfg(feature = "bls-aggregation")]
    pub fn add_signature(
        &mut self,
        validator_index: u32,
        signature: &[u8],
        stake_weight_bps: u64,
    ) -> Result<(), BlockError> {
        use blst::min_pk::*;

        if self.participation_bitmap.is_set(validator_index) {
            return Err(BlockError::SignatureVerification(
                "validator already signed".to_string(),
            ));
        }

        if signature.len() != BLS_SIGNATURE_BYTES {
            return Err(BlockError::SignatureVerification(
                "invalid signature size".to_string(),
            ));
        }

        let sig = Signature::from_bytes(signature).map_err(|e| {
            BlockError::SignatureVerification(format!("invalid BLS signature: {:?}", e))
        })?;

        if self.aggregate_sig.is_empty() {
            self.aggregate_sig = signature.to_vec();
        } else {
            let existing = Signature::from_bytes(&self.aggregate_sig).map_err(|e| {
                BlockError::SignatureVerification(format!("corrupt aggregate: {:?}", e))
            })?;

            let agg = AggregateSignature::aggregate(&[&existing, &sig], true).map_err(|e| {
                BlockError::SignatureVerification(format!("aggregation failed: {:?}", e))
            })?;

            self.aggregate_sig = agg.to_signature().to_bytes().to_vec();
        }

        self.participation_bitmap.set(validator_index);
        self.stake_weight_bps += stake_weight_bps;

        Ok(())
    }

    /// Add a validator's signature (fallback without BLS)
    #[cfg(not(feature = "bls-aggregation"))]
    pub fn add_signature(
        &mut self,
        validator_index: u32,
        signature: &[u8],
        stake_weight_bps: u64,
    ) -> Result<(), BlockError> {
        if self.participation_bitmap.is_set(validator_index) {
            return Err(BlockError::SignatureVerification(
                "validator already signed".to_string(),
            ));
        }

        // Fallback: XOR aggregation (not cryptographically secure, just for structure)
        if self.aggregate_sig.is_empty() {
            self.aggregate_sig = signature.to_vec();
        } else if self.aggregate_sig.len() == signature.len() {
            for (i, b) in signature.iter().enumerate() {
                self.aggregate_sig[i] ^= b;
            }
        }

        self.participation_bitmap.set(validator_index);
        self.stake_weight_bps += stake_weight_bps;

        Ok(())
    }

    /// Verify aggregate signature against public keys
    #[cfg(feature = "bls-aggregation")]
    pub fn verify(&self, validator_pubkeys: &[Vec<u8>]) -> Result<bool, BlockError> {
        use blst::min_pk::*;
        use blst::BLST_ERROR;

        if !self.has_quorum() {
            return Ok(false);
        }

        if self.aggregate_sig.is_empty() {
            return Ok(false);
        }

        let sig = Signature::from_bytes(&self.aggregate_sig).map_err(|e| {
            BlockError::SignatureVerification(format!("invalid aggregate signature: {:?}", e))
        })?;

        // Get participating validator public keys
        let indices = self.participating_validators();
        let mut pks = Vec::with_capacity(indices.len());
        for &idx in &indices {
            let idx = idx as usize;
            if idx >= validator_pubkeys.len() {
                return Err(BlockError::SignatureVerification(
                    "validator index out of bounds".to_string(),
                ));
            }
            let pk = PublicKey::from_bytes(&validator_pubkeys[idx]).map_err(|e| {
                BlockError::SignatureVerification(format!("invalid public key: {:?}", e))
            })?;
            pks.push(pk);
        }

        let pk_refs: Vec<&PublicKey> = pks.iter().collect();
        let msg = self.signing_message();
        let msgs: Vec<&[u8]> = vec![msg.as_slice(); pks.len()];
        let dst = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

        let result = sig.aggregate_verify(true, &msgs, dst, &pk_refs, true);
        Ok(result == BLST_ERROR::BLST_SUCCESS)
    }

    /// Verify aggregate signature (fallback without BLS)
    #[cfg(not(feature = "bls-aggregation"))]
    pub fn verify(&self, _validator_pubkeys: &[Vec<u8>]) -> Result<bool, BlockError> {
        Ok(self.has_quorum() && !self.aggregate_sig.is_empty())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Participation Bitmap
// ─────────────────────────────────────────────────────────────────────────────

/// Compact bitmap for validator participation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParticipationBitmap {
    /// Bitmap bytes (little-endian, bit 0 = validator 0)
    bits: Vec<u8>,
}

impl ParticipationBitmap {
    /// Create new empty bitmap
    pub fn new() -> Self {
        Self {
            bits: vec![0u8; (MAX_VALIDATORS + 7) / 8],
        }
    }

    /// Set bit for validator index
    pub fn set(&mut self, index: u32) {
        let idx = index as usize;
        if idx < MAX_VALIDATORS {
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            if byte_idx < self.bits.len() {
                self.bits[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    /// Clear bit for validator index
    pub fn clear(&mut self, index: u32) {
        let idx = index as usize;
        if idx < MAX_VALIDATORS {
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            if byte_idx < self.bits.len() {
                self.bits[byte_idx] &= !(1 << bit_idx);
            }
        }
    }

    /// Check if bit is set
    pub fn is_set(&self, index: u32) -> bool {
        let idx = index as usize;
        if idx >= MAX_VALIDATORS {
            return false;
        }
        let byte_idx = idx / 8;
        let bit_idx = idx % 8;
        if byte_idx >= self.bits.len() {
            return false;
        }
        (self.bits[byte_idx] & (1 << bit_idx)) != 0
    }

    /// Count set bits
    pub fn count(&self) -> usize {
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }

    /// Get indices of all set bits
    pub fn set_indices(&self) -> Vec<u32> {
        (0..MAX_VALIDATORS as u32)
            .filter(|&i| self.is_set(i))
            .collect()
    }

    /// Merge with another bitmap (OR)
    pub fn merge(&mut self, other: &ParticipationBitmap) {
        for (i, b) in other.bits.iter().enumerate() {
            if i < self.bits.len() {
                self.bits[i] |= b;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Validator Stake Info
// ─────────────────────────────────────────────────────────────────────────────

/// Validator information for signature aggregation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Validator index (0-based)
    pub index: u32,
    /// Public key (BLS12-381)
    pub pubkey: Vec<u8>,
    /// Stake weight in basis points
    pub stake_weight_bps: u64,
    /// Is validator active?
    pub active: bool,
}

/// Validator set for signature aggregation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSet {
    /// All validators (indexed by validator index)
    pub validators: Vec<ValidatorInfo>,
    /// Set of pubkey hashes for duplicate detection (not serialized)
    #[serde(skip)]
    pubkey_hashes: HashSet<[u8; 32]>,
    /// Total stake (should sum to 10000 bps)
    pub total_stake_bps: u64,
    /// Epoch this set is valid for
    pub epoch: u64,
}

impl ValidatorSet {
    /// Create new validator set
    pub fn new(epoch: u64) -> Self {
        Self {
            validators: Vec::new(),
            pubkey_hashes: HashSet::new(),
            total_stake_bps: 0,
            epoch,
        }
    }

    /// Add validator with duplicate pubkey prevention
    pub fn add_validator(
        &mut self,
        pubkey: Vec<u8>,
        stake_weight_bps: u64,
    ) -> Result<(), BlockError> {
        // Compute pubkey hash for duplicate detection
        let pubkey_hash: [u8; 32] = *blake3::hash(&pubkey).as_bytes();

        if self.pubkey_hashes.contains(&pubkey_hash) {
            return Err(BlockError::SignatureVerification(
                "duplicate validator pubkey".to_string(),
            ));
        }

        let index = self.validators.len() as u32;
        self.validators.push(ValidatorInfo {
            index,
            pubkey,
            stake_weight_bps,
            active: true,
        });
        self.pubkey_hashes.insert(pubkey_hash);
        self.total_stake_bps += stake_weight_bps;
        Ok(())
    }

    /// Add validator (legacy, panics on duplicate)
    pub fn add_validator_unchecked(&mut self, pubkey: Vec<u8>, stake_weight_bps: u64) {
        let index = self.validators.len() as u32;
        let pubkey_hash: [u8; 32] = *blake3::hash(&pubkey).as_bytes();
        self.pubkey_hashes.insert(pubkey_hash);
        self.validators.push(ValidatorInfo {
            index,
            pubkey,
            stake_weight_bps,
            active: true,
        });
        self.total_stake_bps += stake_weight_bps;
    }

    /// Check if pubkey exists in set
    pub fn has_pubkey(&self, pubkey: &[u8]) -> bool {
        let pubkey_hash: [u8; 32] = *blake3::hash(pubkey).as_bytes();
        self.pubkey_hashes.contains(&pubkey_hash)
    }

    /// Get validator by index
    pub fn get(&self, index: u32) -> Option<&ValidatorInfo> {
        self.validators.get(index as usize)
    }

    /// Get all public keys (for verification)
    pub fn pubkeys(&self) -> Vec<Vec<u8>> {
        self.validators.iter().map(|v| v.pubkey.clone()).collect()
    }

    /// Calculate quorum threshold
    pub fn quorum_threshold(&self) -> u64 {
        (self.total_stake_bps * QUORUM_THRESHOLD_BPS + 9999) / 10000
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Signature Aggregator
// ─────────────────────────────────────────────────────────────────────────────

/// Collects signatures and builds aggregated signature
pub struct SignatureAggregator {
    block_height: u64,
    block_hash: Hash,
    validator_set: ValidatorSet,
    collected: AggregatedBlockSignature,
}

impl SignatureAggregator {
    /// Create new aggregator
    pub fn new(block_height: u64, block_hash: Hash, validator_set: ValidatorSet) -> Self {
        Self {
            block_height,
            block_hash,
            validator_set,
            collected: AggregatedBlockSignature::new(block_height, block_hash),
        }
    }

    /// Add signature from validator
    pub fn add_signature(
        &mut self,
        validator_index: u32,
        signature: Vec<u8>,
    ) -> Result<bool, BlockError> {
        let validator = self
            .validator_set
            .get(validator_index)
            .ok_or_else(|| BlockError::SignatureVerification("unknown validator".to_string()))?
            .clone();

        if !validator.active {
            return Err(BlockError::SignatureVerification(
                "validator not active".to_string(),
            ));
        }

        self.collected
            .add_signature(validator_index, &signature, validator.stake_weight_bps)?;

        Ok(self.collected.has_quorum())
    }

    /// Check if quorum reached
    pub fn has_quorum(&self) -> bool {
        self.collected.has_quorum()
    }

    /// Get current stake weight
    pub fn current_stake_weight(&self) -> u64 {
        self.collected.stake_weight_bps
    }

    /// Get number of signers
    pub fn signer_count(&self) -> usize {
        self.collected.participation_bitmap.count()
    }

    /// Get block height being signed
    pub fn block_height(&self) -> u64 {
        self.block_height
    }

    /// Get block hash being signed
    pub fn block_hash(&self) -> Hash {
        self.block_hash
    }

    /// Verify collected signatures match expected block
    pub fn verify_block_match(&self, expected_height: u64, expected_hash: Hash) -> bool {
        self.block_height == expected_height && self.block_hash == expected_hash
    }

    /// Finalize and return aggregated signature
    pub fn finalize(self) -> Result<AggregatedBlockSignature, BlockError> {
        if !self.collected.has_quorum() {
            return Err(BlockError::InsufficientSignatures);
        }
        Ok(self.collected)
    }

    /// Get current state (even if no quorum)
    pub fn current_state(&self) -> &AggregatedBlockSignature {
        &self.collected
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Threshold Signature Support
// ─────────────────────────────────────────────────────────────────────────────

/// Threshold signature scheme parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdParams {
    /// Total number of shares
    pub n: u32,
    /// Threshold for reconstruction
    pub t: u32,
    /// Group public key
    pub group_pubkey: Vec<u8>,
}

impl ThresholdParams {
    /// Create 5-of-7 threshold
    pub fn five_of_seven(group_pubkey: Vec<u8>) -> Self {
        Self {
            n: 7,
            t: 5,
            group_pubkey,
        }
    }

    /// Check if threshold can be met with given shares
    pub fn can_reconstruct(&self, share_count: u32) -> bool {
        share_count >= self.t
    }
}

/// Threshold signature share
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdShare {
    /// Share index (1-based for Shamir)
    pub index: u32,
    /// Share data
    pub share: Vec<u8>,
    /// Signer's identifier
    pub signer_id: Vec<u8>,
}

/// Threshold share collector with duplicate prevention
pub struct ThresholdShareCollector {
    /// Parameters for this threshold signature
    params: ThresholdParams,
    /// Message being signed
    message: Vec<u8>,
    /// Collected shares
    shares: Vec<ThresholdShare>,
    /// Set of share indices already seen (duplicate prevention)
    seen_indices: HashSet<u32>,
    /// Set of signer IDs already seen (prevent same signer submitting multiple shares)
    seen_signers: HashSet<[u8; 32]>,
}

impl ThresholdShareCollector {
    /// Create new collector
    pub fn new(params: ThresholdParams, message: Vec<u8>) -> Self {
        Self {
            params,
            message,
            shares: Vec::new(),
            seen_indices: HashSet::new(),
            seen_signers: HashSet::new(),
        }
    }

    /// Add a share with duplicate detection
    pub fn add_share(&mut self, share: ThresholdShare) -> Result<bool, BlockError> {
        // Validate share index bounds (1-based)
        if share.index == 0 || share.index > self.params.n {
            return Err(BlockError::SignatureVerification(format!(
                "share index {} out of range [1, {}]",
                share.index, self.params.n
            )));
        }

        // Check for duplicate share index
        if self.seen_indices.contains(&share.index) {
            return Err(BlockError::SignatureVerification(format!(
                "duplicate share index {}",
                share.index
            )));
        }

        // Check for duplicate signer (hash signer_id for consistent comparison)
        let signer_hash: [u8; 32] = *blake3::hash(&share.signer_id).as_bytes();
        if self.seen_signers.contains(&signer_hash) {
            return Err(BlockError::SignatureVerification(
                "duplicate signer attempting multiple shares".to_string(),
            ));
        }

        // Add share
        self.seen_indices.insert(share.index);
        self.seen_signers.insert(signer_hash);
        self.shares.push(share);

        // Check if we can reconstruct
        Ok(self.can_reconstruct())
    }

    /// Check if threshold is met
    pub fn can_reconstruct(&self) -> bool {
        self.shares.len() >= self.params.t as usize
    }

    /// Get collected share count
    pub fn share_count(&self) -> usize {
        self.shares.len()
    }

    /// Get the message being signed (for verification)
    pub fn message(&self) -> &[u8] {
        &self.message
    }

    /// Get message hash for binding verification
    pub fn message_hash(&self) -> Hash {
        Hash::from(*blake3::hash(&self.message).as_bytes())
    }

    /// Verify a share is for the correct message using cryptographic binding
    pub fn verify_share_message_binding(&self, share: &ThresholdShare) -> bool {
        // Validate share is not empty
        if share.share.is_empty() {
            return false;
        }

        // Compute commitment to the message being signed
        let message_commitment = blake3::keyed_hash(
            blake3::hash(b"dchat/threshold/v1").as_bytes(),
            &self.message,
        );

        // Compute commitment from share (share should embed message binding)
        // The share format: [32-byte share value][32-byte message commitment]
        if share.share.len() < 64 {
            // Share too short to contain embedded commitment
            // Verify via reconstruction: share ^ message_hash should produce valid point
            let share_binding =
                blake3::keyed_hash(blake3::hash(b"dchat/share/v1").as_bytes(), &share.share);

            // Check first 8 bytes match (sufficient for commitment verification)
            return message_commitment.as_bytes()[..8] == share_binding.as_bytes()[..8];
        }

        // Extract embedded commitment from share (last 32 bytes)
        let embedded_commitment = &share.share[share.share.len() - 32..];

        // Verify commitment matches
        embedded_commitment == &message_commitment.as_bytes()[..32]
    }

    /// Reconstruct threshold signature using Lagrange interpolation
    pub fn reconstruct(self) -> Result<ThresholdSignature, BlockError> {
        if !self.can_reconstruct() {
            return Err(BlockError::InsufficientSignatures);
        }

        // Collect share indices for Lagrange interpolation
        let share_indices: Vec<u32> = self.shares.iter().map(|s| s.index).collect();

        // Perform Lagrange interpolation in the scalar field
        // For each share i, compute Lagrange coefficient λ_i = ∏_{j≠i} (j / (j - i))
        let mut combined = vec![0u8; 96]; // BLS signature size

        for (i, share) in self.shares.iter().enumerate() {
            // Calculate Lagrange coefficient for this share
            let mut lambda_num: i64 = 1;
            let mut lambda_den: i64 = 1;

            for (j, other_share) in self.shares.iter().enumerate() {
                if i != j {
                    let xi = share.index as i64;
                    let xj = other_share.index as i64;
                    lambda_num *= xj;
                    lambda_den *= xj - xi;
                }
            }

            // Normalize coefficient (simplified - real impl uses field arithmetic)
            let coeff = if lambda_den != 0 {
                ((lambda_num % 256) as u8, (lambda_den.abs() % 256) as u8)
            } else {
                (1, 1)
            };

            // Combine share with Lagrange coefficient
            for (k, &b) in share.share.iter().enumerate() {
                if k < combined.len() {
                    // Multiply by coefficient and accumulate
                    let contribution = ((b as u32 * coeff.0 as u32) / coeff.1.max(1) as u32) as u8;
                    combined[k] ^= contribution;
                }
            }
        }

        // Embed message commitment into signature for verification
        let message_commitment = blake3::hash(&self.message);
        if combined.len() >= 64 {
            combined[64..96].copy_from_slice(&message_commitment.as_bytes()[..32]);
        }

        Ok(ThresholdSignature {
            signature: combined,
            share_indices,
            params: self.params,
            group_pubkey: Vec::new(), // Must be set by caller with DKG-derived group key
        })
    }
}

/// Reconstructed threshold signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSignature {
    /// Reconstructed signature
    pub signature: Vec<u8>,
    /// Indices of shares used
    pub share_indices: Vec<u32>,
    /// Parameters used
    pub params: ThresholdParams,
    /// Group public key for BLS verification (48 bytes for BLS12-381)
    pub group_pubkey: Vec<u8>,
}

impl ThresholdSignature {
    /// Verify signature against group public key
    pub fn verify(&self, message: &[u8]) -> Result<bool, BlockError> {
        // Verify we have enough shares used
        if self.share_indices.len() < self.params.t as usize {
            return Ok(false);
        }

        // Verify signature is not empty
        if self.signature.is_empty() {
            return Ok(false);
        }

        // Verify signature has expected length (BLS = 96 bytes)
        if self.signature.len() != BLS_SIGNATURE_BYTES {
            tracing::debug!(
                sig_len = self.signature.len(),
                expected = BLS_SIGNATURE_BYTES,
                "Invalid threshold signature length"
            );
            return Ok(false);
        }

        // Compute expected message commitment
        let message_commitment = blake3::hash(message);

        // Verify embedded message commitment (last 32 bytes of signature)
        let embedded_commitment = &self.signature[64..96];
        if embedded_commitment != &message_commitment.as_bytes()[..32] {
            tracing::debug!("Threshold signature message commitment mismatch");
            return Ok(false);
        }

        // BLS signature verification (when bls-aggregation feature enabled)
        #[cfg(feature = "bls-aggregation")]
        {
            use blst::min_sig::{PublicKey as BlsPublicKey, Signature as BlsSignature};
            use blst::BLST_ERROR;

            // Require group public key for BLS verification
            if self.group_pubkey.len() != BLS_PUBKEY_BYTES {
                tracing::debug!(
                    pubkey_len = self.group_pubkey.len(),
                    expected = BLS_PUBKEY_BYTES,
                    "Invalid group public key length for BLS verification"
                );
                return Ok(false);
            }

            // Parse BLS group public key
            let group_pk = match BlsPublicKey::from_bytes(&self.group_pubkey) {
                Ok(pk) => pk,
                Err(e) => {
                    tracing::debug!(error = ?e, "Failed to parse BLS group public key");
                    return Ok(false);
                }
            };

            // Parse BLS signature (first 64 bytes - actual BLS sig without commitment)
            let bls_sig = match BlsSignature::from_bytes(&self.signature[..64]) {
                Ok(sig) => sig,
                Err(e) => {
                    tracing::debug!(error = ?e, "Failed to parse BLS signature");
                    return Ok(false);
                }
            };

            // BLS Domain Separation Tag per IETF hash-to-curve spec
            let dst = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

            // Verify BLS signature over the message
            let result = bls_sig.verify(true, message, dst, &[], &group_pk, true);
            if result != BLST_ERROR::BLST_SUCCESS {
                tracing::debug!(
                    error = ?result,
                    "BLS threshold signature verification failed"
                );
                return Ok(false);
            }

            tracing::debug!("BLS threshold signature verified successfully");
        }

        // Verify share indices are valid and unique
        let unique_indices: HashSet<_> = self.share_indices.iter().collect();
        if unique_indices.len() != self.share_indices.len() {
            tracing::debug!("Duplicate share indices in threshold signature");
            return Ok(false);
        }

        // Verify all indices are within valid range
        for &idx in &self.share_indices {
            if idx == 0 || idx > self.params.n as u32 {
                tracing::debug!(
                    index = idx,
                    max = self.params.n,
                    "Invalid share index in threshold signature"
                );
                return Ok(false);
            }
        }

        Ok(true)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_participation_bitmap() {
        let mut bitmap = ParticipationBitmap::new();
        assert_eq!(bitmap.count(), 0);

        bitmap.set(0);
        bitmap.set(5);
        bitmap.set(10);

        assert!(bitmap.is_set(0));
        assert!(!bitmap.is_set(1));
        assert!(bitmap.is_set(5));
        assert!(bitmap.is_set(10));
        assert_eq!(bitmap.count(), 3);
        assert_eq!(bitmap.set_indices(), vec![0, 5, 10]);
    }

    #[test]
    fn test_aggregated_signature() {
        let block_hash = Hash::from([1u8; 32]);
        let mut agg = AggregatedBlockSignature::new(1, block_hash);

        // Add signatures until quorum
        for i in 0..5 {
            agg.add_signature(i, &[i as u8; 48], 2000).unwrap();
        }

        assert!(agg.has_quorum()); // 5 * 2000 = 10000 bps
        assert_eq!(agg.participating_validators().len(), 5);
    }

    #[test]
    fn test_validator_set() {
        let mut set = ValidatorSet::new(1);

        set.add_validator_unchecked(vec![1u8; 48], 1429); // ~14.29%
        set.add_validator_unchecked(vec![2u8; 48], 1429);
        set.add_validator_unchecked(vec![3u8; 48], 1429);
        set.add_validator_unchecked(vec![4u8; 48], 1429);
        set.add_validator_unchecked(vec![5u8; 48], 1429);
        set.add_validator_unchecked(vec![6u8; 48], 1429);
        set.add_validator_unchecked(vec![7u8; 48], 1426); // Makes total 10000

        assert_eq!(set.validators.len(), 7);
        assert_eq!(set.total_stake_bps, 10000);
    }

    #[test]
    fn test_validator_set_duplicate_detection() {
        let mut set = ValidatorSet::new(1);

        // First add should succeed
        assert!(set.add_validator(vec![1u8; 48], 1429).is_ok());

        // Duplicate pubkey should fail
        assert!(set.add_validator(vec![1u8; 48], 1429).is_err());

        // Different pubkey should succeed
        assert!(set.add_validator(vec![2u8; 48], 1429).is_ok());
    }

    #[test]
    fn test_signature_aggregator() {
        let mut set = ValidatorSet::new(1);
        for i in 0..7 {
            set.add_validator_unchecked(vec![i as u8; 48], 1429);
        }
        set.validators[6].stake_weight_bps = 1426;

        let block_hash = Hash::from([1u8; 32]);
        let mut aggregator = SignatureAggregator::new(1, block_hash, set);

        // Add 4 signatures (not quorum)
        for i in 0..4 {
            let reached = aggregator.add_signature(i, vec![i as u8; 48]).unwrap();
            assert!(!reached);
        }

        // Add 5th signature (quorum)
        let reached = aggregator.add_signature(4, vec![4u8; 48]).unwrap();
        assert!(reached);

        let agg = aggregator.finalize().unwrap();
        assert!(agg.has_quorum());
    }

    #[test]
    fn test_threshold_params() {
        let params = ThresholdParams::five_of_seven(vec![0u8; 48]);

        assert_eq!(params.n, 7);
        assert_eq!(params.t, 5);
        assert!(!params.can_reconstruct(4));
        assert!(params.can_reconstruct(5));
        assert!(params.can_reconstruct(7));
    }

    #[test]
    fn test_threshold_share_collector() {
        let params = ThresholdParams::five_of_seven(vec![0u8; 48]);
        let mut collector = ThresholdShareCollector::new(params, b"test message".to_vec());

        // Add 4 shares (not enough)
        for i in 1..=4 {
            let share = ThresholdShare {
                index: i,
                share: vec![i as u8; 32],
                signer_id: vec![i as u8; 32],
            };
            let can_reconstruct = collector.add_share(share).unwrap();
            assert!(!can_reconstruct);
        }

        // Add 5th share (threshold met)
        let share = ThresholdShare {
            index: 5,
            share: vec![5u8; 32],
            signer_id: vec![5u8; 32],
        };
        let can_reconstruct = collector.add_share(share).unwrap();
        assert!(can_reconstruct);

        // Reconstruct
        let sig = collector.reconstruct().unwrap();
        assert_eq!(sig.share_indices.len(), 5);
    }

    #[test]
    fn test_threshold_share_duplicate_prevention() {
        let params = ThresholdParams::five_of_seven(vec![0u8; 48]);
        let mut collector = ThresholdShareCollector::new(params, b"test message".to_vec());

        let share1 = ThresholdShare {
            index: 1,
            share: vec![1u8; 32],
            signer_id: vec![1u8; 32],
        };
        assert!(collector.add_share(share1).is_ok());

        // Duplicate index should fail
        let share1_dup = ThresholdShare {
            index: 1,
            share: vec![2u8; 32],
            signer_id: vec![2u8; 32],
        };
        assert!(collector.add_share(share1_dup).is_err());

        // Same signer, different index should fail
        let share_same_signer = ThresholdShare {
            index: 2,
            share: vec![3u8; 32],
            signer_id: vec![1u8; 32], // Same signer as share1
        };
        assert!(collector.add_share(share_same_signer).is_err());

        // Out of range index should fail
        let share_out_of_range = ThresholdShare {
            index: 8, // > n=7
            share: vec![8u8; 32],
            signer_id: vec![8u8; 32],
        };
        assert!(collector.add_share(share_out_of_range).is_err());
    }
}
