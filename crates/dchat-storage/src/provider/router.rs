//! Storage Router Facade
//!
//! Provides a unified interface for tiered storage operations:
//! - CockroachDB: Primary storage for message metadata and small content
//! - SQLite: Offline cache for local access
//! - Redis: Hot cache layer
//! - S3/MinIO: Large blob storage
//! - IPFS: Optional content-addressed pinning
//!
//! Messages flow through this router which decides optimal storage tier
//! based on size, type, and provider selection.

use super::blob_ref::{BlobCodec, BlobLocation, BlobRef, LocationStatus, StorageClass};
use super::capabilities::ProviderCapability;
use super::challenges::{ChallengeConfig, StorageChallengeManager};
use super::registry::{ProviderRegistry, ProviderRegistryConfig, RegisteredProvider};
use super::selection::{ProviderSelector, ReplicationConfig, SelectionCriteria};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::error::{StorageError, StorageResult};

fn allow_localhost_chain_rpc_defaults() -> bool {
    matches!(
        std::env::var("DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS")
            .ok()
            .as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

fn is_localhost_rpc_endpoint(endpoint: &str) -> bool {
    let e = endpoint.trim().to_ascii_lowercase();
    e.starts_with("http://localhost")
        || e.starts_with("https://localhost")
        || e.contains("://127.0.0.1")
        || e.contains("://[::1]")
}

/// Size threshold for inline storage vs blob storage (64KB)
pub const INLINE_SIZE_THRESHOLD: usize = 64 * 1024;

/// Size threshold for Redis cache (4KB)
pub const CACHE_SIZE_THRESHOLD: usize = 4 * 1024;

/// Storage router configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRouterConfig {
    /// CockroachDB connection URL
    pub cockroach_url: String,
    /// SQLite database path (for offline cache)
    pub sqlite_path: PathBuf,
    /// Redis cluster URLs
    pub redis_urls: Vec<String>,
    /// Chain RPC endpoint
    pub chain_rpc_endpoint: String,
    /// Inline storage threshold (bytes)
    #[serde(default = "default_inline_threshold")]
    pub inline_threshold: usize,
    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
    /// Enable offline mode (SQLite only)
    #[serde(default)]
    pub offline_mode: bool,
    /// Replication configuration
    #[serde(default)]
    pub replication: ReplicationConfig,
    /// Challenge configuration
    #[serde(default)]
    pub challenges: ChallengeConfig,
    /// User's region (for provider selection)
    pub user_region: Option<String>,
}

fn default_inline_threshold() -> usize {
    INLINE_SIZE_THRESHOLD
}

fn default_cache_ttl() -> u64 {
    3600 // 1 hour
}

impl Default for StorageRouterConfig {
    fn default() -> Self {
        Self {
            cockroach_url: "postgresql://root@localhost:26257/dchat".to_string(),
            sqlite_path: PathBuf::from("dchat_offline.db"),
            redis_urls: vec!["redis://localhost:6379".to_string()],
            chain_rpc_endpoint: "http://localhost:8545".to_string(),
            inline_threshold: INLINE_SIZE_THRESHOLD,
            cache_ttl_secs: 3600,
            offline_mode: false,
            replication: ReplicationConfig::default(),
            challenges: ChallengeConfig::default(),
            user_region: None,
        }
    }
}

/// Message metadata stored in queryable database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// Message ID
    pub id: [u8; 32],
    /// Sender user ID
    pub sender_id: [u8; 32],
    /// Recipient user ID (for DMs) or channel ID
    pub recipient_id: [u8; 32],
    /// Message type
    pub message_type: MessageType,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Inline content (if small enough) - encrypted
    pub inline_content: Option<Vec<u8>>,
    /// Blob reference (if content is large)
    pub blob_ref: Option<BlobRef>,
    /// Whether content is encrypted
    pub encrypted: bool,
    /// Encryption key ID (for key lookup)
    pub encryption_key_id: Option<[u8; 32]>,
    /// Reply to message ID
    pub reply_to: Option<[u8; 32]>,
    /// Edit timestamp
    pub edited_at: Option<DateTime<Utc>>,
    /// Deletion timestamp
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Message type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// Direct message
    Direct,
    /// Channel message
    Channel,
    /// System message
    System,
    /// Attachment only
    Attachment,
}

/// Circuit breaker state for storage backends
#[derive(Debug)]
pub struct StorageCircuitBreaker {
    /// Number of consecutive failures
    failure_count: AtomicU32,
    /// Total failures since last reset
    total_failures: AtomicU64,
    /// Total successes since last reset  
    total_successes: AtomicU64,
    /// Last failure time
    last_failure: parking_lot::RwLock<Option<Instant>>,
    /// Circuit state: 0=closed, 1=open, 2=half-open
    state: AtomicU32,
    /// Failure threshold before opening circuit
    failure_threshold: u32,
    /// Reset timeout before trying again
    reset_timeout: Duration,
    /// Backend name for logging
    name: String,
}

impl StorageCircuitBreaker {
    pub fn new(name: &str, failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            failure_count: AtomicU32::new(0),
            total_failures: AtomicU64::new(0),
            total_successes: AtomicU64::new(0),
            last_failure: parking_lot::RwLock::new(None),
            state: AtomicU32::new(0), // closed
            failure_threshold,
            reset_timeout,
            name: name.to_string(),
        }
    }

    /// Check if circuit allows request
    pub fn should_allow(&self) -> bool {
        let state = self.state.load(Ordering::SeqCst);
        match state {
            0 => true, // closed - allow
            1 => {
                // open - check if reset timeout has passed
                let last_failure = self.last_failure.read();
                if let Some(last) = *last_failure {
                    if last.elapsed() > self.reset_timeout {
                        // Transition to half-open
                        drop(last_failure); // Release read lock before writing
                        self.state.store(2, Ordering::SeqCst);
                        info!("{} circuit breaker transitioning to half-open", self.name);
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }
            }
            2 => true, // half-open - allow limited requests
            _ => false,
        }
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        self.total_successes.fetch_add(1, Ordering::Relaxed);
        self.failure_count.store(0, Ordering::SeqCst);

        let state = self.state.load(Ordering::SeqCst);
        if state != 0 {
            info!("{} circuit breaker closed after success", self.name);
            self.state.store(0, Ordering::SeqCst);
        }
    }

    /// Record a failed operation
    pub fn record_failure(&self) {
        self.total_failures.fetch_add(1, Ordering::Relaxed);
        let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;

        *self.last_failure.write() = Some(Instant::now());

        if failures >= self.failure_threshold {
            let state = self.state.load(Ordering::SeqCst);
            if state != 1 {
                warn!(
                    "{} circuit breaker opened after {} failures",
                    self.name, failures
                );
                self.state.store(1, Ordering::SeqCst);
            }
        }
    }

    /// Get circuit state as string
    pub fn state_string(&self) -> &'static str {
        match self.state.load(Ordering::SeqCst) {
            0 => "closed",
            1 => "open",
            2 => "half-open",
            _ => "unknown",
        }
    }
}

/// Storage router providing tiered storage access
pub struct StorageRouter {
    /// Configuration
    config: StorageRouterConfig,
    /// CockroachDB pool (primary)
    cockroach_pool: Option<PgPool>,
    /// SQLite pool (offline cache)
    sqlite_pool: Option<SqlitePool>,
    /// Redis client (cache)
    redis_client: Option<redis::Client>,
    /// Provider registry
    registry: Arc<ProviderRegistry>,
    /// Provider selector
    selector: ProviderSelector,
    /// Challenge manager
    challenge_manager: Option<Arc<StorageChallengeManager>>,
    /// HTTP client for IPFS gateway operations
    http_client: reqwest::Client,
    /// Local blob cache
    blob_cache: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>,
    /// Circuit breaker for S3/MinIO operations
    s3_circuit_breaker: Arc<StorageCircuitBreaker>,
    /// Circuit breaker for IPFS operations
    ipfs_circuit_breaker: Arc<StorageCircuitBreaker>,
}

impl StorageRouter {
    /// Create a new storage router
    pub async fn new(config: StorageRouterConfig) -> StorageResult<Self> {
        if !config.offline_mode {
            if config.chain_rpc_endpoint.trim().is_empty() {
                return Err(StorageError::Config(
                    "Storage router requires `chain_rpc_endpoint` when offline_mode=false"
                        .to_string(),
                ));
            }

            if is_localhost_rpc_endpoint(&config.chain_rpc_endpoint)
                && !allow_localhost_chain_rpc_defaults()
            {
                return Err(StorageError::Config(
                    "Refusing to use localhost chain RPC endpoint for storage router. Configure a non-localhost RPC endpoint, or (DEV ONLY) set DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1.".to_string(),
                ));
            }
        }

        let registry_config = ProviderRegistryConfig {
            database_url: config.cockroach_url.clone(),
            chain_rpc_endpoint: config.chain_rpc_endpoint.clone(),
            offline_mode: config.offline_mode,
            ..Default::default()
        };
        let registry = Arc::new(ProviderRegistry::new(registry_config).await?);

        let selector = ProviderSelector::new(registry.clone());

        let cockroach_pool = if !config.offline_mode {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(10)
                .connect(&config.cockroach_url)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;

            Self::init_cockroach_schema(&pool).await?;
            Some(pool)
        } else {
            None
        };

        let sqlite_pool = {
            let url = format!("sqlite:{}?mode=rwc", config.sqlite_path.display());
            let pool = SqlitePool::connect(&url)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;

            Self::init_sqlite_schema(&pool).await?;
            Some(pool)
        };

        let redis_client = if !config.offline_mode && !config.redis_urls.is_empty() {
            redis::Client::open(config.redis_urls[0].as_str()).ok()
        } else {
            None
        };

        let challenge_manager = if !config.offline_mode {
            let mut challenge_config = config.challenges.clone();
            challenge_config.offline_mode = config.offline_mode;

            Some(Arc::new(
                StorageChallengeManager::new(
                    registry.clone(),
                    &config.cockroach_url,
                    &config.chain_rpc_endpoint,
                    challenge_config,
                )
                .await?,
            ))
        } else {
            None
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(10))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .pool_max_idle_per_host(10)
            .build()
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        // Initialize circuit breakers for external storage backends
        let s3_circuit_breaker = Arc::new(StorageCircuitBreaker::new(
            "s3",
            5,                       // 5 consecutive failures to open
            Duration::from_secs(30), // 30s before trying again
        ));
        let ipfs_circuit_breaker = Arc::new(StorageCircuitBreaker::new(
            "ipfs",
            3,                       // 3 consecutive failures to open (IPFS is less reliable)
            Duration::from_secs(60), // 60s before trying again
        ));

        info!(
            "Storage router initialized (offline_mode={})",
            config.offline_mode
        );

        Ok(Self {
            config,
            cockroach_pool,
            sqlite_pool,
            redis_client,
            registry,
            selector,
            challenge_manager,
            http_client,
            blob_cache: Arc::new(RwLock::new(HashMap::new())),
            s3_circuit_breaker,
            ipfs_circuit_breaker,
        })
    }

    /// Create router in offline mode (SQLite only)
    pub async fn offline(sqlite_path: PathBuf) -> StorageResult<Self> {
        let config = StorageRouterConfig {
            sqlite_path,
            offline_mode: true,
            ..Default::default()
        };

        Self::new(config).await
    }

    /// Initialize CockroachDB schema
    async fn init_cockroach_schema(pool: &PgPool) -> StorageResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id BYTEA PRIMARY KEY,
                sender_id BYTEA NOT NULL,
                recipient_id BYTEA NOT NULL,
                message_type TEXT NOT NULL,
                timestamp TIMESTAMPTZ NOT NULL,
                inline_content BYTEA,
                blob_ref JSONB,
                encrypted BOOLEAN NOT NULL DEFAULT true,
                encryption_key_id BYTEA,
                reply_to BYTEA,
                edited_at TIMESTAMPTZ,
                deleted_at TIMESTAMPTZ,
                INDEX idx_sender (sender_id),
                INDEX idx_recipient (recipient_id),
                INDEX idx_timestamp (timestamp DESC)
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS blob_refs (
                hash BYTEA PRIMARY KEY,
                size BIGINT NOT NULL,
                codec TEXT NOT NULL,
                mime_type TEXT,
                encrypted BOOLEAN NOT NULL,
                locations JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL,
                last_verified TIMESTAMPTZ
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        info!("CockroachDB schema initialized");
        Ok(())
    }

    /// Initialize SQLite schema (offline cache)
    async fn init_sqlite_schema(pool: &SqlitePool) -> StorageResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cached_messages (
                id BLOB PRIMARY KEY,
                sender_id BLOB NOT NULL,
                recipient_id BLOB NOT NULL,
                message_type TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                inline_content BLOB,
                blob_ref TEXT,
                encrypted INTEGER NOT NULL DEFAULT 1,
                encryption_key_id BLOB,
                reply_to BLOB,
                edited_at TEXT,
                deleted_at TEXT,
                synced INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_cached_sender ON cached_messages(sender_id)
            "#,
        )
        .execute(pool)
        .await
        .ok();

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_cached_recipient ON cached_messages(recipient_id)
            "#,
        )
        .execute(pool)
        .await
        .ok();

        info!("SQLite schema initialized");
        Ok(())
    }

    /// Store a message with automatic tiering
    pub async fn store_message(
        &self,
        sender_id: [u8; 32],
        recipient_id: [u8; 32],
        message_type: MessageType,
        content: Vec<u8>,
        encrypted: bool,
        encryption_key_id: Option<[u8; 32]>,
        reply_to: Option<[u8; 32]>,
    ) -> StorageResult<MessageMetadata> {
        // Generate message ID
        let mut hasher = Sha256::new();
        hasher.update(&sender_id);
        hasher.update(&recipient_id);
        hasher.update(&Utc::now().timestamp_nanos_opt().unwrap_or(0).to_le_bytes());
        hasher.update(&content);
        let id: [u8; 32] = hasher.finalize().into();

        let timestamp = Utc::now();

        // Decide storage tier based on size
        let (inline_content, blob_ref) = if content.len() <= self.config.inline_threshold {
            // Small content - store inline
            (Some(content), None)
        } else {
            // Large content - store as blob
            let blob = self.store_blob(content, encrypted).await?;
            (None, Some(blob))
        };

        let metadata = MessageMetadata {
            id,
            sender_id,
            recipient_id,
            message_type,
            timestamp,
            inline_content,
            blob_ref,
            encrypted,
            encryption_key_id,
            reply_to,
            edited_at: None,
            deleted_at: None,
        };

        // Store in appropriate tiers
        if let Some(pool) = &self.cockroach_pool {
            self.store_message_cockroach(pool, &metadata).await?;
        }

        if let Some(pool) = &self.sqlite_pool {
            self.store_message_sqlite(pool, &metadata).await?;
        }

        // Cache in Redis if small enough
        if metadata
            .inline_content
            .as_ref()
            .map(|c| c.len())
            .unwrap_or(0)
            <= CACHE_SIZE_THRESHOLD
        {
            self.cache_message(&metadata).await;
        }

        debug!("Stored message {}", hex::encode(id));
        Ok(metadata)
    }

    /// Store message in CockroachDB
    async fn store_message_cockroach(
        &self,
        pool: &PgPool,
        metadata: &MessageMetadata,
    ) -> StorageResult<()> {
        let blob_ref_json = metadata
            .blob_ref
            .as_ref()
            .map(|b| serde_json::to_string(b).unwrap_or_default());

        sqlx::query(
            r#"
            INSERT INTO messages
            (id, sender_id, recipient_id, message_type, timestamp, inline_content,
             blob_ref, encrypted, encryption_key_id, reply_to, edited_at, deleted_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (id) DO UPDATE SET
                edited_at = EXCLUDED.edited_at,
                deleted_at = EXCLUDED.deleted_at
            "#,
        )
        .bind(&metadata.id[..])
        .bind(&metadata.sender_id[..])
        .bind(&metadata.recipient_id[..])
        .bind(format!("{:?}", metadata.message_type))
        .bind(metadata.timestamp)
        .bind(&metadata.inline_content)
        .bind(blob_ref_json)
        .bind(metadata.encrypted)
        .bind(metadata.encryption_key_id.as_ref().map(|k| &k[..]))
        .bind(metadata.reply_to.as_ref().map(|r| &r[..]))
        .bind(metadata.edited_at)
        .bind(metadata.deleted_at)
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    /// Store message in SQLite (offline cache)
    async fn store_message_sqlite(
        &self,
        pool: &SqlitePool,
        metadata: &MessageMetadata,
    ) -> StorageResult<()> {
        let blob_ref_json = metadata
            .blob_ref
            .as_ref()
            .map(|b| serde_json::to_string(b).unwrap_or_default());

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO cached_messages
            (id, sender_id, recipient_id, message_type, timestamp, inline_content,
             blob_ref, encrypted, encryption_key_id, reply_to, edited_at, deleted_at, synced)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
            "#,
        )
        .bind(&metadata.id[..])
        .bind(&metadata.sender_id[..])
        .bind(&metadata.recipient_id[..])
        .bind(format!("{:?}", metadata.message_type))
        .bind(metadata.timestamp.to_rfc3339())
        .bind(&metadata.inline_content)
        .bind(blob_ref_json)
        .bind(metadata.encrypted)
        .bind(metadata.encryption_key_id.as_ref().map(|k| k.to_vec()))
        .bind(metadata.reply_to.as_ref().map(|r| r.to_vec()))
        .bind(metadata.edited_at.map(|t| t.to_rfc3339()))
        .bind(metadata.deleted_at.map(|t| t.to_rfc3339()))
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    /// Cache message in Redis
    async fn cache_message(&self, metadata: &MessageMetadata) {
        let Some(client) = &self.redis_client else {
            return;
        };

        let Ok(mut conn) = client.get_connection() else {
            return;
        };

        let key = format!("msg:{}", hex::encode(metadata.id));
        let value = serde_json::to_string(metadata).unwrap_or_default();

        let _: Result<(), _> = redis::cmd("SETEX")
            .arg(&key)
            .arg(self.config.cache_ttl_secs)
            .arg(&value)
            .query(&mut conn);
    }

    /// Store a blob with replication
    pub async fn store_blob(&self, content: Vec<u8>, encrypted: bool) -> StorageResult<BlobRef> {
        // Create initial blob ref
        let codec = if encrypted {
            BlobCodec::Raw
        } else {
            BlobCodec::Raw
        };

        let mut blob_ref =
            BlobRef::from_content(&content, codec, "application/octet-stream", encrypted);

        // Select providers for replication
        let criteria = SelectionCriteria {
            required_size: content.len() as u64,
            required_capabilities: vec![ProviderCapability::ObjectS3],
            user_region: self.config.user_region.clone(),
            max_price_per_gb: None,
            min_reputation: Some(0.5),
            replication: self.config.replication.clone(),
        };

        let selection = self.selector.select_providers(&criteria).await?;

        // Upload to each selected provider (S3 primary)
        for provider in selection
            .primary_providers
            .iter()
            .chain(selection.backup_providers.iter())
        {
            // Upload to S3 (primary storage)
            match self
                .upload_to_provider(&provider.provider, &content, &blob_ref)
                .await
            {
                Ok(mut location) => {
                    // Also pin to IPFS if provider supports it
                    if provider.provider.capabilities.ipfs_pinning.is_some() {
                        match self
                            .upload_to_ipfs(&provider.provider, &content, &blob_ref)
                            .await
                        {
                            Ok(ipfs_location) => {
                                // Merge IPFS CID into S3 location for hybrid access
                                location.ipfs_cid = ipfs_location.ipfs_cid;
                                info!(
                                    "Blob {} also pinned to IPFS via provider {}",
                                    blob_ref.hash_hex(),
                                    hex::encode(provider.provider.id)
                                );
                            }
                            Err(e) => {
                                warn!(
                                    "IPFS pinning failed for blob {} (S3 succeeded): {}",
                                    blob_ref.hash_hex(),
                                    e
                                );
                            }
                        }
                    }
                    blob_ref.add_location(location);
                }
                Err(e) => {
                    warn!(
                        "Failed to upload blob {} to provider {}: {}",
                        blob_ref.hash_hex(),
                        hex::encode(provider.provider.id),
                        e
                    );
                }
            }
        }

        // Ensure minimum replication
        if blob_ref.healthy_locations().len() < self.config.replication.min_replicas as usize {
            return Err(StorageError::Replication(format!(
                "Failed to achieve minimum replication: got {}, need {}",
                blob_ref.healthy_locations().len(),
                self.config.replication.min_replicas
            )));
        }

        // Persist blob ref to database
        if let Some(pool) = &self.cockroach_pool {
            self.persist_blob_ref(pool, &blob_ref).await?;
        }

        // Cache locally
        {
            let mut cache = self.blob_cache.write().await;
            cache.insert(blob_ref.hash, content);
        }

        info!(
            "Stored blob {} ({} bytes) to {} providers (IPFS: {})",
            blob_ref.hash_hex(),
            blob_ref.size,
            blob_ref.locations.len(),
            blob_ref.has_ipfs()
        );

        Ok(blob_ref)
    }

    /// Upload blob to a specific provider with circuit breaker protection
    async fn upload_to_provider(
        &self,
        provider: &RegisteredProvider,
        content: &[u8],
        blob_ref: &BlobRef,
    ) -> StorageResult<BlobLocation> {
        // Check circuit breaker before attempting S3 upload
        if !self.s3_circuit_breaker.should_allow() {
            error!(
                provider_id = %hex::encode(provider.id),
                circuit_state = %self.s3_circuit_breaker.state_string(),
                "S3 circuit breaker is open, rejecting upload request"
            );
            return Err(StorageError::Provider(format!(
                "S3 circuit breaker is open (state: {}), skipping provider {}",
                self.s3_circuit_breaker.state_string(),
                hex::encode(provider.id)
            )));
        }

        let s3_cap = &provider.capabilities.object_s3;

        // Generate S3 key
        let s3_key = format!(
            "blobs/{}/{}",
            &blob_ref.hash_hex()[..4],
            blob_ref.hash_hex()
        );

        // Upload to S3 using rust-s3 0.35 API
        let region = s3::Region::Custom {
            region: s3_cap.region.clone(),
            endpoint: s3_cap.endpoint.clone(),
        };

        let credentials = s3::creds::Credentials::new(
            Some(&s3_cap.auth.access_key_id),
            Some(&s3_cap.auth.secret_access_key),
            s3_cap.auth.session_token.as_deref(),
            None,
            None,
        )
        .map_err(|e| {
            self.s3_circuit_breaker.record_failure();
            error!(
                provider_id = %hex::encode(provider.id),
                bucket = %s3_cap.bucket,
                error = %e,
                "S3 credentials initialization failed"
            );
            StorageError::ObjectStorage(e.to_string())
        })?;

        let bucket = s3::Bucket::new(&s3_cap.bucket, region, credentials).map_err(|e| {
            self.s3_circuit_breaker.record_failure();
            error!(
                provider_id = %hex::encode(provider.id),
                bucket = %s3_cap.bucket,
                region = %s3_cap.region,
                error = %e,
                "S3 bucket initialization failed"
            );
            StorageError::ObjectStorage(e.to_string())
        })?;

        let response = bucket.put_object(&s3_key, content).await.map_err(|e| {
            self.s3_circuit_breaker.record_failure();
            error!(
                provider_id = %hex::encode(provider.id),
                bucket = %s3_cap.bucket,
                s3_key = %s3_key,
                content_size = %content.len(),
                error = %e,
                "S3 put_object failed"
            );
            StorageError::ObjectStorage(e.to_string())
        })?;

        if response.status_code() != 200 {
            self.s3_circuit_breaker.record_failure();
            error!(
                provider_id = %hex::encode(provider.id),
                bucket = %s3_cap.bucket,
                s3_key = %s3_key,
                status_code = %response.status_code(),
                "S3 upload returned non-200 status"
            );
            return Err(StorageError::ObjectStorage(format!(
                "S3 upload failed with status {}",
                response.status_code()
            )));
        }

        // Success - record it
        self.s3_circuit_breaker.record_success();

        // Create location
        let location = BlobLocation {
            provider_id: provider.id,
            s3_key: Some(s3_key),
            s3_bucket: Some(s3_cap.bucket.clone()),
            ipfs_cid: None,
            archive_key: None,
            storage_class: StorageClass::Standard,
            region: Some(s3_cap.region.clone()),
            status: LocationStatus::Healthy,
            created_at: Utc::now(),
            last_accessed: Some(Utc::now()),
            last_challenged: None,
            failed_challenges: 0,
        };

        Ok(location)
    }

    /// Upload blob to IPFS via HTTP API with circuit breaker protection
    /// Returns the CID (Content Identifier) of the pinned content
    async fn upload_to_ipfs(
        &self,
        provider: &RegisteredProvider,
        content: &[u8],
        blob_ref: &BlobRef,
    ) -> StorageResult<BlobLocation> {
        // Check circuit breaker before attempting IPFS upload
        if !self.ipfs_circuit_breaker.should_allow() {
            error!(
                provider_id = %hex::encode(provider.id),
                circuit_state = %self.ipfs_circuit_breaker.state_string(),
                "IPFS circuit breaker is open, rejecting upload request"
            );
            return Err(StorageError::Provider(format!(
                "IPFS circuit breaker is open (state: {}), skipping provider {}",
                self.ipfs_circuit_breaker.state_string(),
                hex::encode(provider.id)
            )));
        }

        let ipfs_cap =
            provider.capabilities.ipfs_pinning.as_ref().ok_or_else(|| {
                StorageError::Provider("Provider does not support IPFS".to_string())
            })?;

        // Check size limits
        if content.len() as u64 > ipfs_cap.max_pin_size {
            return Err(StorageError::Provider(format!(
                "Content size {} exceeds IPFS max pin size {}",
                content.len(),
                ipfs_cap.max_pin_size
            )));
        }

        // Build multipart form for IPFS add
        let form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(content.to_vec())
                .file_name(blob_ref.hash_hex())
                .mime_str(&blob_ref.mime_type)
                .map_err(|e| StorageError::Internal(e.to_string()))?,
        );

        // Determine API endpoint based on pinning API type
        let add_url = match ipfs_cap.pinning_api {
            super::capabilities::PinningApiType::Standard => {
                format!(
                    "{}/api/v0/add?pin=true&cid-version=1",
                    ipfs_cap.api_endpoint
                )
            }
            super::capabilities::PinningApiType::PinningServicesApi => {
                format!("{}/pins", ipfs_cap.api_endpoint)
            }
            super::capabilities::PinningApiType::Pinata => {
                format!("{}/pinning/pinFileToIPFS", ipfs_cap.api_endpoint)
            }
            super::capabilities::PinningApiType::Infura => {
                format!("{}/api/v0/add?pin=true", ipfs_cap.api_endpoint)
            }
            super::capabilities::PinningApiType::Web3Storage => {
                format!("{}/upload", ipfs_cap.api_endpoint)
            }
        };

        // Build request with auth
        let mut request = self.http_client.post(&add_url).multipart(form);

        // Add authentication header if token provided
        if !ipfs_cap.auth_token.is_empty() {
            match ipfs_cap.pinning_api {
                super::capabilities::PinningApiType::Pinata => {
                    request =
                        request.header("Authorization", format!("Bearer {}", ipfs_cap.auth_token));
                }
                super::capabilities::PinningApiType::Web3Storage => {
                    request =
                        request.header("Authorization", format!("Bearer {}", ipfs_cap.auth_token));
                }
                super::capabilities::PinningApiType::Infura => {
                    // Infura uses project_id:project_secret as basic auth
                    request =
                        request.header("Authorization", format!("Basic {}", ipfs_cap.auth_token));
                }
                _ => {
                    request =
                        request.header("Authorization", format!("Bearer {}", ipfs_cap.auth_token));
                }
            }
        }

        // Execute request
        let response = request.send().await.map_err(|e| {
            self.ipfs_circuit_breaker.record_failure();
            error!(
                provider_id = %hex::encode(provider.id),
                api_endpoint = %ipfs_cap.api_endpoint,
                content_size = %content.len(),
                error = %e,
                "IPFS upload request failed"
            );
            StorageError::ObjectStorage(format!("IPFS upload failed: {}", e))
        })?;

        if !response.status().is_success() {
            self.ipfs_circuit_breaker.record_failure();
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(
                provider_id = %hex::encode(provider.id),
                api_endpoint = %ipfs_cap.api_endpoint,
                status = %status,
                response_body = %body,
                "IPFS upload returned error status"
            );
            return Err(StorageError::ObjectStorage(format!(
                "IPFS upload failed with status {}: {}",
                status, body
            )));
        }

        // Parse response to get CID
        let body = response.text().await.map_err(|e| {
            self.ipfs_circuit_breaker.record_failure();
            error!(
                provider_id = %hex::encode(provider.id),
                error = %e,
                "Failed to read IPFS response body"
            );
            StorageError::ObjectStorage(format!("Failed to read IPFS response: {}", e))
        })?;

        // Standard IPFS returns: {"Name":"...","Hash":"Qm...","Size":"..."}
        // Parse CID from response
        let cid = self.parse_ipfs_cid_response(&body, ipfs_cap.pinning_api)?;

        // Success - record it
        self.ipfs_circuit_breaker.record_success();

        info!(
            "Blob {} pinned to IPFS as CID {} via provider {}",
            blob_ref.hash_hex(),
            cid,
            hex::encode(provider.id)
        );

        // Create location with IPFS CID
        let location = BlobLocation {
            provider_id: provider.id,
            s3_key: None,
            s3_bucket: None,
            ipfs_cid: Some(cid),
            archive_key: None,
            storage_class: StorageClass::Standard,
            region: None, // IPFS is location-agnostic
            status: LocationStatus::Healthy,
            created_at: Utc::now(),
            last_accessed: Some(Utc::now()),
            last_challenged: None,
            failed_challenges: 0,
        };

        Ok(location)
    }

    /// Parse CID from IPFS API response based on API type
    fn parse_ipfs_cid_response(
        &self,
        body: &str,
        api_type: super::capabilities::PinningApiType,
    ) -> StorageResult<String> {
        match api_type {
            super::capabilities::PinningApiType::Standard
            | super::capabilities::PinningApiType::Infura => {
                // Standard IPFS: {"Name":"file","Hash":"Qm...","Size":"123"}
                #[derive(Deserialize)]
                struct IpfsAddResponse {
                    #[serde(rename = "Hash")]
                    hash: String,
                }
                let resp: IpfsAddResponse = serde_json::from_str(body)
                    .map_err(|e| StorageError::Internal(format!("Invalid IPFS response: {}", e)))?;
                Ok(resp.hash)
            }
            super::capabilities::PinningApiType::PinningServicesApi => {
                // PSA: {"requestid":"...","status":"pinned","pin":{"cid":"bafy..."}}
                #[derive(Deserialize)]
                struct PsaPin {
                    cid: String,
                }
                #[derive(Deserialize)]
                struct PsaResponse {
                    pin: PsaPin,
                }
                let resp: PsaResponse = serde_json::from_str(body)
                    .map_err(|e| StorageError::Internal(format!("Invalid PSA response: {}", e)))?;
                Ok(resp.pin.cid)
            }
            super::capabilities::PinningApiType::Pinata => {
                // Pinata: {"IpfsHash":"Qm...","PinSize":123,"Timestamp":"..."}
                #[derive(Deserialize)]
                struct PinataResponse {
                    #[serde(rename = "IpfsHash")]
                    ipfs_hash: String,
                }
                let resp: PinataResponse = serde_json::from_str(body).map_err(|e| {
                    StorageError::Internal(format!("Invalid Pinata response: {}", e))
                })?;
                Ok(resp.ipfs_hash)
            }
            super::capabilities::PinningApiType::Web3Storage => {
                // Web3.Storage: {"cid":"bafy...","carCid":"..."}
                #[derive(Deserialize)]
                struct Web3Response {
                    cid: String,
                }
                let resp: Web3Response = serde_json::from_str(body).map_err(|e| {
                    StorageError::Internal(format!("Invalid Web3.Storage response: {}", e))
                })?;
                Ok(resp.cid)
            }
        }
    }

    /// Download blob from IPFS via gateway using http_client
    async fn download_from_ipfs(&self, location: &BlobLocation) -> StorageResult<Vec<u8>> {
        let cid = location
            .ipfs_cid
            .as_ref()
            .ok_or_else(|| StorageError::NotFound("Location missing IPFS CID".to_string()))?;

        // Get provider's gateway URL
        let provider = self
            .registry
            .get_provider(&location.provider_id)
            .await?
            .ok_or_else(|| StorageError::NotFound("Provider not found".to_string()))?;

        let ipfs_cap =
            provider.capabilities.ipfs_pinning.as_ref().ok_or_else(|| {
                StorageError::Provider("Provider does not support IPFS".to_string())
            })?;

        // Construct gateway URL
        let gateway_url = format!(
            "{}/ipfs/{}",
            ipfs_cap.gateway_url.trim_end_matches('/'),
            cid
        );

        debug!("Downloading from IPFS gateway: {}", gateway_url);

        // Download via HTTP
        let response = self
            .http_client
            .get(&gateway_url)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| StorageError::ObjectStorage(format!("IPFS download failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(StorageError::ObjectStorage(format!(
                "IPFS gateway returned status {}",
                response.status()
            )));
        }

        let content = response.bytes().await.map_err(|e| {
            StorageError::ObjectStorage(format!("Failed to read IPFS response: {}", e))
        })?;

        info!("Downloaded {} bytes from IPFS CID {}", content.len(), cid);

        Ok(content.to_vec())
    }

    /// Persist blob ref to CockroachDB
    async fn persist_blob_ref(&self, pool: &PgPool, blob_ref: &BlobRef) -> StorageResult<()> {
        let locations_json = serde_json::to_string(&blob_ref.locations)
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO blob_refs (hash, size, codec, mime_type, encrypted, locations, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (hash) DO UPDATE SET
                locations = EXCLUDED.locations,
                last_verified = $8
            "#,
        )
        .bind(&blob_ref.hash[..])
        .bind(blob_ref.size as i64)
        .bind(format!("{:?}", blob_ref.codec))
        .bind(&blob_ref.mime_type)
        .bind(blob_ref.encrypted)
        .bind(locations_json)
        .bind(blob_ref.created_at)
        .bind(Utc::now())
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    /// Retrieve a blob by hash
    pub async fn retrieve_blob(&self, hash: &[u8; 32]) -> StorageResult<Vec<u8>> {
        // Check local cache first
        {
            let cache = self.blob_cache.read().await;
            if let Some(content) = cache.get(hash) {
                debug!("Blob {} found in local cache", hex::encode(hash));
                return Ok(content.clone());
            }
        }

        // Get blob ref from database
        let blob_ref = self.get_blob_ref(hash).await?;

        // Try to retrieve from providers
        for location in &blob_ref.locations {
            if location.status != LocationStatus::Healthy {
                continue;
            }

            match self.download_from_location(&location).await {
                Ok(content) => {
                    // Verify hash
                    let mut hasher = Sha256::new();
                    hasher.update(&content);
                    let computed_hash: [u8; 32] = hasher.finalize().into();

                    if computed_hash != blob_ref.hash {
                        warn!(
                            "Hash mismatch for blob {} from provider {}",
                            hex::encode(hash),
                            hex::encode(location.provider_id)
                        );
                        continue;
                    }

                    // Cache locally
                    {
                        let mut cache = self.blob_cache.write().await;
                        cache.insert(*hash, content.clone());
                    }

                    return Ok(content);
                }
                Err(e) => {
                    warn!(
                        "Failed to download blob {} from provider {}: {}",
                        hex::encode(hash),
                        hex::encode(location.provider_id),
                        e
                    );
                }
            }
        }

        Err(StorageError::NotFound(format!(
            "Blob {} not retrievable from any provider",
            hex::encode(hash)
        )))
    }

    /// Get blob ref from database
    async fn get_blob_ref(&self, hash: &[u8; 32]) -> StorageResult<BlobRef> {
        let pool = self
            .cockroach_pool
            .as_ref()
            .ok_or_else(|| StorageError::Offline("CockroachDB not available".to_string()))?;

        let row = sqlx::query(
            "SELECT hash, size, codec, mime_type, encrypted, locations, created_at FROM blob_refs WHERE hash = $1",
        )
        .bind(&hash[..])
        .fetch_optional(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?
        .ok_or_else(|| StorageError::NotFound("Blob not found".to_string()))?;

        let hash_bytes: Vec<u8> = row.get("hash");
        let size: i64 = row.get("size");
        let codec_str: String = row.get("codec");
        let mime_type: Option<String> = row.get("mime_type");
        let encrypted: bool = row.get("encrypted");
        let locations_json: String = row.get("locations");
        let created_at: DateTime<Utc> = row.get("created_at");

        let hash: [u8; 32] = hash_bytes
            .try_into()
            .map_err(|_| StorageError::Internal("Invalid hash".to_string()))?;
        let codec = match codec_str.as_str() {
            "Raw" => BlobCodec::Raw,
            "Cbor" => BlobCodec::Cbor,
            "Json" => BlobCodec::Json,
            "Zstd" => BlobCodec::Zstd,
            "Brotli" => BlobCodec::Brotli,
            "Lz4" => BlobCodec::Lz4,
            _ => BlobCodec::Raw,
        };
        let locations: Vec<BlobLocation> = serde_json::from_str(&locations_json)
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        Ok(BlobRef {
            hash,
            size: size as u64,
            codec,
            mime_type: mime_type.unwrap_or_else(|| "application/octet-stream".to_string()),
            encrypted,
            encryption_key_id: None,
            locations,
            created_at,
            last_verified: None,
            metadata: std::collections::HashMap::new(),
        })
    }

    /// Download blob from a location (S3 or IPFS)
    async fn download_from_location(&self, location: &BlobLocation) -> StorageResult<Vec<u8>> {
        // Try IPFS first if CID is available
        if location.ipfs_cid.is_some() {
            match self.download_from_ipfs(location).await {
                Ok(content) => return Ok(content),
                Err(e) => {
                    warn!(
                        "IPFS download failed for CID {:?}, falling back to S3: {}",
                        location.ipfs_cid, e
                    );
                }
            }
        }

        // Fall back to S3
        let s3_key = match &location.s3_key {
            Some(key) => key,
            None => {
                return Err(StorageError::NotFound(
                    "Location has neither IPFS CID nor S3 key".to_string(),
                ));
            }
        };

        // Get provider
        let provider = self
            .registry
            .get_provider(&location.provider_id)
            .await?
            .ok_or_else(|| StorageError::NotFound("Provider not found".to_string()))?;

        let s3_cap = &provider.capabilities.object_s3;

        // Download from S3 using rust-s3 0.35 API
        let region = s3::Region::Custom {
            region: s3_cap.region.clone(),
            endpoint: s3_cap.endpoint.clone(),
        };

        let credentials = s3::creds::Credentials::new(
            Some(&s3_cap.auth.access_key_id),
            Some(&s3_cap.auth.secret_access_key),
            s3_cap.auth.session_token.as_deref(),
            None,
            None,
        )
        .map_err(|e| StorageError::ObjectStorage(e.to_string()))?;

        let bucket = s3::Bucket::new(&s3_cap.bucket, region, credentials)
            .map_err(|e| StorageError::ObjectStorage(e.to_string()))?;

        let response = bucket
            .get_object(s3_key)
            .await
            .map_err(|e| StorageError::ObjectStorage(e.to_string()))?;

        if response.status_code() != 200 {
            return Err(StorageError::ObjectStorage(format!(
                "S3 download failed with status {}",
                response.status_code()
            )));
        }

        Ok(response.to_vec())
    }

    /// Get message by ID
    pub async fn get_message(&self, id: &[u8; 32]) -> StorageResult<Option<MessageMetadata>> {
        // Try Redis cache first
        if let Some(client) = &self.redis_client {
            if let Ok(mut conn) = client.get_connection() {
                let key = format!("msg:{}", hex::encode(id));
                let result: Result<String, _> = redis::cmd("GET").arg(&key).query(&mut conn);
                if let Ok(value) = result {
                    if let Ok(metadata) = serde_json::from_str(&value) {
                        return Ok(Some(metadata));
                    }
                }
            }
        }

        // Try CockroachDB
        if let Some(pool) = &self.cockroach_pool {
            if let Some(metadata) = self.get_message_cockroach(pool, id).await? {
                return Ok(Some(metadata));
            }
        }

        // Try SQLite
        if let Some(pool) = &self.sqlite_pool {
            return self.get_message_sqlite(pool, id).await;
        }

        Ok(None)
    }

    /// Get message from CockroachDB
    async fn get_message_cockroach(
        &self,
        pool: &PgPool,
        id: &[u8; 32],
    ) -> StorageResult<Option<MessageMetadata>> {
        let row = sqlx::query(
            r#"
            SELECT id, sender_id, recipient_id, message_type, timestamp, inline_content,
                   blob_ref, encrypted, encryption_key_id, reply_to, edited_at, deleted_at
            FROM messages WHERE id = $1
            "#,
        )
        .bind(&id[..])
        .fetch_optional(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let metadata = self.row_to_message_metadata(&row)?;
                Ok(Some(metadata))
            }
            None => Ok(None),
        }
    }

    /// Get message from SQLite
    async fn get_message_sqlite(
        &self,
        pool: &SqlitePool,
        id: &[u8; 32],
    ) -> StorageResult<Option<MessageMetadata>> {
        let row = sqlx::query(
            r#"
            SELECT id, sender_id, recipient_id, message_type, timestamp, inline_content,
                   blob_ref, encrypted, encryption_key_id, reply_to, edited_at, deleted_at
            FROM cached_messages WHERE id = ?
            "#,
        )
        .bind(&id[..])
        .fetch_optional(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        match row {
            Some(row) => {
                let metadata = self.sqlite_row_to_message_metadata(&row)?;
                Ok(Some(metadata))
            }
            None => Ok(None),
        }
    }

    /// Convert CockroachDB row to MessageMetadata
    fn row_to_message_metadata(
        &self,
        row: &sqlx::postgres::PgRow,
    ) -> StorageResult<MessageMetadata> {
        use sqlx::Row;

        let id_bytes: Vec<u8> = row.get("id");
        let sender_id_bytes: Vec<u8> = row.get("sender_id");
        let recipient_id_bytes: Vec<u8> = row.get("recipient_id");
        let message_type_str: String = row.get("message_type");
        let timestamp: DateTime<Utc> = row.get("timestamp");
        let inline_content: Option<Vec<u8>> = row.get("inline_content");
        let blob_ref_json: Option<String> = row.get("blob_ref");
        let encrypted: bool = row.get("encrypted");
        let encryption_key_id_bytes: Option<Vec<u8>> = row.get("encryption_key_id");
        let reply_to_bytes: Option<Vec<u8>> = row.get("reply_to");
        let edited_at: Option<DateTime<Utc>> = row.get("edited_at");
        let deleted_at: Option<DateTime<Utc>> = row.get("deleted_at");

        Ok(MessageMetadata {
            id: id_bytes
                .try_into()
                .map_err(|_| StorageError::Internal("Invalid id".to_string()))?,
            sender_id: sender_id_bytes
                .try_into()
                .map_err(|_| StorageError::Internal("Invalid sender_id".to_string()))?,
            recipient_id: recipient_id_bytes
                .try_into()
                .map_err(|_| StorageError::Internal("Invalid recipient_id".to_string()))?,
            message_type: match message_type_str.as_str() {
                "Direct" => MessageType::Direct,
                "Channel" => MessageType::Channel,
                "System" => MessageType::System,
                "Attachment" => MessageType::Attachment,
                _ => MessageType::Direct,
            },
            timestamp,
            inline_content,
            blob_ref: blob_ref_json.and_then(|j| serde_json::from_str(&j).ok()),
            encrypted,
            encryption_key_id: encryption_key_id_bytes.and_then(|b| b.try_into().ok()),
            reply_to: reply_to_bytes.and_then(|b| b.try_into().ok()),
            edited_at,
            deleted_at,
        })
    }

    /// Convert SQLite row to MessageMetadata
    fn sqlite_row_to_message_metadata(
        &self,
        row: &sqlx::sqlite::SqliteRow,
    ) -> StorageResult<MessageMetadata> {
        use sqlx::Row;

        let id_bytes: Vec<u8> = row.get("id");
        let sender_id_bytes: Vec<u8> = row.get("sender_id");
        let recipient_id_bytes: Vec<u8> = row.get("recipient_id");
        let message_type_str: String = row.get("message_type");
        let timestamp_str: String = row.get("timestamp");
        let inline_content: Option<Vec<u8>> = row.get("inline_content");
        let blob_ref_json: Option<String> = row.get("blob_ref");
        let encrypted: bool = row.get("encrypted");
        let encryption_key_id_bytes: Option<Vec<u8>> = row.get("encryption_key_id");
        let reply_to_bytes: Option<Vec<u8>> = row.get("reply_to");
        let edited_at_str: Option<String> = row.get("edited_at");
        let deleted_at_str: Option<String> = row.get("deleted_at");

        let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        Ok(MessageMetadata {
            id: id_bytes
                .try_into()
                .map_err(|_| StorageError::Internal("Invalid id".to_string()))?,
            sender_id: sender_id_bytes
                .try_into()
                .map_err(|_| StorageError::Internal("Invalid sender_id".to_string()))?,
            recipient_id: recipient_id_bytes
                .try_into()
                .map_err(|_| StorageError::Internal("Invalid recipient_id".to_string()))?,
            message_type: match message_type_str.as_str() {
                "Direct" => MessageType::Direct,
                "Channel" => MessageType::Channel,
                "System" => MessageType::System,
                "Attachment" => MessageType::Attachment,
                _ => MessageType::Direct,
            },
            timestamp,
            inline_content,
            blob_ref: blob_ref_json.and_then(|j| serde_json::from_str(&j).ok()),
            encrypted,
            encryption_key_id: encryption_key_id_bytes.and_then(|b| b.try_into().ok()),
            reply_to: reply_to_bytes.and_then(|b| b.try_into().ok()),
            edited_at: edited_at_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            deleted_at: deleted_at_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
        })
    }

    /// List messages for a conversation
    pub async fn list_messages(
        &self,
        user_id: &[u8; 32],
        peer_id: &[u8; 32],
        limit: u32,
        before: Option<DateTime<Utc>>,
    ) -> StorageResult<Vec<MessageMetadata>> {
        if let Some(pool) = &self.cockroach_pool {
            return self
                .list_messages_cockroach(pool, user_id, peer_id, limit, before)
                .await;
        }

        if let Some(pool) = &self.sqlite_pool {
            return self
                .list_messages_sqlite(pool, user_id, peer_id, limit, before)
                .await;
        }

        Ok(vec![])
    }

    /// List messages from CockroachDB
    async fn list_messages_cockroach(
        &self,
        pool: &PgPool,
        user_id: &[u8; 32],
        peer_id: &[u8; 32],
        limit: u32,
        before: Option<DateTime<Utc>>,
    ) -> StorageResult<Vec<MessageMetadata>> {
        let before_ts = before.unwrap_or(Utc::now());

        let rows = sqlx::query(
            r#"
            SELECT id, sender_id, recipient_id, message_type, timestamp, inline_content,
                   blob_ref, encrypted, encryption_key_id, reply_to, edited_at, deleted_at
            FROM messages
            WHERE ((sender_id = $1 AND recipient_id = $2) OR (sender_id = $2 AND recipient_id = $1))
              AND timestamp < $3
              AND deleted_at IS NULL
            ORDER BY timestamp DESC
            LIMIT $4
            "#,
        )
        .bind(&user_id[..])
        .bind(&peer_id[..])
        .bind(before_ts)
        .bind(limit as i32)
        .fetch_all(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut messages = Vec::new();
        for row in rows {
            if let Ok(metadata) = self.row_to_message_metadata(&row) {
                messages.push(metadata);
            }
        }

        Ok(messages)
    }

    /// List messages from SQLite
    async fn list_messages_sqlite(
        &self,
        pool: &SqlitePool,
        user_id: &[u8; 32],
        peer_id: &[u8; 32],
        limit: u32,
        before: Option<DateTime<Utc>>,
    ) -> StorageResult<Vec<MessageMetadata>> {
        let before_ts = before.unwrap_or(Utc::now()).to_rfc3339();

        let rows = sqlx::query(
            r#"
            SELECT id, sender_id, recipient_id, message_type, timestamp, inline_content,
                   blob_ref, encrypted, encryption_key_id, reply_to, edited_at, deleted_at
            FROM cached_messages
            WHERE ((sender_id = ? AND recipient_id = ?) OR (sender_id = ? AND recipient_id = ?))
              AND timestamp < ?
              AND deleted_at IS NULL
            ORDER BY timestamp DESC
            LIMIT ?
            "#,
        )
        .bind(&user_id[..])
        .bind(&peer_id[..])
        .bind(&peer_id[..])
        .bind(&user_id[..])
        .bind(&before_ts)
        .bind(limit as i32)
        .fetch_all(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut messages = Vec::new();
        for row in rows {
            if let Ok(metadata) = self.sqlite_row_to_message_metadata(&row) {
                messages.push(metadata);
            }
        }

        Ok(messages)
    }

    /// Get the provider registry
    pub fn registry(&self) -> &Arc<ProviderRegistry> {
        &self.registry
    }

    /// Get the challenge manager
    pub fn challenge_manager(&self) -> Option<&Arc<StorageChallengeManager>> {
        self.challenge_manager.as_ref()
    }

    /// Check if running in offline mode
    pub fn is_offline(&self) -> bool {
        self.config.offline_mode
    }

    /// Sync local cache with remote
    pub async fn sync_cache(&self) -> StorageResult<u64> {
        let (sqlite_pool, cockroach_pool) = match (&self.sqlite_pool, &self.cockroach_pool) {
            (Some(s), Some(c)) => (s, c),
            _ => return Ok(0),
        };

        // Get unsynced messages from SQLite
        let rows = sqlx::query(
            "SELECT id, sender_id, recipient_id, message_type, timestamp, inline_content, blob_ref, encrypted, encryption_key_id, reply_to, edited_at, deleted_at FROM cached_messages WHERE synced = 0 LIMIT 100"
        )
        .fetch_all(sqlite_pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut synced = 0u64;
        for row in rows {
            if let Ok(metadata) = self.sqlite_row_to_message_metadata(&row) {
                if self
                    .store_message_cockroach(cockroach_pool, &metadata)
                    .await
                    .is_ok()
                {
                    // Mark as synced
                    sqlx::query("UPDATE cached_messages SET synced = 1 WHERE id = ?")
                        .bind(&metadata.id[..])
                        .execute(sqlite_pool)
                        .await
                        .ok();
                    synced += 1;
                }
            }
        }

        if synced > 0 {
            info!("Synced {} messages to remote", synced);
        }

        Ok(synced)
    }

    /// Count total blobs stored by a user
    pub async fn count_user_blobs(&self, user_id: &[u8; 32]) -> StorageResult<u64> {
        if let Some(pool) = &self.cockroach_pool {
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count
                FROM messages m
                INNER JOIN blob_refs b ON m.blob_ref_hash = b.hash
                WHERE m.sender_id = $1
                "#,
            )
            .bind(&user_id[..])
            .fetch_one(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

            let count: i64 = row.get("count");
            return Ok(count as u64);
        }

        // Fallback to SQLite
        if let Some(pool) = &self.sqlite_pool {
            let row = sqlx::query(
                "SELECT COUNT(*) as count FROM cached_messages WHERE sender_id = ? AND blob_ref IS NOT NULL",
            )
            .bind(&user_id[..])
            .fetch_one(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

            let count: i32 = row.get("count");
            return Ok(count as u64);
        }

        Ok(0)
    }

    /// Sum total blob size for a user
    pub async fn sum_user_blob_size(&self, user_id: &[u8; 32]) -> StorageResult<u64> {
        if let Some(pool) = &self.cockroach_pool {
            let row = sqlx::query(
                r#"
                SELECT COALESCE(SUM(b.size), 0) as total_size
                FROM messages m
                INNER JOIN blob_refs b ON m.blob_ref_hash = b.hash
                WHERE m.sender_id = $1
                "#,
            )
            .bind(&user_id[..])
            .fetch_one(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

            let size: i64 = row.get("total_size");
            return Ok(size as u64);
        }

        // For SQLite, we need to parse blob_ref JSON and sum sizes
        if let Some(pool) = &self.sqlite_pool {
            let rows = sqlx::query(
                "SELECT blob_ref FROM cached_messages WHERE sender_id = ? AND blob_ref IS NOT NULL",
            )
            .bind(&user_id[..])
            .fetch_all(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

            let mut total_size: u64 = 0;
            for row in rows {
                let blob_ref_json: Option<String> = row.get("blob_ref");
                if let Some(json) = blob_ref_json {
                    if let Ok(blob_ref) = serde_json::from_str::<BlobRef>(&json) {
                        total_size += blob_ref.size;
                    }
                }
            }
            return Ok(total_size);
        }

        Ok(0)
    }

    /// Get storage usage statistics for a user
    pub async fn get_user_storage_stats(
        &self,
        user_id: &[u8; 32],
    ) -> StorageResult<UserStorageStats> {
        let blob_count = self.count_user_blobs(user_id).await?;
        let total_blob_size = self.sum_user_blob_size(user_id).await?;

        // Count messages
        let message_count = if let Some(pool) = &self.cockroach_pool {
            let row = sqlx::query("SELECT COUNT(*) as count FROM messages WHERE sender_id = $1")
                .bind(&user_id[..])
                .fetch_one(pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
            let count: i64 = row.get("count");
            count as u64
        } else if let Some(pool) = &self.sqlite_pool {
            let row =
                sqlx::query("SELECT COUNT(*) as count FROM cached_messages WHERE sender_id = ?")
                    .bind(&user_id[..])
                    .fetch_one(pool)
                    .await
                    .map_err(|e| StorageError::Database(e.to_string()))?;
            let count: i32 = row.get("count");
            count as u64
        } else {
            0
        };

        // Estimate inline content size
        let inline_size = if let Some(pool) = &self.cockroach_pool {
            let row = sqlx::query(
                "SELECT COALESCE(SUM(LENGTH(inline_content)), 0) as size FROM messages WHERE sender_id = $1 AND inline_content IS NOT NULL"
            )
            .bind(&user_id[..])
            .fetch_one(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
            let size: i64 = row.get("size");
            size as u64
        } else {
            0
        };

        Ok(UserStorageStats {
            user_id: *user_id,
            message_count,
            blob_count,
            total_blob_size,
            inline_content_size: inline_size,
            total_storage_size: total_blob_size + inline_size,
        })
    }
}

/// User storage statistics
#[derive(Debug, Clone)]
pub struct UserStorageStats {
    /// User ID
    pub user_id: [u8; 32],
    /// Total message count
    pub message_count: u64,
    /// Number of blobs stored
    pub blob_count: u64,
    /// Total size of blobs in bytes
    pub total_blob_size: u64,
    /// Total size of inline content in bytes
    pub inline_content_size: u64,
    /// Combined storage size
    pub total_storage_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_offline_router() {
        let temp_dir = tempfile::tempdir().unwrap();
        let sqlite_path = temp_dir.path().join("test.db");

        let router = StorageRouter::offline(sqlite_path).await.unwrap();
        assert!(router.is_offline());
    }

    #[test]
    fn test_inline_threshold() {
        assert_eq!(INLINE_SIZE_THRESHOLD, 64 * 1024);
        assert_eq!(CACHE_SIZE_THRESHOLD, 4 * 1024);
    }
}
