use crate::types::{BridgeError, TransactionId};
use chrono::{DateTime, Utc};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Reason for slashing a validator
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashReason {
    /// Validator signed an invalid transaction
    InvalidSignature,
    /// Validator attempted double-signing
    DoubleSigning,
    /// Validator was offline for extended period
    ExtendedDowntime,
    /// Validator provided false finality proof
    FalseProof,
    /// Validator colluded with attacker
    Collusion,
}

/// Slash event record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashEvent {
    pub validator_id: UserId,
    pub reason: SlashReason,
    pub transaction_id: Option<TransactionId>,
    pub slash_amount: u64, // Amount of stake slashed
    pub evidence: Vec<u8>, // Cryptographic evidence
    pub slashed_at: DateTime<Utc>,
    pub reporter: Option<UserId>, // Who reported the violation
}

/// Validator slashing manager
pub struct SlashingManager {
    slash_events: Arc<RwLock<Vec<SlashEvent>>>,
    slashed_validators: Arc<RwLock<HashMap<UserId, u64>>>, // validator_id -> total slashed amount
}

impl SlashingManager {
    /// Create a new slashing manager
    pub fn new() -> Self {
        Self {
            slash_events: Arc::new(RwLock::new(Vec::new())),
            slashed_validators: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Slash a validator
    pub fn slash_validator(
        &self,
        validator_id: UserId,
        reason: SlashReason,
        slash_amount: u64,
        transaction_id: Option<TransactionId>,
        evidence: Vec<u8>,
        reporter: Option<UserId>,
    ) -> Result<(), BridgeError> {
        let event = SlashEvent {
            validator_id: validator_id.clone(),
            reason,
            transaction_id,
            slash_amount,
            evidence,
            slashed_at: Utc::now(),
            reporter,
        };

        // Record event
        let mut events = self.slash_events.write().unwrap();
        events.push(event);

        // Update total slashed amount
        let mut slashed = self.slashed_validators.write().unwrap();
        *slashed.entry(validator_id).or_insert(0) += slash_amount;

        Ok(())
    }

    /// Get total slashed amount for a validator
    pub fn get_slashed_amount(&self, validator_id: &UserId) -> u64 {
        let slashed = self.slashed_validators.read().unwrap();
        *slashed.get(validator_id).unwrap_or(&0)
    }

    /// Get all slash events for a validator
    pub fn get_validator_slashes(&self, validator_id: &UserId) -> Vec<SlashEvent> {
        let events = self.slash_events.read().unwrap();
        events
            .iter()
            .filter(|e| &e.validator_id == validator_id)
            .cloned()
            .collect()
    }

    /// Get all slash events
    pub fn get_all_slashes(&self) -> Vec<SlashEvent> {
        let events = self.slash_events.read().unwrap();
        events.clone()
    }

    /// Check if validator has been slashed
    pub fn is_slashed(&self, validator_id: &UserId) -> bool {
        self.get_slashed_amount(validator_id) > 0
    }

    /// Get slash count by reason
    pub fn get_slash_count_by_reason(&self, reason: &SlashReason) -> usize {
        let events = self.slash_events.read().unwrap();
        events.iter().filter(|e| &e.reason == reason).count()
    }
}

impl Default for SlashingManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slash_validator() {
        let manager = SlashingManager::new();
        let validator_id = UserId::new();

        manager
            .slash_validator(
                validator_id.clone(),
                SlashReason::InvalidSignature,
                1000,
                None,
                vec![0x01, 0x02],
                None,
            )
            .unwrap();

        assert_eq!(manager.get_slashed_amount(&validator_id), 1000);
        assert!(manager.is_slashed(&validator_id));
    }

    #[test]
    fn test_multiple_slashes() {
        let manager = SlashingManager::new();
        let validator_id = UserId::new();

        // Slash twice
        manager
            .slash_validator(
                validator_id.clone(),
                SlashReason::InvalidSignature,
                500,
                None,
                vec![],
                None,
            )
            .unwrap();

        manager
            .slash_validator(
                validator_id.clone(),
                SlashReason::DoubleSigning,
                300,
                None,
                vec![],
                None,
            )
            .unwrap();

        assert_eq!(manager.get_slashed_amount(&validator_id), 800);

        let slashes = manager.get_validator_slashes(&validator_id);
        assert_eq!(slashes.len(), 2);
    }

    #[test]
    fn test_slash_count_by_reason() {
        let manager = SlashingManager::new();
        let validator1 = UserId::new();
        let validator2 = UserId::new();

        manager
            .slash_validator(
                validator1,
                SlashReason::InvalidSignature,
                1000,
                None,
                vec![],
                None,
            )
            .unwrap();

        manager
            .slash_validator(
                validator2,
                SlashReason::InvalidSignature,
                500,
                None,
                vec![],
                None,
            )
            .unwrap();

        assert_eq!(
            manager.get_slash_count_by_reason(&SlashReason::InvalidSignature),
            2
        );
        assert_eq!(
            manager.get_slash_count_by_reason(&SlashReason::DoubleSigning),
            0
        );
    }

    #[test]
    fn test_get_all_slashes() {
        let manager = SlashingManager::new();
        let validator1 = UserId::new();
        let validator2 = UserId::new();

        manager
            .slash_validator(
                validator1,
                SlashReason::FalseProof,
                1000,
                None,
                vec![],
                None,
            )
            .unwrap();

        manager
            .slash_validator(
                validator2,
                SlashReason::ExtendedDowntime,
                500,
                None,
                vec![],
                None,
            )
            .unwrap();

        let all_slashes = manager.get_all_slashes();
        assert_eq!(all_slashes.len(), 2);
    }

    #[test]
    fn test_unslashed_validator() {
        let manager = SlashingManager::new();
        let validator_id = UserId::new();

        assert_eq!(manager.get_slashed_amount(&validator_id), 0);
        assert!(!manager.is_slashed(&validator_id));
    }
}
