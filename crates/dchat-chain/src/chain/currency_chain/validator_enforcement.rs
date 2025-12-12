//! Validator staking enforcement
//!
//! Ensures validators cannot participate in consensus without proper staking

use dchat_core::error::{Error, Result};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use dchat_core::config::constants::MIN_VALIDATOR_STAKE;

/// Validator staking enforcer
pub struct ValidatorStakingEnforcer {
    /// Cache of validated validator stakes
    stake_cache: Arc<RwLock<HashMap<String, ValidatorStakeStatus>>>,
    /// Currency chain RPC endpoint
    rpc_endpoint: String,
}

/// Validator stake status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorStakeStatus {
    pub validator_key: String,
    pub stake_amount: u64,
    pub is_active: bool,
    pub can_participate: bool,
    pub stake_timestamp: u64,
    pub unlock_timestamp: u64,
    pub last_verified: std::time::SystemTime,
}

impl ValidatorStakingEnforcer {
    /// Create a new validator staking enforcer
    pub fn new(rpc_endpoint: String) -> Self {
        Self {
            stake_cache: Arc::new(RwLock::new(HashMap::new())),
            rpc_endpoint,
        }
    }

    /// Verify that a validator has sufficient stake to participate in consensus
    ///
    /// This MUST be called before:
    /// - Accepting validator into consensus pool
    /// - Allowing validator to propose blocks
    /// - Allowing validator to sign blocks
    /// - Distributing consensus rewards
    pub async fn verify_validator_stake(&self, validator_key: &VerifyingKey) -> Result<bool> {
        let validator_key_hex = hex::encode(validator_key.as_bytes());

        // Check cache first (3 minute TTL for mainnet security)
        {
            let cache = self.stake_cache.read().await;
            if let Some(status) = cache.get(&validator_key_hex) {
                if status.last_verified.elapsed().unwrap_or_default().as_secs() < 180 {
                    if status.can_participate {
                        return Ok(true);
                    } else {
                        warn!(
                            "Validator {} cannot participate: insufficient stake or inactive",
                            validator_key_hex
                        );
                        return Ok(false);
                    }
                }
            }
        }

        // Query chain for current stake status
        let status = self.query_validator_stake_from_chain(validator_key).await?;

        // Update cache
        {
            let mut cache = self.stake_cache.write().await;
            cache.insert(validator_key_hex.clone(), status.clone());
        }

        // Validate stake meets minimum
        if status.stake_amount < MIN_VALIDATOR_STAKE {
            error!(
                "❌ Validator {} stake {} below minimum {} - REJECTING",
                validator_key_hex, status.stake_amount, MIN_VALIDATOR_STAKE
            );
            return Ok(false);
        }

        if !status.is_active {
            warn!(
                "⚠️ Validator {} stake is not active - REJECTING",
                validator_key_hex
            );
            return Ok(false);
        }

        info!(
            "✅ Validator {} stake verified: {} tokens - CAN PARTICIPATE",
            validator_key_hex, status.stake_amount
        );

        Ok(true)
    }

    /// Query validator stake status from currency chain
    async fn query_validator_stake_from_chain(
        &self,
        validator_key: &VerifyingKey,
    ) -> Result<ValidatorStakeStatus> {
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let validator_key_hex = hex::encode(validator_key.as_bytes());

        let query = json!({
            "method": "currency.query_validator_stake",
            "params": {
                "validator_key": validator_key_hex,
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        let client = HttpClient::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&query)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to query validator stake: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Chain RPC error: status {}",
                response.status()
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;

        let stake_amount = response_body["result"]["stake_amount"]
            .as_u64()
            .unwrap_or(0);

        let is_active = response_body["result"]["is_active"]
            .as_bool()
            .unwrap_or(false);

        let stake_timestamp = response_body["result"]["stake_timestamp"]
            .as_u64()
            .unwrap_or(0);

        let unlock_timestamp = response_body["result"]["unlock_timestamp"]
            .as_u64()
            .unwrap_or(0);

        let can_participate = stake_amount >= MIN_VALIDATOR_STAKE && is_active;

        Ok(ValidatorStakeStatus {
            validator_key: validator_key_hex,
            stake_amount,
            is_active,
            can_participate,
            stake_timestamp,
            unlock_timestamp,
            last_verified: std::time::SystemTime::now(),
        })
    }

    /// Enforce validator stake requirement (REJECTS if insufficient)
    ///
    /// Returns Error if validator cannot participate
    pub async fn enforce_validator_stake(&self, validator_key: &VerifyingKey) -> Result<()> {
        let is_valid = self.verify_validator_stake(validator_key).await?;

        if !is_valid {
            return Err(Error::chain(format!(
                "⛔ Validator {} REJECTED: insufficient stake (minimum {} tokens required)",
                hex::encode(validator_key.as_bytes()),
                MIN_VALIDATOR_STAKE
            )));
        }

        Ok(())
    }

    /// Check if validator can propose blocks
    pub async fn can_propose_block(&self, validator_key: &VerifyingKey) -> Result<bool> {
        self.verify_validator_stake(validator_key).await
    }

    /// Check if validator can sign blocks
    pub async fn can_sign_block(&self, validator_key: &VerifyingKey) -> Result<bool> {
        self.verify_validator_stake(validator_key).await
    }

    /// Check if validator can receive rewards
    pub async fn can_receive_rewards(&self, validator_key: &VerifyingKey) -> Result<bool> {
        self.verify_validator_stake(validator_key).await
    }

    /// Clear cache (for testing or when chain state changes)
    pub async fn clear_cache(&self) {
        let mut cache = self.stake_cache.write().await;
        cache.clear();
        info!("Validator stake cache cleared");
    }

    /// Get cached stake status for validator
    pub async fn get_cached_status(
        &self,
        validator_key: &VerifyingKey,
    ) -> Option<ValidatorStakeStatus> {
        let cache = self.stake_cache.read().await;
        cache.get(&hex::encode(validator_key.as_bytes())).cloned()
    }

    /// Get all active validators (those with sufficient stake)
    pub async fn get_active_validators(&self) -> Result<Vec<String>> {
        let cache = self.stake_cache.read().await;
        Ok(cache
            .values()
            .filter(|status| status.can_participate)
            .map(|status| status.validator_key.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn test_validator_stake_validation() {
        let rpc_endpoint = std::env::var("CURRENCY_CHAIN_RPC")
            .unwrap_or_else(|_| "http://localhost:8545".to_string());

        let enforcer = ValidatorStakingEnforcer::new(rpc_endpoint);

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Test with newly generated key (should have no stake)
        let result = enforcer.verify_validator_stake(&verifying_key).await;
        assert!(result.is_ok());
        // New key should not have stake
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_validator_enforcement() {
        let rpc_endpoint = std::env::var("CURRENCY_CHAIN_RPC")
            .unwrap_or_else(|_| "http://localhost:8545".to_string());

        let enforcer = ValidatorStakingEnforcer::new(rpc_endpoint);

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Should be rejected due to insufficient stake
        let result = enforcer.enforce_validator_stake(&verifying_key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validator_permissions() {
        let rpc_endpoint = std::env::var("CURRENCY_CHAIN_RPC")
            .unwrap_or_else(|_| "http://localhost:8545".to_string());

        let enforcer = ValidatorStakingEnforcer::new(rpc_endpoint);

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // New validator should not be able to do anything
        assert!(!enforcer.can_propose_block(&verifying_key).await.unwrap());
        assert!(!enforcer.can_sign_block(&verifying_key).await.unwrap());
        assert!(!enforcer.can_receive_rewards(&verifying_key).await.unwrap());
    }
}
