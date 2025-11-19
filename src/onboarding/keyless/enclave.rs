use dchat_core::error::{Error, Result};
use rand::RngCore;
use std::path::PathBuf;

fn enclave_storage_path() -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push("dchat_enclave");
    dir
}

/// Initialize the secure enclave or simulated enclave storage
pub async fn init_enclave() -> Result<()> {
    let dir = enclave_storage_path();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| Error::internal(format!("Failed to create enclave dir: {}", e)))?;
    Ok(())
}

/// Generate or fetch the device private key from the (simulated) enclave.
/// Returns a 32-byte key.
pub fn generate_device_key() -> Result<Vec<u8>> {
    let mut path = enclave_storage_path();
    path.push("device_key.bin");

    if path.exists() {
        let data = std::fs::read(&path).map_err(|e| Error::internal(format!("read key: {}", e)))?;
        return Ok(data);
    }

    // Generate a random 32-byte key and store it
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    std::fs::write(&path, &key).map_err(|e| Error::internal(format!("write key: {}", e)))?;
    Ok(key)
}

/// Produce an attestation payload proving the key is enclave-backed.
/// This is a simulated attestation string for now.
pub fn attest_device() -> Result<String> {
    // In production, create TPM/SE attestation. For now return deterministic placeholder.
    Ok("dchat-enclave-attestation-v1".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_enclave_init_and_key() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            init_enclave().await.unwrap();
            let key = generate_device_key().unwrap();
            assert_eq!(key.len(), 32);
            let key2 = generate_device_key().unwrap();
            assert_eq!(key, key2);
        });
    }
}
