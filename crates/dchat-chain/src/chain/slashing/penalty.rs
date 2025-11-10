// Slashing Penalty Application
//
// Applies stake reductions and bans to validators/relays
// after slashing evidence is confirmed on-chain.

use ed25519_dalek::VerifyingKey;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

use super::detector::SlashableOffense;
use super::evidence::SlashingEvidence;
use dchat_core::config::constants::{
    SLASH_RATE_CENSORSHIP, SLASH_RATE_DOUBLE_SIGN, SLASH_RATE_INVALID_PROOF, SLASH_RATE_LOW_UPTIME,
};

/// Errors during penalty application
#[derive(Debug, Error)]
pub enum PenaltyError {
    #[error("Validator not found: {0:?}")]
    ValidatorNotFound(VerifyingKey),

    #[error("Insufficient stake to slash")]
    InsufficientStake,

    #[error("Already banned")]
    AlreadyBanned,

    #[error("Invalid penalty amount")]
    InvalidAmount,

    #[error("Chain submission failed: {0}")]
    ChainSubmissionFailed(String),
}

/// Penalty to be applied
#[derive(Debug, Clone)]
pub struct SlashingPenalty {
    /// Validator being penalized
    pub validator: VerifyingKey,

    /// Amount of stake to slash (in tokens)
    pub slash_amount: u64,

    /// Slash rate (percentage as decimal, e.g., 1.0 = 100%)
    pub slash_rate: f64,

    /// Original stake before slashing
    pub original_stake: u64,

    /// Should the validator be permanently banned?
    pub ban: bool,

    /// Offense that triggered this penalty
    pub offense_type: String,
}

impl SlashingPenalty {
    /// Create a penalty from evidence and current stake
    pub fn from_evidence(
        evidence: &SlashingEvidence,
        current_stake: u64,
    ) -> Result<Self, PenaltyError> {
        // Deserialize accused validator key
        let validator_bytes: [u8; 32] = evidence
            .accused
            .as_slice()
            .try_into()
            .map_err(|_| PenaltyError::InvalidAmount)?;
        let validator =
            VerifyingKey::from_bytes(&validator_bytes).map_err(|_| PenaltyError::InvalidAmount)?;

        // Deserialize offense to determine penalty
        let offense: SlashableOffense = serde_json::from_slice(&evidence.offense_data)
            .map_err(|_| PenaltyError::InvalidAmount)?;

        let slash_rate = offense.slash_rate();
        let ban = offense.requires_ban();

        // Calculate slash amount
        let slash_amount = (current_stake as f64 * slash_rate).floor() as u64;

        if slash_amount > current_stake {
            return Err(PenaltyError::InsufficientStake);
        }

        let offense_type = match offense {
            SlashableOffense::DoubleSigning { .. } => "DoubleSigning",
            SlashableOffense::InvalidProof { .. } => "InvalidProof",
            SlashableOffense::Censorship { .. } => "Censorship",
            SlashableOffense::LowUptime { .. } => "LowUptime",
        }
        .to_string();

        Ok(Self {
            validator,
            slash_amount,
            slash_rate,
            original_stake: current_stake,
            ban,
            offense_type,
        })
    }

    /// Get remaining stake after penalty
    pub fn remaining_stake(&self) -> u64 {
        self.original_stake.saturating_sub(self.slash_amount)
    }
}

/// Penalty record for tracking
#[derive(Debug, Clone)]
pub struct PenaltyRecord {
    pub validator: VerifyingKey,
    pub slash_amount: u64,
    pub offense_type: String,
    pub timestamp: u64,
    pub banned: bool,
}

/// Penalty application engine
pub struct PenaltyApplicator {
    /// History of applied penalties
    penalty_history: Arc<RwLock<Vec<PenaltyRecord>>>,

    /// Banned validators (permanent)
    banned_validators: Arc<RwLock<HashMap<VerifyingKey, String>>>,

    /// Total slashed amount (for metrics)
    total_slashed: Arc<RwLock<u64>>,
}

impl PenaltyApplicator {
    /// Create a new penalty applicator
    pub fn new() -> Self {
        Self {
            penalty_history: Arc::new(RwLock::new(Vec::new())),
            banned_validators: Arc::new(RwLock::new(HashMap::new())),
            total_slashed: Arc::new(RwLock::new(0)),
        }
    }

    /// Apply a slashing penalty (call this after on-chain confirmation)
    pub async fn apply_penalty(
        &self,
        penalty: SlashingPenalty,
    ) -> Result<PenaltyRecord, PenaltyError> {
        // Check if already banned
        let banned = self.banned_validators.read().unwrap();
        if banned.contains_key(&penalty.validator) {
            return Err(PenaltyError::AlreadyBanned);
        }
        drop(banned);

        // Apply ban if required
        if penalty.ban {
            let mut banned = self.banned_validators.write().unwrap();
            banned.insert(penalty.validator, penalty.offense_type.clone());
            tracing::warn!(
                "🚫 BANNED: Validator {:?} permanently banned for {}",
                penalty.validator,
                penalty.offense_type
            );
        }

        // Update total slashed amount
        let mut total = self.total_slashed.write().unwrap();
        *total += penalty.slash_amount;

        // Create penalty record
        let record = PenaltyRecord {
            validator: penalty.validator,
            slash_amount: penalty.slash_amount,
            offense_type: penalty.offense_type.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            banned: penalty.ban,
        };

        // Store in history
        let mut history = self.penalty_history.write().unwrap();
        history.push(record.clone());

        tracing::warn!(
            "⚡ SLASHED: Validator {:?} slashed {} tokens ({:.1}%) for {}",
            penalty.validator,
            penalty.slash_amount,
            penalty.slash_rate * 100.0,
            penalty.offense_type
        );

        Ok(record)
    }

    /// Check if a validator is banned
    pub fn is_banned(&self, validator: &VerifyingKey) -> bool {
        self.banned_validators
            .read()
            .unwrap()
            .contains_key(validator)
    }

    /// Get ban reason if validator is banned
    pub fn get_ban_reason(&self, validator: &VerifyingKey) -> Option<String> {
        self.banned_validators
            .read()
            .unwrap()
            .get(validator)
            .cloned()
    }

    /// Get penalty history for a validator
    pub fn get_validator_history(&self, validator: &VerifyingKey) -> Vec<PenaltyRecord> {
        self.penalty_history
            .read()
            .unwrap()
            .iter()
            .filter(|r| &r.validator == validator)
            .cloned()
            .collect()
    }

    /// Get all penalty history
    pub fn get_all_history(&self) -> Vec<PenaltyRecord> {
        self.penalty_history.read().unwrap().clone()
    }

    /// Get total slashed amount across all validators
    pub fn get_total_slashed(&self) -> u64 {
        *self.total_slashed.read().unwrap()
    }

    /// Get statistics
    pub fn get_stats(&self) -> PenaltyStats {
        let history = self.penalty_history.read().unwrap();
        let banned = self.banned_validators.read().unwrap();
        let total = *self.total_slashed.read().unwrap();

        let mut by_offense: HashMap<String, u64> = HashMap::new();
        for record in history.iter() {
            *by_offense.entry(record.offense_type.clone()).or_insert(0) += record.slash_amount;
        }

        PenaltyStats {
            total_penalties: history.len(),
            total_slashed: total,
            total_banned: banned.len(),
            slashed_by_offense: by_offense,
        }
    }
}

impl Default for PenaltyApplicator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct PenaltyStats {
    pub total_penalties: usize,
    pub total_slashed: u64,
    pub total_banned: usize,
    pub slashed_by_offense: HashMap<String, u64>,
}

/// Simulate stake reduction (for testing before on-chain submission)
pub fn simulate_penalty(current_stake: u64, slash_rate: f64) -> (u64, u64) {
    let slash_amount = (current_stake as f64 * slash_rate).floor() as u64;
    let remaining = current_stake.saturating_sub(slash_amount);
    (slash_amount, remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn test_penalty_creation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let offense = SlashableOffense::DoubleSigning {
            block_height: 100,
            signature1: vec![1; 64],
            signature2: vec![2; 64],
            message1_hash: vec![1; 32],
            message2_hash: vec![2; 32],
        };

        let evidence = SlashingEvidence::from_offense(verifying_key, &offense, vec![]);
        let current_stake = 10_000_000u64;

        let penalty = SlashingPenalty::from_evidence(&evidence, current_stake).unwrap();

        assert_eq!(penalty.slash_rate, SLASH_RATE_DOUBLE_SIGN);
        assert_eq!(penalty.slash_amount, current_stake); // 100% slash
        assert!(penalty.ban);
        assert_eq!(penalty.remaining_stake(), 0);
    }

    #[tokio::test]
    async fn test_penalty_application() {
        let applicator = PenaltyApplicator::new();
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let penalty = SlashingPenalty {
            validator: verifying_key,
            slash_amount: 2_000_000,
            slash_rate: 0.20,
            original_stake: 10_000_000,
            ban: false,
            offense_type: "InvalidProof".to_string(),
        };

        let record = applicator.apply_penalty(penalty).await.unwrap();

        assert_eq!(record.slash_amount, 2_000_000);
        assert!(!record.banned);
        assert_eq!(applicator.get_total_slashed(), 2_000_000);
    }

    #[tokio::test]
    async fn test_ban_enforcement() {
        let applicator = PenaltyApplicator::new();
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let penalty = SlashingPenalty {
            validator: verifying_key,
            slash_amount: 10_000_000,
            slash_rate: 1.0,
            original_stake: 10_000_000,
            ban: true,
            offense_type: "DoubleSigning".to_string(),
        };

        applicator.apply_penalty(penalty.clone()).await.unwrap();

        assert!(applicator.is_banned(&verifying_key));
        assert_eq!(
            applicator.get_ban_reason(&verifying_key),
            Some("DoubleSigning".to_string())
        );

        // Try to apply another penalty - should fail
        let result = applicator.apply_penalty(penalty).await;
        assert!(matches!(result, Err(PenaltyError::AlreadyBanned)));
    }

    #[test]
    fn test_penalty_simulation() {
        let stake = 10_000_000u64;

        // 100% slash (double-signing)
        let (slash, remaining) = simulate_penalty(stake, SLASH_RATE_DOUBLE_SIGN);
        assert_eq!(slash, stake);
        assert_eq!(remaining, 0);

        // 20% slash (invalid proof)
        let (slash, remaining) = simulate_penalty(stake, SLASH_RATE_INVALID_PROOF);
        assert_eq!(slash, 2_000_000);
        assert_eq!(remaining, 8_000_000);

        // 10% slash (censorship)
        let (slash, remaining) = simulate_penalty(stake, SLASH_RATE_CENSORSHIP);
        assert_eq!(slash, 1_000_000);
        assert_eq!(remaining, 9_000_000);
    }

    #[tokio::test]
    async fn test_penalty_history() {
        let applicator = PenaltyApplicator::new();
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Apply multiple penalties
        for i in 0..3 {
            let penalty = SlashingPenalty {
                validator: verifying_key,
                slash_amount: 1_000_000,
                slash_rate: 0.10,
                original_stake: 10_000_000,
                ban: false,
                offense_type: format!("Offense{}", i),
            };
            applicator.apply_penalty(penalty).await.unwrap();
        }

        let history = applicator.get_validator_history(&verifying_key);
        assert_eq!(history.len(), 3);
        assert_eq!(applicator.get_total_slashed(), 3_000_000);
    }

    #[tokio::test]
    async fn test_penalty_stats() {
        let applicator = PenaltyApplicator::new();

        for i in 0..5 {
            let signing_key = SigningKey::generate(&mut OsRng);
            let verifying_key = signing_key.verifying_key();

            let penalty = SlashingPenalty {
                validator: verifying_key,
                slash_amount: 1_000_000,
                slash_rate: 0.10,
                original_stake: 10_000_000,
                ban: i == 0, // Ban first one
                offense_type: "Test".to_string(),
            };
            applicator.apply_penalty(penalty).await.unwrap();
        }

        let stats = applicator.get_stats();
        assert_eq!(stats.total_penalties, 5);
        assert_eq!(stats.total_slashed, 5_000_000);
        assert_eq!(stats.total_banned, 1);
    }
}
