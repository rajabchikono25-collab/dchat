use std::env;

use dchat_core::error::{Error, Result};

/// Simulate a biometric check. In real deployments this hooks into platform SDKs.
pub async fn perform_biometric_check() -> Result<bool> {
    // For development we read an env var `DCHAT_BIOMETRIC_OK` = "1" to succeed.
    let ok = env::var("DCHAT_BIOMETRIC_OK").unwrap_or_else(|_| "0".into());
    if ok == "1" {
        Ok(true)
    } else {
        Err(Error::unauthenticated("Biometric check failed"))
    }
}

/// Alias for perform_biometric_check for backward compatibility.
pub async fn authenticate_biometric() -> Result<bool> {
    perform_biometric_check().await
}

/// Check if device supports biometric authentication.
/// Currently this checks for an override environment variable `DCHAT_BIOMETRIC_AVAILABLE`.
pub fn biometric_available() -> bool {
    match env::var("DCHAT_BIOMETRIC_AVAILABLE") {
        Ok(v) => v == "1",
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_biometric_success_and_failure() {
        // Success case
        env::set_var("DCHAT_BIOMETRIC_OK", "1");
        assert!(perform_biometric_check().await.unwrap());

        // Failure case
        env::set_var("DCHAT_BIOMETRIC_OK", "0");
        let res = perform_biometric_check().await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_biometric_env_success() {
        env::set_var("DCHAT_BIOMETRIC_OK", "1");
        assert!(authenticate_biometric().await.unwrap());
        env::remove_var("DCHAT_BIOMETRIC_OK");
    }

    #[tokio::test]
    async fn test_biometric_env_fail() {
        env::set_var("DCHAT_BIOMETRIC_OK", "0");
        let res = authenticate_biometric().await;
        assert!(res.is_err());
        env::remove_var("DCHAT_BIOMETRIC_OK");
    }
}
