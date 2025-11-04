/// Connection Lifecycle Management
///
/// This module manages the full lifecycle of peer connections:
/// 1. Connection Pool - Max 50 connections, target 30
/// 2. Health Monitoring - 30-second health checks
/// 3. Automatic Reconnection - Exponential backoff
/// 4. Pruning Strategies - LRU and reputation-based
///
/// See ARCHITECTURE.md Section 12: Network Resilience
use dchat_core::Result;
use libp2p::PeerId;
use std::time::Duration;

pub mod health;
pub mod pool;
pub mod reconnect;

pub use health::{HealthCheckResult, HealthMonitor, HealthStatus};
pub use pool::{ConnectionInfo, ConnectionPool, ConnectionState};
pub use reconnect::{BackoffStrategy, ReconnectManager, ReconnectPolicy};

/// Connection manager configuration
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Maximum number of connections
    pub max_connections: usize,

    /// Target number of connections to maintain
    pub target_connections: usize,

    /// Health check interval
    pub health_check_interval: Duration,

    /// Connection timeout
    pub connection_timeout: Duration,

    /// Idle connection timeout
    pub idle_timeout: Duration,

    /// Reconnect policy
    pub reconnect_policy: ReconnectPolicy,

    /// Enable connection metrics
    pub enable_metrics: bool,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_connections: 50,
            target_connections: 30,
            health_check_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300), // 5 minutes
            reconnect_policy: ReconnectPolicy::default(),
            enable_metrics: true,
        }
    }
}

/// Connection manager - orchestrates pool, health, and reconnection
pub struct ConnectionManager {
    config: ConnectionConfig,
    pool: ConnectionPool,
    health_monitor: HealthMonitor,
    reconnect_manager: ReconnectManager,
    metrics: ConnectionMetrics,
}

impl ConnectionManager {
    /// Create new connection manager
    pub fn new(config: ConnectionConfig) -> Self {
        let pool = ConnectionPool::new(config.max_connections, config.target_connections);
        let health_monitor = HealthMonitor::new(config.health_check_interval);
        let reconnect_manager = ReconnectManager::new(config.reconnect_policy.clone());

        Self {
            config,
            pool,
            health_monitor,
            reconnect_manager,
            metrics: ConnectionMetrics::new(),
        }
    }

    /// Add connection to pool
    pub async fn add_connection(&mut self, peer_id: PeerId) -> Result<()> {
        // Check if we're at capacity
        if self.pool.is_at_capacity() {
            // Try to prune low-priority connections
            self.prune_connections().await?;

            if self.pool.is_at_capacity() {
                return Err(dchat_core::Error::network("Connection pool at capacity"));
            }
        }

        // Add to pool
        self.pool.add_connection(peer_id).await?;

        // Start health monitoring
        self.health_monitor.monitor_peer(peer_id).await?;

        // Update metrics
        self.metrics.record_connection_added();

        Ok(())
    }

    /// Remove connection from pool
    pub async fn remove_connection(&mut self, peer_id: &PeerId) -> Result<()> {
        self.pool.remove_connection(peer_id).await?;
        self.health_monitor.stop_monitoring(peer_id).await?;
        self.reconnect_manager.cancel_reconnection(peer_id).await?;

        self.metrics.record_connection_removed();

        Ok(())
    }

    /// Handle connection failure
    pub async fn handle_connection_failure(&mut self, peer_id: &PeerId) -> Result<()> {
        // Remove from pool
        self.remove_connection(peer_id).await?;

        // Schedule reconnection
        if self.reconnect_manager.should_reconnect(peer_id) {
            self.reconnect_manager
                .schedule_reconnection(*peer_id)
                .await?;
        }

        self.metrics.record_connection_failure();

        Ok(())
    }

    /// Prune low-priority connections
    async fn prune_connections(&mut self) -> Result<()> {
        // Get pruning candidates
        let candidates = self.pool.get_pruning_candidates().await?;

        if candidates.is_empty() {
            return Ok(());
        }

        // Prune the lowest priority connection
        if let Some(peer_id) = candidates.first() {
            self.remove_connection(peer_id).await?;
            self.metrics.record_connection_pruned();
        }

        Ok(())
    }

    /// Run maintenance tasks
    pub async fn maintain(&mut self) -> Result<()> {
        // Run health checks
        let unhealthy = self.health_monitor.check_all().await?;

        for peer_id in unhealthy {
            self.handle_connection_failure(&peer_id).await?;
        }

        // Process reconnection queue
        let reconnects = self.reconnect_manager.get_due_reconnections().await?;

        for peer_id in reconnects {
            match self.add_connection(peer_id).await {
                Ok(_) => {
                    self.reconnect_manager
                        .mark_reconnection_success(&peer_id)
                        .await?;
                    self.metrics.record_reconnection_success();
                }
                Err(_) => {
                    self.reconnect_manager
                        .mark_reconnection_failure(&peer_id)
                        .await?;
                    self.metrics.record_reconnection_failure();
                }
            }
        }

        // Remove idle connections
        self.remove_idle_connections().await?;

        // Ensure we maintain target connections
        self.ensure_target_connections().await?;

        Ok(())
    }

    /// Remove idle connections
    async fn remove_idle_connections(&mut self) -> Result<()> {
        let idle = self
            .pool
            .get_idle_connections(self.config.idle_timeout)
            .await?;

        for peer_id in idle {
            self.remove_connection(&peer_id).await?;
            self.metrics.record_idle_connection_removed();
        }

        Ok(())
    }

    /// Ensure we maintain target number of connections
    async fn ensure_target_connections(&mut self) -> Result<()> {
        let current = self.pool.connection_count();

        if current < self.config.target_connections {
            // Signal to discovery system that we need more peers
            // (Implementation would integrate with DHT discovery)
        }

        Ok(())
    }

    /// Get connection statistics
    pub fn get_stats(&self) -> ConnectionStats {
        ConnectionStats {
            total_connections: self.pool.connection_count(),
            healthy_connections: self.health_monitor.healthy_count(),
            pending_reconnections: self.reconnect_manager.pending_count(),
            metrics: self.metrics.clone(),
        }
    }

    /// Get connection info for peer
    pub async fn get_connection_info(&self, peer_id: &PeerId) -> Option<ConnectionInfo> {
        self.pool.get_connection_info(peer_id).await
    }
}

/// Connection statistics
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub total_connections: usize,
    pub healthy_connections: usize,
    pub pending_reconnections: usize,
    pub metrics: ConnectionMetrics,
}

/// Connection metrics
#[derive(Debug, Clone)]
pub struct ConnectionMetrics {
    connections_added: u64,
    connections_removed: u64,
    connections_failed: u64,
    connections_pruned: u64,
    reconnections_attempted: u64,
    reconnections_succeeded: u64,
    reconnections_failed: u64,
    idle_connections_removed: u64,
}

impl ConnectionMetrics {
    fn new() -> Self {
        Self {
            connections_added: 0,
            connections_removed: 0,
            connections_failed: 0,
            connections_pruned: 0,
            reconnections_attempted: 0,
            reconnections_succeeded: 0,
            reconnections_failed: 0,
            idle_connections_removed: 0,
        }
    }

    fn record_connection_added(&mut self) {
        self.connections_added += 1;
    }

    fn record_connection_removed(&mut self) {
        self.connections_removed += 1;
    }

    fn record_connection_failure(&mut self) {
        self.connections_failed += 1;
    }

    fn record_connection_pruned(&mut self) {
        self.connections_pruned += 1;
    }

    fn record_reconnection_success(&mut self) {
        self.reconnections_attempted += 1;
        self.reconnections_succeeded += 1;
    }

    fn record_reconnection_failure(&mut self) {
        self.reconnections_attempted += 1;
        self.reconnections_failed += 1;
    }

    fn record_idle_connection_removed(&mut self) {
        self.idle_connections_removed += 1;
    }

    /// Get reconnection success rate
    pub fn reconnection_success_rate(&self) -> f64 {
        if self.reconnections_attempted == 0 {
            return 0.0;
        }
        self.reconnections_succeeded as f64 / self.reconnections_attempted as f64
    }

    /// Get connection stability (1.0 - failure_rate)
    pub fn connection_stability(&self) -> f64 {
        let total = self.connections_added + self.connections_failed;
        if total == 0 {
            return 1.0;
        }
        1.0 - (self.connections_failed as f64 / total as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_config_default() {
        let config = ConnectionConfig::default();
        assert_eq!(config.max_connections, 50);
        assert_eq!(config.target_connections, 30);
        assert_eq!(config.health_check_interval, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_connection_manager_creation() {
        let config = ConnectionConfig::default();
        let manager = ConnectionManager::new(config);

        let stats = manager.get_stats();
        assert_eq!(stats.total_connections, 0);
    }

    #[test]
    fn test_connection_metrics() {
        let mut metrics = ConnectionMetrics::new();

        metrics.record_connection_added();
        metrics.record_reconnection_success();

        assert_eq!(metrics.connections_added, 1);
        assert_eq!(metrics.reconnections_succeeded, 1);
        assert_eq!(metrics.reconnection_success_rate(), 1.0);
    }

    #[test]
    fn test_connection_stability() {
        let mut metrics = ConnectionMetrics::new();

        metrics.record_connection_added();
        metrics.record_connection_added();
        metrics.record_connection_added();
        metrics.record_connection_failure();

        // 3 added, 1 failed = 75% stability
        assert_eq!(metrics.connection_stability(), 0.75);
    }
}
