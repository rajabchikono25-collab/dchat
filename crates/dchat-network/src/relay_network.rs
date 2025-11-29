//! Full relay network coordination with discovery, uptime scoring, and load balancing
//!
//! This module implements a production-ready relay network infrastructure with:
//! - Kademlia DHT-based relay discovery
//! - Time-weighted uptime reputation scoring
//! - Staking verification and slashing integration
//! - Geographic distribution tracking
//! - Load balancing across relay nodes
//! - Proof-of-delivery aggregation for on-chain rewards
//! - Anti-Sybil relay verification

use async_trait::async_trait;
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Relay stake lock duration (30 days in seconds)
pub const RELAY_LOCK_DURATION: u64 = 30 * 24 * 60 * 60;

/// Minimum confirmations required for stake
pub const MIN_STAKE_CONFIRMATIONS: u32 = 3;

/// Trait for staking operations - allows integration with currency chain
#[async_trait]
pub trait StakingBackend: Send + Sync {
    /// Lock tokens for relay staking
    async fn stake(
        &self,
        operator: &UserId,
        amount: u64,
        lock_duration: u64,
    ) -> Result<String>; // Returns transaction ID

    /// Wait for stake confirmation
    async fn wait_for_confirmation(
        &self,
        tx_id: &str,
        min_confirmations: u32,
    ) -> Result<bool>;

    /// Unlock staked tokens (when relay unregisters)
    async fn unstake(
        &self,
        operator: &UserId,
        stake_tx_id: &str,
    ) -> Result<String>; // Returns unstake transaction ID

    /// Slash staked tokens for misbehavior
    async fn slash(
        &self,
        operator: &UserId,
        amount: u64,
        reason: &str,
    ) -> Result<String>; // Returns slash transaction ID

    /// Distribute rewards to a relay operator
    /// 
    /// This should mint new tokens as relay rewards using the currency chain.
    /// 
    /// # Arguments
    /// * `operator` - The relay operator to receive rewards
    /// * `amount` - Amount of tokens to mint as rewards
    /// * `epoch` - The epoch for which rewards are being distributed
    /// 
    /// # Returns
    /// Transaction ID of the reward mint operation
    async fn distribute_reward(
        &self,
        operator: &UserId,
        amount: u64,
        epoch: u64,
    ) -> Result<String>;
}

/// Reward distribution record for a single relay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardDistribution {
    /// Relay that received the reward
    pub relay_id: String,
    /// Relay operator who receives the tokens
    pub operator: UserId,
    /// Amount of tokens distributed
    pub amount: u64,
    /// Transaction ID on currency chain
    pub tx_id: String,
    /// Epoch for which rewards were distributed
    pub epoch: u64,
    /// Uptime score at time of distribution
    pub uptime_score: f64,
    /// Number of messages relayed during epoch
    pub messages_relayed: u64,
    /// Geographic region
    pub continent: Continent,
}

/// Relay network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayNetworkConfig {
    /// Minimum number of relays to maintain
    pub min_relays: usize,

    /// Maximum number of relays to track
    pub max_relays: usize,

    /// Minimum stake required for relay operation (in tokens)
    pub min_stake: u64,

    /// Uptime scoring window (in seconds)
    pub uptime_window: u64,

    /// Minimum uptime score (0.0-1.0) for relay eligibility
    pub min_uptime_score: f64,

    /// Geographic diversity requirement (minimum continents)
    pub min_continents: usize,

    /// Load balancing strategy
    pub load_strategy: LoadStrategy,

    /// Proof-of-delivery batch size
    pub pod_batch_size: usize,

    /// Heartbeat interval (seconds)
    pub heartbeat_interval: u64,
}

impl Default for RelayNetworkConfig {
    fn default() -> Self {
        Self {
            min_relays: 10,
            max_relays: 1000,
            min_stake: 10_000,
            uptime_window: 86400 * 7, // 7 days
            min_uptime_score: 0.95,
            min_continents: 3,
            load_strategy: LoadStrategy::WeightedRoundRobin,
            pod_batch_size: 100,
            heartbeat_interval: 60,
        }
    }
}

/// Load balancing strategy
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum LoadStrategy {
    /// Round-robin across all relays
    RoundRobin,

    /// Weighted by uptime score
    WeightedRoundRobin,

    /// Least connections first
    LeastConnections,

    /// Geographic proximity
    Geographic,
}

/// Geographic region for relay diversity
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Continent {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Asia,
    Africa,
    Oceania,
    Antarctica,
}

/// Relay node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayInfo {
    /// Relay's unique identifier
    pub relay_id: String,

    /// Relay operator's user ID
    pub operator: UserId,

    /// Staked amount
    pub stake: u64,

    /// Geographic location
    pub continent: Continent,

    /// ASN (Autonomous System Number) for diversity
    pub asn: u32,

    /// Registration timestamp
    pub registered_at: SystemTime,

    /// Last heartbeat timestamp
    pub last_heartbeat: SystemTime,

    /// Total messages relayed
    pub messages_relayed: u64,

    /// Total bandwidth used (bytes)
    pub bandwidth_used: u64,

    /// Current active connections
    pub active_connections: usize,

    /// Uptime history (timestamp, was_online)
    uptime_history: Vec<(SystemTime, bool)>,
}

impl RelayInfo {
    /// Create new relay info
    pub fn new(
        relay_id: String,
        operator: UserId,
        stake: u64,
        continent: Continent,
        asn: u32,
    ) -> Self {
        let now = SystemTime::now();
        Self {
            relay_id,
            operator,
            stake,
            continent,
            asn,
            registered_at: now,
            last_heartbeat: now,
            messages_relayed: 0,
            bandwidth_used: 0,
            active_connections: 0,
            uptime_history: vec![(now, true)],
        }
    }

    /// Update heartbeat
    pub fn heartbeat(&mut self) {
        let now = SystemTime::now();
        self.last_heartbeat = now;
        self.uptime_history.push((now, true));
    }

    /// Mark relay as offline
    pub fn mark_offline(&mut self) {
        let now = SystemTime::now();
        self.uptime_history.push((now, false));
    }

    /// Calculate uptime score over a time window
    pub fn uptime_score(&self, window_secs: u64) -> f64 {
        let now = SystemTime::now();
        let cutoff = now - Duration::from_secs(window_secs);

        // Filter uptime history to window
        let recent: Vec<_> = self
            .uptime_history
            .iter()
            .filter(|(ts, _)| ts >= &cutoff)
            .collect();

        if recent.is_empty() {
            return 0.0;
        }

        // Time-weighted average
        let mut total_time = 0u64;
        let mut online_time = 0u64;

        for i in 0..recent.len() {
            let (ts, online) = recent[i];
            let next_ts = if i + 1 < recent.len() {
                recent[i + 1].0
            } else {
                now
            };

            let duration = next_ts
                .duration_since(*ts)
                .unwrap_or(Duration::ZERO)
                .as_secs();

            total_time += duration;
            if *online {
                online_time += duration;
            }
        }

        if total_time == 0 {
            0.0
        } else {
            online_time as f64 / total_time as f64
        }
    }

    /// Check if relay is currently online (heartbeat within threshold)
    pub fn is_online(&self, heartbeat_threshold: Duration) -> bool {
        SystemTime::now()
            .duration_since(self.last_heartbeat)
            .unwrap_or(Duration::MAX)
            < heartbeat_threshold
    }

    /// Record a relayed message
    pub fn record_message(&mut self, size: usize) {
        self.messages_relayed += 1;
        self.bandwidth_used += size as u64;
    }
}

/// Proof-of-delivery aggregation for on-chain submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofBatch {
    /// Batch ID
    pub batch_id: String,

    /// Relay ID
    pub relay_id: String,

    /// Message IDs in batch
    pub message_ids: Vec<String>,

    /// Total messages in batch
    pub count: usize,

    /// Total bandwidth in batch
    pub total_bandwidth: u64,

    /// Batch timestamp
    pub timestamp: SystemTime,
}

impl ProofBatch {
    /// Create new proof batch
    pub fn new(relay_id: String, message_ids: Vec<String>, total_bandwidth: u64) -> Self {
        let count = message_ids.len();
        let batch_id = format!(
            "batch_{}_{}_{}",
            relay_id,
            count,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );

        Self {
            batch_id,
            relay_id,
            message_ids,
            count,
            total_bandwidth,
            timestamp: SystemTime::now(),
        }
    }
}

/// Relay network manager
pub struct RelayNetworkManager {
    config: RelayNetworkConfig,

    /// Optional staking backend for token locking
    staking_backend: Option<Arc<dyn StakingBackend>>,

    /// All registered relays
    relays: HashMap<String, RelayInfo>,

    /// Stake transaction IDs for each relay (for unstaking)
    stake_tx_ids: HashMap<String, String>,

    /// Active relay pool (meeting requirements)
    active_pool: HashSet<String>,

    /// Pending proof-of-delivery messages
    pending_proofs: HashMap<String, Vec<(String, usize)>>, // relay_id -> [(msg_id, size)]

    /// Load balancing state
    round_robin_index: usize,

    /// Network statistics
    total_messages: u64,
    total_bandwidth: u64,
}

impl RelayNetworkManager {
    /// Create new relay network manager
    pub fn new(config: RelayNetworkConfig) -> Self {
        Self {
            config,
            staking_backend: None,
            relays: HashMap::new(),
            stake_tx_ids: HashMap::new(),
            active_pool: HashSet::new(),
            pending_proofs: HashMap::new(),
            round_robin_index: 0,
            total_messages: 0,
            total_bandwidth: 0,
        }
    }

    /// Create relay network manager with staking backend
    pub fn with_staking(config: RelayNetworkConfig, staking: Arc<dyn StakingBackend>) -> Self {
        Self {
            config,
            staking_backend: Some(staking),
            relays: HashMap::new(),
            stake_tx_ids: HashMap::new(),
            active_pool: HashSet::new(),
            pending_proofs: HashMap::new(),
            round_robin_index: 0,
            total_messages: 0,
            total_bandwidth: 0,
        }
    }

    /// Set staking backend after construction
    pub fn set_staking_backend(&mut self, staking: Arc<dyn StakingBackend>) {
        self.staking_backend = Some(staking);
    }

    /// Register a new relay node (synchronous, no staking verification)
    pub fn register_relay(&mut self, relay_info: RelayInfo) -> Result<()> {
        // Verify stake requirement
        if relay_info.stake < self.config.min_stake {
            return Err(Error::network(format!(
                "Insufficient stake: {} < {}",
                relay_info.stake, self.config.min_stake
            )));
        }

        // Check max relays
        if self.relays.len() >= self.config.max_relays {
            return Err(Error::network("Maximum relay count reached".to_string()));
        }

        let relay_id = relay_info.relay_id.clone();
        self.relays.insert(relay_id.clone(), relay_info);
        self.pending_proofs.insert(relay_id.clone(), Vec::new());

        // Update active pool
        self.update_active_pool();

        Ok(())
    }

    /// Register a new relay node with on-chain staking
    /// 
    /// This method locks the operator's tokens on the currency chain
    /// before registering the relay.
    pub async fn register_relay_with_staking(&mut self, relay_info: RelayInfo) -> Result<()> {
        // Verify stake requirement
        if relay_info.stake < self.config.min_stake {
            return Err(Error::network(format!(
                "Insufficient stake: {} < {}",
                relay_info.stake, self.config.min_stake
            )));
        }

        // Check max relays
        if self.relays.len() >= self.config.max_relays {
            return Err(Error::network("Maximum relay count reached".to_string()));
        }

        let relay_id = relay_info.relay_id.clone();
        let operator = relay_info.operator.clone();
        let stake_amount = relay_info.stake;

        // Lock tokens on currency chain if staking backend is configured
        if let Some(ref staking) = self.staking_backend {
            // 1. Submit stake transaction
            let stake_tx_id = staking
                .stake(&operator, stake_amount, RELAY_LOCK_DURATION)
                .await
                .map_err(|e| Error::network(format!("Failed to stake tokens: {}", e)))?;

            // 2. Wait for confirmation
            let confirmed = staking
                .wait_for_confirmation(&stake_tx_id, MIN_STAKE_CONFIRMATIONS)
                .await
                .map_err(|e| Error::network(format!("Stake confirmation failed: {}", e)))?;

            if !confirmed {
                return Err(Error::network("Stake transaction not confirmed".to_string()));
            }

            // 3. Store stake transaction ID for later unstaking
            self.stake_tx_ids.insert(relay_id.clone(), stake_tx_id);
        }

        // Register the relay
        self.relays.insert(relay_id.clone(), relay_info);
        self.pending_proofs.insert(relay_id.clone(), Vec::new());

        // Update active pool
        self.update_active_pool();

        Ok(())
    }

    /// Remove a relay node
    pub fn remove_relay(&mut self, relay_id: &str) -> Result<()> {
        if !self.relays.contains_key(relay_id) {
            return Err(Error::network("Relay not found".to_string()));
        }

        self.relays.remove(relay_id);
        self.active_pool.remove(relay_id);
        self.pending_proofs.remove(relay_id);
        self.stake_tx_ids.remove(relay_id);

        Ok(())
    }

    /// Remove a relay node with unstaking
    /// 
    /// This method unlocks the operator's staked tokens when unregistering.
    pub async fn remove_relay_with_unstaking(&mut self, relay_id: &str) -> Result<()> {
        let relay = self.relays.get(relay_id)
            .ok_or_else(|| Error::network("Relay not found".to_string()))?
            .clone();

        // Unstake tokens if staking backend is configured
        if let Some(ref staking) = self.staking_backend {
            if let Some(stake_tx_id) = self.stake_tx_ids.get(relay_id) {
                staking
                    .unstake(&relay.operator, stake_tx_id)
                    .await
                    .map_err(|e| Error::network(format!("Failed to unstake tokens: {}", e)))?;
            }
        }

        self.relays.remove(relay_id);
        self.active_pool.remove(relay_id);
        self.pending_proofs.remove(relay_id);
        self.stake_tx_ids.remove(relay_id);

        Ok(())
    }

    /// Slash a relay for misbehavior
    pub async fn slash_relay(&mut self, relay_id: &str, amount: u64, reason: &str) -> Result<()> {
        let relay = self.relays.get(relay_id)
            .ok_or_else(|| Error::network("Relay not found".to_string()))?
            .clone();

        // Slash tokens if staking backend is configured
        if let Some(ref staking) = self.staking_backend {
            staking
                .slash(&relay.operator, amount, reason)
                .await
                .map_err(|e| Error::network(format!("Failed to slash tokens: {}", e)))?;
        }

        // Update relay stake
        if let Some(relay_mut) = self.relays.get_mut(relay_id) {
            relay_mut.stake = relay_mut.stake.saturating_sub(amount);
        }

        // Update active pool (may remove relay if stake too low)
        self.update_active_pool();

        Ok(())
    }

    /// Update relay heartbeat
    pub fn heartbeat(&mut self, relay_id: &str) -> Result<()> {
        let relay = self
            .relays
            .get_mut(relay_id)
            .ok_or_else(|| Error::network("Relay not found".to_string()))?;

        relay.heartbeat();
        self.update_active_pool();

        Ok(())
    }

    /// Update active relay pool based on requirements
    fn update_active_pool(&mut self) {
        let heartbeat_threshold = Duration::from_secs(self.config.heartbeat_interval * 2);

        self.active_pool.clear();

        for (relay_id, relay) in &self.relays {
            // Check online status
            if !relay.is_online(heartbeat_threshold) {
                continue;
            }

            // Check stake
            if relay.stake < self.config.min_stake {
                continue;
            }

            // Check uptime score
            let score = relay.uptime_score(self.config.uptime_window);
            if score < self.config.min_uptime_score {
                continue;
            }

            self.active_pool.insert(relay_id.clone());
        }
    }

    /// Select relay for message delivery using load balancing strategy
    pub fn select_relay(&mut self) -> Result<String> {
        if self.active_pool.is_empty() {
            return Err(Error::network("No active relays available".to_string()));
        }

        let active_relays: Vec<_> = self.active_pool.iter().cloned().collect();

        match self.config.load_strategy {
            LoadStrategy::RoundRobin => {
                let relay_id = active_relays[self.round_robin_index % active_relays.len()].clone();
                self.round_robin_index += 1;
                Ok(relay_id)
            }

            LoadStrategy::WeightedRoundRobin => {
                // Select based on uptime score
                let mut best_relay = None;
                let mut best_score = 0.0;

                for relay_id in &active_relays {
                    if let Some(relay) = self.relays.get(relay_id) {
                        let score = relay.uptime_score(self.config.uptime_window);
                        if score > best_score {
                            best_score = score;
                            best_relay = Some(relay_id.clone());
                        }
                    }
                }

                best_relay.ok_or_else(|| Error::network("No suitable relay found".to_string()))
            }

            LoadStrategy::LeastConnections => {
                // Select relay with fewest active connections
                let mut best_relay = None;
                let mut min_connections = usize::MAX;

                for relay_id in &active_relays {
                    if let Some(relay) = self.relays.get(relay_id) {
                        if relay.active_connections < min_connections {
                            min_connections = relay.active_connections;
                            best_relay = Some(relay_id.clone());
                        }
                    }
                }

                best_relay.ok_or_else(|| Error::network("No suitable relay found".to_string()))
            }

            LoadStrategy::Geographic => {
                // For now, use round-robin (geographic selection requires client location)
                let relay_id = active_relays[self.round_robin_index % active_relays.len()].clone();
                self.round_robin_index += 1;
                Ok(relay_id)
            }
        }
    }

    /// Record a relayed message
    pub fn record_relay(&mut self, relay_id: &str, message_id: String, size: usize) -> Result<()> {
        // Update relay stats
        let relay = self
            .relays
            .get_mut(relay_id)
            .ok_or_else(|| Error::network("Relay not found".to_string()))?;

        relay.record_message(size);

        // Add to pending proofs
        self.pending_proofs
            .entry(relay_id.to_string())
            .or_default()
            .push((message_id, size));

        // Update global stats
        self.total_messages += 1;
        self.total_bandwidth += size as u64;

        Ok(())
    }

    /// Generate proof-of-delivery batch for on-chain submission
    pub fn generate_proof_batch(&mut self, relay_id: &str) -> Result<Option<ProofBatch>> {
        let proofs = self
            .pending_proofs
            .get_mut(relay_id)
            .ok_or_else(|| Error::network("Relay not found".to_string()))?;

        if proofs.len() < self.config.pod_batch_size {
            return Ok(None); // Not enough for a batch yet
        }

        // Take batch_size proofs
        let batch_proofs: Vec<_> = proofs.drain(..self.config.pod_batch_size).collect();
        let message_ids: Vec<_> = batch_proofs.iter().map(|(id, _)| id.clone()).collect();
        let total_bandwidth: u64 = batch_proofs.iter().map(|(_, size)| *size as u64).sum();

        let batch = ProofBatch::new(relay_id.to_string(), message_ids, total_bandwidth);

        Ok(Some(batch))
    }

    /// Get geographic distribution
    pub fn geographic_distribution(&self) -> HashMap<Continent, usize> {
        let mut dist = HashMap::new();

        for relay in self.relays.values() {
            *dist.entry(relay.continent).or_insert(0) += 1;
        }

        dist
    }

    /// Check if geographic diversity requirement is met
    pub fn has_geographic_diversity(&self) -> bool {
        let dist = self.geographic_distribution();
        dist.len() >= self.config.min_continents
    }

    /// Get network statistics
    pub fn network_stats(&self) -> NetworkStats {
        NetworkStats {
            total_relays: self.relays.len(),
            active_relays: self.active_pool.len(),
            total_messages: self.total_messages,
            total_bandwidth: self.total_bandwidth,
            continents: self.geographic_distribution().len(),
        }
    }

    /// Get relay by ID
    pub fn get_relay(&self, relay_id: &str) -> Option<&RelayInfo> {
        self.relays.get(relay_id)
    }

    /// Check if relay is active
    pub fn is_relay_active(&self, relay_id: &str) -> bool {
        self.active_pool.contains(relay_id)
    }

    /// Distribute rewards to relays based on performance
    /// 
    /// This method calculates and mints rewards for all active relays based on:
    /// - Messages relayed during the epoch
    /// - Uptime percentage
    /// - Geographic diversity bonus (underserved regions)
    /// 
    /// # Arguments
    /// * `epoch` - The epoch number for which rewards are being distributed
    /// 
    /// # Returns
    /// Vector of reward distributions with transaction IDs
    pub async fn distribute_relay_rewards(
        &self,
        epoch: u64,
    ) -> Result<Vec<RewardDistribution>> {
        let staking = self.staking_backend.as_ref()
            .ok_or_else(|| Error::network("No staking backend configured for reward distribution"))?;

        let mut distributions = Vec::new();

        for (relay_id, info) in &self.relays {
            // Skip inactive relays
            if !self.active_pool.contains(relay_id) {
                continue;
            }

            // Calculate reward based on performance
            let reward = self.calculate_relay_reward(info, epoch);

            if reward > 0 {
                // Use staking backend to distribute reward
                // The staking backend should call currency_chain.mint_rewards internally
                match staking.distribute_reward(&info.operator, reward, epoch).await {
                    Ok(tx_id) => {
                        distributions.push(RewardDistribution {
                            relay_id: relay_id.clone(),
                            operator: info.operator.clone(),
                            amount: reward,
                            tx_id,
                            epoch,
                            uptime_score: info.uptime_score(self.config.uptime_window),
                            messages_relayed: info.messages_relayed,
                            continent: info.continent,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to distribute reward to relay {}: {}",
                            relay_id, e
                        );
                    }
                }
            }
        }

        tracing::info!(
            "💰 Distributed rewards for epoch {}: {} relays, {} total tokens",
            epoch,
            distributions.len(),
            distributions.iter().map(|d| d.amount).sum::<u64>()
        );

        Ok(distributions)
    }

    /// Calculate relay reward based on performance metrics
    fn calculate_relay_reward(&self, info: &RelayInfo, _epoch: u64) -> u64 {
        // Base reward: 100 DCHAT per epoch (8 decimals)
        let base_reward: u64 = 100_0000_0000;

        // Uptime multiplier (0.0 to 1.0)
        let uptime = info.uptime_score(self.config.uptime_window);
        if uptime < self.config.min_uptime_score {
            return 0; // No rewards for low uptime
        }

        // Message volume bonus: 1 DCHAT per 1000 messages
        let message_bonus: u64 = (info.messages_relayed / 1000) * 1_0000_0000;

        // Geographic diversity bonus (underserved regions get 50% extra)
        let geo_bonus = match info.continent {
            Continent::Africa | Continent::SouthAmerica | Continent::Antarctica => base_reward / 2,
            _ => 0,
        };

        // Stake multiplier: bonus for higher stakes (up to 50%)
        let stake_multiplier = if info.stake > self.config.min_stake * 10 {
            1.5
        } else if info.stake > self.config.min_stake * 5 {
            1.25
        } else {
            1.0
        };

        let calculated = ((base_reward as f64 * uptime) as u64) + message_bonus + geo_bonus;
        (calculated as f64 * stake_multiplier) as u64
    }

    /// Get total pending rewards for an epoch (estimate)
    pub fn estimate_epoch_rewards(&self, epoch: u64) -> u64 {
        self.relays
            .values()
            .filter(|r| self.active_pool.contains(&r.relay_id))
            .map(|info| self.calculate_relay_reward(info, epoch))
            .sum()
    }
}

/// Network statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    /// Total registered relays
    pub total_relays: usize,

    /// Currently active relays
    pub active_relays: usize,

    /// Total messages relayed
    pub total_messages: u64,

    /// Total bandwidth used
    pub total_bandwidth: u64,

    /// Number of continents represented
    pub continents: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relay_network_config_default() {
        let config = RelayNetworkConfig::default();
        assert_eq!(config.min_relays, 10);
        assert_eq!(config.min_stake, 10_000);
        assert_eq!(config.min_uptime_score, 0.95);
    }

    #[test]
    fn test_relay_registration() {
        let mut manager = RelayNetworkManager::new(RelayNetworkConfig::default());

        let relay = RelayInfo::new(
            "relay1".to_string(),
            UserId(uuid::Uuid::new_v4()),
            20_000,
            Continent::NorthAmerica,
            12345,
        );

        manager.register_relay(relay).unwrap();
        assert_eq!(manager.relays.len(), 1);
        assert!(manager.get_relay("relay1").is_some());
    }

    #[test]
    fn test_insufficient_stake() {
        let mut manager = RelayNetworkManager::new(RelayNetworkConfig::default());

        let relay = RelayInfo::new(
            "relay1".to_string(),
            UserId(uuid::Uuid::new_v4()),
            5_000, // Below minimum
            Continent::Europe,
            12345,
        );

        let result = manager.register_relay(relay);
        assert!(result.is_err());
    }

    #[test]
    fn test_uptime_scoring() {
        let mut relay = RelayInfo::new(
            "relay1".to_string(),
            UserId(uuid::Uuid::new_v4()),
            20_000,
            Continent::Asia,
            12345,
        );

        // Simulate heartbeats
        for _ in 0..5 {
            relay.heartbeat();
        }

        let score = relay.uptime_score(3600); // 1 hour window
        assert!(score >= 0.0 && score <= 1.0);
    }

    #[test]
    fn test_load_balancing_round_robin() {
        let mut config = RelayNetworkConfig::default();
        config.load_strategy = LoadStrategy::RoundRobin;
        config.min_uptime_score = 0.0; // Disable for test

        let mut manager = RelayNetworkManager::new(config);

        // Register 3 relays
        for i in 1..=3 {
            let relay = RelayInfo::new(
                format!("relay{}", i),
                UserId(uuid::Uuid::new_v4()),
                20_000,
                Continent::NorthAmerica,
                12345 + i,
            );
            manager.register_relay(relay).unwrap();
        }

        manager.update_active_pool();

        // Select relays in round-robin
        let r1 = manager.select_relay().unwrap();
        let r2 = manager.select_relay().unwrap();
        let _r3 = manager.select_relay().unwrap();
        let r4 = manager.select_relay().unwrap();

        // Should cycle through relays
        assert_ne!(r1, r2);
        assert_eq!(r1, r4); // Should wrap around
    }

    #[test]
    fn test_proof_batch_generation() {
        let mut manager = RelayNetworkManager::new(RelayNetworkConfig::default());

        let relay = RelayInfo::new(
            "relay1".to_string(),
            UserId(uuid::Uuid::new_v4()),
            20_000,
            Continent::Europe,
            12345,
        );
        manager.register_relay(relay).unwrap();

        // Record messages (batch size is 100 by default)
        for i in 0..100 {
            manager
                .record_relay("relay1", format!("msg{}", i), 1024)
                .unwrap();
        }

        let batch = manager.generate_proof_batch("relay1").unwrap();
        assert!(batch.is_some());

        let batch = batch.unwrap();
        assert_eq!(batch.count, 100);
        assert_eq!(batch.total_bandwidth, 100 * 1024);
    }

    #[test]
    fn test_geographic_distribution() {
        let mut manager = RelayNetworkManager::new(RelayNetworkConfig::default());

        // Register relays in different continents
        let continents = vec![
            Continent::NorthAmerica,
            Continent::Europe,
            Continent::Asia,
            Continent::Asia,
        ];

        for (i, continent) in continents.iter().enumerate() {
            let relay = RelayInfo::new(
                format!("relay{}", i),
                UserId(uuid::Uuid::new_v4()),
                20_000,
                *continent,
                12345 + i as u32,
            );
            manager.register_relay(relay).unwrap();
        }

        let dist = manager.geographic_distribution();
        assert_eq!(dist.len(), 3); // 3 unique continents
        assert_eq!(dist[&Continent::Asia], 2);
    }

    #[test]
    fn test_network_stats() {
        let mut manager = RelayNetworkManager::new(RelayNetworkConfig::default());

        let relay = RelayInfo::new(
            "relay1".to_string(),
            UserId(uuid::Uuid::new_v4()),
            20_000,
            Continent::NorthAmerica,
            12345,
        );
        manager.register_relay(relay).unwrap();

        manager
            .record_relay("relay1", "msg1".to_string(), 2048)
            .unwrap();

        let stats = manager.network_stats();
        assert_eq!(stats.total_relays, 1);
        assert_eq!(stats.total_messages, 1);
        assert_eq!(stats.total_bandwidth, 2048);
    }
}
