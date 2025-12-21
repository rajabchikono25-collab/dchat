//! Intent system tests for dchat-miniapps
//!
//! Verifies:
//! - Intent creation and validation
//! - Intent type parsing
//! - Cross-chain intent lifecycle
//! - Threshold attestation

use dchat_miniapps::intent::{
    Intent, IntentId, IntentPayload, IntentStatus, IntentType, PendingIntent,
};
use dchat_miniapps::receipt::{Attestation, AttestationSet, CrossChainReceipt, SignerSet};

fn user_id_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

fn intent_id_n(n: u8) -> IntentId {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    IntentId(bytes)
}

fn pubkey_n(n: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[0] = n;
    bytes
}

#[test]
fn test_intent_id_generation() {
    let id1 = IntentId::generate();
    let id2 = IntentId::generate();

    // Each generated ID should be unique
    assert_ne!(id1.0, id2.0);
}

#[test]
fn test_intent_type_transfer() {
    let intent_type = IntentType::Transfer {
        from: user_id_n(1),
        to: user_id_n(2),
        amount: 1000,
        token: None,
    };

    assert!(matches!(intent_type, IntentType::Transfer { .. }));
}

#[test]
fn test_intent_type_payment() {
    let intent_type = IntentType::Payment {
        payer: user_id_n(1),
        payee: user_id_n(2),
        amount: 500,
        memo: Some("test payment".to_string()),
    };

    if let IntentType::Payment { memo, .. } = intent_type {
        assert_eq!(memo, Some("test payment".to_string()));
    }
}

#[test]
fn test_intent_type_app_call() {
    let intent_type = IntentType::AppCall {
        app_id: intent_id_n(10),
        method: "process".to_string(),
        args: vec![1, 2, 3, 4],
    };

    if let IntentType::AppCall { method, args, .. } = intent_type {
        assert_eq!(method, "process");
        assert_eq!(args, vec![1, 2, 3, 4]);
    }
}

#[test]
fn test_intent_payload_creation() {
    let payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: 9999999999,
        chain_id: 1,
    };

    assert_eq!(payload.nonce, 12345);
    assert_eq!(payload.chain_id, 1);
}

#[test]
fn test_intent_creation() {
    let payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    let intent = Intent::new(user_id_n(1), payload, vec![1, 2, 3, 4]);

    assert_eq!(intent.sender, user_id_n(1));
    assert_eq!(intent.signature, vec![1, 2, 3, 4]);
    assert!(intent.id.0 != [0u8; 32]);
}

#[test]
fn test_intent_hash_determinism() {
    let payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    let hash1 = payload.compute_hash();
    let hash2 = payload.compute_hash();

    assert_eq!(hash1, hash2);
}

#[test]
fn test_intent_hash_changes_on_modification() {
    let payload1 = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    let payload2 = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 2000, // Different amount
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    let hash1 = payload1.compute_hash();
    let hash2 = payload2.compute_hash();

    assert_ne!(hash1, hash2);
}

#[test]
fn test_pending_intent_creation() {
    let payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    let intent = Intent::new(user_id_n(1), payload, vec![]);
    let pending = PendingIntent::from_intent(intent);

    assert_eq!(pending.status, IntentStatus::Pending);
    assert!(pending.submitted_at > 0);
    assert!(pending.receipt.is_none());
}

#[test]
fn test_pending_intent_status_transitions() {
    let payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    let intent = Intent::new(user_id_n(1), payload, vec![]);
    let mut pending = PendingIntent::from_intent(intent);

    // Initial state
    assert_eq!(pending.status, IntentStatus::Pending);

    // Transition to confirmed
    pending.status = IntentStatus::Confirmed;
    assert_eq!(pending.status, IntentStatus::Confirmed);

    // Transition to executed
    pending.status = IntentStatus::Executed;
    assert_eq!(pending.status, IntentStatus::Executed);
}

#[test]
fn test_signer_set_creation() {
    let signers = vec![pubkey_n(1), pubkey_n(2), pubkey_n(3)];
    let set = SignerSet::new(signers.clone(), 2); // 2-of-3

    assert_eq!(set.signers.len(), 3);
    assert_eq!(set.threshold, 2);
}

#[test]
fn test_signer_set_contains() {
    let signers = vec![pubkey_n(1), pubkey_n(2), pubkey_n(3)];
    let set = SignerSet::new(signers, 2);

    assert!(set.contains(&pubkey_n(1)));
    assert!(set.contains(&pubkey_n(2)));
    assert!(set.contains(&pubkey_n(3)));
    assert!(!set.contains(&pubkey_n(4)));
}

#[test]
fn test_attestation_creation() {
    let signer = pubkey_n(1);
    let message_hash = [1u8; 32];
    let signature = vec![1, 2, 3, 4, 5];

    let attestation = Attestation::new(signer, message_hash, signature.clone());

    assert_eq!(attestation.signer, signer);
    assert_eq!(attestation.message_hash, message_hash);
    assert_eq!(attestation.signature, signature);
}

#[test]
fn test_attestation_set_creation() {
    let set = AttestationSet::new();

    assert!(set.is_empty());
    assert_eq!(set.count(), 0);
}

#[test]
fn test_attestation_set_add() {
    let mut set = AttestationSet::new();
    let attestation = Attestation::new(pubkey_n(1), [1u8; 32], vec![1, 2, 3]);

    set.add(attestation);

    assert!(!set.is_empty());
    assert_eq!(set.count(), 1);
}

#[test]
fn test_attestation_set_no_duplicate_signers() {
    let mut set = AttestationSet::new();

    // Add first attestation from signer 1
    set.add(Attestation::new(pubkey_n(1), [1u8; 32], vec![1]));

    // Try to add another from same signer - should be rejected or replaced
    set.add(Attestation::new(pubkey_n(1), [1u8; 32], vec![2]));

    // Should still only have one attestation from this signer
    assert_eq!(set.unique_signers().len(), 1);
}

#[test]
fn test_attestation_set_meets_threshold() {
    let signers = vec![pubkey_n(1), pubkey_n(2), pubkey_n(3)];
    let signer_set = SignerSet::new(signers, 2);

    let mut attestations = AttestationSet::new();

    // One attestation - below threshold
    attestations.add(Attestation::new(pubkey_n(1), [1u8; 32], vec![1]));
    assert!(!attestations.meets_threshold(&signer_set));

    // Two attestations - meets threshold
    attestations.add(Attestation::new(pubkey_n(2), [1u8; 32], vec![2]));
    assert!(attestations.meets_threshold(&signer_set));
}

#[test]
fn test_attestation_from_non_member_ignored() {
    let signers = vec![pubkey_n(1), pubkey_n(2), pubkey_n(3)];
    let signer_set = SignerSet::new(signers, 2);

    let mut attestations = AttestationSet::new();

    // Add from non-member
    attestations.add(Attestation::new(pubkey_n(99), [1u8; 32], vec![1]));

    // Add from member
    attestations.add(Attestation::new(pubkey_n(1), [1u8; 32], vec![2]));

    // Only member attestation counts
    assert_eq!(attestations.valid_count(&signer_set), 1);
}

#[test]
fn test_cross_chain_receipt_creation() {
    let intent_id = intent_id_n(1);
    let source_chain = 1;
    let dest_chain = 2;
    let success = true;

    let receipt = CrossChainReceipt::new(intent_id, source_chain, dest_chain, success, None);

    assert_eq!(receipt.intent_id, intent_id);
    assert_eq!(receipt.source_chain, source_chain);
    assert_eq!(receipt.dest_chain, dest_chain);
    assert!(receipt.success);
    assert!(receipt.error.is_none());
}

#[test]
fn test_cross_chain_receipt_failure() {
    let intent_id = intent_id_n(1);

    let receipt = CrossChainReceipt::new(
        intent_id,
        1,
        2,
        false,
        Some("Insufficient funds".to_string()),
    );

    assert!(!receipt.success);
    assert_eq!(receipt.error, Some("Insufficient funds".to_string()));
}

#[test]
fn test_receipt_hash_determinism() {
    let intent_id = intent_id_n(1);

    let receipt1 = CrossChainReceipt::new(intent_id, 1, 2, true, None);
    let receipt2 = CrossChainReceipt::new(intent_id, 1, 2, true, None);

    assert_eq!(receipt1.receipt_hash, receipt2.receipt_hash);
}

#[test]
fn test_receipt_verification() {
    let intent_id = intent_id_n(1);
    let receipt = CrossChainReceipt::new(intent_id, 1, 2, true, None);

    // Receipt should verify against its own hash
    assert!(receipt.verify());
}

#[test]
fn test_receipt_tamper_detection() {
    let intent_id = intent_id_n(1);
    let mut receipt = CrossChainReceipt::new(intent_id, 1, 2, true, None);

    // Tamper with data
    receipt.success = false;

    // Should fail verification
    assert!(!receipt.verify());
}

#[test]
fn test_intent_expiration_check() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Intent that expires in the future
    let future_payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: now + 3600, // 1 hour from now
        chain_id: 1,
    };
    assert!(!future_payload.is_expired(now));

    // Intent that already expired
    let past_payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: now - 3600, // 1 hour ago
        chain_id: 1,
    };
    assert!(past_payload.is_expired(now));
}

#[test]
fn test_intent_serialization() {
    let payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    let intent = Intent::new(user_id_n(1), payload, vec![1, 2, 3, 4]);

    // Serialize
    let bytes = intent.to_bytes();
    assert!(!bytes.is_empty());

    // Deserialize
    let restored = Intent::from_bytes(&bytes);
    assert!(restored.is_ok());

    let restored = restored.unwrap();
    assert_eq!(restored.id, intent.id);
    assert_eq!(restored.sender, intent.sender);
}

#[test]
fn test_multi_chain_intent() {
    // Intent that spans multiple chains
    let payload = IntentPayload {
        intent_type: IntentType::CrossChainTransfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 1000,
            source_chain: 1, // chat chain
            dest_chain: 2,   // currency chain
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1, // origin chain
    };

    if let IntentType::CrossChainTransfer {
        source_chain,
        dest_chain,
        ..
    } = payload.intent_type
    {
        assert_eq!(source_chain, 1);
        assert_eq!(dest_chain, 2);
    }
}

#[test]
fn test_intent_validation_amount_positive() {
    let payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user_id_n(1),
            to: user_id_n(2),
            amount: 0, // Zero amount should be invalid
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    assert!(!payload.is_valid());
}

#[test]
fn test_intent_validation_sender_not_receiver() {
    let user = user_id_n(1);

    let payload = IntentPayload {
        intent_type: IntentType::Transfer {
            from: user,
            to: user, // Same as from - invalid
            amount: 1000,
            token: None,
        },
        nonce: 12345,
        expires_at: u64::MAX,
        chain_id: 1,
    };

    assert!(!payload.is_valid());
}

/// Property-based intent tests
#[cfg(feature = "proptest")]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_intent_id_unique(seed in any::<u64>()) {
            let id1 = IntentId::generate();
            let id2 = IntentId::generate();
            prop_assert_ne!(id1.0, id2.0);
        }

        #[test]
        fn prop_intent_hash_deterministic(
            from in 0u8..255,
            to in 0u8..255,
            amount in 1u64..u64::MAX,
            nonce in any::<u64>()
        ) {
            let payload = IntentPayload {
                intent_type: IntentType::Transfer {
                    from: user_id_n(from),
                    to: user_id_n(to),
                    amount,
                    token: None,
                },
                nonce,
                expires_at: u64::MAX,
                chain_id: 1,
            };

            let hash1 = payload.compute_hash();
            let hash2 = payload.compute_hash();
            prop_assert_eq!(hash1, hash2);
        }

        #[test]
        fn prop_threshold_satisfied_when_enough_attestations(threshold in 1usize..10) {
            let signers: Vec<[u8; 32]> = (0..threshold as u8 + 2)
                .map(|i| pubkey_n(i))
                .collect();
            let signer_set = SignerSet::new(signers.clone(), threshold as u8);

            let mut attestations = AttestationSet::new();
            for i in 0..threshold {
                attestations.add(Attestation::new(signers[i], [1u8; 32], vec![i as u8]));
            }

            prop_assert!(attestations.meets_threshold(&signer_set));
        }
    }
}
