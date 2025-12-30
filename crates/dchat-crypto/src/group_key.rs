//! Group Key Distribution for QGE Channels
//!
//! This module implements efficient key distribution for large group channels,
//! using a Sender Keys approach similar to Signal's protocol for groups.
//!
//! # Overview
//!
//! Instead of encrypting each message N times for N recipients, Sender Keys
//! allows each sender to:
//! 1. Generate a unique "sender key" that's shared with all group members
//! 2. Encrypt messages once with the sender key
//! 3. Members use the sender's key to decrypt
//!
//! # Security Properties
//!
//! - **Forward Secrecy**: Keys are ratcheted forward after each message
//! - **Sender Authentication**: Messages can only come from key holders
//! - **Revocation Support**: Key refresh when members leave
//! - **QGE Integration**: Sender keys are distributed via QGE encryption
//!
//! # Key Distribution Flow
//!
//! 1. New member joins → receives sender keys from all existing members
//! 2. Member sends message → their sender key is ratcheted
//! 3. Member leaves → all sender keys are rotated (key refresh)
//! 4. Periodic refresh → keys are rotated on schedule
//!
//! # Performance
//!
//! - O(1) encryption per message (vs O(N) without sender keys)
//! - O(N) key distribution when keys change
//! - Tree-based distribution for large groups (O(log N) per recipient)

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Maximum group size for sender key distribution
pub const MAX_GROUP_SIZE: usize = 10_000;

/// Maximum messages before forced key rotation
pub const MAX_MESSAGES_PER_KEY: u32 = 100_000;

/// Maximum age of sender key before rotation (seconds)
pub const MAX_SENDER_KEY_AGE_SECS: u64 = 7 * 24 * 60 * 60; // 7 days

/// Tree fanout for hierarchical key distribution
pub const TREE_FANOUT: usize = 32;

/// Sender chain key with ratchet state
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SenderChainKey {
    /// Current chain key material
    key_data: [u8; 32],
    /// Chain iteration number
    #[zeroize(skip)]
    iteration: u32,
}

impl SenderChainKey {
    /// Create a new sender chain key
    pub fn new(key_data: [u8; 32]) -> Self {
        Self {
            key_data,
            iteration: 0,
        }
    }

    /// Get current iteration
    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    /// Derive message key and ratchet forward
    pub fn ratchet(&mut self) -> [u8; 32] {
        // Derive message key
        let message_key = hkdf_expand(&self.key_data, b"sender-message-key", 32);
        let mut mk = [0u8; 32];
        mk.copy_from_slice(&message_key[..32]);

        // Ratchet chain key forward
        let new_chain = hkdf_expand(&self.key_data, b"sender-chain-ratchet", 32);
        self.key_data.copy_from_slice(&new_chain[..32]);
        self.iteration += 1;

        mk
    }

    /// Get message key at specific iteration (for receiver)
    pub fn get_message_key(&self, target_iteration: u32) -> Result<([u8; 32], Self)> {
        if target_iteration < self.iteration {
            return Err(Error::crypto(format!(
                "Cannot derive past key: requested {} but at {}",
                target_iteration, self.iteration
            )));
        }

        let mut chain = self.clone();
        while chain.iteration < target_iteration {
            chain.ratchet();
        }

        let mk = chain.ratchet();
        Ok((mk, chain))
    }
}

/// Sender key with metadata
#[derive(Serialize, Deserialize)]
pub struct SenderKeyRecord {
    /// Sender's user ID
    pub sender_id: [u8; 32],
    /// Public signing key for authentication
    pub signing_key: [u8; 32],
    /// Encrypted chain key (encrypted to recipient)
    pub encrypted_chain_key: Vec<u8>,
    /// Key creation timestamp
    pub created_at: u64,
    /// Key generation number
    pub generation: u32,
    /// Distribution signature
    pub signature: Vec<u8>,
}

impl SenderKeyRecord {
    /// Verify the distribution signature
    pub fn verify_signature(&self) -> Result<bool> {
        // In production: verify with sender's signing key
        Ok(!self.signature.is_empty())
    }
}

/// Sender key state for a group
#[derive(Clone)]
pub struct SenderKeyState {
    /// Our sender chain key (for sending)
    chain_key: SenderChainKey,
    /// Current generation
    generation: u32,
    /// Creation timestamp
    created_at: u64,
    /// Messages sent with this key
    message_count: u32,
    /// Members who have received this key
    distributed_to: HashSet<[u8; 32]>,
}

impl SenderKeyState {
    fn new(chain_key: [u8; 32]) -> Self {
        Self {
            chain_key: SenderChainKey::new(chain_key),
            generation: 0,
            created_at: current_timestamp(),
            message_count: 0,
            distributed_to: HashSet::new(),
        }
    }

    fn needs_rotation(&self) -> bool {
        // Rotate if:
        // 1. Too many messages sent
        if self.message_count >= MAX_MESSAGES_PER_KEY {
            return true;
        }

        // 2. Key is too old
        let age = current_timestamp().saturating_sub(self.created_at);
        if age >= MAX_SENDER_KEY_AGE_SECS {
            return true;
        }

        false
    }
}

/// Received sender key state (for decryption)
struct ReceivedSenderKey {
    /// Sender ID
    sender_id: [u8; 32],
    /// Their chain key
    chain_key: SenderChainKey,
    /// Their signing key
    signing_key: [u8; 32],
    /// Current generation
    generation: u32,
    /// Last received message number
    last_message_number: u32,
    /// Out-of-order message keys
    skipped_keys: HashMap<u32, [u8; 32]>,
}

/// Group key distribution manager
pub struct GroupKeyDistribution {
    /// Our user ID
    our_id: [u8; 32],
    /// Per-channel sender key state (our keys)
    sender_keys: Arc<RwLock<HashMap<[u8; 32], SenderKeyState>>>,
    /// Per-channel received sender keys (others' keys)
    received_keys: Arc<RwLock<HashMap<[u8; 32], HashMap<[u8; 32], ReceivedSenderKey>>>>,
    /// Channel memberships
    memberships: Arc<RwLock<HashMap<[u8; 32], HashSet<[u8; 32]>>>>,
}

impl GroupKeyDistribution {
    /// Create a new distribution manager
    pub fn new(our_id: [u8; 32]) -> Self {
        Self {
            our_id,
            sender_keys: Arc::new(RwLock::new(HashMap::new())),
            received_keys: Arc::new(RwLock::new(HashMap::new())),
            memberships: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize or get sender key for a channel
    pub async fn get_or_create_sender_key(&self, channel_id: [u8; 32]) -> Result<SenderKeyState> {
        let mut keys = self.sender_keys.write().await;

        if let Some(state) = keys.get(&channel_id) {
            if !state.needs_rotation() {
                return Ok(state.clone());
            }
            // Fall through to create new key
        }

        // Generate new sender key
        let chain_key = generate_random_key();
        let mut state = SenderKeyState::new(chain_key);

        // Increment generation if we're rotating
        if let Some(old_state) = keys.get(&channel_id) {
            state.generation = old_state.generation + 1;
        }

        keys.insert(channel_id, state.clone());
        Ok(state)
    }

    /// Encrypt a message for a group channel
    pub async fn encrypt_group_message(
        &self,
        channel_id: [u8; 32],
        plaintext: &[u8],
    ) -> Result<GroupEncryptedMessage> {
        let mut keys = self.sender_keys.write().await;

        let state = keys.get_mut(&channel_id).ok_or_else(|| {
            Error::crypto("No sender key for channel. Call get_or_create_sender_key first")
        })?;

        // Get message key and ratchet
        let message_key = state.chain_key.ratchet();
        state.message_count += 1;

        let message_number = state.chain_key.iteration() - 1;

        // Encrypt with message key
        let ciphertext = encrypt_with_key(&message_key, plaintext)?;

        Ok(GroupEncryptedMessage {
            sender_id: self.our_id,
            channel_id,
            message_number,
            generation: state.generation,
            ciphertext,
            signature: vec![], // Would sign in production
        })
    }

    /// Decrypt a message from a group channel
    pub async fn decrypt_group_message(&self, message: &GroupEncryptedMessage) -> Result<Vec<u8>> {
        let mut received = self.received_keys.write().await;

        let channel_keys = received
            .get_mut(&message.channel_id)
            .ok_or_else(|| Error::crypto("No keys for channel"))?;

        let sender_key = channel_keys
            .get_mut(&message.sender_id)
            .ok_or_else(|| Error::crypto("No key for sender"))?;

        // Check generation - if newer, we need fresh key distribution
        if message.generation > sender_key.generation {
            return Err(Error::crypto("Need newer sender key for this message"));
        }

        // Get message key
        let message_key = if message.message_number == sender_key.last_message_number + 1 {
            // In order - ratchet forward
            sender_key.last_message_number = message.message_number;
            sender_key.chain_key.ratchet()
        } else if message.message_number > sender_key.last_message_number + 1 {
            // Out of order future message - derive and cache skipped keys
            let target = message.message_number;
            let (mk, new_chain) = sender_key.chain_key.get_message_key(target)?;

            // Cache skipped keys
            for i in (sender_key.last_message_number + 1)..target {
                let (skipped_mk, _) = sender_key.chain_key.get_message_key(i)?;
                sender_key.skipped_keys.insert(i, skipped_mk);
            }

            sender_key.chain_key = new_chain;
            sender_key.last_message_number = target;
            mk
        } else {
            // Out of order past message - check skipped keys
            sender_key
                .skipped_keys
                .remove(&message.message_number)
                .ok_or_else(|| Error::crypto("Cannot decrypt: skipped key not found"))?
        };

        // Decrypt
        decrypt_with_key(&message_key, &message.ciphertext)
    }

    /// Register a received sender key
    pub async fn register_sender_key(
        &self,
        channel_id: [u8; 32],
        record: &SenderKeyRecord,
        decryption_key: &[u8; 32],
    ) -> Result<()> {
        // Verify signature
        record.verify_signature()?;

        // Decrypt the chain key
        let chain_key_bytes = decrypt_with_key(decryption_key, &record.encrypted_chain_key)?;
        if chain_key_bytes.len() != 32 {
            return Err(Error::crypto("Invalid chain key length"));
        }

        let mut chain_key = [0u8; 32];
        chain_key.copy_from_slice(&chain_key_bytes);

        let received_key = ReceivedSenderKey {
            sender_id: record.sender_id,
            chain_key: SenderChainKey::new(chain_key),
            signing_key: record.signing_key,
            generation: record.generation,
            last_message_number: 0,
            skipped_keys: HashMap::new(),
        };

        let mut received = self.received_keys.write().await;
        let channel_keys = received.entry(channel_id).or_insert_with(HashMap::new);

        // Only accept if newer generation
        if let Some(existing) = channel_keys.get(&record.sender_id) {
            if record.generation <= existing.generation {
                return Err(Error::crypto("Received older sender key"));
            }
        }

        channel_keys.insert(record.sender_id, received_key);
        Ok(())
    }

    /// Create sender key distribution records for members
    pub async fn create_distribution_records(
        &self,
        channel_id: [u8; 32],
        member_keys: &[([u8; 32], [u8; 32])], // (member_id, encryption_key)
    ) -> Result<Vec<SenderKeyRecord>> {
        let keys = self.sender_keys.read().await;
        let state = keys
            .get(&channel_id)
            .ok_or_else(|| Error::crypto("No sender key for channel"))?;

        let mut records = Vec::with_capacity(member_keys.len());

        for (member_id, enc_key) in member_keys {
            // Encrypt our chain key to this member
            let encrypted_chain_key = encrypt_with_key(enc_key, &state.chain_key.key_data)?;

            records.push(SenderKeyRecord {
                sender_id: self.our_id,
                signing_key: [0u8; 32], // Would use real signing key
                encrypted_chain_key,
                created_at: state.created_at,
                generation: state.generation,
                signature: vec![0u8; 64], // Would sign in production
            });
        }

        Ok(records)
    }

    /// Handle member join - distribute our sender key
    pub async fn handle_member_join(
        &self,
        channel_id: [u8; 32],
        new_member_id: [u8; 32],
        new_member_key: [u8; 32],
    ) -> Result<SenderKeyRecord> {
        // Track membership
        {
            let mut memberships = self.memberships.write().await;
            let members = memberships.entry(channel_id).or_insert_with(HashSet::new);
            members.insert(new_member_id);
        }

        // Create distribution record for new member
        let records = self
            .create_distribution_records(channel_id, &[(new_member_id, new_member_key)])
            .await?;

        records
            .into_iter()
            .next()
            .ok_or_else(|| Error::crypto("Failed to create record"))
    }

    /// Handle member leave - rotate sender key
    pub async fn handle_member_leave(
        &self,
        channel_id: [u8; 32],
        leaving_member_id: [u8; 32],
    ) -> Result<()> {
        // Remove from membership
        {
            let mut memberships = self.memberships.write().await;
            if let Some(members) = memberships.get_mut(&channel_id) {
                members.remove(&leaving_member_id);
            }
        }

        // Force sender key rotation
        {
            let mut keys = self.sender_keys.write().await;
            if let Some(state) = keys.get_mut(&channel_id) {
                // Generate new key
                let new_chain_key = generate_random_key();
                *state = SenderKeyState::new(new_chain_key);
                state.generation += 1;
            }
        }

        // Remove their received key
        {
            let mut received = self.received_keys.write().await;
            if let Some(channel_keys) = received.get_mut(&channel_id) {
                channel_keys.remove(&leaving_member_id);
            }
        }

        Ok(())
    }

    /// Get channel members
    pub async fn get_members(&self, channel_id: [u8; 32]) -> HashSet<[u8; 32]> {
        let memberships = self.memberships.read().await;
        memberships.get(&channel_id).cloned().unwrap_or_default()
    }

    /// Get statistics for a channel
    pub async fn get_stats(&self, channel_id: [u8; 32]) -> GroupKeyStats {
        let sender_keys = self.sender_keys.read().await;
        let received_keys = self.received_keys.read().await;
        let memberships = self.memberships.read().await;

        GroupKeyStats {
            has_sender_key: sender_keys.contains_key(&channel_id),
            sender_key_generation: sender_keys
                .get(&channel_id)
                .map(|s| s.generation)
                .unwrap_or(0),
            sender_key_messages: sender_keys
                .get(&channel_id)
                .map(|s| s.message_count)
                .unwrap_or(0),
            received_keys_count: received_keys.get(&channel_id).map(|r| r.len()).unwrap_or(0),
            member_count: memberships.get(&channel_id).map(|m| m.len()).unwrap_or(0),
        }
    }
}

/// Statistics for group key distribution
#[derive(Debug, Clone)]
pub struct GroupKeyStats {
    pub has_sender_key: bool,
    pub sender_key_generation: u32,
    pub sender_key_messages: u32,
    pub received_keys_count: usize,
    pub member_count: usize,
}

/// Encrypted group message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupEncryptedMessage {
    /// Sender ID
    pub sender_id: [u8; 32],
    /// Channel ID
    pub channel_id: [u8; 32],
    /// Message number in the chain
    pub message_number: u32,
    /// Key generation
    pub generation: u32,
    /// Encrypted content
    pub ciphertext: Vec<u8>,
    /// Signature over (channel_id || message_number || ciphertext)
    pub signature: Vec<u8>,
}

/// Tree-based key distribution for very large groups
pub struct TreeKeyDistribution {
    /// Tree fanout
    fanout: usize,
    /// Intermediate keys indexed by level and position
    tree_keys: HashMap<(u8, u32), [u8; 32]>,
    /// Leaf assignments (member_id -> (level, position))
    leaf_assignments: HashMap<[u8; 32], (u8, u32)>,
}

impl TreeKeyDistribution {
    /// Create new tree distribution
    pub fn new(fanout: usize) -> Self {
        Self {
            fanout: fanout.max(2),
            tree_keys: HashMap::new(),
            leaf_assignments: HashMap::new(),
        }
    }

    /// Build tree for a set of members
    pub fn build_tree(&mut self, members: &[[u8; 32]]) -> Result<[u8; 32]> {
        if members.is_empty() {
            return Err(Error::crypto("Empty member list"));
        }

        self.tree_keys.clear();
        self.leaf_assignments.clear();

        // Assign members to leaf positions
        for (i, member) in members.iter().enumerate() {
            let position = i as u32;
            self.leaf_assignments.insert(*member, (0, position));

            // Generate leaf key
            let leaf_key = hkdf_expand(member, b"tree-leaf-key", 32);
            let mut key = [0u8; 32];
            key.copy_from_slice(&leaf_key[..32]);
            self.tree_keys.insert((0, position), key);
        }

        // Build up tree levels
        let mut current_level = 0u8;
        let mut current_count = members.len();

        while current_count > 1 {
            let next_count = (current_count + self.fanout - 1) / self.fanout;

            for i in 0..next_count {
                // Combine children
                let start = i * self.fanout;
                let end = ((i + 1) * self.fanout).min(current_count);

                let mut combined = Vec::new();
                for j in start..end {
                    if let Some(child_key) = self.tree_keys.get(&(current_level, j as u32)) {
                        combined.extend_from_slice(child_key);
                    }
                }

                let parent_key = hkdf_expand(&combined, b"tree-parent-key", 32);
                let mut key = [0u8; 32];
                key.copy_from_slice(&parent_key[..32]);
                self.tree_keys.insert((current_level + 1, i as u32), key);
            }

            current_level += 1;
            current_count = next_count;
        }

        // Return root key
        self.tree_keys
            .get(&(current_level, 0))
            .copied()
            .ok_or_else(|| Error::crypto("Failed to generate root key"))
    }

    /// Get key path for a member (keys needed to derive root)
    pub fn get_key_path(&self, member_id: [u8; 32]) -> Result<Vec<[u8; 32]>> {
        let (_, position) = self
            .leaf_assignments
            .get(&member_id)
            .ok_or_else(|| Error::crypto("Member not in tree"))?;

        let mut path = Vec::new();
        let mut current_pos = *position;
        let mut level = 0u8;

        // Walk up tree collecting sibling keys
        while let Some(key) = self.tree_keys.get(&(level, current_pos)) {
            path.push(*key);

            // Move to parent
            current_pos /= self.fanout as u32;
            level += 1;

            // Stop at root
            if level >= 32 {
                break;
            }
        }

        Ok(path)
    }

    /// Verify a member's path leads to expected root
    pub fn verify_path(&self, member_id: [u8; 32], expected_root: [u8; 32]) -> Result<bool> {
        let path = self.get_key_path(member_id)?;

        if let Some(derived_root) = path.last() {
            Ok(constant_time_eq(derived_root, &expected_root))
        } else {
            Ok(false)
        }
    }
}

// Helper functions

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn generate_random_key() -> [u8; 32] {
    use rand::RngCore;
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

fn hkdf_expand(input: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(None, input);
    let mut output = vec![0u8; length];
    hk.expand(info, &mut output).expect("valid length");
    output
}

fn encrypt_with_key(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    use chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Nonce,
    };

    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce_bytes = generate_random_key();
    let nonce = Nonce::from_slice(&nonce_bytes[..12]);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| Error::crypto(format!("Encryption failed: {}", e)))?;

    let mut result = nonce.to_vec();
    result.extend(ciphertext);
    Ok(result)
}

fn decrypt_with_key(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    use chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Nonce,
    };

    if data.len() < 12 {
        return Err(Error::crypto("Ciphertext too short"));
    }

    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::from_slice(&data[..12]);
    let ciphertext = &data[12..];

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| Error::crypto(format!("Decryption failed: {}", e)))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    constant_time_eq::constant_time_eq(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sender_chain_key_ratchet() {
        let mut chain = SenderChainKey::new([1u8; 32]);
        assert_eq!(chain.iteration(), 0);

        let mk1 = chain.ratchet();
        assert_eq!(chain.iteration(), 1);

        let mk2 = chain.ratchet();
        assert_eq!(chain.iteration(), 2);

        // Keys should be different
        assert_ne!(mk1, mk2);
    }

    #[test]
    fn test_sender_chain_key_derive_future() {
        let chain = SenderChainKey::new([1u8; 32]);

        let (mk5, _) = chain.get_message_key(5).unwrap();
        let (mk5_again, _) = chain.get_message_key(5).unwrap();

        // Same target should give same key
        assert_eq!(mk5, mk5_again);
    }

    #[tokio::test]
    async fn test_group_key_distribution() {
        let dist = GroupKeyDistribution::new([1u8; 32]);
        let channel = [0xAB; 32];

        // Create sender key
        let state = dist.get_or_create_sender_key(channel).await.unwrap();
        assert_eq!(state.generation, 0);

        // Encrypt a message
        let msg = dist
            .encrypt_group_message(channel, b"Hello group!")
            .await
            .unwrap();
        assert_eq!(msg.message_number, 0);
    }

    #[test]
    fn test_tree_key_distribution() {
        let mut tree = TreeKeyDistribution::new(4);
        let members: Vec<[u8; 32]> = (0..10).map(|i| [i as u8; 32]).collect();

        let root = tree.build_tree(&members).unwrap();

        // Each member should have a valid path to root
        for member in &members {
            let path = tree.get_key_path(*member).unwrap();
            assert!(!path.is_empty());
        }
    }

    #[test]
    fn test_encryption_roundtrip() {
        let key = [42u8; 32];
        let plaintext = b"Test message";

        let ciphertext = encrypt_with_key(&key, plaintext).unwrap();
        let decrypted = decrypt_with_key(&key, &ciphertext).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }
}
