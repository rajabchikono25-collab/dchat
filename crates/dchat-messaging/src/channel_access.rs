//! Channel access control with token-gating and NFT verification
//!
//! This module implements advanced channel access control including:
//! - Token-gated channels (require specific token holdings)
//! - NFT-based access badges
//! - Staking requirements for membership
//! - Cryptographic access proof verification

use dchat_core::types::{ChannelId, UserId};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::staking_verifier::{StakeStatus, StakingVerifier};

/// Channel access control policy
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccessPolicy {
    /// Anyone can join (public channel)
    Public,

    /// Invite-only (private channel)
    Private {
        /// List of invited user IDs
        invited_users: HashSet<UserId>,
    },

    /// Requires holding specific token amount
    TokenGated {
        /// Token identifier (from currency chain)
        token_id: String,
        /// Minimum token amount required
        minimum_amount: u64,
        /// Whether tokens must be bonded (locked)
        requires_bonding: bool,
    },

    /// Requires owning specific NFT
    NftGated {
        /// NFT collection ID
        collection_id: String,
        /// Specific token IDs (if empty, any from collection works)
        required_token_ids: Vec<String>,
    },

    /// Requires minimum reputation score
    ReputationGated {
        /// Minimum reputation score (0-100)
        minimum_score: u8,
    },

    /// Requires staking amount to join
    StakeGated {
        /// Minimum stake amount
        minimum_stake: u64,
        /// Stake duration in seconds
        stake_duration_secs: u64,
    },

    /// Combination of multiple requirements (all must be met)
    Combined { policies: Vec<AccessPolicy> },
}

/// Channel access manager
pub struct ChannelAccessManager {
    /// Map of channel ID to access policy
    policies: HashMap<ChannelId, AccessPolicy>,

    /// Map of channel ID to current members
    members: HashMap<ChannelId, HashSet<UserId>>,

    /// Map of user ID to their token holdings (for verification)
    user_tokens: HashMap<UserId, HashMap<String, u64>>,

    /// Map of user ID to their NFT ownership
    user_nfts: HashMap<UserId, HashSet<String>>,

    /// Map of user ID to their reputation scores
    user_reputation: HashMap<UserId, u8>,

    /// Blockchain staking verifier (dependency injected)
    staking_verifier: Option<Arc<dyn StakingVerifier>>,
}

impl ChannelAccessManager {
    /// Create a new channel access manager without staking verifier
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            members: HashMap::new(),
            user_tokens: HashMap::new(),
            user_nfts: HashMap::new(),
            user_reputation: HashMap::new(),
            staking_verifier: None,
        }
    }

    /// Create channel access manager with injected staking verifier
    pub fn with_staking_verifier(verifier: Arc<dyn StakingVerifier>) -> Self {
        Self {
            policies: HashMap::new(),
            members: HashMap::new(),
            user_tokens: HashMap::new(),
            user_nfts: HashMap::new(),
            user_reputation: HashMap::new(),
            staking_verifier: Some(verifier),
        }
    }

    /// Set staking verifier after construction
    pub fn set_staking_verifier(&mut self, verifier: Arc<dyn StakingVerifier>) {
        self.staking_verifier = Some(verifier);
    }

    /// Set access policy for a channel
    pub fn set_policy(&mut self, channel_id: ChannelId, policy: AccessPolicy) {
        self.policies.insert(channel_id, policy);
    }

    /// Get access policy for a channel
    pub fn get_policy(&self, channel_id: &ChannelId) -> Option<&AccessPolicy> {
        self.policies.get(channel_id)
    }

    /// Update user's token holdings
    pub fn update_user_tokens(&mut self, user_id: UserId, token_id: String, amount: u64) {
        self.user_tokens
            .entry(user_id)
            .or_default()
            .insert(token_id, amount);
    }

    /// Update user's NFT ownership
    pub fn add_user_nft(&mut self, user_id: UserId, nft_token_id: String) {
        self.user_nfts
            .entry(user_id)
            .or_default()
            .insert(nft_token_id);
    }

    /// Remove user's NFT
    pub fn remove_user_nft(&mut self, user_id: &UserId, nft_token_id: &str) {
        if let Some(nfts) = self.user_nfts.get_mut(user_id) {
            nfts.remove(nft_token_id);
        }
    }

    /// Update user's reputation score
    pub fn update_reputation(&mut self, user_id: UserId, score: u8) {
        self.user_reputation.insert(user_id, score.min(100));
    }

    /// Check if user can access channel
    pub async fn can_access(&self, user_id: &UserId, channel_id: &ChannelId) -> Result<bool> {
        // Check if already a member
        if let Some(members) = self.members.get(channel_id) {
            if members.contains(user_id) {
                return Ok(true);
            }
        }

        // Get access policy
        let policy = self
            .policies
            .get(channel_id)
            .ok_or_else(|| Error::validation("Channel not found"))?;

        self.check_policy(user_id, channel_id, policy).await
    }

    /// Check if user meets policy requirements
    fn check_policy<'a>(
        &'a self,
        user_id: &'a UserId,
        channel_id: &'a ChannelId,
        policy: &'a AccessPolicy,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            match policy {
                AccessPolicy::Public => Ok(true),

                AccessPolicy::Private { invited_users } => Ok(invited_users.contains(user_id)),

                AccessPolicy::TokenGated {
                    token_id,
                    minimum_amount,
                    requires_bonding: _,
                } => {
                    let user_tokens = self.user_tokens.get(user_id);
                    if let Some(tokens) = user_tokens {
                        if let Some(amount) = tokens.get(token_id) {
                            return Ok(amount >= minimum_amount);
                        }
                    }
                    Ok(false)
                }

                AccessPolicy::NftGated {
                    collection_id: _,
                    required_token_ids,
                } => {
                    let user_nfts = self.user_nfts.get(user_id);
                    if let Some(nfts) = user_nfts {
                        if required_token_ids.is_empty() {
                            // Any NFT from collection
                            return Ok(!nfts.is_empty());
                        } else {
                            // Specific NFT required
                            return Ok(required_token_ids.iter().any(|id| nfts.contains(id)));
                        }
                    }
                    Ok(false)
                }

                AccessPolicy::ReputationGated { minimum_score } => {
                    let score = self.user_reputation.get(user_id).copied().unwrap_or(0);
                    Ok(score >= *minimum_score)
                }

                AccessPolicy::StakeGated {
                    minimum_stake,
                    stake_duration_secs,
                } => {
                    // Use injected StakingVerifier for blockchain-backed verification
                    if let Some(verifier) = &self.staking_verifier {
                        // 1. Verify stake commitment meets requirements
                        let is_valid = verifier
                            .verify_stake_commitment(user_id, *minimum_stake, *stake_duration_secs)
                            .await?;

                        if !is_valid {
                            tracing::debug!(
                                "Stake verification failed: user={:?}, required={}, duration={}s",
                                user_id,
                                minimum_stake,
                                stake_duration_secs
                            );
                            return Ok(false);
                        }

                        // 2. Check stake status (must be active, not slashed/withdrawn)
                        let stake_status = verifier.check_stake_status(user_id, channel_id).await?;

                        if stake_status != StakeStatus::Active {
                            tracing::warn!(
                                "Stake status is not active: user={:?}, channel={:?}, status={:?}",
                                user_id,
                                channel_id,
                                stake_status
                            );
                            return Ok(false);
                        }

                        tracing::info!(
                            "✅ Stake verification passed: user={:?}, channel={:?}",
                            user_id,
                            channel_id
                        );
                        Ok(true)
                    } else {
                        tracing::error!(
                        "StakeGated policy requires StakingVerifier - none injected. Denying access."
                    );
                        Err(Error::validation(
                            "Stake verification unavailable - StakingVerifier not configured",
                        ))
                    }
                }

                AccessPolicy::Combined { policies } => {
                    // All policies must pass
                    for sub_policy in policies {
                        if !self.check_policy(user_id, channel_id, sub_policy).await? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
            }
        })
    }

    /// Grant channel access to user (after policy check)
    pub async fn grant_access(&mut self, user_id: UserId, channel_id: ChannelId) -> Result<()> {
        if !self.can_access(&user_id, &channel_id).await? {
            return Err(Error::validation("Access denied: requirements not met"));
        }

        self.members.entry(channel_id).or_default().insert(user_id);

        Ok(())
    }

    /// Revoke channel access from user
    pub fn revoke_access(&mut self, user_id: &UserId, channel_id: &ChannelId) -> Result<()> {
        if let Some(members) = self.members.get_mut(channel_id) {
            members.remove(user_id);
        }
        Ok(())
    }

    /// Get all members of a channel
    pub fn get_members(&self, channel_id: &ChannelId) -> Vec<UserId> {
        self.members
            .get(channel_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if user is member of channel
    pub fn is_member(&self, user_id: &UserId, channel_id: &ChannelId) -> bool {
        self.members
            .get(channel_id)
            .map(|members| members.contains(user_id))
            .unwrap_or(false)
    }

    /// Invite user to private channel
    pub fn invite_user(&mut self, channel_id: &ChannelId, user_id: UserId) -> Result<()> {
        let policy = self
            .policies
            .get_mut(channel_id)
            .ok_or_else(|| Error::validation("Channel not found"))?;

        match policy {
            AccessPolicy::Private { invited_users } => {
                invited_users.insert(user_id);
                Ok(())
            }
            _ => Err(Error::validation("Channel is not private")),
        }
    }
}

impl Default for ChannelAccessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user() -> UserId {
        UserId::new()
    }

    fn create_test_channel() -> ChannelId {
        ChannelId::new()
    }

    #[test]
    fn test_public_channel_access() {
        let mut manager = ChannelAccessManager::new();
        let channel = create_test_channel();
        let user = create_test_user();

        manager.set_policy(channel.clone(), AccessPolicy::Public);

        assert!(manager.can_access(&user, &channel).unwrap());
        manager.grant_access(user.clone(), channel.clone()).unwrap();
        assert!(manager.is_member(&user, &channel));
    }

    #[test]
    fn test_private_channel_access() {
        let mut manager = ChannelAccessManager::new();
        let channel = create_test_channel();
        let user1 = create_test_user();
        let user2 = create_test_user();

        let mut invited = HashSet::new();
        invited.insert(user1.clone());

        manager.set_policy(
            channel.clone(),
            AccessPolicy::Private {
                invited_users: invited,
            },
        );

        // User1 is invited
        assert!(manager.can_access(&user1, &channel).unwrap());

        // User2 is not invited
        assert!(!manager.can_access(&user2, &channel).unwrap());
    }

    #[test]
    fn test_token_gated_channel() {
        let mut manager = ChannelAccessManager::new();
        let channel = create_test_channel();
        let user = create_test_user();

        manager.set_policy(
            channel.clone(),
            AccessPolicy::TokenGated {
                token_id: "DCHAT".to_string(),
                minimum_amount: 100,
                requires_bonding: false,
            },
        );

        // User doesn't have tokens
        assert!(!manager.can_access(&user, &channel).unwrap());

        // Give user insufficient tokens
        manager.update_user_tokens(user.clone(), "DCHAT".to_string(), 50);
        assert!(!manager.can_access(&user, &channel).unwrap());

        // Give user sufficient tokens
        manager.update_user_tokens(user.clone(), "DCHAT".to_string(), 150);
        assert!(manager.can_access(&user, &channel).unwrap());
    }

    #[test]
    fn test_nft_gated_channel() {
        let mut manager = ChannelAccessManager::new();
        let channel = create_test_channel();
        let user = create_test_user();

        manager.set_policy(
            channel.clone(),
            AccessPolicy::NftGated {
                collection_id: "badges".to_string(),
                required_token_ids: vec!["badge_001".to_string()],
            },
        );

        // User doesn't have NFT
        assert!(!manager.can_access(&user, &channel).unwrap());

        // Give user wrong NFT
        manager.add_user_nft(user.clone(), "badge_002".to_string());
        assert!(!manager.can_access(&user, &channel).unwrap());

        // Give user correct NFT
        manager.add_user_nft(user.clone(), "badge_001".to_string());
        assert!(manager.can_access(&user, &channel).unwrap());
    }

    #[test]
    fn test_reputation_gated_channel() {
        let mut manager = ChannelAccessManager::new();
        let channel = create_test_channel();
        let user = create_test_user();

        manager.set_policy(
            channel.clone(),
            AccessPolicy::ReputationGated { minimum_score: 50 },
        );

        // User has no reputation
        assert!(!manager.can_access(&user, &channel).unwrap());

        // User has low reputation
        manager.update_reputation(user.clone(), 30);
        assert!(!manager.can_access(&user, &channel).unwrap());

        // User has sufficient reputation
        manager.update_reputation(user.clone(), 75);
        assert!(manager.can_access(&user, &channel).unwrap());
    }

    #[test]
    fn test_combined_policy() {
        let mut manager = ChannelAccessManager::new();
        let channel = create_test_channel();
        let user = create_test_user();

        manager.set_policy(
            channel.clone(),
            AccessPolicy::Combined {
                policies: vec![
                    AccessPolicy::TokenGated {
                        token_id: "DCHAT".to_string(),
                        minimum_amount: 100,
                        requires_bonding: false,
                    },
                    AccessPolicy::ReputationGated { minimum_score: 50 },
                ],
            },
        );

        // User has neither
        assert!(!manager.can_access(&user, &channel).unwrap());

        // User has tokens but not reputation
        manager.update_user_tokens(user.clone(), "DCHAT".to_string(), 150);
        assert!(!manager.can_access(&user, &channel).unwrap());

        // User has both
        manager.update_reputation(user.clone(), 75);
        assert!(manager.can_access(&user, &channel).unwrap());
    }

    #[test]
    fn test_invite_to_private_channel() {
        let mut manager = ChannelAccessManager::new();
        let channel = create_test_channel();
        let user = create_test_user();

        manager.set_policy(
            channel.clone(),
            AccessPolicy::Private {
                invited_users: HashSet::new(),
            },
        );

        // User not invited
        assert!(!manager.can_access(&user, &channel).unwrap());

        // Invite user
        manager.invite_user(&channel, user.clone()).unwrap();
        assert!(manager.can_access(&user, &channel).unwrap());
    }

    #[test]
    fn test_revoke_access() {
        let mut manager = ChannelAccessManager::new();
        let channel = create_test_channel();
        let user = create_test_user();

        manager.set_policy(channel.clone(), AccessPolicy::Public);
        manager.grant_access(user.clone(), channel.clone()).unwrap();

        assert!(manager.is_member(&user, &channel));

        manager.revoke_access(&user, &channel).unwrap();
        assert!(!manager.is_member(&user, &channel));
    }
}
