//! Epoch Snapshot Module
//!
//! Implements epoch boundary state capture for:
//! - Immutable stake/weight views for threshold calculations
//! - Protection against join/leave timing attacks
//! - Efficient state serialization and recovery
//! - Merkle-based integrity verification
//!
//! Security: Snapshots are taken at deterministic block heights
//! and become immutable after finalization delay.

use crate::block_hierarchy::Hash;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Blocks per epoch (approximately 1 hour at 2s blocks)
pub const BLOCKS_PER_EPOCH: u64 = 1800;

/// Finalization delay in blocks (snapshot becomes immutable)
pub const FINALIZATION_DELAY_BLOCKS: u64 = 10;

/// Maximum relay weight cap (basis points)
pub const MAX_RELAY_WEIGHT_BPS: u64 = 500; // 5%

/// Maximum staker power cap (basis points)
pub const MAX_STAKER_POWER_BPS: u64 = 500; // 5%

/// Minimum active relays for valid epoch
pub const MIN_ACTIVE_RELAYS: u64 = 10;

/// Minimum active stakers for valid epoch
pub const MIN_ACTIVE_STAKERS: u64 = 10;

/// Epoch snapshot errors
#[derive(Debug, Error)]
pub enum EpochError {
    #[error("Epoch {0} not found")]
    EpochNotFound(u64),
    
    #[error("Epoch {0} not finalized")]
    NotFinalized(u64),
    
    #[error("Epoch {0} already finalized")]
    AlreadyFinalized(u64),
    
    #[error("Invalid epoch transition: {0} -> {1}")]
    InvalidTransition(u64, u64),
    
    #[error("Insufficient relays: {0} < {1}")]
    InsufficientRelays(u64, u64),
    
    #[error("Insufficient stakers: {0} < {1}")]
    InsufficientStakers(u64, u64),
    
    #[error("Snapshot integrity check failed")]
    IntegrityCheckFailed,
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Geographic region for diversity tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Region {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Africa,
    Asia,
    Oceania,
    Unknown,
}

impl Region {
    pub fn from_country_code(code: &str) -> Self {
        match code.to_uppercase().as_str() {
            "US" | "CA" | "MX" => Region::NorthAmerica,
            "BR" | "AR" | "CL" | "CO" | "PE" | "VE" => Region::SouthAmerica,
            "GB" | "DE" | "FR" | "IT" | "ES" | "NL" | "SE" | "NO" | "FI" | "PL" | "CH" => Region::Europe,
            "ZA" | "NG" | "KE" | "EG" | "MA" => Region::Africa,
            "CN" | "JP" | "KR" | "IN" | "SG" | "HK" | "TW" | "TH" | "VN" | "ID" | "PH" | "MY" => Region::Asia,
            "AU" | "NZ" => Region::Oceania,
            _ => Region::Unknown,
        }
    }
}

/// Relay state in snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayState {
    /// Relay public key
    pub relay_id: [u8; 32],
    /// Raw delivery score (proofs submitted)
    pub delivery_score: u64,
    /// Uptime score (0-10000 bps)
    pub uptime_bps: u64,
    /// Region
    pub region: Region,
    /// Stake amount
    pub stake: u64,
    /// Is relay active?
    pub active: bool,
    /// Normalized weight (set during finalization)
    pub normalized_weight_bps: u64,
    /// Registration epoch
    pub registered_epoch: u64,
    /// Last activity epoch
    pub last_active_epoch: u64,
}

impl RelayState {
    /// Calculate raw weight before normalization
    pub fn raw_weight(&self) -> u64 {
        if !self.active {
            return 0;
        }
        
        // Weight = delivery_score * (uptime / 10000) * stake_factor
        let uptime_factor = self.uptime_bps;
        let stake_factor = (self.stake / 1_000_000_000).min(100); // Cap stake influence
        
        self.delivery_score
            .saturating_mul(uptime_factor)
            .saturating_mul(stake_factor.max(1))
            / 10000
    }
}

/// Staker state in snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakerState {
    /// Staker public key
    pub staker_id: [u8; 32],
    /// Base stake amount
    pub stake_amount: u64,
    /// Lockup duration in epochs
    pub lockup_epochs: u64,
    /// Epoch when stake was locked
    pub lockup_start_epoch: u64,
    /// Epoch when stake unlocks
    pub unlock_epoch: u64,
    /// Temporal multiplier (100 = 1x, up to 800 = 8x)
    pub temporal_multiplier: u64,
    /// Effective voting power
    pub effective_power: u64,
    /// Normalized power (set during finalization)
    pub normalized_power_bps: u64,
    /// Is stake active?
    pub active: bool,
}

impl StakerState {
    /// Calculate temporal multiplier based on lockup duration
    pub fn calculate_multiplier(lockup_epochs: u64) -> u64 {
        // 1 week = 168 epochs -> 1x
        // 1 month = 720 epochs -> 2x
        // 3 months = 2160 epochs -> 4x
        // 1 year = 8640 epochs -> 8x (max)
        match lockup_epochs {
            0..=167 => 100,
            168..=719 => 100 + (lockup_epochs - 168) * 100 / 552, // Interpolate to 2x
            720..=2159 => 200 + (lockup_epochs - 720) * 200 / 1440, // Interpolate to 4x
            2160..=8639 => 400 + (lockup_epochs - 2160) * 400 / 6480, // Interpolate to 8x
            _ => 800, // Max 8x
        }
    }
    
    /// Recalculate effective power
    pub fn recalculate_power(&mut self) {
        self.temporal_multiplier = Self::calculate_multiplier(self.lockup_epochs);
        self.effective_power = self.stake_amount
            .saturating_mul(self.temporal_multiplier)
            / 100;
    }
}

/// Complete epoch snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Epoch number
    pub epoch: u64,
    /// First block of epoch
    pub start_block: u64,
    /// Last block of epoch
    pub end_block: u64,
    /// Block at which snapshot was taken
    pub snapshot_block: u64,
    /// Timestamp of snapshot
    pub timestamp: u64,
    /// Previous epoch's merkle root (chain)
    pub previous_root: Hash,
    
    /// Relay states
    pub relays: BTreeMap<[u8; 32], RelayState>,
    /// Active relay count
    pub active_relay_count: u64,
    /// Total raw relay weight
    pub total_relay_weight: u64,
    
    /// Staker states
    pub stakers: BTreeMap<[u8; 32], StakerState>,
    /// Active staker count
    pub active_staker_count: u64,
    /// Total effective stake power
    pub total_stake_power: u64,
    
    /// Regional distribution
    pub region_counts: HashMap<Region, u64>,
    
    /// Is snapshot finalized?
    pub finalized: bool,
    /// Merkle root of snapshot data
    pub merkle_root: Hash,
}

impl Snapshot {
    /// Create new snapshot
    pub fn new(epoch: u64, previous_root: Hash) -> Self {
        let start_block = epoch * BLOCKS_PER_EPOCH;
        let end_block = start_block + BLOCKS_PER_EPOCH - 1;
        let snapshot_block = start_block + FINALIZATION_DELAY_BLOCKS;
        
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            epoch,
            start_block,
            end_block,
            snapshot_block,
            timestamp,
            previous_root,
            relays: BTreeMap::new(),
            active_relay_count: 0,
            total_relay_weight: 0,
            stakers: BTreeMap::new(),
            active_staker_count: 0,
            total_stake_power: 0,
            region_counts: HashMap::new(),
            finalized: false,
            merkle_root: Hash::default(),
        }
    }
    
    /// Add or update relay
    pub fn upsert_relay(&mut self, relay: RelayState) {
        let was_active = self.relays.get(&relay.relay_id)
            .map(|r| r.active)
            .unwrap_or(false);
        
        // Update region counts
        if relay.active && !was_active {
            *self.region_counts.entry(relay.region).or_insert(0) += 1;
            self.active_relay_count += 1;
        } else if !relay.active && was_active {
            if let Some(count) = self.region_counts.get_mut(&relay.region) {
                *count = count.saturating_sub(1);
            }
            self.active_relay_count = self.active_relay_count.saturating_sub(1);
        }
        
        self.relays.insert(relay.relay_id, relay);
    }
    
    /// Add or update staker
    pub fn upsert_staker(&mut self, mut staker: StakerState) {
        let was_active = self.stakers.get(&staker.staker_id)
            .map(|s| s.active)
            .unwrap_or(false);
        
        // Recalculate power
        staker.recalculate_power();
        
        if staker.active && !was_active {
            self.active_staker_count += 1;
        } else if !staker.active && was_active {
            self.active_staker_count = self.active_staker_count.saturating_sub(1);
        }
        
        self.stakers.insert(staker.staker_id, staker);
    }
    
    /// Finalize the snapshot
    pub fn finalize(&mut self) -> Result<(), EpochError> {
        if self.finalized {
            return Err(EpochError::AlreadyFinalized(self.epoch));
        }
        
        // Check minimum participants
        if self.active_relay_count < MIN_ACTIVE_RELAYS {
            return Err(EpochError::InsufficientRelays(
                self.active_relay_count,
                MIN_ACTIVE_RELAYS,
            ));
        }
        
        if self.active_staker_count < MIN_ACTIVE_STAKERS {
            return Err(EpochError::InsufficientStakers(
                self.active_staker_count,
                MIN_ACTIVE_STAKERS,
            ));
        }
        
        // Calculate total relay weight
        self.total_relay_weight = self.relays.values()
            .filter(|r| r.active)
            .map(|r| r.raw_weight())
            .sum();
        
        // Normalize relay weights with caps
        if self.total_relay_weight > 0 {
            for relay in self.relays.values_mut() {
                if relay.active {
                    let raw = relay.raw_weight();
                    let normalized = raw * 10000 / self.total_relay_weight;
                    relay.normalized_weight_bps = normalized.min(MAX_RELAY_WEIGHT_BPS);
                }
            }
            
            // Redistribute excess weight
            self.redistribute_relay_weight();
        }
        
        // Calculate total stake power
        self.total_stake_power = self.stakers.values()
            .filter(|s| s.active)
            .map(|s| s.effective_power)
            .sum();
        
        // Normalize staker power with caps
        if self.total_stake_power > 0 {
            for staker in self.stakers.values_mut() {
                if staker.active {
                    let normalized = staker.effective_power * 10000 / self.total_stake_power;
                    staker.normalized_power_bps = normalized.min(MAX_STAKER_POWER_BPS);
                }
            }
            
            // Redistribute excess power
            self.redistribute_staker_power();
        }
        
        // Compute merkle root
        self.merkle_root = self.compute_merkle_root();
        self.finalized = true;
        
        Ok(())
    }
    
    /// Redistribute capped relay weight
    fn redistribute_relay_weight(&mut self) {
        // Calculate excess weight from capped relays
        let mut excess: u64 = 0;
        let mut uncapped_total: u64 = 0;
        
        for relay in self.relays.values() {
            if relay.active {
                let raw = relay.raw_weight() * 10000 / self.total_relay_weight;
                if raw > MAX_RELAY_WEIGHT_BPS {
                    excess += raw - MAX_RELAY_WEIGHT_BPS;
                } else {
                    uncapped_total += raw;
                }
            }
        }
        
        // Redistribute proportionally to uncapped relays
        if excess > 0 && uncapped_total > 0 {
            for relay in self.relays.values_mut() {
                if relay.active && relay.normalized_weight_bps < MAX_RELAY_WEIGHT_BPS {
                    let share = relay.normalized_weight_bps * excess / uncapped_total;
                    relay.normalized_weight_bps = 
                        (relay.normalized_weight_bps + share).min(MAX_RELAY_WEIGHT_BPS);
                }
            }
        }
    }
    
    /// Redistribute capped staker power
    fn redistribute_staker_power(&mut self) {
        let mut excess: u64 = 0;
        let mut uncapped_total: u64 = 0;
        
        for staker in self.stakers.values() {
            if staker.active {
                let raw = staker.effective_power * 10000 / self.total_stake_power;
                if raw > MAX_STAKER_POWER_BPS {
                    excess += raw - MAX_STAKER_POWER_BPS;
                } else {
                    uncapped_total += raw;
                }
            }
        }
        
        if excess > 0 && uncapped_total > 0 {
            for staker in self.stakers.values_mut() {
                if staker.active && staker.normalized_power_bps < MAX_STAKER_POWER_BPS {
                    let share = staker.normalized_power_bps * excess / uncapped_total;
                    staker.normalized_power_bps = 
                        (staker.normalized_power_bps + share).min(MAX_STAKER_POWER_BPS);
                }
            }
        }
    }
    
    /// Compute merkle root of snapshot
    fn compute_merkle_root(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        
        // Hash metadata
        hasher.update(&self.epoch.to_le_bytes());
        hasher.update(&self.start_block.to_le_bytes());
        hasher.update(&self.snapshot_block.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.previous_root);
        
        // Hash relays (BTreeMap ensures deterministic order)
        for (id, relay) in &self.relays {
            hasher.update(id);
            hasher.update(&relay.normalized_weight_bps.to_le_bytes());
            hasher.update(&[relay.active as u8]);
        }
        
        // Hash stakers
        for (id, staker) in &self.stakers {
            hasher.update(id);
            hasher.update(&staker.normalized_power_bps.to_le_bytes());
            hasher.update(&[staker.active as u8]);
        }
        
        *hasher.finalize().as_bytes()
    }
    
    /// Verify snapshot integrity
    pub fn verify_integrity(&self) -> bool {
        if !self.finalized {
            return false;
        }
        
        let computed = self.compute_merkle_root();
        computed == self.merkle_root
    }
    
    /// Get relay weight (returns 0 if not found or inactive)
    pub fn get_relay_weight(&self, relay_id: &[u8; 32]) -> u64 {
        self.relays.get(relay_id)
            .filter(|r| r.active)
            .map(|r| r.normalized_weight_bps)
            .unwrap_or(0)
    }
    
    /// Get staker power (returns 0 if not found or inactive)
    pub fn get_staker_power(&self, staker_id: &[u8; 32]) -> u64 {
        self.stakers.get(staker_id)
            .filter(|s| s.active)
            .map(|s| s.normalized_power_bps)
            .unwrap_or(0)
    }
    
    /// Check regional diversity
    pub fn regional_diversity(&self) -> usize {
        self.region_counts.iter()
            .filter(|(_, &count)| count > 0)
            .count()
    }
    
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, EpochError> {
        bincode::serialize(self)
            .map_err(|e| EpochError::SerializationError(e.to_string()))
    }
    
    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, EpochError> {
        bincode::deserialize(data)
            .map_err(|e| EpochError::SerializationError(e.to_string()))
    }
}

/// Snapshot store with LRU eviction
pub struct SnapshotStore {
    /// Finalized snapshots
    snapshots: RwLock<HashMap<u64, Arc<Snapshot>>>,
    /// Maximum snapshots to keep
    max_snapshots: usize,
    /// Current epoch
    current_epoch: AtomicU64,
    /// Current block
    current_block: AtomicU64,
    /// In transition?
    in_transition: AtomicBool,
}

impl SnapshotStore {
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: RwLock::new(HashMap::new()),
            max_snapshots,
            current_epoch: AtomicU64::new(0),
            current_block: AtomicU64::new(0),
            in_transition: AtomicBool::new(false),
        }
    }
    
    /// Get epoch for block
    pub fn epoch_for_block(block: u64) -> u64 {
        block / BLOCKS_PER_EPOCH
    }
    
    /// Get block position within epoch
    pub fn block_in_epoch(block: u64) -> u64 {
        block % BLOCKS_PER_EPOCH
    }
    
    /// Is block in finalization window?
    pub fn is_finalization_block(block: u64) -> bool {
        Self::block_in_epoch(block) == FINALIZATION_DELAY_BLOCKS
    }
    
    /// Current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch.load(Ordering::Acquire)
    }
    
    /// Current block
    pub fn current_block(&self) -> u64 {
        self.current_block.load(Ordering::Acquire)
    }
    
    /// Process new block
    pub fn process_block(&self, block: u64) -> Option<u64> {
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
    
    /// Store finalized snapshot
    pub fn store(&self, snapshot: Snapshot) -> Result<(), EpochError> {
        if !snapshot.finalized {
            return Err(EpochError::NotFinalized(snapshot.epoch));
        }
        
        if !snapshot.verify_integrity() {
            return Err(EpochError::IntegrityCheckFailed);
        }
        
        let epoch = snapshot.epoch;
        
        {
            let mut snapshots = self.snapshots.write();
            snapshots.insert(epoch, Arc::new(snapshot));
            
            // Prune old snapshots
            while snapshots.len() > self.max_snapshots {
                if let Some(&oldest) = snapshots.keys().min() {
                    snapshots.remove(&oldest);
                }
            }
        }
        
        Ok(())
    }
    
    /// Get snapshot
    pub fn get(&self, epoch: u64) -> Option<Arc<Snapshot>> {
        self.snapshots.read().get(&epoch).cloned()
    }
    
    /// Get latest finalized snapshot
    pub fn latest(&self) -> Option<Arc<Snapshot>> {
        let snapshots = self.snapshots.read();
        snapshots.keys().max().and_then(|k| snapshots.get(k).cloned())
    }
    
    /// Get snapshot for threshold calculations at given block
    /// Uses previous epoch during finalization window
    pub fn get_for_threshold(&self, block: u64) -> Result<Arc<Snapshot>, EpochError> {
        let block_epoch = Self::epoch_for_block(block);
        let block_position = Self::block_in_epoch(block);
        
        // During finalization window, use previous epoch
        let use_epoch = if block_position < FINALIZATION_DELAY_BLOCKS && block_epoch > 0 {
            block_epoch - 1
        } else {
            block_epoch
        };
        
        self.get(use_epoch)
            .ok_or(EpochError::EpochNotFound(use_epoch))
    }
    
    /// Verify chain of snapshots
    pub fn verify_chain(&self, from_epoch: u64, to_epoch: u64) -> Result<(), EpochError> {
        let snapshots = self.snapshots.read();
        
        let mut current = to_epoch;
        while current > from_epoch {
            let snapshot = snapshots.get(&current)
                .ok_or(EpochError::EpochNotFound(current))?;
            
            if current > 0 {
                let prev = snapshots.get(&(current - 1))
                    .ok_or(EpochError::EpochNotFound(current - 1))?;
                
                if snapshot.previous_root != prev.merkle_root {
                    return Err(EpochError::IntegrityCheckFailed);
                }
            }
            
            current -= 1;
        }
        
        Ok(())
    }
    
    /// Get snapshot epochs in store
    pub fn epochs(&self) -> Vec<u64> {
        self.snapshots.read().keys().copied().collect()
    }
}

impl Default for SnapshotStore {
    fn default() -> Self {
        Self::new(100) // Keep 100 epochs (~4 days)
    }
}

/// Builder for creating snapshots from chain state
pub struct SnapshotBuilder {
    epoch: u64,
    previous_root: Hash,
    relays: Vec<RelayState>,
    stakers: Vec<StakerState>,
}

impl SnapshotBuilder {
    pub fn new(epoch: u64, previous_root: Hash) -> Self {
        Self {
            epoch,
            previous_root,
            relays: Vec::new(),
            stakers: Vec::new(),
        }
    }
    
    pub fn add_relay(&mut self, relay: RelayState) -> &mut Self {
        self.relays.push(relay);
        self
    }
    
    pub fn add_staker(&mut self, staker: StakerState) -> &mut Self {
        self.stakers.push(staker);
        self
    }
    
    pub fn build(self) -> Result<Snapshot, EpochError> {
        let mut snapshot = Snapshot::new(self.epoch, self.previous_root);
        
        for relay in self.relays {
            snapshot.upsert_relay(relay);
        }
        
        for staker in self.stakers {
            snapshot.upsert_staker(staker);
        }
        
        snapshot.finalize()?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn test_id(n: u8) -> [u8; 32] {
        let mut id = [0u8; 32];
        id[0] = n;
        id
    }
    
    fn create_test_relay(id: u8, active: bool) -> RelayState {
        RelayState {
            relay_id: test_id(id),
            delivery_score: 1000,
            uptime_bps: 9500, // 95%
            region: Region::NorthAmerica,
            stake: 10_000_000_000, // 10k DCHAT
            active,
            normalized_weight_bps: 0,
            registered_epoch: 0,
            last_active_epoch: 0,
        }
    }
    
    fn create_test_staker(id: u8, stake: u64, lockup_epochs: u64) -> StakerState {
        let mut staker = StakerState {
            staker_id: test_id(id),
            stake_amount: stake,
            lockup_epochs,
            lockup_start_epoch: 0,
            unlock_epoch: lockup_epochs,
            temporal_multiplier: 100,
            effective_power: stake,
            normalized_power_bps: 0,
            active: true,
        };
        staker.recalculate_power();
        staker
    }
    
    #[test]
    fn test_epoch_calculations() {
        assert_eq!(SnapshotStore::epoch_for_block(0), 0);
        assert_eq!(SnapshotStore::epoch_for_block(1799), 0);
        assert_eq!(SnapshotStore::epoch_for_block(1800), 1);
        
        assert_eq!(SnapshotStore::block_in_epoch(1810), 10);
        
        assert!(SnapshotStore::is_finalization_block(10));
        assert!(SnapshotStore::is_finalization_block(1810));
        assert!(!SnapshotStore::is_finalization_block(5));
    }
    
    #[test]
    fn test_temporal_multiplier() {
        // Short lockup = 1x
        assert_eq!(StakerState::calculate_multiplier(100), 100);
        
        // 1 week = still 1x
        assert_eq!(StakerState::calculate_multiplier(168), 100);
        
        // 1 month = 2x
        assert_eq!(StakerState::calculate_multiplier(720), 200);
        
        // 3 months = 4x
        assert_eq!(StakerState::calculate_multiplier(2160), 400);
        
        // 1 year = 8x
        assert_eq!(StakerState::calculate_multiplier(8640), 800);
        
        // Longer = still 8x (max)
        assert_eq!(StakerState::calculate_multiplier(20000), 800);
    }
    
    #[test]
    fn test_snapshot_finalization() {
        let mut snapshot = Snapshot::new(0, Hash::default());
        
        // Add enough relays
        for i in 0..MIN_ACTIVE_RELAYS + 5 {
            snapshot.upsert_relay(create_test_relay(i as u8, true));
        }
        
        // Add enough stakers
        for i in 0..MIN_ACTIVE_STAKERS + 5 {
            snapshot.upsert_staker(create_test_staker(
                (i + 100) as u8,
                1_000_000_000,
                720,
            ));
        }
        
        // Finalize
        snapshot.finalize().unwrap();
        
        assert!(snapshot.finalized);
        assert!(snapshot.verify_integrity());
        assert!(snapshot.total_relay_weight > 0);
        assert!(snapshot.total_stake_power > 0);
    }
    
    #[test]
    fn test_weight_caps() {
        let mut snapshot = Snapshot::new(0, Hash::default());
        
        // Add one huge relay and many small ones
        let mut big_relay = create_test_relay(0, true);
        big_relay.delivery_score = 100_000; // Much larger
        snapshot.upsert_relay(big_relay);
        
        for i in 1..MIN_ACTIVE_RELAYS + 1 {
            snapshot.upsert_relay(create_test_relay(i as u8, true));
        }
        
        // Add stakers
        for i in 0..MIN_ACTIVE_STAKERS + 1 {
            snapshot.upsert_staker(create_test_staker(
                (i + 100) as u8,
                1_000_000_000,
                720,
            ));
        }
        
        snapshot.finalize().unwrap();
        
        // Big relay should be capped at 5%
        let big_weight = snapshot.get_relay_weight(&test_id(0));
        assert!(big_weight <= MAX_RELAY_WEIGHT_BPS);
    }
    
    #[test]
    fn test_insufficient_participants() {
        let mut snapshot = Snapshot::new(0, Hash::default());
        
        // Add only a few relays
        for i in 0..5 {
            snapshot.upsert_relay(create_test_relay(i, true));
        }
        
        // Add enough stakers
        for i in 0..MIN_ACTIVE_STAKERS + 1 {
            snapshot.upsert_staker(create_test_staker(
                (i + 100) as u8,
                1_000_000_000,
                720,
            ));
        }
        
        let result = snapshot.finalize();
        assert!(matches!(result, Err(EpochError::InsufficientRelays(_, _))));
    }
    
    #[test]
    fn test_snapshot_store() {
        let store = SnapshotStore::new(10);
        
        // Create and store snapshot
        let mut snapshot = Snapshot::new(0, Hash::default());
        for i in 0..(MIN_ACTIVE_RELAYS + 1) as u8 {
            snapshot.upsert_relay(create_test_relay(i, true));
        }
        for i in 0..(MIN_ACTIVE_STAKERS + 1) as u8 {
            snapshot.upsert_staker(create_test_staker(i + 100, 1_000_000_000, 720));
        }
        snapshot.finalize().unwrap();
        
        store.store(snapshot.clone()).unwrap();
        
        // Retrieve
        let retrieved = store.get(0).unwrap();
        assert_eq!(retrieved.epoch, 0);
        assert!(retrieved.finalized);
        
        // Latest
        let latest = store.latest().unwrap();
        assert_eq!(latest.epoch, 0);
    }
    
    #[test]
    fn test_regional_diversity() {
        let mut snapshot = Snapshot::new(0, Hash::default());
        
        // Add relays from different regions
        let regions = [
            Region::NorthAmerica,
            Region::Europe,
            Region::Asia,
            Region::Oceania,
        ];
        
        for (i, region) in regions.iter().enumerate() {
            for j in 0..3 {
                let mut relay = create_test_relay((i * 3 + j) as u8, true);
                relay.region = *region;
                snapshot.upsert_relay(relay);
            }
        }
        
        assert_eq!(snapshot.regional_diversity(), 4);
    }
    
    #[test]
    fn test_serialization() {
        let mut snapshot = Snapshot::new(0, Hash::default());
        for i in 0..(MIN_ACTIVE_RELAYS + 1) as u8 {
            snapshot.upsert_relay(create_test_relay(i, true));
        }
        for i in 0..(MIN_ACTIVE_STAKERS + 1) as u8 {
            snapshot.upsert_staker(create_test_staker(i + 100, 1_000_000_000, 720));
        }
        snapshot.finalize().unwrap();
        
        // Serialize
        let bytes = snapshot.to_bytes().unwrap();
        
        // Deserialize
        let restored = Snapshot::from_bytes(&bytes).unwrap();
        
        assert_eq!(restored.epoch, snapshot.epoch);
        assert_eq!(restored.merkle_root, snapshot.merkle_root);
        assert!(restored.verify_integrity());
    }
    
    #[test]
    fn test_builder() {
        let mut builder = SnapshotBuilder::new(0, Hash::default());
        
        for i in 0..(MIN_ACTIVE_RELAYS + 1) as u8 {
            builder.add_relay(create_test_relay(i, true));
        }
        for i in 0..(MIN_ACTIVE_STAKERS + 1) as u8 {
            builder.add_staker(create_test_staker(i + 100, 1_000_000_000, 720));
        }
        
        let snapshot = builder.build().unwrap();
        
        assert!(snapshot.finalized);
        assert!(snapshot.verify_integrity());
    }
}
