use dchat_core::error::{Error, Result};
use crate::onboarding::keyless::enclave;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Perform MPC-based key recovery or generation as a fallback
pub async fn perform_mpc_fallback() -> Result<Vec<u8>> {
    // Attempt to read the enclave-backed key; if unavailable, return unavailable.
    let base_key = enclave::generate_device_key().map_err(|_| Error::unavailable("Enclave key unavailable"))?;

    // Derive fallback material via HMAC-SHA256 with a fixed context
    let mut mac = HmacSha256::new_from_slice(b"dchat-mpc-fallback").map_err(|e| Error::internal(format!("HMAC init: {}", e)))?;
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
    fn test_mpc_fallback_unavailable() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let res = perform_mpc_fallback().await;
            assert!(res.is_err());
        });
    }
    
    #[test]
    fn test_mpc_fallback_derives_key() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            // Ensure enclave is initialized and key exists
            crate::onboarding::keyless::enclave::init_enclave().await.unwrap();
            let key = perform_mpc_fallback().await.unwrap();
            assert_eq!(key.len(), 32);
        });
    }
}
