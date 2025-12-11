//! Solana RPC Client
//!
//! Async JSON-RPC 2.0 client for Solana blockchain communication.

use dchat_core::error::{Error, Result};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{Cluster, LAMPORTS_PER_SOL};
use crate::wallet::solana_compat::SolanaAddress;

/// RPC client configuration
#[derive(Debug, Clone)]
pub struct SolanaRpcConfig {
    /// RPC endpoint URL
    pub rpc_url: String,
    /// WebSocket endpoint URL
    pub ws_url: Option<String>,
    /// Request timeout
    pub timeout: Duration,
    /// Max retries on failure
    pub max_retries: u32,
    /// Commitment level for queries
    pub commitment: Commitment,
}

impl SolanaRpcConfig {
    /// Create config for a cluster
    pub fn for_cluster(cluster: Cluster) -> Self {
        Self {
            rpc_url: cluster.rpc_url().to_string(),
            ws_url: Some(cluster.ws_url().to_string()),
            timeout: Duration::from_secs(30),
            max_retries: 3,
            commitment: Commitment::Confirmed,
        }
    }

    /// Create config for custom endpoint
    pub fn custom(rpc_url: String, ws_url: Option<String>) -> Self {
        Self {
            rpc_url,
            ws_url,
            timeout: Duration::from_secs(30),
            max_retries: 3,
            commitment: Commitment::Confirmed,
        }
    }
}

/// Transaction commitment level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Commitment {
    /// Query the most recent block confirmed by supermajority
    Finalized,
    /// Query the most recent block voted on by supermajority
    Confirmed,
    /// Query the most recent block
    Processed,
}

impl Commitment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Commitment::Finalized => "finalized",
            Commitment::Confirmed => "confirmed",
            Commitment::Processed => "processed",
        }
    }
}

/// RPC error types
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("RPC error {code}: {message}")]
    Rpc { code: i64, message: String },

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Timeout")]
    Timeout,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    jsonrpc: String,
    id: u64,
    result: Option<T>,
    #[serde(default)]
    error: Option<RpcErrorInfo>,
}

impl<T> RpcResponse<T> {
    /// Validate the JSON-RPC response format
    fn validate(&self, expected_id: u64) -> std::result::Result<(), RpcError> {
        if self.jsonrpc != "2.0" {
            return Err(RpcError::InvalidResponse(format!(
                "Invalid JSON-RPC version: expected '2.0', got '{}'",
                self.jsonrpc
            )));
        }
        if self.id != expected_id {
            return Err(RpcError::InvalidResponse(format!(
                "Response ID mismatch: expected {}, got {}",
                expected_id, self.id
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct RpcErrorInfo {
    code: i64,
    message: String,
}

/// Solana RPC client
pub struct SolanaRpcClient {
    config: SolanaRpcConfig,
    client: Client,
    request_id: AtomicU64,
}

impl SolanaRpcClient {
    /// Create a new RPC client
    ///
    /// In release builds, enforces HTTPS for non-localhost endpoints.
    pub fn new(config: SolanaRpcConfig) -> Result<Self> {
        // Enforce HTTPS in production (release builds with non-localhost URLs)
        #[cfg(not(debug_assertions))]
        {
            let is_local = config.rpc_url.starts_with("http://localhost")
                || config.rpc_url.starts_with("http://127.0.0.1");
            if !config.rpc_url.starts_with("https://") && !is_local {
                return Err(Error::validation(format!(
                    "HTTPS required for production Solana RPC endpoints. Got: {}. \
                     Use a localhost URL or configure HTTPS.",
                    config.rpc_url
                )));
            }
        }

        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            config,
            client,
            request_id: AtomicU64::new(1),
        })
    }

    /// Create client for a cluster
    pub fn for_cluster(cluster: Cluster) -> Result<Self> {
        Self::new(SolanaRpcConfig::for_cluster(cluster))
    }

    /// Get the next request ID
    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Make an RPC call
    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
    ) -> std::result::Result<T, RpcError> {
        let request_id = self.next_id();
        let request = RpcRequest {
            jsonrpc: "2.0",
            id: request_id,
            method: method.to_string(),
            params,
        };

        let response = self
            .client
            .post(&self.config.rpc_url)
            .json(&request)
            .send()
            .await?;

        let rpc_response: RpcResponse<T> = response.json().await?;

        // Validate JSON-RPC response format
        rpc_response.validate(request_id)?;

        if let Some(error) = rpc_response.error {
            return Err(RpcError::Rpc {
                code: error.code,
                message: error.message,
            });
        }

        rpc_response
            .result
            .ok_or_else(|| RpcError::InvalidResponse("Missing result in response".to_string()))
    }

    // ============ Account Methods ============

    /// Get account info
    pub async fn get_account_info(
        &self,
        address: &SolanaAddress,
    ) -> std::result::Result<Option<AccountInfoResponse>, RpcError> {
        let params = json!([
            address.to_base58(),
            {
                "encoding": "base64",
                "commitment": self.config.commitment.as_str()
            }
        ]);

        let response: GetAccountInfoResponse = self.call("getAccountInfo", params).await?;
        Ok(response.value)
    }

    /// Get multiple accounts
    pub async fn get_multiple_accounts(
        &self,
        addresses: &[SolanaAddress],
    ) -> std::result::Result<Vec<Option<AccountInfoResponse>>, RpcError> {
        let addresses: Vec<String> = addresses.iter().map(|a| a.to_base58()).collect();
        let params = json!([
            addresses,
            {
                "encoding": "base64",
                "commitment": self.config.commitment.as_str()
            }
        ]);

        let response: GetMultipleAccountsResponse =
            self.call("getMultipleAccounts", params).await?;
        Ok(response.value)
    }

    /// Get account balance in lamports
    pub async fn get_balance(&self, address: &SolanaAddress) -> std::result::Result<u64, RpcError> {
        let params = json!([
            address.to_base58(),
            { "commitment": self.config.commitment.as_str() }
        ]);

        let response: BalanceResponse = self.call("getBalance", params).await?;
        Ok(response.value)
    }

    /// Get account balance in SOL
    pub async fn get_balance_sol(
        &self,
        address: &SolanaAddress,
    ) -> std::result::Result<f64, RpcError> {
        let lamports = self.get_balance(address).await?;
        Ok(lamports as f64 / LAMPORTS_PER_SOL as f64)
    }

    // ============ Token Methods ============

    /// Get token account balance
    pub async fn get_token_account_balance(
        &self,
        token_account: &SolanaAddress,
    ) -> std::result::Result<TokenAmount, RpcError> {
        let params = json!([
            token_account.to_base58(),
            { "commitment": self.config.commitment.as_str() }
        ]);

        let response: TokenBalanceResponse = self.call("getTokenAccountBalance", params).await?;
        Ok(response.value)
    }

    /// Get all token accounts for an owner
    pub async fn get_token_accounts_by_owner(
        &self,
        owner: &SolanaAddress,
        mint: Option<&SolanaAddress>,
    ) -> std::result::Result<Vec<TokenAccountInfo>, RpcError> {
        let filter = if let Some(mint) = mint {
            json!({ "mint": mint.to_base58() })
        } else {
            json!({ "programId": super::TOKEN_PROGRAM_ID })
        };

        let params = json!([
            owner.to_base58(),
            filter,
            {
                "encoding": "jsonParsed",
                "commitment": self.config.commitment.as_str()
            }
        ]);

        let response: TokenAccountsResponse = self.call("getTokenAccountsByOwner", params).await?;
        Ok(response.value)
    }

    /// Get token supply
    pub async fn get_token_supply(
        &self,
        mint: &SolanaAddress,
    ) -> std::result::Result<TokenAmount, RpcError> {
        let params = json!([
            mint.to_base58(),
            { "commitment": self.config.commitment.as_str() }
        ]);

        let response: TokenSupplyResponse = self.call("getTokenSupply", params).await?;
        Ok(response.value)
    }

    // ============ Block & Slot Methods ============

    /// Get latest blockhash
    pub async fn get_latest_blockhash(&self) -> std::result::Result<BlockhashResponse, RpcError> {
        let params = json!([
            { "commitment": self.config.commitment.as_str() }
        ]);

        let response: GetLatestBlockhashResponse = self.call("getLatestBlockhash", params).await?;
        Ok(response.value)
    }

    /// Get current slot
    pub async fn get_slot(&self) -> std::result::Result<u64, RpcError> {
        let params = json!([
            { "commitment": self.config.commitment.as_str() }
        ]);

        self.call("getSlot", params).await
    }

    /// Get block height
    pub async fn get_block_height(&self) -> std::result::Result<u64, RpcError> {
        let params = json!([
            { "commitment": self.config.commitment.as_str() }
        ]);

        self.call("getBlockHeight", params).await
    }

    /// Get block time
    pub async fn get_block_time(&self, slot: u64) -> std::result::Result<Option<i64>, RpcError> {
        let params = json!([slot]);
        self.call("getBlockTime", params).await
    }

    // ============ Transaction Methods ============

    /// Send transaction (base64 encoded)
    pub async fn send_transaction(
        &self,
        transaction: &str,
    ) -> std::result::Result<String, RpcError> {
        let params = json!([
            transaction,
            {
                "encoding": "base64",
                "skipPreflight": false,
                "preflightCommitment": self.config.commitment.as_str(),
                "maxRetries": self.config.max_retries
            }
        ]);

        self.call("sendTransaction", params).await
    }

    /// Send raw transaction (bytes)
    pub async fn send_raw_transaction(
        &self,
        transaction_bytes: &[u8],
    ) -> std::result::Result<String, RpcError> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let encoded = STANDARD.encode(transaction_bytes);
        self.send_transaction(&encoded).await
    }

    /// Simulate transaction
    pub async fn simulate_transaction(
        &self,
        transaction: &str,
    ) -> std::result::Result<SimulateTransactionResponse, RpcError> {
        let params = json!([
            transaction,
            {
                "encoding": "base64",
                "commitment": self.config.commitment.as_str(),
                "sigVerify": true,
                "replaceRecentBlockhash": false
            }
        ]);

        let response: SimulateTransactionWrapper = self.call("simulateTransaction", params).await?;
        Ok(response.value)
    }

    /// Get transaction status
    pub async fn get_signature_statuses(
        &self,
        signatures: &[String],
    ) -> std::result::Result<Vec<Option<SignatureStatus>>, RpcError> {
        let params = json!([
            signatures,
            { "searchTransactionHistory": true }
        ]);

        let response: SignatureStatusesResponse = self.call("getSignatureStatuses", params).await?;
        Ok(response.value)
    }

    /// Get transaction details
    pub async fn get_transaction(
        &self,
        signature: &str,
    ) -> std::result::Result<Option<TransactionResponse>, RpcError> {
        let params = json!([
            signature,
            {
                "encoding": "jsonParsed",
                "commitment": self.config.commitment.as_str(),
                "maxSupportedTransactionVersion": 0
            }
        ]);

        self.call("getTransaction", params).await
    }

    /// Confirm transaction (wait for confirmation)
    pub async fn confirm_transaction(
        &self,
        signature: &str,
        timeout: Duration,
    ) -> std::result::Result<bool, RpcError> {
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            let statuses = self
                .get_signature_statuses(&[signature.to_string()])
                .await?;

            if let Some(Some(status)) = statuses.first() {
                if status.err.is_none() {
                    if let Some(confirmations) = status.confirmations {
                        if confirmations > 0
                            || status.confirmation_status == Some("finalized".to_string())
                        {
                            return Ok(true);
                        }
                    } else if status.confirmation_status == Some("finalized".to_string()) {
                        return Ok(true);
                    }
                } else {
                    return Err(RpcError::TransactionFailed(format!(
                        "Transaction failed: {:?}",
                        status.err
                    )));
                }
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Err(RpcError::Timeout)
    }

    // ============ Fee Methods ============

    /// Get minimum balance for rent exemption
    pub async fn get_minimum_balance_for_rent_exemption(
        &self,
        data_len: usize,
    ) -> std::result::Result<u64, RpcError> {
        let params = json!([data_len, { "commitment": self.config.commitment.as_str() }]);
        self.call("getMinimumBalanceForRentExemption", params).await
    }

    /// Get recent prioritization fees
    pub async fn get_recent_prioritization_fees(
        &self,
        addresses: &[SolanaAddress],
    ) -> std::result::Result<Vec<PrioritizationFee>, RpcError> {
        let addresses: Vec<String> = addresses.iter().map(|a| a.to_base58()).collect();
        let params = json!([addresses]);
        self.call("getRecentPrioritizationFees", params).await
    }

    // ============ Program Methods ============

    /// Get program accounts
    pub async fn get_program_accounts(
        &self,
        program_id: &SolanaAddress,
        filters: Option<Vec<AccountFilter>>,
    ) -> std::result::Result<Vec<ProgramAccount>, RpcError> {
        let mut config = json!({
            "encoding": "base64",
            "commitment": self.config.commitment.as_str()
        });

        if let Some(filters) = filters {
            config["filters"] = serde_json::to_value(filters).unwrap();
        }

        let params = json!([program_id.to_base58(), config]);

        self.call("getProgramAccounts", params).await
    }

    // ============ Health & Info ============

    /// Get cluster health
    pub async fn get_health(&self) -> std::result::Result<String, RpcError> {
        self.call("getHealth", json!([])).await
    }

    /// Get version
    pub async fn get_version(&self) -> std::result::Result<VersionInfo, RpcError> {
        self.call("getVersion", json!([])).await
    }

    /// Get cluster nodes
    pub async fn get_cluster_nodes(&self) -> std::result::Result<Vec<ClusterNode>, RpcError> {
        self.call("getClusterNodes", json!([])).await
    }

    /// Request airdrop (devnet/testnet only)
    pub async fn request_airdrop(
        &self,
        address: &SolanaAddress,
        lamports: u64,
    ) -> std::result::Result<String, RpcError> {
        let params = json!([
            address.to_base58(),
            lamports,
            { "commitment": self.config.commitment.as_str() }
        ]);

        self.call("requestAirdrop", params).await
    }
}

// ============ Response Types ============

#[derive(Debug, Deserialize)]
pub struct GetAccountInfoResponse {
    pub value: Option<AccountInfoResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountInfoResponse {
    pub lamports: u64,
    pub owner: String,
    pub data: AccountData,
    pub executable: bool,
    #[serde(rename = "rentEpoch")]
    pub rent_epoch: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AccountData {
    Base64(Vec<String>),
    Json { program: String, parsed: Value },
}

impl AccountData {
    pub fn as_bytes(&self) -> Option<Vec<u8>> {
        match self {
            AccountData::Base64(data) => {
                if let Some(encoded) = data.first() {
                    use base64::{engine::general_purpose::STANDARD, Engine};
                    STANDARD.decode(encoded).ok()
                } else {
                    None
                }
            }
            AccountData::Json { .. } => None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct GetMultipleAccountsResponse {
    pub value: Vec<Option<AccountInfoResponse>>,
}

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    value: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenAmount {
    pub amount: String,
    pub decimals: u8,
    #[serde(rename = "uiAmount")]
    pub ui_amount: Option<f64>,
    #[serde(rename = "uiAmountString")]
    pub ui_amount_string: Option<String>,
}

impl TokenAmount {
    pub fn as_u64(&self) -> u64 {
        self.amount.parse().unwrap_or(0)
    }
}

#[derive(Debug, Deserialize)]
struct TokenBalanceResponse {
    value: TokenAmount,
}

#[derive(Debug, Deserialize)]
struct TokenSupplyResponse {
    value: TokenAmount,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenAccountInfo {
    pub pubkey: String,
    pub account: TokenAccountData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenAccountData {
    pub data: TokenAccountParsedData,
    pub lamports: u64,
    pub owner: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenAccountParsedData {
    pub parsed: TokenAccountParsedInfo,
    pub program: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenAccountParsedInfo {
    pub info: TokenAccountDetails,
    #[serde(rename = "type")]
    pub account_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenAccountDetails {
    pub mint: String,
    pub owner: String,
    #[serde(rename = "tokenAmount")]
    pub token_amount: TokenAmount,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct TokenAccountsResponse {
    value: Vec<TokenAccountInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockhashResponse {
    pub blockhash: String,
    #[serde(rename = "lastValidBlockHeight")]
    pub last_valid_block_height: u64,
}

#[derive(Debug, Deserialize)]
struct GetLatestBlockhashResponse {
    value: BlockhashResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignatureStatus {
    pub slot: u64,
    pub confirmations: Option<u64>,
    pub err: Option<Value>,
    #[serde(rename = "confirmationStatus")]
    pub confirmation_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SignatureStatusesResponse {
    value: Vec<Option<SignatureStatus>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionResponse {
    pub slot: u64,
    pub transaction: Value,
    pub meta: Option<TransactionMeta>,
    #[serde(rename = "blockTime")]
    pub block_time: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionMeta {
    pub err: Option<Value>,
    pub fee: u64,
    #[serde(rename = "preBalances")]
    pub pre_balances: Vec<u64>,
    #[serde(rename = "postBalances")]
    pub post_balances: Vec<u64>,
    #[serde(rename = "logMessages")]
    pub log_messages: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulateTransactionResponse {
    pub err: Option<Value>,
    pub logs: Option<Vec<String>>,
    #[serde(rename = "unitsConsumed")]
    pub units_consumed: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SimulateTransactionWrapper {
    value: SimulateTransactionResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrioritizationFee {
    pub slot: u64,
    #[serde(rename = "prioritizationFee")]
    pub prioritization_fee: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountFilter {
    DataSize(u64),
    Memcmp { offset: usize, bytes: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProgramAccount {
    pub pubkey: String,
    pub account: AccountInfoResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    #[serde(rename = "solana-core")]
    pub solana_core: String,
    #[serde(rename = "feature-set")]
    pub feature_set: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClusterNode {
    pub pubkey: String,
    #[serde(rename = "featureSet")]
    pub feature_set: Option<u64>,
    pub gossip: Option<String>,
    pub rpc: Option<String>,
    #[serde(rename = "shredVersion")]
    pub shred_version: Option<u16>,
    pub tpu: Option<String>,
    pub version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = SolanaRpcConfig::for_cluster(Cluster::Devnet);
        assert_eq!(config.rpc_url, "https://api.devnet.solana.com");
    }

    #[test]
    fn test_commitment_serialization() {
        assert_eq!(Commitment::Finalized.as_str(), "finalized");
        assert_eq!(Commitment::Confirmed.as_str(), "confirmed");
        assert_eq!(Commitment::Processed.as_str(), "processed");
    }
}
