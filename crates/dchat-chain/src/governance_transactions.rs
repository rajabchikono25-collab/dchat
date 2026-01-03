//! On-chain governance transaction types
//!
//! This module defines blockchain transaction types for decentralized governance:
//! - Proposal submission (protocol upgrades, parameter changes, treasury, etc.)
//! - Vote commitment (commit-reveal voting for privacy)
//! - Vote reveal (disclose votes after deadline)
//! - Proposal finalization (tally votes and record outcome)
//! - Governance execution (enact passed proposals)
//!
//! All governance transactions are committed on-chain to ensure:
//! - Transparency and auditability
//! - Censorship resistance
//! - Verifiable vote tallies
//! - Immutable governance history

use chrono::{DateTime, Utc};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Governance transaction type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GovernanceTransactionType {
    /// Submit a new governance proposal
    SubmitProposal,
    /// Commit a vote (hash of vote + salt)
    CommitVote,
    /// Reveal a previously committed vote
    RevealVote,
    /// Finalize proposal (tally votes, determine outcome)
    FinalizeProposal,
    /// Execute a passed proposal
    ExecuteProposal,
    /// Cancel a proposal (by proposer or emergency multisig)
    CancelProposal,
    /// Delegate voting power to another user
    DelegateVotingPower,
    /// Revoke voting power delegation
    RevokeDelegation,
}

/// Type of governance proposal
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GovernanceProposalType {
    /// Protocol upgrade (soft fork or hard fork)
    ProtocolUpgrade {
        /// Target version string (semver)
        target_version: String,
        /// Hash of upgrade specification
        upgrade_hash: String,
        /// Is this a hard fork requiring validator coordination?
        is_hard_fork: bool,
    },
    /// Change a protocol parameter
    ParameterChange {
        /// Parameter name (e.g., "block_time", "min_stake")
        parameter: String,
        /// Current value (serialized)
        current_value: String,
        /// Proposed new value (serialized)
        proposed_value: String,
    },
    /// Allocate funds from treasury
    TreasuryAllocation {
        /// Recipient address/user
        recipient: UserId,
        /// Amount in base units
        amount: u64,
        /// Purpose description
        purpose: String,
    },
    /// Toggle a protocol feature flag
    FeatureToggle {
        /// Feature name
        feature_name: String,
        /// Enable or disable
        enable: bool,
    },
    /// Emergency action (pause, circuit breaker, etc.)
    EmergencyAction {
        /// Action type (pause_protocol, resume_protocol, circuit_breaker)
        action_type: String,
        /// Additional parameters
        parameters: String,
    },
    /// Create a grant program
    GrantProgram {
        /// Program name
        program_name: String,
        /// Total budget allocation
        total_budget: u64,
        /// Duration in days
        duration_days: u32,
    },
    /// Slash a validator or user
    SlashingProposal {
        /// Target to slash
        target: UserId,
        /// Amount to slash
        amount: u64,
        /// Reason for slashing
        reason: String,
        /// Evidence hash (e.g., signed misbehavior proof)
        evidence_hash: String,
    },
    /// Generic text proposal (signaling, non-binding)
    TextProposal {
        /// Proposal content hash
        content_hash: String,
    },
}

/// On-chain governance proposal submission transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceProposalTx {
    /// Unique proposal identifier
    pub proposal_id: Uuid,
    /// Proposer user ID
    pub proposer: UserId,
    /// Proposal type with specific parameters
    pub proposal_type: GovernanceProposalType,
    /// Human-readable title
    pub title: String,
    /// Description hash (full description stored off-chain)
    pub description_hash: String,
    /// Rationale hash (justification stored off-chain)
    pub rationale_hash: String,
    /// Voting start block height
    pub voting_start_height: u64,
    /// Voting end block height
    pub voting_end_height: u64,
    /// Required quorum in basis points (0-10000)
    pub quorum_required_bps: u16,
    /// Required approval in basis points (0-10000)
    pub approval_required_bps: u16,
    /// Proposal deposit amount (slashed if spam)
    pub deposit_amount: u64,
    /// Submission timestamp
    pub submitted_at: DateTime<Utc>,
    /// Proposer's signature over proposal data
    pub signature: Vec<u8>,
}

/// On-chain vote commitment transaction (commit phase of commit-reveal)
///
/// Voters submit hash(vote || salt) during voting period.
/// This hides their vote until the reveal phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceVoteCommitTx {
    /// Proposal being voted on
    pub proposal_id: Uuid,
    /// Voter user ID
    pub voter: UserId,
    /// Commitment: hash(vote_choice || salt)
    /// vote_choice: 0 = against, 1 = for, 2 = abstain
    pub commitment: [u8; 32],
    /// Voter's voting power at snapshot height
    pub voting_power: u64,
    /// Block height of voting power snapshot
    pub snapshot_height: u64,
    /// Commitment timestamp
    pub committed_at: DateTime<Utc>,
    /// Voter's signature
    pub signature: Vec<u8>,
}

/// On-chain vote reveal transaction (reveal phase of commit-reveal)
///
/// After voting ends, voters reveal their actual vote and salt.
/// The chain verifies hash(vote || salt) == commitment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceVoteRevealTx {
    /// Proposal being voted on
    pub proposal_id: Uuid,
    /// Voter user ID
    pub voter: UserId,
    /// Revealed vote choice: 0 = against, 1 = for, 2 = abstain
    pub vote_choice: u8,
    /// Salt used in commitment
    pub salt: [u8; 32],
    /// Reveal timestamp
    pub revealed_at: DateTime<Utc>,
    /// Voter's signature
    pub signature: Vec<u8>,
}

/// Vote choice enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum VoteChoice {
    Against = 0,
    For = 1,
    Abstain = 2,
}

impl VoteChoice {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(VoteChoice::Against),
            1 => Some(VoteChoice::For),
            2 => Some(VoteChoice::Abstain),
            _ => None,
        }
    }
}

/// On-chain proposal finalization transaction
///
/// Submitted after voting and reveal phases end.
/// Records final tally and outcome on-chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceFinalizeTx {
    /// Proposal being finalized
    pub proposal_id: Uuid,
    /// Finalizer (can be anyone after deadline)
    pub finalizer: UserId,
    /// Total votes for (weighted by voting power)
    pub total_votes_for: u64,
    /// Total votes against (weighted by voting power)
    pub total_votes_against: u64,
    /// Total votes abstain (weighted by voting power)
    pub total_votes_abstain: u64,
    /// Total eligible voting power at snapshot
    pub total_eligible_power: u64,
    /// Did proposal meet quorum?
    pub quorum_met: bool,
    /// Final outcome
    pub outcome: ProposalOutcome,
    /// Merkle root of all revealed votes (for verification)
    pub votes_merkle_root: [u8; 32],
    /// Finalization timestamp
    pub finalized_at: DateTime<Utc>,
    /// Finalizer's signature
    pub signature: Vec<u8>,
}

/// Proposal outcome after finalization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalOutcome {
    /// Passed: quorum met and approval threshold reached
    Passed,
    /// Rejected: quorum met but approval threshold not reached
    Rejected,
    /// Failed: quorum not met
    QuorumNotMet,
    /// Cancelled: cancelled by proposer or emergency action
    Cancelled,
    /// Expired: not finalized within allowed window
    Expired,
}

/// On-chain proposal execution transaction
///
/// For passed proposals, records when/how the proposal was executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceExecuteTx {
    /// Proposal being executed
    pub proposal_id: Uuid,
    /// Executor (may require specific permissions)
    pub executor: UserId,
    /// Execution result
    pub result: ExecutionResult,
    /// State root before execution
    pub pre_state_root: [u8; 32],
    /// State root after execution
    pub post_state_root: [u8; 32],
    /// Block height at which execution occurred
    pub execution_height: u64,
    /// Execution timestamp
    pub executed_at: DateTime<Utc>,
    /// Executor's signature
    pub signature: Vec<u8>,
}

/// Result of proposal execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionResult {
    /// Execution succeeded
    Success,
    /// Execution failed with error
    Failed { error: String },
    /// Partial execution (some steps completed)
    Partial {
        completed: Vec<String>,
        failed: Vec<String>,
    },
}

/// On-chain voting power delegation transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceDelegateTx {
    /// Delegator (giving their voting power)
    pub delegator: UserId,
    /// Delegate (receiving voting power)
    pub delegate: UserId,
    /// Amount of voting power to delegate (0 = all)
    pub amount: u64,
    /// Delegation expiry (None = until revoked)
    pub expires_at: Option<DateTime<Utc>>,
    /// Delegation timestamp
    pub delegated_at: DateTime<Utc>,
    /// Delegator's signature
    pub signature: Vec<u8>,
}

/// On-chain delegation revocation transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceRevokeDelegationTx {
    /// Delegator revoking
    pub delegator: UserId,
    /// Delegate being revoked
    pub delegate: UserId,
    /// Revocation timestamp
    pub revoked_at: DateTime<Utc>,
    /// Delegator's signature
    pub signature: Vec<u8>,
}

/// On-chain proposal cancellation transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceCancelTx {
    /// Proposal being cancelled
    pub proposal_id: Uuid,
    /// Who is cancelling (proposer or emergency multisig)
    pub cancelled_by: UserId,
    /// Reason for cancellation
    pub reason: String,
    /// Cancellation timestamp
    pub cancelled_at: DateTime<Utc>,
    /// Canceller's signature
    pub signature: Vec<u8>,
}

/// Wrapper for all governance transaction types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceTransaction {
    SubmitProposal(GovernanceProposalTx),
    CommitVote(GovernanceVoteCommitTx),
    RevealVote(GovernanceVoteRevealTx),
    Finalize(GovernanceFinalizeTx),
    Execute(GovernanceExecuteTx),
    Cancel(GovernanceCancelTx),
    Delegate(GovernanceDelegateTx),
    RevokeDelegation(GovernanceRevokeDelegationTx),
}

impl GovernanceTransaction {
    /// Get the transaction type
    pub fn tx_type(&self) -> GovernanceTransactionType {
        match self {
            GovernanceTransaction::SubmitProposal(_) => GovernanceTransactionType::SubmitProposal,
            GovernanceTransaction::CommitVote(_) => GovernanceTransactionType::CommitVote,
            GovernanceTransaction::RevealVote(_) => GovernanceTransactionType::RevealVote,
            GovernanceTransaction::Finalize(_) => GovernanceTransactionType::FinalizeProposal,
            GovernanceTransaction::Execute(_) => GovernanceTransactionType::ExecuteProposal,
            GovernanceTransaction::Cancel(_) => GovernanceTransactionType::CancelProposal,
            GovernanceTransaction::Delegate(_) => GovernanceTransactionType::DelegateVotingPower,
            GovernanceTransaction::RevokeDelegation(_) => {
                GovernanceTransactionType::RevokeDelegation
            }
        }
    }

    /// Get the proposal ID if applicable
    pub fn proposal_id(&self) -> Option<Uuid> {
        match self {
            GovernanceTransaction::SubmitProposal(tx) => Some(tx.proposal_id),
            GovernanceTransaction::CommitVote(tx) => Some(tx.proposal_id),
            GovernanceTransaction::RevealVote(tx) => Some(tx.proposal_id),
            GovernanceTransaction::Finalize(tx) => Some(tx.proposal_id),
            GovernanceTransaction::Execute(tx) => Some(tx.proposal_id),
            GovernanceTransaction::Cancel(tx) => Some(tx.proposal_id),
            GovernanceTransaction::Delegate(_) => None,
            GovernanceTransaction::RevokeDelegation(_) => None,
        }
    }

    /// Get the signer's user ID
    pub fn signer(&self) -> &UserId {
        match self {
            GovernanceTransaction::SubmitProposal(tx) => &tx.proposer,
            GovernanceTransaction::CommitVote(tx) => &tx.voter,
            GovernanceTransaction::RevealVote(tx) => &tx.voter,
            GovernanceTransaction::Finalize(tx) => &tx.finalizer,
            GovernanceTransaction::Execute(tx) => &tx.executor,
            GovernanceTransaction::Cancel(tx) => &tx.cancelled_by,
            GovernanceTransaction::Delegate(tx) => &tx.delegator,
            GovernanceTransaction::RevokeDelegation(tx) => &tx.delegator,
        }
    }
}

/// Helper to compute vote commitment
pub fn compute_vote_commitment(vote_choice: VoteChoice, salt: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update([vote_choice as u8]);
    hasher.update(salt);
    hasher.finalize().into()
}

/// Verify a vote reveal against its commitment
pub fn verify_vote_reveal(commitment: &[u8; 32], vote_choice: u8, salt: &[u8; 32]) -> bool {
    let Some(choice) = VoteChoice::from_u8(vote_choice) else {
        return false;
    };
    let computed = compute_vote_commitment(choice, salt);
    &computed == commitment
}

/// Governance state snapshot for merkle tree computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceStateSnapshot {
    /// Block height this snapshot is for
    pub block_height: u64,
    /// Active proposals (proposal_id -> proposal hash)
    pub active_proposals: Vec<(Uuid, [u8; 32])>,
    /// Finalized proposals in this block (proposal_id -> outcome hash)
    pub finalized_proposals: Vec<(Uuid, [u8; 32])>,
    /// Vote commitments included in this block
    pub vote_commitments: Vec<[u8; 32]>,
    /// Vote reveals included in this block
    pub vote_reveals: Vec<[u8; 32]>,
    /// Active delegations count
    pub delegation_count: u64,
}

impl GovernanceStateSnapshot {
    /// Compute the merkle root of the governance state
    ///
    /// Structure:
    /// - Root
    ///   ├── Active proposals subtree
    ///   ├── Finalized proposals subtree
    ///   ├── Vote commitments subtree
    ///   ├── Vote reveals subtree
    ///   └── Metadata (delegation count, block height)
    pub fn compute_merkle_root(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};

        // Compute subtree roots
        let active_root = Self::compute_subtree_root(&self.active_proposals);
        let finalized_root = Self::compute_subtree_root(&self.finalized_proposals);
        let commits_root = Self::compute_leaves_root(&self.vote_commitments);
        let reveals_root = Self::compute_leaves_root(&self.vote_reveals);

        // Metadata hash
        let mut metadata_hasher = Sha256::new();
        metadata_hasher.update(b"governance/metadata/v1");
        metadata_hasher.update(self.block_height.to_le_bytes());
        metadata_hasher.update(self.delegation_count.to_le_bytes());
        let metadata_hash: [u8; 32] = metadata_hasher.finalize().into();

        // Combine into final root
        let mut root_hasher = Sha256::new();
        root_hasher.update(b"governance/root/v1");
        root_hasher.update(active_root);
        root_hasher.update(finalized_root);
        root_hasher.update(commits_root);
        root_hasher.update(reveals_root);
        root_hasher.update(metadata_hash);
        root_hasher.finalize().into()
    }

    fn compute_subtree_root(items: &[(Uuid, [u8; 32])]) -> [u8; 32] {
        use sha2::{Digest, Sha256};

        if items.is_empty() {
            return [0u8; 32];
        }

        // Hash each (uuid, hash) pair
        let leaves: Vec<[u8; 32]> = items
            .iter()
            .map(|(id, hash)| {
                let mut hasher = Sha256::new();
                hasher.update(id.as_bytes());
                hasher.update(hash);
                hasher.finalize().into()
            })
            .collect();

        Self::compute_leaves_root(&leaves)
    }

    fn compute_leaves_root(leaves: &[[u8; 32]]) -> [u8; 32] {
        use sha2::{Digest, Sha256};

        if leaves.is_empty() {
            return [0u8; 32];
        }

        if leaves.len() == 1 {
            return leaves[0];
        }

        // Build binary merkle tree
        let mut current_level = leaves.to_vec();

        while current_level.len() > 1 {
            let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

            for chunk in current_level.chunks(2) {
                let mut hasher = Sha256::new();
                hasher.update(chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(chunk[1]);
                } else {
                    // Duplicate last element for odd-length levels
                    hasher.update(chunk[0]);
                }
                next_level.push(hasher.finalize().into());
            }

            current_level = next_level;
        }

        current_level[0]
    }
}

/// Hash a governance finalization for inclusion in the merkle tree
pub fn hash_finalization(finalize_tx: &GovernanceFinalizeTx) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"governance/finalize/v1");
    hasher.update(finalize_tx.proposal_id.as_bytes());
    hasher.update(finalize_tx.total_votes_for.to_le_bytes());
    hasher.update(finalize_tx.total_votes_against.to_le_bytes());
    hasher.update(finalize_tx.total_votes_abstain.to_le_bytes());
    hasher.update([finalize_tx.outcome as u8]);
    hasher.update(finalize_tx.votes_merkle_root);
    hasher.finalize().into()
}

/// Hash a governance proposal for inclusion in the merkle tree
pub fn hash_proposal(proposal_tx: &GovernanceProposalTx) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let serialized = serde_json::to_vec(proposal_tx).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(b"governance/proposal/v1");
    hasher.update(&serialized);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_commitment() {
        let salt = [42u8; 32];
        let commitment = compute_vote_commitment(VoteChoice::For, &salt);

        assert!(verify_vote_reveal(&commitment, 1, &salt)); // For = 1
        assert!(!verify_vote_reveal(&commitment, 0, &salt)); // Against = 0
        assert!(!verify_vote_reveal(&commitment, 2, &salt)); // Abstain = 2

        let wrong_salt = [0u8; 32];
        assert!(!verify_vote_reveal(&commitment, 1, &wrong_salt));
    }

    #[test]
    fn test_vote_choice_conversion() {
        assert_eq!(VoteChoice::from_u8(0), Some(VoteChoice::Against));
        assert_eq!(VoteChoice::from_u8(1), Some(VoteChoice::For));
        assert_eq!(VoteChoice::from_u8(2), Some(VoteChoice::Abstain));
        assert_eq!(VoteChoice::from_u8(3), None);
        assert_eq!(VoteChoice::from_u8(255), None);
    }

    #[test]
    fn test_governance_transaction_types() {
        let proposal_id = Uuid::new_v4();
        let user = UserId::new();

        let tx = GovernanceTransaction::CommitVote(GovernanceVoteCommitTx {
            proposal_id,
            voter: user.clone(),
            commitment: [0u8; 32],
            voting_power: 1000,
            snapshot_height: 100,
            committed_at: Utc::now(),
            signature: vec![],
        });

        assert_eq!(tx.tx_type(), GovernanceTransactionType::CommitVote);
        assert_eq!(tx.proposal_id(), Some(proposal_id));
        assert_eq!(tx.signer(), &user);
    }

    #[test]
    fn test_proposal_outcome_serialization() {
        let outcome = ProposalOutcome::Passed;
        let serialized = serde_json::to_string(&outcome).unwrap();
        let deserialized: ProposalOutcome = serde_json::from_str(&serialized).unwrap();
        assert_eq!(outcome, deserialized);
    }

    #[test]
    fn test_governance_state_snapshot_merkle_root() {
        let snapshot = GovernanceStateSnapshot {
            block_height: 100,
            active_proposals: vec![(Uuid::new_v4(), [1u8; 32]), (Uuid::new_v4(), [2u8; 32])],
            finalized_proposals: vec![(Uuid::new_v4(), [3u8; 32])],
            vote_commitments: vec![[4u8; 32], [5u8; 32]],
            vote_reveals: vec![[6u8; 32]],
            delegation_count: 10,
        };

        let root1 = snapshot.compute_merkle_root();
        let root2 = snapshot.compute_merkle_root();

        // Same snapshot should produce same root (deterministic)
        assert_eq!(root1, root2);

        // Different snapshot should produce different root
        let snapshot2 = GovernanceStateSnapshot {
            block_height: 101, // Different height
            ..snapshot.clone()
        };
        let root3 = snapshot2.compute_merkle_root();
        assert_ne!(root1, root3);
    }

    #[test]
    fn test_empty_governance_snapshot() {
        let snapshot = GovernanceStateSnapshot {
            block_height: 0,
            active_proposals: vec![],
            finalized_proposals: vec![],
            vote_commitments: vec![],
            vote_reveals: vec![],
            delegation_count: 0,
        };

        // Should not panic and produce a valid root
        let root = snapshot.compute_merkle_root();
        assert_ne!(root, [0u8; 32]); // Should not be all zeros (has metadata)
    }
}
