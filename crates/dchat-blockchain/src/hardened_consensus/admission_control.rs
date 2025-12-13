//! Admission Control & Backpressure
//!
//! Implements DoS protection through:
//! - Bounded queues with priority lanes
//! - Per-peer message budgets
//! - Diversity-constrained load balancing (ASN/IP-prefix/region)
//! - Adaptive rate limiting based on system load
//!
//! Security: Prevents resource exhaustion attacks while
//! prioritizing consensus-critical traffic.

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Maximum queue depth per priority lane
pub const MAX_QUEUE_DEPTH: usize = 10_000;

/// Maximum messages per peer per second
pub const DEFAULT_PEER_RATE_LIMIT: u32 = 100;

/// Maximum concurrent connections per IP prefix (/24 for IPv4, /48 for IPv6)
pub const MAX_CONNECTIONS_PER_IP_PREFIX: usize = 10;

/// Maximum concurrent connections per ASN
pub const MAX_CONNECTIONS_PER_ASN: usize = 50;

/// Maximum concurrent connections per region
pub const MAX_CONNECTIONS_PER_REGION: usize = 500;

/// Budget refill interval
pub const BUDGET_REFILL_INTERVAL_MS: u64 = 1000;

/// Admission decision - result of admission check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionDecision {
    /// Accept the message immediately
    Accept,
    /// Reject with reason
    Reject(String),
    /// Defer processing (queue is full but not overloaded)
    Defer,
}

/// Admission control errors
#[derive(Debug, Error)]
pub enum AdmissionError {
    #[error("Queue full for priority {0:?}")]
    QueueFull(Priority),

    #[error("Peer {0} rate limited")]
    RateLimited(String),

    #[error("Peer {0} budget exhausted")]
    BudgetExhausted(String),

    #[error("IP prefix {0} connection limit reached")]
    IpPrefixLimitReached(String),

    #[error("ASN {0} connection limit reached")]
    AsnLimitReached(u32),

    #[error("Region {0:?} connection limit reached")]
    RegionLimitReached(Region),

    #[error("System overloaded")]
    SystemOverloaded,

    #[error("Message too large: {0} bytes (max {1})")]
    MessageTooLarge(usize, usize),

    #[error("Invalid message type")]
    InvalidMessageType,

    #[error("Peer not registered: {0}")]
    PeerNotRegistered(String),
}

/// Message priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    /// Highest: Consensus messages (votes, proposals)
    Consensus = 0,
    /// High: Finality proofs (PoT, TSC)
    Finality = 1,
    /// Medium: Block propagation
    Blocks = 2,
    /// Normal: Message delivery, gossip
    Messages = 3,
    /// Low: Sync requests, discovery
    Sync = 4,
    /// Lowest: Metrics, health checks
    Background = 5,
}

impl Priority {
    /// Get all priorities in order
    pub fn all() -> &'static [Priority] {
        &[
            Priority::Consensus,
            Priority::Finality,
            Priority::Blocks,
            Priority::Messages,
            Priority::Sync,
            Priority::Background,
        ]
    }

    /// Get queue capacity for this priority
    pub fn queue_capacity(&self) -> usize {
        match self {
            Priority::Consensus => MAX_QUEUE_DEPTH,
            Priority::Finality => MAX_QUEUE_DEPTH / 2,
            Priority::Blocks => MAX_QUEUE_DEPTH / 2,
            Priority::Messages => MAX_QUEUE_DEPTH / 4,
            Priority::Sync => MAX_QUEUE_DEPTH / 4,
            Priority::Background => MAX_QUEUE_DEPTH / 8,
        }
    }

    /// Get max message size for this priority
    pub fn max_message_size(&self) -> usize {
        match self {
            Priority::Consensus => 64 * 1024,      // 64 KB
            Priority::Finality => 128 * 1024,      // 128 KB
            Priority::Blocks => 2 * 1024 * 1024,   // 2 MB
            Priority::Messages => 1 * 1024 * 1024, // 1 MB
            Priority::Sync => 4 * 1024 * 1024,     // 4 MB
            Priority::Background => 16 * 1024,     // 16 KB
        }
    }
}

/// Geographic region for diversity (uses epoch_snapshot::Region for canonical type)
pub use super::epoch_snapshot::Region;

/// Peer identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub [u8; 32]);

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(&self.0[..8]))
    }
}

/// Peer connection metadata
#[derive(Debug, Clone)]
pub struct PeerMetadata {
    pub peer_id: PeerId,
    pub ip_addr: IpAddr,
    pub ip_prefix: String,
    pub asn: u32,
    pub region: Region,
    pub connected_at: Instant,
    pub is_relay: bool,
}

impl PeerMetadata {
    /// Calculate IP prefix (/24 for IPv4, /48 for IPv6)
    pub fn calculate_ip_prefix(ip: &IpAddr) -> String {
        match ip {
            IpAddr::V4(v4) => {
                let octets = v4.octets();
                format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2])
            }
            IpAddr::V6(v6) => {
                let segments = v6.segments();
                format!("{:x}:{:x}:{:x}::/48", segments[0], segments[1], segments[2])
            }
        }
    }
}

/// Per-peer budget tracking
#[derive(Debug)]
pub struct PeerBudget {
    /// Messages remaining in current window
    pub messages_remaining: AtomicU64,
    /// Bytes remaining in current window
    pub bytes_remaining: AtomicU64,
    /// Last refill time
    pub last_refill: parking_lot::Mutex<Instant>,
    /// Rate limit (messages per second)
    pub rate_limit: u32,
    /// Byte limit per second
    pub byte_limit: u64,
    /// Total messages processed
    pub total_messages: AtomicU64,
    /// Total bytes processed
    pub total_bytes: AtomicU64,
    /// Messages dropped due to rate limiting
    pub dropped_messages: AtomicU64,
}

impl PeerBudget {
    pub fn new(rate_limit: u32, byte_limit: u64) -> Self {
        Self {
            messages_remaining: AtomicU64::new(rate_limit as u64),
            bytes_remaining: AtomicU64::new(byte_limit),
            last_refill: parking_lot::Mutex::new(Instant::now()),
            rate_limit,
            byte_limit,
            total_messages: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            dropped_messages: AtomicU64::new(0),
        }
    }

    /// Try to consume budget for a message
    pub fn try_consume(&self, bytes: u64) -> bool {
        self.maybe_refill();

        // Try to decrement messages
        let mut current = self.messages_remaining.load(Ordering::Acquire);
        loop {
            if current == 0 {
                self.dropped_messages.fetch_add(1, Ordering::Relaxed);
                return false;
            }

            match self.messages_remaining.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(new) => current = new,
            }
        }

        // Try to decrement bytes
        current = self.bytes_remaining.load(Ordering::Acquire);
        loop {
            if current < bytes {
                // Rollback message count
                self.messages_remaining.fetch_add(1, Ordering::Release);
                self.dropped_messages.fetch_add(1, Ordering::Relaxed);
                return false;
            }

            match self.bytes_remaining.compare_exchange_weak(
                current,
                current - bytes,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(new) => current = new,
            }
        }

        self.total_messages.fetch_add(1, Ordering::Relaxed);
        self.total_bytes.fetch_add(bytes, Ordering::Relaxed);
        true
    }

    fn maybe_refill(&self) {
        let now = Instant::now();
        let mut last = self.last_refill.lock();

        let elapsed = now.duration_since(*last);
        if elapsed.as_millis() >= BUDGET_REFILL_INTERVAL_MS as u128 {
            let periods = elapsed.as_millis() / BUDGET_REFILL_INTERVAL_MS as u128;
            let message_refill = (self.rate_limit as u128 * periods).min(self.rate_limit as u128);
            let byte_refill = (self.byte_limit as u128 * periods).min(self.byte_limit as u128);

            self.messages_remaining
                .store(message_refill as u64, Ordering::Release);
            self.bytes_remaining
                .store(byte_refill as u64, Ordering::Release);
            *last = now;
        }
    }
}

/// Queued message
pub struct QueuedMessage {
    pub peer_id: PeerId,
    pub priority: Priority,
    pub data: Vec<u8>,
    pub enqueued_at: Instant,
}

/// Priority queue with bounded lanes
pub struct PriorityQueue {
    /// Lanes by priority
    lanes: HashMap<Priority, (Sender<QueuedMessage>, Receiver<QueuedMessage>)>,
    /// Current sizes
    sizes: HashMap<Priority, AtomicUsize>,
    /// Total enqueued
    total_enqueued: AtomicU64,
    /// Total dequeued
    total_dequeued: AtomicU64,
    /// Total dropped
    total_dropped: AtomicU64,
}

impl PriorityQueue {
    pub fn new() -> Self {
        let mut lanes = HashMap::new();
        let mut sizes = HashMap::new();

        for priority in Priority::all() {
            let capacity = priority.queue_capacity();
            let (tx, rx) = bounded(capacity);
            lanes.insert(*priority, (tx, rx));
            sizes.insert(*priority, AtomicUsize::new(0));
        }

        Self {
            lanes,
            sizes,
            total_enqueued: AtomicU64::new(0),
            total_dequeued: AtomicU64::new(0),
            total_dropped: AtomicU64::new(0),
        }
    }

    /// Enqueue a message
    pub fn enqueue(&self, msg: QueuedMessage) -> Result<(), AdmissionError> {
        let priority = msg.priority;

        let (sender, _) = self
            .lanes
            .get(&priority)
            .ok_or(AdmissionError::InvalidMessageType)?;

        match sender.try_send(msg) {
            Ok(()) => {
                self.sizes
                    .get(&priority)
                    .unwrap()
                    .fetch_add(1, Ordering::Relaxed);
                self.total_enqueued.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                self.total_dropped.fetch_add(1, Ordering::Relaxed);
                Err(AdmissionError::QueueFull(priority))
            }
            Err(TrySendError::Disconnected(_)) => Err(AdmissionError::QueueFull(priority)),
        }
    }

    /// Dequeue highest priority message
    pub fn dequeue(&self) -> Option<QueuedMessage> {
        for priority in Priority::all() {
            if let Some((_, receiver)) = self.lanes.get(priority) {
                if let Ok(msg) = receiver.try_recv() {
                    self.sizes
                        .get(priority)
                        .unwrap()
                        .fetch_sub(1, Ordering::Relaxed);
                    self.total_dequeued.fetch_add(1, Ordering::Relaxed);
                    return Some(msg);
                }
            }
        }
        None
    }

    /// Dequeue with timeout
    pub fn dequeue_timeout(&self, timeout: Duration) -> Option<QueuedMessage> {
        let deadline = Instant::now() + timeout;
        let per_lane_timeout = timeout / Priority::all().len() as u32;

        for priority in Priority::all() {
            if Instant::now() >= deadline {
                break;
            }

            if let Some((_, receiver)) = self.lanes.get(priority) {
                if let Ok(msg) = receiver.recv_timeout(per_lane_timeout) {
                    self.sizes
                        .get(priority)
                        .unwrap()
                        .fetch_sub(1, Ordering::Relaxed);
                    self.total_dequeued.fetch_add(1, Ordering::Relaxed);
                    return Some(msg);
                }
            }
        }
        None
    }

    /// Get queue sizes
    pub fn get_sizes(&self) -> HashMap<Priority, usize> {
        self.sizes
            .iter()
            .map(|(p, s)| (*p, s.load(Ordering::Relaxed)))
            .collect()
    }

    /// Get total size
    pub fn total_size(&self) -> usize {
        self.sizes.values().map(|s| s.load(Ordering::Relaxed)).sum()
    }

    /// Check if a priority lane is full
    pub fn is_full_for_priority(&self, priority: Priority) -> bool {
        let current_size = self
            .sizes
            .get(&priority)
            .map(|s| s.load(Ordering::Relaxed))
            .unwrap_or(0);
        let capacity = priority.queue_capacity();
        current_size >= capacity
    }

    /// Get stats
    pub fn stats(&self) -> QueueStats {
        QueueStats {
            total_enqueued: self.total_enqueued.load(Ordering::Relaxed),
            total_dequeued: self.total_dequeued.load(Ordering::Relaxed),
            total_dropped: self.total_dropped.load(Ordering::Relaxed),
            current_size: self.total_size(),
            lane_sizes: self.get_sizes(),
        }
    }
}

impl Default for PriorityQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Queue statistics
#[derive(Debug, Clone)]
pub struct QueueStats {
    pub total_enqueued: u64,
    pub total_dequeued: u64,
    pub total_dropped: u64,
    pub current_size: usize,
    pub lane_sizes: HashMap<Priority, usize>,
}

/// Diversity constraints tracker
pub struct DiversityTracker {
    /// Connections per IP prefix
    ip_prefix_counts: DashMap<String, usize>,
    /// Connections per ASN
    asn_counts: DashMap<u32, usize>,
    /// Connections per region
    region_counts: DashMap<Region, usize>,
    /// Limits
    max_per_ip_prefix: usize,
    max_per_asn: usize,
    max_per_region: usize,
}

impl DiversityTracker {
    pub fn new() -> Self {
        Self {
            ip_prefix_counts: DashMap::new(),
            asn_counts: DashMap::new(),
            region_counts: DashMap::new(),
            max_per_ip_prefix: MAX_CONNECTIONS_PER_IP_PREFIX,
            max_per_asn: MAX_CONNECTIONS_PER_ASN,
            max_per_region: MAX_CONNECTIONS_PER_REGION,
        }
    }

    /// Check if a new connection can be accepted
    pub fn can_accept(&self, metadata: &PeerMetadata) -> Result<(), AdmissionError> {
        // Check IP prefix
        let ip_count = self
            .ip_prefix_counts
            .get(&metadata.ip_prefix)
            .map(|r| *r)
            .unwrap_or(0);

        if ip_count >= self.max_per_ip_prefix {
            return Err(AdmissionError::IpPrefixLimitReached(
                metadata.ip_prefix.clone(),
            ));
        }

        // Check ASN
        let asn_count = self.asn_counts.get(&metadata.asn).map(|r| *r).unwrap_or(0);

        if asn_count >= self.max_per_asn {
            return Err(AdmissionError::AsnLimitReached(metadata.asn));
        }

        // Check region
        let region_count = self
            .region_counts
            .get(&metadata.region)
            .map(|r| *r)
            .unwrap_or(0);

        if region_count >= self.max_per_region {
            return Err(AdmissionError::RegionLimitReached(metadata.region));
        }

        Ok(())
    }

    /// Register a new connection
    pub fn register(&self, metadata: &PeerMetadata) -> Result<(), AdmissionError> {
        self.can_accept(metadata)?;

        *self
            .ip_prefix_counts
            .entry(metadata.ip_prefix.clone())
            .or_insert(0) += 1;
        *self.asn_counts.entry(metadata.asn).or_insert(0) += 1;
        *self.region_counts.entry(metadata.region).or_insert(0) += 1;

        Ok(())
    }

    /// Unregister a connection
    pub fn unregister(&self, metadata: &PeerMetadata) {
        if let Some(mut count) = self.ip_prefix_counts.get_mut(&metadata.ip_prefix) {
            *count = count.saturating_sub(1);
        }

        if let Some(mut count) = self.asn_counts.get_mut(&metadata.asn) {
            *count = count.saturating_sub(1);
        }

        if let Some(mut count) = self.region_counts.get_mut(&metadata.region) {
            *count = count.saturating_sub(1);
        }
    }

    /// Get diversity stats
    pub fn stats(&self) -> DiversityStats {
        DiversityStats {
            unique_ip_prefixes: self.ip_prefix_counts.len(),
            unique_asns: self.asn_counts.len(),
            region_distribution: self
                .region_counts
                .iter()
                .map(|r| (*r.key(), *r.value()))
                .collect(),
        }
    }
}

impl Default for DiversityTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Diversity statistics
#[derive(Debug, Clone)]
pub struct DiversityStats {
    pub unique_ip_prefixes: usize,
    pub unique_asns: usize,
    pub region_distribution: HashMap<Region, usize>,
}

/// System load tracker
pub struct LoadTracker {
    /// Current CPU utilization (0-100)
    cpu_utilization: AtomicU64,
    /// Current memory utilization (0-100)
    memory_utilization: AtomicU64,
    /// Current queue depth ratio (0-100)
    queue_depth_ratio: AtomicU64,
    /// Load threshold for throttling
    throttle_threshold: u64,
    /// Load threshold for rejection
    reject_threshold: u64,
}

impl LoadTracker {
    pub fn new(throttle_threshold: u64, reject_threshold: u64) -> Self {
        Self {
            cpu_utilization: AtomicU64::new(0),
            memory_utilization: AtomicU64::new(0),
            queue_depth_ratio: AtomicU64::new(0),
            throttle_threshold,
            reject_threshold,
        }
    }

    /// Update load metrics
    pub fn update(&self, cpu: u64, memory: u64, queue_ratio: u64) {
        self.cpu_utilization.store(cpu, Ordering::Relaxed);
        self.memory_utilization.store(memory, Ordering::Relaxed);
        self.queue_depth_ratio.store(queue_ratio, Ordering::Relaxed);
    }

    /// Get current load level
    pub fn current_load(&self) -> u64 {
        let cpu = self.cpu_utilization.load(Ordering::Relaxed);
        let mem = self.memory_utilization.load(Ordering::Relaxed);
        let queue = self.queue_depth_ratio.load(Ordering::Relaxed);

        // Weighted average: CPU 40%, Memory 30%, Queue 30%
        (cpu * 40 + mem * 30 + queue * 30) / 100
    }

    /// Check if should throttle
    pub fn should_throttle(&self) -> bool {
        self.current_load() >= self.throttle_threshold
    }

    /// Check if should reject
    pub fn should_reject(&self) -> bool {
        self.current_load() >= self.reject_threshold
    }

    /// Get adaptive rate multiplier (1.0 = normal, <1.0 = throttle)
    pub fn rate_multiplier(&self) -> f64 {
        let load = self.current_load();

        if load < self.throttle_threshold {
            1.0
        } else if load >= self.reject_threshold {
            0.0
        } else {
            let range = self.reject_threshold - self.throttle_threshold;
            let position = load - self.throttle_threshold;
            1.0 - (position as f64 / range as f64)
        }
    }
}

impl Default for LoadTracker {
    fn default() -> Self {
        Self::new(70, 90) // Throttle at 70%, reject at 90%
    }
}

/// Main admission controller
pub struct AdmissionController {
    /// Per-peer budgets
    peer_budgets: DashMap<PeerId, Arc<PeerBudget>>,
    /// Peer metadata
    peer_metadata: DashMap<PeerId, PeerMetadata>,
    /// Priority queue
    queue: Arc<PriorityQueue>,
    /// Diversity tracker
    diversity: Arc<DiversityTracker>,
    /// Load tracker
    load: Arc<LoadTracker>,
    /// Default rate limit
    default_rate_limit: u32,
    /// Default byte limit (per second)
    default_byte_limit: u64,
    /// Total connections
    total_connections: AtomicUsize,
    /// Max total connections
    max_connections: usize,
}

impl AdmissionController {
    pub fn new(max_connections: usize) -> Self {
        Self {
            peer_budgets: DashMap::new(),
            peer_metadata: DashMap::new(),
            queue: Arc::new(PriorityQueue::new()),
            diversity: Arc::new(DiversityTracker::new()),
            load: Arc::new(LoadTracker::default()),
            default_rate_limit: DEFAULT_PEER_RATE_LIMIT,
            default_byte_limit: 10 * 1024 * 1024, // 10 MB/s
            total_connections: AtomicUsize::new(0),
            max_connections,
        }
    }

    /// Register a new peer
    pub fn register_peer(&self, metadata: PeerMetadata) -> Result<(), AdmissionError> {
        // Check total connections
        let current = self.total_connections.load(Ordering::Acquire);
        if current >= self.max_connections {
            return Err(AdmissionError::SystemOverloaded);
        }

        // Check diversity constraints
        self.diversity.register(&metadata)?;

        // Create budget
        let budget = Arc::new(PeerBudget::new(
            self.default_rate_limit,
            self.default_byte_limit,
        ));

        let peer_id = metadata.peer_id.clone();
        self.peer_budgets.insert(peer_id.clone(), budget);
        self.peer_metadata.insert(peer_id, metadata);
        self.total_connections.fetch_add(1, Ordering::Release);

        Ok(())
    }

    /// Unregister a peer
    pub fn unregister_peer(&self, peer_id: &PeerId) {
        if let Some((_, metadata)) = self.peer_metadata.remove(peer_id) {
            self.diversity.unregister(&metadata);
            self.peer_budgets.remove(peer_id);
            self.total_connections.fetch_sub(1, Ordering::Release);
        }
    }

    /// Admit a message
    pub fn admit(
        &self,
        peer_id: &PeerId,
        priority: Priority,
        data: Vec<u8>,
    ) -> Result<(), AdmissionError> {
        // Check load
        if self.load.should_reject() {
            return Err(AdmissionError::SystemOverloaded);
        }

        // Check message size
        let max_size = priority.max_message_size();
        if data.len() > max_size {
            return Err(AdmissionError::MessageTooLarge(data.len(), max_size));
        }

        // Check peer budget
        let budget = self
            .peer_budgets
            .get(peer_id)
            .ok_or_else(|| AdmissionError::PeerNotRegistered(peer_id.to_string()))?;

        // Apply load-based rate limiting
        let effective_bytes = if self.load.should_throttle() {
            let multiplier = self.load.rate_multiplier();
            ((data.len() as f64) / multiplier) as u64
        } else {
            data.len() as u64
        };

        if !budget.try_consume(effective_bytes) {
            return Err(AdmissionError::BudgetExhausted(peer_id.to_string()));
        }

        // Enqueue
        let msg = QueuedMessage {
            peer_id: peer_id.clone(),
            priority,
            data,
            enqueued_at: Instant::now(),
        };

        self.queue.enqueue(msg)
    }

    /// Get next message
    pub fn next(&self) -> Option<QueuedMessage> {
        self.queue.dequeue()
    }

    /// Get next message with timeout
    pub fn next_timeout(&self, timeout: Duration) -> Option<QueuedMessage> {
        self.queue.dequeue_timeout(timeout)
    }

    /// Update load metrics
    pub fn update_load(&self, cpu: u64, memory: u64) {
        let queue_ratio = (self.queue.total_size() * 100 / MAX_QUEUE_DEPTH).min(100) as u64;
        self.load.update(cpu, memory, queue_ratio);
    }

    /// Get stats
    pub fn stats(&self) -> AdmissionStats {
        AdmissionStats {
            total_peers: self.peer_budgets.len(),
            total_connections: self.total_connections.load(Ordering::Relaxed),
            queue_stats: self.queue.stats(),
            diversity_stats: self.diversity.stats(),
            current_load: self.load.current_load(),
            is_throttling: self.load.should_throttle(),
            is_rejecting: self.load.should_reject(),
        }
    }

    /// Get peer budget stats
    pub fn peer_stats(&self, peer_id: &PeerId) -> Option<PeerStats> {
        self.peer_budgets.get(peer_id).map(|budget| PeerStats {
            messages_remaining: budget.messages_remaining.load(Ordering::Relaxed),
            bytes_remaining: budget.bytes_remaining.load(Ordering::Relaxed),
            total_messages: budget.total_messages.load(Ordering::Relaxed),
            total_bytes: budget.total_bytes.load(Ordering::Relaxed),
            dropped_messages: budget.dropped_messages.load(Ordering::Relaxed),
        })
    }

    /// Set custom rate limit for a peer
    pub fn set_peer_rate_limit(&self, peer_id: &PeerId, rate_limit: u32, byte_limit: u64) {
        if let Some(old) = self.peer_budgets.get(peer_id) {
            old.messages_remaining
                .store(rate_limit as u64, Ordering::Release);
            old.bytes_remaining.store(byte_limit, Ordering::Release);
        }
    }

    /// Get queue reference
    pub fn queue(&self) -> Arc<PriorityQueue> {
        self.queue.clone()
    }

    /// Get diversity tracker reference
    pub fn diversity(&self) -> Arc<DiversityTracker> {
        self.diversity.clone()
    }

    /// Get load tracker reference
    pub fn load_tracker(&self) -> Arc<LoadTracker> {
        self.load.clone()
    }

    /// Check admission without actually admitting the message
    /// Returns a decision that can be acted upon
    pub fn check_admission(
        &self,
        peer_id: &PeerId,
        priority: Priority,
        message_size: usize,
    ) -> AdmissionDecision {
        // Check load first
        if self.load.should_reject() {
            return AdmissionDecision::Reject("System overloaded".to_string());
        }

        // Check message size
        let max_size = priority.max_message_size();
        if message_size > max_size {
            return AdmissionDecision::Reject(format!(
                "Message too large: {} bytes (max {})",
                message_size, max_size
            ));
        }

        // Check peer budget
        let budget = match self.peer_budgets.get(peer_id) {
            Some(b) => b,
            None => return AdmissionDecision::Reject("Peer not registered".to_string()),
        };

        // Check message budget
        let current_msgs = budget.messages_remaining.load(Ordering::Acquire);
        if current_msgs == 0 {
            return AdmissionDecision::Reject("Rate limited".to_string());
        }

        // Check byte budget
        let current_bytes = budget.bytes_remaining.load(Ordering::Acquire);
        if (message_size as u64) > current_bytes {
            return AdmissionDecision::Reject("Byte budget exhausted".to_string());
        }

        // Check queue capacity
        if self.queue.is_full_for_priority(priority) {
            // Check if we should defer instead of reject
            if self.load.should_throttle() {
                return AdmissionDecision::Defer;
            }
            return AdmissionDecision::Reject("Queue full".to_string());
        }

        AdmissionDecision::Accept
    }
}

/// Admission statistics
#[derive(Debug, Clone)]
pub struct AdmissionStats {
    pub total_peers: usize,
    pub total_connections: usize,
    pub queue_stats: QueueStats,
    pub diversity_stats: DiversityStats,
    pub current_load: u64,
    pub is_throttling: bool,
    pub is_rejecting: bool,
}

/// Per-peer statistics
#[derive(Debug, Clone)]
pub struct PeerStats {
    pub messages_remaining: u64,
    pub bytes_remaining: u64,
    pub total_messages: u64,
    pub total_bytes: u64,
    pub dropped_messages: u64,
}

/// Priority classifier for message types
pub mod classifier {
    use super::*;

    /// Message type identifiers
    #[derive(Debug, Clone, Copy)]
    pub enum MessageType {
        // Consensus
        BlockProposal,
        BlockVote,
        ViewChange,

        // Finality
        PoTProof,
        TSCVote,
        TSCCheckpoint,

        // Blocks
        BlockHeader,
        BlockBody,
        BlockAnnounce,

        // Messages
        DirectMessage,
        ChannelMessage,
        GossipMessage,

        // Sync
        SyncRequest,
        SyncResponse,
        PeerDiscovery,

        // Background
        Ping,
        Pong,
        Metrics,
    }

    impl MessageType {
        /// Get priority for message type
        pub fn priority(&self) -> Priority {
            match self {
                // Consensus
                MessageType::BlockProposal | MessageType::BlockVote | MessageType::ViewChange => {
                    Priority::Consensus
                }

                // Finality
                MessageType::PoTProof | MessageType::TSCVote | MessageType::TSCCheckpoint => {
                    Priority::Finality
                }

                // Blocks
                MessageType::BlockHeader | MessageType::BlockBody | MessageType::BlockAnnounce => {
                    Priority::Blocks
                }

                // Messages
                MessageType::DirectMessage
                | MessageType::ChannelMessage
                | MessageType::GossipMessage => Priority::Messages,

                // Sync
                MessageType::SyncRequest
                | MessageType::SyncResponse
                | MessageType::PeerDiscovery => Priority::Sync,

                // Background
                MessageType::Ping | MessageType::Pong | MessageType::Metrics => {
                    Priority::Background
                }
            }
        }

        /// Classify from message tag byte
        pub fn from_tag(tag: u8) -> Option<Self> {
            match tag {
                // Consensus (0x0X)
                0x01 => Some(MessageType::BlockProposal),
                0x02 => Some(MessageType::BlockVote),
                0x03 => Some(MessageType::ViewChange),

                // Finality (0x1X)
                0x11 => Some(MessageType::PoTProof),
                0x12 => Some(MessageType::TSCVote),
                0x13 => Some(MessageType::TSCCheckpoint),

                // Blocks (0x2X)
                0x21 => Some(MessageType::BlockHeader),
                0x22 => Some(MessageType::BlockBody),
                0x23 => Some(MessageType::BlockAnnounce),

                // Messages (0x3X)
                0x31 => Some(MessageType::DirectMessage),
                0x32 => Some(MessageType::ChannelMessage),
                0x33 => Some(MessageType::GossipMessage),

                // Sync (0x4X)
                0x41 => Some(MessageType::SyncRequest),
                0x42 => Some(MessageType::SyncResponse),
                0x43 => Some(MessageType::PeerDiscovery),

                // Background (0x5X)
                0x51 => Some(MessageType::Ping),
                0x52 => Some(MessageType::Pong),
                0x53 => Some(MessageType::Metrics),

                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn test_peer_id(n: u8) -> PeerId {
        let mut bytes = [0u8; 32];
        bytes[0] = n;
        PeerId(bytes)
    }

    fn test_metadata(n: u8) -> PeerMetadata {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, n, 1));
        PeerMetadata {
            peer_id: test_peer_id(n),
            ip_addr: ip,
            ip_prefix: PeerMetadata::calculate_ip_prefix(&ip),
            asn: 64500 + n as u32,
            region: Region::NorthAmerica,
            connected_at: Instant::now(),
            is_relay: false,
        }
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Consensus < Priority::Finality);
        assert!(Priority::Finality < Priority::Blocks);
        assert!(Priority::Blocks < Priority::Messages);
        assert!(Priority::Messages < Priority::Sync);
        assert!(Priority::Sync < Priority::Background);
    }

    #[test]
    fn test_peer_budget() {
        let budget = PeerBudget::new(10, 1000);

        // Should succeed initially
        for _ in 0..10 {
            assert!(budget.try_consume(50));
        }

        // Should fail when exhausted
        assert!(!budget.try_consume(50));

        // Check stats
        assert_eq!(budget.total_messages.load(Ordering::Relaxed), 10);
        assert_eq!(budget.total_bytes.load(Ordering::Relaxed), 500);
        assert!(budget.dropped_messages.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_priority_queue() {
        let queue = PriorityQueue::new();

        // Enqueue in reverse priority order
        queue
            .enqueue(QueuedMessage {
                peer_id: test_peer_id(1),
                priority: Priority::Background,
                data: vec![1],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        queue
            .enqueue(QueuedMessage {
                peer_id: test_peer_id(2),
                priority: Priority::Consensus,
                data: vec![2],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        queue
            .enqueue(QueuedMessage {
                peer_id: test_peer_id(3),
                priority: Priority::Messages,
                data: vec![3],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        // Should dequeue in priority order
        let msg1 = queue.dequeue().unwrap();
        assert_eq!(msg1.priority, Priority::Consensus);
        assert_eq!(msg1.data, vec![2]);

        let msg2 = queue.dequeue().unwrap();
        assert_eq!(msg2.priority, Priority::Messages);
        assert_eq!(msg2.data, vec![3]);

        let msg3 = queue.dequeue().unwrap();
        assert_eq!(msg3.priority, Priority::Background);
        assert_eq!(msg3.data, vec![1]);
    }

    #[test]
    fn test_diversity_tracker() {
        let tracker = DiversityTracker::new();

        // First connection should succeed
        let meta1 = test_metadata(1);
        tracker.register(&meta1).unwrap();

        // Same IP prefix should eventually fail
        for i in 2..=MAX_CONNECTIONS_PER_IP_PREFIX {
            let mut meta = test_metadata(i as u8);
            meta.ip_prefix = meta1.ip_prefix.clone();
            meta.asn = 64500 + i as u32; // Different ASN
            tracker.register(&meta).unwrap();
        }

        // One more should fail
        let mut meta_fail = test_metadata(100);
        meta_fail.ip_prefix = meta1.ip_prefix.clone();
        meta_fail.asn = 64600;
        assert!(matches!(
            tracker.register(&meta_fail),
            Err(AdmissionError::IpPrefixLimitReached(_))
        ));
    }

    #[test]
    fn test_load_tracker() {
        let tracker = LoadTracker::new(70, 90);

        // Low load
        tracker.update(30, 40, 20);
        assert!(!tracker.should_throttle());
        assert!(!tracker.should_reject());
        assert_eq!(tracker.rate_multiplier(), 1.0);

        // Medium load (throttle)
        tracker.update(80, 70, 60);
        assert!(tracker.should_throttle());
        assert!(!tracker.should_reject());
        assert!(tracker.rate_multiplier() < 1.0);
        assert!(tracker.rate_multiplier() > 0.0);

        // High load (reject)
        tracker.update(95, 95, 95);
        assert!(tracker.should_throttle());
        assert!(tracker.should_reject());
        assert_eq!(tracker.rate_multiplier(), 0.0);
    }

    #[test]
    fn test_admission_controller() {
        let controller = AdmissionController::new(100);

        // Register peer
        let meta = test_metadata(1);
        controller.register_peer(meta.clone()).unwrap();

        // Admit message
        controller
            .admit(&meta.peer_id, Priority::Messages, vec![1, 2, 3, 4])
            .unwrap();

        // Dequeue message
        let msg = controller.next().unwrap();
        assert_eq!(msg.peer_id, meta.peer_id);
        assert_eq!(msg.data, vec![1, 2, 3, 4]);

        // Unregister
        controller.unregister_peer(&meta.peer_id);

        // Should fail now
        assert!(matches!(
            controller.admit(&meta.peer_id, Priority::Messages, vec![]),
            Err(AdmissionError::PeerNotRegistered(_))
        ));
    }

    #[test]
    fn test_message_classifier() {
        use classifier::MessageType;

        assert_eq!(MessageType::BlockProposal.priority(), Priority::Consensus);
        assert_eq!(MessageType::TSCVote.priority(), Priority::Finality);
        assert_eq!(MessageType::BlockHeader.priority(), Priority::Blocks);
        assert_eq!(MessageType::DirectMessage.priority(), Priority::Messages);
        assert_eq!(MessageType::SyncRequest.priority(), Priority::Sync);
        assert_eq!(MessageType::Ping.priority(), Priority::Background);
    }
}
