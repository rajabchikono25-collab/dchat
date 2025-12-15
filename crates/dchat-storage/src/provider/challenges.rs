//! Storage Challenges and Proof System
//!
//! Implements the auditable proof+payment+slashing loop:
//! - Periodic random challenges over stored objects
//! - Byte-range retrieval + signed proofs
//! - Success earns micropayments
//! - Failure reduces reputation and can slash bonds
//! - Receipts stored in database and optionally anchored on-chain

use super::blob_ref::BlobRef;
use super::registry::ProviderRegistry;
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::error::{StorageError, StorageResult};

/// Storage challenge sent to a provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageChallenge {
    /// Unique challenge ID
    pub id: [u8; 32],
    /// Provider being challenged
    pub provider_id: [u8; 32],
    /// Blob being challenged
    pub blob_hash: [u8; 32],
    /// Byte range start (inclusive)
    pub range_start: u64,
    /// Byte range end (exclusive)
    pub range_end: u64,
    /// Random nonce for this challenge
    pub nonce: [u8; 16],
    /// Challenge timestamp
    pub created_at: DateTime<Utc>,
    /// Challenge deadline
    pub deadline: DateTime<Utc>,
    /// Expected hash of the byte range
    pub expected_hash: Option<[u8; 32]>,
    /// Challenge status
    pub status: ChallengeStatus,
    /// Micropayment amount (if successful)
    pub payment_amount: u64,
    /// Slashing amount (if failed)
    pub slash_amount: u64,
}

impl StorageChallenge {
    /// Create a new challenge
    pub fn new(
        provider_id: [u8; 32],
        blob_hash: [u8; 32],
        blob_size: u64,
        payment_amount: u64,
        slash_amount: u64,
        deadline_seconds: u64,
    ) -> Self {
        let mut rng = rand::thread_rng();

        // Generate random byte range (at least 1KB, at most 1MB or blob size)
        let max_range = blob_size.min(1024 * 1024);
        let min_range = blob_size.min(1024);
        let range_size = rng.gen_range(min_range..=max_range);
        let range_start = rng.gen_range(0..=blob_size.saturating_sub(range_size));
        let range_end = range_start + range_size;

        // Generate challenge ID and nonce
        let mut id = [0u8; 32];
        let mut nonce = [0u8; 16];
        rng.fill_bytes(&mut id);
        rng.fill_bytes(&mut nonce);

        let now = Utc::now();

        Self {
            id,
            provider_id,
            blob_hash,
            range_start,
            range_end,
            nonce,
            created_at: now,
            deadline: now + Duration::seconds(deadline_seconds as i64),
            expected_hash: None,
            status: ChallengeStatus::Pending,
            payment_amount,
            slash_amount,
        }
    }

    /// Get challenge ID as hex
    pub fn id_hex(&self) -> String {
        hex::encode(self.id)
    }

    /// Check if challenge has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.deadline
    }

    /// Compute expected hash from content range
    pub fn set_expected_hash(&mut self, content: &[u8]) {
        let mut hasher = Sha256::new();
        hasher.update(&self.nonce);
        hasher.update(content);
        self.expected_hash = Some(hasher.finalize().into());
    }

    /// Verify a proof against expected hash
    pub fn verify_proof(&self, proof: &ChallengeProof) -> bool {
        // Check challenge ID matches
        if proof.challenge_id != self.id {
            return false;
        }

        // Check expected hash
        if let Some(expected) = self.expected_hash {
            // Compute hash of provided data with nonce
            let mut hasher = Sha256::new();
            hasher.update(&self.nonce);
            hasher.update(&proof.data);
            let computed: [u8; 32] = hasher.finalize().into();

            if computed != expected {
                return false;
            }
        }

        // Verify signature
        self.verify_signature(proof)
    }

    /// Verify provider signature on proof
    fn verify_signature(&self, proof: &ChallengeProof) -> bool {
        // Construct signed message
        let mut message = Vec::new();
        message.extend_from_slice(&proof.challenge_id);
        message.extend_from_slice(&self.nonce);
        message.extend_from_slice(&proof.data_hash);

        // Verify signature
        if proof.signature.len() != 64 || proof.provider_pubkey.len() != 32 {
            return false;
        }

        let sig_bytes: [u8; 64] = proof.signature.as_slice().try_into().unwrap_or([0u8; 64]);
        let key_bytes: [u8; 32] = proof
            .provider_pubkey
            .as_slice()
            .try_into()
            .unwrap_or([0u8; 32]);

        let signature = Signature::from_bytes(&sig_bytes);

        let Ok(verifying_key) = VerifyingKey::from_bytes(&key_bytes) else {
            return false;
        };

        verifying_key.verify(&message, &signature).is_ok()
    }
}

/// Challenge status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChallengeStatus {
    /// Challenge created, awaiting response
    Pending,
    /// Response received, being verified
    Verifying,
    /// Challenge passed
    Passed,
    /// Challenge failed
    Failed,
    /// Challenge expired without response
    Expired,
    /// Payment issued for passed challenge
    Paid,
    /// Slashing executed for failed challenge
    Slashed,
}

/// Proof submitted by provider for a challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeProof {
    /// Challenge ID this proof is for
    pub challenge_id: [u8; 32],
    /// The requested byte range data
    pub data: Vec<u8>,
    /// Hash of the data (SHA-256)
    pub data_hash: [u8; 32],
    /// Provider's public key
    pub provider_pubkey: Vec<u8>,
    /// Provider's signature over (challenge_id || nonce || data_hash)
    pub signature: Vec<u8>,
    /// Timestamp of proof submission
    pub submitted_at: DateTime<Utc>,
}

impl ChallengeProof {
    /// Create a new proof
    pub fn create(challenge: &StorageChallenge, data: Vec<u8>, signing_key: &SigningKey) -> Self {
        // Compute data hash
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let data_hash: [u8; 32] = hasher.finalize().into();

        // Create signed message
        let mut message = Vec::new();
        message.extend_from_slice(&challenge.id);
        message.extend_from_slice(&challenge.nonce);
        message.extend_from_slice(&data_hash);

        let signature = signing_key.sign(&message);

        Self {
            challenge_id: challenge.id,
            data,
            data_hash,
            provider_pubkey: signing_key.verifying_key().as_bytes().to_vec(),
            signature: signature.to_bytes().to_vec(),
            submitted_at: Utc::now(),
        }
    }
}

/// Result of a challenge verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResult {
    /// Challenge ID
    pub challenge_id: [u8; 32],
    /// Provider ID
    pub provider_id: [u8; 32],
    /// Whether challenge passed
    pub passed: bool,
    /// Failure reason (if failed)
    pub failure_reason: Option<String>,
    /// Payment transaction ID (if passed)
    pub payment_tx_id: Option<String>,
    /// Slashing transaction ID (if failed)
    pub slash_tx_id: Option<String>,
    /// Result timestamp
    pub timestamp: DateTime<Utc>,
}

/// Storage challenge manager
pub struct StorageChallengeManager {
    /// Database pool for persistence
    pool: Option<PgPool>,
    /// Provider registry
    registry: Arc<ProviderRegistry>,
    /// Pending challenges
    pending_challenges: Arc<RwLock<HashMap<[u8; 32], StorageChallenge>>>,
    /// Challenge configuration
    config: ChallengeConfig,
    /// HTTP client for chain RPC
    http_client: reqwest::Client,
    /// Chain RPC endpoint
    chain_rpc_endpoint: String,
}

/// Challenge configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeConfig {
    /// Challenge interval (how often to challenge each blob)
    pub challenge_interval_hours: u64,
    /// Challenge deadline (seconds to respond)
    pub challenge_deadline_secs: u64,
    /// Base payment per successful challenge (in token units)
    pub base_payment: u64,
    /// Base slash amount per failed challenge
    pub base_slash: u64,
    /// Number of consecutive failures before slashing
    pub failures_before_slash: u32,
    /// Whether to anchor receipts on-chain
    pub anchor_on_chain: bool,
    /// Offline mode (skip chain operations)
    #[serde(default)]
    pub offline_mode: bool,
}

impl Default for ChallengeConfig {
    fn default() -> Self {
        Self {
            challenge_interval_hours: 24,
            challenge_deadline_secs: 300, // 5 minutes
            base_payment: 1000_0000,      // 0.001 DCHAT
            base_slash: 10_0000_0000,     // 0.1 DCHAT
            failures_before_slash: 3,
            anchor_on_chain: true,
            offline_mode: false,
        }
    }
}

impl StorageChallengeManager {
    /// Create a new challenge manager
    pub async fn new(
        registry: Arc<ProviderRegistry>,
        database_url: &str,
        chain_rpc_endpoint: &str,
        config: ChallengeConfig,
    ) -> StorageResult<Self> {
        let pool = if !config.offline_mode {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(database_url)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;

            Self::init_schema(&pool).await?;
            Some(pool)
        } else {
            None
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        Ok(Self {
            pool,
            registry,
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
            config,
            http_client,
            chain_rpc_endpoint: chain_rpc_endpoint.to_string(),
        })
    }

    /// Create manager in offline mode
    pub fn offline(registry: Arc<ProviderRegistry>, config: ChallengeConfig) -> Self {
        Self {
            pool: None,
            registry,
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
            config,
            http_client: reqwest::Client::new(),
            chain_rpc_endpoint: String::new(),
        }
    }

    /// Initialize database schema
    async fn init_schema(pool: &PgPool) -> StorageResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS storage_challenges (
                id BYTEA PRIMARY KEY,
                provider_id BYTEA NOT NULL,
                blob_hash BYTEA NOT NULL,
                range_start BIGINT NOT NULL,
                range_end BIGINT NOT NULL,
                nonce BYTEA NOT NULL,
                created_at TIMESTAMPTZ NOT NULL,
                deadline TIMESTAMPTZ NOT NULL,
                expected_hash BYTEA,
                status TEXT NOT NULL,
                payment_amount BIGINT NOT NULL,
                slash_amount BIGINT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS challenge_results (
                challenge_id BYTEA PRIMARY KEY,
                provider_id BYTEA NOT NULL,
                passed BOOLEAN NOT NULL,
                failure_reason TEXT,
                payment_tx_id TEXT,
                slash_tx_id TEXT,
                timestamp TIMESTAMPTZ NOT NULL,
                chain_anchor_tx TEXT
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        // Create indexes
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_challenges_provider ON storage_challenges(provider_id)",
        )
        .execute(pool)
        .await
        .ok();

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_challenges_status ON storage_challenges(status)",
        )
        .execute(pool)
        .await
        .ok();

        info!("Challenge manager schema initialized");
        Ok(())
    }

    /// Create a new challenge for a blob
    pub async fn create_challenge(
        &self,
        provider_id: [u8; 32],
        blob: &BlobRef,
        expected_content: Option<&[u8]>,
    ) -> StorageResult<StorageChallenge> {
        let mut challenge = StorageChallenge::new(
            provider_id,
            blob.hash,
            blob.size,
            self.config.base_payment,
            self.config.base_slash,
            self.config.challenge_deadline_secs,
        );

        // Set expected hash if content provided
        if let Some(content) = expected_content {
            let range_content =
                &content[challenge.range_start as usize..challenge.range_end as usize];
            challenge.set_expected_hash(range_content);
        }

        // Persist challenge
        if let Some(pool) = &self.pool {
            self.persist_challenge(pool, &challenge).await?;
        }

        // Add to pending challenges
        {
            let mut pending = self.pending_challenges.write().await;
            pending.insert(challenge.id, challenge.clone());
        }

        info!(
            "Created challenge {} for provider {}",
            challenge.id_hex(),
            hex::encode(provider_id)
        );

        Ok(challenge)
    }

    /// Persist challenge to database
    async fn persist_challenge(
        &self,
        pool: &PgPool,
        challenge: &StorageChallenge,
    ) -> StorageResult<()> {
        sqlx::query(
            r#"
            INSERT INTO storage_challenges
            (id, provider_id, blob_hash, range_start, range_end, nonce, created_at, deadline,
             expected_hash, status, payment_amount, slash_amount)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(&challenge.id[..])
        .bind(&challenge.provider_id[..])
        .bind(&challenge.blob_hash[..])
        .bind(challenge.range_start as i64)
        .bind(challenge.range_end as i64)
        .bind(&challenge.nonce[..])
        .bind(challenge.created_at)
        .bind(challenge.deadline)
        .bind(challenge.expected_hash.as_ref().map(|h| &h[..]))
        .bind(format!("{:?}", challenge.status))
        .bind(challenge.payment_amount as i64)
        .bind(challenge.slash_amount as i64)
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    /// Submit a proof for a challenge
    pub async fn submit_proof(&self, proof: ChallengeProof) -> StorageResult<ChallengeResult> {
        // Get challenge
        let challenge = {
            let pending = self.pending_challenges.read().await;
            pending.get(&proof.challenge_id).cloned()
        };

        let Some(mut challenge) = challenge else {
            return Err(StorageError::NotFound("Challenge not found".to_string()));
        };

        // Check deadline
        if challenge.is_expired() {
            challenge.status = ChallengeStatus::Expired;
            return self
                .finalize_challenge(&challenge, false, Some("Challenge expired"))
                .await;
        }

        // Verify proof
        challenge.status = ChallengeStatus::Verifying;
        let passed = challenge.verify_proof(&proof);

        if passed {
            challenge.status = ChallengeStatus::Passed;
            self.finalize_challenge(&challenge, true, None).await
        } else {
            challenge.status = ChallengeStatus::Failed;
            self.finalize_challenge(&challenge, false, Some("Proof verification failed"))
                .await
        }
    }

    /// Finalize a challenge (payment or slashing)
    async fn finalize_challenge(
        &self,
        challenge: &StorageChallenge,
        passed: bool,
        failure_reason: Option<&str>,
    ) -> StorageResult<ChallengeResult> {
        let mut result = ChallengeResult {
            challenge_id: challenge.id,
            provider_id: challenge.provider_id,
            passed,
            failure_reason: failure_reason.map(|s| s.to_string()),
            payment_tx_id: None,
            slash_tx_id: None,
            timestamp: Utc::now(),
        };

        // Update provider reputation
        self.registry
            .update_reputation(&challenge.provider_id, passed)
            .await?;

        if passed {
            // Issue micropayment
            if !self.config.offline_mode {
                result.payment_tx_id = self
                    .issue_payment(&challenge.provider_id, challenge.payment_amount)
                    .await
                    .ok();
            }
            info!("Challenge {} passed, payment issued", challenge.id_hex());
        } else {
            // Check if we need to slash
            let provider = self.registry.get_provider(&challenge.provider_id).await?;
            if let Some(p) = provider {
                if p.failed_challenges >= self.config.failures_before_slash as u64 {
                    // Execute slashing
                    if !self.config.offline_mode {
                        result.slash_tx_id = self
                            .execute_slashing(&challenge.provider_id, challenge.slash_amount)
                            .await
                            .ok();
                    }
                    warn!(
                        "Challenge {} failed, provider {} slashed",
                        challenge.id_hex(),
                        hex::encode(challenge.provider_id)
                    );
                }
            }
        }

        // Persist result
        if let Some(pool) = &self.pool {
            self.persist_result(pool, &result).await?;

            // Anchor on chain if configured
            if self.config.anchor_on_chain && !self.config.offline_mode {
                if let Err(e) = self.anchor_result_on_chain(&result).await {
                    warn!("Failed to anchor result on chain: {}", e);
                }
            }
        }

        // Remove from pending
        {
            let mut pending = self.pending_challenges.write().await;
            pending.remove(&challenge.id);
        }

        Ok(result)
    }

    /// Persist challenge result
    async fn persist_result(&self, pool: &PgPool, result: &ChallengeResult) -> StorageResult<()> {
        sqlx::query(
            r#"
            INSERT INTO challenge_results
            (challenge_id, provider_id, passed, failure_reason, payment_tx_id, slash_tx_id, timestamp)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(&result.challenge_id[..])
        .bind(&result.provider_id[..])
        .bind(result.passed)
        .bind(&result.failure_reason)
        .bind(&result.payment_tx_id)
        .bind(&result.slash_tx_id)
        .bind(result.timestamp)
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        // Update challenge status
        let status = if result.passed {
            ChallengeStatus::Paid
        } else {
            ChallengeStatus::Slashed
        };

        sqlx::query("UPDATE storage_challenges SET status = $1 WHERE id = $2")
            .bind(format!("{:?}", status))
            .bind(&result.challenge_id[..])
            .execute(pool)
            .await
            .ok();

        Ok(())
    }

    /// Issue micropayment to provider
    async fn issue_payment(&self, provider_id: &[u8; 32], amount: u64) -> StorageResult<String> {
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.storage_micropayment",
            "params": {
                "provider_id": hex::encode(provider_id),
                "amount": amount,
                "reason": "challenge_reward"
            }
        });

        let response = self
            .http_client
            .post(&self.chain_rpc_endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| StorageError::Chain("Missing tx_id".to_string()))
    }

    /// Execute slashing against provider bond
    async fn execute_slashing(&self, provider_id: &[u8; 32], amount: u64) -> StorageResult<String> {
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.slash_storage_bond",
            "params": {
                "provider_id": hex::encode(provider_id),
                "amount": amount,
                "reason": "challenge_failure"
            }
        });

        let response = self
            .http_client
            .post(&self.chain_rpc_endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| StorageError::Chain("Missing tx_id".to_string()))
    }

    /// Anchor challenge result on-chain
    async fn anchor_result_on_chain(&self, result: &ChallengeResult) -> StorageResult<String> {
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "chat.anchor_storage_proof",
            "params": {
                "challenge_id": hex::encode(result.challenge_id),
                "provider_id": hex::encode(result.provider_id),
                "passed": result.passed,
                "timestamp": result.timestamp.to_rfc3339()
            }
        });

        let response = self
            .http_client
            .post(&self.chain_rpc_endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| StorageError::Chain("Missing tx_id".to_string()))
    }

    /// Process expired challenges
    pub async fn process_expired_challenges(&self) -> StorageResult<u64> {
        let mut expired_count = 0;

        let expired: Vec<StorageChallenge> = {
            let pending = self.pending_challenges.read().await;
            pending
                .values()
                .filter(|c| c.is_expired())
                .cloned()
                .collect()
        };

        for challenge in expired {
            self.finalize_challenge(&challenge, false, Some("Challenge expired"))
                .await?;
            expired_count += 1;
        }

        if expired_count > 0 {
            info!("Processed {} expired challenges", expired_count);
        }

        Ok(expired_count)
    }

    /// Get challenge statistics
    pub async fn get_statistics(&self) -> ChallengeStatistics {
        let pending = self.pending_challenges.read().await;

        ChallengeStatistics {
            pending_challenges: pending.len(),
            total_pending_value: pending.values().map(|c| c.payment_amount).sum(),
        }
    }
}

/// Challenge statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeStatistics {
    pub pending_challenges: usize,
    pub total_pending_value: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_blob() -> BlobRef {
        BlobRef::from_content(
            b"test content",
            crate::provider::blob_ref::BlobCodec::Raw,
            "text/plain",
            false,
        )
    }

    #[test]
    fn test_challenge_creation() {
        let provider_id = [1u8; 32];
        let blob = create_test_blob();

        let challenge = StorageChallenge::new(provider_id, blob.hash, blob.size, 1000, 10000, 300);

        assert_eq!(challenge.provider_id, provider_id);
        assert_eq!(challenge.blob_hash, blob.hash);
        assert!(challenge.range_end <= blob.size);
        assert!(!challenge.is_expired());
    }

    #[test]
    fn test_proof_creation_and_verification() {
        let provider_id = [1u8; 32];
        let content = b"Hello, World! This is test content for challenge verification.";
        let blob = BlobRef::from_content(
            content,
            crate::provider::blob_ref::BlobCodec::Raw,
            "text/plain",
            false,
        );

        let mut challenge =
            StorageChallenge::new(provider_id, blob.hash, blob.size, 1000, 10000, 300);

        // Set expected hash from actual content range
        let range_content = &content[challenge.range_start as usize..challenge.range_end as usize];
        challenge.set_expected_hash(range_content);

        // Create proof with signing key
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let proof = ChallengeProof::create(&challenge, range_content.to_vec(), &signing_key);

        // Verify proof
        assert!(challenge.verify_proof(&proof));
    }

    #[test]
    fn test_challenge_expiry() {
        let mut challenge = StorageChallenge::new(
            [1u8; 32], [2u8; 32], 1024, 1000, 10000, 0, // Immediate deadline
        );

        // Should be expired immediately
        challenge.deadline = Utc::now() - Duration::seconds(1);
        assert!(challenge.is_expired());
    }
}
