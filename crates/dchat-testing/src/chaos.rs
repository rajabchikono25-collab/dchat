use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Type of chaos fault to inject
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultType {
    /// Add latency to operations
    Latency { duration_ms: u64 },
    /// Drop packets/requests
    PacketLoss { percentage: u8 },
    /// Spike CPU usage
    CpuSpike { percentage: u8, duration_secs: u64 },
    /// Apply memory pressure
    MemoryPressure { mb: u32, duration_secs: u64 },
    /// Slow disk I/O
    DiskSlow { delay_ms: u64 },
    /// Network partition
    NetworkPartition { targets: Vec<String> },
    /// Crash service
    ServiceCrash { restart_delay_secs: u64 },
    /// Cascading failure
    CascadingFailure { failure_chain: Vec<String> },
}

/// Blast radius scope
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlastRadius {
    /// Single pod/instance
    Pod { id: String },
    /// Availability zone
    AvailabilityZone { name: String },
    /// Entire region
    Region { name: String },
    /// Specific service
    Service { name: String },
}

/// Chaos scenario definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosScenario {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub faults: Vec<FaultType>,
    pub blast_radius: BlastRadius,
    pub duration_secs: u64,
    pub recovery_time_secs: u64,
}

impl ChaosScenario {
    /// Create a new chaos scenario
    pub fn new(
        name: String,
        description: String,
        faults: Vec<FaultType>,
        blast_radius: BlastRadius,
        duration_secs: u64,
        recovery_time_secs: u64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            description,
            faults,
            blast_radius,
            duration_secs,
            recovery_time_secs,
        }
    }
}

/// Execution state of a chaos test
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChaosState {
    Pending,
    Running,
    Recovering,
    Completed,
    Failed,
}

/// Result of a chaos test execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosResult {
    pub scenario_id: Uuid,
    pub state: ChaosState,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub recovery_verified: bool,
    pub metrics: HashMap<String, f64>,
    pub errors: Vec<String>,
}

/// Chaos schedule (cron-like)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosSchedule {
    pub id: Uuid,
    pub scenario_id: Uuid,
    pub cron_expression: String, // e.g., "0 2 * * *" for 2am daily
    pub enabled: bool,
}

impl ChaosSchedule {
    /// Create a new chaos schedule
    pub fn new(scenario_id: Uuid, cron_expression: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            scenario_id,
            cron_expression,
            enabled: true,
        }
    }
}

/// Pre-built chaos scenario library
pub struct ScenarioLibrary;

impl ScenarioLibrary {
    /// High latency scenario
    pub fn high_latency() -> ChaosScenario {
        ChaosScenario::new(
            "High Latency".to_string(),
            "Introduce 500ms latency to test timeout handling".to_string(),
            vec![FaultType::Latency { duration_ms: 500 }],
            BlastRadius::Service {
                name: "relay-network".to_string(),
            },
            300, // 5 minutes
            60,  // 1 minute recovery
        )
    }

    /// Packet loss scenario
    pub fn packet_loss() -> ChaosScenario {
        ChaosScenario::new(
            "Packet Loss".to_string(),
            "Drop 20% of packets to test resilience".to_string(),
            vec![FaultType::PacketLoss { percentage: 20 }],
            BlastRadius::Service {
                name: "messaging".to_string(),
            },
            180, // 3 minutes
            30,  // 30 seconds recovery
        )
    }

    /// CPU spike scenario
    pub fn cpu_spike() -> ChaosScenario {
        ChaosScenario::new(
            "CPU Spike".to_string(),
            "Spike CPU to 90% for 2 minutes".to_string(),
            vec![FaultType::CpuSpike {
                percentage: 90,
                duration_secs: 120,
            }],
            BlastRadius::Pod {
                id: "relay-1".to_string(),
            },
            120,
            60,
        )
    }

    /// Memory pressure scenario
    pub fn memory_pressure() -> ChaosScenario {
        ChaosScenario::new(
            "Memory Pressure".to_string(),
            "Apply 500MB memory pressure".to_string(),
            vec![FaultType::MemoryPressure {
                mb: 500,
                duration_secs: 180,
            }],
            BlastRadius::Service {
                name: "database".to_string(),
            },
            180,
            90,
        )
    }

    /// Network partition scenario
    pub fn network_partition() -> ChaosScenario {
        ChaosScenario::new(
            "Network Partition".to_string(),
            "Partition network between AZs".to_string(),
            vec![FaultType::NetworkPartition {
                targets: vec!["az-1".to_string(), "az-2".to_string()],
            }],
            BlastRadius::AvailabilityZone {
                name: "az-1".to_string(),
            },
            300,
            120,
        )
    }

    /// Service crash scenario
    pub fn service_crash() -> ChaosScenario {
        ChaosScenario::new(
            "Service Crash".to_string(),
            "Crash service with 30s restart delay".to_string(),
            vec![FaultType::ServiceCrash {
                restart_delay_secs: 30,
            }],
            BlastRadius::Pod {
                id: "api-server-3".to_string(),
            },
            60,
            120,
        )
    }

    /// Cascading failure scenario
    pub fn cascading_failure() -> ChaosScenario {
        ChaosScenario::new(
            "Cascading Failure".to_string(),
            "Trigger cascading failure across services".to_string(),
            vec![FaultType::CascadingFailure {
                failure_chain: vec![
                    "database".to_string(),
                    "cache".to_string(),
                    "api".to_string(),
                ],
            }],
            BlastRadius::Region {
                name: "us-east-1".to_string(),
            },
            600,
            300,
        )
    }

    /// Disk slow scenario
    pub fn disk_slow() -> ChaosScenario {
        ChaosScenario::new(
            "Disk Slow".to_string(),
            "Add 200ms delay to disk operations".to_string(),
            vec![FaultType::DiskSlow { delay_ms: 200 }],
            BlastRadius::Service {
                name: "storage".to_string(),
            },
            240,
            60,
        )
    }

    /// Multi-fault scenario
    pub fn combined_stress() -> ChaosScenario {
        ChaosScenario::new(
            "Combined Stress".to_string(),
            "Multiple simultaneous faults".to_string(),
            vec![
                FaultType::Latency { duration_ms: 300 },
                FaultType::PacketLoss { percentage: 10 },
                FaultType::CpuSpike {
                    percentage: 70,
                    duration_secs: 180,
                },
            ],
            BlastRadius::Service {
                name: "relay-network".to_string(),
            },
            180,
            120,
        )
    }

    /// Zone failure scenario
    pub fn zone_failure() -> ChaosScenario {
        ChaosScenario::new(
            "Zone Failure".to_string(),
            "Simulate entire availability zone failure".to_string(),
            vec![
                FaultType::NetworkPartition {
                    targets: vec!["az-1".to_string()],
                },
                FaultType::ServiceCrash {
                    restart_delay_secs: 0,
                },
            ],
            BlastRadius::AvailabilityZone {
                name: "az-1".to_string(),
            },
            600,
            300,
        )
    }

    /// Get all pre-built scenarios
    pub fn all_scenarios() -> Vec<ChaosScenario> {
        vec![
            Self::high_latency(),
            Self::packet_loss(),
            Self::cpu_spike(),
            Self::memory_pressure(),
            Self::network_partition(),
            Self::service_crash(),
            Self::cascading_failure(),
            Self::disk_slow(),
            Self::combined_stress(),
            Self::zone_failure(),
        ]
    }
}

/// Chaos testing engine
pub struct ChaosEngine {
    scenarios: Arc<RwLock<HashMap<Uuid, ChaosScenario>>>,
    results: Arc<RwLock<HashMap<Uuid, ChaosResult>>>,
    schedules: Arc<RwLock<HashMap<Uuid, ChaosSchedule>>>,
    active_tests: Arc<RwLock<Vec<Uuid>>>,
}

impl ChaosEngine {
    /// Create a new chaos engine
    pub fn new() -> Self {
        Self {
            scenarios: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
            schedules: Arc::new(RwLock::new(HashMap::new())),
            active_tests: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Load pre-built scenario library
    pub fn load_scenario_library(&self) {
        let scenarios = ScenarioLibrary::all_scenarios();
        let mut store = self.scenarios.write().unwrap();
        for scenario in scenarios {
            store.insert(scenario.id, scenario);
        }
    }

    /// Register a custom scenario
    pub fn register_scenario(&self, scenario: ChaosScenario) -> Uuid {
        let id = scenario.id;
        let mut scenarios = self.scenarios.write().unwrap();
        scenarios.insert(id, scenario);
        id
    }

    /// Get all scenarios
    pub fn get_scenarios(&self) -> Vec<ChaosScenario> {
        let scenarios = self.scenarios.read().unwrap();
        scenarios.values().cloned().collect()
    }

    /// Get scenario by ID
    pub fn get_scenario(&self, id: Uuid) -> Option<ChaosScenario> {
        let scenarios = self.scenarios.read().unwrap();
        scenarios.get(&id).cloned()
    }

    /// Execute a chaos scenario
    pub fn execute_scenario(&self, scenario_id: Uuid) -> Result<Uuid, String> {
        let scenarios = self.scenarios.read().unwrap();
        let _scenario = scenarios
            .get(&scenario_id)
            .ok_or_else(|| "Scenario not found".to_string())?;

        // Check if already running
        let active = self.active_tests.read().unwrap();
        if active.contains(&scenario_id) {
            return Err("Scenario already running".to_string());
        }
        drop(active);

        // Create result entry
        let result = ChaosResult {
            scenario_id,
            state: ChaosState::Running,
            started_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            recovery_verified: false,
            metrics: HashMap::new(),
            errors: Vec::new(),
        };

        let result_id = Uuid::new_v4();
        let mut results = self.results.write().unwrap();
        results.insert(result_id, result);

        // Add to active tests
        let mut active = self.active_tests.write().unwrap();
        active.push(scenario_id);

        Ok(result_id)
    }

    /// Complete a chaos test
    pub fn complete_test(&self, result_id: Uuid, recovery_verified: bool) {
        let mut results = self.results.write().unwrap();
        if let Some(result) = results.get_mut(&result_id) {
            result.state = ChaosState::Completed;
            result.completed_at = Some(chrono::Utc::now().to_rfc3339());
            result.recovery_verified = recovery_verified;

            // Remove from active tests
            let mut active = self.active_tests.write().unwrap();
            active.retain(|id| id != &result.scenario_id);
        }
    }

    /// Fail a chaos test
    pub fn fail_test(&self, result_id: Uuid, error: String) {
        let mut results = self.results.write().unwrap();
        if let Some(result) = results.get_mut(&result_id) {
            result.state = ChaosState::Failed;
            result.completed_at = Some(chrono::Utc::now().to_rfc3339());
            result.errors.push(error);

            // Remove from active tests
            let mut active = self.active_tests.write().unwrap();
            active.retain(|id| id != &result.scenario_id);
        }
    }

    /// Add metric to test result
    pub fn add_metric(&self, result_id: Uuid, name: String, value: f64) {
        let mut results = self.results.write().unwrap();
        if let Some(result) = results.get_mut(&result_id) {
            result.metrics.insert(name, value);
        }
    }

    /// Get test result
    pub fn get_result(&self, result_id: Uuid) -> Option<ChaosResult> {
        let results = self.results.read().unwrap();
        results.get(&result_id).cloned()
    }

    /// Get all results
    pub fn get_all_results(&self) -> Vec<ChaosResult> {
        let results = self.results.read().unwrap();
        results.values().cloned().collect()
    }

    /// Get active tests
    pub fn get_active_tests(&self) -> Vec<Uuid> {
        let active = self.active_tests.read().unwrap();
        active.clone()
    }

    /// Schedule a chaos test
    pub fn schedule_test(&self, schedule: ChaosSchedule) -> Uuid {
        let id = schedule.id;
        let mut schedules = self.schedules.write().unwrap();
        schedules.insert(id, schedule);
        id
    }

    /// Get all schedules
    pub fn get_schedules(&self) -> Vec<ChaosSchedule> {
        let schedules = self.schedules.read().unwrap();
        schedules.values().cloned().collect()
    }

    /// Enable/disable a schedule
    pub fn set_schedule_enabled(&self, schedule_id: Uuid, enabled: bool) {
        let mut schedules = self.schedules.write().unwrap();
        if let Some(schedule) = schedules.get_mut(&schedule_id) {
            schedule.enabled = enabled;
        }
    }

    /// Verify recovery after chaos test
    pub fn verify_recovery(&self, result_id: Uuid, checks: Vec<(&str, bool)>) -> bool {
        let all_passed = checks.iter().all(|(_, passed)| *passed);

        let mut results = self.results.write().unwrap();
        if let Some(result) = results.get_mut(&result_id) {
            result.recovery_verified = all_passed;

            // Add failed checks as errors
            for (check_name, passed) in checks {
                if !passed {
                    result
                        .errors
                        .push(format!("Recovery check failed: {}", check_name));
                }
            }
        }

        all_passed
    }
}

impl Default for ChaosEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_creation() {
        let scenario = ChaosScenario::new(
            "Test".to_string(),
            "Test scenario".to_string(),
            vec![FaultType::Latency { duration_ms: 100 }],
            BlastRadius::Pod {
                id: "pod-1".to_string(),
            },
            60,
            30,
        );

        assert_eq!(scenario.name, "Test");
        assert_eq!(scenario.faults.len(), 1);
        assert_eq!(scenario.duration_secs, 60);
    }

    #[test]
    fn test_scenario_library() {
        let scenarios = ScenarioLibrary::all_scenarios();
        assert_eq!(scenarios.len(), 10);

        let latency = ScenarioLibrary::high_latency();
        assert_eq!(latency.name, "High Latency");
    }

    #[test]
    fn test_chaos_engine_register() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::cpu_spike();

        let id = engine.register_scenario(scenario);
        assert_ne!(id, Uuid::nil());

        let retrieved = engine.get_scenario(id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "CPU Spike");
    }

    #[test]
    fn test_load_scenario_library() {
        let engine = ChaosEngine::new();
        engine.load_scenario_library();

        let scenarios = engine.get_scenarios();
        assert_eq!(scenarios.len(), 10);
    }

    #[test]
    fn test_execute_scenario() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::packet_loss();
        let scenario_id = engine.register_scenario(scenario);

        let result_id = engine.execute_scenario(scenario_id).unwrap();
        assert_ne!(result_id, Uuid::nil());

        let active = engine.get_active_tests();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0], scenario_id);
    }

    #[test]
    fn test_execute_already_running() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::memory_pressure();
        let scenario_id = engine.register_scenario(scenario);

        engine.execute_scenario(scenario_id).unwrap();
        let result = engine.execute_scenario(scenario_id);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Scenario already running");
    }

    #[test]
    fn test_complete_test() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::disk_slow();
        let scenario_id = engine.register_scenario(scenario);

        let result_id = engine.execute_scenario(scenario_id).unwrap();
        engine.complete_test(result_id, true);

        let result = engine.get_result(result_id).unwrap();
        assert_eq!(result.state, ChaosState::Completed);
        assert!(result.recovery_verified);
        assert!(result.completed_at.is_some());

        let active = engine.get_active_tests();
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn test_fail_test() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::service_crash();
        let scenario_id = engine.register_scenario(scenario);

        let result_id = engine.execute_scenario(scenario_id).unwrap();
        engine.fail_test(result_id, "Service did not recover".to_string());

        let result = engine.get_result(result_id).unwrap();
        assert_eq!(result.state, ChaosState::Failed);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_add_metric() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::network_partition();
        let scenario_id = engine.register_scenario(scenario);

        let result_id = engine.execute_scenario(scenario_id).unwrap();
        engine.add_metric(result_id, "latency_p95".to_string(), 145.3);
        engine.add_metric(result_id, "error_rate".to_string(), 2.5);

        let result = engine.get_result(result_id).unwrap();
        assert_eq!(result.metrics.len(), 2);
        assert_eq!(*result.metrics.get("latency_p95").unwrap(), 145.3);
    }

    #[test]
    fn test_schedule_test() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::cascading_failure();
        let scenario_id = engine.register_scenario(scenario);

        let schedule = ChaosSchedule::new(scenario_id, "0 2 * * *".to_string());
        let _schedule_id = engine.schedule_test(schedule);

        let schedules = engine.get_schedules();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].cron_expression, "0 2 * * *");
    }

    #[test]
    fn test_disable_schedule() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::combined_stress();
        let scenario_id = engine.register_scenario(scenario);

        let schedule = ChaosSchedule::new(scenario_id, "0 3 * * *".to_string());
        let schedule_id = engine.schedule_test(schedule);

        engine.set_schedule_enabled(schedule_id, false);

        let schedules = engine.get_schedules();
        assert!(!schedules[0].enabled);
    }

    #[test]
    fn test_verify_recovery() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::zone_failure();
        let scenario_id = engine.register_scenario(scenario);

        let result_id = engine.execute_scenario(scenario_id).unwrap();

        let checks = vec![
            ("service_healthy", true),
            ("data_consistent", true),
            ("no_errors", true),
        ];

        let verified = engine.verify_recovery(result_id, checks);
        assert!(verified);

        let result = engine.get_result(result_id).unwrap();
        assert!(result.recovery_verified);
    }

    #[test]
    fn test_verify_recovery_failure() {
        let engine = ChaosEngine::new();
        let scenario = ScenarioLibrary::high_latency();
        let scenario_id = engine.register_scenario(scenario);

        let result_id = engine.execute_scenario(scenario_id).unwrap();

        let checks = vec![("service_healthy", true), ("data_consistent", false)];

        let verified = engine.verify_recovery(result_id, checks);
        assert!(!verified);

        let result = engine.get_result(result_id).unwrap();
        assert!(!result.recovery_verified);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_get_all_results() {
        let engine = ChaosEngine::new();

        let scenario1 = ScenarioLibrary::cpu_spike();
        let id1 = engine.register_scenario(scenario1);
        engine.execute_scenario(id1).unwrap();

        let scenario2 = ScenarioLibrary::packet_loss();
        let id2 = engine.register_scenario(scenario2);
        engine.execute_scenario(id2).unwrap();

        let results = engine.get_all_results();
        assert_eq!(results.len(), 2);
    }
}
