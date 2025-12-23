//! Program runtime - ties together VM, accounts, metering, and execution

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::account::{
    Account, AccountAccessTracker, AccountData, AccountInfo, AccountMeta, AccountState, Pubkey,
};
use crate::cpi::{CpiContext, CpiResult, CrossProgramInvocation};
use crate::error::{ProgramError, ProgramResult};
use crate::events::{
    AccountDelta, EventCollector, ExecutionReceipt, LogEntry, ProgramEvent, ReturnData,
};
use crate::instruction::Instruction;
use crate::metering::{ComputeBudget, ComputeMeter, SharedComputeMeter};
use crate::scheduler::{ExecutionBatch, ParallelScheduler, ScheduledTransaction};
use crate::syscalls::{SyscallContext, SyscallRegistry, SyscallResult};
use crate::validation::{BytecodeValidator, ValidationConfig};
use crate::vm::{DeterministicVm, VmConfig, VmInstance};

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
pub struct CpiExecutor<'a, 'b> {
    /// The CPI context
    pub ctx: CpiContext<'a, 'b>,
}

impl<'a, 'b> CpiExecutor<'a, 'b> {
    /// Execute a CPI and track the result
    pub fn invoke(
        &mut self,
        instruction: &Instruction,
        stats: &mut ExecutionStats,
    ) -> ProgramResult<SyscallResult> {
        let result = CrossProgramInvocation::invoke(&mut self.ctx, instruction)?;
        stats.record_cpi(&result);
        Ok(SyscallResult::OkBytes(result.return_data))
    }
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
pub struct ExecutionContext<'a> {
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
    /// Program cache reference
    pub program_cache: &'a ProgramCache,
    /// Syscall registry
    pub syscalls: &'a SyscallRegistry,
    /// Instructions executed
    pub instructions_executed: Vec<Pubkey>,
    /// Return data
    pub return_data: Option<ReturnData>,
    /// CPI depth
    pub cpi_depth: usize,
    /// Fee payer
    pub fee_payer: Pubkey,
}

impl<'a> ExecutionContext<'a> {
    /// Create new execution context
    pub fn new(
        transaction_hash: [u8; 32],
        slot: u64,
        timestamp: u64,
        fee_payer: Pubkey,
        compute_budget: ComputeBudget,
        program_cache: &'a ProgramCache,
        syscalls: &'a SyscallRegistry,
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
            program_cache,
            syscalls,
            instructions_executed: Vec::new(),
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

/// Program runtime - executes transactions
pub struct ProgramRuntime {
    /// Configuration
    config: RuntimeConfig,
    /// Program cache
    cache: ProgramCache,
    /// Syscall registry
    syscalls: SyscallRegistry,
    /// Bytecode validator
    validator: BytecodeValidator,
    /// Parallel scheduler
    scheduler: ParallelScheduler,
}

impl ProgramRuntime {
    /// Create new runtime
    pub fn new(config: RuntimeConfig) -> Self {
        let scheduler_config = crate::scheduler::SchedulerConfig {
            max_parallelism: config.execution_threads,
            ..Default::default()
        };
        Self {
            cache: ProgramCache::new(1024),
            syscalls: SyscallRegistry::new(),
            validator: BytecodeValidator::with_config(config.validation_config.clone()),
            scheduler: ParallelScheduler::new(scheduler_config),
            config,
        }
    }

    /// Execute a single instruction
    pub fn execute_instruction<B: AccountBank>(
        &self,
        bank: &mut B,
        instruction: &Instruction,
        ctx: &mut ExecutionContext<'_>,
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

        // Execute program
        self.invoke_program(ctx, &instruction.program_id, &program.bytecode, instruction)?;

        // Record program invocation
        ctx.instructions_executed.push(instruction.program_id);

        Ok(())
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
            self.config.default_compute_budget.clone(),
            &self.cache,
            &self.syscalls,
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

        if !self.config.parallel_execution || transactions.len() < 2 {
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
            self.scheduler.submit(tx.batch.clone(), tx.priority);
        }

        // Schedule for parallel execution
        let mut receipts = Vec::with_capacity(transactions.len());

        while let Some(batch) = self.scheduler.schedule_batch() {
            // Execute non-conflicting transactions in parallel
            // For now, still sequential within batch for safety
            for tx in batch.transactions {
                let instructions = self.decompile_batch(&tx.batch);
                let hash = tx.batch.hash();
                let fee_payer = get_fee_payer(&tx);
                let receipt =
                    self.execute_transaction(bank, &instructions, hash, slot, timestamp, fee_payer);
                receipts.push(receipt);
            }
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
        ctx: &mut ExecutionContext<'_>,
        instruction: &Instruction,
    ) -> ProgramResult<()> {
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
        self.validator.validate(&program.bytecode)?;

        // Execute in VM
        self.invoke_program(ctx, &instruction.program_id, &program.bytecode, instruction)
    }

    /// Invoke a program in the VM
    fn invoke_program(
        &self,
        ctx: &mut ExecutionContext<'_>,
        program_id: &Pubkey,
        bytecode: &[u8],
        instruction: &Instruction,
    ) -> ProgramResult<()> {
        // Validate bytecode first
        let bytecode_info = self.validator.validate(bytecode)?;

        // Create VM and load module
        let vm = DeterministicVm::new(self.config.vm_config.clone());
        let instance = vm.load_module(bytecode, bytecode_info)?;

        // Serialize account infos for the VM
        let account_infos_data = self.serialize_account_infos(ctx, &instruction.accounts)?;

        // Execute
        let syscalls = Arc::new(self.syscalls.clone());
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
        ctx: &ExecutionContext<'_>,
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
        if let Some(cached) = self.cache.get(program_id) {
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
        self.cache.insert(*program_id, program.clone());

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
        ctx: &mut ExecutionContext<'_>,
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

    /// Calculate transaction fee
    fn calculate_fee(&self, budget: &ComputeBudget) -> u64 {
        // Base fee + compute unit fee
        let base_fee = 5000u64;
        let compute_fee = budget.max_units / 1000;
        base_fee + compute_fee
    }

    /// Invalidate program cache entry
    pub fn invalidate_cache(&self, program_id: &Pubkey) {
        self.cache.invalidate(program_id);
    }

    /// Clear entire program cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_context_creation() {
        let cache = ProgramCache::new(100);
        let syscalls = SyscallRegistry::new();
        let fee_payer = Pubkey::new([1u8; 32]);

        let ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            &cache,
            &syscalls,
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
        let cache = ProgramCache::new(100);
        let syscalls = SyscallRegistry::new();
        let fee_payer = Pubkey::new([1u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            &cache,
            &syscalls,
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
        let cache = ProgramCache::new(100);
        let syscalls = SyscallRegistry::new();
        let fee_payer = Pubkey::new([1u8; 32]);

        let mut ctx = ExecutionContext::new(
            [0u8; 32],
            100,
            1000000,
            fee_payer,
            ComputeBudget::default(),
            &cache,
            &syscalls,
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
}
