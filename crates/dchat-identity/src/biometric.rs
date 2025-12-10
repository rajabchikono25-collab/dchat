// Biometric Authentication Module for dchat
// Implements platform-agnostic biometric authentication (TouchID, FaceID, Fingerprint)

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

#[cfg(target_os = "android")]
use jni::JNIEnv;
#[cfg(target_os = "ios")]
use security_framework::item::*;

/// Biometric authentication errors
#[derive(Error, Debug)]
pub enum BiometricError {
    #[error("Biometric authentication not available on this device")]
    NotAvailable,

    #[error("User cancelled authentication")]
    UserCancelled,

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("No biometrics enrolled")]
    NoEnrollment,

    #[error("Platform error: {0}")]
    PlatformError(String),

    #[error("Timeout waiting for authentication")]
    Timeout,
}

/// Biometric authentication type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BiometricType {
    /// Face recognition (FaceID, Face Unlock)
    Face,
    /// Fingerprint (TouchID, Fingerprint)
    Fingerprint,
    /// Iris scan
    Iris,
    /// Voice recognition
    Voice,
}

impl fmt::Display for BiometricType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BiometricType::Face => write!(f, "Face Recognition"),
            BiometricType::Fingerprint => write!(f, "Fingerprint"),
            BiometricType::Iris => write!(f, "Iris"),
            BiometricType::Voice => write!(f, "Voice"),
        }
    }
}

/// Biometric capability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricCapability {
    /// Available biometric types
    pub available_types: Vec<BiometricType>,
    /// Whether biometric hardware is present
    pub hardware_present: bool,
    /// Whether biometrics are enrolled
    pub enrolled: bool,
    /// Device-specific information
    pub device_info: String,
}

/// Biometric authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricConfig {
    /// Preferred biometric type
    pub preferred_type: Option<BiometricType>,
    /// Authentication prompt message
    pub prompt_message: String,
    /// Allow fallback to device passcode
    pub allow_passcode_fallback: bool,
    /// Timeout in seconds (clamped to safe range)
    pub timeout_seconds: u64,
    /// Maximum authentication retries before lockout
    pub max_retries: u32,
    /// Lockout duration in seconds after max retries
    pub lockout_duration_seconds: u64,
}

/// Minimum timeout to prevent denial of service
const MIN_TIMEOUT_SECONDS: u64 = 5;
/// Maximum timeout to prevent resource holding
const MAX_TIMEOUT_SECONDS: u64 = 120;
/// Maximum prompt message length
const MAX_PROMPT_LENGTH: usize = 200;

impl Default for BiometricConfig {
    fn default() -> Self {
        Self {
            preferred_type: None,
            prompt_message: "Authenticate to access dchat".to_string(),
            allow_passcode_fallback: true,
            timeout_seconds: 30,
            max_retries: 5,
            lockout_duration_seconds: 300, // 5 minutes
        }
    }
}

impl BiometricConfig {
    /// Create a validated config, clamping values to safe ranges
    pub fn validated(mut self) -> Self {
        // Clamp timeout to safe range
        self.timeout_seconds = self.timeout_seconds.clamp(MIN_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS);
        // Truncate prompt message
        self.prompt_message = self.prompt_message.chars().take(MAX_PROMPT_LENGTH).collect();
        // Ensure reasonable retry limit
        self.max_retries = self.max_retries.min(10);
        self
    }
}

/// Biometric authentication result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricAuthResult {
    /// Whether authentication succeeded
    pub success: bool,
    /// Type of biometric used
    pub biometric_type: BiometricType,
    /// Timestamp of authentication
    pub timestamp: i64,
    /// Device-specific authentication token
    pub auth_token: Vec<u8>,
}

/// Platform-agnostic biometric authenticator
pub struct BiometricAuthenticator {
    config: BiometricConfig,
}

impl BiometricAuthenticator {
    /// Get the biometric configuration
    pub fn config(&self) -> &BiometricConfig {
        &self.config
    }

    /// Create a new biometric authenticator
    pub fn new(config: BiometricConfig) -> Self {
        Self { config }
    }

    /// Check biometric capabilities of the device
    pub async fn check_capabilities(&self) -> Result<BiometricCapability, BiometricError> {
        #[cfg(target_os = "ios")]
        {
            self.check_capabilities_ios().await
        }

        #[cfg(target_os = "android")]
        {
            self.check_capabilities_android().await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            // Desktop platforms - limited support
            Ok(BiometricCapability {
                available_types: vec![],
                hardware_present: false,
                enrolled: false,
                device_info: "Desktop platform - biometrics not supported".to_string(),
            })
        }
    }

    /// Authenticate using biometrics
    pub async fn authenticate(&self) -> Result<BiometricAuthResult, BiometricError> {
        #[cfg(target_os = "ios")]
        {
            self.authenticate_ios().await
        }

        #[cfg(target_os = "android")]
        {
            self.authenticate_android().await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Err(BiometricError::NotAvailable)
        }
    }

    /// Store a key in the secure enclave/keystore with biometric protection
    pub async fn store_key(&self, _key_id: &str, _key_data: &[u8]) -> Result<(), BiometricError> {
        #[cfg(target_os = "ios")]
        {
            self.store_key_ios(key_id, key_data).await
        }

        #[cfg(target_os = "android")]
        {
            self.store_key_android(key_id, key_data).await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Err(BiometricError::NotAvailable)
        }
    }

    /// Retrieve a key from secure storage (requires biometric authentication)
    pub async fn retrieve_key(&self, _key_id: &str) -> Result<Vec<u8>, BiometricError> {
        #[cfg(target_os = "ios")]
        {
            self.retrieve_key_ios(key_id).await
        }

        #[cfg(target_os = "android")]
        {
            self.retrieve_key_android(key_id).await
        }

        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            Err(BiometricError::NotAvailable)
        }
    }

    /// Delete a key from secure storage
    pub async fn delete_key(&self, _key_id: &str) -> Result<(), BiometricError> {
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
            Err(BiometricError::NotAvailable)
        }
    }

    // iOS-specific implementations
    #[cfg(target_os = "ios")]
    async fn check_capabilities_ios(&self) -> Result<BiometricCapability, BiometricError> {
        use security_framework::item::ItemSearchOptions;

        // Check for biometric hardware using LAContext
        let context = security_framework::item::LAContext::new();
        let can_evaluate = context.can_evaluate_policy(
            security_framework::item::LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
        );

        let mut available_types = Vec::new();
        if can_evaluate {
            // Check biometric type
            match context.biometry_type() {
                security_framework::item::LABiometryType::FaceID => {
                    available_types.push(BiometricType::Face);
                }
                security_framework::item::LABiometryType::TouchID => {
                    available_types.push(BiometricType::Fingerprint);
                }
                _ => {}
            }
        }

        Ok(BiometricCapability {
            available_types,
            hardware_present: can_evaluate,
            enrolled: can_evaluate && !available_types.is_empty(),
            device_info: format!(
                "iOS device with {}",
                if !available_types.is_empty() {
                    available_types[0].to_string()
                } else {
                    "no biometrics".to_string()
                }
            ),
        })
    }

    #[cfg(target_os = "ios")]
    async fn authenticate_ios(&self) -> Result<BiometricAuthResult, BiometricError> {
        use security_framework::item::{LAContext, LAPolicy};

        let context = LAContext::new();

        match context
            .evaluate_policy(
                LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
                &self.config.prompt_message,
            )
            .await
        {
            Ok(success) => {
                if success {
                    Ok(BiometricAuthResult {
                        success: true,
                        biometric_type: BiometricType::Face, // Detect actual type
                        timestamp: chrono::Utc::now().timestamp(),
                        auth_token: vec![0u8; 32], // Generate secure token
                    })
                } else {
                    Err(BiometricError::AuthenticationFailed(
                        "User denied".to_string(),
                    ))
                }
            }
            Err(e) => Err(BiometricError::PlatformError(format!("{:?}", e))),
        }
    }

    #[cfg(target_os = "ios")]
    async fn store_key_ios(&self, key_id: &str, key_data: &[u8]) -> Result<(), BiometricError> {
        use security_framework::item::*;

        let access_control = SecAccessControl::create_with_flags(
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            kSecAccessControlBiometryCurrentSet | kSecAccessControlPrivateKeyUsage,
        )
        .map_err(|e| BiometricError::PlatformError(format!("{:?}", e)))?;

        let mut query = ItemSearchOptions::new();
        query.set_service(key_id);
        query.set_account("dchat");
        query.set_access_control(access_control);
        query.set_data(key_data);

        query
            .add()
            .map_err(|e| BiometricError::PlatformError(format!("{:?}", e)))?;

        Ok(())
    }

    #[cfg(target_os = "ios")]
    async fn retrieve_key_ios(&self, key_id: &str) -> Result<Vec<u8>, BiometricError> {
        use security_framework::item::*;

        let context = LAContext::new();
        context.set_localized_reason(&self.config.prompt_message);

        let mut query = ItemSearchOptions::new();
        query.set_service(key_id);
        query.set_account("dchat");
        query.set_authentication_context(context);

        let data = query
            .search_one()
            .map_err(|e| BiometricError::PlatformError(format!("{:?}", e)))?;

        Ok(data)
    }

    #[cfg(target_os = "ios")]
    async fn delete_key_ios(&self, key_id: &str) -> Result<(), BiometricError> {
        use security_framework::item::*;

        let mut query = ItemSearchOptions::new();
        query.set_service(key_id);
        query.set_account("dchat");

        query
            .delete()
            .map_err(|e| BiometricError::PlatformError(format!("{:?}", e)))?;

        Ok(())
    }

    // Android-specific implementations
    #[cfg(target_os = "android")]
    async fn check_capabilities_android(&self) -> Result<BiometricCapability, BiometricError> {
        // Use Android BiometricManager via JNI
        let jni_env = self.get_jni_env()?;

        let biometric_manager = jni_env
            .call_static_method(
                "android/hardware/biometrics/BiometricManager",
                "from",
                "(Landroid/content/Context;)Landroid/hardware/biometrics/BiometricManager;",
                &[],
            )
            .map_err(|e| BiometricError::PlatformError(format!("{:?}", e)))?;

        let can_authenticate = jni_env
            .call_method(
                biometric_manager,
                "canAuthenticate",
                "(I)I",
                &[android::hardware::biometrics::BiometricManager::BIOMETRIC_STRONG.into()],
            )
            .map_err(|e| BiometricError::PlatformError(format!("{:?}", e)))?;

        let status = can_authenticate
            .i()
            .map_err(|e| BiometricError::PlatformError(format!("{:?}", e)))?;

        match status {
            0 => {
                // BIOMETRIC_SUCCESS
                Ok(BiometricCapability {
                    available_types: vec![BiometricType::Fingerprint],
                    hardware_present: true,
                    enrolled: true,
                    device_info: "Android device with fingerprint".to_string(),
                })
            }
            11 => {
                // BIOMETRIC_ERROR_NO_HARDWARE
                Ok(BiometricCapability {
                    available_types: vec![],
                    hardware_present: false,
                    enrolled: false,
                    device_info: "No biometric hardware".to_string(),
                })
            }
            12 => {
                // BIOMETRIC_ERROR_NONE_ENROLLED
                Ok(BiometricCapability {
                    available_types: vec![BiometricType::Fingerprint],
                    hardware_present: true,
                    enrolled: false,
                    device_info: "Biometric hardware present but not enrolled".to_string(),
                })
            }
            _ => Err(BiometricError::PlatformError(format!(
                "Unknown status: {}",
                status
            ))),
        }
    }

    #[cfg(target_os = "android")]
    async fn authenticate_android(&self) -> Result<BiometricAuthResult, BiometricError> {
        // Android biometric authentication via JNI to BiometricPrompt API
        // Reference: https://developer.android.com/training/sign-in/biometric-auth
        //
        // The FFI functions are implemented in the dchat-android-bridge module
        // which provides Kotlin/Java bindings for BiometricPrompt.
        extern "C" {
            fn dchat_android_biometric_authenticate(
                prompt_title: *const u8,
                prompt_title_len: usize,
                prompt_subtitle: *const u8,
                prompt_subtitle_len: usize,
                negative_button: *const u8,
                negative_button_len: usize,
                timeout_seconds: u64,
                out_auth_token: *mut u8,
                out_auth_token_len: *mut usize,
            ) -> i32;
            
            fn dchat_android_get_biometric_type() -> i32;
        }

        let title = self.config.prompt_message.as_bytes();
        let subtitle = b"Verify your identity to continue";
        let negative = b"Cancel";
        let mut auth_token = vec![0u8; 64];
        let mut token_len: usize = 0;

        let result = unsafe {
            dchat_android_biometric_authenticate(
                title.as_ptr(),
                title.len(),
                subtitle.as_ptr(),
                subtitle.len(),
                negative.as_ptr(),
                negative.len(),
                self.config.timeout_seconds,
                auth_token.as_mut_ptr(),
                &mut token_len,
            )
        };

        match result {
            0 => {
                // Success - determine biometric type used
                let biometric_type = unsafe { dchat_android_get_biometric_type() };
                let bio_type = match biometric_type {
                    1 => BiometricType::Fingerprint,
                    2 => BiometricType::Face,
                    3 => BiometricType::Iris,
                    _ => BiometricType::Fingerprint, // Default
                };

                auth_token.truncate(token_len);

                Ok(BiometricAuthResult {
                    success: true,
                    biometric_type: bio_type,
                    timestamp: chrono::Utc::now().timestamp(),
                    auth_token,
                })
            }
            -1 => Err(BiometricError::NotAvailable),
            -2 => Err(BiometricError::NoEnrollment),
            -3 => Err(BiometricError::UserCancelled),
            -4 => Err(BiometricError::AuthenticationFailed(
                "Biometric authentication failed".to_string(),
            )),
            -5 => Err(BiometricError::Timeout),
            code => Err(BiometricError::PlatformError(format!(
                "Android biometric error code: {}",
                code
            ))),
        }
    }

    #[cfg(target_os = "android")]
    async fn store_key_android(&self, key_id: &str, key_data: &[u8]) -> Result<(), BiometricError> {
        // Android Keystore with biometric protection via JNI
        // Reference: https://developer.android.com/training/articles/keystore
        extern "C" {
            fn dchat_android_store_key_biometric(
                key_alias: *const u8,
                key_alias_len: usize,
                key_data: *const u8,
                key_data_len: usize,
                require_biometric: bool,
            ) -> i32;
        }

        let result = unsafe {
            dchat_android_store_key_biometric(
                key_id.as_ptr(),
                key_id.len(),
                key_data.as_ptr(),
                key_data.len(),
                true,
            )
        };

        match result {
            0 => Ok(()),
            -1 => Err(BiometricError::NotAvailable),
            -2 => Err(BiometricError::NoEnrollment),
            -3 => Err(BiometricError::AuthenticationFailed("Key storage failed".to_string())),
            code => Err(BiometricError::PlatformError(format!(
                "Android Keystore error: {}",
                code
            ))),
        }
    }

    #[cfg(target_os = "android")]
    async fn retrieve_key_android(&self, key_id: &str) -> Result<Vec<u8>, BiometricError> {
        // Retrieve from Android Keystore with biometric authentication
        // Reference: https://developer.android.com/training/sign-in/biometric-auth
        extern "C" {
            fn dchat_android_retrieve_key_biometric(
                key_alias: *const u8,
                key_alias_len: usize,
                out_data: *mut u8,
                out_data_capacity: usize,
                out_data_len: *mut usize,
                prompt_title: *const u8,
                prompt_title_len: usize,
            ) -> i32;
        }

        let prompt = self.config.prompt_message.as_bytes();
        let mut key_data = vec![0u8; 4096]; // Max key size
        let mut key_len: usize = 0;

        let result = unsafe {
            dchat_android_retrieve_key_biometric(
                key_id.as_ptr(),
                key_id.len(),
                key_data.as_mut_ptr(),
                key_data.len(),
                &mut key_len,
                prompt.as_ptr(),
                prompt.len(),
            )
        };

        match result {
            0 => {
                key_data.truncate(key_len);
                Ok(key_data)
            }
            -1 => Err(BiometricError::NotAvailable),
            -2 => Err(BiometricError::NoEnrollment),
            -3 => Err(BiometricError::UserCancelled),
            -4 => Err(BiometricError::AuthenticationFailed("Biometric auth failed".to_string())),
            -5 => Err(BiometricError::Timeout),
            code => Err(BiometricError::PlatformError(format!(
                "Android Keystore retrieval error: {}",
                code
            ))),
        }
    }

    #[cfg(target_os = "android")]
    async fn delete_key_android(&self, key_id: &str) -> Result<(), BiometricError> {
        // Delete from Android Keystore
        extern "C" {
            fn dchat_android_delete_key(key_alias: *const u8, key_alias_len: usize) -> i32;
        }

        let result = unsafe {
            dchat_android_delete_key(key_id.as_ptr(), key_id.len())
        };

        match result {
            0 => Ok(()),
            -1 => Err(BiometricError::PlatformError("Key not found".to_string())),
            code => Err(BiometricError::PlatformError(format!(
                "Android Keystore delete error: {}",
                code
            ))),
        }
    }

    #[cfg(target_os = "android")]
    fn get_jni_env(&self) -> Result<JNIEnv, BiometricError> {
        // Get JNI environment - implementation depends on Android bridge setup
        Err(BiometricError::PlatformError(
            "JNI environment not available".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_biometric_config_default() {
        let config = BiometricConfig::default();
        assert_eq!(config.timeout_seconds, 30);
        assert!(config.allow_passcode_fallback);
    }

    #[tokio::test]
    async fn test_biometric_type_display() {
        assert_eq!(BiometricType::Face.to_string(), "Face Recognition");
        assert_eq!(BiometricType::Fingerprint.to_string(), "Fingerprint");
    }
}
