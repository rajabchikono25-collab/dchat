// DAO Voting Infrastructure
//
// This module implements token-weighted voting, proposals, and
// decentralized governance for protocol decisions.

use chrono::{DateTime, Duration, Utc};
use dchat_core::{Error, Result, UserId};
use serde::{Deserialize, Serialize};
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
    /// Minimum quorum (percentage of total stake)
    pub quorum_percentage: u32,
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
    /// Encrypted ballot (revealed after deadline)
    pub encrypted_ballot: Vec<u8>,
    /// Revealed ballot (Some after reveal phase)
    pub revealed_ballot: Option<bool>, // true = for, false = against
    /// Voting power (token stake)
    pub voting_power: u64,
    /// Timestamp
    pub cast_at: DateTime<Utc>,
}

/// Manager for proposals and voting
pub struct VoteManager {
    /// Active proposals
    proposals: HashMap<Uuid, Proposal>,
    /// Cast votes
    votes: HashMap<Uuid, Vec<Vote>>,
    /// Total staked tokens in system
    total_stake: u64,
}

impl Proposal {
    /// Create a new proposal
    pub fn new(
        proposer: UserId,
        proposal_type: ProposalType,
        title: String,
        description: String,
        voting_period_days: i64,
        quorum_percentage: u32,
    ) -> Result<Self> {
        if quorum_percentage > 100 {
            return Err(Error::validation("Quorum cannot exceed 100%".to_string()));
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
            quorum_percentage,
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
        let required_votes = (total_stake * self.quorum_percentage as u64) / 100;
        total_votes >= required_votes
    }

    /// Check if proposal passes
    pub fn passes(&self) -> bool {
        self.votes_for > self.votes_against
    }
}

impl Vote {
    /// Create an encrypted vote
    ///
    /// Ballot is encrypted to prevent early result visibility
    pub fn new_encrypted(
        voter: UserId,
        proposal_id: Uuid,
        vote_for: bool,
        voting_power: u64,
        encryption_key: &[u8; 32],
    ) -> Result<Self> {
        // Simple XOR encryption (production should use AES-GCM)
        let plaintext = if vote_for { vec![1u8] } else { vec![0u8] };
        let mut encrypted_ballot = plaintext.clone();
        for (i, byte) in encrypted_ballot.iter_mut().enumerate() {
            *byte ^= encryption_key[i % 32];
        }

        Ok(Self {
            voter,
            proposal_id,
            encrypted_ballot,
            revealed_ballot: None,
            voting_power,
            cast_at: Utc::now(),
        })
    }

    /// Reveal the ballot after voting deadline
    pub fn reveal(&mut self, decryption_key: &[u8; 32]) -> Result<bool> {
        if self.revealed_ballot.is_some() {
            return Err(Error::validation("Ballot already revealed".to_string()));
        }

        // Decrypt (simple XOR)
        let mut plaintext = self.encrypted_ballot.clone();
        for (i, byte) in plaintext.iter_mut().enumerate() {
            *byte ^= decryption_key[i % 32];
        }

        let vote_for = plaintext[0] == 1;
        self.revealed_ballot = Some(vote_for);
        Ok(vote_for)
    }
}

impl VoteManager {
    /// Create a new vote manager
    pub fn new(total_stake: u64) -> Self {
        Self {
            proposals: HashMap::new(),
            votes: HashMap::new(),
            total_stake,
        }
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
            50,
        )
        .unwrap();

        assert_eq!(proposal.title, "Test Proposal");
        assert_eq!(proposal.quorum_percentage, 50);
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
            60,
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
            50,
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
            50,
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
            quorum_percentage: 50,
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
            quorum_percentage: 50,
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
            50,
        )
        .unwrap();

        let mut proposal2 = Proposal::new(
            proposer,
            ProposalType::FeatureChange,
            "Finalized".to_string(),
            "Finalized proposal".to_string(),
            7,
            50,
        )
        .unwrap();
        proposal2.finalized = true;

        manager.submit_proposal(proposal1).unwrap();
        manager.submit_proposal(proposal2).unwrap();

        let active = manager.get_active_proposals();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].title, "Active");
    }
}
