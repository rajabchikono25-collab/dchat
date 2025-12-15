//! Two-Stage Finality with Attack Escalation
//!
//! Implements:
//! - Stage 1: Local/Continental fast finality (PoRW + PoT)
//! - Stage 2: Global/Deep finality (TSC checkpoints)
//! - Attack escalation presets for Byzantine scenarios
//! - Challenge-response protocol for fraud proofs
//!
//! Security Model:
//! - Fast path: 200ms local finality for good UX
//! - Deep finality: TSC checkpoint every 10 blocks (~20s)
//! - Attack detection triggers escalation to deeper verification

use crate::block_hierarchy::{Block, Hash, Subblock};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Maximum pending finality items
pub const MAX_PENDING_FINALITY: usize = 1000;

/// TSC checkpoint interval (blocks)
pub const TSC_CHECKPOINT_INTERVAL: u64 = 10; // ~20 seconds

/// Local finality timeout (milliseconds)
pub const LOCAL_FINALITY_TIMEOUT_MS: u64 = 500;

/// Continental finality timeout (milliseconds)  
pub const CONTINENTAL_FINALITY_TIMEOUT_MS: u64 = 2000;

/// Global finality timeout (milliseconds)
pub const GLOBAL_FINALITY_TIMEOUT_MS: u64 = 30000;

/// Challenge window (seconds)
/// Challenge window for finality disputes (seconds)
pub const FINALITY_CHALLENGE_WINDOW_SECS: u64 = 300;

/// Maximum escalation level
pub const MAX_ESCALATION_LEVEL: u8 = 4;

/// Finality errors
#[derive(Debug, Error)]
pub enum FinalityError {
    #[error("Block not found: {0}")]
    BlockNotFound(u64),

    #[error("Finality timeout at stage {0}")]
    FinalityTimeout(u8),

    #[error("Invalid finality transition: {0} -> {1}")]
    InvalidTransition(u8, u8),

    #[error("Challenge period active")]
    ChallengePeriodActive,

    #[error("Escalation in progress")]
    EscalationInProgress,

    #[error("Invalid challenge: {0}")]
    InvalidChallenge(String),

    #[error("Queue full")]
    QueueFull,

    #[error("Checkpoint mismatch at block {0}")]
    CheckpointMismatch(u64),
}

/// Finality stage
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum FinalityStage {
    /// Not yet finalized
    Pending = 0,
    /// Local finality (single region PoRW)
    Local = 1,
    /// Continental finality (multi-region PoRW + PoT)
    Continental = 2,
    /// Global finality (cross-continental + TSC checkpoint)
    Global = 3,
    /// Deep finality (TSC confirmed + no successful challenges)
    Deep = 4,
}

impl FinalityStage {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Pending),
            1 => Some(Self::Local),
            2 => Some(Self::Continental),
            3 => Some(Self::Global),
            4 => Some(Self::Deep),
            _ => None,
        }
    }

    /// Can transition to target stage?
    pub fn can_transition_to(&self, target: FinalityStage) -> bool {
        // Can only go forward, not backward
        (*self as u8) < (target as u8)
    }

    /// Required confirmations for this stage
    pub fn required_confirmations(&self) -> u64 {
        match self {
            Self::Pending => 0,
            Self::Local => 1,
            Self::Continental => 3,
            Self::Global => 10,
            Self::Deep => 100,
        }
    }

    /// Timeout for this stage
    pub fn timeout(&self) -> Duration {
        match self {
            Self::Pending => Duration::from_millis(LOCAL_FINALITY_TIMEOUT_MS),
            Self::Local => Duration::from_millis(LOCAL_FINALITY_TIMEOUT_MS),
            Self::Continental => Duration::from_millis(CONTINENTAL_FINALITY_TIMEOUT_MS),
            Self::Global => Duration::from_millis(GLOBAL_FINALITY_TIMEOUT_MS),
            Self::Deep => Duration::from_secs(FINALITY_CHALLENGE_WINDOW_SECS),
        }
    }
}

/// Escalation level for attack response
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum EscalationLevel {
    /// Normal operation
    Normal = 0,
    /// Elevated - minor anomalies detected
    Elevated = 1,
    /// Warning - potential attack indicators
    Warning = 2,
    /// Critical - active attack detected
    Critical = 3,
    /// Emergency - consensus under attack
    Emergency = 4,
}

impl EscalationLevel {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Normal),
            1 => Some(Self::Elevated),
            2 => Some(Self::Warning),
            3 => Some(Self::Critical),
            4 => Some(Self::Emergency),
            _ => None,
        }
    }
}

/// Escalation preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationPreset {
    /// Preset name
    pub name: String,
    /// Escalation level
    pub level: EscalationLevel,
    /// Required PoRW threshold (basis points)
    pub porw_threshold_bps: u64,
    /// Required TSC threshold (basis points)
    pub tsc_threshold_bps: u64,
    /// Minimum confirmations before finality
    pub min_confirmations: u64,
    /// Challenge window multiplier (1.0 = normal)
    pub challenge_window_multiplier: f64,
    /// Require TSC for all blocks (not just checkpoints)
    pub require_tsc_all_blocks: bool,
    /// Maximum block size reduction (0-100%)
    pub max_block_size_reduction_pct: u8,
    /// Rate limit reduction (0-100%)
    pub rate_limit_reduction_pct: u8,
}

impl EscalationPreset {
    /// Normal operation preset
    pub fn normal() -> Self {
        Self {
            name: "normal".to_string(),
            level: EscalationLevel::Normal,
            porw_threshold_bps: 6667, // 67%
            tsc_threshold_bps: 5100,  // 51%
            min_confirmations: 1,
            challenge_window_multiplier: 1.0,
            require_tsc_all_blocks: false,
            max_block_size_reduction_pct: 0,
            rate_limit_reduction_pct: 0,
        }
    }

    /// Elevated preset (minor anomalies)
    pub fn elevated() -> Self {
        Self {
            name: "elevated".to_string(),
            level: EscalationLevel::Elevated,
            porw_threshold_bps: 7000, // 70%
            tsc_threshold_bps: 5500,  // 55%
            min_confirmations: 3,
            challenge_window_multiplier: 1.5,
            require_tsc_all_blocks: false,
            max_block_size_reduction_pct: 10,
            rate_limit_reduction_pct: 10,
        }
    }

    /// Warning preset (potential attack)
    pub fn warning() -> Self {
        Self {
            name: "warning".to_string(),
            level: EscalationLevel::Warning,
            porw_threshold_bps: 7500, // 75%
            tsc_threshold_bps: 6000,  // 60%
            min_confirmations: 10,
            challenge_window_multiplier: 2.0,
            require_tsc_all_blocks: true,
            max_block_size_reduction_pct: 25,
            rate_limit_reduction_pct: 25,
        }
    }

    /// Critical preset (active attack)
    pub fn critical() -> Self {
        Self {
            name: "critical".to_string(),
            level: EscalationLevel::Critical,
            porw_threshold_bps: 8000, // 80%
            tsc_threshold_bps: 6667,  // 67%
            min_confirmations: 50,
            challenge_window_multiplier: 3.0,
            require_tsc_all_blocks: true,
            max_block_size_reduction_pct: 50,
            rate_limit_reduction_pct: 50,
        }
    }

    /// Emergency preset (consensus under attack)
    pub fn emergency() -> Self {
        Self {
            name: "emergency".to_string(),
            level: EscalationLevel::Emergency,
            porw_threshold_bps: 9000, // 90%
            tsc_threshold_bps: 7500,  // 75%
            min_confirmations: 100,
            challenge_window_multiplier: 5.0,
            require_tsc_all_blocks: true,
            max_block_size_reduction_pct: 75,
            rate_limit_reduction_pct: 75,
        }
    }

    /// Get preset for escalation level
    pub fn for_level(level: EscalationLevel) -> Self {
        match level {
            EscalationLevel::Normal => Self::normal(),
            EscalationLevel::Elevated => Self::elevated(),
            EscalationLevel::Warning => Self::warning(),
            EscalationLevel::Critical => Self::critical(),
            EscalationLevel::Emergency => Self::emergency(),
        }
    }
}

/// Finality status for a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockFinalityStatus {
    /// Block number
    pub block_number: u64,
    /// Block hash
    pub block_hash: Hash,
    /// Current finality stage
    pub stage: FinalityStage,
    /// Timestamp when block was received
    pub received_at: u64,
    /// Timestamp when each stage was reached
    pub stage_timestamps: HashMap<FinalityStage, u64>,
    /// PoRW vote count
    pub porw_votes: u64,
    /// PoRW total weight
    pub porw_weight_bps: u64,
    /// TSC vote count
    pub tsc_votes: u64,
    /// TSC total power
    pub tsc_power_bps: u64,
    /// Is this a TSC checkpoint?
    pub is_checkpoint: bool,
    /// Pending challenges
    pub challenge_count: u64,
    /// Has been challenged?
    pub challenged: bool,
}

impl BlockFinalityStatus {
    pub fn new(block_number: u64, block_hash: Hash) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            block_number,
            block_hash,
            stage: FinalityStage::Pending,
            received_at: now,
            stage_timestamps: HashMap::new(),
            porw_votes: 0,
            porw_weight_bps: 0,
            tsc_votes: 0,
            tsc_power_bps: 0,
            is_checkpoint: block_number % TSC_CHECKPOINT_INTERVAL == 0,
            challenge_count: 0,
            challenged: false,
        }
    }

    /// Advance to next stage if conditions met
    pub fn try_advance(&mut self, preset: &EscalationPreset) -> Option<FinalityStage> {
        let next_stage = match self.stage {
            FinalityStage::Pending => {
                // Need local PoRW threshold
                if self.porw_weight_bps >= preset.porw_threshold_bps / 2 {
                    Some(FinalityStage::Local)
                } else {
                    None
                }
            }
            FinalityStage::Local => {
                // Need full PoRW threshold
                if self.porw_weight_bps >= preset.porw_threshold_bps {
                    Some(FinalityStage::Continental)
                } else {
                    None
                }
            }
            FinalityStage::Continental => {
                // Need TSC checkpoint or preset requires TSC all blocks
                let needs_tsc = self.is_checkpoint || preset.require_tsc_all_blocks;
                if !needs_tsc || self.tsc_power_bps >= preset.tsc_threshold_bps {
                    Some(FinalityStage::Global)
                } else {
                    None
                }
            }
            FinalityStage::Global => {
                // Need challenge window to pass
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                let global_time = self
                    .stage_timestamps
                    .get(&FinalityStage::Global)
                    .copied()
                    .unwrap_or(now);

                let challenge_window = (FINALITY_CHALLENGE_WINDOW_SECS as f64
                    * preset.challenge_window_multiplier)
                    as u64;

                if now >= global_time + challenge_window && !self.challenged {
                    Some(FinalityStage::Deep)
                } else {
                    None
                }
            }
            FinalityStage::Deep => None, // Already final
        };

        if let Some(stage) = next_stage {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            self.stage = stage;
            self.stage_timestamps.insert(stage, now);
        }

        next_stage
    }

    /// Add PoRW vote
    pub fn add_porw_vote(&mut self, weight_bps: u64) {
        self.porw_votes += 1;
        self.porw_weight_bps = self.porw_weight_bps.saturating_add(weight_bps);
    }

    /// Add TSC vote
    pub fn add_tsc_vote(&mut self, power_bps: u64) {
        self.tsc_votes += 1;
        self.tsc_power_bps = self.tsc_power_bps.saturating_add(power_bps);
    }

    /// Register a challenge
    pub fn register_challenge(&mut self) {
        self.challenge_count += 1;
        self.challenged = true;
    }

    /// Time since block was received
    pub fn age_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        now.saturating_sub(self.received_at)
    }
}

/// Challenge types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChallengeType {
    /// Invalid PoRW proof
    InvalidPoRW {
        relay_id: [u8; 32],
        claimed_delivery: Hash,
        evidence: Vec<u8>,
    },
    /// Invalid PoT claim
    InvalidPoT {
        hop_index: u8,
        claimed_time: u64,
        evidence: Vec<u8>,
    },
    /// Double vote by staker
    DoubleVote {
        staker_id: [u8; 32],
        vote_a: Hash,
        vote_b: Hash,
        signatures: (Vec<u8>, Vec<u8>),
    },
    /// Invalid TSC checkpoint
    InvalidCheckpoint {
        checkpoint_hash: Hash,
        expected_hash: Hash,
        merkle_proof: Vec<u8>,
    },
    /// Fork detected
    ForkDetected {
        block_a: Hash,
        block_b: Hash,
        common_ancestor: u64,
    },
}

/// Challenge submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    /// Challenge ID
    pub id: [u8; 32],
    /// Target block
    pub block_number: u64,
    /// Target block hash
    pub block_hash: Hash,
    /// Challenge type
    pub challenge_type: ChallengeType,
    /// Challenger ID
    pub challenger_id: [u8; 32],
    /// Challenger stake (bond)
    pub challenger_bond: u64,
    /// Submission timestamp
    pub submitted_at: u64,
    /// Response deadline
    pub response_deadline: u64,
    /// Status
    pub status: ChallengeStatus,
}

/// Challenge status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChallengeStatus {
    /// Pending response
    Pending,
    /// Response submitted, under review
    UnderReview,
    /// Challenge accepted (block invalidated)
    Accepted,
    /// Challenge rejected (challenger slashed)
    Rejected,
    /// Expired (no response, block invalidated)
    Expired,
}

/// Response to a challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResponse {
    /// Challenge ID being responded to
    pub challenge_id: [u8; 32],
    /// Responder ID
    pub responder_id: [u8; 32],
    /// Response data (proofs, witnesses, etc.)
    pub response_data: Vec<u8>,
    /// Submission timestamp
    pub submitted_at: u64,
}

/// Challenge manager
pub struct ChallengeManager {
    /// Pending challenges by block
    challenges: RwLock<HashMap<u64, Vec<Challenge>>>,
    /// Challenge by ID
    challenge_by_id: RwLock<HashMap<[u8; 32], Challenge>>,
    /// Response deadline (seconds)
    response_deadline_secs: u64,
    /// Minimum challenger bond
    min_bond: u64,
}

impl ChallengeManager {
    pub fn new(response_deadline_secs: u64, min_bond: u64) -> Self {
        Self {
            challenges: RwLock::new(HashMap::new()),
            challenge_by_id: RwLock::new(HashMap::new()),
            response_deadline_secs,
            min_bond,
        }
    }

    /// Submit a challenge
    pub fn submit_challenge(
        &self,
        block_number: u64,
        block_hash: Hash,
        challenge_type: ChallengeType,
        challenger_id: [u8; 32],
        challenger_bond: u64,
    ) -> Result<[u8; 32], FinalityError> {
        if challenger_bond < self.min_bond {
            return Err(FinalityError::InvalidChallenge(format!(
                "Bond too low: {} < {}",
                challenger_bond, self.min_bond
            )));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Generate challenge ID
        let mut hasher = blake3::Hasher::new();
        hasher.update(&block_number.to_le_bytes());
        hasher.update(block_hash.as_bytes());
        hasher.update(&challenger_id);
        hasher.update(&now.to_le_bytes());
        let id = *hasher.finalize().as_bytes();

        let challenge = Challenge {
            id,
            block_number,
            block_hash,
            challenge_type,
            challenger_id,
            challenger_bond,
            submitted_at: now,
            response_deadline: now + self.response_deadline_secs,
            status: ChallengeStatus::Pending,
        };

        {
            let mut challenges = self.challenges.write();
            challenges
                .entry(block_number)
                .or_insert_with(Vec::new)
                .push(challenge.clone());
        }

        {
            let mut by_id = self.challenge_by_id.write();
            by_id.insert(id, challenge);
        }

        Ok(id)
    }

    /// Get challenges for block
    pub fn get_challenges(&self, block_number: u64) -> Vec<Challenge> {
        self.challenges
            .read()
            .get(&block_number)
            .cloned()
            .unwrap_or_default()
    }

    /// Get challenge by ID
    pub fn get_challenge(&self, id: &[u8; 32]) -> Option<Challenge> {
        self.challenge_by_id.read().get(id).cloned()
    }

    /// Submit response to challenge
    pub fn submit_response(&self, response: ChallengeResponse) -> Result<(), FinalityError> {
        let mut by_id = self.challenge_by_id.write();

        let challenge =
            by_id
                .get_mut(&response.challenge_id)
                .ok_or(FinalityError::InvalidChallenge(
                    "Challenge not found".into(),
                ))?;

        if challenge.status != ChallengeStatus::Pending {
            return Err(FinalityError::InvalidChallenge(
                "Challenge not pending".into(),
            ));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now > challenge.response_deadline {
            challenge.status = ChallengeStatus::Expired;
            return Err(FinalityError::InvalidChallenge(
                "Response deadline passed".into(),
            ));
        }

        challenge.status = ChallengeStatus::UnderReview;

        Ok(())
    }

    /// Resolve a challenge
    pub fn resolve_challenge(
        &self,
        challenge_id: &[u8; 32],
        accepted: bool,
    ) -> Result<Challenge, FinalityError> {
        let mut by_id = self.challenge_by_id.write();

        let challenge = by_id
            .get_mut(challenge_id)
            .ok_or(FinalityError::InvalidChallenge(
                "Challenge not found".into(),
            ))?;

        challenge.status = if accepted {
            ChallengeStatus::Accepted
        } else {
            ChallengeStatus::Rejected
        };

        // Update in block list too
        let mut challenges = self.challenges.write();
        if let Some(block_challenges) = challenges.get_mut(&challenge.block_number) {
            for c in block_challenges.iter_mut() {
                if c.id == *challenge_id {
                    c.status = challenge.status;
                    break;
                }
            }
        }

        Ok(challenge.clone())
    }

    /// Check for expired challenges
    pub fn check_expired(&self) -> Vec<Challenge> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut expired = Vec::new();
        let mut by_id = self.challenge_by_id.write();

        for challenge in by_id.values_mut() {
            if challenge.status == ChallengeStatus::Pending && now > challenge.response_deadline {
                challenge.status = ChallengeStatus::Expired;
                expired.push(challenge.clone());
            }
        }

        expired
    }
}

/// Attack detector for escalation
pub struct AttackDetector {
    /// Current escalation level
    level: AtomicU8,
    /// Recent anomaly scores
    anomaly_scores: Mutex<VecDeque<(Instant, u64)>>,
    /// Score thresholds for each level
    thresholds: [u64; 5],
    /// Time window for anomaly scoring
    window: Duration,
}

impl AttackDetector {
    pub fn new() -> Self {
        Self {
            level: AtomicU8::new(0),
            anomaly_scores: Mutex::new(VecDeque::with_capacity(1000)),
            thresholds: [
                0,    // Normal
                100,  // Elevated
                500,  // Warning
                2000, // Critical
                5000, // Emergency
            ],
            window: Duration::from_secs(300), // 5 minute window
        }
    }

    /// Report an anomaly
    pub fn report_anomaly(&self, score: u64) {
        let mut scores = self.anomaly_scores.lock();
        scores.push_back((Instant::now(), score));

        // Prune old entries
        let cutoff = Instant::now() - self.window;
        while scores.front().map_or(false, |(t, _)| *t < cutoff) {
            scores.pop_front();
        }

        // Calculate total score
        let total: u64 = scores.iter().map(|(_, s)| *s).sum();

        // Update level
        let new_level = if total >= self.thresholds[4] {
            4
        } else if total >= self.thresholds[3] {
            3
        } else if total >= self.thresholds[2] {
            2
        } else if total >= self.thresholds[1] {
            1
        } else {
            0
        };

        self.level.store(new_level, Ordering::Release);
    }

    /// Get current escalation level
    pub fn current_level(&self) -> EscalationLevel {
        EscalationLevel::from_u8(self.level.load(Ordering::Acquire))
            .unwrap_or(EscalationLevel::Normal)
    }

    /// Get current preset
    pub fn current_preset(&self) -> EscalationPreset {
        EscalationPreset::for_level(self.current_level())
    }

    /// Report specific attack indicators
    pub fn report_fork_detected(&self) {
        self.report_anomaly(1000);
    }

    pub fn report_double_vote(&self) {
        self.report_anomaly(500);
    }

    pub fn report_invalid_proof(&self) {
        self.report_anomaly(200);
    }

    pub fn report_timeout(&self) {
        self.report_anomaly(50);
    }

    pub fn report_low_participation(&self) {
        self.report_anomaly(100);
    }
}

impl Default for AttackDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Two-stage finality tracker
pub struct TwoStageFinality {
    /// Block finality statuses (indexed by block number)
    statuses: RwLock<HashMap<u64, BlockFinalityStatus>>,
    /// Hash to block number index for hash-based lookups
    hash_index: RwLock<HashMap<Hash, u64>>,
    /// Finalized blocks (deep finality)
    finalized: RwLock<Vec<u64>>,
    /// Last finalized block
    last_finalized: AtomicU64,
    /// Challenge manager
    challenge_manager: ChallengeManager,
    /// Attack detector
    attack_detector: AttackDetector,
    /// Maximum pending blocks
    max_pending: usize,
}

impl TwoStageFinality {
    pub fn new() -> Self {
        Self {
            statuses: RwLock::new(HashMap::new()),
            hash_index: RwLock::new(HashMap::new()),
            finalized: RwLock::new(Vec::new()),
            last_finalized: AtomicU64::new(0),
            challenge_manager: ChallengeManager::new(
                FINALITY_CHALLENGE_WINDOW_SECS,
                1_000_000_000, // 1000 DCHAT minimum bond
            ),
            attack_detector: AttackDetector::new(),
            max_pending: MAX_PENDING_FINALITY,
        }
    }

    /// Track a block by hash (for integration layer compatibility)
    pub fn track_block(&self, block_hash: &Hash) {
        // Auto-assign next block number
        let block_number = {
            let statuses = self.statuses.read();
            statuses.len() as u64 + 1
        };
        let _ = self.register_block(block_number, *block_hash);
    }

    /// Get finality stage by block hash (for integration layer compatibility)
    pub fn get_finality_stage(&self, block_hash: &Hash) -> Option<FinalityStage> {
        let hash_index = self.hash_index.read();
        if let Some(&block_number) = hash_index.get(block_hash) {
            Some(self.get_stage(block_number))
        } else {
            None
        }
    }

    /// Upgrade finality stage directly (for integration layer compatibility)
    pub fn upgrade_finality(&self, block_hash: &Hash, stage: FinalityStage) {
        let hash_index = self.hash_index.read();
        if let Some(&block_number) = hash_index.get(block_hash) {
            drop(hash_index);
            let mut statuses = self.statuses.write();
            if let Some(status) = statuses.get_mut(&block_number) {
                status.stage = stage;
            }
        }
    }

    /// Register a new block
    pub fn register_block(&self, block_number: u64, block_hash: Hash) -> Result<(), FinalityError> {
        let mut statuses = self.statuses.write();

        if statuses.len() >= self.max_pending {
            return Err(FinalityError::QueueFull);
        }

        let status = BlockFinalityStatus::new(block_number, block_hash);
        statuses.insert(block_number, status);
        drop(statuses);

        // Update hash index for hash-based lookups
        self.hash_index.write().insert(block_hash, block_number);

        Ok(())
    }

    /// Add PoRW vote for block
    pub fn add_porw_vote(
        &self,
        block_number: u64,
        weight_bps: u64,
    ) -> Result<Option<FinalityStage>, FinalityError> {
        let mut statuses = self.statuses.write();

        let status = statuses
            .get_mut(&block_number)
            .ok_or(FinalityError::BlockNotFound(block_number))?;

        status.add_porw_vote(weight_bps);

        let preset = self.attack_detector.current_preset();
        Ok(status.try_advance(&preset))
    }

    /// Add TSC vote for block
    pub fn add_tsc_vote(
        &self,
        block_number: u64,
        power_bps: u64,
    ) -> Result<Option<FinalityStage>, FinalityError> {
        let mut statuses = self.statuses.write();

        let status = statuses
            .get_mut(&block_number)
            .ok_or(FinalityError::BlockNotFound(block_number))?;

        status.add_tsc_vote(power_bps);

        let preset = self.attack_detector.current_preset();
        Ok(status.try_advance(&preset))
    }

    /// Get finality status for block
    pub fn get_status(&self, block_number: u64) -> Option<BlockFinalityStatus> {
        self.statuses.read().get(&block_number).cloned()
    }

    /// Get current finality stage for block
    pub fn get_stage(&self, block_number: u64) -> FinalityStage {
        self.statuses
            .read()
            .get(&block_number)
            .map(|s| s.stage)
            .unwrap_or(FinalityStage::Pending)
    }

    /// Is block at least locally finalized?
    pub fn is_locally_final(&self, block_number: u64) -> bool {
        self.get_stage(block_number) >= FinalityStage::Local
    }

    /// Is block continentally finalized?
    pub fn is_continental_final(&self, block_number: u64) -> bool {
        self.get_stage(block_number) >= FinalityStage::Continental
    }

    /// Is block globally finalized?
    pub fn is_globally_final(&self, block_number: u64) -> bool {
        self.get_stage(block_number) >= FinalityStage::Global
    }

    /// Is block deeply finalized (irreversible)?
    pub fn is_deeply_final(&self, block_number: u64) -> bool {
        self.get_stage(block_number) >= FinalityStage::Deep
    }

    /// Get last finalized block number
    pub fn last_finalized_block(&self) -> u64 {
        self.last_finalized.load(Ordering::Acquire)
    }

    /// Submit a challenge
    pub fn submit_challenge(
        &self,
        block_number: u64,
        challenge_type: ChallengeType,
        challenger_id: [u8; 32],
        challenger_bond: u64,
    ) -> Result<[u8; 32], FinalityError> {
        let block_hash = {
            let statuses = self.statuses.read();
            statuses
                .get(&block_number)
                .ok_or(FinalityError::BlockNotFound(block_number))?
                .block_hash
        };

        // Register challenge in status
        {
            let mut statuses = self.statuses.write();
            if let Some(status) = statuses.get_mut(&block_number) {
                status.register_challenge();
            }
        }

        // Report to attack detector
        match &challenge_type {
            ChallengeType::ForkDetected { .. } => {
                self.attack_detector.report_fork_detected();
            }
            ChallengeType::DoubleVote { .. } => {
                self.attack_detector.report_double_vote();
            }
            _ => {
                self.attack_detector.report_invalid_proof();
            }
        }

        self.challenge_manager.submit_challenge(
            block_number,
            block_hash,
            challenge_type,
            challenger_id,
            challenger_bond,
        )
    }

    /// Get current escalation level
    pub fn escalation_level(&self) -> EscalationLevel {
        self.attack_detector.current_level()
    }

    /// Get current escalation preset
    pub fn current_preset(&self) -> EscalationPreset {
        self.attack_detector.current_preset()
    }

    /// Process finality updates
    pub fn tick(&self) -> Vec<(u64, FinalityStage)> {
        let preset = self.attack_detector.current_preset();
        let mut advancements = Vec::new();

        let mut statuses = self.statuses.write();

        for (block_number, status) in statuses.iter_mut() {
            if let Some(new_stage) = status.try_advance(&preset) {
                advancements.push((*block_number, new_stage));

                if new_stage == FinalityStage::Deep {
                    let mut finalized = self.finalized.write();
                    finalized.push(*block_number);

                    self.last_finalized
                        .fetch_max(*block_number, Ordering::AcqRel);
                }
            }
        }

        // Check for expired challenges
        let expired = self.challenge_manager.check_expired();
        for challenge in expired {
            // Expired challenges without response = block invalidated
            // In production, this would trigger chain reorganization
            advancements.push((
                challenge.block_number,
                FinalityStage::Pending, // Roll back
            ));
        }

        advancements
    }

    /// Prune old finalized blocks from tracking
    pub fn prune(&self, keep_latest: usize) {
        let mut statuses = self.statuses.write();
        let mut finalized = self.finalized.write();

        if finalized.len() > keep_latest {
            let cutoff = finalized.len() - keep_latest;
            let to_remove: Vec<_> = finalized.drain(..cutoff).collect();

            for block in to_remove {
                statuses.remove(&block);
            }
        }
    }

    /// Process a full block for finality tracking
    ///
    /// Registers the block and all its subblocks for finality tracking.
    /// Returns the block hash used for registration.
    pub fn process_block(&self, block: &Block) -> Result<Hash, FinalityError> {
        // Compute block hash from state root
        let block_hash = block.state_root;

        // Register the main block
        self.register_block(block.height, block_hash)?;

        // Track subblocks for fine-grained finality
        for (subblock_idx, subblock) in block.subblocks.iter().enumerate() {
            self.process_subblock(block.height, subblock_idx, subblock)?;
        }

        Ok(block_hash)
    }

    /// Process a subblock for finality tracking
    ///
    /// Subblocks contribute to the overall block finality but may have
    /// individual finality states during the confirmation process.
    fn process_subblock(
        &self,
        parent_block_height: u64,
        subblock_idx: usize,
        subblock: &Subblock,
    ) -> Result<(), FinalityError> {
        // Subblocks are tracked within their parent block
        // The parent block number encodes the subblock index
        let subblock_number = parent_block_height * 100 + subblock_idx as u64;

        // Register the subblock with its miniblock headers root as the commitment
        self.register_block(subblock_number, subblock.miniblock_headers_root)?;

        Ok(())
    }

    /// Create an Arc-wrapped shared finality tracker for concurrent access
    pub fn into_shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl Default for TwoStageFinality {
    fn default() -> Self {
        Self::new()
    }
}

/// TSC checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TSCCheckpoint {
    /// Checkpoint block number
    pub block_number: u64,
    /// Block hash at checkpoint
    pub block_hash: Hash,
    /// State root at checkpoint
    pub state_root: Hash,
    /// Previous checkpoint
    pub previous_checkpoint: u64,
    /// Total votes
    pub total_votes: u64,
    /// Total voting power
    pub total_power: u64,
    /// Timestamp
    pub timestamp: u64,
    /// Merkle root of votes
    pub votes_merkle_root: Hash,
    /// Finalized?
    pub finalized: bool,
}

impl TSCCheckpoint {
    pub fn new(
        block_number: u64,
        block_hash: Hash,
        state_root: Hash,
        previous_checkpoint: u64,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            block_number,
            block_hash,
            state_root,
            previous_checkpoint,
            total_votes: 0,
            total_power: 0,
            timestamp,
            votes_merkle_root: Hash::from([0u8; 32]),
            finalized: false,
        }
    }

    /// Is this block a checkpoint candidate?
    pub fn is_checkpoint_block(block_number: u64) -> bool {
        block_number % TSC_CHECKPOINT_INTERVAL == 0
    }
}

/// Checkpoint manager
pub struct CheckpointManager {
    /// Checkpoints by block number
    checkpoints: RwLock<HashMap<u64, TSCCheckpoint>>,
    /// Last finalized checkpoint
    last_finalized: AtomicU64,
}

impl CheckpointManager {
    pub fn new() -> Self {
        Self {
            checkpoints: RwLock::new(HashMap::new()),
            last_finalized: AtomicU64::new(0),
        }
    }

    /// Create checkpoint if block is checkpoint candidate
    pub fn maybe_create_checkpoint(
        &self,
        block_number: u64,
        block_hash: Hash,
        state_root: Hash,
    ) -> Option<TSCCheckpoint> {
        if !TSCCheckpoint::is_checkpoint_block(block_number) {
            return None;
        }

        let previous = self.last_finalized.load(Ordering::Acquire);
        let checkpoint = TSCCheckpoint::new(block_number, block_hash, state_root, previous);

        let mut checkpoints = self.checkpoints.write();
        checkpoints.insert(block_number, checkpoint.clone());

        Some(checkpoint)
    }

    /// Get checkpoint
    pub fn get_checkpoint(&self, block_number: u64) -> Option<TSCCheckpoint> {
        self.checkpoints.read().get(&block_number).cloned()
    }

    /// Finalize checkpoint
    pub fn finalize_checkpoint(&self, block_number: u64) -> Result<(), FinalityError> {
        let mut checkpoints = self.checkpoints.write();

        let checkpoint = checkpoints
            .get_mut(&block_number)
            .ok_or(FinalityError::BlockNotFound(block_number))?;

        checkpoint.finalized = true;
        self.last_finalized.store(block_number, Ordering::Release);

        Ok(())
    }

    /// Get last finalized checkpoint block
    pub fn last_finalized_checkpoint(&self) -> u64 {
        self.last_finalized.load(Ordering::Acquire)
    }

    /// Verify checkpoint chain
    pub fn verify_checkpoint_chain(&self, from: u64, to: u64) -> Result<(), FinalityError> {
        let checkpoints = self.checkpoints.read();

        let mut current = to;
        while current > from {
            let checkpoint = checkpoints
                .get(&current)
                .ok_or(FinalityError::BlockNotFound(current))?;

            if !checkpoint.finalized && current != to {
                return Err(FinalityError::CheckpointMismatch(current));
            }

            current = checkpoint.previous_checkpoint;
        }

        Ok(())
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_hash(n: u8) -> Hash {
        let mut h = [0u8; 32];
        h[0] = n;
        Hash::from(h)
    }

    fn test_id(n: u8) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = n;
        id
    }

    #[test]
    fn test_finality_stage_transitions() {
        assert!(FinalityStage::Pending.can_transition_to(FinalityStage::Local));
        assert!(FinalityStage::Local.can_transition_to(FinalityStage::Continental));
        assert!(FinalityStage::Continental.can_transition_to(FinalityStage::Global));
        assert!(FinalityStage::Global.can_transition_to(FinalityStage::Deep));

        // Cannot go backwards
        assert!(!FinalityStage::Deep.can_transition_to(FinalityStage::Global));
        assert!(!FinalityStage::Global.can_transition_to(FinalityStage::Local));
    }

    #[test]
    fn test_escalation_presets() {
        let normal = EscalationPreset::normal();
        let critical = EscalationPreset::critical();

        // Critical has higher thresholds
        assert!(critical.porw_threshold_bps > normal.porw_threshold_bps);
        assert!(critical.tsc_threshold_bps > normal.tsc_threshold_bps);
        assert!(critical.min_confirmations > normal.min_confirmations);

        // Critical requires TSC for all blocks
        assert!(critical.require_tsc_all_blocks);
        assert!(!normal.require_tsc_all_blocks);
    }

    #[test]
    fn test_block_finality_advancement() {
        let mut status = BlockFinalityStatus::new(1, test_hash(1));
        let preset = EscalationPreset::normal();

        assert_eq!(status.stage, FinalityStage::Pending);

        // Add enough PoRW votes for local finality (50% of 67%)
        status.add_porw_vote(3500); // 35%
        assert!(status.try_advance(&preset).is_some());
        assert_eq!(status.stage, FinalityStage::Local);

        // Add more for continental (67%)
        status.add_porw_vote(3500); // 35% more = 70% total
        assert!(status.try_advance(&preset).is_some());
        assert_eq!(status.stage, FinalityStage::Continental);
    }

    #[test]
    fn test_two_stage_finality_tracker() {
        let tracker = TwoStageFinality::new();

        // Register block
        tracker.register_block(1, test_hash(1)).unwrap();

        assert!(!tracker.is_locally_final(1));

        // Add PoRW votes
        tracker.add_porw_vote(1, 3500).unwrap();
        assert!(tracker.is_locally_final(1));

        tracker.add_porw_vote(1, 3500).unwrap();
        assert!(tracker.is_continental_final(1));
    }

    #[test]
    fn test_attack_detector_escalation() {
        let detector = AttackDetector::new();

        assert_eq!(detector.current_level(), EscalationLevel::Normal);

        // Report minor anomaly
        detector.report_timeout();
        assert_eq!(detector.current_level(), EscalationLevel::Normal);

        // Report multiple anomalies to escalate
        for _ in 0..3 {
            detector.report_low_participation();
        }
        assert!(detector.current_level() >= EscalationLevel::Elevated);

        // Report serious attack
        detector.report_fork_detected();
        assert!(detector.current_level() >= EscalationLevel::Warning);
    }

    #[test]
    fn test_challenge_submission() {
        let tracker = TwoStageFinality::new();

        tracker.register_block(1, test_hash(1)).unwrap();

        let challenge_id = tracker
            .submit_challenge(
                1,
                ChallengeType::InvalidPoRW {
                    relay_id: test_id(1),
                    claimed_delivery: test_hash(2),
                    evidence: vec![1, 2, 3],
                },
                test_id(10),
                2_000_000_000, // 2000 DCHAT
            )
            .unwrap();

        // Block should be marked as challenged
        let status = tracker.get_status(1).unwrap();
        assert!(status.challenged);
        assert_eq!(status.challenge_count, 1);

        // Should affect escalation
        assert!(tracker.escalation_level() >= EscalationLevel::Elevated);
    }

    #[test]
    fn test_tsc_checkpoint() {
        let manager = CheckpointManager::new();

        // Block 10 is a checkpoint
        assert!(TSCCheckpoint::is_checkpoint_block(10));
        assert!(!TSCCheckpoint::is_checkpoint_block(5));

        // Create checkpoint
        let checkpoint = manager.maybe_create_checkpoint(10, test_hash(10), test_hash(100));

        assert!(checkpoint.is_some());
        let checkpoint = checkpoint.unwrap();
        assert_eq!(checkpoint.block_number, 10);
        assert!(!checkpoint.finalized);

        // Finalize
        manager.finalize_checkpoint(10).unwrap();
        assert_eq!(manager.last_finalized_checkpoint(), 10);
    }

    #[test]
    fn test_checkpoint_chain_verification() {
        let manager = CheckpointManager::new();

        // Create chain of checkpoints
        manager.maybe_create_checkpoint(10, test_hash(10), test_hash(100));
        manager.finalize_checkpoint(10).unwrap();

        manager.maybe_create_checkpoint(20, test_hash(20), test_hash(200));
        manager.finalize_checkpoint(20).unwrap();

        manager.maybe_create_checkpoint(30, test_hash(30), test_hash(250));

        // Verify chain
        assert!(manager.verify_checkpoint_chain(10, 30).is_ok());
    }

    #[test]
    fn test_escalation_affects_thresholds() {
        let tracker = TwoStageFinality::new();

        // Normal preset
        let normal = tracker.current_preset();
        assert_eq!(normal.porw_threshold_bps, 6667);

        // Force escalation by reporting attacks
        for _ in 0..10 {
            tracker
                .submit_challenge(
                    1,
                    ChallengeType::ForkDetected {
                        block_a: test_hash(1),
                        block_b: test_hash(2),
                        common_ancestor: 0,
                    },
                    test_id(1),
                    2_000_000_000,
                )
                .ok(); // Ignore errors from missing block
        }

        // Higher threshold after escalation
        let current = tracker.current_preset();
        assert!(current.porw_threshold_bps >= normal.porw_threshold_bps);
    }
}
