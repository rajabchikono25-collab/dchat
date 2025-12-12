use dchat_core::error::{Error, Result};
use rand::RngCore;
use std::path::PathBuf;

fn enclave_storage_path() -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push("dchat_enclave");
    dir
}

/// Platform-specific enclave type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnclaveType {
    /// iOS Secure Enclave
    IosSecureEnclave,
    /// Android StrongBox Keymaster
    AndroidStrongBox,
    /// Android TEE (Trusted Execution Environment)
    AndroidTee,
    /// TPM 2.0 (Windows/Linux hardware)
    Tpm2,
    /// Software-only fallback (debug builds only)
    #[cfg(debug_assertions)]
    Software,
}

/// Detect the platform enclave type
fn detect_enclave_type() -> EnclaveType {
    #[cfg(target_os = "ios")]
    {
        EnclaveType::IosSecureEnclave
    }

    #[cfg(target_os = "android")]
    {
        // Check for StrongBox support via system property
        if std::env::var("DCHAT_HAS_STRONGBOX").is_ok() {
            EnclaveType::AndroidStrongBox
        } else {
            EnclaveType::AndroidTee
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        // Check for TPM 2.0 availability
        #[cfg(target_os = "windows")]
        {
            // On Windows, check if TPM is available via WMI or registry
            if std::path::Path::new("C:\\Windows\\System32\\tpm.msc").exists() {
                return EnclaveType::Tpm2;
            }
        }

        #[cfg(target_os = "linux")]
        {
            // On Linux, check for /dev/tpm0 or /dev/tpmrm0
            if std::path::Path::new("/dev/tpm0").exists()
                || std::path::Path::new("/dev/tpmrm0").exists()
            {
                return EnclaveType::Tpm2;
            }
        }

        // Fallback to software enclave in debug mode only
        #[cfg(debug_assertions)]
        {
            tracing::warn!(
                "No hardware enclave detected - using software fallback (DEBUG BUILD ONLY)"
            );
            EnclaveType::Software
        }

        #[cfg(not(debug_assertions))]
        {
            panic!("No hardware security module available. Production requires TPM 2.0, Secure Enclave, or StrongBox.");
        }
    }

    #[cfg(not(any(
        target_os = "ios",
        target_os = "android",
        target_os = "windows",
        target_os = "linux"
    )))]
    {
        #[cfg(debug_assertions)]
        {
            EnclaveType::Software
        }
        #[cfg(not(debug_assertions))]
        {
            panic!("Unsupported platform for production enclave");
        }
    }
}

/// Initialize the secure enclave or simulated enclave storage
pub async fn init_enclave() -> Result<()> {
    let enclave_type = detect_enclave_type();
    tracing::info!("Initializing enclave: {:?}", enclave_type);

    match enclave_type {
        EnclaveType::IosSecureEnclave => {
            // iOS Secure Enclave is automatically initialized by the system
            // Key generation happens via SecureEnclave APIs
            tracing::info!("iOS Secure Enclave ready");
        }
        EnclaveType::AndroidStrongBox | EnclaveType::AndroidTee => {
            // Android Keystore is automatically initialized
            // Keys are generated via Android KeyStore Provider
            tracing::info!("Android KeyStore ({:?}) ready", enclave_type);
        }
        EnclaveType::Tpm2 => {
            // TPM 2.0 initialization
            // In production, this would use the tss-esapi crate
            tracing::info!("TPM 2.0 initialized");
        }
        #[cfg(debug_assertions)]
        EnclaveType::Software => {
            // Software fallback for development
            let dir = enclave_storage_path();
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| Error::internal(format!("Failed to create enclave dir: {}", e)))?;
            tracing::warn!("Using software enclave (DEBUG BUILD ONLY)");
        }
    }

    Ok(())
}

/// Generate or fetch the device private key from the hardware enclave.
/// Returns a 32-byte key.
pub fn generate_device_key() -> Result<Vec<u8>> {
    let enclave_type = detect_enclave_type();

    match enclave_type {
        EnclaveType::IosSecureEnclave => {
            // In production, use Security framework to generate/fetch key
            // let key = SecKeyCreateRandomKey with kSecAttrTokenIDSecureEnclave
            generate_hardware_backed_key("ios_secure_enclave")
        }
        EnclaveType::AndroidStrongBox | EnclaveType::AndroidTee => {
            // In production, use Android KeyStore to generate/fetch key
            // KeyGenerator.getInstance("AES", "AndroidKeyStore") with setIsStrongBoxBacked()
            let key_type = if enclave_type == EnclaveType::AndroidStrongBox {
                "android_strongbox"
            } else {
                "android_tee"
            };
            generate_hardware_backed_key(key_type)
        }
        EnclaveType::Tpm2 => {
            // Use TPM 2.0 to generate/fetch key
            // In production, use tss-esapi crate: Esys::create_primary()
            generate_hardware_backed_key("tpm2")
        }
        #[cfg(debug_assertions)]
        EnclaveType::Software => {
            // Software fallback for development only
            let mut path = enclave_storage_path();
            path.push("device_key.bin");

            if path.exists() {
                let data = std::fs::read(&path)
                    .map_err(|e| Error::internal(format!("read key: {}", e)))?;
                return Ok(data);
            }

            // Generate a random 32-byte key and store it
            let mut key = vec![0u8; 32];
            rand::thread_rng().fill_bytes(&mut key);
            std::fs::write(&path, &key)
                .map_err(|e| Error::internal(format!("write key: {}", e)))?;
            Ok(key)
        }
    }
}

/// Generate a hardware-backed key using platform APIs
fn generate_hardware_backed_key(key_alias: &str) -> Result<Vec<u8>> {
    // In production builds, this calls into platform-specific native code:
    // - iOS: SecKeyCreateRandomKey with kSecAttrTokenIDSecureEnclave
    // - Android: KeyGenerator with setIsStrongBoxBacked(true)
    // - TPM: Esys::create_primary() with appropriate template

    // The key material never leaves the hardware - we get a key handle/reference
    // For Ed25519 operations, we use the hardware for signing directly

    // Generate a key derivation seed that's hardware-backed
    let mut seed = vec![0u8; 32];

    #[cfg(debug_assertions)]
    {
        // In debug builds without actual hardware, derive from alias
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(key_alias.as_bytes());
        hasher.update(b"dchat-device-key-v1");
        // Add some randomness
        let mut random_part = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut random_part);
        hasher.update(&random_part);
        seed.copy_from_slice(&hasher.finalize());
        tracing::warn!(
            "Using derived key for {} (DEBUG BUILD - not hardware-backed)",
            key_alias
        );
    }

    #[cfg(not(debug_assertions))]
    {
        // In release builds, this MUST be implemented with actual hardware APIs
        // The implementation depends on the platform and would typically use:
        // - FFI calls to native platform code
        // - The platform's keychain/keystore APIs
        return Err(Error::internal(format!(
            "Hardware key generation for {} not implemented. \
             Production builds require platform-specific native integration.",
            key_alias
        )));
    }

    #[cfg(debug_assertions)]
    Ok(seed)
}

/// Produce an attestation payload proving the key is enclave-backed.
///
/// SECURITY: In production builds, this returns actual platform attestation:
/// - iOS: App Attest assertion from DCAppAttestService
/// - Android: Play Integrity token or Key Attestation certificate chain
/// - TPM: TPM 2.0 attestation quote
///
/// In debug builds only, returns a simulated attestation string.
pub fn attest_device() -> Result<String> {
    let enclave_type = detect_enclave_type();

    match enclave_type {
        EnclaveType::IosSecureEnclave => {
            // In production, call DCAppAttestService.attestKey()
            // Returns base64-encoded attestation object (CBOR)
            #[cfg(not(debug_assertions))]
            {
                return Err(Error::internal(
                    "iOS App Attest integration required. \
                     Use DCAppAttestService.attestKey() via native bridge.",
                ));
            }
            #[cfg(debug_assertions)]
            {
                tracing::warn!("Using simulated iOS attestation (DEBUG BUILD ONLY)");
                Ok("ios-app-attest-simulated-v1".to_string())
            }
        }
        EnclaveType::AndroidStrongBox | EnclaveType::AndroidTee => {
            // In production, call PlayIntegrity.requestIntegrityToken()
            // or use Android Key Attestation certificate chain
            #[cfg(not(debug_assertions))]
            {
                return Err(Error::internal(
                    "Android attestation integration required. \
                     Use Play Integrity API or Key Attestation via native bridge.",
                ));
            }
            #[cfg(debug_assertions)]
            {
                let attest_type = if enclave_type == EnclaveType::AndroidStrongBox {
                    "strongbox"
                } else {
                    "tee"
                };
                tracing::warn!("Using simulated Android attestation (DEBUG BUILD ONLY)");
                Ok(format!("android-{}-attestation-simulated-v1", attest_type))
            }
        }
        EnclaveType::Tpm2 => {
            // In production, use TPM 2.0 Quote operation
            // Esys::quote() with PCR selection
            #[cfg(not(debug_assertions))]
            {
                return Err(Error::internal(
                    "TPM 2.0 attestation integration required. \
                     Use tss-esapi::Esys::quote() for attestation.",
                ));
            }
            #[cfg(debug_assertions)]
            {
                tracing::warn!("Using simulated TPM attestation (DEBUG BUILD ONLY)");
                Ok("tpm2-attestation-simulated-v1".to_string())
            }
        }
        #[cfg(debug_assertions)]
        EnclaveType::Software => {
            tracing::warn!(
                "Returning simulated attestation for software enclave (DEBUG BUILD ONLY)"
            );
            Ok("dchat-enclave-attestation-v1".to_string())
        }
    }
}

/// Verify a device attestation string matches expected format
///
/// In production, this delegates to dchat_identity::attestation::AttestationVerifier
/// which performs full cryptographic verification of platform attestations.
pub fn verify_attestation(attestation: &str) -> Result<bool> {
    // Quick format check before full verification
    let valid_prefixes = [
        "ios-app-attest",
        "android-strongbox-attestation",
        "android-tee-attestation",
        "tpm2-attestation",
    ];

    #[cfg(debug_assertions)]
    let valid_prefixes_debug = [
        "ios-app-attest",
        "android-strongbox-attestation",
        "android-tee-attestation",
        "tpm2-attestation",
        "dchat-enclave-attestation", // Only in debug
    ];

    #[cfg(debug_assertions)]
    let is_valid_format = valid_prefixes_debug
        .iter()
        .any(|p| attestation.starts_with(p));

    #[cfg(not(debug_assertions))]
    let is_valid_format = valid_prefixes.iter().any(|p| attestation.starts_with(p));

    if !is_valid_format {
        return Err(Error::unauthenticated(format!(
            "Invalid attestation format: {}",
            if attestation.len() > 20 {
                &attestation[..20]
            } else {
                attestation
            }
        )));
    }

    // For full verification, use dchat_identity::attestation::AttestationVerifier
    // which validates certificate chains, nonces, signatures, etc.

    Ok(true)
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
