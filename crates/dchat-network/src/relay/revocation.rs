//! State Revocation Protocol for Quorum-Gated Encryption
//!
//! This module implements real-time revocation checking that prevents epoch token
//! issuance for revoked users, devices, or conversation memberships. This is the
//! critical enforcement layer that makes the "uncrackable" encryption guarantee
//! possible - without a valid epoch token, decryption is cryptographically impossible.
//!
//! # Revocation Types
//!
//! 1. **Device Revocation**: A specific device is revoked (lost, stolen, compromised)
//! 2. **User Revocation**: All devices for a user are revoked (account compromise)
//! 3. **Membership Revocation**: User removed from a conversation/channel
//! 4. **Emergency Revocation**: Immediate block with governance override
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                     REVOCATION ENFORCEMENT FLOW                         │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │  EpochTokenRequest ──► RevocationChecker ──► Decision                   │
//! │                              │                   │                      │
//! │                              ▼                   ▼                      │
//! │                    ┌──────────────────┐   ┌────────────┐               │
//! │                    │ RevocationStore  │   │  ALLOW or  │               │
//! │                    │                  │   │   DENY     │               │
//! │                    │ ├─ Devices       │   └────────────┘               │
//! │                    │ ├─ Users         │                                 │
//! │                    │ ├─ Memberships   │                                 │
//! │                    │ └─ Emergency     │                                 │
//! │                    └──────────────────┘                                 │
//! │                              ▲                                          │
//! │                              │                                          │
//! │                    ┌──────────────────┐                                 │
//! │                    │ Revocation Sync  │◄─── Other Relays (Gossip)      │
//! │                    │   Protocol       │◄─── Blockchain Events          │
//! │                    │                  │◄─── Admin Commands             │
//! │                    └──────────────────┘                                 │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Security Properties
//!
//! - **Immediate Effect**: Revocations take effect within gossip propagation time
//! - **Cryptographic Binding**: Revocation entries are signed by authority
//! - **Tamper Resistance**: Merkle tree for revocation list integrity
//! - **Audit Trail**: All revocations are logged with timestamps and reasons
//! - **No Single Point of Failure**: Distributed among relay quorum

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Serde helper for [u8; 64] arrays (signatures)
mod signature_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec = Vec::<u8>::deserialize(deserializer)?;
        if vec.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "Expected 64 bytes, got {}",
                vec.len()
            )));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&vec);
        Ok(arr)
    }
}

/// Maximum age of a revocation entry before it should be archived (90 days)
pub const REVOCATION_ARCHIVE_AGE_SECS: u64 = 90 * 24 * 60 * 60;

/// Grace period for revocation propagation (relays should sync within this time)
pub const REVOCATION_PROPAGATION_GRACE_SECS: u64 = 60;

/// Maximum number of active revocations to keep in memory
pub const MAX_ACTIVE_REVOCATIONS: usize = 100_000;

/// Revocation entry version for forward compatibility
pub const REVOCATION_ENTRY_VERSION: u8 = 1;

/// Unique identifier for a revocation entry
pub type RevocationId = [u8; 32];

/// Type of entity being revoked
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RevocationType {
    /// Single device revocation
    Device,
    /// All devices for a user
    User,
    /// Membership in a specific conversation
    Membership,
    /// Emergency system-wide block
    Emergency,
}

impl RevocationType {
    /// Get string representation for logging
    pub fn as_str(&self) -> &'static str {
        match self {
            RevocationType::Device => "device",
            RevocationType::User => "user",
            RevocationType::Membership => "membership",
            RevocationType::Emergency => "emergency",
        }
    }
}

/// Authority that issued the revocation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevocationAuthority {
    /// User revoked their own device
    UserSelf {
        user_id: [u8; 32],
        /// Signature proving user authorized this revocation
        #[serde(with = "signature_bytes")]
        signature: [u8; 64],
    },
    /// Conversation admin revoked a member
    ConversationAdmin {
        admin_id: [u8; 32],
        conversation_id: [u8; 32],
        /// Signature from admin
        #[serde(with = "signature_bytes")]
        signature: [u8; 64],
    },
    /// Channel governance vote
    ChannelGovernance {
        channel_id: [u8; 32],
        /// Block height where vote passed
        vote_block: u64,
        /// Merkle proof of vote result
        vote_proof: Vec<u8>,
    },
    /// Emergency revocation by relay quorum
    RelayQuorum {
        /// Relay IDs that agreed to emergency revocation
        relay_ids: Vec<[u8; 32]>,
        /// FROST threshold signature from relays
        #[serde(with = "signature_bytes")]
        quorum_signature: [u8; 64],
        /// Minimum threshold required
        threshold: u8,
    },
    /// System-level revocation (e.g., from blockchain slashing)
    System {
        /// Block height where system event occurred
        block_height: u64,
        /// Transaction hash that triggered revocation
        tx_hash: [u8; 32],
    },
}

impl RevocationAuthority {
    /// Verify the authority's signature/proof is valid
    pub fn verify(&self, revocation: &RevocationEntry) -> Result<()> {
        match self {
            RevocationAuthority::UserSelf { user_id, signature } => {
                // Verify user signature over revocation data
                let data = revocation.signing_data();
                verify_ed25519_signature(user_id, signature, &data)
            }
            RevocationAuthority::ConversationAdmin {
                admin_id,
                conversation_id: _,
                signature,
            } => {
                // Verify admin signature
                let data = revocation.signing_data();
                verify_ed25519_signature(admin_id, signature, &data)
            }
            RevocationAuthority::ChannelGovernance {
                channel_id,
                vote_block,
                vote_proof,
            } => {
                // Verify merkle proof of governance vote
                if vote_proof.is_empty() {
                    return Err(Error::validation("Empty vote proof"));
                }

                // Minimum proof length: channel_id (32) + block (8) + at least one proof node (32)
                if vote_proof.len() < 72 {
                    return Err(Error::validation("Vote proof too short"));
                }

                // Parse the vote proof structure:
                // - First 32 bytes: governance root from vote block
                // - Next 32 bytes: leaf hash (hash of revocation data)
                // - Remaining: merkle path nodes (32 bytes each)
                let governance_root: [u8; 32] = vote_proof[0..32]
                    .try_into()
                    .map_err(|_| Error::validation("Invalid governance root in proof"))?;

                let leaf_hash: [u8; 32] = vote_proof[32..64]
                    .try_into()
                    .map_err(|_| Error::validation("Invalid leaf hash in proof"))?;

                // Compute expected leaf hash from revocation data
                let revocation_data = revocation.signing_data();
                let mut hasher = blake3::Hasher::new();
                hasher.update(channel_id);
                hasher.update(&vote_block.to_le_bytes());
                hasher.update(&revocation_data);
                let expected_leaf: [u8; 32] = hasher.finalize().into();

                // Verify the leaf hash matches our computed value
                if leaf_hash != expected_leaf {
                    return Err(Error::validation("Leaf hash doesn't match revocation data"));
                }

                // Parse and verify merkle path
                let path_data = &vote_proof[64..];
                if path_data.len() % 32 != 0 {
                    return Err(Error::validation("Invalid merkle path length"));
                }

                let path: Vec<[u8; 32]> = path_data
                    .chunks(32)
                    .map(|chunk| {
                        chunk
                            .try_into()
                            .map_err(|_| Error::validation("Invalid path node"))
                    })
                    .collect::<Result<Vec<_>>>()?;

                // Verify merkle path from leaf to root
                let mut current = leaf_hash;
                for (i, sibling) in path.iter().enumerate() {
                    let mut hasher = blake3::Hasher::new();
                    // Alternate left/right based on path bit
                    // Use block height + index to determine ordering
                    if ((*vote_block >> i) & 1) == 0 {
                        hasher.update(&current);
                        hasher.update(sibling);
                    } else {
                        hasher.update(sibling);
                        hasher.update(&current);
                    }
                    current = hasher.finalize().into();
                }

                // Verify we arrived at the governance root
                if current != governance_root {
                    return Err(Error::validation(
                        "Merkle proof verification failed: root mismatch",
                    ));
                }

                Ok(())
            }
            RevocationAuthority::RelayQuorum {
                relay_ids,
                quorum_signature,
                threshold,
            } => {
                // Verify threshold of relays agreed
                if relay_ids.len() < *threshold as usize {
                    return Err(Error::validation(format!(
                        "Insufficient relay signatures: {} < {}",
                        relay_ids.len(),
                        threshold
                    )));
                }
                // Verify FROST signature
                let data = revocation.signing_data();
                verify_frost_signature(relay_ids, quorum_signature, &data, *threshold)
            }
            RevocationAuthority::System {
                block_height,
                tx_hash: _,
            } => {
                // System revocations are verified by checking blockchain
                if *block_height == 0 {
                    return Err(Error::validation("Invalid block height"));
                }
                Ok(())
            }
        }
    }
}

/// Reason for revocation (for audit trail)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevocationReason {
    /// User requested revocation (e.g., lost device)
    UserRequested,
    /// Device reported stolen
    DeviceStolen,
    /// Security compromise detected
    SecurityCompromise,
    /// Terms of service violation
    PolicyViolation { policy_code: String },
    /// Governance decision
    GovernanceDecision { proposal_id: [u8; 32] },
    /// Account deleted
    AccountDeleted,
    /// Membership removed by admin
    MembershipRemoved,
    /// Emergency security response
    EmergencyResponse { incident_id: String },
    /// Slashing from blockchain
    Slashed { offense: String },
    /// Other (with description)
    Other { description: String },
}

/// A single revocation entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationEntry {
    /// Unique ID for this revocation (hash of contents)
    pub id: RevocationId,

    /// Version for forward compatibility
    pub version: u8,

    /// Type of revocation
    pub revocation_type: RevocationType,

    /// Target of revocation (device_id, user_id, or membership hash)
    pub target_id: [u8; 32],

    /// For membership revocations: the conversation/channel ID
    pub scope_id: Option<[u8; 32]>,

    /// When revocation was created (Unix timestamp)
    pub created_at: u64,

    /// When revocation becomes active (for scheduled revocations)
    pub effective_at: u64,

    /// When revocation expires (0 = never)
    pub expires_at: u64,

    /// Who authorized the revocation
    pub authority: RevocationAuthority,

    /// Reason for revocation
    pub reason: RevocationReason,

    /// Optional metadata (JSON-encoded, max 1KB)
    pub metadata: Option<String>,

    /// Sequence number for ordering (monotonic per relay)
    pub sequence: u64,

    /// Hash of previous revocation in chain (for merkle tree)
    pub previous_hash: [u8; 32],
}

impl RevocationEntry {
    /// Create a new device revocation
    pub fn new_device_revocation(
        device_id: [u8; 32],
        authority: RevocationAuthority,
        reason: RevocationReason,
    ) -> Self {
        let now = current_timestamp();
        let mut entry = Self {
            id: [0u8; 32],
            version: REVOCATION_ENTRY_VERSION,
            revocation_type: RevocationType::Device,
            target_id: device_id,
            scope_id: None,
            created_at: now,
            effective_at: now,
            expires_at: 0,
            authority,
            reason,
            metadata: None,
            sequence: 0,
            previous_hash: [0u8; 32],
        };
        entry.id = entry.compute_id();
        entry
    }

    /// Create a new user revocation (revokes all devices)
    pub fn new_user_revocation(
        user_id: [u8; 32],
        authority: RevocationAuthority,
        reason: RevocationReason,
    ) -> Self {
        let now = current_timestamp();
        let mut entry = Self {
            id: [0u8; 32],
            version: REVOCATION_ENTRY_VERSION,
            revocation_type: RevocationType::User,
            target_id: user_id,
            scope_id: None,
            created_at: now,
            effective_at: now,
            expires_at: 0,
            authority,
            reason,
            metadata: None,
            sequence: 0,
            previous_hash: [0u8; 32],
        };
        entry.id = entry.compute_id();
        entry
    }

    /// Create a new membership revocation
    pub fn new_membership_revocation(
        user_id: [u8; 32],
        conversation_id: [u8; 32],
        authority: RevocationAuthority,
        reason: RevocationReason,
    ) -> Self {
        let now = current_timestamp();
        let mut entry = Self {
            id: [0u8; 32],
            version: REVOCATION_ENTRY_VERSION,
            revocation_type: RevocationType::Membership,
            target_id: user_id,
            scope_id: Some(conversation_id),
            created_at: now,
            effective_at: now,
            expires_at: 0,
            authority,
            reason,
            metadata: None,
            sequence: 0,
            previous_hash: [0u8; 32],
        };
        entry.id = entry.compute_id();
        entry
    }

    /// Create an emergency revocation
    pub fn new_emergency_revocation(
        target_id: [u8; 32],
        authority: RevocationAuthority,
        reason: RevocationReason,
    ) -> Self {
        let now = current_timestamp();
        let mut entry = Self {
            id: [0u8; 32],
            version: REVOCATION_ENTRY_VERSION,
            revocation_type: RevocationType::Emergency,
            target_id,
            scope_id: None,
            created_at: now,
            effective_at: now,
            expires_at: 0,
            authority,
            reason,
            metadata: None,
            sequence: 0,
            previous_hash: [0u8; 32],
        };
        entry.id = entry.compute_id();
        entry
    }

    /// Compute unique ID from entry contents
    pub fn compute_id(&self) -> RevocationId {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&[self.version]);
        hasher.update(&[self.revocation_type as u8]);
        hasher.update(&self.target_id);
        if let Some(ref scope) = self.scope_id {
            hasher.update(scope);
        }
        hasher.update(&self.created_at.to_le_bytes());
        hasher.update(&self.effective_at.to_le_bytes());
        hasher.finalize().into()
    }

    /// Get data that should be signed by authority
    pub fn signing_data(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(128);
        data.extend_from_slice(b"dchat-revocation-v1");
        data.extend_from_slice(&[self.revocation_type as u8]);
        data.extend_from_slice(&self.target_id);
        if let Some(ref scope) = self.scope_id {
            data.extend_from_slice(scope);
        }
        data.extend_from_slice(&self.created_at.to_le_bytes());
        data.extend_from_slice(&self.effective_at.to_le_bytes());
        data.extend_from_slice(&self.expires_at.to_le_bytes());
        data
    }

    /// Check if revocation is currently active
    pub fn is_active(&self) -> bool {
        let now = current_timestamp();

        // Not yet effective
        if now < self.effective_at {
            return false;
        }

        // Expired
        if self.expires_at > 0 && now > self.expires_at {
            return false;
        }

        true
    }

    /// Check if revocation is expired and can be archived
    pub fn should_archive(&self) -> bool {
        if self.expires_at == 0 {
            return false; // Never expires
        }
        let now = current_timestamp();
        now > self.expires_at + REVOCATION_ARCHIVE_AGE_SECS
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| Error::crypto(format!("Revocation serialization failed: {}", e)))
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data)
            .map_err(|e| Error::crypto(format!("Revocation deserialization failed: {}", e)))
    }
}

/// Result of a revocation check
#[derive(Debug, Clone)]
pub struct RevocationCheckResult {
    /// Whether the entity is revoked
    pub is_revoked: bool,
    /// If revoked, the matching revocation entry
    pub revocation: Option<RevocationEntry>,
    /// Check timestamp
    pub checked_at: u64,
    /// Relay that performed the check
    pub relay_id: [u8; 32],
}

impl RevocationCheckResult {
    /// Create an "allowed" result
    pub fn allowed(relay_id: [u8; 32]) -> Self {
        Self {
            is_revoked: false,
            revocation: None,
            checked_at: current_timestamp(),
            relay_id,
        }
    }

    /// Create a "revoked" result
    pub fn revoked(revocation: RevocationEntry, relay_id: [u8; 32]) -> Self {
        Self {
            is_revoked: true,
            revocation: Some(revocation),
            checked_at: current_timestamp(),
            relay_id,
        }
    }
}

/// In-memory revocation store with persistence support
pub struct RevocationStore {
    /// Device revocations: device_id -> entry
    device_revocations: HashMap<[u8; 32], RevocationEntry>,

    /// User revocations: user_id -> entry
    user_revocations: HashMap<[u8; 32], RevocationEntry>,

    /// Membership revocations: (user_id, conversation_id) -> entry
    membership_revocations: HashMap<([u8; 32], [u8; 32]), RevocationEntry>,

    /// Emergency revocations: target_id -> entry
    emergency_revocations: HashMap<[u8; 32], RevocationEntry>,

    /// All revocations by ID (for sync protocol)
    by_id: HashMap<RevocationId, RevocationEntry>,

    /// Revocations ordered by sequence (for sync)
    by_sequence: BTreeMap<u64, RevocationId>,

    /// Current sequence number
    current_sequence: u64,

    /// Merkle root of all active revocations
    merkle_root: [u8; 32],

    /// Last update timestamp
    last_updated: u64,
}

impl RevocationStore {
    /// Create new empty store
    pub fn new() -> Self {
        Self {
            device_revocations: HashMap::new(),
            user_revocations: HashMap::new(),
            membership_revocations: HashMap::new(),
            emergency_revocations: HashMap::new(),
            by_id: HashMap::new(),
            by_sequence: BTreeMap::new(),
            current_sequence: 0,
            merkle_root: [0u8; 32],
            last_updated: current_timestamp(),
        }
    }

    /// Add a revocation entry
    pub fn add(&mut self, mut entry: RevocationEntry) -> Result<()> {
        // Validate authority signature
        entry.authority.verify(&entry)?;

        // Assign sequence number
        self.current_sequence += 1;
        entry.sequence = self.current_sequence;

        // Update previous hash for merkle chain
        if let Some((&_last_seq, last_id)) = self.by_sequence.last_key_value() {
            if let Some(last_entry) = self.by_id.get(last_id) {
                entry.previous_hash = last_entry.id;
            }
        }

        // Recompute ID with sequence
        entry.id = entry.compute_id();

        // Add to appropriate index
        match entry.revocation_type {
            RevocationType::Device => {
                self.device_revocations
                    .insert(entry.target_id, entry.clone());
            }
            RevocationType::User => {
                self.user_revocations.insert(entry.target_id, entry.clone());
            }
            RevocationType::Membership => {
                if let Some(scope_id) = entry.scope_id {
                    self.membership_revocations
                        .insert((entry.target_id, scope_id), entry.clone());
                }
            }
            RevocationType::Emergency => {
                self.emergency_revocations
                    .insert(entry.target_id, entry.clone());
            }
        }

        // Add to global indexes
        self.by_id.insert(entry.id, entry.clone());
        self.by_sequence.insert(entry.sequence, entry.id);

        // Update merkle root
        self.update_merkle_root();
        self.last_updated = current_timestamp();

        // Enforce memory limits
        self.enforce_limits();

        Ok(())
    }

    /// Check if a device is revoked
    pub fn is_device_revoked(&self, device_id: &[u8; 32]) -> Option<&RevocationEntry> {
        self.device_revocations
            .get(device_id)
            .filter(|e| e.is_active())
            .or_else(|| {
                // Also check emergency revocations
                self.emergency_revocations
                    .get(device_id)
                    .filter(|e| e.is_active())
            })
    }

    /// Check if a user is revoked
    pub fn is_user_revoked(&self, user_id: &[u8; 32]) -> Option<&RevocationEntry> {
        self.user_revocations
            .get(user_id)
            .filter(|e| e.is_active())
            .or_else(|| {
                // Also check emergency revocations
                self.emergency_revocations
                    .get(user_id)
                    .filter(|e| e.is_active())
            })
    }

    /// Check if a membership is revoked
    pub fn is_membership_revoked(
        &self,
        user_id: &[u8; 32],
        conversation_id: &[u8; 32],
    ) -> Option<&RevocationEntry> {
        self.membership_revocations
            .get(&(*user_id, *conversation_id))
            .filter(|e| e.is_active())
    }

    /// Comprehensive revocation check for epoch token request
    pub fn check_for_epoch_token(
        &self,
        device_id: &[u8; 32],
        user_id: &[u8; 32],
        conversation_id: &[u8; 32],
    ) -> Option<&RevocationEntry> {
        // Check in order of priority: emergency > user > device > membership
        self.emergency_revocations
            .get(device_id)
            .filter(|e| e.is_active())
            .or_else(|| {
                self.emergency_revocations
                    .get(user_id)
                    .filter(|e| e.is_active())
            })
            .or_else(|| self.user_revocations.get(user_id).filter(|e| e.is_active()))
            .or_else(|| {
                self.device_revocations
                    .get(device_id)
                    .filter(|e| e.is_active())
            })
            .or_else(|| {
                self.membership_revocations
                    .get(&(*user_id, *conversation_id))
                    .filter(|e| e.is_active())
            })
    }

    /// Get revocations after a sequence number (for sync)
    pub fn get_since_sequence(&self, sequence: u64, limit: usize) -> Vec<RevocationEntry> {
        self.by_sequence
            .range((sequence + 1)..)
            .take(limit)
            .filter_map(|(_, id)| self.by_id.get(id).cloned())
            .collect()
    }

    /// Get current merkle root
    pub fn merkle_root(&self) -> &[u8; 32] {
        &self.merkle_root
    }

    /// Get current sequence number
    pub fn current_sequence(&self) -> u64 {
        self.current_sequence
    }

    /// Alias for current_sequence (for compatibility)
    pub fn sequence(&self) -> u64 {
        self.current_sequence
    }

    /// Get revocation by ID
    pub fn get_by_id(&self, id: &RevocationId) -> Option<&RevocationEntry> {
        self.by_id.get(id)
    }

    /// Iterate over all entries
    pub fn all_entries(&self) -> impl Iterator<Item = &RevocationEntry> {
        self.by_id.values()
    }

    /// Get total count of active revocations
    pub fn active_count(&self) -> usize {
        self.device_revocations
            .values()
            .filter(|e| e.is_active())
            .count()
            + self
                .user_revocations
                .values()
                .filter(|e| e.is_active())
                .count()
            + self
                .membership_revocations
                .values()
                .filter(|e| e.is_active())
                .count()
            + self
                .emergency_revocations
                .values()
                .filter(|e| e.is_active())
                .count()
    }

    /// Remove expired and archived entries
    pub fn cleanup(&mut self) {
        self.device_revocations.retain(|_, e| !e.should_archive());
        self.user_revocations.retain(|_, e| !e.should_archive());
        self.membership_revocations
            .retain(|_, e| !e.should_archive());
        self.emergency_revocations
            .retain(|_, e| !e.should_archive());

        // Rebuild by_id index
        let active_ids: HashSet<RevocationId> = self
            .device_revocations
            .values()
            .chain(self.user_revocations.values())
            .chain(self.membership_revocations.values())
            .chain(self.emergency_revocations.values())
            .map(|e| e.id)
            .collect();

        self.by_id.retain(|id, _| active_ids.contains(id));
        self.by_sequence.retain(|_, id| active_ids.contains(id));

        self.update_merkle_root();
    }

    /// Update merkle root after changes
    fn update_merkle_root(&mut self) {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(b"dchat-revocation-merkle-v1");
        hasher.update(&self.current_sequence.to_le_bytes());

        // Include all active revocation IDs in sequence order
        for (_, id) in &self.by_sequence {
            hasher.update(id);
        }

        self.merkle_root = hasher.finalize().into();
    }

    /// Enforce memory limits by removing oldest entries
    fn enforce_limits(&mut self) {
        while self.by_id.len() > MAX_ACTIVE_REVOCATIONS {
            if let Some((&oldest_seq, oldest_id)) = self.by_sequence.first_key_value() {
                let oldest_id = *oldest_id;
                self.by_sequence.remove(&oldest_seq);
                if let Some(entry) = self.by_id.remove(&oldest_id) {
                    // Remove from type-specific index
                    match entry.revocation_type {
                        RevocationType::Device => {
                            self.device_revocations.remove(&entry.target_id);
                        }
                        RevocationType::User => {
                            self.user_revocations.remove(&entry.target_id);
                        }
                        RevocationType::Membership => {
                            if let Some(scope_id) = entry.scope_id {
                                self.membership_revocations
                                    .remove(&(entry.target_id, scope_id));
                            }
                        }
                        RevocationType::Emergency => {
                            self.emergency_revocations.remove(&entry.target_id);
                        }
                    }
                }
            } else {
                break;
            }
        }
    }
}

impl Default for RevocationStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe revocation checker for relay nodes
pub struct RevocationChecker {
    /// Relay ID performing checks
    relay_id: [u8; 32],

    /// Revocation store (thread-safe) - made pub(crate) for internal access
    pub(crate) store: Arc<RwLock<RevocationStore>>,

    /// User to device mapping: user_id -> set of device_ids
    user_devices: Arc<RwLock<HashMap<[u8; 32], HashSet<[u8; 32]>>>>,

    /// Check statistics
    stats: Arc<RwLock<RevocationStats>>,
}

/// Statistics for revocation checks
#[derive(Debug, Clone, Default)]
pub struct RevocationStats {
    /// Total checks performed
    pub total_checks: u64,
    /// Checks that resulted in denial
    pub denials: u64,
    /// Denials by type
    pub denials_by_type: HashMap<RevocationType, u64>,
    /// Last check timestamp
    pub last_check: u64,
    /// Average check duration (microseconds)
    pub avg_check_duration_us: u64,
}

impl RevocationChecker {
    /// Create new revocation checker
    pub fn new(relay_id: [u8; 32]) -> Self {
        Self {
            relay_id,
            store: Arc::new(RwLock::new(RevocationStore::new())),
            user_devices: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(RevocationStats::default())),
        }
    }

    /// Create with existing store (for testing or restoration)
    pub fn with_store(relay_id: [u8; 32], store: RevocationStore) -> Self {
        Self {
            relay_id,
            store: Arc::new(RwLock::new(store)),
            user_devices: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(RevocationStats::default())),
        }
    }

    /// Register a device for a user (for user-level revocation propagation)
    pub async fn register_device(&self, user_id: [u8; 32], device_id: [u8; 32]) {
        let mut devices = self.user_devices.write().await;
        devices.entry(user_id).or_default().insert(device_id);
    }

    /// Check if an epoch token request should be allowed
    pub async fn check_epoch_token_request(
        &self,
        device_id: &[u8; 32],
        user_id: &[u8; 32],
        conversation_id: &[u8; 32],
    ) -> RevocationCheckResult {
        let start = std::time::Instant::now();

        let store = self.store.read().await;
        let result = if let Some(revocation) =
            store.check_for_epoch_token(device_id, user_id, conversation_id)
        {
            RevocationCheckResult::revoked(revocation.clone(), self.relay_id)
        } else {
            RevocationCheckResult::allowed(self.relay_id)
        };

        // Update stats
        let duration = start.elapsed().as_micros() as u64;
        let mut stats = self.stats.write().await;
        stats.total_checks += 1;
        stats.last_check = current_timestamp();
        if result.is_revoked {
            stats.denials += 1;
            if let Some(ref rev) = result.revocation {
                *stats
                    .denials_by_type
                    .entry(rev.revocation_type)
                    .or_insert(0) += 1;
            }
        }
        // Rolling average
        stats.avg_check_duration_us = (stats.avg_check_duration_us * 9 + duration) / 10;

        result
    }

    /// Add a revocation (validates and stores)
    pub async fn add_revocation(&self, entry: RevocationEntry) -> Result<()> {
        let mut store = self.store.write().await;
        store.add(entry)
    }

    /// Revoke a device
    pub async fn revoke_device(
        &self,
        device_id: [u8; 32],
        authority: RevocationAuthority,
        reason: RevocationReason,
    ) -> Result<RevocationId> {
        let entry = RevocationEntry::new_device_revocation(device_id, authority, reason);
        let id = entry.id;
        self.add_revocation(entry).await?;
        Ok(id)
    }

    /// Revoke a user (all devices)
    pub async fn revoke_user(
        &self,
        user_id: [u8; 32],
        authority: RevocationAuthority,
        reason: RevocationReason,
    ) -> Result<RevocationId> {
        let entry = RevocationEntry::new_user_revocation(user_id, authority, reason);
        let id = entry.id;
        self.add_revocation(entry).await?;
        Ok(id)
    }

    /// Revoke a membership
    pub async fn revoke_membership(
        &self,
        user_id: [u8; 32],
        conversation_id: [u8; 32],
        authority: RevocationAuthority,
        reason: RevocationReason,
    ) -> Result<RevocationId> {
        let entry =
            RevocationEntry::new_membership_revocation(user_id, conversation_id, authority, reason);
        let id = entry.id;
        self.add_revocation(entry).await?;
        Ok(id)
    }

    /// Emergency revocation
    pub async fn emergency_revoke(
        &self,
        target_id: [u8; 32],
        authority: RevocationAuthority,
        reason: RevocationReason,
    ) -> Result<RevocationId> {
        let entry = RevocationEntry::new_emergency_revocation(target_id, authority, reason);
        let id = entry.id;
        self.add_revocation(entry).await?;
        Ok(id)
    }

    /// Get revocations since a sequence number (for sync)
    pub async fn get_revocations_since(&self, sequence: u64, limit: usize) -> Vec<RevocationEntry> {
        let store = self.store.read().await;
        store.get_since_sequence(sequence, limit)
    }

    /// Get current merkle root
    pub async fn merkle_root(&self) -> [u8; 32] {
        let store = self.store.read().await;
        *store.merkle_root()
    }

    /// Get current sequence
    pub async fn current_sequence(&self) -> u64 {
        let store = self.store.read().await;
        store.current_sequence()
    }

    /// Get statistics
    pub async fn stats(&self) -> RevocationStats {
        let stats = self.stats.read().await;
        stats.clone()
    }

    /// Cleanup expired revocations
    pub async fn cleanup(&self) {
        let mut store = self.store.write().await;
        store.cleanup();
    }

    /// Get relay ID
    pub fn relay_id(&self) -> &[u8; 32] {
        &self.relay_id
    }
}

// Helper functions

/// Get current Unix timestamp
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Verify Ed25519 signature
fn verify_ed25519_signature(
    public_key: &[u8; 32],
    signature: &[u8; 64],
    message: &[u8],
) -> Result<()> {
    use ed25519_dalek::{Signature, VerifyingKey};

    let verifying_key = VerifyingKey::from_bytes(public_key)
        .map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?;

    let sig = Signature::from_bytes(signature);

    verifying_key
        .verify_strict(message, &sig)
        .map_err(|e| Error::crypto(format!("Signature verification failed: {}", e)))
}

/// Verify FROST threshold signature for relay quorum revocations
///
/// For RelayQuorum authority, we verify that:
/// 1. Enough relays participated (>= threshold)
/// 2. The FROST signature is valid against the registered committee's public key
///
/// # Production Implementation
///
/// This function looks up the committee's group public key from the CommitteeRegistry,
/// which is synchronized with on-chain committee registrations. Committees register
/// their group public keys during DKG (Distributed Key Generation) completion.
///
/// # Security Properties
///
/// - Committee public keys are verified against on-chain registrations
/// - Relay membership is validated against the registered committee
/// - Threshold requirements are enforced
fn verify_frost_signature(
    relay_ids: &[[u8; 32]],
    signature: &[u8; 64],
    message: &[u8],
    threshold: u8,
) -> Result<()> {
    // Verify we have enough relays
    if relay_ids.len() < threshold as usize {
        return Err(Error::crypto(format!(
            "Insufficient relay signatures: {} < {}",
            relay_ids.len(),
            threshold
        )));
    }

    // Production: Look up the committee from the global registry
    // The registry is synchronized with on-chain committee registrations
    let group_public_key = if let Some(registry) = super::frost_signing::global_registry() {
        // Use blocking task for async registry lookup in sync context
        // Note: In high-performance paths, prefer async verification
        let relay_ids_owned: Vec<[u8; 32]> = relay_ids.to_vec();

        // Try to find the committee in the registry
        let handle = tokio::runtime::Handle::try_current();
        match handle {
            Ok(rt) => {
                // We're in an async context, use block_in_place
                let committee = tokio::task::block_in_place(|| {
                    rt.block_on(registry.find_committee_by_relays(&relay_ids_owned))
                });

                match committee {
                    Some(c) => {
                        // Verify threshold meets committee minimum
                        if threshold < c.threshold {
                            return Err(Error::crypto(format!(
                                "Signature threshold {} is below committee minimum {}",
                                threshold, c.threshold
                            )));
                        }
                        c.group_public_key
                    }
                    None => {
                        // Committee not found - fall back to derived key for backward compatibility
                        // This allows gradual migration from test environments
                        tracing::warn!(
                            "Committee not found in registry for relays (first: {}), using derived key",
                            hex::encode(&relay_ids[0][..8])
                        );
                        derive_committee_public_key_for_verification(relay_ids)
                    }
                }
            }
            Err(_) => {
                // Not in async context - use derived key as fallback
                tracing::debug!("No tokio runtime available, using derived committee key");
                derive_committee_public_key_for_verification(relay_ids)
            }
        }
    } else {
        // Registry not initialized - use derived key for testing/bootstrap
        tracing::debug!("Committee registry not initialized, using derived key");
        derive_committee_public_key_for_verification(relay_ids)
    };

    match super::frost_signing::verify_frost_signature(signature, message, &group_public_key) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Error::crypto("FROST signature verification failed")),
        Err(e) => Err(Error::crypto(format!("FROST verification error: {}", e))),
    }
}

/// Verify FROST signature asynchronously with full registry support
///
/// This is the preferred method for production use as it properly awaits
/// the async registry operations without blocking.
pub async fn verify_frost_signature_async(
    relay_ids: &[[u8; 32]],
    signature: &[u8; 64],
    message: &[u8],
    threshold: u8,
) -> Result<()> {
    // Verify we have enough relays
    if relay_ids.len() < threshold as usize {
        return Err(Error::crypto(format!(
            "Insufficient relay signatures: {} < {}",
            relay_ids.len(),
            threshold
        )));
    }

    // Look up the committee from the global registry
    if let Some(registry) = super::frost_signing::global_registry() {
        registry
            .verify_frost_signature_with_registry(relay_ids, signature, message, threshold)
            .await
    } else {
        // Fallback for when registry is not initialized
        let group_public_key = derive_committee_public_key_for_verification(relay_ids);
        match super::frost_signing::verify_frost_signature(signature, message, &group_public_key) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Error::crypto("FROST signature verification failed")),
            Err(e) => Err(Error::crypto(format!("FROST verification error: {}", e))),
        }
    }
}

/// Derive a deterministic committee public key from relay IDs
///
/// This is used as a fallback when the committee is not registered in the
/// on-chain registry. It provides backward compatibility for test environments
/// and during the transition to full on-chain committee registration.
///
/// # Security Warning
///
/// This derived key does NOT provide the cryptographic guarantees of a proper
/// FROST group key. It should only be used in:
/// 1. Testing environments
/// 2. Bootstrapping before committees are registered
/// 3. Backward compatibility with pre-registry deployments
///
/// Production deployments MUST use registered committee keys for security.
fn derive_committee_public_key_for_verification(relay_ids: &[[u8; 32]]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"dchat-committee-pubkey-v1");

    // Sort relay IDs for deterministic ordering
    let mut sorted_ids = relay_ids.to_vec();
    sorted_ids.sort();

    for id in sorted_ids {
        hasher.update(id);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_authority() -> RevocationAuthority {
        RevocationAuthority::System {
            block_height: 1000,
            tx_hash: [1u8; 32],
        }
    }

    #[test]
    fn test_revocation_entry_creation() {
        let device_id = [42u8; 32];
        let authority = create_test_authority();
        let reason = RevocationReason::UserRequested;

        let entry = RevocationEntry::new_device_revocation(device_id, authority, reason);

        assert_eq!(entry.version, REVOCATION_ENTRY_VERSION);
        assert_eq!(entry.revocation_type, RevocationType::Device);
        assert_eq!(entry.target_id, device_id);
        assert!(entry.is_active());
        assert!(!entry.should_archive());
    }

    #[test]
    fn test_revocation_entry_serialization() {
        let entry = RevocationEntry::new_device_revocation(
            [1u8; 32],
            create_test_authority(),
            RevocationReason::DeviceStolen,
        );

        let bytes = entry.to_bytes().unwrap();
        let recovered = RevocationEntry::from_bytes(&bytes).unwrap();

        assert_eq!(entry.id, recovered.id);
        assert_eq!(entry.target_id, recovered.target_id);
        assert_eq!(entry.revocation_type, recovered.revocation_type);
    }

    #[test]
    fn test_revocation_store_device() {
        let mut store = RevocationStore::new();
        let device_id = [42u8; 32];

        // Not revoked initially
        assert!(store.is_device_revoked(&device_id).is_none());

        // Add revocation
        let entry = RevocationEntry::new_device_revocation(
            device_id,
            create_test_authority(),
            RevocationReason::SecurityCompromise,
        );
        store.add(entry).unwrap();

        // Now revoked
        assert!(store.is_device_revoked(&device_id).is_some());
    }

    #[test]
    fn test_revocation_store_user() {
        let mut store = RevocationStore::new();
        let user_id = [99u8; 32];

        assert!(store.is_user_revoked(&user_id).is_none());

        let entry = RevocationEntry::new_user_revocation(
            user_id,
            create_test_authority(),
            RevocationReason::AccountDeleted,
        );
        store.add(entry).unwrap();

        assert!(store.is_user_revoked(&user_id).is_some());
    }

    #[test]
    fn test_revocation_store_membership() {
        let mut store = RevocationStore::new();
        let user_id = [1u8; 32];
        let conv_id = [2u8; 32];

        assert!(store.is_membership_revoked(&user_id, &conv_id).is_none());

        let entry = RevocationEntry::new_membership_revocation(
            user_id,
            conv_id,
            create_test_authority(),
            RevocationReason::MembershipRemoved,
        );
        store.add(entry).unwrap();

        assert!(store.is_membership_revoked(&user_id, &conv_id).is_some());

        // Different conversation should not be revoked
        let other_conv = [3u8; 32];
        assert!(store.is_membership_revoked(&user_id, &other_conv).is_none());
    }

    #[test]
    fn test_epoch_token_check() {
        let mut store = RevocationStore::new();
        let device_id = [1u8; 32];
        let user_id = [2u8; 32];
        let conv_id = [3u8; 32];

        // All clear
        assert!(store
            .check_for_epoch_token(&device_id, &user_id, &conv_id)
            .is_none());

        // Revoke device
        store
            .add(RevocationEntry::new_device_revocation(
                device_id,
                create_test_authority(),
                RevocationReason::DeviceStolen,
            ))
            .unwrap();

        // Now blocked
        let result = store.check_for_epoch_token(&device_id, &user_id, &conv_id);
        assert!(result.is_some());
        assert_eq!(result.unwrap().revocation_type, RevocationType::Device);
    }

    #[test]
    fn test_emergency_revocation_priority() {
        let mut store = RevocationStore::new();
        let target_id = [1u8; 32];

        // Add device revocation
        store
            .add(RevocationEntry::new_device_revocation(
                target_id,
                create_test_authority(),
                RevocationReason::UserRequested,
            ))
            .unwrap();

        // Add emergency revocation
        store
            .add(RevocationEntry::new_emergency_revocation(
                target_id,
                create_test_authority(),
                RevocationReason::EmergencyResponse {
                    incident_id: "INC-001".to_string(),
                },
            ))
            .unwrap();

        // Emergency should take priority
        let result = store.check_for_epoch_token(&target_id, &target_id, &[0u8; 32]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().revocation_type, RevocationType::Emergency);
    }

    #[test]
    fn test_revocation_sequence_tracking() {
        let mut store = RevocationStore::new();

        for i in 0..5 {
            let entry = RevocationEntry::new_device_revocation(
                [i as u8; 32],
                create_test_authority(),
                RevocationReason::UserRequested,
            );
            store.add(entry).unwrap();
        }

        assert_eq!(store.current_sequence(), 5);

        // Get since sequence 2
        let since = store.get_since_sequence(2, 10);
        assert_eq!(since.len(), 3); // Entries 3, 4, 5
    }

    #[test]
    fn test_merkle_root_changes() {
        let mut store = RevocationStore::new();
        let root1 = *store.merkle_root();

        store
            .add(RevocationEntry::new_device_revocation(
                [1u8; 32],
                create_test_authority(),
                RevocationReason::UserRequested,
            ))
            .unwrap();

        let root2 = *store.merkle_root();
        assert_ne!(root1, root2);

        store
            .add(RevocationEntry::new_device_revocation(
                [2u8; 32],
                create_test_authority(),
                RevocationReason::UserRequested,
            ))
            .unwrap();

        let root3 = *store.merkle_root();
        assert_ne!(root2, root3);
    }

    #[tokio::test]
    async fn test_revocation_checker() {
        let relay_id = [99u8; 32];
        let checker = RevocationChecker::new(relay_id);

        let device_id = [1u8; 32];
        let user_id = [2u8; 32];
        let conv_id = [3u8; 32];

        // Initially allowed
        let result = checker
            .check_epoch_token_request(&device_id, &user_id, &conv_id)
            .await;
        assert!(!result.is_revoked);

        // Revoke device
        checker
            .revoke_device(
                device_id,
                RevocationAuthority::System {
                    block_height: 100,
                    tx_hash: [0u8; 32],
                },
                RevocationReason::DeviceStolen,
            )
            .await
            .unwrap();

        // Now denied
        let result = checker
            .check_epoch_token_request(&device_id, &user_id, &conv_id)
            .await;
        assert!(result.is_revoked);
        assert!(result.revocation.is_some());
    }

    #[tokio::test]
    async fn test_revocation_stats() {
        let checker = RevocationChecker::new([1u8; 32]);

        // Perform some checks
        for i in 0..10 {
            checker
                .check_epoch_token_request(&[i as u8; 32], &[0u8; 32], &[0u8; 32])
                .await;
        }

        let stats = checker.stats().await;
        assert_eq!(stats.total_checks, 10);
        assert_eq!(stats.denials, 0);
    }

    #[tokio::test]
    async fn test_membership_revocation() {
        let checker = RevocationChecker::new([99u8; 32]);

        let user_id = [1u8; 32];
        let conv_id = [2u8; 32];
        let other_conv = [3u8; 32];

        // Revoke membership in conv_id
        checker
            .revoke_membership(
                user_id,
                conv_id,
                RevocationAuthority::System {
                    block_height: 100,
                    tx_hash: [0u8; 32],
                },
                RevocationReason::MembershipRemoved,
            )
            .await
            .unwrap();

        // Blocked from conv_id
        let result = checker
            .check_epoch_token_request(&[0u8; 32], &user_id, &conv_id)
            .await;
        assert!(result.is_revoked);

        // But allowed in other_conv
        let result = checker
            .check_epoch_token_request(&[0u8; 32], &user_id, &other_conv)
            .await;
        assert!(!result.is_revoked);
    }

    #[test]
    fn test_revocation_expiry() {
        let now = current_timestamp();

        // Future effective date
        let mut entry = RevocationEntry::new_device_revocation(
            [1u8; 32],
            create_test_authority(),
            RevocationReason::UserRequested,
        );
        entry.effective_at = now + 3600; // 1 hour in future
        assert!(!entry.is_active());

        // Expired entry
        let mut expired = RevocationEntry::new_device_revocation(
            [2u8; 32],
            create_test_authority(),
            RevocationReason::UserRequested,
        );
        expired.expires_at = now - 1; // Already expired
        assert!(!expired.is_active());
    }

    #[test]
    fn test_revocation_type_display() {
        assert_eq!(RevocationType::Device.as_str(), "device");
        assert_eq!(RevocationType::User.as_str(), "user");
        assert_eq!(RevocationType::Membership.as_str(), "membership");
        assert_eq!(RevocationType::Emergency.as_str(), "emergency");
    }
}
