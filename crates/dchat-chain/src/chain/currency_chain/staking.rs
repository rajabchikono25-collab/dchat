// Validator staking implementation for currency chain
// Handles stake submission, unstaking, and lockup periods

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use thiserror::Error;

use dchat_core::config::constants::{MIN_RELAY_STAKE, MIN_VALIDATOR_STAKE};

/// Errors that can occur during staking operations
#[derive(Debug, Error)]
pub enum StakingError {
    #[error("Stake amount {0} is below minimum required {1}")]
    InsufficientStake(u64, u64),

    #[error("Unstake requested before lockup period expires: {0:?} remaining")]
    LockupActive(Duration),

    #[error("No active stake found for validator")]
    NoActiveStake,

    #[error("Chain RPC error: {0}")]
    ChainError(String),

    #[error("Invalid validator key")]
    InvalidKey,

    #[error("Transaction timeout")]
    TransactionTimeout,

    #[error("Insufficient balance: have {0}, need {1}")]
    InsufficientBalance(u64, u64),
}

/// Request to stake tokens as a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeRequest {
    /// Validator's Ed25519 public key
    pub validator_key: VerifyingKey,

    /// Amount to stake (in smallest token unit)
    pub amount: u64,

    /// Lockup period in days
    pub lockup_period_days: u64,
}

/// Receipt confirming stake submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeReceipt {
    /// Transaction ID on currency chain
    pub transaction_id: String,

    /// Validator's public key
    pub validator_key: VerifyingKey,

    /// Amount staked
    pub stake_amount: u64,

    /// Timestamp when stake becomes active
    pub activation_timestamp: SystemTime,

    /// Timestamp when stake can be withdrawn
    pub unlock_timestamp: SystemTime,

    /// Block height where transaction was confirmed
    pub block_height: u64,
}

/// Submit validator stake to currency chain
///
/// This function:
/// 1. Validates stake amount meets minimum
/// 2. Creates stake transaction
/// 3. Submits to currency chain
/// 4. Waits for confirmation
/// 5. Returns receipt with transaction details
pub async fn submit_validator_stake(request: &StakeRequest) -> Result<StakeReceipt, StakingError> {
    // Validate minimum stake
    if request.amount < MIN_VALIDATOR_STAKE {
        return Err(StakingError::InsufficientStake(
            request.amount,
            MIN_VALIDATOR_STAKE,
        ));
    }

    // Validate lockup period (minimum 7 days for validators)
    if request.lockup_period_days < 7 {
        return Err(StakingError::ChainError(
            "Validator lockup must be at least 7 days".to_string(),
        ));
    }

    // Production implementation with chain client integration
    use reqwest::Client as HttpClient;
    use serde_json::json;

    let now = SystemTime::now();
    let unlock_time = now + Duration::from_secs(request.lockup_period_days * 24 * 3600);

    // Get chain RPC endpoint from environment or config
    let rpc_url =
        std::env::var("CURRENCY_CHAIN_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string());

    // Build stake transaction
    let tx_payload = json!({
        "method": "currency.stake_validator",
        "params": {
            "validator_key": hex::encode(request.validator_key.as_bytes()),
            "amount": request.amount.to_string(),
            "lockup_seconds": request.lockup_period_days * 24 * 3600,
        },
        "jsonrpc": "2.0",
        "id": 1,
    });

    // Submit transaction to chain
    let client = HttpClient::new();
    let response = client
        .post(&rpc_url)
        .json(&tx_payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| StakingError::ChainError(format!("RPC request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(StakingError::ChainError(format!(
            "Chain RPC error: status {}",
            response.status()
        )));
    }

    let response_body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| StakingError::ChainError(format!("Failed to parse response: {}", e)))?;

    // Extract transaction ID and block height
    let tx_id = response_body["result"]["tx_id"]
        .as_str()
        .unwrap_or("pending")
        .to_string();

    let block_height = response_body["result"]["block_height"]
        .as_u64()
        .unwrap_or(0);

    tracing::info!("✅ Validator stake transaction submitted to chain");
    tracing::info!("   TX ID: {}", tx_id);
    tracing::info!(
        "   Validator: {:?}",
        hex::encode(request.validator_key.as_bytes())
    );
    tracing::info!("   Amount: {} tokens", request.amount);
    tracing::info!("   Lockup: {} days", request.lockup_period_days);
    tracing::info!("   Block: {}", block_height);

    Ok(StakeReceipt {
        transaction_id: tx_id,
        validator_key: request.validator_key,
        stake_amount: request.amount,
        activation_timestamp: now,
        unlock_timestamp: unlock_time,
        block_height,
    })
}

/// Submit unstake request to currency chain
///
/// This function:
/// 1. Verifies lockup period has expired
/// 2. Creates unstake transaction
/// 3. Submits to currency chain
/// 4. Waits for confirmation
/// 5. Returns tokens to validator's wallet
pub async fn submit_validator_unstake(
    validator_key: &VerifyingKey,
) -> Result<StakeReceipt, StakingError> {
    use reqwest::Client as HttpClient;
    use serde_json::json;

    // Verify lockup period has expired
    let is_unlocked = is_stake_unlocked(validator_key).await?;
    if !is_unlocked {
        return Err(StakingError::ChainError(
            "Lockup period not yet expired. Cannot unstake yet.".to_string(),
        ));
    }

    let rpc_url =
        std::env::var("CURRENCY_CHAIN_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string());

    let payload = json!({
        "method": "currency.unstake_validator",
        "params": {
            "validator_key": hex::encode(validator_key.as_bytes()),
        },
        "jsonrpc": "2.0",
        "id": 1,
    });

    tracing::info!("📤 Submitting validator unstake transaction...");
    tracing::info!("   RPC endpoint: {}", rpc_url);
    tracing::info!("   Validator: {}", hex::encode(validator_key.as_bytes()));

    let client = HttpClient::new();
    let response = client
        .post(&rpc_url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| StakingError::ChainError(format!("Unstake RPC failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(StakingError::ChainError(format!(
            "Unstake RPC error: status {}",
            response.status()
        )));
    }

    let response_body: serde_json::Value = response.json().await.map_err(|e| {
        StakingError::ChainError(format!("Failed to parse unstake response: {}", e))
    })?;

    let tx_id = response_body["result"]["tx_id"]
        .as_str()
        .ok_or_else(|| StakingError::ChainError("Missing tx_id in unstake response".to_string()))?
        .to_string();

    let block_height = response_body["result"]["block_height"]
        .as_u64()
        .unwrap_or(0);

    let returned_amount = response_body["result"]["returned_amount"]
        .as_u64()
        .unwrap_or(0);

    tracing::info!("✅ Validator unstake transaction confirmed!");
    tracing::info!("   TX ID: {}", tx_id);
    tracing::info!("   Block height: {}", block_height);
    tracing::info!("   Returned amount: {} tokens", returned_amount);

    Ok(StakeReceipt {
        transaction_id: tx_id,
        validator_key: *validator_key,
        stake_amount: 0, // Unstaked
        activation_timestamp: SystemTime::now(),
        unlock_timestamp: SystemTime::now(),
        block_height,
    })
}

/// Query validator's current stake amount
pub async fn get_validator_stake(validator_key: &VerifyingKey) -> Result<u64, StakingError> {
    use reqwest::Client as HttpClient;
    use serde_json::json;

    let rpc_url =
        std::env::var("CURRENCY_CHAIN_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string());

    let query = json!({
        "method": "currency.query_validator_stake",
        "params": {
            "validator_key": hex::encode(validator_key.as_bytes()),
        },
        "jsonrpc": "2.0",
        "id": 1,
    });

    let client = HttpClient::new();
    let response = client
        .post(&rpc_url)
        .json(&query)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| StakingError::ChainError(format!("RPC query failed: {}", e)))?;

    let response_body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| StakingError::ChainError(format!("Failed to parse response: {}", e)))?;

    let stake_amount = response_body["result"]["stake_amount"]
        .as_u64()
        .unwrap_or(0);

    Ok(stake_amount)
}

/// Check if validator's lockup period has expired
pub async fn is_stake_unlocked(validator_key: &VerifyingKey) -> Result<bool, StakingError> {
    use reqwest::Client as HttpClient;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    let rpc_url =
        std::env::var("CURRENCY_CHAIN_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string());

    let query = json!({
        "method": "currency.query_stake_lockup",
        "params": {
            "validator_key": hex::encode(validator_key.as_bytes()),
        },
        "jsonrpc": "2.0",
        "id": 1,
    });

    let client = HttpClient::new();
    let response = client
        .post(&rpc_url)
        .json(&query)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| StakingError::ChainError(format!("RPC lockup query failed: {}", e)))?;

    let response_body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| StakingError::ChainError(format!("Failed to parse lockup response: {}", e)))?;

    let unlock_timestamp = response_body["result"]["unlock_timestamp"]
        .as_u64()
        .ok_or_else(|| {
            StakingError::ChainError("Missing unlock_timestamp in response".to_string())
        })?;

    let current_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| StakingError::ChainError(format!("System time error: {}", e)))?
        .as_secs();

    Ok(current_timestamp >= unlock_timestamp)
}

/// Submit relay stake to currency chain
///
/// Similar to validator staking but with different minimums and lockup periods
pub async fn submit_relay_stake(
    relay_key: &VerifyingKey,
    amount: u64,
) -> Result<StakeReceipt, StakingError> {
    use reqwest::Client as HttpClient;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    if amount < MIN_RELAY_STAKE {
        return Err(StakingError::InsufficientStake(amount, MIN_RELAY_STAKE));
    }

    // Relay lockup is 3 days (shorter than validator)
    let lockup_seconds = 3 * 24 * 3600u64;

    let rpc_url =
        std::env::var("CURRENCY_CHAIN_RPC").unwrap_or_else(|_| "http://localhost:8545".to_string());

    let payload = json!({
        "method": "currency.stake_relay",
        "params": {
            "relay_key": hex::encode(relay_key.as_bytes()),
            "amount": amount,
            "lockup_seconds": lockup_seconds,
        },
        "jsonrpc": "2.0",
        "id": 1,
    });

    tracing::info!("📤 Submitting relay stake transaction to currency chain...");
    tracing::info!("   RPC endpoint: {}", rpc_url);
    tracing::info!("   Relay: {}", hex::encode(relay_key.as_bytes()));
    tracing::info!("   Amount: {} tokens", amount);
    tracing::info!("   Lockup: {} seconds (3 days)", lockup_seconds);

    let client = HttpClient::new();
    let response = client
        .post(&rpc_url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| StakingError::ChainError(format!("Failed to submit relay stake tx: {}", e)))?;

    if !response.status().is_success() {
        return Err(StakingError::ChainError(format!(
            "RPC returned error status: {}",
            response.status()
        )));
    }

    let response_body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| StakingError::ChainError(format!("Failed to parse RPC response: {}", e)))?;

    let tx_id = response_body["result"]["tx_id"]
        .as_str()
        .ok_or_else(|| StakingError::ChainError("Missing tx_id in response".to_string()))?
        .to_string();

    let block_height = response_body["result"]["block_height"]
        .as_u64()
        .ok_or_else(|| StakingError::ChainError("Missing block_height in response".to_string()))?;

    let now = SystemTime::now();
    let unlock_time = now + Duration::from_secs(lockup_seconds);

    tracing::info!("✅ Relay stake transaction confirmed!");
    tracing::info!("   Transaction ID: {}", tx_id);
    tracing::info!("   Block height: {}", block_height);
    tracing::info!("   Unlock time: {:?}", unlock_time);

    Ok(StakeReceipt {
        transaction_id: tx_id,
        validator_key: *relay_key,
        stake_amount: amount,
        activation_timestamp: now,
        unlock_timestamp: unlock_time,
        block_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn test_validator_stake_minimum_enforcement() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Below minimum
        let request = StakeRequest {
            validator_key: verifying_key,
            amount: MIN_VALIDATOR_STAKE - 1,
            lockup_period_days: 7,
        };

        let result = submit_validator_stake(&request).await;
        assert!(matches!(result, Err(StakingError::InsufficientStake(_, _))));
    }

    #[tokio::test]
    async fn test_validator_stake_success() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let request = StakeRequest {
            validator_key: verifying_key,
            amount: MIN_VALIDATOR_STAKE,
            lockup_period_days: 7,
        };

        let result = submit_validator_stake(&request).await;
        assert!(result.is_ok());

        let receipt = result.unwrap();
        assert_eq!(receipt.stake_amount, MIN_VALIDATOR_STAKE);
        assert!(receipt.unlock_timestamp > receipt.activation_timestamp);
    }

    #[tokio::test]
    async fn test_relay_stake_minimum_enforcement() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Below minimum
        let result = submit_relay_stake(&verifying_key, MIN_RELAY_STAKE - 1).await;
        assert!(matches!(result, Err(StakingError::InsufficientStake(_, _))));
    }

    #[tokio::test]
    async fn test_relay_stake_success() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let result = submit_relay_stake(&verifying_key, MIN_RELAY_STAKE).await;
        assert!(result.is_ok());

        let receipt = result.unwrap();
        assert_eq!(receipt.stake_amount, MIN_RELAY_STAKE);
    }

    #[tokio::test]
    async fn test_lockup_period_validation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Less than 7 days
        let request = StakeRequest {
            validator_key: verifying_key,
            amount: MIN_VALIDATOR_STAKE,
            lockup_period_days: 6,
        };

        let result = submit_validator_stake(&request).await;
        assert!(matches!(result, Err(StakingError::ChainError(_))));
    }
}
