//! Provider Registry
//!
//! Persistent registry for storage providers with:
//! - Database-backed storage (not in-memory only)
//! - Chain synchronization for provider state
//! - Provider discovery and querying
//! - Capability matching

use super::capabilities::{ProviderCapabilities, ProviderCapability};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::error::{StorageError, StorageResult};

/// Registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRegistryConfig {
    /// Database connection URL (CockroachDB)
    pub database_url: String,
    /// How often to sync with chain (seconds)
    pub chain_sync_interval_secs: u64,
    /// RPC endpoint for chain synchronization
    pub chain_rpc_endpoint: String,
    /// Cache TTL for provider lookups
    pub cache_ttl_secs: u64,
    /// Minimum reputation score to be discoverable
    pub min_reputation: f64,
    /// Offline mode (skip chain sync, for testing)
    #[serde(default)]
    pub offline_mode: bool,
}

impl Default for ProviderRegistryConfig {
    fn default() -> Self {
        Self {
            database_url: "postgresql://dchat:password@localhost:26257/dchat".to_string(),
            chain_sync_interval_secs: 60,
            chain_rpc_endpoint: "http://localhost:8545".to_string(),
            cache_ttl_secs: 300,
            min_reputation: 0.5,
            offline_mode: false,
        }
    }
}

/// A registered storage provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredProvider {
    /// Provider ID (public key hash)
    pub id: [u8; 32],
    /// Provider name (for display)
    pub name: String,
    /// Provider description
    pub description: Option<String>,
    /// Provider capabilities
    pub capabilities: ProviderCapabilities,
    /// Stake amount (in smallest token unit)
    pub stake_amount: u64,
    /// Total storage capacity (bytes)
    pub total_capacity: u64,
    /// Used storage (bytes)
    pub used_storage: u64,
    /// Available storage (bytes)
    pub available_storage: u64,
    /// Reputation score (0.0 to 1.0)
    pub reputation: f64,
    /// Number of successful challenges
    pub successful_challenges: u64,
    /// Number of failed challenges
    pub failed_challenges: u64,
    /// Total earnings (lifetime)
    pub total_earnings: u64,
    /// Currently active (accepting new storage)
    pub is_active: bool,
    /// Registration timestamp
    pub registered_at: DateTime<Utc>,
    /// Last seen timestamp (heartbeat)
    pub last_seen: DateTime<Utc>,
    /// Last chain sync timestamp
    pub last_chain_sync: Option<DateTime<Utc>>,
    /// On-chain registration transaction ID
    pub registration_tx_id: Option<String>,
    /// Provider's operator address (for payments)
    pub operator_address: String,
    /// Geographic regions served (primary)
    pub primary_region: String,
    /// Additional regions served
    pub additional_regions: Vec<String>,
    /// Provider version
    pub version: String,
}

impl RegisteredProvider {
    /// Create a new provider registration
    pub fn new(
        name: &str,
        capabilities: ProviderCapabilities,
        stake_amount: u64,
        total_capacity: u64,
        operator_address: &str,
        primary_region: &str,
    ) -> Self {
        // Generate ID from operator address and timestamp
        let mut hasher = Sha256::new();
        hasher.update(operator_address.as_bytes());
        hasher.update(chrono::Utc::now().timestamp().to_le_bytes());
        let id: [u8; 32] = hasher.finalize().into();

        Self {
            id,
            name: name.to_string(),
            description: None,
            capabilities,
            stake_amount,
            total_capacity,
            used_storage: 0,
            available_storage: total_capacity,
            reputation: 1.0, // Start with perfect reputation
            successful_challenges: 0,
            failed_challenges: 0,
            total_earnings: 0,
            is_active: true,
            registered_at: Utc::now(),
            last_seen: Utc::now(),
            last_chain_sync: None,
            registration_tx_id: None,
            operator_address: operator_address.to_string(),
            primary_region: primary_region.to_string(),
            additional_regions: vec![],
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Get provider ID as hex
    pub fn id_hex(&self) -> String {
        hex::encode(self.id)
    }

    /// Check if provider has required capability
    pub fn has_capability(&self, cap: ProviderCapability) -> bool {
        match cap {
            ProviderCapability::ObjectS3 => true, // All providers have S3
            ProviderCapability::IpfsPinning => self.capabilities.supports_ipfs(),
            ProviderCapability::ArchiveObject => self.capabilities.supports_archive(),
        }
    }

    /// Check if provider serves a region
    pub fn serves_region(&self, region: &str) -> bool {
        self.primary_region == region || self.additional_regions.contains(&region.to_string())
    }

    /// Update usage
    pub fn add_usage(&mut self, bytes: u64) {
        self.used_storage += bytes;
        self.available_storage = self.total_capacity.saturating_sub(self.used_storage);
    }

    /// Release usage
    pub fn release_usage(&mut self, bytes: u64) {
        self.used_storage = self.used_storage.saturating_sub(bytes);
        self.available_storage = self.total_capacity.saturating_sub(self.used_storage);
    }

    /// Update reputation based on challenge result
    pub fn update_reputation(&mut self, success: bool) {
        if success {
            self.successful_challenges += 1;
            // Increase reputation slightly (capped at 1.0)
            self.reputation = (self.reputation * 0.99 + 0.01).min(1.0);
        } else {
            self.failed_challenges += 1;
            // Decrease reputation more significantly
            self.reputation = (self.reputation * 0.95).max(0.0);
        }
    }

    /// Calculate utilization percentage
    pub fn utilization(&self) -> f64 {
        if self.total_capacity == 0 {
            return 0.0;
        }
        self.used_storage as f64 / self.total_capacity as f64
    }

    /// Get price per GB per month for S3 storage
    pub fn s3_price_per_gb_month(&self) -> u64 {
        self.capabilities.object_s3.price_per_gb_month
    }

    /// Mark provider as seen (heartbeat)
    pub fn heartbeat(&mut self) {
        self.last_seen = Utc::now();
    }

    /// Check if provider is stale (no recent heartbeat)
    pub fn is_stale(&self, max_age: Duration) -> bool {
        Utc::now() - self.last_seen > max_age
    }
}

/// Provider Registry with database persistence
pub struct ProviderRegistry {
    /// Configuration
    config: ProviderRegistryConfig,
    /// Database pool
    pool: Option<PgPool>,
    /// In-memory cache for fast lookups
    cache: Arc<RwLock<ProviderCache>>,
    /// HTTP client for chain RPC
    http_client: reqwest::Client,
}

/// In-memory cache for providers
struct ProviderCache {
    providers: HashMap<[u8; 32], RegisteredProvider>,
    by_region: HashMap<String, Vec<[u8; 32]>>,
    last_refresh: DateTime<Utc>,
}

impl Default for ProviderCache {
    fn default() -> Self {
        Self {
            providers: HashMap::new(),
            by_region: HashMap::new(),
            last_refresh: Utc::now(),
        }
    }
}

impl ProviderRegistry {
    /// Create a new provider registry with database connection
    pub async fn new(config: ProviderRegistryConfig) -> StorageResult<Self> {
        info!("Initializing provider registry");

        let pool = if !config.offline_mode {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(10)
                .connect(&config.database_url)
                .await
                .map_err(|e| {
                    error!("Failed to connect to registry database: {}", e);
                    StorageError::Database(e.to_string())
                })?;

            // Initialize schema
            Self::init_schema(&pool).await?;
            Some(pool)
        } else {
            None
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        let registry = Self {
            config,
            pool,
            cache: Arc::new(RwLock::new(ProviderCache::default())),
            http_client,
        };

        // Load providers into cache
        registry.refresh_cache().await?;

        Ok(registry)
    }

    /// Create registry in offline mode (for testing)
    pub fn offline(config: ProviderRegistryConfig) -> Self {
        Self {
            config,
            pool: None,
            cache: Arc::new(RwLock::new(ProviderCache::default())),
            http_client: reqwest::Client::new(),
        }
    }

    /// Initialize database schema
    async fn init_schema(pool: &PgPool) -> StorageResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS storage_providers (
                id BYTEA PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                capabilities JSONB NOT NULL,
                stake_amount BIGINT NOT NULL,
                total_capacity BIGINT NOT NULL,
                used_storage BIGINT NOT NULL DEFAULT 0,
                reputation DOUBLE PRECISION NOT NULL DEFAULT 1.0,
                successful_challenges BIGINT NOT NULL DEFAULT 0,
                failed_challenges BIGINT NOT NULL DEFAULT 0,
                total_earnings BIGINT NOT NULL DEFAULT 0,
                is_active BOOLEAN NOT NULL DEFAULT true,
                registered_at TIMESTAMPTZ NOT NULL,
                last_seen TIMESTAMPTZ NOT NULL,
                last_chain_sync TIMESTAMPTZ,
                registration_tx_id TEXT,
                operator_address TEXT NOT NULL,
                primary_region TEXT NOT NULL,
                additional_regions TEXT[] NOT NULL DEFAULT '{}',
                version TEXT NOT NULL,
                UNIQUE (operator_address)
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        // Create indexes
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_providers_region ON storage_providers(primary_region)",
        )
        .execute(pool)
        .await
        .ok();

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_providers_active ON storage_providers(is_active) WHERE is_active = true",
        )
        .execute(pool)
        .await
        .ok();

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_providers_reputation ON storage_providers(reputation DESC)",
        )
        .execute(pool)
        .await
        .ok();

        info!("Provider registry schema initialized");
        Ok(())
    }

    /// Register a new provider
    pub async fn register_provider(
        &self,
        provider: RegisteredProvider,
    ) -> StorageResult<RegisteredProvider> {
        info!(
            "Registering provider: {} ({})",
            provider.name,
            provider.id_hex()
        );

        // Persist to database
        if let Some(pool) = &self.pool {
            let capabilities_json = serde_json::to_value(&provider.capabilities)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;

            sqlx::query(
                r#"
                INSERT INTO storage_providers 
                (id, name, description, capabilities, stake_amount, total_capacity, used_storage,
                 reputation, successful_challenges, failed_challenges, total_earnings, is_active,
                 registered_at, last_seen, last_chain_sync, registration_tx_id, operator_address,
                 primary_region, additional_regions, version)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
                ON CONFLICT (operator_address) DO UPDATE SET
                    name = EXCLUDED.name,
                    capabilities = EXCLUDED.capabilities,
                    stake_amount = EXCLUDED.stake_amount,
                    total_capacity = EXCLUDED.total_capacity,
                    is_active = EXCLUDED.is_active,
                    last_seen = EXCLUDED.last_seen,
                    version = EXCLUDED.version
                "#,
            )
            .bind(&provider.id[..])
            .bind(&provider.name)
            .bind(&provider.description)
            .bind(&capabilities_json)
            .bind(provider.stake_amount as i64)
            .bind(provider.total_capacity as i64)
            .bind(provider.used_storage as i64)
            .bind(provider.reputation)
            .bind(provider.successful_challenges as i64)
            .bind(provider.failed_challenges as i64)
            .bind(provider.total_earnings as i64)
            .bind(provider.is_active)
            .bind(provider.registered_at)
            .bind(provider.last_seen)
            .bind(provider.last_chain_sync)
            .bind(&provider.registration_tx_id)
            .bind(&provider.operator_address)
            .bind(&provider.primary_region)
            .bind(&provider.additional_regions)
            .bind(&provider.version)
            .execute(pool)
            .await
            .map_err(|e| {
                error!("Failed to persist provider: {}", e);
                StorageError::Database(e.to_string())
            })?;
        }

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.providers.insert(provider.id, provider.clone());
            cache
                .by_region
                .entry(provider.primary_region.clone())
                .or_default()
                .push(provider.id);
        }

        // Submit to chain (if not offline)
        if !self.config.offline_mode {
            if let Err(e) = self.submit_registration_to_chain(&provider).await {
                warn!("Chain registration failed (will retry): {}", e);
            }
        }

        Ok(provider)
    }

    /// Get provider by ID
    pub async fn get_provider(&self, id: &[u8; 32]) -> StorageResult<Option<RegisteredProvider>> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(provider) = cache.providers.get(id) {
                return Ok(Some(provider.clone()));
            }
        }

        // Fall back to database
        if let Some(pool) = &self.pool {
            self.load_provider_from_db(pool, id).await
        } else {
            Ok(None)
        }
    }

    /// Load provider from database
    async fn load_provider_from_db(
        &self,
        pool: &PgPool,
        id: &[u8; 32],
    ) -> StorageResult<Option<RegisteredProvider>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, description, capabilities, stake_amount, total_capacity, used_storage,
                   reputation, successful_challenges, failed_challenges, total_earnings, is_active,
                   registered_at, last_seen, last_chain_sync, registration_tx_id, operator_address,
                   primary_region, additional_regions, version
            FROM storage_providers
            WHERE id = $1
            "#,
        )
        .bind(&id[..])
        .fetch_optional(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        if let Some(row) = row {
            let provider = self.row_to_provider(&row)?;
            Ok(Some(provider))
        } else {
            Ok(None)
        }
    }

    /// Convert database row to RegisteredProvider
    fn row_to_provider(&self, row: &sqlx::postgres::PgRow) -> StorageResult<RegisteredProvider> {
        let id_bytes: Vec<u8> = row.get("id");
        let mut id = [0u8; 32];
        if id_bytes.len() == 32 {
            id.copy_from_slice(&id_bytes);
        }

        let capabilities_json: serde_json::Value = row.get("capabilities");
        let capabilities: ProviderCapabilities = serde_json::from_value(capabilities_json)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        Ok(RegisteredProvider {
            id,
            name: row.get("name"),
            description: row.get("description"),
            capabilities,
            stake_amount: row.get::<i64, _>("stake_amount") as u64,
            total_capacity: row.get::<i64, _>("total_capacity") as u64,
            used_storage: row.get::<i64, _>("used_storage") as u64,
            available_storage: (row.get::<i64, _>("total_capacity")
                - row.get::<i64, _>("used_storage")) as u64,
            reputation: row.get("reputation"),
            successful_challenges: row.get::<i64, _>("successful_challenges") as u64,
            failed_challenges: row.get::<i64, _>("failed_challenges") as u64,
            total_earnings: row.get::<i64, _>("total_earnings") as u64,
            is_active: row.get("is_active"),
            registered_at: row.get("registered_at"),
            last_seen: row.get("last_seen"),
            last_chain_sync: row.get("last_chain_sync"),
            registration_tx_id: row.get("registration_tx_id"),
            operator_address: row.get("operator_address"),
            primary_region: row.get("primary_region"),
            additional_regions: row.get("additional_regions"),
            version: row.get("version"),
        })
    }

    /// List all active providers
    pub async fn list_active_providers(&self) -> StorageResult<Vec<RegisteredProvider>> {
        let cache = self.cache.read().await;
        Ok(cache
            .providers
            .values()
            .filter(|p| p.is_active && p.reputation >= self.config.min_reputation)
            .cloned()
            .collect())
    }

    /// List providers by region
    pub async fn list_providers_by_region(
        &self,
        region: &str,
    ) -> StorageResult<Vec<RegisteredProvider>> {
        let cache = self.cache.read().await;
        let provider_ids = cache.by_region.get(region).cloned().unwrap_or_default();
        Ok(provider_ids
            .iter()
            .filter_map(|id| cache.providers.get(id).cloned())
            .filter(|p| p.is_active)
            .collect())
    }

    /// Find providers with specific capability
    pub async fn find_providers_with_capability(
        &self,
        capability: ProviderCapability,
    ) -> StorageResult<Vec<RegisteredProvider>> {
        let cache = self.cache.read().await;
        Ok(cache
            .providers
            .values()
            .filter(|p| p.is_active && p.has_capability(capability))
            .cloned()
            .collect())
    }

    /// Update provider reputation
    pub async fn update_reputation(
        &self,
        provider_id: &[u8; 32],
        success: bool,
    ) -> StorageResult<()> {
        // Update cache
        {
            let mut cache = self.cache.write().await;
            if let Some(provider) = cache.providers.get_mut(provider_id) {
                provider.update_reputation(success);
            }
        }

        // Update database
        if let Some(pool) = &self.pool {
            let (success_incr, fail_incr) = if success { (1, 0) } else { (0, 1) };

            sqlx::query(
                r#"
                UPDATE storage_providers
                SET successful_challenges = successful_challenges + $2,
                    failed_challenges = failed_challenges + $3,
                    reputation = CASE 
                        WHEN $4 THEN LEAST(reputation * 0.99 + 0.01, 1.0)
                        ELSE GREATEST(reputation * 0.95, 0.0)
                    END
                WHERE id = $1
                "#,
            )
            .bind(&provider_id[..])
            .bind(success_incr)
            .bind(fail_incr)
            .bind(success)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        Ok(())
    }

    /// Update provider usage
    pub async fn update_usage(&self, provider_id: &[u8; 32], delta: i64) -> StorageResult<()> {
        // Update cache
        {
            let mut cache = self.cache.write().await;
            if let Some(provider) = cache.providers.get_mut(provider_id) {
                if delta > 0 {
                    provider.add_usage(delta as u64);
                } else {
                    provider.release_usage((-delta) as u64);
                }
            }
        }

        // Update database
        if let Some(pool) = &self.pool {
            sqlx::query(
                r#"
                UPDATE storage_providers
                SET used_storage = GREATEST(0, used_storage + $2)
                WHERE id = $1
                "#,
            )
            .bind(&provider_id[..])
            .bind(delta)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        }

        Ok(())
    }

    /// Refresh cache from database
    pub async fn refresh_cache(&self) -> StorageResult<()> {
        if let Some(pool) = &self.pool {
            let rows = sqlx::query(
                r#"
                SELECT id, name, description, capabilities, stake_amount, total_capacity, used_storage,
                       reputation, successful_challenges, failed_challenges, total_earnings, is_active,
                       registered_at, last_seen, last_chain_sync, registration_tx_id, operator_address,
                       primary_region, additional_regions, version
                FROM storage_providers
                WHERE is_active = true
                "#,
            )
            .fetch_all(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

            let mut cache = self.cache.write().await;
            cache.providers.clear();
            cache.by_region.clear();

            for row in rows {
                if let Ok(provider) = self.row_to_provider(&row) {
                    cache
                        .by_region
                        .entry(provider.primary_region.clone())
                        .or_default()
                        .push(provider.id);
                    cache.providers.insert(provider.id, provider);
                }
            }

            cache.last_refresh = Utc::now();
            debug!(
                "Refreshed provider cache: {} providers",
                cache.providers.len()
            );
        }

        Ok(())
    }

    /// Submit provider registration to chain
    async fn submit_registration_to_chain(
        &self,
        provider: &RegisteredProvider,
    ) -> StorageResult<String> {
        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.register_storage_provider",
            "params": {
                "provider_id": provider.id_hex(),
                "operator_address": provider.operator_address,
                "stake_amount": provider.stake_amount,
                "capacity": provider.total_capacity,
                "region": provider.primary_region,
            }
        });

        let response = self
            .http_client
            .post(&self.config.chain_rpc_endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        body["result"]["tx_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| StorageError::Chain("Missing tx_id in response".to_string()))
    }

    /// Sync provider state from chain
    pub async fn sync_from_chain(&self) -> StorageResult<u64> {
        if self.config.offline_mode {
            return Ok(0);
        }

        use serde_json::json;

        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "currency.list_storage_providers",
            "params": {}
        });

        let response = self
            .http_client
            .post(&self.config.chain_rpc_endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| StorageError::Chain(e.to_string()))?;

        // Process chain data and update local registry
        let providers = body["result"]["providers"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);

        info!("Synced {} providers from chain", providers);
        Ok(providers as u64)
    }

    /// Get registry statistics
    pub async fn get_statistics(&self) -> RegistryStatistics {
        let cache = self.cache.read().await;
        let active_providers: Vec<_> = cache.providers.values().filter(|p| p.is_active).collect();

        RegistryStatistics {
            total_providers: cache.providers.len(),
            active_providers: active_providers.len(),
            total_capacity: active_providers.iter().map(|p| p.total_capacity).sum(),
            used_storage: active_providers.iter().map(|p| p.used_storage).sum(),
            avg_reputation: if active_providers.is_empty() {
                1.0
            } else {
                active_providers.iter().map(|p| p.reputation).sum::<f64>()
                    / active_providers.len() as f64
            },
            regions: cache.by_region.keys().cloned().collect(),
        }
    }
}

/// Registry statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryStatistics {
    pub total_providers: usize,
    pub active_providers: usize,
    pub total_capacity: u64,
    pub used_storage: u64,
    pub avg_reputation: f64,
    pub regions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::capabilities::{ObjectS3Capability, S3AuthConfig};

    fn create_test_provider(name: &str, region: &str) -> RegisteredProvider {
        let s3 = ObjectS3Capability::minio(
            "https://minio.example.com",
            "dchat",
            region,
            S3AuthConfig::static_credentials("access", "secret"),
        );
        let caps = ProviderCapabilities::s3_only(s3);

        RegisteredProvider::new(
            name,
            caps,
            10_000_0000_0000,          // 10k stake
            1024 * 1024 * 1024 * 1024, // 1 TB
            &format!("0x{}", name),
            region,
        )
    }

    #[test]
    fn test_provider_creation() {
        let provider = create_test_provider("test-provider", "us-east-1");
        assert_eq!(provider.name, "test-provider");
        assert_eq!(provider.primary_region, "us-east-1");
        assert!(provider.is_active);
        assert_eq!(provider.reputation, 1.0);
    }

    #[test]
    fn test_reputation_update() {
        let mut provider = create_test_provider("test-provider", "us-east-1");

        provider.update_reputation(true);
        assert!(provider.reputation > 0.99);
        assert_eq!(provider.successful_challenges, 1);

        provider.update_reputation(false);
        assert!(provider.reputation < 1.0);
        assert_eq!(provider.failed_challenges, 1);
    }

    #[test]
    fn test_usage_tracking() {
        let mut provider = create_test_provider("test-provider", "us-east-1");
        let initial_available = provider.available_storage;

        provider.add_usage(1024 * 1024);
        assert_eq!(provider.used_storage, 1024 * 1024);
        assert_eq!(provider.available_storage, initial_available - 1024 * 1024);

        provider.release_usage(512 * 1024);
        assert_eq!(provider.used_storage, 512 * 1024);
    }

    #[tokio::test]
    async fn test_offline_registry() {
        let mut config = ProviderRegistryConfig::default();
        config.offline_mode = true;

        let registry = ProviderRegistry::offline(config);

        let provider = create_test_provider("test", "us-east-1");
        let result = registry.register_provider(provider).await;
        assert!(result.is_ok());

        let providers = registry.list_active_providers().await.unwrap();
        assert_eq!(providers.len(), 1);
    }
}
