//! Noise Protocol implementation for secure communication

use crate::keys::{PrivateKey, PublicKey};
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use snow::{params::NoiseParams, Builder, HandshakeState, TransportState};

/// Noise protocol patterns we support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NoisePattern {
    XX, // Most common: mutual authentication
    XK, // Responder key known
    IK, // Initiator key known, responder key known
    NK, // No initiator authentication
}

impl NoisePattern {
    fn to_params_string(&self) -> &'static str {
        match self {
            NoisePattern::XX => "Noise_XX_25519_ChaChaPoly_BLAKE2s",
            NoisePattern::XK => "Noise_XK_25519_ChaChaPoly_BLAKE2s",
            NoisePattern::IK => "Noise_IK_25519_ChaChaPoly_BLAKE2s",
            NoisePattern::NK => "Noise_NK_25519_ChaChaPoly_BLAKE2s",
        }
    }
}

/// Represents an ongoing Noise handshake
#[derive(Debug)]
pub struct NoiseHandshake {
    state: HandshakeState,
    is_initiator: bool,
}

impl NoiseHandshake {
    /// Start a new handshake as initiator
    pub fn initiate(
        pattern: NoisePattern,
        local_private_key: &PrivateKey,
        remote_public_key: Option<&PublicKey>,
    ) -> Result<Self> {
        let params: NoiseParams = pattern
            .to_params_string()
            .parse()
            .map_err(|e| Error::crypto(format!("Invalid Noise params: {}", e)))?;

        let mut builder = Builder::new(params);
        builder = builder.local_private_key(local_private_key.as_bytes());

        // Set remote static key if provided
        if let Some(remote_key) = remote_public_key {
            builder = builder.remote_public_key(remote_key.as_bytes());
        }

        let state = builder
            .build_initiator()
            .map_err(|e| Error::crypto(format!("Failed to build initiator: {}", e)))?;

        Ok(Self {
            state,
            is_initiator: true,
        })
    }

    /// Start a new handshake as responder
    pub fn respond(pattern: NoisePattern, local_private_key: &PrivateKey) -> Result<Self> {
        let params: NoiseParams = pattern
            .to_params_string()
            .parse()
            .map_err(|e| Error::crypto(format!("Invalid Noise params: {}", e)))?;

        let builder = Builder::new(params);
        let state = builder
            .local_private_key(local_private_key.as_bytes())
            .build_responder()
            .map_err(|e| Error::crypto(format!("Failed to build responder: {}", e)))?;

        Ok(Self {
            state,
            is_initiator: false,
        })
    }

    /// Write the next handshake message
    pub fn write_message(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut output = vec![0u8; 65535]; // Max Noise message size
        let len = self
            .state
            .write_message(payload, &mut output)
            .map_err(|e| Error::crypto(format!("Failed to write handshake message: {}", e)))?;

        output.truncate(len);
        Ok(output)
    }

    /// Read the next handshake message
    pub fn read_message(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        let mut output = vec![0u8; 65535];
        let len = self
            .state
            .read_message(message, &mut output)
            .map_err(|e| Error::crypto(format!("Failed to read handshake message: {}", e)))?;

        output.truncate(len);
        Ok(output)
    }

    /// Check if handshake is complete
    pub fn is_handshake_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Convert to transport mode after handshake completion
    pub fn into_transport_mode(self) -> Result<NoiseSession> {
        if !self.is_handshake_finished() {
            return Err(Error::crypto("Handshake not yet complete"));
        }

        let transport = self
            .state
            .into_transport_mode()
            .map_err(|e| Error::crypto(format!("Failed to enter transport mode: {}", e)))?;

        Ok(NoiseSession {
            state: transport,
            is_initiator: self.is_initiator,
        })
    }

    /// Get the remote static key if available
    pub fn get_remote_static_key(&self) -> Option<PublicKey> {
        self.state.get_remote_static().map(|bytes| {
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(bytes);
            PublicKey::from_bytes(key_bytes)
        })
    }
}

/// Represents an established Noise session in transport mode
#[derive(Debug)]
pub struct NoiseSession {
    state: TransportState,
    is_initiator: bool,
}

impl NoiseSession {
    /// Encrypt and authenticate a message
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut output = vec![0u8; plaintext.len() + 16]; // Add space for auth tag
        let len = self
            .state
            .write_message(plaintext, &mut output)
            .map_err(|e| Error::crypto(format!("Failed to encrypt message: {}", e)))?;

        output.truncate(len);
        Ok(output)
    }

    /// Decrypt and verify a message
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let mut output = vec![0u8; ciphertext.len()];
        let len = self
            .state
            .read_message(ciphertext, &mut output)
            .map_err(|e| Error::crypto(format!("Failed to decrypt message: {}", e)))?;

        output.truncate(len);
        Ok(output)
    }

    /// Get the session's remote static key
    pub fn get_remote_static_key(&self) -> Option<PublicKey> {
        self.state.get_remote_static().map(|bytes| {
            let mut key_bytes = [0u8; 32];
            key_bytes.copy_from_slice(bytes);
            PublicKey::from_bytes(key_bytes)
        })
    }

    /// Check if this session was initiated by us
    pub fn is_initiator(&self) -> bool {
        self.is_initiator
    }

    /// Rekey the session for forward secrecy
    pub fn rekey(&mut self) -> Result<()> {
        self.state.rekey_outgoing();
        self.state.rekey_incoming();
        Ok(())
    }
}

/// Helper for common Noise handshake patterns
pub struct NoiseHandshakeHelper;

impl NoiseHandshakeHelper {
    /// Perform a complete XX handshake (mutual authentication)
    pub async fn perform_xx_handshake(
        is_initiator: bool,
        local_keypair: &crate::keys::KeyPair,
        mut message_sender: impl FnMut(Vec<u8>) -> Result<()>,
        mut message_receiver: impl FnMut() -> Result<Vec<u8>>,
    ) -> Result<NoiseSession> {
        if is_initiator {
            let mut handshake =
                NoiseHandshake::initiate(NoisePattern::XX, local_keypair.private_key(), None)?;

            // Send first message
            let msg1 = handshake.write_message(&[])?;
            message_sender(msg1)?;

            // Read response
            let msg2 = message_receiver()?;
            handshake.read_message(&msg2)?;

            // Send final message
            let msg3 = handshake.write_message(&[])?;
            message_sender(msg3)?;

            handshake.into_transport_mode()
        } else {
            let mut handshake =
                NoiseHandshake::respond(NoisePattern::XX, local_keypair.private_key())?;

            // Read first message
            let msg1 = message_receiver()?;
            handshake.read_message(&msg1)?;

            // Send response
            let msg2 = handshake.write_message(&[])?;
            message_sender(msg2)?;

            // Read final message
            let msg3 = message_receiver()?;
            handshake.read_message(&msg3)?;

            handshake.into_transport_mode()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyPair;

    #[test]
    fn test_noise_handshake_xx() {
        let alice_keypair = KeyPair::generate();
        let bob_keypair = KeyPair::generate();

        // Start handshakes
        let mut alice =
            NoiseHandshake::initiate(NoisePattern::XX, alice_keypair.private_key(), None).unwrap();

        let mut bob = NoiseHandshake::respond(NoisePattern::XX, bob_keypair.private_key()).unwrap();

        // Perform handshake
        let msg1 = alice.write_message(&[]).unwrap();
        bob.read_message(&msg1).unwrap();

        let msg2 = bob.write_message(&[]).unwrap();
        alice.read_message(&msg2).unwrap();

        let msg3 = alice.write_message(&[]).unwrap();
        bob.read_message(&msg3).unwrap();

        // Convert to transport mode
        let mut alice_transport = alice.into_transport_mode().unwrap();
        let mut bob_transport = bob.into_transport_mode().unwrap();

        // Test encryption/decryption
        let plaintext = b"Hello, Bob!";
        let ciphertext = alice_transport.encrypt(plaintext).unwrap();
        let decrypted = bob_transport.decrypt(&ciphertext).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());

        // Test reverse direction
        let plaintext2 = b"Hello, Alice!";
        let ciphertext2 = bob_transport.encrypt(plaintext2).unwrap();
        let decrypted2 = alice_transport.decrypt(&ciphertext2).unwrap();

        assert_eq!(plaintext2, decrypted2.as_slice());
    }
}
