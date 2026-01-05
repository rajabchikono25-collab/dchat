//! Biometric Authentication Module
//!
//! This module provides biometric authentication capabilities for keyless onboarding.
//!
//! # Production Usage
//!
//! In production builds, this module requires integration with native platform SDKs:
//! - **iOS**: LocalAuthentication framework (Face ID / Touch ID)
//! - **Android**: BiometricPrompt API
//! - **Desktop**: Platform-specific biometric APIs (Windows Hello, macOS Touch ID)
//!
//! The native SDK integration should be implemented via FFI bindings. The expected
//! FFI boundary interface:
//!
//! ```c
//! // Expected C FFI interface for native biometric SDK
//! typedef struct {
//!     int success;        // 1 = authenticated, 0 = failed
//!     int error_code;     // Platform-specific error code
//!     char* error_msg;    // Null-terminated error message (caller must free)
//! } BiometricResult;
//!
//! // Check if biometric hardware is available
//! int dchat_biometric_available(void);
//!
//! // Perform biometric authentication (blocking)
//! BiometricResult dchat_biometric_authenticate(const char* reason);
//! ```
//!
//! # Development & Testing
//!
//! For testing purposes, enable the `mock-biometric` feature or run in `#[cfg(test)]`
//! mode. This enables environment variable simulation:
//! - `DCHAT_BIOMETRIC_OK=1` - Simulate successful authentication
//! - `DCHAT_BIOMETRIC_AVAILABLE=1` - Simulate biometric hardware availability
//!
//! **WARNING**: The mock implementation must NEVER be used in production builds.
//! Production builds without native SDK integration will return appropriate errors.

use dchat_core::error::{Error, Result};

// =============================================================================
// PRODUCTION IMPLEMENTATION (default - no mock feature)
// =============================================================================

/// Perform biometric authentication check.
///
/// # Production Behavior
///
/// In production builds (without `mock-biometric` feature), this function
/// returns an error indicating that native SDK integration is required.
/// This is a safety measure to ensure biometric checks are never silently
/// bypassed in production.
///
/// # Returns
///
/// - `Ok(true)` - Authentication successful (mock mode only)
/// - `Err(Error)` - Authentication failed or unavailable
#[cfg(not(any(test, feature = "mock-biometric")))]
pub async fn perform_biometric_check() -> Result<bool> {
    // Production builds require native SDK integration.
    // This stub ensures we fail safely rather than silently bypassing auth.
    Err(Error::unavailable(
        "Biometric authentication requires native SDK integration. \
         This build was compiled without biometric support. \
         Please use a platform-specific build with native biometric SDK bindings.",
    ))
}

/// Alias for `perform_biometric_check` for backward compatibility.
#[cfg(not(any(test, feature = "mock-biometric")))]
pub async fn authenticate_biometric() -> Result<bool> {
    perform_biometric_check().await
}

/// Check if device supports biometric authentication.
///
/// # Production Behavior
///
/// In production builds without native SDK integration, this always
/// returns `false` to indicate biometrics are not available.
#[cfg(not(any(test, feature = "mock-biometric")))]
pub fn biometric_available() -> bool {
    // Without native SDK integration, biometrics are not available
    false
}

// =============================================================================
// MOCK IMPLEMENTATION (test and mock-biometric feature only)
// =============================================================================

/// Environment variable to control mock biometric authentication result.
/// Set to "1" for success, any other value for failure.
#[cfg(any(test, feature = "mock-biometric"))]
const ENV_BIOMETRIC_OK: &str = "DCHAT_BIOMETRIC_OK";

/// Environment variable to control mock biometric availability.
/// Set to "1" to indicate biometrics are available.
#[cfg(any(test, feature = "mock-biometric"))]
const ENV_BIOMETRIC_AVAILABLE: &str = "DCHAT_BIOMETRIC_AVAILABLE";

/// Perform biometric authentication check (MOCK IMPLEMENTATION).
///
/// # Warning
///
/// This is a mock implementation for testing only. It uses environment
/// variables to simulate biometric authentication:
/// - `DCHAT_BIOMETRIC_OK=1` - Returns success
/// - Otherwise - Returns authentication failure
///
/// **This implementation is only available in test builds or when the
/// `mock-biometric` feature is enabled.**
#[cfg(any(test, feature = "mock-biometric"))]
pub async fn perform_biometric_check() -> Result<bool> {
    use std::env;

    // SAFETY: This code path is only compiled in test/mock builds
    let ok = env::var(ENV_BIOMETRIC_OK).unwrap_or_else(|_| "0".into());
    if ok == "1" {
        Ok(true)
    } else {
        Err(Error::unauthenticated("Biometric check failed (mock mode)"))
    }
}

/// Alias for `perform_biometric_check` for backward compatibility (MOCK).
#[cfg(any(test, feature = "mock-biometric"))]
pub async fn authenticate_biometric() -> Result<bool> {
    perform_biometric_check().await
}

/// Check if device supports biometric authentication (MOCK IMPLEMENTATION).
///
/// # Warning
///
/// This is a mock implementation for testing only. It reads the
/// `DCHAT_BIOMETRIC_AVAILABLE` environment variable:
/// - `DCHAT_BIOMETRIC_AVAILABLE=1` - Returns true
/// - Otherwise - Returns false
#[cfg(any(test, feature = "mock-biometric"))]
pub fn biometric_available() -> bool {
    use std::env;

    match env::var(ENV_BIOMETRIC_AVAILABLE) {
        Ok(v) => v == "1",
        Err(_) => false,
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_biometric_success_and_failure() {
        // Success case
        env::set_var(ENV_BIOMETRIC_OK, "1");
        assert!(perform_biometric_check().await.unwrap());

        // Failure case
        env::set_var(ENV_BIOMETRIC_OK, "0");
        let res = perform_biometric_check().await;
        assert!(res.is_err());

        env::remove_var(ENV_BIOMETRIC_OK);
    }

    #[tokio::test]
    async fn test_biometric_env_success() {
        env::set_var(ENV_BIOMETRIC_OK, "1");
        assert!(authenticate_biometric().await.unwrap());
        env::remove_var(ENV_BIOMETRIC_OK);
    }

    #[tokio::test]
    async fn test_biometric_env_fail() {
        env::set_var(ENV_BIOMETRIC_OK, "0");
        let res = authenticate_biometric().await;
        assert!(res.is_err());
        env::remove_var(ENV_BIOMETRIC_OK);
    }

    #[tokio::test]
    async fn test_biometric_availability() {
        // Not available by default
        env::remove_var(ENV_BIOMETRIC_AVAILABLE);
        assert!(!biometric_available());

        // Available when env var is set
        env::set_var(ENV_BIOMETRIC_AVAILABLE, "1");
        assert!(biometric_available());

        // Not available when set to other value
        env::set_var(ENV_BIOMETRIC_AVAILABLE, "0");
        assert!(!biometric_available());

        env::remove_var(ENV_BIOMETRIC_AVAILABLE);
    }
}
