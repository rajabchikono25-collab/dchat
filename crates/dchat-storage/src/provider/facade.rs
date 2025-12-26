//! Storage Facade - Unified storage interface for SDK and user management
//!
//! This module provides a high-level facade over the tiered storage system,
//! handling automatic routing between:
//! - CockroachDB: Primary persistent storage for message metadata and small content
//! - SQLite: Offline cache for local access
//! - Redis: Hot cache for frequently accessed data
//! - S3/MinIO: Large blob storage
//! - IPFS: Optional content-addressed pinning
//!
//! The facade abstracts away storage complexity and provides:
//! - Automatic tiering based on content size
//! - Provider selection with replication
//! - Challenge/proof verification
//! - Encrypted storage with key management

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::distributed::object_storage::{DistributedObjectStorage, ObjectStorageConfig};
use crate::ipfs::{IpfsClient, IpfsConfig};
use crate::provider::blob_ref::{BlobCodec, BlobLocation, BlobRef, LocationStatus, StorageClass};
use crate::provider::challenges::ChallengeConfig;
use crate::provider::registry::{ProviderRegistry, ProviderRegistryConfig, RegisteredProvider};
use crate::provider::router::{StorageRouter, StorageRouterConfig};
use crate::provider::selection::{
    ProviderSelection, ProviderSelector, ReplicationConfig, SelectionCriteria,
};

/// Errors that can occur in the storage facade
#[derive(Debug, Error)]
pub enum StorageFacadeError {
    #[error("Storage router error: {0}")]
    Router(String),

    #[error("Provider registry error: {0}")]
    Registry(String),

    #[error("Challenge system error: {0}")]
    Challenge(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Blob not found: {0}")]
    BlobNotFound(String),

    #[error("Insufficient storage quota: need {needed} bytes, have {available} bytes")]
    InsufficientQuota { needed: u64, available: u64 },

    #[error("No healthy providers available")]
    NoHealthyProviders,

    #[error("Provider {0} not found")]
    ProviderNotFound(String),

    #[error("Replication failed: only {achieved} of {required} replicas created")]
    ReplicationFailed { required: usize, achieved: usize },

    #[error("Verification failed: {0}")]
    VerificationFailed(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Object storage error: {0}")]
    ObjectStorage(String),

    #[error("IPFS error: {0}")]
    Ipfs(String),
}

/// Result type for storage facade operations
pub type Result<T> = std::result::Result<T, StorageFacadeError>;

/// Configuration for the storage facade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageFacadeConfig {
    /// Threshold for inline storage vs blob storage (bytes)
    pub inline_threshold: usize,

    /// Default replication configuration
    pub default_replication: ReplicationConfig,

    /// Enable IPFS pinning for blobs
    pub enable_ipfs_pinning: bool,

    /// Enable challenge/proof verification
    pub enable_challenges: bool,

    /// Challenge interval in seconds
    pub challenge_interval_secs: u64,

    /// Maximum blob size (bytes)
    pub max_blob_size: usize,

    /// Default storage class for new blobs
    pub default_storage_class: StorageClass,

    /// Enable encryption at rest
    pub encrypt_at_rest: bool,

    /// Encryption key rotation interval (days)
    pub key_rotation_days: u32,

    /// User default quota (bytes)
    pub default_user_quota: u64,

    /// Enable storage receipts for audit
    pub enable_receipts: bool,

    /// Router configuration
    pub router: StorageRouterConfig,

    /// Registry configuration
    pub registry: ProviderRegistryConfig,

    /// Challenge manager configuration
    pub challenges: ChallengeConfig,
}

impl Default for StorageFacadeConfig {
    fn default() -> Self {
        Self {
            inline_threshold: 64 * 1024, // 64KB
            default_replication: ReplicationConfig::default(),
            enable_ipfs_pinning: true,
            enable_challenges: true,
            challenge_interval_secs: 3600,    // 1 hour
            max_blob_size: 100 * 1024 * 1024, // 100MB
            default_storage_class: StorageClass::Standard,
            encrypt_at_rest: true,
            key_rotation_days: 30,
            default_user_quota: 10 * 1024 * 1024 * 1024, // 10GB
            enable_receipts: true,
            router: StorageRouterConfig::default(),
            registry: ProviderRegistryConfig::default(),
            challenges: ChallengeConfig::default(),
        }
    }
}

/// Storage operation type for receipts
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StorageOperation {
    Store,
    Retrieve,
    Replicate,
    ChallengeReward,
    Delete,
}

impl std::fmt::Display for StorageOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store => write!(f, "store"),
            Self::Retrieve => write!(f, "retrieve"),
            Self::Replicate => write!(f, "replicate"),
            Self::ChallengeReward => write!(f, "challenge_reward"),
            Self::Delete => write!(f, "delete"),
        }
    }
}

/// Storage receipt for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageReceipt {
    pub id: i32,
    pub user_id: Vec<u8>,
    pub provider_id: Vec<u8>,
    pub blob_hash: Option<Vec<u8>>,
    pub operation: StorageOperation,
    pub amount: u64,
    pub tx_id: Option<String>,
    pub chain_anchor_tx: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// User storage quota information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStorageQuota {
    pub user_id: Vec<u8>,
    pub total_quota: u64,
    pub used_bytes: u64,
    pub blob_count: u64,
    pub inline_content_bytes: u64,
    pub available: u64,
    pub usage_percent: f64,
}

/// Blob storage result with location details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreBlobResult {
    pub blob_ref: BlobRef,
    pub locations: Vec<BlobLocation>,
    pub receipts: Vec<i32>,
    pub total_cost: u64,
}

/// Message storage result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMessageResult {
    pub message_id: Uuid,
    pub inline: bool,
    pub blob_ref: Option<BlobRef>,
    pub cost: u64,
}

/// Blob health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobHealthStatus {
    pub hash: Vec<u8>,
    pub total_locations: usize,
    pub healthy_locations: usize,
    pub ipfs_locations: usize,
    pub region_diversity: usize,
    pub status: HealthLevel,
    pub needs_replication: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthLevel {
    Healthy,
    AtRisk,
    Critical,
}

/// Tier migration request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierMigrationRequest {
    pub blob_hash: Vec<u8>,
    pub target_storage_class: StorageClass,
    pub target_region: Option<String>,
    pub target_provider_id: Option<Vec<u8>>,
}

/// The main storage facade providing unified access to tiered storage
pub struct StorageFacade {
    config: StorageFacadeConfig,
    db_pool: PgPool,
    router: Arc<StorageRouter>,
    registry: Arc<ProviderRegistry>,
    selector: Arc<ProviderSelector>,
    encryption_key_id: RwLock<String>,
    /// Encryption key cache: key_id -> 32-byte key
    encryption_keys: RwLock<HashMap<String, [u8; 32]>>,
    /// IPFS client for content-addressed storage
    ipfs_client: Option<Arc<IpfsClient>>,
    /// S3/MinIO clients per provider (provider_id -> client)
    object_storage_clients: RwLock<HashMap<[u8; 32], Arc<DistributedObjectStorage>>>,
}

impl StorageFacade {
    /// Create a new storage facade with the given configuration
    pub async fn new(
        config: StorageFacadeConfig,
        db_pool: PgPool,
        router: StorageRouter,
        registry: ProviderRegistry,
    ) -> Result<Self> {
        let registry = Arc::new(registry);
        let selector = ProviderSelector::new(Arc::clone(&registry));

        // Get or create encryption key
        let encryption_key_id = Self::get_or_create_encryption_key(&db_pool).await?;

        // Initialize encryption key cache with current key
        let mut encryption_keys = HashMap::new();
        let current_key =
            Self::load_or_generate_encryption_key(&db_pool, &encryption_key_id).await?;
        encryption_keys.insert(encryption_key_id.clone(), current_key);

        // Initialize IPFS client if pinning is enabled
        let ipfs_client = if config.enable_ipfs_pinning {
            match IpfsClient::new(IpfsConfig::default()) {
                Ok(client) => {
                    info!("IPFS client initialized for content pinning");
                    Some(Arc::new(client))
                }
                Err(e) => {
                    warn!("IPFS client initialization failed, pinning disabled: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            config,
            db_pool,
            router: Arc::new(router),
            registry,
            selector: Arc::new(selector),
            encryption_key_id: RwLock::new(encryption_key_id),
            encryption_keys: RwLock::new(encryption_keys),
            ipfs_client,
            object_storage_clients: RwLock::new(HashMap::new()),
        })
    }

    /// Get or create the current encryption key ID
    async fn get_or_create_encryption_key(_db_pool: &PgPool) -> Result<String> {
        // Generate monthly key ID for rotation
        let key_id = format!("dchat-storage-key-{}", Utc::now().format("%Y%m"));
        Ok(key_id)
    }

    /// Load encryption key from database or generate new one
    async fn load_or_generate_encryption_key(db_pool: &PgPool, key_id: &str) -> Result<[u8; 32]> {
        // Try to load existing key from database
        let row = sqlx::query(
            r#"
            SELECT key_data FROM encryption_keys WHERE key_id = $1
            "#,
        )
        .bind(key_id)
        .fetch_optional(db_pool)
        .await?;

        if let Some(row) = row {
            let key_data: Vec<u8> = row.try_get("key_data")?;
            if key_data.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&key_data);
                return Ok(key);
            }
        }

        // Generate new key using secure random
        let key = dchat_crypto::generate_encryption_key();

        // Store key in database (encrypted at rest by database TDE)
        sqlx::query(
            r#"
            INSERT INTO encryption_keys (key_id, key_data, created_at)
            VALUES ($1, $2, $3)
            ON CONFLICT (key_id) DO NOTHING
            "#,
        )
        .bind(key_id)
        .bind(key.as_slice())
        .bind(Utc::now())
        .execute(db_pool)
        .await?;

        info!(key_id = key_id, "Generated new encryption key");
        Ok(key)
    }

    /// Get encryption key by ID (with caching)
    async fn get_encryption_key(&self, key_id: &str) -> Result<[u8; 32]> {
        // Check cache first
        {
            let cache = self.encryption_keys.read().await;
            if let Some(key) = cache.get(key_id) {
                return Ok(*key);
            }
        }

        // Load from database
        let key = Self::load_or_generate_encryption_key(&self.db_pool, key_id).await?;

        // Cache it
        {
            let mut cache = self.encryption_keys.write().await;
            cache.insert(key_id.to_string(), key);
        }

        Ok(key)
    }

    /// Get or create S3 client for a provider
    async fn get_or_create_s3_client(
        &self,
        provider: &RegisteredProvider,
    ) -> Result<Arc<DistributedObjectStorage>> {
        // Check cache
        {
            let clients = self.object_storage_clients.read().await;
            if let Some(client) = clients.get(&provider.id) {
                return Ok(Arc::clone(client));
            }
        }

        // Load provider credentials from database
        let row = sqlx::query(
            r#"
            SELECT access_key_enc, secret_key_enc, key_id
            FROM provider_credentials
            WHERE provider_id = $1 AND credential_type = 's3'
            "#,
        )
        .bind(provider.id.as_slice())
        .fetch_optional(&self.db_pool)
        .await?;

        let (access_key, secret_key) = if let Some(row) = row {
            let access_key_enc: Vec<u8> = row.try_get("access_key_enc")?;
            let secret_key_enc: Vec<u8> = row.try_get("secret_key_enc")?;
            let cred_key_id: String = row.try_get("key_id")?;

            // Decrypt credentials
            let encryption_key = self.get_encryption_key(&cred_key_id).await?;
            let access_key = dchat_crypto::decrypt_with_key(&encryption_key, &access_key_enc)
                .map_err(|e| StorageFacadeError::Encryption(e.to_string()))?;
            let secret_key = dchat_crypto::decrypt_with_key(&encryption_key, &secret_key_enc)
                .map_err(|e| StorageFacadeError::Encryption(e.to_string()))?;

            (
                String::from_utf8(access_key)
                    .map_err(|e| StorageFacadeError::Encryption(e.to_string()))?,
                String::from_utf8(secret_key)
                    .map_err(|e| StorageFacadeError::Encryption(e.to_string()))?,
            )
        } else {
            return Err(StorageFacadeError::ProviderNotFound(format!(
                "No credentials for provider {}",
                hex::encode(provider.id)
            )));
        };

        // Create S3 config from provider capabilities
        let s3_cap = &provider.capabilities.object_s3;
        let config = ObjectStorageConfig {
            endpoint: s3_cap.endpoint.clone(),
            bucket_name: s3_cap.bucket.clone(),
            access_key,
            secret_key,
            region: provider.primary_region.clone(),
            cdn_url: None,
            multi_region: false,
            upload_timeout_seconds: 60,
        };

        let client = DistributedObjectStorage::new(config)
            .await
            .map_err(|e| StorageFacadeError::ObjectStorage(e.to_string()))?;
        let client = Arc::new(client);

        // Cache client
        {
            let mut clients = self.object_storage_clients.write().await;
            clients.insert(provider.id, Arc::clone(&client));
        }

        Ok(client)
    }

    /// Store a message with automatic tiering
    ///
    /// Small messages are stored inline in the database, larger content
    /// is stored as blobs in S3/IPFS with only metadata in the DB.
    pub async fn store_message(
        &self,
        sender_id: &[u8],
        recipient_id: Option<&[u8]>,
        channel_id: Option<&[u8]>,
        content: &[u8],
        content_type: &str,
        encrypted: bool,
    ) -> Result<StoreMessageResult> {
        // Check user quota
        let quota = self.get_user_quota(sender_id).await?;
        if quota.available < content.len() as u64 {
            return Err(StorageFacadeError::InsufficientQuota {
                needed: content.len() as u64,
                available: quota.available,
            });
        }

        let message_id = Uuid::new_v4();
        let is_inline = content.len() <= self.config.inline_threshold;
        let created_at = Utc::now();

        if is_inline {
            // Store inline in database directly
            self.store_inline_message_direct(
                message_id,
                sender_id,
                recipient_id,
                channel_id,
                content_type,
                content.len(),
                encrypted,
                content,
                created_at,
            )
            .await?;
            self.update_user_quota(sender_id, content.len() as i64, 0)
                .await?;

            Ok(StoreMessageResult {
                message_id,
                inline: true,
                blob_ref: None,
                cost: 0, // Inline storage is included in base plan
            })
        } else {
            // Store as blob
            let blob_result = self
                .store_blob(sender_id, content, content_type, encrypted)
                .await?;

            // Store message metadata with blob reference
            self.store_message_metadata_direct(
                message_id,
                sender_id,
                recipient_id,
                channel_id,
                content_type,
                content.len(),
                encrypted,
                Some(&blob_result.blob_ref.hash),
                created_at,
            )
            .await?;
            self.update_user_quota(sender_id, 0, 1).await?;

            Ok(StoreMessageResult {
                message_id,
                inline: false,
                blob_ref: Some(blob_result.blob_ref),
                cost: blob_result.total_cost,
            })
        }
    }

    /// Store a blob with replication across providers
    pub async fn store_blob(
        &self,
        user_id: &[u8],
        data: &[u8],
        mime_type: &str,
        encrypted: bool,
    ) -> Result<StoreBlobResult> {
        if data.len() > self.config.max_blob_size {
            return Err(StorageFacadeError::Config(format!(
                "Blob size {} exceeds maximum {}",
                data.len(),
                self.config.max_blob_size
            )));
        }

        // Select providers for storage
        let criteria = SelectionCriteria {
            required_size: data.len() as u64,
            required_capabilities: vec![
                crate::provider::capabilities::ProviderCapability::ObjectS3,
            ],
            user_region: None,
            max_price_per_gb: None,
            min_reputation: Some(0.5),
            replication: self.config.default_replication.clone(),
        };

        let selection = self.select_providers(&criteria).await?;
        if selection.primary_providers.is_empty() {
            return Err(StorageFacadeError::NoHealthyProviders);
        }

        // Encrypt data if requested using AES-256-GCM
        let (storage_data, encryption_key_id) = if encrypted && self.config.encrypt_at_rest {
            let key_id = self.encryption_key_id.read().await.clone();
            let encryption_key = self.get_encryption_key(&key_id).await?;

            let encrypted_data = dchat_crypto::encrypt_with_key(&encryption_key, data)
                .map_err(|e| StorageFacadeError::Encryption(e.to_string()))?;

            debug!(
                original_size = data.len(),
                encrypted_size = encrypted_data.len(),
                key_id = %key_id,
                "Encrypted blob data"
            );

            (encrypted_data, Some(key_id))
        } else {
            (data.to_vec(), None)
        };

        // Create blob reference with content hash
        let mut blob_ref =
            BlobRef::from_content(&storage_data, BlobCodec::Raw, mime_type, encrypted);
        blob_ref.encryption_key_id = encryption_key_id.clone();

        // Store blob_ref in database first
        self.store_blob_ref(&blob_ref).await?;

        // Store in each selected provider
        let mut locations = Vec::new();
        let mut receipts = Vec::new();
        let mut total_cost = 0u64;

        for selected in &selection.primary_providers {
            let provider = &selected.provider;
            match self
                .store_blob_at_provider(provider, &blob_ref.hash, &storage_data)
                .await
            {
                Ok((location, cost)) => {
                    // Record location in database
                    self.store_blob_location(&blob_ref.hash, &location).await?;
                    locations.push(location);

                    // Create receipt
                    if self.config.enable_receipts {
                        let receipt_id = self
                            .create_receipt(
                                user_id,
                                &provider.id,
                                Some(&blob_ref.hash),
                                StorageOperation::Store,
                                cost,
                            )
                            .await?;
                        receipts.push(receipt_id);
                    }

                    total_cost += cost;
                }
                Err(e) => {
                    warn!(
                        provider_id = hex::encode(&provider.id),
                        error = %e,
                        "Failed to store blob at provider"
                    );
                }
            }
        }

        // Check if we achieved minimum replication
        if locations.len() < self.config.default_replication.min_replicas {
            return Err(StorageFacadeError::ReplicationFailed {
                required: self.config.default_replication.min_replicas,
                achieved: locations.len(),
            });
        }

        // Update blob_ref with locations
        blob_ref.locations = locations.clone();

        Ok(StoreBlobResult {
            blob_ref,
            locations,
            receipts,
            total_cost,
        })
    }

    /// Store a blob at a specific provider using real S3/IPFS clients
    async fn store_blob_at_provider(
        &self,
        provider: &RegisteredProvider,
        hash: &[u8; 32],
        data: &[u8],
    ) -> Result<(BlobLocation, u64)> {
        let capabilities = &provider.capabilities;
        let s3_cap = &capabilities.object_s3;
        let key = format!("blobs/{}", hex::encode(hash));

        let mut location = BlobLocation::s3(provider.id, &key, &provider.primary_region);
        location.s3_bucket = Some(s3_cap.bucket.clone());
        location.status = LocationStatus::Pending;

        // Get or create S3 client for this provider
        let s3_client = self.get_or_create_s3_client(provider).await?;

        // Upload to S3
        match s3_client
            .upload_bytes(data, &key, "application/octet-stream")
            .await
        {
            Ok(metadata) => {
                info!(
                    provider_id = hex::encode(&provider.id),
                    key = %key,
                    size = data.len(),
                    etag = %metadata.etag,
                    "Blob stored in S3"
                );
                location.status = LocationStatus::Healthy;
            }
            Err(e) => {
                error!(
                    provider_id = hex::encode(&provider.id),
                    key = %key,
                    error = %e,
                    "Failed to upload blob to S3"
                );
                return Err(StorageFacadeError::ObjectStorage(e.to_string()));
            }
        }

        // Pin to IPFS if enabled and provider supports it
        if self.config.enable_ipfs_pinning && capabilities.ipfs_pinning.is_some() {
            if let Some(ipfs_client) = &self.ipfs_client {
                match ipfs_client
                    .upload(
                        data.to_vec(),
                        format!("{}.bin", hex::encode(hash)),
                        "application/octet-stream".to_string(),
                    )
                    .await
                {
                    Ok(ipfs_file) => {
                        info!(
                            cid = %ipfs_file.cid,
                            size = ipfs_file.size,
                            "Blob pinned to IPFS"
                        );
                        location.ipfs_cid = Some(ipfs_file.cid);
                    }
                    Err(e) => {
                        // IPFS pinning is optional, log warning but continue
                        warn!(
                            error = %e,
                            "Failed to pin blob to IPFS, continuing with S3 only"
                        );
                    }
                }
            }
        }

        // Calculate cost based on data size
        let cost = self.calculate_storage_cost(provider, data.len() as u64);

        Ok((location, cost))
    }

    /// Retrieve a blob by its reference
    pub async fn retrieve_blob(&self, user_id: &[u8], blob_ref: &BlobRef) -> Result<Vec<u8>> {
        let healthy_locations: Vec<_> = blob_ref
            .locations
            .iter()
            .filter(|l| l.status == LocationStatus::Healthy)
            .collect();

        if healthy_locations.is_empty() {
            return Err(StorageFacadeError::BlobNotFound(hex::encode(
                &blob_ref.hash,
            )));
        }

        // Try each healthy location
        for location in &healthy_locations {
            match self.retrieve_from_location(location).await {
                Ok(encrypted_data) => {
                    // Decrypt if blob was encrypted
                    let data = if blob_ref.encrypted {
                        if let Some(key_id) = &blob_ref.encryption_key_id {
                            let encryption_key = self.get_encryption_key(key_id).await?;
                            dchat_crypto::decrypt_with_key(&encryption_key, &encrypted_data)
                                .map_err(|e| StorageFacadeError::Encryption(e.to_string()))?
                        } else {
                            return Err(StorageFacadeError::Encryption(
                                "Blob marked as encrypted but no encryption key ID".to_string(),
                            ));
                        }
                    } else {
                        encrypted_data
                    };

                    // Verify hash against decrypted content
                    if !blob_ref.verify_content(&data) {
                        warn!(
                            blob_hash = hex::encode(&blob_ref.hash),
                            location_provider = hex::encode(&location.provider_id),
                            "Hash verification failed, trying next location"
                        );
                        continue;
                    }

                    // Update access time
                    self.update_location_access(&blob_ref.hash, &location.provider_id)
                        .await?;

                    // Create retrieval receipt
                    if self.config.enable_receipts {
                        let provider = self
                            .registry
                            .get_provider(&location.provider_id)
                            .await
                            .map_err(|e| StorageFacadeError::Registry(e.to_string()))?;
                        if let Some(p) = provider {
                            let cost = self.calculate_retrieval_cost(&p, data.len() as u64);
                            let _ = self
                                .create_receipt(
                                    user_id,
                                    &location.provider_id,
                                    Some(&blob_ref.hash),
                                    StorageOperation::Retrieve,
                                    cost,
                                )
                                .await;
                        }
                    }

                    return Ok(data);
                }
                Err(e) => {
                    warn!(
                        location_provider = hex::encode(&location.provider_id),
                        error = %e,
                        "Failed to retrieve from location"
                    );
                }
            }
        }

        Err(StorageFacadeError::BlobNotFound(hex::encode(
            &blob_ref.hash,
        )))
    }

    /// Retrieve data from a specific location using real S3/IPFS clients
    async fn retrieve_from_location(&self, location: &BlobLocation) -> Result<Vec<u8>> {
        // Try S3 first (primary storage)
        if let Some(key) = &location.s3_key {
            // Get provider to create S3 client
            let provider = self
                .registry
                .get_provider(&location.provider_id)
                .await
                .map_err(|e| StorageFacadeError::Registry(e.to_string()))?;

            if let Some(provider) = provider {
                match self.get_or_create_s3_client(&provider).await {
                    Ok(s3_client) => match s3_client.download_bytes(key).await {
                        Ok(data) => {
                            debug!(
                                key = %key,
                                size = data.len(),
                                "Retrieved blob from S3"
                            );
                            return Ok(data);
                        }
                        Err(e) => {
                            warn!(
                                key = %key,
                                error = %e,
                                "Failed to retrieve from S3, trying IPFS fallback"
                            );
                        }
                    },
                    Err(e) => {
                        warn!(
                            provider_id = hex::encode(&location.provider_id),
                            error = %e,
                            "Failed to get S3 client, trying IPFS fallback"
                        );
                    }
                }
            }
        }

        // Try IPFS as fallback
        if let Some(cid) = &location.ipfs_cid {
            if let Some(ipfs_client) = &self.ipfs_client {
                match ipfs_client.download(cid).await {
                    Ok(data) => {
                        info!(
                            cid = %cid,
                            size = data.len(),
                            "Retrieved blob from IPFS"
                        );
                        return Ok(data);
                    }
                    Err(e) => {
                        warn!(
                            cid = %cid,
                            error = %e,
                            "Failed to retrieve from IPFS"
                        );
                    }
                }
            }
        }

        Err(StorageFacadeError::BlobNotFound(format!(
            "Failed to retrieve from all sources for provider {}",
            hex::encode(&location.provider_id)
        )))
    }

    /// Get user storage quota
    pub async fn get_user_quota(&self, user_id: &[u8]) -> Result<UserStorageQuota> {
        let row = sqlx::query(
            r#"
            SELECT 
                total_quota_bytes,
                used_bytes,
                blob_count,
                inline_content_bytes
            FROM user_storage_quotas
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.db_pool)
        .await?;

        match row {
            Some(r) => {
                let total: i64 = r.try_get("total_quota_bytes")?;
                let used: i64 = r.try_get("used_bytes")?;
                let blobs: i64 = r.try_get("blob_count")?;
                let inline: i64 = r.try_get("inline_content_bytes")?;
                let total = total as u64;
                let used = used as u64;
                Ok(UserStorageQuota {
                    user_id: user_id.to_vec(),
                    total_quota: total,
                    used_bytes: used,
                    blob_count: blobs as u64,
                    inline_content_bytes: inline as u64,
                    available: total.saturating_sub(used),
                    usage_percent: (used as f64 / total as f64) * 100.0,
                })
            }
            None => {
                // Create default quota
                let quota = self.config.default_user_quota;
                sqlx::query(
                    r#"
                    INSERT INTO user_storage_quotas (user_id, total_quota_bytes)
                    VALUES ($1, $2)
                    "#,
                )
                .bind(user_id)
                .bind(quota as i64)
                .execute(&self.db_pool)
                .await?;

                Ok(UserStorageQuota {
                    user_id: user_id.to_vec(),
                    total_quota: quota,
                    used_bytes: 0,
                    blob_count: 0,
                    inline_content_bytes: 0,
                    available: quota,
                    usage_percent: 0.0,
                })
            }
        }
    }

    /// Check blob health status
    pub async fn check_blob_health(&self, blob_hash: &[u8]) -> Result<BlobHealthStatus> {
        let row = sqlx::query(
            r#"
            SELECT 
                hash,
                total_locations,
                healthy_locations,
                ipfs_locations,
                region_diversity,
                health_status
            FROM blob_health_status
            WHERE hash = $1
            "#,
        )
        .bind(blob_hash)
        .fetch_optional(&self.db_pool)
        .await?;

        match row {
            Some(r) => {
                let hash: Vec<u8> = r.try_get("hash")?;
                let total: i64 = r.try_get("total_locations")?;
                let healthy: i64 = r.try_get("healthy_locations")?;
                let ipfs: i64 = r.try_get("ipfs_locations")?;
                let regions: i64 = r.try_get("region_diversity")?;
                let status_str: String = r.try_get("health_status")?;

                let status = match status_str.as_str() {
                    "healthy" => HealthLevel::Healthy,
                    "at_risk" => HealthLevel::AtRisk,
                    _ => HealthLevel::Critical,
                };
                let needs_replication =
                    (healthy as usize) < self.config.default_replication.min_replicas;

                Ok(BlobHealthStatus {
                    hash,
                    total_locations: total as usize,
                    healthy_locations: healthy as usize,
                    ipfs_locations: ipfs as usize,
                    region_diversity: regions as usize,
                    status,
                    needs_replication,
                })
            }
            None => Err(StorageFacadeError::BlobNotFound(hex::encode(blob_hash))),
        }
    }

    /// Request tier migration for a blob
    pub async fn request_tier_migration(
        &self,
        _user_id: &[u8],
        request: TierMigrationRequest,
    ) -> Result<i32> {
        // Verify blob exists
        let _health = self.check_blob_health(&request.blob_hash).await?;

        // Create migration request
        let row = sqlx::query(
            r#"
            INSERT INTO blob_tier_migrations (
                blob_hash, target_storage_class, target_region, target_provider_id
            )
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(&request.blob_hash)
        .bind(request.target_storage_class.to_string())
        .bind(&request.target_region)
        .bind(request.target_provider_id.as_deref())
        .fetch_one(&self.db_pool)
        .await?;

        let id: i32 = row.try_get("id")?;

        info!(
            migration_id = id,
            blob_hash = hex::encode(&request.blob_hash),
            target_class = ?request.target_storage_class,
            "Tier migration requested"
        );

        Ok(id)
    }

    /// List providers with their capabilities
    pub async fn list_providers(&self) -> Result<Vec<RegisteredProvider>> {
        self.registry
            .list_active_providers()
            .await
            .map_err(|e| StorageFacadeError::Registry(e.to_string()))
    }

    /// Get provider details
    pub async fn get_provider(&self, provider_id: &[u8; 32]) -> Result<Option<RegisteredProvider>> {
        self.registry
            .get_provider(provider_id)
            .await
            .map_err(|e| StorageFacadeError::Registry(e.to_string()))
    }

    /// Select providers based on criteria
    async fn select_providers(&self, criteria: &SelectionCriteria) -> Result<ProviderSelection> {
        self.selector
            .select_providers(criteria)
            .await
            .map_err(|e| StorageFacadeError::Registry(e.to_string()))
    }

    /// Store inline message content (direct parameters, no struct)
    async fn store_inline_message_direct(
        &self,
        id: Uuid,
        sender_id: &[u8],
        recipient_id: Option<&[u8]>,
        channel_id: Option<&[u8]>,
        content_type: &str,
        size: usize,
        encrypted: bool,
        content: &[u8],
        created_at: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO messages (
                id, sender_id, recipient_id, channel_id,
                content_type, size, encrypted, content, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(id)
        .bind(sender_id)
        .bind(recipient_id)
        .bind(channel_id)
        .bind(content_type)
        .bind(size as i64)
        .bind(encrypted)
        .bind(content)
        .bind(created_at)
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    /// Store message metadata for blob-based messages (direct parameters, no struct)
    async fn store_message_metadata_direct(
        &self,
        id: Uuid,
        sender_id: &[u8],
        recipient_id: Option<&[u8]>,
        channel_id: Option<&[u8]>,
        content_type: &str,
        size: usize,
        encrypted: bool,
        blob_hash: Option<&[u8; 32]>,
        created_at: DateTime<Utc>,
    ) -> Result<()> {
        let blob_hash_slice: Option<&[u8]> = blob_hash.map(|h| h.as_slice());

        sqlx::query(
            r#"
            INSERT INTO messages (
                id, sender_id, recipient_id, channel_id,
                content_type, size, encrypted, blob_hash, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(id)
        .bind(sender_id)
        .bind(recipient_id)
        .bind(channel_id)
        .bind(content_type)
        .bind(size as i64)
        .bind(encrypted)
        .bind(blob_hash_slice)
        .bind(created_at)
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    /// Store blob reference in database
    async fn store_blob_ref(&self, blob_ref: &BlobRef) -> Result<()> {
        let hash_vec: Vec<u8> = blob_ref.hash.to_vec();
        sqlx::query(
            r#"
            INSERT INTO blob_refs (hash, size, codec, encryption_key_id, encrypted, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (hash) DO NOTHING
            "#,
        )
        .bind(&hash_vec)
        .bind(blob_ref.size as i64)
        .bind(blob_ref.codec.to_string())
        .bind(blob_ref.encryption_key_id.as_deref())
        .bind(blob_ref.encrypted)
        .bind(blob_ref.created_at)
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    /// Store blob location in database
    async fn store_blob_location(
        &self,
        blob_hash: &[u8; 32],
        location: &BlobLocation,
    ) -> Result<()> {
        let hash_vec: Vec<u8> = blob_hash.to_vec();
        let provider_vec: Vec<u8> = location.provider_id.to_vec();
        sqlx::query(
            r#"
            INSERT INTO blob_locations (
                blob_hash, provider_id, s3_key, s3_bucket, ipfs_cid,
                storage_class, region, status, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (blob_hash, provider_id) DO UPDATE SET
                s3_key = EXCLUDED.s3_key,
                s3_bucket = EXCLUDED.s3_bucket,
                ipfs_cid = EXCLUDED.ipfs_cid,
                storage_class = EXCLUDED.storage_class,
                status = EXCLUDED.status
            "#,
        )
        .bind(&hash_vec)
        .bind(&provider_vec)
        .bind(location.s3_key.as_deref())
        .bind(location.s3_bucket.as_deref())
        .bind(location.ipfs_cid.as_deref())
        .bind(location.storage_class.to_string())
        .bind(location.region.as_deref())
        .bind(location.status.to_string())
        .bind(location.created_at)
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    /// Update location access time
    async fn update_location_access(
        &self,
        blob_hash: &[u8; 32],
        provider_id: &[u8; 32],
    ) -> Result<()> {
        let hash_vec: Vec<u8> = blob_hash.to_vec();
        let provider_vec: Vec<u8> = provider_id.to_vec();
        sqlx::query(
            r#"
            UPDATE blob_locations
            SET last_accessed = NOW()
            WHERE blob_hash = $1 AND provider_id = $2
            "#,
        )
        .bind(&hash_vec)
        .bind(&provider_vec)
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    /// Update user quota
    async fn update_user_quota(
        &self,
        user_id: &[u8],
        inline_delta: i64,
        blob_delta: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE user_storage_quotas
            SET 
                inline_content_bytes = inline_content_bytes + $2,
                blob_count = blob_count + $3,
                used_bytes = inline_content_bytes + $2,
                last_updated = NOW()
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .bind(inline_delta)
        .bind(blob_delta)
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }

    /// Create storage receipt
    async fn create_receipt(
        &self,
        user_id: &[u8],
        provider_id: &[u8; 32],
        blob_hash: Option<&[u8; 32]>,
        operation: StorageOperation,
        amount: u64,
    ) -> Result<i32> {
        let provider_vec: Vec<u8> = provider_id.to_vec();
        let hash_vec: Option<Vec<u8>> = blob_hash.map(|h| h.to_vec());
        let row = sqlx::query(
            r#"
            INSERT INTO storage_receipts (
                user_id, provider_id, blob_hash, operation_type, amount
            )
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
        )
        .bind(user_id)
        .bind(&provider_vec)
        .bind(hash_vec.as_deref())
        .bind(operation.to_string())
        .bind(amount as i64)
        .fetch_one(&self.db_pool)
        .await?;

        let id: i32 = row.try_get("id")?;
        Ok(id)
    }

    /// Calculate storage cost for a provider
    fn calculate_storage_cost(&self, provider: &RegisteredProvider, size: u64) -> u64 {
        let price_per_gb = provider.capabilities.object_s3.price_per_gb_month;

        // Calculate cost in smallest units (assuming monthly pricing)
        // Convert bytes to GB and multiply by price
        let gb = size as f64 / (1024.0 * 1024.0 * 1024.0);
        (gb * price_per_gb as f64) as u64
    }

    /// Calculate retrieval cost for a provider
    fn calculate_retrieval_cost(&self, provider: &RegisteredProvider, size: u64) -> u64 {
        // Use archive retrieval price if available, otherwise use a fraction of storage price
        let price_per_gb = provider
            .capabilities
            .archive_object
            .as_ref()
            .map(|a| a.retrieval_price_per_gb)
            .unwrap_or_else(|| provider.capabilities.object_s3.price_per_gb_month / 10);

        let gb = size as f64 / (1024.0 * 1024.0 * 1024.0);
        (gb * price_per_gb as f64) as u64
    }

    /// Get the underlying router for direct access
    pub fn router(&self) -> Arc<StorageRouter> {
        Arc::clone(&self.router)
    }

    /// Get the underlying registry for direct access
    pub fn registry(&self) -> Arc<ProviderRegistry> {
        Arc::clone(&self.registry)
    }
}

impl std::fmt::Display for BlobCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlobCodec::Raw => write!(f, "raw"),
            BlobCodec::Cbor => write!(f, "cbor"),
            BlobCodec::Json => write!(f, "json"),
            BlobCodec::MessagePack => write!(f, "msgpack"),
            BlobCodec::Protobuf => write!(f, "protobuf"),
            BlobCodec::Zstd => write!(f, "zstd"),
            BlobCodec::Brotli => write!(f, "brotli"),
            BlobCodec::Lz4 => write!(f, "lz4"),
        }
    }
}

impl std::fmt::Display for LocationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocationStatus::Pending => write!(f, "Pending"),
            LocationStatus::Healthy => write!(f, "Healthy"),
            LocationStatus::Degraded => write!(f, "Degraded"),
            LocationStatus::Failed => write!(f, "Failed"),
            LocationStatus::Migrating => write!(f, "Migrating"),
            LocationStatus::Deleted => write!(f, "Deleted"),
        }
    }
}

impl std::fmt::Display for StorageClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageClass::Standard => write!(f, "Standard"),
            StorageClass::InfrequentAccess => write!(f, "InfrequentAccess"),
            StorageClass::Archive => write!(f, "Archive"),
            StorageClass::DeepArchive => write!(f, "DeepArchive"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_operation_display() {
        assert_eq!(StorageOperation::Store.to_string(), "store");
        assert_eq!(StorageOperation::Retrieve.to_string(), "retrieve");
        assert_eq!(StorageOperation::Replicate.to_string(), "replicate");
        assert_eq!(
            StorageOperation::ChallengeReward.to_string(),
            "challenge_reward"
        );
        assert_eq!(StorageOperation::Delete.to_string(), "delete");
    }

    #[test]
    fn test_default_config() {
        let config = StorageFacadeConfig::default();
        assert_eq!(config.inline_threshold, 64 * 1024);
        assert_eq!(config.default_replication.min_replicas, 2);
        assert!(config.enable_ipfs_pinning);
        assert!(config.enable_challenges);
        assert!(config.encrypt_at_rest);
        assert!(config.enable_receipts);
    }

    #[test]
    fn test_blob_codec_display() {
        assert_eq!(BlobCodec::Raw.to_string(), "raw");
        assert_eq!(BlobCodec::Zstd.to_string(), "zstd");
    }

    #[test]
    fn test_storage_class_display() {
        assert_eq!(StorageClass::Standard.to_string(), "Standard");
        assert_eq!(StorageClass::Archive.to_string(), "Archive");
    }

    #[test]
    fn test_location_status_display() {
        assert_eq!(LocationStatus::Healthy.to_string(), "Healthy");
        assert_eq!(LocationStatus::Pending.to_string(), "Pending");
    }
}
