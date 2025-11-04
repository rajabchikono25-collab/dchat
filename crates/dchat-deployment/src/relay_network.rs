// Distributed Relay Network Configuration and Management
//
// This module implements a geographically distributed relay network with:
// - 20-50 relay nodes across multiple regions
// - Incentive system (uptime rewards, message fees, geographic bonuses)
// - Discovery and load balancing based on latency, health, and stake
// - Reputation tracking and slashing for misbehavior
// - Automatic failover and health monitoring

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};
use thiserror::Error;

use crate::multi_region_config::GeographicRegion;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("Invalid relay configuration: {0}")]
    InvalidConfig(String),
    #[error("Insufficient relay diversity: {0}")]
    InsufficientDiversity(String),
    #[error("Relay health check failed: {0}")]
    HealthCheckFailed(String),
}

/// Relay tier based on performance and stake
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayTier {
    /// Premium tier: High stake (10k+ DCHAT), 99.9% uptime, <50ms latency
    Premium,
    /// Standard tier: Medium stake (5k-10k DCHAT), 99% uptime, <100ms latency
    Standard,
    /// Basic tier: Low stake (1k-5k DCHAT), 95% uptime, <200ms latency
    Basic,
    /// Trial tier: Minimal stake (<1k DCHAT), for new relays
    Trial,
}

impl RelayTier {
    /// Get stake multiplier for rewards (premium relays earn more)
    pub fn stake_multiplier(&self) -> f64 {
        match self {
            RelayTier::Premium => 2.0,
            RelayTier::Standard => 1.5,
            RelayTier::Basic => 1.0,
            RelayTier::Trial => 0.5,
        }
    }

    /// Get minimum stake required for this tier (in DCHAT tokens)
    pub fn min_stake(&self) -> u64 {
        match self {
            RelayTier::Premium => 10_000,
            RelayTier::Standard => 5_000,
            RelayTier::Basic => 1_000,
            RelayTier::Trial => 100,
        }
    }

    /// Get required uptime percentage for this tier
    pub fn required_uptime(&self) -> f64 {
        match self {
            RelayTier::Premium => 0.999,
            RelayTier::Standard => 0.99,
            RelayTier::Basic => 0.95,
            RelayTier::Trial => 0.90,
        }
    }
}

/// Relay node configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    /// Unique relay identifier
    pub relay_id: String,

    /// Geographic region
    pub region: GeographicRegion,

    /// Public address (DNS or IP)
    pub public_address: String,

    /// Listen addresses for incoming connections
    pub listen_addresses: Vec<String>,

    /// WebSocket endpoint for clients
    pub websocket_address: SocketAddr,

    /// RPC endpoint for health checks
    pub rpc_address: SocketAddr,

    /// Relay tier (determines rewards)
    pub tier: RelayTier,

    /// Staked amount (in DCHAT tokens)
    pub stake_amount: u64,

    /// Operator's wallet address for reward payouts
    pub operator_address: String,

    /// Bootstrap validators to connect to
    pub validator_addresses: Vec<String>,

    /// Maximum concurrent connections
    pub max_connections: usize,

    /// Message rate limit (messages/second)
    pub rate_limit: u32,

    /// Geographic bonus multiplier (1.0-2.0)
    pub geographic_bonus: f64,
}

impl RelayConfig {
    /// Calculate expected reward per day (in DCHAT tokens)
    pub fn calculate_daily_reward(
        &self,
        base_reward: u64,
        messages_relayed: u64,
        fee_per_message: f64,
        uptime: f64,
    ) -> f64 {
        // Base reward (for being online)
        let base = base_reward as f64 * self.tier.stake_multiplier();

        // Message fees (per-message earnings)
        let message_fees = messages_relayed as f64 * fee_per_message;

        // Geographic bonus (incentivize underserved regions)
        let geo_bonus = base * (self.geographic_bonus - 1.0);

        // Uptime penalty/bonus
        let uptime_multiplier = if uptime >= self.tier.required_uptime() {
            1.0 + (uptime - self.tier.required_uptime()) * 2.0 // Bonus for exceeding
        } else {
            uptime / self.tier.required_uptime() // Penalty for falling short
        };

        (base + message_fees + geo_bonus) * uptime_multiplier
    }

    /// Calculate relay score for load balancing (0.0-100.0)
    pub fn calculate_score(
        &self,
        latency_ms: u32,
        current_connections: usize,
        uptime: f64,
        reputation: f64,
    ) -> f64 {
        // Latency score (lower is better, 0ms=50 points, 200ms=0 points)
        let latency_score = ((200 - latency_ms.min(200)) as f64 / 4.0).max(0.0);

        // Load score (fewer connections = higher score)
        let load_factor = 1.0 - (current_connections as f64 / self.max_connections as f64);
        let load_score = load_factor * 20.0;

        // Uptime score (99.9% uptime = 15 points)
        let uptime_score = uptime * 15.0;

        // Reputation score (1.0 reputation = 10 points)
        let reputation_score = reputation * 10.0;

        // Tier bonus (premium relays get priority)
        let tier_bonus = match self.tier {
            RelayTier::Premium => 5.0,
            RelayTier::Standard => 3.0,
            RelayTier::Basic => 1.0,
            RelayTier::Trial => 0.0,
        };

        latency_score + load_score + uptime_score + reputation_score + tier_bonus
    }
}

/// Relay reputation and performance tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayReputation {
    /// Relay identifier
    pub relay_id: String,

    /// Total messages relayed successfully
    pub messages_relayed: u64,

    /// Total messages dropped/failed
    pub messages_failed: u64,

    /// Total uptime duration
    pub total_uptime: Duration,

    /// Total registered duration
    pub total_duration: Duration,

    /// Number of slashing events
    pub slashing_events: u32,

    /// Current reputation score (0.0-1.0)
    pub reputation_score: f64,

    /// Last health check timestamp
    pub last_seen: SystemTime,

    /// Average latency (milliseconds)
    pub avg_latency_ms: u32,

    /// Current connections
    pub current_connections: usize,
}

impl RelayReputation {
    /// Create new reputation tracker
    pub fn new(relay_id: String) -> Self {
        Self {
            relay_id,
            messages_relayed: 0,
            messages_failed: 0,
            total_uptime: Duration::from_secs(0),
            total_duration: Duration::from_secs(0),
            slashing_events: 0,
            reputation_score: 1.0, // Start with perfect reputation
            last_seen: SystemTime::now(),
            avg_latency_ms: 0,
            current_connections: 0,
        }
    }

    /// Calculate uptime percentage
    pub fn uptime_percentage(&self) -> f64 {
        if self.total_duration.as_secs() == 0 {
            return 0.0;
        }
        self.total_uptime.as_secs_f64() / self.total_duration.as_secs_f64()
    }

    /// Calculate success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.messages_relayed + self.messages_failed;
        if total == 0 {
            return 1.0;
        }
        self.messages_relayed as f64 / total as f64
    }

    /// Update reputation based on recent performance
    pub fn update_reputation(&mut self) {
        let uptime = self.uptime_percentage();
        let success_rate = self.success_rate();

        // Reputation = 40% uptime + 40% success rate + 20% anti-slashing
        let uptime_component = uptime * 0.4;
        let success_component = success_rate * 0.4;
        let slashing_penalty = (1.0 - (self.slashing_events as f64 * 0.1).min(1.0)) * 0.2;

        self.reputation_score = (uptime_component + success_component + slashing_penalty)
            .max(0.0)
            .min(1.0);
    }

    /// Apply slashing penalty for misbehavior
    pub fn apply_slashing(&mut self, severity: f64) {
        self.slashing_events += 1;
        self.reputation_score = (self.reputation_score - severity).max(0.0);
    }

    /// Check if relay is healthy
    pub fn is_healthy(&self, max_stale_duration: Duration) -> bool {
        let age = SystemTime::now()
            .duration_since(self.last_seen)
            .unwrap_or(Duration::from_secs(u64::MAX));

        age < max_stale_duration && self.reputation_score > 0.5
    }
}

/// Incentive system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncentiveConfig {
    /// Base reward per day for running a relay (DCHAT tokens)
    pub base_reward_per_day: u64,

    /// Fee per message relayed (DCHAT tokens)
    pub fee_per_message: f64,

    /// Geographic bonus multipliers by region
    pub geographic_bonuses: HashMap<GeographicRegion, f64>,

    /// Minimum uptime to receive rewards
    pub min_uptime_for_rewards: f64,

    /// Slashing penalty for downtime (% of stake)
    pub downtime_slashing_rate: f64,

    /// Slashing penalty for message drops (% of stake)
    pub drop_slashing_rate: f64,

    /// Maximum connections bonus multiplier
    pub max_connections_bonus: f64,
}

impl Default for IncentiveConfig {
    fn default() -> Self {
        let mut geographic_bonuses = HashMap::new();
        geographic_bonuses.insert(GeographicRegion::SouthAmerica, 2.0); // High bonus
        geographic_bonuses.insert(GeographicRegion::AsiaPacificSE, 1.8);
        geographic_bonuses.insert(GeographicRegion::AsiaPacificNE, 1.6);
        geographic_bonuses.insert(GeographicRegion::EUCentral, 1.4);
        geographic_bonuses.insert(GeographicRegion::EUWest, 1.2);
        geographic_bonuses.insert(GeographicRegion::USWest, 1.1);
        geographic_bonuses.insert(GeographicRegion::USEast, 1.0); // Standard

        Self {
            base_reward_per_day: 100, // 100 DCHAT/day base
            fee_per_message: 0.001,   // 0.001 DCHAT/message
            geographic_bonuses,
            min_uptime_for_rewards: 0.95, // 95% minimum
            downtime_slashing_rate: 0.01, // 1% per day of downtime
            drop_slashing_rate: 0.001,    // 0.1% per dropped message
            max_connections_bonus: 1.5,   // 50% bonus for high traffic
        }
    }
}

/// Distributed relay network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayNetworkConfig {
    /// Network name
    pub network_name: String,

    /// All relay configurations
    pub relays: Vec<RelayConfig>,

    /// Incentive system configuration
    pub incentives: IncentiveConfig,

    /// Minimum number of relays per region
    pub min_relays_per_region: usize,

    /// Target total relay count
    pub target_relay_count: usize,

    /// Health check interval (seconds)
    pub health_check_interval: u64,

    /// Maximum relay staleness (seconds)
    pub max_relay_staleness: u64,
}

impl RelayNetworkConfig {
    /// Create recommended relay network (20-50 relays across regions)
    pub fn new_recommended(network_name: String, relay_count: usize) -> Result<Self, RelayError> {
        if relay_count < 20 || relay_count > 50 {
            return Err(RelayError::InvalidConfig(
                "Relay count must be between 20 and 50".to_string(),
            ));
        }

        let mut relays = Vec::new();
        let regions = vec![
            (GeographicRegion::USEast, 0.25),        // 25% in US East
            (GeographicRegion::USWest, 0.20),        // 20% in US West
            (GeographicRegion::EUWest, 0.20),        // 20% in EU West
            (GeographicRegion::EUCentral, 0.10),     // 10% in EU Central
            (GeographicRegion::AsiaPacificSE, 0.10), // 10% in Asia SE
            (GeographicRegion::AsiaPacificNE, 0.10), // 10% in Asia NE
            (GeographicRegion::SouthAmerica, 0.05),  // 5% in South America
        ];

        let mut relay_id_counter = 0;
        let mut allocated = 0;

        for (idx, (region, percentage)) in regions.iter().enumerate() {
            // For the last region, allocate remaining relays to avoid rounding errors
            let region_count = if idx == regions.len() - 1 {
                relay_count - allocated
            } else {
                ((relay_count as f64 * percentage).round() as usize).min(relay_count - allocated)
            };

            allocated += region_count;

            let geographic_bonus = IncentiveConfig::default()
                .geographic_bonuses
                .get(region)
                .copied()
                .unwrap_or(1.0);

            for i in 0..region_count {
                let relay_id = format!("relay-{}-{}", region.dns_suffix(), i + 1);
                let tier = match relay_id_counter % 4 {
                    0 => RelayTier::Premium,
                    1 => RelayTier::Standard,
                    2 => RelayTier::Basic,
                    _ => RelayTier::Trial,
                };

                let ws_port = 8080 + relay_id_counter;
                let rpc_port = 9090 + relay_id_counter;

                relays.push(RelayConfig {
                    relay_id: relay_id.clone(),
                    region: *region,
                    public_address: format!("{}.dchat.network", relay_id),
                    listen_addresses: vec![
                        format!("/ip4/0.0.0.0/tcp/{}", ws_port),
                        format!("/ip4/0.0.0.0/udp/{}/quic-v1", ws_port),
                    ],
                    websocket_address: format!("0.0.0.0:{}", ws_port).parse().unwrap(),
                    rpc_address: format!("0.0.0.0:{}", rpc_port).parse().unwrap(),
                    tier,
                    stake_amount: tier.min_stake(),
                    operator_address: format!("dchat_{}_operator", relay_id),
                    validator_addresses: vec![
                        "validator-us-east-1.dchat.network:9545".to_string(),
                        "validator-eu-west-1.dchat.network:9547".to_string(),
                        "validator-ap-southeast-1.dchat.network:9549".to_string(),
                    ],
                    max_connections: match tier {
                        RelayTier::Premium => 10000,
                        RelayTier::Standard => 5000,
                        RelayTier::Basic => 2000,
                        RelayTier::Trial => 500,
                    },
                    rate_limit: match tier {
                        RelayTier::Premium => 1000,
                        RelayTier::Standard => 500,
                        RelayTier::Basic => 200,
                        RelayTier::Trial => 50,
                    },
                    geographic_bonus,
                });

                relay_id_counter += 1;
            }
        }

        Ok(Self {
            network_name,
            relays,
            incentives: IncentiveConfig::default(),
            min_relays_per_region: 2,
            target_relay_count: relay_count,
            health_check_interval: 30,
            max_relay_staleness: 90,
        })
    }

    /// Verify network has sufficient geographic diversity
    pub fn verify_diversity(&self) -> Result<(), RelayError> {
        let mut region_counts: HashMap<GeographicRegion, usize> = HashMap::new();

        for relay in &self.relays {
            *region_counts.entry(relay.region).or_insert(0) += 1;
        }

        // Check minimum relays per region
        for (region, count) in &region_counts {
            if *count < self.min_relays_per_region {
                return Err(RelayError::InsufficientDiversity(format!(
                    "Region {:?} has only {} relays (minimum {})",
                    region, count, self.min_relays_per_region
                )));
            }
        }

        // Check we have at least 4 regions
        if region_counts.len() < 4 {
            return Err(RelayError::InsufficientDiversity(format!(
                "Only {} regions covered (minimum 4)",
                region_counts.len()
            )));
        }

        // Check no region has >50% of relays
        let total_relays = self.relays.len();
        for (region, count) in &region_counts {
            let percentage = *count as f64 / total_relays as f64;
            if percentage > 0.5 {
                return Err(RelayError::InsufficientDiversity(format!(
                    "Region {:?} has {:.1}% of relays (maximum 50%)",
                    region,
                    percentage * 100.0
                )));
            }
        }

        Ok(())
    }

    /// Select best relays for a client based on latency and load
    pub fn select_relays(
        &self,
        client_region: GeographicRegion,
        count: usize,
        reputations: &HashMap<String, RelayReputation>,
    ) -> Vec<String> {
        let mut scored_relays: Vec<(String, f64)> = self
            .relays
            .iter()
            .filter_map(|relay| {
                let reputation = reputations.get(&relay.relay_id)?;
                if !reputation.is_healthy(Duration::from_secs(self.max_relay_staleness)) {
                    return None;
                }

                // Calculate latency to client
                let latency_ms = relay.region.latency_to(&client_region);

                // Calculate relay score
                let score = relay.calculate_score(
                    latency_ms as u32,
                    reputation.current_connections,
                    reputation.uptime_percentage(),
                    reputation.reputation_score,
                );

                Some((relay.relay_id.clone(), score))
            })
            .collect();

        // Sort by score (descending)
        scored_relays.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Return top N relay IDs
        scored_relays
            .into_iter()
            .take(count)
            .map(|(id, _)| id)
            .collect()
    }

    /// Generate TOML configuration for a specific relay
    pub fn generate_relay_toml(&self, relay_id: &str) -> Result<String, RelayError> {
        let relay = self
            .relays
            .iter()
            .find(|r| r.relay_id == relay_id)
            .ok_or_else(|| RelayError::InvalidConfig(format!("Relay {} not found", relay_id)))?;

        let toml = format!(
            r#"# dchat Relay Configuration
# Region: {:?}
# Relay ID: {}
# Tier: {:?}

[network]
relay_id = "{}"
network_name = "{}"
region = "{:?}"

# Listen addresses
listen_addresses = [
{}
]

# Client WebSocket endpoint
websocket_address = "{}"

# RPC endpoint for health checks
rpc_address = "{}"
public_address = "{}"

# Validator connections
validator_addresses = [
{}
]

[relay]
tier = "{:?}"
max_connections = {}
rate_limit = {}
stake_amount = {}
operator_address = "{}"

[incentives]
base_reward_per_day = {}
fee_per_message = {}
geographic_bonus = {}
min_uptime_for_rewards = {}

[health]
health_check_interval = {}
max_relay_staleness = {}
"#,
            relay.region,
            relay.relay_id,
            relay.tier,
            relay.relay_id,
            self.network_name,
            relay.region,
            relay
                .listen_addresses
                .iter()
                .map(|addr| format!("    \"{}\"", addr))
                .collect::<Vec<_>>()
                .join(",\n"),
            relay.websocket_address,
            relay.rpc_address,
            relay.public_address,
            relay
                .validator_addresses
                .iter()
                .map(|addr| format!("    \"{}\"", addr))
                .collect::<Vec<_>>()
                .join(",\n"),
            relay.tier,
            relay.max_connections,
            relay.rate_limit,
            relay.stake_amount,
            relay.operator_address,
            self.incentives.base_reward_per_day,
            self.incentives.fee_per_message,
            relay.geographic_bonus,
            self.incentives.min_uptime_for_rewards,
            self.health_check_interval,
            self.max_relay_staleness,
        );

        Ok(toml)
    }

    /// Get relays by region
    pub fn get_relays_in_region(&self, region: GeographicRegion) -> Vec<&RelayConfig> {
        self.relays.iter().filter(|r| r.region == region).collect()
    }

    /// Get relays by tier
    pub fn get_relays_by_tier(&self, tier: RelayTier) -> Vec<&RelayConfig> {
        self.relays.iter().filter(|r| r.tier == tier).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relay_network_creation() {
        let network = RelayNetworkConfig::new_recommended("dchat-mainnet".to_string(), 30)
            .expect("Failed to create relay network");

        assert_eq!(network.relays.len(), 30);
        assert_eq!(network.network_name, "dchat-mainnet");
    }

    #[test]
    fn test_relay_network_diversity() {
        let network = RelayNetworkConfig::new_recommended("dchat-mainnet".to_string(), 40)
            .expect("Failed to create relay network");

        network.verify_diversity().expect("Diversity check failed");
    }

    #[test]
    fn test_relay_tier_rewards() {
        let premium = RelayTier::Premium;
        let standard = RelayTier::Standard;

        assert!(premium.stake_multiplier() > standard.stake_multiplier());
        assert!(premium.min_stake() > standard.min_stake());
    }

    #[test]
    fn test_relay_score_calculation() {
        let relay = RelayConfig {
            relay_id: "test-relay".to_string(),
            region: GeographicRegion::USEast,
            public_address: "test.dchat.network".to_string(),
            listen_addresses: vec![],
            websocket_address: "0.0.0.0:8080".parse().unwrap(),
            rpc_address: "0.0.0.0:9090".parse().unwrap(),
            tier: RelayTier::Premium,
            stake_amount: 10000,
            operator_address: "test_operator".to_string(),
            validator_addresses: vec![],
            max_connections: 5000,
            rate_limit: 1000,
            geographic_bonus: 1.5,
        };

        let score = relay.calculate_score(50, 1000, 0.999, 0.95);
        assert!(score > 50.0); // Should have high score
    }

    #[test]
    fn test_relay_reputation_tracking() {
        let mut reputation = RelayReputation::new("test-relay".to_string());

        reputation.messages_relayed = 1000;
        reputation.messages_failed = 10;
        reputation.total_uptime = Duration::from_secs(86400); // 1 day
        reputation.total_duration = Duration::from_secs(86400);

        assert_eq!(reputation.uptime_percentage(), 1.0);
        assert_eq!(reputation.success_rate(), 1000.0 / 1010.0);

        reputation.update_reputation();
        assert!(reputation.reputation_score > 0.95);
    }

    #[test]
    fn test_relay_selection() {
        let network = RelayNetworkConfig::new_recommended("dchat-mainnet".to_string(), 30)
            .expect("Failed to create relay network");

        let mut reputations = HashMap::new();
        for relay in &network.relays {
            let mut rep = RelayReputation::new(relay.relay_id.clone());
            rep.messages_relayed = 1000;
            rep.total_uptime = Duration::from_secs(86400);
            rep.total_duration = Duration::from_secs(86400);
            rep.reputation_score = 0.99;
            reputations.insert(relay.relay_id.clone(), rep);
        }

        let selected = network.select_relays(GeographicRegion::USEast, 5, &reputations);
        assert_eq!(selected.len(), 5);
    }

    #[test]
    fn test_relay_toml_generation() {
        let network = RelayNetworkConfig::new_recommended("dchat-mainnet".to_string(), 20)
            .expect("Failed to create relay network");

        let relay_id = &network.relays[0].relay_id;
        let toml = network
            .generate_relay_toml(relay_id)
            .expect("Failed to generate TOML");

        assert!(toml.contains("relay_id"));
        assert!(toml.contains("network_name"));
        assert!(toml.contains("dchat-mainnet"));
    }
}
