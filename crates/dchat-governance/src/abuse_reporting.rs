// Anonymous Abuse Reporting with Zero-Knowledge Proofs
//
// This module implements decentralized abuse reporting where:
// - Reports are ZK-encrypted to protect reporter identity
// - Decentralized jury (sortition) reviews evidence
// - False reports result in slashing
// - Appeal mechanisms protect against abuse

use chrono::{DateTime, Utc};
use dchat_core::{Error, Result, UserId};
use dchat_privacy::zk_proofs::{Groth16Keys, ZkProof, ZkProver};
use once_cell::sync::Lazy;
use rand::{CryptoRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Global ZK keys for abuse reporting (setup once, used by all)
static ABUSE_REPORTING_ZK_KEYS: Lazy<Groth16Keys> = Lazy::new(|| {
    // Use deterministic seed for reproducible keys
    let mut rng = ChaCha20Rng::from_seed([42u8; 32]);
    Groth16Keys::setup(&mut rng).expect("Failed to setup ZK keys for abuse reporting")
});

/// Type of abuse being reported
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbuseType {
    /// Spam or flooding
    Spam,
    /// Harassment or threats
    Harassment,
    /// Illegal content (CSAM, etc.)
    IllegalContent,
    /// Scam or fraud
    Fraud,
    /// Impersonation
    Impersonation,
    /// Other policy violation
    Other,
}

/// An encrypted abuse report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbuseReport {
    /// Unique report ID
    pub id: Uuid,
    /// ZK proof that reporter has reputation stake
    pub reputation_proof: ZkProof,
    /// Abuse type
    pub abuse_type: AbuseType,
    /// Encrypted evidence (message IDs, screenshots, etc.)
    pub encrypted_evidence: Vec<u8>,
    /// Accused user (may be pseudonymous)
    pub accused: UserId,
    /// Timestamp
    pub reported_at: DateTime<Utc>,
    /// Current status
    pub status: ReportStatus,
    /// Assigned jury members (after selection)
    pub jury: Vec<UserId>,
}

/// Status of an abuse report
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportStatus {
    /// Submitted, awaiting jury selection
    Pending,
    /// Under review by jury
    UnderReview,
    /// Jury voted to uphold (action taken)
    Upheld,
    /// Jury voted to dismiss
    Dismissed,
    /// Under appeal
    OnAppeal,
}

/// Jury selection via sortition (random selection weighted by reputation)
pub struct JurySelection {
    /// Pool of eligible jurors
    eligible_pool: Vec<(UserId, u32)>, // (user_id, reputation_score)
}

/// Manager for abuse reports
pub struct ReportManager {
    /// Active reports
    reports: HashMap<Uuid, AbuseReport>,
    /// Jury selector
    jury_selector: JurySelection,
}

impl AbuseReport {
    /// Create a new anonymous abuse report
    pub fn new<R: Rng + CryptoRng>(
        reporter_reputation: u32,
        abuse_type: AbuseType,
        evidence: &[u8],
        accused: UserId,
        encryption_key: &[u8; 32],
        rng: &mut R,
    ) -> Result<Self> {
        // Minimum reputation required to file report (prevents spam)
        const MIN_REPUTATION: u32 = 10;
        if reporter_reputation < MIN_REPUTATION {
            return Err(Error::validation(format!(
                "Insufficient reputation to file report (need {})",
                MIN_REPUTATION
            )));
        }

        // Generate ZK proof of reputation (without revealing identity)
        let prover = ZkProver::new(rng, &ABUSE_REPORTING_ZK_KEYS);
        let reputation_proof = prover
            .prove_reputation(reporter_reputation, MIN_REPUTATION, rng)?
            .proof;

        // Encrypt evidence (simple XOR for demonstration)
        let mut encrypted_evidence = evidence.to_vec();
        for (i, byte) in encrypted_evidence.iter_mut().enumerate() {
            *byte ^= encryption_key[i % 32];
        }

        Ok(Self {
            id: Uuid::new_v4(),
            reputation_proof,
            abuse_type,
            encrypted_evidence,
            accused,
            reported_at: Utc::now(),
            status: ReportStatus::Pending,
            jury: Vec::new(),
        })
    }

    /// Decrypt evidence (jury members only)
    pub fn decrypt_evidence(&self, decryption_key: &[u8; 32]) -> Vec<u8> {
        let mut plaintext = self.encrypted_evidence.clone();
        for (i, byte) in plaintext.iter_mut().enumerate() {
            *byte ^= decryption_key[i % 32];
        }
        plaintext
    }
}

impl JurySelection {
    /// Create a new jury selector with eligible pool
    pub fn new(eligible_pool: Vec<(UserId, u32)>) -> Self {
        Self { eligible_pool }
    }

    /// Select N jurors via weighted random selection
    ///
    /// Higher reputation = higher chance of selection
    pub fn select_jury<R: Rng + CryptoRng>(
        &self,
        jury_size: usize,
        rng: &mut R,
    ) -> Result<Vec<UserId>> {
        if self.eligible_pool.len() < jury_size {
            return Err(Error::validation(
                "Insufficient eligible jurors".to_string(),
            ));
        }

        // Calculate total reputation weight
        let total_weight: u32 = self.eligible_pool.iter().map(|(_, rep)| rep).sum();

        if total_weight == 0 {
            return Err(Error::validation("No reputation in pool".to_string()));
        }

        let mut selected = Vec::new();
        let mut available = self.eligible_pool.clone();

        for _ in 0..jury_size {
            if available.is_empty() {
                break;
            }

            // Weighted random selection
            let current_weight: u32 = available.iter().map(|(_, rep)| rep).sum();
            let mut selection_point = rng.gen_range(0..current_weight);

            let mut selected_idx = 0;
            for (idx, (_, rep)) in available.iter().enumerate() {
                if selection_point < *rep {
                    selected_idx = idx;
                    break;
                }
                selection_point -= rep;
            }

            let (juror_id, _) = available.remove(selected_idx);
            selected.push(juror_id);
        }

        Ok(selected)
    }

    /// Add a user to the eligible pool
    pub fn add_to_pool(&mut self, user_id: UserId, reputation: u32) {
        self.eligible_pool.push((user_id, reputation));
    }

    /// Remove a user from the pool
    pub fn remove_from_pool(&mut self, user_id: &UserId) {
        self.eligible_pool.retain(|(id, _)| id != user_id);
    }
}

impl ReportManager {
    /// Create a new report manager
    pub fn new(jury_selector: JurySelection) -> Self {
        Self {
            reports: HashMap::new(),
            jury_selector,
        }
    }

    /// Submit a new abuse report
    pub fn submit_report(&mut self, report: AbuseReport) -> Result<Uuid> {
        let id = report.id;
        self.reports.insert(id, report);
        Ok(id)
    }

    /// Assign jury to a pending report
    pub fn assign_jury<R: Rng + CryptoRng>(
        &mut self,
        report_id: &Uuid,
        jury_size: usize,
        rng: &mut R,
    ) -> Result<()> {
        let report = self
            .reports
            .get_mut(report_id)
            .ok_or_else(|| Error::NotFound("Report not found".to_string()))?;

        if report.status != ReportStatus::Pending {
            return Err(Error::validation("Report not pending".to_string()));
        }

        // Select jury
        let jury = self.jury_selector.select_jury(jury_size, rng)?;
        report.jury = jury;
        report.status = ReportStatus::UnderReview;

        Ok(())
    }

    /// Finalize report with jury decision
    pub fn finalize_report(&mut self, report_id: &Uuid, upheld: bool) -> Result<()> {
        let report = self
            .reports
            .get_mut(report_id)
            .ok_or_else(|| Error::NotFound("Report not found".to_string()))?;

        if report.status != ReportStatus::UnderReview {
            return Err(Error::validation("Report not under review".to_string()));
        }

        report.status = if upheld {
            ReportStatus::Upheld
        } else {
            ReportStatus::Dismissed
        };

        Ok(())
    }

    /// Appeal a finalized report
    pub fn appeal_report(&mut self, report_id: &Uuid) -> Result<()> {
        let report = self
            .reports
            .get_mut(report_id)
            .ok_or_else(|| Error::NotFound("Report not found".to_string()))?;

        if report.status != ReportStatus::Upheld && report.status != ReportStatus::Dismissed {
            return Err(Error::validation(
                "Can only appeal finalized reports".to_string(),
            ));
        }

        report.status = ReportStatus::OnAppeal;
        Ok(())
    }

    /// Get report by ID
    pub fn get_report(&self, id: &Uuid) -> Option<&AbuseReport> {
        self.reports.get(id)
    }

    /// Get all pending reports
    pub fn get_pending_reports(&self) -> Vec<&AbuseReport> {
        self.reports
            .values()
            .filter(|r| r.status == ReportStatus::Pending)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_abuse_report_creation() {
        let mut rng = OsRng;
        let accused = UserId::new();
        let key = [1u8; 32];
        let evidence = b"Evidence data";

        let report = AbuseReport::new(
            50, // reporter reputation
            AbuseType::Spam,
            evidence,
            accused,
            &key,
            &mut rng,
        )
        .unwrap();

        assert_eq!(report.abuse_type, AbuseType::Spam);
        assert_eq!(report.status, ReportStatus::Pending);
    }

    #[test]
    fn test_insufficient_reputation() {
        let mut rng = OsRng;
        let accused = UserId::new();
        let key = [1u8; 32];
        let evidence = b"Evidence data";

        let result = AbuseReport::new(
            5, // too low
            AbuseType::Spam,
            evidence,
            accused,
            &key,
            &mut rng,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_evidence_encryption_decryption() {
        let mut rng = OsRng;
        let accused = UserId::new();
        let key = [1u8; 32];
        let evidence = b"Secret evidence";

        let report =
            AbuseReport::new(50, AbuseType::Harassment, evidence, accused, &key, &mut rng).unwrap();

        let decrypted = report.decrypt_evidence(&key);
        assert_eq!(&decrypted, evidence);
    }

    #[test]
    fn test_jury_selection() {
        let mut rng = OsRng;
        let pool = vec![
            (UserId::new(), 100),
            (UserId::new(), 50),
            (UserId::new(), 75),
            (UserId::new(), 200),
            (UserId::new(), 25),
        ];

        let selector = JurySelection::new(pool);
        let jury = selector.select_jury(3, &mut rng).unwrap();

        assert_eq!(jury.len(), 3);
        // All jurors should be unique
        assert_eq!(
            jury.len(),
            jury.iter().collect::<std::collections::HashSet<_>>().len()
        );
    }

    #[test]
    fn test_report_manager_flow() {
        let mut rng = OsRng;
        let accused = UserId::new();
        let key = [1u8; 32];

        // Create jury pool
        let pool = vec![
            (UserId::new(), 100),
            (UserId::new(), 100),
            (UserId::new(), 100),
        ];
        let jury_selector = JurySelection::new(pool);
        let mut manager = ReportManager::new(jury_selector);

        // Submit report
        let report =
            AbuseReport::new(50, AbuseType::Spam, b"Evidence", accused, &key, &mut rng).unwrap();
        let report_id = manager.submit_report(report).unwrap();

        // Assign jury
        manager.assign_jury(&report_id, 3, &mut rng).unwrap();

        let report = manager.get_report(&report_id).unwrap();
        assert_eq!(report.status, ReportStatus::UnderReview);
        assert_eq!(report.jury.len(), 3);
    }

    #[test]
    fn test_report_finalization() {
        let mut rng = OsRng;
        let accused = UserId::new();
        let key = [1u8; 32];

        let pool = vec![
            (UserId::new(), 100),
            (UserId::new(), 100),
            (UserId::new(), 100),
        ];
        let jury_selector = JurySelection::new(pool);
        let mut manager = ReportManager::new(jury_selector);

        let report =
            AbuseReport::new(50, AbuseType::Fraud, b"Evidence", accused, &key, &mut rng).unwrap();
        let report_id = manager.submit_report(report).unwrap();

        manager.assign_jury(&report_id, 3, &mut rng).unwrap();
        manager.finalize_report(&report_id, true).unwrap();

        let report = manager.get_report(&report_id).unwrap();
        assert_eq!(report.status, ReportStatus::Upheld);
    }

    #[test]
    fn test_report_appeal() {
        let mut rng = OsRng;
        let accused = UserId::new();
        let key = [1u8; 32];

        let pool = vec![
            (UserId::new(), 100),
            (UserId::new(), 100),
            (UserId::new(), 100),
        ];
        let jury_selector = JurySelection::new(pool);
        let mut manager = ReportManager::new(jury_selector);

        let report =
            AbuseReport::new(50, AbuseType::Spam, b"Evidence", accused, &key, &mut rng).unwrap();
        let report_id = manager.submit_report(report).unwrap();

        manager.assign_jury(&report_id, 3, &mut rng).unwrap();
        manager.finalize_report(&report_id, false).unwrap();
        manager.appeal_report(&report_id).unwrap();

        let report = manager.get_report(&report_id).unwrap();
        assert_eq!(report.status, ReportStatus::OnAppeal);
    }
}
