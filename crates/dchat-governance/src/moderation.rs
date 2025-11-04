// Decentralized Moderation with Community Voting
//
// This module implements community-driven moderation where:
// - Moderators are selected via staking
// - Slashing votes punish abuse of power
// - Transparency logs track all actions
// - Appeal mechanisms protect users

use chrono::{DateTime, Utc};
use dchat_core::{Error, MessageId, Result, UserId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Type of moderation action
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModerationActionType {
    /// Warn user
    Warning,
    /// Temporarily mute user
    Mute,
    /// Ban user from channel
    Ban,
    /// Delete message
    DeleteMessage,
    /// Slash moderator for abuse
    SlashModerator,
}

/// A moderation action taken or proposed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationAction {
    /// Unique action ID
    pub id: Uuid,
    /// Type of action
    pub action_type: ModerationActionType,
    /// Moderator proposing action
    pub moderator: UserId,
    /// Target user (if applicable)
    pub target_user: Option<UserId>,
    /// Target message (if applicable)
    pub target_message: Option<MessageId>,
    /// Reason for action
    pub reason: String,
    /// Timestamp
    pub created_at: DateTime<Utc>,
    /// Has been executed?
    pub executed: bool,
    /// Appeal status
    pub appeal: Option<Appeal>,
}

/// An appeal against a moderation action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appeal {
    /// Appealing user
    pub appellant: UserId,
    /// Appeal reasoning
    pub reason: String,
    /// Timestamp
    pub filed_at: DateTime<Utc>,
    /// Appeal status
    pub status: AppealStatus,
}

/// Status of an appeal
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppealStatus {
    /// Under review
    Pending,
    /// Appeal accepted (action reversed)
    Accepted,
    /// Appeal rejected (action stands)
    Rejected,
}

/// A vote to slash a moderator for abuse of power
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingVote {
    /// Vote ID
    pub id: Uuid,
    /// Moderator being voted on
    pub moderator: UserId,
    /// Evidence of abuse
    pub evidence: Vec<ModerationAction>,
    /// Votes for slashing
    pub votes_for: u64,
    /// Votes against slashing
    pub votes_against: u64,
    /// Vote deadline
    pub deadline: DateTime<Utc>,
    /// Is finalized?
    pub finalized: bool,
}

/// Manager for moderation actions and slashing
pub struct ModerationManager {
    /// Active moderators and their stake
    moderators: HashMap<UserId, u64>,
    /// Moderation actions log
    actions: HashMap<Uuid, ModerationAction>,
    /// Slashing votes
    slashing_votes: HashMap<Uuid, SlashingVote>,
    /// Minimum stake required to be moderator
    min_stake: u64,
}

impl ModerationAction {
    /// Create a new moderation action
    pub fn new(
        moderator: UserId,
        action_type: ModerationActionType,
        target_user: Option<UserId>,
        target_message: Option<MessageId>,
        reason: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            action_type,
            moderator,
            target_user,
            target_message,
            reason,
            created_at: Utc::now(),
            executed: false,
            appeal: None,
        }
    }

    /// File an appeal against this action
    pub fn file_appeal(&mut self, appellant: UserId, reason: String) -> Result<()> {
        if self.appeal.is_some() {
            return Err(Error::validation("Appeal already filed".to_string()));
        }

        self.appeal = Some(Appeal {
            appellant,
            reason,
            filed_at: Utc::now(),
            status: AppealStatus::Pending,
        });

        Ok(())
    }

    /// Resolve an appeal
    pub fn resolve_appeal(&mut self, accepted: bool) -> Result<()> {
        let appeal = self
            .appeal
            .as_mut()
            .ok_or_else(|| Error::validation("No appeal filed".to_string()))?;

        if appeal.status != AppealStatus::Pending {
            return Err(Error::validation("Appeal already resolved".to_string()));
        }

        appeal.status = if accepted {
            AppealStatus::Accepted
        } else {
            AppealStatus::Rejected
        };

        Ok(())
    }
}

impl SlashingVote {
    /// Create a new slashing vote
    pub fn new(
        moderator: UserId,
        evidence: Vec<ModerationAction>,
        voting_period_days: i64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            moderator,
            evidence,
            votes_for: 0,
            votes_against: 0,
            deadline: Utc::now() + chrono::Duration::days(voting_period_days),
            finalized: false,
        }
    }

    /// Check if voting is still open
    pub fn is_open(&self) -> bool {
        !self.finalized && Utc::now() < self.deadline
    }

    /// Check if slash passes (simple majority)
    pub fn passes(&self) -> bool {
        self.votes_for > self.votes_against
    }
}

impl ModerationManager {
    /// Create a new moderation manager
    pub fn new(min_stake: u64) -> Self {
        Self {
            moderators: HashMap::new(),
            actions: HashMap::new(),
            slashing_votes: HashMap::new(),
            min_stake,
        }
    }

    /// Register a moderator with stake
    pub fn register_moderator(&mut self, moderator: UserId, stake: u64) -> Result<()> {
        if stake < self.min_stake {
            return Err(Error::validation(format!(
                "Insufficient stake (need {})",
                self.min_stake
            )));
        }

        self.moderators.insert(moderator, stake);
        Ok(())
    }

    /// Check if user is a moderator
    pub fn is_moderator(&self, user: &UserId) -> bool {
        self.moderators.contains_key(user)
    }

    /// Submit a moderation action
    pub fn submit_action(&mut self, action: ModerationAction) -> Result<Uuid> {
        if !self.is_moderator(&action.moderator) {
            return Err(Error::validation("Not a moderator".to_string()));
        }

        let id = action.id;
        self.actions.insert(id, action);
        Ok(id)
    }

    /// Execute a moderation action
    pub fn execute_action(&mut self, action_id: &Uuid) -> Result<()> {
        let action = self
            .actions
            .get_mut(action_id)
            .ok_or_else(|| Error::NotFound("Action not found".to_string()))?;

        if action.executed {
            return Err(Error::validation("Action already executed".to_string()));
        }

        // Check if there's a pending appeal
        if let Some(appeal) = &action.appeal {
            if appeal.status == AppealStatus::Pending {
                return Err(Error::validation(
                    "Cannot execute while appeal pending".to_string(),
                ));
            }
            if appeal.status == AppealStatus::Accepted {
                return Err(Error::validation("Action overturned by appeal".to_string()));
            }
        }

        action.executed = true;
        Ok(())
    }

    /// Initiate a slashing vote against a moderator
    pub fn initiate_slashing(
        &mut self,
        moderator: UserId,
        evidence: Vec<ModerationAction>,
        voting_period_days: i64,
    ) -> Result<Uuid> {
        if !self.is_moderator(&moderator) {
            return Err(Error::validation("Target is not a moderator".to_string()));
        }

        if evidence.is_empty() {
            return Err(Error::validation("Must provide evidence".to_string()));
        }

        let vote = SlashingVote::new(moderator, evidence, voting_period_days);
        let id = vote.id;
        self.slashing_votes.insert(id, vote);
        Ok(id)
    }

    /// Cast vote on slashing proposal
    pub fn vote_on_slashing(
        &mut self,
        vote_id: &Uuid,
        vote_for: bool,
        voting_power: u64,
    ) -> Result<()> {
        let vote = self
            .slashing_votes
            .get_mut(vote_id)
            .ok_or_else(|| Error::NotFound("Slashing vote not found".to_string()))?;

        if !vote.is_open() {
            return Err(Error::validation("Voting closed".to_string()));
        }

        if vote_for {
            vote.votes_for += voting_power;
        } else {
            vote.votes_against += voting_power;
        }

        Ok(())
    }

    /// Finalize a slashing vote
    pub fn finalize_slashing(&mut self, vote_id: &Uuid) -> Result<bool> {
        let vote = self
            .slashing_votes
            .get_mut(vote_id)
            .ok_or_else(|| Error::NotFound("Slashing vote not found".to_string()))?;

        if vote.finalized {
            return Err(Error::validation("Already finalized".to_string()));
        }

        if Utc::now() < vote.deadline {
            return Err(Error::validation("Voting period not ended".to_string()));
        }

        vote.finalized = true;

        // If slash passes, remove moderator
        if vote.passes() {
            self.moderators.remove(&vote.moderator);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get all moderation actions (transparency log)
    pub fn get_all_actions(&self) -> Vec<&ModerationAction> {
        self.actions.values().collect()
    }

    /// Get actions by moderator
    pub fn get_actions_by_moderator(&self, moderator: &UserId) -> Vec<&ModerationAction> {
        self.actions
            .values()
            .filter(|a| &a.moderator == moderator)
            .collect()
    }

    /// Get active slashing votes
    pub fn get_active_slashing_votes(&self) -> Vec<&SlashingVote> {
        self.slashing_votes
            .values()
            .filter(|v| v.is_open())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moderation_action_creation() {
        let moderator = UserId::new();
        let target = UserId::new();

        let action = ModerationAction::new(
            moderator,
            ModerationActionType::Warning,
            Some(target),
            None,
            "Spam warning".to_string(),
        );

        assert_eq!(action.action_type, ModerationActionType::Warning);
        assert!(!action.executed);
        assert!(action.appeal.is_none());
    }

    #[test]
    fn test_appeal_filing() {
        let moderator = UserId::new();
        let target = UserId::new();

        let mut action = ModerationAction::new(
            moderator,
            ModerationActionType::Ban,
            Some(target.clone()),
            None,
            "Harassment".to_string(),
        );

        action
            .file_appeal(target, "Wrongful ban".to_string())
            .unwrap();
        assert!(action.appeal.is_some());
        assert_eq!(
            action.appeal.as_ref().unwrap().status,
            AppealStatus::Pending
        );
    }

    #[test]
    fn test_moderator_registration() {
        let mut manager = ModerationManager::new(1000);
        let moderator = UserId::new();

        manager.register_moderator(moderator.clone(), 1500).unwrap();
        assert!(manager.is_moderator(&moderator));
    }

    #[test]
    fn test_insufficient_stake() {
        let mut manager = ModerationManager::new(1000);
        let moderator = UserId::new();

        let result = manager.register_moderator(moderator, 500);
        assert!(result.is_err());
    }

    #[test]
    fn test_moderation_action_submission() {
        let mut manager = ModerationManager::new(1000);
        let moderator = UserId::new();
        let target = UserId::new();

        manager.register_moderator(moderator.clone(), 1500).unwrap();

        let action = ModerationAction::new(
            moderator,
            ModerationActionType::Mute,
            Some(target),
            None,
            "Spam".to_string(),
        );

        let action_id = manager.submit_action(action).unwrap();
        assert!(manager.actions.contains_key(&action_id));
    }

    #[test]
    fn test_non_moderator_cannot_submit() {
        let mut manager = ModerationManager::new(1000);
        let non_moderator = UserId::new();
        let target = UserId::new();

        let action = ModerationAction::new(
            non_moderator,
            ModerationActionType::Ban,
            Some(target),
            None,
            "Test".to_string(),
        );

        let result = manager.submit_action(action);
        assert!(result.is_err());
    }

    #[test]
    fn test_slashing_vote_creation() {
        let moderator = UserId::new();
        let action = ModerationAction::new(
            moderator.clone(),
            ModerationActionType::Ban,
            Some(UserId::new()),
            None,
            "Abuse".to_string(),
        );

        let vote = SlashingVote::new(moderator, vec![action], 7);
        assert!(vote.is_open());
        assert!(!vote.finalized);
    }

    #[test]
    fn test_slashing_vote_flow() {
        let mut manager = ModerationManager::new(1000);
        let moderator = UserId::new();

        manager.register_moderator(moderator.clone(), 2000).unwrap();

        let action = ModerationAction::new(
            moderator.clone(),
            ModerationActionType::Ban,
            Some(UserId::new()),
            None,
            "Abuse".to_string(),
        );

        let vote_id = manager
            .initiate_slashing(moderator, vec![action], 7)
            .unwrap();

        // Vote against slash
        manager.vote_on_slashing(&vote_id, false, 100).unwrap();

        // Vote for slash
        manager.vote_on_slashing(&vote_id, true, 200).unwrap();

        let vote = manager.slashing_votes.get(&vote_id).unwrap();
        assert_eq!(vote.votes_for, 200);
        assert_eq!(vote.votes_against, 100);
    }

    #[test]
    fn test_transparency_log() {
        let mut manager = ModerationManager::new(1000);
        let moderator = UserId::new();

        manager.register_moderator(moderator.clone(), 1500).unwrap();

        for i in 0..3 {
            let action = ModerationAction::new(
                moderator.clone(),
                ModerationActionType::Warning,
                Some(UserId::new()),
                None,
                format!("Warning {}", i),
            );
            manager.submit_action(action).unwrap();
        }

        let actions = manager.get_actions_by_moderator(&moderator);
        assert_eq!(actions.len(), 3);
    }
}
