use dchat_core::error::{Error, Result};

/// Simulate a biometric check. In real deployments this hooks into platform SDKs.
pub async fn perform_biometric_check() -> Result<bool> {
    // For development we read an env var `DCHAT_BIOMETRIC_OK` = "1" to succeed.
    let ok = std::env::var("DCHAT_BIOMETRIC_OK").unwrap_or_else(|_| "0".into());
    if ok == "1" {
        Ok(true)
    } else {
        Err(Error::unauthenticated("Biometric check failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_biometric_success_and_failure() {
        let rt = Runtime::new().unwrap();
        // Success case
        std::env::set_var("DCHAT_BIOMETRIC_OK", "1");
        rt.block_on(async {
            assert!(perform_biometric_check().await.unwrap());
        });
        // Failure case
        std::env::set_var("DCHAT_BIOMETRIC_OK", "0");
        rt.block_on(async {
            let res = perform_biometric_check().await;
            assert!(res.is_err());
        });
    }
}
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
    use std::env;

    #[tokio::test]
    async fn test_biometric_env_success() {
        env::set_var("DCHAT_BIOMETRIC_OK", "1");
        assert!(authenticate_biometric().await.unwrap());
        env::remove_var("DCHAT_BIOMETRIC_OK");
    }

    #[tokio::test]
    async fn test_biometric_env_fail() {
        env::set_var("DCHAT_BIOMETRIC_OK", "0");
        assert!(!authenticate_biometric().await.unwrap());
        env::remove_var("DCHAT_BIOMETRIC_OK");
    }
}
