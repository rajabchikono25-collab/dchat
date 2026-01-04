use crate::integration::mock_blockchain::{MockBlockchain, TransactionStatus, TransactionType};
/// Blockchain Integration Tests
///
/// Validates blockchain transaction submission, confirmation,
/// and state management across all SDKs.
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_registration_transaction() {
        let blockchain = MockBlockchain::new();
        let mut data = HashMap::new();
        data.insert("user_id".to_string(), "alice-12345".to_string());
        data.insert("public_key".to_string(), "alice-pub-key".to_string());

        let tx_id = blockchain
            .submit_transaction(TransactionType::RegisterUser, "alice".to_string(), data)
            .expect("Failed to submit registration transaction");

        let tx = blockchain
            .get_transaction(&tx_id)
            .expect("Transaction not found");

        assert_eq!(tx.tx_type, TransactionType::RegisterUser);
        assert_eq!(tx.sender, "alice");
        assert_eq!(tx.status, TransactionStatus::Confirmed);
        assert!(tx.confirmations > 0);
    }

    #[test]
    fn test_direct_message_transaction() {
        let blockchain = MockBlockchain::new();

        // Register both users first
        let mut alice_data = HashMap::new();
        alice_data.insert("user_id".to_string(), "alice-1".to_string());
        blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                alice_data,
            )
            .unwrap();

        let mut bob_data = HashMap::new();
        bob_data.insert("user_id".to_string(), "bob-1".to_string());
        blockchain
            .submit_transaction(TransactionType::RegisterUser, "bob".to_string(), bob_data)
            .unwrap();

        // Send message
        let mut msg_data = HashMap::new();
        msg_data.insert("recipient".to_string(), "bob-1".to_string());
        msg_data.insert("content_hash".to_string(), "msg-hash-123".to_string());

        let tx_id = blockchain
            .submit_transaction(
                TransactionType::SendDirectMessage,
                "alice".to_string(),
                msg_data,
            )
            .expect("Failed to submit message transaction");

        let tx = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.tx_type, TransactionType::SendDirectMessage);
        assert_eq!(tx.sender, "alice");
    }

    #[test]
    fn test_channel_creation_transaction() {
        let blockchain = MockBlockchain::new();

        // Register user first
        let mut alice_data = HashMap::new();
        alice_data.insert("user_id".to_string(), "alice-1".to_string());
        blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                alice_data,
            )
            .unwrap();

        // Create channel
        let mut channel_data = HashMap::new();
        channel_data.insert("channel_name".to_string(), "general".to_string());
        channel_data.insert("description".to_string(), "General discussion".to_string());

        let tx_id = blockchain
            .submit_transaction(
                TransactionType::CreateChannel,
                "alice".to_string(),
                channel_data,
            )
            .expect("Failed to submit channel creation transaction");

        let tx = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.tx_type, TransactionType::CreateChannel);
        assert_eq!(tx.sender, "alice");
    }

    #[test]
    fn test_transaction_confirmation_tracking() {
        let blockchain = MockBlockchain::with_confirmation_threshold(6);

        let mut data = HashMap::new();
        data.insert("user_id".to_string(), "alice-1".to_string());

        let tx_id = blockchain
            .submit_transaction(TransactionType::RegisterUser, "alice".to_string(), data)
            .unwrap();

        let tx = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.confirmations, 6); // Auto-confirmed to threshold

        // Advance blocks
        blockchain.advance_blocks(5);
        let tx_after = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx_after.confirmations, 11); // 6 initial + 5 from advance
    }

    #[test]
    fn test_block_height_tracking() {
        let blockchain = MockBlockchain::new();

        assert_eq!(blockchain.get_current_block(), 1);

        // Submit transaction (auto-confirms)
        let mut data = HashMap::new();
        data.insert("user_id".to_string(), "alice-1".to_string());
        let tx_id = blockchain
            .submit_transaction(TransactionType::RegisterUser, "alice".to_string(), data)
            .unwrap();

        let tx = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.block_height, 1);

        // Advance blocks and submit another
        blockchain.advance_blocks(3);

        let mut data2 = HashMap::new();
        data2.insert("user_id".to_string(), "bob-1".to_string());
        let tx_id2 = blockchain
            .submit_transaction(TransactionType::RegisterUser, "bob".to_string(), data2)
            .unwrap();

        let tx2 = blockchain.get_transaction(&tx_id2).unwrap();
        assert_eq!(tx2.block_height, 4);
    }

    #[test]
    fn test_transaction_filtering_by_type() {
        let blockchain = MockBlockchain::new();

        // Create mix of transaction types
        let mut data = HashMap::new();
        data.insert("user_id".to_string(), "alice-1".to_string());
        blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                data.clone(),
            )
            .unwrap();

        let mut data2 = HashMap::new();
        data2.insert("user_id".to_string(), "bob-1".to_string());
        blockchain
            .submit_transaction(TransactionType::RegisterUser, "bob".to_string(), data2)
            .unwrap();

        let mut data3 = HashMap::new();
        data3.insert("channel_name".to_string(), "general".to_string());
        blockchain
            .submit_transaction(TransactionType::CreateChannel, "alice".to_string(), data3)
            .unwrap();

        // Filter by type
        let reg_txs = blockchain.get_transactions_by_type(TransactionType::RegisterUser);
        assert_eq!(reg_txs.len(), 2);

        let channel_txs = blockchain.get_transactions_by_type(TransactionType::CreateChannel);
        assert_eq!(channel_txs.len(), 1);
    }

    #[test]
    fn test_transaction_filtering_by_sender() {
        let blockchain = MockBlockchain::new();

        // Alice creates multiple transactions
        let mut data = HashMap::new();
        data.insert("user_id".to_string(), "alice-1".to_string());
        blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                data.clone(),
            )
            .unwrap();

        let mut data2 = HashMap::new();
        data2.insert("channel_name".to_string(), "general".to_string());
        blockchain
            .submit_transaction(TransactionType::CreateChannel, "alice".to_string(), data2)
            .unwrap();

        // Bob creates one
        let mut bob_data = HashMap::new();
        bob_data.insert("user_id".to_string(), "bob-1".to_string());
        blockchain
            .submit_transaction(TransactionType::RegisterUser, "bob".to_string(), bob_data)
            .unwrap();

        // Filter by sender
        let alice_txs = blockchain.get_transactions_by_sender("alice");
        assert_eq!(alice_txs.len(), 2);

        let bob_txs = blockchain.get_transactions_by_sender("bob");
        assert_eq!(bob_txs.len(), 1);
    }

    #[test]
    fn test_blockchain_statistics() {
        let blockchain = MockBlockchain::new();

        // Create various transactions
        let mut data1 = HashMap::new();
        data1.insert("user_id".to_string(), "alice-1".to_string());
        blockchain
            .submit_transaction(TransactionType::RegisterUser, "alice".to_string(), data1)
            .unwrap();

        let mut data2 = HashMap::new();
        data2.insert("user_id".to_string(), "bob-1".to_string());
        blockchain
            .submit_transaction(TransactionType::RegisterUser, "bob".to_string(), data2)
            .unwrap();

        let mut data3 = HashMap::new();
        data3.insert("channel_name".to_string(), "general".to_string());
        blockchain
            .submit_transaction(TransactionType::CreateChannel, "alice".to_string(), data3)
            .unwrap();

        let stats = blockchain.get_stats();
        assert_eq!(stats.total_transactions, 3);
        assert_eq!(stats.confirmed_transactions, 3);
        assert_eq!(stats.pending_transactions, 0);
        assert_eq!(stats.current_block_height, 1);

        // Check transaction type breakdown
        assert_eq!(
            stats
                .transactions_by_type
                .get("RegisterUser")
                .copied()
                .unwrap_or(0),
            2
        );
        assert_eq!(
            stats
                .transactions_by_type
                .get("CreateChannel")
                .copied()
                .unwrap_or(0),
            1
        );
    }

    #[test]
    fn test_transaction_sequence_ordering() {
        let blockchain = MockBlockchain::new();

        let mut data_ids: Vec<String> = Vec::new();

        // Submit 5 transactions
        for i in 0..5 {
            let mut data = HashMap::new();
            data.insert("user_id".to_string(), format!("user-{}", i));
            let tx_id = blockchain
                .submit_transaction(TransactionType::RegisterUser, format!("user-{}", i), data)
                .unwrap();
            data_ids.push(tx_id);
        }

        // Verify all transactions are present and in order
        let all_txs = blockchain.get_all_transactions();
        assert_eq!(all_txs.len(), 5);

        for (i, tx) in all_txs.iter().enumerate() {
            assert_eq!(tx.tx_id, data_ids[i]);
        }
    }

    #[test]
    fn test_multiple_users_transaction_flow() {
        let blockchain = MockBlockchain::new();

        // Simulate real-world scenario: multiple users registering and messaging
        let users = vec!["alice", "bob", "charlie"];

        // Step 1: Register users
        for user in &users {
            let mut data = HashMap::new();
            data.insert("user_id".to_string(), format!("{}-id", user));
            data.insert("public_key".to_string(), format!("{}-pub-key", user));
            blockchain
                .submit_transaction(TransactionType::RegisterUser, user.to_string(), data)
                .unwrap();
        }

        // Step 2: Create channels
        for i in 0..2 {
            let mut data = HashMap::new();
            data.insert("channel_name".to_string(), format!("channel-{}", i));
            blockchain
                .submit_transaction(TransactionType::CreateChannel, "alice".to_string(), data)
                .unwrap();
        }

        // Step 3: Exchange messages
        for user in &users {
            let mut data = HashMap::new();
            data.insert("recipient".to_string(), "bob-id".to_string());
            data.insert("message_hash".to_string(), format!("{}-msg", user));
            blockchain
                .submit_transaction(TransactionType::SendDirectMessage, user.to_string(), data)
                .unwrap();
        }

        // Verify final state
        let stats = blockchain.get_stats();
        assert_eq!(stats.total_transactions, 3 + 2 + 3); // 3 registrations + 2 channels + 3 messages
        assert_eq!(stats.confirmed_transactions, 8);

        // Verify transaction counts by sender
        let alice_txs = blockchain.get_transactions_by_sender("alice");
        assert_eq!(alice_txs.len(), 3); // 1 registration + 2 channels
    }
}
