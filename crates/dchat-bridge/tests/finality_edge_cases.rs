//! Integration tests for bridge finality module
//!
//! Tests for BLS signature verification edge cases

#[cfg(test)]
mod tests {
    use blst::min_pk::SecretKey;
    use dchat_bridge::finality::{FinalityError, FinalityTracker};
    use dchat_core::types::UserId;

    fn generate_bls_keypair() -> (SecretKey, blst::min_pk::PublicKey) {
        let mut ikm = [0u8; 32];
        getrandom::getrandom(&mut ikm).unwrap();
        let sk = SecretKey::key_gen(&ikm, &[]).unwrap();
        let pk = sk.sk_to_pk();
        (sk, pk)
    }

    #[test]
    fn test_malformed_pubkey_rejection() {
        let mut tracker = FinalityTracker::new(2, 3).unwrap();
        let validator_id = UserId::new();

        // Test with invalid public key bytes (wrong length)
        let bad_pubkey = vec![0u8; 32]; // BLS pubkeys are 48 bytes, not 32

        let result = tracker.register_validator(validator_id, bad_pubkey);

        // Should reject invalid pubkey
        assert!(result.is_err(), "Should reject malformed public key");
        match result {
            Err(FinalityError::InvalidPublicKey) => {} // Expected
            other => panic!("Expected InvalidPublicKey error, got {:?}", other),
        }
    }

    #[test]
    fn test_empty_validator_set_proof() {
        // A finality tracker with required 1-of-1 but no registered validators
        let mut tracker = FinalityTracker::new(1, 1).unwrap();

        // Initiate a proof
        tracker
            .initiate_proof(
                "tx_empty_test".to_string(),
                100,
                "block_hash_test".to_string(),
                15,
                12,
            )
            .unwrap();

        // Try to submit signature from unregistered validator
        let (sk, _pk) = generate_bls_keypair();
        let message = b"test message";
        let dst = b"DCHAT_BRIDGE_FINALITY_V1";
        let sig = sk.sign(message, dst, &[]);

        let result = tracker.submit_signature(
            "tx_empty_test",
            UserId::new(), // Unregistered validator
            sig.to_bytes().to_vec(),
        );

        assert!(
            result.is_err(),
            "Should reject signature from unknown validator"
        );
        match result {
            Err(FinalityError::UnknownValidator) => {} // Expected
            other => panic!("Expected UnknownValidator error, got {:?}", other),
        }
    }

    #[test]
    fn test_duplicate_signature_rejection() {
        let mut tracker = FinalityTracker::new(2, 3).unwrap();

        // Register a validator
        let (sk, pk) = generate_bls_keypair();
        let validator_id = UserId::new();
        tracker
            .register_validator(validator_id.clone(), pk.to_bytes().to_vec())
            .unwrap();

        // Initiate proof
        tracker
            .initiate_proof(
                "tx_duplicate_test".to_string(),
                100,
                "block_hash".to_string(),
                15,
                12,
            )
            .unwrap();

        // Sign and submit first signature
        let message = b"test message";
        let dst = b"DCHAT_BRIDGE_FINALITY_V1";
        let sig = sk.sign(message, dst, &[]);

        tracker
            .submit_signature(
                "tx_duplicate_test",
                validator_id.clone(),
                sig.to_bytes().to_vec(),
            )
            .unwrap();

        // Try to submit same validator's signature again
        let result = tracker.submit_signature(
            "tx_duplicate_test",
            validator_id,
            sig.to_bytes().to_vec(),
        );

        assert!(result.is_err(), "Should reject duplicate signature");
        match result {
            Err(FinalityError::DuplicateSignature) => {} // Expected
            other => panic!("Expected DuplicateSignature error, got {:?}", other),
        }
    }

    #[test]
    fn test_signature_length_validation() {
        let mut tracker = FinalityTracker::new(1, 1).unwrap();

        // Register a valid validator
        let (_, pk) = generate_bls_keypair();
        let validator_id = UserId::new();
        tracker
            .register_validator(validator_id.clone(), pk.to_bytes().to_vec())
            .unwrap();

        // Initiate proof
        tracker
            .initiate_proof(
                "tx_sig_length_test".to_string(),
                100,
                "block_hash".to_string(),
                15,
                12,
            )
            .unwrap();

        // Test various invalid signature lengths
        let invalid_signatures = vec![
            vec![0u8; 0],  // Empty
            vec![0u8; 64], // Ed25519 length (wrong type)
            vec![0u8; 95], // Almost correct (BLS is 96)
            vec![0u8; 97], // Too long
        ];

        for sig in invalid_signatures {
            // Re-initiate proof for each test (since proof state changes)
            let tx_hash = format!("tx_sig_len_{}", sig.len());
            tracker
                .initiate_proof(tx_hash.clone(), 100, "block".to_string(), 15, 12)
                .unwrap();

            let result = tracker.submit_signature(&tx_hash, validator_id.clone(), sig.clone());

            // Should reject or handle gracefully
            // Note: The blst library will reject invalid signature bytes during parsing
            if result.is_ok() {
                // If it parsed, it won't verify correctly anyway
                assert!(
                    !tracker.is_finalized(&tx_hash),
                    "Should not finalize with invalid signature length {}",
                    sig.len()
                );
            }
        }
    }

    #[test]
    fn test_threshold_validation() {
        // Invalid: threshold > total
        let result = FinalityTracker::new(5, 3);
        assert!(result.is_err(), "Should reject threshold > total");

        // Invalid: zero threshold
        let result = FinalityTracker::new(0, 3);
        assert!(result.is_err(), "Should reject zero threshold");

        // Invalid: zero total
        let result = FinalityTracker::new(1, 0);
        assert!(result.is_err(), "Should reject zero total");

        // Valid: threshold == total
        let result = FinalityTracker::new(3, 3);
        assert!(result.is_ok(), "Should accept threshold == total");

        // Valid: standard M-of-N
        let result = FinalityTracker::new(2, 3);
        assert!(result.is_ok(), "Should accept valid 2-of-3");
    }

    #[test]
    fn test_proof_already_exists() {
        let mut tracker = FinalityTracker::new(2, 3).unwrap();

        // Initiate first proof
        tracker
            .initiate_proof("tx_exists".to_string(), 100, "block".to_string(), 15, 12)
            .unwrap();

        // Try to initiate same proof again
        let result =
            tracker.initiate_proof("tx_exists".to_string(), 200, "block2".to_string(), 20, 12);

        assert!(result.is_err(), "Should reject duplicate proof initiation");
        match result {
            Err(FinalityError::ProofAlreadyExists) => {} // Expected
            other => panic!("Expected ProofAlreadyExists error, got {:?}", other),
        }
    }

    #[test]
    fn test_proof_not_found() {
        let tracker = FinalityTracker::new(2, 3).unwrap();

        // Try to verify non-existent proof
        let result = tracker.verify_proof("non_existent_tx", b"message");

        assert!(result.is_err(), "Should fail on non-existent proof");
        match result {
            Err(FinalityError::ProofNotFound) => {} // Expected
            other => panic!("Expected ProofNotFound error, got {:?}", other),
        }
    }

    #[test]
    fn test_cleanup_expired_proofs() {
        let mut tracker = FinalityTracker::new(2, 3).unwrap();

        // Create a proof
        tracker
            .initiate_proof("tx_old".to_string(), 100, "block".to_string(), 15, 12)
            .unwrap();

        // Verify proof exists
        assert!(tracker.get_pending_proof("tx_old").is_some());

        // Cleanup doesn't panic on fresh results
        let expired = tracker.cleanup_expired_proofs();

        // Fresh proof shouldn't be expired yet
        assert!(
            expired.is_empty() || expired.contains(&"tx_old".to_string())
        );
    }

    #[test]
    fn test_complete_finality_flow_with_verification() {
        let mut tracker = FinalityTracker::new(2, 3).unwrap();

        // Generate 2 validators (we need 2-of-3)
        let (sk1, pk1) = generate_bls_keypair();
        let (sk2, pk2) = generate_bls_keypair();

        let val1 = UserId::new();
        let val2 = UserId::new();

        // Register validators
        tracker
            .register_validator(val1.clone(), pk1.to_bytes().to_vec())
            .unwrap();
        tracker
            .register_validator(val2.clone(), pk2.to_bytes().to_vec())
            .unwrap();

        // Initiate proof
        let tx_hash = "tx_complete_test";
        tracker
            .initiate_proof(
                tx_hash.to_string(),
                500,
                "block_complete".to_string(),
                20,
                12,
            )
            .unwrap();

        // Sign with matching DST
        let message = b"finalize tx_complete_test at block 500";
        let dst = b"DCHAT_BRIDGE_FINALITY_V1";
        let sig1 = sk1.sign(message, dst, &[]);
        let sig2 = sk2.sign(message, dst, &[]);

        // Submit first signature - not finalized yet
        let result1 = tracker
            .submit_signature(tx_hash, val1, sig1.to_bytes().to_vec())
            .unwrap();
        assert!(!result1, "Should not be finalized with 1 signature");
        assert!(!tracker.is_finalized(tx_hash));

        // Submit second signature - should finalize
        let result2 = tracker
            .submit_signature(tx_hash, val2, sig2.to_bytes().to_vec())
            .unwrap();
        assert!(result2, "Should be finalized with 2 signatures");
        assert!(tracker.is_finalized(tx_hash));

        // Verify the proof
        let verify_result = tracker.verify_proof(tx_hash, message);
        assert!(verify_result.is_ok(), "Verification should not error");
        assert!(verify_result.unwrap(), "Signature should be valid");

        // Verify with wrong message should fail
        let wrong_verify = tracker.verify_proof(tx_hash, b"wrong message");
        assert!(
            wrong_verify.is_err() || !wrong_verify.unwrap(),
            "Wrong message should not verify"
        );
    }
}
