//! Epoch Token Cache for QGE Clients
//!
//! This module provides client-side caching of epoch tokens with
//! preemptive refresh to ensure uninterrupted message encryption.
//!
//! # Overview
//!
//! Epoch tokens are required to derive unlock keys for message decryption.
//! This cache:
//!
//! - Stores tokens per conversation/epoch
//! - Preemptively refreshes tokens before expiry
//! - Handles token aggregation from multiple relays
//! - Manages token lifecycle and cleanup
//!
//! # Features
//!
//! - **Preemptive Refresh**: Tokens are refreshed 60 seconds before expiry
//! - **Multi-Conversation**: Independent caching per conversation
//! - **Background Refresh**: Async task refreshes tokens proactively
//! - **Fallback Strategy**: Multiple relay sources for resilience
//! - **Memory Bounded**: LRU eviction prevents unbounded growth
//!
//! # Usage
//!
//! ```ignore
//! let cache = EpochTokenCache::new(device_id, user_id);
//!
//! // Get token (will fetch if not cached)
//! let token = cache.get_or_fetch(conversation_id, relay_client).await?;
//!
//! // Use token to derive unlock key
//! let uk = UnlockKey::derive(&token, &[prefix1, prefix2]);
//! ```

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Maximum cached tokens per conversation (covers several epochs)
pub const MAX_TOKENS_PER_CONVERSATION: usize = 10;

/// Maximum conversations to cache tokens for
pub const MAX_CACHED_CONVERSATIONS: usize = 100;

/// Preemptive refresh time before expiry (seconds)
pub const PREEMPTIVE_REFRESH_SECS: u64 = 60;

/// Minimum time between refresh attempts (seconds)
pub const MIN_REFRESH_INTERVAL_SECS: u64 = 30;

/// Maximum age of a cached token before forced refresh (seconds)
pub const MAX_TOKEN_AGE_SECS: u64 = 3600; // 1 hour

/// Token refresh status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshStatus {
    /// Token is fresh, no refresh needed
    Fresh,
    /// Token should be refreshed soon
    RefreshSoon,
    /// Token is expired, must refresh
    Expired,
    /// No token available
    Missing,
}

/// Cached epoch token with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedToken {
    /// The epoch token itself
    pub token: EpochTokenData,
    /// When this token was cached
    pub cached_at: u64,
    /// When this token expires
    pub expires_at: u64,
    /// Number of times this token was used
    pub use_count: u32,
    /// Whether refresh is in progress
    #[serde(skip)]
    pub refresh_in_progress: bool,
}

impl CachedToken {
    /// Create a new cached token
    pub fn new(token: EpochTokenData, expires_at: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            token,
            cached_at: now,
            expires_at,
            use_count: 0,
            refresh_in_progress: false,
        }
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        now >= self.expires_at
    }

    /// Check if token needs preemptive refresh
    pub fn needs_refresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        now >= self.expires_at.saturating_sub(PREEMPTIVE_REFRESH_SECS)
    }

    /// Get refresh status
    pub fn refresh_status(&self) -> RefreshStatus {
        if self.is_expired() {
            RefreshStatus::Expired
        } else if self.needs_refresh() {
            RefreshStatus::RefreshSoon
        } else {
            RefreshStatus::Fresh
        }
    }

    /// Get remaining time until expiry
    pub fn remaining_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.expires_at.saturating_sub(now)
    }

    /// Record a use of this token
    pub fn record_use(&mut self) {
        self.use_count += 1;
    }
}

/// Minimal epoch token data for caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochTokenData {
    /// Aggregated FROST signature
    pub signature: [u8; 64],
    /// Epoch ID this token is for
    pub epoch_id: u64,
    /// Conversation ID hash
    pub conversation_id_hash: [u8; 32],
    /// Quorum public key used for aggregation
    pub quorum_public_key: [u8; 32],
    /// Number of relays that contributed
    pub contributing_relays: u8,
    /// Threshold used for aggregation
    pub threshold: u8,
}

/// Cache for a single conversation's tokens
#[derive(Debug)]
struct ConversationCache {
    /// Conversation ID hash
    conversation_id_hash: [u8; 32],
    /// Cached tokens by epoch ID
    tokens: HashMap<u64, CachedToken>,
    /// Order of token insertion for LRU eviction
    insertion_order: VecDeque<u64>,
    /// Last successful refresh
    last_refresh: Option<Instant>,
    /// Refresh failure count (for backoff)
    refresh_failures: u32,
}

impl ConversationCache {
    fn new(conversation_id_hash: [u8; 32]) -> Self {
        Self {
            conversation_id_hash,
            tokens: HashMap::new(),
            insertion_order: VecDeque::new(),
            last_refresh: None,
            refresh_failures: 0,
        }
    }

    fn insert(&mut self, epoch_id: u64, token: CachedToken) {
        // Remove oldest if at capacity
        while self.tokens.len() >= MAX_TOKENS_PER_CONVERSATION {
            if let Some(oldest) = self.insertion_order.pop_front() {
                self.tokens.remove(&oldest);
            }
        }

        self.tokens.insert(epoch_id, token);
        self.insertion_order.push_back(epoch_id);
    }

    fn get(&self, epoch_id: u64) -> Option<&CachedToken> {
        self.tokens.get(&epoch_id)
    }

    fn get_mut(&mut self, epoch_id: u64) -> Option<&mut CachedToken> {
        self.tokens.get_mut(&epoch_id)
    }

    fn remove_expired(&mut self) {
        let expired: Vec<u64> = self
            .tokens
            .iter()
            .filter(|(_, t)| t.is_expired())
            .map(|(e, _)| *e)
            .collect();

        for epoch_id in expired {
            self.tokens.remove(&epoch_id);
            self.insertion_order.retain(|&e| e != epoch_id);
        }
    }

    fn record_refresh_success(&mut self) {
        self.last_refresh = Some(Instant::now());
        self.refresh_failures = 0;
    }

    fn record_refresh_failure(&mut self) {
        self.refresh_failures += 1;
    }

    fn backoff_duration(&self) -> Duration {
        // Exponential backoff: 30s, 60s, 120s, 240s, max 300s
        let base = MIN_REFRESH_INTERVAL_SECS;
        let multiplier = 2u64.pow(self.refresh_failures.min(4));
        Duration::from_secs((base * multiplier).min(300))
    }

    fn can_refresh(&self) -> bool {
        match self.last_refresh {
            Some(last) => last.elapsed() >= self.backoff_duration(),
            None => true,
        }
    }
}

/// Epoch token cache for QGE clients
pub struct EpochTokenCache {
    /// Device ID
    device_id: [u8; 32],
    /// User ID
    user_id: [u8; 32],
    /// Per-conversation caches
    caches: Arc<RwLock<HashMap<[u8; 32], ConversationCache>>>,
    /// Order of conversation access for LRU eviction
    conversation_order: Arc<RwLock<VecDeque<[u8; 32]>>>,
    /// Configuration
    config: TokenCacheConfig,
    /// Statistics
    stats: Arc<RwLock<CacheStats>>,
}

/// Configuration for token cache
#[derive(Debug, Clone)]
pub struct TokenCacheConfig {
    /// Maximum tokens per conversation
    pub max_tokens_per_conversation: usize,
    /// Maximum conversations to cache
    pub max_conversations: usize,
    /// Preemptive refresh time (seconds)
    pub preemptive_refresh_secs: u64,
    /// Enable background refresh
    pub background_refresh: bool,
    /// Refresh check interval (seconds)
    pub refresh_check_interval_secs: u64,
}

impl Default for TokenCacheConfig {
    fn default() -> Self {
        Self {
            max_tokens_per_conversation: MAX_TOKENS_PER_CONVERSATION,
            max_conversations: MAX_CACHED_CONVERSATIONS,
            preemptive_refresh_secs: PREEMPTIVE_REFRESH_SECS,
            background_refresh: true,
            refresh_check_interval_secs: 10,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total cache hits
    pub hits: u64,
    /// Total cache misses
    pub misses: u64,
    /// Total refreshes attempted
    pub refresh_attempts: u64,
    /// Successful refreshes
    pub refresh_successes: u64,
    /// Failed refreshes
    pub refresh_failures: u64,
    /// Preemptive refreshes (before expiry)
    pub preemptive_refreshes: u64,
    /// Tokens evicted due to expiry
    pub expired_evictions: u64,
    /// Tokens evicted due to capacity
    pub capacity_evictions: u64,
}

impl CacheStats {
    /// Calculate hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Calculate refresh success rate
    pub fn refresh_success_rate(&self) -> f64 {
        if self.refresh_attempts == 0 {
            0.0
        } else {
            self.refresh_successes as f64 / self.refresh_attempts as f64
        }
    }
}

impl EpochTokenCache {
    /// Create a new epoch token cache
    pub fn new(device_id: [u8; 32], user_id: [u8; 32]) -> Self {
        Self {
            device_id,
            user_id,
            caches: Arc::new(RwLock::new(HashMap::new())),
            conversation_order: Arc::new(RwLock::new(VecDeque::new())),
            config: TokenCacheConfig::default(),
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Create with custom configuration
    pub fn with_config(
        device_id: [u8; 32],
        user_id: [u8; 32],
        config: TokenCacheConfig,
    ) -> Self {
        Self {
            device_id,
            user_id,
            caches: Arc::new(RwLock::new(HashMap::new())),
            conversation_order: Arc::new(RwLock::new(VecDeque::new())),
            config,
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Get a cached token for a conversation/epoch
    pub async fn get(
        &self,
        conversation_id_hash: &[u8; 32],
        epoch_id: u64,
    ) -> Option<EpochTokenData> {
        let caches = self.caches.read().await;

        if let Some(cache) = caches.get(conversation_id_hash) {
            if let Some(cached) = cache.get(epoch_id) {
                if !cached.is_expired() {
                    let mut stats = self.stats.write().await;
                    stats.hits += 1;
                    return Some(cached.token.clone());
                }
            }
        }

        let mut stats = self.stats.write().await;
        stats.misses += 1;
        None
    }

    /// Get token or fetch from relays if not cached
    pub async fn get_or_fetch<F, Fut>(
        &self,
        conversation_id_hash: [u8; 32],
        epoch_id: u64,
        fetch_fn: F,
    ) -> Result<EpochTokenData>
    where
        F: FnOnce([u8; 32], u64, [u8; 32], [u8; 32]) -> Fut,
        Fut: std::future::Future<Output = Result<(EpochTokenData, u64)>>,
    {
        // Check cache first
        if let Some(token) = self.get(&conversation_id_hash, epoch_id).await {
            return Ok(token);
        }

        // Fetch from relays
        let (token, expires_at) = fetch_fn(
            conversation_id_hash,
            epoch_id,
            self.device_id,
            self.user_id,
        )
        .await?;

        // Cache the result
        self.store(conversation_id_hash, epoch_id, token.clone(), expires_at)
            .await;

        Ok(token)
    }

    /// Store a token in the cache
    pub async fn store(
        &self,
        conversation_id_hash: [u8; 32],
        epoch_id: u64,
        token: EpochTokenData,
        expires_at: u64,
    ) {
        let mut caches = self.caches.write().await;

        // Ensure we don't exceed max conversations
        if !caches.contains_key(&conversation_id_hash) {
            let mut order = self.conversation_order.write().await;
            while caches.len() >= self.config.max_conversations {
                if let Some(oldest) = order.pop_front() {
                    caches.remove(&oldest);
                    let mut stats = self.stats.write().await;
                    stats.capacity_evictions += 1;
                }
            }
            order.push_back(conversation_id_hash);
        }

        let cache = caches
            .entry(conversation_id_hash)
            .or_insert_with(|| ConversationCache::new(conversation_id_hash));

        let cached = CachedToken::new(token, expires_at);
        cache.insert(epoch_id, cached);
    }

    /// Check refresh status for a conversation/epoch
    pub async fn refresh_status(
        &self,
        conversation_id_hash: &[u8; 32],
        epoch_id: u64,
    ) -> RefreshStatus {
        let caches = self.caches.read().await;

        if let Some(cache) = caches.get(conversation_id_hash) {
            if let Some(token) = cache.get(epoch_id) {
                return token.refresh_status();
            }
        }

        RefreshStatus::Missing
    }

    /// Get tokens needing refresh across all conversations
    pub async fn get_tokens_needing_refresh(&self) -> Vec<([u8; 32], u64)> {
        let caches = self.caches.read().await;
        let mut needing_refresh = Vec::new();

        for (conversation_id_hash, cache) in caches.iter() {
            if !cache.can_refresh() {
                continue;
            }

            for (epoch_id, token) in cache.tokens.iter() {
                if token.needs_refresh() && !token.refresh_in_progress {
                    needing_refresh.push((*conversation_id_hash, *epoch_id));
                }
            }
        }

        needing_refresh
    }

    /// Mark a token as being refreshed
    pub async fn mark_refresh_in_progress(
        &self,
        conversation_id_hash: &[u8; 32],
        epoch_id: u64,
    ) {
        let mut caches = self.caches.write().await;
        if let Some(cache) = caches.get_mut(conversation_id_hash) {
            if let Some(token) = cache.get_mut(epoch_id) {
                token.refresh_in_progress = true;
            }
        }
    }

    /// Record refresh result
    pub async fn record_refresh_result(
        &self,
        conversation_id_hash: &[u8; 32],
        success: bool,
    ) {
        let mut caches = self.caches.write().await;
        let mut stats = self.stats.write().await;

        stats.refresh_attempts += 1;

        if let Some(cache) = caches.get_mut(conversation_id_hash) {
            if success {
                cache.record_refresh_success();
                stats.refresh_successes += 1;
            } else {
                cache.record_refresh_failure();
                stats.refresh_failures += 1;
            }
        }
    }

    /// Clean up expired tokens
    pub async fn cleanup_expired(&self) {
        let mut caches = self.caches.write().await;
        let mut stats = self.stats.write().await;

        for cache in caches.values_mut() {
            let before = cache.tokens.len();
            cache.remove_expired();
            let removed = before - cache.tokens.len();
            stats.expired_evictions += removed as u64;
        }
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    /// Get current cache size
    pub async fn size(&self) -> (usize, usize) {
        let caches = self.caches.read().await;
        let conversations = caches.len();
        let tokens: usize = caches.values().map(|c| c.tokens.len()).sum();
        (conversations, tokens)
    }

    /// Clear all cached tokens for a conversation
    pub async fn clear_conversation(&self, conversation_id_hash: &[u8; 32]) {
        let mut caches = self.caches.write().await;
        caches.remove(conversation_id_hash);

        let mut order = self.conversation_order.write().await;
        order.retain(|c| c != conversation_id_hash);
    }

    /// Clear all cached tokens
    pub async fn clear_all(&self) {
        let mut caches = self.caches.write().await;
        caches.clear();

        let mut order = self.conversation_order.write().await;
        order.clear();
    }

    /// Record token use (for analytics)
    pub async fn record_use(&self, conversation_id_hash: &[u8; 32], epoch_id: u64) {
        let mut caches = self.caches.write().await;
        if let Some(cache) = caches.get_mut(conversation_id_hash) {
            if let Some(token) = cache.get_mut(epoch_id) {
                token.record_use();
            }
        }
    }

    /// Start background refresh task
    pub fn start_background_refresh<F, Fut>(
        self: Arc<Self>,
        fetch_fn: F,
    ) -> tokio::task::JoinHandle<()>
    where
        F: Fn([u8; 32], u64, [u8; 32], [u8; 32]) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(EpochTokenData, u64)>> + Send,
    {
        let interval = Duration::from_secs(self.config.refresh_check_interval_secs);

        tokio::spawn(async move {
            loop {
                // Cleanup expired tokens
                self.cleanup_expired().await;

                // Get tokens needing refresh
                let needing_refresh = self.get_tokens_needing_refresh().await;

                for (conversation_id_hash, epoch_id) in needing_refresh {
                    self.mark_refresh_in_progress(&conversation_id_hash, epoch_id)
                        .await;

                    // Get current epoch (we should fetch current/next, not old)
                    let current_epoch = current_epoch_id();

                    // Only refresh if it's current or next epoch
                    if epoch_id < current_epoch.saturating_sub(1) {
                        continue;
                    }

                    match fetch_fn(
                        conversation_id_hash,
                        current_epoch,
                        self.device_id,
                        self.user_id,
                    )
                    .await
                    {
                        Ok((token, expires_at)) => {
                            self.store(conversation_id_hash, current_epoch, token, expires_at)
                                .await;
                            self.record_refresh_result(&conversation_id_hash, true)
                                .await;

                            let mut stats = self.stats.write().await;
                            stats.preemptive_refreshes += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to refresh token for {:?}: {}",
                                hex::encode(conversation_id_hash),
                                e
                            );
                            self.record_refresh_result(&conversation_id_hash, false)
                                .await;
                        }
                    }
                }

                tokio::time::sleep(interval).await;
            }
        })
    }
}

/// Helper to get current epoch ID
fn current_epoch_id() -> u64 {
    const EPOCH_DURATION_SECS: u64 = 600; // 10 minutes
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / EPOCH_DURATION_SECS)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_token(epoch_id: u64) -> EpochTokenData {
        EpochTokenData {
            signature: [0xAB; 64],
            epoch_id,
            conversation_id_hash: [0xCD; 32],
            quorum_public_key: [0xEF; 32],
            contributing_relays: 5,
            threshold: 4,
        }
    }

    #[tokio::test]
    async fn test_cache_creation() {
        let cache = EpochTokenCache::new([1; 32], [2; 32]);
        let (conversations, tokens) = cache.size().await;
        assert_eq!(conversations, 0);
        assert_eq!(tokens, 0);
    }

    #[tokio::test]
    async fn test_store_and_get() {
        let cache = EpochTokenCache::new([1; 32], [2; 32]);
        let conversation = [0xAB; 32];
        let epoch_id = 100;
        let token = create_test_token(epoch_id);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        cache
            .store(conversation, epoch_id, token.clone(), now + 600)
            .await;

        let retrieved = cache.get(&conversation, epoch_id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().epoch_id, epoch_id);
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = EpochTokenCache::new([1; 32], [2; 32]);
        let result = cache.get(&[0xAB; 32], 100).await;
        assert!(result.is_none());

        let stats = cache.stats().await;
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_expired_token_not_returned() {
        let cache = EpochTokenCache::new([1; 32], [2; 32]);
        let conversation = [0xAB; 32];
        let epoch_id = 100;
        let token = create_test_token(epoch_id);

        // Store with already-expired timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        cache
            .store(conversation, epoch_id, token, now - 1) // Already expired
            .await;

        let retrieved = cache.get(&conversation, epoch_id).await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_refresh_status() {
        let cache = EpochTokenCache::new([1; 32], [2; 32]);
        let conversation = [0xAB; 32];

        // Missing token
        let status = cache.refresh_status(&conversation, 100).await;
        assert_eq!(status, RefreshStatus::Missing);

        // Store fresh token
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        cache
            .store(
                conversation,
                100,
                create_test_token(100),
                now + 600, // Far in future
            )
            .await;

        let status = cache.refresh_status(&conversation, 100).await;
        assert_eq!(status, RefreshStatus::Fresh);
    }

    #[tokio::test]
    async fn test_clear_conversation() {
        let cache = EpochTokenCache::new([1; 32], [2; 32]);
        let conversation = [0xAB; 32];

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        cache
            .store(conversation, 100, create_test_token(100), now + 600)
            .await;
        cache
            .store(conversation, 101, create_test_token(101), now + 600)
            .await;

        let (conversations, tokens) = cache.size().await;
        assert_eq!(conversations, 1);
        assert_eq!(tokens, 2);

        cache.clear_conversation(&conversation).await;

        let (conversations, tokens) = cache.size().await;
        assert_eq!(conversations, 0);
        assert_eq!(tokens, 0);
    }

    #[tokio::test]
    async fn test_lru_eviction() {
        let config = TokenCacheConfig {
            max_tokens_per_conversation: 3,
            ..Default::default()
        };
        let cache = EpochTokenCache::with_config([1; 32], [2; 32], config);
        let conversation = [0xAB; 32];

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Store 4 tokens (exceeds limit of 3)
        for i in 100..104 {
            cache
                .store(conversation, i, create_test_token(i), now + 600)
                .await;
        }

        let (_, tokens) = cache.size().await;
        assert_eq!(tokens, 3); // Limited to 3

        // Oldest (100) should be evicted
        let oldest = cache.get(&conversation, 100).await;
        assert!(oldest.is_none());

        // Newest should still be there
        let newest = cache.get(&conversation, 103).await;
        assert!(newest.is_some());
    }

    #[tokio::test]
    async fn test_stats_hit_rate() {
        let cache = EpochTokenCache::new([1; 32], [2; 32]);
        let conversation = [0xAB; 32];

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        cache
            .store(conversation, 100, create_test_token(100), now + 600)
            .await;

        // 2 hits
        cache.get(&conversation, 100).await;
        cache.get(&conversation, 100).await;

        // 1 miss
        cache.get(&conversation, 999).await;

        let stats = cache.stats().await;
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate() - 0.666).abs() < 0.01);
    }
}
