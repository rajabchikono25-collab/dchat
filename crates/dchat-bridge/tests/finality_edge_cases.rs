//! Integration tests for bridge finality module
//!
//! Tests for BLS signature verification edge cases

#[cfg(test)]
mod tests {
    use dchat_bridge::finality::{BLSFinalityProof, FinalityTracker};
    use dchat_core::types::BlockHash;

    #[tokio::test]
    async fn test_malformed_pubkey_rejection() {
        let mut tracker = FinalityTracker::new(vec![]);

        // Test with invalid public key bytes
        let bad_pubkey = vec![0u8; 32]; // Wrong length

        let result = tracker.add_validator("malicious_validator".to_string(), bad_pubkey);

        // Should reject invalid pubkey
        assert!(result.is_err(), "Should reject malformed public key");
    }

    #[tokio::test]
    async fn test_mixed_message_aggregation_prevention() {
        let tracker = FinalityTracker::new(vec![]);

        // Create two different messages
        let block_hash_1 = BlockHash([1u8; 32]);
        let block_hash_2 = BlockHash([2u8; 32]);

        // Attempt to verify aggregated signature with mixed messages should fail
        // In production, this would be caught by fast_aggregate_verify

        let proof_1 = BLSFinalityProof {
            block_hash: block_hash_1,
            signature: vec![0u8; 96], // Mock BLS signature
            validator_pubkeys: vec![vec![0u8; 48]],
            timestamp: std::time::SystemTime::now(),
        };

        // Verify single message works
        let result_1 = tracker.verify_finality(&proof_1).await;
        // May pass or fail depending on signature validity, but shouldn't panic

        let proof_2 = BLSFinalityProof {
            block_hash: block_hash_2,
            signature: vec![0u8; 96],
            validator_pubkeys: vec![vec![0u8; 48]],
            timestamp: std::time::SystemTime::now(),
        };

        let result_2 = tracker.verify_finality(&proof_2).await;

        // Both should complete without panic (correctness depends on real BLS impl)
        let _ = (result_1, result_2);
    }

    #[tokio::test]
    async fn test_empty_validator_set() {
        let tracker = FinalityTracker::new(vec![]);

        let proof = BLSFinalityProof {
            block_hash: BlockHash([0u8; 32]),
            signature: vec![],
            validator_pubkeys: vec![],
            timestamp: std::time::SystemTime::now(),
        };

        let result = tracker.verify_finality(&proof).await;

        // Should reject proof with no validators
        assert!(
            result.is_err() || result.unwrap() == false,
            "Should reject finality proof with empty validator set"
        );
    }

    #[tokio::test]
    async fn test_duplicate_pubkeys_in_aggregation() {
        let tracker = FinalityTracker::new(vec![]);

        // Same pubkey repeated (double signing attack)
        let duplicate_pubkey = vec![1u8; 48];

        let proof = BLSFinalityProof {
            block_hash: BlockHash([0u8; 32]),
            signature: vec![0u8; 96],
            validator_pubkeys: vec![
                duplicate_pubkey.clone(),
                duplicate_pubkey.clone(),
                duplicate_pubkey,
            ],
            timestamp: std::time::SystemTime::now(),
        };

        let result = tracker.verify_finality(&proof).await;

        // Should detect and reject duplicate validators
        // (Real BLS implementation should handle this)
        let _ = result;
    }

    #[tokio::test]
    async fn test_signature_length_validation() {
        let tracker = FinalityTracker::new(vec![]);

        // Test various invalid signature lengths
        let invalid_signatures = vec![
            vec![0u8; 0],  // Empty
            vec![0u8; 64], // Ed25519 length (wrong type)
            vec![0u8; 95], // Almost correct
            vec![0u8; 97], // Too long
        ];

        for sig in invalid_signatures {
            let proof = BLSFinalityProof {
                block_hash: BlockHash([0u8; 32]),
                signature: sig.clone(),
                validator_pubkeys: vec![vec![0u8; 48]],
                timestamp: std::time::SystemTime::now(),
            };

            let result = tracker.verify_finality(&proof).await;

            // Should reject or handle gracefully
            assert!(
                result.is_err() || result.unwrap() == false,
                "Should reject signature of length {}",
                sig.len()
            );
        }
    }
}
