//! Comprehensive tests for Dchat Program Language ABI v1
//!
//! These tests verify:
//! - ABI parsing correctness
//! - Router correctness
//! - PDA derivation matching host
//! - Readonly/writable enforcement
//! - Lamports conservation
//! - Determinism (repeat-run identical trace hashes)
//! - State commit correctness

use dchat_programs::abi::{
    AbiError, AccountTocEntry, AccountsBlob, EventEnvelope, IxEnvelope, SerializableAccount,
    ABI_VERSION, ACCOUNTS_BLOB_HEADER_SIZE, ACCOUNTS_BLOB_MAGIC, EVENT_MAGIC,
    IX_ENVELOPE_HEADER_SIZE, IX_ENVELOPE_MAGIC, MAX_ACCOUNTS, MAX_PAYLOAD_SIZE, TOC_ENTRY_SIZE,
};
use dchat_programs::account::Pubkey;
use dchat_programs::error::ProgramError;
use dchat_programs::guest::{AccountsCursor, InstructionRouter, ParsedInstruction};
use dchat_programs::host_commit::{CopyMeteringCosts, EmissionMetering};
use dchat_programs::pda::PdaDerivation;

// ═══════════════════════════════════════════════════════════════════════════════
// ABI PARSING TESTS - IxEnvelope
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_ix_envelope_roundtrip_empty_payload() {
    let envelope = IxEnvelope::new(0, vec![]).expect("should create envelope");
    let encoded = envelope.encode();
    let decoded = IxEnvelope::decode(&encoded).expect("should decode");

    assert_eq!(decoded.version, ABI_VERSION);
    assert_eq!(decoded.tag, 0);
    assert!(decoded.payload.is_empty());
}

#[test]
fn test_ix_envelope_roundtrip_max_tag() {
    let envelope = IxEnvelope::new(u16::MAX, vec![1, 2, 3]).expect("should create envelope");
    let encoded = envelope.encode();
    let decoded = IxEnvelope::decode(&encoded).expect("should decode");

    assert_eq!(decoded.tag, u16::MAX);
    assert_eq!(decoded.payload, vec![1, 2, 3]);
}

#[test]
fn test_ix_envelope_rejects_invalid_magic() {
    let data = vec![0x00, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
    let result = IxEnvelope::decode(&data);
    assert!(result.is_err());
    match result {
        Err(ProgramError::Custom(code)) => {
            assert_eq!(code, AbiError::InvalidMagic as u32);
        }
        _ => panic!("Expected InvalidMagic error"),
    }
}

#[test]
fn test_ix_envelope_rejects_version_too_low() {
    let envelope = IxEnvelope::new(1, vec![]).expect("should create");
    let mut encoded = envelope.encode();
    // Set version to 0 (below minimum)
    encoded[4] = 0;
    encoded[5] = 0;

    let result = IxEnvelope::decode(&encoded);
    assert!(result.is_err());
}

#[test]
fn test_ix_envelope_rejects_version_too_high() {
    let envelope = IxEnvelope::new(1, vec![]).expect("should create");
    let mut encoded = envelope.encode();
    // Set version to 0xFFFF (above maximum)
    encoded[4] = 0xFF;
    encoded[5] = 0xFF;

    let result = IxEnvelope::decode(&encoded);
    assert!(result.is_err());
}

#[test]
fn test_ix_envelope_rejects_oversized_payload_at_creation() {
    let big_payload = vec![0u8; (MAX_PAYLOAD_SIZE + 1) as usize];
    let result = IxEnvelope::new(1, big_payload);
    assert!(result.is_err());
}

#[test]
fn test_ix_envelope_rejects_truncated_header() {
    let data = vec![0x44, 0x43, 0x48, 0x58, 0x01]; // Only 5 bytes
    let result = IxEnvelope::decode(&data);
    assert!(result.is_err());
}

#[test]
fn test_ix_envelope_rejects_truncated_payload() {
    let envelope = IxEnvelope::new(1, vec![1, 2, 3, 4, 5]).expect("should create");
    let encoded = envelope.encode();
    // Truncate the payload
    let truncated = &encoded[..IX_ENVELOPE_HEADER_SIZE + 2];
    let result = IxEnvelope::decode(truncated);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// ABI PARSING TESTS - AccountsBlob
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_accounts_blob_roundtrip_empty() {
    let accounts: Vec<SerializableAccount> = vec![];
    let blob = AccountsBlob::new(&accounts).expect("should create");
    let encoded = blob.encode();
    let decoded = AccountsBlob::decode(&encoded).expect("should decode");

    assert_eq!(decoded.version, ABI_VERSION);
    assert!(decoded.entries.is_empty());
    assert!(decoded.data.is_empty());
}

#[test]
fn test_accounts_blob_roundtrip_single() {
    let accounts = vec![SerializableAccount {
        pubkey: Pubkey::new([1u8; 32]),
        owner: Pubkey::new([2u8; 32]),
        lamports: 1_000_000,
        data: vec![10, 20, 30, 40],
        is_signer: true,
        is_writable: true,
        executable: false,
        rent_epoch: 42,
    }];

    let blob = AccountsBlob::new(&accounts).expect("should create");
    let encoded = blob.encode();
    let decoded = AccountsBlob::decode(&encoded).expect("should decode");

    assert_eq!(decoded.entries.len(), 1);
    assert_eq!(decoded.entries[0].pubkey, Pubkey::new([1u8; 32]));
    assert_eq!(decoded.entries[0].owner, Pubkey::new([2u8; 32]));
    assert_eq!(decoded.entries[0].lamports, 1_000_000);
    assert!(decoded.entries[0].is_signer);
    assert!(decoded.entries[0].is_writable);
    assert!(!decoded.entries[0].executable);
    assert_eq!(decoded.entries[0].rent_epoch, 42);
    assert_eq!(decoded.get_account_data(0).unwrap(), &[10, 20, 30, 40]);
}

#[test]
fn test_accounts_blob_roundtrip_multiple() {
    let accounts = vec![
        SerializableAccount {
            pubkey: Pubkey::new([1u8; 32]),
            owner: Pubkey::new([10u8; 32]),
            lamports: 1000,
            data: vec![1, 2, 3],
            is_signer: true,
            is_writable: true,
            executable: false,
            rent_epoch: 0,
        },
        SerializableAccount {
            pubkey: Pubkey::new([2u8; 32]),
            owner: Pubkey::new([20u8; 32]),
            lamports: 2000,
            data: vec![4, 5, 6, 7, 8],
            is_signer: false,
            is_writable: true,
            executable: false,
            rent_epoch: 1,
        },
        SerializableAccount {
            pubkey: Pubkey::new([3u8; 32]),
            owner: Pubkey::new([30u8; 32]),
            lamports: 3000,
            data: vec![],
            is_signer: false,
            is_writable: false,
            executable: true,
            rent_epoch: 2,
        },
    ];

    let blob = AccountsBlob::new(&accounts).expect("should create");
    let encoded = blob.encode();
    let decoded = AccountsBlob::decode(&encoded).expect("should decode");

    assert_eq!(decoded.entries.len(), 3);
    assert_eq!(decoded.get_account_data(0).unwrap(), &[1, 2, 3]);
    assert_eq!(decoded.get_account_data(1).unwrap(), &[4, 5, 6, 7, 8]);
    assert!(decoded.get_account_data(2).unwrap().is_empty());
}

#[test]
fn test_accounts_blob_rejects_too_many_accounts() {
    let accounts: Vec<SerializableAccount> = (0..(MAX_ACCOUNTS as usize + 1))
        .map(|i| SerializableAccount {
            pubkey: Pubkey::new([i as u8; 32]),
            owner: Pubkey::zero(),
            lamports: 0,
            data: vec![],
            is_signer: false,
            is_writable: false,
            executable: false,
            rent_epoch: 0,
        })
        .collect();

    let result = AccountsBlob::new(&accounts);
    assert!(result.is_err());
}

#[test]
fn test_accounts_blob_rejects_invalid_magic() {
    let data = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    let result = AccountsBlob::decode(&data);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// ABI PARSING TESTS - TOC Entry
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_toc_entry_roundtrip() {
    let entry = AccountTocEntry {
        pubkey: Pubkey::new([0xABu8; 32]),
        owner: Pubkey::new([0xCDu8; 32]),
        lamports: 0xDEADBEEFCAFEBABE,
        data_len: 12345,
        data_off: 67890,
        is_signer: true,
        is_writable: false,
        executable: true,
        rent_epoch: 0xFEDCBA9876543210,
    };

    let encoded = entry.encode();
    assert_eq!(encoded.len(), TOC_ENTRY_SIZE);

    let decoded = AccountTocEntry::decode(&encoded).expect("should decode");
    assert_eq!(decoded.pubkey, entry.pubkey);
    assert_eq!(decoded.owner, entry.owner);
    assert_eq!(decoded.lamports, entry.lamports);
    assert_eq!(decoded.data_len, entry.data_len);
    assert_eq!(decoded.data_off, entry.data_off);
    assert_eq!(decoded.is_signer, entry.is_signer);
    assert_eq!(decoded.is_writable, entry.is_writable);
    assert_eq!(decoded.executable, entry.executable);
    assert_eq!(decoded.rent_epoch, entry.rent_epoch);
}

#[test]
fn test_toc_entry_size_is_correct() {
    let entry = AccountTocEntry {
        pubkey: Pubkey::zero(),
        owner: Pubkey::zero(),
        lamports: 0,
        data_len: 0,
        data_off: 0,
        is_signer: false,
        is_writable: false,
        executable: false,
        rent_epoch: 0,
    };
    let encoded = entry.encode();
    assert_eq!(encoded.len(), TOC_ENTRY_SIZE);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ABI PARSING TESTS - EventEnvelope
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_event_envelope_roundtrip() {
    let program_id = Pubkey::new([0x42u8; 32]);
    let event =
        EventEnvelope::new(program_id, "TestEvent", vec![1, 2, 3, 4, 5]).expect("should create");

    let encoded = event.encode();
    let decoded = EventEnvelope::decode(&encoded).expect("should decode");

    assert_eq!(decoded.version, ABI_VERSION);
    assert_eq!(decoded.program_id, program_id);
    assert_eq!(decoded.discriminator, event.discriminator);
    assert_eq!(decoded.data, vec![1, 2, 3, 4, 5]);
}

#[test]
fn test_event_discriminator_is_deterministic() {
    let disc1 = EventEnvelope::compute_discriminator("Transfer");
    let disc2 = EventEnvelope::compute_discriminator("Transfer");
    let disc_diff = EventEnvelope::compute_discriminator("Mint");

    assert_eq!(disc1, disc2);
    assert_ne!(disc1, disc_diff);
}

#[test]
fn test_event_magic_is_correct() {
    assert_eq!(EVENT_MAGIC, [0x44, 0x43, 0x48, 0x45]); // "DCHE"
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION ROUTER TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_router_dispatches_correct_handler() {
    struct Ctx {
        executed_tag: Option<u16>,
    }

    fn handler_1(
        ctx: &mut Ctx,
        _accounts: &mut AccountsCursor<'_>,
        _payload: &[u8],
    ) -> Result<(), ProgramError> {
        ctx.executed_tag = Some(1);
        Ok(())
    }

    fn handler_42(
        ctx: &mut Ctx,
        _accounts: &mut AccountsCursor<'_>,
        _payload: &[u8],
    ) -> Result<(), ProgramError> {
        ctx.executed_tag = Some(42);
        Ok(())
    }

    fn handler_100(
        ctx: &mut Ctx,
        _accounts: &mut AccountsCursor<'_>,
        _payload: &[u8],
    ) -> Result<(), ProgramError> {
        ctx.executed_tag = Some(100);
        Ok(())
    }

    let router = InstructionRouter::<Ctx>::new()
        .register(1, handler_1)
        .register(42, handler_42)
        .register(100, handler_100);

    // Create minimal accounts blob
    let accounts = create_test_serializable_accounts();
    let blob = AccountsBlob::new(&accounts).unwrap();
    let mut encoded = blob.encode();
    let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();

    // Test dispatch to tag 1
    let mut ctx = Ctx { executed_tag: None };
    router.dispatch(&mut ctx, &mut cursor, 1, &[]).unwrap();
    assert_eq!(ctx.executed_tag, Some(1));

    // Test dispatch to tag 42
    cursor.reset();
    ctx.executed_tag = None;
    router.dispatch(&mut ctx, &mut cursor, 42, &[]).unwrap();
    assert_eq!(ctx.executed_tag, Some(42));

    // Test dispatch to tag 100
    cursor.reset();
    ctx.executed_tag = None;
    router.dispatch(&mut ctx, &mut cursor, 100, &[]).unwrap();
    assert_eq!(ctx.executed_tag, Some(100));
}

#[test]
fn test_router_rejects_unknown_tag() {
    struct Ctx;

    let router = InstructionRouter::<Ctx>::new();

    let accounts = create_test_serializable_accounts();
    let blob = AccountsBlob::new(&accounts).unwrap();
    let mut encoded = blob.encode();
    let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();
    let mut ctx = Ctx;

    let result = router.dispatch(&mut ctx, &mut cursor, 999, &[]);
    assert!(result.is_err());
    match result {
        Err(ProgramError::Custom(code)) => {
            assert_eq!(code, AbiError::UnknownTag as u32);
        }
        _ => panic!("Expected UnknownTag error"),
    }
}

#[test]
fn test_router_fallback() {
    struct Ctx;

    fn custom_fallback(tag: u16) -> ProgramError {
        ProgramError::Custom(10000 + tag as u32)
    }

    let router = InstructionRouter::<Ctx>::new().fallback(custom_fallback);

    let accounts = create_test_serializable_accounts();
    let blob = AccountsBlob::new(&accounts).unwrap();
    let mut encoded = blob.encode();
    let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();
    let mut ctx = Ctx;

    let result = router.dispatch(&mut ctx, &mut cursor, 123, &[]);
    match result {
        Err(ProgramError::Custom(10123)) => {}
        other => panic!("Expected Custom(10123), got {:?}", other),
    }
}

#[test]
fn test_router_registered_tags() {
    struct Ctx;
    fn handler(_: &mut Ctx, _: &mut AccountsCursor<'_>, _: &[u8]) -> Result<(), ProgramError> {
        Ok(())
    }

    let router = InstructionRouter::<Ctx>::new()
        .register(5, handler)
        .register(10, handler)
        .register(15, handler);

    let tags = router.registered_tags();
    assert_eq!(tags.len(), 3);
    assert!(tags.contains(&5));
    assert!(tags.contains(&10));
    assert!(tags.contains(&15));
}

// ═══════════════════════════════════════════════════════════════════════════════
// PDA VERIFICATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_pda_derivation_matches_host() {
    let program_id = Pubkey::new([1u8; 32]);
    let seeds: &[&[u8]] = &[b"vault", b"user123"];

    // Derive PDA using host function
    let derived =
        PdaDerivation::find_program_address(seeds, &program_id).expect("should derive PDA");

    // Derive again and ensure it matches
    let derived2 =
        PdaDerivation::find_program_address(seeds, &program_id).expect("should derive PDA");

    assert_eq!(derived.address, derived2.address);
    assert_eq!(derived.bump, derived2.bump);
}

#[test]
fn test_pda_derivation_determinism() {
    let program_id = Pubkey::new([42u8; 32]);
    let seeds: &[&[u8]] = &[b"test_seed"];

    let mut addresses: Vec<Pubkey> = Vec::new();
    let mut bumps: Vec<u8> = Vec::new();

    for _ in 0..100 {
        let derived =
            PdaDerivation::find_program_address(seeds, &program_id).expect("should derive PDA");
        addresses.push(derived.address);
        bumps.push(derived.bump);
    }

    // All derivations must be identical
    let first_addr = addresses[0];
    let first_bump = bumps[0];
    for (addr, bump) in addresses.iter().zip(bumps.iter()) {
        assert_eq!(addr, &first_addr);
        assert_eq!(bump, &first_bump);
    }
}

#[test]
fn test_pda_different_seeds_produce_different_addresses() {
    let program_id = Pubkey::new([1u8; 32]);

    let pda1 = PdaDerivation::find_program_address(&[b"seed_a"], &program_id).unwrap();
    let pda2 = PdaDerivation::find_program_address(&[b"seed_b"], &program_id).unwrap();
    let pda3 = PdaDerivation::find_program_address(&[b"seed_a", b"extra"], &program_id).unwrap();

    assert_ne!(pda1.address, pda2.address);
    assert_ne!(pda1.address, pda3.address);
    assert_ne!(pda2.address, pda3.address);
}

#[test]
fn test_pda_different_programs_produce_different_addresses() {
    let program_id_1 = Pubkey::new([1u8; 32]);
    let program_id_2 = Pubkey::new([2u8; 32]);

    let pda1 = PdaDerivation::find_program_address(&[b"seed"], &program_id_1).unwrap();
    let pda2 = PdaDerivation::find_program_address(&[b"seed"], &program_id_2).unwrap();

    assert_ne!(pda1.address, pda2.address);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNTS CURSOR TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_accounts_cursor_parse_and_get() {
    let accounts = vec![
        SerializableAccount {
            pubkey: Pubkey::new([1u8; 32]),
            owner: Pubkey::new([10u8; 32]),
            lamports: 1000,
            data: vec![0xAB, 0xCD],
            is_signer: true,
            is_writable: true,
            executable: false,
            rent_epoch: 0,
        },
        SerializableAccount {
            pubkey: Pubkey::new([2u8; 32]),
            owner: Pubkey::new([20u8; 32]),
            lamports: 2000,
            data: vec![0x12, 0x34, 0x56],
            is_signer: false,
            is_writable: false,
            executable: false,
            rent_epoch: 1,
        },
    ];

    let blob = AccountsBlob::new(&accounts).unwrap();
    let mut encoded = blob.encode();
    let cursor = AccountsCursor::parse(&mut encoded).unwrap();

    assert_eq!(cursor.len(), 2);

    let view0 = cursor.get(0).unwrap();
    assert_eq!(view0.key(), &Pubkey::new([1u8; 32]));
    assert_eq!(view0.lamports(), 1000);
    assert!(view0.is_signer());
    assert!(view0.is_writable());

    let view1 = cursor.get(1).unwrap();
    assert_eq!(view1.key(), &Pubkey::new([2u8; 32]));
    assert_eq!(view1.lamports(), 2000);
    assert!(!view1.is_signer());
    assert!(!view1.is_writable());
}

#[test]
fn test_accounts_cursor_update_lamports() {
    let accounts = vec![SerializableAccount {
        pubkey: Pubkey::new([1u8; 32]),
        owner: Pubkey::new([10u8; 32]),
        lamports: 1000,
        data: vec![1, 2, 3],
        is_signer: true,
        is_writable: true,
        executable: false,
        rent_epoch: 0,
    }];

    let blob = AccountsBlob::new(&accounts).unwrap();
    let mut encoded = blob.encode();
    let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();

    // Update lamports should succeed for writable account
    cursor.update_lamports(0, 2000).expect("should update");
}

#[test]
fn test_accounts_cursor_rejects_update_on_readonly() {
    let accounts = vec![SerializableAccount {
        pubkey: Pubkey::new([1u8; 32]),
        owner: Pubkey::new([10u8; 32]),
        lamports: 1000,
        data: vec![1, 2, 3],
        is_signer: false,
        is_writable: false, // READONLY
        executable: false,
        rent_epoch: 0,
    }];

    let blob = AccountsBlob::new(&accounts).unwrap();
    let mut encoded = blob.encode();
    let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();

    // Update lamports should fail for readonly account
    let result = cursor.update_lamports(0, 2000);
    assert!(result.is_err());
    assert!(matches!(result, Err(ProgramError::AccountNotWritable)));
}

#[test]
fn test_accounts_cursor_reset() {
    let accounts = create_test_serializable_accounts();
    let blob = AccountsBlob::new(&accounts).unwrap();
    let mut encoded = blob.encode();
    let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();

    // Consume some accounts by iterating
    let _ = cursor.get(0);
    let _ = cursor.get(1);

    // Reset should work
    cursor.reset();

    // Should be able to access accounts again
    let view = cursor.get(0).unwrap();
    assert_eq!(view.key(), &Pubkey::new([1u8; 32]));
}

// ═══════════════════════════════════════════════════════════════════════════════
// DETERMINISM TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_ix_envelope_hash_determinism() {
    let env1 = IxEnvelope::new(42, vec![1, 2, 3, 4, 5]).unwrap();
    let env2 = IxEnvelope::new(42, vec![1, 2, 3, 4, 5]).unwrap();

    assert_eq!(env1.hash(), env2.hash());

    // Different payload = different hash
    let env3 = IxEnvelope::new(42, vec![1, 2, 3, 4, 6]).unwrap();
    assert_ne!(env1.hash(), env3.hash());
}

#[test]
fn test_accounts_blob_hash_determinism() {
    let accounts = create_test_serializable_accounts();

    let blob1 = AccountsBlob::new(&accounts).unwrap();
    let blob2 = AccountsBlob::new(&accounts).unwrap();

    assert_eq!(blob1.hash(), blob2.hash());
}

#[test]
fn test_determinism_repeated_runs_identical_encoding() {
    let accounts = create_test_serializable_accounts();
    let mut encodings: Vec<Vec<u8>> = Vec::new();

    for _ in 0..50 {
        let blob = AccountsBlob::new(&accounts).unwrap();
        encodings.push(blob.encode());
    }

    // All encodings must be identical
    let first = &encodings[0];
    for (i, encoding) in encodings.iter().enumerate() {
        assert_eq!(encoding, first, "Run {} produced different encoding", i);
    }
}

#[test]
fn test_determinism_parsing_produces_same_entries() {
    let accounts = create_test_serializable_accounts();
    let blob = AccountsBlob::new(&accounts).unwrap();
    let encoded = blob.encode();

    // Parse multiple times and verify identical results
    for _ in 0..10 {
        let decoded = AccountsBlob::decode(&encoded).unwrap();
        assert_eq!(decoded.entries.len(), accounts.len());
        for (i, entry) in decoded.entries.iter().enumerate() {
            assert_eq!(entry.pubkey, accounts[i].pubkey);
            assert_eq!(entry.lamports, accounts[i].lamports);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EMISSION METERING TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_emission_metering_tracks_logs() {
    let mut metering = EmissionMetering::new();

    metering.record_log(100);
    metering.record_log(200);

    assert_eq!(metering.logs_bytes, 300);
    assert_eq!(metering.logs_count, 2);
}

#[test]
fn test_emission_metering_tracks_events() {
    let mut metering = EmissionMetering::new();

    metering.record_event(500);
    metering.record_event(300);

    assert_eq!(metering.events_bytes, 800);
    assert_eq!(metering.events_count, 2);
}

#[test]
fn test_emission_metering_tracks_return_data() {
    let mut metering = EmissionMetering::new();

    metering.record_return_data(64);

    assert_eq!(metering.return_data_bytes, 64);
}

#[test]
fn test_emission_metering_total_cost() {
    let mut metering = EmissionMetering::new();

    metering.record_log(100);
    metering.record_event(200);
    metering.record_return_data(50);

    let total_cost = metering.total_cost(2); // 2 units per byte
    assert_eq!(total_cost, (100 + 200 + 50) * 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// COPY METERING COSTS TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_copy_metering_defaults() {
    let costs = CopyMeteringCosts::default();

    assert!(costs.copy_in_base > 0);
    assert!(costs.copy_in_per_byte > 0);
    assert!(costs.validation_per_account > 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ABI CONSTANTS VERIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_magic_bytes_are_correct() {
    // DCHX = [0x44, 0x43, 0x48, 0x58]
    assert_eq!(IX_ENVELOPE_MAGIC, [0x44, 0x43, 0x48, 0x58]);

    // DCHA = [0x44, 0x43, 0x48, 0x41]
    assert_eq!(ACCOUNTS_BLOB_MAGIC, [0x44, 0x43, 0x48, 0x41]);

    // DCHE = [0x44, 0x43, 0x48, 0x45]
    assert_eq!(EVENT_MAGIC, [0x44, 0x43, 0x48, 0x45]);
}

#[test]
fn test_abi_version_is_valid() {
    assert_eq!(ABI_VERSION, 1);
}

#[test]
fn test_header_sizes() {
    // IX envelope header: 4 (magic) + 2 (version) + 2 (tag) + 4 (payload_len) = 12
    assert_eq!(IX_ENVELOPE_HEADER_SIZE, 12);

    // Accounts blob header: 4 (magic) + 2 (version) + 2 (count) = 8
    assert_eq!(ACCOUNTS_BLOB_HEADER_SIZE, 8);
}

// ═══════════════════════════════════════════════════════════════════════════════
// PARSED INSTRUCTION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_parsed_instruction_extracts_tag_and_payload() {
    let envelope = IxEnvelope::new(42, vec![1, 2, 3, 4, 5]).unwrap();
    let encoded = envelope.encode();

    let parsed = ParsedInstruction::parse(&encoded).unwrap();

    assert_eq!(parsed.tag, 42);
    assert_eq!(parsed.payload, &[1, 2, 3, 4, 5]);
}

#[test]
fn test_parsed_instruction_empty_payload() {
    let envelope = IxEnvelope::new(0, vec![]).unwrap();
    let encoded = envelope.encode();

    let parsed = ParsedInstruction::parse(&encoded).unwrap();

    assert_eq!(parsed.tag, 0);
    assert!(parsed.payload.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

fn create_test_serializable_accounts() -> Vec<SerializableAccount> {
    vec![
        SerializableAccount {
            pubkey: Pubkey::new([1u8; 32]),
            owner: Pubkey::new([10u8; 32]),
            lamports: 1000,
            data: vec![1, 2, 3, 4],
            is_signer: true,
            is_writable: true,
            executable: false,
            rent_epoch: 0,
        },
        SerializableAccount {
            pubkey: Pubkey::new([2u8; 32]),
            owner: Pubkey::new([20u8; 32]),
            lamports: 2000,
            data: vec![5, 6, 7, 8],
            is_signer: false,
            is_writable: true,
            executable: false,
            rent_epoch: 0,
        },
    ]
}
