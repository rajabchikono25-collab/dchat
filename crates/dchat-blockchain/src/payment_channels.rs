//! Payment Channels for Off-Chain Micropayments
//!
//! This module implements production-grade payment channels with:
//! - Cryptographic signature verification on all state updates
//! - Balance invariant enforcement
//! - Unilateral close with timelock for dispute resolution
//! - Challenge mechanism for fraud prevention
//! - Support for cooperative and forced closes
//!
//! # Security
//! - All state updates require valid Ed25519 signatures
//! - Balance conservation enforced (sender + receiver = capacity)
//! - Monotonically increasing nonces prevent replay attacks
//! - 48-hour dispute window for unilateral closes
//! - Fraud slashing for malicious close attempts

use chrono::{DateTime, Duration, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Dispute window duration (48 hours)
pub const DISPUTE_WINDOW_SECONDS: i64 = 48 * 60 * 60;

/// Minimum channel capacity (10 DCHAT)
pub const MIN_CHANNEL_CAPACITY: u64 = 10_000_000; // 10 tokens with 6 decimals

/// Maximum channel capacity (100,000 DCHAT)
pub const MAX_CHANNEL_CAPACITY: u64 = 100_000_000_000;

/// Channel state at a point in time
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelState {
    /// Monotonically increasing nonce (prevents replay)
    pub nonce: u64,
    /// Sender's remaining balance
    pub sender_balance: u64,
    /// Receiver's balance (accumulated payments)
    pub receiver_balance: u64,
}

impl ChannelState {
    /// Create initial state with all balance on sender side
    pub fn initial(capacity: u64) -> Self {
        Self {
            nonce: 0,
            sender_balance: capacity,
            receiver_balance: 0,
        }
    }

    /// Get total balance (should always equal channel capacity)
    pub fn total(&self) -> u64 {
        self.sender_balance + self.receiver_balance
    }
}

/// Signed channel state update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedStateUpdate {
    /// The state being committed
    pub state: ChannelState,
    /// Sender's signature over the state (64 bytes Ed25519)
    #[serde(with = "serde_bytes")]
    pub sender_signature: Vec<u8>,
    /// Receiver's signature (optional for unilateral updates)
    #[serde(with = "option_bytes")]
    pub receiver_signature: Option<Vec<u8>>,
    /// When this update was created
    pub timestamp: DateTime<Utc>,
}

/// Helper module for Option<Vec<u8>> serialization
mod option_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(bytes) => serde_bytes::serialize(bytes, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<serde_bytes::ByteBuf>::deserialize(deserializer)
            .map(|opt| opt.map(|bb| bb.into_vec()))
    }
}

impl SignedStateUpdate {
    /// Create a new signed update from sender
    pub fn new_from_sender(state: ChannelState, channel_id: &str, sender_key: &SigningKey) -> Self {
        let message = Self::compute_message(channel_id, &state);
        let signature = sender_key.sign(&message);

        Self {
            state,
            sender_signature: signature.to_bytes().to_vec(),
            receiver_signature: None,
            timestamp: Utc::now(),
        }
    }

    /// Add receiver's signature to make update bilateral
    pub fn add_receiver_signature(&mut self, channel_id: &str, receiver_key: &SigningKey) {
        let message = Self::compute_message(channel_id, &self.state);
        let signature = receiver_key.sign(&message);
        self.receiver_signature = Some(signature.to_bytes().to_vec());
    }

    /// Compute the message to be signed
    fn compute_message(channel_id: &str, state: &ChannelState) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(channel_id.as_bytes());
        hasher.update(state.nonce.to_le_bytes());
        hasher.update(state.sender_balance.to_le_bytes());
        hasher.update(state.receiver_balance.to_le_bytes());
        hasher.finalize().into()
    }

    /// Verify sender's signature
    pub fn verify_sender_signature(
        &self,
        channel_id: &str,
        sender_key: &VerifyingKey,
    ) -> Result<()> {
        let message = Self::compute_message(channel_id, &self.state);
        let sig_bytes: [u8; 64] = self
            .sender_signature
            .as_slice()
            .try_into()
            .map_err(|_| Error::validation("Invalid signature length"))?;
        let signature = Signature::from_bytes(&sig_bytes);

        sender_key
            .verify(&message, &signature)
            .map_err(|e| Error::validation(format!("Invalid sender signature: {}", e)))
    }

    /// Verify receiver's signature (if present)
    pub fn verify_receiver_signature(
        &self,
        channel_id: &str,
        receiver_key: &VerifyingKey,
    ) -> Result<()> {
        let sig_vec = self
            .receiver_signature
            .as_ref()
            .ok_or_else(|| Error::validation("No receiver signature present"))?;

        let message = Self::compute_message(channel_id, &self.state);
        let sig_bytes: [u8; 64] = sig_vec
            .as_slice()
            .try_into()
            .map_err(|_| Error::validation("Invalid signature length"))?;
        let signature = Signature::from_bytes(&sig_bytes);

        receiver_key
            .verify(&message, &signature)
            .map_err(|e| Error::validation(format!("Invalid receiver signature: {}", e)))
    }

    /// Check if update is fully signed (bilateral)
    pub fn is_bilateral(&self) -> bool {
        self.receiver_signature.is_some()
    }
}

/// Payment channel status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelStatus {
    /// Channel is open and operational
    Open,
    /// Unilateral close initiated, in dispute period
    Closing,
    /// Channel is closed
    Closed,
    /// Channel was challenged and fraud detected
    Disputed,
}

/// Payment channel between two parties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentChannel {
    /// Unique channel identifier
    pub channel_id: String,
    /// Sender (payer) user ID
    pub sender_id: UserId,
    /// Receiver (payee) user ID  
    pub receiver_id: UserId,
    /// Sender's public key for signature verification
    pub sender_key: [u8; 32],
    /// Receiver's public key for signature verification
    pub receiver_key: [u8; 32],
    /// Channel capacity (locked tokens)
    pub capacity: u64,
    /// Current confirmed state
    pub current_state: ChannelState,
    /// Latest bilateral update
    pub latest_bilateral: Option<SignedStateUpdate>,
    /// Channel status
    pub status: ChannelStatus,
    /// When channel was opened
    pub opened_at: DateTime<Utc>,
    /// Last state update timestamp
    pub last_update: DateTime<Utc>,
    /// On-chain funding transaction
    pub funding_tx_id: Option<String>,
}

impl PaymentChannel {
    /// Create a new payment channel
    pub fn new(
        sender_id: UserId,
        receiver_id: UserId,
        sender_key: VerifyingKey,
        receiver_key: VerifyingKey,
        capacity: u64,
    ) -> Result<Self> {
        if capacity < MIN_CHANNEL_CAPACITY {
            return Err(Error::validation(format!(
                "Channel capacity {} below minimum {}",
                capacity, MIN_CHANNEL_CAPACITY
            )));
        }

        if capacity > MAX_CHANNEL_CAPACITY {
            return Err(Error::validation(format!(
                "Channel capacity {} exceeds maximum {}",
                capacity, MAX_CHANNEL_CAPACITY
            )));
        }

        let channel_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        Ok(Self {
            channel_id,
            sender_id,
            receiver_id,
            sender_key: sender_key.to_bytes(),
            receiver_key: receiver_key.to_bytes(),
            capacity,
            current_state: ChannelState::initial(capacity),
            latest_bilateral: None,
            status: ChannelStatus::Open,
            opened_at: now,
            last_update: now,
            funding_tx_id: None,
        })
    }

    /// Get sender's verifying key
    pub fn get_sender_key(&self) -> Result<VerifyingKey> {
        VerifyingKey::from_bytes(&self.sender_key)
            .map_err(|e| Error::internal(format!("Invalid sender key: {}", e)))
    }

    /// Get receiver's verifying key
    pub fn get_receiver_key(&self) -> Result<VerifyingKey> {
        VerifyingKey::from_bytes(&self.receiver_key)
            .map_err(|e| Error::internal(format!("Invalid receiver key: {}", e)))
    }

    /// Update channel state with signature verification
    pub fn update_state(&mut self, update: SignedStateUpdate) -> Result<()> {
        // 1. Verify channel is open
        if self.status != ChannelStatus::Open {
            return Err(Error::validation(format!(
                "Cannot update channel with status {:?}",
                self.status
            )));
        }

        // 2. Verify nonce is increasing
        if update.state.nonce <= self.current_state.nonce {
            return Err(Error::validation(format!(
                "Invalid nonce: got {}, expected > {}",
                update.state.nonce, self.current_state.nonce
            )));
        }

        // 3. Verify balance invariant (total must equal capacity)
        let total = update.state.total();
        if total != self.capacity {
            return Err(Error::validation(format!(
                "Balance invariant violation: {} + {} = {}, expected {}",
                update.state.sender_balance, update.state.receiver_balance, total, self.capacity
            )));
        }

        // 4. Verify sender signature (CRITICAL)
        let sender_key = self.get_sender_key()?;
        update.verify_sender_signature(&self.channel_id, &sender_key)?;

        // 5. Verify receiver signature if present
        if update.receiver_signature.is_some() {
            let receiver_key = self.get_receiver_key()?;
            update.verify_receiver_signature(&self.channel_id, &receiver_key)?;
        }

        // 6. Update state
        self.current_state = update.state.clone();
        self.last_update = Utc::now();

        // Store as latest bilateral if fully signed
        if update.is_bilateral() {
            self.latest_bilateral = Some(update);
        }

        Ok(())
    }

    /// Create a payment (sender to receiver transfer)
    pub fn create_payment(
        &self,
        amount: u64,
        sender_key: &SigningKey,
    ) -> Result<SignedStateUpdate> {
        if self.status != ChannelStatus::Open {
            return Err(Error::validation("Channel not open"));
        }

        if amount > self.current_state.sender_balance {
            return Err(Error::validation(format!(
                "Insufficient balance: have {}, need {}",
                self.current_state.sender_balance, amount
            )));
        }

        let new_state = ChannelState {
            nonce: self.current_state.nonce + 1,
            sender_balance: self.current_state.sender_balance - amount,
            receiver_balance: self.current_state.receiver_balance + amount,
        };

        Ok(SignedStateUpdate::new_from_sender(
            new_state,
            &self.channel_id,
            sender_key,
        ))
    }
}

/// Unilateral close request (when counterparty is unresponsive)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnilateralCloseRequest {
    /// Channel being closed
    pub channel_id: String,
    /// Final state submitted for close
    pub final_state: SignedStateUpdate,
    /// Who initiated the close
    pub initiator: UserId,
    /// When close was submitted
    pub submitted_at: DateTime<Utc>,
    /// When dispute period ends
    pub dispute_deadline: DateTime<Utc>,
    /// Whether this has been challenged
    pub challenged: bool,
    /// On-chain transaction ID
    pub chain_tx_id: Option<String>,
}

/// Challenge to a unilateral close
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseChallenge {
    /// Channel being challenged
    pub channel_id: String,
    /// Newer state proving fraud
    pub newer_state: SignedStateUpdate,
    /// Who submitted the challenge
    pub challenger: UserId,
    /// When challenge was submitted
    pub submitted_at: DateTime<Utc>,
    /// On-chain transaction ID
    pub chain_tx_id: Option<String>,
}

/// Payment channel error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum ChannelError {
    #[error("Channel not found: {0}")]
    NotFound(String),

    #[error("Invalid nonce: expected > {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },

    #[error("Balance invariant violated: {expected} != {got}")]
    BalanceInvariantViolation { expected: u64, got: u64 },

    #[error("Invalid sender signature")]
    InvalidSenderSignature,

    #[error("Invalid receiver signature")]
    InvalidReceiverSignature,

    #[error("Not a party to channel")]
    NotChannelParty,

    #[error("Cannot challenge own close")]
    CannotChallengeSelf,

    #[error("No pending close to challenge")]
    NoPendingClose,

    #[error("Challenge state not newer")]
    StateNotNewer,

    #[error("Channel not open")]
    ChannelNotOpen,

    #[error("Insufficient credits: have {have}, need {need}")]
    InsufficientCredits { have: u64, need: u64 },

    #[error("Dispute period expired")]
    DisputePeriodExpired,

    #[error("Dispute period not yet expired")]
    DisputePeriodActive,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Payment channel manager
pub struct PaymentChannelManager {
    /// All channels (channel_id -> channel)
    channels: Arc<RwLock<HashMap<String, PaymentChannel>>>,
    /// Pending unilateral closes
    pending_closes: Arc<RwLock<HashMap<String, UnilateralCloseRequest>>>,
    /// Challenges
    challenges: Arc<RwLock<HashMap<String, CloseChallenge>>>,
    /// Fraud events for slashing
    fraud_events: Arc<RwLock<Vec<FraudEvent>>>,
}

/// Fraud event for slashing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudEvent {
    pub channel_id: String,
    pub fraudster: UserId,
    pub submitted_nonce: u64,
    pub actual_nonce: u64,
    pub detected_at: DateTime<Utc>,
    pub slashed: bool,
}

impl PaymentChannelManager {
    /// Create new payment channel manager
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            pending_closes: Arc::new(RwLock::new(HashMap::new())),
            challenges: Arc::new(RwLock::new(HashMap::new())),
            fraud_events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Open a new payment channel
    pub fn open_channel(
        &self,
        sender_id: UserId,
        receiver_id: UserId,
        sender_key: VerifyingKey,
        receiver_key: VerifyingKey,
        capacity: u64,
    ) -> Result<PaymentChannel> {
        let channel =
            PaymentChannel::new(sender_id, receiver_id, sender_key, receiver_key, capacity)?;

        let channel_id = channel.channel_id.clone();
        self.channels
            .write()
            .unwrap()
            .insert(channel_id, channel.clone());

        tracing::info!(
            "💳 Payment channel opened: {} (capacity: {})",
            channel.channel_id,
            capacity
        );

        Ok(channel)
    }

    /// Get channel by ID
    pub fn get_channel(&self, channel_id: &str) -> Result<PaymentChannel> {
        self.channels
            .read()
            .unwrap()
            .get(channel_id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", channel_id)))
    }

    /// Update channel state
    pub fn update_channel_state(&self, channel_id: &str, update: SignedStateUpdate) -> Result<()> {
        let mut channels = self.channels.write().unwrap();
        let channel = channels
            .get_mut(channel_id)
            .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", channel_id)))?;

        channel.update_state(update)
    }

    /// Process a micropayment through the channel (off-chain)
    ///
    /// SECURITY: This method requires a properly signed state update.
    /// The sender must sign the new state to authorize the payment.
    ///
    /// Returns a transaction ID for tracking
    pub fn process_payment(
        &self,
        channel_id: &str,
        signed_update: SignedStateUpdate,
    ) -> std::result::Result<String, ChannelError> {
        let mut channels = self.channels.write().unwrap();
        let channel = channels
            .get_mut(channel_id)
            .ok_or_else(|| ChannelError::NotFound(channel_id.to_string()))?;

        // Verify channel is open
        if channel.status != ChannelStatus::Open {
            return Err(ChannelError::ChannelNotOpen);
        }

        // Verify nonce is incrementing
        if signed_update.state.nonce != channel.current_state.nonce + 1 {
            return Err(ChannelError::InvalidNonce {
                expected: channel.current_state.nonce + 1,
                got: signed_update.state.nonce,
            });
        }

        // Verify balance invariant
        let total = signed_update.state.sender_balance + signed_update.state.receiver_balance;
        if total != channel.capacity {
            return Err(ChannelError::BalanceInvariantViolation {
                expected: channel.capacity,
                got: total,
            });
        }

        // SECURITY: Verify sender signature (critical - prevents unauthorized payments)
        let sender_key = channel
            .get_sender_key()
            .map_err(|e| ChannelError::Internal(e.to_string()))?;
        signed_update
            .verify_sender_signature(channel_id, &sender_key)
            .map_err(|_| ChannelError::InvalidSenderSignature)?;

        // Calculate payment amount for logging
        let amount = channel
            .current_state
            .sender_balance
            .saturating_sub(signed_update.state.sender_balance);

        // Update state
        channel.current_state = signed_update.state.clone();
        channel.last_update = Utc::now();

        // Store as latest bilateral if fully signed
        if signed_update.is_bilateral() {
            channel.latest_bilateral = Some(signed_update);
        }

        // Generate a unique tx ID for this off-chain payment
        let tx_id = format!(
            "pc-{}-{}-{}",
            channel_id,
            channel.current_state.nonce,
            Utc::now().timestamp_millis()
        );

        tracing::debug!(
            "💸 Off-chain payment processed: {} tokens in channel {} (tx: {})",
            amount,
            channel_id,
            tx_id
        );

        Ok(tx_id)
    }

    /// Initiate unilateral close (when counterparty is unresponsive)
    pub fn initiate_unilateral_close(
        &self,
        channel_id: &str,
        final_state: SignedStateUpdate,
        initiator_id: &UserId,
    ) -> std::result::Result<UnilateralCloseRequest, ChannelError> {
        let mut channels = self.channels.write().unwrap();
        let channel = channels
            .get_mut(channel_id)
            .ok_or_else(|| ChannelError::NotFound(channel_id.to_string()))?;

        // Verify initiator is party to channel
        if *initiator_id != channel.sender_id && *initiator_id != channel.receiver_id {
            return Err(ChannelError::NotChannelParty);
        }

        // Verify state signature
        let initiator_key = if *initiator_id == channel.sender_id {
            channel.get_sender_key()
        } else {
            channel.get_receiver_key()
        }
        .map_err(|e| ChannelError::Internal(e.to_string()))?;

        // Verify the submitted state
        if *initiator_id == channel.sender_id {
            final_state
                .verify_sender_signature(channel_id, &initiator_key)
                .map_err(|_| ChannelError::InvalidSenderSignature)?;
        }

        // Update channel status
        channel.status = ChannelStatus::Closing;

        let now = Utc::now();
        let close_request = UnilateralCloseRequest {
            channel_id: channel_id.to_string(),
            final_state,
            initiator: initiator_id.clone(),
            submitted_at: now,
            dispute_deadline: now + Duration::seconds(DISPUTE_WINDOW_SECONDS),
            challenged: false,
            chain_tx_id: None,
        };

        drop(channels);

        self.pending_closes
            .write()
            .unwrap()
            .insert(channel_id.to_string(), close_request.clone());

        tracing::info!(
            "⏱️ Unilateral close initiated for channel {} (dispute deadline: {})",
            channel_id,
            close_request.dispute_deadline
        );

        Ok(close_request)
    }

    /// Challenge a unilateral close with newer state
    pub fn challenge_close(
        &self,
        channel_id: &str,
        newer_state: SignedStateUpdate,
        challenger_id: &UserId,
    ) -> std::result::Result<CloseChallenge, ChannelError> {
        let pending_closes = self.pending_closes.read().unwrap();
        let pending = pending_closes
            .get(channel_id)
            .ok_or(ChannelError::NoPendingClose)?
            .clone();
        drop(pending_closes);

        // SECURITY: Verify dispute period has not expired
        if Utc::now() >= pending.dispute_deadline {
            return Err(ChannelError::DisputePeriodExpired);
        }

        // Verify challenger is the other party
        if *challenger_id == pending.initiator {
            return Err(ChannelError::CannotChallengeSelf);
        }

        // Verify newer state has higher nonce
        if newer_state.state.nonce <= pending.final_state.state.nonce {
            return Err(ChannelError::StateNotNewer);
        }

        // Get channel for signature verification
        let channel = self
            .get_channel(channel_id)
            .map_err(|_| ChannelError::NotFound(channel_id.to_string()))?;

        // SECURITY: Verify BOTH signatures - state must be bilateral
        // A valid challenge requires a state that was mutually agreed upon
        let sender_key = channel
            .get_sender_key()
            .map_err(|e| ChannelError::Internal(e.to_string()))?;
        let receiver_key = channel
            .get_receiver_key()
            .map_err(|e| ChannelError::Internal(e.to_string()))?;

        newer_state
            .verify_sender_signature(channel_id, &sender_key)
            .map_err(|_| ChannelError::InvalidSenderSignature)?;

        // Receiver signature must be present for a valid challenge
        if newer_state.receiver_signature.is_none() {
            return Err(ChannelError::Internal(
                "Challenge state must be bilateral (signed by both parties)".to_string(),
            ));
        }
        newer_state
            .verify_receiver_signature(channel_id, &receiver_key)
            .map_err(|_| ChannelError::InvalidReceiverSignature)?;

        // Record fraud event for slashing
        let fraud_event = FraudEvent {
            channel_id: channel_id.to_string(),
            fraudster: pending.initiator.clone(),
            submitted_nonce: pending.final_state.state.nonce,
            actual_nonce: newer_state.state.nonce,
            detected_at: Utc::now(),
            slashed: false,
        };

        self.fraud_events.write().unwrap().push(fraud_event);

        // Update pending close as challenged
        let mut pending_closes = self.pending_closes.write().unwrap();
        if let Some(close_req) = pending_closes.get_mut(channel_id) {
            close_req.challenged = true;
        }
        drop(pending_closes);

        // Update channel status
        let mut channels = self.channels.write().unwrap();
        if let Some(ch) = channels.get_mut(channel_id) {
            ch.status = ChannelStatus::Disputed;
            ch.current_state = newer_state.state.clone();
        }
        drop(channels);

        let challenge = CloseChallenge {
            channel_id: channel_id.to_string(),
            newer_state,
            challenger: challenger_id.clone(),
            submitted_at: Utc::now(),
            chain_tx_id: None,
        };

        self.challenges
            .write()
            .unwrap()
            .insert(channel_id.to_string(), challenge.clone());

        tracing::warn!(
            "🚨 FRAUD DETECTED: Channel {} - initiator submitted nonce {}, actual is {}",
            channel_id,
            pending.final_state.state.nonce,
            challenge.newer_state.state.nonce
        );

        Ok(challenge)
    }

    /// Finalize unilateral close after dispute period
    pub fn finalize_unilateral_close(
        &self,
        channel_id: &str,
    ) -> std::result::Result<(u64, u64), ChannelError> {
        let pending = self
            .pending_closes
            .read()
            .unwrap()
            .get(channel_id)
            .cloned()
            .ok_or(ChannelError::NoPendingClose)?;

        // Check dispute period has passed
        if Utc::now() < pending.dispute_deadline {
            return Err(ChannelError::DisputePeriodActive);
        }

        // Check not challenged
        if pending.challenged {
            return Err(ChannelError::Internal(
                "Close was challenged, cannot finalize".to_string(),
            ));
        }

        // Update channel as closed
        let mut channels = self.channels.write().unwrap();
        if let Some(channel) = channels.get_mut(channel_id) {
            channel.status = ChannelStatus::Closed;
        }

        // Remove from pending closes
        self.pending_closes.write().unwrap().remove(channel_id);

        let sender_balance = pending.final_state.state.sender_balance;
        let receiver_balance = pending.final_state.state.receiver_balance;

        tracing::info!(
            "✅ Channel {} closed: sender={}, receiver={}",
            channel_id,
            sender_balance,
            receiver_balance
        );

        Ok((sender_balance, receiver_balance))
    }

    /// Cooperative close (both parties agree)
    pub fn cooperative_close(
        &self,
        channel_id: &str,
        final_state: SignedStateUpdate,
    ) -> Result<(u64, u64)> {
        // Verify both signatures are present
        if !final_state.is_bilateral() {
            return Err(Error::validation(
                "Cooperative close requires both signatures",
            ));
        }

        let mut channels = self.channels.write().unwrap();
        let channel = channels
            .get_mut(channel_id)
            .ok_or_else(|| Error::NotFound(format!("Channel not found: {}", channel_id)))?;

        // Verify signatures
        let sender_key = channel.get_sender_key()?;
        let receiver_key = channel.get_receiver_key()?;

        final_state.verify_sender_signature(channel_id, &sender_key)?;
        final_state.verify_receiver_signature(channel_id, &receiver_key)?;

        // Verify balance invariant
        if final_state.state.total() != channel.capacity {
            return Err(Error::validation("Balance invariant violation"));
        }

        // Close channel
        channel.status = ChannelStatus::Closed;
        channel.current_state = final_state.state.clone();

        let sender_balance = final_state.state.sender_balance;
        let receiver_balance = final_state.state.receiver_balance;

        tracing::info!(
            "✅ Channel {} cooperatively closed: sender={}, receiver={}",
            channel_id,
            sender_balance,
            receiver_balance
        );

        Ok((sender_balance, receiver_balance))
    }

    /// Get all fraud events (for slashing)
    pub fn get_fraud_events(&self) -> Vec<FraudEvent> {
        self.fraud_events.read().unwrap().clone()
    }

    /// Mark fraud event as slashed
    pub fn mark_fraud_slashed(&self, channel_id: &str) {
        let mut events = self.fraud_events.write().unwrap();
        for event in events.iter_mut() {
            if event.channel_id == channel_id {
                event.slashed = true;
            }
        }
    }

    /// Get all channels for a user
    pub fn get_user_channels(&self, user_id: &UserId) -> Vec<PaymentChannel> {
        self.channels
            .read()
            .unwrap()
            .values()
            .filter(|c| c.sender_id == *user_id || c.receiver_id == *user_id)
            .cloned()
            .collect()
    }

    /// Get pending close for channel
    pub fn get_pending_close(&self, channel_id: &str) -> Option<UnilateralCloseRequest> {
        self.pending_closes.read().unwrap().get(channel_id).cloned()
    }
}

impl Default for PaymentChannelManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Message credits channel for high-frequency messaging
/// Uses payment channels for efficient off-chain message fee settlement
pub struct MessageCreditsChannel {
    /// Underlying payment channel ID
    pub channel_id: String,
    /// Messages sent since last settlement
    pub pending_messages: u64,
    /// Fee per message
    pub fee_per_message: u64,
    /// Sender's signing key for updates
    sender_signing_key: Option<SigningKey>,
}

impl MessageCreditsChannel {
    /// Default message fee (0.1 DCHAT with 6 decimals)
    pub const DEFAULT_MESSAGE_FEE: u64 = 100_000;

    /// Create new message credits channel
    pub fn new(channel_id: String, fee_per_message: Option<u64>) -> Self {
        Self {
            channel_id,
            pending_messages: 0,
            fee_per_message: fee_per_message.unwrap_or(Self::DEFAULT_MESSAGE_FEE),
            sender_signing_key: None,
        }
    }

    /// Set sender's signing key for creating updates
    pub fn set_sender_key(&mut self, key: SigningKey) {
        self.sender_signing_key = Some(key);
    }

    /// Use credit for a message
    pub fn use_credit(
        &mut self,
        channel_manager: &PaymentChannelManager,
    ) -> std::result::Result<SignedStateUpdate, ChannelError> {
        let channel = channel_manager
            .get_channel(&self.channel_id)
            .map_err(|_| ChannelError::NotFound(self.channel_id.clone()))?;

        if channel.status != ChannelStatus::Open {
            return Err(ChannelError::ChannelNotOpen);
        }

        if channel.current_state.sender_balance < self.fee_per_message {
            return Err(ChannelError::InsufficientCredits {
                have: channel.current_state.sender_balance,
                need: self.fee_per_message,
            });
        }

        let sender_key = self
            .sender_signing_key
            .as_ref()
            .ok_or_else(|| ChannelError::Internal("Sender key not set".to_string()))?;

        let update = channel
            .create_payment(self.fee_per_message, sender_key)
            .map_err(|e| ChannelError::Internal(e.to_string()))?;

        self.pending_messages += 1;

        Ok(update)
    }

    /// Get number of remaining credits
    pub fn remaining_credits(&self, channel_manager: &PaymentChannelManager) -> u64 {
        match channel_manager.get_channel(&self.channel_id) {
            Ok(channel) => channel.current_state.sender_balance / self.fee_per_message,
            Err(_) => 0,
        }
    }

    /// Get total messages sent through this channel
    pub fn total_messages(&self) -> u64 {
        self.pending_messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    fn generate_keypair() -> (SigningKey, VerifyingKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        (signing_key, verifying_key)
    }

    #[test]
    fn test_channel_creation() {
        let (_, sender_key) = generate_keypair();
        let (_, receiver_key) = generate_keypair();

        let channel = PaymentChannel::new(
            UserId(Uuid::new_v4()),
            UserId(Uuid::new_v4()),
            sender_key,
            receiver_key,
            MIN_CHANNEL_CAPACITY,
        )
        .unwrap();

        assert_eq!(channel.status, ChannelStatus::Open);
        assert_eq!(channel.current_state.sender_balance, MIN_CHANNEL_CAPACITY);
        assert_eq!(channel.current_state.receiver_balance, 0);
    }

    #[test]
    fn test_channel_capacity_limits() {
        let (_, sender_key) = generate_keypair();
        let (_, receiver_key) = generate_keypair();

        // Below minimum should fail
        let result = PaymentChannel::new(
            UserId(Uuid::new_v4()),
            UserId(Uuid::new_v4()),
            sender_key,
            receiver_key,
            MIN_CHANNEL_CAPACITY - 1,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_state_update_with_signatures() {
        let (sender_signing, sender_key) = generate_keypair();
        let (_, receiver_key) = generate_keypair();

        let mut channel = PaymentChannel::new(
            UserId(Uuid::new_v4()),
            UserId(Uuid::new_v4()),
            sender_key,
            receiver_key,
            MIN_CHANNEL_CAPACITY,
        )
        .unwrap();

        // Create payment
        let update = channel.create_payment(1_000_000, &sender_signing).unwrap();

        // Apply update
        channel.update_state(update).unwrap();

        assert_eq!(
            channel.current_state.sender_balance,
            MIN_CHANNEL_CAPACITY - 1_000_000
        );
        assert_eq!(channel.current_state.receiver_balance, 1_000_000);
        assert_eq!(channel.current_state.nonce, 1);
    }

    #[test]
    fn test_invalid_nonce_rejected() {
        let (sender_signing, sender_key) = generate_keypair();
        let (_, receiver_key) = generate_keypair();

        let mut channel = PaymentChannel::new(
            UserId(Uuid::new_v4()),
            UserId(Uuid::new_v4()),
            sender_key,
            receiver_key,
            MIN_CHANNEL_CAPACITY,
        )
        .unwrap();

        // First update succeeds
        let update1 = channel.create_payment(1_000_000, &sender_signing).unwrap();
        channel.update_state(update1).unwrap();

        // Create update with same nonce (should fail)
        let state = ChannelState {
            nonce: 1, // Same as current
            sender_balance: MIN_CHANNEL_CAPACITY - 2_000_000,
            receiver_balance: 2_000_000,
        };
        let update2 =
            SignedStateUpdate::new_from_sender(state, &channel.channel_id, &sender_signing);

        let result = channel.update_state(update2);
        assert!(result.is_err());
    }

    #[test]
    fn test_balance_invariant_enforced() {
        let (sender_signing, sender_key) = generate_keypair();
        let (_, receiver_key) = generate_keypair();

        let mut channel = PaymentChannel::new(
            UserId(Uuid::new_v4()),
            UserId(Uuid::new_v4()),
            sender_key,
            receiver_key,
            MIN_CHANNEL_CAPACITY,
        )
        .unwrap();

        // Create state that violates balance invariant
        let state = ChannelState {
            nonce: 1,
            sender_balance: MIN_CHANNEL_CAPACITY - 1_000_000,
            receiver_balance: 2_000_000, // Total != capacity
        };
        let update =
            SignedStateUpdate::new_from_sender(state, &channel.channel_id, &sender_signing);

        let result = channel.update_state(update);
        assert!(result.is_err());
    }

    #[test]
    fn test_channel_manager_open_and_update() {
        let (sender_signing, sender_key) = generate_keypair();
        let (_, receiver_key) = generate_keypair();

        let manager = PaymentChannelManager::new();

        let channel = manager
            .open_channel(
                UserId(Uuid::new_v4()),
                UserId(Uuid::new_v4()),
                sender_key,
                receiver_key,
                MIN_CHANNEL_CAPACITY,
            )
            .unwrap();

        let channel_id = channel.channel_id.clone();

        // Create and apply update
        let update = channel.create_payment(1_000_000, &sender_signing).unwrap();
        manager.update_channel_state(&channel_id, update).unwrap();

        let updated = manager.get_channel(&channel_id).unwrap();
        assert_eq!(
            updated.current_state.sender_balance,
            MIN_CHANNEL_CAPACITY - 1_000_000
        );
    }

    #[test]
    fn test_unilateral_close() {
        let (sender_signing, sender_key) = generate_keypair();
        let (_, receiver_key) = generate_keypair();

        let sender_id = UserId(Uuid::new_v4());
        let receiver_id = UserId(Uuid::new_v4());

        let manager = PaymentChannelManager::new();

        let channel = manager
            .open_channel(
                sender_id.clone(),
                receiver_id.clone(),
                sender_key,
                receiver_key,
                MIN_CHANNEL_CAPACITY,
            )
            .unwrap();

        // Create a payment first
        let update = channel.create_payment(1_000_000, &sender_signing).unwrap();
        manager
            .update_channel_state(&channel.channel_id, update.clone())
            .unwrap();

        // Initiate unilateral close
        let close_req = manager
            .initiate_unilateral_close(&channel.channel_id, update, &sender_id)
            .unwrap();

        assert!(!close_req.challenged);
        assert!(close_req.dispute_deadline > Utc::now());

        // Verify channel is in Closing status
        let ch = manager.get_channel(&channel.channel_id).unwrap();
        assert_eq!(ch.status, ChannelStatus::Closing);
    }

    #[test]
    fn test_cooperative_close() {
        let (sender_signing, sender_key) = generate_keypair();
        let (receiver_signing, receiver_key) = generate_keypair();

        let manager = PaymentChannelManager::new();

        let channel = manager
            .open_channel(
                UserId(Uuid::new_v4()),
                UserId(Uuid::new_v4()),
                sender_key,
                receiver_key,
                MIN_CHANNEL_CAPACITY,
            )
            .unwrap();

        // Create bilateral state
        let state = ChannelState {
            nonce: 1,
            sender_balance: MIN_CHANNEL_CAPACITY - 3_000_000,
            receiver_balance: 3_000_000,
        };
        let mut update =
            SignedStateUpdate::new_from_sender(state, &channel.channel_id, &sender_signing);
        update.add_receiver_signature(&channel.channel_id, &receiver_signing);

        assert!(update.is_bilateral());

        // Close cooperatively
        let (sender_balance, receiver_balance) = manager
            .cooperative_close(&channel.channel_id, update)
            .unwrap();

        assert_eq!(sender_balance, MIN_CHANNEL_CAPACITY - 3_000_000);
        assert_eq!(receiver_balance, 3_000_000);

        // Verify channel is closed
        let ch = manager.get_channel(&channel.channel_id).unwrap();
        assert_eq!(ch.status, ChannelStatus::Closed);
    }

    #[test]
    fn test_message_credits_channel() {
        let (sender_signing, sender_key) = generate_keypair();
        let (_, receiver_key) = generate_keypair();

        let manager = PaymentChannelManager::new();

        let channel = manager
            .open_channel(
                UserId(Uuid::new_v4()),
                UserId(Uuid::new_v4()),
                sender_key,
                receiver_key,
                10_000_000, // 10 DCHAT
            )
            .unwrap();

        let mut credits = MessageCreditsChannel::new(
            channel.channel_id.clone(),
            Some(100_000), // 0.1 DCHAT per message
        );
        credits.set_sender_key(sender_signing);

        // Should have 100 credits (10,000,000 / 100,000)
        assert_eq!(credits.remaining_credits(&manager), 100);

        // Use a credit
        let update = credits.use_credit(&manager).unwrap();
        manager
            .update_channel_state(&channel.channel_id, update)
            .unwrap();

        // Should now have 99 credits
        assert_eq!(credits.remaining_credits(&manager), 99);
        assert_eq!(credits.total_messages(), 1);
    }
}
