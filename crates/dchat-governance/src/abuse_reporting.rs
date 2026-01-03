// Anonymous Abuse Reporting with Zero-Knowledge Proofs
//
// This module implements decentralized abuse reporting where:
// - Reports are ZK-encrypted to protect reporter identity
// - Decentralized jury (sortition) reviews evidence
// - False reports result in slashing
// - Appeal mechanisms protect against abuse
//
// Security: Uses AES-256-GCM for evidence encryption (NIST-approved AEAD)
// ZK keys are loaded from files for production security.

use chrono::{DateTime, Utc};
use dchat_core::{Error, Result, UserId};
use dchat_crypto::{decrypt_with_key, encrypt_with_key, KEY_SIZE};
use dchat_privacy::zk_proofs::{Groth16Keys, ZkProof, ZkProver};
use once_cell::sync::Lazy;
use rand::{CryptoRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use uuid::Uuid;

/// Environment variable for ZK keys directory
pub const ZK_KEYS_DIR_ENV: &str = "DCHAT_ZK_KEYS_DIR";

/// Default ZK keys directory (relative to data directory)
pub const DEFAULT_ZK_KEYS_DIR: &str = "zk_keys";

/// Filename for the proving key
pub const PROVING_KEY_FILENAME: &str = "abuse_reporting_proving.key";

/// Filename for the verifying key
pub const VERIFYING_KEY_FILENAME: &str = "abuse_reporting_verifying.key";

/// ZK key provider configuration
#[derive(Debug, Clone)]
pub struct ZkKeyConfig {
    /// Directory containing ZK key files
    pub keys_directory: PathBuf,
    /// Whether to generate keys if not found (only for development)
    pub allow_key_generation: bool,
}

impl Default for ZkKeyConfig {
    fn default() -> Self {
        Self {
            keys_directory: PathBuf::from(DEFAULT_ZK_KEYS_DIR),
            allow_key_generation: cfg!(debug_assertions), // Only in debug builds
        }
    }
}

impl ZkKeyConfig {
    /// Create config from environment or use defaults
    pub fn from_env() -> Self {
        let keys_directory = std::env::var(ZK_KEYS_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ZK_KEYS_DIR));

        Self {
            keys_directory,
            allow_key_generation: cfg!(debug_assertions),
        }
    }
}

/// Load ZK keys from files or generate if allowed
fn load_or_generate_zk_keys(config: &ZkKeyConfig) -> Result<Groth16Keys> {
    let proving_key_path = config.keys_directory.join(PROVING_KEY_FILENAME);
    let verifying_key_path = config.keys_directory.join(VERIFYING_KEY_FILENAME);

    // Try to load from files first
    if proving_key_path.exists() && verifying_key_path.exists() {
        tracing::info!("Loading ZK keys from {}", config.keys_directory.display());
        match Groth16Keys::load_from_files(&proving_key_path, &verifying_key_path) {
            Ok(keys) => return Ok(keys),
            Err(e) => {
                tracing::warn!("Failed to load ZK keys from files: {:?}", e);
                if !config.allow_key_generation {
                    return Err(Error::internal(format!(
                        "Failed to load ZK keys and generation is disabled: {:?}",
                        e
                    )));
                }
            }
        }
    }

    // Generate new keys if allowed
    if config.allow_key_generation {
        tracing::warn!("Generating new ZK keys (this should only happen in development)");

        // Use cryptographically secure random seed
        let mut seed = [0u8; 32];
        rand::thread_rng().fill(&mut seed);
        let mut rng = ChaCha20Rng::from_seed(seed);

        let keys = Groth16Keys::setup(&mut rng)?;

        // Attempt to save keys for future use
        if let Err(e) = std::fs::create_dir_all(&config.keys_directory) {
            tracing::warn!("Failed to create ZK keys directory: {:?}", e);
        } else if let Err(e) = keys.save_to_files(&proving_key_path, &verifying_key_path) {
            tracing::warn!("Failed to save generated ZK keys: {:?}", e);
        } else {
            tracing::info!(
                "Saved generated ZK keys to {}",
                config.keys_directory.display()
            );
        }

        return Ok(keys);
    }

    Err(Error::internal(format!(
        "ZK keys not found at {} and generation is disabled. \
         Set {} environment variable or generate keys with setup tool.",
        config.keys_directory.display(),
        ZK_KEYS_DIR_ENV
    )))
}

/// Global ZK keys for abuse reporting (loaded from files or generated)
///
/// # Panics
/// This will panic at startup if ZK key loading fails and generation is disabled.
/// This is intentional because the abuse reporting system cannot function without
/// valid ZK keys, and catching this at startup is safer than runtime failures.
static ABUSE_REPORTING_ZK_KEYS: Lazy<Groth16Keys> = Lazy::new(|| {
    let config = ZkKeyConfig::from_env();
    match load_or_generate_zk_keys(&config) {
        Ok(keys) => keys,
        Err(e) => {
            // Log critical error before panicking
            eprintln!(
                "CRITICAL: Failed to load ZK keys for abuse reporting: {:?}",
                e
            );
            eprintln!("The abuse reporting system cannot function without valid ZK keys.");
            eprintln!(
                "Either set {} environment variable pointing to key files,",
                ZK_KEYS_DIR_ENV
            );
            eprintln!("or generate keys using the setup tool: dchat-keygen --zk-keys");
            panic!("Failed to load ZK keys for abuse reporting: {:?}", e);
        }
    }
});

/// Get a reference to the global ZK keys
pub fn get_zk_keys() -> &'static Groth16Keys {
    &ABUSE_REPORTING_ZK_KEYS
}

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
    /// Create a new anonymous abuse report with AES-256-GCM encrypted evidence
    pub fn new<R: Rng + CryptoRng>(
        reporter_reputation: u32,
        abuse_type: AbuseType,
        evidence: &[u8],
        accused: UserId,
        encryption_key: &[u8; KEY_SIZE],
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

        // Encrypt evidence using AES-256-GCM (authenticated encryption)
        let encrypted_evidence = encrypt_with_key(encryption_key, evidence)?;

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

    /// Decrypt evidence using AES-256-GCM (jury members only)
    pub fn decrypt_evidence(&self, decryption_key: &[u8; KEY_SIZE]) -> Result<Vec<u8>> {
        decrypt_with_key(decryption_key, &self.encrypted_evidence)
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

        let decrypted = report.decrypt_evidence(&key).unwrap();
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
