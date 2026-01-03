// DAO Voting Infrastructure
//
// This module implements token-weighted voting, proposals, and
// decentralized governance for protocol decisions.
//
// Security: Uses commit-reveal scheme for anonymous voting to prevent
// early result visibility and vote buying.

use chrono::{DateTime, Duration, Utc};
use dchat_core::config::GovernanceConfig;
use dchat_core::{Error, Result, UserId};
use dchat_crypto::{decrypt_with_key, encrypt_with_key, KEY_SIZE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

/// Type of proposal being voted on
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalType {
    /// Protocol feature addition or modification
    FeatureChange,
    /// Slashing decision for bad actor
    Slashing,
    /// Treasury allocation
    TreasurySpend,
    /// Moderation policy update
    ModerationPolicy,
    /// Emergency protocol action
    Emergency,
}

/// A governance proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    /// Unique proposal ID
    pub id: Uuid,
    /// Proposal creator
    pub proposer: UserId,
    /// Proposal type
    pub proposal_type: ProposalType,
    /// Human-readable title
    pub title: String,
    /// Detailed description
    pub description: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Voting deadline
    pub deadline: DateTime<Utc>,
    /// Minimum quorum in basis points (0-10000, where 10000 = 100% of total stake)
    pub quorum_bps: u16,
    /// Current vote tally
    pub votes_for: u64,
    pub votes_against: u64,
    /// Has voting ended?
    pub finalized: bool,
}

/// A vote on a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    /// Voter ID
    pub voter: UserId,
    /// Proposal being voted on
    pub proposal_id: Uuid,
    /// Vote commitment hash (H(vote_choice || salt))
    /// Used for commit-reveal anonymous voting
    pub commitment: [u8; 32],
    /// Encrypted ballot (legacy, kept for backward compatibility)
    #[serde(default)]
    pub encrypted_ballot: Vec<u8>,
    /// Revealed ballot (Some after reveal phase)
    pub revealed_ballot: Option<bool>, // true = for, false = against
    /// Salt used for commitment (populated after reveal)
    #[serde(default)]
    pub reveal_salt: Option<[u8; 32]>,
    /// Voting power (token stake)
    pub voting_power: u64,
    /// Timestamp
    pub cast_at: DateTime<Utc>,
}

/// Configuration for the vote manager
#[derive(Debug, Clone)]
pub struct VoteManagerConfig {
    /// Default quorum threshold in basis points (0-10000)
    pub default_quorum_bps: u16,
    /// Default voting period in hours
    pub voting_period_hours: u32,
    /// Minimum stake required to submit a proposal
    pub minimum_stake_for_proposal: u64,
    /// Enable anonymous/encrypted voting
    pub enable_anonymous_voting: bool,
    /// Default approval threshold in basis points
    pub default_approval_bps: u16,
}

impl Default for VoteManagerConfig {
    fn default() -> Self {
        Self {
            default_quorum_bps: 1000, // 10%
            voting_period_hours: 168, // 1 week
            minimum_stake_for_proposal: 1000,
            enable_anonymous_voting: true,
            default_approval_bps: 5001, // Simple majority
        }
    }
}

impl From<GovernanceConfig> for VoteManagerConfig {
    fn from(cfg: GovernanceConfig) -> Self {
        Self {
            default_quorum_bps: cfg.quorum_threshold_bps,
            voting_period_hours: cfg.voting_period_hours,
            minimum_stake_for_proposal: cfg.minimum_stake_for_proposal,
            enable_anonymous_voting: cfg.enable_anonymous_voting,
            default_approval_bps: cfg.approval_threshold_bps,
        }
    }
}

/// Manager for proposals and voting
pub struct VoteManager {
    /// Active proposals
    proposals: HashMap<Uuid, Proposal>,
    /// Cast votes
    votes: HashMap<Uuid, Vec<Vote>>,
    /// Total staked tokens in system
    total_stake: u64,
    /// Configuration
    config: VoteManagerConfig,
}

impl Proposal {
    /// Create a new proposal
    pub fn new(
        proposer: UserId,
        proposal_type: ProposalType,
        title: String,
        description: String,
        voting_period_days: i64,
        quorum_bps: u16,
    ) -> Result<Self> {
        if quorum_bps > 10000 {
            return Err(Error::validation(
                "Quorum cannot exceed 10000 bps (100%)".to_string(),
            ));
        }

        let now = Utc::now();
        let deadline = now + Duration::days(voting_period_days);

        Ok(Self {
            id: Uuid::new_v4(),
            proposer,
            proposal_type,
            title,
            description,
            created_at: now,
            deadline,
            quorum_bps,
            votes_for: 0,
            votes_against: 0,
            finalized: false,
        })
    }

    /// Check if voting is still open
    pub fn is_open(&self) -> bool {
        !self.finalized && Utc::now() < self.deadline
    }

    /// Check if quorum has been met
    pub fn meets_quorum(&self, total_stake: u64) -> bool {
        let total_votes = self.votes_for + self.votes_against;
        // quorum_bps is in basis points (0-10000)
        let required_votes = (total_stake as u128 * self.quorum_bps as u128 / 10000) as u64;
        total_votes >= required_votes
    }

    /// Check if proposal passes
    pub fn passes(&self) -> bool {
        self.votes_for > self.votes_against
    }
}

/// Compute vote commitment hash: H(vote_choice || salt)
pub fn compute_vote_commitment(vote_for: bool, salt: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update([if vote_for { 1u8 } else { 0u8 }]);
    hasher.update(salt);
    hasher.finalize().into()
}

/// Verify a vote reveal against its commitment
pub fn verify_vote_reveal(commitment: &[u8; 32], vote_for: bool, salt: &[u8; 32]) -> bool {
    let computed = compute_vote_commitment(vote_for, salt);
    &computed == commitment
}

/// Generate a cryptographically secure random salt for vote commitment
pub fn generate_vote_salt() -> [u8; 32] {
    use rand::RngCore;
    let mut salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

impl Vote {
    /// Create a committed vote using commit-reveal scheme
    ///
    /// The voter provides a commitment H(vote || salt). The actual vote
    /// is only revealed after the voting deadline to prevent early
    /// result visibility and vote buying.
    ///
    /// Returns: (Vote, salt) - voter must save salt to reveal later
    pub fn new_committed(
        voter: UserId,
        proposal_id: Uuid,
        vote_for: bool,
        voting_power: u64,
    ) -> (Self, [u8; 32]) {
        let salt = generate_vote_salt();
        let commitment = compute_vote_commitment(vote_for, &salt);

        let vote = Self {
            voter,
            proposal_id,
            commitment,
            encrypted_ballot: Vec::new(), // Not used in commit-reveal
            revealed_ballot: None,
            reveal_salt: None,
            voting_power,
            cast_at: Utc::now(),
        };

        (vote, salt)
    }

    /// Create a vote with a pre-computed commitment
    ///
    /// Used when the voter has already computed the commitment client-side.
    pub fn from_commitment(
        voter: UserId,
        proposal_id: Uuid,
        commitment: [u8; 32],
        voting_power: u64,
    ) -> Self {
        Self {
            voter,
            proposal_id,
            commitment,
            encrypted_ballot: Vec::new(),
            revealed_ballot: None,
            reveal_salt: None,
            voting_power,
            cast_at: Utc::now(),
        }
    }

    /// Create an encrypted vote using AES-256-GCM (legacy)
    ///
    /// Kept for backward compatibility. New code should use new_committed.
    #[deprecated(since = "0.2.0", note = "Use new_committed for commit-reveal scheme")]
    pub fn new_encrypted(
        voter: UserId,
        proposal_id: Uuid,
        vote_for: bool,
        voting_power: u64,
        encryption_key: &[u8; KEY_SIZE],
    ) -> Result<Self> {
        // Encode vote as single byte
        let plaintext = if vote_for { vec![1u8] } else { vec![0u8] };

        // Encrypt using AES-256-GCM (authenticated encryption)
        let encrypted_ballot = encrypt_with_key(encryption_key, &plaintext)?;

        // Generate dummy commitment for compatibility
        let salt = generate_vote_salt();
        let commitment = compute_vote_commitment(vote_for, &salt);

        Ok(Self {
            voter,
            proposal_id,
            commitment,
            encrypted_ballot,
            revealed_ballot: None,
            reveal_salt: Some(salt),
            voting_power,
            cast_at: Utc::now(),
        })
    }

    /// Reveal the ballot using commit-reveal scheme
    ///
    /// Voter provides the original vote choice and salt. The system verifies
    /// that H(vote || salt) matches the original commitment.
    pub fn reveal_with_salt(&mut self, vote_for: bool, salt: &[u8; 32]) -> Result<bool> {
        if self.revealed_ballot.is_some() {
            return Err(Error::validation("Ballot already revealed".to_string()));
        }

        // Verify the commitment
        if !verify_vote_reveal(&self.commitment, vote_for, salt) {
            return Err(Error::validation(
                "Vote reveal does not match commitment".to_string(),
            ));
        }

        self.revealed_ballot = Some(vote_for);
        self.reveal_salt = Some(*salt);
        Ok(vote_for)
    }

    /// Reveal the ballot after voting deadline using AES-256-GCM decryption (legacy)
    #[deprecated(
        since = "0.2.0",
        note = "Use reveal_with_salt for commit-reveal scheme"
    )]
    pub fn reveal(&mut self, decryption_key: &[u8; KEY_SIZE]) -> Result<bool> {
        if self.revealed_ballot.is_some() {
            return Err(Error::validation("Ballot already revealed".to_string()));
        }

        // Decrypt using AES-256-GCM (verifies authentication tag)
        let plaintext = decrypt_with_key(decryption_key, &self.encrypted_ballot)?;

        if plaintext.is_empty() {
            return Err(Error::validation("Invalid ballot format".to_string()));
        }

        let vote_for = plaintext[0] == 1;
        self.revealed_ballot = Some(vote_for);
        Ok(vote_for)
    }

    /// Check if the vote has been revealed
    pub fn is_revealed(&self) -> bool {
        self.revealed_ballot.is_some()
    }

    /// Get the commitment hash
    pub fn commitment(&self) -> &[u8; 32] {
        &self.commitment
    }
}

impl VoteManager {
    /// Create a new vote manager with default configuration
    pub fn new(total_stake: u64) -> Self {
        Self {
            proposals: HashMap::new(),
            votes: HashMap::new(),
            total_stake,
            config: VoteManagerConfig::default(),
        }
    }

    /// Create a new vote manager with custom configuration
    pub fn with_config(total_stake: u64, config: VoteManagerConfig) -> Self {
        Self {
            proposals: HashMap::new(),
            votes: HashMap::new(),
            total_stake,
            config,
        }
    }

    /// Get the current configuration
    pub fn config(&self) -> &VoteManagerConfig {
        &self.config
    }

    /// Create a proposal using the manager's default configuration
    pub fn create_proposal(
        &self,
        proposer: UserId,
        proposal_type: ProposalType,
        title: String,
        description: String,
    ) -> Result<Proposal> {
        // Convert voting period from hours to days
        let voting_period_days = (self.config.voting_period_hours as i64 + 23) / 24;
        Proposal::new(
            proposer,
            proposal_type,
            title,
            description,
            voting_period_days,
            self.config.default_quorum_bps,
        )
    }

    /// Submit a new proposal
    pub fn submit_proposal(&mut self, proposal: Proposal) -> Result<Uuid> {
        let id = proposal.id;
        self.proposals.insert(id, proposal);
        self.votes.insert(id, Vec::new());
        Ok(id)
    }

    /// Cast a vote on a proposal
    pub fn cast_vote(&mut self, vote: Vote) -> Result<()> {
        let proposal = self
            .proposals
            .get(&vote.proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if !proposal.is_open() {
            return Err(Error::validation("Voting is closed".to_string()));
        }

        // Check for duplicate vote
        let existing_votes = self.votes.get(&vote.proposal_id).unwrap();
        if existing_votes.iter().any(|v| v.voter == vote.voter) {
            return Err(Error::validation("Already voted".to_string()));
        }

        self.votes.get_mut(&vote.proposal_id).unwrap().push(vote);
        Ok(())
    }

    /// Reveal all votes for a proposal (after deadline)
    pub fn reveal_votes(&mut self, proposal_id: &Uuid, decryption_key: &[u8; 32]) -> Result<()> {
        let proposal = self
            .proposals
            .get(proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if Utc::now() < proposal.deadline {
            return Err(Error::validation("Voting period not ended".to_string()));
        }

        let votes = self.votes.get_mut(proposal_id).unwrap();
        for vote in votes.iter_mut() {
            if vote.revealed_ballot.is_none() {
                vote.reveal(decryption_key)?;
            }
        }

        Ok(())
    }

    /// Finalize a proposal (count votes and determine outcome)
    pub fn finalize_proposal(&mut self, proposal_id: &Uuid) -> Result<bool> {
        let proposal = self
            .proposals
            .get_mut(proposal_id)
            .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

        if proposal.finalized {
            return Err(Error::validation("Proposal already finalized".to_string()));
        }

        if Utc::now() < proposal.deadline {
            return Err(Error::validation("Voting period not ended".to_string()));
        }

        // Tally revealed votes
        let votes = self.votes.get(proposal_id).unwrap();
        let mut votes_for = 0u64;
        let mut votes_against = 0u64;

        for vote in votes {
            if let Some(ballot) = vote.revealed_ballot {
                if ballot {
                    votes_for += vote.voting_power;
                } else {
                    votes_against += vote.voting_power;
                }
            }
        }

        proposal.votes_for = votes_for;
        proposal.votes_against = votes_against;
        proposal.finalized = true;

        // Check quorum and result
        if !proposal.meets_quorum(self.total_stake) {
            return Ok(false); // Failed due to quorum
        }

        Ok(proposal.passes())
    }

    /// Get proposal by ID
    pub fn get_proposal(&self, id: &Uuid) -> Option<&Proposal> {
        self.proposals.get(id)
    }

    /// Get all active proposals
    pub fn get_active_proposals(&self) -> Vec<&Proposal> {
        self.proposals.values().filter(|p| p.is_open()).collect()
    }

    /// Update total stake
    pub fn update_total_stake(&mut self, new_total: u64) {
        self.total_stake = new_total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposal_creation() {
        let proposer = UserId::new();
        let proposal = Proposal::new(
            proposer,
            ProposalType::FeatureChange,
            "Test Proposal".to_string(),
            "A test proposal".to_string(),
            7,
            5000, // 50% quorum in bps
        )
        .unwrap();

        assert_eq!(proposal.title, "Test Proposal");
        assert_eq!(proposal.quorum_bps, 5000);
        assert!(proposal.is_open());
        assert!(!proposal.finalized);
    }

    #[test]
    fn test_vote_encryption_decryption() {
        let voter = UserId::new();
        let proposal_id = Uuid::new_v4();
        let key = [42u8; 32];

        let mut vote = Vote::new_encrypted(voter, proposal_id, true, 100, &key).unwrap();
        assert!(vote.revealed_ballot.is_none());

        let revealed = vote.reveal(&key).unwrap();
        assert_eq!(revealed, true);
        assert_eq!(vote.revealed_ballot, Some(true));
    }

    #[test]
    fn test_vote_manager_submit_proposal() {
        let mut manager = VoteManager::new(10000);
        let proposer = UserId::new();

        let proposal = Proposal::new(
            proposer,
            ProposalType::TreasurySpend,
            "Fund Project X".to_string(),
            "Allocate 1000 tokens to Project X".to_string(),
            7,
            6000, // 60% quorum in bps
        )
        .unwrap();

        let id = manager.submit_proposal(proposal).unwrap();
        assert!(manager.get_proposal(&id).is_some());
    }

    #[test]
    fn test_vote_casting() {
        let mut manager = VoteManager::new(10000);
        let proposer = UserId::new();
        let voter = UserId::new();

        let proposal = Proposal::new(
            proposer,
            ProposalType::FeatureChange,
            "Test".to_string(),
            "Test".to_string(),
            7,
            5000, // 50% in bps
        )
        .unwrap();
        let proposal_id = manager.submit_proposal(proposal).unwrap();

        let key = [1u8; 32];
        let vote = Vote::new_encrypted(voter, proposal_id, true, 100, &key).unwrap();

        manager.cast_vote(vote).unwrap();
    }

    #[test]
    fn test_duplicate_vote_prevention() {
        let mut manager = VoteManager::new(10000);
        let proposer = UserId::new();
        let voter = UserId::new();

        let proposal = Proposal::new(
            proposer,
            ProposalType::FeatureChange,
            "Test".to_string(),
            "Test".to_string(),
            7,
            5000, // 50% in bps
        )
        .unwrap();
        let proposal_id = manager.submit_proposal(proposal).unwrap();

        let key = [1u8; 32];
        let vote1 = Vote::new_encrypted(voter.clone(), proposal_id, true, 100, &key).unwrap();
        let vote2 = Vote::new_encrypted(voter, proposal_id, false, 100, &key).unwrap();

        manager.cast_vote(vote1).unwrap();
        let result = manager.cast_vote(vote2);
        assert!(result.is_err()); // Should fail
    }

    #[test]
    fn test_quorum_check() {
        let proposal = Proposal {
            id: Uuid::new_v4(),
            proposer: UserId::new(),
            proposal_type: ProposalType::FeatureChange,
            title: "Test".to_string(),
            description: "Test".to_string(),
            created_at: Utc::now(),
            deadline: Utc::now() + Duration::days(7),
            quorum_bps: 5000, // 50% in basis points
            votes_for: 600,
            votes_against: 400,
            finalized: false,
        };

        assert!(proposal.meets_quorum(2000)); // 1000/2000 = 50%
        assert!(!proposal.meets_quorum(3000)); // 1000/3000 = 33% < 50%
    }

    #[test]
    fn test_proposal_passes() {
        let mut proposal = Proposal {
            id: Uuid::new_v4(),
            proposer: UserId::new(),
            proposal_type: ProposalType::FeatureChange,
            title: "Test".to_string(),
            description: "Test".to_string(),
            created_at: Utc::now(),
            deadline: Utc::now() + Duration::days(7),
            quorum_bps: 5000, // 50% in basis points
            votes_for: 600,
            votes_against: 400,
            finalized: false,
        };

        assert!(proposal.passes());

        proposal.votes_for = 400;
        proposal.votes_against = 600;
        assert!(!proposal.passes());
    }

    #[test]
    fn test_active_proposals_filter() {
        let mut manager = VoteManager::new(10000);
        let proposer = UserId::new();

        let proposal1 = Proposal::new(
            proposer.clone(),
            ProposalType::FeatureChange,
            "Active".to_string(),
            "Active proposal".to_string(),
            7,
            5000, // 50% in bps
        )
        .unwrap();

        let mut proposal2 = Proposal::new(
            proposer,
            ProposalType::FeatureChange,
            "Finalized".to_string(),
            "Finalized proposal".to_string(),
            7,
            5000, // 50% in bps
        )
        .unwrap();
        proposal2.finalized = true;

        manager.submit_proposal(proposal1).unwrap();
        manager.submit_proposal(proposal2).unwrap();

        let active = manager.get_active_proposals();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].title, "Active");
    }

    #[test]
    fn test_commit_reveal_vote() {
        let voter = UserId::new();
        let proposal_id = Uuid::new_v4();

        // Create a committed vote
        let (mut vote, salt) = Vote::new_committed(voter, proposal_id, true, 100);

        // Vote should not be revealed yet
        assert!(vote.revealed_ballot.is_none());
        assert!(!vote.is_revealed());

        // Reveal with correct salt
        let revealed = vote.reveal_with_salt(true, &salt).unwrap();
        assert!(revealed);
        assert_eq!(vote.revealed_ballot, Some(true));
        assert!(vote.is_revealed());
    }

    #[test]
    fn test_commit_reveal_wrong_salt_fails() {
        let voter = UserId::new();
        let proposal_id = Uuid::new_v4();

        // Create a committed vote
        let (mut vote, _original_salt) = Vote::new_committed(voter, proposal_id, true, 100);

        // Try to reveal with wrong salt
        let wrong_salt = [99u8; 32];
        let result = vote.reveal_with_salt(true, &wrong_salt);
        assert!(result.is_err());
    }

    #[test]
    fn test_commit_reveal_wrong_choice_fails() {
        let voter = UserId::new();
        let proposal_id = Uuid::new_v4();

        // Create a committed vote for "yes"
        let (mut vote, salt) = Vote::new_committed(voter, proposal_id, true, 100);

        // Try to reveal with different choice (vote flipping attack)
        let result = vote.reveal_with_salt(false, &salt);
        assert!(result.is_err());
    }

    #[test]
    fn test_vote_commitment_helpers() {
        let salt = [42u8; 32];

        // Compute commitment
        let commitment_for = compute_vote_commitment(true, &salt);
        let commitment_against = compute_vote_commitment(false, &salt);

        // Different votes should have different commitments
        assert_ne!(commitment_for, commitment_against);

        // Verify reveals
        assert!(verify_vote_reveal(&commitment_for, true, &salt));
        assert!(!verify_vote_reveal(&commitment_for, false, &salt));
        assert!(verify_vote_reveal(&commitment_against, false, &salt));
        assert!(!verify_vote_reveal(&commitment_against, true, &salt));

        // Wrong salt should fail
        let wrong_salt = [99u8; 32];
        assert!(!verify_vote_reveal(&commitment_for, true, &wrong_salt));
    }

    #[test]
    fn test_generate_vote_salt() {
        let salt1 = generate_vote_salt();
        let salt2 = generate_vote_salt();

        // Salts should be unique
        assert_ne!(salt1, salt2);

        // Salts should not be all zeros
        assert_ne!(salt1, [0u8; 32]);
        assert_ne!(salt2, [0u8; 32]);
    }

    #[test]
    fn test_vote_from_commitment() {
        let voter = UserId::new();
        let proposal_id = Uuid::new_v4();
        let salt = generate_vote_salt();
        let commitment = compute_vote_commitment(true, &salt);

        // Create vote from pre-computed commitment
        let mut vote = Vote::from_commitment(voter, proposal_id, commitment, 100);

        // Should reveal correctly with original choice and salt
        let revealed = vote.reveal_with_salt(true, &salt).unwrap();
        assert!(revealed);
    }

    #[test]
    fn test_double_reveal_fails() {
        let voter = UserId::new();
        let proposal_id = Uuid::new_v4();

        let (mut vote, salt) = Vote::new_committed(voter, proposal_id, true, 100);

        // First reveal succeeds
        vote.reveal_with_salt(true, &salt).unwrap();

        // Second reveal fails
        let result = vote.reveal_with_salt(true, &salt);
        assert!(result.is_err());
    }
}
