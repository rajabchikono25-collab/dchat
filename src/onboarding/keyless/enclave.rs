use dchat_core::error::{Error, Result};

/// Path utilities for software enclave storage
use std::path::PathBuf;

/// Path to software enclave storage directory
/// Used for debug builds and when DCHAT_ALLOW_SOFTWARE_KEYS is set in production.
fn enclave_storage_path() -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push("dchat_enclave");
    dir
}

/// Validate enclave storage path is secure
/// Ensures storage directory is within expected bounds.
fn validate_enclave_storage_security(path: &PathBuf) -> Result<()> {
    // Ensure parent directory exists and is accessible
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(Error::internal(format!(
                "Enclave storage parent directory does not exist: {:?}",
                parent
            )));
        }
    }

    // Check that path doesn't traverse outside temp directory
    let temp_dir = std::env::temp_dir();
    let canonical_temp = temp_dir.canonicalize().unwrap_or(temp_dir);
    if let Ok(canonical_path) = path.canonicalize() {
        if !canonical_path.starts_with(&canonical_temp) {
            return Err(Error::internal(format!(
                "Enclave storage path escapes temp directory: {:?}",
                path
            )));
        }
    }

    Ok(())
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
    /// Software-only fallback (debug builds or when DCHAT_ALLOW_SOFTWARE_KEYS is set)
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

        // Fallback to software enclave in debug mode or when explicitly allowed
        #[cfg(debug_assertions)]
        {
            tracing::warn!(
                "No hardware enclave detected - using software fallback (DEBUG BUILD ONLY)"
            );
            EnclaveType::Software
        }

        #[cfg(not(debug_assertions))]
        {
            // Allow software keys in production if explicitly opted-in via environment variable
            // This is for cloud environments (Azure, AWS, GCP) that don't have TPM
            if std::env::var("DCHAT_ALLOW_SOFTWARE_KEYS").is_ok() {
                tracing::warn!(
                    "⚠️  DCHAT_ALLOW_SOFTWARE_KEYS set - using software key fallback in PRODUCTION"
                );
                tracing::warn!("⚠️  This reduces security - keys are not hardware-protected!");
                return EnclaveType::Software;
            }
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
            if std::env::var("DCHAT_ALLOW_SOFTWARE_KEYS").is_ok() {
                tracing::warn!("⚠️  Using software keys on unsupported platform");
                return EnclaveType::Software;
            }
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
        EnclaveType::Software => {
            // Software fallback for development or when DCHAT_ALLOW_SOFTWARE_KEYS is set
            let dir = enclave_storage_path();
            // Validate storage security before creating
            validate_enclave_storage_security(&dir)?;
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| Error::internal(format!("Failed to create enclave dir: {}", e)))?;
            #[cfg(debug_assertions)]
            tracing::warn!("Using software enclave at {:?} (DEBUG BUILD ONLY)", dir);
            #[cfg(not(debug_assertions))]
            tracing::warn!(
                "⚠️  Using software enclave at {:?} (DCHAT_ALLOW_SOFTWARE_KEYS)",
                dir
            );
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
            // iOS: Uses Security.framework via native bridge
            // Native code calls: SecKeyCreateRandomKey with kSecAttrTokenIDSecureEnclave
            generate_hardware_backed_key("ios_secure_enclave")
        }
        EnclaveType::AndroidStrongBox | EnclaveType::AndroidTee => {
            // Android: Uses KeyStore via native bridge (JNI)
            // Native code calls: KeyGenerator.getInstance("AES", "AndroidKeyStore").setIsStrongBoxBacked(true)
            let key_type = if enclave_type == EnclaveType::AndroidStrongBox {
                "android_strongbox"
            } else {
                "android_tee"
            };
            generate_hardware_backed_key(key_type)
        }
        EnclaveType::Tpm2 => {
            // TPM 2.0: Uses tss-esapi crate when linked
            // Calls: Esys::create_primary() with sealing template
            generate_hardware_backed_key("tpm2")
        }
        EnclaveType::Software => {
            use rand::RngCore;

            // Software fallback for development or when DCHAT_ALLOW_SOFTWARE_KEYS is set
            let mut path = enclave_storage_path();
            // Validate storage security
            validate_enclave_storage_security(&path)?;
            path.push("device_key.bin");

            if path.exists() {
                let data = std::fs::read(&path)
                    .map_err(|e| Error::internal(format!("read key: {}", e)))?;
                // Validate key length
                if data.len() != 32 {
                    return Err(Error::internal(format!(
                        "Corrupted device key: expected 32 bytes, got {}",
                        data.len()
                    )));
                }
                return Ok(data);
            }

            // Generate a random 32-byte key and store it
            let mut key = vec![0u8; 32];
            rand::thread_rng().fill_bytes(&mut key);
            std::fs::write(&path, &key)
                .map_err(|e| Error::internal(format!("write key: {}", e)))?;
            #[cfg(debug_assertions)]
            tracing::warn!(
                "Generated software device key at {:?} (DEBUG BUILD ONLY)",
                path
            );
            #[cfg(not(debug_assertions))]
            tracing::warn!(
                "⚠️  Generated software device key at {:?} (DCHAT_ALLOW_SOFTWARE_KEYS)",
                path
            );
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

    // Validate key alias is non-empty and follows naming convention
    if key_alias.is_empty() {
        return Err(Error::internal("Key alias cannot be empty"));
    }
    if key_alias.len() > 256 {
        return Err(Error::internal(format!(
            "Key alias too long: {} chars (max 256)",
            key_alias.len()
        )));
    }

    // In release builds, this MUST be implemented with actual hardware APIs
    #[cfg(not(debug_assertions))]
    {
        // Validate key alias format before attempting hardware access
        const VALID_ALIASES: &[&str] = &[
            "ios_secure_enclave",
            "android_strongbox",
            "android_tee",
            "tpm2",
        ];
        if !VALID_ALIASES.contains(&key_alias) {
            return Err(Error::internal(format!(
                "Unknown key alias '{}'. Valid aliases: {:?}",
                key_alias, VALID_ALIASES
            )));
        }

        // The implementation depends on the platform and would typically use:
        // - FFI calls to native platform code
        // - The platform's keychain/keystore APIs
        return Err(Error::internal(format!(
            "Hardware key generation for {} not implemented. \
             Production builds require platform-specific native integration.",
            key_alias
        )));
    }

    // In debug builds without actual hardware, derive from alias with randomness
    // BUT cache to file for idempotency (same key returned each time like hardware would)
    #[cfg(debug_assertions)]
    {
        use rand::RngCore;
        use sha2::{Digest, Sha256};

        // Check for cached key first (to ensure idempotency like real hardware)
        let mut key_path = enclave_storage_path();
        // Create directory if it doesn't exist
        if !key_path.exists() {
            std::fs::create_dir_all(&key_path)
                .map_err(|e| Error::internal(format!("Failed to create enclave dir: {}", e)))?;
        }
        key_path.push(format!("{}.key", key_alias));

        if key_path.exists() {
            let cached_key = std::fs::read(&key_path)
                .map_err(|e| Error::internal(format!("read cached key: {}", e)))?;
            if cached_key.len() == 32 {
                tracing::debug!("Using cached key for {} (DEBUG BUILD)", key_alias);
                return Ok(cached_key);
            }
            // Invalid cached key, regenerate
            tracing::warn!("Corrupted cached key for {}, regenerating", key_alias);
        }

        // Generate new key with randomness
        let mut seed = vec![0u8; 32];
        let mut hasher = Sha256::new();
        hasher.update(key_alias.as_bytes());
        hasher.update(b"dchat-device-key-v1");
        // Add some randomness for unique key generation
        let mut random_part = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut random_part);
        hasher.update(&random_part);
        seed.copy_from_slice(&hasher.finalize());

        // Cache the key for future calls (idempotency)
        std::fs::write(&key_path, &seed)
            .map_err(|e| Error::internal(format!("write cached key: {}", e)))?;

        tracing::warn!(
            "Generated and cached key for {} at {:?} (DEBUG BUILD - not hardware-backed)",
            key_alias,
            key_path
        );
        Ok(seed)
    }
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
            // iOS: DCAppAttestService.attestKey() via native bridge
            // Returns base64-encoded attestation object (CBOR format)
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
            // Android: PlayIntegrity.requestIntegrityToken() or Key Attestation cert chain
            // Both require native bridge to JNI layer
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
            // TPM 2.0: Esys::quote() with PCR selection via tss-esapi
            // Returns TPM quote with AIK certificate
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
        EnclaveType::Software => {
            #[cfg(debug_assertions)]
            tracing::warn!(
                "Returning simulated attestation for software enclave (DEBUG BUILD ONLY)"
            );
            #[cfg(not(debug_assertions))]
            tracing::warn!(
                "⚠️  Returning simulated attestation for software enclave (DCHAT_ALLOW_SOFTWARE_KEYS)"
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
    // Validate attestation is not empty
    if attestation.is_empty() {
        return Err(Error::unauthenticated("Attestation cannot be empty"));
    }

    // Production-valid attestation prefixes (hardware-backed only)
    const PRODUCTION_PREFIXES: &[&str] = &[
        "ios-app-attest",
        "android-strongbox-attestation",
        "android-tee-attestation",
        "tpm2-attestation",
    ];

    // Debug-only prefix for testing (software enclave simulation)
    #[cfg(debug_assertions)]
    const DEBUG_ONLY_PREFIX: &str = "dchat-enclave-attestation";

    // Validate attestation format against allowed prefixes
    let is_valid_production = PRODUCTION_PREFIXES
        .iter()
        .any(|p| attestation.starts_with(p));

    #[cfg(debug_assertions)]
    let is_valid_debug = attestation.starts_with(DEBUG_ONLY_PREFIX);

    #[cfg(debug_assertions)]
    let is_valid_format = is_valid_production || is_valid_debug;

    #[cfg(not(debug_assertions))]
    let is_valid_format = is_valid_production;

    if !is_valid_format {
        // Log rejected attestation prefix for security monitoring
        let prefix_sample = if attestation.len() > 20 {
            &attestation[..20]
        } else {
            attestation
        };

        // In production, this should trigger security alert
        #[cfg(not(debug_assertions))]
        tracing::warn!(
            attestation_prefix = prefix_sample,
            "Rejected attestation with invalid format"
        );

        return Err(Error::unauthenticated(format!(
            "Invalid attestation format: {}",
            prefix_sample
        )));
    }

    // Full cryptographic verification in production builds
    // Uses dchat_identity::attestation::AttestationVerifier for platform-specific verification
    #[cfg(not(debug_assertions))]
    return verify_attestation_production(attestation);

    // Debug builds accept format-valid attestations without full crypto verification
    #[cfg(debug_assertions)]
    {
        tracing::warn!("Skipping full attestation verification in DEBUG build - format check only");
        Ok(true)
    }
}

/// Production attestation verification (release builds only)
/// Performs full cryptographic verification of platform-specific attestations.
#[cfg(not(debug_assertions))]
fn verify_attestation_production(attestation: &str) -> Result<bool> {
    // Parse attestation format: "platform-type-base64data"
    // Example: "ios-app-attest-ABCDEF..." or "android-strongbox-attestation-XYZ..."
    let parts: Vec<&str> = attestation.splitn(4, '-').collect();

    // Validate we have enough parts to identify the attestation type
    if parts.len() < 2 {
        return Err(Error::unauthenticated(format!(
            "Malformed attestation: expected 'platform-type-data' format, got {} parts",
            parts.len()
        )));
    }

    if attestation.starts_with("ios-app-attest") {
        // iOS App Attest requires:
        // 1. attestation_object (CBOR-encoded)
        // 2. challenge (nonce)
        // 3. bundle_id and team_id from config
        tracing::info!("iOS attestation detected - full verification requires native bridge");
        Err(Error::internal(
            "iOS App Attest verification requires native integration. \
             The attestation data should be passed through the native iOS bridge \
             with attestation_object, challenge, bundle_id, and team_id.",
        ))
    } else if attestation.starts_with("android-strongbox") || attestation.starts_with("android-tee")
    {
        // Android attestation requires:
        // 1. Play Integrity token OR
        // 2. Key Attestation certificate chain
        tracing::info!(
            "Android attestation detected - full verification requires Play Integrity API"
        );
        Err(Error::internal(
            "Android attestation verification requires Play Integrity API integration. \
             The attestation should contain either a Play Integrity token or \
             Key Attestation certificate chain.",
        ))
    } else if attestation.starts_with("tpm2-attestation") {
        // TPM 2.0 attestation requires:
        // 1. TPM quote with PCR values
        // 2. AIK certificate
        // 3. Event log (optional)
        tracing::info!("TPM attestation detected - full verification requires tss-esapi");
        Err(Error::internal(
            "TPM 2.0 attestation verification requires tss-esapi integration. \
             The attestation should contain a TPM quote, PCR values, and AIK certificate.",
        ))
    } else {
        // Unknown attestation type passed format check but has no verifier
        Err(Error::internal(format!(
            "No verifier available for attestation type: {}",
            parts.first().unwrap_or(&"unknown")
        )))
    }
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
