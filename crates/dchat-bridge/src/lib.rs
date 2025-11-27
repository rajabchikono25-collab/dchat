//! Cross-Chain Bridge Infrastructure
//!
//! This module implements atomic cross-chain operations between:
//! - Chat chain (identity, messaging, governance)
//! - Currency chain (payments, staking, economics)
//!
//! Features:
//! - Atomic swaps with rollback safety
//! - Finality tracking
//! - State synchronization
//! - Bridge validator consensus
//! - Multi-signature validation (M-of-N)

use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

pub mod finality;
pub mod multisig;
pub mod slashing;

// Re-export types for multisig module
pub mod types {
    use super::*;
    pub use dchat_core::types::UserId;
    pub type TransactionId = Uuid;

    /// Bridge-specific error types
    #[derive(Debug, Clone)]
    pub enum BridgeError {
        InvalidThreshold,
        DuplicateValidator,
        UnknownValidator,
        DuplicateSignature,
        InvalidSignature,
        TransactionAlreadyExists,
        TransactionNotFound,
    }

    impl fmt::Display for BridgeError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                BridgeError::InvalidThreshold => write!(f, "Invalid multi-sig threshold"),
                BridgeError::DuplicateValidator => write!(f, "Duplicate validator in set"),
                BridgeError::UnknownValidator => write!(f, "Unknown validator"),
                BridgeError::DuplicateSignature => write!(f, "Duplicate signature"),
                BridgeError::InvalidSignature => write!(f, "Invalid signature"),
                BridgeError::TransactionAlreadyExists => write!(f, "Transaction already exists"),
                BridgeError::TransactionNotFound => write!(f, "Transaction not found"),
            }
        }
    }

    impl std::error::Error for BridgeError {}

    /// Error type for BridgeManager initialization
    #[derive(Debug)]
    pub enum BridgeManagerError {
        /// Multi-sig configuration error
        MultiSigConfig(BridgeError),
        /// Finality tracker initialization error
        FinalityTracker(super::finality::FinalityError),
    }

    impl fmt::Display for BridgeManagerError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                BridgeManagerError::MultiSigConfig(e) => write!(f, "MultiSig config error: {}", e),
                BridgeManagerError::FinalityTracker(e) => write!(f, "Finality tracker error: {}", e),
            }
        }
    }

    impl std::error::Error for BridgeManagerError {}

    impl From<BridgeError> for BridgeManagerError {
        fn from(e: BridgeError) -> Self {
            BridgeManagerError::MultiSigConfig(e)
        }
    }

    impl From<super::finality::FinalityError> for BridgeManagerError {
        fn from(e: super::finality::FinalityError) -> Self {
            BridgeManagerError::FinalityTracker(e)
        }
    }
}

/// Blockchain identifier
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ChainId {
    /// Chat chain for messaging and identity
    ChatChain,
    /// Currency chain for economics
    CurrencyChain,
}

/// Cross-chain transaction status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BridgeTransactionStatus {
    /// Transaction initiated on source chain
    Initiated,
    /// Waiting for finality on source chain
    PendingFinality,
    /// Ready to execute on destination chain
    ReadyToExecute,
    /// Executed on destination chain
    Executed,
    /// Failed and rolled back
    RolledBack,
    /// Timed out
    TimedOut,
}

/// A cross-chain bridge transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransaction {
    pub id: Uuid,
    pub source_chain: ChainId,
    pub destination_chain: ChainId,
    pub initiator: UserId,
    pub source_tx_hash: String,
    pub destination_tx_hash: Option<String>,
    pub amount: u64,
    pub status: BridgeTransactionStatus,
    pub initiated_at: DateTime<Utc>,
    pub finalized_at: Option<DateTime<Utc>>,
    pub timeout_at: DateTime<Utc>,
}

/// Finality proof for a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalityProof {
    pub chain: ChainId,
    pub tx_hash: String,
    pub block_number: u64,
    pub confirmations: u32,
    pub required_confirmations: u32,
    pub proof_data: Vec<u8>,
    pub is_final: bool,
}

/// Bridge validator for cross-chain consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeValidator {
    pub validator_id: UserId,
    pub stake_amount: u64,
    pub is_active: bool,
    pub uptime_score: f32,
}

/// Bridge state synchronization record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSyncRecord {
    pub id: Uuid,
    pub chain: ChainId,
    pub state_key: String,
    pub state_value: Vec<u8>,
    pub block_number: u64,
    pub synced_at: DateTime<Utc>,
}

/// Bridge manager for cross-chain operations
pub struct BridgeManager {
    transactions: HashMap<Uuid, BridgeTransaction>,
    validators: HashMap<UserId, BridgeValidator>,
    finality_proofs: HashMap<String, FinalityProof>,
    state_sync: Vec<StateSyncRecord>,
    required_confirmations: HashMap<ChainId, u32>,
    pub multisig: multisig::MultiSigManager,
    pub slashing: slashing::SlashingManager,
    pub finality_tracker: finality::FinalityTracker,
}

impl BridgeManager {
    /// Create a new bridge manager with default 2-of-3 multi-sig.
    /// 
    /// # Panics
    /// 
    /// This method panics if the default configuration is invalid.
    /// For fallible construction, use [`try_new`](#method.try_new).
    pub fn new() -> Self {
        Self::try_new().expect("Failed to create BridgeManager with default configuration")
    }

    /// Create a new bridge manager with default 2-of-3 multi-sig (fallible).
    /// 
    /// Returns an error if the configuration is invalid.
    pub fn try_new() -> std::result::Result<Self, types::BridgeManagerError> {
        let mut required_confirmations = HashMap::new();
        required_confirmations.insert(ChainId::ChatChain, 12);
        required_confirmations.insert(ChainId::CurrencyChain, 20);

        // Create default validator set (3 validators, 2-of-3 threshold)
        let validator1 = multisig::ValidatorId::new(UserId::new(), vec![1; 32]);
        let validator2 = multisig::ValidatorId::new(UserId::new(), vec![2; 32]);
        let validator3 = multisig::ValidatorId::new(UserId::new(), vec![3; 32]);

        let multisig_config =
            multisig::MultiSigConfig::new(2, vec![validator1, validator2, validator3])?;

        // Create finality tracker with 5-of-7 consensus
        let finality_tracker = finality::FinalityTracker::new(5, 7)?;

        Ok(Self {
            transactions: HashMap::new(),
            validators: HashMap::new(),
            finality_proofs: HashMap::new(),
            state_sync: Vec::new(),
            required_confirmations,
            multisig: multisig::MultiSigManager::new(multisig_config),
            slashing: slashing::SlashingManager::new(),
            finality_tracker,
        })
    }

    /// Initiate a cross-chain transaction
    pub fn initiate_transaction(
        &mut self,
        source_chain: ChainId,
        destination_chain: ChainId,
        initiator: UserId,
        source_tx_hash: String,
        amount: u64,
        timeout_seconds: i64,
    ) -> Result<Uuid> {
        if source_chain == destination_chain {
            return Err(Error::validation(
                "Source and destination chains must be different",
            ));
        }

        let transaction = BridgeTransaction {
            id: Uuid::new_v4(),
            source_chain,
            destination_chain,
            initiator,
            source_tx_hash,
            destination_tx_hash: None,
            amount,
            status: BridgeTransactionStatus::Initiated,
            initiated_at: Utc::now(),
            finalized_at: None,
            timeout_at: Utc::now() + chrono::Duration::seconds(timeout_seconds),
        };

        let tx_id = transaction.id;
        self.transactions.insert(tx_id, transaction);
        Ok(tx_id)
    }

    /// Submit finality proof for a transaction
    pub fn submit_finality_proof(
        &mut self,
        tx_hash: String,
        chain: ChainId,
        block_number: u64,
        confirmations: u32,
        proof_data: Vec<u8>,
    ) -> Result<()> {
        let required = self
            .required_confirmations
            .get(&chain)
            .copied()
            .unwrap_or(12);

        let proof = FinalityProof {
            chain: chain.clone(),
            tx_hash: tx_hash.clone(),
            block_number,
            confirmations,
            required_confirmations: required,
            proof_data,
            is_final: confirmations >= required,
        };

        self.finality_proofs.insert(tx_hash, proof);
        Ok(())
    }

    /// Check if transaction has reached finality
    pub fn check_finality(&self, tx_hash: &str) -> bool {
        // Check both old finality proofs and new BLS-based tracker
        self.finality_proofs
            .get(tx_hash)
            .map(|p| p.is_final)
            .unwrap_or_else(|| self.finality_tracker.is_finalized(tx_hash))
    }

    /// Initiate BLS-aggregated finality proof
    pub fn initiate_bls_finality(
        &mut self,
        tx_hash: String,
        block_number: u64,
        block_hash: String,
        confirmations: u32,
    ) -> Result<()> {
        let required = self
            .required_confirmations
            .get(&ChainId::ChatChain)
            .copied()
            .unwrap_or(12);

        self.finality_tracker
            .initiate_proof(tx_hash, block_number, block_hash, confirmations, required)
            .map_err(|e| Error::validation(format!("Failed to initiate finality proof: {}", e)))?;

        Ok(())
    }

    /// Submit validator BLS signature for finality
    pub fn submit_finality_signature(
        &mut self,
        tx_hash: &str,
        validator_id: UserId,
        signature: Vec<u8>,
    ) -> Result<bool> {
        let is_finalized = self
            .finality_tracker
            .submit_signature(tx_hash, validator_id, signature)
            .map_err(|e| Error::validation(format!("Failed to submit signature: {}", e)))?;

        Ok(is_finalized)
    }

    /// Register validator for BLS finality consensus
    pub fn register_bls_validator(
        &mut self,
        validator_id: UserId,
        bls_pubkey: Vec<u8>,
    ) -> Result<()> {
        self.finality_tracker
            .register_validator(validator_id, bls_pubkey)
            .map_err(|e| Error::validation(format!("Failed to register validator: {}", e)))?;

        Ok(())
    }

    /// Verify BLS finality proof
    pub fn verify_bls_finality(&self, tx_hash: &str, message: &[u8]) -> Result<bool> {
        self.finality_tracker
            .verify_proof(tx_hash, message)
            .map_err(|e| Error::validation(format!("Finality verification failed: {}", e)))
    }

    /// Update transaction status to pending finality
    pub fn update_pending_finality(&mut self, tx_id: Uuid) -> Result<()> {
        let tx = self
            .transactions
            .get_mut(&tx_id)
            .ok_or_else(|| Error::validation("Transaction not found"))?;

        if tx.status != BridgeTransactionStatus::Initiated {
            return Err(Error::validation("Invalid status transition"));
        }

        tx.status = BridgeTransactionStatus::PendingFinality;
        Ok(())
    }

    /// Mark transaction as ready to execute
    pub fn mark_ready_to_execute(&mut self, tx_id: Uuid) -> Result<()> {
        // Check finality first (before mutable borrow)
        let source_tx_hash = self
            .transactions
            .get(&tx_id)
            .map(|tx| tx.source_tx_hash.clone())
            .ok_or_else(|| Error::validation("Transaction not found"))?;

        if !self.check_finality(&source_tx_hash) {
            return Err(Error::validation("Transaction not finalized"));
        }

        // Now get mutable reference
        let tx = self
            .transactions
            .get_mut(&tx_id)
            .ok_or_else(|| Error::validation("Transaction not found"))?;

        tx.status = BridgeTransactionStatus::ReadyToExecute;
        Ok(())
    }

    /// Execute transaction on destination chain
    pub fn execute_transaction(&mut self, tx_id: Uuid, destination_tx_hash: String) -> Result<()> {
        let tx = self
            .transactions
            .get_mut(&tx_id)
            .ok_or_else(|| Error::validation("Transaction not found"))?;

        if tx.status != BridgeTransactionStatus::ReadyToExecute {
            return Err(Error::validation("Transaction not ready to execute"));
        }

        tx.destination_tx_hash = Some(destination_tx_hash);
        tx.status = BridgeTransactionStatus::Executed;
        tx.finalized_at = Some(Utc::now());
        Ok(())
    }

    /// Rollback a failed transaction
    pub fn rollback_transaction(&mut self, tx_id: Uuid) -> Result<()> {
        let tx = self
            .transactions
            .get_mut(&tx_id)
            .ok_or_else(|| Error::validation("Transaction not found"))?;

        tx.status = BridgeTransactionStatus::RolledBack;
        tx.finalized_at = Some(Utc::now());
        Ok(())
    }

    /// Check for timed out transactions
    pub fn check_timeouts(&mut self) -> Vec<Uuid> {
        let now = Utc::now();
        let mut timed_out = Vec::new();

        for (id, tx) in self.transactions.iter_mut() {
            if tx.timeout_at < now && tx.status != BridgeTransactionStatus::Executed {
                tx.status = BridgeTransactionStatus::TimedOut;
                timed_out.push(*id);
            }
        }

        timed_out
    }

    /// Register a bridge validator
    pub fn register_validator(&mut self, validator_id: UserId, stake_amount: u64) -> Result<()> {
        if stake_amount < 1000 {
            return Err(Error::validation("Insufficient stake amount"));
        }

        let validator = BridgeValidator {
            validator_id: validator_id.clone(),
            stake_amount,
            is_active: true,
            uptime_score: 100.0,
        };

        self.validators.insert(validator_id, validator);
        Ok(())
    }

    /// Update validator uptime score
    pub fn update_validator_score(&mut self, validator_id: &UserId, score: f32) -> Result<()> {
        let validator = self
            .validators
            .get_mut(validator_id)
            .ok_or_else(|| Error::validation("Validator not found"))?;

        if !(0.0..=100.0).contains(&score) {
            return Err(Error::validation("Score must be between 0 and 100"));
        }

        validator.uptime_score = score;

        // Deactivate if score too low
        if score < 50.0 {
            validator.is_active = false;
        }

        Ok(())
    }

    /// Synchronize state between chains
    pub fn sync_state(
        &mut self,
        chain: ChainId,
        state_key: String,
        state_value: Vec<u8>,
        block_number: u64,
    ) -> Result<Uuid> {
        let record = StateSyncRecord {
            id: Uuid::new_v4(),
            chain,
            state_key,
            state_value,
            block_number,
            synced_at: Utc::now(),
        };

        let record_id = record.id;
        self.state_sync.push(record);
        Ok(record_id)
    }

    /// Get transaction by ID
    pub fn get_transaction(&self, tx_id: Uuid) -> Option<&BridgeTransaction> {
        self.transactions.get(&tx_id)
    }

    /// Get all active validators
    pub fn get_active_validators(&self) -> Vec<&BridgeValidator> {
        self.validators.values().filter(|v| v.is_active).collect()
    }

    /// Get finality proof
    pub fn get_finality_proof(&self, tx_hash: &str) -> Option<&FinalityProof> {
        self.finality_proofs.get(tx_hash)
    }
}

impl Default for BridgeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user(_name: &str) -> UserId {
        UserId::new()
    }

    #[test]
    fn test_initiate_transaction() {
        let mut bridge = BridgeManager::new();
        let user = create_test_user("alice");

        let tx_id = bridge
            .initiate_transaction(
                ChainId::ChatChain,
                ChainId::CurrencyChain,
                user,
                "tx_123".to_string(),
                1000,
                3600,
            )
            .unwrap();

        let tx = bridge.get_transaction(tx_id).unwrap();
        assert_eq!(tx.status, BridgeTransactionStatus::Initiated);
        assert_eq!(tx.amount, 1000);
    }

    #[test]
    fn test_same_chain_rejected() {
        let mut bridge = BridgeManager::new();
        let user = create_test_user("bob");

        let result = bridge.initiate_transaction(
            ChainId::ChatChain,
            ChainId::ChatChain,
            user,
            "tx_456".to_string(),
            500,
            3600,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_finality_proof() {
        let mut bridge = BridgeManager::new();

        bridge
            .submit_finality_proof(
                "tx_789".to_string(),
                ChainId::ChatChain,
                100,
                15,
                vec![1, 2, 3],
            )
            .unwrap();

        assert!(bridge.check_finality("tx_789"));
    }

    #[test]
    fn test_finality_not_reached() {
        let mut bridge = BridgeManager::new();

        bridge
            .submit_finality_proof(
                "tx_abc".to_string(),
                ChainId::ChatChain,
                100,
                5,
                vec![1, 2, 3],
            )
            .unwrap();

        assert!(!bridge.check_finality("tx_abc"));
    }

    #[test]
    fn test_transaction_flow() {
        let mut bridge = BridgeManager::new();
        let user = create_test_user("charlie");

        let tx_id = bridge
            .initiate_transaction(
                ChainId::ChatChain,
                ChainId::CurrencyChain,
                user,
                "tx_def".to_string(),
                2000,
                3600,
            )
            .unwrap();

        bridge.update_pending_finality(tx_id).unwrap();

        bridge
            .submit_finality_proof("tx_def".to_string(), ChainId::ChatChain, 200, 20, vec![])
            .unwrap();

        bridge.mark_ready_to_execute(tx_id).unwrap();
        bridge
            .execute_transaction(tx_id, "dest_tx_123".to_string())
            .unwrap();

        let tx = bridge.get_transaction(tx_id).unwrap();
        assert_eq!(tx.status, BridgeTransactionStatus::Executed);
        assert!(tx.finalized_at.is_some());
    }

    #[test]
    fn test_rollback() {
        let mut bridge = BridgeManager::new();
        let user = create_test_user("dave");

        let tx_id = bridge
            .initiate_transaction(
                ChainId::CurrencyChain,
                ChainId::ChatChain,
                user,
                "tx_ghi".to_string(),
                1500,
                3600,
            )
            .unwrap();

        bridge.rollback_transaction(tx_id).unwrap();

        let tx = bridge.get_transaction(tx_id).unwrap();
        assert_eq!(tx.status, BridgeTransactionStatus::RolledBack);
    }

    #[test]
    fn test_register_validator() {
        let mut bridge = BridgeManager::new();
        let validator = create_test_user("validator1");

        bridge.register_validator(validator.clone(), 5000).unwrap();

        let validators = bridge.get_active_validators();
        assert_eq!(validators.len(), 1);
        assert_eq!(validators[0].stake_amount, 5000);
    }

    #[test]
    fn test_insufficient_stake() {
        let mut bridge = BridgeManager::new();
        let validator = create_test_user("validator2");

        let result = bridge.register_validator(validator, 500);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_validator_score() {
        let mut bridge = BridgeManager::new();
        let validator = create_test_user("validator3");

        bridge.register_validator(validator.clone(), 3000).unwrap();
        bridge.update_validator_score(&validator, 85.0).unwrap();

        let val = bridge.validators.get(&validator).unwrap();
        assert_eq!(val.uptime_score, 85.0);
        assert!(val.is_active);
    }

    #[test]
    fn test_deactivate_low_score_validator() {
        let mut bridge = BridgeManager::new();
        let validator = create_test_user("validator4");

        bridge.register_validator(validator.clone(), 2000).unwrap();
        bridge.update_validator_score(&validator, 30.0).unwrap();

        let val = bridge.validators.get(&validator).unwrap();
        assert!(!val.is_active);
    }

    #[test]
    fn test_state_sync() {
        let mut bridge = BridgeManager::new();

        let record_id = bridge
            .sync_state(
                ChainId::ChatChain,
                "user_balance".to_string(),
                vec![1, 2, 3, 4],
                150,
            )
            .unwrap();

        assert_ne!(record_id, Uuid::nil());
        assert_eq!(bridge.state_sync.len(), 1);
    }
}
