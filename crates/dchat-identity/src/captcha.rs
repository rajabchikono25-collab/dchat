//! CAPTCHA verification module for bot protection
//!
//! This module provides CAPTCHA verification support as an additional layer of
//! bot protection beyond the proof-of-work challenge. It supports multiple
//! CAPTCHA providers and can be used to gate high-risk operations.
//!
//! # Supported Providers
//! - **hCaptcha**: Privacy-focused CAPTCHA (recommended)
//! - **Cloudflare Turnstile**: Low-friction, privacy-preserving challenges
//! - **Disabled**: For testing or trusted environments
//!
//! # Security Model
//!
//! CAPTCHA verification is designed as a secondary defense layer:
//! 1. PoW challenge creates computational cost (primary protection)
//! 2. CAPTCHA adds human verification for suspicious accounts
//! 3. Behavioral analysis may trigger additional CAPTCHA requirements
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use dchat_identity::captcha::{CaptchaConfig, CaptchaProvider, verify_captcha};
//!
//! let config = CaptchaConfig {
//!     provider: CaptchaProvider::HCaptcha,
//!     secret_key: "your-secret-key".to_string(),
//!     site_key: "your-site-key".to_string(),
//!     ..Default::default()
//! };
//!
//! let result = verify_captcha(&config, "user-response-token", "127.0.0.1").await?;
//! if result.success {
//!     // User verified as human
//! }
//! ```

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
#[cfg(feature = "captcha")]
use std::time::Duration;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// CAPTCHA service provider
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CaptchaProvider {
    /// hCaptcha - privacy-focused (recommended)
    HCaptcha,
    /// Cloudflare Turnstile - low friction, privacy-preserving
    Turnstile,
    /// Disabled - no CAPTCHA verification (for testing/trusted environments)
    #[default]
    Disabled,
}

/// CAPTCHA verification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptchaConfig {
    /// CAPTCHA service provider
    pub provider: CaptchaProvider,
    /// Server-side secret key (from CAPTCHA provider dashboard)
    pub secret_key: String,
    /// Client-side site key (public, used in frontend)
    pub site_key: String,
    /// Verification timeout in seconds
    pub timeout_seconds: u64,
    /// Minimum score threshold for Turnstile (0.0-1.0, higher = more confident)
    pub min_score: f64,
    /// Whether to enforce CAPTCHA for all registrations or just suspicious ones
    pub enforce_for_all_registrations: bool,
    /// Bot score threshold that triggers CAPTCHA requirement
    pub bot_score_threshold: f64,
}

impl Default for CaptchaConfig {
    fn default() -> Self {
        Self {
            provider: CaptchaProvider::Disabled,
            secret_key: String::new(),
            site_key: String::new(),
            timeout_seconds: 10,
            min_score: 0.5,
            enforce_for_all_registrations: false,
            bot_score_threshold: 0.7,
        }
    }
}

impl CaptchaConfig {
    /// Create config for hCaptcha
    pub fn hcaptcha(secret_key: String, site_key: String) -> Self {
        Self {
            provider: CaptchaProvider::HCaptcha,
            secret_key,
            site_key,
            ..Default::default()
        }
    }

    /// Create config for Cloudflare Turnstile
    pub fn turnstile(secret_key: String, site_key: String) -> Self {
        Self {
            provider: CaptchaProvider::Turnstile,
            secret_key,
            site_key,
            ..Default::default()
        }
    }

    /// Check if CAPTCHA is enabled
    pub fn is_enabled(&self) -> bool {
        self.provider != CaptchaProvider::Disabled
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.provider == CaptchaProvider::Disabled {
            return Ok(());
        }

        if self.secret_key.is_empty() {
            return Err(Error::validation("CAPTCHA secret key is required"));
        }

        if self.site_key.is_empty() {
            return Err(Error::validation("CAPTCHA site key is required"));
        }

        if self.timeout_seconds == 0 {
            return Err(Error::validation("CAPTCHA timeout must be > 0"));
        }

        if self.provider == CaptchaProvider::Turnstile 
            && (self.min_score < 0.0 || self.min_score > 1.0) 
        {
            return Err(Error::validation("Turnstile min_score must be between 0.0 and 1.0"));
        }

        Ok(())
    }
}

// =============================================================================
// VERIFICATION RESULT
// =============================================================================

/// Result of CAPTCHA verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptchaVerificationResult {
    /// Whether the verification succeeded
    pub success: bool,
    /// Challenge timestamp (when the CAPTCHA was solved)
    pub challenge_ts: Option<String>,
    /// Hostname that served the CAPTCHA
    pub hostname: Option<String>,
    /// Error codes from the provider (if verification failed)
    pub error_codes: Vec<String>,
    /// Score (for Turnstile, 0.0-1.0)
    pub score: Option<f64>,
    /// Action associated with the CAPTCHA
    pub action: Option<String>,
}

impl CaptchaVerificationResult {
    /// Create a successful result for disabled CAPTCHA
    pub fn disabled() -> Self {
        Self {
            success: true,
            challenge_ts: None,
            hostname: None,
            error_codes: vec!["captcha-disabled".to_string()],
            score: Some(1.0),
            action: None,
        }
    }

    /// Create a failed result
    pub fn failed(error_codes: Vec<String>) -> Self {
        Self {
            success: false,
            challenge_ts: None,
            hostname: None,
            error_codes,
            score: None,
            action: None,
        }
    }
}

// =============================================================================
// hCaptcha VERIFICATION
// =============================================================================

/// hCaptcha verification response
#[cfg(feature = "captcha")]
#[derive(Debug, Deserialize)]
struct HCaptchaResponse {
    success: bool,
    challenge_ts: Option<String>,
    hostname: Option<String>,
    #[serde(default)]
    #[serde(rename = "error-codes")]
    error_codes: Vec<String>,
}

/// Verify hCaptcha response token
#[cfg(feature = "captcha")]
async fn verify_hcaptcha(
    secret_key: &str,
    response_token: &str,
    remote_ip: Option<&str>,
    timeout: Duration,
) -> Result<CaptchaVerificationResult> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;

    let mut params = vec![
        ("secret", secret_key.to_string()),
        ("response", response_token.to_string()),
    ];

    if let Some(ip) = remote_ip {
        params.push(("remoteip", ip.to_string()));
    }

    let response = client
        .post("https://api.hcaptcha.com/siteverify")
        .form(&params)
        .send()
        .await
        .map_err(|e| Error::network(format!("hCaptcha verification request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(Error::network(format!(
            "hCaptcha API returned error status: {}",
            response.status()
        )));
    }

    let hcaptcha_response: HCaptchaResponse = response
        .json()
        .await
        .map_err(|e| Error::network(format!("Failed to parse hCaptcha response: {}", e)))?;

    Ok(CaptchaVerificationResult {
        success: hcaptcha_response.success,
        challenge_ts: hcaptcha_response.challenge_ts,
        hostname: hcaptcha_response.hostname,
        error_codes: hcaptcha_response.error_codes,
        score: None, // hCaptcha doesn't provide scores
        action: None,
    })
}

// =============================================================================
// CLOUDFLARE TURNSTILE VERIFICATION
// =============================================================================

/// Turnstile verification response
#[cfg(feature = "captcha")]
#[derive(Debug, Deserialize)]
struct TurnstileResponse {
    success: bool,
    challenge_ts: Option<String>,
    hostname: Option<String>,
    #[serde(default)]
    #[serde(rename = "error-codes")]
    error_codes: Vec<String>,
    action: Option<String>,
    cdata: Option<String>,
}

/// Verify Cloudflare Turnstile response token
#[cfg(feature = "captcha")]
async fn verify_turnstile(
    secret_key: &str,
    response_token: &str,
    remote_ip: Option<&str>,
    timeout: Duration,
) -> Result<CaptchaVerificationResult> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;

    let mut params = vec![
        ("secret", secret_key.to_string()),
        ("response", response_token.to_string()),
    ];

    if let Some(ip) = remote_ip {
        params.push(("remoteip", ip.to_string()));
    }

    let response = client
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .form(&params)
        .send()
        .await
        .map_err(|e| Error::network(format!("Turnstile verification request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(Error::network(format!(
            "Turnstile API returned error status: {}",
            response.status()
        )));
    }

    let turnstile_response: TurnstileResponse = response
        .json()
        .await
        .map_err(|e| Error::network(format!("Failed to parse Turnstile response: {}", e)))?;

    Ok(CaptchaVerificationResult {
        success: turnstile_response.success,
        challenge_ts: turnstile_response.challenge_ts,
        hostname: turnstile_response.hostname,
        error_codes: turnstile_response.error_codes,
        score: None, // Turnstile scores are enterprise-only
        action: turnstile_response.action,
    })
}

// =============================================================================
// MAIN VERIFICATION API
// =============================================================================

/// Verify a CAPTCHA response token
///
/// # Arguments
/// * `config` - CAPTCHA configuration (provider, keys, etc.)
/// * `response_token` - The token from the client-side CAPTCHA widget
/// * `remote_ip` - Optional client IP address for additional validation
///
/// # Returns
/// * `Ok(CaptchaVerificationResult)` - Verification result (check `.success` field)
/// * `Err(Error)` - If verification request failed
///
/// # Security Notes
/// - Always verify on the server side, never trust client-side validation
/// - Store the verification result for audit logging
/// - Rate limit CAPTCHA verification attempts
#[cfg(feature = "captcha")]
pub async fn verify_captcha(
    config: &CaptchaConfig,
    response_token: &str,
    remote_ip: Option<&str>,
) -> Result<CaptchaVerificationResult> {
    // Validate configuration
    config.validate()?;

    let timeout = Duration::from_secs(config.timeout_seconds);

    match config.provider {
        CaptchaProvider::Disabled => {
            tracing::debug!("CAPTCHA verification skipped (disabled)");
            Ok(CaptchaVerificationResult::disabled())
        }
        CaptchaProvider::HCaptcha => {
            tracing::debug!("Verifying hCaptcha response");
            verify_hcaptcha(&config.secret_key, response_token, remote_ip, timeout).await
        }
        CaptchaProvider::Turnstile => {
            tracing::debug!("Verifying Turnstile response");
            verify_turnstile(&config.secret_key, response_token, remote_ip, timeout).await
        }
    }
}

/// Verify a CAPTCHA response token (non-async stub when feature disabled)
#[cfg(not(feature = "captcha"))]
pub async fn verify_captcha(
    config: &CaptchaConfig,
    _response_token: &str,
    _remote_ip: Option<&str>,
) -> Result<CaptchaVerificationResult> {
    if config.provider == CaptchaProvider::Disabled {
        return Ok(CaptchaVerificationResult::disabled());
    }
    
    Err(Error::internal(
        "CAPTCHA verification requires the 'captcha' feature to be enabled"
    ))
}

// =============================================================================
// CAPTCHA REQUIREMENT CHECKER
// =============================================================================

/// Determines if CAPTCHA should be required for an operation
#[derive(Debug)]
pub struct CaptchaRequirementChecker {
    config: CaptchaConfig,
}

impl CaptchaRequirementChecker {
    /// Create a new checker with the given config
    pub fn new(config: CaptchaConfig) -> Self {
        Self { config }
    }

    /// Check if CAPTCHA is required for registration
    ///
    /// # Arguments
    /// * `bot_score` - Behavioral bot score from rate limiter (0.0-1.0)
    /// * `is_suspicious_ip` - Whether the client IP is flagged as suspicious
    /// * `has_previous_failed_attempts` - Whether user has failed registrations
    pub fn requires_captcha_for_registration(
        &self,
        bot_score: Option<f64>,
        is_suspicious_ip: bool,
        has_previous_failed_attempts: bool,
    ) -> bool {
        if !self.config.is_enabled() {
            return false;
        }

        // Always require if enforced for all
        if self.config.enforce_for_all_registrations {
            return true;
        }

        // Require if bot score exceeds threshold
        if let Some(score) = bot_score {
            if score >= self.config.bot_score_threshold {
                tracing::info!(
                    bot_score = score,
                    threshold = self.config.bot_score_threshold,
                    "CAPTCHA required due to high bot score"
                );
                return true;
            }
        }

        // Require if IP is suspicious
        if is_suspicious_ip {
            tracing::info!("CAPTCHA required due to suspicious IP");
            return true;
        }

        // Require if previous failed attempts
        if has_previous_failed_attempts {
            tracing::info!("CAPTCHA required due to previous failed registration attempts");
            return true;
        }

        false
    }

    /// Check if CAPTCHA is required to lift a bot flag
    pub fn requires_captcha_for_bot_unflag(&self) -> bool {
        self.config.is_enabled()
    }

    /// Get the client-side site key
    pub fn site_key(&self) -> &str {
        &self.config.site_key
    }

    /// Get the CAPTCHA provider
    pub fn provider(&self) -> CaptchaProvider {
        self.config.provider
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_captcha_config_defaults() {
        let config = CaptchaConfig::default();
        assert!(!config.is_enabled());
        assert_eq!(config.provider, CaptchaProvider::Disabled);
    }

    #[test]
    fn test_captcha_config_hcaptcha() {
        let config = CaptchaConfig::hcaptcha(
            "secret".to_string(),
            "site".to_string(),
        );
        assert!(config.is_enabled());
        assert_eq!(config.provider, CaptchaProvider::HCaptcha);
    }

    #[test]
    fn test_captcha_config_turnstile() {
        let config = CaptchaConfig::turnstile(
            "secret".to_string(),
            "site".to_string(),
        );
        assert!(config.is_enabled());
        assert_eq!(config.provider, CaptchaProvider::Turnstile);
    }

    #[test]
    fn test_captcha_config_validation_disabled() {
        let config = CaptchaConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_captcha_config_validation_missing_secret() {
        let config = CaptchaConfig {
            provider: CaptchaProvider::HCaptcha,
            secret_key: String::new(),
            site_key: "site".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_captcha_config_validation_missing_site_key() {
        let config = CaptchaConfig {
            provider: CaptchaProvider::HCaptcha,
            secret_key: "secret".to_string(),
            site_key: String::new(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_requirement_checker_disabled() {
        let checker = CaptchaRequirementChecker::new(CaptchaConfig::default());
        assert!(!checker.requires_captcha_for_registration(None, false, false));
        assert!(!checker.requires_captcha_for_registration(Some(0.9), true, true));
    }

    #[test]
    fn test_requirement_checker_enforce_all() {
        let config = CaptchaConfig {
            provider: CaptchaProvider::HCaptcha,
            secret_key: "secret".to_string(),
            site_key: "site".to_string(),
            enforce_for_all_registrations: true,
            ..Default::default()
        };
        let checker = CaptchaRequirementChecker::new(config);
        assert!(checker.requires_captcha_for_registration(None, false, false));
    }

    #[test]
    fn test_requirement_checker_bot_score() {
        let config = CaptchaConfig {
            provider: CaptchaProvider::HCaptcha,
            secret_key: "secret".to_string(),
            site_key: "site".to_string(),
            bot_score_threshold: 0.5,
            ..Default::default()
        };
        let checker = CaptchaRequirementChecker::new(config);
        
        // Below threshold - no CAPTCHA
        assert!(!checker.requires_captcha_for_registration(Some(0.3), false, false));
        
        // Above threshold - CAPTCHA required
        assert!(checker.requires_captcha_for_registration(Some(0.6), false, false));
    }

    #[test]
    fn test_requirement_checker_suspicious_ip() {
        let config = CaptchaConfig {
            provider: CaptchaProvider::HCaptcha,
            secret_key: "secret".to_string(),
            site_key: "site".to_string(),
            ..Default::default()
        };
        let checker = CaptchaRequirementChecker::new(config);
        
        assert!(checker.requires_captcha_for_registration(None, true, false));
    }

    #[test]
    fn test_requirement_checker_failed_attempts() {
        let config = CaptchaConfig {
            provider: CaptchaProvider::HCaptcha,
            secret_key: "secret".to_string(),
            site_key: "site".to_string(),
            ..Default::default()
        };
        let checker = CaptchaRequirementChecker::new(config);
        
        assert!(checker.requires_captcha_for_registration(None, false, true));
    }

    #[test]
    fn test_verification_result_disabled() {
        let result = CaptchaVerificationResult::disabled();
        assert!(result.success);
        assert_eq!(result.score, Some(1.0));
    }

    #[test]
    fn test_verification_result_failed() {
        let result = CaptchaVerificationResult::failed(vec!["invalid-token".to_string()]);
        assert!(!result.success);
        assert_eq!(result.error_codes, vec!["invalid-token"]);
    }

    #[tokio::test]
    async fn test_verify_captcha_disabled() {
        let config = CaptchaConfig::default();
        let result = verify_captcha(&config, "any-token", None).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    #[cfg(not(feature = "captcha"))]
    async fn test_verify_captcha_feature_disabled() {
        let config = CaptchaConfig {
            provider: CaptchaProvider::HCaptcha,
            secret_key: "secret".to_string(),
            site_key: "site".to_string(),
            ..Default::default()
        };
        let result = verify_captcha(&config, "token", None).await;
        assert!(result.is_err());
    }
}
