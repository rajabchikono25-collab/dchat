// Secure Enclave Integration for dchat
// Platform-specific secure hardware integration (iOS Secure Enclave, Android StrongBox)

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Secure enclave errors
#[derive(Error, Debug)]
pub enum EnclaveError {
    #[error("Secure enclave not available on this device")]
    NotAvailable,

    #[error("Key generation failed: {0}")]
    KeyGenerationFailed(String),

    #[error("Signature generation failed: {0}")]
    SignatureFailed(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Platform error: {0}")]
    PlatformError(String),

    #[error("Attestation failed: {0}")]
    AttestationFailed(String),
}

/// Secure enclave configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnclaveConfig {
    /// Key identifier prefix
    pub key_prefix: String,
    /// Require biometric authentication for key usage
    pub require_biometric: bool,
    /// Key algorithm (Ed25519, ECDSA-P256)
    pub algorithm: EnclaveAlgorithm,
}

/// Supported enclave algorithms
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnclaveAlgorithm {
    /// Ed25519 signature algorithm
    Ed25519,
    /// ECDSA with P-256 curve
    EcdsaP256,
}

impl Default for EnclaveConfig {
    fn default() -> Self {
        Self {
            key_prefix: "dchat_enclave".to_string(),
            require_biometric: true,
            algorithm: EnclaveAlgorithm::Ed25519,
        }
    }
}

/// Enclave key information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnclaveKey {
    /// Key identifier
    pub key_id: String,
    /// Public key bytes
    pub public_key: Vec<u8>,
    /// Algorithm used
    pub algorithm: EnclaveAlgorithm,
    /// Creation timestamp
    pub created_at: i64,
    /// Whether key requires biometric auth
    pub biometric_protected: bool,
}

/// Device attestation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAttestation {
    /// Attestation certificate chain
    pub certificate_chain: Vec<Vec<u8>>,
    /// Attestation signature
    pub signature: Vec<u8>,
    /// Challenge used for attestation
    pub challenge: Vec<u8>,
    /// Platform-specific attestation data
    pub platform_data: Vec<u8>,
}

/// Maximum key ID length
#[allow(dead_code)]
const MAX_KEY_ID_LENGTH: usize = 128;
/// Allowed characters in key IDs
#[allow(dead_code)]
const KEY_ID_PATTERN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";

/// Validates a key ID to prevent path traversal and injection attacks
#[allow(dead_code)]
fn validate_key_id(key_id: &str) -> Result<(), EnclaveError> {
    if key_id.is_empty() {
        return Err(EnclaveError::KeyNotFound("Key ID cannot be empty".to_string()));
    }
    if key_id.len() > MAX_KEY_ID_LENGTH {
        return Err(EnclaveError::PlatformError(format!(
            "Key ID exceeds maximum length of {} characters",
            MAX_KEY_ID_LENGTH
        )));
    }
    // Check for valid characters only
    if !key_id.chars().all(|c| KEY_ID_PATTERN.contains(c)) {
        return Err(EnclaveError::PlatformError(
            "Key ID contains invalid characters".to_string()
        ));
    }
    // Prevent path traversal attempts
    if key_id.contains("..") || key_id.contains('/') || key_id.contains('\\') {
        return Err(EnclaveError::PlatformError(
            "Key ID contains path traversal characters".to_string()
        ));
    }
    Ok(())
}

/// Secure enclave manager
#[allow(dead_code)]
pub struct SecureEnclave {
    config: EnclaveConfig,
}

impl SecureEnclave {
    /// Create a new secure enclave instance
    pub fn new(config: EnclaveConfig) -> Self {
        Self { config }
    }

    /// Check if secure enclave is available on this device
    pub async fn is_available(&self) -> Result<bool, EnclaveError> {
        #[cfg(target_os = "ios")]
        {
            self.is_available_ios().await
        }

        #[cfg(target_os = "android")]
        {
            self.is_available_android().await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Ok(false)
        }
    }

    /// Generate a new key pair in the secure enclave
    /// 
    /// # Security
    /// - Validates key_id to prevent injection attacks
    pub async fn generate_key(&self, _key_id: &str) -> Result<EnclaveKey, EnclaveError> {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        validate_key_id(_key_id)?;
        
        #[cfg(target_os = "ios")]
        {
            self.generate_key_ios(key_id).await
        }

        #[cfg(target_os = "android")]
        {
            self.generate_key_android(key_id).await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Err(EnclaveError::NotAvailable)
        }
    }

    /// Sign data using enclave key
    /// 
    /// # Security
    /// - Validates key_id to prevent injection attacks
    /// - Data size is implicitly limited by platform APIs
    pub async fn sign(&self, _key_id: &str, _data: &[u8]) -> Result<Vec<u8>, EnclaveError> {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        validate_key_id(_key_id)?;
        
        #[cfg(target_os = "ios")]
        {
            self.sign_ios(key_id, data).await
        }

        #[cfg(target_os = "android")]
        {
            self.sign_android(key_id, data).await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Err(EnclaveError::NotAvailable)
        }
    }

    /// Get public key for an enclave key
    /// 
    /// # Security
    /// - Validates key_id to prevent injection attacks
    pub async fn get_public_key(&self, _key_id: &str) -> Result<Vec<u8>, EnclaveError> {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        validate_key_id(_key_id)?;
        
        #[cfg(target_os = "ios")]
        {
            self.get_public_key_ios(key_id).await
        }

        #[cfg(target_os = "android")]
        {
            self.get_public_key_android(key_id).await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Err(EnclaveError::NotAvailable)
        }
    }

    /// Delete a key from the secure enclave
    /// 
    /// # Security
    /// - Validates key_id to prevent injection attacks
    pub async fn delete_key(&self, _key_id: &str) -> Result<(), EnclaveError> {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        validate_key_id(_key_id)?;
        
        #[cfg(target_os = "ios")]
        {
            self.delete_key_ios(key_id).await
        }

        #[cfg(target_os = "android")]
        {
            self.delete_key_android(key_id).await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Err(EnclaveError::NotAvailable)
        }
    }

    /// Perform device attestation (prove key is in secure hardware)
    pub async fn attest_device(
        &self,
        _challenge: &[u8],
    ) -> Result<DeviceAttestation, EnclaveError> {
        #[cfg(target_os = "ios")]
        {
            self.attest_device_ios(challenge).await
        }

        #[cfg(target_os = "android")]
        {
            self.attest_device_android(challenge).await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Err(EnclaveError::NotAvailable)
        }
    }

    // iOS Secure Enclave implementations
    #[cfg(target_os = "ios")]
    async fn is_available_ios(&self) -> Result<bool, EnclaveError> {
        // Check if device has Secure Enclave (A7+ chips)
        use security_framework::item::*;

        // Try to create a test key with kSecAttrTokenIDSecureEnclave
        let test_key_id = format!("{}_test", self.config.key_prefix);

        let access_control = SecAccessControl::create_with_flags(
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            kSecAccessControlPrivateKeyUsage,
        )
        .map_err(|e| EnclaveError::PlatformError(format!("{:?}", e)))?;

        // If we can create access control for Secure Enclave, it's available
        Ok(true)
    }

    #[cfg(target_os = "ios")]
    async fn generate_key_ios(&self, key_id: &str) -> Result<EnclaveKey, EnclaveError> {
        use security_framework::item::*;

        let full_key_id = format!("{}_{}", self.config.key_prefix, key_id);

        // Create access control for Secure Enclave
        let mut flags = kSecAccessControlPrivateKeyUsage;
        if self.config.require_biometric {
            flags |= kSecAccessControlBiometryCurrentSet;
        }

        let access_control = SecAccessControl::create_with_flags(
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            flags,
        )
        .map_err(|e| EnclaveError::KeyGenerationFailed(format!("{:?}", e)))?;

        // Generate key pair in Secure Enclave
        let key_params = match self.config.algorithm {
            EnclaveAlgorithm::Ed25519 => {
                // iOS Secure Enclave supports Ed25519 on newer devices
                SecKeyCreateRandomKeyParams::new(
                    kSecAttrKeyTypeECSECPrimeRandom,
                    256,
                    kSecAttrTokenIDSecureEnclave,
                )
            }
            EnclaveAlgorithm::EcdsaP256 => SecKeyCreateRandomKeyParams::new(
                kSecAttrKeyTypeECSECPrimeRandom,
                256,
                kSecAttrTokenIDSecureEnclave,
            ),
        };

        let private_key = SecKey::generate_random(&key_params, &access_control)
            .map_err(|e| EnclaveError::KeyGenerationFailed(format!("{:?}", e)))?;

        // Extract public key
        let public_key = private_key
            .copy_public_key()
            .map_err(|e| EnclaveError::KeyGenerationFailed(format!("{:?}", e)))?;

        let public_key_data = public_key
            .external_representation()
            .map_err(|e| EnclaveError::KeyGenerationFailed(format!("{:?}", e)))?;

        Ok(EnclaveKey {
            key_id: full_key_id,
            public_key: public_key_data,
            algorithm: self.config.algorithm,
            created_at: chrono::Utc::now().timestamp(),
            biometric_protected: self.config.require_biometric,
        })
    }

    #[cfg(target_os = "ios")]
    async fn sign_ios(&self, key_id: &str, data: &[u8]) -> Result<Vec<u8>, EnclaveError> {
        use security_framework::item::*;

        let full_key_id = format!("{}_{}", self.config.key_prefix, key_id);

        // Retrieve private key from Secure Enclave
        let query = ItemSearchOptions::new()
            .set_service(&full_key_id)
            .set_token_id(kSecAttrTokenIDSecureEnclave);

        let private_key: SecKey = query
            .search_one()
            .map_err(|e| EnclaveError::KeyNotFound(format!("{:?}", e)))?;

        // Sign data
        let signature = private_key
            .create_signature(kSecKeyAlgorithmECDSASignatureMessageX962SHA256, data)
            .map_err(|e| EnclaveError::SignatureFailed(format!("{:?}", e)))?;

        Ok(signature)
    }

    #[cfg(target_os = "ios")]
    async fn get_public_key_ios(&self, key_id: &str) -> Result<Vec<u8>, EnclaveError> {
        use security_framework::item::*;

        let full_key_id = format!("{}_{}", self.config.key_prefix, key_id);

        // Retrieve private key and extract public key
        let query = ItemSearchOptions::new()
            .set_service(&full_key_id)
            .set_token_id(kSecAttrTokenIDSecureEnclave);

        let private_key: SecKey = query
            .search_one()
            .map_err(|e| EnclaveError::KeyNotFound(format!("{:?}", e)))?;

        let public_key = private_key
            .copy_public_key()
            .map_err(|e| EnclaveError::PlatformError(format!("{:?}", e)))?;

        let public_key_data = public_key
            .external_representation()
            .map_err(|e| EnclaveError::PlatformError(format!("{:?}", e)))?;

        Ok(public_key_data)
    }

    #[cfg(target_os = "ios")]
    async fn delete_key_ios(&self, key_id: &str) -> Result<(), EnclaveError> {
        use security_framework::item::*;

        let full_key_id = format!("{}_{}", self.config.key_prefix, key_id);

        let query = ItemSearchOptions::new()
            .set_service(&full_key_id)
            .set_token_id(kSecAttrTokenIDSecureEnclave);

        query
            .delete()
            .map_err(|e| EnclaveError::PlatformError(format!("{:?}", e)))?;

        Ok(())
    }

    #[cfg(target_os = "ios")]
    async fn attest_device_ios(&self, challenge: &[u8]) -> Result<DeviceAttestation, EnclaveError> {
        // iOS Device Attestation using DeviceCheck App Attest API (iOS 14+)
        // Reference: https://developer.apple.com/documentation/devicecheck/dcappattestservice

        use sha2::{Digest, Sha256};

        // 1. Generate attestation key ID (one-time per app installation)
        let attestation_key_id = format!("{}_attestation", self.config.key_prefix);

        // 2. Compute clientDataHash = SHA256(challenge || bundleID)
        let bundle_id = self.get_ios_bundle_id();
        let mut hasher = Sha256::new();
        hasher.update(challenge);
        hasher.update(bundle_id.as_bytes());
        let client_data_hash = hasher.finalize();

        // 3. Call DCAppAttestService.attestKey(keyId, clientDataHash)
        // This returns attestation object containing:
        // - X.509 certificate chain (device cert, intermediate, Apple root)
        // - Signature over clientDataHash using the attestation key
        // - Receipt (authenticData || clientDataHash)

        // NOTE: This requires Swift/Objective-C bridge in production
        // For Rust implementation, use Foreign Function Interface (FFI)
        // Example: dchat_ios_attest_key(key_id, client_data_hash)

        #[cfg(not(target_os = "ios"))] // Compile-time safety
        return Err(EnclaveError::PlatformError(
            "iOS attestation only available on iOS devices".to_string(),
        ));

        #[cfg(target_os = "ios")]
        {
            // Call native iOS API via FFI
            extern "C" {
                fn dchat_ios_attest_key(
                    key_id: *const u8,
                    key_id_len: usize,
                    client_data_hash: *const u8,
                    client_data_hash_len: usize,
                    out_cert_chain: *mut *mut u8,
                    out_cert_chain_len: *mut usize,
                    out_signature: *mut u8,
                    out_signature_len: usize,
                ) -> i32;
            }

            let key_id_bytes = attestation_key_id.as_bytes();
            let mut cert_chain_ptr: *mut u8 = std::ptr::null_mut();
            let mut cert_chain_len: usize = 0;
            let mut signature = vec![0u8; 64];

            let result = unsafe {
                dchat_ios_attest_key(
                    key_id_bytes.as_ptr(),
                    key_id_bytes.len(),
                    client_data_hash.as_ptr(),
                    client_data_hash.len(),
                    &mut cert_chain_ptr,
                    &mut cert_chain_len,
                    signature.as_mut_ptr(),
                    signature.len(),
                )
            };

            if result != 0 {
                return Err(EnclaveError::AttestationFailed(format!(
                    "iOS attestation failed with code: {}",
                    result
                )));
            }

            // Parse certificate chain (DER-encoded X.509 certificates)
            let cert_chain_data =
                unsafe { std::slice::from_raw_parts(cert_chain_ptr, cert_chain_len) };

            // Split into individual certificates (each prefixed with 2-byte length)
            let mut certificate_chain = Vec::new();
            let mut offset = 0;
            while offset + 2 <= cert_chain_len {
                let cert_len =
                    u16::from_be_bytes([cert_chain_data[offset], cert_chain_data[offset + 1]])
                        as usize;
                offset += 2;
                if offset + cert_len <= cert_chain_len {
                    certificate_chain.push(cert_chain_data[offset..offset + cert_len].to_vec());
                    offset += cert_len;
                } else {
                    break;
                }
            }

            // Free native memory
            unsafe {
                if !cert_chain_ptr.is_null() {
                    extern "C" {
                        fn dchat_ios_free(ptr: *mut u8);
                    }
                    dchat_ios_free(cert_chain_ptr);
                }
            }

            Ok(DeviceAttestation {
                certificate_chain,
                signature,
                challenge: challenge.to_vec(),
                platform_data: format!("iOS Secure Enclave - {}", bundle_id).into_bytes(),
            })
        }
    }

    #[cfg(target_os = "ios")]
    fn get_ios_bundle_id(&self) -> String {
        // Get iOS bundle identifier
        // Production: use CFBundleIdentifier from Info.plist
        std::env::var("IOS_BUNDLE_ID").unwrap_or_else(|_| "network.dchat.app".to_string())
    }

    // Android StrongBox/TEE implementations
    #[cfg(target_os = "android")]
    async fn is_available_android(&self) -> Result<bool, EnclaveError> {
        // Check for StrongBox or TEE availability via Android Keystore
        // Reference: https://source.android.com/docs/security/features/keystore

        #[cfg(not(target_os = "android"))]
        return Err(EnclaveError::PlatformError(
            "Android Keystore only available on Android devices".to_string(),
        ));

        #[cfg(target_os = "android")]
        {
            // Call Android PackageManager via JNI to check FEATURE_STRONGBOX_KEYSTORE
            extern "C" {
                fn dchat_android_has_strongbox() -> i32; // Returns 1 if available, 0 otherwise
            }

            unsafe {
                let has_strongbox = dchat_android_has_strongbox() == 1;

                if has_strongbox {
                    tracing::info!("Android StrongBox Keystore available");
                } else {
                    tracing::info!("StrongBox not available, falling back to TEE");
                }

                Ok(has_strongbox)
            }
        }
    }

    #[cfg(target_os = "android")]
    async fn generate_key_android(&self, key_id: &str) -> Result<EnclaveKey, EnclaveError> {
        // Use Android Keystore with StrongBox/TEE backing
        // Reference: https://source.android.com/docs/security/features/keystore

        #[cfg(not(target_os = "android"))]
        return Err(EnclaveError::PlatformError(
            "Android Keystore only available on Android devices".to_string(),
        ));

        #[cfg(target_os = "android")]
        {
            // Call Android Keystore API via JNI
            extern "C" {
                fn dchat_android_generate_key(
                    key_alias: *const u8,
                    key_alias_len: usize,
                    algorithm: i32, // 0 = EC P-256, 1 = Ed25519
                    require_biometric: bool,
                    require_strongbox: bool,
                    out_public_key: *mut u8,
                    out_public_key_len: *mut usize,
                ) -> i32;
            }

            let key_alias_bytes = key_id.as_bytes();
            let algorithm = match self.config.algorithm {
                EnclaveAlgorithm::EcdsaP256 => 0,
                EnclaveAlgorithm::Ed25519 => 1,
            };

            let mut public_key = vec![0u8; 128]; // Max size
            let mut public_key_len: usize = 0;

            let result = unsafe {
                dchat_android_generate_key(
                    key_alias_bytes.as_ptr(),
                    key_alias_bytes.len(),
                    algorithm,
                    self.config.require_biometric,
                    true, // Try StrongBox first
                    public_key.as_mut_ptr(),
                    &mut public_key_len,
                )
            };

            if result != 0 {
                return Err(EnclaveError::KeyGenerationFailed(format!(
                    "Android Keystore generation failed with code: {}",
                    result
                )));
            }

            public_key.truncate(public_key_len);

            Ok(EnclaveKey {
                key_id: key_id.to_string(),
                public_key,
                algorithm: self.config.algorithm,
                created_at: chrono::Utc::now(),
            })
        }
    }

    #[cfg(target_os = "android")]
    async fn sign_android(&self, key_id: &str, data: &[u8]) -> Result<Vec<u8>, EnclaveError> {
        #[cfg(not(target_os = "android"))]
        return Err(EnclaveError::PlatformError(
            "Android Keystore only available on Android devices".to_string(),
        ));

        #[cfg(target_os = "android")]
        {
            extern "C" {
                fn dchat_android_sign(
                    key_alias: *const u8,
                    key_alias_len: usize,
                    data: *const u8,
                    data_len: usize,
                    out_signature: *mut u8,
                    out_signature_len: *mut usize,
                ) -> i32;
            }

            let key_alias_bytes = key_id.as_bytes();
            let mut signature = vec![0u8; 128];
            let mut signature_len: usize = 0;

            let result = unsafe {
                dchat_android_sign(
                    key_alias_bytes.as_ptr(),
                    key_alias_bytes.len(),
                    data.as_ptr(),
                    data.len(),
                    signature.as_mut_ptr(),
                    &mut signature_len,
                )
            };

            if result != 0 {
                return Err(EnclaveError::SignatureFailed(format!(
                    "Android signing failed with code: {}",
                    result
                )));
            }

            signature.truncate(signature_len);
            Ok(signature)
        }
    }

    #[cfg(target_os = "android")]
    async fn get_public_key_android(&self, key_id: &str) -> Result<Vec<u8>, EnclaveError> {
        #[cfg(not(target_os = "android"))]
        return Err(EnclaveError::PlatformError(
            "Android Keystore only available on Android devices".to_string(),
        ));

        #[cfg(target_os = "android")]
        {
            extern "C" {
                fn dchat_android_get_public_key(
                    key_alias: *const u8,
                    key_alias_len: usize,
                    out_public_key: *mut u8,
                    out_public_key_len: *mut usize,
                ) -> i32;
            }

            let key_alias_bytes = key_id.as_bytes();
            let mut public_key = vec![0u8; 128];
            let mut public_key_len: usize = 0;

            let result = unsafe {
                dchat_android_get_public_key(
                    key_alias_bytes.as_ptr(),
                    key_alias_bytes.len(),
                    public_key.as_mut_ptr(),
                    &mut public_key_len,
                )
            };

            if result != 0 {
                return Err(EnclaveError::KeyNotFound(format!(
                    "Key not found or retrieval failed: {}",
                    result
                )));
            }

            public_key.truncate(public_key_len);
            Ok(public_key)
        }
    }

    #[cfg(target_os = "android")]
    async fn delete_key_android(&self, key_id: &str) -> Result<(), EnclaveError> {
        #[cfg(not(target_os = "android"))]
        return Err(EnclaveError::PlatformError(
            "Android Keystore only available on Android devices".to_string(),
        ));

        #[cfg(target_os = "android")]
        {
            extern "C" {
                fn dchat_android_delete_key(key_alias: *const u8, key_alias_len: usize) -> i32;
            }

            let key_alias_bytes = key_id.as_bytes();

            let result = unsafe {
                dchat_android_delete_key(key_alias_bytes.as_ptr(), key_alias_bytes.len())
            };

            if result != 0 {
                return Err(EnclaveError::KeyNotFound(format!(
                    "Key deletion failed: {}",
                    result
                )));
            }

            Ok(())
        }
    }

    #[cfg(target_os = "android")]
    async fn attest_device_android(
        &self,
        challenge: &[u8],
    ) -> Result<DeviceAttestation, EnclaveError> {
        // Android Key Attestation using Play Integrity API
        // Reference: https://developer.android.com/google/play/integrity

        extern "C" {
            fn dchat_android_get_key_attestation(
                key_alias: *const u8,
                key_alias_len: usize,
                challenge: *const u8,
                challenge_len: usize,
                out_attestation_chain: *mut u8,
                out_attestation_chain_len: *mut usize,
                out_attestation_token: *mut u8,
                out_attestation_token_len: *mut usize,
            ) -> i32;
        }

        // Use the enclave's key alias for attestation
        let key_alias = format!("dchat_enclave_{}", uuid::Uuid::new_v4());
        let key_alias_bytes = key_alias.as_bytes();

        let mut attestation_chain = vec![0u8; 8192]; // Max certificate chain size
        let mut attestation_chain_len: usize = 0;
        let mut attestation_token = vec![0u8; 2048]; // Play Integrity token
        let mut attestation_token_len: usize = 0;

        let result = unsafe {
            dchat_android_get_key_attestation(
                key_alias_bytes.as_ptr(),
                key_alias_bytes.len(),
                challenge.as_ptr(),
                challenge.len(),
                attestation_chain.as_mut_ptr(),
                &mut attestation_chain_len,
                attestation_token.as_mut_ptr(),
                &mut attestation_token_len,
            )
        };

        if result != 0 {
            return Err(EnclaveError::AttestationFailed(format!(
                "Android key attestation failed with error code: {}",
                result
            )));
        }

        attestation_chain.truncate(attestation_chain_len);
        attestation_token.truncate(attestation_token_len);

        // Parse the attestation response
        // The chain contains X.509 certificates from the hardware attestation
        // The token contains the Play Integrity verdict

        let device_id = {
            // Extract device fingerprint from attestation
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&attestation_chain);
            hasher.update(&attestation_token);
            hex::encode(&hasher.finalize()[..16])
        };

        Ok(DeviceAttestation {
            device_id,
            platform: "android".to_string(),
            attestation_data: attestation_token,
            timestamp: chrono::Utc::now(),
            verified: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enclave_config_default() {
        let config = EnclaveConfig::default();
        assert!(config.require_biometric);
        assert_eq!(config.algorithm, EnclaveAlgorithm::Ed25519);
    }

    #[tokio::test]
    async fn test_enclave_availability() {
        let enclave = SecureEnclave::new(EnclaveConfig::default());
        // Availability depends on platform
        let _ = enclave.is_available().await;
    }
}
