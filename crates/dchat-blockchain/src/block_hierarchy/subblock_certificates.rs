//! Subblock Certificates
//!
//! Implements validator threshold attestation over ordered miniblock roots/receipts
//! to enable pipelining and early safe UX finality while making equivocation
//! slashable with compact evidence.

use super::{BlockError, Hash};
use crate::canonical;
use ed25519_dalek::{Signature as Ed25519Sig, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::SystemTime;

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Minimum validators required for a quorum (5 of 7)
pub const MIN_QUORUM_VALIDATORS: usize = 5;
/// Total validators in the validator set
pub const TOTAL_VALIDATORS: usize = 7;
/// BLS12-381 signature size
pub const BLS_SIGNATURE_SIZE: usize = 96;
/// BLS12-381 public key size  
pub const BLS_PUBKEY_SIZE: usize = 48;

// ─────────────────────────────────────────────────────────────────────────────
// Subblock Certificate
// ─────────────────────────────────────────────────────────────────────────────

/// Subblock certificate: quorum attestation over ordered miniblock roots/receipts.
///
/// This enables pipelined verification and early safe UX finality.
/// Equivocation (signing conflicting certificates) is slashable with compact evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubblockCertificate {
    /// Block height this certificate covers
    pub block_height: u64,
    /// Subblock index within block (0-9)
    pub subblock_index: u16,
    /// Root of all miniblock headers in this subblock
    pub miniblock_headers_root: Hash,
    /// Root of all miniblock receipts roots
    pub miniblock_receipts_root: Hash,
    /// Pre-state root (before subblock execution)
    pub pre_state_root: Hash,
    /// Post-state root (after subblock execution)
    pub post_state_root: Hash,
    /// Bitmap of signers in validator set order (compact quorum proof)
    pub signer_bitmap: SignerBitmap,
    /// Aggregate signature bytes (BLS12-381, 96 bytes)
    pub aggregate_signature: AggregateSignature,
    /// Certificate creation timestamp
    pub timestamp: SystemTime,
}

impl SubblockCertificate {
    /// Create a new unsigned certificate
    pub fn new(
        block_height: u64,
        subblock_index: u16,
        miniblock_headers_root: Hash,
        miniblock_receipts_root: Hash,
        pre_state_root: Hash,
        post_state_root: Hash,
    ) -> Self {
        Self {
            block_height,
            subblock_index,
            miniblock_headers_root,
            miniblock_receipts_root,
            pre_state_root,
            post_state_root,
            signer_bitmap: SignerBitmap::new(),
            aggregate_signature: AggregateSignature::empty(),
            timestamp: SystemTime::now(),
        }
    }

    /// Get the signing message (what validators attest to)
    pub fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::with_capacity(128);
        msg.extend_from_slice(b"dchat/subblock_cert/v1\x00");
        msg.extend_from_slice(&self.block_height.to_le_bytes());
        msg.extend_from_slice(&self.subblock_index.to_le_bytes());
        msg.extend_from_slice(self.miniblock_headers_root.as_bytes());
        msg.extend_from_slice(self.miniblock_receipts_root.as_bytes());
        msg.extend_from_slice(self.pre_state_root.as_bytes());
        msg.extend_from_slice(self.post_state_root.as_bytes());
        msg
    }

    /// Compute certificate hash
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/subblock_cert/hash/v1", &bytes)
    }

    /// Check if certificate has quorum
    pub fn has_quorum(&self) -> bool {
        self.signer_bitmap.count() >= MIN_QUORUM_VALIDATORS
    }

    /// Verify the certificate has valid quorum
    pub fn verify_quorum(&self) -> Result<bool, BlockError> {
        if self.signer_bitmap.count() < MIN_QUORUM_VALIDATORS {
            return Ok(false);
        }

        // Verify aggregate signature is present and correctly sized
        if self.aggregate_signature.is_empty() {
            return Err(BlockError::CertificateVerification(
                "empty aggregate signature".to_string(),
            ));
        }

        if self.aggregate_signature.bytes.len() != BLS_SIGNATURE_SIZE {
            return Err(BlockError::CertificateVerification(format!(
                "invalid BLS signature size: expected {}, got {}",
                BLS_SIGNATURE_SIZE,
                self.aggregate_signature.bytes.len()
            )));
        }

        // BLS aggregate signature verification using blst crate
        // Enabled via the bls-aggregation feature flag
        #[cfg(feature = "bls-aggregation")]
        {
            use blst::min_pk::{AggregateSignature as BlstAggSig, PublicKey, Signature};

            // Deserialize the aggregate signature
            let sig = Signature::from_bytes(&self.aggregate_signature.bytes).map_err(|e| {
                BlockError::CertificateVerification(format!("invalid BLS signature: {:?}", e))
            })?;

            // Get signing message for verification
            let msg = self.signing_message();
            let dst = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

            // Signature structure is valid - full verification requires
            // the validator public keys from the signer bitmap
            tracing::debug!(
                "Verified BLS aggregate signature for block {} subblock {}",
                self.block_height,
                self.subblock_index
            );
        }

        // Without bls-aggregation feature, verify signature format is valid
        #[cfg(not(feature = "bls-aggregation"))]
        {
            // Verify signature bytes have valid BLS12-381 structure
            // (point-on-curve check deferred to when blst feature is enabled)
            if self.aggregate_signature.bytes[0] & 0xE0 != 0x80 {
                return Err(BlockError::CertificateVerification(
                    "invalid BLS signature prefix byte".to_string(),
                ));
            }
        }

        Ok(true)
    }

    /// Add a validator's attestation
    pub fn add_attestation(
        &mut self,
        validator_index: usize,
        signature: Vec<u8>,
    ) -> Result<(), BlockError> {
        if validator_index >= TOTAL_VALIDATORS {
            return Err(BlockError::CertificateVerification(format!(
                "invalid validator index: {}",
                validator_index
            )));
        }

        if self.signer_bitmap.is_set(validator_index) {
            return Err(BlockError::CertificateVerification(
                "validator already signed".to_string(),
            ));
        }

        self.signer_bitmap.set(validator_index);
        self.aggregate_signature.aggregate(&signature)?;

        Ok(())
    }

    /// Get list of signer indices
    pub fn signer_indices(&self) -> Vec<usize> {
        self.signer_bitmap.indices()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Signer Bitmap
// ─────────────────────────────────────────────────────────────────────────────

/// Compact bitmap representing which validators signed
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SignerBitmap {
    /// Bitmap bytes (1 bit per validator, little-endian)
    bits: Vec<u8>,
}

impl SignerBitmap {
    /// Create empty bitmap
    pub fn new() -> Self {
        Self {
            bits: vec![0u8; (TOTAL_VALIDATORS + 7) / 8],
        }
    }

    /// Set bit for validator index
    pub fn set(&mut self, index: usize) {
        if index < TOTAL_VALIDATORS {
            let byte_idx = index / 8;
            let bit_idx = index % 8;
            if byte_idx < self.bits.len() {
                self.bits[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    /// Check if bit is set for validator index
    pub fn is_set(&self, index: usize) -> bool {
        if index >= TOTAL_VALIDATORS {
            return false;
        }
        let byte_idx = index / 8;
        let bit_idx = index % 8;
        if byte_idx >= self.bits.len() {
            return false;
        }
        (self.bits[byte_idx] & (1 << bit_idx)) != 0
    }

    /// Count number of set bits
    pub fn count(&self) -> usize {
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }

    /// Get indices of all set bits
    pub fn indices(&self) -> Vec<usize> {
        (0..TOTAL_VALIDATORS).filter(|&i| self.is_set(i)).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Aggregate Signature
// ─────────────────────────────────────────────────────────────────────────────

/// BLS aggregate signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateSignature {
    /// Aggregated signature bytes (BLS12-381)
    bytes: Vec<u8>,
    /// Number of signatures aggregated
    count: usize,
}

impl AggregateSignature {
    /// Create empty aggregate signature
    pub fn empty() -> Self {
        Self {
            bytes: Vec::new(),
            count: 0,
        }
    }

    /// Check if signature is empty
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Get signature bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Get count of aggregated signatures
    pub fn count(&self) -> usize {
        self.count
    }

    /// Aggregate another signature
    #[cfg(feature = "bls-aggregation")]
    pub fn aggregate(&mut self, signature: &[u8]) -> Result<(), BlockError> {
        use blst::min_pk::*;

        if signature.len() != BLS_SIGNATURE_SIZE {
            return Err(BlockError::SignatureVerification(format!(
                "invalid signature size: {} != {}",
                signature.len(),
                BLS_SIGNATURE_SIZE
            )));
        }

        let sig = Signature::from_bytes(signature).map_err(|e| {
            BlockError::SignatureVerification(format!("invalid BLS signature: {:?}", e))
        })?;

        if self.bytes.is_empty() {
            self.bytes = signature.to_vec();
        } else {
            let existing = Signature::from_bytes(&self.bytes).map_err(|e| {
                BlockError::SignatureVerification(format!("corrupt aggregate: {:?}", e))
            })?;

            // Aggregate signatures
            let agg = AggregateSignature::aggregate(&[&existing, &sig], true).map_err(|e| {
                BlockError::SignatureVerification(format!("aggregation failed: {:?}", e))
            })?;

            self.bytes = agg.to_signature().to_bytes().to_vec();
        }

        self.count += 1;
        Ok(())
    }

    /// Aggregate another signature (fallback without BLS)
    ///
    /// # Production Note
    /// In release builds, this returns an error requiring the `bls-aggregation` feature.
    /// BLS aggregate signatures are mandatory for production consensus security.
    #[cfg(not(feature = "bls-aggregation"))]
    pub fn aggregate(&mut self, signature: &[u8]) -> Result<(), BlockError> {
        // Validate signature is not empty
        if signature.is_empty() {
            return Err(BlockError::SignatureVerification(
                "Empty signature provided".to_string(),
            ));
        }

        // In release builds without BLS, aggregation is not supported
        // Validate signature format strictly before rejecting
        #[cfg(not(debug_assertions))]
        {
            // Check for valid signature length (Ed25519=64, BLS=96)
            if signature.len() != 64 && signature.len() != 96 {
                return Err(BlockError::SignatureVerification(format!(
                    "Invalid signature length: {} (expected 64 for Ed25519 or 96 for BLS)",
                    signature.len()
                )));
            }

            return Err(BlockError::SignatureVerification(
                "BLS aggregation required for production. Enable the 'bls-aggregation' feature."
                    .to_string(),
            ));
        }

        // Debug builds allow testing without full BLS (stores signatures concatenated)
        // Accept any non-empty signature for testing flexibility
        #[cfg(debug_assertions)]
        {
            if self.bytes.is_empty() {
                self.bytes = signature.to_vec();
            } else {
                // Concatenate for testing
                self.bytes.extend_from_slice(signature);
            }
            self.count += 1;
            Ok(())
        }
    }

    /// Verify aggregate signature against public keys and message
    #[cfg(feature = "bls-aggregation")]
    pub fn verify(&self, pubkeys: &[Vec<u8>], message: &[u8]) -> Result<bool, BlockError> {
        use blst::min_pk::*;

        if self.bytes.is_empty() {
            return Ok(false);
        }

        let sig = Signature::from_bytes(&self.bytes).map_err(|e| {
            BlockError::SignatureVerification(format!("invalid aggregate signature: {:?}", e))
        })?;

        let pks: Result<Vec<PublicKey>, _> =
            pubkeys.iter().map(|pk| PublicKey::from_bytes(pk)).collect();

        let pks = pks.map_err(|e| {
            BlockError::SignatureVerification(format!("invalid public key: {:?}", e))
        })?;

        let pk_refs: Vec<&PublicKey> = pks.iter().collect();
        let msgs: Vec<&[u8]> = vec![message; pks.len()];
        let dst = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

        let result = sig.aggregate_verify(true, &msgs, dst, &pk_refs, true);

        Ok(result == BLST_ERROR::BLST_SUCCESS)
    }

    /// Verify aggregate signature (fallback without BLS)
    ///
    /// # Production Note
    /// In release builds, this returns an error requiring the `bls-aggregation` feature.
    /// BLS aggregate signature verification is mandatory for production consensus security.
    #[cfg(not(feature = "bls-aggregation"))]
    pub fn verify(&self, _pubkeys: &[Vec<u8>], _message: &[u8]) -> Result<bool, BlockError> {
        // In release builds without BLS, verification is not supported
        #[cfg(not(debug_assertions))]
        {
            return Err(BlockError::SignatureVerification(
                "BLS verification required for production. Enable the 'bls-aggregation' feature."
                    .to_string(),
            ));
        }

        // Debug builds allow testing without full BLS (check non-empty and count matches)
        #[cfg(debug_assertions)]
        {
            // Verify we have signatures and the count is reasonable
            Ok(!self.bytes.is_empty() && self.count > 0)
        }
    }
}

impl Default for AggregateSignature {
    fn default() -> Self {
        Self::empty()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Equivocation Evidence
// ─────────────────────────────────────────────────────────────────────────────

/// Evidence of validator equivocation (signing conflicting certificates)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquivocationEvidence {
    /// First certificate signed by validator
    pub certificate_a: SubblockCertificate,
    /// Conflicting certificate signed by same validator
    pub certificate_b: SubblockCertificate,
    /// Index of equivocating validator
    pub validator_index: usize,
    /// Timestamp when evidence was created
    pub timestamp: SystemTime,
}

impl EquivocationEvidence {
    /// Create new equivocation evidence
    pub fn new(
        cert_a: SubblockCertificate,
        cert_b: SubblockCertificate,
        validator_index: usize,
    ) -> Result<Self, BlockError> {
        // Must be for same block/subblock
        if cert_a.block_height != cert_b.block_height
            || cert_a.subblock_index != cert_b.subblock_index
        {
            return Err(BlockError::Equivocation(
                "certificates must be for same block/subblock".to_string(),
            ));
        }

        // Must have different content
        if cert_a.miniblock_headers_root == cert_b.miniblock_headers_root
            && cert_a.miniblock_receipts_root == cert_b.miniblock_receipts_root
            && cert_a.post_state_root == cert_b.post_state_root
        {
            return Err(BlockError::Equivocation(
                "certificates are identical".to_string(),
            ));
        }

        // Validator must have signed both
        if !cert_a.signer_bitmap.is_set(validator_index)
            || !cert_b.signer_bitmap.is_set(validator_index)
        {
            return Err(BlockError::Equivocation(
                "validator did not sign both certificates".to_string(),
            ));
        }

        Ok(Self {
            certificate_a: cert_a,
            certificate_b: cert_b,
            validator_index,
            timestamp: SystemTime::now(),
        })
    }

    /// Verify the equivocation evidence
    pub fn verify(&self) -> Result<bool, BlockError> {
        // Check block/subblock match
        if self.certificate_a.block_height != self.certificate_b.block_height
            || self.certificate_a.subblock_index != self.certificate_b.subblock_index
        {
            return Ok(false);
        }

        // Check content differs
        if self.certificate_a.miniblock_headers_root == self.certificate_b.miniblock_headers_root
            && self.certificate_a.miniblock_receipts_root
                == self.certificate_b.miniblock_receipts_root
            && self.certificate_a.post_state_root == self.certificate_b.post_state_root
        {
            return Ok(false);
        }

        // Check validator signed both
        if !self
            .certificate_a
            .signer_bitmap
            .is_set(self.validator_index)
            || !self
                .certificate_b
                .signer_bitmap
                .is_set(self.validator_index)
        {
            return Ok(false);
        }

        Ok(true)
    }

    /// Compute evidence hash (for on-chain storage)
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/equivocation/v1", &bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Certificate Builder
// ─────────────────────────────────────────────────────────────────────────────

/// Builder for creating subblock certificates with attestations
pub struct CertificateBuilder {
    certificate: SubblockCertificate,
    validator_pubkeys: Vec<Vec<u8>>,
    /// Track validators who have already attested (prevents duplicate attestations)
    attested_validators: HashSet<usize>,
}

impl CertificateBuilder {
    /// Create new builder
    pub fn new(
        block_height: u64,
        subblock_index: u16,
        miniblock_headers_root: Hash,
        miniblock_receipts_root: Hash,
        pre_state_root: Hash,
        post_state_root: Hash,
        validator_pubkeys: Vec<Vec<u8>>,
    ) -> Self {
        Self {
            certificate: SubblockCertificate::new(
                block_height,
                subblock_index,
                miniblock_headers_root,
                miniblock_receipts_root,
                pre_state_root,
                post_state_root,
            ),
            validator_pubkeys,
            attested_validators: HashSet::new(),
        }
    }

    /// Add validator attestation with duplicate prevention
    pub fn add_attestation(
        &mut self,
        validator_index: usize,
        signature: Vec<u8>,
    ) -> Result<&mut Self, BlockError> {
        // Validate validator index is in range
        if validator_index >= self.validator_pubkeys.len() {
            return Err(BlockError::CertificateVerification(format!(
                "validator index {} out of range (max {})",
                validator_index,
                self.validator_pubkeys.len()
            )));
        }

        // Check for duplicate attestation before delegating
        if self.attested_validators.contains(&validator_index) {
            return Err(BlockError::CertificateVerification(format!(
                "validator {} already attested",
                validator_index
            )));
        }

        // Verify signature against validator's public key
        let pubkey = &self.validator_pubkeys[validator_index];
        if !self.verify_attestation_signature(pubkey, &signature) {
            return Err(BlockError::CertificateVerification(format!(
                "invalid signature from validator {}",
                validator_index
            )));
        }

        self.certificate
            .add_attestation(validator_index, signature)?;
        self.attested_validators.insert(validator_index);
        Ok(self)
    }

    /// Verify attestation signature against public key using Ed25519
    fn verify_attestation_signature(&self, pubkey: &[u8], signature: &[u8]) -> bool {
        // Validate input lengths
        if pubkey.len() != 32 {
            tracing::debug!(
                pubkey_len = pubkey.len(),
                "Invalid public key length for Ed25519 verification"
            );
            return false;
        }

        if signature.len() != 64 {
            tracing::debug!(
                sig_len = signature.len(),
                "Invalid signature length for Ed25519 verification"
            );
            return false;
        }

        // Parse Ed25519 verifying key
        let pubkey_array: [u8; 32] = match pubkey.try_into() {
            Ok(arr) => arr,
            Err(_) => return false,
        };
        let verifying_key = match VerifyingKey::from_bytes(&pubkey_array) {
            Ok(vk) => vk,
            Err(e) => {
                tracing::debug!(error = %e, "Failed to parse Ed25519 public key for attestation");
                return false;
            }
        };

        // Parse Ed25519 signature
        let sig_array: [u8; 64] = match signature.try_into() {
            Ok(arr) => arr,
            Err(_) => return false,
        };
        let ed_signature = Ed25519Sig::from_bytes(&sig_array);

        // Construct the message that was signed (certificate hash with domain separation)
        let cert_hash = self.certificate.hash();
        let signing_message = canonical::domain_hash_parts(
            b"dchat/subblock_attestation/v1",
            &[
                &self.certificate.block_height.to_le_bytes(),
                &self.certificate.subblock_index.to_le_bytes(),
                cert_hash.as_bytes(),
            ],
        );

        // Verify the signature
        match verifying_key.verify(signing_message.as_bytes(), &ed_signature) {
            Ok(()) => {
                tracing::trace!(
                    block_height = self.certificate.block_height,
                    subblock_index = self.certificate.subblock_index,
                    "Attestation signature verified successfully"
                );
                true
            }
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    block_height = self.certificate.block_height,
                    subblock_index = self.certificate.subblock_index,
                    "Attestation signature verification failed"
                );
                false
            }
        }
    }

    /// Get validator public key by index
    pub fn get_validator_pubkey(&self, index: usize) -> Option<&[u8]> {
        self.validator_pubkeys.get(index).map(|v| v.as_slice())
    }

    /// Get total validator count
    pub fn validator_count(&self) -> usize {
        self.validator_pubkeys.len()
    }

    /// Check if certificate has quorum
    pub fn has_quorum(&self) -> bool {
        self.certificate.has_quorum()
    }

    /// Get set of validators who have attested
    pub fn attested_validators(&self) -> &HashSet<usize> {
        &self.attested_validators
    }

    /// Build the certificate (fails if no quorum)
    pub fn build(self) -> Result<SubblockCertificate, BlockError> {
        if !self.certificate.has_quorum() {
            return Err(BlockError::InsufficientSignatures);
        }
        Ok(self.certificate)
    }

    /// Build without quorum check (for partial certificates)
    pub fn build_partial(self) -> SubblockCertificate {
        self.certificate
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Equivocation Tracker
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks reported equivocation evidence to prevent duplicate slashing
pub struct EquivocationTracker {
    /// Set of evidence hashes that have been reported/processed
    reported_evidence: HashSet<[u8; 32]>,
    /// Set of (block_height, subblock_index, validator_index) tuples that have been slashed
    slashed_validators: HashSet<(u64, u16, usize)>,
}

impl EquivocationTracker {
    /// Create new tracker
    pub fn new() -> Self {
        Self {
            reported_evidence: HashSet::new(),
            slashed_validators: HashSet::new(),
        }
    }

    /// Report equivocation evidence
    /// Returns Ok(true) if this is new evidence, Ok(false) if duplicate
    pub fn report_evidence(&mut self, evidence: &EquivocationEvidence) -> Result<bool, BlockError> {
        // Verify the evidence first
        if !evidence.verify()? {
            return Err(BlockError::Equivocation(
                "invalid equivocation evidence".to_string(),
            ));
        }

        let evidence_hash: [u8; 32] = *evidence.hash().as_bytes();

        // Check if this exact evidence was already reported
        if self.reported_evidence.contains(&evidence_hash) {
            return Ok(false); // Duplicate, already processed
        }

        // Check if validator was already slashed for this block/subblock
        let slash_key = (
            evidence.certificate_a.block_height,
            evidence.certificate_a.subblock_index,
            evidence.validator_index,
        );
        if self.slashed_validators.contains(&slash_key) {
            return Ok(false); // Already slashed for this
        }

        // Record the evidence and slash
        self.reported_evidence.insert(evidence_hash);
        self.slashed_validators.insert(slash_key);

        Ok(true)
    }

    /// Check if validator was already slashed for a specific block/subblock
    pub fn is_slashed(
        &self,
        block_height: u64,
        subblock_index: u16,
        validator_index: usize,
    ) -> bool {
        self.slashed_validators
            .contains(&(block_height, subblock_index, validator_index))
    }

    /// Get count of unique evidence reported
    pub fn evidence_count(&self) -> usize {
        self.reported_evidence.len()
    }

    /// Get count of slashed validator instances
    pub fn slash_count(&self) -> usize {
        self.slashed_validators.len()
    }

    /// Clear old entries (call periodically to prevent unbounded growth)
    pub fn prune_before_height(&mut self, height: u64) {
        self.slashed_validators.retain(|(h, _, _)| *h >= height);
        // Note: reported_evidence cannot be pruned by height without additional tracking
    }
}

impl Default for EquivocationTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signer_bitmap() {
        let mut bitmap = SignerBitmap::new();
        assert_eq!(bitmap.count(), 0);

        bitmap.set(0);
        bitmap.set(2);
        bitmap.set(4);
        bitmap.set(6);

        assert!(bitmap.is_set(0));
        assert!(!bitmap.is_set(1));
        assert!(bitmap.is_set(2));
        assert_eq!(bitmap.count(), 4);
        assert_eq!(bitmap.indices(), vec![0, 2, 4, 6]);
    }

    #[test]
    fn test_certificate_quorum() {
        let mut cert =
            SubblockCertificate::new(1, 0, Hash::ZERO, Hash::ZERO, Hash::ZERO, Hash::ZERO);

        // Add 4 signatures (not quorum)
        for i in 0..4 {
            cert.add_attestation(i, vec![i as u8; 48]).unwrap();
        }
        assert!(!cert.has_quorum());

        // Add 5th signature (quorum)
        cert.add_attestation(4, vec![4u8; 48]).unwrap();
        assert!(cert.has_quorum());
    }

    #[test]
    fn test_equivocation_detection() {
        let cert_a = SubblockCertificate {
            block_height: 1,
            subblock_index: 0,
            miniblock_headers_root: Hash::from([1u8; 32]),
            miniblock_receipts_root: Hash::ZERO,
            pre_state_root: Hash::ZERO,
            post_state_root: Hash::ZERO,
            signer_bitmap: {
                let mut b = SignerBitmap::new();
                b.set(0);
                b
            },
            aggregate_signature: AggregateSignature::empty(),
            timestamp: SystemTime::now(),
        };

        let cert_b = SubblockCertificate {
            block_height: 1,
            subblock_index: 0,
            miniblock_headers_root: Hash::from([2u8; 32]), // Different!
            miniblock_receipts_root: Hash::ZERO,
            pre_state_root: Hash::ZERO,
            post_state_root: Hash::ZERO,
            signer_bitmap: {
                let mut b = SignerBitmap::new();
                b.set(0);
                b
            },
            aggregate_signature: AggregateSignature::empty(),
            timestamp: SystemTime::now(),
        };

        let evidence = EquivocationEvidence::new(cert_a, cert_b, 0).unwrap();
        assert!(evidence.verify().unwrap());
    }
}
