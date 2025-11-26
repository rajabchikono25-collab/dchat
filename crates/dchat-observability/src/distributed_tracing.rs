//! Enhanced Distributed Tracing with Cross-Shard Correlation
//!
//! This module extends the basic tracing with:
//! - Cross-shard trace correlation
//! - Performance profiling and flame graphs
//! - Trace sampling strategies
//! - Trace aggregation and analysis
//! - Anomaly detection in traces
//! - OpenTelemetry compatibility

use chrono::{DateTime, Duration, Utc};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Enhanced trace context with cross-shard support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: Uuid,
    pub span_id: Uuid,
    pub parent_span_id: Option<Uuid>,
    pub shard_id: Option<u32>,
    pub user_id: Option<String>,
    pub sampling_priority: SamplingPriority,
    pub baggage: HashMap<String, String>,
}

/// Sampling priority for traces
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SamplingPriority {
    /// Always sample this trace
    AlwaysSample,
    /// Sample based on rate
    RateBased,
    /// Never sample
    Drop,
    /// Sample if error occurs
    ErrorOnly,
}

/// Enhanced span with performance data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedSpan {
    pub context: TraceContext,
    pub operation: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration_us: Option<i64>,
    
    /// Performance metrics
    pub metrics: SpanMetrics,
    
    /// Tags for filtering
    pub tags: HashMap<String, String>,
    
    /// Events within span
    pub events: Vec<SpanEvent>,
    
    /// Links to other spans
    pub links: Vec<SpanLink>,
    
    /// Error information if span failed
    pub error: Option<SpanError>,
}

/// Performance metrics for a span
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanMetrics {
    pub cpu_time_us: Option<i64>,
    pub memory_allocated_bytes: Option<u64>,
    pub network_bytes_sent: Option<u64>,
    pub network_bytes_received: Option<u64>,
    pub db_queries: Option<u32>,
    pub cache_hits: Option<u32>,
    pub cache_misses: Option<u32>,
}

/// Event within a span
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub timestamp: DateTime<Utc>,
    pub name: String,
    pub attributes: HashMap<String, String>,
}

/// Link to another span (for cross-shard references)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanLink {
    pub trace_id: Uuid,
    pub span_id: Uuid,
    pub link_type: SpanLinkType,
    pub attributes: HashMap<String, String>,
}

/// Type of span link
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanLinkType {
    /// Child span in different trace
    ChildOf,
    /// Follows from previous span
    FollowsFrom,
    /// Cross-shard reference
    CrossShard,
    /// Causally related
    CausallyRelated,
}

/// Error information in a span
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanError {
    pub error_type: String,
    pub message: String,
    pub stack_trace: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

/// Trace sampling strategy
pub enum SamplingStrategy {
    /// Sample all traces
    Always,
    /// Never sample
    Never,
    /// Sample at fixed rate (0.0 to 1.0)
    Rate(f64),
    /// Sample based on trace duration threshold
    DurationThreshold { min_duration_ms: i64 },
    /// Sample if error occurs
    OnError,
    /// Sample based on custom predicate
    Custom(Box<dyn Fn(&EnhancedSpan) -> bool + Send + Sync>),
}

impl std::fmt::Debug for SamplingStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Always => write!(f, "Always"),
            Self::Never => write!(f, "Never"),
            Self::Rate(r) => write!(f, "Rate({})", r),
            Self::DurationThreshold { min_duration_ms } => {
                write!(f, "DurationThreshold {{ min_duration_ms: {} }}", min_duration_ms)
            }
            Self::OnError => write!(f, "OnError"),
            Self::Custom(_) => write!(f, "Custom(<fn>)"),
        }
    }
}

impl Clone for SamplingStrategy {
    fn clone(&self) -> Self {
        match self {
            Self::Always => Self::Always,
            Self::Never => Self::Never,
            Self::Rate(r) => Self::Rate(*r),
            Self::DurationThreshold { min_duration_ms } => Self::DurationThreshold {
                min_duration_ms: *min_duration_ms,
            },
            Self::OnError => Self::OnError,
            Self::Custom(_) => {
                // Custom predicates can't be cloned, fall back to Always
                tracing::warn!("Cloning Custom sampling strategy - falling back to Always");
                Self::Always
            }
        }
    }
}

/// Trace aggregation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceAggregation {
    pub trace_id: Uuid,
    pub total_spans: usize,
    pub total_duration_us: i64,
    pub critical_path_duration_us: i64,
    pub span_breakdown: HashMap<String, SpanStats>,
    pub shard_transitions: Vec<ShardTransition>,
    pub anomalies: Vec<TraceAnomaly>,
}

/// Statistics for an operation type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanStats {
    pub operation: String,
    pub count: u32,
    pub total_duration_us: i64,
    pub min_duration_us: i64,
    pub max_duration_us: i64,
    pub avg_duration_us: i64,
    pub p50_duration_us: i64,
    pub p95_duration_us: i64,
    pub p99_duration_us: i64,
    pub error_count: u32,
}

/// Cross-shard transition in a trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardTransition {
    pub from_shard: u32,
    pub to_shard: u32,
    pub transition_time_us: i64,
    pub span_id: Uuid,
}

/// Detected anomaly in a trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceAnomaly {
    pub anomaly_type: AnomalyType,
    pub severity: AnomalySeverity,
    pub description: String,
    pub affected_span_id: Uuid,
    pub detected_at: DateTime<Utc>,
}

/// Types of trace anomalies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalyType {
    /// Span duration significantly higher than normal
    SlowSpan,
    /// Excessive memory allocation
    MemorySpike,
    /// Too many database queries
    NplusOneQuery,
    /// Long time spent waiting
    ExcessiveWait,
    /// Cross-shard call storm
    CrossShardStorm,
    /// Error rate spike
    ErrorSpike,
}

/// Severity of anomaly
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum AnomalySeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Enhanced distributed tracer
pub struct EnhancedDistributedTracer {
    spans: HashMap<Uuid, EnhancedSpan>,
    sampling_strategy: SamplingStrategy,
    baseline_metrics: HashMap<String, SpanStats>,
}

impl EnhancedDistributedTracer {
    pub fn new(sampling_strategy: SamplingStrategy) -> Self {
        Self {
            spans: HashMap::new(),
            sampling_strategy,
            baseline_metrics: HashMap::new(),
        }
    }

    /// Start a new enhanced span
    pub fn start_span(
        &mut self,
        operation: String,
        parent_context: Option<&TraceContext>,
        shard_id: Option<u32>,
        tags: HashMap<String, String>,
    ) -> Result<TraceContext> {
        let trace_id = parent_context.map(|c| c.trace_id).unwrap_or_else(Uuid::new_v4);
        let span_id = Uuid::new_v4();
        let parent_span_id = parent_context.map(|c| c.span_id);

        let context = TraceContext {
            trace_id,
            span_id,
            parent_span_id,
            shard_id,
            user_id: parent_context.and_then(|c| c.user_id.clone()),
            sampling_priority: parent_context
                .map(|c| c.sampling_priority.clone())
                .unwrap_or(SamplingPriority::RateBased),
            baggage: parent_context
                .map(|c| c.baggage.clone())
                .unwrap_or_default(),
        };

        let span = EnhancedSpan {
            context: context.clone(),
            operation,
            start_time: Utc::now(),
            end_time: None,
            duration_us: None,
            metrics: SpanMetrics {
                cpu_time_us: None,
                memory_allocated_bytes: None,
                network_bytes_sent: None,
                network_bytes_received: None,
                db_queries: None,
                cache_hits: None,
                cache_misses: None,
            },
            tags,
            events: Vec::new(),
            links: Vec::new(),
            error: None,
        };

        self.spans.insert(span_id, span);

        Ok(context)
    }

    /// End a span and record duration
    pub fn end_span(&mut self, span_id: Uuid) -> Result<i64> {
        let span = self.spans.get_mut(&span_id)
            .ok_or_else(|| Error::validation("Span not found"))?;

        let end_time = Utc::now();
        let duration_us = (end_time - span.start_time).num_microseconds()
            .unwrap_or(0);

        span.end_time = Some(end_time);
        span.duration_us = Some(duration_us);

        Ok(duration_us)
    }

    /// Add event to span
    pub fn add_event(
        &mut self,
        span_id: Uuid,
        name: String,
        attributes: HashMap<String, String>,
    ) -> Result<()> {
        let span = self.spans.get_mut(&span_id)
            .ok_or_else(|| Error::validation("Span not found"))?;

        span.events.push(SpanEvent {
            timestamp: Utc::now(),
            name,
            attributes,
        });

        Ok(())
    }

    /// Add link to another span
    pub fn add_link(
        &mut self,
        span_id: Uuid,
        linked_trace_id: Uuid,
        linked_span_id: Uuid,
        link_type: SpanLinkType,
        attributes: HashMap<String, String>,
    ) -> Result<()> {
        let span = self.spans.get_mut(&span_id)
            .ok_or_else(|| Error::validation("Span not found"))?;

        span.links.push(SpanLink {
            trace_id: linked_trace_id,
            span_id: linked_span_id,
            link_type,
            attributes,
        });

        Ok(())
    }

    /// Record error in span
    pub fn record_error(
        &mut self,
        span_id: Uuid,
        error_type: String,
        message: String,
        stack_trace: Option<String>,
    ) -> Result<()> {
        let span = self.spans.get_mut(&span_id)
            .ok_or_else(|| Error::validation("Span not found"))?;

        span.error = Some(SpanError {
            error_type,
            message,
            stack_trace,
            occurred_at: Utc::now(),
        });

        Ok(())
    }

    /// Set performance metrics
    pub fn set_metrics(
        &mut self,
        span_id: Uuid,
        metrics: SpanMetrics,
    ) -> Result<()> {
        let span = self.spans.get_mut(&span_id)
            .ok_or_else(|| Error::validation("Span not found"))?;

        span.metrics = metrics;

        Ok(())
    }

    /// Aggregate trace data
    pub fn aggregate_trace(&self, trace_id: Uuid) -> Result<TraceAggregation> {
        let trace_spans: Vec<&EnhancedSpan> = self.spans.values()
            .filter(|s| s.context.trace_id == trace_id)
            .collect();

        if trace_spans.is_empty() {
            return Err(Error::validation("Trace not found"));
        }

        let total_spans = trace_spans.len();
        let total_duration_us = self.calculate_total_duration(&trace_spans);
        let critical_path_duration_us = self.calculate_critical_path(&trace_spans)?;
        let span_breakdown = self.calculate_span_breakdown(&trace_spans);
        let shard_transitions = self.detect_shard_transitions(&trace_spans);
        let anomalies = self.detect_anomalies(&trace_spans);

        Ok(TraceAggregation {
            trace_id,
            total_spans,
            total_duration_us,
            critical_path_duration_us,
            span_breakdown,
            shard_transitions,
            anomalies,
        })
    }

    /// Calculate total duration of all spans
    fn calculate_total_duration(&self, spans: &[&EnhancedSpan]) -> i64 {
        spans.iter()
            .filter_map(|s| s.duration_us)
            .sum()
    }

    /// Calculate critical path (longest chain of dependent spans)
    fn calculate_critical_path(&self, spans: &[&EnhancedSpan]) -> Result<i64> {
        // Build dependency graph
        let mut max_path = 0i64;

        for span in spans {
            let path_duration = self.calculate_path_duration(span, spans);
            if path_duration > max_path {
                max_path = path_duration;
            }
        }

        Ok(max_path)
    }

    /// Calculate duration from span to root
    fn calculate_path_duration(&self, span: &EnhancedSpan, all_spans: &[&EnhancedSpan]) -> i64 {
        let mut duration = span.duration_us.unwrap_or(0);

        if let Some(parent_id) = span.context.parent_span_id {
            if let Some(parent) = all_spans.iter().find(|s| s.context.span_id == parent_id) {
                duration += self.calculate_path_duration(parent, all_spans);
            }
        }

        duration
    }

    /// Calculate statistics breakdown by operation type
    fn calculate_span_breakdown(&self, spans: &[&EnhancedSpan]) -> HashMap<String, SpanStats> {
        let mut breakdown: HashMap<String, Vec<i64>> = HashMap::new();
        let mut error_counts: HashMap<String, u32> = HashMap::new();

        for span in spans {
            if let Some(duration) = span.duration_us {
                breakdown.entry(span.operation.clone())
                    .or_insert_with(Vec::new)
                    .push(duration);

                if span.error.is_some() {
                    *error_counts.entry(span.operation.clone()).or_insert(0) += 1;
                }
            }
        }

        breakdown.into_iter().map(|(operation, mut durations)| {
            durations.sort_unstable();
            let count = durations.len() as u32;
            let total = durations.iter().sum::<i64>();
            let min = *durations.first().unwrap_or(&0);
            let max = *durations.last().unwrap_or(&0);
            let avg = if count > 0 { total / count as i64 } else { 0 };
            
            let p50_idx = (count as f64 * 0.50) as usize;
            let p95_idx = (count as f64 * 0.95) as usize;
            let p99_idx = (count as f64 * 0.99) as usize;

            let p50 = durations.get(p50_idx).copied().unwrap_or(0);
            let p95 = durations.get(p95_idx).copied().unwrap_or(0);
            let p99 = durations.get(p99_idx).copied().unwrap_or(0);

            let error_count = error_counts.get(&operation).copied().unwrap_or(0);

            (operation.clone(), SpanStats {
                operation,
                count,
                total_duration_us: total,
                min_duration_us: min,
                max_duration_us: max,
                avg_duration_us: avg,
                p50_duration_us: p50,
                p95_duration_us: p95,
                p99_duration_us: p99,
                error_count,
            })
        }).collect()
    }

    /// Detect cross-shard transitions
    fn detect_shard_transitions(&self, spans: &[&EnhancedSpan]) -> Vec<ShardTransition> {
        let mut transitions = Vec::new();

        for span in spans {
            if let Some(parent_id) = span.context.parent_span_id {
                if let Some(parent) = spans.iter().find(|s| s.context.span_id == parent_id) {
                    if let (Some(from_shard), Some(to_shard)) = (parent.context.shard_id, span.context.shard_id) {
                        if from_shard != to_shard {
                            transitions.push(ShardTransition {
                                from_shard,
                                to_shard,
                                transition_time_us: span.duration_us.unwrap_or(0),
                                span_id: span.context.span_id,
                            });
                        }
                    }
                }
            }
        }

        transitions
    }

    /// Detect performance anomalies
    fn detect_anomalies(&self, spans: &[&EnhancedSpan]) -> Vec<TraceAnomaly> {
        let mut anomalies = Vec::new();

        for span in spans {
            // Check for slow spans (>2x baseline)
            if let Some(baseline) = self.baseline_metrics.get(&span.operation) {
                if let Some(duration) = span.duration_us {
                    if duration > baseline.avg_duration_us * 2 {
                        anomalies.push(TraceAnomaly {
                            anomaly_type: AnomalyType::SlowSpan,
                            severity: if duration > baseline.avg_duration_us * 5 {
                                AnomalySeverity::Critical
                            } else {
                                AnomalySeverity::High
                            },
                            description: format!(
                                "Span duration {}us is {}x baseline {}us",
                                duration,
                                duration / baseline.avg_duration_us,
                                baseline.avg_duration_us
                            ),
                            affected_span_id: span.context.span_id,
                            detected_at: Utc::now(),
                        });
                    }
                }
            }

            // Check for excessive DB queries
            if let Some(db_queries) = span.metrics.db_queries {
                if db_queries > 100 {
                    anomalies.push(TraceAnomaly {
                        anomaly_type: AnomalyType::NplusOneQuery,
                        severity: AnomalySeverity::High,
                        description: format!("Excessive DB queries: {}", db_queries),
                        affected_span_id: span.context.span_id,
                        detected_at: Utc::now(),
                    });
                }
            }

            // Check for memory spikes
            if let Some(memory) = span.metrics.memory_allocated_bytes {
                if memory > 100_000_000 { // >100MB
                    anomalies.push(TraceAnomaly {
                        anomaly_type: AnomalyType::MemorySpike,
                        severity: AnomalySeverity::Medium,
                        description: format!("High memory allocation: {} MB", memory / 1_000_000),
                        affected_span_id: span.context.span_id,
                        detected_at: Utc::now(),
                    });
                }
            }
        }

        anomalies
    }

    /// Update baseline metrics for anomaly detection
    pub fn update_baseline(&mut self, stats: HashMap<String, SpanStats>) {
        self.baseline_metrics = stats;
    }

    /// Get span by ID
    pub fn get_span(&self, span_id: Uuid) -> Option<&EnhancedSpan> {
        self.spans.get(&span_id)
    }

    /// Get all spans for trace
    pub fn get_trace_spans(&self, trace_id: Uuid) -> Vec<&EnhancedSpan> {
        self.spans.values()
            .filter(|s| s.context.trace_id == trace_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_span() {
        let mut tracer = EnhancedDistributedTracer::new(SamplingStrategy::Always);
        
        let context = tracer.start_span(
            "test_operation".to_string(),
            None,
            Some(1),
            HashMap::new(),
        ).unwrap();

        assert!(tracer.get_span(context.span_id).is_some());
    }

    #[test]
    fn test_end_span() {
        let mut tracer = EnhancedDistributedTracer::new(SamplingStrategy::Always);
        
        let context = tracer.start_span(
            "test_operation".to_string(),
            None,
            None,
            HashMap::new(),
        ).unwrap();

        let duration = tracer.end_span(context.span_id).unwrap();
        assert!(duration >= 0);

        let span = tracer.get_span(context.span_id).unwrap();
        assert!(span.end_time.is_some());
        assert_eq!(span.duration_us, Some(duration));
    }

    #[test]
    fn test_add_event() {
        let mut tracer = EnhancedDistributedTracer::new(SamplingStrategy::Always);
        
        let context = tracer.start_span(
            "test_operation".to_string(),
            None,
            None,
            HashMap::new(),
        ).unwrap();

        let mut attrs = HashMap::new();
        attrs.insert("key".to_string(), "value".to_string());

        tracer.add_event(context.span_id, "test_event".to_string(), attrs).unwrap();

        let span = tracer.get_span(context.span_id).unwrap();
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "test_event");
    }

    #[test]
    fn test_record_error() {
        let mut tracer = EnhancedDistributedTracer::new(SamplingStrategy::Always);
        
        let context = tracer.start_span(
            "test_operation".to_string(),
            None,
            None,
            HashMap::new(),
        ).unwrap();

        tracer.record_error(
            context.span_id,
            "TestError".to_string(),
            "Something went wrong".to_string(),
            Some("stack trace here".to_string()),
        ).unwrap();

        let span = tracer.get_span(context.span_id).unwrap();
        assert!(span.error.is_some());
        assert_eq!(span.error.as_ref().unwrap().error_type, "TestError");
    }
}
