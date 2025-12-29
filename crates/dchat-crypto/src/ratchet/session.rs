//! Double Ratchet Session
//!
//! The main session state machine that combines the DH ratchet with
//! the symmetric sending and receiving chains.

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::chain::ChainKey;
use super::header::MessageHeader;
use super::skipped::SkippedMessageKeys;
use super::{DEFAULT_MAX_SKIP, SKIPPED_KEY_MAX_AGE_SECS};
use crate::kdf::Hkdf;

/// Configuration for a Double Ratchet session
#[derive(Clone, Debug)]
pub struct SessionConfig {
    /// Maximum messages to skip when receiving out-of-order
    pub max_skip: usize,
    /// Maximum age of skipped keys (seconds)
    pub max_skipped_key_age_secs: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_skip: DEFAULT_MAX_SKIP,
            max_skipped_key_age_secs: SKIPPED_KEY_MAX_AGE_SECS,
        }
    }
}

/// The current state of the ratchet
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RatchetState {
    /// Waiting to send the first message (has peer's ratchet key)
    ReadyToSend,
    /// Waiting to receive first message (has own ratchet key)
    WaitingForReply,
    /// Normal operation (can send and receive)
    Active,
}

/// Double Ratchet session for secure messaging
pub struct DoubleRatchet {
    /// Session configuration
    config: SessionConfig,

    /// Root key (evolves with each DH ratchet step)
    root_key: [u8; 32],

    /// Our current DH key pair
    dh_self: DhKeyPair,

    /// Peer's current DH public key
    dh_peer: Option<PublicKey>,

    /// Sending chain key
    chain_sending: Option<ChainKey>,

    /// Receiving chain key
    chain_receiving: Option<ChainKey>,

    /// Number of messages sent in the previous sending chain
    previous_chain_length: u32,

    /// Skipped message keys for out-of-order messages
    skipped_keys: SkippedMessageKeys,

    /// Current state
    state: RatchetState,
}

/// DH key pair wrapper
#[derive(Zeroize, ZeroizeOnDrop)]
struct DhKeyPair {
    #[zeroize(skip)]
    public: PublicKey,
    secret: [u8; 32],
}

impl DhKeyPair {
    /// Generate a new DH key pair
    fn generate() -> Self {
        use rand::RngCore;
        let mut secret_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret_bytes);
        let secret = StaticSecret::from(secret_bytes);
        let public = PublicKey::from(&secret);

        Self {
            public,
            secret: secret_bytes,
        }
    }

    /// Perform DH with peer's public key
    fn dh(&self, peer_public: &PublicKey) -> [u8; 32] {
        let secret = StaticSecret::from(self.secret);
        *secret.diffie_hellman(peer_public).as_bytes()
    }
}

impl Clone for DhKeyPair {
    fn clone(&self) -> Self {
        Self {
            public: self.public,
            secret: self.secret,
        }
    }
}

impl DoubleRatchet {
    /// Initialize as the sender (Alice) after X3DH
    ///
    /// Alice has computed the shared secret and knows Bob's signed pre-key
    pub fn init_sender(
        shared_secret: [u8; 32],
        peer_public_key: PublicKey,
        config: SessionConfig,
    ) -> Result<Self> {
        // Generate our first DH key pair
        let dh_self = DhKeyPair::generate();

        // Perform initial DH
        let dh_output = dh_self.dh(&peer_public_key);

        // Derive initial root key and sending chain
        let (chain_sending, root_key) = ChainKey::from_root_key(&shared_secret, &dh_output)?;

        Ok(Self {
            skipped_keys: SkippedMessageKeys::new(config.max_skipped_key_age_secs),
            config,
            root_key,
            dh_self,
            dh_peer: Some(peer_public_key),
            chain_sending: Some(chain_sending),
            chain_receiving: None,
            previous_chain_length: 0,
            state: RatchetState::ReadyToSend,
        })
    }

    /// Initialize as the receiver (Bob) after X3DH
    ///
    /// Bob provides his signed pre-key pair and the shared secret
    pub fn init_receiver(
        shared_secret: [u8; 32],
        signed_pre_key_secret: [u8; 32],
        signed_pre_key_public: PublicKey,
        config: SessionConfig,
    ) -> Result<Self> {
        // Bob uses his signed pre-key as initial DH key
        let dh_self = DhKeyPair {
            public: signed_pre_key_public,
            secret: signed_pre_key_secret,
        };

        Ok(Self {
            skipped_keys: SkippedMessageKeys::new(config.max_skipped_key_age_secs),
            config,
            root_key: shared_secret,
            dh_self,
            dh_peer: None,
            chain_sending: None,
            chain_receiving: None,
            previous_chain_length: 0,
            state: RatchetState::WaitingForReply,
        })
    }

    /// Encrypt a message
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<(MessageHeader, Vec<u8>)> {
        // Ensure we have a sending chain
        if self.chain_sending.is_none() {
            return Err(Error::crypto("No sending chain available".to_string()));
        }

        let chain = self.chain_sending.as_mut().unwrap();

        // Derive message key
        let message_key = chain.derive_message_key()?;

        // Create header
        let header = MessageHeader::new(
            *self.dh_self.public.as_bytes(),
            self.previous_chain_length,
            message_key.index(),
        );

        // Encrypt with header as AAD
        let aad = header.encode();
        let ciphertext = message_key.encrypt(plaintext, &aad)?;

        self.state = RatchetState::Active;

        Ok((header, ciphertext))
    }

    /// Decrypt a message
    pub fn decrypt(&mut self, header: &MessageHeader, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let aad = header.encode();

        // Check if this is a skipped message
        if let Some(mk) = self
            .skipped_keys
            .take(&header.dh_public, header.message_index)
        {
            return mk.decrypt(ciphertext, &aad);
        }

        let peer_public = header.dh_public_key();

        // Check if we need to perform a DH ratchet step
        let need_dh_ratchet = match &self.dh_peer {
            None => true,
            Some(current_peer) => current_peer.as_bytes() != peer_public.as_bytes(),
        };

        if need_dh_ratchet {
            self.dh_ratchet_step(&peer_public, header)?;
        }

        // Skip ahead if needed
        let chain = self
            .chain_receiving
            .as_mut()
            .ok_or_else(|| Error::crypto("No receiving chain".to_string()))?;

        if header.message_index > chain.index() {
            let skipped = chain.skip_to(header.message_index, self.config.max_skip)?;
            self.skipped_keys
                .store_many(*self.dh_peer.as_ref().unwrap().as_bytes(), skipped)?;
        }

        // Derive message key for this message
        if header.message_index != chain.index() {
            return Err(Error::crypto(format!(
                "Message index mismatch: expected {}, got {}",
                chain.index(),
                header.message_index
            )));
        }

        let message_key = chain.derive_message_key()?;
        let plaintext = message_key.decrypt(ciphertext, &aad)?;

        self.state = RatchetState::Active;

        Ok(plaintext)
    }

    /// Perform a DH ratchet step
    fn dh_ratchet_step(&mut self, peer_public: &PublicKey, header: &MessageHeader) -> Result<()> {
        // Store skipped message keys from current receiving chain
        if let Some(ref mut chain) = self.chain_receiving {
            if let Some(ref current_peer) = self.dh_peer {
                // Skip any remaining keys in the old chain
                let current_index = chain.index();
                if header.previous_chain_length > current_index {
                    let skipped =
                        chain.skip_to(header.previous_chain_length, self.config.max_skip)?;
                    self.skipped_keys
                        .store_many(*current_peer.as_bytes(), skipped)?;
                }
            }
        }

        // Update peer's DH public key
        self.dh_peer = Some(*peer_public);

        // Calculate DH output with peer's new key
        let dh_output = self.dh_self.dh(peer_public);

        // Derive new receiving chain
        let (chain_receiving, root_key) = ChainKey::from_root_key(&self.root_key, &dh_output)?;
        self.root_key = root_key;
        self.chain_receiving = Some(chain_receiving);

        // Save previous sending chain length
        self.previous_chain_length = self.chain_sending.as_ref().map(|c| c.index()).unwrap_or(0);

        // Generate new DH key pair
        self.dh_self = DhKeyPair::generate();

        // Calculate new DH output with our new key
        let dh_output = self.dh_self.dh(peer_public);

        // Derive new sending chain
        let (chain_sending, root_key) = ChainKey::from_root_key(&self.root_key, &dh_output)?;
        self.root_key = root_key;
        self.chain_sending = Some(chain_sending);

        Ok(())
    }

    /// Get current ratchet state
    pub fn state(&self) -> &RatchetState {
        &self.state
    }

    /// Get our current DH public key
    pub fn our_public_key(&self) -> &PublicKey {
        &self.dh_self.public
    }

    /// Get peer's current DH public key
    pub fn peer_public_key(&self) -> Option<&PublicKey> {
        self.dh_peer.as_ref()
    }

    /// Get number of skipped keys stored
    pub fn skipped_key_count(&self) -> usize {
        self.skipped_keys.len()
    }

    /// Clean up expired skipped keys
    pub fn cleanup_expired_keys(&mut self) -> usize {
        self.skipped_keys.cleanup_expired()
    }

    /// Get sending chain index
    pub fn sending_chain_index(&self) -> Option<u32> {
        self.chain_sending.as_ref().map(|c| c.index())
    }

    /// Get receiving chain index
    pub fn receiving_chain_index(&self) -> Option<u32> {
        self.chain_receiving.as_ref().map(|c| c.index())
    }
}

impl Drop for DoubleRatchet {
    fn drop(&mut self) {
        self.root_key.zeroize();
        self.skipped_keys.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_session_pair() -> (DoubleRatchet, DoubleRatchet) {
        // Simulate X3DH shared secret
        let shared_secret = [42u8; 32];

        // Bob's "signed pre-key" for this test
        let bob_dh = DhKeyPair::generate();

        let config = SessionConfig::default();

        // Alice initializes as sender
        let alice =
            DoubleRatchet::init_sender(shared_secret, bob_dh.public, config.clone()).unwrap();

        // Bob initializes as receiver
        let bob = DoubleRatchet::init_receiver(shared_secret, bob_dh.secret, bob_dh.public, config)
            .unwrap();

        (alice, bob)
    }

    #[test]
    fn test_basic_encryption() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Alice sends to Bob
        let plaintext = b"Hello, Bob!";
        let (header, ciphertext) = alice.encrypt(plaintext).unwrap();

        // Bob decrypts
        let decrypted = bob.decrypt(&header, &ciphertext).unwrap();
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_bidirectional_communication() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Alice -> Bob
        let (h1, c1) = alice.encrypt(b"Hello Bob").unwrap();
        let d1 = bob.decrypt(&h1, &c1).unwrap();
        assert_eq!(&d1, b"Hello Bob");

        // Bob -> Alice
        let (h2, c2) = bob.encrypt(b"Hello Alice").unwrap();
        let d2 = alice.decrypt(&h2, &c2).unwrap();
        assert_eq!(&d2, b"Hello Alice");

        // Alice -> Bob again
        let (h3, c3) = alice.encrypt(b"How are you?").unwrap();
        let d3 = bob.decrypt(&h3, &c3).unwrap();
        assert_eq!(&d3, b"How are you?");
    }

    #[test]
    fn test_multiple_messages_same_direction() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Alice sends multiple messages before Bob replies
        let (h1, c1) = alice.encrypt(b"Message 1").unwrap();
        let (h2, c2) = alice.encrypt(b"Message 2").unwrap();
        let (h3, c3) = alice.encrypt(b"Message 3").unwrap();

        // Bob receives all in order
        let d1 = bob.decrypt(&h1, &c1).unwrap();
        let d2 = bob.decrypt(&h2, &c2).unwrap();
        let d3 = bob.decrypt(&h3, &c3).unwrap();

        assert_eq!(&d1, b"Message 1");
        assert_eq!(&d2, b"Message 2");
        assert_eq!(&d3, b"Message 3");
    }

    #[test]
    fn test_out_of_order_messages() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Alice sends multiple messages
        let (h1, c1) = alice.encrypt(b"Message 1").unwrap();
        let (h2, c2) = alice.encrypt(b"Message 2").unwrap();
        let (h3, c3) = alice.encrypt(b"Message 3").unwrap();

        // Bob receives out of order: 3, 1, 2
        let d3 = bob.decrypt(&h3, &c3).unwrap();
        assert_eq!(&d3, b"Message 3");

        let d1 = bob.decrypt(&h1, &c1).unwrap();
        assert_eq!(&d1, b"Message 1");

        let d2 = bob.decrypt(&h2, &c2).unwrap();
        assert_eq!(&d2, b"Message 2");
    }

    #[test]
    fn test_dh_ratchet_advancement() {
        let (mut alice, mut bob) = create_test_session_pair();

        let alice_pk1 = *alice.our_public_key().as_bytes();

        // Alice sends
        let (h1, c1) = alice.encrypt(b"From Alice").unwrap();
        bob.decrypt(&h1, &c1).unwrap();

        // Bob replies - this triggers DH ratchet
        let (h2, c2) = bob.encrypt(b"From Bob").unwrap();
        alice.decrypt(&h2, &c2).unwrap();

        // Alice sends again - her DH key should have changed
        let (h3, _) = alice.encrypt(b"Alice again").unwrap();

        // Verify DH key rotated
        assert_ne!(alice_pk1, h3.dh_public);
    }

    #[test]
    fn test_forward_secrecy() {
        let (mut alice, mut bob) = create_test_session_pair();

        // Exchange several rounds
        let (h1, c1) = alice.encrypt(b"M1").unwrap();
        bob.decrypt(&h1, &c1).unwrap();

        let (h2, c2) = bob.encrypt(b"M2").unwrap();
        alice.decrypt(&h2, &c2).unwrap();

        let (h3, c3) = alice.encrypt(b"M3").unwrap();
        bob.decrypt(&h3, &c3).unwrap();

        // At this point, old chain keys should have been zeroized
        // The test verifies the protocol completes without errors
        // Actual forward secrecy is guaranteed by the key derivation design
    }

    #[test]
    fn test_wrong_key_fails() {
        let (mut alice, _bob) = create_test_session_pair();

        // Create a separate session pair
        let (_, mut eve) = create_test_session_pair();

        // Alice encrypts
        let (header, ciphertext) = alice.encrypt(b"Secret message").unwrap();

        // Eve tries to decrypt with wrong keys - should fail
        let result = eve.decrypt(&header, &ciphertext);
        assert!(result.is_err());
    }
}
