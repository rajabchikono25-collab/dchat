/// User Management Flow Integration Tests
///
/// Validates user creation, updates, and operations work correctly
/// with on-chain state management across all SDKs.
use crate::integration::mock_blockchain::{MockBlockchain, TransactionType};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    /// Represents a user state as would be stored
    struct User {
        user_id: String,
        username: String,
        public_key: String,
        created_at: String,
        on_chain_confirmed: bool,
        block_height: u64,
    }

    #[test]
    fn test_single_user_creation_flow() {
        let blockchain = MockBlockchain::new();

        // Simulate user creation in all SDKs
        let user_id = "user-alice-12345";
        let username = "alice";
        let public_key = "alice-public-key";

        let mut data = HashMap::new();
        data.insert("user_id".to_string(), user_id.to_string());
        data.insert("username".to_string(), username.to_string());
        data.insert("public_key".to_string(), public_key.to_string());

        let tx_id = blockchain
            .submit_transaction(TransactionType::RegisterUser, username.to_string(), data)
            .expect("Failed to create user");

        // Verify user is on-chain
        let tx = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.sender, username);
        assert!(tx.confirmations > 0);
    }

    #[test]
    fn test_multiple_users_creation() {
        let blockchain = MockBlockchain::new();
        let users = vec![
            ("alice", "alice-12345"),
            ("bob", "bob-67890"),
            ("charlie", "charlie-11111"),
        ];

        let mut tx_ids = Vec::new();

        for (username, user_id) in &users {
            let mut data = HashMap::new();
            data.insert("user_id".to_string(), user_id.to_string());
            data.insert("username".to_string(), username.to_string());
            data.insert("public_key".to_string(), format!("{}-pub", username));

            let tx_id = blockchain
                .submit_transaction(TransactionType::RegisterUser, username.to_string(), data)
                .unwrap();
            tx_ids.push(tx_id);
        }

        // Verify all users created
        assert_eq!(tx_ids.len(), 3);

        // Verify each transaction
        for (i, (username, user_id)) in users.iter().enumerate() {
            let tx = blockchain.get_transaction(&tx_ids[i]).unwrap();
            assert_eq!(tx.sender, *username);
            assert_eq!(tx.tx_type, TransactionType::RegisterUser);
        }
    }

    #[test]
    fn test_user_can_send_message_to_other_user() {
        let blockchain = MockBlockchain::new();

        // Step 1: Create alice
        let mut alice_data = HashMap::new();
        alice_data.insert("user_id".to_string(), "alice-1".to_string());
        alice_data.insert("username".to_string(), "alice".to_string());
        let alice_tx = blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                alice_data,
            )
            .unwrap();

        // Step 2: Create bob
        let mut bob_data = HashMap::new();
        bob_data.insert("user_id".to_string(), "bob-1".to_string());
        bob_data.insert("username".to_string(), "bob".to_string());
        let bob_tx = blockchain
            .submit_transaction(TransactionType::RegisterUser, "bob".to_string(), bob_data)
            .unwrap();

        // Verify both created
        assert!(blockchain.get_transaction(&alice_tx).is_some());
        assert!(blockchain.get_transaction(&bob_tx).is_some());

        // Step 3: Alice sends message to bob
        let mut msg_data = HashMap::new();
        msg_data.insert("recipient_id".to_string(), "bob-1".to_string());
        msg_data.insert("content_hash".to_string(), "msg-abc123".to_string());

        let msg_tx = blockchain
            .submit_transaction(
                TransactionType::SendDirectMessage,
                "alice".to_string(),
                msg_data,
            )
            .unwrap();

        let msg = blockchain.get_transaction(&msg_tx).unwrap();
        assert_eq!(msg.tx_type, TransactionType::SendDirectMessage);
        assert_eq!(msg.sender, "alice");
    }

    #[test]
    fn test_user_can_create_channel() {
        let blockchain = MockBlockchain::new();

        // Step 1: Create user
        let mut user_data = HashMap::new();
        user_data.insert("user_id".to_string(), "alice-1".to_string());
        let user_tx = blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                user_data,
            )
            .unwrap();

        assert!(blockchain.get_transaction(&user_tx).is_some());

        // Step 2: User creates channel
        let mut channel_data = HashMap::new();
        channel_data.insert("channel_id".to_string(), "channel-1".to_string());
        channel_data.insert("channel_name".to_string(), "general".to_string());
        channel_data.insert("description".to_string(), "General discussion".to_string());

        let channel_tx = blockchain
            .submit_transaction(
                TransactionType::CreateChannel,
                "alice".to_string(),
                channel_data,
            )
            .unwrap();

        let channel = blockchain.get_transaction(&channel_tx).unwrap();
        assert_eq!(channel.tx_type, TransactionType::CreateChannel);
        assert_eq!(channel.sender, "alice");
    }

    #[test]
    fn test_user_can_post_to_channel() {
        let blockchain = MockBlockchain::new();

        // Step 1: Create user
        let mut user_data = HashMap::new();
        user_data.insert("user_id".to_string(), "alice-1".to_string());
        blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                user_data,
            )
            .unwrap();

        // Step 2: Create channel
        let mut channel_data = HashMap::new();
        channel_data.insert("channel_id".to_string(), "channel-1".to_string());
        channel_data.insert("channel_name".to_string(), "general".to_string());
        blockchain
            .submit_transaction(
                TransactionType::CreateChannel,
                "alice".to_string(),
                channel_data,
            )
            .unwrap();

        // Step 3: Post to channel
        let mut post_data = HashMap::new();
        post_data.insert("channel_id".to_string(), "channel-1".to_string());
        post_data.insert("content_hash".to_string(), "post-hash-123".to_string());

        let post_tx = blockchain
            .submit_transaction(
                TransactionType::PostToChannel,
                "alice".to_string(),
                post_data,
            )
            .unwrap();

        let post = blockchain.get_transaction(&post_tx).unwrap();
        assert_eq!(post.tx_type, TransactionType::PostToChannel);
        assert_eq!(post.sender, "alice");
    }

    #[test]
    fn test_user_activity_history() {
        let blockchain = MockBlockchain::new();

        // User performs multiple activities
        let activities = vec![
            (TransactionType::RegisterUser, "Register"),
            (TransactionType::SendDirectMessage, "Message 1"),
            (TransactionType::SendDirectMessage, "Message 2"),
            (TransactionType::CreateChannel, "Create channel"),
            (TransactionType::PostToChannel, "Post 1"),
        ];

        for (tx_type, _label) in &activities {
            let mut data = HashMap::new();
            data.insert("content".to_string(), "data".to_string());
            blockchain
                .submit_transaction(tx_type.clone(), "alice".to_string(), data)
                .unwrap();
        }

        // Verify user history
        let user_txs = blockchain.get_transactions_by_sender("alice");
        assert_eq!(user_txs.len(), 5);

        // Verify transaction types in order
        assert_eq!(user_txs[0].tx_type, TransactionType::RegisterUser);
        assert_eq!(user_txs[1].tx_type, TransactionType::SendDirectMessage);
        assert_eq!(user_txs[2].tx_type, TransactionType::SendDirectMessage);
        assert_eq!(user_txs[3].tx_type, TransactionType::CreateChannel);
        assert_eq!(user_txs[4].tx_type, TransactionType::PostToChannel);
    }

    #[test]
    fn test_user_confirmation_workflow() {
        let blockchain = MockBlockchain::with_confirmation_threshold(6);

        // Create user
        let mut data = HashMap::new();
        data.insert("user_id".to_string(), "alice-1".to_string());

        let tx_id = blockchain
            .submit_transaction(TransactionType::RegisterUser, "alice".to_string(), data)
            .unwrap();

        // Initially confirmed (auto-confirm)
        let tx = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.confirmations, 6);

        // After 5 more blocks
        blockchain.advance_blocks(5);
        let tx = blockchain.get_transaction(&tx_id).unwrap();
        assert_eq!(tx.confirmations, 11);

        // Transaction is considered "confirmed" once confirmations >= threshold
        assert!(tx.confirmations >= 6);
    }

    #[test]
    fn test_user_message_exchange() {
        let blockchain = MockBlockchain::new();

        // Create two users
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

        // Alice sends message to Bob
        let mut msg1_data = HashMap::new();
        msg1_data.insert("recipient".to_string(), "bob-1".to_string());
        msg1_data.insert("content".to_string(), "Hello Bob".to_string());
        let msg1_tx = blockchain
            .submit_transaction(
                TransactionType::SendDirectMessage,
                "alice".to_string(),
                msg1_data,
            )
            .unwrap();

        // Bob sends message to Alice
        let mut msg2_data = HashMap::new();
        msg2_data.insert("recipient".to_string(), "alice-1".to_string());
        msg2_data.insert("content".to_string(), "Hi Alice".to_string());
        let msg2_tx = blockchain
            .submit_transaction(
                TransactionType::SendDirectMessage,
                "bob".to_string(),
                msg2_data,
            )
            .unwrap();

        // Verify messages
        let msg1 = blockchain.get_transaction(&msg1_tx).unwrap();
        assert_eq!(msg1.sender, "alice");

        let msg2 = blockchain.get_transaction(&msg2_tx).unwrap();
        assert_eq!(msg2.sender, "bob");

        // Verify message counts
        let alice_msgs = blockchain
            .get_transactions_by_sender("alice")
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::SendDirectMessage)
            .count();
        assert_eq!(alice_msgs, 1);

        let bob_msgs = blockchain
            .get_transactions_by_sender("bob")
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::SendDirectMessage)
            .count();
        assert_eq!(bob_msgs, 1);
    }

    #[test]
    fn test_user_channel_administration() {
        let blockchain = MockBlockchain::new();

        // Create user
        let mut user_data = HashMap::new();
        user_data.insert("user_id".to_string(), "alice-1".to_string());
        blockchain
            .submit_transaction(
                TransactionType::RegisterUser,
                "alice".to_string(),
                user_data,
            )
            .unwrap();

        // Create multiple channels
        for i in 0..3 {
            let mut channel_data = HashMap::new();
            channel_data.insert("channel_id".to_string(), format!("channel-{}", i));
            channel_data.insert("channel_name".to_string(), format!("Channel {}", i));
            blockchain
                .submit_transaction(
                    TransactionType::CreateChannel,
                    "alice".to_string(),
                    channel_data,
                )
                .unwrap();
        }

        // Post to each channel
        for i in 0..3 {
            let mut post_data = HashMap::new();
            post_data.insert("channel_id".to_string(), format!("channel-{}", i));
            post_data.insert("content".to_string(), format!("Post {}", i));
            blockchain
                .submit_transaction(
                    TransactionType::PostToChannel,
                    "alice".to_string(),
                    post_data,
                )
                .unwrap();
        }

        // Verify activity
        let alice_txs = blockchain.get_transactions_by_sender("alice");
        let channel_creates = alice_txs
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::CreateChannel)
            .count();
        let posts = alice_txs
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::PostToChannel)
            .count();

        assert_eq!(channel_creates, 3);
        assert_eq!(posts, 3);
        assert_eq!(alice_txs.len(), 7); // 1 registration + 3 channels + 3 posts
    }

    #[test]
    fn test_concurrent_user_operations() {
        let blockchain = MockBlockchain::new();
        let users = vec!["alice", "bob", "charlie", "dave"];

        // All users register
        for user in &users {
            let mut data = HashMap::new();
            data.insert("user_id".to_string(), format!("{}-id", user));
            blockchain
                .submit_transaction(TransactionType::RegisterUser, user.to_string(), data)
                .unwrap();
        }

        // All users create channels
        for (i, user) in users.iter().enumerate() {
            let mut data = HashMap::new();
            data.insert("channel_id".to_string(), format!("channel-{}", i));
            data.insert("channel_name".to_string(), format!("{}'s channel", user));
            blockchain
                .submit_transaction(TransactionType::CreateChannel, user.to_string(), data)
                .unwrap();
        }

        // Each user sends a message to next user (round-robin)
        for i in 0..users.len() {
            let sender = users[i];
            let recipient = users[(i + 1) % users.len()];

            let mut data = HashMap::new();
            data.insert("recipient".to_string(), format!("{}-id", recipient));
            data.insert(
                "content".to_string(),
                format!("{} -> {}", sender, recipient),
            );
            blockchain
                .submit_transaction(TransactionType::SendDirectMessage, sender.to_string(), data)
                .unwrap();
        }

        // Verify total transactions
        let stats = blockchain.get_stats();
        assert_eq!(stats.total_transactions, 4 + 4 + 4); // 4 registrations + 4 channels + 4 messages

        // Verify each user has activity
        for user in &users {
            let txs = blockchain.get_transactions_by_sender(user);
            assert_eq!(txs.len(), 3); // 1 registration + 1 channel + 1 message
        }
    }
}
