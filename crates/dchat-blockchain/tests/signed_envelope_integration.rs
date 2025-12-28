//! Integration tests for the signed envelope transaction system.
//!
//! These tests verify the end-to-end flow of:
//! 1. Key derivation from wallet
//! 2. UserId derivation from public key
//! 3. Envelope creation and signing
//! 4. Envelope verification
//! 5. Submission to chat chain client

use dchat_blockchain::chat_chain::{ChatChainClient, ChatChainConfig};
use dchat_blockchain::wallet::{Wallet, WalletConfig};
use dchat_chain::signed_envelope::{
    address_from_public_key, chain_ids, EnvelopeBuilder, EnvelopeVerifier,
    SignedTransactionEnvelope, UnifiedTransactionType, ENVELOPE_VERSION,
};
use dchat_chain::TransactionType;
use dchat_core::types::UserId;
use dchat_crypto::MnemonicLength;
use uuid::Uuid;

/// Test that address derivation from public key is deterministic
#[test]
fn test_address_from_public_key_deterministic() {
    let pubkey: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];

    let uuid1 = address_from_public_key(&pubkey);
    let uuid2 = address_from_public_key(&pubkey);

    assert_eq!(uuid1, uuid2, "Same pubkey should produce same UUID");
}

/// Test that different public keys produce different addresses
#[test]
fn test_address_from_public_key_unique() {
    let pubkey1: [u8; 32] = [0x01; 32];
    let pubkey2: [u8; 32] = [0x02; 32];

    let uuid1 = address_from_public_key(&pubkey1);
    let uuid2 = address_from_public_key(&pubkey2);

    assert_ne!(
        uuid1, uuid2,
        "Different pubkeys should produce different UUIDs"
    );
}

/// Test wallet creation and public key extraction
#[test]
fn test_wallet_public_key_extraction() {
    let config = WalletConfig::default();
    let (wallet, _phrase) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();

    let public_key = wallet.public_key().expect("Should have public key");
    let pubkey_bytes: &[u8; 32] = public_key.as_bytes();

    // Should be able to derive UserId from public key
    let user_uuid = address_from_public_key(pubkey_bytes);
    let user_id = UserId(user_uuid);

    // UserId should be valid
    assert_ne!(user_id.0, Uuid::nil());
}

/// Test envelope creation and signing
#[test]
fn test_envelope_creation_and_signing() {
    let config = WalletConfig::default();
    let (wallet, _phrase) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();

    let public_key = wallet.public_key().expect("Should have public key");
    let pubkey_bytes: [u8; 32] = *public_key.as_bytes();

    let signing_key = wallet.signing_key().expect("Should have signing key");

    // Create envelope
    let envelope = EnvelopeBuilder::new(chain_ids::TESTNET_CHAT, pubkey_bytes)
        .nonce(1)
        .chat_tx(TransactionType::RegisterUser)
        .payload(b"test payload".to_vec())
        .build_and_sign(signing_key)
        .expect("Should build and sign envelope");

    // Verify envelope structure
    assert_eq!(envelope.version, ENVELOPE_VERSION);
    assert_eq!(envelope.chain_id, chain_ids::TESTNET_CHAT);
    assert_eq!(envelope.nonce, 1);
    assert_eq!(envelope.public_key, pubkey_bytes);
    assert!(matches!(
        envelope.tx_type,
        UnifiedTransactionType::Chat(TransactionType::RegisterUser)
    ));
}

/// Test envelope verification
#[test]
fn test_envelope_verification() {
    let config = WalletConfig::default();
    let (wallet, _phrase) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();

    let public_key = wallet.public_key().expect("Should have public key");
    let pubkey_bytes: [u8; 32] = *public_key.as_bytes();
    let signing_key = wallet.signing_key().expect("Should have signing key");

    // Create and sign envelope
    let envelope = EnvelopeBuilder::new(chain_ids::TESTNET_CHAT, pubkey_bytes)
        .nonce(1)
        .chat_tx(TransactionType::RegisterUser)
        .payload(b"test payload".to_vec())
        .build_and_sign(signing_key)
        .expect("Should build and sign envelope");

    // Create verifier
    let mut verifier = EnvelopeVerifier::new(chain_ids::TESTNET_CHAT);

    // Verify should succeed
    let result = verifier.verify_and_accept(&envelope);
    assert!(result.is_ok(), "Verification should succeed: {:?}", result);
}

/// Test nonce replay protection
#[test]
fn test_nonce_replay_protection() {
    let config = WalletConfig::default();
    let (wallet, _phrase) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();

    let public_key = wallet.public_key().expect("Should have public key");
    let pubkey_bytes: [u8; 32] = *public_key.as_bytes();
    let signing_key = wallet.signing_key().expect("Should have signing key");

    // Create two envelopes with same nonce
    let envelope1 = EnvelopeBuilder::new(chain_ids::TESTNET_CHAT, pubkey_bytes)
        .nonce(1)
        .chat_tx(TransactionType::RegisterUser)
        .payload(b"first".to_vec())
        .build_and_sign(signing_key)
        .expect("Should build envelope 1");

    let envelope2 = EnvelopeBuilder::new(chain_ids::TESTNET_CHAT, pubkey_bytes)
        .nonce(1) // Same nonce!
        .chat_tx(TransactionType::RegisterUser)
        .payload(b"second".to_vec())
        .build_and_sign(signing_key)
        .expect("Should build envelope 2");

    // Create verifier
    let mut verifier = EnvelopeVerifier::new(chain_ids::TESTNET_CHAT);

    // First should succeed
    let result1 = verifier.verify_and_accept(&envelope1);
    assert!(result1.is_ok(), "First envelope should verify");

    // Second should fail (nonce not increasing)
    let result2 = verifier.verify_and_accept(&envelope2);
    assert!(result2.is_err(), "Replay should be rejected");
}

/// Test wrong chain ID rejection
#[test]
fn test_wrong_chain_id_rejected() {
    let config = WalletConfig::default();
    let (wallet, _phrase) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();

    let public_key = wallet.public_key().expect("Should have public key");
    let pubkey_bytes: [u8; 32] = *public_key.as_bytes();
    let signing_key = wallet.signing_key().expect("Should have signing key");

    // Create envelope for mainnet
    let envelope = EnvelopeBuilder::new(chain_ids::MAINNET_CHAT, pubkey_bytes)
        .nonce(1)
        .chat_tx(TransactionType::RegisterUser)
        .payload(b"test".to_vec())
        .build_and_sign(signing_key)
        .expect("Should build envelope");

    // Create verifier for testnet
    let mut verifier = EnvelopeVerifier::new(chain_ids::TESTNET_CHAT);

    // Verification should fail due to chain ID mismatch
    let result = verifier.verify_and_accept(&envelope);
    assert!(result.is_err(), "Wrong chain ID should be rejected");
}

/// Test ChatChainClient integration with signed envelopes
#[tokio::test]
#[cfg(feature = "test-mocks")]
async fn test_chat_chain_client_signed_envelope_submission() {
    let wallet_config = WalletConfig::default();
    let (wallet, _phrase) = Wallet::create(wallet_config, MnemonicLength::Words12, None).unwrap();

    let public_key = wallet.public_key().expect("Should have public key");
    let pubkey_bytes: [u8; 32] = *public_key.as_bytes();
    let signing_key = wallet.signing_key().expect("Should have signing key");

    // Create mock chat chain client
    let chat_config = ChatChainConfig::default();
    let client = ChatChainClient::new_mock(chat_config);

    // Derive UserId from public key (should match what client expects)
    let user_uuid = address_from_public_key(&pubkey_bytes);
    let user_id = UserId(user_uuid);

    // Create signed envelope
    let envelope = EnvelopeBuilder::new(client.chain_id(), pubkey_bytes)
        .nonce(1)
        .chat_tx(TransactionType::RegisterUser)
        .payload(
            serde_json::to_vec(&serde_json::json!({
                "public_key": hex::encode(&pubkey_bytes),
                "timestamp": chrono::Utc::now().timestamp(),
            }))
            .unwrap(),
        )
        .build_and_sign(signing_key)
        .expect("Should build envelope");

    // Submit to client
    let result = client.submit_signed_envelope(envelope).await;
    assert!(result.is_ok(), "Submission should succeed: {:?}", result);

    // Verify user was registered
    let reputation = client.get_reputation(&user_id).unwrap();
    assert_eq!(reputation, 50, "New user should have 50 reputation");
}
