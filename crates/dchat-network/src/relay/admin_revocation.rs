//! Admin Revocation for QGE Channels
//!
//! This module implements channel administration capabilities for revoking
//! member access. Admins can ban users, enforce timeouts, and manage
//! channel membership with immediate cryptographic effect.
//!
//! # Revocation Types
//!
//! - **Immediate Ban**: User loses access instantly, keys are rotated
//! - **Timeout**: Temporary access suspension with automatic restoration
//! - **Soft Revoke**: User can still read but cannot send
//! - **Shadow Ban**: User thinks they're participating but messages aren't delivered
//!
//! # Cryptographic Effects
//!
//! When a user is revoked:
//! 1. Their epoch tokens are invalidated
//! 2. Their sender keys are removed from group state
//! 3. New epoch keys are distributed excluding them
//! 4. Historical access is optionally revoked (with SUK rotation)
//!
//! # Admin Hierarchy
//!
//! - **Owner**: Full control, can revoke admins
//! - **Admin**: Can revoke regular members
//! - **Moderator**: Can timeout but not permanently ban
//! - **Member**: No revocation powers

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Maximum timeout duration (30 days)
pub const MAX_TIMEOUT_SECS: u64 = 30 * 24 * 60 * 60;

/// Minimum timeout duration (1 minute)
pub const MIN_TIMEOUT_SECS: u64 = 60;

/// Maximum revocation history entries per channel
pub const MAX_REVOCATION_HISTORY: usize = 10_000;

/// Grace period before revocation takes full effect (seconds)
pub const REVOCATION_GRACE_PERIOD_SECS: u64 = 30;

/// Admin role levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AdminRole {
    /// Regular member - no admin powers
    Member = 0,
    /// Moderator - can timeout, mute
    Moderator = 1,
    /// Administrator - can ban, unban, manage moderators
    Admin = 2,
    /// Owner - full control, can transfer ownership
    Owner = 3,
}

impl AdminRole {
    pub fn can_revoke(&self, target_role: AdminRole, action: &RevocationAction) -> bool {
        match action {
            RevocationAction::Timeout { .. } => {
                *self >= AdminRole::Moderator && *self > target_role
            }
            RevocationAction::Mute { .. } => *self >= AdminRole::Moderator && *self > target_role,
            RevocationAction::SoftRevoke { .. } => *self >= AdminRole::Admin && *self > target_role,
            RevocationAction::Ban { .. } => *self >= AdminRole::Admin && *self > target_role,
            RevocationAction::ShadowBan { .. } => *self >= AdminRole::Admin && *self > target_role,
        }
    }

    pub fn can_grant_role(&self, role: AdminRole) -> bool {
        *self > role
    }
}

/// Revocation action type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RevocationAction {
    /// Temporary timeout
    Timeout { duration_secs: u64, reason: String },
    /// Cannot send messages but can read
    Mute {
        duration_secs: Option<u64>, // None = permanent
        reason: String,
    },
    /// Can read but messages are filtered/hidden
    SoftRevoke { reason: String },
    /// Full ban - loses all access
    Ban {
        reason: String,
        revoke_history: bool,
    },
    /// User thinks they're posting but messages aren't delivered
    ShadowBan { reason: String },
}

impl RevocationAction {
    pub fn action_name(&self) -> &'static str {
        match self {
            RevocationAction::Timeout { .. } => "timeout",
            RevocationAction::Mute { .. } => "mute",
            RevocationAction::SoftRevoke { .. } => "soft_revoke",
            RevocationAction::Ban { .. } => "ban",
            RevocationAction::ShadowBan { .. } => "shadow_ban",
        }
    }

    pub fn is_permanent(&self) -> bool {
        matches!(
            self,
            RevocationAction::Ban { .. }
                | RevocationAction::ShadowBan { .. }
                | RevocationAction::Mute {
                    duration_secs: None,
                    ..
                }
        )
    }

    pub fn duration(&self) -> Option<Duration> {
        match self {
            RevocationAction::Timeout { duration_secs, .. } => {
                Some(Duration::from_secs(*duration_secs))
            }
            RevocationAction::Mute {
                duration_secs: Some(secs),
                ..
            } => Some(Duration::from_secs(*secs)),
            _ => None,
        }
    }
}

/// Revocation request from admin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationRequest {
    /// Channel ID
    pub channel_id: [u8; 32],
    /// Admin making the request
    pub admin_id: [u8; 32],
    /// Target user to revoke
    pub target_id: [u8; 32],
    /// Action to take
    pub action: RevocationAction,
    /// Timestamp
    pub timestamp: u64,
    /// Request nonce (for dedup)
    pub nonce: [u8; 16],
    /// Admin signature
    pub signature: Vec<u8>,
}

impl RevocationRequest {
    pub fn new(
        channel_id: [u8; 32],
        admin_id: [u8; 32],
        target_id: [u8; 32],
        action: RevocationAction,
    ) -> Self {
        use rand::RngCore;
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);

        Self {
            channel_id,
            admin_id,
            target_id,
            action,
            timestamp: current_timestamp(),
            nonce,
            signature: Vec::new(),
        }
    }

    /// Get the data to be signed
    fn signing_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.channel_id);
        data.extend_from_slice(&self.admin_id);
        data.extend_from_slice(&self.target_id);
        data.extend_from_slice(self.action.action_name().as_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&self.nonce);
        data
    }

    /// Sign the request with Ed25519 signing key
    pub fn sign(&mut self, signing_key: &ed25519_dalek::SigningKey) {
        use ed25519_dalek::Signer;
        let data = self.signing_data();
        let signature = signing_key.sign(&data);
        self.signature = signature.to_bytes().to_vec();
    }

    /// Verify the Ed25519 signature using the admin's verifying key
    pub fn verify(&self, verifying_key: &ed25519_dalek::VerifyingKey) -> Result<bool> {
        if self.signature.len() != 64 {
            return Ok(false);
        }
        let sig_bytes: [u8; 64] = match self.signature.as_slice().try_into() {
            Ok(b) => b,
            Err(_) => return Ok(false),
        };
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        let data = self.signing_data();
        use ed25519_dalek::Verifier;
        Ok(verifying_key.verify(&data, &signature).is_ok())
    }

    /// Legacy sign method using raw key bytes (for backwards compatibility)
    /// Derives an Ed25519 signing key from the provided bytes using HKDF.
    pub fn sign_with_bytes(&mut self, key_bytes: &[u8]) {
        // Derive Ed25519 seed from key bytes
        use hkdf::Hkdf;
        use sha2::Sha256;

        let hk = Hkdf::<Sha256>::new(None, key_bytes);
        let mut seed = [0u8; 32];
        hk.expand(b"dchat-admin-revocation-signing", &mut seed)
            .expect("valid length");

        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        self.sign(&signing_key);
    }
}

/// Revocation record for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationRecord {
    /// Unique ID
    pub id: [u8; 32],
    /// The original request
    pub request: RevocationRequest,
    /// When revocation was applied
    pub applied_at: u64,
    /// When revocation expires (None = permanent)
    pub expires_at: Option<u64>,
    /// Current status
    pub status: RevocationStatus,
    /// Appeal status if any
    pub appeal: Option<AppealStatus>,
}

/// Current status of a revocation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevocationStatus {
    /// Pending (in grace period)
    Pending,
    /// Actively enforced
    Active,
    /// Expired naturally
    Expired,
    /// Manually lifted by admin
    Lifted,
    /// Overturned by appeal
    Appealed,
}

/// Appeal status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppealStatus {
    /// Appeal submitted at
    pub submitted_at: u64,
    /// Appeal reason
    pub reason: String,
    /// Reviewed by (admin ID)
    pub reviewed_by: Option<[u8; 32]>,
    /// Appeal decision
    pub decision: Option<AppealDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppealDecision {
    /// Appeal approved, revocation lifted
    Approved,
    /// Appeal denied
    Denied,
    /// Appeal dismissed (spam, duplicate)
    Dismissed,
}

/// Member state in a channel
#[derive(Debug, Clone)]
pub struct MemberState {
    /// User ID
    pub user_id: [u8; 32],
    /// Current role
    pub role: AdminRole,
    /// Active revocations
    pub revocations: Vec<RevocationRecord>,
    /// Joined timestamp
    pub joined_at: u64,
    /// Last activity
    pub last_active: u64,
}

impl MemberState {
    fn new(user_id: [u8; 32], role: AdminRole) -> Self {
        let now = current_timestamp();
        Self {
            user_id,
            role,
            revocations: Vec::new(),
            joined_at: now,
            last_active: now,
        }
    }

    /// Check if user can send messages
    pub fn can_send(&self) -> bool {
        for revocation in &self.revocations {
            if revocation.status != RevocationStatus::Active {
                continue;
            }

            match &revocation.request.action {
                RevocationAction::Timeout { .. }
                | RevocationAction::Mute { .. }
                | RevocationAction::Ban { .. } => return false,
                _ => {}
            }
        }
        true
    }

    /// Check if user can read messages
    pub fn can_read(&self) -> bool {
        for revocation in &self.revocations {
            if revocation.status != RevocationStatus::Active {
                continue;
            }

            if matches!(revocation.request.action, RevocationAction::Ban { .. }) {
                return false;
            }
        }
        true
    }

    /// Check if user is shadow banned
    pub fn is_shadow_banned(&self) -> bool {
        self.revocations.iter().any(|r| {
            r.status == RevocationStatus::Active
                && matches!(r.request.action, RevocationAction::ShadowBan { .. })
        })
    }

    /// Get active revocation type
    pub fn active_revocation(&self) -> Option<&RevocationAction> {
        self.revocations
            .iter()
            .find(|r| r.status == RevocationStatus::Active)
            .map(|r| &r.request.action)
    }
}

/// Channel state for revocation management
struct ChannelRevocationState {
    /// Channel ID
    channel_id: [u8; 32],
    /// Member states
    members: HashMap<[u8; 32], MemberState>,
    /// Revocation history
    history: VecDeque<RevocationRecord>,
    /// Processed request nonces (for dedup)
    processed_nonces: HashSet<[u8; 16]>,
    /// Key rotation required flag
    key_rotation_required: bool,
}

impl ChannelRevocationState {
    fn new(channel_id: [u8; 32]) -> Self {
        Self {
            channel_id,
            members: HashMap::new(),
            history: VecDeque::new(),
            processed_nonces: HashSet::new(),
            key_rotation_required: false,
        }
    }
}

/// Admin revocation manager
pub struct AdminRevocationManager {
    /// Our user ID
    our_id: [u8; 32],
    /// Per-channel state
    channels: Arc<RwLock<HashMap<[u8; 32], ChannelRevocationState>>>,
    /// Callback for key rotation
    key_rotation_callback: Arc<RwLock<Option<Box<dyn Fn([u8; 32], [u8; 32]) + Send + Sync>>>>,
    /// Callback for revocation events
    event_callback: Arc<RwLock<Option<Box<dyn Fn(RevocationEvent) + Send + Sync>>>>,
}

/// Revocation event for notifications
#[derive(Debug, Clone)]
pub enum RevocationEvent {
    /// Member was revoked
    MemberRevoked {
        channel_id: [u8; 32],
        target_id: [u8; 32],
        action: RevocationAction,
        admin_id: [u8; 32],
    },
    /// Revocation was lifted
    RevocationLifted {
        channel_id: [u8; 32],
        target_id: [u8; 32],
        lifted_by: [u8; 32],
    },
    /// Revocation expired
    RevocationExpired {
        channel_id: [u8; 32],
        target_id: [u8; 32],
    },
    /// Key rotation triggered
    KeyRotationRequired {
        channel_id: [u8; 32],
        reason: String,
    },
    /// Appeal submitted
    AppealSubmitted {
        channel_id: [u8; 32],
        target_id: [u8; 32],
        revocation_id: [u8; 32],
    },
}

impl AdminRevocationManager {
    /// Create a new revocation manager
    pub fn new(our_id: [u8; 32]) -> Self {
        Self {
            our_id,
            channels: Arc::new(RwLock::new(HashMap::new())),
            key_rotation_callback: Arc::new(RwLock::new(None)),
            event_callback: Arc::new(RwLock::new(None)),
        }
    }

    /// Set key rotation callback
    pub async fn set_key_rotation_callback<F>(&self, callback: F)
    where
        F: Fn([u8; 32], [u8; 32]) + Send + Sync + 'static,
    {
        let mut cb = self.key_rotation_callback.write().await;
        *cb = Some(Box::new(callback));
    }

    /// Set event callback
    pub async fn set_event_callback<F>(&self, callback: F)
    where
        F: Fn(RevocationEvent) + Send + Sync + 'static,
    {
        let mut cb = self.event_callback.write().await;
        *cb = Some(Box::new(callback));
    }

    /// Initialize channel with owner
    pub async fn init_channel(&self, channel_id: [u8; 32], owner_id: [u8; 32]) {
        let mut channels = self.channels.write().await;
        let mut state = ChannelRevocationState::new(channel_id);
        state
            .members
            .insert(owner_id, MemberState::new(owner_id, AdminRole::Owner));
        channels.insert(channel_id, state);
    }

    /// Add member to channel
    pub async fn add_member(
        &self,
        channel_id: [u8; 32],
        member_id: [u8; 32],
        role: AdminRole,
    ) -> Result<()> {
        let mut channels = self.channels.write().await;
        let state = channels
            .get_mut(&channel_id)
            .ok_or_else(|| Error::network("Channel not found"))?;

        if state.members.contains_key(&member_id) {
            return Err(Error::network("Member already exists"));
        }

        state
            .members
            .insert(member_id, MemberState::new(member_id, role));
        Ok(())
    }

    /// Process a revocation request with signature verification
    ///
    /// The `admin_verifying_key` should be the Ed25519 public key of the admin
    /// making the request, obtained through a trusted channel.
    pub async fn process_revocation(
        &self,
        request: RevocationRequest,
        admin_verifying_key: Option<&ed25519_dalek::VerifyingKey>,
    ) -> Result<RevocationRecord> {
        // Verify request signature if key provided
        if let Some(vk) = admin_verifying_key {
            if !request.verify(vk)? {
                return Err(Error::network("Invalid request signature"));
            }
        } else if !request.signature.is_empty() {
            // Signature exists but no key provided - cannot verify
            return Err(Error::network(
                "Signature present but no verifying key provided",
            ));
        }

        let mut channels = self.channels.write().await;
        let state = channels
            .get_mut(&request.channel_id)
            .ok_or_else(|| Error::network("Channel not found"))?;

        // Check for duplicate
        if state.processed_nonces.contains(&request.nonce) {
            return Err(Error::network("Duplicate revocation request"));
        }

        // Get admin and target states
        let admin_role = state
            .members
            .get(&request.admin_id)
            .map(|m| m.role)
            .ok_or_else(|| Error::network("Admin not found"))?;

        let target_role = state
            .members
            .get(&request.target_id)
            .map(|m| m.role)
            .ok_or_else(|| Error::network("Target not found"))?;

        // Check permissions
        if !admin_role.can_revoke(target_role, &request.action) {
            return Err(Error::network(
                "Insufficient permissions for this revocation",
            ));
        }

        // Validate timeout duration
        if let RevocationAction::Timeout { duration_secs, .. } = &request.action {
            if *duration_secs < MIN_TIMEOUT_SECS || *duration_secs > MAX_TIMEOUT_SECS {
                return Err(Error::network("Invalid timeout duration"));
            }
        }

        // Create revocation record
        let now = current_timestamp();
        let expires_at = request.action.duration().map(|d| now + d.as_secs());

        let mut record_id = [0u8; 32];
        let id_data = blake3::hash(
            &[
                &request.channel_id[..],
                &request.target_id[..],
                &request.nonce[..],
            ]
            .concat(),
        );
        record_id.copy_from_slice(id_data.as_bytes());

        let record = RevocationRecord {
            id: record_id,
            request: request.clone(),
            applied_at: now,
            expires_at,
            status: RevocationStatus::Active,
            appeal: None,
        };

        // Apply to member state
        if let Some(member) = state.members.get_mut(&request.target_id) {
            // Clear any conflicting revocations
            member
                .revocations
                .retain(|r| r.status != RevocationStatus::Active);
            member.revocations.push(record.clone());
        }

        // Track nonce
        state.processed_nonces.insert(request.nonce);

        // Add to history
        state.history.push_back(record.clone());
        while state.history.len() > MAX_REVOCATION_HISTORY {
            state.history.pop_front();
        }

        // Flag for key rotation if this is a ban with history revocation
        if matches!(
            request.action,
            RevocationAction::Ban {
                revoke_history: true,
                ..
            }
        ) {
            state.key_rotation_required = true;
        }

        // Emit event
        drop(channels);
        self.emit_event(RevocationEvent::MemberRevoked {
            channel_id: request.channel_id,
            target_id: request.target_id,
            action: request.action.clone(),
            admin_id: request.admin_id,
        })
        .await;

        // Trigger key rotation if needed
        if matches!(
            request.action,
            RevocationAction::Ban {
                revoke_history: true,
                ..
            }
        ) {
            self.emit_event(RevocationEvent::KeyRotationRequired {
                channel_id: request.channel_id,
                reason: "Member banned with history revocation".to_string(),
            })
            .await;
        }

        Ok(record)
    }

    /// Lift a revocation
    pub async fn lift_revocation(
        &self,
        channel_id: [u8; 32],
        target_id: [u8; 32],
        lifted_by: [u8; 32],
    ) -> Result<()> {
        let mut channels = self.channels.write().await;
        let state = channels
            .get_mut(&channel_id)
            .ok_or_else(|| Error::network("Channel not found"))?;

        // Check permissions (need higher role than original admin)
        let lifter_role = state
            .members
            .get(&lifted_by)
            .map(|m| m.role)
            .ok_or_else(|| Error::network("Lifter not found"))?;

        if let Some(member) = state.members.get_mut(&target_id) {
            for revocation in &mut member.revocations {
                if revocation.status == RevocationStatus::Active {
                    let original_admin_role = state
                        .members
                        .get(&revocation.request.admin_id)
                        .map(|m| m.role)
                        .unwrap_or(AdminRole::Member);

                    if lifter_role <= original_admin_role && lifter_role != AdminRole::Owner {
                        return Err(Error::network(
                            "Cannot lift revocation by higher/equal admin",
                        ));
                    }

                    revocation.status = RevocationStatus::Lifted;
                }
            }
        }

        drop(channels);
        self.emit_event(RevocationEvent::RevocationLifted {
            channel_id,
            target_id,
            lifted_by,
        })
        .await;

        Ok(())
    }

    /// Submit an appeal
    pub async fn submit_appeal(
        &self,
        channel_id: [u8; 32],
        target_id: [u8; 32],
        reason: String,
    ) -> Result<()> {
        let mut channels = self.channels.write().await;
        let state = channels
            .get_mut(&channel_id)
            .ok_or_else(|| Error::network("Channel not found"))?;

        let mut revocation_id = None;

        if let Some(member) = state.members.get_mut(&target_id) {
            for revocation in &mut member.revocations {
                if revocation.status == RevocationStatus::Active && revocation.appeal.is_none() {
                    revocation.appeal = Some(AppealStatus {
                        submitted_at: current_timestamp(),
                        reason: reason.clone(),
                        reviewed_by: None,
                        decision: None,
                    });
                    revocation_id = Some(revocation.id);
                    break;
                }
            }
        }

        let rev_id =
            revocation_id.ok_or_else(|| Error::network("No active revocation to appeal"))?;

        drop(channels);
        self.emit_event(RevocationEvent::AppealSubmitted {
            channel_id,
            target_id,
            revocation_id: rev_id,
        })
        .await;

        Ok(())
    }

    /// Review an appeal
    pub async fn review_appeal(
        &self,
        channel_id: [u8; 32],
        target_id: [u8; 32],
        reviewer_id: [u8; 32],
        decision: AppealDecision,
    ) -> Result<()> {
        let mut channels = self.channels.write().await;
        let state = channels
            .get_mut(&channel_id)
            .ok_or_else(|| Error::network("Channel not found"))?;

        // Check reviewer permissions
        let reviewer_role = state
            .members
            .get(&reviewer_id)
            .map(|m| m.role)
            .ok_or_else(|| Error::network("Reviewer not found"))?;

        if reviewer_role < AdminRole::Admin {
            return Err(Error::network("Only admins can review appeals"));
        }

        if let Some(member) = state.members.get_mut(&target_id) {
            for revocation in &mut member.revocations {
                if let Some(appeal) = &mut revocation.appeal {
                    if appeal.decision.is_none() {
                        appeal.reviewed_by = Some(reviewer_id);
                        appeal.decision = Some(decision);

                        if decision == AppealDecision::Approved {
                            revocation.status = RevocationStatus::Appealed;
                        }
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Check for expired revocations
    pub async fn process_expirations(&self) {
        let now = current_timestamp();
        let mut expired = Vec::new();

        let mut channels = self.channels.write().await;

        for (channel_id, state) in channels.iter_mut() {
            for (user_id, member) in state.members.iter_mut() {
                for revocation in &mut member.revocations {
                    if revocation.status == RevocationStatus::Active {
                        if let Some(expires_at) = revocation.expires_at {
                            if now >= expires_at {
                                revocation.status = RevocationStatus::Expired;
                                expired.push((*channel_id, *user_id));
                            }
                        }
                    }
                }
            }
        }

        drop(channels);

        for (channel_id, target_id) in expired {
            self.emit_event(RevocationEvent::RevocationExpired {
                channel_id,
                target_id,
            })
            .await;
        }
    }

    /// Get member's current status
    pub async fn get_member_status(
        &self,
        channel_id: [u8; 32],
        member_id: [u8; 32],
    ) -> Option<MemberState> {
        let channels = self.channels.read().await;
        channels.get(&channel_id)?.members.get(&member_id).cloned()
    }

    /// Check if member can send
    pub async fn can_member_send(&self, channel_id: [u8; 32], member_id: [u8; 32]) -> bool {
        self.get_member_status(channel_id, member_id)
            .await
            .map(|m| m.can_send())
            .unwrap_or(false)
    }

    /// Check if member can read
    pub async fn can_member_read(&self, channel_id: [u8; 32], member_id: [u8; 32]) -> bool {
        self.get_member_status(channel_id, member_id)
            .await
            .map(|m| m.can_read())
            .unwrap_or(false)
    }

    /// Check if member is shadow banned
    pub async fn is_shadow_banned(&self, channel_id: [u8; 32], member_id: [u8; 32]) -> bool {
        self.get_member_status(channel_id, member_id)
            .await
            .map(|m| m.is_shadow_banned())
            .unwrap_or(false)
    }

    /// Get revocation history for channel
    pub async fn get_revocation_history(
        &self,
        channel_id: [u8; 32],
        limit: usize,
    ) -> Vec<RevocationRecord> {
        let channels = self.channels.read().await;
        channels
            .get(&channel_id)
            .map(|s| s.history.iter().rev().take(limit).cloned().collect())
            .unwrap_or_default()
    }

    /// Start background expiration task
    pub fn start_expiration_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                self.process_expirations().await;
            }
        })
    }

    /// Emit event
    async fn emit_event(&self, event: RevocationEvent) {
        if let Some(callback) = self.event_callback.read().await.as_ref() {
            callback(event);
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_admin_revocation_creation() {
        let manager = AdminRevocationManager::new([1u8; 32]);
        let channel = [0xAB; 32];
        let owner = [0xCD; 32];

        manager.init_channel(channel, owner).await;

        let status = manager.get_member_status(channel, owner).await;
        assert!(status.is_some());
        assert_eq!(status.unwrap().role, AdminRole::Owner);
    }

    #[tokio::test]
    async fn test_add_member() {
        let manager = AdminRevocationManager::new([1u8; 32]);
        let channel = [0xAB; 32];
        let owner = [0xCD; 32];
        let member = [0xEF; 32];

        manager.init_channel(channel, owner).await;
        manager
            .add_member(channel, member, AdminRole::Member)
            .await
            .unwrap();

        let status = manager.get_member_status(channel, member).await.unwrap();
        assert_eq!(status.role, AdminRole::Member);
        assert!(status.can_send());
        assert!(status.can_read());
    }

    #[tokio::test]
    async fn test_revocation_request() {
        let manager = AdminRevocationManager::new([1u8; 32]);
        let channel = [0xAB; 32];
        let owner = [0xCD; 32];
        let member = [0xEF; 32];

        manager.init_channel(channel, owner).await;
        manager
            .add_member(channel, member, AdminRole::Member)
            .await
            .unwrap();

        // Generate a signing key for the owner
        let owner_signing_key = ed25519_dalek::SigningKey::from_bytes(&[0xCD; 32]);
        let owner_verifying_key = owner_signing_key.verifying_key();

        let mut request = RevocationRequest::new(
            channel,
            owner,
            member,
            RevocationAction::Timeout {
                duration_secs: 3600,
                reason: "Test timeout".to_string(),
            },
        );
        request.sign(&owner_signing_key);

        let record = manager
            .process_revocation(request, Some(&owner_verifying_key))
            .await
            .unwrap();
        assert_eq!(record.status, RevocationStatus::Active);

        let status = manager.get_member_status(channel, member).await.unwrap();
        assert!(!status.can_send());
        assert!(status.can_read());
    }

    #[tokio::test]
    async fn test_permission_check() {
        let admin_role = AdminRole::Admin;
        let mod_role = AdminRole::Moderator;
        let member_role = AdminRole::Member;

        // Admin can ban member
        let ban = RevocationAction::Ban {
            reason: "Test".to_string(),
            revoke_history: false,
        };
        assert!(admin_role.can_revoke(member_role, &ban));
        assert!(!mod_role.can_revoke(member_role, &ban));

        // Moderator can timeout member
        let timeout = RevocationAction::Timeout {
            duration_secs: 3600,
            reason: "Test".to_string(),
        };
        assert!(mod_role.can_revoke(member_role, &timeout));
        assert!(!member_role.can_revoke(member_role, &timeout));
    }

    #[tokio::test]
    async fn test_lift_revocation() {
        let manager = AdminRevocationManager::new([1u8; 32]);
        let channel = [0xAB; 32];
        let owner = [0xCD; 32];
        let admin = [0x11; 32];
        let member = [0xEF; 32];

        manager.init_channel(channel, owner).await;
        manager
            .add_member(channel, admin, AdminRole::Admin)
            .await
            .unwrap();
        manager
            .add_member(channel, member, AdminRole::Member)
            .await
            .unwrap();

        // Admin bans member
        let admin_signing_key = ed25519_dalek::SigningKey::from_bytes(&[0x11; 32]);
        let admin_verifying_key = admin_signing_key.verifying_key();

        let mut request = RevocationRequest::new(
            channel,
            admin,
            member,
            RevocationAction::Ban {
                reason: "Test".to_string(),
                revoke_history: false,
            },
        );
        request.sign(&admin_signing_key);
        manager
            .process_revocation(request, Some(&admin_verifying_key))
            .await
            .unwrap();

        // Owner lifts the ban
        manager
            .lift_revocation(channel, member, owner)
            .await
            .unwrap();

        let status = manager.get_member_status(channel, member).await.unwrap();
        assert!(status.can_send());
    }
}
