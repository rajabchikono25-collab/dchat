//! End-to-end system tests

use dchat::prelude::*;
use dchat_crypto::keys::PrivateKey;
use dchat_messaging::delivery::DeliveryStatus;
use std::time::SystemTime;

#[tokio::test]
async fn test_complete_system_initialization() {
    // Test that all components can be initialized together
    let _config = Config::default();
    let _event_bus = EventBus::new(1000);
    
    // Core components
    let keypair = KeyPair::generate();
    let _identity_manager = IdentityManager::new();
    let _device_manager = DeviceManager::new();
    
    // Crypto components
    let master_key = PrivateKey::generate();
    let _handshake_manager = HandshakeManager::new(master_key.clone(), 30);
    let rotation_policy = RotationPolicy {
        max_age_hours: 168,
        max_messages_per_key: 10_000,
        rotate_on_events: vec![],
    };
    let _rotation_manager = KeyRotationManager::new(master_key, rotation_policy);
    
    // Messaging components
    let message_order = MessageOrder::new();
    let _delivery_tracker = DeliveryTracker::new(3);
    let offline_queue = OfflineQueue::default();
    
    // Verify all components are initialized
    assert!(keypair.public_key().as_bytes().len() > 0);
    assert_eq!(message_order.pending_count("test"), 0);
    assert_eq!(offline_queue.total_pending(), 0);
}

#[tokio::test]
async fn test_end_to_end_encrypted_message() {
    use dchat_crypto::noise::{NoiseHandshake, NoisePattern};
    
    // Alice and Bob keypairs
    let alice_keypair = KeyPair::generate();
    let bob_keypair = KeyPair::generate();
    
    // Perform Noise XX handshake
    let mut alice_handshake = NoiseHandshake::initiate(
        NoisePattern::XX,
        alice_keypair.private_key(),
        None,
    ).unwrap();
    
    let mut bob_handshake = NoiseHandshake::respond(
        NoisePattern::XX,
        bob_keypair.private_key(),
    ).unwrap();
    
    // Handshake step 1: Alice -> Bob
    let msg1 = alice_handshake.write_message(&[]).unwrap();
    let _ = bob_handshake.read_message(&msg1).unwrap();
    
    // Handshake step 2: Bob -> Alice
    let msg2 = bob_handshake.write_message(&[]).unwrap();
    let _ = alice_handshake.read_message(&msg2).unwrap();
    
    // Handshake step 3: Alice -> Bob
    let msg3 = alice_handshake.write_message(&[]).unwrap();
    let _ = bob_handshake.read_message(&msg3).unwrap();
    
    // Get transport sessions
    let mut alice_session = alice_handshake.into_transport_mode().unwrap();
    let mut bob_session = bob_handshake.into_transport_mode().unwrap();
    
    // Send encrypted message
    let plaintext = b"Hello Bob, this is Alice!";
    let ciphertext = alice_session.encrypt(plaintext).unwrap();
    let decrypted = bob_session.decrypt(&ciphertext).unwrap();
    
    assert_eq!(plaintext, decrypted.as_slice());
}

#[tokio::test]
async fn test_multi_user_conversation() {
    let alice_id = UserId(uuid::Uuid::new_v4());
    let bob_id = UserId(uuid::Uuid::new_v4());
    let charlie_id = UserId(uuid::Uuid::new_v4());
    
    let mut message_order = MessageOrder::new();
    let conv_id = format!("group-{}", uuid::Uuid::new_v4());
    
    // Simulate messages from different users
    let messages = vec![
        (alice_id.clone(), "Hello everyone!"),
        (bob_id.clone(), "Hi Alice!"),
        (charlie_id, "Hey folks!"),
        (alice_id.clone(), "How are you all?"),
        (bob_id.clone(), "Great!"),
    ];
    
    for (seq, (_sender, _content)) in messages.iter().enumerate() {
        let msg_id = MessageId(uuid::Uuid::new_v4());
        let sequence = SequenceNumber(seq as u64);
        
        assert!(message_order.register_message(conv_id.clone(), sequence, msg_id));
    }
    
    // Verify no pending messages
    assert_eq!(message_order.pending_count(&conv_id), 0);
}

#[test]
fn test_hierarchical_key_derivation() {
    let master_key = PrivateKey::generate();
    
    // Derive device keys
    let device1_key = IdentityDerivation::derive_device_key(&master_key, 0).unwrap();
    let device2_key = IdentityDerivation::derive_device_key(&master_key, 1).unwrap();
    
    // Keys should be different
    assert_ne!(device1_key.public_key().as_bytes(), device2_key.public_key().as_bytes());
    
    // Derive conversation keys
    let conv1_key = IdentityDerivation::derive_conversation_key(&master_key, 0).unwrap();
    let conv2_key = IdentityDerivation::derive_conversation_key(&master_key, 1).unwrap();
    
    // Keys should be different
    assert_ne!(conv1_key.public_key().as_bytes().len(), 0);
    assert_ne!(conv1_key.public_key().as_bytes(), conv2_key.public_key().as_bytes());
}

#[test]
fn test_reputation_tracking() {
    use dchat_core::types::ReputationScore;
    
    let mut reputation = ReputationScore {
        total: 225,
        messaging: 100,
        governance: 50,
        relay: 75,
        last_updated: chrono::Utc::now(),
    };
    
    // Simulate earning reputation
    reputation.messaging += 10;
    reputation.governance += 5;
    reputation.relay += 15;
    
    assert_eq!(reputation.messaging, 110);
    assert_eq!(reputation.governance, 55);
    assert_eq!(reputation.relay, 90);
    
    // Calculate total reputation
    let total = reputation.messaging + reputation.governance + reputation.relay;
    assert_eq!(total, 255);
}

#[tokio::test]
async fn test_event_bus_pub_sub() {
    let event_bus = EventBus::new(100);
    
    // Subscribe to events
    let mut subscriber = event_bus.subscribe();
    
    // Publish events
    let alice_keypair = KeyPair::generate();
    event_bus.publish(Event::UserRegistered {
        user_id: UserId(uuid::Uuid::new_v4()),
        username: "alice".to_string(),
        public_key: alice_keypair.public_key().to_core_public_key(),
    }).await.unwrap();
    
    let message = dchat_core::types::Message {
        id: MessageId::new(),
        channel_id: ChannelId::new(),
        sender_id: UserId(uuid::Uuid::new_v4()),
        content: MessageContent::Text("test".to_string()),
        timestamp: chrono::Utc::now(),
        sequence_number: 1,
        reply_to: None,
        edited_at: None,
        signature: None,
    };
    event_bus.publish(Event::MessageReceived {
        message,
    }).await.unwrap();
    
    // Receive events
    let event1 = subscriber.recv().await.unwrap();
    let event2 = subscriber.recv().await.unwrap();
    
    match event1 {
        Event::UserRegistered { username, .. } => assert_eq!(username, "alice"),
        _ => panic!("Expected UserRegistered event"),
    }
    
    match event2 {
        Event::MessageReceived { .. } => {},
        _ => panic!("Expected MessageReceived event"),
    }
}

#[test]
fn test_message_expiration_policies() {
    use std::time::Duration;
    
    let mut expiration = MessageExpiration::new();
    
    let sender = UserId(uuid::Uuid::new_v4());
    let recipient = UserId(uuid::Uuid::new_v4());
    
    // Create message with duration-based expiration
    let message = MessageBuilder::new()
        .direct(sender, recipient)
        .content(MessageContent::Text("Expires soon".to_string()))
        .encrypted_payload(vec![1, 2, 3])
        .expires_in(Duration::from_secs(1))
        .build()
        .unwrap();
    
    assert!(!message.is_expired());
    
    // Wait for expiration (would need tokio::time::sleep in real test)
    // For unit test, just verify the logic works
    assert_eq!(expiration.view_count(&message.id), 0);
    
    // Test view-based expiration
    expiration.set_policy(message.id.clone(), ExpirationPolicy::AfterViews(3));
    expiration.record_view(message.id.clone());
    expiration.record_view(message.id.clone());
    assert!(!expiration.should_expire(&message));
    
    expiration.record_view(message.id.clone());
    assert!(expiration.should_expire(&message));
}

#[test]
fn test_storage_lifecycle_management() {
    use dchat_storage::lifecycle::{DataTier, LifecycleManager, TtlConfig};
    use std::time::Duration;
    
    let config = TtlConfig::default();
    let mut lifecycle = LifecycleManager::new(config);
    
    let key = "test-data".to_string();
    
    // Set TTL for the key (this adds to tracking)
    lifecycle.set_ttl(key.clone(), Duration::from_secs(3600));
    
    // Record access
    lifecycle.record_access(key.clone());
    
    // Should start in cold tier
    assert_eq!(lifecycle.get_tier(&key), DataTier::Cold);
    
    // After many accesses, should be hot
    for _ in 0..150 {
        lifecycle.record_access(key.clone());
    }
    lifecycle.update_tier(&key);
    assert_eq!(lifecycle.get_tier(&key), DataTier::Hot);
    
    // Get stats
    let stats = lifecycle.stats();
    assert!(stats.total_tracked > 0);
}

#[tokio::test]
async fn test_simple_message_send() {
    // Create sender and recipient
    let sender_id = UserId::new();
    let recipient_id = UserId::new();
    
    // Create a simple direct message
    let message = MessageBuilder::new()
        .direct(sender_id.clone(), recipient_id.clone())
        .content(MessageContent::Text("Hello, World!".to_string()))
        .encrypted_payload(b"encrypted_hello".to_vec())
        .build()
        .expect("Failed to build message");
    
    // Verify message properties
    assert_eq!(message.sender(), Some(sender_id.clone()));
    assert_eq!(message.status, MessageStatus::Created);
    assert!(!message.is_expired());
    assert!(message.is_deliverable());
    assert!(message.id.to_string().len() > 0);
    
    // Create message ordering system
    let mut order = MessageOrder::new();
    
    // Register message with sequence number
    let in_order = order.register_message(
        sender_id.to_string(),
        SequenceNumber(0),
        message.id,
    );
    assert!(in_order, "First message should be in order");
    
    // Verify no pending messages
    assert_eq!(order.pending_count(&sender_id.to_string()), 0);
    
    // Create delivery tracker
    let mut tracker = DeliveryTracker::new(3);
    
    // Mark message as sent
    tracker.mark_sent(message.id);
    assert_eq!(tracker.get_status(&message.id), Some(DeliveryStatus::Sent));
    
    // Record delivery attempt
    tracker.record_attempt(message.id).expect("Should record attempt");
    assert_eq!(tracker.attempt_count(&message.id), 2); // mark_sent counts as 1, record_attempt adds 1
    
    // Create delivery proof
    let proof = DeliveryProof {
        message_id: message.id,
        relay_peer_id: "relay-001".to_string(),
        recipient_signature: Some(Signature::new(vec![1, 2, 3])),
        timestamp: SystemTime::now(),
        chain_tx_hash: Some("0xabc123".to_string()),
    };
    
    // Store proof
    tracker.store_proof(proof.clone());
    
    // Verify proof stored and status updated
    assert!(tracker.get_proof(&message.id).is_some());
    assert!(proof.is_on_chain());
    assert_eq!(tracker.get_status(&message.id), Some(DeliveryStatus::OnChain));
    assert!(tracker.is_delivered(&message.id));
    
    println!("✅ Message sent successfully:");
    println!("  From: {}", sender_id);
    println!("  To: {}", recipient_id);
    println!("  ID: {}", message.id);
    println!("  Content: Hello, World!");
    println!("  Encrypted: {:?}", &message.encrypted_payload[..8]);
    println!("  On-chain: {:?}", proof.chain_tx_hash);
}

#[tokio::test]
async fn test_storage_and_persistence() {
    use dchat_storage::database::{Database, DatabaseConfig};
    use dchat_storage::deduplication::DeduplicationStore;
    use std::path::PathBuf;
    
    // Create in-memory database config
    let db_config = DatabaseConfig {
        path: PathBuf::from(":memory:"),
        max_connections: 5,
        enable_wal: false,
        connection_timeout_secs: 30,
        idle_timeout_secs: 300,
        max_lifetime_secs: 1800,
    };
    
    // Initialize database
    let db = Database::new(db_config)
        .await
        .expect("Failed to create database");
    
    println!("✅ Database initialized");
    
    // Create a user
    let user_id = UserId::new();
    let username = "alice";
    let public_key = KeyPair::generate().public_key().as_bytes().to_vec();
    
    db.insert_user(
        &user_id.to_string(),
        username,
        &public_key,
    )
    .await
    .expect("Failed to insert user");
    
    println!("✅ User created: {} ({})", username, user_id);
    
    // Retrieve the user
    let retrieved_user = db.get_user(&user_id.to_string())
        .await
        .expect("Failed to get user")
        .expect("User not found");
    
    assert_eq!(retrieved_user.username, username);
    assert_eq!(retrieved_user.public_key, public_key);
    println!("✅ User retrieved: {} ({})", retrieved_user.username, retrieved_user.id);
    
    // Create a deduplication store
    let mut dedup_store = DeduplicationStore::new();
    
    // Store some content hashes
    let (msg1_hash, _) = dedup_store.store(b"Message 1 content", None).unwrap();
    let (msg2_hash, _) = dedup_store.store(b"Message 2 content", None).unwrap();
    let (msg3_hash, _) = dedup_store.store(b"Message 1 content", None).unwrap(); // Duplicate
    
    println!("✅ Content stored with deduplication");
    println!("  Message 1 hash: {:?}", &msg1_hash.as_bytes()[..8]);
    println!("  Message 2 hash: {:?}", &msg2_hash.as_bytes()[..8]);
    println!("  Message 3 hash: {:?}", &msg3_hash.as_bytes()[..8]);
    
    // Check deduplication
    assert_eq!(msg1_hash, msg3_hash, "Identical content should have identical hash");
    assert_ne!(msg1_hash, msg2_hash, "Different content should have different hash");
    assert_eq!(dedup_store.ref_count(&msg1_hash), 2, "Message 1 should have 2 references");
    assert_eq!(dedup_store.ref_count(&msg2_hash), 1, "Message 2 should have 1 reference");
    
    println!("✅ Deduplication verified");
    
    // Test lifecycle manager
    let mut lifecycle = LifecycleManager::new(TtlConfig::default());
    let key1 = "test_key_1".to_string();
    let key2 = "test_key_2".to_string();
    
    // Record some accesses and set expirations
    for _ in 0..10 {
        lifecycle.record_access(key1.clone());
    }
    lifecycle.set_ttl(key1.clone(), std::time::Duration::from_secs(3600));
    
    for _ in 0..5 {
        lifecycle.record_access(key2.clone());
    }
    lifecycle.set_ttl(key2.clone(), std::time::Duration::from_secs(7200));
    
    println!("✅ Lifecycle manager: 2 keys with TTL configured");
    
    // Get statistics
    let stats = lifecycle.stats();
    assert!(stats.total_tracked > 0, "Should have tracked items");
    println!("✅ Storage stats: {} items tracked", stats.total_tracked);
    println!("  Hot count: {}", stats.hot_count);
    println!("  Warm count: {}", stats.warm_count);
    println!("  Cold count: {}", stats.cold_count);
    println!("  Expired count: {}", stats.expired_count);
    
    println!("\n✅ Storage and Persistence Test Complete!");
    println!("  Database: In-memory SQLite");
    println!("  Users: 1 stored and retrieved");
    println!("  Content hashes: 2 unique, 1 duplicate detected");
    println!("  Lifecycle records: 2 tracked with TTL");
}
