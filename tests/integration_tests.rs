//! Integration tests for the complete dchat system

use dchat::prelude::*;
use dchat_crypto::keys::PrivateKey;

#[tokio::test]
async fn test_full_message_flow() {
    // Create two users
    let alice_keypair = KeyPair::generate();
    let bob_keypair = KeyPair::generate();

    let alice_id = UserId(uuid::Uuid::new_v4());
    let bob_id = UserId(uuid::Uuid::new_v4());

    // Initialize identity manager
    let mut identity_manager = IdentityManager::new();
    let mut alice_identity = Identity::new("alice".to_string(), &alice_keypair);
    alice_identity.user_id = alice_id.clone();
    let mut bob_identity = Identity::new("bob".to_string(), &bob_keypair);
    bob_identity.user_id = bob_id.clone();
    identity_manager.register_identity(alice_identity).unwrap();
    identity_manager.register_identity(bob_identity).unwrap();

    // Create a message
    let sender_id = alice_id.clone();
    let recipient_id = bob_id.clone();
    let message = MessageBuilder::new()
        .direct(sender_id, recipient_id.clone())
        .content(MessageContent::Text("Hello Bob!".to_string()))
        .encrypted_payload(vec![1, 2, 3, 4])
        .build()
        .unwrap();

    // Track message ordering
    let mut message_order = MessageOrder::new();
    let sequence = SequenceNumber(0);
    assert!(message_order.register_message("alice-bob".to_string(), sequence, message.id));

    // Track delivery
    let mut delivery_tracker = DeliveryTracker::new(3);
    delivery_tracker.mark_sent(message.id);
    delivery_tracker.mark_relay_ack(message.id);

    // Store delivery proof with recipient signature
    let proof = DeliveryProof {
        message_id: message.id,
        relay_peer_id: "relay-1".to_string(),
        recipient_id: recipient_id.clone(),
        content_hash: String::new(), // Empty for backwards compat
        recipient_signature: Some(Signature::new(vec![4, 5, 6])),
        timestamp: std::time::SystemTime::now(),
        chain_tx_hash: None,
        reward_amount: 10_000,
    };
    delivery_tracker.store_proof(proof);

    assert!(delivery_tracker.is_delivered(&message.id));
}

#[tokio::test]
async fn test_multi_device_sync() {
    let user_id = UserId(uuid::Uuid::new_v4());

    // Create multiple devices
    let device1_keypair = KeyPair::generate();
    let device2_keypair = KeyPair::generate();

    let device1 = Device {
        device_id: uuid::Uuid::new_v4().to_string(),
        device_name: "Phone".to_string(),
        device_type: DeviceType::Mobile,
        public_key: device1_keypair.public_key().to_core_public_key(),
        added_at: chrono::Utc::now(),
        last_seen: chrono::Utc::now(),
        trusted: true,
    };

    let device2 = Device {
        device_id: uuid::Uuid::new_v4().to_string(),
        device_name: "Laptop".to_string(),
        device_type: DeviceType::Desktop,
        public_key: device2_keypair.public_key().to_core_public_key(),
        added_at: chrono::Utc::now(),
        last_seen: chrono::Utc::now(),
        trusted: true,
    };

    // Register devices
    let mut device_manager = DeviceManager::new();
    device_manager
        .add_device(user_id.clone(), device1.clone())
        .unwrap();
    device_manager
        .add_device(user_id.clone(), device2.clone())
        .unwrap();

    assert_eq!(device_manager.get_devices(&user_id).len(), 2);

    // Test sync
    let mut sync_manager = SyncManager::new(100);
    let sync_msg = SyncMessage {
        sync_id: uuid::Uuid::new_v4().to_string(),
        user_id: user_id.clone(),
        device_id: device1.device_id.clone(),
        message_type: dchat_identity::sync::SyncMessageType::IdentityUpdate,
        encrypted_payload: vec![1, 2, 3],
        timestamp: chrono::Utc::now(),
        vector_clock: dchat_identity::sync::VectorClock::default(),
    };

    sync_manager.add_sync_message(sync_msg.clone()).unwrap();
    assert_eq!(sync_manager.get_pending_syncs(&user_id).len(), 1);
}

#[tokio::test]
async fn test_guardian_recovery_flow() {
    let user_id = UserId(uuid::Uuid::new_v4());

    // Create guardians
    let guardian1_keypair = KeyPair::generate();
    let guardian2_keypair = KeyPair::generate();
    let guardian3_keypair = KeyPair::generate();

    let guardian1 = Guardian {
        guardian_id: UserId(uuid::Uuid::new_v4()),
        public_key: guardian1_keypair.public_key().to_core_public_key(),
        added_at: chrono::Utc::now(),
        trusted: true,
    };

    let guardian2 = Guardian {
        guardian_id: UserId(uuid::Uuid::new_v4()),
        public_key: guardian2_keypair.public_key().to_core_public_key(),
        added_at: chrono::Utc::now(),
        trusted: true,
    };

    let guardian3 = Guardian {
        guardian_id: UserId(uuid::Uuid::new_v4()),
        public_key: guardian3_keypair.public_key().to_core_public_key(),
        added_at: chrono::Utc::now(),
        trusted: true,
    };

    // Setup guardian manager
    let mut guardian_manager = GuardianManager::new(24);
    guardian_manager
        .add_guardian(user_id.clone(), guardian1.clone())
        .unwrap();
    guardian_manager
        .add_guardian(user_id.clone(), guardian2.clone())
        .unwrap();
    guardian_manager
        .add_guardian(user_id.clone(), guardian3.clone())
        .unwrap();

    // Initiate recovery
    let new_keypair = KeyPair::generate();
    let recovery_id = format!("recovery-{}", uuid::Uuid::new_v4());
    guardian_manager
        .initiate_recovery(
            recovery_id.clone(),
            user_id.clone(),
            new_keypair.public_key().to_core_public_key(),
        )
        .unwrap();

    // Approve recovery (need 2 out of 3)
    guardian_manager
        .approve_recovery(&recovery_id, guardian1.guardian_id.clone(), vec![1, 2, 3])
        .unwrap();
    guardian_manager
        .approve_recovery(&recovery_id, guardian2.guardian_id.clone(), vec![4, 5, 6])
        .unwrap();

    // Check if recovery can be executed
    let request = guardian_manager.get_recovery_request(&recovery_id).unwrap();
    assert_eq!(
        request.status,
        dchat_identity::guardian::RecoveryStatus::TimelockActive
    );
}

#[tokio::test]
async fn test_burner_identity_lifecycle() {
    let parent_user_id = UserId(uuid::Uuid::new_v4());
    let burner_keypair = KeyPair::generate();

    // Create burner identity with message limit
    let mut burner = BurnerIdentity::with_message_limit(&burner_keypair, 10, Some(parent_user_id));

    // Send messages until limit
    for _ in 0..9 {
        burner.record_message_sent();
        assert!(!burner.is_expired());
    }

    // One more should reach limit
    burner.record_message_sent();
    assert!(burner.is_expired());
}

#[test]
fn test_message_deduplication() {
    let mut dedup_store = DeduplicationStore::new();

    let content = b"Hello, World!".to_vec();

    // Store same content multiple times
    let (hash1, _) = dedup_store.store(&content, None).unwrap();
    let (hash2, _) = dedup_store.store(&content, None).unwrap();
    let (hash3, _) = dedup_store.store(&content, None).unwrap();

    // All should have same hash
    assert_eq!(hash1, hash2);
    assert_eq!(hash2, hash3);

    // Should only store once
    assert_eq!(dedup_store.item_count(), 1);
    assert_eq!(dedup_store.ref_count(&hash1), 3);

    // Calculate savings
    let savings = dedup_store.savings();
    assert!(savings.saved_bytes > 0);
}

#[test]
fn test_key_rotation_policy() {
    // Duration not needed in this test

    let policy = RotationPolicy {
        max_age_hours: 1, // 1 hour
        max_messages_per_key: 100,
        rotate_on_events: vec![],
    };

    let master_key = PrivateKey::generate();
    let mut rotation_manager = KeyRotationManager::new(master_key, policy);

    let purpose = "test-key";

    // Create initial key
    rotation_manager.get_key(purpose).unwrap();

    // Should not rotate immediately after creating
    assert!(!rotation_manager.should_rotate(purpose).unwrap());

    // Record many messages
    for _ in 0..101 {
        rotation_manager.record_message_sent(purpose).unwrap();
    }

    // Should now need rotation
    assert!(rotation_manager.should_rotate(purpose).unwrap());
}

#[test]
fn test_offline_queue_limits() {
    let mut queue = OfflineQueue::new(5, 1000); // Max 5 messages, 1KB total

    let sender = UserId(uuid::Uuid::new_v4());
    let recipient = UserId(uuid::Uuid::new_v4());

    // Fill queue
    for i in 0..5 {
        let message = MessageBuilder::new()
            .direct(sender.clone(), recipient.clone())
            .content(MessageContent::Text(format!("Message {}", i)))
            .encrypted_payload(vec![0u8; 100])
            .build()
            .unwrap();

        assert!(queue.enqueue(recipient.clone(), message).is_ok());
    }

    // Next message should fail (queue full)
    let overflow_message = MessageBuilder::new()
        .direct(sender.clone(), recipient.clone())
        .content(MessageContent::Text("Overflow".to_string()))
        .encrypted_payload(vec![0u8; 100])
        .build()
        .unwrap();

    assert!(queue.enqueue(recipient.clone(), overflow_message).is_err());
}
