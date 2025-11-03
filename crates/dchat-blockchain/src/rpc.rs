//! RPC client for blockchain communication

use dchat_core::error::{Result, Error};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// RPC client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    /// RPC endpoint URL
    pub url: String,
    /// Request timeout (seconds)
    pub timeout: u64,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8545".to_string(),
            timeout: 30,
        }
    }
}

/// RPC client for blockchain node communication
pub struct RpcClient {
    config: RpcConfig,
    client: reqwest::Client,
}

impl RpcClient {
    /// Create a new RPC client
    pub fn new(config: RpcConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout))
            .build()
            .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;
        
        Ok(Self { config, client })
    }

    /// Submit raw transaction
    pub async fn submit_transaction(&self, tx_data: Vec<u8>) -> Result<String> {
        let tx_hex = hex::encode(&tx_data);
        
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_sendRawTransaction",
            "params": [format!("0x{}", tx_hex)],
            "id": 1
        });
        
        let response = self.client
            .post(&self.config.url)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;
        
        if !response.status().is_success() {
            return Err(Error::network(format!("RPC returned status {}", response.status())));
        }
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;
        
        if let Some(error) = json.get("error") {
            return Err(Error::network(format!("RPC error: {}", error)));
        }
        
        json.get("result")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::network("Missing result in RPC response".to_string()))
    }

    /// Query transaction receipt
    pub async fn get_transaction_receipt(&self, tx_hash: &str) -> Result<Option<serde_json::Value>> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash],
            "id": 1
        });
        
        let response = self.client
            .post(&self.config.url)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;
        
        Ok(json.get("result").cloned())
    }

    /// Get current block number
    pub async fn get_block_number(&self) -> Result<u64> {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });
        
        let response = self.client
            .post(&self.config.url)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;
        
        let json: serde_json::Value = response.json().await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;
        
        let hex_str = json.get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::network("Missing result in RPC response".to_string()))?;
        
        let hex_str = hex_str.trim_start_matches("0x");
        u64::from_str_radix(hex_str, 16)
            .map_err(|e| Error::network(format!("Invalid block number: {}", e)))
    }
}
