//! Proof-of-delivery tracking

use dchat_core::error::{Error, Result};
use dchat_core::types::{MessageId, Signature};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Proof that a message was delivered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryProof {
    /// Message ID
    pub message_id: MessageId,

    /// Relay node that delivered
    pub relay_peer_id: String,

    /// Recipient signature acknowledging receipt
    pub recipient_signature: Option<Signature>,

    /// Timestamp of delivery
    pub timestamp: SystemTime,

    /// On-chain transaction hash (if submitted)
    pub chain_tx_hash: Option<String>,
}

impl DeliveryProof {
    /// Verify the proof is valid
    pub fn verify(&self, _recipient_pubkey: &[u8]) -> Result<bool> {
        // In a real implementation, verify:
        // 1. Recipient signature is valid
        // 2. Chain transaction exists and is confirmed
        // 3. Timestamp is reasonable

        Ok(self.recipient_signature.is_some())
    }

    /// Check if proof is on-chain
    pub fn is_on_chain(&self) -> bool {
        self.chain_tx_hash.is_some()
    }
}

/// Status of delivery tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    /// Message sent to network
    Sent,

    /// Relay acknowledged receipt
    RelayAcknowledged,

    /// Recipient acknowledged receipt
    RecipientAcknowledged,

    /// Proof submitted on-chain
    OnChain,

    /// Delivery failed
    Failed,
}

/// Delivery tracker for monitoring message delivery
pub struct DeliveryTracker {
    /// Track delivery status per message
    statuses: HashMap<MessageId, DeliveryStatus>,

    /// Store delivery proofs
    proofs: HashMap<MessageId, DeliveryProof>,

    /// Track delivery attempts
    attempts: HashMap<MessageId, u32>,

    /// Maximum delivery attempts before marking as failed
    max_attempts: u32,
}

impl DeliveryTracker {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            statuses: HashMap::new(),
            proofs: HashMap::new(),
            attempts: HashMap::new(),
            max_attempts,
        }
    }

    /// Mark message as sent
    pub fn mark_sent(&mut self, message_id: MessageId) {
        self.statuses.insert(message_id, DeliveryStatus::Sent);
        self.attempts.insert(message_id, 1);
    }

    /// Record a delivery attempt
    pub fn record_attempt(&mut self, message_id: MessageId) -> Result<()> {
        let attempts = self.attempts.entry(message_id).or_insert(0);
        *attempts += 1;

        if *attempts > self.max_attempts {
            self.statuses.insert(message_id, DeliveryStatus::Failed);
            return Err(Error::messaging(
                "Max delivery attempts exceeded".to_string(),
            ));
        }

        Ok(())
    }

    /// Mark relay acknowledgment
    pub fn mark_relay_ack(&mut self, message_id: MessageId) {
        self.statuses
            .insert(message_id, DeliveryStatus::RelayAcknowledged);
    }

    /// Store delivery proof
    pub fn store_proof(&mut self, proof: DeliveryProof) {
        let message_id = proof.message_id;

        if proof.is_on_chain() {
            self.statuses.insert(message_id, DeliveryStatus::OnChain);
        } else if proof.recipient_signature.is_some() {
            self.statuses
                .insert(message_id, DeliveryStatus::RecipientAcknowledged);
        }

        self.proofs.insert(message_id, proof);
    }

    /// Get delivery status
    pub fn get_status(&self, message_id: &MessageId) -> Option<DeliveryStatus> {
        self.statuses.get(message_id).copied()
    }

    /// Get delivery proof
    pub fn get_proof(&self, message_id: &MessageId) -> Option<&DeliveryProof> {
        self.proofs.get(message_id)
    }

    /// Check if message was delivered
    pub fn is_delivered(&self, message_id: &MessageId) -> bool {
        matches!(
            self.get_status(message_id),
            Some(DeliveryStatus::RecipientAcknowledged | DeliveryStatus::OnChain)
        )
    }

    /// Get failed messages
    pub fn failed_messages(&self) -> Vec<MessageId> {
        self.statuses
            .iter()
            .filter(|(_, status)| **status == DeliveryStatus::Failed)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get attempt count
    pub fn attempt_count(&self, message_id: &MessageId) -> u32 {
        self.attempts.get(message_id).copied().unwrap_or(0)
    }
}

impl Default for DeliveryTracker {
    fn default() -> Self {
        Self::new(3) // Default 3 attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delivery_tracking() {
        let mut tracker = DeliveryTracker::new(3);
        let msg_id = MessageId(uuid::Uuid::new_v4());

        tracker.mark_sent(msg_id.clone());
        assert_eq!(tracker.get_status(&msg_id), Some(DeliveryStatus::Sent));

        tracker.mark_relay_ack(msg_id.clone());
        assert_eq!(
            tracker.get_status(&msg_id),
            Some(DeliveryStatus::RelayAcknowledged)
        );

        let proof = DeliveryProof {
            message_id: msg_id.clone(),
            relay_peer_id: "relay1".to_string(),
            recipient_signature: Some(Signature(vec![1, 2, 3])),
            timestamp: SystemTime::now(),
            chain_tx_hash: None,
        };

        tracker.store_proof(proof);
        assert!(tracker.is_delivered(&msg_id));
    }

    #[test]
    fn test_max_attempts() {
        let mut tracker = DeliveryTracker::new(3);
        let msg_id = MessageId(uuid::Uuid::new_v4());

        tracker.mark_sent(msg_id.clone());

        assert!(tracker.record_attempt(msg_id.clone()).is_ok());
        assert!(tracker.record_attempt(msg_id.clone()).is_ok());

        let result = tracker.record_attempt(msg_id.clone());
        assert!(result.is_err());
        assert_eq!(tracker.get_status(&msg_id), Some(DeliveryStatus::Failed));
    }
}
