//! Intent system tests for dchat-miniapps
//!
//! Verifies:
//! - Intent creation and validation
//! - Intent type parsing
//! - Cross-chain intent lifecycle
//! - Threshold attestation

use chrono::{Duration, Utc};
use dchat_miniapps::intent::{
    Intent, IntentAccountMeta, IntentId, IntentPayload, IntentStatus, IntentType, VoteOption,
};
use dchat_miniapps::receipt::{AttestationSet, SignerSet};
use dchat_miniapps::registry::AppId;

fn user_bytes(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

fn app_id_n(n: u8) -> AppId {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    AppId(bytes)
}

fn pubkey_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

#[test]
fn test_intent_id_generation() {
    let id1 = IntentId::new();
    let id2 = IntentId::new();

    // Each generated ID should be unique
    assert_ne!(id1.0, id2.0);
}

#[test]
fn test_intent_id_default() {
    let id1 = IntentId::default();
    let id2 = IntentId::default();

    // Each default ID is new, so unique
    assert_ne!(id1.0, id2.0);
}

#[test]
fn test_intent_id_parse() {
    let id = IntentId::new();
    let id_str = id.0.to_string();
    let parsed = IntentId::parse(&id_str).expect("should parse");
    assert_eq!(id.0, parsed.0);
}

#[test]
fn test_intent_type_transfer() {
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: Some("test transfer".to_string()),
    };

    if let IntentType::Transfer {
        recipient,
        amount,
        memo,
        ..
    } = &intent_type
    {
        assert_eq!(recipient[0], 2);
        assert_eq!(*amount, 1000);
        assert_eq!(memo, &Some("test transfer".to_string()));
    } else {
        panic!("Expected Transfer variant");
    }
}

#[test]
fn test_intent_type_transfer_with_token() {
    let mint = user_bytes(99);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: Some(mint),
        amount: 5000,
        memo: None,
    };

    if let IntentType::Transfer {
        mint: token_mint, ..
    } = &intent_type
    {
        assert_eq!(token_mint, &Some(mint));
    } else {
        panic!("Expected Transfer variant");
    }
}

#[test]
fn test_intent_type_program_call() {
    let program_id = pubkey_n(10);
    let intent_type = IntentType::ProgramCall {
        program_id,
        instruction_data: vec![1, 2, 3, 4],
        accounts: vec![
            IntentAccountMeta {
                pubkey: pubkey_n(1),
                is_signer: true,
                is_writable: true,
            },
            IntentAccountMeta {
                pubkey: pubkey_n(2),
                is_signer: false,
                is_writable: true,
            },
        ],
    };

    if let IntentType::ProgramCall {
        program_id: pid,
        instruction_data,
        accounts,
    } = &intent_type
    {
        assert_eq!(pid[0], 10);
        assert_eq!(instruction_data, &vec![1, 2, 3, 4]);
        assert_eq!(accounts.len(), 2);
        assert!(accounts[0].is_signer);
        assert!(!accounts[1].is_signer);
    } else {
        panic!("Expected ProgramCall variant");
    }
}

#[test]
fn test_intent_type_batch() {
    let ops = vec![
        IntentType::Transfer {
            recipient: user_bytes(1),
            mint: None,
            amount: 100,
            memo: None,
        },
        IntentType::Transfer {
            recipient: user_bytes(2),
            mint: None,
            amount: 200,
            memo: None,
        },
    ];

    let intent_type = IntentType::Batch { operations: ops };

    if let IntentType::Batch { operations } = &intent_type {
        assert_eq!(operations.len(), 2);
    } else {
        panic!("Expected Batch variant");
    }
}

#[test]
fn test_intent_type_stake() {
    let intent_type = IntentType::Stake {
        validator: pubkey_n(5),
        amount: 10000,
    };

    if let IntentType::Stake { validator, amount } = &intent_type {
        assert_eq!(validator[0], 5);
        assert_eq!(*amount, 10000);
    } else {
        panic!("Expected Stake variant");
    }
}

#[test]
fn test_intent_type_unstake() {
    let intent_type = IntentType::Unstake {
        stake_account: pubkey_n(6),
        amount: 5000,
    };

    if let IntentType::Unstake {
        stake_account,
        amount,
    } = &intent_type
    {
        assert_eq!(stake_account[0], 6);
        assert_eq!(*amount, 5000);
    } else {
        panic!("Expected Unstake variant");
    }
}

#[test]
fn test_intent_type_vote() {
    let intent_type = IntentType::Vote {
        proposal_id: pubkey_n(7),
        vote: VoteOption::Yes,
    };

    if let IntentType::Vote { proposal_id, vote } = &intent_type {
        assert_eq!(proposal_id[0], 7);
        assert!(matches!(vote, VoteOption::Yes));
    } else {
        panic!("Expected Vote variant");
    }
}

#[test]
fn test_vote_options() {
    let votes = [VoteOption::Yes, VoteOption::No, VoteOption::Abstain];

    // Ensure all variants are distinct
    for (i, v1) in votes.iter().enumerate() {
        for (j, v2) in votes.iter().enumerate() {
            if i == j {
                assert!(std::mem::discriminant(v1) == std::mem::discriminant(v2));
            } else {
                assert!(std::mem::discriminant(v1) != std::mem::discriminant(v2));
            }
        }
    }
}

#[test]
fn test_intent_payload_new() {
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: None,
    };

    let payload = IntentPayload::new(sender, intent_type, 100, 1);

    assert_eq!(payload.sender, sender);
    assert_eq!(payload.max_fee, 100);
    assert_eq!(payload.chain_id, 1);
    assert!(payload.nonce != 0); // Random nonce
    assert!(payload.deadline > Utc::now()); // Future deadline
}

#[test]
fn test_intent_payload_hash_deterministic() {
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: Some("test".to_string()),
    };

    let payload = IntentPayload {
        intent_type,
        sender,
        nonce: 12345,
        chain_id: 1,
        max_fee: 100,
        deadline: Utc::now() + Duration::hours(1),
    };

    let hash1 = payload.hash();
    let hash2 = payload.hash();

    // Same payload should produce same hash
    assert_eq!(hash1, hash2);
}

#[test]
fn test_intent_payload_hash_changes_with_data() {
    let sender = user_bytes(1);
    let deadline = Utc::now() + Duration::hours(1);

    let payload1 = IntentPayload {
        intent_type: IntentType::Transfer {
            recipient: user_bytes(2),
            mint: None,
            amount: 1000,
            memo: None,
        },
        sender,
        nonce: 12345,
        chain_id: 1,
        max_fee: 100,
        deadline,
    };

    let payload2 = IntentPayload {
        intent_type: IntentType::Transfer {
            recipient: user_bytes(2),
            mint: None,
            amount: 2000, // Different amount
            memo: None,
        },
        sender,
        nonce: 12345,
        chain_id: 1,
        max_fee: 100,
        deadline,
    };

    // Different payloads should produce different hashes
    assert_ne!(payload1.hash(), payload2.hash());
}

#[test]
fn test_intent_payload_validate_success() {
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: None,
    };

    let payload = IntentPayload::new(sender, intent_type, 100, 1);
    assert!(payload.validate().is_ok());
}

#[test]
fn test_intent_payload_validate_zero_amount() {
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 0, // Invalid - zero amount
        memo: None,
    };

    let payload = IntentPayload::new(sender, intent_type, 100, 1);
    assert!(payload.validate().is_err());
}

#[test]
fn test_intent_payload_validate_zero_fee() {
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: None,
    };

    let payload = IntentPayload::new(sender, intent_type, 0, 1); // Invalid - zero fee
    assert!(payload.validate().is_err());
}

#[test]
fn test_intent_payload_validate_empty_batch() {
    let sender = user_bytes(1);
    let intent_type = IntentType::Batch { operations: vec![] }; // Invalid - empty

    let payload = IntentPayload::new(sender, intent_type, 100, 1);
    assert!(payload.validate().is_err());
}

#[test]
fn test_intent_payload_validate_too_many_accounts() {
    let sender = user_bytes(1);

    // Create 65 accounts (exceeds limit of 64)
    let accounts: Vec<IntentAccountMeta> = (0..65)
        .map(|i| IntentAccountMeta {
            pubkey: pubkey_n(i as u8),
            is_signer: false,
            is_writable: false,
        })
        .collect();

    let intent_type = IntentType::ProgramCall {
        program_id: pubkey_n(1),
        instruction_data: vec![],
        accounts,
    };

    let payload = IntentPayload::new(sender, intent_type, 100, 1);
    assert!(payload.validate().is_err());
}

#[test]
fn test_intent_payload_size() {
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: None,
    };

    let payload = IntentPayload::new(sender, intent_type, 100, 1);
    let size = payload.size();

    // Should have some reasonable size
    assert!(size > 0);
    assert!(size < 1000); // Basic transfer shouldn't be huge
}

#[test]
fn test_intent_creation() {
    let app_id = app_id_n(1);
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: None,
    };
    let payload = IntentPayload::new(sender, intent_type, 100, 1);

    let intent = Intent::new(app_id, payload);

    assert_eq!(intent.app_id, app_id);
    assert_eq!(intent.status, IntentStatus::Created);
    assert!(intent.signature.is_none());
    assert!(intent.error.is_none());
    assert!(intent.receipt_id.is_none());
}

#[test]
fn test_intent_status_transitions() {
    // Test terminal states
    assert!(IntentStatus::Executed.is_terminal());
    assert!(IntentStatus::Failed.is_terminal());
    assert!(IntentStatus::Expired.is_terminal());
    assert!(IntentStatus::Cancelled.is_terminal());

    // Test non-terminal states
    assert!(!IntentStatus::Created.is_terminal());
    assert!(!IntentStatus::Signed.is_terminal());
    assert!(!IntentStatus::Pending.is_terminal());
    assert!(!IntentStatus::Executing.is_terminal());

    // Test pending states
    assert!(IntentStatus::Pending.is_pending());
    assert!(IntentStatus::Executing.is_pending());
    assert!(!IntentStatus::Created.is_pending());
    assert!(!IntentStatus::Executed.is_pending());
}

#[test]
fn test_intent_mark_pending() {
    let app_id = app_id_n(1);
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: None,
    };
    let payload = IntentPayload::new(sender, intent_type, 100, 1);
    let mut intent = Intent::new(app_id, payload);

    intent.mark_pending();
    assert_eq!(intent.status, IntentStatus::Pending);
}

#[test]
fn test_intent_mark_executing() {
    let app_id = app_id_n(1);
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: None,
    };
    let payload = IntentPayload::new(sender, intent_type, 100, 1);
    let mut intent = Intent::new(app_id, payload);

    intent.mark_executing();
    assert_eq!(intent.status, IntentStatus::Executing);
}

#[test]
fn test_intent_mark_failed() {
    let app_id = app_id_n(1);
    let sender = user_bytes(1);
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: None,
    };
    let payload = IntentPayload::new(sender, intent_type, 100, 1);
    let mut intent = Intent::new(app_id, payload);

    intent.mark_failed("test error");
    assert_eq!(intent.status, IntentStatus::Failed);
    assert_eq!(intent.error, Some("test error".to_string()));
}

#[test]
fn test_signer_set_creation() {
    let signer_set = SignerSet::new(67, 3); // 67% threshold, min 3 attestations

    assert_eq!(signer_set.total_stake, 0);
}

#[test]
fn test_signer_set_default_thresholds() {
    let signer_set = SignerSet::default_thresholds();

    // Should be created with reasonable defaults
    assert_eq!(signer_set.total_stake, 0);
}

#[test]
fn test_signer_set_add_signer() {
    let mut signer_set = SignerSet::new(67, 3);

    signer_set.add_signer(pubkey_n(1), 1000);
    signer_set.add_signer(pubkey_n(2), 2000);
    signer_set.add_signer(pubkey_n(3), 3000);

    assert_eq!(signer_set.total_stake, 6000);
    assert!(signer_set.is_valid_signer(&pubkey_n(1)));
    assert!(signer_set.is_valid_signer(&pubkey_n(2)));
    assert!(signer_set.is_valid_signer(&pubkey_n(3)));
    assert!(!signer_set.is_valid_signer(&pubkey_n(4)));
}

#[test]
fn test_signer_set_remove_signer() {
    let mut signer_set = SignerSet::new(67, 3);

    signer_set.add_signer(pubkey_n(1), 1000);
    signer_set.add_signer(pubkey_n(2), 2000);

    assert_eq!(signer_set.total_stake, 3000);

    signer_set.remove_signer(&pubkey_n(1));

    assert_eq!(signer_set.total_stake, 2000);
    assert!(!signer_set.is_valid_signer(&pubkey_n(1)));
    assert!(signer_set.is_valid_signer(&pubkey_n(2)));
}

#[test]
fn test_signer_set_required_stake() {
    let mut signer_set = SignerSet::new(67, 3);

    signer_set.add_signer(pubkey_n(1), 1000);
    signer_set.add_signer(pubkey_n(2), 2000);
    signer_set.add_signer(pubkey_n(3), 3000);
    // Total: 6000, 67% = 4020

    let required = signer_set.required_stake();
    assert_eq!(required, 4020);
}

#[test]
fn test_attestation_set_creation() {
    let attestation_set = AttestationSet::new();

    assert_eq!(attestation_set.count(), 0);
    assert_eq!(attestation_set.total_stake, 0);
}

#[test]
fn test_intent_account_meta() {
    let meta = IntentAccountMeta {
        pubkey: pubkey_n(1),
        is_signer: true,
        is_writable: false,
    };

    assert_eq!(meta.pubkey[0], 1);
    assert!(meta.is_signer);
    assert!(!meta.is_writable);
}

#[test]
fn test_intent_type_serialization() {
    let intent_type = IntentType::Transfer {
        recipient: user_bytes(2),
        mint: None,
        amount: 1000,
        memo: Some("test".to_string()),
    };

    // Should serialize to JSON
    let json = serde_json::to_string(&intent_type).expect("serialize");
    assert!(json.contains("Transfer"));
    assert!(json.contains("1000"));

    // Should deserialize back
    let deserialized: IntentType = serde_json::from_str(&json).expect("deserialize");
    if let IntentType::Transfer { amount, memo, .. } = deserialized {
        assert_eq!(amount, 1000);
        assert_eq!(memo, Some("test".to_string()));
    } else {
        panic!("Expected Transfer");
    }
}

#[test]
fn test_intent_status_serialization() {
    let statuses = [
        IntentStatus::Created,
        IntentStatus::Signed,
        IntentStatus::Pending,
        IntentStatus::Executing,
        IntentStatus::Executed,
        IntentStatus::Failed,
        IntentStatus::Expired,
        IntentStatus::Cancelled,
    ];

    for status in statuses {
        let json = serde_json::to_string(&status).expect("serialize");
        let deserialized: IntentStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(status, deserialized);
    }
}
