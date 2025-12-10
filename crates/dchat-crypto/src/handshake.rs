//! Handshake management combining Noise Protocol with key rotation

use crate::{
    keys::{KeyPair, PrivateKey, PublicKey},
    noise::{NoiseHandshake, NoisePattern, NoiseSession},
    rotation::KeyRotationManager,
};
use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Handshake state for a peer connection
#[derive(Debug)]
pub enum HandshakeState {
    /// No handshake started
    None,
    /// Handshake in progress
    InProgress {
        handshake: NoiseHandshake,
        started_at: DateTime<Utc>,
    },
    /// Handshake completed
    Completed {
        session: NoiseSession,
        completed_at: DateTime<Utc>,
        remote_static_key: Option<PublicKey>,
    },
    /// Handshake failed
    Failed {
        error: String,
        failed_at: DateTime<Utc>,
    },
}

/// Manages handshakes with multiple peers
/// 
/// Provides key rotation, timeout handling, and state management
/// for cryptographic handshakes with network peers.
pub struct HandshakeManager {
    local_keypair: KeyPair,
    rotation_manager: KeyRotationManager,
    peer_handshakes: HashMap<String, HandshakeState>,
    handshake_timeout_seconds: u64,
}

impl HandshakeManager {
    /// Get the local keypair used for handshakes
    pub fn local_keypair(&self) -> &KeyPair {
        &self.local_keypair
    }

    /// Create a new handshake manager
    pub fn new(master_key: PrivateKey, handshake_timeout_seconds: u64) -> Self {
        let local_keypair = KeyPair::from_private_key(master_key.clone());
        let rotation_manager =
            KeyRotationManager::new(master_key, crate::rotation::RotationPolicy::default());

        Self {
            local_keypair,
            rotation_manager,
            peer_handshakes: HashMap::new(),
            handshake_timeout_seconds,
        }
    }

    /// Initiate a handshake with a peer
    pub fn initiate_handshake(
        &mut self,
        peer_id: &str,
        pattern: NoisePattern,
        remote_static_key: Option<&PublicKey>,
    ) -> Result<Vec<u8>> {
        // Get or rotate key for this peer
        let handshake_key = self
            .rotation_manager
            .get_key(&format!("handshake:{}", peer_id))?;

        let mut handshake =
            NoiseHandshake::initiate(pattern, handshake_key.private_key(), remote_static_key)?;

        // Write first message
        let first_message = handshake.write_message(&[])?;

        // Store handshake state
        self.peer_handshakes.insert(
            peer_id.to_string(),
            HandshakeState::InProgress {
                handshake,
                started_at: Utc::now(),
            },
        );

        Ok(first_message)
    }

    /// Respond to a handshake initiation
    pub fn respond_to_handshake(
        &mut self,
        peer_id: &str,
        pattern: NoisePattern,
        initial_message: &[u8],
    ) -> Result<Vec<u8>> {
        // Get or rotate key for this peer
        let handshake_key = self
            .rotation_manager
            .get_key(&format!("handshake:{}", peer_id))?;

        let mut handshake = NoiseHandshake::respond(pattern, handshake_key.private_key())?;

        // Read initial message
        handshake.read_message(initial_message)?;

        // Write response
        let response = handshake.write_message(&[])?;

        // Store handshake state
        self.peer_handshakes.insert(
            peer_id.to_string(),
            HandshakeState::InProgress {
                handshake,
                started_at: Utc::now(),
            },
        );

        Ok(response)
    }

    /// Process a handshake message
    pub fn process_handshake_message(
        &mut self,
        peer_id: &str,
        message: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        let state = self
            .peer_handshakes
            .get_mut(peer_id)
            .ok_or_else(|| Error::crypto("No handshake in progress with peer"))?;

        match state {
            HandshakeState::InProgress { handshake, .. } => {
                // Read the message
                handshake.read_message(message)?;

                // Check if we need to send a response
                if handshake.is_handshake_finished() {
                    // Handshake complete - we need to take ownership
                    let old_state = std::mem::replace(state, HandshakeState::None);
                    if let HandshakeState::InProgress { handshake, .. } = old_state {
                        let session = handshake.into_transport_mode()?;
                        let remote_static_key = session.get_remote_static_key();

                        *state = HandshakeState::Completed {
                            session,
                            completed_at: Utc::now(),
                            remote_static_key,
                        };
                    }

                    Ok(None) // No response needed
                } else {
                    // Send next message
                    let response = handshake.write_message(&[])?;

                    // Check again if handshake is now complete
                    if handshake.is_handshake_finished() {
                        let old_state = std::mem::replace(state, HandshakeState::None);
                        if let HandshakeState::InProgress { handshake, .. } = old_state {
                            let session = handshake.into_transport_mode()?;
                            let remote_static_key = session.get_remote_static_key();

                            *state = HandshakeState::Completed {
                                session,
                                completed_at: Utc::now(),
                                remote_static_key,
                            };
                        }
                    }

                    Ok(Some(response))
                }
            }
            _ => Err(Error::crypto(
                "Invalid handshake state for processing message",
            )),
        }
    }

    /// Get the session for a completed handshake
    pub fn get_session(&mut self, peer_id: &str) -> Result<&mut NoiseSession> {
        match self.peer_handshakes.get_mut(peer_id) {
            Some(HandshakeState::Completed { session, .. }) => Ok(session),
            Some(HandshakeState::InProgress { .. }) => {
                Err(Error::crypto("Handshake still in progress"))
            }
            Some(HandshakeState::Failed { error, .. }) => {
                Err(Error::crypto(format!("Handshake failed: {}", error)))
            }
            Some(HandshakeState::None) | None => Err(Error::crypto("No handshake with peer")),
        }
    }

    /// Check for timed out handshakes and clean them up
    pub fn cleanup_timed_out_handshakes(&mut self) -> Vec<String> {
        let now = Utc::now();
        let timeout_duration = chrono::Duration::seconds(self.handshake_timeout_seconds as i64);
        let mut timed_out = Vec::new();

        self.peer_handshakes.retain(|peer_id, state| {
            match state {
                HandshakeState::InProgress { started_at, .. } => {
                    if now - *started_at > timeout_duration {
                        timed_out.push(peer_id.clone());
                        false // Remove this handshake
                    } else {
                        true // Keep this handshake
                    }
                }
                _ => true, // Keep completed, failed, or none states
            }
        });

        timed_out
    }

    /// Get handshake state for a peer
    pub fn get_handshake_state(&self, peer_id: &str) -> Option<&HandshakeState> {
        self.peer_handshakes.get(peer_id)
    }

    /// Reset handshake for a peer
    pub fn reset_handshake(&mut self, peer_id: &str) {
        self.peer_handshakes.remove(peer_id);
    }

    /// Get list of peers with active sessions
    pub fn get_active_peers(&self) -> Vec<String> {
        self.peer_handshakes
            .iter()
            .filter_map(|(peer_id, state)| {
                matches!(state, HandshakeState::Completed { .. }).then_some(peer_id.clone())
            })
            .collect()
    }

    /// Trigger key rotation for handshakes
    pub fn rotate_handshake_keys(&mut self) -> Result<()> {
        // Rotate the main handshake key
        self.rotation_manager
            .trigger_rotation_event(crate::rotation::RotationEvent::Manual)?;

        Ok(())
    }
}

/// Handshake configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeConfig {
    pub default_pattern: NoisePattern,
    pub timeout_seconds: u64,
    pub enable_key_rotation: bool,
    pub rotation_interval_hours: u32,
}

impl Default for HandshakeConfig {
    fn default() -> Self {
        Self {
            default_pattern: NoisePattern::XX,
            timeout_seconds: 30,
            enable_key_rotation: true,
            rotation_interval_hours: 24,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::PrivateKey;

    #[test]
    fn test_handshake_manager_xx() {
        let alice_key = PrivateKey::generate();
        let bob_key = PrivateKey::generate();

        let mut alice_manager = HandshakeManager::new(alice_key, 30);
        let mut bob_manager = HandshakeManager::new(bob_key, 30);

        // Alice initiates handshake
        let msg1 = alice_manager
            .initiate_handshake("bob", NoisePattern::XX, None)
            .unwrap();

        // Bob responds
        let msg2 = bob_manager
            .respond_to_handshake("alice", NoisePattern::XX, &msg1)
            .unwrap();

        // Alice processes response
        let msg3 = alice_manager
            .process_handshake_message("bob", &msg2)
            .unwrap();

        // Bob processes final message
        if let Some(msg3) = msg3 {
            bob_manager
                .process_handshake_message("alice", &msg3)
                .unwrap();
        }

        // Both should have completed sessions
        assert!(alice_manager.get_session("bob").is_ok());
        assert!(bob_manager.get_session("alice").is_ok());

        // Test encryption/decryption
        let alice_session = alice_manager.get_session("bob").unwrap();
        let bob_session = bob_manager.get_session("alice").unwrap();

        let plaintext = b"Hello from Alice!";
        let ciphertext = alice_session.encrypt(plaintext).unwrap();
        let decrypted = bob_session.decrypt(&ciphertext).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_handshake_timeout_cleanup() {
        let key = PrivateKey::generate();
        let mut manager = HandshakeManager::new(key, 1); // 1 second timeout

        // Start a handshake but don't complete it
        manager
            .initiate_handshake("peer1", NoisePattern::XX, None)
            .unwrap();

        // Should have one in-progress handshake
        assert!(matches!(
            manager.get_handshake_state("peer1"),
            Some(HandshakeState::InProgress { .. })
        ));

        // Wait for timeout (in real test, we'd mock time)
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Cleanup should remove the timed-out handshake
        let timed_out = manager.cleanup_timed_out_handshakes();
        assert_eq!(timed_out, vec!["peer1"]);
        assert!(manager.get_handshake_state("peer1").is_none());
    }
}
