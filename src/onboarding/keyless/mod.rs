pub mod biometric;
pub mod enclave;
pub mod mpc_fallback;

use dchat_core::error::{Error, Result};
use dchat_identity::{verify_device_attestation, AttestationResult};

/// Initialize keyless onboarding subsystem (e.g., load enclave, setup biometric hooks)
pub async fn init_keyless() -> Result<()> {
    enclave::init_enclave().await?;
    Ok(())
}

/// Generate or retrieve a device key (may be backed by enclave or MPC)
pub fn get_device_key() -> Result<Vec<u8>> {
    // Prefer enclave-backed key, fallback to MPC-derived key
    if let Ok(key) = enclave::generate_device_key() {
        return Ok(key);
    }
    // Try MPC fallback (may block briefly)
    match tokio::runtime::Runtime::new()
        .map_err(|e| Error::internal(format!("failed to create runtime: {}", e)))?
        .block_on(mpc_fallback::perform_mpc_fallback())
    {
        Ok(k) => Ok(k),
        Err(_) => Err(Error::internal("Device key unavailable")),
    }
}

/// Perform attestation via enclave and verify via identity crate verifier
pub async fn attest_and_verify() -> Result<AttestationResult> {
    let att = enclave::attest_device()?;
    let res = verify_device_attestation(&att)?;
    Ok(res)
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_init_and_get_key() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            init_keyless().await.unwrap();
            let key = get_device_key().unwrap();
            assert_eq!(key.len(), 32);
        });
    }
}
