//! Threshold Normalization & Epoch Snapshots
//!
//! Implements:
//! - Epoch snapshots for consistent stake/weight views
//! - PoRW 67% and TSC 51% threshold normalization
//! - Protection against churn/grinding attacks
//! - Stake-weighted quorum calculations
//!
//! Security: Thresholds are computed against immutable epoch
//! snapshots, preventing manipulation through join/leave timing.

use crate::block_hierarchy::Hash;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Epoch duration in blocks
pub const EPOCH_LENGTH_BLOCKS: u64 = 1800; // ~1 hour at 2s blocks

/// Epoch duration in seconds
pub const EPOCH_LENGTH_SECS: u64 = 3600;

/// Epoch duration as a Duration type for timeout handling
pub const EPOCH_DURATION: Duration = Duration::from_secs(EPOCH_LENGTH_SECS);

/// Snapshot finalization delay (blocks after epoch start)
pub const SNAPSHOT_FINALIZATION_DELAY: u64 = 10;

/// Maximum stake age for TSC (epochs)
pub const MAX_STAKE_AGE_EPOCHS: u64 = 52; // ~1 year

/// PoRW quorum threshold (67% = 2/3 + 1)
pub const PORW_QUORUM_BPS: u64 = 6667; // 66.67% in basis points

/// TSC quorum threshold (51% simple majority)
pub const TSC_QUORUM_BPS: u64 = 5100; // 51% in basis points

/// Minimum participation for valid vote
pub const MIN_PARTICIPATION_BPS: u64 = 3333; // 33.33% minimum

/// Threshold errors
#[derive(Debug, Error)]
pub enum ThresholdError {
    #[error("Epoch {0} not found")]
    EpochNotFound(u64),

    #[error("Epoch {0} snapshot not finalized")]
    SnapshotNotFinalized(u64),

    #[error("Stake not found: {0:?}")]
    StakeNotFound([u8; 32]),

    #[error("Relay not found: {0:?}")]
    RelayNotFound([u8; 32]),

    #[error("Insufficient participation: {0} bps (min {1} bps)")]
    InsufficientParticipation(u64, u64),

    #[error("Quorum not reached: {0} bps (required {1} bps)")]
    QuorumNotReached(u64, u64),

    #[error("Invalid threshold: {0}")]
    InvalidThreshold(String),

    #[error("Epoch transition in progress")]
    EpochTransitionInProgress,
}

/// Relay weight entry in epoch snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayWeight {
    /// Relay ID
    pub relay_id: [u8; 32],
    /// Normalized weight (0-10000 basis points of total)
    pub weight_bps: u64,
    /// Raw score (delivery proofs, uptime, etc.)
    pub raw_score: u64,
    /// Is relay active this epoch?
    pub active: bool,
}

/// Stake entry in epoch snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeEntry {
    /// Staker ID
    pub staker_id: [u8; 32],
    /// Base stake amount
    pub stake_amount: u64,
    /// Temporal multiplier (based on lockup duration)
    pub temporal_multiplier: u64, // 100 = 1x, 800 = 8x
    /// Effective voting power (stake * multiplier / 100)
    pub effective_power: u64,
    /// Lockup epoch (when stake was locked)
    pub lockup_epoch: u64,
    /// Unlock epoch (when stake can be withdrawn)
    pub unlock_epoch: u64,
}

/// Epoch snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochSnapshot {
    /// Epoch number
    pub epoch: u64,
    /// Epoch start block
    pub start_block: u64,
    /// Epoch end block
    pub end_block: u64,
    /// Snapshot block (start + finalization delay)
    pub snapshot_block: u64,
    /// Timestamp when snapshot was taken
    pub timestamp: u64,
    /// Previous epoch hash (for chain)
    pub previous_epoch_hash: Hash,

    /// Relay weights (PoRW)
    pub relay_weights: HashMap<[u8; 32], RelayWeight>,
    /// Total relay weight (sum of all weight_bps)
    pub total_relay_weight: u64,
    /// Active relay count
    pub active_relay_count: u64,

    /// Staker entries (TSC)
    pub stake_entries: HashMap<[u8; 32], StakeEntry>,
    /// Total effective stake power
    pub total_stake_power: u64,
    /// Total stakers
    pub staker_count: u64,

    /// Is this snapshot finalized?
    pub finalized: bool,
    /// Merkle root of snapshot data
    pub merkle_root: Hash,
}

impl EpochSnapshot {
    /// Create a new epoch snapshot
    pub fn new(epoch: u64, start_block: u64, previous_epoch_hash: Hash) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            epoch,
            start_block,
            end_block: start_block + EPOCH_LENGTH_BLOCKS - 1,
            snapshot_block: start_block + SNAPSHOT_FINALIZATION_DELAY,
            timestamp,
            previous_epoch_hash,
            relay_weights: HashMap::new(),
            total_relay_weight: 0,
            active_relay_count: 0,
            stake_entries: HashMap::new(),
            total_stake_power: 0,
            staker_count: 0,
            finalized: false,
            merkle_root: Hash::from([0u8; 32]),
        }
    }

    /// Add or update relay weight
    pub fn set_relay_weight(&mut self, relay_id: [u8; 32], raw_score: u64, active: bool) {
        let weight = RelayWeight {
            relay_id,
            weight_bps: 0, // Normalized after all relays added
            raw_score,
            active,
        };

        if active {
            self.active_relay_count += 1;
        }

        self.relay_weights.insert(relay_id, weight);
    }

    /// Add or update stake entry
    pub fn set_stake(
        &mut self,
        staker_id: [u8; 32],
        stake_amount: u64,
        temporal_multiplier: u64,
        lockup_epoch: u64,
        unlock_epoch: u64,
    ) {
        let effective_power = stake_amount * temporal_multiplier / 100;

        let entry = StakeEntry {
            staker_id,
            stake_amount,
            temporal_multiplier,
            effective_power,
            lockup_epoch,
            unlock_epoch,
        };

        self.stake_entries.insert(staker_id, entry);
        self.staker_count += 1;
    }

    /// Finalize the snapshot (normalize weights, compute root)
    pub fn finalize(&mut self) {
        // Normalize relay weights
        let total_raw: u64 = self
            .relay_weights
            .values()
            .filter(|r| r.active)
            .map(|r| r.raw_score)
            .sum();

        if total_raw > 0 {
            for weight in self.relay_weights.values_mut() {
                if weight.active {
                    weight.weight_bps = weight.raw_score * 10000 / total_raw;
                }
            }
        }

        self.total_relay_weight = 10000; // Always 100% after normalization

        // Calculate total stake power
        self.total_stake_power = self.stake_entries.values().map(|s| s.effective_power).sum();

        // Compute Merkle root
        self.merkle_root = self.compute_merkle_root();

        self.finalized = true;
    }

    fn compute_merkle_root(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();

        // Hash epoch metadata
        hasher.update(&self.epoch.to_le_bytes());
        hasher.update(&self.start_block.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(self.previous_epoch_hash.as_bytes());

        // Hash relay weights
        let mut relay_ids: Vec<_> = self.relay_weights.keys().collect();
        relay_ids.sort();
        for id in relay_ids {
            let weight = &self.relay_weights[id];
            hasher.update(id);
            hasher.update(&weight.weight_bps.to_le_bytes());
            hasher.update(&[weight.active as u8]);
        }

        // Hash stake entries
        let mut staker_ids: Vec<_> = self.stake_entries.keys().collect();
        staker_ids.sort();
        for id in staker_ids {
            let entry = &self.stake_entries[id];
            hasher.update(id);
            hasher.update(&entry.effective_power.to_le_bytes());
        }

        Hash::from(*hasher.finalize().as_bytes())
    }

    /// Get relay weight (normalized)
    pub fn get_relay_weight(&self, relay_id: &[u8; 32]) -> Option<u64> {
        self.relay_weights
            .get(relay_id)
            .filter(|r| r.active)
            .map(|r| r.weight_bps)
    }

    /// Get stake power
    pub fn get_stake_power(&self, staker_id: &[u8; 32]) -> Option<u64> {
        self.stake_entries.get(staker_id).map(|s| s.effective_power)
    }

    /// Check if block is in this epoch
    pub fn contains_block(&self, block: u64) -> bool {
        block >= self.start_block && block <= self.end_block
    }
}

/// PoRW vote aggregator with threshold calculation
pub struct PoRWThresholdCalculator {
    /// Reference epoch snapshot
    snapshot: Arc<EpochSnapshot>,
    /// Votes received (relay_id -> vote)
    votes: HashMap<[u8; 32], bool>,
    /// Total voting weight collected
    total_voting_weight: u64,
    /// Positive voting weight
    positive_weight: u64,
}

impl PoRWThresholdCalculator {
    pub fn new(snapshot: Arc<EpochSnapshot>) -> Result<Self, ThresholdError> {
        if !snapshot.finalized {
            return Err(ThresholdError::SnapshotNotFinalized(snapshot.epoch));
        }

        Ok(Self {
            snapshot,
            votes: HashMap::new(),
            total_voting_weight: 0,
            positive_weight: 0,
        })
    }

    /// Add a vote from a relay
    pub fn add_vote(&mut self, relay_id: [u8; 32], vote: bool) -> Result<(), ThresholdError> {
        // Check relay is in snapshot
        let weight = self
            .snapshot
            .get_relay_weight(&relay_id)
            .ok_or(ThresholdError::RelayNotFound(relay_id))?;

        // Ignore duplicate votes
        if self.votes.contains_key(&relay_id) {
            return Ok(());
        }

        self.votes.insert(relay_id, vote);
        self.total_voting_weight += weight;

        if vote {
            self.positive_weight += weight;
        }

        Ok(())
    }

    /// Check if quorum is reached (67%)
    pub fn has_quorum(&self) -> bool {
        self.positive_weight * 10000 / self.snapshot.total_relay_weight >= PORW_QUORUM_BPS
    }

    /// Check participation threshold
    pub fn has_sufficient_participation(&self) -> bool {
        self.total_voting_weight * 10000 / self.snapshot.total_relay_weight >= MIN_PARTICIPATION_BPS
    }

    /// Get current approval percentage (basis points)
    pub fn approval_bps(&self) -> u64 {
        if self.total_voting_weight == 0 {
            return 0;
        }
        self.positive_weight * 10000 / self.total_voting_weight
    }

    /// Get participation percentage (basis points)
    pub fn participation_bps(&self) -> u64 {
        if self.snapshot.total_relay_weight == 0 {
            return 0;
        }
        self.total_voting_weight * 10000 / self.snapshot.total_relay_weight
    }

    /// Finalize and verify quorum
    pub fn finalize(&self) -> Result<PoRWQuorumResult, ThresholdError> {
        // Check participation
        if !self.has_sufficient_participation() {
            return Err(ThresholdError::InsufficientParticipation(
                self.participation_bps(),
                MIN_PARTICIPATION_BPS,
            ));
        }

        Ok(PoRWQuorumResult {
            epoch: self.snapshot.epoch,
            total_weight: self.snapshot.total_relay_weight,
            voting_weight: self.total_voting_weight,
            positive_weight: self.positive_weight,
            participation_bps: self.participation_bps(),
            approval_bps: self.approval_bps(),
            quorum_reached: self.has_quorum(),
            voter_count: self.votes.len() as u64,
        })
    }
}

/// PoRW quorum result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoRWQuorumResult {
    pub epoch: u64,
    pub total_weight: u64,
    pub voting_weight: u64,
    pub positive_weight: u64,
    pub participation_bps: u64,
    pub approval_bps: u64,
    pub quorum_reached: bool,
    pub voter_count: u64,
}

/// TSC vote aggregator with threshold calculation
pub struct TSCThresholdCalculator {
    /// Reference epoch snapshot
    snapshot: Arc<EpochSnapshot>,
    /// Votes received (staker_id -> vote)
    votes: HashMap<[u8; 32], bool>,
    /// Total voting power collected
    total_voting_power: u64,
    /// Positive voting power
    positive_power: u64,
}

impl TSCThresholdCalculator {
    pub fn new(snapshot: Arc<EpochSnapshot>) -> Result<Self, ThresholdError> {
        if !snapshot.finalized {
            return Err(ThresholdError::SnapshotNotFinalized(snapshot.epoch));
        }

        Ok(Self {
            snapshot,
            votes: HashMap::new(),
            total_voting_power: 0,
            positive_power: 0,
        })
    }

    /// Add a vote from a staker
    pub fn add_vote(&mut self, staker_id: [u8; 32], vote: bool) -> Result<(), ThresholdError> {
        // Check staker is in snapshot
        let power = self
            .snapshot
            .get_stake_power(&staker_id)
            .ok_or(ThresholdError::StakeNotFound(staker_id))?;

        // Ignore duplicate votes
        if self.votes.contains_key(&staker_id) {
            return Ok(());
        }

        self.votes.insert(staker_id, vote);
        self.total_voting_power += power;

        if vote {
            self.positive_power += power;
        }

        Ok(())
    }

    /// Check if quorum is reached (51%)
    pub fn has_quorum(&self) -> bool {
        self.positive_power * 10000 / self.snapshot.total_stake_power >= TSC_QUORUM_BPS
    }

    /// Check participation threshold
    pub fn has_sufficient_participation(&self) -> bool {
        self.total_voting_power * 10000 / self.snapshot.total_stake_power >= MIN_PARTICIPATION_BPS
    }

    /// Get current approval percentage (basis points)
    pub fn approval_bps(&self) -> u64 {
        if self.total_voting_power == 0 {
            return 0;
        }
        self.positive_power * 10000 / self.total_voting_power
    }

    /// Get participation percentage (basis points)
    pub fn participation_bps(&self) -> u64 {
        if self.snapshot.total_stake_power == 0 {
            return 0;
        }
        self.total_voting_power * 10000 / self.snapshot.total_stake_power
    }

    /// Finalize and verify quorum
    pub fn finalize(&self) -> Result<TSCQuorumResult, ThresholdError> {
        // Check participation
        if !self.has_sufficient_participation() {
            return Err(ThresholdError::InsufficientParticipation(
                self.participation_bps(),
                MIN_PARTICIPATION_BPS,
            ));
        }

        Ok(TSCQuorumResult {
            epoch: self.snapshot.epoch,
            total_power: self.snapshot.total_stake_power,
            voting_power: self.total_voting_power,
            positive_power: self.positive_power,
            participation_bps: self.participation_bps(),
            approval_bps: self.approval_bps(),
            quorum_reached: self.has_quorum(),
            voter_count: self.votes.len() as u64,
        })
    }
}

/// TSC quorum result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TSCQuorumResult {
    pub epoch: u64,
    pub total_power: u64,
    pub voting_power: u64,
    pub positive_power: u64,
    pub participation_bps: u64,
    pub approval_bps: u64,
    pub quorum_reached: bool,
    pub voter_count: u64,
}

/// Epoch snapshot manager
pub struct EpochSnapshotManager {
    /// Current epoch
    current_epoch: AtomicU64,
    /// Epoch snapshots
    snapshots: RwLock<HashMap<u64, Arc<EpochSnapshot>>>,
    /// Maximum snapshots to retain
    max_snapshots: usize,
    /// Current block
    current_block: AtomicU64,
}

impl EpochSnapshotManager {
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            current_epoch: AtomicU64::new(0),
            snapshots: RwLock::new(HashMap::new()),
            max_snapshots,
            current_block: AtomicU64::new(0),
        }
    }

    /// Get epoch for a block
    pub fn epoch_for_block(block: u64) -> u64 {
        block / EPOCH_LENGTH_BLOCKS
    }

    /// Get epoch start block
    pub fn epoch_start_block(epoch: u64) -> u64 {
        epoch * EPOCH_LENGTH_BLOCKS
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch.load(Ordering::Acquire)
    }

    /// Set current block (triggers epoch transition if needed)
    pub fn set_current_block(&self, block: u64) -> Option<u64> {
        self.current_block.store(block, Ordering::Release);

        let epoch = Self::epoch_for_block(block);
        let current = self.current_epoch.load(Ordering::Acquire);

        if epoch > current {
            self.current_epoch.store(epoch, Ordering::Release);
            Some(epoch)
        } else {
            None
        }
    }

    /// Get or create snapshot for epoch
    pub fn get_or_create_snapshot(&self, epoch: u64, previous_hash: Hash) -> Arc<EpochSnapshot> {
        // Check if exists
        {
            let snapshots = self.snapshots.read();
            if let Some(snapshot) = snapshots.get(&epoch) {
                return snapshot.clone();
            }
        }

        // Create new
        let snapshot = Arc::new(EpochSnapshot::new(
            epoch,
            Self::epoch_start_block(epoch),
            previous_hash,
        ));

        let mut snapshots = self.snapshots.write();
        snapshots.insert(epoch, snapshot.clone());

        // Prune old snapshots
        while snapshots.len() > self.max_snapshots {
            if let Some(&oldest) = snapshots.keys().min() {
                snapshots.remove(&oldest);
            }
        }

        snapshot
    }

    /// Get finalized snapshot for epoch
    pub fn get_finalized_snapshot(&self, epoch: u64) -> Option<Arc<EpochSnapshot>> {
        self.snapshots
            .read()
            .get(&epoch)
            .filter(|s| s.finalized)
            .cloned()
    }

    /// Finalize a snapshot
    pub fn finalize_snapshot(&self, epoch: u64) -> Result<Arc<EpochSnapshot>, ThresholdError> {
        let mut snapshots = self.snapshots.write();

        let snapshot = snapshots
            .get(&epoch)
            .ok_or(ThresholdError::EpochNotFound(epoch))?
            .clone();

        // Need to mutate through Arc - in production use Arc<RwLock<>>
        // For now, we create a new finalized snapshot
        let mut finalized = (*snapshot).clone();
        finalized.finalize();

        let finalized = Arc::new(finalized);
        snapshots.insert(epoch, finalized.clone());

        Ok(finalized)
    }

    /// Get the appropriate snapshot for threshold calculations
    /// (Uses previous epoch for stability during transitions)
    pub fn get_threshold_snapshot(&self, block: u64) -> Result<Arc<EpochSnapshot>, ThresholdError> {
        let block_epoch = Self::epoch_for_block(block);
        let block_in_epoch = block - Self::epoch_start_block(block_epoch);

        // During early blocks of epoch, use previous epoch snapshot
        if block_in_epoch < SNAPSHOT_FINALIZATION_DELAY && block_epoch > 0 {
            self.get_finalized_snapshot(block_epoch - 1)
                .ok_or(ThresholdError::EpochNotFound(block_epoch - 1))
        } else {
            self.get_finalized_snapshot(block_epoch)
                .ok_or(ThresholdError::SnapshotNotFinalized(block_epoch))
        }
    }

    /// Create PoRW calculator for a block
    pub fn create_porw_calculator(
        &self,
        block: u64,
    ) -> Result<PoRWThresholdCalculator, ThresholdError> {
        let snapshot = self.get_threshold_snapshot(block)?;
        PoRWThresholdCalculator::new(snapshot)
    }

    /// Create TSC calculator for a block
    pub fn create_tsc_calculator(
        &self,
        block: u64,
    ) -> Result<TSCThresholdCalculator, ThresholdError> {
        let snapshot = self.get_threshold_snapshot(block)?;
        TSCThresholdCalculator::new(snapshot)
    }
}

/// Combined threshold checker for block validation
pub struct CombinedThresholdChecker {
    epoch_manager: Arc<EpochSnapshotManager>,
}

impl CombinedThresholdChecker {
    pub fn new(epoch_manager: Arc<EpochSnapshotManager>) -> Self {
        Self { epoch_manager }
    }

    /// Validate block has sufficient PoRW support
    pub fn validate_porw_threshold(
        &self,
        block: u64,
        votes: &[([u8; 32], bool)],
    ) -> Result<PoRWQuorumResult, ThresholdError> {
        let mut calculator = self.epoch_manager.create_porw_calculator(block)?;

        for (relay_id, vote) in votes {
            calculator.add_vote(*relay_id, *vote)?;
        }

        let result = calculator.finalize()?;

        if !result.quorum_reached {
            return Err(ThresholdError::QuorumNotReached(
                result.approval_bps,
                PORW_QUORUM_BPS,
            ));
        }

        Ok(result)
    }

    /// Validate checkpoint has sufficient TSC support
    pub fn validate_tsc_threshold(
        &self,
        block: u64,
        votes: &[([u8; 32], bool)],
    ) -> Result<TSCQuorumResult, ThresholdError> {
        let mut calculator = self.epoch_manager.create_tsc_calculator(block)?;

        for (staker_id, vote) in votes {
            calculator.add_vote(*staker_id, *vote)?;
        }

        let result = calculator.finalize()?;

        if !result.quorum_reached {
            return Err(ThresholdError::QuorumNotReached(
                result.approval_bps,
                TSC_QUORUM_BPS,
            ));
        }

        Ok(result)
    }

    /// Combined validation (PoRW + TSC)
    pub fn validate_combined(
        &self,
        block: u64,
        porw_votes: &[([u8; 32], bool)],
        tsc_votes: &[([u8; 32], bool)],
    ) -> Result<CombinedQuorumResult, ThresholdError> {
        let porw_result = self.validate_porw_threshold(block, porw_votes)?;
        let tsc_result = self.validate_tsc_threshold(block, tsc_votes)?;

        Ok(CombinedQuorumResult {
            block,
            porw: porw_result,
            tsc: tsc_result,
        })
    }
}

/// Combined quorum result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinedQuorumResult {
    pub block: u64,
    pub porw: PoRWQuorumResult,
    pub tsc: TSCQuorumResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_relay_id(n: u8) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = n;
        id
    }

    fn test_staker_id(n: u8) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = n;
        id[31] = 0xFF; // Distinguish from relay IDs
        id
    }

    fn create_test_snapshot() -> EpochSnapshot {
        let mut snapshot = EpochSnapshot::new(0, 0, Hash::from([0u8; 32]));

        // Add 10 relays with equal weight
        for i in 0..10 {
            snapshot.set_relay_weight(test_relay_id(i), 100, true);
        }

        // Add 10 stakers with varying stake
        for i in 0..10 {
            snapshot.set_stake(
                test_staker_id(i),
                1000 * (i as u64 + 1), // 1000, 2000, ..., 10000
                100,                   // 1x multiplier
                0,
                52,
            );
        }

        snapshot.finalize();
        snapshot
    }

    #[test]
    fn test_epoch_snapshot_finalization() {
        let snapshot = create_test_snapshot();

        assert!(snapshot.finalized);
        assert_eq!(snapshot.active_relay_count, 10);
        assert_eq!(snapshot.staker_count, 10);
        assert_eq!(snapshot.total_relay_weight, 10000); // Normalized

        // Each relay should have 1000 bps (10%)
        for i in 0..10 {
            let weight = snapshot.get_relay_weight(&test_relay_id(i)).unwrap();
            assert_eq!(weight, 1000);
        }

        // Stake power should be sum: 1000+2000+...+10000 = 55000
        assert_eq!(snapshot.total_stake_power, 55000);
    }

    #[test]
    fn test_porw_threshold_67_percent() {
        let snapshot = Arc::new(create_test_snapshot());
        let mut calculator = PoRWThresholdCalculator::new(snapshot).unwrap();

        // 6 out of 10 = 60% (not enough)
        for i in 0..6 {
            calculator.add_vote(test_relay_id(i), true).unwrap();
        }

        assert!(!calculator.has_quorum());
        assert_eq!(calculator.approval_bps(), 10000); // All voters voted yes

        // 7 out of 10 = 70% (enough for 67%)
        calculator.add_vote(test_relay_id(6), true).unwrap();

        // 7000 bps participation, 7000 bps approval
        assert!(calculator.has_quorum());
    }

    #[test]
    fn test_tsc_threshold_51_percent() {
        let snapshot = Arc::new(create_test_snapshot());
        let mut calculator = TSCThresholdCalculator::new(snapshot).unwrap();

        // Total stake = 55000
        // 51% = 28050

        // Add votes from biggest stakers (10000 + 9000 + 8000 = 27000 < 28050)
        calculator.add_vote(test_staker_id(9), true).unwrap(); // 10000
        calculator.add_vote(test_staker_id(8), true).unwrap(); // 9000
        calculator.add_vote(test_staker_id(7), true).unwrap(); // 8000

        assert!(!calculator.has_quorum());

        // Add one more (+ 7000 = 34000 >= 28050)
        calculator.add_vote(test_staker_id(6), true).unwrap(); // 7000

        assert!(calculator.has_quorum());
    }

    #[test]
    fn test_participation_threshold() {
        let snapshot = Arc::new(create_test_snapshot());
        let mut calculator = PoRWThresholdCalculator::new(snapshot).unwrap();

        // Only 2 relays vote (20% participation < 33% required)
        calculator.add_vote(test_relay_id(0), true).unwrap();
        calculator.add_vote(test_relay_id(1), true).unwrap();

        assert!(!calculator.has_sufficient_participation());

        // Add 2 more (40% participation >= 33%)
        calculator.add_vote(test_relay_id(2), true).unwrap();
        calculator.add_vote(test_relay_id(3), true).unwrap();

        assert!(calculator.has_sufficient_participation());
    }

    #[test]
    fn test_epoch_manager() {
        let manager = EpochSnapshotManager::new(10);

        // Block 0 -> Epoch 0
        assert_eq!(EpochSnapshotManager::epoch_for_block(0), 0);
        assert_eq!(EpochSnapshotManager::epoch_for_block(1799), 0);
        assert_eq!(EpochSnapshotManager::epoch_for_block(1800), 1);

        // Create snapshot
        let snapshot = manager.get_or_create_snapshot(0, Hash::from([0u8; 32]));
        assert!(!snapshot.finalized);

        // Set current block triggers epoch transition
        let new_epoch = manager.set_current_block(1800);
        assert_eq!(new_epoch, Some(1));
        assert_eq!(manager.current_epoch(), 1);
    }

    #[test]
    fn test_duplicate_votes_ignored() {
        let snapshot = Arc::new(create_test_snapshot());
        let mut calculator = PoRWThresholdCalculator::new(snapshot).unwrap();

        // Vote once
        calculator.add_vote(test_relay_id(0), true).unwrap();
        let weight_after_first = calculator.positive_weight;

        // Vote again (should be ignored)
        calculator.add_vote(test_relay_id(0), true).unwrap();
        let weight_after_second = calculator.positive_weight;

        assert_eq!(weight_after_first, weight_after_second);
    }

    #[test]
    fn test_invalid_voter_rejected() {
        let snapshot = Arc::new(create_test_snapshot());
        let mut calculator = PoRWThresholdCalculator::new(snapshot).unwrap();

        // Non-existent relay
        let result = calculator.add_vote(test_relay_id(99), true);
        assert!(matches!(result, Err(ThresholdError::RelayNotFound(_))));
    }
}
