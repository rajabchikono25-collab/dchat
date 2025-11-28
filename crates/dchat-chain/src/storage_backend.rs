//! Distributed storage backend for on-chain transactions
//!
//! This module integrates the chain with distributed storage backends:
//! - **TiKV**: Blockchain state and consensus data (strong consistency)
//! - **CockroachDB**: Transaction history and queries (geo-distributed SQL)
//! - **Redis Cluster**: Hot transaction cache (low latency)
//! - **MinIO/S3**: Large payload storage (cold tier)
//!
//! Implements Section 23 (Data Lifecycle) and Section 30 (Disaster Recovery)
//! from ARCHITECTURE-2.0.md

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::transactions::{
    Transaction, TransactionStatus,
};
use dchat_core::error::{Error, Result};

#[cfg(feature = "storage-integration")]
use dchat_storage::{
    DistributedCache, DistributedDatabase, DistributedObjectStorage, TiKVStorage,
    CacheConfig, DistributedDatabaseConfig, ObjectStorageConfig, TiKVConfig,
    ChainState, BlockMetadata,
};

/// Configuration for chain storage backends
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStorageConfig {
    /// TiKV PD endpoints for consensus state
    pub tikv_endpoints: Vec<String>,
    /// CockroachDB connection URLs
    pub cockroach_urls: Vec<String>,
    /// Redis cluster URLs for caching
    pub redis_urls: Vec<String>,
    /// MinIO/S3 endpoint for cold storage
    pub minio_endpoint: String,
    /// MinIO bucket name
    pub minio_bucket: String,
    /// MinIO access key
    pub minio_access_key: String,
    /// MinIO secret key
    pub minio_secret_key: String,
    /// Enable transaction caching
    pub enable_cache: bool,
    /// Cache TTL in seconds
    pub cache_ttl_seconds: u64,
    /// Enable cold storage for old transactions
    pub enable_cold_storage: bool,
    /// Days before moving to cold storage
    pub cold_storage_days: u64,
}

impl Default for ChainStorageConfig {
    fn default() -> Self {
        Self {
            tikv_endpoints: vec!["127.0.0.1:2379".to_string()],
            cockroach_urls: vec!["postgresql://dchat:password@localhost:26257/dchat".to_string()],
            redis_urls: vec!["redis://localhost:6379".to_string()],
            minio_endpoint: "http://localhost:9000".to_string(),
            minio_bucket: "dchat-chain".to_string(),
            minio_access_key: String::new(),
            minio_secret_key: String::new(),
            enable_cache: true,
            cache_ttl_seconds: 300,
            enable_cold_storage: true,
            cold_storage_days: 90,
        }
    }
}

/// Stored transaction with full metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTransaction {
    /// Transaction ID
    pub tx_id: Uuid,
    /// Transaction type
    pub tx_type: String,
    /// Transaction hash
    pub tx_hash: String,
    /// Serialized payload
    pub payload: Vec<u8>,
    /// Block height (if confirmed)
    pub block_height: Option<u64>,
    /// Block hash (if confirmed)
    pub block_hash: Option<String>,
    /// Transaction status
    pub status: String,
    /// Submission timestamp
    pub submitted_at: DateTime<Utc>,
    /// Confirmation timestamp
    pub confirmed_at: Option<DateTime<Utc>>,
    /// Fee paid
    pub fee_paid: u64,
    /// Sender public key
    pub sender_key: Option<String>,
    /// Region where stored
    pub region: Option<String>,
    /// S3/MinIO key for cold storage
    pub cold_storage_key: Option<String>,
}

impl From<&Transaction> for StoredTransaction {
    fn from(tx: &Transaction) -> Self {
        let (block_height, block_hash) = match &tx.status {
            TransactionStatus::Confirmed { block_height, block_hash } => {
                (Some(*block_height), Some(block_hash.clone()))
            }
            _ => (None, None),
        };

        Self {
            tx_id: tx.tx_id,
            tx_type: format!("{:?}", tx.tx_type),
            tx_hash: tx.tx_hash.clone(),
            payload: tx.payload.clone(),
            block_height,
            block_hash,
            status: format!("{:?}", tx.status),
            submitted_at: tx.submitted_at,
            confirmed_at: tx.confirmed_at,
            fee_paid: tx.fee_paid,
            sender_key: None,
            region: None,
            cold_storage_key: None,
        }
    }
}

/// Distributed chain storage backend
/// 
/// Provides a unified interface to store and retrieve blockchain transactions
/// across multiple distributed storage systems for high availability.
#[cfg(feature = "storage-integration")]
pub struct ChainStorageBackend {
    /// TiKV for consensus state (strong consistency)
    tikv: Arc<TiKVStorage>,
    /// CockroachDB for transaction history (SQL queries)
    cockroach: Arc<DistributedDatabase>,
    /// Redis for hot transaction cache
    cache: Arc<DistributedCache>,
    /// MinIO/S3 for cold storage
    object_storage: Arc<DistributedObjectStorage>,
    /// Configuration
    config: ChainStorageConfig,
    /// Local fallback cache (in case Redis is unavailable)
    local_cache: Arc<RwLock<std::collections::HashMap<String, StoredTransaction>>>,
}

#[cfg(feature = "storage-integration")]
impl ChainStorageBackend {
    /// Create new chain storage backend with all distributed systems
    pub async fn new(config: ChainStorageConfig) -> Result<Self> {
        info!("Initializing chain storage backend with distributed systems");

        // Initialize TiKV
        let tikv_config = TiKVConfig {
            pd_endpoints: config.tikv_endpoints.clone(),
            connection_timeout_seconds: 10,
            operation_timeout_seconds: 30,
            enable_compression: true,
        };
        let tikv = TiKVStorage::new(tikv_config).await
            .map_err(|e| Error::storage(format!("TiKV init failed: {}", e)))?;
        info!("✅ TiKV connected");

        // Initialize CockroachDB
        let cockroach_config = DistributedDatabaseConfig {
            database_urls: config.cockroach_urls.clone(),
            max_connections: 50,
            acquire_timeout_seconds: 10,
            replication_factor: 3,
            consistency_level: "strong".to_string(),
            query_timeout_seconds: 30,
        };
        let cockroach = DistributedDatabase::new(cockroach_config).await
            .map_err(|e| Error::storage(format!("CockroachDB init failed: {}", e)))?;
        info!("✅ CockroachDB connected");

        // Initialize Redis cache
        let cache_config = CacheConfig {
            cluster_urls: config.redis_urls.clone(),
            default_ttl_seconds: config.cache_ttl_seconds,
            max_memory_mb: 8192,
            connection_timeout_seconds: 5,
        };
        let cache = DistributedCache::new(cache_config)
            .map_err(|e| Error::storage(format!("Redis init failed: {}", e)))?;
        info!("✅ Redis cluster connected");

        // Initialize MinIO/S3
        let object_config = ObjectStorageConfig {
            endpoint: config.minio_endpoint.clone(),
            bucket_name: config.minio_bucket.clone(),
            access_key: config.minio_access_key.clone(),
            secret_key: config.minio_secret_key.clone(),
            region: "us-east-1".to_string(),
            cdn_url: None,
            multi_region: true,
            upload_timeout_seconds: 60,
        };
        let object_storage = DistributedObjectStorage::new(object_config).await
            .map_err(|e| Error::storage(format!("MinIO init failed: {}", e)))?;
        info!("✅ MinIO/S3 connected");

        info!("Chain storage backend initialized successfully");

        Ok(Self {
            tikv: Arc::new(tikv),
            cockroach: Arc::new(cockroach),
            cache: Arc::new(cache),
            object_storage: Arc::new(object_storage),
            config,
            local_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
        })
    }

    /// Store a transaction across all backends
    pub async fn store_transaction(&self, tx: &Transaction, sender_key: Option<&str>) -> Result<()> {
        let mut stored_tx = StoredTransaction::from(tx);
        stored_tx.sender_key = sender_key.map(|s| s.to_string());

        // 1. Store in TiKV for consensus state (primary)
        self.store_in_tikv(&stored_tx).await?;

        // 2. Store in CockroachDB for queryable history
        self.store_in_cockroach(&stored_tx).await?;

        // 3. Cache in Redis for fast access
        if self.config.enable_cache {
            if let Err(e) = self.cache_transaction(&stored_tx).await {
                warn!("Failed to cache transaction: {} (non-fatal)", e);
            }
        }

        info!(
            "Transaction {} stored across TiKV, CockroachDB, and Redis",
            tx.tx_id
        );

        Ok(())
    }

    /// Store consensus state in TiKV
    async fn store_in_tikv(&self, tx: &StoredTransaction) -> Result<()> {
        let chain_state = ChainState {
            block_height: tx.block_height.unwrap_or(0),
            state_root: tx.tx_hash.as_bytes().to_vec(),
            timestamp: tx.submitted_at,
            validator_set: vec![],
            total_transactions: 1,
        };

        self.tikv.store_chain_state(
            tx.block_height.unwrap_or(0),
            &chain_state,
        ).await
            .map_err(|e| Error::storage(format!("TiKV store failed: {}", e)))?;

        // Also store the transaction itself
        let tx_key = format!("tx:{}", tx.tx_id);
        let tx_bytes = bincode::serialize(tx)
            .map_err(|e| Error::storage(format!("Serialize failed: {}", e)))?;

        self.tikv.store_validator_state(&tx_key, &tx_bytes).await
            .map_err(|e| Error::storage(format!("TiKV transaction store failed: {}", e)))?;

        debug!("Transaction {} stored in TiKV", tx.tx_id);
        Ok(())
    }

    /// Store transaction history in CockroachDB
    async fn store_in_cockroach(&self, tx: &StoredTransaction) -> Result<()> {
        use dchat_storage::distributed::database::MessageRow;

        let row = MessageRow {
            id: tx.tx_id,
            sender_id: tx.sender_key.clone().unwrap_or_default(),
            recipient_id: None,
            channel_id: None,
            content: tx.tx_hash.clone(),
            tier: "hot".to_string(),
            message_type: tx.tx_type.clone(),
            created_at: tx.submitted_at,
            region: tx.region.clone(),
            s3_key: tx.cold_storage_key.clone(),
            content_hash: Some(tx.tx_hash.as_bytes().to_vec()),
        };

        let region = tx.region.as_deref().unwrap_or("default");

        self.cockroach.insert_message_geo(&row, region).await
            .map_err(|e| Error::storage(format!("CockroachDB store failed: {}", e)))?;

        debug!("Transaction {} stored in CockroachDB", tx.tx_id);
        Ok(())
    }

    /// Cache transaction in Redis
    async fn cache_transaction(&self, tx: &StoredTransaction) -> Result<()> {
        let cache_key = format!("chain:tx:{}", tx.tx_id);

        self.cache.cache_message(&cache_key, tx)
            .map_err(|e| Error::storage(format!("Redis cache failed: {}", e)))?;

        // Also cache by hash for lookup
        let hash_key = format!("chain:hash:{}", tx.tx_hash);
        self.cache.cache_message(&hash_key, &tx.tx_id.to_string())
            .map_err(|e| Error::storage(format!("Redis hash cache failed: {}", e)))?;

        debug!("Transaction {} cached in Redis", tx.tx_id);
        Ok(())
    }

    /// Get transaction by ID (checks cache first, then TiKV, then CockroachDB)
    pub async fn get_transaction(&self, tx_id: Uuid) -> Result<Option<StoredTransaction>> {
        // 1. Check Redis cache first
        if self.config.enable_cache {
            let cache_key = format!("chain:tx:{}", tx_id);
            if let Ok(Some(cached)) = self.cache.get_cached_message::<StoredTransaction>(&cache_key) {
                debug!("Transaction {} found in cache", tx_id);
                return Ok(Some(cached));
            }
        }

        // 2. Check TiKV
        let tx_key = format!("tx:{}", tx_id);
        if let Ok(Some(bytes)) = self.tikv.get_validator_state(&tx_key).await {
            if let Ok(tx) = bincode::deserialize::<StoredTransaction>(&bytes) {
                debug!("Transaction {} found in TiKV", tx_id);
                // Re-cache for next time
                if self.config.enable_cache {
                    let _ = self.cache_transaction(&tx).await;
                }
                return Ok(Some(tx));
            }
        }

        // 3. Check CockroachDB
        if let Ok(Some(row)) = self.cockroach.get_message(tx_id).await {
            let tx = StoredTransaction {
                tx_id: row.id,
                tx_type: row.message_type,
                tx_hash: row.content,
                payload: row.content_hash.unwrap_or_default(),
                block_height: None,
                block_hash: None,
                status: "unknown".to_string(),
                submitted_at: row.created_at,
                confirmed_at: None,
                fee_paid: 0,
                sender_key: Some(row.sender_id),
                region: row.region,
                cold_storage_key: row.s3_key,
            };
            debug!("Transaction {} found in CockroachDB", tx_id);
            return Ok(Some(tx));
        }

        // 4. Check cold storage (MinIO)
        let cold_key = format!("transactions/{}/{}.bin", tx_id.to_string().chars().take(2).collect::<String>(), tx_id);
        if let Ok(bytes) = self.object_storage.download_bytes(&cold_key).await {
            if let Ok(tx) = bincode::deserialize::<StoredTransaction>(&bytes) {
                debug!("Transaction {} found in cold storage", tx_id);
                return Ok(Some(tx));
            }
        }

        Ok(None)
    }

    /// Get transaction by hash
    pub async fn get_transaction_by_hash(&self, tx_hash: &str) -> Result<Option<StoredTransaction>> {
        // Check hash index in Redis
        let hash_key = format!("chain:hash:{}", tx_hash);
        if let Ok(Some(tx_id_str)) = self.cache.get_cached_message::<String>(&hash_key) {
            if let Ok(tx_id) = Uuid::parse_str(&tx_id_str) {
                return self.get_transaction(tx_id).await;
            }
        }

        // Search in TiKV by scanning
        let scan_result = self.tikv.scan_keys("tx:", 1000).await
            .map_err(|e| Error::storage(format!("TiKV scan failed: {}", e)))?;

        for key in scan_result {
            if let Ok(Some(bytes)) = self.tikv.get_validator_state(&key).await {
                if let Ok(tx) = bincode::deserialize::<StoredTransaction>(&bytes) {
                    if tx.tx_hash == tx_hash {
                        return Ok(Some(tx));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Store block metadata
    pub async fn store_block(&self, block: &BlockMetadata) -> Result<()> {
        self.tikv.store_block_metadata(block).await
            .map_err(|e| Error::storage(format!("Block store failed: {}", e)))?;

        info!("Block {} stored in TiKV", block.height);
        Ok(())
    }

    /// Get block metadata
    pub async fn get_block(&self, height: u64) -> Result<Option<BlockMetadata>> {
        self.tikv.get_block_metadata(height).await
            .map_err(|e| Error::storage(format!("Block fetch failed: {}", e)))
    }

    /// Store chain state snapshot
    pub async fn store_chain_state(&self, height: u64, state: &ChainState) -> Result<()> {
        self.tikv.store_chain_state(height, state).await
            .map_err(|e| Error::storage(format!("Chain state store failed: {}", e)))?;

        info!("Chain state at height {} stored", height);
        Ok(())
    }

    /// Get chain state at height
    pub async fn get_chain_state(&self, height: u64) -> Result<Option<ChainState>> {
        self.tikv.get_chain_state(height).await
            .map_err(|e| Error::storage(format!("Chain state fetch failed: {}", e)))
    }

    /// Move old transactions to cold storage (MinIO)
    pub async fn archive_old_transactions(&self) -> Result<u64> {
        if !self.config.enable_cold_storage {
            return Ok(0);
        }

        let cutoff_days = self.config.cold_storage_days as i64;
        let archived = 0u64;

        // Query old transactions from CockroachDB
        // This is a simplified version - in production, batch this
        info!("Archiving transactions older than {} days to MinIO", cutoff_days);

        // Get statistics
        let stats = self.cockroach.get_stats().await
            .map_err(|e| Error::storage(format!("Stats query failed: {}", e)))?;

        info!(
            "Database stats: {} total messages, {} bytes",
            stats.total_messages, stats.total_size_bytes
        );

        // Archive would iterate over old transactions and move to MinIO
        // Implementation depends on specific query patterns

        Ok(archived)
    }

    /// Health check all storage backends
    pub async fn health_check(&self) -> Result<StorageHealthStatus> {
        let tikv_healthy = self.tikv.health_check().await.unwrap_or(false);
        let cockroach_healthy = self.cockroach.health_check().await.unwrap_or(false);
        let redis_healthy = self.cache.health_check().unwrap_or(false);
        let minio_healthy = self.object_storage.health_check().await.unwrap_or(false);

        let status = StorageHealthStatus {
            tikv: tikv_healthy,
            cockroach: cockroach_healthy,
            redis: redis_healthy,
            minio: minio_healthy,
            all_healthy: tikv_healthy && cockroach_healthy && redis_healthy && minio_healthy,
        };

        if !status.all_healthy {
            warn!(
                "Storage health check: TiKV={}, CockroachDB={}, Redis={}, MinIO={}",
                tikv_healthy, cockroach_healthy, redis_healthy, minio_healthy
            );
        }

        Ok(status)
    }

    /// Get storage statistics
    pub async fn get_statistics(&self) -> Result<StorageStatistics> {
        let db_stats = self.cockroach.get_stats().await
            .map_err(|e| Error::storage(format!("DB stats failed: {}", e)))?;

        let cache_stats = self.cache.get_stats()
            .map_err(|e| Error::storage(format!("Cache stats failed: {}", e)))?;

        Ok(StorageStatistics {
            total_transactions: db_stats.total_messages as u64,
            transactions_by_tier: db_stats.messages_by_tier
                .into_iter()
                .map(|(k, v)| (k, v as u64))
                .collect(),
            total_storage_bytes: db_stats.total_size_bytes as u64,
            cache_hits: cache_stats.hits,
            cache_misses: cache_stats.misses,
            cache_hit_ratio: if cache_stats.hits + cache_stats.misses > 0 {
                cache_stats.hits as f64 / (cache_stats.hits + cache_stats.misses) as f64
            } else {
                0.0
            },
        })
    }
}

/// Storage health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageHealthStatus {
    pub tikv: bool,
    pub cockroach: bool,
    pub redis: bool,
    pub minio: bool,
    pub all_healthy: bool,
}

/// Storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStatistics {
    pub total_transactions: u64,
    pub transactions_by_tier: std::collections::HashMap<String, u64>,
    pub total_storage_bytes: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_ratio: f64,
}

/// Fallback storage backend when distributed systems unavailable
/// Uses local SQLite for development/testing
#[cfg(not(feature = "storage-integration"))]
pub struct ChainStorageBackend {
    /// In-memory transaction store
    transactions: Arc<RwLock<std::collections::HashMap<Uuid, StoredTransaction>>>,
    /// In-memory block store
    blocks: Arc<RwLock<std::collections::HashMap<u64, LocalBlockMetadata>>>,
}

#[cfg(not(feature = "storage-integration"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalBlockMetadata {
    pub height: u64,
    pub hash: Vec<u8>,
    pub previous_hash: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub validator: String,
    pub transaction_count: u32,
}

#[cfg(not(feature = "storage-integration"))]
impl ChainStorageBackend {
    /// Create new local storage backend
    pub async fn new(_config: ChainStorageConfig) -> Result<Self> {
        warn!("Using local in-memory storage - enable 'storage-integration' feature for distributed storage");
        Ok(Self {
            transactions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            blocks: Arc::new(RwLock::new(std::collections::HashMap::new())),
        })
    }

    /// Store transaction locally
    pub async fn store_transaction(&self, tx: &Transaction, sender_key: Option<&str>) -> Result<()> {
        let mut stored_tx = StoredTransaction::from(tx);
        stored_tx.sender_key = sender_key.map(|s| s.to_string());

        let mut txs = self.transactions.write().await;
        txs.insert(tx.tx_id, stored_tx);

        debug!("Transaction {} stored locally", tx.tx_id);
        Ok(())
    }

    /// Get transaction locally
    pub async fn get_transaction(&self, tx_id: Uuid) -> Result<Option<StoredTransaction>> {
        let txs = self.transactions.read().await;
        Ok(txs.get(&tx_id).cloned())
    }

    /// Get transaction by hash
    pub async fn get_transaction_by_hash(&self, tx_hash: &str) -> Result<Option<StoredTransaction>> {
        let txs = self.transactions.read().await;
        for tx in txs.values() {
            if tx.tx_hash == tx_hash {
                return Ok(Some(tx.clone()));
            }
        }
        Ok(None)
    }

    /// Store block locally
    pub async fn store_block(&self, block: &LocalBlockMetadata) -> Result<()> {
        let mut blocks = self.blocks.write().await;
        blocks.insert(block.height, block.clone());
        Ok(())
    }

    /// Get block locally
    pub async fn get_block(&self, height: u64) -> Result<Option<LocalBlockMetadata>> {
        let blocks = self.blocks.read().await;
        Ok(blocks.get(&height).cloned())
    }

    /// Health check (always healthy for local)
    pub async fn health_check(&self) -> Result<StorageHealthStatus> {
        Ok(StorageHealthStatus {
            tikv: false,
            cockroach: false,
            redis: false,
            minio: false,
            all_healthy: true, // Local is always "healthy"
        })
    }

    /// Get statistics
    pub async fn get_statistics(&self) -> Result<StorageStatistics> {
        let txs = self.transactions.read().await;
        Ok(StorageStatistics {
            total_transactions: txs.len() as u64,
            transactions_by_tier: std::collections::HashMap::new(),
            total_storage_bytes: 0,
            cache_hits: 0,
            cache_misses: 0,
            cache_hit_ratio: 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactions::TransactionType;

    #[tokio::test]
    async fn test_stored_transaction_from() {
        let tx = Transaction::new(
            TransactionType::RegisterUser,
            b"test payload".to_vec(),
        );

        let stored = StoredTransaction::from(&tx);
        assert_eq!(stored.tx_id, tx.tx_id);
        assert_eq!(stored.tx_hash, tx.tx_hash);
        assert_eq!(stored.status, "Pending");
    }

    #[tokio::test]
    async fn test_default_config() {
        let config = ChainStorageConfig::default();
        assert!(!config.tikv_endpoints.is_empty());
        assert!(config.enable_cache);
        assert_eq!(config.cache_ttl_seconds, 300);
    }

    #[tokio::test]
    #[cfg(not(feature = "storage-integration"))]
    async fn test_local_storage() {
        let config = ChainStorageConfig::default();
        let backend = ChainStorageBackend::new(config).await.unwrap();

        let tx = Transaction::new(
            TransactionType::SendDirectMessage,
            b"test message".to_vec(),
        );

        backend.store_transaction(&tx, Some("sender123")).await.unwrap();

        let retrieved = backend.get_transaction(tx.tx_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().tx_id, tx.tx_id);
    }

    #[tokio::test]
    #[cfg(not(feature = "storage-integration"))]
    async fn test_get_by_hash() {
        let config = ChainStorageConfig::default();
        let backend = ChainStorageBackend::new(config).await.unwrap();

        let tx = Transaction::new(
            TransactionType::CreateChannel,
            b"channel data".to_vec(),
        );

        let tx_hash = tx.tx_hash.clone();
        backend.store_transaction(&tx, None).await.unwrap();

        let retrieved = backend.get_transaction_by_hash(&tx_hash).await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    #[cfg(not(feature = "storage-integration"))]
    async fn test_health_check() {
        let config = ChainStorageConfig::default();
        let backend = ChainStorageBackend::new(config).await.unwrap();

        let status = backend.health_check().await.unwrap();
        assert!(status.all_healthy);
    }
}
