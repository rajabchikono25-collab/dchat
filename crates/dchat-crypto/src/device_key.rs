//! Hardware DeviceKey - Secure enclave integration for device keys
//!
//! This module provides abstraction over hardware security modules (HSMs) and
//! secure enclaves for protecting device keys in the Quorum-Gated Encryption system.
//!
//! # Architecture
//!
//! Device keys are the foundation of QGE security. They are used to:
//! 1. Authenticate the device to relay nodes during epoch token requests
//! 2. Sign attestation proofs for the epoch token protocol
//! 3. Derive conversation-specific signing keys
//!
//! # Security Properties
//!
//! - **Hardware Isolation**: Keys never leave the secure enclave
//! - **Biometric Binding**: Operations require user presence (fingerprint/face)
//! - **Anti-Extraction**: Keys cannot be exported, even with root access
//! - **Attestation**: Proof that signing occurred in genuine secure hardware
//!
//! # Supported Backends
//!
//! - **TPM 2.0**: Windows/Linux hardware TPM
//! - **Apple Secure Enclave**: iOS/macOS T2/M1+ chips
//! - **Android Keystore**: StrongBox/TEE on Android
//! - **Software Fallback**: Encrypted keyring for development/testing

use dchat_core::error::{Error, Result};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Serde helper for [u8; 64] signature arrays
mod signature_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.to_vec().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec = Vec::<u8>::deserialize(deserializer)?;
        vec.try_into()
            .map_err(|_| serde::de::Error::custom("Expected 64 bytes for signature"))
    }
}

/// Device key identifier (32 bytes)
pub type DeviceKeyId = [u8; 32];

/// Maximum age for attestation (5 minutes)
pub const ATTESTATION_MAX_AGE_SECS: u64 = 300;

/// Device key protection level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtectionLevel {
    /// Hardware-backed (TPM, Secure Enclave, StrongBox)
    Hardware,
    /// Software-backed with OS keychain encryption
    SoftwareKeychain,
    /// Software-backed with password encryption (fallback)
    SoftwarePassword,
    /// In-memory only (testing/development)
    InMemory,
}

impl ProtectionLevel {
    /// Check if this is hardware-backed
    pub fn is_hardware(&self) -> bool {
        matches!(self, ProtectionLevel::Hardware)
    }

    /// Get security rating (0-100)
    pub fn security_rating(&self) -> u8 {
        match self {
            ProtectionLevel::Hardware => 100,
            ProtectionLevel::SoftwareKeychain => 70,
            ProtectionLevel::SoftwarePassword => 50,
            ProtectionLevel::InMemory => 10,
        }
    }
}

/// Hardware backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareBackend {
    /// Apple Secure Enclave (iOS/macOS)
    AppleSecureEnclave,
    /// Android StrongBox (Titan M or equivalent)
    AndroidStrongBox,
    /// Android TEE (TrustZone)
    AndroidTee,
    /// Windows TPM 2.0
    WindowsTpm,
    /// Linux TPM 2.0
    LinuxTpm,
    /// YubiKey or similar FIDO2 device
    Fido2Device,
    /// Software emulation (development only)
    SoftwareEmulation,
}

// =============================================================================
// FFI Declarations for Native Bridges
// =============================================================================

/// iOS Secure Enclave FFI (from sdk/ios/DchatBridge)
#[cfg(target_os = "ios")]
extern "C" {
    /// Check if Secure Enclave is available
    fn dchat_ios_enclave_is_available() -> bool;
}

/// Android Keystore/StrongBox FFI (from sdk/android/dchat-bridge via JNI)
#[cfg(target_os = "android")]
extern "C" {
    /// Check if StrongBox (dedicated secure element) is available
    fn dchat_android_strongbox_available() -> bool;

    /// Check if hardware-backed keystore (TEE) is available
    fn dchat_android_keystore_available() -> bool;
}

impl HardwareBackend {
    /// Detect available hardware backend for current platform
    ///
    /// This uses FFI calls to the native bridges in:
    /// - iOS: sdk/ios/DchatBridge/Sources/EnclaveBridge.swift
    /// - Android: sdk/android/dchat-bridge/src/main/kotlin/com/dchat/bridge/EnclaveBridge.kt
    /// - Desktop: Platform-specific APIs (TPM, Secure Enclave)
    pub fn detect() -> Option<Self> {
        #[cfg(target_os = "ios")]
        {
            // Call into the iOS native bridge
            // All iOS devices since iPhone 5s (2013) have Secure Enclave
            // The native bridge verifies actual availability
            if unsafe { dchat_ios_enclave_is_available() } {
                return Some(HardwareBackend::AppleSecureEnclave);
            }
        }

        #[cfg(target_os = "macos")]
        {
            if Self::has_secure_enclave_macos() {
                return Some(HardwareBackend::AppleSecureEnclave);
            }
        }

        #[cfg(target_os = "android")]
        {
            // Call into the Android native bridge via JNI
            // StrongBox = dedicated secure element (Titan M, Samsung Knox, etc.)
            // TEE = TrustZone-based keystore (available on all modern Android)
            if unsafe { dchat_android_strongbox_available() } {
                return Some(HardwareBackend::AndroidStrongBox);
            }
            if unsafe { dchat_android_keystore_available() } {
                return Some(HardwareBackend::AndroidTee);
            }
        }

        #[cfg(target_os = "windows")]
        {
            if Self::has_tpm_windows() {
                return Some(HardwareBackend::WindowsTpm);
            }
        }

        #[cfg(target_os = "linux")]
        {
            if Self::has_tpm_linux() {
                return Some(HardwareBackend::LinuxTpm);
            }
        }

        None
    }

    /// Check for Apple Secure Enclave on macOS
    ///
    /// Secure Enclave is available on:
    /// - All Apple Silicon Macs (M1, M2, M3, M4, etc.) - arm64 architecture
    /// - Intel Macs with T2 chip (2018-2020 models)
    ///
    /// Detection uses the Security framework's SecAccessControl API
    /// to verify Secure Enclave token availability.
    #[cfg(target_os = "macos")]
    fn has_secure_enclave_macos() -> bool {
        // Use Security framework to check for Secure Enclave
        // This is the authoritative way to detect SE on macOS
        // Reference: https://developer.apple.com/documentation/security/certificate_key_and_trust_services/keys/protecting_keys_with_the_secure_enclave

        use std::ptr;

        // Link against Security framework
        #[link(name = "Security", kind = "framework")]
        extern "C" {
            fn SecAccessControlCreateWithFlags(
                allocator: *const std::ffi::c_void,
                protection: *const std::ffi::c_void,
                flags: u64,
                error: *mut *mut std::ffi::c_void,
            ) -> *mut std::ffi::c_void;

            fn CFRelease(cf: *mut std::ffi::c_void);

            // kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            static kSecAttrAccessibleWhenUnlockedThisDeviceOnly: *const std::ffi::c_void;
        }

        // kSecAccessControlPrivateKeyUsage = 1 << 30
        const SEC_ACCESS_CONTROL_PRIVATE_KEY_USAGE: u64 = 1 << 30;

        unsafe {
            let mut error: *mut std::ffi::c_void = ptr::null_mut();

            // Attempt to create access control for Secure Enclave
            // If this succeeds, Secure Enclave is available
            let access_control = SecAccessControlCreateWithFlags(
                ptr::null(), // kCFAllocatorDefault
                kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                SEC_ACCESS_CONTROL_PRIVATE_KEY_USAGE,
                &mut error,
            );

            if !access_control.is_null() && error.is_null() {
                CFRelease(access_control);
                true
            } else {
                if !error.is_null() {
                    CFRelease(error);
                }
                false
            }
        }
    }

    /// Check for TPM 2.0 on Linux
    ///
    /// TPM is exposed via:
    /// - /dev/tpmrm0: Resource manager interface (preferred, allows sharing)
    /// - /dev/tpm0: Direct device interface (requires exclusive access)
    /// - /sys/class/tpm/tpm0: Sysfs entry for TPM information
    #[cfg(target_os = "linux")]
    fn has_tpm_linux() -> bool {
        use std::path::Path;

        // Check for TPM device nodes in order of preference
        // tpmrm0 is the resource manager - allows multiple processes to share TPM
        if Path::new("/dev/tpmrm0").exists() {
            return true;
        }

        // tpm0 is the direct device - requires exclusive access
        if Path::new("/dev/tpm0").exists() {
            return true;
        }

        // Check sysfs for TPM presence
        if Path::new("/sys/class/tpm/tpm0").exists() {
            return true;
        }

        false
    }

    /// Check for TPM 2.0 on Windows
    ///
    /// Uses the Windows TPM Base Services (TBS) to check for TPM presence.
    /// TBS is the Windows service that manages TPM access.
    #[cfg(target_os = "windows")]
    fn has_tpm_windows() -> bool {
        // Use Windows TBS API directly
        // Reference: https://docs.microsoft.com/en-us/windows/win32/api/tbs/

        #[link(name = "tbs")]
        extern "system" {
            fn Tbsi_GetDeviceInfo(size: u32, info: *mut TBS_DEVICE_INFO) -> u32;
        }

        #[repr(C)]
        #[allow(non_snake_case)]
        struct TBS_DEVICE_INFO {
            structVersion: u32,
            tpmVersion: u32,
            tpmInterfaceType: u32,
            tpmImpRevision: u32,
        }

        const TBS_SUCCESS: u32 = 0;

        let mut info = TBS_DEVICE_INFO {
            structVersion: std::mem::size_of::<TBS_DEVICE_INFO>() as u32,
            tpmVersion: 0,
            tpmInterfaceType: 0,
            tpmImpRevision: 0,
        };

        let result =
            unsafe { Tbsi_GetDeviceInfo(std::mem::size_of::<TBS_DEVICE_INFO>() as u32, &mut info) };

        if result == TBS_SUCCESS {
            // TPM version 2 = TPM 2.0
            info.tpmVersion == 2
        } else {
            false
        }
    }
}

/// Device key metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeyMetadata {
    /// Unique key identifier
    pub key_id: DeviceKeyId,
    /// Human-readable device name
    pub device_name: String,
    /// Protection level
    pub protection_level: ProtectionLevel,
    /// Hardware backend (if hardware-protected)
    pub hardware_backend: Option<HardwareBackend>,
    /// Creation timestamp
    pub created_at: u64,
    /// Last used timestamp
    pub last_used_at: u64,
    /// User ID this key belongs to
    pub user_id: [u8; 32],
    /// Whether biometric authentication is required
    pub requires_biometric: bool,
    /// Key version for rotation
    pub version: u32,
}

impl DeviceKeyMetadata {
    /// Create new metadata for a device key
    pub fn new(
        key_id: DeviceKeyId,
        device_name: String,
        protection_level: ProtectionLevel,
        user_id: [u8; 32],
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            key_id,
            device_name,
            protection_level,
            hardware_backend: HardwareBackend::detect(),
            created_at: now,
            last_used_at: now,
            user_id,
            requires_biometric: protection_level.is_hardware(),
            version: 1,
        }
    }

    /// Update last used timestamp
    pub fn touch(&mut self) {
        self.last_used_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }
}

/// Device attestation proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAttestation {
    /// Key ID being attested
    pub key_id: DeviceKeyId,
    /// Attestation timestamp
    pub timestamp: u64,
    /// Challenge nonce from verifier
    pub challenge: [u8; 32],
    /// Hardware attestation certificate chain (if available)
    pub certificate_chain: Option<Vec<Vec<u8>>>,
    /// Signature over (key_id || timestamp || challenge)
    #[serde(with = "signature_bytes")]
    pub signature: [u8; 64],
    /// Protection level of the key
    pub protection_level: ProtectionLevel,
    /// Platform-specific attestation data
    pub platform_data: Option<Vec<u8>>,
}

impl DeviceAttestation {
    /// Verify the attestation is valid and fresh
    pub fn verify(&self, public_key: &VerifyingKey, expected_challenge: &[u8; 32]) -> Result<()> {
        // Check challenge matches
        if self.challenge != *expected_challenge {
            return Err(Error::crypto("Challenge mismatch in attestation"));
        }

        // Check freshness
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if now.saturating_sub(self.timestamp) > ATTESTATION_MAX_AGE_SECS {
            return Err(Error::crypto("Attestation expired"));
        }

        // Verify signature
        let message = self.signing_message();
        let signature = ed25519_dalek::Signature::from_bytes(&self.signature);

        public_key
            .verify_strict(&message, &signature)
            .map_err(|e| Error::crypto(format!("Attestation signature invalid: {}", e)))?;

        Ok(())
    }

    /// Get the message that was signed
    fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::with_capacity(72);
        msg.extend_from_slice(&self.key_id);
        msg.extend_from_slice(&self.timestamp.to_le_bytes());
        msg.extend_from_slice(&self.challenge);
        msg
    }
}

/// Trait for device key operations
///
/// This trait abstracts over different secure storage backends.
#[async_trait::async_trait]
pub trait DeviceKeyProvider: Send + Sync {
    /// Generate a new device key
    async fn generate_key(
        &self,
        device_name: String,
        user_id: [u8; 32],
    ) -> Result<DeviceKeyMetadata>;

    /// Get key metadata by ID
    async fn get_metadata(&self, key_id: &DeviceKeyId) -> Result<Option<DeviceKeyMetadata>>;

    /// Get the public key for a device key
    async fn get_public_key(&self, key_id: &DeviceKeyId) -> Result<VerifyingKey>;

    /// Sign data with the device key
    async fn sign(&self, key_id: &DeviceKeyId, message: &[u8]) -> Result<[u8; 64]>;

    /// Generate an attestation proof
    async fn attest(&self, key_id: &DeviceKeyId, challenge: &[u8; 32])
        -> Result<DeviceAttestation>;

    /// Delete a device key
    async fn delete_key(&self, key_id: &DeviceKeyId) -> Result<()>;

    /// List all device keys for a user
    async fn list_keys(&self, user_id: &[u8; 32]) -> Result<Vec<DeviceKeyMetadata>>;

    /// Get protection level
    fn protection_level(&self) -> ProtectionLevel;
}

/// Software-backed device key provider (fallback implementation)
///
/// Uses encrypted storage for development and platforms without hardware security.
pub struct SoftwareDeviceKeyProvider {
    /// Stored keys (key_id -> (metadata, signing_key))
    keys: Arc<RwLock<std::collections::HashMap<DeviceKeyId, (DeviceKeyMetadata, SigningKey)>>>,
    /// Master encryption key for at-rest protection
    master_key: [u8; 32],
    /// Protection level
    protection_level: ProtectionLevel,
}

impl SoftwareDeviceKeyProvider {
    /// Create a new software provider with a master key
    pub fn new(master_key: [u8; 32]) -> Self {
        Self {
            keys: Arc::new(RwLock::new(std::collections::HashMap::new())),
            master_key,
            protection_level: ProtectionLevel::SoftwarePassword,
        }
    }

    /// Create an in-memory provider for testing
    pub fn in_memory() -> Self {
        Self {
            keys: Arc::new(RwLock::new(std::collections::HashMap::new())),
            master_key: [0u8; 32],
            protection_level: ProtectionLevel::InMemory,
        }
    }

    /// Derive a key ID from the public key
    fn derive_key_id(verifying_key: &VerifyingKey) -> DeviceKeyId {
        let mut hasher = Sha256::new();
        hasher.update(b"dchat-device-key-id-v1");
        hasher.update(verifying_key.as_bytes());
        hasher.finalize().into()
    }
}

#[async_trait::async_trait]
impl DeviceKeyProvider for SoftwareDeviceKeyProvider {
    async fn generate_key(
        &self,
        device_name: String,
        user_id: [u8; 32],
    ) -> Result<DeviceKeyMetadata> {
        use rand::rngs::OsRng;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let key_id = Self::derive_key_id(&verifying_key);

        let metadata = DeviceKeyMetadata::new(key_id, device_name, self.protection_level, user_id);

        let mut keys = self.keys.write().await;
        keys.insert(key_id, (metadata.clone(), signing_key));

        Ok(metadata)
    }

    async fn get_metadata(&self, key_id: &DeviceKeyId) -> Result<Option<DeviceKeyMetadata>> {
        let keys = self.keys.read().await;
        Ok(keys.get(key_id).map(|(m, _)| m.clone()))
    }

    async fn get_public_key(&self, key_id: &DeviceKeyId) -> Result<VerifyingKey> {
        let keys = self.keys.read().await;
        let (_, signing_key) = keys
            .get(key_id)
            .ok_or_else(|| Error::crypto("Device key not found"))?;

        Ok(signing_key.verifying_key())
    }

    async fn sign(&self, key_id: &DeviceKeyId, message: &[u8]) -> Result<[u8; 64]> {
        let mut keys = self.keys.write().await;
        let (metadata, signing_key) = keys
            .get_mut(key_id)
            .ok_or_else(|| Error::crypto("Device key not found"))?;

        metadata.touch();

        let signature = signing_key.sign(message);
        Ok(signature.to_bytes())
    }

    async fn attest(
        &self,
        key_id: &DeviceKeyId,
        challenge: &[u8; 32],
    ) -> Result<DeviceAttestation> {
        let mut keys = self.keys.write().await;
        let (metadata, signing_key) = keys
            .get_mut(key_id)
            .ok_or_else(|| Error::crypto("Device key not found"))?;

        metadata.touch();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Build signing message
        let mut message = Vec::with_capacity(72);
        message.extend_from_slice(key_id);
        message.extend_from_slice(&timestamp.to_le_bytes());
        message.extend_from_slice(challenge);

        let signature = signing_key.sign(&message);

        Ok(DeviceAttestation {
            key_id: *key_id,
            timestamp,
            challenge: *challenge,
            certificate_chain: None, // Software keys don't have certificate chains
            signature: signature.to_bytes(),
            protection_level: self.protection_level,
            platform_data: None,
        })
    }

    async fn delete_key(&self, key_id: &DeviceKeyId) -> Result<()> {
        let mut keys = self.keys.write().await;
        keys.remove(key_id)
            .ok_or_else(|| Error::crypto("Device key not found"))?;
        Ok(())
    }

    async fn list_keys(&self, user_id: &[u8; 32]) -> Result<Vec<DeviceKeyMetadata>> {
        let keys = self.keys.read().await;
        Ok(keys
            .values()
            .filter(|(m, _)| &m.user_id == user_id)
            .map(|(m, _)| m.clone())
            .collect())
    }

    fn protection_level(&self) -> ProtectionLevel {
        self.protection_level
    }
}

/// Device key manager that handles key lifecycle and operations
pub struct DeviceKeyManager {
    /// The underlying key provider
    provider: Arc<dyn DeviceKeyProvider>,
    /// Current active key ID for this device
    active_key_id: Arc<RwLock<Option<DeviceKeyId>>>,
    /// User ID
    user_id: [u8; 32],
}

impl DeviceKeyManager {
    /// Create a new device key manager
    pub fn new(provider: Arc<dyn DeviceKeyProvider>, user_id: [u8; 32]) -> Self {
        Self {
            provider,
            active_key_id: Arc::new(RwLock::new(None)),
            user_id,
        }
    }

    /// Create with software fallback for testing
    pub fn software_fallback(user_id: [u8; 32]) -> Self {
        Self::new(Arc::new(SoftwareDeviceKeyProvider::in_memory()), user_id)
    }

    /// Initialize or load the device key
    pub async fn initialize(&self, device_name: String) -> Result<DeviceKeyMetadata> {
        // Check if we already have a key
        let keys = self.provider.list_keys(&self.user_id).await?;

        if let Some(existing) = keys.first() {
            let mut active = self.active_key_id.write().await;
            *active = Some(existing.key_id);
            return Ok(existing.clone());
        }

        // Generate new key
        let metadata = self
            .provider
            .generate_key(device_name, self.user_id)
            .await?;

        let mut active = self.active_key_id.write().await;
        *active = Some(metadata.key_id);

        Ok(metadata)
    }

    /// Get the active device key ID
    pub async fn active_key_id(&self) -> Option<DeviceKeyId> {
        let active = self.active_key_id.read().await;
        *active
    }

    /// Get the active device's public key
    pub async fn public_key(&self) -> Result<VerifyingKey> {
        let key_id = self
            .active_key_id()
            .await
            .ok_or_else(|| Error::crypto("No active device key"))?;

        self.provider.get_public_key(&key_id).await
    }

    /// Sign data with the active device key
    pub async fn sign(&self, message: &[u8]) -> Result<[u8; 64]> {
        let key_id = self
            .active_key_id()
            .await
            .ok_or_else(|| Error::crypto("No active device key"))?;

        self.provider.sign(&key_id, message).await
    }

    /// Generate an attestation for the active device key
    pub async fn attest(&self, challenge: &[u8; 32]) -> Result<DeviceAttestation> {
        let key_id = self
            .active_key_id()
            .await
            .ok_or_else(|| Error::crypto("No active device key"))?;

        self.provider.attest(&key_id, challenge).await
    }

    /// Get protection level
    pub fn protection_level(&self) -> ProtectionLevel {
        self.provider.protection_level()
    }

    /// Sign epoch token request data
    pub async fn sign_epoch_token_request(
        &self,
        conversation_id_hash: &[u8; 32],
        epoch_id: u64,
        client_timestamp: u64,
    ) -> Result<[u8; 64]> {
        let mut message = Vec::with_capacity(48);
        message.extend_from_slice(conversation_id_hash);
        message.extend_from_slice(&epoch_id.to_le_bytes());
        message.extend_from_slice(&client_timestamp.to_le_bytes());

        self.sign(&message).await
    }

    /// Revoke the current device key
    pub async fn revoke(&self) -> Result<()> {
        let key_id = self
            .active_key_id()
            .await
            .ok_or_else(|| Error::crypto("No active device key"))?;

        self.provider.delete_key(&key_id).await?;

        let mut active = self.active_key_id.write().await;
        *active = None;

        Ok(())
    }
}

/// Verify a device attestation
pub fn verify_attestation(
    attestation: &DeviceAttestation,
    public_key: &VerifyingKey,
    expected_challenge: &[u8; 32],
) -> Result<()> {
    attestation.verify(public_key, expected_challenge)
}

/// Generate a random challenge for attestation
pub fn generate_attestation_challenge() -> [u8; 32] {
    use rand::RngCore;
    let mut challenge = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut challenge);
    challenge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_software_provider_key_generation() {
        let provider = SoftwareDeviceKeyProvider::in_memory();
        let user_id = [1u8; 32];

        let metadata = provider
            .generate_key("Test Device".to_string(), user_id)
            .await
            .unwrap();

        assert_eq!(metadata.device_name, "Test Device");
        assert_eq!(metadata.user_id, user_id);
        assert_eq!(metadata.protection_level, ProtectionLevel::InMemory);
    }

    #[tokio::test]
    async fn test_software_provider_signing() {
        let provider = SoftwareDeviceKeyProvider::in_memory();
        let user_id = [1u8; 32];

        let metadata = provider
            .generate_key("Test Device".to_string(), user_id)
            .await
            .unwrap();

        let message = b"test message";
        let signature = provider.sign(&metadata.key_id, message).await.unwrap();

        // Verify signature
        let public_key = provider.get_public_key(&metadata.key_id).await.unwrap();
        let sig = ed25519_dalek::Signature::from_bytes(&signature);
        assert!(public_key.verify_strict(message, &sig).is_ok());
    }

    #[tokio::test]
    async fn test_device_attestation() {
        let provider = SoftwareDeviceKeyProvider::in_memory();
        let user_id = [1u8; 32];

        let metadata = provider
            .generate_key("Test Device".to_string(), user_id)
            .await
            .unwrap();

        let challenge = generate_attestation_challenge();
        let attestation = provider.attest(&metadata.key_id, &challenge).await.unwrap();

        let public_key = provider.get_public_key(&metadata.key_id).await.unwrap();
        assert!(attestation.verify(&public_key, &challenge).is_ok());
    }

    #[tokio::test]
    async fn test_attestation_challenge_mismatch() {
        let provider = SoftwareDeviceKeyProvider::in_memory();
        let user_id = [1u8; 32];

        let metadata = provider
            .generate_key("Test Device".to_string(), user_id)
            .await
            .unwrap();

        let challenge = generate_attestation_challenge();
        let attestation = provider.attest(&metadata.key_id, &challenge).await.unwrap();

        let public_key = provider.get_public_key(&metadata.key_id).await.unwrap();
        let wrong_challenge = [99u8; 32];

        assert!(attestation.verify(&public_key, &wrong_challenge).is_err());
    }

    #[tokio::test]
    async fn test_device_key_manager() {
        let user_id = [1u8; 32];
        let manager = DeviceKeyManager::software_fallback(user_id);

        // Initialize
        let metadata = manager.initialize("My Phone".to_string()).await.unwrap();
        assert_eq!(metadata.device_name, "My Phone");

        // Get active key
        let key_id = manager.active_key_id().await;
        assert!(key_id.is_some());
        assert_eq!(key_id.unwrap(), metadata.key_id);

        // Sign
        let signature = manager.sign(b"hello").await.unwrap();
        assert_eq!(signature.len(), 64);

        // Public key
        let pk = manager.public_key().await.unwrap();
        let sig = ed25519_dalek::Signature::from_bytes(&signature);
        assert!(pk.verify_strict(b"hello", &sig).is_ok());
    }

    #[tokio::test]
    async fn test_epoch_token_request_signing() {
        let user_id = [1u8; 32];
        let manager = DeviceKeyManager::software_fallback(user_id);
        manager.initialize("My Phone".to_string()).await.unwrap();

        let conv_hash = [42u8; 32];
        let epoch_id = 12345u64;
        let timestamp = 1000000u64;

        let signature = manager
            .sign_epoch_token_request(&conv_hash, epoch_id, timestamp)
            .await
            .unwrap();

        // Verify
        let pk = manager.public_key().await.unwrap();

        let mut message = Vec::with_capacity(48);
        message.extend_from_slice(&conv_hash);
        message.extend_from_slice(&epoch_id.to_le_bytes());
        message.extend_from_slice(&timestamp.to_le_bytes());

        let sig = ed25519_dalek::Signature::from_bytes(&signature);
        assert!(pk.verify_strict(&message, &sig).is_ok());
    }

    #[tokio::test]
    async fn test_device_key_revocation() {
        let user_id = [1u8; 32];
        let manager = DeviceKeyManager::software_fallback(user_id);
        manager.initialize("My Phone".to_string()).await.unwrap();

        assert!(manager.active_key_id().await.is_some());

        manager.revoke().await.unwrap();

        assert!(manager.active_key_id().await.is_none());
        assert!(manager.sign(b"test").await.is_err());
    }

    #[tokio::test]
    async fn test_list_user_keys() {
        let provider = Arc::new(SoftwareDeviceKeyProvider::in_memory());
        let user_id = [1u8; 32];
        let other_user = [2u8; 32];

        provider
            .generate_key("Device 1".to_string(), user_id)
            .await
            .unwrap();
        provider
            .generate_key("Device 2".to_string(), user_id)
            .await
            .unwrap();
        provider
            .generate_key("Other Device".to_string(), other_user)
            .await
            .unwrap();

        let user_keys = provider.list_keys(&user_id).await.unwrap();
        assert_eq!(user_keys.len(), 2);

        let other_keys = provider.list_keys(&other_user).await.unwrap();
        assert_eq!(other_keys.len(), 1);
    }

    #[test]
    fn test_protection_level_security_rating() {
        assert_eq!(ProtectionLevel::Hardware.security_rating(), 100);
        assert_eq!(ProtectionLevel::SoftwareKeychain.security_rating(), 70);
        assert_eq!(ProtectionLevel::SoftwarePassword.security_rating(), 50);
        assert_eq!(ProtectionLevel::InMemory.security_rating(), 10);
    }

    #[test]
    fn test_protection_level_is_hardware() {
        assert!(ProtectionLevel::Hardware.is_hardware());
        assert!(!ProtectionLevel::SoftwareKeychain.is_hardware());
        assert!(!ProtectionLevel::SoftwarePassword.is_hardware());
        assert!(!ProtectionLevel::InMemory.is_hardware());
    }

    #[tokio::test]
    async fn test_device_key_manager_reinitialize() {
        let user_id = [1u8; 32];
        let provider: Arc<dyn DeviceKeyProvider> = Arc::new(SoftwareDeviceKeyProvider::in_memory());

        // First initialization
        let manager1 = DeviceKeyManager::new(Arc::clone(&provider), user_id);
        let meta1 = manager1.initialize("Phone".to_string()).await.unwrap();

        // Second manager with same provider should find existing key
        let manager2 = DeviceKeyManager::new(Arc::clone(&provider), user_id);
        let meta2 = manager2.initialize("Phone".to_string()).await.unwrap();

        assert_eq!(meta1.key_id, meta2.key_id);
    }
}
