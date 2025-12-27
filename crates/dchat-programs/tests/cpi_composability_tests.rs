//! CPI Composability Tests
//!
//! Tests the cross-program invocation pattern where bots perform authorized
//! marketplace operations. This validates the BotRegistry capability grant
//! system works correctly with the Marketplace program.

use dchat_programs::account::Pubkey;
use dchat_programs::bot_registry::{
    BotAccount, BotCapabilityGrantAccount, BotRegistryInstruction, BotStatus, BOT_SEED,
    CAPABILITY_SEED, MAX_ALLOWED_MINTS,
};
use dchat_programs::marketplace::{
    EscrowAccount, EscrowState, MarketplaceInstruction, RecipientSplit, ESCROW_SEED, MAX_RECIPIENTS,
};
use dchat_programs::native_programs::{BOT_REGISTRY_PROGRAM_ID, MARKETPLACE_PROGRAM_ID};
use dchat_programs::pda::PdaDerivation;

// ═══════════════════════════════════════════════════════════════════════════════
// TEST HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

fn pubkey_n(n: u8) -> Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    Pubkey::new(bytes)
}

// Capability action bitmasks (matching bot_registry.rs constants)
const ACTION_CREATE_ESCROW: u64 = 1 << 0;
const ACTION_RELEASE: u64 = 1 << 1;
const ACTION_REFUND: u64 = 1 << 2;
const ACTION_RAISE_DISPUTE: u64 = 1 << 3;
const ACTION_RESOLVE_DISPUTE: u64 = 1 << 4;

// ═══════════════════════════════════════════════════════════════════════════════
// BOT + MARKETPLACE CPI TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_can_have_marketplace_capability() {
    // Setup: Bot owner and bot
    let owner = pubkey_n(1);
    let bot_signing_key = pubkey_n(2);
    let current_slot = 1000;

    // Create bot account
    let bot = BotAccount::new(owner, bot_signing_key, current_slot, 5);

    assert_eq!(bot.status, BotStatus::Active);
    assert!(bot.is_active());

    // Create capability grant for marketplace operations
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(10), // bot PDA
        MARKETPLACE_PROGRAM_ID,
        ACTION_CREATE_ESCROW | ACTION_RELEASE | ACTION_REFUND,
        Some(1_000_000), // max 1M tokens per operation
        &[],             // any mint allowed
        0,               // no expiry
        8,
    )
    .expect("create capability grant");

    // Verify grant targets marketplace
    assert_eq!(grant.target_program, MARKETPLACE_PROGRAM_ID);

    // Verify allowed actions
    assert!(grant.is_action_allowed(ACTION_CREATE_ESCROW));
    assert!(grant.is_action_allowed(ACTION_RELEASE));
    assert!(grant.is_action_allowed(ACTION_REFUND));
    assert!(!grant.is_action_allowed(ACTION_RAISE_DISPUTE));
    assert!(!grant.is_action_allowed(ACTION_RESOLVE_DISPUTE));

    // Verify amount limit
    assert!(grant.is_amount_allowed(500_000));
    assert!(grant.is_amount_allowed(1_000_000));
    assert!(!grant.is_amount_allowed(1_000_001));
}

#[test]
fn test_bot_capability_with_mint_restriction() {
    // Setup: Allow bot to only operate with specific mints
    let allowed_mint_1 = pubkey_n(20);
    let allowed_mint_2 = pubkey_n(21);

    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(10), // bot PDA
        MARKETPLACE_PROGRAM_ID,
        ACTION_CREATE_ESCROW,
        None, // no amount limit
        &[allowed_mint_1, allowed_mint_2],
        0, // no expiry
        8,
    )
    .expect("create capability grant with mints");

    // Should allow the specified mints
    assert!(grant.is_mint_allowed(&allowed_mint_1));
    assert!(grant.is_mint_allowed(&allowed_mint_2));

    // Should reject other mints
    let unauthorized_mint = pubkey_n(22);
    assert!(!grant.is_mint_allowed(&unauthorized_mint));
}

#[test]
fn test_bot_capability_pda_derivation_matches_escrow() {
    // This test verifies that the PDA derivation patterns are compatible
    // for cross-program references

    let buyer = pubkey_n(1);
    let seller = pubkey_n(2);
    let mint = pubkey_n(3);
    let nonce: u64 = 12345;

    // Derive escrow PDA
    let escrow_seeds: &[&[u8]] = &[ESCROW_SEED, buyer.0.as_ref(), &nonce.to_le_bytes()];
    let escrow_pda = PdaDerivation::find_program_address(escrow_seeds, &MARKETPLACE_PROGRAM_ID);
    assert!(escrow_pda.is_ok());

    // Now create a bot and capability grant that references this marketplace operation
    let bot_owner = pubkey_n(10);
    let bot_signing_key = pubkey_n(11);

    // Derive bot PDA
    let bot_seeds: &[&[u8]] = &[BOT_SEED, bot_owner.0.as_ref()];
    let bot_pda = PdaDerivation::find_program_address(bot_seeds, &BOT_REGISTRY_PROGRAM_ID);
    assert!(bot_pda.is_ok());

    let bot_pda_result = bot_pda.unwrap();
    let bot_address = bot_pda_result.address;

    // Derive capability grant PDA for this bot's marketplace access
    let capability_seeds: &[&[u8]] = &[
        CAPABILITY_SEED,
        bot_address.0.as_ref(),
        MARKETPLACE_PROGRAM_ID.0.as_ref(),
    ];
    let capability_pda =
        PdaDerivation::find_program_address(capability_seeds, &BOT_REGISTRY_PROGRAM_ID);
    assert!(capability_pda.is_ok());

    // All PDAs should be unique
    let escrow_pda_result = escrow_pda.unwrap();
    let escrow_addr = escrow_pda_result.address;
    let capability_pda_result = capability_pda.unwrap();
    let capability_addr = capability_pda_result.address;

    assert_ne!(escrow_addr, bot_address);
    assert_ne!(escrow_addr, capability_addr);
    assert_ne!(bot_address, capability_addr);
}

#[test]
fn test_escrow_creation_requires_valid_capability() {
    // Simulate the CPI flow:
    // 1. Bot is registered
    // 2. Bot has capability grant for marketplace
    // 3. Bot creates escrow on behalf of user

    let bot_owner = pubkey_n(1);
    let bot_signing_key = pubkey_n(2);
    let current_slot = 1000;

    // Step 1: Bot registration
    let bot = BotAccount::new(bot_owner, bot_signing_key, current_slot, 5);
    assert!(bot.is_active());

    // Step 2: Grant capability for marketplace create_escrow
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(100), // bot PDA (mock)
        MARKETPLACE_PROGRAM_ID,
        ACTION_CREATE_ESCROW,
        Some(10_000_000_000),   // 10B max
        &[],                    // any mint
        current_slot + 100_000, // expires in 100k slots
        8,
    )
    .expect("create grant");

    // Verify grant is valid and not expired
    assert!(grant.is_action_allowed(ACTION_CREATE_ESCROW));
    assert!(!grant.is_expired(current_slot));
    assert!(!grant.is_expired(current_slot + 50_000));
    assert!(grant.is_expired(current_slot + 100_001)); // expired

    // Step 3: Bot creates escrow (simulated)
    let buyer = pubkey_n(10);
    let seller = pubkey_n(11);
    let mint = pubkey_n(20);

    let recipients = [RecipientSplit {
        pubkey: seller,
        share_bps: 10000, // 100%
    }];

    // Verify the capability allows this amount
    let escrow_amount = 1_000_000;
    assert!(grant.is_amount_allowed(escrow_amount));

    // Create escrow
    let escrow = EscrowAccount::new(
        buyer,
        &recipients,
        mint,
        escrow_amount,
        current_slot,
        current_slot + 86400, // expires in ~1 day
        pubkey_n(50),         // dispute resolver
        10,
    )
    .expect("create escrow");

    assert_eq!(escrow.state, EscrowState::Locked);
    assert_eq!(escrow.amount, escrow_amount);
}

#[test]
fn test_instruction_encoding_for_cpi() {
    // Verify instructions can be properly encoded for CPI calls

    // Bot registration instruction
    let register = BotRegistryInstruction::RegisterBot {
        bot_signing_pubkey: pubkey_n(1),
        expected_nonce: 0,
    };
    let register_bytes = register.pack();
    let register_restored =
        BotRegistryInstruction::unpack(&register_bytes).expect("unpack register");
    match register_restored {
        BotRegistryInstruction::RegisterBot {
            bot_signing_pubkey,
            expected_nonce,
        } => {
            assert_eq!(bot_signing_pubkey, pubkey_n(1));
            assert_eq!(expected_nonce, 0);
        }
        _ => panic!("wrong variant"),
    }

    // Marketplace create escrow instruction
    let recipients = vec![RecipientSplit {
        pubkey: pubkey_n(2),
        share_bps: 10000,
    }];
    let create = MarketplaceInstruction::CreateEscrow {
        recipients,
        amount: 5_000_000,
        expiry_slot: 1000000,
        dispute_resolver: pubkey_n(99),
        expected_nonce: 42,
    };
    let create_bytes = create.pack();
    let create_restored = MarketplaceInstruction::unpack(&create_bytes).expect("unpack create");
    match create_restored {
        MarketplaceInstruction::CreateEscrow {
            amount,
            expected_nonce,
            ..
        } => {
            assert_eq!(amount, 5_000_000);
            assert_eq!(expected_nonce, 42);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_capability_grant_instruction_encoding() {
    // Encode a capability grant instruction
    let allowed_mints = vec![pubkey_n(20), pubkey_n(21)];
    let allowed_actions = ACTION_CREATE_ESCROW | ACTION_RELEASE;

    let grant_ix = BotRegistryInstruction::GrantCapability {
        target_program: MARKETPLACE_PROGRAM_ID,
        allowed_actions,
        max_amount: Some(1_000_000_000),
        allowed_mints: allowed_mints.clone(),
        expiry_slot: 999999,
        expected_nonce: 5,
    };

    let bytes = grant_ix.pack();
    let restored = BotRegistryInstruction::unpack(&bytes).expect("unpack grant");

    match restored {
        BotRegistryInstruction::GrantCapability {
            target_program,
            allowed_actions: restored_actions,
            max_amount,
            allowed_mints: restored_mints,
            expiry_slot,
            expected_nonce,
        } => {
            assert_eq!(target_program, MARKETPLACE_PROGRAM_ID);
            assert_eq!(restored_actions, allowed_actions);
            assert_eq!(max_amount, Some(1_000_000_000));
            assert_eq!(restored_mints.len(), 2);
            assert_eq!(expiry_slot, 999999);
            assert_eq!(expected_nonce, 5);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_verify_capability_instruction() {
    // The VerifyCapability instruction allows checking if a bot has permission
    // for a specific action without actually executing it

    let verify_ix = BotRegistryInstruction::VerifyCapability {
        target_program: MARKETPLACE_PROGRAM_ID,
        action: ACTION_RELEASE,
        mint: Some(pubkey_n(30)),
        amount: Some(500_000),
    };

    let bytes = verify_ix.pack();
    let restored = BotRegistryInstruction::unpack(&bytes).expect("unpack verify");

    match restored {
        BotRegistryInstruction::VerifyCapability {
            target_program,
            action,
            amount,
            mint,
        } => {
            assert_eq!(target_program, MARKETPLACE_PROGRAM_ID);
            assert_eq!(action, ACTION_RELEASE);
            assert_eq!(amount, Some(500_000));
            assert_eq!(mint, Some(pubkey_n(30)));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn test_full_cpi_workflow_simulation() {
    // Simulate a complete CPI workflow:
    // 1. User registers a bot
    // 2. User grants bot capability to create escrows
    // 3. Bot creates escrow on user's behalf
    // 4. Seller delivers
    // 5. Bot releases funds

    let user = pubkey_n(1);
    let bot_signing_key = pubkey_n(2);
    let seller = pubkey_n(3);
    let mint = pubkey_n(10);
    let current_slot: u64 = 1000;
    let amount: u64 = 1_000_000;

    // Step 1: Register bot
    let mut bot = BotAccount::new(user, bot_signing_key, current_slot, 5);
    assert!(bot.is_active());
    assert_eq!(bot.nonce, 0);

    // Increment nonce after registration
    bot.increment_nonce();
    assert_eq!(bot.nonce, 1);

    // Step 2: Grant capability
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(100), // bot PDA
        MARKETPLACE_PROGRAM_ID,
        ACTION_CREATE_ESCROW | ACTION_RELEASE,
        Some(10_000_000),
        &[mint], // only allow specific mint
        current_slot + 100_000,
        8,
    )
    .expect("grant");

    assert!(grant.is_action_allowed(ACTION_CREATE_ESCROW));
    assert!(grant.is_action_allowed(ACTION_RELEASE));
    assert!(grant.is_mint_allowed(&mint));
    assert!(grant.is_amount_allowed(amount));

    // Step 3: Bot creates escrow
    let recipients = [RecipientSplit {
        pubkey: seller,
        share_bps: 10000,
    }];

    let mut escrow = EscrowAccount::new(
        user,
        &recipients,
        mint,
        amount,
        current_slot,
        current_slot + 86400,
        pubkey_n(99), // dispute resolver
        10,
    )
    .expect("escrow");

    assert_eq!(escrow.state, EscrowState::Locked);
    assert_eq!(escrow.nonce, 0);
    escrow.increment_nonce();

    // Step 4: Seller marks as delivered
    assert!(escrow.state.can_transition_to(EscrowState::Delivered));
    escrow.state = EscrowState::Delivered;
    escrow.increment_nonce();

    // Step 5: Bot releases funds (with capability check)
    assert!(grant.is_action_allowed(ACTION_RELEASE));
    assert!(grant.is_amount_allowed(escrow.amount));
    assert!(escrow.state.can_transition_to(EscrowState::Released));
    escrow.state = EscrowState::Released;
    escrow.increment_nonce();

    // Final state
    assert_eq!(escrow.state, EscrowState::Released);
    assert!(escrow.state.is_terminal());
    assert_eq!(escrow.nonce, 3);
}

#[test]
fn test_suspended_bot_cannot_act() {
    // A suspended bot should fail capability checks
    let owner = pubkey_n(1);
    let bot_signing_key = pubkey_n(2);
    let current_slot = 1000;

    let mut bot = BotAccount::new(owner, bot_signing_key, current_slot, 5);
    assert!(bot.is_active());

    // Suspend the bot
    bot.status = BotStatus::Suspended;
    assert!(!bot.is_active());

    // Even though capability exists, bot status check should fail
    let _grant = BotCapabilityGrantAccount::new(
        pubkey_n(100),
        MARKETPLACE_PROGRAM_ID,
        ACTION_CREATE_ESCROW,
        None,
        &[],
        0,
        8,
    )
    .expect("grant");

    // In production: BotRegistryProcessor would check bot.is_active() before
    // allowing any operation through VerifyCapability
    assert!(!bot.is_active(), "Suspended bot should not be active");
}

#[test]
fn test_expired_capability_rejected() {
    let current_slot = 10000;

    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(100),
        MARKETPLACE_PROGRAM_ID,
        ACTION_CREATE_ESCROW,
        None,
        &[],
        current_slot - 1, // already expired
        8,
    )
    .expect("grant");

    assert!(grant.is_expired(current_slot));

    // In production: capability check would fail
    // The grant exists but should not be honored
}

#[test]
fn test_amount_exceeds_capability_limit() {
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(100),
        MARKETPLACE_PROGRAM_ID,
        ACTION_CREATE_ESCROW,
        Some(1_000_000), // max 1M
        &[],
        0,
        8,
    )
    .expect("grant");

    // Under limit - allowed
    assert!(grant.is_amount_allowed(999_999));
    assert!(grant.is_amount_allowed(1_000_000));

    // Over limit - rejected
    assert!(!grant.is_amount_allowed(1_000_001));
    assert!(!grant.is_amount_allowed(10_000_000));
}

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_multi_recipient_escrow_with_bot() {
    // Bot creates a multi-recipient escrow (revenue split)
    let current_slot = 1000;

    let recipients = [
        RecipientSplit {
            pubkey: pubkey_n(10),
            share_bps: 5000,
        }, // 50%
        RecipientSplit {
            pubkey: pubkey_n(11),
            share_bps: 3000,
        }, // 30%
        RecipientSplit {
            pubkey: pubkey_n(12),
            share_bps: 2000,
        }, // 20%
    ];

    // Grant with high limit for multi-party payments
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(100),
        MARKETPLACE_PROGRAM_ID,
        ACTION_CREATE_ESCROW | ACTION_RELEASE,
        Some(100_000_000_000), // 100B limit
        &[],
        0,
        8,
    )
    .expect("grant");

    let amount = 10_000_000_000; // 10B tokens
    assert!(grant.is_amount_allowed(amount));

    let escrow = EscrowAccount::new(
        pubkey_n(1), // buyer
        &recipients,
        pubkey_n(20), // mint
        amount,
        current_slot,
        0, // no expiry
        pubkey_n(99),
        10,
    )
    .expect("multi-recipient escrow");

    assert_eq!(escrow.recipient_count, 3);
    assert_eq!(escrow.recipients[0].share_bps, 5000);
    assert_eq!(escrow.recipients[1].share_bps, 3000);
    assert_eq!(escrow.recipients[2].share_bps, 2000);
}

#[test]
fn test_dispute_resolution_capability() {
    // Only specific bots can resolve disputes
    let grant = BotCapabilityGrantAccount::new(
        pubkey_n(100),
        MARKETPLACE_PROGRAM_ID,
        ACTION_RESOLVE_DISPUTE, // only dispute resolution
        None,
        &[],
        0,
        8,
    )
    .expect("dispute resolver grant");

    // Can resolve disputes
    assert!(grant.is_action_allowed(ACTION_RESOLVE_DISPUTE));

    // Cannot create, release, or refund
    assert!(!grant.is_action_allowed(ACTION_CREATE_ESCROW));
    assert!(!grant.is_action_allowed(ACTION_RELEASE));
    assert!(!grant.is_action_allowed(ACTION_REFUND));
}
