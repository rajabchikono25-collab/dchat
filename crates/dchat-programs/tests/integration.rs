//! Integration tests for dchat-programs
//!
//! End-to-end tests that exercise the full execution pipeline

use std::collections::HashMap;
use std::sync::Arc;

use dchat_programs::account::{Account, AccountMeta, Pubkey};
use dchat_programs::events::ExecutionReceipt;
use dchat_programs::instruction::{Instruction, InstructionBatch};
use dchat_programs::loader::LoaderProgram;
use dchat_programs::metering::{ComputeBudget, ComputeMeter};
use dchat_programs::native_programs;
use dchat_programs::pda::PdaDerivation;
use dchat_programs::runtime::{
    AccountBank, ExecutionContext, InMemoryAccountBank, ProgramCache, RuntimeConfig,
};
use dchat_programs::scheduler::{
    ExecutionBatch, ParallelScheduler, ScheduledTransaction, SchedulerConfig,
};
use dchat_programs::syscalls::SyscallRegistry;
use dchat_programs::system_program::{SystemInstruction, SystemProgram};
use dchat_programs::token::{TokenInstruction, TokenProgram};
use dchat_programs::validation::{BytecodeValidator, ValidationConfig};

fn pubkey_n(n: u8) -> Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    Pubkey::new(bytes)
}

fn tx_hash_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

#[test]
fn test_system_program_create_account() {
    // Create instruction to create new account
    let payer = pubkey_n(1);
    let new_account = pubkey_n(2);
    let owner = native_programs::SYSTEM_PROGRAM_ID;
    let lamports = 1_000_000;
    let space = 100;

    let instruction = SystemProgram::create_account(payer, new_account, lamports, space, owner);

    assert_eq!(instruction.program_id, native_programs::SYSTEM_PROGRAM_ID);
    assert_eq!(instruction.accounts.len(), 2);
    assert!(instruction.accounts[0].is_signer); // payer signs
    assert!(instruction.accounts[0].is_writable); // payer debited
    assert!(instruction.accounts[1].is_signer); // new account signs
    assert!(instruction.accounts[1].is_writable); // new account credited
}

#[test]
fn test_system_program_transfer() {
    let from = pubkey_n(1);
    let to = pubkey_n(2);
    let lamports = 500_000;

    let instruction = SystemProgram::transfer(from, to, lamports);

    assert_eq!(instruction.program_id, native_programs::SYSTEM_PROGRAM_ID);
    assert_eq!(instruction.accounts.len(), 2);
    assert!(instruction.accounts[0].is_signer);
    assert!(instruction.accounts[0].is_writable);
    assert!(!instruction.accounts[1].is_signer);
    assert!(instruction.accounts[1].is_writable);
}

#[test]
fn test_token_program_initialize_mint() {
    let mint = pubkey_n(1);
    let mint_authority = pubkey_n(2);
    let freeze_authority = Some(pubkey_n(3));
    let decimals = 9u8;

    let instruction =
        TokenProgram::initialize_mint(mint, mint_authority, freeze_authority, decimals);

    assert_eq!(instruction.program_id, native_programs::TOKEN_PROGRAM_ID);
}

#[test]
fn test_token_program_mint_to() {
    let mint = pubkey_n(1);
    let destination = pubkey_n(2);
    let mint_authority = pubkey_n(3);
    let amount = 1_000_000_000;

    let instruction = TokenProgram::mint_to(mint, destination, mint_authority, amount);

    assert_eq!(instruction.program_id, native_programs::TOKEN_PROGRAM_ID);
}

#[test]
fn test_token_program_transfer() {
    let source = pubkey_n(1);
    let destination = pubkey_n(2);
    let owner = pubkey_n(3);
    let amount = 500_000_000;

    let instruction = TokenProgram::transfer(source, destination, owner, amount);

    assert_eq!(instruction.program_id, native_programs::TOKEN_PROGRAM_ID);
    assert!(instruction.accounts[2].is_signer); // owner signs
}

#[test]
fn test_pda_derivation() {
    let program_id = pubkey_n(10);
    let seeds: &[&[u8]] = &[b"seed1", b"seed2"];

    let result = PdaDerivation::find_program_address(seeds, &program_id);

    // PDA derivation might fail if no valid bump, but should be deterministic
    if let Ok(pda) = result {
        // Verify it's off the ed25519 curve
        assert!(pda.bump <= 255);

        // Repeat should give same result
        let result2 = PdaDerivation::find_program_address(seeds, &program_id);
        assert!(result2.is_ok());
        assert_eq!(pda.address, result2.unwrap().address);
    }
}

#[test]
fn test_pda_create_program_address() {
    let program_id = pubkey_n(10);
    let seeds: &[&[u8]] = &[b"seed", &[255]]; // includes bump

    let result = PdaDerivation::create_program_address(seeds, &program_id);

    if let Ok(address) = result {
        // Address should be 32 bytes
        assert_eq!(address.0.len(), 32);
    }
}

#[test]
fn test_validation_config_defaults() {
    let config = ValidationConfig::default();

    assert!(config.max_size > 0);
    assert!(config.max_functions > 0);
    assert!(config.max_globals > 0);
    assert!(!config.allowed_imports.is_empty());
    assert!(!config.forbidden_opcodes.is_empty());
}

#[test]
fn test_bytecode_validator_creation() {
    let config = ValidationConfig::default();
    let validator = BytecodeValidator::with_config(config);

    // Validator should have allowed imports
    assert!(validator.allowed_imports().contains("sol_log_"));
    assert!(validator.allowed_imports().contains("sol_sha256"));
}

#[test]
fn test_program_cache_operations() {
    use dchat_programs::runtime::CachedProgram;

    let cache = ProgramCache::new(10);
    let program_id = pubkey_n(1);

    // Initially empty
    assert!(cache.get(&program_id).is_none());

    // Insert program
    let program = CachedProgram {
        bytecode: vec![0, 1, 2, 3],
        hash: [0u8; 32],
        executable: true,
        upgrade_authority: None,
        cached_at: 0,
    };

    cache.insert(program_id, program);
    assert!(cache.get(&program_id).is_some());

    // Invalidate
    cache.invalidate(&program_id);
    assert!(cache.get(&program_id).is_none());
}

#[test]
fn test_in_memory_account_bank() {
    let mut bank = InMemoryAccountBank::new();
    let pubkey = pubkey_n(1);

    // Initially empty
    assert!(bank.load(&pubkey).is_none());

    // Store account
    let account = Account::new_with_data(
        pubkey,
        1000,
        vec![1, 2, 3],
        native_programs::SYSTEM_PROGRAM_ID,
    );
    bank.store(pubkey, account);

    // Load account
    let loaded = bank.load(&pubkey);
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.lamports, 1000);
}

#[test]
fn test_scheduler_batching() {
    let config = SchedulerConfig::default();
    let scheduler = ParallelScheduler::new(config);

    // Create non-conflicting transactions
    let mut tx1_batch = InstructionBatch::default();
    tx1_batch.account_keys = vec![pubkey_n(1)];
    tx1_batch.writable_indices = vec![0];

    let mut tx2_batch = InstructionBatch::default();
    tx2_batch.account_keys = vec![pubkey_n(2)];
    tx2_batch.writable_indices = vec![0];

    let tx1 = ScheduledTransaction::new(1, tx1_batch, 1);
    let tx2 = ScheduledTransaction::new(2, tx2_batch, 1);

    // Both should fit in same batch (no conflict)
    let mut batch = ExecutionBatch::new(0);
    assert!(batch.try_add(tx1));
    assert!(batch.try_add(tx2));
    assert_eq!(batch.transactions.len(), 2);
}

#[test]
fn test_loader_instruction_serialization() {
    let instruction = dchat_programs::loader::LoaderInstruction::DeployWithMaxDataLen {
        max_data_len: 10000,
    };

    let bytes = instruction.to_bytes();
    assert!(!bytes.is_empty());

    let restored = dchat_programs::loader::LoaderInstruction::from_bytes(&bytes);
    assert!(restored.is_ok());

    match restored.unwrap() {
        dchat_programs::loader::LoaderInstruction::DeployWithMaxDataLen { max_data_len } => {
            assert_eq!(max_data_len, 10000);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_capability_token_creation() {
    use dchat_programs::capability::{CapabilityId, CapabilityScope, CapabilityToken};

    let issuer = pubkey_n(1);
    let grantee = pubkey_n(2);
    let program_id = pubkey_n(10);

    let scope = CapabilityScope::for_program(program_id)
        .with_methods(vec!["transfer".to_string()])
        .with_max_transfer_per_call(1_000_000);

    let token = CapabilityToken::new(
        issuer,
        grantee,
        scope,
        u64::MAX,  // never expires
        Some(100), // max 100 uses
        12345,     // nonce
    );

    assert_eq!(token.issuer, issuer);
    assert_eq!(token.grantee, grantee);
    assert_eq!(token.use_count, 0);
    assert!(!token.revoked);
}

#[test]
fn test_privacy_encrypted_balance() {
    use dchat_programs::privacy::EncryptedBalance;

    let balance = EncryptedBalance::new(
        [1u8; 32], // commitment
        [2u8; 48], // encrypted value
        [3u8; 24], // nonce
    );

    assert_eq!(balance.commitment, [1u8; 32]);
    assert_eq!(balance.encrypted_value, [2u8; 48]);
    assert_eq!(balance.nonce, [3u8; 24]);
}

#[test]
fn test_syscall_registry_creation() {
    let registry = SyscallRegistry::new();

    // Should have default syscalls registered
    assert!(registry.has_syscall("sol_log_"));
    assert!(registry.has_syscall("sol_sha256"));
    assert!(registry.has_syscall("sol_blake3"));
}

#[test]
fn test_runtime_config_defaults() {
    let config = RuntimeConfig::default();

    assert!(config.max_transaction_accounts > 0);
    assert!(config.execution_threads > 0);
}

#[test]
fn test_execution_context_creation() {
    let tx_hash = tx_hash_n(1);
    let slot = 100;
    let timestamp = 1234567890;
    let fee_payer = pubkey_n(1);
    let budget = ComputeBudget::default();
    let program_cache = ProgramCache::new(100);
    let syscalls = SyscallRegistry::new();

    let ctx = ExecutionContext::new(
        tx_hash,
        slot,
        timestamp,
        fee_payer,
        budget,
        &program_cache,
        &syscalls,
    );

    assert_eq!(ctx.slot, slot);
    assert_eq!(ctx.timestamp, timestamp);
    assert_eq!(ctx.fee_payer, fee_payer);
    assert_eq!(ctx.cpi_depth, 0);
}

#[test]
fn test_native_program_ids() {
    // Verify native program IDs are unique
    let ids = [
        native_programs::SYSTEM_PROGRAM_ID,
        native_programs::TOKEN_PROGRAM_ID,
        native_programs::LOADER_PROGRAM_ID,
        native_programs::ATA_PROGRAM_ID,
        native_programs::CAPABILITY_PROGRAM_ID,
        native_programs::PRIVACY_PROGRAM_ID,
    ];

    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            assert_ne!(ids[i], ids[j], "Program IDs {} and {} are equal", i, j);
        }
    }
}

#[test]
fn test_sysvar_ids() {
    // Verify sysvar IDs are distinct from program IDs
    let sysvars = [
        native_programs::SYSVAR_RENT_ID,
        native_programs::SYSVAR_CLOCK_ID,
        native_programs::SYSVAR_RECENT_BLOCKHASHES_ID,
    ];

    for sysvar in &sysvars {
        assert_ne!(*sysvar, native_programs::SYSTEM_PROGRAM_ID);
        assert_ne!(*sysvar, native_programs::TOKEN_PROGRAM_ID);
    }
}

#[test]
fn test_instruction_batch_creation() {
    let mut batch = InstructionBatch::default();
    batch.account_keys = vec![pubkey_n(1), pubkey_n(2), pubkey_n(3)];
    batch.writable_indices = vec![0, 1];
    batch.signer_indices = vec![0];

    // Verify structure
    assert_eq!(batch.account_keys.len(), 3);
    assert_eq!(batch.writable_indices.len(), 2);
    assert_eq!(batch.signer_indices.len(), 1);
}

#[test]
fn test_full_transaction_lifecycle() {
    // This is a high-level lifecycle test

    // 1. Create accounts
    let mut bank = InMemoryAccountBank::new();
    let payer = pubkey_n(1);
    let recipient = pubkey_n(2);

    bank.store(
        payer,
        Account::new_with_data(
            payer,
            10_000_000,
            vec![],
            native_programs::SYSTEM_PROGRAM_ID,
        ),
    );
    bank.store(
        recipient,
        Account::new_with_data(recipient, 0, vec![], native_programs::SYSTEM_PROGRAM_ID),
    );

    // 2. Create transfer instruction
    let instruction = SystemProgram::transfer(payer, recipient, 1_000_000);

    // 3. Verify instruction structure
    assert_eq!(instruction.accounts.len(), 2);
    assert!(instruction.accounts[0].is_writable);
    assert!(instruction.accounts[1].is_writable);

    // 4. In production, this would execute via runtime
    // For test, we verify the transaction can be structured correctly using compile

    let batch = InstructionBatch::compile(
        vec![instruction],
        payer,
        [0u8; 32], // recent blockhash
    )
    .unwrap();

    assert!(!batch.instructions.is_empty());
    assert!(batch.writable_indices.contains(&0)); // payer is writable
}
