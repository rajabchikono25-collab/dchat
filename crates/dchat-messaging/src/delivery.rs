//! Proof-of-delivery tracking

use dchat_blockchain::{BlockchainClient, TransactionStatus};
use dchat_core::error::{Error, Result};
use dchat_core::types::{MessageId, Signature};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Proof that a message was delivered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryProof {
    /// Message ID
    pub message_id: MessageId,

    /// Relay node that delivered
    pub relay_peer_id: String,

    /// Recipient user ID
    pub recipient_id: dchat_core::types::UserId,

    /// Message content hash (SHA-256)
    pub content_hash: String,

    /// Recipient signature acknowledging receipt
    pub recipient_signature: Option<Signature>,

    /// Timestamp of delivery
    pub timestamp: SystemTime,

    /// On-chain transaction hash (if submitted)
    pub chain_tx_hash: Option<String>,

    /// Reward amount for relay (in tokens)
    pub reward_amount: u64,
}

impl DeliveryProof {
    /// Verify the proof is valid (without chain verification)
    /// Use verify_with_chain_client for full verification including on-chain confirmation
    pub fn verify(&self, recipient_pubkey: &[u8]) -> Result<bool> {
        use ed25519_dalek::{Signature, VerifyingKey};

        // 1. Verify recipient signature if present
        if let Some(sig) = &self.recipient_signature {
            // Parse public key (32 bytes for Ed25519)
            if recipient_pubkey.len() != 32 {
                return Err(Error::validation("Invalid recipient public key length"));
            }

            let verifying_key = VerifyingKey::from_bytes(
                recipient_pubkey
                    .try_into()
                    .map_err(|_| Error::validation("Failed to parse public key"))?,
            )
            .map_err(|e| Error::validation(format!("Invalid public key: {}", e)))?;

            // Parse signature (64 bytes for Ed25519)
            if sig.0.len() != 64 {
                return Err(Error::validation("Invalid signature length"));
            }

            let signature = Signature::from_bytes(
                &sig.0[..]
                    .try_into()
                    .map_err(|_| Error::validation("Failed to parse signature"))?,
            );

            // Construct message to verify: message_id || relay_peer_id || timestamp
            let mut message_bytes = Vec::new();
            message_bytes.extend_from_slice(self.message_id.0.as_bytes());
            message_bytes.extend_from_slice(self.relay_peer_id.as_bytes());
            if let Ok(duration) = self.timestamp.duration_since(std::time::UNIX_EPOCH) {
                message_bytes.extend_from_slice(&duration.as_secs().to_le_bytes());
            }

            // Verify signature
            verifying_key
                .verify_strict(&message_bytes, &signature)
                .map_err(|e| Error::validation(format!("Signature verification failed: {}", e)))?;
        } else {
            return Ok(false); // No signature present
        }

        // 2. Verify timestamp is reasonable (within last 24 hours)
        if let Ok(elapsed) = self.timestamp.elapsed() {
            if elapsed > std::time::Duration::from_secs(86400) {
                return Err(Error::validation("Delivery proof timestamp too old"));
            }
        }

        Ok(true)
    }

    /// Verify the proof with full on-chain transaction confirmation
    pub async fn verify_with_chain_client(
        &self,
        recipient_pubkey: &[u8],
        chain_client: Option<&impl ChainVerifier>,
    ) -> Result<bool> {
        // First verify the basic proof (signature and timestamp)
        if !self.verify(recipient_pubkey)? {
            return Ok(false);
        }

        // Then verify chain transaction if present and client provided
        if let Some(tx_hash) = &self.chain_tx_hash {
            if let Some(verifier) = chain_client {
                let confirmed = verifier.verify_transaction_confirmed(tx_hash).await?;
                if !confirmed {
                    tracing::warn!("Transaction {} not yet confirmed on chain", tx_hash);
                    return Ok(false);
                }
                tracing::info!("✅ Transaction {} confirmed on chain", tx_hash);
            } else {
                tracing::debug!(
                    "Chain TX {} present but no verifier provided (skipping chain verification)",
                    tx_hash
                );
            }
        }

        Ok(true)
    }

    /// Check if proof is on-chain
    pub fn is_on_chain(&self) -> bool {
        self.chain_tx_hash.is_some()
    }

    /// Submit this delivery proof to the blockchain
    /// Returns the transaction ID if successful
    pub async fn submit_to_chain(
        &mut self,
        blockchain_client: &BlockchainClient,
    ) -> Result<uuid::Uuid> {
        use chrono::{DateTime, Utc};

        // Ensure we have a recipient signature
        let signature = self
            .recipient_signature
            .as_ref()
            .ok_or_else(|| Error::validation("Cannot submit proof without recipient signature"))?;

        // Convert SystemTime to DateTime<Utc>
        let timestamp = self
            .timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| Error::internal(format!("Invalid timestamp: {}", e)))?;
        let datetime = DateTime::<Utc>::from_timestamp(timestamp.as_secs() as i64, 0)
            .ok_or_else(|| Error::internal("Failed to convert timestamp"))?;

        // Submit to blockchain
        let tx_id = blockchain_client
            .submit_delivery_proof(
                self.message_id,
                self.relay_peer_id.clone(),
                self.recipient_id.clone(),
                &signature.0,
                datetime,
                self.content_hash.clone(),
                self.reward_amount,
            )
            .await?;

        // Store transaction hash (use tx_id as hash for now)
        self.chain_tx_hash = Some(tx_id.to_string());

        tracing::info!(
            "✅ Delivery proof submitted to chain: message={}, tx={}",
            self.message_id.0,
            tx_id
        );

        Ok(tx_id)
    }

    /// Wait for the blockchain transaction to be confirmed
    pub async fn wait_for_confirmation(
        &self,
        blockchain_client: &BlockchainClient,
        timeout_secs: u64,
    ) -> Result<bool> {
        let tx_hash = self
            .chain_tx_hash
            .as_ref()
            .ok_or_else(|| Error::validation("No chain transaction to wait for"))?;

        let tx_id = uuid::Uuid::parse_str(tx_hash)
            .map_err(|e| Error::validation(format!("Invalid transaction ID: {}", e)))?;

        // Poll with timeout
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            match blockchain_client.is_transaction_confirmed(tx_id).await {
                Ok(confirmed) => {
                    if confirmed {
                        tracing::info!("✅ Delivery proof confirmed on-chain: {}", tx_id);
                        return Ok(true);
                    }
                }
                Err(e) => {
                    tracing::warn!("Error checking confirmation status: {}", e);
                }
            }

            // Wait before next poll (exponential backoff)
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        tracing::warn!(
            "⏱️ Timeout waiting for delivery proof confirmation: {}",
            tx_id
        );
        Ok(false)
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
        let recipient_id = dchat_core::types::UserId(uuid::Uuid::new_v4());

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
            recipient_id,
            content_hash: "test_hash".to_string(),
            recipient_signature: Some(Signature(vec![1, 2, 3])),
            timestamp: SystemTime::now(),
            chain_tx_hash: None,
            reward_amount: 100,
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

    #[tokio::test]
    async fn test_delivery_proof_blockchain_submission() {
        use ed25519_dalek::{SigningKey, Signer};
        use rand::rngs::OsRng;

        // Generate test keys
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        // Create message to sign
        let msg_id = MessageId(uuid::Uuid::new_v4());
        let relay_peer_id = "relay_test_123";
        let recipient_id = dchat_core::types::UserId(uuid::Uuid::new_v4());
        let timestamp = SystemTime::now();

        let mut message_bytes = Vec::new();
        message_bytes.extend_from_slice(msg_id.0.as_bytes());
        message_bytes.extend_from_slice(relay_peer_id.as_bytes());
        if let Ok(duration) = timestamp.duration_since(std::time::UNIX_EPOCH) {
            message_bytes.extend_from_slice(&duration.as_secs().to_le_bytes());
        }

        // Sign message
        let signature = signing_key.sign(&message_bytes);

        // Create delivery proof
        let mut proof = DeliveryProof {
            message_id: msg_id,
            relay_peer_id: relay_peer_id.to_string(),
            recipient_id,
            content_hash: "test_hash_12345".to_string(),
            recipient_signature: Some(Signature(signature.to_bytes().to_vec())),
            timestamp,
            chain_tx_hash: None,
            reward_amount: 100,
        };

        // Verify proof before submission
        assert!(proof.verify(verifying_key.as_bytes()).unwrap());

        // Submit to blockchain
        let blockchain_client = BlockchainClient::default();
        let tx_id = proof
            .submit_to_chain(&blockchain_client)
            .await
            .expect("Failed to submit proof");

        // Verify transaction was stored
        assert!(proof.is_on_chain());
        assert_eq!(proof.chain_tx_hash, Some(tx_id.to_string()));

        // Verify transaction exists in blockchain
        let tx = blockchain_client.get_transaction(tx_id);
        assert!(tx.is_some());
        let tx = tx.unwrap();
        assert_eq!(
            tx.tx_type,
            dchat_chain::TransactionType::SubmitDeliveryProof
        );
    }

    #[tokio::test]
    async fn test_delivery_proof_submission_without_signature() {
        let msg_id = MessageId(uuid::Uuid::new_v4());
        let recipient_id = dchat_core::types::UserId(uuid::Uuid::new_v4());

        // Create proof without signature
        let mut proof = DeliveryProof {
            message_id: msg_id,
            relay_peer_id: "relay_test".to_string(),
            recipient_id,
            content_hash: "test_hash".to_string(),
            recipient_signature: None,
            timestamp: SystemTime::now(),
            chain_tx_hash: None,
            reward_amount: 100,
        };

        let blockchain_client = BlockchainClient::default();
        let result = proof.submit_to_chain(&blockchain_client).await;

        // Should fail without signature
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("recipient signature"));
    }

    #[tokio::test]
    async fn test_delivery_proof_confirmation_timeout() {
        use ed25519_dalek::{SigningKey, Signer};
        use rand::rngs::OsRng;

        // Generate test keys
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);

        // Create message and sign it
        let msg_id = MessageId(uuid::Uuid::new_v4());
        let relay_peer_id = "relay_timeout_test";
        let recipient_id = dchat_core::types::UserId(uuid::Uuid::new_v4());
        let timestamp = SystemTime::now();

        let mut message_bytes = Vec::new();
        message_bytes.extend_from_slice(msg_id.0.as_bytes());
        message_bytes.extend_from_slice(relay_peer_id.as_bytes());
        if let Ok(duration) = timestamp.duration_since(std::time::UNIX_EPOCH) {
            message_bytes.extend_from_slice(&duration.as_secs().to_le_bytes());
        }

        let signature = signing_key.sign(&message_bytes);

        // Create and submit proof
        let mut proof = DeliveryProof {
            message_id: msg_id,
            relay_peer_id: relay_peer_id.to_string(),
            recipient_id,
            content_hash: "test_hash".to_string(),
            recipient_signature: Some(Signature(signature.to_bytes().to_vec())),
            timestamp,
            chain_tx_hash: None,
            reward_amount: 100,
        };

        let blockchain_client = BlockchainClient::default();
        proof
            .submit_to_chain(&blockchain_client)
            .await
            .expect("Failed to submit");

        // Wait with short timeout (should timeout since we don't confirm blocks)
        let confirmed = proof
            .wait_for_confirmation(&blockchain_client, 2)
            .await
            .expect("Wait failed");

        // Should timeout without manual confirmation
        assert!(!confirmed);
    }

    #[test]
    fn test_delivery_proof_with_new_fields() {
        let msg_id = MessageId(uuid::Uuid::new_v4());
        let recipient_id = dchat_core::types::UserId(uuid::Uuid::new_v4());

        let proof = DeliveryProof {
            message_id: msg_id,
            relay_peer_id: "relay123".to_string(),
            recipient_id,
            content_hash: "sha256_hash_here".to_string(),
            recipient_signature: None,
            timestamp: SystemTime::now(),
            chain_tx_hash: None,
            reward_amount: 250,
        };

        assert_eq!(proof.relay_peer_id, "relay123");
        assert_eq!(proof.recipient_id, recipient_id);
        assert_eq!(proof.content_hash, "sha256_hash_here");
        assert_eq!(proof.reward_amount, 250);
        assert!(!proof.is_on_chain());
    }
}

/// Chain verification interface for delivery proofs
#[async_trait::async_trait]
pub trait ChainVerifier: Send + Sync {
    /// Verify that a transaction is confirmed on-chain
    /// Returns true if transaction exists and has sufficient confirmations
    async fn verify_transaction_confirmed(&self, tx_hash: &str) -> Result<bool>;

    /// Get transaction confirmation depth
    async fn get_confirmation_depth(&self, tx_hash: &str) -> Result<u64>;
}

/// Production implementation using BlockchainClient
pub struct ProductionChainVerifier {
    client: Arc<BlockchainClient>,
    required_confirmations: u32,
    /// Cache of recently verified transactions (tx_hash -> confirmed)
    confirmation_cache: Arc<tokio::sync::RwLock<HashMap<String, (bool, SystemTime)>>>,
    /// Cache TTL (default 5 minutes)
    cache_ttl: Duration,
}

impl ProductionChainVerifier {
    /// Create new verifier with blockchain client
    pub fn new(client: Arc<BlockchainClient>, required_confirmations: u32) -> Self {
        Self {
            client,
            required_confirmations,
            confirmation_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Check if cached value is still valid
    async fn check_cache(&self, tx_hash: &str) -> Option<bool> {
        let cache = self.confirmation_cache.read().await;
        if let Some((confirmed, timestamp)) = cache.get(tx_hash) {
            if let Ok(elapsed) = timestamp.elapsed() {
                if elapsed < self.cache_ttl {
                    tracing::debug!("Using cached confirmation for {}", tx_hash);
                    return Some(*confirmed);
                }
            }
        }
        None
    }

    /// Update cache with new confirmation status
    async fn update_cache(&self, tx_hash: String, confirmed: bool) {
        let mut cache = self.confirmation_cache.write().await;
        cache.insert(tx_hash, (confirmed, SystemTime::now()));
    }
}

#[async_trait::async_trait]
impl ChainVerifier for ProductionChainVerifier {
    async fn verify_transaction_confirmed(&self, tx_hash: &str) -> Result<bool> {
        // Check cache first
        if let Some(cached) = self.check_cache(tx_hash).await {
            return Ok(cached);
        }

        // Parse tx_hash as UUID (dchat transactions use UUIDs)
        let tx_id = uuid::Uuid::parse_str(tx_hash)
            .map_err(|e| Error::validation(format!("Invalid transaction hash: {}", e)))?;

        // Query blockchain for confirmation
        let confirmed = self.client.is_transaction_confirmed(tx_id).await?;

        // If confirmed, verify confirmation depth
        if confirmed {
            if let Some(status) = self.client.get_transaction_status(tx_id) {
                match status {
                    TransactionStatus::Confirmed {
                        block_height,
                        block_hash: _,
                    } => {
                        let current_block = self.client.get_current_height().await
                            .unwrap_or(block_height);
                        let confirmations = current_block.saturating_sub(block_height);

                        if confirmations < self.required_confirmations as u64 {
                            tracing::debug!(
                                "Transaction {} has {} confirmations (need {})",
                                tx_hash,
                                confirmations,
                                self.required_confirmations
                            );
                            self.update_cache(tx_hash.to_string(), false).await;
                            return Ok(false);
                        }

                        tracing::info!(
                            "✅ Transaction {} confirmed with {} confirmations",
                            tx_hash,
                            confirmations
                        );
                        self.update_cache(tx_hash.to_string(), true).await;
                        return Ok(true);
                    }
                    _ => {
                        self.update_cache(tx_hash.to_string(), false).await;
                        return Ok(false);
                    }
                }
            }
        }

        self.update_cache(tx_hash.to_string(), false).await;
        Ok(false)
    }

    async fn get_confirmation_depth(&self, tx_hash: &str) -> Result<u64> {
        let tx_id = uuid::Uuid::parse_str(tx_hash)
            .map_err(|e| Error::validation(format!("Invalid transaction hash: {}", e)))?;

        if let Some(status) = self.client.get_transaction_status(tx_id) {
            match status {
                TransactionStatus::Confirmed {
                    block_height,
                    block_hash: _,
                } => {
                    let current_block = self.client.get_current_height().await
                        .unwrap_or(block_height);
                    Ok(current_block.saturating_sub(block_height))
                }
                _ => Ok(0),
            }
        } else {
            Ok(0)
        }
    }
}
