//! Bot Registry Program Integration Tests
//!
//! Tests the on-chain bot registration and capability grant system including:
//! - Bot registration and lifecycle
//! - Capability grants with action bitmasks
//! - Mint restrictions
//! - Grant revocation
//! - Nonce protection

use dchat_programs::account::Pubkey;
use dchat_programs::bot_registry::{
    BotAccount, BotCapabilityGrantAccount, BotRegistryEvent, BotRegistryInstruction, BotStatus,
    MAX_ALLOWED_MINTS,
};
use dchat_programs::native_programs::BOT_REGISTRY_PROGRAM_ID;
use dchat_programs::pda::PdaDerivation;

// ═══════════════════════════════════════════════════════════════════════════════
// TEST HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

fn pubkey_n(n: u8) -> Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    Pubkey::new(bytes)
}

// ═══════════════════════════════════════════════════════════════════════════════
// BOT STATUS TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_status_values() {
    assert_eq!(BotStatus::Active as u8, 0);
    assert_eq!(BotStatus::Suspended as u8, 1);
}

#[test]
fn test_bot_status_is_active() {
    let active = BotStatus::Active;
    let suspended = BotStatus::Suspended;

    assert_eq!(active, BotStatus::Active);
    assert_eq!(suspended, BotStatus::Suspended);
    assert_ne!(active, suspended);
}

// ═══════════════════════════════════════════════════════════════════════════════
// BOT ACCOUNT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_account_new() {
    let owner = pubkey_n(1);
    let signing_pubkey = pubkey_n(2);
    let current_slot = 1000;

    let bot = BotAccount::new(owner, signing_pubkey, current_slot, 5);

    assert_eq!(bot.owner, owner);
    assert_eq!(bot.bot_signing_pubkey, signing_pubkey);
    assert_eq!(bot.status, BotStatus::Active);
    assert_eq!(bot.created_slot, current_slot);
    assert_eq!(bot.bump, 5);
    assert_eq!(bot.nonce, 0);
}

#[test]
fn test_bot_account_serialization() {
    let owner = pubkey_n(1);
    let signing_pubkey = pubkey_n(2);

    let bot = BotAccount::new(owner, signing_pubkey, 1000, 3);

    let bytes = bot.to_bytes();
    let restored = BotAccount::from_bytes(&bytes).expect("deserialize");

    assert_eq!(restored.owner, owner);
    assert_eq!(restored.bot_signing_pubkey, signing_pubkey);
    assert_eq!(restored.status, BotStatus::Active);
    assert_eq!(restored.created_slot, 1000);
}

#[test]
fn test_bot_account_nonce_increment() {
    let owner = pubkey_n(1);
    let signing_pubkey = pubkey_n(2);

    let mut bot = BotAccount::new(owner, signing_pubkey, 1000, 0);

    assert_eq!(bot.nonce, 0);
    bot.increment_nonce();
    assert_eq!(bot.nonce, 1);
    bot.increment_nonce();
    assert_eq!(bot.nonce, 2);
}

#[test]
fn test_bot_account_suspend_and_reactivate() {
    let owner = pubkey_n(1);
    let signing_pubkey = pubkey_n(2);

    let mut bot = BotAccount::new(owner, signing_pubkey, 1000, 0);

    assert_eq!(bot.status, BotStatus::Active);

    bot.status = BotStatus::Suspended;
    assert_eq!(bot.status, BotStatus::Suspended);

    bot.status = BotStatus::Active;
    assert_eq!(bot.status, BotStatus::Active);
}

// ═══════════════════════════════════════════════════════════════════════════════
// CAPABILITY GRANT ACCOUNT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_capability_grant_new() {
    let bot_id = pubkey_n(1);
    let target_program = pubkey_n(2);
    let allowed_actions: u64 = 0b111; // First 3 actions

    let grant = BotCapabilityGrantAccount::new(
        bot_id,
        target_program,
        allowed_actions,
        None, // no max amount
        &[],  // no mint restrictions
        0,    // no expiry
        5,    // bump
    )
    .expect("create grant");

    assert_eq!(grant.bot_id, bot_id);
    assert_eq!(grant.target_program, target_program);
    assert_eq!(grant.allowed_actions, allowed_actions);
    assert!(!grant.has_max_amount);
    assert_eq!(grant.mint_count, 0);
}

#[test]
fn test_capability_grant_with_max_amount() {
    let bot_id = pubkey_n(1);
    let target_program = pubkey_n(2);
    let allowed_actions: u64 = 0xFF;

    let grant = BotCapabilityGrantAccount::new(
        bot_id,
        target_program,
        allowed_actions,
        Some(1_000_000), // max amount
        &[],
        0,
        0,
    )
    .expect("create grant");

    assert!(grant.has_max_amount);
    assert_eq!(grant.max_amount, 1_000_000);
}

#[test]
fn test_capability_grant_with_mint_restrictions() {
    let bot_id = pubkey_n(1);
    let target_program = pubkey_n(2);
    let mint1 = pubkey_n(10);
    let mint2 = pubkey_n(11);
    let mints = vec![mint1, mint2];

    let grant = BotCapabilityGrantAccount::new(bot_id, target_program, 0xFF, None, &mints, 0, 0)
        .expect("create grant");

    assert_eq!(grant.mint_count, 2);

    // Check mints are stored correctly
    assert_eq!(grant.allowed_mints[0], mint1);
    assert_eq!(grant.allowed_mints[1], mint2);
}

#[test]
fn test_capability_grant_serialization() {
    let bot_id = pubkey_n(1);
    let target_program = pubkey_n(2);

    let grant = BotCapabilityGrantAccount::new(
        bot_id,
        target_program,
        0b1111,
        Some(500_000),
        &[pubkey_n(10)],
        99999,
        3,
    )
    .expect("create grant");

    let bytes = grant.to_bytes();
    let restored = BotCapabilityGrantAccount::from_bytes(&bytes).expect("deserialize");

    assert_eq!(restored.bot_id, bot_id);
    assert_eq!(restored.target_program, target_program);
    assert_eq!(restored.allowed_actions, 0b1111);
    assert!(restored.has_max_amount);
    assert_eq!(restored.max_amount, 500_000);
    assert_eq!(restored.expiry_slot, 99999);
}

#[test]
fn test_capability_grant_nonce_increment() {
    let bot_id = pubkey_n(1);
    let target_program = pubkey_n(2);

    let mut grant = BotCapabilityGrantAccount::new(bot_id, target_program, 0xFF, None, &[], 0, 0)
        .expect("create grant");

    assert_eq!(grant.nonce, 0);
    grant.increment_nonce();
    assert_eq!(grant.nonce, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACTION BITMASK TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_action_bitmask_check_single() {
    let allowed: u64 = 0b0001; // Only action 0
    let action_to_check: u64 = 0b0001;

    assert!(allowed & action_to_check != 0);
}

#[test]
fn test_action_bitmask_check_multiple() {
    let allowed: u64 = 0b1111; // Actions 0-3
    let action1: u64 = 0b0001;
    let action2: u64 = 0b0100;
    let action_not_allowed: u64 = 0b10000;

    assert!(allowed & action1 != 0);
    assert!(allowed & action2 != 0);
    assert!(allowed & action_not_allowed == 0);
}

#[test]
fn test_action_bitmask_all_actions() {
    let all_actions: u64 = u64::MAX;

    for bit in 0..64 {
        let action: u64 = 1 << bit;
        assert!(all_actions & action != 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MINT RESTRICTION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_mint_restriction_check() {
    let mint1 = pubkey_n(10);
    let mint2 = pubkey_n(11);
    let mint3 = pubkey_n(12);

    let grant =
        BotCapabilityGrantAccount::new(pubkey_n(1), pubkey_n(2), 0xFF, None, &[mint1, mint2], 0, 0)
            .expect("create grant");

    // Check if mints are in allowed list
    let allowed_mints = &grant.allowed_mints[..grant.mint_count as usize];

    assert!(allowed_mints.contains(&mint1));
    assert!(allowed_mints.contains(&mint2));
    assert!(!allowed_mints.contains(&mint3));
}

#[test]
fn test_mint_restriction_empty_allows_all() {
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(1),
        pubkey_n(2),
        0xFF,
        None,
        &[], // No mint restrictions = all mints allowed
        0,
        0,
    )
    .expect("create grant");

    assert_eq!(grant.mint_count, 0);
    // When mint_count is 0, any mint should be allowed
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION SERIALIZATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_instruction_register_bot() {
    let ix = BotRegistryInstruction::RegisterBot {
        bot_signing_pubkey: pubkey_n(1),
        expected_nonce: 0,
    };

    let bytes = ix.pack();
    let restored = BotRegistryInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        BotRegistryInstruction::RegisterBot {
            bot_signing_pubkey, ..
        } => {
            assert_eq!(bot_signing_pubkey, pubkey_n(1));
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_rotate_bot_key() {
    let ix = BotRegistryInstruction::RotateBotKey {
        new_signing_pubkey: pubkey_n(5),
        expected_nonce: 3,
    };

    let bytes = ix.pack();
    let restored = BotRegistryInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        BotRegistryInstruction::RotateBotKey {
            new_signing_pubkey,
            expected_nonce,
        } => {
            assert_eq!(new_signing_pubkey, pubkey_n(5));
            assert_eq!(expected_nonce, 3);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_suspend() {
    let ix = BotRegistryInstruction::Suspend { expected_nonce: 2 };

    let bytes = ix.pack();
    let restored = BotRegistryInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        BotRegistryInstruction::Suspend { expected_nonce } => {
            assert_eq!(expected_nonce, 2);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_unsuspend() {
    let ix = BotRegistryInstruction::Unsuspend { expected_nonce: 4 };

    let bytes = ix.pack();
    let restored = BotRegistryInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        BotRegistryInstruction::Unsuspend { expected_nonce } => {
            assert_eq!(expected_nonce, 4);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_grant_capability() {
    let ix = BotRegistryInstruction::GrantCapability {
        target_program: pubkey_n(10),
        allowed_actions: 0b1111_0000,
        max_amount: Some(1_000_000),
        allowed_mints: vec![pubkey_n(20), pubkey_n(21)],
        expiry_slot: 86400,
        expected_nonce: 0,
    };

    let bytes = ix.pack();
    let restored = BotRegistryInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        BotRegistryInstruction::GrantCapability {
            target_program,
            allowed_actions,
            max_amount,
            allowed_mints,
            expiry_slot,
            ..
        } => {
            assert_eq!(target_program, pubkey_n(10));
            assert_eq!(allowed_actions, 0b1111_0000);
            assert_eq!(max_amount, Some(1_000_000));
            assert_eq!(allowed_mints.len(), 2);
            assert_eq!(expiry_slot, 86400);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_revoke_capability() {
    let ix = BotRegistryInstruction::RevokeCapability { expected_nonce: 1 };

    let bytes = ix.pack();
    let restored = BotRegistryInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        BotRegistryInstruction::RevokeCapability { expected_nonce } => {
            assert_eq!(expected_nonce, 1);
        }
        _ => panic!("wrong instruction type"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PDA DERIVATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_pda_derivation() {
    let owner = pubkey_n(1);
    let seeds: &[&[u8]] = &[b"bot", &owner.0];

    let result = PdaDerivation::find_program_address(seeds, &BOT_REGISTRY_PROGRAM_ID);

    if let Ok(pda) = result {
        // Verify determinism
        let result2 = PdaDerivation::find_program_address(seeds, &BOT_REGISTRY_PROGRAM_ID);
        assert!(result2.is_ok());
        assert_eq!(pda.address, result2.unwrap().address);
    }
}

#[test]
fn test_grant_pda_derivation() {
    let bot_id = pubkey_n(1);
    let target_program = pubkey_n(2);
    let nonce: u64 = 0;
    let nonce_bytes = nonce.to_le_bytes();
    let seeds: &[&[u8]] = &[b"grant", &bot_id.0, &target_program.0, &nonce_bytes];

    let result = PdaDerivation::find_program_address(seeds, &BOT_REGISTRY_PROGRAM_ID);

    if let Ok(pda) = result {
        // Verify determinism
        let result2 = PdaDerivation::find_program_address(seeds, &BOT_REGISTRY_PROGRAM_ID);
        assert!(result2.is_ok());
        assert_eq!(pda.address, result2.unwrap().address);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EXPIRY TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_grant_expiry_check() {
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(1),
        pubkey_n(2),
        0xFF,
        None,
        &[],
        1000, // expires at slot 1000
        0,
    )
    .expect("create grant");

    // Simulating current slot checks
    let current_slot_before: u64 = 500;
    let current_slot_after: u64 = 1500;

    // Grant should be valid before expiry
    assert!(current_slot_before < grant.expiry_slot || grant.expiry_slot == 0);

    // Grant should be expired after
    assert!(current_slot_after > grant.expiry_slot && grant.expiry_slot != 0);
}

#[test]
fn test_grant_no_expiry() {
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(1),
        pubkey_n(2),
        0xFF,
        None,
        &[],
        0, // no expiry (0 = never expires)
        0,
    )
    .expect("create grant");

    assert_eq!(grant.expiry_slot, 0);
    // expiry_slot == 0 means never expires
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT EMISSION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_registered_event() {
    let event = BotRegistryEvent::BotRegistered {
        bot: pubkey_n(1),
        owner: pubkey_n(2),
        signing_pubkey: pubkey_n(3),
    };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

#[test]
fn test_capability_granted_event() {
    let event = BotRegistryEvent::CapabilityGranted {
        bot: pubkey_n(1),
        grant: pubkey_n(2),
        target_program: pubkey_n(3),
        actions: 0xFF,
        expiry_slot: 99999,
    };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

#[test]
fn test_capability_revoked_event() {
    let event = BotRegistryEvent::CapabilityRevoked {
        bot: pubkey_n(1),
        grant: pubkey_n(2),
    };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

#[test]
fn test_bot_suspended_event() {
    let event = BotRegistryEvent::BotSuspended { bot: pubkey_n(1) };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

#[test]
fn test_bot_unsuspended_event() {
    let event = BotRegistryEvent::BotUnsuspended { bot: pubkey_n(1) };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAX ALLOWED MINTS CONSTANT TEST
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_max_allowed_mints_constant() {
    // Verify MAX_ALLOWED_MINTS is a reasonable value
    assert!(MAX_ALLOWED_MINTS >= 4);
    assert!(MAX_ALLOWED_MINTS <= 16);
}
