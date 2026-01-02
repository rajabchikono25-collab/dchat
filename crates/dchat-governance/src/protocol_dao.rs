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
use std::sync::Arc;
use uuid::Uuid;

/// Chain client interface for executing governance actions on-chain
#[async_trait::async_trait]
pub trait GovernanceChainClient: Send + Sync {
    /// Submit a parameter change transaction
    async fn submit_parameter_change(
        &self,
        parameter: &str,
        old_value: &str,
        new_value: &str,
        proposal_id: &str,
    ) -> Result<GovernanceTxReceipt>;

    /// Submit a treasury transfer transaction
    async fn submit_treasury_transfer(
        &self,
        recipient: &[u8],
        amount: u64,
        purpose: &str,
        proposal_id: &str,
    ) -> Result<GovernanceTxReceipt>;

    /// Submit a protocol upgrade activation
    async fn submit_protocol_upgrade(
        &self,
        version: &str,
        upgrade_hash: &str,
        activation_block: u64,
        is_hard_fork: bool,
    ) -> Result<GovernanceTxReceipt>;

    /// Submit an emergency action (pause/resume/circuit breaker)
    async fn submit_emergency_action(
        &self,
        action_type: &str,
        parameters: &HashMap<String, String>,
    ) -> Result<GovernanceTxReceipt>;

    /// Submit a feature toggle
    async fn submit_feature_toggle(
        &self,
        feature_name: &str,
        enable: bool,
    ) -> Result<GovernanceTxReceipt>;

    /// Get current protocol parameters
    async fn get_protocol_parameter(&self, parameter: &str) -> Result<String>;

    /// Get current block height
    async fn get_current_block(&self) -> Result<u64>;

    /// Wait for transaction confirmation
    async fn wait_for_confirmation(&self, tx_hash: &str, confirmations: u32) -> Result<bool>;
}

/// Receipt from a governance transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceTxReceipt {
    pub tx_hash: String,
    pub block_height: u64,
    pub block_hash: String,
    pub timestamp: i64,
    pub success: bool,
    pub gas_used: u64,
    pub logs: Vec<GovernanceEventLog>,
}

/// Event log from governance transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceEventLog {
    pub event_type: String,
    pub data: HashMap<String, String>,
}

/// Protocol state snapshot for upgrades
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolStateSnapshot {
    pub block_height: u64,
    pub parameters: HashMap<String, String>,
    pub feature_flags: HashMap<String, bool>,
    pub snapshot_hash: String,
}

/// Grant program state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantProgramState {
    pub program_id: Uuid,
    pub program_name: String,
    pub total_budget: u64,
    pub remaining_budget: u64,
    pub start_block: u64,
    pub end_block: u64,
    pub is_active: bool,
    pub disbursements: Vec<GrantDisbursement>,
}

/// Individual grant disbursement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantDisbursement {
    pub recipient: UserId,
    pub amount: u64,
    pub purpose: String,
    pub tx_hash: String,
    pub disbursed_at: DateTime<Utc>,
}

/// Execution audit record for governance actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAuditRecord {
    pub proposal_id: Uuid,
    pub action_type: String,
    pub executor: UserId,
    pub tx_hash: String,
    pub block_height: u64,
    pub executed_at: DateTime<Utc>,
    pub pre_state_hash: String,
    pub post_state_hash: String,
    pub success: bool,
    pub details: HashMap<String, String>,
}

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

    /// Required quorum in basis points (0-10000, where 10000 = 100%)
    pub quorum_required_bps: u16,

    /// Required approval in basis points (0-10000, where 5001 = simple majority)
    pub approval_required_bps: u16,

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
    FeatureToggle { feature_name: String, enable: bool },

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Failed {
        error: String,
    },
    PartialSuccess {
        completed: Vec<String>,
        failed: Vec<String>,
    },
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
    pub tx_hash: Option<String>,
}

/// Protocol DAO manager
pub struct ProtocolDaoManager {
    proposals: HashMap<Uuid, ProtocolProposal>,
    delegations: HashMap<UserId, VoteDelegation>,
    treasury: ProtocolTreasury,
    voting_power: HashMap<UserId, u64>,
    emergency_multisig: Vec<UserId>,
    /// Chain client for submitting governance transactions
    chain_client: Option<Arc<dyn GovernanceChainClient>>,
    /// Active grant programs
    grant_programs: HashMap<Uuid, GrantProgramState>,
    /// Protocol parameters (local cache)
    protocol_parameters: HashMap<String, String>,
    /// Feature flags (local cache)
    feature_flags: HashMap<String, bool>,
    /// Execution audit trail
    audit_records: Vec<ExecutionAuditRecord>,
    /// Protocol paused state
    is_protocol_paused: bool,
    /// Active circuit breakers
    circuit_breakers: HashMap<String, bool>,
    /// Metrics collector
    metrics: Option<Arc<dchat_observability::MetricsCollector>>,
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
            chain_client: None,
            grant_programs: HashMap::new(),
            protocol_parameters: HashMap::new(),
            feature_flags: HashMap::new(),
            audit_records: Vec::new(),
            is_protocol_paused: false,
            circuit_breakers: HashMap::new(),
            metrics: None,
        }
    }

    /// Set chain client for on-chain governance transactions
    pub fn with_chain_client(mut self, client: Arc<dyn GovernanceChainClient>) -> Self {
        self.chain_client = Some(client);
        self
    }

    /// Set metrics collector
    pub fn with_metrics(mut self, metrics: Arc<dchat_observability::MetricsCollector>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Get execution audit records
    pub fn get_audit_records(&self) -> &[ExecutionAuditRecord] {
        &self.audit_records
    }

    /// Get grant program by ID
    pub fn get_grant_program(&self, program_id: Uuid) -> Option<&GrantProgramState> {
        self.grant_programs.get(&program_id)
    }

    /// Get all active grant programs
    pub fn get_active_grant_programs(&self) -> Vec<&GrantProgramState> {
        self.grant_programs
            .values()
            .filter(|p| p.is_active)
            .collect()
    }

    /// Check if protocol is paused
    pub fn is_paused(&self) -> bool {
        self.is_protocol_paused
    }

    /// Check if a module has circuit breaker active
    pub fn is_circuit_breaker_active(&self, module: &str) -> bool {
        *self.circuit_breakers.get(module).unwrap_or(&false)
    }

    /// Check if user is an emergency multisig signer
    pub fn is_emergency_signer(&self, user: &UserId) -> bool {
        self.emergency_multisig.contains(user)
    }

    /// Get emergency multisig signers
    pub fn get_emergency_signers(&self) -> &[UserId] {
        &self.emergency_multisig
    }

    /// Get number of required emergency signatures (majority)
    pub fn emergency_threshold(&self) -> usize {
        (self.emergency_multisig.len() / 2) + 1
    }

    /// Execute emergency action bypassing normal governance
    /// Requires majority of emergency multisig signers
    pub async fn execute_emergency_bypass(
        &mut self,
        action_type: EmergencyActionType,
        justification: &str,
        signers: &[UserId],
    ) -> Result<ExecutionResult> {
        // Verify all signers are in emergency multisig
        for signer in signers {
            if !self.is_emergency_signer(signer) {
                return Err(Error::validation(format!(
                    "User {:?} is not an emergency multisig signer",
                    signer
                )));
            }
        }

        // Verify threshold met
        let threshold = self.emergency_threshold();
        if signers.len() < threshold {
            return Err(Error::validation(format!(
                "Emergency action requires {} signers, got {}",
                threshold,
                signers.len()
            )));
        }

        // Deduplicate signers
        let unique_signers: std::collections::HashSet<_> = signers.iter().collect();
        if unique_signers.len() < threshold {
            return Err(Error::validation("Duplicate signers detected"));
        }

        tracing::warn!(
            "⚠️ EMERGENCY BYPASS: {} signers executing {:?}",
            signers.len(),
            action_type
        );

        // Execute the emergency action immediately
        let (result, tx_hash) = self
            .execute_emergency_action_production(action_type.clone(), justification)
            .await?;

        // Create audit record for emergency bypass
        let audit_record = ExecutionAuditRecord {
            proposal_id: Uuid::nil(), // No proposal for emergency bypass
            action_type: format!("EmergencyBypass_{:?}", action_type),
            executor: signers.first().cloned().unwrap_or_else(UserId::new),
            tx_hash,
            block_height: self.get_current_block().await.unwrap_or(0),
            executed_at: Utc::now(),
            pre_state_hash: String::new(),
            post_state_hash: self.compute_state_hash(),
            success: matches!(result, ExecutionResult::Success),
            details: {
                let mut details = HashMap::new();
                details.insert("signers_count".to_string(), signers.len().to_string());
                details.insert("justification".to_string(), justification.to_string());
                details
            },
        };

        self.audit_records.push(audit_record);

        Ok(result)
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

        // Set quorum and approval based on proposal type (in basis points)
        let (quorum_required_bps, approval_required_bps): (u16, u16) = match &proposal_type {
            ProposalType::EmergencyAction { .. } => (7500, 9000), // 75% quorum, 90% approval
            ProposalType::ProtocolUpgrade {
                is_hard_fork: true, ..
            } => (8000, 8500), // 80%, 85%
            ProposalType::ProtocolUpgrade {
                is_hard_fork: false,
                ..
            } => (6000, 7500), // 60%, 75%
            ProposalType::ParameterChange { .. } => (5000, 6600), // 50%, 66%
            ProposalType::TreasuryAllocation { .. } => (5500, 7000), // 55%, 70%
            ProposalType::FeatureToggle { .. } => (4500, 6000),   // 45%, 60%
            ProposalType::GrantProgram { .. } => (5000, 6500),    // 50%, 65%
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
            quorum_required_bps,
            approval_required_bps,
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
        // Get voter's power first (before mutable borrow of proposal)
        let voting_power = self.get_effective_voting_power(&voter);
        if voting_power == 0 {
            return Err(Error::validation("No voting power"));
        }

        let proposal = self
            .proposals
            .get_mut(&proposal_id)
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

        // Calculate quadratic weight if enabled
        let quadratic_weight = if proposal.voting.use_quadratic_voting {
            (voting_power as f64).sqrt()
        } else {
            voting_power as f64
        };

        // Check for existing vote and remove it
        if let Some(old_vote) = proposal.voting.votes.remove(&voter) {
            match old_vote.vote_type {
                VoteType::For => {
                    proposal.voting.total_votes_for -= old_vote.quadratic_weight as u64
                }
                VoteType::Against => {
                    proposal.voting.total_votes_against -= old_vote.quadratic_weight as u64
                }
                VoteType::Abstain => {
                    proposal.voting.total_votes_abstain -= old_vote.quadratic_weight as u64
                }
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
        self.delegations
            .remove(&delegator)
            .ok_or_else(|| Error::validation("No active delegation"))?;
        Ok(())
    }

    /// Finalize proposal after voting ends
    pub fn finalize_proposal(&mut self, proposal_id: Uuid) -> Result<ProposalStatus> {
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
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

        // Check quorum (using basis points: 10000 = 100%)
        let participation_rate_bps = ((proposal.voting.total_voting_power as u128 * 10000)
            / total_eligible_power as u128) as u64;
        if participation_rate_bps < proposal.quorum_required_bps as u64 {
            proposal.status = ProposalStatus::Rejected;
            return Ok(ProposalStatus::Rejected);
        }

        // Check approval
        let total_decisive_votes =
            proposal.voting.total_votes_for + proposal.voting.total_votes_against;
        if total_decisive_votes == 0 {
            proposal.status = ProposalStatus::Rejected;
            return Ok(ProposalStatus::Rejected);
        }

        let approval_rate_bps = ((proposal.voting.total_votes_for as u128 * 10000)
            / total_decisive_votes as u128) as u64;
        if approval_rate_bps >= proposal.approval_required_bps as u64 {
            proposal.status = ProposalStatus::Passed;
            Ok(ProposalStatus::Passed)
        } else {
            proposal.status = ProposalStatus::Rejected;
            Ok(ProposalStatus::Rejected)
        }
    }

    /// Execute a passed proposal
    pub async fn execute_proposal(
        &mut self,
        proposal_id: Uuid,
        executor: UserId,
    ) -> Result<ExecutionResult> {
        let proposal = self
            .proposals
            .get(&proposal_id)
            .ok_or_else(|| Error::validation("Proposal not found"))?;

        if proposal.status != ProposalStatus::Passed {
            return Err(Error::validation("Proposal has not passed"));
        }

        // Clone the proposal type for async execution
        let proposal_type = proposal.proposal_type.clone();
        let proposal_id_str = proposal_id.to_string();

        // Compute pre-state hash for audit trail
        let pre_state_hash = self.compute_state_hash();

        // Execute based on proposal type with on-chain submission
        let (result, tx_hash) = match proposal_type {
            ProposalType::ParameterChange {
                parameter,
                proposed_value,
                current_value,
            } => {
                self.execute_parameter_change_production(
                    &proposal_id_str,
                    parameter,
                    &current_value,
                    &proposed_value,
                )
                .await?
            }
            ProposalType::TreasuryAllocation {
                amount,
                recipient,
                purpose,
            } => {
                self.execute_treasury_allocation_production(
                    proposal_id,
                    amount,
                    recipient,
                    &purpose,
                )
                .await?
            }
            ProposalType::ProtocolUpgrade {
                version,
                upgrade_hash,
                is_hard_fork,
            } => {
                self.execute_protocol_upgrade_production(&version, &upgrade_hash, is_hard_fork)
                    .await?
            }
            ProposalType::EmergencyAction {
                action_type,
                justification,
            } => {
                self.execute_emergency_action_production(action_type, &justification)
                    .await?
            }
            ProposalType::FeatureToggle {
                feature_name,
                enable,
            } => {
                self.execute_feature_toggle_production(&feature_name, enable)
                    .await?
            }
            ProposalType::GrantProgram {
                program_name,
                total_budget,
                duration_days,
            } => {
                self.execute_grant_program_production(
                    proposal_id,
                    &program_name,
                    total_budget,
                    duration_days,
                )
                .await?
            }
        };

        // Compute post-state hash
        let post_state_hash = self.compute_state_hash();

        // Update proposal status - safe because we verified it exists at the start
        let proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| Error::internal("Proposal disappeared during execution"))?;

        if matches!(
            result,
            ExecutionResult::Success | ExecutionResult::PartialSuccess { .. }
        ) {
            proposal.status = ProposalStatus::Executed;
        } else {
            proposal.status = ProposalStatus::ExecutionFailed {
                reason: format!("{:?}", result),
            };
        }

        proposal.executed_at = Some(Utc::now());
        proposal.execution = Some(ExecutionInfo {
            executor: Some(executor.clone()),
            execution_tx_hash: tx_hash.clone(),
            execution_result: result.clone(),
            executed_at: Utc::now(),
        });

        // Create audit record
        let action_type = match &proposal.proposal_type {
            ProposalType::ParameterChange { .. } => "ParameterChange",
            ProposalType::TreasuryAllocation { .. } => "TreasuryAllocation",
            ProposalType::ProtocolUpgrade { .. } => "ProtocolUpgrade",
            ProposalType::EmergencyAction { .. } => "EmergencyAction",
            ProposalType::FeatureToggle { .. } => "FeatureToggle",
            ProposalType::GrantProgram { .. } => "GrantProgram",
        };

        let audit_record = ExecutionAuditRecord {
            proposal_id,
            action_type: action_type.to_string(),
            executor,
            tx_hash,
            block_height: self.get_current_block().await.unwrap_or(0),
            executed_at: Utc::now(),
            pre_state_hash,
            post_state_hash,
            success: matches!(result, ExecutionResult::Success),
            details: HashMap::new(),
        };

        self.audit_records.push(audit_record);

        // Record metrics
        if let Some(metrics) = &self.metrics {
            let mut labels = HashMap::new();
            labels.insert("action_type".to_string(), action_type.to_string());
            labels.insert(
                "success".to_string(),
                matches!(result, ExecutionResult::Success).to_string(),
            );
            let _ = metrics
                .record_counter(
                    "governance_executions_total".to_string(),
                    1.0,
                    labels,
                    "Total governance proposal executions".to_string(),
                )
                .await;
        }

        tracing::info!(
            "✅ Governance proposal {} executed: action={}, success={}",
            proposal_id,
            action_type,
            matches!(result, ExecutionResult::Success)
        );

        Ok(result)
    }

    /// Compute hash of current governance state for audit
    fn compute_state_hash(&self) -> String {
        use blake3::Hasher;
        let mut hasher = Hasher::new();

        // Hash treasury state
        hasher.update(&self.treasury.total_balance.to_le_bytes());
        hasher.update(&self.treasury.available_funds.to_le_bytes());
        hasher.update(&self.treasury.reserved_funds.to_le_bytes());

        // Hash protocol parameters
        for (key, value) in &self.protocol_parameters {
            hasher.update(key.as_bytes());
            hasher.update(value.as_bytes());
        }

        // Hash feature flags
        for (key, value) in &self.feature_flags {
            hasher.update(key.as_bytes());
            hasher.update(&[*value as u8]);
        }

        hex::encode(hasher.finalize().as_bytes())
    }

    /// Get current block height from chain
    async fn get_current_block(&self) -> Result<u64> {
        if let Some(client) = &self.chain_client {
            client.get_current_block().await
        } else {
            Ok(0)
        }
    }

    /// Execute parameter change with on-chain transaction
    async fn execute_parameter_change_production(
        &mut self,
        proposal_id: &str,
        parameter: ProtocolParameter,
        current_value: &str,
        new_value: &str,
    ) -> Result<(ExecutionResult, String)> {
        let parameter_name = format!("{:?}", parameter);

        // Submit to chain if client available
        let tx_hash = if let Some(client) = &self.chain_client {
            let receipt = client
                .submit_parameter_change(&parameter_name, current_value, new_value, proposal_id)
                .await?;

            if !receipt.success {
                return Ok((
                    ExecutionResult::Failed {
                        error: format!("Chain transaction failed: tx_hash={}", receipt.tx_hash),
                    },
                    receipt.tx_hash,
                ));
            }

            // Wait for confirmation (2 blocks for safety)
            let confirmed = client.wait_for_confirmation(&receipt.tx_hash, 2).await?;
            if !confirmed {
                return Ok((
                    ExecutionResult::Failed {
                        error: "Transaction confirmation timeout".to_string(),
                    },
                    receipt.tx_hash,
                ));
            }

            receipt.tx_hash
        } else {
            format!("local_{}", Uuid::new_v4())
        };

        // Update local parameter cache
        self.protocol_parameters
            .insert(parameter_name.clone(), new_value.to_string());

        tracing::info!(
            "Parameter {} changed: {} → {} (tx: {})",
            parameter_name,
            current_value,
            new_value,
            tx_hash
        );

        Ok((ExecutionResult::Success, tx_hash))
    }

    /// Execute treasury allocation with on-chain transfer
    async fn execute_treasury_allocation_production(
        &mut self,
        proposal_id: Uuid,
        amount: u64,
        recipient: UserId,
        purpose: &str,
    ) -> Result<(ExecutionResult, String)> {
        // Verify sufficient funds
        if amount > self.treasury.available_funds {
            return Ok((
                ExecutionResult::Failed {
                    error: format!(
                        "Insufficient treasury funds: requested {}, available {}",
                        amount, self.treasury.available_funds
                    ),
                },
                String::new(),
            ));
        }

        // Submit to chain if client available
        let tx_hash = if let Some(client) = &self.chain_client {
            let receipt = client
                .submit_treasury_transfer(
                    &recipient.0.as_bytes()[..],
                    amount,
                    purpose,
                    &proposal_id.to_string(),
                )
                .await?;

            if !receipt.success {
                return Ok((
                    ExecutionResult::Failed {
                        error: format!("Treasury transfer failed: {}", receipt.tx_hash),
                    },
                    receipt.tx_hash,
                ));
            }

            // Wait for confirmation
            let confirmed = client.wait_for_confirmation(&receipt.tx_hash, 2).await?;
            if !confirmed {
                return Ok((
                    ExecutionResult::Failed {
                        error: "Transfer confirmation timeout".to_string(),
                    },
                    receipt.tx_hash,
                ));
            }

            receipt.tx_hash
        } else {
            format!("local_{}", Uuid::new_v4())
        };

        // Update local treasury state
        let allocation = TreasuryAllocation {
            id: Uuid::new_v4(),
            proposal_id,
            amount,
            recipient: recipient.clone(),
            purpose: purpose.to_string(),
            allocated_at: Utc::now(),
            disbursed: true,
            disbursed_at: Some(Utc::now()),
            tx_hash: Some(tx_hash.clone()),
        };

        self.treasury.available_funds -= amount;
        self.treasury.total_balance -= amount;
        self.treasury.allocations.push(allocation);

        tracing::info!(
            "Treasury allocation: {} tokens to {:?} for '{}' (tx: {})",
            amount,
            recipient,
            purpose,
            tx_hash
        );

        Ok((ExecutionResult::Success, tx_hash))
    }

    /// Execute protocol upgrade with staged rollout
    async fn execute_protocol_upgrade_production(
        &mut self,
        version: &str,
        upgrade_hash: &str,
        is_hard_fork: bool,
    ) -> Result<(ExecutionResult, String)> {
        // Calculate activation block (grace period for node upgrades)
        let current_block = self.get_current_block().await?;
        let grace_period_blocks = if is_hard_fork { 14400 } else { 7200 }; // ~2 days or 1 day
        let activation_block = current_block + grace_period_blocks;

        let tx_hash = if let Some(client) = &self.chain_client {
            let receipt = client
                .submit_protocol_upgrade(version, upgrade_hash, activation_block, is_hard_fork)
                .await?;

            if !receipt.success {
                return Ok((
                    ExecutionResult::Failed {
                        error: format!("Upgrade scheduling failed: {}", receipt.tx_hash),
                    },
                    receipt.tx_hash,
                ));
            }

            receipt.tx_hash
        } else {
            format!("local_{}", Uuid::new_v4())
        };

        tracing::info!(
            "🚀 Protocol upgrade scheduled: v{} (hash: {}) activating at block {} (hard_fork: {})",
            version,
            upgrade_hash,
            activation_block,
            is_hard_fork
        );

        Ok((ExecutionResult::Success, tx_hash))
    }

    /// Execute emergency action with immediate effect
    async fn execute_emergency_action_production(
        &mut self,
        action_type: EmergencyActionType,
        justification: &str,
    ) -> Result<(ExecutionResult, String)> {
        let action_str = format!("{:?}", action_type);
        let mut params = HashMap::new();
        params.insert("justification".to_string(), justification.to_string());

        // Add action-specific parameters
        match &action_type {
            EmergencyActionType::CircuitBreaker { module } => {
                params.insert("module".to_string(), module.clone());
            }
            EmergencyActionType::EmergencyOverride { parameter, value } => {
                params.insert("parameter".to_string(), parameter.clone());
                params.insert("value".to_string(), value.clone());
            }
            EmergencyActionType::ForceUpgrade { version } => {
                params.insert("version".to_string(), version.clone());
            }
            _ => {}
        }

        let tx_hash = if let Some(client) = &self.chain_client {
            let receipt = client.submit_emergency_action(&action_str, &params).await?;

            if !receipt.success {
                return Ok((
                    ExecutionResult::Failed {
                        error: format!("Emergency action failed: {}", receipt.tx_hash),
                    },
                    receipt.tx_hash,
                ));
            }

            receipt.tx_hash
        } else {
            format!("local_{}", Uuid::new_v4())
        };

        // Apply local state changes immediately
        match action_type {
            EmergencyActionType::PauseProtocol => {
                self.is_protocol_paused = true;
                tracing::warn!("⚠️ PROTOCOL PAUSED: {}", justification);
            }
            EmergencyActionType::ResumeProtocol => {
                self.is_protocol_paused = false;
                tracing::info!("✅ Protocol resumed");
            }
            EmergencyActionType::CircuitBreaker { module } => {
                self.circuit_breakers.insert(module.clone(), true);
                tracing::warn!("⚠️ Circuit breaker activated for module: {}", module);
            }
            EmergencyActionType::EmergencyOverride { parameter, value } => {
                self.protocol_parameters
                    .insert(parameter.clone(), value.clone());
                tracing::warn!("⚠️ Emergency override: {} = {}", parameter, value);
            }
            EmergencyActionType::ForceUpgrade { version } => {
                tracing::warn!("⚠️ Force upgrade initiated to version: {}", version);
            }
        }

        Ok((ExecutionResult::Success, tx_hash))
    }

    /// Execute feature toggle with on-chain persistence
    async fn execute_feature_toggle_production(
        &mut self,
        feature_name: &str,
        enable: bool,
    ) -> Result<(ExecutionResult, String)> {
        let tx_hash = if let Some(client) = &self.chain_client {
            let receipt = client.submit_feature_toggle(feature_name, enable).await?;

            if !receipt.success {
                return Ok((
                    ExecutionResult::Failed {
                        error: format!("Feature toggle failed: {}", receipt.tx_hash),
                    },
                    receipt.tx_hash,
                ));
            }

            receipt.tx_hash
        } else {
            format!("local_{}", Uuid::new_v4())
        };

        // Update local feature flags
        self.feature_flags.insert(feature_name.to_string(), enable);

        tracing::info!(
            "Feature '{}' toggled: {} (tx: {})",
            feature_name,
            if enable { "enabled" } else { "disabled" },
            tx_hash
        );

        Ok((ExecutionResult::Success, tx_hash))
    }

    /// Execute grant program creation with budget allocation
    async fn execute_grant_program_production(
        &mut self,
        proposal_id: Uuid,
        program_name: &str,
        total_budget: u64,
        duration_days: u32,
    ) -> Result<(ExecutionResult, String)> {
        // Verify sufficient funds
        if total_budget > self.treasury.available_funds {
            return Ok((
                ExecutionResult::Failed {
                    error: format!(
                        "Insufficient treasury funds for grant program: requested {}, available {}",
                        total_budget, self.treasury.available_funds
                    ),
                },
                String::new(),
            ));
        }

        // Reserve funds on chain
        let tx_hash = if let Some(client) = &self.chain_client {
            let mut params = HashMap::new();
            params.insert("program_name".to_string(), program_name.to_string());
            params.insert("budget".to_string(), total_budget.to_string());
            params.insert("duration_days".to_string(), duration_days.to_string());

            let receipt = client
                .submit_emergency_action("CreateGrantProgram", &params)
                .await?;

            if !receipt.success {
                return Ok((
                    ExecutionResult::Failed {
                        error: format!("Grant program creation failed: {}", receipt.tx_hash),
                    },
                    receipt.tx_hash,
                ));
            }

            receipt.tx_hash
        } else {
            format!("local_{}", Uuid::new_v4())
        };

        // Calculate program timeline
        let current_block = self.get_current_block().await.unwrap_or(0);
        let blocks_per_day = 7200u64; // ~12s per block
        let end_block = current_block + (duration_days as u64 * blocks_per_day);

        // Create grant program state
        let program = GrantProgramState {
            program_id: proposal_id,
            program_name: program_name.to_string(),
            total_budget,
            remaining_budget: total_budget,
            start_block: current_block,
            end_block,
            is_active: true,
            disbursements: Vec::new(),
        };

        self.grant_programs.insert(proposal_id, program);

        // Update treasury
        self.treasury.available_funds -= total_budget;
        self.treasury.reserved_funds += total_budget;

        tracing::info!(
            "📋 Grant program '{}' created: budget={}, duration={}d, ends at block {} (tx: {})",
            program_name,
            total_budget,
            duration_days,
            end_block,
            tx_hash
        );

        Ok((ExecutionResult::Success, tx_hash))
    }

    /// Disburse grant from an active program
    pub async fn disburse_grant(
        &mut self,
        program_id: Uuid,
        recipient: UserId,
        amount: u64,
        purpose: &str,
    ) -> Result<String> {
        // Get current block first before mutable borrow
        let current_block = self.get_current_block().await?;

        // Get program info for validation
        let (is_active, end_block, remaining_budget, program_name) = {
            let program = self
                .grant_programs
                .get(&program_id)
                .ok_or_else(|| Error::validation("Grant program not found"))?;
            (
                program.is_active,
                program.end_block,
                program.remaining_budget,
                program.program_name.clone(),
            )
        };

        if !is_active {
            return Err(Error::validation("Grant program is not active"));
        }

        if current_block > end_block {
            // Mark as inactive
            if let Some(program) = self.grant_programs.get_mut(&program_id) {
                program.is_active = false;
            }
            return Err(Error::validation("Grant program has expired"));
        }

        if amount > remaining_budget {
            return Err(Error::validation(format!(
                "Insufficient program budget: requested {}, remaining {}",
                amount, remaining_budget
            )));
        }

        // Submit disbursement to chain
        let tx_hash = if let Some(client) = &self.chain_client {
            let receipt = client
                .submit_treasury_transfer(
                    &recipient.0.as_bytes()[..],
                    amount,
                    &format!("Grant: {} - {}", program_name, purpose),
                    &program_id.to_string(),
                )
                .await?;

            if !receipt.success {
                return Err(Error::chain(format!(
                    "Grant disbursement failed: {}",
                    receipt.tx_hash
                )));
            }

            receipt.tx_hash
        } else {
            format!("local_{}", Uuid::new_v4())
        };

        // Record disbursement - now we can mutate
        // Safe because we verified it exists earlier
        let program = self
            .grant_programs
            .get_mut(&program_id)
            .ok_or_else(|| Error::internal("Grant program disappeared during disbursement"))?;

        let disbursement = GrantDisbursement {
            recipient: recipient.clone(),
            amount,
            purpose: purpose.to_string(),
            tx_hash: tx_hash.clone(),
            disbursed_at: Utc::now(),
        };

        program.remaining_budget -= amount;
        program.disbursements.push(disbursement);

        // Update treasury
        self.treasury.reserved_funds -= amount;
        self.treasury.total_balance -= amount;

        tracing::info!(
            "💰 Grant disbursed: {} tokens to {:?} from program '{}' (tx: {})",
            amount,
            recipient,
            program.program_name,
            tx_hash
        );

        Ok(tx_hash)
    }

    /// Get effective voting power including delegations
    fn get_effective_voting_power(&self, user: &UserId) -> u64 {
        let own_power = self.voting_power.get(user).copied().unwrap_or(0);

        // Add delegated power
        let delegated_power: u64 = self
            .delegations
            .values()
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
        self.proposals
            .values()
            .filter(|p| p.status == ProposalStatus::Active || p.status == ProposalStatus::Pending)
            .collect()
    }

    /// Get treasury status
    pub fn get_treasury(&self) -> &ProtocolTreasury {
        &self.treasury
    }

    /// Update emergency multisig signers (requires governance proposal)
    /// This should only be called after a successful governance vote
    pub fn update_emergency_multisig(&mut self, new_signers: Vec<UserId>) -> Result<()> {
        if new_signers.is_empty() {
            return Err(Error::validation("Emergency multisig cannot be empty"));
        }

        if new_signers.len() < 3 {
            return Err(Error::validation(
                "Emergency multisig requires at least 3 signers",
            ));
        }

        // Check for duplicates
        let unique_count = new_signers
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        if unique_count != new_signers.len() {
            return Err(Error::validation("Duplicate signers in emergency multisig"));
        }

        tracing::info!(
            "🔐 Emergency multisig updated: {} signers",
            new_signers.len()
        );

        self.emergency_multisig = new_signers;
        Ok(())
    }

    /// Reset a circuit breaker (requires emergency action or governance)
    pub fn reset_circuit_breaker(&mut self, module: &str) {
        self.circuit_breakers.remove(module);
        tracing::info!("✅ Circuit breaker reset for module: {}", module);
    }

    /// Get all active circuit breakers
    pub fn get_active_circuit_breakers(&self) -> Vec<&String> {
        self.circuit_breakers
            .iter()
            .filter(|(_, &active)| active)
            .map(|(name, _)| name)
            .collect()
    }

    /// Get feature flag status
    pub fn get_feature_flag(&self, feature: &str) -> Option<bool> {
        self.feature_flags.get(feature).copied()
    }

    /// Get all feature flags
    pub fn get_all_feature_flags(&self) -> &HashMap<String, bool> {
        &self.feature_flags
    }

    /// Get protocol parameter
    pub fn get_protocol_parameter(&self, param: &str) -> Option<&String> {
        self.protocol_parameters.get(param)
    }

    /// Get all protocol parameters
    pub fn get_all_protocol_parameters(&self) -> &HashMap<String, String> {
        &self.protocol_parameters
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

        let proposal_id = dao
            .submit_proposal(
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
            )
            .unwrap();

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

        let proposal_id = dao
            .submit_proposal(
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
            )
            .unwrap();

        // Activate proposal manually for testing
        let proposal = dao.proposals.get_mut(&proposal_id).unwrap();
        proposal.voting_starts_at = Utc::now() - Duration::hours(1);
        proposal.status = ProposalStatus::Active;

        // Cast votes
        dao.cast_vote(proposal_id, voter1, VoteType::For).unwrap();
        dao.cast_vote(proposal_id, voter2, VoteType::Against)
            .unwrap();

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

        dao.delegate_voting_power(delegator.clone(), delegate.clone(), Some(30))
            .unwrap();

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

        let proposal_id = dao
            .submit_proposal(
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
            )
            .unwrap();

        // Skip to passed status
        let proposal = dao.proposals.get_mut(&proposal_id).unwrap();
        proposal.status = ProposalStatus::Passed;

        // Note: execute_proposal is now async, so this test is simplified
        // Full async tests should use #[tokio::test]
        assert_eq!(dao.treasury.available_funds, 10000); // Funds not yet deducted
    }

    #[tokio::test]
    async fn test_treasury_allocation_async() {
        let mut dao = ProtocolDaoManager::new(vec![]);
        dao.add_treasury_funds(10000);

        let proposer = create_test_user();
        let recipient = create_test_user();

        dao.set_voting_power(proposer.clone(), 2000);

        let proposal_id = dao
            .submit_proposal(
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
            )
            .unwrap();

        // Skip to passed status
        let proposal = dao.proposals.get_mut(&proposal_id).unwrap();
        proposal.status = ProposalStatus::Passed;

        let result = dao.execute_proposal(proposal_id, proposer).await.unwrap();
        assert!(matches!(result, ExecutionResult::Success));
        assert_eq!(dao.treasury.available_funds, 9000);
    }

    #[tokio::test]
    async fn test_emergency_action() {
        let mut dao = ProtocolDaoManager::new(vec![]);
        let proposer = create_test_user();

        dao.set_voting_power(proposer.clone(), 2000);

        let proposal_id = dao
            .submit_proposal(
                proposer.clone(),
                ProposalType::EmergencyAction {
                    action_type: EmergencyActionType::PauseProtocol,
                    justification: "Security incident detected".to_string(),
                },
                "Emergency Pause".to_string(),
                "Pause protocol due to security issue".to_string(),
                "Active exploit detected".to_string(),
                1,
            )
            .unwrap();

        // Skip to passed status
        let proposal = dao.proposals.get_mut(&proposal_id).unwrap();
        proposal.status = ProposalStatus::Passed;

        assert!(!dao.is_paused());
        let result = dao.execute_proposal(proposal_id, proposer).await.unwrap();
        assert!(matches!(result, ExecutionResult::Success));
        assert!(dao.is_paused());
    }

    #[tokio::test]
    async fn test_feature_toggle() {
        let mut dao = ProtocolDaoManager::new(vec![]);
        let proposer = create_test_user();

        dao.set_voting_power(proposer.clone(), 2000);

        let proposal_id = dao
            .submit_proposal(
                proposer.clone(),
                ProposalType::FeatureToggle {
                    feature_name: "advanced_encryption".to_string(),
                    enable: true,
                },
                "Enable Advanced Encryption".to_string(),
                "Enable the new encryption feature".to_string(),
                "Testing complete".to_string(),
                7,
            )
            .unwrap();

        // Skip to passed status
        let proposal = dao.proposals.get_mut(&proposal_id).unwrap();
        proposal.status = ProposalStatus::Passed;

        let result = dao.execute_proposal(proposal_id, proposer).await.unwrap();
        assert!(matches!(result, ExecutionResult::Success));

        // Verify feature flag is set
        assert_eq!(dao.feature_flags.get("advanced_encryption"), Some(&true));
    }
}
