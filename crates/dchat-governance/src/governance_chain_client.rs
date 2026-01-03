//! HTTP JSON-RPC client for governance chain operations
//!
//! This module provides a concrete implementation of the `GovernanceChainClient` trait
//! that communicates with the chat chain via HTTP JSON-RPC for:
//! - Submitting governance transactions (proposals, votes, execution)
//! - Querying governance state (parameters, proposals, vote tallies)
//! - Waiting for transaction confirmations
//!
//! The client is designed to work with the dchat chat chain's governance module.

use crate::protocol_dao::{GovernanceChainClient, GovernanceEventLog, GovernanceTxReceipt};
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// HTTP JSON-RPC client for governance operations on the chat chain
#[derive(Clone)]
pub struct HttpGovernanceChainClient {
    rpc_url: String,
    client: Arc<reqwest::Client>,
    /// Number of confirmations to wait for by default
    default_confirmations: u32,
    /// Signing key for submitting transactions (hex encoded)
    signing_key: Option<String>,
}

impl HttpGovernanceChainClient {
    /// Create new HTTP governance chain client
    pub fn new(rpc_url: String) -> Self {
        Self {
            rpc_url,
            client: Arc::new(
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .expect("Failed to create HTTP client"),
            ),
            default_confirmations: 3,
            signing_key: None,
        }
    }

    /// Create client with signing key for transaction submission
    pub fn with_signing_key(mut self, signing_key: String) -> Self {
        self.signing_key = Some(signing_key);
        self
    }

    /// Set default confirmations to wait for
    pub fn with_confirmations(mut self, confirmations: u32) -> Self {
        self.default_confirmations = confirmations;
        self
    }

    /// Create from environment variables
    ///
    /// Reads:
    /// - `DCHAT_CHAT_CHAIN_RPC_URL` or `CHAT_CHAIN_RPC` - RPC endpoint
    /// - `DCHAT_GOVERNANCE_SIGNING_KEY` - Optional signing key for tx submission
    pub fn from_env() -> Result<Self> {
        let rpc_url = std::env::var("DCHAT_CHAT_CHAIN_RPC_URL")
            .ok()
            .and_then(|v| {
                let trimmed = v.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            })
            .or_else(|| {
                std::env::var("CHAT_CHAIN_RPC").ok().and_then(|v| {
                    let trimmed = v.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                })
            })
            .ok_or_else(|| {
                Error::Config(
                    "Chat chain RPC URL not configured. Set env `DCHAT_CHAT_CHAIN_RPC_URL`."
                        .to_string(),
                )
            })?;

        let mut client = Self::new(rpc_url);

        // Optionally load signing key
        if let Ok(key) = std::env::var("DCHAT_GOVERNANCE_SIGNING_KEY") {
            let trimmed = key.trim().to_string();
            if !trimmed.is_empty() {
                client = client.with_signing_key(trimmed);
            }
        }

        Ok(client)
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
            .map_err(|e| Error::network(format!("Governance RPC request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Governance RPC returned error status: {}",
                response.status()
            )));
        }

        let rpc_response: JsonRpcResponse<R> = response.json().await.map_err(|e| {
            Error::network(format!("Failed to parse governance RPC response: {}", e))
        })?;

        if let Some(error) = rpc_response.error {
            return Err(Error::network(format!(
                "Governance RPC error {}: {}",
                error.code, error.message
            )));
        }

        rpc_response
            .result
            .ok_or_else(|| Error::network("Governance RPC response missing result"))
    }

    /// Helper to convert RPC response to GovernanceTxReceipt
    fn parse_tx_receipt(raw: RawTxReceipt) -> GovernanceTxReceipt {
        GovernanceTxReceipt {
            tx_hash: raw.tx_hash,
            block_height: raw.block_height,
            block_hash: raw.block_hash,
            timestamp: raw.timestamp,
            success: raw.success,
            gas_used: raw.gas_used,
            logs: raw
                .logs
                .into_iter()
                .map(|log| GovernanceEventLog {
                    event_type: log.event_type,
                    data: log.data,
                })
                .collect(),
        }
    }
}

/// Raw transaction receipt from RPC
#[derive(Deserialize)]
struct RawTxReceipt {
    tx_hash: String,
    block_height: u64,
    block_hash: String,
    timestamp: i64,
    success: bool,
    gas_used: u64,
    logs: Vec<RawEventLog>,
}

#[derive(Deserialize)]
struct RawEventLog {
    event_type: String,
    data: HashMap<String, String>,
}

#[async_trait::async_trait]
impl GovernanceChainClient for HttpGovernanceChainClient {
    async fn submit_parameter_change(
        &self,
        parameter: &str,
        old_value: &str,
        new_value: &str,
        proposal_id: &str,
    ) -> Result<GovernanceTxReceipt> {
        #[derive(Serialize)]
        struct Params<'a> {
            parameter: &'a str,
            old_value: &'a str,
            new_value: &'a str,
            proposal_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            signing_key: Option<&'a str>,
        }

        let params = Params {
            parameter,
            old_value,
            new_value,
            proposal_id,
            signing_key: self.signing_key.as_deref(),
        };

        let raw: RawTxReceipt = self
            .call_rpc("governance.submit_parameter_change", params)
            .await?;

        Ok(Self::parse_tx_receipt(raw))
    }

    async fn submit_treasury_transfer(
        &self,
        recipient: &[u8],
        amount: u64,
        purpose: &str,
        proposal_id: &str,
    ) -> Result<GovernanceTxReceipt> {
        #[derive(Serialize)]
        struct Params<'a> {
            recipient: String,
            amount: u64,
            purpose: &'a str,
            proposal_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            signing_key: Option<&'a str>,
        }

        let params = Params {
            recipient: hex::encode(recipient),
            amount,
            purpose,
            proposal_id,
            signing_key: self.signing_key.as_deref(),
        };

        let raw: RawTxReceipt = self
            .call_rpc("governance.submit_treasury_transfer", params)
            .await?;

        Ok(Self::parse_tx_receipt(raw))
    }

    async fn submit_protocol_upgrade(
        &self,
        version: &str,
        upgrade_hash: &str,
        activation_block: u64,
        is_hard_fork: bool,
    ) -> Result<GovernanceTxReceipt> {
        #[derive(Serialize)]
        struct Params<'a> {
            version: &'a str,
            upgrade_hash: &'a str,
            activation_block: u64,
            is_hard_fork: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            signing_key: Option<&'a str>,
        }

        let params = Params {
            version,
            upgrade_hash,
            activation_block,
            is_hard_fork,
            signing_key: self.signing_key.as_deref(),
        };

        let raw: RawTxReceipt = self
            .call_rpc("governance.submit_protocol_upgrade", params)
            .await?;

        Ok(Self::parse_tx_receipt(raw))
    }

    async fn submit_emergency_action(
        &self,
        action_type: &str,
        parameters: &HashMap<String, String>,
    ) -> Result<GovernanceTxReceipt> {
        #[derive(Serialize)]
        struct Params<'a> {
            action_type: &'a str,
            parameters: &'a HashMap<String, String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            signing_key: Option<&'a str>,
        }

        let params = Params {
            action_type,
            parameters,
            signing_key: self.signing_key.as_deref(),
        };

        let raw: RawTxReceipt = self
            .call_rpc("governance.submit_emergency_action", params)
            .await?;

        Ok(Self::parse_tx_receipt(raw))
    }

    async fn submit_feature_toggle(
        &self,
        feature_name: &str,
        enable: bool,
    ) -> Result<GovernanceTxReceipt> {
        #[derive(Serialize)]
        struct Params<'a> {
            feature_name: &'a str,
            enable: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            signing_key: Option<&'a str>,
        }

        let params = Params {
            feature_name,
            enable,
            signing_key: self.signing_key.as_deref(),
        };

        let raw: RawTxReceipt = self
            .call_rpc("governance.submit_feature_toggle", params)
            .await?;

        Ok(Self::parse_tx_receipt(raw))
    }

    async fn get_protocol_parameter(&self, parameter: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Params<'a> {
            parameter: &'a str,
        }

        #[derive(Deserialize)]
        struct Response {
            value: String,
        }

        let params = Params { parameter };

        let response: Response = self
            .call_rpc("governance.get_protocol_parameter", params)
            .await?;

        Ok(response.value)
    }

    async fn get_current_block(&self) -> Result<u64> {
        #[derive(Deserialize)]
        struct Response {
            height: u64,
        }

        let response: Response = self.call_rpc("chain.get_current_block", ()).await?;

        Ok(response.height)
    }

    async fn wait_for_confirmation(&self, tx_hash: &str, confirmations: u32) -> Result<bool> {
        #[derive(Serialize)]
        struct Params<'a> {
            tx_hash: &'a str,
        }

        #[derive(Deserialize)]
        struct TxStatus {
            confirmed: bool,
            confirmations: u32,
            block_height: Option<u64>,
        }

        let params = Params { tx_hash };
        let target_confirmations = if confirmations == 0 {
            self.default_confirmations
        } else {
            confirmations
        };

        // Poll for confirmation with exponential backoff
        let max_attempts = 30;
        let mut attempt = 0;
        let mut delay = Duration::from_millis(500);

        while attempt < max_attempts {
            let status: TxStatus = self.call_rpc("chain.get_tx_status", &params).await?;

            if status.confirmed && status.confirmations >= target_confirmations {
                return Ok(true);
            }

            attempt += 1;
            tokio::time::sleep(delay).await;
            delay = std::cmp::min(delay * 2, Duration::from_secs(10));
        }

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = HttpGovernanceChainClient::new("http://localhost:8545".to_string());
        assert_eq!(client.default_confirmations, 3);
        assert!(client.signing_key.is_none());
    }

    #[test]
    fn test_client_builder_pattern() {
        let client = HttpGovernanceChainClient::new("http://localhost:8545".to_string())
            .with_signing_key("abc123".to_string())
            .with_confirmations(6);

        assert_eq!(client.default_confirmations, 6);
        assert_eq!(client.signing_key, Some("abc123".to_string()));
    }
}
