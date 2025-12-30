//! Blockchain staking verification trait for token-gated channel access
//!
//! Provides dependency injection interface for verifying stake commitments
//! against currency chain state. Enables production blockchain integration
//! while supporting test mocks.

use async_trait::async_trait;
use dchat_core::types::{ChannelId, UserId};
use dchat_core::Result;
use serde::{Deserialize, Serialize};

/// Status of user's stake commitment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StakeStatus {
    /// Stake is active and locked
    Active,
    /// Stake has been slashed due to misbehavior
    Slashed,
    /// Stake has been withdrawn (no longer valid)
    Withdrawn,
    /// No stake found for this user/channel
    NotFound,
}

/// Blockchain stake verification interface
///
/// Implementers query the currency chain for stake commitments
/// and lockup status. Used by ChannelAccessManager to enforce
/// stake-gated channel access policies.
#[async_trait]
pub trait StakingVerifier: Send + Sync {
    /// Verify user has committed sufficient stake for required duration
    ///
    /// # Arguments
    /// * `user_id` - User identity to verify
    /// * `minimum_stake` - Minimum token amount required
    /// * `duration_secs` - Minimum lockup duration in seconds
    ///
    /// # Returns
    /// `true` if user has active stake meeting requirements
    async fn verify_stake_commitment(
        &self,
        user_id: &UserId,
        minimum_stake: u64,
        duration_secs: u64,
    ) -> Result<bool>;

    /// Check current status of user's stake for a channel
    ///
    /// # Arguments
    /// * `user_id` - User identity to check
    /// * `channel_id` - Channel the stake is associated with
    ///
    /// # Returns
    /// Current stake status (Active, Slashed, Withdrawn, NotFound)
    async fn check_stake_status(
        &self,
        user_id: &UserId,
        channel_id: &ChannelId,
    ) -> Result<StakeStatus>;

    /// Query user's total staked amount across all channels
    async fn get_total_stake(&self, user_id: &UserId) -> Result<u64>;

    /// Query remaining lockup time for user's stake
    ///
    /// # Returns
    /// Seconds remaining before stake can be withdrawn, or 0 if unlocked
    async fn get_lockup_remaining(&self, user_id: &UserId, channel_id: &ChannelId) -> Result<u64>;
}

/// Mock implementation for testing and development
///
/// # Safety
/// This mock implementation is ONLY for testing and should NEVER be used in production.
/// It bypasses all blockchain verification and is feature-gated to test builds only.
#[cfg(any(test, feature = "test-mocks"))]
pub struct MockStakingVerifier {
    /// Hardcoded stakes for testing
    stakes: std::collections::HashMap<UserId, u64>,
}

#[cfg(any(test, feature = "test-mocks"))]
impl MockStakingVerifier {
    pub fn new() -> Self {
        #[cfg(not(any(test, debug_assertions, feature = "test-mocks")))]
        {
            panic!(
                "CRITICAL: MockStakingVerifier instantiated in production build! \
                This is a security vulnerability. Use ChainStakingVerifier instead."
            );
        }

        #[cfg(any(test, debug_assertions, feature = "test-mocks"))]
        tracing::warn!(
            "⚠️  MockStakingVerifier in use - FOR TESTING ONLY. \
            Do not deploy to production!"
        );

        Self {
            stakes: std::collections::HashMap::new(),
        }
    }

    pub fn add_stake(&mut self, user_id: UserId, amount: u64) {
        self.stakes.insert(user_id, amount);
    }
}

#[cfg(any(test, feature = "test-mocks"))]
impl Default for MockStakingVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-mocks"))]
#[async_trait]
impl StakingVerifier for MockStakingVerifier {
    async fn verify_stake_commitment(
        &self,
        user_id: &UserId,
        minimum_stake: u64,
        _duration_secs: u64,
    ) -> Result<bool> {
        let stake = self.stakes.get(user_id).copied().unwrap_or(0);
        Ok(stake >= minimum_stake)
    }

    async fn check_stake_status(
        &self,
        user_id: &UserId,
        _channel_id: &ChannelId,
    ) -> Result<StakeStatus> {
        if self.stakes.contains_key(user_id) {
            Ok(StakeStatus::Active)
        } else {
            Ok(StakeStatus::NotFound)
        }
    }

    async fn get_total_stake(&self, user_id: &UserId) -> Result<u64> {
        Ok(self.stakes.get(user_id).copied().unwrap_or(0))
    }

    async fn get_lockup_remaining(
        &self,
        _user_id: &UserId,
        _channel_id: &ChannelId,
    ) -> Result<u64> {
        Ok(0) // Mock always returns unlocked
    }
}

/// Production implementation using dchat-chain currency chain client
pub struct ChainStakingVerifier {
    /// RPC endpoint for currency chain (must be HTTPS in production)
    rpc_url: String,
    /// HTTP client for JSON-RPC calls
    client: reqwest::Client,
}

impl ChainStakingVerifier {
    /// Create a new ChainStakingVerifier with HTTPS enforcement
    ///
    /// # Security
    /// In production (non-debug) builds, the RPC URL MUST use HTTPS.
    /// This prevents man-in-the-middle attacks on stake verification,
    /// which could allow attackers to bypass stake requirements.
    ///
    /// # Errors
    /// Returns an error in production if the URL does not use HTTPS
    /// (exception: localhost URLs for development)
    pub fn new(rpc_url: String) -> Result<Self> {
        Self::validate_rpc_url(&rpc_url)?;

        Ok(Self {
            rpc_url,
            client: reqwest::Client::new(),
        })
    }

    /// Validate RPC URL for security requirements
    ///
    /// # Security Rules (Production)
    /// 1. HTTPS is required (prevents MITM attacks)
    /// 2. Exception: localhost/127.0.0.1 allowed for local development
    ///
    /// # Security Rules (Debug/Test)
    /// - All URLs allowed with warning for non-HTTPS
    fn validate_rpc_url(rpc_url: &str) -> Result<()> {
        let url = url::Url::parse(rpc_url)
            .map_err(|e| dchat_core::Error::validation(format!("Invalid RPC URL: {}", e)))?;

        let scheme = url.scheme();
        let host = url.host_str().unwrap_or("");

        // Check if this is a localhost URL (exempt from HTTPS in all builds)
        let is_localhost = host == "localhost"
            || host == "127.0.0.1"
            || host == "::1"
            || host.ends_with(".localhost");

        // In production builds, enforce HTTPS (except for localhost)
        #[cfg(not(debug_assertions))]
        {
            if scheme != "https" && !is_localhost {
                return Err(dchat_core::Error::validation(format!(
                    "SECURITY: RPC URL must use HTTPS in production. \
                        Got '{}' with scheme '{}'. \
                        Non-HTTPS connections allow MITM attacks on stake verification. \
                        Use 'https://{}' or connect to localhost for development.",
                    rpc_url, scheme, host
                )));
            }
        }

        // In debug builds, warn but allow non-HTTPS
        #[cfg(debug_assertions)]
        {
            if scheme != "https" && !is_localhost {
                tracing::warn!(
                    "⚠️  SECURITY WARNING: RPC URL '{}' does not use HTTPS. \
                    This would be rejected in production builds. \
                    Non-HTTPS connections are vulnerable to MITM attacks.",
                    rpc_url
                );
            }
        }

        // Additional validation: reject HTTP basic auth in URLs (credentials in URL)
        if url.username() != "" || url.password().is_some() {
            tracing::warn!(
                "SECURITY WARNING: RPC URL contains embedded credentials. \
                Consider using environment variables or secure credential storage."
            );
        }

        Ok(())
    }

    pub fn from_env() -> Result<Self> {
        fn allow_localhost_chain_rpc_defaults_from_env() -> bool {
            match std::env::var("DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS") {
                Ok(v) => {
                    let v = v.trim();
                    v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
                }
                Err(_) => false,
            }
        }

        let rpc_url = std::env::var("DCHAT_CURRENCY_CHAIN_RPC_URL")
            .or_else(|_| std::env::var("CURRENCY_CHAIN_RPC"))
            .ok()
            .or_else(|| {
                if allow_localhost_chain_rpc_defaults_from_env() {
                    tracing::warn!(
                        "Using localhost currency chain RPC default (DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1). This is unsafe for production."
                    );
                    Some("http://localhost:8545".to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                dchat_core::Error::Config(
                    "Currency chain RPC URL not configured. Set env `DCHAT_CURRENCY_CHAIN_RPC_URL` (preferred) or legacy `CURRENCY_CHAIN_RPC`.\n\
For local development only, you can opt into localhost defaults by setting `DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1`."
                        .to_string(),
                )
            })?;

        Self::new(rpc_url)
    }
}

#[async_trait]
impl StakingVerifier for ChainStakingVerifier {
    async fn verify_stake_commitment(
        &self,
        user_id: &UserId,
        minimum_stake: u64,
        duration_secs: u64,
    ) -> Result<bool> {
        use serde_json::json;

        let query = json!({
            "method": "currency.verify_stake_commitment",
            "params": {
                "user_id": user_id.to_string(),
                "minimum_stake": minimum_stake,
                "duration_secs": duration_secs,
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&query)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| dchat_core::Error::network(format!("RPC request failed: {}", e)))?;

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| dchat_core::Error::network(format!("Failed to parse response: {}", e)))?;

        let is_valid = response_body["result"]["is_valid"]
            .as_bool()
            .unwrap_or(false);

        Ok(is_valid)
    }

    async fn check_stake_status(
        &self,
        user_id: &UserId,
        channel_id: &ChannelId,
    ) -> Result<StakeStatus> {
        use serde_json::json;

        let query = json!({
            "method": "currency.query_stake_status",
            "params": {
                "user_id": user_id.to_string(),
                "channel_id": channel_id.to_string(),
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&query)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| dchat_core::Error::network(format!("RPC request failed: {}", e)))?;

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| dchat_core::Error::network(format!("Failed to parse response: {}", e)))?;

        let status_str = response_body["result"]["status"]
            .as_str()
            .unwrap_or("not_found");

        let status = match status_str {
            "active" => StakeStatus::Active,
            "slashed" => StakeStatus::Slashed,
            "withdrawn" => StakeStatus::Withdrawn,
            _ => StakeStatus::NotFound,
        };

        Ok(status)
    }

    async fn get_total_stake(&self, user_id: &UserId) -> Result<u64> {
        use serde_json::json;

        let query = json!({
            "method": "currency.query_total_stake",
            "params": {
                "user_id": user_id.to_string(),
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&query)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| dchat_core::Error::network(format!("RPC request failed: {}", e)))?;

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| dchat_core::Error::network(format!("Failed to parse response: {}", e)))?;

        let total = response_body["result"]["total_stake"].as_u64().unwrap_or(0);

        Ok(total)
    }

    async fn get_lockup_remaining(&self, user_id: &UserId, channel_id: &ChannelId) -> Result<u64> {
        use serde_json::json;

        let query = json!({
            "method": "currency.query_lockup_remaining",
            "params": {
                "user_id": user_id.to_string(),
                "channel_id": channel_id.to_string(),
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&query)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| dchat_core::Error::network(format!("RPC request failed: {}", e)))?;

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| dchat_core::Error::network(format!("Failed to parse response: {}", e)))?;

        let remaining = response_body["result"]["lockup_remaining_secs"]
            .as_u64()
            .unwrap_or(0);

        Ok(remaining)
    }
}
