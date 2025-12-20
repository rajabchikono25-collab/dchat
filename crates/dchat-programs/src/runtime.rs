//! Program runtime - ties together VM, accounts, metering, and execution

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::account::{Account, AccountAccessTracker, AccountInfo, AccountMeta, Pubkey};
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
    pub fn new() -> Self {
        Self::default()
    }

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
            let account = loaded[i].clone().unwrap_or_else(|| Account {
                key: meta.pubkey,
                lamports: 0,
                data: Vec::new(),
                owner: crate::native_programs::SYSTEM_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            });

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
                    snapshot.lamports,
                    current.lamports,
                    &snapshot.data,
                    &current.data,
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
        Self {
            cache: ProgramCache::new(1024),
            syscalls: SyscallRegistry::new(),
            validator: BytecodeValidator::new(config.validation_config.clone()),
            scheduler: ParallelScheduler::new(config.execution_threads),
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
        let result =
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
            if payer.lamports < fee {
                return ExecutionReceipt::failure(
                    transaction_hash,
                    slot,
                    &ProgramError::InsufficientFunds,
                    0,
                    0,
                    Vec::new(),
                );
            }
            payer.lamports -= fee;
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
        if !self.config.parallel_execution || transactions.len() < 2 {
            // Execute sequentially
            return transactions
                .into_iter()
                .map(|tx| {
                    self.execute_transaction(
                        bank,
                        &tx.instructions,
                        tx.hash,
                        slot,
                        timestamp,
                        tx.fee_payer,
                    )
                })
                .collect();
        }

        // Schedule for parallel execution
        let batches = self.scheduler.schedule_batch(&transactions);
        let mut receipts = Vec::with_capacity(transactions.len());

        for batch in batches {
            // Execute non-conflicting transactions in parallel
            // For now, still sequential within batch for safety
            for tx in batch.transactions {
                let receipt = self.execute_transaction(
                    bank,
                    &tx.instructions,
                    tx.hash,
                    slot,
                    timestamp,
                    tx.fee_payer,
                );
                receipts.push(receipt);
            }
        }

        receipts
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
                    bytecode: account.data.clone(),
                    hash: blake3::hash(&account.data).into(),
                    executable: true,
                    upgrade_authority: None,
                    cached_at: ctx.slot,
                }
            }
        };

        // Validate bytecode
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
        // Create VM instance
        let vm = DeterministicVm::new(self.config.vm_config.clone());
        let mut instance = vm.instantiate(bytecode)?;

        // Prepare account infos
        let account_infos: Vec<AccountInfo<'_>> = instruction
            .accounts
            .iter()
            .filter_map(|meta| {
                ctx.accounts
                    .get(&meta.pubkey)
                    .map(|acc| AccountInfo::from_account(acc, meta.is_signer, meta.is_writable))
            })
            .collect();

        // Execute
        instance.execute(&instruction.data, &account_infos, ctx.meter.as_ref())?;

        Ok(())
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
            bytecode: account.data.clone(),
            hash: blake3::hash(&account.data).into(),
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

        if instruction.program_id == crate::native_programs::SYSTEM_PROGRAM_ID {
            let ix = crate::system_program::SystemInstruction::from_bytes(&instruction.data)?;

            let mut accounts: Vec<&mut Account> = instruction
                .accounts
                .iter()
                .filter_map(|meta| ctx.accounts.get_mut(&meta.pubkey))
                .collect();

            crate::system_program::SystemProgramProcessor::process(
                &ix,
                &mut accounts.iter_mut().collect::<Vec<_>>(),
                ctx.meter.as_ref(),
            )
        } else if instruction.program_id == crate::native_programs::TOKEN_PROGRAM_ID {
            let mut accounts: Vec<&mut Account> = instruction
                .accounts
                .iter()
                .filter_map(|meta| ctx.accounts.get_mut(&meta.pubkey))
                .collect();

            crate::token::TokenProgramProcessor::process(
                &instruction.data,
                &mut accounts.iter_mut().collect::<Vec<_>>(),
                ctx.meter.as_ref(),
            )
        } else {
            Err(ProgramError::UnsupportedProgram)
        }
    }

    /// Calculate transaction fee
    fn calculate_fee(&self, budget: &ComputeBudget) -> u64 {
        // Base fee + compute unit fee
        let base_fee = 5000u64;
        let compute_fee = budget.compute_units / 1000;
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
            lamports: 1000,
            data: vec![1, 2, 3],
            owner: Pubkey::new([2u8; 32]),
            executable: false,
            rent_epoch: 0,
        };

        bank.store(pubkey, account.clone());

        let loaded = bank.load(&pubkey);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().lamports, 1000);
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
            lamports: 1000,
            data: vec![1, 2, 3],
            owner: Pubkey::new([3u8; 32]),
            executable: false,
            rent_epoch: 0,
        };

        ctx.snapshots.insert(pubkey, account.clone());

        // Modify account
        let mut modified = account.clone();
        modified.lamports = 500;
        ctx.accounts.insert(pubkey, modified);

        let deltas = ctx.compute_deltas();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].prev_lamports, 1000);
        assert_eq!(deltas[0].new_lamports, 500);
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
            lamports: 1000,
            data: vec![1, 2, 3],
            owner: Pubkey::new([3u8; 32]),
            executable: false,
            rent_epoch: 0,
        };

        ctx.snapshots.insert(pubkey, original.clone());

        // Modify
        let mut modified = original.clone();
        modified.lamports = 0;
        ctx.accounts.insert(pubkey, modified);

        // Rollback
        ctx.rollback();

        // Should be back to original
        let account = ctx.get_account(&pubkey).unwrap();
        assert_eq!(account.lamports, 1000);
    }
}
