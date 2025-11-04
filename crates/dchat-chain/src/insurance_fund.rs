//! Insurance Fund for Economic Security
//!
//! This module implements an insurance fund that protects the network against:
//! - Relay node failures and lost messages
//! - Slashing penalties that exceed node collateral
//! - Economic attacks and token draining
//! - Emergency situations requiring compensation
//!
//! The fund is managed by governance and automatically replenishes from fees.

use chrono::{DateTime, Utc};
use dchat_core::types::UserId;
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Insurance claim type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaimType {
    /// Relay node failed to deliver message
    RelayFailure {
        relay_id: UserId,
        affected_users: Vec<UserId>,
        message_count: u64,
    },

    /// Slashing penalty exceeded node collateral
    SlashingOverflow {
        node_id: UserId,
        deficit_amount: u64,
    },

    /// Economic attack compensation
    AttackCompensation {
        attack_type: String,
        affected_users: Vec<UserId>,
        total_loss: u64,
    },

    /// Emergency governance decision
    EmergencyCompensation {
        proposal_id: String,
        affected_users: Vec<UserId>,
        reason: String,
    },
}

/// Insurance claim status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaimStatus {
    /// Claim submitted, awaiting review
    Pending,

    /// Under investigation
    UnderReview,

    /// Approved, awaiting payment
    Approved { amount: u64 },

    /// Paid out
    Paid {
        amount: u64,
        transaction_hash: String,
    },

    /// Rejected with reason
    Rejected { reason: String },
}

/// Insurance claim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsuranceClaim {
    /// Unique claim ID
    pub id: Uuid,

    /// Claimant user ID
    pub claimant: UserId,

    /// Claim type and details
    pub claim_type: ClaimType,

    /// Requested compensation amount
    pub requested_amount: u64,

    /// Current status
    pub status: ClaimStatus,

    /// Submission timestamp
    pub submitted_at: DateTime<Utc>,

    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,

    /// Supporting evidence (hashes of documents, proofs)
    pub evidence: Vec<String>,

    /// Votes from governance (approve/reject)
    pub votes: HashMap<UserId, bool>,
}

/// Insurance fund statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundStatistics {
    /// Total fund balance
    pub total_balance: u64,

    /// Total claims submitted
    pub total_claims: u64,

    /// Total claims approved
    pub approved_claims: u64,

    /// Total amount paid out
    pub total_paid_out: u64,

    /// Current pending claims
    pub pending_claims: u64,

    /// Average claim amount
    pub average_claim_amount: u64,

    /// Fund health ratio (balance / average_monthly_payout)
    pub health_ratio: f64,
}

/// Insurance fund configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundConfiguration {
    /// Minimum fund balance before stopping payouts
    pub minimum_balance: u64,

    /// Maximum claim amount without governance approval
    pub auto_approve_threshold: u64,

    /// Percentage of transaction fees allocated to fund
    pub fee_allocation_percent: u8,

    /// Minimum votes required for claim approval
    pub min_votes_for_approval: u32,

    /// Maximum claim processing time (seconds)
    pub max_processing_time_secs: u64,
}

/// Insurance fund manager
pub struct InsuranceFund {
    /// Current fund balance
    balance: u64,

    /// All claims (historical and active)
    claims: HashMap<Uuid, InsuranceClaim>,

    /// Fund configuration
    config: FundConfiguration,

    /// Transaction history (deposits and withdrawals)
    transactions: Vec<FundTransaction>,
}

/// Fund transaction record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundTransaction {
    /// Transaction ID
    pub id: Uuid,

    /// Transaction type
    pub transaction_type: TransactionType,

    /// Amount
    pub amount: u64,

    /// Timestamp
    pub timestamp: DateTime<Utc>,

    /// Related claim (if any)
    pub related_claim: Option<Uuid>,

    /// Transaction hash on blockchain
    pub tx_hash: String,
}

/// Type of fund transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    /// Deposit from fees
    FeeDeposit,

    /// Deposit from governance
    GovernanceDeposit,

    /// Claim payout
    ClaimPayout,

    /// Emergency withdrawal
    EmergencyWithdrawal,
}

impl InsuranceFund {
    /// Create a new insurance fund
    pub fn new(initial_balance: u64, config: FundConfiguration) -> Self {
        Self {
            balance: initial_balance,
            claims: HashMap::new(),
            config,
            transactions: Vec::new(),
        }
    }

    /// Get current balance
    pub fn balance(&self) -> u64 {
        self.balance
    }

    /// Submit a new claim
    pub fn submit_claim(
        &mut self,
        claimant: UserId,
        claim_type: ClaimType,
        requested_amount: u64,
        evidence: Vec<String>,
    ) -> Result<Uuid> {
        if requested_amount == 0 {
            return Err(Error::validation("Claim amount must be greater than 0"));
        }

        if requested_amount > self.balance {
            return Err(Error::validation("Claim amount exceeds fund balance"));
        }

        let claim = InsuranceClaim {
            id: Uuid::new_v4(),
            claimant,
            claim_type,
            requested_amount,
            status: ClaimStatus::Pending,
            submitted_at: Utc::now(),
            updated_at: Utc::now(),
            evidence,
            votes: HashMap::new(),
        };

        let claim_id = claim.id;
        self.claims.insert(claim_id, claim);

        Ok(claim_id)
    }

    /// Vote on a claim (governance)
    pub fn vote_on_claim(&mut self, claim_id: Uuid, voter: UserId, approve: bool) -> Result<()> {
        let claim = self
            .claims
            .get_mut(&claim_id)
            .ok_or_else(|| Error::validation("Claim not found"))?;

        if !matches!(
            claim.status,
            ClaimStatus::Pending | ClaimStatus::UnderReview
        ) {
            return Err(Error::validation("Claim is not in votable status"));
        }

        claim.votes.insert(voter, approve);
        claim.updated_at = Utc::now();

        // Auto-approve if threshold met
        if claim.requested_amount <= self.config.auto_approve_threshold {
            self.approve_claim(claim_id)?;
        }

        Ok(())
    }

    /// Approve a claim
    pub fn approve_claim(&mut self, claim_id: Uuid) -> Result<()> {
        let claim = self
            .claims
            .get_mut(&claim_id)
            .ok_or_else(|| Error::validation("Claim not found"))?;

        // Check votes
        let approve_votes = claim.votes.values().filter(|&&v| v).count();
        if approve_votes < self.config.min_votes_for_approval as usize {
            return Err(Error::validation("Insufficient votes for approval"));
        }

        claim.status = ClaimStatus::Approved {
            amount: claim.requested_amount,
        };
        claim.updated_at = Utc::now();

        Ok(())
    }

    /// Pay out an approved claim
    pub fn payout_claim(&mut self, claim_id: Uuid, tx_hash: String) -> Result<()> {
        let claim = self
            .claims
            .get_mut(&claim_id)
            .ok_or_else(|| Error::validation("Claim not found"))?;

        let amount = match &claim.status {
            ClaimStatus::Approved { amount } => *amount,
            _ => return Err(Error::validation("Claim is not approved")),
        };

        if amount > self.balance {
            return Err(Error::validation("Insufficient fund balance"));
        }

        // Deduct from balance
        self.balance -= amount;

        // Update claim status
        claim.status = ClaimStatus::Paid {
            amount,
            transaction_hash: tx_hash.clone(),
        };
        claim.updated_at = Utc::now();

        // Record transaction
        self.transactions.push(FundTransaction {
            id: Uuid::new_v4(),
            transaction_type: TransactionType::ClaimPayout,
            amount,
            timestamp: Utc::now(),
            related_claim: Some(claim_id),
            tx_hash,
        });

        Ok(())
    }

    /// Reject a claim
    pub fn reject_claim(&mut self, claim_id: Uuid, reason: String) -> Result<()> {
        let claim = self
            .claims
            .get_mut(&claim_id)
            .ok_or_else(|| Error::validation("Claim not found"))?;

        claim.status = ClaimStatus::Rejected { reason };
        claim.updated_at = Utc::now();

        Ok(())
    }

    /// Deposit funds into the insurance fund
    pub fn deposit(&mut self, amount: u64, transaction_type: TransactionType, tx_hash: String) {
        self.balance += amount;

        self.transactions.push(FundTransaction {
            id: Uuid::new_v4(),
            transaction_type,
            amount,
            timestamp: Utc::now(),
            related_claim: None,
            tx_hash,
        });
    }

    /// Get claim by ID
    pub fn get_claim(&self, claim_id: &Uuid) -> Option<&InsuranceClaim> {
        self.claims.get(claim_id)
    }

    /// Get all pending claims
    pub fn get_pending_claims(&self) -> Vec<&InsuranceClaim> {
        self.claims
            .values()
            .filter(|c| matches!(c.status, ClaimStatus::Pending | ClaimStatus::UnderReview))
            .collect()
    }

    /// Get statistics
    pub fn get_statistics(&self) -> FundStatistics {
        let total_claims = self.claims.len() as u64;
        let approved_claims = self
            .claims
            .values()
            .filter(|c| {
                matches!(
                    c.status,
                    ClaimStatus::Approved { .. } | ClaimStatus::Paid { .. }
                )
            })
            .count() as u64;

        let total_paid_out: u64 = self
            .claims
            .values()
            .filter_map(|c| match &c.status {
                ClaimStatus::Paid { amount, .. } => Some(*amount),
                _ => None,
            })
            .sum();

        let pending_claims = self.get_pending_claims().len() as u64;

        let average_claim_amount = if total_claims > 0 {
            total_paid_out / total_claims
        } else {
            0
        };

        let health_ratio = if average_claim_amount > 0 {
            self.balance as f64 / (average_claim_amount * 12) as f64 // 12 months buffer
        } else {
            f64::INFINITY
        };

        FundStatistics {
            total_balance: self.balance,
            total_claims,
            approved_claims,
            total_paid_out,
            pending_claims,
            average_claim_amount,
            health_ratio,
        }
    }

    /// Check fund health
    pub fn is_healthy(&self) -> bool {
        self.balance >= self.config.minimum_balance
    }
}

impl Default for FundConfiguration {
    fn default() -> Self {
        Self {
            minimum_balance: 100_000,      // 100k tokens
            auto_approve_threshold: 1_000, // 1k tokens
            fee_allocation_percent: 10,    // 10% of fees
            min_votes_for_approval: 3,
            max_processing_time_secs: 7 * 24 * 3600, // 7 days
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user() -> UserId {
        UserId::new()
    }

    #[test]
    fn test_create_insurance_fund() {
        let config = FundConfiguration::default();
        let fund = InsuranceFund::new(1_000_000, config);

        assert_eq!(fund.balance(), 1_000_000);
        assert!(fund.is_healthy());
    }

    #[test]
    fn test_submit_claim() {
        let config = FundConfiguration::default();
        let mut fund = InsuranceFund::new(1_000_000, config);

        let claimant = create_test_user();
        let claim_id = fund
            .submit_claim(
                claimant,
                ClaimType::RelayFailure {
                    relay_id: create_test_user(),
                    affected_users: vec![create_test_user()],
                    message_count: 10,
                },
                5_000,
                vec!["proof1".to_string()],
            )
            .unwrap();

        let claim = fund.get_claim(&claim_id).unwrap();
        assert_eq!(claim.requested_amount, 5_000);
        assert!(matches!(claim.status, ClaimStatus::Pending));
    }

    #[test]
    fn test_vote_and_approve_claim() {
        let config = FundConfiguration::default();
        let mut fund = InsuranceFund::new(1_000_000, config);

        let claimant = create_test_user();
        let claim_id = fund
            .submit_claim(
                claimant,
                ClaimType::SlashingOverflow {
                    node_id: create_test_user(),
                    deficit_amount: 2_000,
                },
                2_000,
                vec![],
            )
            .unwrap();

        // Vote on claim
        for _ in 0..3 {
            fund.vote_on_claim(claim_id, create_test_user(), true)
                .unwrap();
        }

        // Approve claim
        fund.approve_claim(claim_id).unwrap();

        let claim = fund.get_claim(&claim_id).unwrap();
        assert!(matches!(claim.status, ClaimStatus::Approved { .. }));
    }

    #[test]
    fn test_payout_claim() {
        let config = FundConfiguration::default();
        let mut fund = InsuranceFund::new(1_000_000, config);

        let claimant = create_test_user();
        let claim_id = fund
            .submit_claim(
                claimant,
                ClaimType::AttackCompensation {
                    attack_type: "DDoS".to_string(),
                    affected_users: vec![create_test_user()],
                    total_loss: 10_000,
                },
                10_000,
                vec![],
            )
            .unwrap();

        // Approve
        for _ in 0..3 {
            fund.vote_on_claim(claim_id, create_test_user(), true)
                .unwrap();
        }
        fund.approve_claim(claim_id).unwrap();

        // Payout
        let initial_balance = fund.balance();
        fund.payout_claim(claim_id, "tx_hash_123".to_string())
            .unwrap();

        assert_eq!(fund.balance(), initial_balance - 10_000);

        let claim = fund.get_claim(&claim_id).unwrap();
        assert!(matches!(claim.status, ClaimStatus::Paid { .. }));
    }

    #[test]
    fn test_reject_claim() {
        let config = FundConfiguration::default();
        let mut fund = InsuranceFund::new(1_000_000, config);

        let claimant = create_test_user();
        let claim_id = fund
            .submit_claim(
                claimant,
                ClaimType::EmergencyCompensation {
                    proposal_id: "prop_001".to_string(),
                    affected_users: vec![],
                    reason: "Test".to_string(),
                },
                5_000,
                vec![],
            )
            .unwrap();

        fund.reject_claim(claim_id, "Insufficient evidence".to_string())
            .unwrap();

        let claim = fund.get_claim(&claim_id).unwrap();
        assert!(matches!(claim.status, ClaimStatus::Rejected { .. }));
    }

    #[test]
    fn test_deposit_to_fund() {
        let config = FundConfiguration::default();
        let mut fund = InsuranceFund::new(1_000_000, config);

        let initial_balance = fund.balance();
        fund.deposit(50_000, TransactionType::FeeDeposit, "tx_001".to_string());

        assert_eq!(fund.balance(), initial_balance + 50_000);
    }

    #[test]
    fn test_get_statistics() {
        let config = FundConfiguration::default();
        let mut fund = InsuranceFund::new(1_000_000, config);

        // Submit multiple claims
        for _ in 0..5 {
            fund.submit_claim(
                create_test_user(),
                ClaimType::RelayFailure {
                    relay_id: create_test_user(),
                    affected_users: vec![],
                    message_count: 1,
                },
                1_000,
                vec![],
            )
            .unwrap();
        }

        let stats = fund.get_statistics();
        assert_eq!(stats.total_claims, 5);
        assert_eq!(stats.pending_claims, 5);
        assert_eq!(stats.total_balance, 1_000_000);
    }

    #[test]
    fn test_fund_health() {
        let config = FundConfiguration {
            minimum_balance: 100_000,
            ..Default::default()
        };

        let fund1 = InsuranceFund::new(200_000, config.clone());
        assert!(fund1.is_healthy());

        let fund2 = InsuranceFund::new(50_000, config);
        assert!(!fund2.is_healthy());
    }
}
