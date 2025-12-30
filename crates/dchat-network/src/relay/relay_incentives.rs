//! Relay Incentives for QGE
//!
//! This module implements economic incentives for relay participation in the
//! Quorum-Gated Encryption system. Relays earn rewards for:
//!
//! - **Token Issuance**: Participating in threshold signing for epoch tokens
//! - **Message Routing**: Relaying encrypted messages between users
//! - **Uptime**: Maintaining high availability
//! - **Geographic Coverage**: Operating in underserved regions
//!
//! # Reward Structure
//!
//! Rewards are calculated per epoch (10 minutes) and distributed based on:
//! - Base reward for being online and responsive
//! - Per-token issuance fee
//! - Per-message relay fee
//! - Uptime bonus (multiplier for sustained availability)
//! - Geographic bonus for underserved regions
//!
//! # Slashing Conditions
//!
//! Relays can be slashed (lose stake) for:
//! - Signing invalid tokens
//! - Double-signing (signing conflicting data)
//! - Extended downtime without notice
//! - Malicious behavior (censorship, data manipulation)
//!
//! # Stake Requirements
//!
//! Relays must stake DCHAT tokens to participate:
//! - Minimum stake required for committee eligibility
//! - Higher stake increases selection probability
//! - Stake locked during active participation

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Minimum stake required to participate as relay (in smallest units)
pub const MIN_RELAY_STAKE: u64 = 10_000_000_000_000; // 10,000 DCHAT

/// Maximum stake counted for selection (prevents whale dominance)
pub const MAX_EFFECTIVE_STAKE: u64 = 100_000_000_000_000; // 100,000 DCHAT

/// Base reward per epoch (in smallest units)
pub const BASE_EPOCH_REWARD: u64 = 1_000_000_000; // 1 DCHAT

/// Reward per token issuance (in smallest units)
pub const TOKEN_ISSUANCE_REWARD: u64 = 10_000_000; // 0.01 DCHAT

/// Reward per message relayed (in smallest units)
pub const MESSAGE_RELAY_REWARD: u64 = 1_000_000; // 0.001 DCHAT

/// Maximum uptime bonus multiplier (2x)
pub const MAX_UPTIME_BONUS: f64 = 2.0;

/// Epochs of sustained uptime for max bonus
pub const UPTIME_BONUS_EPOCHS: u64 = 4320; // 30 days

/// Slashing percentage for minor offense
pub const MINOR_SLASH_PERCENT: u64 = 1;

/// Slashing percentage for major offense (e.g., double signing)
pub const MAJOR_SLASH_PERCENT: u64 = 10;

/// Slashing percentage for critical offense (e.g., key compromise)
pub const CRITICAL_SLASH_PERCENT: u64 = 100;

/// Minimum uptime percentage for rewards
pub const MIN_UPTIME_PERCENT: f64 = 0.95;

/// Geographic regions for diversity bonuses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeoRegion {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Africa,
    MiddleEast,
    Asia,
    Oceania,
    Unknown,
}

impl GeoRegion {
    /// Get diversity bonus multiplier for underserved regions
    pub fn diversity_bonus(&self) -> f64 {
        match self {
            // Underserved regions get bonus
            GeoRegion::Africa => 1.5,
            GeoRegion::SouthAmerica => 1.3,
            GeoRegion::MiddleEast => 1.3,
            GeoRegion::Oceania => 1.2,
            // Well-served regions - no bonus
            GeoRegion::NorthAmerica => 1.0,
            GeoRegion::Europe => 1.0,
            GeoRegion::Asia => 1.0,
            GeoRegion::Unknown => 0.8, // Penalty for unknown location
        }
    }
}

/// Relay performance metrics for an epoch
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EpochPerformance {
    /// Epoch ID
    pub epoch_id: u64,
    /// Tokens signed
    pub tokens_signed: u64,
    /// Messages relayed
    pub messages_relayed: u64,
    /// Uptime percentage (0.0 - 1.0)
    pub uptime: f64,
    /// Response latency (median, ms)
    pub median_latency_ms: u64,
    /// Failed requests
    pub failed_requests: u64,
    /// Was in active committee
    pub in_committee: bool,
}

/// Reward breakdown for an epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochReward {
    /// Epoch ID
    pub epoch_id: u64,
    /// Base reward
    pub base_reward: u64,
    /// Token issuance rewards
    pub token_rewards: u64,
    /// Message relay rewards
    pub message_rewards: u64,
    /// Uptime bonus amount
    pub uptime_bonus: u64,
    /// Geographic bonus amount
    pub geo_bonus: u64,
    /// Total reward
    pub total: u64,
    /// Calculation timestamp
    pub calculated_at: u64,
}

impl EpochReward {
    fn zero(epoch_id: u64) -> Self {
        Self {
            epoch_id,
            base_reward: 0,
            token_rewards: 0,
            message_rewards: 0,
            uptime_bonus: 0,
            geo_bonus: 0,
            total: 0,
            calculated_at: current_timestamp(),
        }
    }
}

/// Slashing event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvent {
    /// Unique ID
    pub id: [u8; 32],
    /// Relay that was slashed
    pub relay_id: [u8; 32],
    /// Reason for slashing
    pub reason: SlashingReason,
    /// Amount slashed
    pub amount: u64,
    /// Evidence hash
    pub evidence_hash: [u8; 32],
    /// Timestamp
    pub timestamp: u64,
    /// Reported by
    pub reporter: Option<[u8; 32]>,
}

/// Reason for slashing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlashingReason {
    /// Signed invalid token
    InvalidTokenSignature { token_hash: [u8; 32] },
    /// Double-signed (same epoch, different data)
    DoubleSigning {
        epoch_id: u64,
        signature1_hash: [u8; 32],
        signature2_hash: [u8; 32],
    },
    /// Extended downtime without notice
    UnexcusedDowntime { missing_epochs: u64 },
    /// Censoring specific users
    Censorship { evidence_hash: [u8; 32] },
    /// Data manipulation
    DataManipulation { evidence_hash: [u8; 32] },
    /// Other
    Other { description: String },
}

impl SlashingReason {
    pub fn slash_percent(&self) -> u64 {
        match self {
            SlashingReason::InvalidTokenSignature { .. } => MINOR_SLASH_PERCENT,
            SlashingReason::UnexcusedDowntime { missing_epochs } => {
                // 1% per day of downtime, max 10%
                (missing_epochs / 144).min(10)
            }
            SlashingReason::DoubleSigning { .. } => MAJOR_SLASH_PERCENT,
            SlashingReason::Censorship { .. } => MAJOR_SLASH_PERCENT,
            SlashingReason::DataManipulation { .. } => CRITICAL_SLASH_PERCENT,
            SlashingReason::Other { .. } => MINOR_SLASH_PERCENT,
        }
    }
}

/// Relay stake info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayStake {
    /// Relay ID
    pub relay_id: [u8; 32],
    /// Total staked amount
    pub staked_amount: u64,
    /// Locked until (for unstaking)
    pub locked_until: Option<u64>,
    /// Geographic region
    pub region: GeoRegion,
    /// Consecutive epochs of uptime
    pub uptime_streak: u64,
    /// Total rewards earned
    pub total_rewards: u64,
    /// Total slashed
    pub total_slashed: u64,
    /// Active since timestamp
    pub active_since: u64,
}

impl RelayStake {
    pub fn new(relay_id: [u8; 32], staked_amount: u64, region: GeoRegion) -> Self {
        Self {
            relay_id,
            staked_amount,
            locked_until: None,
            region,
            uptime_streak: 0,
            total_rewards: 0,
            total_slashed: 0,
            active_since: current_timestamp(),
        }
    }

    /// Get effective stake (capped at max)
    pub fn effective_stake(&self) -> u64 {
        self.staked_amount.min(MAX_EFFECTIVE_STAKE)
    }

    /// Calculate uptime bonus multiplier
    pub fn uptime_multiplier(&self) -> f64 {
        let progress = (self.uptime_streak as f64) / (UPTIME_BONUS_EPOCHS as f64);
        1.0 + (MAX_UPTIME_BONUS - 1.0) * progress.min(1.0)
    }

    /// Is eligible for committee
    pub fn is_eligible(&self) -> bool {
        self.staked_amount >= MIN_RELAY_STAKE && self.total_slashed < self.staked_amount
    }
}

/// Pending reward claim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingReward {
    /// Relay ID
    pub relay_id: [u8; 32],
    /// Epoch range
    pub from_epoch: u64,
    pub to_epoch: u64,
    /// Total amount
    pub amount: u64,
    /// Claim expiry
    pub expires_at: u64,
}

/// Relay incentives manager
pub struct RelayIncentivesManager {
    /// Relay stakes
    stakes: Arc<RwLock<HashMap<[u8; 32], RelayStake>>>,
    /// Performance history per relay
    performance_history: Arc<RwLock<HashMap<[u8; 32], VecDeque<EpochPerformance>>>>,
    /// Reward history per relay
    reward_history: Arc<RwLock<HashMap<[u8; 32], VecDeque<EpochReward>>>>,
    /// Pending rewards
    pending_rewards: Arc<RwLock<HashMap<[u8; 32], PendingReward>>>,
    /// Slashing history
    slashing_history: Arc<RwLock<VecDeque<SlashingEvent>>>,
    /// Region statistics
    region_stats: Arc<RwLock<HashMap<GeoRegion, usize>>>,
}

impl RelayIncentivesManager {
    /// Create a new incentives manager
    pub fn new() -> Self {
        Self {
            stakes: Arc::new(RwLock::new(HashMap::new())),
            performance_history: Arc::new(RwLock::new(HashMap::new())),
            reward_history: Arc::new(RwLock::new(HashMap::new())),
            pending_rewards: Arc::new(RwLock::new(HashMap::new())),
            slashing_history: Arc::new(RwLock::new(VecDeque::new())),
            region_stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a relay stake
    pub async fn register_stake(
        &self,
        relay_id: [u8; 32],
        amount: u64,
        region: GeoRegion,
    ) -> Result<()> {
        if amount < MIN_RELAY_STAKE {
            return Err(Error::network(format!(
                "Stake {} below minimum {}",
                amount, MIN_RELAY_STAKE
            )));
        }

        let mut stakes = self.stakes.write().await;
        let stake = stakes
            .entry(relay_id)
            .or_insert_with(|| RelayStake::new(relay_id, 0, region));

        stake.staked_amount += amount;
        stake.region = region;

        // Update region stats
        let mut regions = self.region_stats.write().await;
        *regions.entry(region).or_insert(0) += 1;

        Ok(())
    }

    /// Record epoch performance
    pub async fn record_performance(
        &self,
        relay_id: [u8; 32],
        performance: EpochPerformance,
    ) -> Result<()> {
        // Update stake uptime streak
        {
            let mut stakes = self.stakes.write().await;
            if let Some(stake) = stakes.get_mut(&relay_id) {
                if performance.uptime >= MIN_UPTIME_PERCENT {
                    stake.uptime_streak += 1;
                } else {
                    stake.uptime_streak = 0;
                }
            }
        }

        // Record performance
        let mut history = self.performance_history.write().await;
        let relay_history = history.entry(relay_id).or_insert_with(VecDeque::new);
        relay_history.push_back(performance);

        // Keep last 1000 epochs
        while relay_history.len() > 1000 {
            relay_history.pop_front();
        }

        Ok(())
    }

    /// Calculate reward for an epoch
    pub async fn calculate_epoch_reward(
        &self,
        relay_id: [u8; 32],
        epoch_id: u64,
    ) -> Result<EpochReward> {
        let stakes = self.stakes.read().await;
        let stake = stakes
            .get(&relay_id)
            .ok_or_else(|| Error::network("Relay not staked"))?;

        if !stake.is_eligible() {
            return Ok(EpochReward::zero(epoch_id));
        }

        // Get performance for this epoch
        let history = self.performance_history.read().await;
        let performance = history
            .get(&relay_id)
            .and_then(|h| h.iter().find(|p| p.epoch_id == epoch_id))
            .cloned()
            .unwrap_or_default();

        // Check minimum uptime
        if performance.uptime < MIN_UPTIME_PERCENT {
            return Ok(EpochReward::zero(epoch_id));
        }

        // Calculate base components
        let base_reward = if performance.in_committee {
            BASE_EPOCH_REWARD
        } else {
            0
        };

        let token_rewards = performance.tokens_signed * TOKEN_ISSUANCE_REWARD;
        let message_rewards = performance.messages_relayed * MESSAGE_RELAY_REWARD;

        let subtotal = base_reward + token_rewards + message_rewards;

        // Apply bonuses
        let uptime_multiplier = stake.uptime_multiplier();
        let geo_multiplier = stake.region.diversity_bonus();

        let uptime_bonus = ((subtotal as f64) * (uptime_multiplier - 1.0)) as u64;
        let geo_bonus = ((subtotal as f64) * (geo_multiplier - 1.0)) as u64;

        let total = subtotal + uptime_bonus + geo_bonus;

        Ok(EpochReward {
            epoch_id,
            base_reward,
            token_rewards,
            message_rewards,
            uptime_bonus,
            geo_bonus,
            total,
            calculated_at: current_timestamp(),
        })
    }

    /// Process rewards for completed epoch
    pub async fn process_epoch_rewards(&self, epoch_id: u64) -> Result<Vec<EpochReward>> {
        let stakes = self.stakes.read().await;
        let relay_ids: Vec<_> = stakes.keys().copied().collect();
        drop(stakes);

        let mut rewards = Vec::new();

        for relay_id in relay_ids {
            let reward = self.calculate_epoch_reward(relay_id, epoch_id).await?;
            if reward.total > 0 {
                // Add to reward history
                let mut history = self.reward_history.write().await;
                let relay_history = history.entry(relay_id).or_insert_with(VecDeque::new);
                relay_history.push_back(reward.clone());
                while relay_history.len() > 1000 {
                    relay_history.pop_front();
                }

                // Update stake totals
                let mut stakes = self.stakes.write().await;
                if let Some(stake) = stakes.get_mut(&relay_id) {
                    stake.total_rewards += reward.total;
                }

                rewards.push(reward);
            }
        }

        Ok(rewards)
    }

    /// Report a slashable offense
    pub async fn report_offense(
        &self,
        relay_id: [u8; 32],
        reason: SlashingReason,
        evidence_hash: [u8; 32],
        reporter: Option<[u8; 32]>,
    ) -> Result<SlashingEvent> {
        let mut stakes = self.stakes.write().await;
        let stake = stakes
            .get_mut(&relay_id)
            .ok_or_else(|| Error::network("Relay not staked"))?;

        let slash_percent = reason.slash_percent();
        let slash_amount = (stake.staked_amount * slash_percent) / 100;

        stake.total_slashed += slash_amount;

        // Generate event ID
        let mut id = [0u8; 32];
        let id_data = blake3::hash(
            &[
                &relay_id[..],
                &evidence_hash[..],
                &current_timestamp().to_le_bytes()[..],
            ]
            .concat(),
        );
        id.copy_from_slice(id_data.as_bytes());

        let event = SlashingEvent {
            id,
            relay_id,
            reason,
            amount: slash_amount,
            evidence_hash,
            timestamp: current_timestamp(),
            reporter,
        };

        // Record slashing
        let mut slashing = self.slashing_history.write().await;
        slashing.push_back(event.clone());
        while slashing.len() > 10000 {
            slashing.pop_front();
        }

        Ok(event)
    }

    /// Get relay stats
    pub async fn get_relay_stats(&self, relay_id: [u8; 32]) -> Option<RelayIncentiveStats> {
        let stakes = self.stakes.read().await;
        let stake = stakes.get(&relay_id)?;

        let history = self.reward_history.read().await;
        let rewards = history.get(&relay_id);

        let total_epochs = rewards.map(|r| r.len()).unwrap_or(0);
        let recent_rewards: u64 = rewards
            .map(|r| r.iter().rev().take(144).map(|e| e.total).sum()) // Last 24h
            .unwrap_or(0);

        Some(RelayIncentiveStats {
            relay_id,
            staked_amount: stake.staked_amount,
            effective_stake: stake.effective_stake(),
            uptime_streak: stake.uptime_streak,
            uptime_multiplier: stake.uptime_multiplier(),
            geo_multiplier: stake.region.diversity_bonus(),
            total_rewards: stake.total_rewards,
            total_slashed: stake.total_slashed,
            total_epochs_active: total_epochs,
            rewards_last_24h: recent_rewards,
            is_eligible: stake.is_eligible(),
        })
    }

    /// Get network-wide stats
    pub async fn get_network_stats(&self) -> NetworkIncentiveStats {
        let stakes = self.stakes.read().await;
        let regions = self.region_stats.read().await;
        let slashing = self.slashing_history.read().await;

        let total_staked: u64 = stakes.values().map(|s| s.staked_amount).sum();
        let total_rewards: u64 = stakes.values().map(|s| s.total_rewards).sum();
        let total_slashed: u64 = stakes.values().map(|s| s.total_slashed).sum();
        let active_relays = stakes.values().filter(|s| s.is_eligible()).count();

        NetworkIncentiveStats {
            total_staked,
            total_rewards_distributed: total_rewards,
            total_slashed,
            active_relays,
            total_relays: stakes.len(),
            region_distribution: regions.clone(),
            slashing_events_24h: slashing
                .iter()
                .filter(|e| current_timestamp().saturating_sub(e.timestamp) < 86400)
                .count(),
        }
    }

    /// Start reward processing task
    pub fn start_reward_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            const EPOCH_DURATION_SECS: u64 = 600;
            let mut last_epoch = current_timestamp() / EPOCH_DURATION_SECS;

            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;

                let current_epoch = current_timestamp() / EPOCH_DURATION_SECS;
                if current_epoch > last_epoch {
                    // Process rewards for completed epoch
                    if let Err(e) = self.process_epoch_rewards(last_epoch).await {
                        tracing::error!("Failed to process epoch rewards: {}", e);
                    }
                    last_epoch = current_epoch;
                }
            }
        })
    }
}

impl Default for RelayIncentivesManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Relay incentive statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayIncentiveStats {
    pub relay_id: [u8; 32],
    pub staked_amount: u64,
    pub effective_stake: u64,
    pub uptime_streak: u64,
    pub uptime_multiplier: f64,
    pub geo_multiplier: f64,
    pub total_rewards: u64,
    pub total_slashed: u64,
    pub total_epochs_active: usize,
    pub rewards_last_24h: u64,
    pub is_eligible: bool,
}

/// Network-wide incentive statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkIncentiveStats {
    pub total_staked: u64,
    pub total_rewards_distributed: u64,
    pub total_slashed: u64,
    pub active_relays: usize,
    pub total_relays: usize,
    pub region_distribution: HashMap<GeoRegion, usize>,
    pub slashing_events_24h: usize,
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stake_registration() {
        let manager = RelayIncentivesManager::new();
        let relay_id = [1u8; 32];

        manager
            .register_stake(relay_id, MIN_RELAY_STAKE, GeoRegion::Africa)
            .await
            .unwrap();

        let stats = manager.get_relay_stats(relay_id).await.unwrap();
        assert_eq!(stats.staked_amount, MIN_RELAY_STAKE);
        assert!(stats.is_eligible);
        assert_eq!(stats.geo_multiplier, 1.5); // Africa bonus
    }

    #[tokio::test]
    async fn test_stake_below_minimum() {
        let manager = RelayIncentivesManager::new();
        let relay_id = [1u8; 32];

        let result = manager
            .register_stake(relay_id, MIN_RELAY_STAKE - 1, GeoRegion::NorthAmerica)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_uptime_streak_bonus() {
        let manager = RelayIncentivesManager::new();
        let relay_id = [1u8; 32];

        manager
            .register_stake(relay_id, MIN_RELAY_STAKE, GeoRegion::Europe)
            .await
            .unwrap();

        // Record good performance for many epochs
        for epoch in 0..100 {
            manager
                .record_performance(
                    relay_id,
                    EpochPerformance {
                        epoch_id: epoch,
                        uptime: 0.99,
                        tokens_signed: 10,
                        messages_relayed: 100,
                        in_committee: true,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
        }

        let stats = manager.get_relay_stats(relay_id).await.unwrap();
        assert_eq!(stats.uptime_streak, 100);
        assert!(stats.uptime_multiplier > 1.0);
    }

    #[tokio::test]
    async fn test_slashing() {
        let manager = RelayIncentivesManager::new();
        let relay_id = [1u8; 32];

        manager
            .register_stake(relay_id, MIN_RELAY_STAKE, GeoRegion::Asia)
            .await
            .unwrap();

        let event = manager
            .report_offense(
                relay_id,
                SlashingReason::DoubleSigning {
                    epoch_id: 100,
                    signature1_hash: [0xAA; 32],
                    signature2_hash: [0xBB; 32],
                },
                [0xCC; 32],
                Some([2u8; 32]),
            )
            .await
            .unwrap();

        assert_eq!(event.amount, MIN_RELAY_STAKE * MAJOR_SLASH_PERCENT / 100);

        let stats = manager.get_relay_stats(relay_id).await.unwrap();
        assert_eq!(stats.total_slashed, event.amount);
    }

    #[tokio::test]
    async fn test_effective_stake_cap() {
        let manager = RelayIncentivesManager::new();
        let relay_id = [1u8; 32];

        // Stake way above max
        let large_stake = MAX_EFFECTIVE_STAKE * 2;
        manager
            .register_stake(relay_id, large_stake, GeoRegion::Europe)
            .await
            .unwrap();

        let stats = manager.get_relay_stats(relay_id).await.unwrap();
        assert_eq!(stats.staked_amount, large_stake);
        assert_eq!(stats.effective_stake, MAX_EFFECTIVE_STAKE);
    }

    #[test]
    fn test_geo_diversity_bonus() {
        assert_eq!(GeoRegion::Africa.diversity_bonus(), 1.5);
        assert_eq!(GeoRegion::SouthAmerica.diversity_bonus(), 1.3);
        assert_eq!(GeoRegion::Europe.diversity_bonus(), 1.0);
        assert_eq!(GeoRegion::Unknown.diversity_bonus(), 0.8);
    }
}
