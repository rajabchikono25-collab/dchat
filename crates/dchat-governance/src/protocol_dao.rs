//! Protocol-level DAO Governance
//!
//! This module provides governance mechanisms for the dchat protocol itself,
//! including:
//! - Protocol parameter adjustments (consensus, economic, network)
//! - Protocol upgrades and soft/hard forks
//! - Treasury management and fund allocation
//! - Emergency actions and circuit breakers
//! - Proposal lifecycle (submission → voting → execution)
//! - Quadratic voting and delegation

use chrono::{DateTime, Duration, Utc};
use dchat_core::{types::UserId, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Protocol DAO proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolProposal {
    pub id: Uuid,
    pub proposer: UserId,
    pub proposal_type: ProposalType,
    pub title: String,
    pub description: String,
    pub rationale: String,
    
    /// Current status
    pub status: ProposalStatus,
    
    /// Voting information
    pub voting: VotingInfo,
    
    /// Required quorum percentage (0-100)
    pub quorum_required: u8,
    
    /// Required approval percentage (0-100)
    pub approval_required: u8,
    
    /// Execution details
    pub execution: Option<ExecutionInfo>,
    
    /// Timestamps
    pub created_at: DateTime<Utc>,
    pub voting_starts_at: DateTime<Utc>,
    pub voting_ends_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
}

/// Types of protocol proposals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalType {
    /// Change protocol parameters
    ParameterChange {
        parameter: ProtocolParameter,
        current_value: String,
        proposed_value: String,
    },
    
    /// Protocol upgrade
    ProtocolUpgrade {
        version: String,
        upgrade_hash: String,
        is_hard_fork: bool,
    },
    
    /// Treasury allocation
    TreasuryAllocation {
        amount: u64,
        recipient: UserId,
        purpose: String,
    },
    
    /// Emergency action
    EmergencyAction {
        action_type: EmergencyActionType,
        justification: String,
    },
    
    /// Feature toggle
    FeatureToggle {
        feature_name: String,
        enable: bool,
    },
    
    /// Grant program
    GrantProgram {
        program_name: String,
        total_budget: u64,
        duration_days: u32,
    },
}

/// Protocol parameters that can be adjusted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolParameter {
    // Consensus parameters
    BlockTime,
    ValidatorCount,
    MinimumStake,
    SlashingRate,
    
    // Economic parameters
    TransactionFee,
    MessageFee,
    RelayReward,
    StakingYield,
    
    // Network parameters
    MaxPeers,
    MessageTTL,
    MaxMessageSize,
    RateLimitPerUser,
    
    // Governance parameters
    ProposalDeposit,
    VotingPeriod,
    MinimumQuorum,
    ApprovalThreshold,
}

/// Emergency action types
##[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmergencyActionType {
    /// Pause protocol operations
    PauseProtocol,
    
    /// Resume protocol operations
    ResumeProtocol,
    
    /// Activate circuit breaker for specific module
    CircuitBreaker { module: String },
    
    /// Emergency parameter override
    EmergencyOverride { parameter: String, value: String },
    
    /// Force protocol upgrade
    ForceUpgrade { version: String },
}

/// Proposal status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalStatus {
    /// Proposal submitted, waiting for voting period
    Pending,
    
    /// Currently accepting votes
    Active,
    
    /// Voting completed, passed
    Passed,
    
    /// Voting completed, rejected
    Rejected,
    
    /// Executed successfully
    Executed,
    
    /// Execution failed
    ExecutionFailed { reason: String },
    
    /// Cancelled by proposer or emergency action
    Cancelled,
}

/// Voting information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VotingInfo {
    pub total_votes_for: u64,
    pub total_votes_against: u64,
    pub total_votes_abstain: u64,
    pub total_voting_power: u64,
    pub votes: HashMap<UserId, Vote>,
    pub use_quadratic_voting: bool,
}

/// Individual vote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub voter: UserId,
    pub vote_type: VoteType,
    pub voting_power: u64,
    pub quadratic_weight: f64,
    pub delegated_from: Option<UserId>,
    pub timestamp: DateTime<Utc>,
}

/// Vote types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VoteType {
    For,
    Against,
    Abstain,
}

/// Execution information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInfo {
    pub executor: Option<UserId>,
    pub execution_tx_hash: String,
    pub execution_result: ExecutionResult,
    pub executed_at: DateTime<Utc>,
}

/// Execution results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionResult {
    Success,
    Failed { error: String },
    PartialSuccess { completed: Vec<String>, failed: Vec<String> },
}

/// Vote delegation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteDelegation {
    pub delegator: UserId,
    pub delegate: UserId,
    pub delegated_power: u64,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Treasury information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolTreasury {
    pub total_balance: u64,
    pub reserved_funds: u64,
    pub available_funds: u64,
    pub allocations: Vec<TreasuryAllocation>,
}

/// Treasury allocation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreasuryAllocation {
    pub id: Uuid,
    pub proposal_id: Uuid,
    pub amount: u64,
    pub recipient: UserId,
    pub purpose: String,
    pub allocated_at: DateTime<Utc>,
    pub disbursed: bool,
    pub disbursed_at: Option<DateTime<Utc>>,
}

/// Protocol DAO manager
pub struct ProtocolDaoManager {
    proposals: HashMap<Uuid, ProtocolProposal>,
    delegations: HashMap<UserId, VoteDelegation>,
    treasury: ProtocolTreasury,
    voting_power: HashMap<UserId, u64>,
    emergency_multisig: Vec<UserId>,
}

impl ProtocolDaoManager {
    pub fn new(emergency_multisig: Vec<UserId>) -> Self {
        Self {
            proposals: HashMap::new(),
            delegations: HashMap::new(),
            treasury: ProtocolTreasury {
                total_balance: 0,
                reserved_funds: 0,
                available_funds: 0,
                allocations: Vec::new(),
            },
            voting_power: HashMap::new(),
            emergency_multisig,
        }
    }

    /// Submit a protocol proposal
    pub fn submit_proposal(
        &mut self,
        proposer: UserId,
        proposal_type: ProposalType,
        title: String,
        description: String,
        rationale: String,
        voting_duration_days: u32,
    ) -> Result<Uuid> {
        // Verify proposer has minimum stake/reputation
        let proposer_power = self.voting_power.get(&proposer).copied().unwrap_or(0);
        if proposer_power < 1000 {
            return Err(Error::validation("Insufficient voting power to propose"));
        }

        // Set quorum and approval based on proposal type
        let (quorum_required, approval_required) = match &proposal_type {
            ProposalType::EmergencyAction { .. } => (75, 90),
            ProposalType::ProtocolUpgrade { is_hard_fork: true, .. } => (80, 85),
            ProposalType::ProtocolUpgrade { is_hard_fork: false, .. } => (60, 75),
            ProposalType::ParameterChange { .. } => (50, 66),
            ProposalType::TreasuryAllocation { .. } => (55, 70),
            ProposalType::FeatureToggle { .. } => (45, 60),
            ProposalType::GrantProgram { .. } => (50, 65),
        };

        let now = Utc::now();
        let voting_starts = now + Duration::days(2); // 2 day review period
        let voting_ends = voting_starts + Duration::days(voting_duration_days as i64);

        let proposal = ProtocolProposal {
            id: Uuid::new_v4(),
            proposer,
            proposal_type,
            title,
            description,
            rationale,
            status: ProposalStatus::Pending,
            voting: VotingInfo {
                total_votes_for: 0,
                total_votes_against: 0,
                total_votes_abstain: 0,
                total_voting_power: 0,
                votes: HashMap::new(),
                use_quadratic_voting: true, // Enable by default
            },
            quorum_required,
            approval_required,
            execution: None,
            created_at: now,
            voting_starts_at: voting_starts,
            voting_ends_at: voting_ends,
            executed_at: None,
        };

        let proposal_id = proposal.id;
        self.proposals.insert(proposal_id, proposal);

        Ok(proposal_id)
    }

    /// Cast a vote on a proposal
    pub fn cast_vote(
        &mut self,
        proposal_id: Uuid,
        voter: UserId,
        vote_type: VoteType,
    ) -> Result<()> {
        let proposal = self.proposals.get_mut(&proposal_id)
            .ok_or_else(|| Error::validation("Proposal not found"))?;

        // Check if voting is active
        let now = Utc::now();
        if now < proposal.voting_starts_at {
            return Err(Error::validation("Voting has not started yet"));
        }
        if now > proposal.voting_ends_at {
            return Err(Error::validation("Voting has ended"));
        }
        if proposal.status != ProposalStatus::Active && proposal.status != ProposalStatus::Pending {
            return Err(Error::validation("Proposal is not active"));
        }

        // Update status to active if pending
        if proposal.status == ProposalStatus::Pending && now >= proposal.voting_starts_at {
            proposal.status = ProposalStatus::Active;
        }

        // Get voter's power (including delegations)
        let voting_power = self.get_effective_voting_power(&voter);
        if voting_power == 0 {
            return Err(Error::validation("No voting power"));
        }

        // Calculate quadratic weight if enabled
        let quadratic_weight = if proposal.voting.use_quadratic_voting {
            (voting_power as f64).sqrt()
        } else {
            voting_power as f64
        };

        // Check for existing vote and remove it
        if let Some(old_vote) = proposal.voting.votes.remove(&voter) {
            match old_vote.vote_type {
                VoteType::For => proposal.voting.total_votes_for -= old_vote.quadratic_weight as u64,
                VoteType::Against => proposal.voting.total_votes_against -= old_vote.quadratic_weight as u64,
                VoteType::Abstain => proposal.voting.total_votes_abstain -= old_vote.quadratic_weight as u64,
            }
        } else {
            proposal.voting.total_voting_power += voting_power;
        }

        // Add new vote
        let vote = Vote {
            voter: voter.clone(),
            vote_type: vote_type.clone(),
            voting_power,
            quadratic_weight,
            delegated_from: None,
            timestamp: Utc::now(),
        };

        match vote_type {
            VoteType::For => proposal.voting.total_votes_for += quadratic_weight as u64,
            VoteType::Against => proposal.voting.total_votes_against += quadratic_weight as u64,
            VoteType::Abstain => proposal.voting.total_votes_abstain += quadratic_weight as u64,
        }

        proposal.voting.votes.insert(voter, vote);

        Ok(())
    }

    /// Delegate voting power to another user
    pub fn delegate_voting_power(
        &mut self,
        delegator: UserId,
        delegate: UserId,
        expires_in_days: Option<u32>,
    ) -> Result<()> {
        if delegator == delegate {
            return Err(Error::validation("Cannot delegate to self"));
        }

        let delegated_power = self.voting_power.get(&delegator).copied().unwrap_or(0);
        if delegated_power == 0 {
            return Err(Error::validation("No voting power to delegate"));
        }

        let expires_at = expires_in_days.map(|days| Utc::now() + Duration::days(days as i64));

        let delegation = VoteDelegation {
            delegator: delegator.clone(),
            delegate,
            delegated_power,
            created_at: Utc::now(),
            expires_at,
        };

        self.delegations.insert(delegator, delegation);

        Ok(())
    }

    /// Revoke voting delegation
    pub fn revoke_delegation(&mut self, delegator: UserId) -> Result<()> {
        self.delegations.remove(&delegator)
            .ok_or_else(|| Error::validation("No active delegation"))?;
        Ok(())
    }

    /// Finalize proposal after voting ends
    pub fn finalize_proposal(&mut self, proposal_id: Uuid) -> Result<ProposalStatus> {
        let proposal = self.proposals.get_mut(&proposal_id)
            .ok_or_else(|| Error::validation("Proposal not found"))?;

        if Utc::now() < proposal.voting_ends_at {
            return Err(Error::validation("Voting period has not ended"));
        }

        if proposal.status != ProposalStatus::Active {
            return Err(Error::validation("Proposal is not active"));
        }

        // Calculate total eligible voting power (all stakers)
        let total_eligible_power: u64 = self.voting_power.values().sum();
        if total_eligible_power == 0 {
            return Err(Error::validation("No eligible voters"));
        }

        // Check quorum
        let participation_rate = (proposal.voting.total_voting_power * 100) / total_eligible_power;
        if participation_rate < proposal.quorum_required as u64 {
            proposal.status = ProposalStatus::Rejected;
            return Ok(ProposalStatus::Rejected);
        }

        // Check approval
        let total_decisive_votes = proposal.voting.total_votes_for + proposal.voting.total_votes_against;
        if total_decisive_votes == 0 {
            proposal.status = ProposalStatus::Rejected;
            return Ok(ProposalStatus::Rejected);
        }

        let approval_rate = (proposal.voting.total_votes_for * 100) / total_decisive_votes;
        if approval_rate >= proposal.approval_required as u64 {
            proposal.status = ProposalStatus::Passed;
            Ok(ProposalStatus::Passed)
        } else {
            proposal.status = ProposalStatus::Rejected;
            Ok(ProposalStatus::Rejected)
        }
    }

    /// Execute a passed proposal
    pub fn execute_proposal(
        &mut self,
        proposal_id: Uuid,
        executor: UserId,
    ) -> Result<ExecutionResult> {
        let proposal = self.proposals.get_mut(&proposal_id)
            .ok_or_else(|| Error::validation("Proposal not found"))?;

        if proposal.status != ProposalStatus::Passed {
            return Err(Error::validation("Proposal has not passed"));
        }

        // Execute based on proposal type
        let result = match &proposal.proposal_type {
            ProposalType::ParameterChange { parameter, proposed_value, .. } => {
                self.execute_parameter_change(parameter.clone(), proposed_value.clone())?
            },
            ProposalType::TreasuryAllocation { amount, recipient, purpose } => {
                self.execute_treasury_allocation(proposal_id, *amount, recipient.clone(), purpose.clone())?
            },
            ProposalType::ProtocolUpgrade { version, upgrade_hash, .. } => {
                self.execute_protocol_upgrade(version.clone(), upgrade_hash.clone())?
            },
            ProposalType::EmergencyAction { action_type, .. } => {
                self.execute_emergency_action(action_type.clone())?
            },
            ProposalType::FeatureToggle { feature_name, enable } => {
                self.execute_feature_toggle(feature_name.clone(), *enable)?
            },
            ProposalType::GrantProgram { program_name, total_budget, duration_days } => {
                self.execute_grant_program(program_name.clone(), *total_budget, *duration_days)?
            },
        };

        proposal.status = ProposalStatus::Executed;
        proposal.executed_at = Some(Utc::now());
        proposal.execution = Some(ExecutionInfo {
            executor: Some(executor),
            execution_tx_hash: format!("exec_{}", Uuid::new_v4()),
            execution_result: result.clone(),
            executed_at: Utc::now(),
        });

        Ok(result)
    }

    /// Execute parameter change
    fn execute_parameter_change(&mut self, _parameter: ProtocolParameter, _value: String) -> Result<ExecutionResult> {
        // In production, this would update the actual protocol parameter
        // via chain state transition
        Ok(ExecutionResult::Success)
    }

    /// Execute treasury allocation
    fn execute_treasury_allocation(
        &mut self,
        proposal_id: Uuid,
        amount: u64,
        recipient: UserId,
        purpose: String,
    ) -> Result<ExecutionResult> {
        if amount > self.treasury.available_funds {
            return Ok(ExecutionResult::Failed {
                error: "Insufficient treasury funds".to_string(),
            });
        }

        let allocation = TreasuryAllocation {
            id: Uuid::new_v4(),
            proposal_id,
            amount,
            recipient,
            purpose,
            allocated_at: Utc::now(),
            disbursed: false,
            disbursed_at: None,
        };

        self.treasury.available_funds -= amount;
        self.treasury.reserved_funds += amount;
        self.treasury.allocations.push(allocation);

        Ok(ExecutionResult::Success)
    }

    /// Execute protocol upgrade
    fn execute_protocol_upgrade(&mut self, version: String, _upgrade_hash: String) -> Result<ExecutionResult> {
        // In production, this would trigger the upgrade process
        Ok(ExecutionResult::Success)
    }

    /// Execute emergency action
    fn execute_emergency_action(&mut self, _action_type: EmergencyActionType) -> Result<ExecutionResult> {
        // In production, this would execute the emergency action
        Ok(ExecutionResult::Success)
    }

    /// Execute feature toggle
    fn execute_feature_toggle(&mut self, _feature_name: String, _enable: bool) -> Result<ExecutionResult> {
        // In production, this would toggle the feature flag
        Ok(ExecutionResult::Success)
    }

    /// Execute grant program
    fn execute_grant_program(&mut self, _program_name: String, total_budget: u64, _duration_days: u32) -> Result<ExecutionResult> {
        if total_budget > self.treasury.available_funds {
            return Ok(ExecutionResult::Failed {
                error: "Insufficient treasury funds for grant program".to_string(),
            });
        }

        self.treasury.available_funds -= total_budget;
        self.treasury.reserved_funds += total_budget;

        Ok(ExecutionResult::Success)
    }

    /// Get effective voting power including delegations
    fn get_effective_voting_power(&self, user: &UserId) -> u64 {
        let own_power = self.voting_power.get(user).copied().unwrap_or(0);
        
        // Add delegated power
        let delegated_power: u64 = self.delegations.values()
            .filter(|d| &d.delegate == user)
            .filter(|d| d.expires_at.map_or(true, |exp| Utc::now() < exp))
            .map(|d| d.delegated_power)
            .sum();

        own_power + delegated_power
    }

    /// Set voting power for user (called from staking module)
    pub fn set_voting_power(&mut self, user: UserId, power: u64) {
        self.voting_power.insert(user, power);
    }

    /// Add funds to treasury
    pub fn add_treasury_funds(&mut self, amount: u64) {
        self.treasury.total_balance += amount;
        self.treasury.available_funds += amount;
    }

    /// Get proposal
    pub fn get_proposal(&self, proposal_id: Uuid) -> Option<&ProtocolProposal> {
        self.proposals.get(&proposal_id)
    }

    /// Get all active proposals
    pub fn get_active_proposals(&self) -> Vec<&ProtocolProposal> {
        self.proposals.values()
            .filter(|p| p.status == ProposalStatus::Active || p.status == ProposalStatus::Pending)
            .collect()
    }

    /// Get treasury status
    pub fn get_treasury(&self) -> &ProtocolTreasury {
        &self.treasury
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user() -> UserId {
        UserId::new()
    }

    #[test]
    fn test_submit_proposal() {
        let mut dao = ProtocolDaoManager::new(vec![]);
        let proposer = create_test_user();
        
        // Set voting power
        dao.set_voting_power(proposer.clone(), 2000);

        let proposal_id = dao.submit_proposal(
            proposer,
            ProposalType::ParameterChange {
                parameter: ProtocolParameter::BlockTime,
                current_value: "6s".to_string(),
                proposed_value: "5s".to_string(),
            },
            "Reduce Block Time".to_string(),
            "Proposal to reduce block time for faster finality".to_string(),
            "Benchmarks show 5s is safe".to_string(),
            7,
        ).unwrap();

        assert!(dao.get_proposal(proposal_id).is_some());
    }

    #[test]
    fn test_voting() {
        let mut dao = ProtocolDaoManager::new(vec![]);
        let proposer = create_test_user();
        let voter1 = create_test_user();
        let voter2 = create_test_user();

        dao.set_voting_power(proposer.clone(), 2000);
        dao.set_voting_power(voter1.clone(), 1000);
        dao.set_voting_power(voter2.clone(), 1500);

        let mut proposal_id = dao.submit_proposal(
            proposer,
            ProposalType::ParameterChange {
                parameter: ProtocolParameter::TransactionFee,
                current_value: "100".to_string(),
                proposed_value: "50".to_string(),
            },
            "Lower Fees".to_string(),
            "Make the network more accessible".to_string(),
            "Current fees are too high".to_string(),
            7,
        ).unwrap();

        // Activate proposal manually for testing
        let proposal = dao.proposals.get_mut(&proposal_id).unwrap();
        proposal.voting_starts_at = Utc::now() - Duration::hours(1);
        proposal.status = ProposalStatus::Active;

        // Cast votes
        dao.cast_vote(proposal_id, voter1, VoteType::For).unwrap();
        dao.cast_vote(proposal_id, voter2, VoteType::Against).unwrap();

        let proposal = dao.get_proposal(proposal_id).unwrap();
        assert!(proposal.voting.total_votes_for > 0);
        assert!(proposal.voting.total_votes_against > 0);
    }

    #[test]
    fn test_delegation() {
        let mut dao = ProtocolDaoManager::new(vec![]);
        let delegator = create_test_user();
        let delegate = create_test_user();

        dao.set_voting_power(delegator.clone(), 1000);

        dao.delegate_voting_power(delegator.clone(), delegate.clone(), Some(30)).unwrap();

        let effective_power = dao.get_effective_voting_power(&delegate);
        assert_eq!(effective_power, 1000);

        dao.revoke_delegation(delegator).unwrap();
        let effective_power_after = dao.get_effective_voting_power(&delegate);
        assert_eq!(effective_power_after, 0);
    }

    #[test]
    fn test_treasury_allocation() {
        let mut dao = ProtocolDaoManager::new(vec![]);
        dao.add_treasury_funds(10000);

        let proposer = create_test_user();
        let recipient = create_test_user();
        
        dao.set_voting_power(proposer.clone(), 2000);

        let proposal_id = dao.submit_proposal(
            proposer.clone(),
            ProposalType::TreasuryAllocation {
                amount: 1000,
                recipient: recipient.clone(),
                purpose: "Development grant".to_string(),
            },
            "Dev Grant".to_string(),
            "Fund development work".to_string(),
            "Need more developers".to_string(),
            7,
        ).unwrap();

        // Skip to passed status
        let proposal = dao.proposals.get_mut(&proposal_id).unwrap();
        proposal.status = ProposalStatus::Passed;

        let result = dao.execute_proposal(proposal_id, proposer).unwrap();
        assert!(matches!(result, ExecutionResult::Success));
        assert_eq!(dao.treasury.available_funds, 9000);
    }
}
