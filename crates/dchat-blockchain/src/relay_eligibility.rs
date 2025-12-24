//! Relay Epoch Eligibility for Fee Distribution
//!
//! This module computes relay eligibility for epoch-based reward distribution.
//! Eligibility requires passing two gates:
//! 1. **Uptime Gate**: Relay must have sufficient verified work activity slots
//! 2. **Work Events Gate**: Relay must have minimum verifiable work events
//!
//! # Architecture
//!
//! - Epochs are block-based: epoch `e` spans blocks `[e*EPOCH_LENGTH, (e+1)*EPOCH_LENGTH - 1]`
//! - Each epoch is divided into slots (default: 60 slots of 30 blocks each)
//! - A slot is "up" if the relay produced at least one verifiable work event in that slot
//! - Uptime ratio = up_slots / total_slots (must be >= threshold, default 0.80)
//!
//! # Security Properties
//!
//! - Work events are verifiable (cryptographic proofs, not heartbeats)
//! - Deduplication prevents double-counting of work events
//! - Registration before epoch start prevents gaming
//! - Deterministic computation for consensus agreement

use crate::hardened_consensus::threshold_normalization::EPOCH_LENGTH_BLOCKS;
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Default minimum uptime ratio (80%)
pub const DEFAULT_MIN_UPTIME_RATIO: f64 = 0.80;

/// Default minimum work events per epoch
pub const DEFAULT_MIN_WORK_EVENTS: u64 = 1;

/// Number of slots per epoch for uptime calculation
pub const SLOTS_PER_EPOCH: u64 = 60;

/// Blocks per slot (1800 / 60 = 30 blocks per slot)
pub const BLOCKS_PER_SLOT: u64 = EPOCH_LENGTH_BLOCKS / SLOTS_PER_EPOCH;

/// Configuration for relay eligibility computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EligibilityConfig {
    /// Minimum uptime ratio (0.0 to 1.0)
    pub min_uptime_ratio: f64,
    /// Minimum work events required per epoch
    pub min_work_events: u64,
    /// Number of slots per epoch
    pub slots_per_epoch: u64,
}

impl Default for EligibilityConfig {
    fn default() -> Self {
        Self {
            min_uptime_ratio: DEFAULT_MIN_UPTIME_RATIO,
            min_work_events: DEFAULT_MIN_WORK_EVENTS,
            slots_per_epoch: SLOTS_PER_EPOCH,
        }
    }
}

/// Relay registration info from the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredRelay {
    /// Unique relay identifier (e.g., public key hex or relay_id string)
    pub relay_id: String,
    /// Operator's UserId who receives rewards
    pub operator: UserId,
    /// Staked amount (determines reward weight)
    pub stake: u64,
    /// Block height when relay was registered
    pub registered_at_block: u64,
    /// Whether relay is currently suspended/jailed/slashed
    pub is_suspended: bool,
}

/// A verifiable work event from a relay
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RelayWorkEvent {
    /// Relay that performed the work
    pub relay_id: String,
    /// Block height when this work was recorded/finalized
    pub block_height: u64,
    /// Unique identifier for deduplication (e.g., proof hash)
    pub event_id: [u8; 32],
    /// Type of work event
    pub event_type: WorkEventType,
}

/// Types of verifiable relay work
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum WorkEventType {
    /// Proof of delivery (message delivered with recipient signature)
    ProofOfDelivery,
    /// Proof of relay work (PoRW consensus participation)  
    ProofOfRelayWork,
    /// Proof of transit (message routing proof)
    ProofOfTransit,
}

/// Eligibility result for a single relay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayEligibility {
    pub relay_id: String,
    pub operator: UserId,
    pub stake: u64,
    pub is_eligible: bool,
    /// Uptime ratio (0.0 to 1.0)
    pub uptime_ratio: f64,
    /// Number of "up" slots
    pub up_slots: u64,
    /// Total slots in epoch
    pub total_slots: u64,
    /// Total work events in epoch (after dedup)
    pub work_events: u64,
    /// Reason for ineligibility (if not eligible)
    pub ineligibility_reason: Option<IneligibilityReason>,
}

/// Reason a relay was deemed ineligible
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IneligibilityReason {
    /// Relay was registered after epoch started
    RegisteredAfterEpochStart {
        registered_block: u64,
        epoch_start_block: u64,
    },
    /// Relay is suspended/jailed/slashed
    Suspended,
    /// Uptime ratio below threshold
    InsufficientUptime { ratio: f64, required: f64 },
    /// Work events below threshold
    InsufficientWorkEvents { count: u64, required: u64 },
}

/// Computes epoch block range
pub fn epoch_block_range(epoch: u64) -> (u64, u64) {
    let start = epoch * EPOCH_LENGTH_BLOCKS;
    let end = start + EPOCH_LENGTH_BLOCKS - 1;
    (start, end)
}

/// Computes which slot a block belongs to within an epoch
pub fn block_to_slot(block_height: u64, epoch: u64) -> Option<u64> {
    let (epoch_start, epoch_end) = epoch_block_range(epoch);
    if block_height < epoch_start || block_height > epoch_end {
        return None;
    }
    Some((block_height - epoch_start) / BLOCKS_PER_SLOT)
}

/// Compute relay eligibility for an epoch
///
/// # Arguments
/// * `epoch` - The epoch number to compute eligibility for
/// * `registered_relays` - All registered relays from the registry
/// * `work_events` - All work events (will be filtered to epoch range)
/// * `config` - Eligibility configuration
///
/// # Returns
/// Vector of eligibility results for each relay
pub fn compute_relay_eligibility(
    epoch: u64,
    registered_relays: &[RegisteredRelay],
    work_events: &[RelayWorkEvent],
    config: &EligibilityConfig,
) -> Vec<RelayEligibility> {
    let (epoch_start, epoch_end) = epoch_block_range(epoch);
    let total_slots = config.slots_per_epoch;

    // Filter and deduplicate work events within epoch
    let epoch_events: HashSet<&RelayWorkEvent> = work_events
        .iter()
        .filter(|e| e.block_height >= epoch_start && e.block_height <= epoch_end)
        .collect();

    // Group events by relay
    let mut events_by_relay: HashMap<&str, Vec<&RelayWorkEvent>> = HashMap::new();
    for event in &epoch_events {
        events_by_relay
            .entry(&event.relay_id)
            .or_default()
            .push(event);
    }

    let mut results = Vec::with_capacity(registered_relays.len());

    for relay in registered_relays {
        // Check registration before epoch start
        if relay.registered_at_block >= epoch_start {
            results.push(RelayEligibility {
                relay_id: relay.relay_id.clone(),
                operator: relay.operator.clone(),
                stake: relay.stake,
                is_eligible: false,
                uptime_ratio: 0.0,
                up_slots: 0,
                total_slots,
                work_events: 0,
                ineligibility_reason: Some(IneligibilityReason::RegisteredAfterEpochStart {
                    registered_block: relay.registered_at_block,
                    epoch_start_block: epoch_start,
                }),
            });
            continue;
        }

        // Check suspension status
        if relay.is_suspended {
            results.push(RelayEligibility {
                relay_id: relay.relay_id.clone(),
                operator: relay.operator.clone(),
                stake: relay.stake,
                is_eligible: false,
                uptime_ratio: 0.0,
                up_slots: 0,
                total_slots,
                work_events: 0,
                ineligibility_reason: Some(IneligibilityReason::Suspended),
            });
            continue;
        }

        // Get relay's events
        let relay_events = events_by_relay.get(relay.relay_id.as_str());
        let event_count = relay_events.map(|v| v.len() as u64).unwrap_or(0);

        // Compute slot coverage (uptime)
        let mut covered_slots: HashSet<u64> = HashSet::new();
        if let Some(events) = relay_events {
            for event in events {
                if let Some(slot) = block_to_slot(event.block_height, epoch) {
                    covered_slots.insert(slot);
                }
            }
        }

        let up_slots = covered_slots.len() as u64;
        let uptime_ratio = if total_slots > 0 {
            up_slots as f64 / total_slots as f64
        } else {
            0.0
        };

        // Check work events gate first (more fundamental - no work = obviously ineligible)
        if event_count < config.min_work_events {
            results.push(RelayEligibility {
                relay_id: relay.relay_id.clone(),
                operator: relay.operator.clone(),
                stake: relay.stake,
                is_eligible: false,
                uptime_ratio,
                up_slots,
                total_slots,
                work_events: event_count,
                ineligibility_reason: Some(IneligibilityReason::InsufficientWorkEvents {
                    count: event_count,
                    required: config.min_work_events,
                }),
            });
            continue;
        }

        // Check uptime gate
        if uptime_ratio < config.min_uptime_ratio {
            results.push(RelayEligibility {
                relay_id: relay.relay_id.clone(),
                operator: relay.operator.clone(),
                stake: relay.stake,
                is_eligible: false,
                uptime_ratio,
                up_slots,
                total_slots,
                work_events: event_count,
                ineligibility_reason: Some(IneligibilityReason::InsufficientUptime {
                    ratio: uptime_ratio,
                    required: config.min_uptime_ratio,
                }),
            });
            continue;
        }

        // Relay is eligible
        results.push(RelayEligibility {
            relay_id: relay.relay_id.clone(),
            operator: relay.operator.clone(),
            stake: relay.stake,
            is_eligible: true,
            uptime_ratio,
            up_slots,
            total_slots,
            work_events: event_count,
            ineligibility_reason: None,
        });
    }

    results
}

/// Aggregate eligible relays into recipients for fee distribution
///
/// Groups relays by operator, sums stakes, returns deterministically sorted list
///
/// # Returns
/// - `Some(recipients)` if there are eligible relays (sorted deterministically by operator, no zeros/dupes)
/// - `None` if no eligible relays
pub fn aggregate_eligible_relays(
    eligibility_results: &[RelayEligibility],
) -> Option<Vec<(UserId, u64)>> {
    // Group by operator (using HashMap since UserId doesn't implement Ord)
    let mut operator_stakes: HashMap<UserId, u64> = HashMap::new();

    for result in eligibility_results {
        if result.is_eligible && result.stake > 0 {
            *operator_stakes.entry(result.operator.clone()).or_insert(0) += result.stake;
        }
    }

    if operator_stakes.is_empty() {
        return None;
    }

    // Convert to Vec and sort deterministically by UserId string representation
    let mut recipients: Vec<(UserId, u64)> = operator_stakes
        .into_iter()
        .filter(|(_, stake)| *stake > 0)
        .collect();

    // Sort by UserId's string representation for deterministic ordering
    recipients.sort_by(|a, b| a.0.to_string().cmp(&b.0.to_string()));

    if recipients.is_empty() {
        None
    } else {
        Some(recipients)
    }
}

/// Summary of epoch eligibility computation for logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EligibilitySummary {
    pub epoch: u64,
    pub total_relays: usize,
    pub eligible_relays: usize,
    pub ineligible_relays: usize,
    pub total_eligible_stake: u64,
    pub unique_operators: usize,
    pub ineligibility_breakdown: IneligibilityBreakdown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IneligibilityBreakdown {
    pub registered_after_start: usize,
    pub suspended: usize,
    pub insufficient_uptime: usize,
    pub insufficient_work_events: usize,
}

/// Generate a summary of eligibility results
pub fn summarize_eligibility(epoch: u64, results: &[RelayEligibility]) -> EligibilitySummary {
    let mut breakdown = IneligibilityBreakdown::default();
    let mut total_eligible_stake = 0u64;
    let mut eligible_operators: HashSet<UserId> = HashSet::new();
    let mut eligible_count = 0;

    for result in results {
        if result.is_eligible {
            eligible_count += 1;
            total_eligible_stake += result.stake;
            eligible_operators.insert(result.operator.clone());
        } else if let Some(ref reason) = result.ineligibility_reason {
            match reason {
                IneligibilityReason::RegisteredAfterEpochStart { .. } => {
                    breakdown.registered_after_start += 1;
                }
                IneligibilityReason::Suspended => {
                    breakdown.suspended += 1;
                }
                IneligibilityReason::InsufficientUptime { .. } => {
                    breakdown.insufficient_uptime += 1;
                }
                IneligibilityReason::InsufficientWorkEvents { .. } => {
                    breakdown.insufficient_work_events += 1;
                }
            }
        }
    }

    EligibilitySummary {
        epoch,
        total_relays: results.len(),
        eligible_relays: eligible_count,
        ineligible_relays: results.len() - eligible_count,
        total_eligible_stake,
        unique_operators: eligible_operators.len(),
        ineligibility_breakdown: breakdown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_relay(
        id: &str,
        operator_num: u8,
        stake: u64,
        registered_block: u64,
    ) -> RegisteredRelay {
        RegisteredRelay {
            relay_id: id.to_string(),
            operator: UserId(uuid::Uuid::from_bytes([operator_num; 16])),
            stake,
            registered_at_block: registered_block,
            is_suspended: false,
        }
    }

    fn make_work_event(relay_id: &str, block_height: u64, event_num: u8) -> RelayWorkEvent {
        RelayWorkEvent {
            relay_id: relay_id.to_string(),
            block_height,
            event_id: [event_num; 32],
            event_type: WorkEventType::ProofOfDelivery,
        }
    }

    #[test]
    fn test_epoch_block_range() {
        assert_eq!(epoch_block_range(0), (0, 1799));
        assert_eq!(epoch_block_range(1), (1800, 3599));
        assert_eq!(epoch_block_range(10), (18000, 19799));
    }

    #[test]
    fn test_block_to_slot() {
        // Epoch 0: blocks 0-1799, 60 slots of 30 blocks each
        assert_eq!(block_to_slot(0, 0), Some(0));
        assert_eq!(block_to_slot(29, 0), Some(0));
        assert_eq!(block_to_slot(30, 0), Some(1));
        assert_eq!(block_to_slot(1799, 0), Some(59));

        // Block outside epoch returns None
        assert_eq!(block_to_slot(1800, 0), None);
        assert_eq!(block_to_slot(0, 1), None);
    }

    #[test]
    fn test_eligibility_requires_both_gates() {
        let config = EligibilityConfig {
            min_uptime_ratio: 0.80,
            min_work_events: 1,
            slots_per_epoch: 60,
        };

        // Relay registered before epoch 0 starts
        let relays = vec![make_relay("relay1", 1, 1000, 0)];

        // No work events - fails work events gate
        let results = compute_relay_eligibility(1, &relays, &[], &config);
        assert!(!results[0].is_eligible);
        assert!(matches!(
            results[0].ineligibility_reason,
            Some(IneligibilityReason::InsufficientWorkEvents { .. })
        ));

        // Work events in only 10 slots (16.7% uptime) - fails uptime gate
        let mut work_events: Vec<RelayWorkEvent> = Vec::new();
        for i in 0..10 {
            work_events.push(make_work_event("relay1", 1800 + i * 30, i as u8));
        }
        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        assert!(!results[0].is_eligible);
        assert!(matches!(
            results[0].ineligibility_reason,
            Some(IneligibilityReason::InsufficientUptime { .. })
        ));
    }

    #[test]
    fn test_eligibility_passes_both_gates() {
        let config = EligibilityConfig::default();
        let relays = vec![make_relay("relay1", 1, 1000, 0)];

        // Work events covering >= 80% of slots (48 slots)
        let mut work_events: Vec<RelayWorkEvent> = Vec::new();
        for i in 0..48 {
            work_events.push(make_work_event("relay1", 1800 + i * 30, i as u8));
        }

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        assert!(results[0].is_eligible);
        assert_eq!(results[0].up_slots, 48);
        assert!(results[0].uptime_ratio >= 0.80);
        assert_eq!(results[0].work_events, 48);
    }

    #[test]
    fn test_registration_after_epoch_start() {
        let config = EligibilityConfig::default();

        // Relay registered at block 1800 (epoch 1 starts at 1800)
        let relays = vec![make_relay("relay1", 1, 1000, 1800)];

        // Plenty of work events
        let work_events: Vec<RelayWorkEvent> = (0..60)
            .map(|i| make_work_event("relay1", 1800 + i * 30, i as u8))
            .collect();

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        assert!(!results[0].is_eligible);
        assert!(matches!(
            results[0].ineligibility_reason,
            Some(IneligibilityReason::RegisteredAfterEpochStart { .. })
        ));
    }

    #[test]
    fn test_suspended_relay_ineligible() {
        let config = EligibilityConfig::default();
        let mut relay = make_relay("relay1", 1, 1000, 0);
        relay.is_suspended = true;
        let relays = vec![relay];

        let work_events: Vec<RelayWorkEvent> = (0..60)
            .map(|i| make_work_event("relay1", 1800 + i * 30, i as u8))
            .collect();

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        assert!(!results[0].is_eligible);
        assert!(matches!(
            results[0].ineligibility_reason,
            Some(IneligibilityReason::Suspended)
        ));
    }

    #[test]
    fn test_aggregation_by_operator() {
        let config = EligibilityConfig::default();

        // Two relays with same operator
        let operator_uuid = uuid::Uuid::from_bytes([1; 16]);
        let relays = vec![
            RegisteredRelay {
                relay_id: "relay1".to_string(),
                operator: UserId(operator_uuid),
                stake: 1000,
                registered_at_block: 0,
                is_suspended: false,
            },
            RegisteredRelay {
                relay_id: "relay2".to_string(),
                operator: UserId(operator_uuid),
                stake: 2000,
                registered_at_block: 0,
                is_suspended: false,
            },
        ];

        // Both have enough work
        let mut work_events: Vec<RelayWorkEvent> = Vec::new();
        for i in 0..48 {
            work_events.push(make_work_event("relay1", 1800 + i * 30, i as u8));
            work_events.push(make_work_event("relay2", 1800 + i * 30, (i + 100) as u8));
        }

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        assert!(results.iter().all(|r| r.is_eligible));

        let recipients = aggregate_eligible_relays(&results).unwrap();

        // Should aggregate to single operator
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0].0, UserId(operator_uuid));
        assert_eq!(recipients[0].1, 3000); // 1000 + 2000
    }

    #[test]
    fn test_deterministic_ordering() {
        let config = EligibilityConfig::default();

        // Multiple operators with different UUIDs
        let relays: Vec<RegisteredRelay> = (1..=5)
            .map(|i| make_relay(&format!("relay{}", i), i, 1000, 0))
            .collect();

        let work_events: Vec<RelayWorkEvent> = (0..48)
            .flat_map(|slot| {
                (1..=5).map(move |i| {
                    make_work_event(
                        &format!("relay{}", i),
                        1800 + slot * 30,
                        (slot * 10 + i) as u8,
                    )
                })
            })
            .collect();

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        let recipients1 = aggregate_eligible_relays(&results).unwrap();

        // Shuffle and recompute - should get same order
        let mut shuffled_relays = relays.clone();
        shuffled_relays.reverse();
        let results2 = compute_relay_eligibility(1, &shuffled_relays, &work_events, &config);
        let recipients2 = aggregate_eligible_relays(&results2).unwrap();

        assert_eq!(recipients1, recipients2);
    }

    #[test]
    fn test_empty_eligible_set() {
        let config = EligibilityConfig::default();

        // All relays ineligible (registered too late)
        let relays = vec![make_relay("relay1", 1, 1000, 1800)];
        let work_events: Vec<RelayWorkEvent> = Vec::new();

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        let recipients = aggregate_eligible_relays(&results);

        assert!(recipients.is_none());
    }

    #[test]
    fn test_work_event_deduplication() {
        let config = EligibilityConfig::default();
        let relays = vec![make_relay("relay1", 1, 1000, 0)];

        // Duplicate work events (same event_id)
        let mut work_events: Vec<RelayWorkEvent> = Vec::new();
        for i in 0..48 {
            let event = make_work_event("relay1", 1800 + i * 30, i as u8);
            work_events.push(event.clone());
            work_events.push(event); // Duplicate
        }

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        // Should still only count 48 unique events
        assert_eq!(results[0].work_events, 48);
        assert!(results[0].is_eligible);
    }

    #[test]
    fn test_slot_based_uptime_counts_distinct_slots() {
        let config = EligibilityConfig::default();
        let relays = vec![make_relay("relay1", 1, 1000, 0)];

        // Multiple events in same slot shouldn't increase uptime
        let mut work_events: Vec<RelayWorkEvent> = Vec::new();
        for i in 0..100 {
            // All events in slot 0 (blocks 1800-1829)
            work_events.push(make_work_event("relay1", 1800, i as u8));
        }

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);

        // Should have 100 work events but only 1 up slot
        assert_eq!(results[0].work_events, 100);
        assert_eq!(results[0].up_slots, 1);
        assert!(!results[0].is_eligible); // Uptime too low (1/60 = 1.67%)
    }

    #[test]
    fn test_summary_generation() {
        let config = EligibilityConfig::default();

        let relays = vec![
            make_relay("eligible1", 1, 1000, 0),
            make_relay("eligible2", 2, 2000, 0),
            make_relay("late_reg", 3, 500, 1800),
            RegisteredRelay {
                relay_id: "suspended".to_string(),
                operator: UserId(uuid::Uuid::from_bytes([4; 16])),
                stake: 750,
                registered_at_block: 0,
                is_suspended: true,
            },
        ];

        let work_events: Vec<RelayWorkEvent> = (0..48)
            .flat_map(|slot| {
                vec![
                    make_work_event("eligible1", 1800 + slot * 30, slot as u8),
                    make_work_event("eligible2", 1800 + slot * 30, (slot + 100) as u8),
                ]
            })
            .collect();

        let results = compute_relay_eligibility(1, &relays, &work_events, &config);
        let summary = summarize_eligibility(1, &results);

        assert_eq!(summary.epoch, 1);
        assert_eq!(summary.total_relays, 4);
        assert_eq!(summary.eligible_relays, 2);
        assert_eq!(summary.ineligible_relays, 2);
        assert_eq!(summary.total_eligible_stake, 3000);
        assert_eq!(summary.unique_operators, 2);
        assert_eq!(summary.ineligibility_breakdown.registered_after_start, 1);
        assert_eq!(summary.ineligibility_breakdown.suspended, 1);
    }
}
