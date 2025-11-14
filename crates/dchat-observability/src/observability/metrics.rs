// src/observability/metrics.rs - Comprehensive Prometheus metrics for production observability
//
// Provides telemetry for all critical paths:
// - Consensus: proposal rate, finality time, validator participation
// - Network: peer count, latency, message rate, churn
// - Relay: reputation scores, delivery proofs, reward distribution
// - Discovery: DNS/DHT/cache performance
// - Version: negotiation success rate, downgrade attempts
// - NAT: traversal method usage, success rates

use prometheus::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, Opts, Registry,
};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("Prometheus registration failed: {0}")]
    RegistrationFailed(String),
    #[error("Metrics export failed: {0}")]
    ExportFailed(String),
    #[error("Invalid label: {0}")]
    InvalidLabel(String),
}

/// Prometheus exporter with pre-registered metrics for all dchat systems
pub struct PrometheusExporter {
    registry: Registry,

    // Consensus metrics
    consensus_proposals_total: Counter,
    consensus_finality_seconds: Histogram,
    consensus_validator_participation: Gauge,
    consensus_bft_threshold_violations: Counter,

    // Network metrics
    network_peers_total: Gauge,
    network_peer_churn_total: CounterVec,
    network_latency_seconds: HistogramVec,
    network_messages_total: CounterVec,
    network_bytes_total: CounterVec,

    // Relay metrics
    relay_reputation_score: GaugeVec,
    relay_delivery_proofs_total: Counter,
    relay_proof_batch_size: Histogram,
    relay_rewards_issued_total: Counter,
    relay_tier_distribution: GaugeVec,

    // Discovery metrics
    discovery_queries_total: CounterVec,
    discovery_query_duration_seconds: HistogramVec,
    discovery_cache_operations_total: CounterVec,
    discovery_peer_count: GaugeVec,

    // Version negotiation metrics
    version_negotiations_total: CounterVec,
    version_negotiation_duration_seconds: Histogram,
    version_downgrade_attempts_total: Counter,
    version_major_mismatches_total: Counter,

    // NAT traversal metrics
    nat_traversal_attempts_total: CounterVec,
    nat_method_success_rate: GaugeVec,

    // Region diversity metrics
    region_validator_distribution: GaugeVec,
    region_diversity_percentage: Gauge,
    region_diversity_warnings_total: Counter,

    // Slashing metrics
    slashing_violations_detected_total: CounterVec,
    slashing_penalties_applied_total: CounterVec,
    slashing_stake_slashed_total: Counter,
}

impl PrometheusExporter {
    /// Create a new Prometheus exporter with all dchat metrics registered
    pub fn new() -> Result<Self, MetricsError> {
        let registry = Registry::new();

        // Consensus metrics
        let consensus_proposals_total = Counter::with_opts(Opts::new(
            "dchat_consensus_proposals_total",
            "Total number of consensus proposals",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(consensus_proposals_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let consensus_finality_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "dchat_consensus_finality_seconds",
                "Time to finality for consensus proposals",
            )
            .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]),
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(consensus_finality_seconds.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let consensus_validator_participation = Gauge::with_opts(Opts::new(
            "dchat_consensus_validator_participation",
            "Percentage of validators participating in consensus",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(consensus_validator_participation.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let consensus_bft_threshold_violations = Counter::with_opts(Opts::new(
            "dchat_consensus_bft_threshold_violations_total",
            "Total BFT threshold violations",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(consensus_bft_threshold_violations.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        // Network metrics
        let network_peers_total = Gauge::with_opts(Opts::new(
            "dchat_network_peers_total",
            "Current number of connected peers",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(network_peers_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let network_peer_churn_total = CounterVec::new(
            Opts::new("dchat_network_peer_churn_total", "Total peer churn events"),
            &["event"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(network_peer_churn_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let network_latency_seconds = HistogramVec::new(
            HistogramOpts::new("dchat_network_latency_seconds", "Network latency per peer")
                .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
            &["peer_id"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(network_latency_seconds.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let network_messages_total = CounterVec::new(
            Opts::new(
                "dchat_network_messages_total",
                "Total network messages by type",
            ),
            &["message_type", "direction"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(network_messages_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let network_bytes_total = CounterVec::new(
            Opts::new(
                "dchat_network_bytes_total",
                "Total network bytes by direction",
            ),
            &["direction"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(network_bytes_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        // Relay metrics
        let relay_reputation_score = GaugeVec::new(
            Opts::new("dchat_relay_reputation_score", "Relay reputation score"),
            &["relay_id"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(relay_reputation_score.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let relay_delivery_proofs_total = Counter::with_opts(Opts::new(
            "dchat_relay_delivery_proofs_total",
            "Total delivery proofs generated",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(relay_delivery_proofs_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let relay_proof_batch_size = Histogram::with_opts(
            HistogramOpts::new("dchat_relay_proof_batch_size", "Delivery proof batch size")
                .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0]),
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(relay_proof_batch_size.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let relay_rewards_issued_total = Counter::with_opts(Opts::new(
            "dchat_relay_rewards_issued_total",
            "Total relay rewards issued (smallest currency unit)",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(relay_rewards_issued_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let relay_tier_distribution = GaugeVec::new(
            Opts::new("dchat_relay_tier_distribution", "Number of relays per tier"),
            &["tier"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(relay_tier_distribution.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        // Discovery metrics
        let discovery_queries_total = CounterVec::new(
            Opts::new(
                "dchat_discovery_queries_total",
                "Total discovery queries by method and result",
            ),
            &["method", "result"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(discovery_queries_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let discovery_query_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "dchat_discovery_query_duration_seconds",
                "Discovery query duration by method",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 30.0]),
            &["method"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(discovery_query_duration_seconds.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let discovery_cache_operations_total = CounterVec::new(
            Opts::new(
                "dchat_discovery_cache_operations_total",
                "Total cache operations by type",
            ),
            &["operation", "result"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(discovery_cache_operations_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let discovery_peer_count = GaugeVec::new(
            Opts::new(
                "dchat_discovery_peer_count",
                "Number of discovered peers by method",
            ),
            &["method"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(discovery_peer_count.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        // Version negotiation metrics
        let version_negotiations_total = CounterVec::new(
            Opts::new(
                "dchat_version_negotiations_total",
                "Total version negotiations by result",
            ),
            &["result"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(version_negotiations_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let version_negotiation_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "dchat_version_negotiation_duration_seconds",
                "Version negotiation duration",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(version_negotiation_duration_seconds.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let version_downgrade_attempts_total = Counter::with_opts(Opts::new(
            "dchat_version_downgrade_attempts_total",
            "Total detected downgrade attempts",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(version_downgrade_attempts_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let version_major_mismatches_total = Counter::with_opts(Opts::new(
            "dchat_version_major_mismatches_total",
            "Total major version mismatches",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(version_major_mismatches_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        // NAT traversal metrics
        let nat_traversal_attempts_total = CounterVec::new(
            Opts::new(
                "dchat_nat_traversal_attempts_total",
                "Total NAT traversal attempts by method and result",
            ),
            &["method", "result"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(nat_traversal_attempts_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let nat_method_success_rate = GaugeVec::new(
            Opts::new(
                "dchat_nat_method_success_rate",
                "NAT traversal success rate by method (0-1)",
            ),
            &["method"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(nat_method_success_rate.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        // Region diversity metrics
        let region_validator_distribution = GaugeVec::new(
            Opts::new(
                "dchat_region_validator_distribution",
                "Number of validators per region",
            ),
            &["region"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(region_validator_distribution.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let region_diversity_percentage = Gauge::with_opts(Opts::new(
            "dchat_region_diversity_percentage",
            "Maximum region concentration percentage",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(region_diversity_percentage.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let region_diversity_warnings_total = Counter::with_opts(Opts::new(
            "dchat_region_diversity_warnings_total",
            "Total region diversity warnings",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(region_diversity_warnings_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        // Slashing metrics
        let slashing_violations_detected_total = CounterVec::new(
            Opts::new(
                "dchat_slashing_violations_detected_total",
                "Total slashing violations by type",
            ),
            &["violation_type"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(slashing_violations_detected_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let slashing_penalties_applied_total = CounterVec::new(
            Opts::new(
                "dchat_slashing_penalties_applied_total",
                "Total slashing penalties by severity",
            ),
            &["severity"],
        )
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(slashing_penalties_applied_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        let slashing_stake_slashed_total = Counter::with_opts(Opts::new(
            "dchat_slashing_stake_slashed_total",
            "Total stake slashed (smallest currency unit)",
        ))
        .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;
        registry
            .register(Box::new(slashing_stake_slashed_total.clone()))
            .map_err(|e| MetricsError::RegistrationFailed(e.to_string()))?;

        Ok(Self {
            registry,
            consensus_proposals_total,
            consensus_finality_seconds,
            consensus_validator_participation,
            consensus_bft_threshold_violations,
            network_peers_total,
            network_peer_churn_total,
            network_latency_seconds,
            network_messages_total,
            network_bytes_total,
            relay_reputation_score,
            relay_delivery_proofs_total,
            relay_proof_batch_size,
            relay_rewards_issued_total,
            relay_tier_distribution,
            discovery_queries_total,
            discovery_query_duration_seconds,
            discovery_cache_operations_total,
            discovery_peer_count,
            version_negotiations_total,
            version_negotiation_duration_seconds,
            version_downgrade_attempts_total,
            version_major_mismatches_total,
            nat_traversal_attempts_total,
            nat_method_success_rate,
            region_validator_distribution,
            region_diversity_percentage,
            region_diversity_warnings_total,
            slashing_violations_detected_total,
            slashing_penalties_applied_total,
            slashing_stake_slashed_total,
        })
    }

    /// Export all metrics in Prometheus text format
    pub fn export(&self) -> Result<String, MetricsError> {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .map_err(|e| MetricsError::ExportFailed(e.to_string()))?;
        String::from_utf8(buffer).map_err(|e| MetricsError::ExportFailed(e.to_string()))
    }

    // Consensus metric helpers
    pub fn record_consensus_proposal(&self) {
        self.consensus_proposals_total.inc();
    }

    pub fn record_consensus_finality(&self, duration_seconds: f64) {
        self.consensus_finality_seconds.observe(duration_seconds);
    }

    pub fn set_validator_participation(&self, percentage: f64) {
        self.consensus_validator_participation.set(percentage);
    }

    pub fn record_bft_threshold_violation(&self) {
        self.consensus_bft_threshold_violations.inc();
    }

    // Network metric helpers
    pub fn set_peer_count(&self, count: usize) {
        self.network_peers_total.set(count as f64);
    }

    pub fn record_peer_join(&self) {
        self.network_peer_churn_total
            .with_label_values(&["join"])
            .inc();
    }

    pub fn record_peer_leave(&self) {
        self.network_peer_churn_total
            .with_label_values(&["leave"])
            .inc();
    }

    pub fn record_peer_latency(&self, peer_id: &str, latency_seconds: f64) {
        self.network_latency_seconds
            .with_label_values(&[peer_id])
            .observe(latency_seconds);
    }

    pub fn record_message(&self, message_type: &str, direction: &str) {
        self.network_messages_total
            .with_label_values(&[message_type, direction])
            .inc();
    }

    pub fn record_bytes(&self, direction: &str, bytes: u64) {
        self.network_bytes_total
            .with_label_values(&[direction])
            .inc_by(bytes as f64);
    }

    // Relay metric helpers
    pub fn set_relay_reputation(&self, relay_id: &str, score: f64) {
        self.relay_reputation_score
            .with_label_values(&[relay_id])
            .set(score);
    }

    pub fn record_delivery_proof(&self) {
        self.relay_delivery_proofs_total.inc();
    }

    pub fn record_proof_batch_size(&self, size: usize) {
        self.relay_proof_batch_size.observe(size as f64);
    }

    pub fn record_relay_reward(&self, amount: u64) {
        self.relay_rewards_issued_total.inc_by(amount as f64);
    }

    pub fn set_relay_tier_count(&self, tier: &str, count: usize) {
        self.relay_tier_distribution
            .with_label_values(&[tier])
            .set(count as f64);
    }

    // Discovery metric helpers
    pub fn record_discovery_query(&self, method: &str, result: &str) {
        self.discovery_queries_total
            .with_label_values(&[method, result])
            .inc();
    }

    pub fn record_discovery_duration(&self, method: &str, duration_seconds: f64) {
        self.discovery_query_duration_seconds
            .with_label_values(&[method])
            .observe(duration_seconds);
    }

    pub fn record_cache_operation(&self, operation: &str, result: &str) {
        self.discovery_cache_operations_total
            .with_label_values(&[operation, result])
            .inc();
    }

    pub fn set_discovered_peer_count(&self, method: &str, count: usize) {
        self.discovery_peer_count
            .with_label_values(&[method])
            .set(count as f64);
    }

    // Version negotiation metric helpers
    pub fn record_version_negotiation(&self, result: &str) {
        self.version_negotiations_total
            .with_label_values(&[result])
            .inc();
    }

    pub fn record_version_negotiation_duration(&self, duration_seconds: f64) {
        self.version_negotiation_duration_seconds
            .observe(duration_seconds);
    }

    pub fn record_downgrade_attempt(&self) {
        self.version_downgrade_attempts_total.inc();
    }

    pub fn record_major_mismatch(&self) {
        self.version_major_mismatches_total.inc();
    }

    // NAT traversal metric helpers
    pub fn record_nat_attempt(&self, method: &str, result: &str) {
        self.nat_traversal_attempts_total
            .with_label_values(&[method, result])
            .inc();
    }

    pub fn set_nat_success_rate(&self, method: &str, rate: f64) {
        self.nat_method_success_rate
            .with_label_values(&[method])
            .set(rate);
    }

    // Region diversity metric helpers
    pub fn set_region_validator_count(&self, region: &str, count: usize) {
        self.region_validator_distribution
            .with_label_values(&[region])
            .set(count as f64);
    }

    pub fn set_region_diversity_percentage(&self, percentage: f64) {
        self.region_diversity_percentage.set(percentage);
    }

    pub fn record_diversity_warning(&self) {
        self.region_diversity_warnings_total.inc();
    }

    // Slashing metric helpers
    pub fn record_slashing_violation(&self, violation_type: &str) {
        self.slashing_violations_detected_total
            .with_label_values(&[violation_type])
            .inc();
    }

    pub fn record_slashing_penalty(&self, severity: &str) {
        self.slashing_penalties_applied_total
            .with_label_values(&[severity])
            .inc();
    }

    pub fn record_slashed_stake(&self, amount: u64) {
        self.slashing_stake_slashed_total.inc_by(amount as f64);
    }
}

// Implement NatMetricsExporter trait from dchat-network to enable seamless integration
#[cfg(feature = "nat-telemetry-integration")]
impl dchat_network::network::nat_telemetry::NatMetricsExporter for PrometheusExporter {
    fn record_nat_attempt(&self, method: &str, result: &str) {
        self.record_nat_attempt(method, result);
    }

    fn set_nat_success_rate(&self, method: &str, rate: f64) {
        self.set_nat_success_rate(method, rate);
    }
}

/// Global Prometheus exporter singleton
static PROMETHEUS_EXPORTER: once_cell::sync::OnceCell<Arc<PrometheusExporter>> =
    once_cell::sync::OnceCell::new();

/// Initialize the global Prometheus exporter
///
/// With `nat-telemetry-integration` feature enabled, this also registers
/// the exporter with dchat-network's NAT telemetry system for automatic metrics export.
pub fn initialize_prometheus() -> Result<(), MetricsError> {
    let exporter = PrometheusExporter::new()?;
    let arc_exporter = Arc::new(exporter);
    
    PROMETHEUS_EXPORTER
        .set(arc_exporter.clone())
        .map_err(|_| {
            MetricsError::RegistrationFailed("Prometheus already initialized".to_string())
        })?;

    // Register with NAT telemetry if feature is enabled
    #[cfg(feature = "nat-telemetry-integration")]
    dchat_network::network::nat_telemetry::set_metrics_exporter(arc_exporter.clone());

    Ok(())
}

/// Get the global Prometheus exporter
pub fn get_prometheus() -> Option<Arc<PrometheusExporter>> {
    PROMETHEUS_EXPORTER.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exporter_creation() {
        let exporter = PrometheusExporter::new();
        assert!(exporter.is_ok(), "Exporter creation should succeed");
    }

    #[test]
    fn test_consensus_metrics() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.record_consensus_proposal();
        exporter.record_consensus_finality(1.5);
        exporter.set_validator_participation(0.95);
        exporter.record_bft_threshold_violation();

        let output = exporter.export().unwrap();
        assert!(
            output.contains("dchat_consensus_proposals_total"),
            "Should export proposal count"
        );
        assert!(
            output.contains("dchat_consensus_finality_seconds"),
            "Should export finality histogram"
        );
        assert!(
            output.contains("dchat_consensus_validator_participation"),
            "Should export participation"
        );
    }

    #[test]
    fn test_network_metrics() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.set_peer_count(42);
        exporter.record_peer_join();
        exporter.record_peer_leave();
        exporter.record_peer_latency("peer123", 0.025);
        exporter.record_message("handshake", "inbound");
        exporter.record_bytes("outbound", 1024);

        let output = exporter.export().unwrap();
        assert!(
            output.contains("dchat_network_peers_total"),
            "Should export peer count"
        );
        assert!(
            output.contains("dchat_network_peer_churn_total"),
            "Should export churn"
        );
        assert!(
            output.contains("dchat_network_latency_seconds"),
            "Should export latency"
        );
    }

    #[test]
    fn test_relay_metrics() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.set_relay_reputation("relay456", 0.85);
        exporter.record_delivery_proof();
        exporter.record_proof_batch_size(15);
        exporter.record_relay_reward(1000000);
        exporter.set_relay_tier_count("gold", 5);

        let output = exporter.export().unwrap();
        assert!(
            output.contains("dchat_relay_reputation_score"),
            "Should export reputation"
        );
        assert!(
            output.contains("dchat_relay_delivery_proofs_total"),
            "Should export proof count"
        );
        assert!(
            output.contains("dchat_relay_proof_batch_size"),
            "Should export batch size"
        );
    }

    #[test]
    fn test_discovery_metrics() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.record_discovery_query("dns", "success");
        exporter.record_discovery_duration("dht", 0.5);
        exporter.record_cache_operation("hit", "success");
        exporter.set_discovered_peer_count("cached", 10);

        let output = exporter.export().unwrap();
        assert!(
            output.contains("dchat_discovery_queries_total"),
            "Should export query count"
        );
        assert!(
            output.contains("dchat_discovery_query_duration_seconds"),
            "Should export duration"
        );
    }

    #[test]
    fn test_version_metrics() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.record_version_negotiation("compatible");
        exporter.record_version_negotiation_duration(0.005);
        exporter.record_downgrade_attempt();
        exporter.record_major_mismatch();

        let output = exporter.export().unwrap();
        assert!(
            output.contains("dchat_version_negotiations_total"),
            "Should export negotiation count"
        );
        assert!(
            output.contains("dchat_version_downgrade_attempts_total"),
            "Should export downgrade attempts"
        );
    }

    #[test]
    fn test_nat_metrics() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.record_nat_attempt("upnp", "success");
        exporter.set_nat_success_rate("hole_punch", 0.75);

        let output = exporter.export().unwrap();
        assert!(
            output.contains("dchat_nat_traversal_attempts_total"),
            "Should export NAT attempts"
        );
        assert!(
            output.contains("dchat_nat_method_success_rate"),
            "Should export success rate"
        );
    }

    #[test]
    fn test_region_metrics() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.set_region_validator_count("us-east", 10);
        exporter.set_region_diversity_percentage(35.0);
        exporter.record_diversity_warning();

        let output = exporter.export().unwrap();
        assert!(
            output.contains("dchat_region_validator_distribution"),
            "Should export region distribution"
        );
        assert!(
            output.contains("dchat_region_diversity_percentage"),
            "Should export diversity percentage"
        );
    }

    #[test]
    fn test_slashing_metrics() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.record_slashing_violation("double_sign");
        exporter.record_slashing_penalty("severe");
        exporter.record_slashed_stake(5000000);

        let output = exporter.export().unwrap();
        assert!(
            output.contains("dchat_slashing_violations_detected_total"),
            "Should export violations"
        );
        assert!(
            output.contains("dchat_slashing_penalties_applied_total"),
            "Should export penalties"
        );
        assert!(
            output.contains("dchat_slashing_stake_slashed_total"),
            "Should export slashed stake"
        );
    }

    #[test]
    fn test_export_format() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.record_consensus_proposal();
        let output = exporter.export().unwrap();

        // Verify Prometheus text format
        assert!(output.contains("# HELP"), "Should contain HELP metadata");
        assert!(output.contains("# TYPE"), "Should contain TYPE metadata");
        assert!(
            output.contains("dchat_"),
            "All metrics should have dchat_ prefix"
        );
    }

    #[test]
    fn test_histogram_buckets() {
        let exporter = PrometheusExporter::new().unwrap();

        exporter.record_consensus_finality(0.5);
        exporter.record_consensus_finality(1.5);
        exporter.record_consensus_finality(10.5);

        let output = exporter.export().unwrap();
        assert!(
            output.contains("_bucket"),
            "Histogram should export buckets"
        );
        assert!(output.contains("_sum"), "Histogram should export sum");
        assert!(output.contains("_count"), "Histogram should export count");
    }

    #[test]
    fn test_global_singleton() {
        // Note: This test can only run once per process due to OnceCell
        // In practice, initialize_prometheus() is called at application startup

        // Test get_prometheus() when not initialized
        let result = get_prometheus();
        // May be Some or None depending on test execution order
        if let Some(exporter) = result {
            exporter.record_consensus_proposal();
            assert!(exporter.export().is_ok());
        }
    }
}
