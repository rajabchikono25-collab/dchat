//! Double Ratchet Protocol Implementation
//!
//! This module implements the Double Ratchet algorithm as specified in
//! the Signal Protocol, providing forward secrecy and post-compromise
//! security for 1:1 encrypted messaging.
//!
//! # Overview
//!
//! The Double Ratchet combines three ratchets:
//! 1. **Diffie-Hellman Ratchet**: Provides post-compromise security by
//!    establishing new shared secrets when receiving messages
//! 2. **Sending Chain**: Symmetric ratchet for generating message keys
//! 3. **Receiving Chain**: Symmetric ratchet for decrypting incoming messages
//!
//! # Security Properties
//!
//! - **Forward Secrecy**: Past messages cannot be decrypted if current keys are compromised
//! - **Post-Compromise Security**: Security is restored after a key compromise
//!   once a DH ratchet step occurs
//! - **Message Ordering**: Out-of-order messages can be decrypted (with limits)
//!
//! # Usage
//!
//! ```rust,ignore
//! use dchat_crypto::ratchet::{DoubleRatchet, X3dhKeyBundle, X3dhResult};
//!
//! // Initiator (Alice) creates session after X3DH
//! let x3dh_result = X3dhKeyBundle::initiate(&alice_identity, &bob_prekey_bundle)?;
//! let mut alice_session = DoubleRatchet::init_sender(
//!     x3dh_result.shared_secret,
//!     bob_public_key,
//! )?;
//!
//! // Encrypt a message
//! let (header, ciphertext) = alice_session.encrypt(b"Hello Bob!")?;
//!
//! // Responder (Bob) creates session from initial message
//! let mut bob_session = DoubleRatchet::init_receiver(
//!     x3dh_shared_secret,
//!     bob_keypair,
//! )?;
//!
//! // Decrypt the message
//! let plaintext = bob_session.decrypt(&header, &ciphertext)?;
//! ```
//!
//! # References
//!
//! - [Signal Double Ratchet Specification](https://signal.org/docs/specifications/doubleratchet/)
//! - [X3DH Key Agreement Protocol](https://signal.org/docs/specifications/x3dh/)

mod chain;
mod header;
mod session;
mod skipped;
mod x3dh;

pub use chain::{ChainKey, MessageKey, KDF_CK, KDF_RK};
pub use header::{HeaderEncoder, MessageHeader};
pub use session::{DoubleRatchet, RatchetState, SessionConfig};
pub use skipped::{SkippedMessageKeys, MAX_SKIP};
pub use x3dh::{
    X3dhEphemeralKey, X3dhIdentityKey, X3dhKeyBundle, X3dhOneTimePreKey, X3dhPreKeyBundle,
    X3dhResult, X3dhSignedPreKey,
};

/// Default maximum number of message keys to skip (for out-of-order messages)
pub const DEFAULT_MAX_SKIP: usize = 1000;

/// Maximum age of skipped keys before they are discarded (in seconds)
pub const SKIPPED_KEY_MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60; // 7 days
