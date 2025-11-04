// TiKV backend for blockchain state storage
//
// This module provides distributed key-value storage for blockchain consensus
// state with strong consistency guarantees. TiKV offers ACID transactions and
// horizontal scalability for consensus data.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tikv_client::{Config, Key, RawClient, Value};
use tracing::{debug, error, info, warn};

use crate::error::{StorageError, StorageResult};

/// Configuration for TiKV storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiKVConfig {
    /// PD (Placement Driver) endpoints
    pub pd_endpoints: Vec<String>,
    /// Connection timeout
    pub connection_timeout_seconds: u64,
    /// Operation timeout
    pub operation_timeout_seconds: u64,
    /// Enable compression
    pub enable_compression: bool,
}

impl Default for TiKVConfig {
    fn default() -> Self {
        Self {
            pd_endpoints: vec!["127.0.0.1:2379".to_string()],
            connection_timeout_seconds: 10,
            operation_timeout_seconds: 30,
            enable_compression: true,
        }
    }
}

/// TiKV storage client for blockchain state
pub struct TiKVStorage {
    /// TiKV raw client
    client: RawClient,
    /// Configuration
    config: TiKVConfig,
}

/// Blockchain state data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainState {
    pub block_height: u64,
    pub state_root: Vec<u8>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub validator_set: Vec<String>,
    pub total_transactions: u64,
}

/// Block metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMetadata {
    pub height: u64,
    pub hash: Vec<u8>,
    pub previous_hash: Vec<u8>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub validator: String,
    pub transaction_count: u32,
}

impl TiKVStorage {
    /// Create new TiKV storage client
    pub async fn new(config: TiKVConfig) -> StorageResult<Self> {
        info!(
            "Connecting to TiKV cluster with {} PD endpoints",
            config.pd_endpoints.len()
        );

        // tikv-client 0.3 API: RawClient::new() takes Vec<String> directly
        let client = RawClient::new(config.pd_endpoints.clone())
            .await
            .map_err(|e| {
                error!("Failed to create TiKV client: {}", e);
                StorageError::TiKV(format!("Connection failed: {}", e))
            })?;

        info!("Successfully connected to TiKV cluster");
        Ok(Self { client, config })
    }

    /// Store chain state with strong consistency
    pub async fn store_chain_state(
        &self,
        block_height: u64,
        state: &ChainState,
    ) -> StorageResult<()> {
        let key = format!("chain:block:{}", block_height);
        let value =
            bincode::serialize(state).map_err(|e| StorageError::Serialization(e.to_string()))?;

        debug!("Storing chain state for block {}", block_height);

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client
                .put(Key::from(key.as_bytes().to_vec()), Value::from(value)),
        )
        .await;

        match result {
            Ok(Ok(_)) => {
                debug!("Successfully stored chain state for block {}", block_height);
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Failed to store chain state: {}", e);
                Err(StorageError::TiKV(format!("Put failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout storing chain state");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Get chain state with linearizable read
    pub async fn get_chain_state(&self, block_height: u64) -> StorageResult<Option<ChainState>> {
        let key = format!("chain:block:{}", block_height);

        debug!("Retrieving chain state for block {}", block_height);

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client.get(Key::from(key.as_bytes().to_vec())),
        )
        .await;

        match result {
            Ok(Ok(Some(value))) => {
                let state: ChainState = bincode::deserialize(&value)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Ok(Ok(None)) => Ok(None),
            Ok(Err(e)) => {
                error!("Failed to get chain state: {}", e);
                Err(StorageError::TiKV(format!("Get failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout getting chain state");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Store block metadata
    pub async fn store_block_metadata(&self, metadata: &BlockMetadata) -> StorageResult<()> {
        let key = format!("block:meta:{}", metadata.height);
        let value =
            bincode::serialize(metadata).map_err(|e| StorageError::Serialization(e.to_string()))?;

        debug!("Storing block metadata for height {}", metadata.height);

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client
                .put(Key::from(key.as_bytes().to_vec()), Value::from(value)),
        )
        .await;

        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => {
                error!("Failed to store block metadata: {}", e);
                Err(StorageError::TiKV(format!("Put failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout storing block metadata");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Get block metadata
    pub async fn get_block_metadata(&self, height: u64) -> StorageResult<Option<BlockMetadata>> {
        let key = format!("block:meta:{}", height);

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client.get(Key::from(key.as_bytes().to_vec())),
        )
        .await;

        match result {
            Ok(Ok(Some(value))) => {
                let metadata: BlockMetadata = bincode::deserialize(&value)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(metadata))
            }
            Ok(Ok(None)) => Ok(None),
            Ok(Err(e)) => {
                error!("Failed to get block metadata: {}", e);
                Err(StorageError::TiKV(format!("Get failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout getting block metadata");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Store validator state
    pub async fn store_validator_state(
        &self,
        validator_id: &str,
        state: &[u8],
    ) -> StorageResult<()> {
        let key = format!("validator:state:{}", validator_id);

        debug!("Storing validator state for {}", validator_id);

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client.put(
                Key::from(key.as_bytes().to_vec()),
                Value::from(state.to_vec()),
            ),
        )
        .await;

        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => {
                error!("Failed to store validator state: {}", e);
                Err(StorageError::TiKV(format!("Put failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout storing validator state");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Get validator state
    pub async fn get_validator_state(&self, validator_id: &str) -> StorageResult<Option<Vec<u8>>> {
        let key = format!("validator:state:{}", validator_id);

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client.get(Key::from(key.as_bytes().to_vec())),
        )
        .await;

        match result {
            Ok(Ok(value)) => Ok(value.map(|v| v.to_vec())),
            Ok(Err(e)) => {
                error!("Failed to get validator state: {}", e);
                Err(StorageError::TiKV(format!("Get failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout getting validator state");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Delete key
    pub async fn delete(&self, key: &str) -> StorageResult<()> {
        debug!("Deleting key: {}", key);

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client.delete(Key::from(key.as_bytes().to_vec())),
        )
        .await;

        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => {
                error!("Failed to delete key: {}", e);
                Err(StorageError::TiKV(format!("Delete failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout deleting key");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Batch get multiple keys
    pub async fn batch_get(&self, keys: Vec<String>) -> StorageResult<Vec<Option<Vec<u8>>>> {
        debug!("Batch getting {} keys", keys.len());

        let tikv_keys: Vec<Key> = keys
            .iter()
            .map(|k| Key::from(k.as_bytes().to_vec()))
            .collect();

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client.batch_get(tikv_keys),
        )
        .await;

        match result {
            Ok(Ok(kvpairs)) => {
                let values = kvpairs.into_iter().map(|kv| Some(kv.1.to_vec())).collect();
                Ok(values)
            }
            Ok(Err(e)) => {
                error!("Failed to batch get: {}", e);
                Err(StorageError::TiKV(format!("Batch get failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout in batch get");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Batch put multiple key-value pairs
    pub async fn batch_put(&self, kvpairs: Vec<(String, Vec<u8>)>) -> StorageResult<()> {
        debug!("Batch putting {} key-value pairs", kvpairs.len());

        let tikv_kvpairs: Vec<(Key, Value)> = kvpairs
            .into_iter()
            .map(|(k, v)| (Key::from(k.as_bytes().to_vec()), Value::from(v)))
            .collect();

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client.batch_put(tikv_kvpairs),
        )
        .await;

        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => {
                error!("Failed to batch put: {}", e);
                Err(StorageError::TiKV(format!("Batch put failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout in batch put");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Scan keys with prefix
    pub async fn scan_keys(&self, prefix: &str, limit: u32) -> StorageResult<Vec<String>> {
        debug!("Scanning keys with prefix: {}, limit: {}", prefix, limit);

        let start_key = Key::from(prefix.as_bytes().to_vec());
        // Create end key by incrementing last byte of prefix for range scan
        let mut end_bytes = prefix.as_bytes().to_vec();
        if let Some(last) = end_bytes.last_mut() {
            *last = last.saturating_add(1);
        }
        let end_key = Key::from(end_bytes);

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.operation_timeout_seconds),
            self.client.scan_keys(start_key..end_key, limit),
        )
        .await;

        match result {
            Ok(Ok(keys)) => {
                let string_keys = keys
                    .into_iter()
                    .map(|k| String::from_utf8_lossy((&k).into()).to_string())
                    .collect();
                Ok(string_keys)
            }
            Ok(Err(e)) => {
                error!("Failed to scan keys: {}", e);
                Err(StorageError::TiKV(format!("Scan failed: {}", e)))
            }
            Err(_) => {
                error!("Timeout scanning keys");
                Err(StorageError::Timeout)
            }
        }
    }

    /// Health check - test TiKV connectivity
    pub async fn health_check(&self) -> StorageResult<bool> {
        let test_key = format!("_health_check_{}", uuid::Uuid::new_v4());
        let test_value = b"health check";

        // Try to put
        let put_result = self
            .client
            .put(
                Key::from(test_key.as_bytes().to_vec()),
                Value::from(test_value.to_vec()),
            )
            .await;

        if put_result.is_err() {
            error!("TiKV health check failed: put error");
            return Ok(false);
        }

        // Try to get
        let get_result = self
            .client
            .get(Key::from(test_key.as_bytes().to_vec()))
            .await;

        let healthy = match get_result {
            Ok(Some(value)) => value == test_value,
            _ => false,
        };

        // Clean up
        let _ = self
            .client
            .delete(Key::from(test_key.as_bytes().to_vec()))
            .await;

        Ok(healthy)
    }
}

/// Helper functions for TiKV key generation
pub mod keys {
    /// Generate key for chain state
    pub fn chain_state_key(block_height: u64) -> String {
        format!("chain:block:{}", block_height)
    }

    /// Generate key for block metadata
    pub fn block_metadata_key(height: u64) -> String {
        format!("block:meta:{}", height)
    }

    /// Generate key for validator state
    pub fn validator_state_key(validator_id: &str) -> String {
        format!("validator:state:{}", validator_id)
    }

    /// Generate key for consensus round
    pub fn consensus_round_key(round: u64) -> String {
        format!("consensus:round:{}", round)
    }

    /// Generate key for transaction
    pub fn transaction_key(tx_hash: &str) -> String {
        format!("tx:{}", tx_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let chain_key = keys::chain_state_key(12345);
        assert_eq!(chain_key, "chain:block:12345");

        let block_key = keys::block_metadata_key(67890);
        assert_eq!(block_key, "block:meta:67890");

        let validator_key = keys::validator_state_key("validator1");
        assert_eq!(validator_key, "validator:state:validator1");

        let tx_key = keys::transaction_key("abc123");
        assert_eq!(tx_key, "tx:abc123");
    }

    #[test]
    fn test_default_config() {
        let config = TiKVConfig::default();
        assert_eq!(config.connection_timeout_seconds, 10);
        assert_eq!(config.operation_timeout_seconds, 30);
        assert!(config.enable_compression);
    }
}
