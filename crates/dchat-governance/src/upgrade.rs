// Protocol Upgrade Governance
//
// This module implements decentralized protocol upgrade voting,
// hard fork coordination, and backward compatibility management.

use chrono::{DateTime, Duration, Utc};
use dchat_core::{Error, Result, UserId, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Semantic version representation
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(Error::validation(
                "Invalid version format. Expected: major.minor.patch",
            ));
        }

        let major = parts[0]
            .parse()
            .map_err(|_| Error::validation("Invalid major version"))?;
        let minor = parts[1]
            .parse()
            .map_err(|_| Error::validation("Invalid minor version"))?;
        let patch = parts[2]
            .parse()
            .map_err(|_| Error::validation("Invalid patch version"))?;

        Ok(Self {
            major,
            minor,
            patch,
        })
    }

    /// Check if this is a breaking change (major version bump)
    pub fn is_breaking_change(&self, other: &Version) -> bool {
        self.major > other.major
    }

    /// Check if versions are compatible (same major version)
    pub fn is_compatible(&self, other: &Version) -> bool {
        self.major == other.major
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Type of upgrade
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeType {
    /// Minor update (backward compatible)
    SoftFork,
    /// Major update (breaking changes, requires hard fork)
    HardFork,
    /// Emergency security patch
    SecurityPatch,
    /// Feature flag toggle (no code change)
    FeatureToggle { feature: String },
}

/// Upgrade proposal status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeStatus {
    /// Proposal submitted, voting in progress
    Proposed,
    /// Voting passed, awaiting activation
    Approved,
    /// Scheduled for specific block height
    Scheduled { activation_height: u64 },
    /// Upgrade is now active
    Active,
    /// Proposal rejected by vote
    Rejected,
    /// Cancelled by emergency governance action
    Cancelled,
}

/// Network upgrade proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeProposal {
    pub id: Uuid,
    pub proposer: UserId,
    pub upgrade_type: UpgradeType,
    pub current_version: Version,
    pub target_version: Version,

    /// Human-readable title
    pub title: String,
    /// Detailed description and rationale
    pub description: String,
    /// Technical specification URL (e.g., GitHub PR)
    pub spec_url: Option<String>,

    /// When voting ends
    pub voting_deadline: DateTime<Utc>,
    /// When upgrade activates (if approved)
    pub activation_time: Option<DateTime<Utc>>,
    /// Block height at which to activate
    pub activation_height: Option<u64>,

    /// Current status
    pub status: UpgradeStatus,

    /// Vote tally
    pub votes_for: u64,
    pub votes_against: u64,
    pub quorum_percentage: u32,

    /// Created timestamp
    pub created_at: DateTime<Utc>,

    /// Validator signatures (for hard forks)
    pub validator_signatures: Vec<ValidatorSignature>,
}

/// Validator signature for upgrade approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator_id: UserId,
    pub stake_amount: u64,
    pub signature: Vec<u8>,
    pub signed_at: DateTime<Utc>,
}

/// Fork state tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkState {
    /// Fork identifier
    pub fork_id: String,
    /// Parent version before fork
    pub parent_version: Version,
    /// New version after fork
    pub fork_version: Version,
    /// When fork occurred
    pub fork_height: u64,
    pub fork_time: DateTime<Utc>,

    /// Nodes that followed this fork
    pub supporting_nodes: HashSet<UserId>,
    /// Cumulative stake on this fork
    pub total_stake: u64,

    /// Is this the canonical chain?
    pub is_canonical: bool,
}

/// Upgrade governance manager
pub struct UpgradeManager {
    /// Active upgrade proposals
    proposals: HashMap<Uuid, UpgradeProposal>,

    /// Current protocol version
    current_version: Version,

    /// Fork history
    fork_history: Vec<ForkState>,

    /// Total network stake
    total_stake: u64,

    /// Minimum validator approval for hard forks (percentage)
    hard_fork_threshold: u32,

    /// Minimum voting period for upgrades (days)
    min_voting_period_days: i64,
}

impl UpgradeProposal {
    /// Create a new upgrade proposal
    pub fn new(
        proposer: UserId,
        upgrade_type: UpgradeType,
        current_version: Version,
        target_version: Version,
        title: String,
        description: String,
        voting_period_days: i64,
        quorum_percentage: u32,
    ) -> Result<Self> {
        if quorum_percentage > 100 {
            return Err(Error::validation("Quorum cannot exceed 100%"));
        }

        // Validate version progression
        if target_version <= current_version {
            return Err(Error::validation(
                "Target version must be greater than current version",
            ));
        }

        // Hard forks require major version bump
        if matches!(upgrade_type, UpgradeType::HardFork)
            && !target_version.is_breaking_change(&current_version)
        {
            return Err(Error::validation("Hard forks require major version bump"));
        }

        let now = Utc::now();
        let deadline = now + Duration::days(voting_period_days);

        Ok(Self {
            id: Uuid::new_v4(),
            proposer,
            upgrade_type,
            current_version,
            target_version,
            title,
            description,
            spec_url: None,
            voting_deadline: deadline,
            activation_time: None,
            activation_height: None,
            status: UpgradeStatus::Proposed,
            votes_for: 0,
            votes_against: 0,
            quorum_percentage,
            created_at: now,
            validator_signatures: Vec::new(),
        })
    }

    /// Check if voting is still open
    pub fn is_voting_open(&self) -> bool {
        matches!(self.status, UpgradeStatus::Proposed) && Utc::now() < self.voting_deadline
    }

    /// Check if proposal passes
    pub fn passes(&self, total_stake: u64) -> bool {
        let total_votes = self.votes_for + self.votes_against;
        let required_votes = (total_stake * self.quorum_percentage as u64) / 100;

        total_votes >= required_votes && self.votes_for > self.votes_against
    }

    /// Add validator signature (for hard forks)
    pub fn add_validator_signature(&mut self, signature: ValidatorSignature) -> Result<()> {
        // Check for duplicate
        if self
            .validator_signatures
            .iter()
            .any(|s| s.validator_id == signature.validator_id)
        {
            return Err(Error::validation("Validator already signed"));
        }

        self.validator_signatures.push(signature);
        Ok(())
    }

    /// Calculate validator approval percentage
    pub fn validator_approval_percentage(&self, total_stake: u64) -> u32 {
        let signed_stake: u64 = self
            .validator_signatures
            .iter()
            .map(|s| s.stake_amount)
            .sum();
        ((signed_stake * 100) / total_stake) as u32
    }
}

impl UpgradeManager {
    /// Create a new upgrade manager
    pub fn new() -> Self {
        let current = Version::parse(PROTOCOL_VERSION).unwrap_or_else(|_| Version::new(0, 1, 0));

        Self {
            proposals: HashMap::new(),
            current_version: current,
            fork_history: Vec::new(),
            total_stake: 0,
            hard_fork_threshold: 67, // 67% validator approval for hard forks
            min_voting_period_days: 14, // Minimum 2 weeks for major upgrades
        }
    }

    /// Submit a new upgrade proposal
    pub fn submit_proposal(&mut self, mut proposal: UpgradeProposal) -> Result<Uuid> {
        // Validate voting period for hard forks
        if matches!(proposal.upgrade_type, UpgradeType::HardFork) {
            let voting_days = (proposal.voting_deadline - proposal.created_at).num_days();
            if voting_days < self.min_voting_period_days {
                return Err(Error::validation(format!(
                    "Hard forks require minimum {} day voting period",
                    self.min_voting_period_days
                )));
            }
        }

        // Ensure current version matches
        if proposal.current_version != self.current_version {
            proposal.current_version = self.current_version.clone();
        }

        let id = proposal.id;
        self.proposals.insert(id, proposal);
        Ok(id)
    }

    /// Cast vote on upgrade proposal
    pub fn cast_upgrade_vote(
        &mut self,
        proposal_id: Uuid,
        _voter: UserId,
        vote_for: bool,
        voting_power: u64,
    ) -> Result<()> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if !proposal.is_voting_open() {
            return Err(Error::validation("Voting is closed"));
        }

        if vote_for {
            proposal.votes_for += voting_power;
        } else {
            proposal.votes_against += voting_power;
        }

        Ok(())
    }

    /// Finalize upgrade proposal voting
    pub fn finalize_proposal(&mut self, proposal_id: Uuid) -> Result<bool> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if !matches!(proposal.status, UpgradeStatus::Proposed) {
            return Err(Error::validation("Proposal already finalized"));
        }

        if Utc::now() < proposal.voting_deadline {
            return Err(Error::validation("Voting period not ended"));
        }

        let passed = proposal.passes(self.total_stake);

        if passed {
            // For hard forks, check validator approval
            if matches!(proposal.upgrade_type, UpgradeType::HardFork) {
                let validator_approval = proposal.validator_approval_percentage(self.total_stake);
                if validator_approval < self.hard_fork_threshold {
                    proposal.status = UpgradeStatus::Rejected;
                    return Ok(false);
                }
            }

            proposal.status = UpgradeStatus::Approved;
            Ok(true)
        } else {
            proposal.status = UpgradeStatus::Rejected;
            Ok(false)
        }
    }

    /// Schedule approved upgrade for activation
    pub fn schedule_upgrade(
        &mut self,
        proposal_id: Uuid,
        activation_height: u64,
        activation_time: DateTime<Utc>,
    ) -> Result<()> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if !matches!(proposal.status, UpgradeStatus::Approved) {
            return Err(Error::validation("Proposal must be approved first"));
        }

        proposal.activation_height = Some(activation_height);
        proposal.activation_time = Some(activation_time);
        proposal.status = UpgradeStatus::Scheduled { activation_height };

        Ok(())
    }

    /// Activate an upgrade at the scheduled height
    pub fn activate_upgrade(&mut self, proposal_id: Uuid, current_height: u64) -> Result<()> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        match proposal.status {
            UpgradeStatus::Scheduled { activation_height } => {
                if current_height < activation_height {
                    return Err(Error::validation("Activation height not reached"));
                }
            }
            _ => return Err(Error::validation("Proposal not scheduled")),
        }

        // Record fork if this is a hard fork
        if matches!(proposal.upgrade_type, UpgradeType::HardFork) {
            let fork = ForkState {
                fork_id: format!("v{}", proposal.target_version),
                parent_version: proposal.current_version.clone(),
                fork_version: proposal.target_version.clone(),
                fork_height: proposal.activation_height.unwrap(),
                fork_time: Utc::now(),
                supporting_nodes: HashSet::new(),
                total_stake: 0,
                is_canonical: true, // Assume canonical if governance passed
            };
            self.fork_history.push(fork);
        }

        // Update current version
        self.current_version = proposal.target_version.clone();
        proposal.status = UpgradeStatus::Active;

        Ok(())
    }

    /// Emergency cancel an upgrade (requires governance vote)
    pub fn cancel_upgrade(&mut self, proposal_id: Uuid) -> Result<()> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        proposal.status = UpgradeStatus::Cancelled;
        Ok(())
    }

    /// Get current protocol version
    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    /// Get all active proposals
    pub fn get_active_proposals(&self) -> Vec<&UpgradeProposal> {
        self.proposals
            .values()
            .filter(|p| p.is_voting_open())
            .collect()
    }

    /// Get upgrade proposal by ID
    pub fn get_proposal(&self, id: &Uuid) -> Option<&UpgradeProposal> {
        self.proposals.get(id)
    }

    /// Get fork history
    pub fn get_fork_history(&self) -> &[ForkState] {
        &self.fork_history
    }

    /// Check if a peer version is compatible
    pub fn is_compatible_version(&self, peer_version: &Version) -> bool {
        self.current_version.is_compatible(peer_version)
    }

    /// Update total stake
    pub fn update_total_stake(&mut self, new_total: u64) {
        self.total_stake = new_total;
    }

    /// Set hard fork threshold
    pub fn set_hard_fork_threshold(&mut self, threshold: u32) -> Result<()> {
        if threshold > 100 {
            return Err(Error::validation("Threshold cannot exceed 100%"));
        }
        self.hard_fork_threshold = threshold;
        Ok(())
    }
}

impl Default for UpgradeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn test_version_compatibility() {
        let v1 = Version::new(1, 2, 3);
        let v2 = Version::new(1, 3, 0);
        let v3 = Version::new(2, 0, 0);

        assert!(v1.is_compatible(&v2)); // Same major
        assert!(!v1.is_compatible(&v3)); // Different major
        assert!(v3.is_breaking_change(&v1)); // Major version bump
    }

    #[test]
    fn test_create_soft_fork_proposal() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(1, 1, 0);

        let proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Add new feature".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        assert_eq!(proposal.status, UpgradeStatus::Proposed);
        assert!(proposal.is_voting_open());
    }

    #[test]
    fn test_hard_fork_requires_major_version() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(1, 1, 0); // Minor bump, not major

        let result = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current,
            target,
            "Breaking change".to_string(),
            "Description".to_string(),
            14,
            67,
        );

        assert!(result.is_err()); // Should fail
    }

    #[test]
    fn test_hard_fork_with_major_version() {
        let proposer = UserId::new();
        let current = Version::new(1, 0, 0);
        let target = Version::new(2, 0, 0); // Major bump

        let result = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current,
            target,
            "Breaking change".to_string(),
            "Description".to_string(),
            14,
            67,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_upgrade_manager_submit_proposal() {
        let mut manager = UpgradeManager::new();
        let proposer = UserId::new();

        let current = manager.current_version().clone();
        let target = Version::new(current.major, current.minor + 1, 0);

        let proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Test upgrade".to_string(),
            "Description".to_string(),
            7,
            60,
        )
        .unwrap();

        let id = manager.submit_proposal(proposal).unwrap();
        assert!(manager.get_proposal(&id).is_some());
    }

    #[test]
    fn test_vote_and_finalize() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);

        let proposer = UserId::new();
        let voter1 = UserId::new();
        let voter2 = UserId::new();

        let current = manager.current_version().clone();
        let target = Version::new(current.major, current.minor + 1, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current,
            target,
            "Test upgrade".to_string(),
            "Description".to_string(),
            0, // Immediate deadline for testing
            60,
        )
        .unwrap();

        // Set deadline to past
        proposal.voting_deadline = Utc::now() - Duration::seconds(1);
        let proposal_id = manager.submit_proposal(proposal).unwrap();

        // Cast votes
        manager
            .cast_upgrade_vote(proposal_id, voter1, true, 7000)
            .unwrap();
        manager
            .cast_upgrade_vote(proposal_id, voter2, false, 2000)
            .unwrap();

        // Finalize
        let passed = manager.finalize_proposal(proposal_id).unwrap();
        assert!(passed); // 7000 > 2000 and meets 60% quorum
    }

    #[test]
    fn test_validator_signatures() {
        let proposer = UserId::new();
        let validator1 = UserId::new();
        let validator2 = UserId::new();

        let current = Version::new(1, 0, 0);
        let target = Version::new(2, 0, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current,
            target,
            "Hard fork".to_string(),
            "Description".to_string(),
            14,
            67,
        )
        .unwrap();

        let sig1 = ValidatorSignature {
            validator_id: validator1,
            stake_amount: 4000,
            signature: vec![1, 2, 3],
            signed_at: Utc::now(),
        };

        let sig2 = ValidatorSignature {
            validator_id: validator2,
            stake_amount: 3000,
            signature: vec![4, 5, 6],
            signed_at: Utc::now(),
        };

        proposal.add_validator_signature(sig1).unwrap();
        proposal.add_validator_signature(sig2).unwrap();

        assert_eq!(proposal.validator_approval_percentage(10000), 70); // 7000/10000
    }

    #[test]
    fn test_hard_fork_threshold_check() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);
        manager.set_hard_fork_threshold(67).unwrap();

        let proposer = UserId::new();
        let current = manager.current_version().clone();
        let target = Version::new(current.major + 1, 0, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current,
            target,
            "Hard fork".to_string(),
            "Description".to_string(),
            0,
            60,
        )
        .unwrap();

        // Add validator signatures (only 60% approval)
        let validator = UserId::new();
        let sig = ValidatorSignature {
            validator_id: validator,
            stake_amount: 6000,
            signature: vec![1, 2, 3],
            signed_at: Utc::now(),
        };
        proposal.add_validator_signature(sig).unwrap();

        proposal.voting_deadline = Utc::now() - Duration::seconds(1);
        proposal.votes_for = 7000;
        proposal.votes_against = 2000;

        let proposal_id = manager.submit_proposal(proposal).unwrap();

        // Should fail due to insufficient validator approval (60% < 67%)
        let passed = manager.finalize_proposal(proposal_id).unwrap();
        assert!(!passed);
    }

    #[test]
    fn test_schedule_and_activate() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);

        let proposer = UserId::new();
        let current = manager.current_version().clone();
        let target = Version::new(current.major, current.minor + 1, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::SoftFork,
            current.clone(),
            target.clone(),
            "Test upgrade".to_string(),
            "Description".to_string(),
            0,
            60,
        )
        .unwrap();

        proposal.voting_deadline = Utc::now() - Duration::seconds(1);
        proposal.votes_for = 7000;
        proposal.votes_against = 2000;

        let proposal_id = manager.submit_proposal(proposal).unwrap();
        manager.finalize_proposal(proposal_id).unwrap();

        // Schedule
        manager
            .schedule_upgrade(proposal_id, 1000, Utc::now() + Duration::hours(1))
            .unwrap();

        // Activate
        manager.activate_upgrade(proposal_id, 1000).unwrap();

        assert_eq!(manager.current_version(), &target);
    }

    #[test]
    fn test_fork_history_tracking() {
        let mut manager = UpgradeManager::new();
        manager.update_total_stake(10000);

        let proposer = UserId::new();
        let current = manager.current_version().clone();
        let target = Version::new(current.major + 1, 0, 0);

        let mut proposal = UpgradeProposal::new(
            proposer,
            UpgradeType::HardFork,
            current.clone(),
            target.clone(),
            "Hard fork".to_string(),
            "Description".to_string(),
            0,
            60,
        )
        .unwrap();

        // Add sufficient validator signatures
        for i in 0..7 {
            let validator = UserId::new();
            let sig = ValidatorSignature {
                validator_id: validator,
                stake_amount: 1000,
                signature: vec![i],
                signed_at: Utc::now(),
            };
            proposal.add_validator_signature(sig).unwrap();
        }

        proposal.voting_deadline = Utc::now() - Duration::seconds(1);
        proposal.votes_for = 7000;
        proposal.votes_against = 2000;

        let proposal_id = manager.submit_proposal(proposal).unwrap();
        manager.finalize_proposal(proposal_id).unwrap();
        manager
            .schedule_upgrade(proposal_id, 1000, Utc::now())
            .unwrap();
        manager.activate_upgrade(proposal_id, 1000).unwrap();

        // Check fork history
        let forks = manager.get_fork_history();
        assert_eq!(forks.len(), 1);
        assert_eq!(forks[0].fork_version, target);
        assert_eq!(forks[0].parent_version, current);
        assert!(forks[0].is_canonical);
    }
}
