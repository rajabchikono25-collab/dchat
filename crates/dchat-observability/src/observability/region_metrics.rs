// Region Diversity Monitoring and Alerting
//
// Monitors validator distribution across geographic regions in real-time,
// detecting concentration violations and triggering alerts when diversity
// requirements are at risk.

use dchat_core::config::constants::{MAX_REGION_PERCENTAGE, MIN_REGIONS, REGION_WARNING_THRESHOLD};
use dchat_identity::peer_registry::PeerRegistry;
use dchat_validator::thresholds::{DiversityWarningLevel, RegionDiversityMetrics};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Region diversity monitor with real-time alerting
pub struct RegionMonitor {
    peer_registry: Arc<PeerRegistry>,
    metrics: Arc<RwLock<RegionDiversityMetrics>>,
    alert_history: Arc<RwLock<Vec<DiversityAlert>>>,
    last_check: Arc<RwLock<Instant>>,
    check_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct DiversityAlert {
    pub timestamp: Instant,
    pub level: AlertLevel,
    pub message: String,
    pub region: Option<String>,
    pub percentage: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    Info,
    Warning,
    Critical,
}

impl RegionMonitor {
    /// Create a new region diversity monitor
    pub fn new(peer_registry: Arc<PeerRegistry>, check_interval: Duration) -> Self {
        Self {
            peer_registry,
            metrics: Arc::new(RwLock::new(RegionDiversityMetrics::new())),
            alert_history: Arc::new(RwLock::new(Vec::new())),
            last_check: Arc::new(RwLock::new(Instant::now())),
            check_interval,
        }
    }

    /// Start the monitoring task
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.check_interval);

            loop {
                interval.tick().await;

                if let Err(e) = self.check_diversity().await {
                    error!("Region diversity check failed: {}", e);
                }
            }
        })
    }

    /// Perform a diversity check
    pub async fn check_diversity(&self) -> Result<(), MonitorError> {
        // Update metrics from peer registry
        let validators = self
            .peer_registry
            .get_validators()
            .map_err(|e| MonitorError::RegistryError(e.to_string()))?;

        let mut metrics = self.metrics.write().await;
        *metrics = RegionDiversityMetrics::new();

        for validator in &validators {
            if let Some(ref region) = validator.region {
                metrics.add_validator(region.clone());
            }
        }

        // Check warning level
        let warning_level =
            metrics.get_warning_level(MIN_REGIONS, MAX_REGION_PERCENTAGE, REGION_WARNING_THRESHOLD);

        // Generate alerts based on warning level
        match warning_level {
            DiversityWarningLevel::Critical => {
                if let Some((region, percentage)) = metrics.max_region_concentration() {
                    self.emit_alert(
                        AlertLevel::Critical,
                        format!(
                            "CRITICAL: Region {} has {:.1}% of validators (max allowed: {:.1}%)",
                            region,
                            percentage * 100.0,
                            MAX_REGION_PERCENTAGE * 100.0
                        ),
                        Some(region.clone()),
                        Some(percentage),
                    )
                    .await;
                }

                if metrics.regions_represented < MIN_REGIONS {
                    self.emit_alert(
                        AlertLevel::Critical,
                        format!(
                            "CRITICAL: Only {} regions represented (minimum: {})",
                            metrics.regions_represented, MIN_REGIONS
                        ),
                        None,
                        None,
                    )
                    .await;
                }
            }

            DiversityWarningLevel::Warning => {
                if let Some((region, percentage)) = metrics.max_region_concentration() {
                    if percentage >= REGION_WARNING_THRESHOLD {
                        self.emit_alert(
                            AlertLevel::Warning,
                            format!(
                                "WARNING: Region {} has {:.1}% of validators (approaching {:.1}% limit)",
                                region,
                                percentage * 100.0,
                                MAX_REGION_PERCENTAGE * 100.0
                            ),
                            Some(region.clone()),
                            Some(percentage),
                        ).await;
                    }
                }
            }

            DiversityWarningLevel::Healthy => {
                // Log info-level status
                info!(
                    "Region diversity healthy: {} validators across {} regions",
                    metrics.total_validators, metrics.regions_represented
                );
            }
        }

        *self.last_check.write().await = Instant::now();

        Ok(())
    }

    /// Emit an alert
    async fn emit_alert(
        &self,
        level: AlertLevel,
        message: String,
        region: Option<String>,
        percentage: Option<f64>,
    ) {
        let alert = DiversityAlert {
            timestamp: Instant::now(),
            level,
            message: message.clone(),
            region,
            percentage,
        };

        // Log the alert
        match level {
            AlertLevel::Critical => error!("🚨 {}", message),
            AlertLevel::Warning => warn!("⚠️  {}", message),
            AlertLevel::Info => info!("ℹ️  {}", message),
        }

        // Store in history
        let mut history = self.alert_history.write().await;
        history.push(alert);

        // Keep only last 1000 alerts
        if history.len() > 1000 {
            let excess = history.len() - 1000;
            history.drain(0..excess);
        }
    }

    /// Get current metrics
    pub async fn get_metrics(&self) -> RegionDiversityMetrics {
        self.metrics.read().await.clone()
    }

    /// Get recent alerts
    pub async fn get_recent_alerts(&self, limit: usize) -> Vec<DiversityAlert> {
        let history = self.alert_history.read().await;
        let start = history.len().saturating_sub(limit);
        history[start..].to_vec()
    }

    /// Get region distribution report
    pub async fn get_distribution_report(&self) -> RegionDistributionReport {
        let metrics = self.metrics.read().await;
        let validators = self.peer_registry.get_validators().unwrap_or_default();

        let mut region_details = HashMap::new();

        for (region, count) in &metrics.region_distribution {
            let percentage = metrics.region_percentage(region);
            let validator_ids: Vec<String> = validators
                .iter()
                .filter(|v| v.region.as_ref() == Some(region))
                .map(|v| format!("{:?}", v.peer_id))
                .collect();

            region_details.insert(
                region.clone(),
                RegionDetail {
                    validator_count: *count,
                    percentage,
                    is_warning: percentage >= REGION_WARNING_THRESHOLD,
                    is_critical: percentage > MAX_REGION_PERCENTAGE,
                    validator_ids,
                },
            );
        }

        RegionDistributionReport {
            total_validators: metrics.total_validators,
            regions_represented: metrics.regions_represented,
            region_details,
            meets_requirements: metrics
                .check_diversity(MIN_REGIONS, MAX_REGION_PERCENTAGE)
                .is_ok(),
            warning_level: metrics.get_warning_level(
                MIN_REGIONS,
                MAX_REGION_PERCENTAGE,
                REGION_WARNING_THRESHOLD,
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegionDistributionReport {
    pub total_validators: usize,
    pub regions_represented: usize,
    pub region_details: HashMap<String, RegionDetail>,
    pub meets_requirements: bool,
    pub warning_level: DiversityWarningLevel,
}

#[derive(Debug, Clone)]
pub struct RegionDetail {
    pub validator_count: usize,
    pub percentage: f64,
    pub is_warning: bool,
    pub is_critical: bool,
    pub validator_ids: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum MonitorError {
    #[error("Registry error: {0}")]
    RegistryError(String),
}

#[cfg(test)]
#[cfg(feature = "nat-telemetry-integration")]
mod tests {
    use super::*;
    use dchat_identity::peer_registry::{AuthenticatedPeer, DiscoveryMethod};
    use dchat_network::PeerId;
    use std::time::SystemTime;

    #[tokio::test]
    async fn test_region_monitoring() {
        let registry = Arc::new(PeerRegistry::new());

        // Add exactly 2 validators per region across 4 regions for healthy balance (2/8 = 25%)
        // 4 regions > MIN_REGIONS(3) and 25% < WARNING(35%)
        for i in 0..8 {
            let peer_id = PeerId::random();
            let public_key =
                ed25519_dalek::SigningKey::generate(&mut rand::thread_rng()).verifying_key();

            let region = if i < 2 {
                "us-east"
            } else if i < 4 {
                "eu-west"
            } else if i < 6 {
                "asia-se"
            } else {
                "af-south"
            };

            let peer = AuthenticatedPeer {
                peer_id,
                public_key,
                region: Some(region.to_string()),
                discovered_via: DiscoveryMethod::Dns(format!("validator{}.dchat.network", i)),
                first_seen: SystemTime::now(),
                last_active: Instant::now(),
                stake_amount: Some(10_000_000),
                role: PeerRole::Validator,
                reputation: 1.0,
            };

            registry.register_peer(peer).unwrap();
        }

        let monitor = Arc::new(RegionMonitor::new(registry, Duration::from_secs(5)));

        // Check diversity
        monitor.check_diversity().await.unwrap();

        let report = monitor.get_distribution_report().await;
        assert_eq!(report.regions_represented, 4);
        // With 2 validators per region across 4 regions (25% each), should be healthy
        assert!(report.meets_requirements);
        assert_eq!(report.warning_level, DiversityWarningLevel::Healthy);
    }
}
