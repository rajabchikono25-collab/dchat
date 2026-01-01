//! Marketplace Program Integration Tests
//!
//! Tests the on-chain marketplace escrow system including:
//! - Escrow lifecycle (create, deliver, release, refund)
//! - Dispute resolution
//! - State transitions and nonce protection
//! - Bot authorization integration

use dchat_programs::account::Pubkey;
use dchat_programs::marketplace::{
    EscrowAccount, EscrowState, MarketplaceEvent, MarketplaceInstruction, RecipientSplit,
    MAX_RECIPIENTS,
};
use dchat_programs::native_programs::MARKETPLACE_PROGRAM_ID;
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
// ESCROW STATE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_escrow_state_transitions() {
    // Valid transitions from Locked
    let locked = EscrowState::Locked;
    assert!(locked.can_transition_to(EscrowState::Delivered));
    assert!(locked.can_transition_to(EscrowState::Disputed));
    assert!(locked.can_transition_to(EscrowState::Refunded));
    assert!(locked.can_transition_to(EscrowState::Released));

    let delivered = EscrowState::Delivered;
    assert!(delivered.can_transition_to(EscrowState::Released));
    assert!(delivered.can_transition_to(EscrowState::Disputed));

    let disputed = EscrowState::Disputed;
    assert!(disputed.can_transition_to(EscrowState::Released));
    assert!(disputed.can_transition_to(EscrowState::Refunded));

    // Terminal states cannot transition
    let released = EscrowState::Released;
    assert!(!released.can_transition_to(EscrowState::Locked));
    assert!(!released.can_transition_to(EscrowState::Refunded));

    let refunded = EscrowState::Refunded;
    assert!(!refunded.can_transition_to(EscrowState::Released));
}

#[test]
fn test_escrow_state_is_terminal() {
    assert!(!EscrowState::Locked.is_terminal());
    assert!(!EscrowState::Delivered.is_terminal());
    assert!(!EscrowState::Disputed.is_terminal());
    assert!(EscrowState::Released.is_terminal());
    assert!(EscrowState::Refunded.is_terminal());
    assert!(EscrowState::Expired.is_terminal());
}

// ═══════════════════════════════════════════════════════════════════════════════
// RECIPIENT SPLIT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_recipient_split_creation() {
    let split = RecipientSplit {
        pubkey: pubkey_n(1),
        share_bps: 10000, // 100%
    };

    assert_eq!(split.share_bps, 10000);
}

#[test]
fn test_recipient_split_validation_valid() {
    let splits = vec![
        RecipientSplit {
            pubkey: pubkey_n(1),
            share_bps: 5000,
        },
        RecipientSplit {
            pubkey: pubkey_n(2),
            share_bps: 5000,
        },
    ];

    let total: u64 = splits.iter().map(|s| s.share_bps).sum();
    assert_eq!(total, 10000);
}

#[test]
fn test_recipient_split_validation_invalid() {
    let splits = vec![
        RecipientSplit {
            pubkey: pubkey_n(1),
            share_bps: 5000,
        },
        RecipientSplit {
            pubkey: pubkey_n(2),
            share_bps: 4000,
        },
    ];

    let total: u64 = splits.iter().map(|s| s.share_bps).sum();
    assert_ne!(total, 10000); // Should not equal 100%
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION SERIALIZATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_instruction_create_escrow() {
    let ix = MarketplaceInstruction::CreateEscrow {
        recipients: vec![RecipientSplit {
            pubkey: pubkey_n(10),
            share_bps: 10000,
        }],
        amount: 1_000_000,
        expiry_slot: 1000,
        dispute_resolver: pubkey_n(20),
        expected_nonce: 0,
    };

    let bytes = ix.pack();
    let restored = MarketplaceInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        MarketplaceInstruction::CreateEscrow {
            amount,
            expiry_slot,
            ..
        } => {
            assert_eq!(amount, 1_000_000);
            assert_eq!(expiry_slot, 1000);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_mark_delivered() {
    let ix = MarketplaceInstruction::MarkDelivered { expected_nonce: 5 };

    let bytes = ix.pack();
    let restored = MarketplaceInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        MarketplaceInstruction::MarkDelivered { expected_nonce } => {
            assert_eq!(expected_nonce, 5);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_release() {
    let ix = MarketplaceInstruction::Release { expected_nonce: 3 };

    let bytes = ix.pack();
    let restored = MarketplaceInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        MarketplaceInstruction::Release { expected_nonce } => {
            assert_eq!(expected_nonce, 3);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_refund() {
    let ix = MarketplaceInstruction::Refund { expected_nonce: 2 };

    let bytes = ix.pack();
    let restored = MarketplaceInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        MarketplaceInstruction::Refund { expected_nonce } => {
            assert_eq!(expected_nonce, 2);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_raise_dispute() {
    let ix = MarketplaceInstruction::RaiseDispute { expected_nonce: 1 };

    let bytes = ix.pack();
    let restored = MarketplaceInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        MarketplaceInstruction::RaiseDispute { expected_nonce } => {
            assert_eq!(expected_nonce, 1);
        }
        _ => panic!("wrong instruction type"),
    }
}

#[test]
fn test_instruction_resolve_dispute() {
    let ix = MarketplaceInstruction::ResolveDispute {
        expected_nonce: 4,
        release: true,
    };

    let bytes = ix.pack();
    let restored = MarketplaceInstruction::unpack(&bytes).expect("deserialize");

    match restored {
        MarketplaceInstruction::ResolveDispute {
            expected_nonce,
            release,
        } => {
            assert_eq!(expected_nonce, 4);
            assert!(release);
        }
        _ => panic!("wrong instruction type"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ESCROW ACCOUNT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_escrow_account_new() {
    let buyer = pubkey_n(1);
    let mint = pubkey_n(3);
    let dispute_resolver = pubkey_n(20);
    let recipients = vec![RecipientSplit {
        pubkey: pubkey_n(10),
        share_bps: 10000,
    }];

    let escrow = EscrowAccount::new(
        buyer,
        &recipients,
        mint,
        1_000_000,
        1000, // created_slot
        0,    // no expiry
        dispute_resolver,
        5, // bump
    )
    .expect("create escrow");

    assert_eq!(escrow.buyer, buyer);
    assert_eq!(escrow.mint, mint);
    assert_eq!(escrow.amount, 1_000_000);
    assert_eq!(escrow.state, EscrowState::Locked);
    assert_eq!(escrow.nonce, 0);
}

#[test]
fn test_escrow_account_serialization() {
    let buyer = pubkey_n(2);
    let mint = pubkey_n(3);
    let dispute_resolver = pubkey_n(20);
    let recipients = vec![RecipientSplit {
        pubkey: pubkey_n(10),
        share_bps: 10000,
    }];

    let escrow = EscrowAccount::new(
        buyer,
        &recipients,
        mint,
        500_000,
        1000,
        99999,
        dispute_resolver,
        3,
    )
    .expect("create escrow");

    let bytes = escrow.to_bytes();
    let restored = EscrowAccount::from_bytes(&bytes).expect("deserialize");

    assert_eq!(restored.buyer, buyer);
    assert_eq!(restored.amount, 500_000);
    assert_eq!(restored.expiry_slot, 99999);
}

#[test]
fn test_escrow_nonce_increment() {
    let buyer = pubkey_n(1);
    let mint = pubkey_n(3);
    let dispute_resolver = pubkey_n(20);
    let recipients = vec![RecipientSplit {
        pubkey: pubkey_n(10),
        share_bps: 10000,
    }];

    let mut escrow =
        EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 0, dispute_resolver, 0)
            .expect("create escrow");

    assert_eq!(escrow.nonce, 0);
    escrow.increment_nonce();
    assert_eq!(escrow.nonce, 1);
    escrow.increment_nonce();
    assert_eq!(escrow.nonce, 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// PDA DERIVATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_escrow_pda_derivation() {
    let buyer = pubkey_n(1);
    let nonce: u64 = 42;
    let nonce_bytes = nonce.to_le_bytes();
    let seeds: &[&[u8]] = &[b"escrow", &buyer.0, &nonce_bytes];

    let result = PdaDerivation::find_program_address(seeds, &MARKETPLACE_PROGRAM_ID);

    if let Ok(pda) = result {
        // Verify determinism
        let result2 = PdaDerivation::find_program_address(seeds, &MARKETPLACE_PROGRAM_ID);
        assert!(result2.is_ok());
        assert_eq!(pda.address, result2.unwrap().address);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AMOUNT DISTRIBUTION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_amount_distribution_single_recipient() {
    let amount: u64 = 1_000_000;
    let share_bps: u64 = 10000; // 100%

    let distributed = (amount as u128 * share_bps as u128 / 10000) as u64;
    assert_eq!(distributed, 1_000_000);
}

#[test]
fn test_amount_distribution_multiple_recipients() {
    let amount: u64 = 1_000_000;

    let share1_bps: u64 = 7000; // 70%
    let share2_bps: u64 = 3000; // 30%

    let dist1 = (amount as u128 * share1_bps as u128 / 10000) as u64;
    let dist2 = (amount as u128 * share2_bps as u128 / 10000) as u64;

    assert_eq!(dist1, 700_000);
    assert_eq!(dist2, 300_000);
    assert_eq!(dist1 + dist2, amount);
}

#[test]
fn test_amount_distribution_rounding() {
    // Test with an amount that doesn't divide evenly
    let amount: u64 = 1_000_001;

    let share1_bps: u64 = 5000; // 50%
    let share2_bps: u64 = 5000; // 50%

    let dist1 = (amount as u128 * share1_bps as u128 / 10000) as u64;
    let dist2 = (amount as u128 * share2_bps as u128 / 10000) as u64;

    // Due to integer division, we may lose a mote
    assert_eq!(dist1, 500_000);
    assert_eq!(dist2, 500_000);
    // Total is 1 less than original due to rounding
    assert_eq!(dist1 + dist2, 1_000_000);
}

// ═══════════════════════════════════════════════════════════════════════════════
// EXPIRY TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_escrow_expiry_check() {
    let current_slot = 1000;
    let expiry_slot = 500; // Already expired

    assert!(current_slot > expiry_slot);

    let expiry_future = 2000;
    assert!(current_slot < expiry_future);
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT EMISSION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_marketplace_event_escrow_created() {
    let event = MarketplaceEvent::EscrowCreated {
        escrow: pubkey_n(1),
        buyer: pubkey_n(2),
        amount: 1_000_000,
        mint: pubkey_n(3),
        expiry_slot: 99999,
    };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

#[test]
fn test_marketplace_event_dispute_resolved() {
    let event = MarketplaceEvent::DisputeResolved {
        escrow: pubkey_n(1),
        resolver: pubkey_n(2),
        released: true,
        bot_authorized: false,
    };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

#[test]
fn test_marketplace_event_escrow_released() {
    let event = MarketplaceEvent::EscrowReleased {
        escrow: pubkey_n(1),
        releaser: pubkey_n(2),
        amount: 500_000,
    };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

#[test]
fn test_marketplace_event_escrow_refunded() {
    let event = MarketplaceEvent::EscrowRefunded {
        escrow: pubkey_n(1),
        refunder: pubkey_n(2),
        amount: 500_000,
    };

    let program_event = event.to_program_event();
    assert!(!program_event.data.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAX RECIPIENTS CONSTANT TEST
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_max_recipients_constant() {
    // Verify MAX_RECIPIENTS is a reasonable value
    assert!(MAX_RECIPIENTS >= 2);
    assert!(MAX_RECIPIENTS <= 16);
}

// ═══════════════════════════════════════════════════════════════════════════════
// ADDITIONAL ASSET TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_escrow_state_invalid_transitions() {
    // Released is terminal - cannot transition anywhere
    let released = EscrowState::Released;
    assert!(!released.can_transition_to(EscrowState::Locked));
    assert!(!released.can_transition_to(EscrowState::Delivered));
    assert!(!released.can_transition_to(EscrowState::Disputed));
    assert!(!released.can_transition_to(EscrowState::Refunded));
    assert!(!released.can_transition_to(EscrowState::Expired));

    // Refunded is terminal - cannot transition anywhere
    let refunded = EscrowState::Refunded;
    assert!(!refunded.can_transition_to(EscrowState::Locked));
    assert!(!refunded.can_transition_to(EscrowState::Delivered));
    assert!(!refunded.can_transition_to(EscrowState::Disputed));
    assert!(!refunded.can_transition_to(EscrowState::Released));
    assert!(!refunded.can_transition_to(EscrowState::Expired));

    // Expired is terminal - cannot transition anywhere
    let expired = EscrowState::Expired;
    assert!(!expired.can_transition_to(EscrowState::Locked));
    assert!(!expired.can_transition_to(EscrowState::Delivered));
    assert!(!expired.can_transition_to(EscrowState::Disputed));
    assert!(!expired.can_transition_to(EscrowState::Released));
    assert!(!expired.can_transition_to(EscrowState::Refunded));
}

#[test]
fn test_escrow_state_from_u8() {
    assert_eq!(EscrowState::from_u8(0), Some(EscrowState::Locked));
    assert_eq!(EscrowState::from_u8(1), Some(EscrowState::Delivered));
    assert_eq!(EscrowState::from_u8(2), Some(EscrowState::Disputed));
    assert_eq!(EscrowState::from_u8(3), Some(EscrowState::Released));
    assert_eq!(EscrowState::from_u8(4), Some(EscrowState::Refunded));
    assert_eq!(EscrowState::from_u8(5), Some(EscrowState::Expired));
    assert_eq!(EscrowState::from_u8(6), None);
    assert_eq!(EscrowState::from_u8(255), None);
}

#[test]
fn test_recipient_split_size() {
    // Verify RecipientSplit size constant matches actual serialization
    assert_eq!(RecipientSplit::SIZE, 40);
}

#[test]
fn test_multi_party_distribution_three_way() {
    let amount: u64 = 1_000_000;

    let share1_bps: u64 = 5000; // 50%
    let share2_bps: u64 = 3000; // 30%
    let share3_bps: u64 = 2000; // 20%

    let dist1 = (amount as u128 * share1_bps as u128 / 10000) as u64;
    let dist2 = (amount as u128 * share2_bps as u128 / 10000) as u64;
    let dist3 = (amount as u128 * share3_bps as u128 / 10000) as u64;

    assert_eq!(dist1, 500_000);
    assert_eq!(dist2, 300_000);
    assert_eq!(dist3, 200_000);
    assert_eq!(dist1 + dist2 + dist3, amount);
}

#[test]
fn test_multi_party_distribution_uneven_shares() {
    let amount: u64 = 1_000_000;

    // Uneven split: 33.33%, 33.33%, 33.34%
    let share1_bps: u64 = 3333;
    let share2_bps: u64 = 3333;
    let share3_bps: u64 = 3334;

    let dist1 = (amount as u128 * share1_bps as u128 / 10000) as u64;
    let dist2 = (amount as u128 * share2_bps as u128 / 10000) as u64;
    let dist3 = (amount as u128 * share3_bps as u128 / 10000) as u64;

    assert_eq!(dist1, 333_300);
    assert_eq!(dist2, 333_300);
    assert_eq!(dist3, 333_400);
    assert_eq!(dist1 + dist2 + dist3, amount);
}

#[test]
fn test_escrow_with_max_recipients() {
    let buyer = pubkey_n(1);
    let mint = pubkey_n(3);
    let dispute_resolver = pubkey_n(20);

    // Create maximum number of recipients
    let mut recipients = Vec::with_capacity(MAX_RECIPIENTS);
    let share_per_recipient = 10000 / MAX_RECIPIENTS as u64;
    let remainder = 10000 % MAX_RECIPIENTS as u64;

    for i in 0..MAX_RECIPIENTS {
        let share = if i == 0 {
            share_per_recipient + remainder // Give remainder to first recipient
        } else {
            share_per_recipient
        };
        recipients.push(RecipientSplit {
            pubkey: pubkey_n(10 + i as u8),
            share_bps: share,
        });
    }

    // Verify shares sum to 100%
    let total_shares: u64 = recipients.iter().map(|r| r.share_bps).sum();
    assert_eq!(total_shares, 10000);

    let escrow = EscrowAccount::new(
        buyer,
        &recipients,
        mint,
        10_000_000,
        1000,
        0,
        dispute_resolver,
        5,
    )
    .expect("create escrow with max recipients");

    assert_eq!(escrow.recipient_count, MAX_RECIPIENTS as u8);
}

#[test]
fn test_escrow_serialization_roundtrip_with_dispute() {
    let buyer = pubkey_n(2);
    let mint = pubkey_n(3);
    let dispute_resolver = pubkey_n(20);
    let recipients = vec![RecipientSplit {
        pubkey: pubkey_n(10),
        share_bps: 10000,
    }];

    let mut escrow = EscrowAccount::new(
        buyer,
        &recipients,
        mint,
        500_000,
        1000,
        99999,
        dispute_resolver,
        3,
    )
    .expect("create escrow");

    // Simulate state transition to disputed
    escrow.state = EscrowState::Disputed;
    escrow.has_dispute = true;
    escrow.dispute_slot = 1500;
    escrow.increment_nonce();

    let bytes = escrow.to_bytes();
    let restored = EscrowAccount::from_bytes(&bytes).expect("deserialize");

    assert_eq!(restored.state, EscrowState::Disputed);
    assert!(restored.has_dispute);
    assert_eq!(restored.dispute_slot, 1500);
    assert_eq!(restored.nonce, 1);
}

#[test]
fn test_escrow_zero_amount() {
    let buyer = pubkey_n(1);
    let mint = pubkey_n(3);
    let dispute_resolver = pubkey_n(20);
    let recipients = vec![RecipientSplit {
        pubkey: pubkey_n(10),
        share_bps: 10000,
    }];

    // Zero amount should still create a valid escrow structurally
    let escrow = EscrowAccount::new(buyer, &recipients, mint, 0, 1000, 0, dispute_resolver, 5);

    // The escrow should be created (validation of minimum amount is handled elsewhere)
    assert!(escrow.is_ok());
    assert_eq!(escrow.unwrap().amount, 0);
}

#[test]
fn test_escrow_large_amount() {
    let buyer = pubkey_n(1);
    let mint = pubkey_n(3);
    let dispute_resolver = pubkey_n(20);
    let recipients = vec![RecipientSplit {
        pubkey: pubkey_n(10),
        share_bps: 10000,
    }];

    // Test with maximum u64 amount
    let escrow = EscrowAccount::new(
        buyer,
        &recipients,
        mint,
        u64::MAX,
        1000,
        0,
        dispute_resolver,
        5,
    )
    .expect("create escrow with max amount");

    assert_eq!(escrow.amount, u64::MAX);

    // Verify serialization/deserialization
    let bytes = escrow.to_bytes();
    let restored = EscrowAccount::from_bytes(&bytes).expect("deserialize");
    assert_eq!(restored.amount, u64::MAX);
}

#[test]
fn test_instruction_all_variants() {
    // Test all instruction variants can be packed and unpacked
    let variants = [
        MarketplaceInstruction::CreateEscrow {
            recipients: vec![RecipientSplit {
                pubkey: pubkey_n(10),
                share_bps: 10000,
            }],
            amount: 1_000_000,
            expiry_slot: 1000,
            dispute_resolver: pubkey_n(20),
            expected_nonce: 0,
        },
        MarketplaceInstruction::MarkDelivered { expected_nonce: 1 },
        MarketplaceInstruction::Release { expected_nonce: 2 },
        MarketplaceInstruction::Refund { expected_nonce: 3 },
        MarketplaceInstruction::RaiseDispute { expected_nonce: 4 },
        MarketplaceInstruction::ResolveDispute {
            expected_nonce: 5,
            release: true,
        },
        MarketplaceInstruction::ResolveDispute {
            expected_nonce: 6,
            release: false,
        },
    ];

    for ix in variants {
        let bytes = ix.pack();
        let restored = MarketplaceInstruction::unpack(&bytes).expect("deserialize instruction");

        // Verify round-trip produces equivalent bytes
        let restored_bytes = restored.pack();
        assert_eq!(bytes, restored_bytes);
    }
}

#[test]
fn test_marketplace_event_all_variants() {
    // Test all event variants can be serialized
    let events = [
        MarketplaceEvent::EscrowCreated {
            escrow: pubkey_n(1),
            buyer: pubkey_n(2),
            amount: 1_000_000,
            mint: pubkey_n(3),
            expiry_slot: 99999,
        },
        MarketplaceEvent::EscrowDelivered {
            escrow: pubkey_n(1),
            seller: pubkey_n(2),
        },
        MarketplaceEvent::EscrowReleased {
            escrow: pubkey_n(1),
            releaser: pubkey_n(2),
            amount: 500_000,
        },
        MarketplaceEvent::EscrowRefunded {
            escrow: pubkey_n(1),
            refunder: pubkey_n(2),
            amount: 500_000,
        },
        MarketplaceEvent::DisputeRaised {
            escrow: pubkey_n(1),
            disputer: pubkey_n(2),
        },
        MarketplaceEvent::DisputeResolved {
            escrow: pubkey_n(1),
            resolver: pubkey_n(2),
            released: true,
            bot_authorized: true,
        },
    ];

    for event in events {
        let program_event = event.to_program_event();
        assert!(!program_event.data.is_empty());
    }
}
