//! Events and execution receipts for program execution

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::account::Pubkey;
use crate::error::ProgramError;

/// Event index for fast lookups by discriminator and program
#[derive(Debug, Default)]
pub struct EventIndex {
    /// Events indexed by discriminator
    by_discriminator: HashMap<[u8; 8], Vec<EventId>>,
    /// Events indexed by program ID
    by_program: HashMap<Pubkey, Vec<EventId>>,
}

impl EventIndex {
    /// Create a new empty event index
    pub fn new() -> Self {
        Self::default()
    }

    /// Index an event for fast lookup
    pub fn index_event(&mut self, event: &ProgramEvent) {
        self.by_discriminator
            .entry(event.discriminator)
            .or_default()
            .push(event.id);
        self.by_program
            .entry(event.program_id)
            .or_default()
            .push(event.id);
    }

    /// Find events by discriminator
    pub fn find_by_discriminator(&self, discriminator: &[u8; 8]) -> &[EventId] {
        self.by_discriminator
            .get(discriminator)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Find events by program ID
    pub fn find_by_program(&self, program_id: &Pubkey) -> &[EventId] {
        self.by_program
            .get(program_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Unique event identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub [u8; 32]);

impl EventId {
    /// Generate event ID from components
    pub fn generate(transaction_hash: &[u8; 32], program_id: &Pubkey, event_index: u32) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(transaction_hash);
        hasher.update(&program_id.0);
        hasher.update(&event_index.to_le_bytes());
        Self(hasher.finalize().into())
    }

    /// Zero event ID
    pub const fn zero() -> Self {
        Self([0u8; 32])
    }
}

/// Program event emitted during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramEvent {
    /// Unique event ID
    pub id: EventId,
    /// Program that emitted the event
    pub program_id: Pubkey,
    /// Event discriminator (8-byte identifier)
    pub discriminator: [u8; 8],
    /// Event data
    pub data: Vec<u8>,
    /// Slot when event was emitted
    pub slot: u64,
    /// Index within transaction
    pub index: u32,
    /// Whether this is from a CPI
    pub is_cpi: bool,
    /// CPI depth (0 for top-level)
    pub cpi_depth: u8,
}

impl ProgramEvent {
    /// Create new event
    pub fn new(
        tx_hash: &[u8; 32],
        program_id: Pubkey,
        discriminator: [u8; 8],
        data: Vec<u8>,
        slot: u64,
        index: u32,
        cpi_depth: u8,
    ) -> Self {
        let id = EventId::generate(tx_hash, &program_id, index);
        Self {
            id,
            program_id,
            discriminator,
            data,
            slot,
            index,
            is_cpi: cpi_depth > 0,
            cpi_depth,
        }
    }

    /// Compute discriminator from event name
    pub fn compute_discriminator(name: &str) -> [u8; 8] {
        let hash = blake3::hash(format!("event:{}", name).as_bytes());
        let bytes: [u8; 32] = hash.into();
        let mut discriminator = [0u8; 8];
        discriminator.copy_from_slice(&bytes[..8]);
        discriminator
    }

    /// Event size for metering
    pub fn size(&self) -> usize {
        32 + // id
        32 + // program_id
        8 + // discriminator
        self.data.len() +
        8 + // slot
        4 + // index
        1 + // is_cpi
        1 // cpi_depth
    }
}

/// Log entry during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Log message
    pub message: String,
    /// Program that logged
    pub program_id: Pubkey,
    /// CPI depth
    pub depth: u8,
}

/// Account modification during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountDelta {
    /// Account pubkey
    pub pubkey: Pubkey,
    /// Previous owner
    pub prev_owner: Pubkey,
    /// New owner
    pub new_owner: Pubkey,
    /// Previous motes
    pub prev_motes: u64,
    /// New motes
    pub new_motes: u64,
    /// Previous data hash
    pub prev_data_hash: [u8; 32],
    /// New data hash
    pub new_data_hash: [u8; 32],
    /// Data was reallocated
    pub reallocated: bool,
    /// Previous size
    pub prev_size: usize,
    /// New size
    pub new_size: usize,
}

impl AccountDelta {
    /// Create delta from before/after state
    pub fn compute(
        pubkey: Pubkey,
        prev_owner: Pubkey,
        new_owner: Pubkey,
        prev_motes: u64,
        new_motes: u64,
        prev_data: &[u8],
        new_data: &[u8],
    ) -> Self {
        Self {
            pubkey,
            prev_owner,
            new_owner,
            prev_motes,
            new_motes,
            prev_data_hash: blake3::hash(prev_data).into(),
            new_data_hash: blake3::hash(new_data).into(),
            reallocated: prev_data.len() != new_data.len(),
            prev_size: prev_data.len(),
            new_size: new_data.len(),
        }
    }

    /// Whether account was modified
    pub fn is_modified(&self) -> bool {
        self.prev_owner != self.new_owner
            || self.prev_motes != self.new_motes
            || self.prev_data_hash != self.new_data_hash
    }
}

/// Execution receipt - canonical proof of execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    /// Transaction hash
    pub transaction_hash: [u8; 32],
    /// Slot when executed
    pub slot: u64,
    /// Success or failure
    pub success: bool,
    /// Error code if failed
    pub error_code: Option<u32>,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Compute units consumed
    pub compute_units_consumed: u64,
    /// Fee paid
    pub fee_paid: u64,
    /// Events emitted
    pub events: Vec<ProgramEvent>,
    /// Log messages
    pub logs: Vec<LogEntry>,
    /// Account modifications
    pub account_deltas: Vec<AccountDelta>,
    /// Return data
    pub return_data: Option<ReturnData>,
    /// Programs invoked
    pub programs_invoked: Vec<Pubkey>,
    /// Receipt hash (for verification)
    pub receipt_hash: [u8; 32],
}

impl ExecutionReceipt {
    /// Create successful receipt
    pub fn success(
        transaction_hash: [u8; 32],
        slot: u64,
        compute_units_consumed: u64,
        fee_paid: u64,
        events: Vec<ProgramEvent>,
        logs: Vec<LogEntry>,
        account_deltas: Vec<AccountDelta>,
        return_data: Option<ReturnData>,
        programs_invoked: Vec<Pubkey>,
    ) -> Self {
        let mut receipt = Self {
            transaction_hash,
            slot,
            success: true,
            error_code: None,
            error_message: None,
            compute_units_consumed,
            fee_paid,
            events,
            logs,
            account_deltas,
            return_data,
            programs_invoked,
            receipt_hash: [0u8; 32],
        };
        receipt.receipt_hash = receipt.compute_hash();
        receipt
    }

    /// Create failed receipt
    pub fn failure(
        transaction_hash: [u8; 32],
        slot: u64,
        error: &ProgramError,
        compute_units_consumed: u64,
        fee_paid: u64,
        logs: Vec<LogEntry>,
    ) -> Self {
        let mut receipt = Self {
            transaction_hash,
            slot,
            success: false,
            error_code: Some(error.to_code()),
            error_message: Some(error.to_string()),
            compute_units_consumed,
            fee_paid,
            events: Vec::new(),
            logs,
            account_deltas: Vec::new(),
            return_data: None,
            programs_invoked: Vec::new(),
            receipt_hash: [0u8; 32],
        };
        receipt.receipt_hash = receipt.compute_hash();
        receipt
    }

    /// Compute receipt hash
    pub fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.transaction_hash);
        hasher.update(&self.slot.to_le_bytes());
        hasher.update(&[self.success as u8]);
        if let Some(code) = self.error_code {
            hasher.update(&code.to_le_bytes());
        }
        hasher.update(&self.compute_units_consumed.to_le_bytes());
        hasher.update(&self.fee_paid.to_le_bytes());

        // Hash events
        for event in &self.events {
            hasher.update(&event.id.0);
        }

        // Hash account deltas
        for delta in &self.account_deltas {
            hasher.update(&delta.pubkey.0);
            hasher.update(&delta.new_data_hash);
        }

        hasher.finalize().into()
    }

    /// Verify receipt hash
    pub fn verify(&self) -> bool {
        self.receipt_hash == self.compute_hash()
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }
}

/// Return data from program execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnData {
    /// Program that set the return data
    pub program_id: Pubkey,
    /// The return data
    pub data: Vec<u8>,
}

impl ReturnData {
    /// Maximum return data size
    pub const MAX_SIZE: usize = 1024;

    /// Create new return data
    pub fn new(program_id: Pubkey, data: Vec<u8>) -> Option<Self> {
        if data.len() > Self::MAX_SIZE {
            None
        } else {
            Some(Self { program_id, data })
        }
    }
}

/// Event collector during execution
pub struct EventCollector {
    /// Transaction hash
    transaction_hash: [u8; 32],
    /// Current slot
    slot: u64,
    /// Collected events
    events: Vec<ProgramEvent>,
    /// Log entries
    logs: Vec<LogEntry>,
    /// Current event index
    event_index: u32,
    /// Maximum events per transaction
    max_events: u32,
    /// Maximum log bytes
    max_log_bytes: usize,
    /// Current log bytes
    current_log_bytes: usize,
}

/// Event/log checkpoint used to rollback collector state on CPI failure
#[derive(Debug, Clone, Copy)]
pub struct EventCollectorCheckpoint {
    events_len: usize,
    logs_len: usize,
    event_index: u32,
    current_log_bytes: usize,
}

impl EventCollector {
    /// Create new collector
    pub fn new(transaction_hash: [u8; 32], slot: u64) -> Self {
        Self {
            transaction_hash,
            slot,
            events: Vec::new(),
            logs: Vec::new(),
            event_index: 0,
            max_events: 256,
            max_log_bytes: 10 * 1024, // 10KB
            current_log_bytes: 0,
        }
    }

    /// Emit an event
    pub fn emit_event(
        &mut self,
        program_id: Pubkey,
        discriminator: [u8; 8],
        data: Vec<u8>,
        cpi_depth: u8,
    ) -> Result<EventId, &'static str> {
        if self.events.len() >= self.max_events as usize {
            return Err("too many events");
        }

        let event = ProgramEvent::new(
            &self.transaction_hash,
            program_id,
            discriminator,
            data,
            self.slot,
            self.event_index,
            cpi_depth,
        );

        let id = event.id;
        self.events.push(event);
        self.event_index += 1;

        Ok(id)
    }

    /// Add log entry
    pub fn log(
        &mut self,
        program_id: Pubkey,
        message: String,
        depth: u8,
    ) -> Result<(), &'static str> {
        let log_size = message.len();

        if self.current_log_bytes + log_size > self.max_log_bytes {
            return Err("log buffer full");
        }

        self.logs.push(LogEntry {
            message,
            program_id,
            depth,
        });

        self.current_log_bytes += log_size;
        Ok(())
    }

    /// Consume collected events and logs
    pub fn consume(self) -> (Vec<ProgramEvent>, Vec<LogEntry>) {
        (self.events, self.logs)
    }

    /// Create a checkpoint of current collector state.
    ///
    /// Used by CPI to ensure events/logs produced by a failing callee are discarded.
    pub fn checkpoint(&self) -> EventCollectorCheckpoint {
        EventCollectorCheckpoint {
            events_len: self.events.len(),
            logs_len: self.logs.len(),
            event_index: self.event_index,
            current_log_bytes: self.current_log_bytes,
        }
    }

    /// Roll back collector state to a previous checkpoint.
    ///
    /// This truncates event/log buffers and restores internal counters so subsequent
    /// emissions remain deterministic.
    pub fn rollback_to(&mut self, checkpoint: EventCollectorCheckpoint) {
        self.events.truncate(checkpoint.events_len);
        self.logs.truncate(checkpoint.logs_len);
        self.event_index = checkpoint.event_index;
        self.current_log_bytes = checkpoint.current_log_bytes;
    }

    /// Get event count
    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    /// Get log count
    pub fn log_count(&self) -> usize {
        self.logs.len()
    }
}

/// Event filter for querying events
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Filter by program ID
    pub program_id: Option<Pubkey>,
    /// Filter by discriminator
    pub discriminator: Option<[u8; 8]>,
    /// Filter by slot range
    pub slot_range: Option<(u64, u64)>,
    /// Filter by transaction hash
    pub transaction_hash: Option<[u8; 32]>,
    /// Limit results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl EventFilter {
    /// Create filter for a program
    pub fn for_program(program_id: Pubkey) -> Self {
        Self {
            program_id: Some(program_id),
            ..Default::default()
        }
    }

    /// Add discriminator filter
    pub fn with_discriminator(mut self, disc: [u8; 8]) -> Self {
        self.discriminator = Some(disc);
        self
    }

    /// Add slot range filter
    pub fn with_slot_range(mut self, start: u64, end: u64) -> Self {
        self.slot_range = Some((start, end));
        self
    }

    /// Add limit
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Check if event matches filter
    pub fn matches(&self, event: &ProgramEvent) -> bool {
        if let Some(pid) = &self.program_id {
            if event.program_id != *pid {
                return false;
            }
        }

        if let Some(disc) = &self.discriminator {
            if event.discriminator != *disc {
                return false;
            }
        }

        if let Some((start, end)) = &self.slot_range {
            if event.slot < *start || event.slot > *end {
                return false;
            }
        }

        true
    }
}

/// Event subscription for streaming
#[derive(Debug, Clone)]
pub struct EventSubscription {
    /// Subscription ID
    pub id: u64,
    /// Filter for events
    pub filter: EventFilter,
    /// Callback channel (represented as subscription info)
    pub created_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_id_generation() {
        let tx_hash = [1u8; 32];
        let program_id = Pubkey::new([2u8; 32]);

        let id1 = EventId::generate(&tx_hash, &program_id, 0);
        let id2 = EventId::generate(&tx_hash, &program_id, 0);
        let id3 = EventId::generate(&tx_hash, &program_id, 1);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_event_discriminator() {
        let disc1 = ProgramEvent::compute_discriminator("Transfer");
        let disc2 = ProgramEvent::compute_discriminator("Transfer");
        let disc3 = ProgramEvent::compute_discriminator("Mint");

        assert_eq!(disc1, disc2);
        assert_ne!(disc1, disc3);
    }

    #[test]
    fn test_execution_receipt() {
        let tx_hash = [1u8; 32];
        let receipt = ExecutionReceipt::success(
            tx_hash,
            100,
            50000,
            5000,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
        );

        assert!(receipt.success);
        assert!(receipt.verify());
    }

    #[test]
    fn test_execution_receipt_failure() {
        let tx_hash = [1u8; 32];
        let error = ProgramError::InsufficientFunds;
        let receipt = ExecutionReceipt::failure(tx_hash, 100, &error, 10000, 5000, Vec::new());

        assert!(!receipt.success);
        assert!(receipt.error_code.is_some());
        assert!(receipt.verify());
    }

    #[test]
    fn test_event_collector() {
        let tx_hash = [1u8; 32];
        let mut collector = EventCollector::new(tx_hash, 100);

        let program_id = Pubkey::new([2u8; 32]);
        let disc = ProgramEvent::compute_discriminator("Test");

        let id = collector
            .emit_event(program_id, disc, vec![1, 2, 3], 0)
            .unwrap();
        assert_ne!(id, EventId::zero());

        collector
            .log(program_id, "Test log".to_string(), 0)
            .unwrap();

        let (events, logs) = collector.consume();
        assert_eq!(events.len(), 1);
        assert_eq!(logs.len(), 1);
    }

    #[test]
    fn test_account_delta() {
        let pubkey = Pubkey::new([1u8; 32]);
        let owner = Pubkey::new([2u8; 32]);

        let prev_data = vec![1, 2, 3];
        let new_data = vec![1, 2, 3, 4, 5];

        let delta = AccountDelta::compute(pubkey, owner, owner, 1000, 900, &prev_data, &new_data);

        assert!(delta.is_modified());
        assert!(delta.reallocated);
        assert_eq!(delta.prev_size, 3);
        assert_eq!(delta.new_size, 5);
    }

    #[test]
    fn test_event_filter() {
        let program_id = Pubkey::new([1u8; 32]);
        let disc = [1u8; 8];

        let event = ProgramEvent {
            id: EventId::zero(),
            program_id,
            discriminator: disc,
            data: Vec::new(),
            slot: 100,
            index: 0,
            is_cpi: false,
            cpi_depth: 0,
        };

        let filter = EventFilter::for_program(program_id)
            .with_discriminator(disc)
            .with_slot_range(50, 150);

        assert!(filter.matches(&event));

        let filter2 = EventFilter::for_program(Pubkey::new([2u8; 32]));
        assert!(!filter2.matches(&event));
    }

    #[test]
    fn test_return_data() {
        let program_id = Pubkey::new([1u8; 32]);

        let data = vec![1u8; 100];
        let return_data = ReturnData::new(program_id, data.clone());
        assert!(return_data.is_some());

        let too_large = vec![1u8; ReturnData::MAX_SIZE + 1];
        let return_data = ReturnData::new(program_id, too_large);
        assert!(return_data.is_none());
    }
}
