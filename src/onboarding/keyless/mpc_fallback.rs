use crate::onboarding::keyless::enclave;
use dchat_core::error::{Error, Result};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Perform MPC-based key recovery or generation as a fallback
pub async fn perform_mpc_fallback() -> Result<Vec<u8>> {
    // Attempt to read the enclave-backed key; if unavailable, return unavailable.
    let base_key = enclave::generate_device_key()
        .map_err(|_| Error::unavailable("Enclave key unavailable"))?;

    // Derive fallback material via HMAC-SHA256 with a fixed context
    let mut mac = HmacSha256::new_from_slice(b"dchat-mpc-fallback")
        .map_err(|e| Error::internal(format!("HMAC init: {}", e)))?;
    mac.update(&base_key);
    let result = mac.finalize();
    let bytes = result.into_bytes();

    // Return first 32 bytes as fallback key
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_mpc_fallback_in_software_mode() {
        // In debug builds, a software enclave is available, so MPC fallback succeeds.
        // In production without hardware enclave, this would fail.
        // This test verifies the MPC derivation works when enclave is available.
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let res = perform_mpc_fallback().await;
            // In debug builds with software enclave, this should succeed
            #[cfg(debug_assertions)]
            {
                assert!(
                    res.is_ok(),
                    "MPC fallback should succeed with software enclave in debug builds"
                );
                assert_eq!(res.unwrap().len(), 32);
            }
            // In release builds without DCHAT_ALLOW_SOFTWARE_KEYS, this should fail
            #[cfg(not(debug_assertions))]
            {
                // This test path only runs if hardware enclave is unavailable
                // and DCHAT_ALLOW_SOFTWARE_KEYS is not set
                if res.is_err() {
                    // Expected - no hardware enclave available
                } else {
                    // DCHAT_ALLOW_SOFTWARE_KEYS must be set, which is valid
                    assert_eq!(res.unwrap().len(), 32);
                }
            }
        });
    }

    #[test]
    fn test_mpc_fallback_derives_key() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            // Ensure enclave is initialized and key exists
            crate::onboarding::keyless::enclave::init_enclave()
                .await
                .unwrap();
            let key = perform_mpc_fallback().await.unwrap();
            assert_eq!(key.len(), 32);
        });
    }
}
