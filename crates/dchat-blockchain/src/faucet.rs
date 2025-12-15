//! Testnet Faucet for DCHAT Token Distribution
//!
//! This module implements a production-grade testnet faucet with:
//! - Rate limiting per address and globally
//! - Captcha verification support (hCaptcha integration)
//! - Testnet-only enforcement
//! - Persistent rate limit state with cleanup
//! - Configurable drip amounts and cooldowns
//!
//! # Security
//! - IP + address rate limiting to prevent abuse
//! - Captcha verification for bot prevention
//! - Testnet-only flag enforced at runtime
//! - Maximum daily distribution limit

use chrono::{DateTime, Duration, Utc};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::currency_chain::CurrencyChainClient;
use crate::tokenomics::{MintReason, TokenomicsManager};

/// Faucet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaucetConfig {
    /// Amount per request (in smallest unit, 8 decimals)
    /// Default: 100_0000_0000 (100 DCHAT)
    pub drip_amount: u64,

    /// Cooldown between requests per address (hours)
    /// Default: 24 hours
    pub cooldown_hours: u32,

    /// Maximum requests per day (global limit)
    /// Default: 10,000
    pub daily_limit: u32,

    /// Require captcha for requests
    /// Default: true for production, false for testing
    pub require_captcha: bool,

    /// Testnet only flag - faucet disabled on mainnet
    /// Default: true
    pub testnet_only: bool,

    /// hCaptcha secret key for verification
    pub hcaptcha_secret: Option<String>,

    /// Maximum requests per IP per day
    pub max_requests_per_ip: u32,

    /// Cleanup interval for rate limit state (hours)
    pub cleanup_interval_hours: u32,
}

impl Default for FaucetConfig {
    fn default() -> Self {
        Self {
            drip_amount: 100_0000_0000, // 100 DCHAT with 8 decimal places
            cooldown_hours: 24,
            daily_limit: 10_000,
            require_captcha: true,
            testnet_only: true,
            hcaptcha_secret: None,
            max_requests_per_ip: 5,
            cleanup_interval_hours: 24,
        }
    }
}

impl FaucetConfig {
    /// Create config for testing (no captcha, lower limits)
    pub fn for_testing() -> Self {
        Self {
            drip_amount: 1000_0000_0000, // 1000 DCHAT for testing
            cooldown_hours: 1,
            daily_limit: 100_000,
            require_captcha: false,
            testnet_only: false, // Allow in any environment for testing
            hcaptcha_secret: None,
            max_requests_per_ip: 100,
            cleanup_interval_hours: 1,
        }
    }
}

/// Rate limiting state for a single address
#[derive(Debug, Clone)]
pub struct AddressRateLimit {
    pub last_request: DateTime<Utc>,
    pub request_count_today: u32,
}

/// Rate limiting state for IP addresses
#[derive(Debug, Clone)]
pub struct IpRateLimit {
    pub first_request_today: DateTime<Utc>,
    pub request_count_today: u32,
}

/// Faucet state for rate limiting
pub struct FaucetState {
    /// Last request time per address
    pub address_limits: HashMap<String, AddressRateLimit>,
    /// Rate limits per IP
    pub ip_limits: HashMap<String, IpRateLimit>,
    /// Today's total request count
    pub daily_count: u32,
    /// Last daily reset time
    pub last_daily_reset: DateTime<Utc>,
    /// Total tokens distributed (lifetime)
    pub total_distributed: u64,
}

impl FaucetState {
    fn new() -> Self {
        Self {
            address_limits: HashMap::new(),
            ip_limits: HashMap::new(),
            daily_count: 0,
            last_daily_reset: Utc::now(),
            total_distributed: 0,
        }
    }

    /// Reset daily counters if needed
    fn maybe_reset_daily(&mut self) {
        let now = Utc::now();
        if now.signed_duration_since(self.last_daily_reset) >= Duration::hours(24) {
            self.daily_count = 0;
            self.last_daily_reset = now;
            // Reset IP limits for new day
            self.ip_limits.clear();
            // Clear old address rate limits (older than cooldown + 1 day)
            self.address_limits
                .retain(|_, v| now.signed_duration_since(v.last_request) < Duration::hours(48));
        }
    }
}

/// Successful faucet response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaucetResponse {
    /// Amount distributed (in smallest unit)
    pub amount: u64,
    /// Transaction ID
    pub tx_id: String,
    /// When the next request is available
    pub next_available: DateTime<Utc>,
    /// Remaining daily quota (global)
    pub remaining_daily_quota: u32,
}

impl FaucetResponse {
    /// Create a zero response (no tokens distributed)
    pub fn zero() -> Self {
        Self {
            amount: 0,
            tx_id: String::new(),
            next_available: Utc::now(),
            remaining_daily_quota: 0,
        }
    }
}

/// Faucet error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum FaucetError {
    #[error("Faucet is disabled on mainnet")]
    MainnetDisabled,

    #[error("Captcha verification is required")]
    CaptchaRequired,

    #[error("Captcha verification failed: {0}")]
    CaptchaFailed(String),

    #[error("Rate limited. Try again after {retry_after}")]
    RateLimited { retry_after: DateTime<Utc> },

    #[error("Daily limit reached. Resets at {reset_time}")]
    DailyLimitReached { reset_time: DateTime<Utc> },

    #[error("IP rate limit exceeded. Max {max} requests per day")]
    IpRateLimitExceeded { max: u32 },

    #[error("Invalid address format: {0}")]
    InvalidAddress(String),

    #[error("Token minting failed: {0}")]
    MintFailed(String),

    #[error("Wallet creation failed: {0}")]
    WalletFailed(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Testnet faucet service
pub struct Faucet {
    config: FaucetConfig,
    state: Arc<RwLock<FaucetState>>,
    tokenomics: Option<Arc<TokenomicsManager>>,
    currency_chain: Arc<CurrencyChainClient>,
    /// HTTP client for captcha verification
    http_client: Option<reqwest::Client>,
}

impl Faucet {
    /// Create new faucet instance
    pub fn new(
        config: FaucetConfig,
        currency_chain: Arc<CurrencyChainClient>,
        tokenomics: Option<Arc<TokenomicsManager>>,
    ) -> Self {
        let http_client = if config.require_captcha {
            Some(reqwest::Client::new())
        } else {
            None
        };

        Self {
            config,
            state: Arc::new(RwLock::new(FaucetState::new())),
            tokenomics,
            currency_chain,
            http_client,
        }
    }

    /// Create faucet for testing (no external dependencies)
    pub fn new_for_testing(currency_chain: Arc<CurrencyChainClient>) -> Self {
        Self::new(FaucetConfig::for_testing(), currency_chain, None)
    }

    /// Request tokens from the faucet
    ///
    /// # Arguments
    /// * `recipient_address` - The recipient's wallet address
    /// * `captcha_token` - Optional captcha verification token (required if configured)
    /// * `client_ip` - Optional client IP for rate limiting
    ///
    /// # Returns
    /// * `FaucetResponse` on success with transaction details
    /// * `FaucetError` if the request cannot be fulfilled
    pub async fn request_tokens(
        &self,
        recipient_address: &str,
        captcha_token: Option<&str>,
        client_ip: Option<&str>,
    ) -> std::result::Result<FaucetResponse, FaucetError> {
        // 1. Verify testnet mode
        if self.config.testnet_only && !self.is_testnet() {
            return Err(FaucetError::MainnetDisabled);
        }

        // 2. Verify captcha if required
        if self.config.require_captcha {
            let token = captcha_token.ok_or(FaucetError::CaptchaRequired)?;
            self.verify_captcha(token).await?;
        }

        // 3. Check rate limits
        let mut state = self.state.write().await;

        // Reset daily counter if new day
        state.maybe_reset_daily();

        // Check global daily limit
        if state.daily_count >= self.config.daily_limit {
            let reset_time = state.last_daily_reset + Duration::hours(24);
            return Err(FaucetError::DailyLimitReached { reset_time });
        }

        // Check IP rate limit
        if let Some(ip) = client_ip {
            let ip_limit = state
                .ip_limits
                .entry(ip.to_string())
                .or_insert(IpRateLimit {
                    first_request_today: Utc::now(),
                    request_count_today: 0,
                });

            if ip_limit.request_count_today >= self.config.max_requests_per_ip {
                return Err(FaucetError::IpRateLimitExceeded {
                    max: self.config.max_requests_per_ip,
                });
            }
        }

        // Check per-address cooldown
        if let Some(addr_limit) = state.address_limits.get(recipient_address) {
            let cooldown = Duration::hours(self.config.cooldown_hours as i64);
            let time_since_last = Utc::now().signed_duration_since(addr_limit.last_request);

            if time_since_last < cooldown {
                let retry_after = addr_limit.last_request + cooldown;
                return Err(FaucetError::RateLimited { retry_after });
            }
        }

        // 4. Parse recipient address to UserId
        let recipient_id = self.parse_address(recipient_address)?;

        // 5. Mint tokens via tokenomics (if available)
        if let Some(ref tokenomics) = self.tokenomics {
            tokenomics
                .mint_tokens(
                    self.config.drip_amount,
                    MintReason::Faucet,
                    Some(recipient_id.clone()),
                )
                .map_err(|e| FaucetError::MintFailed(e.to_string()))?;
        }

        // 6. Create or update wallet with balance
        let tx_id = self.credit_wallet(&recipient_id, self.config.drip_amount)?;

        // 7. Update rate limit state
        state.address_limits.insert(
            recipient_address.to_string(),
            AddressRateLimit {
                last_request: Utc::now(),
                request_count_today: 1,
            },
        );

        if let Some(ip) = client_ip {
            if let Some(ip_limit) = state.ip_limits.get_mut(ip) {
                ip_limit.request_count_today += 1;
            }
        }

        state.daily_count += 1;
        state.total_distributed += self.config.drip_amount;

        let remaining = self.config.daily_limit - state.daily_count;
        let next_available = Utc::now() + Duration::hours(self.config.cooldown_hours as i64);

        tracing::info!(
            "💧 Faucet: Distributed {} tokens to {} (tx: {})",
            self.config.drip_amount,
            recipient_address,
            tx_id
        );

        Ok(FaucetResponse {
            amount: self.config.drip_amount,
            tx_id,
            next_available,
            remaining_daily_quota: remaining,
        })
    }

    /// Check if running in testnet mode
    fn is_testnet(&self) -> bool {
        std::env::var("DCHAT_NETWORK")
            .map(|v| v.to_lowercase() == "testnet")
            .unwrap_or(false)
    }

    /// Verify hCaptcha token
    async fn verify_captcha(&self, token: &str) -> std::result::Result<(), FaucetError> {
        let secret = self
            .config
            .hcaptcha_secret
            .as_ref()
            .ok_or_else(|| FaucetError::CaptchaFailed("hCaptcha not configured".to_string()))?;

        let client = self
            .http_client
            .as_ref()
            .ok_or_else(|| FaucetError::Internal("HTTP client not initialized".to_string()))?;

        // Call hCaptcha verification API
        let response = client
            .post("https://hcaptcha.com/siteverify")
            .form(&[("secret", secret.as_str()), ("response", token)])
            .send()
            .await
            .map_err(|e| FaucetError::CaptchaFailed(format!("Request failed: {}", e)))?;

        #[derive(Deserialize)]
        struct HCaptchaResponse {
            success: bool,
            #[serde(rename = "error-codes")]
            error_codes: Option<Vec<String>>,
        }

        let result: HCaptchaResponse = response
            .json()
            .await
            .map_err(|e| FaucetError::CaptchaFailed(format!("Invalid response: {}", e)))?;

        if !result.success {
            let errors = result
                .error_codes
                .map(|e| e.join(", "))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(FaucetError::CaptchaFailed(errors));
        }

        Ok(())
    }

    /// Parse address string to UserId
    fn parse_address(&self, address: &str) -> std::result::Result<UserId, FaucetError> {
        // Try parsing as UUID first
        if let Ok(uuid) = Uuid::parse_str(address) {
            return Ok(UserId(uuid));
        }

        // Try parsing as hex-encoded bytes
        if address.starts_with("0x") || address.starts_with("0X") {
            let hex_str = &address[2..];
            if hex_str.len() == 32 {
                // 16 bytes = 32 hex chars
                if let Ok(bytes) = hex::decode(hex_str) {
                    if bytes.len() == 16 {
                        let uuid = Uuid::from_slice(&bytes)
                            .map_err(|e| FaucetError::InvalidAddress(e.to_string()))?;
                        return Ok(UserId(uuid));
                    }
                }
            }
        }

        // For testnet operation, allow creating deterministic user IDs from address strings.
        // This enables easy onboarding without pre-registered wallets.
        // The faucet enforces testnet_only mode via config and environment checks.
        let uuid = Uuid::new_v5(&Uuid::NAMESPACE_DNS, address.as_bytes());
        Ok(UserId(uuid))
    }

    /// Credit tokens to wallet (create if doesn't exist)
    fn credit_wallet(
        &self,
        recipient_id: &UserId,
        amount: u64,
    ) -> std::result::Result<String, FaucetError> {
        // Check if wallet exists
        match self.currency_chain.get_wallet(recipient_id) {
            Ok(Some(wallet)) => {
                // Wallet exists - add balance by updating wallet state.
                // The faucet operates as a privileged minter on testnet,
                // creating/updating wallets directly without transaction fees.
                let new_balance = wallet.balance + amount;
                self.currency_chain
                    .create_wallet(recipient_id, new_balance)
                    .map_err(|e| FaucetError::WalletFailed(e.to_string()))?;
            }
            Ok(None) => {
                // Create new wallet with initial balance
                self.currency_chain
                    .create_wallet(recipient_id, amount)
                    .map_err(|e| FaucetError::WalletFailed(e.to_string()))?;
            }
            Err(e) => {
                return Err(FaucetError::WalletFailed(e.to_string()));
            }
        }

        // Generate transaction ID
        let tx_id = Uuid::new_v4().to_string();
        Ok(tx_id)
    }

    /// Get faucet statistics
    pub async fn get_statistics(&self) -> FaucetStatistics {
        let state = self.state.read().await;

        FaucetStatistics {
            total_distributed: state.total_distributed,
            daily_count: state.daily_count,
            daily_limit: self.config.daily_limit,
            unique_addresses: state.address_limits.len(),
            drip_amount: self.config.drip_amount,
            cooldown_hours: self.config.cooldown_hours,
            is_testnet_only: self.config.testnet_only,
        }
    }

    /// Check if an address is eligible for faucet
    pub async fn check_eligibility(
        &self,
        address: &str,
        client_ip: Option<&str>,
    ) -> EligibilityResult {
        let state = self.state.read().await;

        // Check address cooldown
        if let Some(addr_limit) = state.address_limits.get(address) {
            let cooldown = Duration::hours(self.config.cooldown_hours as i64);
            let time_since_last = Utc::now().signed_duration_since(addr_limit.last_request);

            if time_since_last < cooldown {
                let next_eligible = addr_limit.last_request + cooldown;
                return EligibilityResult {
                    eligible: false,
                    reason: Some(format!("Cooldown active until {}", next_eligible)),
                    next_eligible: Some(next_eligible),
                    amount_available: 0,
                };
            }
        }

        // Check IP limit
        if let Some(ip) = client_ip {
            if let Some(ip_limit) = state.ip_limits.get(ip) {
                if ip_limit.request_count_today >= self.config.max_requests_per_ip {
                    let reset_time = state.last_daily_reset + Duration::hours(24);
                    return EligibilityResult {
                        eligible: false,
                        reason: Some(format!(
                            "IP rate limit exceeded ({}/{})",
                            ip_limit.request_count_today, self.config.max_requests_per_ip
                        )),
                        next_eligible: Some(reset_time),
                        amount_available: 0,
                    };
                }
            }
        }

        // Check global daily limit
        if state.daily_count >= self.config.daily_limit {
            let reset_time = state.last_daily_reset + Duration::hours(24);
            return EligibilityResult {
                eligible: false,
                reason: Some("Daily limit reached".to_string()),
                next_eligible: Some(reset_time),
                amount_available: 0,
            };
        }

        EligibilityResult {
            eligible: true,
            reason: None,
            next_eligible: None,
            amount_available: self.config.drip_amount,
        }
    }

    /// Cleanup old rate limit entries
    pub async fn cleanup_old_entries(&self) {
        let mut state = self.state.write().await;
        let cutoff = Utc::now() - Duration::hours(self.config.cleanup_interval_hours as i64);

        state.address_limits.retain(|_, v| v.last_request > cutoff);

        tracing::debug!(
            "Faucet cleanup: {} address entries remaining",
            state.address_limits.len()
        );
    }
}

/// Faucet statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaucetStatistics {
    /// Total tokens distributed (lifetime)
    pub total_distributed: u64,
    /// Requests today
    pub daily_count: u32,
    /// Daily limit
    pub daily_limit: u32,
    /// Unique addresses served
    pub unique_addresses: usize,
    /// Amount per request
    pub drip_amount: u64,
    /// Cooldown hours
    pub cooldown_hours: u32,
    /// Whether testnet-only mode is enabled
    pub is_testnet_only: bool,
}

/// Eligibility check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EligibilityResult {
    /// Whether the address is eligible
    pub eligible: bool,
    /// Reason if not eligible
    pub reason: Option<String>,
    /// When the address will be eligible next
    pub next_eligible: Option<DateTime<Utc>>,
    /// Amount available if eligible
    pub amount_available: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency_chain::CurrencyChainConfig;

    fn create_test_faucet() -> Faucet {
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        Faucet::new_for_testing(currency_chain)
    }

    #[tokio::test]
    async fn test_faucet_request_tokens() {
        let faucet = create_test_faucet();

        let result = faucet.request_tokens("test_address_123", None, None).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.amount, 1000_0000_0000);
        assert!(!response.tx_id.is_empty());
    }

    #[tokio::test]
    async fn test_faucet_rate_limiting() {
        let faucet = create_test_faucet();
        let address = "rate_limit_test_address";

        // First request should succeed
        let result1 = faucet.request_tokens(address, None, None).await;
        assert!(result1.is_ok());

        // Second request should be rate limited
        let result2 = faucet.request_tokens(address, None, None).await;
        assert!(matches!(result2, Err(FaucetError::RateLimited { .. })));
    }

    #[tokio::test]
    async fn test_faucet_ip_rate_limiting() {
        let mut config = FaucetConfig::for_testing();
        config.max_requests_per_ip = 2;
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let faucet = Faucet::new(config, currency_chain, None);

        let ip = "192.168.1.1";

        // First two requests should succeed (different addresses, same IP)
        let result1 = faucet.request_tokens("addr1", None, Some(ip)).await;
        assert!(result1.is_ok());

        let result2 = faucet.request_tokens("addr2", None, Some(ip)).await;
        assert!(result2.is_ok());

        // Third request should fail due to IP limit
        let result3 = faucet.request_tokens("addr3", None, Some(ip)).await;
        assert!(matches!(
            result3,
            Err(FaucetError::IpRateLimitExceeded { .. })
        ));
    }

    #[tokio::test]
    async fn test_faucet_daily_limit() {
        let mut config = FaucetConfig::for_testing();
        config.daily_limit = 2;
        config.cooldown_hours = 0; // No cooldown for this test
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let faucet = Faucet::new(config, currency_chain, None);

        // First two requests should succeed
        let _ = faucet.request_tokens("addr1", None, None).await.unwrap();
        let _ = faucet.request_tokens("addr2", None, None).await.unwrap();

        // Third request should fail due to daily limit
        let result3 = faucet.request_tokens("addr3", None, None).await;
        assert!(matches!(
            result3,
            Err(FaucetError::DailyLimitReached { .. })
        ));
    }

    #[tokio::test]
    async fn test_faucet_statistics() {
        let faucet = create_test_faucet();

        // Make a request
        let _ = faucet.request_tokens("stats_test_addr", None, None).await;

        let stats = faucet.get_statistics().await;
        assert_eq!(stats.daily_count, 1);
        assert_eq!(stats.unique_addresses, 1);
        assert!(stats.total_distributed > 0);
    }

    #[tokio::test]
    async fn test_faucet_eligibility_check() {
        let faucet = create_test_faucet();

        // Check eligibility before any request
        let eligibility = faucet.check_eligibility("new_address", None).await;
        assert!(eligibility.eligible);
        assert!(eligibility.amount_available > 0);

        // Make a request
        let _ = faucet.request_tokens("new_address", None, None).await;

        // Check eligibility after request (should be ineligible due to cooldown)
        let eligibility = faucet.check_eligibility("new_address", None).await;
        assert!(!eligibility.eligible);
        assert!(eligibility.next_eligible.is_some());
    }

    #[tokio::test]
    async fn test_faucet_captcha_required() {
        let mut config = FaucetConfig::for_testing();
        config.require_captcha = true;
        config.hcaptcha_secret = Some("test_secret".to_string());
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let faucet = Faucet::new(config, currency_chain, None);

        // Request without captcha should fail
        let result = faucet.request_tokens("captcha_test", None, None).await;
        assert!(matches!(result, Err(FaucetError::CaptchaRequired)));
    }

    #[test]
    fn test_parse_address_uuid() {
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let faucet = Faucet::new_for_testing(currency_chain);

        let uuid = Uuid::new_v4();
        let result = faucet.parse_address(&uuid.to_string());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, uuid);
    }

    #[test]
    fn test_parse_address_string() {
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let faucet = Faucet::new_for_testing(currency_chain);

        let result = faucet.parse_address("my_test_address");
        assert!(result.is_ok());
        // Should create deterministic UUID from string
        let result2 = faucet.parse_address("my_test_address");
        assert_eq!(result.unwrap(), result2.unwrap());
    }

    #[tokio::test]
    async fn test_mainnet_disabled() {
        let mut config = FaucetConfig::default();
        config.testnet_only = true;
        config.require_captcha = false;
        let currency_chain =
            Arc::new(CurrencyChainClient::new_mock(CurrencyChainConfig::default()));
        let faucet = Faucet::new(config, currency_chain, None);

        // In mainnet mode (DCHAT_NETWORK not set to "testnet"), should fail
        let result = faucet.request_tokens("mainnet_test", None, None).await;
        assert!(matches!(result, Err(FaucetError::MainnetDisabled)));
    }
}
