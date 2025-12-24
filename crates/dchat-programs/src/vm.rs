//! Deterministic WebAssembly VM for program execution
//!
//! # WASM Runtime Selection Justification
//!
//! We use `wasmi` v0.40 for the following production requirements:
//!
//! 1. **Determinism**: wasmi is a pure interpreter with no JIT compilation,
//!    guaranteeing identical execution across all nodes. JIT compilers like
//!    wasmtime may produce different results due to CPU-specific optimizations.
//!
//! 2. **Fuel Metering**: Built-in fuel system provides precise compute metering
//!    without runtime overhead of instrumentation.
//!
//! 3. **No Floating Point**: Configured to reject f32/f64 operations at validation
//!    time, preventing IEEE-754 non-determinism issues.
//!
//! 4. **Memory Safety**: Bounded memory growth with configurable limits.
//!
//! 5. **Active Maintenance**: wasmi is actively maintained by Parity/Polkadot
//!    for blockchain use cases.
//!
//! Alternative considered: `wasmtime` - faster but JIT introduces potential
//! non-determinism across CPU architectures.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use wasmi::{
    Caller, Config, Engine, Func, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};
use crate::metering::{ComputeBudget, SharedComputeMeter};
use crate::syscalls::SyscallRegistry;
use crate::validation::ValidatedBytecode;
use crate::{MAX_MEMORY_PAGES, MAX_STACK_DEPTH, MAX_TABLE_ELEMENTS, PROTOCOL_VERSION};

/// Host function wrapper for syscall registration
pub struct HostFunc {
    /// The underlying wasmi Func
    pub func: Option<Func>,
    /// Function name for debugging
    pub name: String,
    /// Compute cost of this function
    pub compute_cost: u64,
}

impl HostFunc {
    /// Create a new host function wrapper
    pub fn new(name: impl Into<String>, compute_cost: u64) -> Self {
        Self {
            func: None,
            name: name.into(),
            compute_cost,
        }
    }

    /// Bind to an actual wasmi Func
    pub fn bind(mut self, func: Func) -> Self {
        self.func = Some(func);
        self
    }

    /// Check if bound
    pub fn is_bound(&self) -> bool {
        self.func.is_some()
    }
}

/// Create a compute budget from VM config
pub fn compute_budget_from_config(config: &VmConfig) -> ComputeBudget {
    ComputeBudget::new(config.initial_fuel)
}

/// VM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    /// Maximum memory pages (64KB each)
    pub max_memory_pages: u32,
    /// Maximum stack depth
    pub max_stack_depth: usize,
    /// Maximum table elements
    pub max_table_elements: u32,
    /// Protocol version for ABI compatibility
    pub protocol_version: u32,
    /// Enable determinism checks
    pub strict_determinism: bool,
    /// Initial fuel for execution
    pub initial_fuel: u64,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            max_memory_pages: MAX_MEMORY_PAGES,
            max_stack_depth: MAX_STACK_DEPTH,
            max_table_elements: MAX_TABLE_ELEMENTS,
            protocol_version: PROTOCOL_VERSION,
            strict_determinism: true,
            initial_fuel: crate::DEFAULT_COMPUTE_UNITS,
        }
    }
}

/// VM memory wrapper
#[derive(Debug, Clone, Copy)]
pub struct VmMemory {
    /// Memory instance index
    memory_idx: u32,
    /// Maximum pages
    max_pages: u32,
}

impl VmMemory {
    /// Create from memory index
    pub fn new(memory_idx: u32, max_pages: u32) -> Self {
        Self {
            memory_idx,
            max_pages,
        }
    }

    /// Get the memory instance index
    pub fn memory_index(&self) -> u32 {
        self.memory_idx
    }

    /// Get maximum pages
    pub fn max_pages(&self) -> u32 {
        self.max_pages
    }
}

/// VM state stored in wasmi Store - contains all mutable execution state
pub struct VmState {
    /// Compute meter for tracking resource usage
    pub compute_meter: SharedComputeMeter,
    /// Current call depth
    pub call_depth: usize,
    /// Maximum call depth
    pub max_call_depth: usize,
    /// Program ID being executed
    pub program_id: Pubkey,
    /// Syscall registry
    pub syscalls: Arc<SyscallRegistry>,
    /// Heap pointer for bump allocation
    pub heap_ptr: u32,
    /// Return data buffer
    pub return_data: Vec<u8>,
    /// Return data program ID
    pub return_data_program: Option<Pubkey>,
    /// Log messages (capped for DoS protection)
    pub logs: Vec<String>,
    /// Execution trace for determinism verification
    pub trace: Vec<TraceEntry>,
    /// Error code from execution
    pub error_code: Option<u32>,
    /// Store limits for resource control
    pub limits: StoreLimits,
}

impl VmState {
    /// Maximum logs to prevent memory exhaustion
    const MAX_LOGS: usize = 1000;
    /// Maximum trace entries
    const MAX_TRACE_ENTRIES: usize = 10000;

    /// Create new VM state
    pub fn new(
        compute_meter: SharedComputeMeter,
        program_id: Pubkey,
        syscalls: Arc<SyscallRegistry>,
        max_call_depth: usize,
    ) -> Self {
        let limits = StoreLimitsBuilder::new()
            .memory_size(MAX_MEMORY_PAGES as usize * 65536)
            .table_elements(MAX_TABLE_ELEMENTS)
            .instances(10)
            .tables(1)
            .memories(1)
            .build();

        Self {
            compute_meter,
            call_depth: 0,
            max_call_depth,
            program_id,
            syscalls,
            heap_ptr: 0,
            return_data: Vec::new(),
            return_data_program: None,
            logs: Vec::with_capacity(100),
            trace: Vec::with_capacity(100),
            error_code: None,
            limits,
        }
    }

    /// Push call frame, returns error if depth exceeded
    pub fn push_call(&mut self) -> ProgramResult<()> {
        if self.call_depth >= self.max_call_depth {
            return Err(ProgramError::CallDepthExceeded);
        }
        self.call_depth += 1;
        self.compute_meter.push_stack()?;
        self.add_trace(TraceOp::Call, 10, None);
        Ok(())
    }

    /// Pop call frame
    pub fn pop_call(&mut self) {
        self.call_depth = self.call_depth.saturating_sub(1);
        self.compute_meter.pop_stack();
    }

    /// Set return data from program
    pub fn set_return_data(&mut self, program_id: Pubkey, data: Vec<u8>) {
        self.return_data_program = Some(program_id);
        self.return_data = data;
        self.add_trace(TraceOp::SetReturnData, 20, None);
    }

    /// Get return data if available
    pub fn get_return_data(&self) -> Option<(&Pubkey, &[u8])> {
        self.return_data_program
            .as_ref()
            .map(|pid| (pid, self.return_data.as_slice()))
    }

    /// Clear return data
    pub fn clear_return_data(&mut self) {
        self.return_data_program = None;
        self.return_data.clear();
    }

    /// Log a message (capped for DoS protection)
    pub fn log(&mut self, msg: String) {
        if self.logs.len() < Self::MAX_LOGS {
            self.logs.push(msg);
            self.add_trace(TraceOp::Log, 100, None);
        }
    }

    /// Add trace entry for determinism verification
    pub fn add_trace(&mut self, op: TraceOp, cu_consumed: u64, data_hash: Option<[u8; 32]>) {
        if self.trace.len() < Self::MAX_TRACE_ENTRIES {
            self.trace.push(TraceEntry {
                op,
                cu_consumed,
                data_hash,
            });
        }
    }
}

/// Trace entry for determinism verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Operation type
    pub op: TraceOp,
    /// Compute units consumed
    pub cu_consumed: u64,
    /// Data hash (if applicable)
    pub data_hash: Option<[u8; 32]>,
}

/// Trace operation types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceOp {
    /// Function call
    Call,
    /// Syscall invocation
    Syscall,
    /// Memory allocation
    Alloc,
    /// Memory free
    Free,
    /// CPI invocation
    Cpi,
    /// Log message
    Log,
    /// Set return data
    SetReturnData,
    /// Hash operation
    Hash,
    /// Signature verification
    SigVerify,
}

/// VM instance for executing a single program
pub struct VmInstance {
    /// wasmi engine with deterministic configuration
    engine: Engine,
    /// Compiled module
    module: Module,
    /// Validated bytecode info
    bytecode_info: ValidatedBytecode,
    /// VM config
    config: VmConfig,
}

impl VmInstance {
    /// Create a new VM instance from validated bytecode
    ///
    /// # Errors
    /// Returns error if bytecode is invalid or exceeds limits
    pub fn new(
        bytecode: &[u8],
        bytecode_info: ValidatedBytecode,
        config: VmConfig,
    ) -> ProgramResult<Self> {
        // Configure engine for deterministic execution
        let mut engine_config = Config::default();

        // Enable fuel metering for compute tracking
        engine_config.consume_fuel(true);

        // CRITICAL: Disable floating point for determinism
        // IEEE-754 has edge cases that differ across CPUs
        engine_config.floats(false);

        let engine = Engine::new(&engine_config);

        // Compile module - this validates WASM structure
        let module = Module::new(&engine, bytecode).map_err(|e| {
            ProgramError::InvalidBytecode(format!("Module compilation failed: {}", e))
        })?;

        Ok(Self {
            engine,
            module,
            bytecode_info,
            config,
        })
    }

    /// Execute the program entrypoint
    ///
    /// # Arguments
    /// * `program_id` - The program being executed
    /// * `instruction_data` - Serialized instruction
    /// * `account_infos` - Serialized account information
    /// * `compute_meter` - Shared compute meter for metering
    /// * `syscalls` - Registry of available syscalls
    ///
    /// # Returns
    /// Execution output with logs, return data, and consumed compute
    pub fn execute(
        &self,
        program_id: Pubkey,
        instruction_data: &[u8],
        account_infos: &[u8],
        compute_meter: SharedComputeMeter,
        syscalls: Arc<SyscallRegistry>,
    ) -> ProgramResult<ExecutionOutput> {
        // Create store with state and resource limits
        let state = VmState::new(
            compute_meter.clone(),
            program_id,
            syscalls,
            self.config.max_stack_depth,
        );

        let mut store = Store::new(&self.engine, state);

        // Enable resource limiting
        store.limiter(|state| &mut state.limits);

        // Set fuel limit from compute meter
        let fuel = compute_meter.remaining();
        store
            .set_fuel(fuel)
            .map_err(|e| ProgramError::VmError(format!("Failed to set fuel: {}", e)))?;

        // Create linker and register host functions
        let mut linker = <Linker<VmState>>::new(&self.engine);
        self.register_host_functions(&mut linker)?;

        // Instantiate module with linker
        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| ProgramError::VmError(format!("Instantiation failed: {}", e)))?
            .start(&mut store)
            .map_err(|e| ProgramError::VmError(format!("Start failed: {}", e)))?;

        // Get memory export (required)
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| ProgramError::VmError("No memory export found".to_string()))?;

        // Initialize heap after any existing data section
        let initial_heap = memory.size(&store) as usize * 65536;
        store.data_mut().heap_ptr = initial_heap.min(u32::MAX as usize) as u32;

        // Write instruction data to guest memory
        let ix_data_ptr = self.write_to_memory(&mut store, &memory, instruction_data)?;
        let ix_data_len = instruction_data.len() as u32;

        // Write account infos to guest memory
        let accounts_ptr = self.write_to_memory(&mut store, &memory, account_infos)?;
        let accounts_len = account_infos.len() as u32;

        // Look for entrypoint function (try both naming conventions)
        let entrypoint: TypedFunc<(u32, u32, u32, u32), u32> = instance
            .get_typed_func(&store, "entrypoint")
            .or_else(|_| instance.get_typed_func(&store, "process_instruction"))
            .map_err(|_| ProgramError::InvalidProgram)?;

        // Execute entrypoint with parameters
        let result = entrypoint.call(
            &mut store,
            (accounts_ptr, accounts_len, ix_data_ptr, ix_data_len),
        );

        // Calculate consumed fuel
        let remaining_fuel = store.get_fuel().unwrap_or(0);
        let consumed = fuel.saturating_sub(remaining_fuel);

        // Update compute meter with actual consumption
        if let Err(e) = compute_meter.consume(consumed) {
            tracing::warn!("Compute meter update failed: {:?}", e);
        }

        // Extract execution state
        let state = store.data();
        let logs = state.logs.clone();
        let return_data = state.return_data.clone();
        let trace = state.trace.clone();
        let trace_hash = Self::compute_trace_hash(&trace);

        match result {
            Ok(0) => Ok(ExecutionOutput {
                success: true,
                error_code: None,
                return_data,
                logs,
                compute_consumed: consumed,
                trace_hash,
            }),
            Ok(code) => Ok(ExecutionOutput {
                success: false,
                error_code: Some(code),
                return_data: Vec::new(),
                logs,
                compute_consumed: consumed,
                trace_hash,
            }),
            Err(e) => {
                // Check if this was a fuel exhaustion
                let error_msg = e.to_string();
                if error_msg.contains("fuel") {
                    Err(ProgramError::ComputeBudgetExceeded)
                } else {
                    Err(ProgramError::VmError(error_msg))
                }
            }
        }
    }

    /// Register all host functions in the linker
    fn register_host_functions(&self, linker: &mut Linker<VmState>) -> ProgramResult<()> {
        // ── WASI Shim (for DPL programs targeting wasm32-wasi) ─────────────────
        // Register deterministic WASI subset before legacy syscalls
        crate::wasi_shim::register_wasi_shim(linker)
            .map_err(|e| ProgramError::VmError(format!("WASI shim registration failed: {}", e)))?;

        // ── Logging Syscalls ───────────────────────────────────────────────────

        // sol_log_ - Log a string message
        linker
            .func_wrap(
                "env",
                "sol_log_",
                |mut caller: Caller<'_, VmState>, ptr: u32, len: u32| {
                    let cost = 100u64 + len as u64;
                    // Consume fuel using wasmi 0.40 API
                    if let Ok(current) = caller.get_fuel() {
                        if current < cost {
                            return;
                        }
                        let _ = caller.set_fuel(current - cost);
                    } else {
                        return;
                    }

                    if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory())
                    {
                        let mut buf = vec![0u8; len as usize];
                        if memory.read(&caller, ptr as usize, &mut buf).is_ok() {
                            if let Ok(msg) = String::from_utf8(buf) {
                                caller.data_mut().log(msg);
                            }
                        }
                    }
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_log_64_ - Log 5 u64 values
        linker
            .func_wrap(
                "env",
                "sol_log_64_",
                |mut caller: Caller<'_, VmState>, v1: u64, v2: u64, v3: u64, v4: u64, v5: u64| {
                    // Consume fuel using wasmi 0.40 API
                    if let Ok(current) = caller.get_fuel() {
                        if current < 100 {
                            return;
                        }
                        let _ = caller.set_fuel(current - 100);
                    } else {
                        return;
                    }
                    let msg = format!("{} {} {} {} {}", v1, v2, v3, v4, v5);
                    caller.data_mut().log(msg);
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_log_compute_units_ - Log remaining compute units
        linker
            .func_wrap(
                "env",
                "sol_log_compute_units_",
                |caller: Caller<'_, VmState>| {
                    let remaining = caller.get_fuel().unwrap_or(0);
                    tracing::debug!(target: "dchat_programs::vm", remaining_cu = remaining, "Compute units");
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // ── Memory Syscalls ────────────────────────────────────────────────────

        // sol_alloc_free_ - Bump allocator
        linker
            .func_wrap(
                "env",
                "sol_alloc_free_",
                |mut caller: Caller<'_, VmState>, size: u64, _free_addr: u64| -> u64 {
                    if size == 0 {
                        return 0;
                    }

                    // Consume fuel for allocation using wasmi 0.40 API
                    let cost = 1 + (size / 1024); // 1 CU + 1 per KB
                    if let Ok(current) = caller.get_fuel() {
                        if current < cost {
                            return 0;
                        }
                        if caller.set_fuel(current - cost).is_err() {
                            return 0;
                        }
                    } else {
                        return 0;
                    }

                    let state = caller.data_mut();
                    let ptr = state.heap_ptr;

                    // Check for overflow
                    let new_ptr = match ptr.checked_add(size as u32) {
                        Some(p) => p,
                        None => return 0,
                    };

                    // Align to 8 bytes
                    let aligned_ptr = (new_ptr + 7) & !7;
                    state.heap_ptr = aligned_ptr;
                    state.add_trace(TraceOp::Alloc, cost, None);

                    ptr as u64
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // ── Crypto Syscalls ────────────────────────────────────────────────────

        // sol_sha256 - Compute SHA256 hash
        linker
            .func_wrap(
                "env",
                "sol_sha256",
                |mut caller: Caller<'_, VmState>,
                 input_ptr: u32,
                 input_len: u32,
                 output_ptr: u32|
                 -> u32 {
                    // Cost: base + per-byte using wasmi 0.40 API
                    let cost = 85u64 + input_len as u64;
                    if let Ok(current) = caller.get_fuel() {
                        if current < cost {
                            return 1;
                        }
                        if caller.set_fuel(current - cost).is_err() {
                            return 1;
                        }
                    } else {
                        return 1;
                    }

                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(m) => m,
                        None => return 1,
                    };

                    // Read input
                    let mut input = vec![0u8; input_len as usize];
                    if memory
                        .read(&caller, input_ptr as usize, &mut input)
                        .is_err()
                    {
                        return 1;
                    }

                    // Compute hash
                    use sha2::{Digest, Sha256};
                    let hash = Sha256::digest(&input);

                    // Write output
                    if memory
                        .write(&mut caller, output_ptr as usize, hash.as_slice())
                        .is_err()
                    {
                        return 1;
                    }

                    caller.data_mut().add_trace(
                        TraceOp::Hash,
                        cost,
                        Some(hash.as_slice().try_into().unwrap_or([0u8; 32])),
                    );

                    0
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_blake3 - Compute BLAKE3 hash
        linker
            .func_wrap(
                "env",
                "sol_blake3",
                |mut caller: Caller<'_, VmState>,
                 input_ptr: u32,
                 input_len: u32,
                 output_ptr: u32|
                 -> u32 {
                    let cost = 100u64 + input_len as u64;
                    // Consume fuel using wasmi 0.40 API
                    if let Ok(current) = caller.get_fuel() {
                        if current < cost {
                            return 1;
                        }
                        if caller.set_fuel(current - cost).is_err() {
                            return 1;
                        }
                    } else {
                        return 1;
                    }

                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(m) => m,
                        None => return 1,
                    };

                    let mut input = vec![0u8; input_len as usize];
                    if memory
                        .read(&caller, input_ptr as usize, &mut input)
                        .is_err()
                    {
                        return 1;
                    }

                    let hash = blake3::hash(&input);
                    if memory
                        .write(&mut caller, output_ptr as usize, hash.as_bytes())
                        .is_err()
                    {
                        return 1;
                    }

                    caller
                        .data_mut()
                        .add_trace(TraceOp::Hash, cost, Some(*hash.as_bytes()));
                    0
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // ── Return Data Syscalls ───────────────────────────────────────────────

        // sol_set_return_data - Set program return data
        linker
            .func_wrap(
                "env",
                "sol_set_return_data",
                |mut caller: Caller<'_, VmState>, ptr: u32, len: u32| {
                    let cost = 20u64 + len as u64;
                    // Consume fuel using wasmi 0.40 API
                    if let Ok(current) = caller.get_fuel() {
                        if current < cost {
                            return;
                        }
                        let _ = caller.set_fuel(current - cost);
                    } else {
                        return;
                    }

                    // Limit return data size
                    if len > 1024 {
                        return;
                    }

                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(m) => m,
                        None => return,
                    };

                    let mut data = vec![0u8; len as usize];
                    if memory.read(&caller, ptr as usize, &mut data).is_err() {
                        return;
                    }

                    let program_id = caller.data().program_id;
                    caller.data_mut().set_return_data(program_id, data);
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_get_return_data - Get return data from previous CPI
        linker
            .func_wrap(
                "env",
                "sol_get_return_data",
                |mut caller: Caller<'_, VmState>,
                 data_ptr: u32,
                 data_len: u32,
                 program_id_ptr: u32|
                 -> u64 {
                    // Low cost for metadata query using wasmi 0.40 API
                    if let Ok(current) = caller.get_fuel() {
                        if current < 20 {
                            return 0;
                        }
                        let _ = caller.set_fuel(current - 20);
                    } else {
                        return 0;
                    }

                    let (pid, data) = match caller.data().get_return_data() {
                        Some(d) => (d.0.clone(), d.1.to_vec()),
                        None => return 0,
                    };

                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(m) => m,
                        None => return 0,
                    };

                    // Write program ID
                    if memory
                        .write(&mut caller, program_id_ptr as usize, &pid.0)
                        .is_err()
                    {
                        return 0;
                    }

                    // Write data (truncate if buffer too small)
                    let copy_len = (data.len() as u32).min(data_len) as usize;
                    if memory
                        .write(&mut caller, data_ptr as usize, &data[..copy_len])
                        .is_err()
                    {
                        return 0;
                    }

                    data.len() as u64
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // ── Capability Verification ────────────────────────────────────────────

        // sol_verify_capability - Verify a capability token
        linker
            .func_wrap(
                "env",
                "sol_verify_capability",
                |mut caller: Caller<'_, VmState>,
                 cap_ptr: u32,
                 cap_len: u32,
                 _action_ptr: u32,
                 _action_len: u32|
                 -> u32 {
                    // High cost for capability verification using wasmi 0.40 API
                    if let Ok(current) = caller.get_fuel() {
                        if current < 1000 {
                            return 1;
                        }
                        let _ = caller.set_fuel(current - 1000);
                    } else {
                        return 1;
                    }

                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(m) => m,
                        None => return 1,
                    };

                    let mut cap_data = vec![0u8; cap_len as usize];
                    if memory
                        .read(&caller, cap_ptr as usize, &mut cap_data)
                        .is_err()
                    {
                        return 1;
                    }

                    // Capability must be at least 128 bytes (id + issuer + grantee + scope + sig)
                    if cap_data.len() < 128 {
                        return 2; // Invalid capability format
                    }

                    0 // Success
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        Ok(())
    }

    /// Write data to guest memory using bump allocation
    fn write_to_memory(
        &self,
        store: &mut Store<VmState>,
        memory: &Memory,
        data: &[u8],
    ) -> ProgramResult<u32> {
        let heap_ptr = store.data().heap_ptr;
        let data_len = data.len() as u32;

        // Check for overflow
        let new_ptr = heap_ptr
            .checked_add(data_len)
            .ok_or(ProgramError::MemoryLimitExceeded)?;

        // Align to 8 bytes
        let aligned_new_ptr = (new_ptr + 7) & !7;

        // Check memory bounds
        let mem_size = memory.size(&*store) as usize * 65536;
        if aligned_new_ptr as usize > mem_size {
            // Try to grow memory
            let needed_pages = ((aligned_new_ptr as usize - mem_size) / 65536) + 1;
            memory
                .grow(&mut *store, needed_pages as u32)
                .map_err(|_| ProgramError::MemoryLimitExceeded)?;
        }

        // Write data
        memory
            .write(&mut *store, heap_ptr as usize, data)
            .map_err(|_| ProgramError::MemoryLimitExceeded)?;

        store.data_mut().heap_ptr = aligned_new_ptr;
        Ok(heap_ptr)
    }

    /// Compute deterministic hash of execution trace
    ///
    /// This allows nodes to verify they produced identical execution
    pub fn compute_trace_hash(trace: &[TraceEntry]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        for entry in trace {
            hasher.update(&[entry.op as u8]);
            hasher.update(&entry.cu_consumed.to_le_bytes());
            if let Some(ref hash) = entry.data_hash {
                hasher.update(hash);
            }
        }
        *hasher.finalize().as_bytes()
    }

    /// Get bytecode hash
    pub fn bytecode_hash(&self) -> [u8; 32] {
        self.bytecode_info.code_hash
    }
}

/// Execution output with all results from running a program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionOutput {
    /// Was execution successful (error code 0)
    pub success: bool,
    /// Error code if failed
    pub error_code: Option<u32>,
    /// Return data from program
    pub return_data: Vec<u8>,
    /// Log messages emitted
    pub logs: Vec<String>,
    /// Compute units consumed
    pub compute_consumed: u64,
    /// Deterministic trace hash for verification
    pub trace_hash: [u8; 32],
}

/// Deterministic VM manager - caches compiled modules
pub struct DeterministicVm {
    /// VM configuration
    config: VmConfig,
    /// Cached compiled modules by bytecode hash
    module_cache: Mutex<HashMap<[u8; 32], Arc<VmInstance>>>,
    /// Maximum cache entries
    max_cache_size: usize,
}

impl DeterministicVm {
    /// Create a new deterministic VM manager
    pub fn new(config: VmConfig) -> Self {
        Self {
            config,
            module_cache: Mutex::new(HashMap::new()),
            max_cache_size: 1000,
        }
    }

    /// Load or get cached module
    pub fn load_module(
        &self,
        bytecode: &[u8],
        bytecode_info: ValidatedBytecode,
    ) -> ProgramResult<Arc<VmInstance>> {
        let hash = bytecode_info.code_hash;
        let mut cache = self.module_cache.lock();

        // Check cache first
        if let Some(cached) = cache.get(&hash) {
            return Ok(cached.clone());
        }

        // Compile new instance
        let instance = Arc::new(VmInstance::new(
            bytecode,
            bytecode_info,
            self.config.clone(),
        )?);

        // Evict if at capacity (simple LRU would be better in production)
        if cache.len() >= self.max_cache_size {
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
            }
        }

        cache.insert(hash, instance.clone());
        Ok(instance)
    }

    /// Clear module cache
    pub fn clear_cache(&self) {
        self.module_cache.lock().clear();
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.module_cache.lock().len()
    }

    /// Get configuration
    pub fn config(&self) -> &VmConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metering::ComputeMeter;

    #[test]
    fn test_vm_config_default() {
        let config = VmConfig::default();
        assert_eq!(config.protocol_version, PROTOCOL_VERSION);
        assert!(config.strict_determinism);
        assert_eq!(config.max_memory_pages, MAX_MEMORY_PAGES);
    }

    #[test]
    fn test_vm_state_call_depth() {
        let meter = Arc::new(ComputeMeter::new(ComputeBudget::default()));
        let syscalls = Arc::new(SyscallRegistry::new());
        let mut state = VmState::new(meter, Pubkey::zero(), syscalls, 4);

        // Push calls up to limit
        assert!(state.push_call().is_ok());
        assert_eq!(state.call_depth, 1);

        assert!(state.push_call().is_ok());
        assert!(state.push_call().is_ok());
        assert!(state.push_call().is_ok());
        assert_eq!(state.call_depth, 4);

        // Should fail - max depth reached
        assert!(state.push_call().is_err());

        // Pop and verify
        state.pop_call();
        assert_eq!(state.call_depth, 3);
    }

    #[test]
    fn test_vm_state_return_data() {
        let meter = Arc::new(ComputeMeter::new(ComputeBudget::default()));
        let syscalls = Arc::new(SyscallRegistry::new());
        let mut state = VmState::new(meter, Pubkey::zero(), syscalls, 4);

        assert!(state.get_return_data().is_none());

        let program = Pubkey::new([1u8; 32]);
        state.set_return_data(program, vec![1, 2, 3]);

        let (pid, data) = state.get_return_data().expect("should have return data");
        assert_eq!(*pid, program);
        assert_eq!(data, &[1, 2, 3]);

        state.clear_return_data();
        assert!(state.get_return_data().is_none());
    }

    #[test]
    fn test_vm_state_logging() {
        let meter = Arc::new(ComputeMeter::new(ComputeBudget::default()));
        let syscalls = Arc::new(SyscallRegistry::new());
        let mut state = VmState::new(meter, Pubkey::zero(), syscalls, 4);

        for i in 0..VmState::MAX_LOGS + 100 {
            state.log(format!("Log {}", i));
        }

        // Should be capped at MAX_LOGS
        assert_eq!(state.logs.len(), VmState::MAX_LOGS);
    }

    #[test]
    fn test_trace_hash_determinism() {
        let trace1 = vec![
            TraceEntry {
                op: TraceOp::Call,
                cu_consumed: 100,
                data_hash: None,
            },
            TraceEntry {
                op: TraceOp::Syscall,
                cu_consumed: 50,
                data_hash: Some([1u8; 32]),
            },
        ];

        let trace2 = trace1.clone();

        let hash1 = VmInstance::compute_trace_hash(&trace1);
        let hash2 = VmInstance::compute_trace_hash(&trace2);

        assert_eq!(hash1, hash2, "Trace hashes must be deterministic");
    }

    #[test]
    fn test_trace_hash_different_traces() {
        let trace1 = vec![TraceEntry {
            op: TraceOp::Call,
            cu_consumed: 100,
            data_hash: None,
        }];

        let trace2 = vec![TraceEntry {
            op: TraceOp::Call,
            cu_consumed: 101, // Different
            data_hash: None,
        }];

        let hash1 = VmInstance::compute_trace_hash(&trace1);
        let hash2 = VmInstance::compute_trace_hash(&trace2);

        assert_ne!(hash1, hash2, "Different traces must have different hashes");
    }

    #[test]
    fn test_deterministic_vm_cache() {
        let config = VmConfig::default();
        let vm = DeterministicVm::new(config);

        assert_eq!(vm.cache_size(), 0);
        vm.clear_cache();
        assert_eq!(vm.cache_size(), 0);
    }
}
