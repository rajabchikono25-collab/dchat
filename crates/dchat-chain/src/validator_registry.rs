//! Validator Registry
//!
//! Provides a standardized interface for querying validator information from the blockchain.
//! This module replaces temporary deterministic key derivation with real on-chain lookups.

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Information about a registered validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Validator identifier
    pub validator_id: String,
    /// Ed25519 public key (32 bytes)
    pub public_key: [u8; 32],
    /// Current stake amount
    pub stake: u64,
    /// Whether validator is active
    pub is_active: bool,
    /// Region/location
    pub region: Option<String>,
    /// Registration timestamp
    pub registered_at: u64,
}

/// Validator registry trait for on-chain lookups
#[async_trait::async_trait]
pub trait ValidatorRegistry: Send + Sync {
    /// Get validator information by validator ID
    async fn get_validator_info(&self, validator_id: &str) -> Result<ValidatorInfo>;

    /// Get validator public key by validator ID
    async fn get_validator_pubkey(&self, validator_id: &str) -> Result<[u8; 32]> {
        let info = self.get_validator_info(validator_id).await?;
        Ok(info.public_key)
    }

    /// Get all active validators
    async fn get_active_validators(&self) -> Result<Vec<ValidatorInfo>>;

    /// Check if validator is active
    async fn is_validator_active(&self, validator_id: &str) -> Result<bool> {
        match self.get_validator_info(validator_id).await {
            Ok(info) => Ok(info.is_active),
            Err(_) => Ok(false),
        }
    }
}

/// In-memory validator registry for testing and development
pub struct InMemoryValidatorRegistry {
    validators: Arc<RwLock<HashMap<String, ValidatorInfo>>>,
}

impl InMemoryValidatorRegistry {
    pub fn new() -> Self {
        Self {
            validators: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a validator (for testing)
    pub async fn register_validator(&self, info: ValidatorInfo) {
        let mut validators = self.validators.write().await;
        validators.insert(info.validator_id.clone(), info);
    }
}

impl Default for InMemoryValidatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ValidatorRegistry for InMemoryValidatorRegistry {
    async fn get_validator_info(&self, validator_id: &str) -> Result<ValidatorInfo> {
        let validators = self.validators.read().await;
        validators.get(validator_id).cloned().ok_or_else(|| {
            Error::NotFound(format!("Validator {} not found in registry", validator_id))
        })
    }

    async fn get_active_validators(&self) -> Result<Vec<ValidatorInfo>> {
        let validators = self.validators.read().await;
        Ok(validators
            .values()
            .filter(|v| v.is_active)
            .cloned()
            .collect())
    }
}

/// On-chain validator registry implementation
pub struct OnChainValidatorRegistry {
    rpc_endpoint: String,
    cache: Arc<RwLock<HashMap<String, (ValidatorInfo, std::time::SystemTime)>>>,
    cache_ttl_secs: u64,
    metrics: Option<Arc<dchat_observability::MetricsCollector>>,
}

impl OnChainValidatorRegistry {
    pub fn new(rpc_endpoint: String) -> Self {
        Self {
            rpc_endpoint,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl_secs: 300, // 5 minute cache
            metrics: None,
        }
    }

    pub fn with_cache_ttl(mut self, ttl_secs: u64) -> Self {
        self.cache_ttl_secs = ttl_secs;
        self
    }

    pub fn with_metrics(mut self, metrics: Arc<dchat_observability::MetricsCollector>) -> Self {
        self.metrics = Some(metrics);
        self
    }
}

#[async_trait::async_trait]
impl ValidatorRegistry for OnChainValidatorRegistry {
    async fn get_validator_info(&self, validator_id: &str) -> Result<ValidatorInfo> {
        let start = Instant::now();

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some((info, timestamp)) = cache.get(validator_id) {
                if timestamp.elapsed().unwrap_or_default().as_secs() < self.cache_ttl_secs {
                    tracing::debug!("Validator {} info retrieved from cache", validator_id);

                    // Record cache hit metric
                    if let Some(metrics) = &self.metrics {
                        let mut labels = HashMap::new();
                        labels.insert("result".to_string(), "cache_hit".to_string());
                        let _ = metrics
                            .record_counter(
                                "validator_registry_queries_total".to_string(),
                                1.0,
                                labels,
                                "Total validator registry queries".to_string(),
                            )
                            .await;
                    }

                    return Ok(info.clone());
                }
            }
        }

        // Query from blockchain
        tracing::debug!(
            "Querying validator {} from blockchain at {}",
            validator_id,
            self.rpc_endpoint
        );

        use reqwest::Client as HttpClient;
        use serde_json::json;

        let query = json!({
            "method": "validator.get_info",
            "params": {
                "validator_id": validator_id,
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
            .map_err(|e| Error::network(format!("Failed to query validator registry: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Validator registry RPC error: status {}",
                response.status()
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse validator info: {}", e)))?;

        // Check for error in response
        if let Some(error) = response_body.get("error") {
            return Err(Error::network(format!(
                "Validator registry error: {}",
                error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error")
            )));
        }

        let result = response_body
            .get("result")
            .ok_or_else(|| Error::network("Missing result in validator registry response"))?;

        // Parse public key
        let public_key_hex = result
            .get("public_key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| Error::validation("Missing public_key in validator info"))?;

        let public_key_bytes = hex::decode(public_key_hex)
            .map_err(|e| Error::validation(format!("Invalid public key hex: {}", e)))?;

        if public_key_bytes.len() != 32 {
            return Err(Error::validation(format!(
                "Invalid public key length for validator {}: expected 32, got {}",
                validator_id,
                public_key_bytes.len()
            )));
        }

        let mut public_key = [0u8; 32];
        public_key.copy_from_slice(&public_key_bytes);

        let info = ValidatorInfo {
            validator_id: validator_id.to_string(),
            public_key,
            stake: result.get("stake").and_then(|s| s.as_u64()).unwrap_or(0),
            is_active: result
                .get("is_active")
                .and_then(|a| a.as_bool())
                .unwrap_or(false),
            region: result
                .get("region")
                .and_then(|r| r.as_str())
                .map(|s| s.to_string()),
            registered_at: result
                .get("registered_at")
                .and_then(|t| t.as_u64())
                .unwrap_or(0),
        };

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                validator_id.to_string(),
                (info.clone(), std::time::SystemTime::now()),
            );
        }

        let duration_ms = start.elapsed().as_millis() as f64;

        // Record metrics
        if let Some(metrics) = &self.metrics {
            // Record cache miss + successful query
            let mut labels = HashMap::new();
            labels.insert("result".to_string(), "cache_miss".to_string());
            let _ = metrics
                .record_counter(
                    "validator_registry_queries_total".to_string(),
                    1.0,
                    labels,
                    "Total validator registry queries".to_string(),
                )
                .await;

            // Record query duration
            let _ = metrics
                .observe_histogram(
                    "validator_registry_query_duration_ms".to_string(),
                    duration_ms,
                    HashMap::new(),
                    "Validator registry query duration in milliseconds".to_string(),
                )
                .await;
        }

        tracing::info!(
            "✅ Retrieved validator {} from chain: stake={}, active={} ({}ms)",
            validator_id,
            info.stake,
            info.is_active,
            duration_ms
        );

        Ok(info)
    }

    async fn get_active_validators(&self) -> Result<Vec<ValidatorInfo>> {
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let query = json!({
            "method": "validator.list_active",
            "params": {},
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
            .map_err(|e| Error::network(format!("Failed to query active validators: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Validator registry RPC error: status {}",
                response.status()
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse active validators: {}", e)))?;

        let validators = response_body
            .get("result")
            .and_then(|r| r.as_array())
            .ok_or_else(|| Error::network("Invalid response format for active validators"))?;

        let mut result = Vec::new();

        for v in validators {
            let public_key_hex = v
                .get("public_key")
                .and_then(|k| k.as_str())
                .ok_or_else(|| Error::validation("Missing public_key"))?;

            let public_key_bytes = hex::decode(public_key_hex)
                .map_err(|e| Error::validation(format!("Invalid public key hex: {}", e)))?;

            if public_key_bytes.len() != 32 {
                continue; // Skip invalid entries
            }

            let mut public_key = [0u8; 32];
            public_key.copy_from_slice(&public_key_bytes);

            result.push(ValidatorInfo {
                validator_id: v
                    .get("validator_id")
                    .and_then(|id| id.as_str())
                    .unwrap_or("")
                    .to_string(),
                public_key,
                stake: v.get("stake").and_then(|s| s.as_u64()).unwrap_or(0),
                is_active: true,
                region: v
                    .get("region")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string()),
                registered_at: v.get("registered_at").and_then(|t| t.as_u64()).unwrap_or(0),
            });
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_registry() {
        let registry = InMemoryValidatorRegistry::new();

        let info = ValidatorInfo {
            validator_id: "test-validator".to_string(),
            public_key: [1u8; 32],
            stake: 10000,
            is_active: true,
            region: Some("us-east".to_string()),
            registered_at: 1234567890,
        };

        registry.register_validator(info.clone()).await;

        let retrieved = registry.get_validator_info("test-validator").await.unwrap();
        assert_eq!(retrieved.validator_id, "test-validator");
        assert_eq!(retrieved.public_key, [1u8; 32]);
        assert_eq!(retrieved.stake, 10000);

        let pubkey = registry
            .get_validator_pubkey("test-validator")
            .await
            .unwrap();
        assert_eq!(pubkey, [1u8; 32]);

        let is_active = registry
            .is_validator_active("test-validator")
            .await
            .unwrap();
        assert!(is_active);
    }

    #[tokio::test]
    async fn test_validator_not_found() {
        let registry = InMemoryValidatorRegistry::new();

        let result = registry.get_validator_info("nonexistent").await;
        assert!(result.is_err());

        let is_active = registry.is_validator_active("nonexistent").await.unwrap();
        assert!(!is_active);
    }
}
