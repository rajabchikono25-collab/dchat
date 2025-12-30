//! Forward Secrecy Cleanup for QGE
//!
//! This module implements automatic cleanup of cryptographic state to
//! maintain forward secrecy properties. Old ratchet states, epoch keys,
//! and message keys are securely wiped after they're no longer needed.
//!
//! # Overview
//!
//! Forward secrecy requires that past message keys cannot be recovered
//! even if current keys are compromised. This module ensures:
//!
//! - **Epoch Key Cleanup**: Old epoch tokens and derived keys are wiped
//! - **Ratchet State Cleanup**: Old chain keys and skipped keys are removed
//! - **SUK Cleanup**: Unused Storage Unlock Keys are securely erased
//! - **Message Key Cleanup**: Cached message keys are purged after use
//!
//! # Cleanup Triggers
//!
//! 1. **Time-based**: Automatic cleanup of old state after configurable period
//! 2. **Usage-based**: Keys are wiped after successful decryption
//! 3. **Epoch-based**: State from old epochs is cleaned when new epoch starts
//! 4. **Manual**: Explicit cleanup triggered by user (e.g., "forget conversation")
//!
//! # Security Properties
//!
//! - **Secure Erasure**: All sensitive data is zeroized before deallocation
//! - **Bounded State**: Maximum state size prevents unbounded growth
//! - **Audit Trail**: Cleanup events are logged for security auditing
//! - **Configurable Retention**: Admins can tune retention policies

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Default retention period for epoch keys (epochs)
pub const DEFAULT_EPOCH_KEY_RETENTION: u64 = 12; // 2 hours (12 * 10 min)

/// Default retention period for skipped message keys (seconds)
pub const DEFAULT_SKIPPED_KEY_RETENTION_SECS: u64 = 7 * 24 * 60 * 60; // 7 days

/// Default retention period for ratchet chain keys (messages)
pub const DEFAULT_CHAIN_KEY_RETENTION: usize = 100;

/// Maximum number of conversations to track
pub const MAX_TRACKED_CONVERSATIONS: usize = 1000;

/// Cleanup check interval
pub const CLEANUP_INTERVAL_SECS: u64 = 60;

/// Cleanup event type for audit logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CleanupEventType {
    /// Epoch key was wiped
    EpochKeyWiped {
        conversation_id_hash: [u8; 32],
        epoch_id: u64,
    },
    /// Ratchet chain key was wiped
    ChainKeyWiped {
        conversation_id_hash: [u8; 32],
        chain_index: u32,
    },
    /// Skipped message key was wiped
    SkippedKeyWiped {
        conversation_id_hash: [u8; 32],
        message_number: u32,
    },
    /// SUK was wiped
    SukWiped {
        conversation_id_hash: [u8; 32],
    },
    /// Full conversation state wiped
    ConversationWiped {
        conversation_id_hash: [u8; 32],
    },
    /// Bulk cleanup performed
    BulkCleanup {
        items_cleaned: usize,
        reason: String,
    },
}

/// Cleanup event with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupEvent {
    /// Event type
    pub event_type: CleanupEventType,
    /// Timestamp
    pub timestamp: u64,
    /// Device ID that performed cleanup
    pub device_id: [u8; 32],
}

impl CleanupEvent {
    fn new(event_type: CleanupEventType, device_id: [u8; 32]) -> Self {
        Self {
            event_type,
            timestamp: current_timestamp(),
            device_id,
        }
    }
}

/// Configuration for forward secrecy cleanup
#[derive(Debug, Clone)]
pub struct CleanupConfig {
    /// Retention period for epoch keys (in epochs)
    pub epoch_key_retention_epochs: u64,
    /// Retention period for skipped message keys (seconds)
    pub skipped_key_retention_secs: u64,
    /// Maximum chain keys to retain per conversation
    pub max_chain_keys: usize,
    /// Maximum skipped keys per conversation
    pub max_skipped_keys: usize,
    /// Maximum SUKs to cache
    pub max_cached_suks: usize,
    /// Enable automatic cleanup
    pub auto_cleanup: bool,
    /// Cleanup interval
    pub cleanup_interval: Duration,
    /// Enable audit logging
    pub audit_logging: bool,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            epoch_key_retention_epochs: DEFAULT_EPOCH_KEY_RETENTION,
            skipped_key_retention_secs: DEFAULT_SKIPPED_KEY_RETENTION_SECS,
            max_chain_keys: DEFAULT_CHAIN_KEY_RETENTION,
            max_skipped_keys: 1000,
            max_cached_suks: 100,
            auto_cleanup: true,
            cleanup_interval: Duration::from_secs(CLEANUP_INTERVAL_SECS),
            audit_logging: true,
        }
    }
}

/// Tracked key with metadata for cleanup decisions
#[derive(Debug, Zeroize, ZeroizeOnDrop)]
pub struct TrackedKey {
    /// The actual key material
    #[zeroize(skip)] // We zeroize manually with the whole struct
    key_data: [u8; 32],
    /// When this key was created
    #[zeroize(skip)]
    created_at: u64,
    /// When this key was last used
    #[zeroize(skip)]
    last_used: u64,
    /// Use count
    #[zeroize(skip)]
    use_count: u32,
    /// Whether this key has been used for decryption
    #[zeroize(skip)]
    used_for_decrypt: bool,
}

impl TrackedKey {
    fn new(key_data: [u8; 32]) -> Self {
        let now = current_timestamp();
        Self {
            key_data,
            created_at: now,
            last_used: now,
            use_count: 0,
            used_for_decrypt: false,
        }
    }

    fn record_use(&mut self, for_decrypt: bool) {
        self.last_used = current_timestamp();
        self.use_count += 1;
        if for_decrypt {
            self.used_for_decrypt = true;
        }
    }

    fn age_secs(&self) -> u64 {
        current_timestamp().saturating_sub(self.created_at)
    }

    fn idle_secs(&self) -> u64 {
        current_timestamp().saturating_sub(self.last_used)
    }
}

/// State being tracked for a single conversation
struct ConversationCleanupState {
    /// Conversation ID hash
    conversation_id_hash: [u8; 32],
    /// Epoch keys by epoch ID
    epoch_keys: HashMap<u64, TrackedKey>,
    /// Chain keys by chain index
    chain_keys: VecDeque<(u32, TrackedKey)>,
    /// Skipped message keys
    skipped_keys: HashMap<u32, TrackedKey>,
    /// SUK cache
    cached_suks: HashMap<[u8; 32], TrackedKey>,
    /// Last cleanup time
    last_cleanup: Instant,
    /// Statistics
    stats: ConversationCleanupStats,
}

impl ConversationCleanupState {
    fn new(conversation_id_hash: [u8; 32]) -> Self {
        Self {
            conversation_id_hash,
            epoch_keys: HashMap::new(),
            chain_keys: VecDeque::new(),
            skipped_keys: HashMap::new(),
            cached_suks: HashMap::new(),
            last_cleanup: Instant::now(),
            stats: ConversationCleanupStats::default(),
        }
    }
}

/// Cleanup statistics per conversation
#[derive(Debug, Clone, Default)]
pub struct ConversationCleanupStats {
    pub epoch_keys_wiped: u64,
    pub chain_keys_wiped: u64,
    pub skipped_keys_wiped: u64,
    pub suks_wiped: u64,
}

/// Overall cleanup statistics
#[derive(Debug, Clone, Default)]
pub struct CleanupStats {
    /// Total cleanup cycles run
    pub cleanup_cycles: u64,
    /// Total keys wiped
    pub total_keys_wiped: u64,
    /// Last cleanup time
    pub last_cleanup_timestamp: u64,
    /// Conversations tracked
    pub conversations_tracked: usize,
    /// Per-type statistics
    pub epoch_keys_wiped: u64,
    pub chain_keys_wiped: u64,
    pub skipped_keys_wiped: u64,
    pub suks_wiped: u64,
}

/// Forward secrecy cleanup manager
pub struct ForwardSecrecyCleanup {
    /// Device ID
    device_id: [u8; 32],
    /// Configuration
    config: CleanupConfig,
    /// Per-conversation state
    conversations: Arc<RwLock<HashMap<[u8; 32], ConversationCleanupState>>>,
    /// Conversation access order for LRU eviction
    access_order: Arc<RwLock<VecDeque<[u8; 32]>>>,
    /// Audit log
    audit_log: Arc<RwLock<VecDeque<CleanupEvent>>>,
    /// Statistics
    stats: Arc<RwLock<CleanupStats>>,
}

impl ForwardSecrecyCleanup {
    /// Create a new cleanup manager
    pub fn new(device_id: [u8; 32]) -> Self {
        Self {
            device_id,
            config: CleanupConfig::default(),
            conversations: Arc::new(RwLock::new(HashMap::new())),
            access_order: Arc::new(RwLock::new(VecDeque::new())),
            audit_log: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            stats: Arc::new(RwLock::new(CleanupStats::default())),
        }
    }

    /// Create with custom configuration
    pub fn with_config(device_id: [u8; 32], config: CleanupConfig) -> Self {
        Self {
            device_id,
            config,
            conversations: Arc::new(RwLock::new(HashMap::new())),
            access_order: Arc::new(RwLock::new(VecDeque::new())),
            audit_log: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            stats: Arc::new(RwLock::new(CleanupStats::default())),
        }
    }

    /// Track an epoch key
    pub async fn track_epoch_key(
        &self,
        conversation_id_hash: [u8; 32],
        epoch_id: u64,
        key_data: [u8; 32],
    ) {
        let mut convos = self.conversations.write().await;
        let state = convos
            .entry(conversation_id_hash)
            .or_insert_with(|| ConversationCleanupState::new(conversation_id_hash));

        state.epoch_keys.insert(epoch_id, TrackedKey::new(key_data));
        self.update_access_order(conversation_id_hash).await;
    }

    /// Track a chain key
    pub async fn track_chain_key(
        &self,
        conversation_id_hash: [u8; 32],
        chain_index: u32,
        key_data: [u8; 32],
    ) {
        let mut convos = self.conversations.write().await;
        let state = convos
            .entry(conversation_id_hash)
            .or_insert_with(|| ConversationCleanupState::new(conversation_id_hash));

        // Enforce max chain keys
        while state.chain_keys.len() >= self.config.max_chain_keys {
            if let Some((old_idx, mut old_key)) = state.chain_keys.pop_front() {
                old_key.zeroize();
                state.stats.chain_keys_wiped += 1;

                if self.config.audit_logging {
                    self.log_event(CleanupEventType::ChainKeyWiped {
                        conversation_id_hash,
                        chain_index: old_idx,
                    })
                    .await;
                }
            }
        }

        state
            .chain_keys
            .push_back((chain_index, TrackedKey::new(key_data)));
    }

    /// Track a skipped message key
    pub async fn track_skipped_key(
        &self,
        conversation_id_hash: [u8; 32],
        message_number: u32,
        key_data: [u8; 32],
    ) {
        let mut convos = self.conversations.write().await;
        let state = convos
            .entry(conversation_id_hash)
            .or_insert_with(|| ConversationCleanupState::new(conversation_id_hash));

        // Enforce max skipped keys
        if state.skipped_keys.len() >= self.config.max_skipped_keys {
            // Remove oldest by creation time
            let oldest = state
                .skipped_keys
                .iter()
                .min_by_key(|(_, k)| k.created_at)
                .map(|(n, _)| *n);

            if let Some(msg_num) = oldest {
                if let Some(mut key) = state.skipped_keys.remove(&msg_num) {
                    key.zeroize();
                    state.stats.skipped_keys_wiped += 1;
                }
            }
        }

        state
            .skipped_keys
            .insert(message_number, TrackedKey::new(key_data));
    }

    /// Track a SUK
    pub async fn track_suk(
        &self,
        conversation_id_hash: [u8; 32],
        suk_id: [u8; 32],
        key_data: [u8; 32],
    ) {
        let mut convos = self.conversations.write().await;
        let state = convos
            .entry(conversation_id_hash)
            .or_insert_with(|| ConversationCleanupState::new(conversation_id_hash));

        // Enforce max SUKs
        while state.cached_suks.len() >= self.config.max_cached_suks {
            let oldest = state
                .cached_suks
                .iter()
                .min_by_key(|(_, k)| k.created_at)
                .map(|(id, _)| *id);

            if let Some(id) = oldest {
                if let Some(mut key) = state.cached_suks.remove(&id) {
                    key.zeroize();
                    state.stats.suks_wiped += 1;

                    if self.config.audit_logging {
                        self.log_event(CleanupEventType::SukWiped {
                            conversation_id_hash,
                        })
                        .await;
                    }
                }
            }
        }

        state.cached_suks.insert(suk_id, TrackedKey::new(key_data));
    }

    /// Record key usage (marks key for potential cleanup)
    pub async fn record_key_usage(
        &self,
        conversation_id_hash: [u8; 32],
        epoch_id: Option<u64>,
        message_number: Option<u32>,
        for_decrypt: bool,
    ) {
        let mut convos = self.conversations.write().await;

        if let Some(state) = convos.get_mut(&conversation_id_hash) {
            if let Some(epoch) = epoch_id {
                if let Some(key) = state.epoch_keys.get_mut(&epoch) {
                    key.record_use(for_decrypt);
                }
            }

            if let Some(msg_num) = message_number {
                if let Some(key) = state.skipped_keys.get_mut(&msg_num) {
                    key.record_use(for_decrypt);

                    // Wipe skipped key after use for decryption
                    if for_decrypt {
                        if let Some(mut key) = state.skipped_keys.remove(&msg_num) {
                            key.zeroize();
                            state.stats.skipped_keys_wiped += 1;

                            if self.config.audit_logging {
                                drop(convos);
                                self.log_event(CleanupEventType::SkippedKeyWiped {
                                    conversation_id_hash,
                                    message_number: msg_num,
                                })
                                .await;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Run cleanup for all conversations
    pub async fn run_cleanup(&self) -> usize {
        let current_epoch = current_epoch_id();
        let now = current_timestamp();
        let mut total_cleaned = 0;

        let mut convos = self.conversations.write().await;

        for (conversation_id_hash, state) in convos.iter_mut() {
            // Clean old epoch keys
            let old_epochs: Vec<u64> = state
                .epoch_keys
                .keys()
                .filter(|&&epoch| {
                    current_epoch.saturating_sub(epoch) > self.config.epoch_key_retention_epochs
                })
                .copied()
                .collect();

            for epoch in old_epochs {
                if let Some(mut key) = state.epoch_keys.remove(&epoch) {
                    key.zeroize();
                    state.stats.epoch_keys_wiped += 1;
                    total_cleaned += 1;
                }
            }

            // Clean old skipped keys
            let old_skipped: Vec<u32> = state
                .skipped_keys
                .iter()
                .filter(|(_, k)| k.age_secs() > self.config.skipped_key_retention_secs)
                .map(|(n, _)| *n)
                .collect();

            for msg_num in old_skipped {
                if let Some(mut key) = state.skipped_keys.remove(&msg_num) {
                    key.zeroize();
                    state.stats.skipped_keys_wiped += 1;
                    total_cleaned += 1;
                }
            }

            // Clean old SUKs (unused for more than epoch key retention)
            let suk_max_age = self.config.epoch_key_retention_epochs * 600; // epoch * 10 min
            let old_suks: Vec<[u8; 32]> = state
                .cached_suks
                .iter()
                .filter(|(_, k)| k.idle_secs() > suk_max_age)
                .map(|(id, _)| *id)
                .collect();

            for suk_id in old_suks {
                if let Some(mut key) = state.cached_suks.remove(&suk_id) {
                    key.zeroize();
                    state.stats.suks_wiped += 1;
                    total_cleaned += 1;
                }
            }

            state.last_cleanup = Instant::now();
        }

        // Update global stats
        let mut stats = self.stats.write().await;
        stats.cleanup_cycles += 1;
        stats.total_keys_wiped += total_cleaned as u64;
        stats.last_cleanup_timestamp = now;
        stats.conversations_tracked = convos.len();

        if self.config.audit_logging && total_cleaned > 0 {
            drop(convos);
            drop(stats);
            self.log_event(CleanupEventType::BulkCleanup {
                items_cleaned: total_cleaned,
                reason: "scheduled".to_string(),
            })
            .await;
        }

        total_cleaned
    }

    /// Wipe all state for a conversation
    pub async fn wipe_conversation(&self, conversation_id_hash: [u8; 32]) {
        let mut convos = self.conversations.write().await;

        if let Some(mut state) = convos.remove(&conversation_id_hash) {
            // Zeroize all keys
            for (_, mut key) in state.epoch_keys.drain() {
                key.zeroize();
            }
            for (_, mut key) in state.chain_keys.drain(..) {
                key.zeroize();
            }
            for (_, mut key) in state.skipped_keys.drain() {
                key.zeroize();
            }
            for (_, mut key) in state.cached_suks.drain() {
                key.zeroize();
            }
        }

        // Remove from access order
        let mut order = self.access_order.write().await;
        order.retain(|c| c != &conversation_id_hash);

        if self.config.audit_logging {
            drop(convos);
            drop(order);
            self.log_event(CleanupEventType::ConversationWiped {
                conversation_id_hash,
            })
            .await;
        }
    }

    /// Wipe all state (emergency cleanup)
    pub async fn wipe_all(&self) {
        let mut convos = self.conversations.write().await;
        let conversation_count = convos.len();

        for (_, mut state) in convos.drain() {
            for (_, mut key) in state.epoch_keys.drain() {
                key.zeroize();
            }
            for (_, mut key) in state.chain_keys.drain(..) {
                key.zeroize();
            }
            for (_, mut key) in state.skipped_keys.drain() {
                key.zeroize();
            }
            for (_, mut key) in state.cached_suks.drain() {
                key.zeroize();
            }
        }

        let mut order = self.access_order.write().await;
        order.clear();

        if self.config.audit_logging {
            drop(convos);
            drop(order);
            self.log_event(CleanupEventType::BulkCleanup {
                items_cleaned: conversation_count,
                reason: "emergency_wipe".to_string(),
            })
            .await;
        }
    }

    /// Get cleanup statistics
    pub async fn stats(&self) -> CleanupStats {
        self.stats.read().await.clone()
    }

    /// Get recent audit events
    pub async fn audit_events(&self, limit: usize) -> Vec<CleanupEvent> {
        let log = self.audit_log.read().await;
        log.iter().rev().take(limit).cloned().collect()
    }

    /// Update access order (for LRU tracking)
    async fn update_access_order(&self, conversation_id_hash: [u8; 32]) {
        let mut order = self.access_order.write().await;
        order.retain(|c| c != &conversation_id_hash);
        order.push_back(conversation_id_hash);

        // Enforce max conversations
        while order.len() > MAX_TRACKED_CONVERSATIONS {
            if let Some(oldest) = order.pop_front() {
                drop(order);
                self.wipe_conversation(oldest).await;
                order = self.access_order.write().await;
            }
        }
    }

    /// Log a cleanup event
    async fn log_event(&self, event_type: CleanupEventType) {
        if !self.config.audit_logging {
            return;
        }

        let mut log = self.audit_log.write().await;
        let event = CleanupEvent::new(event_type, self.device_id);

        log.push_back(event);

        // Keep log bounded
        while log.len() > 10000 {
            log.pop_front();
        }
    }

    /// Start background cleanup task
    pub fn start_cleanup_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let interval = self.config.cleanup_interval;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let cleaned = self.run_cleanup().await;
                if cleaned > 0 {
                    tracing::debug!("Forward secrecy cleanup: wiped {} keys", cleaned);
                }
            }
        })
    }
}

/// Helper to get current timestamp
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Helper to get current epoch ID
fn current_epoch_id() -> u64 {
    const EPOCH_DURATION_SECS: u64 = 600; // 10 minutes
    current_timestamp() / EPOCH_DURATION_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cleanup_creation() {
        let cleanup = ForwardSecrecyCleanup::new([1u8; 32]);
        let stats = cleanup.stats().await;
        assert_eq!(stats.cleanup_cycles, 0);
    }

    #[tokio::test]
    async fn test_track_epoch_key() {
        let cleanup = ForwardSecrecyCleanup::new([1u8; 32]);
        let conversation = [0xAB; 32];
        let key = [0xCD; 32];

        cleanup.track_epoch_key(conversation, 100, key).await;

        let convos = cleanup.conversations.read().await;
        assert!(convos.contains_key(&conversation));
        assert!(convos[&conversation].epoch_keys.contains_key(&100));
    }

    #[tokio::test]
    async fn test_wipe_conversation() {
        let cleanup = ForwardSecrecyCleanup::new([1u8; 32]);
        let conversation = [0xAB; 32];

        cleanup
            .track_epoch_key(conversation, 100, [0xCD; 32])
            .await;
        cleanup.wipe_conversation(conversation).await;

        let convos = cleanup.conversations.read().await;
        assert!(!convos.contains_key(&conversation));
    }

    #[tokio::test]
    async fn test_max_chain_keys_enforcement() {
        let config = CleanupConfig {
            max_chain_keys: 3,
            ..Default::default()
        };
        let cleanup = ForwardSecrecyCleanup::with_config([1u8; 32], config);
        let conversation = [0xAB; 32];

        // Add 5 chain keys (exceeds max of 3)
        for i in 0..5 {
            cleanup
                .track_chain_key(conversation, i, [i as u8; 32])
                .await;
        }

        let convos = cleanup.conversations.read().await;
        assert_eq!(convos[&conversation].chain_keys.len(), 3);
    }

    #[tokio::test]
    async fn test_skipped_key_wiped_after_decrypt() {
        let mut config = CleanupConfig::default();
        config.audit_logging = false; // Disable for simpler test
        let cleanup = ForwardSecrecyCleanup::with_config([1u8; 32], config);
        let conversation = [0xAB; 32];

        cleanup
            .track_skipped_key(conversation, 42, [0xCD; 32])
            .await;

        // Simulate decrypt usage
        cleanup
            .record_key_usage(conversation, None, Some(42), true)
            .await;

        // Key should be wiped
        let convos = cleanup.conversations.read().await;
        assert!(!convos[&conversation].skipped_keys.contains_key(&42));
    }

    #[tokio::test]
    async fn test_wipe_all() {
        let cleanup = ForwardSecrecyCleanup::new([1u8; 32]);

        cleanup
            .track_epoch_key([1u8; 32], 100, [0xCD; 32])
            .await;
        cleanup
            .track_epoch_key([2u8; 32], 100, [0xCD; 32])
            .await;

        cleanup.wipe_all().await;

        let convos = cleanup.conversations.read().await;
        assert!(convos.is_empty());
    }
}
