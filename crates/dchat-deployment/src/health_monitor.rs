// Health Monitoring and Automatic Failover System
// 30-second health checks, DNS failover, auto-scaling, alerting

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use thiserror::Error;

/// Health monitoring errors
#[derive(Debug, Error)]
pub enum HealthError {
    #[error("Health check failed: {0}")]
    CheckFailed(String),
    #[error("Failover error: {0}")]
    FailoverError(String),
    #[error("Alert delivery failed: {0}")]
    AlertFailed(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Component health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Component is healthy
    Healthy,
    /// Component is degraded but operational
    Degraded,
    /// Component is unhealthy
    Unhealthy,
    /// Component status unknown
    Unknown,
}

/// Infrastructure component type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComponentType {
    /// Validator node
    Validator,
    /// Relay node
    Relay,
    /// CockroachDB storage
    CockroachDB,
    /// Redis cache
    Redis,
    /// MinIO object storage
    MinIO,
    /// TiKV key-value store
    TiKV,
    /// Backup system
    Backup,
}

/// Health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Component identifier
    pub component_id: String,
    /// Component type
    pub component_type: ComponentType,
    /// Health status
    pub status: HealthStatus,
    /// Response time (milliseconds)
    pub response_time_ms: u64,
    /// Last check timestamp
    pub timestamp: u64,
    /// Error message if unhealthy
    pub error_message: Option<String>,
    /// Additional metrics
    pub metrics: HashMap<String, f64>,
}

impl HealthCheckResult {
    pub fn new(component_id: String, component_type: ComponentType) -> Self {
        Self {
            component_id,
            component_type,
            status: HealthStatus::Unknown,
            response_time_ms: 0,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            error_message: None,
            metrics: HashMap::new(),
        }
    }

    pub fn healthy(mut self, response_time_ms: u64) -> Self {
        self.status = HealthStatus::Healthy;
        self.response_time_ms = response_time_ms;
        self
    }

    pub fn degraded(mut self, response_time_ms: u64, reason: String) -> Self {
        self.status = HealthStatus::Degraded;
        self.response_time_ms = response_time_ms;
        self.error_message = Some(reason);
        self
    }

    pub fn unhealthy(mut self, error: String) -> Self {
        self.status = HealthStatus::Unhealthy;
        self.error_message = Some(error);
        self
    }

    pub fn with_metric(mut self, key: String, value: f64) -> Self {
        self.metrics.insert(key, value);
        self
    }
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Check interval (seconds)
    pub interval_seconds: u64,
    /// Timeout for each check (seconds)
    pub timeout_seconds: u64,
    /// Consecutive failures before marking unhealthy
    pub failure_threshold: u32,
    /// Consecutive successes before marking healthy
    pub success_threshold: u32,
    /// Enable automatic remediation
    pub auto_remediate: bool,
}

impl HealthCheckConfig {
    pub fn new_production() -> Self {
        Self {
            interval_seconds: 30,
            timeout_seconds: 10,
            failure_threshold: 3,
            success_threshold: 2,
            auto_remediate: true,
        }
    }

    pub fn new_aggressive() -> Self {
        Self {
            interval_seconds: 10,
            timeout_seconds: 5,
            failure_threshold: 2,
            success_threshold: 1,
            auto_remediate: true,
        }
    }
}

/// DNS failover configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DNSFailoverConfig {
    /// DNS provider (route53, cloudflare, gcp)
    pub provider: String,
    /// Hosted zone ID
    pub zone_id: String,
    /// Domain name
    pub domain: String,
    /// TTL for DNS records (seconds)
    pub ttl: u32,
    /// Health check endpoint path
    pub health_check_path: String,
    /// Failover policy
    pub policy: FailoverPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailoverPolicy {
    /// Use priority-based failover
    Priority,
    /// Use weighted round-robin
    Weighted,
    /// Use latency-based routing
    LatencyBased,
    /// Use geolocation-based routing
    Geolocation,
}

impl DNSFailoverConfig {
    pub fn new_production() -> Self {
        Self {
            provider: "route53".to_string(),
            zone_id: "Z1234567890ABC".to_string(),
            domain: "dchat.network".to_string(),
            ttl: 60,
            health_check_path: "/health".to_string(),
            policy: FailoverPolicy::LatencyBased,
        }
    }
}

/// Auto-scaling configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoScalingConfig {
    /// Enable auto-scaling
    pub enabled: bool,
    /// Minimum instances
    pub min_instances: u32,
    /// Maximum instances
    pub max_instances: u32,
    /// Target CPU utilization (%)
    pub target_cpu_percent: f64,
    /// Target memory utilization (%)
    pub target_memory_percent: f64,
    /// Scale-up cooldown (seconds)
    pub scale_up_cooldown: u64,
    /// Scale-down cooldown (seconds)
    pub scale_down_cooldown: u64,
    /// Scale-up step size
    pub scale_up_step: u32,
    /// Scale-down step size
    pub scale_down_step: u32,
}

impl AutoScalingConfig {
    pub fn new_production() -> Self {
        Self {
            enabled: true,
            min_instances: 3,
            max_instances: 20,
            target_cpu_percent: 70.0,
            target_memory_percent: 80.0,
            scale_up_cooldown: 300,
            scale_down_cooldown: 600,
            scale_up_step: 2,
            scale_down_step: 1,
        }
    }

    pub fn should_scale_up(
        &self,
        current_cpu: f64,
        current_memory: f64,
        current_instances: u32,
    ) -> bool {
        if !self.enabled || current_instances >= self.max_instances {
            return false;
        }
        current_cpu > self.target_cpu_percent || current_memory > self.target_memory_percent
    }

    pub fn should_scale_down(
        &self,
        current_cpu: f64,
        current_memory: f64,
        current_instances: u32,
    ) -> bool {
        if !self.enabled || current_instances <= self.min_instances {
            return false;
        }
        current_cpu < self.target_cpu_percent * 0.5
            && current_memory < self.target_memory_percent * 0.5
    }

    pub fn calculate_target_instances(&self, current_cpu: f64, current_instances: u32) -> u32 {
        if current_cpu > self.target_cpu_percent {
            (current_instances + self.scale_up_step).min(self.max_instances)
        } else if current_cpu < self.target_cpu_percent * 0.5 {
            current_instances
                .saturating_sub(self.scale_down_step)
                .max(self.min_instances)
        } else {
            current_instances
        }
    }
}

/// Alert channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertChannel {
    /// Channel name
    pub name: String,
    /// Channel type (slack, pagerduty, email, webhook)
    pub channel_type: String,
    /// Webhook URL or endpoint
    pub endpoint: String,
    /// Alert severity levels to send
    pub severity_levels: Vec<String>,
    /// Rate limiting (max alerts per hour)
    pub rate_limit: u32,
}

impl AlertChannel {
    pub fn new_slack(webhook_url: String) -> Self {
        Self {
            name: "slack".to_string(),
            channel_type: "slack".to_string(),
            endpoint: webhook_url,
            severity_levels: vec!["critical".to_string(), "warning".to_string()],
            rate_limit: 60,
        }
    }

    pub fn new_pagerduty(integration_key: String) -> Self {
        Self {
            name: "pagerduty".to_string(),
            channel_type: "pagerduty".to_string(),
            endpoint: format!(
                "https://events.pagerduty.com/v2/enqueue/{}",
                integration_key
            ),
            severity_levels: vec!["critical".to_string()],
            rate_limit: 30,
        }
    }

    /// Validate alert channel configuration
    pub fn is_valid(&self) -> bool {
        // Check endpoint is not empty
        if self.endpoint.is_empty() {
            return false;
        }

        // Check for placeholder values
        match self.channel_type.as_str() {
            "slack" => {
                // Slack webhook should be a valid URL
                self.endpoint.starts_with("https://hooks.slack.com/services/")
                    && !self.endpoint.contains("/XXX/")
                    && !self.endpoint.contains("/YYY/")
                    && !self.endpoint.contains("/ZZZ")
            }
            "pagerduty" => {
                // PagerDuty endpoint should contain a real integration key
                self.endpoint.contains("events.pagerduty.com")
                    && !self.endpoint.contains("pagerduty_integration_key")
            }
            "email" | "webhook" => {
                // Email/webhook should have non-empty endpoint
                !self.endpoint.contains("example.com") && !self.endpoint.contains("placeholder")
            }
            _ => {
                // Unknown type, but if endpoint is set, assume valid
                true
            }
        }
    }

    pub fn new_email(smtp_endpoint: String) -> Self {
        Self {
            name: "email".to_string(),
            channel_type: "email".to_string(),
            endpoint: smtp_endpoint,
            severity_levels: vec![
                "critical".to_string(),
                "warning".to_string(),
                "info".to_string(),
            ],
            rate_limit: 30,
        }
    }
}

/// BFT consensus monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BFTMonitorConfig {
    /// Total number of validators
    pub total_validators: u32,
    /// Minimum healthy validators (BFT threshold)
    pub min_healthy: u32,
    /// Alert if below this threshold
    pub alert_threshold: u32,
    /// Check validator consensus participation
    pub check_participation: bool,
}

impl BFTMonitorConfig {
    pub fn new_production() -> Self {
        Self {
            total_validators: 7,
            min_healthy: 5,     // 5-of-7 BFT threshold
            alert_threshold: 6, // Alert if only 6 healthy
            check_participation: true,
        }
    }

    pub fn is_consensus_healthy(&self, healthy_count: u32) -> bool {
        healthy_count >= self.min_healthy
    }

    pub fn should_alert(&self, healthy_count: u32) -> bool {
        healthy_count <= self.alert_threshold
    }

    pub fn calculate_consensus_percentage(&self, healthy_count: u32) -> f64 {
        (healthy_count as f64 / self.total_validators as f64) * 100.0
    }
}

/// Prometheus metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusConfig {
    /// Prometheus endpoint
    pub endpoint: String,
    /// Scrape interval (seconds)
    pub scrape_interval: u64,
    /// Retention period (days)
    pub retention_days: u32,
    /// Enable remote write
    pub remote_write_enabled: bool,
    /// Remote write endpoint
    pub remote_write_endpoint: Option<String>,
}

impl PrometheusConfig {
    pub fn new_production() -> Self {
        Self {
            endpoint: "http://prometheus:9090".to_string(),
            scrape_interval: 30,
            retention_days: 90,
            remote_write_enabled: false,
            remote_write_endpoint: None,
        }
    }
}

/// Grafana dashboard configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrafanaConfig {
    /// Grafana endpoint
    pub endpoint: String,
    /// API key
    pub api_key: String,
    /// Dashboard IDs
    pub dashboards: Vec<String>,
    /// Organization ID
    pub org_id: u32,
}

impl GrafanaConfig {
    pub fn new_production() -> Self {
        Self {
            endpoint: std::env::var("GRAFANA_ENDPOINT")
                .unwrap_or_else(|_| "https://grafana.dchat.internal".to_string()),
            api_key: std::env::var("GRAFANA_API_KEY").expect(
                "GRAFANA_API_KEY environment variable must be set for production deployment",
            ),
            dashboards: vec![
                "infrastructure-overview".to_string(),
                "validator-health".to_string(),
                "storage-metrics".to_string(),
                "backup-status".to_string(),
            ],
            org_id: std::env::var("GRAFANA_ORG_ID")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
        }
    }

    /// Create configuration for development/testing (API key not required)
    pub fn new_dev() -> Self {
        Self {
            endpoint: "http://localhost:3000".to_string(),
            api_key: std::env::var("GRAFANA_API_KEY").unwrap_or_default(),
            dashboards: vec!["infrastructure-overview".to_string()],
            org_id: 1,
        }
    }
}

/// Complete health monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMonitorConfig {
    /// Health check configuration
    pub health_check: HealthCheckConfig,
    /// DNS failover configuration
    pub dns_failover: DNSFailoverConfig,
    /// Auto-scaling configuration
    pub auto_scaling: AutoScalingConfig,
    /// Alert channels
    pub alert_channels: Vec<AlertChannel>,
    /// BFT monitoring
    pub bft_monitor: BFTMonitorConfig,
    /// Prometheus configuration
    pub prometheus: PrometheusConfig,
    /// Grafana configuration
    pub grafana: GrafanaConfig,
}

impl HealthMonitorConfig {
    pub fn new_production() -> Self {
        // Load alert channels from environment variables
        let mut alert_channels = Vec::new();

        // Slack webhook URL from DCHAT_SLACK_WEBHOOK_URL
        if let Ok(slack_url) = std::env::var("DCHAT_SLACK_WEBHOOK_URL") {
            if !slack_url.is_empty() && slack_url != "https://hooks.slack.com/services/XXX/YYY/ZZZ" {
                alert_channels.push(AlertChannel::new_slack(slack_url));
            } else {
                tracing::warn!(
                    "DCHAT_SLACK_WEBHOOK_URL is set but contains placeholder value. Slack alerts will not be sent."
                );
            }
        } else {
            tracing::warn!(
                "DCHAT_SLACK_WEBHOOK_URL not set. Slack alerts will not be configured. \
                Set this environment variable to enable Slack notifications."
            );
        }

        // PagerDuty integration key from DCHAT_PAGERDUTY_KEY
        if let Ok(pd_key) = std::env::var("DCHAT_PAGERDUTY_KEY") {
            if !pd_key.is_empty() && pd_key != "pagerduty_integration_key" {
                alert_channels.push(AlertChannel::new_pagerduty(pd_key));
            } else {
                tracing::warn!(
                    "DCHAT_PAGERDUTY_KEY is set but contains placeholder value. PagerDuty alerts will not be sent."
                );
            }
        } else {
            tracing::warn!(
                "DCHAT_PAGERDUTY_KEY not set. PagerDuty alerts will not be configured. \
                Set this environment variable to enable PagerDuty notifications."
            );
        }

        // Fallback to placeholder if no valid alert channels configured (for development)
        if alert_channels.is_empty() {
            tracing::warn!(
                "No valid alert channels configured. Using placeholder channels for development. \
                Configure DCHAT_SLACK_WEBHOOK_URL and/or DCHAT_PAGERDUTY_KEY for production."
            );
            alert_channels.push(AlertChannel::new_slack(
                "https://hooks.slack.com/services/XXX/YYY/ZZZ".to_string(),
            ));
            alert_channels.push(AlertChannel::new_pagerduty(
                "pagerduty_integration_key".to_string(),
            ));
        }

        Self {
            health_check: HealthCheckConfig::new_production(),
            dns_failover: DNSFailoverConfig::new_production(),
            auto_scaling: AutoScalingConfig::new_production(),
            alert_channels,
            bft_monitor: BFTMonitorConfig::new_production(),
            prometheus: PrometheusConfig::new_production(),
            grafana: GrafanaConfig::new_production(),
        }
    }

    /// Generate JSON configuration
    pub fn generate_json(&self) -> Result<String, HealthError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| HealthError::ConfigError(format!("JSON serialization failed: {}", e)))
    }

    /// Verify configuration
    pub fn verify(&self) -> Result<(), HealthError> {
        // Check health check interval
        if self.health_check.interval_seconds == 0 {
            return Err(HealthError::ConfigError(
                "Health check interval cannot be 0".to_string(),
            ));
        }

        // Check BFT threshold
        if self.bft_monitor.min_healthy > self.bft_monitor.total_validators {
            return Err(HealthError::ConfigError(
                "BFT min_healthy exceeds total validators".to_string(),
            ));
        }

        // Check auto-scaling bounds
        if self.auto_scaling.min_instances > self.auto_scaling.max_instances {
            return Err(HealthError::ConfigError(
                "Auto-scaling min > max".to_string(),
            ));
        }

        // Check alert channels
        if self.alert_channels.is_empty() {
            return Err(HealthError::ConfigError(
                "At least one alert channel required".to_string(),
            ));
        }

        // Validate alert channels and warn about placeholders
        let valid_channels: Vec<_> = self
            .alert_channels
            .iter()
            .filter(|channel| channel.is_valid())
            .collect();

        if valid_channels.is_empty() {
            tracing::warn!(
                "All {} configured alert channels contain placeholder values. \
                Alerts will not be delivered in production. \
                Set DCHAT_SLACK_WEBHOOK_URL and/or DCHAT_PAGERDUTY_KEY environment variables.",
                self.alert_channels.len()
            );
        } else if valid_channels.len() < self.alert_channels.len() {
            tracing::warn!(
                "Only {}/{} alert channels are valid. {} channel(s) contain placeholder values.",
                valid_channels.len(),
                self.alert_channels.len(),
                self.alert_channels.len() - valid_channels.len()
            );
        }

        Ok(())
    }
}

/// Component health tracker
#[derive(Debug, Clone)]
pub struct ComponentHealthTracker {
    /// Component ID
    pub component_id: String,
    /// Component type
    pub component_type: ComponentType,
    /// Recent health checks
    pub recent_checks: Vec<HealthCheckResult>,
    /// Consecutive failures
    pub consecutive_failures: u32,
    /// Consecutive successes
    pub consecutive_successes: u32,
    /// Current status
    pub current_status: HealthStatus,
    /// Last failover timestamp
    pub last_failover: Option<u64>,
}

impl ComponentHealthTracker {
    pub fn new(component_id: String, component_type: ComponentType) -> Self {
        Self {
            component_id,
            component_type,
            recent_checks: Vec::new(),
            consecutive_failures: 0,
            consecutive_successes: 0,
            current_status: HealthStatus::Unknown,
            last_failover: None,
        }
    }

    pub fn update(&mut self, result: HealthCheckResult, config: &HealthCheckConfig) -> bool {
        // Add to recent checks (keep last 100)
        self.recent_checks.push(result.clone());
        if self.recent_checks.len() > 100 {
            self.recent_checks.remove(0);
        }

        // Update consecutive counts
        let previous_status = self.current_status;
        match result.status {
            HealthStatus::Healthy => {
                self.consecutive_failures = 0;
                self.consecutive_successes += 1;
                if self.consecutive_successes >= config.success_threshold {
                    self.current_status = HealthStatus::Healthy;
                }
            }
            HealthStatus::Degraded => {
                self.consecutive_failures += 1;
                self.consecutive_successes = 0;
                if self.consecutive_failures >= config.failure_threshold {
                    self.current_status = HealthStatus::Degraded;
                }
            }
            HealthStatus::Unhealthy => {
                self.consecutive_failures += 1;
                self.consecutive_successes = 0;
                if self.consecutive_failures >= config.failure_threshold {
                    self.current_status = HealthStatus::Unhealthy;
                }
            }
            HealthStatus::Unknown => {}
        }

        // Return true if status changed
        previous_status != self.current_status
    }

    pub fn calculate_uptime_percentage(&self, window_seconds: u64) -> f64 {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let relevant_checks: Vec<_> = self
            .recent_checks
            .iter()
            .filter(|check| now - check.timestamp <= window_seconds)
            .collect();

        if relevant_checks.is_empty() {
            return 0.0;
        }

        let healthy_count = relevant_checks
            .iter()
            .filter(|check| check.status == HealthStatus::Healthy)
            .count();

        (healthy_count as f64 / relevant_checks.len() as f64) * 100.0
    }

    pub fn get_average_response_time(&self) -> u64 {
        if self.recent_checks.is_empty() {
            return 0;
        }

        let total: u64 = self
            .recent_checks
            .iter()
            .map(|check| check.response_time_ms)
            .sum();

        total / self.recent_checks.len() as u64
    }
}

/// Alert message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert ID
    pub id: String,
    /// Severity (critical, warning, info)
    pub severity: String,
    /// Alert title
    pub title: String,
    /// Alert description
    pub description: String,
    /// Component ID
    pub component_id: String,
    /// Component type
    pub component_type: ComponentType,
    /// Timestamp
    pub timestamp: u64,
    /// Additional context
    pub context: HashMap<String, String>,
}

impl Alert {
    pub fn new_critical(
        component_id: String,
        component_type: ComponentType,
        title: String,
        description: String,
    ) -> Self {
        Self {
            id: format!("alert-{}", uuid::Uuid::new_v4()),
            severity: "critical".to_string(),
            title,
            description,
            component_id,
            component_type,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            context: HashMap::new(),
        }
    }

    pub fn new_warning(
        component_id: String,
        component_type: ComponentType,
        title: String,
        description: String,
    ) -> Self {
        Self {
            id: format!("alert-{}", uuid::Uuid::new_v4()),
            severity: "warning".to_string(),
            title,
            description,
            component_id,
            component_type,
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            context: HashMap::new(),
        }
    }

    pub fn with_context(mut self, key: String, value: String) -> Self {
        self.context.insert(key, value);
        self
    }

    pub fn format_slack_message(&self) -> String {
        let emoji = match self.severity.as_str() {
            "critical" => "🚨",
            "warning" => "⚠️",
            _ => "ℹ️",
        };

        format!(
            "{} *{}*\n\n*Component:* {} ({})\n*Description:* {}\n*Time:* <t:{}:F>",
            emoji,
            self.title,
            self.component_id,
            format!("{:?}", self.component_type),
            self.description,
            self.timestamp
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_config() {
        let config = HealthCheckConfig::new_production();
        assert_eq!(config.interval_seconds, 30);
        assert_eq!(config.timeout_seconds, 10);
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.success_threshold, 2);
        assert!(config.auto_remediate);
    }

    #[test]
    fn test_health_check_result() {
        let result = HealthCheckResult::new("validator-1".to_string(), ComponentType::Validator)
            .healthy(50)
            .with_metric("cpu_percent".to_string(), 45.2);

        assert_eq!(result.status, HealthStatus::Healthy);
        assert_eq!(result.response_time_ms, 50);
        assert_eq!(result.metrics.get("cpu_percent"), Some(&45.2));
    }

    #[test]
    fn test_dns_failover_config() {
        let config = DNSFailoverConfig::new_production();
        assert_eq!(config.provider, "route53");
        assert_eq!(config.domain, "dchat.network");
        assert_eq!(config.ttl, 60);
        assert_eq!(config.policy, FailoverPolicy::LatencyBased);
    }

    #[test]
    fn test_auto_scaling_config() {
        let config = AutoScalingConfig::new_production();
        assert_eq!(config.min_instances, 3);
        assert_eq!(config.max_instances, 20);
        assert_eq!(config.target_cpu_percent, 70.0);
        assert_eq!(config.target_memory_percent, 80.0);
    }

    #[test]
    fn test_auto_scaling_decisions() {
        let config = AutoScalingConfig::new_production();

        // Should scale up
        assert!(config.should_scale_up(85.0, 70.0, 5));

        // Should not scale up (at max)
        assert!(!config.should_scale_up(85.0, 70.0, 20));

        // Should scale down
        assert!(config.should_scale_down(20.0, 30.0, 10));

        // Should not scale down (at min)
        assert!(!config.should_scale_down(20.0, 30.0, 3));
    }

    #[test]
    fn test_auto_scaling_target_calculation() {
        let config = AutoScalingConfig::new_production();

        // Scale up
        let target = config.calculate_target_instances(85.0, 5);
        assert_eq!(target, 7); // 5 + 2

        // Scale down
        let target = config.calculate_target_instances(25.0, 10);
        assert_eq!(target, 9); // 10 - 1

        // No change
        let target = config.calculate_target_instances(65.0, 8);
        assert_eq!(target, 8);
    }

    #[test]
    fn test_alert_channel_creation() {
        let slack = AlertChannel::new_slack("https://hooks.slack.com/test".to_string());
        assert_eq!(slack.channel_type, "slack");
        assert_eq!(slack.severity_levels.len(), 2);

        let pagerduty = AlertChannel::new_pagerduty("pd_key".to_string());
        assert_eq!(pagerduty.channel_type, "pagerduty");
        assert_eq!(pagerduty.severity_levels.len(), 1);
        assert_eq!(pagerduty.severity_levels[0], "critical");
    }

    #[test]
    fn test_bft_monitor_config() {
        let config = BFTMonitorConfig::new_production();
        assert_eq!(config.total_validators, 7);
        assert_eq!(config.min_healthy, 5);
        assert_eq!(config.alert_threshold, 6);

        // Consensus healthy
        assert!(config.is_consensus_healthy(7));
        assert!(config.is_consensus_healthy(5));
        assert!(!config.is_consensus_healthy(4));

        // Should alert
        assert!(config.should_alert(6));
        assert!(config.should_alert(5));
        assert!(!config.should_alert(7));

        // Percentage
        assert_eq!(config.calculate_consensus_percentage(7), 100.0);
        assert_eq!(config.calculate_consensus_percentage(5), 71.42857142857143);
    }

    #[test]
    fn test_health_monitor_config() {
        // Set required Grafana API key for test
        let original_grafana = std::env::var("GRAFANA_API_KEY");
        std::env::set_var("GRAFANA_API_KEY", "test_grafana_key");

        let config = HealthMonitorConfig::new_production();
        assert!(config.verify().is_ok());
        assert_eq!(config.alert_channels.len(), 2);
        assert_eq!(config.bft_monitor.total_validators, 7);

        // Restore Grafana key
        match original_grafana {
            Ok(val) => std::env::set_var("GRAFANA_API_KEY", val),
            Err(_) => std::env::remove_var("GRAFANA_API_KEY"),
        }
    }

    #[test]
    fn test_component_health_tracker() {
        let mut tracker =
            ComponentHealthTracker::new("validator-1".to_string(), ComponentType::Validator);
        let config = HealthCheckConfig::new_production();

        // First healthy check
        let result =
            HealthCheckResult::new("validator-1".to_string(), ComponentType::Validator).healthy(50);
        let changed = tracker.update(result, &config);
        assert_eq!(tracker.consecutive_successes, 1);
        assert!(!changed); // Not enough successes yet

        // Second healthy check
        let result =
            HealthCheckResult::new("validator-1".to_string(), ComponentType::Validator).healthy(45);
        let changed = tracker.update(result, &config);
        assert_eq!(tracker.consecutive_successes, 2);
        assert_eq!(tracker.current_status, HealthStatus::Healthy);
        assert!(changed); // Status changed to Healthy
    }

    #[test]
    fn test_component_uptime_calculation() {
        let mut tracker = ComponentHealthTracker::new("relay-1".to_string(), ComponentType::Relay);

        // Add 10 healthy checks
        for _ in 0..10 {
            let result =
                HealthCheckResult::new("relay-1".to_string(), ComponentType::Relay).healthy(30);
            tracker.recent_checks.push(result);
        }

        // Add 2 unhealthy checks
        for _ in 0..2 {
            let result = HealthCheckResult::new("relay-1".to_string(), ComponentType::Relay)
                .unhealthy("Connection timeout".to_string());
            tracker.recent_checks.push(result);
        }

        let uptime = tracker.calculate_uptime_percentage(3600);
        assert_eq!(uptime, 10.0 / 12.0 * 100.0); // 83.33%
    }

    #[test]
    fn test_alert_channel_validation() {
        // Valid Slack webhook
        let valid_slack = AlertChannel::new_slack(
            "https://hooks.slack.com/services/T12345678/B12345678/abc123def456".to_string(),
        );
        assert!(valid_slack.is_valid());

        // Invalid Slack webhook (placeholder)
        let invalid_slack =
            AlertChannel::new_slack("https://hooks.slack.com/services/XXX/YYY/ZZZ".to_string());
        assert!(!invalid_slack.is_valid());

        // Valid PagerDuty
        let valid_pd = AlertChannel::new_pagerduty("real_integration_key_12345".to_string());
        assert!(valid_pd.is_valid());

        // Invalid PagerDuty (placeholder)
        let invalid_pd = AlertChannel::new_pagerduty("pagerduty_integration_key".to_string());
        assert!(!invalid_pd.is_valid());

        // Invalid empty endpoint
        let empty_channel = AlertChannel {
            name: "test".to_string(),
            channel_type: "slack".to_string(),
            endpoint: "".to_string(),
            severity_levels: vec!["critical".to_string()],
            rate_limit: 30,
        };
        assert!(!empty_channel.is_valid());
    }

    #[test]
    fn test_health_monitor_config_with_env_vars() {
        // Save original env vars
        let original_slack = std::env::var("DCHAT_SLACK_WEBHOOK_URL");
        let original_pd = std::env::var("DCHAT_PAGERDUTY_KEY");
        let original_grafana = std::env::var("GRAFANA_API_KEY");

        // Set required Grafana API key for test
        std::env::set_var("GRAFANA_API_KEY", "test_grafana_key");

        // Test with valid env vars
        std::env::set_var(
            "DCHAT_SLACK_WEBHOOK_URL",
            "https://hooks.slack.com/services/T123/B456/xyz789",
        );
        std::env::set_var("DCHAT_PAGERDUTY_KEY", "real_key_abc123");

        let config = HealthMonitorConfig::new_production();
        assert_eq!(config.alert_channels.len(), 2);
        assert!(config.alert_channels[0].is_valid());
        assert!(config.alert_channels[1].is_valid());

        // Test with only Slack configured
        std::env::remove_var("DCHAT_PAGERDUTY_KEY");
        let config = HealthMonitorConfig::new_production();
        assert!(config.alert_channels.iter().any(|c| c.channel_type == "slack"));
        assert!(config.alert_channels.iter().any(|c| c.is_valid()));

        // Restore original env vars
        match original_slack {
            Ok(val) => std::env::set_var("DCHAT_SLACK_WEBHOOK_URL", val),
            Err(_) => std::env::remove_var("DCHAT_SLACK_WEBHOOK_URL"),
        }
        match original_pd {
            Ok(val) => std::env::set_var("DCHAT_PAGERDUTY_KEY", val),
            Err(_) => std::env::remove_var("DCHAT_PAGERDUTY_KEY"),
        }
        match original_grafana {
            Ok(val) => std::env::set_var("GRAFANA_API_KEY", val),
            Err(_) => std::env::remove_var("GRAFANA_API_KEY"),
        }
    }

    #[test]
    fn test_config_verification_warns_about_invalid_channels() {
        // Set required Grafana API key for test
        let original_grafana = std::env::var("GRAFANA_API_KEY");
        std::env::set_var("GRAFANA_API_KEY", "test_grafana_key");

        // Create config with placeholder channels
        let mut config = HealthMonitorConfig::new_production();
        
        // Override with placeholder channels
        config.alert_channels = vec![
            AlertChannel::new_slack("https://hooks.slack.com/services/XXX/YYY/ZZZ".to_string()),
            AlertChannel::new_pagerduty("pagerduty_integration_key".to_string()),
        ];

        // Verification should pass (with warnings) but not error
        assert!(config.verify().is_ok());

        // All channels should be invalid
        assert!(config.alert_channels.iter().all(|c| !c.is_valid()));

        // Restore Grafana key
        match original_grafana {
            Ok(val) => std::env::set_var("GRAFANA_API_KEY", val),
            Err(_) => std::env::remove_var("GRAFANA_API_KEY"),
        }
    }

    #[test]
    fn test_alert_creation() {
        let alert = Alert::new_critical(
            "validator-1".to_string(),
            ComponentType::Validator,
            "Validator Offline".to_string(),
            "Validator has been unreachable for 5 minutes".to_string(),
        )
        .with_context("region".to_string(), "us-east-1".to_string());

        assert_eq!(alert.severity, "critical");
        assert_eq!(alert.title, "Validator Offline");
        assert_eq!(alert.context.get("region"), Some(&"us-east-1".to_string()));

        let slack_msg = alert.format_slack_message();
        assert!(slack_msg.contains("🚨"));
        assert!(slack_msg.contains("Validator Offline"));
    }

    #[test]
    fn test_alert_slack_formatting() {
        let alert = Alert::new_warning(
            "storage-1".to_string(),
            ComponentType::CockroachDB,
            "High Disk Usage".to_string(),
            "Disk usage at 85%".to_string(),
        );

        let slack_msg = alert.format_slack_message();
        assert!(slack_msg.contains("⚠️"));
        assert!(slack_msg.contains("High Disk Usage"));
        assert!(slack_msg.contains("storage-1"));
    }

    #[test]
    fn test_prometheus_config() {
        let config = PrometheusConfig::new_production();
        assert_eq!(config.endpoint, "http://prometheus:9090");
        assert_eq!(config.scrape_interval, 30);
        assert_eq!(config.retention_days, 90);
        assert!(!config.remote_write_enabled);
    }

    #[test]
    fn test_grafana_config() {
        // Set required Grafana API key for test
        let original_grafana = std::env::var("GRAFANA_API_KEY");
        std::env::set_var("GRAFANA_API_KEY", "test_grafana_key");

        let config = GrafanaConfig::new_production();
        assert_eq!(config.endpoint, "https://grafana.dchat.internal");
        assert_eq!(config.dashboards.len(), 4);
        assert!(config
            .dashboards
            .contains(&"infrastructure-overview".to_string()));

        // Restore Grafana key
        match original_grafana {
            Ok(val) => std::env::set_var("GRAFANA_API_KEY", val),
            Err(_) => std::env::remove_var("GRAFANA_API_KEY"),
        }
    }
}
