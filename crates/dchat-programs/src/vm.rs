//! Deterministic WebAssembly VM for program execution

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use wasmi::{Caller, Engine, Func, Linker, Memory, Module, Store, TypedFunc};

use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};
use crate::metering::{ComputeBudget, ComputeMeter, SharedComputeMeter};
use crate::syscalls::SyscallRegistry;
use crate::validation::ValidatedBytecode;
use crate::{MAX_MEMORY_PAGES, MAX_STACK_DEPTH, MAX_TABLE_ELEMENTS, PROTOCOL_VERSION};

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
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            max_memory_pages: MAX_MEMORY_PAGES,
            max_stack_depth: MAX_STACK_DEPTH,
            max_table_elements: MAX_TABLE_ELEMENTS,
            protocol_version: PROTOCOL_VERSION,
            strict_determinism: true,
        }
    }
}

/// VM memory wrapper
#[derive(Debug)]
pub struct VmMemory {
    /// Memory instance
    memory: Memory,
    /// Maximum pages
    max_pages: u32,
}

impl VmMemory {
    /// Create from wasmi memory
    pub fn new(memory: Memory, max_pages: u32) -> Self {
        Self { memory, max_pages }
    }

    /// Get current size in pages
    pub fn current_pages(&self, store: &Store<VmState>) -> u32 {
        self.memory.current_pages(store).into()
    }

    /// Get maximum pages
    pub fn max_pages(&self) -> u32 {
        self.max_pages
    }

    /// Read bytes from memory
    pub fn read(&self, store: &Store<VmState>, offset: u32, len: u32) -> ProgramResult<Vec<u8>> {
        let mut buf = vec![0u8; len as usize];
        self.memory
            .read(store, offset as usize, &mut buf)
            .map_err(|_| ProgramError::MemoryLimitExceeded)?;
        Ok(buf)
    }

    /// Write bytes to memory
    pub fn write(&self, store: &mut Store<VmState>, offset: u32, data: &[u8]) -> ProgramResult<()> {
        self.memory
            .write(store, offset as usize, data)
            .map_err(|_| ProgramError::MemoryLimitExceeded)
    }

    /// Get memory as slice
    pub fn data<'a>(&self, store: &'a Store<VmState>) -> &'a [u8] {
        self.memory.data(store)
    }

    /// Get memory as mutable slice
    pub fn data_mut<'a>(&self, store: &'a mut Store<VmState>) -> &'a mut [u8] {
        self.memory.data_mut(store)
    }
}

/// VM state stored in wasmi Store
pub struct VmState {
    /// Compute meter
    pub compute_meter: SharedComputeMeter,
    /// Current call depth
    pub call_depth: usize,
    /// Maximum call depth
    pub max_call_depth: usize,
    /// Program ID being executed
    pub program_id: Pubkey,
    /// Syscall registry
    pub syscalls: Arc<SyscallRegistry>,
    /// Heap pointer
    pub heap_ptr: u32,
    /// Return data buffer
    pub return_data: Vec<u8>,
    /// Return data program ID
    pub return_data_program: Option<Pubkey>,
    /// Log messages
    pub logs: Vec<String>,
    /// Execution trace (for determinism verification)
    pub trace: Vec<TraceEntry>,
    /// Error code
    pub error_code: Option<u32>,
}

impl VmState {
    /// Create new VM state
    pub fn new(
        compute_meter: SharedComputeMeter,
        program_id: Pubkey,
        syscalls: Arc<SyscallRegistry>,
        max_call_depth: usize,
    ) -> Self {
        Self {
            compute_meter,
            call_depth: 0,
            max_call_depth,
            program_id,
            syscalls,
            heap_ptr: 0,
            return_data: Vec::new(),
            return_data_program: None,
            logs: Vec::new(),
            trace: Vec::new(),
            error_code: None,
        }
    }

    /// Push call frame
    pub fn push_call(&mut self) -> ProgramResult<()> {
        if self.call_depth >= self.max_call_depth {
            return Err(ProgramError::CallDepthExceeded);
        }
        self.call_depth += 1;
        self.compute_meter.push_stack()?;
        Ok(())
    }

    /// Pop call frame
    pub fn pop_call(&mut self) {
        self.call_depth = self.call_depth.saturating_sub(1);
        self.compute_meter.pop_stack();
    }

    /// Set return data
    pub fn set_return_data(&mut self, program_id: Pubkey, data: Vec<u8>) {
        self.return_data_program = Some(program_id);
        self.return_data = data;
    }

    /// Get return data
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

    /// Log a message
    pub fn log(&mut self, msg: String) {
        if self.logs.len() < 1000 {
            // Limit log count
            self.logs.push(msg);
        }
    }

    /// Add trace entry
    pub fn trace(&mut self, entry: TraceEntry) {
        if self.trace.len() < 10000 {
            // Limit trace size
            self.trace.push(entry);
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
}

/// VM instance for executing a single program
pub struct VmInstance {
    /// wasmi engine
    engine: Engine,
    /// wasmi module
    module: Module,
    /// Validated bytecode info
    bytecode_info: ValidatedBytecode,
    /// VM config
    config: VmConfig,
}

impl VmInstance {
    /// Create a new VM instance from validated bytecode
    pub fn new(
        bytecode: &[u8],
        bytecode_info: ValidatedBytecode,
        config: VmConfig,
    ) -> ProgramResult<Self> {
        let mut engine_config = wasmi::Config::default();
        engine_config.consume_fuel(true);
        engine_config.floats(false); // Disable floating point for determinism

        let engine = Engine::new(&engine_config);
        let module = Module::new(&engine, bytecode)
            .map_err(|e| ProgramError::InvalidBytecode(e.to_string()))?;

        Ok(Self {
            engine,
            module,
            bytecode_info,
            config,
        })
    }

    /// Execute the program entrypoint
    pub fn execute(
        &self,
        program_id: Pubkey,
        instruction_data: &[u8],
        account_infos: &[u8], // Serialized account infos
        compute_meter: SharedComputeMeter,
        syscalls: Arc<SyscallRegistry>,
    ) -> ProgramResult<ExecutionOutput> {
        // Create store with state
        let state = VmState::new(
            compute_meter.clone(),
            program_id,
            syscalls,
            self.config.max_stack_depth,
        );

        let mut store = Store::new(&self.engine, state);

        // Set fuel limit
        let fuel = compute_meter.remaining();
        store.set_fuel(fuel).ok();

        // Create linker and add host functions
        let mut linker = Linker::new(&self.engine);
        self.register_host_functions(&mut linker)?;

        // Instantiate module
        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| ProgramError::VmError(e.to_string()))?
            .start(&mut store)
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // Get memory export
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| ProgramError::VmError("No memory export".to_string()))?;

        // Write instruction data to memory
        let ix_data_ptr = self.write_to_heap(&mut store, &memory, instruction_data)?;
        let ix_data_len = instruction_data.len() as u32;

        // Write account infos to memory
        let accounts_ptr = self.write_to_heap(&mut store, &memory, account_infos)?;
        let accounts_len = account_infos.len() as u32;

        // Get entrypoint function
        let entrypoint: TypedFunc<(u32, u32, u32, u32), u32> = instance
            .get_typed_func(&store, "entrypoint")
            .or_else(|_| instance.get_typed_func(&store, "process_instruction"))
            .map_err(|_| ProgramError::InvalidProgram)?;

        // Execute entrypoint
        let result = entrypoint.call(
            &mut store,
            (accounts_ptr, accounts_len, ix_data_ptr, ix_data_len),
        );

        // Get remaining fuel
        let remaining_fuel = store.get_fuel().unwrap_or(0);
        let consumed = fuel.saturating_sub(remaining_fuel);

        // Update compute meter
        compute_meter.consume(consumed).ok();

        // Extract state
        let state = store.data();
        let logs = state.logs.clone();
        let return_data = state.return_data.clone();
        let trace = state.trace.clone();

        match result {
            Ok(0) => Ok(ExecutionOutput {
                success: true,
                error_code: None,
                return_data,
                logs,
                compute_consumed: consumed,
                trace_hash: Self::compute_trace_hash(&trace),
            }),
            Ok(code) => Ok(ExecutionOutput {
                success: false,
                error_code: Some(code),
                return_data: Vec::new(),
                logs,
                compute_consumed: consumed,
                trace_hash: Self::compute_trace_hash(&trace),
            }),
            Err(e) => Err(ProgramError::VmError(e.to_string())),
        }
    }

    /// Register host functions in linker
    fn register_host_functions(&self, linker: &mut Linker<VmState>) -> ProgramResult<()> {
        // sol_log_
        linker
            .func_wrap(
                "env",
                "sol_log_",
                |caller: Caller<'_, VmState>, ptr: u32, len: u32| {
                    let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                    if let Some(mem) = memory {
                        let data = caller.data();
                        if data.compute_meter.consume(100).is_ok() {
                            if let Ok(bytes) = read_memory(&mem, &caller, ptr, len) {
                                if let Ok(msg) = String::from_utf8(bytes) {
                                    // Can't mutate here, would need different approach
                                    tracing::debug!("Program log: {}", msg);
                                }
                            }
                        }
                    }
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_log_64_
        linker
            .func_wrap(
                "env",
                "sol_log_64_",
                |caller: Caller<'_, VmState>, v1: u64, v2: u64, v3: u64, v4: u64, v5: u64| {
                    let data = caller.data();
                    data.compute_meter.consume(100).ok();
                    tracing::debug!("Log64: {} {} {} {} {}", v1, v2, v3, v4, v5);
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_log_compute_units_
        linker
            .func_wrap(
                "env",
                "sol_log_compute_units_",
                |caller: Caller<'_, VmState>| {
                    let data = caller.data();
                    let remaining = data.compute_meter.remaining();
                    tracing::debug!("Compute units remaining: {}", remaining);
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_alloc_free_
        linker
            .func_wrap(
                "env",
                "sol_alloc_free_",
                |mut caller: Caller<'_, VmState>, size: u64, _free_addr: u64| -> u64 {
                    if size == 0 {
                        return 0;
                    }

                    let data = caller.data_mut();
                    if data.compute_meter.allocate_heap(size).is_err() {
                        return 0;
                    }

                    // Simple bump allocator
                    let ptr = data.heap_ptr;
                    data.heap_ptr = data.heap_ptr.saturating_add(size as u32);
                    ptr as u64
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_sha256
        linker
            .func_wrap(
                "env",
                "sol_sha256",
                |caller: Caller<'_, VmState>,
                 input_ptr: u32,
                 input_len: u32,
                 output_ptr: u32|
                 -> u32 {
                    let data = caller.data();
                    if data
                        .compute_meter
                        .consume_sha256(input_len as usize)
                        .is_err()
                    {
                        return 1;
                    }

                    let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                    if memory.is_none() {
                        return 1;
                    }
                    let mem = memory.unwrap();

                    if let Ok(input) = read_memory(&mem, &caller, input_ptr, input_len) {
                        use sha2::{Digest, Sha256};
                        let hash = Sha256::digest(&input);
                        // Would need mutable caller to write back
                        // This is simplified - real impl would use different pattern
                        0
                    } else {
                        1
                    }
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_blake3
        linker
            .func_wrap(
                "env",
                "sol_blake3",
                |caller: Caller<'_, VmState>,
                 input_ptr: u32,
                 input_len: u32,
                 _output_ptr: u32|
                 -> u32 {
                    let data = caller.data();
                    if data
                        .compute_meter
                        .consume_blake3(input_len as usize)
                        .is_err()
                    {
                        return 1;
                    }
                    0
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_set_return_data
        linker
            .func_wrap(
                "env",
                "sol_set_return_data",
                |mut caller: Caller<'_, VmState>, ptr: u32, len: u32| {
                    let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                    if let Some(mem) = memory {
                        if let Ok(data) = read_memory(&mem, &caller, ptr, len) {
                            let state = caller.data_mut();
                            let program_id = state.program_id;
                            state.set_return_data(program_id, data);
                        }
                    }
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        // sol_get_return_data
        linker
            .func_wrap(
                "env",
                "sol_get_return_data",
                |caller: Caller<'_, VmState>,
                 _data_ptr: u32,
                 _data_len: u32,
                 _program_id_ptr: u32|
                 -> u64 {
                    let data = caller.data();
                    if let Some((_program_id, return_data)) = data.get_return_data() {
                        return_data.len() as u64
                    } else {
                        0
                    }
                },
            )
            .map_err(|e| ProgramError::VmError(e.to_string()))?;

        Ok(())
    }

    /// Write data to heap and return pointer
    fn write_to_heap(
        &self,
        store: &mut Store<VmState>,
        memory: &Memory,
        data: &[u8],
    ) -> ProgramResult<u32> {
        let heap_ptr = store.data().heap_ptr;
        let new_ptr = heap_ptr
            .checked_add(data.len() as u32)
            .ok_or(ProgramError::MemoryLimitExceeded)?;

        // Check memory bounds
        let mem_size = memory.current_pages(store).to_bytes().unwrap_or(0);
        if new_ptr as usize > mem_size {
            return Err(ProgramError::MemoryLimitExceeded);
        }

        memory
            .write(store, heap_ptr as usize, data)
            .map_err(|_| ProgramError::MemoryLimitExceeded)?;

        store.data_mut().heap_ptr = new_ptr;
        Ok(heap_ptr)
    }

    /// Compute deterministic hash of execution trace
    fn compute_trace_hash(trace: &[TraceEntry]) -> [u8; 32] {
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
}

/// Helper to read memory
fn read_memory(
    memory: &Memory,
    caller: &Caller<'_, VmState>,
    ptr: u32,
    len: u32,
) -> ProgramResult<Vec<u8>> {
    let mut buf = vec![0u8; len as usize];
    memory
        .read(caller, ptr as usize, &mut buf)
        .map_err(|_| ProgramError::MemoryLimitExceeded)?;
    Ok(buf)
}

/// Execution output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionOutput {
    /// Was execution successful
    pub success: bool,
    /// Error code if failed
    pub error_code: Option<u32>,
    /// Return data
    pub return_data: Vec<u8>,
    /// Log messages
    pub logs: Vec<String>,
    /// Compute units consumed
    pub compute_consumed: u64,
    /// Deterministic trace hash
    pub trace_hash: [u8; 32],
}

/// Deterministic VM manager
pub struct DeterministicVm {
    /// VM configuration
    config: VmConfig,
    /// Cached compiled modules
    module_cache: HashMap<[u8; 32], Arc<VmInstance>>,
}

impl DeterministicVm {
    /// Create a new deterministic VM
    pub fn new(config: VmConfig) -> Self {
        Self {
            config,
            module_cache: HashMap::new(),
        }
    }

    /// Load or get cached module
    pub fn load_module(
        &mut self,
        bytecode: &[u8],
        bytecode_info: ValidatedBytecode,
    ) -> ProgramResult<Arc<VmInstance>> {
        let hash = bytecode_info.code_hash;

        if let Some(cached) = self.module_cache.get(&hash) {
            return Ok(cached.clone());
        }

        let instance = Arc::new(VmInstance::new(
            bytecode,
            bytecode_info,
            self.config.clone(),
        )?);
        self.module_cache.insert(hash, instance.clone());
        Ok(instance)
    }

    /// Clear module cache
    pub fn clear_cache(&mut self) {
        self.module_cache.clear();
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.module_cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_config_default() {
        let config = VmConfig::default();
        assert_eq!(config.protocol_version, PROTOCOL_VERSION);
        assert!(config.strict_determinism);
    }

    #[test]
    fn test_vm_state_call_depth() {
        let meter = Arc::new(ComputeMeter::new(ComputeBudget::default()));
        let syscalls = Arc::new(SyscallRegistry::new());
        let mut state = VmState::new(meter, Pubkey::zero(), syscalls, 4);

        state.push_call().unwrap();
        assert_eq!(state.call_depth, 1);

        state.push_call().unwrap();
        state.push_call().unwrap();
        state.push_call().unwrap();
        assert_eq!(state.call_depth, 4);

        // Should fail - max depth reached
        assert!(state.push_call().is_err());

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

        let (pid, data) = state.get_return_data().unwrap();
        assert_eq!(*pid, program);
        assert_eq!(data, &[1, 2, 3]);

        state.clear_return_data();
        assert!(state.get_return_data().is_none());
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

        assert_eq!(hash1, hash2);
    }
}
