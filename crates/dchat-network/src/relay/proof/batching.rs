/// Efficient batching of delivery proofs for on-chain submission.
///
/// This module handles the aggregation of multiple delivery proofs into batches,
/// optimizing gas costs and reducing on-chain transaction overhead.
///
/// # Batch Submission Strategy
///
/// - **Batch Size**: 100 proofs per submission (balances efficiency vs. latency)
/// - **Timeout**: 5 minutes maximum wait before forcing submission
/// - **Priority**: High-reputation relays get faster batch inclusion
/// - **Verification**: All proofs validated before batch submission
///
/// # Economic Model
///
/// Batching reduces per-proof submission cost from ~100k gas to ~1k gas,
/// making micropayments economically viable for relay rewards.
use super::delivery::{DeliveryProof, MessageId};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum number of proofs per batch submission.
pub const BATCH_SIZE: usize = 100;

/// Maximum time to wait before forcing batch submission (5 minutes).
pub const BATCH_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// A batch of delivery proofs ready for on-chain submission.
///
/// Aggregates multiple proofs from one or more relays, reducing submission overhead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofBatch {
    /// All proofs in this batch
    pub proofs: Vec<DeliveryProof>,

    /// Unique batch identifier (hash of all proof message IDs)
    pub batch_id: BatchId,

    /// Unix timestamp when batch was created
    pub created_at: u64,

    /// Total number of unique relays in this batch
    pub relay_count: usize,

    /// Total number of unique recipients in this batch
    pub recipient_count: usize,
}

/// Unique identifier for a proof batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BatchId(pub [u8; 32]);

impl BatchId {
    /// Creates a new batch ID from proof message IDs.
    ///
    /// Uses blake3 to hash all message IDs deterministically.
    pub fn from_proofs(proofs: &[DeliveryProof]) -> Self {
        use blake3::Hasher;

        let mut hasher = Hasher::new();
        for proof in proofs {
            hasher.update(proof.message_id.as_bytes());
        }

        let hash = hasher.finalize();
        Self(*hash.as_bytes())
    }

    /// Returns the inner byte array.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl ProofBatch {
    /// Creates a new batch from a collection of proofs.
    ///
    /// # Arguments
    ///
    /// * `proofs` - Vector of delivery proofs (max BATCH_SIZE)
    ///
    /// # Returns
    ///
    /// - `Ok(ProofBatch)` if batch is valid
    /// - `Err(String)` if validation fails
    ///
    /// # Validation
    ///
    /// - All proofs must verify cryptographically
    /// - No duplicate message IDs allowed
    /// - Batch size must not exceed BATCH_SIZE
    pub fn new(proofs: Vec<DeliveryProof>) -> Result<Self, String> {
        if proofs.is_empty() {
            return Err("Cannot create empty batch".to_string());
        }

        if proofs.len() > BATCH_SIZE {
            return Err(format!(
                "Batch size {} exceeds maximum {}",
                proofs.len(),
                BATCH_SIZE
            ));
        }

        // Verify all proofs
        for (i, proof) in proofs.iter().enumerate() {
            proof
                .verify()
                .map_err(|e| format!("Proof {} verification failed: {}", i, e))?;
        }

        // Check for duplicate message IDs
        let mut seen_ids = std::collections::HashSet::new();
        for proof in &proofs {
            if !seen_ids.insert(proof.message_id) {
                return Err(format!(
                    "Duplicate message ID in batch: {:?}",
                    proof.message_id
                ));
            }
        }

        // Count unique relays and recipients
        let relay_count = proofs
            .iter()
            .map(|p| p.relay_key)
            .collect::<std::collections::HashSet<_>>()
            .len();

        let recipient_count = proofs
            .iter()
            .map(|p| p.recipient_key)
            .collect::<std::collections::HashSet<_>>()
            .len();

        let batch_id = BatchId::from_proofs(&proofs);
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(Self {
            proofs,
            batch_id,
            created_at,
            relay_count,
            recipient_count,
        })
    }

    /// Returns the number of proofs in this batch.
    pub fn len(&self) -> usize {
        self.proofs.len()
    }

    /// Checks if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.proofs.is_empty()
    }

    /// Checks if the batch is full (reached BATCH_SIZE).
    pub fn is_full(&self) -> bool {
        self.proofs.len() >= BATCH_SIZE
    }

    /// Returns all message IDs in this batch.
    pub fn message_ids(&self) -> Vec<MessageId> {
        self.proofs.iter().map(|p| p.message_id).collect()
    }

    /// Calculates total rewards for all relays in this batch.
    ///
    /// Returns a mapping of relay key -> total reward amount.
    pub fn calculate_rewards(&self, reward_per_proof: u64) -> HashMap<VerifyingKey, u64> {
        let mut rewards = HashMap::new();
        for proof in &self.proofs {
            *rewards.entry(proof.relay_key).or_insert(0) += reward_per_proof;
        }
        rewards
    }
}

/// Manages accumulation of proofs into batches for submission.
///
/// Collects proofs as they arrive, automatically creating batches when:
/// - Batch size reaches BATCH_SIZE (100 proofs)
/// - BATCH_TIMEOUT expires (5 minutes)
pub struct BatchAccumulator {
    /// Current pending proofs
    pending: Vec<DeliveryProof>,

    /// When the current batch was started
    batch_started_at: Instant,

    /// Total batches created
    batches_created: u64,
}

impl BatchAccumulator {
    /// Creates a new batch accumulator.
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            batch_started_at: Instant::now(),
            batches_created: 0,
        }
    }

    /// Adds a proof to the accumulator.
    ///
    /// Returns `Some(ProofBatch)` if a batch is ready for submission.
    pub fn add_proof(&mut self, proof: DeliveryProof) -> Result<Option<ProofBatch>, String> {
        // Verify proof before adding
        proof.verify()?;

        self.pending.push(proof);

        // Check if we should create a batch
        if self.should_create_batch() {
            self.create_batch()
        } else {
            Ok(None)
        }
    }

    /// Checks if a batch should be created based on size or timeout.
    fn should_create_batch(&self) -> bool {
        self.pending.len() >= BATCH_SIZE || self.batch_started_at.elapsed() >= BATCH_TIMEOUT
    }

    /// Creates a batch from pending proofs and resets accumulator.
    fn create_batch(&mut self) -> Result<Option<ProofBatch>, String> {
        if self.pending.is_empty() {
            return Ok(None);
        }

        let proofs = std::mem::take(&mut self.pending);
        let batch = ProofBatch::new(proofs)?;

        self.batch_started_at = Instant::now();
        self.batches_created += 1;

        Ok(Some(batch))
    }

    /// Forces creation of a batch even if not full (e.g., on shutdown).
    pub fn flush(&mut self) -> Result<Option<ProofBatch>, String> {
        self.create_batch()
    }

    /// Returns the number of pending proofs.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Returns time elapsed since batch started.
    pub fn time_until_timeout(&self) -> Duration {
        BATCH_TIMEOUT.saturating_sub(self.batch_started_at.elapsed())
    }

    /// Returns total number of batches created.
    pub fn batches_created(&self) -> u64 {
        self.batches_created
    }
}

impl Default for BatchAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::proof::delivery::{create_recipient_acknowledgment, MessageId};
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use std::time::SystemTime;

    fn create_test_proof(message_id: MessageId) -> DeliveryProof {
        let mut csprng = OsRng;
        let relay_key = SigningKey::generate(&mut csprng);
        let recipient_key = SigningKey::generate(&mut csprng);

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let recipient_sig = create_recipient_acknowledgment(
            message_id,
            &relay_key.verifying_key(),
            timestamp,
            &recipient_key,
        );

        DeliveryProof::new(
            message_id,
            &relay_key,
            recipient_key.verifying_key(),
            recipient_sig,
        )
    }

    #[test]
    fn test_batch_creation() {
        let proofs = vec![
            create_test_proof(MessageId::new([1u8; 32])),
            create_test_proof(MessageId::new([2u8; 32])),
            create_test_proof(MessageId::new([3u8; 32])),
        ];

        let batch = ProofBatch::new(proofs).unwrap();
        assert_eq!(batch.len(), 3);
        assert!(!batch.is_full());
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_empty_batch_rejected() {
        let result = ProofBatch::new(vec![]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_oversized_batch_rejected() {
        let mut proofs = Vec::new();
        for i in 0..BATCH_SIZE + 1 {
            let mut id = [0u8; 32];
            id[0] = i as u8;
            proofs.push(create_test_proof(MessageId::new(id)));
        }

        let result = ProofBatch::new(proofs);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds maximum"));
    }

    #[test]
    fn test_duplicate_message_ids_rejected() {
        let msg_id = MessageId::new([42u8; 32]);
        let proofs = vec![
            create_test_proof(msg_id),
            create_test_proof(msg_id), // Duplicate
        ];

        let result = ProofBatch::new(proofs);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate"));
    }

    #[test]
    fn test_batch_id_deterministic() {
        let proofs = vec![
            create_test_proof(MessageId::new([1u8; 32])),
            create_test_proof(MessageId::new([2u8; 32])),
        ];

        let batch1 = ProofBatch::new(proofs.clone()).unwrap();
        let batch2 = ProofBatch::new(proofs).unwrap();

        assert_eq!(batch1.batch_id, batch2.batch_id);
    }

    #[test]
    fn test_batch_accumulator() {
        let mut accumulator = BatchAccumulator::new();
        assert_eq!(accumulator.pending_count(), 0);

        // Add proofs one by one
        for i in 0..5 {
            let mut id = [0u8; 32];
            id[0] = i;
            let proof = create_test_proof(MessageId::new(id));
            let result = accumulator.add_proof(proof).unwrap();
            assert!(result.is_none()); // Not enough for batch yet
        }

        assert_eq!(accumulator.pending_count(), 5);
    }

    #[test]
    fn test_accumulator_auto_batch_on_size() {
        let mut accumulator = BatchAccumulator::new();

        // Add BATCH_SIZE proofs
        for i in 0..BATCH_SIZE {
            let mut id = [0u8; 32];
            id[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            let proof = create_test_proof(MessageId::new(id));
            let result = accumulator.add_proof(proof).unwrap();

            if i < BATCH_SIZE - 1 {
                assert!(result.is_none());
            } else {
                assert!(result.is_some());
                let batch = result.unwrap();
                assert_eq!(batch.len(), BATCH_SIZE);
                assert!(batch.is_full());
            }
        }

        assert_eq!(accumulator.pending_count(), 0);
        assert_eq!(accumulator.batches_created(), 1);
    }

    #[test]
    fn test_accumulator_flush() {
        let mut accumulator = BatchAccumulator::new();

        // Add just 3 proofs (not enough to trigger auto-batch)
        for i in 0..3 {
            let mut id = [0u8; 32];
            id[0] = i;
            let proof = create_test_proof(MessageId::new(id));
            accumulator.add_proof(proof).unwrap();
        }

        assert_eq!(accumulator.pending_count(), 3);

        // Force flush
        let batch = accumulator.flush().unwrap().unwrap();
        assert_eq!(batch.len(), 3);
        assert_eq!(accumulator.pending_count(), 0);
    }

    #[test]
    fn test_batch_rewards_calculation() {
        let relay_key = SigningKey::generate(&mut OsRng);

        // Create 3 proofs from same relay
        let mut proofs = Vec::new();
        for i in 0..3 {
            let recipient_key = SigningKey::generate(&mut OsRng);
            let msg_id = MessageId::new([i; 32]);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let recipient_sig = create_recipient_acknowledgment(
                msg_id,
                &relay_key.verifying_key(),
                timestamp,
                &recipient_key,
            );

            let proof = DeliveryProof::new(
                msg_id,
                &relay_key,
                recipient_key.verifying_key(),
                recipient_sig,
            );
            proofs.push(proof);
        }

        let batch = ProofBatch::new(proofs).unwrap();
        let rewards = batch.calculate_rewards(100);

        assert_eq!(rewards.len(), 1); // One relay
        assert_eq!(rewards.get(&relay_key.verifying_key()), Some(&300)); // 3 proofs * 100
    }

    #[test]
    fn test_batch_message_ids() {
        let proofs = vec![
            create_test_proof(MessageId::new([1u8; 32])),
            create_test_proof(MessageId::new([2u8; 32])),
            create_test_proof(MessageId::new([3u8; 32])),
        ];

        let batch = ProofBatch::new(proofs).unwrap();
        let ids = batch.message_ids();

        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&MessageId::new([1u8; 32])));
        assert!(ids.contains(&MessageId::new([2u8; 32])));
        assert!(ids.contains(&MessageId::new([3u8; 32])));
    }
}
