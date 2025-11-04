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
    /// Timeout in seconds
    pub timeout_seconds: u64,
}

impl Default for BiometricConfig {
    fn default() -> Self {
        Self {
            preferred_type: None,
            prompt_message: "Authenticate to access dchat".to_string(),
            allow_passcode_fallback: true,
            timeout_seconds: 30,
        }
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
#[allow(dead_code)]
pub struct BiometricAuthenticator {
    config: BiometricConfig,
}

impl BiometricAuthenticator {
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
        // PRODUCTION NOTE: Android biometric authentication requires JNI (Java Native Interface)
        // bindings to call Android's BiometricPrompt API from Rust.
        //
        // Implementation requirements:
        // 1. Create JNI bridge in a separate Android library module
        // 2. Implement BiometricPrompt.AuthenticationCallback in Kotlin/Java
        // 3. Expose callback interface via JNI to Rust
        // 4. Link Android biometric framework (androidx.biometric)
        //
        // This cannot be implemented in pure Rust and requires Android-specific build setup.
        // For production deployment on Android, use a separate biometric plugin or module.
        //
        // Alternative: Use Flutter/React Native biometric plugins if using a hybrid architecture.
        Err(BiometricError::PlatformError(
            "Android biometric authentication requires JNI bindings. \
             See biometric.rs documentation for implementation guide."
                .to_string(),
        ))
    }

    #[cfg(target_os = "android")]
    async fn store_key_android(&self, key_id: &str, key_data: &[u8]) -> Result<(), BiometricError> {
        // Use Android Keystore with biometric protection
        Err(BiometricError::PlatformError(
            "Android implementation pending".to_string(),
        ))
    }

    #[cfg(target_os = "android")]
    async fn retrieve_key_android(&self, key_id: &str) -> Result<Vec<u8>, BiometricError> {
        // Retrieve from Android Keystore with biometric authentication
        Err(BiometricError::PlatformError(
            "Android implementation pending".to_string(),
        ))
    }

    #[cfg(target_os = "android")]
    async fn delete_key_android(&self, key_id: &str) -> Result<(), BiometricError> {
        // Delete from Android Keystore
        Err(BiometricError::PlatformError(
            "Android implementation pending".to_string(),
        ))
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
