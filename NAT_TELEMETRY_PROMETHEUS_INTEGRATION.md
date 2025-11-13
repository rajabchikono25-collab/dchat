# NAT Telemetry Prometheus Integration

## Overview
This document describes the integration between `dchat-network`'s NAT telemetry system and `dchat-observability`'s Prometheus metrics exporter, implemented to resolve PROBUS audit item #2 (MEDIUM priority).

## Architecture

### Problem: Circular Dependency
- `dchat-network` contains NAT traversal telemetry
- `dchat-observability` contains Prometheus metrics infrastructure
- `dchat-observability` already depends on `dchat-network` (for network health checks)
- Cannot add `dchat-observability` as dependency of `dchat-network` (circular dependency)

### Solution: Trait-Based Dependency Injection
We implemented a trait-based approach that allows `dchat-network` to export metrics without knowing about `dchat-observability`:

```rust
// In dchat-network/src/network/nat_telemetry.rs
pub trait NatMetricsExporter: Send + Sync {
    fn record_nat_attempt(&self, method: &str, result: &str);
    fn set_nat_success_rate(&self, method: &str, rate: f64);
}

pub fn set_metrics_exporter(exporter: Arc<dyn NatMetricsExporter>);
```

`dchat-observability` implements this trait for `PrometheusExporter` when the `nat-telemetry-integration` feature is enabled.

## Components Modified

### 1. dchat-network (crates/dchat-network/)

**File**: `Cargo.toml`
- Added optional `prometheus` dependency (version 0.13)
- Added `metrics` feature flag (currently unused, reserved for future use)

**File**: `src/network/nat_telemetry.rs`
- Added `NatMetricsExporter` trait for external metrics integration
- Added `set_metrics_exporter()` function to register external exporter
- Added `get_metrics_exporter()` private function to retrieve registered exporter
- Modified `record_attempt()` method to call exporter if configured

### 2. dchat-observability (crates/dchat-observability/)

**File**: `Cargo.toml`
- Added `nat-telemetry-integration` feature flag

**File**: `src/observability/metrics.rs`
- Implemented `NatMetricsExporter` trait for `PrometheusExporter` (conditional on feature)
- Modified `initialize_prometheus()` to automatically register with NAT telemetry when feature is enabled

## Usage

### At Application Startup
```rust
use dchat_observability::observability::metrics;

// Initialize Prometheus (with nat-telemetry-integration feature enabled)
metrics::initialize_prometheus()?;

// NAT telemetry will now automatically export metrics to Prometheus
```

### Metrics Exported
The following Prometheus metrics are exported when NAT traversal attempts occur:

1. **`dchat_nat_traversal_attempts_total`** (Counter)
   - Labels: `method`, `result`
   - Methods: `upnp`, `direct`, `hole_punch`, `turn`
   - Results: `success`, `failure`, `timeout`

2. **`dchat_nat_method_success_rate`** (Gauge)
   - Labels: `method`
   - Value: Success rate from 0.0 to 1.0
   - Updated after each attempt

### Example Prometheus Queries
```promql
# Total NAT traversal attempts by method
sum(dchat_nat_traversal_attempts_total) by (method)

# Success rate by method
dchat_nat_method_success_rate

# Success rate percentage
dchat_nat_method_success_rate * 100

# Failed attempts in last hour
rate(dchat_nat_traversal_attempts_total{result="failure"}[1h])

# TURN relay usage (fallback indicator)
rate(dchat_nat_traversal_attempts_total{method="turn"}[5m])
```

## Feature Flags

### dchat-network
- `metrics` (default: disabled): Reserved for future direct Prometheus integration if needed

### dchat-observability
- `nat-telemetry-integration` (default: disabled): Enables automatic NAT telemetry integration
  - When enabled: PrometheusExporter implements NatMetricsExporter
  - Automatically registers with dchat-network during `initialize_prometheus()`

## Building with Integration

### Enable NAT telemetry integration:
```bash
cargo build -p dchat-observability --features nat-telemetry-integration
```

### For production deployments:
Add to root `Cargo.toml` or application `Cargo.toml`:
```toml
[dependencies]
dchat-observability = { path = "crates/dchat-observability", features = ["nat-telemetry-integration"] }
```

## Testing

### Unit Tests
NAT telemetry tests remain in `dchat-network/src/network/nat_telemetry.rs` (existing tests still pass)

Prometheus integration tests are in `dchat-observability/src/observability/metrics.rs`:
```rust
#[test]
fn test_nat_metrics() {
    let exporter = PrometheusExporter::new().unwrap();
    exporter.record_nat_attempt("upnp", "success");
    exporter.set_nat_success_rate("hole_punch", 0.75);
    let output = exporter.export().unwrap();
    assert!(output.contains("dchat_nat_traversal_attempts_total"));
}
```

### Integration Testing
```rust
use dchat_network::network::nat_telemetry::{NatTelemetry, NatMethod, NatResult};
use dchat_observability::observability::metrics;
use std::time::Duration;

// Initialize Prometheus
metrics::initialize_prometheus().unwrap();

// Get NAT telemetry instance
let telemetry = NatTelemetry::new();

// Record attempts - metrics are automatically exported
telemetry.record_attempt(
    NatMethod::Upnp,
    NatResult::Success,
    Duration::from_millis(125)
).await.unwrap();

// Verify metrics
let exporter = metrics::get_prometheus().unwrap();
let output = exporter.export().unwrap();
assert!(output.contains("dchat_nat_traversal_attempts_total"));
```

## Benefits

1. **Zero Overhead**: When `nat-telemetry-integration` feature is disabled, no runtime cost
2. **No Circular Dependencies**: Trait-based injection avoids compile-time circular dependencies
3. **Automatic Registration**: Prometheus initialization automatically sets up NAT telemetry integration
4. **Production Ready**: Metrics are immediately available in Prometheus/Grafana dashboards
5. **Extensible**: Other metrics exporters can implement `NatMetricsExporter` trait

## Production Observability

### Recommended Grafana Dashboard Panels

**Panel 1: NAT Method Success Rate**
```promql
dchat_nat_method_success_rate * 100
```
Visualization: Gauge (0-100%)

**Panel 2: NAT Traversal Attempts Rate**
```promql
rate(dchat_nat_traversal_attempts_total[5m])
```
Visualization: Graph (attempts/second)

**Panel 3: TURN Fallback Usage**
```promql
rate(dchat_nat_traversal_attempts_total{method="turn"}[5m]) /
rate(dchat_nat_traversal_attempts_total[5m])
```
Visualization: Graph (percentage)

**Panel 4: Failure Rate by Method**
```promql
rate(dchat_nat_traversal_attempts_total{result="failure"}[5m]) /
rate(dchat_nat_traversal_attempts_total[5m])
```
Visualization: Heatmap

### Alerting Rules

**High Failure Rate**
```yaml
- alert: HighNATFailureRate
  expr: dchat_nat_method_success_rate < 0.5
  for: 10m
  annotations:
    summary: "NAT traversal success rate below 50%"
```

**Excessive TURN Usage**
```yaml
- alert: ExcessiveTURNUsage
  expr: rate(dchat_nat_traversal_attempts_total{method="turn"}[5m]) > 0.8
  for: 5m
  annotations:
    summary: "Over 80% of connections using TURN relay (expensive fallback)"
```

## Future Enhancements

1. **Histogram Metrics**: Add connection duration histograms for latency analysis
2. **ASN Labels**: Add AS number labels to track NAT success by network provider
3. **Geographic Labels**: Add region labels to analyze NAT behavior by geography
4. **Peer Count Correlation**: Correlate NAT success rates with peer count metrics

## References

- PROBUS Audit: Task #2 (NAT Telemetry Prometheus Metrics)
- Original TODO: `dchat-network/src/network/nat_telemetry.rs:170-179`
- Prometheus Exporter: `dchat-observability/src/observability/metrics.rs`
- NAT Telemetry: `dchat-network/src/network/nat_telemetry.rs`
