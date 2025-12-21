//! Intent system - cross-chain Intent→Execute→Receipt pattern

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, Bytes};
use uuid::Uuid;

use crate::error::{MiniAppError, MiniAppResult};
use crate::registry::AppId;
use crate::{INTENT_EXPIRY_SECONDS, MAX_INTENTS_PER_MINUTE};

/// Intent ID (unique identifier)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IntentId(pub Uuid);

impl IntentId {
    /// Generate new ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse from string
    pub fn parse(s: &str) -> MiniAppResult<Self> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|_| MiniAppError::InvalidIntent("invalid intent ID".to_string()))
    }
}

impl Default for IntentId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for IntentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Intent status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentStatus {
    /// Intent created, pending signature
    Created,
    /// Intent signed, pending submission
    Signed,
    /// Intent submitted to chat chain
    Pending,
    /// Intent being executed on currency chain
    Executing,
    /// Intent executed successfully
    Executed,
    /// Intent failed
    Failed,
    /// Intent expired
    Expired,
    /// Intent cancelled by user
    Cancelled,
}

impl IntentStatus {
    /// Check if intent is terminal (no more state changes)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Executed | Self::Failed | Self::Expired | Self::Cancelled
        )
    }

    /// Check if intent is pending (waiting for execution)
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending | Self::Executing)
    }
}

/// Intent type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum IntentType {
    /// Transfer tokens
    Transfer {
        /// Recipient address
        recipient: [u8; 32],
        /// Token mint (or native if None)
        mint: Option<[u8; 32]>,
        /// Amount in smallest unit
        amount: u64,
        /// Optional memo
        memo: Option<String>,
    },

    /// Execute program instruction
    ProgramCall {
        /// Program ID
        program_id: [u8; 32],
        /// Instruction data
        instruction_data: Vec<u8>,
        /// Account metas
        accounts: Vec<IntentAccountMeta>,
    },

    /// Batch of operations
    Batch {
        /// Operations to execute atomically
        operations: Vec<IntentType>,
    },

    /// Stake tokens
    Stake {
        /// Validator address
        validator: [u8; 32],
        /// Amount to stake
        amount: u64,
    },

    /// Unstake tokens
    Unstake {
        /// Stake account
        stake_account: [u8; 32],
        /// Amount to unstake
        amount: u64,
    },

    /// Vote on proposal
    Vote {
        /// Proposal ID
        proposal_id: [u8; 32],
        /// Vote option
        vote: VoteOption,
    },
}

/// Account meta for intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAccountMeta {
    /// Account pubkey
    pub pubkey: [u8; 32],
    /// Is signer
    pub is_signer: bool,
    /// Is writable
    pub is_writable: bool,
}

/// Vote option
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteOption {
    /// Vote yes
    Yes,
    /// Vote no
    No,
    /// Abstain
    Abstain,
}

/// Intent payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentPayload {
    /// Intent type and data
    pub intent_type: IntentType,
    /// Sender's address
    pub sender: [u8; 32],
    /// Nonce (prevents replay)
    pub nonce: u64,
    /// Chain ID (prevents cross-chain replay)
    pub chain_id: u64,
    /// Maximum fee willing to pay
    pub max_fee: u64,
    /// Deadline timestamp
    pub deadline: DateTime<Utc>,
}

impl IntentPayload {
    /// Create new payload
    pub fn new(sender: [u8; 32], intent_type: IntentType, max_fee: u64, chain_id: u64) -> Self {
        Self {
            intent_type,
            sender,
            nonce: rand::random(),
            chain_id,
            max_fee,
            deadline: Utc::now() + Duration::seconds(INTENT_EXPIRY_SECONDS as i64),
        }
    }

    /// Compute hash for signing
    pub fn hash(&self) -> [u8; 32] {
        let serialized = bincode::serialize(self).expect("serializable");
        blake3::hash(&serialized).into()
    }

    /// Validate payload
    pub fn validate(&self) -> MiniAppResult<()> {
        // Check deadline
        if Utc::now() > self.deadline {
            return Err(MiniAppError::IntentExpired(
                "intent deadline passed".to_string(),
            ));
        }

        // Check max fee is reasonable
        if self.max_fee == 0 {
            return Err(MiniAppError::InvalidIntent(
                "max_fee must be > 0".to_string(),
            ));
        }

        // Validate intent type specific constraints
        match &self.intent_type {
            IntentType::Transfer { amount, .. } => {
                if *amount == 0 {
                    return Err(MiniAppError::InvalidIntent(
                        "transfer amount must be > 0".to_string(),
                    ));
                }
            }
            IntentType::ProgramCall { accounts, .. } => {
                if accounts.len() > 64 {
                    return Err(MiniAppError::InvalidIntent(
                        "too many accounts (max 64)".to_string(),
                    ));
                }
            }
            IntentType::Batch { operations } => {
                if operations.is_empty() {
                    return Err(MiniAppError::InvalidIntent(
                        "batch cannot be empty".to_string(),
                    ));
                }
                if operations.len() > 10 {
                    return Err(MiniAppError::InvalidIntent(
                        "batch too large (max 10 operations)".to_string(),
                    ));
                }
            }
            IntentType::Stake { amount, .. } | IntentType::Unstake { amount, .. } => {
                if *amount == 0 {
                    return Err(MiniAppError::InvalidIntent(
                        "stake amount must be > 0".to_string(),
                    ));
                }
            }
            IntentType::Vote { .. } => {}
        }

        Ok(())
    }

    /// Estimate size in bytes
    pub fn size(&self) -> usize {
        bincode::serialize(self).map(|v| v.len()).unwrap_or(0)
    }
}

/// Full intent with ID and signature
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Intent ID
    pub id: IntentId,
    /// App that initiated the intent
    pub app_id: AppId,
    /// Intent payload
    pub payload: IntentPayload,
    /// User signature (Ed25519 64-byte signature)
    #[serde_as(as = "Option<Bytes>")]
    pub signature: Option<[u8; 64]>,
    /// Status
    pub status: IntentStatus,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,
    /// Error message if failed
    pub error: Option<String>,
    /// Receipt ID if executed
    pub receipt_id: Option<crate::receipt::ReceiptId>,
}

impl Intent {
    /// Create new intent
    pub fn new(app_id: AppId, payload: IntentPayload) -> Self {
        let now = Utc::now();
        Self {
            id: IntentId::new(),
            app_id,
            payload,
            signature: None,
            status: IntentStatus::Created,
            created_at: now,
            updated_at: now,
            error: None,
            receipt_id: None,
        }
    }

    /// Sign the intent
    pub fn sign(&mut self, signing_key: &SigningKey) -> MiniAppResult<()> {
        let hash = self.payload.hash();
        let signature = signing_key.sign(&hash);
        self.signature = Some(signature.to_bytes());
        self.status = IntentStatus::Signed;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Verify signature
    pub fn verify_signature(&self, verifying_key: &VerifyingKey) -> MiniAppResult<()> {
        let signature_bytes = self.signature.ok_or(MiniAppError::IntentSignatureInvalid)?;

        let hash = self.payload.hash();
        let signature = Signature::from_bytes(&signature_bytes);

        verifying_key
            .verify(&hash, &signature)
            .map_err(|_| MiniAppError::IntentSignatureInvalid)
    }

    /// Mark as pending (submitted to chain)
    pub fn mark_pending(&mut self) {
        self.status = IntentStatus::Pending;
        self.updated_at = Utc::now();
    }

    /// Mark as executing
    pub fn mark_executing(&mut self) {
        self.status = IntentStatus::Executing;
        self.updated_at = Utc::now();
    }

    /// Mark as executed
    pub fn mark_executed(&mut self, receipt_id: crate::receipt::ReceiptId) {
        self.status = IntentStatus::Executed;
        self.receipt_id = Some(receipt_id);
        self.updated_at = Utc::now();
    }

    /// Mark as failed
    pub fn mark_failed(&mut self, error: &str) {
        self.status = IntentStatus::Failed;
        self.error = Some(error.to_string());
        self.updated_at = Utc::now();
    }

    /// Mark as expired
    pub fn mark_expired(&mut self) {
        self.status = IntentStatus::Expired;
        self.updated_at = Utc::now();
    }

    /// Mark as cancelled
    pub fn mark_cancelled(&mut self) {
        self.status = IntentStatus::Cancelled;
        self.updated_at = Utc::now();
    }

    /// Check if intent is expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.payload.deadline
    }

    /// Validate intent
    pub fn validate(&self) -> MiniAppResult<()> {
        self.payload.validate()?;

        // Must be signed if past Created status
        if self.status != IntentStatus::Created && self.signature.is_none() {
            return Err(MiniAppError::IntentSignatureInvalid);
        }

        Ok(())
    }
}

/// Pending intent (in-memory tracking)
#[derive(Debug)]
pub struct PendingIntent {
    /// Intent
    pub intent: Intent,
    /// Submission attempts
    pub attempts: u32,
    /// Last attempt timestamp
    pub last_attempt: Option<Instant>,
    /// Retry delay in seconds
    pub retry_delay: u64,
}

impl PendingIntent {
    /// Create new pending intent
    pub fn new(intent: Intent) -> Self {
        Self {
            intent,
            attempts: 0,
            last_attempt: None,
            retry_delay: 1,
        }
    }

    /// Record attempt
    pub fn record_attempt(&mut self) {
        self.attempts += 1;
        self.last_attempt = Some(Instant::now());
        // Exponential backoff
        self.retry_delay = (self.retry_delay * 2).min(60);
    }

    /// Check if should retry
    pub fn should_retry(&self) -> bool {
        if self.attempts >= 5 {
            return false;
        }

        if let Some(last) = self.last_attempt {
            last.elapsed().as_secs() >= self.retry_delay
        } else {
            true
        }
    }
}

/// Intent builder for fluent API
pub struct IntentBuilder {
    app_id: AppId,
    sender: [u8; 32],
    chain_id: u64,
    max_fee: u64,
    intent_type: Option<IntentType>,
}

impl IntentBuilder {
    /// Create new builder
    pub fn new(app_id: AppId, sender: [u8; 32], chain_id: u64) -> Self {
        Self {
            app_id,
            sender,
            chain_id,
            max_fee: 10000, // Default max fee
            intent_type: None,
        }
    }

    /// Set max fee
    pub fn max_fee(mut self, max_fee: u64) -> Self {
        self.max_fee = max_fee;
        self
    }

    /// Set transfer intent
    pub fn transfer(mut self, recipient: [u8; 32], amount: u64) -> Self {
        self.intent_type = Some(IntentType::Transfer {
            recipient,
            mint: None,
            amount,
            memo: None,
        });
        self
    }

    /// Set token transfer intent
    pub fn token_transfer(mut self, recipient: [u8; 32], mint: [u8; 32], amount: u64) -> Self {
        self.intent_type = Some(IntentType::Transfer {
            recipient,
            mint: Some(mint),
            amount,
            memo: None,
        });
        self
    }

    /// Set program call intent
    pub fn program_call(
        mut self,
        program_id: [u8; 32],
        instruction_data: Vec<u8>,
        accounts: Vec<IntentAccountMeta>,
    ) -> Self {
        self.intent_type = Some(IntentType::ProgramCall {
            program_id,
            instruction_data,
            accounts,
        });
        self
    }

    /// Set stake intent
    pub fn stake(mut self, validator: [u8; 32], amount: u64) -> Self {
        self.intent_type = Some(IntentType::Stake { validator, amount });
        self
    }

    /// Build the intent
    pub fn build(self) -> MiniAppResult<Intent> {
        let intent_type = self
            .intent_type
            .ok_or_else(|| MiniAppError::InvalidIntent("no intent type specified".to_string()))?;

        let payload = IntentPayload::new(self.sender, intent_type, self.max_fee, self.chain_id);
        payload.validate()?;

        Ok(Intent::new(self.app_id, payload))
    }
}

/// Rate limiter for intents
pub struct IntentRateLimiter {
    /// Requests per user in current window
    requests: RwLock<HashMap<[u8; 32], (u32, Instant)>>,
    /// Window duration
    window: std::time::Duration,
    /// Max requests per window
    max_requests: u32,
}

impl IntentRateLimiter {
    /// Create new limiter
    pub fn new(max_requests: u32, window: std::time::Duration) -> Self {
        Self {
            requests: RwLock::new(HashMap::new()),
            window,
            max_requests,
        }
    }

    /// Check if request is allowed
    pub fn check(&self, user_id: &[u8; 32]) -> MiniAppResult<()> {
        let mut requests = self.requests.write();
        let now = Instant::now();

        let (count, start) = requests.entry(*user_id).or_insert((0, now));

        // Reset if window passed
        if now.duration_since(*start) > self.window {
            *count = 0;
            *start = now;
        }

        if *count >= self.max_requests {
            return Err(MiniAppError::IntentRateLimitExceeded {
                count: *count,
                max: self.max_requests,
            });
        }

        *count += 1;
        Ok(())
    }

    /// Cleanup old entries
    pub fn cleanup(&self) {
        let mut requests = self.requests.write();
        let now = Instant::now();

        requests.retain(|_, (_, start)| now.duration_since(*start) <= self.window * 2);
    }
}

impl Default for IntentRateLimiter {
    fn default() -> Self {
        Self::new(MAX_INTENTS_PER_MINUTE, std::time::Duration::from_secs(60))
    }
}

/// Intent store (manages pending intents)
pub struct IntentStore {
    /// Intents by ID
    intents: RwLock<HashMap<IntentId, Intent>>,
    /// Pending intents by user
    by_user: RwLock<HashMap<[u8; 32], Vec<IntentId>>>,
    /// Rate limiter
    rate_limiter: IntentRateLimiter,
}

impl IntentStore {
    /// Create new store
    pub fn new() -> Self {
        Self {
            intents: RwLock::new(HashMap::new()),
            by_user: RwLock::new(HashMap::new()),
            rate_limiter: IntentRateLimiter::default(),
        }
    }

    /// Submit new intent
    pub fn submit(&self, intent: Intent) -> MiniAppResult<IntentId> {
        // Rate limit check
        self.rate_limiter.check(&intent.payload.sender)?;

        // Validate intent
        intent.validate()?;

        let id = intent.id;
        let user = intent.payload.sender;

        // Store intent
        self.intents.write().insert(id, intent);
        self.by_user.write().entry(user).or_default().push(id);

        Ok(id)
    }

    /// Get intent by ID
    pub fn get(&self, id: &IntentId) -> Option<Intent> {
        self.intents.read().get(id).cloned()
    }

    /// Update intent
    pub fn update(&self, intent: Intent) -> MiniAppResult<()> {
        let mut intents = self.intents.write();
        if !intents.contains_key(&intent.id) {
            return Err(MiniAppError::IntentNotFound(intent.id.to_string()));
        }
        intents.insert(intent.id, intent);
        Ok(())
    }

    /// Get user's intents
    pub fn get_by_user(&self, user: &[u8; 32]) -> Vec<Intent> {
        let by_user = self.by_user.read();
        let intents = self.intents.read();

        by_user
            .get(user)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| intents.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get pending intents
    pub fn get_pending(&self) -> Vec<Intent> {
        self.intents
            .read()
            .values()
            .filter(|i| i.status.is_pending())
            .cloned()
            .collect()
    }

    /// Expire old intents
    pub fn expire_old_intents(&self) {
        let mut intents = self.intents.write();
        for intent in intents.values_mut() {
            if intent.is_expired() && !intent.status.is_terminal() {
                intent.mark_expired();
            }
        }
    }

    /// Cleanup completed intents older than duration
    pub fn cleanup(&self, max_age: Duration) {
        let cutoff = Utc::now() - max_age;

        let mut intents = self.intents.write();
        let mut by_user = self.by_user.write();

        let to_remove: Vec<IntentId> = intents
            .iter()
            .filter(|(_, i)| i.status.is_terminal() && i.updated_at < cutoff)
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            if let Some(intent) = intents.remove(&id) {
                if let Some(user_intents) = by_user.get_mut(&intent.payload.sender) {
                    user_intents.retain(|i| i != &id);
                }
            }
        }
    }
}

impl Default for IntentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_intent_id() {
        let id = IntentId::new();
        let str_id = id.to_string();
        let parsed = IntentId::parse(&str_id).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_intent_builder() {
        let app_id = AppId([1u8; 32]);
        let sender = [2u8; 32];
        let recipient = [3u8; 32];

        let intent = IntentBuilder::new(app_id, sender, 1)
            .transfer(recipient, 1000)
            .max_fee(5000)
            .build()
            .unwrap();

        assert_eq!(intent.app_id, app_id);
        assert_eq!(intent.payload.sender, sender);
        assert_eq!(intent.status, IntentStatus::Created);
    }

    #[test]
    fn test_intent_signing() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let app_id = AppId([1u8; 32]);
        let sender = verifying_key.to_bytes();

        let mut intent = IntentBuilder::new(app_id, sender, 1)
            .transfer([3u8; 32], 1000)
            .build()
            .unwrap();

        intent.sign(&signing_key).unwrap();
        assert_eq!(intent.status, IntentStatus::Signed);

        intent.verify_signature(&verifying_key).unwrap();
    }

    #[test]
    fn test_intent_expiry() {
        let app_id = AppId([1u8; 32]);
        let mut payload = IntentPayload::new(
            [2u8; 32],
            IntentType::Transfer {
                recipient: [3u8; 32],
                mint: None,
                amount: 1000,
                memo: None,
            },
            5000,
            1,
        );

        // Set deadline in the past
        payload.deadline = Utc::now() - Duration::seconds(1);

        assert!(payload.validate().is_err());
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = IntentRateLimiter::new(2, std::time::Duration::from_secs(60));
        let user = [1u8; 32];

        // First two should pass
        assert!(limiter.check(&user).is_ok());
        assert!(limiter.check(&user).is_ok());

        // Third should fail
        assert!(limiter.check(&user).is_err());
    }

    #[test]
    fn test_intent_store() {
        let store = IntentStore::new();
        let app_id = AppId([1u8; 32]);
        let sender = [2u8; 32];

        let intent = IntentBuilder::new(app_id, sender, 1)
            .transfer([3u8; 32], 1000)
            .build()
            .unwrap();

        let id = store.submit(intent).unwrap();

        let loaded = store.get(&id);
        assert!(loaded.is_some());

        let user_intents = store.get_by_user(&sender);
        assert_eq!(user_intents.len(), 1);
    }

    #[test]
    fn test_intent_status_transitions() {
        let app_id = AppId([1u8; 32]);
        let mut intent = IntentBuilder::new(app_id, [2u8; 32], 1)
            .transfer([3u8; 32], 1000)
            .build()
            .unwrap();

        assert!(!intent.status.is_terminal());
        assert!(!intent.status.is_pending());

        intent.mark_pending();
        assert!(intent.status.is_pending());

        intent.mark_executing();
        assert!(intent.status.is_pending());

        let receipt_id = crate::receipt::ReceiptId::new();
        intent.mark_executed(receipt_id);
        assert!(intent.status.is_terminal());
    }
}
