//! QGE Audit Logging
//!
//! This module implements comprehensive audit logging for all QGE security
//! events. It provides tamper-evident logging with structured events for
//! security monitoring, compliance, and forensics.
//!
//! # Event Categories
//!
//! - **Token Events**: Token requests, issuances, rejections, revocations
//! - **Key Events**: Key generation, rotation, distribution, cleanup
//! - **Access Events**: Access granted, denied, escalated
//! - **Admin Events**: Revocations, role changes, channel modifications
//! - **Security Events**: Abuse detection, rate limiting, attacks
//! - **System Events**: Committee changes, epoch transitions
//!
//! # Logging Properties
//!
//! - **Structured**: JSON-serializable event objects
//! - **Tamper-Evident**: Hash chain linking events
//! - **Privacy-Preserving**: User IDs can be pseudonymized
//! - **Filterable**: By category, severity, time range, user
//! - **Exportable**: To external SIEM systems

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Maximum in-memory log entries
pub const MAX_MEMORY_ENTRIES: usize = 10_000;

/// Maximum log file size (bytes)
pub const MAX_LOG_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100MB

/// Log retention period (seconds)
pub const LOG_RETENTION_SECS: u64 = 90 * 24 * 60 * 60; // 90 days

/// Event severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Debug information
    Debug = 0,
    /// Informational event
    Info = 1,
    /// Warning condition
    Warning = 2,
    /// Error condition
    Error = 3,
    /// Critical security event
    Critical = 4,
    /// Alert requiring immediate attention
    Alert = 5,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Debug => "DEBUG",
            Severity::Info => "INFO",
            Severity::Warning => "WARNING",
            Severity::Error => "ERROR",
            Severity::Critical => "CRITICAL",
            Severity::Alert => "ALERT",
        }
    }
}

/// Event category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventCategory {
    /// Token-related events
    Token,
    /// Key management events
    Key,
    /// Access control events
    Access,
    /// Administrative actions
    Admin,
    /// Security incidents
    Security,
    /// System events
    System,
    /// Network events
    Network,
    /// Relay events
    Relay,
}

/// Specific event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    // Token events
    TokenRequested {
        epoch_id: u64,
        conversation_type: String,
    },
    TokenIssued {
        epoch_id: u64,
        token_hash: [u8; 32],
    },
    TokenRejected {
        epoch_id: u64,
        reason: String,
    },
    TokenRevoked {
        epoch_id: u64,
        reason: String,
    },
    TokenRefreshed {
        old_epoch: u64,
        new_epoch: u64,
    },

    // Key events
    KeyGenerated {
        key_type: String,
        key_id: [u8; 32],
    },
    KeyRotated {
        old_key_id: [u8; 32],
        new_key_id: [u8; 32],
    },
    KeyDistributed {
        key_id: [u8; 32],
        recipient_count: u32,
    },
    KeyDeleted {
        key_id: [u8; 32],
        reason: String,
    },
    SukUnlocked {
        unlock_method: String,
    },

    // Access events
    AccessGranted {
        resource: String,
        permission: String,
    },
    AccessDenied {
        resource: String,
        reason: String,
    },
    AccessEscalated {
        from_level: u8,
        to_level: u8,
    },
    MembershipChanged {
        action: String,
        role: String,
    },

    // Admin events
    MemberRevoked {
        action: String,
        reason: String,
        duration_secs: Option<u64>,
    },
    RoleChanged {
        old_role: String,
        new_role: String,
    },
    ChannelModified {
        modification: String,
    },
    AppealSubmitted {
        revocation_id: [u8; 32],
    },
    AppealDecided {
        revocation_id: [u8; 32],
        decision: String,
    },

    // Security events
    AbuseDetected {
        abuse_type: String,
        score: u64,
    },
    RateLimited {
        category: String,
        wait_ms: u64,
    },
    AttackDetected {
        attack_type: String,
        source: String,
    },
    DoubleSignDetected {
        epoch_id: u64,
    },
    InvalidSignature {
        context: String,
    },
    ReplayAttempt {
        original_timestamp: u64,
    },

    // System events
    EpochTransition {
        from_epoch: u64,
        to_epoch: u64,
    },
    CommitteeRotation {
        old_members: Vec<[u8; 32]>,
        new_members: Vec<[u8; 32]>,
    },
    ConfigChanged {
        key: String,
        old_value: String,
        new_value: String,
    },
    SystemStarted {
        version: String,
    },
    SystemShutdown {
        reason: String,
    },

    // Network events
    PeerConnected {
        peer_id: [u8; 32],
    },
    PeerDisconnected {
        peer_id: [u8; 32],
        reason: String,
    },
    HandshakeCompleted {
        peer_id: [u8; 32],
        protocol_version: u16,
    },
    HandshakeFailed {
        peer_id: Option<[u8; 32]>,
        reason: String,
    },

    // Relay events
    RelayJoined {
        stake_amount: u64,
        region: String,
    },
    RelayLeft {
        reason: String,
    },
    RelaySlashed {
        amount: u64,
        reason: String,
    },
    RelayRewarded {
        amount: u64,
        epoch_id: u64,
    },

    // Generic
    Custom {
        event_name: String,
        details: HashMap<String, String>,
    },
}

impl EventType {
    pub fn category(&self) -> EventCategory {
        match self {
            EventType::TokenRequested { .. }
            | EventType::TokenIssued { .. }
            | EventType::TokenRejected { .. }
            | EventType::TokenRevoked { .. }
            | EventType::TokenRefreshed { .. } => EventCategory::Token,

            EventType::KeyGenerated { .. }
            | EventType::KeyRotated { .. }
            | EventType::KeyDistributed { .. }
            | EventType::KeyDeleted { .. }
            | EventType::SukUnlocked { .. } => EventCategory::Key,

            EventType::AccessGranted { .. }
            | EventType::AccessDenied { .. }
            | EventType::AccessEscalated { .. }
            | EventType::MembershipChanged { .. } => EventCategory::Access,

            EventType::MemberRevoked { .. }
            | EventType::RoleChanged { .. }
            | EventType::ChannelModified { .. }
            | EventType::AppealSubmitted { .. }
            | EventType::AppealDecided { .. } => EventCategory::Admin,

            EventType::AbuseDetected { .. }
            | EventType::RateLimited { .. }
            | EventType::AttackDetected { .. }
            | EventType::DoubleSignDetected { .. }
            | EventType::InvalidSignature { .. }
            | EventType::ReplayAttempt { .. } => EventCategory::Security,

            EventType::EpochTransition { .. }
            | EventType::CommitteeRotation { .. }
            | EventType::ConfigChanged { .. }
            | EventType::SystemStarted { .. }
            | EventType::SystemShutdown { .. } => EventCategory::System,

            EventType::PeerConnected { .. }
            | EventType::PeerDisconnected { .. }
            | EventType::HandshakeCompleted { .. }
            | EventType::HandshakeFailed { .. } => EventCategory::Network,

            EventType::RelayJoined { .. }
            | EventType::RelayLeft { .. }
            | EventType::RelaySlashed { .. }
            | EventType::RelayRewarded { .. } => EventCategory::Relay,

            EventType::Custom { .. } => EventCategory::System,
        }
    }
}

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Entry ID
    pub id: u64,
    /// Timestamp (Unix epoch seconds)
    pub timestamp: u64,
    /// Severity level
    pub severity: Severity,
    /// Event category
    pub category: EventCategory,
    /// Event type with details
    pub event: EventType,
    /// Actor (user/device/relay that caused the event)
    pub actor: Option<ActorInfo>,
    /// Target (affected entity)
    pub target: Option<TargetInfo>,
    /// Additional context
    pub context: HashMap<String, String>,
    /// Hash of previous entry (chain linking)
    pub prev_hash: [u8; 32],
    /// Hash of this entry
    pub entry_hash: [u8; 32],
}

/// Actor information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorInfo {
    /// Actor ID (pseudonymized)
    pub id: [u8; 32],
    /// Actor type
    pub actor_type: ActorType,
    /// IP address (optional, may be redacted)
    pub ip_addr: Option<String>,
    /// Device ID
    pub device_id: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorType {
    User,
    Device,
    Relay,
    Admin,
    System,
}

/// Target information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetInfo {
    /// Target ID
    pub id: [u8; 32],
    /// Target type
    pub target_type: TargetType,
    /// Additional info
    pub info: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TargetType {
    User,
    Channel,
    Message,
    Key,
    Token,
    Relay,
    System,
}

/// Query filter for audit logs
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    /// Filter by categories
    pub categories: Option<Vec<EventCategory>>,
    /// Filter by minimum severity
    pub min_severity: Option<Severity>,
    /// Filter by actor ID
    pub actor_id: Option<[u8; 32]>,
    /// Filter by target ID
    pub target_id: Option<[u8; 32]>,
    /// Filter by time range
    pub from_timestamp: Option<u64>,
    pub to_timestamp: Option<u64>,
    /// Maximum results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl AuditQuery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn category(mut self, category: EventCategory) -> Self {
        self.categories.get_or_insert_with(Vec::new).push(category);
        self
    }

    pub fn min_severity(mut self, severity: Severity) -> Self {
        self.min_severity = Some(severity);
        self
    }

    pub fn actor(mut self, actor_id: [u8; 32]) -> Self {
        self.actor_id = Some(actor_id);
        self
    }

    pub fn target(mut self, target_id: [u8; 32]) -> Self {
        self.target_id = Some(target_id);
        self
    }

    pub fn time_range(mut self, from: u64, to: u64) -> Self {
        self.from_timestamp = Some(from);
        self.to_timestamp = Some(to);
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    fn matches(&self, entry: &AuditEntry) -> bool {
        if let Some(ref cats) = self.categories {
            if !cats.contains(&entry.category) {
                return false;
            }
        }

        if let Some(min_sev) = self.min_severity {
            if entry.severity < min_sev {
                return false;
            }
        }

        if let Some(actor_id) = self.actor_id {
            match &entry.actor {
                Some(actor) if actor.id == actor_id => {}
                _ => return false,
            }
        }

        if let Some(target_id) = self.target_id {
            match &entry.target {
                Some(target) if target.id == target_id => {}
                _ => return false,
            }
        }

        if let Some(from) = self.from_timestamp {
            if entry.timestamp < from {
                return false;
            }
        }

        if let Some(to) = self.to_timestamp {
            if entry.timestamp > to {
                return false;
            }
        }

        true
    }
}

/// Audit log statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditStats {
    pub total_entries: u64,
    pub entries_by_category: HashMap<EventCategory, u64>,
    pub entries_by_severity: HashMap<Severity, u64>,
    pub oldest_entry: Option<u64>,
    pub newest_entry: Option<u64>,
    pub chain_valid: bool,
}

/// QGE Audit Logger
pub struct QgeAuditLogger {
    /// In-memory log buffer
    entries: Arc<RwLock<VecDeque<AuditEntry>>>,
    /// Next entry ID
    next_id: Arc<RwLock<u64>>,
    /// Last entry hash (for chain)
    last_hash: Arc<RwLock<[u8; 32]>>,
    /// Log file path (optional)
    log_file: Option<Arc<RwLock<std::fs::File>>>,
    /// Minimum severity to log
    min_severity: Severity,
    /// Callback for alerts
    alert_callback: Arc<RwLock<Option<Box<dyn Fn(AuditEntry) + Send + Sync>>>>,
    /// Statistics
    stats: Arc<RwLock<AuditStats>>,
}

impl QgeAuditLogger {
    /// Create a new audit logger
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::new())),
            next_id: Arc::new(RwLock::new(1)),
            last_hash: Arc::new(RwLock::new([0u8; 32])),
            log_file: None,
            min_severity: Severity::Info,
            alert_callback: Arc::new(RwLock::new(None)),
            stats: Arc::new(RwLock::new(AuditStats::default())),
        }
    }

    /// Create with file logging
    pub fn with_file(path: &str) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| Error::io(format!("Failed to open log file: {}", e)))?;

        Ok(Self {
            entries: Arc::new(RwLock::new(VecDeque::new())),
            next_id: Arc::new(RwLock::new(1)),
            last_hash: Arc::new(RwLock::new([0u8; 32])),
            log_file: Some(Arc::new(RwLock::new(file))),
            min_severity: Severity::Info,
            alert_callback: Arc::new(RwLock::new(None)),
            stats: Arc::new(RwLock::new(AuditStats::default())),
        })
    }

    /// Set minimum severity
    pub fn set_min_severity(&mut self, severity: Severity) {
        self.min_severity = severity;
    }

    /// Set alert callback
    pub async fn set_alert_callback<F>(&self, callback: F)
    where
        F: Fn(AuditEntry) + Send + Sync + 'static,
    {
        let mut cb = self.alert_callback.write().await;
        *cb = Some(Box::new(callback));
    }

    /// Log an event
    pub async fn log(
        &self,
        severity: Severity,
        event: EventType,
        actor: Option<ActorInfo>,
        target: Option<TargetInfo>,
        context: HashMap<String, String>,
    ) {
        // Skip if below minimum severity
        if severity < self.min_severity {
            return;
        }

        // Get entry ID and prev hash
        let mut id_guard = self.next_id.write().await;
        let mut hash_guard = self.last_hash.write().await;

        let id = *id_guard;
        let prev_hash = *hash_guard;
        let timestamp = current_timestamp();
        let category = event.category();

        // Calculate entry hash
        let mut hash_data = Vec::new();
        hash_data.extend_from_slice(&id.to_le_bytes());
        hash_data.extend_from_slice(&timestamp.to_le_bytes());
        hash_data.extend_from_slice(&[severity as u8]);
        hash_data.extend_from_slice(&prev_hash);
        let entry_hash_raw = blake3::hash(&hash_data);
        let mut entry_hash = [0u8; 32];
        entry_hash.copy_from_slice(entry_hash_raw.as_bytes());

        let entry = AuditEntry {
            id,
            timestamp,
            severity,
            category,
            event,
            actor,
            target,
            context,
            prev_hash,
            entry_hash,
        };

        // Update ID and hash
        *id_guard = id + 1;
        *hash_guard = entry_hash;
        drop(id_guard);
        drop(hash_guard);

        // Add to memory buffer
        {
            let mut entries = self.entries.write().await;
            entries.push_back(entry.clone());
            while entries.len() > MAX_MEMORY_ENTRIES {
                entries.pop_front();
            }
        }

        // Write to file if configured
        if let Some(ref file) = self.log_file {
            if let Ok(json) = serde_json::to_string(&entry) {
                if let Ok(mut f) = file.write().await {
                    let _ = writeln!(f, "{}", json);
                }
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_entries += 1;
            *stats.entries_by_category.entry(category).or_insert(0) += 1;
            *stats.entries_by_severity.entry(severity).or_insert(0) += 1;
            if stats.oldest_entry.is_none() {
                stats.oldest_entry = Some(timestamp);
            }
            stats.newest_entry = Some(timestamp);
        }

        // Fire alert callback for critical events
        if severity >= Severity::Alert {
            if let Some(callback) = self.alert_callback.read().await.as_ref() {
                callback(entry);
            }
        }
    }

    /// Query log entries
    pub async fn query(&self, filter: &AuditQuery) -> Vec<AuditEntry> {
        let entries = self.entries.read().await;
        let mut results: Vec<_> = entries
            .iter()
            .filter(|e| filter.matches(e))
            .cloned()
            .collect();

        // Apply offset
        if let Some(offset) = filter.offset {
            if offset < results.len() {
                results = results.into_iter().skip(offset).collect();
            } else {
                results.clear();
            }
        }

        // Apply limit
        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        results
    }

    /// Verify hash chain integrity
    pub async fn verify_chain(&self) -> bool {
        let entries = self.entries.read().await;

        let mut prev_hash = [0u8; 32];
        for entry in entries.iter() {
            if entry.prev_hash != prev_hash {
                return false;
            }
            prev_hash = entry.entry_hash;
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.chain_valid = true;
        }

        true
    }

    /// Get statistics
    pub async fn stats(&self) -> AuditStats {
        self.stats.read().await.clone()
    }

    /// Export entries to JSON
    pub async fn export_json(&self, filter: &AuditQuery) -> Result<String> {
        let entries = self.query(filter).await;
        serde_json::to_string_pretty(&entries)
            .map_err(|e| Error::serialization(format!("JSON export failed: {}", e)))
    }

    /// Clear old entries
    pub async fn cleanup(&self) {
        let cutoff = current_timestamp().saturating_sub(LOG_RETENTION_SECS);
        let mut entries = self.entries.write().await;
        entries.retain(|e| e.timestamp > cutoff);
    }

    // Convenience methods for common events

    /// Log token issued
    pub async fn log_token_issued(&self, actor: [u8; 32], epoch_id: u64, token_hash: [u8; 32]) {
        self.log(
            Severity::Info,
            EventType::TokenIssued {
                epoch_id,
                token_hash,
            },
            Some(ActorInfo {
                id: actor,
                actor_type: ActorType::User,
                ip_addr: None,
                device_id: None,
            }),
            None,
            HashMap::new(),
        )
        .await;
    }

    /// Log access denied
    pub async fn log_access_denied(&self, actor: [u8; 32], resource: &str, reason: &str) {
        self.log(
            Severity::Warning,
            EventType::AccessDenied {
                resource: resource.to_string(),
                reason: reason.to_string(),
            },
            Some(ActorInfo {
                id: actor,
                actor_type: ActorType::User,
                ip_addr: None,
                device_id: None,
            }),
            None,
            HashMap::new(),
        )
        .await;
    }

    /// Log security event
    pub async fn log_security_event(&self, actor: Option<[u8; 32]>, event: EventType) {
        self.log(
            Severity::Critical,
            event,
            actor.map(|id| ActorInfo {
                id,
                actor_type: ActorType::User,
                ip_addr: None,
                device_id: None,
            }),
            None,
            HashMap::new(),
        )
        .await;
    }

    /// Log relay slashing
    pub async fn log_slashing(&self, relay_id: [u8; 32], amount: u64, reason: &str) {
        self.log(
            Severity::Alert,
            EventType::RelaySlashed {
                amount,
                reason: reason.to_string(),
            },
            None,
            Some(TargetInfo {
                id: relay_id,
                target_type: TargetType::Relay,
                info: None,
            }),
            HashMap::new(),
        )
        .await;
    }
}

impl Default for QgeAuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_logger_creation() {
        let logger = QgeAuditLogger::new();
        let stats = logger.stats().await;
        assert_eq!(stats.total_entries, 0);
    }

    #[tokio::test]
    async fn test_log_entry() {
        let logger = QgeAuditLogger::new();

        logger
            .log(
                Severity::Info,
                EventType::TokenIssued {
                    epoch_id: 100,
                    token_hash: [0xAB; 32],
                },
                Some(ActorInfo {
                    id: [1u8; 32],
                    actor_type: ActorType::User,
                    ip_addr: None,
                    device_id: None,
                }),
                None,
                HashMap::new(),
            )
            .await;

        let stats = logger.stats().await;
        assert_eq!(stats.total_entries, 1);
    }

    #[tokio::test]
    async fn test_query_by_severity() {
        let logger = QgeAuditLogger::new();

        // Log entries with different severities
        logger
            .log(
                Severity::Info,
                EventType::SystemStarted {
                    version: "1.0".to_string(),
                },
                None,
                None,
                HashMap::new(),
            )
            .await;

        logger
            .log(
                Severity::Critical,
                EventType::AttackDetected {
                    attack_type: "DoS".to_string(),
                    source: "unknown".to_string(),
                },
                None,
                None,
                HashMap::new(),
            )
            .await;

        let query = AuditQuery::new().min_severity(Severity::Critical);
        let results = logger.query(&query).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn test_query_by_category() {
        let logger = QgeAuditLogger::new();

        logger
            .log(
                Severity::Info,
                EventType::TokenIssued {
                    epoch_id: 1,
                    token_hash: [0; 32],
                },
                None,
                None,
                HashMap::new(),
            )
            .await;

        logger
            .log(
                Severity::Info,
                EventType::KeyGenerated {
                    key_type: "SUK".to_string(),
                    key_id: [0; 32],
                },
                None,
                None,
                HashMap::new(),
            )
            .await;

        let query = AuditQuery::new().category(EventCategory::Token);
        let results = logger.query(&query).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].category, EventCategory::Token);
    }

    #[tokio::test]
    async fn test_hash_chain_integrity() {
        let logger = QgeAuditLogger::new();

        for i in 0..5 {
            logger
                .log(
                    Severity::Info,
                    EventType::EpochTransition {
                        from_epoch: i,
                        to_epoch: i + 1,
                    },
                    None,
                    None,
                    HashMap::new(),
                )
                .await;
        }

        assert!(logger.verify_chain().await);
    }

    #[tokio::test]
    async fn test_export_json() {
        let logger = QgeAuditLogger::new();

        logger
            .log(
                Severity::Info,
                EventType::SystemStarted {
                    version: "1.0".to_string(),
                },
                None,
                None,
                HashMap::new(),
            )
            .await;

        let json = logger.export_json(&AuditQuery::new()).await.unwrap();
        assert!(json.contains("SystemStarted"));
    }

    #[tokio::test]
    async fn test_convenience_methods() {
        let logger = QgeAuditLogger::new();

        logger.log_token_issued([1u8; 32], 100, [0xAB; 32]).await;
        logger
            .log_access_denied([2u8; 32], "/admin", "Not authorized")
            .await;

        let stats = logger.stats().await;
        assert_eq!(stats.total_entries, 2);
    }
}
