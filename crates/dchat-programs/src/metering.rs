//! Metering system for compute units, memory, and storage costs

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{ProgramError, ProgramResult};

/// Compute unit costs for various operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeUnitCosts {
    /// Base cost per instruction
    pub instruction_base: u64,
    /// Cost per byte of instruction data
    pub instruction_data_byte: u64,
    /// Cost per account in instruction
    pub instruction_account: u64,
    /// Cost for a memory operation (load/store)
    pub memory_op: u64,
    /// Cost per byte of memory allocation
    pub memory_alloc_byte: u64,
    /// Cost for a branch/jump
    pub branch: u64,
    /// Cost for a function call
    pub call: u64,
    /// Cost for arithmetic operations
    pub arithmetic: u64,
    /// Cost for comparison operations
    pub comparison: u64,
    /// Cost for a syscall invocation
    pub syscall_base: u64,
}

impl Default for ComputeUnitCosts {
    fn default() -> Self {
        Self {
            instruction_base: 100,
            instruction_data_byte: 1,
            instruction_account: 50,
            memory_op: 1,
            memory_alloc_byte: 1,
            branch: 1,
            call: 10,
            arithmetic: 1,
            comparison: 1,
            syscall_base: 100,
        }
    }
}

/// Cryptographic operation costs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoOpCosts {
    /// SHA256 hash per 64 bytes
    pub sha256_base: u64,
    pub sha256_per_byte: u64,
    /// BLAKE3 hash per 64 bytes
    pub blake3_base: u64,
    pub blake3_per_byte: u64,
    /// Ed25519 signature verification
    pub ed25519_verify: u64,
    /// Secp256k1 signature recovery
    pub secp256k1_recover: u64,
    /// Curve25519 point multiplication
    pub curve25519_mul: u64,
    /// Poseidon hash (for ZK proofs)
    pub poseidon_base: u64,
    pub poseidon_per_input: u64,
    /// Keccak256 hash
    pub keccak256_base: u64,
    pub keccak256_per_byte: u64,
}

impl Default for CryptoOpCosts {
    fn default() -> Self {
        Self {
            sha256_base: 100,
            sha256_per_byte: 1,
            blake3_base: 80,
            blake3_per_byte: 1,
            ed25519_verify: 2000,
            secp256k1_recover: 3000,
            curve25519_mul: 2500,
            poseidon_base: 500,
            poseidon_per_input: 100,
            keccak256_base: 100,
            keccak256_per_byte: 1,
        }
    }
}

/// Storage operation costs (in lamports)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCosts {
    /// Cost per byte of storage read
    pub read_per_byte: u64,
    /// Cost per byte of storage write
    pub write_per_byte: u64,
    /// Base cost for account creation
    pub account_creation_base: u64,
    /// Cost per byte of account data
    pub account_data_per_byte: u64,
    /// Rent per byte per epoch
    pub rent_per_byte_epoch: u64,
    /// Rent exemption multiplier (years)
    pub rent_exemption_threshold: f64,
}

impl Default for StorageCosts {
    fn default() -> Self {
        Self {
            read_per_byte: 1,
            write_per_byte: 10,
            account_creation_base: 1_000_000,
            account_data_per_byte: 10_000,
            rent_per_byte_epoch: 1,
            rent_exemption_threshold: 2.0,
        }
    }
}

/// Memory costs and limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCosts {
    /// Maximum heap size in bytes
    pub max_heap_bytes: u64,
    /// Maximum stack size in bytes
    pub max_stack_bytes: u64,
    /// Cost per byte of heap allocation
    pub heap_alloc_cost: u64,
    /// Cost for stack push
    pub stack_push_cost: u64,
}

impl Default for MemoryCosts {
    fn default() -> Self {
        Self {
            max_heap_bytes: 32 * 1024 * 1024, // 32 MB
            max_stack_bytes: 64 * 1024,       // 64 KB
            heap_alloc_cost: 1,
            stack_push_cost: 1,
        }
    }
}

/// Complete compute budget for a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeBudget {
    /// Maximum compute units allowed
    pub max_units: u64,
    /// Heap size limit
    pub heap_size: u64,
    /// Stack size limit
    pub stack_size: u64,
    /// Cost configuration
    pub unit_costs: ComputeUnitCosts,
    /// Crypto costs
    pub crypto_costs: CryptoOpCosts,
    /// Storage costs
    pub storage_costs: StorageCosts,
    /// Memory costs
    pub memory_costs: MemoryCosts,
}

impl Default for ComputeBudget {
    fn default() -> Self {
        Self {
            max_units: crate::DEFAULT_COMPUTE_UNITS,
            heap_size: 32 * 1024, // 32 KB default
            stack_size: 4 * 1024, // 4 KB default
            unit_costs: ComputeUnitCosts::default(),
            crypto_costs: CryptoOpCosts::default(),
            storage_costs: StorageCosts::default(),
            memory_costs: MemoryCosts::default(),
        }
    }
}

impl ComputeBudget {
    /// Create a new budget with custom max units
    pub fn new(max_units: u64) -> Self {
        Self {
            max_units,
            ..Default::default()
        }
    }

    /// Create maximum budget
    pub fn max() -> Self {
        Self {
            max_units: crate::MAX_COMPUTE_UNITS,
            heap_size: 256 * 1024, // 256 KB max
            stack_size: 64 * 1024, // 64 KB max
            ..Default::default()
        }
    }

    /// Calculate fee for this budget (in lamports)
    pub fn compute_fee(&self, lamports_per_cu: u64) -> u64 {
        self.max_units.saturating_mul(lamports_per_cu)
    }
}

/// Compute meter for tracking execution costs
#[derive(Debug)]
pub struct ComputeMeter {
    /// Remaining compute units
    remaining: AtomicU64,
    /// Total consumed units
    consumed: AtomicU64,
    /// Budget configuration
    budget: ComputeBudget,
    /// Current heap usage
    heap_used: AtomicU64,
    /// Current stack depth
    stack_depth: AtomicU64,
}

impl ComputeMeter {
    /// Create a new compute meter with budget
    pub fn new(budget: ComputeBudget) -> Self {
        Self {
            remaining: AtomicU64::new(budget.max_units),
            consumed: AtomicU64::new(0),
            budget,
            heap_used: AtomicU64::new(0),
            stack_depth: AtomicU64::new(0),
        }
    }

    /// Get remaining compute units
    pub fn remaining(&self) -> u64 {
        self.remaining.load(Ordering::SeqCst)
    }

    /// Get consumed compute units
    pub fn consumed(&self) -> u64 {
        self.consumed.load(Ordering::SeqCst)
    }

    /// Consume compute units
    pub fn consume(&self, units: u64) -> ProgramResult<()> {
        loop {
            let current = self.remaining.load(Ordering::SeqCst);
            if current < units {
                return Err(ProgramError::ComputeBudgetExceeded);
            }
            if self
                .remaining
                .compare_exchange_weak(current, current - units, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                self.consumed.fetch_add(units, Ordering::SeqCst);
                return Ok(());
            }
        }
    }

    /// Consume units for instruction base cost
    pub fn consume_instruction(&self, data_len: usize, account_count: usize) -> ProgramResult<()> {
        let cost = self.budget.unit_costs.instruction_base
            + (data_len as u64 * self.budget.unit_costs.instruction_data_byte)
            + (account_count as u64 * self.budget.unit_costs.instruction_account);
        self.consume(cost)
    }

    /// Consume units for memory operation
    pub fn consume_memory_op(&self) -> ProgramResult<()> {
        self.consume(self.budget.unit_costs.memory_op)
    }

    /// Consume units for function call
    pub fn consume_call(&self) -> ProgramResult<()> {
        self.consume(self.budget.unit_costs.call)
    }

    /// Consume units for syscall
    pub fn consume_syscall(&self) -> ProgramResult<()> {
        self.consume(self.budget.unit_costs.syscall_base)
    }

    /// Consume units for SHA256 hash
    pub fn consume_sha256(&self, data_len: usize) -> ProgramResult<()> {
        let cost = self.budget.crypto_costs.sha256_base
            + (data_len as u64 * self.budget.crypto_costs.sha256_per_byte);
        self.consume(cost)
    }

    /// Consume units for BLAKE3 hash
    pub fn consume_blake3(&self, data_len: usize) -> ProgramResult<()> {
        let cost = self.budget.crypto_costs.blake3_base
            + (data_len as u64 * self.budget.crypto_costs.blake3_per_byte);
        self.consume(cost)
    }

    /// Consume units for Ed25519 signature verification
    pub fn consume_ed25519_verify(&self) -> ProgramResult<()> {
        self.consume(self.budget.crypto_costs.ed25519_verify)
    }

    /// Allocate heap memory
    pub fn allocate_heap(&self, bytes: u64) -> ProgramResult<()> {
        loop {
            let current = self.heap_used.load(Ordering::SeqCst);
            let new_usage = current.saturating_add(bytes);
            if new_usage > self.budget.heap_size {
                return Err(ProgramError::MemoryLimitExceeded);
            }
            if self
                .heap_used
                .compare_exchange_weak(current, new_usage, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // Also consume compute units for allocation
                let cost = bytes * self.budget.memory_costs.heap_alloc_cost;
                return self.consume(cost);
            }
        }
    }

    /// Free heap memory
    pub fn free_heap(&self, bytes: u64) {
        self.heap_used.fetch_sub(bytes, Ordering::SeqCst);
    }

    /// Get current heap usage
    pub fn heap_used(&self) -> u64 {
        self.heap_used.load(Ordering::SeqCst)
    }

    /// Push stack frame
    pub fn push_stack(&self) -> ProgramResult<()> {
        let current = self.stack_depth.fetch_add(1, Ordering::SeqCst);
        if current >= crate::MAX_STACK_DEPTH as u64 {
            self.stack_depth.fetch_sub(1, Ordering::SeqCst);
            return Err(ProgramError::StackOverflow);
        }
        self.consume(self.budget.memory_costs.stack_push_cost)
    }

    /// Pop stack frame
    pub fn pop_stack(&self) {
        self.stack_depth.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get current stack depth
    pub fn stack_depth(&self) -> u64 {
        self.stack_depth.load(Ordering::SeqCst)
    }

    /// Get the budget
    pub fn budget(&self) -> &ComputeBudget {
        &self.budget
    }

    /// Create a snapshot for CPI
    pub fn snapshot(&self) -> MeterSnapshot {
        MeterSnapshot {
            remaining: self.remaining.load(Ordering::SeqCst),
            consumed: self.consumed.load(Ordering::SeqCst),
            heap_used: self.heap_used.load(Ordering::SeqCst),
            stack_depth: self.stack_depth.load(Ordering::SeqCst),
        }
    }

    /// Restore from snapshot (for rollback on CPI failure)
    pub fn restore(&self, snapshot: &MeterSnapshot) {
        self.remaining.store(snapshot.remaining, Ordering::SeqCst);
        self.consumed.store(snapshot.consumed, Ordering::SeqCst);
        self.heap_used.store(snapshot.heap_used, Ordering::SeqCst);
        self.stack_depth
            .store(snapshot.stack_depth, Ordering::SeqCst);
    }
}

/// Snapshot of meter state for rollback
#[derive(Debug, Clone, Copy)]
pub struct MeterSnapshot {
    /// Remaining units
    pub remaining: u64,
    /// Consumed units
    pub consumed: u64,
    /// Heap used
    pub heap_used: u64,
    /// Stack depth
    pub stack_depth: u64,
}

/// Thread-safe shared compute meter for CPI
pub type SharedComputeMeter = Arc<ComputeMeter>;

/// Fee reservation for upfront payment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeReservation {
    /// Payer public key
    pub payer: crate::account::Pubkey,
    /// Reserved compute units
    pub compute_units: u64,
    /// Reserved storage bytes
    pub storage_bytes: u64,
    /// Total reserved lamports
    pub lamports_reserved: u64,
    /// Reservation timestamp
    pub reserved_at: u64,
    /// Expiry slot
    pub expires_at_slot: u64,
}

impl FeeReservation {
    /// Create a new fee reservation
    pub fn new(
        payer: crate::account::Pubkey,
        compute_units: u64,
        storage_bytes: u64,
        lamports_per_cu: u64,
        storage_lamports_per_byte: u64,
        current_slot: u64,
        ttl_slots: u64,
    ) -> Self {
        let compute_lamports = compute_units.saturating_mul(lamports_per_cu);
        let storage_lamports = storage_bytes.saturating_mul(storage_lamports_per_byte);

        Self {
            payer,
            compute_units,
            storage_bytes,
            lamports_reserved: compute_lamports.saturating_add(storage_lamports),
            reserved_at: current_slot,
            expires_at_slot: current_slot.saturating_add(ttl_slots),
        }
    }

    /// Check if reservation is expired
    pub fn is_expired(&self, current_slot: u64) -> bool {
        current_slot > self.expires_at_slot
    }

    /// Calculate refund for unused compute units
    pub fn calculate_refund(&self, consumed_units: u64, lamports_per_cu: u64) -> u64 {
        let unused = self.compute_units.saturating_sub(consumed_units);
        unused.saturating_mul(lamports_per_cu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_meter_consumption() {
        let budget = ComputeBudget::new(1000);
        let meter = ComputeMeter::new(budget);

        assert_eq!(meter.remaining(), 1000);
        assert_eq!(meter.consumed(), 0);

        meter.consume(100).unwrap();
        assert_eq!(meter.remaining(), 900);
        assert_eq!(meter.consumed(), 100);

        meter.consume(900).unwrap();
        assert_eq!(meter.remaining(), 0);
        assert_eq!(meter.consumed(), 1000);

        // Should fail - no remaining units
        assert!(meter.consume(1).is_err());
    }

    #[test]
    fn test_compute_meter_heap() {
        let mut budget = ComputeBudget::default();
        budget.heap_size = 1000;
        let meter = ComputeMeter::new(budget);

        meter.allocate_heap(500).unwrap();
        assert_eq!(meter.heap_used(), 500);

        meter.allocate_heap(500).unwrap();
        assert_eq!(meter.heap_used(), 1000);

        // Should fail - exceeds limit
        assert!(meter.allocate_heap(1).is_err());

        meter.free_heap(500);
        assert_eq!(meter.heap_used(), 500);
    }

    #[test]
    fn test_compute_meter_stack() {
        let budget = ComputeBudget::new(10000);
        let meter = ComputeMeter::new(budget);

        for _ in 0..10 {
            meter.push_stack().unwrap();
        }
        assert_eq!(meter.stack_depth(), 10);

        for _ in 0..5 {
            meter.pop_stack();
        }
        assert_eq!(meter.stack_depth(), 5);
    }

    #[test]
    fn test_meter_snapshot_restore() {
        let budget = ComputeBudget::new(1000);
        let meter = ComputeMeter::new(budget);

        meter.consume(100).unwrap();
        let snapshot = meter.snapshot();

        meter.consume(200).unwrap();
        assert_eq!(meter.consumed(), 300);

        meter.restore(&snapshot);
        assert_eq!(meter.consumed(), 100);
        assert_eq!(meter.remaining(), 900);
    }

    #[test]
    fn test_fee_reservation() {
        let payer = crate::account::Pubkey::new([1u8; 32]);
        let reservation = FeeReservation::new(
            payer, 1000, // compute units
            100,  // storage bytes
            10,   // lamports per CU
            100,  // lamports per storage byte
            100,  // current slot
            50,   // TTL slots
        );

        assert_eq!(reservation.lamports_reserved, 10000 + 10000); // 1000*10 + 100*100
        assert!(!reservation.is_expired(100));
        assert!(!reservation.is_expired(150));
        assert!(reservation.is_expired(151));

        let refund = reservation.calculate_refund(800, 10);
        assert_eq!(refund, 2000); // (1000-800) * 10
    }
}
