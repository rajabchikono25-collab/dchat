//! Oracle Network for Temporal Stake Consensus (TSC)
//!
//! Provides predictive validation signals for TSC consensus:
//! - Message traffic predictions
//! - Network load forecasting
//! - Stake concentration analysis
//! - Validator behavior prediction
//! - Economic risk assessment
//!
//! Key features:
//! - Decentralized oracle network with reputation scoring
//! - Weighted aggregation of multiple oracle sources
//! - Outlier detection and slashing for bad oracles
//! - Stake-weighted oracle participation

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing;

#[derive(Debug, Error)]
pub enum OracleError {
    #[error("Oracle not registered: {0}")]
    OracleNotRegistered(String),

    #[error("Insufficient oracle stake: {0} < {1} minimum")]
    InsufficientStake(u64, u64),

    #[error("Invalid oracle signature: {0}")]
    InvalidSignature(String),

    #[error("Oracle data expired: submitted at {0}, current time {1}")]
    DataExpired(DateTime<Utc>, DateTime<Utc>),

    #[error("Outlier oracle data: {0}")]
    OutlierDetected(String),

    #[error("Oracle slashed: {0}")]
    OracleSlashed(String),
}

pub type Result<T> = std::result::Result<T, OracleError>;

/// Oracle prediction categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PredictionType {
    /// Message traffic prediction (messages/hour)
    MessageTraffic,

    /// Network load prediction (0.0-1.0)
    NetworkLoad,

    /// Stake concentration Gini coefficient (0.0-1.0)
    StakeConcentration,

    /// Validator reliability score (0.0-1.0)
    ValidatorReliability,

    /// Economic risk assessment (0.0-1.0)
    EconomicRisk,
}

/// Oracle prediction data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OraclePrediction {
    pub prediction_type: PredictionType,
    pub value: f64,
    pub confidence: f64, // 0.0-1.0
    pub timestamp: DateTime<Utc>,
    pub oracle_pubkey: VerifyingKey,
    pub signature: Signature,
}

impl OraclePrediction {
    /// Verify oracle signature
    pub fn verify(&self) -> Result<()> {
        let message = self.signing_message();
        self.oracle_pubkey
            .verify_strict(&message, &self.signature)
            .map_err(|e| OracleError::InvalidSignature(format!("{:?}", e)))
    }

    /// Create message for signing
    fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&(self.prediction_type as u8).to_le_bytes());
        msg.extend_from_slice(&self.value.to_le_bytes());
        msg.extend_from_slice(&self.confidence.to_le_bytes());
        msg.extend_from_slice(&self.timestamp.timestamp().to_le_bytes());
        msg
    }

    /// Check if prediction is still fresh (within 5 minutes)
    pub fn is_fresh(&self) -> bool {
        let now = Utc::now();
        let age = now.signed_duration_since(self.timestamp);
        age.num_seconds() <= 300 // 5 minutes
    }
}

/// Oracle registration and stake
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRegistration {
    pub pubkey: VerifyingKey,
    pub stake_amount: u64,
    pub reputation_score: f64, // 0.0-1.0
    pub total_predictions: u64,
    pub correct_predictions: u64,
    pub slashed_amount: u64,
    pub registration_time: DateTime<Utc>,
    pub last_prediction_time: Option<DateTime<Utc>>,
    pub is_active: bool,
}

impl OracleRegistration {
    /// Calculate oracle weight based on stake and reputation
    pub fn calculate_weight(&self) -> f64 {
        if !self.is_active || self.slashed_amount > 0 {
            return 0.0;
        }

        // Logarithmic stake weight to prevent whale dominance
        let stake_weight = (self.stake_amount as f64).ln().max(1.0);

        // Reputation multiplier (0.5x - 2.0x)
        let reputation_multiplier = 0.5 + (self.reputation_score * 1.5);

        // Experience bonus (up to 1.5x after 10,000 predictions)
        let experience_multiplier = if self.total_predictions > 0 {
            1.0 + ((self.total_predictions as f64).ln() / 10.0).min(0.5)
        } else {
            1.0 // No bonus for new oracles
        };

        stake_weight * reputation_multiplier * experience_multiplier
    }

    /// Update reputation based on prediction accuracy
    pub fn update_reputation(&mut self, was_correct: bool) {
        self.total_predictions += 1;
        if was_correct {
            self.correct_predictions += 1;
        }

        let accuracy = self.correct_predictions as f64 / self.total_predictions as f64;

        // Exponential moving average (EMA) for reputation
        const ALPHA: f64 = 0.1; // Smoothing factor
        self.reputation_score = ALPHA * accuracy + (1.0 - ALPHA) * self.reputation_score;
    }
}

/// Aggregated oracle consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConsensus {
    pub prediction_type: PredictionType,
    pub weighted_median: f64,
    pub confidence_interval: (f64, f64),
    pub total_weight: f64,
    pub oracle_count: usize,
    pub timestamp: DateTime<Utc>,
}

/// Oracle network manager
pub struct OracleNetwork {
    /// Registered oracles
    oracles: Arc<RwLock<HashMap<VerifyingKey, OracleRegistration>>>,

    /// Recent predictions (last 100 per type)
    predictions: Arc<RwLock<HashMap<PredictionType, Vec<OraclePrediction>>>>,

    /// Minimum stake requirement (in DCHAT tokens)
    min_stake: u64,
}

impl OracleNetwork {
    /// Create new oracle network
    pub fn new(min_stake: u64) -> Self {
        Self {
            oracles: Arc::new(RwLock::new(HashMap::new())),
            predictions: Arc::new(RwLock::new(HashMap::new())),
            min_stake,
        }
    }

    /// Register new oracle
    pub fn register_oracle(&self, pubkey: VerifyingKey, stake_amount: u64) -> Result<()> {
        if stake_amount < self.min_stake {
            return Err(OracleError::InsufficientStake(stake_amount, self.min_stake));
        }

        let registration = OracleRegistration {
            pubkey,
            stake_amount,
            reputation_score: 0.5, // Start at neutral
            total_predictions: 0,
            correct_predictions: 0,
            slashed_amount: 0,
            registration_time: Utc::now(),
            last_prediction_time: None,
            is_active: true,
        };

        let mut oracles = self.oracles.write().unwrap();
        oracles.insert(pubkey, registration);

        tracing::info!(
            "Oracle registered: {:?} with stake {}",
            pubkey,
            stake_amount
        );

        Ok(())
    }

    /// Submit oracle prediction
    pub fn submit_prediction(&self, prediction: OraclePrediction) -> Result<()> {
        // Verify signature
        prediction.verify()?;

        // Check if oracle is registered
        let mut oracles = self.oracles.write().unwrap();
        let oracle = oracles.get_mut(&prediction.oracle_pubkey).ok_or_else(|| {
            OracleError::OracleNotRegistered(format!("{:?}", prediction.oracle_pubkey))
        })?;

        if !oracle.is_active {
            return Err(OracleError::OracleSlashed(format!(
                "{:?}",
                prediction.oracle_pubkey
            )));
        }

        // Check freshness
        if !prediction.is_fresh() {
            return Err(OracleError::DataExpired(prediction.timestamp, Utc::now()));
        }

        // Update oracle last prediction time
        oracle.last_prediction_time = Some(prediction.timestamp);

        // Store prediction
        let mut predictions = self.predictions.write().unwrap();
        let type_predictions = predictions
            .entry(prediction.prediction_type)
            .or_insert_with(Vec::new);

        type_predictions.push(prediction.clone());

        // Keep only last 100 predictions per type
        if type_predictions.len() > 100 {
            type_predictions.remove(0);
        }

        tracing::debug!(
            "Oracle prediction submitted: {:?} = {} (confidence: {})",
            prediction.prediction_type,
            prediction.value,
            prediction.confidence
        );

        Ok(())
    }

    /// Get oracle consensus for a prediction type
    pub fn get_consensus(&self, prediction_type: PredictionType) -> Result<OracleConsensus> {
        let predictions = self.predictions.read().unwrap();
        let type_predictions = predictions.get(&prediction_type).ok_or_else(|| {
            OracleError::OracleNotRegistered("No predictions available".to_string())
        })?;

        if type_predictions.is_empty() {
            return Err(OracleError::OracleNotRegistered(
                "No predictions available".to_string(),
            ));
        }

        // Filter fresh predictions (last 5 minutes)
        let fresh_predictions: Vec<_> = type_predictions.iter().filter(|p| p.is_fresh()).collect();

        if fresh_predictions.is_empty() {
            return Err(OracleError::DataExpired(
                type_predictions.last().unwrap().timestamp,
                Utc::now(),
            ));
        }

        // Calculate weighted predictions
        let oracles = self.oracles.read().unwrap();
        let mut weighted_values = Vec::new();
        let mut total_weight = 0.0;

        for pred in &fresh_predictions {
            if let Some(oracle) = oracles.get(&pred.oracle_pubkey) {
                let weight = oracle.calculate_weight() * pred.confidence;
                weighted_values.push((pred.value, weight));
                total_weight += weight;
            }
        }

        // Sort by value for median calculation
        weighted_values.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        // Calculate weighted median
        let mut cumulative_weight = 0.0;
        let target_weight = total_weight / 2.0;
        let mut weighted_median = 0.0;

        for (value, weight) in &weighted_values {
            cumulative_weight += weight;
            if cumulative_weight >= target_weight {
                weighted_median = *value;
                break;
            }
        }

        // Calculate confidence interval (25th-75th percentile)
        let p25_target = total_weight * 0.25;
        let p75_target = total_weight * 0.75;

        cumulative_weight = 0.0;
        let mut p25 = weighted_values[0].0;
        let mut p75 = weighted_values[weighted_values.len() - 1].0;

        for (value, weight) in &weighted_values {
            cumulative_weight += weight;
            if cumulative_weight >= p25_target && p25 == weighted_values[0].0 {
                p25 = *value;
            }
            if cumulative_weight >= p75_target {
                p75 = *value;
                break;
            }
        }

        Ok(OracleConsensus {
            prediction_type,
            weighted_median,
            confidence_interval: (p25, p75),
            total_weight,
            oracle_count: fresh_predictions.len(),
            timestamp: Utc::now(),
        })
    }

    /// Detect and slash outlier oracles
    pub fn detect_outliers(
        &self,
        prediction_type: PredictionType,
        actual_value: f64,
    ) -> Vec<VerifyingKey> {
        let predictions = self.predictions.read().unwrap();
        let type_predictions = predictions.get(&prediction_type);

        if type_predictions.is_none() {
            return Vec::new();
        }

        let mut slashed_oracles = Vec::new();
        let mut oracles = self.oracles.write().unwrap();

        for pred in type_predictions.unwrap() {
            if !pred.is_fresh() {
                continue;
            }

            // Calculate prediction error
            let error = (pred.value - actual_value).abs();
            let relative_error = error / actual_value.max(1.0);

            // Slash if error > 50% and confidence was high
            if relative_error > 0.5 && pred.confidence > 0.7 {
                if let Some(oracle) = oracles.get_mut(&pred.oracle_pubkey) {
                    let slash_amount = (oracle.stake_amount as f64 * 0.1) as u64; // 10% slash
                    oracle.slashed_amount += slash_amount;
                    oracle.stake_amount -= slash_amount;
                    oracle.update_reputation(false);

                    tracing::warn!(
                        "Oracle {:?} slashed {} for outlier prediction: predicted {}, actual {}",
                        pred.oracle_pubkey,
                        slash_amount,
                        pred.value,
                        actual_value
                    );

                    slashed_oracles.push(pred.oracle_pubkey);

                    // Deactivate if slashed more than 50% of original stake
                    if oracle.slashed_amount > oracle.stake_amount {
                        oracle.is_active = false;
                        tracing::warn!(
                            "Oracle {:?} deactivated due to excessive slashing",
                            pred.oracle_pubkey
                        );
                    }
                }
            } else {
                // Update reputation for good predictions
                if let Some(oracle) = oracles.get_mut(&pred.oracle_pubkey) {
                    oracle.update_reputation(relative_error < 0.2);
                }
            }
        }

        slashed_oracles
    }

    /// Get oracle statistics
    pub fn get_oracle_stats(&self, pubkey: &VerifyingKey) -> Option<OracleRegistration> {
        let oracles = self.oracles.read().unwrap();
        oracles.get(pubkey).cloned()
    }

    /// Get all active oracles
    pub fn get_active_oracles(&self) -> Vec<OracleRegistration> {
        let oracles = self.oracles.read().unwrap();
        oracles.values().filter(|o| o.is_active).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_oracle_registration() {
        let network = OracleNetwork::new(1000);
        let keypair = SigningKey::generate(&mut OsRng);
        let pubkey = keypair.verifying_key();

        // Test successful registration
        assert!(network.register_oracle(pubkey, 5000).is_ok());

        // Test insufficient stake
        let keypair2 = SigningKey::generate(&mut OsRng);
        let pubkey2 = keypair2.verifying_key();
        assert!(network.register_oracle(pubkey2, 500).is_err());
    }

    #[test]
    fn test_oracle_weight_calculation() {
        let mut oracle = OracleRegistration {
            pubkey: SigningKey::generate(&mut OsRng).verifying_key(),
            stake_amount: 10000,
            reputation_score: 0.8,
            total_predictions: 1000,
            correct_predictions: 800,
            slashed_amount: 0,
            registration_time: Utc::now(),
            last_prediction_time: None,
            is_active: true,
        };

        let weight = oracle.calculate_weight();
        assert!(weight > 0.0);

        // Test slashed oracle has zero weight
        oracle.slashed_amount = 1000;
        assert_eq!(oracle.calculate_weight(), 0.0);
    }
}
