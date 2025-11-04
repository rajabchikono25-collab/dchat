//! Distributed Observability and Monitoring Infrastructure
//!
//! This module provides:
//! - Prometheus-style metrics collection
//! - Distributed tracing capabilities
//! - Health check endpoints
//! - Network health dashboards
//! - Alert rule evaluation and routing

pub mod alerting;

use chrono::{DateTime, Utc};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Metric types supported by the observability system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MetricType {
    /// Counter that only increases
    Counter,
    /// Gauge that can go up and down
    Gauge,
    /// Histogram for distributions
    Histogram,
    /// Summary for quantiles
    Summary,
}

/// A single metric value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub metric_type: MetricType,
    pub value: f64,
    pub labels: HashMap<String, String>,
    pub timestamp: DateTime<Utc>,
    pub help: String,
}

/// Health status of a component
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthStatus {
    /// Component is healthy
    Healthy,
    /// Component is degraded but functional
    Degraded,
    /// Component is unhealthy
    Unhealthy,
}

/// Health check result for a component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub component: String,
    pub status: HealthStatus,
    pub message: String,
    pub checked_at: DateTime<Utc>,
    pub details: HashMap<String, String>,
}

/// Distributed trace span
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub tags: HashMap<String, String>,
    pub logs: Vec<SpanLog>,
}

/// Log entry within a trace span
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanLog {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
    pub fields: HashMap<String, String>,
}

/// Log severity level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Metrics collector and manager
pub struct MetricsCollector {
    metrics: Arc<RwLock<HashMap<String, Metric>>>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a counter metric
    pub async fn record_counter(
        &self,
        name: String,
        value: f64,
        labels: HashMap<String, String>,
        help: String,
    ) -> Result<()> {
        let mut metrics = self.metrics.write().await;

        let key = Self::metric_key(&name, &labels);
        if let Some(existing) = metrics.get_mut(&key) {
            existing.value += value;
            existing.timestamp = Utc::now();
        } else {
            metrics.insert(
                key,
                Metric {
                    name,
                    metric_type: MetricType::Counter,
                    value,
                    labels,
                    timestamp: Utc::now(),
                    help,
                },
            );
        }
        Ok(())
    }

    /// Set a gauge metric
    pub async fn set_gauge(
        &self,
        name: String,
        value: f64,
        labels: HashMap<String, String>,
        help: String,
    ) -> Result<()> {
        let mut metrics = self.metrics.write().await;

        let key = Self::metric_key(&name, &labels);
        metrics.insert(
            key,
            Metric {
                name,
                metric_type: MetricType::Gauge,
                value,
                labels,
                timestamp: Utc::now(),
                help,
            },
        );
        Ok(())
    }

    /// Record a histogram observation
    pub async fn observe_histogram(
        &self,
        name: String,
        value: f64,
        labels: HashMap<String, String>,
        help: String,
    ) -> Result<()> {
        let mut metrics = self.metrics.write().await;

        let key = Self::metric_key(&name, &labels);
        metrics.insert(
            key,
            Metric {
                name,
                metric_type: MetricType::Histogram,
                value,
                labels,
                timestamp: Utc::now(),
                help,
            },
        );
        Ok(())
    }

    /// Get all metrics
    pub async fn get_metrics(&self) -> Vec<Metric> {
        let metrics = self.metrics.read().await;
        metrics.values().cloned().collect()
    }

    /// Clear all metrics
    pub async fn clear(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.clear();
    }

    fn metric_key(name: &str, labels: &HashMap<String, String>) -> String {
        let mut key = name.to_string();
        let mut label_vec: Vec<_> = labels.iter().collect();
        label_vec.sort_by_key(|(k, _)| *k);
        for (k, v) in label_vec {
            key.push_str(&format!(",{}={}", k, v));
        }
        key
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Health checker for system components
pub struct HealthChecker {
    checks: Arc<RwLock<Vec<HealthCheck>>>,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new() -> Self {
        Self {
            checks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a health check result
    pub async fn add_check(
        &self,
        component: String,
        status: HealthStatus,
        message: String,
        details: HashMap<String, String>,
    ) -> Result<()> {
        let mut checks = self.checks.write().await;

        checks.push(HealthCheck {
            component,
            status,
            message,
            checked_at: Utc::now(),
            details,
        });

        Ok(())
    }

    /// Get all health checks
    pub async fn get_checks(&self) -> Vec<HealthCheck> {
        let checks = self.checks.read().await;
        checks.clone()
    }

    /// Get overall system health
    pub async fn get_overall_health(&self) -> HealthStatus {
        let checks = self.checks.read().await;

        if checks.is_empty() {
            return HealthStatus::Healthy;
        }

        let has_unhealthy = checks.iter().any(|c| c.status == HealthStatus::Unhealthy);
        let has_degraded = checks.iter().any(|c| c.status == HealthStatus::Degraded);

        if has_unhealthy {
            HealthStatus::Unhealthy
        } else if has_degraded {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        }
    }

    /// Clear all health checks
    pub async fn clear(&self) {
        let mut checks = self.checks.write().await;
        checks.clear();
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Distributed tracing system
pub struct DistributedTracer {
    spans: Arc<RwLock<HashMap<String, TraceSpan>>>,
}

impl DistributedTracer {
    /// Create a new distributed tracer
    pub fn new() -> Self {
        Self {
            spans: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a new trace span
    pub async fn start_span(
        &self,
        trace_id: String,
        span_id: String,
        parent_span_id: Option<String>,
        operation_name: String,
        tags: HashMap<String, String>,
    ) -> Result<()> {
        let mut spans = self.spans.write().await;

        let span = TraceSpan {
            trace_id: trace_id.clone(),
            span_id: span_id.clone(),
            parent_span_id,
            operation_name,
            start_time: Utc::now(),
            end_time: None,
            tags,
            logs: Vec::new(),
        };

        spans.insert(span_id, span);
        Ok(())
    }

    /// End a trace span
    pub async fn end_span(&self, span_id: &str) -> Result<()> {
        let mut spans = self.spans.write().await;

        if let Some(span) = spans.get_mut(span_id) {
            span.end_time = Some(Utc::now());
            Ok(())
        } else {
            Err(Error::validation("Span not found"))
        }
    }

    /// Add a log to a span
    pub async fn log_to_span(
        &self,
        span_id: &str,
        level: LogLevel,
        message: String,
        fields: HashMap<String, String>,
    ) -> Result<()> {
        let mut spans = self.spans.write().await;

        if let Some(span) = spans.get_mut(span_id) {
            span.logs.push(SpanLog {
                timestamp: Utc::now(),
                level,
                message,
                fields,
            });
            Ok(())
        } else {
            Err(Error::validation("Span not found"))
        }
    }

    /// Get a span by ID
    pub async fn get_span(&self, span_id: &str) -> Option<TraceSpan> {
        let spans = self.spans.read().await;
        spans.get(span_id).cloned()
    }

    /// Get all spans for a trace
    pub async fn get_trace(&self, trace_id: &str) -> Vec<TraceSpan> {
        let spans = self.spans.read().await;
        spans
            .values()
            .filter(|s| s.trace_id == trace_id)
            .cloned()
            .collect()
    }

    /// Get span duration in milliseconds
    pub fn span_duration_ms(span: &TraceSpan) -> Option<i64> {
        span.end_time
            .map(|end| (end - span.start_time).num_milliseconds())
    }
}

impl Default for DistributedTracer {
    fn default() -> Self {
        Self::new()
    }
}

/// Comprehensive observability manager integrating all subsystems
pub struct ObservabilityManager {
    pub metrics: MetricsCollector,
    pub health: HealthChecker,
    pub tracer: DistributedTracer,
    pub alerting: alerting::AlertManager,
}

impl ObservabilityManager {
    /// Create a new observability manager with all subsystems
    pub fn new() -> Self {
        Self {
            metrics: MetricsCollector::new(),
            health: HealthChecker::new(),
            tracer: DistributedTracer::new(),
            alerting: alerting::AlertManager::new(),
        }
    }
}

impl Default for ObservabilityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_counter() {
        let collector = MetricsCollector::new();
        let mut labels = HashMap::new();
        labels.insert("endpoint".to_string(), "/api/messages".to_string());

        collector
            .record_counter(
                "http_requests_total".to_string(),
                1.0,
                labels.clone(),
                "Total HTTP requests".to_string(),
            )
            .await
            .unwrap();

        collector
            .record_counter(
                "http_requests_total".to_string(),
                1.0,
                labels,
                "Total HTTP requests".to_string(),
            )
            .await
            .unwrap();

        let metrics = collector.get_metrics().await;
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].value, 2.0);
    }

    #[tokio::test]
    async fn test_set_gauge() {
        let collector = MetricsCollector::new();
        let labels = HashMap::new();

        collector
            .set_gauge(
                "active_connections".to_string(),
                42.0,
                labels.clone(),
                "Active connections".to_string(),
            )
            .await
            .unwrap();

        collector
            .set_gauge(
                "active_connections".to_string(),
                35.0,
                labels,
                "Active connections".to_string(),
            )
            .await
            .unwrap();

        let metrics = collector.get_metrics().await;
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].value, 35.0);
        assert_eq!(metrics[0].metric_type, MetricType::Gauge);
    }

    #[tokio::test]
    async fn test_observe_histogram() {
        let collector = MetricsCollector::new();
        let labels = HashMap::new();

        collector
            .observe_histogram(
                "request_duration_ms".to_string(),
                125.5,
                labels,
                "Request duration".to_string(),
            )
            .await
            .unwrap();

        let metrics = collector.get_metrics().await;
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].metric_type, MetricType::Histogram);
    }

    #[tokio::test]
    async fn test_health_checker() {
        let checker = HealthChecker::new();
        let mut details = HashMap::new();
        details.insert("uptime".to_string(), "99.9%".to_string());

        checker
            .add_check(
                "database".to_string(),
                HealthStatus::Healthy,
                "DB is responsive".to_string(),
                details,
            )
            .await
            .unwrap();

        let checks = checker.get_checks().await;
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].component, "database");
        assert_eq!(checks[0].status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_overall_health() {
        let checker = HealthChecker::new();

        checker
            .add_check(
                "service1".to_string(),
                HealthStatus::Healthy,
                "OK".to_string(),
                HashMap::new(),
            )
            .await
            .unwrap();

        checker
            .add_check(
                "service2".to_string(),
                HealthStatus::Degraded,
                "Slow".to_string(),
                HashMap::new(),
            )
            .await
            .unwrap();

        let overall = checker.get_overall_health().await;
        assert_eq!(overall, HealthStatus::Degraded);
    }

    #[tokio::test]
    async fn test_overall_health_unhealthy() {
        let checker = HealthChecker::new();

        checker
            .add_check(
                "service1".to_string(),
                HealthStatus::Healthy,
                "OK".to_string(),
                HashMap::new(),
            )
            .await
            .unwrap();

        checker
            .add_check(
                "service2".to_string(),
                HealthStatus::Unhealthy,
                "Down".to_string(),
                HashMap::new(),
            )
            .await
            .unwrap();

        let overall = checker.get_overall_health().await;
        assert_eq!(overall, HealthStatus::Unhealthy);
    }

    #[tokio::test]
    async fn test_distributed_tracing() {
        let tracer = DistributedTracer::new();
        let mut tags = HashMap::new();
        tags.insert("service".to_string(), "api".to_string());

        tracer
            .start_span(
                "trace_123".to_string(),
                "span_1".to_string(),
                None,
                "handle_request".to_string(),
                tags,
            )
            .await
            .unwrap();

        let span = tracer.get_span("span_1").await.unwrap();
        assert_eq!(span.operation_name, "handle_request");
        assert!(span.end_time.is_none());
    }

    #[tokio::test]
    async fn test_end_span() {
        let tracer = DistributedTracer::new();

        tracer
            .start_span(
                "trace_456".to_string(),
                "span_2".to_string(),
                None,
                "process_message".to_string(),
                HashMap::new(),
            )
            .await
            .unwrap();

        tracer.end_span("span_2").await.unwrap();

        let span = tracer.get_span("span_2").await.unwrap();
        assert!(span.end_time.is_some());
    }

    #[tokio::test]
    async fn test_span_logging() {
        let tracer = DistributedTracer::new();

        tracer
            .start_span(
                "trace_789".to_string(),
                "span_3".to_string(),
                None,
                "database_query".to_string(),
                HashMap::new(),
            )
            .await
            .unwrap();

        let mut fields = HashMap::new();
        fields.insert("query".to_string(), "SELECT * FROM users".to_string());

        tracer
            .log_to_span(
                "span_3",
                LogLevel::Info,
                "Executing query".to_string(),
                fields,
            )
            .await
            .unwrap();

        let span = tracer.get_span("span_3").await.unwrap();
        assert_eq!(span.logs.len(), 1);
        assert_eq!(span.logs[0].level, LogLevel::Info);
    }
}
