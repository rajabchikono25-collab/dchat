//! Chaos Engineering and Testing Infrastructure
//!
//! This module provides:
//! - Network simulation and partition testing
//! - Fault injection
//! - Recovery scenario testing
//! - Distributed system chaos experiments
//! - Comprehensive chaos testing suite

pub mod chaos;

use chrono::{DateTime, Utc};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::time::Duration;

/// Types of chaos experiments
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChaosExperimentType {
    /// Network partition (split brain)
    NetworkPartition,
    /// Packet loss simulation
    PacketLoss,
    /// Latency injection
    LatencyInjection,
    /// Node crash/restart
    NodeFailure,
    /// Resource exhaustion (CPU, memory)
    ResourceExhaustion,
    /// Clock skew simulation
    ClockSkew,
    /// Byzantine node behavior
    ByzantineFault,
}

/// Network partition configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionConfig {
    pub partition_a: Vec<String>,
    pub partition_b: Vec<String>,
    pub duration_seconds: u64,
    pub allow_healing: bool,
}

/// Fault injection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultInjection {
    pub target_node: String,
    pub fault_type: ChaosExperimentType,
    pub severity: f32, // 0.0 to 1.0
    pub duration: Duration,
}

/// Chaos experiment result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentResult {
    pub experiment_id: String,
    pub experiment_type: ChaosExperimentType,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub success: bool,
    pub metrics: HashMap<String, f64>,
    pub errors: Vec<String>,
}

/// Network condition simulator
pub struct NetworkSimulator {
    latency_ms: u64,
    packet_loss_rate: f32,
    bandwidth_limit_kbps: Option<u64>,
    is_partitioned: bool,
}

impl NetworkSimulator {
    /// Create a new network simulator with default conditions
    pub fn new() -> Self {
        Self {
            latency_ms: 0,
            packet_loss_rate: 0.0,
            bandwidth_limit_kbps: None,
            is_partitioned: false,
        }
    }

    /// Set network latency
    pub fn set_latency(&mut self, latency_ms: u64) {
        self.latency_ms = latency_ms;
    }

    /// Set packet loss rate (0.0 to 1.0)
    pub fn set_packet_loss(&mut self, rate: f32) -> Result<()> {
        if !(0.0..=1.0).contains(&rate) {
            return Err(Error::validation(
                "Packet loss rate must be between 0.0 and 1.0",
            ));
        }
        self.packet_loss_rate = rate;
        Ok(())
    }

    /// Set bandwidth limit in kilobits per second
    pub fn set_bandwidth_limit(&mut self, kbps: u64) {
        self.bandwidth_limit_kbps = Some(kbps);
    }

    /// Create a network partition
    pub fn create_partition(&mut self) {
        self.is_partitioned = true;
    }

    /// Heal the network partition
    pub fn heal_partition(&mut self) {
        self.is_partitioned = false;
    }

    /// Check if network is partitioned
    pub fn is_partitioned(&self) -> bool {
        self.is_partitioned
    }

    /// Get current latency
    pub fn get_latency(&self) -> u64 {
        self.latency_ms
    }

    /// Get packet loss rate
    pub fn get_packet_loss_rate(&self) -> f32 {
        self.packet_loss_rate
    }

    /// Reset to default conditions
    pub fn reset(&mut self) {
        self.latency_ms = 0;
        self.packet_loss_rate = 0.0;
        self.bandwidth_limit_kbps = None;
        self.is_partitioned = false;
    }
}

impl Default for NetworkSimulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Chaos experiment orchestrator
pub struct ChaosOrchestrator {
    experiments: HashMap<String, ExperimentResult>,
    active_faults: Vec<FaultInjection>,
}

impl ChaosOrchestrator {
    /// Create a new chaos orchestrator
    pub fn new() -> Self {
        Self {
            experiments: HashMap::new(),
            active_faults: Vec::new(),
        }
    }

    /// Start a chaos experiment
    pub fn start_experiment(
        &mut self,
        experiment_id: String,
        experiment_type: ChaosExperimentType,
    ) -> Result<()> {
        if self.experiments.contains_key(&experiment_id) {
            return Err(Error::validation("Experiment ID already exists"));
        }

        let result = ExperimentResult {
            experiment_id: experiment_id.clone(),
            experiment_type,
            started_at: Utc::now(),
            ended_at: None,
            success: false,
            metrics: HashMap::new(),
            errors: Vec::new(),
        };

        self.experiments.insert(experiment_id, result);
        Ok(())
    }

    /// End a chaos experiment
    pub fn end_experiment(&mut self, experiment_id: &str, success: bool) -> Result<()> {
        let experiment = self
            .experiments
            .get_mut(experiment_id)
            .ok_or_else(|| Error::validation("Experiment not found"))?;

        experiment.ended_at = Some(Utc::now());
        experiment.success = success;
        Ok(())
    }

    /// Record experiment metric
    pub fn record_metric(&mut self, experiment_id: &str, key: String, value: f64) -> Result<()> {
        let experiment = self
            .experiments
            .get_mut(experiment_id)
            .ok_or_else(|| Error::validation("Experiment not found"))?;

        experiment.metrics.insert(key, value);
        Ok(())
    }

    /// Record experiment error
    pub fn record_error(&mut self, experiment_id: &str, error: String) -> Result<()> {
        let experiment = self
            .experiments
            .get_mut(experiment_id)
            .ok_or_else(|| Error::validation("Experiment not found"))?;

        experiment.errors.push(error);
        Ok(())
    }

    /// Inject a fault
    pub fn inject_fault(&mut self, fault: FaultInjection) -> Result<()> {
        self.active_faults.push(fault);
        Ok(())
    }

    /// Remove all active faults
    pub fn clear_faults(&mut self) {
        self.active_faults.clear();
    }

    /// Get experiment result
    pub fn get_experiment(&self, experiment_id: &str) -> Option<&ExperimentResult> {
        self.experiments.get(experiment_id)
    }

    /// Get all experiments
    pub fn get_all_experiments(&self) -> Vec<&ExperimentResult> {
        self.experiments.values().collect()
    }

    /// Get active faults
    pub fn get_active_faults(&self) -> &[FaultInjection] {
        &self.active_faults
    }

    /// Calculate experiment success rate
    pub fn calculate_success_rate(&self) -> f32 {
        if self.experiments.is_empty() {
            return 0.0;
        }

        let successful = self
            .experiments
            .values()
            .filter(|e| e.ended_at.is_some() && e.success)
            .count();

        successful as f32 / self.experiments.len() as f32
    }
}

impl Default for ChaosOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Recovery scenario tester
pub struct RecoveryTester {
    scenarios: Vec<RecoveryScenario>,
}

/// A recovery test scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryScenario {
    pub name: String,
    pub failure_type: ChaosExperimentType,
    pub expected_recovery_time_ms: u64,
    pub actual_recovery_time_ms: Option<u64>,
    pub recovered: bool,
}

impl RecoveryTester {
    /// Create a new recovery tester
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
        }
    }

    /// Add a recovery scenario
    pub fn add_scenario(&mut self, scenario: RecoveryScenario) {
        self.scenarios.push(scenario);
    }

    /// Mark scenario as recovered
    pub fn mark_recovered(&mut self, scenario_name: &str, recovery_time_ms: u64) -> Result<()> {
        let scenario = self
            .scenarios
            .iter_mut()
            .find(|s| s.name == scenario_name)
            .ok_or_else(|| Error::validation("Scenario not found"))?;

        scenario.recovered = true;
        scenario.actual_recovery_time_ms = Some(recovery_time_ms);
        Ok(())
    }

    /// Get all scenarios
    pub fn get_scenarios(&self) -> &[RecoveryScenario] {
        &self.scenarios
    }

    /// Calculate recovery success rate
    pub fn recovery_success_rate(&self) -> f32 {
        if self.scenarios.is_empty() {
            return 0.0;
        }

        let recovered = self.scenarios.iter().filter(|s| s.recovered).count();
        recovered as f32 / self.scenarios.len() as f32
    }
}

impl Default for RecoveryTester {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_simulator() {
        let mut sim = NetworkSimulator::new();

        sim.set_latency(100);
        assert_eq!(sim.get_latency(), 100);

        sim.set_packet_loss(0.1).unwrap();
        assert_eq!(sim.get_packet_loss_rate(), 0.1);

        sim.set_bandwidth_limit(1000);
        assert_eq!(sim.bandwidth_limit_kbps, Some(1000));
    }

    #[test]
    fn test_network_partition() {
        let mut sim = NetworkSimulator::new();

        assert!(!sim.is_partitioned());

        sim.create_partition();
        assert!(sim.is_partitioned());

        sim.heal_partition();
        assert!(!sim.is_partitioned());
    }

    #[test]
    fn test_invalid_packet_loss() {
        let mut sim = NetworkSimulator::new();

        let result = sim.set_packet_loss(1.5);
        assert!(result.is_err());
    }

    #[test]
    fn test_start_experiment() {
        let mut orchestrator = ChaosOrchestrator::new();

        orchestrator
            .start_experiment("exp1".to_string(), ChaosExperimentType::NetworkPartition)
            .unwrap();

        let exp = orchestrator.get_experiment("exp1").unwrap();
        assert_eq!(exp.experiment_type, ChaosExperimentType::NetworkPartition);
        assert!(exp.ended_at.is_none());
    }

    #[test]
    fn test_end_experiment() {
        let mut orchestrator = ChaosOrchestrator::new();

        orchestrator
            .start_experiment("exp2".to_string(), ChaosExperimentType::PacketLoss)
            .unwrap();

        orchestrator.end_experiment("exp2", true).unwrap();

        let exp = orchestrator.get_experiment("exp2").unwrap();
        assert!(exp.ended_at.is_some());
        assert!(exp.success);
    }

    #[test]
    fn test_record_metric() {
        let mut orchestrator = ChaosOrchestrator::new();

        orchestrator
            .start_experiment("exp3".to_string(), ChaosExperimentType::LatencyInjection)
            .unwrap();

        orchestrator
            .record_metric("exp3", "latency_p99".to_string(), 125.5)
            .unwrap();

        let exp = orchestrator.get_experiment("exp3").unwrap();
        assert_eq!(exp.metrics.get("latency_p99"), Some(&125.5));
    }

    #[test]
    fn test_record_error() {
        let mut orchestrator = ChaosOrchestrator::new();

        orchestrator
            .start_experiment("exp4".to_string(), ChaosExperimentType::NodeFailure)
            .unwrap();

        orchestrator
            .record_error("exp4", "Connection timeout".to_string())
            .unwrap();

        let exp = orchestrator.get_experiment("exp4").unwrap();
        assert_eq!(exp.errors.len(), 1);
    }

    #[test]
    fn test_inject_fault() {
        let mut orchestrator = ChaosOrchestrator::new();

        let fault = FaultInjection {
            target_node: "node1".to_string(),
            fault_type: ChaosExperimentType::ResourceExhaustion,
            severity: 0.8,
            duration: Duration::from_secs(60),
        };

        orchestrator.inject_fault(fault).unwrap();

        assert_eq!(orchestrator.get_active_faults().len(), 1);
    }

    #[test]
    fn test_clear_faults() {
        let mut orchestrator = ChaosOrchestrator::new();

        let fault = FaultInjection {
            target_node: "node2".to_string(),
            fault_type: ChaosExperimentType::ClockSkew,
            severity: 0.5,
            duration: Duration::from_secs(30),
        };

        orchestrator.inject_fault(fault).unwrap();
        orchestrator.clear_faults();

        assert_eq!(orchestrator.get_active_faults().len(), 0);
    }

    #[test]
    fn test_success_rate() {
        let mut orchestrator = ChaosOrchestrator::new();

        orchestrator
            .start_experiment("exp5".to_string(), ChaosExperimentType::NetworkPartition)
            .unwrap();
        orchestrator.end_experiment("exp5", true).unwrap();

        orchestrator
            .start_experiment("exp6".to_string(), ChaosExperimentType::PacketLoss)
            .unwrap();
        orchestrator.end_experiment("exp6", false).unwrap();

        let rate = orchestrator.calculate_success_rate();
        assert_eq!(rate, 0.5);
    }

    #[test]
    fn test_recovery_scenario() {
        let mut tester = RecoveryTester::new();

        let scenario = RecoveryScenario {
            name: "partition_recovery".to_string(),
            failure_type: ChaosExperimentType::NetworkPartition,
            expected_recovery_time_ms: 5000,
            actual_recovery_time_ms: None,
            recovered: false,
        };

        tester.add_scenario(scenario);

        tester.mark_recovered("partition_recovery", 4500).unwrap();

        let scenarios = tester.get_scenarios();
        assert!(scenarios[0].recovered);
        assert_eq!(scenarios[0].actual_recovery_time_ms, Some(4500));
    }

    #[test]
    fn test_recovery_success_rate() {
        let mut tester = RecoveryTester::new();

        let scenario1 = RecoveryScenario {
            name: "test1".to_string(),
            failure_type: ChaosExperimentType::NodeFailure,
            expected_recovery_time_ms: 3000,
            actual_recovery_time_ms: None,
            recovered: false,
        };

        let scenario2 = RecoveryScenario {
            name: "test2".to_string(),
            failure_type: ChaosExperimentType::PacketLoss,
            expected_recovery_time_ms: 2000,
            actual_recovery_time_ms: Some(1800),
            recovered: true,
        };

        tester.add_scenario(scenario1);
        tester.add_scenario(scenario2);

        let rate = tester.recovery_success_rate();
        assert_eq!(rate, 0.5);
    }
}
