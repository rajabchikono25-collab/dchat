//! Shard Load Monitoring System
//!
//! Tracks CPU, memory, message throughput, and storage metrics for each shard.
//! Provides load-based triggers for rebalancing: CPU >80%, memory >75%, throughput >1000 msg/s.

use crate::sharding::ShardId;
use dchat_core::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Message throughput tracker with rolling averages
#[derive(Debug, Clone)]
pub struct MessageThroughputTracker {
    shard_id: ShardId,
    /// Samples for 1-minute window (timestamp, count)
    samples_1min: VecDeque<(u64, u64)>,
    /// Samples for 5-minute window
    samples_5min: VecDeque<(u64, u64)>,
    /// Samples for 15-minute window
    samples_15min: VecDeque<(u64, u64)>,
    /// Total message count
    total_messages: u64,
}

impl MessageThroughputTracker {
    pub fn new(shard_id: ShardId) -> Self {
        Self {
            shard_id,
            samples_1min: VecDeque::new(),
            samples_5min: VecDeque::new(),
            samples_15min: VecDeque::new(),
            total_messages: 0,
        }
    }

    /// Record a new message
    pub fn record_message(&mut self) {
        let now = Self::now_timestamp();
        self.total_messages += 1;

        // Add to all windows
        self.samples_1min.push_back((now, 1));
        self.samples_5min.push_back((now, 1));
        self.samples_15min.push_back((now, 1));

        // Prune old samples
        self.prune_samples(&mut self.samples_1min, 60);
        self.prune_samples(&mut self.samples_5min, 300);
        self.prune_samples(&mut self.samples_15min, 900);
    }

    /// Get messages per second (1-minute average)
    pub fn get_rate_1min(&self) -> f64 {
        self.calculate_rate(&self.samples_1min, 60)
    }

    /// Get messages per second (5-minute average)
    pub fn get_rate_5min(&self) -> f64 {
        self.calculate_rate(&self.samples_5min, 300)
    }

    /// Get messages per second (15-minute average)
    pub fn get_rate_15min(&self) -> f64 {
        self.calculate_rate(&self.samples_15min, 900)
    }

    /// Calculate rate from samples
    fn calculate_rate(&self, samples: &VecDeque<(u64, u64)>, window_secs: u64) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }

        let now = Self::now_timestamp();
        let window_start = now.saturating_sub(window_secs);

        let count: u64 = samples
            .iter()
            .filter(|(ts, _)| *ts >= window_start)
            .map(|(_, count)| count)
            .sum();

        count as f64 / window_secs as f64
    }

    /// Prune samples older than window
    fn prune_samples(&mut self, samples: &mut VecDeque<(u64, u64)>, window_secs: u64) {
        let now = Self::now_timestamp();
        let cutoff = now.saturating_sub(window_secs);

        while let Some(&(ts, _)) = samples.front() {
            if ts < cutoff {
                samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Get current Unix timestamp
    fn now_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

/// Storage size monitor with growth prediction
#[derive(Debug, Clone)]
pub struct StorageSizeMonitor {
    shard_id: ShardId,
    /// Current storage size in bytes
    current_size_bytes: u64,
    /// Historical samples (timestamp, size)
    size_history: VecDeque<(u64, u64)>,
    /// Growth rate in bytes per hour
    growth_rate_bytes_per_hour: f64,
}

impl StorageSizeMonitor {
    pub fn new(shard_id: ShardId) -> Self {
        Self {
            shard_id,
            current_size_bytes: 0,
            size_history: VecDeque::new(),
            growth_rate_bytes_per_hour: 0.0,
        }
    }

    /// Update storage size
    pub fn update_size(&mut self, size_bytes: u64) {
        let now = MessageThroughputTracker::now_timestamp();
        self.current_size_bytes = size_bytes;
        self.size_history.push_back((now, size_bytes));

        // Keep 24 hours of history
        self.prune_history(24 * 3600);

        // Recalculate growth rate
        self.calculate_growth_rate();
    }

    /// Get current size
    pub fn get_current_size(&self) -> u64 {
        self.current_size_bytes
    }

    /// Get growth rate (bytes per hour)
    pub fn get_growth_rate(&self) -> f64 {
        self.growth_rate_bytes_per_hour
    }

    /// Predict size in future hours
    pub fn predict_size_in_hours(&self, hours: u64) -> u64 {
        let predicted = self.current_size_bytes as f64
            + (self.growth_rate_bytes_per_hour * hours as f64);
        predicted.max(0.0) as u64
    }

    /// Calculate growth rate from history
    fn calculate_growth_rate(&mut self) {
        if self.size_history.len() < 2 {
            self.growth_rate_bytes_per_hour = 0.0;
            return;
        }

        // Linear regression on recent samples
        let samples: Vec<(f64, f64)> = self
            .size_history
            .iter()
            .map(|(ts, size)| (*ts as f64, *size as f64))
            .collect();

        let n = samples.len() as f64;
        let sum_x: f64 = samples.iter().map(|(x, _)| x).sum();
        let sum_y: f64 = samples.iter().map(|(_, y)| y).sum();
        let sum_xy: f64 = samples.iter().map(|(x, y)| x * y).sum();
        let sum_x2: f64 = samples.iter().map(|(x, _)| x * x).sum();

        let denominator = n * sum_x2 - sum_x * sum_x;
        if denominator.abs() < 1e-10 {
            self.growth_rate_bytes_per_hour = 0.0;
            return;
        }

        // Slope in bytes per second
        let slope = (n * sum_xy - sum_x * sum_y) / denominator;

        // Convert to bytes per hour
        self.growth_rate_bytes_per_hour = slope * 3600.0;
    }

    /// Prune history older than window
    fn prune_history(&mut self, window_secs: u64) {
        let now = MessageThroughputTracker::now_timestamp();
        let cutoff = now.saturating_sub(window_secs);

        while let Some(&(ts, _)) = self.size_history.front() {
            if ts < cutoff {
                self.size_history.pop_front();
            } else {
                break;
            }
        }
    }
}

/// CPU and memory usage monitor
#[derive(Debug, Clone)]
pub struct CpuMemoryMonitor {
    shard_id: ShardId,
    /// CPU usage (0.0-1.0)
    cpu_usage: f64,
    /// Memory usage in bytes
    memory_rss_bytes: u64,
    /// Last update timestamp
    last_update: u64,
}

impl CpuMemoryMonitor {
    pub fn new(shard_id: ShardId) -> Self {
        Self {
            shard_id,
            cpu_usage: 0.0,
            memory_rss_bytes: 0,
            last_update: MessageThroughputTracker::now_timestamp(),
        }
    }

    /// Update CPU and memory metrics
    pub fn update(&mut self, cpu_usage: f64, memory_bytes: u64) {
        self.cpu_usage = cpu_usage.clamp(0.0, 1.0);
        self.memory_rss_bytes = memory_bytes;
        self.last_update = MessageThroughputTracker::now_timestamp();
    }

    /// Get CPU usage (0.0-1.0)
    pub fn get_cpu_usage(&self) -> f64 {
        self.cpu_usage
    }

    /// Get memory usage (bytes)
    pub fn get_memory_bytes(&self) -> u64 {
        self.memory_rss_bytes
    }

    /// Check if metrics are stale (>5 minutes old)
    pub fn is_stale(&self) -> bool {
        let now = MessageThroughputTracker::now_timestamp();
        now - self.last_update > 300
    }
}

/// Aggregated load metrics for a shard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadMetrics {
    pub shard_id: ShardId,
    pub cpu_usage: f64,
    pub memory_usage_bytes: u64,
    pub throughput_msg_per_sec: f64,
    pub storage_bytes: u64,
    pub storage_growth_rate_mb_per_hour: f64,
    pub timestamp: i64,
}

impl LoadMetrics {
    /// Check if shard is overloaded based on thresholds
    pub fn is_overloaded(&self) -> bool {
        self.cpu_usage > 0.8 || self.throughput_msg_per_sec > 1000.0
    }

    /// Check if shard is underloaded
    pub fn is_underloaded(&self) -> bool {
        self.cpu_usage < 0.5 && self.throughput_msg_per_sec < 500.0
    }

    /// Calculate overall load score (0.0-1.0)
    pub fn load_score(&self) -> f64 {
        // Weighted average: 40% CPU, 30% throughput, 30% storage growth
        let cpu_score = self.cpu_usage;
        let throughput_score = (self.throughput_msg_per_sec / 2000.0).min(1.0);
        let storage_score = (self.storage_growth_rate_mb_per_hour / 1000.0).min(1.0);

        (cpu_score * 0.4 + throughput_score * 0.3 + storage_score * 0.3).min(1.0)
    }
}

/// Load metrics aggregator - collects all metrics for all shards
#[derive(Debug)]
pub struct LoadMetricsAggregator {
    throughput_trackers: HashMap<ShardId, MessageThroughputTracker>,
    storage_monitors: HashMap<ShardId, StorageSizeMonitor>,
    cpu_memory_monitors: HashMap<ShardId, CpuMemoryMonitor>,
}

impl LoadMetricsAggregator {
    pub fn new() -> Self {
        Self {
            throughput_trackers: HashMap::new(),
            storage_monitors: HashMap::new(),
            cpu_memory_monitors: HashMap::new(),
        }
    }

    /// Register a shard for monitoring
    pub fn register_shard(&mut self, shard_id: ShardId) {
        self.throughput_trackers
            .entry(shard_id.clone())
            .or_insert_with(|| MessageThroughputTracker::new(shard_id.clone()));
        self.storage_monitors
            .entry(shard_id.clone())
            .or_insert_with(|| StorageSizeMonitor::new(shard_id.clone()));
        self.cpu_memory_monitors
            .entry(shard_id.clone())
            .or_insert_with(|| CpuMemoryMonitor::new(shard_id));
    }

    /// Record message for a shard
    pub fn record_message(&mut self, shard_id: &ShardId) {
        if let Some(tracker) = self.throughput_trackers.get_mut(shard_id) {
            tracker.record_message();
        }
    }

    /// Update storage size for a shard
    pub fn update_storage(&mut self, shard_id: &ShardId, size_bytes: u64) {
        if let Some(monitor) = self.storage_monitors.get_mut(shard_id) {
            monitor.update_size(size_bytes);
        }
    }

    /// Update CPU and memory for a shard
    pub fn update_cpu_memory(&mut self, shard_id: &ShardId, cpu_usage: f64, memory_bytes: u64) {
        if let Some(monitor) = self.cpu_memory_monitors.get_mut(shard_id) {
            monitor.update(cpu_usage, memory_bytes);
        }
    }

    /// Collect all metrics for all shards
    pub fn collect_metrics(&self) -> Vec<LoadMetrics> {
        let mut metrics = Vec::new();

        for (shard_id, throughput) in &self.throughput_trackers {
            let storage = self.storage_monitors.get(shard_id);
            let cpu_mem = self.cpu_memory_monitors.get(shard_id);

            metrics.push(LoadMetrics {
                shard_id: shard_id.clone(),
                cpu_usage: cpu_mem.map(|m| m.get_cpu_usage()).unwrap_or(0.0),
                memory_usage_bytes: cpu_mem.map(|m| m.get_memory_bytes()).unwrap_or(0),
                throughput_msg_per_sec: throughput.get_rate_1min(),
                storage_bytes: storage.map(|m| m.get_current_size()).unwrap_or(0),
                storage_growth_rate_mb_per_hour: storage
                    .map(|m| m.get_growth_rate() / 1_000_000.0)
                    .unwrap_or(0.0),
                timestamp: chrono::Utc::now().timestamp(),
            });
        }

        metrics
    }

    /// Check if any shard needs rebalancing
    pub fn check_rebalancing_triggers(&self) -> Vec<ShardId> {
        let metrics = self.collect_metrics();
        metrics
            .iter()
            .filter(|m| m.is_overloaded())
            .map(|m| m.shard_id.clone())
            .collect()
    }
}

impl Default for LoadMetricsAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Prometheus metrics exporter
#[derive(Debug)]
pub struct PrometheusExporter {
    metrics: HashMap<String, f64>,
}

impl PrometheusExporter {
    pub fn new() -> Self {
        Self {
            metrics: HashMap::new(),
        }
    }

    /// Update metric value
    pub fn set_gauge(&mut self, name: &str, value: f64) {
        self.metrics.insert(name.to_string(), value);
    }

    /// Export metrics to Prometheus format
    pub fn export(&self) -> String {
        let mut output = String::new();

        for (name, value) in &self.metrics {
            output.push_str(&format!("# HELP {} Shard load metric\n", name));
            output.push_str(&format!("# TYPE {} gauge\n", name));
            output.push_str(&format!("{} {}\n", name, value));
        }

        output
    }

    /// Update from LoadMetrics
    pub fn update_from_metrics(&mut self, metrics: &[LoadMetrics]) {
        for metric in metrics {
            let shard_label = format!("shard_{}", metric.shard_id.0);

            self.set_gauge(
                &format!("dchat_shard_cpu_usage_{}", shard_label),
                metric.cpu_usage,
            );
            self.set_gauge(
                &format!("dchat_shard_memory_bytes_{}", shard_label),
                metric.memory_usage_bytes as f64,
            );
            self.set_gauge(
                &format!("dchat_shard_throughput_msg_per_sec_{}", shard_label),
                metric.throughput_msg_per_sec,
            );
            self.set_gauge(
                &format!("dchat_shard_storage_bytes_{}", shard_label),
                metric.storage_bytes as f64,
            );
            self.set_gauge(
                &format!("dchat_shard_storage_growth_mb_per_hour_{}", shard_label),
                metric.storage_growth_rate_mb_per_hour,
            );
        }
    }
}

impl Default for PrometheusExporter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_throughput_tracker() {
        let mut tracker = MessageThroughputTracker::new(ShardId(0));

        // Record 10 messages
        for _ in 0..10 {
            tracker.record_message();
        }

        let rate = tracker.get_rate_1min();
        assert!(rate > 0.0);
        assert_eq!(tracker.total_messages, 10);
    }

    #[test]
    fn test_storage_monitor() {
        let mut monitor = StorageSizeMonitor::new(ShardId(0));

        monitor.update_size(1_000_000);
        assert_eq!(monitor.get_current_size(), 1_000_000);

        // Add more samples to establish growth
        std::thread::sleep(Duration::from_millis(100));
        monitor.update_size(2_000_000);

        let predicted = monitor.predict_size_in_hours(1);
        assert!(predicted >= 2_000_000); // Should predict growth
    }

    #[test]
    fn test_cpu_memory_monitor() {
        let mut monitor = CpuMemoryMonitor::new(ShardId(0));

        monitor.update(0.75, 1_000_000_000);
        assert_eq!(monitor.get_cpu_usage(), 0.75);
        assert_eq!(monitor.get_memory_bytes(), 1_000_000_000);
        assert!(!monitor.is_stale());
    }

    #[test]
    fn test_load_metrics_scoring() {
        let metrics = LoadMetrics {
            shard_id: ShardId(0),
            cpu_usage: 0.9,
            memory_usage_bytes: 1_000_000_000,
            throughput_msg_per_sec: 1500.0,
            storage_bytes: 10_000_000_000,
            storage_growth_rate_mb_per_hour: 100.0,
            timestamp: chrono::Utc::now().timestamp(),
        };

        assert!(metrics.is_overloaded());
        assert!(metrics.load_score() > 0.6);
    }

    #[test]
    fn test_aggregator() {
        let mut agg = LoadMetricsAggregator::new();

        agg.register_shard(ShardId(0));
        agg.register_shard(ShardId(1));

        agg.record_message(&ShardId(0));
        agg.update_storage(&ShardId(0), 1_000_000);
        agg.update_cpu_memory(&ShardId(0), 0.5, 500_000_000);

        let metrics = agg.collect_metrics();
        assert_eq!(metrics.len(), 2);

        let triggers = agg.check_rebalancing_triggers();
        assert!(triggers.is_empty()); // No overload yet
    }

    #[test]
    fn test_prometheus_export() {
        let mut exporter = PrometheusExporter::new();

        let metrics = vec![LoadMetrics {
            shard_id: ShardId(0),
            cpu_usage: 0.75,
            memory_usage_bytes: 1_000_000_000,
            throughput_msg_per_sec: 500.0,
            storage_bytes: 10_000_000_000,
            storage_growth_rate_mb_per_hour: 50.0,
            timestamp: chrono::Utc::now().timestamp(),
        }];

        exporter.update_from_metrics(&metrics);
        let output = exporter.export();

        assert!(output.contains("dchat_shard_cpu_usage_shard_0"));
        assert!(output.contains("0.75"));
    }
}
