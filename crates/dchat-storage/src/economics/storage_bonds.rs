//! Storage bond staking system
//!
//! Users must stake tokens to store data on the network
//! Implements Section 23 (Data Lifecycle & Storage Economics) from ARCHITECTURE.md

use dchat_core::error::{Error, Result};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Minimum storage bond per GB (in smallest token unit)
pub const MIN_STORAGE_BOND_PER_GB: u64 = 100_000;

/// Storage bond manager
pub struct StorageBondManager {
    /// Active storage bonds
    bonds: Arc<RwLock<HashMap<String, StorageBond>>>,
    /// Currency chain RPC endpoint
    rpc_endpoint: String,
}

/// Storage bond information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBond {
    /// User's public key
    pub user_key: String,
    /// Amount of tokens bonded
    pub bond_amount: u64,
    /// Storage quota in bytes
    pub storage_quota_bytes: u64,
    /// Currently used storage in bytes
    pub used_storage_bytes: u64,
    /// Bond creation timestamp
    pub created_at: u64,
    /// Bond expiration timestamp (optional)
    pub expires_at: Option<u64>,
    /// Whether bond is active
    pub is_active: bool,
}

/// Storage bond request
#[derive(Debug, Clone)]
pub struct StorageBondRequest {
    /// User's public key
    pub user_key: VerifyingKey,
    /// Amount to bond (in smallest token unit)
    pub bond_amount: u64,
    /// Requested storage quota in GB
    pub storage_quota_gb: u64,
}

/// Storage bond receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBondReceipt {
    /// Transaction ID on currency chain
    pub transaction_id: String,
    /// User's public key
    pub user_key: String,
    /// Bonded amount
    pub bond_amount: u64,
    /// Allocated storage quota in bytes
    pub storage_quota_bytes: u64,
    /// Block height
    pub block_height: u64,
}

impl StorageBondManager {
    /// Create a new storage bond manager
    pub fn new(rpc_endpoint: String) -> Self {
        Self {
            bonds: Arc::new(RwLock::new(HashMap::new())),
            rpc_endpoint,
        }
    }

    /// Submit storage bond to currency chain
    pub async fn submit_storage_bond(&self, request: &StorageBondRequest) -> Result<StorageBondReceipt> {
        let user_key_hex = hex::encode(request.user_key.as_bytes());

        // Calculate required bond
        let required_bond = request.storage_quota_gb * MIN_STORAGE_BOND_PER_GB;

        if request.bond_amount < required_bond {
            return Err(Error::validation(format!(
                "Insufficient bond: provided {}, required {} for {} GB",
                request.bond_amount, required_bond, request.storage_quota_gb
            )));
        }

        // Submit to currency chain
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let payload = json!({
            "method": "currency.create_storage_bond",
            "params": {
                "user_key": user_key_hex,
                "bond_amount": request.bond_amount,
                "storage_quota_bytes": request.storage_quota_gb * 1_073_741_824, // GB to bytes
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        info!("📤 Submitting storage bond transaction...");
        info!("   User: {}", user_key_hex);
        info!("   Bond amount: {} tokens", request.bond_amount);
        info!("   Storage quota: {} GB", request.storage_quota_gb);

        let client = HttpClient::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to submit storage bond: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "RPC error: status {}",
                response.status()
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;

        let tx_id = response_body["result"]["tx_id"]
            .as_str()
            .ok_or_else(|| Error::network("Missing tx_id in response".to_string()))?
            .to_string();

        let block_height = response_body["result"]["block_height"]
            .as_u64()
            .ok_or_else(|| Error::network("Missing block_height in response".to_string()))?;

        let storage_quota_bytes = request.storage_quota_gb * 1_073_741_824;

        // Cache the bond
        let bond = StorageBond {
            user_key: user_key_hex.clone(),
            bond_amount: request.bond_amount,
            storage_quota_bytes,
            used_storage_bytes: 0,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            expires_at: None,
            is_active: true,
        };

        {
            let mut bonds = self.bonds.write().await;
            bonds.insert(user_key_hex.clone(), bond);
        }

        info!("✅ Storage bond created!");
        info!("   Transaction ID: {}", tx_id);
        info!("   Block height: {}", block_height);
        info!("   Quota allocated: {} GB", request.storage_quota_gb);

        Ok(StorageBondReceipt {
            transaction_id: tx_id,
            user_key: user_key_hex,
            bond_amount: request.bond_amount,
            storage_quota_bytes,
            block_height,
        })
    }

    /// Check if user has sufficient storage quota
    pub async fn check_storage_quota(
        &self,
        user_key: &VerifyingKey,
        required_bytes: u64,
    ) -> Result<bool> {
        let user_key_hex = hex::encode(user_key.as_bytes());

        // Try cache first
        {
            let bonds = self.bonds.read().await;
            if let Some(bond) = bonds.get(&user_key_hex) {
                if !bond.is_active {
                    return Ok(false);
                }
                let available = bond.storage_quota_bytes.saturating_sub(bond.used_storage_bytes);
                return Ok(available >= required_bytes);
            }
        }

        // Query from chain
        let bond = self.query_storage_bond_from_chain(user_key).await?;

        if !bond.is_active {
            warn!("User {} storage bond is not active", user_key_hex);
            return Ok(false);
        }

        let available = bond.storage_quota_bytes.saturating_sub(bond.used_storage_bytes);
        Ok(available >= required_bytes)
    }

    /// Record storage usage
    pub async fn record_storage_usage(&self, user_key: &VerifyingKey, bytes_used: u64) -> Result<()> {
        let user_key_hex = hex::encode(user_key.as_bytes());

        let mut bonds = self.bonds.write().await;
        if let Some(bond) = bonds.get_mut(&user_key_hex) {
            bond.used_storage_bytes += bytes_used;

            if bond.used_storage_bytes > bond.storage_quota_bytes {
                error!(
                    "User {} exceeded storage quota: used {}, quota {}",
                    user_key_hex, bond.used_storage_bytes, bond.storage_quota_bytes
                );
                return Err(Error::validation("Storage quota exceeded".to_string()));
            }

            info!(
                "Storage usage recorded for {}: +{} bytes (total: {} / {})",
                user_key_hex, bytes_used, bond.used_storage_bytes, bond.storage_quota_bytes
            );
        }

        Ok(())
    }

    /// Query storage bond from currency chain
    async fn query_storage_bond_from_chain(&self, user_key: &VerifyingKey) -> Result<StorageBond> {
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let user_key_hex = hex::encode(user_key.as_bytes());

        let query = json!({
            "method": "currency.query_storage_bond",
            "params": {
                "user_key": user_key_hex,
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
            .map_err(|e| Error::network(format!("Failed to query storage bond: {}", e)))?;

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;

        let bond_amount = response_body["result"]["bond_amount"].as_u64().unwrap_or(0);
        let storage_quota_bytes = response_body["result"]["storage_quota_bytes"]
            .as_u64()
            .unwrap_or(0);
        let used_storage_bytes = response_body["result"]["used_storage_bytes"]
            .as_u64()
            .unwrap_or(0);
        let is_active = response_body["result"]["is_active"].as_bool().unwrap_or(false);
        let created_at = response_body["result"]["created_at"].as_u64().unwrap_or(0);

        Ok(StorageBond {
            user_key: user_key_hex,
            bond_amount,
            storage_quota_bytes,
            used_storage_bytes,
            created_at,
            expires_at: None,
            is_active,
        })
    }

    /// Release storage bond
    pub async fn release_bond(&self, user_key: &VerifyingKey) -> Result<u64> {
        let user_key_hex = hex::encode(user_key.as_bytes());

        use reqwest::Client as HttpClient;
        use serde_json::json;

        let payload = json!({
            "method": "currency.release_storage_bond",
            "params": {
                "user_key": user_key_hex,
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        let client = HttpClient::new();
        let response = client
            .post(&self.rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to release bond: {}", e)))?;

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;

        let returned_amount = response_body["result"]["returned_amount"]
            .as_u64()
            .unwrap_or(0);

        // Remove from cache
        {
            let mut bonds = self.bonds.write().await;
            bonds.remove(&user_key_hex);
        }

        info!("✅ Storage bond released for {}", user_key_hex);
        info!("   Returned amount: {} tokens", returned_amount);

        Ok(returned_amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn test_storage_bond_creation() {
        // Skip if no RPC endpoint is set
        if std::env::var("CURRENCY_CHAIN_RPC").is_err() {
            println!("Skipping test_storage_bond_creation - CURRENCY_CHAIN_RPC not set");
            return;
        }
        
        let rpc_endpoint = std::env::var("CURRENCY_CHAIN_RPC").unwrap();
        let manager = StorageBondManager::new(rpc_endpoint);

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let request = StorageBondRequest {
            user_key: verifying_key,
            bond_amount: MIN_STORAGE_BOND_PER_GB * 10, // 10 GB
            storage_quota_gb: 10,
        };

        let result = manager.submit_storage_bond(&request).await;
        assert!(result.is_ok());

        let receipt = result.unwrap();
        assert_eq!(receipt.bond_amount, MIN_STORAGE_BOND_PER_GB * 10);
    }

    #[tokio::test]
    async fn test_insufficient_bond() {
        // This test validates the bond amount check without RPC
        let manager = StorageBondManager::new("http://unused:8545".to_string());

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let request = StorageBondRequest {
            user_key: verifying_key,
            bond_amount: MIN_STORAGE_BOND_PER_GB / 2, // Insufficient
            storage_quota_gb: 10,
        };

        // This should fail before making any RPC call due to insufficient bond
        let result = manager.submit_storage_bond(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_storage_quota_check() {
        let manager = StorageBondManager::new("http://unused:8545".to_string());

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Add a bond to the cache first so we don't make RPC calls
        let user_key_hex = hex::encode(verifying_key.as_bytes());
        {
            let mut bonds = manager.bonds.write().await;
            bonds.insert(user_key_hex.clone(), StorageBond {
                user_key: user_key_hex,
                bond_amount: MIN_STORAGE_BOND_PER_GB * 10,
                storage_quota_bytes: 10 * 1_073_741_824, // 10 GB
                used_storage_bytes: 5 * 1_073_741_824,   // 5 GB used
                created_at: 0,
                expires_at: None,
                is_active: true,
            });
        }

        // Should have quota for 1 MB (5 GB available)
        let has_quota = manager
            .check_storage_quota(&verifying_key, 1_000_000)
            .await
            .unwrap();
        assert!(has_quota);

        // Should NOT have quota for 6 GB (only 5 GB available)
        let has_large_quota = manager
            .check_storage_quota(&verifying_key, 6 * 1_073_741_824)
            .await
            .unwrap();
        assert!(!has_large_quota);
    }
}
