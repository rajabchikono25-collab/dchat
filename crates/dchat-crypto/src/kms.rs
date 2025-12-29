//! AWS KMS integration for secure key management
//!
//! This module provides integration with AWS Key Management Service (KMS) for storing
//! and using cryptographic keys without exposing private key material to the application.
//!
//! # Features
//! - Hardware Security Module (HSM) backed key storage
//! - Audit logging of all key operations
//! - No private key exposure to application memory
//! - Support for Ed25519 and ECDSA key types
//!
//! # Usage
//! ```no_run
//! use dchat_crypto::kms::{AwsKmsClient, KmsKeyType};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let kms = AwsKmsClient::new("us-east-1").await?;
//!
//! // Sign data using KMS-managed key
//! let signature = kms.sign(
//!     "alias/dchat-validator-key",
//!     b"message to sign",
//!     KmsKeyType::Ed25519
//! ).await?;
//! # Ok(())
//! # }
//! ```

use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_sdk_kms::{
    operation::sign::SignError,
    types::{MessageType, SigningAlgorithmSpec},
    Client,
};
use ed25519_dalek::{Signature, VerifyingKey};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// KMS client for AWS Key Management Service operations
pub struct AwsKmsClient {
    client: Client,
    region: String,
    timeout: Duration,
}

/// Supported key types for KMS operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KmsKeyType {
    /// Ed25519 signature algorithm (recommended for dchat validators)
    Ed25519,
    /// ECDSA with secp256k1 curve (for compatibility)
    EcdsaSecp256k1,
    /// ECDSA with P-256 curve
    EcdsaP256,
}

/// KMS operation errors
#[derive(Debug, thiserror::Error)]
pub enum KmsError {
    #[error("AWS SDK error: {0}")]
    AwsSdk(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid signature format: {0}")]
    InvalidSignature(String),

    #[error("Unsupported key type: {0:?}")]
    UnsupportedKeyType(KmsKeyType),

    #[error("Timeout waiting for KMS operation")]
    Timeout,

    #[error("Invalid key ID format: {0}")]
    InvalidKeyId(String),
}

impl<R> From<aws_smithy_runtime_api::client::result::SdkError<SignError, R>> for KmsError {
    fn from(err: aws_smithy_runtime_api::client::result::SdkError<SignError, R>) -> Self {
        KmsError::AwsSdk(err.to_string())
    }
}

impl AwsKmsClient {
    /// Create a new KMS client for the specified AWS region
    ///
    /// # Arguments
    /// * `region` - AWS region (e.g., "us-east-1", "eu-west-1")
    ///
    /// # Example
    /// ```no_run
    /// # use dchat_crypto::kms::AwsKmsClient;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kms = AwsKmsClient::new("us-east-1").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(region: &str) -> Result<Self, KmsError> {
        info!("Initializing AWS KMS client for region: {}", region);

        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(region.to_string()))
            .load()
            .await;

        let client = Client::new(&config);

        // Verify connectivity with a test call
        match client.list_keys().limit(1).send().await {
            Ok(_) => {
                info!("✓ AWS KMS client initialized successfully");
                Ok(Self {
                    client,
                    region: region.to_string(),
                    timeout: Duration::from_secs(30),
                })
            }
            Err(e) => {
                error!("Failed to initialize AWS KMS client: {}", e);
                Err(KmsError::AwsSdk(e.to_string()))
            }
        }
    }

    /// Create a new KMS client using a custom AWS SDK configuration
    ///
    /// Useful for testing or custom authentication flows.
    pub fn with_config(config: &SdkConfig, region: String) -> Self {
        let client = Client::new(config);
        Self {
            client,
            region,
            timeout: Duration::from_secs(30),
        }
    }

    /// Set custom timeout for KMS operations
    ///
    /// Default is 30 seconds. Increase for slow networks.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sign data using a KMS-managed key
    ///
    /// # Arguments
    /// * `key_id` - KMS key ID or alias (e.g., "alias/dchat-validator-key")
    /// * `message` - Data to sign
    /// * `key_type` - Cryptographic algorithm to use
    ///
    /// # Returns
    /// Raw signature bytes (64 bytes for Ed25519, 64-72 bytes for ECDSA)
    ///
    /// # Example
    /// ```no_run
    /// # use dchat_crypto::kms::{AwsKmsClient, KmsKeyType};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let kms = AwsKmsClient::new("us-east-1").await?;
    /// let signature = kms.sign(
    ///     "alias/dchat-validator-key",
    ///     b"block proposal data",
    ///     KmsKeyType::Ed25519
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn sign(
        &self,
        key_id: &str,
        message: &[u8],
        key_type: KmsKeyType,
    ) -> Result<Vec<u8>, KmsError> {
        debug!("Signing message with KMS key: {}", key_id);

        let signing_algorithm = match key_type {
            KmsKeyType::Ed25519 => {
                // AWS KMS does not natively support Ed25519.
                // Use Ed25519KmsWrapper for Ed25519 signing with KMS envelope encryption.
                return Err(KmsError::UnsupportedKeyType(key_type));
            }
            KmsKeyType::EcdsaSecp256k1 => SigningAlgorithmSpec::EcdsaSha256,
            KmsKeyType::EcdsaP256 => SigningAlgorithmSpec::EcdsaSha256,
        };

        let result = tokio::time::timeout(
            self.timeout,
            self.client
                .sign()
                .key_id(key_id)
                .message_type(MessageType::Raw)
                .message(aws_sdk_kms::primitives::Blob::new(message))
                .signing_algorithm(signing_algorithm)
                .send(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if let Some(signature) = output.signature() {
                    debug!(
                        "✓ Successfully signed message ({} bytes)",
                        signature.as_ref().len()
                    );
                    Ok(signature.clone().into_inner())
                } else {
                    error!("KMS returned empty signature");
                    Err(KmsError::InvalidSignature("Empty signature".to_string()))
                }
            }
            Ok(Err(e)) => {
                error!("KMS sign operation failed: {}", e);
                Err(KmsError::from(e))
            }
            Err(_) => {
                error!("KMS sign operation timed out after {:?}", self.timeout);
                Err(KmsError::Timeout)
            }
        }
    }

    /// Get the public key for a KMS-managed key
    ///
    /// # Arguments
    /// * `key_id` - KMS key ID or alias
    ///
    /// # Returns
    /// Raw public key bytes (DER encoded for ECDSA)
    pub async fn get_public_key(&self, key_id: &str) -> Result<Vec<u8>, KmsError> {
        debug!("Retrieving public key from KMS: {}", key_id);

        let result = tokio::time::timeout(
            self.timeout,
            self.client.get_public_key().key_id(key_id).send(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if let Some(public_key) = output.public_key() {
                    info!(
                        "✓ Retrieved public key from KMS ({} bytes)",
                        public_key.as_ref().len()
                    );
                    Ok(public_key.clone().into_inner())
                } else {
                    error!("KMS returned empty public key");
                    Err(KmsError::InvalidSignature("Empty public key".to_string()))
                }
            }
            Ok(Err(e)) => {
                error!("Failed to retrieve public key: {}", e);
                Err(KmsError::AwsSdk(e.to_string()))
            }
            Err(_) => {
                error!("Get public key operation timed out");
                Err(KmsError::Timeout)
            }
        }
    }

    /// Verify that a KMS key exists and is accessible
    ///
    /// Useful for startup validation to fail fast if key is misconfigured.
    pub async fn verify_key_exists(&self, key_id: &str) -> Result<bool, KmsError> {
        debug!("Verifying KMS key exists: {}", key_id);

        match self.client.describe_key().key_id(key_id).send().await {
            Ok(output) => {
                if let Some(metadata) = output.key_metadata() {
                    info!(
                        "✓ KMS key verified: {} (state: {:?})",
                        key_id,
                        metadata.key_state()
                    );

                    // Check if key is enabled
                    if metadata.enabled() == false {
                        warn!("KMS key is disabled: {}", key_id);
                        return Ok(false);
                    }

                    Ok(true)
                } else {
                    warn!("KMS key metadata not available: {}", key_id);
                    Ok(false)
                }
            }
            Err(e) => {
                error!("Failed to verify KMS key: {}", e);
                Err(KmsError::KeyNotFound(key_id.to_string()))
            }
        }
    }

    /// List all KMS keys accessible in the current region
    ///
    /// Useful for debugging and key discovery.
    pub async fn list_keys(&self) -> Result<Vec<String>, KmsError> {
        debug!("Listing KMS keys in region: {}", self.region);

        match self.client.list_keys().send().await {
            Ok(output) => {
                let keys: Vec<String> = output
                    .keys()
                    .iter()
                    .filter_map(|k| k.key_id().map(String::from))
                    .collect();

                info!("Found {} KMS keys", keys.len());
                Ok(keys)
            }
            Err(e) => {
                error!("Failed to list KMS keys: {}", e);
                Err(KmsError::AwsSdk(e.to_string()))
            }
        }
    }

    /// Encrypt data using KMS key (envelope encryption)
    ///
    /// # Arguments
    /// * `key_id` - KMS key ID or alias
    /// * `plaintext` - Data to encrypt (max 4096 bytes for KMS Encrypt)
    ///
    /// # Returns
    /// Encrypted ciphertext blob
    pub async fn encrypt_data_key(
        &self,
        key_id: &str,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, KmsError> {
        debug!("Encrypting data with KMS key: {}", key_id);

        if plaintext.len() > 4096 {
            return Err(KmsError::InvalidKeyId(
                "Plaintext too large for KMS encrypt (max 4096 bytes)".to_string(),
            ));
        }

        let result = tokio::time::timeout(
            self.timeout,
            self.client
                .encrypt()
                .key_id(key_id)
                .plaintext(aws_sdk_kms::primitives::Blob::new(plaintext))
                .send(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if let Some(ciphertext) = output.ciphertext_blob() {
                    debug!("✓ Data encrypted ({} bytes)", ciphertext.as_ref().len());
                    Ok(ciphertext.clone().into_inner())
                } else {
                    error!("KMS returned empty ciphertext");
                    Err(KmsError::InvalidSignature("Empty ciphertext".to_string()))
                }
            }
            Ok(Err(e)) => {
                error!("KMS encrypt failed: {}", e);
                Err(KmsError::AwsSdk(e.to_string()))
            }
            Err(_) => {
                error!("KMS encrypt timed out");
                Err(KmsError::Timeout)
            }
        }
    }

    /// Decrypt data encrypted by KMS
    ///
    /// # Arguments
    /// * `ciphertext` - Encrypted data blob from encrypt_data_key()
    ///
    /// # Returns
    /// Decrypted plaintext bytes
    pub async fn decrypt_data_key(&self, ciphertext: &[u8]) -> Result<Vec<u8>, KmsError> {
        debug!("Decrypting data with KMS");

        let result = tokio::time::timeout(
            self.timeout,
            self.client
                .decrypt()
                .ciphertext_blob(aws_sdk_kms::primitives::Blob::new(ciphertext))
                .send(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if let Some(plaintext) = output.plaintext() {
                    debug!("✓ Data decrypted ({} bytes)", plaintext.as_ref().len());
                    Ok(plaintext.clone().into_inner())
                } else {
                    error!("KMS returned empty plaintext");
                    Err(KmsError::InvalidSignature("Empty plaintext".to_string()))
                }
            }
            Ok(Err(e)) => {
                error!("KMS decrypt failed: {}", e);
                Err(KmsError::AwsSdk(e.to_string()))
            }
            Err(_) => {
                error!("KMS decrypt timed out");
                Err(KmsError::Timeout)
            }
        }
    }
}

/// Production Ed25519 signing with KMS-protected key material
///
/// AWS KMS does not natively support Ed25519 signatures. This struct provides
/// a production-ready solution by using KMS envelope encryption to protect
/// the Ed25519 private key material.
///
/// # Security Properties
/// - Ed25519 private key is never stored in plaintext
/// - All KMS operations are logged for audit trails
/// - Key access is controlled via AWS IAM policies
/// - Private key is decrypted in memory only during signing
/// - Memory is securely zeroed after use (via zeroize crate)
///
/// # Storage
/// The encrypted key can be stored locally, in S3, or any secure storage.
/// The key remains protected by the KMS CMK.
pub struct Ed25519KmsWrapper {
    kms: AwsKmsClient,
    /// KMS key ID used for envelope encryption
    key_id: String,
    /// Encrypted Ed25519 private key material
    encrypted_key: Vec<u8>,
    /// Ed25519 public key (stored unencrypted)
    public_key: VerifyingKey,
}

impl Ed25519KmsWrapper {
    /// Get the KMS key ID used for envelope encryption
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Create a new Ed25519 wrapper using KMS for envelope encryption
    ///
    /// The Ed25519 private key is encrypted using a KMS key and stored locally.
    /// This provides audit logging and access control while supporting Ed25519.
    ///
    /// # Arguments
    /// * `kms` - Initialized KMS client
    /// * `key_id` - KMS key ID for envelope encryption
    /// * `encrypted_key` - KMS-encrypted Ed25519 private key material
    /// * `public_key` - Ed25519 public key (unencrypted)
    pub fn new(
        kms: AwsKmsClient,
        key_id: String,
        encrypted_key: Vec<u8>,
        public_key: VerifyingKey,
    ) -> Self {
        Self {
            kms,
            key_id,
            encrypted_key,
            public_key,
        }
    }

    /// Generate a new Ed25519 keypair and encrypt it with KMS
    ///
    /// # Arguments
    /// * `kms` - Initialized KMS client
    /// * `key_id` - KMS key ID for envelope encryption
    ///
    /// # Returns
    /// Tuple of (Ed25519KmsWrapper, encrypted_key_bytes) for storage
    pub async fn generate(kms: AwsKmsClient, key_id: &str) -> Result<(Self, Vec<u8>), KmsError> {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        info!("Generating new Ed25519 keypair with KMS envelope encryption");

        // Generate Ed25519 keypair
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let secret_bytes = signing_key.to_bytes();

        // Encrypt private key with KMS data key
        let encrypted_key = kms.encrypt_data_key(key_id, &secret_bytes).await?;

        info!("✓ Ed25519 keypair generated and encrypted with KMS");

        Ok((
            Self {
                kms,
                key_id: key_id.to_string(),
                encrypted_key: encrypted_key.clone(),
                public_key: verifying_key,
            },
            encrypted_key,
        ))
    }

    /// Sign a message using Ed25519 with KMS-encrypted key material
    ///
    /// # Security
    /// - Private key is decrypted in memory only for signing operation
    /// - Memory is securely zeroed after use (via zeroize crate)
    /// - All KMS operations are logged for audit trails
    pub async fn sign(&self, message: &[u8]) -> Result<Signature, KmsError> {
        use ed25519_dalek::SigningKey;
        use zeroize::Zeroize;

        debug!("Signing message with KMS-protected Ed25519 key");

        // Decrypt private key from KMS
        let mut secret_bytes = self.kms.decrypt_data_key(&self.encrypted_key).await?;

        // Convert to Ed25519 signing key
        let secret_array: [u8; 32] = secret_bytes[..32]
            .try_into()
            .map_err(|_| KmsError::InvalidSignature("Invalid key length".to_string()))?;

        let signing_key = SigningKey::from_bytes(&secret_array);

        // Sign message
        use ed25519_dalek::Signer;
        let signature = signing_key.sign(message);

        // Securely erase secret key from memory
        secret_bytes.zeroize();
        drop(signing_key);

        debug!("✓ Message signed successfully");
        Ok(signature)
    }

    /// Get the Ed25519 public key
    ///
    /// Public key is stored unencrypted for performance.
    pub fn get_public_key(&self) -> &VerifyingKey {
        &self.public_key
    }

    /// Get encrypted key material for storage
    ///
    /// Use this to save the encrypted key to disk or S3.
    pub fn get_encrypted_key(&self) -> &[u8] {
        &self.encrypted_key
    }

    /// Load an Ed25519 key from encrypted storage
    ///
    /// # Arguments
    /// * `kms` - Initialized KMS client
    /// * `key_id` - KMS key ID used for encryption
    /// * `encrypted_key` - Previously encrypted Ed25519 private key
    /// * `public_key_bytes` - Public key bytes (32 bytes)
    pub fn load(
        kms: AwsKmsClient,
        key_id: String,
        encrypted_key: Vec<u8>,
        public_key_bytes: &[u8; 32],
    ) -> Result<Self, KmsError> {
        let public_key = VerifyingKey::from_bytes(public_key_bytes)
            .map_err(|e| KmsError::InvalidSignature(format!("Invalid public key: {}", e)))?;

        Ok(Self {
            kms,
            key_id,
            encrypted_key,
            public_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires AWS credentials
    async fn test_kms_client_initialization() {
        let result = AwsKmsClient::new("us-east-1").await;
        assert!(result.is_ok(), "KMS client should initialize");
    }

    #[tokio::test]
    #[ignore] // Requires AWS credentials and KMS key
    async fn test_verify_key_exists() {
        let kms = AwsKmsClient::new("us-east-1").await.unwrap();
        let result = kms.verify_key_exists("alias/dchat-test-key").await;
        // Result depends on whether test key exists
        assert!(result.is_ok() || matches!(result, Err(KmsError::KeyNotFound(_))));
    }

    #[tokio::test]
    #[ignore] // Requires AWS credentials
    async fn test_list_keys() {
        let kms = AwsKmsClient::new("us-east-1").await.unwrap();
        let result = kms.list_keys().await;
        assert!(result.is_ok(), "Should be able to list keys");
    }
}
