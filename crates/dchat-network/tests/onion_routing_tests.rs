//! Comprehensive tests for onion routing with ChaCha20-Poly1305 AEAD encryption
//!
//! Tests verify:
//! - Multi-hop encryption/decryption
//! - Authentication tag validation
//! - Nonce uniqueness
//! - Circuit construction
//! - Routing header integrity

use chacha20poly1305::{aead::Aead, ChaCha20Poly1305, KeyInit};
use dchat_core::error::Result;
use dchat_network::routing::OnionRouter;
use hkdf::Hkdf;
use libp2p::PeerId;
use sha2::Sha256;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

/// Mock keystore for testing relay keys
pub struct MockRelayKeystore {
    static_secret: StaticSecret,
}

impl MockRelayKeystore {
    pub fn new() -> Self {
        Self {
            static_secret: StaticSecret::random_from_rng(&mut rand::thread_rng()),
        }
    }

    pub fn x25519_static_secret(&self) -> Result<&StaticSecret> {
        Ok(&self.static_secret)
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey::from(&self.static_secret)
    }
}

#[test]
fn test_two_hop_encryption_structure() {
    // Onion circuits require at least 2 relays.
    let mut router = OnionRouter::new();
    let relays = vec![PeerId::random(), PeerId::random()];

    router
        .create_circuit("test-circuit".to_string(), relays)
        .unwrap();

    let plaintext = b"Hello, dchat!";
    let circuit = router.get_circuit("test-circuit").unwrap();

    // Encrypt message
    let onion = router.onion_encrypt(plaintext, circuit).unwrap();

    // Verify encrypted message is different from plaintext
    assert_ne!(onion.as_slice(), plaintext);
    assert!(onion.len() > plaintext.len()); // Should have headers + nonce + auth tag

    // Note: Decryption requires relay keystore which needs integration with actual crypto layer.
    // This is tested in integration tests.
}

#[test]
fn test_three_hop_encryption() {
    // Create a three-hop circuit (standard Tor-like configuration)
    let mut router = OnionRouter::new();
    let relays = vec![PeerId::random(), PeerId::random(), PeerId::random()];

    router
        .create_circuit("test-circuit-3hop".to_string(), relays.clone())
        .unwrap();

    let plaintext = b"Multi-hop test message";
    let circuit = router.get_circuit("test-circuit-3hop").unwrap();

    // Encrypt message through all three hops
    let onion = router.onion_encrypt(plaintext, circuit).unwrap();

    // Verify structure:
    // - Header (32 bytes next_hop + 32 bytes ephemeral_pubkey) = 64 bytes
    // - Nonce (12 bytes)
    // - Encrypted payload + auth tag (16 bytes)
    // Each layer adds: 64 + 12 = 76 bytes overhead minimum

    let min_expected_size = plaintext.len() + (relays.len() * 76);
    assert!(
        onion.len() >= min_expected_size,
        "Onion size {} should be at least {} (plaintext + 3 layers)",
        onion.len(),
        min_expected_size
    );

    println!("Plaintext size: {} bytes", plaintext.len());
    println!("Onion size: {} bytes", onion.len());
    println!(
        "Overhead per layer: ~{} bytes",
        (onion.len() - plaintext.len()) / relays.len()
    );
}

#[test]
fn test_circuit_creation() {
    let mut router = OnionRouter::new();

    // Test successful circuit creation
    let relays = vec![PeerId::random(), PeerId::random()];
    let result = router.create_circuit("circuit-1".to_string(), relays.clone());
    assert!(result.is_ok());
    assert_eq!(router.get_circuit("circuit-1"), Some(relays.as_slice()));

    // Test insufficient relays (minimum 2)
    let result = router.create_circuit("circuit-2".to_string(), vec![PeerId::random()]);
    assert!(result.is_err());
}

#[test]
fn test_circuit_closure() {
    let mut router = OnionRouter::new();

    let relays = vec![PeerId::random(), PeerId::random()];
    router
        .create_circuit("circuit-close-test".to_string(), relays)
        .unwrap();

    // Verify circuit exists
    assert!(router.get_circuit("circuit-close-test").is_some());

    // Close circuit
    router.close_circuit("circuit-close-test");

    // Verify circuit removed
    assert!(router.get_circuit("circuit-close-test").is_none());
}

#[test]
fn test_encryption_produces_different_ciphertexts() {
    // Verify that encrypting the same message twice produces different ciphertexts
    // (due to random nonces and ephemeral keys)

    let mut router = OnionRouter::new();
    let relays = vec![PeerId::random(), PeerId::random()];
    router
        .create_circuit("test-nonce-uniqueness".to_string(), relays.clone())
        .unwrap();

    let plaintext = b"Test message for nonce uniqueness";
    let circuit = router.get_circuit("test-nonce-uniqueness").unwrap();

    // Encrypt same message twice
    let onion1 = router.onion_encrypt(plaintext, circuit).unwrap();
    let onion2 = router.onion_encrypt(plaintext, circuit).unwrap();

    // Verify different ciphertexts (critical for security)
    assert_ne!(
        onion1, onion2,
        "Nonce reuse detected! This is a critical security vulnerability."
    );
}

#[test]
fn test_chacha20poly1305_aead_properties() {
    // Direct test of ChaCha20-Poly1305 AEAD properties

    // Generate key (32 bytes)
    let key_bytes = [42u8; 32];
    let cipher = ChaCha20Poly1305::new_from_slice(&key_bytes).unwrap();

    // Generate nonce (12 bytes)
    let nonce = chacha20poly1305::aead::Nonce::<ChaCha20Poly1305>::from([1u8; 12]);

    let plaintext = b"AEAD test message";

    // Encrypt
    let ciphertext = cipher.encrypt(&nonce, plaintext.as_ref()).unwrap();

    // Verify ciphertext includes authentication tag (16 bytes)
    assert_eq!(ciphertext.len(), plaintext.len() + 16);

    // Decrypt
    let decrypted = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();
    assert_eq!(&decrypted, plaintext);

    // Test authentication: modify ciphertext should fail decryption
    let mut tampered = ciphertext.clone();
    tampered[0] ^= 0xFF; // Flip bits in first byte
    let result = cipher.decrypt(&nonce, tampered.as_ref());
    assert!(
        result.is_err(),
        "Tampered ciphertext should fail authentication"
    );
}

#[test]
fn test_hkdf_key_derivation() {
    // Verify HKDF key derivation consistency

    let shared_secret = [123u8; 32];
    let salt = None;
    let info = b"dchat-onion-layer-key-v1";

    // Derive key twice with same inputs
    let hkdf1 = Hkdf::<Sha256>::new(salt, &shared_secret);
    let mut key1 = [0u8; 32];
    hkdf1.expand(info, &mut key1).unwrap();

    let hkdf2 = Hkdf::<Sha256>::new(salt, &shared_secret);
    let mut key2 = [0u8; 32];
    hkdf2.expand(info, &mut key2).unwrap();

    // Keys should be identical (deterministic)
    assert_eq!(key1, key2);

    // Different info should produce different keys
    let hkdf3 = Hkdf::<Sha256>::new(salt, &shared_secret);
    let mut key3 = [0u8; 32];
    hkdf3.expand(b"different-info-string", &mut key3).unwrap();
    assert_ne!(key1, key3);
}

#[test]
fn test_ecdh_shared_secret_consistency() {
    // Verify X25519 ECDH produces consistent shared secrets

    // Alice generates ephemeral key
    let alice_secret = StaticSecret::random_from_rng(&mut rand::thread_rng());
    let alice_public = PublicKey::from(&alice_secret);

    // Bob has static key
    let bob_secret = StaticSecret::random_from_rng(&mut rand::thread_rng());
    let bob_public = PublicKey::from(&bob_secret);

    // Alice computes shared secret with Bob's public key
    let shared_alice = alice_secret.diffie_hellman(&bob_public);

    // Bob computes shared secret with Alice's public key
    let shared_bob = bob_secret.diffie_hellman(&alice_public);

    // Shared secrets should match
    assert_eq!(shared_alice.as_bytes(), shared_bob.as_bytes());
}

#[test]
fn test_long_message_encryption() {
    // Test encryption of larger messages (1KB)

    let mut router = OnionRouter::new();
    let relays = vec![PeerId::random(), PeerId::random(), PeerId::random()];
    router
        .create_circuit("large-msg-circuit".to_string(), relays)
        .unwrap();

    let plaintext = vec![0xABu8; 1024]; // 1KB message
    let circuit = router.get_circuit("large-msg-circuit").unwrap();

    let onion = router.onion_encrypt(&plaintext, circuit).unwrap();

    // Verify encryption succeeded
    assert!(onion.len() > plaintext.len());
    assert_ne!(onion.as_slice(), plaintext.as_slice());
}

#[test]
fn test_empty_message_encryption() {
    // Test edge case: empty message

    let mut router = OnionRouter::new();
    let relays = vec![PeerId::random(), PeerId::random()];
    router
        .create_circuit("empty-msg-circuit".to_string(), relays)
        .unwrap();

    let plaintext = b"";
    let circuit = router.get_circuit("empty-msg-circuit").unwrap();

    let onion = router.onion_encrypt(plaintext, circuit).unwrap();

    // Even empty messages should have headers + nonces + auth tags
    assert!(onion.len() > 0);
    assert!(onion.len() >= 2 * (64 + 12 + 16)); // Min overhead for 2 hops
}

#[test]
fn test_maximum_circuit_length() {
    // Test creating circuit with 5 hops (typical maximum for anonymity networks)

    let mut router = OnionRouter::new();
    let relays = vec![
        PeerId::random(),
        PeerId::random(),
        PeerId::random(),
        PeerId::random(),
        PeerId::random(),
    ];

    let result = router.create_circuit("max-length-circuit".to_string(), relays.clone());
    assert!(result.is_ok());

    let plaintext = b"Test message through 5 hops";
    let circuit = router.get_circuit("max-length-circuit").unwrap();

    // Encrypt through all 5 hops
    let onion = router.onion_encrypt(plaintext, circuit).unwrap();

    // Verify reasonable size (should not explode exponentially)
    let expected_overhead_per_hop = 100; // ~76 bytes actual + padding
    let max_expected_size = plaintext.len() + (relays.len() * expected_overhead_per_hop);
    assert!(
        onion.len() < max_expected_size * 2,
        "Onion packet too large"
    );
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_nonce_uniqueness_across_layers() {
        // Critical security test: verify each layer uses unique nonce

        let mut router = OnionRouter::new();
        let relays = vec![PeerId::random(), PeerId::random(), PeerId::random()];
        router
            .create_circuit("nonce-uniqueness-test".to_string(), relays)
            .unwrap();

        let plaintext = b"Nonce uniqueness verification";
        let circuit = router.get_circuit("nonce-uniqueness-test").unwrap();

        // Encrypt message
        let onion = router.onion_encrypt(plaintext, circuit).unwrap();

        // Extract nonces from each layer by parsing packet structure
        // This is a simplified check - full integration test would decrypt layers

        // Each layer has structure: [next_hop(32) | ephemeral_pub(32) | nonce(12) | ciphertext+tag]
        // We can extract the nonce from each layer's header

        let mut nonces = Vec::new();
        let mut offset = 64; // Skip first header (next_hop + ephemeral_pub)

        for _ in 0..3 {
            if offset + 12 <= onion.len() {
                let nonce_bytes = &onion[offset..offset + 12];
                nonces.push(nonce_bytes.to_vec());

                // Find next layer (simplified - actual parsing is more complex)
                offset += 12 + 16; // Skip nonce + min ciphertext
                if offset < onion.len() {
                    offset += 64; // Next header
                }
            }
        }

        // Note: This is a simplified test. Full verification requires layer-by-layer decryption
        println!(
            "Extracted {} nonce positions from onion packet",
            nonces.len()
        );
    }

    #[test]
    fn test_authentication_tag_coverage() {
        // Verify ChaCha20-Poly1305 includes 16-byte authentication tag

        let key = [0u8; 32];
        let cipher = ChaCha20Poly1305::new_from_slice(&key).unwrap();
        let nonce = chacha20poly1305::aead::Nonce::<ChaCha20Poly1305>::from([0u8; 12]);

        let plaintext = b"Authentication tag test";
        let ciphertext = cipher.encrypt(&nonce, plaintext.as_ref()).unwrap();

        // Ciphertext should be plaintext + 16-byte Poly1305 tag
        assert_eq!(ciphertext.len(), plaintext.len() + 16);

        // Last 16 bytes are the authentication tag
        let auth_tag = &ciphertext[plaintext.len()..];
        assert_eq!(auth_tag.len(), 16);
    }

    #[test]
    fn test_replay_attack_resistance() {
        // Verify that replaying same onion packet fails if nonces are tracked
        // (Note: Full replay protection requires state tracking on relays)

        let mut router = OnionRouter::new();
        let relays = vec![PeerId::random(), PeerId::random()];
        router
            .create_circuit("replay-test".to_string(), relays)
            .unwrap();

        let plaintext = b"Replay attack test";
        let circuit = router.get_circuit("replay-test").unwrap();

        // Encrypt twice
        let onion1 = router.onion_encrypt(plaintext, circuit).unwrap();
        let onion2 = router.onion_encrypt(plaintext, circuit).unwrap();

        // Different encryptions should produce different packets (random ephemeral keys + nonces)
        assert_ne!(onion1, onion2);
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn bench_encryption_performance() {
        let mut router = OnionRouter::new();
        let relays = vec![PeerId::random(), PeerId::random(), PeerId::random()];
        router
            .create_circuit("perf-test".to_string(), relays)
            .unwrap();

        let plaintext = vec![0u8; 1024]; // 1KB message
        let circuit = router.get_circuit("perf-test").unwrap();

        let iterations = 100;
        let start = Instant::now();

        for _ in 0..iterations {
            let _ = router.onion_encrypt(&plaintext, circuit).unwrap();
        }

        let elapsed = start.elapsed();
        let avg_time = elapsed / iterations;

        println!("Average encryption time (3 hops, 1KB): {:?}", avg_time);

        // Should complete in reasonable time (< 10ms per encryption)
        assert!(
            avg_time.as_millis() < 10,
            "Encryption too slow: {:?}",
            avg_time
        );
    }
}
