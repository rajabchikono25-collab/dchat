//! Syscall registry and handlers for program-runtime interface

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::account::{AccountInfo, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::events::EventCollector;
use crate::metering::ComputeMeter;

/// Syscall execution result with serializable data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallOutput {
    /// Whether syscall succeeded
    pub success: bool,
    /// Output data (if any)
    pub data: Vec<u8>,
    /// Compute units consumed
    pub compute_consumed: u64,
}

impl SyscallOutput {
    /// Create successful output
    pub fn success(data: Vec<u8>, compute: u64) -> Self {
        Self {
            success: true,
            data,
            compute_consumed: compute,
        }
    }

    /// Create failed output
    pub fn failure(compute: u64) -> Self {
        Self {
            success: false,
            data: Vec::new(),
            compute_consumed: compute,
        }
    }
}

/// Validate syscall arguments and return detailed result
pub fn validate_syscall_args(syscall: SyscallId, args: &[u64]) -> ProgramResult<()> {
    let expected_args = syscall.expected_arg_count();
    if args.len() != expected_args {
        return Err(ProgramError::InvalidArgument);
    }
    Ok(())
}

/// Syscall identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyscallId(pub u32);

impl SyscallId {
    // Logging syscalls
    /// Log a message to program output
    pub const SOL_LOG: Self = Self(0);
    /// Log 64-bit integers to program output
    pub const SOL_LOG_64: Self = Self(1);
    /// Log a public key to program output
    pub const SOL_LOG_PUBKEY: Self = Self(2);
    /// Log arbitrary data to program output
    pub const SOL_LOG_DATA: Self = Self(3);
    /// Log remaining compute units
    pub const SOL_LOG_COMPUTE_UNITS: Self = Self(4);

    // Crypto syscalls
    /// SHA-256 hash computation
    pub const SOL_SHA256: Self = Self(10);
    /// BLAKE3 hash computation
    pub const SOL_BLAKE3: Self = Self(11);
    /// Keccak-256 hash computation
    pub const SOL_KECCAK256: Self = Self(12);
    /// Secp256k1 signature recovery
    pub const SOL_SECP256K1_RECOVER: Self = Self(13);
    /// Ed25519 signature verification
    pub const SOL_ED25519_VERIFY: Self = Self(14);
    /// Poseidon hash for ZK circuits
    pub const SOL_POSEIDON: Self = Self(15);

    // Memory syscalls
    /// Copy non-overlapping memory
    pub const SOL_MEMCPY: Self = Self(20);
    /// Fill memory with value
    pub const SOL_MEMSET: Self = Self(21);
    /// Move possibly overlapping memory
    pub const SOL_MEMMOVE: Self = Self(22);
    /// Compare memory regions
    pub const SOL_MEMCMP: Self = Self(23);

    // Account syscalls
    /// Create program-derived address
    pub const SOL_CREATE_PROGRAM_ADDRESS: Self = Self(30);
    /// Find program-derived address with bump seed
    pub const SOL_TRY_FIND_PROGRAM_ADDRESS: Self = Self(31);
    /// Get clock sysvar
    pub const SOL_GET_CLOCK_SYSVAR: Self = Self(32);
    /// Get rent sysvar
    pub const SOL_GET_RENT_SYSVAR: Self = Self(33);
    /// Get epoch schedule sysvar
    pub const SOL_GET_EPOCH_SCHEDULE_SYSVAR: Self = Self(34);

    // CPI syscalls
    /// Cross-program invocation with signed seeds
    pub const SOL_INVOKE_SIGNED: Self = Self(40);
    /// Set return data for caller
    pub const SOL_SET_RETURN_DATA: Self = Self(41);
    /// Get return data from callee
    pub const SOL_GET_RETURN_DATA: Self = Self(42);

    // Program syscalls
    /// Get previously processed sibling instruction
    pub const SOL_GET_PROCESSED_SIBLING_INSTRUCTION: Self = Self(50);
    /// Get current CPI stack height
    pub const SOL_GET_STACK_HEIGHT: Self = Self(51);

    // Allocator
    /// Heap allocation and deallocation
    pub const SOL_ALLOC_FREE: Self = Self(60);

    /// Get expected argument count for a syscall
    pub fn expected_arg_count(&self) -> usize {
        match self.0 {
            0..=4 => 2,   // Logging syscalls: ptr, len
            10..=15 => 3, // Crypto syscalls: input ptr, input len, output ptr
            20..=23 => 3, // Memory syscalls: dest, src/val, len
            30..=31 => 4, // PDA syscalls: seeds ptr, seeds len, program_id, bump
            32..=34 => 1, // Sysvar syscalls: output ptr
            40 => 5, // Invoke signed: instruction ptr, accounts ptr, accounts len, seeds ptr, seeds len
            41..=42 => 2, // Return data: ptr, len
            50..=51 => 1, // Stack inspection: output ptr
            60 => 2, // Alloc/free: size, ptr
            _ => 0,
        }
    }
}

/// Syscall cost in compute units
#[derive(Debug, Clone, Copy)]
pub struct SyscallCost {
    /// Base cost
    pub base: u64,
    /// Cost per byte of input
    pub per_byte: u64,
    /// Cost per iteration (for variable work)
    pub per_iteration: u64,
}

impl SyscallCost {
    /// Create new syscall cost
    pub const fn new(base: u64, per_byte: u64, per_iteration: u64) -> Self {
        Self {
            base,
            per_byte,
            per_iteration,
        }
    }

    /// Calculate total cost
    pub fn total(&self, bytes: usize, iterations: usize) -> u64 {
        self.base + (self.per_byte * bytes as u64) + (self.per_iteration * iterations as u64)
    }
}

/// Default syscall costs
pub mod costs {
    use super::SyscallCost;

    /// Cost for logging operations
    pub const LOG: SyscallCost = SyscallCost::new(100, 1, 0);
    /// Cost for SHA-256 hash
    pub const SHA256: SyscallCost = SyscallCost::new(85, 1, 0);
    /// Cost for BLAKE3 hash
    pub const BLAKE3: SyscallCost = SyscallCost::new(100, 1, 0);
    /// Cost for Keccak-256 hash
    pub const KECCAK256: SyscallCost = SyscallCost::new(85, 1, 0);
    /// Cost for secp256k1 key recovery
    pub const SECP256K1_RECOVER: SyscallCost = SyscallCost::new(25000, 0, 0);
    /// Cost for Ed25519 signature verification
    pub const ED25519_VERIFY: SyscallCost = SyscallCost::new(3000, 0, 0);
    /// Cost for Poseidon hash (ZK-friendly)
    pub const POSEIDON: SyscallCost = SyscallCost::new(2000, 0, 100);
    /// Cost for memory copy
    pub const MEMCPY: SyscallCost = SyscallCost::new(3, 0, 0);
    /// Cost for memory set
    pub const MEMSET: SyscallCost = SyscallCost::new(3, 0, 0);
    /// Cost for memory move
    pub const MEMMOVE: SyscallCost = SyscallCost::new(3, 0, 0);
    /// Cost for memory compare
    pub const MEMCMP: SyscallCost = SyscallCost::new(3, 0, 0);
    /// Cost for PDA creation
    pub const CREATE_PROGRAM_ADDRESS: SyscallCost = SyscallCost::new(1500, 0, 0);
    /// Cost for PDA discovery
    pub const TRY_FIND_PROGRAM_ADDRESS: SyscallCost = SyscallCost::new(1500, 0, 1500);
    /// Cost for sysvar access
    pub const GET_SYSVAR: SyscallCost = SyscallCost::new(100, 0, 0);
    /// Cost for cross-program invocation
    pub const INVOKE_SIGNED: SyscallCost = SyscallCost::new(1000, 0, 0);
    /// Cost for setting return data
    pub const SET_RETURN_DATA: SyscallCost = SyscallCost::new(20, 1, 0);
    /// Cost for getting return data
    pub const GET_RETURN_DATA: SyscallCost = SyscallCost::new(20, 1, 0);
    /// Cost for heap allocation
    pub const ALLOC: SyscallCost = SyscallCost::new(1, 0, 0);
}

/// Syscall context passed to handlers
pub struct SyscallContext<'a, 'b> {
    /// Current program ID
    pub program_id: Pubkey,
    /// Account infos
    pub accounts: &'a [AccountInfo<'b>],
    /// Compute meter
    pub meter: &'a ComputeMeter,
    /// Event collector
    pub events: &'a mut EventCollector,
    /// CPI depth
    pub depth: u8,
    /// Return data storage
    pub return_data: Option<(Pubkey, Vec<u8>)>,
    /// Instruction data
    pub instruction_data: &'a [u8],
}

/// Result from syscall execution
#[derive(Debug)]
pub enum SyscallResult {
    /// Success with return value
    Ok(u64),
    /// Success with bytes
    OkBytes(Vec<u8>),
    /// Error
    Err(ProgramError),
}

/// Syscall handler trait
pub trait SyscallHandler: Send + Sync {
    /// Execute the syscall
    fn execute(
        &self,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        memory: &mut [u8],
    ) -> SyscallResult;

    /// Get syscall cost
    fn cost(&self) -> SyscallCost;
}

/// Logging syscall handler
pub struct LogHandler;

impl SyscallHandler for LogHandler {
    fn execute(
        &self,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        memory: &mut [u8],
    ) -> SyscallResult {
        if args.len() < 2 {
            return SyscallResult::Err(ProgramError::InvalidArgument);
        }

        let ptr = args[0] as usize;
        let len = args[1] as usize;

        if ptr + len > memory.len() {
            return SyscallResult::Err(ProgramError::MemoryAccessViolation);
        }

        // Consume compute
        let cost = self.cost().total(len, 0);
        if ctx.meter.consume(cost).is_err() {
            return SyscallResult::Err(ProgramError::ComputationalBudgetExceeded);
        }

        let message = String::from_utf8_lossy(&memory[ptr..ptr + len]).to_string();
        if ctx.events.log(ctx.program_id, message, ctx.depth).is_err() {
            return SyscallResult::Err(ProgramError::LogBufferFull);
        }

        SyscallResult::Ok(0)
    }

    fn cost(&self) -> SyscallCost {
        costs::LOG
    }
}

/// SHA256 syscall handler
pub struct Sha256Handler;

impl SyscallHandler for Sha256Handler {
    fn execute(
        &self,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        memory: &mut [u8],
    ) -> SyscallResult {
        if args.len() < 3 {
            return SyscallResult::Err(ProgramError::InvalidArgument);
        }

        let input_ptr = args[0] as usize;
        let input_len = args[1] as usize;
        let output_ptr = args[2] as usize;

        if input_ptr + input_len > memory.len() || output_ptr + 32 > memory.len() {
            return SyscallResult::Err(ProgramError::MemoryAccessViolation);
        }

        // Consume compute
        let cost = self.cost().total(input_len, 0);
        if ctx.meter.consume(cost).is_err() {
            return SyscallResult::Err(ProgramError::ComputationalBudgetExceeded);
        }

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&memory[input_ptr..input_ptr + input_len]);
        let result = hasher.finalize();

        memory[output_ptr..output_ptr + 32].copy_from_slice(&result);

        SyscallResult::Ok(0)
    }

    fn cost(&self) -> SyscallCost {
        costs::SHA256
    }
}

/// BLAKE3 syscall handler
pub struct Blake3Handler;

impl SyscallHandler for Blake3Handler {
    fn execute(
        &self,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        memory: &mut [u8],
    ) -> SyscallResult {
        if args.len() < 3 {
            return SyscallResult::Err(ProgramError::InvalidArgument);
        }

        let input_ptr = args[0] as usize;
        let input_len = args[1] as usize;
        let output_ptr = args[2] as usize;

        if input_ptr + input_len > memory.len() || output_ptr + 32 > memory.len() {
            return SyscallResult::Err(ProgramError::MemoryAccessViolation);
        }

        let cost = self.cost().total(input_len, 0);
        if ctx.meter.consume(cost).is_err() {
            return SyscallResult::Err(ProgramError::ComputationalBudgetExceeded);
        }

        let hash = blake3::hash(&memory[input_ptr..input_ptr + input_len]);
        memory[output_ptr..output_ptr + 32].copy_from_slice(hash.as_bytes());

        SyscallResult::Ok(0)
    }

    fn cost(&self) -> SyscallCost {
        costs::BLAKE3
    }
}

/// Memory copy syscall handler
pub struct MemcpyHandler;

impl SyscallHandler for MemcpyHandler {
    fn execute(
        &self,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        memory: &mut [u8],
    ) -> SyscallResult {
        if args.len() < 3 {
            return SyscallResult::Err(ProgramError::InvalidArgument);
        }

        let dst = args[0] as usize;
        let src = args[1] as usize;
        let len = args[2] as usize;

        if dst + len > memory.len() || src + len > memory.len() {
            return SyscallResult::Err(ProgramError::MemoryAccessViolation);
        }

        // Check for overlap (use memmove instead)
        if dst < src + len && src < dst + len {
            return SyscallResult::Err(ProgramError::MemoryOverlap);
        }

        let cost = self.cost().total(len, 0);
        if ctx.meter.consume(cost).is_err() {
            return SyscallResult::Err(ProgramError::ComputationalBudgetExceeded);
        }

        // Safe non-overlapping copy - copy through intermediate buffer
        // to avoid borrow checker issues. The overlap check above ensures this is valid.
        let mut temp = vec![0u8; len];
        temp.copy_from_slice(&memory[src..src + len]);
        memory[dst..dst + len].copy_from_slice(&temp);

        SyscallResult::Ok(0)
    }

    fn cost(&self) -> SyscallCost {
        costs::MEMCPY
    }
}

/// Create program address syscall handler
pub struct CreateProgramAddressHandler;

impl SyscallHandler for CreateProgramAddressHandler {
    fn execute(
        &self,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        memory: &mut [u8],
    ) -> SyscallResult {
        if args.len() < 4 {
            return SyscallResult::Err(ProgramError::InvalidArgument);
        }

        let seeds_ptr = args[0] as usize;
        let seeds_len = args[1] as usize;
        let program_id_ptr = args[2] as usize;
        let output_ptr = args[3] as usize;

        let cost = self.cost().total(0, 0);
        if ctx.meter.consume(cost).is_err() {
            return SyscallResult::Err(ProgramError::ComputationalBudgetExceeded);
        }

        // Read program ID
        if program_id_ptr + 32 > memory.len() {
            return SyscallResult::Err(ProgramError::MemoryAccessViolation);
        }
        let mut program_id_bytes = [0u8; 32];
        program_id_bytes.copy_from_slice(&memory[program_id_ptr..program_id_ptr + 32]);
        let program_id = Pubkey::new(program_id_bytes);

        // Read seeds (simplified - assume seeds_ptr points to contiguous seed data)
        if seeds_ptr + seeds_len > memory.len() {
            return SyscallResult::Err(ProgramError::MemoryAccessViolation);
        }
        let seed_data = &memory[seeds_ptr..seeds_ptr + seeds_len];

        // Compute PDA
        let mut hasher = blake3::Hasher::new();
        hasher.update(seed_data);
        hasher.update(&program_id.0);
        hasher.update(b"ProgramDerivedAddress");
        let hash: [u8; 32] = hasher.finalize().into();

        // Check not on curve (simplified check)
        if crate::pda::PdaDerivation::is_on_curve(&hash) {
            return SyscallResult::Err(ProgramError::InvalidSeeds);
        }

        // Write output
        if output_ptr + 32 > memory.len() {
            return SyscallResult::Err(ProgramError::MemoryAccessViolation);
        }
        memory[output_ptr..output_ptr + 32].copy_from_slice(&hash);

        SyscallResult::Ok(0)
    }

    fn cost(&self) -> SyscallCost {
        costs::CREATE_PROGRAM_ADDRESS
    }
}

/// Set return data syscall handler
pub struct SetReturnDataHandler;

impl SyscallHandler for SetReturnDataHandler {
    fn execute(
        &self,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        memory: &mut [u8],
    ) -> SyscallResult {
        if args.len() < 2 {
            return SyscallResult::Err(ProgramError::InvalidArgument);
        }

        let ptr = args[0] as usize;
        let len = args[1] as usize;

        if len > 1024 {
            return SyscallResult::Err(ProgramError::ReturnDataTooLarge);
        }

        if ptr + len > memory.len() {
            return SyscallResult::Err(ProgramError::MemoryAccessViolation);
        }

        let cost = self.cost().total(len, 0);
        if ctx.meter.consume(cost).is_err() {
            return SyscallResult::Err(ProgramError::ComputationalBudgetExceeded);
        }

        let data = memory[ptr..ptr + len].to_vec();
        ctx.return_data = Some((ctx.program_id, data));

        SyscallResult::Ok(0)
    }

    fn cost(&self) -> SyscallCost {
        costs::SET_RETURN_DATA
    }
}

/// Allocator syscall handler
pub struct AllocHandler {
    /// Current heap position
    heap_pos: std::sync::atomic::AtomicUsize,
    /// Heap size
    heap_size: usize,
}

impl AllocHandler {
    /// Create new allocator
    pub fn new(heap_size: usize) -> Self {
        Self {
            heap_pos: std::sync::atomic::AtomicUsize::new(0),
            heap_size,
        }
    }

    /// Reset allocator
    pub fn reset(&self) {
        self.heap_pos.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

impl SyscallHandler for AllocHandler {
    fn execute(
        &self,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        _memory: &mut [u8],
    ) -> SyscallResult {
        if args.is_empty() {
            return SyscallResult::Err(ProgramError::InvalidArgument);
        }

        let size = args[0] as usize;

        // Align to 8 bytes
        let aligned_size = (size + 7) & !7;

        let cost = self.cost().total(aligned_size, 0);
        if ctx.meter.consume(cost).is_err() {
            return SyscallResult::Err(ProgramError::ComputationalBudgetExceeded);
        }

        let pos = self
            .heap_pos
            .fetch_add(aligned_size, std::sync::atomic::Ordering::SeqCst);

        if pos + aligned_size > self.heap_size {
            return SyscallResult::Err(ProgramError::HeapExhausted);
        }

        // Return heap offset
        SyscallResult::Ok(pos as u64)
    }

    fn cost(&self) -> SyscallCost {
        costs::ALLOC
    }
}

/// Syscall registry - holds all registered syscall handlers
#[derive(Clone)]
pub struct SyscallRegistry {
    /// Registered handlers
    handlers: HashMap<SyscallId, Arc<dyn SyscallHandler>>,
}

impl SyscallRegistry {
    /// Create new registry with default syscalls
    pub fn new() -> Self {
        let mut registry = Self {
            handlers: HashMap::new(),
        };
        registry.register_defaults();
        registry
    }

    /// Register default syscalls
    fn register_defaults(&mut self) {
        self.register(SyscallId::SOL_LOG, Arc::new(LogHandler));
        self.register(SyscallId::SOL_SHA256, Arc::new(Sha256Handler));
        self.register(SyscallId::SOL_BLAKE3, Arc::new(Blake3Handler));
        self.register(SyscallId::SOL_MEMCPY, Arc::new(MemcpyHandler));
        self.register(
            SyscallId::SOL_CREATE_PROGRAM_ADDRESS,
            Arc::new(CreateProgramAddressHandler),
        );
        self.register(
            SyscallId::SOL_SET_RETURN_DATA,
            Arc::new(SetReturnDataHandler),
        );
        self.register(
            SyscallId::SOL_ALLOC_FREE,
            Arc::new(AllocHandler::new(32 * 1024)),
        ); // 32KB heap
    }

    /// Register a syscall handler
    pub fn register(&mut self, id: SyscallId, handler: Arc<dyn SyscallHandler>) {
        self.handlers.insert(id, handler);
    }

    /// Get handler for syscall
    pub fn get(&self, id: SyscallId) -> Option<Arc<dyn SyscallHandler>> {
        self.handlers.get(&id).cloned()
    }

    /// Execute a syscall
    pub fn execute(
        &self,
        id: SyscallId,
        ctx: &mut SyscallContext<'_, '_>,
        args: &[u64],
        memory: &mut [u8],
    ) -> SyscallResult {
        match self.get(id) {
            Some(handler) => handler.execute(ctx, args, memory),
            None => SyscallResult::Err(ProgramError::SyscallNotFound),
        }
    }

    /// Get syscall cost
    pub fn get_cost(&self, id: SyscallId) -> Option<SyscallCost> {
        self.handlers.get(&id).map(|h| h.cost())
    }
}

impl Default for SyscallRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context<'a, 'b>(
        meter: &'a ComputeMeter,
        events: &'a mut EventCollector,
    ) -> SyscallContext<'a, 'b> {
        SyscallContext {
            program_id: Pubkey::new([1u8; 32]),
            accounts: &[],
            meter,
            events,
            depth: 0,
            return_data: None,
            instruction_data: &[],
        }
    }

    #[test]
    fn test_log_syscall() {
        let meter = ComputeMeter::new(crate::metering::ComputeBudget::new(100000));
        let mut events = EventCollector::new([0u8; 32], 0);
        let mut ctx = make_context(&meter, &mut events);

        let handler = LogHandler;
        let mut memory = vec![0u8; 1024];
        let message = b"Hello, World!";
        memory[0..message.len()].copy_from_slice(message);

        let result = handler.execute(&mut ctx, &[0, message.len() as u64], &mut memory);

        assert!(matches!(result, SyscallResult::Ok(0)));
    }

    #[test]
    fn test_sha256_syscall() {
        let meter = ComputeMeter::new(crate::metering::ComputeBudget::new(100000));
        let mut events = EventCollector::new([0u8; 32], 0);
        let mut ctx = make_context(&meter, &mut events);

        let handler = Sha256Handler;
        let mut memory = vec![0u8; 1024];
        let input = b"test input";
        memory[0..input.len()].copy_from_slice(input);

        let result = handler.execute(
            &mut ctx,
            &[0, input.len() as u64, 100], // output at offset 100
            &mut memory,
        );

        assert!(matches!(result, SyscallResult::Ok(0)));

        // Verify hash is non-zero
        let hash = &memory[100..132];
        assert!(hash.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_blake3_syscall() {
        let meter = ComputeMeter::new(crate::metering::ComputeBudget::new(100000));
        let mut events = EventCollector::new([0u8; 32], 0);
        let mut ctx = make_context(&meter, &mut events);

        let handler = Blake3Handler;
        let mut memory = vec![0u8; 1024];
        let input = b"test input";
        memory[0..input.len()].copy_from_slice(input);

        let result = handler.execute(&mut ctx, &[0, input.len() as u64, 100], &mut memory);

        assert!(matches!(result, SyscallResult::Ok(0)));

        // Verify hash matches expected
        let expected = blake3::hash(input);
        assert_eq!(&memory[100..132], expected.as_bytes());
    }

    #[test]
    fn test_alloc_syscall() {
        let meter = ComputeMeter::new(crate::metering::ComputeBudget::new(100000));
        let mut events = EventCollector::new([0u8; 32], 0);
        let mut ctx = make_context(&meter, &mut events);

        let handler = AllocHandler::new(32 * 1024);
        let mut memory = vec![0u8; 1024];

        // First allocation
        let result = handler.execute(&mut ctx, &[100], &mut memory);
        assert!(matches!(result, SyscallResult::Ok(0)));

        // Second allocation (should be offset)
        let result = handler.execute(&mut ctx, &[100], &mut memory);
        assert!(matches!(result, SyscallResult::Ok(104))); // Aligned to 8 bytes
    }

    #[test]
    fn test_syscall_registry() {
        let registry = SyscallRegistry::new();

        // Should have default handlers
        assert!(registry.get(SyscallId::SOL_LOG).is_some());
        assert!(registry.get(SyscallId::SOL_SHA256).is_some());
        assert!(registry.get(SyscallId::SOL_BLAKE3).is_some());
    }

    #[test]
    fn test_memory_bounds_check() {
        let meter = ComputeMeter::new(crate::metering::ComputeBudget::new(100000));
        let mut events = EventCollector::new([0u8; 32], 0);
        let mut ctx = make_context(&meter, &mut events);

        let handler = LogHandler;
        let mut memory = vec![0u8; 100];

        // Out of bounds access
        let result = handler.execute(
            &mut ctx,
            &[50, 100], // 50 + 100 > 100
            &mut memory,
        );

        assert!(matches!(
            result,
            SyscallResult::Err(ProgramError::MemoryAccessViolation)
        ));
    }

    #[test]
    fn test_set_return_data() {
        let meter = ComputeMeter::new(crate::metering::ComputeBudget::new(100000));
        let mut events = EventCollector::new([0u8; 32], 0);
        let mut ctx = make_context(&meter, &mut events);

        let handler = SetReturnDataHandler;
        let mut memory = vec![0u8; 1024];
        let data = b"return data";
        memory[0..data.len()].copy_from_slice(data);

        let result = handler.execute(&mut ctx, &[0, data.len() as u64], &mut memory);

        assert!(matches!(result, SyscallResult::Ok(0)));
        assert!(ctx.return_data.is_some());
        assert_eq!(ctx.return_data.as_ref().unwrap().1, data);
    }
}
