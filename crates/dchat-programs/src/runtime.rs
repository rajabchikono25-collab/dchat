//! Program runtime - ties together VM, accounts, metering, and execution
//!
//! Production-grade CPI implementation with:
//! - Real invocation frame model
//! - State isolation via copy-on-write snapshots
//! - Global borrow tracking across nested calls
//! - Deterministic return data and event/log merging

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::account::{
    Account, AccountAccessTracker, AccountData, AccountInfo, AccountMeta, AccountState, Pubkey,
};
use crate::cpi::{
    AccountSnapshot, CpiExecutionOverlay, CpiGuard, CpiResult, PrivilegeChecker, CPI_BASE_COST,
    CPI_PDA_DERIVATION_COST, CPI_PER_ACCOUNT_COST, CPI_PER_DATA_BYTE_COST, MAX_CPI_RETURN_DATA,
};
use crate::cpi::{CpiContext, CrossProgramInvocation};
use crate::error::{ProgramError, ProgramResult};
use crate::events::{
    AccountDelta, EventCollector, ExecutionReceipt, LogEntry, ProgramEvent, ReturnData,
};
use crate::instruction::Instruction;
use crate::metering::{ComputeBudget, ComputeMeter, MeterSnapshot, SharedComputeMeter};
use crate::pda::{PdaDerivation, MAX_SEEDS, MAX_SEED_LEN};
use crate::scheduler::{ExecutionBatch, ParallelScheduler, ScheduledTransaction};
use crate::syscalls::{SyscallContext, SyscallRegistry, SyscallResult};
use crate::validation::{BytecodeValidator, ValidationConfig};
use crate::vm::{DeterministicVm, VmConfig, VmInstance};
use crate::MAX_CPI_DEPTH;

/// Execution statistics for monitoring and profiling
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionStats {
    /// Total instructions executed
    pub instructions_executed: u64,
    /// Total compute units consumed
    pub compute_consumed: u64,
    /// Total CPI invocations
    pub cpi_count: u64,
    /// Events emitted
    pub events_emitted: u64,
    /// Logs written
    pub logs_written: u64,
    /// Accounts modified
    pub accounts_modified: u64,
    /// Total execution time in microseconds
    pub execution_time_us: u64,
}

impl ExecutionStats {
    /// Record a CPI result
    pub fn record_cpi(&mut self, result: &CpiResult) {
        self.cpi_count += 1;
        self.compute_consumed += result.compute_consumed;
    }

    /// Record events and logs
    pub fn record_events(&mut self, events: &[ProgramEvent], logs: &[LogEntry]) {
        self.events_emitted += events.len() as u64;
        self.logs_written += logs.len() as u64;
    }

    /// Record account modifications
    pub fn record_deltas(&mut self, deltas: &[AccountDelta]) {
        self.accounts_modified += deltas.len() as u64;
    }

    /// Record execution timing
    pub fn record_timing(&mut self, start: Instant) {
        self.execution_time_us = start.elapsed().as_micros() as u64;
    }
}

/// Context wrapper for CPI operations
/// CPI invoker trait for WASM host functions and internal runtime callers
pub trait CpiInvoker: Send + Sync {
    /// Invoke a program with optional signer seeds originating from the caller program.
    fn invoke_signed(
        &self,
        ctx: &mut ExecutionContext,
        request: CpiInvocationRequest,
    ) -> ProgramResult<CpiInvocationResponse>;
}

/// CPI invocation request payload
#[derive(Debug, Clone)]
pub struct CpiInvocationRequest {
    /// Instruction to execute
    pub instruction: Instruction,
    /// Caller program ID (for PDA derivations and privilege checks)
    pub caller_program: Pubkey,
    /// Optional seeds for PDA signing
    pub signer_seeds: Vec<Vec<Vec<u8>>>,
}

/// CPI invocation response payload
#[derive(Debug, Clone)]
pub struct CpiInvocationResponse {
    /// Callee return data (if set)
    pub return_data: Vec<u8>,
    /// Compute consumed by callee
    pub compute_consumed: u64,
}

/// Execution batch processor using parallel scheduler
pub struct BatchProcessor {
    /// Parallel scheduler
    pub scheduler: ParallelScheduler,
    /// Pending transactions
    pub pending: Vec<ScheduledTransaction>,
}

impl BatchProcessor {
    /// Create a new batch processor
    pub fn new(scheduler: ParallelScheduler) -> Self {
        Self {
            scheduler,
            pending: Vec::new(),
        }
    }

    /// Add a transaction to the batch
    pub fn add(&mut self, tx: ScheduledTransaction) {
        self.pending.push(tx);
    }

    /// Build an execution batch from pending transactions
    pub fn build_batch(&mut self, batch_id: u64) -> ExecutionBatch {
        let transactions = std::mem::take(&mut self.pending);
        let total_priority = transactions.iter().map(|t| t.priority as u64).sum();
        ExecutionBatch {
            id: batch_id,
            transactions,
            total_priority,
        }
    }
}

/// VM instance pool for reusing compiled modules
pub struct VmInstancePool {
    /// Pool of VM instances
    instances: RwLock<Vec<VmInstance>>,
    /// Maximum pool size
    max_size: usize,
}

impl VmInstancePool {
    /// Create a new pool
    pub fn new(max_size: usize) -> Self {
        Self {
            instances: RwLock::new(Vec::with_capacity(max_size)),
            max_size,
        }
    }

    /// Get an instance from the pool or create new
    pub fn get_or_create(
        &self,
        bytecode: &[u8],
        bytecode_info: crate::validation::ValidatedBytecode,
        config: VmConfig,
    ) -> ProgramResult<VmInstance> {
        // Try to get from pool
        if let Some(instance) = self.instances.write().pop() {
            return Ok(instance);
        }
        // Create new instance
        VmInstance::new(bytecode, bytecode_info, config)
    }

    /// Return an instance to the pool
    pub fn return_instance(&self, instance: VmInstance) {
        let mut pool = self.instances.write();
        if pool.len() < self.max_size {
            pool.push(instance);
        }
    }
}

/// Validation helper using bytecode validator
pub fn validate_program_bytecode(
    bytecode: &[u8],
    _config: &ValidationConfig,
) -> ProgramResult<crate::validation::ValidatedBytecode> {
    let validator = BytecodeValidator::new();
    validator
        .validate(bytecode)
        .map_err(|e| ProgramError::InvalidBytecode(e.to_string()))
}

/// Get account info from account for CPI - creates a simplified read-only view
/// Note: For full mutable access, use the proper AccountInfo construction with RefCells
pub fn account_to_read_only_key(account: &Account) -> Pubkey {
    account.owner
}

/// Build account metas from instruction for loading
pub fn collect_account_metas(instructions: &[Instruction]) -> Vec<AccountMeta> {
    instructions
        .iter()
        .flat_map(|ix| ix.accounts.iter().cloned())
        .collect()
}

/// Create a compute budget from defaults
pub fn default_compute_budget() -> ComputeBudget {
    ComputeBudget::default()
}

/// Create return data from bytes
pub fn create_return_data(program_id: Pubkey, data: Vec<u8>) -> ReturnData {
    ReturnData { program_id, data }
}

/// Create a deterministic VM with default config
pub fn create_default_vm() -> DeterministicVm {
    DeterministicVm::new(VmConfig::default())
}

/// Program cache entry
#[derive(Debug, Clone)]
pub struct CachedProgram {
    /// Program bytecode
    pub bytecode: Vec<u8>,
    /// Program hash
    pub hash: [u8; 32],
    /// Whether executable
    pub executable: bool,
    /// Upgrade authority
    pub upgrade_authority: Option<Pubkey>,
    /// Cache timestamp
    pub cached_at: u64,
}

/// Program cache for loaded programs
pub struct ProgramCache {
    /// Cached programs
    programs: RwLock<HashMap<Pubkey, CachedProgram>>,
    /// Maximum cache size
    max_size: usize,
}

impl ProgramCache {
    /// Create new cache
    pub fn new(max_size: usize) -> Self {
        Self {
            programs: RwLock::new(HashMap::new()),
            max_size,
        }
    }

    /// Get cached program
    pub fn get(&self, program_id: &Pubkey) -> Option<CachedProgram> {
        self.programs.read().get(program_id).cloned()
    }

    /// Insert program into cache
    pub fn insert(&self, program_id: Pubkey, program: CachedProgram) {
        let mut cache = self.programs.write();

        // Evict if at capacity
        if cache.len() >= self.max_size {
            // Simple LRU: remove oldest
            if let Some(oldest) = cache.keys().next().cloned() {
                cache.remove(&oldest);
            }
        }

        cache.insert(program_id, program);
    }

    /// Invalidate cached program
    pub fn invalidate(&self, program_id: &Pubkey) {
        self.programs.write().remove(program_id);
    }

    /// Clear all cached programs
    pub fn clear(&self) {
        self.programs.write().clear();
    }
}

/// Account bank for loading and storing accounts
pub trait AccountBank: Send + Sync {
    /// Load account
    fn load(&self, pubkey: &Pubkey) -> Option<Account>;
    /// Store account
    fn store(&mut self, pubkey: Pubkey, account: Account);
    /// Load multiple accounts
    fn load_many(&self, pubkeys: &[Pubkey]) -> Vec<Option<Account>>;
    /// Store multiple accounts
    fn store_many(&mut self, accounts: Vec<(Pubkey, Account)>);
}

/// In-memory account bank for testing
#[derive(Default)]
pub struct InMemoryAccountBank {
    accounts: HashMap<Pubkey, Account>,
}

impl InMemoryAccountBank {
    /// Create a new empty account bank
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an account bank with pre-populated accounts
    pub fn with_accounts(accounts: HashMap<Pubkey, Account>) -> Self {
        Self { accounts }
    }
}

impl AccountBank for InMemoryAccountBank {
    fn load(&self, pubkey: &Pubkey) -> Option<Account> {
        self.accounts.get(pubkey).cloned()
    }

    fn store(&mut self, pubkey: Pubkey, account: Account) {
        self.accounts.insert(pubkey, account);
    }

    fn load_many(&self, pubkeys: &[Pubkey]) -> Vec<Option<Account>> {
        pubkeys.iter().map(|p| self.load(p)).collect()
    }

    fn store_many(&mut self, accounts: Vec<(Pubkey, Account)>) {
        for (pubkey, account) in accounts {
            self.store(pubkey, account);
        }
    }
}

/// Execution context for a single transaction
pub struct ExecutionContext {
    /// Transaction hash
    pub transaction_hash: [u8; 32],
    /// Current slot
    pub slot: u64,
    /// Block timestamp
    pub timestamp: u64,
    /// Compute budget
    pub compute_budget: ComputeBudget,
    /// Compute meter
    pub meter: SharedComputeMeter,
    /// Account borrow tracker
    pub borrow_tracker: AccountAccessTracker,
    /// Event collector
    pub events: EventCollector,
    /// Accounts loaded for this transaction
    pub accounts: HashMap<Pubkey, Account>,
    /// Account snapshots (for rollback)
    pub snapshots: HashMap<Pubkey, Account>,
    /// Account meta flags accumulated for this transaction (union of all metas passed)
    pub account_metas: HashMap<Pubkey, AccountMeta>,
    /// Program cache reference
    pub program_cache: Arc<ProgramCache>,
    /// Syscall registry
    pub syscalls: Arc<SyscallRegistry>,
    /// Instructions executed (completed programs for this transaction)
    pub instructions_executed: Vec<Pubkey>,
    /// Active call chain - stack of currently executing program IDs
    /// Used for reentrancy detection: if a program appears twice in this stack,
    /// it means we have A -> B -> A reentrancy which is forbidden.
    pub call_chain: Vec<Pubkey>,
    /// Return data
    pub return_data: Option<ReturnData>,
    /// CPI depth
    pub cpi_depth: usize,
    /// Fee payer
    pub fee_payer: Pubkey,
}

impl ExecutionContext {
    /// Create new execution context
    pub fn new(
        transaction_hash: [u8; 32],
        slot: u64,
        timestamp: u64,
        fee_payer: Pubkey,
        compute_budget: ComputeBudget,
        program_cache: Arc<ProgramCache>,
        syscalls: Arc<SyscallRegistry>,
    ) -> Self {
        let meter = Arc::new(ComputeMeter::new(compute_budget.clone()));
        Self {
            transaction_hash,
            slot,
            timestamp,
            compute_budget,
            meter,
            borrow_tracker: AccountAccessTracker::new(),
            events: EventCollector::new(transaction_hash, slot),
            accounts: HashMap::new(),
            snapshots: HashMap::new(),
            account_metas: HashMap::new(),
            program_cache,
            syscalls,
            instructions_executed: Vec::new(),
            call_chain: Vec::with_capacity(MAX_CPI_DEPTH + 1),
            return_data: None,
            cpi_depth: 0,
            fee_payer,
        }
    }

    /// Load accounts for transaction
    pub fn load_accounts<B: AccountBank>(
        &mut self,
        bank: &B,
        metas: &[AccountMeta],
    ) -> ProgramResult<()> {
        let pubkeys: Vec<Pubkey> = metas.iter().map(|m| m.pubkey).collect();
        let loaded = bank.load_many(&pubkeys);

        for (i, meta) in metas.iter().enumerate() {
            let account = loaded[i]
                .clone()
                .unwrap_or_else(|| Account::new(meta.pubkey));

            // Snapshot for rollback
            self.snapshots.insert(meta.pubkey, account.clone());
            self.accounts.insert(meta.pubkey, account);

            // Accumulate meta flags (union of all appearances)
            self.account_metas
                .entry(meta.pubkey)
                .and_modify(|m| {
                    m.is_signer |= meta.is_signer;
                    m.is_writable |= meta.is_writable;
                })
                .or_insert(*meta);
        }

        Ok(())
    }

    /// Get account by pubkey
    pub fn get_account(&self, pubkey: &Pubkey) -> Option<&Account> {
        self.accounts.get(pubkey)
    }

    /// Get mutable account by pubkey
    pub fn get_account_mut(&mut self, pubkey: &Pubkey) -> Option<&mut Account> {
        self.accounts.get_mut(pubkey)
    }

    /// Rollback all account changes
    pub fn rollback(&mut self) {
        for (pubkey, snapshot) in self.snapshots.drain() {
            self.accounts.insert(pubkey, snapshot);
        }
    }

    /// Compute account deltas
    pub fn compute_deltas(&self) -> Vec<AccountDelta> {
        let mut deltas = Vec::new();

        for (pubkey, current) in &self.accounts {
            if let Some(snapshot) = self.snapshots.get(pubkey) {
                let delta = AccountDelta::compute(
                    *pubkey,
                    snapshot.owner,
                    current.owner,
                    snapshot.motes,
                    current.motes,
                    snapshot.data.as_slice(),
                    current.data.as_slice(),
                );

                if delta.is_modified() {
                    deltas.push(delta);
                }
            }
        }

        deltas
    }

    /// Set return data
    pub fn set_return_data(&mut self, program_id: Pubkey, data: Vec<u8>) -> ProgramResult<()> {
        if data.len() > ReturnData::MAX_SIZE {
            return Err(ProgramError::ReturnDataTooLarge);
        }
        self.return_data = Some(ReturnData { program_id, data });
        Ok(())
    }
}

/// Program runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// VM configuration
    pub vm_config: VmConfig,
    /// Validation configuration
    pub validation_config: ValidationConfig,
    /// Default compute budget
    pub default_compute_budget: ComputeBudget,
    /// Maximum transaction accounts
    pub max_transaction_accounts: usize,
    /// Enable parallel execution
    pub parallel_execution: bool,
    /// Number of execution threads
    pub execution_threads: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            vm_config: VmConfig::default(),
            validation_config: ValidationConfig::default(),
            default_compute_budget: ComputeBudget::default(),
            max_transaction_accounts: 64,
            parallel_execution: true,
            execution_threads: 4,
        }
    }
}

/// Program runtime internal components (wrapped in Arc for sharing)
struct RuntimeComponents {
    /// Configuration
    config: RuntimeConfig,
    /// Program cache
    cache: Arc<ProgramCache>,
    /// Syscall registry
    syscalls: Arc<SyscallRegistry>,
    /// Bytecode validator
    validator: BytecodeValidator,
    /// Parallel scheduler
    scheduler: Arc<ParallelScheduler>,
}

/// Program runtime - executes transactions
pub struct ProgramRuntime {
    components: Arc<RuntimeComponents>,
}

impl ProgramRuntime {
    /// Create new runtime
    pub fn new(config: RuntimeConfig) -> Self {
        let scheduler_config = crate::scheduler::SchedulerConfig {
            max_parallelism: config.execution_threads,
            ..Default::default()
        };

        let components = RuntimeComponents {
            cache: Arc::new(ProgramCache::new(1024)),
            syscalls: Arc::new(SyscallRegistry::new()),
            validator: BytecodeValidator::with_config(config.validation_config.clone()),
            scheduler: Arc::new(ParallelScheduler::new(scheduler_config)),
            config,
        };

        Self {
            components: Arc::new(components),
        }
    }

    fn cfg(&self) -> &RuntimeConfig {
        &self.components.config
    }

    fn syscalls(&self) -> Arc<SyscallRegistry> {
        self.components.syscalls.clone()
    }

    fn cache(&self) -> Arc<ProgramCache> {
        self.components.cache.clone()
    }

    fn scheduler(&self) -> Arc<ParallelScheduler> {
        self.components.scheduler.clone()
    }

    fn validator(&self) -> &BytecodeValidator {
        &self.components.validator
    }

    /// Invoke a program via CPI, enforcing depth, privilege, and rollback semantics.
    ///
    /// This is the production-grade CPI implementation that:
    /// 1. Validates depth and reentrancy constraints
    /// 2. Derives and validates PDA signers using caller's program ID
    /// 3. Enforces privilege escalation rules
    /// 4. Enforces global borrow rules across nested calls
    /// 5. Creates state snapshots for rollback on failure
    /// 6. Executes the callee program through the same VM pathway
    /// 7. Commits or rolls back state based on execution result
    /// 8. Merges events/logs/return data into parent context
    ///
    /// CONSENSUS-CRITICAL: Compute is consumed even on failure (no refund)
    pub fn invoke_cpi(
        &self,
        ctx: &mut ExecutionContext,
        request: CpiInvocationRequest,
    ) -> ProgramResult<CpiInvocationResponse> {
        // 1. Depth check
        if ctx.cpi_depth >= MAX_CPI_DEPTH {
            return Err(ProgramError::CallDepthExceeded);
        }

        // 2. Validate instruction
        let instruction = request.instruction;
        instruction.validate()?;

        // 2.5 Bind CPI caller to the currently executing program.
        //
        // The caller program ID is consensus-critical for PDA derivation semantics.
        // We treat the active call chain as the source of truth and reject mismatches.
        let active_caller = ctx
            .call_chain
            .last()
            .copied()
            .ok_or(ProgramError::InvalidInstructionData)?;
        if request.caller_program != active_caller {
            return Err(ProgramError::InvalidInstructionData);
        }

        // 3. Reentrancy check - use the active call chain stack
        // If the callee program is already in the call chain, this is reentrancy.
        // Example: A calls B, B calls A => call_chain = [A, B], callee = A => REJECT
        // This prevents patterns like A -> B -> A which could cause state corruption.
        if ctx.call_chain.contains(&instruction.program_id) {
            return Err(ProgramError::ReentrancyDetected);
        }

        // 4. Consume base CPI cost upfront (before any other work)
        let base_cost = CPI_BASE_COST
            + (instruction.accounts.len() as u64 * CPI_PER_ACCOUNT_COST)
            + (instruction.data.len() as u64 * CPI_PER_DATA_BYTE_COST);
        ctx.meter.consume(base_cost)?;

        // 5. Track compute snapshot for accounting (NOT for rollback)
        let pre_consumed = ctx.meter.consumed();

        // 6. Preserve return data so it can be restored on failure
        let prev_return = ctx.return_data.clone();

        // 7. Pre-compute PDA signers from provided seeds (using caller's program ID)
        let mut pda_signers = std::collections::HashSet::new();
        for seed_set in &request.signer_seeds {
            // Validate seed constraints
            if seed_set.len() > MAX_SEEDS {
                return Err(ProgramError::InvalidSeeds);
            }
            for seed in seed_set {
                if seed.len() > MAX_SEED_LEN {
                    return Err(ProgramError::SeedTooLong);
                }
            }

            // Charge for PDA derivation
            ctx.meter.consume(CPI_PDA_DERIVATION_COST)?;

            let seed_refs: Vec<&[u8]> = seed_set.iter().map(|s| s.as_slice()).collect();
            let pda = PdaDerivation::find_program_address(&seed_refs, &active_caller)?;
            pda_signers.insert(pda.address);
        }

        // 8. Privilege and borrow checks; record borrows for later release
        let mut borrowed: Vec<(Pubkey, bool)> = Vec::new();

        // Ensure no privilege escalation and enforce borrow rules
        for meta in &instruction.accounts {
            // Caller must have had at least the requested privileges unless PDA signer is provided
            let caller_meta = ctx
                .account_metas
                .get(&meta.pubkey)
                .ok_or(ProgramError::AccountNotFound)?;

            // Signer check: caller had it OR PDA seeds satisfy it
            if meta.is_signer && !caller_meta.is_signer && !pda_signers.contains(&meta.pubkey) {
                return Err(ProgramError::MissingRequiredSignature);
            }

            // Writable check: must have been writable in caller's context
            if meta.is_writable && !caller_meta.is_writable {
                return Err(ProgramError::AccountNotWritable);
            }

            // Global borrow enforcement across the entire call chain
            if meta.is_writable {
                if !ctx.borrow_tracker.can_borrow_mutable(&meta.pubkey) {
                    return Err(ProgramError::BorrowsOverlap);
                }
                ctx.borrow_tracker.add_mutable_borrow(meta.pubkey)?;
                borrowed.push((meta.pubkey, true));
            } else {
                if !ctx.borrow_tracker.can_borrow_immutable(&meta.pubkey) {
                    return Err(ProgramError::BorrowsOverlap);
                }
                ctx.borrow_tracker.add_immutable_borrow(meta.pubkey)?;
                borrowed.push((meta.pubkey, false));
            }
        }

        // 9. Snapshot touched accounts for rollback
        // This is the copy-on-write mechanism for state isolation
        let mut touched_originals: HashMap<Pubkey, Account> = HashMap::new();
        for meta in &instruction.accounts {
            if meta.is_writable {
                // Only snapshot writable accounts (read-only don't need rollback)
                if let Some(account) = ctx.accounts.get(&meta.pubkey) {
                    touched_originals.insert(meta.pubkey, account.clone());
                }
            }
        }

        // 10. Push callee onto call chain and increase CPI depth
        ctx.call_chain.push(instruction.program_id);
        ctx.cpi_depth = ctx.cpi_depth.saturating_add(1);

        // 11. Checkpoint events/logs so failing CPI does not leak side effects
        let events_checkpoint = ctx.events.checkpoint();

        // 12. Execute callee program
        let result = (|| -> ProgramResult<()> {
            // Resolve program bytecode (cache or account)
            let program = match ctx.program_cache.get(&instruction.program_id) {
                Some(p) => p,
                None => {
                    let account = ctx
                        .get_account(&instruction.program_id)
                        .ok_or(ProgramError::AccountNotFound)?;
                    if !account.executable {
                        return Err(ProgramError::AccountNotExecutable);
                    }
                    CachedProgram {
                        bytecode: account.data.to_vec(),
                        hash: blake3::hash(account.data.as_slice()).into(),
                        executable: true,
                        upgrade_authority: None,
                        cached_at: ctx.slot,
                    }
                }
            };

            // Validate bytecode
            self.validator().validate(&program.bytecode)?;

            // Run the callee via the same execution pathway as top-level instructions
            self.invoke_program(
                ctx,
                &instruction.program_id,
                &program.bytecode,
                &instruction,
            )
        })();

        // 13. Pop callee from call chain and decrease depth (regardless of result)
        ctx.call_chain.pop();
        ctx.cpi_depth = ctx.cpi_depth.saturating_sub(1);

        // 14. Always release borrows (even on failure)
        for (pubkey, is_mut) in &borrowed {
            if *is_mut {
                ctx.borrow_tracker.release_mutable_borrow(pubkey);
            } else {
                ctx.borrow_tracker.release_immutable_borrow(pubkey);
            }
        }

        // 15. Handle result - commit or rollback
        match result {
            Ok(()) => {
                // SUCCESS: Callee state changes are committed (already applied)
                let post_consumed = ctx.meter.consumed();
                let compute_delta = post_consumed.saturating_sub(pre_consumed);

                // Get return data set by callee (if any), bounded
                let ret_data = ctx
                    .return_data
                    .as_ref()
                    .map(|r| {
                        if r.data.len() > MAX_CPI_RETURN_DATA {
                            r.data[..MAX_CPI_RETURN_DATA].to_vec()
                        } else {
                            r.data.clone()
                        }
                    })
                    .unwrap_or_default();

                Ok(CpiInvocationResponse {
                    return_data: ret_data,
                    compute_consumed: compute_delta,
                })
            }
            Err(e) => {
                // FAILURE: Roll back all writable account state changes
                for (pubkey, original) in touched_originals {
                    ctx.accounts.insert(pubkey, original);
                }

                // Discard any events/logs emitted during the failing CPI
                ctx.events.rollback_to(events_checkpoint);

                // IMPORTANT: Do NOT restore compute meter - compute is consumed on failure
                // This is consensus-critical for DoS prevention

                // Restore return data to parent's value
                ctx.return_data = prev_return;

                // Note: We do NOT roll back events/logs collected during this CPI
                // They are discarded implicitly by the failure (not merged to parent)

                Err(e)
            }
        }
    }

    /// Execute a single instruction
    pub fn execute_instruction<B: AccountBank>(
        &self,
        bank: &mut B,
        instruction: &Instruction,
        ctx: &mut ExecutionContext,
    ) -> ProgramResult<()> {
        // Validate instruction
        instruction.validate()?;

        // Load accounts
        ctx.load_accounts(bank, &instruction.accounts)?;

        // Get program
        let program = self.load_program(bank, &instruction.program_id)?;

        // Verify program is executable
        if !program.executable {
            return Err(ProgramError::AccountNotExecutable);
        }

        // Push program onto call chain for reentrancy tracking
        ctx.call_chain.push(instruction.program_id);

        // Execute program
        let result =
            self.invoke_program(ctx, &instruction.program_id, &program.bytecode, instruction);

        // Pop program from call chain (regardless of result)
        ctx.call_chain.pop();

        // Record program invocation on success
        if result.is_ok() {
            ctx.instructions_executed.push(instruction.program_id);
        }

        result
    }

    /// Execute a transaction (multiple instructions)
    pub fn execute_transaction<B: AccountBank>(
        &self,
        bank: &mut B,
        instructions: &[Instruction],
        transaction_hash: [u8; 32],
        slot: u64,
        timestamp: u64,
        fee_payer: Pubkey,
    ) -> ExecutionReceipt {
        let start = Instant::now();

        let mut ctx = ExecutionContext::new(
            transaction_hash,
            slot,
            timestamp,
            fee_payer,
            self.cfg().default_compute_budget.clone(),
            self.cache(),
            self.syscalls(),
        );

        // Collect all account metas
        let mut all_metas: Vec<AccountMeta> = instructions
            .iter()
            .flat_map(|ix| ix.accounts.iter().cloned())
            .collect();
        all_metas.push(AccountMeta::new(fee_payer, true)); // Fee payer

        // Load accounts
        if let Err(e) = ctx.load_accounts(bank, &all_metas) {
            return ExecutionReceipt::failure(transaction_hash, slot, &e, 0, 0, Vec::new());
        }

        // Deduct fee upfront
        let fee = self.calculate_fee(&ctx.compute_budget);
        if let Some(payer) = ctx.get_account_mut(&fee_payer) {
            if payer.motes < fee {
                return ExecutionReceipt::failure(
                    transaction_hash,
                    slot,
                    &ProgramError::InsufficientFunds,
                    0,
                    0,
                    Vec::new(),
                );
            }
            payer.motes -= fee;
        }

        // Execute each instruction
        for instruction in instructions {
            match self.execute_instruction_internal(&mut ctx, instruction) {
                Ok(()) => {}
                Err(e) => {
                    // Rollback on failure
                    ctx.rollback();

                    let (events, logs) = ctx.events.consume();
                    return ExecutionReceipt::failure(
                        transaction_hash,
                        slot,
                        &e,
                        ctx.meter.consumed(),
                        fee,
                        logs,
                    );
                }
            }
        }

        // Success - commit accounts
        let deltas = ctx.compute_deltas();
        for (pubkey, account) in ctx.accounts.drain() {
            bank.store(pubkey, account);
        }

        let (events, logs) = ctx.events.consume();

        ExecutionReceipt::success(
            transaction_hash,
            slot,
            ctx.meter.consumed(),
            fee,
            events,
            logs,
            deltas,
            ctx.return_data,
            ctx.instructions_executed,
        )
    }

    /// Execute a batch of transactions in parallel
    pub fn execute_batch<B: AccountBank + Clone>(
        &self,
        bank: &mut B,
        transactions: Vec<ScheduledTransaction>,
        slot: u64,
        timestamp: u64,
    ) -> Vec<ExecutionReceipt> {
        // Get fee payer from first instruction's first signer account
        let get_fee_payer = |tx: &ScheduledTransaction| -> Pubkey {
            tx.batch
                .account_keys
                .first()
                .copied()
                .unwrap_or(Pubkey::zero())
        };

        if !self.cfg().parallel_execution || transactions.len() < 2 {
            // Execute sequentially
            return transactions
                .into_iter()
                .map(|tx| {
                    let instructions = self.decompile_batch(&tx.batch);
                    let hash = tx.batch.hash();
                    let fee_payer = get_fee_payer(&tx);
                    self.execute_transaction(bank, &instructions, hash, slot, timestamp, fee_payer)
                })
                .collect();
        }

        // Submit all transactions for scheduling
        for tx in &transactions {
            self.scheduler().submit(tx.batch.clone(), tx.priority);
        }

        // Schedule for parallel execution
        let mut receipts = Vec::with_capacity(transactions.len());

        while let Some(batch) = self.scheduler().schedule_batch() {
            // Execute non-conflicting transactions in parallel using rayon
            // Safety: The scheduler guarantees transactions in a batch don't conflict
            // (no overlapping write sets, no read-write conflicts)
            use rayon::prelude::*;

            // Pre-load all accounts needed by this batch into a thread-safe snapshot
            let batch_accounts = self.load_batch_accounts_snapshot(bank, &batch);

            // Execute in parallel, returning both receipt and modified accounts
            let batch_results: Vec<(ExecutionReceipt, HashMap<Pubkey, Account>)> = batch
                .transactions
                .into_par_iter()
                .map(|tx| {
                    let instructions = self.decompile_batch(&tx.batch);
                    let hash = tx.batch.hash();
                    let fee_payer = get_fee_payer(&tx);
                    self.execute_transaction_with_snapshot(
                        &batch_accounts,
                        &instructions,
                        hash,
                        slot,
                        timestamp,
                        fee_payer,
                    )
                })
                .collect();

            // Commit all account mutations from successful transactions
            for (receipt, modified_accounts) in &batch_results {
                if receipt.success {
                    for (pubkey, account) in modified_accounts {
                        bank.store(*pubkey, account.clone());
                    }
                }
            }

            receipts.extend(batch_results.into_iter().map(|(r, _)| r));
        }

        receipts
    }

    /// Decompile instruction batch back to instructions
    fn decompile_batch(&self, batch: &crate::instruction::InstructionBatch) -> Vec<Instruction> {
        batch
            .instructions
            .iter()
            .map(|compiled| {
                let program_id = batch
                    .account_keys
                    .get(compiled.program_id_index as usize)
                    .copied()
                    .unwrap_or(Pubkey::zero());
                let accounts: Vec<AccountMeta> = compiled
                    .accounts
                    .iter()
                    .filter_map(|&idx| {
                        batch.account_keys.get(idx as usize).map(|&pubkey| {
                            let is_signer = batch.signer_indices.contains(&idx);
                            let is_writable = batch.writable_indices.contains(&idx);
                            AccountMeta {
                                pubkey,
                                is_signer,
                                is_writable,
                            }
                        })
                    })
                    .collect();
                Instruction {
                    program_id,
                    accounts,
                    data: compiled.data.clone(),
                }
            })
            .collect()
    }

    /// Internal instruction execution (without account loading)
    fn execute_instruction_internal(
        &self,
        ctx: &mut ExecutionContext,
        instruction: &Instruction,
    ) -> ProgramResult<()> {
        // Push program onto call chain for reentrancy tracking
        ctx.call_chain.push(instruction.program_id);

        let result = (|| {
            // Check for native program
            if self.is_native_program(&instruction.program_id) {
                return self.execute_native_program(ctx, instruction);
            }

            // Get program bytecode
            let program = match ctx.program_cache.get(&instruction.program_id) {
                Some(p) => p,
                None => {
                    // Try to load from accounts
                    let account = ctx
                        .get_account(&instruction.program_id)
                        .ok_or(ProgramError::AccountNotFound)?;

                    if !account.executable {
                        return Err(ProgramError::AccountNotExecutable);
                    }

                    CachedProgram {
                        bytecode: account.data.to_vec(),
                        hash: blake3::hash(account.data.as_slice()).into(),
                        executable: true,
                        upgrade_authority: None,
                        cached_at: ctx.slot,
                    }
                }
            };

            // Validate bytecode (From impl handles error conversion)
            self.validator().validate(&program.bytecode)?;

            // Execute in VM
            self.invoke_program(ctx, &instruction.program_id, &program.bytecode, instruction)
        })();

        // Pop program from call chain (regardless of result)
        ctx.call_chain.pop();

        // Record program invocation on success
        if result.is_ok() {
            ctx.instructions_executed.push(instruction.program_id);
        }

        result
    }

    /// Invoke a program in the VM
    fn invoke_program(
        &self,
        ctx: &mut ExecutionContext,
        program_id: &Pubkey,
        bytecode: &[u8],
        instruction: &Instruction,
    ) -> ProgramResult<()> {
        // Validate bytecode first
        let bytecode_info = self.validator().validate(bytecode)?;

        // Create VM and load module
        let vm = DeterministicVm::new(self.cfg().vm_config.clone());
        let instance = vm.load_module(bytecode, bytecode_info)?;

        // Serialize account infos for the VM
        let account_infos_data = self.serialize_account_infos(ctx, &instruction.accounts)?;

        // Execute
        let syscalls = self.syscalls();
        let _output = instance.execute(
            *program_id,
            &instruction.data,
            &account_infos_data,
            ctx.meter.clone(),
            syscalls,
        )?;

        Ok(())
    }

    /// Serialize account infos for VM consumption
    fn serialize_account_infos(
        &self,
        ctx: &ExecutionContext,
        metas: &[AccountMeta],
    ) -> ProgramResult<Vec<u8>> {
        let mut data = Vec::new();
        // Simple format: count, then for each: key(32), motes(8), data_len(4), data, owner(32), executable(1)
        data.extend_from_slice(&(metas.len() as u32).to_le_bytes());
        for meta in metas {
            if let Some(account) = ctx.accounts.get(&meta.pubkey) {
                data.extend_from_slice(account.key.as_bytes());
                data.extend_from_slice(&account.motes.to_le_bytes());
                let account_data = account.data.as_slice();
                data.extend_from_slice(&(account_data.len() as u32).to_le_bytes());
                data.extend_from_slice(account_data);
                data.extend_from_slice(account.owner.as_bytes());
                data.push(account.executable as u8);
                data.push(meta.is_signer as u8);
                data.push(meta.is_writable as u8);
            }
        }
        Ok(data)
    }

    /// Load a program from bank or cache
    fn load_program<B: AccountBank>(
        &self,
        bank: &B,
        program_id: &Pubkey,
    ) -> ProgramResult<CachedProgram> {
        // Check cache first
        if let Some(cached) = self.cache().get(program_id) {
            return Ok(cached);
        }

        // Load from bank
        let account = bank.load(program_id).ok_or(ProgramError::AccountNotFound)?;

        if !account.executable {
            return Err(ProgramError::AccountNotExecutable);
        }

        let program = CachedProgram {
            bytecode: account.data.to_vec(),
            hash: blake3::hash(account.data.as_slice()).into(),
            executable: true,
            upgrade_authority: None, // Would need to read from programdata
            cached_at: 0,
        };

        // Cache it
        self.cache().insert(*program_id, program.clone());

        Ok(program)
    }

    /// Check if program is native
    fn is_native_program(&self, program_id: &Pubkey) -> bool {
        *program_id == crate::native_programs::SYSTEM_PROGRAM_ID
            || *program_id == crate::native_programs::TOKEN_PROGRAM_ID
            || *program_id == crate::native_programs::LOADER_PROGRAM_ID
            || *program_id == crate::native_programs::ATA_PROGRAM_ID
            || *program_id == crate::native_programs::CAPABILITY_PROGRAM_ID
            || *program_id == crate::native_programs::PRIVACY_PROGRAM_ID
    }

    /// Execute native program
    fn execute_native_program(
        &self,
        ctx: &mut ExecutionContext,
        instruction: &Instruction,
    ) -> ProgramResult<()> {
        // Consume base cost
        ctx.meter.consume(100)?;

        // Collect pubkeys from instruction
        let pubkeys: Vec<Pubkey> = instruction
            .accounts
            .iter()
            .map(|meta| meta.pubkey)
            .collect();

        // Extract accounts from context - we remove them temporarily to get owned mutable access
        let mut extracted: Vec<(Pubkey, Account)> = Vec::with_capacity(pubkeys.len());
        for pk in &pubkeys {
            if let Some(account) = ctx.accounts.remove(pk) {
                extracted.push((*pk, account));
            }
        }

        // Create mutable references to the extracted accounts
        let mut account_refs: Vec<&mut Account> = extracted.iter_mut().map(|(_, a)| a).collect();
        let accounts_slice: &mut [&mut Account] = &mut account_refs;

        // Execute the appropriate native program
        let result = if instruction.program_id == crate::native_programs::SYSTEM_PROGRAM_ID {
            let ix = crate::system_program::SystemInstruction::from_bytes(&instruction.data)?;
            crate::system_program::SystemProgramProcessor::process(
                &ix,
                accounts_slice,
                ctx.meter.as_ref(),
            )
        } else if instruction.program_id == crate::native_programs::TOKEN_PROGRAM_ID {
            crate::token::TokenProgramProcessor::process(
                &instruction.data,
                accounts_slice,
                ctx.meter.as_ref(),
            )
        } else {
            Err(ProgramError::UnsupportedProgram)
        };

        // Put accounts back into context (even on failure, for rollback tracking)
        for (pk, account) in extracted {
            ctx.accounts.insert(pk, account);
        }

        result
    }

    /// Load a snapshot of accounts needed by a batch for parallel execution
    /// Returns a thread-safe map of account snapshots
    fn load_batch_accounts_snapshot<B: AccountBank>(
        &self,
        bank: &B,
        batch: &crate::scheduler::ExecutionBatch,
    ) -> Arc<parking_lot::RwLock<HashMap<Pubkey, Account>>> {
        let mut all_pubkeys = std::collections::HashSet::new();

        // Collect all account keys from all transactions in batch
        for tx in &batch.transactions {
            for pk in &tx.read_accounts {
                all_pubkeys.insert(*pk);
            }
            for pk in &tx.write_accounts {
                all_pubkeys.insert(*pk);
            }
            // Also include fee payer (first account key)
            if let Some(pk) = tx.batch.account_keys.first() {
                all_pubkeys.insert(*pk);
            }
        }

        // Load all accounts
        let pubkey_vec: Vec<Pubkey> = all_pubkeys.into_iter().collect();
        let loaded = bank.load_many(&pubkey_vec);

        let mut accounts = HashMap::new();
        for (i, pk) in pubkey_vec.iter().enumerate() {
            let account = loaded[i].clone().unwrap_or_else(|| Account::new(*pk));
            accounts.insert(*pk, account);
        }

        Arc::new(parking_lot::RwLock::new(accounts))
    }

    /// Execute a transaction using a pre-loaded account snapshot (for parallel execution)
    /// Returns both the execution receipt and the modified accounts for commit
    fn execute_transaction_with_snapshot(
        &self,
        accounts_snapshot: &Arc<parking_lot::RwLock<HashMap<Pubkey, Account>>>,
        instructions: &[Instruction],
        transaction_hash: [u8; 32],
        slot: u64,
        timestamp: u64,
        fee_payer: Pubkey,
    ) -> (ExecutionReceipt, HashMap<Pubkey, Account>) {
        let start = Instant::now();

        let mut ctx = ExecutionContext::new(
            transaction_hash,
            slot,
            timestamp,
            fee_payer,
            self.cfg().default_compute_budget.clone(),
            self.cache(),
            self.syscalls(),
        );

        // Collect all account metas
        let mut all_metas: Vec<AccountMeta> = instructions
            .iter()
            .flat_map(|ix| ix.accounts.iter().cloned())
            .collect();
        all_metas.push(AccountMeta::new(fee_payer, true));

        // Load accounts from snapshot
        {
            let snapshot = accounts_snapshot.read();
            for meta in &all_metas {
                let account = snapshot
                    .get(&meta.pubkey)
                    .cloned()
                    .unwrap_or_else(|| Account::new(meta.pubkey));

                ctx.snapshots.insert(meta.pubkey, account.clone());
                ctx.accounts.insert(meta.pubkey, account);
            }
        }

        // Deduct fee upfront
        let fee = self.calculate_fee(&ctx.compute_budget);
        if let Some(payer) = ctx.get_account_mut(&fee_payer) {
            if payer.motes < fee {
                return (
                    ExecutionReceipt::failure(
                        transaction_hash,
                        slot,
                        &ProgramError::InsufficientFunds,
                        0,
                        0,
                        Vec::new(),
                    ),
                    HashMap::new(),
                );
            }
            payer.motes -= fee;
        }

        // Execute each instruction
        for instruction in instructions {
            match self.execute_instruction_internal(&mut ctx, instruction) {
                Ok(()) => {}
                Err(e) => {
                    ctx.rollback();
                    let (_, logs) = ctx.events.consume();
                    return (
                        ExecutionReceipt::failure(
                            transaction_hash,
                            slot,
                            &e,
                            ctx.meter.consumed(),
                            fee,
                            logs,
                        ),
                        HashMap::new(),
                    );
                }
            }
        }

        // Success - compute deltas and collect modified accounts
        let deltas = ctx.compute_deltas();
        let (events, logs) = ctx.events.consume();
        let modified_accounts = ctx.accounts.clone();

        let receipt = ExecutionReceipt::success(
            transaction_hash,
            slot,
            ctx.meter.consumed(),
            fee,
            events,
            logs,
            deltas,
            ctx.return_data,
            ctx.instructions_executed,
        );

        (receipt, modified_accounts)
    }

    /// Calculate transaction fee
    fn calculate_fee(&self, budget: &ComputeBudget) -> u64 {
        // Base fee + compute unit fee
        let base_fee = 5000u64;
        let compute_fee = budget.max_units / 1000;
        base_fee + compute_fee
    }

    /// Invalidate program cache entry
    pub fn invalidate_cache(&self, program_id: &Pubkey) {
        self.cache().invalidate(program_id);
    }

    /// Clear entire program cache
    pub fn clear_cache(&self) {
        self.cache().clear();
    }
}

impl CpiInvoker for ProgramRuntime {
    fn invoke_signed(
        &self,
        ctx: &mut ExecutionContext,
        request: CpiInvocationRequest,
    ) -> ProgramResult<CpiInvocationResponse> {
        self.invoke_cpi(ctx, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_execution_context_creation() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        assert_eq!(ctx.slot, 100);
        assert_eq!(ctx.cpi_depth, 0);
    }

    #[test]
    fn test_in_memory_bank() {
        let mut bank = InMemoryAccountBank::new();
        let pubkey = Pubkey::new([1u8; 32]);

        let account = Account {
            key: pubkey,
            motes: 1000,
            data: AccountData::new(vec![1, 2, 3]),
            owner: Pubkey::new([2u8; 32]),
            executable: false,
            rent_epoch: 0,
            state: AccountState::Initialized,
        };

        bank.store(pubkey, account.clone());

        let loaded = bank.load(&pubkey);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().motes, 1000);
    }

    #[test]
    fn test_program_cache() {
        let cache = ProgramCache::new(10);
        let program_id = Pubkey::new([1u8; 32]);

        let program = CachedProgram {
            bytecode: vec![1, 2, 3],
            hash: [0u8; 32],
            executable: true,
            upgrade_authority: None,
            cached_at: 0,
        };

        cache.insert(program_id, program.clone());

        let cached = cache.get(&program_id);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().bytecode, vec![1, 2, 3]);
    }

    #[test]
    fn test_runtime_creation() {
        let config = RuntimeConfig::default();
        let runtime = ProgramRuntime::new(config);

        // Should have native programs registered
        assert!(runtime.is_native_program(&crate::native_programs::SYSTEM_PROGRAM_ID));
        assert!(runtime.is_native_program(&crate::native_programs::TOKEN_PROGRAM_ID));
    }

    #[test]
    fn test_fee_calculation() {
        let config = RuntimeConfig::default();
        let runtime = ProgramRuntime::new(config);

        let budget = ComputeBudget::new(100000);
        let fee = runtime.calculate_fee(&budget);

        // Base fee + compute fee
        assert!(fee >= 5000);
    }

    #[test]
    fn test_account_deltas() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        // Add account with snapshot
        let pubkey = Pubkey::new([2u8; 32]);
        let account = Account {
            key: pubkey,
            motes: 1000,
            data: AccountData::new(vec![1, 2, 3]),
            owner: Pubkey::new([3u8; 32]),
            executable: false,
            rent_epoch: 0,
            state: AccountState::Initialized,
        };

        ctx.snapshots.insert(pubkey, account.clone());

        // Modify account
        let mut modified = account.clone();
        modified.motes = 500;
        ctx.accounts.insert(pubkey, modified);

        let deltas = ctx.compute_deltas();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].prev_motes, 1000);
        assert_eq!(deltas[0].new_motes, 500);
    }

    #[test]
    fn test_execution_rollback() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        let pubkey = Pubkey::new([2u8; 32]);
        let original = Account {
            key: pubkey,
            motes: 1000,
            data: AccountData::new(vec![1, 2, 3]),
            owner: Pubkey::new([3u8; 32]),
            executable: false,
            rent_epoch: 0,
            state: AccountState::Initialized,
        };

        ctx.snapshots.insert(pubkey, original.clone());

        // Modify
        let mut modified = original.clone();
        modified.motes = 0;
        ctx.accounts.insert(pubkey, modified);

        // Rollback
        ctx.rollback();

        // Should be back to original
        let account = ctx.get_account(&pubkey).unwrap();
        assert_eq!(account.motes, 1000);
    }

    #[test]
    fn test_call_chain_initialization() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        // Call chain should be empty initially
        assert!(ctx.call_chain.is_empty());
        assert_eq!(ctx.cpi_depth, 0);
    }

    #[test]
    fn test_call_chain_capacity() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        // Call chain should have capacity for MAX_CPI_DEPTH + 1
        assert!(ctx.call_chain.capacity() >= crate::MAX_CPI_DEPTH + 1);
    }

    #[test]
    fn test_call_chain_push_pop() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        let program_a = Pubkey::new([1u8; 32]);
        let program_b = Pubkey::new([2u8; 32]);
        let program_c = Pubkey::new([3u8; 32]);

        // Simulate call chain: A -> B -> C
        ctx.call_chain.push(program_a);
        assert!(ctx.call_chain.contains(&program_a));
        assert!(!ctx.call_chain.contains(&program_b));

        ctx.call_chain.push(program_b);
        assert!(ctx.call_chain.contains(&program_a));
        assert!(ctx.call_chain.contains(&program_b));

        ctx.call_chain.push(program_c);
        assert_eq!(ctx.call_chain.len(), 3);
        assert!(ctx.call_chain.contains(&program_c));

        // Pop C
        ctx.call_chain.pop();
        assert_eq!(ctx.call_chain.len(), 2);
        assert!(!ctx.call_chain.contains(&program_c));
        assert!(ctx.call_chain.contains(&program_b));

        // Pop B
        ctx.call_chain.pop();
        assert_eq!(ctx.call_chain.len(), 1);
        assert!(!ctx.call_chain.contains(&program_b));
        assert!(ctx.call_chain.contains(&program_a));

        // Pop A
        ctx.call_chain.pop();
        assert!(ctx.call_chain.is_empty());
    }

    #[test]
    fn test_reentrancy_detection_via_call_chain() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        let program_a = Pubkey::new([1u8; 32]);
        let program_b = Pubkey::new([2u8; 32]);

        // Simulate A calling B
        ctx.call_chain.push(program_a);
        ctx.call_chain.push(program_b);

        // Now if B tries to call A, that's reentrancy
        assert!(ctx.call_chain.contains(&program_a));
        // This would be detected by the reentrancy check in invoke_cpi

        // B calling C (not in chain) is OK
        let program_c = Pubkey::new([3u8; 32]);
        assert!(!ctx.call_chain.contains(&program_c));
    }

    #[test]
    fn test_self_reentrancy_detection() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        let program_a = Pubkey::new([1u8; 32]);

        // A is executing
        ctx.call_chain.push(program_a);

        // A trying to call itself is reentrancy
        assert!(ctx.call_chain.contains(&program_a));
    }

    #[test]
    fn test_transitive_reentrancy_detection() {
        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([1u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        let program_a = Pubkey::new([1u8; 32]);
        let program_b = Pubkey::new([2u8; 32]);
        let program_c = Pubkey::new([3u8; 32]);

        // Simulate A -> B -> C
        ctx.call_chain.push(program_a);
        ctx.call_chain.push(program_b);
        ctx.call_chain.push(program_c);

        // C trying to call A is transitive reentrancy (A -> B -> C -> A)
        assert!(ctx.call_chain.contains(&program_a));

        // C trying to call B is also reentrancy (A -> B -> C -> B)
        assert!(ctx.call_chain.contains(&program_b));

        // C trying to call D is OK
        let program_d = Pubkey::new([4u8; 32]);
        assert!(!ctx.call_chain.contains(&program_d));
    }

    #[test]
    fn test_cpi_rejects_forged_caller_program() {
        let config = RuntimeConfig::default();
        let runtime = ProgramRuntime::new(config);

        let cache = Arc::new(ProgramCache::new(100));
        let syscalls = Arc::new(SyscallRegistry::new());
        let fee_payer = Pubkey::new([9u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            cache,
            syscalls,
        );

        let real_caller = Pubkey::new([1u8; 32]);
        let forged_caller = Pubkey::new([2u8; 32]);
        let callee = Pubkey::new([3u8; 32]);

        // Simulate that we're currently executing real_caller
        ctx.call_chain.push(real_caller);

        let request = CpiInvocationRequest {
            instruction: Instruction {
                program_id: callee,
                accounts: vec![],
                data: vec![],
            },
            caller_program: forged_caller,
            signer_seeds: vec![],
        };

        let result = runtime.invoke_cpi(&mut ctx, request);
        assert!(matches!(result, Err(ProgramError::InvalidInstructionData)));
    }
}
