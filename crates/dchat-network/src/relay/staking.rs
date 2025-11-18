//! Relay staking enforcement and validation
//!
//! Ensures relays cannot participate in the network without proper staking

use dchat_core::error::{Error, Result};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use dchat_core::config::constants::MIN_RELAY_STAKE;

/// Relay staking validator
pub struct RelayStakingValidator {
    /// Cache of validated relay stakes
    stake_cache: Arc<RwLock<HashMap<String, RelayStakeInfo>>>,
    /// Currency chain RPC endpoint
    rpc_endpoint: String,
}

/// Relay stake information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayStakeInfo {
    pub relay_key: String,
    pub stake_amount: u64,
    pub is_active: bool,
    pub stake_timestamp: u64,
    pub unlock_timestamp: u64,
    pub last_verified: std::time::SystemTime,
}

impl RelayStakingValidator {
    /// Create a new relay staking validator
    pub fn new(rpc_endpoint: String) -> Self {
        Self {
            stake_cache: Arc::new(RwLock::new(HashMap::new())),
            rpc_endpoint,
        }
    }

    /// Verify that a relay has sufficient stake to participate
    ///
    /// This is called before:
    /// - Accepting a relay into the network
    /// - Processing relay messages
    /// - Assigning relay to circuits
    pub async fn verify_relay_stake(&self, relay_key: &VerifyingKey) -> Result<bool> {
        let relay_key_hex = hex::encode(relay_key.as_bytes());

        // Check cache first
        {
            let cache = self.stake_cache.read().await;
            if let Some(info) = cache.get(&relay_key_hex) {
                // Cache valid for 5 minutes
                if info.last_verified.elapsed().unwrap_or_default().as_secs() < 300 {
                    if info.is_active && info.stake_amount >= MIN_RELAY_STAKE {
                        return Ok(true);
                    } else {
                        warn!("Relay {} has insufficient or inactive stake", relay_key_hex);
                        return Ok(false);
                    }
                }
            }
        }

        // Query chain for current stake
        let stake_info = self.query_relay_stake_from_chain(relay_key).await?;

        // Update cache
        {
            let mut cache = self.stake_cache.write().await;
            cache.insert(relay_key_hex.clone(), stake_info.clone());
        }

        // Validate stake meets minimum
        if stake_info.stake_amount < MIN_RELAY_STAKE {
            error!(
                "Relay {} stake {} below minimum {}",
                relay_key_hex, stake_info.stake_amount, MIN_RELAY_STAKE
            );
            return Ok(false);
        }

        if !stake_info.is_active {
            warn!("Relay {} stake is not active", relay_key_hex);
            return Ok(false);
        }

        info!(
            "✅ Relay {} stake verified: {} tokens",
            relay_key_hex, stake_info.stake_amount
        );

        Ok(true)
    }

    /// Query relay stake from currency chain
    async fn query_relay_stake_from_chain(&self, relay_key: &VerifyingKey) -> Result<RelayStakeInfo> {
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let relay_key_hex = hex::encode(relay_key.as_bytes());

        let query = json!({
            "method": "currency.query_relay_stake",
            "params": {
                "relay_key": relay_key_hex,
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
            .map_err(|e| Error::network(format!("Failed to query relay stake: {}", e)))?;

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

        Ok(RelayStakeInfo {
            relay_key: relay_key_hex,
            stake_amount,
            is_active,
            stake_timestamp,
            unlock_timestamp,
            last_verified: std::time::SystemTime::now(),
        })
    }

    /// Reject relay from network if stake is insufficient
    pub async fn enforce_relay_stake(&self, relay_key: &VerifyingKey) -> Result<()> {
        let is_valid = self.verify_relay_stake(relay_key).await?;

        if !is_valid {
            return Err(Error::network(format!(
                "Relay {} rejected: insufficient stake (minimum {} tokens required)",
                hex::encode(relay_key.as_bytes()),
                MIN_RELAY_STAKE
            )));
        }

        Ok(())
    }

    /// Clear cache (for testing or when chain state changes)
    pub async fn clear_cache(&self) {
        let mut cache = self.stake_cache.write().await;
        cache.clear();
        info!("Relay stake cache cleared");
    }

    /// Get cached stake info for relay
    pub async fn get_cached_stake(&self, relay_key: &VerifyingKey) -> Option<RelayStakeInfo> {
        let cache = self.stake_cache.read().await;
        cache.get(&hex::encode(relay_key.as_bytes())).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn test_relay_stake_validation() {
        let rpc_endpoint = std::env::var("CURRENCY_CHAIN_RPC")
            .unwrap_or_else(|_| "http://localhost:8545".to_string());

        let validator = RelayStakingValidator::new(rpc_endpoint);

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Test with newly generated key (should have no stake)
        let result = validator.verify_relay_stake(&verifying_key).await;
        assert!(result.is_ok());
        // New key should not have stake
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_relay_enforcement() {
        let rpc_endpoint = std::env::var("CURRENCY_CHAIN_RPC")
            .unwrap_or_else(|_| "http://localhost:8545".to_string());

        let validator = RelayStakingValidator::new(rpc_endpoint);

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Should be rejected due to insufficient stake
        let result = validator.enforce_relay_stake(&verifying_key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cache_functionality() {
        let rpc_endpoint = std::env::var("CURRENCY_CHAIN_RPC")
            .unwrap_or_else(|_| "http://localhost:8545".to_string());

        let validator = RelayStakingValidator::new(rpc_endpoint);

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Manually insert into cache
        {
            let mut cache = validator.stake_cache.write().await;
            cache.insert(
                hex::encode(verifying_key.as_bytes()),
                RelayStakeInfo {
                    relay_key: hex::encode(verifying_key.as_bytes()),
                    stake_amount: MIN_RELAY_STAKE,
                    is_active: true,
                    stake_timestamp: 0,
                    unlock_timestamp: 0,
                    last_verified: std::time::SystemTime::now(),
                },
            );
        }

        // Should retrieve from cache
        let cached = validator.get_cached_stake(&verifying_key).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().stake_amount, MIN_RELAY_STAKE);

        // Clear cache
        validator.clear_cache().await;
        let cached_after = validator.get_cached_stake(&verifying_key).await;
        assert!(cached_after.is_none());
    }
}
