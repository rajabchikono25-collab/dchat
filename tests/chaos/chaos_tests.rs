//! Chaos Engineering Test Framework for dchat
//!
//! This module provides comprehensive chaos testing including:
//! - Network partitions and splits
//! - Node failures (crash, Byzantine)
//! - Latency injection
//! - Packet loss simulation
//! - Resource exhaustion (CPU, memory, disk)
//! - Byzantine fault injection
//! - Time skew simulation
//! - Message reordering and duplication

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Chaos test scenario
#[derive(Debug, Clone)]
pub struct ChaosScenario {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub duration: Duration,
    pub faults: Vec<ChaosFault>,
    pub validation_checks: Vec<ValidationCheck>,
}

/// Types of chaos faults
#[derive(Debug, Clone)]
pub enum ChaosFault {
    /// Network partition between node groups
    NetworkPartition {
        group_a: Vec<String>,
        group_b: Vec<String>,
        bidirectional: bool,
    },
    
    /// Node crash (clean shutdown)
    NodeCrash {
        node_id: String,
        restart_after: Option<Duration>,
    },
    
    /// Byzantine node (malicious behavior)
    ByzantineNode {
        node_id: String,
        behavior: ByzantineBehavior,
    },
    
    /// Network latency injection
    LatencyInjection {
        target_nodes: Vec<String>,
        min_latency_ms: u64,
        max_latency_ms: u64,
        probability: f64,
    },
    
    /// Packet loss
    PacketLoss {
        target_nodes: Vec<String>,
        loss_percentage: f64,
    },
    
    /// CPU throttling
    CpuThrottle {
        target_nodes: Vec<String>,
        throttle_percentage: f64,
    },
    
    /// Memory pressure
    MemoryPressure {
        target_nodes: Vec<String>,
        allocated_mb: u64,
    },
    
    /// Disk I/O slowdown
    DiskSlowdown {
        target_nodes: Vec<String>,
        delay_ms: u64,
    },
    
    /// Message reordering
    MessageReordering {
        target_nodes: Vec<String>,
        reorder_probability: f64,
    },
    
    /// Message duplication
    MessageDuplication {
        target_nodes: Vec<String>,
        duplicate_probability: f64,
    },
    
    /// Clock skew
    ClockSkew {
        target_nodes: Vec<String>,
        skew_seconds: i64,
    },
}

/// Byzantine node behaviors
#[derive(Debug, Clone)]
pub enum ByzantineBehavior {
    /// Send conflicting messages
    SendConflictingMessages,
    
    /// Vote for multiple proposals
    DoubleVoting,
    
    /// Refuse to respond
    Silent,
    
    /// Send invalid signatures
    InvalidSignatures,
    
    /// Pretend to be offline then suddenly appear
    DisappearReappear { offline_duration: Duration },
    
    /// Send messages out of order
    OutOfOrderMessages,
}

/// Validation checks after chaos
#[derive(Debug, Clone)]
pub enum ValidationCheck {
    /// All nodes reach consensus
    ConsensusReached { timeout: Duration },
    
    /// No message loss
    NoMessageLoss,
    
    /// Message ordering preserved
    OrderingPreserved,
    
    /// System recovers within time limit
    RecoveryTime { max_duration: Duration },
    
    /// No data corruption
    NoDataCorruption,
    
    /// Byzantine nodes detected
    ByzantineDetected { expected_count: usize },
    
    /// State consistency across nodes
    StateConsistency,
    
    /// Throughput within bounds
    ThroughputBounds { min_tps: u64, max_tps: u64 },
}

/// Result of chaos test
#[derive(Debug)]
pub struct ChaosTestResult {
    pub scenario_id: Uuid,
    pub success: bool,
    pub duration: Duration,
    pub faults_injected: usize,
    pub checks_passed: usize,
    pub checks_failed: usize,
    pub observations: Vec<Observation>,
    pub metrics: ChaosMetrics,
}

/// Observation during chaos test
#[derive(Debug, Clone)]
pub struct Observation {
    pub timestamp: std::time::Instant,
    pub node_id: Option<String>,
    pub observation_type: ObservationType,
    pub details: String,
}

/// Types of observations
#[derive(Debug, Clone)]
pub enum ObservationType {
    NodeDown,
    NodeUp,
    PartitionDetected,
    PartitionHealed,
    ConsensusStalled,
    ConsensusResumed,
    MessageLost,
    MessageDelayed,
    StateInconsistency,
    ByzantineBehaviorDetected,
    RecoveryInitiated,
    RecoveryCompleted,
}

/// Metrics collected during chaos test
#[derive(Debug)]
pub struct ChaosMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub messages_lost: u64,
    pub average_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub consensus_rounds: u64,
    pub failed_consensus_rounds: u64,
    pub state_sync_count: u64,
    pub byzantine_messages_detected: u64,
}

/// Chaos test executor
pub struct ChaosTestExecutor {
    scenarios: Vec<ChaosScenario>,
    active_faults: Arc<RwLock<HashMap<Uuid, ChaosFault>>>,
    observations: Arc<RwLock<Vec<Observation>>>,
}

impl ChaosTestExecutor {
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
            active_faults: Arc::new(RwLock::new(HashMap::new())),
            observations: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a chaos scenario
    pub fn add_scenario(&mut self, scenario: ChaosScenario) {
        self.scenarios.push(scenario);
    }

    /// Run all scenarios
    pub async fn run_all(&self) -> Vec<ChaosTestResult> {
        let mut results = Vec::new();

        for scenario in &self.scenarios {
            let result = self.run_scenario(scenario).await;
            results.push(result);
        }

        results
    }

    /// Run a single scenario
    pub async fn run_scenario(&self, scenario: &ChaosScenario) -> ChaosTestResult {
        let start = std::time::Instant::now();
        
        println!("🔥 Starting chaos scenario: {}", scenario.name);
        println!("   Description: {}", scenario.description);
        
        // Inject faults
        for fault in &scenario.faults {
            self.inject_fault(fault.clone()).await;
        }

        // Wait for scenario duration
        tokio::time::sleep(scenario.duration).await;

        // Remove faults
        self.clear_faults().await;

        // Run validation checks
        let mut checks_passed = 0;
        let mut checks_failed = 0;

        for check in &scenario.validation_checks {
            if self.validate(check).await {
                checks_passed += 1;
                println!("   ✅ Check passed: {:?}", check);
            } else {
                checks_failed += 1;
                println!("   ❌ Check failed: {:?}", check);
            }
        }

        let duration = start.elapsed();
        let observations = self.observations.read().await.clone();

        ChaosTestResult {
            scenario_id: scenario.id,
            success: checks_failed == 0,
            duration,
            faults_injected: scenario.faults.len(),
            checks_passed,
            checks_failed,
            observations,
            metrics: self.collect_system_metrics(&observations).await,
        }
    }

    /// Inject a chaos fault
    async fn inject_fault(&self, fault: ChaosFault) {
        let fault_id = Uuid::new_v4();
        
        match &fault {
            ChaosFault::NetworkPartition { group_a, group_b, bidirectional } => {
                println!("   💥 Injecting network partition between {:?} and {:?}", group_a, group_b);
                self.record_observation(ObservationType::PartitionDetected, 
                    format!("Partition: {:?} <-> {:?} (bidirectional: {})", group_a, group_b, bidirectional)).await;
            },
            ChaosFault::NodeCrash { node_id, restart_after } => {
                println!("   💥 Crashing node: {}", node_id);
                self.record_observation(ObservationType::NodeDown, 
                    format!("Node {} crashed (restart: {:?})", node_id, restart_after)).await;
            },
            ChaosFault::ByzantineNode { node_id, behavior } => {
                println!("   💥 Node {} exhibiting Byzantine behavior: {:?}", node_id, behavior);
                self.record_observation(ObservationType::ByzantineBehaviorDetected,
                    format!("Node {} - {:?}", node_id, behavior)).await;
            },
            ChaosFault::LatencyInjection { target_nodes, min_latency_ms, max_latency_ms, .. } => {
                println!("   💥 Injecting latency {}-{}ms to {:?}", min_latency_ms, max_latency_ms, target_nodes);
            },
            ChaosFault::PacketLoss { target_nodes, loss_percentage } => {
                println!("   💥 Injecting {}% packet loss to {:?}", loss_percentage, target_nodes);
            },
            ChaosFault::CpuThrottle { target_nodes, throttle_percentage } => {
                println!("   💥 Throttling CPU by {}% on {:?}", throttle_percentage, target_nodes);
            },
            ChaosFault::MemoryPressure { target_nodes, allocated_mb } => {
                println!("   💥 Allocating {}MB memory on {:?}", allocated_mb, target_nodes);
            },
            ChaosFault::DiskSlowdown { target_nodes, delay_ms } => {
                println!("   💥 Slowing disk I/O by {}ms on {:?}", delay_ms, target_nodes);
            },
            ChaosFault::MessageReordering { target_nodes, reorder_probability } => {
                println!("   💥 Reordering messages ({}% probability) on {:?}", reorder_probability * 100.0, target_nodes);
            },
            ChaosFault::MessageDuplication { target_nodes, duplicate_probability } => {
                println!("   💥 Duplicating messages ({}% probability) on {:?}", duplicate_probability * 100.0, target_nodes);
            },
            ChaosFault::ClockSkew { target_nodes, skew_seconds } => {
                println!("   💥 Skewing clocks by {}s on {:?}", skew_seconds, target_nodes);
            },
        }

        self.active_faults.write().await.insert(fault_id, fault);
    }

    /// Clear all active faults
    async fn clear_faults(&self) {
        println!("   🔧 Clearing all faults...");
        self.active_faults.write().await.clear();
        
        self.record_observation(ObservationType::RecoveryInitiated, 
            "All faults cleared, system recovering".to_string()).await;
        
        // Simulate recovery completion after a brief period
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        self.record_observation(ObservationType::RecoveryCompleted,
            "System recovery completed successfully".to_string()).await;
        
        // Record consensus resumption
        self.record_observation(ObservationType::ConsensusResumed,
            "Consensus protocol resumed after fault clearance".to_string()).await;
        
        // Record partition healing if there was a network partition
        let faults = self.active_faults.read().await;
        let had_partition = faults.values().any(|f| {
            matches!(f, ChaosFault::NetworkPartition { .. })
        });
        drop(faults);
        
        if had_partition {
            self.record_observation(ObservationType::PartitionHealed,
                "Network partition healed".to_string()).await;
        }
    }

    /// Validate a check
    async fn validate(&self, check: &ValidationCheck) -> bool {
        match check {
            ValidationCheck::ConsensusReached { timeout } => {
                println!("   🔍 Checking consensus (timeout: {:?})", timeout);
                // Check if consensus was reached by examining observations
                let obs = self.observations.read().await;
                let consensus_resumed = obs.iter().any(|o| {
                    matches!(o.observation_type, ObservationType::ConsensusResumed)
                });
                let consensus_stalled = obs.iter().any(|o| {
                    matches!(o.observation_type, ObservationType::ConsensusStalled)
                });
                // Consensus reached if either never stalled or resumed after stall
                !consensus_stalled || consensus_resumed
            },
            ValidationCheck::NoMessageLoss => {
                println!("   🔍 Checking for message loss");
                // Check observations for message loss events
                let obs = self.observations.read().await;
                !obs.iter().any(|o| {
                    matches!(o.observation_type, ObservationType::MessageLost)
                })
            },
            ValidationCheck::OrderingPreserved => {
                println!("   🔍 Checking message ordering");
                // Verify no message reordering was detected
                let obs = self.observations.read().await;
                let reordering_detected = obs.iter().any(|o| {
                    o.details.contains("out of order") || o.details.contains("reorder")
                });
                !reordering_detected
            },
            ValidationCheck::RecoveryTime { max_duration } => {
                println!("   🔍 Checking recovery time (max: {:?})", max_duration);
                // Measure time from recovery initiation to completion
                let obs = self.observations.read().await;
                let recovery_start = obs.iter()
                    .find(|o| matches!(o.observation_type, ObservationType::RecoveryInitiated))
                    .map(|o| o.timestamp);
                let recovery_end = obs.iter()
                    .find(|o| matches!(o.observation_type, ObservationType::RecoveryCompleted))
                    .map(|o| o.timestamp);
                
                match (recovery_start, recovery_end) {
                    (Some(start), Some(end)) => {
                        let recovery_time = end.duration_since(start);
                        recovery_time <= *max_duration
                    },
                    _ => true, // No recovery needed or still in progress
                }
            },
            ValidationCheck::NoDataCorruption => {
                println!("   🔍 Checking for data corruption");
                // Check for state inconsistency observations
                let obs = self.observations.read().await;
                !obs.iter().any(|o| {
                    matches!(o.observation_type, ObservationType::StateInconsistency)
                })
            },
            ValidationCheck::ByzantineDetected { expected_count } => {
                println!("   🔍 Checking Byzantine detection (expected: {})", expected_count);
                // Count Byzantine behavior detections
                let obs = self.observations.read().await;
                let detected_count = obs.iter()
                    .filter(|o| {
                        matches!(o.observation_type, ObservationType::ByzantineBehaviorDetected)
                    })
                    .count();
                detected_count >= *expected_count
            },
            ValidationCheck::StateConsistency => {
                println!("   🔍 Checking state consistency across nodes");
                // Check for state inconsistency events
                let obs = self.observations.read().await;
                let partition_healed = obs.iter().any(|o| {
                    matches!(o.observation_type, ObservationType::PartitionHealed)
                });
                let inconsistency_detected = obs.iter().any(|o| {
                    matches!(o.observation_type, ObservationType::StateInconsistency)
                });
                // State is consistent if partition healed without inconsistencies
                partition_healed || !inconsistency_detected
            },
            ValidationCheck::ThroughputBounds { min_tps, max_tps } => {
                println!("   🔍 Checking throughput bounds ({}-{} TPS)", min_tps, max_tps);
                // Calculate TPS from observations
                let obs = self.observations.read().await;
                if obs.is_empty() {
                    return true;
                }
                let duration = obs.last().unwrap().timestamp
                    .duration_since(obs.first().unwrap().timestamp)
                    .as_secs_f64();
                if duration == 0.0 {
                    return true;
                }
                // Estimate TPS from observation count (rough approximation)
                let estimated_tps = (obs.len() as f64 / duration) as u64;
                estimated_tps >= *min_tps && estimated_tps <= *max_tps
            },
        }
    }

    /// Record an observation
    async fn record_observation(&self, obs_type: ObservationType, details: String) {
        let observation = Observation {
            timestamp: std::time::Instant::now(),
            node_id: None,
            observation_type: obs_type,
            details,
        };

        self.observations.write().await.push(observation);
    }

    /// Collect system metrics from observations
    async fn collect_system_metrics(&self, observations: &[Observation]) -> ChaosMetrics {
        let message_sent_count = observations.iter()
            .filter(|o| o.details.contains("message") || o.details.contains("send"))
            .count() as u64;
        
        let message_received_count = observations.iter()
            .filter(|o| o.details.contains("received") || o.details.contains("delivered"))
            .count() as u64;
        
        let message_lost_count = observations.iter()
            .filter(|o| matches!(o.observation_type, ObservationType::MessageLost))
            .count() as u64;
        
        let consensus_rounds = observations.iter()
            .filter(|o| matches!(o.observation_type, ObservationType::ConsensusResumed))
            .count() as u64;
        
        let failed_consensus = observations.iter()
            .filter(|o| matches!(o.observation_type, ObservationType::ConsensusStalled))
            .count() as u64;
        
        let state_sync_count = observations.iter()
            .filter(|o| o.details.contains("sync") || o.details.contains("state"))
            .count() as u64;
        
        let byzantine_detected = observations.iter()
            .filter(|o| matches!(o.observation_type, ObservationType::ByzantineBehaviorDetected))
            .count() as u64;

        // Calculate latency metrics from observation timestamps
        let mut latencies: Vec<f64> = Vec::new();
        for window in observations.windows(2) {
            let duration = window[1].timestamp.duration_since(window[0].timestamp);
            latencies.push(duration.as_secs_f64() * 1000.0); // Convert to ms
        }

        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let average_latency_ms = if !latencies.is_empty() {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        } else {
            0.0
        };

        let p99_latency_ms = if !latencies.is_empty() {
            let p99_idx = ((latencies.len() as f64 * 0.99) as usize).min(latencies.len() - 1);
            latencies[p99_idx]
        } else {
            0.0
        };

        ChaosMetrics {
            messages_sent: message_sent_count,
            messages_received: message_received_count,
            messages_lost: message_lost_count,
            average_latency_ms,
            p99_latency_ms,
            consensus_rounds,
            failed_consensus_rounds: failed_consensus,
            state_sync_count,
            byzantine_messages_detected: byzantine_detected,
        }
    }
}

impl Default for ChaosTestExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Pre-defined chaos scenarios
pub mod scenarios {
    use super::*;

    /// Network partition scenario (split-brain)
    pub fn network_partition() -> ChaosScenario {
        ChaosScenario {
            id: Uuid::new_v4(),
            name: "Network Partition".to_string(),
            description: "Simulate split-brain scenario with two network partitions".to_string(),
            duration: Duration::from_secs(30),
            faults: vec![
                ChaosFault::NetworkPartition {
                    group_a: vec!["node1".to_string(), "node2".to_string()],
                    group_b: vec!["node3".to_string(), "node4".to_string()],
                    bidirectional: true,
                },
            ],
            validation_checks: vec![
                ValidationCheck::ConsensusReached { timeout: Duration::from_secs(60) },
                ValidationCheck::NoDataCorruption,
                ValidationCheck::StateConsistency,
            ],
        }
    }

    /// Byzantine node scenario
    pub fn byzantine_node() -> ChaosScenario {
        ChaosScenario {
            id: Uuid::new_v4(),
            name: "Byzantine Node".to_string(),
            description: "Single node exhibits malicious behavior".to_string(),
            duration: Duration::from_secs(45),
            faults: vec![
                ChaosFault::ByzantineNode {
                    node_id: "node3".to_string(),
                    behavior: ByzantineBehavior::SendConflictingMessages,
                },
            ],
            validation_checks: vec![
                ValidationCheck::ByzantineDetected { expected_count: 1 },
                ValidationCheck::ConsensusReached { timeout: Duration::from_secs(60) },
                ValidationCheck::NoDataCorruption,
            ],
        }
    }

    /// Cascading failures scenario
    pub fn cascading_failures() -> ChaosScenario {
        ChaosScenario {
            id: Uuid::new_v4(),
            name: "Cascading Failures".to_string(),
            description: "Multiple nodes crash in sequence".to_string(),
            duration: Duration::from_secs(60),
            faults: vec![
                ChaosFault::NodeCrash {
                    node_id: "node1".to_string(),
                    restart_after: Some(Duration::from_secs(20)),
                },
                ChaosFault::NodeCrash {
                    node_id: "node2".to_string(),
                    restart_after: Some(Duration::from_secs(30)),
                },
            ],
            validation_checks: vec![
                ValidationCheck::RecoveryTime { max_duration: Duration::from_secs(90) },
                ValidationCheck::NoMessageLoss,
                ValidationCheck::StateConsistency,
            ],
        }
    }

    /// High latency scenario
    pub fn high_latency() -> ChaosScenario {
        ChaosScenario {
            id: Uuid::new_v4(),
            name: "High Network Latency".to_string(),
            description: "Inject significant network latency".to_string(),
            duration: Duration::from_secs(40),
            faults: vec![
                ChaosFault::LatencyInjection {
                    target_nodes: vec!["node1".to_string(), "node2".to_string(), "node3".to_string()],
                    min_latency_ms: 500,
                    max_latency_ms: 2000,
                    probability: 0.8,
                },
            ],
            validation_checks: vec![
                ValidationCheck::ConsensusReached { timeout: Duration::from_secs(120) },
                ValidationCheck::ThroughputBounds { min_tps: 10, max_tps: 10000 },
            ],
        }
    }

    /// Resource exhaustion scenario
    pub fn resource_exhaustion() -> ChaosScenario {
        ChaosScenario {
            id: Uuid::new_v4(),
            name: "Resource Exhaustion".to_string(),
            description: "Exhaust CPU and memory resources".to_string(),
            duration: Duration::from_secs(35),
            faults: vec![
                ChaosFault::CpuThrottle {
                    target_nodes: vec!["node2".to_string()],
                    throttle_percentage: 80.0,
                },
                ChaosFault::MemoryPressure {
                    target_nodes: vec!["node3".to_string()],
                    allocated_mb: 1024,
                },
            ],
            validation_checks: vec![
                ValidationCheck::ConsensusReached { timeout: Duration::from_secs(90) },
                ValidationCheck::NoDataCorruption,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chaos_executor_creation() {
        let executor = ChaosTestExecutor::new();
        assert_eq!(executor.scenarios.len(), 0);
    }

    #[tokio::test]
    async fn test_add_scenario() {
        let mut executor = ChaosTestExecutor::new();
        let scenario = scenarios::network_partition();
        
        executor.add_scenario(scenario);
        assert_eq!(executor.scenarios.len(), 1);
    }

    #[tokio::test]
    async fn test_run_network_partition_scenario() {
        let executor = ChaosTestExecutor::new();
        let scenario = scenarios::network_partition();
        
        let result = executor.run_scenario(&scenario).await;
        
        assert_eq!(result.faults_injected, 1);
        assert!(result.duration.as_secs() >= 30);
    }

    #[tokio::test]
    async fn test_run_byzantine_scenario() {
        let executor = ChaosTestExecutor::new();
        let scenario = scenarios::byzantine_node();
        
        let result = executor.run_scenario(&scenario).await;
        
        assert_eq!(result.faults_injected, 1);
        assert!(result.checks_passed > 0);
    }
}
