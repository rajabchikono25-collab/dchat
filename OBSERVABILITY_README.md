# dChat Observability Stack

Complete production-ready observability infrastructure for dChat decentralized chat network.

## Components

### Core Stack
- **Prometheus** - Metrics collection and alerting (port 9090)
- **Grafana** - Visualization dashboards (port 3000)
- **Alertmanager** - Alert routing and notifications (port 9093)
- **OpenTelemetry Collector** - Distributed tracing (ports 4317/4318)
- **Jaeger** - Trace visualization (port 16686)
- **Loki** - Log aggregation (port 3100)
- **Promtail** - Log shipping

### Exporters
- **Node Exporter** - System metrics (port 9100)
- **Redis Exporter** - Redis Cluster metrics (port 9121)

## Quick Start

### 1. Prerequisites

```bash
# Install Docker and Docker Compose
sudo apt-get update
sudo apt-get install docker.io docker-compose

# Clone the repository
cd /path/to/dchat
```

### 2. Configure Environment Variables

Create `.env` file in the dchat root directory:

```bash
# Grafana
GRAFANA_ADMIN_PASSWORD=your-secure-password

# Alertmanager
SLACK_WEBHOOK_URL=https://hooks.slack.com/services/YOUR/WEBHOOK/URL
PAGERDUTY_SERVICE_KEY=your-pagerduty-integration-key
EMAIL_PASSWORD=your-smtp-password

# OpenTelemetry
OTLP_API_KEY=your-otlp-backend-api-key

# Redis
REDIS_PASSWORD=your-redis-password
```

### 3. Start the Observability Stack

```bash
# Start all services
docker-compose -f docker-compose-observability.yml up -d

# Check service status
docker-compose -f docker-compose-observability.yml ps

# View logs
docker-compose -f docker-compose-observability.yml logs -f
```

### 4. Access Web UIs

- **Grafana**: http://localhost:3000 (default: admin/changeme)
- **Prometheus**: http://localhost:9090
- **Alertmanager**: http://localhost:9093
- **Jaeger**: http://localhost:16686

## Grafana Dashboards

Pre-built dashboards are located in `grafana/`:

1. **dchat-network-health.json** - Network peer status, latency, throughput
2. **dchat-relay-performance.json** - Relay uptime, queue depth, proof-of-delivery
3. **dchat-validator-status.json** - Block production, consensus participation, forks

### Import Dashboards

1. Open Grafana (http://localhost:3000)
2. Navigate to **Dashboards** → **Import**
3. Upload JSON files from `grafana/` directory
4. Configure Prometheus data source if prompted

## Prometheus Alerts

Alert rules are defined in `prometheus/alerts.yml`:

### Alert Categories

- **Network Alerts**: High latency, low peer count, connection failures
- **Relay Alerts**: Queue depth, uptime score, proof-of-delivery failures
- **Validator Alerts**: Missed blocks, low consensus participation, forks
- **System Alerts**: Node down, high memory/CPU usage, low disk space
- **Storage Alerts**: Database failures, low cache hit ratio, high latency

### Testing Alerts

```bash
# Manually fire a test alert
curl -X POST http://localhost:9090/api/v1/alerts
```

## Alertmanager Notification Channels

Configured receivers in `alertmanager/config.yml`:

- **Critical Alerts** → PagerDuty + Slack (#dchat-critical-alerts)
- **Network Team** → Slack (#dchat-network) + Email
- **Validator Team** → Slack (#dchat-validators) + Email
- **Relay Team** → Slack (#dchat-relays)
- **Storage Team** → Slack (#dchat-storage)
- **Operations Team** → Slack (#dchat-ops) + Email

### Customize Notification Routes

Edit `alertmanager/config.yml` and restart:

```bash
docker-compose -f docker-compose-observability.yml restart alertmanager
```

## OpenTelemetry Tracing

### Instrument Your Code

Add OpenTelemetry SDK to your Rust application:

```toml
# Cargo.toml
[dependencies]
opentelemetry = "0.21"
opentelemetry-otlp = "0.14"
tracing-opentelemetry = "0.22"
```

```rust
// src/main.rs
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::layer::SubscriberExt;

fn init_tracing() {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint("http://localhost:4317"),
        )
        .install_simple()
        .unwrap();

    let opentelemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    
    let subscriber = tracing_subscriber::registry()
        .with(opentelemetry_layer);
    
    tracing::subscriber::set_global_default(subscriber).unwrap();
}
```

### View Traces in Jaeger

1. Open Jaeger UI: http://localhost:16686
2. Select **dchat** service from dropdown
3. Search for traces by operation name, tags, or duration

## Log Aggregation with Loki

### Query Logs in Grafana

1. Open Grafana → **Explore**
2. Select **Loki** data source
3. Use LogQL queries:

```logql
# All dchat logs
{job="dchat"}

# Validator errors
{component="validator"} |= "error"

# Consensus events
{job="consensus"} | json | height > 1000

# High-latency network events
{job="dchat"} | json | latency_ms > 500
```

### Log Retention

Default retention: **30 days** (configured in `loki/config.yml`)

To change:

```yaml
# loki/config.yml
table_manager:
  retention_period: 720h  # 30 days
```

## Metrics Reference

### Network Metrics

- `dchat_network_peers{state="connected"}` - Number of connected peers
- `dchat_network_latency_seconds` - Network round-trip time (histogram)
- `dchat_messages_sent_total` - Total messages sent (counter)
- `dchat_network_connection_failures_total` - Failed connection attempts (counter)
- `dchat_dht_routing_table_size` - DHT routing table entries (gauge)

### Relay Metrics

- `dchat_relay_messages_routed_total` - Messages routed by relay (counter)
- `dchat_relay_uptime_score` - Relay uptime percentage (gauge)
- `dchat_relay_queue_depth` - Current message queue depth (gauge)
- `dchat_relay_proof_of_delivery_total` - Proof-of-delivery submissions (counter)
- `dchat_relay_reputation_score` - Relay reputation (gauge)

### Validator Metrics

- `dchat_validator_blocks_produced_total` - Blocks produced (counter)
- `dchat_chain_block_height` - Current block height (gauge)
- `dchat_validator_consensus_participation_rate` - Participation % (gauge)
- `dchat_validator_missed_blocks_total` - Missed blocks (counter)
- `dchat_consensus_round_duration_seconds` - Consensus round time (histogram)

### Storage Metrics

- `dchat_storage_database_health` - Database health status (0/1)
- `dchat_cache_hits_total` - Cache hits (counter)
- `dchat_cache_misses_total` - Cache misses (counter)
- `dchat_storage_operation_duration_seconds` - Storage op latency (histogram)

## Troubleshooting

### Prometheus Not Scraping Targets

1. Check target status: http://localhost:9090/targets
2. Verify network connectivity:
   ```bash
   docker exec dchat-prometheus curl -v http://validator1-ohio.schikuno.top:9090/metrics
   ```
3. Check Prometheus logs:
   ```bash
   docker logs dchat-prometheus
   ```

### Grafana Dashboards Show No Data

1. Verify Prometheus data source configuration in Grafana
2. Check Prometheus has data: http://localhost:9090/graph
3. Ensure time range in Grafana matches data availability

### Alerts Not Firing

1. Check alerting rules are loaded:
   ```bash
   curl http://localhost:9090/api/v1/rules
   ```
2. Verify Alertmanager connectivity:
   ```bash
   curl http://localhost:9093/-/healthy
   ```
3. Check alert inhibition rules in Alertmanager config

### OpenTelemetry Not Receiving Traces

1. Verify OTel Collector health:
   ```bash
   curl http://localhost:13133/
   ```
2. Check OTel Collector logs:
   ```bash
   docker logs dchat-otel-collector
   ```
3. Ensure application is using correct endpoint (localhost:4317)

## Production Deployment

### High Availability

For production, deploy multiple instances with shared storage:

```yaml
# prometheus/prometheus.yml
remote_write:
  - url: 'https://prometheus-ha-cluster.example.com/api/v1/write'
```

### Security

1. Enable TLS for all services
2. Configure authentication for Grafana, Prometheus, Jaeger
3. Use secrets management for API keys and passwords
4. Restrict network access with firewall rules

### Scaling

- **Prometheus**: Use federation or remote write to Thanos/Cortex
- **Loki**: Deploy in microservices mode with S3/GCS backend
- **Jaeger**: Use Elasticsearch or Cassandra for storage
- **Grafana**: Deploy behind load balancer with shared database

## Maintenance

### Backup Configuration

```bash
# Backup all observability configs
tar -czf observability-backup-$(date +%Y%m%d).tar.gz \
  grafana/ prometheus/ alertmanager/ opentelemetry/ loki/ promtail/
```

### Update Services

```bash
# Pull latest images
docker-compose -f docker-compose-observability.yml pull

# Restart services
docker-compose -f docker-compose-observability.yml up -d
```

### Clean Up Old Data

```bash
# Remove old Prometheus data (older than 90 days is auto-deleted)
docker exec dchat-prometheus find /prometheus -mtime +90 -delete

# Compact Loki data
docker exec dchat-loki /usr/bin/loki -config.file=/etc/loki/local-config.yaml -target=compactor
```

## Support

- **Documentation**: https://docs.dchat.network/observability
- **Issues**: https://github.com/dchat/dchat/issues
- **Slack**: #dchat-observability

## License

Same as dChat project (see root LICENSE file)
