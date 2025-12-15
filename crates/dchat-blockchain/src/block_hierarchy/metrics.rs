//! Block Hierarchy Metrics
//!
//! Comprehensive metrics for monitoring the block→subblock→miniblock pipeline
//! including bytes per level, verification counts, time-to-certificate,
//! DA sampling success, and fraud proof tracking.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

// ─────────────────────────────────────────────────────────────────────────────
// Counters and Gauges
// ─────────────────────────────────────────────────────────────────────────────

/// Atomic counter with labels
#[derive(Default)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    pub fn reset(&self) -> u64 {
        self.value.swap(0, Ordering::Relaxed)
    }
}

/// Gauge (can go up or down)
#[derive(Default)]
pub struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, v: u64) {
        self.value.store(v, Ordering::Relaxed);
    }

    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// Histogram for latency tracking
pub struct Histogram {
    /// Bucket boundaries (in microseconds)
    buckets: Vec<u64>,
    /// Counts per bucket
    counts: Vec<AtomicU64>,
    /// Sum of all values
    sum: AtomicU64,
    /// Total count
    count: AtomicU64,
}

impl Histogram {
    pub fn new(buckets: Vec<u64>) -> Self {
        let counts = buckets.iter().map(|_| AtomicU64::new(0)).collect();
        Self {
            buckets,
            counts,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    pub fn with_default_buckets() -> Self {
        // Buckets in microseconds: 100us, 1ms, 10ms, 50ms, 100ms, 500ms, 1s, 5s, 10s
        Self::new(vec![
            100, 1_000, 10_000, 50_000, 100_000, 500_000, 1_000_000, 5_000_000, 10_000_000,
        ])
    }

    pub fn observe(&self, value_us: u64) {
        self.sum.fetch_add(value_us, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);

        for (i, &boundary) in self.buckets.iter().enumerate() {
            if value_us <= boundary {
                self.counts[i].fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }

    pub fn observe_duration(&self, duration: Duration) {
        self.observe(duration.as_micros() as u64);
    }

    pub fn mean(&self) -> f64 {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        self.sum.load(Ordering::Relaxed) as f64 / count as f64
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get percentile (approximate)
    pub fn percentile(&self, p: f64) -> u64 {
        let total = self.count.load(Ordering::Relaxed);
        if total == 0 {
            return 0;
        }

        let target = (total as f64 * p) as u64;
        let mut cumulative = 0u64;

        for (i, count) in self.counts.iter().enumerate() {
            cumulative += count.load(Ordering::Relaxed);
            if cumulative >= target {
                return self.buckets[i];
            }
        }

        *self.buckets.last().unwrap_or(&0)
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::with_default_buckets()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Block Hierarchy Metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Comprehensive metrics for block hierarchy
pub struct BlockHierarchyMetrics {
    // ── Block Level ──
    /// Blocks produced
    pub blocks_produced: Counter,
    /// Blocks verified
    pub blocks_verified: Counter,
    /// Block bytes (total)
    pub block_bytes_total: Counter,
    /// Current block height
    pub block_height: Gauge,
    /// Time to block certificate
    pub block_certificate_time: Histogram,
    /// Block verification time
    pub block_verify_time: Histogram,

    // ── Subblock Level ──
    /// Subblocks produced
    pub subblocks_produced: Counter,
    /// Subblocks verified
    pub subblocks_verified: Counter,
    /// Subblock bytes (total)
    pub subblock_bytes_total: Counter,
    /// Subblocks per block
    pub subblocks_per_block: Histogram,
    /// Time to subblock certificate
    pub subblock_certificate_time: Histogram,

    // ── Miniblock Level ──
    /// Miniblocks produced
    pub miniblocks_produced: Counter,
    /// Miniblocks verified
    pub miniblocks_verified: Counter,
    /// Miniblock bytes (total)
    pub miniblock_bytes_total: Counter,
    /// Miniblocks per subblock
    pub miniblocks_per_subblock: Histogram,
    /// Transactions per miniblock
    pub txs_per_miniblock: Histogram,
    /// Miniblock body size bytes
    pub miniblock_body_bytes: Histogram,

    // ── Lane Sharding ──
    /// Transactions per lane
    pub txs_per_lane: Vec<Counter>,
    /// Lane execution time
    pub lane_execution_time: Histogram,

    // ── Signature Aggregation ──
    /// Signatures aggregated
    pub signatures_aggregated: Counter,
    /// Signature bytes saved (vs individual)
    pub signature_bytes_saved: Counter,
    /// Aggregate verification time
    pub aggregate_verify_time: Histogram,

    // ── DA Sampling ──
    /// DA samples requested
    pub da_samples_requested: Counter,
    /// DA samples successful
    pub da_samples_successful: Counter,
    /// DA sample latency
    pub da_sample_latency: Histogram,
    /// DA availability percentage
    pub da_availability_percent: Gauge,

    // ── Erasure Coding ──
    /// Erasure encodes performed
    pub erasure_encodes: Counter,
    /// Erasure reconstructions
    pub erasure_reconstructions: Counter,
    /// Shards transmitted
    pub shards_transmitted: Counter,
    /// Reconstruction time
    pub reconstruction_time: Histogram,

    // ── Fraud Proofs ──
    /// Fraud proofs created
    pub fraud_proofs_created: Counter,
    /// Fraud proofs verified
    pub fraud_proofs_verified: Counter,
    /// Fraud proofs by type
    pub fraud_proofs_by_type: RwLock<HashMap<String, u64>>,
    /// Invalid blocks detected
    pub invalid_blocks_detected: Counter,

    // ── Execution ──
    /// Transactions executed
    pub txs_executed: Counter,
    /// Successful transactions
    pub txs_successful: Counter,
    /// Failed transactions
    pub txs_failed: Counter,
    /// Gas used total
    pub gas_used_total: Counter,
    /// Execution time per tx
    pub tx_execution_time: Histogram,
    /// State root computation time
    pub state_root_time: Histogram,
}

impl BlockHierarchyMetrics {
    /// Create new metrics instance
    pub fn new() -> Self {
        Self {
            // Block
            blocks_produced: Counter::new(),
            blocks_verified: Counter::new(),
            block_bytes_total: Counter::new(),
            block_height: Gauge::new(),
            block_certificate_time: Histogram::default(),
            block_verify_time: Histogram::default(),

            // Subblock
            subblocks_produced: Counter::new(),
            subblocks_verified: Counter::new(),
            subblock_bytes_total: Counter::new(),
            subblocks_per_block: Histogram::new(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            subblock_certificate_time: Histogram::default(),

            // Miniblock
            miniblocks_produced: Counter::new(),
            miniblocks_verified: Counter::new(),
            miniblock_bytes_total: Counter::new(),
            miniblocks_per_subblock: Histogram::new(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            txs_per_miniblock: Histogram::new(vec![1, 10, 50, 100, 200, 300, 400, 500]),
            miniblock_body_bytes: Histogram::new(vec![
                1024, 10_240, 51_200, 102_400, 256_000, 512_000,
            ]),

            // Lane
            txs_per_lane: (0..256).map(|_| Counter::new()).collect(),
            lane_execution_time: Histogram::default(),

            // Signatures
            signatures_aggregated: Counter::new(),
            signature_bytes_saved: Counter::new(),
            aggregate_verify_time: Histogram::default(),

            // DA
            da_samples_requested: Counter::new(),
            da_samples_successful: Counter::new(),
            da_sample_latency: Histogram::default(),
            da_availability_percent: Gauge::new(),

            // Erasure
            erasure_encodes: Counter::new(),
            erasure_reconstructions: Counter::new(),
            shards_transmitted: Counter::new(),
            reconstruction_time: Histogram::default(),

            // Fraud
            fraud_proofs_created: Counter::new(),
            fraud_proofs_verified: Counter::new(),
            fraud_proofs_by_type: RwLock::new(HashMap::new()),
            invalid_blocks_detected: Counter::new(),

            // Execution
            txs_executed: Counter::new(),
            txs_successful: Counter::new(),
            txs_failed: Counter::new(),
            gas_used_total: Counter::new(),
            tx_execution_time: Histogram::default(),
            state_root_time: Histogram::default(),
        }
    }

    // ── Recording Helpers ──

    /// Record block produced
    pub fn record_block_produced(&self, height: u64, bytes: u64, certificate_time: Duration) {
        self.blocks_produced.inc();
        self.block_bytes_total.inc_by(bytes);
        self.block_height.set(height);
        self.block_certificate_time
            .observe_duration(certificate_time);
    }

    /// Record block verified
    pub fn record_block_verified(&self, verify_time: Duration) {
        self.blocks_verified.inc();
        self.block_verify_time.observe_duration(verify_time);
    }

    /// Record subblock produced
    pub fn record_subblock_produced(
        &self,
        bytes: u64,
        miniblock_count: u64,
        certificate_time: Duration,
    ) {
        self.subblocks_produced.inc();
        self.subblock_bytes_total.inc_by(bytes);
        self.miniblocks_per_subblock.observe(miniblock_count);
        self.subblock_certificate_time
            .observe_duration(certificate_time);
    }

    /// Record miniblock produced
    pub fn record_miniblock_produced(&self, tx_count: u64, body_bytes: u64) {
        self.miniblocks_produced.inc();
        self.miniblock_bytes_total.inc_by(body_bytes);
        self.txs_per_miniblock.observe(tx_count);
        self.miniblock_body_bytes.observe(body_bytes);
    }

    /// Record lane transaction
    pub fn record_lane_tx(&self, lane_id: u8) {
        if (lane_id as usize) < self.txs_per_lane.len() {
            self.txs_per_lane[lane_id as usize].inc();
        }
    }

    /// Record signature aggregation
    pub fn record_signature_aggregation(&self, sig_count: u64, bytes_saved: u64) {
        self.signatures_aggregated.inc_by(sig_count);
        self.signature_bytes_saved.inc_by(bytes_saved);
    }

    /// Record DA sample
    pub fn record_da_sample(&self, success: bool, latency: Duration) {
        self.da_samples_requested.inc();
        if success {
            self.da_samples_successful.inc();
        }
        self.da_sample_latency.observe_duration(latency);

        // Update availability percentage
        let total = self.da_samples_requested.get();
        let successful = self.da_samples_successful.get();
        if total > 0 {
            self.da_availability_percent.set((successful * 100) / total);
        }
    }

    /// Record erasure encode
    pub fn record_erasure_encode(&self, shard_count: u64) {
        self.erasure_encodes.inc();
        self.shards_transmitted.inc_by(shard_count);
    }

    /// Record erasure reconstruction
    pub fn record_erasure_reconstruction(&self, recon_time: Duration) {
        self.erasure_reconstructions.inc();
        self.reconstruction_time.observe_duration(recon_time);
    }

    /// Record fraud proof
    pub fn record_fraud_proof(&self, fraud_type: &str) {
        self.fraud_proofs_created.inc();
        let mut by_type = self.fraud_proofs_by_type.write().unwrap();
        *by_type.entry(fraud_type.to_string()).or_insert(0) += 1;
    }

    /// Record transaction execution
    pub fn record_tx_execution(&self, success: bool, gas_used: u64, exec_time: Duration) {
        self.txs_executed.inc();
        if success {
            self.txs_successful.inc();
        } else {
            self.txs_failed.inc();
        }
        self.gas_used_total.inc_by(gas_used);
        self.tx_execution_time.observe_duration(exec_time);
    }

    // ── Export Functions ──

    /// Export as Prometheus text format
    pub fn export_prometheus(&self) -> String {
        let mut out = String::new();

        // Block metrics
        out.push_str(&format!(
            "# HELP dchat_blocks_produced_total Total blocks produced\n\
             # TYPE dchat_blocks_produced_total counter\n\
             dchat_blocks_produced_total {}\n\n",
            self.blocks_produced.get()
        ));

        out.push_str(&format!(
            "# HELP dchat_block_height Current block height\n\
             # TYPE dchat_block_height gauge\n\
             dchat_block_height {}\n\n",
            self.block_height.get()
        ));

        out.push_str(&format!(
            "# HELP dchat_block_bytes_total Total block bytes\n\
             # TYPE dchat_block_bytes_total counter\n\
             dchat_block_bytes_total {}\n\n",
            self.block_bytes_total.get()
        ));

        // Subblock metrics
        out.push_str(&format!(
            "# HELP dchat_subblocks_produced_total Total subblocks produced\n\
             # TYPE dchat_subblocks_produced_total counter\n\
             dchat_subblocks_produced_total {}\n\n",
            self.subblocks_produced.get()
        ));

        // Miniblock metrics
        out.push_str(&format!(
            "# HELP dchat_miniblocks_produced_total Total miniblocks produced\n\
             # TYPE dchat_miniblocks_produced_total counter\n\
             dchat_miniblocks_produced_total {}\n\n",
            self.miniblocks_produced.get()
        ));

        // Transactions
        out.push_str(&format!(
            "# HELP dchat_txs_executed_total Total transactions executed\n\
             # TYPE dchat_txs_executed_total counter\n\
             dchat_txs_executed_total {}\n\n",
            self.txs_executed.get()
        ));

        out.push_str(&format!(
            "# HELP dchat_txs_successful_total Successful transactions\n\
             # TYPE dchat_txs_successful_total counter\n\
             dchat_txs_successful_total {}\n\n",
            self.txs_successful.get()
        ));

        // DA metrics
        out.push_str(&format!(
            "# HELP dchat_da_availability_percent DA availability percentage\n\
             # TYPE dchat_da_availability_percent gauge\n\
             dchat_da_availability_percent {}\n\n",
            self.da_availability_percent.get()
        ));

        // Fraud proofs
        out.push_str(&format!(
            "# HELP dchat_fraud_proofs_created_total Total fraud proofs created\n\
             # TYPE dchat_fraud_proofs_created_total counter\n\
             dchat_fraud_proofs_created_total {}\n\n",
            self.fraud_proofs_created.get()
        ));

        // Latency histograms (p50, p95, p99)
        out.push_str(&format!(
            "# HELP dchat_block_certificate_time_us Block certificate time microseconds\n\
             # TYPE dchat_block_certificate_time_us summary\n\
             dchat_block_certificate_time_us{{quantile=\"0.5\"}} {}\n\
             dchat_block_certificate_time_us{{quantile=\"0.95\"}} {}\n\
             dchat_block_certificate_time_us{{quantile=\"0.99\"}} {}\n\n",
            self.block_certificate_time.percentile(0.5),
            self.block_certificate_time.percentile(0.95),
            self.block_certificate_time.percentile(0.99),
        ));

        out
    }

    /// Export as JSON
    pub fn export_json(&self) -> String {
        serde_json::json!({
            "blocks": {
                "produced": self.blocks_produced.get(),
                "verified": self.blocks_verified.get(),
                "bytes_total": self.block_bytes_total.get(),
                "height": self.block_height.get(),
                "certificate_time_mean_us": self.block_certificate_time.mean(),
            },
            "subblocks": {
                "produced": self.subblocks_produced.get(),
                "verified": self.subblocks_verified.get(),
                "bytes_total": self.subblock_bytes_total.get(),
            },
            "miniblocks": {
                "produced": self.miniblocks_produced.get(),
                "verified": self.miniblocks_verified.get(),
                "bytes_total": self.miniblock_bytes_total.get(),
            },
            "transactions": {
                "executed": self.txs_executed.get(),
                "successful": self.txs_successful.get(),
                "failed": self.txs_failed.get(),
                "gas_used_total": self.gas_used_total.get(),
            },
            "signatures": {
                "aggregated": self.signatures_aggregated.get(),
                "bytes_saved": self.signature_bytes_saved.get(),
            },
            "da_sampling": {
                "requested": self.da_samples_requested.get(),
                "successful": self.da_samples_successful.get(),
                "availability_percent": self.da_availability_percent.get(),
            },
            "erasure": {
                "encodes": self.erasure_encodes.get(),
                "reconstructions": self.erasure_reconstructions.get(),
                "shards_transmitted": self.shards_transmitted.get(),
            },
            "fraud_proofs": {
                "created": self.fraud_proofs_created.get(),
                "verified": self.fraud_proofs_verified.get(),
                "invalid_blocks": self.invalid_blocks_detected.get(),
            },
        })
        .to_string()
    }

    /// Get summary stats
    pub fn summary(&self) -> MetricsSummary {
        MetricsSummary {
            blocks_produced: self.blocks_produced.get(),
            block_height: self.block_height.get(),
            subblocks_produced: self.subblocks_produced.get(),
            miniblocks_produced: self.miniblocks_produced.get(),
            txs_executed: self.txs_executed.get(),
            txs_success_rate: if self.txs_executed.get() > 0 {
                (self.txs_successful.get() as f64 / self.txs_executed.get() as f64) * 100.0
            } else {
                0.0
            },
            da_availability: self.da_availability_percent.get() as f64,
            fraud_proofs: self.fraud_proofs_created.get(),
            avg_certificate_time_ms: self.block_certificate_time.mean() / 1000.0,
        }
    }
}

impl Default for BlockHierarchyMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub blocks_produced: u64,
    pub block_height: u64,
    pub subblocks_produced: u64,
    pub miniblocks_produced: u64,
    pub txs_executed: u64,
    pub txs_success_rate: f64,
    pub da_availability: f64,
    pub fraud_proofs: u64,
    pub avg_certificate_time_ms: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Timer Helper
// ─────────────────────────────────────────────────────────────────────────────

/// RAII timer for measuring durations
pub struct Timer {
    start: Instant,
    histogram: Option<*const Histogram>,
}

impl Timer {
    pub fn start(histogram: &Histogram) -> Self {
        Self {
            start: Instant::now(),
            histogram: Some(histogram as *const _),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    pub fn stop(self) -> Duration {
        let elapsed = self.start.elapsed();
        if let Some(h) = self.histogram {
            // Safety: pointer valid within scope
            unsafe { (*h).observe_duration(elapsed) };
        }
        elapsed
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Global Metrics Instance
// ─────────────────────────────────────────────────────────────────────────────

use once_cell::sync::Lazy;

/// Global metrics instance
pub static METRICS: Lazy<BlockHierarchyMetrics> = Lazy::new(BlockHierarchyMetrics::new);

/// Get global metrics reference
pub fn metrics() -> &'static BlockHierarchyMetrics {
    &METRICS
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);

        counter.inc();
        assert_eq!(counter.get(), 1);

        counter.inc_by(10);
        assert_eq!(counter.get(), 11);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new();
        assert_eq!(gauge.get(), 0);

        gauge.set(100);
        assert_eq!(gauge.get(), 100);

        gauge.inc();
        assert_eq!(gauge.get(), 101);

        gauge.dec();
        assert_eq!(gauge.get(), 100);
    }

    #[test]
    fn test_histogram() {
        let hist = Histogram::with_default_buckets();

        hist.observe(100);
        hist.observe(1000);
        hist.observe(10_000);

        assert_eq!(hist.count(), 3);
        assert!(hist.mean() > 0.0);
    }

    #[test]
    fn test_metrics_recording() {
        let metrics = BlockHierarchyMetrics::new();

        metrics.record_block_produced(1, 1000, Duration::from_millis(50));
        assert_eq!(metrics.blocks_produced.get(), 1);
        assert_eq!(metrics.block_height.get(), 1);

        metrics.record_miniblock_produced(100, 50000);
        assert_eq!(metrics.miniblocks_produced.get(), 1);

        metrics.record_tx_execution(true, 21000, Duration::from_micros(100));
        assert_eq!(metrics.txs_executed.get(), 1);
        assert_eq!(metrics.txs_successful.get(), 1);
    }

    #[test]
    fn test_prometheus_export() {
        let metrics = BlockHierarchyMetrics::new();
        metrics.record_block_produced(1, 1000, Duration::from_millis(50));

        let output = metrics.export_prometheus();
        assert!(output.contains("dchat_blocks_produced_total"));
        assert!(output.contains("dchat_block_height"));
    }

    #[test]
    fn test_json_export() {
        let metrics = BlockHierarchyMetrics::new();
        metrics.record_block_produced(1, 1000, Duration::from_millis(50));

        let output = metrics.export_json();
        assert!(output.contains("\"produced\":1"));
    }
}
