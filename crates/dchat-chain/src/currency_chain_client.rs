//! Currency chain HTTP client for slashing and staking operations
//!
//! This module provides an HTTP JSON-RPC client implementation for interacting
//! with the currency chain for validator slashing, staking queries, and reward distribution.

use crate::dispute_resolution::CurrencyChainClient;
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// HTTP JSON-RPC client for currency chain operations
#[derive(Clone)]
pub struct HttpCurrencyChainClient {
    rpc_url: String,
    client: Arc<reqwest::Client>,
}

impl HttpCurrencyChainClient {
    /// Create new HTTP currency chain client
    pub fn new(rpc_url: String) -> Self {
        Self {
            rpc_url,
            client: Arc::new(reqwest::Client::new()),
        }
    }

    /// Create from environment variable.
    ///
    /// Resolution order:
    /// - `DCHAT_CURRENCY_CHAIN_RPC_URL`
    /// - `CURRENCY_CHAIN_RPC` (legacy)
    pub fn from_env() -> Result<Self> {
        let rpc_url = std::env::var("DCHAT_CURRENCY_CHAIN_RPC_URL")
            .ok()
            .and_then(|v| {
                let trimmed = v.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            })
            .or_else(|| {
                std::env::var("CURRENCY_CHAIN_RPC").ok().and_then(|v| {
                    let trimmed = v.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                })
            })
            .ok_or_else(|| {
                Error::Config(
                    "Currency chain RPC URL not configured. Set env `DCHAT_CURRENCY_CHAIN_RPC_URL` (preferred) or `CURRENCY_CHAIN_RPC`.".to_string(),
                )
            })?;

        Ok(Self::new(rpc_url))
    }

    /// Make JSON-RPC call
    async fn call_rpc<P: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R> {
        #[derive(Serialize)]
        struct JsonRpcRequest<T> {
            jsonrpc: &'static str,
            id: u64,
            method: String,
            params: T,
        }

        #[derive(Deserialize)]
        struct JsonRpcResponse<T> {
            result: Option<T>,
            error: Option<JsonRpcError>,
        }

        #[derive(Deserialize)]
        struct JsonRpcError {
            code: i32,
            message: String,
        }

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: method.to_string(),
            params,
        };

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "RPC returned error status: {}",
                response.status()
            )));
        }

        let rpc_response: JsonRpcResponse<R> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse RPC response: {}", e)))?;

        if let Some(error) = rpc_response.error {
            return Err(Error::network(format!(
                "RPC error {}: {}",
                error.code, error.message
            )));
        }

        rpc_response
            .result
            .ok_or_else(|| Error::network("RPC response missing result"))
    }
}

#[async_trait::async_trait]
impl CurrencyChainClient for HttpCurrencyChainClient {
    async fn get_validator_stake(&self, validator_key: &[u8]) -> Result<u64> {
        #[derive(Serialize)]
        struct Params {
            validator_key: String,
        }

        let params = Params {
            validator_key: hex::encode(validator_key),
        };

        let stake: u64 = self
            .call_rpc("currency.query_validator_stake", params)
            .await?;

        Ok(stake)
    }

    async fn execute_slash(
        &self,
        validator_key: &[u8],
        slash_amount: u64,
        reason: &str,
    ) -> Result<String> {
        #[derive(Serialize)]
        struct Params {
            validator_key: String,
            slash_amount: u64,
            reason: String,
        }

        #[derive(Deserialize)]
        struct SlashResponse {
            transaction_id: String,
            slashed_amount: u64,
            remaining_stake: u64,
        }

        let params = Params {
            validator_key: hex::encode(validator_key),
            slash_amount,
            reason: reason.to_string(),
        };

        let response: SlashResponse = self
            .call_rpc("currency.slash_validator_stake", params)
            .await?;

        tracing::info!(
            "Slash executed: tx={}, slashed={}, remaining={}",
            response.transaction_id,
            response.slashed_amount,
            response.remaining_stake
        );

        Ok(response.transaction_id)
    }

    async fn transfer_reward(&self, recipient_key: &[u8], amount: u64) -> Result<String> {
        #[derive(Serialize)]
        struct Params {
            recipient_key: String,
            amount: u64,
            memo: String,
        }

        #[derive(Deserialize)]
        struct TransferResponse {
            transaction_id: String,
            transferred_amount: u64,
        }

        let params = Params {
            recipient_key: hex::encode(recipient_key),
            amount,
            memo: "Dispute resolution reward".to_string(),
        };

        let response: TransferResponse = self.call_rpc("currency.transfer_reward", params).await?;

        tracing::info!(
            "Reward transferred: tx={}, amount={}",
            response.transaction_id,
            response.transferred_amount
        );

        Ok(response.transaction_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires running currency chain
    async fn test_get_validator_stake() {
        let client = HttpCurrencyChainClient::from_env().expect("client from env");
        let validator_key = b"test_validator_key";

        let result = client.get_validator_stake(validator_key).await;

        // Should either succeed or fail with network error (not panic)
        match result {
            Ok(stake) => println!("Stake: {}", stake),
            Err(e) => println!("Expected error: {:?}", e),
        }
    }

    #[tokio::test]
    #[ignore] // Requires running currency chain
    async fn test_execute_slash() {
        let client = HttpCurrencyChainClient::from_env().expect("client from env");
        let validator_key = b"test_validator_key";

        let result = client
            .execute_slash(validator_key, 1000, "Test slash")
            .await;

        match result {
            Ok(tx_id) => println!("Slash TX: {}", tx_id),
            Err(e) => println!("Expected error: {:?}", e),
        }
    }

    #[test]
    fn test_client_creation() {
        let client = HttpCurrencyChainClient::new("http://localhost:8545".to_string());
        assert_eq!(client.rpc_url, "http://localhost:8545");
    }

    #[test]
    fn test_from_env() {
        std::env::set_var("DCHAT_CURRENCY_CHAIN_RPC_URL", "http://localhost:8545");
        let _client = HttpCurrencyChainClient::from_env().expect("client from env");
    }
}
